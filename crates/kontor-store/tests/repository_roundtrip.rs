//! Repository behaviour: project scoping, restart round trips, atomic failure,
//! command receipts and the outbox.
//!
//! The mutants this suite exists to kill:
//!
//! * removing the project id from a query or a foreign key;
//! * accepting a dependency cycle, or leaving half a graph behind when one is
//!   refused;
//! * treating an acknowledgement as completion, or reusing an idempotency key as
//!   a new command;
//! * letting a duplicate source event create a second work graph;
//! * writing an observed state straight into the derived state, or letting a
//!   disappeared process close a run;
//! * reopening a terminal run;
//! * mirroring one external comment twice, or losing an edit's provenance;
//! * treating an absent calendar as closed.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::calendar::{
    CalendarProfileSpec, CalendarResolution, EffectiveCalendarState, ExceptionKind,
    ExceptionProvenance, ExecutionAuthorization, HolidayMergePolicy, IanaTimeZone, OverrideExpiry,
    OverrideRevocation, ScheduleOverride, TimeRange, Weekday, WeeklyWindow, WorkCalendarAssignment,
    WorkScope, resolve_effective_state,
};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, BoundedText, CalendarExceptionId,
    CalendarProfileId, CanonicalDocument, CommandReceiptId, ConnectorKey, ContentHash,
    CredentialAlias, CurrencyCode, EventCursor, ExternalId, ExternalIssueTypeKey, ExternalName,
    GateKey, IdempotencyKey, IntakeReceiptId, MiniProjectId, Money, PhaseKey, ProjectId, RealmId,
    RoleKey, RuntimeBindingId, RuntimeKindKey, SCHEMA_VERSION, ScheduleOverrideId, SeatBindingId,
    SemanticMilestoneKey, SourceEventId, SpecVersion, StatusConflictId, TaskId, TaskWorkflowId,
    TeamRunId, TeamTemplateId, TicketLinkId, TicketObservationId, Timestamp, TriggerKey,
    WorkCalendarId, WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::realm::{EventEnvelope, RealmCursor, ReceiptEnvelope};
use kontor_core::receipt::{
    AggregateRef, CommandKind, CommandReceipt, CommandReceiptState, NoEffectEvidence,
};
use kontor_core::repository::RealmRepository;
use kontor_core::repository::{
    AccountProfileUpdate, CalendarRepository, CommandRepository, CompletionWrite,
    ConnectorSpecSelector, CredentialReference, CredentialReferenceKind, IntakeOutcome,
    IntakeRepository, NewAccountProfile, NewAgentRun, NewCommandIntent, NewGateEvaluation,
    NewIntakeReevaluation, NewLocalCommand, NewMiniProject, NewObservation, NewProject,
    NewRuntimeEvent, NewSourceEvent, NewTask, NewTaskPersonaSnapshot, NewTaskWorkflow, NewTeamRun,
    NewTicketLink, PhaseAdvance, ProjectRepository, ReceiptAdvance, ReevaluationOutcome,
    RepositoryError, RunClosure, RunRepository, RuntimeBinding, SourceEventIngest, SpecRepository,
    StoredCompletionProfile, StoredCompletionWake, StoredEpicCompletion, StoredRemediationProposal,
    TaskTransitionRequest, TeamRunAdvance, TeamRunClosure, TicketRepository, WorkflowRepository,
};
use kontor_core::spec::{
    ArtifactContentType, ArtifactContractSpec, BudgetBounds, CanonicalSourceEvent, DedupExpression,
    GateSpec, IntakeReceipt, IntakeResult, JsonPointer, PersonaScenarioSpec, PhaseEdge, PhaseSpec,
    ResolvedWorkProfileSnapshot, RoleAuthority, RoleRef, RuntimeRoutingRef, SourceIdentity,
    SourceProcessingState, TeamRunSnapshot, TeamTemplateRevision, TriggerSpec, WorkProfileSpec,
};
use kontor_core::state::{
    DerivedRunState, DesiredRunState, Freshness, GateVerdict, NativeRuntimeIdentity,
    ObservedRunState, RunLifecycle, RuntimeContact, TaskState, TaskTeamClosure, TeamChildEvidence,
    TeamEvidenceSource, TeamTerminalEvidence, TerminalEvidence, TerminalEvidenceSource,
    TerminalOutcome,
};
use kontor_core::ticket::{
    ExternalCommentRevision, ExternalTicketObservation, ExternalWorkflowSpec, StatusConflict,
    StatusConflictKind, StatusSelector,
};
use kontor_store::{Applied, SqliteStore};
use rusqlite::Connection;
use tempfile::TempDir;

/// The two external-workflow fixtures. They describe the same semantic
/// milestones with entirely different external status ids and names, so a
/// persistence path that behaved differently for one of them would fail here.
const WORKFLOW_ASMA: &str =
    include_str!("../../kontor-core/tests/fixtures/external_workflow_asma.json");
const WORKFLOW_ALTERNATE: &str =
    include_str!("../../kontor-core/tests/fixtures/external_workflow_alternate.json");
/// An arbitrary, non-seed phase DAG.
const ARBITRARY_PROFILE: &str =
    include_str!("../../kontor-core/tests/fixtures/work_profile_arbitrary.json");
/// A persona scenario whose actor and evaluator are distinct.
const PERSONA_SCENARIO: &str =
    include_str!("../../kontor-core/tests/fixtures/persona_scenario.json");
/// A bounded, fully pinned trigger.
const TRIGGER_FIXTURE: &str = include_str!("../../kontor-core/tests/fixtures/trigger.json");

/// Every table a refused write could conceivably touch.
///
/// The census is deliberately whole-database rather than targeted: a rollback
/// that leaked a row into a table the test did not think about would still be
/// caught.
const CENSUS_TABLES: &[&str] = &[
    "account_profiles",
    "agent_runs",
    "calendar_exceptions",
    "calendar_profiles",
    "command_outbox",
    "command_receipts",
    "command_targets",
    "committee_re_review_claims",
    "context_packs",
    "epic_completion_remediation_command_claims",
    "epic_jira_transition_intents",
    "execution_authorization_tasks",
    "execution_authorizations",
    "external_comments",
    "external_ticket_observations",
    "external_workflow_specs",
    "guardrail_evaluations",
    "handoffs",
    "holiday_sources",
    "intake_receipts",
    "jira_links",
    "mini_projects",
    "persona_scenarios",
    "projects",
    "realm_metadata",
    "resource_leases",
    "runtime_bindings",
    "runtime_events",
    "runtime_reconciliation_epochs",
    "schedule_overrides",
    "source_events",
    "status_conflicts",
    "epic_status_conflicts",
    "status_transition_receipts",
    "task_dependencies",
    "task_gate_evaluations",
    "task_modules",
    "task_persona_snapshots",
    "task_workflows",
    "tasks",
    "team_runs",
    "team_templates",
    "ticket_field_specs",
    "ticket_sync_projections",
    "trigger_specs",
    "work_calendars",
    "work_profiles",
];

/// Count every row in the database, read through an independent connection.
///
/// Comparing a census taken before a refused write with one taken after is how
/// this suite proves "zero partial rows" rather than merely "an error was
/// returned".
fn census(fixture: &Fixture) -> BTreeMap<&'static str, i64> {
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    CENSUS_TABLES
        .iter()
        .map(|table| {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap_or_else(|_| panic!("`{table}` is countable"));
            (*table, count)
        })
        .collect()
}

/// The mutable state of one agent run, for before/after comparison.
fn run_state(
    fixture: &Fixture,
    run: AgentRunId,
) -> (AggregateRevision, DesiredRunState, TaskState) {
    let stored = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    let task = fixture
        .store
        .get_task(fixture.project, fixture.task)
        .expect("the read succeeds")
        .expect("the task exists");
    (stored.revision, stored.projection.desired, task.state)
}

/// Assert that a refused write changed nothing at all.
fn assert_unchanged(
    before: &BTreeMap<&'static str, i64>,
    after: &BTreeMap<&'static str, i64>,
    context: &str,
) {
    for (table, count) in before {
        assert_eq!(
            after.get(table),
            Some(count),
            "{context}: `{table}` row count changed after a refused write"
        );
    }
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn now() -> Timestamp {
    at("2026-08-09T10:00:00Z")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a valid external id")
}

fn phase(text: &str) -> PhaseKey {
    PhaseKey::parse(text).expect("a valid phase key")
}

fn role(text: &str) -> RoleKey {
    RoleKey::parse(text).expect("a valid role key")
}

fn artifact(text: &str) -> ArtifactKey {
    ArtifactKey::parse(text).expect("a valid artifact key")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker
    }))
    .expect("a canonical document")
}

/// A complete non-secret account profile. Every credential-bearing field is an
/// opaque alias or a canonical document, so the fixture itself demonstrates that
/// nothing resolvable is persisted.
fn account_profile(id: AccountProfileId, project_id: ProjectId, label: &str) -> NewAccountProfile {
    NewAccountProfile {
        id,
        project_id,
        label: name(label),
        external_account_id: Some(external("acct-1")),
        harness: RuntimeKindKey::parse("zz.runtime").expect("a valid runtime key"),
        credential_ref: CredentialReference {
            kind: CredentialReferenceKind::ConfigHome,
            alias: CredentialAlias::parse("zz-alpha").expect("a valid alias"),
        },
        environment: document("environment"),
        routing: document("routing"),
        capability: document("capability"),
        provider_identity: Some(external("provider-alpha")),
        enabled: true,
        created_at: now(),
    }
}

fn budget() -> BudgetBounds {
    BudgetBounds {
        max_tokens: 1_000,
        max_commands: 10,
        max_duration_seconds: 600,
        max_cost: Money {
            minor_units: 100,
            currency: CurrencyCode::parse("NOK").expect("a valid currency"),
        },
    }
}

/// A three-phase profile with one gate, built in code so this suite depends on
/// no particular fixture file.
fn work_profile() -> WorkProfileSpec {
    WorkProfileSpec {
        schema_version: SCHEMA_VERSION,
        id: WorkProfileKey::parse("zz.profile").expect("a valid profile key"),
        version: SpecVersion::FIRST,
        name: name("Round trip profile"),
        phases: vec![
            PhaseSpec {
                id: phase("zz.one"),
                label: name("One"),
                required_artifacts: vec![artifact("zz.output")],
                gates: Vec::new(),
                rejection_route: None,
            },
            PhaseSpec {
                id: phase("zz.two"),
                label: name("Two"),
                required_artifacts: Vec::new(),
                gates: vec![GateKey::parse("zz.gate").expect("a valid gate key")],
                rejection_route: Some(phase("zz.one")),
            },
            PhaseSpec {
                id: phase("zz.three"),
                label: name("Three"),
                required_artifacts: Vec::new(),
                gates: Vec::new(),
                rejection_route: None,
            },
        ],
        edges: vec![
            PhaseEdge {
                from: phase("zz.one"),
                to: phase("zz.two"),
                handoff_role: None,
            },
            PhaseEdge {
                from: phase("zz.two"),
                to: phase("zz.three"),
                handoff_role: None,
            },
        ],
        entry_phase: phase("zz.one"),
        terminal_phases: vec![phase("zz.three")],
        roles: vec![
            RoleRef {
                role: role("zz.maker"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("zz.reviewer"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("zz.waiver"),
                version: SpecVersion::FIRST,
            },
        ],
        skills: Vec::new(),
        team_template: None,
        artifacts: vec![ArtifactContractSpec {
            key: artifact("zz.output"),
            label: name("Output"),
            producer_phase: phase("zz.one"),
            content_type: ArtifactContentType::Report,
            evidence_required: true,
        }],
        gates: vec![GateSpec {
            id: GateKey::parse("zz.gate").expect("a valid gate key"),
            phase: phase("zz.two"),
            evaluator_roles: vec![role("zz.reviewer")],
            required_evidence: vec![artifact("zz.output")],
            rejection_target: phase("zz.one"),
            waiver_allowed: true,
            waiver_roles: vec![role("zz.waiver")],
        }],
        runtime_routing: RuntimeRoutingRef {
            runtime_kind: RuntimeKindKey::parse("zz.runtime").expect("a valid runtime key"),
            version: SpecVersion::FIRST,
        },
        budget_defaults: budget(),
        calendar_policy: None,
        external_workflow: None,
        context_window: None,
    }
}

fn identity(generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("zz.runtime").expect("a valid runtime key"),
        host: name("host-1"),
        generation,
        native_id: external("session-1"),
    }
}

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
    other_project: ProjectId,
    task: TaskId,
    other_task: TaskId,
    account: AccountProfileId,
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");

    let project = ProjectId::generate();
    let other_project = ProjectId::generate();
    for (id, suffix) in [(project, "a"), (other_project, "b")] {
        store
            .create_project(&NewProject {
                id,
                name: name(&format!("Project {suffix}")),
                root_path: name(&format!("/tmp/project-{suffix}")),
                created_at: now(),
            })
            .expect("a project is created");
    }

    let task = TaskId::generate();
    let other_task = TaskId::generate();
    for (id, owner) in [(task, project), (other_task, other_project)] {
        store
            .create_task(&NewTask {
                id,
                project_id: owner,
                mini_project_id: None,
                title: name("A task"),
                module: None,
                state: TaskState::Ready,
                created_at: now(),
            })
            .expect("a task is created");
    }

    let account = AccountProfileId::generate();
    store
        .create_account_profile(&account_profile(account, project, "Account"))
        .expect("an account profile is created");

    Fixture {
        _directory: directory,
        path,
        store,
        project,
        other_project,
        task,
        other_task,
        account,
    }
}

// ---------------------------------------------------------------------------
// Project scoping
// ---------------------------------------------------------------------------

#[test]
fn a_valid_id_from_another_project_never_resolves() {
    let fixture = fixture();

    assert!(
        fixture
            .store
            .get_task(fixture.project, fixture.task)
            .expect("the read succeeds")
            .is_some()
    );
    assert!(
        fixture
            .store
            .get_task(fixture.project, fixture.other_task)
            .expect("the read succeeds")
            .is_none(),
        "a globally valid id from another project must not resolve"
    );
    assert_eq!(
        fixture
            .store
            .list_tasks(fixture.project)
            .expect("the read succeeds")
            .len(),
        1
    );

    // The same holds for writes: a cross-project dependency is refused.
    let error = fixture
        .store
        .set_task_dependencies(fixture.project, fixture.task, &[fixture.other_task])
        .expect_err("a cross-project dependency must be refused");
    assert!(matches!(error, RepositoryError::Conflict { .. }));
    assert!(
        fixture
            .store
            .set_task_dependencies(fixture.project, fixture.task, &[])
            .is_ok()
    );
}

#[test]
fn a_dependency_cycle_is_refused_and_writes_nothing() {
    let fixture = fixture();
    let second = TaskId::generate();
    fixture
        .store
        .create_task(&NewTask {
            id: second,
            project_id: fixture.project,
            mini_project_id: None,
            title: name("Second"),
            module: None,
            state: TaskState::Ready,
            created_at: now(),
        })
        .expect("a second task is created");

    fixture
        .store
        .set_task_dependencies(fixture.project, second, &[fixture.task])
        .expect("a forward edge is accepted");

    let before = census(&fixture);
    assert_eq!(before.get("task_dependencies"), Some(&1));

    // Closing the loop must be refused.
    let error = fixture
        .store
        .set_task_dependencies(fixture.project, fixture.task, &[second])
        .expect_err("a cycle must be refused");
    assert!(matches!(error, RepositoryError::Domain(_)));

    // The refused call deletes and re-inserts before it validates, so this is
    // also a rollback assertion: the accepted edge must still be there and no
    // new one may have appeared.
    assert_unchanged(&before, &census(&fixture), "dependency cycle");

    // And it must leave the accepted edge alone.
    fixture
        .store
        .set_task_dependencies(fixture.project, second, &[fixture.task])
        .expect("the earlier edge is still valid");
    assert_unchanged(&before, &census(&fixture), "re-applied dependency");

    let self_dep_before = census(&fixture);
    assert!(
        fixture
            .store
            .set_task_dependencies(fixture.project, fixture.task, &[fixture.task])
            .is_err(),
        "a self dependency must be refused"
    );
    assert_unchanged(&self_dep_before, &census(&fixture), "self dependency");
}

// ---------------------------------------------------------------------------
// Specifications and snapshots across a restart
// ---------------------------------------------------------------------------

#[test]
fn specifications_and_snapshots_survive_a_restart_byte_for_byte() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let project = ProjectId::generate();
    let task = TaskId::generate();
    let template = TeamTemplateId::generate();
    let team_run = TeamRunId::generate();
    let workflow = TaskWorkflowId::generate();
    let profile = work_profile();
    let profile_hash;

    {
        let store = SqliteStore::open(&path).expect("the store opens");
        store
            .create_project(&NewProject {
                id: project,
                name: name("Project"),
                root_path: name("/tmp/project"),
                created_at: now(),
            })
            .expect("a project is created");
        store
            .create_task(&NewTask {
                id: task,
                project_id: project,
                mini_project_id: None,
                title: name("A task"),
                module: None,
                state: TaskState::Ready,
                created_at: now(),
            })
            .expect("a task is created");

        profile_hash = store
            .insert_work_profile(project, &profile)
            .expect("the profile revision is stored");

        // A duplicate (id, version) is impossible.
        assert!(
            store.insert_work_profile(project, &profile).is_err(),
            "an immutable revision may not be replaced"
        );

        store
            .insert_team_template(
                project,
                &TeamTemplateRevision {
                    template_id: template,
                    version: SpecVersion::FIRST,
                    name: name("Team"),
                    definition: document("team"),
                    role_authority: vec![RoleAuthority {
                        role: role("zz.reviewer"),
                        may_evaluate: vec![GateKey::parse("zz.gate").expect("a valid gate key")],
                        may_waive: Vec::new(),
                    }],
                },
            )
            .expect("the team revision is stored");

        let snapshot =
            ResolvedWorkProfileSnapshot::resolve(&profile, now()).expect("the profile resolves");
        store
            .create_task_workflow(&NewTaskWorkflow {
                id: workflow,
                project_id: project,
                task_id: task,
                snapshot,
                current_phase: phase("zz.one"),
                created_at: now(),
            })
            .expect("the workflow is created");

        let revision = store
            .get_team_template(project, template, SpecVersion::FIRST)
            .expect("the read succeeds")
            .expect("the revision exists");
        store
            .create_team_run(&NewTeamRun {
                id: team_run,
                project_id: project,
                task_id: task,
                snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
                created_at: now(),
            })
            .expect("the team run is created");
    }

    // Restart.
    let store = SqliteStore::open(&path).expect("the store reopens");
    let restored = store
        .get_work_profile(project, &profile.id, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("the revision exists");
    assert_eq!(restored, profile);
    assert_eq!(
        restored.canonicalize().expect("canonicalizes").hash(),
        &profile_hash,
        "the reopened definition must hash identically"
    );

    let reopened = store
        .get_active_task_workflow(project, task)
        .expect("the read succeeds")
        .expect("the workflow exists");
    assert_eq!(reopened.snapshot.definition, profile);
    reopened
        .snapshot
        .verify()
        .expect("the reopened snapshot still matches its pinned digest");

    let run = store
        .get_team_run(project, team_run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert_eq!(run.snapshot.template_id, template);
    assert_eq!(run.snapshot.definition.hash(), document("team").hash());
    assert!(run.snapshot.may_evaluate(
        &role("zz.reviewer"),
        &GateKey::parse("zz.gate").expect("a valid gate key")
    ));

    // Another project cannot read either of them.
    let stranger = ProjectId::generate();
    assert!(
        store
            .get_work_profile(stranger, &profile.id, SpecVersion::FIRST)
            .expect("the read succeeds")
            .is_none()
    );
    assert!(
        store
            .get_team_run(stranger, team_run)
            .expect("the read succeeds")
            .is_none()
    );
}

#[test]
fn both_external_workflow_fixtures_persist_reopen_and_hash_identically() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let project = ProjectId::generate();

    let fixtures: Vec<ExternalWorkflowSpec> = vec![
        serde_json::from_str(WORKFLOW_ASMA).expect("the first workflow fixture parses"),
        serde_json::from_str(WORKFLOW_ALTERNATE).expect("the second workflow fixture parses"),
    ];
    // The two fixtures must genuinely disagree about every external spelling,
    // otherwise this test would prove nothing about name independence.
    let first_ids: BTreeSet<&str> = fixtures[0]
        .statuses
        .iter()
        .map(|status| status.selector.status_id.as_str())
        .collect();
    let second_ids: BTreeSet<&str> = fixtures[1]
        .statuses
        .iter()
        .map(|status| status.selector.status_id.as_str())
        .collect();
    assert!(
        first_ids.is_disjoint(&second_ids),
        "the fixtures must share no external status id"
    );
    assert_ne!(fixtures[0].connector, fixtures[1].connector);

    let mut inserted_hashes = Vec::new();
    {
        let store = SqliteStore::open(&path).expect("the store opens");
        store
            .create_project(&NewProject {
                id: project,
                name: name("Project"),
                root_path: name("/tmp/project"),
                created_at: now(),
            })
            .expect("a project is created");

        for spec in &fixtures {
            // A profile-specific mapping pins a work-profile revision, and that
            // pin is a foreign key: the revision has to exist in this project
            // first.
            if let (Some(key), Some(version)) =
                (spec.work_profile.as_ref(), spec.work_profile_version)
            {
                let mut profile: WorkProfileSpec =
                    serde_json::from_str(ARBITRARY_PROFILE).expect("the profile fixture parses");
                profile.id = key.clone();
                profile.version = version;
                store
                    .insert_work_profile(project, &profile)
                    .expect("the pinned profile revision is stored");
            }
            inserted_hashes.push(
                store
                    .insert_external_workflow_spec(project, spec)
                    .expect("the workflow revision is stored"),
            );
            // The revision is immutable: the same (connector, project, issue
            // type, version) cannot be written twice.
            assert!(
                store.insert_external_workflow_spec(project, spec).is_err(),
                "an immutable workflow revision may not be replaced"
            );
        }
    }

    // Reopen through a completely fresh connection.
    let store = SqliteStore::open(&path).expect("the store reopens");
    for (spec, inserted_hash) in fixtures.iter().zip(&inserted_hashes) {
        let selector = ConnectorSpecSelector {
            project_id: project,
            connector: spec.connector.clone(),
            project: spec.project.clone(),
            issue_type: spec.issue_type.clone(),
            version: spec.version,
        };
        let reopened = store
            .get_external_workflow_spec(&selector)
            .expect("the read succeeds")
            .expect("the revision exists");

        assert_eq!(&reopened, spec, "the reopened definition must be identical");
        let recomputed = reopened.canonicalize().expect("canonicalizes");
        assert_eq!(
            recomputed.hash(),
            inserted_hash,
            "the reopened definition must hash to the digest recorded at insert"
        );
        // The reader also verifies the stored bytes against the stored digest,
        // so a reordered or re-indented copy would already have been refused.
        assert_eq!(
            recomputed.json(),
            spec.canonicalize().expect("canonicalizes").json()
        );

        // Another project cannot reach either revision.
        let stranger = ConnectorSpecSelector {
            project_id: ProjectId::generate(),
            ..selector
        };
        assert!(
            store
                .get_external_workflow_spec(&stranger)
                .expect("the read succeeds")
                .is_none()
        );
    }

    // The two digests differ, which is what proves the store did not collapse
    // two differently named workflows into one row.
    assert_ne!(inserted_hashes[0], inserted_hashes[1]);
}

#[test]
fn workflow_installation_rolls_back_when_its_authority_receipt_is_refused() {
    let fixture = fixture();
    let workflow: ExternalWorkflowSpec =
        serde_json::from_str(WORKFLOW_ALTERNATE).expect("the workflow fixture parses");
    let occupied_receipt = CommandReceiptId::generate();
    let occupied_intent = document("occupied receipt");
    fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id: occupied_receipt,
            idempotency_key: IdempotencyKey::parse("workflow-occupied-receipt")
                .expect("a valid key"),
            kind: CommandKind::TransitionTask,
            target: AggregateRef::Task {
                task_id: fixture.task,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: occupied_intent.clone(),
            payload: occupied_intent,
            desired: None,
            not_before: now(),
            created_at: now(),
        })
        .expect("the colliding receipt id is occupied first");
    let key = IdempotencyKey::parse("workflow-atomic-refusal").expect("a valid key");
    let intent = document("workflow install");
    let command = ReceiptEnvelope::new(
        fixture.store.realm(),
        NewCommandIntent {
            project_id: fixture.project,
            receipt_id: occupied_receipt,
            idempotency_key: key.clone(),
            // The operation is valid but its receipt id is already occupied.
            // That storage refusal occurs after the policy write begins inside
            // the transaction and must roll the policy and revision back.
            kind: CommandKind::InstallWorkflowSpec,
            target: AggregateRef::Project {
                project_id: fixture.project,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: intent.clone(),
            payload: intent,
            desired: None,
            not_before: now(),
            created_at: now(),
        },
    );
    let before = census(&fixture);

    fixture
        .store
        .install_external_workflow_spec_with_intent(
            fixture.project,
            AggregateRevision::INITIAL,
            &workflow,
            &command,
        )
        .expect_err("a workflow policy without its Admin receipt is refused atomically");

    assert_unchanged(
        &before,
        &census(&fixture),
        "a refused atomic workflow installation",
    );
    let project = fixture
        .store
        .get_project(fixture.project)
        .expect("the project read succeeds")
        .expect("the project exists");
    assert_eq!(project.revision, AggregateRevision::INITIAL);
    assert!(
        fixture
            .store
            .get_receipt_by_key(&key)
            .expect("the receipt lookup succeeds")
            .is_none()
    );
    assert!(
        fixture
            .store
            .get_external_workflow_spec(&ConnectorSpecSelector {
                project_id: fixture.project,
                connector: workflow.connector,
                project: workflow.project,
                issue_type: workflow.issue_type,
                version: workflow.version,
            })
            .expect("the workflow lookup succeeds")
            .is_none()
    );
}

#[test]
fn stale_atomic_withdrawal_leaves_no_receipt_and_preserves_the_moved_task() {
    let fixture = fixture();
    let transition = |to, expected_revision| TaskTransitionRequest {
        project_id: fixture.project,
        task_id: fixture.task,
        expected_revision,
        to,
        resume_receipt: None,
        reopen: false,
        run_outcome: None,
        produced_artifacts: BTreeSet::new(),
        completed_phases: BTreeSet::new(),
        team_closure: TaskTeamClosure::NoTeam,
        occurred_at: now(),
    };
    let moved = fixture
        .store
        .transition_task(&transition(TaskState::Blocked, AggregateRevision::INITIAL))
        .expect("a concurrent writer moves the task");
    let key = IdempotencyKey::parse("withdraw-stale-atomic").expect("a valid key");
    let intent = document("withdraw task");
    let command = ReceiptEnvelope::new(
        fixture.store.realm(),
        NewCommandIntent {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: key.clone(),
            kind: CommandKind::WithdrawTask,
            target: AggregateRef::Task {
                task_id: fixture.task,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: intent.clone(),
            payload: intent,
            desired: None,
            not_before: now(),
            created_at: now(),
        },
    );
    let before = census(&fixture);

    fixture
        .store
        .transition_task_with_intent(
            &transition(TaskState::Withdrawn, AggregateRevision::INITIAL),
            &command,
        )
        .expect_err("the stale withdrawal is refused");

    assert_unchanged(&before, &census(&fixture), "a stale atomic withdrawal");
    assert!(
        fixture
            .store
            .get_receipt_by_key(&key)
            .expect("the receipt lookup succeeds")
            .is_none()
    );
    let preserved = fixture
        .store
        .get_task(fixture.project, fixture.task)
        .expect("the task read succeeds")
        .expect("the task exists");
    assert_eq!(preserved.state, TaskState::Blocked);
    assert_eq!(preserved.revision, moved.revision);
}

#[test]
fn the_arbitrary_profile_and_persona_fixtures_persist_reopen_and_hash_identically() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let project = ProjectId::generate();

    let profile: WorkProfileSpec =
        serde_json::from_str(ARBITRARY_PROFILE).expect("the profile fixture parses");
    let persona: PersonaScenarioSpec =
        serde_json::from_str(PERSONA_SCENARIO).expect("the persona fixture parses");
    let (profile_hash, persona_hash);

    {
        let store = SqliteStore::open(&path).expect("the store opens");
        store
            .create_project(&NewProject {
                id: project,
                name: name("Project"),
                root_path: name("/tmp/project"),
                created_at: now(),
            })
            .expect("a project is created");
        profile_hash = store
            .insert_work_profile(project, &profile)
            .expect("the arbitrary profile is stored");
        persona_hash = store
            .insert_persona_scenario(project, &persona)
            .expect("the persona scenario is stored");
    }

    let store = SqliteStore::open(&path).expect("the store reopens");
    let reopened_profile = store
        .get_work_profile(project, &profile.id, profile.version)
        .expect("the read succeeds")
        .expect("the revision exists");
    assert_eq!(reopened_profile, profile);
    assert_eq!(
        reopened_profile
            .canonicalize()
            .expect("canonicalizes")
            .hash(),
        &profile_hash
    );

    let reopened_persona = store
        .get_persona_scenario(project, persona.scenario_id, persona.version)
        .expect("the read succeeds")
        .expect("the revision exists");
    assert_eq!(reopened_persona, persona);
    assert_eq!(
        reopened_persona
            .canonicalize()
            .expect("canonicalizes")
            .hash(),
        &persona_hash
    );

    // A resolved snapshot of the arbitrary DAG also survives the restart with
    // its pinned digest intact.
    let snapshot = ResolvedWorkProfileSnapshot::resolve(&reopened_profile, now())
        .expect("the profile resolves");
    snapshot.verify().expect("the snapshot matches its digest");
    assert_eq!(&snapshot.definition_hash, &profile_hash);
}

// ---------------------------------------------------------------------------
// Gates, phases and task closure
// ---------------------------------------------------------------------------

fn with_workflow(fixture: &Fixture) -> TaskWorkflowId {
    let profile = work_profile();
    // The revision is immutable, so a second call simply reuses the one already
    // stored rather than failing the fixture.
    if fixture
        .store
        .get_work_profile(fixture.project, &profile.id, profile.version)
        .expect("the read succeeds")
        .is_none()
    {
        fixture
            .store
            .insert_work_profile(fixture.project, &profile)
            .expect("the profile is stored");
    }
    let snapshot =
        ResolvedWorkProfileSnapshot::resolve(&profile, now()).expect("the profile resolves");
    let workflow = TaskWorkflowId::generate();
    fixture
        .store
        .create_task_workflow(&NewTaskWorkflow {
            id: workflow,
            project_id: fixture.project,
            task_id: fixture.task,
            snapshot,
            current_phase: phase("zz.one"),
            created_at: now(),
        })
        .expect("the workflow is created");
    workflow
}

#[test]
fn a_gate_may_only_be_decided_by_the_authority_the_profile_names() {
    let fixture = fixture();
    let workflow = with_workflow(&fixture);
    let gate = GateKey::parse("zz.gate").expect("a valid gate key");

    let evaluation =
        |role_key: &str, verdict: GateVerdict, evidence: Vec<ArtifactKey>| NewGateEvaluation {
            project_id: fixture.project,
            workflow_id: workflow,
            gate: gate.clone(),
            verdict,
            evaluator_role: role(role_key),
            evaluator_account: fixture.account,
            evidence,
            agent_run_id: None,
            session_evidence: None,
            reviewer_principal: None,
            policy_evaluation_id: None,
            recorded_at: now(),
        };

    // The maker is not an evaluator.
    assert!(
        fixture
            .store
            .append_gate_evaluation(&evaluation(
                "zz.maker",
                GateVerdict::Passed,
                vec![artifact("zz.output")]
            ))
            .is_err()
    );
    // The evaluator cannot waive; only the distinct waiver authority can.
    assert!(
        fixture
            .store
            .append_gate_evaluation(&evaluation(
                "zz.reviewer",
                GateVerdict::Waived,
                vec![artifact("zz.output")]
            ))
            .is_err()
    );
    // A pass needs the evidence the profile requires.
    assert!(
        fixture
            .store
            .append_gate_evaluation(&evaluation("zz.reviewer", GateVerdict::Passed, Vec::new()))
            .is_err()
    );

    assert_eq!(
        fixture
            .store
            .append_gate_evaluation(&evaluation("zz.reviewer", GateVerdict::Started, Vec::new()))
            .expect("starting a gate is legal"),
        1
    );
    assert_eq!(
        fixture
            .store
            .append_gate_evaluation(&evaluation(
                "zz.reviewer",
                GateVerdict::Passed,
                vec![artifact("zz.output")]
            ))
            .expect("an authorized, evidenced pass is legal"),
        2,
        "evaluations are append-only and numbered"
    );

    let history = fixture
        .store
        .list_gate_evaluations(fixture.project, workflow)
        .expect("the read succeeds");
    assert_eq!(history.len(), 2);
    let states = fixture
        .store
        .gate_states(fixture.project, workflow)
        .expect("the read succeeds");
    assert_eq!(
        states.get(&gate).copied(),
        Some(kontor_core::state::GateState::Passed)
    );
}

#[test]
fn a_phase_advances_only_along_a_declared_edge_and_only_under_a_compare_and_swap() {
    let fixture = fixture();
    let workflow = with_workflow(&fixture);

    // A phase the profile does not connect to.
    assert!(
        fixture
            .store
            .advance_phase(&PhaseAdvance {
                project_id: fixture.project,
                workflow_id: workflow,
                expected_revision: AggregateRevision::INITIAL,
                next_phase: phase("zz.three"),
                advanced_at: now(),
            })
            .is_err()
    );

    let second = fixture
        .store
        .advance_phase(&PhaseAdvance {
            project_id: fixture.project,
            workflow_id: workflow,
            expected_revision: AggregateRevision::INITIAL,
            next_phase: phase("zz.two"),
            advanced_at: now(),
        })
        .expect("a declared edge is followed");
    assert_eq!(second.get(), 2);

    // The stale expectation is refused.
    let error = fixture
        .store
        .advance_phase(&PhaseAdvance {
            project_id: fixture.project,
            workflow_id: workflow,
            expected_revision: AggregateRevision::INITIAL,
            next_phase: phase("zz.three"),
            advanced_at: now(),
        })
        .expect_err("a stale revision must be refused");
    assert!(matches!(error, RepositoryError::Domain(_)));
}

#[test]
fn a_task_closes_only_when_its_pinned_profile_says_it_may() {
    let fixture = fixture();
    let workflow = with_workflow(&fixture);
    let gate = GateKey::parse("zz.gate").expect("a valid gate key");

    let request = |to: TaskState, revision: AggregateRevision| TaskTransitionRequest {
        project_id: fixture.project,
        task_id: fixture.task,
        expected_revision: revision,
        to,
        resume_receipt: None,
        reopen: false,
        run_outcome: None,
        produced_artifacts: [artifact("zz.output")].into_iter().collect(),
        completed_phases: [phase("zz.one"), phase("zz.two"), phase("zz.three")]
            .into_iter()
            .collect(),
        // This fixture's profile pins no team, so there are no role slots to
        // answer for.
        team_closure: TaskTeamClosure::NoTeam,
        occurred_at: now(),
    };

    let task = fixture
        .store
        .transition_task(&request(TaskState::InProgress, AggregateRevision::INITIAL))
        .expect("the task starts");
    assert_eq!(task.state, TaskState::InProgress);

    // The gate has not passed yet.
    assert!(
        fixture
            .store
            .transition_task(&request(TaskState::Done, task.revision))
            .is_err(),
        "an outstanding gate must block closure"
    );

    fixture
        .store
        .append_gate_evaluation(&NewGateEvaluation {
            project_id: fixture.project,
            workflow_id: workflow,
            gate,
            verdict: GateVerdict::Passed,
            evaluator_role: role("zz.reviewer"),
            evaluator_account: fixture.account,
            evidence: vec![artifact("zz.output")],
            agent_run_id: None,
            session_evidence: None,
            reviewer_principal: None,
            policy_evaluation_id: None,
            recorded_at: now(),
        })
        .expect("the gate passes");

    // A missing artifact still blocks it.
    let mut incomplete = request(TaskState::Done, task.revision);
    incomplete.produced_artifacts = BTreeSet::new();
    assert!(fixture.store.transition_task(&incomplete).is_err());

    let closed = fixture
        .store
        .transition_task(&request(TaskState::Done, task.revision))
        .expect("a certified task closes");
    assert_eq!(closed.state, TaskState::Done);

    // A terminal task is immutable, in Rust and in SQL.
    assert!(
        fixture
            .store
            .transition_task(&request(TaskState::Ready, closed.revision))
            .is_err()
    );
}

// ---------------------------------------------------------------------------
// Runs, observations and closure
// ---------------------------------------------------------------------------

struct RunFixture {
    fixture: Fixture,
    run: AgentRunId,
}

fn with_run(binding: bool) -> RunFixture {
    let fixture = fixture();
    let profile = work_profile();
    fixture
        .store
        .insert_work_profile(fixture.project, &profile)
        .expect("the profile is stored");
    let template = TeamTemplateId::generate();
    fixture
        .store
        .insert_team_template(
            fixture.project,
            &TeamTemplateRevision {
                template_id: template,
                version: SpecVersion::FIRST,
                name: name("Team"),
                definition: document("team"),
                role_authority: Vec::new(),
            },
        )
        .expect("the team revision is stored");
    let revision = fixture
        .store
        .get_team_template(fixture.project, template, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("the revision exists");
    let team_run = TeamRunId::generate();
    fixture
        .store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: fixture.project,
            task_id: fixture.task,
            snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
            created_at: now(),
        })
        .expect("the team run is created");

    let run = AgentRunId::generate();
    fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: run,
            project_id: fixture.project,
            team_run_id: team_run,
            parent_agent_run_id: None,
            role: role("zz.maker"),
            account_profile_id: Some(fixture.account),
            binding: binding.then(|| kontor_core::repository::RuntimeBinding {
                id: kontor_core::id::RuntimeBindingId::generate(),
                agent_run_id: run,
                identity: identity(1),
                bound_at: now(),
            }),
            created_at: now(),
        })
        .expect("the agent run is created");

    RunFixture { fixture, run }
}

fn event(
    fixture: &Fixture,
    run: AgentRunId,
    native: Option<&str>,
    marker: &str,
) -> NewRuntimeEvent {
    sequenced_event(fixture, run, native, marker, 1)
}

fn sequenced_event(
    fixture: &Fixture,
    run: AgentRunId,
    native: Option<&str>,
    marker: &str,
    native_sequence: u64,
) -> NewRuntimeEvent {
    NewRuntimeEvent {
        project_id: fixture.project,
        agent_run_id: run,
        identity: identity(1),
        native_event_id: native.map(external),
        native_sequence,
        payload: document(marker),
        observed_at: now(),
    }
}

/// Append a terminal observation and return the evidence that closes the run
/// with it.
///
/// Closure evidence is a pointer into stored rows, so a test must actually
/// store the row it cites — which is the whole point of the binding rule.
fn runtime_closure(
    fixture: &Fixture,
    run: AgentRunId,
    outcome: TerminalOutcome,
    observed: ObservedRunState,
    marker: &str,
    native_sequence: u64,
) -> TerminalEvidence {
    let current = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    fixture
        .store
        .record_observation(&NewObservation {
            event: sequenced_event(fixture, run, Some(marker), marker, native_sequence),
            observed,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: current.revision,
            quota_state: None,
        })
        .expect("the terminal observation is recorded");
    let stored = fixture
        .store
        .read_runtime_events(fixture.project, run, None)
        .expect("the read succeeds")
        .into_iter()
        .next_back()
        .expect("the event exists");
    TerminalEvidence {
        outcome,
        source: TerminalEvidenceSource::RuntimeObservation {
            cursor: stored.cursor,
        },
        evidence_hash: stored.payload.hash().clone(),
        closed_at: at("2026-08-09T11:00:00Z"),
    }
}

#[test]
fn a_raw_event_is_appended_before_state_is_reduced_and_replays_are_idempotent() {
    let RunFixture { fixture, run } = with_run(true);

    let first = fixture
        .store
        .append_runtime_event(&event(&fixture, run, Some("n-1"), "first"))
        .expect("the event appends");
    let replay = fixture
        .store
        .append_runtime_event(&event(&fixture, run, Some("n-1"), "first"))
        .expect("a replay is not an error");
    assert_eq!(first, replay, "a replayed event keeps its original cursor");

    let events = fixture
        .store
        .read_runtime_events(fixture.project, run, None)
        .expect("the read succeeds");
    assert_eq!(events.len(), 1, "a replay must not append twice");

    // Resuming strictly after a cursor never repeats it.
    assert!(
        fixture
            .store
            .read_runtime_events(fixture.project, run, Some(first))
            .expect("the read succeeds")
            .is_empty()
    );

    // Confirmation requires intent: an unrequested session is divergence, not
    // confirmation.
    fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: IdempotencyKey::parse("launch-1").expect("a valid key"),
            kind: CommandKind::LaunchRun,
            target: AggregateRef::AgentRun { agent_run_id: run },
            target_revision: AggregateRevision::INITIAL,
            intent: document("launch"),
            payload: document("launch-payload"),
            desired: Some(DesiredRunState::RunRequested),
            not_before: now(),
            created_at: now(),
        })
        .expect("the intent is recorded");
    let current = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    let projection = fixture
        .store
        .record_observation(&NewObservation {
            event: event(&fixture, run, Some("n-2"), "second"),
            observed: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: current.revision,
            quota_state: None,
        })
        .expect("the observation is recorded");
    assert_eq!(projection.observed, ObservedRunState::Running);
    assert_eq!(projection.derived, DerivedRunState::Confirmed);
    assert_eq!(projection.lifecycle, RunLifecycle::Running);
    assert_eq!(
        fixture
            .store
            .get_team_run(fixture.project, current.team_run_id)
            .expect("the team reads")
            .expect("the team exists")
            .lifecycle,
        RunLifecycle::Running,
        "one observation transaction advances the child and its TeamRun"
    );
    assert_eq!(
        fixture
            .store
            .read_runtime_events(fixture.project, run, None)
            .expect("the read succeeds")
            .len(),
        2,
        "the raw event is stored before the state is reduced from it"
    );
}

#[test]
fn a_disappeared_process_never_closes_a_run() {
    let RunFixture { fixture, run } = with_run(true);

    let projection = fixture
        .store
        .record_observation(&NewObservation {
            event: event(&fixture, run, Some("n-1"), "gone"),
            observed: ObservedRunState::Running,
            contact: RuntimeContact::ProcessMissing,
            freshness: Freshness::Fresh,
            expected_revision: AggregateRevision::INITIAL,
            quota_state: None,
        })
        .expect("the observation is recorded");
    assert_eq!(projection.derived, DerivedRunState::LostContact);
    assert!(!projection.derived.is_terminal());
    assert!(!projection.lifecycle.is_terminal());

    let stored = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(stored.terminal.is_none());
    assert_eq!(stored.projection.derived, DerivedRunState::LostContact);
    assert_eq!(
        stored.projection.observed,
        ObservedRunState::Running,
        "the observation is retained, not overwritten by the conclusion"
    );
}

#[test]
fn desired_observed_and_derived_all_survive_a_restart_with_different_values() {
    let RunFixture { fixture, run } = with_run(true);

    fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: IdempotencyKey::parse("cancel-1").expect("a valid key"),
            kind: CommandKind::CancelRun,
            target: AggregateRef::AgentRun { agent_run_id: run },
            target_revision: AggregateRevision::INITIAL,
            intent: document("cancel"),
            payload: document("cancel-payload"),
            desired: Some(DesiredRunState::CancelRequested),
            not_before: now(),
            created_at: now(),
        })
        .expect("the intent is recorded");

    let current = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    let projection = fixture
        .store
        .record_observation(&NewObservation {
            event: event(&fixture, run, Some("n-1"), "still-running"),
            observed: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: current.revision,
            quota_state: None,
        })
        .expect("the observation is recorded");

    // We asked for a cancel, the runtime says it is running, and we may only
    // conclude that they disagree. Three dimensions, three different values.
    assert_eq!(projection.desired, DesiredRunState::CancelRequested);
    assert_eq!(projection.observed, ObservedRunState::Running);
    assert_eq!(projection.derived, DerivedRunState::Diverged);
    assert_eq!(
        projection.lifecycle,
        kontor_core::state::RunLifecycle::Running
    );

    // Reopen the same file through a fresh connection: every dimension is still
    // stored separately.
    let reopened = SqliteStore::open(&fixture.path).expect("the store reopens");
    let restored = reopened
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert_eq!(
        restored.projection.desired,
        DesiredRunState::CancelRequested
    );
    assert_eq!(restored.projection.observed, ObservedRunState::Running);
    assert_eq!(restored.projection.derived, DerivedRunState::Diverged);
    assert_eq!(
        restored.projection.lifecycle,
        kontor_core::state::RunLifecycle::Running
    );
    assert!(restored.terminal.is_none());
    assert!(restored.projection.last_cursor.is_some());
}

#[test]
fn a_closed_run_is_immutable_and_recovery_creates_a_successor() {
    let RunFixture { fixture, run } = with_run(true);
    let evidence = runtime_closure(
        &fixture,
        run,
        TerminalOutcome::Failed,
        ObservedRunState::Failed,
        "terminal",
        5,
    );
    let stored = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");

    fixture
        .store
        .close_agent_run(&RunClosure {
            project_id: fixture.project,
            agent_run_id: run,
            expected_revision: stored.revision,
            evidence: evidence.clone(),
        })
        .expect("an evidenced closure succeeds");

    let closed = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(closed.projection.is_closed());
    assert_eq!(
        closed.projection.derived,
        DerivedRunState::Terminal {
            outcome: TerminalOutcome::Failed
        }
    );
    assert!(closed.closed_at.is_some());

    // Closing twice, observing again, or closing with the old revision all fail.
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: closed.revision,
                evidence,
            })
            .is_err()
    );
    assert!(
        fixture
            .store
            .record_observation(&NewObservation {
                event: event(&fixture, run, Some("n-late"), "late"),
                observed: ObservedRunState::Running,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: closed.revision,
                quota_state: None,
            })
            .is_err(),
        "a closed run accepts no further observation"
    );

    // Recovery is a successor run, never a reopen.
    let successor = AgentRunId::generate();
    fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: successor,
            project_id: fixture.project,
            team_run_id: closed.team_run_id,
            parent_agent_run_id: Some(run),
            role: role("zz.maker"),
            account_profile_id: Some(fixture.account),
            binding: None,
            created_at: at("2026-08-09T11:05:00Z"),
        })
        .expect("a successor run is created");
    let successor = fixture
        .store
        .get_agent_run(fixture.project, successor)
        .expect("the read succeeds")
        .expect("the successor exists");
    assert_eq!(successor.parent_agent_run_id, Some(run));

    // A dangling parent is refused.
    assert!(
        fixture
            .store
            .create_agent_run(&NewAgentRun {
                id: AgentRunId::generate(),
                project_id: fixture.project,
                team_run_id: closed.team_run_id,
                parent_agent_run_id: Some(AgentRunId::generate()),
                role: role("zz.maker"),
                account_profile_id: None,
                binding: None,
                created_at: now(),
            })
            .is_err()
    );
}

#[test]
fn every_refused_write_leaves_zero_partial_rows_revisions_events_or_outbox_entries() {
    let RunFixture { fixture, run } = with_run(true);
    let existing = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");

    let before = census(&fixture);
    let state_before = run_state(&fixture, run);

    // 1. A dangling parent: refused before anything is written.
    assert!(
        fixture
            .store
            .create_agent_run(&NewAgentRun {
                id: AgentRunId::generate(),
                project_id: fixture.project,
                team_run_id: existing.team_run_id,
                parent_agent_run_id: Some(AgentRunId::generate()),
                role: role("zz.maker"),
                account_profile_id: None,
                binding: None,
                created_at: now(),
            })
            .is_err()
    );
    assert_unchanged(&before, &census(&fixture), "dangling parent agent run");

    // 2. A binding that collides with an existing native identity. This one
    //    fails *after* the agent_runs row has already been inserted inside the
    //    transaction, so it proves the row is rolled back rather than merely
    //    that an error surfaced.
    let collision = fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: AgentRunId::generate(),
            project_id: fixture.project,
            team_run_id: existing.team_run_id,
            parent_agent_run_id: None,
            role: role("zz.maker"),
            account_profile_id: None,
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: AgentRunId::generate(),
                // The same (kind, host, generation, native id) the existing
                // run is already bound to.
                identity: identity(1),
                bound_at: now(),
            }),
            created_at: now(),
        })
        .expect_err("a colliding native identity must be refused");
    // The failure is the binding's unique index, which is reached only after
    // the agent run row has been written inside the same transaction.
    assert!(
        matches!(collision, RepositoryError::Conflict { .. }),
        "expected a uniqueness conflict, got {collision:?}"
    );
    let after = census(&fixture);
    assert_unchanged(&before, &after, "colliding runtime binding");
    assert_eq!(
        after.get("agent_runs"),
        before.get("agent_runs"),
        "the agent run inserted earlier in the same transaction must be rolled back"
    );

    // 3. A cross-project parent.
    assert!(
        fixture
            .store
            .create_agent_run(&NewAgentRun {
                id: AgentRunId::generate(),
                project_id: fixture.other_project,
                team_run_id: existing.team_run_id,
                parent_agent_run_id: Some(run),
                role: role("zz.maker"),
                account_profile_id: None,
                binding: None,
                created_at: now(),
            })
            .is_err()
    );
    assert_unchanged(&before, &census(&fixture), "cross-project parent");

    // 4. A duplicate specification revision.
    assert!(
        fixture
            .store
            .insert_work_profile(fixture.project, &work_profile())
            .is_err(),
        "an immutable revision may not be replaced"
    );
    assert_unchanged(&before, &census(&fixture), "duplicate profile revision");

    // 5. A self-approving persona scenario.
    let mut persona: PersonaScenarioSpec =
        serde_json::from_str(PERSONA_SCENARIO).expect("the persona fixture parses");
    persona.evaluator_roles = vec![persona.actor_role.clone()];
    assert!(
        fixture
            .store
            .insert_persona_scenario(fixture.project, &persona)
            .is_err(),
        "an actor must not evaluate its own scenario"
    );
    assert_unchanged(&before, &census(&fixture), "self-approving persona");

    // 6. An external workflow whose ownership milestone has no rule.
    let mut workflow: ExternalWorkflowSpec =
        serde_json::from_str(WORKFLOW_ASMA).expect("the workflow fixture parses");
    workflow.ownership_milestone =
        SemanticMilestoneKey::parse("milestone.absent").expect("a valid milestone key");
    assert!(
        fixture
            .store
            .insert_external_workflow_spec(fixture.project, &workflow)
            .is_err(),
        "an ownership milestone with no rule must be refused"
    );
    // ... and one whose milestone targets an undeclared status.
    let mut drifted: ExternalWorkflowSpec =
        serde_json::from_str(WORKFLOW_ASMA).expect("the workflow fixture parses");
    drifted.milestones[0].target = StatusSelector {
        status_id: external("never-declared"),
        status_name: name("Never declared"),
    };
    assert!(
        fixture
            .store
            .insert_external_workflow_spec(fixture.project, &drifted)
            .is_err()
    );
    assert_unchanged(&before, &census(&fixture), "invalid workflow mapping");

    // 7. A closure whose evidence does not evidence the claimed outcome.
    // Evidence that points at an event which never reported a terminal state.
    let history = fixture
        .store
        .read_runtime_events(fixture.project, run, None)
        .expect("the read succeeds");
    let unevidenced = TerminalEvidence {
        outcome: TerminalOutcome::Succeeded,
        source: TerminalEvidenceSource::RuntimeObservation {
            cursor: history.first().map_or_else(
                || kontor_core::id::EventCursor::parse(1).unwrap(),
                |e| e.cursor,
            ),
        },
        evidence_hash: ContentHash::of(b"evidence"),
        closed_at: at("2026-08-09T11:00:00Z"),
    };
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: existing.revision,
                evidence: unevidenced,
            })
            .is_err(),
        "closure without matching evidence must be refused"
    );
    assert_unchanged(&before, &census(&fixture), "unevidenced closure");

    // 8. A cross-project ticket link.
    assert!(
        fixture
            .store
            .create_ticket_link(&NewTicketLink {
                id: TicketLinkId::generate(),
                project_id: fixture.project,
                task_id: fixture.other_task,
                connector: ConnectorKey::parse("connector.alpha").expect("a valid connector"),
                external_issue_key: external("ABC-1"),
                created_at: now(),
            })
            .is_err(),
        "a link to another project's task must be refused"
    );
    assert_unchanged(&before, &census(&fixture), "cross-project ticket link");

    // 9. A gate evaluation by a role the profile does not authorize.
    let workflow_id = with_workflow_for(&fixture, fixture.other_task);
    let unauthorized_before = census(&fixture);
    assert!(
        fixture
            .store
            .append_gate_evaluation(&NewGateEvaluation {
                project_id: fixture.other_project,
                workflow_id,
                gate: GateKey::parse("zz.gate").expect("a valid gate key"),
                verdict: GateVerdict::Passed,
                evaluator_role: role("zz.maker"),
                evaluator_account: fixture.account,
                evidence: vec![artifact("zz.output")],
                agent_run_id: None,
                session_evidence: None,
                reviewer_principal: None,
                policy_evaluation_id: None,
                recorded_at: now(),
            })
            .is_err()
    );
    assert_unchanged(
        &unauthorized_before,
        &census(&fixture),
        "unauthorized gate evaluation",
    );

    // Nothing above moved a revision or a lifecycle either.
    assert_eq!(
        run_state(&fixture, run),
        state_before,
        "no refused write may advance a revision or a state"
    );
}

/// Attach a freshly stored profile to `task`, returning the workflow id.
fn with_workflow_for(fixture: &Fixture, task: TaskId) -> TaskWorkflowId {
    let owner = if task == fixture.task {
        fixture.project
    } else {
        fixture.other_project
    };
    let profile = work_profile();
    fixture
        .store
        .insert_work_profile(owner, &profile)
        .expect("the profile is stored");
    let snapshot =
        ResolvedWorkProfileSnapshot::resolve(&profile, now()).expect("the profile resolves");
    let workflow = TaskWorkflowId::generate();
    fixture
        .store
        .create_task_workflow(&NewTaskWorkflow {
            id: workflow,
            project_id: owner,
            task_id: task,
            snapshot,
            current_phase: phase("zz.one"),
            created_at: now(),
        })
        .expect("the workflow is created");
    workflow
}

// ---------------------------------------------------------------------------
// Commands and the outbox
// ---------------------------------------------------------------------------

#[test]
fn intent_desired_state_outbox_and_event_commit_together() {
    let RunFixture { fixture, run } = with_run(true);
    let receipt_id = CommandReceiptId::generate();
    let key = IdempotencyKey::parse("launch-1").expect("a valid key");
    let intent = NewCommandIntent {
        project_id: fixture.project,
        receipt_id,
        idempotency_key: key.clone(),
        kind: CommandKind::LaunchRun,
        target: AggregateRef::AgentRun { agent_run_id: run },
        target_revision: AggregateRevision::INITIAL,
        intent: document("launch"),
        payload: document("launch-payload"),
        desired: Some(DesiredRunState::RunRequested),
        not_before: now(),
        created_at: now(),
    };

    let before = census(&fixture);
    let receipt = fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    assert_eq!(receipt.state, CommandReceiptState::IntentPersisted);

    // All five effects land together: receipt, outbox entry, desired state and
    // — the one the earlier implementation was missing — one intent event.
    let after = census(&fixture);
    assert_eq!(
        after.get("command_receipts").copied().unwrap_or_default(),
        before.get("command_receipts").copied().unwrap_or_default() + 1
    );
    assert_eq!(
        after.get("command_outbox").copied().unwrap_or_default(),
        before.get("command_outbox").copied().unwrap_or_default() + 1
    );
    assert_eq!(
        after.get("runtime_events").copied().unwrap_or_default(),
        before.get("runtime_events").copied().unwrap_or_default() + 1,
        "a successful intent appends exactly one intent event"
    );
    assert_eq!(
        fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .projection
            .desired,
        DesiredRunState::RunRequested,
        "the desired state moved in the same transaction"
    );
    let outbox = fixture
        .store
        .claim_outbox(fixture.project, now(), 10)
        .expect("the outbox is readable");
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].receipt_id, receipt_id);

    // A byte-identical replay returns the original receipt and writes nothing.
    let replay = fixture
        .store
        .record_intent(&intent)
        .expect("a replay returns the original");
    assert_eq!(replay.id, receipt.id);
    assert_unchanged(&after, &census(&fixture), "replayed intent");
    assert_eq!(
        fixture
            .store
            .claim_outbox(fixture.project, now(), 10)
            .expect("the outbox is readable")
            .len(),
        1,
        "a replay must not enqueue a second dispatch"
    );

    // The same key with a different intent fails and changes nothing.
    let after_success = census(&fixture);
    let state_after_success = run_state(&fixture, run);
    let different = NewCommandIntent {
        receipt_id: CommandReceiptId::generate(),
        intent: document("cancel"),
        ..intent
    };
    assert!(fixture.store.record_intent(&different).is_err());
    assert_unchanged(
        &after_success,
        &census(&fixture),
        "idempotency key reused with a different intent",
    );
    assert_eq!(
        fixture
            .store
            .claim_outbox(fixture.project, now(), 10)
            .expect("the outbox is readable")
            .len(),
        1
    );
    assert_eq!(run_state(&fixture, run), state_after_success);
}

#[test]
fn a_refused_intent_rolls_back_receipt_outbox_desired_state_and_event_together() {
    let RunFixture { fixture, run } = with_run(true);

    // Give the run an event history first, so "no event row was added" is a
    // comparison against a non-zero baseline rather than against nothing.
    fixture
        .store
        .append_runtime_event(&event(&fixture, run, Some("n-0"), "history"))
        .expect("the event appends");

    let before = census(&fixture);
    let state_before = run_state(&fixture, run);
    assert_eq!(before.get("runtime_events"), Some(&1));
    assert_eq!(before.get("command_receipts"), Some(&0));
    assert_eq!(before.get("command_outbox"), Some(&0));

    // The run is at revision 1. Computing an intent against revision 2 makes the
    // desired-state compare-and-swap fail — and that CAS runs *after* the
    // receipt and the outbox entry have already been inserted in the same
    // transaction. Everything written so far must therefore disappear.
    let stale = AggregateRevision::parse(2).expect("a positive revision");
    assert_ne!(
        state_before.0, stale,
        "the run must not already be at the stale revision"
    );

    let key = IdempotencyKey::parse("launch-stale").expect("a valid key");
    let error = fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: key.clone(),
            kind: CommandKind::LaunchRun,
            target: AggregateRef::AgentRun { agent_run_id: run },
            target_revision: stale,
            intent: document("launch"),
            payload: document("launch-payload"),
            desired: Some(DesiredRunState::RunRequested),
            not_before: now(),
            created_at: now(),
        })
        .expect_err("a stale target revision must refuse the whole intent");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "expected a revision conflict, got {error:?}"
    );

    // Intent, desired state, outbox entry and event log are one unit of work.
    let after = census(&fixture);
    assert_unchanged(&before, &after, "refused command intent");
    assert_eq!(
        after.get("command_receipts"),
        Some(&0),
        "the receipt inserted earlier in the transaction must be rolled back"
    );
    assert_eq!(
        after.get("command_outbox"),
        Some(&0),
        "the outbox entry must be rolled back with it"
    );
    assert_eq!(
        after.get("runtime_events"),
        Some(&1),
        "no event row may survive a refused intent"
    );
    assert!(
        fixture
            .store
            .get_receipt_by_key(&key)
            .expect("the read succeeds")
            .is_none(),
        "the idempotency key must remain unused after a refused intent"
    );
    assert!(
        fixture
            .store
            .claim_outbox(fixture.project, now(), 10)
            .expect("the outbox is readable")
            .is_empty()
    );
    assert_eq!(
        run_state(&fixture, run),
        state_before,
        "neither the desired state nor the revision may move"
    );

    // The same key is still free, which proves the failed attempt reserved
    // nothing: the intent succeeds now that it cites the true revision.
    fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: key,
            kind: CommandKind::LaunchRun,
            target: AggregateRef::AgentRun { agent_run_id: run },
            target_revision: state_before.0,
            intent: document("launch"),
            payload: document("launch-payload"),
            desired: Some(DesiredRunState::RunRequested),
            not_before: now(),
            created_at: now(),
        })
        .expect("the corrected intent is recorded");
    let final_census = census(&fixture);
    assert_eq!(final_census.get("command_receipts"), Some(&1));
    assert_eq!(final_census.get("command_outbox"), Some(&1));
    assert_eq!(
        final_census.get("runtime_events"),
        Some(&2),
        "the corrected intent adds exactly one intent event to the unified log"
    );

    // The observation event and the intent event share one cursor space, and the
    // intent event points at its receipt.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let (kind, linked): (String, Option<String>) = connection
        .query_row(
            "SELECT event_kind, command_receipt_id FROM runtime_events ORDER BY cursor DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("readable");
    assert_eq!(kind, "command_intent");
    assert!(linked.is_some(), "the intent event names its receipt");

    // Runtime readers still see only runtime observations.
    assert_eq!(
        fixture
            .store
            .read_runtime_events(fixture.project, run, None)
            .expect("the read succeeds")
            .len(),
        1,
        "an intent event is not a runtime observation"
    );
}

#[test]
fn an_unknown_dispatch_result_blocks_a_retry_until_it_is_reconciled() {
    let RunFixture { fixture, run } = with_run(true);
    let receipt_id = CommandReceiptId::generate();
    let key = IdempotencyKey::parse("launch-1").expect("a valid key");
    fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id,
            idempotency_key: key.clone(),
            kind: CommandKind::LaunchRun,
            target: AggregateRef::AgentRun { agent_run_id: run },
            target_revision: AggregateRevision::INITIAL,
            intent: document("launch"),
            payload: document("launch-payload"),
            desired: Some(DesiredRunState::RunRequested),
            not_before: now(),
            created_at: now(),
        })
        .expect("the intent is recorded");

    let advance = |to: CommandReceiptState,
                   correlation: Option<&str>,
                   evidence,
                   result_ref: Option<&str>| ReceiptAdvance {
        project_id: fixture.project,
        receipt_id,
        to,
        correlation: correlation.map(external),
        native_identity: None,
        result_ref: result_ref.map(external),
        no_effect: evidence,
        occurred_at: now(),
    };

    // The dispatch correlation is minted by the claim, not by the caller: it is
    // the outbox entry's durable token, and every later step reuses it.
    let claims = fixture
        .store
        .claim_due(fixture.project, now(), 10)
        .expect("the outbox is claimable");
    let correlation = claims
        .into_iter()
        .find(|claim| claim.receipt_id == receipt_id)
        .expect("the due entry is claimed")
        .correlation;

    // A correlation the outbox never claimed is refused, whichever door it
    // arrives through.
    assert!(
        fixture
            .store
            .advance_receipt(&advance(
                CommandReceiptState::Dispatched,
                Some("corr-invented"),
                None,
                None
            ))
            .is_err(),
        "a caller may not replace the persisted correlation"
    );

    fixture
        .store
        .advance_receipt(&advance(CommandReceiptState::Dispatched, None, None, None))
        .expect("the dispatch happens");
    let unknown = fixture
        .store
        .advance_receipt(&advance(
            CommandReceiptState::ConfirmationUnknown,
            None,
            None,
            None,
        ))
        .expect("the result is unknown");
    assert_eq!(unknown.state, CommandReceiptState::ConfirmationUnknown);
    assert_eq!(unknown.attempts, 1);

    // A blind retry is refused.
    assert!(
        fixture
            .store
            .advance_receipt(&advance(
                CommandReceiptState::DispatchPending,
                None,
                None,
                None
            ))
            .is_err(),
        "a retry without reconciliation must be refused"
    );

    // Evidence for another correlation proves nothing.
    let wrong = NoEffectEvidence {
        correlation: external("corr-other"),
        searched_identity: Some(identity(1)),
        reconciled_at: now(),
        evidence_hash: ContentHash::of(b"lookup"),
    };
    assert!(
        fixture
            .store
            .advance_receipt(&advance(
                CommandReceiptState::DispatchPending,
                None,
                Some(wrong),
                None
            ))
            .is_err()
    );

    let evidence = NoEffectEvidence {
        correlation: correlation.clone(),
        searched_identity: Some(identity(1)),
        reconciled_at: now(),
        evidence_hash: ContentHash::of(b"lookup"),
    };
    let retried = fixture
        .store
        .advance_receipt(&advance(
            CommandReceiptState::DispatchPending,
            None,
            Some(evidence),
            None,
        ))
        .expect("reconciliation authorizes one retry");
    assert_eq!(retried.state, CommandReceiptState::DispatchPending);
    assert_eq!(
        retried.correlation,
        Some(correlation.clone()),
        "the original correlation is retained"
    );

    // Acknowledgement is not completion.
    fixture
        .store
        .advance_receipt(&advance(CommandReceiptState::Dispatched, None, None, None))
        .expect("a second dispatch happens");
    let acknowledged = fixture
        .store
        .advance_receipt(&advance(
            CommandReceiptState::Acknowledged,
            None,
            None,
            None,
        ))
        .expect("the target acknowledges");
    assert!(!acknowledged.state.is_terminal());

    // Confirmation is the one thing the legacy door cannot do on its word alone.
    assert!(
        fixture
            .store
            .advance_receipt(&advance(CommandReceiptState::Confirmed, None, None, None))
            .is_err(),
        "a confirmation must cite the evidence for it"
    );
    let confirmed = fixture
        .store
        .advance_receipt(&advance(
            CommandReceiptState::Confirmed,
            None,
            None,
            Some("native-confirmation-1"),
        ))
        .expect("the effect is confirmed");
    assert!(confirmed.state.is_terminal());
    assert!(
        fixture
            .store
            .advance_receipt(&advance(
                CommandReceiptState::DispatchPending,
                None,
                None,
                None
            ))
            .is_err(),
        "a settled receipt never moves again"
    );

    // The legacy request shape is translated, not exempted: every step it took
    // left a durable history row behind.
    let history = fixture
        .store
        .receipt_history(fixture.project, receipt_id)
        .expect("the history reads");
    assert_eq!(
        history
            .iter()
            .map(|entry| entry.state)
            .collect::<Vec<CommandReceiptState>>(),
        vec![
            CommandReceiptState::IntentPersisted,
            CommandReceiptState::DispatchPending,
            CommandReceiptState::Dispatched,
            CommandReceiptState::ConfirmationUnknown,
            CommandReceiptState::DispatchPending,
            CommandReceiptState::Dispatched,
            CommandReceiptState::Acknowledged,
            CommandReceiptState::Confirmed,
        ],
        "the trait method appends the same history the protocol does"
    );
    assert_eq!(
        history
            .last()
            .expect("a confirmed receipt has history")
            .evidence_ref,
        Some(external("native-confirmation-1")),
        "the confirmation's evidence is durable, not just its state"
    );
    assert_eq!(
        fixture
            .store
            .get_receipt_by_key(&key)
            .expect("the read succeeds")
            .expect("the receipt exists")
            .state,
        CommandReceiptState::Confirmed
    );
}

// ---------------------------------------------------------------------------
// Intake
// ---------------------------------------------------------------------------

fn source_event(id: SourceEventId, external_id: &str, marker: &str) -> CanonicalSourceEvent {
    CanonicalSourceEvent {
        id,
        identity: SourceIdentity {
            source_kind: kontor_core::id::SourceKindKey::parse("webhook").expect("a valid kind"),
            source_connection: kontor_core::id::SourceConnectionKey::parse("conn.alpha")
                .expect("a valid connection"),
            external_event_id: external(external_id),
        },
        envelope: document(marker),
        external_observed_at: now(),
        ingested_at: now(),
        processing_state: SourceProcessingState::Evaluated,
    }
}

fn intake(event: &CanonicalSourceEvent, key: &str) -> IntakeReceipt {
    IntakeReceipt {
        id: IntakeReceiptId::generate(),
        source_event_id: event.id,
        source_event_hash: event.envelope.hash().clone(),
        trigger: TriggerKey::parse("zz.trigger").expect("a valid trigger key"),
        trigger_version: SpecVersion::FIRST,
        result: IntakeResult::Proposed,
        approval: None,
        proposed: None,
        idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
        dedup_key: DedupExpression {
            pointers: vec![JsonPointer::parse("/marker").expect("a valid pointer")],
        }
        .evaluate(&event.envelope)
        .expect("the dedup key evaluates"),
        duplicate_of: None,
        predecessor_receipt_id: None,
        decided_at: now(),
    }
}

/// The team child digest, recomputed without the production helper.
///
/// Mirrors the core suite's independent computation: SHA-256 over the canonical
/// JSON of the children ordered by run id, with object keys sorted. Duplicated
/// on purpose — an expectation borrowed from the code under test asserts only
/// that the code equals itself.
///
/// Note that the *ordering* half of the contract cannot be exercised from here:
/// the store reads a team's children with `ORDER BY id`, so this boundary never
/// sees an unordered input. The core suite owns that case.
fn independent_team_digest(children: &[TeamChildEvidence]) -> ContentHash {
    let mut ordered: Vec<&TeamChildEvidence> = children.iter().collect();
    ordered.sort_by_key(|child| child.agent_run_id.to_string());
    let entries: Vec<String> = ordered
        .iter()
        .map(|child| {
            let hash = child
                .evidence_hash
                .as_ref()
                .map_or_else(|| "null".to_owned(), |h| format!("\"{}\"", h.as_str()));
            format!(
                "{{\"agent_run_id\":\"{}\",\"evidence_hash\":{},\"lifecycle\":\"{}\"}}",
                child.agent_run_id, hash, child.lifecycle
            )
        })
        .collect();
    ContentHash::of(
        format!(
            "{{\"children\":[{}],\"schema_version\":1}}",
            entries.join(",")
        )
        .as_bytes(),
    )
}

/// Mint a real command receipt in `fixture.project` and return its id.
///
/// Capability, approval, revocation and resolution receipt ids are foreign keys
/// *and* authority: a test can neither wave a fresh UUID at them nor reuse one
/// receipt for everything. The kind and the target are therefore parameters —
/// minting an unrelated receipt and calling it a capability is exactly the
/// forbidden case, and the negative tests below use this same helper to build
/// it deliberately.
fn with_receipt(
    fixture: &Fixture,
    key: &str,
    kind: CommandKind,
    target: AggregateRef,
) -> CommandReceiptId {
    let id = CommandReceiptId::generate();
    fixture
        .store
        .record_intent(&NewCommandIntent {
            project_id: fixture.project,
            receipt_id: id,
            idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
            kind,
            target,
            target_revision: AggregateRevision::INITIAL,
            intent: document(key),
            payload: document(key),
            desired: None,
            not_before: now(),
            created_at: now(),
        })
        .expect("the receipt is recorded");
    id
}

/// Store the trigger revision an intake receipt pins to.
///
/// The receipt carries a composite foreign key to `trigger_specs`, so a
/// decision can no longer name a revision that does not exist.
fn with_trigger_in(fixture: &Fixture, project: ProjectId, version: SpecVersion) {
    let mut spec: TriggerSpec =
        serde_json::from_str(TRIGGER_FIXTURE).expect("the trigger fixture parses");
    spec.id = TriggerKey::parse("zz.trigger").expect("a valid trigger key");
    spec.version = version;

    // The trigger's pins are foreign keys now, so the revisions it names have to
    // exist before it does. Seeding them here is what makes the cross-project
    // and dangling-pin cases below fail for the *right* reason.
    let mut profile: WorkProfileSpec =
        serde_json::from_str(ARBITRARY_PROFILE).expect("the profile fixture parses");
    profile.id = spec.work_profile.clone();
    profile.version = spec.work_profile_version;
    if fixture
        .store
        .get_work_profile(project, &profile.id, profile.version)
        .expect("the read succeeds")
        .is_none()
    {
        fixture
            .store
            .insert_work_profile(project, &profile)
            .expect("the pinned profile revision is stored");
    }
    if fixture
        .store
        .get_team_template(
            project,
            spec.team_template.template_id,
            spec.team_template.version,
        )
        .expect("the read succeeds")
        .is_none()
    {
        fixture
            .store
            .insert_team_template(
                project,
                &TeamTemplateRevision {
                    template_id: spec.team_template.template_id,
                    version: spec.team_template.version,
                    name: name("Team"),
                    definition: document("team"),
                    role_authority: Vec::new(),
                },
            )
            .expect("the pinned team revision is stored");
    }

    fixture
        .store
        .insert_trigger_spec(project, &spec)
        .expect("the trigger revision is stored");
}

fn with_trigger(fixture: &Fixture, version: SpecVersion) {
    with_trigger_in(fixture, fixture.project, version);
}

#[test]
fn a_repeated_source_event_returns_the_original_decision() {
    let fixture = fixture();
    with_trigger(&fixture, SpecVersion::FIRST);

    let first = source_event(SourceEventId::generate(), "ext-1", "payload");
    let receipt = intake(&first, "intake-1");
    let outcome = fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: first.clone(),
            receipt: receipt.clone(),
        })
        .expect("the event is recorded");
    assert!(matches!(outcome, IntakeOutcome::Recorded(_)));

    // Everything that follows must add exactly nothing.
    let before = census(&fixture);
    assert_eq!(before.get("source_events"), Some(&1));
    assert_eq!(before.get("intake_receipts"), Some(&1));

    // The same external id again, carrying the same canonical bytes.
    let repeat = source_event(SourceEventId::generate(), "ext-1", "payload");
    match fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: repeat.clone(),
            receipt: intake(&repeat, "intake-2"),
        })
        .expect("a duplicate is not an error")
    {
        IntakeOutcome::Duplicate(original) => assert_eq!(original.id, receipt.id),
        other => panic!("expected a duplicate, got {other:?}"),
    }
    assert_unchanged(&before, &census(&fixture), "repeated source identity");

    // The same source identity carrying DIFFERENT canonical bytes is not a
    // replay: upstream changed what it said under an id it had already used.
    // Returning the old decision would silently discard the new content.
    let contradiction = source_event(SourceEventId::generate(), "ext-1", "a different payload");
    assert!(
        fixture
            .store
            .record_source_event(&NewSourceEvent {
                project_id: fixture.project,
                event: contradiction.clone(),
                receipt: intake(&contradiction, "intake-conflict"),
            })
            .is_err(),
        "the same identity with a different digest is a conflict, not a duplicate"
    );
    assert_unchanged(&before, &census(&fixture), "contradicting source identity");

    // A receipt that decides some *other* event cannot be filed against this
    // one, however consistent it is with itself.
    let unrelated = source_event(SourceEventId::generate(), "ext-8", "unrelated");
    assert!(
        fixture
            .store
            .record_source_event(&NewSourceEvent {
                project_id: fixture.project,
                event: unrelated,
                receipt: intake(&first, "intake-misfiled"),
            })
            .is_err(),
        "a decision must be about the event it is stored with"
    );
    assert_unchanged(&before, &census(&fixture), "misfiled intake decision");

    // A different external id carrying the identical canonical payload.
    let renamed = source_event(SourceEventId::generate(), "ext-2", "payload");
    match fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: renamed.clone(),
            receipt: intake(&renamed, "intake-3"),
        })
        .expect("a duplicate is not an error")
    {
        IntakeOutcome::Duplicate(original) => assert_eq!(original.id, receipt.id),
        other => panic!("expected a duplicate, got {other:?}"),
    }
    assert_unchanged(&before, &census(&fixture), "repeated canonical payload");

    // A decision that contradicts itself is refused outright, and the event it
    // arrived with is not persisted either.
    let inconsistent = source_event(SourceEventId::generate(), "ext-9", "other-payload");
    let mut broken = intake(&inconsistent, "intake-broken");
    broken.result = IntakeResult::Approved; // approved, but with no approval evidence
    assert!(
        fixture
            .store
            .record_source_event(&NewSourceEvent {
                project_id: fixture.project,
                event: inconsistent,
                receipt: broken,
            })
            .is_err(),
        "an inconsistent decision must be refused"
    );
    assert_unchanged(&before, &census(&fixture), "inconsistent intake decision");

    assert_eq!(
        fixture
            .store
            .find_intake_receipt(fixture.project, &first.identity)
            .expect("the read succeeds")
            .expect("the receipt exists")
            .id,
        receipt.id
    );

    // Another project's identical event is its own event, not a duplicate.
    with_trigger_in(&fixture, fixture.other_project, SpecVersion::FIRST);
    let elsewhere = source_event(SourceEventId::generate(), "ext-1", "payload");
    let elsewhere_receipt = intake(&elsewhere, "intake-4");
    assert!(matches!(
        fixture
            .store
            .record_source_event(&NewSourceEvent {
                project_id: fixture.other_project,
                event: elsewhere,
                receipt: elsewhere_receipt,
            })
            .expect("the event is recorded"),
        IntakeOutcome::Recorded(_)
    ));
}

// ---------------------------------------------------------------------------
// External tickets
// ---------------------------------------------------------------------------

#[test]
fn external_comments_deduplicate_replays_and_keep_edits() {
    let fixture = fixture();
    let link = TicketLinkId::generate();
    fixture
        .store
        .create_ticket_link(&NewTicketLink {
            id: link,
            project_id: fixture.project,
            task_id: fixture.task,
            connector: ConnectorKey::parse("connector.alpha").expect("a valid connector"),
            external_issue_key: external("ABC-1"),
            created_at: now(),
        })
        .expect("the link is created");

    let comment = |body: &str, observed: &str| {
        let body = BoundedText::parse(body).expect("a valid body");
        ExternalCommentRevision {
            link_id: link,
            external_comment_id: external("c-1"),
            author_account_id: external("acct-human"),
            author_display: Some(name("A Human")),
            external_created_at: at("2026-08-09T09:00:00Z"),
            external_updated_at: at(observed),
            body_hash: ContentHash::of(body.as_str().as_bytes()),
            body,
            observed_at: at(observed),
            supersedes: None,
        }
    };

    assert!(
        fixture
            .store
            .append_comment(fixture.project, &comment("hello", "2026-08-09T10:00:00Z"))
            .expect("the comment is mirrored")
    );
    assert!(
        !fixture
            .store
            .append_comment(fixture.project, &comment("hello", "2026-08-09T10:30:00Z"))
            .expect("a replay is not an error"),
        "a cursor replay must not mirror the same comment twice"
    );
    assert!(
        fixture
            .store
            .append_comment(
                fixture.project,
                &comment("hello, corrected", "2026-08-09T11:00:00Z")
            )
            .expect("an edit is a new revision"),
        "an edit must be kept with its own provenance"
    );

    // A tampered digest is refused before it reaches SQL.
    let mut tampered = comment("hello", "2026-08-09T12:00:00Z");
    tampered.body_hash = ContentHash::of(b"not the body");
    assert!(
        fixture
            .store
            .append_comment(fixture.project, &tampered)
            .is_err()
    );
}

#[test]
fn a_conflict_keeps_its_inputs_and_resolves_exactly_once() {
    let fixture = fixture();
    let link = TicketLinkId::generate();
    fixture
        .store
        .create_ticket_link(&NewTicketLink {
            id: link,
            project_id: fixture.project,
            task_id: fixture.task,
            connector: ConnectorKey::parse("connector.alpha").expect("a valid connector"),
            external_issue_key: external("ABC-1"),
            created_at: now(),
        })
        .expect("the link is created");

    let observation_id = TicketObservationId::generate();
    fixture
        .store
        .append_observation(
            fixture.project,
            &ExternalTicketObservation {
                id: observation_id,
                link_id: link,
                status: StatusSelector {
                    status_id: external("S-1"),
                    status_name: name("Some status"),
                },
                status_category: name("in progress"),
                issue_type: ExternalIssueTypeKey::parse("task").expect("a valid issue type"),
                assignee_account_id: None,
                assignee_display: None,
                external_version: None,
                observed_at: now(),
                payload_hash: ContentHash::of(b"observation"),
            },
        )
        .expect("the observation is appended");

    let conflict = StatusConflict {
        id: StatusConflictId::generate(),
        link_id: link,
        kind: StatusConflictKind::MultipleLiveTransitions,
        observation_id,
        task_revision: AggregateRevision::INITIAL,
        spec_version: SpecVersion::FIRST,
        milestone: None,
        detected_at: now(),
    };
    fixture
        .store
        .insert_conflict(fixture.project, &conflict)
        .expect("the conflict is recorded");

    let mut replay = conflict.clone();
    replay.id = StatusConflictId::generate();
    replay.detected_at = at("2026-08-09T10:01:00Z");
    fixture
        .store
        .insert_conflict(fixture.project, &replay)
        .expect("the exact open conflict is an idempotent replay");
    let count = Connection::open(&fixture.path)
        .expect("the database opens")
        .query_row(
            "SELECT count(*) FROM status_conflicts
             WHERE project_id = ?1 AND link_id = ?2 AND kind = ?3
               AND resolved_at IS NULL",
            rusqlite::params![
                fixture.project.to_string(),
                link.to_string(),
                conflict.kind.as_str()
            ],
            |row| row.get::<_, i64>(0),
        )
        .expect("open conflicts are countable");
    assert_eq!(count, 1, "an exact replay cannot accumulate another row");

    let mut contradictory = replay;
    contradictory.task_revision = AggregateRevision::parse(2).expect("a valid revision");
    assert!(matches!(
        fixture
            .store
            .insert_conflict(fixture.project, &contradictory),
        Err(RepositoryError::Conflict { .. })
    ));

    let command = NewLocalCommand {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("resolve-conflict").expect("a valid key"),
        kind: CommandKind::ResolveStatusConflict,
        target: AggregateRef::TicketLink { link_id: link },
        target_revision: AggregateRevision::INITIAL,
        intent: document("resolve-conflict"),
        created_at: at("2026-08-09T12:00:00Z"),
    };
    let first = fixture
        .store
        .resolve_task_jira_conflict_atomically(
            fixture.project,
            conflict.id,
            &command,
            at("2026-08-09T12:00:00Z"),
        )
        .expect("the conflict resolves");
    assert!(first.is_fresh());
    let replayed = fixture
        .store
        .resolve_task_jira_conflict_atomically(
            fixture.project,
            conflict.id,
            &command,
            at("2026-08-09T13:00:00Z"),
        )
        .expect("the same key replays its own close");
    assert!(!replayed.is_fresh());

    let losing_key = IdempotencyKey::parse("resolve-conflict-loser").expect("a valid key");
    let competing = NewLocalCommand {
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: losing_key.clone(),
        ..command
    };
    assert!(
        fixture
            .store
            .resolve_task_jira_conflict_atomically(
                fixture.project,
                conflict.id,
                &competing,
                at("2026-08-09T13:00:00Z")
            )
            .is_err(),
        "another key cannot claim the completed resolution"
    );
    assert!(
        fixture
            .store
            .get_receipt_by_key(&losing_key)
            .expect("the receipt ledger reads")
            .is_none(),
        "the losing key leaves no false effect receipt"
    );

    // Another project cannot resolve it at all.
    assert!(
        fixture
            .store
            .resolve_task_jira_conflict_atomically(
                fixture.other_project,
                conflict.id,
                &NewLocalCommand {
                    project_id: fixture.other_project,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: IdempotencyKey::parse("resolve-conflict-other-project")
                        .expect("a valid key"),
                    kind: CommandKind::ResolveStatusConflict,
                    target: AggregateRef::TicketLink { link_id: link },
                    target_revision: AggregateRevision::INITIAL,
                    intent: document("resolve-conflict-other-project"),
                    created_at: at("2026-08-09T13:00:00Z"),
                },
                at("2026-08-09T13:00:00Z")
            )
            .is_err()
    );
}

// ---------------------------------------------------------------------------
// Calendars
// ---------------------------------------------------------------------------

fn calendar_profile() -> CalendarProfileSpec {
    CalendarProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id: CalendarProfileId::generate(),
        version: SpecVersion::FIRST,
        name: name("Office hours"),
        windows: vec![WeeklyWindow {
            weekday: Weekday::Monday,
            start: "08:00:00".parse().expect("a civil time"),
            end: "16:00:00".parse().expect("a civil time"),
        }],
        holiday_merge: HolidayMergePolicy::TreatAsClosed,
        drain_lead_minutes: 30,
    }
}

#[test]
fn a_project_with_no_calendar_is_unrestricted_not_closed() {
    let fixture = fixture();
    assert!(
        fixture
            .store
            .active_assignment(fixture.project)
            .expect("the read succeeds")
            .is_none(),
        "no assignment is not an error"
    );

    let state = resolve_effective_state(&CalendarResolution {
        assignment: None,
        profile: None,
        exceptions: &[],
        schedule_override: None,
        mini_project: None,
        task: None,
        now: now(),
    })
    .expect("resolution succeeds");
    assert_eq!(state, EffectiveCalendarState::Unrestricted);
}

#[test]
fn a_pinned_calendar_revision_is_never_silently_upgraded() {
    let fixture = fixture();
    let profile = calendar_profile();
    fixture
        .store
        .insert_calendar_profile(&profile)
        .expect("the profile revision is stored");

    let assignment = WorkCalendarAssignment {
        id: WorkCalendarId::generate(),
        project_id: fixture.project,
        profile_id: profile.profile_id,
        profile_version: profile.version,
        timezone: IanaTimeZone::parse("Europe/Oslo").expect("a known time zone"),
        window_override: None,
        active: true,
        created_at: now(),
        retired_at: None,
    };
    fixture
        .store
        .assign_calendar(&assignment)
        .expect("the assignment is stored");

    let stored = fixture
        .store
        .active_assignment(fixture.project)
        .expect("the read succeeds")
        .expect("an assignment exists");
    assert_eq!(stored.profile_version, profile.version);

    // A newer revision of the same profile exists, but resolution refuses it
    // until the assignment is deliberately re-pinned.
    let mut newer = profile.clone();
    newer.version = profile.version.next().expect("a next version");
    newer.windows[0].end = "20:00:00".parse().expect("a civil time");
    fixture
        .store
        .insert_calendar_profile(&newer)
        .expect("the newer revision is stored");

    assert!(
        resolve_effective_state(&CalendarResolution {
            assignment: Some(&stored),
            profile: Some(&newer),
            exceptions: &[],
            schedule_override: None,
            mini_project: None,
            task: None,
            now: now(),
        })
        .is_err(),
        "a pinned revision must not be silently upgraded"
    );

    // 2026-08-10 is a Monday; 09:00 Oslo is 07:00 UTC.
    let state = resolve_effective_state(&CalendarResolution {
        assignment: Some(&stored),
        profile: Some(&profile),
        exceptions: &[],
        schedule_override: None,
        mini_project: None,
        task: None,
        now: at("2026-08-10T07:00:00Z"),
    })
    .expect("resolution succeeds");
    assert_eq!(state, EffectiveCalendarState::Open);

    // 15:45 Oslo is inside the 30-minute drain lead.
    let draining = resolve_effective_state(&CalendarResolution {
        assignment: Some(&stored),
        profile: Some(&profile),
        exceptions: &[],
        schedule_override: None,
        mini_project: None,
        task: None,
        now: at("2026-08-10T13:45:00Z"),
    })
    .expect("resolution succeeds");
    assert_eq!(draining, EffectiveCalendarState::Draining);

    // Sunday is outside every window.
    let closed = resolve_effective_state(&CalendarResolution {
        assignment: Some(&stored),
        profile: Some(&profile),
        exceptions: &[],
        schedule_override: None,
        mini_project: None,
        task: None,
        now: at("2026-08-09T10:00:00Z"),
    })
    .expect("resolution succeeds");
    assert_eq!(closed, EffectiveCalendarState::Closed);

    assert!(
        IanaTimeZone::parse("Mars/Olympus").is_err(),
        "an unknown time zone must be refused"
    );
}

#[test]
fn calendar_exceptions_are_append_only_and_scoped_to_their_project() {
    let fixture = fixture();
    let profile = calendar_profile();
    fixture
        .store
        .insert_calendar_profile(&profile)
        .expect("the profile revision is stored");
    let calendar = WorkCalendarId::generate();
    fixture
        .store
        .assign_calendar(&WorkCalendarAssignment {
            id: calendar,
            project_id: fixture.project,
            profile_id: profile.profile_id,
            profile_version: profile.version,
            timezone: IanaTimeZone::parse("Europe/Oslo").expect("a known time zone"),
            window_override: None,
            active: true,
            created_at: now(),
            retired_at: None,
        })
        .expect("the assignment is stored");

    let exception = kontor_core::calendar::CalendarExceptionRevision {
        id: CalendarExceptionId::generate(),
        project_id: fixture.project,
        work_calendar_id: calendar,
        start_date: "2026-08-10".parse().expect("a civil date"),
        end_date: "2026-08-10".parse().expect("a civil date"),
        kind: ExceptionKind::Closed,
        label: name("Public holiday"),
        provenance: ExceptionProvenance::Manual {
            by: fixture.account,
        },
        supersedes: None,
        created_at: now(),
    };
    fixture
        .store
        .append_exception(&exception)
        .expect("the exception is appended");

    let stored = fixture
        .store
        .list_exceptions(fixture.project, calendar)
        .expect("the read succeeds");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0], exception);
    assert!(
        fixture
            .store
            .list_exceptions(fixture.other_project, calendar)
            .expect("the read succeeds")
            .is_empty()
    );

    // The exception closes the Monday the window would otherwise open.
    let state = resolve_effective_state(&CalendarResolution {
        assignment: fixture
            .store
            .active_assignment(fixture.project)
            .expect("the read succeeds")
            .as_ref(),
        profile: Some(&profile),
        exceptions: &stored,
        schedule_override: None,
        mini_project: None,
        task: None,
        now: at("2026-08-10T07:00:00Z"),
    })
    .expect("resolution succeeds");
    assert_eq!(state, EffectiveCalendarState::Closed);

    // An inverted range never reaches SQL.
    let mut inverted = exception;
    inverted.id = CalendarExceptionId::generate();
    inverted.end_date = "2026-08-01".parse().expect("a civil date");
    assert!(fixture.store.append_exception(&inverted).is_err());
}

#[test]
fn an_override_is_bounded_by_its_scope_ceiling_and_revocation() {
    let fixture = fixture();
    let scope = WorkScope::Task {
        task_id: fixture.task,
    };
    let receipt = with_receipt(
        &fixture,
        "approve-override",
        CommandKind::ApproveScheduleOverride,
        scope.aggregate(fixture.project),
    );
    let id = ScheduleOverrideId::generate();
    let over = ScheduleOverride {
        id,
        project_id: fixture.project,
        scope,
        reason: name("Incident"),
        start: at("2026-08-09T10:00:00Z"),
        expiry: OverrideExpiry::FixedAt {
            at: at("2026-08-09T12:00:00Z"),
        },
        hard_ceiling: at("2026-08-09T14:00:00Z"),
        max_concurrency: 1,
        budget: budget(),
        approved_by: fixture.account,
        approval_receipt: receipt,
        revocations: Vec::new(),
    };
    fixture
        .store
        .insert_override(&over)
        .expect("the override is stored");

    let stored = fixture
        .store
        .get_override(fixture.project, id)
        .expect("the read succeeds")
        .expect("the override exists");
    assert_eq!(stored.effective_end(), at("2026-08-09T12:00:00Z"));
    assert!(stored.is_active(at("2026-08-09T11:00:00Z"), None, Some(fixture.task)));
    assert!(
        !stored.is_active(at("2026-08-09T13:00:00Z"), None, Some(fixture.task)),
        "an override must not outlive its expiry"
    );
    assert!(
        !stored.is_active(at("2026-08-09T11:00:00Z"), None, Some(fixture.other_task)),
        "an override must not escape its scope"
    );

    // An expiry beyond the hard ceiling never reaches SQL.
    let mut unbounded = over.clone();
    unbounded.id = ScheduleOverrideId::generate();
    unbounded.expiry = OverrideExpiry::FixedAt {
        at: at("2026-08-09T20:00:00Z"),
    };
    assert!(fixture.store.insert_override(&unbounded).is_err());

    // Revocation happens once.
    let revocation = OverrideRevocation {
        revoked_at: at("2026-08-09T11:00:00Z"),
        revoked_by: fixture.account,
        receipt: with_receipt(
            &fixture,
            "revoke-override",
            CommandKind::RevokeScheduleOverride,
            scope.aggregate(fixture.project),
        ),
    };
    fixture
        .store
        .revoke_override(fixture.project, id, &revocation)
        .expect("the override is revoked");
    assert!(
        fixture
            .store
            .revoke_override(fixture.project, id, &revocation)
            .is_err()
    );
    let revoked = fixture
        .store
        .get_override(fixture.project, id)
        .expect("the read succeeds")
        .expect("the override exists");
    assert_eq!(revoked.revocations.len(), 1);
    assert!(!revoked.is_active(at("2026-08-09T11:30:00Z"), None, Some(fixture.task)));
}

#[test]
fn an_execution_authorization_is_bounded_and_arms_only_its_scope() {
    let fixture = fixture();
    let authorization = ExecutionAuthorization {
        id: kontor_core::id::ExecutionAuthorizationId::generate(),
        project_id: fixture.project,
        scope: WorkScope::Task {
            task_id: fixture.task,
        },
        selected_tasks: vec![fixture.task],
        allowed_start: TimeRange {
            start: at("2026-08-09T10:00:00Z"),
            end: at("2026-08-09T18:00:00Z"),
        },
        max_concurrency: 2,
        budget: budget(),
        created_by: fixture.account,
        capability_receipt: with_receipt(
            &fixture,
            "capability",
            CommandKind::AuthorizeExecution,
            AggregateRef::Task {
                task_id: fixture.task,
            },
        ),
        created_at: now(),
    };
    fixture
        .store
        .insert_authorization(&authorization)
        .expect("the authorization is stored");

    assert!(authorization.arms(at("2026-08-09T11:00:00Z"), None, Some(fixture.task)));
    assert!(
        !authorization.arms(at("2026-08-09T19:00:00Z"), None, Some(fixture.task)),
        "an authorization must not arm outside its window"
    );
    assert!(
        !authorization.arms(at("2026-08-09T11:00:00Z"), None, Some(fixture.other_task)),
        "an authorization must not arm outside its scope"
    );

    let mut unbounded = authorization;
    unbounded.id = kontor_core::id::ExecutionAuthorizationId::generate();
    unbounded.max_concurrency = 0;
    assert!(fixture.store.insert_authorization(&unbounded).is_err());
}

/// Two sibling goals in `fixture.project`, each owning one task.
///
/// Sibling goals are the interesting shape: both tasks are in the right
/// project, both goals are in the right project, and every foreign key is
/// satisfiable — so nothing but an explicit membership check separates them.
struct Goals {
    goal: MiniProjectId,
    sibling_goal: MiniProjectId,
    task: TaskId,
    sibling_task: TaskId,
}

fn with_goals(fixture: &Fixture) -> Goals {
    let mut made = Vec::new();
    for label in ["Goal", "Sibling goal"] {
        let goal = MiniProjectId::generate();
        fixture
            .store
            .create_mini_project(&NewMiniProject {
                id: goal,
                project_id: fixture.project,
                name: name(label),
                created_at: now(),
            })
            .expect("a goal is created");
        let task = TaskId::generate();
        fixture
            .store
            .create_task(&NewTask {
                id: task,
                project_id: fixture.project,
                mini_project_id: Some(goal),
                title: name("A task in a goal"),
                module: None,
                state: TaskState::Ready,
                created_at: now(),
            })
            .expect("a task is created");
        made.push((goal, task));
    }
    Goals {
        goal: made[0].0,
        sibling_goal: made[1].0,
        task: made[0].1,
        sibling_task: made[1].1,
    }
}

/// An authorization over `scope` arming `selected`, backed by `receipt`.
fn authorization_over(
    fixture: &Fixture,
    scope: WorkScope,
    selected: Vec<TaskId>,
    receipt: CommandReceiptId,
) -> ExecutionAuthorization {
    ExecutionAuthorization {
        id: kontor_core::id::ExecutionAuthorizationId::generate(),
        project_id: fixture.project,
        scope,
        selected_tasks: selected,
        allowed_start: TimeRange {
            start: at("2026-08-09T10:00:00Z"),
            end: at("2026-08-09T18:00:00Z"),
        },
        max_concurrency: 2,
        budget: budget(),
        created_by: fixture.account,
        capability_receipt: receipt,
        created_at: now(),
    }
}

/// An override over `scope`, backed by `receipt`.
fn override_over(
    fixture: &Fixture,
    scope: WorkScope,
    receipt: CommandReceiptId,
) -> ScheduleOverride {
    ScheduleOverride {
        id: ScheduleOverrideId::generate(),
        project_id: fixture.project,
        scope,
        reason: name("Incident"),
        start: at("2026-08-09T10:00:00Z"),
        expiry: OverrideExpiry::FixedAt {
            at: at("2026-08-09T12:00:00Z"),
        },
        hard_ceiling: at("2026-08-09T14:00:00Z"),
        max_concurrency: 1,
        budget: budget(),
        approved_by: fixture.account,
        approval_receipt: receipt,
        revocations: Vec::new(),
    }
}

#[test]
fn a_capability_receipt_must_record_this_command_over_this_exact_scope() {
    let fixture = fixture();
    let goals = with_goals(&fixture);
    let scope = WorkScope::MiniProject {
        mini_project_id: goals.goal,
    };
    // Every receipt is minted first: recording an intent is itself a write, and
    // the census below must only ever move because of the call under test.
    let wrong_kind_receipt = with_receipt(
        &fixture,
        "capability-wrong-kind",
        CommandKind::ApproveScheduleOverride,
        scope.aggregate(fixture.project),
    );
    let wrong_target_receipt = with_receipt(
        &fixture,
        "capability-wrong-target",
        CommandKind::AuthorizeExecution,
        AggregateRef::MiniProject {
            mini_project_id: goals.sibling_goal,
        },
    );
    let right_receipt = with_receipt(
        &fixture,
        "capability-right",
        CommandKind::AuthorizeExecution,
        scope.aggregate(fixture.project),
    );
    let before = census(&fixture);

    // Right scope, wrong command: an override approval over the same goal is
    // not a capability to run work in it.
    let wrong_kind = authorization_over(&fixture, scope, vec![goals.task], wrong_kind_receipt);
    assert!(
        fixture.store.insert_authorization(&wrong_kind).is_err(),
        "a receipt for a different command must not arm work"
    );

    // Right command, wrong scope: a capability over the sibling goal.
    let wrong_target = authorization_over(&fixture, scope, vec![goals.task], wrong_target_receipt);
    assert!(
        fixture.store.insert_authorization(&wrong_target).is_err(),
        "a capability over another goal must not arm this one"
    );

    let after = census(&fixture);
    for table in ["execution_authorizations", "execution_authorization_tasks"] {
        assert_eq!(
            after.get(table),
            before.get(table),
            "a refused authorization must leave `{table}` untouched"
        );
    }

    // The same shape with the right receipt is accepted, so the refusals above
    // are about authority and nothing else.
    let allowed = authorization_over(&fixture, scope, vec![goals.task], right_receipt);
    fixture
        .store
        .insert_authorization(&allowed)
        .expect("a correctly authorized capability arms work");
}

#[test]
fn a_task_from_a_sibling_goal_is_not_armed_by_a_goal_authorization() {
    let fixture = fixture();
    let goals = with_goals(&fixture);
    let scope = WorkScope::MiniProject {
        mini_project_id: goals.goal,
    };
    let mut receipts = ["sibling", "goalless", "member"].into_iter().map(|label| {
        with_receipt(
            &fixture,
            &format!("{label}-capability"),
            CommandKind::AuthorizeExecution,
            scope.aggregate(fixture.project),
        )
    });
    let sibling_receipt = receipts.next().expect("three receipts");
    let goalless_receipt = receipts.next().expect("three receipts");
    let member_receipt = receipts.next().expect("three receipts");
    let before = census(&fixture);

    // Every foreign key here is satisfiable: the sibling task is a real task in
    // the right project. Only its goal membership is wrong.
    let across_goals =
        authorization_over(&fixture, scope, vec![goals.sibling_task], sibling_receipt);
    assert!(
        fixture.store.insert_authorization(&across_goals).is_err(),
        "a goal authorization must not arm a task from a sibling goal"
    );

    // A task in no goal at all is equally outside a goal scope.
    let goalless = authorization_over(&fixture, scope, vec![fixture.task], goalless_receipt);
    assert!(
        fixture.store.insert_authorization(&goalless).is_err(),
        "a goal authorization must not arm a task that belongs to no goal"
    );

    assert_unchanged(
        &before,
        &census(&fixture),
        "an authorization refused for goal membership",
    );

    // The task that really is in this goal is armed.
    let inside = authorization_over(&fixture, scope, vec![goals.task], member_receipt);
    fixture
        .store
        .insert_authorization(&inside)
        .expect("a task inside the goal is armed");
    assert_eq!(
        census(&fixture).get("execution_authorization_tasks"),
        Some(&1),
        "exactly the one member task is armed"
    );
}

#[test]
fn an_override_approval_and_its_revocation_each_need_their_own_receipt() {
    let fixture = fixture();
    let goals = with_goals(&fixture);
    let scope = WorkScope::MiniProject {
        mini_project_id: goals.goal,
    };
    // Every receipt is minted first: recording an intent is itself a write, and
    // the census below must only ever move because of the call under test.
    let mint = |label: &str, kind, target| with_receipt(&fixture, label, kind, target);
    let wrong_kind_receipt = mint(
        "override-wrong-kind",
        CommandKind::AuthorizeExecution,
        scope.aggregate(fixture.project),
    );
    let wrong_target_receipt = mint(
        "override-wrong-target",
        CommandKind::ApproveScheduleOverride,
        AggregateRef::MiniProject {
            mini_project_id: goals.sibling_goal,
        },
    );
    let approve_receipt = mint(
        "override-approve",
        CommandKind::ApproveScheduleOverride,
        scope.aggregate(fixture.project),
    );
    let revoke_wrong_target = mint(
        "revoke-wrong-target",
        CommandKind::RevokeScheduleOverride,
        AggregateRef::MiniProject {
            mini_project_id: goals.sibling_goal,
        },
    );
    let revoke_receipt = mint(
        "revoke-right",
        CommandKind::RevokeScheduleOverride,
        scope.aggregate(fixture.project),
    );
    let before = census(&fixture);

    // Approval: right scope, wrong command.
    let wrong_kind = override_over(&fixture, scope, wrong_kind_receipt);
    assert!(
        fixture.store.insert_override(&wrong_kind).is_err(),
        "a capability receipt must not approve an override"
    );

    // Approval: right command, wrong scope.
    let wrong_target = override_over(&fixture, scope, wrong_target_receipt);
    assert!(
        fixture.store.insert_override(&wrong_target).is_err(),
        "an approval over another goal must not open this one"
    );
    assert_unchanged(&before, &census(&fixture), "a refused override approval");

    let approved = override_over(&fixture, scope, approve_receipt);
    fixture
        .store
        .insert_override(&approved)
        .expect("a correctly approved override is stored");
    let after_approval = census(&fixture);

    // Revocation: the approval receipt is not permission to undo itself.
    assert!(
        fixture
            .store
            .revoke_override(
                fixture.project,
                approved.id,
                &OverrideRevocation {
                    revoked_at: at("2026-08-09T11:00:00Z"),
                    revoked_by: fixture.account,
                    receipt: approved.approval_receipt,
                },
            )
            .is_err(),
        "an approval receipt must not revoke the override it approved"
    );

    // Revocation: right command, wrong scope.
    assert!(
        fixture
            .store
            .revoke_override(
                fixture.project,
                approved.id,
                &OverrideRevocation {
                    revoked_at: at("2026-08-09T11:00:00Z"),
                    revoked_by: fixture.account,
                    receipt: revoke_wrong_target,
                },
            )
            .is_err(),
        "a revocation aimed at another goal must not revoke this override"
    );
    assert!(
        fixture
            .store
            .get_override(fixture.project, approved.id)
            .expect("the read succeeds")
            .expect("the override exists")
            .revocations
            .is_empty(),
        "a refused revocation must leave the override live"
    );
    assert_unchanged(
        &after_approval,
        &census(&fixture),
        "a refused override revocation",
    );

    fixture
        .store
        .revoke_override(
            fixture.project,
            approved.id,
            &OverrideRevocation {
                revoked_at: at("2026-08-09T11:00:00Z"),
                revoked_by: fixture.account,
                receipt: revoke_receipt,
            },
        )
        .expect("a correctly authorized revocation succeeds");
}

#[test]
fn a_conflict_resolution_receipt_must_resolve_this_conflicted_link() {
    let fixture = fixture();
    let mut links = Vec::new();
    for (index, task) in [fixture.task, fixture.task].into_iter().enumerate() {
        let link = TicketLinkId::generate();
        fixture
            .store
            .create_ticket_link(&NewTicketLink {
                id: link,
                project_id: fixture.project,
                task_id: task,
                connector: ConnectorKey::parse("connector.alpha").expect("a valid connector"),
                external_issue_key: external(&format!("ABC-{}", index + 10)),
                created_at: now(),
            })
            .expect("the link is created");
        links.push(link);
    }
    let (link, other_link) = (links[0], links[1]);

    let observation_id = TicketObservationId::generate();
    fixture
        .store
        .append_observation(
            fixture.project,
            &ExternalTicketObservation {
                id: observation_id,
                link_id: link,
                status: StatusSelector {
                    status_id: external("S-1"),
                    status_name: name("Some status"),
                },
                status_category: name("in progress"),
                issue_type: ExternalIssueTypeKey::parse("task").expect("a valid issue type"),
                assignee_account_id: None,
                assignee_display: None,
                external_version: None,
                observed_at: now(),
                payload_hash: ContentHash::of(b"observation"),
            },
        )
        .expect("the observation is appended");
    let record = StatusConflict {
        id: StatusConflictId::generate(),
        link_id: link,
        kind: StatusConflictKind::MultipleLiveTransitions,
        observation_id,
        task_revision: AggregateRevision::INITIAL,
        spec_version: SpecVersion::FIRST,
        milestone: None,
        detected_at: now(),
    };
    fixture
        .store
        .insert_conflict(fixture.project, &record)
        .expect("the conflict is recorded");
    let wrong_kind_receipt = with_receipt(
        &fixture,
        "conflict-wrong-kind",
        CommandKind::TransitionTicket,
        AggregateRef::TicketLink { link_id: link },
    );
    let wrong_target_receipt = with_receipt(
        &fixture,
        "conflict-wrong-target",
        CommandKind::ResolveStatusConflict,
        AggregateRef::TicketLink {
            link_id: other_link,
        },
    );
    let right_receipt = with_receipt(
        &fixture,
        "conflict-right",
        CommandKind::ResolveStatusConflict,
        AggregateRef::TicketLink { link_id: link },
    );
    let before = census(&fixture);

    // Right link, wrong command.
    assert!(
        fixture
            .store
            .resolve_conflict(
                fixture.project,
                record.id,
                wrong_kind_receipt,
                at("2026-08-09T12:00:00Z"),
            )
            .is_err(),
        "converging a ticket is not resolving a conflict about it"
    );

    // Right command, wrong link.
    assert!(
        fixture
            .store
            .resolve_conflict(
                fixture.project,
                record.id,
                wrong_target_receipt,
                at("2026-08-09T12:00:00Z"),
            )
            .is_err(),
        "a resolution about another link must not close this conflict"
    );

    assert_unchanged(&before, &census(&fixture), "a refused conflict resolution");

    fixture
        .store
        .resolve_conflict(
            fixture.project,
            record.id,
            right_receipt,
            at("2026-08-09T12:00:00Z"),
        )
        .expect("a correctly authorized resolution closes the conflict");
}

#[test]
fn the_stored_command_vocabulary_is_exactly_the_domain_vocabulary() {
    let fixture = fixture();
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let insert = |kind: &str| {
        connection.execute(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, '{}', 1, '{}', ?5, 'intent_persisted', 0, ?6, ?6)",
            rusqlite::params![
                CommandReceiptId::generate().to_string(),
                fixture.project.to_string(),
                format!("vocabulary-{kind}"),
                kind,
                ContentHash::of(b"intent").as_str(),
                now().to_string(),
            ],
        )
    };

    // Every command the domain can produce must be storable. A kind the enum
    // knows and the check constraint does not would fail only at runtime, on
    // the authority paths that consume these receipts.
    for kind in CommandKind::ALL {
        insert(kind.as_str())
            .unwrap_or_else(|error| panic!("`{kind}` must satisfy the check constraint: {error}"));
    }
    // And nothing else: a kind SQL accepts but the domain cannot parse would be
    // an unreadable row.
    assert!(
        insert("not_a_command").is_err(),
        "the check constraint must not admit a command the domain cannot parse"
    );
    assert_eq!(
        census(&fixture).get("command_receipts"),
        Some(&i64::try_from(CommandKind::ALL.len()).expect("a small count")),
        "exactly one row per command kind was stored"
    );
}

#[test]
fn an_incompatible_command_kind_and_target_never_reaches_sql() {
    let fixture = fixture();
    let before = census(&fixture);

    let intent = |kind, target, desired| NewCommandIntent {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("incompatible").expect("a valid key"),
        kind,
        target,
        target_revision: AggregateRevision::INITIAL,
        intent: document("incompatible"),
        payload: document("incompatible"),
        desired,
        not_before: now(),
        created_at: now(),
    };
    let task = AggregateRef::Task {
        task_id: fixture.task,
    };

    // A ticket command against a task, carrying nothing at all. Every other
    // rule is satisfied — no desired state to object to, a real task in the
    // right project — so only the target legality itself can refuse it.
    assert!(
        fixture
            .store
            .record_intent(&intent(CommandKind::SyncTicket, task, None))
            .is_err(),
        "a ticket command must not target a task"
    );

    // A launch is not something a task can be asked for.
    assert!(
        fixture
            .store
            .record_intent(&intent(
                CommandKind::LaunchRun,
                task,
                Some(DesiredRunState::RunRequested)
            ))
            .is_err(),
        "a run command must not target a task"
    );
    // A task command carries no desired run state.
    assert!(
        fixture
            .store
            .record_intent(&intent(
                CommandKind::ResumeTask,
                task,
                Some(DesiredRunState::RunRequested)
            ))
            .is_err(),
        "a task command must not carry a desired run state"
    );
    // A run command without its own desired state is incomplete.
    assert!(
        fixture
            .store
            .record_intent(&intent(
                CommandKind::LaunchRun,
                AggregateRef::AgentRun {
                    agent_run_id: kontor_core::id::AgentRunId::generate()
                },
                None
            ))
            .is_err(),
        "a launch must carry the desired state it asks for"
    );
    // The wrong desired state for the command is not merely unusual.
    assert!(
        fixture
            .store
            .record_intent(&intent(
                CommandKind::LaunchRun,
                AggregateRef::AgentRun {
                    agent_run_id: kontor_core::id::AgentRunId::generate()
                },
                Some(DesiredRunState::CancelRequested)
            ))
            .is_err(),
        "a launch must not ask for a cancellation"
    );

    assert_unchanged(&before, &census(&fixture), "an incompatible command intent");
}

#[test]
fn the_gate_state_map_reduces_the_whole_append_only_history() {
    let fixture = fixture();
    let workflow = with_workflow(&fixture);
    let gate = GateKey::parse("zz.gate").expect("a valid gate key");
    let record = |verdict: GateVerdict, role_key: &str, evidence: Vec<ArtifactKey>| {
        fixture
            .store
            .append_gate_evaluation(&NewGateEvaluation {
                project_id: fixture.project,
                workflow_id: workflow,
                gate: gate.clone(),
                verdict,
                evaluator_role: role(role_key),
                evaluator_account: fixture.account,
                evidence,
                agent_run_id: None,
                session_evidence: None,
                reviewer_principal: None,
                policy_evaluation_id: None,
                recorded_at: now(),
            })
            .expect("the evaluation is appended")
    };

    record(GateVerdict::Started, "zz.reviewer", Vec::new());
    record(GateVerdict::Rejected, "zz.reviewer", Vec::new());
    record(GateVerdict::Started, "zz.reviewer", Vec::new());
    record(
        GateVerdict::Waived,
        "zz.waiver",
        vec![artifact("zz.output")],
    );

    let states: BTreeMap<_, _> = fixture
        .store
        .gate_states(fixture.project, workflow)
        .expect("the read succeeds");
    assert_eq!(
        states.get(&gate).copied(),
        Some(kontor_core::state::GateState::Waived),
        "the newest verdict wins, and every earlier one is retained"
    );
    assert_eq!(
        fixture
            .store
            .list_gate_evaluations(fixture.project, workflow)
            .expect("the read succeeds")
            .len(),
        4
    );
}

// ---------------------------------------------------------------------------
// Realm ingress
// ---------------------------------------------------------------------------

#[test]
fn a_mismatched_realm_envelope_or_foreign_id_fails_atomically() {
    let RunFixture { fixture, run } = with_run(true);
    let local = fixture.store.realm();

    // A genuinely different Realm: its own database, its own identity.
    let elsewhere = TempDir::new().expect("a temporary directory");
    let foreign_store =
        SqliteStore::open(&elsewhere.path().join("kontor.db")).expect("the other store opens");
    let foreign = foreign_store.realm();
    assert_ne!(local, foreign, "two databases are two realms");

    let before = census(&fixture);
    let state_before = run_state(&fixture, run);

    let intent = NewCommandIntent {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("launch-realm").expect("a valid key"),
        kind: CommandKind::LaunchRun,
        target: AggregateRef::AgentRun { agent_run_id: run },
        target_revision: state_before.0,
        intent: document("launch"),
        payload: document("launch-payload"),
        desired: Some(DesiredRunState::RunRequested),
        not_before: now(),
        created_at: now(),
    };

    // 1. An intent envelope stamped with the other Realm is refused before a
    //    transaction opens.
    let error = fixture
        .store
        .record_intent_in_realm(&ReceiptEnvelope::new(foreign, intent.clone()))
        .expect_err("a foreign realm envelope must be refused");
    assert!(
        matches!(
            error,
            RepositoryError::Domain(kontor_core::DomainError::RealmMismatch { .. })
        ),
        "expected a realm mismatch, got {error:?}"
    );
    assert_unchanged(&before, &census(&fixture), "foreign realm intent");

    // 2. An observation envelope from the other Realm, likewise.
    assert!(
        fixture
            .store
            .record_observation_in_realm(&EventEnvelope::new(
                foreign,
                EventCursor::parse(1).expect("a positive cursor"),
                NewObservation {
                    event: event(&fixture, run, Some("n-realm"), "foreign"),
                    observed: ObservedRunState::Running,
                    contact: RuntimeContact::Reachable,
                    freshness: Freshness::Fresh,
                    expected_revision: state_before.0,
                    quota_state: None,
                },
            ))
            .is_err()
    );
    assert_unchanged(&before, &census(&fixture), "foreign realm observation");

    // 3. A source event envelope from the other Realm.
    let source = source_event(SourceEventId::generate(), "ext-realm", "payload");
    let receipt = intake(&source, "intake-realm");
    assert!(
        fixture
            .store
            .record_source_event_in_realm(&ReceiptEnvelope::new(
                foreign,
                NewSourceEvent {
                    project_id: fixture.project,
                    event: source,
                    receipt,
                },
            ))
            .is_err()
    );
    assert_unchanged(&before, &census(&fixture), "foreign realm source event");

    // 4. A re-evaluation envelope from the other Realm. Re-evaluation accepts a
    //    source event id and a digest from outside, so it is an ingress path in
    //    its own right and needs the same proof as the initial intake.
    with_trigger(&fixture, SpecVersion::FIRST);
    let stored_source = source_event(SourceEventId::generate(), "ext-realm-2", "payload");
    fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: stored_source.clone(),
            receipt: intake(&stored_source, "intake-realm-2"),
        })
        .expect("the event exists locally");
    let after_source = census(&fixture);
    let reevaluation = NewIntakeReevaluation {
        project_id: fixture.project,
        source_event_id: stored_source.id,
        source_event_hash: stored_source.envelope.hash().clone(),
        receipt: {
            let mut receipt = intake(&stored_source, "intake-realm-3");
            receipt.trigger_version = SpecVersion::FIRST.next().expect("a next version");
            receipt
        },
    };
    let error = fixture
        .store
        .reevaluate_source_event_in_realm(&ReceiptEnvelope::new(foreign, reevaluation))
        .expect_err("a foreign realm re-evaluation must be refused");
    assert!(
        matches!(
            error,
            RepositoryError::Domain(kontor_core::DomainError::RealmMismatch { .. })
        ),
        "expected a realm mismatch, got {error:?}"
    );
    assert_unchanged(
        &after_source,
        &census(&fixture),
        "foreign realm re-evaluation",
    );

    // 5. A receipt minted elsewhere cannot be replayed in here. A receipt is the
    //    value that travels furthest from the store that minted it, so this is
    //    exactly where a cross-Realm mix-up would surface.
    let foreign_receipt = CommandReceipt {
        id: CommandReceiptId::generate(),
        project_id: fixture.project,
        idempotency_key: IdempotencyKey::parse("launch-imported").expect("a valid key"),
        kind: CommandKind::LaunchRun,
        target: AggregateRef::AgentRun { agent_run_id: run },
        target_revision: state_before.0,
        intent: document("launch"),
        state: CommandReceiptState::IntentPersisted,
        correlation: None,
        native_identity: None,
        result_ref: None,
        attempts: 0,
        created_at: now(),
        updated_at: now(),
    };
    assert!(
        fixture
            .store
            .import_receipt_in_realm(&ReceiptEnvelope::new(foreign, foreign_receipt.clone()))
            .is_err(),
        "a receipt stamped with another realm must be refused"
    );
    // Under the local Realm it is refused for a different reason: this Realm
    // never minted it, so there is no such row. Absence *is* the isolation.
    assert!(
        matches!(
            fixture
                .store
                .import_receipt_in_realm(&ReceiptEnvelope::new(local, foreign_receipt)),
            Err(RepositoryError::NotFound { .. })
        ),
        "an unknown receipt is not silently created on import"
    );
    assert_unchanged(&after_source, &census(&fixture), "foreign realm receipt");

    // 6. A cursor counted in the other Realm cannot address this event stream.
    let foreign_cursor = RealmCursor::new(foreign, EventCursor::parse(1).expect("a cursor"));
    assert!(
        fixture
            .store
            .read_events_after(fixture.project, run, Some(foreign_cursor))
            .is_err(),
        "a cursor from another realm counts in a different space"
    );

    // Nothing above moved a revision or a desired state.
    assert_eq!(run_state(&fixture, run), state_before);

    // The same envelope under the *local* realm is accepted.
    let accepted = fixture
        .store
        .record_intent_in_realm(&ReceiptEnvelope::new(local, intent))
        .expect("a local realm envelope is accepted");
    assert_eq!(accepted.state, CommandReceiptState::IntentPersisted);

    // 7. A Realm-local envelope carrying an id that exists only in the *other*
    //    Realm still fails: the row is simply absent here. There is no fallback
    //    lookup and no cross-database attach.
    let foreign_project = ProjectId::generate();
    foreign_store
        .create_project(&NewProject {
            id: foreign_project,
            name: name("Elsewhere"),
            root_path: name("/tmp/elsewhere"),
            created_at: now(),
        })
        .expect("the other realm has its own project");
    let after_accept = census(&fixture);
    let smuggled = NewCommandIntent {
        project_id: foreign_project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("launch-smuggled").expect("a valid key"),
        kind: CommandKind::LaunchRun,
        target: AggregateRef::AgentRun { agent_run_id: run },
        target_revision: AggregateRevision::INITIAL,
        intent: document("launch"),
        payload: document("launch-payload"),
        desired: Some(DesiredRunState::RunRequested),
        not_before: now(),
        created_at: now(),
    };
    assert!(
        fixture
            .store
            .record_intent_in_realm(&ReceiptEnvelope::new(local, smuggled))
            .is_err(),
        "a foreign id under the local realm id must not resolve"
    );
    assert_unchanged(&after_accept, &census(&fixture), "smuggled foreign id");

    // Reads are Realm-qualified on the way out too.
    let snapshot = fixture
        .store
        .snapshot_agent_run(fixture.project, run)
        .expect("the snapshot succeeds");
    assert_eq!(snapshot.realm_id, local);
    assert!(snapshot.peek(foreign).is_err());
    let events = fixture
        .store
        .read_events_after(fixture.project, run, None)
        .expect("the read succeeds");
    assert!(events.iter().all(|envelope| envelope.realm_id == local));
}

#[test]
fn replayed_or_out_of_order_native_events_never_regress_projection_or_revision() {
    let RunFixture { fixture, run } = with_run(true);

    let advance = |sequence: u64, marker: &str, observed: ObservedRunState| {
        let current = fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists");
        fixture.store.record_observation(&NewObservation {
            event: sequenced_event(&fixture, run, Some(marker), marker, sequence),
            observed,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: current.revision,
            quota_state: None,
        })
    };

    advance(5, "n-5", ObservedRunState::Running).expect("the first observation reduces");
    let settled = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert_eq!(settled.projection.observed, ObservedRunState::Running);
    let baseline_revision = settled.revision;
    let baseline_cursor = settled.projection.last_cursor;
    let events_after_first = census(&fixture)
        .get("runtime_events")
        .copied()
        .unwrap_or_default();

    // An exact replay of the same native event: no state change, no revision.
    let replayed =
        advance(5, "n-5", ObservedRunState::Succeeded).expect("a replay is not an error");
    assert_eq!(replayed.observed, ObservedRunState::Running);
    // An older sequence arriving late: appended as evidence, but not applied.
    let stale =
        advance(3, "n-3", ObservedRunState::Failed).expect("an older event is not an error");
    assert_eq!(stale.observed, ObservedRunState::Running);
    // The same sequence under a different native id: still not strictly newer.
    let equal = advance(5, "n-5b", ObservedRunState::Cancelled).expect("an equal sequence is fine");
    assert_eq!(equal.observed, ObservedRunState::Running);

    let after = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert_eq!(
        after.projection.observed,
        ObservedRunState::Running,
        "a duplicate or older observation must not overwrite observed state"
    );
    assert_eq!(after.projection.derived, settled.projection.derived);
    assert_eq!(
        after.projection.last_cursor, baseline_cursor,
        "the reduced cursor must not move"
    );
    assert_eq!(
        after.revision, baseline_revision,
        "a duplicate or older observation must not increment the revision"
    );
    assert!(
        census(&fixture)
            .get("runtime_events")
            .copied()
            .unwrap_or_default()
            > events_after_first,
        "genuinely new-but-older events are still appended as evidence"
    );

    // Only a strictly newer sequence in the bound generation reduces state.
    let progressed = advance(6, "n-6", ObservedRunState::WaitingInput).expect("newer reduces");
    assert_eq!(progressed.observed, ObservedRunState::WaitingInput);
    let moved = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert_eq!(moved.revision.get(), baseline_revision.get() + 1);

    // An event from a different runtime generation is reconciliation input, not
    // an overwrite: it does not belong to this run's binding.
    let current = moved.revision;
    let foreign = NewRuntimeEvent {
        project_id: fixture.project,
        agent_run_id: run,
        identity: identity(9),
        native_event_id: Some(external("n-9")),
        native_sequence: 99,
        payload: document("other-generation"),
        observed_at: now(),
    };
    assert!(
        fixture
            .store
            .record_observation(&NewObservation {
                event: foreign,
                observed: ObservedRunState::Succeeded,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: current,
                quota_state: None,
            })
            .is_err(),
        "an event from another generation must not reduce this run"
    );
    assert_eq!(
        fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .projection
            .observed,
        ObservedRunState::WaitingInput
    );
}

#[test]
fn terminal_evidence_must_belong_to_the_closed_run_and_match_its_hash() {
    let RunFixture { fixture, run } = with_run(true);

    // A sibling run in the same team, with its own terminal event.
    let existing = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    let sibling = AgentRunId::generate();
    fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: sibling,
            project_id: fixture.project,
            team_run_id: existing.team_run_id,
            parent_agent_run_id: None,
            role: role("zz.maker"),
            account_profile_id: None,
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: sibling,
                identity: NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse("zz.runtime").expect("a valid key"),
                    host: name("host-2"),
                    generation: 1,
                    native_id: external("session-2"),
                },
                bound_at: now(),
            }),
            created_at: now(),
        })
        .expect("the sibling run is created");

    // Real evidence for *this* run.
    let evidence = runtime_closure(
        &fixture,
        run,
        TerminalOutcome::Succeeded,
        ObservedRunState::Succeeded,
        "own-terminal",
        7,
    );
    let current = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");

    // 1. Evidence belonging to a different run is refused.
    let sibling_events = {
        let sibling_current = fixture
            .store
            .get_agent_run(fixture.project, sibling)
            .expect("the read succeeds")
            .expect("the sibling exists");
        fixture
            .store
            .record_observation(&NewObservation {
                event: NewRuntimeEvent {
                    project_id: fixture.project,
                    agent_run_id: sibling,
                    identity: NativeRuntimeIdentity {
                        runtime_kind: RuntimeKindKey::parse("zz.runtime").expect("a valid key"),
                        host: name("host-2"),
                        generation: 1,
                        native_id: external("session-2"),
                    },
                    native_event_id: Some(external("s-1")),
                    native_sequence: 1,
                    payload: document("sibling-terminal"),
                    observed_at: now(),
                },
                observed: ObservedRunState::Succeeded,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: sibling_current.revision,
                quota_state: None,
            })
            .expect("the sibling observation is recorded");
        fixture
            .store
            .read_runtime_events(fixture.project, sibling, None)
            .expect("the read succeeds")
    };
    // Everything from here on must add nothing at all.
    let before = census(&fixture);
    let borrowed = TerminalEvidence {
        source: TerminalEvidenceSource::RuntimeObservation {
            cursor: sibling_events[0].cursor,
        },
        evidence_hash: sibling_events[0].payload.hash().clone(),
        ..evidence.clone()
    };
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: borrowed,
            })
            .is_err(),
        "another run's event must not close this run"
    );

    // 2. A digest that does not match the cited event is refused.
    let wrong_hash = TerminalEvidence {
        evidence_hash: ContentHash::of(b"not the payload"),
        ..evidence.clone()
    };
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: wrong_hash,
            })
            .is_err()
    );

    // 3. An outcome the cited event does not evidence is refused.
    let wrong_outcome = TerminalEvidence {
        outcome: TerminalOutcome::Failed,
        ..evidence.clone()
    };
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: wrong_outcome,
            })
            .is_err()
    );

    // 4. An operator receipt cannot claim a runtime verdict.
    let forged = TerminalEvidence {
        outcome: TerminalOutcome::Cancelled,
        source: TerminalEvidenceSource::OperatorAbandon {
            receipt_id: CommandReceiptId::generate(),
        },
        ..evidence.clone()
    };
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: forged,
            })
            .is_err(),
        "an operator receipt can only evidence an abandoned run"
    );

    assert_unchanged(&before, &census(&fixture), "unbound terminal evidence");

    // A late, OLDER terminal event is still appended as raw evidence, but the
    // monotonic guard refused to reduce it — so it never became this run's
    // observed truth and must not be able to close it either. Without this
    // check, closure is a back door around the ordering rule.
    //
    // Appending that raw event is itself a legitimate write, so the zero-state
    // baseline is retaken across it rather than around it.
    let stale = sequenced_event(&fixture, run, Some("stale-terminal"), "stale-terminal", 1);
    let stale_hash = stale.payload.hash().clone();
    fixture
        .store
        .record_observation(&NewObservation {
            event: stale,
            observed: ObservedRunState::Failed,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: current.revision,
            quota_state: None,
        })
        .expect("an older event is still appended");
    let stale_cursor = fixture
        .store
        .read_runtime_events(fixture.project, run, None)
        .expect("the read succeeds")
        .into_iter()
        .find(|event| event.payload.hash() == &stale_hash)
        .expect("the older event was appended")
        .cursor;
    let before = census(&fixture);
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: TerminalEvidence {
                    outcome: TerminalOutcome::Failed,
                    source: TerminalEvidenceSource::RuntimeObservation {
                        cursor: stale_cursor
                    },
                    evidence_hash: stale_hash,
                    closed_at: at("2026-08-09T11:00:00Z"),
                },
            })
            .is_err(),
        "an event the projection never reduced must not close the run"
    );
    assert_unchanged(
        &before,
        &census(&fixture),
        "closure cited an unreduced event",
    );
    assert_eq!(
        fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .revision,
        current.revision,
        "no refused closure may move the revision"
    );

    // The genuine, bound evidence closes the run.
    fixture
        .store
        .close_agent_run(&RunClosure {
            project_id: fixture.project,
            agent_run_id: run,
            expected_revision: current.revision,
            evidence,
        })
        .expect("bound evidence closes the run");
    let closed = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(closed.projection.is_closed());
    assert_eq!(
        closed.terminal.expect("evidence is stored").outcome,
        TerminalOutcome::Succeeded
    );
}

#[test]
fn sensitive_material_is_rejected_from_every_persisted_string_category_without_echo() {
    let fixture = fixture();
    let before = census(&fixture);
    let canary = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";

    // The typed constructors are the choke point: a credential cannot even be
    // built into a value the store would accept, so it never reaches SQL.
    let rejected: Vec<Result<(), kontor_core::DomainError>> = vec![
        ExternalName::parse(canary).map(|_| ()),
        ExternalId::parse(canary).map(|_| ()),
        BoundedText::parse(canary).map(|_| ()),
        IdempotencyKey::parse(canary).map(|_| ()),
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "envelope": { "note": canary }
        }))
        .map(|_| ()),
    ];
    for (index, outcome) in rejected.iter().enumerate() {
        let error = outcome
            .as_ref()
            .err()
            .unwrap_or_else(|| panic!("category {index} accepted the canary"));
        assert!(
            !error.to_string().contains(canary) && !format!("{error:?}").contains(canary),
            "category {index} echoed the canary"
        );
    }

    // A project name is a persisted string like any other: the write is refused
    // before SQL and nothing lands.
    assert!(ExternalName::parse(canary).is_err());
    assert_unchanged(&before, &census(&fixture), "sensitive project name");

    // A source envelope carrying a credential is refused with the event.
    let bad_envelope = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "kind": "request.created",
        "authorization": "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"
    }));
    assert!(
        bad_envelope.is_err(),
        "an unredacted envelope must not be constructible"
    );
    assert_unchanged(&before, &census(&fixture), "sensitive source envelope");

    // And the whole database is still empty of anything the canary touched.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    for table in [
        "projects",
        "source_events",
        "command_receipts",
        "runtime_events",
    ] {
        let hits: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("countable");
        assert_eq!(
            hits,
            before.get(table).copied().unwrap_or_default(),
            "`{table}` grew despite every write being refused"
        );
    }
}

// ---------------------------------------------------------------------------
// Normalized relationships, persona snapshots, successor intake, team lifecycle
// ---------------------------------------------------------------------------

#[test]
fn cross_project_targets_tasks_evidence_pins_and_trigger_refs_fail_without_partial_rows() {
    let RunFixture { fixture, run } = with_run(true);
    let before = census(&fixture);
    let state_before = run_state(&fixture, run);

    // 1. A command target belonging to another project. The normalized row's
    //    composite FK refuses it even though the receipt itself looks fine.
    let foreign_task = NewCommandIntent {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("target-foreign").expect("a valid key"),
        kind: CommandKind::ResumeTask,
        target: AggregateRef::Task {
            task_id: fixture.other_task,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: document("resume"),
        payload: document("resume-payload"),
        desired: None,
        not_before: now(),
        created_at: now(),
    };
    assert!(
        fixture.store.record_intent(&foreign_task).is_err(),
        "a command target from another project must be refused"
    );
    assert_unchanged(&before, &census(&fixture), "cross-project command target");

    // 2. An authorization selecting a task from another project.
    let foreign_selection = ExecutionAuthorization {
        id: kontor_core::id::ExecutionAuthorizationId::generate(),
        project_id: fixture.project,
        scope: WorkScope::Project,
        selected_tasks: vec![fixture.other_task],
        allowed_start: TimeRange {
            start: at("2026-08-09T10:00:00Z"),
            end: at("2026-08-09T18:00:00Z"),
        },
        max_concurrency: 1,
        budget: budget(),
        created_by: fixture.account,
        capability_receipt: CommandReceiptId::generate(),
        created_at: now(),
    };
    assert!(
        fixture
            .store
            .insert_authorization(&foreign_selection)
            .is_err(),
        "a selected task from another project must be refused"
    );
    assert_unchanged(
        &before,
        &census(&fixture),
        "cross-project authorization task",
    );

    // 3. An intake receipt pinned to a trigger revision that does not exist.
    //
    // The *decision* is refused, and the event it was about stays. Intake
    // commits the canonical identity before anything evaluates it, so a failed
    // evaluation leaves a stored, undecided event — which is the state a retry
    // resumes from — rather than discarding evidence a source system already
    // handed over. What must not appear is a receipt.
    let source = source_event(SourceEventId::generate(), "ext-pin", "payload");
    let receipt = intake(&source, "intake-pin");
    assert!(
        fixture
            .store
            .record_source_event(&NewSourceEvent {
                project_id: fixture.project,
                event: source.clone(),
                receipt,
            })
            .is_err(),
        "a receipt must pin a trigger revision that exists"
    );
    let after_pin = census(&fixture);
    assert_eq!(
        after_pin.get("source_events"),
        before.get("source_events").map(|count| count + 1).as_ref(),
        "the event is durable even though the decision about it was refused"
    );
    assert_eq!(
        after_pin.get("intake_receipts"),
        before.get("intake_receipts"),
        "no decision was recorded"
    );
    assert!(
        matches!(
            fixture
                .store
                .ingest_source_event(fixture.project, &source)
                .expect("re-ingesting a stored event is not an error"),
            SourceEventIngest::Unevaluated(_)
        ),
        "the stored event is resumable, not a duplicate of a decision nobody made"
    );

    // 4. A persona snapshot whose task belongs to another project.
    let workflow = with_workflow(&fixture);
    let persona: PersonaScenarioSpec =
        serde_json::from_str(PERSONA_SCENARIO).expect("the persona fixture parses");
    fixture
        .store
        .insert_persona_scenario(fixture.project, &persona)
        .expect("the scenario revision is stored");
    let with_persona = census(&fixture);
    assert!(
        fixture
            .store
            .create_task_persona_snapshot(&NewTaskPersonaSnapshot {
                project_id: fixture.project,
                task_id: fixture.other_task,
                workflow_id: workflow,
                scenario_id: persona.scenario_id,
                version: persona.version,
                created_at: now(),
            })
            .is_err(),
        "a persona snapshot must not attach to another project's task"
    );
    assert_unchanged(
        &with_persona,
        &census(&fixture),
        "cross-project persona snapshot",
    );

    // 5. Terminal evidence citing a receipt from another project.
    let foreign_evidence = TerminalEvidence {
        outcome: TerminalOutcome::Abandoned,
        source: TerminalEvidenceSource::OperatorAbandon {
            receipt_id: CommandReceiptId::generate(),
        },
        evidence_hash: ContentHash::of(b"evidence"),
        closed_at: at("2026-08-09T11:00:00Z"),
    };
    assert!(
        fixture
            .store
            .close_agent_run(&RunClosure {
                project_id: fixture.project,
                agent_run_id: run,
                expected_revision: state_before.0,
                evidence: foreign_evidence,
            })
            .is_err()
    );
    assert_eq!(
        run_state(&fixture, run),
        state_before,
        "no refused write may move a revision or a state"
    );
}

#[test]
fn persona_snapshot_persists_reopens_and_rejects_cross_project_or_wrong_gate() {
    let fixture = fixture();
    let workflow = with_workflow(&fixture);

    // The fixture persona names the profile's gate and an evaluator the gate
    // authorizes.
    let mut persona: PersonaScenarioSpec =
        serde_json::from_str(PERSONA_SCENARIO).expect("the persona fixture parses");
    persona.gate_under_test = GateKey::parse("zz.gate").expect("a valid gate key");
    persona.actor_role = role("zz.persona");
    persona.evaluator_roles = vec![role("zz.reviewer")];
    fixture
        .store
        .insert_persona_scenario(fixture.project, &persona)
        .expect("the scenario revision is stored");

    let request = NewTaskPersonaSnapshot {
        project_id: fixture.project,
        task_id: fixture.task,
        workflow_id: workflow,
        scenario_id: persona.scenario_id,
        version: persona.version,
        created_at: now(),
    };
    let snapshot = fixture
        .store
        .create_task_persona_snapshot(&request)
        .expect("an authorized scenario freezes onto the task");
    assert_eq!(snapshot.definition.gate_under_test, persona.gate_under_test);

    // It reopens byte-identically, with its digest revalidated on read.
    let reopened = fixture
        .store
        .get_task_persona_snapshot(
            fixture.project,
            fixture.task,
            persona.scenario_id,
            persona.version,
        )
        .expect("the read succeeds")
        .expect("the snapshot exists");
    assert_eq!(reopened, snapshot);
    assert_eq!(
        reopened.definition_hash,
        persona
            .canonicalize()
            .expect("canonicalizes")
            .hash()
            .clone()
    );

    // Another project cannot see it.
    assert!(
        fixture
            .store
            .get_task_persona_snapshot(
                fixture.other_project,
                fixture.task,
                persona.scenario_id,
                persona.version
            )
            .expect("the read succeeds")
            .is_none()
    );

    // There is no update or delete operation, and the row is immutable in SQL.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    assert!(
        connection
            .execute("UPDATE task_persona_snapshots SET snapshot = '{}'", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM task_persona_snapshots", [])
            .is_err()
    );

    // A scenario whose gate the pinned profile does not declare is refused.
    let mut wrong_gate = persona.clone();
    wrong_gate.scenario_id = kontor_core::id::PersonaScenarioId::generate();
    wrong_gate.gate_under_test = GateKey::parse("zz.absent").expect("a valid gate key");
    fixture
        .store
        .insert_persona_scenario(fixture.project, &wrong_gate)
        .expect("the scenario revision is stored");
    let before = census(&fixture);
    assert!(
        fixture
            .store
            .create_task_persona_snapshot(&NewTaskPersonaSnapshot {
                scenario_id: wrong_gate.scenario_id,
                ..request.clone()
            })
            .is_err(),
        "a gate the pinned profile does not declare must be refused"
    );
    assert_unchanged(&before, &census(&fixture), "wrong-gate persona snapshot");

    // So is a persona that would evaluate its own gate.
    let mut self_evaluating = persona.clone();
    self_evaluating.scenario_id = kontor_core::id::PersonaScenarioId::generate();
    self_evaluating.actor_role = role("zz.reviewer");
    self_evaluating.evaluator_roles = vec![role("zz.waiver")];
    fixture
        .store
        .insert_persona_scenario(fixture.project, &self_evaluating)
        .expect("the scenario revision is stored");
    let before = census(&fixture);
    assert!(
        fixture
            .store
            .create_task_persona_snapshot(&NewTaskPersonaSnapshot {
                scenario_id: self_evaluating.scenario_id,
                ..request
            })
            .is_err(),
        "the acting persona must not hold authority over its own gate"
    );
    assert_unchanged(
        &before,
        &census(&fixture),
        "self-approving persona snapshot",
    );
}

#[test]
fn an_existing_source_event_can_create_one_linked_receipt_per_newer_trigger_revision() {
    let fixture = fixture();
    with_trigger(&fixture, SpecVersion::FIRST);

    let source = source_event(SourceEventId::generate(), "ext-1", "payload");
    let first = intake(&source, "intake-1");
    fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: source.clone(),
            receipt: first.clone(),
        })
        .expect("the event and its first decision are recorded");

    let after_first = census(&fixture);
    assert_eq!(after_first.get("source_events"), Some(&1));
    assert_eq!(after_first.get("intake_receipts"), Some(&1));

    let reevaluation = |version: SpecVersion, key: &str| {
        let mut receipt = intake(&source, key);
        receipt.trigger_version = version;
        NewIntakeReevaluation {
            project_id: fixture.project,
            source_event_id: source.id,
            source_event_hash: source.envelope.hash().clone(),
            receipt,
        }
    };

    // The same revision is idempotent — but only for the *same* decision. A
    // trigger revision is deterministic, so a genuine replay re-derives the same
    // verdict and the same idempotency key.
    match fixture
        .store
        .reevaluate_source_event(&reevaluation(SpecVersion::FIRST, "intake-1"))
        .expect("a same-revision replay is not an error")
    {
        ReevaluationOutcome::AlreadyDecided(existing) => assert_eq!(existing.id, first.id),
        other => panic!("expected the existing decision, got {other:?}"),
    }
    assert_unchanged(&after_first, &census(&fixture), "same-revision replay");

    // The same revision proposing a DIFFERENT decision is a contradiction, not
    // a replay: one pinned revision cannot have decided two ways about one
    // event. Returning the stored receipt here would silently swallow the
    // disagreement.
    assert!(
        fixture
            .store
            .reevaluate_source_event(&reevaluation(SpecVersion::FIRST, "intake-different-key"))
            .is_err(),
        "a different idempotency key under the same revision is not a replay"
    );
    let mut different_verdict = reevaluation(SpecVersion::FIRST, "intake-1");
    different_verdict.receipt.result = IntakeResult::Ignored;
    assert!(
        fixture
            .store
            .reevaluate_source_event(&different_verdict)
            .is_err(),
        "a different verdict under the same revision is not a replay"
    );
    assert_unchanged(
        &after_first,
        &census(&fixture),
        "contradicting same-revision decision",
    );

    // A successor must decide the event the request names, not some other one.
    let elsewhere = source_event(SourceEventId::generate(), "ext-other", "other");
    let mut misfiled = reevaluation(SpecVersion::FIRST, "intake-1");
    misfiled.receipt.source_event_id = elsewhere.id;
    assert!(
        fixture.store.reevaluate_source_event(&misfiled).is_err(),
        "the decision must be about the event being re-evaluated"
    );
    let mut wrong_digest = reevaluation(SpecVersion::FIRST, "intake-1");
    wrong_digest.receipt.source_event_hash = ContentHash::of(b"a different envelope");
    assert!(
        fixture
            .store
            .reevaluate_source_event(&wrong_digest)
            .is_err(),
        "the decision must cite the digest the request proved"
    );
    assert_unchanged(&after_first, &census(&fixture), "misfiled re-evaluation");

    // A revision that does not exist cannot be pinned to.
    let second_version = SpecVersion::FIRST.next().expect("a next version");
    assert!(
        fixture
            .store
            .reevaluate_source_event(&reevaluation(second_version, "intake-2"))
            .is_err(),
        "a receipt must pin a trigger revision that exists"
    );
    assert_unchanged(&after_first, &census(&fixture), "missing trigger revision");

    // A strictly newer revision adds exactly one linked successor — and no
    // second source event or work graph.
    with_trigger(&fixture, second_version);
    let successor = match fixture
        .store
        .reevaluate_source_event(&reevaluation(second_version, "intake-2"))
        .expect("a newer revision supersedes")
    {
        ReevaluationOutcome::Superseded(receipt) => *receipt,
        other => panic!("expected a successor, got {other:?}"),
    };
    assert_eq!(
        successor.predecessor_receipt_id,
        Some(first.id),
        "the successor links to the decision it supersedes"
    );
    let after_second = census(&fixture);
    assert_eq!(
        after_second.get("source_events"),
        Some(&1),
        "re-evaluation never creates a second source event"
    );
    assert_eq!(after_second.get("intake_receipts"), Some(&2));
    assert_eq!(
        after_second.get("tasks"),
        after_first.get("tasks"),
        "re-evaluation never auto-creates a work graph"
    );

    // The original decision is untouched.
    assert_eq!(
        fixture
            .store
            .get_intake_receipt(fixture.project, first.id)
            .expect("the read succeeds")
            .expect("the original exists")
            .predecessor_receipt_id,
        None
    );

    // An older revision cannot supersede a newer decision.
    assert!(
        fixture
            .store
            .reevaluate_source_event(&reevaluation(SpecVersion::FIRST, "intake-old"))
            .is_err(),
        "an older revision must not supersede a newer decision"
    );

    // Neither can a changed source digest.
    let mut tampered = reevaluation(second_version, "intake-3");
    tampered.source_event_hash = ContentHash::of(b"a different envelope");
    assert!(fixture.store.reevaluate_source_event(&tampered).is_err());
    assert_unchanged(&after_second, &census(&fixture), "changed source digest");

    // Nor a cross-project event.
    let mut foreign = reevaluation(second_version, "intake-4");
    foreign.project_id = fixture.other_project;
    assert!(fixture.store.reevaluate_source_event(&foreign).is_err());
    assert_unchanged(
        &after_second,
        &census(&fixture),
        "cross-project re-evaluation",
    );
}

#[test]
fn team_run_advances_and_closes_with_cas_and_bound_evidence() {
    let RunFixture { fixture, run } = with_run(true);
    let existing = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    let team = existing.team_run_id;

    let stored = fixture
        .store
        .get_team_run(fixture.project, team)
        .expect("the read succeeds")
        .expect("the team exists");
    assert_eq!(stored.lifecycle, RunLifecycle::Queued);

    // A terminal value is never reached through an advance.
    assert!(
        fixture
            .store
            .advance_team_run(&TeamRunAdvance {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: stored.revision,
                to: RunLifecycle::Succeeded,
                occurred_at: now(),
            })
            .is_err(),
        "closure is evidence-bearing, not an advance"
    );
    // Nor through an illegal one.
    assert!(
        fixture
            .store
            .advance_team_run(&TeamRunAdvance {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: stored.revision,
                to: RunLifecycle::WaitingInput,
                occurred_at: now(),
            })
            .is_err(),
        "queued cannot jump straight to waiting_input"
    );

    let second = fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: fixture.project,
            team_run_id: team,
            expected_revision: stored.revision,
            to: RunLifecycle::Launching,
            occurred_at: now(),
        })
        .expect("a declared advance succeeds");
    assert_eq!(second.get(), stored.revision.get() + 1);

    // A stale expectation is refused.
    assert!(
        fixture
            .store
            .advance_team_run(&TeamRunAdvance {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: stored.revision,
                to: RunLifecycle::Running,
                occurred_at: now(),
            })
            .is_err(),
        "a stale revision must be refused"
    );
    let running = fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: fixture.project,
            team_run_id: team,
            expected_revision: second,
            to: RunLifecycle::Running,
            occurred_at: now(),
        })
        .expect("the team runs");

    // Closing while a child is still open is refused: the outcome *and* the
    // digest are computed from the children's persisted rows, never asserted by
    // the caller. `digest_of` reads them back through the port, so the evidence
    // this test submits is genuinely bound to what is in the database.
    let digest_of = |children: &[AgentRunId]| {
        let evidence: Vec<TeamChildEvidence> = children
            .iter()
            .map(|id| {
                let stored = fixture
                    .store
                    .get_agent_run(fixture.project, *id)
                    .expect("the read succeeds")
                    .expect("the child exists");
                TeamChildEvidence {
                    agent_run_id: *id,
                    lifecycle: stored.projection.lifecycle,
                    evidence_hash: stored.terminal.map(|t| t.evidence_hash),
                }
            })
            .collect();
        // Computed here rather than by the production helper. Taking the
        // expected value from `team_child_evidence_digest` would make the
        // repository boundary agree with whatever that function currently does,
        // including a change to the shape it hashes.
        independent_team_digest(&evidence)
    };
    let child_evidence = |outcome: TerminalOutcome| TeamTerminalEvidence {
        outcome,
        source: TeamEvidenceSource::ChildEvidence { team_run_id: team },
        evidence_hash: digest_of(&[run]),
        closed_at: at("2026-08-09T12:00:00Z"),
    };
    let before = census(&fixture);
    assert!(
        fixture
            .store
            .close_team_run(&TeamRunClosure {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: running,
                evidence: child_evidence(TerminalOutcome::Succeeded),
            })
            .is_err(),
        "an open child must block team closure"
    );
    assert_unchanged(&before, &census(&fixture), "team closed with an open child");

    // Close the only child successfully.
    let closure = runtime_closure(
        &fixture,
        run,
        TerminalOutcome::Succeeded,
        ObservedRunState::Succeeded,
        "child-terminal",
        4,
    );
    let child = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    fixture
        .store
        .close_agent_run(&RunClosure {
            project_id: fixture.project,
            agent_run_id: run,
            expected_revision: child.revision,
            evidence: closure,
        })
        .expect("the child closes");

    // An outcome the children do not compute is refused.
    assert!(
        fixture
            .store
            .close_team_run(&TeamRunClosure {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: running,
                evidence: child_evidence(TerminalOutcome::Failed),
            })
            .is_err(),
        "the claimed outcome must match what the children compute"
    );
    // So is evidence citing a different team.
    assert!(
        fixture
            .store
            .close_team_run(&TeamRunClosure {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: running,
                evidence: TeamTerminalEvidence {
                    source: TeamEvidenceSource::ChildEvidence {
                        team_run_id: TeamRunId::generate(),
                    },
                    ..child_evidence(TerminalOutcome::Succeeded)
                },
            })
            .is_err(),
        "child evidence must belong to the team being closed"
    );
    // And an operator receipt claiming a runtime verdict.
    assert!(
        fixture
            .store
            .close_team_run(&TeamRunClosure {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: running,
                evidence: TeamTerminalEvidence {
                    source: TeamEvidenceSource::OperatorAbandon {
                        receipt_id: CommandReceiptId::generate(),
                    },
                    ..child_evidence(TerminalOutcome::Succeeded)
                },
            })
            .is_err(),
        "an operator can only abandon a team"
    );

    fixture
        .store
        .close_team_run(&TeamRunClosure {
            project_id: fixture.project,
            team_run_id: team,
            expected_revision: running,
            evidence: child_evidence(TerminalOutcome::Succeeded),
        })
        .expect("computed child evidence closes the team");

    let closed = fixture
        .store
        .get_team_run(fixture.project, team)
        .expect("the read succeeds")
        .expect("the team exists");
    assert_eq!(closed.lifecycle, RunLifecycle::Succeeded);
    assert_eq!(
        closed.terminal.expect("evidence is stored").outcome,
        TerminalOutcome::Succeeded
    );
    assert!(closed.closed_at.is_some());

    // A closed team neither advances nor closes again.
    assert!(
        fixture
            .store
            .advance_team_run(&TeamRunAdvance {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: closed.revision,
                to: RunLifecycle::Running,
                occurred_at: now(),
            })
            .is_err()
    );
    assert!(
        fixture
            .store
            .close_team_run(&TeamRunClosure {
                project_id: fixture.project,
                team_run_id: team,
                expected_revision: closed.revision,
                evidence: child_evidence(TerminalOutcome::Succeeded),
            })
            .is_err()
    );
}

#[test]
fn an_empty_ledger_snapshot_resumes_without_a_gap_or_an_overlap() {
    let RunFixture { fixture, run } = with_run(true);

    // A snapshot of a run whose ledger is still empty has to name *some*
    // position, and a subscriber resumes strictly after it. Cursor 1 is
    // reserved as that origin, so no event can ever land on it.
    let snapshot = fixture
        .store
        .snapshot_agent_run(fixture.project, run)
        .expect("the snapshot succeeds");
    let origin = snapshot.cursor();
    assert!(
        fixture
            .store
            .read_events_after(fixture.project, run, Some(origin))
            .expect("the read succeeds")
            .is_empty(),
        "an empty ledger has nothing after its snapshot"
    );

    // The first real event must still be delivered to a subscriber that resumed
    // at the snapshot. This is the exact off-by-one: if the first event could
    // also be cursor 1, resuming strictly after 1 would silently skip it.
    let state = run_state(&fixture, run);
    fixture
        .store
        .record_observation(&NewObservation {
            event: sequenced_event(&fixture, run, Some("first"), "first", 1),
            observed: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: state.0,
            quota_state: None,
        })
        .expect("the first observation is recorded");

    let resumed = fixture
        .store
        .read_events_after(fixture.project, run, Some(origin))
        .expect("the read succeeds");
    assert_eq!(
        resumed.len(),
        1,
        "the first event after an empty-ledger snapshot must not be skipped"
    );
    assert!(
        resumed[0].cursor > origin.cursor,
        "every event is strictly after the origin"
    );

    // Resuming at that event returns nothing again: strictly-after has no
    // duplicate either.
    let latest = fixture
        .store
        .snapshot_agent_run(fixture.project, run)
        .expect("the snapshot succeeds");
    assert_eq!(latest.snapshot_cursor, resumed[0].cursor);
    assert!(
        fixture
            .store
            .read_events_after(fixture.project, run, Some(latest.cursor()))
            .expect("the read succeeds")
            .is_empty(),
        "resuming at the newest event delivers it no second time"
    );
}

// ---------------------------------------------------------------------------
// Account profiles
// ---------------------------------------------------------------------------

#[test]
fn an_account_profile_round_trips_through_a_reopen() {
    let fixture = fixture();
    let id = AccountProfileId::generate();
    let created = fixture
        .store
        .create_account_profile(&account_profile(id, fixture.project, "Alpha"))
        .expect("the profile is created");
    assert_eq!(created.revision, AggregateRevision::INITIAL);
    assert_eq!(created.updated_at, created.created_at);

    // Reopening proves the fields are on disk rather than in the returned value.
    drop(fixture.store);
    let store = SqliteStore::open(&fixture.path).expect("the store reopens");
    let loaded = store
        .get_account_profile(fixture.project, id)
        .expect("the read succeeds")
        .expect("the profile survives a reopen");
    assert_eq!(loaded, created);
    assert_eq!(loaded.credential_ref.alias.as_str(), "zz-alpha");
    assert_eq!(
        loaded.credential_ref.kind,
        CredentialReferenceKind::ConfigHome
    );
    assert!(loaded.enabled);
}

#[test]
fn account_profiles_are_listed_and_read_per_project() {
    let fixture = fixture();
    let mine = AccountProfileId::generate();
    let theirs = AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&account_profile(mine, fixture.project, "Mine"))
        .expect("the profile is created");
    fixture
        .store
        .create_account_profile(&account_profile(theirs, fixture.other_project, "Theirs"))
        .expect("the profile is created");

    // A valid id belonging to another project resolves to nothing, in both the
    // point read and the list.
    assert!(
        fixture
            .store
            .get_account_profile(fixture.project, theirs)
            .expect("the read succeeds")
            .is_none()
    );
    let listed: Vec<AccountProfileId> = fixture
        .store
        .list_account_profiles(fixture.project)
        .expect("the list succeeds")
        .into_iter()
        .map(|profile| profile.id)
        .collect();
    assert!(listed.contains(&mine));
    assert!(!listed.contains(&theirs));
    assert!(listed.contains(&fixture.account));
}

#[test]
fn an_account_profile_update_is_a_compare_and_swap() {
    let fixture = fixture();
    let id = AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&account_profile(id, fixture.project, "Alpha"))
        .expect("the profile is created");

    let update = AccountProfileUpdate {
        project_id: fixture.project,
        id,
        expected_revision: AggregateRevision::INITIAL,
        label: name("Alpha renamed"),
        enabled: false,
        updated_at: now(),
    };
    let updated = fixture
        .store
        .update_account_profile(&update)
        .expect("the first update succeeds");
    assert_eq!(updated.label.as_str(), "Alpha renamed");
    assert!(!updated.enabled);
    assert_eq!(updated.revision.get(), 2);

    // Replaying the same expected revision now conflicts, and writes nothing.
    let error = fixture
        .store
        .update_account_profile(&update)
        .expect_err("a stale revision must be refused");
    assert!(
        matches!(
            error,
            RepositoryError::Domain(kontor_core::DomainError::RevisionConflict {
                expected: 1,
                found: 2,
                ..
            })
        ),
        "expected a revision conflict, got {error:?}"
    );
    let unchanged = fixture
        .store
        .get_account_profile(fixture.project, id)
        .expect("the read succeeds")
        .expect("the profile is still there");
    assert_eq!(unchanged, updated, "a refused update writes nothing");
}

#[test]
fn an_account_profile_from_another_project_is_never_updated() {
    let fixture = fixture();
    let id = AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&account_profile(id, fixture.other_project, "Theirs"))
        .expect("the profile is created");

    let error = fixture
        .store
        .update_account_profile(&AccountProfileUpdate {
            project_id: fixture.project,
            id,
            expected_revision: AggregateRevision::INITIAL,
            label: name("Stolen"),
            enabled: false,
            updated_at: now(),
        })
        .expect_err("a profile in another project must not resolve");
    assert!(matches!(error, RepositoryError::NotFound { .. }));
    assert!(
        fixture
            .store
            .get_account_profile(fixture.other_project, id)
            .expect("the read succeeds")
            .expect("the profile is untouched")
            .enabled
    );
}

#[test]
fn a_referenced_account_profile_cannot_be_deleted() {
    // `with_run` pins its agent run to `fixture.account`, so the schema's
    // `ON DELETE RESTRICT` reference is what refuses the delete.
    let RunFixture { fixture, .. } = with_run(false);

    let error = fixture
        .store
        .delete_account_profile(fixture.project, fixture.account, AggregateRevision::INITIAL)
        .expect_err("a referenced profile must not be deleted");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "expected a conflict, got {error:?}"
    );
    assert!(
        fixture
            .store
            .get_account_profile(fixture.project, fixture.account)
            .expect("the read succeeds")
            .is_some(),
        "a refused delete leaves the profile in place"
    );

    // Disabling it is the supported retirement path, and it still works.
    fixture
        .store
        .update_account_profile(&AccountProfileUpdate {
            project_id: fixture.project,
            id: fixture.account,
            expected_revision: AggregateRevision::INITIAL,
            label: name("Account"),
            enabled: false,
            updated_at: now(),
        })
        .expect("a referenced profile may be disabled");
}

#[test]
fn an_unreferenced_account_profile_is_deleted_under_a_compare_and_swap() {
    let fixture = fixture();
    let id = AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&account_profile(id, fixture.project, "Spare"))
        .expect("the profile is created");

    let stale = AggregateRevision::parse(9).expect("a positive revision");
    let error = fixture
        .store
        .delete_account_profile(fixture.project, id, stale)
        .expect_err("a stale revision must be refused");
    assert!(
        matches!(
            error,
            RepositoryError::Domain(kontor_core::DomainError::RevisionConflict { .. })
        ),
        "expected a revision conflict, got {error:?}"
    );
    assert!(
        fixture
            .store
            .get_account_profile(fixture.project, id)
            .expect("the read succeeds")
            .is_some()
    );

    fixture
        .store
        .delete_account_profile(fixture.project, id, AggregateRevision::INITIAL)
        .expect("an unreferenced profile at the expected revision is deleted");
    assert!(
        fixture
            .store
            .get_account_profile(fixture.project, id)
            .expect("the read succeeds")
            .is_none()
    );
}

#[test]
fn an_account_profile_credential_identity_cannot_be_edited_by_direct_sql() {
    let fixture = fixture();
    let id = AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&account_profile(id, fixture.project, "Alpha"))
        .expect("the profile is created");
    drop(fixture.store);

    let connection = rusqlite::Connection::open(&fixture.path).expect("a raw connection opens");
    for statement in [
        "UPDATE account_profiles SET harness = 'zz.other', revision = revision + 1",
        "UPDATE account_profiles SET credential_ref_alias = 'zz-beta', revision = revision + 1",
        "UPDATE account_profiles SET credential_ref_kind = 'keychain', revision = revision + 1",
        "UPDATE account_profiles SET environment_refs_hash = \
         'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', \
         revision = revision + 1",
        // Even a legitimate field change must move the revision exactly one step.
        "UPDATE account_profiles SET label = 'Sneaky'",
        "UPDATE account_profiles SET label = 'Sneaky', revision = revision + 5",
        // Nulling a frozen column out is the edit a `<>` comparison would have
        // waved through: `NULL <> 'zz.runtime'` is `NULL`, which a trigger's
        // `WHEN` clause reads as "no violation". The trigger uses `IS NOT`.
        "UPDATE account_profiles SET harness = NULL, revision = revision + 1",
        "UPDATE account_profiles SET credential_ref_alias = NULL, revision = revision + 1",
        "UPDATE account_profiles SET revision = NULL",
        "UPDATE account_profiles SET enabled = NULL, revision = revision + 1",
    ] {
        connection
            .execute(statement, [])
            .expect_err("the immutability trigger must refuse this");
    }

    // An insert that omits the non-secret identity is refused outright, so a
    // v1-shaped row cannot be created by a v2 binary.
    connection
        .execute(
            "INSERT INTO account_profiles (id, project_id, label, created_at)
             VALUES ('0193f000-0000-7000-8000-0000000000e1', ?1, 'Bare', '2026-08-09T10:00:00Z')",
            [fixture.project.to_string()],
        )
        .expect_err("a profile without its credential identity must be refused");
}

#[test]
fn an_account_profile_change_from_a_foreign_realm_is_refused_before_any_write() {
    let fixture = fixture();
    let id = AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&account_profile(id, fixture.project, "Alpha"))
        .expect("the profile is created");

    let foreign = ReceiptEnvelope::new(
        RealmId::generate(),
        AccountProfileUpdate {
            project_id: fixture.project,
            id,
            expected_revision: AggregateRevision::INITIAL,
            label: name("Foreign"),
            enabled: false,
            updated_at: now(),
        },
    );
    let error = fixture
        .store
        .update_account_profile_in_realm(&foreign)
        .expect_err("a foreign realm must be refused");
    assert!(matches!(
        error,
        RepositoryError::Domain(kontor_core::DomainError::RealmMismatch { .. })
    ));

    let unchanged = fixture
        .store
        .get_account_profile(fixture.project, id)
        .expect("the read succeeds")
        .expect("the profile is untouched");
    assert_eq!(unchanged.revision, AggregateRevision::INITIAL);
    assert!(unchanged.enabled);

    // The same change under this store's own Realm is applied.
    let local = ReceiptEnvelope::new(fixture.store.realm_id(), foreign.value);
    let updated = fixture
        .store
        .update_account_profile_in_realm(&local)
        .expect("the local realm is accepted");
    assert_eq!(updated.revision.get(), 2);

    // And the snapshot leaves the Realm qualified.
    let snapshot = fixture
        .store
        .snapshot_account_profile(fixture.project, id)
        .expect("the snapshot succeeds");
    assert_eq!(snapshot.realm_id, fixture.store.realm_id());
    assert_eq!(
        snapshot
            .peek(fixture.store.realm_id())
            .expect("the realm matches")
            .as_ref()
            .expect("the profile exists")
            .revision,
        updated.revision
    );
}

// ---------------------------------------------------------------------------
// Context-window policy and compaction receipts
// ---------------------------------------------------------------------------

fn policy_snapshot(
    class: kontor_core::spec::ContextWindowClass,
    ceiling: Option<u64>,
) -> kontor_core::spec::ContextPolicySnapshot {
    let declared = kontor_core::spec::ContextWindowPolicy {
        class,
        ..kontor_core::spec::ContextWindowPolicy::standard()
    };
    let resolved =
        kontor_core::spec::resolve_context_window(&kontor_core::spec::ContextPolicyInputs {
            role_slot: Some(&declared),
            ..kontor_core::spec::ContextPolicyInputs::default()
        })
        .expect("the slot declaration resolves");
    let requested = kontor_core::spec::RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let effective = kontor_core::spec::EffectiveContextPolicy::derive(
        &requested,
        &kontor_core::spec::ContextWindowBounds {
            safe_ceiling_tokens: ceiling,
            minimum_trigger_tokens: None,
        },
        true,
    )
    .expect("the effective half derives");
    kontor_core::spec::ContextPolicySnapshot::freeze(requested, effective, now())
        .expect("both halves freeze")
}

fn compaction_receipt(
    run: AgentRunId,
    status: kontor_core::compaction::CompactionStatus,
    after: Option<NativeRuntimeIdentity>,
) -> kontor_core::compaction::CompactionReceipt {
    let snapshot = policy_snapshot(kontor_core::spec::ContextWindowClass::Standard, None);
    kontor_core::compaction::CompactionReceipt {
        schema_version: SCHEMA_VERSION,
        id: kontor_core::id::CompactionReceiptId::generate(),
        agent_run_id: run,
        binding_id: kontor_core::id::RuntimeBindingId::generate(),
        native_before: identity(1),
        native_after: after,
        requested: snapshot.requested,
        effective: snapshot.effective,
        trigger: kontor_core::compaction::CompactionTrigger::Threshold,
        capabilities: document("capabilities"),
        status,
        telemetry: kontor_core::compaction::CompactionTelemetry::unknown(),
        context_pack_hash: ContentHash::of(b"context-pack"),
        handoff_hash: None,
        evidence: (status == kontor_core::compaction::CompactionStatus::Confirmed)
            .then(|| external("compaction-evidence")),
        recorded_at: now(),
    }
}

/// MUT-CTX-04. Dropping the effective clamp evidence on the way back makes this
/// fail: the reopened projection stops equalling the persisted snapshot.
#[test]
fn a_context_policy_survives_crash_reopen_with_its_clamp_evidence() {
    let RunFixture { fixture, run } = with_run(true);
    // Deep asks for 512000; the runtime attests a lower ceiling, so the stored
    // record must carry both the request and the clamp that changed it.
    let snapshot = policy_snapshot(kontor_core::spec::ContextWindowClass::Deep, Some(200_000));
    assert_eq!(snapshot.requested.trigger_tokens, Some(512_000));
    assert_eq!(snapshot.effective.trigger_tokens, Some(200_000));
    assert_eq!(
        snapshot.effective.clamp,
        kontor_core::spec::ContextClamp::ToSafeCeiling
    );

    fixture
        .store
        .record_run_context_policy(fixture.project, run, &snapshot)
        .expect("the policy is frozen onto the run");

    // Reopen the file: nothing is held in memory across this.
    drop(fixture.store);
    let reopened = SqliteStore::open(&fixture.path).expect("the store reopens");
    let read = reopened
        .get_run_context_policy(fixture.project, run)
        .expect("the read succeeds")
        .expect("the policy survived the reopen");

    assert_eq!(read, snapshot);
    assert_eq!(read.requested.trigger_tokens, Some(512_000));
    assert_eq!(read.effective.trigger_tokens, Some(200_000));
    assert_eq!(
        read.effective.clamp,
        kontor_core::spec::ContextClamp::ToSafeCeiling
    );
    assert_eq!(
        read.effective.bounds.safe_ceiling_tokens,
        Some(200_000),
        "the ceiling the clamp was derived from survives with it"
    );
    read.verify().expect("the reopened snapshot re-verifies");
}

#[test]
fn a_run_context_policy_is_written_once_and_cannot_be_revised() {
    let RunFixture { fixture, run } = with_run(true);
    let first = policy_snapshot(kontor_core::spec::ContextWindowClass::Standard, None);
    fixture
        .store
        .record_run_context_policy(fixture.project, run, &first)
        .expect("the policy is frozen");

    // The identical pair again is a replay of the same act.
    fixture
        .store
        .record_run_context_policy(fixture.project, run, &first)
        .expect("an identical replay is the same act");

    // A different one is a second answer to a settled question.
    let revised = policy_snapshot(kontor_core::spec::ContextWindowClass::Lean, None);
    assert!(matches!(
        fixture
            .store
            .record_run_context_policy(fixture.project, run, &revised)
            .expect_err("a live run cannot be re-launched under a different policy"),
        RepositoryError::Conflict { .. }
    ));

    // The stored value is still the original.
    let read = fixture
        .store
        .get_run_context_policy(fixture.project, run)
        .expect("the read succeeds")
        .expect("the policy is there");
    assert_eq!(read, first);
}

#[test]
fn another_project_can_neither_read_nor_target_a_context_policy() {
    let RunFixture { fixture, run } = with_run(true);
    let snapshot = policy_snapshot(kontor_core::spec::ContextWindowClass::Standard, None);
    fixture
        .store
        .record_run_context_policy(fixture.project, run, &snapshot)
        .expect("the policy is frozen");

    assert_eq!(
        fixture
            .store
            .get_run_context_policy(fixture.other_project, run)
            .expect("the read succeeds"),
        None,
        "a stranger project reads nothing"
    );
    assert!(matches!(
        fixture
            .store
            .record_run_context_policy(fixture.other_project, run, &snapshot)
            .expect_err("a stranger project cannot write to this run"),
        RepositoryError::NotFound { .. }
    ));
    assert_eq!(
        fixture
            .store
            .latest_compaction_receipt(fixture.other_project, run)
            .expect("the read succeeds"),
        None
    );
}

#[test]
fn a_compaction_receipt_is_idempotent_by_id_and_conflicts_on_different_content() {
    let RunFixture { fixture, run } = with_run(true);
    let receipt = compaction_receipt(
        run,
        kontor_core::compaction::CompactionStatus::Confirmed,
        Some(identity(1)),
    );

    let stored = fixture
        .store
        .record_compaction_receipt(fixture.project, &receipt)
        .expect("the receipt is recorded");
    assert_eq!(stored, receipt);

    // The identical attempt replays to the stored row rather than writing twice.
    let replay = fixture
        .store
        .record_compaction_receipt(fixture.project, &receipt)
        .expect("an identical replay returns the original");
    assert_eq!(replay, receipt);

    // The same id carrying a different outcome is a contradiction, and the
    // stored terminal receipt is not regressed by it.
    let mut contradicting =
        compaction_receipt(run, kontor_core::compaction::CompactionStatus::Failed, None);
    contradicting.id = receipt.id;
    assert!(matches!(
        fixture
            .store
            .record_compaction_receipt(fixture.project, &contradicting)
            .expect_err("a reused id with different content conflicts"),
        RepositoryError::Conflict { .. }
    ));

    let latest = fixture
        .store
        .latest_compaction_receipt(fixture.project, run)
        .expect("the read succeeds")
        .expect("a receipt is there");
    assert_eq!(latest, receipt);
    assert_eq!(
        latest.status,
        kontor_core::compaction::CompactionStatus::Confirmed
    );
}

#[test]
fn a_confirmed_receipt_whose_session_moved_never_reaches_storage() {
    let RunFixture { fixture, run } = with_run(true);
    // A `confirmed` claim over a session in another generation is exactly the
    // drift the contract refuses, and the store refuses it too rather than
    // trusting the caller to have checked.
    let drifted = compaction_receipt(
        run,
        kontor_core::compaction::CompactionStatus::Confirmed,
        Some(identity(2)),
    );
    assert!(
        fixture
            .store
            .record_compaction_receipt(fixture.project, &drifted)
            .is_err()
    );
    assert_eq!(
        fixture
            .store
            .latest_compaction_receipt(fixture.project, run)
            .expect("the read succeeds"),
        None,
        "the refusal leaves no partial state"
    );
}

#[test]
fn compaction_receipts_survive_crash_reopen_and_report_the_most_recent() {
    let RunFixture { fixture, run } = with_run(true);
    let first = compaction_receipt(
        run,
        kontor_core::compaction::CompactionStatus::Unsupported,
        None,
    );
    let mut second = compaction_receipt(
        run,
        kontor_core::compaction::CompactionStatus::Confirmed,
        Some(identity(1)),
    );
    second.recorded_at = at("2026-08-11T09:00:00Z");
    for receipt in [&first, &second] {
        fixture
            .store
            .record_compaction_receipt(fixture.project, receipt)
            .expect("the receipt is recorded");
    }

    drop(fixture.store);
    let reopened = SqliteStore::open(&fixture.path).expect("the store reopens");
    let latest = reopened
        .latest_compaction_receipt(fixture.project, run)
        .expect("the read succeeds")
        .expect("a receipt survived");
    assert_eq!(latest, second);
    // Unknown telemetry is still unknown after a round trip through storage.
    assert!(latest.telemetry.is_unknown());
}

/// Every team-closure evidence source survives a reload as *itself*.
///
/// The settled-turn source was persisted correctly and decoded back as
/// `ChildEvidence`, because the decoder's catch-all collapsed everything it did
/// not name. That is the separate typing being quietly undone on the way out of
/// the database: a team closed because its declared slots finished their turns
/// came back claiming its child runs had ended — a statement about runs that are
/// deliberately still live.
///
/// A round trip per source, so a future variant that is persisted and not decoded
/// fails here rather than in whatever reads it next.
#[test]
fn every_team_closure_source_round_trips_as_itself() {
    let RunFixture { fixture, run } = with_run(true);
    let existing = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    let team = existing.team_run_id;

    // Drive the team to a closable lifecycle.
    let stored = fixture
        .store
        .get_team_run(fixture.project, team)
        .expect("the read succeeds")
        .expect("the team exists");
    let launching = fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: fixture.project,
            team_run_id: team,
            expected_revision: stored.revision,
            to: RunLifecycle::Launching,
            occurred_at: now(),
        })
        .expect("the team launches");
    let running = fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: fixture.project,
            team_run_id: team,
            expected_revision: launching,
            to: RunLifecycle::Running,
            occurred_at: now(),
        })
        .expect("the team runs");

    // A settled-turn closure. Its child run is deliberately left **open**: that
    // is the whole case this source exists for, and it is what makes the
    // collapsed decoding a lie rather than a harmless relabelling.
    let child = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(
        child.terminal.is_none(),
        "the seat is still live, which is the point of this source"
    );

    let evidence_hash = ContentHash::of(b"declared-slot policy digest");
    fixture
        .store
        .close_team_run(&TeamRunClosure {
            project_id: fixture.project,
            team_run_id: team,
            expected_revision: running,
            evidence: TeamTerminalEvidence {
                outcome: TerminalOutcome::Succeeded,
                source: TeamEvidenceSource::SettledTurns { team_run_id: team },
                evidence_hash: evidence_hash.clone(),
                closed_at: at("2026-08-09T12:00:00Z"),
            },
        })
        .expect("a settled-turn closure is accepted with a live child");

    let reloaded = fixture
        .store
        .get_team_run(fixture.project, team)
        .expect("the read succeeds")
        .expect("the team exists");
    let terminal = reloaded.terminal.expect("the team is closed");
    assert_eq!(
        terminal.source,
        TeamEvidenceSource::SettledTurns { team_run_id: team },
        "a settled-turn closure reloads as one, not as child evidence"
    );
    assert_eq!(terminal.outcome, TerminalOutcome::Succeeded);
    assert_eq!(terminal.evidence_hash, evidence_hash);
    // And the seat it was closed around is still open, on the way back out too.
    let after = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(
        after.terminal.is_none(),
        "closing the team on settled turns did not close its child"
    );
}

/// A team template whose frozen definition really declares role slots.
///
/// The shared fixture's template carries an opaque marker document, which is
/// fine for the tests that only need a revision to point at. The declared-slot
/// re-proof reads the slots *out of* that document, so it needs one that has
/// them.
fn slotted_team_definition(slots: &[&str]) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "slots": slots
            .iter()
            .map(|id| serde_json::json!({ "id": id }))
            .collect::<Vec<_>>(),
    }))
    .expect("a canonical document")
}

/// The declared-slot re-proof refuses a settled-turn closure whose evidence is
/// missing, and refuses it for that exact reason.
///
/// This is the defence-in-depth half of the settled-turn closure. Through public
/// operations the certifier accepts every declared slot before a team can ever be
/// marked closed, so the store check can never fire there. It exists for the
/// state this test constructs directly: a team already recorded as closed on
/// settled turns whose `role_turns` evidence no longer accounts for one of its
/// declared slots — corruption, a partial write, or a future caller that closes a
/// team another way.
#[test]
fn a_settled_turn_closure_missing_a_slots_turn_is_refused_by_the_store() {
    let fixture = fixture();

    // A template that declares two slots, and a team run frozen against it.
    let template = TeamTemplateId::generate();
    fixture
        .store
        .insert_team_template(
            fixture.project,
            &TeamTemplateRevision {
                template_id: template,
                version: SpecVersion::FIRST,
                name: name("Two-slot team"),
                definition: slotted_team_definition(&["zz.maker", "zz.checker"]),
                role_authority: Vec::new(),
            },
        )
        .expect("the team revision is stored");
    let revision = fixture
        .store
        .get_team_template(fixture.project, template, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("the revision exists");
    let team_run = TeamRunId::generate();
    fixture
        .store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: fixture.project,
            task_id: fixture.task,
            snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
            created_at: now(),
        })
        .expect("the team run is created");

    // The task's profile must *pin* that team, or no closure is demanded at all
    // and this test would prove nothing about the re-proof.
    let mut profile = work_profile();
    profile.id = kontor_core::id::WorkProfileKey::parse("zz.teamed").expect("a profile key");
    profile.team_template = Some(kontor_core::spec::TeamTemplateRef {
        template_id: template,
        version: SpecVersion::FIRST,
    });
    fixture
        .store
        .insert_work_profile(fixture.project, &profile)
        .expect("the profile is stored");
    fixture
        .store
        .create_task_workflow(&NewTaskWorkflow {
            id: TaskWorkflowId::generate(),
            project_id: fixture.project,
            task_id: fixture.task,
            snapshot: ResolvedWorkProfileSnapshot::resolve(&profile, now())
                .expect("the profile resolves"),
            current_phase: phase("zz.one"),
            created_at: now(),
        })
        .expect("the workflow is created");

    // One seat per declared slot, each settling its final turn. The runs stay
    // open on purpose: that is the state this closure basis exists for.
    let mut seats = Vec::new();
    for slot in ["zz.maker", "zz.checker"] {
        let run = AgentRunId::generate();
        fixture
            .store
            .create_agent_run(&NewAgentRun {
                id: run,
                project_id: fixture.project,
                team_run_id: team_run,
                parent_agent_run_id: None,
                role: role(slot),
                account_profile_id: Some(fixture.account),
                binding: None,
                created_at: now(),
            })
            .expect("the seat is created");
        fixture
            .store
            .settle_role_turn(&kontor_store::NewRoleTurn {
                id: kontor_core::id::RoleTurnId::generate(),
                project_id: fixture.project,
                task_id: fixture.task,
                team_run_id: team_run,
                agent_run_id: run,
                role_slot_id: kontor_core::id::RoleSlotId::parse(slot).expect("a slot"),
                idempotency_key: format!("turn-{slot}"),
                task_revision: AggregateRevision::INITIAL,
                binding_generation: 1,
                authority_tier: "operator",
                account_profile: Some(fixture.account),
                artifacts: [artifact("zz.output")].into_iter().collect(),
                evidence_hash: ContentHash::of(slot.as_bytes()),
                settled_at: now(),
            })
            .expect("the turn settles");
        seats.push(run);
    }

    // Close the team on settled turns and drive the task to the point of a
    // terminal transition.
    let launching = fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: fixture.project,
            team_run_id: team_run,
            expected_revision: AggregateRevision::INITIAL,
            to: RunLifecycle::Launching,
            occurred_at: now(),
        })
        .expect("the team launches");
    let running = fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: fixture.project,
            team_run_id: team_run,
            expected_revision: launching,
            to: RunLifecycle::Running,
            occurred_at: now(),
        })
        .expect("the team runs");
    let policy_digest = ContentHash::of(b"declared-slot policy digest");
    fixture
        .store
        .close_team_run(&TeamRunClosure {
            project_id: fixture.project,
            team_run_id: team_run,
            expected_revision: running,
            evidence: TeamTerminalEvidence {
                outcome: TerminalOutcome::Succeeded,
                source: TeamEvidenceSource::SettledTurns {
                    team_run_id: team_run,
                },
                evidence_hash: policy_digest.clone(),
                closed_at: now(),
            },
        })
        .expect("a settled-turn closure is accepted");

    let terminal = |revision: AggregateRevision| TaskTransitionRequest {
        project_id: fixture.project,
        task_id: fixture.task,
        expected_revision: revision,
        to: TaskState::Failed,
        resume_receipt: None,
        reopen: false,
        run_outcome: Some(TerminalOutcome::Failed),
        produced_artifacts: Default::default(),
        completed_phases: Default::default(),
        team_closure: TaskTeamClosure::Certified {
            team_run_id: team_run,
            policy_digest: policy_digest.clone(),
        },
        occurred_at: now(),
    };
    // With both declared slots accounted for, the store's re-proof is satisfied.
    // The re-proof runs on *terminal* transitions, so the start below is not
    // where it fires — the negative half is. What the start is for is leaving
    // the task in a state from which that terminal transition is otherwise
    // *legal*: without it, removing the check would still refuse for the wrong
    // reason and the mutation below would prove nothing.
    let started = fixture
        .store
        .transition_task(&TaskTransitionRequest {
            to: TaskState::InProgress,
            run_outcome: None,
            ..terminal(AggregateRevision::INITIAL)
        })
        .expect("both declared slots settled a turn, so the team is accounted for");

    // Now the defence-in-depth case: the same closure with one slot's evidence
    // gone. A second connection to the same file, because no public operation can
    // produce this state — which is exactly why the check exists.
    {
        let raw = rusqlite::Connection::open(&fixture.path).expect("the file reopens");
        // The row is permanent by trigger — that guard is asserted by
        // `schema_v1.rs` and is working here too, which is why it has to be
        // lifted to reach the state under test. Lifting it *is* the point: this
        // is the corruption case, and the application has no way to reach it.
        raw.execute_batch(
            "DROP TRIGGER role_turns_are_permanent;
             DELETE FROM role_turns WHERE role_slot_id = 'zz.checker';",
        )
        .expect("the row is removed");
    }

    // The very citation that was just accepted is now refused, and nothing but
    // the persisted evidence changed between the two calls.
    let refusal = fixture
        .store
        .transition_task(&terminal(started.revision))
        .expect_err("a missing declared slot must be refused");
    let text = refusal.to_string();
    assert!(
        text.contains("declared role slot") && text.contains("settled no turn"),
        "refused for the missing settled slot, not something else: {text}"
    );
}

#[test]
fn a_settled_turns_declared_artifacts_are_evidence_for_the_ticket_gate() {
    let fixture = fixture();

    // A team run and one seat, so a turn has something to settle against.
    let template = TeamTemplateId::generate();
    fixture
        .store
        .insert_team_template(
            fixture.project,
            &TeamTemplateRevision {
                template_id: template,
                version: SpecVersion::FIRST,
                name: name("One-slot team"),
                definition: slotted_team_definition(&["zz.maker"]),
                role_authority: Vec::new(),
            },
        )
        .expect("the team revision is stored");
    let revision = fixture
        .store
        .get_team_template(fixture.project, template, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("the revision exists");
    let team_run = TeamRunId::generate();
    fixture
        .store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: fixture.project,
            task_id: fixture.task,
            snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
            created_at: now(),
        })
        .expect("the team run is created");
    let run = AgentRunId::generate();
    fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: run,
            project_id: fixture.project,
            team_run_id: team_run,
            parent_agent_run_id: None,
            role: role("zz.maker"),
            account_profile_id: Some(fixture.account),
            binding: None,
            created_at: now(),
        })
        .expect("the seat is created");

    // Nothing has been produced yet, so the gate sees nothing.
    assert!(
        fixture
            .store
            .list_task_artifact_keys(fixture.project, fixture.task)
            .expect("the read succeeds")
            .is_empty(),
        "an unsettled task evidences no artifact"
    );

    // The turn declares one contract key beside the free-form labels a real turn
    // carries — a filename and a commit sha. `artifact_evidence` stays empty:
    // this is the ordinary delivery path, which registers no locator.
    fixture
        .store
        .settle_role_turn(&kontor_store::NewRoleTurn {
            id: kontor_core::id::RoleTurnId::generate(),
            project_id: fixture.project,
            task_id: fixture.task,
            team_run_id: team_run,
            agent_run_id: run,
            role_slot_id: kontor_core::id::RoleSlotId::parse("zz.maker").expect("a slot"),
            idempotency_key: "turn-artifact-evidence".to_owned(),
            task_revision: AggregateRevision::INITIAL,
            binding_generation: 1,
            authority_tier: "operator",
            account_profile: Some(fixture.account),
            artifacts: [
                artifact("zz.output"),
                artifact("architecture.md"),
                artifact("commit-6f884a0e552272d05583c3db6f4edcf414a44f40"),
            ]
            .into_iter()
            .collect(),
            evidence_hash: ContentHash::of(b"turn"),
            settled_at: now(),
        })
        .expect("the turn settles");

    let keys = fixture
        .store
        .list_task_artifact_keys(fixture.project, fixture.task)
        .expect("the read succeeds");

    // The declared contract key is evidence. Without this the completion ticket
    // gate is unsatisfiable for every task closed through `turn-settle`.
    assert!(
        keys.contains(&name("zz.output")),
        "a settled turn's declared artifact is evidence: {keys:?}"
    );
    // The free-form labels are carried through rather than failing the read.
    assert!(
        keys.contains(&name("architecture.md")),
        "a filename label does not break the read: {keys:?}"
    );
    assert_eq!(
        keys.len(),
        3,
        "every declared label is reported once: {keys:?}"
    );
}

#[test]
fn no_gate_verdict_can_turn_its_citations_into_producer_evidence() {
    let fixture = fixture();
    let workflow = with_workflow(&fixture);
    let cite = |actor: &str, verdict: GateVerdict, evidence: Vec<ArtifactKey>| NewGateEvaluation {
        project_id: fixture.project,
        workflow_id: workflow,
        gate: GateKey::parse("zz.gate").expect("a valid gate key"),
        verdict,
        evaluator_role: role(actor),
        evaluator_account: fixture.account,
        evidence,
        agent_run_id: None,
        session_evidence: None,
        reviewer_principal: None,
        policy_evaluation_id: None,
        recorded_at: now(),
    };

    // A rejection cites the artifact it looked at. Refusing the work is not
    // evidence that the artifact contract was met.
    fixture
        .store
        .append_gate_evaluation(&cite(
            "zz.reviewer",
            GateVerdict::Rejected,
            vec![artifact("zz.rejected-only")],
        ))
        .expect("the rejection records");
    // A waiver excuses the requirement; it does not produce the artifact. Only
    // the distinct waiver authority may record one, and the store makes it cite
    // the profile's required evidence just as a pass must — so this waiver names
    // `zz.output` itself. That is precisely the leak worth guarding: the artifact
    // is *named* by a verdict that never accepted the work.
    fixture
        .store
        .append_gate_evaluation(&cite(
            "zz.waiver",
            GateVerdict::Waived,
            vec![artifact("zz.output"), artifact("zz.waived-only")],
        ))
        .expect("the waiver records");

    assert!(
        fixture
            .store
            .list_task_artifact_keys(fixture.project, fixture.task)
            .expect("the read succeeds")
            .is_empty(),
        "neither a rejection nor a waiver evidences an artifact, even one it cites"
    );

    // The same evaluator then passes, citing the contract artifact. A pass is
    // still only an evaluation of producer evidence; it cannot create it.
    fixture
        .store
        .append_gate_evaluation(&cite(
            "zz.reviewer",
            GateVerdict::Passed,
            vec![artifact("zz.output")],
        ))
        .expect("the gate passes");

    assert!(
        fixture
            .store
            .list_task_artifact_keys(fixture.project, fixture.task)
            .expect("the read succeeds")
            .is_empty(),
        "a pass, rejection and waiver all cite evidence without producing it"
    );
}

/// A team definition whose second slot may be waived by the first slot's role.
fn waivable_team_definition() -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "slots": [
            {
                "id": "zz.maker",
                "role": {"role": "zz.maker", "version": 1},
                "waiver_policy": null
            },
            {
                "id": "zz.checker",
                "role": {"role": "zz.checker", "version": 1},
                "waiver_policy": {
                    "authorized_roles": ["zz.maker"],
                    "required_evidence": ["zz.output"]
                }
            }
        ],
    }))
    .expect("a canonical document")
}

/// A team run frozen against the waivable template, with a seat for the maker.
struct WaiverFixture {
    fixture: Fixture,
    team_run: TeamRunId,
    template: TeamTemplateId,
    maker_run: AgentRunId,
}

fn waiver_fixture() -> WaiverFixture {
    let fixture = fixture();
    let template = TeamTemplateId::generate();
    fixture
        .store
        .insert_team_template(
            fixture.project,
            &TeamTemplateRevision {
                template_id: template,
                version: SpecVersion::FIRST,
                name: name("Waivable team"),
                definition: waivable_team_definition(),
                role_authority: Vec::new(),
            },
        )
        .expect("the template is stored");
    let revision = fixture
        .store
        .get_team_template(fixture.project, template, SpecVersion::FIRST)
        .expect("the template is readable")
        .expect("the template exists");
    let team_run = TeamRunId::generate();
    fixture
        .store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: fixture.project,
            task_id: fixture.task,
            snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
            created_at: now(),
        })
        .expect("the team run is created");
    // Only the maker is seated. The checker is *declared but never bound*, which
    // is the state the whole waiver design exists for.
    let maker_run = AgentRunId::generate();
    fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: maker_run,
            project_id: fixture.project,
            team_run_id: team_run,
            parent_agent_run_id: None,
            role: kontor_core::id::RoleKey::parse("zz.maker").expect("a role"),
            account_profile_id: Some(fixture.account),
            binding: None,
            created_at: now(),
        })
        .expect("the seat is created");
    WaiverFixture {
        fixture,
        team_run,
        template,
        maker_run,
    }
}

fn waiver_request(fixture: &WaiverFixture, key: &str) -> kontor_store::NewRoleSlotWaiver {
    kontor_store::NewRoleSlotWaiver {
        id: kontor_core::id::RoleSlotWaiverId::generate(),
        project_id: fixture.fixture.project,
        task_id: fixture.fixture.task,
        team_run_id: fixture.team_run,
        role_slot_id: kontor_core::id::RoleSlotId::parse("zz.checker").expect("a slot"),
        idempotency_key: key.to_owned(),
        expected_team_revision: AggregateRevision::INITIAL,
        authorized_role: "zz.maker".to_owned(),
        evidence: vec!["zz.output".to_owned()],
        evidence_hash: ContentHash::of(b"waiver evidence"),
        recorded_at: now(),
    }
}

/// Journey 5 and 6. Authority, evidence and the never-bound predicate are each
/// proved inside the write transaction, and each refuses on its own terms.
#[test]
fn a_waiver_is_refused_without_policy_authority_evidence_or_an_unbound_slot() {
    let fixture = waiver_fixture();

    // A slot the frozen template does not allow waiving at all.
    let mut undeclared = waiver_request(&fixture, "w-maker");
    undeclared.role_slot_id = kontor_core::id::RoleSlotId::parse("zz.maker").expect("a slot");
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&undeclared)
        .expect_err("a slot with no policy cannot be waived");
    assert!(
        refusal.to_string().contains("does not allow"),
        "{refusal:?}"
    );

    // A role the policy does not authorize.
    let mut wrong_role = waiver_request(&fixture, "w-role");
    wrong_role.authorized_role = "zz.checker".to_owned();
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&wrong_role)
        .expect_err("an unauthorized role cannot waive");
    assert!(
        refusal.to_string().contains("cannot excuse itself"),
        "the slot's own role is refused before the roster check: {refusal:?}"
    );
    let mut stranger = waiver_request(&fixture, "w-stranger");
    stranger.authorized_role = "zz.stranger".to_owned();
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&stranger)
        .expect_err("an unauthorized role cannot waive");
    assert!(
        refusal.to_string().contains("not authorized"),
        "{refusal:?}"
    );

    // Evidence the policy demands and the waiver does not cite.
    let mut thin = waiver_request(&fixture, "w-evidence");
    thin.evidence = vec!["zz.unrelated".to_owned()];
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&thin)
        .expect_err("incomplete evidence cannot waive");
    assert!(
        refusal.to_string().contains("every evidence reference"),
        "{refusal:?}"
    );

    // A stale team revision.
    let mut stale = waiver_request(&fixture, "w-stale");
    stale.expected_team_revision = AggregateRevision::INITIAL.next().expect("a revision");
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&stale)
        .expect_err("a stale revision cannot waive");
    assert!(refusal.to_string().contains("revision"), "{refusal:?}");

    // And the never-bound predicate, which is the one that distinguishes an
    // unbound slot from an unreachable one. The checker gets a seat *and* a
    // binding, and is then no longer waivable — permanently, because the binding
    // history is what is consulted.
    let checker = AgentRunId::generate();
    fixture
        .fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: checker,
            project_id: fixture.fixture.project,
            team_run_id: fixture.team_run,
            parent_agent_run_id: None,
            role: kontor_core::id::RoleKey::parse("zz.checker").expect("a role"),
            account_profile_id: Some(fixture.fixture.account),
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: checker,
                identity: identity(7),
                bound_at: now(),
            }),
            created_at: now(),
        })
        .expect("the checker is seated and bound");
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&waiver_request(&fixture, "w-bound"))
        .expect_err("an ever-bound slot cannot be waived");
    assert!(refusal.to_string().contains("ever bound"), "{refusal:?}");
    let _ = fixture.template;
    let _ = fixture.maker_run;
}

/// Journey 4. One key replays; a second key on a waived slot is refused rather
/// than appended; the same key with different content is a conflict.
#[test]
fn a_waiver_replays_on_its_key_and_is_never_appended_to() {
    let fixture = waiver_fixture();
    let (first, applied, revision) = fixture
        .fixture
        .store
        .waive_role_slot(&waiver_request(&fixture, "w-1"))
        .expect("the waiver is recorded");
    assert_eq!(applied, kontor_store::Applied::Created);
    assert_eq!(
        revision,
        AggregateRevision::INITIAL.next().expect("a revision"),
        "recording a waiver advances the team it excuses a slot of"
    );

    // Same key, same content: the original row, and no second revision bump.
    let mut replay = waiver_request(&fixture, "w-1");
    replay.id = kontor_core::id::RoleSlotWaiverId::generate();
    let (again, applied, replayed_revision) = fixture
        .fixture
        .store
        .waive_role_slot(&replay)
        .expect("the waiver replays");
    assert_eq!(applied, kontor_store::Applied::Unchanged);
    assert_eq!(again.id, first.id, "the original row, not a new one");
    assert_eq!(replayed_revision, revision, "a replay moves nothing");

    // Same key, different content.
    let mut altered = waiver_request(&fixture, "w-1");
    altered.evidence_hash = ContentHash::of(b"different evidence");
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&altered)
        .expect_err("a key cannot waive two different things");
    assert!(
        refusal.to_string().contains("different content"),
        "{refusal:?}"
    );

    // A different key on the same slot appends nothing.
    let mut second = waiver_request(&fixture, "w-2");
    second.expected_team_revision = revision;
    let refusal = fixture
        .fixture
        .store
        .waive_role_slot(&second)
        .expect_err("a slot is waived once or not at all");
    assert!(
        refusal.to_string().contains("already waived"),
        "{refusal:?}"
    );
    assert_eq!(
        fixture
            .fixture
            .store
            .list_role_slot_waivers(fixture.fixture.project, fixture.team_run)
            .expect("the waivers are readable")
            .len(),
        1,
        "exactly one waiver survives every attempt above"
    );
}

/// Journey 7. After a waiver the database itself refuses the seat: no new run
/// for the slot, no binding, no turn, and no operational update to a placeholder.
/// Direct SQL, because these are the writes an application bug would attempt.
#[test]
fn a_waived_slot_is_refused_a_run_a_binding_a_turn_and_an_update() {
    let fixture = waiver_fixture();
    // A placeholder seat for the slot: unbound, non-terminal, and left exactly
    // as it is by the waiver. This is the BLK-009 shape the design allows.
    fixture
        .fixture
        .store
        .create_agent_run(&NewAgentRun {
            id: AgentRunId::generate(),
            project_id: fixture.fixture.project,
            team_run_id: fixture.team_run,
            parent_agent_run_id: None,
            role: kontor_core::id::RoleKey::parse("zz.checker").expect("a role"),
            account_profile_id: Some(fixture.fixture.account),
            binding: None,
            created_at: now(),
        })
        .expect("the placeholder is created");
    fixture
        .fixture
        .store
        .waive_role_slot(&waiver_request(&fixture, "w-1"))
        .expect("the waiver is recorded");

    let raw = rusqlite::Connection::open(&fixture.fixture.path).expect("the file reopens");
    let project = fixture.fixture.project.to_string();
    let team = fixture.team_run.to_string();

    for (statement, params, expected) in [
        (
            "UPDATE role_slot_waivers SET authorized_role = 'zz.other' WHERE project_id = ?1",
            vec![project.clone()],
            "immutable",
        ),
        (
            "DELETE FROM role_slot_waivers WHERE project_id = ?1",
            vec![project.clone()],
            "never removed",
        ),
    ] {
        let error = raw
            .execute(&statement.replace("?1", &format!("'{}'", params[0])), [])
            .expect_err("the waiver is permanent");
        assert!(error.to_string().contains(expected), "{error}");
    }

    // A second seat for the waived slot.
    let error = raw
        .execute(
            "INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle,
                 desired_state, observed_state, derived_state, revision, created_at)
             VALUES (?1, ?2, ?3, 'zz.checker', 'queued', 'run_requested', 'unknown', 'unknown',
                     1, '2026-08-09T10:00:00Z')",
            rusqlite::params![AgentRunId::generate().to_string(), project, team],
        )
        .expect_err("a waived slot cannot be seated");
    assert!(error.to_string().contains("cannot be seated"), "{error}");

    // An operational update to the placeholder run the waived slot may own.
    // Waiving a slot must never become a back door to declaring a run finished.
    let error = raw
        .execute(
            "UPDATE agent_runs SET lifecycle = 'succeeded', terminal_outcome = 'succeeded'
             WHERE project_id = ?1 AND role_key = 'zz.checker'",
            rusqlite::params![project.clone()],
        )
        .expect_err("a waived slot has no operable run");
    assert!(error.to_string().contains("no operable run"), "{error}");

    // A turn for the waived slot.
    let error = raw
        .execute(
            "INSERT INTO role_turns (id, project_id, task_id, team_run_id, agent_run_id,
                 role_slot_id, turn_ordinal, idempotency_key, task_revision, binding_generation,
                 authority_tier, artifacts, evidence_hash, settled_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'zz.checker', 1, 'sneak', 1, 1, 'operator', '[]',
                     '0000000000000000000000000000000000000000000000000000000000000000',
                     '2026-08-09T10:00:00Z')",
            rusqlite::params![
                kontor_core::id::RoleTurnId::generate().to_string(),
                project,
                fixture.fixture.task.to_string(),
                team,
                fixture.maker_run.to_string()
            ],
        )
        .expect_err("a waived slot cannot settle a turn");
    assert!(
        error.to_string().contains("cannot settle a turn"),
        "{error}"
    );
}

/// Journey 2, 9 and 10 at the store: the disposition closure re-proves itself
/// from the rows, the new source round-trips as its own variant, and a slot that
/// is neither settled nor waived withholds the task's closure.
#[test]
fn a_disposition_closure_reproves_itself_and_round_trips_as_its_own_source() {
    let fixture = waiver_fixture();
    let project = fixture.fixture.project;

    // The maker settles; the checker is waived.
    fixture
        .fixture
        .store
        .settle_role_turn(&kontor_store::NewRoleTurn {
            id: kontor_core::id::RoleTurnId::generate(),
            project_id: project,
            task_id: fixture.fixture.task,
            team_run_id: fixture.team_run,
            agent_run_id: fixture.maker_run,
            role_slot_id: kontor_core::id::RoleSlotId::parse("zz.maker").expect("a slot"),
            idempotency_key: "turn-maker".to_owned(),
            task_revision: AggregateRevision::INITIAL,
            binding_generation: 1,
            authority_tier: "operator",
            account_profile: Some(fixture.fixture.account),
            artifacts: [artifact("zz.output")].into_iter().collect(),
            evidence_hash: ContentHash::of(b"maker turn"),
            settled_at: now(),
        })
        .expect("the maker's turn settles");
    let (waiver, _, _) = fixture
        .fixture
        .store
        .waive_role_slot(&waiver_request(&fixture, "w-1"))
        .expect("the checker is waived");

    // The digest the closure must carry, computed the one canonical way.
    let snapshot = fixture
        .fixture
        .store
        .get_team_run(project, fixture.team_run)
        .expect("the team is readable")
        .expect("the team exists")
        .snapshot;
    let digest = kontor_core::state::role_slot_disposition_digest(
        SCHEMA_VERSION,
        fixture.team_run,
        snapshot.template_id,
        snapshot.template_version,
        snapshot.definition.hash(),
        &[
            (
                kontor_core::id::RoleSlotId::parse("zz.maker").expect("a slot"),
                kontor_core::state::SlotDisposition::SettledTurn {
                    evidence_hash: ContentHash::of(b"maker turn"),
                },
            ),
            (
                kontor_core::id::RoleSlotId::parse("zz.checker").expect("a slot"),
                kontor_core::state::SlotDisposition::WaivedUnbound {
                    evidence_hash: waiver.evidence_hash.clone(),
                },
            ),
        ],
    )
    .expect("the digest is computable");

    let launching = fixture
        .fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: project,
            team_run_id: fixture.team_run,
            expected_revision: AggregateRevision::INITIAL.next().expect("a revision"),
            to: RunLifecycle::Launching,
            occurred_at: now(),
        })
        .expect("the team launches");
    let running = fixture
        .fixture
        .store
        .advance_team_run(&TeamRunAdvance {
            project_id: project,
            team_run_id: fixture.team_run,
            expected_revision: launching,
            to: RunLifecycle::Running,
            occurred_at: now(),
        })
        .expect("the team runs");
    fixture
        .fixture
        .store
        .close_team_run(&TeamRunClosure {
            project_id: project,
            team_run_id: fixture.team_run,
            expected_revision: running,
            evidence: TeamTerminalEvidence {
                outcome: TerminalOutcome::Succeeded,
                source: TeamEvidenceSource::RoleSlotDispositions {
                    team_run_id: fixture.team_run,
                },
                evidence_hash: digest.clone(),
                closed_at: now(),
            },
        })
        .expect("a disposition closure is accepted");

    // Journey 9: the new source reloads as itself, not as a neighbour.
    let reloaded = fixture
        .fixture
        .store
        .get_team_run(project, fixture.team_run)
        .expect("the team is readable")
        .expect("the team exists");
    assert_eq!(
        reloaded
            .terminal
            .as_ref()
            .expect("the team is closed")
            .source,
        TeamEvidenceSource::RoleSlotDispositions {
            team_run_id: fixture.team_run
        },
        "a disposition closure must not decode as some other source"
    );

    // The task's profile must pin this team, or no closure is demanded and the
    // citation would be refused for an unrelated reason.
    let mut profile = work_profile();
    profile.id = kontor_core::id::WorkProfileKey::parse("zz.teamed").expect("a profile key");
    profile.team_template = Some(kontor_core::spec::TeamTemplateRef {
        template_id: fixture.template,
        version: SpecVersion::FIRST,
    });
    fixture
        .fixture
        .store
        .insert_work_profile(project, &profile)
        .expect("the profile is stored");
    fixture
        .fixture
        .store
        .create_task_workflow(&NewTaskWorkflow {
            id: TaskWorkflowId::generate(),
            project_id: project,
            task_id: fixture.fixture.task,
            snapshot: ResolvedWorkProfileSnapshot::resolve(&profile, now())
                .expect("the profile resolves"),
            current_phase: phase("zz.one"),
            created_at: now(),
        })
        .expect("the workflow is created");

    // The re-proof runs on a *terminal* transition, so the refusal is proved
    // first: it leaves the task alive for the acceptance below, and the two
    // cannot then be confused for one another.
    let closure = |digest: ContentHash, revision: AggregateRevision| TaskTransitionRequest {
        project_id: project,
        task_id: fixture.fixture.task,
        expected_revision: revision,
        to: TaskState::Failed,
        resume_receipt: None,
        reopen: false,
        run_outcome: Some(TerminalOutcome::Failed),
        produced_artifacts: Default::default(),
        completed_phases: Default::default(),
        team_closure: TaskTeamClosure::Certified {
            team_run_id: fixture.team_run,
            policy_digest: digest,
        },
        occurred_at: now(),
    };
    let started = fixture
        .fixture
        .store
        .transition_task(&TaskTransitionRequest {
            to: TaskState::InProgress,
            run_outcome: None,
            team_closure: TaskTeamClosure::NoTeam,
            ..closure(digest.clone(), AggregateRevision::INITIAL)
        })
        .expect("the task starts");

    // A cited digest that is not the one this team closed with is refused, which
    // is the check the store did not previously make at all.
    let refusal = fixture
        .fixture
        .store
        .transition_task(&closure(
            ContentHash::of(b"some other digest"),
            started.revision,
        ))
        .expect_err("a fabricated policy digest must be refused");
    assert!(
        refusal.to_string().contains("cited policy digest"),
        "{refusal:?}"
    );

    // The real one is accepted.
    fixture
        .fixture
        .store
        .transition_task(&closure(digest, started.revision))
        .expect("every declared slot is disposed of exactly once");
}

/// The exactly-one-source rule, on the state only corruption can produce.
///
/// The triggers make a slot that both settled a turn and was waived
/// unrepresentable through any supported path, which is exactly why the closure
/// re-proof must still refuse it: the rule has to be true of the *data*, not of
/// the code that happened to write it. Direct SQL, with the guard lifted.
#[test]
fn a_slot_that_both_settled_and_was_waived_is_refused_by_the_store() {
    let fixture = waiver_fixture();
    let project = fixture.fixture.project;
    fixture
        .fixture
        .store
        .waive_role_slot(&waiver_request(&fixture, "w-1"))
        .expect("the checker is waived");

    {
        let raw = rusqlite::Connection::open(&fixture.fixture.path).expect("the file reopens");
        raw.execute_batch(
            "DROP TRIGGER role_turns_refuse_waived_slot;
             DROP TRIGGER role_turns_are_permanent;",
        )
        .expect("the guards are lifted");
        for (id, slot, key) in [
            (
                "aaaaaaaa-0000-4000-8000-000000000001",
                "zz.maker",
                "t-maker",
            ),
            (
                "aaaaaaaa-0000-4000-8000-000000000002",
                "zz.checker",
                "t-checker",
            ),
        ] {
            raw.execute(
                "INSERT INTO role_turns (id, project_id, task_id, team_run_id, agent_run_id,
                     role_slot_id, turn_ordinal, idempotency_key, task_revision,
                     binding_generation, authority_tier, artifacts, evidence_hash, settled_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, 1, 1, 'operator', '[]',
                         '0000000000000000000000000000000000000000000000000000000000000000',
                         '2026-08-09T10:00:00Z')",
                rusqlite::params![
                    id,
                    project.to_string(),
                    fixture.fixture.task.to_string(),
                    fixture.team_run.to_string(),
                    fixture.maker_run.to_string(),
                    slot,
                    key
                ],
            )
            .expect("the row is written");
        }
    }

    // `zz.checker` now carries both sources. The re-proof must say so, by name,
    // rather than silently preferring one.
    let refusal = ensure_disposed_refusal(&fixture);
    assert!(
        refusal.contains("both settled a turn and was waived"),
        "the contradiction is named, not resolved: {refusal}"
    );
}

/// Drive the disposition re-proof and return the refusal it produces.
fn ensure_disposed_refusal(fixture: &WaiverFixture) -> String {
    let project = fixture.fixture.project;
    // Close the team on dispositions with whatever digest, then transition: the
    // digest check runs after the one-source walk, so the walk answers first.
    let raw = rusqlite::Connection::open(&fixture.fixture.path).expect("the file reopens");
    raw.execute(
        "UPDATE team_runs SET lifecycle = 'succeeded', terminal_outcome = 'succeeded',
             terminal_source_kind = 'role_slot_dispositions',
             terminal_evidence_hash =
                 '1111111111111111111111111111111111111111111111111111111111111111',
             closed_at = '2026-08-09T10:00:00Z'
         WHERE project_id = ?1 AND id = ?2",
        rusqlite::params![project.to_string(), fixture.team_run.to_string()],
    )
    .expect("the team is marked closed");
    drop(raw);

    let mut profile = work_profile();
    profile.id = kontor_core::id::WorkProfileKey::parse("zz.teamed2").expect("a profile key");
    profile.team_template = Some(kontor_core::spec::TeamTemplateRef {
        template_id: fixture.template,
        version: SpecVersion::FIRST,
    });
    fixture
        .fixture
        .store
        .insert_work_profile(project, &profile)
        .expect("the profile is stored");
    fixture
        .fixture
        .store
        .create_task_workflow(&NewTaskWorkflow {
            id: TaskWorkflowId::generate(),
            project_id: project,
            task_id: fixture.fixture.task,
            snapshot: ResolvedWorkProfileSnapshot::resolve(&profile, now())
                .expect("the profile resolves"),
            current_phase: phase("zz.one"),
            created_at: now(),
        })
        .expect("the workflow is created");
    let started = fixture
        .fixture
        .store
        .transition_task(&TaskTransitionRequest {
            project_id: project,
            task_id: fixture.fixture.task,
            expected_revision: AggregateRevision::INITIAL,
            to: TaskState::InProgress,
            resume_receipt: None,
            reopen: false,
            run_outcome: None,
            produced_artifacts: Default::default(),
            completed_phases: Default::default(),
            team_closure: TaskTeamClosure::NoTeam,
            occurred_at: now(),
        })
        .expect("the task starts");
    fixture
        .fixture
        .store
        .transition_task(&TaskTransitionRequest {
            project_id: project,
            task_id: fixture.fixture.task,
            expected_revision: started.revision,
            to: TaskState::Failed,
            resume_receipt: None,
            reopen: false,
            run_outcome: Some(TerminalOutcome::Failed),
            produced_artifacts: Default::default(),
            completed_phases: Default::default(),
            team_closure: TaskTeamClosure::Certified {
                team_run_id: fixture.team_run,
                policy_digest: ContentHash::of(b"whatever"),
            },
            occurred_at: now(),
        })
        .expect_err("the contradiction must be refused")
        .to_string()
}

// ---------------------------------------------------------------------------
// Epic completion (KON-OP-06)
// ---------------------------------------------------------------------------

/// A published Completion Profile revision is immutable and published once.
#[test]
fn a_published_completion_profile_revision_cannot_be_republished() {
    let fixture = fixture();
    let revision = |version: u32| StoredCompletionProfile {
        project_id: fixture.project,
        id: name("house-style"),
        version: SpecVersion::parse(version).expect("a version"),
        name: name("House style"),
        definition: serde_json::json!({"id": "house-style", "version": version}),
        definition_hash: ContentHash::of(format!("house-style-{version}").as_bytes()),
        published_at: now(),
    };

    fixture
        .store
        .publish_completion_profile(&revision(1))
        .expect("the first revision publishes");
    let conflict = fixture
        .store
        .publish_completion_profile(&revision(1))
        .expect_err("an immutable revision is published once");
    assert!(
        matches!(conflict, RepositoryError::Conflict { .. }),
        "expected a conflict, got {conflict:?}"
    );
    // A *next* version is an ordinary publication, not a replacement.
    fixture
        .store
        .publish_completion_profile(&revision(2))
        .expect("the next revision publishes");

    let published = fixture
        .store
        .list_completion_profiles(fixture.project)
        .expect("the catalog reads back");
    assert_eq!(published.len(), 2, "both revisions stand");
    assert_eq!(published[0].version.get(), 1, "oldest first");
    assert_eq!(published[1].version.get(), 2);
    // The other project shares neither row.
    assert!(
        fixture
            .store
            .list_completion_profiles(fixture.other_project)
            .expect("readable")
            .is_empty(),
        "a published profile belongs to exactly one project"
    );
}

/// A deterministic derived profile belongs to the project, not to one epic.
/// A second epic that resolves the same exact revision must reuse the immutable
/// row rather than treating its JSON definition as different content.
#[test]
fn two_epics_reuse_the_same_exact_derived_completion_profile() {
    let fixture = fixture();
    let first_epic = MiniProjectId::generate();
    let second_epic = MiniProjectId::generate();
    for (epic, title) in [
        (first_epic, "First derived-profile epic"),
        (second_epic, "Second derived-profile epic"),
    ] {
        fixture
            .store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: fixture.project,
                name: name(title),
                created_at: now(),
            })
            .expect("the epic exists");
    }

    let profile = StoredCompletionProfile {
        project_id: fixture.project,
        id: name("derived-operational-default"),
        version: SpecVersion::parse(1).expect("a version"),
        name: name("Derived operational default"),
        definition: serde_json::json!({
            "id": "derived-operational-default",
            "policy": {"required_gates": ["review", "qa", "release"]},
            "version": 1
        }),
        definition_hash: ContentHash::of(b"derived-operational-default@1"),
        published_at: now(),
    };
    let completion = |epic| StoredEpicCompletion {
        project_id: fixture.project,
        mini_project_id: epic,
        profile_id: profile.id.clone(),
        profile_version: profile.version,
        definition_hash: profile.definition_hash.clone(),
        state: serde_json::json!({"phase": "tickets"}),
        revision: AggregateRevision::INITIAL,
        updated_at: now(),
    };
    let command = |epic, key: &str| NewLocalCommand {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
        kind: CommandKind::AdvanceCompletion,
        target: AggregateRef::MiniProject {
            mini_project_id: epic,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "operation": "completion_advance",
            "epic": epic.to_string(),
        }))
        .expect("the intent canonicalizes"),
        created_at: now(),
    };

    fixture
        .store
        .commit_epic_completion_with_profile(
            &completion(first_epic),
            CompletionWrite::Create,
            Some(&profile),
            &[],
            &command(first_epic, "completion-derived-profile-first"),
        )
        .expect("the first epic publishes and pins the derived profile");
    fixture
        .store
        .commit_epic_completion_with_profile(
            &completion(second_epic),
            CompletionWrite::Create,
            Some(&profile),
            &[],
            &command(second_epic, "completion-derived-profile-second"),
        )
        .expect("the second epic reuses the identical derived profile");

    assert_eq!(
        fixture
            .store
            .list_completion_profiles(fixture.project)
            .expect("the profile catalog reads")
            .len(),
        1,
        "both epics share one immutable project profile revision"
    );
    assert!(
        fixture
            .store
            .get_epic_completion(fixture.project, first_epic)
            .expect("the first completion reads")
            .is_some()
    );
    assert!(
        fixture
            .store
            .get_epic_completion(fixture.project, second_epic)
            .expect("the second completion reads")
            .is_some()
    );
}

/// Two advances from one revision cannot both win.
///
/// The compare-and-swap is what makes the planned effects safe: if the second
/// write landed, it would overwrite the first transition *and* the effect
/// identities that were recorded with it, leaving dispatched work no state
/// claims.
#[test]
fn a_completion_transition_from_a_superseded_revision_changes_nothing() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    let completion = |revision: u64, phase: &str| StoredEpicCompletion {
        project_id: fixture.project,
        mini_project_id: epic,
        profile_id: name("operational_default"),
        profile_version: SpecVersion::parse(1).expect("a version"),
        definition_hash: ContentHash::of(b"operational_default@1"),
        state: serde_json::json!({"phase": phase}),
        revision: AggregateRevision::parse(revision).expect("a revision"),
        updated_at: now(),
    };

    fixture
        .store
        .create_epic_completion(&completion(1, "tickets"))
        .expect("the run starts");
    let twice = fixture
        .store
        .create_epic_completion(&completion(1, "tickets"))
        .expect_err("one epic has one completion run");
    assert!(
        matches!(twice, RepositoryError::Conflict { .. }),
        "expected a conflict, got {twice:?}"
    );

    let first = AggregateRevision::parse(1).expect("a revision");
    fixture
        .store
        .update_epic_completion(&completion(2, "integration"), first)
        .expect("the first transition commits");
    let lost = fixture
        .store
        .update_epic_completion(&completion(2, "closeout"), first)
        .expect_err("a superseded revision writes nothing");
    assert!(
        matches!(lost, RepositoryError::Conflict { .. }),
        "expected a conflict, got {lost:?}"
    );

    let stored = fixture
        .store
        .get_epic_completion(fixture.project, epic)
        .expect("readable")
        .expect("the run stands");
    assert_eq!(stored.revision.get(), 2);
    assert_eq!(
        stored.state["phase"], "integration",
        "the losing write must not have overwritten the winner"
    );
    // An epic in another project is absent, never this one's row.
    assert!(
        fixture
            .store
            .get_epic_completion(fixture.other_project, epic)
            .expect("readable")
            .is_none()
    );
}

/// A wake that cannot be stored takes its completion transition and receipt
/// down with it.
///
/// Completion state, every derived wake and the command receipt are one
/// durable effect. A foreign-project wake exercises a real database boundary:
/// the foreign key rejects it, and the transaction must leave the prior
/// completion revision and the command ledger untouched.
#[test]
fn a_completion_whose_wake_cannot_be_stored_commits_neither() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    let completion = |revision: u64, phase: &str| StoredEpicCompletion {
        project_id: fixture.project,
        mini_project_id: epic,
        profile_id: name("operational_default"),
        profile_version: SpecVersion::parse(1).expect("a version"),
        definition_hash: ContentHash::of(b"operational_default@1"),
        state: serde_json::json!({"phase": phase}),
        revision: AggregateRevision::parse(revision).expect("a revision"),
        updated_at: now(),
    };
    fixture
        .store
        .create_epic_completion(&completion(1, "tickets"))
        .expect("the run starts");

    let key = IdempotencyKey::parse("completion-atomic-wake").expect("a valid key");
    let command = NewLocalCommand {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: key.clone(),
        kind: CommandKind::AdvanceCompletion,
        target: AggregateRef::MiniProject {
            mini_project_id: epic,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "operation": "completion_advance",
            "epic": epic.to_string(),
        }))
        .expect("the intent canonicalizes"),
        created_at: now(),
    };
    let wake = StoredCompletionWake {
        project_id: ProjectId::generate(),
        mini_project_id: epic,
        completion_revision: AggregateRevision::parse(2).expect("a revision"),
        reason: name("completion_advanced"),
        seat_binding_id: SeatBindingId::generate(),
        receipt: ContentHash::of(b"operational_default@1"),
        appended_at: now(),
        acknowledged_at: None,
    };

    let first = AggregateRevision::parse(1).expect("a revision");
    fixture
        .store
        .commit_epic_completion(
            &completion(2, "integration"),
            CompletionWrite::Advance(first),
            &[wake],
            &command,
        )
        .expect_err("a wake that cannot be stored refuses the whole commit");

    let stored = fixture
        .store
        .get_epic_completion(fixture.project, epic)
        .expect("the run reads")
        .expect("the run is still there");
    assert_eq!(
        stored.revision, first,
        "the transition is unmade, not left standing without its wake"
    );
    assert!(
        fixture
            .store
            .get_receipt_by_key(&key)
            .expect("the receipt read succeeds")
            .is_none(),
        "no receipt may stand for a transition that did not commit"
    );
}

#[test]
fn a_completion_commit_refuses_command_scope_and_unchanged_state_mismatches() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    let other = MiniProjectId::generate();
    for (id, title) in [(epic, "Owned completion"), (other, "Foreign completion")] {
        fixture
            .store
            .create_mini_project(&NewMiniProject {
                id,
                project_id: fixture.project,
                name: name(title),
                created_at: now(),
            })
            .expect("the epic exists");
    }
    let completion = |phase: &str| StoredEpicCompletion {
        project_id: fixture.project,
        mini_project_id: epic,
        profile_id: name("operational_default"),
        profile_version: SpecVersion::parse(1).expect("a version"),
        definition_hash: ContentHash::of(b"operational_default@1"),
        state: serde_json::json!({"phase": phase}),
        revision: AggregateRevision::INITIAL,
        updated_at: now(),
    };
    fixture
        .store
        .create_epic_completion(&completion("tickets"))
        .expect("the run starts");

    let command = |target| NewLocalCommand {
        project_id: fixture.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse(&format!("completion-scope-{target:?}"))
            .expect("a key"),
        kind: CommandKind::AdvanceCompletion,
        target,
        target_revision: AggregateRevision::INITIAL,
        intent: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "operation": "completion_advance",
            "epic": epic.to_string(),
        }))
        .expect("intent"),
        created_at: now(),
    };

    assert!(matches!(
        fixture.store.commit_epic_completion(
            &completion("tickets"),
            CompletionWrite::Unchanged,
            &[],
            &command(AggregateRef::MiniProject {
                mini_project_id: other
            }),
        ),
        Err(RepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        fixture.store.commit_epic_completion(
            &completion("changed-without-write"),
            CompletionWrite::Unchanged,
            &[],
            &command(AggregateRef::MiniProject {
                mini_project_id: epic
            }),
        ),
        Err(RepositoryError::Conflict { .. })
    ));
    let foreign_wake = StoredCompletionWake {
        project_id: fixture.project,
        mini_project_id: other,
        completion_revision: AggregateRevision::INITIAL,
        reason: name("completion_advanced"),
        seat_binding_id: SeatBindingId::generate(),
        receipt: completion("tickets").definition_hash,
        appended_at: now(),
        acknowledged_at: None,
    };
    assert!(matches!(
        fixture.store.commit_epic_completion(
            &completion("tickets"),
            CompletionWrite::Unchanged,
            &[foreign_wake],
            &command(AggregateRef::MiniProject {
                mini_project_id: epic
            }),
        ),
        Err(RepositoryError::Conflict { .. })
    ));
}

/// One observation wakes the TPM once, however many times it is delivered.
#[test]
fn a_replayed_completion_wake_reuses_its_intent_instead_of_adding_a_second() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    let seat = SeatBindingId::generate();
    let wake = |revision: u64, reason: &str, seat: SeatBindingId| StoredCompletionWake {
        project_id: fixture.project,
        mini_project_id: epic,
        completion_revision: AggregateRevision::parse(revision).expect("a revision"),
        reason: name(reason),
        seat_binding_id: seat,
        receipt: ContentHash::of(b"wake"),
        appended_at: now(),
        acknowledged_at: None,
    };

    assert!(
        fixture
            .store
            .append_completion_wake(&wake(1, "completion_advanced", seat))
            .expect("the intent appends"),
        "the first delivery appends"
    );
    assert!(
        !fixture
            .store
            .append_completion_wake(&wake(1, "completion_advanced", seat))
            .expect("the replay is not an error"),
        "a duplicate observation must not append a second turn"
    );
    // A different revision, reason or seat is a different observation.
    assert!(
        fixture
            .store
            .append_completion_wake(&wake(2, "completion_advanced", seat))
            .expect("appends"),
    );
    assert!(
        fixture
            .store
            .append_completion_wake(&wake(1, "remediation_routed", seat))
            .expect("appends"),
    );

    fixture
        .store
        .acknowledge_completion_wake(&wake(1, "completion_advanced", seat), now())
        .expect("the runtime took the turn");
    let wakes = fixture
        .store
        .list_completion_wakes(fixture.project, epic)
        .expect("readable");
    assert_eq!(wakes.len(), 3, "three distinct observations, three intents");
    let acknowledged: Vec<_> = wakes
        .iter()
        .filter(|wake| wake.acknowledged_at.is_some())
        .collect();
    assert_eq!(
        acknowledged.len(),
        1,
        "only the acknowledged intent is settled"
    );
    assert_eq!(acknowledged[0].completion_revision.get(), 1);
    assert_eq!(acknowledged[0].reason.as_str(), "completion_advanced");
}

/// One failed round holds one bounded proposal.
#[test]
fn a_second_remediation_proposal_for_one_round_is_refused() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    fixture
        .store
        .create_mini_project(&NewMiniProject {
            id: epic,
            project_id: fixture.project,
            name: name("Remediation epic"),
            created_at: now(),
        })
        .expect("the epic exists");
    let lsa_seat = SeatBindingId::generate();
    let proposal = |correction: &str| StoredRemediationProposal {
        project_id: fixture.project,
        mini_project_id: epic,
        completion_generation: 1,
        round: 1,
        failed_round_evidence: ContentHash::of(b"round-1-findings"),
        proposal: ContentHash::of(correction.as_bytes()),
        lsa_seat_binding_id: lsa_seat,
        lsa_occupancy_generation: 3,
        proposed_at: now(),
    };
    let command = |key: &str, correction: &str| {
        let intent = CanonicalDocument::from_serializable(&serde_json::json!({
            "schema_version": 1,
            "operation": "completion_remediate_propose",
            "failed_round_evidence": ContentHash::of(b"round-1-findings").as_str(),
            "proposal": ContentHash::of(correction.as_bytes()).as_str(),
        }))
        .expect("the proposal intent canonicalizes");
        ReceiptEnvelope::new(
            fixture.store.realm(),
            NewLocalCommand {
                project_id: fixture.project,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
                kind: CommandKind::RemediateCompletion,
                target: AggregateRef::MiniProject {
                    mini_project_id: epic,
                },
                target_revision: AggregateRevision::INITIAL,
                intent,
                created_at: now(),
            },
        )
    };

    let first = fixture
        .store
        .commit_remediation_proposal(
            &command("proposal-one", "narrow the change"),
            &proposal("narrow the change"),
            AggregateRevision::INITIAL,
            true,
        )
        .expect("the proposal records");
    assert_eq!(first.1, Applied::Created);
    let replay = fixture
        .store
        .commit_remediation_proposal(
            &command("proposal-one", "narrow the change"),
            &proposal("narrow the change"),
            AggregateRevision::INITIAL,
            false,
        )
        .expect("the exact command replays");
    assert_eq!(replay.0.id, first.0.id);
    assert_eq!(replay.1, Applied::Unchanged);
    let second = fixture
        .store
        .commit_remediation_proposal(
            &command("proposal-two", "something else entirely"),
            &proposal("something else entirely"),
            AggregateRevision::INITIAL,
            true,
        )
        .expect_err("one round has one proposal");
    assert!(
        matches!(second, RepositoryError::Conflict { .. }),
        "expected a conflict, got {second:?}"
    );

    let stored = fixture
        .store
        .get_remediation_proposal(fixture.project, epic, 1, 1)
        .expect("readable")
        .expect("the proposal stands");
    assert_eq!(
        stored.proposal,
        ContentHash::of(b"narrow the change"),
        "the first proposal is the one a route will read"
    );
    assert_eq!(stored.lsa_occupancy_generation, 3);
    assert!(
        fixture
            .store
            .get_remediation_proposal(fixture.project, epic, 1, 2)
            .expect("readable")
            .is_none(),
        "an unproposed round has nothing to route"
    );
}

/// Recovery may materialize a missing receipt only when an immutable claim
/// proves the exact key and intent and the proposal effect already matches.
#[test]
fn an_exact_orphaned_proposal_claim_recovers_once_and_rejects_impostors() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    fixture
        .store
        .create_mini_project(&NewMiniProject {
            id: epic,
            project_id: fixture.project,
            name: name("Proposal recovery epic"),
            created_at: now(),
        })
        .expect("the epic exists");
    let seat = SeatBindingId::generate();
    let proposal = StoredRemediationProposal {
        project_id: fixture.project,
        mini_project_id: epic,
        completion_generation: 1,
        round: 1,
        failed_round_evidence: ContentHash::of(b"failed-round"),
        proposal: ContentHash::of(b"bounded correction"),
        lsa_seat_binding_id: seat,
        lsa_occupancy_generation: 2,
        proposed_at: now(),
    };
    let intent = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "operation": "completion_remediate_propose",
        "failed_round_evidence": proposal.failed_round_evidence.as_str(),
        "proposal": proposal.proposal.as_str(),
        "actor": seat,
        "generation": 2,
    }))
    .expect("the intent canonicalizes");
    let key = IdempotencyKey::parse("proposal-fault-boundary").expect("a valid key");
    let command = |key: IdempotencyKey, intent: CanonicalDocument| {
        ReceiptEnvelope::new(
            fixture.store.realm(),
            NewLocalCommand {
                project_id: fixture.project,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key,
                kind: CommandKind::RemediateCompletion,
                target: AggregateRef::MiniProject {
                    mini_project_id: epic,
                },
                target_revision: AggregateRevision::INITIAL,
                intent,
                created_at: now(),
            },
        )
    };

    // Reproduce the old boundary: replay authority and exact effect committed,
    // but the local receipt did not. The production path now writes all three
    // rows together; this direct seed exists only to prove restart recovery.
    let connection = Connection::open(&fixture.path).expect("the database reopens");
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_command_claims
                 (project_id, mini_project_id, round, action, idempotency_key,
                  intent_hash, effect_revision, claimed_at)
             VALUES (?1, ?2, 1, 'lsa_proposal', ?3, ?4, 1, ?5)",
            rusqlite::params![
                fixture.project.to_string(),
                epic.to_string(),
                key.as_str(),
                intent.hash().as_str(),
                "2026-08-26T09:00:00Z",
            ],
        )
        .expect("the exact replay claim is seeded");
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, round, failed_round_evidence, proposal,
                  lsa_seat_binding_id, lsa_occupancy_generation, proposed_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, 2, ?6)",
            rusqlite::params![
                fixture.project.to_string(),
                epic.to_string(),
                proposal.failed_round_evidence.as_str(),
                proposal.proposal.as_str(),
                seat.to_string(),
                "2026-08-26T09:00:00Z",
            ],
        )
        .expect("the exact proposal effect is seeded");
    drop(connection);

    let recovered = fixture
        .store
        .commit_remediation_proposal(
            &command(key.clone(), intent.clone()),
            &proposal,
            AggregateRevision::INITIAL,
            false,
        )
        .expect("the exact retry materializes its receipt");
    assert_eq!(recovered.1, Applied::Unchanged);
    let replayed = fixture
        .store
        .commit_remediation_proposal(
            &command(key.clone(), intent.clone()),
            &proposal,
            AggregateRevision::INITIAL,
            false,
        )
        .expect("the recovered command replays");
    assert_eq!(replayed.0.id, recovered.0.id);
    assert_eq!(replayed.1, Applied::Unchanged);

    let changed_intent = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "operation": "completion_remediate_propose",
        "failed_round_evidence": ContentHash::of(b"different evidence").as_str(),
        "proposal": proposal.proposal.as_str(),
        "actor": seat,
        "generation": 2,
    }))
    .expect("the changed intent canonicalizes");
    assert!(matches!(
        fixture.store.commit_remediation_proposal(
            &command(key, changed_intent),
            &proposal,
            AggregateRevision::INITIAL,
            false,
        ),
        Err(RepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        fixture.store.commit_remediation_proposal(
            &command(
                IdempotencyKey::parse("proposal-impostor-key").expect("a valid key"),
                intent,
            ),
            &proposal,
            AggregateRevision::INITIAL,
            true,
        ),
        Err(RepositoryError::Conflict { .. })
    ));
}

/// A route whose state effect survived but whose receipt did not is recovered
/// only through its exact immutable claim; wake and receipt are repaired in the
/// same transaction without applying the completion transition twice.
#[test]
fn an_exact_orphaned_route_effect_recovers_receipt_and_wake_once() {
    let fixture = fixture();
    let epic = MiniProjectId::generate();
    fixture
        .store
        .create_mini_project(&NewMiniProject {
            id: epic,
            project_id: fixture.project,
            name: name("Route recovery epic"),
            created_at: now(),
        })
        .expect("the epic exists");
    let completion = |revision: u64, phase: &str| StoredEpicCompletion {
        project_id: fixture.project,
        mini_project_id: epic,
        profile_id: name("operational_default"),
        profile_version: SpecVersion::parse(1).expect("a version"),
        definition_hash: ContentHash::of(b"operational_default@1"),
        state: serde_json::json!({"phase": phase, "revision": revision}),
        revision: AggregateRevision::parse(revision).expect("a revision"),
        updated_at: now(),
    };
    fixture
        .store
        .create_epic_completion(&completion(1, "await_remediation"))
        .expect("the completion starts");
    let next = completion(2, "remediating");
    let intent = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "operation": "completion_remediate_route",
        "round": 1,
        "route": ContentHash::of(b"route").as_str(),
    }))
    .expect("the route intent canonicalizes");
    let key = IdempotencyKey::parse("route-fault-boundary").expect("a valid key");
    let command = |key: IdempotencyKey, intent: CanonicalDocument| {
        ReceiptEnvelope::new(
            fixture.store.realm(),
            NewLocalCommand {
                project_id: fixture.project,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key,
                kind: CommandKind::RemediateCompletion,
                target: AggregateRef::MiniProject {
                    mini_project_id: epic,
                },
                target_revision: AggregateRevision::INITIAL,
                intent,
                created_at: now(),
            },
        )
    };
    let connection = Connection::open(&fixture.path).expect("the database reopens");
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_command_claims
                 (project_id, mini_project_id, round, action, idempotency_key,
                  intent_hash, effect_revision, claimed_at)
             VALUES (?1, ?2, 1, 'tpm_route', ?3, ?4, 2, ?5)",
            rusqlite::params![
                fixture.project.to_string(),
                epic.to_string(),
                key.as_str(),
                intent.hash().as_str(),
                "2026-08-26T09:05:00Z",
            ],
        )
        .expect("the exact route claim is seeded");
    connection
        .execute(
            "UPDATE epic_completion SET state = ?3, revision = 2, updated_at = ?4
              WHERE project_id = ?1 AND mini_project_id = ?2 AND revision = 1",
            rusqlite::params![
                fixture.project.to_string(),
                epic.to_string(),
                next.state.to_string(),
                "2026-08-26T09:05:00Z",
            ],
        )
        .expect("the route effect is seeded");
    drop(connection);
    let wake = StoredCompletionWake {
        project_id: fixture.project,
        mini_project_id: epic,
        completion_revision: AggregateRevision::parse(2).expect("a revision"),
        reason: name("remediation_routed"),
        seat_binding_id: SeatBindingId::generate(),
        receipt: next.definition_hash.clone(),
        appended_at: now(),
        acknowledged_at: None,
    };

    let recovered = fixture
        .store
        .commit_remediation_route(
            &command(key.clone(), intent.clone()),
            1,
            1,
            &next,
            AggregateRevision::INITIAL,
            AggregateRevision::parse(2).expect("a revision"),
            &wake,
            true,
        )
        .expect("the exact route retry recovers");
    assert_eq!(recovered.0.state, next.state);
    assert_eq!(recovered.0.revision, next.revision);
    assert_eq!(recovered.2, Applied::Unchanged);
    let replayed = fixture
        .store
        .commit_remediation_route(
            &command(key.clone(), intent.clone()),
            1,
            1,
            &next,
            AggregateRevision::INITIAL,
            AggregateRevision::parse(2).expect("a revision"),
            &wake,
            true,
        )
        .expect("the recovered route replays");
    assert_eq!(replayed.1.id, recovered.1.id);
    assert_eq!(replayed.2, Applied::Unchanged);
    assert_eq!(
        fixture
            .store
            .list_completion_wakes(fixture.project, epic)
            .expect("the wake reads")
            .len(),
        1,
        "recovery and replay share one wake"
    );

    let different = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "operation": "completion_remediate_route",
        "round": 1,
        "route": ContentHash::of(b"different route").as_str(),
    }))
    .expect("the changed route canonicalizes");
    assert!(matches!(
        fixture.store.commit_remediation_route(
            &command(key, different),
            1,
            1,
            &next,
            AggregateRevision::INITIAL,
            AggregateRevision::parse(2).expect("a revision"),
            &wake,
            true,
        ),
        Err(RepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        fixture.store.commit_remediation_route(
            &command(
                IdempotencyKey::parse("route-impostor-key").expect("a valid key"),
                intent,
            ),
            1,
            1,
            &next,
            AggregateRevision::INITIAL,
            AggregateRevision::parse(2).expect("a revision"),
            &wake,
            true,
        ),
        Err(RepositoryError::Conflict { .. })
    ));
}

/// A durable effect is not replay authority by itself. Without the immutable
/// command claim written by the atomic path, recovery fails closed and rolls
/// back the tentative receipt, claim and wake rows.
#[test]
fn orphaned_remediation_effects_without_claims_cannot_be_adopted() {
    let fixture = fixture();
    let proposal_epic = MiniProjectId::generate();
    fixture
        .store
        .create_mini_project(&NewMiniProject {
            id: proposal_epic,
            project_id: fixture.project,
            name: name("Unclaimed proposal epic"),
            created_at: now(),
        })
        .expect("the proposal epic exists");
    let proposal = StoredRemediationProposal {
        project_id: fixture.project,
        mini_project_id: proposal_epic,
        completion_generation: 1,
        round: 1,
        failed_round_evidence: ContentHash::of(b"failed-round"),
        proposal: ContentHash::of(b"unclaimed correction"),
        lsa_seat_binding_id: SeatBindingId::generate(),
        lsa_occupancy_generation: 4,
        proposed_at: now(),
    };
    let proposal_intent = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "operation": "completion_remediate_propose",
        "failed_round_evidence": proposal.failed_round_evidence.as_str(),
        "proposal": proposal.proposal.as_str(),
        "actor": proposal.lsa_seat_binding_id,
        "generation": 4,
    }))
    .expect("the proposal intent canonicalizes");
    let proposal_key = IdempotencyKey::parse("unclaimed-proposal-effect").expect("a valid key");
    let proposal_command = ReceiptEnvelope::new(
        fixture.store.realm(),
        NewLocalCommand {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: proposal_key.clone(),
            kind: CommandKind::RemediateCompletion,
            target: AggregateRef::MiniProject {
                mini_project_id: proposal_epic,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: proposal_intent,
            created_at: now(),
        },
    );
    let connection = Connection::open(&fixture.path).expect("the database reopens");
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, round, failed_round_evidence, proposal,
                  lsa_seat_binding_id, lsa_occupancy_generation, proposed_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, 4, ?6)",
            rusqlite::params![
                fixture.project.to_string(),
                proposal_epic.to_string(),
                proposal.failed_round_evidence.as_str(),
                proposal.proposal.as_str(),
                proposal.lsa_seat_binding_id.to_string(),
                "2026-08-26T09:10:00Z",
            ],
        )
        .expect("the unclaimed proposal effect is seeded");
    drop(connection);
    assert!(matches!(
        fixture.store.commit_remediation_proposal(
            &proposal_command,
            &proposal,
            AggregateRevision::INITIAL,
            false,
        ),
        Err(RepositoryError::Conflict { .. })
    ));

    let route_epic = MiniProjectId::generate();
    fixture
        .store
        .create_mini_project(&NewMiniProject {
            id: route_epic,
            project_id: fixture.project,
            name: name("Unclaimed route epic"),
            created_at: now(),
        })
        .expect("the route epic exists");
    let routed = StoredEpicCompletion {
        project_id: fixture.project,
        mini_project_id: route_epic,
        profile_id: name("operational_default"),
        profile_version: SpecVersion::parse(1).expect("a version"),
        definition_hash: ContentHash::of(b"operational_default@1"),
        state: serde_json::json!({"phase": "remediating", "revision": 2}),
        revision: AggregateRevision::parse(2).expect("a revision"),
        updated_at: now(),
    };
    fixture
        .store
        .create_epic_completion(&routed)
        .expect("the unclaimed route effect is seeded");
    let route_intent = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "operation": "completion_remediate_route",
        "round": 1,
        "route": ContentHash::of(b"unclaimed route").as_str(),
    }))
    .expect("the route intent canonicalizes");
    let route_key = IdempotencyKey::parse("unclaimed-route-effect").expect("a valid key");
    let route_command = ReceiptEnvelope::new(
        fixture.store.realm(),
        NewLocalCommand {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: route_key.clone(),
            kind: CommandKind::RemediateCompletion,
            target: AggregateRef::MiniProject {
                mini_project_id: route_epic,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: route_intent,
            created_at: now(),
        },
    );
    let wake = StoredCompletionWake {
        project_id: fixture.project,
        mini_project_id: route_epic,
        completion_revision: routed.revision,
        reason: name("remediation_routed"),
        seat_binding_id: SeatBindingId::generate(),
        receipt: routed.definition_hash.clone(),
        appended_at: now(),
        acknowledged_at: None,
    };
    assert!(matches!(
        fixture.store.commit_remediation_route(
            &route_command,
            1,
            1,
            &routed,
            AggregateRevision::INITIAL,
            routed.revision,
            &wake,
            true,
        ),
        Err(RepositoryError::Conflict { .. })
    ));

    let connection = Connection::open(&fixture.path).expect("the database reopens");
    for key in [&proposal_key, &route_key] {
        let receipts: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM command_receipts WHERE idempotency_key = ?1",
                [key.as_str()],
                |row| row.get(0),
            )
            .expect("receipt count reads");
        let claims: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM epic_completion_remediation_command_claims
                  WHERE idempotency_key = ?1",
                [key.as_str()],
                |row| row.get(0),
            )
            .expect("claim count reads");
        assert_eq!((receipts, claims), (0, 0));
    }
    assert!(
        fixture
            .store
            .list_completion_wakes(fixture.project, route_epic)
            .expect("the wakes read")
            .is_empty(),
        "the rejected recovery cannot leave a wake"
    );
}
