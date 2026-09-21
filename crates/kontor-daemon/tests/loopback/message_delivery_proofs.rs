//! Legacy acknowledgement recovery never sends; it persists one bounded proof page.
use super::*;
use kontor_runtime::capability::RuntimeBindingSnapshot;
use kontor_runtime::request::MessageId;
use kontor_store::MessageDeliveryProof;

async fn fixture(
    length: usize,
    candidate_at: usize,
) -> (World, AgentRunId, RuntimeBindingSnapshot, MessageId) {
    let world = World::open().await;
    let (run, snapshot) = world.launch().await;
    let held = world
        .daemon
        .state()
        .sessions()
        .get(snapshot.binding_id())
        .unwrap();
    let anchor = MessageId::generate();
    issued_turn(&world, &held, anchor);
    let message = MessageId::generate();
    issue_message(&world, &held, message);
    while world.fake.content(&held).len() < candidate_at - 1 {
        world
            .fake
            .observe_trailing_tool_call(&held, kontor_api::now())
            .unwrap();
    }
    world
        .fake
        .observe_turn_completion(&held, message, kontor_api::now())
        .unwrap();
    while world.fake.content(&held).len() < length {
        world
            .fake
            .observe_trailing_tool_call(&held, kontor_api::now())
            .unwrap();
    }
    world.fake.take_calls();
    (world, run, held, message)
}

async fn step(
    world: &World,
    run: AgentRunId,
    message: MessageId,
    revision: u64,
    key: &str,
) -> harness::Answer {
    Call::post(
        format!("/v1/sessions/{run}/messages:reconcile"),
        &serde_json::json!({"message_id": message.to_string(), "expected_revision": revision}),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await
}

fn proof(world: &World, message: MessageId) -> MessageDeliveryProof {
    world
        .daemon
        .state()
        .with_store(|store| store.message_delivery_proof(&message.to_string()))
        .unwrap()
        .unwrap()
}

fn unknown(world: &World, message: MessageId) {
    assert_eq!(
        world
            .daemon
            .state()
            .message_issuance(message)
            .unwrap()
            .unwrap()
            .delivered_at,
        None
    );
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, AdapterCall::Send(..) | AdapterCall::Resume(..))),
        "proof is read-only against native sessions: {:?}",
        world.fake.calls()
    );
}

async fn finish(world: &World, run: AgentRunId, message: MessageId) -> MessageDeliveryProof {
    for _ in 0..100 {
        let current = proof(world, message);
        if current.state != "scanning" {
            return current;
        }
        let answer = step(
            world,
            run,
            message,
            current.revision,
            &format!("page-{}", current.revision),
        )
        .await;
        assert_eq!(answer.status, 200, "{}", answer.body);
    }
    panic!("bounded pages did not reach the frozen upper bound")
}

#[tokio::test]
async fn legacy_message_proof_survives_restart_replays_and_long_history_without_dispatch() {
    let (world, run, held, message) = fixture(3000, 130).await;
    let seed = step(&world, run, message, 0, "start").await;
    assert_eq!(seed.status, 200, "{}", seed.body);
    let initial = proof(&world, message);
    assert_eq!(
        (
            initial.revision,
            initial.through_sequence,
            initial.upper_sequence
        ),
        (1, 0, 3000)
    );
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, AdapterCall::History(..)))
    );
    let page = step(&world, run, message, 1, "page-one").await;
    assert_eq!(page.status, 200, "{}", page.body);
    let checkpoint = proof(&world, message);
    assert_eq!(checkpoint.through_sequence, 64);
    unknown(&world, message);
    for (revision, key) in [(2, "page-two"), (3, "page-three")] {
        let answer = step(&world, run, message, revision, key).await;
        assert_eq!(answer.status, 200, "{}", answer.body);
    }
    let checkpoint = proof(&world, message);
    assert_eq!(checkpoint.through_sequence, 192);
    assert_eq!(checkpoint.occurrences, 1);
    assert_eq!(checkpoint.candidate_sequence, Some(130));
    assert!(checkpoint.candidate_body_hash.is_some());
    // Advancing native content cannot move the proof's original endpoint.
    world
        .fake
        .observe_trailing_tool_call(&held, kontor_api::now())
        .unwrap();
    let World {
        directory,
        daemon,
        router,
        fake,
        project,
        task,
        team_run,
    } = world;
    daemon.state().signals().stop();
    drop(router);
    drop(daemon);
    fake.rebuild_adapter_state();
    fake.forget_timeline_epochs();
    let daemon = Daemon::start(
        DaemonConfig::at(directory.path()).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .unwrap();
    assert_eq!(daemon.reconcile().await, BarrierState::Open);
    let router = daemon.router();
    let world = World {
        directory,
        daemon,
        router,
        fake,
        project,
        task,
        team_run,
    };
    world.fake.take_calls();
    let replay = step(&world, run, message, 1, "page-one").await;
    assert_eq!(
        replay.json(),
        page.json(),
        "lost acknowledgement replays the same page receipt"
    );
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, AdapterCall::History(..) | AdapterCall::TailWindow(..)))
    );
    assert_eq!(proof(&world, message), checkpoint);
    let stale = step(&world, run, message, 1, "stale").await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    let changed = step(&world, run, message, 2, "page-one").await;
    assert_eq!(changed.status, 409, "{}", changed.body);
    let complete = finish(&world, run, message).await;
    assert_eq!(
        (
            complete.state.as_str(),
            complete.upper_sequence,
            complete.through_sequence,
            complete.occurrences
        ),
        ("confirmed", 3000, 3000, 1)
    );
    assert_eq!(complete.candidate_sequence, Some(130));
    assert_eq!(complete.anchor, initial.anchor);
    assert_eq!(
        world
            .daemon
            .state()
            .message_issuance(message)
            .unwrap()
            .unwrap()
            .delivered_at,
        Some((initial.anchor.epoch, 130))
    );
    assert_eq!(
        world
            .daemon
            .state()
            .message_issuance(message)
            .unwrap()
            .unwrap()
            .boundary_at,
        None,
        "legacy issuance never receives a fabricated floor"
    );
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, AdapterCall::Send(..) | AdapterCall::Resume(..)))
    );
    world.fake.accept_duplicate_native_message_ids();
    world.fake.take_calls();
    let changed_first = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body":"different instruction"}),
    )
    .signed_as(&world, "operator")
    .with_key(message.to_string())
    .send(&world)
    .await;
    assert_eq!(changed_first.status, 409, "{}", changed_first.body);
    let replay = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body":"current turn request"}),
    )
    .signed_as(&world, "operator")
    .with_key(message.to_string())
    .send(&world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["value"]["sequence"], 130);
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, AdapterCall::Send(..))),
        "normal delivery replays the durable proof before adapter send"
    );
    let changed_body = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body":"different instruction"}),
    )
    .signed_as(&world, "operator")
    .with_key(message.to_string())
    .send(&world)
    .await;
    assert_eq!(changed_body.status, 409, "{}", changed_body.body);
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, AdapterCall::Send(..)))
    );

    let get = Call::get(format!(
        "/v1/sessions/{run}/messages/proof?message_id={message}"
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(get.status, 200, "{}", get.body);
    assert!(get.body.contains("confirmed"));
}

#[tokio::test]
async fn legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery() {
    let (world, run, held, message) = fixture(400, 30).await;
    world
        .fake
        .observe_turn_completion(&held, message, kontor_api::now())
        .unwrap();
    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
    let completed = finish(&world, run, message).await;
    assert_eq!(completed.state, "duplicate");
    assert_eq!(completed.occurrences, 2);
    assert_eq!(completed.candidate_sequence, Some(30));
    unknown(&world, message);
}

#[tokio::test]
async fn legacy_message_proof_absence_never_authorizes_dispatch() {
    let (world, run, held, message) = fixture(130, 30).await;
    let mut content = world.fake.content(&held);
    content[29].subject = kontor_runtime::timeline::EventSubject::None;
    world.fake.replace_recorded_history(&held, content).unwrap();
    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
    assert_eq!(finish(&world, run, message).await.state, "absent");
    unknown(&world, message);
}

#[tokio::test]
async fn legacy_message_proof_lost_history_or_changed_epoch_keeps_its_original_checkpoint() {
    for failure in ["epoch", "gap", "overlap", "early_end", "anchor", "upper"] {
        let (world, run, held, message) = fixture(130, 30).await;
        assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
        let original = proof(&world, message);
        let mut content = world.fake.content(&held);
        match failure {
            "epoch" => {
                world.fake.set_unread_timeline_epoch(&held, 99).unwrap();
            }
            "gap" => {
                content.remove(10);
            }
            "overlap" => {
                content.insert(10, content[9].clone());
            }
            "early_end" => {
                content.truncate(40);
            }
            "anchor" => {
                content[0].subject = kontor_runtime::timeline::EventSubject::None;
            }
            "upper" => {
                content[129].subject =
                    kontor_runtime::timeline::EventSubject::Message(MessageId::generate());
            }
            _ => unreachable!(),
        }
        if failure != "epoch" {
            world.fake.replace_recorded_history(&held, content).unwrap();
        }
        let mut checkpoint = original;
        let mut refused = false;
        for _ in 0..5 {
            let answer = step(
                &world,
                run,
                message,
                checkpoint.revision,
                &format!("{failure}-{}", checkpoint.revision),
            )
            .await;
            if answer.status == 409 {
                assert_eq!(
                    proof(&world, message),
                    checkpoint,
                    "refusal retains exact progress: {failure}"
                );
                refused = true;
                break;
            }
            assert_eq!(answer.status, 200, "{}", answer.body);
            checkpoint = proof(&world, message);
        }
        assert!(refused, "missing refusal for {failure}");
        unknown(&world, message);
    }
}

#[tokio::test]
async fn legacy_message_proof_candidate_rewrite_is_detected_before_confirmation() {
    for changed in ["candidate", "anchor"] {
        let (world, run, held, message) = fixture(130, 30).await;
        assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
        assert_eq!(step(&world, run, message, 1, "first").await.status, 200);
        assert_eq!(proof(&world, message).candidate_sequence, Some(30));
        let mut content = world.fake.content(&held);
        if changed == "candidate" {
            content[29].payload = kontor_core::id::CanonicalDocument::from_value(
                &serde_json::json!({"schema_version":1,"body":"rewritten"}),
            )
            .unwrap();
        } else {
            content[0].subject = kontor_runtime::timeline::EventSubject::None;
        }
        world.fake.replace_recorded_history(&held, content).unwrap();
        assert_eq!(step(&world, run, message, 2, "second").await.status, 200);
        let checkpoint = proof(&world, message);
        let final_page = step(&world, run, message, 3, "final").await;
        assert_eq!(final_page.status, 409, "{}", final_page.body);
        assert_eq!(proof(&world, message), checkpoint);
        unknown(&world, message);
    }
}

#[tokio::test]
async fn legacy_message_proof_failed_receipt_rolls_back_delivery_and_progress() {
    let (world, run, _held, message) = fixture(50, 30).await;
    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
    let checkpoint = proof(&world, message);
    let connection =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    connection.execute_batch("CREATE TRIGGER refuse_proof_receipt BEFORE INSERT ON runtime_message_delivery_proof_steps BEGIN SELECT RAISE(ABORT, 'injected receipt failure'); END;").unwrap();
    let failed = step(&world, run, message, 1, "finish").await;
    assert!(!failed.status.is_success(), "{}", failed.body);
    assert_eq!(proof(&world, message), checkpoint);
    unknown(&world, message);
    connection
        .execute_batch("DROP TRIGGER refuse_proof_receipt;")
        .unwrap();
    assert_eq!(step(&world, run, message, 1, "finish").await.status, 200);
    assert_eq!(proof(&world, message).state, "confirmed");
}

#[tokio::test]
async fn legacy_message_proof_requires_real_prior_evidence_and_exact_binding() {
    let world = World::open().await;
    let (run, snapshot) = world.launch().await;
    let held = world
        .daemon
        .state()
        .sessions()
        .get(snapshot.binding_id())
        .unwrap();
    let message = MessageId::generate();
    issue_message(&world, &held, message);
    world
        .fake
        .observe_turn_completion(&held, message, kontor_api::now())
        .unwrap();
    world.fake.take_calls();
    let refused = step(&world, run, message, 0, "no-anchor").await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.message_delivery_proof(&message.to_string()))
            .unwrap()
            .is_none()
    );
    unknown(&world, message);
    let forbidden = Call::post(
        format!("/v1/sessions/{run}/messages:reconcile"),
        &serde_json::json!({"message_id":message.to_string(),"expected_revision":0}),
    )
    .signed_as(&world, "observer")
    .with_key("observer")
    .send(&world)
    .await;
    assert_eq!(forbidden.status, 403, "{}", forbidden.body);
    let foreign = MessageId::generate();
    world
        .daemon
        .state()
        .record_message_issuance(
            held.identity(),
            RuntimeBindingId::generate(),
            foreign,
            "session_message_send",
            &foreign.to_string(),
            None,
        )
        .unwrap();
    world.fake.take_calls();
    let wrong_binding = step(&world, run, foreign, 0, "wrong-binding").await;
    assert_eq!(wrong_binding.status, 409, "{}", wrong_binding.body);
    assert_eq!(wrong_binding.code(), "stale_binding");
    assert!(world.fake.calls().iter().all(|call| !matches!(
        call,
        AdapterCall::History(..) | AdapterCall::TailWindow(..) | AdapterCall::Send(..)
    )));
}

#[tokio::test]
async fn legacy_message_proof_store_guards_identity_progress_and_stale_writers() {
    let (world, run, _held, message) = fixture(130, 30).await;
    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
    let original = proof(&world, message);
    let issued = world
        .daemon
        .state()
        .message_issuance(message)
        .unwrap()
        .unwrap();
    for flaw in [
        "revision",
        "upper",
        "anchor",
        "generation",
        "digest",
        "rewind",
        "verdict",
    ] {
        let mut next = original.clone();
        next.revision += 1;
        next.through_sequence = 1;
        next.anchor_seen = true;
        let expected = if flaw == "revision" {
            0
        } else {
            original.revision
        };
        match flaw {
            "upper" => next.upper_sequence += 1,
            "anchor" => next.anchor.sequence += 1,
            "generation" => next.runtime_generation += 1,
            "digest" => next.issuance_hash = "f".repeat(64),
            "rewind" => next.through_sequence = 0,
            "verdict" => next.state = "confirmed".to_owned(),
            _ => {}
        }
        let result = world.daemon.state().with_store(|store| {
            store.advance_message_delivery_proof(&issued, &next, expected, flaw, &"a".repeat(64))
        });
        assert!(result.is_err(), "store admitted {flaw}");
        assert_eq!(proof(&world, message), original);
        unknown(&world, message);
    }
}

#[tokio::test]
async fn legacy_message_proof_negative_or_incomplete_evidence_blocks_normal_delivery_replay() {
    let (world, run, held, message) = fixture(130, 30).await;
    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
    let mut content = world.fake.content(&held);
    content[29].subject = kontor_runtime::timeline::EventSubject::None;
    world.fake.replace_recorded_history(&held, content).unwrap();
    for expected in ["scanning", "absent"] {
        assert_eq!(proof(&world, message).state, expected);
        let request = kontor_runtime::request::SendMessageRequest {
            binding: held.clone(),
            message_id: message,
            body: kontor_core::id::BoundedText::parse("current turn request").unwrap(),
            sent_at: kontor_api::now(),
        };
        let replay = world
            .daemon
            .state()
            .send_issued_message(world.fake.as_ref(), &request)
            .await;
        assert!(
            replay.is_err(),
            "{expected} proof must not fall through to native send"
        );
        unknown(&world, message);
        if expected == "scanning" {
            assert_eq!(finish(&world, run, message).await.state, "absent");
        }
    }
}

#[tokio::test]
async fn legacy_message_proof_database_keeps_original_candidate_and_counts_immutable() {
    let (world, run, _held, message) = fixture(130, 30).await;
    assert_eq!(step(&world, run, message, 0, "start").await.status, 200);
    assert_eq!(step(&world, run, message, 1, "first").await.status, 200);
    let original = proof(&world, message);
    assert_eq!(original.candidate_sequence, Some(30));
    let connection =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    for (field, value) in [
        ("candidate_sequence", serde_json::json!(31)),
        ("candidate_hash", serde_json::json!("f".repeat(64))),
        ("candidate_body_hash", serde_json::json!("f".repeat(64))),
        (
            "candidate_accepted_at",
            serde_json::json!("2026-01-01T00:00:00Z"),
        ),
        ("occurrences", serde_json::json!(0)),
        ("anchor_seen", serde_json::json!(false)),
    ] {
        let mut document = serde_json::to_value(&original).unwrap();
        document["revision"] = serde_json::json!(original.revision + 1);
        document["through_sequence"] = serde_json::json!(original.through_sequence + 1);
        document[field] = value;
        let canonical = kontor_core::id::CanonicalDocument::from_value(&document).unwrap();
        let result = connection.execute("UPDATE runtime_message_delivery_proofs SET revision = revision + 1, proof_json = ?2, proof_hash = ?3 WHERE message_id = ?1",
            rusqlite::params![message.to_string(), canonical.json(), canonical.hash().as_str()]);
        assert!(
            result.is_err(),
            "the immutable proof field {field} was overwritten"
        );
        assert_eq!(proof(&world, message), original);
    }
    unknown(&world, message);
}
