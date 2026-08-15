//! Scheduling against the real store, a bundled profile pack and a real adapter.
//!
//! The pass in `kontor-scheduler` is proved pure by its own suite and the
//! transaction in `kontor-store` by its own. This one proves the seam: that a
//! decision made from what the store actually holds commits as one durable
//! admission, that the runtime is never contacted while it commits, and that a
//! dispatch whose result nobody knows does not become a second admission — in
//! this process or after a restart.
//!
//! The work profile and team template come out of the **bundled seed pack**, and
//! nothing here tells the scheduler which pack it is. That is the point: the pass
//! is asserted to have no branch on a profile id (`no_seed_branching.rs`), and
//! this suite runs it against a seeded profile to show the claim holds where it
//! would matter.
//!
//! The mutants this suite exists to kill:
//!
//! * an admission that contacts a runtime inside its transaction, so a write lock
//!   is held across a native call;
//! * a lost launch acknowledgement that produces a second run, a second lease or
//!   a second outbox entry when the scheduler tries again;
//! * a restart that re-admits work it already admitted, because the evidence it
//!   would have found lives only in memory;
//! * a lease that survives its own admission being replayed, or is taken twice
//!   for one place.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, BoundedText, CanonicalDocument,
    CommandReceiptId, CredentialAlias, CurrencyCode, ExecutionAuthorizationId, ExternalId,
    ExternalName, IdempotencyKey, ModuleKey, Money, ProjectId, ResourceLeaseId, RuntimeBindingId,
    RuntimeKindKey, SCHEMA_VERSION, TaskId, TaskWorkflowId, TeamRunId, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CalendarRepository, CommandRepository, CredentialReference, CredentialReferenceKind,
    NewAccountProfile, NewAgentRun, NewCommandIntent, NewProject, NewTask, NewTeamRun,
    ProjectRepository, RunRepository, SpecRepository,
};
use kontor_core::spec::{BudgetBounds, TeamRunSnapshot};
use kontor_core::state::{DesiredRunState, RunLifecycle, TaskState};
use kontor_profiles::pack::{
    PackAvailability, ProfilePackSpec, ResolvedProfileBundle, resolve_profile,
};
use kontor_profiles::seeds::bundled_pack;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{AdapterCall, RequestKey, ScriptStep, ScriptedFakeRuntime};
use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_scheduler::{
    AccountAdmissionEvidence, AccountPin, AdaptiveWindow, AdaptiveWindowConfig, AdmissionEventId,
    AdmittedCandidate, AuthorizationEvidence, CalendarAdmission, Candidate, CapacityConfig,
    CapacityUsage, ExternalWorkEvidence, FleetPreflight, PreflightOutcome, ReconciliationEvidence,
    ReconciliationScope, RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin,
    minimum_launch_capabilities, plan,
};
use kontor_store::{AdmissionCommit, SqliteStore};
use kontor_teams::run::{SlotLaunch, TeamRunLease, TeamRunSlots};
use kontor_teams::spec::{RoleSlotId, TeamTemplateSpec};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC fixture timestamp")
}

fn now() -> Timestamp {
    at("2026-08-12T09:00:00Z")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4_096,
            max_history_page: 64,
            max_concurrent_sessions: 16,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

/// The first seeded bundle that pins a team, so the roster has a seat to fill.
fn seeded_bundle(pack: &ProfilePackSpec) -> ResolvedProfileBundle {
    for entry in &pack.manifest {
        if entry.availability != PackAvailability::Seeded {
            continue;
        }
        let Ok(bundle) = resolve_profile(pack, &entry.category, now()) else {
            continue;
        };
        if bundle.team.is_some() {
            return bundle;
        }
    }
    panic!("a bundled profile pins a team template");
}

struct World {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    fake: ScriptedFakeRuntime,
    project: ProjectId,
    task: TaskId,
    account: AccountProfileId,
    authorization: ExecutionAuthorizationId,
    snapshot: TeamRunSnapshot,
    slot: RoleSlotId,
    workspace: WorkspaceBindingSnapshot,
    team_run: TeamRunId,
    workflow: TaskWorkflowId,
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
                name: name("Scheduling contract"),
                root_path: name("/tmp/kontor-scheduling-contract"),
                created_at: now(),
            })
            .expect("a project is created");

        let task = TaskId::generate();
        store
            .create_task(&NewTask {
                id: task,
                project_id: project,
                mini_project_id: None,
                title: name("A schedulable task"),
                module: Some(ModuleKey::parse("directory.app").expect("a valid module key")),
                state: TaskState::Ready,
                created_at: now(),
            })
            .expect("a task is created");

        // The runtime family is the fake's own: routing is pinned before the
        // scheduler sees it, and the account has to authenticate against the same
        // family the launch is routed to.
        let harness_kind = RuntimeKindKey::parse("fake.runtime").expect("a valid runtime key");
        let account = AccountProfileId::generate();
        store
            .create_account_profile(&NewAccountProfile {
                id: account,
                project_id: project,
                label: name("Scheduling account"),
                external_account_id: None,
                harness: harness_kind,
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: CredentialAlias::parse("sc-alpha").expect("a valid alias"),
                },
                environment: document("environment"),
                routing: document("routing"),
                capability: document("capability"),
                provider_identity: None,
                enabled: true,
                created_at: now(),
            })
            .expect("an account profile is created");

        // A bundled seed profile and its team, stored through the real store.
        let pack = bundled_pack().expect("the bundled pack loads");
        let bundle = seeded_bundle(&pack);
        let revision = bundle.team.clone().expect("the profile pinned a team");
        let team = TeamTemplateSpec::from_revision(&revision).expect("the team reads back");
        let slot = team
            .slots
            .first()
            .expect("a team has at least one seat")
            .id
            .clone();
        store
            .insert_work_profile(project, &bundle.profile.definition)
            .expect("the profile revision is stored");
        store
            .insert_team_template(project, &revision)
            .expect("the team revision is stored");
        let snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION);

        let capability_receipt = CommandReceiptId::generate();
        store
            .record_intent(&NewCommandIntent {
                project_id: project,
                receipt_id: capability_receipt,
                idempotency_key: IdempotencyKey::parse("scheduling-authorize")
                    .expect("a valid key"),
                kind: CommandKind::AuthorizeExecution,
                target: AggregateRef::Project {
                    project_id: project,
                },
                target_revision: AggregateRevision::INITIAL,
                intent: document("authorize-intent"),
                payload: document("authorize-payload"),
                desired: None,
                not_before: now(),
                created_at: now(),
            })
            .expect("the capability receipt is recorded");
        let authorization = ExecutionAuthorizationId::generate();
        store
            .insert_authorization(&ExecutionAuthorization {
                id: authorization,
                project_id: project,
                scope: WorkScope::Project,
                selected_tasks: Vec::new(),
                allowed_start: TimeRange {
                    start: at("2026-08-12T00:00:00Z"),
                    end: at("2026-08-13T00:00:00Z"),
                },
                max_concurrency: 8,
                budget: BudgetBounds {
                    max_tokens: 1_000,
                    max_commands: 10,
                    max_duration_seconds: 600,
                    max_cost: Money {
                        minor_units: 100,
                        currency: CurrencyCode::parse("NOK").expect("a valid currency"),
                    },
                },
                created_by: account,
                capability_receipt,
                created_at: now(),
            })
            .expect("the authorization is stored");

        let fake = ScriptedFakeRuntime::new(capabilities());
        let workspace = fake
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id: TeamRunId::generate(),
                task_id: task,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: WorkspaceRoot::parse("/w/scheduling-task").expect("an absolute path"),
                requested_at: at("2026-08-12T08:59:00Z"),
            })
            .await
            .expect("the workspace is prepared")
            .snapshot;
        fake.take_calls();

        Self {
            _directory: directory,
            path,
            store,
            fake,
            project,
            task,
            account,
            authorization,
            snapshot,
            slot,
            workspace,
            team_run: TeamRunId::generate(),
            workflow: TaskWorkflowId::generate(),
        }
    }

    /// The candidate, built from what the store actually holds right now.
    fn candidate(&self, taken_at: Timestamp) -> Candidate {
        let harness_kind = RuntimeKindKey::parse("fake.runtime").expect("a valid runtime key");
        Candidate {
            project_id: self.project,
            task_id: self.task,
            mini_project_id: None,
            workflow_id: self.workflow,
            state: TaskState::Ready,
            revision: AggregateRevision::INITIAL,
            created_at: now(),
            priority: 500,
            module: Some(ModuleKey::parse("directory.app").expect("a valid module key")),
            worktree: None,
            depends_on: BTreeSet::new(),
            serializes_with: BTreeSet::new(),
            origin: TaskOrigin::Manual,
            authorization: Some(AuthorizationEvidence {
                id: self.authorization,
                project_id: self.project,
                scope: WorkScope::Project,
                selected_tasks: BTreeSet::new(),
                allowed_start: at("2026-08-12T00:00:00Z"),
                allowed_end: at("2026-08-13T00:00:00Z"),
                max_concurrency: 8,
            }),
            calendar: CalendarAdmission::unrestricted(),
            runtime: RuntimeAdmissionEvidence {
                runtime_kind: harness_kind.clone(),
                host: name("fake-host"),
                generation: self.fake.generation(),
                capabilities: self.fake.capabilities(),
                required: minimum_launch_capabilities(),
                health: RuntimeHealth::Healthy,
                reconciliation: ReconciliationEvidence {
                    epoch_completed: true,
                    scope: ReconciliationScope {
                        project_id: self.project,
                        runtime_kind: harness_kind.clone(),
                        host: name("fake-host"),
                        generation: self.fake.generation(),
                    },
                    open_replay_gap: false,
                    divergence: false,
                    orphan_ambiguity: false,
                    stale_lost_contact: false,
                },
                last_confirmed_at: Some(taken_at),
            },
            account: AccountAdmissionEvidence {
                pin: Some(AccountPin {
                    account_profile_id: self.account,
                    pinned_revision: AggregateRevision::INITIAL,
                    current_revision: AggregateRevision::INITIAL,
                    enabled: true,
                    cooldown_until: None,
                    harness: harness_kind,
                    declared_capabilities: BTreeSet::new(),
                    provider_identity: None,
                    preflight: FleetPreflight {
                        outcome: PreflightOutcome::Passed,
                        evidence_hash: document("preflight").hash().clone(),
                        observed_at: taken_at,
                    },
                }),
                required_capabilities: BTreeSet::new(),
            },
            external: ExternalWorkEvidence::default(),
        }
    }

    /// A snapshot whose lease and capacity inputs come from the store, not from a
    /// literal.
    fn scheduling_snapshot(&self, store: &SqliteStore, taken_at: Timestamp) -> SchedulingSnapshot {
        SchedulingSnapshot {
            schema_version: SCHEMA_VERSION,
            taken_at,
            candidates: vec![self.candidate(taken_at)],
            in_flight_tasks: store
                .tasks_with_open_runs()
                .expect("the in-flight tasks are readable"),
            completed_tasks: BTreeSet::new(),
            module_leases: store
                .active_module_claims(taken_at)
                .expect("the module claims are readable"),
            worktree_leases: store
                .active_worktree_leases(taken_at)
                .expect("the worktree leases are readable"),
            usage: CapacityUsage {
                global_in_flight: 0,
                project_in_flight: BTreeMap::new(),
                mission_in_flight: BTreeMap::new(),
                account_in_flight: BTreeMap::new(),
                provider_in_flight: BTreeMap::new(),
                runtime_in_flight: BTreeMap::new(),
            },
            capacity: CapacityConfig {
                global_max_in_flight: 8,
                project_max_in_flight: 8,
                mission_max_in_flight: 8,
                account_max_in_flight: 8,
                provider_max_in_flight: 8,
                runtime_max_in_flight: 8,
                adaptive: AdaptiveWindowConfig {
                    initial: 4,
                    floor: 2,
                    ceiling: 7,
                    growth_step: 1,
                },
            },
            adaptive_window: AdaptiveWindow::start(AdaptiveWindowConfig {
                initial: 4,
                floor: 2,
                ceiling: 7,
                growth_step: 1,
            }),
            freshness: jiff::SignedDuration::from_secs(120),
        }
    }

    /// The commit request for one admitted decision. Every id is fixed by `label`
    /// only through the launch key, so a replay re-sends the identical request.
    fn commit<'a>(
        &self,
        admitted: &'a AdmittedCandidate,
        peers: &'a BTreeSet<TaskId>,
        parts: &Parts,
        decided_at: Timestamp,
    ) -> AdmissionCommit<'a> {
        AdmissionCommit {
            admitted,
            serializes_with: peers,
            capacity: CapacityConfig {
                global_max_in_flight: 8,
                project_max_in_flight: 8,
                mission_max_in_flight: 8,
                account_max_in_flight: 8,
                provider_max_in_flight: 8,
                runtime_max_in_flight: 8,
                adaptive: AdaptiveWindowConfig {
                    initial: 4,
                    floor: 2,
                    ceiling: 7,
                    growth_step: 1,
                },
            },
            team_run: NewTeamRun {
                id: self.team_run,
                project_id: self.project,
                task_id: self.task,
                snapshot: self.snapshot.clone(),
                created_at: decided_at,
            },
            agent_run: NewAgentRun {
                id: parts.agent_run,
                project_id: self.project,
                team_run_id: self.team_run,
                parent_agent_run_id: None,
                role: self.slot.as_role_key().clone(),
                account_profile_id: Some(self.account),
                binding: None,
                created_at: decided_at,
            },
            launch: NewCommandIntent {
                project_id: self.project,
                receipt_id: parts.receipt,
                idempotency_key: parts.launch_key.clone(),
                kind: CommandKind::LaunchRun,
                target: AggregateRef::AgentRun {
                    agent_run_id: parts.agent_run,
                },
                target_revision: AggregateRevision::INITIAL,
                intent: document("launch-intent"),
                payload: document("launch-payload"),
                desired: Some(DesiredRunState::RunRequested),
                not_before: decided_at,
                created_at: decided_at,
            },
            admission_event_id: parts.admission,
            module_lease_id: Some(parts.module_lease),
            worktree_lease_id: None,
            holder_instance: ExternalId::parse("scheduler-instance-a").expect("a valid holder"),
            lease_expires_at: decided_at + jiff::SignedDuration::from_secs(300),
            evidence: document("admission-evidence"),
            decided_at,
        }
    }

    /// Restart the control plane on the same file.
    ///
    /// The old connection is dropped as the new one replaces it, so anything a
    /// replay finds afterwards was read from disk rather than remembered.
    fn restart(&mut self) {
        self.store = SqliteStore::open(&self.path).expect("the store reopens");
    }

    fn launch_calls(&self) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::Launch(_)))
            .count()
    }

    fn rows(&self, sql: &str) -> i64 {
        let connection = rusqlite::Connection::open(&self.path).expect("a raw connection opens");
        connection
            .query_row(sql, [], |row| row.get(0))
            .expect("the count is readable")
    }
}

/// The ids one admission mints. A retry re-sends exactly these.
struct Parts {
    agent_run: AgentRunId,
    receipt: CommandReceiptId,
    launch_key: IdempotencyKey,
    admission: AdmissionEventId,
    module_lease: ResourceLeaseId,
}

impl Parts {
    fn new() -> Self {
        Self {
            agent_run: AgentRunId::generate(),
            receipt: CommandReceiptId::generate(),
            launch_key: IdempotencyKey::parse("scheduling-launch").expect("a valid key"),
            admission: AdmissionEventId::generate(),
            module_lease: ResourceLeaseId::generate(),
        }
    }
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_lost_launch_and_a_restart_still_leave_exactly_one_durable_admission() {
    let mut world = World::open().await;
    let peers = BTreeSet::new();
    let parts = Parts::new();

    // 1. Decide from what the store holds. The profile is a bundled seed one and
    //    the pass has no idea which.
    let decided = plan(&world.scheduling_snapshot(&world.store, now())).expect("the pass runs");
    assert_eq!(decided.admitted_count(), 1);
    let admitted = decided.batch().next().expect("one admission").clone();
    assert_eq!(admitted.task_id, world.task);

    // 2. Commit it. Nothing has touched the runtime.
    let first = world
        .store
        .admit_candidate(&world.commit(&admitted, &peers, &parts, now()))
        .expect("the admission commits");
    assert!(!first.replayed);
    assert!(
        world.fake.calls().is_empty(),
        "an admission transaction must not contact a runtime"
    );

    let run = world
        .store
        .get_agent_run(world.project, parts.agent_run)
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(run.projection.lifecycle, RunLifecycle::Queued);

    // 3. The dispatcher claims the launch — writing its durable correlation before
    //    any native call — and the launch then fails at the transport. Nobody
    //    knows whether a session started.
    let claims = world
        .store
        .claim_due(world.project, now(), 10)
        .expect("the outbox is claimable");
    assert!(claims.iter().any(|claim| claim.receipt_id == parts.receipt));

    world.fake.push_step_for(
        ScriptStep::TransportFailure {
            operation: RuntimeCapability::Launch,
        },
        RequestKey::Run(parts.agent_run),
    );
    let lease = TeamRunLease::acquire(world.team_run).expect("this test is the only manager");
    let mut slots = TeamRunSlots::open(lease, &world.snapshot).expect("the seats open");
    let permit = slots
        .reserve(&world.slot, parts.agent_run)
        .expect("a vacant seat reserves");
    let launch = SlotLaunch {
        task_id: world.task,
        binding_id: RuntimeBindingId::generate(),
        workspace: Some(world.workspace.clone()),
        cwd: world.workspace.root().clone(),
        account_profile_id: Some(world.account),
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
        requested_at: now(),
    };
    let authority = world
        .fake
        .admit_launch(&permit.admission_request(&launch))
        .await
        .expect("the runtime admits this seat")
        .into_authority()
        .expect("a vacant seat is admitted rather than resumed");
    let prepared = permit.launch_request(authority, launch);
    let outcome = world.fake.launch(prepared.request()).await;
    assert!(
        outcome.is_err(),
        "the scripted transport failure leaves the result unknown"
    );
    // The fake records a launch only once it is past the transport, so nothing
    // reached the runtime — which is exactly the uncertainty this test is about.
    assert_eq!(world.launch_calls(), 0);

    // The dispatch cannot simply be repeated: the receipt has left
    // `intent_persisted`, so the outbox will not hand the work out again. Whether
    // it may be sent a second time is a recovery question answered with evidence,
    // never by a lease that happened to expire.
    assert!(
        world
            .store
            .claim_due(world.project, at("2026-08-12T09:00:30Z"), 10)
            .expect("the outbox is readable")
            .iter()
            .all(|claim| claim.receipt_id != parts.receipt),
        "a claimed launch is not re-claimable"
    );

    // 4. The scheduler wakes up and re-sends the same admission, because from its
    //    side nothing was ever confirmed.
    let second = world
        .store
        .admit_candidate(&world.commit(&admitted, &peers, &parts, at("2026-08-12T09:01:00Z")))
        .expect("the retry finds the original admission");
    assert!(second.replayed);
    assert_eq!(second.admission_event_id, first.admission_event_id);
    assert_eq!(second.receipt.id, first.receipt.id);
    assert_eq!(second.module_lease_id, first.module_lease_id);

    // 5. And again after a restart, from a store reopened on the same file. The
    //    evidence a replay needs is on disk, not in this process.
    world.restart();
    let third = world
        .store
        .admit_candidate(&world.commit(&admitted, &peers, &parts, at("2026-08-12T09:02:00Z")))
        .expect("a restarted scheduler finds the original admission");
    assert!(third.replayed);
    assert_eq!(third.admission_event_id, first.admission_event_id);

    // One of everything, and the runtime was asked to launch exactly once.
    assert_eq!(world.rows("SELECT count(*) FROM agent_runs"), 1);
    assert_eq!(world.rows("SELECT count(*) FROM team_runs"), 1);
    assert_eq!(world.rows("SELECT count(*) FROM resource_leases"), 1);
    assert_eq!(
        world.rows("SELECT count(*) FROM scheduler_admission_events WHERE decision = 'admitted'"),
        1
    );
    assert_eq!(
        world.rows("SELECT count(*) FROM command_receipts WHERE kind = 'launch_run'"),
        1,
        "one launch command, however many times the admission was re-sent"
    );
    assert_eq!(world.launch_calls(), 0);
}

#[tokio::test]
async fn the_scheduler_admits_a_bundled_profile_and_its_lease_stops_the_next_contender() {
    let world = World::open().await;
    let peers = BTreeSet::new();
    let parts = Parts::new();

    let decided = plan(&world.scheduling_snapshot(&world.store, now())).expect("the pass runs");
    let admitted = decided.batch().next().expect("one admission").clone();
    world
        .store
        .admit_candidate(&world.commit(&admitted, &peers, &parts, now()))
        .expect("the admission commits");

    // The module the bundled task declares is now claimed Realm-wide.
    let claims = world
        .store
        .active_module_claims(now())
        .expect("the claims are readable");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].task_id, world.task);

    // A *different* task contending for the same module. This is the one the lease
    // has to stop: it is not the task that holds the claim, so nothing about its
    // own state refuses it.
    let contender = TaskId::generate();
    world
        .store
        .create_task(&NewTask {
            id: contender,
            project_id: world.project,
            mini_project_id: None,
            title: name("A contending task"),
            module: Some(ModuleKey::parse("directory.app").expect("a valid module key")),
            state: TaskState::Ready,
            created_at: now(),
        })
        .expect("the contending task is created");

    let mut snapshot = world.scheduling_snapshot(&world.store, now());
    let mut second = world.candidate(now());
    second.task_id = contender;
    snapshot.candidates = vec![second];

    let again = plan(&snapshot).expect("the pass runs again");
    assert_eq!(
        again.admitted_count(),
        0,
        "the lease the first pass took is what stops the next contender"
    );
    assert_eq!(
        again
            .decisions
            .first()
            .and_then(kontor_scheduler::CandidateDecision::rejection_code),
        Some(kontor_scheduler::RejectionCode::ModuleInFlight)
    );

    // And the task that already holds the claim is refused for the reason that
    // actually applies to it, rather than for its own lease.
    let mut own = world.scheduling_snapshot(&world.store, now());
    own.candidates = vec![world.candidate(now())];
    assert_eq!(
        plan(&own)
            .expect("the pass runs")
            .decisions
            .first()
            .and_then(kontor_scheduler::CandidateDecision::rejection_code),
        Some(kontor_scheduler::RejectionCode::TaskAlreadyInFlight),
        "a task never contends with itself for its own module"
    );
}
