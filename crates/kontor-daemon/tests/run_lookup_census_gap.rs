//! ASMA-8514 / KR-09: run lookup, native-binding census and typed gap recording.
//!
//! These drive the same router the daemon serves. The first half proves the two
//! observer reads — a run resolves to its task and epic, and the census lists
//! every binding with the state the running process can actually prove. The
//! second half proves the authority boundary: an operator records a typed gap
//! and an observer's attempt is refused before the handler, with the contract's
//! own `forbidden` code rather than a generic failure.

// This binary uses part of the shared harness; the rest is not dead code.
#[allow(dead_code)]
mod harness;

use harness::{Call, World, at, fake_family, name};
use kontor_core::id::{
    AgentRunId, CanonicalDocument, ExternalId, MiniProjectId, OperationalGapId, ProjectId,
    RuntimeBindingId, TaskId, TeamRunId,
};
use kontor_core::repository::{
    NewAgentRun, NewMiniProject, NewObservation, NewRuntimeEvent, NewTask, NewTeamRun,
    ProjectRepository, RunClosure, RunRepository, RuntimeBinding,
};
use kontor_core::state::{
    Freshness, NativeRuntimeIdentity, ObservedRunState, RunLifecycle, RuntimeContact, TaskState,
    TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
};

fn seeded_identity(agent_run: AgentRunId) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: fake_family(),
        host: name("fake-host"),
        generation: 1,
        native_id: ExternalId::parse(&format!("native-{agent_run}")).expect("a native id"),
    }
}

/// Close a seeded run with the runtime observation that proves it terminal.
///
/// The census derives `retired` from a closed run, so the fixture needs a real
/// closure rather than a hand-edited lifecycle: the store re-proves the cited
/// event and the run's own binding inside the closing transaction.
fn retire_run(world: &World, agent_run: AgentRunId) {
    world.daemon.state().with_store(|store| {
        let current = store
            .get_agent_run(world.project, agent_run)
            .expect("the seeded run reads")
            .expect("the seeded run exists");
        store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: world.project,
                    agent_run_id: agent_run,
                    identity: seeded_identity(agent_run),
                    native_event_id: None,
                    native_sequence: 1,
                    payload: CanonicalDocument::from_value(&serde_json::json!({
                        "schema_version": 1,
                        "marker": "terminal"
                    }))
                    .expect("a canonical terminal payload"),
                    observed_at: at("2026-10-21T09:04:00Z"),
                },
                observed: ObservedRunState::Succeeded,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: current.revision,
                quota_state: None,
            })
            .expect("the terminal observation is recorded");
        let terminal = store
            .read_runtime_events(world.project, agent_run, None)
            .expect("the run events read")
            .into_iter()
            .next_back()
            .expect("the terminal event is stored");
        let current = store
            .get_agent_run(world.project, agent_run)
            .expect("the observed run reads")
            .expect("the observed run exists");
        store
            .close_agent_run(&RunClosure {
                project_id: world.project,
                agent_run_id: agent_run,
                expected_revision: current.revision,
                evidence: TerminalEvidence {
                    outcome: TerminalOutcome::Succeeded,
                    source: TerminalEvidenceSource::RuntimeObservation {
                        cursor: terminal.cursor,
                    },
                    evidence_hash: terminal.payload.hash().clone(),
                    closed_at: at("2026-10-21T09:05:00Z"),
                },
            })
            .expect("the run closes with its own event");
    });
}

/// Persist a task that belongs to an epic, with one run and one binding that
/// this process deliberately does not hold a session snapshot for.
fn seed_epic_task_run(world: &World) -> (MiniProjectId, TaskId, AgentRunId, RuntimeBindingId) {
    let epic = MiniProjectId::generate();
    let task = TaskId::generate();
    let team_run = TeamRunId::generate();
    let agent_run = AgentRunId::generate();
    let binding = RuntimeBindingId::generate();
    world.daemon.state().with_store(|store| {
        store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: world.project,
                name: name("Run lookup epic"),
                created_at: at("2026-10-21T09:00:00Z"),
            })
            .expect("the epic is created");
        store
            .create_task(&NewTask {
                id: task,
                project_id: world.project,
                mini_project_id: Some(epic),
                title: name("Run lookup task"),
                module: None,
                state: TaskState::Ready,
                created_at: at("2026-10-21T09:00:00Z"),
            })
            .expect("the task is created");
        let snapshot = store
            .get_team_run(world.project, world.team_run)
            .expect("the fixture team run reads")
            .expect("the fixture team run exists")
            .snapshot;
        store
            .create_team_run(&NewTeamRun {
                id: team_run,
                project_id: world.project,
                task_id: task,
                snapshot,
                created_at: at("2026-10-21T09:00:00Z"),
            })
            .expect("the team run is created");
        store
            .create_agent_run(&NewAgentRun {
                id: agent_run,
                project_id: world.project,
                team_run_id: team_run,
                parent_agent_run_id: None,
                role: kontor_core::id::RoleKey::parse("run-lookup-seat").expect("a role key"),
                account_profile_id: None,
                binding: Some(RuntimeBinding {
                    id: binding,
                    agent_run_id: agent_run,
                    identity: seeded_identity(agent_run),
                    bound_at: at("2026-10-21T09:00:00Z"),
                }),
                created_at: at("2026-10-21T09:00:00Z"),
            })
            .expect("the run and its binding are created");
    });
    (epic, task, agent_run, binding)
}

fn gap_body(gap_id: OperationalGapId, agent_run_id: AgentRunId) -> serde_json::Value {
    serde_json::json!({
        "gap_id": gap_id.to_string(),
        "failure_class": "capability_missing",
        "intended_operation": "kontor_run_get the admitted run",
        "checkpoint": "before task-specific exploration",
        "fallback_action": "read the recorded binding through the supported census",
        "owner": "KR-09 delivery seat",
        "agent_run_id": agent_run_id.to_string(),
    })
}

#[tokio::test]
async fn an_observer_reads_a_run_task_and_epic_and_every_binding_state() {
    let world = World::open().await;
    let (attached_run, attached_snapshot) = world.launch().await;
    let (epic, task, detached_run, detached_binding) = seed_epic_task_run(&world);
    let (retired_epic, retired_task, retired_run, retired_binding) = seed_epic_task_run(&world);
    retire_run(&world, retired_run);

    // The fixture task has no epic.
    let attached = Call::get(format!("/v1/runs/{attached_run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(attached.status, 200, "{}", attached.body);
    let attached = attached.json();
    assert_eq!(attached["realm_id"], world.realm_id().to_string());
    assert_eq!(attached["value"]["task_id"], world.task.to_string());
    assert!(
        attached["value"]["epic_id"].is_null(),
        "a task without an epic answers null"
    );

    // The seeded task resolves both identities, so a caller can read the task
    // and epic without reconstructing them from labels.
    let detached = Call::get(format!("/v1/runs/{detached_run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(detached.status, 200, "{}", detached.body);
    let detached = detached.json();
    assert_eq!(detached["value"]["task_id"], task.to_string());
    assert_eq!(detached["value"]["epic_id"], epic.to_string());

    let census = Call::get("/v1/runtime-bindings")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(census.status, 200, "{}", census.body);
    let census = census.json();
    let bindings = census["value"]["bindings"]
        .as_array()
        .expect("the census lists bindings");
    assert_eq!(
        bindings.len(),
        3,
        "every persisted binding is listed, including the detached and retired ones"
    );

    let attached_binding = bindings
        .iter()
        .find(|binding| binding["binding_id"] == attached_snapshot.binding_id().to_string())
        .expect("the attached binding is listed");
    assert_eq!(attached_binding["agent_run_id"], attached_run.to_string());
    assert_eq!(attached_binding["task_id"], world.task.to_string());
    assert_eq!(attached_binding["state"], "attached");
    assert_eq!(attached_binding["run_lifecycle"], "queued");
    assert!(attached_binding["run_closed_at"].is_null());

    let detached_binding = bindings
        .iter()
        .find(|binding| binding["binding_id"] == detached_binding.to_string())
        .expect("the detached binding is listed");
    assert_eq!(detached_binding["agent_run_id"], detached_run.to_string());
    assert_eq!(detached_binding["task_id"], task.to_string());
    assert_eq!(detached_binding["epic_id"], epic.to_string());
    assert_eq!(detached_binding["state"], "detached");
    assert_eq!(detached_binding["run_lifecycle"], "queued");

    let retired_binding = bindings
        .iter()
        .find(|binding| binding["binding_id"] == retired_binding.to_string())
        .expect("the retired binding is still listed");
    assert_eq!(retired_binding["agent_run_id"], retired_run.to_string());
    assert_eq!(retired_binding["task_id"], retired_task.to_string());
    assert_eq!(retired_binding["epic_id"], retired_epic.to_string());
    assert_eq!(retired_binding["state"], "retired");
    assert_eq!(
        retired_binding["run_lifecycle"],
        RunLifecycle::Succeeded.as_str()
    );
    assert!(
        retired_binding["run_closed_at"].is_string(),
        "a retired binding carries the instant its run closed"
    );
}

#[tokio::test]
async fn an_operator_records_a_typed_gap_and_an_observer_is_refused() {
    let world = World::open().await;
    let (epic, task, agent_run, _) = seed_epic_task_run(&world);
    let project = ProjectId::parse(&world.project.to_string()).expect("a project id");
    let gap_id = OperationalGapId::generate();
    let uri = format!("/v1/projects/{project}/tasks/{task}/operational-gaps:record");
    let key = "KR-09-gap-record";

    let refused = Call::post(&uri, &gap_body(gap_id, agent_run))
        .signed_as(&world, "observer")
        .with_key(key)
        .send(&world)
        .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    let refused = refused.json();
    assert_eq!(refused["code"], "forbidden", "{}", refused);
    assert!(
        refused["rule"]
            .as_str()
            .is_some_and(|rule| rule.contains("higher realm authority")),
        "the refusal is the authority's typed one: {refused}"
    );
    let none = world
        .daemon
        .state()
        .with_store(|store| store.operational_gaps_for_task(world.project, task))
        .expect("the gap ledger reads");
    assert!(
        none.is_empty(),
        "an observer's refused attempt wrote nothing"
    );

    let recorded = Call::post(&uri, &gap_body(gap_id, agent_run))
        .signed_as(&world, "operator")
        .with_key(key)
        .send(&world)
        .await;
    assert_eq!(recorded.status, 200, "{}", recorded.body);
    let recorded = recorded.json();
    assert_eq!(recorded["realm_id"], world.realm_id().to_string());
    assert_eq!(recorded["gap_id"], gap_id.to_string());
    assert_eq!(recorded["project_id"], project.to_string());
    assert_eq!(recorded["task_id"], task.to_string());
    assert_eq!(recorded["epic_id"], epic.to_string());
    assert_eq!(recorded["agent_run_id"], agent_run.to_string());
    assert_eq!(recorded["failure_class"], "capability_missing");
    assert_eq!(recorded["status"], "open");
    assert_eq!(recorded["replayed"], false);

    let replayed = Call::post(&uri, &gap_body(gap_id, agent_run))
        .signed_as(&world, "operator")
        .with_key(key)
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    let replayed = replayed.json();
    assert_eq!(replayed["gap_id"], gap_id.to_string());
    assert_eq!(replayed["replayed"], true);

    let mut changed = gap_body(gap_id, agent_run);
    changed["owner"] = serde_json::json!("another owner");
    let conflicted = Call::post(&uri, &changed)
        .signed_as(&world, "operator")
        .with_key(key)
        .send(&world)
        .await;
    assert_eq!(conflicted.status, 409, "{}", conflicted.body);
    assert_eq!(
        conflicted.json()["code"],
        "revision_conflict",
        "the same key naming different content is a typed conflict"
    );

    let stored = world
        .daemon
        .state()
        .with_store(|store| store.operational_gaps_for_task(world.project, task))
        .expect("the gap ledger reads");
    assert_eq!(stored.len(), 1, "one observation, one record");
    assert_eq!(stored[0].gap_id, gap_id);
    assert_eq!(stored[0].mini_project_id, Some(epic));
    assert_eq!(stored[0].failure_class.as_str(), "capability_missing");
    assert_eq!(
        stored[0].owner.as_str(),
        "KR-09 delivery seat",
        "the refused second content left the original record untouched"
    );
}
