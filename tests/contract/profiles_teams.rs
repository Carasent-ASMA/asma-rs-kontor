//! Profiles and teams against the real store and the real adapter port.
//!
//! This suite deliberately uses no mock session type. Revisions and snapshots go
//! through [`SqliteStore`]; every native session comes out of
//! [`ScriptedFakeRuntime`] as a real [`RuntimeBindingSnapshot`]. What is proved
//! here is therefore proved about the same types a Paseo, AO or Codex adapter
//! will be handed.
//!
//! The mutants this suite exists to kill:
//!
//! * bypassing occupied-slot admission, so a second session starts in one seat;
//! * reusing the old binding id for a replacement instead of minting a fresh one;
//! * launching a replacement before the old run closed with stored evidence;
//! * dropping `parent_agent_run_id`, or moving the successor to another slot;
//! * treating two sessions of one logical role as parallel without two slots;
//! * hydrating a roster from state that contains two live leaves, a cross-slot
//!   parent, a branching parent or an over-deep chain;
//! * rewriting a stored v1 revision or snapshot after v2 is published.

use std::collections::BTreeSet;

use kontor_core::id::CanonicalDocument;
use kontor_core::id::{
    AccountProfileId, AgentRunId, ArtifactKey, BoundedText, ContentHash, CredentialAlias,
    ExternalId, ExternalName, MiniProjectId, ProjectId, RuntimeBindingId, RuntimeKindKey,
    SCHEMA_VERSION, SpecVersion, TaskId, TaskWorkflowId, TeamRunId, Timestamp, parse_utc_timestamp,
};
use kontor_core::repository::{
    AgentRun, CredentialReference, CredentialReferenceKind, NewAccountProfile, NewGateEvaluation,
    NewObservation, NewProject, NewRuntimeEvent, NewTask, NewTaskPersonaSnapshot, NewTaskWorkflow,
    NewTeamRun, ProjectRepository, RunClosure, RunRepository, SpecRepository,
    TaskTransitionRequest, WorkflowRepository,
};
use kontor_core::spec::{
    PersonaScenarioSpec, ResolvedWorkProfileSnapshot, TeamRunSnapshot, WorkProfileSpec,
};
use kontor_core::state::{
    Freshness, ObservedRunState, RunLifecycle, RuntimeContact, TaskState, TaskTeamClosure,
    TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
};
use kontor_profiles::pack::{
    PackAvailability, ProfilePackSpec, ResolvedProfileBundle, TaskTeamEvidence,
    certify_task_closure, resolve_profile, revise_persona_scenario, revise_work_profile,
};
use kontor_profiles::seeds::bundled_pack;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::capability::{
    RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{AdapterCall, ScriptedFakeRuntime};
use kontor_runtime::request::LaunchPlacement;
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_store::SqliteStore;
use kontor_teams::run::{SlotLaunch, TeamRunLease, TeamRunSlots};
use kontor_teams::spec::{RoleSlotId, TeamTemplateSpec, revise_team_template};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn now() -> Timestamp {
    at("2026-08-10T09:00:00Z")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn execution_scope(task_id: TaskId, worktree: WorkspaceRoot) -> ExecutionScope {
    ExecutionScope::for_task(
        EpicScope {
            mini_project_id: MiniProjectId::generate(),
            external_epic_key: ExternalId::parse("ASMA-PROFILES").expect("epic key"),
            short_title: name("Profiles contract"),
        },
        TaskScope {
            task_id,
            external_issue_key: ExternalId::parse("ASMA-PROFILES-1").expect("issue key"),
            short_code: ExternalId::parse("PROFILES-1").expect("short code"),
            worktree,
        },
    )
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker
    }))
    .expect("a canonical document")
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 16,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

/// The bundled category whose pinned team seats one logical role twice.
///
/// Found structurally — by cardinality, never by name — so the suite proves the
/// parallelism rule rather than the seed that happens to exercise it.
fn parallel_category(pack: &ProfilePackSpec) -> ResolvedProfileBundle {
    for entry in &pack.manifest {
        if entry.availability != PackAvailability::Seeded {
            continue;
        }
        let Some(bundle) = resolve_profile(pack, &entry.category, now()).ok() else {
            continue;
        };
        let Some(revision) = &bundle.team else {
            continue;
        };
        let team = TeamTemplateSpec::from_revision(revision).expect("the team reads back");
        if team
            .roles
            .iter()
            .any(|requirement| requirement.min_slots >= 2)
        {
            return bundle;
        }
    }
    panic!("a bundled team seats one logical role more than once");
}

/// The two slots of the repeated logical role, in declaration order.
fn parallel_slots(team: &TeamTemplateSpec) -> Vec<RoleSlotId> {
    let repeated = team
        .roles
        .iter()
        .find(|requirement| requirement.min_slots >= 2)
        .expect("the repeated role requirement");
    team.slots_of(&repeated.role.role)
        .iter()
        .map(|slot| slot.id.clone())
        .collect()
}

struct World {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
    task: TaskId,
    fake: ScriptedFakeRuntime,
    team_run_id: TeamRunId,
    workspace: WorkspaceBindingSnapshot,
    bundle: ResolvedProfileBundle,
    team: TeamTemplateSpec,
    snapshot: TeamRunSnapshot,
}

impl World {
    async fn open() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("kontor.db");
        let store = SqliteStore::open(&path).expect("the store opens");

        let project = ProjectId::generate();
        store
            .create_project(&NewProject {
                id: project,
                name: name("Contract project"),
                root_path: name("/tmp/kontor-contract"),
                created_at: now(),
            })
            .expect("a project is created");
        let task = TaskId::generate();
        store
            .create_task(&NewTask {
                id: task,
                project_id: project,
                mini_project_id: None,
                title: name("A team task"),
                module: None,
                state: kontor_core::state::TaskState::Ready,
                created_at: now(),
            })
            .expect("a task is created");

        let pack = bundled_pack().expect("the bundled pack loads");
        let bundle = parallel_category(&pack);
        let revision = bundle.team.clone().expect("the profile pinned a team");
        let team = TeamTemplateSpec::from_revision(&revision).expect("the team reads back");

        // Both revisions land through the existing store, not beside it.
        store
            .insert_work_profile(project, &bundle.profile.definition)
            .expect("the profile revision is stored");
        store
            .insert_team_template(project, &revision)
            .expect("the team revision is stored");

        let snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION);
        let team_run_id = TeamRunId::generate();
        store
            .create_team_run(&NewTeamRun {
                id: team_run_id,
                project_id: project,
                task_id: task,
                snapshot: snapshot.clone(),
                created_at: now(),
            })
            .expect("the team run is created");

        let fake = ScriptedFakeRuntime::new(capabilities());
        let workspace = fake
            .prepare_workspace(&WorkspacePrepareRequest {
                scope: execution_scope(
                    task,
                    WorkspaceRoot::parse("/w/contract-task").expect("an absolute path"),
                ),
                team_run_id,
                task_id: task,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: WorkspaceRoot::parse("/w/contract-task").expect("an absolute path"),
                requested_at: at("2026-08-10T08:59:00Z"),
            })
            .await
            .expect("the runtime prepares one task workspace")
            .snapshot;

        Self {
            _directory: directory,
            path,
            store,
            project,
            task,
            fake,
            team_run_id,
            workspace,
            bundle,
            team,
            snapshot,
        }
    }

    fn slots(&self) -> TeamRunSlots {
        TeamRunSlots::open(self.lease(), &self.snapshot).expect("the seats open")
    }

    /// Exclusive ownership of this world's team run.
    fn lease(&self) -> TeamRunLease {
        TeamRunLease::acquire(self.team_run_id).expect("this world is the only writer")
    }

    fn launch_count(&self) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::Launch(_)))
            .count()
    }

    fn launch_input(&self) -> SlotLaunch {
        SlotLaunch {
            scope: execution_scope(self.task, self.workspace.root().clone()),
            task_id: self.task,
            binding_id: RuntimeBindingId::generate(),
            placement: Some(LaunchPlacement::Workspace(self.workspace.clone())),
            cwd: self.workspace.root().clone(),
            account_profile_id: None,
            prompt: BoundedText::parse("do the work").expect("bounded text"),
            model_rung: kontor_core::spec::ModelRung {
                provider: kontor_core::spec::ProviderRef("test".to_owned()),
                model: kontor_core::spec::ModelRef("test".to_owned()),
                effort: None,
            },
            context_policy: kontor_core::spec::ContextPolicySnapshot::standard(
                &kontor_core::spec::ContextWindowBounds::unknown(),
                true,
                kontor_core::id::SCHEMA_VERSION,
                now(),
            )
            .expect("the standard fallback freezes"),
            autonomy: kontor_core::spec::SeatAutonomy::standard(),
            requested_at: now(),
        }
    }

    /// Reserve the seat, have the runtime admit it, launch, persist and bind.
    ///
    /// The launch precedes the insert because KON-MVP-03 writes an agent run and
    /// its runtime binding in a single statement and offers no later bind: the
    /// native identity does not exist until the runtime has answered. Admission
    /// comes before both, and is the step that decides whether this seat may
    /// hold a session at all — the local reservation only decides whether *this
    /// roster* thinks so.
    async fn occupy(
        &self,
        slots: &mut TeamRunSlots,
        slot: &RoleSlotId,
        permit_run: AgentRunId,
        successor_of: Option<kontor_teams::run::ClosedSlot>,
    ) -> RuntimeBindingSnapshot {
        let permit = match successor_of {
            Some(closed) => slots
                .reserve_successor(closed, permit_run)
                .expect("a closed seat mints the successor permit"),
            None => slots
                .reserve(slot, permit_run)
                .expect("a vacant seat reserves"),
        };
        let launch = self.launch_input();
        let authority = self
            .fake
            .admit_launch(&permit.admission_request(&launch))
            .await
            .expect("the runtime admits this seat")
            .into_authority()
            .expect("a vacant seat is admitted rather than resumed");
        let prepared = permit.launch_request(authority, launch);
        let outcome = self
            .fake
            .launch(prepared.request())
            .await
            .expect("the seat launches");
        self.store
            .create_agent_run(
                &prepared
                    .new_agent_run(self.project, None, Some(&outcome.snapshot), now())
                    .expect("the launched session belongs to this attempt"),
            )
            .expect("the attempt and its binding are persisted together");
        slots
            .bind(prepared, &outcome.snapshot)
            .expect("the seat binds its own session");
        outcome.snapshot
    }

    /// Record a terminal observation from the run's own session and close it.
    ///
    /// The observation carries the binding's real [`NativeRuntimeIdentity`]:
    /// KON-MVP-03 refuses an event that some other session emitted, which is the
    /// invariant that makes "closed with evidence" mean anything.
    fn close_in_store(&self, binding: &RuntimeBindingSnapshot, sequence: u64) -> AgentRun {
        // The observation being recorded came *from* the runtime, so the
        // runtime has to have made it. Without this the store would be closing
        // a run whose session the runtime still considers live — which is
        // exactly what runtime-owned admission refuses to act on.
        self.fake
            .observe_terminal(binding, ObservedRunState::Succeeded)
            .expect("the runtime observes its own session finish");
        let run = binding.agent_run_id();
        let current = self
            .store
            .get_agent_run(self.project, run)
            .expect("the read succeeds")
            .expect("the run exists");
        self.store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: self.project,
                    agent_run_id: run,
                    identity: binding.identity().clone(),
                    native_event_id: None,
                    native_sequence: sequence,
                    payload: CanonicalDocument::from_value(&serde_json::json!({
                        "schema_version": 1,
                        "marker": format!("terminal-{sequence}")
                    }))
                    .expect("a canonical payload"),
                    observed_at: at("2026-08-10T10:00:00Z"),
                },
                observed: ObservedRunState::Succeeded,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: current.revision,
            })
            .expect("the terminal observation is recorded");

        let stored = self
            .store
            .read_runtime_events(self.project, run, None)
            .expect("the read succeeds")
            .into_iter()
            .next_back()
            .expect("the event exists");
        let current = self
            .store
            .get_agent_run(self.project, run)
            .expect("the read succeeds")
            .expect("the run exists");
        self.store
            .close_agent_run(&RunClosure {
                project_id: self.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: TerminalEvidence {
                    outcome: TerminalOutcome::Succeeded,
                    source: TerminalEvidenceSource::RuntimeObservation {
                        cursor: stored.cursor,
                    },
                    evidence_hash: stored.payload.hash().clone(),
                    closed_at: at("2026-08-10T10:01:00Z"),
                },
            })
            .expect("an evidenced closure succeeds");

        self.store
            .get_agent_run(self.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
    }

    /// Freeze the resolved profile onto the task and return its workflow.
    fn freeze_workflow(&self) -> TaskWorkflowId {
        let workflow = TaskWorkflowId::generate();
        self.store
            .create_task_workflow(&NewTaskWorkflow {
                id: workflow,
                project_id: self.project,
                task_id: self.task,
                snapshot: self.bundle.profile.clone(),
                current_phase: self.bundle.profile.definition.entry_phase.clone(),
                created_at: now(),
            })
            .expect("the workflow freezes the profile onto the task");
        workflow
    }

    /// Record an authorized pass for every gate the pinned profile declares.
    fn pass_every_gate(&self, workflow: TaskWorkflowId) {
        let account = AccountProfileId::generate();
        self.store
            .create_account_profile(&NewAccountProfile {
                id: account,
                project_id: self.project,
                label: name("Evaluator"),
                external_account_id: None,
                harness: RuntimeKindKey::parse("zz.runtime").expect("a valid runtime key"),
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: CredentialAlias::parse("zz-evaluator").expect("a valid alias"),
                },
                environment: document("evaluator-environment"),
                routing: document("evaluator-routing"),
                capability: document("evaluator-capability"),
                provider_identity: None,
                enabled: true,
                created_at: now(),
            })
            .expect("an evaluator account exists");
        for gate in &self.bundle.profile.definition.gates {
            self.store
                .append_gate_evaluation(&NewGateEvaluation {
                    project_id: self.project,
                    workflow_id: workflow,
                    gate: gate.id.clone(),
                    verdict: kontor_core::state::GateVerdict::Passed,
                    evaluator_role: gate.evaluator_roles[0].clone(),
                    evaluator_account: account,
                    evidence: gate.required_evidence.clone(),
                    // This fixture is about profile and team structure, not
                    // about reviewer identity: it records no principal, and a
                    // verdict attributable to nobody counts towards nobody's
                    // rejection stream.
                    agent_run_id: None,
                    reviewer_principal: None,
                    policy_evaluation_id: None,
                    recorded_at: now(),
                })
                .expect("an authorized evaluator passes the gate");
        }
    }

    /// A task transition that satisfies the profile in full and cites `team`.
    fn terminal_request(
        &self,
        to: TaskState,
        expected_revision: kontor_core::id::AggregateRevision,
        team: TaskTeamClosure,
        phases: &BTreeSet<kontor_core::id::PhaseKey>,
        artifacts: &BTreeSet<ArtifactKey>,
    ) -> TaskTransitionRequest {
        TaskTransitionRequest {
            project_id: self.project,
            task_id: self.task,
            expected_revision,
            to,
            resume_receipt: None,
            reopen: false,
            run_outcome: None,
            produced_artifacts: artifacts.clone(),
            completed_phases: phases.clone(),
            team_closure: team,
            occurred_at: now(),
        }
    }

    fn stored_runs(&self, ids: &[AgentRunId]) -> Vec<AgentRun> {
        ids.iter()
            .map(|id| {
                self.store
                    .get_agent_run(self.project, *id)
                    .expect("the read succeeds")
                    .expect("the run exists")
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// AC-5 — same-role parallelism needs two declared slots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_declared_slots_of_one_role_run_concurrently_in_one_workspace() {
    let world = World::open().await;
    let pair = parallel_slots(&world.team);
    assert_eq!(pair.len(), 2, "the logical role is seated exactly twice");
    assert_ne!(pair[0], pair[1], "with two distinct slot ids");

    let mut slots = world.slots();
    let first_run = AgentRunId::generate();
    let second_run = AgentRunId::generate();
    let first = world.occupy(&mut slots, &pair[0], first_run, None).await;
    let second = world.occupy(&mut slots, &pair[1], second_run, None).await;

    assert_ne!(first.binding_id(), second.binding_id());
    assert_ne!(first.identity(), second.identity());
    assert_eq!(world.launch_count(), 2);

    // One team run, one verified workspace, two seats.
    assert_eq!(world.fake.workspace_count(), 1);

    for (slot, run) in [(&pair[0], first_run), (&pair[1], second_run)] {
        let stored = world
            .store
            .get_agent_run(world.project, run)
            .expect("the read succeeds")
            .expect("the run exists");
        assert_eq!(&stored.role, slot.as_role_key(), "each row keeps its seat");
        assert_eq!(stored.team_run_id, world.team_run_id);
        assert!(stored.parent_agent_run_id.is_none(), "both are roots");
    }
}

// ---------------------------------------------------------------------------
// AC-4 — one live native session per seat
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_second_session_for_one_seat_is_refused_before_the_runtime_is_called() {
    let world = World::open().await;
    let pair = parallel_slots(&world.team);
    let mut slots = world.slots();

    let bound = world
        .occupy(&mut slots, &pair[0], AgentRunId::generate(), None)
        .await;
    let launches = world.launch_count();

    let refused = slots.reserve(&pair[0], AgentRunId::generate());
    assert!(
        refused.is_err(),
        "an occupied seat must not mint a second permit"
    );
    assert_eq!(
        world.launch_count(),
        launches,
        "the refusal happens before the fake records a launch"
    );
    assert_eq!(
        slots.current_binding(&pair[0]),
        Some(&bound),
        "the seat still holds exactly its first binding"
    );

    // The other seat is unaffected: the rule is per slot, not per team.
    world
        .occupy(&mut slots, &pair[1], AgentRunId::generate(), None)
        .await;
    assert_eq!(world.launch_count(), launches + 1);
}

#[tokio::test]
async fn replacement_closes_the_old_session_and_links_the_successor() {
    let world = World::open().await;
    let pair = parallel_slots(&world.team);
    let mut slots = world.slots();

    let first_run = AgentRunId::generate();
    let old = world.occupy(&mut slots, &pair[0], first_run, None).await;
    let old_binding_id = old.binding_id();
    let old_identity = old.identity().clone();

    let occupied = slots.occupied(&pair[0]).expect("the seat is occupied");
    let pending = slots
        .begin_replacement(occupied)
        .expect("replacement begins");
    assert_eq!(pending.binding(), &old, "the old binding is retained");
    assert!(
        slots.reserve(&pair[0], AgentRunId::generate()).is_err(),
        "no launch is reachable while the old session is open"
    );
    assert_eq!(world.launch_count(), 1);

    // Only a stored, evidenced terminal closes it.
    let closed_row = world.close_in_store(&old, 5);
    assert!(closed_row.projection.is_closed());
    let closed = slots
        .close_replaced(pending, &closed_row)
        .expect("the evidenced terminal closes the seat");

    let successor_run = AgentRunId::generate();
    let fresh = world
        .occupy(&mut slots, &pair[0], successor_run, Some(closed))
        .await;

    let successor = world
        .store
        .get_agent_run(world.project, successor_run)
        .expect("the read succeeds")
        .expect("the successor exists");
    assert_eq!(
        successor.parent_agent_run_id,
        Some(first_run),
        "the successor is linked to the run it replaces"
    );
    assert_eq!(
        &successor.role,
        pair[0].as_role_key(),
        "and stays in the same seat"
    );
    assert_eq!(successor.team_run_id, world.team_run_id);
    assert_ne!(fresh.binding_id(), old_binding_id, "a fresh binding id");
    assert_ne!(&old_identity, fresh.identity(), "a fresh native session");

    // The retired run and its binding are untouched by all of that.
    let retired = world
        .store
        .get_agent_run(world.project, first_run)
        .expect("the read succeeds")
        .expect("the retired run exists");
    assert!(retired.projection.is_closed(), "it stays closed");
    assert!(retired.closed_at.is_some());
    assert_eq!(
        retired.terminal.as_ref().map(|evidence| evidence.outcome),
        Some(TerminalOutcome::Succeeded),
        "with the evidence it closed on"
    );
    assert_eq!(
        old.binding_id(),
        old_binding_id,
        "the old value never moved"
    );
    assert_eq!(old.identity(), &old_identity);
}

#[tokio::test]
async fn hydration_from_two_live_leaves_yields_no_launch_permit() {
    let world = World::open().await;
    let pair = parallel_slots(&world.team);
    let mut slots = world.slots();

    // One seat, two persisted roots: exactly what a lost acknowledgement looks
    // like after a restart. The rows are written through the raw port, which is
    // the unsupported path this backstop exists for.
    let first = AgentRunId::generate();
    world.occupy(&mut slots, &pair[0], first, None).await;
    let second = AgentRunId::generate();
    world
        .store
        .create_agent_run(&kontor_core::repository::NewAgentRun {
            id: second,
            project_id: world.project,
            team_run_id: world.team_run_id,
            parent_agent_run_id: None,
            role: pair[0].as_role_key().clone(),
            account_profile_id: None,
            binding: None,
            created_at: now(),
        })
        .expect("the raw port accepts a second row for the same seat");

    let rows = world.stored_runs(&[first, second]);
    // The live manager must go before a restart can rebuild the same team run:
    // one team run has one writer, which is the other half of this guarantee.
    drop(slots);
    let refused = TeamRunSlots::hydrate(world.lease(), &world.snapshot, &rows, &[]);
    assert!(
        refused.is_err(),
        "two non-terminal runs in one seat must fail closed"
    );

    let launches = world.launch_count();
    assert_eq!(
        launches, 1,
        "and no launch was attempted from the broken state"
    );
}

#[tokio::test]
async fn hydration_rejects_lineage_the_slot_api_could_never_have_produced() {
    let world = World::open().await;
    let pair = parallel_slots(&world.team);
    let mut slots = world.slots();

    let first = AgentRunId::generate();
    let first_binding = world.occupy(&mut slots, &pair[0], first, None).await;
    let closed = world.close_in_store(&first_binding, 5);

    let other = AgentRunId::generate();
    let other_binding = world.occupy(&mut slots, &pair[1], other, None).await;
    let other_closed = world.close_in_store(&other_binding, 6);
    drop(slots);

    // A successor whose parent is another seat's run.
    let cross_slot = AgentRun {
        id: AgentRunId::generate(),
        parent_agent_run_id: Some(other_closed.id),
        role: pair[0].as_role_key().clone(),
        ..closed.clone()
    };
    assert!(
        TeamRunSlots::hydrate(
            world.lease(),
            &world.snapshot,
            &[closed.clone(), other_closed.clone(), cross_slot],
            &[]
        )
        .is_err(),
        "a parent outside the seat must fail closed"
    );

    // Two successors of one parent.
    let branch_a = AgentRun {
        id: AgentRunId::generate(),
        parent_agent_run_id: Some(closed.id),
        ..closed.clone()
    };
    let branch_b = AgentRun {
        id: AgentRunId::generate(),
        parent_agent_run_id: Some(closed.id),
        ..closed.clone()
    };
    assert!(
        TeamRunSlots::hydrate(
            world.lease(),
            &world.snapshot,
            &[closed.clone(), branch_a, branch_b],
            &[]
        )
        .is_err(),
        "a branching parent must fail closed"
    );

    // A chain longer than the template allows.
    let mut chain = vec![closed.clone()];
    let mut parent = closed.id;
    for _ in 0..=world.team.max_successor_depth {
        let next = AgentRun {
            id: AgentRunId::generate(),
            parent_agent_run_id: Some(parent),
            ..closed.clone()
        };
        parent = next.id;
        chain.push(next);
    }
    assert!(
        TeamRunSlots::hydrate(world.lease(), &world.snapshot, &chain, &[]).is_err(),
        "a chain past the declared successor depth must fail closed"
    );

    // A well-formed chain of the same runs hydrates and stays usable.
    let honest = vec![closed.clone(), other_closed];
    let roster = TeamRunSlots::hydrate(world.lease(), &world.snapshot, &honest, &[])
        .expect("a well-formed roster hydrates");
    assert_eq!(roster.attempt_count(&pair[0]), 1);
    assert_eq!(roster.live_run(&pair[0]), None, "the seat closed cleanly");
}

// ---------------------------------------------------------------------------
// AC-6 — team closure over declared seats, through the store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_team_certificate_converts_into_evidence_the_store_recomputes() {
    let world = World::open().await;
    let mut slots = world.slots();

    // Run every declared seat once and close it with real stored evidence.
    let mut ids = Vec::new();
    for (index, declared) in world.team.slots.iter().enumerate() {
        let run = AgentRunId::generate();
        let binding = world.occupy(&mut slots, &declared.id, run, None).await;
        let row = world.close_in_store(
            &binding,
            u64::try_from(index).expect("a small index") * 10 + 5,
        );
        let occupied = slots.occupied(&declared.id).expect("the seat is occupied");
        slots
            .close_completed(occupied, &row)
            .expect("the seat closes on its own evidence");
        ids.push(run);
    }

    let certificate = slots
        .certify_team_closure(&[])
        .expect("every declared seat is accounted for");
    assert_eq!(certificate.outcome(), TerminalOutcome::Succeeded);
    assert_eq!(certificate.children().len(), world.team.slots.len());

    drop(slots);
    let evidence = certificate
        .into_terminal_evidence(at("2026-08-10T11:00:00Z"))
        .expect("the certificate converts");
    // The digest is the existing core one, so the store recomputes it unchanged.
    evidence
        .verify_children(world.team_run_id, certificate.children())
        .expect("the core child-evidence digest matches");

    // Hydrating the same rows out of storage certifies to the same policy.
    let rows = world.stored_runs(&ids);
    let rehydrated = TeamRunSlots::hydrate(world.lease(), &world.snapshot, &rows, &[])
        .expect("the closed roster hydrates")
        .certify_team_closure(&[])
        .expect("and certifies");
    assert_eq!(rehydrated.policy_digest(), certificate.policy_digest());
}

#[tokio::test]
async fn omitting_one_declared_seat_refuses_team_closure() {
    let world = World::open().await;
    let mut slots = world.slots();

    let mut ids = Vec::new();
    for (index, declared) in world.team.slots.iter().enumerate().skip(1) {
        let run = AgentRunId::generate();
        let binding = world.occupy(&mut slots, &declared.id, run, None).await;
        let row = world.close_in_store(
            &binding,
            u64::try_from(index).expect("a small index") * 10 + 5,
        );
        let occupied = slots.occupied(&declared.id).expect("the seat is occupied");
        slots
            .close_completed(occupied, &row)
            .expect("the seat closes");
        ids.push(run);
    }

    assert!(
        slots.certify_team_closure(&[]).is_err(),
        "a declared seat that never ran must refuse closure"
    );
    drop(slots);

    let rows = world.stored_runs(&ids);
    assert!(
        TeamRunSlots::hydrate(world.lease(), &world.snapshot, &rows, &[])
            .expect("the partial roster hydrates")
            .certify_team_closure(&[])
            .is_err(),
        "and the same hole refuses it after a restart"
    );
}

/// Finding 4 regression: an open seat keeps the task off a terminal state.
///
/// Every phase, gate and artifact the profile declares is satisfied here. The
/// only thing outstanding is one role slot that still holds a live native
/// session — and that alone must keep the task from closing.
#[tokio::test]
async fn a_task_cannot_close_while_a_declared_role_slot_is_still_open() {
    let world = World::open().await;
    let mut slots = world.slots();
    let profile = &world.bundle.profile.definition;

    // Satisfy the profile completely.
    let phases: BTreeSet<_> = profile
        .phases
        .iter()
        .map(|phase| phase.id.clone())
        .collect();
    let gates: std::collections::BTreeMap<_, _> = profile
        .gates
        .iter()
        .map(|gate| (gate.id.clone(), kontor_core::state::GateState::Passed))
        .collect();
    let artifacts: BTreeSet<_> = profile
        .artifacts
        .iter()
        .map(|contract| contract.key.clone())
        .collect();

    // Close every seat but one; leave the last one holding a live session.
    let (open_seat, closed_seats) = world
        .team
        .slots
        .split_last()
        .expect("the team declares at least one seat");
    for (index, declared) in closed_seats.iter().enumerate() {
        let run = AgentRunId::generate();
        let binding = world.occupy(&mut slots, &declared.id, run, None).await;
        let row = world.close_in_store(&binding, u64::try_from(index).expect("small") * 10 + 5);
        let occupied = slots.occupied(&declared.id).expect("the seat is occupied");
        slots
            .close_completed(occupied, &row)
            .expect("the seat closes");
    }
    world
        .occupy(&mut slots, &open_seat.id, AgentRunId::generate(), None)
        .await;
    assert!(
        slots.live_run(&open_seat.id).is_some(),
        "one seat is still running"
    );

    // The store seam refuses it too — and that is the part a caller cannot
    // route around by skipping the profiles facade. Everything the profile asks
    // for is recorded; the one live seat is the whole reason this must fail.
    let workflow = world.freeze_workflow();
    world.pass_every_gate(workflow);
    let task = world
        .store
        .get_task(world.project, world.task)
        .expect("the read succeeds")
        .expect("the task exists");
    let task = world
        .store
        .transition_task(&world.terminal_request(
            TaskState::InProgress,
            task.revision,
            TaskTeamClosure::NoTeam,
            &phases,
            &artifacts,
        ))
        .expect("the task starts");

    // Citing no team at all, for a profile that pins one.
    assert!(
        world
            .store
            .transition_task(&world.terminal_request(
                TaskState::Done,
                task.revision,
                TaskTeamClosure::NoTeam,
                &phases,
                &artifacts,
            ))
            .is_err(),
        "the store must not close a team-bearing task with no team citation"
    );

    // Citing the real team run, which is still open at one seat.
    assert!(
        world
            .store
            .transition_task(&world.terminal_request(
                TaskState::Done,
                task.revision,
                TaskTeamClosure::Certified {
                    team_run_id: world.team_run_id,
                    policy_digest: ContentHash::of(b"claimed"),
                },
                &phases,
                &artifacts,
            ))
            .is_err(),
        "the store must not close a task whose team run has a live role slot"
    );

    // No team certificate is obtainable either...
    assert!(
        slots.certify_team_closure(&[]).is_err(),
        "a live seat has no team closure"
    );
    // ...so the task has no way to certify closure either.
    assert!(
        certify_task_closure(
            &world.bundle.profile,
            TaskTeamEvidence::NoTeam,
            &phases,
            &gates,
            &artifacts,
            &[]
        )
        .is_err(),
        "a satisfied profile is not enough while a role slot is open"
    );

    // Close the last seat and both halves line up.
    let occupied = slots.occupied(&open_seat.id).expect("the seat is occupied");
    let run = occupied.agent_run_id();
    let binding = occupied.binding().clone();
    drop(occupied);
    let row = world.close_in_store(&binding, 900);
    assert_eq!(row.id, run);
    let occupied = slots.occupied(&open_seat.id).expect("the seat is occupied");
    slots
        .close_completed(occupied, &row)
        .expect("the last seat closes");

    let certificate = slots
        .certify_team_closure(&[])
        .expect("every declared seat is now accounted for");
    certify_task_closure(
        &world.bundle.profile,
        TaskTeamEvidence::Certified {
            team_run_id: world.team_run_id,
            certificate: &certificate,
        },
        &phases,
        &gates,
        &artifacts,
        &[],
    )
    .expect("profile closure plus team closure closes the task");

    // The store seam agrees, once the team run itself is closed.
    let team = world
        .store
        .get_team_run(world.project, world.team_run_id)
        .expect("the read succeeds")
        .expect("the team run exists");
    world
        .store
        .close_team_run(&kontor_core::repository::TeamRunClosure {
            project_id: world.project,
            team_run_id: world.team_run_id,
            expected_revision: team.revision,
            evidence: certificate
                .into_terminal_evidence(at("2026-08-10T11:00:00Z"))
                .expect("the certificate converts"),
        })
        .expect("the team closes on the certificate it produced");

    let closed = world
        .store
        .transition_task(&world.terminal_request(
            TaskState::Done,
            task.revision,
            certificate.task_team_closure(),
            &phases,
            &artifacts,
        ))
        .expect("a task whose profile and team have both closed may close");
    assert_eq!(closed.state, TaskState::Done);
}

/// The store re-proves a team citation instead of believing it.
///
/// A [`TaskTeamClosure::Certified`] value names a team run; it is not evidence
/// about one. Each case here cites something the caller could plausibly hold
/// while the substance is wrong, and the *rule* that refuses is asserted — so a
/// case cannot pass because some neighbouring check happened to fire.
#[tokio::test]
async fn the_store_re_proves_a_team_citation_against_its_own_rows() {
    let world = World::open().await;
    let mut slots = world.slots();
    let profile = &world.bundle.profile.definition;
    let phases: BTreeSet<_> = profile
        .phases
        .iter()
        .map(|phase| phase.id.clone())
        .collect();
    let artifacts: BTreeSet<_> = profile
        .artifacts
        .iter()
        .map(|contract| contract.key.clone())
        .collect();

    let workflow = world.freeze_workflow();
    world.pass_every_gate(workflow);
    let task = world
        .store
        .get_task(world.project, world.task)
        .expect("the read succeeds")
        .expect("the task exists");
    let task = world
        .store
        .transition_task(&world.terminal_request(
            TaskState::InProgress,
            task.revision,
            TaskTeamClosure::NoTeam,
            &phases,
            &artifacts,
        ))
        .expect("the task starts");

    // Run and close every declared seat, then close the team run on the
    // certificate that proves it.
    for (index, declared) in world.team.slots.iter().enumerate() {
        let run = AgentRunId::generate();
        let binding = world.occupy(&mut slots, &declared.id, run, None).await;
        let row = world.close_in_store(&binding, u64::try_from(index).expect("small") * 10 + 5);
        let occupied = slots.occupied(&declared.id).expect("the seat is occupied");
        slots
            .close_completed(occupied, &row)
            .expect("the seat closes");
    }
    let certificate = slots
        .certify_team_closure(&[])
        .expect("every declared seat is accounted for");
    let team = world
        .store
        .get_team_run(world.project, world.team_run_id)
        .expect("the read succeeds")
        .expect("the team run exists");
    world
        .store
        .close_team_run(&kontor_core::repository::TeamRunClosure {
            project_id: world.project,
            team_run_id: world.team_run_id,
            expected_revision: team.revision,
            evidence: certificate
                .into_terminal_evidence(at("2026-08-10T11:00:00Z"))
                .expect("the certificate converts"),
        })
        .expect("the team closes");

    let cite = |team_run_id| TaskTeamClosure::Certified {
        team_run_id,
        policy_digest: certificate.policy_digest().clone(),
    };
    let refusal = |team_run_id| -> &'static str {
        match world.store.transition_task(&world.terminal_request(
            TaskState::Done,
            task.revision,
            cite(team_run_id),
            &phases,
            &artifacts,
        )) {
            Err(kontor_core::repository::RepositoryError::Domain(
                kontor_core::DomainError::MissingEvidence { rule, .. },
            )) => rule,
            other => panic!("expected a missing-evidence refusal, got {other:?}"),
        }
    };

    // A team run this project has never stored.
    assert_eq!(
        refusal(TeamRunId::generate()),
        "the cited team run is not stored in this project"
    );

    // A real team run of a *different* task in the same project.
    let other_task = TaskId::generate();
    world
        .store
        .create_task(&NewTask {
            id: other_task,
            project_id: world.project,
            mini_project_id: None,
            title: name("Another task"),
            module: None,
            state: kontor_core::state::TaskState::Ready,
            created_at: now(),
        })
        .expect("a second task is created");
    let other_team = TeamRunId::generate();
    world
        .store
        .create_team_run(&NewTeamRun {
            id: other_team,
            project_id: world.project,
            task_id: other_task,
            snapshot: world.snapshot.clone(),
            created_at: now(),
        })
        .expect("a second team run is created");
    assert_eq!(
        refusal(other_team),
        "the cited team run serves a different task",
        "the task the team served is checked before anything else about it"
    );

    // A closed team run that has since gained a live run: closure was true when
    // it was recorded, and is not true now.
    world
        .store
        .create_agent_run(&kontor_core::repository::NewAgentRun {
            id: AgentRunId::generate(),
            project_id: world.project,
            team_run_id: world.team_run_id,
            parent_agent_run_id: None,
            role: world.team.slots[0].id.as_role_key().clone(),
            account_profile_id: None,
            binding: None,
            created_at: now(),
        })
        .expect("the raw port admits a run into a closed team");
    assert_eq!(
        refusal(world.team_run_id),
        "a role slot of the cited team run is still open",
        "a team that closed and then reopened a seat is not closed now"
    );
}

// ---------------------------------------------------------------------------
// AC-1 — publishing v2 rewrites nothing about v1
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publishing_a_second_revision_leaves_every_v1_row_and_snapshot_alone() {
    let world = World::open().await;

    // The task pins v1 of the profile, and a persona freezes onto that pin.
    let workflow = TaskWorkflowId::generate();
    world
        .store
        .create_task_workflow(&NewTaskWorkflow {
            id: workflow,
            project_id: world.project,
            task_id: world.task,
            snapshot: world.bundle.profile.clone(),
            current_phase: world.bundle.profile.definition.entry_phase.clone(),
            created_at: now(),
        })
        .expect("the workflow freezes v1 onto the task");

    let pack = bundled_pack().expect("the bundled pack loads");
    let persona = persona_for(&pack, &world.bundle.profile.definition);
    world
        .store
        .insert_persona_scenario(world.project, &persona)
        .expect("the scenario revision is stored");
    let frozen = world
        .store
        .create_task_persona_snapshot(&NewTaskPersonaSnapshot {
            project_id: world.project,
            task_id: world.task,
            workflow_id: workflow,
            scenario_id: persona.scenario_id,
            version: persona.version,
            created_at: now(),
        })
        .expect("the scenario freezes onto the task");
    let frozen_hash = frozen.definition_hash.clone();

    // A duplicate v1, in any of the three families, is refused.
    assert!(
        world
            .store
            .insert_work_profile(world.project, &world.bundle.profile.definition)
            .is_err(),
        "a work profile revision is immutable"
    );
    assert!(
        world
            .store
            .insert_team_template(
                world.project,
                &world.team.to_revision().expect("it canonicalizes")
            )
            .is_err(),
        "a team template revision is immutable"
    );
    assert!(
        world
            .store
            .insert_persona_scenario(world.project, &persona)
            .is_err(),
        "a persona scenario revision is immutable"
    );

    // Publish v2 of all three.
    let profile_v2 = revise_work_profile(&world.bundle.profile.definition, |profile| {
        profile.budget_defaults.max_tokens += 1;
    })
    .expect("the profile revises");
    let team_v2 = revise_team_template(&world.team, |team| {
        team.max_handoff_depth = team.max_handoff_depth.saturating_sub(1).max(1);
    })
    .expect("the team revises");
    let persona_v2 = revise_persona_scenario(&persona, |scenario| {
        scenario.prohibited_actions.push(name("Also never do this"));
    })
    .expect("the scenario revises");

    world
        .store
        .insert_work_profile(world.project, &profile_v2)
        .expect("v2 of the profile is stored");
    world
        .store
        .insert_team_template(
            world.project,
            &team_v2.to_revision().expect("it canonicalizes"),
        )
        .expect("v2 of the team is stored");
    world
        .store
        .insert_persona_scenario(world.project, &persona_v2)
        .expect("v2 of the scenario is stored");

    // Reopen the file and prove v1 is byte-identical and still pinned.
    drop(world.store);
    let reopened = SqliteStore::open(&world.path).expect("the store reopens");

    let stored_v1 = reopened
        .get_work_profile(
            world.project,
            &world.bundle.profile.definition.id,
            SpecVersion::FIRST,
        )
        .expect("the read succeeds")
        .expect("v1 is still there");
    assert_eq!(
        &stored_v1, &world.bundle.profile.definition,
        "v1 of the profile is unchanged"
    );
    assert_eq!(
        stored_v1.canonicalize().expect("it canonicalizes").hash(),
        &world.bundle.profile.definition_hash,
        "and still hashes to what the task pinned"
    );

    let stored_team_v1 = reopened
        .get_team_template(world.project, world.team.template_id, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("v1 is still there");
    assert_eq!(
        stored_team_v1.definition.hash(),
        world.snapshot.definition.hash(),
        "v1 of the team is unchanged"
    );
    assert_eq!(
        TeamTemplateSpec::from_revision(&stored_team_v1).expect("it reads back"),
        world.team
    );

    let stored_persona_v1 = reopened
        .get_persona_scenario(world.project, persona.scenario_id, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("v1 is still there");
    assert_eq!(
        &stored_persona_v1, &persona,
        "v1 of the scenario is unchanged"
    );

    let workflow_after = reopened
        .get_active_task_workflow(world.project, world.task)
        .expect("the read succeeds")
        .expect("the workflow exists");
    assert_eq!(
        workflow_after.snapshot.definition_hash, world.bundle.profile.definition_hash,
        "the task's pinned profile snapshot did not move to v2"
    );
    workflow_after
        .snapshot
        .verify()
        .expect("the pinned snapshot still verifies");

    let persona_after = reopened
        .get_task_persona_snapshot(
            world.project,
            world.task,
            persona.scenario_id,
            SpecVersion::FIRST,
        )
        .expect("the read succeeds")
        .expect("the frozen scenario exists");
    assert_eq!(
        persona_after.definition_hash, frozen_hash,
        "the task's frozen persona snapshot did not move to v2"
    );

    let team_run = reopened
        .get_team_run(world.project, world.team_run_id)
        .expect("the read succeeds")
        .expect("the team run exists");
    assert_eq!(
        team_run.snapshot.template_version,
        SpecVersion::FIRST,
        "the team run still runs the revision it started on"
    );
    assert_eq!(
        team_run.snapshot.definition.hash(),
        world.snapshot.definition.hash()
    );
}

/// The bundled persona bound to `profile`, or a scenario built for one of its
/// gates when the pack ships none for that profile.
fn persona_for(pack: &ProfilePackSpec, profile: &WorkProfileSpec) -> PersonaScenarioSpec {
    if let Some(bound) = pack
        .personas
        .iter()
        .find(|persona| persona.profile == profile.id && persona.profile_version == profile.version)
    {
        return bound.scenario.clone();
    }

    // Build one structurally: any gate, an actor that holds no authority over
    // it, and the gate's own evaluators as the independent verifiers.
    let gate = profile
        .gates
        .first()
        .expect("a validated profile declares a gate");
    let evidence: Vec<ArtifactKey> = if gate.required_evidence.is_empty() {
        vec![
            profile
                .artifacts
                .first()
                .expect("a validated profile declares an artifact")
                .key
                .clone(),
        ]
    } else {
        gate.required_evidence.clone()
    };
    let declared: BTreeSet<&kontor_core::id::RoleKey> = gate
        .evaluator_roles
        .iter()
        .chain(gate.waiver_roles.iter())
        .collect();
    let actor = ["zz.actor-a", "zz.actor-b"]
        .into_iter()
        .map(|text| kontor_core::id::RoleKey::parse(text).expect("a role key"))
        .find(|candidate| !declared.contains(candidate))
        .expect("an unauthorized actor role exists");

    PersonaScenarioSpec {
        schema_version: SCHEMA_VERSION,
        scenario_id: kontor_core::id::PersonaScenarioId::generate(),
        version: SpecVersion::FIRST,
        persona: kontor_core::id::PersonaKey::parse("zz.simulated").expect("a persona key"),
        characteristics: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "notes": ["A simulated actor built for this contract"]
        }))
        .expect("a canonical document"),
        identity: kontor_core::spec::TestIdentityRef {
            reference: name("seeded-contract-actor"),
            seeded: true,
        },
        environment: kontor_core::spec::EnvironmentRef {
            kind: kontor_core::spec::EnvironmentKind::Sandbox,
            reference: name("contract-sandbox"),
        },
        steps: vec![kontor_core::spec::ScenarioStep {
            order: 1,
            instruction: name("Exercise the gate under test"),
            expected_evidence: evidence.clone(),
        }],
        prohibited_actions: vec![name("Leave the sandbox")],
        required_evidence: evidence,
        gate_under_test: gate.id.clone(),
        actor_role: actor,
        evaluator_roles: gate.evaluator_roles.clone(),
    }
}

// ---------------------------------------------------------------------------
// The resolved bundle survives the round trip through storage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_resolved_bundle_matches_what_the_store_reads_back() {
    let world = World::open().await;
    world.bundle.verify().expect("the bundle verifies");

    let stored = world
        .store
        .get_work_profile(
            world.project,
            &world.bundle.profile.definition.id,
            world.bundle.profile.definition.version,
        )
        .expect("the read succeeds")
        .expect("the profile is stored");
    let round_tripped = ResolvedWorkProfileSnapshot::resolve(&stored, now())
        .expect("the stored definition resolves");
    assert_eq!(
        round_tripped.definition_hash, world.bundle.profile.definition_hash,
        "resolution is deterministic across storage"
    );

    let revision = world.team.to_revision().expect("it canonicalizes");
    let stored_team = world
        .store
        .get_team_template(world.project, world.team.template_id, world.team.version)
        .expect("the read succeeds")
        .expect("the team is stored");
    assert_eq!(stored_team.definition.hash(), revision.definition.hash());
    assert_eq!(stored_team.role_authority, world.team.role_authority());
    assert_eq!(
        stored_team.role_authority, world.snapshot.role_authority,
        "the run snapshot froze the same authority the template derives"
    );

    // Every seat the run can fill is a seat the pinned snapshot declares.
    let slots = world.slots();
    for declared in &slots.template().slots {
        assert!(
            RoleSlotId::parse(declared.id.as_str()).is_ok(),
            "every declared seat is addressable as a stored role"
        );
        assert_eq!(slots.attempt_count(&declared.id), 0, "and starts empty");
    }
    assert_eq!(
        slots.template().slots.len(),
        world.team.slots.len(),
        "the roster is the template's, not whatever ran"
    );
    assert_eq!(
        RunLifecycle::Queued.terminal_outcome(),
        None,
        "a queued attempt is never a closed one"
    );
}
