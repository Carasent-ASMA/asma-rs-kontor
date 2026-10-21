//! ASMA-7985 / KR-27: the attention channel contract.
//!
//! Every test starts a disposable Realm and points `attention.yml` at a mock
//! Teams Workflows endpoint on localhost. The real Workflows URL is DEP-003 and
//! does not exist until KR-15; no test may know it.

use std::time::Duration;

use kontor_core::repository::{ProjectRepository, WorkflowRepository};
use kontor_core::state::{TaskState, TaskTeamClosure};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::harness::World;
use crate::kickoff_world_with_attention;

fn attention_yaml(url: &str, max_attempts: u32) -> String {
    format!(
        r#"schema_version: 1
sink:
  url: {url}/workflows/attention
  timeout_seconds: 1
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
