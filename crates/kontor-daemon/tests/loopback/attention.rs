//! ASMA-7985 / KR-27: the attention channel contract.
//!
//! Every test starts a disposable Realm and points `attention.yml` at a mock
//! Teams Workflows endpoint on localhost. The real Workflows URL is DEP-003 and
//! does not exist until KR-15; no test may know it.

use std::time::Duration;

use kontor_core::repository::{ProjectRepository, RunRepository, WorkflowRepository};
use kontor_core::state::{TaskState, TaskTeamClosure};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::harness::World;
use crate::kickoff_world_with_attention;

fn attention_yaml(url: &str, max_attempts: u32) -> String {
    attention_yaml_with(url, 1, max_attempts)
}

fn attention_yaml_with(url: &str, timeout_seconds: u64, max_attempts: u32) -> String {
    format!(
        r#"schema_version: 1
sinks:
  - channel: teams_workflow
    url: {url}/workflows/attention
    timeout_seconds: {timeout_seconds}
    max_attempts: {max_attempts}
mention:
  target: operator@example.com
emit:
  kinds:
    - task_needs_human
    - task_blocked
    - task_parked
    - task_failed
    - command_confirmed
    - hold_review_overdue
    - waiting_seat_pending_permission
    - waiting_seat_waiting_input
    - waiting_seat_stalled
"#
    )
}

fn attention_yaml_two(first: &str, second: &str) -> String {
    format!(
        r#"schema_version: 1
sinks:
  - channel: teams_workflow
    url: {first}/workflows/attention
    timeout_seconds: 1
    max_attempts: 1
  - channel: teams_workflow
    url: {second}/workflows/attention
    timeout_seconds: 1
    max_attempts: 1
mention:
  target: operator@example.com
emit:
  kinds:
    - task_needs_human
"#
    )
}

async fn mount_accepting(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/workflows/attention"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}

async fn wait_for_requests(server: &MockServer, expected: usize) -> Vec<wiremock::Request> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let requests = server
            .received_requests()
            .await
            .expect("the mock records requests");
        if requests.len() >= expected || tokio::time::Instant::now() >= deadline {
            return requests;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A task entering `needs_human` produces exactly one content-safe alert.
#[tokio::test]
async fn a_task_entering_needs_human_emits_exactly_one_content_safe_alert() {
    let server = MockServer::start().await;
    mount_accepting(&server).await;
    let world = World::open_with_attention(&attention_yaml(&server.uri(), 1)).await;
    let (_seat_run, _seat_snapshot) = world.launch().await;

    let current = world
        .daemon
        .state()
        .with_store(|store| store.get_task(world.project, world.task))
        .expect("the seeded task reads")
        .expect("the seeded task exists");
    world
        .daemon
        .state()
        .with_store(|store| {
            store.transition_task(&kontor_core::repository::TaskTransitionRequest {
                project_id: world.project,
                task_id: world.task,
                expected_revision: current.revision,
                to: TaskState::NeedsHuman,
                resume_receipt: None,
                reopen: false,
                run_outcome: None,
                produced_artifacts: std::collections::BTreeSet::new(),
                completed_phases: std::collections::BTreeSet::new(),
                team_closure: TaskTeamClosure::NoTeam,
                occurred_at: kontor_api::now(),
            })
        })
        .expect("the task enters needs_human");

    let requests = wait_for_requests(&server, 1).await;
    assert_eq!(
        requests.len(),
        1,
        "one task transition is one alert, never two"
    );
    let body = String::from_utf8(requests[0].body.clone()).expect("a JSON body");
    assert!(
        body.contains("<at>operator@example.com</at>"),
        "the required mention is added by the destination adapter"
    );
    assert!(
        body.contains("needs_human"),
        "the payload names the settled state"
    );
    assert!(
        body.contains("harness-seat"),
        "the payload names the seat that last worked the task: {body}"
    );
    for forbidden in ["transcript", "assistant_response", "tool_result"] {
        assert!(
            !body.to_lowercase().contains(forbidden),
            "the alert carried {forbidden}: {body}"
        );
    }
    let document: Value = serde_json::from_str(&body).expect("valid JSON");
    let card = &document["attachments"][0]["content"];
    assert_eq!(card["type"], "AdaptiveCard");
    assert!(
        card["msteams"]["entities"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("operator@example.com"))
    );
}

/// B1: every configured destination receives the alert once; one destination's
/// copy is not the whole fan-out.
#[tokio::test]
async fn every_configured_destination_receives_the_alert_once() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    mount_accepting(&first).await;
    mount_accepting(&second).await;
    let world = World::open_with_attention(&attention_yaml_two(&first.uri(), &second.uri())).await;
    let (_seat_run, _seat_snapshot) = world.launch().await;

    let current = world
        .daemon
        .state()
        .with_store(|store| store.get_task(world.project, world.task))
        .expect("the seeded task reads")
        .expect("the seeded task exists");
    world
        .daemon
        .state()
        .with_store(|store| {
            store.transition_task(&kontor_core::repository::TaskTransitionRequest {
                project_id: world.project,
                task_id: world.task,
                expected_revision: current.revision,
                to: TaskState::NeedsHuman,
                resume_receipt: None,
                reopen: false,
                run_outcome: None,
                produced_artifacts: std::collections::BTreeSet::new(),
                completed_phases: std::collections::BTreeSet::new(),
                team_closure: TaskTeamClosure::NoTeam,
                occurred_at: kontor_api::now(),
            })
        })
        .expect("the task enters needs_human");

    for (name, server) in [("first", &first), ("second", &second)] {
        let requests = wait_for_requests(server, 1).await;
        assert_eq!(
            requests.len(),
            1,
            "destination {name} receives exactly one copy"
        );
        let body = String::from_utf8(requests[0].body.clone()).expect("a JSON body");
        assert!(body.contains("task_needs_human"), "{name}: {body}");
    }
}

/// A failed delivery is bounded, logged and discarded; the command succeeds.
#[tokio::test]
async fn a_failed_delivery_leaves_the_command_result_unchanged() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/workflows/attention"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let world = World::open_with_attention(&attention_yaml(&server.uri(), 2)).await;

    let current = world
        .daemon
        .state()
        .with_store(|store| store.get_task(world.project, world.task))
        .expect("the seeded task reads")
        .expect("the seeded task exists");
    let moved = world
        .daemon
        .state()
        .with_store(|store| {
            store.transition_task(&kontor_core::repository::TaskTransitionRequest {
                project_id: world.project,
                task_id: world.task,
                expected_revision: current.revision,
                to: TaskState::Blocked,
                resume_receipt: None,
                reopen: false,
                run_outcome: None,
                produced_artifacts: std::collections::BTreeSet::new(),
                completed_phases: std::collections::BTreeSet::new(),
                team_closure: TaskTeamClosure::NoTeam,
                occurred_at: kontor_api::now(),
            })
        })
        .expect("the command succeeds even though delivery will fail");
    assert_eq!(moved.state, TaskState::Blocked);

    let requests = wait_for_requests(&server, 2).await;
    assert_eq!(
        requests.len(),
        2,
        "the bounded attempt count is honoured and then the alert is discarded"
    );
    let current = world
        .daemon
        .state()
        .with_store(|store| store.get_task(world.project, world.task))
        .expect("the task reads")
        .expect("the task exists");
    assert_eq!(
        current.state,
        TaskState::Blocked,
        "a delivery outcome never rolls back or changes the command"
    );
}

/// A hold passing its review date raises exactly one alert, and a second sweep
/// is not a second alert.
#[tokio::test]
async fn a_hold_passing_its_review_date_emits_exactly_one_alert() {
    let server = MockServer::start().await;
    mount_accepting(&server).await;
    let fixture = kickoff_world_with_attention(
        "attention-hold",
        "ASMA-9501",
        "ASMA-9502",
        &attention_yaml(&server.uri(), 1),
    )
    .await;
    let mut body = crate::epic_body(
        1,
        "Attention hold",
        &fixture.category,
        serde_json::json!([{"title": "First"}]),
    );
    body["initial_hold"] = serde_json::json!({
        "held_by": fixture.account_id,
        "reason_code": "waiting_on_decision",
        "reason": "Wait for the operator decision",
        "lift_condition": "manual",
        "review_by": "2020-01-01T00:00:00Z"
    });
    let applied = crate::harness::Call::post(
        format!("/v1/projects/{}/epics:apply", fixture.project),
        &body,
    )
    .signed_as(&fixture.world, "admin")
    .with_key("attention-hold-apply")
    .send(&fixture.world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);

    assert_eq!(
        fixture
            .world
            .daemon
            .sweep_holds()
            .await
            .expect("the sweep runs"),
        1,
        "the hold passed its review date, so one attention is raised"
    );
    let requests = wait_for_requests(&server, 1).await;
    assert_eq!(requests.len(), 1, "one expiry is one alert");
    let rendered = String::from_utf8(requests[0].body.clone()).expect("a JSON body");
    assert!(rendered.contains("hold_review_overdue"));
    assert!(rendered.contains("<at>operator@example.com</at>"));

    assert_eq!(
        fixture
            .world
            .daemon
            .sweep_holds()
            .await
            .expect("the sweep replays"),
        0,
        "a second sweep is not a second attention"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the mock records requests")
            .len(),
        1,
        "re-observing the same overdue hold sends no second alert"
    );
}

/// The waiting-seat class emits once per unchanged evidence signature.
///
/// This is rule (b): an open task whose seat is observed `WaitingInput` while
/// the turn may still be waiting on an unanswered question. The second scan is
/// the cadence replay and must not become a repeating alarm.
#[tokio::test]
async fn a_waiting_seat_emits_once_and_re_observation_is_suppressed() {
    use kontor_core::id::{
        AgentRunId, CanonicalDocument, ExternalId, ExternalName, RoleKey, RuntimeBindingId,
        RuntimeKindKey, TeamRunId, Timestamp,
    };
    use kontor_core::repository::{
        NewAgentRun, NewObservation, NewRuntimeEvent, NewTeamRun, RunRepository, RuntimeBinding,
    };
    use kontor_core::state::{
        Freshness, NativeRuntimeIdentity, ObservedRunState, RunLifecycle, RuntimeContact,
    };

    let server = MockServer::start().await;
    mount_accepting(&server).await;
    let world = World::open_with_attention(&attention_yaml(&server.uri(), 1)).await;

    let team_run = TeamRunId::generate();
    let agent_run = AgentRunId::generate();
    let binding = RuntimeBindingId::generate();
    world.daemon.state().with_store(|store| {
        let snapshot = store
            .get_team_run(world.project, world.team_run)
            .expect("the fixture team run reads")
            .expect("the fixture team run exists")
            .snapshot;
        store
            .create_team_run(&NewTeamRun {
                id: team_run,
                project_id: world.project,
                task_id: world.task,
                snapshot,
                created_at: Timestamp::now(),
            })
            .expect("the team run is created");
        let identity = NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("the fake family"),
            host: ExternalName::parse("fake-host").expect("a host"),
            generation: 1,
            native_id: ExternalId::parse(&format!("native-{agent_run}")).expect("a native id"),
        };
        store
            .create_agent_run(&NewAgentRun {
                id: agent_run,
                project_id: world.project,
                team_run_id: team_run,
                parent_agent_run_id: None,
                role: RoleKey::parse("attention-seat").expect("a role"),
                account_profile_id: None,
                binding: Some(RuntimeBinding {
                    id: binding,
                    agent_run_id: agent_run,
                    identity: identity.clone(),
                    bound_at: Timestamp::now(),
                }),
                created_at: Timestamp::now(),
            })
            .expect("the agent run is created");
        let run = store
            .get_agent_run(world.project, agent_run)
            .expect("the run reads")
            .expect("the run exists");
        store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: world.project,
                    agent_run_id: agent_run,
                    identity,
                    native_event_id: None,
                    native_sequence: 1,
                    payload: CanonicalDocument::from_value(&serde_json::json!({
                        "schema_version": 1,
                        "observed_state": "waiting_input",
                        "contact": "reachable",
                        "native_sequence": 1,
                    }))
                    .expect("a canonical control payload"),
                    observed_at: Timestamp::now(),
                },
                observed: ObservedRunState::WaitingInput,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: run.revision,
                quota_state: None,
            })
            .expect("the observation reduces");
        assert_eq!(
            store
                .get_agent_run(world.project, agent_run)
                .expect("the run reads")
                .expect("the run exists")
                .projection
                .lifecycle,
            RunLifecycle::WaitingInput
        );
    });

    // Rule (c): a seat that calls itself running with no progress inside the
    // staleness bound is the combination Paseo's sidebar cannot show.
    let stalled_run = AgentRunId::generate();
    let stalled_binding = RuntimeBindingId::generate();
    world.daemon.state().with_store(|store| {
        let snapshot = store
            .get_team_run(world.project, world.team_run)
            .expect("the fixture team run reads")
            .expect("the fixture team run exists")
            .snapshot;
        let stalled_team = TeamRunId::generate();
        store
            .create_team_run(&NewTeamRun {
                id: stalled_team,
                project_id: world.project,
                task_id: world.task,
                snapshot,
                created_at: Timestamp::now(),
            })
            .expect("the stalled team run is created");
        let identity = NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("the fake family"),
            host: ExternalName::parse("fake-host").expect("a host"),
            generation: 1,
            native_id: ExternalId::parse(&format!("native-{stalled_run}")).expect("a native id"),
        };
        store
            .create_agent_run(&NewAgentRun {
                id: stalled_run,
                project_id: world.project,
                team_run_id: stalled_team,
                parent_agent_run_id: None,
                role: RoleKey::parse("stalled-seat").expect("a role"),
                account_profile_id: None,
                binding: Some(RuntimeBinding {
                    id: stalled_binding,
                    agent_run_id: stalled_run,
                    identity: identity.clone(),
                    bound_at: Timestamp::now(),
                }),
                created_at: Timestamp::now(),
            })
            .expect("the stalled run is created");
        let run = store
            .get_agent_run(world.project, stalled_run)
            .expect("the run reads")
            .expect("the run exists");
        store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: world.project,
                    agent_run_id: stalled_run,
                    identity,
                    native_event_id: None,
                    native_sequence: 1,
                    payload: CanonicalDocument::from_value(&serde_json::json!({
                        "schema_version": 1,
                        "observed_state": "running",
                        "contact": "reachable",
                        "native_sequence": 1,
                    }))
                    .expect("a canonical control payload"),
                    observed_at: Timestamp::now() - jiff::SignedDuration::from_secs(1300),
                },
                observed: ObservedRunState::Running,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: run.revision,
                quota_state: None,
            })
            .expect("the stalled observation reduces");
    });

    assert_eq!(
        world
            .daemon
            .observe_waiting_seats_once()
            .await
            .expect("the waiting-seat scan runs"),
        2,
        "the waiting-input and the stalled seat each raise one alert"
    );
    let requests = wait_for_requests(&server, 2).await;
    assert_eq!(requests.len(), 2);
    let rendered = requests
        .iter()
        .map(|request| String::from_utf8(request.body.clone()).expect("a JSON body"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("waiting_seat_waiting_input"));
    assert!(rendered.contains("waiting_seat_stalled"));

    assert_eq!(
        world
            .daemon
            .observe_waiting_seats_once()
            .await
            .expect("the waiting-seat scan replays"),
        0,
        "the same unchanged seat is suppressed"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the mock records requests")
            .len(),
        2,
        "a cadence tick is not a repeating alarm"
    );
}

/// Rule (a): a seat holding a pending permission raises exactly one alert, and
/// re-observing the same evidence raises none.
#[tokio::test]
async fn a_pending_permission_emits_once_and_unchanged_evidence_is_suppressed() {
    use kontor_core::id::ExternalId;

    let server = MockServer::start().await;
    mount_accepting(&server).await;
    let world = World::open_with_attention(&attention_yaml(&server.uri(), 1)).await;
    let (_run, snapshot) = world.launch().await;
    world
        .fake
        .raise_permission(
            &snapshot,
            ExternalId::parse("permission-attention-1").expect("a permission id"),
        )
        .expect("the fake runtime holds the pending permission");

    assert_eq!(
        world
            .daemon
            .observe_waiting_seats_once()
            .await
            .expect("the waiting-seat scan runs"),
        1,
        "a non-empty pending-permission list is one alert"
    );
    let requests = wait_for_requests(&server, 1).await;
    assert_eq!(requests.len(), 1);
    let body = String::from_utf8(requests[0].body.clone()).expect("a JSON body");
    assert!(body.contains("waiting_seat_pending_permission"), "{body}");

    assert_eq!(
        world
            .daemon
            .observe_waiting_seats_once()
            .await
            .expect("the second scan runs"),
        0,
        "unchanged evidence is suppressed"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the mock records requests")
            .len(),
        1,
        "the destination saw exactly one alert"
    );
}

/// The abandon receipt is born confirmed and raises exactly one command alert;
/// an exact idempotent replay raises none.
#[tokio::test]
async fn an_abandon_receipt_emits_once_and_an_exact_replay_emits_none() {
    use kontor_core::id::{
        AgentRunId, CanonicalDocument, CommandReceiptId, IdempotencyKey, RoleKey,
    };
    use kontor_core::receipt::AggregateRef;
    use kontor_core::repository::{NewAbandonReceipt, NewAgentRun};

    let server = MockServer::start().await;
    mount_accepting(&server).await;
    let world = World::open_with_attention(&attention_yaml(&server.uri(), 1)).await;
    let run = AgentRunId::generate();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.create_agent_run(&NewAgentRun {
                id: run,
                project_id: world.project,
                team_run_id: world.team_run,
                parent_agent_run_id: None,
                role: RoleKey::parse("abandon-seat").expect("a role"),
                account_profile_id: None,
                binding: None,
                created_at: kontor_api::now(),
            })
        })
        .expect("an unbound run is created");
    let revision = world
        .daemon
        .state()
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("the run reads")
        .expect("the run exists")
        .revision;
    let receipt_id = CommandReceiptId::generate();
    let request = NewAbandonReceipt {
        project_id: world.project,
        receipt_id,
        idempotency_key: IdempotencyKey::parse("attention-abandon").expect("a key"),
        target: AggregateRef::AgentRun { agent_run_id: run },
        target_revision: revision,
        intent: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "operation": "attention_abandon_fixture"
        }))
        .expect("a canonical decision"),
        recorded_at: kontor_api::now(),
    };
    world
        .daemon
        .state()
        .with_store(|store| store.record_abandon_receipt(&request))
        .expect("the abandon receipt is confirmed");

    let requests = wait_for_requests(&server, 1).await;
    assert_eq!(requests.len(), 1, "one abandon is one alert");
    let body = String::from_utf8(requests[0].body.clone()).expect("a JSON body");
    assert!(body.contains("command_confirmed"), "{body}");
    assert!(
        body.contains(&world.task.to_string()),
        "the command alert names its task: {body}"
    );
    assert!(
        body.contains("abandon-seat"),
        "the command alert names its seat: {body}"
    );

    world
        .daemon
        .state()
        .with_store(|store| store.record_abandon_receipt(&request))
        .expect("the exact replay answers with the first receipt");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the mock records requests")
            .len(),
        1,
        "an exact replay does not raise a second alert"
    );
}

/// B2 / TEST-017: two HTTP rejections by the same principal park the task and
/// raise exactly one `task_parked` alert.
#[tokio::test]
async fn a_second_rejection_parks_the_task_and_emits_exactly_one_alert() {
    use kontor_core::id::TaskId;

    let server = MockServer::start().await;
    mount_accepting(&server).await;
    let world = World::open_empty_with_attention(&attention_yaml(&server.uri(), 1)).await;
    world.script(super::HISTORY_LIVE);
    world.daemon.reconcile().await;
    let (seed, runs) = super::seated(&world, "attention-park").await;
    let _inspector = super::inspector_run(&world, &runs).await;
    let (uri, revision, gate) = super::gate_record_target(&world, &seed).await;
    let workflow = super::active_workflow(&world, &seed);
    let gate_spec = workflow
        .snapshot
        .definition
        .gates
        .iter()
        .find(|candidate| candidate.id.as_str() == gate)
        .expect("the projected gate is frozen")
        .clone();
    let principal = "attention-reviewer-principal";

    let first = crate::harness::Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": revision,
            "verdict": "rejected",
            "evaluator_role": gate_spec.evaluator_roles[0].as_str(),
            "evaluator_account": seed.account,
            "reviewer_principal": principal,
            "evidence": [],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("attention-park-reject-1")
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the mock records requests")
            .len(),
        0,
        "the first rejection does not park"
    );

    // The first rejection routes the workflow back to implementation. Repeating
    // the gate after rework is what the second rejection represents; this
    // fixture rewinds only the workflow's current phase/revision, not any gate
    // evidence, then drives the second rejection through the HTTP route too.
    let project = kontor_core::id::ProjectId::parse(&seed.project).expect("a project id");
    let task_id = TaskId::parse(&seed.task).expect("a task id");
    let database = world.directory.path().join(kontor_daemon::DATABASE_FILE);
    let connection = rusqlite::Connection::open(database).expect("the realm database opens");
    let next_revision = workflow.revision.get() + 1;
    connection
        .execute(
            "UPDATE task_workflows SET current_phase = ?1, revision = ?2
             WHERE project_id = ?3 AND id = ?4",
            rusqlite::params![
                gate_spec.phase.as_str(),
                i64::try_from(next_revision).expect("a small revision"),
                project.to_string(),
                workflow.id.to_string()
            ],
        )
        .expect("the fixture returns the workflow to the gate phase");

    let second = crate::harness::Call::post(
        &uri,
        &serde_json::json!({
            "expected_revision": next_revision,
            "verdict": "rejected",
            "evaluator_role": gate_spec.evaluator_roles[0].as_str(),
            "evaluator_account": seed.account,
            "reviewer_principal": principal,
            "evidence": [],
        }),
    )
    .signed_as(&world, "operator")
    .with_key("attention-park-reject-2")
    .send(&world)
    .await;
    assert_eq!(second.status, 200, "{}", second.body);

    let requests = wait_for_requests(&server, 1).await;
    assert_eq!(requests.len(), 1, "the park is exactly one alert");
    let body = String::from_utf8(requests[0].body.clone()).expect("a JSON body");
    assert!(body.contains("task_parked"), "{body}");
    let task = world
        .daemon
        .state()
        .with_store(|store| store.get_task(project, task_id))
        .expect("the task reads")
        .expect("the task exists");
    assert_eq!(task.state, TaskState::Parked);

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("the mock records requests")
            .len(),
        1,
        "the second rejection does not double-emit"
    );
}
