//! The loopback contract, black box, against a real Realm and a scripted runtime.
//!
//! Every test here drives the same `axum::Router` the binary serves, over a real
//! state root with a real lock, a real credential file and a real migrated
//! database. Nothing binds a socket, launches the daemon binary or spawns a child
//! process (TST-001).
//!
//! The mutants this suite exists to kill:
//!
//! * binding anything but loopback, or letting a flag widen it;
//! * two daemons on one state root, or two state roots sharing a Realm identity;
//! * reaching a handler without a credential, from a hostile `Host`, or from an
//!   origin this Realm was never configured for;
//! * an answer, a receipt or an SSE frame that does not name its Realm;
//! * resolving a run, task or session from another Realm;
//! * an observer writing, or an operator reaching an admin route;
//! * dispatching a runtime operation the binding's *frozen* snapshot does not
//!   cover;
//! * a replayed command writing a second receipt, or a reused key silently
//!   succeeding;
//! * mutating on a stale revision, or refusing one without saying what the current
//!   revision is;
//! * a durable feed that skips, repeats, or cannot be resumed from a persisted
//!   position after a restart;
//! * scheduling before reconciliation finished;
//! * a lost acknowledgement producing a second native effect;
//! * continuing a session stream across an epoch change or a sequence gap;
//! * an unsupported operation producing a runtime effect anyway;
//! * a secret, a runtime endpoint or a transcript reaching the database, the
//!   contract document, a response or a stored event row.

mod harness;

use harness::{Call, World, at, capabilities_without, fake_family, name, secret};
use kontor_api::state::BarrierState;
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{AgentRunId, CanonicalDocument, ProjectId, TaskId};
use kontor_core::repository::{
    NewObservation, NewProject, NewRuntimeEvent, ProjectRepository, RealmRepository, RunRepository,
};
use kontor_core::state::{Freshness, ObservedRunState, RuntimeContact};
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_runtime::capability::RuntimeCapability;
use kontor_runtime::fake::{AdapterCall, RequestKey, ScriptStep};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A session with four recorded items and two more waiting to be streamed.
const HISTORY_LIVE: &str = r#"{
  "history": [
    {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "one"},
    {"kind": "tool_call", "sequence": 2, "emitted_at": "2026-08-10T09:02:00Z", "body": "two"},
    {"kind": "message", "sequence": 3, "emitted_at": "2026-08-10T09:03:00Z", "body": "three"},
    {"kind": "log", "sequence": 4, "emitted_at": "2026-08-10T09:04:00Z", "body": "four"}
  ],
  "live": [
    {"kind": "message", "sequence": 5, "emitted_at": "2026-08-10T09:05:00Z", "body": "five"},
    {"kind": "message", "sequence": 6, "emitted_at": "2026-08-10T09:06:00Z", "body": "six"}
  ]
}"#;

/// A live stream whose second frame renumbers the session's content.
const EPOCH_CHANGE: &str = r#"{
  "history": [
    {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "one"}
  ],
  "live": [
    {"kind": "message", "sequence": 2, "emitted_at": "2026-08-10T09:02:00Z", "body": "two"},
    {"kind": "message", "sequence": 3, "epoch": 9, "emitted_at": "2026-08-10T09:03:00Z", "body": "renumbered"}
  ]
}"#;

/// A session waiting on one permission request.
const PERMISSION_WAIT: &str = r#"{
  "history": [
    {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "working"},
    {"kind": "permission_request", "sequence": 2, "permission_id": "perm-1",
     "emitted_at": "2026-08-10T09:02:00Z", "body": "may i"}
  ]
}"#;

/// One canonical command body against a task, which is a witness-rule target.
fn resume_task_body(world: &World, revision: u64) -> serde_json::Value {
    serde_json::json!({
        "project_id": world.project.to_string(),
        "target": {"kind": "task", "task_id": world.task.to_string()},
        "expected_revision": revision,
        "desired_state": serde_json::Value::Null,
        "intent": {"schema_version": 1, "marker": "resume"},
        "payload": {"schema_version": 1, "marker": "resume-payload"},
    })
}

/// Append one control-plane observation, the way a reconciliation or an adapter
/// would, so the durable feed has something real to deliver.
fn observe(world: &World, run: AgentRunId, sequence: u64, revision: u64) {
    let identity = world.daemon.state().with_store(|store| {
        store
            .snapshot_run_inspection(run)
            .expect("the run reads back")
            .open(world.realm_id())
            .expect("our own realm")
            .expect("the run exists")
            .run
            .binding
            .expect("the run is bound")
            .identity
    });
    world.daemon.state().with_store(|store| {
        store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: world.project,
                    agent_run_id: run,
                    identity: identity.clone(),
                    native_event_id: None,
                    native_sequence: sequence,
                    // Control metadata only: the store's positive allowlist is what
                    // makes a transcript impossible here, and this payload is built
                    // to satisfy it rather than to work around it.
                    payload: CanonicalDocument::from_value(&serde_json::json!({
                        "schema_version": 1,
                        "native_sequence": sequence,
                        "observed_state": "running"
                    }))
                    .expect("control metadata"),
                    observed_at: at("2026-08-10T09:10:00Z"),
                },
                observed: ObservedRunState::Running,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: kontor_core::id::AggregateRevision::parse(revision)
                    .expect("a positive revision"),
            })
            .expect("the observation is recorded");
    });
    world.daemon.state().signals().appended();
}

// ---------------------------------------------------------------------------
// Bind, lock and realm isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_non_loopback_bind_is_refused_and_leaves_nothing_behind() {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let refused = Daemon::start(
        DaemonConfig::at(directory.path()).with_bind("0.0.0.0:7717".parse().expect("an address")),
        RuntimeRegistry::new(),
    );
    assert!(
        refused.is_err(),
        "a wildcard bind is not loopback and must be refused"
    );
    assert!(
        std::fs::read_dir(directory.path())
            .expect("the directory is readable")
            .next()
            .is_none(),
        "a refused configuration must not leave a lock, a database or credentials behind"
    );
}

#[tokio::test]
async fn a_second_daemon_on_one_state_root_fails_and_two_roots_are_two_realms() {
    let first = World::open().await;
    let second = Daemon::start(
        DaemonConfig::at(first.directory.path()).with_port(0),
        RuntimeRegistry::new(),
    );
    assert!(
        second.is_err(),
        "one state root holds one daemon: the second must fail cleanly"
    );

    let other = World::open().await;
    assert_ne!(
        first.realm_id(),
        other.realm_id(),
        "two state roots are two realms"
    );
    // Isolation is not a filter, it is an absence: the other realm's run has no
    // row here, so it cannot resolve however it is addressed.
    let (run, _) = other.launch().await;
    let answer = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&first, "observer")
        .send(&first)
        .await;
    assert_eq!(answer.status, 404);
    assert_eq!(answer.code(), "not_found");
    assert_eq!(
        answer.realm(),
        first.realm_id(),
        "a refusal names the realm that refused, not the one that was asked about"
    );
}

// ---------------------------------------------------------------------------
// Ingress
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_missing_or_wrong_credential_is_refused() {
    let world = World::open().await;
    let anonymous = Call::get("/v1/health").anonymous().send(&world).await;
    assert_eq!(anonymous.status, 401);
    assert_eq!(anonymous.code(), "unauthenticated");

    let wrong = Call::get("/v1/health")
        .with_token("not-this-realms-secret")
        .send(&world)
        .await;
    assert_eq!(wrong.status, 401);
    assert_eq!(wrong.code(), "unauthenticated");

    // Another Realm's real secret is still not this Realm's.
    let other = World::open().await;
    let foreign = Call::get("/v1/health")
        .with_token(secret(&other, "admin"))
        .send(&world)
        .await;
    assert_eq!(foreign.status, 401);
}

#[tokio::test]
async fn a_malformed_host_never_reaches_a_handler() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let before = world.fake.calls().len();

    // Each of these parses as an authority whose host part is loopback, and each
    // was accepted before the ingress reassembled what it had actually parsed.
    for host in [
        "localhost:bad",
        "localhost:",
        "127.0.0.1:x",
        "127.0.0.1:65536",
        "[::1]junk",
        "evil@127.0.0.1",
        "evil@localhost:7717",
        "127.0.0.1.evil.com",
        "2130706433",
        "[::1",
    ] {
        // A read route and a session route, so the refusal is the ingress's and
        // not one handler's.
        for uri in [
            "/v1/health".to_owned(),
            format!("/v1/sessions/{run}/timeline"),
        ] {
            let answer = Call::get(&uri)
                .signed_as(&world, "admin")
                .claiming_host(host)
                .send(&world)
                .await;
            assert_eq!(
                answer.status, 403,
                "Host `{host}` reached {uri}: {}",
                answer.body
            );
            assert_eq!(answer.code(), "forbidden");
        }
    }
    assert!(
        world.fake.calls().len() == before,
        "a refused Host must not have reached a runtime"
    );
}

#[tokio::test]
async fn a_hostile_host_or_origin_is_refused_before_any_handler() {
    let world = World::open().await;
    for host in ["kontor.example.com", "10.0.0.4:7717", "0.0.0.0:7717"] {
        let answer = Call::get("/v1/health")
            .signed_as(&world, "observer")
            .claiming_host(host)
            .send(&world)
            .await;
        assert_eq!(answer.status, 403, "{host} must be refused");
        assert_eq!(answer.code(), "forbidden");
    }
    let hostile_origin = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .claiming_origin("https://evil.example")
        .send(&world)
        .await;
    assert_eq!(hostile_origin.status, 403);
    assert_eq!(hostile_origin.code(), "forbidden");

    let configured = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .claiming_origin("tauri://localhost")
        .send(&world)
        .await;
    assert_eq!(configured.status, 200);
}

// ---------------------------------------------------------------------------
// Realm qualification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_answer_a_receipt_and_every_frame_name_the_realm() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;

    for uri in [
        "/v1/health".to_owned(),
        "/v1/realm".to_owned(),
        format!("/v1/runs/{run}"),
        format!("/v1/projects/{}/tasks/{}", world.project, world.task),
    ] {
        let answer = Call::get(&uri)
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(answer.status, 200, "{uri} answers: {}", answer.body);
        assert_eq!(answer.realm(), world.realm_id(), "{uri} names its realm");
    }

    let receipt = Call::post("/v1/commands/resume_task", &resume_task_body(&world, 1))
        .signed_as(&world, "operator")
        .with_key("realm-qualified-1")
        .send(&world)
        .await;
    assert_eq!(receipt.status, 200, "{}", receipt.body);
    assert_eq!(
        receipt.realm(),
        world.realm_id(),
        "a receipt is realm-qualified"
    );

    let timeline = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(timeline.status, 200, "{}", timeline.body);
    assert_eq!(timeline.realm(), world.realm_id());

    // Every SSE frame, on both cursor spaces.
    observe(&world, run, 1, 1);
    world.daemon.state().signals().stop();
    let feed = Call::get("/v1/events")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let frames = feed.frames();
    assert!(!frames.is_empty(), "the feed delivered the stored event");
    for (_, _, data) in &frames {
        assert_eq!(
            data.get("realm_id").and_then(serde_json::Value::as_str),
            Some(world.realm_id().to_string().as_str()),
            "every control-plane frame names the realm"
        );
    }

    let anchor = timeline.json()["anchor"]
        .as_str()
        .expect("an anchor")
        .to_owned();
    let stream = Call::get(format!("/v1/sessions/{run}/stream?after={anchor}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let content = stream.frames();
    assert!(!content.is_empty(), "the live stream delivered content");
    for (_, _, data) in &content {
        assert_eq!(
            data.get("realm_id").and_then(serde_json::Value::as_str),
            Some(world.realm_id().to_string().as_str()),
            "every session-content frame names the realm"
        );
    }
}

// ---------------------------------------------------------------------------
// Authority
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_authority_tiers_are_enforced_per_route() {
    let world = World::open().await;

    // An observer reads.
    assert_eq!(
        Call::get("/v1/health")
            .signed_as(&world, "observer")
            .send(&world)
            .await
            .status,
        200
    );
    // An observer does not write.
    let refused = Call::post("/v1/commands/resume_task", &resume_task_body(&world, 1))
        .signed_as(&world, "observer")
        .with_key("observer-may-not-write")
        .send(&world)
        .await;
    assert_eq!(refused.status, 403);
    assert_eq!(refused.code(), "forbidden");

    // An operator writes an ordinary control-plane command.
    assert_eq!(
        Call::post("/v1/commands/resume_task", &resume_task_body(&world, 1))
            .signed_as(&world, "operator")
            .with_key("operator-writes")
            .send(&world)
            .await
            .status,
        200
    );

    // Authority over who may act at all is the admin tier's, and an operator does
    // not reach it.
    let authorize = serde_json::json!({
        "project_id": world.project.to_string(),
        "target": {"kind": "task", "task_id": world.task.to_string()},
        "expected_revision": 1,
        "desired_state": serde_json::Value::Null,
        "intent": {"schema_version": 1, "marker": "authorize"},
        "payload": {"schema_version": 1, "marker": "authorize-payload"},
    });
    let operator = Call::post("/v1/commands/authorize_execution", &authorize)
        .signed_as(&world, "operator")
        .with_key("operator-may-not-authorize")
        .send(&world)
        .await;
    assert_eq!(operator.status, 403);
    assert_eq!(operator.code(), "forbidden");

    let admin = Call::post("/v1/commands/authorize_execution", &authorize)
        .signed_as(&world, "admin")
        .with_key("admin-authorizes")
        .send(&world)
        .await;
    assert_eq!(admin.status, 200, "{}", admin.body);
}

// ---------------------------------------------------------------------------
// Frozen capabilities
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unsupported_session_operation_has_zero_runtime_effect() {
    let world = World::open_with(capabilities_without(&[
        RuntimeCapability::History,
        RuntimeCapability::LiveEvents,
        RuntimeCapability::SendMessage,
        RuntimeCapability::PermissionResponse,
    ]))
    .await;
    let (run, _) = world.launch().await;
    let before = world.fake.calls().len();

    let timeline = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(timeline.status, 422);
    assert_eq!(timeline.code(), "unsupported_capability");

    let stream = Call::get(format!("/v1/sessions/{run}/stream?after=whatever"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(stream.status, 422);
    assert_eq!(stream.code(), "unsupported_capability");

    let message = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "hello"}),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(message.status, 422);
    assert_eq!(message.code(), "unsupported_capability");

    let permission = Call::post(
        format!("/v1/sessions/{run}/permissions/perm-1"),
        &serde_json::json!({"decision": "allow"}),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(permission.status, 422);
    assert_eq!(permission.code(), "unsupported_capability");

    let after: Vec<_> = world.fake.calls();
    assert!(
        !after[before..].iter().any(|call| matches!(
            call,
            AdapterCall::History(_)
                | AdapterCall::SubscribeLive(_)
                | AdapterCall::Send(_, _)
                | AdapterCall::RespondPermission(_)
        )),
        "an unsupported operation must be refused before dispatch, not after: {:?}",
        &after[before..]
    );
}

#[tokio::test]
async fn a_session_this_process_holds_no_frozen_snapshot_for_is_stale() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, snapshot) = world.launch().await;
    // A restart is exactly this: the persisted binding survives and the frozen
    // capability snapshot does not.
    world
        .daemon
        .state()
        .sessions()
        .forget(snapshot.binding_id());

    let answer = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 409);
    assert_eq!(answer.code(), "stale_binding");
}

#[tokio::test]
async fn a_run_that_was_never_bound_has_no_session() {
    let world = World::open().await;
    let run = world.unbound_run();
    let answer = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 404, "{}", answer.body);
    assert_eq!(answer.code(), "not_found");
}

#[tokio::test]
async fn the_registry_key_matches_what_the_fake_issues() {
    let world = World::open().await;
    let (_, snapshot) = world.launch().await;
    assert_eq!(
        snapshot.identity().runtime_kind,
        fake_family(),
        "the registry key and the family the runtime stamps on a binding are the same key"
    );
}

// ---------------------------------------------------------------------------
// Commands: idempotency and revisions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_replayed_command_returns_the_original_receipt_and_a_reused_key_conflicts() {
    let world = World::open().await;
    let body = resume_task_body(&world, 1);

    let first = Call::post("/v1/commands/resume_task", &body)
        .signed_as(&world, "operator")
        .with_key("replay-me")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["replayed"], serde_json::json!(false));

    let replay = Call::post("/v1/commands/resume_task", &body)
        .signed_as(&world, "operator")
        .with_key("replay-me")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["replayed"], serde_json::json!(true));
    assert_eq!(
        replay.json()["value"]["receipt_id"],
        first.json()["value"]["receipt_id"],
        "an exact replay returns the receipt that was already durable"
    );

    // The same key with a different intent is a different command wearing a used
    // key.
    let mut changed = body.clone();
    changed["intent"] = serde_json::json!({"schema_version": 1, "marker": "something-else"});
    let conflict = Call::post("/v1/commands/resume_task", &changed)
        .signed_as(&world, "operator")
        .with_key("replay-me")
        .send(&world)
        .await;
    assert_eq!(conflict.status, 409);
    assert_eq!(conflict.code(), "idempotency_conflict");
}

#[tokio::test]
async fn a_mutation_without_an_idempotency_key_is_refused() {
    let world = World::open().await;
    let answer = Call::post("/v1/commands/resume_task", &resume_task_body(&world, 1))
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
}

#[tokio::test]
async fn a_stale_revision_reports_the_current_one_and_mutates_nothing() {
    let world = World::open().await;
    let answer = Call::post("/v1/commands/resume_task", &resume_task_body(&world, 7))
        .signed_as(&world, "operator")
        .with_key("stale-revision")
        .send(&world)
        .await;
    assert_eq!(answer.status, 409);
    assert_eq!(answer.code(), "revision_conflict");
    assert_eq!(
        answer.json()["current_revision"],
        serde_json::json!(1),
        "a stale revision is answered with the revision the caller needs"
    );
    assert_eq!(answer.realm(), world.realm_id());

    // Nothing was written: the key is still free for the right revision.
    let accepted = Call::post("/v1/commands/resume_task", &resume_task_body(&world, 1))
        .signed_as(&world, "operator")
        .with_key("stale-revision")
        .send(&world)
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(accepted.json()["replayed"], serde_json::json!(false));
}

#[tokio::test]
async fn a_command_against_an_unknown_target_is_not_found() {
    let world = World::open().await;
    let mut body = resume_task_body(&world, 1);
    body["target"] = serde_json::json!({"kind": "task", "task_id": TaskId::generate().to_string()});
    let answer = Call::post("/v1/commands/resume_task", &body)
        .signed_as(&world, "operator")
        .with_key("unknown-target")
        .send(&world)
        .await;
    assert_eq!(answer.status, 404);
    assert_eq!(answer.code(), "not_found");
}

#[tokio::test]
async fn a_command_kind_that_may_not_target_that_aggregate_is_refused() {
    let world = World::open().await;
    // `launch_run` moves a run's desired state; a task is not a legal target for
    // it, and the domain's own matrix is what says so.
    let mut body = resume_task_body(&world, 1);
    body["desired_state"] = serde_json::json!("run_requested");
    let answer = Call::post("/v1/commands/launch_run", &body)
        .signed_as(&world, "operator")
        .with_key("wrong-target-kind")
        .send(&world)
        .await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
}

// ---------------------------------------------------------------------------
// The durable control-plane feed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_feed_delivers_every_event_once_in_order_and_ids_are_positions() {
    let world = World::open().await;
    let (run, _) = world.launch().await;
    observe(&world, run, 1, 1);
    observe(&world, run, 2, 2);
    observe(&world, run, 3, 3);
    world.daemon.state().signals().stop();

    let answer = Call::get("/v1/events")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let frames = answer.frames();
    let cursors: Vec<i64> = frames
        .iter()
        .map(|(_, id, _)| id.parse().expect("the frame id is a control-plane cursor"))
        .collect();
    assert_eq!(cursors.len(), 3, "three observations, three frames");
    assert!(
        cursors.windows(2).all(|pair| pair[1] > pair[0]),
        "positions are strictly increasing: {cursors:?}"
    );
    for (index, (event, id, data)) in frames.iter().enumerate() {
        assert_eq!(event, "control");
        assert_eq!(
            data["cursor"].as_i64().map(|value| value.to_string()),
            Some(id.clone()),
            "the frame id is the event's own persisted cursor"
        );
        assert_eq!(
            data["native_sequence"],
            serde_json::json!(index as u64 + 1),
            "the runtime's own ordering survives the wire"
        );
    }
}

#[tokio::test]
async fn a_resumed_feed_starts_strictly_after_the_position_it_was_given() {
    let world = World::open().await;
    let (run, _) = world.launch().await;
    observe(&world, run, 1, 1);
    observe(&world, run, 2, 2);
    world.daemon.state().signals().stop();

    let first = Call::get("/v1/events")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let frames = first.frames();
    let last: i64 = frames.last().expect("a frame").1.parse().expect("a cursor");

    for uri in [format!("/v1/events?after={last}")] {
        let resumed = Call::get(&uri)
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert!(
            resumed.frames().is_empty(),
            "resuming at the newest position delivers nothing again"
        );
    }

    // The same position, presented the way the SSE specification does.
    let via_header = Call::get("/v1/events")
        .signed_as(&world, "observer")
        .with_header("last-event-id", last.to_string())
        .send(&world)
        .await;
    assert_eq!(via_header.status, 200);
    assert!(
        via_header.frames().is_empty(),
        "Last-Event-ID resumes at exactly the position ?after does"
    );
}

#[tokio::test]
async fn after_and_last_event_id_may_not_disagree() {
    let world = World::open().await;
    world.daemon.state().signals().stop();
    let answer = Call::get("/v1/events?after=2")
        .signed_as(&world, "observer")
        .with_header("last-event-id", "5")
        .send(&world)
        .await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
}

#[tokio::test]
async fn a_position_outside_the_retained_history_demands_a_resnapshot() {
    let world = World::open().await;
    let (run, _) = world.launch().await;
    observe(&world, run, 1, 1);
    world.daemon.state().signals().stop();

    let answer = Call::get("/v1/events?after=9999")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 410);
    assert_eq!(answer.code(), "resnapshot_required");
    assert_eq!(answer.realm(), world.realm_id());
    assert!(
        answer.json()["newest_cursor"].as_i64().is_some(),
        "a resnapshot names the window the caller must snapshot against"
    );
}

#[tokio::test]
async fn the_feed_resumes_from_sqlite_after_the_process_is_rebuilt() {
    let world = World::open().await;
    let (run, _) = world.launch().await;
    observe(&world, run, 1, 1);
    observe(&world, run, 2, 2);
    world.daemon.state().signals().stop();

    let before = Call::get("/v1/events")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let seen: i64 = before
        .frames()
        .last()
        .expect("a frame")
        .1
        .parse()
        .expect("a cursor");

    // A restart: the daemon is dropped — releasing the lock — and a second one
    // opens the same state root. The temporary directory is kept alive on purpose,
    // because deleting it would make this a *fresh* realm test rather than a
    // restart one, and a fresh realm would pass the assertions below for the wrong
    // reason.
    let realm_before = world.realm_id();
    let observer = secret(&world, "observer");
    let World {
        directory, daemon, ..
    } = world;
    let state_root = directory.path().to_owned();
    drop(daemon);

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the same state root reopens");
    assert_eq!(
        restarted.realm_id(),
        realm_before,
        "a realm's identity survives a restart"
    );
    assert_eq!(
        secret_from(&state_root, "observer"),
        observer,
        "the credentials survive a restart rather than being regenerated"
    );
    restarted.state().signals().stop();

    let router = restarted.router();
    let answer = Call::get("/v1/events")
        .with_token(&observer)
        .with_header("last-event-id", seen.to_string())
        .send_to(&router)
        .await;
    assert_eq!(answer.status, 200);
    assert!(
        answer.frames().is_empty(),
        "resuming after a restart at the newest position repeats nothing"
    );

    // And the events themselves are still there: the feed is answered from SQLite,
    // so an earlier position replays the same log the previous process served.
    let from_origin = Call::get("/v1/events")
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(
        from_origin.frames().len(),
        2,
        "a restarted process serves the same durable log"
    );
    drop(directory);
}

/// One tier's secret, read straight out of a state root.
fn secret_from(state_root: &std::path::Path, tier: &str) -> String {
    let bytes = std::fs::read(kontor_daemon::credentials::path_in(state_root))
        .expect("the realm wrote its credential file");
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    document[tier].as_str().expect("the tier").to_owned()
}

// ---------------------------------------------------------------------------
// The scheduling barrier
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scheduling_is_shut_until_reconciliation_finishes() {
    let world = World::open().await;
    let before = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        before.json()["reconciliation"],
        serde_json::json!("pending")
    );
    assert_eq!(before.json()["scheduling_open"], serde_json::json!(false));

    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let after = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(after.json()["reconciliation"], serde_json::json!("open"));
    assert_eq!(after.json()["scheduling_open"], serde_json::json!(true));
}

#[tokio::test]
async fn a_runtime_that_cannot_be_reached_leaves_scheduling_shut() {
    let world = World::open().await;
    world.launch().await;
    world.fake.push_step_for(
        ScriptStep::TransportFailure {
            operation: RuntimeCapability::Discovery,
        },
        RequestKey::Sessions,
    );
    assert_eq!(
        world.daemon.reconcile().await,
        BarrierState::Failed,
        "a census that could not run proves nothing, so the barrier stays shut"
    );
    let health = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(health.json()["scheduling_open"], serde_json::json!(false));
}

// ---------------------------------------------------------------------------
// Session content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_timeline_and_the_strict_after_stream_have_no_gap_or_duplicate() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;

    // Page through history two at a time, so continuation is exercised rather
    // than assumed.
    let mut sequences = Vec::new();
    let mut cursor: Option<String> = None;
    let anchor;
    loop {
        let uri = match &cursor {
            None => format!("/v1/sessions/{run}/timeline?limit=2"),
            Some(after) => format!("/v1/sessions/{run}/timeline?limit=2&after={after}"),
        };
        let page = Call::get(&uri)
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(page.status, 200, "{}", page.body);
        let body = page.json();
        for item in body["items"].as_array().expect("items") {
            sequences.push(item["sequence"].as_u64().expect("a sequence"));
        }
        cursor = body["next"].as_str().map(str::to_owned);
        if cursor.is_none() {
            anchor = body["anchor"].as_str().expect("an anchor").to_owned();
            break;
        }
    }
    assert_eq!(
        sequences,
        vec![1, 2, 3, 4],
        "history is exactly once, in order"
    );

    let stream = Call::get(format!("/v1/sessions/{run}/stream?after={anchor}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let live: Vec<u64> = stream
        .frames()
        .iter()
        .filter(|(event, _, _)| event == "content")
        .map(|(_, _, data)| data["item"]["sequence"].as_u64().expect("a sequence"))
        .collect();
    assert_eq!(
        live,
        vec![5, 6],
        "live delivery starts strictly after the anchor"
    );

    let mut all = sequences;
    all.extend(live);
    assert!(
        all.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "no event is skipped or repeated between history and live: {all:?}"
    );
}

#[tokio::test]
async fn a_live_stream_without_an_anchor_is_refused() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let answer = Call::get(format!("/v1/sessions/{run}/stream"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
}

#[tokio::test]
async fn a_timeline_cursor_from_another_session_is_refused_before_dispatch() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let foreign = kontor_runtime::timeline::HistoryCursor::issue(
        kontor_core::id::RuntimeBindingId::generate(),
        kontor_runtime::timeline::TimelinePosition {
            epoch: 1,
            sequence: 2,
        },
    );
    let before = world.fake.calls().len();
    let answer = Call::get(format!(
        "/v1/sessions/{run}/timeline?after={}",
        foreign.as_str()
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(answer.status, 400);
    assert!(
        !world.fake.calls()[before..]
            .iter()
            .any(|call| matches!(call, AdapterCall::History(_))),
        "a cursor for another session is refused without asking the runtime"
    );
}

#[tokio::test]
async fn an_epoch_change_ends_the_stream_with_a_refetch() {
    let world = World::open().await;
    world.script(EPOCH_CHANGE);
    let (run, _) = world.launch().await;

    let timeline = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let anchor = timeline.json()["anchor"]
        .as_str()
        .expect("an anchor")
        .to_owned();

    let stream = Call::get(format!("/v1/sessions/{run}/stream?after={anchor}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let frames = stream.frames();
    let (event, _, data) = frames.last().expect("the stream said something");
    assert_eq!(
        event, "error",
        "a broken timeline ends the stream: {frames:?}"
    );
    assert_eq!(data["code"], serde_json::json!("timeline_refetch_required"));
    assert_eq!(
        data["realm_id"].as_str(),
        Some(world.realm_id().to_string().as_str())
    );
    assert!(
        frames
            .iter()
            .filter(|(event, _, _)| event == "content")
            .count()
            < 2,
        "nothing past the break is delivered: {frames:?}"
    );
}

// ---------------------------------------------------------------------------
// Messages and permissions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_lost_acknowledgement_is_replayed_without_a_second_native_effect() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, snapshot) = world.launch().await;
    let key = kontor_runtime::request::MessageId::generate().to_string();
    world.fake.push_step_for(
        ScriptStep::LoseSendAck,
        RequestKey::Message(
            kontor_runtime::request::MessageId::parse(&key).expect("a canonical message id"),
        ),
    );

    let lost = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "the message that was committed"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    assert_eq!(
        lost.status, 503,
        "a lost acknowledgement is a fact about the channel: {}",
        lost.body
    );
    assert_eq!(lost.code(), "unavailable");

    // The retry is answered from the runtime's ledger, and the content grew by one
    // item and not by two.
    let retried = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "the message that was committed"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    assert_eq!(retried.status, 200, "{}", retried.body);
    assert_eq!(retried.realm(), world.realm_id());
    assert_eq!(
        retried.json()["value"]["message_id"],
        serde_json::json!(key)
    );

    let delivered = world
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::Send(binding, _) if *binding == snapshot.binding_id()))
        .count();
    assert_eq!(delivered, 2, "the caller retried once");
    let timeline = Call::get(format!("/v1/sessions/{run}/timeline?limit=64"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let messages = timeline.json()["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|item| item["message_id"].as_str() == Some(&key))
        .count();
    assert_eq!(
        messages, 1,
        "the retry replayed the original effect rather than committing a second one"
    );
}

#[tokio::test]
async fn the_same_key_with_different_content_is_an_idempotency_conflict() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let key = kontor_runtime::request::MessageId::generate().to_string();

    let first = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "one thing"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);

    let contradiction = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "quite another thing"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    assert_eq!(contradiction.status, 409);
    assert_eq!(contradiction.code(), "idempotency_conflict");
}

#[tokio::test]
async fn a_session_key_must_be_a_stable_client_message_id() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let answer = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "hello"}),
    )
    .signed_as(&world, "operator")
    .with_key("not-a-message-id")
    .send(&world)
    .await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
}

#[tokio::test]
async fn a_permission_answer_is_applied_once_and_a_foreign_request_is_refused() {
    let world = World::open().await;
    world.script(PERMISSION_WAIT);
    let (run, _) = world.launch().await;
    let key = kontor_runtime::request::MessageId::generate().to_string();

    let allowed = Call::post(
        format!("/v1/sessions/{run}/permissions/perm-1"),
        &serde_json::json!({"decision": "allow"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    assert_eq!(allowed.status, 200, "{}", allowed.body);
    assert_eq!(allowed.realm(), world.realm_id());
    assert_eq!(
        allowed.json()["value"]["decision"],
        serde_json::json!("allow")
    );

    // A contradiction under the same response id fails typed.
    let contradiction = Call::post(
        format!("/v1/sessions/{run}/permissions/perm-1"),
        &serde_json::json!({"decision": "deny"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    assert_eq!(contradiction.status, 409);
    assert_eq!(contradiction.code(), "idempotency_conflict");

    // A request this session's content never raised is refused before dispatch.
    let before = world.fake.calls().len();
    let foreign = Call::post(
        format!("/v1/sessions/{run}/permissions/perm-elsewhere"),
        &serde_json::json!({"decision": "allow"}),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(foreign.status, 404);
    assert!(
        !world.fake.calls()[before..]
            .iter()
            .any(|call| matches!(call, AdapterCall::RespondPermission(_))),
        "a permission request this session never raised is refused before dispatch"
    );
}

// ---------------------------------------------------------------------------
// The configured runtime fleet
// ---------------------------------------------------------------------------

/// A fleet description naming one Paseo plane and one AO lane.
///
/// Both compose without reaching anything: a Paseo transport validates two
/// strings and an AO transport builds an HTTP client. Neither connects until a
/// route asks it to.
fn fleet_settings() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "runtimes": [
            {
                "family": "paseo",
                "runtime_kind": "paseo.agent",
                "host_key": "paseo-host",
                "mini_project_id": "mini-1",
                "jira_epic_key": "ASMA-7759",
                "mini_project_short_title": "Kontor MVP",
                "plan_item_key": "KON-MVP-15",
                "task_short_title": "Loopback seat",
                "canonical_worktree_cwd": "/w/kontor-task",
                "orchestrator_agent_id": "orchestrator-1",
                "max_concurrent_sessions": 4,
                "executable": "paseo",
                "host_target": "https://operator:hunter2@paseo.example",
                "timeout_seconds": 30
            },
            {
                "family": "ao",
                "runtime_kind": "ao.claude-code",
                "host": "ao-host",
                "project_id": "proj-1",
                "project_path": "/w/ao-project",
                "kind": "worker",
                "harness": "claude-code",
                "max_concurrent_sessions": 8,
                "endpoint": "http://127.0.0.1:1/",
                "timeout_seconds": 10
            }
        ]
    })
}

#[tokio::test]
async fn the_shipped_startup_path_composes_the_configured_fleet() {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    std::fs::write(
        kontor_daemon::runtimes::path_in(directory.path()),
        serde_json::to_vec_pretty(&fleet_settings()).expect("a fleet document"),
    )
    .expect("the fleet description is written");

    // `start_configured` is the path `main.rs` takes. An empty registry here is
    // the defect: every session route would answer as unconfigured no matter what
    // the operator wrote.
    let daemon = Daemon::start_configured(DaemonConfig::at(directory.path()).with_port(0))
        .expect("the configured realm starts");
    let families: Vec<String> = daemon
        .state()
        .runtimes()
        .families()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        families,
        vec!["ao.claude-code".to_owned(), "paseo.agent".to_owned()],
        "both configured families are live adapters in the registry"
    );

    let observer = secret_from(directory.path(), "observer");
    let router = daemon.router();
    let health = Call::get("/v1/health")
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(health.status, 200, "{}", health.body);
    assert_eq!(
        health.json()["runtimes"],
        serde_json::json!(["ao.claude-code", "paseo.agent"]),
        "and the realm reports the fleet it is actually holding"
    );
    assert!(
        !health.body.contains("hunter2"),
        "a configured credential does not reach the control plane"
    );
    daemon.shutdown();
    drop(directory);
}

#[tokio::test]
async fn a_realm_with_no_fleet_still_starts_and_says_so() {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let daemon = Daemon::start_configured(DaemonConfig::at(directory.path()).with_port(0))
        .expect("a realm with no runtimes still starts");
    assert!(
        daemon.state().runtimes().families().next().is_none(),
        "an absent fleet description is a realm with no runtimes, not a failure"
    );
    daemon.shutdown();
    drop(directory);
}

#[tokio::test]
async fn a_misconfigured_fleet_refuses_the_start() {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let mut broken = fleet_settings();
    broken["runtimes"][1]["endpoint"] = serde_json::json!("not-a-url");
    std::fs::write(
        kontor_daemon::runtimes::path_in(directory.path()),
        serde_json::to_vec_pretty(&broken).expect("a fleet document"),
    )
    .expect("the fleet description is written");

    let refused = Daemon::start_configured(DaemonConfig::at(directory.path()).with_port(0))
        .expect_err("a runtime that cannot be composed is not served around");
    let rendered = refused.to_string();
    assert!(rendered.contains("endpoint"), "the refusal names the field");
    assert!(
        !rendered.contains("hunter2") && !rendered.contains("not-a-url"),
        "and never the value: {rendered}"
    );
}

#[tokio::test]
async fn a_configured_adapter_backs_every_session_route() {
    // The registry seam `start_configured` fills is the one a session route reads,
    // so the scripted runtime proves the whole path: resolve the binding, select
    // the family, have the adapter vouch for it, then dispatch.
    let world = World::open().await;
    world.script(PERMISSION_WAIT);
    let (run, snapshot) = world.launch().await;
    assert_eq!(
        snapshot.identity().runtime_kind,
        fake_family(),
        "the binding names the family the registry answers to"
    );
    let before = world.fake.calls().len();

    let timeline = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(timeline.status, 200, "{}", timeline.body);
    let anchor = timeline.json()["anchor"]
        .as_str()
        .expect("an anchor")
        .to_owned();

    let stream = Call::get(format!("/v1/sessions/{run}/stream?after={anchor}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(stream.status, 200);

    let message = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "through the configured adapter"}),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(message.status, 200, "{}", message.body);

    let permission = Call::post(
        format!("/v1/sessions/{run}/permissions/perm-1"),
        &serde_json::json!({"decision": "allow"}),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(permission.status, 200, "{}", permission.body);

    // Every one of the four reached the runtime rather than being answered from
    // this process.
    let calls = world.fake.calls();
    let reached = &calls[before..];
    for expected in ["history", "live", "send", "permission"] {
        let seen = reached.iter().any(|call| {
            matches!(
                (expected, call),
                ("history", AdapterCall::History(_))
                    | ("live", AdapterCall::SubscribeLive(_))
                    | ("send", AdapterCall::Send(_, _))
                    | ("permission", AdapterCall::RespondPermission(_))
            )
        });
        assert!(seen, "the {expected} route dispatched: {reached:?}");
    }
}

#[tokio::test]
async fn a_session_whose_family_is_not_configured_answers_unavailable() {
    // The honest negative, and the one the empty-registry defect made universal.
    // The run and its binding are durable; the runtime that owns the session is
    // the thing that is missing, and the answer says exactly that.
    let world = World::open_unconfigured().await;
    let run = world.bind_to_family(&fake_family());

    for uri in [
        format!("/v1/sessions/{run}/timeline"),
        format!("/v1/sessions/{run}/stream?after=1:0"),
    ] {
        let answer = Call::get(&uri)
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(answer.status, 503, "{uri}: {}", answer.body);
        assert_eq!(answer.code(), "unavailable");
        assert_eq!(answer.realm(), world.realm_id());
    }

    // The control plane is unaffected: an unconfigured fleet is not a broken realm.
    let health = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(health.status, 200);
    assert_eq!(
        health.json()["runtimes"],
        serde_json::json!([]),
        "and the realm reports holding no runtime rather than implying one"
    );
}

// ---------------------------------------------------------------------------
// Disclosure
// ---------------------------------------------------------------------------

/// Text that must never appear in a response, in the contract document, in a
/// stored row or in a log line.
///
/// The transcript canaries are the bodies the harness's own fixtures use, so the
/// scan is not looking for a word that happens to be absent — it is looking for
/// content this very test wrote into a session.
const TRANSCRIPT_CANARIES: &[&str] = &["do the loopback work", "the message that was committed"];

#[tokio::test]
async fn no_response_or_stored_row_carries_a_secret_a_runtime_endpoint_or_a_transcript() {
    let world = World::open().await;
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let key = kontor_runtime::request::MessageId::generate().to_string();
    Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "the message that was committed"}),
    )
    .signed_as(&world, "operator")
    .with_key(&key)
    .send(&world)
    .await;
    observe(&world, run, 1, 1);
    world.daemon.state().signals().stop();

    let secrets: Vec<String> = ["observer", "operator", "admin"]
        .iter()
        .map(|tier| secret(&world, tier))
        .collect();

    // Every control-plane surface: the answers, the contract document and the feed.
    for uri in [
        "/v1/health".to_owned(),
        "/v1/realm".to_owned(),
        "/v1/openapi.json".to_owned(),
        "/v1/events".to_owned(),
        format!("/v1/runs/{run}"),
        format!("/v1/projects/{}/tasks/{}", world.project, world.task),
    ] {
        let answer = Call::get(&uri)
            .signed_as(&world, "admin")
            .send(&world)
            .await;
        for secret in &secrets {
            assert!(
                !answer.body.contains(secret.as_str()),
                "{uri} disclosed a realm credential"
            );
        }
        for canary in TRANSCRIPT_CANARIES {
            assert!(
                !answer.body.contains(canary),
                "{uri} disclosed session content: {canary}"
            );
        }
        for endpoint in ["/fake-runtime-root", "fake-host:"] {
            assert!(
                !answer.body.contains(endpoint),
                "{uri} disclosed a runtime location"
            );
        }
    }

    // The database itself. A transcript that reached SQLite would show up here even
    // if every response happened to omit it.
    let database = std::fs::read(world.directory.path().join(kontor_daemon::DATABASE_FILE))
        .expect("the database file is readable");
    let raw = String::from_utf8_lossy(&database);
    for canary in TRANSCRIPT_CANARIES {
        assert!(
            !raw.contains(canary),
            "session content reached the durable log: {canary}"
        );
    }
    for secret in &secrets {
        assert!(
            !raw.contains(secret.as_str()),
            "a realm credential reached the durable log"
        );
    }

    // And the stored event rows, read back through the port a subscriber uses.
    let page = world
        .daemon
        .state()
        .with_store(|store| store.realm_event_page(None, 256))
        .expect("the feed reads back");
    for envelope in &page.events {
        let payload = envelope
            .peek(world.realm_id())
            .expect("our own realm")
            .payload
            .json()
            .to_owned();
        for canary in TRANSCRIPT_CANARIES {
            assert!(
                !payload.contains(canary),
                "a stored event row carries session content: {canary}"
            );
        }
    }
}

#[tokio::test]
async fn the_credential_file_is_owner_only() {
    let world = World::open().await;
    let path = kontor_daemon::credentials::path_in(world.directory.path());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("the credential file exists")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o600,
            "a realm credential is readable by its owner only"
        );
    }
    #[cfg(not(unix))]
    {
        assert!(path.exists(), "the realm wrote its credential file");
    }
}

// ---------------------------------------------------------------------------
// Cross-realm envelopes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_command_naming_another_realms_project_resolves_to_nothing() {
    let world = World::open().await;
    let other = World::open().await;

    let mut body = resume_task_body(&world, 1);
    body["project_id"] = serde_json::json!(other.project.to_string());
    body["target"] = serde_json::json!({"kind": "task", "task_id": other.task.to_string()});
    let answer = Call::post("/v1/commands/resume_task", &body)
        .signed_as(&world, "operator")
        .with_key("another-realms-work")
        .send(&world)
        .await;
    assert_eq!(answer.status, 404);
    assert_eq!(answer.code(), "not_found");
    assert_eq!(answer.realm(), world.realm_id());
}

#[tokio::test]
async fn a_task_from_another_project_in_this_realm_does_not_resolve() {
    let world = World::open().await;
    let stranger = ProjectId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_project(&NewProject {
                id: stranger,
                name: name("Another project"),
                root_path: name("/tmp/kontor-stranger"),
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("a second project is created");
    });
    let answer = Call::get(format!("/v1/projects/{stranger}/tasks/{}", world.task))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        answer.status, 404,
        "a globally unique id is not tenant isolation: the predicate is"
    );
}

#[tokio::test]
async fn a_cursor_from_another_realm_is_refused_typed_before_any_read() {
    let world = World::open().await;
    let other = World::open().await;
    let (run, _) = world.launch().await;
    observe(&world, run, 1, 1);

    // A control-plane position is only meaningful inside the realm that allocated
    // it. On the wire a caller cannot even express a foreign one — `?after=` is an
    // integer this realm qualifies with its own id — so the case is proved where it
    // can exist: at the ingress the transport itself goes through.
    let foreign = kontor_core::realm::RealmCursor::new(
        other.realm_id(),
        kontor_core::id::EventCursor::parse(1).expect("a position"),
    );
    let refusal = world
        .daemon
        .state()
        .with_store(|store| store.realm_event_page(Some(foreign), 16))
        .expect_err("a position from another realm counts in a different space");
    let mapped = kontor_api::error::ApiError::from_repository(world.realm_id(), &refusal);
    assert_eq!(
        mapped.code,
        kontor_api::error::ApiErrorCode::RealmMismatch,
        "and it is refused as exactly that, before a row is read"
    );

    // The same envelope rule holds for a snapshot taken here: opening it against
    // another realm's id refuses rather than handing the value over.
    let snapshot = world
        .daemon
        .state()
        .with_store(|store| store.snapshot_run_inspection(run))
        .expect("the run reads back");
    assert!(
        snapshot.open(other.realm_id()).is_err(),
        "an envelope is opened by proving the realm, not by asking politely"
    );
}

// ---------------------------------------------------------------------------
// Snapshot consistency
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_snapshot_carries_the_position_it_is_consistent_with() {
    let world = World::open().await;
    let (run, _) = world.launch().await;

    let before = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let first = before.json()["snapshot_cursor"].as_i64().expect("a cursor");

    observe(&world, run, 1, 1);

    let after = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let second = after.json()["snapshot_cursor"].as_i64().expect("a cursor");
    assert!(
        second > first,
        "a snapshot taken after an append is consistent with a later position"
    );
    assert_eq!(
        after.json()["value"]["projection"]["observed"],
        serde_json::json!("running"),
        "the projection and the cursor come from the same read"
    );
    // Freshness is derived at read time from `last_confirmed_at`, and this run has
    // none: its observation disagrees with a run that was never asked to do
    // anything, so nothing was *confirmed* and `unknown` is the honest answer. The
    // pairing is what matters — a stored confirmation instant and a freshness that
    // ignored it would be the bug.
    assert!(
        after.json()["value"]["projection"]["last_confirmed_at"].is_null(),
        "the fixture reduced no confirmation"
    );
    assert_eq!(
        after.json()["value"]["projection"]["freshness"],
        serde_json::json!("unknown"),
        "with no confirmation to age, freshness is unknown rather than fresh"
    );
    assert_eq!(
        after.json()["value"]["projection"]["derived"],
        serde_json::json!("diverged"),
        "an observation that disagrees with intent is a divergence, never a closure"
    );

    // Resuming strictly after the snapshot position delivers nothing that the
    // snapshot already accounted for.
    world.daemon.state().signals().stop();
    let feed = Call::get(format!("/v1/events?after={second}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert!(
        feed.frames().is_empty(),
        "a subscriber resumes strictly after the snapshot cursor without a duplicate"
    );
}

#[tokio::test]
async fn a_task_snapshot_reports_the_pinned_specification_revisions() {
    let world = World::open().await;
    let answer = Call::get(format!(
        "/v1/projects/{}/tasks/{}",
        world.project, world.task
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let body = answer.json();
    assert_eq!(body["value"]["revision"], serde_json::json!(1));
    assert!(
        body["value"]["applied"].is_object(),
        "a task snapshot reports which pinned revisions it is running under"
    );
}

#[tokio::test]
async fn a_run_snapshot_reports_the_team_template_revision_and_its_binding() {
    let world = World::open().await;
    let (run, snapshot) = world.launch().await;
    let answer = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let body = answer.json();
    assert_eq!(
        body["value"]["binding"]["native_id"],
        serde_json::json!(snapshot.identity().native_id.as_str())
    );
    assert_eq!(
        body["value"]["binding"]["attached"],
        serde_json::json!(true)
    );
    assert!(
        body["value"]["applied"]["team_template"].is_string(),
        "a run reports the team revision its team pinned"
    );
    assert!(
        body["value"]["gaps"].as_array().expect("gaps").is_empty(),
        "a healthy run has no recorded discontinuity"
    );
}

// ---------------------------------------------------------------------------
// Graceful shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_shuts_scheduling_and_releases_the_state_root() {
    let world = World::open().await;
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let state_root = world.directory.path().to_owned();
    let directory = world.directory;
    world.daemon.shutdown();

    // The claim is gone, so the next daemon may take it.
    let next = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("a released state root can be claimed again");
    assert_eq!(next.state().barrier().state(), BarrierState::Pending);
    drop(next);
    drop(directory);
}
