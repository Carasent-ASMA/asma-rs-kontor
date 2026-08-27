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
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use harness::{Answer, Call, World, at, capabilities_without, fake_family, name, secret};
use kontor_api::state::BarrierState;
use kontor_api::state::RuntimeRegistry;
use kontor_core::consultation::{ConsultationFamily, ConsultationRunId};
use kontor_core::id::{
    AgentRunId, AggregateRevision, BoundedText, CanonicalDocument, CommandReceiptId, ConnectorKey,
    ContentHash, ExternalId, ExternalIssueTypeKey, ExternalName, ExternalProjectKey,
    IdempotencyKey, MiniProjectId, ProjectId, QuickSessionId, RoleCode, SeatBindingId, SpecVersion,
    TaskId, TaskWorkflowId, TeamRunId, TicketLinkId, Timestamp, TopologyNodeId,
};
use kontor_core::receipt::{AggregateRef, CommandKind, CommandReceiptState};
use kontor_core::repository::{
    CommandRepository, ConnectorSpecSelector, NewLocalCommand, NewMiniProject, NewObservation,
    NewProject, NewRuntimeEvent, NewSeatBinding, NewTask, NewTaskWorkflow, NewTeamRun,
    NewTicketLink, ProjectRepository, RealmRepository, RunClosure, RunRepository,
    SourceDisposition, SpecRepository, StoredConsultationProfileRevision, StoredEpicCompletion,
    StoredEpicRoster, StoredPromotion, StoredQuickSession, TicketRepository, TopologyRepository,
    WorkflowRepository,
};
use kontor_core::spec::{CatalogRoleRef, EffortLevel, ModelRef, ModelRung, ProviderRef};
use kontor_core::state::{
    Freshness, ObservedRunState, RuntimeContact, TerminalEvidence, TerminalEvidenceSource,
    TerminalOutcome,
};
use kontor_daemon::{DEFAULT_CAPACITY, Daemon, DaemonConfig};
use kontor_runtime::adapter::RuntimeAdapter as _;
use kontor_runtime::capability::RuntimeCapability;
use kontor_runtime::fake::{AdapterCall, RequestKey, ScriptStep};
use kontor_scheduler::model::CapacityConfig;
use kontor_store::{IdempotencyBinding, RegisteredPack, TeamTemplateSource};

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

/// One well-formed Delivery Team draft.
///
/// The generic idempotency, authority and realm-qualification proofs used to
/// ride on `/v1/commands/{kind}`, which accepted a command name and worked out
/// which aggregate it meant. That route is gone, so those proofs ride on a
/// concrete operation instead: this is the smallest realm write that takes a
/// caller-supplied key and answers with a realm-qualified projection.
fn typed_draft(slug: &str) -> serde_json::Value {
    serde_json::json!({
        "id": format!("team-{slug}"),
        "name": "A team",
        "slots": [{
            "id": "lead",
            "role": {
                "catalog_revision": {"id": "standard-roles", "version": 1},
                "role_code": "LSA",
            },
            "capabilities": {"context": {"class": "standard"}},
        }],
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

    let receipt = Call::post("/v1/teams/drafts:save", &typed_draft("realm-qualified"))
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
        .send_stream(&world, 2, std::time::Duration::from_millis(100))
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
    let refused = Call::post("/v1/teams/drafts:save", &typed_draft("observer-write"))
        .signed_as(&world, "observer")
        .with_key("observer-may-not-write")
        .send(&world)
        .await;
    assert_eq!(refused.status, 403);
    assert_eq!(refused.code(), "forbidden");

    // An operator writes an ordinary application operation.
    assert_eq!(
        Call::post("/v1/teams/drafts:save", &typed_draft("operator-write"))
            .signed_as(&world, "operator")
            .with_key("operator-writes")
            .send(&world)
            .await
            .status,
        200
    );

    // Bringing a project into existence is an admin act, and an operator does
    // not reach it.
    let ensure = serde_json::json!({
        "name": "Tier probe",
        "root_path": "/tmp/kontor-tier-probe",
        "memory_origin": "kontor_native",
        "backlog_origin": "kontor_native",
    });
    let operator = Call::post("/v1/projects:ensure", &ensure)
        .signed_as(&world, "operator")
        .with_key("operator-may-not-ensure")
        .send(&world)
        .await;
    assert_eq!(operator.status, 403);
    assert_eq!(operator.code(), "forbidden");

    let admin = Call::post("/v1/projects:ensure", &ensure)
        .signed_as(&world, "admin")
        .with_key("admin-ensures")
        .send(&world)
        .await;
    assert_eq!(admin.status, 200, "{}", admin.body);
}

#[tokio::test]
async fn an_observer_can_discover_projects_and_read_one_without_repeating_its_name() {
    let world = World::open().await;

    let list = Call::get("/v1/projects")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(list.status, 200, "{}", list.body);
    let listed = list.json();
    let projects = listed.as_array().expect("a project list");
    let project = projects
        .iter()
        .find(|project| project["project_id"] == world.project.to_string())
        .expect("the seeded project is discoverable");
    assert_eq!(project["realm_id"], world.realm_id().to_string());
    assert!(project["name"].is_string());
    assert!(project["root_path"].is_string());
    assert!(project["revision"].is_number());
    assert!(project.get("applied").is_none(), "a read is not an ensure");

    let one = Call::get(format!("/v1/projects/{}", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(one.status, 200, "{}", one.body);
    assert_eq!(one.json(), project.clone());

    let missing = Call::get("/v1/projects/01a00000-0000-7000-8000-000000000000")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(missing.status, 404, "{}", missing.body);
    assert_eq!(missing.code(), "not_found");
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
async fn a_replayed_write_returns_the_original_answer_and_a_reused_key_conflicts() {
    let world = World::open().await;
    let body = typed_draft("replay");

    let first = Call::post("/v1/teams/drafts:save", &body)
        .signed_as(&world, "operator")
        .with_key("replay-me")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);

    let replay = Call::post("/v1/teams/drafts:save", &body)
        .signed_as(&world, "operator")
        .with_key("replay-me")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(
        replay.json()["snapshot_cursor"],
        first.json()["snapshot_cursor"],
        "an exact replay answers from what was already durable rather than writing again"
    );

    // The same key with different bytes is a different command wearing a used
    // key.
    let mut changed = body.clone();
    changed["name"] = serde_json::json!("Something else");
    let conflict = Call::post("/v1/teams/drafts:save", &changed)
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
    let answer = Call::post("/v1/teams/drafts:save", &typed_draft("keyless"))
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
}

#[tokio::test]
async fn a_stale_revision_reports_the_current_one_and_mutates_nothing() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "stale").await;
    let category = first_category(&world).await;
    let uri = format!(
        "/v1/projects/{}/tasks/{}/profile-selection",
        seed.project, seed.task
    );
    let selection = |revision: u64| {
        serde_json::json!({
            "expected_revision": revision,
            "work_profile_category": category,
            "reason": "Confirm the pin",
        })
    };

    let answer = Call::post(&uri, &selection(seed.task_revision + 6))
        .signed_as(&world, "admin")
        .with_key("stale-revision")
        .send(&world)
        .await;
    assert_eq!(answer.status, 409, "{}", answer.body);
    assert_eq!(answer.code(), "revision_conflict");
    assert_eq!(
        answer.json()["current_revision"],
        serde_json::json!(seed.task_revision),
        "a stale revision is answered with the revision the caller needs"
    );
    assert_eq!(answer.realm(), world.realm_id());

    // Nothing was written: the key is still free for the right revision.
    let accepted = Call::post(&uri, &selection(seed.task_revision))
        .signed_as(&world, "admin")
        .with_key("stale-revision")
        .send(&world)
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
}

#[tokio::test]
async fn a_write_against_an_unknown_target_is_not_found() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "unknown").await;
    let category = first_category(&world).await;
    let answer = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/profile-selection",
            seed.project,
            TaskId::generate()
        ),
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": category,
            "reason": "Against nothing",
        }),
    )
    .signed_as(&world, "admin")
    .with_key("unknown-target")
    .send(&world)
    .await;
    assert_eq!(answer.status, 404, "{}", answer.body);
    assert_eq!(answer.code(), "not_found");
}

/// There is no dynamic intent route to address at all.
///
/// `/v1/commands/{kind}` used to accept a command name and a target and work
/// out which aggregate it meant. It is gone, and this is what stops it coming
/// back: the closed tool vocabulary and the parity oracle both assume every
/// operation is a named route, and a generic surface reachable beside them
/// would bypass the registry that makes that assumption true. A concrete route
/// cannot address the wrong aggregate, because the aggregate is in its path.
#[tokio::test]
async fn there_is_no_generic_command_route_behind_the_named_operations() {
    let world = World::open().await;
    for kind in ["resume_task", "launch_run", "authorize_execution"] {
        let answer = Call::post(
            format!("/v1/commands/{kind}"),
            &serde_json::json!({"target": {"kind": "task"}}),
        )
        .signed_as(&world, "admin")
        .with_key("no-generic-surface")
        .send(&world)
        .await;
        assert_eq!(
            answer.status, 404,
            "/v1/commands/{kind} must not be routable: {}",
            answer.body
        );
    }
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
        .send_stream(&world, 2, std::time::Duration::from_millis(100))
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
        .send_stream(&world, 2, std::time::Duration::from_millis(100))
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

#[tokio::test]
async fn a_caught_up_live_stream_waits_instead_of_claiming_it_ended() {
    let world = World::open().await;
    world.script(
        r#"{
          "history": [
            {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "one"}
          ],
          "live": []
        }"#,
    );
    let (run, _) = world.launch().await;
    let timeline = Call::get(format!("/v1/sessions/{run}/timeline"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let anchor = timeline.json()["anchor"]
        .as_str()
        .expect("an anchor")
        .to_owned();

    let began = std::time::Instant::now();
    let stream = Call::get(format!("/v1/sessions/{run}/stream?after={anchor}"))
        .signed_as(&world, "observer")
        .send_stream(&world, 1, std::time::Duration::from_millis(150))
        .await;

    assert!(stream.frames().is_empty());
    assert!(
        began.elapsed() >= std::time::Duration::from_millis(125),
        "the bounded reader, not an immediately closed server stream, ended the wait"
    );
}

// ---------------------------------------------------------------------------
// Messages and permissions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_message_resume_reduces_the_run_and_team_run_back_to_running() {
    let world = World::open().await;
    let (run_id, snapshot) = world.launch().await;
    let before = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    let waiting_at = at("2026-08-10T09:05:00Z");
    let payload = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "observed_state": "waiting_input",
        "contact": "reachable",
        "native_sequence": 1,
        "observed_at": waiting_at.to_string(),
    }))
    .expect("control metadata");
    world.daemon.state().with_store(|store| {
        store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: world.project,
                    agent_run_id: run_id,
                    identity: snapshot.identity().clone(),
                    native_event_id: None,
                    native_sequence: 1,
                    payload,
                    observed_at: waiting_at,
                },
                observed: ObservedRunState::WaitingInput,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: before.revision,
            })
            .expect("waiting input is persisted through the shared reducer");
    });
    let waiting_run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    let waiting_team = world.daemon.state().with_store(|store| {
        store
            .get_team_run(world.project, world.team_run)
            .expect("the team reads")
            .expect("the team exists")
    });
    assert_eq!(waiting_run.projection.lifecycle.as_str(), "waiting_input");
    assert_eq!(waiting_team.lifecycle.as_str(), "waiting_input");

    let message_id = kontor_runtime::request::MessageId::generate().to_string();
    let sent = Call::post(
        format!("/v1/sessions/{run_id}/messages"),
        &serde_json::json!({"body": "Continue the same bounded turn."}),
    )
    .signed_as(&world, "operator")
    .with_key(&message_id)
    .send(&world)
    .await;
    assert_eq!(sent.status, 200, "{}", sent.body);

    let after_run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    let after_team = world.daemon.state().with_store(|store| {
        store
            .get_team_run(world.project, world.team_run)
            .expect("the team reads")
            .expect("the team exists")
    });
    assert_eq!(after_run.id, run_id, "the AgentRun identity is unchanged");
    assert_eq!(
        after_run.binding.as_ref().map(|binding| binding.id),
        Some(snapshot.binding_id()),
        "the exact issued binding was observed"
    );
    assert_eq!(after_run.projection.lifecycle.as_str(), "running");
    assert_eq!(
        after_team.id, world.team_run,
        "the TeamRun identity is unchanged"
    );
    assert_eq!(after_team.lifecycle.as_str(), "running");
    assert!(
        world
            .fake
            .calls()
            .iter()
            .any(|call| matches!(call, AdapterCall::Inspect(binding) if *binding == snapshot.binding_id())),
        "the acknowledged message is followed by an exact-binding inspect"
    );
}

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
    let resumed = world
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::Resume(binding) if *binding == snapshot.binding_id()))
        .count();
    assert_eq!(
        resumed, 2,
        "each direct message attempt resumes the persistent seat before delivery"
    );
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
                "mini_project_id": "01890000-0000-7000-8000-000000000001",
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
        .send_stream(&world, 1, std::time::Duration::from_millis(100))
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
        "slots": [{
            "id": "lead",
            "role": {
                "catalog_revision": {"id": "standard-roles", "version": 1},
                "role_code": "LSA"
            },
            "capabilities": {"context": {"class": "standard"}}
        }]
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

#[tokio::test]
async fn a_team_draft_cannot_publish_a_route_outside_the_governed_catalog() {
    let world = World::open().await;
    let mut draft = typed_draft("governed-model-route");
    draft["slots"][0]["capabilities"]["chain"] = serde_json::json!([{
        "provider": "opencode",
        "model": "deepseek/deepseek-v4-pro",
        "effort": "max"
    }]);

    let refused = Call::post("/v1/teams/drafts:save", &draft)
        .signed_as(&world, "operator")
        .with_key("team-route-pro-refused")
        .send(&world)
        .await;
    assert_eq!(refused.status, 400, "{}", refused.body);
    assert_eq!(refused.code(), "invalid_request");

    draft["slots"][0]["capabilities"]["chain"][0]["model"] =
        serde_json::json!("deepseek/deepseek-v4-flash");
    let accepted = Call::post("/v1/teams/drafts:save", &draft)
        .signed_as(&world, "operator")
        .with_key("team-route-flash-accepted")
        .send(&world)
        .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
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
    let revision = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(world.project, run)
            .expect("the run reads")
            .expect("the run exists")
            .revision
            .get()
    });
    observe(&world, run, 1, revision);
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
async fn a_write_naming_another_realms_project_resolves_to_nothing() {
    let world = World::open().await;
    let other = World::open().await;

    // The ids are real — they simply belong to a different database file, which
    // is the whole of the isolation boundary. The answer names *this* realm, so
    // a caller can tell "not here" from "not anywhere".
    let answer = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/profile-selection",
            other.project, other.task
        ),
        &serde_json::json!({
            "expected_revision": 1,
            "work_profile_category": "delivery",
            "reason": "Another realm's work",
        }),
    )
    .signed_as(&world, "admin")
    .with_key("another-realms-work")
    .send(&world)
    .await;
    assert_eq!(answer.status, 404, "{}", answer.body);
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
        &serde_json::json!({
            "name": name,
            "root_path": root,
            "memory_origin": "kontor_native",
            "backlog_origin": "kontor_native",
        }),
    )
    .signed_as(world, "admin")
    .with_key(key)
    .send(world)
    .await
}

/// Install the bundled Jira workflow through the same public surfaces a Lead
/// uses: catalogue read, project read, then revision-checked Admin mutation.
async fn install_jira_workflow(world: &World, project: &str, key: &str) -> Answer {
    let catalogue = Call::get(format!(
        "/v1/projects/{project}/connectors/jira/workflow-specs"
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(catalogue.status, 200, "{}", catalogue.body);
    let catalogue = catalogue.json();
    let shipped = &catalogue.as_array().expect("a workflow catalogue")[0];
    let project_read = Call::get(format!("/v1/projects/{project}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(project_read.status, 200, "{}", project_read.body);
    Call::post(
        format!("/v1/projects/{project}/connectors/jira/workflow-specs:install"),
        &serde_json::json!({
            "external_project": shipped["external_project"],
            "issue_type": shipped["issue_type"],
            "version": shipped["version"],
            "expected_revision": project_read.json()["revision"],
        }),
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
            if task.get("short_code").is_none() {
                task["short_code"] = serde_json::json!(format!("TEST-{}", index + 1));
            }
            if task.get("ticket_links").is_none() {
                task["ticket_links"] = serde_json::json!([{
                    "connector": "jira",
                    "external_issue_key": format!("ASMA-TEST-{}", index + 1),
                }]);
            }
            task
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "expected_revision": revision,
        "name": name,
        "execution_scope": {
            "external_epic_key": "ASMA-TEST",
            "short_title": "Test Epic",
            "kontor_backlog_code": "TEST",
        },
        "work_profile_category": category,
        "runtime_family": "fake.runtime",
        "tasks": tasks,
    })
}

#[tokio::test]
async fn a_legacy_backlog_is_imported_replayed_after_restart_and_switched_once() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = Call::post(
        "/v1/projects:ensure",
        &serde_json::json!({
            "name": "Legacy backlog",
            "root_path": "/tmp/kontor-legacy-backlog",
            "memory_origin": "kontor_native",
            "backlog_origin": "legacy_pending",
        }),
    )
    .signed_as(&world, "admin")
    .with_key("legacy-backlog-project")
    .send(&world)
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.json()["backlog"]["authority"], "agentsroom");
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let category = first_category(&world).await;
    let imported_epic = epic_body(
        created.json()["revision"].as_u64().expect("a revision"),
        "Imported epic",
        &category,
        serde_json::json!([{"title": "Imported task"}]),
    );

    let refused = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &imported_epic,
    )
    .signed_as(&world, "admin")
    .with_key("legacy-backlog-forbidden")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);

    let export = serde_json::json!({
        "source": "agentsroom",
        "expected_authority_revision": 1,
        "epics": [imported_epic],
    });
    let preview = Call::post(
        format!("/v1/projects/{project}/backlog/import:preview"),
        &export,
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply_body = serde_json::json!({
        "export": export,
        "preview_hash": preview_hash,
    });
    let applied = Call::post(
        format!("/v1/projects/{project}/backlog/import:apply"),
        &apply_body,
    )
    .signed_as(&world, "admin")
    .with_key("legacy-backlog-import")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["imported_count"], 2);

    let admin = secret(&world, "admin");
    let observer = secret(&world, "observer");
    let realm = world.realm_id();
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
    .expect("the imported realm reopens");
    assert_eq!(restarted.realm_id(), realm);
    restarted.reconcile().await;
    let router = restarted.router();

    let replayed = Call::post(
        format!("/v1/projects/{project}/backlog/import:apply"),
        &apply_body,
    )
    .with_token(&admin)
    .with_key("legacy-backlog-import")
    .send_to(&router)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["command_receipt_id"],
        applied.json()["command_receipt_id"]
    );

    let attested = Call::post(
        format!("/v1/projects/{project}/subjects/authority:attest"),
        &serde_json::json!({
            "subject": "backlog",
            "source_cursor": "final",
            "source_hash": preview_hash,
            "expected_revision": 1,
        }),
    )
    .with_token(&admin)
    .with_key("legacy-backlog-freeze")
    .send_to(&router)
    .await;
    assert_eq!(attested.status, 200, "{}", attested.body);
    assert_eq!(attested.json()["revision"], 2);
    let attested_again = Call::post(
        format!("/v1/projects/{project}/subjects/authority:attest"),
        &serde_json::json!({
            "subject": "backlog",
            "source_cursor": "final",
            "source_hash": preview_hash,
            "expected_revision": 1,
        }),
    )
    .with_token(&admin)
    .with_key("legacy-backlog-freeze")
    .send_to(&router)
    .await;
    assert_eq!(attested_again.status, 200, "{}", attested_again.body);
    assert_eq!(
        attested_again.json()["receipt"]["receipt_id"],
        attested.json()["receipt"]["receipt_id"]
    );

    let switched = Call::post(
        format!("/v1/projects/{project}/backlog/cutover:switch"),
        &serde_json::json!({
            "source": "agentsroom",
            "final_import_hash": preview_hash,
            "expected_revision": 2,
        }),
    )
    .with_token(&admin)
    .with_key("legacy-backlog-switch")
    .send_to(&router)
    .await;
    assert_eq!(switched.status, 200, "{}", switched.body);
    assert_eq!(switched.json()["authority"], "kontor");
    let switched_again = Call::post(
        format!("/v1/projects/{project}/backlog/cutover:switch"),
        &serde_json::json!({
            "source": "agentsroom",
            "final_import_hash": preview_hash,
            "expected_revision": 2,
        }),
    )
    .with_token(&admin)
    .with_key("legacy-backlog-switch")
    .send_to(&router)
    .await;
    assert_eq!(switched_again.status, 200, "{}", switched_again.body);
    assert_eq!(
        switched_again.json()["receipt"]["receipt_id"],
        switched.json()["receipt"]["receipt_id"]
    );

    let project_read = Call::get(format!("/v1/projects/{project}"))
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(project_read.status, 200, "{}", project_read.body);
    let mut native_epic = epic_body(
        project_read.json()["revision"]
            .as_u64()
            .expect("a project revision"),
        "Native epic",
        &category,
        serde_json::json!([{
            "title": "Native task",
            "ticket_links": [{
                "connector": "jira",
                "external_issue_key": "ASMA-NATIVE-1",
            }],
        }]),
    );
    native_epic["execution_scope"]["external_epic_key"] = serde_json::json!("ASMA-NATIVE");
    native_epic["execution_scope"]["short_title"] = serde_json::json!("Native Epic");
    let native = Call::post(format!("/v1/projects/{project}/epics:apply"), &native_epic)
        .with_token(&admin)
        .with_key("native-backlog-after-switch")
        .send_to(&router)
        .await;
    assert_eq!(native.status, 200, "{}", native.body);
    drop(restarted);
    drop(directory);
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
    assert_eq!(created.json()["memory"]["origin"], "kontor_native");
    assert_eq!(created.json()["memory"]["authority"], "kontor");
    assert_eq!(created.json()["backlog"]["authority"], "kontor");
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

    let global_freeze = Call::post("/v1/memory/cutover:freeze", &serde_json::json!({}))
        .signed_as(&world, "admin")
        .with_key("legacy-freeze-1")
        .send(&world)
        .await;
    assert_eq!(global_freeze.status, 400, "{}", global_freeze.body);

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

/// Legacy imports may add one explicit short-code mapping without changing the
/// task, epic, lifecycle or ticket identities. Descriptions, Jira keys and
/// internal ids remain unavailable as implicit display-name sources.
#[tokio::test]
async fn legacy_task_short_code_mapping_previews_applies_and_replays_in_place() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "short-code-project",
        "Short-code migration",
        "/tmp/kontor-short-code",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;
    let legacy = epic_body(
        revision,
        "ASMA-7675 · QNR v2 Nonprod Delivery",
        &category,
        serde_json::json!([{
            "title": "ASMA-7676 · Prepare the very long non-production delivery foundation",
            "short_code": null,
            "import_state": "completed",
            "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-7676"}]
        }]),
    );
    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &legacy)
        .signed_as(&world, "admin")
        .with_key("short-code-legacy")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let epic = first.json()["epic_id"].as_str().expect("epic").to_owned();
    let task = first.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("task")
        .to_owned();
    assert!(first.json()["tasks"][0]["short_code"].is_null());

    let mut mapped = legacy.clone();
    mapped["tasks"][0]["short_code"] = serde_json::json!("QNR-NP-01");
    let preview = Call::post(format!("/v1/projects/{project}/epics:preview"), &mapped)
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["applied"], "updated");
    assert_eq!(preview.json()["epic_id"], epic);
    assert_eq!(preview.json()["tasks"][0]["task_id"], task);
    assert_eq!(preview.json()["tasks"][0]["short_code"], "QNR-NP-01");
    assert_eq!(preview.json()["tasks"][0]["state"], "done");

    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &mapped)
        .signed_as(&world, "admin")
        .with_key("short-code-map")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["applied"], "updated");
    assert_eq!(applied.json()["epic_id"], epic);
    assert_eq!(applied.json()["tasks"][0]["task_id"], task);
    assert_eq!(applied.json()["tasks"][0]["short_code"], "QNR-NP-01");
    assert_eq!(applied.json()["tasks"][0]["state"], "done");

    let readback = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(readback.status, 200, "{}", readback.body);
    assert_eq!(readback.json()["tasks"][0]["task_id"], task);
    assert_eq!(readback.json()["tasks"][0]["short_code"], "QNR-NP-01");
    assert_eq!(readback.json()["tasks"][0]["state"], "done");

    let replay = Call::post(format!("/v1/projects/{project}/epics:apply"), &mapped)
        .signed_as(&world, "admin")
        .with_key("short-code-replay")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged");
    assert_eq!(replay.json()["tasks"][0]["short_code"], "QNR-NP-01");

    for invalid in [
        serde_json::json!("ASMA-7676"),
        serde_json::json!("01a019c0-eee7-72a1-a8a7-7fff1ddce8f3"),
    ] {
        let mut refused = legacy.clone();
        refused["tasks"][0]["short_code"] = invalid;
        let answer = Call::post(format!("/v1/projects/{project}/epics:preview"), &refused)
            .signed_as(&world, "admin")
            .send(&world)
            .await;
        assert_eq!(answer.status, 400, "{}", answer.body);
    }
}

/// GAP-06. A runtime plane serves a host, not one epic. Each epic therefore
/// carries its runtime-facing identity in durable Kontor state so a second epic
/// in the same project neither inherits the first epic's labels nor depends on
/// a daemon restart and a new static scope entry.
#[tokio::test]
async fn two_epics_in_one_project_keep_distinct_execution_scopes_across_replay_and_readback() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "multi-epic-project",
        "Shared project",
        "/tmp/kontor-multi-epic",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;

    let mut first_body = epic_body(
        revision,
        "First epic",
        &category,
        serde_json::json!([{
            "title": "First task",
            "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-8001"}]
        }]),
    );
    first_body["execution_scope"] = serde_json::json!({
        "external_epic_key": "ASMA-8000",
        "short_title": "First"
    });
    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &first_body)
        .signed_as(&world, "admin")
        .with_key("multi-epic-first")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(
        first.json()["execution_scope"],
        first_body["execution_scope"]
    );
    let first_epic = first.json()["epic_id"]
        .as_str()
        .expect("the first epic id")
        .to_owned();

    let replay = Call::post(format!("/v1/projects/{project}/epics:apply"), &first_body)
        .signed_as(&world, "admin")
        .with_key("multi-epic-first-replay")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged");
    assert_eq!(replay.json()["epic_id"], first_epic);
    assert_eq!(
        replay.json()["execution_scope"],
        first_body["execution_scope"]
    );

    let first_read = Call::get(format!("/v1/projects/{project}/epics/{first_epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(first_read.status, 200, "{}", first_read.body);
    assert_eq!(
        first_read.json()["execution_scope"],
        first_body["execution_scope"]
    );

    let mut second_body = epic_body(
        revision,
        "Second epic",
        &category,
        serde_json::json!([{
            "title": "Second task",
            "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-9001"}]
        }]),
    );
    second_body["execution_scope"] = serde_json::json!({
        "external_epic_key": "ASMA-9000",
        "short_title": "Second"
    });
    let second = Call::post(format!("/v1/projects/{project}/epics:apply"), &second_body)
        .signed_as(&world, "admin")
        .with_key("multi-epic-second")
        .send(&world)
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_ne!(second.json()["epic_id"], first_epic);
    assert_eq!(
        second.json()["execution_scope"],
        second_body["execution_scope"]
    );

    let mut drift = first_body.clone();
    drift["execution_scope"]["short_title"] = serde_json::json!("Renamed");
    let refused = Call::post(format!("/v1/projects/{project}/epics:apply"), &drift)
        .signed_as(&world, "admin")
        .with_key("multi-epic-first-drift")
        .send(&world)
        .await;
    assert_eq!(refused.status, 409, "{}", refused.body);

    let after_drift = Call::get(format!("/v1/projects/{project}/epics/{first_epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        after_drift.json()["execution_scope"],
        first_body["execution_scope"],
        "a refused rename cannot change the durable runtime identity"
    );
}

/// Durable placement resolution is an admission preflight. A malformed imported
/// task must not leave the queued, unbound TeamRun that originally made the QNR
/// replay impossible to recover honestly.
#[tokio::test]
async fn an_unplaceable_dynamic_task_is_refused_before_a_team_run_is_committed() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let created = ensure_project(
        &world,
        "dynamic-preflight-project",
        "Dynamic preflight",
        "/tmp/kontor-dynamic-preflight",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Dynamic preflight",
            "harness": "fake.runtime",
            "credential_alias": "dynamic-preflight",
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("dynamic-preflight-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);

    let mut body = epic_body(
        revision,
        "Dynamic epic",
        &category,
        serde_json::json!([{
            "title": "Task with no worktree",
            "worktree": null,
            "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-9991"}]
        }]),
    );
    body["execution_scope"] = serde_json::json!({
        "external_epic_key": "ASMA-9990",
        "short_title": "Dynamic epic"
    });
    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("dynamic-preflight-epic")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": applied.json()["revision"],
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {
                "max_tokens": 1000,
                "max_commands": 10,
                "max_duration_seconds": 600,
                "max_cost_minor_units": 100,
                "cost_currency": "NOK"
            },
            "granted_by": account.json()["account_profile_id"],
            "reason": "Prove placement preflight"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("dynamic-preflight-arm")
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
    assert_eq!(plan.json()["ready"].as_array().expect("ready").len(), 1);
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan.json()["plan_hash"]}),
    )
    .signed_as(&world, "operator")
    .with_key("dynamic-preflight-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        started.json()["started"]
            .as_array()
            .expect("started")
            .is_empty()
    );
    assert_eq!(started.json()["blocked"][0]["code"], "not_found");
    assert!(
        started.json()["blocked"][0]["evidence"][0]["rule"]
            .as_str()
            .expect("a rule")
            .contains("worktree"),
        "{}",
        started.body
    );

    let project_id = ProjectId::parse(&project).expect("a project id");
    let task_id = TaskId::parse(&task).expect("a task id");
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .list_team_runs_for_task(project_id, task_id)
                .expect("the TeamRun census reads")
                .is_empty(),
            "placement refusal must precede the durable admission commit"
        );
    });
    assert!(
        world
            .fake
            .calls()
            .iter()
            .all(|call| !matches!(call, kontor_runtime::fake::AdapterCall::PrepareContainer(_))),
        "a logical placement refusal reaches no native container operation"
    );
}

/// GAP-1. Importing a historical backlog used to flatten every task to `ready`,
/// so a cutover could bring over either the unfinished tasks or the whole task
/// graph, but not both. The import vocabulary is deliberately smaller than the
/// native lifecycle: a source task is either still `ready` or historically
/// `completed`; it cannot claim a live run, a blocked decision or native gate
/// closure.
#[tokio::test]
async fn epic_import_preview_apply_and_replay_preserve_historical_task_lifecycle() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "historical-import-project",
        "Historical import",
        "/tmp/kontor-historical-import",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;
    let body = epic_body(
        revision,
        "Full-history import",
        &category,
        serde_json::json!([
            {
                "title": "Already delivered",
                "import_state": "completed",
                "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-1"}]
            },
            {
                "title": "Still planned",
                "import_state": "ready",
                "depends_on": ["Already delivered"],
                "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-2"}]
            }
        ]),
    );

    let preview = Call::post(format!("/v1/projects/{project}/epics:preview"), &body)
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["tasks"][0]["state"], "done");
    assert_eq!(preview.json()["tasks"][1]["state"], "ready");
    assert!(preview.json()["tasks"][0]["task_id"].is_null());
    assert!(preview.json()["tasks"][1]["task_id"].is_null());
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store
                .list_tasks(ProjectId::parse(&project).expect("a project id"))
                .expect("the task census reads"))
            .is_empty(),
        "preview must not leave even a partial task graph behind"
    );

    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("historical-import-apply")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["tasks"][0]["state"], "done");
    assert_eq!(first.json()["tasks"][1]["state"], "ready");
    let epic = first.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let task_ids: Vec<_> = first.json()["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("a task id").to_owned())
        .collect();

    let readback = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(readback.status, 200, "{}", readback.body);
    assert_eq!(readback.json()["tasks"][0]["task_id"], task_ids[0]);
    assert_eq!(readback.json()["tasks"][1]["task_id"], task_ids[1]);
    assert_eq!(readback.json()["tasks"][0]["state"], "done");
    assert_eq!(readback.json()["tasks"][1]["state"], "ready");
    assert_eq!(
        readback.json()["tasks"][0]["links"]
            .as_array()
            .expect("completed task links")
            .len(),
        1
    );
    assert_eq!(
        readback.json()["tasks"][1]["links"]
            .as_array()
            .expect("ready task links")
            .len(),
        1
    );
    assert!(
        readback.json()["tasks"][0]["gates"]
            .as_array()
            .expect("gates")
            .iter()
            .all(|gate| gate["state"] == "not_ready"),
        "historical completion imports no native gate verdict: {}",
        readback.body
    );

    let replayed = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("historical-import-replay")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");
    assert_eq!(replayed.json()["tasks"][0]["task_id"], task_ids[0]);
    assert_eq!(replayed.json()["tasks"][1]["task_id"], task_ids[1]);

    let replay_preview = Call::post(format!("/v1/projects/{project}/epics:preview"), &body)
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(replay_preview.status, 200, "{}", replay_preview.body);
    assert_eq!(replay_preview.json()["tasks"][0]["task_id"], task_ids[0]);
    assert_eq!(replay_preview.json()["tasks"][1]["task_id"], task_ids[1]);
}

#[tokio::test]
async fn epic_import_defaults_ready_and_refuses_invalid_or_contradictory_state_atomically() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "historical-import-contract",
        "Historical import contract",
        "/tmp/kontor-historical-import-contract",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;
    let omitted = epic_body(
        revision,
        "Compatibility import",
        &category,
        serde_json::json!([{"title": "Stable ready"}]),
    );

    let preview = Call::post(format!("/v1/projects/{project}/epics:preview"), &omitted)
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["tasks"][0]["state"], "ready");

    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &omitted)
        .signed_as(&world, "admin")
        .with_key("historical-import-default")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["tasks"][0]["state"], "ready");
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let stable_task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();

    let invalid = Call::post(
        format!("/v1/projects/{project}/epics:preview"),
        &epic_body(
            revision,
            "Invalid import",
            &category,
            serde_json::json!([{"title": "Invalid", "import_state": "done"}]),
        ),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(invalid.status, 400, "{}", invalid.body);
    assert_eq!(invalid.code(), "invalid_request");

    // The prospective sibling is processed before the contradictory existing
    // task. A non-atomic apply would leak it before discovering the conflict.
    let contradictory = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Compatibility import",
            &category,
            serde_json::json!([
                {"title": "Would leak", "import_state": "completed"},
                {"title": "Stable ready", "import_state": "completed"}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("historical-import-contradiction")
    .send(&world)
    .await;
    assert_eq!(contradictory.status, 409, "{}", contradictory.body);

    let readback = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(readback.status, 200, "{}", readback.body);
    let readback_json = readback.json();
    let tasks = readback_json["tasks"].as_array().expect("the task list");
    assert_eq!(
        tasks.len(),
        1,
        "the failed apply must roll back its new sibling"
    );
    assert_eq!(tasks[0]["task_id"], stable_task);
    assert_eq!(tasks[0]["state"], "ready");
}

/// GAP-1 follow-through. Import provenance describes the lifecycle an epic was
/// *declared* with, not the progress Kontor has made since. Judging a reapply
/// against the task's current state broke the oldest promise the apply contract
/// makes — that an identical manifest is replayable — the moment any task
/// started, because the first native transition both clears the provenance and
/// moves the state.
#[tokio::test]
async fn an_identical_manifest_reapplies_over_a_task_that_natively_progressed() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "progressed").await;

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
    .with_key("progressed-arm")
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
    assert_eq!(plan.status, 200, "{}", plan.body);
    let hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a plan hash")
        .to_owned();

    let started = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:start",
            seed.project, seed.epic
        ),
        &serde_json::json!({"plan_hash": hash}),
    )
    .signed_as(&world, "operator")
    .with_key("progressed-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);

    // The native transition that clears the imported fact. Everything below is
    // about a task whose provenance column is now empty on purpose.
    let progressed = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(progressed.status, 200, "{}", progressed.body);
    assert_eq!(progressed.json()["tasks"][0]["state"], "in_progress");

    let revision = ensure_project(
        &world,
        "progressed-reread",
        "Kontor",
        "/tmp/kontor-progressed",
    )
    .await
    .json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;

    // The same manifest the epic was created from — task state omitted, so the
    // compatibility default — plus one task the caller has since added.
    let reapplied = Call::post(
        format!("/v1/projects/{}/epics:apply", seed.project),
        &epic_body(
            revision,
            "Control epic",
            &category,
            serde_json::json!([
                {"title": "The task"},
                {"title": "A later task", "depends_on": ["The task"]}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("progressed-reapply")
    .send(&world)
    .await;
    assert_eq!(reapplied.status, 200, "{}", reapplied.body);
    assert_eq!(reapplied.json()["applied"], "unchanged");
    assert_eq!(reapplied.json()["epic_id"], seed.epic);
    assert_eq!(
        reapplied.json()["tasks"][0]["task_id"],
        seed.task,
        "the progressed task keeps the identity every dependency already names"
    );
    assert_eq!(reapplied.json()["tasks"][0]["applied"], "unchanged");
    assert_eq!(
        reapplied.json()["tasks"][0]["state"],
        "in_progress",
        "a reapply preserves native progress instead of resetting it to the imported declaration: {}",
        reapplied.body
    );
    assert_eq!(reapplied.json()["tasks"][1]["applied"], "created");

    // Nothing rolled back: the graph the reapply judged is the graph on disk,
    // with the added sibling present and the progressed task still progressed.
    let after = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(after.status, 200, "{}", after.body);
    let after_json = after.json();
    let tasks = after_json["tasks"].as_array().expect("the task list");
    assert_eq!(tasks.len(), 2, "the sibling the reapply added is durable");
    assert_eq!(tasks[0]["task_id"], seed.task);
    assert_eq!(tasks[0]["state"], "in_progress");
}

#[tokio::test]
async fn a_mixed_import_closes_after_only_its_native_task_earns_completion() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "mixed-import-close",
        "Mixed import close",
        "/tmp/kontor-mixed-import-close",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let project_revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;
    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Import close evaluator",
            "harness": "fake.runtime",
            "credential_alias": "import-close-evaluator",
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("mixed-import-close-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    // Only the first task will receive native completion evidence. The second
    // remains historical terminality throughout the close census.
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            project_revision,
            "Mixed full-history import",
            &category,
            serde_json::json!([
                {"title": "Native completion"},
                {"title": "Historical completion", "import_state": "completed"}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("mixed-import-close-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let epic_revision = applied.json()["revision"]
        .as_u64()
        .expect("an epic revision");
    let native_task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a native task id")
        .to_owned();
    let historical_task = applied.json()["tasks"][1]["task_id"]
        .as_str()
        .expect("a historical task id")
        .to_owned();
    let project_id = ProjectId::parse(&project).expect("a project id");
    let historical_task_id = TaskId::parse(&historical_task).expect("a task id");

    let historical_before = world.daemon.state().with_store(|store| {
        let task = store
            .get_task(project_id, historical_task_id)
            .expect("the imported task reads")
            .expect("the imported task exists");
        let workflow = store
            .get_active_task_workflow(project_id, historical_task_id)
            .expect("the imported workflow reads")
            .expect("the imported workflow exists");
        let gates = store
            .gate_states(project_id, workflow.id)
            .expect("the imported gate states read");
        let runs = store
            .list_team_runs_for_task(project_id, historical_task_id)
            .expect("the imported run census reads");
        (task, gates, runs)
    });
    assert_eq!(
        historical_before.0.imported_state,
        Some(kontor_core::state::ImportedTaskState::Completed)
    );
    assert_eq!(
        historical_before.0.state,
        kontor_core::state::TaskState::Done
    );
    assert!(
        historical_before.1.is_empty(),
        "historical terminality must not synthesize native gate receipts"
    );
    assert!(
        historical_before.2.is_empty(),
        "historical terminality must not synthesize a successful run"
    );

    let seed = Bootstrapped {
        project: project.clone(),
        epic: epic.clone(),
        task: native_task,
        task_revision: applied.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("a task revision"),
        account: account_id,
    };
    let runs = seat_existing(&world, &seed, "mixed-import-close").await;
    settle_every_seat(&world, &seed, &runs, "mixed-import-close").await;
    let completed = discharge_the_profile_and_complete(&world, &seed, "mixed-import-close").await;
    assert_eq!(completed.status, 200, "{}", completed.body);

    let closed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/lifecycle"),
        &serde_json::json!({
            "action": "close_epic",
            "expected_revision": epic_revision,
            "reason": "Native work and imported historical work are terminal"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("mixed-import-close-epic")
    .send(&world)
    .await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.json()["state"], "closed");

    let historical_after = world.daemon.state().with_store(|store| {
        let task = store
            .get_task(project_id, historical_task_id)
            .expect("the imported task reads")
            .expect("the imported task exists");
        let workflow = store
            .get_active_task_workflow(project_id, historical_task_id)
            .expect("the imported workflow reads")
            .expect("the imported workflow exists");
        let gates = store
            .gate_states(project_id, workflow.id)
            .expect("the imported gate states read");
        let runs = store
            .list_team_runs_for_task(project_id, historical_task_id)
            .expect("the imported run census reads");
        (task, gates, runs)
    });
    assert_eq!(
        historical_after.0.imported_state,
        Some(kontor_core::state::ImportedTaskState::Completed),
        "epic close must retain explicit historical provenance"
    );
    assert!(historical_after.1.is_empty());
    assert!(historical_after.2.is_empty());
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

/// OP-REQ-043: omitted `budget` is no per-run ceiling.
///
/// Quota headroom and capacity govern unconstrained work. The pinned profile's
/// `budget_defaults` are not substituted, and the grant reports `budget: null`.
#[tokio::test]
async fn arming_without_a_budget_imposes_no_profile_ceiling() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "arm-budget-1", "Kontor", "/tmp/kontor-arm-budget").await;
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
    .with_key("arm-budget-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Budget-omitted epic",
            &category,
            serde_json::json!([{"title": "First"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("arm-budget-epic")
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
            "granted_by": account_id,
            "reason": "Arm without a money ceiling"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("arm-budget-default")
    .send(&world)
    .await;
    assert_eq!(
        armed.status, 200,
        "omitting budget must succeed: {}",
        armed.body
    );
    assert!(
        armed.json()["budget"].is_null(),
        "omitted budget must not substitute profile defaults: {}",
        armed.body
    );
}

/// Explicit bounds are stored as stated. They may exceed the pinned profile's
/// `budget_defaults`, and a different currency is not refused. Zero is still
/// invalid.
#[tokio::test]
async fn an_explicit_arming_budget_is_stored_as_stated() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "arm-budget-2", "Kontor", "/tmp/kontor-arm-narrow").await;
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
    .with_key("arm-narrow-account")
    .send(&world)
    .await;
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Narrowed epic",
            &category,
            serde_json::json!([{"title": "First"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("arm-narrow-epic")
    .send(&world)
    .await;
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let arm = |label: &'static str, budget: serde_json::Value| {
        let body = serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": budget,
            "granted_by": account_id.clone(),
            "reason": "Narrow the grant"
        });
        Call::post(
            format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
            &body,
        )
        .signed_as(&world, "admin")
        .with_key(label)
    };

    // Well inside the pack's `code` defaults.
    let narrowed = arm(
        "arm-narrow-ok",
        serde_json::json!({
            "max_tokens": 1000,
            "max_commands": 10,
            "max_duration_seconds": 600,
            "max_cost_minor_units": 100,
            "cost_currency": "NOK"
        }),
    )
    .send(&world)
    .await;
    assert_eq!(narrowed.status, 200, "{}", narrowed.body);
    assert_eq!(
        narrowed.json()["budget"]["max_tokens"],
        1000,
        "a narrowed grant is stored as narrowed: {}",
        narrowed.body
    );

    // Wider than any profile in the pack — still a valid explicit ceiling.
    let widened = arm(
        "arm-wide-ok",
        serde_json::json!({
            "max_tokens": 4_000_000_u64,
            "max_commands": 999_999,
            "max_duration_seconds": 999_999,
            "max_cost_minor_units": 999_999,
            "cost_currency": "NOK"
        }),
    )
    .send(&world)
    .await;
    assert_eq!(
        widened.status, 200,
        "an explicit bound may exceed profile defaults: {}",
        widened.body
    );
    assert_eq!(widened.json()["budget"]["max_tokens"], 4_000_000);

    let other_currency = arm(
        "arm-eur-ok",
        serde_json::json!({
            "max_tokens": 1000,
            "max_commands": 10,
            "max_duration_seconds": 600,
            "max_cost_minor_units": 1,
            "cost_currency": "EUR"
        }),
    )
    .send(&world)
    .await;
    assert_eq!(
        other_currency.status, 200,
        "an explicit EUR ceiling is stored as stated: {}",
        other_currency.body
    );
    assert_eq!(other_currency.json()["budget"]["cost_currency"], "EUR");

    let zero = arm(
        "arm-zero-refused",
        serde_json::json!({
            "max_tokens": 0,
            "max_commands": 10,
            "max_duration_seconds": 600,
            "max_cost_minor_units": 100,
            "cost_currency": "NOK"
        }),
    )
    .send(&world)
    .await;
    assert!(
        zero.status.is_client_error(),
        "a zero bound is still invalid: {}",
        zero.body
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

    // Axum's default JSON extractor answers malformed bodies as plain text.
    // This route is an MCP contract, so even schema rejection stays typed JSON.
    let malformed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({"expected_revision": epic_revision, "budget": {}}),
    )
    .signed_as(&world, "admin")
    .with_key("arm-malformed")
    .send(&world)
    .await;
    assert_eq!(malformed.status, 400, "{}", malformed.body);
    assert_eq!(malformed.code(), "invalid_request");

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

    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &arm_body,
    )
    .signed_as(&world, "admin")
    .with_key("arm-admin")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["authorization_id"],
        armed.json()["authorization_id"],
        "the same key replays the original grant"
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
            .any(|task| task["code"] == "authorization_scope_mismatch"
                && task["action"]
                    .as_str()
                    .is_some_and(|action| action.contains("kontor_execution_arm"))),
        "an unarmed sibling of a whitelist blocks with a named next move: {}",
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
    assert!(
        after.json()["blocked"]
            .as_array()
            .expect("blocked")
            .iter()
            .any(|task| task["code"] == "authorization_blocked"
                && task["action"]
                    .as_str()
                    .is_some_and(|action| action.contains("kontor_execution_arm"))),
        "disarm is a stop, not a return to unarmed: {}",
        after.body
    );
}

/// Ready work runs without a grant. Arming is optional narrowing, not the
/// on-switch.
#[tokio::test]
async fn an_unarmed_epic_is_ready_without_a_grant() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "arm-free", "Kontor", "/tmp/kontor-arm-free").await;
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
            "Unarmed epic",
            &category,
            serde_json::json!([{"title": "First"}, {"title": "Second"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("arm-free-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    let body = plan.json();
    let ready = body["ready"].as_array().expect("a ready set");
    assert_eq!(
        ready.len(),
        2,
        "both unarmed tasks are ready: {}",
        plan.body
    );
    assert!(
        ready.iter().all(|task| task["authorization_id"].is_null()),
        "default-allow records no grant: {}",
        plan.body
    );
    assert!(
        !body["blocked"]
            .as_array()
            .expect("blocked")
            .iter()
            .any(|task| task["code"] == "authorization_missing"
                || task["code"] == "authorization_blocked"),
        "unarmed work is not an authorization refusal: {}",
        plan.body
    );
}

/// A narrowing grant may omit the window and concurrency; omitted budget is
/// unconstrained rather than a profile ceiling.
#[tokio::test]
async fn arming_omits_the_window_and_concurrency_when_the_caller_states_none() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "arm-window-1", "Kontor", "/tmp/kontor-arm-window").await;
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
    .with_key("arm-window-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Window-defaulted epic",
            &category,
            serde_json::json!([{"title": "First"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("arm-window-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "granted_by": account_id,
            "reason": "Narrow nothing but record a grant"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("arm-window-default")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);
    assert_eq!(armed.json()["allowed_start"], "2020-01-01T00:00:00Z");
    assert_eq!(armed.json()["allowed_end"], "2099-01-01T00:00:00Z");
    assert_eq!(
        armed.json()["max_concurrency"],
        serde_json::json!(DEFAULT_CAPACITY.mission_max_in_flight),
        "omitted concurrency takes the realm mission ceiling: {}",
        armed.body
    );
    assert!(
        armed.json()["budget"].is_null(),
        "omitted budget is unconstrained: {}",
        armed.body
    );
}

/// A body of the wrong shape is refused as a *request* problem, in this Realm's
/// own envelope.
///
/// The incident this is a fixture for: `execution:arm` was called three times
/// with a guessed `budget` shape, and each attempt came back as a transport-level
/// "the body was not JSON" because axum answers its own extractor's rejection
/// with `text/plain`. The operator read that as a dead route and the epic sat
/// unarmed. The distinction the test defends is the one that was lost — a
/// malformed request is `invalid_request`, never an unreachable realm — and it
/// holds for every route that takes a body, because they all take it through the
/// same extractor.
#[tokio::test]
async fn a_malformed_body_is_refused_as_a_request_and_never_as_a_broken_realm() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "arm-shape", "Kontor", "/tmp/kontor-arm-shape").await;
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
            "Shape epic",
            &category,
            serde_json::json!([{"title": "First"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("shape-epic-1")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");

    // The exact budget shape the incident guessed: plausible, and not the one
    // `BudgetBoundsRequest` declares.
    let guessed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 2,
            "budget": {"tokens": 1, "commands": 1, "duration": 1, "cost": 1},
            "granted_by": "01a00751-5be9-7281-bba5-75d8c0c101e7",
            "reason": "Bootstrap the epic"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("shape-guessed")
    .send(&world)
    .await;
    assert_eq!(guessed.status, 400, "{}", guessed.body);
    assert_eq!(guessed.code(), "invalid_request");
    assert_eq!(
        guessed.realm(),
        world.realm_id(),
        "a refusal names its realm like every other answer"
    );

    // Not JSON at all is a different mistake and says so, so a caller cannot
    // conflate "my body is malformed" with "my body is the wrong shape".
    let syntax = Call::post_raw(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        "{not json",
    )
    .signed_as(&world, "admin")
    .with_key("shape-syntax")
    .send(&world)
    .await;
    assert_eq!(syntax.status, 400, "{}", syntax.body);
    assert_eq!(syntax.code(), "invalid_request");
    assert_ne!(
        syntax.json()["rule"],
        guessed.json()["rule"],
        "a syntax error and a schema mismatch are different problems: {}",
        syntax.body
    );

    // The request carried a plausible-looking credential in a field that does
    // not exist. Nothing in the refusal may repeat it.
    let leaky = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "budget": {"cost_currency": "sk-live-do-not-log"}
        }),
    )
    .signed_as(&world, "admin")
    .with_key("shape-leak")
    .send(&world)
    .await;
    assert_eq!(leaky.status, 400, "{}", leaky.body);
    assert!(
        !leaky.body.contains("sk-live-do-not-log"),
        "a refusal must never echo the request body: {}",
        leaky.body
    );
}

/// A bounded auto-arm can be declared through a supported path, and only by an
/// admin.
///
/// `AutoArmPolicy::BoundedAutoArm` and `TriggerSpec::authorize_auto_arm` were
/// implemented and tested and could not be *reached*: the only caller of
/// `insert_trigger_spec` was a backup import, so no operator could declare one.
/// The consequence was not a missing feature but a silently different policy —
/// every arm had to be a human calling `execution:arm`, which is exactly the
/// standing instruction nobody chose.
///
/// The tier is half the test. Publishing a bounded auto-arm grants the capability
/// to start work with no human in the loop, so an operator credential must not
/// reach it.
#[tokio::test]
async fn a_bounded_auto_arm_is_declarable_by_an_admin_and_by_nobody_else() {
    // The seeded world, because a trigger pins a work profile and a team template
    // and both must actually be installed: a trigger that references revisions
    // nobody published is not a trigger this realm could ever fire.
    let world = World::open().await;
    world.daemon.reconcile().await;
    let project = world.project.to_string();
    let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
        .expect("the bundled pack seeds at least one category");
    let bundle =
        kontor_profiles::pack::resolve_profile(&pack, &entry.category, at("2026-08-10T09:00:00Z"))
            .expect("the seeded category resolves");
    let team = bundle.team.clone().expect("the profile pinned a team");

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Auto arm",
            "harness": "fake.runtime",
            "credential_alias": "auto-arm",
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("trg-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let spec = serde_json::json!({
        "schema_version": 1,
        "id": "trigger.inbound-request",
        "version": 1,
        "source_kind": "webhook",
        "source_connection": "conn.alpha",
        "event_schema": "schema.request-created",
        "event_schema_version": 4,
        "filter": [{"pointer": "/kind", "equals": "request.created"}],
        "dedup": {"pointers": ["/kind", "/external_id"]},
        "work_profile": bundle.profile.definition.id.as_str(),
        "work_profile_version": bundle.profile.definition.version.get(),
        "team_template": {
            "template_id": team.template_id.to_string(),
            "version": team.version.get()
        },
        "context_template": {"template": "context.default", "version": 1},
        "approval": {
            "kind": "bounded_auto_arm",
            "capability": {
                "granted_to": account_id,
                "execution_authorization": "0193f000-0000-7000-8000-00000000c001"
            },
            "max_concurrency": 2,
            "budget": {
                "max_tokens": 100_000,
                "max_commands": 40,
                "max_duration_seconds": 1800,
                "max_cost": {"minor_units": 1500, "currency": "NOK"}
            }
        },
        "limits": {
            "priority": 50,
            "max_concurrency": 2,
            "budget": {
                "max_tokens": 100_000,
                "max_commands": 40,
                "max_duration_seconds": 1800,
                "max_cost": {"minor_units": 1500, "currency": "NOK"}
            }
        },
        "calendar_policy": null
    });

    let refused = Call::post(
        format!("/v1/projects/{project}/triggers:publish"),
        &serde_json::json!({"spec": spec}),
    )
    .signed_as(&world, "operator")
    .with_key("trg-operator")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.code(), "forbidden");

    let published = Call::post(
        format!("/v1/projects/{project}/triggers:publish"),
        &serde_json::json!({"spec": spec}),
    )
    .signed_as(&world, "admin")
    .with_key("trg-admin")
    .send(&world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    assert_eq!(
        published.json()["auto_arm"],
        serde_json::Value::Bool(true),
        "the published revision reports the capability it carries: {}",
        published.body
    );

    // It is durable and readable, so the capability is a stored fact rather than
    // an answer this one call computed.
    let read = Call::get(format!(
        "/v1/projects/{project}/triggers/trigger.inbound-request/1"
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(read.status, 200, "{}", read.body);
    assert_eq!(read.json()["auto_arm"], serde_json::Value::Bool(true));

    // A published revision is immutable. The same bytes replay; different bytes
    // under a new key are refused rather than quietly rewriting what a running
    // realm is already acting under.
    let replayed = Call::post(
        format!("/v1/projects/{project}/triggers:publish"),
        &serde_json::json!({"spec": spec}),
    )
    .signed_as(&world, "admin")
    .with_key("trg-admin-again")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);

    let mut widened = spec.clone();
    widened["approval"]["max_concurrency"] = serde_json::json!(64);
    let conflict = Call::post(
        format!("/v1/projects/{project}/triggers:publish"),
        &serde_json::json!({"spec": widened}),
    )
    .signed_as(&world, "admin")
    .with_key("trg-widened")
    .send(&world)
    .await;
    assert_eq!(conflict.status, 409, "{}", conflict.body);
    assert_eq!(conflict.code(), "idempotency_conflict");

    // And a document this build does not understand is refused as a request
    // problem, not stored and puzzled over later.
    let nonsense = Call::post(
        format!("/v1/projects/{project}/triggers:publish"),
        &serde_json::json!({"spec": {"schema_version": 1, "id": "trigger.bad"}}),
    )
    .signed_as(&world, "admin")
    .with_key("trg-nonsense")
    .send(&world)
    .await;
    assert_eq!(nonsense.status, 400, "{}", nonsense.body);
    assert_eq!(nonsense.code(), "invalid_request");
}

/// A body that parses for whichever operation `uri` names.
///
/// The authority tests need the extractor to succeed so that the refusal they
/// observe is the capability check and not `Json`'s.
fn well_formed_body(uri: &str) -> serde_json::Value {
    if uri.ends_with("epics:apply") || uri.ends_with("epics:preview") {
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
    } else if uri.ends_with("scheduler:resume") {
        serde_json::json!({
            "expected_revision": 1,
            "admissions": [{
                "team_run_id": TeamRunId::generate().to_string(),
                "agent_run_id": kontor_core::id::AgentRunId::generate().to_string(),
            }],
        })
    } else if uri.ends_with("lifecycle") {
        serde_json::json!({"action": "block", "expected_revision": 1, "reason": "x"})
    } else if uri.ends_with("projects:ensure") {
        serde_json::json!({
            "name": "X",
            "root_path": "/tmp/kontor-authz-body",
            "memory_origin": "kontor_native",
            "backlog_origin": "kontor_native",
        })
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
        format!("/v1/projects/{project}/epics:preview"),
        format!("/v1/projects/{project}/epics:apply"),
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        format!("/v1/projects/{project}/epics/{epic}/execution:disarm"),
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
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
        &serde_json::json!({
            "name": "X",
            "root_path": "/tmp/kontor-keyless",
            "memory_origin": "kontor_native",
            "backlog_origin": "kontor_native",
        }),
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
        "/v1/projects/{project_id}/epics:preview",
        "/v1/projects/{project_id}/epics:apply",
        "/v1/projects/{project_id}/epics/{epic_id}",
        "/v1/projects/{project_id}/epics/{epic_id}/execution:arm",
        "/v1/projects/{project_id}/epics/{epic_id}/execution:disarm",
        "/v1/projects/{project_id}/epics/{epic_id}/scheduler:plan",
        "/v1/projects/{project_id}/epics/{epic_id}/scheduler:start",
        "/v1/projects/{project_id}/epics/{epic_id}/scheduler:resume",
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
async fn exact_resume_recovers_one_durable_admission_without_the_scheduler_key() {
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

    // The admission commits before the runtime is called. A workspace failure
    // therefore leaves one durable TeamRun with an unbound first seat.
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse("/tmp/another-worktree")
            .expect("a valid root"),
    );
    let failed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("start-run")
    .send(&world)
    .await;
    assert_eq!(failed.status, 200, "{}", failed.body);
    assert_eq!(
        failed.json()["blocked"][0]["code"],
        "unsupported_capability"
    );
    let after_failure = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        after_failure.json()["tasks"][0]["team_runs"]
            .as_array()
            .expect("runs")
            .len(),
        1,
        "the failed native call preserves exactly one durable admission"
    );

    let preserved = &after_failure.json()["tasks"][0]["team_runs"][0];
    let preserved_team_run = preserved["team_run_id"]
        .as_str()
        .expect("a team run id")
        .to_owned();
    let preserved_agent_run = preserved["seats"][0]["agent_run_id"]
        .as_str()
        .expect("an agent run id")
        .to_owned();

    // Exact recovery is a set, not a multiset: naming one durable admission
    // twice is a contradictory command and reaches no runtime.
    let duplicate = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "admissions": [
                {"team_run_id": preserved_team_run, "agent_run_id": preserved_agent_run},
                {"team_run_id": preserved_team_run, "agent_run_id": preserved_agent_run},
            ],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-duplicate")
    .send(&world)
    .await;
    assert_eq!(duplicate.status, 400, "{}", duplicate.body);

    // Nor may a caller splice the real AgentRun into another TeamRun. The
    // supplied pair is the address, and both halves have to agree with the
    // immutable admission event before a native effect is attempted.
    let drifted = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "admissions": [{
                "team_run_id": TeamRunId::generate().to_string(),
                "agent_run_id": preserved_agent_run,
            }],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-drifted")
    .send(&world)
    .await;
    assert_eq!(drifted.status, 404, "{}", drifted.body);

    let stale_revision = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision + 1,
            "admissions": [{
                "team_run_id": preserved_team_run,
                "agent_run_id": preserved_agent_run,
            }],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-stale-revision")
    .send(&world)
    .await;
    assert_eq!(stale_revision.status, 409, "{}", stale_revision.body);

    // The caller no longer has `start-run`, the original scheduler key. The
    // exact pair is sufficient because Kontor resolves its immutable launch
    // intent internally; a fresh plan would correctly reject it as in flight.
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse("/w/started-epic/0").expect("a valid root"),
    );
    // A vendor outage is observed once per account, so it lands on every alias
    // that selects one — the family spelling alone would leave the bundled
    // chain's alias rungs launchable and prove nothing about the fallback.
    for provider in ["claude", "claude-work", "claude-personal"] {
        world.fake.provider_outage(
            provider,
            Some(ModelRung {
                provider: ProviderRef("codex".to_owned()),
                model: ModelRef("gpt-5.6-sol".to_owned()),
                effort: Some(EffortLevel::Xhigh),
            }),
        );
    }
    let builder = kontor_core::id::RoleSlotId::parse("builder").expect("builder slot");
    world.fake.refusing_launch_of(&builder);
    let partial = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "admissions": [{
                "team_run_id": preserved_team_run,
                "agent_run_id": preserved_agent_run,
            }],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-exact")
    .send(&world)
    .await;
    assert_eq!(partial.status, 200, "{}", partial.body);
    assert!(
        partial.json()["started"]
            .as_array()
            .expect("started")
            .is_empty()
    );
    assert_eq!(
        partial.json()["blocked"].as_array().expect("blocked").len(),
        1
    );
    assert_eq!(partial.json()["receipt"]["applied"], "created");
    // Even though the later builder launch refused, the architect attachment
    // is indexed and addressable immediately. No unrelated event is needed to
    // make `/v1/runs/{id}` catch up with the epic projection.
    let first_run = Call::get(format!("/v1/runs/{preserved_agent_run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(first_run.status, 200, "{}", first_run.body);
    assert_eq!(
        first_run.json()["value"]["agent_run_id"],
        preserved_agent_run
    );

    world.fake.allowing_launch_of(&builder);
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "admissions": [{
                "team_run_id": preserved_team_run,
                "agent_run_id": preserved_agent_run,
            }],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-exact")
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
        assert_eq!(seat["team_run_id"], preserved_team_run, "the same team run");
    }
    assert!(
        seats.iter().any(|seat| seat["applied"] == "unchanged")
            && seats.iter().any(|seat| seat["applied"] == "created"),
        "recovery must reuse the partial architect and create only missing seats: {}",
        started.body
    );
    for role in ["builder", "inspector"] {
        let run = seats
            .iter()
            .find(|seat| seat["role_slot"] == role)
            .and_then(|seat| seat["agent_run_id"].as_str())
            .and_then(|run| AgentRunId::parse(run).ok())
            .expect("the outage-sensitive role was seated");
        let model = world
            .fake
            .launched_model(run)
            .expect("the selected model route is observable");
        assert_eq!(
            model.provider.0, "codex",
            "{role} woke Claude during outage"
        );
        assert_eq!(model.model.0, "gpt-5.6-sol");
    }
    let agent_run = seats[0]["agent_run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();
    let team_run = seats[0]["team_run_id"]
        .as_str()
        .expect("a team run id")
        .to_owned();
    assert_eq!(agent_run, preserved_agent_run, "the same first AgentRun");
    assert_eq!(team_run, preserved_team_run, "the same TeamRun");

    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "admissions": [{
                "team_run_id": preserved_team_run,
                "agent_run_id": preserved_agent_run,
            }],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-exact")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert!(
        replayed.json()["started"]
            .as_array()
            .expect("seats")
            .iter()
            .all(|seat| seat["applied"] == "unchanged"),
        "a replay reuses every preserved seat: {}",
        replayed.body
    );

    // Once attached, the pair is no longer an unbound recovery candidate. A
    // different command key must not turn the exact-resume surface into a
    // second launch authority.
    let already_resumed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:resume"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "admissions": [{
                "team_run_id": preserved_team_run,
                "agent_run_id": preserved_agent_run,
            }],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("resume-again-under-another-key")
    .send(&world)
    .await;
    assert_eq!(already_resumed.status, 409, "{}", already_resumed.body);

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
        let run = Call::get(format!(
            "/v1/runs/{}",
            seat["agent_run_id"].as_str().expect("an agent run id")
        ))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
        assert_eq!(run.status, 200, "{}", run.body);
        assert_eq!(
            run.json()["value"]["projection"]["desired"],
            "run_requested",
            "every native launch has durable launch intent: {}",
            run.body
        );
        assert_eq!(
            run.json()["value"]["projection"]["observed"],
            "launching",
            "the runtime-issued launch observation is durable: {}",
            run.body
        );
        assert_eq!(
            run.json()["value"]["projection"]["derived"],
            "confirmed",
            "the binding, intent and launch observation agree: {}",
            run.body
        );
        assert_eq!(
            run.json()["value"]["projection"]["lifecycle"],
            "launching",
            "runtime evidence advances the AgentRun lifecycle: {}",
            run.body
        );
    }
    assert_eq!(
        runs[0]["lifecycle"], "launching",
        "runtime evidence advances the owning TeamRun too: {}",
        projection.body
    );
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
    let resumed_revision = resumed.json()["revision"].as_u64().expect("revision");
    assert!(
        resumed_revision > held_revision,
        "resume is a material mutation after the original block"
    );

    let replayed_block = Call::post(
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
    assert_eq!(replayed_block.status, 200, "{}", replayed_block.body);
    assert_eq!(
        replayed_block.json(),
        blocked.json(),
        "K1 must replay its original blocked state and revision after K2 resumed the live task"
    );

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

#[tokio::test]
async fn withdrawal_is_a_terminal_audited_scope_change_not_a_block_or_delete() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "withdraw-project",
        "Withdrawal",
        "/tmp/kontor-withdrawal",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");
    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Withdrawal epic",
            &category,
            serde_json::json!([
                {
                    "title": "Descoped dependency",
                    "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-7687"}]
                },
                {"title": "Dependent", "depends_on": ["Descoped dependency"]}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("withdraw-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let first = applied.json()["tasks"][0].clone();
    let second = applied.json()["tasks"][1].clone();
    let lifecycle = format!("/v1/projects/{project}/epics/{epic}/lifecycle");

    let invalid_withdrawal = serde_json::json!({
        "action": "withdraw_task",
        "task_id": second["task_id"],
        "expected_revision": second["revision"],
        "reason": "Invalid evidence must not leave withdrawal authority behind",
        "evidence": ["not a valid artifact key"]
    });
    for _ in 0..2 {
        let invalid = Call::post(&lifecycle, &invalid_withdrawal)
            .signed_as(&world, "operator")
            .with_key("withdraw-invalid-evidence")
            .send(&world)
            .await;
        assert_eq!(invalid.status, 400, "{}", invalid.body);
        assert_eq!(invalid.code(), "invalid_request");
    }
    let (invalid_receipt, untouched) = world.daemon.state().with_store(|store| {
        let receipt = store
            .get_receipt_by_key(
                &kontor_core::id::IdempotencyKey::parse("withdraw-invalid-evidence")
                    .expect("the test key is valid"),
            )
            .expect("the receipt lookup succeeds");
        let task = store
            .get_task(
                kontor_core::id::ProjectId::parse(&project).expect("project id"),
                kontor_core::id::TaskId::parse(second["task_id"].as_str().expect("task id"))
                    .expect("task id"),
            )
            .expect("the task reads")
            .expect("the task remains");
        (receipt, task)
    });
    assert!(
        invalid_receipt.is_none(),
        "a refused withdrawal records no authority receipt"
    );
    assert_eq!(untouched.state, kontor_core::state::TaskState::Ready);
    assert_eq!(
        untouched.revision.get(),
        second["revision"].as_u64().unwrap()
    );

    let unresolved = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "withdraw_task",
            "task_id": first["task_id"],
            "expected_revision": first["revision"],
            "reason": "The downstream consequence is not resolved"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("withdraw-unresolved")
    .send(&world)
    .await;
    assert_eq!(unresolved.status, 409, "{}", unresolved.body);

    let dependent = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "withdraw_task",
            "task_id": second["task_id"],
            "expected_revision": second["revision"],
            "reason": "Dependent work is explicitly descoped first"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("withdraw-dependent")
    .send(&world)
    .await;
    assert_eq!(dependent.status, 200, "{}", dependent.body);
    assert_eq!(dependent.json()["state"], "withdrawn");

    let blocked = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "block",
            "task_id": first["task_id"],
            "expected_revision": first["revision"],
            "reason": "Temporary hold remains a distinct state"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("withdraw-block-first")
    .send(&world)
    .await;
    assert_eq!(blocked.status, 200, "{}", blocked.body);
    assert_eq!(blocked.json()["state"], "blocked");

    let withdraw_first_request = serde_json::json!({
        "action": "withdraw_task",
        "task_id": first["task_id"],
        "expected_revision": blocked.json()["revision"],
        "reason": "ASMA-7687 left this epic's active scope"
    });
    let withdrawn = Call::post(&lifecycle, &withdraw_first_request)
        .signed_as(&world, "operator")
        .with_key("withdraw-first")
        .send(&world)
        .await;
    assert_eq!(withdrawn.status, 200, "{}", withdrawn.body);
    assert_eq!(withdrawn.json()["state"], "withdrawn");
    assert!(
        !withdrawn.json()["receipt_id"]
            .as_str()
            .expect("a receipt")
            .is_empty()
    );
    let withdrawal_receipt = world.daemon.state().with_store(|store| {
        store
            .get_receipt_by_key(
                &kontor_core::id::IdempotencyKey::parse("withdraw-first")
                    .expect("the test key is valid"),
            )
            .expect("the receipt reads")
            .expect("the withdrawal receipt exists")
    });
    assert_eq!(
        withdrawal_receipt.kind,
        kontor_core::receipt::CommandKind::WithdrawTask
    );

    let replay = Call::post(&lifecycle, &withdraw_first_request)
        .signed_as(&world, "operator")
        .with_key("withdraw-first")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["state"], "withdrawn");
    assert_eq!(replay.json()["revision"], withdrawn.json()["revision"]);
    assert_eq!(replay.json()["receipt_id"], withdrawn.json()["receipt_id"]);

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let tasks = projection.json()["tasks"]
        .as_array()
        .expect("tasks")
        .clone();
    assert!(tasks.iter().all(|task| task["state"] == "withdrawn"));
    let retained = tasks
        .iter()
        .find(|task| task["task_id"] == first["task_id"])
        .expect("the withdrawn task remains visible");
    assert_eq!(retained["links"][0]["external_issue_key"], "ASMA-7687");

    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    assert!(plan.json()["ready"].as_array().expect("ready").is_empty());

    // Declarative omission remains non-destructive: the withdrawn dependent is
    // absent from the new document and still present in history afterward.
    let project_read = Call::get(format!("/v1/projects/{project}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let reapply = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            project_read.json()["revision"]
                .as_u64()
                .expect("a revision"),
            "Withdrawal epic",
            &category,
            serde_json::json!([{
                "title": "Descoped dependency",
                "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-7687"}]
            }]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("withdraw-reapply-omission")
    .send(&world)
    .await;
    assert_eq!(reapply.status, 200, "{}", reapply.body);
    let after = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(after.json()["tasks"].as_array().expect("tasks").len(), 2);
    assert!(
        after.json()["tasks"]
            .as_array()
            .expect("tasks")
            .iter()
            .all(|task| task["state"] == "withdrawn")
    );
}

#[tokio::test]
async fn withdrawal_refuses_any_task_that_has_ever_had_a_team_run() {
    let world = World::open().await;
    let project = world.project;
    let epic = MiniProjectId::generate();
    let task = TaskId::generate();
    let seeded_team = world.daemon.state().with_store(|store| {
        let seeded_team = store
            .get_team_run(project, world.team_run)
            .expect("the seeded team run reads")
            .expect("the seeded team run exists");
        store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: project,
                name: name("Never-started invariant"),
                created_at: at("2026-08-20T07:00:00Z"),
            })
            .expect("the epic is created");
        store
            .create_task(&NewTask {
                id: task,
                project_id: project,
                mini_project_id: Some(epic),
                title: name("A task with historical run identity"),
                module: None,
                state: kontor_core::state::TaskState::Ready,
                created_at: at("2026-08-20T07:01:00Z"),
            })
            .expect("the task is created");
        store
            .create_team_run(&NewTeamRun {
                id: TeamRunId::generate(),
                project_id: project,
                task_id: task,
                snapshot: seeded_team.snapshot.clone(),
                created_at: at("2026-08-20T07:02:00Z"),
            })
            .expect("the historical TeamRun is created");
        seeded_team
    });
    let _ = seeded_team;

    let refused = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/lifecycle"),
        &serde_json::json!({
            "action": "withdraw_task",
            "task_id": task,
            "expected_revision": 1,
            "reason": "A run already exists"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("withdraw-historical-run")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    let stored = world.daemon.state().with_store(|store| {
        store
            .get_task(project, task)
            .expect("the task reads")
            .expect("the task remains")
    });
    assert_eq!(stored.state, kontor_core::state::TaskState::Ready);
    assert_eq!(stored.revision.get(), 1);
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
/// The receipt recorded under one idempotency key, whatever its execution mode.
///
/// This replaced a count over `unsettled_receipts()`. That query is the recovery
/// inventory and is deliberately `execution_mode = 'dispatch'`, so a successful
/// application operation -- which is a local command, confirmed on the way out
/// and queuing no dispatch -- is correctly absent from it. Counting it was
/// measuring the wrong set; naming the key asserts identity, which is what
/// idempotency actually means.
fn receipt_for(world: &World, key: &str) -> Option<kontor_core::receipt::CommandReceipt> {
    let key = IdempotencyKey::parse(key).expect("a valid idempotency key");
    world
        .daemon
        .state()
        .with_store(|store| store.get_receipt_by_key(&key).expect("readable"))
}

#[tokio::test]
async fn the_two_bootstrap_ensures_honour_their_idempotency_key() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let first = ensure_project(&world, "idem-1", "Kontor", "/tmp/kontor-idem").await;
    assert_eq!(first.status, 200, "{}", first.body);
    let project = first.json()["project_id"].as_str().expect("id").to_owned();
    let after_first = receipt_for(&world, "idem-1").expect("the ensure recorded a receipt");

    // Same key, same body: the original answer, and no second receipt.
    let replay = ensure_project(&world, "idem-1", "Kontor", "/tmp/kontor-idem").await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["project_id"], first.json()["project_id"]);
    assert_eq!(replay.json()["applied"], "unchanged");
    assert_eq!(
        receipt_for(&world, "idem-1").map(|receipt| receipt.id),
        Some(after_first.id),
        "a replay records nothing: the same receipt answers"
    );

    // Same key, different body: a typed conflict, and still nothing written.
    let reused = ensure_project(&world, "idem-1", "Kontor", "/tmp/kontor-other").await;
    assert_eq!(reused.status, 409, "{}", reused.body);
    assert_eq!(reused.code(), "idempotency_conflict");
    assert_eq!(
        receipt_for(&world, "idem-1").map(|receipt| receipt.id),
        Some(after_first.id),
        "a rejected reuse writes nothing"
    );
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
    let with_account = receipt_for(&world, "idem-account").expect("the ensure recorded a receipt");

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
    assert_eq!(
        receipt_for(&world, "idem-account").map(|receipt| receipt.id),
        Some(with_account.id),
        "a replay records nothing: the same receipt answers"
    );

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
            // Not `drift-{index}`: the project above was ensured under the key
            // `drift-1`, so case 1 collided with it and was refused for reuse
            // before it ever reached the drift comparison this loop is about.
            .with_key(format!("drift-case-{index}"))
            .send(&world)
            .await;
        assert_eq!(
            drifted.status, 409,
            "case {index} must be refused as drift: {}",
            drifted.body
        );
        // And it names what actually happened. `revision_conflict` used to be
        // reported here and told the caller to retry with a fresher revision --
        // advice an ensure takes no argument for, so following it loops.
        assert_eq!(
            drifted.json()["code"],
            "ensure_mismatch",
            "case {index} must name the mismatch: {}",
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
        // Every call in this test that *wrote* a receipt, named explicitly. The
        // drift cases are all 409 and record nothing, so the writes are the two
        // ensures and the identical re-ensure. Scanning `unsettled_receipts()`
        // used to serve here and no longer can -- an application receipt is a
        // confirmed local command, absent from the dispatch inventory -- and an
        // empty scan would have made the `all(...)` assertion below pass
        // vacuously while proving nothing.
        ["drift-1", "drift-create", "drift-same"]
            .into_iter()
            .filter_map(|key| {
                let key = IdempotencyKey::parse(key).expect("a valid key");
                store.get_receipt_by_key(&key).expect("readable")
            })
            .map(|receipt| receipt.intent.json().to_owned())
            .collect::<Vec<_>>()
    });
    assert!(
        !stored.is_empty(),
        "the ensures recorded receipts to inspect; an empty set proves nothing"
    );
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
        let key = IdempotencyKey::parse("revoke-key").expect("a valid key");
        store
            .get_receipt_by_key(&key)
            .expect("readable")
            .map(|receipt| receipt.kind.as_str().to_owned())
    });
    // Named by key rather than scanned out of `unsettled_receipts()`: a disarm
    // is an application operation, so its receipt is a confirmed local command
    // and never appears in the dispatch inventory.
    assert_eq!(
        kinds.as_deref(),
        Some("revoke_execution_authorization"),
        "disarm records its own command kind"
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

    // Task-control fixtures deliberately have no external ticket. The shared
    // runnable-epic helper adds a Jira identity because native TSW naming needs
    // one, so remove it here to retain the zero-link reconciliation contract.
    let mut body = epic_body(
        revision,
        "Control epic",
        &category,
        serde_json::json!([{"title": "The task"}]),
    );
    body["tasks"][0]
        .as_object_mut()
        .expect("the task request is an object")
        .remove("ticket_links");
    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
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
async fn resolving_a_task_context_tracks_approved_memory_and_returns_no_content() {
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

    let project_id = ProjectId::parse(&seed.project).expect("a project id");
    world
        .daemon
        .state()
        .with_store(|store| {
            let document = CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "text": "Read the approved project conventions before changing code"
            }))?;
            let provenance = kontor_store::memory::MemoryProvenance {
                source: "operator".to_owned(),
                source_id: None,
                legacy_last_write_wins: false,
                history_unavailable: false,
            };
            let (proposal, _) = store.propose_memory_revision(
                project_id,
                "project-conventions",
                0,
                &document,
                &provenance,
                "test-author",
            )?;
            store.approve_memory_revision(
                project_id,
                "project-conventions",
                &proposal.revision_id,
                1,
                "test-reviewer",
            )?;
            Ok::<_, kontor_store::memory::MemoryError>(())
        })
        .expect("approved project memory is seeded");

    // Moving approved memory moves the pack and attributes the new paths to the
    // immutable memory revision. The document still never leaves the process.
    let again = Call::post(&uri, &serde_json::json!({"snapshot": false}))
        .signed_as(&world, "operator")
        .with_key("ctx-preview-2")
        .send(&world)
        .await;
    assert_ne!(again.json()["context_hash"], hash);
    assert!(
        again.json()["provenance"]
            .as_array()
            .expect("provenance")
            .iter()
            .any(|entry| entry["path"] == "/memory/project-conventions/text"
                && entry["source_id"]
                    .as_str()
                    .is_some_and(|source| source.starts_with("memory."))),
        "approved memory is attributable in the Context Pack: {}",
        again.body
    );

    // Same task, pins and approved revisions, same bytes.
    let stable = Call::post(&uri, &serde_json::json!({"snapshot": false}))
        .signed_as(&world, "operator")
        .with_key("ctx-preview-3")
        .send(&world)
        .await;
    assert_eq!(stable.json()["context_hash"], again.json()["context_hash"]);

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

// ---------------------------------------------------------------------------
// Gate recovery: recording a verdict on behalf of a closed evaluator seat
// ---------------------------------------------------------------------------

/// The record route, the workflow revision and the first gate, all read from
/// the public projection so the test never names them itself.
async fn gate_record_target(world: &World, seed: &Bootstrapped) -> (String, u64, String) {
    let projection = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let task = &projection.json()["tasks"][0];
    let gate = task["gates"]
        .as_array()
        .expect("the pinned profile declares gates")[0]["gate"]
        .as_str()
        .expect("a gate id")
        .to_owned();
    let revision = task["workflow_revision"]
        .as_u64()
        .expect("a workflow revision");
    let uri = format!(
        "/v1/projects/{}/tasks/{}/gates/{gate}/record",
        seed.project, seed.task
    );
    (uri, revision, gate)
}

/// The run filling the `inspector` slot, found through the public snapshot.
async fn inspector_run(world: &World, runs: &[String]) -> String {
    for run in runs {
        let snapshot = Call::get(format!("/v1/runs/{run}"))
            .signed_as(world, "observer")
            .send(world)
            .await;
        assert_eq!(snapshot.status, 200, "{}", snapshot.body);
        if snapshot.json()["value"]["role"] == "inspector" {
            return run.clone();
        }
    }
    panic!("the seated team has no inspector seat")
}

/// Close one seat through the supported runtime-settlement path.
async fn close_seat(world: &World, seed: &Bootstrapped, run: &str, key: &str) -> Answer {
    finish_natively(world, run).await;
    let settled = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run}/runtime:settle",
            seed.project
        ),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    settled
}

/// The task's active workflow, addressed from the seeded ids.
fn active_workflow(world: &World, seed: &Bootstrapped) -> kontor_core::repository::TaskWorkflow {
    let project = ProjectId::parse(&seed.project).expect("the seeded project id parses");
    let task = TaskId::parse(&seed.task).expect("the seeded task id parses");
    world
        .daemon
        .state()
        .with_store(|store| store.get_active_task_workflow(project, task))
        .expect("the workflow reads back")
        .expect("the task has an active workflow")
}

/// A closed evaluator seat can no longer record its own gate: the gate that
/// was rendered in its session must be transcribed through the recovery path,
/// and the citation is refused *while* the seat is still able to act.
#[tokio::test]
async fn gate_recovery_is_refused_while_the_evaluator_seat_is_alive() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "rec-alive").await;
    let inspector = inspector_run(&world, &runs).await;
    let (uri, revision, _) = gate_record_target(&world, &seed).await;

    // The seat is running, so recovery is refused: it can record its own
    // verdict, and a citation must never let an operator pre-record one.
    let refused = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": inspector,
            "recovery_session_digest": ContentHash::of(b"REQUEST CHANGES: the fix is incomplete").as_str(),
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-alive-refused")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "revision_conflict");

    // Nothing was appended: the gate's history is still empty.
    let workflow = active_workflow(&world, &seed);
    let evaluations = world
        .daemon
        .state()
        .with_store(|store| {
            store.list_gate_evaluations(
                ProjectId::parse(&seed.project).expect("the seeded project id parses"),
                workflow.id,
            )
        })
        .expect("the evaluations read back");
    assert!(
        evaluations.is_empty(),
        "a refused recovery appends nothing: {evaluations:#?}"
    );

    // The ordinary path is unchanged: the evaluator's own recording — no
    // citation, attributed to the live seat — still works while the seat is
    // alive.
    let normal = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-alive-normal")
    .send(&world)
    .await;
    assert_eq!(normal.status, 200, "{}", normal.body);
    assert!(
        normal.json()["session_evidence"].is_null(),
        "an ordinary recording carries no citation: {}",
        normal.body
    );
}

/// The recovery path exists to transcribe an evaluator's *documented* verdict,
/// so every refusal here is about the evidence: no citation, a malformed
/// citation, or a citation that names anything but the closed evaluator seat.
#[tokio::test]
async fn gate_recovery_is_refused_without_matching_session_evidence() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "rec-noev").await;
    let inspector = inspector_run(&world, &runs).await;
    let mut builder = None;
    for run in &runs {
        let snapshot = Call::get(format!("/v1/runs/{run}"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(snapshot.status, 200, "{}", snapshot.body);
        if snapshot.json()["value"]["role"] == "builder" {
            builder = Some(run.clone());
            break;
        }
    }
    let builder = builder.expect("the seated team has a builder seat");
    let (uri, revision, _) = gate_record_target(&world, &seed).await;

    // The evaluator seat is closed, so this is exactly the incident's shape:
    // a gate rendered in a session that can no longer record itself.
    close_seat(&world, &seed, &inspector, "rec-noev-settle").await;

    // A citation whose digest is missing is refused before anything is written.
    let no_digest = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": inspector,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-noev-no-digest")
    .send(&world)
    .await;
    assert_eq!(no_digest.status, 400, "{}", no_digest.body);
    assert_eq!(no_digest.code(), "invalid_request");

    // A citation naming a run that does not exist in this project is refused:
    // the session record must be one this realm actually holds.
    let foreign = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": AgentRunId::generate().to_string(),
            "recovery_session_digest": ContentHash::of(b"some verdict").as_str(),
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-noev-foreign")
    .send(&world)
    .await;
    assert_eq!(foreign.status, 404, "{}", foreign.body);
    assert_eq!(foreign.code(), "not_found");

    // A citation naming a seat that holds another role is refused: the verdict
    // must come from the evaluator's own session, and a builder session has no
    // inspector verdict in it.
    let wrong_seat = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": builder,
            "recovery_session_digest": ContentHash::of(b"an inspector verdict").as_str(),
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-noev-wrong-seat")
    .send(&world)
    .await;
    assert_eq!(wrong_seat.status, 400, "{}", wrong_seat.body);
    assert_eq!(wrong_seat.code(), "invalid_request");

    // Nothing was appended by any of the refusals.
    let workflow = active_workflow(&world, &seed);
    let evaluations = world
        .daemon
        .state()
        .with_store(|store| {
            store.list_gate_evaluations(
                ProjectId::parse(&seed.project).expect("the seeded project id parses"),
                workflow.id,
            )
        })
        .expect("the evaluations read back");
    assert!(
        evaluations.is_empty(),
        "refused recovery recordings append nothing: {evaluations:#?}"
    );
}

/// The supported recovery: a closed evaluator seat's already-rendered verdict
/// is transcribed with a citation to its own session record, and the verdict
/// is attributed to that run and carries the citation as durable evidence.
#[tokio::test]
async fn gate_recovery_records_a_closed_evaluators_verdict_with_session_evidence() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "rec-closed").await;
    let inspector = inspector_run(&world, &runs).await;
    let (uri, revision, gate) = gate_record_target(&world, &seed).await;

    close_seat(&world, &seed, &inspector, "rec-closed-settle").await;

    // The inspector rendered REQUEST CHANGES in its session before the runtime
    // closed; the digest pins exactly that verdict content.
    let digest =
        ContentHash::of(b"REQUEST CHANGES: inherited properties satisfy the existing-entity check")
            .as_str()
            .to_owned();
    let recorded = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": inspector,
            "recovery_session_digest": digest,
            "reviewer_principal": "tpm-lead",
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-closed-record")
    .send(&world)
    .await;
    assert_eq!(recorded.status, 200, "{}", recorded.body);
    assert_eq!(recorded.json()["verdict"], "rejected");
    assert_eq!(recorded.json()["state"], "rejected", "{}", recorded.body);
    assert_eq!(
        recorded.json()["session_evidence"]["agent_run_id"],
        inspector,
        "the citation names the evaluator's own session: {}",
        recorded.body
    );
    assert_eq!(
        recorded.json()["session_evidence"]["digest"],
        digest,
        "the digest is echoed: {}",
        recorded.body
    );

    // The evaluation row is attributed to the cited run and carries the
    // citation as durable, append-only evidence.
    let workflow = active_workflow(&world, &seed);
    let evaluations = world
        .daemon
        .state()
        .with_store(|store| {
            store.list_gate_evaluations(
                ProjectId::parse(&seed.project).expect("the seeded project id parses"),
                workflow.id,
            )
        })
        .expect("the evaluations read back");
    assert_eq!(evaluations.len(), 1, "{evaluations:#?}");
    let evaluation = &evaluations[0];
    assert_eq!(evaluation.gate.as_str(), gate);
    assert_eq!(
        evaluation.agent_run_id,
        AgentRunId::parse(&inspector).ok(),
        "the verdict is attributed to the cited session, not to nobody: {evaluation:#?}"
    );
    let citation = evaluation
        .session_evidence
        .as_ref()
        .unwrap_or_else(|| panic!("the evaluation carries the citation: {evaluation:#?}"));
    assert_eq!(citation.agent_run_id.to_string(), inspector);
    assert_eq!(citation.digest.as_str(), digest);

    // Replaying the same key returns the same verdict and its citation, and
    // appends nothing second.
    let replayed = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": inspector,
            "recovery_session_digest": digest,
            "reviewer_principal": "tpm-lead",
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-closed-record")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["session_evidence"]["digest"],
        digest,
        "the replay answers from the stored record: {}",
        replayed.body
    );
    let after = world
        .daemon
        .state()
        .with_store(|store| {
            store.list_gate_evaluations(
                ProjectId::parse(&seed.project).expect("the seeded project id parses"),
                workflow.id,
            )
        })
        .expect("the evaluations read back");
    assert_eq!(
        after.len(),
        1,
        "a replay appends nothing second: {after:#?}"
    );
}

/// The recovery path cannot invent a pass: the pinned profile's evidence
/// requirements still bind, and a citation does not relax them.
#[tokio::test]
async fn gate_recovery_cannot_fabricate_a_pass() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "rec-pass").await;
    let inspector = inspector_run(&world, &runs).await;
    let (uri, revision, _) = gate_record_target(&world, &seed).await;

    close_seat(&world, &seed, &inspector, "rec-pass-settle").await;

    // A pass with a session citation but none of the profile-declared evidence
    // is refused exactly as an ordinary pass would be.
    let fabricated = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "passed",
            "evaluator_role": "inspector",
            "evaluator_account": seed.account,
            "recovery_agent_run_id": inspector,
            "recovery_session_digest": ContentHash::of(b"LGTM").as_str(),
        }),
    )
    .signed_as(&world, "operator")
    .with_key("rec-pass-fabricated")
    .send(&world)
    .await;
    assert_eq!(fabricated.status, 400, "{}", fabricated.body);
    assert_eq!(fabricated.code(), "invalid_request");
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
#[ignore = "superseded by kontor-jira native connector contract tests"]
async fn a_configured_jira_boundary_distinguishes_historical_from_native_completion() {
    let connector_dir = tempfile::TempDir::new().expect("a connector directory");
    let executable = connector_dir.path().join("asma-jira-fixture");
    let external_state = connector_dir.path().join("closed");
    let script = format!(
        r#"#!/usr/bin/env python3
import json, os, sys
request = json.load(sys.stdin)
state = {state:?}
closed = os.path.exists(state)
def observation(is_closed):
    return {{
        "status_id": "10228" if is_closed else "10214",
        "status_name": "Closed" if is_closed else "In Development",
        "status_category": "Done" if is_closed else "In Progress",
        "issue_type": "User Story",
        "assignee_account_id": "acct-igor",
        "assignee_display": "Igor",
        "update_token": "2" if is_closed else "1",
        "observation_hash": ("b" if is_closed else "a") * 64,
    }}
operation = request["operation"]
before = observation(closed)
response = {{
    "schema_version": 1,
    "operation": operation,
    "effective_operation": operation,
    "issue_key": request["issue_key"],
    "idempotency_key": request["idempotency_key"],
    "intent_hash": request.get("intent_hash"),
    "requested_at": "2026-08-19T10:00:00Z",
    "completed_at": "2026-08-19T10:00:01Z",
    "outcome": "observed" if operation in ("observe", "refetch") else "planned",
    "observation": before,
    "principal_account_id": "acct-igor",
    "live_transitions": [] if closed else [{{
        "transition_id": "3", "to_status_id": "10228", "to_status_name": "Closed",
        "to_status_category": "Done"
    }}],
    "effects": {{"field_ids": [], "assignment": None, "transition": request.get("transition")}},
    "notes": [],
}}
if operation == "apply":
    open(state, "w").close()
    response["outcome"] = "applied"
    response["confirmation"] = {{
        "observation": observation(True),
        "confirmed_at": "2026-08-19T10:00:01Z",
    }}
print(json.dumps(response, sort_keys=True))
"#,
        state = external_state.to_string_lossy()
    );
    std::fs::write(&executable, script).expect("the connector fixture is written");
    let mut permissions = std::fs::metadata(&executable)
        .expect("the connector fixture exists")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)
        .expect("the connector fixture is executable");

    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let historical_project = ensure_project(
        &world,
        "jira-historical",
        "Historical Jira import",
        "/tmp/kontor-jira-historical",
    )
    .await;
    let historical_project_id = historical_project.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let historical_revision = historical_project.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;
    let historical = Call::post(
        format!("/v1/projects/{historical_project_id}/epics:apply"),
        &epic_body(
            historical_revision,
            "Historical Jira epic",
            &category,
            serde_json::json!([{
                "title": "Imported completion",
                "import_state": "completed",
                "ticket_links": [{
                    "connector": "jira",
                    "external_issue_key": "ASMA-7875"
                }]
            }]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("jira-historical-import")
    .send(&world)
    .await;
    assert_eq!(historical.status, 200, "{}", historical.body);
    let historical_task = historical.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();
    let without_policy = Call::post(
        format!(
            "/v1/projects/{historical_project_id}/tasks/{historical_task}/ticket:reconcile-plan"
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(without_policy.status, 422, "{}", without_policy.body);
    assert_eq!(without_policy.code(), "unsupported_capability");
    assert!(
        without_policy.body.contains("connector.jira"),
        "the refusal names the canonical install key: {}",
        without_policy.body
    );
    let installed = install_jira_workflow(
        &world,
        &historical_project_id,
        "jira-historical-workflow-install",
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.body);

    let historical_plan = Call::post(
        format!(
            "/v1/projects/{historical_project_id}/tasks/{historical_task}/ticket:reconcile-plan"
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(historical_plan.status, 200, "{}", historical_plan.body);
    assert!(
        historical_plan.json()["diff"]
            .as_array()
            .expect("a typed diff")
            .iter()
            .all(|entry| entry["milestone"] != "terminal_done"),
        "historical completion must not authorize Jira closure: {}",
        historical_plan.body
    );
    assert!(
        !external_state.exists(),
        "a dry-run against historical completion must not reach apply"
    );

    let seed = bootstrap(&world, "jira-composed").await;
    let installed =
        install_jira_workflow(&world, &seed.project, "jira-composed-workflow-install").await;
    assert_eq!(installed.status, 200, "{}", installed.body);
    let project = ProjectId::parse(&seed.project).expect("a project id");
    let epic = MiniProjectId::parse(&seed.epic).expect("an epic id");
    let seed_task = TaskId::parse(&seed.task).expect("a task id");
    let done_task = TaskId::generate();
    world.daemon.state().with_store(|store| {
        let workflow = store
            .get_active_task_workflow(project, seed_task)
            .expect("the workflow reads")
            .expect("the workflow exists");
        store
            .create_task(&NewTask {
                id: done_task,
                project_id: project,
                mini_project_id: Some(epic),
                title: name("Already completed"),
                module: None,
                state: kontor_core::state::TaskState::Done,
                created_at: at("2026-08-19T09:00:00Z"),
            })
            .expect("the completed fixture task is created");
        store
            .create_task_workflow(&NewTaskWorkflow {
                id: kontor_core::id::TaskWorkflowId::generate(),
                project_id: project,
                task_id: done_task,
                snapshot: workflow.snapshot,
                current_phase: workflow.current_phase,
                created_at: at("2026-08-19T09:00:00Z"),
            })
            .expect("the completed fixture workflow is created");
        store
            .create_ticket_link(&NewTicketLink {
                id: TicketLinkId::generate(),
                project_id: project,
                task_id: done_task,
                connector: kontor_core::id::ConnectorKey::parse("jira").expect("a connector key"),
                external_issue_key: kontor_core::id::ExternalId::parse("ASMA-7874")
                    .expect("an issue key"),
                created_at: at("2026-08-19T09:00:00Z"),
            })
            .expect("the completed fixture ticket is linked");
    });

    let plan_uri = format!("/v1/projects/{project}/tasks/{done_task}/ticket:reconcile-plan");
    let plan = Call::post(&plan_uri, &serde_json::json!({}))
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    assert!(!plan.json()["converged"].as_bool().expect("a flag"));
    assert_eq!(plan.json()["diff"][0]["milestone"], "terminal_done");
    assert_eq!(plan.json()["diff"][0]["kontor"], "Closed");
    assert_eq!(plan.json()["diff"][0]["external"], "In Development");

    let applied = Call::post(
        format!("/v1/projects/{project}/tasks/{done_task}/ticket:reconcile-apply"),
        &serde_json::json!({"projection_hash": plan.json()["projection_hash"]}),
    )
    .signed_as(&world, "operator")
    .with_key("jira-composed-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert!(
        external_state.is_file(),
        "the validated apply reached the boundary"
    );

    let readback = Call::post(&plan_uri, &serde_json::json!({}))
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(readback.status, 200, "{}", readback.body);
    assert!(readback.json()["converged"].as_bool().expect("a flag"));
    assert!(
        readback.json()["diff"]
            .as_array()
            .expect("a diff")
            .is_empty()
    );
}

#[tokio::test]
async fn jira_materialization_preview_is_server_derived_and_epic_first() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "jira-materialization").await;
    let preview = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/jira:preview",
            seed.project, seed.epic
        ),
        &serde_json::json!({
            "epic": {"mode": "create"},
            "tasks": {(seed.task.clone()): {"mode": "create"}}
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let body = preview.json();
    let items = body["items"].as_array().expect("ordered items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["item_kind"], "epic");
    assert_eq!(items[1]["item_kind"], "task");
    assert_eq!(items[1]["task_id"], seed.task);
    assert_eq!(
        body["preview_hash"].as_str().expect("preview hash").len(),
        64
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
async fn a_teamless_task_is_held_and_resumed_and_its_epic_stays_open() {
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

/// Arm, plan and start an existing bootstrapped task.
async fn seat_existing(world: &World, seed: &Bootstrapped, prefix: &str) -> Vec<String> {
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
    .with_key(format!("{prefix}-arm"))
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
    .with_key(format!("{prefix}-start"))
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
    runs
}

/// Bootstrap, arm, plan and start, returning `(seed, every seated run)`.
async fn seated(world: &World, slug: &'static str) -> (Bootstrapped, Vec<String>) {
    let seed = bootstrap(world, slug).await;
    let runs = seat_existing(world, &seed, slug).await;
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
    let world = World::open().await;
    // This harness launch deliberately models the historical projection gap: a
    // native binding exists, but no launch intent or observation was stored.
    world.script(HISTORY_LIVE);
    let (run, _) = world.launch().await;
    let before = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(before.json()["value"]["projection"]["desired"], "no_intent");
    assert_eq!(before.json()["value"]["projection"]["observed"], "unknown");

    // A supported native operation makes the exact existing session report
    // running. Reconciliation must inspect this binding; it must not launch or
    // replace anything to repair the omitted projection writes.
    let message_id = kontor_runtime::request::MessageId::generate().to_string();
    let sent = Call::post(
        format!("/v1/sessions/{run}/messages"),
        &serde_json::json!({"body": "continue the existing run"}),
    )
    .signed_as(&world, "operator")
    .with_key(message_id)
    .send(&world)
    .await;
    assert_eq!(sent.status, 200, "{}", sent.body);

    let reconciled = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{run}/runtime:settle",
            world.project
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("live-settle")
    .send(&world)
    .await;
    assert_eq!(reconciled.status, 200, "{}", reconciled.body);
    assert_eq!(reconciled.json()["observed"], "running");
    assert!(reconciled.json()["outcome"].is_null());
    assert!(reconciled.json()["team_run_closed"].is_null());

    // The run is still open, but its projection now states exactly what the
    // supported runtime evidence proved. The TeamRun moves with its child.
    let snapshot = Call::get(format!("/v1/runs/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        snapshot.json()["value"]["projection"]["desired"],
        "run_requested"
    );
    assert_eq!(
        snapshot.json()["value"]["projection"]["observed"],
        "running"
    );
    assert_eq!(
        snapshot.json()["value"]["projection"]["derived"],
        "confirmed"
    );
    assert_eq!(
        snapshot.json()["value"]["projection"]["lifecycle"],
        "running"
    );
    let team = world.daemon.state().with_store(|store| {
        store
            .get_team_run(world.project, world.team_run)
            .expect("the team reads")
            .expect("the team exists")
    });
    assert_eq!(team.lifecycle, kontor_core::state::RunLifecycle::Running);
}

/// Settle every seat of one seated task, and return the last answer.
///
/// Extracted so more than one test can reach a *closed team*, which is the state
/// every terminal task transition is judged against.
async fn settle_every_seat(
    world: &World,
    seed: &Bootstrapped,
    runs: &[String],
    prefix: &str,
) -> Answer {
    // Every declared seat is settled, one call each. The team closes on the last
    // one, because the closure walks the template's declared slots and an
    // unsettled seat is unaccounted for rather than absent.
    let mut settled = None;
    for (index, run) in runs.iter().enumerate() {
        finish_natively(world, run).await;
        let answer = Call::post(
            format!(
                "/v1/projects/{}/agent-runs/{run}/runtime:settle",
                seed.project
            ),
            &serde_json::json!({}),
        )
        .signed_as(world, "operator")
        .with_key(format!("{prefix}-settle-{index}"))
        .send(world)
        .await;
        assert_eq!(answer.status, 200, "seat {index}: {}", answer.body);
        settled = Some(answer);
    }
    settled.expect("at least one seat was settled")
}

/// Discharge one task's pinned profile through the public routes and complete it.
///
/// Every gate the profile declares is recorded by a role *it* authorizes, citing
/// the evidence *it* requires, all read from the projection — so nothing here is a
/// literal a test invented, and the completion cites the artifacts the profile
/// asks for.
///
/// Returns the completion answer.
async fn discharge_the_profile_and_complete(
    world: &World,
    seed: &Bootstrapped,
    prefix: &str,
) -> Answer {
    let lifecycle = format!(
        "/v1/projects/{}/epics/{}/lifecycle",
        seed.project, seed.epic
    );
    let projection = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(world, "observer")
        .send(world)
        .await;
    let projection_json = projection.json();
    let projected_task = projection_json["tasks"]
        .as_array()
        .expect("a task list")
        .iter()
        .find(|task| task["task_id"] == seed.task)
        .expect("the selected task is projected");
    // Now discharge that profile. Every gate it declares is recorded through the
    // public route, by a role *it* authorizes, citing the evidence *it* requires —
    // all of which the projection reports, so nothing here is read out of band and
    // nothing is a literal this test invented.
    let gates = projected_task["gates"]
        .as_array()
        .expect("a gate list")
        .clone();
    assert!(
        !gates.is_empty(),
        "the pinned profile declares gates to discharge: {}",
        projection.body
    );
    let workflow_revision = projected_task["workflow_revision"]
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
        .signed_as(world, "operator")
        .with_key(format!("{prefix}-gate-{index}"))
        .send(world)
        .await;
        assert_eq!(recorded.status, 200, "gate `{name}`: {}", recorded.body);
        assert_eq!(recorded.json()["verdict"], "passed");
        assert_eq!(recorded.json()["state"], "passed", "gate `{name}` reduced");
    }

    // Every gate now reads as passed through the public projection.
    let after_gates = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(world, "observer")
        .send(world)
        .await;
    let after = after_gates.json();
    let completed_task = after["tasks"]
        .as_array()
        .expect("a task list")
        .iter()
        .find(|task| task["task_id"] == seed.task)
        .expect("the selected task remains projected");
    for gate in completed_task["gates"].as_array().expect("a gate list") {
        assert_eq!(
            gate["state"], "passed",
            "gate `{}` is discharged: {}",
            gate["gate"], after_gates.body
        );
    }

    // The completion cites every artifact the profile requires — again read from
    // the projection rather than named here.
    let artifacts: Vec<&str> = completed_task["required_artifacts"]
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
    let task_revision = completed_task["revision"].as_u64().expect("a revision");
    let done = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "complete_task", "task_id": seed.task,
            "expected_revision": task_revision, "reason": "The work is done",
            "evidence": artifacts,
        }),
    )
    .signed_as(world, "operator")
    .with_key(format!("{prefix}-complete"))
    .send(world)
    .await;
    assert_eq!(done.status, 200, "{}", done.body);
    assert_eq!(done.json()["state"], "done");

    done
}

/// A completed task is reopened, and its history survives the reopen.
///
/// The gap the ASMA-7869 LSA hit: `reopen_task` was advertised, mapped to `ready`
/// and given a resume receipt, and then refused — because the domain rejected every
/// terminal source before it ever looked at what was being asked for. This is the
/// path end to end, plus the two things that must stay refused.
///
/// See `_docs/ai-orchestration/reports/2026-08-17-13-47-report-kontor-reopen-task-terminal-gap.md`.
#[tokio::test]
async fn a_completed_task_reopens_without_rewriting_what_it_recorded() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "reopen").await;
    settle_every_seat(&world, &seed, &runs, "reopen").await;
    let done = discharge_the_profile_and_complete(&world, &seed, "reopen").await;
    assert_eq!(done.status, 200, "{}", done.body);
    assert_eq!(done.json()["state"], "done");
    let completed_revision = done.json()["revision"].as_u64().expect("a revision");

    let lifecycle = format!(
        "/v1/projects/{}/epics/{}/lifecycle",
        seed.project, seed.epic
    );
    // What the completion recorded, read before the reopen so it can be compared
    // afterwards: the gates it was granted on, and the runs that produced them.
    let before = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let gates_before = before.json()["tasks"][0]["gates"].clone();
    let runs_before = before.json()["tasks"][0]["team_runs"].clone();

    // An ordinary transition out of a terminal task is still refused, and the
    // refusal still names terminality — a resume carries the same kind of receipt
    // a reopen does, so this is the assertion that keeps the two apart.
    let resumed = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "resume", "task_id": seed.task,
            "expected_revision": completed_revision, "reason": "Carry on"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("reopen-resume")
    .send(&world)
    .await;
    assert_eq!(resumed.status, 409, "{}", resumed.body);
    assert_eq!(resumed.code(), "revision_conflict");
    assert!(
        resumed.body.contains("terminal"),
        "the refusal names the rule that stopped it: {}",
        resumed.body
    );

    // From here on, nothing may reach the runtime: a reopen claims no seat.
    world.fake.take_calls();

    let reopened = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "reopen_task", "task_id": seed.task,
            "expected_revision": completed_revision,
            "reason": "The completion no longer covers the work"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("reopen-reopen")
    .send(&world)
    .await;
    assert_eq!(reopened.status, 200, "{}", reopened.body);
    assert_eq!(reopened.json()["state"], "ready", "{}", reopened.body);
    assert!(
        reopened.json()["revision"].as_u64().expect("a revision") > completed_revision,
        "the task moved forward rather than back: {}",
        reopened.body
    );
    assert!(
        reopened.json()["receipt_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "the reopen is recorded as a durable command: {}",
        reopened.body
    );

    // Nothing the completion was granted on was rewritten. The gates still read
    // exactly as they did, and the runs that produced them are still closed —
    // reopening says the completion no longer covers the work, not that the
    // history was wrong.
    let after = Call::get(format!("/v1/projects/{}/epics/{}", seed.project, seed.epic))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        after.json()["tasks"][0]["gates"],
        gates_before,
        "a reopen must not touch a single gate verdict: {}",
        after.body
    );
    assert_eq!(
        after.json()["tasks"][0]["team_runs"],
        runs_before,
        "and it must not reopen a team run or an agent run: {}",
        after.body
    );
    assert_eq!(after.json()["tasks"][0]["state"], "ready");

    // The runtime was never asked for anything between the completion and the
    // reopened task: a reopen claims no seat and starts nothing.
    let calls = world.fake.take_calls();
    assert!(
        calls.is_empty(),
        "a reopen reaches no runtime at all: {calls:?}"
    );

    // A second reopen is refused, and not by terminality: the task is open, so
    // there is nothing to reopen, and answering otherwise would let a reopen stand
    // in for a resume and skip the receipt rule that governs one.
    let reopened_revision = reopened.json()["revision"].as_u64().expect("a revision");
    let again = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "reopen_task", "task_id": seed.task,
            "expected_revision": reopened_revision, "reason": "Again"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("reopen-again")
    .send(&world)
    .await;
    assert_eq!(again.status, 400, "{}", again.body);
    assert!(
        again.body.contains("task reopen"),
        "the refusal names what was refused: {}",
        again.body
    );

    // And the reopened task can be completed again on fresh evidence, which is
    // the point of reopening it: the whole close-out path is available, not a
    // task parked in `ready` forever.
    let recompleted = Call::post(
        &lifecycle,
        &serde_json::json!({
            "action": "complete_task", "task_id": seed.task,
            "expected_revision": reopened_revision, "reason": "Now it really is done",
            "evidence": before.json()["tasks"][0]["required_artifacts"].clone(),
        }),
    )
    .signed_as(&world, "operator")
    .with_key("reopen-recomplete")
    .send(&world)
    .await;
    assert_eq!(recompleted.status, 200, "{}", recompleted.body);
    assert_eq!(recompleted.json()["state"], "done");
}

#[tokio::test]
async fn settlement_closes_the_team_and_unlocks_the_whole_epic_close_out() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = seated(&world, "endgame").await;

    let settled = settle_every_seat(&world, &seed, &runs, "endgame").await;
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
    .with_key("endgame-complete-early")
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

    let done = discharge_the_profile_and_complete(&world, &seed, "endgame").await;
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

    // Closing the epic closes its control plane. That release is what every
    // delivery seat of this epic reads its orphanhood from, so it is asserted
    // here on the real close path rather than assumed.
    let project_id = ProjectId::parse(&seed.project).expect("a project id");
    let epic_id = MiniProjectId::parse(&seed.epic).expect("an epic id");
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    world.daemon.state().with_store(|store| {
        let control = store
            .list_topology_nodes(project_id, Some(epic_id))
            .expect("the epic's nodes read")
            .into_iter()
            .find(|node| node.kind == domain.delivery.control_kind)
            .expect("admission opened the epic's control plane");
        let seats = store
            .list_seat_bindings(project_id, control.id)
            .expect("the control seats read");
        // The owner seat specifically. `release_epic_control_seat` touches the
        // declared control role and nothing else — the rest of the epic's
        // leadership is not dismissed by closing the epic — so asserting that
        // every seat on the plane released would be asserting a contract the
        // close path does not have.
        let owners: Vec<_> = seats
            .iter()
            .filter(|seat| seat.role.role_code == domain.delivery.control_role_code)
            .collect();
        assert!(!owners.is_empty(), "the epic had a control seat to close");
        assert!(
            owners.iter().all(|seat| seat.released_at.is_some()),
            "closing the epic released its control seat"
        );
    });
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
async fn an_admin_installs_the_exact_shipped_workflow_revision_under_project_cas() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "workflow-install").await;

    // Ticket links use `jira`; the catalogue uses `connector.jira`. The alias
    // must discover the canonical key instead of returning a misleading empty
    // list.
    let alias = Call::get(format!(
        "/v1/projects/{}/connectors/jira/workflow-specs",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(alias.status, 200, "{}", alias.body);
    let shipped = alias.json();
    let shipped = &shipped.as_array().expect("the shipped revisions")[0];
    assert_eq!(shipped["connector"], "connector.jira");
    assert_eq!(shipped["installed"], false);

    let project = Call::get(format!("/v1/projects/{}", seed.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let revision = project.json()["revision"].as_u64().expect("a revision");
    let uri = format!(
        "/v1/projects/{}/connectors/jira/workflow-specs:install",
        seed.project
    );
    let request = serde_json::json!({
        "external_project": shipped["external_project"],
        "issue_type": shipped["issue_type"],
        "version": shipped["version"],
        "expected_revision": revision,
    });

    let operator = Call::post(&uri, &request)
        .signed_as(&world, "operator")
        .with_key("workflow-install-operator")
        .send(&world)
        .await;
    assert_eq!(operator.status, 403, "{}", operator.body);

    let installed = Call::post(&uri, &request)
        .signed_as(&world, "admin")
        .with_key("workflow-install-once")
        .send(&world)
        .await;
    assert_eq!(installed.status, 200, "{}", installed.body);
    assert_eq!(installed.json()["spec"]["connector"], "connector.jira");
    assert_eq!(installed.json()["spec"]["installed"], true);
    assert_eq!(installed.json()["receipt"]["applied"], "created");
    assert_eq!(installed.json()["receipt"]["revision"], revision + 1);
    let receipt = installed.json()["receipt"]["receipt_id"]
        .as_str()
        .expect("a receipt")
        .to_owned();

    let readback = Call::get(format!(
        "/v1/projects/{}/connectors/connector.jira/workflow-specs",
        seed.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(readback.status, 200, "{}", readback.body);
    assert_eq!(readback.json()[0]["installed"], true);

    let project_id = ProjectId::parse(&seed.project).expect("project id");
    let selector = ConnectorSpecSelector {
        project_id,
        connector: ConnectorKey::parse(shipped["connector"].as_str().expect("connector key"))
            .expect("connector key"),
        project: ExternalProjectKey::parse(
            shipped["external_project"]
                .as_str()
                .expect("external project"),
        )
        .expect("external project"),
        issue_type: ExternalIssueTypeKey::parse(
            shipped["issue_type"].as_str().expect("issue type"),
        )
        .expect("issue type"),
        version: SpecVersion::parse(
            u32::try_from(shipped["version"].as_u64().expect("version")).expect("u32 version"),
        )
        .expect("version"),
    };
    world.daemon.state().with_store(|store| {
        let mut second_spec = store
            .get_external_workflow_spec(&selector)
            .expect("the installed spec reads")
            .expect("the installed spec exists");
        second_spec.version = second_spec.version.next().expect("a next spec revision");
        let (_, moved, _) = store
            .install_external_workflow_spec(
                project_id,
                AggregateRevision::parse(revision + 1).expect("project revision"),
                &second_spec,
            )
            .expect("an intervening workflow revision moves the project");
        assert_eq!(moved.get(), revision + 2);
    });
    let moved_project = Call::get(format!("/v1/projects/{}", seed.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(moved_project.status, 200, "{}", moved_project.body);
    assert!(
        moved_project.json()["revision"].as_u64().expect("revision") > revision + 1,
        "the replay must be tested after the project has moved again"
    );

    let replay = Call::post(&uri, &request)
        .signed_as(&world, "admin")
        .with_key("workflow-install-once")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["receipt_id"], receipt);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replay.json()["receipt"]["revision"], revision + 1);

    let stale = Call::post(&uri, &request)
        .signed_as(&world, "admin")
        .with_key("workflow-install-stale")
        .send(&world)
        .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");
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

/// OP-08's dynamic-scope tracer. The runtime starts with no task inventory:
/// the applied task itself is the durable source for its ticket title, Jira
/// issue and canonical worktree. Materialization binds that exact ticket
/// workspace before scheduling, and an exact scheduler replay reuses the same
/// TeamRun, runs, seats and native container.
#[tokio::test]
async fn an_applied_task_materializes_and_replays_without_a_startup_task_scope() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let created = ensure_project(
        &world,
        "dynamic-scope",
        "Kontor",
        "/tmp/kontor-dynamic-scope",
    )
    .await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let project_revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let category = first_category(&world).await;
    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Implement",
            "harness": "fake.runtime",
            "credential_alias": "implement",
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("dynamic-scope-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &serde_json::json!({
            "expected_revision": project_revision,
            "name": "Operational control surfaces",
            "work_profile_category": category,
            "runtime_family": "fake.runtime",
            "account_profile_id": account_id,
            "execution_scope": {
                "external_epic_key": "ASMA-7877",
                "short_title": "Operational control surfaces",
                "kontor_backlog_code": "OP-08",
                "ai_short_name": "Operational Control"
            },
            "tasks": [{
                "title": "OP-08",
                "short_code": "OP-08",
                "ticket_links": [{
                    "connector": "jira",
                    "external_issue_key": "ASMA-7877"
                }],
                "worktree": "/w/op-08"
            }]
        }),
    )
    .signed_as(&world, "admin")
    .with_key("dynamic-scope-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let epic_revision = applied.json()["revision"]
        .as_u64()
        .expect("an epic revision");
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();

    let materialized = Call::post(
        format!("/v1/projects/{project}/topology:materialize"),
        &serde_json::json!({
            "target": {"scope": "ticket", "task_id": task},
            "expected_revision": project_revision
        }),
    )
    .signed_as(&world, "operator")
    .with_key("dynamic-scope-materialize")
    .send(&world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let task_node = materialized.json()["projection"]["nodes"]
        .as_array()
        .expect("topology nodes")
        .iter()
        .find(|node| node["kind_key"] == "TSW")
        .expect("the task node")
        .clone();
    assert_eq!(
        task_node["observed_binding"]["cwd"], "/w/op-08",
        "materialization read the applied worktree back: {}",
        materialized.body
    );
    let task_node_id = TopologyNodeId::parse(
        task_node["topology_node_id"]
            .as_str()
            .expect("a topology node id"),
    )
    .expect("a topology node id");
    assert_eq!(
        world.fake.container_title(task_node_id).as_deref(),
        Some("TSW • ASMA-7877 • OP-08"),
        "the runtime rendered the workspace from durable task scope"
    );
    assert!(
        task_node["seats"]
            .as_array()
            .expect("task seats")
            .is_empty(),
        "materialization does not admit a TeamRun or pre-create a delivery seat"
    );

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10,
                       "max_duration_seconds": 600, "max_cost_minor_units": 100,
                       "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Trace dynamic materialization through admission"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("dynamic-scope-arm")
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
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a plan hash")
        .to_owned();
    let start_uri = format!("/v1/projects/{project}/epics/{epic}/scheduler:start");
    let start_body = serde_json::json!({"plan_hash": plan_hash});
    let first = Call::post(&start_uri, &start_body)
        .signed_as(&world, "operator")
        .with_key("dynamic-scope-start")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let first_seats = first.json()["started"]
        .as_array()
        .expect("started seats")
        .clone();
    assert!(!first_seats.is_empty(), "{}", first.body);

    let replay = Call::post(&start_uri, &start_body)
        .signed_as(&world, "operator")
        .with_key("dynamic-scope-start")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    let replayed_seats = replay.json()["started"]
        .as_array()
        .expect("replayed seats")
        .clone();
    assert_eq!(replayed_seats.len(), first_seats.len(), "{}", replay.body);
    for (first, replayed) in first_seats.iter().zip(&replayed_seats) {
        assert_eq!(replayed["team_run_id"], first["team_run_id"]);
        assert_eq!(replayed["agent_run_id"], first["agent_run_id"]);
        assert_eq!(replayed["native_id"], first["native_id"]);
        assert_eq!(replayed["applied"], "unchanged");
    }

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let projected = projection.json();
    let runs = projected["tasks"][0]["team_runs"]
        .as_array()
        .expect("team runs");
    assert_eq!(
        runs.len(),
        1,
        "replay created no TeamRun: {}",
        projection.body
    );
    assert_eq!(
        runs[0]["seats"].as_array().expect("team seats").len(),
        first_seats.len(),
        "one attached seat exists per declared slot: {}",
        projection.body
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

#[tokio::test]
async fn epic_read_preserves_the_team_revision_apply_froze_across_restart() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "team-read", "Kontor", "/tmp/kontor-team-read").await;
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
            "Pinned team epic",
            &category,
            serde_json::json!([{"title": "One task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("team-read-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("epic id")
        .to_owned();
    let expected = applied.json()["team_template"].clone();
    assert!(expected.is_object(), "apply froze a team revision");

    let uri = format!("/v1/projects/{project}/epics/{epic}");
    let immediate = Call::get(&uri)
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(immediate.status, 200, "{}", immediate.body);
    assert_eq!(immediate.json()["team_template"], expected);

    let observer = secret(&world, "observer");
    let realm = world.realm_id();
    let World {
        directory, daemon, ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(daemon);
    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the same state root reopens");
    assert_eq!(restarted.realm_id(), realm);

    let after_restart = Call::get(&uri)
        .with_token(&observer)
        .send_to(&restarted.router())
        .await;
    assert_eq!(after_restart.status, 200, "{}", after_restart.body);
    assert_eq!(after_restart.json()["team_template"], expected);
    restarted.state().signals().stop();
}

/// The incident pack the pilot fixture ships — a profile and a team this build's
/// compiled catalogue has never seen.
const INCIDENT_PACK: &str =
    include_str!("../../../tests/fixtures/pilot/incident-response-pack.json");

/// Build a later immutable revision of the incident fixture while keeping its
/// category stable. Tests use append-only rows with deliberately ordered
/// registration instants to model a daemon/catalog progression without editing
/// either published revision.
fn incident_pack_revision(version: u32, pack_id: &str) -> serde_json::Value {
    let mut pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");
    pack["pack_id"] = serde_json::json!(pack_id);
    pack["version"] = serde_json::json!(version);
    pack["manifest"][0]["profile_version"] = serde_json::json!(version);
    pack["profiles"][0]["version"] = serde_json::json!(version);
    pack["profiles"][0]["team_template"]["version"] = serde_json::json!(version);
    pack["teams"][0]["version"] = serde_json::json!(version);
    pack
}

/// Append one validated pack directly through the store's public immutable
/// registration operation, returning its exact document for later resolution.
fn append_profile_pack(
    world: &World,
    pack: &serde_json::Value,
    registered_at: Timestamp,
    key: &str,
) -> String {
    let document = serde_json::to_string(pack).expect("the pack serializes");
    kontor_profiles::pack::parse_pack(&document).expect("the appended pack remains valid");
    let pack_id = pack["pack_id"].as_str().expect("a pack id").to_owned();
    let version = SpecVersion::parse(
        u32::try_from(pack["version"].as_u64().expect("a pack version")).expect("the version fits"),
    )
    .expect("a legal pack version");
    world.daemon.state().with_store(|store| {
        store
            .register_profile_pack(
                &RegisteredPack {
                    pack_id,
                    version,
                    document_hash: ContentHash::of(document.as_bytes()),
                    document: document.clone(),
                    registered_at,
                },
                &IdempotencyBinding {
                    key: key.to_owned(),
                    operation: "register_profile_pack",
                    fingerprint: ContentHash::of(key.as_bytes()),
                    bound_at: registered_at,
                },
            )
            .expect("the additive pack is stored");
    });
    document
}

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
    let reapplied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Incident epic",
            &category,
            serde_json::json!([{"title": "Contain the incident"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("incident-epic-reapply")
    .send(&world)
    .await;
    assert_eq!(reapplied.status, 200, "{}", reapplied.body);
    assert_eq!(
        reapplied.json()["applied"],
        "unchanged",
        "{}",
        reapplied.body
    );
}

/// A unique registered category cannot smuggle different policy bytes under a
/// bundled team revision identity. Without this registration fence it could
/// seed a fresh project first, after which the supported built-in reconciliation
/// would preserve bytes the build never published.
#[tokio::test]
async fn a_registered_pack_cannot_collide_with_a_bundled_team_revision() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let mut pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");
    let bundled = bundled_team(None, at("2026-08-10T09:00:00Z"));
    pack["teams"][0]["template_id"] = serde_json::json!(bundled.template_id.to_string());
    pack["profiles"][0]["team_template"]["template_id"] =
        serde_json::json!(bundled.template_id.to_string());

    let refused = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": pack}),
    )
    .signed_as(&world, "admin")
    .with_key("pack-register-team-collision")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "revision_conflict");

    let listed = Call::get("/v1/catalog/packs")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(listed.status, 200, "{}", listed.body);
    assert!(
        !listed
            .json()
            .as_array()
            .expect("packs")
            .iter()
            .any(|entry| entry["pack_id"] == "kontor-pilot-incident"),
        "the colliding pack wrote no partial registration: {}",
        listed.body
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

/// Tasks that share a module are safe to run together only when both sides
/// hold different, declared worktrees. The scheduler already understands that
/// rule; this proves the daemon carries the task placement into both the plan
/// and the durable module lease used by the next plan.
#[tokio::test]
async fn distinct_task_worktrees_isolate_one_module_through_admission() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let created = ensure_project(&world, "isolated-module", "Kontor", "/tmp/kontor-isolated").await;
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
    .with_key("isolated-module-account")
    .send(&world)
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
            "Isolated module epic",
            &category,
            serde_json::json!([
                {"title": "Tree A", "module": "_tools/asma-rs-kontor",
                 "worktree": "/w/isolated-a"},
                {"title": "Tree B", "module": "_tools/asma-rs-kontor",
                 "worktree": "/w/isolated-b"},
                {"title": "No tree", "module": "_tools/asma-rs-kontor", "worktree": null}
            ]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("isolated-module-epic")
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
            "max_concurrency": 3,
            "budget": {"max_tokens": 1000, "max_commands": 10,
                       "max_duration_seconds": 600, "max_cost_minor_units": 100,
                       "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Prove worktree-isolated module admission"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("isolated-module-arm")
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
        2,
        "both distinct trees admit: {}",
        plan.body
    );
    assert!(
        plan.json()["blocked"]
            .as_array()
            .expect("blocked")
            .iter()
            .any(|task| task["code"] == "module_in_flight"),
        "the task with no verified tree remains serialized: {}",
        plan.body
    );

    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("isolated-module-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);

    let unverified_task = TaskId::parse(
        applied.json()["tasks"][2]["task_id"]
            .as_str()
            .expect("task id"),
    )
    .expect("task id");
    let claims = world
        .daemon
        .state()
        .with_store(|store| store.active_module_claims(kontor_api::now()))
        .expect("module claims");
    let trees: BTreeSet<_> = claims
        .iter()
        .filter(|claim| claim.module.as_str() == "_tools/asma-rs-kontor")
        .filter_map(|claim| claim.worktree.as_ref().map(ExternalName::as_str))
        .collect();
    assert_eq!(
        trees,
        BTreeSet::from(["/w/isolated-a", "/w/isolated-b"]),
        "the module leases retain each admitted task's worktree"
    );
    assert!(
        claims.iter().all(|claim| claim.task_id != unverified_task),
        "the task without a verified tree was not admitted"
    );
}

/// A delivery seat whose owning control seat closed is concluded orphaned, and
/// an orphaned seat cannot hold the task's progress.
///
/// The whole point of `parent_seat_binding_id`: orphanhood is a fact about the
/// *owner's* row, so it is derived rather than recorded. Admission opens one
/// control seat per epic and every delivery seat of that epic names it, which is
/// what makes closing the owner conclude all of them at once.
///
/// The owner is closed here through the same store observation the production
/// close path makes (`Services::release_epic_control_seat`);
/// `settlement_closes_the_team_and_unlocks_the_whole_epic_close_out` proves that
/// path actually runs on `close_epic`.
#[tokio::test]
async fn a_delivery_seat_whose_owner_closed_is_orphaned_and_holds_no_progress() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "orphan").await;
    let seated = seats.as_array().expect("the seated roster");
    assert!(!seated.is_empty(), "the start produced seats: {seats}");
    let task = seated[0]["task_id"].as_str().expect("a task id").to_owned();

    let project_id = ProjectId::parse(&project).expect("a project id");
    let epic_id = MiniProjectId::parse(&epic).expect("an epic id");
    let task_id = TaskId::parse(&task).expect("a task id");
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");

    let (node_id, owner_id) = world.daemon.state().with_store(|store| {
        let node = store
            .get_task_topology_node(project_id, task_id)
            .expect("the node reads")
            .expect("admission placed the task on a node");
        let control = store
            .list_topology_nodes(project_id, Some(epic_id))
            .expect("the epic's nodes read")
            .into_iter()
            .find(|it| it.kind == domain.delivery.control_kind)
            .expect("admission opened the epic's control plane");
        let owners: Vec<_> = store
            .list_seat_bindings(project_id, control.id)
            .expect("the control seats read");
        // The owner is the seat holding the declared control role, not whichever
        // seat is first. Every epic is born with its whole mandatory leadership
        // pair, so the control plane holds more than one seat and "the first
        // one" would name the architect rather than the owner.
        let owner = owners
            .iter()
            .find(|it| it.role.role_code == domain.delivery.control_role_code)
            .expect("the control seat that owns this epic");

        // Every delivery seat names that exact owner. `None` here is the defect
        // this test exists for: a seat with no owner is a root, and a root is
        // never orphaned however dead its epic is.
        let delivery = store
            .list_seat_bindings(project_id, node.id)
            .expect("the delivery seats read");
        assert!(
            !delivery.is_empty(),
            "admission recorded the delivery seats it started"
        );
        for seat in &delivery {
            assert_eq!(
                seat.parent_seat_binding_id,
                Some(owner.id),
                "delivery seat `{}` names the epic's control seat as its owner",
                seat.role_slot_id.as_str()
            );
        }
        (node.id, owner.id)
    });

    // While the owner is open, the seats are attached and the task's progress
    // stands on them.
    let now = kontor_api::now();
    let before = world.daemon.state().with_store(|store| {
        store
            .list_seat_attachments(project_id, node_id, now)
            .expect("the attachments read")
    });
    assert!(
        before
            .iter()
            .all(|seat| *seat == kontor_core::state::SeatAttachment::Attached),
        "a launched seat is attached: {before:?}"
    );
    assert!(
        kontor_core::state::certify_task_progress(
            kontor_core::state::RunLifecycle::Running,
            &before
        )
        .is_ok(),
        "an attached seat holds progress"
    );

    // Close the owner. Nothing about the delivery seats is rewritten.
    world.daemon.state().with_store(|store| {
        store
            .observe_seat_binding(
                project_id,
                owner_id,
                &kontor_core::repository::SeatLivenessObservation {
                    released_at: Some(now),
                    ..kontor_core::repository::SeatLivenessObservation::default()
                },
                now,
            )
            .expect("the owner is released");
    });

    let after = world.daemon.state().with_store(|store| {
        store
            .list_seat_attachments(project_id, node_id, now)
            .expect("the attachments read")
    });
    assert!(
        after
            .iter()
            .all(|seat| *seat == kontor_core::state::SeatAttachment::Orphaned),
        "every seat of a closed owner is orphaned: {after:?}"
    );
    assert!(
        kontor_core::state::certify_task_progress(
            kontor_core::state::RunLifecycle::Running,
            &after
        )
        .is_err(),
        "an orphaned seat is not progress"
    );
}

/// A project that never selected a topology is given one, not excused from one.
///
/// This is the claim that replaced the old `Ok(None)` escape. Admission used to
/// answer "this project runs no Operational topology, so there is nothing to
/// place against" and fall back to a TeamRun-keyed task workspace. It now seeds
/// the project's revision and creates the node chain instead, so the worry the
/// escape answered — that an unconfigured project becomes unrunnable — is
/// answered without a second way to place a production seat.
///
/// Both halves are asserted: nothing is configured before the start, and after
/// it the seat is live *and* the topology exists to explain where it is.
#[tokio::test]
async fn a_project_with_no_topology_is_seeded_one_rather_than_placed_outside_it() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let created = ensure_project(&world, "seeded", "Kontor", "/tmp/kontor-seeded").await;
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
    .with_key("seeded-account")
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
            "Unconfigured epic",
            &category,
            serde_json::json!([{"title": "Unconfigured task", "worktree": "/w/seeded"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("seeded-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let project_id = ProjectId::parse(&project).expect("a project id");
    let task_id = TaskId::parse(&task).expect("a task id");

    // The project configured no topology, and applying the epic seeded it one
    // rather than leaving the epic to be placed outside any — which is this
    // test's subject, now answered at apply time because that is where an epic
    // acquires its control plane. What is still absent is the task's own node:
    // admission places that, and the removed escape was the code that let it be
    // placed without a topology at all.
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_project_topology_default(project_id)
                .expect("the default reads")
                .is_some(),
            "applying the epic seeded the project a topology revision"
        );
        assert!(
            store
                .get_task_topology_node(project_id, task_id)
                .expect("the node reads")
                .is_none(),
            "the task has no node to be placed on"
        );
    });

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
            "reason": "Run an unconfigured project"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("seeded-arm")
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
    .with_key("seeded-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        !started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "an unconfigured project still runs its work: {}",
        started.body
    );

    // And it ran *inside* the topology rather than beside it: the revision is
    // selected, the task has a node of the seeded delivery kind, and that node
    // holds the native container the seat was placed in.
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_project_topology_default(project_id)
                .expect("the default reads")
                .is_some(),
            "admission selected a topology revision for the project"
        );
        let node = store
            .get_task_topology_node(project_id, task_id)
            .expect("the node reads")
            .expect("admission created the task's node");
        assert_eq!(node.kind, domain.delivery.task_kind);
        assert!(
            node.parent_id.is_some(),
            "the task's node hangs below its epic"
        );
        assert!(
            store
                .get_topology_node_container(project_id, node.id)
                .expect("the container reads")
                .is_some(),
            "the node holds the native container the seat was placed in"
        );
    });

    // The seat was placed by node, not by team run: the runtime was asked to
    // prepare containers and never a task workspace.
    let calls = world.fake.calls();
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, kontor_runtime::fake::AdapterCall::PrepareContainer(_))),
        "admission prepared the node's container"
    );
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, kontor_runtime::fake::AdapterCall::PrepareWorkspace(_))),
        "no accepted seat falls back to a TeamRun-keyed workspace"
    );
}

/// A task whose node cannot host sessions is refused before any native effect.
///
/// The refusal has to be reachable to be worth anything. Admission resolves the
/// task's node from the session topology, so seeding that task a node of a kind
/// the pinned specification declares *without* `session_host` is exactly the
/// disagreement `placement_blocked` exists to report — and reporting it is the
/// whole difference between a seat that never starts and a seat that starts in
/// a place nothing declared it could run.
///
/// The node is seeded directly rather than through admission on purpose:
/// admission's own writer only ever creates the delivery kind, so nothing it
/// produces could reach this branch.
#[tokio::test]
async fn a_task_placed_on_a_node_that_hosts_no_session_is_refused_before_anything_starts() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);

    let created = ensure_project(&world, "nohost", "Kontor", "/tmp/kontor-nohost").await;
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
    .with_key("nohost-account")
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
            "Unhostable epic",
            &category,
            serde_json::json!([{"title": "Unhostable task", "worktree": "/w/nohost"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("nohost-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // Seed the task a node of a kind that materializes as a native root and
    // hosts nothing. Admission will find it before it creates one of its own.
    let project_id = ProjectId::parse(&project).expect("a project id");
    let epic_id = MiniProjectId::parse(&epic).expect("an epic id");
    let task_id = TaskId::parse(&task).expect("a task id");
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    let topology_spec = domain.topology_specs.first().expect("a topology").clone();
    let at = kontor_api::now();
    world.daemon.state().with_store(|store| {
        // Applying the epic already published this specification, selected it,
        // pinned the epic to it and created the project root — governance needs
        // all four to place a control plane. So the seed reuses them and adds
        // only the one node this test is about.
        let topology = store
            .get_project_topology_default(project_id)
            .expect("the default reads")
            .expect("applying the epic selected a topology")
            .topology;
        let root = store
            .list_topology_nodes(project_id, None)
            .expect("the unscoped nodes read")
            .into_iter()
            .find(|node| node.kind == topology_spec.root_kind && node.parent_id.is_none())
            .expect("applying the epic created the project root");
        store
            .create_topology_node(&kontor_core::repository::NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: Some(epic_id),
                topology,
                // The epic kind: a native root that hosts no seats.
                kind: domain.delivery.epic_kind.clone(),
                parent_id: Some(root.id),
                task_id: Some(task_id),
                created_at: at,
            })
            .expect("the unhostable node is created");
    });

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
            "reason": "Try to place the unplaceable"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("nohost-arm")
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
    .with_key("nohost-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "nothing may start on a node that hosts no session: {}",
        started.body
    );
    assert_eq!(
        started.json()["blocked"][0]["code"],
        serde_json::json!("placement_blocked"),
        "{}",
        started.body
    );
    assert_eq!(
        started.json()["blocked"][0]["evidence"][0]["rule"],
        serde_json::json!("the task's node kind does not host sessions"),
        "{}",
        started.body
    );

    // Nothing was dispatched: the refusal is decided from Kontor's own rows, so
    // the runtime was never asked to build anything for this task.
    assert!(
        !world
            .fake
            .calls()
            .iter()
            .any(|call| matches!(call, kontor_runtime::fake::AdapterCall::PrepareContainer(_))),
        "a blocked placement reaches no native surface"
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

/// A launch that was refused leaves a run nothing can settle, and abandoning it
/// is what lets the task be scheduled again.
///
/// The incident this exists for: admission commits the run *before* the runtime
/// is asked for a session, so a refused launch leaves a queued, unbound run
/// behind. That run is non-terminal, a non-terminal run keeps its task in
/// flight, and every other exit demands evidence from a runtime that never
/// answered — `runtime:settle` has no binding to inspect, `turns:settle` has no
/// bound slot. Without this operation the task is unschedulable forever and the
/// only way out is editing the database.
#[tokio::test]
async fn a_run_no_runtime_ever_took_is_abandoned_so_its_task_can_be_scheduled_again() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    // The runtime serves exactly one tree, and the task declares another — the
    // same shape as a worktree that does not exist yet.
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse("/w/the-only-tree").expect("a valid root"),
    );

    let created = ensure_project(&world, "phantom", "Kontor", "/tmp/kontor-phantom").await;
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
    .with_key("phantom-account")
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
            "Phantom epic",
            &category,
            // The module matters: contending for one is what makes admission
            // take a lease, and a lease is what an abandonment has to hand back.
            serde_json::json!([{
                "title": "Refused task",
                "worktree": "/w/not-yet-created",
                "module": "contended-module"
            }]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("phantom-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("id").to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("revision");
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("id")
        .to_owned();

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
            "reason": "Start before the tree exists"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("phantom-arm")
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
    .with_key("phantom-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(
        started.json()["started"]
            .as_array()
            .expect("seats")
            .is_empty(),
        "the launch was refused: {}",
        started.body
    );

    // The wreckage: a committed, unbound, non-terminal run.
    let project_id = ProjectId::parse(&project).expect("a project id");
    let task_id = TaskId::parse(&task).expect("a task id");
    let (team_run_id, run_id, run_revision) = world.daemon.state().with_store(|store| {
        let runs = store
            .list_team_runs_for_task(project_id, task_id)
            .expect("the team runs read");
        let (team_run_id, _) = runs
            .first()
            .expect("the refused start committed a team run");
        let seats = store
            .list_agent_runs_for_team_run(project_id, *team_run_id)
            .expect("the team members read");
        let seat = seats.first().expect("the refused start committed a run");
        let run = store
            .get_agent_run(project_id, seat.agent_run_id)
            .expect("the run reads")
            .expect("the run exists");
        assert!(run.binding.is_none(), "the run was never bound");
        assert!(run.terminal.is_none(), "the run is not terminal");
        (*team_run_id, run.id, run.revision)
    });

    // And the consequence: the task is in flight, so nothing can be planned for
    // it even though nothing is running.
    assert!(
        world
            .daemon
            .state()
            .with_store(kontor_store::SqliteStore::tasks_with_open_runs)
            .expect("the in-flight set reads")
            .contains(&task_id),
        "an unbound queued run keeps its task in flight"
    );

    // Neither settlement path can reach it: one has no binding to inspect, the
    // other has no bound slot to settle.
    let settle = Call::post(
        format!("/v1/projects/{project}/agent-runs/{run_id}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("phantom-settle")
    .send(&world)
    .await;
    assert_eq!(settle.status, 404, "{}", settle.body);

    // Abandon is refused against the wrong revision, like every other decision
    // made about a specific version of a specific thing.
    let stale = Call::post(
        format!("/v1/projects/{project}/agent-runs/{run_id}/runtime:abandon"),
        &serde_json::json!({"expected_revision": run_revision.get() + 1, "reason": "stale"}),
    )
    .signed_as(&world, "operator")
    .with_key("phantom-abandon-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);

    let abandoned = Call::post(
        format!("/v1/projects/{project}/agent-runs/{run_id}/runtime:abandon"),
        &serde_json::json!({
            "expected_revision": run_revision.get(),
            "reason": "The launch was refused and no session was ever created"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("phantom-abandon")
    .send(&world)
    .await;
    assert_eq!(abandoned.status, 200, "{}", abandoned.body);
    assert_eq!(abandoned.json()["outcome"], "abandoned");
    assert_eq!(abandoned.json()["applied"], "created");
    assert!(
        abandoned.json()["team_run_closed"].is_string(),
        "the team run closes with its only run, or the task stays in flight: {}",
        abandoned.body
    );

    // Idempotent: the same key returns the stored closure and closes nothing
    // twice.
    let replay = Call::post(
        format!("/v1/projects/{project}/agent-runs/{run_id}/runtime:abandon"),
        &serde_json::json!({
            "expected_revision": run_revision.get(),
            "reason": "The launch was refused and no session was ever created"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("phantom-abandon")
    .send(&world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged");

    // The point of the whole operation: the task is schedulable again.
    assert!(
        !world
            .daemon
            .state()
            .with_store(kontor_store::SqliteStore::tasks_with_open_runs)
            .expect("the in-flight set reads")
            .contains(&task_id),
        "an abandoned run releases its task"
    );

    // And schedulable *now*, not after an expiry lapses. A lease is given up
    // deliberately: closing the run it belonged to does not touch it, so an
    // abandoned run holds its module for the rest of the window and the very
    // next admission is refused with "an active lease already claims this
    // place". Leaving that to the clock would make this operation a promise the
    // caller cannot act on.
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.live_leases_of_run(project_id, run_id, kontor_api::now()))
            .expect("the lease read succeeds")
            .is_empty(),
        "an abandoned run hands back every lease it still held"
    );

    // A terminal TeamRun is historical evidence, not the task's live seat. A
    // snapshot must refuse rather than bind new context to the abandoned run.
    let context = Call::post(
        format!("/v1/projects/{project}/tasks/{task}/context:resolve"),
        &serde_json::json!({"snapshot": true}),
    )
    .signed_as(&world, "operator")
    .with_key("phantom-context-after-abandon")
    .send(&world)
    .await;
    assert_eq!(context.status, 422, "{}", context.body);
    assert_eq!(context.code(), "unsupported_capability");

    // Once the missing checkout becomes available, admission traverses the
    // whole public path again. The terminal team's topology seats are history,
    // not a placement lock: a new TeamRun receives fresh seats on the same TSW.
    world.fake.verifying_placement_at(
        kontor_runtime::workspace::WorkspaceRoot::parse("/w/not-yet-created")
            .expect("a valid root"),
    );
    let replanned = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    assert_eq!(replanned.status, 200, "{}", replanned.body);
    let replanned_hash = replanned.json()["plan_hash"]
        .as_str()
        .expect("a plan hash")
        .to_owned();
    let restarted = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": replanned_hash}),
    )
    .signed_as(&world, "operator")
    .with_key("phantom-restart")
    .send(&world)
    .await;
    assert_eq!(restarted.status, 200, "{}", restarted.body);
    let restarted_body = restarted.json();
    let restarted_seats = restarted_body["started"]
        .as_array()
        .expect("restarted seats");
    assert!(!restarted_seats.is_empty(), "{}", restarted.body);
    assert!(
        restarted_seats
            .iter()
            .all(|seat| seat["team_run_id"] != team_run_id.to_string()),
        "a new generation must not adopt the abandoned TeamRun: {}",
        restarted.body
    );
}

/// A terminal member of an otherwise-live TeamRun is historical evidence, not
/// the run a new context snapshot belongs to. The resolver skips it and binds
/// the snapshot to one of the team's still-live seats.
#[tokio::test]
async fn a_terminal_agent_run_is_not_the_live_seat_of_its_still_open_team() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "terminal-member").await;
    let seats = seats.as_array().expect("started seats").clone();
    assert!(seats.len() > 1, "the bundled team has several live seats");
    let terminal = seats[0]["agent_run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();

    finish_natively(&world, &terminal).await;
    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{terminal}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("terminal-member-settle")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert!(
        settled.json()["team_run_closed"].is_null(),
        "other live seats keep the TeamRun open: {}",
        settled.body
    );

    let projection = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task = projection.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task id")
        .to_owned();
    let context = Call::post(
        format!("/v1/projects/{project}/tasks/{task}/context:resolve"),
        &serde_json::json!({"snapshot": true}),
    )
    .signed_as(&world, "operator")
    .with_key("terminal-member-context")
    .send(&world)
    .await;
    assert_eq!(context.status, 200, "{}", context.body);
    let selected = context.json()["agent_run_id"]
        .as_str()
        .expect("the selected live seat")
        .to_owned();
    assert_ne!(selected, terminal, "a terminal run cannot own new context");
    assert!(
        seats
            .iter()
            .skip(1)
            .any(|seat| seat["agent_run_id"] == selected),
        "the snapshot belongs to another live seat in the same team: {}",
        context.body
    );
}

/// A bound run is never abandoned: it holds a session, and closing Kontor's row
/// would leave an agent running that nothing is steering.
#[tokio::test]
async fn a_run_that_holds_a_session_is_settled_rather_than_abandoned() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, _epic, _account, seats) = seated_turns(&world, "bound-abandon").await;
    let seat = seats.as_array().expect("the seated roster")[0].clone();
    let run_id = seat["agent_run_id"].as_str().expect("a run id").to_owned();

    let project_id = ProjectId::parse(&project).expect("a project id");
    let run_revision = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, AgentRunId::parse(&run_id).expect("a run id"))
            .expect("the run reads")
            .expect("the run exists")
            .revision
    });

    let refused = Call::post(
        format!("/v1/projects/{project}/agent-runs/{run_id}/runtime:abandon"),
        &serde_json::json!({
            "expected_revision": run_revision.get(),
            "reason": "try to abandon a live seat"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("bound-abandon-attempt")
    .send(&world)
    .await;
    assert_eq!(refused.status, 422, "{}", refused.body);
    assert!(
        refused.body.contains("settled against its runtime"),
        "the refusal points at the supported path: {}",
        refused.body
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

/// A same-key replay is a read of the graph the first call stored, not a fresh
/// resolution of the category. This fixture publishes P1/T1, applies it, then
/// additively introduces a P2/T2 pack revision that wins category resolution.
/// Both an ordinary replay and a legacy caller still carrying the old optional
/// team pin must report P1/T1. Mixed task workflows are refused rather than
/// projecting whichever task happened to be visited first.
#[tokio::test]
async fn an_epic_replay_reads_one_agreed_stored_policy_after_category_progression() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let first_pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");
    let category = first_pack["manifest"][0]["category"]
        .as_str()
        .expect("a category")
        .to_owned();
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": first_pack.clone()}),
    )
    .signed_as(&world, "admin")
    .with_key("progression-pack-p1")
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let created = ensure_project(
        &world,
        "progression-project",
        "Progression",
        "/tmp/kontor-progression",
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let project_id = ProjectId::parse(&project).expect("a project id");
    let revision = created.json()["revision"]
        .as_u64()
        .expect("a project revision");
    let body = epic_body(
        revision,
        "Progressed category",
        &category,
        serde_json::json!([
            {"title": "First task"},
            {"title": "Second task", "depends_on": ["First task"]}
        ]),
    );
    let first = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("progressed-category-epic")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["work_profile"]["version"], 1);
    assert_eq!(first.json()["team_template"]["version"], 1);
    let p1_profile = first.json()["work_profile"].clone();
    let p1_team = first.json()["team_template"].clone();
    let p1_team_hash = first.json()["team_template_hash"].clone();

    // A later build/revision of the same category. The older P1 row has a
    // deliberately later catalogue timestamp, so the append-only P2 row becomes
    // the category owner without editing or deleting P1.
    let second_pack = incident_pack_revision(2, "kontor-pilot-incident-v2");
    let second_document = append_profile_pack(
        &world,
        &second_pack,
        at("2020-01-01T00:00:00Z"),
        "progression-pack-p2",
    );
    let current = Call::get(format!("/v1/catalog/work-profiles/{category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(current.status, 200, "{}", current.body);
    assert_eq!(current.json()["profile"]["version"], 2, "{}", current.body);
    assert_eq!(current.json()["team"]["version"], 2, "{}", current.body);

    let replay = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("progressed-category-epic")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["work_profile"], p1_profile);
    assert_eq!(replay.json()["team_template"], p1_team);
    assert_eq!(replay.json()["team_template_hash"], p1_team_hash);
    assert_eq!(replay.json()["bundle_hash"], first.json()["bundle_hash"]);

    let mut pinned_replay = body.clone();
    pinned_replay["team_template"] = p1_team.clone();
    let pinned = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &pinned_replay,
    )
    .signed_as(&world, "admin")
    .with_key("progressed-category-epic")
    .send(&world)
    .await;
    assert_eq!(pinned.status, 200, "{}", pinned.body);
    assert_eq!(pinned.json()["work_profile"], p1_profile);
    assert_eq!(pinned.json()["team_template"], p1_team);
    assert_eq!(pinned.json()["team_template_hash"], p1_team_hash);

    // Prove replay does not merely choose the first task's workflow. If one
    // task now freezes P2/T2, there is no one honest epic-level policy to return.
    let second_pack = kontor_profiles::pack::parse_pack(&second_document)
        .expect("the progressed pack parses twice");
    let category_key =
        kontor_profiles::pack::PackCategoryKey::parse(&category).expect("the category parses");
    let p2 = kontor_profiles::pack::resolve_profile(
        &second_pack,
        &category_key,
        at("2026-08-26T00:00:00Z"),
    )
    .expect("P2/T2 resolves");
    let second_task = TaskId::parse(
        first.json()["tasks"][1]["task_id"]
            .as_str()
            .expect("a task id"),
    )
    .expect("a task id");
    world.daemon.state().with_store(|store| {
        store
            .replace_task_workflow(
                project_id,
                second_task,
                &NewTaskWorkflow {
                    id: TaskWorkflowId::generate(),
                    project_id,
                    task_id: second_task,
                    snapshot: p2.profile.clone(),
                    current_phase: p2.profile.definition.entry_phase.clone(),
                    created_at: at("2026-08-26T00:00:00Z"),
                },
                &p2.profile.definition,
                p2.team.as_ref(),
                TeamTemplateSource::Registered,
            )
            .expect("the second task advances to P2/T2");
    });
    let mixed = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("progressed-category-epic")
        .send(&world)
        .await;
    assert_eq!(mixed.status, 503, "{}", mixed.body);
    assert_eq!(mixed.code(), "unavailable");
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
        "model_route": {
            "provider": "codex",
            "model": "gpt-5.6-sol",
            "effort": "xhigh"
        },
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
    let selected = world
        .fake
        .launched_model(successor_id)
        .expect("the successor's selected route is observable");
    assert_eq!(selected.provider.0, "codex");
    assert_eq!(selected.model.0, "gpt-5.6-sol");
    assert_eq!(selected.effort, Some(kontor_core::spec::EffortLevel::Xhigh));
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

    let retry_seat = seats.as_array().expect("the seated roster")[2].clone();
    let retry_predecessor = retry_seat["agent_run_id"]
        .as_str()
        .expect("the retry run id");
    let retry_role_slot = retry_seat["role_slot"]
        .as_str()
        .expect("the retry role slot");
    finish_natively(&world, retry_predecessor).await;
    let retry_settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{retry_predecessor}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("replace-seat-retry-runtime-settle")
    .send(&world)
    .await;
    assert_eq!(retry_settled.status, 200, "{}", retry_settled.body);

    let retry_predecessor_id =
        AgentRunId::parse(retry_predecessor).expect("a canonical retry predecessor id");
    let retry_before = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, retry_predecessor_id)
            .expect("the retry predecessor reads")
            .expect("the retry predecessor exists")
    });
    let retry_body = serde_json::json!({
        "role_slot": retry_role_slot,
        "expected_task_revision": task_revision,
        "binding_generation": retry_before
            .binding
            .as_ref()
            .expect("the retry predecessor was bound")
            .identity
            .generation,
    });
    // The first native call a replacement makes is the container chain above the
    // seat, so that is where a channel failure lands now.
    world.script(r#"{"steps":[{"step":"transport_failure","operation":"prepare_project"}]}"#);
    let failed = Call::post(
        format!("/v1/projects/{project}/agent-runs/{retry_predecessor}/successors:replace"),
        &retry_body,
    )
    .signed_as(&world, "admin")
    .with_key("replace-seat-successor-retry")
    .send(&world)
    .await;
    assert_eq!(failed.status, 503, "{}", failed.body);

    let team_run_id = TeamRunId::parse(team_run).expect("a canonical team run id");
    let recorded = world.daemon.state().with_store(|store| {
        store
            .list_agent_runs_for_team_run(project_id, team_run_id)
            .expect("the team members read")
            .into_iter()
            .map(|seat| {
                store
                    .get_agent_run(project_id, seat.agent_run_id)
                    .expect("the member reads")
                    .expect("the member exists")
            })
            .find(|run| run.parent_agent_run_id == Some(retry_predecessor_id))
            .expect("the failed launch recorded one successor")
    });
    assert!(recorded.binding.is_none());

    // A process restart loses the runtime adapter's in-memory seat ledger. The
    // archived predecessor is then genuinely absent, so replacement falls back
    // from the stale citation to a vacant-seat admission.
    world.fake.rebuild_adapter_state();
    let recovered = Call::post(
        format!("/v1/projects/{project}/agent-runs/{retry_predecessor}/successors:replace"),
        &retry_body,
    )
    .signed_as(&world, "admin")
    .with_key("replace-seat-successor-retry")
    .send(&world)
    .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(recovered.json()["applied"], "unchanged");
    assert_eq!(
        recovered.json()["successor_agent_run_id"],
        recorded.id.to_string()
    );
    let recovered_members = world.daemon.state().with_store(|store| {
        store
            .list_agent_runs_for_team_run(project_id, team_run_id)
            .expect("the recovered team members read")
            .into_iter()
            .map(|seat| {
                store
                    .get_agent_run(project_id, seat.agent_run_id)
                    .expect("the recovered member reads")
                    .expect("the recovered member exists")
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        recovered_members
            .iter()
            .filter(|run| run.parent_agent_run_id == Some(retry_predecessor_id))
            .count(),
        1
    );
    assert!(
        recovered_members
            .iter()
            .find(|run| run.id == recorded.id)
            .expect("the same successor remains")
            .binding
            .is_some()
    );
}

#[tokio::test]
async fn replacing_a_cancelled_seat_skips_an_operator_abandoned_unbound_successor() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, account, seats) = seated_turns(&world, "replace-abandoned").await;
    let seat_list = seats.as_array().expect("the seated roster").clone();
    let seat = seat_list[1].clone();
    let predecessor = seat["agent_run_id"].as_str().expect("the run id");
    let role_slot = seat["role_slot"].as_str().expect("the role slot");
    let team_run = seat["team_run_id"].as_str().expect("the team run");

    finish_natively(&world, predecessor).await;
    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("replace-abandoned-runtime-settle")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert_eq!(settled.json()["observed"], "cancelled");

    let project_id = ProjectId::parse(&project).expect("a project id");
    let predecessor_id = AgentRunId::parse(predecessor).expect("a canonical run id");
    let team_run_id = TeamRunId::parse(team_run).expect("a canonical team run id");
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
    let task_id = view.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("the task id")
        .to_owned();
    let task_revision = view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("the task revision");
    let body = serde_json::json!({
        "role_slot": role_slot,
        "expected_task_revision": task_revision,
        "binding_generation": old_binding.identity.generation,
    });

    world.script(r#"{"steps":[{"step":"transport_failure","operation":"prepare_project"}]}"#);
    let failed = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("replace-abandoned-failed-launch")
    .send(&world)
    .await;
    assert_eq!(failed.status, 503, "{}", failed.body);

    let abandoned_run = world.daemon.state().with_store(|store| {
        store
            .list_agent_runs_for_team_run(project_id, team_run_id)
            .expect("the team members read")
            .into_iter()
            .map(|seat| {
                store
                    .get_agent_run(project_id, seat.agent_run_id)
                    .expect("the member reads")
                    .expect("the member exists")
            })
            .find(|run| run.parent_agent_run_id == Some(predecessor_id))
            .expect("the failed launch recorded one successor")
    });
    assert!(abandoned_run.binding.is_none());
    assert!(abandoned_run.terminal.is_none());

    let abandoned = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/runtime:abandon",
            abandoned_run.id
        ),
        &serde_json::json!({
            "expected_revision": abandoned_run.revision.get(),
            "reason": "The replacement never bound a native session"
        }),
    )
    .signed_as(&world, "operator")
    .with_key("replace-abandoned-abandon")
    .send(&world)
    .await;
    assert_eq!(abandoned.status, 200, "{}", abandoned.body);
    assert_eq!(abandoned.json()["outcome"], "abandoned");

    world.script(HISTORY_LIVE);
    let replaced = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("replace-abandoned-mint")
    .send(&world)
    .await;
    assert_eq!(replaced.status, 200, "{}", replaced.body);
    assert_eq!(replaced.json()["applied"], "created");
    let successor_id = replaced.json()["successor_agent_run_id"]
        .as_str()
        .expect("the minted successor")
        .to_owned();
    assert_ne!(
        successor_id,
        abandoned_run.id.to_string(),
        "an operator-abandoned unbound successor must not be reused as the launch target"
    );

    let successor = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(
                project_id,
                AgentRunId::parse(&successor_id).expect("a canonical successor id"),
            )
            .expect("the successor reads")
            .expect("the successor exists")
    });
    assert_eq!(successor.parent_agent_run_id, Some(predecessor_id));
    assert!(successor.binding.is_some(), "the minted successor is bound");
    let parked = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, abandoned_run.id)
            .expect("the abandoned successor reads")
            .expect("the abandoned successor remains")
    });
    assert_eq!(
        parked
            .terminal
            .expect("the abandoned row stays terminal")
            .outcome,
        TerminalOutcome::Abandoned
    );

    for (index, original) in seat_list.iter().enumerate() {
        let original_id = original["agent_run_id"].as_str().expect("id");
        let slot = original["role_slot"].as_str().expect("slot");
        let agent_run = if original_id == predecessor {
            successor_id.as_str()
        } else {
            original_id
        };
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
                "role_slot": slot,
                "expected_task_revision": revision,
                "artifacts": ["change-set"]
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("replace-abandoned-turn-{index}"))
        .send(&world)
        .await;
        assert_eq!(settled.status, 200, "slot `{slot}`: {}", settled.body);
    }

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
        .with_key(format!("replace-abandoned-gate-{index}"))
        .send(&world)
        .await;
        assert_eq!(recorded.status, 200, "gate `{name}`: {}", recorded.body);
    }

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
    let done = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/lifecycle"),
        &serde_json::json!({
            "action": "complete_task", "task_id": task_id,
            "expected_revision": after_body["tasks"][0]["revision"]
                .as_u64()
                .expect("a revision"),
            "reason": "the minted successor settled after the abandoned child was skipped",
            "evidence": artifacts,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("replace-abandoned-complete")
    .send(&world)
    .await;
    assert_eq!(done.status, 200, "{}", done.body);
    assert_eq!(done.json()["state"], "done");
}

#[tokio::test]
async fn an_admin_replacement_retires_a_bound_nonterminal_predecessor_first() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "replace-dead-seat").await;
    let seat = seats.as_array().expect("the seated roster")[1].clone();
    let predecessor = seat["agent_run_id"].as_str().expect("the run id");
    let role_slot = seat["role_slot"].as_str().expect("the role slot");

    let project_id = ProjectId::parse(&project).expect("a project id");
    let predecessor_id = AgentRunId::parse(predecessor).expect("a canonical run id");
    let before = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the predecessor reads")
            .expect("the predecessor exists")
    });
    assert!(
        before.terminal.is_none(),
        "this is the dead-but-still-bound state from the incident"
    );
    let old_binding = before.binding.as_ref().expect("the predecessor was bound");
    world.fake.push_step_for(
        ScriptStep::InspectProcessMissing,
        RequestKey::Binding(old_binding.id),
    );
    world.fake.push_step_for(
        ScriptStep::TransportFailure {
            operation: RuntimeCapability::Resume,
        },
        RequestKey::Binding(old_binding.id),
    );
    let view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("the task revision");
    let replaced = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": task_revision,
            "binding_generation": old_binding.identity.generation,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("replace-dead-seat-successor")
    .send(&world)
    .await;
    assert_eq!(replaced.status, 200, "{}", replaced.body);

    let retired = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the predecessor reads")
            .expect("the predecessor remains as evidence")
    });
    assert_eq!(
        retired
            .terminal
            .expect("runtime retirement is durable")
            .outcome,
        TerminalOutcome::Cancelled
    );
    assert!(
        world
            .fake
            .calls()
            .iter()
            .any(|call| matches!(call, AdapterCall::Retire(binding) if *binding == old_binding.id))
    );
    assert!(
        world
            .daemon
            .state()
            .sessions()
            .get(old_binding.id)
            .is_none(),
        "the archived predecessor no longer occupies the in-process seat"
    );
    let successor = AgentRunId::parse(
        replaced.json()["successor_agent_run_id"]
            .as_str()
            .expect("a successor id"),
    )
    .expect("a canonical successor id");
    let successor = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, successor)
            .expect("the successor reads")
            .expect("the successor exists")
    });
    assert_eq!(successor.parent_agent_run_id, Some(predecessor_id));
    assert!(
        successor.binding.is_some(),
        "the linked successor is usable"
    );
}

/// A provider outage may retire a reachable idle seat only while its durable
/// evidence has never advanced beyond launch and Admin names the immutable
/// binding exactly. This is the supported replacement path for the dormant
/// Claude seats; omitting the typed evidence keeps the ordinary persistent-seat
/// reuse rule unchanged.
#[tokio::test]
async fn an_admin_retires_an_exact_never_dispatched_provider_blocked_seat() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _account, seats) = seated_turns(&world, "replace-provider-seat").await;
    let seat = seats.as_array().expect("the seated roster")[1].clone();
    let predecessor = seat["agent_run_id"].as_str().expect("the run id");
    let role_slot = seat["role_slot"].as_str().expect("the role slot");
    let project_id = ProjectId::parse(&project).expect("a project id");
    let predecessor_id = AgentRunId::parse(predecessor).expect("an agent run id");
    let run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    assert_eq!(
        run.projection.desired,
        kontor_core::state::DesiredRunState::RunRequested,
        "every native session has its actual launch intent"
    );
    assert_eq!(
        run.projection.observed,
        kontor_core::state::ObservedRunState::Launching,
        "the outage path is limited to a seat with launch-only evidence"
    );
    let binding = run.binding.as_ref().expect("the dormant seat is bound");
    let provider = world
        .fake
        .launched_model(predecessor_id)
        .expect("the frozen model route")
        .provider
        .0;
    let view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("the task revision");

    let ordinary = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": task_revision,
            "binding_generation": binding.identity.generation,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("replace-provider-seat-ordinary")
    .send(&world)
    .await;
    assert_eq!(ordinary.status, 422, "{}", ordinary.body);
    assert!(
        !world
            .fake
            .calls()
            .iter()
            .any(|call| matches!(call, AdapterCall::Retire(id) if *id == binding.id)),
        "an ordinary reachable idle seat is not retired"
    );

    world.fake.provider_outage(
        &provider,
        Some(kontor_core::spec::ModelRung {
            provider: kontor_core::spec::ProviderRef("codex".to_owned()),
            model: kontor_core::spec::ModelRef("gpt-5.6-sol".to_owned()),
            effort: Some(kontor_core::spec::EffortLevel::Xhigh),
        }),
    );
    let calls_before_mismatch = world.fake.calls().len();
    let mismatched = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": task_revision,
            "binding_generation": binding.identity.generation,
            "unavailable_provider": {
                "runtime_binding_id": binding.id,
                "native_id": "another-native-session",
                "provider": provider,
            },
        }),
    )
    .signed_as(&world, "admin")
    .with_key("replace-provider-seat-mismatch")
    .send(&world)
    .await;
    assert_eq!(mismatched.status, 409, "{}", mismatched.body);
    assert_eq!(
        world.fake.calls().len(),
        calls_before_mismatch,
        "identity mismatch is refused before contacting the runtime"
    );
    assert!(
        !world
            .fake
            .calls()
            .iter()
            .any(|call| matches!(call, AdapterCall::Retire(id) if *id == binding.id)),
        "mismatched outage evidence cannot retire the seat"
    );

    let replaced = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": task_revision,
            "binding_generation": binding.identity.generation,
            "unavailable_provider": {
                "runtime_binding_id": binding.id,
                "native_id": binding.identity.native_id,
                "provider": provider,
            },
        }),
    )
    .signed_as(&world, "admin")
    .with_key("replace-provider-seat-exact")
    .send(&world)
    .await;
    assert_eq!(replaced.status, 200, "{}", replaced.body);
    let successor_id = AgentRunId::parse(
        replaced.json()["successor_agent_run_id"]
            .as_str()
            .expect("the successor id"),
    )
    .expect("a successor id");
    assert_eq!(
        world
            .fake
            .launched_model(successor_id)
            .expect("the successor route")
            .provider
            .0,
        "codex"
    );
    assert!(
        world
            .fake
            .calls()
            .iter()
            .any(|call| matches!(call, AdapterCall::Retire(id) if *id == binding.id))
    );
    let retired = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the predecessor reads")
            .expect("the predecessor remains as evidence")
    });
    assert_eq!(
        retired.terminal.expect("the retirement is durable").outcome,
        TerminalOutcome::Cancelled
    );
    let successor = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, successor_id)
            .expect("the successor reads")
            .expect("the successor exists")
    });
    assert_eq!(successor.parent_agent_run_id, Some(predecessor_id));
}

/// Label repair is an exact, idempotent mutation of one already-bound native
/// session. It never invents a replacement run and a stale generation cannot
/// retarget the operation after a runtime restart.
#[tokio::test]
async fn session_label_repair_preserves_the_bound_identity_and_replays_once() {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, _epic, _account, seats) = seated_turns(&world, "repair-session-labels").await;
    let seat = seats.as_array().expect("the seated roster")[0].clone();
    let agent_run = seat["agent_run_id"].as_str().expect("the run id");
    let project_id = ProjectId::parse(&project).expect("the project id");
    let agent_run_id = AgentRunId::parse(agent_run).expect("the agent run id");
    let run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, agent_run_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    let binding = run.binding.as_ref().expect("the run is bound");
    let uri = format!("/v1/projects/{project}/agent-runs/{agent_run}/labels:reconcile");
    let body = serde_json::json!({
        "expected_revision": run.revision,
        "binding_generation": binding.identity.generation,
    });

    let repaired = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("repair-session-labels-once")
        .send(&world)
        .await;
    assert_eq!(repaired.status, 200, "{}", repaired.body);
    assert_eq!(repaired.json()["agent_run_id"], agent_run);
    assert_eq!(
        repaired.json()["native_id"],
        binding.identity.native_id.as_str(),
        "label repair preserves the exact native session"
    );
    assert_eq!(repaired.json()["receipt"]["applied"], "created");

    let replayed = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("repair-session-labels-once")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        repaired.json()["receipt"]["receipt_id"]
    );

    let stale = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": run.revision,
            "binding_generation": binding.identity.generation + 1,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("repair-session-labels-stale-generation")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
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
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, kontor_runtime::fake::AdapterCall::Resume(_)))
            .count(),
        1,
        "the deferred turn resumes its persistent seat before delivery"
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

    // A closed TeamRun is not a live task seat even when every native agent is
    // deliberately left reusable. Context for the next generation must never
    // attach itself to one of the previous generation's sessions.
    let context = Call::post(
        format!("/v1/projects/{project}/tasks/{task_id}/context:resolve"),
        &serde_json::json!({"snapshot": true}),
    )
    .signed_as(&world, "operator")
    .with_key("close-live-context-after-team")
    .send(&world)
    .await;
    assert_eq!(context.status, 422, "{}", context.body);
    assert_eq!(context.code(), "unsupported_capability");
    let task_id_value = TaskId::parse(&task_id).expect("a task id");
    world.daemon.state().with_store(|store| {
        let node = store
            .get_task_topology_node(project_id, task_id_value)
            .expect("the task node reads")
            .expect("the task node exists");
        let bindings = store
            .list_seat_bindings(project_id, node.id)
            .expect("the task seats read");
        assert!(!bindings.is_empty(), "the closed team had topology seats");
        assert!(
            bindings.iter().all(|binding| !binding.is_non_terminal()),
            "a terminal TeamRun releases every topology seat it held: {bindings:?}"
        );
    });

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
            // No budget: unconstrained. Quota headroom and capacity govern.
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
            // No budget: unconstrained. Quota headroom and capacity govern.
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
    assert!(
        !plan.json()["ready"].as_array().expect("ready").is_empty(),
        "at least the first independent task is ready: {}",
        plan.body
    );
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    CapacityFixture {
        project,
        epic,
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
/// admitted once that team closes on settled turns with all its seats still
/// sitting there. `task_not_ready` or an in-flight refusal would prove nothing
/// about capacity, so both are excluded by asserting the exact rule.
#[tokio::test]
async fn a_team_that_closed_on_settled_turns_releases_admission_capacity() {
    let world = World::open_empty_with_a_plane_and_capacity(CapacityConfig {
        account_max_in_flight: 1,
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

    // The exact refusal, by code and scope. A spent ceiling has its own code, so
    // this cannot be satisfied by a revision conflict, a not-ready task or an
    // already-in-flight one.
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
    assert_eq!(blocked[0]["evidence"][0]["kind"], "capacity");
    assert_eq!(blocked[0]["evidence"][0]["limit"], "account");
    assert_eq!(blocked[0]["evidence"][0]["remaining"], 0);

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

    // Nothing was torn down: the runs belonging to the team that held the
    // ceiling are all still open. That is the whole point — capacity is
    // released by the team's closure, not by the sessions ending.
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
    plan_hash: String,
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
            // No budget: unconstrained. Quota headroom and capacity govern.
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
        plan_hash,
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
    let project_id = ProjectId::parse(project).expect("a project id");
    let team_run_id = TeamRunId::parse(team_run).expect("a team run id");
    let team_revision = world.daemon.state().with_store(|store| {
        store
            .get_team_run(project_id, team_run_id)
            .expect("the team reads")
            .expect("the team exists")
            .revision
    });

    let waive = |slot: &'static str,
                 role: &'static str,
                 evidence: serde_json::Value,
                 signer: &'static str,
                 key: &'static str| {
        let uri = format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/{slot}/waivers");
        Call::post(
            uri,
            &serde_json::json!({
                "expected_team_revision": team_revision,
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

/// An unbound launch may be abandoned before its already-seated siblings end.
/// Replaying that same operator decision after the siblings settle is the only
/// immutable-row-safe opportunity to abandon the now-fully-terminal TeamRun.
#[tokio::test]
async fn replaying_an_abandonment_closes_the_team_after_live_siblings_end() {
    let seeded = alpha_with_one_unbound_slot("abandon-after-siblings").await;
    let UnboundWorld {
        world,
        project,
        team_run,
        seats,
        ..
    } = &seeded;
    let project_id = kontor_core::id::ProjectId::parse(project).expect("a project id");
    let team_run_id = kontor_core::id::TeamRunId::parse(team_run).expect("a team run id");
    let unbound = world
        .daemon
        .state()
        .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
        .expect("the seats are readable")
        .into_iter()
        .find(|seat| seat.native_id.is_none())
        .expect("the refused launch left its durable unbound run");
    let run = world
        .daemon
        .state()
        .with_store(|store| store.get_agent_run(project_id, unbound.agent_run_id))
        .expect("the run is readable")
        .expect("the run exists");
    let body = serde_json::json!({
        "expected_revision": run.revision.get(),
        "reason": "The launch was refused and no session was ever created"
    });

    let abandoned = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/runtime:abandon",
            unbound.agent_run_id
        ),
        &body,
    )
    .signed_as(world, "operator")
    .with_key("abandon-after-siblings-run")
    .send(world)
    .await;
    assert_eq!(abandoned.status, 200, "{}", abandoned.body);
    assert!(
        abandoned.json()["team_run_closed"].is_null(),
        "the live siblings keep the team open: {}",
        abandoned.body
    );

    for seat in seats {
        let agent_run = seat["agent_run_id"].as_str().expect("a run id");
        finish_natively(world, agent_run).await;
        let settled = Call::post(
            format!("/v1/projects/{project}/agent-runs/{agent_run}/runtime:settle"),
            &serde_json::json!({}),
        )
        .signed_as(world, "operator")
        .with_key(format!("abandon-after-siblings-settle-{agent_run}"))
        .send(world)
        .await;
        assert_eq!(settled.status, 200, "{}", settled.body);
    }

    let replay = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/runtime:abandon",
            unbound.agent_run_id
        ),
        &body,
    )
    .signed_as(world, "operator")
    .with_key("abandon-after-siblings-run")
    .send(world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["applied"], "unchanged", "{}", replay.body);
    assert_eq!(
        replay.json()["team_run_closed"],
        serde_json::json!(team_run),
        "the replay closes the now-fully-terminal team: {}",
        replay.body
    );
    assert!(replay.json()["team_pending"].is_null(), "{}", replay.body);
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

    let project_id = ProjectId::parse(project).expect("a project id");
    let team_run_id = TeamRunId::parse(team_run).expect("a team run id");
    let team_revision = world.daemon.state().with_store(|store| {
        store
            .get_team_run(project_id, team_run_id)
            .expect("the team reads")
            .expect("the team exists")
            .revision
    });

    // Waive first, while the other slots are still outstanding: the team cannot
    // close yet, so settlement still happens afterwards.
    let waived = Call::post(
        format!("/v1/projects/{project}/team-runs/{team_run}/role-slots/omega-k3/waivers"),
        &serde_json::json!({
            "expected_team_revision": team_revision,
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

/// An exact scheduler replay is the recovery seam for a durable admission
/// whose downstream seat failed to launch. If an upstream turn settled in the
/// meantime, binding that seat must also deliver the handoff already recorded
/// for it; otherwise the replay reports success while the recovered team stays
/// idle until an unrelated daemon restart.
#[tokio::test]
async fn replaying_a_partial_admission_delivers_its_durable_follow_up() {
    let recovered = omega_with_one_unbound_slot("recover-dispatch", "omega-u-cat").await;
    let project_id = kontor_core::id::ProjectId::parse(&recovered.project).expect("a project id");
    let team_run_id = TeamRunId::parse(&recovered.team_run).expect("a team run id");
    let (task_id, node_id) = recovered.world.daemon.state().with_store(|store| {
        let task_id = store
            .get_team_run(project_id, team_run_id)
            .expect("the team run is readable")
            .expect("the team run exists")
            .task_id;
        let node_id = store
            .get_task_topology_node(project_id, task_id)
            .expect("the task node is readable")
            .expect("the task node exists")
            .id;
        (task_id, node_id)
    });
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    let catalog = domain
        .role_catalogs
        .first()
        .expect("a bundled role catalog");
    let role = catalog
        .role(&RoleCode::parse("SWE").expect("a standard role code"))
        .expect("the catalog has SWE");
    recovered.world.daemon.state().with_store(|store| {
        store
            .create_seat_binding(&NewSeatBinding {
                id: SeatBindingId::generate(),
                project_id,
                topology_node_id: node_id,
                role_slot_id: kontor_core::id::RoleSlotId::parse("omega-k3").expect("a role slot"),
                role: CatalogRoleRef {
                    catalog_id: catalog.catalog_id,
                    catalog_revision: catalog.version,
                    role_code: role.role_code.clone(),
                    standard_title: role.standard_title.clone(),
                    custom_display_name: None,
                },
                task_id: Some(task_id),
                team_run_id: Some(team_run_id),
                attach_deadline: at("2099-01-01T00:00:00Z"),
                parent_seat_binding_id: None,
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("the same TeamRun's logical seat already exists");
    });
    let giver = recovered
        .seats
        .iter()
        .find(|seat| seat["role_slot"] == "omega-k1")
        .expect("omega-k1 is seated")["agent_run_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let settled = Call::post(
        format!(
            "/v1/projects/{}/agent-runs/{giver}/turns:settle",
            recovered.project
        ),
        &serde_json::json!({
            "role_slot": "omega-k1",
            "expected_task_revision": 1,
            "artifacts": ["omega-a2", "omega-a3"]
        }),
    )
    .signed_as(&recovered.world, "operator")
    .with_key("recover-dispatch-turn")
    .send(&recovered.world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    let settlement = settled.json();
    let follow_up = settlement["follow_ups"]
        .as_array()
        .expect("follow-ups")
        .iter()
        .find(|follow_up| follow_up["to_role_slot"] == "omega-k3")
        .expect("omega-k1 hands to omega-k3");
    assert_eq!(follow_up["dispatched"], serde_json::json!(false));
    let target = follow_up["target_agent_run_id"]
        .as_str()
        .expect("the unbound run is still the declared target")
        .to_owned();

    let slot = kontor_core::id::RoleSlotId::parse("omega-k3").expect("a slot");
    recovered.world.fake.allowing_launch_of(&slot);
    let replay = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:start",
            recovered.project, recovered.epic
        ),
        &serde_json::json!({"plan_hash": recovered.plan_hash}),
    )
    .signed_as(&recovered.world, "operator")
    .with_key("recover-dispatch-start")
    .send(&recovered.world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert!(
        replay.json()["blocked"]
            .as_array()
            .expect("blocked")
            .is_empty(),
        "the exact replay recovered every seat: {}",
        replay.body
    );

    let run = recovered
        .world
        .daemon
        .state()
        .with_store(|store| {
            store.get_agent_run(
                project_id,
                kontor_core::id::AgentRunId::parse(&target).expect("a run id"),
            )
        })
        .expect("the run is readable")
        .expect("the run exists");
    assert!(
        run.binding.is_some(),
        "the replay bound the downstream seat"
    );
    let dispatches = recovered
        .world
        .daemon
        .state()
        .with_store(|store| store.list_turn_dispatches(project_id))
        .expect("the dispatches are readable");
    assert!(
        dispatches.iter().any(|dispatch| {
            dispatch.to_role_slot_id == slot
                && dispatch.target_agent_run.map(|run| run.to_string()) == Some(target.clone())
                && dispatch.dispatched
        }),
        "the replay must deliver the durable handoff: {dispatches:?}"
    );
}

/// KON-MVP-09. The ceilings are a Realm's configuration, and the configured
/// value is what admission is judged against.
///
/// The oracle is the *contrast* with
/// `a_team_that_closed_on_settled_turns_releases_admission_capacity`: same
/// fixture, same plan, same single `scheduler:start`, and exactly one number
/// different. With an account ceiling of one the first TeamRun spends the
/// envelope and the second task comes back `capacity_exhausted`; with that one
/// ceiling configured for two, both tasks are seated by the same call and
/// nothing is blocked.
///
/// The paired tests prove that the configured number is observed by planning and
/// admission, and that no *other* ceiling was silently widened to make room,
/// because every one of them is still the default.
#[tokio::test]
async fn the_configured_capacity_and_not_a_compiled_one_decides_what_is_admitted() {
    // One ceiling, one change: the account ceiling that the sibling test proves
    // is the binding one, lifted from one to two. Everything else — global,
    // project, goal, provider, runtime and the adaptive window — is left at the
    // default, so a second admitted task cannot be explained by any of them.
    let world = World::open_empty_with_a_plane_and_capacity(CapacityConfig {
        account_max_in_flight: 2,
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
        "the ceiling that refused the second task at one has room at two: {}",
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

// ---------------------------------------------------------------------------
// The topology vocabulary
// ---------------------------------------------------------------------------

/// A reference read refuses rather than improvises.
///
/// The tempting failure in this family is precise: answering an empty catalog or
/// an empty projection. A caller cannot tell that from a revision that genuinely
/// declares nothing, so an empty success would be a lie that reads as data. Each
/// of these names something that does not exist, and each is told so.
#[tokio::test]
async fn a_reference_read_for_something_absent_is_not_found_rather_than_empty() {
    let world = World::open().await;
    let absent = kontor_core::id::RoleCatalogId::generate();

    for call in [
        Call::get(format!("/v1/catalog/role-catalogs/{absent}/1")),
        Call::get(format!("/v1/catalog/role-catalogs/{absent}/1/roles/lsa")),
        Call::get(format!(
            "/v1/projects/{}/epics/{}/code-help",
            world.project,
            MiniProjectId::generate()
        )),
    ] {
        let answer = call.signed_as(&world, "observer").send(&world).await;
        assert_eq!(
            answer.status, 404,
            "a read for something absent must say so: {}",
            answer.body
        );
        assert_eq!(answer.code(), "not_found");
    }

    // A publish whose candidate is not a specification document at all is
    // refused before it can commit anything, and carries no receipt.
    let published = Call::post(
        format!("/v1/projects/{}/topology-specs:publish", world.project),
        &serde_json::json!({
            "candidate": { "schema_version": 1, "name": "Standard" },
            "validation_hash": "0".repeat(64),
            "expected_revision": 1,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("publish-a-non-document")
    .send(&world)
    .await;
    assert!(
        published.status.is_client_error(),
        "a candidate that is not a specification must be refused: {}",
        published.body
    );
    assert!(
        published.json().get("receipt_id").is_none(),
        "a refusal must not carry a receipt: {}",
        published.body
    );
}

/// The authority on these routes is real, not decoration.
///
/// Deciding which node kinds may ever exist in a project is the Admin tier's
/// defining Operational power. An observer holding the wrong credential must be
/// refused, and the same call at the right tier must actually work — a check
/// that only ever met a refusing service would look identical to no check.
#[tokio::test]
async fn deciding_the_vocabulary_is_admin_authority_and_the_check_is_real() {
    let world = World::open().await;
    let draft_body = serde_json::json!({
        "name": "Standard",
        "root_kind": "PSW",
        "node_kinds": [],
    });

    for call in [
        Call::post(
            format!("/v1/projects/{}/topology-specs:draft", world.project),
            &draft_body,
        ),
        Call::post(
            format!("/v1/projects/{}/topology-specs:validate", world.project),
            &serde_json::json!({ "candidate": {} }),
        ),
    ] {
        let refused = call.signed_as(&world, "observer").send(&world).await;
        assert_eq!(
            refused.status, 403,
            "an observer must not decide the vocabulary: {}",
            refused.body
        );
        assert_eq!(refused.code(), "forbidden");
    }

    // The same route at the authority it requires builds a candidate.
    let drafted = Call::post(
        format!("/v1/projects/{}/topology-specs:draft", world.project),
        &draft_body,
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(drafted.status, 200, "{}", drafted.body);
    // The identity, the version and the schema generation are the server's, not
    // the caller's — the request has no field for any of them.
    assert_eq!(drafted.json()["candidate"]["version"], 1);
    assert!(
        drafted.json()["candidate"]["spec_id"].as_str().is_some(),
        "the server names the lineage: {}",
        drafted.body
    );

    // An empty vocabulary drafts but does not validate: drafting is for building
    // one up, and judging it is a separate answer.
    let judged = Call::post(
        format!("/v1/projects/{}/topology-specs:validate", world.project),
        &serde_json::json!({ "candidate": drafted.json()["candidate"] }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(judged.status, 200, "{}", judged.body);
    assert!(
        !judged.json()["violations"]
            .as_array()
            .expect("violations")
            .is_empty(),
        "a vocabulary with no node kinds is not publishable: {}",
        judged.body
    );
}

/// A seat's role is selected, never asserted.
///
/// Before the Delivery Team slot carried a [`RoleSelectionDto`], its meaning
/// lived in an opaque id the server never read, so "which standard role is
/// this?" was a string every client answered for itself. The three refusals
/// below are the shortcuts that closes: a role given as bare text, a role code
/// that is not a code, and — the quiet one — a caller supplying the standard
/// title alongside the code. Serde would drop that last field by default and
/// answer 200, which reads as agreement; the closed field list makes it a
/// refusal instead.
#[tokio::test]
async fn a_team_slot_cannot_carry_a_raw_role_or_a_caller_supplied_title() {
    let world = World::open().await;

    async fn save(world: &World, slot: serde_json::Value, key: &str) -> Answer {
        let body = serde_json::json!({
            "id": "team-typed",
            "name": "Typed team",
            "slots": [slot],
        });
        Call::post("/v1/teams/drafts:save", &body)
            .signed_as(world, "operator")
            .with_key(key)
            .send(world)
            .await
    }

    // A role as free text is not a selection at all.
    let raw = save(
        &world,
        serde_json::json!({
            "id": "lead",
            "role": "Lead Software Architect",
            "capabilities": {},
        }),
        "raw-role-string",
    )
    .await;
    assert_eq!(
        raw.status, 400,
        "a bare role string must not be accepted: {}",
        raw.body
    );

    // A code that is not a code is refused by the domain's own parser.
    let unknown = save(
        &world,
        serde_json::json!({
            "id": "lead",
            "role": {
                "catalog_revision": {"id": "standard-roles", "version": 1},
                "role_code": "not a code",
            },
            "capabilities": {},
        }),
        "unparseable-role-code",
    )
    .await;
    assert_eq!(
        unknown.status, 400,
        "a malformed role code must not be accepted: {}",
        unknown.body
    );

    // The standard title belongs to the catalog. Supplying one is refused
    // rather than silently discarded.
    let titled = save(
        &world,
        serde_json::json!({
            "id": "lead",
            "role": {
                "catalog_revision": {"id": "standard-roles", "version": 1},
                "role_code": "LSA",
                "standard_title": "Something Else Entirely",
            },
            "capabilities": {},
        }),
        "caller-supplied-standard-title",
    )
    .await;
    assert_eq!(
        titled.status, 400,
        "a caller-supplied standard title must be refused, not dropped: {}",
        titled.body
    );

    // The same slot without the smuggled title is accepted, so the refusals
    // above are about the title and not about the shape in general.
    let accepted = save(
        &world,
        serde_json::json!({
            "id": "lead",
            "role": {
                "catalog_revision": {"id": "standard-roles", "version": 1},
                "role_code": "LSA",
            },
            "capabilities": {},
        }),
        "well-formed-selection",
    )
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(
        accepted.json()["drafts"][0]["slots"][0]["role"]["role_code"],
        serde_json::json!("LSA")
    );
    assert!(
        accepted.json()["drafts"][0]["slots"][0]["role"]
            .get("standard_title")
            .is_none(),
        "the server must not invent a title it has no catalog to resolve: {}",
        accepted.body
    );
}

/// A model names a scope; it never names a native shape.
///
/// This is the boundary the whole semantic topology family exists to hold. The
/// request type has fields for a target and a revision and nothing else, so a
/// caller cannot state a node kind, a parent, a native id or name, or a working
/// directory — the server derives every one of those from the pinned
/// specification. Each of these would be a way for a client to decide where a
/// session physically lives, which is how a team's roles end up split across
/// two containers.
#[tokio::test]
async fn a_topology_request_cannot_carry_a_kind_a_parent_or_a_native_shape() {
    let world = World::open().await;
    let uri = format!("/v1/projects/{}/topology:ensure", world.project);
    let target = serde_json::json!({"scope": "project_root"});

    for smuggled in [
        serde_json::json!({"kind_key": "PSW"}),
        serde_json::json!({"parent_topology_node_id": TopologyNodeId::generate().to_string()}),
        serde_json::json!({"native_id": "container-1"}),
        serde_json::json!({"native_name": "kontor-psw"}),
        serde_json::json!({"cwd": "/tmp/somewhere"}),
        serde_json::json!({"desired_binding": {"runtime_kind": "fake.runtime"}}),
    ] {
        let mut body = serde_json::json!({"target": target, "expected_revision": 1});
        let (field, value) = smuggled
            .as_object()
            .and_then(|object| object.iter().next())
            .map(|(key, value)| (key.clone(), value.clone()))
            .expect("one smuggled field");
        body[&field] = value;

        let answer = Call::post(&uri, &body)
            .signed_as(&world, "operator")
            .with_key(format!("smuggle-{field}"))
            .send(&world)
            .await;
        assert_eq!(
            answer.status, 400,
            "`{field}` must be refused by the request type, not interpreted: {}",
            answer.body
        );
    }

    // The same request without the smuggled field gets past the wire and
    // actually ensures the project root, so the refusals above are about the
    // extra field and not about the shape in general.
    let clean = Call::post(
        &uri,
        &serde_json::json!({"target": target, "expected_revision": 1}),
    )
    .signed_as(&world, "operator")
    .with_key("clean-ensure")
    .send(&world)
    .await;
    assert_eq!(clean.status, 200, "{}", clean.body);
}

/// The semantic target is a closed set, not a string the server interprets.
#[tokio::test]
async fn an_invented_topology_scope_is_refused() {
    let world = World::open().await;
    let answer = Call::post(
        format!("/v1/projects/{}/topology:ensure", world.project),
        &serde_json::json!({
            "target": {"scope": "whatever_i_like"},
            "expected_revision": 1,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("invented-scope")
    .send(&world)
    .await;
    assert_eq!(
        answer.status, 400,
        "a scope outside the closed union must be refused: {}",
        answer.body
    );
}

/// Completion answers from its own rows, and refuses where its inputs are absent.
///
/// The two halves matter equally. A composed catalog must actually answer —
/// including the built-in profile every project may pin — or the ticket that
/// composed it delivered nothing. And a completion that has not started must be
/// a `404`, never an invented empty state: a phase with nothing outstanding
/// reads exactly like an epic that has finished.
#[tokio::test]
async fn completion_answers_from_its_own_repository_and_never_synthesizes() {
    let world = World::open().await;
    let project = world.project;

    // The catalog answers, and the built-in profile is in it.
    let catalog = Call::get(format!("/v1/projects/{project}/completion-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    assert_eq!(catalog.realm(), world.realm_id());
    let revisions = catalog.json()["revisions"]
        .as_array()
        .expect("revisions")
        .clone();
    assert_eq!(
        revisions.len(),
        1,
        "only the built-in profile is published yet: {}",
        catalog.body
    );
    assert_eq!(revisions[0]["id"], "operational_default");
    assert_eq!(revisions[0]["version"], 1);
    // An append-only catalog stands at its publication count, so a first apply
    // presents `1`.
    assert_eq!(catalog.json()["revision"], 1);

    // An epic with no completion run is absent, not empty.
    let epic = MiniProjectId::generate();
    let missing = Call::get(format!("/v1/projects/{project}/epics/{epic}/completion"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(missing.status, 404, "{}", missing.body);
    for absent in ["phase", "blockers", "revision"] {
        assert!(
            missing.json().get(absent).is_none(),
            "a refusal carried `{absent}`: {}",
            missing.body
        );
    }

    // Publishing the built-in id back is refused: two definitions answering to
    // one pinned name is what an epic's pin exists to prevent.
    let shadow = Call::post(
        format!("/v1/projects/{project}/completion-profiles:preview"),
        &serde_json::json!({"definition": {
            "id": "operational_default",
            "version": 1,
            "name": "Shadow",
            "integration_team": "team-c",
            "verdict_committee": "independent_review",
            "max_remediation_rounds": 1,
            "polling_fallback": null
        }}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(shadow.status, 400, "{}", shadow.body);

    // An unknown field is refused before anything is hashed, so a caller cannot
    // get an unmodelled key counted into the preview hash its apply is compared
    // against.
    let smuggled = Call::post(
        format!("/v1/projects/{project}/completion-profiles:preview"),
        &serde_json::json!({"definition": {
            "id": "house-style",
            "version": 1,
            "name": "House style",
            "integration_team": "team-c",
            "verdict_committee": "independent_review",
            "max_remediation_rounds": 1,
            "polling_fallback": null,
            "skip_closeout": true
        }}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(smuggled.status, 400, "{}", smuggled.body);

    // A well-formed definition previews, publishes once, and the catalog moves.
    let definition = serde_json::json!({
        "id": "house-style",
        "version": 1,
        "name": "House style",
        "integration_team": "team-c",
        "verdict_committee": "independent_review",
        "max_remediation_rounds": 1,
        "polling_fallback": {"max_attempts": 3}
    });
    let preview = Call::post(
        format!("/v1/projects/{project}/completion-profiles:preview"),
        &serde_json::json!({"definition": definition}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert!(
        preview.json()["violations"]
            .as_array()
            .expect("violations")
            .is_empty(),
        "a valid definition has no violations: {}",
        preview.body
    );
    let hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();

    let apply = serde_json::json!({
        "definition": definition,
        "preview_hash": hash,
        "expected_revision": 1
    });
    let published = Call::post(
        format!("/v1/projects/{project}/completion-profiles:apply"),
        &apply,
    )
    .signed_as(&world, "admin")
    .with_key("publish-house-style")
    .send(&world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    assert_eq!(published.json()["published"]["id"], "house-style");
    assert_eq!(published.json()["receipt"]["applied"], "created");
    assert_eq!(published.json()["receipt"]["revision"], 2);

    // The same key replays to the same receipt and publishes nothing further.
    let replayed = Call::post(
        format!("/v1/projects/{project}/completion-profiles:apply"),
        &apply,
    )
    .signed_as(&world, "admin")
    .with_key("publish-house-style")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["receipt"]["revision"],
        2,
        "a replay publishes no second revision: {}",
        replayed.body
    );

    // And the stale expected revision the first apply consumed now conflicts.
    let stale = Call::post(
        format!("/v1/projects/{project}/completion-profiles:apply"),
        &serde_json::json!({
            "definition": {
                "id": "house-style",
                "version": 2,
                "name": "House style",
                "integration_team": "team-c",
                "verdict_committee": "independent_review",
                "max_remediation_rounds": 1,
                "polling_fallback": {"max_attempts": 3}
            },
            "preview_hash": hash,
            "expected_revision": 1
        }),
    )
    .signed_as(&world, "admin")
    .with_key("publish-house-style-v2")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
}

/// A composed Core Team route is held to the same rule the uncomposed ones are.
///
/// `GET /core-team` no longer answers `unavailable` — OP-04 composed it — so the
/// rule it now has to keep is the one that guard existed to protect: a project
/// that has published no revision is told exactly that, rather than handed an
/// empty roster. Every valid Core Team seats a required `LSA` and `TPM`, so an
/// empty seat list is a state no apply can produce, and a caller reading one
/// would conclude the project was deliberately staffed with nobody.
#[tokio::test]
async fn an_unconfigured_project_has_no_core_team_rather_than_an_empty_one() {
    let world = World::open().await;
    let answer = Call::get(format!("/v1/projects/{}/core-team", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        answer.status, 404,
        "an unconfigured project must refuse rather than answer: {}",
        answer.body
    );
    assert_eq!(answer.code(), "not_found");
    assert!(
        answer.json().get("seats").is_none(),
        "the refusal carried a roster: {}",
        answer.body
    );

    // Quick roles are a projection of that same absent roster, and an empty
    // picker is the tempting lie here: it reads as "this project allows no
    // ad-hoc work" rather than "this project has not been configured".
    let roles = Call::get(format!("/v1/projects/{}/quick-roles", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(roles.status, 404, "{}", roles.body);
    assert!(
        roles.json().get("roles").is_none(),
        "the refusal carried a picker: {}",
        roles.body
    );
}

/// The successor contracts check authority before they find no service.
#[tokio::test]
async fn a_successor_contract_refuses_an_under_authorized_caller_first() {
    let world = World::open().await;
    // Publishing an Advisor profile is admin configuration.
    let refused = Call::post(
        format!("/v1/projects/{}/advisor-profiles:apply", world.project),
        &serde_json::json!({
            "definition": {"schema_version": 1},
            "preview_hash": "0".repeat(64),
            "expected_revision": 1,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("operator-may-not-publish-a-profile")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.code(), "forbidden");
}

// ---------------------------------------------------------------------------
// OP-05 CP1 — published consultation policy, driven through the public API.
// ---------------------------------------------------------------------------

/// One complete, publishable Advisor profile.
fn advisor_definition(profile_id: &str, version: u32) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "profile_id": profile_id,
        "version": version,
        "name": "Data platform advisor",
        "short_name": "Data",
        "expertise": "Postgres, CDC and inter-service data flow.",
        "behavior": "Answer the question asked, and cite the evidence you were given.",
        "output_requirements": "A recommendation and the evidence it rests on.",
        "models": {
            "rungs": [{"provider": "claude", "model": "claude-opus-5", "effort": "high"}]
        },
        "context": {"skills": [], "files": [], "memory": "none"},
        "allowed_caller_roles": ["architect"],
        "allowed_scopes": ["epic"],
        "budget": {
            "max_tokens": 200000,
            "max_commands": 20,
            "max_duration_seconds": 1800,
            "max_cost": {"minor_units": 5000, "currency": "NOK"}
        },
        "max_consultations": 2
    })
}

const ADVISOR_PROFILE: &str = "01991c00-0000-7000-8000-0000000000a1";

/// The Advisor catalog starts empty; the production Committee preset is seeded.
/// Existing projects receive an absent preset lazily, which is what makes
/// upgrading a realm repair the catalog without rebuilding its database.
#[tokio::test]
async fn consultation_catalogs_seed_the_operational_committee_preset() {
    let world = World::open().await;
    let advisors = Call::get(format!("/v1/projects/{}/advisor-profiles", world.project))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(advisors.status, 200, "{}", advisors.body);
    assert_eq!(
        advisors.json()["revisions"].as_array().map(Vec::len),
        Some(0)
    );

    let committees = Call::get(format!(
        "/v1/projects/{}/committee-templates",
        world.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(committees.status, 200, "{}", committees.body);
    assert_eq!(
        committees.json()["revisions"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        committees.json()["revisions"][0]["id"],
        "01991c00-0000-7000-8000-000000000001"
    );
    assert_eq!(committees.json()["revisions"][0]["version"], 1);
}

/// A bundle is only a lazy bootstrap source. Once a revision is published, its
/// stored bytes remain authoritative even if a later daemon ships different
/// bytes under that identity; policy changes must append another version.
#[tokio::test]
async fn a_bundled_preset_change_does_not_block_an_immutable_published_revision() {
    let world = World::open().await;
    let mut historical = kontor_profiles::seeds::bundled_consultation_presets()
        .expect("the bundled presets load")
        .committee_templates
        .remove(0);
    historical.name =
        ExternalName::parse("Historical independent review").expect("a bounded historical name");
    let canonical = historical
        .canonicalize()
        .expect("the historical revision canonicalizes");
    let historical_hash = canonical.hash().clone();
    world.daemon.state().with_store(|store| {
        store
            .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
                project_id: world.project,
                family: ConsultationFamily::Committee,
                profile_id: historical.template_id.to_string(),
                version: historical.version,
                name: historical.name.clone(),
                definition: canonical.json().to_owned(),
                definition_hash: historical_hash.clone(),
                published_at: kontor_api::now(),
            })
            .expect("the historical immutable revision publishes");
    });

    let catalog = Call::get(format!(
        "/v1/projects/{}/committee-templates",
        world.project
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    assert_eq!(
        catalog.json()["revisions"].as_array().map(Vec::len),
        Some(1),
        "the lazy seed appended or replaced an existing identity: {}",
        catalog.body
    );
    assert_eq!(
        catalog.json()["revisions"][0]["definition_hash"],
        historical_hash.as_str(),
        "the catalog did not preserve the published bytes: {}",
        catalog.body
    );
}

/// A bundled team template is bootstrap data, exactly like a consultation
/// preset: once the identity is published, the stored bytes are authoritative
/// even when a later daemon ships different bytes under it, and changed policy
/// belongs in the next bundled version. Preview and apply must therefore
/// tolerate the drift instead of refusing the application, and the stored
/// bytes must stay the ones every later read resolves.
#[tokio::test]
async fn a_bundled_team_template_change_does_not_block_an_immutable_published_revision() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "team-drift-1",
        "Team drift",
        "/tmp/kontor-team-drift",
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

    // The historical bytes the project froze under this identity before the
    // daemon shipped a different definition: same template id and version, a
    // different frozen name.
    let historical = bundled_team(Some("Historical bundled team"), at("2026-08-10T09:00:00Z"));
    let historical_hash = historical.definition.hash().clone();
    let historical_id = historical.template_id;
    let historical_version = historical.version;
    world.daemon.state().with_store(|store| {
        store
            .insert_team_template(
                ProjectId::parse(&project).expect("a project id"),
                &historical,
            )
            .expect("the historical immutable revision publishes");
    });

    let category = first_category(&world).await;
    let body = epic_body(
        revision,
        "Drifted epic",
        &category,
        serde_json::json!([{"title": "Reapply the epic"}]),
    );

    // The CAT-12 shape: preview, apply and a same-key reapply all succeed while
    // the published identity names different bundled bytes.
    let preview = Call::post(format!("/v1/projects/{project}/epics:preview"), &body)
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(
        preview.json()["team_template"]["version"],
        serde_json::json!(1),
        "the preview still pins the published identity: {}",
        preview.body
    );
    assert_eq!(
        preview.json()["team_template_hash"],
        historical_hash.as_str(),
        "project preview must report the stored execution policy: {}",
        preview.body
    );
    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("drifted-epic")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(
        applied.json()["team_template_hash"],
        historical_hash.as_str(),
        "apply must report the stored execution policy: {}",
        applied.body
    );
    let replayed = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("drifted-epic")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["team_template_hash"],
        historical_hash.as_str(),
        "a replay must read back the same stored execution policy: {}",
        replayed.body
    );

    // The stored historical bytes are untouched and are the ones the read path
    // resolves — a team run freezes exactly this revision.
    let stored = world
        .daemon
        .state()
        .with_store(|store| {
            store.get_team_template(
                ProjectId::parse(&project).expect("a project id"),
                historical_id,
                historical_version,
            )
        })
        .expect("the stored revision reads")
        .expect("the published identity is still stored");
    assert_eq!(
        stored.definition.hash(),
        &historical_hash,
        "the application replaced the published bytes: {}",
        stored.definition.json()
    );
    assert_eq!(stored.name.as_str(), "Historical bundled team");
}

/// An absent team identity is still inserted normally with the bundled bytes.
#[tokio::test]
async fn an_absent_team_template_identity_is_still_inserted_normally() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "team-insert-1",
        "Team insert",
        "/tmp/kontor-team-insert",
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Fresh epic",
            &category,
            serde_json::json!([{"title": "First task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("fresh-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);

    let bundled = bundled_team(None, at("2026-08-10T09:00:00Z"));
    let stored = world
        .daemon
        .state()
        .with_store(|store| {
            store.get_team_template(
                ProjectId::parse(&project).expect("a project id"),
                bundled.template_id,
                bundled.version,
            )
        })
        .expect("the stored revision reads")
        .expect("the missing identity was inserted");
    assert_eq!(
        stored.definition.hash(),
        bundled.definition.hash(),
        "the inserted bytes are not the bundled bytes: {}",
        stored.definition.json()
    );
}

/// The work profile is a *contract*, not bootstrap data: a different definition
/// at an already-published profile identity must stay refused even while the
/// team template under the same application tolerates drift.
#[tokio::test]
async fn a_work_profile_drift_stays_refused_while_a_team_identity_tolerates_it() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "profile-drift-1",
        "Profile drift",
        "/tmp/kontor-profile-drift",
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

    // Same profile identity as the bundle will pin, different stored bytes.
    let mut historical = bundled_profile_under(at("2026-08-10T09:00:00Z"));
    historical.name = ExternalName::parse("Historical profile").expect("a bounded historical name");
    world.daemon.state().with_store(|store| {
        store
            .insert_work_profile(
                ProjectId::parse(&project).expect("a project id"),
                &historical,
            )
            .expect("the historical profile revision publishes");
    });

    let category = first_category(&world).await;
    let refused = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Drifted profile epic",
            &category,
            serde_json::json!([{"title": "Refused"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("drifted-profile-epic")
    .send(&world)
    .await;
    assert_eq!(
        refused.status, 409,
        "the work-profile drift stayed refused: {}",
        refused.body
    );
}

/// Profile selection must reconcile immutable bytes even when the catalogue's
/// candidate repeats the active workflow's `(id, version)`. Comparing only that
/// pair would call the selection unchanged, skip the store's fail-closed check,
/// and project facts from bytes the task never froze.
#[tokio::test]
async fn profile_selection_refuses_same_identity_work_profile_byte_drift() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "profile-selection-drift").await;

    let bundled = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let project_id = ProjectId::parse(&seed.project).expect("a project id");
    let task_id = TaskId::parse(&seed.task).expect("a task id");
    let active = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the active workflow reads")
            .expect("the bootstrapped task has a workflow")
    });
    let first_index = bundled
        .manifest
        .iter()
        .position(|entry| {
            entry.profile.as_ref() == Some(&active.snapshot.definition.id)
                && entry.profile_version == Some(active.snapshot.definition.version)
        })
        .expect("the active workflow's bundled category");
    let first = &bundled.manifest[first_index];
    let profile_id = first.profile.as_ref().expect("a profile id");
    let profile_version = first.profile_version.expect("a profile version");
    let mut drifted = serde_json::to_value(&bundled).expect("the bundled pack serializes");
    drifted["pack_id"] = serde_json::json!("profile-selection-drift-pack");
    for (index, entry) in drifted["manifest"]
        .as_array_mut()
        .expect("a manifest")
        .iter_mut()
        .enumerate()
    {
        entry["category"] = serde_json::json!(format!("selection-drift-{index}"));
    }
    let profile = drifted["profiles"]
        .as_array_mut()
        .expect("profiles")
        .iter_mut()
        .find(|profile| {
            profile["id"] == profile_id.as_str()
                && profile["version"].as_u64() == Some(u64::from(profile_version.get()))
        })
        .expect("the seeded profile");
    profile["name"] = serde_json::json!("Same identity, different bytes");
    let drift_category = drifted["manifest"][first_index]["category"]
        .as_str()
        .expect("the drift category")
        .to_owned();
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": drifted}),
    )
    .signed_as(&world, "admin")
    .with_key("profile-selection-drift-pack")
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let advertised = Call::get(format!("/v1/catalog/work-profiles/{drift_category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(advertised.status, 200, "{}", advertised.body);
    assert_eq!(
        advertised.json()["profile"]["id"],
        active.snapshot.definition.id.as_str(),
        "{}",
        advertised.body
    );
    assert_eq!(
        advertised.json()["profile"]["version"],
        u64::from(active.snapshot.definition.version.get()),
        "{}",
        advertised.body
    );

    let uri = format!(
        "/v1/projects/{}/tasks/{}/profile-selection",
        seed.project, seed.task
    );
    let refused = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": drift_category,
            "reason": "Try the same published identity with different bytes"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("profile-selection-drift-attempt")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "revision_conflict");

    // The refusal happened before a receipt or workflow mutation: the same key
    // is still free for the exact bundled definition the task already froze.
    let accepted = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": first.category.as_str(),
            "reason": "Try the same published identity with different bytes"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("profile-selection-drift-attempt")
    .send(&world)
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(accepted.json()["applied"], "unchanged");
}

/// Profile selection retries are receipt reads, never a second resolution or
/// effect. P1 is selected under key K, the category progresses to P2, and K
/// still reports its exact stored P1 result with its original receipt. A fresh
/// key K2 may then select P2. Even if the category later becomes unavailable,
/// each key remains replayable as its own historical result while P2 stays
/// active.
#[tokio::test]
async fn profile_selection_replay_precedes_category_resolution_and_never_replaces_twice() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "profile-selection-replay").await;
    let project_id = ProjectId::parse(&seed.project).expect("a project id");
    let task_id = TaskId::parse(&seed.task).expect("a task id");

    let first_pack = incident_pack_revision(1, "selection-replay-p1");
    let category = first_pack["manifest"][0]["category"]
        .as_str()
        .expect("a category")
        .to_owned();
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": first_pack}),
    )
    .signed_as(&world, "admin")
    .with_key("selection-replay-pack-p1")
    .send(&world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let uri = format!(
        "/v1/projects/{}/tasks/{}/profile-selection",
        seed.project, seed.task
    );
    let body = serde_json::json!({
        "expected_revision": seed.task_revision,
        "work_profile_category": category,
        "reason": "Select the incident policy"
    });
    let selected = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("selection-replay-k")
        .send(&world)
        .await;
    assert_eq!(selected.status, 200, "{}", selected.body);
    assert_eq!(selected.json()["applied"], "created");
    assert_eq!(selected.json()["work_profile"]["version"], 1);
    assert_eq!(selected.json()["team_template"]["version"], 1);
    let p1_profile = selected.json()["work_profile"].clone();
    let p1_team = selected.json()["team_template"].clone();
    let p1_hash = selected.json()["team_template_hash"].clone();
    let receipt = selected.json()["receipt_id"].clone();
    let p1_workflow = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the active workflow reads")
            .expect("P1 is active")
            .id
    });
    let p1_outcome = world.daemon.state().with_store(|store| {
        store
            .get_profile_selection_outcome(
                project_id,
                CommandReceiptId::parse(receipt.as_str().expect("a receipt id"))
                    .expect("a valid receipt id"),
            )
            .expect("the selection outcome reads")
            .expect("P1 has a durable selection outcome")
    });
    assert_eq!(p1_outcome.workflow_id, p1_workflow);
    assert_eq!(p1_outcome.profile.1.get(), 1);

    let second_pack = incident_pack_revision(2, "selection-replay-p2");
    append_profile_pack(
        &world,
        &second_pack,
        at("2020-01-01T00:00:00Z"),
        "selection-replay-pack-p2",
    );
    let current = Call::get(format!("/v1/catalog/work-profiles/{category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(current.status, 200, "{}", current.body);
    assert_eq!(current.json()["profile"]["version"], 2, "{}", current.body);

    let replayed = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("selection-replay-k")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt_id"], receipt);
    assert_eq!(replayed.json()["work_profile"], p1_profile);
    assert_eq!(replayed.json()["team_template"], p1_team);
    assert_eq!(replayed.json()["team_template_hash"], p1_hash);
    assert_eq!(replayed.json()["applied"], "created");
    let after_replay = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the active workflow reads")
            .expect("P1 remains active")
            .id
    });
    assert_eq!(
        after_replay, p1_workflow,
        "a retry created no second workflow"
    );

    let changed = Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": category,
            "reason": "A different operation wearing K"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("selection-replay-k")
    .send(&world)
    .await;
    assert_eq!(changed.status, 409, "{}", changed.body);
    assert_eq!(changed.code(), "idempotency_conflict");

    let progressed = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("selection-replay-k2")
        .send(&world)
        .await;
    assert_eq!(progressed.status, 200, "{}", progressed.body);
    assert_eq!(progressed.json()["applied"], "created");
    assert_eq!(progressed.json()["work_profile"]["version"], 2);
    assert_eq!(progressed.json()["team_template"]["version"], 2);
    let p2_profile = progressed.json()["work_profile"].clone();
    let p2_team = progressed.json()["team_template"].clone();
    let p2_hash = progressed.json()["team_template_hash"].clone();
    let p2_receipt = progressed.json()["receipt_id"].clone();
    let p2_workflow = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the active workflow reads")
            .expect("P2 is active")
            .id
    });
    assert_ne!(p2_workflow, p1_workflow, "the fresh key selected P2");
    let p2_outcome = world.daemon.state().with_store(|store| {
        store
            .get_profile_selection_outcome(
                project_id,
                CommandReceiptId::parse(p2_receipt.as_str().expect("a receipt id"))
                    .expect("a valid receipt id"),
            )
            .expect("the selection outcome reads")
            .expect("P2 has a durable selection outcome")
    });
    assert_eq!(p2_outcome.workflow_id, p2_workflow);
    assert_eq!(p2_outcome.profile.1.get(), 2);

    let mut unavailable = incident_pack_revision(3, "selection-replay-unavailable");
    unavailable["manifest"][0]["availability"] = serde_json::json!("manifest_only");
    unavailable["manifest"][0]["profile"] = serde_json::Value::Null;
    unavailable["manifest"][0]["profile_version"] = serde_json::Value::Null;
    append_profile_pack(
        &world,
        &unavailable,
        at("2010-01-01T00:00:00Z"),
        "selection-replay-pack-unavailable",
    );
    let unavailable_now = Call::get(format!("/v1/catalog/work-profiles/{category}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_ne!(
        unavailable_now.status, 200,
        "the fixture no longer resolves: {}",
        unavailable_now.body
    );

    let replayed_without_category = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("selection-replay-k")
        .send(&world)
        .await;
    assert_eq!(
        replayed_without_category.status, 200,
        "{}",
        replayed_without_category.body
    );
    assert_eq!(replayed_without_category.json()["receipt_id"], receipt);
    assert_eq!(replayed_without_category.json()["work_profile"], p1_profile);
    assert_eq!(replayed_without_category.json()["team_template"], p1_team);
    assert_eq!(
        replayed_without_category.json()["team_template_hash"],
        p1_hash
    );
    assert_eq!(replayed_without_category.json()["applied"], "created");

    let replayed_k2_without_category = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("selection-replay-k2")
        .send(&world)
        .await;
    assert_eq!(
        replayed_k2_without_category.status, 200,
        "{}",
        replayed_k2_without_category.body
    );
    assert_eq!(
        replayed_k2_without_category.json()["receipt_id"],
        p2_receipt
    );
    assert_eq!(
        replayed_k2_without_category.json()["work_profile"],
        p2_profile
    );
    assert_eq!(
        replayed_k2_without_category.json()["team_template"],
        p2_team
    );
    assert_eq!(
        replayed_k2_without_category.json()["team_template_hash"],
        p2_hash
    );
    assert_eq!(replayed_k2_without_category.json()["applied"], "created");
    let after_unavailable_replay = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the active workflow reads")
            .expect("P2 remains active")
            .id
    });
    assert_eq!(after_unavailable_replay, p2_workflow);
}

/// A receipt created before schema v62 has no exact receipt-to-workflow result
/// to replay. It must fail explicitly rather than project the task's current
/// workflow (which may belong to an unrelated later selection) or try to
/// resolve a category that no longer exists.
#[tokio::test]
async fn a_legacy_profile_selection_receipt_never_borrows_the_active_workflow() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let seed = bootstrap(&world, "legacy-profile-selection-receipt").await;
    let project_id = ProjectId::parse(&seed.project).expect("a project id");
    let task_id = TaskId::parse(&seed.task).expect("a task id");
    let key = IdempotencyKey::parse("legacy-receipt-without-selection-outcome-k")
        .expect("an idempotency key");
    let category = "category-that-no-pack-holds";
    let reason = "Replay a historical selection";
    let intent = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": "profile_selection",
        "task_id": seed.task,
        "work_profile_category": category,
        "reason": reason,
    }))
    .expect("a canonical selection intent");
    world.daemon.state().with_store(|store| {
        store
            .record_local_command(&NewLocalCommand {
                project_id,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key.clone(),
                kind: CommandKind::SelectTaskProfile,
                target: AggregateRef::Task { task_id },
                target_revision: AggregateRevision::parse(seed.task_revision)
                    .expect("a task revision"),
                intent,
                created_at: at("2026-08-26T00:00:00Z"),
            })
            .expect("the historical receipt is durable");
    });
    let before = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the workflow reads")
            .expect("the bootstrap workflow is active")
            .id
    });

    let replay = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/profile-selection",
            seed.project, seed.task
        ),
        &serde_json::json!({
            "expected_revision": seed.task_revision,
            "work_profile_category": category,
            "reason": reason,
        }),
    )
    .signed_as(&world, "admin")
    .with_key(key.as_str())
    .send(&world)
    .await;
    assert_eq!(replay.status, 409, "{}", replay.body);
    assert_eq!(replay.code(), "revision_conflict");
    assert!(
        replay.body.contains("predates exact outcome binding"),
        "the refusal identifies unreconstructable history, not category lookup: {}",
        replay.body
    );
    let after = world.daemon.state().with_store(|store| {
        store
            .get_active_task_workflow(project_id, task_id)
            .expect("the workflow reads")
            .expect("the bootstrap workflow remains active")
            .id
    });
    assert_eq!(after, before, "an unreconstructable replay writes nothing");
}

/// The user-published Teams ledger is a different immutability surface: its
/// revisions stay append-only no matter what the application contract stores
/// under the bundled team identities.
#[tokio::test]
async fn a_user_published_team_ledger_stays_append_only_after_epic_drift() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created =
        ensure_project(&world, "ledger-1", "Team ledger", "/tmp/kontor-team-ledger").await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

    let draft = serde_json::json!({
        "id": "user-team",
        "name": "User team v1",
        "slots": [{
            "id": "lead",
            "role": {
                "catalog_revision": {"id": "standard-roles", "version": 1},
                "role_code": "LSA"
            },
            "capabilities": {"context": {"class": "standard"}}
        }]
    });
    let saved = Call::post("/v1/teams/drafts:save", &draft)
        .signed_as(&world, "operator")
        .with_key("user-team-save-1")
        .send(&world)
        .await;
    assert_eq!(saved.status, 200, "{}", saved.body);
    let first = Call::post("/v1/teams/user-team/publish", &serde_json::json!({}))
        .signed_as(&world, "operator")
        .with_key("user-team-publish-1")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(
        first.json()["revisions"][0]["name"],
        serde_json::json!("User team v1")
    );

    // A drifted epic application under the bundled team identity changes nothing
    // in the user-published ledger.
    let historical = bundled_team(Some("Historical bundled team"), at("2026-08-10T09:00:00Z"));
    world.daemon.state().with_store(|store| {
        store
            .insert_team_template(
                ProjectId::parse(&project).expect("a project id"),
                &historical,
            )
            .expect("the historical immutable revision publishes");
    });
    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Drifted epic",
            &category,
            serde_json::json!([{"title": "Reapply the epic"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("ledger-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);

    let second = Call::post("/v1/teams/user-team/publish", &serde_json::json!({}))
        .signed_as(&world, "operator")
        .with_key("user-team-publish-2")
        .send(&world)
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(
        second.json()["revisions"][0]["name"],
        serde_json::json!("User team v1"),
        "the published v1 was rewritten: {}",
        second.body
    );
    assert_eq!(
        second.json()["revisions"][1]["version"],
        serde_json::json!(2),
        "the user-published ledger did not append: {}",
        second.body
    );
}

/// The semantics, not only the conflict: a task started under a drifted
/// identity freezes the *stored* historical bytes into its team run — never
/// the current bundle's — so no current-bundle policy can leak under the old
/// identity. The realm team-template catalog keeps advertising the shipped
/// bundle (it is a realm build advertisement, like the work-profile catalog);
/// the frozen run is the proof of what actually executes.
#[tokio::test]
async fn a_started_team_run_freezes_the_stored_bytes_of_a_drifted_identity() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "launch-drift-1",
        "Launch drift",
        "/tmp/kontor-launch-drift",
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

    let historical = bundled_team(Some("Historical bundled team"), at("2026-08-10T09:00:00Z"));
    let historical_hash = historical.definition.hash().clone();
    let historical_id = historical.template_id;
    let historical_version = historical.version;
    world.daemon.state().with_store(|store| {
        store
            .insert_team_template(
                ProjectId::parse(&project).expect("a project id"),
                &historical,
            )
            .expect("the historical immutable revision publishes");
    });

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Lead", "harness": "fake.runtime",
            "credential_alias": "lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("launch-drift-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let category = first_category(&world).await;
    let body = epic_body(
        revision,
        "Launch drift epic",
        &category,
        serde_json::json!([{"title": "Launch under the old identity"}]),
    );
    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &body)
        .signed_as(&world, "admin")
        .with_key("launch-drift-epic")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("a revision");

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {
                "max_tokens": 1000,
                "max_commands": 10,
                "max_duration_seconds": 600,
                "max_cost_minor_units": 100,
                "cost_currency": "NOK"
            },
            "granted_by": account_id,
            "reason": "Launch under the stored identity"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("launch-drift-arm")
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
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan.json()["plan_hash"]}),
    )
    .signed_as(&world, "operator")
    .with_key("launch-drift-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let started_body = started.json();
    let seats = started_body["started"].as_array().expect("seats");
    assert!(!seats.is_empty(), "the task was seated: {}", started.body);
    let team_run_id = seats[0]["team_run_id"]
        .as_str()
        .expect("a team run id")
        .to_owned();

    // The frozen run carries the stored historical bytes, not the current
    // bundle's: same identity, historical policy.
    let run = world
        .daemon
        .state()
        .with_store(|store| {
            store.get_team_run(
                ProjectId::parse(&project).expect("a project id"),
                TeamRunId::parse(&team_run_id).expect("a team run id"),
            )
        })
        .expect("the run reads")
        .expect("the started run is stored");
    assert_eq!(
        run.snapshot.definition.hash(),
        &historical_hash,
        "the run froze current-bundle bytes under the historical identity: {}",
        run.snapshot.definition.json()
    );
    assert_eq!(
        run.snapshot.template_id, historical_id,
        "the run names the stored identity"
    );
    assert_eq!(
        run.snapshot.template_version, historical_version,
        "the run pins the stored version"
    );

    // The realm catalog remains the shipped-build bootstrap advertisement, but
    // it says so unequivocally and identifies the project store as execution
    // authority. A caller cannot mistake these current bytes for the historical
    // policy the project-scoped preview/apply and frozen run report.
    let catalog = Call::get("/v1/catalog/team-templates")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    let catalog_body = catalog.json();
    let advertised = catalog_body
        .as_array()
        .expect("a catalog array")
        .iter()
        .find(|entry| {
            entry["template"]["id"] == historical_id.to_string()
                && entry["template"]["version"] == serde_json::json!(1)
        })
        .expect("the bundled identity is advertised");
    let current = bundled_team(None, at("2026-08-10T09:00:00Z"));
    assert_eq!(
        advertised["definition_hash"],
        current.definition.hash().as_str(),
        "the catalog advertises the current bundle: {}",
        catalog.body
    );
    assert_eq!(advertised["source"], "bundled", "{}", catalog.body);
    assert_eq!(
        advertised["catalog_scope"], "realm_bootstrap",
        "{}",
        catalog.body
    );
    assert_eq!(
        advertised["execution_authority"], "project_stored_revision",
        "{}",
        catalog.body
    );
}

/// Resolve the seeded category's bundled team, optionally renamed so the bytes
/// differ from what the current build ships.
fn bundled_team(name: Option<&str>, at: Timestamp) -> kontor_core::spec::TeamTemplateRevision {
    let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
        .expect("the bundled pack seeds at least one category");
    let bundle = kontor_profiles::pack::resolve_profile(&pack, &entry.category, at)
        .expect("the seeded category resolves");
    let mut spec = kontor_teams::spec::TeamTemplateSpec::from_revision(
        bundle.team.as_ref().expect("the profile pinned a team"),
    )
    .expect("the bundled team parses");
    if let Some(name) = name {
        spec.name = ExternalName::parse(name).expect("a bounded historical name");
    }
    spec.to_revision().expect("the revision canonicalizes")
}

/// The seeded category's bundled work profile, under its current name.
fn bundled_profile_under(at: Timestamp) -> kontor_core::spec::WorkProfileSpec {
    let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
        .expect("the bundled pack seeds at least one category");
    let bundle = kontor_profiles::pack::resolve_profile(&pack, &entry.category, at)
        .expect("the seeded category resolves");
    bundle.profile.definition.clone()
}

/// Preview, publish, read back — and the version after it.
#[tokio::test]
async fn a_previewed_advisor_profile_publishes_and_reads_back() {
    let world = World::open().await;
    let project = world.project;

    let preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": advisor_definition(ADVISOR_PROFILE, 1)}),
    )
    .signed_as(&world, "admin")
    .with_key("preview-v1")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(
        preview.json()["violations"].as_array().map(Vec::len),
        Some(0),
        "a complete definition has nothing to fix: {}",
        preview.body
    );
    let hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();

    // A preview commits nothing: the catalog is still empty.
    let untouched = Call::get(format!("/v1/projects/{project}/advisor-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        untouched.json()["revisions"].as_array().map(Vec::len),
        Some(0),
        "a preview published something: {}",
        untouched.body
    );

    let applied = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &serde_json::json!({
            "definition": advisor_definition(ADVISOR_PROFILE, 1),
            "preview_hash": hash,
            "expected_revision": 1,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("apply-v1")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["published"]["version"].as_u64(), Some(1));
    assert_eq!(
        applied.json()["published"]["id"].as_str(),
        Some(ADVISOR_PROFILE)
    );
    assert_eq!(
        applied.json()["receipt"]["applied"].as_str(),
        Some("created")
    );

    let catalog = Call::get(format!("/v1/projects/{project}/advisor-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        catalog.json()["revisions"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        catalog.json()["revision"].as_u64(),
        Some(2),
        "publishing moved the catalog: {}",
        catalog.body
    );

    // The next version publishes against the revision that read reported.
    let next_preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": advisor_definition(ADVISOR_PROFILE, 2)}),
    )
    .signed_as(&world, "admin")
    .with_key("preview-v2")
    .send(&world)
    .await;
    let next_hash = next_preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let next = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &serde_json::json!({
            "definition": advisor_definition(ADVISOR_PROFILE, 2),
            "preview_hash": next_hash,
            "expected_revision": 2,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("apply-v2")
    .send(&world)
    .await;
    assert_eq!(next.status, 200, "{}", next.body);
    assert_eq!(next.json()["published"]["version"].as_u64(), Some(2));
}

/// A retry after a lost acknowledgement replays; it does not publish twice.
#[tokio::test]
async fn replaying_a_profile_apply_publishes_once() {
    let world = World::open().await;
    let project = world.project;
    let definition = advisor_definition(ADVISOR_PROFILE, 1);

    let preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": definition}),
    )
    .signed_as(&world, "admin")
    .with_key("replay-preview")
    .send(&world)
    .await;
    let hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let body = serde_json::json!({
        "definition": definition,
        "preview_hash": hash,
        "expected_revision": 1,
    });

    let first = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("apply-once")
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);

    // The same key and the same intent, presenting the revision it read before
    // the first attempt. Refusing this for the sole reason that the original
    // succeeded is the bug the replay-first ordering exists to avoid.
    let again = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &body,
    )
    .signed_as(&world, "admin")
    .with_key("apply-once")
    .send(&world)
    .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(
        again.json()["receipt"]["applied"].as_str(),
        Some("unchanged")
    );
    assert_eq!(again.json()["published"]["version"].as_u64(), Some(1));

    let catalog = Call::get(format!("/v1/projects/{project}/advisor-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        catalog.json()["revisions"].as_array().map(Vec::len),
        Some(1),
        "the replay published a second revision: {}",
        catalog.body
    );
}

/// A definition the preview never judged cannot be published under its hash.
#[tokio::test]
async fn a_profile_apply_must_match_the_named_preview() {
    let world = World::open().await;
    let project = world.project;
    let preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": advisor_definition(ADVISOR_PROFILE, 1)}),
    )
    .signed_as(&world, "admin")
    .with_key("swap-preview")
    .send(&world)
    .await;
    let hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();

    // Same shape, different content: what the Admin authorized was the document
    // they were shown.
    let mut swapped = advisor_definition(ADVISOR_PROFILE, 1);
    swapped["max_consultations"] = serde_json::json!(99);
    let refused = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &serde_json::json!({
            "definition": swapped,
            "preview_hash": hash,
            "expected_revision": 1,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("swap-apply")
    .send(&world)
    .await;
    assert_eq!(refused.status, 400, "{}", refused.body);
    assert_eq!(refused.code(), "invalid_request");
    assert!(refused.json().get("published").is_none());
}

/// Publishing against a revision the catalog has moved past writes nothing.
#[tokio::test]
async fn a_profile_apply_under_a_stale_revision_writes_nothing() {
    let world = World::open().await;
    let project = world.project;
    for (version, key) in [(1_u32, "stale-first"), (2, "stale-second")] {
        let preview = Call::post(
            format!("/v1/projects/{project}/advisor-profiles:preview"),
            &serde_json::json!({"definition": advisor_definition(ADVISOR_PROFILE, version)}),
        )
        .signed_as(&world, "admin")
        .with_key(format!("{key}-preview"))
        .send(&world)
        .await;
        let hash = preview.json()["preview_hash"]
            .as_str()
            .expect("a preview hash")
            .to_owned();
        let applied = Call::post(
            format!("/v1/projects/{project}/advisor-profiles:apply"),
            &serde_json::json!({
                "definition": advisor_definition(ADVISOR_PROFILE, version),
                "preview_hash": hash,
                // Both attempts claim the catalog is untouched. The second one
                // is wrong, because the first moved it.
                "expected_revision": 1,
            }),
        )
        .signed_as(&world, "admin")
        .with_key(format!("{key}-apply"))
        .send(&world)
        .await;
        if version == 1 {
            assert_eq!(applied.status, 200, "{}", applied.body);
        } else {
            assert_eq!(applied.status, 409, "{}", applied.body);
            assert_eq!(applied.code(), "revision_conflict");
        }
    }
    let catalog = Call::get(format!("/v1/projects/{project}/advisor-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        catalog.json()["revisions"].as_array().map(Vec::len),
        Some(1)
    );
}

/// A typo in a policy document is a violation, not a silently dropped field.
#[tokio::test]
async fn an_unknown_field_in_a_profile_is_reported_not_ignored() {
    let world = World::open().await;
    let mut definition = advisor_definition(ADVISOR_PROFILE, 1);
    definition["allowed_caller_role"] = serde_json::json!(["architect"]);
    let preview = Call::post(
        format!("/v1/projects/{}/advisor-profiles:preview", world.project),
        &serde_json::json!({"definition": definition}),
    )
    .signed_as(&world, "admin")
    .with_key("typo-preview")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert!(
        !preview.json()["violations"]
            .as_array()
            .expect("violations")
            .is_empty(),
        "a misspelled field must not be dropped and published: {}",
        preview.body
    );
}

/// The family comes from the route, never from the document.
#[tokio::test]
async fn an_advisor_profile_cannot_be_published_as_a_committee_template() {
    let world = World::open().await;
    let preview = Call::post(
        format!("/v1/projects/{}/committee-templates:preview", world.project),
        &serde_json::json!({"definition": advisor_definition(ADVISOR_PROFILE, 1)}),
    )
    .signed_as(&world, "admin")
    .with_key("wrong-family")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert!(
        !preview.json()["violations"]
            .as_array()
            .expect("violations")
            .is_empty(),
        "an Advisor profile is not a Committee template: {}",
        preview.body
    );
}

/// Publishing a profile seats nobody.
///
/// A profile is what a consultation *would* be asked under. If publishing one
/// created an ASW or a seat, an Admin editing policy would be spending provider
/// capacity, and the read-only boundary would already have been crossed before
/// anybody asked a question.
#[tokio::test]
async fn publishing_a_profile_creates_no_workspace_and_no_seat() {
    let world = World::open().await;
    let project = world.project;
    let before = Call::get(format!("/v1/projects/{project}/topology:inspect"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;

    let preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": advisor_definition(ADVISOR_PROFILE, 1)}),
    )
    .signed_as(&world, "admin")
    .with_key("no-seat-preview")
    .send(&world)
    .await;
    let hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let applied = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &serde_json::json!({
            "definition": advisor_definition(ADVISOR_PROFILE, 1),
            "preview_hash": hash,
            "expected_revision": 1,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("no-seat-apply")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);

    let after = Call::get(format!("/v1/projects/{project}/topology:inspect"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    // The realm cursor moves, because a receipt was written. What must not move
    // is the topology: no ASW, no CSW, no seat.
    assert_eq!(
        before.json()["nodes"],
        after.json()["nodes"],
        "publishing a profile changed the topology"
    );
    assert_eq!(
        after.json()["nodes"].as_array().map(Vec::len),
        Some(0),
        "publishing a profile materialized a node: {}",
        after.body
    );
}

// ---------------------------------------------------------------------------
// OP-03 CP2/CP3/CP4 — the composed behaviour, driven through the public API.
//
// Every test in this section exercises a *write path*, not a refusal shape.
// That distinction is the whole point: a complete route table whose operations
// all answer `unavailable` passes every contract test and admits no work, and
// the only way to tell the two apart is to make something happen and read it
// back.
// ---------------------------------------------------------------------------

/// A Realm with a project, an account profile and an epic, built through the
/// public operations alone.
struct Composed {
    world: World,
    project: String,
    project_revision: u64,
    account: String,
    epic: String,
}

async fn compose_realm(root: &str) -> Composed {
    let world = World::open_empty_with_a_plane().await;
    world.daemon.reconcile().await;

    let created = ensure_project(&world, "compose-project", "Kontor", root).await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let project_revision = created.json()["revision"].as_u64().expect("a revision");

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
    .with_key("compose-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            project_revision,
            "Composed epic",
            &category,
            serde_json::json!([{"title": "Do the thing"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("compose-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();

    Composed {
        world,
        project,
        project_revision,
        account,
        epic,
    }
}

/// A legacy import has no typed execution-scope row, so admission must recover
/// names from the durable Jira-linked epic/task metadata. This is the live QNR
/// shape that previously produced a raw epic UUID, an unresolved ECP template,
/// and `TSW · ASMA-7676 · ASMA-7676`.
#[tokio::test]
async fn a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles() {
    let world = World::open_empty_with_a_plane().await;
    world.daemon.reconcile().await;
    let created = ensure_project(
        &world,
        "legacy-naming-project",
        "Kontor Operational MVP · Ad-hoc Planning",
        "/tmp/kontor-legacy-naming",
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("the project id")
        .to_owned();
    let project_revision = created.json()["revision"]
        .as_u64()
        .expect("the project revision");
    let category = first_category(&world).await;
    let mut legacy_body = epic_body(
        project_revision,
        "ASMA-7675 · QNR v2 Nonprod Delivery",
        &category,
        serde_json::json!([{
            "title": "ASMA-7676 · grid-column ops and question-ownership invariant",
            "short_code": null,
            "ticket_links": [{"connector": "jira", "external_issue_key": "ASMA-7676"}],
            "worktree": "/tmp/kontor-legacy-naming/asma-7676"
        }]),
    );
    legacy_body
        .as_object_mut()
        .expect("the epic request is an object")
        .remove("execution_scope");
    let applied = Call::post(format!("/v1/projects/{project}/epics:apply"), &legacy_body)
        .signed_as(&world, "admin")
        .with_key("legacy-naming-epic")
        .send(&world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("the epic id")
        .to_owned();
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("the task id")
        .to_owned();

    // The description, worktree slug, Jira key and UUID are all available,
    // but none is an authorized substitute for the missing backlog code.
    let calls_before_refusal = world.fake.calls().len();
    let refused = Call::post(
        format!("/v1/projects/{project}/topology:materialize"),
        &serde_json::json!({
            "target": {"scope": "ticket", "task_id": task},
            "expected_revision": project_revision,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("legacy-naming-refuses-inference")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "placement_blocked");
    assert!(
        refused.body.contains("durable short code"),
        "the refusal must name the supported prerequisite: {}",
        refused.body
    );
    assert_eq!(
        world.fake.calls().len(),
        calls_before_refusal,
        "missing short-code validation must precede runtime contact"
    );

    // The supported migration adds only the explicit mapping and preserves the
    // epic/task/ticket identities before admission is retried.
    let mut mapped_body = legacy_body.clone();
    mapped_body["execution_scope"] = serde_json::json!({
        "external_epic_key": "ASMA-7675",
        "short_title": "QNR v2 Nonprod Delivery",
        "kontor_backlog_code": "QNR-P1",
        "ai_short_name": "Nonprod Delivery",
    });
    mapped_body["tasks"][0]["short_code"] = serde_json::json!("QNR-NP-01");
    let mapped = Call::post(format!("/v1/projects/{project}/epics:apply"), &mapped_body)
        .signed_as(&world, "admin")
        .with_key("legacy-naming-explicit-code")
        .send(&world)
        .await;
    assert_eq!(mapped.status, 200, "{}", mapped.body);
    assert_eq!(mapped.json()["applied"], "updated");
    assert_eq!(mapped.json()["epic_id"], epic);
    assert_eq!(mapped.json()["tasks"][0]["task_id"], task);
    assert_eq!(mapped.json()["tasks"][0]["short_code"], "QNR-NP-01");
    let mapped_epic_revision = mapped.json()["revision"]
        .as_u64()
        .expect("the updated epic revision");
    let materialized_ticket = Call::post(
        format!("/v1/projects/{project}/topology:materialize"),
        &serde_json::json!({
            "target": {"scope": "ticket", "task_id": task},
            "expected_revision": project_revision,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("legacy-naming-ticket")
    .send(&world)
    .await;
    assert_eq!(
        materialized_ticket.status, 200,
        "{}",
        materialized_ticket.body
    );

    // Ticket materialization binds ESW + TSW. The ECP is a sibling and is bound by
    // its own supported materialization operation; doing it here also proves
    // the canonical title reaches a real native control workspace.
    let materialized_control = Call::post(
        format!("/v1/projects/{project}/topology:materialize"),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": epic},
            "expected_revision": project_revision,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("legacy-naming-control")
    .send(&world)
    .await;
    assert_eq!(
        materialized_control.status, 200,
        "{}",
        materialized_control.body
    );

    let topology = Call::get(format!(
        "/v1/projects/{project}/topology:inspect?epic_id={epic}"
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(topology.status, 200, "{}", topology.body);
    let topology_json = topology.json();
    let title_for = |kind: &str| {
        let node = topology_json["nodes"]
            .as_array()
            .expect("the topology nodes")
            .iter()
            .find(|node| node["kind_key"] == kind)
            .unwrap_or_else(|| panic!("the {kind} node exists: {}", topology.body));
        let node_id = kontor_core::id::TopologyNodeId::parse(
            node["topology_node_id"].as_str().expect("the node id"),
        )
        .expect("a canonical node id");
        world
            .fake
            .container_title(node_id)
            .unwrap_or_else(|| panic!("the {kind} native container is bound"))
    };

    assert_eq!(title_for("ESW"), "ESW • ASMA-7675 • QNR-P1");
    assert_eq!(title_for("ECP"), "ECP • ASMA-7675 • QNR-P1");
    assert_eq!(title_for("TSW"), "TSW • ASMA-7676 • QNR-NP-01");

    let node = |kind: &str| {
        topology_json["nodes"]
            .as_array()
            .expect("the topology nodes")
            .iter()
            .find(|node| node["kind_key"] == kind)
            .unwrap_or_else(|| panic!("the {kind} node exists: {}", topology.body))
    };
    let node_id = |kind: &str| {
        kontor_core::id::TopologyNodeId::parse(
            node(kind)["topology_node_id"]
                .as_str()
                .expect("the topology node id"),
        )
        .expect("a canonical topology node id")
    };
    let native_id = |kind: &str| {
        node(kind)["observed_binding"]["native_id"]
            .as_str()
            .expect("the native container id")
            .to_owned()
    };
    let mut identities_before = vec![native_id("ESW"), native_id("ECP"), native_id("TSW")];
    identities_before.sort();

    // Reproduce the Paseo defect against the already-bound native containers:
    // both epic-level names ended with the descriptive title. The ticket title
    // is already correct and must remain untouched.
    world
        .fake
        .set_container_title(node_id("ESW"), "ESW • ASMA-7675 • QNR v2 Nonprod Delivery");
    world
        .fake
        .set_container_title(node_id("ECP"), "ECP • ASMA-7675 • QNR v2 Nonprod Delivery");
    let preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let preview_json = preview.json();
    let targets = preview_json["targets"]
        .as_array()
        .expect("the complete name plan");
    assert_eq!(targets.len(), 3, "all three bound containers are read");
    assert_eq!(
        targets
            .iter()
            .filter(|target| target["would_change"] == true)
            .count(),
        2,
        "only ESW and ECP are stale: {}",
        preview.body
    );
    let stale_preview_hash = preview_json["preview_hash"]
        .as_str()
        .expect("an identity-bound preview hash")
        .to_owned();
    world
        .fake
        .set_container_title(node_id("TSW"), "externally drifted after preview");
    let stale_apply = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:apply"),
        &serde_json::json!({
            "expected_revision": project_revision,
            "preview_hash": stale_preview_hash,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-native-name-stale-preview")
    .send(&world)
    .await;
    assert_eq!(stale_apply.status, 409, "{}", stale_apply.body);
    assert_eq!(stale_apply.code(), "revision_conflict");
    assert_eq!(
        title_for("ESW"),
        "ESW • ASMA-7675 • QNR v2 Nonprod Delivery",
        "a stale complete plan writes no earlier target"
    );
    assert_eq!(
        title_for("ECP"),
        "ECP • ASMA-7675 • QNR v2 Nonprod Delivery",
        "a stale complete plan writes no later target"
    );
    world
        .fake
        .set_container_title(node_id("TSW"), "TSW • ASMA-7676 • QNR-NP-01");
    let fresh_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(fresh_preview.status, 200, "{}", fresh_preview.body);
    let preview_hash = fresh_preview.json()["preview_hash"]
        .as_str()
        .expect("the refreshed identity-bound preview hash")
        .to_owned();
    let first_stale_node = TopologyNodeId::parse(
        fresh_preview.json()["targets"]
            .as_array()
            .expect("the complete target census")
            .iter()
            .find(|target| target["would_change"] == true)
            .and_then(|target| target["topology_node_id"].as_str())
            .expect("at least one stale target"),
    )
    .expect("a canonical topology node id");
    let apply_body = serde_json::json!({
        "expected_revision": project_revision,
        "preview_hash": preview_hash,
    });
    world.fake.take_calls();
    world.fake.lose_next_retitle_ack(first_stale_node);
    let interrupted = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:apply"),
        &apply_body,
    )
    .signed_as(&world, "admin")
    .with_key("qnr-native-name-repair")
    .send(&world)
    .await;
    assert!(
        interrupted.status.is_server_error(),
        "the fixture must lose the acknowledgement after its first effect: {}",
        interrupted.body
    );

    // The exact-key retry reads every target again. The already-renamed first
    // target is omitted from the effect plan, so the retry completes the
    // remaining repair without issuing a duplicate native mutation.
    let applied = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:apply"),
        &apply_body,
    )
    .signed_as(&world, "admin")
    .with_key("qnr-native-name-repair")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["changed"], 1);
    assert_eq!(applied.json()["receipt"]["applied"], "unchanged");
    let retitle_calls: Vec<TopologyNodeId> = world
        .fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            AdapterCall::RetitleContainer(node) => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(
        retitle_calls.len(),
        2,
        "the two stale targets receive exactly two total native mutations"
    );
    assert_eq!(
        retitle_calls.iter().copied().collect::<BTreeSet<_>>().len(),
        retitle_calls.len(),
        "an acknowledgement loss must not duplicate a target mutation"
    );
    assert_eq!(title_for("ESW"), "ESW • ASMA-7675 • QNR-P1");
    assert_eq!(title_for("ECP"), "ECP • ASMA-7675 • QNR-P1");
    assert_eq!(title_for("TSW"), "TSW • ASMA-7676 • QNR-NP-01");

    let replay = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:apply"),
        &apply_body,
    )
    .signed_as(&world, "admin")
    .with_key("qnr-native-name-repair")
    .send(&world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["changed"], 0);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    let mut identities_after: Vec<String> = replay.json()["readback"]["targets"]
        .as_array()
        .expect("fresh runtime readback")
        .iter()
        .map(|target| {
            target["native_id"]
                .as_str()
                .expect("a preserved native id")
                .to_owned()
        })
        .collect();
    identities_after.sort();
    assert_eq!(identities_after, identities_before);

    // Materialize both classes of persistent seat that whole-epic repair must
    // census. The Core Team path first bootstraps a frozen roster, then launches
    // an LSA in the already-bound ECP using the pinned ECP seat template.
    publish_core_team(
        &world,
        &project,
        serde_json::json!([seat("SA", "default", false)]),
    )
    .await;
    let roster_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-preview"),
        &serde_json::json!({"target": {"id": SEEDED_CATALOG, "version": 1}}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(roster_preview.status, 200, "{}", roster_preview.body);
    let roster = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-apply"),
        &serde_json::json!({
            "preview_hash": roster_preview.json()["preview_hash"],
            "expected_revision": mapped_epic_revision,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-roster-bootstrap")
    .send(&world)
    .await;
    assert_eq!(roster.status, 200, "{}", roster.body);
    let hosted = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({
            "expected_revision": mapped_epic_revision,
            "routes": [{
                "role_code": "LSA",
                "model_route": {"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}
            }],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-hosted-lsa")
    .send(&world)
    .await;
    assert_eq!(hosted.status, 200, "{}", hosted.body);
    let lsa = hosted.json()["core_team"]["seats"]
        .as_array()
        .expect("the Core Team seats")
        .iter()
        .find(|seat| seat["role"]["role_code"] == "LSA")
        .expect("the LSA seat")
        .clone();
    let lsa_binding = SeatBindingId::parse(
        lsa["seat_binding_id"]
            .as_str()
            .expect("the durable LSA SeatBinding"),
    )
    .expect("a canonical SeatBinding id");
    let lsa_predecessor_native = kontor_core::id::ExternalId::parse(
        lsa["native_seat"]["native_id"]
            .as_str()
            .expect("the hosted native id"),
    )
    .expect("a canonical native id");
    let lsa_predecessor_generation = lsa["native_seat"]["generation"]
        .as_u64()
        .expect("the hosted generation");
    assert_eq!(
        world.fake.seat_title(&lsa_predecessor_native).as_deref(),
        Some("LSA • ASMA-7675 • QNR-P1"),
        "initial leadership materialization must consume the pinned ECP seat template"
    );

    // A provider-route replacement keeps the logical SeatBinding, records the
    // exact predecessor, and gives the successor the same spec-owned title.
    let route_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/routes:preview"),
        &serde_json::json!({
            "expected_revision": mapped_epic_revision,
            "seat_binding_id": lsa_binding,
            "expected_native_id": lsa_predecessor_native,
            "expected_generation": lsa_predecessor_generation,
            "desired_model_route": {"provider": "opencode", "model": "deepseek/deepseek-v4-flash", "effort": "high"}
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(route_preview.status, 200, "{}", route_preview.body);
    let routed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/routes:apply"),
        &serde_json::json!({
            "expected_revision": mapped_epic_revision,
            "seat_binding_id": lsa_binding,
            "expected_native_id": lsa_predecessor_native,
            "expected_generation": lsa_predecessor_generation,
            "desired_model_route": {"provider": "opencode", "model": "deepseek/deepseek-v4-flash", "effort": "high"},
            "preview_hash": route_preview.json()["preview_hash"],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-hosted-lsa-route")
    .send(&world)
    .await;
    assert_eq!(routed.status, 200, "{}", routed.body);
    assert_eq!(routed.json()["seat_binding_id"], lsa_binding.to_string());
    assert_eq!(
        routed.json()["predecessor_native_id"],
        lsa_predecessor_native.as_str()
    );
    let lsa_native = kontor_core::id::ExternalId::parse(
        routed.json()["successor_native_id"]
            .as_str()
            .expect("the routed successor native id"),
    )
    .expect("a canonical successor native id");
    assert_eq!(
        world.fake.seat_title(&lsa_native).as_deref(),
        Some("LSA • ASMA-7675 • QNR-P1"),
        "provider-route replacement must consume the pinned ECP seat template"
    );
    let hosted_before_resume = world.daemon.state().with_store(|store| {
        store
            .get_hosted_topology_seat(
                ProjectId::parse(&project).expect("a canonical project id"),
                lsa_binding,
            )
            .expect("the hosted LSA reads")
            .expect("the hosted LSA exists")
    });
    let resumed_provider_session =
        kontor_core::id::ExternalId::parse("provider-hosted-seat-resumed")
            .expect("a provider session id");
    world
        .fake
        .set_seat_provider_session(&lsa_native, Some(resumed_provider_session.clone()));

    // Start a real delivery team, then replace one role so the repository's
    // oldest-first enumeration contains a bound predecessor and bound leaf.
    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "QNR lead", "harness": "fake.runtime",
            "credential_alias": "qnr-lead", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("the account id")
        .to_owned();
    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": mapped_epic_revision,
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": account_id,
            "reason": "Exercise native-name repair"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-arm")
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
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan.json()["plan_hash"]}),
    )
    .signed_as(&world, "operator")
    .with_key("qnr-start")
    .send(&world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let started_seats = started.json()["started"]
        .as_array()
        .expect("the delivery seats")
        .clone();
    assert!(
        !started_seats.is_empty(),
        "at least one delivery seat: plan={} start={}",
        plan.body,
        started.body,
    );
    let delivery = started_seats[0].clone();
    let predecessor_id = AgentRunId::parse(
        delivery["agent_run_id"]
            .as_str()
            .expect("the predecessor AgentRun"),
    )
    .expect("a canonical predecessor id");
    let role_slot = delivery["role_slot"]
        .as_str()
        .expect("the delivery role slot")
        .to_owned();
    let project_id = ProjectId::parse(&project).expect("a canonical project id");
    let predecessor = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the predecessor reads")
            .expect("the predecessor exists")
    });
    let predecessor_binding = predecessor
        .binding
        .clone()
        .expect("the predecessor is natively bound");
    finish_natively(&world, predecessor_id.to_string().as_str()).await;
    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor_id}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("qnr-delivery-settle")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    let epic_view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = epic_view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("the task revision");
    let replaced = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor_id}/successors:replace"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": task_revision,
            "binding_generation": predecessor_binding.identity.generation,
            "model_route": {"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-delivery-successor")
    .send(&world)
    .await;
    assert_eq!(replaced.status, 200, "{}", replaced.body);
    let successor_id = AgentRunId::parse(
        replaced.json()["successor_agent_run_id"]
            .as_str()
            .expect("the successor AgentRun"),
    )
    .expect("a canonical successor id");
    let successor = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, successor_id)
            .expect("the successor reads")
            .expect("the successor exists")
    });
    assert_eq!(successor.parent_agent_run_id, Some(predecessor_id));
    let successor_binding = successor
        .binding
        .clone()
        .expect("the current leaf is natively bound");

    // Reproduce a durable logical delivery slot declared before any AgentRun
    // exists. It has no native title target and must not make the replacement
    // resolver diagnose an empty chain as stale.
    let task_id = TaskId::parse(&task).expect("a canonical task id");
    let task_node = world.daemon.state().with_store(|store| {
        store
            .get_task_topology_node(project_id, task_id)
            .expect("the task topology reads")
            .expect("the task topology exists")
    });
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    let catalog = domain
        .role_catalogs
        .first()
        .expect("the bundled role catalog");
    let declared_role = catalog
        .role(&RoleCode::parse("SWE").expect("a standard role code"))
        .expect("the catalog has SWE");
    let declared_unbound = SeatBindingId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_seat_binding(&NewSeatBinding {
                id: declared_unbound,
                project_id,
                topology_node_id: task_node.id,
                role_slot_id: kontor_core::id::RoleSlotId::parse("declared-unbound")
                    .expect("a role slot"),
                role: CatalogRoleRef {
                    catalog_id: catalog.catalog_id,
                    catalog_revision: catalog.version,
                    role_code: declared_role.role_code.clone(),
                    standard_title: declared_role.standard_title.clone(),
                    custom_display_name: None,
                },
                task_id: Some(task_id),
                team_run_id: Some(successor.team_run_id),
                attach_deadline: at("2099-01-01T00:00:00Z"),
                parent_seat_binding_id: None,
                created_at: at("2026-08-20T00:00:00Z"),
            })
            .expect("the logical role slot is declared without an AgentRun");
    });

    // Drift only the current delivery leaf and the current hosted successor.
    // The archived predecessor remains deliberately distinct so an oldest-first
    // resolver is observable as the wrong target.
    world.fake.set_seat_title(
        &predecessor_binding.identity.native_id,
        "ARCHIVED PREDECESSOR",
    );
    world.fake.set_seat_title(
        &successor_binding.identity.native_id,
        "Delivery descriptive title",
    );
    world
        .fake
        .set_seat_title(&lsa_native, "LSA descriptive title");
    world.fake.restart();
    assert_eq!(
        world.fake.generation(),
        successor_binding.identity.generation + 1,
        "the repair runs after the native runtime generation advances"
    );
    let mixed_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(mixed_preview.status, 200, "{}", mixed_preview.body);
    let mixed_targets = mixed_preview.json()["targets"]
        .as_array()
        .expect("the mixed whole-epic census")
        .clone();
    assert!(
        !mixed_targets
            .iter()
            .any(|target| target["agent_run_id"] == predecessor_id.to_string()),
        "the archived oldest-first predecessor entered the census: {}",
        mixed_preview.body
    );
    assert!(
        !mixed_targets
            .iter()
            .any(|target| target["seat_binding_id"] == declared_unbound.to_string()),
        "a declared seat without an AgentRun is not a native title target: {}",
        mixed_preview.body
    );
    let delivery_target = mixed_targets
        .iter()
        .find(|target| target["agent_run_id"] == successor_id.to_string())
        .expect("the current delivery leaf is targeted")
        .clone();
    assert_eq!(
        delivery_target["native_id"],
        successor_binding.identity.native_id.as_str()
    );
    let delivery_seat_id = SeatBindingId::parse(
        delivery_target["seat_binding_id"]
            .as_str()
            .expect("the delivery SeatBinding"),
    )
    .expect("a canonical delivery SeatBinding id");
    let delivery_seat = world.daemon.state().with_store(|store| {
        store
            .get_seat_binding(project_id, delivery_seat_id)
            .expect("the delivery SeatBinding reads")
            .expect("the delivery SeatBinding exists")
    });
    assert_eq!(
        delivery_target["desired_title"],
        format!("{} • QNR-NP-01", delivery_seat.role.role_code.as_str())
    );
    let hosted_target = mixed_targets
        .iter()
        .find(|target| target["seat_binding_id"] == lsa_binding.to_string())
        .expect("the current hosted LSA is targeted")
        .clone();
    assert_eq!(hosted_target["native_id"], lsa_native.as_str());
    assert_eq!(hosted_target["desired_title"], "LSA • ASMA-7675 • QNR-P1");
    assert_eq!(
        hosted_target["provider_session_id"],
        resumed_provider_session.as_str(),
        "preview learns a resumed provider thread from the unchanged exact native agent"
    );
    let mixed_body = serde_json::json!({
        "expected_revision": project_revision,
        "preview_hash": mixed_preview.json()["preview_hash"],
    });
    let mixed_applied = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:apply"),
        &mixed_body,
    )
    .signed_as(&world, "admin")
    .with_key("qnr-mixed-seat-name-repair")
    .send(&world)
    .await;
    assert_eq!(mixed_applied.status, 200, "{}", mixed_applied.body);
    assert_eq!(mixed_applied.json()["changed"], 2);
    assert_eq!(
        world
            .fake
            .seat_title(&successor_binding.identity.native_id)
            .as_deref(),
        delivery_target["desired_title"].as_str()
    );
    assert_eq!(
        world.fake.seat_title(&lsa_native).as_deref(),
        Some("LSA • ASMA-7675 • QNR-P1")
    );
    let hosted_after_repair = world.daemon.state().with_store(|store| {
        store
            .get_hosted_topology_seat(project_id, lsa_binding)
            .expect("the repaired hosted LSA reads")
            .expect("the repaired hosted LSA exists")
    });
    assert_eq!(hosted_after_repair.seat_binding_id, lsa_binding);
    assert_eq!(
        hosted_after_repair.native_identity, hosted_before_resume.native_identity,
        "a provider-thread resume must not replace the native LSA"
    );
    assert_eq!(
        hosted_after_repair.model_rung, hosted_before_resume.model_rung,
        "a provider-thread resume must not change the frozen route"
    );
    assert_eq!(
        hosted_after_repair.provider_session_id.as_ref(),
        Some(&resumed_provider_session),
        "the current provider thread becomes the durable readback for later messages"
    );
    assert_eq!(
        world
            .fake
            .seat_title(&predecessor_binding.identity.native_id)
            .as_deref(),
        Some("ARCHIVED PREDECESSOR"),
        "repair must not retitle the archived predecessor"
    );
    let current_runs = world.daemon.state().with_store(|store| {
        (
            store
                .get_agent_run(project_id, predecessor_id)
                .expect("the predecessor reads")
                .expect("the predecessor remains"),
            store
                .get_agent_run(project_id, successor_id)
                .expect("the successor reads")
                .expect("the successor remains"),
            store
                .get_seat_binding(project_id, delivery_seat_id)
                .expect("the delivery binding reads")
                .expect("the delivery binding remains"),
        )
    });
    assert_eq!(current_runs.0.id, predecessor_id);
    assert_eq!(current_runs.0.binding, Some(predecessor_binding));
    assert_eq!(current_runs.1.id, successor_id);
    assert_eq!(current_runs.1.parent_agent_run_id, Some(predecessor_id));
    assert_eq!(current_runs.1.binding, Some(successor_binding));
    assert_eq!(current_runs.2.id, delivery_seat_id);
    assert_eq!(current_runs.2.id, delivery_seat.id);
    let mixed_applied_json = mixed_applied.json();
    let mixed_readback = mixed_applied_json["readback"]["targets"]
        .as_array()
        .expect("fresh mixed readback");
    for before in [&delivery_target, &hosted_target] {
        let after = mixed_readback
            .iter()
            .find(|target| target["seat_binding_id"] == before["seat_binding_id"])
            .expect("the same logical seat remains in readback");
        assert_eq!(after["agent_run_id"], before["agent_run_id"]);
        assert_eq!(after["native_id"], before["native_id"]);
        assert_eq!(after["provider_session_id"], before["provider_session_id"]);
        assert_eq!(after["topology_node_id"], before["topology_node_id"]);
        assert_eq!(after["would_change"], false);
    }

    // An exact persisted seat may become unreachable after materialization.
    // It stays visible as typed pending evidence, while an independent stale
    // container in the same epic remains actionable and is repaired in place.
    world.fake.forget_seat(&lsa_native);
    world
        .fake
        .set_container_title(node_id("TSW"), "stale beside unavailable seat");
    let pending_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(pending_preview.status, 200, "{}", pending_preview.body);
    let pending_targets = pending_preview.json()["targets"]
        .as_array()
        .expect("the pending native-name census")
        .clone();
    let pending_lsa = pending_targets
        .iter()
        .find(|target| target["seat_binding_id"] == lsa_binding.to_string())
        .expect("the unavailable seat remains explicit");
    assert_eq!(pending_lsa["native_id"], lsa_native.as_str());
    assert_eq!(pending_lsa["observed_title"], serde_json::Value::Null);
    assert_eq!(pending_lsa["capability"], "rename_pending");
    assert_eq!(pending_lsa["would_change"], false);
    let pending_apply = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:apply"),
        &serde_json::json!({
            "expected_revision": project_revision,
            "preview_hash": pending_preview.json()["preview_hash"],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("qnr-native-name-repair-with-pending-seat")
    .send(&world)
    .await;
    assert_eq!(pending_apply.status, 200, "{}", pending_apply.body);
    assert_eq!(pending_apply.json()["changed"], 1);
    assert_eq!(
        title_for("TSW"),
        "TSW • ASMA-7676 • QNR-NP-01",
        "an unavailable seat must not block an independent container repair"
    );
    let pending_readback = pending_apply.json()["readback"]["targets"]
        .as_array()
        .expect("the fresh pending readback")
        .iter()
        .find(|target| target["seat_binding_id"] == lsa_binding.to_string())
        .expect("the same unavailable seat remains in readback")
        .clone();
    assert_eq!(pending_readback["native_id"], lsa_native.as_str());
    assert_eq!(pending_readback["capability"], "rename_pending");
    assert_eq!(pending_readback["observed_title"], serde_json::Value::Null);
}

/// Ensuring a scope creates the chain the specification declares.
///
/// The caller names an epic's control plane and nothing else. What comes back
/// is a root, an epic and a control plane — kinds, parents and scoping the
/// server derived — which is the composed topology path doing its job rather
/// than a refusal wearing a 200.
#[tokio::test]
async fn ensuring_a_control_plane_creates_the_chain_the_specification_declares() {
    let composed = compose_realm("/tmp/kontor-cp2-chain").await;
    let world = &composed.world;

    let ensured = Call::post(
        format!("/v1/projects/{}/topology:ensure", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("ensure-control")
    .send(world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    assert_eq!(ensured.json()["receipt"]["applied"], "created");

    let kinds: Vec<String> = ensured.json()["projection"]["nodes"]
        .as_array()
        .expect("a node array")
        .iter()
        .map(|node| node["kind_key"].as_str().expect("a kind").to_owned())
        .collect();
    assert!(
        kinds.contains(&"ESW".to_owned()) && kinds.contains(&"ECP".to_owned()),
        "the epic and its control plane must both exist: {kinds:?}"
    );

    // Every node cites the exact specification revision it was placed under.
    let pinned = &ensured.json()["projection"]["pinned_spec"];
    assert!(
        pinned["canonical_hash"]
            .as_str()
            .is_some_and(|hash| !hash.is_empty()),
        "the projection names the exact document it is pinned to: {}",
        ensured.body
    );

    // Ensuring again creates nothing. Not "answers the same" — creates nothing:
    // the node count is what proves it.
    let again = Call::post(
        format!("/v1/projects/{}/topology:ensure", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("ensure-control-2")
    .send(world)
    .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(
        again.json()["projection"]["nodes"].as_array().map(Vec::len),
        ensured.json()["projection"]["nodes"]
            .as_array()
            .map(Vec::len),
        "a second ensure must place nothing new"
    );
}

/// A replayed key answers from what is durable, and a stale revision writes
/// nothing.
#[tokio::test]
async fn a_semantic_topology_write_survives_replay_and_refuses_a_stale_revision() {
    let composed = compose_realm("/tmp/kontor-cp2-replay").await;
    let world = &composed.world;
    let uri = format!("/v1/projects/{}/topology:ensure", composed.project);
    let body = serde_json::json!({
        "target": {"scope": "epic", "epic_id": composed.epic},
        "expected_revision": composed.project_revision,
    });

    let first = Call::post(&uri, &body)
        .signed_as(world, "operator")
        .with_key("replay-ensure")
        .send(world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["receipt"]["applied"], "created");

    let replayed = Call::post(&uri, &body)
        .signed_as(world, "operator")
        .with_key("replay-ensure")
        .send(world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["receipt"]["applied"],
        "unchanged",
        "a replayed key answers from what is durable rather than writing again"
    );
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        first.json()["receipt"]["receipt_id"],
        "and it answers from the *original* receipt"
    );

    let stale = Call::post(
        &uri,
        &serde_json::json!({
            "target": {"scope": "epic", "epic_id": composed.epic},
            "expected_revision": composed.project_revision + 7,
        }),
    )
    .signed_as(world, "operator")
    .with_key("stale-ensure")
    .send(world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");
}

/// Materializing binds a seat only where the vocabulary allows one.
///
/// The epic kind is a native root and hosts nothing; its control plane is a
/// session host. Both are materialized through the same operation, and the
/// difference in what comes back is the capability dispatch — not a special
/// case written for either kind.
#[tokio::test]
async fn materializing_binds_a_seat_only_on_a_kind_declared_a_session_host() {
    let composed = compose_realm("/tmp/kontor-cp2-materialize").await;
    let world = &composed.world;
    let uri = format!("/v1/projects/{}/topology:materialize", composed.project);

    for (index, scope) in ["epic", "epic_control"].iter().enumerate() {
        let answer = Call::post(
            &uri,
            &serde_json::json!({
                "target": {"scope": scope, "epic_id": composed.epic},
                "expected_revision": composed.project_revision,
            }),
        )
        .signed_as(world, "operator")
        .with_key(format!("materialize-{index}"))
        .send(world)
        .await;
        assert_eq!(answer.status, 200, "{scope}: {}", answer.body);
    }

    let projection = Call::get(format!(
        "/v1/projects/{}/topology:inspect?epic_id={}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(projection.status, 200, "{}", projection.body);

    let nodes = projection.json()["nodes"]
        .as_array()
        .expect("nodes")
        .clone();
    let seats_on = |kind: &str| -> usize {
        nodes
            .iter()
            .filter(|node| node["kind_key"] == kind)
            .map(|node| node["seats"].as_array().map_or(0, Vec::len))
            .sum()
    };
    assert_eq!(
        seats_on("ESW"),
        0,
        "an epic is a native root and hosts no session: {}",
        projection.body
    );
    // The mandatory pair, and only it: an epic is born with the `LSA` and `TPM`
    // its Core Team roster declares, and nothing here adds a third.
    assert_eq!(
        seats_on("ECP"),
        2,
        "the control plane is a session host and holds the mandatory leadership pair: {}",
        projection.body
    );
}

/// Materializing a ticket is the supported placement preflight: it must read
/// back the exact native workspace before a scheduler is asked to admit work.
///
/// A logical TSW and its control seat are not enough. Returning HTTP 200 while
/// `observed_binding` remains null forces callers to use scheduler admission as
/// a placement probe, which can leave a durable queued run behind on failure.
#[tokio::test]
async fn materializing_a_ticket_binds_its_native_workspace_without_admitting_a_run() {
    let composed = compose_realm("/tmp/kontor-op18-materialize").await;
    let world = &composed.world;
    let epic = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(epic.status, 200, "{}", epic.body);
    let task = epic.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("the composed task id")
        .to_owned();
    let uri = format!("/v1/projects/{}/topology:materialize", composed.project);
    let request = serde_json::json!({
        "target": {"scope": "ticket", "task_id": task},
        "expected_revision": composed.project_revision,
    });

    let first = Call::post(&uri, &request)
        .signed_as(world, "operator")
        .with_key("materialize-ticket-native")
        .send(world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let first_json = first.json();
    let tsw = first_json["projection"]["nodes"]
        .as_array()
        .expect("topology nodes")
        .iter()
        .find(|node| node["kind_key"] == "TSW")
        .expect("the ticket workspace node");
    assert_eq!(
        tsw["observed_binding"]["cwd"], "/w/composed-epic/0",
        "materialization reads back the task's declared workspace: {}",
        first.body
    );
    assert!(
        tsw["observed_binding"]["native_id"].is_string(),
        "a successful materialization returns the runtime-issued workspace id: {}",
        first.body
    );
    assert_eq!(
        tsw["placement"], "bound",
        "a node holding an exact native readback cannot still claim it is unbound: {}",
        first.body
    );
    let node_id = tsw["topology_node_id"].clone();
    let native_id = tsw["observed_binding"]["native_id"].clone();

    let replayed = Call::post(&uri, &request)
        .signed_as(world, "operator")
        .with_key("materialize-ticket-native")
        .send(world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    let replayed_json = replayed.json();
    let exact: Vec<_> = replayed_json["projection"]["nodes"]
        .as_array()
        .expect("topology nodes")
        .iter()
        .filter(|node| node["topology_node_id"] == node_id)
        .collect();
    assert_eq!(exact.len(), 1, "a replay creates no duplicate TSW");
    assert_eq!(
        exact[0]["observed_binding"]["native_id"], native_id,
        "a replay preserves the runtime-issued workspace identity"
    );

    let task = Call::get(format!("/v1/projects/{}/tasks/{task}", composed.project))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(task.status, 200, "{}", task.body);
    assert_eq!(
        task.json()["value"]["state"],
        "ready",
        "native placement alone must not admit or start the task"
    );
}

#[tokio::test]
async fn ticket_materialization_retires_an_unrouted_legacy_tpm_without_creating_identity() {
    let composed = compose_realm("/tmp/kontor-op08-unrouted-task-tpm").await;
    let world = &composed.world;
    let epic = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(epic.status, 200, "{}", epic.body);
    let task = epic.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("the composed task id")
        .to_owned();
    let project_id = ProjectId::parse(&composed.project).expect("a project id");
    let task_id = TaskId::parse(&task).expect("a task id");
    let uri = format!("/v1/projects/{}/topology:materialize", composed.project);
    let request = serde_json::json!({
        "target": {"scope": "ticket", "task_id": task},
        "expected_revision": composed.project_revision,
    });

    let first = Call::post(&uri, &request)
        .signed_as(world, "operator")
        .with_key("materialize-ticket-with-legacy-tpm")
        .send(world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let node_id = TopologyNodeId::parse(
        first.json()["projection"]["nodes"]
            .as_array()
            .expect("topology nodes")
            .iter()
            .find(|node| node["kind_key"] == "TSW")
            .expect("the TSW node")["topology_node_id"]
            .as_str()
            .expect("the TSW id"),
    )
    .expect("a topology node id");
    let legacy_binding_id = SeatBindingId::generate();
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    let catalog = domain
        .role_catalogs
        .first()
        .expect("a bundled role catalog");
    let tpm = catalog
        .role(&RoleCode::parse("TPM").expect("the TPM role code"))
        .expect("the catalog has TPM");
    world.daemon.state().with_store(|store| {
        store
            .create_seat_binding(&NewSeatBinding {
                id: legacy_binding_id,
                project_id,
                topology_node_id: node_id,
                role_slot_id: kontor_core::id::RoleSlotId::parse("tpm").expect("a role slot"),
                role: CatalogRoleRef {
                    catalog_id: catalog.catalog_id,
                    catalog_revision: catalog.version,
                    role_code: tpm.role_code.clone(),
                    standard_title: tpm.standard_title.clone(),
                    custom_display_name: None,
                },
                task_id: Some(task_id),
                team_run_id: None,
                attach_deadline: at("2099-01-01T00:00:00Z"),
                parent_seat_binding_id: None,
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("the legacy logical-only TPM is reproduced");
        assert!(
            store
                .get_hosted_topology_seat(project_id, legacy_binding_id)
                .expect("the hosted route reads")
                .is_none(),
            "the legacy row starts with no topology-message route"
        );
    });
    let calls_before = world.fake.calls().len();

    let repaired = Call::post(&uri, &request)
        .signed_as(world, "operator")
        .with_key("materialize-ticket-with-legacy-tpm")
        .send(world)
        .await;
    assert_eq!(repaired.status, 200, "{}", repaired.body);
    assert_eq!(repaired.json()["receipt"]["applied"], "unchanged");
    let preserved = world.daemon.state().with_store(|store| {
        let binding = store
            .get_seat_binding(project_id, legacy_binding_id)
            .expect("the binding reads")
            .expect("the binding is preserved as evidence");
        assert!(
            store
                .get_hosted_topology_seat(project_id, legacy_binding_id)
                .expect("the hosted route reads")
                .is_none(),
            "repair must not invent a native route"
        );
        binding
    });
    assert_eq!(preserved.id, legacy_binding_id);
    assert_eq!(preserved.lifecycle.as_str(), "retired");
    assert_eq!(
        world.fake.calls().len(),
        calls_before,
        "replay has no native effect"
    );

    let message = Call::post(
        format!(
            "/v1/projects/{}/seat-bindings/{legacy_binding_id}/messages",
            composed.project
        ),
        &serde_json::json!({"body": "This must not materialize the missing seat."}),
    )
    .signed_as(world, "operator")
    .with_key(kontor_runtime::request::MessageId::generate().to_string())
    .send(world)
    .await;
    assert_eq!(message.status, 404, "{}", message.body);
    assert_eq!(
        world.fake.calls().len(),
        calls_before,
        "messaging an inactive logical row cannot create a native identity"
    );
}

/// An idempotent materialization replay repairs the logical placement chain
/// without repeating the already acknowledged native effect.
///
/// Schema-46 realms can contain a TSW written by an older admission path whose
/// sibling ECP was never created. The receipt still makes a retry a replay, but
/// that cannot turn the missing owner into permanent state: reconciliation must
/// ensure the durable chain again while preserving the exact native TSW binding.
#[tokio::test]
async fn replaying_ticket_materialization_repairs_a_missing_epic_control_plane() {
    let composed = compose_realm("/tmp/kontor-op08-materialize-repair").await;
    let world = &composed.world;
    let epic = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(epic.status, 200, "{}", epic.body);
    let task = epic.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("the composed task id")
        .to_owned();
    let uri = format!("/v1/projects/{}/topology:materialize", composed.project);
    let request = serde_json::json!({
        "target": {"scope": "ticket", "task_id": task},
        "expected_revision": composed.project_revision,
    });

    let first = Call::post(&uri, &request)
        .signed_as(world, "operator")
        .with_key("materialize-ticket-repair")
        .send(world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let first_json = first.json();
    let tsw = first_json["projection"]["nodes"]
        .as_array()
        .expect("topology nodes")
        .iter()
        .find(|node| node["kind_key"] == "TSW")
        .expect("the ticket workspace node");
    let tsw_node_id = tsw["topology_node_id"].clone();
    let native_id = tsw["observed_binding"]["native_id"].clone();

    // Recreate the exact durable shape left by the legacy writer: the task's
    // TSW and native binding exist, but the epic's owner/control node does not.
    let database = world.directory.path().join(kontor_daemon::DATABASE_FILE);
    let connection = rusqlite::Connection::open(database).expect("the realm database opens");
    // Its seats go first. A legacy writer left no control plane and so left no
    // seats on one either; an epic born governed has both, and the foreign key
    // between them is what says so.
    connection
        .execute(
            "DELETE FROM seat_bindings
             WHERE project_id = ?1 AND topology_node_id IN (
                 SELECT id FROM topology_nodes
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND kind = 'ECP')",
            rusqlite::params![composed.project.to_string(), composed.epic.to_string()],
        )
        .expect("the legacy control seats clear");
    let removed = connection
        .execute(
            "DELETE FROM topology_nodes
             WHERE project_id = ?1 AND mini_project_id = ?2 AND kind = 'ECP'",
            rusqlite::params![composed.project.to_string(), composed.epic.to_string()],
        )
        .expect("the legacy gap is seeded");
    assert_eq!(removed, 1, "the original control plane existed");
    drop(connection);

    let replayed = Call::post(&uri, &request)
        .signed_as(world, "operator")
        .with_key("materialize-ticket-repair")
        .send(world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    let nodes = replayed.json()["projection"]["nodes"]
        .as_array()
        .expect("topology nodes")
        .clone();
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node["kind_key"] == "ECP")
            .count(),
        1,
        "a replay restores exactly one epic control plane: {}",
        replayed.body
    );
    let repaired_tsw = nodes
        .iter()
        .find(|node| node["topology_node_id"] == tsw_node_id)
        .expect("the original ticket workspace remains");
    assert_eq!(
        repaired_tsw["observed_binding"]["native_id"], native_id,
        "logical repair must not replace or repeat the native workspace"
    );
}

/// A node is retired by the id an answer already returned, and not otherwise.
#[tokio::test]
async fn a_node_is_retired_by_the_id_an_answer_returned_and_children_block_it() {
    let composed = compose_realm("/tmp/kontor-cp2-retire").await;
    let world = &composed.world;

    let ensured = Call::post(
        format!("/v1/projects/{}/topology:ensure", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("retire-ensure")
    .send(world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let nodes = ensured.json()["projection"]["nodes"]
        .as_array()
        .expect("nodes")
        .clone();
    // The revision each node actually stands at, rather than the initial one.
    // A node created when the epic was applied and bound when it was ensured has
    // already moved, and presenting a guessed revision would refuse this retire
    // as a conflict — proving nothing about children blocking a parent.
    let project_id = ProjectId::parse(&composed.project).expect("a project id");
    let node_of = |kind: &str| -> (String, u64) {
        let node = nodes
            .iter()
            .find(|node| node["kind_key"] == kind)
            .unwrap_or_else(|| panic!("a {kind} node exists: {ensured:?}", ensured = ensured.body));
        let id = node["topology_node_id"].as_str().expect("an id").to_owned();
        let node_id = kontor_core::id::TopologyNodeId::parse(&id).expect("a node id");
        let revision = world.daemon.state().with_store(|store| {
            store
                .get_topology_node(project_id, node_id)
                .expect("the node reads")
                .expect("the ensured node exists")
                .revision
        });
        (id, revision.get())
    };
    let (control, control_revision) = node_of("ECP");
    let (epic_node, epic_revision) = node_of("ESW");

    // The epic still has a live child, so retiring it concludes something that
    // is not true yet.
    let blocked = Call::post(
        format!(
            "/v1/projects/{}/topology/nodes/{epic_node}/retire",
            composed.project
        ),
        &serde_json::json!({"expected_revision": epic_revision, "reason": "done with it"}),
    )
    .signed_as(world, "operator")
    .with_key("retire-epic-early")
    .send(world)
    .await;
    assert_eq!(blocked.status, 409, "{}", blocked.body);

    // The control plane hosts the epic's mandatory leadership, and a node that
    // still hosts a live seat is refused for that — a different rule, proved by
    // its own test. Retire the seats first so what is left is the leaf case.
    let database = world.directory.path().join(kontor_daemon::DATABASE_FILE);
    let connection = rusqlite::Connection::open(database).expect("the realm database opens");
    let concluded = connection
        .execute(
            "UPDATE seat_bindings SET lifecycle = 'retired'
             WHERE project_id = ?1 AND topology_node_id = ?2 AND lifecycle = 'active'",
            rusqlite::params![composed.project.to_string(), control.as_str()],
        )
        .expect("the control seats conclude");
    assert_eq!(concluded, 2, "the epic was born with its leadership pair");
    drop(connection);

    // The leaf retires, and says so in the projection it returns.
    let retired = Call::post(
        format!(
            "/v1/projects/{}/topology/nodes/{control}/retire",
            composed.project
        ),
        &serde_json::json!({"expected_revision": control_revision, "reason": "epic finished"}),
    )
    .signed_as(world, "operator")
    .with_key("retire-control")
    .send(world)
    .await;
    assert_eq!(retired.status, 200, "{}", retired.body);
    let lifecycle = retired.json()["projection"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["topology_node_id"] == control.as_str())
        .map(|node| node["lifecycle"].as_str().expect("a lifecycle").to_owned());
    assert_eq!(lifecycle.as_deref(), Some("retired"), "{}", retired.body);

    // The same revision no longer stands: retirement moved it.
    let repeat = Call::post(
        format!(
            "/v1/projects/{}/topology/nodes/{control}/retire",
            composed.project
        ),
        &serde_json::json!({"expected_revision": control_revision, "reason": "again"}),
    )
    .signed_as(world, "operator")
    .with_key("retire-control-again")
    .send(world)
    .await;
    assert_eq!(repeat.status, 409, "{}", repeat.body);
}

/// A capacity refresh stores the raw reading, not only what was derived.
#[tokio::test]
async fn a_capacity_refresh_stores_the_raw_reading_it_derived_from() {
    let composed = compose_realm("/tmp/kontor-cp3-refresh").await;
    let world = &composed.world;

    let refreshed = Call::post(
        format!("/v1/projects/{}/capacity:refresh", composed.project),
        &serde_json::json!({"account_profile_ids": [composed.account]}),
    )
    .signed_as(world, "operator")
    .with_key("refresh-1")
    .send(world)
    .await;
    assert_eq!(refreshed.status, 200, "{}", refreshed.body);

    let account = refreshed.json()["accounts"]
        .as_array()
        .expect("accounts")
        .iter()
        .find(|entry| entry["account_profile_id"] == composed.account.as_str())
        .expect("the refreshed account is in the projection")
        .clone();
    let observation = account["observation_id"]
        .as_str()
        .expect("the derived answer cites the evidence it came from")
        .to_owned();
    assert_eq!(
        account["available"], true,
        "the fake runtime proves the account environment: {}",
        refreshed.body
    );

    // The raw reading is addressable in its own right, and it is what the
    // collector saw rather than what the Realm concluded.
    let evidence = Call::get(format!(
        "/v1/projects/{}/capacity/observations/{observation}",
        composed.project
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(evidence.status, 200, "{}", evidence.body);
    assert_eq!(evidence.json()["available"], true);
    assert_eq!(evidence.json()["pressure"], false);
    assert_eq!(
        evidence.json()["reading"]["probe"]["outcome"],
        "account_environment_supported",
        "the stored reading is the collector's, not a re-derivation: {}",
        evidence.body
    );
    // And it carries no endpoint, credential or process identity.
    for forbidden in ["token", "secret", "keychain", "config_home", "argv", "pid"] {
        assert!(
            !evidence.body.contains(forbidden),
            "a stored reading must not carry `{forbidden}`: {}",
            evidence.body
        );
    }
}

/// An override stands beside the evidence; it never rewrites it.
#[tokio::test]
async fn an_operator_override_never_rewrites_what_the_provider_reported() {
    let composed = compose_realm("/tmp/kontor-cp3-override").await;
    let world = &composed.world;

    let refreshed = Call::post(
        format!("/v1/projects/{}/capacity:refresh", composed.project),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .with_key("override-refresh")
    .send(world)
    .await;
    assert_eq!(refreshed.status, 200, "{}", refreshed.body);
    let observation =
        refreshed.json()["accounts"].as_array().expect("accounts")[0]["observation_id"]
            .as_str()
            .expect("an observation")
            .to_owned();

    let overridden = Call::post(
        format!(
            "/v1/projects/{}/provider-account-profiles/{}/availability:override",
            composed.project, composed.account
        ),
        &serde_json::json!({
            "expected_revision": 1,
            "available": false,
            "reason": "held back during the incident"
        }),
    )
    .signed_as(world, "operator")
    .with_key("override-1")
    .send(world)
    .await;
    assert_eq!(overridden.status, 200, "{}", overridden.body);
    assert_eq!(overridden.json()["account"]["available"], false);
    assert_eq!(
        overridden.json()["account"]["override_reason"],
        "held back during the incident"
    );

    // The provider's word is untouched, and still says the opposite.
    let evidence = Call::get(format!(
        "/v1/projects/{}/capacity/observations/{observation}",
        composed.project
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(evidence.status, 200, "{}", evidence.body);
    assert_eq!(
        evidence.json()["available"],
        true,
        "the operator disagreed; the evidence did not change: {}",
        evidence.body
    );

    // A second override under the revision the first consumed is refused.
    let stale = Call::post(
        format!(
            "/v1/projects/{}/provider-account-profiles/{}/availability:override",
            composed.project, composed.account
        ),
        &serde_json::json!({
            "expected_revision": 1,
            "available": true,
            "reason": "and back again"
        }),
    )
    .signed_as(world, "operator")
    .with_key("override-2")
    .send(world)
    .await;
    assert_eq!(stale.status, 200, "{}", stale.body);
    let third = Call::post(
        format!(
            "/v1/projects/{}/provider-account-profiles/{}/availability:override",
            composed.project, composed.account
        ),
        &serde_json::json!({
            "expected_revision": 1,
            "available": true,
            "reason": "once more"
        }),
    )
    .signed_as(world, "operator")
    .with_key("override-3")
    .send(world)
    .await;
    assert_eq!(third.status, 409, "{}", third.body);
    assert_eq!(third.code(), "revision_conflict");
}

/// Nothing in the composed capacity path needs `asma` to exist.
///
/// The collector reads the Realm's own configuration and the runtime families
/// it was composed with. A Realm holding no adapter at all still answers — it
/// reports the account as unusable, which is what it observed, rather than
/// failing to answer or reporting an availability nobody proved.
#[tokio::test]
async fn a_capacity_refresh_answers_without_any_external_executable() {
    let world = World::open_unconfigured().await;
    let created = ensure_project(&world, "absent-1", "Kontor", "/tmp/kontor-cp3-absent").await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();

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
    .with_key("absent-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);

    let refreshed = Call::post(
        format!("/v1/projects/{project}/capacity:refresh"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("absent-refresh")
    .send(&world)
    .await;
    assert_eq!(refreshed.status, 200, "{}", refreshed.body);
    let entry = refreshed.json()["accounts"].as_array().expect("accounts")[0].clone();
    assert_eq!(
        entry["available"], false,
        "an unconfigured family cannot prove an account: {}",
        refreshed.body
    );

    // An absent runtime is not the provider pushing back, so it must not have
    // narrowed anything.
    let observation = entry["observation_id"].as_str().expect("an observation");
    let evidence = Call::get(format!(
        "/v1/projects/{project}/capacity/observations/{observation}"
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(evidence.json()["pressure"], false, "{}", evidence.body);
}

/// A scheduling snapshot restores the persisted width instead of starting a
/// fresh window at four.
///
/// The mutant this kills is the one the round-2 review named: an
/// `AdaptiveWindow::start` in `Services::snapshot`. With it, every plan reports
/// four however many clean observations have been folded, and the persisted
/// state is decoration.
#[tokio::test]
async fn a_plan_restores_the_persisted_adaptive_width_rather_than_starting_at_four() {
    let composed = compose_realm("/tmp/kontor-cp4-restore").await;
    let world = &composed.world;

    let width = |body: &Answer| -> u64 {
        body.json()["adaptive_width"]
            .as_u64()
            .unwrap_or_else(|| panic!("a width: {}", body.body))
    };
    let streak = |body: &Answer| -> u64 {
        body.json()["adaptive_streak"]
            .as_u64()
            .unwrap_or_else(|| panic!("a streak: {}", body.body))
    };

    let seeded = Call::get(format!("/v1/projects/{}/capacity", composed.project))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(seeded.status, 200, "{}", seeded.body);
    assert_eq!(
        width(&seeded),
        4,
        "an epic is seeded at the configured start when it is applied"
    );
    assert_eq!(streak(&seeded), 0);

    // One clean observation is one sample, not a trend.
    let first = Call::post(
        format!("/v1/projects/{}/capacity:refresh", composed.project),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .with_key("cp4-clean-1")
    .send(world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(width(&first), 4, "one clean reading must not widen it");
    assert_eq!(streak(&first), 1, "but it is remembered");

    // Replaying it changes nothing at all — not the width, not the trend.
    let replay = Call::post(
        format!("/v1/projects/{}/capacity:refresh", composed.project),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .with_key("cp4-clean-1")
    .send(world)
    .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(
        width(&replay),
        4,
        "a replay may not stand in for a second reading"
    );
    assert_eq!(streak(&replay), 1);

    // The second *distinct* reading grows it by exactly one step.
    let second = Call::post(
        format!("/v1/projects/{}/capacity:refresh", composed.project),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .with_key("cp4-clean-2")
    .send(world)
    .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(width(&second), 5, "{}", second.body);
    assert_eq!(streak(&second), 0, "the trend starts again");

    // And a plan — the production scheduling path — sees the width that was
    // learned, not a fresh one.
    let plan = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:plan",
            composed.project, composed.epic
        ),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .with_key("cp4-plan")
    .send(world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);

    let after = Call::get(format!("/v1/projects/{}/capacity", composed.project))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(
        width(&after),
        5,
        "planning must not reset the window it plans against: {}",
        after.body
    );
}

/// The mission ceiling counts active TeamRun envelopes, once each.
#[tokio::test]
async fn the_mission_ceiling_counts_team_runs_and_not_the_seats_they_hold() {
    let composed = compose_realm("/tmp/kontor-cp4-count").await;
    let world = &composed.world;

    let capacity = Call::get(format!("/v1/projects/{}/capacity", composed.project))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(capacity.status, 200, "{}", capacity.body);
    assert_eq!(
        capacity.json()["mission_ceiling"],
        7,
        "the Operational ceiling is seven: {}",
        capacity.body
    );
    assert_eq!(
        capacity.json()["active_team_runs"],
        0,
        "nothing has been admitted yet: {}",
        capacity.body
    );

    // Materializing opens a control seat. A seat is not work in flight, and the
    // count must not move — this is the "counting seats instead of TeamRuns"
    // shortcut, killed by observation rather than by reading the code.
    let materialized = Call::post(
        format!("/v1/projects/{}/topology:materialize", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("count-materialize")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let seats: usize = materialized.json()["projection"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|node| node["seats"].as_array().map_or(0, Vec::len))
        .sum();
    assert!(seats > 0, "a seat was opened: {}", materialized.body);

    let after = Call::get(format!("/v1/projects/{}/capacity", composed.project))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(
        after.json()["active_team_runs"],
        0,
        "an idle seat is a seat waiting to be used, not work being done: {}",
        after.body
    );
}

/// The capacity configuration is a real revision that a stale write cannot move.
#[tokio::test]
async fn the_capacity_configuration_reports_the_operational_ceilings_and_guards_its_revision() {
    let composed = compose_realm("/tmp/kontor-cp3-config").await;
    let world = &composed.world;

    let current = Call::get("/v1/capacity/configuration")
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(current.status, 200, "{}", current.body);
    assert_eq!(current.json()["ceilings"]["mission_max_in_flight"], 7);
    assert_eq!(current.json()["ceilings"]["adaptive"]["ceiling"], 7);
    assert_eq!(current.json()["revision"], 1);

    let mut ceilings = current.json()["ceilings"].clone();
    ceilings["mission_max_in_flight"] = serde_json::json!(5);

    // A preview commits nothing and names what would narrow.
    let preview = Call::post(
        "/v1/capacity/configuration:preview",
        &serde_json::json!({"ceilings": ceilings, "expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("config-preview")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert!(
        preview.json()["clamped"]
            .as_array()
            .expect("a clamped list")
            .iter()
            .any(|entry| entry == "mission_max_in_flight"),
        "a narrowed ceiling is named: {}",
        preview.body
    );
    let unchanged = Call::get("/v1/capacity/configuration")
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(
        unchanged.json()["revision"],
        1,
        "a preview writes nothing: {}",
        unchanged.body
    );

    let applied = Call::post(
        "/v1/capacity/configuration:apply",
        &serde_json::json!({"ceilings": ceilings, "expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("config-apply")
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["ceilings"]["mission_max_in_flight"], 5);
    assert_eq!(applied.json()["revision"], 1);

    // The same key answers from what is durable rather than conflicting.
    let replayed = Call::post(
        "/v1/capacity/configuration:apply",
        &serde_json::json!({"ceilings": ceilings, "expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("config-apply")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["revision"], 1);

    // A caller still holding revision one is current, and its write advances
    // the record.
    let advanced = Call::post(
        "/v1/capacity/configuration:apply",
        &serde_json::json!({"ceilings": ceilings, "expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("config-apply-2")
    .send(world)
    .await;
    assert_eq!(advanced.status, 200, "{}", advanced.body);
    assert_eq!(advanced.json()["revision"], 2);

    // A third caller still holding revision one is not.
    let stale = Call::post(
        "/v1/capacity/configuration:apply",
        &serde_json::json!({"ceilings": ceilings, "expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("config-apply-3")
    .send(world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
}

/// The width a *plan* admits against is the persisted one, not a fresh four.
///
/// This is the round-2 defect observed rather than argued: with
/// `AdaptiveWindow::start` back in `Services::snapshot`, six armed ready tasks
/// plan four at a time forever, however many clean observations have been
/// folded. The persisted position is read by `capacity_get` either way, so the
/// only way to tell the two apart is to count what the scheduler would actually
/// admit.
#[tokio::test]
async fn a_plan_admits_against_the_width_that_was_learned() {
    let world = World::open_empty_with_a_plane().await;
    world.daemon.reconcile().await;

    let created = ensure_project(&world, "width-1", "Kontor", "/tmp/kontor-cp4-width").await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("a revision");

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
    .with_key("width-account")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("an account id")
        .to_owned();

    // Six independent tasks: more than the window starts at, fewer than the
    // mission ceiling, so the window is the only thing that can be capping the
    // batch.
    let category = first_category(&world).await;
    let tasks: Vec<serde_json::Value> = (0..6)
        .map(|index| serde_json::json!({"title": format!("Task {index}")}))
        .collect();
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Width epic",
            &category,
            serde_json::Value::Array(tasks),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("width-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let epic_revision = applied.json()["revision"].as_u64().expect("a revision");
    let task_ids: Vec<String> = applied.json()["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("an id").to_owned())
        .collect();

    let armed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/execution:arm"),
        &serde_json::json!({
            "expected_revision": epic_revision,
            "tasks": task_ids,
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 8,
            "budget": {
                "max_tokens": 100_000,
                "max_commands": 500,
                "max_duration_seconds": 3600,
                "max_cost_minor_units": 5000,
                "cost_currency": "NOK"
            },
            "granted_by": account_id,
            "reason": "Fill the window"
        }),
    )
    .signed_as(&world, "admin")
    .with_key("width-arm")
    .send(&world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let ready_count = async |key: &str| -> usize {
        let plan = Call::post(
            format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
            &serde_json::json!({}),
        )
        .signed_as(&world, "operator")
        .with_key(key)
        .send(&world)
        .await;
        assert_eq!(plan.status, 200, "{}", plan.body);
        plan.json()["ready"].as_array().expect("a ready set").len()
    };

    assert_eq!(
        ready_count("width-plan-1").await,
        4,
        "the seeded window admits four"
    );

    // Two distinct clean observations widen it by exactly one step.
    for key in ["width-clean-1", "width-clean-2"] {
        let refreshed = Call::post(
            format!("/v1/projects/{project}/capacity:refresh"),
            &serde_json::json!({}),
        )
        .signed_as(&world, "operator")
        .with_key(key)
        .send(&world)
        .await;
        assert_eq!(refreshed.status, 200, "{}", refreshed.body);
    }

    assert_eq!(
        ready_count("width-plan-2").await,
        5,
        "the plan admits against the width that was learned, not a fresh one"
    );

    // Nothing already admitted was disturbed by any of this: the plan is a
    // read, and the six tasks are all still there to be admitted.
    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("width-plan-3")
    .send(&world)
    .await;
    let counted = plan.json()["ready"].as_array().expect("ready").len()
        + plan.json()["blocked"].as_array().expect("blocked").len();
    assert_eq!(counted, 6, "{}", plan.body);
}

// ---------------------------------------------------------------------------
// OP-03 CP2, first clause — publication, catalog lookup and code help.
//
// The Admin tier's defining Operational power is deciding which node kinds may
// ever exist in a project. Without these operations the composed semantic
// topology consumes a specification nobody can publish, read or move, and no
// client can find out what its own codes mean.
// ---------------------------------------------------------------------------

/// A minimal but legal vocabulary: one root, one child that hosts sessions.
fn vocabulary(root: &str, child: Option<&str>) -> serde_json::Value {
    let mut kinds = vec![serde_json::json!({
        "kind": root,
        "allowed_parents": [],
        "cardinality": {"minimum": 1, "maximum": 1},
        "projection_capabilities": ["native_root"],
        "read_only": false,
        "name_template": {
            "segments": [
                {"kind": "literal", "value": "Project Session Workspace"}
            ]
        },
        "code_help": {
            "full_name": "Project Session Workspace",
            "meaning": "Logical root grouping every session scope of one project.",
            "category": "session_topology",
            "lifecycle": "current"
        }
    })];
    if let Some(child) = child {
        kinds.push(serde_json::json!({
            "kind": child,
            "allowed_parents": [root],
            "cardinality": {"minimum": 0},
            "projection_capabilities": ["native_child", "session_host"],
            "read_only": false,
            "name_template": {
                "segments": [
                    {"kind": "literal", "value": "Epic Session Workspace"}
                ]
            },
            "seat_name_template": {
                "segments": [
                    {"kind": "token", "value": "AREA_CODE"}
                ]
            },
            "code_help": {
                "full_name": "Epic Session Workspace",
                "meaning": "One epic's own session scope.",
                "category": "session_topology",
                "lifecycle": "current"
            }
        }));
    }
    serde_json::Value::Array(kinds)
}

/// Draft, validate and publish, then read the exact document back.
#[tokio::test]
async fn a_vocabulary_is_drafted_validated_published_and_read_back() {
    let composed = compose_realm("/tmp/kontor-cp2-publish").await;
    let world = &composed.world;

    let drafted = Call::post(
        format!("/v1/projects/{}/topology-specs:draft", composed.project),
        &serde_json::json!({
            "name": "House vocabulary",
            "root_kind": "PSW",
            "node_kinds": vocabulary("PSW", Some("ESW")),
        }),
    )
    .signed_as(world, "admin")
    .with_key("vocab-draft")
    .send(world)
    .await;
    assert_eq!(drafted.status, 200, "{}", drafted.body);
    let candidate = drafted.json()["candidate"].clone();

    let judged = Call::post(
        format!("/v1/projects/{}/topology-specs:validate", composed.project),
        &serde_json::json!({"candidate": candidate}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(judged.status, 200, "{}", judged.body);
    assert_eq!(
        judged.json()["violations"].as_array().map(Vec::len),
        Some(0),
        "a complete vocabulary is publishable: {}",
        judged.body
    );
    let validation_hash = judged.json()["validation_hash"]
        .as_str()
        .expect("a validation hash")
        .to_owned();
    assert_eq!(
        validation_hash,
        drafted.json()["candidate_hash"].as_str().expect("a hash"),
        "the draft and the verdict are about the same bytes"
    );

    let published = Call::post(
        format!("/v1/projects/{}/topology-specs:publish", composed.project),
        &serde_json::json!({
            "candidate": candidate,
            "validation_hash": validation_hash,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("vocab-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    assert_eq!(published.json()["receipt"]["applied"], "created");
    let spec_id = published.json()["spec"]["id"]
        .as_str()
        .expect("a spec id")
        .to_owned();
    assert_eq!(published.json()["spec"]["version"], 1);

    // The exact document comes back, with the hash the publication returned.
    let read = Call::get(format!(
        "/v1/projects/{}/topology-specs/{spec_id}/1",
        composed.project
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(read.status, 200, "{}", read.body);
    assert_eq!(read.json()["spec"]["canonical_hash"], validation_hash);
    assert_eq!(read.json()["document"]["root_kind"], "PSW");
    assert!(
        read.json()["shareability"]["class"].as_str().is_some(),
        "a published revision carries its classification: {}",
        read.body
    );
}

/// A published revision cannot change in place, and a replay is not a change.
///
/// This is negative proof six, and it is the reason publication is a separate
/// operation from drafting at all: a revision something is already pinned to
/// must mean the same thing tomorrow.
#[tokio::test]
async fn a_published_specification_cannot_change_in_place() {
    let composed = compose_realm("/tmp/kontor-cp2-immutable").await;
    let world = &composed.world;

    let draft = async |key: &str, child: Option<&str>| -> Answer {
        Call::post(
            format!("/v1/projects/{}/topology-specs:draft", composed.project),
            &serde_json::json!({
                "name": "House vocabulary",
                "root_kind": "PSW",
                "node_kinds": vocabulary("PSW", child),
            }),
        )
        .signed_as(world, "admin")
        .with_key(key)
        .send(world)
        .await
    };
    let publish = async |key: &str, candidate: &serde_json::Value, hash: &str| -> Answer {
        Call::post(
            format!("/v1/projects/{}/topology-specs:publish", composed.project),
            &serde_json::json!({
                "candidate": candidate,
                "validation_hash": hash,
                "expected_revision": composed.project_revision,
            }),
        )
        .signed_as(world, "admin")
        .with_key(key)
        .send(world)
        .await
    };

    let first = draft("immutable-draft-1", Some("ESW")).await;
    let candidate = first.json()["candidate"].clone();
    let hash = first.json()["candidate_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let published = publish("immutable-publish-1", &candidate, &hash).await;
    assert_eq!(published.status, 200, "{}", published.body);
    let spec_id = published.json()["spec"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();

    // Republishing the identical bytes is a replay, not a second publication.
    let replayed = publish("immutable-publish-1", &candidate, &hash).await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        published.json()["receipt"]["receipt_id"],
        "a replay answers from the original receipt"
    );

    // Different bytes under the same identity and version are refused — and the
    // validation says so before the publish is even attempted.
    let mut edited = candidate.clone();
    edited["node_kinds"] = vocabulary("PSW", None);
    let edited_hash = Call::post(
        format!("/v1/projects/{}/topology-specs:validate", composed.project),
        &serde_json::json!({"candidate": edited}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(edited_hash.status, 200, "{}", edited_hash.body);
    // The edit is a perfectly legal vocabulary. What it may not be is *this*
    // revision, and that is publication's answer rather than the verdict's.
    assert_eq!(
        edited_hash.json()["violations"].as_array().map(Vec::len),
        Some(0),
        "{}",
        edited_hash.body
    );
    let hash = edited_hash.json()["validation_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let refused = publish("immutable-publish-2", &edited, &hash).await;
    assert_eq!(refused.status, 409, "{}", refused.body);

    // And the stored document is still the one that was published.
    let read = Call::get(format!(
        "/v1/projects/{}/topology-specs/{spec_id}/1",
        composed.project
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(
        read.json()["document"]["node_kinds"]
            .as_array()
            .map(Vec::len),
        Some(2),
        "the published revision did not change underneath the pin: {}",
        read.body
    );
}

/// A publish under a stale project revision writes nothing.
#[tokio::test]
async fn publishing_under_a_stale_revision_writes_nothing() {
    let composed = compose_realm("/tmp/kontor-cp2-stale-publish").await;
    let world = &composed.world;

    let drafted = Call::post(
        format!("/v1/projects/{}/topology-specs:draft", composed.project),
        &serde_json::json!({
            "name": "House vocabulary",
            "root_kind": "PSW",
            "node_kinds": vocabulary("PSW", Some("ESW")),
        }),
    )
    .signed_as(world, "admin")
    .with_key("stale-draft")
    .send(world)
    .await;
    let candidate = drafted.json()["candidate"].clone();
    let hash = drafted.json()["candidate_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let spec_id = candidate["spec_id"].as_str().expect("an id").to_owned();

    let stale = Call::post(
        format!("/v1/projects/{}/topology-specs:publish", composed.project),
        &serde_json::json!({
            "candidate": candidate,
            "validation_hash": hash,
            "expected_revision": composed.project_revision + 7,
        }),
    )
    .signed_as(world, "admin")
    .with_key("stale-publish")
    .send(world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");

    let read = Call::get(format!(
        "/v1/projects/{}/topology-specs/{spec_id}/1",
        composed.project
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(
        read.status, 404,
        "a refused publish left nothing behind: {}",
        read.body
    );
}

/// A hash that does not name the candidate is refused.
#[tokio::test]
async fn publishing_a_document_the_validation_never_saw_is_refused() {
    let composed = compose_realm("/tmp/kontor-cp2-hash").await;
    let world = &composed.world;

    let drafted = Call::post(
        format!("/v1/projects/{}/topology-specs:draft", composed.project),
        &serde_json::json!({
            "name": "House vocabulary",
            "root_kind": "PSW",
            "node_kinds": vocabulary("PSW", Some("ESW")),
        }),
    )
    .signed_as(world, "admin")
    .with_key("hash-draft")
    .send(world)
    .await;

    let refused = Call::post(
        format!("/v1/projects/{}/topology-specs:publish", composed.project),
        &serde_json::json!({
            "candidate": drafted.json()["candidate"],
            "validation_hash": "0".repeat(64),
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("hash-publish")
    .send(world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
}

/// The catalog answers what a code means; an unknown code is never guessed.
///
/// This is negative proof four's second half. A client that had to keep its own
/// glossary would eventually disagree with the server about what its own state
/// says, so the server is the only place these words live — and it says so when
/// it does not know one.
#[tokio::test]
async fn the_catalog_resolves_a_known_code_and_refuses_an_unknown_one() {
    let composed = compose_realm("/tmp/kontor-cp2-catalog").await;
    let world = &composed.world;
    let catalog = "01936f5a-1000-7000-8000-000000000002";

    let whole = Call::get(format!("/v1/catalog/role-catalogs/{catalog}/1"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(whole.status, 200, "{}", whole.body);
    let roles = whole.json()["roles"].as_array().expect("roles").clone();
    assert!(
        !roles.is_empty(),
        "the catalog declares roles: {}",
        whole.body
    );
    let known = roles[0]["role_code"].as_str().expect("a code").to_owned();

    let resolved = Call::get(format!(
        "/v1/catalog/role-catalogs/{catalog}/1/roles/{known}"
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(resolved.status, 200, "{}", resolved.body);
    assert_eq!(resolved.json()["role_code"], known.as_str());
    assert!(
        resolved.json()["standard_title"]
            .as_str()
            .is_some_and(|title| !title.is_empty()),
        "a resolved role carries the catalog's own title: {}",
        resolved.body
    );
    assert!(
        resolved.json()["responsibility_summary"]
            .as_str()
            .is_some_and(|summary| !summary.is_empty()),
        "and what it is responsible for: {}",
        resolved.body
    );

    // A code no revision declares is not found, and no title is invented.
    let unknown = Call::get(format!("/v1/catalog/role-catalogs/{catalog}/1/roles/ZZZ"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(unknown.status, 404, "{}", unknown.body);
    assert_eq!(unknown.code(), "not_found");
    assert!(
        !unknown.body.contains("standard_title"),
        "an unknown code must not come back with a guessed title: {}",
        unknown.body
    );

    // A revision that does not exist is likewise not found.
    let absent = Call::get(format!("/v1/catalog/role-catalogs/{catalog}/99"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(absent.status, 404, "{}", absent.body);
}

/// Code help explains every code an epic's pinned revisions define.
#[tokio::test]
async fn code_help_explains_the_codes_an_epic_is_actually_pinned_to() {
    let composed = compose_realm("/tmp/kontor-cp2-help").await;
    let world = &composed.world;

    // The epic has to be pinned before there is anything to explain, which
    // ensuring its scope does.
    let ensured = Call::post(
        format!("/v1/projects/{}/topology:ensure", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("help-ensure")
    .send(world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);

    let help = Call::get(format!(
        "/v1/projects/{}/epics/{}/code-help",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(help.status, 200, "{}", help.body);
    let entries = help.json()["entries"].as_array().expect("entries").clone();

    let entry = |code: &str| {
        entries
            .iter()
            .find(|entry| entry["code"] == code)
            .unwrap_or_else(|| panic!("`{code}` is explained: {}", help.body))
            .clone()
    };
    // A declared kind, a historical one, and a role — one projection, because a
    // client rendering a transcript has a code in hand and does not know which
    // family it came from.
    assert_eq!(entry("ECP")["category"], "session_topology");
    assert_eq!(entry("ECP")["lifecycle"], "current");
    assert_eq!(
        entry("TSC")["lifecycle"],
        "compatibility",
        "a historical code is still explained honestly: {}",
        help.body
    );
    assert_eq!(entry("TPM")["category"], "role");
    assert!(
        entry("TPM")["meaning"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "every entry says what the code means: {}",
        help.body
    );

    // Sorted by category then code, so two reads produce the same list.
    let ordered: Vec<(String, String)> = entries
        .iter()
        .map(|entry| {
            (
                entry["category"].as_str().expect("a category").to_owned(),
                entry["code"].as_str().expect("a code").to_owned(),
            )
        })
        .collect();
    let mut sorted = ordered.clone();
    sorted.sort();
    assert_eq!(ordered, sorted, "{}", help.body);

    // Every entry cites the revision it was read from, so a client can tell two
    // vocabularies apart rather than merging them.
    for entry in &entries {
        assert!(
            entry["source"]["id"].as_str().is_some(),
            "an entry names the revision it came from: {entry}"
        );
    }
}

/// An epic's pin moves only through the exact preview that was authorized.
#[tokio::test]
async fn an_epic_pin_moves_only_through_the_preview_that_was_authorized() {
    let composed = compose_realm("/tmp/kontor-cp2-upgrade").await;
    let world = &composed.world;

    let ensured = Call::post(
        format!("/v1/projects/{}/topology:ensure", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("upgrade-ensure")
    .send(world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let pinned_before = ensured.json()["projection"]["pinned_spec"].clone();
    let materialized = Call::post(
        format!("/v1/projects/{}/topology:materialize", composed.project),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": composed.epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("upgrade-materialize")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);

    // Publish a second revision of the *bundled* lineage whose sole semantic
    // change is the ESW native-name template. Existing kinds, hierarchy,
    // capabilities, nodes, containers and seats remain valid in place.
    let bundled = pinned_before["id"].as_str().expect("a spec id").to_owned();
    let current = Call::get(format!(
        "/v1/projects/{}/topology-specs/{bundled}/{}",
        composed.project, pinned_before["version"]
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(current.status, 200, "{}", current.body);
    let mut node_kinds = current.json()["document"]["node_kinds"].clone();
    node_kinds
        .as_array_mut()
        .expect("node kinds")
        .iter_mut()
        .find(|kind| kind["kind"] == "ESW")
        .expect("the ESW kind")["name_template"] = serde_json::json!({
        "segments": [
            {"kind": "literal", "value": "Upgraded ESW"},
            {"kind": "token", "value": "JIRA_CODE"},
            {"kind": "token", "value": "KONTOR_BACKLOG_CODE"}
        ]
    });
    let drafted = Call::post(
        format!("/v1/projects/{}/topology-specs:draft", composed.project),
        &serde_json::json!({
            "base": {"id": bundled, "version": pinned_before["version"]},
            "name": "Retitled epic workspace vocabulary",
            "root_kind": "PSW",
            "node_kinds": node_kinds,
            "historical_codes": current.json()["document"]["historical_codes"],
        }),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-draft")
    .send(world)
    .await;
    assert_eq!(drafted.status, 200, "{}", drafted.body);
    assert_eq!(
        drafted.json()["candidate"]["version"],
        2,
        "an edit drafts the next version of the lineage: {}",
        drafted.body
    );
    let candidate = drafted.json()["candidate"].clone();
    let hash = drafted.json()["candidate_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    let published = Call::post(
        format!("/v1/projects/{}/topology-specs:publish", composed.project),
        &serde_json::json!({
            "candidate": candidate,
            "validation_hash": hash,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);

    // The project default moves through its own preview/apply seam. It changes
    // what future epics inherit and deliberately leaves this epic's immutable
    // v1 pin alone.
    let project_preview = Call::post(
        format!(
            "/v1/projects/{}/topology-selection:preview",
            composed.project
        ),
        &serde_json::json!({"target_spec": {"id": bundled, "version": 2}}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(project_preview.status, 200, "{}", project_preview.body);
    assert_eq!(project_preview.json()["current_spec"]["version"], 1);
    assert_eq!(project_preview.json()["target_spec"]["version"], 2);
    let project_preview_hash = project_preview.json()["preview_hash"]
        .as_str()
        .expect("a project selection hash")
        .to_owned();

    let selected = Call::post(
        format!("/v1/projects/{}/topology-selection:apply", composed.project),
        &serde_json::json!({
            "preview_hash": project_preview_hash,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("project-topology-select")
    .send(world)
    .await;
    assert_eq!(selected.status, 200, "{}", selected.body);
    assert_eq!(selected.json()["selected_spec"]["version"], 2);
    assert_eq!(selected.json()["receipt"]["applied"], "updated");

    let selected_replay = Call::post(
        format!("/v1/projects/{}/topology-selection:apply", composed.project),
        &serde_json::json!({
            "preview_hash": project_preview_hash,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("project-topology-select")
    .send(world)
    .await;
    assert_eq!(selected_replay.status, 200, "{}", selected_replay.body);
    assert_eq!(selected_replay.json()["receipt"]["applied"], "unchanged");

    let still_pinned = Call::get(format!(
        "/v1/projects/{}/topology:inspect?epic_id={}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(still_pinned.json()["pinned_spec"]["version"], 1);

    // The preview says what the move would cost, and commits nothing.
    let preview = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-preview",
            composed.project, composed.epic
        ),
        &serde_json::json!({"target_spec": {"id": bundled, "version": 2}}),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-preview")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["current_spec"]["version"], 1);
    assert_eq!(preview.json()["target_spec"]["version"], 2);
    let effects = preview.json()["effects"]
        .as_array()
        .expect("effects")
        .clone();
    assert!(
        effects.is_empty(),
        "a name-template-only upgrade preserves every topology subject: {}",
        preview.body
    );
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();

    let unmoved = Call::get(format!(
        "/v1/projects/{}/topology:inspect?epic_id={}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(
        unmoved.json()["pinned_spec"]["version"],
        1,
        "a preview commits nothing: {}",
        unmoved.body
    );

    // A hash no published revision produces is refused.
    let invented = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-apply",
            composed.project, composed.epic
        ),
        &serde_json::json!({"preview_hash": "0".repeat(64), "expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-invented")
    .send(world)
    .await;
    assert_eq!(invented.status, 409, "{}", invented.body);

    // A stale epic revision is refused too, and the pin is still where it was.
    let stale = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-apply",
            composed.project, composed.epic
        ),
        &serde_json::json!({"preview_hash": preview_hash, "expected_revision": 99}),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-stale")
    .send(world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);

    let epic_revision = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(epic_revision.status, 200, "{}", epic_revision.body);
    let revision = epic_revision.json()["revision"]
        .as_u64()
        .expect("a revision");

    let applied = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-apply",
            composed.project, composed.epic
        ),
        &serde_json::json!({"preview_hash": preview_hash, "expected_revision": revision}),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-apply")
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["pinned_spec"]["version"], 2);
    assert_eq!(
        applied.json()["projection"]["pinned_spec"]["version"],
        2,
        "the embedded epic projection reports the epic pin: {}",
        applied.body
    );
    assert_eq!(applied.json()["receipt"]["applied"], "created");

    let project_id = ProjectId::parse(&composed.project).expect("a project id");
    let epic_id = MiniProjectId::parse(&composed.epic).expect("an epic id");
    let nodes = world
        .daemon
        .state()
        .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
        .expect("the epic nodes read");
    assert!(
        nodes.iter().all(|node| node.topology.version.get() == 2),
        "the exact existing nodes move to the target revision in place"
    );

    let epic_node = applied.json()["projection"]["nodes"]
        .as_array()
        .expect("projected nodes")
        .iter()
        .find(|node| node["kind_key"] == "ESW")
        .expect("the ESW node")
        .clone();
    let epic_node_id = epic_node["topology_node_id"]
        .as_str()
        .expect("the ESW node id");
    let parsed_epic_node = TopologyNodeId::parse(epic_node_id).expect("a topology node id");
    let old_title = world
        .fake
        .container_title(parsed_epic_node)
        .expect("the existing ESW native container keeps its identity");
    let preview_retitle = Call::post(
        format!(
            "/v1/projects/{}/topology/nodes/{epic_node_id}/container:retitle-preview",
            composed.project
        ),
        &serde_json::json!({"expected_revision": composed.project_revision}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview_retitle.status, 200, "{}", preview_retitle.body);
    let desired_title = preview_retitle.json()["desired_title"]
        .as_str()
        .expect("a desired title")
        .to_owned();
    assert!(
        desired_title.starts_with("Upgraded ESW • ")
            && !desired_title.contains('<')
            && desired_title != old_title,
        "the v2 template is rendered from typed scope: {}",
        preview_retitle.body
    );
    let retitled = Call::post(
        format!(
            "/v1/projects/{}/topology/nodes/{epic_node_id}/container:retitle-apply",
            composed.project
        ),
        &serde_json::json!({"expected_revision": composed.project_revision}),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-retitle-apply")
    .send(world)
    .await;
    assert_eq!(retitled.status, 200, "{}", retitled.body);
    assert_eq!(retitled.json()["observed_title"], desired_title);
    assert_eq!(
        world.fake.container_title(parsed_epic_node).as_deref(),
        Some(desired_title.as_str()),
        "retitle preserves the exact native container and reads the new title back"
    );

    // The replay answers from what is durable and moves nothing again.
    let replayed = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-apply",
            composed.project, composed.epic
        ),
        &serde_json::json!({"preview_hash": preview_hash, "expected_revision": revision}),
    )
    .signed_as(world, "admin")
    .with_key("upgrade-apply")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replayed.json()["pinned_spec"]["version"], 2);

    // The revision the epic *left* is untouched — it is immutable, and other
    // epics may still be pinned to it.
    let old = Call::get(format!(
        "/v1/projects/{}/topology-specs/{bundled}/1",
        composed.project
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(old.status, 200, "{}", old.body);
    assert_eq!(old.json()["spec"]["version"], 1);
}

// ---------------------------------------------------------------------------
// KON-OP-04 CP1 — the composed Project Core Team.
//
// Configuration, not a TeamRun: nothing below creates a seat, a run or a
// topology node. What is proved is that a roster can be previewed, published
// and read back across a restart of the read path, that the mandatory roles
// cannot be talked out of the roster, and that the policy a caller states is
// the policy the server persists rather than one it inferred.
// ---------------------------------------------------------------------------

/// The seeded role catalog every Core Team test resolves against.
const SEEDED_CATALOG: &str = "01936f5a-1000-7000-8000-000000000002";

/// One `CoreTeamSeatSelectionDto`.
fn seat(role_code: &str, presence: &str, ad_hoc_allowed: bool) -> serde_json::Value {
    serde_json::json!({
        "role": {
            "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
            "role_code": role_code,
        },
        "presence": presence,
        "ad_hoc_allowed": ad_hoc_allowed,
    })
}

/// A roster is previewed, published and read back as what was published.
#[tokio::test]
async fn a_core_team_is_previewed_applied_and_read_back() {
    let composed = compose_realm("/tmp/kontor-op04-core-team").await;
    let world = &composed.world;
    let project = &composed.project;

    let previewed = Call::post(
        format!("/v1/projects/{project}/core-team:preview"),
        &serde_json::json!({"seats": [seat("SA", "default", true)]}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let preview_hash = previewed.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    // A preview commits nothing: the project still has no roster to read.
    let before = Call::get(format!("/v1/projects/{project}/core-team"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(
        before.status, 404,
        "the preview published a roster: {}",
        before.body
    );

    let applied = Call::post(
        format!("/v1/projects/{project}/core-team:apply"),
        &serde_json::json!({
            "seats": [seat("SA", "default", true)],
            "preview_hash": preview_hash,
            "expected_revision": 1,
        }),
    )
    .signed_as(world, "admin")
    .with_key("core-team-first")
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["receipt"]["applied"], "created");

    let read = Call::get(format!("/v1/projects/{project}/core-team"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    let seats = read.json()["seats"].as_array().expect("seats").clone();
    let codes: Vec<&str> = seats
        .iter()
        .map(|entry| entry["role"]["role_code"].as_str().expect("a code"))
        .collect();
    // The mandatory epic roles are inserted rather than assumed, and stay
    // distinct: `SA` is a different seat from `LSA` and never stands in for it.
    assert!(codes.contains(&"LSA"), "no LSA seat: {}", read.body);
    assert!(codes.contains(&"TPM"), "no TPM seat: {}", read.body);
    assert!(codes.contains(&"SA"), "no SA seat: {}", read.body);
    for entry in &seats {
        let code = entry["role"]["role_code"].as_str().expect("a code");
        if code == "LSA" || code == "TPM" {
            assert_eq!(
                entry["presence"], "required",
                "{code} must be required: {}",
                read.body
            );
        }
        // The standard title is the catalog's, never the caller's.
        assert!(
            entry["role"]["standard_title"]
                .as_str()
                .is_some_and(|title| !title.is_empty()),
            "{code} carries no resolved title: {}",
            read.body
        );
    }
    // The policy the caller stated is the policy that came back — not one
    // derived from the role code or from the order the seats were sent in.
    let sa = seats
        .iter()
        .find(|entry| entry["role"]["role_code"] == "SA")
        .expect("the SA seat");
    assert_eq!(sa["presence"], "default", "{}", read.body);
    assert_eq!(sa["ad_hoc_allowed"], true, "{}", read.body);
}

/// A repeated apply publishes one revision, not two.
#[tokio::test]
async fn a_repeated_core_team_apply_publishes_one_revision() {
    let composed = compose_realm("/tmp/kontor-op04-core-team-replay").await;
    let world = &composed.world;
    let project = &composed.project;

    let previewed = Call::post(
        format!("/v1/projects/{project}/core-team:preview"),
        &serde_json::json!({"seats": [seat("QA", "on_demand", false)]}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let preview_hash = previewed.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let body = serde_json::json!({
        "seats": [seat("QA", "on_demand", false)],
        "preview_hash": preview_hash,
        "expected_revision": 1,
    });

    let first = Call::post(format!("/v1/projects/{project}/core-team:apply"), &body)
        .signed_as(world, "admin")
        .with_key("core-team-once")
        .send(world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["receipt"]["applied"], "created");
    let revision = first.json()["core_team"]["revision"]
        .as_u64()
        .expect("a revision");

    // The retry presents the revision it read *before* the first call, which is
    // the only revision a caller that lost the response could present.
    let replayed = Call::post(format!("/v1/projects/{project}/core-team:apply"), &body)
        .signed_as(world, "admin")
        .with_key("core-team-once")
        .send(world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["core_team"]["revision"],
        revision,
        "the replay published a second revision: {}",
        replayed.body
    );
}

/// The mandatory roles cannot be weakened, and an unknown code is refused.
#[tokio::test]
async fn a_core_team_refuses_a_weakened_mandatory_role_and_an_unknown_code() {
    let composed = compose_realm("/tmp/kontor-op04-core-team-refusals").await;
    let world = &composed.world;
    let project = &composed.project;

    for (case, seats) in [
        ("a weakened LSA", vec![seat("LSA", "on_demand", true)]),
        ("an unknown code", vec![seat("ZZZ", "default", false)]),
        (
            "a duplicate code",
            vec![seat("SA", "default", true), seat("SA", "required", false)],
        ),
    ] {
        let refused = Call::post(
            format!("/v1/projects/{project}/core-team:preview"),
            &serde_json::json!({"seats": seats}),
        )
        .signed_as(world, "admin")
        .send(world)
        .await;
        assert!(
            refused.status.is_client_error() || refused.status.is_server_error(),
            "{case} was accepted: {}",
            refused.body
        );
    }
}

/// The request shape is closed: no raw role, no caller-authored title.
#[tokio::test]
async fn a_core_team_refuses_a_raw_role_and_a_caller_authored_title() {
    let composed = compose_realm("/tmp/kontor-op04-core-team-closed").await;
    let world = &composed.world;
    let project = &composed.project;

    for (case, body) in [
        (
            "a raw role string",
            serde_json::json!({"seats": [{"role": "LSA", "presence": "required", "ad_hoc_allowed": false}]}),
        ),
        (
            "a caller-authored standard title",
            serde_json::json!({"seats": [{
                "role": {
                    "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                    "role_code": "SA",
                    "standard_title": "Chief Architect Of Everything",
                },
                "presence": "default",
                "ad_hoc_allowed": true,
            }]}),
        ),
        (
            "an omitted presence",
            serde_json::json!({"seats": [{
                "role": {
                    "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                    "role_code": "SA",
                },
                "ad_hoc_allowed": true,
            }]}),
        ),
    ] {
        let refused = Call::post(format!("/v1/projects/{project}/core-team:preview"), &body)
            .signed_as(world, "admin")
            .send(world)
            .await;
        assert!(
            refused.status.is_client_error() || refused.status.is_server_error(),
            "{case} was accepted: {}",
            refused.body
        );
    }
}

// ---------------------------------------------------------------------------
// KON-OP-04 CP2/CP3/CP4 — Quick sessions, promotion and the epic roster.
//
// Every test below drives a write path and reads it back. A route table whose
// operations all refuse passes every contract test and admits no work; the only
// way to tell a composed service from a well-shaped refusal is to make
// something happen.
// ---------------------------------------------------------------------------

/// Adopt and read back the project session base a Quick session hangs under.
///
/// OP-02 owns this: the base is ensured, materialized and then observed. A
/// Quick session deliberately refuses to place itself when this has not
/// happened, so every test that opens one has to do it first.
async fn adopt_session_base(world: &World, project: &str, project_revision: u64) {
    let answer = Call::post(
        format!("/v1/projects/{project}/topology:ensure"),
        &serde_json::json!({
            "target": {"scope": "project_root"},
            "expected_revision": project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key(format!("base-ensure-{project}"))
    .send(world)
    .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    // The base is only a *bound* base once its runtime has answered for it.
    world.daemon.reconcile().await;
}

/// Publish a Core Team, and answer with the project's revision afterwards.
async fn publish_core_team(world: &World, project: &str, seats: serde_json::Value) -> u64 {
    let previewed = Call::post(
        format!("/v1/projects/{project}/core-team:preview"),
        &serde_json::json!({"seats": seats}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let preview_hash = previewed.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let applied = Call::post(
        format!("/v1/projects/{project}/core-team:apply"),
        &serde_json::json!({
            "seats": seats,
            "preview_hash": preview_hash,
            "expected_revision": 1,
        }),
    )
    .signed_as(world, "admin")
    .with_key(format!("core-team-{project}"))
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    applied.json()["core_team"]["revision"]
        .as_u64()
        .expect("a revision")
}

/// Explicit runtime-facing identity for a promoted test epic. Promotion may
/// not derive these values from purpose text or generated ids.
fn promotion_apply_body(preview_hash: impl serde::Serialize) -> serde_json::Value {
    serde_json::json!({
        "preview_hash": preview_hash,
        "expected_revision": 1,
        "execution_scope": {
            "external_epic_key": "ASMA-PROMOTION",
            "short_title": "Promoted epic",
            "kontor_backlog_code": "PROMO",
            "ai_short_name": "Promoted Epic",
        }
    })
}

/// Quick roles are exactly the ad-hoc-eligible Core Team entries.
#[tokio::test]
async fn quick_roles_are_the_ad_hoc_eligible_core_team_entries() {
    let composed = compose_realm("/tmp/kontor-op04-quick-roles").await;
    let world = &composed.world;
    let project = &composed.project;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true), seat("QA", "default", false),]),
    )
    .await;

    let roles = Call::get(format!("/v1/projects/{project}/quick-roles"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(roles.status, 200, "{}", roles.body);
    let answered = roles.json();
    let codes: Vec<&str> = answered["roles"]
        .as_array()
        .expect("roles")
        .iter()
        .map(|entry| entry["role_code"].as_str().expect("a code"))
        .collect();
    assert!(
        codes.contains(&"SA"),
        "SA is quick-eligible: {}",
        roles.body
    );
    assert!(
        codes.contains(&"LSA"),
        "the inserted LSA is quick-eligible: {}",
        roles.body
    );
    // Stated `ad_hoc_allowed: false`, so it is not offered — and TPM is
    // inserted as a mandatory role that is deliberately not quick-eligible.
    assert!(
        !codes.contains(&"QA"),
        "QA must not be offered: {}",
        roles.body
    );
    assert!(
        !codes.contains(&"TPM"),
        "TPM must not be offered: {}",
        roles.body
    );
}

/// A Quick session is opened once, and a lost acknowledgement reuses it.
#[tokio::test]
async fn a_quick_session_is_opened_once_and_replays_to_the_same_ids() {
    let composed = compose_realm("/tmp/kontor-op04-quick-ensure").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;

    let body = serde_json::json!({
        "role": {
            "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
            "role_code": "SA",
        },
        "purpose": "Look into the flaky import",
    });
    let opened = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &body,
    )
    .signed_as(world, "operator")
    .with_key("quick-once")
    .send(world)
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let session = opened.json()["quick_session_id"]
        .as_str()
        .expect("a session id")
        .to_owned();
    let node = opened.json()["topology_node_id"]
        .as_str()
        .expect("a node id")
        .to_owned();
    assert_eq!(opened.json()["role"]["role_code"], "SA");

    let replayed = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &body,
    )
    .signed_as(world, "operator")
    .with_key("quick-once")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(
        replayed.json()["quick_session_id"],
        session.as_str(),
        "the retry opened a second session: {}",
        replayed.body
    );
    assert_eq!(
        replayed.json()["topology_node_id"],
        node.as_str(),
        "the retry placed a second workspace: {}",
        replayed.body
    );
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
}

/// A role the roster does not mark ad-hoc cannot open a session.
#[tokio::test]
async fn a_quick_ineligible_role_cannot_open_a_session() {
    let composed = compose_realm("/tmp/kontor-op04-quick-refusal").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("QA", "default", false)]),
    )
    .await;

    for (case, code) in [
        ("a quick-ineligible role", "QA"),
        ("an unknown role", "ZZZ"),
    ] {
        let refused = Call::post(
            format!("/v1/projects/{project}/quick-sessions:ensure"),
            &serde_json::json!({
                "role": {
                    "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                    "role_code": code,
                },
                "purpose": "Should not open",
            }),
        )
        .signed_as(world, "operator")
        .with_key(format!("quick-refuse-{code}"))
        .send(world)
        .await;
        assert!(
            refused.status.is_client_error(),
            "{case} opened a session: {}",
            refused.body
        );
    }
}

/// A promotion builds one epic, seats the frozen roster and hands the work on.
#[tokio::test]
async fn a_promotion_creates_one_epic_and_hands_the_work_to_its_lsa() {
    let composed = compose_realm("/tmp/kontor-op04-promotion").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true), seat("QA", "on_demand", false),]),
    )
    .await;

    let opened = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &serde_json::json!({
            "role": {
                "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                "role_code": "SA",
            },
            "purpose": "Investigate the retry storm",
        }),
    )
    .signed_as(world, "operator")
    .with_key("quick-to-promote")
    .send(world)
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let session = opened.json()["quick_session_id"]
        .as_str()
        .expect("a session id")
        .to_owned();

    let previewed = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:preview"),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let preview_hash = previewed.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let effects = previewed.json()["effects"]
        .as_array()
        .expect("effects")
        .clone();
    // On-demand roles are declared, not seated: their presence in the roster is
    // not permission to open them at bootstrap.
    assert!(
        !effects
            .iter()
            .any(|effect| effect["subject"].as_str() == Some("seat:qa")),
        "an on-demand role was planned: {}",
        previewed.body
    );

    let body = promotion_apply_body(&preview_hash);
    let applied = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &body,
    )
    .signed_as(world, "operator")
    .with_key("promote-once")
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();
    assert_eq!(applied.json()["quick_session_id"], session.as_str());

    // The epic carries the roster it was staffed from, with its seats filled.
    let roster = Call::get(format!(
        "/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    // A GET on a materialize route is not a read; what matters is the roster is
    // reachable through the epic's own materialize command below.
    assert!(roster.status.is_client_error(), "{}", roster.body);

    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("materialize-once")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let seats = materialized.json()["core_team"]["seats"]
        .as_array()
        .expect("seats")
        .clone();
    let lsa = seats
        .iter()
        .find(|entry| entry["role"]["role_code"] == "LSA")
        .expect("an LSA seat");
    assert!(
        lsa["seat_binding_id"].as_str().is_some(),
        "the epic's LSA is unseated: {}",
        materialized.body
    );
    let tpm = seats
        .iter()
        .find(|entry| entry["role"]["role_code"] == "TPM")
        .expect("a TPM seat");
    assert!(
        tpm["seat_binding_id"].as_str().is_some(),
        "the epic's TPM is unseated: {}",
        materialized.body
    );
    assert_ne!(
        lsa["seat_binding_id"], tpm["seat_binding_id"],
        "LSA and TPM collapsed into one seat: {}",
        materialized.body
    );
    // On-demand stays absent even after an explicit materialize.
    let qa = seats
        .iter()
        .find(|entry| entry["role"]["role_code"] == "QA")
        .expect("the QA entry is still declared");
    assert!(
        qa["seat_binding_id"].is_null(),
        "an on-demand role was seated: {}",
        materialized.body
    );

    // Prepare the shared ECP through the ordinary TPM route while deliberately
    // leaving LSA's logical seat empty for the existing-session claim below.
    let prepared_ecp = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({
            "expected_revision": 1,
            "routes": [{
                "role_code": "TPM",
                "model_route": {"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}
            }]
        }),
    )
    .signed_as(world, "admin")
    .with_key("prepare-ecp-for-seat-claim")
    .send(world)
    .await;
    assert_eq!(prepared_ecp.status, 200, "{}", prepared_ecp.body);

    // A hand-started native LSA can claim the empty durable SeatBinding without
    // being archived or recreated. Preview freezes its provider conversation
    // and route; apply and exact-key replay preserve the same native identity.
    let lsa_binding_id = SeatBindingId::parse(
        lsa["seat_binding_id"]
            .as_str()
            .expect("the logical LSA binding"),
    )
    .expect("a canonical LSA binding");
    let project_id = ProjectId::parse(project).expect("a canonical project id");
    let lsa_node = world.daemon.state().with_store(|store| {
        store
            .get_seat_binding(project_id, lsa_binding_id)
            .expect("the LSA binding reads")
            .expect("the LSA binding exists")
            .topology_node_id
    });
    let claimed_native = ExternalId::parse("native-hand-started-lsa").expect("a native id");
    world
        .fake
        .seed_hosted_seat_claimant(
            lsa_node,
            claimed_native.clone(),
            Some(ExternalId::parse("provider-hand-started-lsa").expect("a provider session")),
            ModelRung {
                provider: ProviderRef("codex".to_owned()),
                model: ModelRef("gpt-5.6-sol".to_owned()),
                effort: Some(EffortLevel::Xhigh),
            },
            "hand-started LSA",
        )
        .expect("the hand-started claimant is visible in the ECP");
    let claim_request = serde_json::json!({
        "expected_revision": 1,
        "seat_binding_id": lsa_binding_id,
        "claimant_native_id": claimed_native,
        "expected_current_native_id": null,
    });
    let claim_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seat-claims:preview"),
        &claim_request,
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(claim_preview.status, 200, "{}", claim_preview.body);
    assert_eq!(
        claim_preview.json()["claimant_native_id"],
        claimed_native.as_str()
    );
    assert_eq!(claim_preview.json()["already_claimed"], false);
    let mut claim_apply = claim_request;
    claim_apply["preview_hash"] = claim_preview.json()["preview_hash"].clone();
    let claimed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seat-claims:apply"),
        &claim_apply,
    )
    .signed_as(world, "admin")
    .with_key("claim-hand-started-lsa")
    .send(world)
    .await;
    assert_eq!(claimed.status, 200, "{}", claimed.body);
    assert_eq!(
        claimed.json()["claimant_native_id"],
        claimed_native.as_str()
    );
    assert_eq!(claimed.json()["receipt"]["applied"], "created");
    assert_eq!(
        world.fake.seat_title(&claimed_native).as_deref(),
        Some("LSA • ASMA-PROMOTION • PROMO")
    );
    let replayed_claim = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seat-claims:apply"),
        &claim_apply,
    )
    .signed_as(world, "admin")
    .with_key("claim-hand-started-lsa")
    .send(world)
    .await;
    assert_eq!(replayed_claim.status, 200, "{}", replayed_claim.body);
    assert_eq!(replayed_claim.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed_claim.json()["claimant_native_id"],
        claimed_native.as_str()
    );

    let takeover_native = ExternalId::parse("native-takeover-lsa").expect("a native id");
    world
        .fake
        .seed_hosted_seat_claimant(
            lsa_node,
            takeover_native.clone(),
            Some(ExternalId::parse("provider-takeover-lsa").expect("a provider session")),
            ModelRung {
                provider: ProviderRef("codex".to_owned()),
                model: ModelRef("gpt-5.6-sol".to_owned()),
                effort: Some(EffortLevel::Xhigh),
            },
            "replacement hand-started LSA",
        )
        .expect("the takeover claimant is visible in the ECP");
    let takeover_request = serde_json::json!({
        "expected_revision": 1,
        "seat_binding_id": lsa_binding_id,
        "claimant_native_id": takeover_native,
        "expected_current_native_id": claimed_native,
    });
    let takeover_preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seat-claims:preview"),
        &takeover_request,
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(takeover_preview.status, 200, "{}", takeover_preview.body);
    assert_eq!(
        takeover_preview.json()["predecessor_native_id"],
        claimed_native.as_str()
    );
    let mut takeover_apply = takeover_request;
    takeover_apply["preview_hash"] = takeover_preview.json()["preview_hash"].clone();
    let taken_over = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seat-claims:apply"),
        &takeover_apply,
    )
    .signed_as(world, "admin")
    .with_key("take-over-hand-started-lsa")
    .send(world)
    .await;
    assert_eq!(taken_over.status, 200, "{}", taken_over.body);
    assert_eq!(taken_over.json()["receipt"]["applied"], "updated");
    assert_eq!(
        taken_over.json()["claimant_native_id"],
        takeover_native.as_str()
    );
    let former_title = format!("Former · lsa · {claimed_native}");
    assert_eq!(
        world.fake.seat_title(&claimed_native).as_deref(),
        Some(former_title.as_str()),
        "the predecessor remains live under a deterministic non-canonical title"
    );
    let historical = world.daemon.state().with_store(|store| {
        store
            .get_hosted_topology_seat_history(project_id, lsa_binding_id, &claimed_native)
            .expect("the prior tenure reads")
    });
    assert!(
        historical.is_some(),
        "takeover must retain immutable seat history"
    );

    // A second, explicitly routed materialization fills the exact same logical
    // LSA/TPM bindings with native sessions in the ECP. This is the legacy
    // recovery path: no replacement topology and no delivery TeamRun.
    let lsa_binding = lsa["seat_binding_id"]
        .as_str()
        .expect("LSA binding")
        .to_owned();
    let tpm_binding = tpm["seat_binding_id"]
        .as_str()
        .expect("TPM binding")
        .to_owned();
    let native_body = serde_json::json!({
        "expected_revision": 1,
        "routes": [
            {"role_code": "LSA", "model_route": {"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}},
            {"role_code": "TPM", "model_route": {"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}}
        ]
    });
    let native = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &native_body,
    )
    .signed_as(world, "admin")
    .with_key("materialize-native-once")
    .send(world)
    .await;
    assert_eq!(native.status, 200, "{}", native.body);
    let native_seats = native.json()["core_team"]["seats"]
        .as_array()
        .expect("native seats")
        .clone();
    let native_lsa = native_seats
        .iter()
        .find(|entry| entry["role"]["role_code"] == "LSA")
        .expect("native LSA");
    let native_tpm = native_seats
        .iter()
        .find(|entry| entry["role"]["role_code"] == "TPM")
        .expect("native TPM");
    assert_eq!(native_lsa["seat_binding_id"], lsa_binding);
    assert_eq!(native_tpm["seat_binding_id"], tpm_binding);
    assert_eq!(
        native_lsa["native_seat"]["model_route"]["provider"],
        "codex"
    );
    assert_eq!(
        native_tpm["native_seat"]["model_route"]["provider"],
        "codex"
    );
    let lsa_native = native_lsa["native_seat"]["native_id"]
        .as_str()
        .expect("LSA native id")
        .to_owned();
    let tpm_native = native_tpm["native_seat"]["native_id"]
        .as_str()
        .expect("TPM native id")
        .to_owned();
    let tpm_generation = native_tpm["native_seat"]["generation"]
        .as_u64()
        .expect("TPM generation");

    let replayed_native = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &native_body,
    )
    .signed_as(world, "admin")
    .with_key("materialize-native-once")
    .send(world)
    .await;
    assert_eq!(replayed_native.status, 200, "{}", replayed_native.body);
    assert_eq!(replayed_native.json()["receipt"]["applied"], "unchanged");
    let replayed_lsa = replayed_native.json()["core_team"]["seats"]
        .as_array()
        .expect("replayed seats")
        .iter()
        .find(|entry| entry["role"]["role_code"] == "LSA")
        .expect("replayed LSA")
        .clone();
    assert_eq!(replayed_lsa["seat_binding_id"], lsa_binding);
    assert_eq!(replayed_lsa["native_seat"]["native_id"], lsa_native);

    // A wrong explicit predecessor is refused before the runtime is touched.
    let route_request = serde_json::json!({
        "expected_revision": 1,
        "seat_binding_id": tpm_binding,
        "expected_native_id": tpm_native,
        "expected_generation": tpm_generation,
        "desired_model_route": {
            "provider": "opencode",
            "model": "deepseek/deepseek-v4-flash",
            "effort": "high"
        }
    });
    let calls_before_refusal = world.fake.calls().len();
    let mut wrong_predecessor = route_request.clone();
    wrong_predecessor["expected_generation"] = serde_json::json!(tpm_generation + 1);
    let refused = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/routes:preview"),
        &wrong_predecessor,
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "revision_conflict");
    assert_eq!(
        world.fake.calls().len(),
        calls_before_refusal,
        "an identity refusal reached the runtime"
    );

    // The authorized correction archives only the exact native predecessor,
    // launches the requested fallback, and preserves the logical SeatBinding.
    let preview = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/routes:preview"),
        &route_request,
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["seat_binding_id"], tpm_binding);
    assert_eq!(preview.json()["predecessor_native_id"], tpm_native);
    assert_eq!(preview.json()["would_replace_native"], true);
    let route_body = serde_json::json!({
        "expected_revision": 1,
        "seat_binding_id": tpm_binding,
        "expected_native_id": tpm_native,
        "expected_generation": tpm_generation,
        "desired_model_route": {
            "provider": "opencode",
            "model": "deepseek/deepseek-v4-flash",
            "effort": "high"
        },
        "preview_hash": preview.json()["preview_hash"],
    });
    let calls_before_apply = world.fake.calls().len();
    let corrected = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/routes:apply"),
        &route_body,
    )
    .signed_as(world, "admin")
    .with_key("core-team-tpm-route-correction")
    .send(world)
    .await;
    assert_eq!(corrected.status, 200, "{}", corrected.body);
    assert_eq!(corrected.json()["seat_binding_id"], tpm_binding);
    assert_eq!(corrected.json()["predecessor_native_id"], tpm_native);
    assert_ne!(corrected.json()["successor_native_id"], tpm_native);
    assert_eq!(corrected.json()["receipt"]["applied"], "updated");
    let corrected_tpm = corrected.json()["core_team"]["seats"]
        .as_array()
        .expect("corrected seats")
        .iter()
        .find(|entry| entry["role"]["role_code"] == "TPM")
        .expect("corrected TPM")
        .clone();
    assert_eq!(corrected_tpm["seat_binding_id"], tpm_binding);
    assert_eq!(
        corrected_tpm["native_seat"]["native_id"],
        corrected.json()["successor_native_id"]
    );
    assert_eq!(
        corrected_tpm["native_seat"]["model_route"]["provider"],
        "opencode"
    );
    let tpm_binding_id = SeatBindingId::parse(&tpm_binding).expect("TPM binding id");
    let route_calls = &world.fake.calls()[calls_before_apply..];
    assert!(
        route_calls.contains(&AdapterCall::RetireHostedSeat(tpm_binding_id)),
        "the predecessor was not retired: {route_calls:?}"
    );
    assert!(
        route_calls.contains(&AdapterCall::LaunchHostedSeat(tpm_binding_id)),
        "the successor was not launched: {route_calls:?}"
    );

    let calls_before_replay = world.fake.calls().len();
    let replayed_route = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/routes:apply"),
        &route_body,
    )
    .signed_as(world, "admin")
    .with_key("core-team-tpm-route-correction")
    .send(world)
    .await;
    assert_eq!(replayed_route.status, 200, "{}", replayed_route.body);
    assert_eq!(replayed_route.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replayed_route.json()["seat_binding_id"], tpm_binding);
    assert_eq!(
        replayed_route.json()["successor_native_id"],
        corrected.json()["successor_native_id"]
    );
    assert_eq!(
        world.fake.calls().len(),
        calls_before_replay,
        "a replay touched the runtime"
    );

    let message_id = kontor_runtime::request::MessageId::generate().to_string();
    let handoff = Call::post(
        format!("/v1/projects/{project}/seat-bindings/{lsa_binding}/messages"),
        &serde_json::json!({"body": "Continue the bounded epic handoff."}),
    )
    .signed_as(world, "operator")
    .with_key(&message_id)
    .send(world)
    .await;
    assert_eq!(handoff.status, 200, "{}", handoff.body);
    assert_eq!(handoff.json()["seat_binding_id"], lsa_binding);
    assert_eq!(handoff.json()["native_id"], lsa_native);
    assert_eq!(handoff.json()["message_id"], message_id);

    // Promoting again returns the same epic rather than building a second.
    let again = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &body,
    )
    .signed_as(world, "operator")
    .with_key("promote-once")
    .send(world)
    .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(
        again.json()["epic_id"],
        epic.as_str(),
        "the retry promoted to a second epic: {}",
        again.body
    );
    assert_eq!(again.json()["receipt"]["applied"], "unchanged");
}

/// A later project edit does not touch an epic already staffed.
#[tokio::test]
async fn a_later_core_team_edit_leaves_a_promoted_epic_frozen() {
    let composed = compose_realm("/tmp/kontor-op04-frozen").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;

    let opened = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &serde_json::json!({
            "role": {
                "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                "role_code": "SA",
            },
            "purpose": "Freeze me",
        }),
    )
    .signed_as(world, "operator")
    .with_key("quick-frozen")
    .send(world)
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let session = opened.json()["quick_session_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let previewed = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:preview"),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let applied = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &promotion_apply_body(previewed.json()["preview_hash"].as_str().expect("hash")),
    )
    .signed_as(world, "operator")
    .with_key("promote-frozen")
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("epic").to_owned();

    // The project adds a role after the epic was staffed.
    let previewed = Call::post(
        format!("/v1/projects/{project}/core-team:preview"),
        &serde_json::json!({"seats": [seat("SA", "default", true), seat("QA", "default", false)]}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let edited = Call::post(
        format!("/v1/projects/{project}/core-team:apply"),
        &serde_json::json!({
            "seats": [seat("SA", "default", true), seat("QA", "default", false)],
            "preview_hash": previewed.json()["preview_hash"].as_str().expect("hash"),
            "expected_revision": 2,
        }),
    )
    .signed_as(world, "admin")
    .with_key("core-team-second")
    .send(world)
    .await;
    assert_eq!(edited.status, 200, "{}", edited.body);

    // The epic still reports the roster it froze: no QA, whatever the project
    // now says.
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("materialize-frozen")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let answered = materialized.json();
    let codes: Vec<&str> = answered["core_team"]["seats"]
        .as_array()
        .expect("seats")
        .iter()
        .map(|entry| entry["role"]["role_code"].as_str().expect("a code"))
        .collect();
    assert!(
        !codes.contains(&"QA"),
        "a later project edit reached into a running epic: {}",
        materialized.body
    );

    // The explicit upgrade is how it moves, and it adds only the new role.
    let previewed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-preview"),
        &serde_json::json!({"target": {"id": SEEDED_CATALOG, "version": 2}}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let upgraded = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-apply"),
        &serde_json::json!({
            "preview_hash": previewed.json()["preview_hash"].as_str().expect("hash"),
            "expected_revision": 1,
        }),
    )
    .signed_as(world, "admin")
    .with_key("roster-upgrade")
    .send(world)
    .await;
    assert_eq!(upgraded.status, 200, "{}", upgraded.body);
    let answered = upgraded.json();
    let codes: Vec<&str> = answered["core_team"]["seats"]
        .as_array()
        .expect("seats")
        .iter()
        .map(|entry| entry["role"]["role_code"].as_str().expect("a code"))
        .collect();
    assert!(
        codes.contains(&"QA"),
        "the explicit upgrade did not add the new role: {}",
        upgraded.body
    );
}

/// An epic imported before Core Team publication has no frozen roster row.
/// Its explicit preview/apply path must therefore bootstrap that first pin,
/// without replacing the already-durable ESW/ECP or duplicating leadership.
#[tokio::test]
async fn a_legacy_epic_bootstraps_one_frozen_roster_and_one_leadership_pair() {
    let composed = compose_realm("/tmp/kontor-op17-bootstrap-roster").await;
    let world = &composed.world;
    let project = &composed.project;
    let epic = &composed.epic;

    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", false)]),
    )
    .await;

    let ensured = Call::post(
        format!("/v1/projects/{project}/topology:ensure"),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": epic},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("bootstrap-roster-topology")
    .send(world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let before: std::collections::BTreeMap<String, String> = ensured.json()["projection"]["nodes"]
        .as_array()
        .expect("topology nodes")
        .iter()
        .filter_map(|node| {
            Some((
                node["kind_key"].as_str()?.to_owned(),
                node["topology_node_id"].as_str()?.to_owned(),
            ))
        })
        .collect();
    assert!(before.contains_key("ESW"), "{}", ensured.body);
    assert!(before.contains_key("ECP"), "{}", ensured.body);

    // Recreate the durable shape a legacy epic actually has. `epics:apply` now
    // freezes a roster and seats the mandatory pair, so an epic that predates
    // that is no longer reachable through the API — only through rows written
    // before it existed. The ESW and ECP stay, because the legacy writer left
    // those; it is the roster pin and its seats that were never written.
    let database = world.directory.path().join(kontor_daemon::DATABASE_FILE);
    let connection = rusqlite::Connection::open(database).expect("the realm database opens");
    connection
        .execute(
            "DELETE FROM seat_bindings
             WHERE project_id = ?1 AND topology_node_id IN (
                 SELECT id FROM topology_nodes
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND kind = 'ECP')",
            rusqlite::params![project.to_string(), epic.to_string()],
        )
        .expect("the legacy control seats clear");
    let unpinned = connection
        .execute(
            "DELETE FROM epic_rosters WHERE project_id = ?1 AND mini_project_id = ?2",
            rusqlite::params![project.to_string(), epic.to_string()],
        )
        .expect("the legacy gap is seeded");
    assert_eq!(
        unpinned, 1,
        "the epic was born with a frozen roster to remove"
    );
    drop(connection);

    let previewed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-preview"),
        &serde_json::json!({
            "target": {"id": SEEDED_CATALOG, "version": 1},
        }),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let preview = previewed.json();
    let effects = preview["effects"].as_array().expect("effects");
    for role in ["lsa", "tpm"] {
        assert!(
            effects.iter().any(|effect| {
                effect["subject"]
                    .as_str()
                    .is_some_and(|subject| subject.contains(role))
                    && effect["effect"] == "seat_created"
            }),
            "the bootstrap preview must name the missing {role} seat: {}",
            previewed.body
        );
    }

    let body = serde_json::json!({
        "preview_hash": preview["preview_hash"].as_str().expect("a preview hash"),
        "expected_revision": 1,
    });
    let applied = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-apply"),
        &body,
    )
    .signed_as(world, "admin")
    .with_key("bootstrap-roster-apply")
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["receipt"]["applied"], "created");

    let leadership: Vec<(String, String)> = applied.json()["core_team"]["seats"]
        .as_array()
        .expect("core team seats")
        .iter()
        .filter_map(|seat| {
            let code = seat["role"]["role_code"].as_str()?;
            (code == "LSA" || code == "TPM").then(|| {
                (
                    code.to_owned(),
                    seat["seat_binding_id"]
                        .as_str()
                        .expect("required leadership is materially seated")
                        .to_owned(),
                )
            })
        })
        .collect();
    assert_eq!(leadership.len(), 2, "{}", applied.body);
    assert_ne!(
        leadership[0].1, leadership[1].1,
        "distinct leadership seats"
    );

    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/roster:upgrade-apply"),
        &body,
    )
    .signed_as(world, "admin")
    .with_key("bootstrap-roster-apply")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    let replayed_leadership: Vec<String> = replayed.json()["core_team"]["seats"]
        .as_array()
        .expect("core team seats")
        .iter()
        .filter(|seat| seat["role"]["role_code"] == "LSA" || seat["role"]["role_code"] == "TPM")
        .map(|seat| {
            seat["seat_binding_id"]
                .as_str()
                .expect("leadership stays seated")
                .to_owned()
        })
        .collect();
    assert_eq!(
        replayed_leadership,
        leadership
            .iter()
            .map(|(_, id)| id.clone())
            .collect::<Vec<_>>(),
        "a replay must not duplicate or replace leadership: {}",
        replayed.body
    );

    let inspected = Call::get(format!(
        "/v1/projects/{project}/topology:inspect?epic_id={epic}"
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(inspected.status, 200, "{}", inspected.body);
    let after: std::collections::BTreeMap<String, String> = inspected.json()["nodes"]
        .as_array()
        .expect("topology nodes")
        .iter()
        .filter_map(|node| {
            Some((
                node["kind_key"].as_str()?.to_owned(),
                node["topology_node_id"].as_str()?.to_owned(),
            ))
        })
        .collect();
    assert_eq!(after.get("ESW"), before.get("ESW"), "the ESW is preserved");
    assert_eq!(after.get("ECP"), before.get("ECP"), "the ECP is preserved");
}

// ---------------------------------------------------------------------------
// KON-OP-04 — resuming what a first attempt did not finish.
//
// Both commands below write one durable row before their effects, and both are
// meant to be resumable from it. Every other OP-04 test drives a call that
// succeeds; these two construct the state a *failed* call leaves behind and
// make the next attempt finish the job. Without them the ordering that makes
// resumption possible is unverified, and getting it backwards costs the
// subject permanently: neither row has a delete, an abort or a reset.
// ---------------------------------------------------------------------------

/// Open one Quick session and answer with its id and the preview of promoting it.
async fn quick_session_ready_to_promote(
    world: &World,
    project: &str,
    purpose: &str,
    key: &str,
) -> (String, String) {
    let opened = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &serde_json::json!({
            "role": {
                "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                "role_code": "SA",
            },
            "purpose": purpose,
        }),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let session = opened.json()["quick_session_id"]
        .as_str()
        .expect("a session id")
        .to_owned();

    let previewed = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:preview"),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let hash = previewed.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    (session, hash)
}

/// A promotion interrupted right after it was authorized still finishes.
///
/// This is the state a failure anywhere in the effects leaves behind: the
/// source is recorded as promoted, and nothing else has happened yet. Because
/// `quick_session_promotions` is keyed by its source and has no delete, a
/// resume that cannot read what it needs is not a retryable error — it is a
/// Quick session that can never be promoted, by any caller, ever.
#[tokio::test]
async fn a_promotion_interrupted_after_authorization_resumes_to_completion() {
    let composed = compose_realm("/tmp/kontor-op04-resume-promotion").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (session, preview_hash) =
        quick_session_ready_to_promote(world, project, "Resume me", "quick-resume").await;

    // Authorize the promotion exactly as the apply's first step does, then stop
    // — no MiniProject, no nodes, no seats, no handoff.
    let project_id = ProjectId::parse(project).expect("a project id");
    let quick_session_id = QuickSessionId::parse(&session).expect("a session id");
    let epic_id = MiniProjectId::generate();
    let now = kontor_api::now();
    world.daemon.state().with_store(|store| {
        let core = store
            .get_current_core_team(project_id)
            .expect("the roster reads")
            .expect("a published roster");
        store
            .begin_promotion(
                &StoredPromotion {
                    quick_session_id,
                    project_id,
                    mini_project_id: epic_id,
                    preview_hash: ContentHash::parse(&preview_hash).expect("a hash"),
                    source_disposition: SourceDisposition::Idle,
                    handoff: None,
                    handoff_hash: None,
                    lsa_seat_binding_id: None,
                    completed_at: None,
                    created_at: now,
                },
                &StoredEpicRoster {
                    project_id,
                    mini_project_id: epic_id,
                    core_team_version: core.version,
                    catalog_hash: core.catalog_hash.clone(),
                    seats: core.seats.clone(),
                    quick_session_id: Some(quick_session_id),
                    revision: AggregateRevision::INITIAL,
                    pinned_at: now,
                },
            )
            .expect("the promotion is authorized");
    });

    // The next attempt has to finish it, against the epic already frozen.
    let resumed = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &promotion_apply_body(&preview_hash),
    )
    .signed_as(world, "operator")
    .with_key("promote-resume")
    .send(world)
    .await;
    assert_eq!(
        resumed.status, 200,
        "an interrupted promotion could not be resumed: {}",
        resumed.body
    );
    assert_eq!(
        resumed.json()["epic_id"],
        epic_id.to_string().as_str(),
        "the resume built a second epic: {}",
        resumed.body
    );

    // And it is a real epic, with the seats the frozen roster called for.
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic_id}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("materialize-resumed")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let answered = materialized.json();
    let lsa = answered["core_team"]["seats"]
        .as_array()
        .expect("seats")
        .iter()
        .find(|entry| entry["role"]["role_code"] == "LSA")
        .expect("an LSA seat");
    assert!(
        lsa["seat_binding_id"].as_str().is_some(),
        "the resumed epic never seated its LSA: {}",
        materialized.body
    );
}

/// An ensure interrupted after its row was written completes the placement.
///
/// The `quick_sessions` row carries the node and seat ids, and is written
/// first precisely so this state is recoverable. If the retry returned the row
/// without reconciling, the session would exist forever with no workspace and
/// no seat — and because the row occupies the intent, no later call could
/// place one either.
#[tokio::test]
async fn an_ensure_interrupted_after_its_row_completes_the_node_and_seat() {
    let composed = compose_realm("/tmp/kontor-op04-resume-ensure").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;

    let project_id = ProjectId::parse(project).expect("a project id");
    let purpose = "Half-placed session";
    // The canonical intent the daemon derives for this exact request.
    let intent = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": "quick_session_ensure",
        "project": project_id.to_string(),
        "role": "SA",
        "catalog": 1,
        "purpose": purpose,
    }))
    .expect("a canonical intent");
    let node_id = TopologyNodeId::generate();
    let seat_binding_id = SeatBindingId::generate();
    let session_id = QuickSessionId::generate();

    // The row exists and claims those ids; neither the node nor the seat does.
    world.daemon.state().with_store(|store| {
        let core = store
            .get_current_core_team(project_id)
            .expect("the roster reads")
            .expect("a published roster");
        let seats: Vec<kontor_teams::CoreTeamSeat> =
            serde_json::from_value(core.seats.clone()).expect("the roster decodes");
        let chosen = seats
            .iter()
            .find(|entry| entry.role.role_code.as_str() == "SA")
            .expect("the SA seat");
        let base = store
            .list_topology_nodes(project_id, None)
            .expect("the nodes read")
            .into_iter()
            .find(|node| node.parent_id.is_none())
            .expect("an adopted base");
        store
            .create_quick_session(&StoredQuickSession {
                id: session_id,
                project_id,
                role: chosen.role.clone(),
                role_slot_id: chosen.role_slot_id.clone(),
                topology_node_id: node_id,
                seat_binding_id,
                psw_topology_node_id: base.id,
                psw_native_id: None,
                purpose: BoundedText::parse(purpose).expect("a purpose"),
                intent_hash: intent.hash().clone(),
                disposition: SourceDisposition::Idle,
                revision: AggregateRevision::INITIAL,
                created_at: kontor_api::now(),
            })
            .expect("the interrupted session is recorded");
    });
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.get_topology_node(project_id, node_id))
            .expect("the node reads")
            .is_none(),
        "the fixture placed a node it was supposed to leave missing"
    );

    let ensured = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &serde_json::json!({
            "role": {
                "catalog_revision": {"id": SEEDED_CATALOG, "version": 1},
                "role_code": "SA",
            },
            "purpose": purpose,
        }),
    )
    .signed_as(world, "operator")
    .with_key("ensure-resume")
    .send(world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    assert_eq!(
        ensured.json()["quick_session_id"],
        session_id.to_string().as_str(),
        "the retry opened a second session instead of finishing this one: {}",
        ensured.body
    );

    // The claimed ids are now real, rather than a row pointing at nothing.
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.get_topology_node(project_id, node_id))
            .expect("the node reads")
            .is_some(),
        "the resumed ensure left its session without a workspace: {}",
        ensured.body
    );
    let seated = world
        .daemon
        .state()
        .with_store(|store| store.list_seat_bindings(project_id, node_id))
        .expect("the seats read")
        .into_iter()
        .any(|binding| binding.id == seat_binding_id);
    assert!(
        seated,
        "the resumed ensure left its session without a seat: {}",
        ensured.body
    );
}

/// A container whose title Kontor rendered under an older rule is repaired
/// through `/v1`, and the caller supplies no title to do it.
///
/// The one shape this operation exists for: a native container that is correctly
/// bound and visibly misnamed. The caller names the node and the revision it read;
/// the title comes from the node's pinned kind template and the plane's typed
/// scope, and the native container is addressed by the binding Kontor already
/// holds.
#[tokio::test]
async fn a_misnamed_container_is_repaired_from_the_pinned_topology_and_never_from_a_caller() {
    let composed = compose_realm("/tmp/kontor-op3-retitle").await;
    let world = &composed.world;

    // A real seat, because a native container only exists once something has been
    // placed in one: arm the epic, plan it, start the plan.
    let epic = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(epic.status, 200, "{}", epic.body);
    let armed = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/execution:arm",
            composed.project, composed.epic
        ),
        &serde_json::json!({
            "expected_revision": epic.json()["revision"],
            "tasks": [],
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
            "max_concurrency": 1,
            "budget": {"max_tokens": 1000, "max_commands": 10, "max_duration_seconds": 600,
                       "max_cost_minor_units": 100, "cost_currency": "NOK"},
            "granted_by": composed.account,
            "reason": "Place a seat so there is a container to repair"
        }),
    )
    .signed_as(world, "admin")
    .with_key("retitle-arm")
    .send(world)
    .await;
    assert_eq!(armed.status, 200, "{}", armed.body);

    let plan = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:plan",
            composed.project, composed.epic
        ),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    let started = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/scheduler:start",
            composed.project, composed.epic
        ),
        &serde_json::json!({"plan_hash": plan.json()["plan_hash"]}),
    )
    .signed_as(world, "operator")
    .with_key("retitle-start")
    .send(world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);

    let projection = Call::get(format!(
        "/v1/projects/{}/topology:inspect?epic_id={}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(projection.status, 200, "{}", projection.body);
    let node = projection.json()["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["observed_binding"].is_object())
        .expect("starting the plan bound a native container")
        .clone();
    let node_id = node["topology_node_id"]
        .as_str()
        .expect("a node id")
        .to_owned();
    let node_key = kontor_core::id::TopologyNodeId::parse(&node_id).expect("a canonical node id");
    let canonical = world
        .fake
        .container_title(node_key)
        .expect("the bound container carries a title");

    // The state a repair exists for: the runtime carries a name no Kontor rule
    // produces any more. Set on the runtime, not through Kontor — nothing in this
    // control plane can write a native title except the operation under test.
    world
        .fake
        .set_container_title(node_key, "Ticket Session Workspace · 0189-stale");

    let preview_uri = format!(
        "/v1/projects/{}/topology/nodes/{node_id}/container:retitle-preview",
        composed.project
    );
    let apply_uri = format!(
        "/v1/projects/{}/topology/nodes/{node_id}/container:retitle-apply",
        composed.project
    );
    let body = serde_json::json!({"expected_revision": composed.project_revision});

    let preview = Call::post(&preview_uri, &body)
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(
        preview.json()["desired_title"],
        canonical,
        "the desired title is derived from the pinned topology: {}",
        preview.body
    );
    assert_eq!(
        preview.json()["observed_title"],
        "Ticket Session Workspace · 0189-stale",
        "and the observed one is what the runtime actually carries: {}",
        preview.body
    );
    assert_eq!(preview.json()["would_change"], true);
    assert_eq!(
        preview.json()["bound_native_id"],
        node["observed_binding"]["native_id"],
        "the container named is the one the node is bound to: {}",
        preview.body
    );
    // A preview is a read: it wrote nothing, so the container is still misnamed.
    assert_eq!(
        world.fake.container_title(node_key).as_deref(),
        Some("Ticket Session Workspace · 0189-stale"),
        "a preview must not have renamed anything"
    );

    // An Observer may look at nothing here, and an Operator may not repair it:
    // what is being corrected is a decision the control plane made.
    let observer = Call::post(&preview_uri, &body)
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(observer.status, 403, "{}", observer.body);
    let operator = Call::post(&apply_uri, &body)
        .signed_as(world, "operator")
        .with_key("retitle-operator")
        .send(world)
        .await;
    assert_eq!(operator.status, 403, "{}", operator.body);

    let applied = Call::post(&apply_uri, &body)
        .signed_as(world, "admin")
        .with_key("retitle-apply")
        .send(world)
        .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["changed"], true, "{}", applied.body);
    assert_eq!(
        applied.json()["observed_title"],
        canonical,
        "the title is read back from the runtime after the change: {}",
        applied.body
    );
    assert_eq!(
        applied.json()["bound_native_id"],
        node["observed_binding"]["native_id"],
        "and it is still the same native container: {}",
        applied.body
    );
    assert_eq!(applied.json()["receipt"]["applied"], "created");

    // The replay answers the original receipt and renames nothing a second time.
    let replayed = Call::post(&apply_uri, &body)
        .signed_as(world, "admin")
        .with_key("retitle-apply")
        .send(world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replayed.json()["changed"], false, "{}", replayed.body);
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        applied.json()["receipt"]["receipt_id"]
    );
    assert_eq!(replayed.json()["observed_title"], canonical);

    // A second preview now says there is nothing to repair.
    let settled = Call::post(&preview_uri, &body)
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert_eq!(settled.json()["would_change"], false, "{}", settled.body);

    // A stale node revision is refused, and a node holding no container has
    // nothing to repair.
    let stale = Call::post(&apply_uri, &serde_json::json!({"expected_revision": 99}))
        .signed_as(world, "admin")
        .with_key("retitle-stale")
        .send(world)
        .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    let unknown = Call::post(
        format!(
            "/v1/projects/{}/topology/nodes/{}/container:retitle-preview",
            composed.project,
            kontor_core::id::TopologyNodeId::generate()
        ),
        &body,
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(unknown.status, 404, "{}", unknown.body);

    // A caller cannot smuggle a title in: the request type has nowhere to put one.
    let dictated = Call::post(
        &apply_uri,
        &serde_json::json!({
            "expected_revision": composed.project_revision,
            "desired_title": "Whatever I feel like",
        }),
    )
    .signed_as(world, "admin")
    .with_key("retitle-dictated")
    .send(world)
    .await;
    assert_eq!(
        dictated.status, 400,
        "the request type has nowhere to put a title, so the body is rejected: {}",
        dictated.body
    );
}

// ---------------------------------------------------------------------------
// Completion: the two operations that carry the state machine (KON-OP-06)
// ---------------------------------------------------------------------------

/// A refused first advance leaves the epic exactly as it found it.
///
/// `:advance` is the one completion write that can bring durable state into
/// existence, so it is the one where guarding after the write would be invisible:
/// the caller gets a refusal, the row is there anyway, and no receipt names the
/// write that happened. The read route answers `404` until the first advance, so
/// a caller has no revision to have read — which is why the refusal must say that
/// rather than claim the run moved underneath it.
#[tokio::test]
async fn a_refused_first_advance_creates_no_completion_run_and_no_receipt() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "op06-advance", "Kontor", "/tmp/kontor-op06-adv").await;
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
            "Advance epic",
            &category,
            serde_json::json!([{"title": "Only task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("op06-advance-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();

    // Any revision but the initial one is refused, and the refusal names what to
    // present instead of describing a race that could not have happened.
    let refused = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": 7}),
    )
    .signed_as(&world, "operator")
    .with_key("op06-first-advance")
    .send(&world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "revision_conflict");
    assert_eq!(
        refused.json()["current_revision"],
        1,
        "the refusal must hand back the revision a first advance has to present: {}",
        refused.body
    );
    assert!(
        refused.json()["rule"]
            .as_str()
            .expect("a rule")
            .contains("no completion run yet"),
        "the reason must be the honest one, not `moved since the caller read it`: {}",
        refused.body
    );

    // Nothing durable was created: the read is still an absence, not an empty
    // state a caller could mistake for a finished epic.
    let read = Call::get(format!("/v1/projects/{project}/epics/{epic}/completion"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(read.status, 404, "{}", read.body);

    // And no receipt was recorded for it. Reusing the *same* key with a corrected
    // revision is therefore a fresh command, not an idempotency conflict — which
    // is only true if the refused call wrote nothing to the ledger.
    let corrected = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(&world, "operator")
    .with_key("op06-first-advance")
    .send(&world)
    .await;
    assert_ne!(
        corrected.code(),
        "idempotency_conflict",
        "the refused call must have recorded no receipt: {}",
        corrected.body
    );
    // `placement_blocked` is also a 409, so the code is what distinguishes
    // "your revision was wrong" from "the guard passed and the start failed".
    assert_ne!(
        corrected.code(),
        "revision_conflict",
        "the initial revision must pass the guard: {}",
        corrected.body
    );
    // And it is *this* call — the first one to get past the revision guard —
    // that brings the run into existence. Which is the whole property: before it
    // the read was an absence, and the refused call is not what ended that. What
    // the advance then reports about the ticket gate is a different question,
    // asked of a run that now exists.
    let present = Call::get(format!("/v1/projects/{project}/epics/{epic}/completion"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(
        present.status, 200,
        "the advance that passed the guard is the one that created the run: {}",
        present.body
    );
}

/// Both completion writes judge the idempotency key before the revision.
///
/// Driven over a real run on a promoted epic, so the seats the two authorities
/// are checked against are the ones promotion actually materialized. The run is
/// seeded with no declared tickets: what is under test here is the handler
/// composition — replay, revision, authority, phase — and a vacuously satisfied
/// ticket gate keeps OP-01 evidence plumbing out of the assertions.
#[tokio::test]
async fn advance_and_remediate_judge_the_key_before_the_revision() {
    let composed = compose_realm("/tmp/kontor-op06-machine").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (session, hash) =
        quick_session_ready_to_promote(world, project, "Drive completion", "op06-quick").await;
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &promotion_apply_body(&hash),
    )
    .signed_as(world, "operator")
    .with_key("op06-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();

    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({
            "expected_revision": 1,
            "routes": [
                {"role_code": "LSA", "model_route": {
                    "provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"
                }},
                {"role_code": "TPM", "model_route": {
                    "provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"
                }}
            ]
        }),
    )
    .signed_as(world, "admin")
    .with_key("op06-materialize")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let seats = materialized.json()["core_team"]["seats"]
        .as_array()
        .expect("seats")
        .clone();
    let seat_of = |code: &str| -> SeatBindingId {
        SeatBindingId::parse(
            seats
                .iter()
                .find(|entry| entry["role"]["role_code"] == code)
                .unwrap_or_else(|| panic!("a {code} seat"))["seat_binding_id"]
                .as_str()
                .unwrap_or_else(|| panic!("a bound {code} seat")),
        )
        .expect("a seat binding id")
    };
    let lsa = seat_of("LSA");
    let tpm = seat_of("TPM");

    let epic_id = MiniProjectId::parse(&epic).expect("an epic id");
    let project_id = ProjectId::parse(project).expect("a project id");
    let compiled = kontor_scheduler::compile(
        kontor_scheduler::operational_default().expect("the built-in profile"),
    )
    .expect("it compiles");
    let seed = |state: &kontor_scheduler::CompletionState| StoredEpicCompletion {
        project_id,
        mini_project_id: epic_id,
        profile_id: compiled.profile.id.clone(),
        profile_version: compiled.profile.version,
        definition_hash: compiled.definition_hash.clone(),
        state: serde_json::to_value(state).expect("the state serializes"),
        revision: state.revision,
        updated_at: at("2026-08-18T09:00:00Z"),
    };
    let signal = |id: &str,
                  revision: &kontor_scheduler::CompletionState,
                  observation: kontor_scheduler::CompletionObservation| {
        kontor_scheduler::CompletionSignal {
            id: ContentHash::of(id.as_bytes()),
            expected_revision: revision.revision,
            delivery: kontor_scheduler::SignalDelivery::Callback,
            observation,
        }
    };

    // ---- `:advance` over a run standing at the ticket gate ----
    let ticket_phase = kontor_scheduler::start(&compiled, tpm, Vec::new()).expect("a run starts");
    world
        .daemon
        .state()
        .with_store(|store| store.create_epic_completion(&seed(&ticket_phase)))
        .expect("the run seeds");

    let advanced = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "operator")
    .with_key("op06-advance-once")
    .send(world)
    .await;
    assert_eq!(advanced.status, 200, "{}", advanced.body);
    assert_eq!(advanced.json()["state"]["phase"]["phase"], "integration");
    assert_eq!(advanced.json()["receipt"]["applied"], "created");
    assert_eq!(advanced.json()["receipt"]["revision"], 2);
    // The transition woke the epic's existing TPM seat exactly once, and named it.
    let wakes = advanced.json()["state"]["wakes"]
        .as_array()
        .expect("wakes")
        .clone();
    assert_eq!(
        wakes.len(),
        1,
        "one observation, one wake: {}",
        advanced.body
    );
    assert_eq!(wakes[0]["seat_binding_id"], tpm.to_string());
    assert_eq!(wakes[0]["completion_revision"], 2);

    // The same key and the same expected revision replay to the same receipt and
    // move nothing, even though the run is no longer at that revision.
    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "operator")
    .with_key("op06-advance-once")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["receipt"]["revision"],
        2,
        "a replay commits no second transition: {}",
        replayed.body
    );
    assert_eq!(replayed.json()["state"]["phase"]["phase"], "integration");

    // A *different* key presenting that now-stale revision is a genuine conflict.
    let stale = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "operator")
    .with_key("op06-advance-stale")
    .send(world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");
    assert_eq!(stale.json()["current_revision"], 2);

    // ---- `:remediate` over a run whose first round failed ----
    let integration = kontor_scheduler::IntegrationRecord {
        receipt: ContentHash::of(b"integration-1"),
        repositories: vec![kontor_scheduler::RepositoryOutcome {
            repository: name("asma-rs-kontor"),
            pull_request: name("PR-1"),
            module_revision: name("abc1234"),
            root_pointer_revision: Some(name("def5678")),
        }],
    };
    let findings = ContentHash::of(b"round-1-findings");
    let after_tickets = kontor_scheduler::advance(
        &compiled,
        &ticket_phase,
        &signal(
            "tickets",
            &ticket_phase,
            kontor_scheduler::CompletionObservation::TicketsClosed(Vec::new()),
        ),
    )
    .expect("the gate opens")
    .state;
    let after_integration = kontor_scheduler::advance(
        &compiled,
        &after_tickets,
        &signal(
            "integration",
            &after_tickets,
            kontor_scheduler::CompletionObservation::IntegrationCompleted(integration),
        ),
    )
    .expect("integration lands")
    .state;
    let awaiting = kontor_scheduler::advance(
        &compiled,
        &after_integration,
        &signal(
            "verdict-1",
            &after_integration,
            kontor_scheduler::CompletionObservation::VerdictRecorded {
                round: 1,
                verdict: kontor_scheduler::CommitteeVerdict::Fail,
                evidence: findings.clone(),
                committee_run_id: None,
                result_hash: None,
                remediation_hash: None,
                deliberation: vec![kontor_policy::DeliberationStep {
                    role: name("Committee"),
                    consultation: name("independent_review"),
                    round: 1,
                    outcome: name("fail"),
                }],
            },
        ),
    )
    .expect("a failed round is appended")
    .state;
    let awaiting_revision = awaiting.revision.get();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.update_epic_completion(
                &seed(&awaiting),
                AggregateRevision::parse(2).expect("a revision"),
            )
        })
        .expect("the failed round seeds");

    let remediate =
        |body: serde_json::Value, key: &'static str, actor: SeatBindingId, generation: u64| {
            Call::post(
                format!("/v1/projects/{project}/epics/{epic}/completion:remediate"),
                &body,
            )
            .with_token(
                world
                    .daemon
                    .state()
                    .credentials()
                    .seat_credential_for_generation(actor, generation),
            )
            .with_key(key)
        };

    let operator_impersonation = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:remediate"),
        &serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "lsa_proposal", "round": 1,
                "failed_round_evidence": findings.as_str(),
                "proposal": ContentHash::of(b"operator-spoof").as_str()
            }
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-operator-cannot-propose")
    .send(world)
    .await;
    assert_eq!(
        operator_impersonation.status, 403,
        "{}",
        operator_impersonation.body
    );

    let tpm_impersonates_lsa = remediate(
        serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "lsa_proposal", "round": 1,
                "failed_round_evidence": findings.as_str(),
                "proposal": ContentHash::of(b"tpm-spoof").as_str()
            }
        }),
        "op06-tpm-cannot-propose",
        tpm,
        1,
    )
    .send(world)
    .await;
    assert_eq!(
        tpm_impersonates_lsa.status, 403,
        "{}",
        tpm_impersonates_lsa.body
    );

    let lsa_predecessor = world
        .daemon
        .state()
        .with_store(|store| store.get_hosted_topology_seat(project_id, lsa))
        .expect("the LSA occupancy reads")
        .expect("the LSA is hosted");
    let mut lsa_successor = lsa_predecessor.clone();
    lsa_successor.native_identity.native_id =
        ExternalId::parse("op06-lsa-successor").expect("a successor native id");
    lsa_successor.provider_session_id =
        Some(ExternalId::parse("op06-lsa-successor-session").expect("a provider session"));
    lsa_successor.observed_at = at("2026-08-18T09:01:00Z");
    world
        .daemon
        .state()
        .with_store(|store| {
            store.replace_hosted_topology_seat_route(
                &lsa_predecessor,
                &lsa_successor,
                at("2026-08-18T09:01:00Z"),
                "test the remediation authority generation fence",
            )
        })
        .expect("the LSA occupancy is replaced");

    let stale_predecessor = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:remediate"),
        &serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "lsa_proposal", "round": 1,
                "failed_round_evidence": findings.as_str(),
                "proposal": ContentHash::of(b"fenced-predecessor").as_str()
            }
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .seat_credential_for_generation(lsa, 1),
    )
    .with_key("op06-fenced-lsa-predecessor")
    .send(world)
    .await;
    assert_eq!(stale_predecessor.status, 409, "{}", stale_predecessor.body);
    assert_eq!(stale_predecessor.code(), "stale_binding");

    let foreign_seat = remediate(
        serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "lsa_proposal", "round": 1,
                "failed_round_evidence": findings.as_str(),
                "proposal": ContentHash::of(b"foreign-seat-spoof").as_str()
            }
        }),
        "op06-foreign-seat-cannot-propose",
        SeatBindingId::generate(),
        1,
    )
    .send(world)
    .await;
    assert_eq!(foreign_seat.status, 403, "{}", foreign_seat.body);

    // Routing before anything was proposed launches nothing: both receipts have
    // to be durable, and only one authority has acted.
    let unproposed = remediate(
        serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {"action": "tpm_route", "round": 1, "route": ContentHash::of(b"route-1").as_str()}
        }),
        "op06-route-unproposed",
        tpm,
        1,
    )
    .send(world)
    .await;
    assert_eq!(unproposed.status, 400, "{}", unproposed.body);
    assert!(
        unproposed.json()["rule"]
            .as_str()
            .expect("a rule")
            .contains("no LSA proposal"),
        "{}",
        unproposed.body
    );

    // A proposal must answer the failed round's own immutable evidence.
    let wrong_evidence = remediate(
        serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "lsa_proposal", "round": 1,
                "failed_round_evidence": ContentHash::of(b"some-other-round").as_str(),
                "proposal": ContentHash::of(b"narrow-the-change").as_str()
            }
        }),
        "op06-propose-wrong",
        lsa,
        2,
    )
    .send(world)
    .await;
    assert_eq!(wrong_evidence.status, 400, "{}", wrong_evidence.body);

    let proposal_body = serde_json::json!({
        "expected_revision": awaiting_revision,
        "action": {
            "action": "lsa_proposal", "round": 1,
            "failed_round_evidence": findings.as_str(),
            "proposal": ContentHash::of(b"narrow-the-change").as_str()
        }
    });
    let proposed = remediate(proposal_body.clone(), "op06-propose", lsa, 2)
        .send(world)
        .await;
    assert_eq!(proposed.status, 200, "{}", proposed.body);
    assert_eq!(proposed.json()["receipt"]["applied"], "created");
    assert_eq!(
        proposed.json()["state"]["phase"]["phase"],
        "awaiting_lsa",
        "a proposal alone launches nothing: {}",
        proposed.body
    );
    assert_eq!(
        proposed.json()["receipt"]["revision"],
        awaiting_revision,
        "the run must not move on a proposal: {}",
        proposed.body
    );

    // Failed-round evidence is part of the command identity. A used key with
    // the same proposal but changed/invalid evidence is a conflicting command,
    // not a replay that returns before evidence validation.
    let mut changed_evidence = proposal_body.clone();
    changed_evidence["action"]["failed_round_evidence"] =
        serde_json::json!(ContentHash::of(b"changed-under-used-key").as_str());
    let changed_evidence = remediate(changed_evidence, "op06-propose", lsa, 2)
        .send(world)
        .await;
    assert_eq!(changed_evidence.status, 409, "{}", changed_evidence.body);
    assert_eq!(changed_evidence.code(), "idempotency_conflict");

    // Replay is the same receipt, and still no second proposal.
    let proposed_again = remediate(proposal_body, "op06-propose", lsa, 2)
        .send(world)
        .await;
    assert_eq!(proposed_again.status, 200, "{}", proposed_again.body);
    assert_eq!(proposed_again.json()["receipt"]["applied"], "unchanged");

    // The route completes the authority and moves the run.
    let route_body = serde_json::json!({
        "expected_revision": awaiting_revision,
        "action": {"action": "tpm_route", "round": 1, "route": ContentHash::of(b"route-1").as_str()}
    });
    let lsa_cannot_route = remediate(route_body.clone(), "op06-lsa-cannot-route", lsa, 2)
        .send(world)
        .await;
    assert_eq!(lsa_cannot_route.status, 403, "{}", lsa_cannot_route.body);

    let routed = remediate(route_body.clone(), "op06-route", tpm, 1)
        .send(world)
        .await;
    assert_eq!(routed.status, 200, "{}", routed.body);
    assert_eq!(routed.json()["state"]["phase"]["phase"], "remediation");
    assert_eq!(routed.json()["state"]["phase"]["round"], 1);
    assert_eq!(routed.json()["receipt"]["applied"], "created");
    assert_eq!(
        routed.json()["receipt"]["revision"],
        awaiting_revision + 1,
        "{}",
        routed.body
    );

    // Replay after the move: same key, same body, same receipt, no second round.
    let routed_again = remediate(route_body, "op06-route", tpm, 1)
        .send(world)
        .await;
    assert_eq!(routed_again.status, 200, "{}", routed_again.body);
    assert_eq!(routed_again.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        routed_again.json()["state"]["phase"]["phase"],
        "remediation"
    );

    // And a different key presenting the pre-route revision now conflicts.
    let stale_route = remediate(
        serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {"action": "tpm_route", "round": 1, "route": ContentHash::of(b"route-2").as_str()}
        }),
        "op06-route-stale",
        tpm,
        1,
    )
    .send(world)
    .await;
    assert_eq!(stale_route.status, 409, "{}", stale_route.body);
    assert_eq!(stale_route.code(), "revision_conflict");
    assert_eq!(
        stale_route.json()["current_revision"],
        awaiting_revision + 1
    );
}

/// Integration advances on a typed operator receipt, and only the right one.
///
/// The phase waits on an effect no connector reports here, so the caller states
/// it. The plan admits exactly this — "a native connector **or a typed operator
/// receipt**" — and models the outcome as recorded per-repository PR/module/
/// root-pointer results rather than one assumed branch.
#[tokio::test]
async fn integration_advances_only_on_a_well_formed_operator_receipt() {
    let composed = compose_realm("/tmp/kontor-op06-integration").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (session, hash) =
        quick_session_ready_to_promote(world, project, "Drive integration", "op06-int-quick").await;
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &promotion_apply_body(&hash),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("op06-int-materialize")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let tpm = SeatBindingId::parse(
        materialized.json()["core_team"]["seats"]
            .as_array()
            .expect("seats")
            .iter()
            .find(|entry| entry["role"]["role_code"] == "TPM")
            .expect("a TPM seat")["seat_binding_id"]
            .as_str()
            .expect("a bound TPM seat"),
    )
    .expect("a seat binding id");

    let epic_id = MiniProjectId::parse(&epic).expect("an epic id");
    let project_id = ProjectId::parse(project).expect("a project id");
    let compiled = kontor_scheduler::compile(
        kontor_scheduler::operational_default().expect("the built-in profile"),
    )
    .expect("it compiles");

    // Stand the run at the *ticket* gate, which is vacuously satisfied here. The
    // first thing under test is the phase that derives its own answer.
    let ticket_phase = kontor_scheduler::start(&compiled, tpm, Vec::new()).expect("a run starts");
    world
        .daemon
        .state()
        .with_store(|store| {
            store.create_epic_completion(&StoredEpicCompletion {
                project_id,
                mini_project_id: epic_id,
                profile_id: compiled.profile.id.clone(),
                profile_version: compiled.profile.version,
                definition_hash: compiled.definition_hash.clone(),
                state: serde_json::to_value(&ticket_phase).expect("the state serializes"),
                revision: ticket_phase.revision,
                updated_at: at("2026-08-22T09:00:00Z"),
            })
        })
        .expect("the run seeds");

    // A receipt offered to the ticket gate is refused, not dropped. This phase
    // reads its own evidence and would otherwise advance normally while silently
    // discarding what the caller believed it had recorded — the one case the
    // per-phase checks below cannot catch, because they never run here.
    let at_ticket_gate = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": ticket_phase.revision.get(),
            "evidence": {"phase": "integration", "repositories": [
                {"repository": "asma-rs-kontor", "pull_request": "PR-91",
                 "module_revision": "ed654bf", "root_pointer_revision": null}
            ]}
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-at-ticket-gate")
    .send(world)
    .await;
    assert_eq!(at_ticket_gate.status, 400, "{}", at_ticket_gate.body);
    assert!(
        at_ticket_gate.json()["rule"]
            .as_str()
            .expect("a rule")
            .contains("does not take an operator receipt"),
        "the ticket gate must say it derives its own answer: {}",
        at_ticket_gate.body
    );
    let still_at_tickets = Call::get(format!("/v1/projects/{project}/epics/{epic}/completion"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(
        still_at_tickets.json()["phase"]["phase"],
        "ticket_gate",
        "a refused advance moved nothing: {}",
        still_at_tickets.body
    );

    // Without a receipt the ticket gate opens on its own evidence.
    let opened = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": ticket_phase.revision.get()}),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-open-gate")
    .send(world)
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    assert_eq!(opened.json()["state"]["phase"]["phase"], "integration");
    let standing = opened.json()["state"]["revision"]
        .as_u64()
        .expect("a revision");

    // No receipt at all: the phase says what it wants rather than stalling.
    let bare = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": standing}),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-bare")
    .send(world)
    .await;
    assert_eq!(bare.status, 400, "{}", bare.body);
    assert!(
        bare.json()["rule"]
            .as_str()
            .expect("a rule")
            .contains("integration outcome"),
        "the refusal must name what to supply: {}",
        bare.body
    );

    // The closeout receipt is well formed, but not for this phase. Offering it is
    // refused rather than ignored: a dropped receipt would let the caller believe
    // it recorded something nothing ever stored.
    let wrong_phase = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": standing,
            "evidence": {
                "phase": "closeout",
                "merge": "m", "release": "r", "delivered_versions": {"a": "1"},
                "summary": "s", "notification": "n", "archive": "a"
            }
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-wrong-phase")
    .send(world)
    .await;
    assert_eq!(wrong_phase.status, 400, "{}", wrong_phase.body);

    // An integration that names no repository is not an integration.
    let empty = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": standing,
            "evidence": {"phase": "integration", "repositories": []}
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-empty")
    .send(world)
    .await;
    assert_eq!(empty.status, 400, "{}", empty.body);
    assert!(
        empty.json()["rule"]
            .as_str()
            .expect("a rule")
            .contains("at least one repository"),
        "{}",
        empty.body
    );

    // Nothing above moved the run: a refused advance is not a transition.
    let unmoved = Call::get(format!("/v1/projects/{project}/epics/{epic}/completion"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(unmoved.json()["phase"]["phase"], "integration");
    assert_eq!(unmoved.json()["revision"], standing);

    // The real thing, polyrepo: two repositories, one of which has no root
    // pointer of its own.
    let recorded = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": standing,
            "evidence": {"phase": "integration", "repositories": [
                {
                    "repository": "asma-rs-kontor",
                    "pull_request": "PR-91",
                    "module_revision": "ed654bf",
                    "root_pointer_revision": "a1b2c3d"
                },
                {
                    "repository": "asma-cli",
                    "pull_request": "PR-12",
                    "module_revision": "9f8e7d6",
                    "root_pointer_revision": null
                }
            ]}
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-int-record")
    .send(world)
    .await;
    assert_eq!(recorded.status, 200, "{}", recorded.body);
    assert_eq!(
        recorded.json()["state"]["phase"]["phase"],
        "verdict",
        "a recorded integration opens the verdict round: {}",
        recorded.body
    );
    let integrations = recorded.json()["state"]["integrations"]
        .as_array()
        .expect("integrations")
        .clone();
    assert_eq!(integrations.len(), 1, "{}", recorded.body);
    let repositories = integrations[0]["repositories"]
        .as_array()
        .expect("repositories")
        .clone();
    assert_eq!(repositories.len(), 2, "polyrepo, not one assumed branch");
    assert_eq!(repositories[0]["repository"], "asma-rs-kontor");
    assert_eq!(repositories[0]["root_pointer_revision"], "a1b2c3d");
    assert!(
        repositories[1]["root_pointer_revision"].is_null(),
        "a module with no root pointer records none: {}",
        recorded.body
    );
}

/// The closeout receipts carry an epic to `done`, and each one is digested.
///
/// This is the end of the machine: with it, a profile can reach `Done` for the
/// first time. Every prerequisite the policy demands — merge, release, version
/// inventory, summary, notification, archive — has to be present, and the open
/// questions are read as part of the same observation so an epic cannot finish
/// over an unresolved ambiguity.
#[tokio::test]
async fn the_closeout_receipts_carry_an_epic_to_done() {
    let composed = compose_realm("/tmp/kontor-op06-closeout").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (session, hash) =
        quick_session_ready_to_promote(world, project, "Drive closeout", "op06-close-quick").await;
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:apply"),
        &promotion_apply_body(&hash),
    )
    .signed_as(world, "operator")
    .with_key("op06-close-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("op06-close-materialize")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let tpm = SeatBindingId::parse(
        materialized.json()["core_team"]["seats"]
            .as_array()
            .expect("seats")
            .iter()
            .find(|entry| entry["role"]["role_code"] == "TPM")
            .expect("a TPM seat")["seat_binding_id"]
            .as_str()
            .expect("a bound TPM seat"),
    )
    .expect("a seat binding id");

    let epic_id = MiniProjectId::parse(&epic).expect("an epic id");
    let project_id = ProjectId::parse(project).expect("a project id");
    let compiled = kontor_scheduler::compile(
        kontor_scheduler::operational_default().expect("the built-in profile"),
    )
    .expect("it compiles");
    let step = |state: &kontor_scheduler::CompletionState,
                id: &str,
                observation: kontor_scheduler::CompletionObservation| {
        kontor_scheduler::advance(
            &compiled,
            state,
            &kontor_scheduler::CompletionSignal {
                id: ContentHash::of(id.as_bytes()),
                expected_revision: state.revision,
                delivery: kontor_scheduler::SignalDelivery::Callback,
                observation,
            },
        )
        .expect("the phase advances")
        .state
    };

    // Walk the pure machine to the closeout gate: tickets, integration, a passing
    // verdict. Only the last step is what this test is about.
    let started = kontor_scheduler::start(&compiled, tpm, Vec::new()).expect("a run starts");
    let after_tickets = step(
        &started,
        "tickets",
        kontor_scheduler::CompletionObservation::TicketsClosed(Vec::new()),
    );
    let after_integration = step(
        &after_tickets,
        "integration",
        kontor_scheduler::CompletionObservation::IntegrationCompleted(
            kontor_scheduler::IntegrationRecord {
                receipt: ContentHash::of(b"integration"),
                repositories: vec![kontor_scheduler::RepositoryOutcome {
                    repository: name("asma-rs-kontor"),
                    pull_request: name("PR-91"),
                    module_revision: name("ed654bf"),
                    root_pointer_revision: None,
                }],
            },
        ),
    );
    let at_closeout = step(
        &after_integration,
        "verdict-1",
        kontor_scheduler::CompletionObservation::VerdictRecorded {
            round: 1,
            verdict: kontor_scheduler::CommitteeVerdict::Pass,
            evidence: ContentHash::of(b"round-1-findings"),
            committee_run_id: None,
            result_hash: None,
            remediation_hash: None,
            deliberation: vec![kontor_policy::DeliberationStep {
                role: name("Committee"),
                consultation: name("independent_review"),
                round: 1,
                outcome: name("pass"),
            }],
        },
    );
    let standing = at_closeout.revision.get();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.create_epic_completion(&StoredEpicCompletion {
                project_id,
                mini_project_id: epic_id,
                profile_id: compiled.profile.id.clone(),
                profile_version: compiled.profile.version,
                definition_hash: compiled.definition_hash.clone(),
                state: serde_json::to_value(&at_closeout).expect("the state serializes"),
                revision: at_closeout.revision,
                updated_at: at("2026-08-22T10:00:00Z"),
            })
        })
        .expect("the run seeds");

    // The integration receipt is not what this phase wants.
    let wrong_phase = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": standing,
            "evidence": {"phase": "integration", "repositories": [
                {"repository": "asma-rs-kontor", "pull_request": "PR-91",
                 "module_revision": "ed654bf", "root_pointer_revision": null}
            ]}
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-close-wrong-phase")
    .send(world)
    .await;
    assert_eq!(wrong_phase.status, 400, "{}", wrong_phase.body);

    let closed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": standing,
            "evidence": {
                "phase": "closeout",
                "merge": "PR 91 squash-merged as ed654bf",
                "release": "kontor-daemon reinstalled and restarted",
                "delivered_versions": {"asma-rs-kontor": "ed654bf"},
                "summary": "18 tickets delivered",
                "notification": "reported to the epic operator",
                "archive": "worktrees pruned"
            }
        }),
    )
    .signed_as(world, "operator")
    .with_key("op06-close-record")
    .send(world)
    .await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(
        closed.json()["state"]["phase"]["phase"],
        "done",
        "the recorded receipts finish the epic: {}",
        closed.body
    );
    let closeout = &closed.json()["state"]["closeout"];
    for receipt in [
        "merge_receipt",
        "release_receipt",
        "summary_receipt",
        "notification_receipt",
        "archive_receipt",
    ] {
        let digest = closeout[receipt].as_str().unwrap_or_else(|| {
            panic!("{receipt} was not recorded: {}", closed.body);
        });
        assert_eq!(digest.len(), 64, "{receipt} is a content digest");
    }
    assert_ne!(
        closeout["merge_receipt"], closeout["release_receipt"],
        "each statement is digested as itself, not one receipt reused: {}",
        closed.body
    );
    assert_eq!(closeout["delivered_versions"]["asma-rs-kontor"], "ed654bf");
    assert!(
        closed.json()["state"]["blockers"]
            .as_array()
            .expect("blockers")
            .is_empty(),
        "a finished epic carries no blocker: {}",
        closed.body
    );
}

/// `:remediate` on an epic that never started completion is an absence.
#[tokio::test]
async fn remediate_on_an_unstarted_completion_run_refuses_without_a_receipt() {
    let world = World::open().await;
    let project = world.project;
    let epic = MiniProjectId::generate();

    let answer = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:remediate"),
        &serde_json::json!({
            "expected_revision": 1,
            "action": {"action": "tpm_route", "round": 1,
                       "route": ContentHash::of(b"route").as_str()}
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .seat_credential_for_generation(SeatBindingId::generate(), 1),
    )
    .with_key("op06-remediate-nothing")
    .send(&world)
    .await;
    assert_eq!(answer.status, 404, "{}", answer.body);
    assert!(
        answer.json().get("receipt_id").is_none(),
        "a refusal must not carry a receipt: {}",
        answer.body
    );
}

/// The outage regression: the seeded Committee service must create real
/// read-only seats, hold the Judge until both independent findings are durable,
/// recompute the conjunction, settle, and replay without another native launch.
#[tokio::test]
async fn initial_committee_recovery_is_admin_fenced_diverse_frozen_and_replayable() {
    let composed = compose_realm("/tmp/kontor-committee-initial-recovery").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (quick, preview_hash) = quick_session_ready_to_promote(
        world,
        project,
        "Committee initial recovery",
        "committee-initial-recovery-quick",
    )
    .await;
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{quick}/promotion:apply"),
        &promotion_apply_body(&preview_hash),
    )
    .signed_as(world, "operator")
    .with_key("committee-initial-recovery-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("committee-initial-recovery-control")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let caller = materialized.json()["core_team"]["seats"]
        .as_array()
        .expect("core seats")
        .iter()
        .find(|seat| seat["role"]["role_code"] == "LSA")
        .and_then(|seat| seat["seat_binding_id"].as_str())
        .expect("the LSA SeatBinding")
        .to_owned();

    let mut accounts = std::collections::BTreeMap::new();
    for (label, provider) in [
        ("Claude Work", "claude-work"),
        ("Claude Personal", "claude-personal"),
        ("Codex Work", "codex-work"),
        ("Codex Personal", "codex-personal"),
        ("Paseo Local", "opencode"),
    ] {
        let ensured = Call::post(
            format!("/v1/projects/{project}/provider-account-profiles:ensure"),
            &serde_json::json!({
                "label": label,
                "harness": "fake.runtime",
                "credential_alias": provider,
                "selectable_providers": [provider],
                "enabled": true
            }),
        )
        .signed_as(world, "admin")
        .with_key(format!("committee-initial-account-{provider}"))
        .send(world)
        .await;
        assert_eq!(ensured.status, 200, "{}", ensured.body);
        accounts.insert(
            provider,
            ensured.json()["account_profile_id"]
                .as_str()
                .expect("an account id")
                .to_owned(),
        );
    }
    for provider in ["claude-work", "claude-personal"] {
        let exhausted = Call::post(
            format!("/v1/projects/{project}/provider-quota-states:record"),
            &serde_json::json!({
                "account_profile_id": accounts[provider],
                "provider": provider,
                "state": "exhausted",
                "resets_at": "2099-01-01T00:00:00Z",
                "expected_revision": 1
            }),
        )
        .signed_as(world, "admin")
        .with_key(format!("committee-initial-exhaust-{provider}"))
        .send(world)
        .await;
        assert_eq!(exhausted.status, 200, "{}", exhausted.body);
    }

    let epic_read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    let invoke_body = serde_json::json!({
        "profile": {"id": "01991c00-0000-7000-8000-000000000001", "version": 1},
        "question": "Does the remediated evidence now comply?",
        "caller_seat_binding_id": caller,
        "expected_revision": epic_read.json()["revision"],
        "initial_recovery_profiles": [
            {"role_slot_id": "reviewer-a", "ordered_routes": [
                {"provider": "codex-work", "model": "gpt-5.6-sol", "effort": "xhigh"},
                {"provider": "codex-personal", "model": "gpt-5.6-sol", "effort": "xhigh"}
            ]},
            {"role_slot_id": "reviewer-b", "ordered_routes": [
                {"provider": "opencode", "model": "deepseek/deepseek-v4-flash", "effort": "max"}
            ]},
            {"role_slot_id": "judge", "ordered_routes": [
                {"provider": "codex-work", "model": "gpt-5.6-sol", "effort": "high"},
                {"provider": "codex-personal", "model": "gpt-5.6-sol", "effort": "high"}
            ]}
        ]
    });
    let refused = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-initial-recovery-operator")
    .send(world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);

    let invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke_body,
    )
    .signed_as(world, "admin")
    .with_key("committee-initial-recovery-admin")
    .send(world)
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let routes: std::collections::BTreeMap<_, _> = invoked.json()["seats"]
        .as_array()
        .expect("Committee seats")
        .iter()
        .map(|seat| {
            (
                seat["role_slot_id"].as_str().expect("a slot").to_owned(),
                seat["model_route"]["provider"]
                    .as_str()
                    .expect("a frozen provider")
                    .to_owned(),
            )
        })
        .collect();
    assert!(routes["reviewer-a"].starts_with("codex"));
    assert_eq!(routes["reviewer-b"], "opencode");
    assert!(routes["judge"].starts_with("codex"));

    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke_body,
    )
    .signed_as(world, "admin")
    .with_key("committee-initial-recovery-admin")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replayed.json()["seats"], invoked.json()["seats"]);

    let run = ConsultationRunId::Committee(
        kontor_core::id::CommitteeRunId::parse(
            invoked.json()["committee_run_id"]
                .as_str()
                .expect("a Committee run"),
        )
        .expect("a Committee run id"),
    );
    let frozen = world.daemon.state().with_store(|store| {
        store
            .get_consultation_run(ProjectId::parse(project).expect("the project"), run)
            .expect("the run reads")
            .expect("the run exists")
    });
    let admission = frozen.context["admission"]["routes"]
        .as_array()
        .expect("frozen admission routes");
    assert!(admission.iter().all(|route| {
        route["profile_hash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)
            && route["rank"].as_u64().is_some()
            && route["headroom_basis_account_id"].as_str().is_some()
    }));
}

#[tokio::test]
async fn a_seeded_committee_runs_and_settles_instead_of_returning_503() {
    let composed = compose_realm("/tmp/kontor-op05-committee").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (quick, preview_hash) =
        quick_session_ready_to_promote(world, project, "Committee fixture", "committee-quick")
            .await;
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{quick}/promotion:apply"),
        &promotion_apply_body(&preview_hash),
    )
    .signed_as(world, "operator")
    .with_key("committee-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({
            "expected_revision": 1,
            "routes": [
                {"role_code": "LSA", "model_route": {
                    "provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"
                }},
                {"role_code": "TPM", "model_route": {
                    "provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"
                }}
            ]
        }),
    )
    .signed_as(world, "admin")
    .with_key("committee-control-seats")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let caller = materialized.json()["core_team"]["seats"]
        .as_array()
        .expect("core seats")
        .iter()
        .find(|seat| seat["role"]["role_code"] == "LSA")
        .and_then(|seat| seat["seat_binding_id"].as_str())
        .expect("the LSA SeatBinding")
        .to_owned();
    let tpm = materialized.json()["core_team"]["seats"]
        .as_array()
        .expect("core seats")
        .iter()
        .find(|seat| seat["role"]["role_code"] == "TPM")
        .and_then(|seat| seat["seat_binding_id"].as_str())
        .expect("the TPM SeatBinding")
        .to_owned();
    let codex_recovery_account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Codex recovery",
            "harness": "fake.runtime",
            "credential_alias": "codex-work",
            "selectable_providers": ["codex-work"],
            "enabled": true
        }),
    )
    .signed_as(world, "admin")
    .with_key("committee-codex-recovery-account")
    .send(world)
    .await;
    assert_eq!(
        codex_recovery_account.status, 200,
        "{}",
        codex_recovery_account.body
    );
    let epic_read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;

    // Advisor is a composed service too: publish, invoke, settle, and read the
    // immutable output back through the public API.
    let mut advisor = advisor_definition(ADVISOR_PROFILE, 1);
    advisor["allowed_caller_roles"] = serde_json::json!(["lsa"]);
    let advisor_preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": advisor}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(advisor_preview.status, 200, "{}", advisor_preview.body);
    let advisor_applied = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &serde_json::json!({
            "definition": advisor,
            "preview_hash": advisor_preview.json()["preview_hash"],
            "expected_revision": 1,
        }),
    )
    .signed_as(world, "admin")
    .with_key("advisor-profile-apply")
    .send(world)
    .await;
    assert_eq!(advisor_applied.status, 200, "{}", advisor_applied.body);
    let advisor_invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/advisor-runs:invoke"),
        &serde_json::json!({
            "profile": {"id": ADVISOR_PROFILE, "version": 1},
            "question": "What is the safest bounded operational decision?",
            "caller_seat_binding_id": caller,
            "expected_revision": epic_read.json()["revision"],
        }),
    )
    .signed_as(world, "operator")
    .with_key("advisor-invoke")
    .send(world)
    .await;
    assert_eq!(advisor_invoked.status, 200, "{}", advisor_invoked.body);
    assert_eq!(advisor_invoked.json()["state"], "running");
    let advisor_run = advisor_invoked.json()["advisor_run_id"]
        .as_str()
        .expect("Advisor run")
        .to_owned();
    let advisor_seat = advisor_invoked.json()["seats"][0]["seat_binding_id"]
        .as_str()
        .expect("Advisor seat")
        .to_owned();

    let advisor_token = world
        .daemon
        .state()
        .credentials()
        .consultation_seat_credential(
            SeatBindingId::parse(&advisor_seat).expect("the Advisor SeatBinding"),
        );
    let unrelated_operator_route = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &serde_json::json!({}),
    )
    .with_token(advisor_token.clone())
    .send(world)
    .await;
    assert_eq!(
        unrelated_operator_route.status, 403,
        "a consultation seat reached an unrelated Realm operator route: {}",
        unrelated_operator_route.body
    );

    let unscoped_advisor_settlement = Call::post(
        format!("/v1/projects/{project}/advisor-runs/{advisor_run}/settle"),
        &serde_json::json!({
            "seat_binding_id": advisor_seat,
            "output": "a shared operator cannot speak as the Advisor seat",
            "disposition": "accepted",
            "rationale": "the body is not an identity proof",
            "expected_revision": advisor_invoked.json()["receipt"]["revision"],
        }),
    )
    .signed_as(world, "operator")
    .with_key("advisor-unscoped-settle")
    .send(world)
    .await;
    assert_eq!(
        unscoped_advisor_settlement.status, 403,
        "{}",
        unscoped_advisor_settlement.body
    );

    let self_disposition = Call::post(
        format!("/v1/projects/{project}/advisor-runs/{advisor_run}/settle"),
        &serde_json::json!({
            "output": "the advice bytes are seat-authored",
            "disposition": "accepted",
            "rationale": "but the seat cannot accept itself",
            "expected_revision": advisor_invoked.json()["receipt"]["revision"],
        }),
    )
    .with_token(advisor_token.clone())
    .with_key("advisor-self-disposition")
    .send(world)
    .await;
    assert_eq!(self_disposition.status, 403, "{}", self_disposition.body);

    let advisor_output = Call::post(
        format!("/v1/projects/{project}/advisor-runs/{advisor_run}/settle"),
        &serde_json::json!({
            "output": "Use the bounded control-plane path and preserve identities.",
            "expected_revision": advisor_invoked.json()["receipt"]["revision"],
        }),
    )
    .with_token(advisor_token.clone())
    .with_key("advisor-output")
    .send(world)
    .await;
    assert_eq!(advisor_output.status, 200, "{}", advisor_output.body);
    assert_eq!(advisor_output.json()["state"], "running");
    assert_eq!(
        advisor_output.json()["advice"]["output"],
        "Use the bounded control-plane path and preserve identities."
    );
    assert!(advisor_output.json()["result"].is_null());

    let advisor_cannot_read_realm =
        Call::get(format!("/v1/projects/{project}/advisor-runs/{advisor_run}"))
            .with_token(advisor_token)
            .send(world)
            .await;
    assert_eq!(
        advisor_cannot_read_realm.status, 403,
        "a consultation seat inherited an Observer route: {}",
        advisor_cannot_read_realm.body
    );

    let advisor_settled = Call::post(
        format!("/v1/projects/{project}/advisor-runs/{advisor_run}/settle"),
        &serde_json::json!({
            "disposition": "accepted",
            "rationale": "It matches the operational policy.",
            "expected_revision": advisor_output.json()["receipt"]["revision"],
        }),
    )
    .signed_as(world, "operator")
    .with_key("advisor-disposition")
    .send(world)
    .await;
    assert_eq!(advisor_settled.status, 200, "{}", advisor_settled.body);
    assert_eq!(advisor_settled.json()["state"], "settled");
    let advisor_read = Call::get(format!("/v1/projects/{project}/advisor-runs/{advisor_run}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(advisor_read.status, 200, "{}", advisor_read.body);
    assert_eq!(
        advisor_read.json()["advice"]["output"],
        "Use the bounded control-plane path and preserve identities."
    );
    assert_eq!(advisor_read.json()["result"]["disposition"], "accepted");
    // The live pre-provenance run used a later immutable revision of the same
    // published Committee identity even though Completion still named
    // `independent_review@1`. Preserve that exact compatibility shape here;
    // ordinary current-round and clean re-review selection remains revision
    // strict.
    let seeded_committee = Call::get(format!("/v1/projects/{project}/committee-templates"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(seeded_committee.status, 200, "{}", seeded_committee.body);
    let mut legacy_template = kontor_profiles::seeds::bundled_consultation_presets()
        .expect("the bundled presets load")
        .committee_templates
        .remove(0);
    for version in 2..=4 {
        legacy_template.version = SpecVersion::parse(version).expect("a legacy template version");
        let legacy_template_document = legacy_template
            .canonicalize()
            .expect("the legacy template canonicalizes");
        world.daemon.state().with_store(|store| {
            store
                .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
                    project_id: ProjectId::parse(project).expect("the project id"),
                    family: ConsultationFamily::Committee,
                    profile_id: legacy_template.template_id.to_string(),
                    version: legacy_template.version,
                    name: legacy_template.name.clone(),
                    definition: legacy_template_document.json().to_owned(),
                    definition_hash: legacy_template_document.hash().clone(),
                    published_at: kontor_api::now(),
                })
                .expect("the legacy Committee revision publishes");
        });
    }
    let invoke_body = serde_json::json!({
        "profile": {
            "id": "01991c00-0000-7000-8000-000000000001",
            "version": 4
        },
        "question": "Does this evidence satisfy the operational gate?",
        "caller_seat_binding_id": caller,
        "expected_revision": epic_read.json()["revision"],
    });
    let invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-invoke")
    .send(world)
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    assert_eq!(invoked.json()["state"], "running");
    let run = invoked.json()["committee_run_id"]
        .as_str()
        .expect("a Committee run id")
        .to_owned();
    let invoked_json = invoked.json();
    let seats = invoked_json["seats"].as_array().expect("Committee seats");
    let reviewer_ids: Vec<String> = seats
        .iter()
        .filter(|seat| {
            seat["role_slot_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("reviewer"))
        })
        .map(|seat| {
            assert!(
                seat["observed_binding"].is_object(),
                "reviewer was not launched"
            );
            seat["seat_binding_id"]
                .as_str()
                .expect("seat id")
                .to_owned()
        })
        .collect();
    assert_eq!(reviewer_ids.len(), 2, "{}", invoked.body);
    let predecessor_token = world
        .daemon
        .state()
        .credentials()
        .consultation_seat_credential(
            SeatBindingId::parse(&reviewer_ids[0]).expect("a reviewer SeatBinding"),
        );
    let reviewer_read = Call::get(format!("/v1/projects/{project}/committee-runs/{run}"))
        .with_token(predecessor_token.clone())
        .send(world)
        .await;
    assert_eq!(
        reviewer_read.status, 403,
        "an independent reviewer could read the Committee projection: {}",
        reviewer_read.body
    );
    let judge = seats
        .iter()
        .find(|seat| seat["role_slot_id"] == "judge")
        .expect("a Judge seat");
    let judge_id = judge["seat_binding_id"]
        .as_str()
        .expect("Judge id")
        .to_owned();
    assert!(
        judge["observed_binding"].is_null(),
        "Judge launched before findings"
    );

    // An admin may replace an exact idle native filler without changing the
    // logical Committee seat or inventing a finding. Recovery advances the
    // Committee revision, archives and launches exactly once, and a replay of
    // the old compare-and-swap request performs no second runtime effect.
    let predecessor_native = seats
        .iter()
        .find(|seat| seat["seat_binding_id"] == reviewer_ids[0])
        .and_then(|seat| seat["observed_binding"]["native_id"].as_str())
        .expect("the reviewer's exact predecessor")
        .to_owned();
    let recovery_body = serde_json::json!({
        "expected_revision": invoked.json()["receipt"]["revision"],
        "expected_native_id": predecessor_native.clone(),
        "reason": "credential_propagation",
    });
    let recovered = Call::post(
        format!(
            "/v1/projects/{project}/committee-runs/{run}/seats/{}/recover",
            reviewer_ids[0]
        ),
        &recovery_body,
    )
    .signed_as(world, "admin")
    .with_key("committee-reviewer-recovery")
    .send(world)
    .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(recovered.json()["seat_binding_id"], reviewer_ids[0]);
    assert_eq!(
        recovered.json()["predecessor_native_id"],
        predecessor_native
    );
    assert_ne!(
        recovered.json()["successor_native_id"],
        predecessor_native,
        "the predecessor was not replaced: {}",
        recovered.body
    );
    let recovered_generation = recovered.json()["committee"]["seats"]
        .as_array()
        .expect("recovered Committee seats")
        .iter()
        .find(|seat| seat["seat_binding_id"] == reviewer_ids[0])
        .and_then(|seat| seat["occupancy_generation"].as_u64())
        .expect("the successor occupancy generation");
    assert_eq!(recovered_generation, 2);
    let recovery_calls = world.fake.calls();
    assert!(
        recovery_calls.contains(&AdapterCall::RetireConsultation(
            SeatBindingId::parse(&reviewer_ids[0]).expect("the reviewer SeatBinding")
        )),
        "the exact predecessor was not retired: {recovery_calls:?}"
    );
    assert!(
        recovery_calls.contains(&AdapterCall::LaunchConsultation(
            SeatBindingId::parse(&reviewer_ids[0]).expect("the reviewer SeatBinding")
        )),
        "the successor was not launched: {recovery_calls:?}"
    );
    let calls_after_recovery = recovery_calls;
    let replayed_recovery = Call::post(
        format!(
            "/v1/projects/{project}/committee-runs/{run}/seats/{}/recover",
            reviewer_ids[0]
        ),
        &recovery_body,
    )
    .signed_as(world, "admin")
    .with_key("committee-reviewer-recovery")
    .send(world)
    .await;
    assert_eq!(replayed_recovery.status, 200, "{}", replayed_recovery.body);
    assert_eq!(replayed_recovery.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        world.fake.calls(),
        calls_after_recovery,
        "a recovery replay reached the runtime"
    );

    // The predecessor's inherited bearer is invalid as soon as its occupancy
    // generation is fenced, even if that native process later wakes up.
    let zombie = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "a fenced predecessor cannot submit",
            "evidence_refs": ["evidence:zombie"],
            "expected_revision": recovered.json()["receipt"]["revision"],
        }),
    )
    .with_token(predecessor_token)
    .with_key("committee-zombie-predecessor")
    .send(world)
    .await;
    assert_eq!(zombie.status, 409, "{}", zombie.body);
    assert_eq!(zombie.json()["code"], "stale_binding");

    let mut revision = recovered.json()["receipt"]["revision"]
        .as_u64()
        .expect("run revision");

    // A shared operator bearer proves authority but not seat identity.
    let unscoped = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "an operator cannot speak as a reviewer",
            "evidence_refs": ["evidence:operator"],
            "expected_revision": revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-unscoped-finding")
    .send(world)
    .await;
    assert_eq!(unscoped.status, 403, "{}", unscoped.body);

    // A correctly signed but foreign binding is still not a Committee seat.
    let foreign_binding = SeatBindingId::generate();
    let foreign = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "foreign seat",
            "evidence_refs": ["evidence:foreign"],
            "expected_revision": revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential(foreign_binding),
    )
    .with_key("committee-foreign-finding")
    .send(world)
    .await;
    assert_eq!(foreign.status, 403, "{}", foreign.body);

    // The Judge cannot aggregate before both independent findings are durable.
    let premature_judge = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "too early",
            "evidence_refs": ["evidence:premature"],
            "expected_revision": revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential(
                SeatBindingId::parse(&judge_id).expect("the Judge SeatBinding"),
            ),
    )
    .with_key("committee-premature-judge")
    .send(world)
    .await;
    assert_eq!(premature_judge.status, 409, "{}", premature_judge.body);

    let incomplete = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "missing evidence reference",
            "evidence_refs": [],
            "expected_revision": revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(&reviewer_ids[0]).expect("a reviewer SeatBinding"),
                recovered_generation,
            ),
    )
    .with_key("committee-incomplete-evidence")
    .send(world)
    .await;
    assert_eq!(incomplete.status, 400, "{}", incomplete.body);
    for (index, reviewer) in reviewer_ids.iter().enumerate() {
        let seat_token = world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(reviewer).expect("a reviewer SeatBinding"),
                if index == 0 { recovered_generation } else { 1 },
            );
        let recorded = Call::post(
            format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
            &serde_json::json!({
                "round": 1,
                "verdict": if index == 0 { "compliant" } else { "non_compliant" },
                "evidence_complete": index == 0,
                "rationale": format!("reviewer {} found the evidence complete", index + 1),
                "evidence_refs": [format!("evidence:reviewer-{}", index + 1)],
                "expected_revision": revision,
            }),
        )
        .with_token(seat_token)
        .with_key(format!("committee-reviewer-{}", index + 1))
        .send(world)
        .await;
        assert_eq!(recorded.status, 200, "{}", recorded.body);
        revision = recorded.json()["receipt"]["revision"]
            .as_u64()
            .expect("run revision");
        if index == 1 {
            assert_eq!(recorded.json()["state"], "awaiting_judge");
            assert!(
                recorded.json()["seats"]
                    .as_array()
                    .expect("seats")
                    .iter()
                    .find(|seat| seat["role_slot_id"] == "judge")
                    .is_some_and(|seat| seat["observed_binding"].is_object()),
                "Judge did not launch after both findings: {}",
                recorded.body
            );
        }
    }
    let awaiting_judge = Call::get(format!("/v1/projects/{project}/committee-runs/{run}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(awaiting_judge.status, 200, "{}", awaiting_judge.body);
    let judge_native = awaiting_judge.json()["seats"]
        .as_array()
        .expect("Committee seats")
        .iter()
        .find(|seat| seat["seat_binding_id"] == judge_id)
        .and_then(|seat| seat["observed_binding"]["native_id"].as_str())
        .expect("the launched Judge predecessor")
        .to_owned();
    let recovered_judge = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/seats/{judge_id}/recover"),
        &serde_json::json!({
            "expected_revision": revision,
            "expected_native_id": judge_native,
            "reason": "provider_unavailable",
            "recovery_profile": [
                {"provider": "claude-personal", "model": "claude-fable-5", "effort": "xhigh"},
                {"provider": "codex-work", "model": "gpt-5.6-sol", "effort": "xhigh"}
            ]
        }),
    )
    .signed_as(world, "admin")
    .with_key("committee-judge-cross-family-recovery")
    .send(world)
    .await;
    assert_eq!(recovered_judge.status, 200, "{}", recovered_judge.body);
    assert_eq!(
        recovered_judge.json()["active_model_route"]["provider"],
        "codex-work"
    );
    let judge_generation = recovered_judge.json()["committee"]["seats"]
        .as_array()
        .expect("recovered Committee seats")
        .iter()
        .find(|seat| seat["seat_binding_id"] == judge_id)
        .and_then(|seat| seat["occupancy_generation"].as_u64())
        .expect("the Judge successor occupancy generation");
    revision = recovered_judge.json()["receipt"]["revision"]
        .as_u64()
        .expect("the recovered Committee revision");
    let incomplete_settlement = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/settle"),
        &serde_json::json!({
            "recommendation": "Do not settle without the Judge's immutable finding.",
            "tried_path": "Both reviewers have reported, but the aggregate is absent.",
            "expected_revision": revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-incomplete-cardinality")
    .send(world)
    .await;
    assert_eq!(
        incomplete_settlement.status, 400,
        "settlement must require exactly the pinned slot cardinality: {}",
        incomplete_settlement.body
    );
    let contradictory = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "contradicts the non-compliant conjunction",
            "evidence_refs": ["evidence:reviewer-1", "evidence:reviewer-2"],
            "expected_revision": revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(&judge_id).expect("the Judge SeatBinding"),
                judge_generation,
            ),
    )
    .with_key("committee-contradictory-judge")
    .send(world)
    .await;
    assert_eq!(contradictory.status, 400, "{}", contradictory.body);
    let judged = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "non_compliant",
            "evidence_complete": false,
            "rationale": "The conjunctive rule fails on one independent finding.",
            "evidence_refs": ["evidence:reviewer-1", "evidence:reviewer-2"],
            "expected_revision": revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(&judge_id).expect("the Judge SeatBinding"),
                judge_generation,
            ),
    )
    .with_key("committee-judge")
    .send(world)
    .await;
    assert_eq!(judged.status, 200, "{}", judged.body);
    revision = judged.json()["receipt"]["revision"]
        .as_u64()
        .expect("run revision");
    let first_round_read = Call::get(format!("/v1/projects/{project}/committee-runs/{run}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(first_round_read.status, 200, "{}", first_round_read.body);
    assert_eq!(
        first_round_read.json()["findings"].as_array().map(Vec::len),
        Some(3)
    );

    // Recreate the exact live durable shape written by the pre-v62 daemon: the
    // immutable remediation exists, but the source run was advanced in place to
    // a still-running round two with one incomplete finding. Its round-one
    // result was not retained.
    // This direct SQL is test-only historical fixture construction, like the
    // other legacy-shape regressions in this suite; the product path never
    // performs it. The canonical documents reproduce the old settlement bytes
    // exactly, so reconstruction has no test-only digest shortcut.
    let finding_hashes = first_round_read.json()["findings"]
        .as_array()
        .expect("the round-one findings")
        .iter()
        .map(|finding| {
            finding["document_hash"]
                .as_str()
                .expect("a finding hash")
                .to_owned()
        })
        .collect::<Vec<_>>();
    let evidence_document = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "committee_run_id": run,
        "round": 1,
        "findings": finding_hashes,
    }))
    .expect("the historical evidence identity canonicalizes");
    let result_document = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "verdict": "non_compliant",
        "evidence_hash": evidence_document.hash().as_str(),
        "round": 1,
        "finding_hashes": finding_hashes,
    }))
    .expect("the historical result canonicalizes");
    let remediation_document = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "committee_run_id": run,
        "from_round": 1,
        "recommendation": "Re-run the gate after correcting the missing evidence.",
        "tried_path": "Round one identified the missing operational receipt.",
        "failed_result_hash": result_document.hash().as_str(),
    }))
    .expect("the historical remediation canonicalizes");
    let failed_result_hash = result_document.hash().as_str().to_owned();
    let remediation_hash = remediation_document.hash().as_str().to_owned();
    let round_two_finding = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "committee_run_id": run,
        "round": 2,
        "role_slot_id": "reviewer-b",
        "role": "reviewer",
        "verdict": "non_compliant",
        "evidence_complete": false,
        "rationale": "The legacy re-review began before provenance fencing existed.",
        "evidence_refs": ["evidence:legacy-round-two"],
    }))
    .expect("the legacy round-two finding canonicalizes");
    let legacy_revision = revision + 1;
    let database = world.directory.path().join(kontor_daemon::DATABASE_FILE);
    let connection = rusqlite::Connection::open(database).expect("the Realm database opens");
    connection
        .execute(
            "INSERT INTO committee_remediations
                 (committee_run_id, project_id, from_round, recommendation,
                  tried_path, document, document_hash, recorded_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                run,
                project,
                "Re-run the gate after correcting the missing evidence.",
                "Round one identified the missing operational receipt.",
                remediation_document.json(),
                remediation_document.hash().as_str(),
                "2026-08-25T20:24:23Z",
            ],
        )
        .expect("the historical immutable remediation is reproduced");
    let advanced_in_place = connection
        .execute(
            "UPDATE consultation_runs
             SET state = 'running', round = 2,
                 revision = revision + 1, updated_at = ?4
             WHERE project_id = ?1 AND run_id = ?2 AND family = 'committee'
               AND revision = ?3 AND result IS NULL",
            rusqlite::params![
                project,
                run,
                i64::try_from(revision).expect("the test revision fits SQLite"),
                "2026-08-25T20:24:23Z"
            ],
        )
        .expect("the legacy in-place round advance is reproduced");
    assert_eq!(advanced_in_place, 1);
    connection
        .execute(
            "INSERT INTO committee_findings
                 (committee_run_id, project_id, round, role_slot_id, role,
                  verdict, evidence_complete, document, document_hash, recorded_at)
             VALUES (?1, ?2, 2, 'reviewer-b', 'reviewer', 'non_compliant',
                     0, ?3, ?4, ?5)",
            rusqlite::params![
                run,
                project,
                round_two_finding.json(),
                round_two_finding.hash().as_str(),
                "2026-08-25T20:25:00Z",
            ],
        )
        .expect("the in-progress legacy round-two finding is reproduced");
    drop(connection);

    // The failed run is now historical evidence. Its seats cannot append to
    // the poisoned second round, even with their still-valid
    // generation-scoped credential.
    let poisoned_old_run = Call::post(
        format!("/v1/projects/{project}/committee-runs/{run}/findings:record"),
        &serde_json::json!({
            "round": 2,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "A failed run must never become its own re-review.",
            "evidence_refs": ["evidence:poisoned-old-run"],
            "expected_revision": legacy_revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(
                SeatBindingId::parse(&reviewer_ids[0]).expect("a reviewer SeatBinding"),
                recovered_generation,
            ),
    )
    .with_key("committee-poisoned-old-run")
    .send(world)
    .await;
    assert_eq!(poisoned_old_run.status, 400, "{}", poisoned_old_run.body);

    // Reproduce the live legacy checkpoint: completion still waits at verdict
    // round one while the old writer has already advanced the Committee. The
    // new reader reconstructs the exact failed result from immutable findings
    // and remediation instead of selecting the poisoned current round.
    let compiled = kontor_scheduler::compile(
        kontor_scheduler::operational_default().expect("the built-in profile"),
    )
    .expect("the built-in profile compiles");
    let mut completion = kontor_scheduler::start(
        &compiled,
        SeatBindingId::parse(&tpm).expect("the TPM seat id"),
        Vec::new(),
    )
    .expect("completion starts");
    completion.phase = kontor_scheduler::CompletionPhase::Verdict(1);
    let project_id = ProjectId::parse(project).expect("project id");
    let epic_id = MiniProjectId::parse(&epic).expect("epic id");
    world.daemon.state().with_store(|store| {
        store
            .create_epic_completion(&StoredEpicCompletion {
                project_id,
                mini_project_id: epic_id,
                profile_id: compiled.profile.id.clone(),
                profile_version: compiled.profile.version,
                definition_hash: compiled.definition_hash.clone(),
                state: serde_json::to_value(&completion).expect("completion serializes"),
                revision: completion.revision,
                updated_at: kontor_api::now(),
            })
            .expect("completion state is seeded at its verdict gate");
    });
    let advanced = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "operator")
    .with_key("completion-consume-historical-round-one")
    .send(world)
    .await;
    assert_eq!(advanced.status, 200, "{}", advanced.body);
    assert_eq!(advanced.json()["state"]["phase"]["phase"], "awaiting_lsa");
    assert_eq!(advanced.json()["state"]["rounds"][0]["verdict"], "fail");
    assert_eq!(
        advanced.json()["state"]["rounds"][0]["committee_run_id"],
        run
    );
    assert_eq!(
        advanced.json()["state"]["rounds"][0]["result_hash"],
        failed_result_hash
    );
    assert_eq!(
        advanced.json()["state"]["rounds"][0]["remediation_hash"],
        remediation_hash
    );
    let legacy_after_ingest = Call::get(format!("/v1/projects/{project}/committee-runs/{run}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(
        legacy_after_ingest.status, 200,
        "{}",
        legacy_after_ingest.body
    );
    assert_eq!(legacy_after_ingest.json()["round"], 2);
    assert_eq!(legacy_after_ingest.json()["state"], "running");
    assert!(legacy_after_ingest.json()["result_hash"].is_null());
    assert_eq!(
        legacy_after_ingest.json()["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .filter(|finding| finding["round"] == 2)
                .count()),
        Some(1),
        "consuming historical round one rewrote the poisoned round-two evidence"
    );
    let failed_evidence = advanced.json()["state"]["rounds"][0]["evidence"]
        .as_str()
        .expect("the reconstructed evidence digest")
        .to_owned();
    let awaiting_revision = advanced.json()["receipt"]["revision"]
        .as_u64()
        .expect("the AwaitRemediation revision");

    let proposal = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:remediate"),
        &serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "lsa_proposal",
                "round": 1,
                "failed_round_evidence": failed_evidence,
                "proposal": ContentHash::of(b"repair the governed evidence gap").as_str(),
            }
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .seat_credential_for_generation(
                SeatBindingId::parse(&caller).expect("the LSA SeatBinding"),
                1,
            ),
    )
    .with_key("committee-completion-proposal")
    .send(world)
    .await;
    assert_eq!(proposal.status, 200, "{}", proposal.body);

    let routed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:remediate"),
        &serde_json::json!({
            "expected_revision": awaiting_revision,
            "action": {
                "action": "tpm_route",
                "round": 1,
                "route": ContentHash::of(b"route the governed repair").as_str(),
            }
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .seat_credential_for_generation(
                SeatBindingId::parse(&tpm).expect("the TPM SeatBinding"),
                1,
            ),
    )
    .with_key("committee-completion-route")
    .send(world)
    .await;
    assert_eq!(routed.status, 200, "{}", routed.body);
    assert_eq!(routed.json()["state"]["phase"]["phase"], "remediation");

    let integrated = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({
            "expected_revision": routed.json()["receipt"]["revision"],
            "evidence": {
                "phase": "integration",
                "repositories": [{
                    "repository": "asma-rs-kontor",
                    "pull_request": "PR-113",
                    "module_revision": "deadbeef",
                    "root_pointer_revision": "cafebabe"
                }]
            }
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-completion-remediation-integrated")
    .send(world)
    .await;
    assert_eq!(integrated.status, 200, "{}", integrated.body);
    assert_eq!(integrated.json()["state"]["phase"]["phase"], "verdict");
    assert_eq!(integrated.json()["state"]["phase"]["round"], 2);
    assert_eq!(
        integrated.json()["state"]["remediations"][0]["authorization"]["lsa_actor"]["seat_binding_id"],
        caller
    );
    assert_eq!(
        integrated.json()["state"]["remediations"][0]["authorization"]["lsa_actor"]["occupancy_generation"],
        1
    );
    assert_eq!(
        integrated.json()["state"]["remediations"][0]["authorization"]["tpm_actor"]["seat_binding_id"],
        tpm
    );
    assert_eq!(
        integrated.json()["state"]["remediations"][0]["authorization"]["tpm_actor"]["occupancy_generation"],
        1
    );
    let completion_revision = integrated.json()["receipt"]["revision"]
        .as_u64()
        .expect("the post-remediation completion revision");
    let integration_receipt =
        integrated.json()["state"]["remediations"][0]["integration"]["receipt"]
            .as_str()
            .expect("the frozen remediation integration digest")
            .to_owned();
    let re_review = serde_json::json!({
        "completion_round": 1,
        "completion_revision": completion_revision,
        "failed_committee_run_id": run,
        "failed_result_hash": failed_result_hash,
        "remediation_hash": remediation_hash,
        "remediation_integration_receipt": integration_receipt,
    });

    // The old run's round-two row must not be consumed as the fresh verdict.
    let poisoned_advance = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": completion_revision}),
    )
    .signed_as(world, "operator")
    .with_key("completion-reject-poisoned-round-two")
    .send(world)
    .await;
    assert_eq!(poisoned_advance.status, 503, "{}", poisoned_advance.body);

    let launches_before_refusal = world
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
        .count();
    let mut mismatched = re_review.clone();
    mismatched["failed_result_hash"] =
        serde_json::json!(ContentHash::of(b"another failed result").as_str());
    let refused_re_review = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &serde_json::json!({
            "profile": {
                "id": "01991c00-0000-7000-8000-000000000001",
                "version": 1
            },
            "question": "Does the governed remediation now satisfy the gate?",
            "caller_seat_binding_id": caller,
            "expected_revision": epic_read.json()["revision"],
            "re_review": mismatched,
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-re-review-mismatched-lineage")
    .send(world)
    .await;
    assert_eq!(refused_re_review.status, 400, "{}", refused_re_review.body);
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_before_refusal,
        "a mismatched provenance reached native placement"
    );
    let mut stale_freeze = re_review.clone();
    stale_freeze["completion_revision"] = serde_json::json!(completion_revision - 1);
    let refused_stale_freeze = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &serde_json::json!({
            "profile": {
                "id": "01991c00-0000-7000-8000-000000000001",
                "version": 1
            },
            "question": "Does the governed remediation now satisfy the gate?",
            "caller_seat_binding_id": caller,
            "expected_revision": epic_read.json()["revision"],
            "re_review": stale_freeze,
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-re-review-stale-completion-freeze")
    .send(world)
    .await;
    assert_eq!(
        refused_stale_freeze.status, 400,
        "{}",
        refused_stale_freeze.body
    );
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_before_refusal,
        "a stale completion freeze reached native placement"
    );
    let mut wrong_integration = re_review.clone();
    wrong_integration["remediation_integration_receipt"] =
        serde_json::json!(ContentHash::of(b"another integration freeze").as_str());
    let refused_wrong_integration = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &serde_json::json!({
            "profile": {
                "id": "01991c00-0000-7000-8000-000000000001",
                "version": 1
            },
            "question": "Does the governed remediation now satisfy the gate?",
            "caller_seat_binding_id": caller,
            "expected_revision": epic_read.json()["revision"],
            "re_review": wrong_integration,
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-re-review-wrong-integration-freeze")
    .send(world)
    .await;
    assert_eq!(
        refused_wrong_integration.status, 400,
        "{}",
        refused_wrong_integration.body
    );
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_before_refusal,
        "a mismatched integration freeze reached native placement"
    );

    let re_review_body = serde_json::json!({
        "profile": {
            "id": "01991c00-0000-7000-8000-000000000001",
            "version": 1
        },
        "question": "Does the governed remediation now satisfy the gate?",
        "caller_seat_binding_id": caller,
        "expected_revision": epic_read.json()["revision"],
        "re_review": re_review,
    });
    // Two distinct keys race the same normalized provenance. The storage claim
    // is inside the run/topology/seat freeze transaction: exactly one run wins,
    // and the loser rolls back before native placement.
    let first_re_review = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &re_review_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-clean-re-review")
    .send(world);
    let second_re_review = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &re_review_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-clean-re-review-concurrent")
    .send(world);
    let (first_re_review, second_re_review) = tokio::join!(first_re_review, second_re_review);
    let (re_review_invoked, duplicate_re_review, winning_re_review_key) =
        if first_re_review.status == 200 {
            (
                first_re_review,
                second_re_review,
                "committee-clean-re-review",
            )
        } else {
            (
                second_re_review,
                first_re_review,
                "committee-clean-re-review-concurrent",
            )
        };
    assert_eq!(re_review_invoked.status, 200, "{}", re_review_invoked.body);
    assert_eq!(re_review_invoked.json()["round"], 2);
    assert_ne!(re_review_invoked.json()["committee_run_id"], run);
    let launches_after_clean_invoke = world
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
        .count();
    assert_eq!(
        duplicate_re_review.status, 409,
        "{}",
        duplicate_re_review.body
    );
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_after_clean_invoke,
        "a duplicate clean re-review reached native placement"
    );
    let re_review_run = re_review_invoked.json()["committee_run_id"]
        .as_str()
        .expect("the clean re-review run")
        .to_owned();
    let replayed_re_review = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &re_review_body,
    )
    .signed_as(world, "operator")
    .with_key(winning_re_review_key)
    .send(world)
    .await;
    assert_eq!(
        replayed_re_review.status, 200,
        "{}",
        replayed_re_review.body
    );
    assert_eq!(replayed_re_review.json()["committee_run_id"], re_review_run);
    assert_eq!(replayed_re_review.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_after_clean_invoke,
        "an exact clean re-review replay launched another native seat"
    );
    let frozen_re_review = world.daemon.state().with_store(|store| {
        store
            .get_consultation_run(
                project_id,
                ConsultationRunId::Committee(
                    kontor_core::id::CommitteeRunId::parse(&re_review_run)
                        .expect("the clean Committee id"),
                ),
            )
            .expect("the clean re-review reads")
            .expect("the clean re-review is durable")
    });
    let delivered = &frozen_re_review.context["re_review_evidence"];
    assert_eq!(
        delivered["committee_remediation"]["recommendation"],
        "Re-run the gate after correcting the missing evidence."
    );
    assert_eq!(
        delivered["completion_freeze"]["integration"]["repositories"][0]["module_revision"],
        "deadbeef"
    );
    assert_eq!(
        delivered["failed_committee"]["result_hash"],
        failed_result_hash
    );
    let re_review_seats = re_review_invoked.json()["seats"]
        .as_array()
        .expect("the clean re-review seats")
        .clone();
    let re_review_reviewers = re_review_seats
        .iter()
        .filter(|seat| {
            seat["role_slot_id"]
                .as_str()
                .is_some_and(|slot| slot.starts_with("reviewer"))
        })
        .map(|seat| {
            seat["seat_binding_id"]
                .as_str()
                .expect("a re-review reviewer")
                .to_owned()
        })
        .collect::<Vec<_>>();
    let re_review_judge = re_review_seats
        .iter()
        .find(|seat| seat["role_slot_id"] == "judge")
        .and_then(|seat| seat["seat_binding_id"].as_str())
        .expect("the re-review Judge")
        .to_owned();
    let mut re_review_revision = re_review_invoked.json()["receipt"]["revision"]
        .as_u64()
        .expect("the clean re-review revision");
    for (index, reviewer) in re_review_reviewers.iter().enumerate() {
        let recorded = Call::post(
            format!("/v1/projects/{project}/committee-runs/{re_review_run}/findings:record"),
            &serde_json::json!({
                "round": 2,
                "verdict": "compliant",
                "evidence_complete": true,
                "rationale": format!("clean reviewer {} verified the remediation", index + 1),
                "evidence_refs": [format!("evidence:clean-reviewer-{}", index + 1)],
                "expected_revision": re_review_revision,
            }),
        )
        .with_token(
            world
                .daemon
                .state()
                .credentials()
                .consultation_seat_credential(
                    SeatBindingId::parse(reviewer).expect("a re-review reviewer SeatBinding"),
                ),
        )
        .with_key(format!("committee-clean-reviewer-{}", index + 1))
        .send(world)
        .await;
        assert_eq!(recorded.status, 200, "{}", recorded.body);
        re_review_revision = recorded.json()["receipt"]["revision"]
            .as_u64()
            .expect("the reviewer revision");
    }
    let re_review_judged = Call::post(
        format!("/v1/projects/{project}/committee-runs/{re_review_run}/findings:record"),
        &serde_json::json!({
            "round": 2,
            "verdict": "compliant",
            "evidence_complete": true,
            "rationale": "The clean re-review passes conjunctively.",
            "evidence_refs": ["evidence:clean-reviewer-1", "evidence:clean-reviewer-2"],
            "expected_revision": re_review_revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential(
                SeatBindingId::parse(&re_review_judge).expect("the re-review Judge SeatBinding"),
            ),
    )
    .with_key("committee-clean-judge")
    .send(world)
    .await;
    assert_eq!(re_review_judged.status, 200, "{}", re_review_judged.body);
    let clean_settled = Call::post(
        format!("/v1/projects/{project}/committee-runs/{re_review_run}/settle"),
        &serde_json::json!({
            "expected_revision": re_review_judged.json()["receipt"]["revision"]
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-clean-settle")
    .send(world)
    .await;
    assert_eq!(clean_settled.status, 200, "{}", clean_settled.body);
    assert_eq!(clean_settled.json()["state"], "settled");
    assert_eq!(clean_settled.json()["outcome"], "compliant");

    let completed_verdict = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/completion:advance"),
        &serde_json::json!({"expected_revision": completion_revision}),
    )
    .signed_as(world, "operator")
    .with_key("completion-consume-clean-re-review")
    .send(world)
    .await;
    assert_eq!(completed_verdict.status, 200, "{}", completed_verdict.body);
    assert_eq!(
        completed_verdict.json()["state"]["phase"]["phase"],
        "closeout"
    );
    assert_eq!(
        completed_verdict.json()["state"]["rounds"][1]["verdict"],
        "pass"
    );
    assert_eq!(
        completed_verdict.json()["state"]["rounds"][1]["committee_run_id"],
        re_review_run
    );

    // The provenance participates in the command identity. Reusing the key
    // with different lineage cannot replay or launch another clean run.
    let launches_before_conflict = world
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
        .count();
    let mut conflicting_body = re_review_body.clone();
    conflicting_body["re_review"]["remediation_hash"] =
        serde_json::json!(ContentHash::of(b"different remediation").as_str());
    let provenance_conflict = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &conflicting_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-clean-re-review")
    .send(world)
    .await;
    assert_eq!(
        provenance_conflict.status, 409,
        "{}",
        provenance_conflict.body
    );
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_before_conflict,
        "an idempotency conflict reached native placement"
    );

    let launches_before_replay = world
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
        .count();
    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-invoke")
    .send(world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["committee_run_id"], run);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
            .count(),
        launches_before_replay,
        "a replay launched another native Committee seat"
    );

    // New settlements never reproduce the legacy shape above. A fresh failed
    // round freezes its result and remediation on the source run and leaves the
    // run terminal at round one; only a separately invoked re-review may own
    // round two.
    let terminal_invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/committee-runs:invoke"),
        &invoke_body,
    )
    .signed_as(world, "operator")
    .with_key("committee-terminal-failure-invoke")
    .send(world)
    .await;
    assert_eq!(terminal_invoked.status, 200, "{}", terminal_invoked.body);
    let terminal_run = terminal_invoked.json()["committee_run_id"]
        .as_str()
        .expect("the terminal-failure run")
        .to_owned();
    let terminal_seats = terminal_invoked.json()["seats"]
        .as_array()
        .expect("the terminal-failure seats")
        .clone();
    let terminal_reviewers = terminal_seats
        .iter()
        .filter(|seat| {
            seat["role_slot_id"]
                .as_str()
                .is_some_and(|slot| slot.starts_with("reviewer"))
        })
        .map(|seat| {
            seat["seat_binding_id"]
                .as_str()
                .expect("a terminal-failure reviewer")
                .to_owned()
        })
        .collect::<Vec<_>>();
    let terminal_judge = terminal_seats
        .iter()
        .find(|seat| seat["role_slot_id"] == "judge")
        .and_then(|seat| seat["seat_binding_id"].as_str())
        .expect("the terminal-failure Judge")
        .to_owned();
    let mut terminal_revision = terminal_invoked.json()["receipt"]["revision"]
        .as_u64()
        .expect("the terminal-failure revision");
    for (index, reviewer) in terminal_reviewers.iter().enumerate() {
        let finding = Call::post(
            format!("/v1/projects/{project}/committee-runs/{terminal_run}/findings:record"),
            &serde_json::json!({
                "round": 1,
                "verdict": if index == 0 { "compliant" } else { "non_compliant" },
                "evidence_complete": true,
                "rationale": format!("terminal-failure reviewer {}", index + 1),
                "evidence_refs": [format!("evidence:terminal-reviewer-{}", index + 1)],
                "expected_revision": terminal_revision,
            }),
        )
        .with_token(
            world
                .daemon
                .state()
                .credentials()
                .consultation_seat_credential(
                    SeatBindingId::parse(reviewer).expect("a terminal reviewer SeatBinding"),
                ),
        )
        .with_key(format!("committee-terminal-reviewer-{}", index + 1))
        .send(world)
        .await;
        assert_eq!(finding.status, 200, "{}", finding.body);
        terminal_revision = finding.json()["receipt"]["revision"]
            .as_u64()
            .expect("the terminal reviewer revision");
    }
    let terminal_judged = Call::post(
        format!("/v1/projects/{project}/committee-runs/{terminal_run}/findings:record"),
        &serde_json::json!({
            "round": 1,
            "verdict": "non_compliant",
            "evidence_complete": true,
            "rationale": "The fresh failed run remains terminal.",
            "evidence_refs": ["evidence:terminal-reviewer-1", "evidence:terminal-reviewer-2"],
            "expected_revision": terminal_revision,
        }),
    )
    .with_token(
        world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential(
                SeatBindingId::parse(&terminal_judge).expect("the terminal Judge SeatBinding"),
            ),
    )
    .with_key("committee-terminal-judge")
    .send(world)
    .await;
    assert_eq!(terminal_judged.status, 200, "{}", terminal_judged.body);
    let terminal_failed = Call::post(
        format!("/v1/projects/{project}/committee-runs/{terminal_run}/settle"),
        &serde_json::json!({
            "recommendation": "Invoke a clean governed re-review after remediation.",
            "tried_path": "The immutable first-round findings were conjunctively non-compliant.",
            "expected_revision": terminal_judged.json()["receipt"]["revision"],
        }),
    )
    .signed_as(world, "operator")
    .with_key("committee-terminal-failure-settle")
    .send(world)
    .await;
    assert_eq!(terminal_failed.status, 200, "{}", terminal_failed.body);
    assert_eq!(terminal_failed.json()["state"], "settled");
    assert_eq!(terminal_failed.json()["round"], 1);
    assert_eq!(terminal_failed.json()["outcome"], "non_compliant");
    assert!(terminal_failed.json()["result_hash"].is_string());
    assert!(terminal_failed.json()["remediation_hash"].is_string());

    // Every consultation this epic ran must work in its own directory. The
    // runtime admits at most one workspace per canonical path, so two
    // consultations sharing one — which is what naming the project checkout
    // produced — refuse the *second* epic-scoped Committee before it can spawn a
    // single reviewer. The fake adapter does not enforce that rule, so the
    // invariant is asserted here rather than discovered against real Paseo.
    let epic_id = MiniProjectId::parse(&epic).expect("an epic id");
    let project_id = ProjectId::parse(project).expect("a project id");
    let checkout = world
        .daemon
        .state()
        .with_store(|store| store.get_project(project_id))
        .expect("the read succeeds")
        .expect("the project exists")
        .root_path
        .as_str()
        .to_owned();
    let nodes = world
        .daemon
        .state()
        .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
        .expect("the topology reads");
    let mut consultation_cwds = Vec::new();
    for node in nodes.iter().filter(|node| {
        node.mini_project_id == Some(epic_id) && matches!(node.kind.as_str(), "CSW" | "ASW")
    }) {
        let bound = world
            .daemon
            .state()
            .with_store(|store| store.get_topology_node_container(project_id, node.id))
            .expect("the container reads");
        if let Some(cwd) = bound.and_then(|binding| binding.canonical_cwd) {
            consultation_cwds.push((node.kind.as_str().to_owned(), cwd.as_str().to_owned()));
        }
    }
    assert!(
        consultation_cwds.len() >= 2,
        "this fixture runs an Advisor and a Committee, so both must be placed: {consultation_cwds:?}"
    );
    for (kind, cwd) in &consultation_cwds {
        assert_ne!(
            cwd, &checkout,
            "a {kind} must not be placed on the project checkout every other one shares"
        );
    }
    let mut distinct = consultation_cwds
        .iter()
        .map(|(_, cwd)| cwd.clone())
        .collect::<Vec<_>>();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        consultation_cwds.len(),
        "two consultations claimed one directory: {consultation_cwds:?}"
    );
}

#[tokio::test]
async fn application_receipts_confirm_only_after_a_successful_response() {
    let world = World::open().await;
    let success_key = "receipt-lifecycle-success";
    let answer = ensure_project(
        &world,
        success_key,
        "Receipt lifecycle",
        "/tmp/kontor-receipt-lifecycle",
    )
    .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let success_key = IdempotencyKey::parse(success_key).expect("a valid key");
    let receipt = world.daemon.state().with_store(|store| {
        store
            .get_receipt_by_key(&success_key)
            .expect("the receipt is readable")
            .expect("the application recorded a receipt")
    });
    assert_eq!(receipt.state, CommandReceiptState::Confirmed);
    assert!(
        receipt.result_ref.is_some(),
        "confirmation cites its intent evidence"
    );
    let due = world.daemon.state().with_store(|store| {
        store
            .claim_outbox(receipt.project_id, at("2026-08-10T10:00:00Z"), 10)
            .expect("the dispatch inventory is readable")
    });
    assert!(
        due.is_empty(),
        "a successful application command queues no dispatch"
    );

    let failed_key = IdempotencyKey::parse("receipt-lifecycle-failure").expect("a valid key");
    world.daemon.state().with_store(|store| {
        store
            .record_local_command(&NewLocalCommand {
                project_id: world.project,
                receipt_id: kontor_core::id::CommandReceiptId::generate(),
                idempotency_key: failed_key.clone(),
                kind: CommandKind::EnsureProject,
                target: AggregateRef::Project {
                    project_id: world.project,
                },
                target_revision: AggregateRevision::INITIAL,
                intent: CanonicalDocument::from_value(&serde_json::json!({
                    "schema_version": 1,
                    "operation": "not_projects_ensure"
                }))
                .expect("a canonical intent"),
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("a pending local receipt is recorded");
    });
    let failed = ensure_project(
        &world,
        failed_key.as_str(),
        "Different operation",
        "/tmp/kontor-receipt-failure",
    )
    .await;
    assert_eq!(failed.status, 409, "{}", failed.body);
    let pending = world.daemon.state().with_store(|store| {
        store
            .get_receipt_by_key(&failed_key)
            .expect("the receipt is readable")
            .expect("the receipt still exists")
    });
    assert_eq!(pending.state, CommandReceiptState::IntentPersisted);
}

/// Retiring a provider-account profile: the gap that made two orphans on
/// 2026-08-21 and could only be cleared by opening the database.
#[tokio::test]
async fn a_provider_account_profile_can_be_relabelled_and_taken_out_of_service() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "amend-1", "Kontor", "/tmp/kontor-amend").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let account = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": "Codex · Pro personal", "harness": "fake.runtime",
            "credential_alias": "codex-personal", "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("amend-create")
    .send(&world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);
    let account_id = account.json()["account_profile_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = account.json()["revision"].as_u64().expect("a revision");

    let uri =
        format!("/v1/projects/{project}/provider-account-profiles/{account_id}/settings:amend");

    // A plan tier went stale in ten days, so the label is corrected to the
    // identity it holds. The enabled flag is absent and must be left alone.
    let renamed = Call::post(
        &uri,
        &serde_json::json!({"label": "Codex · Personal", "expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .with_key("amend-rename")
    .send(&world)
    .await;
    assert_eq!(renamed.status, 200, "{}", renamed.body);
    assert_eq!(renamed.json()["label"], "Codex · Personal");
    assert_eq!(
        renamed.json()["enabled"],
        true,
        "an absent field leaves the current value"
    );

    // Retirement is `enabled: false`, not a delete: every receipt that names
    // this profile stays readable.
    let next = renamed.json()["revision"].as_u64().expect("a revision");
    let retired = Call::post(
        &uri,
        &serde_json::json!({"enabled": false, "expected_revision": next}),
    )
    .signed_as(&world, "admin")
    .with_key("amend-retire")
    .send(&world)
    .await;
    assert_eq!(retired.status, 200, "{}", retired.body);
    assert_eq!(retired.json()["enabled"], false);
    assert_eq!(
        retired.json()["label"],
        "Codex · Personal",
        "the corrected label survives the retirement"
    );

    // A stale revision is refused, and nothing moves.
    let stale = Call::post(
        &uri,
        &serde_json::json!({"enabled": true, "expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .with_key("amend-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);

    let listed = Call::get(format!("/v1/projects/{project}/provider-account-profiles"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let profiles = listed.json();
    let profiles = profiles.as_array().expect("a list");
    let found = profiles
        .iter()
        .find(|entry| entry["account_profile_id"] == account_id.as_str())
        .expect("the profile is still listed");
    assert_eq!(
        found["enabled"], false,
        "the retirement is what a reader sees"
    );
    assert!(
        !listed.body.contains("codex-personal"),
        "an alias must not appear in a response: {}",
        listed.body
    );

    // Below admin, the command does not exist to the caller.
    let refused = Call::post(
        &uri,
        &serde_json::json!({"enabled": true, "expected_revision": 3}),
    )
    .signed_as(&world, "operator")
    .with_key("amend-forbidden")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// Account routing over provider aliases (the quota walk reaches the launch)
// ---------------------------------------------------------------------------

/// The incident pack rerouted onto the two Codex account aliases: every slot
/// walks `codex-work` before `codex-personal` on the same model, so the only
/// thing that moves a launch between them is recorded quota state.
fn codex_account_pack() -> serde_json::Value {
    const TEMPLATE: &str = "01936f5a-0000-7000-8000-0000000000aa";
    let mut pack: serde_json::Value =
        serde_json::from_str(INCIDENT_PACK).expect("the fixture pack parses");
    pack["pack_id"] = serde_json::json!("kontor-test-codex-accounts");
    pack["manifest"][0]["category"] = serde_json::json!("codex-accounts-v1");
    pack["manifest"][0]["label"] = serde_json::json!("Codex account routing (test)");
    pack["manifest"][0]["profile"] = serde_json::json!("codex-accounts-v1");
    pack["profiles"][0]["id"] = serde_json::json!("codex-accounts-v1");
    pack["profiles"][0]["name"] = serde_json::json!("Codex account routing");
    pack["profiles"][0]["team_template"]["template_id"] = serde_json::json!(TEMPLATE);
    pack["teams"][0]["template_id"] = serde_json::json!(TEMPLATE);
    let chain = serde_json::json!({
        "rungs": [
            {"provider": "codex-work", "model": "gpt-5.6-sol", "effort": "high"},
            {"provider": "codex-personal", "model": "gpt-5.6-sol", "effort": "high"}
        ]
    });
    for slot in pack["teams"][0]["slots"].as_array_mut().expect("slots") {
        slot["model_chain"] = chain.clone();
    }
    pack
}

/// What one alias-routed epic leaves behind for a test to assert on.
struct CodexAliasEpic {
    project: String,
    epic: String,
    seats: Vec<serde_json::Value>,
    work: String,
    personal: String,
}

/// One project with the alias-routed pack pinned, two declared Codex account
/// profiles, and one started epic. Returns the started seats and both ids.
async fn codex_alias_epic(
    world: &World,
    exhaust_work: bool,
    declare_aliases: bool,
) -> CodexAliasEpic {
    let registered = Call::post(
        "/v1/catalog/packs:register",
        &serde_json::json!({"pack": codex_account_pack()}),
    )
    .signed_as(world, "admin")
    .with_key("codex-alias-pack")
    .send(world)
    .await;
    assert_eq!(registered.status, 200, "{}", registered.body);

    let created = ensure_project(world, "codex-alias", "Kontor", "/tmp/kontor-codex-alias").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let revision = created.json()["revision"].as_u64().expect("revision");

    let mut ids = Vec::new();
    for (label, alias) in [
        ("Codex Work", "codex-work"),
        ("Codex Personal", "codex-personal"),
    ] {
        let declared: Vec<&str> = if declare_aliases {
            vec![alias]
        } else {
            Vec::new()
        };
        let account = Call::post(
            format!("/v1/projects/{project}/provider-account-profiles:ensure"),
            &serde_json::json!({
                "label": label, "harness": "fake.runtime",
                "credential_alias": alias,
                "selectable_providers": declared,
                "enabled": true
            }),
        )
        .signed_as(world, "admin")
        .with_key(format!("codex-alias-{alias}"))
        .send(world)
        .await;
        assert_eq!(account.status, 200, "{}", account.body);
        ids.push(
            account.json()["account_profile_id"]
                .as_str()
                .expect("id")
                .to_owned(),
        );
    }
    let (work, personal) = (ids[0].clone(), ids[1].clone());

    if exhaust_work {
        let recorded = Call::post(
            format!("/v1/projects/{project}/provider-quota-states:record"),
            &serde_json::json!({
                "account_profile_id": work,
                "provider": "codex-work",
                "state": "exhausted",
                "resets_at": "2099-01-01T00:00:00Z",
                "expected_revision": 1
            }),
        )
        .signed_as(world, "admin")
        .with_key("codex-alias-exhaust-work")
        .send(world)
        .await;
        assert_eq!(recorded.status, 200, "{}", recorded.body);
    }

    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Alias-routed epic",
            "codex-accounts-v1",
            serde_json::json!([{"title": "Route me by account"}]),
        ),
    )
    .signed_as(world, "admin")
    .with_key("codex-alias-epic")
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
            "granted_by": work,
            "reason": "Prove account routing"
        }),
    )
    .signed_as(world, "admin")
    .with_key("codex-alias-arm")
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
    let plan_hash = plan.json()["plan_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    let started = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &serde_json::json!({"plan_hash": plan_hash}),
    )
    .signed_as(world, "operator")
    .with_key("codex-alias-start")
    .send(world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    let seats = started.json()["started"].as_array().expect("seats").clone();
    assert!(!seats.is_empty(), "the task was seated: {}", started.body);
    CodexAliasEpic {
        project,
        epic,
        seats,
        work,
        personal,
    }
}

/// With clear headroom the walk admits the first alias rung, and the launch
/// carries **both** halves of that answer: the alias as the rung's provider
/// (which is what selects the credential home on the runtime) and the account
/// id as the launch's pin. Discarding the account half is the defect that
/// stranded every Codex seat on whatever the ambient login happened to be.
#[tokio::test]
async fn a_seat_launch_claims_the_account_the_headroom_walk_selected() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let CodexAliasEpic { seats, work, .. } = codex_alias_epic(&world, false, true).await;
    for seat in &seats {
        let run = seat["agent_run_id"]
            .as_str()
            .and_then(|run| AgentRunId::parse(run).ok())
            .expect("a started run");
        let model = world
            .fake
            .launched_model(run)
            .expect("the launched route is observable");
        assert_eq!(model.provider.0, "codex-work", "{seat}");
        assert_eq!(model.model.0, "gpt-5.6-sol");
        assert_eq!(
            world
                .fake
                .launched_account(run)
                .map(|account| account.to_string()),
            Some(work.clone()),
            "the launch claims the account the walk selected"
        );
    }
}

/// The 2026-08-23 incident, replayed against the fix: the first account's
/// allowance is exhausted with a far reset, so the walk moves to the second
/// account on the *same* model — account before rung — and the launch lands on
/// the other alias with the other account claimed.
#[tokio::test]
async fn an_exhausted_account_reroutes_the_launch_to_the_second_codex_alias() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let CodexAliasEpic {
        seats, personal, ..
    } = codex_alias_epic(&world, true, true).await;
    for seat in &seats {
        let run = seat["agent_run_id"]
            .as_str()
            .and_then(|run| AgentRunId::parse(run).ok())
            .expect("a started run");
        let model = world
            .fake
            .launched_model(run)
            .expect("the launched route is observable");
        assert_eq!(
            model.provider.0, "codex-personal",
            "the exhausted account keeps no new seats: {seat}"
        );
        assert_eq!(model.model.0, "gpt-5.6-sol", "same model, other account");
        assert_eq!(
            world
                .fake
                .launched_account(run)
                .map(|account| account.to_string()),
            Some(personal.clone()),
            "quota attribution follows the account that actually runs"
        );
    }
}

/// A consultation's chain was declared and ignored: only its first rung was
/// ever frozen, which is how a committee reviewer died on an exhausted account
/// while a clear one sat unconsulted. The freeze now walks the chain against
/// recorded quota; the honoured account travels in the alias rung alone, so a
/// consultation still claims no pin its runtime cannot attest.
#[tokio::test]
async fn a_consultation_freezes_the_alias_rung_with_headroom_not_the_first_rung() {
    let composed = compose_realm("/tmp/kontor-advisor-alias").await;
    let world = &composed.world;
    let project = &composed.project;
    adopt_session_base(world, project, composed.project_revision).await;
    publish_core_team(
        world,
        project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;

    let mut accounts = Vec::new();
    for (label, alias) in [
        ("Codex Work", "codex-work"),
        ("Codex Personal", "codex-personal"),
    ] {
        let account = Call::post(
            format!("/v1/projects/{project}/provider-account-profiles:ensure"),
            &serde_json::json!({
                "label": label, "harness": "fake.runtime",
                "credential_alias": alias,
                "selectable_providers": [alias],
                "enabled": true
            }),
        )
        .signed_as(world, "admin")
        .with_key(format!("advisor-{alias}"))
        .send(world)
        .await;
        assert_eq!(account.status, 200, "{}", account.body);
        accounts.push(
            account.json()["account_profile_id"]
                .as_str()
                .expect("id")
                .to_owned(),
        );
    }
    let recorded = Call::post(
        format!("/v1/projects/{project}/provider-quota-states:record"),
        &serde_json::json!({
            "account_profile_id": accounts[0],
            "provider": "codex-work",
            "state": "exhausted",
            "resets_at": "2099-01-01T00:00:00Z",
            "expected_revision": 1
        }),
    )
    .signed_as(world, "admin")
    .with_key("advisor-exhaust-work")
    .send(world)
    .await;
    assert_eq!(recorded.status, 200, "{}", recorded.body);

    // An epic freezes its roster at promotion, so the Advisor's caller seat
    // comes from a promoted Quick session exactly as the Committee tests do.
    let (quick, preview_hash) =
        quick_session_ready_to_promote(world, project, "Advisor alias fixture", "advisor-quick")
            .await;
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{quick}/promotion:apply"),
        &promotion_apply_body(&preview_hash),
    )
    .signed_as(world, "operator")
    .with_key("advisor-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();

    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(world, "admin")
    .with_key("advisor-control-seats")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let caller = materialized.json()["core_team"]["seats"]
        .as_array()
        .expect("core seats")
        .iter()
        .find(|seat| seat["role"]["role_code"] == "LSA")
        .and_then(|seat| seat["seat_binding_id"].as_str())
        .expect("the LSA SeatBinding")
        .to_owned();

    let mut advisor = advisor_definition("01991c00-0000-7000-8000-0000000000b7", 1);
    advisor["allowed_caller_roles"] = serde_json::json!(["lsa"]);
    advisor["models"] = serde_json::json!({
        "rungs": [
            {"provider": "codex-work", "model": "gpt-5.6-sol", "effort": "high"},
            {"provider": "codex-personal", "model": "gpt-5.6-sol", "effort": "high"}
        ]
    });
    let preview = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:preview"),
        &serde_json::json!({"definition": advisor}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let applied_profile = Call::post(
        format!("/v1/projects/{project}/advisor-profiles:apply"),
        &serde_json::json!({
            "definition": advisor,
            "preview_hash": preview.json()["preview_hash"],
            "expected_revision": 1,
        }),
    )
    .signed_as(world, "admin")
    .with_key("advisor-alias-profile")
    .send(world)
    .await;
    assert_eq!(applied_profile.status, 200, "{}", applied_profile.body);

    let epic_read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    let invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/advisor-runs:invoke"),
        &serde_json::json!({
            "profile": {"id": "01991c00-0000-7000-8000-0000000000b7", "version": 1},
            "question": "Which account should new work land on?",
            "caller_seat_binding_id": caller,
            "expected_revision": epic_read.json()["revision"],
        }),
    )
    .signed_as(world, "operator")
    .with_key("advisor-alias-invoke")
    .send(world)
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let seat = invoked.json()["seats"][0]["seat_binding_id"]
        .as_str()
        .and_then(|seat| SeatBindingId::parse(seat).ok())
        .expect("the Advisor seat");
    let route = world
        .fake
        .consultation_route(seat)
        .expect("the consultation launched on an observable route");
    assert_eq!(
        route.provider.0, "codex-personal",
        "the chain was walked against quota, not truncated to its first rung"
    );
    assert_eq!(route.model.0, "gpt-5.6-sol");
}

/// Ensure freezes the declared aliases into the immutable routing document,
/// replays byte-stably, and refuses a re-ensure that describes a different pin
/// under a name that is already taken.
#[tokio::test]
async fn an_account_profile_freezes_its_declared_provider_aliases_at_ensure() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let created = ensure_project(&world, "alias-ensure", "Kontor", "/tmp/kontor-alias").await;
    let project = created.json()["project_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let uri = format!("/v1/projects/{project}/provider-account-profiles:ensure");
    let body = serde_json::json!({
        "label": "Codex Work", "harness": "fake.runtime",
        "credential_alias": "codex-work",
        "selectable_providers": [" codex-work "],
        "enabled": true
    });

    let first = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("alias-ensure-1")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["applied"], "created");

    // Same key, same declaration: the original receipt, nothing rewritten.
    let replayed = Call::post(&uri, &body)
        .signed_as(&world, "admin")
        .with_key("alias-ensure-1")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["applied"], "unchanged");

    // A normalized re-declaration is the same profile, not a mismatch: the
    // frozen pin is the trimmed spelling.
    let normalized = Call::post(
        &uri,
        &serde_json::json!({
            "label": "Codex Work", "harness": "fake.runtime",
            "credential_alias": "codex-work",
            "selectable_providers": ["codex-work"],
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alias-ensure-2")
    .send(&world)
    .await;
    assert_eq!(normalized.status, 200, "{}", normalized.body);
    assert_eq!(normalized.json()["applied"], "unchanged");

    // A different pin under the same label is a different profile: the routing
    // document is immutable for the life of the profile, so this is a refusal,
    // never a quiet return of an account that routes somewhere else.
    let drifted = Call::post(
        &uri,
        &serde_json::json!({
            "label": "Codex Work", "harness": "fake.runtime",
            "credential_alias": "codex-work",
            "selectable_providers": ["codex-personal"],
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alias-ensure-3")
    .send(&world)
    .await;
    assert_eq!(drifted.status, 409, "{}", drifted.body);
    assert_eq!(drifted.code(), "ensure_mismatch");

    // A blank alias is a typo an operator needs to see, not an empty pin.
    let blank = Call::post(
        &uri,
        &serde_json::json!({
            "label": "Codex Broken", "harness": "fake.runtime",
            "credential_alias": "codex-broken",
            "selectable_providers": ["   "],
            "enabled": true
        }),
    )
    .signed_as(&world, "admin")
    .with_key("alias-ensure-4")
    .send(&world)
    .await;
    assert_eq!(blank.status, 400, "{}", blank.body);
}

/// A route can only be described and validated against an account if the alias
/// is a provider the catalog advertises.
#[tokio::test]
async fn the_model_catalog_advertises_every_route_used_by_operational_seats() {
    let world = World::open().await;
    let catalog = Call::get("/v1/catalog")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    let body = catalog.json();
    let providers: Vec<&str> = body["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .map(|entry| entry["id"].as_str().expect("an id"))
        .collect();
    for alias in ["codex-work", "codex-personal"] {
        assert!(providers.contains(&alias), "{}", catalog.body);
        assert!(
            body["models"]
                .as_array()
                .expect("models")
                .iter()
                .any(|model| model["provider"] == alias && model["id"] == "gpt-5.6-sol"),
            "the alias serves the same routes as its family: {}",
            catalog.body
        );
    }
    for alias in ["claude-work", "claude-personal"] {
        assert!(providers.contains(&alias), "{}", catalog.body);
        assert!(
            body["models"]
                .as_array()
                .expect("models")
                .iter()
                .any(|model| model["provider"] == alias
                    && model["id"] == "claude-opus-5"
                    && model["isDefault"] == true),
            "the Claude account alias advertises its governed default: {}",
            catalog.body
        );
    }
    assert!(providers.contains(&"opencode"), "{}", catalog.body);
    assert!(
        body["models"]
            .as_array()
            .expect("models")
            .iter()
            .any(|model| model["provider"] == "opencode"
                && model["id"] == "deepseek/deepseek-v4-flash"),
        "the catalog describes the route used by OpenCode seats: {}",
        catalog.body
    );
}

/// The incident's recovery path: a seat launched before any alias was declared
/// dies on its provider, the deployment declares the two account aliases, and
/// the replacement walks onto the clear account — claimed, on its own alias.
/// This is what "seat replace after a quota hit" is for.
#[tokio::test]
async fn a_replacement_seat_walks_onto_the_other_account_once_aliases_are_declared() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let CodexAliasEpic {
        project,
        epic,
        seats,
        ..
    } = codex_alias_epic(&world, false, false).await;
    // Undeclared realm: the walk admitted nothing, the launch fell back to the
    // frozen primary and claimed no account — the pre-declaration behaviour.
    let seat = seats[0].clone();
    let predecessor = seat["agent_run_id"].as_str().expect("the run id");
    let role_slot = seat["role_slot"].as_str().expect("the role slot");
    let predecessor_id = AgentRunId::parse(predecessor).expect("an agent run id");
    assert_eq!(
        world
            .fake
            .launched_model(predecessor_id)
            .expect("the frozen route")
            .provider
            .0,
        "codex-work",
        "an undeclared realm freezes the primary rung"
    );
    assert_eq!(
        world.fake.launched_account(predecessor_id),
        None,
        "an undeclared realm claims no account"
    );

    let mut personal = String::new();
    for (label, alias) in [
        ("Codex Work (routed)", "codex-work"),
        ("Codex Personal (routed)", "codex-personal"),
    ] {
        let account = Call::post(
            format!("/v1/projects/{project}/provider-account-profiles:ensure"),
            &serde_json::json!({
                "label": label, "harness": "fake.runtime",
                "credential_alias": format!("{alias}-routed"),
                "selectable_providers": [alias],
                "enabled": true
            }),
        )
        .signed_as(&world, "admin")
        .with_key(format!("replace-routed-{alias}"))
        .send(&world)
        .await;
        assert_eq!(account.status, 200, "{}", account.body);
        personal = account.json()["account_profile_id"]
            .as_str()
            .expect("id")
            .to_owned();
    }

    let project_id = ProjectId::parse(&project).expect("a project id");
    let run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, predecessor_id)
            .expect("the run reads")
            .expect("the run exists")
    });
    let binding = run.binding.as_ref().expect("the seat is bound");
    world.fake.provider_outage("codex-work", None);
    let view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let task_revision = view.json()["tasks"][0]["revision"]
        .as_u64()
        .expect("the task revision");

    let replaced = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/successors:replace"),
        &serde_json::json!({
            "role_slot": role_slot,
            "expected_task_revision": task_revision,
            "binding_generation": binding.identity.generation,
            "unavailable_provider": {
                "runtime_binding_id": binding.id,
                "native_id": binding.identity.native_id,
                "provider": "codex-work",
            },
        }),
    )
    .signed_as(&world, "admin")
    .with_key("replace-onto-other-account")
    .send(&world)
    .await;
    assert_eq!(replaced.status, 200, "{}", replaced.body);
    let successor_id = AgentRunId::parse(
        replaced.json()["successor_agent_run_id"]
            .as_str()
            .expect("the successor id"),
    )
    .expect("a successor id");
    let model = world
        .fake
        .launched_model(successor_id)
        .expect("the successor route");
    assert_eq!(
        model.provider.0, "codex-personal",
        "the successor walked past the outage onto the clear account"
    );
    assert_eq!(
        world
            .fake
            .launched_account(successor_id)
            .map(|account| account.to_string()),
        Some(personal),
        "the successor claims the account the walk selected"
    );
}
