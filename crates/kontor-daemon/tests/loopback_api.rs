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

use harness::{Answer, Call, World, at, capabilities_without, fake_family, name, secret};
use kontor_api::state::BarrierState;
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{AgentRunId, CanonicalDocument, ProjectId, TaskId};
use kontor_core::repository::{
    NewObservation, NewProject, NewRuntimeEvent, ProjectRepository, RealmRepository, RunRepository,
};
use kontor_core::state::{Freshness, ObservedRunState, RuntimeContact};
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_runtime::adapter::RuntimeAdapter as _;
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
                "provider": "codex",
                "jira_epic_key": "ASMA-7759",
                "mini_project_short_title": "Kontor MVP",
                "plan_item_key": "KON-MVP-15",
                "task_short_title": "Loopback seat",
                "project_root_cwd": "/w/kontor",
                "canonical_worktree_cwd": "/w/kontor-task",
                "orchestrator_agent_id": "orchestrator-1",
                "max_concurrent_sessions": 4,
                "executable": "paseo",
                "host_target": "https://operator:hunter2@paseo.example",
                "endpoint": "ws://127.0.0.1:6767/ws",
                "client_id": "kontor-mini-1",
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

// ---------------------------------------------------------------------------
// The empty-realm bootstrap journey
//
// Everything below starts from a `kontord` that has been installed and
// configured and has never been used: no project, no goal, no task, no team run,
// no seed script and no direct SQL. One admin credential drives the whole
// sequence through public application operations, which is the only way to prove
// the sequence is actually possible rather than merely plausible against a
// fixture something else wrote.
// ---------------------------------------------------------------------------

/// Ensure the one project an empty Realm needs, returning `(id, revision)`.
async fn ensure_project(world: &World, key: &str, name: &str, root: &str) -> Answer {
    Call::post(
        "/v1/projects:ensure",
        &serde_json::json!({"name": name, "root_path": root}),
    )
    .signed_as(world, "admin")
    .with_key(key)
    .send(world)
    .await
}

/// The first runnable work-profile category the bundled pack advertises.
async fn first_category(world: &World) -> String {
    let catalog = Call::get("/v1/catalog/work-profiles")
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    catalog.json().as_array().expect("a catalog array")[0]
        .get("category")
        .and_then(serde_json::Value::as_str)
        .expect("a category")
        .to_owned()
}

/// A two-task epic whose second task waits on the first.
fn epic_body(
    revision: u64,
    name: &str,
    category: &str,
    tasks: serde_json::Value,
) -> serde_json::Value {
    // Every task gets a worktree unless the caller stated one. A task with no
    // declared worktree cannot be seated — there is nowhere to prepare its
    // workspace — so a helper that omitted it would make most of this suite
    // assert against a graph that could never run.
    let tasks = tasks
        .as_array()
        .expect("a task array")
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let mut task = task.clone();
            if task.get("worktree").is_none() {
                task["worktree"] = serde_json::json!(
                    format!("/w/{name}/{index}")
                        .to_lowercase()
                        .replace(' ', "-")
                );
            }
            task
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "expected_revision": revision,
        "name": name,
        "work_profile_category": category,
        "runtime_family": "fake.runtime",
        "tasks": tasks,
    })
}

#[tokio::test]
async fn an_empty_realm_is_bootstrapped_through_public_operations_alone() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    // Nothing has been seeded, and the realm says so by having no project to
    // resolve rather than by an empty list nobody could have written.
    let realm = Call::get("/v1/realm")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(realm.status, 200);

    let created = ensure_project(&world, "bootstrap-1", "Kontor", "/tmp/kontor-empty").await;
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.json()["applied"], "created");
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

    // The same ensure is the same project, and nothing was written twice.
    let replayed = ensure_project(&world, "bootstrap-2", "Kontor", "/tmp/kontor-empty").await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(replayed.json()["project_id"], created.json()["project_id"]);

    // A different name at the same root is drift, not an update.
    let drift = ensure_project(&world, "bootstrap-3", "Something else", "/tmp/kontor-empty").await;
    assert_eq!(drift.status, 409, "{}", drift.body);

    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Bootstrap epic",
            &category,
            serde_json::json!([
                {"title": "Design the thing", "ticket_links": [
                    {"connector": "jira", "external_issue_key": "ASMA-1"}
                ]},
                {"title": "Build the thing", "depends_on": ["Design the thing"]}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("epic-1")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let tasks = applied.json()["tasks"].as_array().expect("tasks").clone();
    assert_eq!(tasks.len(), 2, "the whole graph was applied at once");
    assert_eq!(tasks[0]["applied"], "created");
    assert_eq!(
        tasks[1]["depends_on"].as_array().expect("edges").len(),
        1,
        "the dependency edge was resolved from a sibling title"
    );
    assert_eq!(
        tasks[0]["links"].as_array().expect("links").len(),
        1,
        "the ticket link was attached in the same operation"
    );

    // The projection reads back the graph, the selections and the links.
    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    assert_eq!(projection.realm(), world.realm_id());
    assert_eq!(
        projection.json()["tasks"].as_array().expect("tasks").len(),
        2
    );
    assert!(
        projection.json()["work_profile"]["id"].is_string(),
        "every task pinned the profile the epic selected"
    );
    assert!(
        projection.json()["tasks"][0]["workflow_revision"].is_u64(),
        "a task with an active workflow reports the revision a gate recording \
         must present: {}",
        projection.body
    );
}

#[tokio::test]
async fn reapplying_the_identical_epic_writes_nothing_and_drift_is_refused() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "reapply-1", "Kontor", "/tmp/kontor-reapply").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;
    let body = epic_body(
        revision,
        "Idempotent epic",
        &category,
        serde_json::json!([{"title": "Only task"}]),
    );

    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("reapply-epic-1")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let epic = first.json()["epic_id"].as_str().expect("id").to_owned();

    let again = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("reapply-epic-2")
        .send(&world)
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(
        again.json()["epic_id"],
        epic,
        "the same epic, not a second one"
    );
    assert_eq!(again.json()["applied"], "unchanged");
    assert_eq!(again.json()["tasks"][0]["applied"], "unchanged");

    // Same key, different bytes: a conflict, and no second epic.
    let reused = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "A different epic",
            &category,
            serde_json::json!([{"title": "Only task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("reapply-epic-1")
    .send(&world)
    .await;
    assert_eq!(reused.status, 409, "{}", reused.body);
}

#[tokio::test]
async fn a_cyclic_or_dangling_epic_rolls_the_whole_application_back() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "cycle-1", "Kontor", "/tmp/kontor-cycle").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    for tasks in [
        // A two-node cycle.
        serde_json::json!([
            {"title": "A", "depends_on": ["B"]},
            {"title": "B", "depends_on": ["A"]}
        ]),
        // An edge naming a task the epic never states.
        serde_json::json!([{"title": "A", "depends_on": ["Nowhere"]}]),
        // A task depending on itself.
        serde_json::json!([{"title": "A", "depends_on": ["A"]}]),
        // The same external issue linked twice to one task.
        serde_json::json!([{"title": "A", "ticket_links": [
            {"connector": "jira", "external_issue_key": "ASMA-9"},
            {"connector": "jira", "external_issue_key": "ASMA-9"}
        ]}]),
    ] {
        let refused = Call::post(
            format!("/v1/projects/{project}/epics:apply"),
            &epic_body(revision, "Refused epic", &category, tasks.clone()),
        )
        .signed_as(&world, "admin")
        .with_key(format!("cycle-{}", tasks))
        .send(&world)
        .await;
        assert!(
            refused.status.is_client_error(),
            "{tasks} must be refused: {}",
            refused.body
        );
    }

    // Nothing survived any of them: the goal itself was never created.
    let epics = world.daemon.state().with_store(|store| {
        store
            .list_tasks(kontor_core::id::ProjectId::parse(&project).expect("a project id"))
            .expect("the tasks read back")
    });
    assert!(
        epics.is_empty(),
        "a refused apply must leave no task behind, found {}",
        epics.len()
    );
}

#[tokio::test]
async fn arming_disarming_and_planning_are_scoped_authority_decisions() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "arm-1", "Kontor", "/tmp/kontor-arm").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead architect",
            "harness": "fake.runtime",
            "credential_alias": "lead-architect",
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("account-1")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();
    // The catalog says nothing a caller could authenticate with.
    let listed = Call::get(format!("/v1/projects/{project}/provider-account-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(listed.status, 200);
    for forbidden in ["credential", "token", "secret", "keychain", "config_home"] {
        assert!(
            !listed.body.contains(forbidden),
            "the account catalog must not carry `{forbidden}`: {}",
            listed.body
        );
    }

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Armed epic",
            &category,
            serde_json::json!([{"title": "First"}, {"title": "Second"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("arm-epic-1")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let first_task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();

    let arm_body = serde_json::json!({
        "expected_revision": epic_revision,
        "tasks": [first_task],
        "allowed_start": "2020-01-01T00:00:00Z",
        "allowed_end": "2099-01-01T00:00:00Z",
        "max_concurrency": 2,
        "budget": {
            "max_tokens": 100000,
            "max_commands": 500,
            "max_duration_seconds": 3600,
            "max_cost_minor_units": 5000,
            "cost_currency": "NOK"
        },
        "granted_by": account_id,
        "reason": "Bootstrap the epic"
    });

    // Arming is admin authority: an operator credential does not reach it.
    let refused = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &arm_body,
    )
    .signed_as(&world, "operator")
    .with_key("arm-operator")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.code(), "forbidden");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &arm_body,
    )
    .signed_as(&world, "admin")
    .with_key("arm-admin")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);
    let authorization = armed.json()["authorization_id"]
        .as_str()
        .expect("an authorization id")
        .to_owned();
    assert_eq!(
        armed.json()["selected_tasks"]
            .as_array()
            .expect("tasks")
            .len(),
        1,
        "arming names exactly the scope it was asked for"
    );

    // The planner explains itself, and writes nothing while doing it.
    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    assert!(plan.json()["plan_hash"].is_string());
    let ready: Vec<_> = plan.json()["ready"]
        .as_array()
        .expect("a ready set")
        .iter()
        .map(|task| task["task_id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(
        ready,
        vec![first_task.clone()],
        "only the armed task is ready; its sibling is not armed: {}",
        plan.body
    );
    assert!(
        plan.json()["blocked"]
            .as_array()
            .expect("blocked")
            .iter()
            .any(|task| task["code"] == "authorization_missing"),
        "an unarmed sibling blocks with a named reason: {}",
        plan.body
    );

    // Disarming revokes future admission; the planner then admits nothing.
    let disarmed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:disarm"),
        &serde_json::json!({
            "authorization_id": authorization,
            "revoked_by": account_id,
            "reason": "Stand the epic down"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("disarm-1")
    .send(&world)
    .await;
    assert_eq!(disarmed.status, 200, "{}", disarmed.body);
    assert!(disarmed.json()["revoked_at"].is_string());

    let after = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(after.status, 200, "{}", after.body);
    assert!(
        after.json()["ready"].as_array().expect("ready").is_empty(),
        "a revoked authorization arms nothing: {}",
        after.body
    );
}

/// A body that parses for whichever operation `uri` names.
///
/// The authority tests need the extractor to succeed so that the refusal they
/// observe is the capability check and not `Json`'s.
fn well_formed_body(uri: &str) -> serde_json::Value {
    if uri.ends_with("epics:apply") {
        serde_json::json!({
            "expected_revision": 1, "name": "X", "work_profile_category": "x",
            "runtime_family": "fake.runtime", "tasks": []
        })
    } else if uri.ends_with("provider-account-profiles:ensure") {
        serde_json::json!({
            "label": "X", "harness": "fake.runtime",
            "credential_alias": "x", "enabled": true
        })
    } else if uri.ends_with("execution:arm") {
        serde_json::json!({
            "expected_revision": 1, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1, "max_commands": 1, "max_duration_seconds": 1,
                       "max_cost_minor_units": 1, "cost_currency": "NOK"},
            "granted_by": kontor_core::id::AccountProfileId::generate().to_string(),
            "reason": "x"
        })
    } else if uri.ends_with("execution:disarm") {
        serde_json::json!({
            "authorization_id": kontor_core::id::ExecutionAuthorizationId::generate().to_string(),
            "revoked_by": kontor_core::id::AccountProfileId::generate().to_string(),
            "reason": "x"
        })
    } else if uri.ends_with("scheduler:start") {
        serde_json::json!({"plan_hash": "0".repeat(64)})
    } else if uri.ends_with("lifecycle") {
        serde_json::json!({"action": "block", "expected_revision": 1, "reason": "x"})
    } else if uri.ends_with("projects:ensure") {
        serde_json::json!({"name": "X", "root_path": "/tmp/kontor-authz-body"})
    } else {
        serde_json::json!({})
    }
}

#[tokio::test]
async fn every_application_operation_refuses_an_unauthenticated_or_under_privileged_caller() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "authz-1", "Kontor", "/tmp/kontor-authz").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let epic = kontor_core::id::MiniProjectId::generate().to_string();

    let mutations = [
        "/v1/projects:ensure".to_owned(),
        format!("/v1/projects/{project}/epics:apply"),
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        format!("/v1/projects/{project}/epics/{epic}/execution:disarm"),
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        format!("/v1/projects/{project}/epics/{epic}/lifecycle"),
    ];
    for uri in &mutations {
        // The body is well formed on purpose: a malformed one would be refused by
        // the extractor, and the assertion here is about *authority*.
        let body = well_formed_body(uri);
        let anonymous = Call::post(uri, &body)
            .anonymous()
            .with_key("authz")
            .send(&world)
            .await;
        assert_eq!(anonymous.status, 401, "{uri}: {}", anonymous.body);

        let observer = Call::post(uri, &body)
            .signed_as(&world, "observer")
            .with_key("authz")
            .send(&world)
            .await;
        assert_eq!(observer.status, 403, "{uri}: {}", observer.body);
        assert_eq!(observer.code(), "forbidden");
    }

    // And a mutation with no idempotency key is refused before it does anything.
    let keyless = Call::post(
        "/v1/projects:ensure",
        &serde_json::json!({"name": "X", "root_path": "/tmp/kontor-keyless"}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(keyless.status, 400, "{}", keyless.body);
    assert_eq!(keyless.code(), "invalid_request");
}

#[tokio::test]
async fn the_contract_document_lists_every_application_route_and_no_unsafe_surface() {
    let world = World::open_empty().await;
    let document = Call::get("/v1/openapi.json")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(document.status, 200);
    let paths = document.json()["paths"].clone();

    for route in [
        "/v1/projects:ensure",
        "/v1/catalog/work-profiles",
        "/v1/catalog/team-templates",
        "/v1/runtime-capabilities",
        "/v1/projects/{project_id}/provider-account-profiles",
        "/v1/projects/{project_id}/provider-account-profiles:ensure",
        "/v1/projects/{project_id}/epics:apply",
        "/v1/projects/{project_id}/epics/{epic_id}",
        "/v1/projects/{project_id}/epics/{epic_id}/execution:arm",
        "/v1/projects/{project_id}/epics/{epic_id}/execution:disarm",
        "/v1/projects/{project_id}/epics/{epic_id}/scheduler:plan",
        "/v1/projects/{project_id}/epics/{epic_id}/scheduler:start",
        "/v1/projects/{project_id}/epics/{epic_id}/lifecycle",
        "/v1/projects/{project_id}/tasks/{task_id}/context:resolve",
        "/v1/projects/{project_id}/tasks/{task_id}/gates/{gate_id}/record",
        "/v1/projects/{project_id}/tasks/{task_id}/profile-selection",
        "/v1/projects/{project_id}/tasks/{task_id}/team-selection",
        "/v1/projects/{project_id}/tasks/{task_id}/account-selection",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-plan",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-apply",
        "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:settle",
        "/v1/catalog/work-profiles/{category}",
        "/v1/catalog/work-profiles/{category}/validate",
        "/v1/projects/{project_id}/triggers/{trigger}/{version}",
        "/v1/projects/{project_id}/intake:submit",
        "/v1/projects/{project_id}/intake/{receipt_id}",
        "/v1/projects/{project_id}/connectors/{connector}/field-specs",
        "/v1/projects/{project_id}/connectors/{connector}/workflow-specs",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:conflicts",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:resolve-conflict",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:pull-comments",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:comments",
        "/v1/projects/{project_id}/tasks/{task_id}/ticket:claim",
    ] {
        assert!(
            paths.get(route).is_some(),
            "the contract must list {route}: {}",
            document.body
        );
    }

    // Nothing in the contract creates a native session, names a runtime endpoint
    // or carries credential material. The scan is over *names* — routes and
    // schema properties — rather than over prose, because a doc comment saying
    // "never an endpoint" is the opposite of a disclosure and a substring search
    // over the whole body cannot tell the two apart.
    let mut names: Vec<String> = paths
        .as_object()
        .expect("a path map")
        .keys()
        .map(|route| route.to_lowercase())
        .collect();
    collect_property_names(&document.json()["components"]["schemas"], &mut names);
    // Whole segments, not substrings: `max_tokens` is a budget ceiling and
    // `bearer_token` would be a disclosure, and only segment matching tells them
    // apart without hand-maintaining an allowlist of near-misses.
    for forbidden in [
        "endpoint", "url", "token", "secret", "password", "keychain", "assignee",
    ] {
        assert!(
            !names.iter().any(|name| name
                .split(['_', '-', '/', ':', '.'])
                .any(|part| part == forbidden)),
            "the public contract must expose no `{forbidden}` name"
        );
    }
    for forbidden in [
        "sessions:create",
        "session_create",
        "create_session",
        "config_home",
        "credential_value",
        "credential_ref",
    ] {
        assert!(
            !names.iter().any(|name| name.contains(forbidden)),
            "the public contract must expose no `{forbidden}` name"
        );
    }
    // The one credential-shaped name the contract has is the opaque alias, which
    // is the whole stored reference and resolves to nothing without a policy that
    // already approves it. Asserting it is *the* one keeps the exception explicit
    // rather than letting the scan above quietly widen.
    // The comment mirror reports *that* a revision exists, who wrote it, when,
    // and what it hashes to — never its prose. `external_comment_id` is an
    // identifier and is allowed by name; anything else comment-shaped would be
    // the body arriving under a different spelling.
    let commentish: Vec<&String> = names
        .iter()
        .filter(|name| {
            name.split(['_', '-', '/', ':', '.'])
                .any(|part| part == "comment")
        })
        .collect();
    assert!(
        commentish
            .iter()
            .all(|name| name.as_str() == "external_comment_id"),
        "the only comment-shaped name may be the external identifier, found {commentish:?}"
    );
    // And the mirror's own schema is checked directly rather than through the
    // name sweep above: `body` is a legitimate property elsewhere in the
    // contract (a session message has one), so a global ban would prove nothing
    // about the place that must not grow one.
    let mirrored = document.json()["components"]["schemas"]["TicketCommentDto"]["properties"]
        .as_object()
        .expect("the comment mirror's schema")
        .keys()
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>();
    assert!(
        mirrored.iter().all(|property| !matches!(
            property.as_str(),
            "body" | "text" | "prose" | "content" | "rendered"
        )),
        "the comment mirror must carry no prose, found {mirrored:?}"
    );
    assert!(mirrored.iter().any(|property| property == "body_hash"));

    let credentialish: Vec<&String> = names
        .iter()
        .filter(|name| name.contains("credential"))
        .collect();
    assert!(
        credentialish
            .iter()
            .all(|name| name.as_str() == "credential_alias"),
        "the only credential-shaped name may be the opaque alias, found {credentialish:?}"
    );
}

/// Every property name declared anywhere under a schema map.
fn collect_property_names(schemas: &serde_json::Value, into: &mut Vec<String>) {
    match schemas {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if key == "properties"
                    && let Some(properties) = value.as_object()
                {
                    into.extend(properties.keys().map(|name| name.to_lowercase()));
                }
                collect_property_names(value, into);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_property_names(item, into);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn starting_a_named_plan_creates_one_seat_through_admission_and_reuses_it_on_replay() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;

    let created = ensure_project(&world, "start-1", "Kontor", "/tmp/kontor-start").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("start-account")
    .send(&world)
    .await;
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Started epic",
            &category,
            serde_json::json!([{"title": "Only task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("start-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Start the epic"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("start-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    assert_eq!(
        plan.json()["ready"].as_array().expect("ready").len(),
        1,
        "the armed task is ready: {}",
        plan.body
    );
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    // A plan hash this realm never produced is refused: a caller starts the batch
    // it was shown, not whatever the world looks like now.
    let stale = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": "0".repeat(64)}),
    )
    .signed_as(&world, "operator")
    .with_key("start-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);

    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("start-run")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].as_array().expect("seats").clone();
    // One seat per *declared* role slot, and no slot twice. A team that seated
    // only some of its roles could never be certified closed; one that seated a
    // role twice would have two sessions in one seat.
    assert!(
        !seats.is_empty(),
        "the admitted task was seated: {}",
        started.body
    );
    let slots: std::collections::BTreeSet<&str> = seats
        .iter()
        .map(|seat| seat["role_slot"].as_str().expect("a slot"))
        .collect();
    assert_eq!(slots.len(), seats.len(), "no role slot is seated twice");
    for seat in &seats {
        assert_eq!(seat["applied"], "created");
        assert_eq!(seat["team_run_id"], seats[0]["team_run_id"], "one team run");
    }
    let agent_run = seats[0]["agent_run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();
    let team_run = seats[0]["team_run_id"]
        .as_str()
        .expect("a team run id")
        .to_owned();

    // The seat is a real, addressable session — created by admission, never by a
    // public session-create route, because there is no such route.
    let timeline = Call::get(format!("/v1/sessions/{agent_run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(timeline.status, 200, "{}", timeline.body);

    // The epic projection now reports the seat.
    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let runs = projection.json()["tasks"][0]["team_runs"]
        .as_array()
        .expect("runs")
        .clone();
    assert_eq!(runs.len(), 1, "exactly one team run: {}", projection.body);
    assert_eq!(runs[0]["team_run_id"], team_run);
    // Every seat of the one team run is projected, and each is a real attached
    // session rather than a row standing in for one.
    let projected = runs[0]["seats"].as_array().expect("seats");
    assert_eq!(projected.len(), seats.len(), "{}", projection.body);
    assert!(
        projected
            .iter()
            .any(|seat| seat["agent_run_id"] == agent_run),
        "the run the start returned is one of them: {}",
        projection.body
    );
    for seat in projected {
        assert!(
            seat["attached"].as_bool().expect("a flag"),
            "this process holds the frozen snapshot for every seat it launched"
        );
    }
}

#[tokio::test]
async fn lifecycle_transitions_are_legal_revisioned_and_gated() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "life-1", "Kontor", "/tmp/kontor-life").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Lifecycle epic",
            &category,
            serde_json::json!([{"title": "Held task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("life-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let task_revision = applied.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("revision");

    let lifecycle = format!("/v1/projects/{project}/epics/{epic}/lifecycle");

    // A stale revision is refused, and the caller is told the current one.
    let stale = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "block", "task_id": task,
            "expected_revision": task_revision + 99, "reason": "Hold it"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("life-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");
    assert_eq!(
        stale.json()["current_revision"].as_u64(),
        Some(task_revision),
        "a refusal carries the revision the caller must present next"
    );

    let blocked = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "block", "task_id": task,
            "expected_revision": task_revision, "reason": "Hold it"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("life-block")
    .send(&world)
    .await;
    assert_eq!(blocked.status, 200, "{}", blocked.body);
    assert_eq!(blocked.json()["state"], "blocked");
    let held_revision = blocked.json()["revision"].as_u64().expect("revision");

    // Resume returns the task to ordinary scheduler eligibility. Nothing about it
    // touches a runtime: the task is simply eligible again.
    let resumed = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "resume", "task_id": task,
            "expected_revision": held_revision, "reason": "Carry on"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("life-resume")
    .send(&world)
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(resumed.json()["state"], "ready");

    // Closing the epic while a task is still open is refused.
    let premature = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "close_epic", "expected_revision": epic_revision,
            "reason": "Call it done"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("life-close-early")
    .send(&world)
    .await;
    assert_eq!(premature.status, 409, "{}", premature.body);

    // And a task-scoped action that names no task is refused before anything moves.
    let taskless = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "block", "expected_revision": 1, "reason": "Hold what?"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("life-taskless")
    .send(&world)
    .await;
    assert_eq!(taskless.status, 400, "{}", taskless.body);
    assert_eq!(taskless.code(), "invalid_request");
}

// ---------------------------------------------------------------------------
// Bootstrap idempotency
//
// Every mutation carries an `Idempotency-Key`, and the two bootstrap ensures are
// the ones that used to discard it. What is asserted here is the whole contract:
// the same key with the same body answers from the original receipt, the same key
// with a different body is a typed conflict, and neither produces a second row.
// ---------------------------------------------------------------------------

/// How many command receipts this Realm holds.
fn receipts(world: &World) -> i64 {
    world.daemon.state().with_store(|store| {
        store
            .unsettled_receipts()
            .expect("the receipts are readable")
            .len() as i64
    })
}

#[tokio::test]
async fn the_two_bootstrap_ensures_honour_their_idempotency_key() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let first = ensure_project(&world, "idem-1", "Kontor", "/tmp/kontor-idem").await;
    assert_eq!(first.status, 200, "{}", first.body);
    let project = first.json()["project_id"].as_str().expect("id").to_owned();
    let after_first = receipts(&world);
    assert_eq!(after_first, 1, "the ensure recorded exactly one receipt");

    // Same key, same body: the original answer, and no second receipt.
    let replay = ensure_project(&world, "idem-1", "Kontor", "/tmp/kontor-idem").await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["project_id"], first.json()["project_id"]);
    assert_eq!(replay.json()["applied"], "unchanged");
    assert_eq!(receipts(&world), after_first, "a replay records nothing");

    // Same key, different body: a typed conflict, and still nothing written.
    let reused = ensure_project(&world, "idem-1", "Kontor", "/tmp/kontor-other").await;
    assert_eq!(reused.status, 409, "{}", reused.body);
    assert_eq!(reused.code(), "idempotency_conflict");
    assert_eq!(receipts(&world), after_first);
    let projects = world.daemon.state().with_store(|store| {
        store
            .get_project(kontor_core::id::ProjectId::parse(&project).expect("a project id"))
            .expect("readable")
    });
    assert!(projects.is_some(), "the original project is untouched");

    // The account-profile ensure obeys the same three rules.
    let account = serde_json::json!({
        "label": "Lead", "harness": "fake.runtime",
        "credential_alias": "lead-alias", "enabled": true
    });
    let uri = format!("/v1/projects/{project}/provider-account-profiles:ensure");
    let created = Call::post(&uri, &account)
        .signed_as(&world, "admin")
        .with_key("idem-account")
        .send(&world)
        .await;
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.json()["applied"], "created");
    let with_account = receipts(&world);

    let replayed = Call::post(&uri, &account)
        .signed_as(&world, "admin")
        .with_key("idem-account")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(
        replayed.json()["account_profile_id"],
        created.json()["account_profile_id"]
    );
    assert_eq!(receipts(&world), with_account, "a replay records nothing");

    let conflicting = Call::post(
        &uri,
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "a-different-alias", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("idem-account")
    .send(&world)
    .await;
    assert_eq!(conflicting.status, 409, "{}", conflicting.body);
    assert_eq!(conflicting.code(), "idempotency_conflict");
}

#[tokio::test]
async fn an_account_ensure_compares_every_supplied_field_and_never_echoes_the_alias() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "drift-1", "Kontor", "/tmp/kontor-drift").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let uri = format!("/v1/projects/{project}/provider-account-profiles:ensure");

    let first = Call::post(
        &uri,
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "original-alias", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("drift-create")
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);

    // Every supplied identity or state field is compared under a *fresh* key, so
    // what is being asserted is drift detection and not key reuse.
    for (index, body) in [
        // A different approved alias.
        serde_json::json!({"label": "Lead", "harness": "fake.runtime",
                           "credential_alias": "other-alias", "enabled": true}),
        // A different launch policy.
        serde_json::json!({"label": "Lead", "harness": "fake.runtime",
                           "credential_alias": "original-alias", "enabled": false}),
        // A different runtime family.
        serde_json::json!({"label": "Lead", "harness": "other.runtime",
                           "credential_alias": "original-alias", "enabled": true}),
    ]
    .into_iter()
    .enumerate()
    {
        let drifted = Call::post(&uri, &body)
            .signed_as(&world, "admin")
            .with_key(format!("drift-{index}"))
            .send(&world)
            .await;
        assert_eq!(
            drifted.status, 409,
            "case {index} must be refused as drift: {}",
            drifted.body
        );
        // The refusal says a field disagreed and never which, because naming the
        // field would confirm a guessed alias.
        assert!(
            !drifted.body.contains("original-alias") && !drifted.body.contains("other-alias"),
            "a refusal must not echo an alias: {}",
            drifted.body
        );
    }

    // And an identical ensure under a fresh key is unchanged rather than drift.
    let same = Call::post(
        &uri,
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "original-alias", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("drift-same")
    .send(&world)
    .await;
    assert_eq!(same.status, 200, "{}", same.body);
    assert_eq!(same.json()["applied"], "unchanged");

    // The alias reaches no answer anywhere: not the create, not the replay, not
    // the list, and not the durable receipt.
    let listed = Call::get(format!("/v1/projects/{project}/provider-account-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    for body in [&first.body, &same.body, &listed.body] {
        assert!(
            !body.contains("original-alias"),
            "an alias must not appear in a response: {body}"
        );
    }
    let stored = world.daemon.state().with_store(|store| {
        let mut found = Vec::new();
        for (project_id, receipt_id) in store.unsettled_receipts().expect("readable") {
            let receipt = store
                .get_receipt(project_id, receipt_id)
                .expect("readable")
                .expect("the receipt exists");
            found.push(receipt.intent.json().to_owned());
        }
        found
    });
    assert!(
        stored
            .iter()
            .all(|intent| !intent.contains("original-alias")),
        "an alias must not be persisted in a receipt intent: {stored:?}"
    );
    assert!(
        stored
            .iter()
            .any(|intent| intent.contains("credential_alias_digest")),
        "the intent still distinguishes two aliases, by digest"
    );
}

#[tokio::test]
async fn disarming_records_its_own_command_kind_and_checks_the_key_before_answering() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "revoke-1", "Kontor", "/tmp/kontor-revoke").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("revoke-account")
    .send(&world)
    .await;
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Revoked epic",
            &category,
            serde_json::json!([{"title": "Only task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("revoke-epic")
    .send(&world)
    .await;
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 10, "max_commands": 10, "max_duration_seconds": 10,
                       "max_cost_minor_units": 10, "cost_currency": "NOK"},
            "granted_by": account_id, "reason": "Arm it"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("revoke-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);
    let authorization = armed.json()["authorization_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let disarm_uri = format!("/v1/projects/{project}/epics/{epic}/execution:disarm");
    let disarm_body = |reason: &str| {
        serde_json::json!({
            "authorization_id": authorization,
            "revoked_by": account_id,
            "reason": reason
        })
    };

    let revoked = Call::post(&disarm_uri, &disarm_body("Stand down"))
        .signed_as(&world, "admin")
        .with_key("revoke-key")
        .send(&world)
        .await;
    assert_eq!(revoked.status, 200, "{}", revoked.body);
    assert!(revoked.json()["revoked_at"].is_string());

    // The receipt is its own kind: a calendar-override revocation must not be
    // replayable as the authority that disarmed the work.
    let kinds = world.daemon.state().with_store(|store| {
        let mut found = Vec::new();
        for (project_id, receipt_id) in store.unsettled_receipts().expect("readable") {
            let receipt = store
                .get_receipt(project_id, receipt_id)
                .expect("readable")
                .expect("the receipt exists");
            found.push(receipt.kind.as_str().to_owned());
        }
        found
    });
    assert!(
        kinds
            .iter()
            .any(|kind| kind == "revoke_execution_authorization"),
        "disarm records its own command kind, found {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|kind| kind == "revoke_schedule_override"),
        "disarming an authorization is not a calendar override revocation"
    );

    // An already-revoked authorization still validates the key first: a *changed*
    // request under a used key is a conflict, not a replay of the original.
    let masquerade = Call::post(&disarm_uri, &disarm_body("A different reason entirely"))
        .signed_as(&world, "admin")
        .with_key("revoke-key")
        .send(&world)
        .await;
    assert_eq!(masquerade.status, 409, "{}", masquerade.body);
    assert_eq!(masquerade.code(), "idempotency_conflict");

    // And the true replay answers with the revocation that already happened.
    let replay = Call::post(&disarm_uri, &disarm_body("Stand down"))
        .signed_as(&world, "admin")
        .with_key("revoke-key")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert!(replay.json()["revoked_at"].is_string());
}

// ---------------------------------------------------------------------------
// The Lead-required control and evidence operations
// ---------------------------------------------------------------------------

/// One bootstrapped project, epic and account, ready for a task-scoped test.
struct Bootstrapped {
    project: String,
    epic: String,
    task: String,
    task_revision: u64,
    account: String,
}

/// Bring an empty realm to "one epic with one task", the shortest state in which
/// every task-scoped operation is addressable.
async fn bootstrap(world: &World, slug: &'static str) -> Bootstrapped {
    let created = ensure_project(world, slug, "Kontor", &format!("/tmp/kontor-{slug}")).await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(world, "admin")
    .with_key(format!("{slug}-bootstrap-account"))
    .send(world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Control epic",
            &category,
            serde_json::json!([{"title": "The task"}]),
        ),
    )
    .signed_as(world, "admin")
    .with_key(format!("{slug}-epic"))
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    Bootstrapped {
        project,
        epic: applied.json()["epic_id"].as_str().expect("id").to_owned(),
        task: applied.json()["tasks"][0]["task_id"]
            .as_str()
            .expect("id")
            .to_owned(),
        task_revision: applied.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("revision"),
        account: account.json()["account_profile_id"]
            .as_str()
            .expect("id")
            .to_owned(),
    }
}

#[tokio::test]
async fn resolving_a_task_context_is_deterministic_and_returns_no_content() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "ctx").await;
    let uri = format!(
        "/v1/projects/{}/tasks/{}/context:resolve",
        seed.project, seed.task
    );

    let first = Call::post(&uri, &serde_json::json!({"snapshot": false}))
        .signed_as(&world, "operator")
        .with_key("ctx-preview-1")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let hash = first.json()["context_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    assert_eq!(hash.len(), 64, "the hash is a content digest");
    assert!(
        first.json()["context_pack_id"].is_null(),
        "a preview freezes nothing"
    );
    assert!(
        !first.json()["provenance"]
            .as_array()
            .expect("provenance")
            .is_empty(),
        "every resolved path is attributable: {}",
        first.body
    );

    // Same task, same pins, same bytes: a preview is a pure function of what the
    // task is, so a caller can compare two of them.
    let again = Call::post(&uri, &serde_json::json!({"snapshot": false}))
        .signed_as(&world, "operator")
        .with_key("ctx-preview-2")
        .send(&world)
        .await;
    assert_eq!(again.json()["context_hash"], hash);

    // The merged content itself never leaves the process.
    assert!(
        first.json().get("resolved").is_none() && first.json().get("content").is_none(),
        "a resolution returns its digest and its provenance, never the document"
    );

    // A snapshot needs a run to belong to, and this task has none.
    let premature = Call::post(&uri, &serde_json::json!({"snapshot": true}))
        .signed_as(&world, "operator")
        .with_key("ctx-snapshot")
        .send(&world)
        .await;
    assert_eq!(premature.status, 422, "{}", premature.body);
    assert_eq!(premature.code(), "unsupported_capability");

    // Observers may not resolve; the operation reads pins and freezes evidence.
    let observer = Call::post(&uri, &serde_json::json!({"snapshot": false}))
        .signed_as(&world, "observer")
        .with_key("ctx-observer")
        .send(&world)
        .await;
    assert_eq!(observer.status, 403);
}

#[tokio::test]
async fn a_gate_verdict_is_append_only_authority_checked_and_waiver_is_admin() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "gate").await;

    // The gate and the role come from the task's *pinned profile*, read back
    // through the public projection rather than named by the test.
    let projection = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let task = projection.json()["tasks"][0].clone();
    let gate = task["gates"].as_array().expect("a gate list")[0]["gate"]
        .as_str()
        .expect("the pinned profile declares at least one gate")
        .to_owned();
    // The revision a gate recording must present is the *workflow's*, and it is
    // read from the projection like everything else. Assuming it is 1 would be
    // right only until the first phase advance, and would be exactly the
    // out-of-band knowledge this suite is meant to prove unnecessary.
    let workflow_revision = task["workflow_revision"]
        .as_u64()
        .expect("a task with an active workflow reports its revision");
    let uri = format!(
        "/v1/projects/{}/tasks/{}/gates/{gate}/record",
        seed.project, seed.task
    );

    // A role the pinned profile does not authorize for this gate is refused, and
    // the refusal is the domain's, not the transport's.
    let unauthorized = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": workflow_revision,
            "verdict": "rejected",
            "evaluator_role": "nobody-in-particular",
            "evaluator_account": seed.account,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("gate-unauthorized")
    .send(&world)
    .await;
    assert_eq!(unauthorized.status, 403, "{}", unauthorized.body);
    assert_eq!(unauthorized.code(), "forbidden");

    // Waiving is admin authority, checked before the service is reached.
    let waived_by_operator = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": workflow_revision,
            "verdict": "waived",
            "evaluator_role": "nobody-in-particular",
            "evaluator_account": seed.account,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("gate-waive-operator")
    .send(&world)
    .await;
    assert_eq!(
        waived_by_operator.status, 403,
        "{}",
        waived_by_operator.body
    );

    // A stale workflow revision is refused before anything is appended.
    let stale = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": workflow_revision + 99,
            "verdict": "rejected",
            "evaluator_role": "nobody-in-particular",
            "evaluator_account": seed.account,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("gate-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");
}

#[tokio::test]
async fn selection_corrections_are_pre_run_admin_decisions() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "sel").await;
    let category = first_category(&world).await;

    // A profile correction to the profile already pinned is unchanged, not a
    // second workflow.
    let profile_uri = format!(
        "/v1/projects/{}/tasks/{}/profile-selection",
        seed.project, seed.task
    );
    let same = Call::post(
        &profile_uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": category,
            "reason": "Confirm the pin"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("sel-profile-same")
    .send(&world)
    .await;
    assert_eq!(same.status, 200, "{}", same.body);
    assert_eq!(same.json()["applied"], "unchanged");

    // An operator may not correct a selection.
    let operator = Call::post(
        &profile_uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": category,
            "reason": "Not mine to make"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("sel-profile-operator")
    .send(&world)
    .await;
    assert_eq!(operator.status, 403);

    // The team a task runs is its profile's pin: confirming it succeeds, and a
    // mismatch is refused rather than silently substituted.
    let team = same.json()["team_template"].clone();
    let team_uri = format!(
        "/v1/projects/{}/tasks/{}/team-selection",
        seed.project, seed.task
    );
    let confirmed = Call::post(
        &team_uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "team_template": team,
            "reason": "Confirm the team"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("sel-team-ok")
    .send(&world)
    .await;
    assert_eq!(confirmed.status, 200, "{}", confirmed.body);
    assert_eq!(confirmed.json()["team_template"], team);

    let mismatched = Call::post(
        &team_uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "team_template": {"id": kontor_core::id::TeamTemplateId::generate().to_string(),
                              "version": 1},
            "reason": "Some other team"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("sel-team-drift")
    .send(&world)
    .await;
    assert_eq!(mismatched.status, 409, "{}", mismatched.body);

    // An account correction capability-checks the runtime and stores only the
    // profile id and revision.
    let account_uri = format!(
        "/v1/projects/{}/tasks/{}/account-selection",
        seed.project, seed.task
    );
    let pinned = Call::post(
        &account_uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "account_profile_id": seed.account,
            "reason": "Run as the lead"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("sel-account")
    .send(&world)
    .await;
    assert_eq!(pinned.status, 200, "{}", pinned.body);
    assert_eq!(pinned.json()["account_profile_id"], seed.account);
    assert!(
        !pinned.body.contains("lead\""),
        "an account selection must not echo the alias: {}",
        pinned.body
    );

    // An unknown profile is not found, and a stale task revision is a conflict.
    let unknown = Call::post(
        &account_uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "account_profile_id": kontor_core::id::AccountProfileId::generate().to_string(),
            "reason": "Nobody"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("sel-account-unknown")
    .send(&world)
    .await;
    assert_eq!(unknown.status, 404, "{}", unknown.body);
}

#[tokio::test]
async fn ticket_reconciliation_is_a_typed_dry_run_that_names_its_plan() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "tix").await;
    let plan_uri = format!(
        "/v1/projects/{}/tasks/{}/ticket:reconcile-plan",
        seed.project, seed.task
    );

    let plan = Call::post(&plan_uri, &serde_json::json!({}))
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    let hash = plan.json()["projection_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    assert_eq!(hash.len(), 64);
    assert!(
        plan.json()["converged"].as_bool().expect("a flag"),
        "a task with no links has nothing to converge: {}",
        plan.body
    );
    // The plan carries typed milestones and nothing a caller could use to write
    // an arbitrary status, assignee or comment.
    for forbidden in ["assignee", "comment", "status"] {
        assert!(
            !plan.body.contains(forbidden),
            "the plan must not carry `{forbidden}`: {}",
            plan.body
        );
    }

    let apply_uri = format!(
        "/v1/projects/{}/tasks/{}/ticket:reconcile-apply",
        seed.project, seed.task
    );
    // A plan hash this realm never produced is refused.
    let stale = Call::post(
        &apply_uri,
        &serde_json::json!({"projection_hash": "0".repeat(64)}),
    )
    .signed_as(&world, "operator")
    .with_key("tix-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);

    let applied = Call::post(&apply_uri, &serde_json::json!({"projection_hash": hash}))
        .signed_as(&world, "operator")
        .with_key("tix-apply")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(
        applied.json()["projection_hash"],
        plan.json()["projection_hash"]
    );
}

#[tokio::test]
async fn a_started_task_leaves_ready_so_it_can_legally_be_completed() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "close").await;

    let armed = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/execution:arm",
            seed.project, seed.epic
        ),
        &serde_json::json!({
            "expected_revision": 1, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 10, "max_commands": 10, "max_duration_seconds": 10,
                       "max_cost_minor_units": 10, "cost_currency": "NOK"},
            "granted_by": seed.account, "reason": "Arm it"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("close-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:plan",
            seed.project, seed.epic
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    let hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    let started = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:start",
            seed.project, seed.epic
        ),
        &serde_json::json!({"plan_hash": hash}),
    )
    .signed_as(&world, "operator")
    .with_key("close-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        !started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "the team run seats every role its template declares: {}",
        started.body
    );

    // The whole point: a started task is *in progress*, which is the only state
    // completion is reachable from. A task left in `ready` could be started and
    // then never legally finished.
    let projection = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(projection.json()["tasks"][0]["state"], "in_progress");

    // Closing the epic is refused while its task is non-terminal, and the refusal
    // is the domain gate rather than a missing route.
    let premature = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/lifecycle",
            seed.project, seed.epic
        ),
        &serde_json::json!({
            "action": "close_epic", "expected_revision": 1, "reason": "Too early"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("close-early")
    .send(&world)
    .await;
    assert_eq!(premature.status, 409, "{}", premature.body);

    // And completing the task is refused for the *stated* reason — its team run
    // has not closed, which is settlement the operator has not done yet rather
    // than a missing operation. `runtime:settle` is the way past it.
    let task_revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let complete = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/lifecycle",
            seed.project, seed.epic
        ),
        &serde_json::json!({
            "action": "complete_task", "task_id": seed.task,
            "expected_revision": task_revision, "reason": "Done"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("close-complete")
    .send(&world)
    .await;
    assert_eq!(complete.status, 422, "{}", complete.body);
    assert_eq!(complete.code(), "unsupported_capability");
}

#[tokio::test]
async fn a_teamless_task_completes_reopens_and_lets_its_epic_close_and_reopen() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "term").await;
    let lifecycle = format!(
        "/v1/projects/{}/epics/{}/lifecycle",
        seed.project, seed.epic
    );

    // No scheduler start, so no team run: this is the close-out path a task that
    // was completed outside a Kontor seat takes, and it is the one the domain lets
    // a client drive end to end today.
    let cancelled = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "block", "task_id": seed.task,
            "expected_revision": seed.task_revision, "reason": "Hold"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("term-block")
    .send(&world)
    .await;
    assert_eq!(cancelled.status, 200, "{}", cancelled.body);
    let held = cancelled.json()["revision"].as_u64().expect("a revision");

    // Resume needs a command receipt as its authority, and the operation supplies
    // one: a resume that could happen without it would be a task leaving a held
    // state on nobody's say-so.
    let resumed = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "resume", "task_id": seed.task,
            "expected_revision": held, "reason": "Carry on"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("term-resume")
    .send(&world)
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(resumed.json()["state"], "ready");

    // Closing the epic is still refused: `ready` is not terminal.
    let refused = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "close_epic", "expected_revision": 1, "reason": "Not yet"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("term-close-early")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert!(
        refused.body.contains("terminal"),
        "the refusal names the rule: {}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// Runtime settlement
//
// The one operation that lets a client drive a seated task to a terminal epic
// through public operations alone. What is asserted below is mostly what it
// *refuses*: an outcome the caller cannot supply, a closure the runtime has not
// evidenced, and a second observation on a run already settled.
// ---------------------------------------------------------------------------

/// A script whose one cancel reports an authoritatively observed termination.
///
/// It is how the *runtime* is made to finish: the fake only reaches a terminal
/// session through a cancel it observed, so a test that needs a finished session
/// drives the runtime there and then asks Kontor to settle. Nothing here reaches
/// into Kontor — the runtime is simply told what to be.
const OBSERVED_TERMINAL: &str = r#"{
  "history": [
    {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "working"}
  ],
  "steps": [{"step": "cancel_observed_terminal"}]
}"#;

/// Drive the scripted runtime's session for `run` to a terminal state.
///
/// The step is queued here, immediately before the cancel, rather than at the top
/// of the test: the fake matches its queue strictly by operation, so a cancel step
/// loaded earlier would be consumed by whichever of `prepare_workspace`,
/// `admit_launch` or `launch` reached the runtime first.
async fn finish_natively(world: &World, run: &str) {
    world.script(OBSERVED_TERMINAL);
    let agent_run_id = AgentRunId::parse(run).expect("an agent run id");
    let binding = world.daemon.state().with_store(|store| {
        store
            .snapshot_run_inspection(agent_run_id)
            .expect("readable")
            .open(world.realm_id())
            .expect("our own realm")
            .expect("the run exists")
            .run
            .binding
            .expect("the run is bound")
    });
    let snapshot = world
        .daemon
        .state()
        .sessions()
        .get(binding.id)
        .expect("this process holds the frozen snapshot");
    world
        .fake
        .cancel(&kontor_runtime::request::CancelRequest {
            binding: snapshot,
            requested_at: at("2026-08-10T09:30:00Z"),
        })
        .await
        .expect("the runtime observes its own termination");
}

/// Bootstrap, arm, plan and start, returning `(seed, every seated run)`.
async fn seated(world: &World, slug: &'static str) -> (Bootstrapped, Vec<String>) {
    let seed = bootstrap(world, slug).await;
    let armed = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/execution:arm",
            seed.project, seed.epic
        ),
        &serde_json::json!({
            "expected_revision": 1, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 10, "max_commands": 10, "max_duration_seconds": 10,
                       "max_cost_minor_units": 10, "cost_currency": "NOK"},
            "granted_by": seed.account, "reason": "Arm it"
        }),
    )
    .signed_as(world, "admin")
    .with_key(format!("{slug}-arm"))
    .send(world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:plan",
            seed.project, seed.epic
        ),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    let hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let started = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:start",
            seed.project, seed.epic
        ),
        &serde_json::json!({"plan_hash": hash}),
    )
    .signed_as(world, "operator")
    .with_key(format!("{slug}-start"))
    .send(world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let runs: Vec<String> = started.json()["started"]
        .as_array()
        .expect("seats")
        .iter()
        .map(|seat| {
            seat["agent_run_id"]
                .as_str()
                .expect("an agent run id")
                .to_owned()
        })
        .collect();
    assert!(
        !runs.is_empty(),
        "the start produced no seat: {}",
        started.body
    );
    (seed, runs)
}

#[tokio::test]
async fn settling_a_run_takes_a_fresh_inspect_and_never_a_supplied_verdict() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "settle").await;
    let run = runs[0].clone();
    finish_natively(&world, &run).await;
    let uri = format!(
        "/v1/projects/{}/agent-runs/{run}/runtime:settle",
        seed.project
    );

    let before = world.fake.calls().len();
    let settled = Call::post(&uri, &serde_json::json!({}))
        .signed_as(&world, "operator")
        .with_key("settle-1")
        .send(&world)
        .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert_eq!(settled.json()["applied"], "created");
    assert_eq!(settled.json()["outcome"], "cancelled");
    assert!(
        settled.json()["evidence_cursor"].is_i64(),
        "the closure cites a position in this realm's own log: {}",
        settled.body
    );
    // The runtime was actually asked. A settlement that concluded from a cached
    // projection would close a run on a description of the past.
    assert!(
        world
            .fake
            .calls()
            .iter()
            .skip(before)
            .any(|call| matches!(call, AdapterCall::Inspect { .. })),
        "settlement takes a fresh inspect: {:?}",
        world.fake.calls()
    );

    // The run is closed, and the closure points at a stored observation rather
    // than at anything the caller said.
    let snapshot = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(snapshot.status, 200, "{}", snapshot.body);
    assert_eq!(
        snapshot.json()["value"]["projection"]["derived"],
        "terminal"
    );
    assert_eq!(
        snapshot.json()["value"]["projection"]["outcome"],
        "cancelled"
    );

    // Idempotent: the same key replays, and a *fresh* key still takes no second
    // observation because the run is already settled.
    let calls = world.fake.calls().len();
    for key in ["settle-1", "settle-2"] {
        let again = Call::post(&uri, &serde_json::json!({}))
            .signed_as(&world, "operator")
            .with_key(key)
            .send(&world)
            .await;
        assert_eq!(again.status, 200, "{key}: {}", again.body);
        assert_eq!(again.json()["applied"], "unchanged");
        assert_eq!(again.json()["outcome"], "cancelled");
    }
    assert_eq!(
        world.fake.calls().len(),
        calls,
        "a settled run is not inspected again"
    );

    // The operation takes no body that could carry an outcome: a request that
    // tries to supply one is ignored, not honoured.
    let smuggled = Call::post(
        &uri,
        &serde_json::json!({"outcome": "failed", "terminal_state": "failed",
                            "evidence_hash": "0".repeat(64)}),
    )
    .signed_as(&world, "operator")
    .with_key("settle-smuggled")
    .send(&world)
    .await;
    assert_eq!(smuggled.status, 200, "{}", smuggled.body);
    assert_eq!(
        smuggled.json()["outcome"],
        "cancelled",
        "the runtime's verdict stands, not the caller's"
    );

    // Observers may not settle: it closes a run.
    let observer = Call::post(&uri, &serde_json::json!({}))
        .signed_as(&world, "observer")
        .with_key("settle-observer")
        .send(&world)
        .await;
    assert_eq!(observer.status, 403);
}

#[tokio::test]
async fn a_run_the_runtime_says_is_still_working_is_not_settled() {
    let world = World::open_empty().await;
    // The default script reports a live session, so `inspect` answers with a
    // non-terminal state.
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "live").await;
    let run = runs[0].clone();

    let refused = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run}/runtime:settle",
            seed.project
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("live-settle")
    .send(&world)
    .await;
    assert_eq!(refused.status, 422, "{}", refused.body);
    assert_eq!(refused.code(), "unsupported_capability");

    // And the run is still open: an uncertain answer closes nothing.
    let snapshot = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_ne!(
        snapshot.json()["value"]["projection"]["derived"],
        "terminal"
    );
}

#[tokio::test]
async fn settlement_closes_the_team_and_unlocks_the_whole_epic_close_out() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "endgame").await;

    // Every declared seat is settled, one call each. The team closes on the last
    // one, because the closure walks the template's declared slots and an
    // unsettled seat is unaccounted for rather than absent.
    let mut settled = None;
    for (index, run) in runs.iter().enumerate() {
        finish_natively(&world, run).await;
        let answer = Call::post(
            format!(
                "/v1/projects/{}/agent-runs/{run}/runtime:settle",
                seed.project
            ),
            &serde_json::json!({}),
        )
        .signed_as(&world, "operator")
        .with_key(format!("endgame-settle-{index}"))
        .send(&world)
        .await;
        assert_eq!(answer.status, 200, "seat {index}: {}", answer.body);
        settled = Some(answer);
    }
    let settled = settled.expect("at least one seat was settled");
    // Every declared role slot is terminal, so the team's closure was certified
    // from the frozen template rather than asserted by anyone.
    assert!(
        settled.json()["team_run_closed"].is_string(),
        "the team run closes once its declared slots are done: {}",
        settled.body
    );
    assert!(settled.json()["team_pending"].is_null());

    // The task can now be completed: it cites the certified team closure, which
    // the store re-proves against its own rows.
    let projection = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let lifecycle = format!(
        "/v1/projects/{}/epics/{}/lifecycle",
        seed.project, seed.epic
    );
    let completed = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "complete_task", "task_id": seed.task,
            "expected_revision": task_revision, "reason": "The work is done"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("endgame-complete")
    .send(&world)
    .await;
    // Before any gate is recorded the task stops on its pinned profile's own
    // closure — not on a missing team certificate. `unsupported_capability` here
    // would mean the certificate is still absent; a domain refusal about profile
    // closure means it was derived, cited and re-proved by the store, and the
    // task stopped on its own declared work.
    assert_ne!(
        completed.code(),
        "unsupported_capability",
        "the team closure certificate is derived and cited, not missing: {}",
        completed.body
    );
    assert_eq!(
        completed.status, 400,
        "the task stops on its pinned profile's own closure: {}",
        completed.body
    );

    // Now discharge that profile. Every gate it declares is recorded through the
    // public route, by a role *it* authorizes, citing the evidence *it* requires —
    // all of which the projection reports, so nothing here is read out of band and
    // nothing is a literal this test invented.
    let gates = projection.json()["tasks"][0]["gates"]
        .as_array()
        .expect("a gate list")
        .clone();
    assert!(
        !gates.is_empty(),
        "the pinned profile declares gates to discharge: {}",
        projection.body
    );
    let workflow_revision = projection.json()["tasks"][0]["workflow_revision"]
        .as_u64()
        .expect("a task with an active workflow reports its revision");
    for (index, gate) in gates.iter().enumerate() {
        let name = gate["gate"].as_str().expect("a gate");
        let evaluator = gate["evaluator_roles"]
            .as_array()
            .expect("declared evaluators")
            .first()
            .and_then(serde_json::Value::as_str)
            .expect("the profile authorizes a role for every gate it declares");
        let evidence: Vec<&str> = gate["required_evidence"]
            .as_array()
            .expect("declared evidence")
            .iter()
            .map(|artifact| artifact.as_str().expect("an artifact"))
            .collect();
        let recorded = Call::post(
            format!(
                "/v1/projects/{}/tasks/{}/gates/{name}/record",
                seed.project, seed.task
            ),
            &serde_json::json!({
                "expected_revision": workflow_revision,
                "verdict": "passed",
                "evaluator_role": evaluator,
                "evaluator_account": seed.account,
                "evidence": evidence,
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("endgame-gate-{index}"))
        .send(&world)
        .await;
        assert_eq!(recorded.status, 200, "gate `{name}`: {}", recorded.body);
        assert_eq!(recorded.json()["verdict"], "passed");
        assert_eq!(recorded.json()["state"], "passed", "gate `{name}` reduced");
    }

    // Every gate now reads as passed through the public projection.
    let after_gates = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    for gate in after_gates.json()["tasks"][0]["gates"]
        .as_array()
        .expect("a gate list")
    {
        assert_eq!(
            gate["state"], "passed",
            "gate `{}` is discharged: {}",
            gate["gate"], after_gates.body
        );
    }

    // The completion cites every artifact the profile requires — again read from
    // the projection rather than named here.
    let after = after_gates.json();
    let artifacts: Vec<&str> = after["tasks"][0]["required_artifacts"]
        .as_array()
        .expect("required artifacts")
        .iter()
        .map(|artifact| artifact.as_str().expect("an artifact"))
        .collect();
    assert!(
        !artifacts.is_empty(),
        "the profile requires artifacts: {}",
        after_gates.body
    );
    let task_revision = after["tasks"][0]["revision"].as_u64().expect("a revision");
    let done = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "complete_task", "task_id": seed.task,
            "expected_revision": task_revision, "reason": "The work is done",
            "evidence": artifacts,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("endgame-complete-final")
    .send(&world)
    .await;
    assert_eq!(done.status, 200, "{}", done.body);
    assert_eq!(done.json()["state"], "done");

    // And with every task terminal and every team run closed, the epic closes.
    let closed = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "close_epic", "expected_revision": 1, "reason": "Epic complete"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("endgame-close")
    .send(&world)
    .await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.json()["state"], "closed");
}

#[tokio::test]
async fn a_profile_category_reports_its_whole_shape_and_validates_itself() {
    let world = World::open_empty().await;
    let category = first_category(&world).await;

    let detail = Call::get(format!("/v1/catalog/work-profiles/{category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(detail.status, 200, "{}", detail.body);
    let body = detail.json();

    // The catalog entry and the detail must agree about the revision and the
    // digest: two reads of the same category that disagreed would mean a caller
    // pinning from one and freezing from the other.
    let catalog = Call::get("/v1/catalog/work-profiles")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let entry = catalog
        .json()
        .as_array()
        .expect("a catalog")
        .iter()
        .find(|entry| entry["category"] == serde_json::json!(category))
        .cloned()
        .expect("the category the catalog advertised");
    assert_eq!(body["profile"], entry["profile"]);
    assert_eq!(body["team"], entry["team"]);
    // The *definition* digest is the stable one and is asserted as such. The
    // bundle digest deliberately is not: it covers the resolution, which records
    // when it happened, so two reads of an unchanged category answer with two
    // different bundle digests. Asserting equality there would be asserting that
    // the two reads happened in the same instant.
    assert_ne!(
        body["bundle_hash"], entry["bundle_hash"],
        "a bundle digest covers its own resolution time"
    );
    assert_eq!(
        body["definition_hash"].as_str().expect("a digest").len(),
        64
    );

    // Everything the closure sequence needs is here, which is the point of the
    // route: a Lead learns the gate authority and the artifact contracts without
    // reading the pack out of band.
    let phases = body["phases"].as_array().expect("phases");
    assert!(!phases.is_empty(), "{}", detail.body);
    assert!(
        phases
            .iter()
            .any(|phase| phase["phase"] == body["entry_phase"]),
        "the entry phase must be one of the declared phases: {}",
        detail.body
    );
    let gates = body["gates"].as_array().expect("gates");
    assert!(!gates.is_empty(), "{}", detail.body);
    for gate in gates {
        assert_eq!(gate["state"], "not_ready", "a profile has run nothing");
        assert!(
            !gate["evaluator_roles"]
                .as_array()
                .expect("roles")
                .is_empty(),
            "every gate names who may judge it: {}",
            detail.body
        );
    }
    assert!(!body["artifacts"].as_array().expect("artifacts").is_empty());

    // The eligible roots are exactly the slots no handoff feeds — the same rule
    // the seating uses — so what the API reports and what a start actually does
    // cannot drift apart.
    let downstream: Vec<&serde_json::Value> = body["handoffs"]
        .as_array()
        .expect("handoffs")
        .iter()
        .map(|handoff| &handoff["to_slot"])
        .collect();
    for root in body["eligible_roots"].as_array().expect("roots") {
        assert!(
            !downstream.contains(&root),
            "a root is fed by no handoff: {}",
            detail.body
        );
    }

    let validated = Call::post(
        format!("/v1/catalog/work-profiles/{category}/validate"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(validated.status, 200, "{}", validated.body);
    assert_eq!(validated.json()["availability"], "seeded");
    assert_eq!(validated.json()["pack_valid"], true);
    assert_eq!(validated.json()["bundle_verified"], true);
    assert!(
        validated.json()["bundle_hash"].is_string(),
        "{}",
        validated.body
    );
    assert!(validated.json()["refused"].is_null(), "{}", validated.body);

    // A category this build does not advertise is absent, not empty: an empty
    // answer would say "this profile declares no gates".
    let unknown = Call::get("/v1/catalog/work-profiles/no-such-category")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(unknown.status, 404, "{}", unknown.body);
}

#[tokio::test]
async fn intake_decides_under_a_pinned_trigger_or_reports_it_is_not_installed() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "intake").await;

    // An empty realm has no trigger revisions, and says so. This is the honest
    // shape of the gap: submitting under a trigger nothing installed cannot
    // decide anything, and answering `ignored` would be a decision.
    let missing = Call::get(format!(
        "/v1/projects/{}/triggers/nightly-sweep/1",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(missing.status, 404, "{}", missing.body);
    assert_eq!(missing.code(), "not_found");

    let submitted = Call::post(
        format!("/v1/projects/{}/intake:submit", seed.project),
        &serde_json::json!({
            "trigger": "nightly-sweep",
            "trigger_version": 1,
            "external_event_id": "EVT-1",
            "external_observed_at": "2026-08-13T09:00:00Z",
            "envelope": {"schema_version": 1, "kind": "push"}
        }),
    )
    .signed_as(&world, "operator")
    .with_key("intake-submit-1")
    .send(&world)
    .await;
    assert_eq!(submitted.status, 404, "{}", submitted.body);

    // An unknown decision id is absent rather than fabricated.
    let unknown = Call::get(format!(
        "/v1/projects/{}/intake/0199a0a0-0000-7000-8000-000000000000",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(unknown.status, 404, "{}", unknown.body);

    // Submitting is an operator decision, and an observer may not make it.
    let refused = Call::post(
        format!("/v1/projects/{}/intake:submit", seed.project),
        &serde_json::json!({
            "trigger": "nightly-sweep",
            "trigger_version": 1,
            "external_event_id": "EVT-2",
            "external_observed_at": "2026-08-13T09:00:00Z",
            "envelope": {"schema_version": 1}
        }),
    )
    .signed_as(&world, "observer")
    .with_key("intake-submit-2")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
}

#[tokio::test]
async fn a_connector_reports_the_specifications_this_build_ships() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "specs").await;

    let fields = Call::get(format!(
        "/v1/projects/{}/connectors/connector.jira/field-specs",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(fields.status, 200, "{}", fields.body);
    let fields = fields.json();
    let fields = fields.as_array().expect("a spec list");
    assert!(
        !fields.is_empty(),
        "this build ships a bundled field mapping"
    );
    for spec in fields {
        assert_eq!(spec["connector"], "connector.jira");
        assert!(!spec["covers"].as_array().expect("covers").is_empty());
        assert!(!spec["definition_hash"].as_str().expect("hash").is_empty());
        // Nothing was installed into this project, and the read says so rather
        // than implying the mapping is already pinned here.
        assert_eq!(spec["installed"], false, "{spec}");
    }

    let workflows = Call::get(format!(
        "/v1/projects/{}/connectors/connector.jira/workflow-specs",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(workflows.status, 200, "{}", workflows.body);
    let workflows = workflows.json();
    let workflows = workflows.as_array().expect("a spec list");
    assert!(!workflows.is_empty());
    for spec in workflows {
        assert!(
            !spec["covers"].as_array().expect("covers").is_empty(),
            "a workflow mapping declares the milestones it converges: {spec}"
        );
    }

    // A connector this build has no mapping for is an empty list, not a 404:
    // the connector key is open vocabulary, and "we ship nothing for it" is a
    // complete answer.
    let other = Call::get(format!(
        "/v1/projects/{}/connectors/connector.unknown/field-specs",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(other.status, 200, "{}", other.body);
    assert_eq!(other.json().as_array().expect("a list").len(), 0);
}

#[tokio::test]
async fn conflicts_comments_and_claims_are_task_scoped_and_disclose_no_content() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "tickets").await;

    // A task nothing has reconciled holds no conflicts and no comments, and both
    // reads say that with an empty list rather than a refusal.
    let conflicts = Call::get(format!(
        "/v1/projects/{}/tasks/{}/ticket:conflicts",
        seed.project, seed.task
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(conflicts.status, 200, "{}", conflicts.body);
    assert_eq!(conflicts.json().as_array().expect("a list").len(), 0);

    let comments = Call::get(format!(
        "/v1/projects/{}/tasks/{}/ticket:comments",
        seed.project, seed.task
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(comments.status, 200, "{}", comments.body);
    assert_eq!(comments.json().as_array().expect("a list").len(), 0);

    // Resolving a conflict that was never raised is absent, not a silent no-op
    // that would hand back a receipt for a decision nobody could have made.
    let phantom = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/ticket:resolve-conflict",
            seed.project, seed.task
        ),
        &serde_json::json!({"conflict_id": "0199a0a0-0000-7000-8000-000000000001"}),
    )
    .signed_as(&world, "operator")
    .with_key("tickets-resolve-1")
    .send(&world)
    .await;
    assert_eq!(phantom.status, 404, "{}", phantom.body);

    // A task with no links has nothing to pull, and that is a legitimate zero
    // rather than a claim about an external system nothing contacted.
    let pulled = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/ticket:pull-comments",
            seed.project, seed.task
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("tickets-pull-1")
    .send(&world)
    .await;
    assert_eq!(pulled.status, 200, "{}", pulled.body);
    assert_eq!(pulled.json()["mirrored"], 0);
    assert_eq!(pulled.json()["held"], 0);
    assert!(
        !pulled.json()["receipt_id"]
            .as_str()
            .expect("a receipt")
            .is_empty()
    );

    // Replaying the key answers from the receipt already recorded rather than
    // recording a second one.
    let replayed = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/ticket:pull-comments",
            seed.project, seed.task
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("tickets-pull-1")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt_id"], pulled.json()["receipt_id"]);

    // Claiming a task that is linked to nothing has nothing to claim.
    let unclaimable = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/ticket:claim",
            seed.project, seed.task
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("tickets-claim-1")
    .send(&world)
    .await;
    assert_eq!(unclaimable.status, 404, "{}", unclaimable.body);

    // Every one of these is an operator decision or an observer read, and the
    // tiers are checked rather than assumed.
    let forbidden = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/ticket:claim",
            seed.project, seed.task
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "observer")
    .with_key("tickets-claim-2")
    .send(&world)
    .await;
    assert_eq!(forbidden.status, 403, "{}", forbidden.body);
}

#[tokio::test]
async fn a_linked_task_claims_its_tickets_and_refuses_to_pull_without_a_connector() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let created = ensure_project(&world, "linked", "Kontor", "/tmp/kontor-linked").await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Linked epic",
            &category,
            serde_json::json!([{
                "title": "The linked task",
                "ticket_links": [{"connector": "connector.jira", "external_issue_key": "ASMA-1"}]
            }]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("linked-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // A claim records Kontor's own decision to hold the ticket. It names the
    // domain's action and the links it covers, and nothing on the way in could
    // have named an assignee.
    let claimed = Call::post(
        format!("/v1/projects/{project}/tasks/{task}/ticket:claim"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("linked-claim-1")
    .send(&world)
    .await;
    assert_eq!(claimed.status, 200, "{}", claimed.body);
    assert_eq!(claimed.json()["action"], "reassign_to_principal");
    assert_eq!(claimed.json()["links"].as_array().expect("links").len(), 1);

    let replayed = Call::post(
        format!("/v1/projects/{project}/tasks/{task}/ticket:claim"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("linked-claim-1")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt_id"], claimed.json()["receipt_id"]);

    // Pulling comments for a task that *is* linked needs the connector this
    // realm does not have, and refuses rather than reporting a zero that would
    // be indistinguishable from "there were no new comments".
    let pulled = Call::post(
        format!("/v1/projects/{project}/tasks/{task}/ticket:pull-comments"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("linked-pull-1")
    .send(&world)
    .await;
    assert_eq!(pulled.status, 503, "{}", pulled.body);
    assert_eq!(pulled.code(), "unavailable");
}

/// The compact command's guards, exercised through the real route.
///
/// MUT-CTX-07's command-surface proof: removing
/// `CompactRequest::validate` from the handler makes the boundary case pass
/// through to the adapter, which this test catches by counting adapter calls.
#[tokio::test]
async fn a_compact_command_enforces_its_guards_before_any_runtime_effect() {
    let world = World::open().await;
    let (run, _) = world.launch().await;

    let compact = |body: serde_json::Value| {
        Call::post(format!("/v1/sessions/{run}/compact"), &body)
            .signed_as(&world, "operator")
            .with_key(kontor_core::id::CompactionReceiptId::generate().to_string())
    };
    let pack = kontor_core::id::ContentHash::of(b"pack")
        .as_str()
        .to_owned();
    let handoff = kontor_core::id::ContentHash::of(b"handoff")
        .as_str()
        .to_owned();

    // A finished turn is not a trigger, and there is no spelling for it.
    let before = world.fake.calls().len();
    let invented = compact(serde_json::json!({
        "trigger": "turn_finished",
        "context_pack_hash": pack,
        "handoff_hash": handoff,
        "active_tool": false,
        "unresolved_permission": false,
    }))
    .send(&world)
    .await;
    assert_eq!(invented.status, 400);
    assert_eq!(
        world.fake.calls().len(),
        before,
        "an invented trigger must reach the runtime not at all"
    );

    // A boundary compaction with no sealed handoff is refused, and refused
    // *before* the adapter is called.
    let before = world.fake.calls().len();
    let unsealed = compact(serde_json::json!({
        "trigger": "scope_boundary",
        "context_pack_hash": pack,
        "active_tool": false,
        "unresolved_permission": false,
    }))
    .send(&world)
    .await;
    assert_eq!(unsealed.status, 422);
    assert_eq!(
        world.fake.calls().len(),
        before,
        "a boundary compaction with no durable handoff must not reach the runtime"
    );

    // A session mid-tool-action is not at a safe point.
    let before = world.fake.calls().len();
    let busy = compact(serde_json::json!({
        "trigger": "operator",
        "context_pack_hash": pack,
        "handoff_hash": handoff,
        "active_tool": true,
        "unresolved_permission": false,
    }))
    .send(&world)
    .await;
    assert_eq!(busy.status, 422);
    assert_eq!(world.fake.calls().len(), before);

    // …and neither is one with a permission nobody answered.
    let before = world.fake.calls().len();
    let waiting = compact(serde_json::json!({
        "trigger": "operator",
        "context_pack_hash": pack,
        "handoff_hash": handoff,
        "active_tool": false,
        "unresolved_permission": true,
    }))
    .send(&world)
    .await;
    assert_eq!(waiting.status, 422);
    assert_eq!(world.fake.calls().len(), before);
}

/// An observer may preview a policy; only an operator may compact.
#[tokio::test]
async fn the_preview_reads_and_the_compact_command_requires_an_operator() {
    let world = World::open().await;
    let (run, _) = world.launch().await;

    let preview = Call::post(
        "/v1/context-policy/preview".to_owned(),
        &serde_json::json!({
            "role_slot": {
                "class": "deep",
                "enforcement": "best_effort",
                "trigger_scope": "growth_after_prefix",
                "boundary_compaction": true
            },
            "context_policy_capable": true
        }),
    )
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200);
    assert_eq!(preview.json()["requested_class"], "deep");
    assert_eq!(preview.json()["requested_tokens"], 512_000);
    assert_eq!(preview.json()["source"], "role_slot");
    assert_eq!(preview.json()["capability"], "configured");

    // The same preview from an unauthenticated caller is refused.
    let anonymous = Call::post(
        "/v1/context-policy/preview".to_owned(),
        &serde_json::json!({"context_policy_capable": true}),
    )
    .send(&world)
    .await;
    assert!(anonymous.status == 401 || anonymous.status == 403);

    // Compaction is an operator act.
    let as_observer = Call::post(
        format!("/v1/sessions/{run}/compact"),
        &serde_json::json!({
            "trigger": "threshold",
            "context_pack_hash": kontor_core::id::ContentHash::of(b"pack").as_str(),
            "active_tool": false,
            "unresolved_permission": false,
        }),
    )
    .signed_as(&world, "observer")
    .with_key(kontor_core::id::CompactionReceiptId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(as_observer.status, 403);
}

/// Bring an empty realm to "one armed epic with one ready task", and return the
/// project, the epic and the plan hash the next `scheduler:start` must present.
async fn armed_and_planned(world: &World, slug: &'static str) -> (String, String, String) {
    let created = ensure_project(world, slug, "Kontor", &format!("/tmp/kontor-{slug}")).await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(world, "admin")
    .with_key(format!("{slug}-account"))
    .send(world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Planed epic",
            &category,
            serde_json::json!([{"title": "Only task"}]),
        ),
    )
    .signed_as(world, "admin")
    .with_key(format!("{slug}-epic"))
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Start the epic"
        }),
    )
    .signed_as(world, "admin")
    .with_key(format!("{slug}-arm"))
    .send(world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    assert_eq!(
        plan.json()["ready"].as_array().expect("ready").len(),
        1,
        "the armed task is ready: {}",
        plan.body
    );
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    (project, epic, plan_hash)
}

/// BLK-001. A runtime that holds a plane-level container is unusable until
/// something creates it, and until this round nothing did: `prepare_project` had
/// no production caller, so startup reconciliation failed, the barrier settled
/// `Failed`, and admission blocked every armed task. Deleting either
/// `prepare_plane` call fails this test.
#[tokio::test]
async fn a_runtime_with_a_plane_is_prepared_by_startup_so_a_seat_can_be_materialized() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);

    // Nothing has prepared it yet, which is the whole starting condition.
    assert!(
        !world.fake.plane_is_prepared(),
        "a freshly composed plane is not prepared"
    );

    // Startup reconciliation prepares the plane and *then* takes its census. The
    // barrier opening is the observable difference: a census inside a plane that
    // does not exist refuses, and a refusal is not an empty realm.
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    assert!(
        world.fake.plane_is_prepared(),
        "startup reconciliation prepared the plane"
    );

    let health = Call::get("/v1/health")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(health.json()["scheduling_open"], serde_json::json!(true));

    let (project, epic, plan_hash) = armed_and_planned(&world, "plane-start").await;

    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash.clone()}),
    )
    .signed_as(&world, "operator")
    .with_key("plane-start-run")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].as_array().expect("seats").clone();
    assert!(
        !seats.is_empty(),
        "a seat is materialized on a plane-holding runtime: {}",
        started.body
    );
    for seat in &seats {
        assert_eq!(seat["applied"], "created");
    }

    // Same-seat: one team run, no slot seated twice, and this process holds the
    // frozen snapshot for every seat it launched. A plane prepared once per
    // admission — rather than a fresh project per seat — is what makes that true;
    // a second project would have put the team's seats in two places.
    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let runs = projection.json()["tasks"][0]["team_runs"]
        .as_array()
        .expect("runs")
        .clone();
    assert_eq!(runs.len(), 1, "exactly one team run: {}", projection.body);
    let projected = runs[0]["seats"].as_array().expect("seats");
    assert_eq!(projected.len(), seats.len(), "{}", projection.body);
    let slots: std::collections::BTreeSet<&str> = projected
        .iter()
        .map(|seat| seat["role_slot"].as_str().expect("a slot"))
        .collect();
    assert_eq!(slots.len(), projected.len(), "no role slot is seated twice");
    for seat in projected {
        assert!(
            seat["attached"].as_bool().expect("a flag"),
            "every seat is a live attached session: {}",
            projection.body
        );
    }

    // The plane was prepared, not re-created, on the way through: reconciliation
    // asked once and admission asked again, and both are the same plane.
    assert!(world.fake.plane_is_prepared());

    // ponytail: `scheduler:start` re-derives its batch before consulting the
    // receipt, so replaying an identical start after the task has left `ready` is
    // a `409` rather than the original answer. That is existing behaviour and is
    // deliberately not asserted here — it is unrelated to the plane and changing
    // it is not this round's scope.
    let _ = plan_hash;
}

/// BLK-001, second caller. Startup is not the only path that needs the plane:
/// `prepare_workspace` is addressed inside it too, and a realm whose runtime lost
/// its plane after the census — a restarted Paseo daemon, a plane registered
/// after this process started — would otherwise admit nothing until the next
/// restart. Dropping the plane after reconciliation isolates the admission-path
/// caller: with it removed, this fails while the test above still passes.
#[tokio::test]
async fn admission_prepares_the_plane_itself_rather_than_relying_on_startup() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let (project, epic, plan_hash) = armed_and_planned(&world, "plane-admit").await;

    // The runtime forgets its plane after the census and before admission — a
    // restarted Paseo daemon, or a plane registered after this process started.
    world.fake.forget_the_plane();
    assert!(!world.fake.plane_is_prepared());

    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("plane-admit-run")
    .send(&world)
    .await;
    assert_eq!(
        started.status, 200,
        "admission prepares the plane it is about to work inside: {}",
        started.body
    );
    assert!(
        world.fake.plane_is_prepared(),
        "the admission path prepared the plane"
    );
    assert!(
        !started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "{}",
        started.body
    );
}

/// FND-001. A byte-identical reapply of an unchanged graph returned a *different*
/// `bundle_hash`, so a caller diffing it to detect drift saw drift on every
/// replay. The digest is now over the stored graph and carries nothing about the
/// call that stored it.
#[tokio::test]
async fn reapplying_an_identical_epic_returns_the_identical_graph_digest() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "digest", "Kontor", "/tmp/kontor-digest").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let body = epic_body(
        revision,
        "Digest epic",
        &category,
        serde_json::json!([
            {"title": "First", "ticket_links": [{"connector": "connector.jira",
                                                 "external_issue_key": "ASMA-1"}]},
            {"title": "Second", "depends_on": ["First"]}
        ]),
    );

    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("digest-1")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["applied"], "created");

    // A *different* key, so this is a genuine second application of the same
    // graph rather than a receipt replay handing back the first answer.
    let again = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("digest-2")
        .send(&world)
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["applied"], "unchanged");

    assert_eq!(
        again.json()["bundle_hash"],
        first.json()["bundle_hash"],
        "an unchanged graph digests identically:\nfirst {}\nagain {}",
        first.body,
        again.body
    );
    // And it is a digest, not an empty string that would compare equal to itself.
    assert_eq!(
        first.json()["bundle_hash"]
            .as_str()
            .expect("a digest")
            .len(),
        64
    );

    // The other reapply facts the finding named still hold, so the digest is
    // stable *because the graph is*, not because it stopped describing it.
    assert_eq!(again.json()["epic_id"], first.json()["epic_id"]);
    assert_eq!(again.json()["revision"], first.json()["revision"]);
    assert_eq!(again.json()["work_profile"], first.json()["work_profile"]);
    assert_eq!(again.json()["team_template"], first.json()["team_template"]);

    // A graph that genuinely moved digests differently. Without this the test
    // above passes for a constant.
    let grown = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Digest epic",
            &category,
            serde_json::json!([
                {"title": "First", "ticket_links": [{"connector": "connector.jira",
                                                     "external_issue_key": "ASMA-1"}]},
                {"title": "Second", "depends_on": ["First"]},
                {"title": "Third"}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("digest-3")
    .send(&world)
    .await;
    assert_eq!(grown.status, 200, "{}", grown.body);
    assert_ne!(
        grown.json()["bundle_hash"],
        first.json()["bundle_hash"],
        "a third task changes the graph digest: {}",
        grown.body
    );
}

/// The incident pack the pilot fixture ships — a profile and a team this build's
/// compiled catalogue has never seen.
const INCIDENT_PACK: &str =
    include_str!("../../../tests/fixtures/pilot/incident-response-pack.json");

/// BLK-002. The catalogue was compiled in and `/v1/catalog/**` was read-only, so
/// a custom work profile or team template could not enter over the MCP-only
/// boundary at all. It can now, additively and revisioned — and an epic can pin
/// it, which is the only thing that makes registration worth anything.
#[tokio::test]
async fn a_registered_pack_widens_the_catalogue_and_an_epic_can_pin_its_profile() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");
    let category = pack["manifest"][0]["category"]
        .as_str()
        .expect("a category")
        .to_owned();

    // Before registration the realm advertises the compiled seeds and nothing
    // else, and the custom category is absent rather than empty.
    let before = Call::get("/v1/catalog/work-profiles")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert!(
        !before
            .json()
            .as_array()
            .expect("a catalog")
            .iter()
            .any(|entry| entry["category"] == serde_json::json!(category)),
        "the build ships no incident profile: {}",
        before.body
    );
    let absent = Call::get(format!("/v1/catalog/work-profiles/{category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(absent.status, 404, "{}", absent.body);

    // Registration is an admin decision: it widens what every later apply in
    // this realm may freeze onto a task.
    let refused = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "operator")
    .with_key("pack-register-forbidden")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);

    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("pack-register-1")
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);
    assert_eq!(registered.json()["applied"], "created");
    assert_eq!(registered.json()["source"], "registered");
    assert_eq!(registered.json()["pack_id"], pack["pack_id"]);
    assert_eq!(
        registered.json()["categories"],
        serde_json::json!([category])
    );
    assert!(
        !registered.json()["team_templates"]
            .as_array()
            .expect("templates")
            .is_empty(),
        "the pack carried a team template: {}",
        registered.body
    );

    // Same key, same fingerprint: the original answer.
    let again = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("pack-register-1")
    .send(&world)
    .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["applied"], "unchanged");
    assert_eq!(
        again.json()["document_hash"],
        registered.json()["document_hash"]
    );

    // A revision is immutable: the same version carrying different bytes is a
    // conflict, not an update, because an epic already frozen against it must
    // keep meaning what it meant.
    let mut edited = pack.clone();
    edited["manifest"][0]["label"] = serde_json::json!("Incident response (edited)");
    let drifted = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": edited}),
    )
    .signed_as(&world, "admin")
    .with_key("pack-register-3")
    .send(&world)
    .await;
    assert_eq!(drifted.status, 409, "{}", drifted.body);

    // A document that does not validate never reaches the store.
    let malformed = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": {"schema_version": 1, "pack_id": "nope"}}),
    )
    .signed_as(&world, "admin")
    .with_key("pack-register-4")
    .send(&world)
    .await;
    assert_eq!(malformed.status, 400, "{}", malformed.body);

    // The catalogue now advertises it, alongside — never instead of — the seeds.
    let listed = Call::get("/v1/catalog/packs")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_body = listed.json();
    let sources: Vec<&str> = listed_body
        .as_array()
        .expect("packs")
        .iter()
        .map(|entry| entry["source"].as_str().expect("a source"))
        .collect();
    assert!(sources.contains(&"bundled"), "{}", listed.body);
    assert!(sources.contains(&"registered"), "{}", listed.body);

    let after = Call::get("/v1/catalog/work-profiles")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let after_body = after.json();
    let categories: Vec<&str> = after_body
        .as_array()
        .expect("a catalog")
        .iter()
        .map(|entry| entry["category"].as_str().expect("a category"))
        .collect();
    assert!(categories.contains(&category.as_str()), "{}", after.body);
    assert!(
        categories.len() > 1,
        "the compiled seeds are still there: {}",
        after.body
    );

    // It resolves like any other category, with its own gates and its own team.
    let detail = Call::get(format!("/v1/catalog/work-profiles/{category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(detail.status, 200, "{}", detail.body);
    assert!(detail.json()["team"].is_object(), "{}", detail.body);

    // And the whole point: an epic pins it, and every task is frozen against it.
    let created = ensure_project(&world, "incident", "Kontor", "/tmp/kontor-incident").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Incident epic",
            &category,
            serde_json::json!([{"title": "Contain the incident"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("incident-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(
        applied.json()["work_profile"]["id"],
        serde_json::json!(category),
        "the epic froze the registered profile: {}",
        applied.body
    );
}

/// A registered pack may not redefine a category the build ships. Registration
/// widens the catalogue; it never changes what an already-frozen epic pinned.
#[tokio::test]
async fn a_registered_pack_may_not_shadow_a_compiled_category() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seeded = first_category(&world).await;

    let mut pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");
    pack["manifest"][0]["category"] = serde_json::json!(seeded);

    let refused = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack}),
    )
    .signed_as(&world, "admin")
    .with_key("pack-shadow-1")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "revision_conflict");
}

/// The idempotency key on a realm-scoped registration is bound to a fingerprint
/// of the whole logical operation, not to the pack alone. Content immutability
/// alone cannot refuse a key reused for a *different* pack — two registrations of
/// two different packs are each independently valid, and nothing would be
/// comparing them to each other. This is that comparison.
#[tokio::test]
async fn a_registration_key_is_bound_to_one_logical_operation() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");

    let first = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("bound-key")
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["applied"], "created");

    // Same key, same fingerprint → the original answer, and nothing written.
    let replayed = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("bound-key")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(
        replayed.json()["document_hash"],
        first.json()["document_hash"]
    );

    // Same key, same (pack_id, version), different bytes → refused. Both the
    // key binding and the revision's immutability say no; the key is judged
    // first, so this is reported as the key having meant something else.
    let mut edited = pack.clone();
    edited["manifest"][0]["label"] = serde_json::json!("Incident response (edited)");
    let drifted = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": edited}),
    )
    .signed_as(&world, "admin")
    .with_key("bound-key")
    .send(&world)
    .await;
    assert_eq!(drifted.status, 409, "{}", drifted.body);

    // Same key, a *different* pack entirely → refused. This is the case the
    // content check could never have caught: a second pack at a fresh id and a
    // fresh version is a perfectly valid registration on its own, and only the
    // key binding knows the key already stood for another one.
    let mut other = pack.clone();
    other["pack_id"] = serde_json::json!("kontor-pilot-other");
    other["manifest"][0]["category"] = serde_json::json!("other-response-v1");
    other["profiles"][0]["id"] = serde_json::json!("other-response-v1");
    other["manifest"][0]["profile"] = serde_json::json!("other-response-v1");
    let reused = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": other.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("bound-key")
    .send(&world)
    .await;
    assert_eq!(
        reused.status, 409,
        "one key may stand for one registration: {}",
        reused.body
    );
    assert_eq!(reused.code(), "revision_conflict");

    // And the refusal changed nothing: the second pack is genuinely absent, not
    // half-registered by the call that was refused.
    let listed = Call::get("/v1/catalog/packs")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let listed_body = listed.json();
    assert!(
        !listed_body
            .as_array()
            .expect("packs")
            .iter()
            .any(|entry| entry["pack_id"] == serde_json::json!("kontor-pilot-other")),
        "a refused registration wrote nothing: {}",
        listed.body
    );

    // Under its own key it registers, so the refusal above was about the key and
    // not about the pack.
    let accepted = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": other}),
    )
    .signed_as(&world, "admin")
    .with_key("other-key")
    .send(&world)
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(accepted.json()["applied"], "created");
}

/// BLK-003. Seat admission used to synthesize `/w/<task_id>` because no field in
/// the model carried a worktree. A runtime that verifies placement refuses that
/// root, so no seat could ever bind a native session; one that does not verify
/// would have run the work in a directory nobody chose. The task now declares
/// where its work happens, and admission passes exactly that.
#[tokio::test]
async fn a_seat_is_prepared_at_the_worktree_the_task_declares() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    // This runtime serves exactly one worktree and refuses every other root —
    // the shape a real Paseo plane has, and the one the default fake does not.
    let canonical = "/w/declared-tree";
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse(canonical).expect("a valid root"),
    );

    let created = ensure_project(&world, "worktree", "Kontor", "/tmp/kontor-worktree").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("worktree-account")
    .send(&world)
    .await;
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Worktree epic",
            &category,
            serde_json::json!([{"title": "Placed task", "worktree": canonical}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("worktree-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(
        applied.json()["tasks"][0]["worktree"],
        serde_json::json!(canonical),
        "the apply reports where the task will run: {}",
        applied.body
    );
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    // The projection reports it too, so a Lead can see a task's placement
    // without having to discover it by failing to seat one.
    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        projection.json()["tasks"][0]["worktree"],
        serde_json::json!(canonical),
        "{}",
        projection.body
    );

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Place the work"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("worktree-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    // The seat is prepared at the declared root, so a placement-verifying
    // runtime accepts it. Before the model carried the path this call refused.
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("worktree-start")
    .send(&world)
    .await;
    assert_eq!(
        started.status, 200,
        "the seat is prepared where the task says it lives: {}",
        started.body
    );
    assert!(
        !started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "{}",
        started.body
    );
}

/// The other half of BLK-003: a task nobody placed is refused rather than placed
/// at a guess, and a task placed somewhere the runtime will not work reports what
/// actually happened instead of a bare "the runtime refused the operation" —
/// which is BLK-004.
#[tokio::test]
async fn an_unplaced_task_is_refused_and_a_misplaced_one_says_why() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse("/w/only-here").expect("a valid root"),
    );

    let created = ensure_project(&world, "unplaced", "Kontor", "/tmp/kontor-unplaced").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("unplaced-account")
    .send(&world)
    .await;
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Stated with an explicit `null`, so this is a task nobody placed rather
    // than one the helper placed for us.
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &serde_json::json!({
            "expected_revision": revision,
            "name": "Unplaced epic",
            "work_profile_category": category,
            "runtime_family": "fake.runtime",
            "tasks": [{"title": "Nowhere task", "worktree": serde_json::Value::Null}],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("unplaced-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert!(
        applied.json()["tasks"][0]["worktree"].is_null(),
        "nobody placed it: {}",
        applied.body
    );
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Try to place nothing"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("unplaced-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    // Refused, and refused for the stated reason — never seated at a guessed
    // path. This is the assertion that makes the placeholder unreachable.
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("unplaced-start")
    .send(&world)
    .await;
    // The batch succeeds and the *task* is reported blocked, which is right: one
    // task's placement problem is not a reason to fail the others. What matters
    // is that it was not seated, and that the reason travelled.
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "an unplaced task is never seated at a guessed path: {}",
        started.body
    );
    let blocked = started.json()["blocked"]
        .as_array()
        .expect("blocked")
        .clone();
    assert_eq!(blocked.len(), 1, "{}", started.body);
    assert_eq!(blocked[0]["code"], "not_found");
    assert!(
        blocked[0]["evidence"][0]["rule"]
            .as_str()
            .expect("a rule")
            .contains("worktree"),
        "the refusal names what is missing: {}",
        started.body
    );
}

/// BLK-004. A workspace refusal used to fall through the error catch-all and
/// reach an operator as "the session's runtime refused the operation", with the
/// runtime's own rule discarded. It is now a named, distinct refusal.
#[tokio::test]
async fn a_workspace_refusal_is_reported_as_a_placement_fact() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse("/w/the-only-tree").expect("a valid root"),
    );

    let created = ensure_project(&world, "misplaced", "Kontor", "/tmp/kontor-misplaced").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("misplaced-account")
    .send(&world)
    .await;
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // A real path, correctly formed, and simply not the one this runtime serves.
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Misplaced epic",
            &category,
            serde_json::json!([{"title": "Elsewhere task", "worktree": "/w/somewhere-else"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("misplaced-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Place it wrong"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("misplaced-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("misplaced-start")
    .send(&world)
    .await;

    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "{}",
        started.body
    );
    let blocked = started.json()["blocked"]
        .as_array()
        .expect("blocked")
        .clone();
    assert_eq!(blocked.len(), 1, "{}", started.body);

    // The distinguishing assertions: `unsupported_capability`, not the
    // `unavailable` catch-all a transport failure also produces — so an operator
    // can tell "this runtime will not work there" from "this runtime could not
    // be reached" — and a rule that says which of the two it was. Before the
    // mapping, this arrived as `unavailable` / "the session's runtime refused
    // the operation", with the runtime's own rule discarded.
    assert_eq!(blocked[0]["code"], "unsupported_capability");
    assert!(
        blocked[0]["evidence"][0]["rule"]
            .as_str()
            .expect("a rule")
            .contains("workspace"),
        "the refusal is about placement: {}",
        started.body
    );
}

/// FND-002. `bundle_hash` disagreed between a fresh apply and the *same key*
/// served from the receipt: the two paths built the answer by different routes
/// and each digested its own shape. They now share one digest by construction.
#[tokio::test]
async fn a_receipt_served_replay_returns_the_digest_the_apply_returned() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "receipt", "Kontor", "/tmp/kontor-receipt").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;

    let body = epic_body(
        revision,
        "Receipt epic",
        &category,
        serde_json::json!([
            {"title": "First", "ticket_links": [{"connector": "connector.jira",
                                                 "external_issue_key": "ASMA-1"}]},
            {"title": "Second", "depends_on": ["First"]}
        ]),
    );

    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("receipt-key")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["applied"], "created");

    // The *same* key. This is served from the receipt, by a different code path
    // than the apply above, which is exactly where the two used to disagree.
    let replayed = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("receipt-key")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(
        replayed.json()["bundle_hash"],
        first.json()["bundle_hash"],
        "a receipt-served replay digests the same graph identically:\nfirst {}\nreplay {}",
        first.body,
        replayed.body
    );

    // And a genuine second application under a *different* key agrees with both,
    // so all three routes to the same graph produce one digest.
    let reapplied = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("receipt-key-2")
        .send(&world)
        .await;
    assert_eq!(reapplied.status, 200, "{}", reapplied.body);
    assert_eq!(reapplied.json()["applied"], "unchanged");
    assert_eq!(
        reapplied.json()["bundle_hash"],
        first.json()["bundle_hash"],
        "{}",
        reapplied.body
    );
}
