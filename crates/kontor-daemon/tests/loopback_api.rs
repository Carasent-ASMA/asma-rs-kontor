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
use kontor_core::id::{
    AgentRunId, CanonicalDocument, ConnectorKey, ExternalId, ProjectId, RoleSlotId, TaskId,
    TaskWorkflowId, TicketLinkId,
};
use kontor_core::repository::{
    NewAgentRun, NewObservation, NewProject, NewRuntimeEvent, NewTask, NewTaskWorkflow,
    ProjectRepository, RealmRepository, RunRepository, TicketRepository, WorkflowRepository,
};
use kontor_core::spec::ResolvedWorkProfileSnapshot;
use kontor_core::state::{Freshness, ObservedRunState, RuntimeContact, TaskState};
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

// ---------------------------------------------------------------------------
// The KON-MVP-16 second amendment: lists, the scheduling explanation, external
// ticket evidence and live session discovery
// ---------------------------------------------------------------------------
//
// Each of these routes was staged in the first round of KON-MVP-16 and is wired in
// this one, so what follows is the evidence that each answers from persisted rows or
// from the adapter rather than from a default. The extra mutants they exist to kill:
//
// * a list that answers for the wrong project, or answers an unknown project with an
//   empty list instead of a refusal;
// * a scheduling plan that admits a task with no execution authorization — the one
//   default that would make the whole route a lie;
// * a plan that reports only the first blocker after all;
// * a plan that commits anything, which would be the worst defect available here;
// * a ticket history that answers for an unknown link with an empty page, which
//   reads as "never touched" rather than "no such link";
// * an authority tier slipping on any of the new routes.

/// Give the harness task an active workflow, so it becomes a scheduling candidate.
///
/// The profile comes from the bundled pack the harness already stored, because a run
/// through the real store needs a revision its foreign keys can see.
async fn with_workflow(world: &World) -> TaskWorkflowId {
    let workflow_id = TaskWorkflowId::generate();
    world.daemon.state().with_store(|store| {
        let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
        let entry = pack
            .manifest
            .iter()
            .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
            .expect("the bundled pack seeds at least one category");
        let bundle = kontor_profiles::pack::resolve_profile(
            &pack,
            &entry.category,
            at("2026-08-10T09:00:00Z"),
        )
        .expect("the seeded category resolves");
        let snapshot: ResolvedWorkProfileSnapshot = bundle.profile.clone();
        let entry_phase = snapshot.definition.entry_phase.clone();
        store
            .create_task_workflow(&NewTaskWorkflow {
                id: workflow_id,
                project_id: world.project,
                task_id: world.task,
                snapshot,
                current_phase: entry_phase,
                created_at: at("2026-08-10T09:05:00Z"),
            })
            .expect("the task workflow is created");
    });
    workflow_id
}

/// Link the harness task to an external ticket.
fn with_ticket(world: &World) -> TicketLinkId {
    let link_id = TicketLinkId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_ticket_link(&kontor_core::repository::NewTicketLink {
                id: link_id,
                project_id: world.project,
                task_id: world.task,
                connector: ConnectorKey::parse("test.connector").expect("a valid connector key"),
                external_issue_key: ExternalId::parse("ASMA-7760").expect("a valid issue key"),
                created_at: at("2026-08-10T09:10:00Z"),
            })
            .expect("the ticket link is created");
    });
    link_id
}

/// Persist one extra task in the harness project.
fn another_task(world: &World, title: &str) -> TaskId {
    let task_id = TaskId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_task(&NewTask {
                id: task_id,
                project_id: world.project,
                mini_project_id: None,
                title: name(title),
                module: None,
                state: TaskState::Ready,
                created_at: at("2026-08-10T09:20:00Z"),
            })
            .expect("a task is created");
    });
    task_id
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_project_list_names_the_realm_and_the_project_the_harness_seeded() {
    let world = World::open().await;
    let answer = Call::get("/v1/projects")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(
        answer.realm(),
        world.realm_id(),
        "every answer names its realm"
    );
    let listed = answer.json()["value"]
        .as_array()
        .expect("a list is an array")
        .clone();
    assert_eq!(listed.len(), 1, "the harness seeds exactly one project");
    assert_eq!(
        listed[0]["project_id"],
        serde_json::json!(world.project.to_string())
    );
    assert!(
        listed[0]["revision"]
            .as_u64()
            .is_some_and(|value| value >= 1),
        "a list entry carries the revision a later write must present: {}",
        answer.body
    );
}

#[tokio::test]
async fn the_mission_and_run_lists_answer_for_the_project_they_were_asked_about() {
    let world = World::open().await;
    let (agent_run_id, _snapshot) = world.launch().await;

    let missions = Call::get(format!("/v1/projects/{}/team-runs", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(missions.status, 200, "{}", missions.body);
    let listed = missions.json()["value"]
        .as_array()
        .expect("a list is an array")
        .clone();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0]["team_run_id"],
        serde_json::json!(world.team_run.to_string())
    );
    assert_eq!(
        listed[0]["task_id"],
        serde_json::json!(world.task.to_string()),
        "a mission names the task it serves"
    );

    let runs = Call::get(format!("/v1/projects/{}/runs", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(runs.status, 200, "{}", runs.body);
    let listed = runs.json()["value"]
        .as_array()
        .expect("a list is an array")
        .clone();
    assert!(
        listed
            .iter()
            .any(|run| run["agent_run_id"] == serde_json::json!(agent_run_id.to_string())),
        "the launched run appears in its project's run list: {}",
        runs.body
    );
}

#[tokio::test]
async fn a_run_list_filtered_by_mission_answers_only_that_missions_runs() {
    let world = World::open().await;
    let (agent_run_id, _snapshot) = world.launch().await;

    let matching = Call::get(format!(
        "/v1/projects/{}/runs?team_run={}",
        world.project, world.team_run
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(matching.status, 200, "{}", matching.body);
    assert!(
        matching.json()["value"]
            .as_array()
            .expect("a list is an array")
            .iter()
            .any(|run| run["agent_run_id"] == serde_json::json!(agent_run_id.to_string()))
    );

    // A filter naming another mission must answer empty rather than ignoring the
    // filter, which is the failure a caller would never notice.
    let other = kontor_core::id::TeamRunId::generate();
    let empty = Call::get(format!(
        "/v1/projects/{}/runs?team_run={other}",
        world.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(empty.status, 200, "{}", empty.body);
    assert_eq!(
        empty.json()["value"]
            .as_array()
            .expect("a list is an array")
            .len(),
        0,
        "a filter that matches nothing answers nothing, not everything"
    );
}

#[tokio::test]
async fn an_unknown_project_is_refused_rather_than_answered_with_an_empty_list() {
    let world = World::open().await;
    let unknown = kontor_core::id::ProjectId::generate();
    let answer = Call::get(format!("/v1/projects/{unknown}/tasks"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 404, "{}", answer.body);
    assert_eq!(answer.code(), "not_found");
}

// ---------------------------------------------------------------------------
// The scheduling explanation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_task_with_no_execution_authorization_is_refused_and_never_admitted() {
    // The most important assertion in this file. The snapshot is assembled with no
    // authorization evidence when none is recorded, and the scheduler's answer to
    // that is a refusal. If a default ever made this admit, the whole route would be
    // telling an operator that unarmed work is runnable.
    let world = World::open().await;
    with_workflow(&world).await;

    let answer = Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(answer.realm(), world.realm_id());
    let plan = answer.json()["value"].clone();
    assert_eq!(
        plan["admitted"],
        serde_json::json!(0),
        "nothing is armed, so nothing is admitted: {}",
        answer.body
    );
    let decisions = plan["decisions"]
        .as_array()
        .expect("a plan carries decisions")
        .clone();
    let decision = decisions
        .iter()
        .find(|decision| decision["task_id"] == serde_json::json!(world.task.to_string()))
        .expect("the seeded task is a candidate");
    assert_eq!(decision["admitted"], serde_json::json!(false));
    assert_eq!(
        decision["code"],
        serde_json::json!("authorization_missing"),
        "the refusal is the real one and not a placeholder: {}",
        answer.body
    );
}

#[tokio::test]
async fn a_plan_reports_every_blocker_and_not_only_the_first() {
    let world = World::open().await;
    with_workflow(&world).await;
    let answer = Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let decisions = answer.json()["value"]["decisions"]
        .as_array()
        .expect("a plan carries decisions")
        .clone();
    let decision = decisions
        .iter()
        .find(|decision| decision["task_id"] == serde_json::json!(world.task.to_string()))
        .expect("the seeded task is a candidate")
        .clone();
    let blockers = decision["blockers"]
        .as_array()
        .expect("a refused candidate carries its blockers")
        .clone();
    assert!(
        !blockers.is_empty(),
        "a refused candidate must say what refused it: {}",
        answer.body
    );
    // The list is in evaluation order and its first entry is the decision's own
    // code, which is what makes the two impossible to contradict.
    assert_eq!(
        blockers[0]["code"], decision["code"],
        "the first blocker is the code the decision reports: {}",
        answer.body
    );
    for blocker in &blockers {
        assert!(
            blocker["blocker"]
                .as_str()
                .is_some_and(|name| !name.is_empty()),
            "every blocker names itself: {blocker}"
        );
    }
}

#[tokio::test]
async fn a_plan_names_every_value_it_assembled_rather_than_read() {
    let world = World::open().await;
    let answer = Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let defaults = answer.json()["value"]["assembled_defaults"]
        .as_array()
        .expect("a plan discloses its assembled defaults")
        .clone();
    assert!(
        defaults.len() >= 7,
        "each snapshot field with no stored source must be named: {}",
        answer.body
    );
    let joined = defaults
        .iter()
        .filter_map(|note| note.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for expected in ["priority", "origin", "account pin", "capacity"] {
        assert!(
            joined.contains(expected),
            "`{expected}` is assembled and must be disclosed: {joined}"
        );
    }
}

#[tokio::test]
async fn a_task_with_no_active_workflow_is_named_rather_than_silently_dropped() {
    let world = World::open().await;
    let extra = another_task(&world, "A task with no workflow");
    let answer = Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let plan = answer.json()["value"].clone();
    let without: Vec<String> = plan["without_workflow"]
        .as_array()
        .expect("a plan names the tasks it could not consider")
        .iter()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        without.contains(&extra.to_string()),
        "a task with no workflow is reported, not dropped: {}",
        answer.body
    );
    assert!(
        plan["considered"].as_u64().is_some_and(|count| count >= 2),
        "the count is of tasks looked at, not of candidates built: {}",
        answer.body
    );
}

#[tokio::test]
async fn a_plan_admits_nothing_and_queues_nothing() {
    // A read that committed an admission would be the worst defect available here,
    // so it is asserted directly: the run list is the same before and after.
    let world = World::open().await;
    with_workflow(&world).await;
    let before = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs(world.project, None))
        .expect("the run list is readable");

    Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;

    let after = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs(world.project, None))
        .expect("the run list is readable");
    assert_eq!(
        before.len(),
        after.len(),
        "planning is a read and must create no run"
    );
}

#[tokio::test]
async fn a_project_with_no_work_calendar_is_planned_without_a_calendar_blocker() {
    // Absence of a calendar is not a closed calendar, and it is not a reason to
    // refuse an answer either. The harness seeds no assignment, so the plan route
    // answers and no decision names `calendar` among its blockers.
    let world = World::open().await;
    with_workflow(&world).await;
    assert!(
        !world
            .daemon
            .state()
            .with_store(|store| store.has_calendar_assignment(world.project))
            .expect("the flag is readable"),
        "the harness seeds no calendar assignment"
    );

    let answer = Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        answer.status, 200,
        "a project with no calendar has no window to resolve: {}",
        answer.body
    );
    assert!(
        !answer.body.contains("calendar_closed"),
        "an unconfigured project is unrestricted, never closed: {}",
        answer.body
    );
}

/// A configured, currently closed calendar is resolved by KON-MVP-21 and reaches
/// the plan route as an ordinary blocker — not as a refusal to answer.
#[tokio::test]
async fn a_project_whose_calendar_is_closed_is_planned_and_reports_the_calendar_blocker() {
    let world = World::open().await;
    with_workflow(&world).await;

    // A calendar that is open for one minute a week, in a zone the bundled tzdb
    // knows. Whenever this test runs, the calendar is almost certainly closed —
    // and the assertion below only needs "not admitted because of the calendar",
    // which is true for every instant outside that minute.
    world.daemon.state().with_store(|store| {
        use kontor_core::calendar::{
            CalendarProfileSpec, HolidayMergePolicy, IanaTimeZone, Weekday, WeeklyWindow,
            WorkCalendarAssignment,
        };
        use kontor_core::repository::{CalendarRepository, SpecRepository};

        let profile = CalendarProfileSpec {
            schema_version: kontor_core::id::SCHEMA_VERSION,
            profile_id: kontor_core::id::CalendarProfileId::generate(),
            version: kontor_core::id::SpecVersion::FIRST,
            name: name("One minute a week"),
            windows: vec![WeeklyWindow {
                weekday: Weekday::Monday,
                start: "03:00:00".parse().expect("a civil time"),
                end: "03:01:00".parse().expect("a civil time"),
            }],
            holiday_merge: HolidayMergePolicy::TreatAsClosed,
            drain_lead_minutes: 0,
        };
        store
            .insert_calendar_profile(&profile)
            .expect("the profile revision is stored");
        store
            .assign_calendar(&WorkCalendarAssignment {
                id: kontor_core::id::WorkCalendarId::generate(),
                project_id: world.project,
                profile_id: profile.profile_id,
                profile_version: profile.version,
                timezone: IanaTimeZone::parse("Europe/Oslo").expect("a bundled tzdb zone"),
                window_override: None,
                active: true,
                created_at: at("2026-08-10T09:00:00Z"),
                retired_at: None,
            })
            .expect("the assignment is stored");
    });

    let answer = Call::get(format!("/v1/projects/{}/scheduler/plan", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        answer.status, 200,
        "a configured calendar is resolved, not refused: {}",
        answer.body
    );
    assert_eq!(
        answer.json()["value"]["admitted"],
        serde_json::json!(0),
        "a closed calendar admits no new top-level work: {}",
        answer.body
    );
    let blockers = answer.json()["value"]["decisions"][0]["blockers"].to_string();
    assert!(
        blockers.contains("calendar"),
        "the calendar is one of the blockers a reader can see: {blockers}"
    );
}

// ---------------------------------------------------------------------------
// External tickets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_ticket_link_is_listed_and_read_with_the_revision_a_command_needs() {
    let world = World::open().await;
    let link_id = with_ticket(&world);

    let listed = Call::get(format!("/v1/projects/{}/tickets", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(listed.status, 200, "{}", listed.body);
    assert_eq!(listed.realm(), world.realm_id());
    let links = listed.json()["value"]
        .as_array()
        .expect("a list is an array")
        .clone();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["link_id"], serde_json::json!(link_id.to_string()));
    assert_eq!(
        links[0]["external_issue_key"],
        serde_json::json!("ASMA-7760")
    );
    assert!(
        links[0]["revision"]
            .as_u64()
            .is_some_and(|value| value >= 1),
        "a convergence command needs this revision: {}",
        listed.body
    );

    let shown = Call::get(format!("/v1/projects/{}/tickets/{link_id}", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(shown.status, 200, "{}", shown.body);
    let ticket = shown.json()["value"].clone();
    assert_eq!(
        ticket["link"]["link_id"],
        serde_json::json!(link_id.to_string())
    );
    // Nothing has been projected or observed yet, and that is reported as absence
    // rather than as an empty object a caller would read as "no fields to write".
    assert!(ticket["projection"].is_null(), "{}", shown.body);
    assert!(ticket["observed"].is_null(), "{}", shown.body);
    assert_eq!(ticket["unresolved_conflicts"], serde_json::json!(0));
}

#[tokio::test]
async fn a_ticket_history_for_an_unknown_link_is_refused_and_not_answered_empty() {
    // An empty page would read as "this ticket has never been touched", which is a
    // different and wrong statement about a link that does not exist.
    let world = World::open().await;
    let unknown = TicketLinkId::generate();
    for route in ["comments", "transitions"] {
        let answer = Call::get(format!(
            "/v1/projects/{}/tickets/{unknown}/{route}",
            world.project
        ))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
        assert_eq!(answer.status, 404, "{route}: {}", answer.body);
        assert_eq!(answer.code(), "not_found", "{route}");
    }
}

#[tokio::test]
async fn a_linked_tickets_histories_answer_empty_because_nothing_has_happened_yet() {
    let world = World::open().await;
    let link_id = with_ticket(&world);
    for route in ["comments", "transitions"] {
        let answer = Call::get(format!(
            "/v1/projects/{}/tickets/{link_id}/{route}",
            world.project
        ))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
        assert_eq!(answer.status, 200, "{route}: {}", answer.body);
        assert_eq!(answer.realm(), world.realm_id(), "{route}");
        assert_eq!(
            answer.json()["value"]
                .as_array()
                .expect("a list is an array")
                .len(),
            0,
            "{route} has nothing recorded yet, and says so with a page rather than a refusal"
        );
    }
}

#[tokio::test]
async fn a_ticket_convergence_command_is_receipt_backed_and_ticket_scoped() {
    let world = World::open().await;
    let link_id = with_ticket(&world);
    let revision = world
        .daemon
        .state()
        .with_store(|store| store.get_ticket_link(world.project, link_id))
        .expect("the link is readable")
        .expect("the link exists")
        .revision;

    let body = serde_json::json!({
        "project_id": world.project.to_string(),
        "target": { "kind": "ticket_link", "link_id": link_id.to_string() },
        "expected_revision": revision.get(),
        "desired_state": serde_json::Value::Null,
        "intent": { "schema_version": 1, "reason": "converge the ticket" },
        "payload": { "schema_version": 1 },
    });
    let answer = Call::post("/v1/commands/sync_ticket", &body)
        .signed_as(&world, "operator")
        .with_key("sync-once")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let receipt = answer.json();
    assert_eq!(
        receipt["value"]["kind"],
        serde_json::json!("sync_ticket"),
        "the receipt records the command that was asked for"
    );
    assert_eq!(
        receipt["value"]["target"]["link_id"],
        serde_json::json!(link_id.to_string()),
        "and it is scoped to the one ticket link it named"
    );
    assert_eq!(receipt["replayed"], serde_json::json!(false));

    // The same key and the same intent replays rather than converging twice.
    let replay = Call::post("/v1/commands/sync_ticket", &body)
        .signed_as(&world, "operator")
        .with_key("sync-once")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["replayed"], serde_json::json!(true));
    assert_eq!(
        replay.json()["value"]["receipt_id"],
        receipt["value"]["receipt_id"],
        "a replay returns the receipt that was already durable"
    );
}

// ---------------------------------------------------------------------------
// Live session discovery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_discovery_reports_which_native_sessions_this_realm_already_holds() {
    let world = World::open().await;
    let (_agent_run_id, snapshot) = world.launch().await;

    let answer = Call::get(format!("/v1/runtimes/{}/sessions", harness::fake_family()))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert_eq!(answer.realm(), world.realm_id());
    let sessions = answer.json()["value"]
        .as_array()
        .expect("a list is an array")
        .clone();
    let native = snapshot.identity().native_id.to_string();
    let found = sessions
        .iter()
        .find(|session| session["native_id"] == serde_json::json!(native));
    if let Some(session) = found {
        assert_eq!(
            session["bound"],
            serde_json::json!(true),
            "a session this realm holds a binding for is reported as bound: {}",
            answer.body
        );
        assert_eq!(
            session["runtime_kind"],
            serde_json::json!(harness::fake_family().to_string())
        );
    }
}

#[tokio::test]
async fn discovery_against_an_unconfigured_runtime_is_not_found() {
    let world = World::open().await;
    let answer = Call::get("/v1/runtimes/absent.runtime/sessions")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 404, "{}", answer.body);
    assert_eq!(answer.code(), "not_found");
}

#[tokio::test]
async fn discovery_against_a_runtime_that_never_declared_it_is_unsupported() {
    // Not an empty list: an empty list would say "this runtime owns no sessions",
    // which is not something a runtime without discovery can be asked.
    let world = World::open_with(harness::capabilities_without(&[
        kontor_runtime::capability::RuntimeCapability::Discovery,
    ]))
    .await;
    let answer = Call::get(format!("/v1/runtimes/{}/sessions", harness::fake_family()))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 422, "{}", answer.body);
    assert_eq!(answer.code(), "unsupported_capability");
}

// ---------------------------------------------------------------------------
// Authority
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_new_read_requires_a_credential_and_answers_an_observer() {
    let world = World::open().await;
    let link_id = with_ticket(&world);
    let routes = vec![
        "/v1/projects".to_owned(),
        format!("/v1/projects/{}/team-runs", world.project),
        format!("/v1/projects/{}/runs", world.project),
        format!("/v1/projects/{}/scheduler/plan", world.project),
        format!("/v1/projects/{}/tickets", world.project),
        format!("/v1/projects/{}/tickets/{link_id}", world.project),
        format!("/v1/projects/{}/tickets/{link_id}/comments", world.project),
        format!(
            "/v1/projects/{}/tickets/{link_id}/transitions",
            world.project
        ),
        format!("/v1/runtimes/{}/sessions", harness::fake_family()),
    ];
    for route in routes {
        let anonymous = Call::get(route.clone()).anonymous().send(&world).await;
        assert_eq!(
            anonymous.status, 401,
            "{route} must not answer an unauthenticated caller: {}",
            anonymous.body
        );
        assert_eq!(anonymous.code(), "unauthenticated", "{route}");

        let observer = Call::get(route.clone())
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert!(
            observer.status.is_success(),
            "{route} is a read and must answer an observer: {} {}",
            observer.status,
            observer.body
        );
        assert_eq!(
            observer.realm(),
            world.realm_id(),
            "{route} must name its realm"
        );
    }
}

#[tokio::test]
async fn a_ticket_convergence_command_refuses_an_observer() {
    let world = World::open().await;
    let link_id = with_ticket(&world);
    let body = serde_json::json!({
        "project_id": world.project.to_string(),
        "target": { "kind": "ticket_link", "link_id": link_id.to_string() },
        "expected_revision": 1,
        "desired_state": serde_json::Value::Null,
        "intent": { "schema_version": 1 },
        "payload": { "schema_version": 1 },
    });
    let answer = Call::post("/v1/commands/sync_ticket", &body)
        .signed_as(&world, "observer")
        .with_key("observer-may-not")
        .send(&world)
        .await;
    assert_eq!(answer.status, 403, "{}", answer.body);
    assert_eq!(answer.code(), "forbidden");
}

/// One extra run, so the run list has something to filter.
#[tokio::test]
async fn an_unbound_run_still_appears_in_the_run_list() {
    // A list of runs that only showed *bound* ones would hide every queued run,
    // which is exactly the set an operator is looking for when nothing is happening.
    let world = World::open().await;
    let agent_run_id = AgentRunId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_agent_run(&NewAgentRun {
                id: agent_run_id,
                project_id: world.project,
                team_run_id: world.team_run,
                parent_agent_run_id: None,
                role: RoleSlotId::parse("queued-seat")
                    .expect("a valid slot key")
                    .into_role_key(),
                account_profile_id: None,
                binding: None,
                created_at: at("2026-08-10T09:30:00Z"),
            })
            .expect("an unbound run is persisted");
    });

    let answer = Call::get(format!("/v1/projects/{}/runs", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert!(
        answer.json()["value"]
            .as_array()
            .expect("a list is an array")
            .iter()
            .any(|run| run["agent_run_id"] == serde_json::json!(agent_run_id.to_string())),
        "a queued run with no binding is still a run: {}",
        answer.body
    );
}
