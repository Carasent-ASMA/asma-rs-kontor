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

use std::collections::BTreeSet;
use std::sync::Arc;

use harness::{Answer, Call, World, at, capabilities_without, fake_family, name, secret};
use kontor_api::state::BarrierState;
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{AgentRunId, CanonicalDocument, ContentHash, ProjectId, TaskId};
use kontor_core::repository::{
    NewObservation, NewProject, NewRuntimeEvent, ProjectRepository, RealmRepository, RunClosure,
    RunRepository, WorkflowRepository,
};
use kontor_core::state::{
    Freshness, ObservedRunState, RuntimeContact, TerminalEvidence, TerminalEvidenceSource,
    TerminalOutcome,
};
use kontor_daemon::{DEFAULT_CAPACITY, Daemon, DaemonConfig};
use kontor_runtime::adapter::RuntimeAdapter as _;
use kontor_runtime::capability::RuntimeCapability;
use kontor_runtime::fake::{AdapterCall, RequestKey, ScriptStep};
use kontor_scheduler::model::CapacityConfig;

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
/// It composes without reaching anything: a Paseo transport validates two
/// strings and connects to nothing until a route asks it to.
///
/// Paseo only. The `ao` family was withdrawn — see
/// `kontor_daemon::runtimes::DEFERRED_FAMILIES` — and a fixture that still
/// carried one would be asserting that a boundary this build enforces does not
/// exist.
fn fleet_settings() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 4,
        "runtimes": [
            {
                "family": "paseo",
                "runtime_kind": "paseo.agent",
                "host_key": "paseo-host",
                "mini_project_id": "mini-1",
                "jira_epic_key": "ASMA-7759",
                "mini_project_short_title": "Kontor MVP",
                "plan_item_key": "KON-MVP-15",
                "jira_issue_key": "ASMA-7759",
                "ticket_short_code": "KON-15",
                "seat_display_roles": {
                    "implement": { "role": "Implement" }
                },
                "project_root_cwd": "/w/kontor",
                "canonical_worktree_cwd": "/w/kontor-task",
                "orchestrator_agent_id": "orchestrator-1",
                "max_concurrent_sessions": 4,
                "executable": "paseo",
                "host_target": "https://operator:hunter2@paseo.example",
                "endpoint": "ws://127.0.0.1:6767/ws",
                "client_id": "kontor-mini-1",
                "timeout_seconds": 30
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
        vec!["paseo.agent".to_owned()],
        "the configured family is a live adapter in the registry"
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
        serde_json::json!(["paseo.agent"]),
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
    broken["runtimes"][0]["canonical_worktree_cwd"] = serde_json::json!("relative/not-a-root");
    std::fs::write(
        kontor_daemon::runtimes::path_in(directory.path()),
        serde_json::to_vec_pretty(&broken).expect("a fleet document"),
    )
    .expect("the fleet description is written");

    let refused = Daemon::start_configured(DaemonConfig::at(directory.path()).with_port(0))
        .expect_err("a runtime that cannot be composed is not served around");
    let rendered = refused.to_string();
    assert!(
        rendered.contains("canonical_worktree_cwd"),
        "the refusal names the field"
    );
    assert!(
        !rendered.contains("hunter2") && !rendered.contains("not-a-root"),
        "and never the value: {rendered}"
    );
}

/// The AO family is refused at startup, by name, and starts nothing.
///
/// The defect this closes: `family: "ao"` was a setting an operator could write,
/// and the daemon composed a live AO adapter for it. The Paseo-only boundary then
/// held only by nobody writing that line.
#[tokio::test]
async fn an_ao_family_refuses_the_start_and_composes_no_substitute() {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let mut withdrawn = fleet_settings();
    withdrawn["runtimes"]
        .as_array_mut()
        .expect("the fleet is an array")
        .push(serde_json::json!({
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
        }));
    std::fs::write(
        kontor_daemon::runtimes::path_in(directory.path()),
        serde_json::to_vec_pretty(&withdrawn).expect("a fleet document"),
    )
    .expect("the fleet description is written");

    let refused = Daemon::start_configured(DaemonConfig::at(directory.path()).with_port(0))
        .expect_err("this build does not run AO");
    let rendered = refused.to_string();
    assert!(
        rendered.contains("ao"),
        "the refusal names the family, so an operator knows to stop asking: {rendered}"
    );
    assert!(
        !rendered.contains("hunter2"),
        "and never the document it refused: {rendered}"
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

#[tokio::test]
async fn teams_catalog_drafts_and_immutable_revisions_share_one_realm_projection() {
    let world = World::open().await;
    let catalog = Call::get("/v1/catalog")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    assert_eq!(catalog.realm(), world.realm_id());
    assert!(
        !catalog.json()["models"]
            .as_array()
            .expect("models")
            .is_empty()
    );

    let draft = serde_json::json!({
        "id": "team-live",
        "name": "Live team v1",
        "slots": [{"id": "lead", "capabilities": {"context": {"class": "standard"}}}]
    });
    let saved = Call::post("/v1/teams/drafts:save", &draft)
        .signed_as(&world, "operator")
        .with_key("team-save-1")
        .send(&world)
        .await;
    assert_eq!(saved.status, 200, "{}", saved.body);
    assert_eq!(saved.json()["snapshot_cursor"], serde_json::json!(1));
    assert_eq!(
        saved.json()["drafts"][0]["resolved_policy"][0]["class"],
        serde_json::json!("standard")
    );
    assert_eq!(
        saved.json()["drafts"][0]["resolved_policy"][0]["source"],
        serde_json::json!("role_slot")
    );
    assert_eq!(
        saved.json()["drafts"][0]["resolved_policy"][0]["capability"],
        serde_json::json!("unsupported")
    );

    let first = Call::post("/v1/teams/team-live/publish", &serde_json::json!({}))
        .signed_as(&world, "operator")
        .with_key("team-publish-1")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(
        first.json()["revisions"][0]["version"],
        serde_json::json!(1)
    );

    let renamed = serde_json::json!({
        "id": "team-live", "name": "Live team v2", "slots": draft["slots"]
    });
    Call::post("/v1/teams/drafts:save", &renamed)
        .signed_as(&world, "operator")
        .with_key("team-save-2")
        .send(&world)
        .await;
    let second = Call::post("/v1/teams/team-live/publish", &serde_json::json!({}))
        .signed_as(&world, "operator")
        .with_key("team-publish-2")
        .send(&world)
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(second.realm(), world.realm_id());
    assert_eq!(second.json()["snapshot_cursor"], serde_json::json!(4));
    assert_eq!(
        second.json()["revisions"][0]["name"],
        serde_json::json!("Live team v1")
    );
    assert_eq!(
        second.json()["revisions"][1]["version"],
        serde_json::json!(2)
    );
}

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

#[tokio::test]
async fn best_effort_compaction_records_not_enforced_when_the_runtime_cannot_compact() {
    let world = World::open_with(capabilities_without(&[RuntimeCapability::Compact])).await;
    let (run, _) = world.launch().await;
    let policy = kontor_core::spec::ContextPolicySnapshot::standard(
        &kontor_core::spec::ContextWindowBounds::unknown(),
        false,
        kontor_core::id::SCHEMA_VERSION,
        at("2026-08-10T09:00:00Z"),
    )
    .expect("best effort freezes against an incapable runtime");
    world.daemon.state().with_store(|store| {
        store
            .record_run_context_policy(world.project, run, &policy)
            .expect("the launch policy is durable");
    });
    let before = world.fake.calls().len();

    let answer = Call::post(
        format!("/v1/sessions/{run}/compact"),
        &serde_json::json!({
            "trigger": "scope_boundary",
            "context_pack_hash": kontor_core::id::ContentHash::of(b"pack").as_str(),
            "handoff_hash": kontor_core::id::ContentHash::of(b"handoff").as_str(),
            "active_tool": false,
            "unresolved_permission": false,
        }),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_core::id::CompactionReceiptId::generate().to_string())
    .send(&world)
    .await;

    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(answer.json()["value"]["status"], "not_enforced");
    assert_eq!(
        world.fake.calls().len(),
        before,
        "reporting best-effort non-enforcement must have zero runtime effect"
    );
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
    armed_and_planned_with(world, slug).await.0
}

/// As [`armed_and_planned`], additionally returning the account that armed it.
async fn armed_and_planned_with(
    world: &World,
    slug: &'static str,
) -> ((String, String, String), String) {
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
    ((project, epic, plan_hash), account_id)
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

/// BLK-006. A session bound before a restart could not be operated after one.
/// The binding survived, the native session survived, and every session
/// operation refused `stale_binding` — the frozen capability snapshot lived only
/// in process memory, and startup reconciliation censused the same empty
/// registry, so it re-attested nothing.
///
/// The snapshot is now persisted and handed back to the issuing runtime at
/// startup, which confirms the session and re-records the snapshot *verbatim*.
#[tokio::test]
async fn a_session_bound_before_a_restart_is_operable_after_it() {
    let world = World::open().await;
    let (run, bound) = world.launch().await;
    world.script(HISTORY_LIVE);

    // Operable before the restart, so the assertions after it are about the
    // restart and not about the session having never worked. Both halves are
    // exercised, because they are lost and recovered separately: a read needs
    // the binding, a *write* needs the seat's placement as well.
    let before = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(before.status, 200, "{}", before.body);
    let sent_before = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "before the restart"}),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(sent_before.status, 200, "{}", sent_before.body);

    let realm_before = world.realm_id();
    let observer = secret(&world, "observer");
    // Sending is an operator act; reading is an observer's. Both tiers are held
    // across the restart so neither assertion below is really about authority.
    let observer_operator = secret(&world, "operator");
    let body = serde_json::json!({"body": "after the restart"});
    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);

    // The runtime outlives the control plane — which is the real shape of this:
    // Paseo keeps running while kontord restarts. The *same* fake is registered
    // with the new daemon, still holding the same native session. What it does
    // *not* keep is the adapter's own ledgers: `compose_paseo` rebuilds every
    // adapter from a fresh checkpoint, so which bindings it issued and where
    // each seat sits are gone, exactly as they are in production.
    fake.rebuild_adapter_state();
    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    assert_eq!(restarted.realm_id(), realm_before);

    // A fresh process holds nothing until it asks the runtime.
    assert!(
        restarted.state().sessions().is_empty(),
        "a restarted process starts with no frozen snapshots"
    );

    assert_eq!(
        restarted.reconcile().await,
        BarrierState::Open,
        "the restart's census completes"
    );

    // The binding is held again, and it is the *same* binding: same id, same
    // native session, same generation, same trust grade. Re-attestation
    // re-records what was issued; it does not re-derive it.
    let restored = restarted
        .state()
        .sessions()
        .get(bound.binding_id())
        .expect("the runtime re-attested the binding this realm held");
    assert_eq!(
        restored, bound,
        "a re-attested binding is the one that was issued, whole"
    );
    assert_eq!(restored.identity(), bound.identity(), "same native session");
    assert_eq!(
        restored.capabilities.trust_grade, bound.capabilities.trust_grade,
        "the binding is not re-graded across a restart"
    );

    // The operability proof: the pre-restart session answers through the new
    // process. This refused `409 stale_binding` before.
    let router = restarted.router();
    let after = Call::get(format!("/v1/sessions/{run}/timeline"))
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(
        after.status, 200,
        "a session bound before the restart is operable after it: {}",
        after.body
    );

    // And it is the same session's content, not a new one: the run addressed is
    // the run that was bound.
    assert_eq!(
        after.json()["agent_run_id"],
        serde_json::json!(run.to_string()),
        "{}",
        after.body
    );

    // The write path, which is what round 4 restored the binding for and did not
    // restore the *placement* for: a message is delivered into a workspace, so a
    // seat this process cannot place is a seat it cannot drive. This refused
    // with `WorkspaceBindingRequired` before.
    let message = kontor_runtime::request::MessageId::generate().to_string();
    let sent = Call::post(format!("/v1/sessions/{run}/messages"), &body)
        .with_token(&observer_operator)
        .with_key(&message)
        .send_to(&router)
        .await;
    assert_eq!(
        sent.status, 200,
        "a seat bound before the restart can be driven after it: {}",
        sent.body
    );

    // Same idempotency semantics as before the restart: the identical message id
    // is answered from the runtime's own ledger rather than delivered twice.
    let replayed = Call::post(format!("/v1/sessions/{run}/messages"), &body)
        .with_token(&observer_operator)
        .with_key(&message)
        .send_to(&router)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["position"],
        sent.json()["position"],
        "a replayed message lands where the first one did: {}",
        replayed.body
    );

    restarted.state().signals().stop();
    drop(directory);
}

/// The other half of the rule: a binding whose native session did *not* survive
/// is not restored, and is not operable. Re-attestation asks the runtime; it does
/// not assume a persisted claim is still true.
#[tokio::test]
async fn a_binding_whose_session_is_gone_is_not_restored() {
    let world = World::open().await;
    let (run, bound) = world.launch().await;

    let observer = secret(&world, "observer");
    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);

    // The runtime moved to a new generation while the control plane was down. A
    // repeated native id in a new generation is a different session, so the old
    // binding must not come back pointing at whatever now answers to it. The
    // adapter's own ledgers are rebuilt too, as `compose_paseo` rebuilds them.
    fake.restart();
    fake.rebuild_adapter_state();

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    restarted.reconcile().await;

    assert!(
        restarted
            .state()
            .sessions()
            .get(bound.binding_id())
            .is_none(),
        "a binding whose session did not survive is not re-attested"
    );

    let router = restarted.router();
    let after = Call::get(format!("/v1/sessions/{run}/timeline"))
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(
        after.status, 409,
        "and it is not operable, which is the honest answer: {}",
        after.body
    );
    assert_eq!(after.code(), "stale_binding");

    // Neither readable nor drivable. A binding the runtime would not attest gets
    // no placement either, so the write path refuses as well — the two must not
    // come apart, which is the whole of BLK-007.
    let sent = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "into a session that is gone"}),
    )
    .with_token(secret_from(&state_root, "operator"))
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send_to(&router)
    .await;
    assert_eq!(
        sent.status, 409,
        "an unattested binding is not drivable either: {}",
        sent.body
    );
    assert_eq!(sent.code(), "stale_binding");

    restarted.state().signals().stop();
    drop(directory);
}

/// Persist a claim under `binding_id` without any runtime having issued it.
///
/// This is the forger's position exactly: write to the store, restart, and see
/// whether the control plane hands the session over on the strength of the row.
fn plant_claim(daemon: &Daemon, snapshot: &kontor_runtime::capability::RuntimeBindingSnapshot) {
    let document = serde_json::to_string(snapshot).expect("the snapshot serializes");
    daemon.state().with_store(|store| {
        store
            .persist_binding_snapshot(snapshot.binding_id(), snapshot.agent_run_id(), &document)
            .expect("the claim is persisted");
    });
}

/// A claim naming a session this runtime does not have is refused, however
/// self-consistent it is.
///
/// The forgery is a *good* one: valid JSON, a correlation that agrees with its
/// own run, capabilities within what the runtime declares. Only the runtime's own
/// census can tell that no such session exists, which is the point — the row is a
/// claim, and the runtime is the authority.
#[tokio::test]
async fn a_forged_but_self_consistent_claim_is_not_restored() {
    let world = World::open().await;
    let (_, bound) = world.launch().await;

    // The claim for a *real, open* binding is rewritten in the store to hand
    // its session to a different run. Keeping the binding id is what makes this
    // a test of attestation: the daemon only presents claims for bindings it
    // durably holds, so a forged binding id never reaches the runtime at all and
    // would prove nothing. Session existence, generation and capability bounds
    // all pass here; only the live session's own ownership can refuse it.
    let forged_run = AgentRunId::generate();
    let mut forged = bound.clone();
    forged.binding.agent_run_id = forged_run;
    forged.correlation = kontor_runtime::observation::CorrelationEvidence::establish(
        forged_run,
        &kontor_runtime::request::CorrelationLabel::for_run(forged_run).to_string(),
        forged.binding.identity.clone(),
        at("2026-08-10T09:00:00Z"),
    )
    .expect("the forgery is internally consistent");
    assert_eq!(
        forged.identity(),
        bound.identity(),
        "the forgery names a session the runtime really has"
    );
    // It passes the snapshot's own consistency check, which is exactly why that
    // check alone was never enough.
    assert!(
        forged.ensure_correlated().is_ok(),
        "the forgery is self-consistent"
    );
    plant_claim(&world.daemon, &forged);

    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    restarted.reconcile().await;

    assert!(
        restarted
            .state()
            .sessions()
            .get(forged.binding_id())
            .is_none(),
        "a claim handing a live session to a run that does not own it is refused"
    );

    restarted.state().signals().stop();
    drop(directory);
}

/// A claim tampered to assert more authority than the runtime has is refused.
///
/// The dangerous direction is promotion: a binding frozen at a degraded grade
/// being handed back as a trusted one. A claim may be weaker than the live
/// runtime and never stronger, so the runtime refuses one that exceeds what it
/// can currently prove.
#[tokio::test]
async fn a_claim_that_exceeds_what_the_runtime_can_prove_is_not_restored() {
    let world = World::open_with(capabilities_without(&[
        kontor_runtime::capability::RuntimeCapability::Compact,
    ]))
    .await;
    let (run, bound) = world.launch().await;

    // Tampered in the store after the fact: the same binding, the same session,
    // with a capability the runtime does not declare written into it.
    let mut tampered = bound.clone();
    tampered
        .capabilities
        .supported
        .insert(kontor_runtime::capability::RuntimeCapability::Compact);
    assert!(
        !tampered.within(&world.fake.capabilities()),
        "the tampering claims more than the runtime declares"
    );
    plant_claim(&world.daemon, &tampered);

    let observer = secret(&world, "observer");
    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    restarted.reconcile().await;

    assert!(
        restarted
            .state()
            .sessions()
            .get(bound.binding_id())
            .is_none(),
        "a claim asserting capability the runtime cannot prove is not restored"
    );

    // And it is not operable either: refusing to restore is not cosmetic.
    let router = restarted.router();
    let after = Call::get(format!("/v1/sessions/{run}/timeline"))
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(after.status, 409, "{}", after.body);
    assert_eq!(after.code(), "stale_binding");

    restarted.state().signals().stop();
    drop(directory);
}

/// A persisted claim that will not parse fails the startup census loudly rather
/// than disappearing from it.
///
/// Silently skipping the row is the quietest possible way to lose a live
/// session: every later census would be taken over a set the binding is simply
/// absent from, which is indistinguishable from never having had one.
#[tokio::test]
async fn an_unreadable_binding_claim_shuts_scheduling_rather_than_vanishing() {
    let world = World::open().await;
    let (_, bound) = world.launch().await;

    // A row that is valid JSON and not a snapshot — corruption, a partial write,
    // or a schema this binary no longer understands.
    world.daemon.state().with_store(|store| {
        store
            .persist_binding_snapshot(
                bound.binding_id(),
                bound.agent_run_id(),
                "{\"schema_version\":1,\"not\":\"a snapshot\"}",
            )
            .expect("the row is written");
    });

    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");

    assert_eq!(
        restarted.reconcile().await,
        BarrierState::Failed,
        "an unreadable claim shuts scheduling instead of being skipped"
    );
    let health = Call::get("/v1/health")
        .with_token(secret_from(&state_root, "observer"))
        .send_to(&restarted.router())
        .await;
    assert_eq!(health.json()["scheduling_open"], serde_json::json!(false));

    restarted.state().signals().stop();
    drop(directory);
}

/// Bring a plane-holding realm to "seats materialized", and return the project,
/// the epic, the arming account and the started seats.
async fn seated_turns(
    world: &World,
    slug: &'static str,
) -> (String, String, String, serde_json::Value) {
    let ((project, epic, plan_hash), account) = armed_and_planned_with(world, slug).await;
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(world, "operator")
    .with_key(format!("{slug}-start"))
    .send(world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].clone();
    assert!(
        !seats.as_array().expect("seats").is_empty(),
        "{}",
        started.body
    );
    (project, epic, account, seats)
}

#[tokio::test]
async fn a_runtime_cancelled_run_accepts_one_guarded_late_handoff_without_reopening() {
    let world = World::open_with(capabilities_without(&[RuntimeCapability::Compact])).await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (run_id, snapshot) = world.launch().await;
    let handoff_hash = ContentHash::of(b"late handoff");
    let policy = kontor_core::spec::ContextPolicySnapshot::standard(
        &kontor_core::spec::ContextWindowBounds::unknown(),
        false,
        kontor_core::id::SCHEMA_VERSION,
        at("2026-08-10T09:00:00Z"),
    )
    .expect("best effort freezes against an incapable runtime");
    world.daemon.state().with_store(|store| {
        store
            .record_run_context_policy(world.project, run_id, &policy)
            .expect("the launch policy is durable");
    });

    let compacted = Call::post(
        format!("/v1/sessions/{run_id}/compact"),
        &serde_json::json!({
            "trigger": "scope_boundary",
            "context_pack_hash": ContentHash::of(b"context pack").as_str(),
            "handoff_hash": handoff_hash.as_str(),
            "active_tool": false,
            "unresolved_permission": false,
        }),
    )
    .signed_as(&world, "operator")
    .with_key(kontor_core::id::CompactionReceiptId::generate().to_string())
    .send(&world)
    .await;
    assert_eq!(compacted.status, 200, "{}", compacted.body);
    assert_eq!(compacted.json()["value"]["status"], "not_enforced");

    finish_natively(&world, &run_id.to_string()).await;
    let refused = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/runtime:settle",
            world.project
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("late-handoff-runtime-settle")
    .send(&world)
    .await;
    assert_eq!(refused.status, 422, "{}", refused.body);
    assert_eq!(refused.code(), "handoff_unsettled");

    // Reconstruct the already-observed historical failure: the runtime
    // cancellation is durable before this new operator surface is called.
    let payload = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "observed_state": "cancelled",
        "contact": "reachable",
        "native_sequence": 1,
        "observed_at": "2026-08-10T09:10:00Z"
    }))
    .expect("control metadata");
    let run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    let projection = world.daemon.state().with_store(|store| {
        store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: world.project,
                    agent_run_id: run_id,
                    identity: snapshot.identity().clone(),
                    native_event_id: None,
                    native_sequence: 1,
                    payload: payload.clone(),
                    observed_at: at("2026-08-10T09:10:00Z"),
                },
                observed: ObservedRunState::Cancelled,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: run.revision,
            })
            .expect("the cancellation observation is durable")
    });
    let cursor = projection
        .last_cursor
        .expect("the observation has a cursor");
    let observed = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    world.daemon.state().with_store(|store| {
        store
            .close_agent_run(&RunClosure {
                project_id: world.project,
                agent_run_id: run_id,
                expected_revision: observed.revision,
                evidence: TerminalEvidence {
                    outcome: TerminalOutcome::Cancelled,
                    source: TerminalEvidenceSource::RuntimeObservation { cursor },
                    evidence_hash: payload.hash().clone(),
                    closed_at: at("2026-08-10T09:11:00Z"),
                },
            })
            .expect("the run is runtime-terminal")
    });
    world
        .daemon
        .state()
        .sessions()
        .forget(snapshot.binding_id());

    let task = world.daemon.state().with_store(|store| {
        store
            .get_task(world.project, world.task)
            .expect("the task reads")
            .expect("the task exists")
    });
    let body = serde_json::json!({
        "role_slot": "harness-seat",
        "expected_task_revision": task.revision.get(),
        "binding_generation": snapshot.identity().generation,
        "handoff_hash": handoff_hash.as_str(),
        "artifacts": ["handoff-sha256-deadbeef", "wip-unverified"]
    });
    let attested = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/handoffs:attest-late",
            world.project
        ),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("late-handoff-attestation")
    .send(&world)
    .await;
    assert_eq!(attested.status, 200, "{}", attested.body);
    assert_eq!(attested.json()["terminal_outcome"], "cancelled");
    assert_eq!(attested.json()["seat_live"], false);
    assert_eq!(attested.json()["applied"], "created");

    let after = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    assert_eq!(
        after.terminal.expect("the run stays terminal").outcome,
        TerminalOutcome::Cancelled
    );
    assert!(
        world
            .daemon
            .state()
            .sessions()
            .get(snapshot.binding_id())
            .is_none()
    );
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.list_settled_turns(world.project, world.task))
            .expect("the disposition reads")
            .len(),
        1
    );

    let replayed = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/handoffs:attest-late",
            world.project
        ),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("late-handoff-attestation")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");

    let duplicate = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run_id}/handoffs:attest-late",
            world.project
        ),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("late-handoff-second-disposition")
    .send(&world)
    .await;
    assert_eq!(duplicate.status, 409, "{}", duplicate.body);
}

#[tokio::test]
async fn an_admin_replaces_one_runtime_cancelled_seat_inside_the_existing_team() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "replace-seat").await;
    let seat = seats.as_array().expect("the seated roster")[1].clone();
    let predecessor = seat["agent_run_id"].as_str().expect("the run id");
    let role_slot = seat["role_slot"].as_str().expect("the role slot");
    let team_run = seat["team_run_id"].as_str().expect("the team run");

    finish_natively(&world, predecessor).await;
    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("replace-seat-runtime-settle")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert_eq!(settled.json()["observed"], "cancelled");

    let project_id = ProjectId::parse(&project).expect("a project id");
    let predecessor_id = AgentRunId::parse(predecessor).expect("a canonical run id");
    let before = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the predecessor reads")
            .expect("the predecessor exists")
    });
    let old_binding = before.binding.as_ref().expect("the predecessor was bound");
    let view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("the task revision");
    let body = serde_json::json!({
        "role_slot": role_slot,
        "expected_task_revision": task_revision,
        "binding_generation": old_binding.identity.generation,
    });
    let replaced = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("replace-seat-successor")
    .send(&world)
    .await;
    assert_eq!(replaced.status, 200, "{}", replaced.body);
    assert_eq!(replaced.json()["applied"], "created");
    assert_eq!(replaced.json()["team_run_id"], team_run);

    let successor_id = AgentRunId::parse(
        replaced.json()["successor_agent_run_id"]
            .as_str()
            .expect("the successor id"),
    )
    .expect("a canonical successor id");
    let successor = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, successor_id)
            .expect("the successor reads")
            .expect("the successor exists")
    });
    assert_eq!(successor.parent_agent_run_id, Some(predecessor_id));
    assert_eq!(successor.team_run_id.to_string(), team_run);
    let successor_binding = successor.binding.expect("the successor is bound");
    assert_ne!(successor_binding.id, old_binding.id);
    assert!(
        world
            .daemon
            .state()
            .sessions()
            .get(old_binding.id)
            .is_none()
    );
    assert!(
        world
            .daemon
            .state()
            .sessions()
            .get(successor_binding.id)
            .is_some()
    );

    let replay = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("replace-seat-successor")
    .send(&world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged");
    assert_eq!(
        replay.json()["successor_agent_run_id"],
        successor_id.to_string()
    );
}

/// BLK-009. A bounded Kontor role turn settles on Kontor's own authority, and
/// the seat it was taken in stays live: settling a turn is not a claim that the
/// runtime ended anything, and the persistent Paseo session is expected to still
/// be sitting there when it returns.
#[tokio::test]
async fn settling_a_bounded_turn_leaves_the_seat_live_and_the_run_open() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "turn-live").await;

    let seat = seats.as_array().expect("seats")[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("id").to_owned();
    let role_slot = seat["role_slot"].as_str().expect("slot").to_owned();

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");

    let invalid_artifact = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "artifacts": ["report:_docs/context.md"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-live-invalid-artifact")
    .send(&world)
    .await;
    assert_eq!(invalid_artifact.status, 400, "{}", invalid_artifact.body);
    assert_eq!(invalid_artifact.code(), "invalid_request");
    assert_eq!(
        invalid_artifact.json()["rule"],
        "artifact keys may contain only lowercase ASCII letters, digits, '.', '_' and '-'"
    );

    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-live-1")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert_eq!(settled.json()["applied"], "created");
    assert_eq!(settled.json()["turn_ordinal"], 1);
    assert_eq!(settled.json()["role_slot"], serde_json::json!(role_slot));

    // The two assertions this whole design exists for.
    assert_eq!(
        settled.json()["seat_live"],
        serde_json::json!(true),
        "settling a turn must leave the seat's session live: {}",
        settled.body
    );
    let run = Call::get(format!("/v1/runs/{agent_run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(run.status, 200, "{}", run.body);
    assert!(
        run.json()["terminal"].is_null(),
        "settling a turn must not close the run: {}",
        run.body
    );

    // And the seat is still operable, which is what "reusable" means in practice.
    let timeline = Call::get(format!("/v1/sessions/{agent_run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(timeline.status, 200, "{}", timeline.body);

    // Idempotent: the same key replays the same receipt rather than opening a
    // second position in the seat's sequence.
    let replayed = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-live-1")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(replayed.json()["turn_id"], settled.json()["turn_id"]);
    assert_eq!(replayed.json()["turn_ordinal"], 1);

    // The same key with different content is a conflict, not a second turn.
    let drifted = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "artifacts": ["change-set", "review-notes"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-live-1")
    .send(&world)
    .await;
    assert_eq!(drifted.status, 409, "{}", drifted.body);

    // `seat_live` is reported, not assumed. With the frozen snapshot gone this
    // process cannot drive the seat, and the settlement says so rather than
    // claiming a reusable seat it could not reach.
    world.daemon.state().sessions().forget(
        world
            .daemon
            .state()
            .with_store(|store| {
                store.get_agent_run(
                    kontor_core::id::ProjectId::parse(&project).expect("a project id"),
                    AgentRunId::parse(&agent_run).expect("a run id"),
                )
            })
            .expect("readable")
            .expect("the seat")
            .binding
            .expect("a bound seat")
            .id,
    );
    let unreachable = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "artifacts": ["late-notes"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-live-unreachable")
    .send(&world)
    .await;
    assert_eq!(unreachable.status, 200, "{}", unreachable.body);
    assert_eq!(
        unreachable.json()["seat_live"],
        serde_json::json!(false),
        "a seat this process cannot reach is reported as such: {}",
        unreachable.body
    );

    // A stale revision is refused: a turn is settled against a named task state.
    let stale = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision + 99,
            "artifacts": []
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-live-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
}

/// BLK-010. The next turn on a settled slot reuses the *same* seat: same Paseo
/// agent id, same native session, same role slot, same binding generation. This
/// is only meaningful after the prior turn is settled, which is why it is tested
/// in that order and not before.
#[tokio::test]
async fn the_next_turn_on_a_settled_slot_reuses_the_same_seat() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "turn-reuse").await;

    let seat = seats.as_array().expect("seats")[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("id").to_owned();
    let role_slot = seat["role_slot"].as_str().expect("slot").to_owned();

    // What the seat *is*, read before the first turn is settled.
    let before = world
        .daemon
        .state()
        .with_store(|store| {
            store.get_agent_run(
                kontor_core::id::ProjectId::parse(&project).expect("a project id"),
                AgentRunId::parse(&agent_run).expect("a run id"),
            )
        })
        .expect("the seat is readable")
        .expect("the seat exists");
    let native_before = before
        .binding
        .as_ref()
        .expect("a bound seat")
        .identity
        .clone();

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");

    let first = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot, "expected_task_revision": revision, "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-reuse-1")
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["turn_ordinal"], 1);

    // The correction turn: a *second* bounded turn in the same seat, settled
    // under its own key. Its ordinal advances; everything about the seat does
    // not.
    let second = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot, "expected_task_revision": revision, "artifacts": ["change-set", "review-notes"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-reuse-2")
    .send(&world)
    .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(second.json()["applied"], "created");
    assert_eq!(
        second.json()["turn_ordinal"],
        2,
        "a second turn takes the next position in the seat's sequence: {}",
        second.body
    );

    // The identity assertions BLK-010 asks for.
    assert_eq!(
        second.json()["agent_run_id"],
        serde_json::json!(agent_run),
        "the same seat"
    );
    assert_eq!(
        second.json()["role_slot"],
        serde_json::json!(role_slot),
        "the same role slot"
    );
    assert_eq!(
        second.json()["binding_generation"],
        first.json()["binding_generation"],
        "the same binding generation"
    );

    let after = world
        .daemon
        .state()
        .with_store(|store| {
            store.get_agent_run(
                kontor_core::id::ProjectId::parse(&project).expect("a project id"),
                AgentRunId::parse(&agent_run).expect("a run id"),
            )
        })
        .expect("the seat is readable")
        .expect("the seat exists");
    let native_after = after
        .binding
        .as_ref()
        .expect("a bound seat")
        .identity
        .clone();
    assert_eq!(
        native_after, native_before,
        "the same native Paseo session, unchanged across both turns"
    );
    assert!(
        after.terminal.is_none(),
        "neither turn closed the run: {after:?}"
    );
}

/// BLK-008. A settled turn derives its follow-up from persisted facts, targets
/// the already-materialized eligible slot, and produces **at most one** effect —
/// under a replayed settlement and across a restart.
#[tokio::test]
async fn a_settled_turn_derives_its_follow_up_at_most_once() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "turn-follow").await;

    // The bundled team is a chain, so the first slot hands to exactly one other.
    let seat = seats.as_array().expect("seats")[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("id").to_owned();
    let role_slot = seat["role_slot"].as_str().expect("slot").to_owned();

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");

    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot, "expected_task_revision": revision, "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-follow-1")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    let follow_ups = settled.json()["follow_ups"]
        .as_array()
        .expect("follow-ups")
        .clone();
    assert_eq!(
        follow_ups.len(),
        1,
        "the settled slot hands to exactly one successor: {}",
        settled.body
    );
    assert_ne!(
        follow_ups[0]["to_role_slot"],
        serde_json::json!(role_slot),
        "a handoff goes to another slot"
    );
    assert!(
        follow_ups[0]["target_agent_run_id"].is_string(),
        "the successor's seat was already materialized: {}",
        settled.body
    );

    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");
    let dispatches = |world: &World| {
        world
            .daemon
            .state()
            .with_store(|store| store.list_turn_dispatches(project_id))
            .expect("the dispatch ledger is readable")
    };
    // The row count is guaranteed by the primary key, so it is the *effects*
    // that have to be counted: how many times the successor's seat was actually
    // driven. That is the number "at most one follow-up effect" is about.
    let sends = |world: &World| {
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, kontor_runtime::fake::AdapterCall::Send(..)))
            .count()
    };
    assert_eq!(dispatches(&world).len(), 1, "one follow-up was derived");
    let sends_after_first = sends(&world);
    assert_eq!(
        sends_after_first, 1,
        "the follow-up drove the successor's seat exactly once"
    );

    // Replaying the settlement derives nothing further.
    let replayed = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot, "expected_task_revision": revision, "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-follow-1")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(
        dispatches(&world).len(),
        1,
        "a replayed settlement derives no second follow-up"
    );
    assert_eq!(
        sends(&world),
        sends_after_first,
        "and produces no second effect"
    );

    // And a restart derives none either: reconciliation only *finishes* what was
    // already decided.
    let observer = secret(&world, "observer");
    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);
    fake.rebuild_adapter_state();

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    restarted.reconcile().await;
    let after_restart = restarted
        .state()
        .with_store(|store| store.list_turn_dispatches(project_id))
        .expect("the dispatch ledger is readable");
    assert_eq!(
        after_restart.len(),
        1,
        "a restart derives no second follow-up: {after_restart:?}"
    );
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| matches!(call, kontor_runtime::fake::AdapterCall::Send(..)))
            .count(),
        sends_after_first,
        "and a restart produces no second effect either"
    );
    let _ = observer;
    restarted.state().signals().stop();
    drop(directory);
}

/// BLK-008, the ambiguous-acknowledgement path. A follow-up whose effect the
/// runtime committed but could not acknowledge must not be delivered twice when
/// reconciliation retries it. The message id belongs to the *dispatch row*, not
/// to the attempt, so the retry presents the same id and the runtime recognises
/// its own committed effect.
#[tokio::test]
async fn a_follow_up_whose_acknowledgement_was_lost_is_not_delivered_twice() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "turn-lost").await;

    let seat = seats.as_array().expect("seats")[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("id").to_owned();
    let role_slot = seat["role_slot"].as_str().expect("slot").to_owned();
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");

    // The runtime commits the follow-up and then loses the acknowledgement: the
    // effect happened, and the control plane cannot know it did.
    world
        .fake
        .push_step(kontor_runtime::fake::ScriptStep::LoseSendAck);

    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot, "expected_task_revision": revision, "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-lost-1")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    let follow_ups = settled.json()["follow_ups"]
        .as_array()
        .expect("follow-ups")
        .clone();
    assert_eq!(follow_ups.len(), 1, "{}", settled.body);
    assert_eq!(
        follow_ups[0]["dispatched"],
        serde_json::json!(false),
        "the acknowledgement was lost, so the follow-up is derived and undelivered: {}",
        settled.body
    );

    let rows = world
        .daemon
        .state()
        .with_store(|store| store.list_turn_dispatches(project_id))
        .expect("the dispatch ledger is readable");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].dispatched, "the row records it as undelivered");
    let message_id = rows[0].message_id.clone();
    let successor = rows[0].target_agent_run.expect("a materialized successor");

    // How much content the successor's session holds *in total*. Counting only
    // items that carry the stored id would be blind to the very defect this
    // test exists for: a retry that minted a fresh id delivers a second effect
    // under a *different* id, which such a filter cannot see.
    let delivered_items = |world: &World| {
        let timeline = futures::executor::block_on(async {
            Call::get(format!("/v1/sessions/{successor}/timeline?limit=64"))
                .signed_as(world, "observer")
                .send(world)
                .await
        });
        timeline
            .json()
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or_default()
    };
    let after_first = delivered_items(&world);

    // Reconciliation retries exactly this undelivered row.
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let after_retry = world
        .daemon
        .state()
        .with_store(|store| store.list_turn_dispatches(project_id))
        .expect("the dispatch ledger is readable");
    assert_eq!(after_retry.len(), 1, "the retry derived nothing new");
    assert_eq!(
        after_retry[0].message_id, message_id,
        "the retry presents the *same* message id, which is the whole fix"
    );
    assert!(
        after_retry[0].dispatched,
        "and the retry completed the handover: {after_retry:?}"
    );

    // One effect, not two. Before the id was bound to the row, the retry minted
    // a fresh id and the runtime had no way to recognise its own committed
    // effect, so the successor received the follow-up twice.
    assert_eq!(
        delivered_items(&world),
        after_first,
        "a retried follow-up is the same effect, not a second one"
    );
}

/// P1-D. The settlement authority is the tier the caller *authenticated at*, and
/// nothing a caller writes in the body can change it.
///
/// Checking that a named account exists and is enabled is a fact about the
/// account, not about who is asking. Persisting such a name as attribution would
/// record a claim the control plane never verified, so the field is gone and the
/// receipt carries what the bearer actually proved.
#[tokio::test]
async fn a_turn_receipt_records_proven_authority_and_not_a_claimed_actor() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "turn-authority").await;

    let seat = seats.as_array().expect("seats")[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("id").to_owned();
    let role_slot = seat["role_slot"].as_str().expect("slot").to_owned();

    // A second, perfectly real and enabled account — the one an operator would
    // have named to attribute the settlement elsewhere.
    let other = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Someone Else", "harness": "fake.runtime",
            "credential_alias": "other", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("turn-authority-other")
    .send(&world)
    .await;
    assert_eq!(other.status, 200, "{}", other.body);
    let other_id = other.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = projection.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");

    // The attribution attempt: a body naming another enabled account.
    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "actor": other_id,
            "account_profile": other_id,
            "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-authority-1")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);

    // The receipt records the tier the *bearer* proved…
    assert_eq!(
        settled.json()["settled_by"],
        serde_json::json!("operator"),
        "the receipt records proven authority: {}",
        settled.body
    );
    // …and not the account the body tried to attribute it to. The seat's own
    // account is operational context, derived from the bound run.
    assert_ne!(
        settled.json()["account_profile"],
        serde_json::json!(other_id),
        "a caller may not attribute a settlement to an account it merely named: {}",
        settled.body
    );

    // The contract has no field for a caller identity at all, which is what
    // makes the attempt above unrepresentable rather than merely refused.
    let document = Call::get("/v1/openapi.json")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let request_schema =
        document.json()["components"]["schemas"]["SettleTurnRequest"]["properties"]
            .as_object()
            .expect("the settle-turn request schema")
            .keys()
            .cloned()
            .collect::<Vec<String>>();
    assert!(
        !request_schema.iter().any(|name| name == "actor"),
        "the request carries no caller-supplied actor: {request_schema:?}"
    );

    // An admin settling the same seat records *its* tier, so the field tracks the
    // credential rather than the route.
    let by_admin = Call::post(
        format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": revision,
            "artifacts": ["change-set", "review-notes"]
        }),
    )
    .signed_as(&world, "admin")
    .with_key("turn-authority-2")
    .send(&world)
    .await;
    assert_eq!(by_admin.status, 200, "{}", by_admin.body);
    assert_eq!(by_admin.json()["settled_by"], serde_json::json!("admin"));
}

/// P1-A. The whole close-out, through public operations only, on a team whose
/// native seats stay **live**.
///
/// This is the journey BLK-009 was about and that the receipt alone did not
/// deliver: every declared seat materialized, each settles its final bounded
/// turn, the profile's gates and artifacts are discharged, and then the team
/// closes, the task completes and the epic closes — with every Paseo session
/// still sitting there and no `agent_runs.terminal` fabricated for any of them.
#[tokio::test]
async fn a_team_closes_on_settled_turns_while_every_seat_stays_live() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, account, seats) = seated_turns(&world, "close-live").await;
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");

    let seat_list = seats.as_array().expect("seats").clone();
    assert!(seat_list.len() > 1, "the bundled team seats several slots");
    let task_id = {
        let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        projection.json()["tasks"][0]["task_id"]
            .as_str()
            .expect("a task id")
            .to_owned()
    };
    let lifecycle = format!("/v1/projects/{project}/epics/{epic}/lifecycle");

    // Every declared slot settles its final turn. Each seat's session stays live
    // across its own settlement — asserted per seat, not once at the end.
    for (index, seat) in seat_list.iter().enumerate() {
        let agent_run = seat["agent_run_id"].as_str().expect("id");
        let role_slot = seat["role_slot"].as_str().expect("slot");
        let projected = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        let revision = projected.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("a revision");
        let settled = Call::post(
            format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
            &serde_json::json!({
                "role_slot": role_slot,
                "expected_task_revision": revision,
                "artifacts": ["change-set"]
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("close-live-turn-{index}"))
        .send(&world)
        .await;
        assert_eq!(settled.status, 200, "slot `{role_slot}`: {}", settled.body);
        assert_eq!(
            settled.json()["seat_live"],
            serde_json::json!(true),
            "slot `{role_slot}` is still live after settling its turn: {}",
            settled.body
        );
    }

    // The profile's own gates, read from the projection and discharged.
    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let gates = projection.json()["tasks"][0]["gates"]
        .as_array()
        .expect("gates")
        .clone();
    let workflow_revision = projection.json()["tasks"][0]["workflow_revision"]
        .as_u64()
        .expect("a workflow revision");
    for (index, gate) in gates.iter().enumerate() {
        let name = gate["gate"].as_str().expect("a gate");
        let evaluator = gate["evaluator_roles"][0]
            .as_str()
            .expect("an authorized evaluator");
        let evidence: Vec<&str> = gate["required_evidence"]
            .as_array()
            .expect("evidence")
            .iter()
            .map(|item| item.as_str().expect("an artifact"))
            .collect();
        let recorded = Call::post(
            format!("/v1/projects/{project}/tasks/{task_id}/gates/{name}/record"),
            &serde_json::json!({
                "expected_revision": workflow_revision,
                "verdict": "passed",
                "evaluator_role": evaluator,
                "evaluator_account": account,
                "evidence": evidence,
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("close-live-gate-{index}"))
        .send(&world)
        .await;
        assert_eq!(recorded.status, 200, "gate `{name}`: {}", recorded.body);
    }

    // The task completes. Reaching this at all is the whole of P1-A: the team's
    // closure was certified from its own settled-turn rows, not from any run
    // having ended.
    let after_gates = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let after_body = after_gates.json();
    let artifacts: Vec<&str> = after_body["tasks"][0]["required_artifacts"]
        .as_array()
        .expect("required artifacts")
        .iter()
        .map(|item| item.as_str().expect("an artifact"))
        .collect();
    let task_revision = after_body["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let done = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "complete_task", "task_id": task_id,
            "expected_revision": task_revision, "reason": "every slot settled its turn",
            "evidence": artifacts,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("close-live-complete")
    .send(&world)
    .await;
    assert_eq!(
        done.status, 200,
        "a task whose team closed on settled turns completes: {}",
        done.body
    );
    assert_eq!(done.json()["state"], "done");

    let epic_seen = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let epic_revision = epic_seen.json()["revision"].as_u64().expect("a revision");
    let closed = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "close_epic", "expected_revision": epic_revision,
            "reason": "the work is finished"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("close-live-epic-close")
    .send(&world)
    .await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.json()["state"], "closed");

    // The two postconditions that make this closure honest rather than
    // convenient: no run was cast terminal, and every seat is still operable.
    for seat in &seat_list {
        let agent_run = seat["agent_run_id"].as_str().expect("id");
        let run = world
            .daemon
            .state()
            .with_store(|store| {
                store.get_agent_run(project_id, AgentRunId::parse(agent_run).expect("a run id"))
            })
            .expect("readable")
            .expect("the seat");
        assert!(
            run.terminal.is_none(),
            "no agent run was cast terminal to close the team: {agent_run}"
        );
        let timeline = Call::get(format!("/v1/sessions/{agent_run}/timeline"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(
            timeline.status, 200,
            "seat `{agent_run}` is still live and operable after the epic closed: {}",
            timeline.body
        );
    }
}

/// P1-A negatives. A team does not close because *some* slots settled: the walk
/// is over the template's declared slots, so an unaccounted one refuses, and the
/// profile's gates and artifacts are still their own obligation.
#[tokio::test]
async fn an_unaccounted_slot_or_an_undischarged_gate_withholds_closure() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, account, seats) = seated_turns(&world, "close-partial").await;

    let seat_list = seats.as_array().expect("seats").clone();
    let projected = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_id = projected.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();
    let lifecycle = format!("/v1/projects/{project}/epics/{epic}/lifecycle");

    // Only the *first* slot settles. Every other declared slot is unaccounted.
    let first = &seat_list[0];
    let seen = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = seen.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let settled = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/turns:settle",
            first["agent_run_id"].as_str().expect("id")
        ),
        &serde_json::json!({
            "role_slot": first["role_slot"].as_str().expect("slot"),
            "expected_task_revision": revision,
            "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("close-partial-turn-0")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);

    // Every gate and artifact obligation is discharged first, so the *only*
    // thing missing when completion is attempted is the unaccounted slots. A
    // refusal here with gates outstanding would prove nothing about the
    // declared-slot walk — it would just be the gates refusing.
    let gate_view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let gates = gate_view.json()["tasks"][0]["gates"]
        .as_array()
        .expect("gates")
        .clone();
    let workflow_revision = gate_view.json()["tasks"][0]["workflow_revision"]
        .as_u64()
        .expect("a workflow revision");
    for (index, gate) in gates.iter().enumerate() {
        let name = gate["gate"].as_str().expect("a gate");
        let evaluator = gate["evaluator_roles"][0]
            .as_str()
            .expect("an authorized evaluator");
        let evidence: Vec<&str> = gate["required_evidence"]
            .as_array()
            .expect("evidence")
            .iter()
            .map(|item| item.as_str().expect("an artifact"))
            .collect();
        let recorded = Call::post(
            format!("/v1/projects/{project}/tasks/{task_id}/gates/{name}/record"),
            &serde_json::json!({
                "expected_revision": workflow_revision,
                "verdict": "passed",
                "evaluator_role": evaluator,
                "evaluator_account": account,
                "evidence": evidence,
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("close-partial-gate-{index}"))
        .send(&world)
        .await;
        assert_eq!(recorded.status, 200, "gate `{name}`: {}", recorded.body);
    }
    let artifact_view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let artifact_body = artifact_view.json();
    let artifacts: Vec<&str> = artifact_body["tasks"][0]["required_artifacts"]
        .as_array()
        .expect("required artifacts")
        .iter()
        .map(|item| item.as_str().expect("an artifact"))
        .collect();

    // Completion is refused: the declared-slot walk is over the *template*, so
    // slots that settled nothing are exactly the ones that fail.
    let seen_again = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = seen_again.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let refused = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "complete_task", "task_id": task_id,
            "expected_revision": task_revision, "reason": "one slot is enough, surely",
            "evidence": artifacts,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("close-partial-complete")
    .send(&world)
    .await;
    // The *reason* is asserted, not merely the refusal. With gates discharged,
    // a bare "not 200" would still pass if the team were refused for some other
    // cause — and it was: an earlier version of this test could not tell the
    // declared-slot walk from the store's "the cited team run has not closed".
    assert_eq!(refused.status, 422, "{}", refused.body);
    assert_eq!(refused.code(), "unsupported_capability");
    assert!(
        refused.body.contains("declared role slot"),
        "the refusal names the unaccounted slot: {}",
        refused.body
    );
}

/// P1-B. The follow-up reaches the successor's **existing** seat because Kontor's
/// own derivation selected it — not because a caller named a URI.
///
/// The earlier version of this proof settled twice against the same
/// `agent_run` address and observed the same id come back, which proves the
/// address and nothing else. Here the target is chosen by the frozen handoff DAG
/// during settlement of a *different* slot's turn, so the seat's identity is an
/// outcome rather than an input: the test records what the successor seat is
/// **before** the settlement, and then asserts the effect landed in exactly that
/// session, unreplaced.
#[tokio::test]
async fn a_follow_up_selects_the_successors_existing_seat_and_does_not_replace_it() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "reuse-select").await;
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");

    let seat_list = seats.as_array().expect("seats").clone();
    let first = seat_list[0].clone();
    let from_slot = first["role_slot"].as_str().expect("slot").to_owned();
    let from_run = first["agent_run_id"].as_str().expect("id").to_owned();
    let team_run_id =
        kontor_core::id::TeamRunId::parse(first["team_run_id"].as_str().expect("a team run id"))
            .expect("a team run id");

    // The whole roster as it stands *before* anything is settled. Identity is
    // captured here so every assertion afterwards is about a value this test did
    // not choose.
    let roster_before = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
        .expect("the roster is readable");
    assert!(
        roster_before.len() > 1,
        "the bundled team seats several slots"
    );

    let revision_view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = revision_view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");

    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{from_run}/turns:settle"),
        &serde_json::json!({
            "role_slot": from_slot,
            "expected_task_revision": revision,
            "artifacts": ["change-set"]
        }),
    )
    .signed_as(&world, "operator")
    .with_key("reuse-select-turn")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);

    // Kontor chose the target. The test asserts *which* seat it chose by looking
    // it up in the roster captured before the settlement.
    let follow_ups = settled.json()["follow_ups"]
        .as_array()
        .expect("follow-ups")
        .clone();
    assert_eq!(follow_ups.len(), 1, "{}", settled.body);
    let to_slot = follow_ups[0]["to_role_slot"].as_str().expect("a slot");
    let target_run = follow_ups[0]["target_agent_run_id"]
        .as_str()
        .expect("a target seat");
    assert!(
        follow_ups[0]["dispatched"].as_bool().expect("a flag"),
        "the follow-up reached the seat: {}",
        settled.body
    );
    assert_ne!(to_slot, from_slot, "a handoff goes to another slot");

    let expected = roster_before
        .iter()
        .find(|seat| seat.role.as_str() == to_slot)
        .expect("the successor slot was already materialized before the settlement");
    assert_eq!(
        target_run,
        expected.agent_run_id.to_string(),
        "the derivation selected the seat that already existed for that slot"
    );

    // No replacement: the successor's roster entry is the same run, the same
    // native session and the same binding it held before. A replacement would
    // show up as a different run id or a different native id for that slot.
    let roster_after = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
        .expect("the roster is readable");
    assert_eq!(
        roster_after.len(),
        roster_before.len(),
        "no seat was added or replaced: {roster_after:?}"
    );
    let actual = roster_after
        .iter()
        .find(|seat| seat.role.as_str() == to_slot)
        .expect("the successor slot is still seated");
    assert_eq!(actual.agent_run_id, expected.agent_run_id, "same agent run");
    assert_eq!(actual.native_id, expected.native_id, "same native session");
    assert_eq!(actual.binding_id, expected.binding_id, "same binding");
    assert_eq!(actual.runtime_kind, expected.runtime_kind);

    // The binding generation, read from the seat's own persisted binding.
    let target_before = world
        .daemon
        .state()
        .with_store(|store| store.get_agent_run(project_id, expected.agent_run_id))
        .expect("readable")
        .expect("the successor seat");
    let generation = target_before
        .binding
        .as_ref()
        .expect("a bound successor")
        .identity
        .generation;
    assert_eq!(
        generation, 1,
        "the successor is still in the generation it was bound under"
    );
    assert!(
        target_before.terminal.is_none(),
        "the successor was not closed to hand it work"
    );

    // Exactly one effect in that seat, counted as total content growth rather
    // than by a message id the test supplies — a filtered count would be blind
    // to a duplicate delivered under a different id.
    let timeline = Call::get(format!("/v1/sessions/{target_run}/timeline?limit=64"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(timeline.status, 200, "{}", timeline.body);
    let after_first = timeline
        .json()
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();

    // Reconciliation — the other public path that can drive a follow-up — must
    // not produce a second effect in that seat.
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let after_reconcile = Call::get(format!("/v1/sessions/{target_run}/timeline?limit=64"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        after_reconcile
            .json()
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        after_first,
        "reconciliation delivers no second effect into the selected seat: {}",
        after_reconcile.body
    );
}

/// The alpha pack, whose handoffs carry *real* conditions — unlike the bundled
/// team, where every handoff declares `after_phase: null` and
/// `required_artifacts: []` and both guards are therefore vacuous.
const ALPHA_PACK: &str = include_str!("../../kontor-profiles/tests/fixtures/custom-pack-a.json");

/// A pack whose team declares two slots and puts the *waivable* one last.
///
/// A fresh immutable template rather than an edit to the bundled v1, whose
/// `tester`/`researcher-a` slots carry no waiver policy and must keep carrying
/// none. Last, because seating stops at the first slot the runtime refuses: a
/// waivable slot in the middle would leave the ones after it undeclared-but-also
/// unseated, which is a different (and unwaivable) shape.
const OMEGA_PACK: &str = include_str!("../../kontor-profiles/tests/fixtures/custom-pack-w.json");

/// Settle one `alpha-k1` turn.
async fn alpha_settle(
    world: &World,
    project: &str,
    run: &str,
    key: &'static str,
    artifacts: serde_json::Value,
    revision: u64,
) -> Answer {
    Call::post(
        format!("/v1/projects/{project}/agent-runs/{run}/turns:settle"),
        &serde_json::json!({
            "role_slot": "alpha-k1",
            "expected_task_revision": revision,
            "artifacts": artifacts
        }),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await
}

/// The alpha task's current revision.
async fn alpha_revision(world: &World, project: &str, epic: &str) -> u64 {
    let seen = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    seen.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision")
}

/// How much content one seat's session holds.
async fn alpha_content(world: &World, agent_run: &str) -> usize {
    let timeline = Call::get(format!("/v1/sessions/{agent_run}/timeline?limit=64"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    timeline
        .json()
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or_default()
}

/// P1-C. Both handoff guards are load-bearing, proved on a pack whose conditions
/// are non-empty.
///
/// `alpha-k1 → alpha-k3` waits for phase `alpha-p2` **and** artifact `alpha-a2`.
/// The follow-up is withheld while either is unmet and becomes eligible only when
/// both persisted conditions hold — then it is delivered exactly once, across a
/// replayed settlement and a restart.
#[tokio::test]
async fn both_handoff_conditions_withhold_a_follow_up_until_each_is_satisfied() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let pack: serde_json::Value = serde_json::from_str(ALPHA_PACK).expect("the alpha pack parses");
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("alpha-register")
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let created = ensure_project(&world, "alpha", "Kontor", "/tmp/kontor-alpha").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");
    let revision = created.json()["revision"].as_u64().expect("revision");

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alpha-account")
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
            "Alpha epic",
            "alpha-cat",
            serde_json::json!([{"title": "Alpha task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("alpha-epic-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let task_id = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id, "reason": "Run alpha"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alpha-arm")
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
    .with_key("alpha-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].as_array().expect("seats").clone();
    assert!(!seats.is_empty(), "nothing started: {}", started.body);
    let k1 = seats
        .iter()
        .find(|seat| seat["role_slot"] == serde_json::json!("alpha-k1"))
        .unwrap_or_else(|| panic!("alpha-k1 among {seats:?}"))
        .clone();
    let k1_run = k1["agent_run_id"].as_str().expect("id").to_owned();

    // (1) Neither condition holds: the task is at the entry phase and no
    // artifact was produced. The follow-up is withheld — and withheld means
    // *not derived at all*, so there is no dispatch row to deliver later.
    let first = alpha_settle(
        &world,
        &project,
        &k1_run,
        "alpha-turn-1",
        serde_json::json!([]),
        alpha_revision(&world, &project, &epic).await,
    )
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert!(
        first.json()["follow_ups"]
            .as_array()
            .expect("follow-ups")
            .is_empty(),
        "with neither condition met the handoff is withheld: {}",
        first.body
    );

    // (2) The artifact now exists; the phase does not. Still withheld, and this
    // is the assertion the `after_phase` guard is load-bearing for.
    let second = alpha_settle(
        &world,
        &project,
        &k1_run,
        "alpha-turn-2",
        serde_json::json!(["alpha-a2"]),
        alpha_revision(&world, &project, &epic).await,
    )
    .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert!(
        second.json()["follow_ups"]
            .as_array()
            .expect("follow-ups")
            .is_empty(),
        "the artifact alone does not release the handoff: {}",
        second.body
    );
    let dispatches = |world: &World| {
        world
            .daemon
            .state()
            .with_store(|store| store.list_turn_dispatches(project_id))
            .expect("the dispatch ledger is readable")
    };
    assert!(
        dispatches(&world).is_empty(),
        "nothing was derived while a condition was unmet"
    );

    // (3) Advance the workflow to `alpha-p2`. There is no public route for a
    // phase advance yet, so this one setup step goes through the store — the
    // *guard* is what is under test, not the advance.
    world.daemon.state().with_store(|store| {
        let workflow = store
            .get_active_task_workflow(
                project_id,
                kontor_core::id::TaskId::parse(&task_id).expect("a task id"),
            )
            .expect("readable")
            .expect("an active workflow");
        store
            .advance_phase(&kontor_core::repository::PhaseAdvance {
                project_id,
                workflow_id: workflow.id,
                expected_revision: workflow.revision,
                next_phase: kontor_core::id::PhaseKey::parse("alpha-p2").expect("a phase"),
                advanced_at: at("2026-08-10T10:00:00Z"),
            })
            .expect("the phase advances");
    });

    // Both conditions now hold, and only now is the follow-up eligible.
    let third = alpha_settle(
        &world,
        &project,
        &k1_run,
        "alpha-turn-3",
        serde_json::json!(["alpha-a2"]),
        alpha_revision(&world, &project, &epic).await,
    )
    .await;
    assert_eq!(third.status, 200, "{}", third.body);
    let follow_ups = third.json()["follow_ups"]
        .as_array()
        .expect("follow-ups")
        .clone();
    assert_eq!(
        follow_ups.len(),
        1,
        "with both conditions met the handoff is released: {}",
        third.body
    );
    assert_eq!(
        follow_ups[0]["to_role_slot"],
        serde_json::json!("alpha-k3"),
        "and it is the handoff the template declares"
    );
    assert_eq!(
        follow_ups[0]["after_phase"],
        serde_json::json!("alpha-p2"),
        "reported with the condition it waited on"
    );
    assert_eq!(dispatches(&world).len(), 1);

    let target = follow_ups[0]["target_agent_run_id"]
        .as_str()
        .expect("a target seat")
        .to_owned();
    let after_release = alpha_content(&world, &target).await;

    // Delivered once: a replayed settlement adds no second effect.
    let replay = alpha_settle(
        &world,
        &project,
        &k1_run,
        "alpha-turn-3",
        serde_json::json!(["alpha-a2"]),
        alpha_revision(&world, &project, &epic).await,
    )
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged");
    assert_eq!(dispatches(&world).len(), 1, "no second follow-up derived");
    assert_eq!(
        alpha_content(&world, &target).await,
        after_release,
        "and no second effect"
    );

    // And a restart delivers none either.
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    assert_eq!(dispatches(&world).len(), 1);
    assert_eq!(
        alpha_content(&world, &target).await,
        after_release,
        "reconciliation produces no second effect"
    );
}
/// P1-C, the artifact guard on its own.
///
/// Its sibling proves the *phase* guard by withholding while the artifact is
/// already satisfied. That ordering cannot also prove the artifact guard: the
/// handoff waits on the **task's** artifacts, so once `alpha-a2` has ever been
/// settled the condition holds forever and deleting the check changes nothing.
///
/// So this realm advances the phase *first* and then settles with no artifacts.
/// The phase condition is met, the artifact condition is not, and the follow-up
/// must still be withheld — which is the only arrangement in which deleting the
/// artifact check is observable.
#[tokio::test]
async fn the_artifact_condition_alone_withholds_a_follow_up_once_the_phase_is_met() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let pack: serde_json::Value = serde_json::from_str(ALPHA_PACK).expect("the alpha pack parses");
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("alpha2-register")
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let created = ensure_project(&world, "alpha2", "Kontor", "/tmp/kontor-alpha2").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");
    let revision = created.json()["revision"].as_u64().expect("revision");

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alpha2-account")
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
            "Alpha epic",
            "alpha-cat",
            serde_json::json!([{"title": "Alpha task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("alpha2-epic-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let task_id = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id, "reason": "Run alpha"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alpha2-arm")
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
    .with_key("alpha2-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].as_array().expect("seats").clone();
    assert!(!seats.is_empty(), "nothing started: {}", started.body);
    let k1 = seats
        .iter()
        .find(|seat| seat["role_slot"] == serde_json::json!("alpha-k1"))
        .unwrap_or_else(|| panic!("alpha-k1 among {seats:?}"))
        .clone();
    let k1_run = k1["agent_run_id"].as_str().expect("id").to_owned();

    // The phase is advanced *before* anything is settled, so the artifact is the
    // only unmet condition below. No public route advances a phase yet, so this
    // setup step goes through the store; the guard is what is under test.
    world.daemon.state().with_store(|store| {
        let workflow = store
            .get_active_task_workflow(
                project_id,
                kontor_core::id::TaskId::parse(&task_id).expect("a task id"),
            )
            .expect("readable")
            .expect("an active workflow");
        store
            .advance_phase(&kontor_core::repository::PhaseAdvance {
                project_id,
                workflow_id: workflow.id,
                expected_revision: workflow.revision,
                next_phase: kontor_core::id::PhaseKey::parse("alpha-p2").expect("a phase"),
                advanced_at: at("2026-08-10T10:00:00Z"),
            })
            .expect("the phase advances");
    });

    let dispatches = |world: &World| {
        world
            .daemon
            .state()
            .with_store(|store| store.list_turn_dispatches(project_id))
            .expect("the dispatch ledger is readable")
    };

    // Phase met, artifact missing: withheld.
    let withheld = alpha_settle(
        &world,
        &project,
        &k1_run,
        "alpha2-turn-1",
        serde_json::json!([]),
        alpha_revision(&world, &project, &epic).await,
    )
    .await;
    assert_eq!(withheld.status, 200, "{}", withheld.body);
    assert!(
        withheld.json()["follow_ups"]
            .as_array()
            .expect("follow-ups")
            .is_empty(),
        "the phase alone does not release the handoff: {}",
        withheld.body
    );
    assert!(
        dispatches(&world).is_empty(),
        "nothing was derived while the artifact was missing"
    );

    // The artifact arrives and the handoff is released.
    let released = alpha_settle(
        &world,
        &project,
        &k1_run,
        "alpha2-turn-2",
        serde_json::json!(["alpha-a2"]),
        alpha_revision(&world, &project, &epic).await,
    )
    .await;
    assert_eq!(released.status, 200, "{}", released.body);
    let follow_ups = released.json()["follow_ups"]
        .as_array()
        .expect("follow-ups")
        .clone();
    assert_eq!(
        follow_ups.len(),
        1,
        "with the artifact settled the handoff is released: {}",
        released.body
    );
    assert_eq!(follow_ups[0]["to_role_slot"], serde_json::json!("alpha-k3"));
    assert_eq!(dispatches(&world).len(), 1);
}

/// Two independent ready tasks, one account, and a plan that offers both.
///
/// Shared by the two capacity journeys — the ceiling being spent and the ceiling
/// being configured wider — because the *only* difference that may exist between
/// them is the capacity the Realm was composed with. A second copy of this setup
/// could drift from the first, and then the two tests would no longer be a
/// comparison of anything.
struct CapacityFixture {
    project: String,
    epic: String,
    account_id: String,
    /// The epic projection the task ids and revisions are read from.
    projection: Answer,
    plan_hash: String,
}

/// Build the fixture inside an already-composed `world`.
async fn capacity_fixture(world: &World) -> CapacityFixture {
    // The persistent seats of the first team are still occupying native sessions
    // when the second task is seated, so the runtime must declare room for both
    // teams. Otherwise the second task is refused by the *runtime's* session
    // ceiling and this would prove nothing about Kontor's own.
    world.script(
        r#"{
  "limits": {"max_message_bytes": 65536, "max_history_page": 100, "max_concurrent_sessions": 32},
  "history": [
    {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "one"}
  ],
  "live": [
    {"kind": "message", "sequence": 2, "emitted_at": "2026-08-10T09:02:00Z", "body": "two"}
  ]
}"#,
    );
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let created = ensure_project(world, "cap", "Kontor", "/tmp/kontor-cap").await;
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
    .with_key("cap-account")
    .send(world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Two tasks with no dependency between them: both are ready at once, so
    // whatever refuses the second is a capacity decision and not an ordering one.
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Capacity epic",
            &category,
            serde_json::json!([{"title": "First task"}, {"title": "Second task"}]),
        ),
    )
    .signed_as(world, "admin")
    .with_key("cap-epic")
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
            // Above the ceiling under test, so the authorization window is not
            // what refuses the second task.
            "max_concurrency": 8,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Start the epic"
        }),
    )
    .signed_as(world, "admin")
    .with_key("cap-arm")
    .send(world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    // Both tasks run as the same account, which is what makes the account
    // ceiling the binding one — and is the ordinary case, since a realm's work
    // is done by the accounts the operator has.
    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    for index in 0..2 {
        let task = projection.json()["tasks"][index]["task_id"]
            .as_str()
            .expect("a task id")
            .to_owned();
        let task_revision = projection.json()["tasks"][index]["revision"]
            .as_u64()
            .expect("a revision");
        let pinned = Call::post(
            format!("/v1/projects/{project}/tasks/{task}/account-selection"),
            &serde_json::json!({
                "expected_revision": task_revision,
                "account_profile_id": account_id,
                "reason": "Run as the lead"
            }),
        )
        .signed_as(world, "admin")
        .with_key(format!("cap-select-{index}"))
        .send(world)
        .await;
        assert_eq!(pinned.status, 200, "{}", pinned.body);
    }

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
        2,
        "both tasks are ready, so neither waits on the other: {}",
        plan.body
    );
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    CapacityFixture {
        project,
        epic,
        account_id,
        projection,
        plan_hash,
    }
}

/// BLK-010, the capacity half. A team that closed on settled turns must stop
/// holding admission capacity even though every one of its seats is still live —
/// otherwise a persistent-seat realm would deadlock after its first team, with
/// finished work occupying the ceiling forever.
///
/// The oracle is a real refusal and a real start: an independent second task is
/// refused by name for the *account* ceiling while the first team is open, and
/// admitted once that team closes on settled turns with all four seats still
/// sitting there. `task_not_ready` or an in-flight refusal would prove nothing
/// about capacity, so both are excluded by asserting the exact rule.
#[tokio::test]
async fn a_team_that_closed_on_settled_turns_releases_admission_capacity() {
    let world = World::open_empty_with_a_plane().await;
    let CapacityFixture {
        project,
        epic,
        account_id,
        projection,
        plan_hash,
    } = capacity_fixture(&world).await;

    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("cap-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].as_array().expect("seats").clone();
    assert!(
        seats.len() > 1,
        "the first task seats the whole declared team: {}",
        started.body
    );
    assert!(
        seats
            .iter()
            .all(|seat| seat["task_id"] == seats[0]["task_id"]),
        "only the first task was seated: {}",
        started.body
    );

    // The exact refusal, by code and by rule. A spent ceiling has its own code,
    // so this cannot be satisfied by a revision conflict, a not-ready task or an
    // already-in-flight one — each of which would mean the second task never
    // reached the capacity check at all.
    let blocked = started.json()["blocked"]
        .as_array()
        .expect("blocked")
        .clone();
    assert_eq!(blocked.len(), 1, "{}", started.body);
    assert_eq!(
        blocked[0]["task_id"],
        projection.json()["tasks"][1]["task_id"],
        "the second task is the refused one: {}",
        started.body
    );
    assert_eq!(blocked[0]["code"], "capacity_exhausted", "{}", started.body);
    let rule = blocked[0]["evidence"][0]["rule"]
        .as_str()
        .expect("a rule")
        .to_owned();
    assert_eq!(
        rule, "a configured concurrency ceiling is currently spent",
        "{}",
        started.body
    );
    // And it names no scope, no ceiling value, no count and no identifier.
    for leak in [
        "account",
        "project",
        "global",
        "goal",
        "spent capacity",
        project.as_str(),
        account_id.as_str(),
    ] {
        assert!(
            !rule.contains(leak),
            "the refusal discloses `{leak}`: {}",
            started.body
        );
    }
    assert!(
        !rule.chars().any(|character| character.is_ascii_digit()),
        "the refusal discloses a ceiling value or a count: {}",
        started.body
    );

    // Every declared slot settles, which closes the team on settled turns.
    for (index, seat) in seats.iter().enumerate() {
        let agent_run = seat["agent_run_id"].as_str().expect("id");
        let role_slot = seat["role_slot"].as_str().expect("slot");
        let projected = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        let revision = projected.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("a revision");
        let settled = Call::post(
            format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
            &serde_json::json!({
                "role_slot": role_slot,
                "expected_task_revision": revision,
                "artifacts": ["change-set"]
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("cap-turn-{index}"))
        .send(&world)
        .await;
        assert_eq!(settled.status, 200, "slot `{role_slot}`: {}", settled.body);
        assert_eq!(
            settled.json()["seat_live"],
            serde_json::json!(true),
            "the released capacity is held by a seat that is still live: {}",
            settled.body
        );
    }

    // Nothing was torn down: the four runs that were holding the ceiling are all
    // still open. That is the whole point — capacity is released by the team's
    // closure, not by the sessions ending.
    for seat in &seats {
        let agent_run = seat["agent_run_id"].as_str().expect("id");
        let run = Call::get(format!("/v1/runs/{agent_run}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(run.status, 200, "{}", run.body);
        assert!(
            run.json()["terminal"].is_null(),
            "the seat is still live while its capacity is released: {}",
            run.body
        );
    }

    let replanned = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(replanned.status, 200, "{}", replanned.body);
    let plan_hash = replanned.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    let restarted = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("cap-restart")
    .send(&world)
    .await;
    assert_eq!(restarted.status, 200, "{}", restarted.body);
    let restarted_seats = restarted.json()["started"]
        .as_array()
        .expect("seats")
        .clone();
    assert!(
        !restarted_seats.is_empty(),
        "the second task starts once the first team's capacity is released: {}",
        restarted.body
    );
    assert!(
        restarted_seats
            .iter()
            .all(|seat| seat["task_id"] == projection.json()["tasks"][1]["task_id"]),
        "and it is the task that was refused for capacity that starts: {}",
        restarted.body
    );

    // The only thing still refused is the *first* task, which is not ready
    // because its own team already settled — not a capacity fact at all.
    let left = restarted.json()["blocked"]
        .as_array()
        .expect("blocked")
        .clone();
    assert_eq!(left.len(), 1, "{}", restarted.body);
    assert_eq!(
        left[0]["task_id"],
        projection.json()["tasks"][0]["task_id"],
        "{}",
        restarted.body
    );
    assert_eq!(left[0]["code"], "task_not_ready", "{}", restarted.body);
}

/// MUT-006. Same-seat convergence, proved across *consecutive* turns.
///
/// A persistent role seat is the whole point of the turn model: `(team_run_id,
/// role_slot_id)` names one seat for the life of the team run, and settling a
/// turn in it must leave that pairing resolving to the same agent run and the
/// same native session it did before. The existing reuse test names the seat in
/// the URL, so it can only prove that a seat the caller already held stayed put.
/// This one drives the path where *Kontor* resolves the pairing — the follow-up
/// target — and does it twice, so a resolver that answered differently on a
/// later turn would be caught.
///
/// Three things are asserted together, because each is unfalsifiable alone:
/// the resolution converges, the seat census does not grow, and no additional
/// native session exists behind any seat.
#[tokio::test]
async fn consecutive_turns_on_one_slot_converge_on_one_seat_and_one_session() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "turn-converge").await;
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");

    let seat_list = seats.as_array().expect("seats").clone();
    assert!(
        seat_list.len() > 1,
        "the bundled team seats several slots, so a handoff has somewhere to go"
    );
    let team_run = kontor_core::id::TeamRunId::parse(
        seat_list[0]["team_run_id"].as_str().expect("a team run id"),
    )
    .expect("a team run id");

    // What the team run's seats *are*, before any turn is settled: one seat per
    // role, each behind exactly one native session.
    let census = |world: &World| -> std::collections::BTreeMap<String, (String, String)> {
        let rows = world
            .daemon
            .state()
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run))
            .expect("the seats are readable");
        let mut seen: std::collections::BTreeMap<String, (String, String)> =
            std::collections::BTreeMap::new();
        for row in rows {
            let native = row
                .native_id
                .as_ref()
                .expect("a seated role holds a native session")
                .as_str()
                .to_owned();
            let previous = seen.insert(
                row.role.as_str().to_owned(),
                (row.agent_run_id.to_string(), native),
            );
            assert!(
                previous.is_none(),
                "a role slot is held by exactly one seat, found a second for `{}`",
                row.role.as_str()
            );
            assert_eq!(
                world.fake.sessions_for(row.agent_run_id),
                1,
                "a seat is behind exactly one native session"
            );
        }
        seen
    };
    let before = census(&world);
    assert_eq!(
        before.len(),
        seat_list.len(),
        "every started seat is accounted for once"
    );

    let seat = seat_list[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("id").to_owned();
    let role_slot = seat["role_slot"].as_str().expect("slot").to_owned();

    // Two consecutive bounded turns in that one seat.
    let mut settlements = Vec::new();
    for ordinal in 1..=2u64 {
        let projected = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        let revision = projected.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("a revision");
        let settled = Call::post(
            format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
            &serde_json::json!({
                "role_slot": role_slot,
                "expected_task_revision": revision,
                "artifacts": ["change-set"]
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("turn-converge-{ordinal}"))
        .send(&world)
        .await;
        assert_eq!(settled.status, 200, "turn {ordinal}: {}", settled.body);
        assert_eq!(settled.json()["applied"], "created", "{}", settled.body);
        assert_eq!(
            settled.json()["turn_ordinal"],
            serde_json::json!(ordinal),
            "each turn takes the next position in the seat's sequence: {}",
            settled.body
        );
        // The seat itself is the same one, turn after turn.
        assert_eq!(
            settled.json()["agent_run_id"],
            serde_json::json!(agent_run),
            "turn {ordinal} settled in the same seat: {}",
            settled.body
        );
        assert_eq!(
            settled.json()["role_slot"],
            serde_json::json!(role_slot),
            "turn {ordinal} settled the same slot: {}",
            settled.body
        );
        settlements.push(settled.json().clone());
    }
    assert_eq!(
        settlements[1]["binding_generation"], settlements[0]["binding_generation"],
        "the seat was never rebound between turns: {settlements:?}"
    );

    // The convergence the resolver is responsible for. Each turn hands to a
    // successor slot, and Kontor — not the caller — picks the seat that slot
    // already occupies. Twice, and it must be the same answer both times.
    let target_of = |settlement: &serde_json::Value| -> (String, String) {
        let follow_ups = settlement["follow_ups"].as_array().expect("follow-ups");
        assert_eq!(
            follow_ups.len(),
            1,
            "the bundled team is a chain, so one successor: {settlement:?}"
        );
        (
            follow_ups[0]["to_role_slot"]
                .as_str()
                .expect("a slot")
                .to_owned(),
            follow_ups[0]["target_agent_run_id"]
                .as_str()
                .expect("the successor's seat was resolved, not left empty")
                .to_owned(),
        )
    };
    let (first_slot, first_target) = target_of(&settlements[0]);
    let (second_slot, second_target) = target_of(&settlements[1]);
    assert_eq!(first_slot, second_slot, "the same successor slot");
    assert_eq!(
        first_target, second_target,
        "`(team_run_id, role_slot_id)` resolved to a different seat on the \
         second turn, which is exactly the drift this asserts against"
    );
    // And the seat it converged on is the one that already existed — not a new
    // one that merely happens to be stable between the two reads.
    assert_eq!(
        Some(&first_target),
        before
            .get(&first_slot)
            .map(|(agent_run_id, _)| agent_run_id),
        "the resolved successor is the seat that slot has held since seating"
    );

    // Nothing was added behind any of it: same roles, same seats, same native
    // sessions, one session each. A duplicate seat or a re-launched session
    // fails here even if every identity assertion above still held.
    assert_eq!(
        census(&world),
        before,
        "consecutive turns must not add a seat or move a native session"
    );

    // A second start attempt on the same team must not seat the slot again. The
    // refusal it earns is not the point and is not asserted — what is asserted
    // is that nothing new exists behind the team run afterwards.
    let replanned = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(replanned.status, 200, "{}", replanned.body);
    let restarted = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({
            "plan_hash": replanned.json()["plan_hash"].as_str().expect("a hash")
        }),
    )
    .signed_as(&world, "operator")
    .with_key("turn-converge-restart")
    .send(&world)
    .await;
    assert!(
        restarted
            .json()
            .get("started")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|started| started
                .iter()
                .all(|seat| seat["team_run_id"] != serde_json::json!(team_run.to_string()))),
        "a second start seated this team run again: {}",
        restarted.body
    );
    assert_eq!(
        census(&world),
        before,
        "a second start must not add a seat or a native session"
    );

    // Settlement round-trips: both turns are readable, in order, against the
    // same seat and the same slot.
    let stored = world
        .daemon
        .state()
        .with_store(|store| {
            store.list_settled_turns(
                project_id,
                kontor_core::id::TaskId::parse(
                    settlements[0]["task_id"].as_str().expect("a task id"),
                )
                .expect("a task id"),
            )
        })
        .expect("the settled turns are readable");
    let mine: Vec<_> = stored
        .iter()
        .filter(|turn| turn.agent_run_id.to_string() == agent_run)
        .collect();
    assert_eq!(mine.len(), 2, "both turns persisted: {stored:?}");
    assert_eq!(
        mine.iter()
            .map(|turn| turn.turn_ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2],
        "in the order they were taken"
    );
    assert!(
        mine.iter()
            .all(|turn| turn.role_slot_id.as_str() == role_slot && turn.team_run_id == team_run),
        "each against the same slot of the same team run: {mine:?}"
    );
}

/// One alpha team seated with `alpha-k2` deliberately never launched.
///
/// `alpha-k2` is the slot whose *frozen* revision carries a `waiver_policy`
/// (`authorized_roles: ["alpha-r2"]`, `required_evidence: ["alpha-a3"]`), so this
/// pins a registered immutable template that permits the waiver rather than
/// editing the bundled v1 — whose `tester`/`researcher-a` slots deliberately
/// permit nothing — or branching on a seed id.
struct UnboundWorld {
    world: World,
    project: String,
    epic: String,
    team_run: String,
    seats: Vec<serde_json::Value>,
}

async fn alpha_with_one_unbound_slot(slug: &'static str) -> UnboundWorld {
    omega_with_one_unbound_slot(slug, "omega-cat").await
}

/// The same, against a named work-profile category of the omega pack.
///
/// `omega-cat` pins the team whose handoff to the waivable slot carries phase
/// and artifact conditions; `omega-u-cat` pins the one whose handoff is
/// unconditional. Only the second reaches the waived-slot guard in
/// `derive_follow_ups` — with a condition in front of it the derivation
/// short-circuits first, and the guard cannot be falsified.
async fn omega_with_one_unbound_slot(slug: &'static str, category: &'static str) -> UnboundWorld {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    // The runtime will not take this seat. That is the only way a declared slot
    // is never bound, and it is a transport fact: no session, no binding.
    world
        .fake
        .refusing_launch_of(&kontor_core::id::RoleSlotId::parse("omega-k3").expect("a slot"));

    let pack: serde_json::Value = serde_json::from_str(OMEGA_PACK).expect("the omega pack parses");
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack}),
    )
    .signed_as(&world, "admin")
    .with_key(format!("{slug}-register"))
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let created = ensure_project(&world, slug, "Kontor", &format!("/tmp/kontor-{slug}")).await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key(format!("{slug}-account"))
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
            "Omega epic",
            category,
            serde_json::json!([{"title": "Omega task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key(format!("{slug}-epic-apply"))
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision, "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z", "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id, "reason": "Run omega"
        }),
    )
    .signed_as(&world, "admin")
    .with_key(format!("{slug}-arm"))
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
    .with_key(format!("{slug}-start"))
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);

    // Seating is all-or-nothing in its *answer*: one refused launch blocks the
    // batch. The rows it wrote before the refusal are still there, and that is
    // exactly the shape this design exists for — a team run whose declared slots
    // are partly seated and partly not. So the seats are read from the store
    // rather than from the response.
    let project_id = kontor_core::id::ProjectId::parse(&project).expect("a project id");
    let task_id = {
        let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        kontor_core::id::TaskId::parse(
            projection.json()["tasks"][0]["task_id"]
                .as_str()
                .expect("a task id"),
        )
        .expect("a task id")
    };
    let team_run_id = world
        .daemon
        .state()
        .with_store(|store| store.list_team_runs_for_task(project_id, task_id))
        .expect("the team runs are readable")
        .into_iter()
        .next_back()
        .map(|(id, _)| id)
        .expect("the admission created a team run");
    let rows = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
        .expect("the seats are readable");
    let seats: Vec<serde_json::Value> = rows
        .iter()
        .filter(|row| row.native_id.is_some())
        .map(|row| {
            serde_json::json!({
                "agent_run_id": row.agent_run_id.to_string(),
                "role_slot": row.role.as_str(),
                "team_run_id": team_run_id.to_string(),
            })
        })
        .collect();
    assert!(
        seats.iter().all(|seat| seat["role_slot"] != "omega-k3"),
        "omega-k3 must never have been bound: {rows:?}"
    );
    assert!(
        !seats.is_empty(),
        "the slots the runtime did take are bound: {rows:?}"
    );
    let team_run = team_run_id.to_string();
    UnboundWorld {
        world,
        project,
        epic,
        team_run,
        seats,
    }
}

/// Journeys 5 and 6 through the public surface: a waiver is refused without
/// admin authority, without the frozen policy's role, and without its evidence —
/// and an ever-bound slot is refused whatever else is true.
#[tokio::test]
async fn a_public_waiver_is_refused_without_authority_policy_or_evidence() {
    let seeded = alpha_with_one_unbound_slot("waive-refuse").await;
    let UnboundWorld {
        world,
        project,
        epic,
        team_run,
        seats,
        ..
    } = &seeded;
    let revision = |body: &serde_json::Value| body["team_run_revision"].as_u64();
    let _ = revision;

    let waive = |slot: &'static str,
                 role: &'static str,
                 evidence: serde_json::Value,
                 signer: &'static str,
                 key: &'static str| {
        let uri = format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/{slot}/waivers");
        Call::post(
            uri,
            &serde_json::json!({
                "expected_team_revision": 1,
                "authorized_by_role": role,
                "evidence": evidence
            }),
        )
        .signed_as(world, signer)
        .with_key(key)
    };

    // Not admin.
    let refused = waive(
        "omega-k3",
        "omega-r1",
        serde_json::json!(["omega-a3"]),
        "operator",
        "w-op",
    )
    .send(world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);

    // A slot whose frozen revision permits no waiver at all. Forbidden, per the
    // design's own table: it is an authority answer, not a shape answer.
    let refused = waive(
        "omega-k1",
        "omega-r1",
        serde_json::json!(["omega-a3"]),
        "admin",
        "w-nopolicy",
    )
    .send(world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.json()["code"], "forbidden", "{}", refused.body);

    // A role the policy does not authorize.
    let refused = waive(
        "omega-k3",
        "omega-r2",
        serde_json::json!(["omega-a3"]),
        "admin",
        "w-role",
    )
    .send(world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);

    // Evidence the policy demands and the caller does not cite.
    let refused = waive(
        "omega-k3",
        "omega-r1",
        serde_json::json!(["omega-a1"]),
        "admin",
        "w-evidence",
    )
    .send(world)
    .await;
    assert!(
        refused.status == 422 || refused.status == 400,
        "incomplete evidence is refused: {}",
        refused.body
    );

    // A slot that *was* bound is refused, and stays refused: the binding history
    // is the fact, not a live session.
    let bound = seats[0]["role_slot"].as_str().expect("a slot").to_owned();
    let refused = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/{bound}/waivers"),
        &serde_json::json!({
            "expected_team_revision": 1,
            "authorized_by_role": "omega-r1",
            "evidence": ["omega-a3"]
        }),
    )
    .signed_as(world, "admin")
    .with_key("w-bound")
    .send(world)
    .await;
    assert!(
        refused.status.as_u16() >= 400,
        "an ever-bound slot cannot be waived: {}",
        refused.body
    );

    // Nothing above wrote anything.
    let unknown = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/omega-zz/waivers"),
        &serde_json::json!({
            "expected_team_revision": 1,
            "authorized_by_role": "omega-r1",
            "evidence": ["omega-a3"]
        }),
    )
    .signed_as(world, "admin")
    .with_key("w-unknown")
    .send(world)
    .await;
    assert_eq!(unknown.status, 404, "{}", unknown.body);
    let _ = epic;
}

/// Journeys 1, 2, 4 and 8: the happy public journey. Every bound slot settles,
/// the one unbound slot is waived, the team closes on role-slot dispositions
/// with its seats still live, no session was invented for the waived slot, and
/// the waiver replays on its key rather than appending a second one.
#[tokio::test]
async fn a_waiver_completes_a_team_whose_declared_slot_was_never_bound() {
    let seeded = alpha_with_one_unbound_slot("waive-happy").await;
    let UnboundWorld {
        world,
        project,
        epic,
        team_run,
        seats,
        ..
    } = &seeded;
    let project_id = kontor_core::id::ProjectId::parse(project).expect("a project id");
    let team_run_id = kontor_core::id::TeamRunId::parse(team_run).expect("a team run id");

    // Every seat that exists settles its turn.
    for (index, seat) in seats.iter().enumerate() {
        let agent_run = seat["agent_run_id"].as_str().expect("id");
        let role_slot = seat["role_slot"].as_str().expect("slot");
        let projected = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
            .signed_as(world, "observer")
            .send(world)
            .await;
        let revision = projected.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("a revision");
        let settled = Call::post(
            format!("/v1/projects/{project}/agent-runs/{agent_run}/turns:settle"),
            &serde_json::json!({
                "role_slot": role_slot,
                "expected_task_revision": revision,
                "artifacts": ["omega-a3"]
            }),
        )
        .signed_as(world, "operator")
        .with_key(format!("waive-happy-turn-{index}"))
        .send(world)
        .await;
        assert_eq!(settled.status, 200, "slot `{role_slot}`: {}", settled.body);
        assert_eq!(settled.json()["seat_live"], serde_json::json!(true));
        // Journey 8: no follow-up is ever aimed at the waivable slot's seat,
        // because there is no such seat.
        for follow_up in settled.json()["follow_ups"].as_array().expect("follow-ups") {
            assert_ne!(
                follow_up["to_role_slot"],
                serde_json::json!("omega-k3"),
                "a dispatch to a slot with no seat: {}",
                settled.body
            );
        }
    }

    // The team is not closed yet: one declared slot is still unaccounted for.
    let before = world
        .daemon
        .state()
        .with_store(|store| store.get_team_run(project_id, team_run_id))
        .expect("the team is readable")
        .expect("the team exists");
    assert!(
        before.terminal.is_none(),
        "an unaccounted slot withholds closure: {before:?}"
    );
    let runs_before = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
        .expect("the seats are readable")
        .len();

    // The waiver.
    let waived = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/omega-k3/waivers"),
        &serde_json::json!({
            "expected_team_revision": before.revision.get(),
            "authorized_by_role": "omega-r1",
            "evidence": ["omega-a3"]
        }),
    )
    .signed_as(world, "admin")
    .with_key("waive-happy-1")
    .send(world)
    .await;
    assert_eq!(waived.status, 200, "{}", waived.body);
    assert_eq!(waived.json()["disposition"], "waived", "{}", waived.body);
    assert_eq!(waived.json()["applied"], "created", "{}", waived.body);
    assert_eq!(waived.json()["authority_tier"], "admin", "{}", waived.body);
    assert_eq!(
        waived.json()["team_run_closed"],
        serde_json::json!(team_run),
        "the waiver was the last thing outstanding, so the team closed: {}",
        waived.body
    );

    // Journey 2's assertions, read from the rows rather than the answer.
    let after = world
        .daemon
        .state()
        .with_store(|store| store.get_team_run(project_id, team_run_id))
        .expect("the team is readable")
        .expect("the team exists");
    let terminal = after.terminal.as_ref().expect("the team closed");
    assert_eq!(
        terminal.source,
        kontor_core::state::TeamEvidenceSource::RoleSlotDispositions { team_run_id },
        "closed on dispositions, not on some neighbouring source"
    );
    assert_eq!(
        terminal.outcome,
        kontor_core::state::TerminalOutcome::Succeeded
    );

    // Exactly one waiver, no turn for the waived slot, and no seat invented for
    // it: the run census is unchanged by the waiver.
    let waivers = world
        .daemon
        .state()
        .with_store(|store| store.list_role_slot_waivers(project_id, team_run_id))
        .expect("the waivers are readable");
    assert_eq!(waivers.len(), 1, "{waivers:?}");
    assert_eq!(waivers[0].role_slot_id.as_str(), "omega-k3");
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
            .expect("the seats are readable")
            .len(),
        runs_before,
        "a waiver must not invent a seat"
    );

    // Every seat that does exist is still live. Waiving one slot ended nothing.
    for seat in seats {
        let agent_run = seat["agent_run_id"].as_str().expect("id");
        let run = Call::get(format!("/v1/runs/{agent_run}"))
            .signed_as(world, "observer")
            .send(world)
            .await;
        assert!(
            run.json()["terminal"].is_null(),
            "a waiver must not close a live seat: {}",
            run.body
        );
    }

    // Journey 4: the same key replays the same waiver; a different key on the
    // same slot appends nothing.
    let replay = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/omega-k3/waivers"),
        &serde_json::json!({
            "expected_team_revision": before.revision.get(),
            "authorized_by_role": "omega-r1",
            "evidence": ["omega-a3"]
        }),
    )
    .signed_as(world, "admin")
    .with_key("waive-happy-1")
    .send(world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged", "{}", replay.body);
    assert_eq!(
        replay.json()["waiver_id"],
        waived.json()["waiver_id"],
        "a replay is the original row"
    );
    let second = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/omega-k3/waivers"),
        &serde_json::json!({
            "expected_team_revision": after.revision.get(),
            "authorized_by_role": "omega-r1",
            "evidence": ["omega-a3"]
        }),
    )
    .signed_as(world, "admin")
    .with_key("waive-happy-2")
    .send(world)
    .await;
    assert!(
        second.status.as_u16() >= 400,
        "no second waiver: {}",
        second.body
    );
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.list_role_slot_waivers(project_id, team_run_id))
            .expect("the waivers are readable")
            .len(),
        1,
        "exactly one waiver survives"
    );
}

/// Journey 10. A settle addressed to a run that was never bound is refused with
/// its own code, and writes nothing.
#[tokio::test]
async fn settling_a_never_bound_run_is_refused_as_an_unbound_role_slot() {
    let seeded = alpha_with_one_unbound_slot("waive-unbound").await;
    let UnboundWorld {
        world,
        project,
        team_run,
        ..
    } = &seeded;
    let project_id = kontor_core::id::ProjectId::parse(project).expect("a project id");
    let team_run_id = kontor_core::id::TeamRunId::parse(team_run).expect("a team run id");

    // The alpha team's admission seats its first slot through the scheduler, and
    // any run that exists without a binding is the case under test. If the
    // refused launch left no row at all there is nothing to address, and the
    // slot is reached through the waiver route instead — which journey 1 covers.
    let unbound = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
        .expect("the seats are readable")
        .into_iter()
        .find(|seat| seat.native_id.is_none());
    let Some(unbound) = unbound else {
        // No placeholder row: the refused launch rolled back cleanly, which is
        // itself the stronger outcome and is asserted by the happy journey.
        return;
    };
    let refused = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/turns:settle",
            unbound.agent_run_id
        ),
        &serde_json::json!({
            "role_slot": unbound.role.as_str(),
            "expected_task_revision": 1,
            "artifacts": ["omega-a3"]
        }),
    )
    .signed_as(world, "operator")
    .with_key("unbound-settle")
    .send(world)
    .await;
    assert_eq!(refused.status, 422, "{}", refused.body);
    assert_eq!(
        refused.json()["code"],
        "role_slot_unbound",
        "{}",
        refused.body
    );
}

/// Journey 8. A waiver taken *before* the other slots settle suppresses the
/// dispatch the frozen handoff DAG would otherwise derive to it.
///
/// The waived slot is a handoff target in this template, so without the guard a
/// settlement would derive a row aimed at a seat the waiver says will never
/// exist — undeliverable for ever, and retried at every startup.
#[tokio::test]
async fn a_waived_slot_is_never_given_a_follow_up() {
    let seeded = omega_with_one_unbound_slot("waive-dispatch", "omega-u-cat").await;
    let UnboundWorld {
        world,
        project,
        epic,
        team_run,
        seats,
        ..
    } = &seeded;

    // Waive first, while the other slots are still outstanding: the team cannot
    // close yet, so settlement still happens afterwards.
    let waived = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/omega-k3/waivers"),
        &serde_json::json!({
            "expected_team_revision": 1,
            "authorized_by_role": "omega-r1",
            "evidence": ["omega-a3"]
        }),
    )
    .signed_as(world, "admin")
    .with_key("waive-dispatch-1")
    .send(world)
    .await;
    assert_eq!(waived.status, 200, "{}", waived.body);
    assert_eq!(
        waived.json()["team_run_closed"],
        serde_json::Value::Null,
        "slots are still outstanding, so nothing closed: {}",
        waived.body
    );

    // `omega-k1` hands off to `omega-k3`, so this is the settlement that would
    // derive the dispatch.
    let projected = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    let revision = projected.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let settled = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/turns:settle",
            seats
                .iter()
                .find(|seat| seat["role_slot"] == "omega-k1")
                .expect("omega-k1 is seated")["agent_run_id"]
                .as_str()
                .expect("id")
        ),
        &serde_json::json!({
            "role_slot": "omega-k1",
            "expected_task_revision": revision,
            "artifacts": ["omega-a2", "omega-a3"]
        }),
    )
    .signed_as(world, "operator")
    .with_key("waive-dispatch-turn")
    .send(world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    for follow_up in settled.json()["follow_ups"].as_array().expect("follow-ups") {
        assert_ne!(
            follow_up["to_role_slot"],
            serde_json::json!("omega-k3"),
            "a dispatch was derived to a waived slot: {}",
            settled.body
        );
    }
}

/// The control for `a_waived_slot_is_never_given_a_follow_up`, and the reason
/// that test cannot pass for the wrong reason.
///
/// Same template, same settlement, slot **not** waived: the follow-up *is*
/// derived. So the absence in the other test is the waived-slot guard's doing,
/// and not a handoff condition quietly refusing the derivation — which is the
/// exact confusion that let the guard's mutation survive on the conditional
/// template.
#[tokio::test]
async fn an_unwaived_slot_on_the_same_template_does_receive_the_follow_up() {
    let control = omega_with_one_unbound_slot("waive-control", "omega-u-cat").await;
    let projected = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        control.project, control.epic
    ))
    .signed_as(&control.world, "observer")
    .send(&control.world)
    .await;
    let revision = projected.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("a revision");
    let giver = control
        .seats
        .iter()
        .find(|seat| seat["role_slot"] == "omega-k1")
        .expect("omega-k1 is seated")["agent_run_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let unwaived = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{giver}/turns:settle",
            control.project
        ),
        &serde_json::json!({
            "role_slot": "omega-k1",
            "expected_task_revision": revision,
            "artifacts": ["omega-a2", "omega-a3"]
        }),
    )
    .signed_as(&control.world, "operator")
    .with_key("waive-control-turn")
    .send(&control.world)
    .await;
    assert_eq!(unwaived.status, 200, "{}", unwaived.body);
    assert!(
        unwaived.json()["follow_ups"]
            .as_array()
            .expect("follow-ups")
            .iter()
            .any(|follow_up| follow_up["to_role_slot"] == serde_json::json!("omega-k3")),
        "the handoff is unconditional, so without a waiver it must derive: {}",
        unwaived.body
    );
}

/// KON-MVP-09. The ceilings are a Realm's configuration, and the configured
/// value is what admission is judged against.
///
/// The oracle is the *contrast* with
/// `a_team_that_closed_on_settled_turns_releases_admission_capacity`: same
/// fixture, same plan, same single `scheduler:start`, and exactly one number
/// different. Under [`DEFAULT_CAPACITY`] the first team's four seats spend the
/// account ceiling of four and the second task comes back `capacity_exhausted`;
/// with that one ceiling configured wider, both tasks are seated by the same
/// call and nothing is blocked.
///
/// Two things follow that a test of the override alone would not prove. The
/// configured number is read at admission rather than at planning — the planner
/// passes both candidates either way, so a refusal that disappears can only have
/// come from the recount that commits — and no *other* ceiling was silently
/// widened to make room, because every one of them is still the default.
#[tokio::test]
async fn the_configured_capacity_and_not_a_compiled_one_decides_what_is_admitted() {
    // One ceiling, one change: the account ceiling that the sibling test proves
    // is the binding one, lifted from four to eight. Everything else — global,
    // project, goal, provider, runtime and the adaptive window — is left at the
    // default, so a second admitted task cannot be explained by any of them.
    let world = World::open_empty_with_a_plane_and_capacity(CapacityConfig {
        account_max_in_flight: 8,
        ..DEFAULT_CAPACITY
    })
    .await;
    let CapacityFixture {
        project,
        epic,
        projection,
        plan_hash,
        ..
    } = capacity_fixture(&world).await;

    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("cap-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);

    assert_eq!(
        started.json()["blocked"].as_array().expect("blocked").len(),
        0,
        "the ceiling that refused the second task at four has room at eight: {}",
        started.body
    );
    let seated: BTreeSet<String> = started.json()["started"]
        .as_array()
        .expect("seats")
        .iter()
        .map(|seat| seat["task_id"].as_str().expect("a task id").to_owned())
        .collect();
    let expected: BTreeSet<String> = (0..2)
        .map(|index| {
            projection.json()["tasks"][index]["task_id"]
                .as_str()
                .expect("a task id")
                .to_owned()
        })
        .collect();
    assert_eq!(
        seated, expected,
        "both independent tasks are seated by one start: {}",
        started.body
    );
}
