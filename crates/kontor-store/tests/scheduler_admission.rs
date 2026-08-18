//! Durable admission and lease mechanics, against a real file-backed database.
//!
//! The pass in `kontor-scheduler` is proved pure by its own suite. This one
//! proves what a decision *does*: that a run, a lease, a launch intent and the
//! decision itself become durable together or not at all, that two schedulers
//! cannot both admit one task, and that a lease is a claim on a place rather than
//! a statement about work.
//!
//! Every test uses a file-backed store, because the properties under test are
//! properties of a database with indexes, triggers and a write lock — an
//! `:memory:` database would prove nothing about the exclusion that stops a
//! second admission.
//!
//! The mutants this suite exists to kill:
//!
//! * an admission that writes the run before it proves the lease is free, so a
//!   refused admission leaves a queued run behind;
//! * a lease conflict checked in Rust only, so a caller that bypassed the store's
//!   own function gets the overlap anyway;
//! * project-local module contention, which is the collision the v1 index could
//!   not see;
//! * a replayed admission that queues a second run, a second lease or a second
//!   launch;
//! * an expired lease that concludes something about the run that held it;
//! * a stale holder that can still renew or release after its token advanced;
//! * a capacity ceiling trusted from the snapshot rather than recounted.

use std::collections::BTreeSet;

use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, CommandReceiptId,
    CurrencyCode, ExecutionAuthorizationId, ExternalId, ExternalName, IdempotencyKey,
    MiniProjectId, ModuleKey, Money, ProjectId, ResourceLeaseId, RuntimeKindKey, SCHEMA_VERSION,
    SpecVersion, TaskId, TaskWorkflowId, TeamRunId, TeamTemplateId, Timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CalendarRepository, CommandRepository, CredentialReference, CredentialReferenceKind,
    NewAccountProfile, NewAgentRun, NewCommandIntent, NewMiniProject, NewProject, NewTask,
    NewTeamRun, ProjectRepository, RepositoryError, RunRepository, SpecRepository,
};
use kontor_core::spec::{BudgetBounds, RoleAuthority, TeamRunSnapshot, TeamTemplateRevision};
use kontor_core::state::{DesiredRunState, RunLifecycle, TaskState};
use kontor_scheduler::{
    AdmissionEventId, AdmittedCandidate, CalendarAdmission, CapacityConfig, CapacityLimitKind,
    CapacitySnapshot, OrderingInputs, RejectionCode, RejectionEvidence,
};
use kontor_store::{
    AdmissionCommit, LeaseEventKind, LeaseRelease, LeaseRenewal, RecordedRejection, SqliteStore,
};
use rusqlite::Connection;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC fixture timestamp")
}

fn now() -> Timestamp {
    at("2026-08-12T09:00:00Z")
}

fn later(seconds: i64) -> Timestamp {
    now() + jiff::SignedDuration::from_secs(seconds)
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn module(text: &str) -> ModuleKey {
    ModuleKey::parse(text).expect("a valid module key")
}

fn role(text: &str) -> kontor_core::id::RoleKey {
    kontor_core::id::RoleKey::parse(text).expect("a valid role key")
}

fn runtime_kind() -> RuntimeKindKey {
    RuntimeKindKey::parse("sa.runtime").expect("a valid runtime key")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

/// Ceilings wide enough that only a test that narrows one sees a capacity refusal.
fn wide_capacity() -> CapacityConfig {
    CapacityConfig {
        global_max_in_flight: 50,
        project_max_in_flight: 50,
        mission_max_in_flight: 50,
        account_max_in_flight: 50,
        provider_max_in_flight: 50,
        runtime_max_in_flight: 50,
        adaptive: kontor_scheduler::AdaptiveWindowConfig {
            initial: 50,
            floor: 1,
            ceiling: 50,
            growth_step: 1,
        },
    }
}

struct Harness {
    _directory: TempDir,
    store: SqliteStore,
}

/// One project's worth of the graph an admission needs above it.
struct Scope {
    project: ProjectId,
    mission: MiniProjectId,
    account: AccountProfileId,
    template: TeamTemplateRevision,
    authorization: ExecutionAuthorizationId,
}

impl Harness {
    fn new() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let store =
            SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");
        Self {
            _directory: directory,
            store,
        }
    }

    fn raw(&self) -> Connection {
        let connection = Connection::open(self._directory.path().join("kontor.db"))
            .expect("a raw connection opens");
        connection
            .pragma_update(None, "foreign_keys", true)
            .expect("foreign keys can be enabled");
        connection
    }

    /// A project with a goal, an account, a team template and an authorization
    /// that arms every task in it.
    fn scope(&self, label: &str) -> Scope {
        let project = ProjectId::generate();
        self.store
            .create_project(&NewProject {
                id: project,
                name: name(label),
                root_path: name(&format!("/tmp/{label}")),
                created_at: now(),
            })
            .expect("a project is created");

        let mission = MiniProjectId::generate();
        self.store
            .create_mini_project(&NewMiniProject {
                id: mission,
                project_id: project,
                name: name(&format!("{label} goal")),
                created_at: now(),
            })
            .expect("a goal is created");

        let account = AccountProfileId::generate();
        self.store
            .create_account_profile(&NewAccountProfile {
                id: account,
                project_id: project,
                label: name(&format!("{label} account")),
                external_account_id: None,
                harness: runtime_kind(),
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: kontor_core::id::CredentialAlias::parse("sa-alpha")
                        .expect("a valid alias"),
                },
                environment: document("environment"),
                routing: document("routing"),
                capability: document("capability"),
                provider_identity: None,
                enabled: true,
                created_at: now(),
            })
            .expect("an account profile is created");

        let template = TeamTemplateRevision {
            template_id: TeamTemplateId::generate(),
            version: SpecVersion::FIRST,
            name: name(&format!("{label} team")),
            definition: document("team"),
            role_authority: vec![RoleAuthority {
                role: role("sa.maker"),
                may_evaluate: Vec::new(),
                may_waive: Vec::new(),
            }],
        };
        self.store
            .insert_team_template(project, &template)
            .expect("the template is stored");

        // An authorization is receipt-backed: the capability receipt has to be a
        // real `authorize_execution` command against this scope, so the fixture
        // records one rather than inventing an id.
        let capability_receipt = CommandReceiptId::generate();
        self.store
            .record_intent(&NewCommandIntent {
                project_id: project,
                receipt_id: capability_receipt,
                idempotency_key: IdempotencyKey::parse(&format!("{label}-authorize"))
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
        self.store
            .insert_authorization(&ExecutionAuthorization {
                id: authorization,
                project_id: project,
                scope: WorkScope::Project,
                selected_tasks: Vec::new(),
                allowed_start: TimeRange {
                    start: at("2026-08-12T00:00:00Z"),
                    end: at("2026-08-13T00:00:00Z"),
                },
                max_concurrency: 50,
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

        Scope {
            project,
            mission,
            account,
            template,
            authorization,
        }
    }

    fn task(&self, scope: &Scope, title: &str, state: TaskState) -> TaskId {
        let task = TaskId::generate();
        self.store
            .create_task(&NewTask {
                id: task,
                project_id: scope.project,
                mini_project_id: Some(scope.mission),
                title: name(title),
                module: None,
                state,
                created_at: now(),
            })
            .expect("a task is created");
        task
    }

    /// A decision in its admitted shape, as the pass would have produced it.
    fn admitted(
        &self,
        scope: &Scope,
        task: TaskId,
        module_key: Option<ModuleKey>,
        worktree: Option<ExternalName>,
    ) -> AdmittedCandidate {
        AdmittedCandidate {
            project_id: scope.project,
            task_id: task,
            revision: AggregateRevision::INITIAL,
            workflow_id: TaskWorkflowId::generate(),
            ordering: OrderingInputs {
                priority: 500,
                created_at: now(),
                task_id: task,
            },
            capacity: CapacitySnapshot {
                remaining: [(CapacityLimitKind::Global, 50)].into_iter().collect(),
                effective: 50,
                binding: CapacityLimitKind::Global,
            },
            module: module_key,
            worktree,
            authorization_id: scope.authorization,
            calendar: CalendarAdmission::unrestricted(),
            account_profile_id: Some(scope.account),
            runtime_kind: runtime_kind(),
            runtime_generation: 7,
            intake_receipt_id: None,
        }
    }
}

/// The mutable ids one admission mints, kept together so a replay can reuse the
/// launch key while everything else is fresh.
struct Parts {
    team_run: TeamRunId,
    agent_run: AgentRunId,
    receipt: CommandReceiptId,
    launch_key: IdempotencyKey,
    admission: AdmissionEventId,
    module_lease: ResourceLeaseId,
    worktree_lease: ResourceLeaseId,
}

impl Parts {
    fn new(label: &str) -> Self {
        Self {
            team_run: TeamRunId::generate(),
            agent_run: AgentRunId::generate(),
            receipt: CommandReceiptId::generate(),
            launch_key: IdempotencyKey::parse(&format!("launch-{label}")).expect("a valid key"),
            admission: AdmissionEventId::generate(),
            module_lease: ResourceLeaseId::generate(),
            worktree_lease: ResourceLeaseId::generate(),
        }
    }
}

/// Assemble the commit request the way a daemon would.
fn commit<'a>(
    scope: &Scope,
    admitted: &'a AdmittedCandidate,
    peers: &'a BTreeSet<TaskId>,
    parts: &Parts,
    template: &TeamTemplateRevision,
    decided_at: Timestamp,
) -> AdmissionCommit<'a> {
    AdmissionCommit {
        admitted,
        serializes_with: peers,
        capacity: wide_capacity(),
        team_run: NewTeamRun {
            id: parts.team_run,
            project_id: scope.project,
            task_id: admitted.task_id,
            snapshot: TeamRunSnapshot::from_revision(template, SCHEMA_VERSION),
            created_at: decided_at,
        },
        agent_run: NewAgentRun {
            id: parts.agent_run,
            project_id: scope.project,
            team_run_id: parts.team_run,
            parent_agent_run_id: None,
            role: role("sa.maker"),
            account_profile_id: admitted.account_profile_id,
            binding: None,
            created_at: decided_at,
        },
        launch: NewCommandIntent {
            project_id: scope.project,
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
        module_lease_id: admitted.module.as_ref().map(|_| parts.module_lease),
        worktree_lease_id: admitted.worktree.as_ref().map(|_| parts.worktree_lease),
        holder_instance: ExternalId::parse("scheduler-instance-a").expect("a valid holder"),
        lease_expires_at: decided_at + jiff::SignedDuration::from_secs(300),
        evidence: document("admission-evidence"),
        decided_at,
    }
}

fn count(connection: &Connection, sql: &str) -> i64 {
    connection
        .query_row(sql, [], |row| row.get(0))
        .expect("the count is readable")
}

// ---------------------------------------------------------------------------
// Atomicity
// ---------------------------------------------------------------------------

#[test]
fn an_admission_writes_the_run_the_lease_the_intent_and_the_decision_together() {
    let harness = Harness::new();
    let scope = harness.scope("atomic");
    let task = harness.task(&scope, "Admitted task", TaskState::Ready);
    let admitted = harness.admitted(
        &scope,
        task,
        Some(module("directory.app")),
        Some(name("/trees/one")),
    );
    let peers = BTreeSet::new();
    let parts = Parts::new("atomic");

    let outcome = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect("the admission commits");

    assert!(!outcome.replayed);
    assert_eq!(outcome.admission_event_id, parts.admission);
    assert_eq!(outcome.module_lease_id, Some(parts.module_lease));
    assert_eq!(outcome.worktree_lease_id, Some(parts.worktree_lease));
    assert!(outcome.reclaimed.is_empty());

    // The run is queued and its desired state is the launch's — the intent's
    // compare-and-swap ran in the same transaction.
    let run = harness
        .store
        .get_agent_run(scope.project, parts.agent_run)
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(run.projection.lifecycle, RunLifecycle::Queued);
    assert_eq!(run.projection.desired, DesiredRunState::RunRequested);
    assert_eq!(run.account_profile_id, Some(scope.account));
    assert!(
        run.binding.is_none(),
        "an admitted run is queued, not launched"
    );

    // The launch is in the outbox, undispatched. Nothing contacted a runtime.
    let due = harness
        .store
        .claim_outbox(scope.project, now(), 10)
        .expect("the outbox is readable");
    assert!(
        due.iter()
            .any(|entry| entry.receipt_id == parts.receipt && entry.dispatched_at.is_none())
    );

    // Both leases exist, active, at the first fencing token, and each has exactly
    // one `acquired` event.
    for (lease_id, expected_place, expected_tree) in [
        (
            parts.module_lease,
            name("directory.app"),
            Some(name("/trees/one")),
        ),
        (parts.worktree_lease, name("/trees/one"), None),
    ] {
        let lease = harness
            .store
            .get_lease(scope.project, lease_id)
            .expect("the lease is readable")
            .expect("the lease exists");
        assert!(lease.is_active());
        assert_eq!(lease.resource_key, expected_place);
        assert_eq!(lease.worktree_key, expected_tree);
        assert_eq!(lease.fencing_token, 1);
        assert_eq!(lease.agent_run_id, parts.agent_run);
        assert_eq!(lease.admission_event_id, Some(parts.admission));
        assert_eq!(
            harness
                .store
                .lease_history(scope.project, lease_id)
                .expect("the history is readable"),
            vec![(1, LeaseEventKind::Acquired, 1)]
        );
    }

    let connection = harness.raw();
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM scheduler_admission_events WHERE decision = 'admitted'"
        ),
        1
    );
    assert_eq!(
        count(&connection, "SELECT count(*) FROM resource_leases"),
        2
    );
}

#[test]
fn a_refused_admission_writes_nothing_at_all() {
    let harness = Harness::new();
    let scope = harness.scope("nothing");
    let task = harness.task(&scope, "Moved task", TaskState::Ready);
    // The decision was computed against revision 1; the task is at revision 1 but
    // has left `ready`, which is the shape of a task somebody resumed, parked or
    // cancelled between the snapshot and the commit.
    let mut admitted = harness.admitted(&scope, task, Some(module("directory.app")), None);
    admitted.revision = AggregateRevision::parse(9).expect("a revision");
    let peers = BTreeSet::new();
    let parts = Parts::new("nothing");

    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect_err("a stale revision refuses the admission");
    assert!(
        matches!(error, RepositoryError::Domain(_)),
        "a moved revision is a domain conflict, got {error:?}"
    );

    let connection = harness.raw();
    for table in [
        "team_runs",
        "agent_runs",
        "resource_leases",
        "lease_events",
        "scheduler_admission_events",
        "command_outbox",
    ] {
        let rows = count(&connection, &format!("SELECT count(*) FROM {table}"));
        let expected = i64::from(table == "command_outbox");
        assert_eq!(
            rows, expected,
            "`{table}` must carry nothing from a refused admission \
             (the authorization's own receipt is the one outbox row)"
        );
    }
}

#[test]
fn a_task_that_left_ready_is_refused_even_at_the_admitted_revision() {
    let harness = Harness::new();
    let scope = harness.scope("left-ready");
    let task = harness.task(&scope, "Draft task", TaskState::Draft);
    let admitted = harness.admitted(&scope, task, None, None);
    let peers = BTreeSet::new();
    let parts = Parts::new("left-ready");

    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect_err("a task that is not ready is refused");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "{error:?}"
    );
}

// ---------------------------------------------------------------------------
// Exclusion
// ---------------------------------------------------------------------------

#[test]
fn two_schedulers_never_admit_the_same_module_twice_even_across_projects() {
    let harness = Harness::new();
    let first = harness.scope("realm-a");
    let second = harness.scope("realm-b");
    let shared = module("directory.app");

    let task_a = harness.task(&first, "Task A", TaskState::Ready);
    let admitted_a = harness.admitted(&first, task_a, Some(shared.clone()), None);
    let peers = BTreeSet::new();
    let parts_a = Parts::new("realm-a");
    harness
        .store
        .admit_candidate(&commit(
            &first,
            &admitted_a,
            &peers,
            &parts_a,
            &first.template,
            now(),
        ))
        .expect("the first admission commits");

    // A *different project* in the same Realm. v1's index was keyed on
    // project_id, so this is exactly the overlap it could not prevent — and the
    // module is one place on disk whatever the project rows say.
    let task_b = harness.task(&second, "Task B", TaskState::Ready);
    let admitted_b = harness.admitted(&second, task_b, Some(shared), None);
    let parts_b = Parts::new("realm-b");
    let error = harness
        .store
        .admit_candidate(&commit(
            &second,
            &admitted_b,
            &peers,
            &parts_b,
            &second.template,
            now(),
        ))
        .expect_err("one module is held once across the Realm");
    // The subject matters. The store refuses this with its *own* rule, before the
    // index or the trigger has to; a bare `storage` conflict here would mean the
    // pre-check had gone and the caller was being handed a raw constraint failure
    // to interpret. The structural backstop is proved separately, against direct
    // SQL that never came through this function.
    assert!(
        matches!(
            error,
            RepositoryError::Conflict {
                subject: "resource lease",
                ..
            }
        ),
        "{error:?}"
    );

    let connection = harness.raw();
    assert_eq!(
        count(&connection, "SELECT count(*) FROM resource_leases"),
        1,
        "the refused admission left no lease"
    );
    assert_eq!(count(&connection, "SELECT count(*) FROM agent_runs"), 1);
}

#[test]
fn the_exclusion_holds_against_direct_sql_that_never_came_through_the_store() {
    let harness = Harness::new();
    let scope = harness.scope("trigger");
    let task = harness.task(&scope, "Held task", TaskState::Ready);
    let admitted = harness.admitted(&scope, task, Some(module("directory.app")), None);
    let peers = BTreeSet::new();
    let parts = Parts::new("trigger");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect("the admission commits");

    // A caller that skipped `admit_candidate` entirely. The Rust pre-check is a
    // courtesy; the index and the trigger are the rule.
    let connection = harness.raw();
    let forged = connection.execute(
        "INSERT INTO resource_leases
             (id, project_id, resource_key, worktree_key, agent_run_id, acquired_at,
              lease_kind, expires_at, fencing_token, holder_instance)
         VALUES (?1, ?2, 'directory.app', NULL, ?3, '2026-08-12T09:00:00Z',
                 'module', '2026-08-12T09:05:00Z', 1, 'forged')",
        rusqlite::params![
            ResourceLeaseId::generate().to_string(),
            scope.project.to_string(),
            parts.agent_run.to_string()
        ],
    );
    assert!(
        forged.is_err(),
        "direct SQL must not be able to double-claim a module"
    );

    // An isolated claim on the same module is refused too: an unisolated holder
    // excludes every contender, which no index can say on its own.
    let isolated = connection.execute(
        "INSERT INTO resource_leases
             (id, project_id, resource_key, worktree_key, agent_run_id, acquired_at,
              lease_kind, expires_at, fencing_token, holder_instance)
         VALUES (?1, ?2, 'directory.app', '/trees/other', ?3, '2026-08-12T09:00:00Z',
                 'module', '2026-08-12T09:05:00Z', 1, 'forged')",
        rusqlite::params![
            ResourceLeaseId::generate().to_string(),
            scope.project.to_string(),
            parts.agent_run.to_string()
        ],
    );
    assert!(
        isolated.is_err(),
        "an unisolated holder excludes an isolated contender too"
    );
}

#[test]
fn distinct_verified_trees_may_hold_one_module_and_a_shared_tree_may_not() {
    let harness = Harness::new();
    let scope = harness.scope("trees");
    let shared = module("directory.app");
    let peers = BTreeSet::new();

    let task_a = harness.task(&scope, "Tree A", TaskState::Ready);
    let admitted_a = harness.admitted(&scope, task_a, Some(shared.clone()), Some(name("/trees/a")));
    let parts_a = Parts::new("tree-a");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted_a,
            &peers,
            &parts_a,
            &scope.template,
            now(),
        ))
        .expect("the first tree admits");

    // A distinct verified tree over the same module: admitted.
    let task_b = harness.task(&scope, "Tree B", TaskState::Ready);
    let admitted_b = harness.admitted(&scope, task_b, Some(shared.clone()), Some(name("/trees/b")));
    let parts_b = Parts::new("tree-b");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted_b,
            &peers,
            &parts_b,
            &scope.template,
            now(),
        ))
        .expect("a distinct tree isolates the same module");

    // The *same* tree again is not isolation, and it is also a second claim on the
    // tree itself.
    let task_c = harness.task(&scope, "Tree A again", TaskState::Ready);
    let admitted_c = harness.admitted(&scope, task_c, Some(shared), Some(name("/trees/a")));
    let parts_c = Parts::new("tree-a-again");
    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted_c,
            &peers,
            &parts_c,
            &scope.template,
            now(),
        ))
        .expect_err("a duplicate tree is not isolation");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "{error:?}"
    );

    let claims = harness
        .store
        .active_module_claims(now())
        .expect("the claims are readable");
    assert_eq!(claims.len(), 2);
    let trees: BTreeSet<Option<ExternalName>> =
        claims.iter().map(|claim| claim.worktree.clone()).collect();
    assert_eq!(
        trees,
        [Some(name("/trees/a")), Some(name("/trees/b"))]
            .into_iter()
            .collect()
    );
    assert_eq!(
        harness
            .store
            .active_worktree_leases(now())
            .expect("the trees are readable"),
        [name("/trees/a"), name("/trees/b")].into_iter().collect()
    );
}

#[test]
fn a_pre_fix_module_lease_recovers_its_declared_task_worktree() {
    let harness = Harness::new();
    let scope = harness.scope("legacy-tree");
    let task = harness.task(&scope, "Legacy tree", TaskState::Ready);
    let tree = name("/trees/legacy");
    harness
        .store
        .set_task_worktree(scope.project, task, &tree)
        .expect("the task worktree is declared");

    let admitted = harness.admitted(
        &scope,
        task,
        Some(module("shared.module")),
        Some(tree.clone()),
    );
    let parts = Parts::new("legacy-tree");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &BTreeSet::new(),
            &parts,
            &scope.template,
            now(),
        ))
        .expect("the task admits");

    let raw = harness.raw();
    raw.execute_batch(
        "DROP TRIGGER resource_leases_advance_rules;
         DROP TRIGGER resource_leases_require_lease_event;",
    )
    .expect("the fixture can model the pre-fix row shape");
    raw.execute(
        "UPDATE resource_leases SET worktree_key = NULL WHERE id = ?1",
        [parts.module_lease.to_string()],
    )
    .expect("the fixture models a lease written before worktree retention");

    let claims = harness
        .store
        .active_module_claims(now())
        .expect("the claims are readable");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].worktree, Some(tree));
}

/// The general form of "two schedulers never admit one task twice".
///
/// Every other exclusion has a gap for this case: the task's own module lease does
/// not contend with the task that holds it, the launch idempotency key is the
/// caller's to vary, and the task row may still read `ready`. So the transaction
/// refuses a task that already has an open run, whatever else is different about
/// the second request.
#[test]
fn one_task_is_never_admitted_twice_even_with_a_fresh_key_and_no_module() {
    let harness = Harness::new();
    let scope = harness.scope("twice");
    let task = harness.task(&scope, "Admitted once", TaskState::Ready);
    let peers = BTreeSet::new();

    let first_admitted = harness.admitted(&scope, task, None, None);
    let first_parts = Parts::new("twice-first");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &first_admitted,
            &peers,
            &first_parts,
            &scope.template,
            now(),
        ))
        .expect("the first admission commits");

    // A second instance, deciding from a snapshot taken before the first
    // committed: a different launch key, a different run, no module to collide on.
    let second_admitted = harness.admitted(&scope, task, None, None);
    let second_parts = Parts::new("twice-second");
    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &second_admitted,
            &peers,
            &second_parts,
            &scope.template,
            now(),
        ))
        .expect_err("one task is admitted once");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "{error:?}"
    );

    let connection = harness.raw();
    assert_eq!(count(&connection, "SELECT count(*) FROM agent_runs"), 1);
    assert_eq!(count(&connection, "SELECT count(*) FROM team_runs"), 1);
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM scheduler_admission_events WHERE decision = 'admitted'"
        ),
        1
    );

    // The same fact is what a caller reads when it builds the next snapshot.
    assert_eq!(
        harness
            .store
            .tasks_with_open_runs()
            .expect("the in-flight tasks are readable"),
        [task].into_iter().collect()
    );
}

#[test]
fn a_serialization_peer_with_an_open_run_blocks_the_admission() {
    let harness = Harness::new();
    let scope = harness.scope("serialize");
    let peers_none = BTreeSet::new();

    let peer_task = harness.task(&scope, "Peer", TaskState::Ready);
    let peer_admitted = harness.admitted(&scope, peer_task, None, None);
    let peer_parts = Parts::new("peer");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &peer_admitted,
            &peers_none,
            &peer_parts,
            &scope.template,
            now(),
        ))
        .expect("the peer admits");

    // The pass could not have seen this: another instance admitted the peer after
    // the snapshot was taken. Only a read under the write lock catches it.
    let task = harness.task(&scope, "Serialized", TaskState::Ready);
    let admitted = harness.admitted(&scope, task, None, None);
    let peers: BTreeSet<TaskId> = [peer_task].into_iter().collect();
    let parts = Parts::new("serialized");
    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect_err("a peer with an open run blocks");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "{error:?}"
    );
}

#[test]
fn a_ceiling_is_recounted_from_the_rows_rather_than_trusted() {
    let harness = Harness::new();
    let scope = harness.scope("ceiling");
    let peers = BTreeSet::new();

    let first_task = harness.task(&scope, "First", TaskState::Ready);
    let first_admitted = harness.admitted(&scope, first_task, None, None);
    let first_parts = Parts::new("ceiling-first");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &first_admitted,
            &peers,
            &first_parts,
            &scope.template,
            now(),
        ))
        .expect("the first admission commits");

    // The decision claims 50 units of headroom. The store recounts and finds the
    // configured ceiling of one already spent, so the claim buys nothing.
    let task = harness.task(&scope, "Second", TaskState::Ready);
    let admitted = harness.admitted(&scope, task, None, None);
    let parts = Parts::new("ceiling-second");
    let mut request = commit(&scope, &admitted, &peers, &parts, &scope.template, now());
    request.capacity.global_max_in_flight = 1;
    let error = harness
        .store
        .admit_candidate(&request)
        .expect_err("a spent ceiling refuses the admission");
    // Its own variant, not a conflict. Nothing the caller presented was stale,
    // so the two refusals mean different things and are typed differently.
    assert_eq!(
        error,
        RepositoryError::CapacityExhausted { scope: "global" },
        "{error:?}"
    );

    // Each keyed ceiling is recounted independently, and each says which one it
    // was — internally. That name is what the boundary withholds, so it has to
    // exist here to be withheld there.
    for expected in ["project", "goal", "account"] {
        let mut request = commit(&scope, &admitted, &peers, &parts, &scope.template, now());
        match expected {
            "project" => request.capacity.project_max_in_flight = 1,
            "goal" => request.capacity.mission_max_in_flight = 1,
            _ => request.capacity.account_max_in_flight = 1,
        }
        let error = harness
            .store
            .admit_candidate(&request)
            .expect_err("each keyed ceiling is recounted");
        assert_eq!(
            error,
            RepositoryError::CapacityExhausted { scope: expected },
            "{error:?}"
        );
    }
}

#[test]
fn four_five_seat_teams_spend_four_capacity_envelopes() {
    let harness = Harness::new();
    let scope = harness.scope("team-capacity");
    let peers = BTreeSet::new();

    for team in 0..4 {
        let task = harness.task(&scope, &format!("Team {team}"), TaskState::Ready);
        let admitted = harness.admitted(&scope, task, None, None);
        let parts = Parts::new(&format!("team-capacity-{team}"));
        let mut request = commit(&scope, &admitted, &peers, &parts, &scope.template, now());
        request.capacity.global_max_in_flight = 4;
        request.capacity.project_max_in_flight = 4;
        request.capacity.mission_max_in_flight = 4;
        request.capacity.account_max_in_flight = 4;
        harness
            .store
            .admit_candidate(&request)
            .expect("each of four TeamRun envelopes admits");

        for seat in 1..5 {
            harness
                .store
                .create_agent_run(&NewAgentRun {
                    id: AgentRunId::generate(),
                    project_id: scope.project,
                    team_run_id: parts.team_run,
                    parent_agent_run_id: None,
                    role: role(&format!("seat-{team}-{seat}")),
                    account_profile_id: Some(scope.account),
                    binding: None,
                    created_at: now(),
                })
                .expect("the declared seat is durable");
        }
    }

    let open_seats: i64 = harness
        .raw()
        .query_row(
            "SELECT count(*) FROM agent_runs WHERE lifecycle = 'queued'",
            [],
            |row| row.get(0),
        )
        .expect("the seat population is readable");
    assert_eq!(open_seats, 20);

    let task = harness.task(&scope, "Fifth team", TaskState::Ready);
    let admitted = harness.admitted(&scope, task, None, None);
    let parts = Parts::new("team-capacity-fifth");
    let mut request = commit(&scope, &admitted, &peers, &parts, &scope.template, now());
    request.capacity.global_max_in_flight = 4;
    assert_eq!(
        harness.store.admit_candidate(&request),
        Err(RepositoryError::CapacityExhausted { scope: "global" })
    );
}

#[test]
fn a_dependency_that_has_not_finished_blocks_the_admission() {
    let harness = Harness::new();
    let scope = harness.scope("deps");
    let dependency = harness.task(&scope, "Dependency", TaskState::Ready);
    let task = harness.task(&scope, "Dependent", TaskState::Ready);
    harness
        .store
        .set_task_dependencies(scope.project, task, &[dependency])
        .expect("the edge is stored");

    let admitted = harness.admitted(&scope, task, None, None);
    let peers = BTreeSet::new();
    let parts = Parts::new("deps");
    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect_err("an unfinished dependency blocks");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "{error:?}"
    );
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

#[test]
fn a_replayed_admission_produces_one_durable_admission_and_no_second_launch() {
    let harness = Harness::new();
    let scope = harness.scope("replay");
    let task = harness.task(&scope, "Replayed task", TaskState::Ready);
    let admitted = harness.admitted(
        &scope,
        task,
        Some(module("directory.app")),
        Some(name("/trees/one")),
    );
    let peers = BTreeSet::new();
    let parts = Parts::new("replay");

    let first = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect("the admission commits");
    assert!(!first.replayed);

    // The acknowledgement was lost, so the caller re-sends the request it already
    // built — the same ids, the same launch key, the same intent — which is what a
    // retry after a lost reply actually looks like. Only the clock has moved.
    let second = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            later(30),
        ))
        .expect("the retry finds the original admission");

    assert!(second.replayed);
    assert_eq!(second.admission_event_id, first.admission_event_id);
    assert_eq!(second.receipt.id, first.receipt.id);
    assert_eq!(second.module_lease_id, first.module_lease_id);
    assert_eq!(second.worktree_lease_id, first.worktree_lease_id);

    let connection = harness.raw();
    assert_eq!(count(&connection, "SELECT count(*) FROM agent_runs"), 1);
    assert_eq!(count(&connection, "SELECT count(*) FROM team_runs"), 1);
    assert_eq!(
        count(&connection, "SELECT count(*) FROM resource_leases"),
        2
    );
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM scheduler_admission_events WHERE decision = 'admitted'"
        ),
        1
    );
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM command_outbox WHERE receipt_id = (
                 SELECT id FROM command_receipts WHERE kind = 'launch_run')"
        ),
        1,
        "one launch, not two"
    );
}

#[test]
fn a_reused_launch_key_naming_a_different_task_is_refused() {
    let harness = Harness::new();
    let scope = harness.scope("reuse");
    let peers = BTreeSet::new();

    let first_task = harness.task(&scope, "First", TaskState::Ready);
    let first_admitted = harness.admitted(&scope, first_task, None, None);
    let parts = Parts::new("reuse");
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &first_admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect("the first admission commits");

    // The same key, a different run. The intent digest and the target both differ,
    // so this is a different command wearing a used key rather than a retry.
    let other_task = harness.task(&scope, "Other", TaskState::Ready);
    let other_admitted = harness.admitted(&scope, other_task, None, None);
    let mut other_parts = Parts::new("reuse-other");
    other_parts.launch_key = parts.launch_key.clone();
    let error = harness
        .store
        .admit_candidate(&commit(
            &scope,
            &other_admitted,
            &peers,
            &other_parts,
            &scope.template,
            now(),
        ))
        .expect_err("a reused key with a different target is refused");
    assert!(matches!(error, RepositoryError::Domain(_)), "{error:?}");
}

// ---------------------------------------------------------------------------
// Lease lifecycle
// ---------------------------------------------------------------------------

/// Admit one task and return its scope, parts and module lease.
fn admitted_with_module(harness: &Harness, label: &str) -> (Scope, Parts) {
    let scope = harness.scope(label);
    let task = harness.task(&scope, "Leased task", TaskState::Ready);
    let admitted = harness.admitted(&scope, task, Some(module("directory.app")), None);
    let peers = BTreeSet::new();
    let parts = Parts::new(label);
    harness
        .store
        .admit_candidate(&commit(
            &scope,
            &admitted,
            &peers,
            &parts,
            &scope.template,
            now(),
        ))
        .expect("the admission commits");
    (scope, parts)
}

#[test]
fn a_renewal_rotates_the_token_and_an_old_token_can_neither_renew_nor_release() {
    let harness = Harness::new();
    let (scope, parts) = admitted_with_module(&harness, "fencing");

    let renewed = harness
        .store
        .renew_lease(&LeaseRenewal {
            project_id: scope.project,
            lease_id: parts.module_lease,
            presented_token: 1,
            expires_at: later(600),
            renewed_at: later(60),
        })
        .expect("the holder renews with the current token");
    assert_eq!(renewed.fencing_token, 2);
    assert_eq!(renewed.expires_at, later(600));
    assert!(renewed.is_active());

    // The stale holder: it was asleep while the lease was renewed, and the token
    // it remembers is no longer the one on the row.
    let stale_renew = harness.store.renew_lease(&LeaseRenewal {
        project_id: scope.project,
        lease_id: parts.module_lease,
        presented_token: 1,
        expires_at: later(900),
        renewed_at: later(120),
    });
    assert!(
        matches!(stale_renew, Err(RepositoryError::Conflict { .. })),
        "{stale_renew:?}"
    );

    let receipt = release_receipt(&harness, &scope, "stale-release");
    let stale_release = harness.store.release_lease(&LeaseRelease {
        project_id: scope.project,
        lease_id: parts.module_lease,
        presented_token: 1,
        receipt_id: receipt,
        released_at: later(120),
    });
    assert!(
        matches!(stale_release, Err(RepositoryError::Conflict { .. })),
        "a stale holder must not be able to release work it no longer owns: {stale_release:?}"
    );

    // A renewal that does not move the expiry forward spends a token for nothing.
    let backwards = harness.store.renew_lease(&LeaseRenewal {
        project_id: scope.project,
        lease_id: parts.module_lease,
        presented_token: 2,
        expires_at: later(300),
        renewed_at: later(120),
    });
    assert!(matches!(backwards, Err(RepositoryError::Conflict { .. })));

    // The current holder still can, and the history records every step.
    harness
        .store
        .release_lease(&LeaseRelease {
            project_id: scope.project,
            lease_id: parts.module_lease,
            presented_token: 2,
            receipt_id: release_receipt(&harness, &scope, "real-release"),
            released_at: later(180),
        })
        .expect("the current holder releases");
    assert_eq!(
        harness
            .store
            .lease_history(scope.project, parts.module_lease)
            .expect("the history is readable"),
        vec![
            (1, LeaseEventKind::Acquired, 1),
            (2, LeaseEventKind::Renewed, 2),
            (3, LeaseEventKind::Released, 2),
        ]
    );
}

/// A receipt a release can cite. Any recorded command in the project will do;
/// what matters is that the release is receipt-backed rather than inferred.
fn release_receipt(harness: &Harness, scope: &Scope, label: &str) -> CommandReceiptId {
    let receipt_id = CommandReceiptId::generate();
    harness
        .store
        .record_intent(&NewCommandIntent {
            project_id: scope.project,
            receipt_id,
            idempotency_key: IdempotencyKey::parse(&format!("release-{label}"))
                .expect("a valid key"),
            kind: CommandKind::ResumeTask,
            target: AggregateRef::Task {
                task_id: harness.task(scope, "Release carrier", TaskState::Ready),
            },
            target_revision: AggregateRevision::INITIAL,
            intent: document(&format!("release-intent-{label}")),
            payload: document(&format!("release-payload-{label}")),
            desired: None,
            not_before: now(),
            created_at: now(),
        })
        .expect("the release receipt is recorded");
    receipt_id
}

#[test]
fn a_lapsed_lease_is_reclaimable_and_says_nothing_about_the_run_that_held_it() {
    let harness = Harness::new();
    let (first_scope, first_parts) = admitted_with_module(&harness, "lapse");

    let before = harness
        .store
        .get_agent_run(first_scope.project, first_parts.agent_run)
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(before.projection.lifecycle, RunLifecycle::Queued);

    // The lease was taken at 09:00 and lives 300 seconds. At 09:10 it has lapsed,
    // so the module is free — and a second project may take it.
    let reclaim_at = later(600);
    assert!(
        harness
            .store
            .active_module_claims(reclaim_at)
            .expect("the claims are readable")
            .is_empty(),
        "a lapsed lease does not hold its module"
    );

    let second_scope = harness.scope("lapse-reclaim");
    let task = harness.task(&second_scope, "Reclaimer", TaskState::Ready);
    let admitted = harness.admitted(&second_scope, task, Some(module("directory.app")), None);
    let peers = BTreeSet::new();
    let parts = Parts::new("lapse-reclaim");
    let outcome = harness
        .store
        .admit_candidate(&commit(
            &second_scope,
            &admitted,
            &peers,
            &parts,
            &second_scope.template,
            reclaim_at,
        ))
        .expect("a lapsed module is reclaimable");
    assert_eq!(outcome.reclaimed, vec![first_parts.module_lease]);

    // The lapsed lease is expired, not released: nobody decided it, so there is no
    // receipt — and the expiry is recorded as its own kind of event.
    let lapsed = harness
        .store
        .get_lease(first_scope.project, first_parts.module_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    assert!(!lapsed.is_active());
    assert!(lapsed.expired_at.is_some());
    assert!(
        lapsed.released_at.is_none(),
        "an expiry is not a release: nobody decided it"
    );
    assert_eq!(
        harness
            .store
            .lease_history(first_scope.project, first_parts.module_lease)
            .expect("the history is readable"),
        vec![
            (1, LeaseEventKind::Acquired, 1),
            (2, LeaseEventKind::Expired, 1),
        ]
    );

    // **The invariant that matters most.** Losing a lease concludes nothing about
    // the work: the run is exactly as it was, still open, with no terminal
    // evidence and no outcome invented for it.
    let after = harness
        .store
        .get_agent_run(first_scope.project, first_parts.agent_run)
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(after.projection.lifecycle, RunLifecycle::Queued);
    assert!(after.terminal.is_none());
    assert!(after.closed_at.is_none());
    assert_eq!(after, before);

    // The successor records what it took the place over from.
    let successor = harness
        .store
        .get_lease(second_scope.project, parts.module_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    assert_eq!(
        successor.renewed_from_lease_id,
        Some(first_parts.module_lease)
    );
    assert_eq!(successor.fencing_token, 1);

    // And the reclaimed lease is beyond renewal or release, whatever token is
    // presented.
    assert!(
        harness
            .store
            .renew_lease(&LeaseRenewal {
                project_id: first_scope.project,
                lease_id: first_parts.module_lease,
                presented_token: 1,
                expires_at: later(1_200),
                renewed_at: later(700),
            })
            .is_err(),
        "an expired lease is reclaimed by a new lease, never revived"
    );
}

/// Reclaim lineage names a lease on the *same* place, never on the other one the
/// admission also claimed.
///
/// The mutant this kills is a single pooled list of everything the admission
/// reclaimed: with a module and a worktree both being taken over, the worktree
/// lease would cite the lease the module was reclaimed from — a link to a claim on
/// a different place, which is the one thing this column must not say.
#[test]
fn each_reclaimed_place_gives_its_own_lease_its_own_lineage() {
    let harness = Harness::new();
    let peers = BTreeSet::new();

    // A first admission holding both a module and a tree, at a short expiry.
    let first = harness.scope("lineage-first");
    let first_task = harness.task(&first, "First holder", TaskState::Ready);
    let first_admitted = harness.admitted(
        &first,
        first_task,
        Some(module("directory.app")),
        Some(name("/trees/one")),
    );
    let first_parts = Parts::new("lineage-first");
    harness
        .store
        .admit_candidate(&commit(
            &first,
            &first_admitted,
            &peers,
            &first_parts,
            &first.template,
            now(),
        ))
        .expect("the first admission commits");

    // Both leases lapse, and a second admission takes over both places at once.
    let reclaim_at = later(600);
    let second = harness.scope("lineage-second");
    let second_task = harness.task(&second, "Reclaimer", TaskState::Ready);
    let second_admitted = harness.admitted(
        &second,
        second_task,
        Some(module("directory.app")),
        Some(name("/trees/one")),
    );
    let second_parts = Parts::new("lineage-second");
    let outcome = harness
        .store
        .admit_candidate(&commit(
            &second,
            &second_admitted,
            &peers,
            &second_parts,
            &second.template,
            reclaim_at,
        ))
        .expect("both lapsed places are reclaimable");
    assert_eq!(outcome.reclaimed.len(), 2);

    let module_lease = harness
        .store
        .get_lease(second.project, second_parts.module_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    let worktree_lease = harness
        .store
        .get_lease(second.project, second_parts.worktree_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    assert_eq!(
        module_lease.renewed_from_lease_id,
        Some(first_parts.module_lease),
        "the module lease cites the module lease it took over from"
    );
    assert_eq!(
        worktree_lease.renewed_from_lease_id,
        Some(first_parts.worktree_lease),
        "the worktree lease cites the worktree lease it took over from"
    );
    assert_ne!(
        module_lease.renewed_from_lease_id,
        worktree_lease.renewed_from_lease_id
    );
}

/// A lease that was renewed and *then* lapsed is still reclaimable, and its expiry
/// is recorded at the token it actually ended on.
///
/// The two halves are one fact. `resource_leases_require_lease_event` matches the
/// appended event against the token on the row, so an expiry logged at the wrong
/// token is not merely mis-recorded — it is refused, and the place becomes
/// permanently unreclaimable. The mutant this kills is an expiry that assumes the
/// first token instead of reading the one the lease is holding.
#[test]
fn a_renewed_lease_that_lapses_is_reclaimable_at_the_token_it_ended_on() {
    let harness = Harness::new();
    let (first_scope, first_parts) = admitted_with_module(&harness, "renewed-lapse");

    harness
        .store
        .renew_lease(&LeaseRenewal {
            project_id: first_scope.project,
            lease_id: first_parts.module_lease,
            presented_token: 1,
            expires_at: later(600),
            renewed_at: later(60),
        })
        .expect("the holder renews");

    // Past the renewed expiry, so the place has lapsed at token 2.
    let reclaim_at = later(900);
    let second = harness.scope("renewed-lapse-next");
    let task = harness.task(&second, "Reclaimer", TaskState::Ready);
    let admitted = harness.admitted(&second, task, Some(module("directory.app")), None);
    let peers = BTreeSet::new();
    let parts = Parts::new("renewed-lapse-next");
    let outcome = harness
        .store
        .admit_candidate(&commit(
            &second,
            &admitted,
            &peers,
            &parts,
            &second.template,
            reclaim_at,
        ))
        .expect("a renewed lease that lapsed is still reclaimable");
    assert_eq!(outcome.reclaimed, vec![first_parts.module_lease]);

    assert_eq!(
        harness
            .store
            .lease_history(first_scope.project, first_parts.module_lease)
            .expect("the history is readable"),
        vec![
            (1, LeaseEventKind::Acquired, 1),
            (2, LeaseEventKind::Renewed, 2),
            (3, LeaseEventKind::Expired, 2),
        ],
        "the expiry is recorded at the token the lease was actually holding"
    );
}

#[test]
fn a_lease_may_not_be_renewed_once_the_run_it_protects_has_closed() {
    let harness = Harness::new();
    let (scope, parts) = admitted_with_module(&harness, "closed-run");

    // The closed run is the *precondition* here, not the subject: what is under
    // test is the lease rule. So the run is closed with direct SQL, the way
    // `schema_v1.rs` builds its preconditions, rather than through a closure path
    // whose own evidence rules belong to another ticket's suite.
    let closure_receipt = release_receipt(&harness, &scope, "closure");
    let connection = harness.raw();
    connection
        .execute(
            "UPDATE agent_runs
             SET lifecycle = 'parked', derived_state = 'terminal',
                 terminal_outcome = 'abandoned', terminal_source_kind = 'operator_abandon',
                 terminal_receipt_id = ?1, terminal_evidence_hash = ?2, closed_at = ?3,
                 revision = revision + 1
             WHERE project_id = ?4 AND id = ?5",
            rusqlite::params![
                closure_receipt.to_string(),
                "0".repeat(64),
                later(60).to_string(),
                scope.project.to_string(),
                parts.agent_run.to_string()
            ],
        )
        .expect("the run is closed");

    let error = harness.store.renew_lease(&LeaseRenewal {
        project_id: scope.project,
        lease_id: parts.module_lease,
        presented_token: 1,
        expires_at: later(600),
        renewed_at: later(120),
    });
    assert!(
        matches!(error, Err(RepositoryError::Conflict { .. })),
        "a lease exists to protect work, and this work is over: {error:?}"
    );
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

#[test]
fn refusals_are_recorded_and_neither_the_decision_nor_the_history_can_be_rewritten() {
    let harness = Harness::new();
    let scope = harness.scope("record");
    let task = harness.task(&scope, "Refused task", TaskState::Ready);

    let rejection = RecordedRejection::new(
        scope.project,
        task,
        RejectionCode::CapacityExhausted,
        &[RejectionEvidence::Capacity {
            limit: CapacityLimitKind::Global,
            remaining: 0,
        }],
    )
    .expect("the refusal canonicalizes");
    harness
        .store
        .record_admission_rejections(now(), std::slice::from_ref(&rejection))
        .expect("the refusal is recorded");

    let connection = harness.raw();
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM scheduler_admission_events
             WHERE decision = 'rejected' AND rejection_code = 'capacity_exhausted'"
        ),
        1
    );
    // A refusal started nothing, so it holds nothing.
    assert_eq!(
        count(
            &connection,
            "SELECT count(*) FROM scheduler_admission_events
             WHERE decision = 'rejected'
               AND (agent_run_id IS NOT NULL OR team_run_id IS NOT NULL
                    OR launch_receipt_id IS NOT NULL OR authorization_id IS NOT NULL)"
        ),
        0
    );
    assert_eq!(
        count(&connection, "SELECT count(*) FROM resource_leases"),
        0,
        "a refusal takes no place"
    );

    for statement in [
        "UPDATE scheduler_admission_events SET rejection_code = 'task_not_ready'",
        "DELETE FROM scheduler_admission_events",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "an admission decision is immutable: `{statement}` must be refused"
        );
    }
}

/// A lease stops being active only *because its history says so*.
///
/// This is the invariant the database owes on its own. The store appends the event
/// before it moves the row, but "the store does it correctly" is not the claim:
/// `lease_events` is where an audit reads which holder was authoritative when, and
/// a direct `UPDATE` that ends a lease without an event would leave that record
/// silently incomplete.
///
/// The mutants this kills are the two halves of the hole: a trigger that checks
/// only the *shape* of an update and not whether it was recorded, and one that
/// lets a termination rotate the fencing token in the same statement — so the
/// token a lease ended on would be a value nothing ever logged.
#[test]
fn ending_a_lease_by_direct_sql_without_its_event_is_refused() {
    let harness = Harness::new();
    let (scope, parts) = admitted_with_module(&harness, "require-event");
    let connection = harness.raw();
    let lease = parts.module_lease.to_string();
    let project = scope.project.to_string();

    // 1. A release with no `released` row anywhere.
    let receipt = release_receipt(&harness, &scope, "forged").to_string();
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET released_at = ?1, release_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4",
                rusqlite::params![later(60).to_string(), receipt, project, lease],
            )
            .is_err(),
        "a release with no appended event must be refused"
    );

    // 2. An expiry with no `expired` row.
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET expired_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                rusqlite::params![later(60).to_string(), project, lease],
            )
            .is_err(),
        "an expiry with no appended event must be refused"
    );

    // 3. A renewal that rotates the token with nothing logging the rotation.
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET fencing_token = 2, expires_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                rusqlite::params![later(600).to_string(), project, lease],
            )
            .is_err(),
        "a token rotation with no appended event must be refused"
    );

    // 4. An event of the *wrong kind* does not satisfy the requirement. An
    //    `expired` row is not evidence of a release, however real it is.
    connection
        .execute(
            "INSERT INTO lease_events
                 (project_id, lease_id, sequence, event, fencing_token, receipt_id, occurred_at)
             VALUES (?1, ?2, 2, 'expired', 1, NULL, ?3)",
            rusqlite::params![project, lease, later(60).to_string()],
        )
        .expect("the history is append-only, not unwritable");
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET released_at = ?1, release_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4",
                rusqlite::params![later(60).to_string(), receipt, project, lease],
            )
            .is_err(),
        "an `expired` event is not evidence of a release"
    );

    // 5. An event at the *wrong instant* does not satisfy it either: the row and
    //    its evidence have to agree about when it happened.
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET expired_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                rusqlite::params![later(120).to_string(), project, lease],
            )
            .is_err(),
        "an expiry must record the instant its event recorded"
    );

    // 6. Matching kind, token and instant: the update the appended event actually
    //    accounts for is admitted. The trigger binds the row to its history rather
    //    than freezing the row, which is what keeps a claim releasable at all.
    connection
        .execute(
            "UPDATE resource_leases SET expired_at = ?1
             WHERE project_id = ?2 AND id = ?3",
            rusqlite::params![later(60).to_string(), project, lease],
        )
        .expect("the expiry its own event records is admitted");
    let lease_row = harness
        .store
        .get_lease(scope.project, parts.module_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    assert!(!lease_row.is_active());
    assert_eq!(
        lease_row.fencing_token, 1,
        "ending a lease freezes the token it ended on"
    );
}

/// A release must record the instant its own event recorded.
///
/// Its own test because `ux_lease_events_terminal` allows one terminal event per
/// lease, so a lease that already carries a planted `expired` row cannot also be
/// used to probe the release clause.
///
/// The mutant this kills is a trigger that matches only the event's *kind*: a
/// `released` row appended for one instant would then license a release recorded
/// at any other, and the row and its evidence would disagree about when the claim
/// ended.
#[test]
fn a_release_must_record_the_instant_its_event_recorded() {
    let harness = Harness::new();
    let (scope, parts) = admitted_with_module(&harness, "release-instant");
    let connection = harness.raw();
    let lease = parts.module_lease.to_string();
    let project = scope.project.to_string();
    let receipt = release_receipt(&harness, &scope, "release-instant").to_string();

    // A real `released` event, at 09:01.
    connection
        .execute(
            "INSERT INTO lease_events
                 (project_id, lease_id, sequence, event, fencing_token, receipt_id, occurred_at)
             VALUES (?1, ?2, 2, 'released', 1, ?3, ?4)",
            rusqlite::params![project, lease, receipt, later(60).to_string()],
        )
        .expect("the event is appended");

    // A release claiming 09:02 is not the release that event records.
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET released_at = ?1, release_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4",
                rusqlite::params![later(120).to_string(), receipt, project, lease],
            )
            .is_err(),
        "a release at another instant is not the one the event records"
    );

    // At the instant the event records, it is admitted.
    connection
        .execute(
            "UPDATE resource_leases SET released_at = ?1, release_receipt_id = ?2
             WHERE project_id = ?3 AND id = ?4",
            rusqlite::params![later(60).to_string(), receipt, project, lease],
        )
        .expect("the release its own event records is admitted");
    let released = harness
        .store
        .get_lease(scope.project, parts.module_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    assert_eq!(released.released_at, Some(later(60)));
}

/// Ending a lease may not rotate its token or move its expiry.
///
/// The token a lease ended on is the one every later reader judges a stale holder
/// against, so a statement that ends the lease *and* moves it would leave that
/// comparison pointing at a value no holder ever held — even with a matching
/// event appended, because the event would be at the new token.
#[test]
fn a_termination_may_not_also_rotate_the_token_or_move_the_expiry() {
    let harness = Harness::new();
    let (scope, parts) = admitted_with_module(&harness, "freeze-on-end");
    let connection = harness.raw();
    let lease = parts.module_lease.to_string();
    let project = scope.project.to_string();

    // A perfectly well-formed `expired` event — at the token the update wants to
    // move to. The shape rule refuses the update anyway.
    connection
        .execute(
            "INSERT INTO lease_events
                 (project_id, lease_id, sequence, event, fencing_token, receipt_id, occurred_at)
             VALUES (?1, ?2, 2, 'expired', 2, NULL, ?3)",
            rusqlite::params![project, lease, later(60).to_string()],
        )
        .expect("the event is appended");
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET expired_at = ?1, fencing_token = 2
                 WHERE project_id = ?2 AND id = ?3",
                rusqlite::params![later(60).to_string(), project, lease],
            )
            .is_err(),
        "an expiry must not rotate the token"
    );
    assert!(
        connection
            .execute(
                "UPDATE resource_leases SET expired_at = ?1, expires_at = ?2
                 WHERE project_id = ?3 AND id = ?4",
                rusqlite::params![
                    later(60).to_string(),
                    later(900).to_string(),
                    project,
                    lease
                ],
            )
            .is_err(),
        "an expiry must not move the expiry it is ending on"
    );
}

/// The store's own release and expiry paths still work, and each leaves its event.
///
/// The negative tests above would also pass against a lease that had become
/// unreleasable, so this is the half that proves the trigger binds the row to its
/// history rather than freezing it.
#[test]
fn the_stores_own_release_and_expiry_paths_still_succeed_and_leave_their_evidence() {
    let harness = Harness::new();

    // Release, through the store.
    let (released_scope, released_parts) = admitted_with_module(&harness, "store-release");
    harness
        .store
        .release_lease(&LeaseRelease {
            project_id: released_scope.project,
            lease_id: released_parts.module_lease,
            presented_token: 1,
            receipt_id: release_receipt(&harness, &released_scope, "store-release"),
            released_at: later(60),
        })
        .expect("the holder releases through the store");
    let released = harness
        .store
        .get_lease(released_scope.project, released_parts.module_lease)
        .expect("the lease is readable")
        .expect("the lease exists");
    assert_eq!(released.released_at, Some(later(60)));
    assert!(released.expired_at.is_none());
    assert_eq!(
        harness
            .store
            .lease_history(released_scope.project, released_parts.module_lease)
            .expect("the history is readable"),
        vec![
            (1, LeaseEventKind::Acquired, 1),
            (2, LeaseEventKind::Released, 1),
        ]
    );

    // Renewal then release, so the terminal event is at a rotated token.
    let (renewed_scope, renewed_parts) = admitted_with_module(&harness, "store-renew");
    harness
        .store
        .renew_lease(&LeaseRenewal {
            project_id: renewed_scope.project,
            lease_id: renewed_parts.module_lease,
            presented_token: 1,
            expires_at: later(600),
            renewed_at: later(60),
        })
        .expect("the holder renews through the store");
    harness
        .store
        .release_lease(&LeaseRelease {
            project_id: renewed_scope.project,
            lease_id: renewed_parts.module_lease,
            presented_token: 2,
            receipt_id: release_receipt(&harness, &renewed_scope, "store-renew"),
            released_at: later(120),
        })
        .expect("the current holder releases");
    assert_eq!(
        harness
            .store
            .lease_history(renewed_scope.project, renewed_parts.module_lease)
            .expect("the history is readable"),
        vec![
            (1, LeaseEventKind::Acquired, 1),
            (2, LeaseEventKind::Renewed, 2),
            (3, LeaseEventKind::Released, 2),
        ],
        "the token on the row is always the token of its newest logged event"
    );

    // Expiry, through the admission that reclaims the place.
    let (lapsed_scope, lapsed_parts) = admitted_with_module(&harness, "store-expire");
    let reclaim_at = later(600);
    let reclaimer = harness.scope("store-expire-next");
    let task = harness.task(&reclaimer, "Reclaimer", TaskState::Ready);
    let admitted = harness.admitted(&reclaimer, task, Some(module("directory.app")), None);
    let peers = BTreeSet::new();
    let parts = Parts::new("store-expire-next");
    harness
        .store
        .admit_candidate(&commit(
            &reclaimer,
            &admitted,
            &peers,
            &parts,
            &reclaimer.template,
            reclaim_at,
        ))
        .expect("the lapsed place is reclaimable");
    assert_eq!(
        harness
            .store
            .lease_history(lapsed_scope.project, lapsed_parts.module_lease)
            .expect("the history is readable"),
        vec![
            (1, LeaseEventKind::Acquired, 1),
            (2, LeaseEventKind::Expired, 1),
        ]
    );
}

#[test]
fn a_lease_history_entry_can_be_neither_updated_nor_deleted() {
    let harness = Harness::new();
    let (_scope, _parts) = admitted_with_module(&harness, "immutable");
    let connection = harness.raw();
    for statement in [
        "UPDATE lease_events SET event = 'released'",
        "DELETE FROM lease_events",
        "DELETE FROM resource_leases",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "`{statement}` must be refused"
        );
    }
}

#[test]
fn the_decision_records_the_ordering_the_capacity_and_the_evidence_digest() {
    let harness = Harness::new();
    let scope = harness.scope("evidence");
    let task = harness.task(&scope, "Evidenced task", TaskState::Ready);
    let admitted = harness.admitted(&scope, task, None, None);
    let peers = BTreeSet::new();
    let parts = Parts::new("evidence");
    let request = commit(&scope, &admitted, &peers, &parts, &scope.template, now());
    let expected_digest = request.evidence.hash().clone();
    harness
        .store
        .admit_candidate(&request)
        .expect("the admission commits");

    let connection = harness.raw();
    let (stored_digest, authorization, decided_at): (String, String, String) = connection
        .query_row(
            "SELECT evidence_hash, authorization_id, decided_at
             FROM scheduler_admission_events WHERE decision = 'admitted'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the decision is readable");
    assert_eq!(stored_digest, expected_digest.as_str());
    assert_eq!(authorization, scope.authorization.to_string());
    assert_eq!(decided_at, now().to_string());
}
