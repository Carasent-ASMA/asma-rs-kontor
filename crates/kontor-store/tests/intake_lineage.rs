//! Durable intake against a real file-backed database: two commits, one graph.
//!
//! `kontor-intake` proves the decision is deterministic. This suite proves what
//! the *database* does with it — which is where every property that survives a
//! crash or a race actually lives. Every test opens a file-backed store, because
//! the concurrency authority under test is SQLite's own uniqueness enforcement
//! and an `:memory:` database would prove nothing about it.
//!
//! The mutants this suite exists to kill:
//!
//! * an intake that writes the event and the decision in one transaction, so a
//!   crash between ingestion and evaluation loses the event a source system
//!   already handed over;
//! * a replay that answers from the old decision after upstream changed the
//!   bytes under an id it had already used;
//! * two concurrent deliveries of one event producing two events, two decisions
//!   or — worst — two work graphs;
//! * an approval that creates work without lineage, or lineage without work;
//! * a rejection that is not terminal, or that creates work anyway;
//! * a bounded auto-arm that the store admits without re-checking the bounds,
//!   so a caller who skips `kontor-intake` skips the policy;
//! * a second decision attached to a proposal that already has one.

use std::collections::BTreeMap;

use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash,
    CurrencyCode, ExecutionAuthorizationId, ExternalName, IdempotencyKey, IntakeDecisionId,
    IntakeReceiptId, MiniProjectId, Money, ProjectId, SourceEventId, SpecVersion, TaskId,
    Timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CalendarRepository, CommandRepository, CredentialReference, CredentialReferenceKind,
    IntakeAuthority, IntakeDecisionOutcome, IntakeOutcome, IntakeRepository, IntakeWorkPlan,
    NewAccountProfile, NewCommandIntent, NewIntakeDecision, NewIntakeDecisionRecord,
    NewIntakeReevaluation, NewMiniProject, NewProject, NewSourceEvent, NewTask, NewTaskWorkflow,
    ProjectRepository, SourceEventIngest, SpecRepository, WorkflowRepository,
};
use kontor_core::spec::{
    AutoArmPolicy, BudgetBounds, CanonicalSourceEvent, ExecutionCapability, IntakeReceipt,
    IntakeResult, ResolvedWorkProfileSnapshot, SourceIdentity, SourceProcessingState,
    TeamTemplateRevision, TriggerSpec, WorkProfileSpec,
};
use kontor_core::state::TaskState;
use kontor_scheduler::model::TaskOrigin;
use kontor_store::SqliteStore;
use tempfile::TempDir;

const TRIGGER_FIXTURE: &str = include_str!("../../kontor-core/tests/fixtures/trigger.json");
const PROFILE_FIXTURE: &str = include_str!("../../kontor-profiles/fixtures/mvp-profile-pack.json");

const DECIDED_AT: &str = "2026-08-12T09:00:00Z";

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC fixture timestamp")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a legal external name")
}

fn key(text: &str) -> IdempotencyKey {
    IdempotencyKey::parse(text).expect("a legal idempotency key")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

fn budget() -> BudgetBounds {
    BudgetBounds {
        max_tokens: 100_000,
        max_commands: 40,
        max_duration_seconds: 1_800,
        max_cost: Money {
            minor_units: 1_500,
            currency: CurrencyCode::parse("NOK").expect("a legal currency"),
        },
    }
}

struct Fixture {
    directory: TempDir,
    store: SqliteStore,
    project: ProjectId,
    account: AccountProfileId,
    trigger: TriggerSpec,
}

impl Fixture {
    fn open() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let store =
            SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");
        let project = ProjectId::generate();
        store
            .create_project(&NewProject {
                id: project,
                name: name("Intake"),
                root_path: name("/tmp/intake"),
                created_at: at(DECIDED_AT),
            })
            .expect("the project is created");
        let account = AccountProfileId::generate();
        store
            .create_account_profile(&NewAccountProfile {
                id: account,
                project_id: project,
                label: name("Operator"),
                external_account_id: None,
                harness: kontor_core::id::RuntimeKindKey::parse("sa.runtime")
                    .expect("a legal runtime key"),
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: kontor_core::id::CredentialAlias::parse("sa-alpha")
                        .expect("a legal alias"),
                },
                environment: document("environment"),
                routing: document("routing"),
                capability: document("capability"),
                provider_identity: None,
                enabled: true,
                created_at: at(DECIDED_AT),
            })
            .expect("the account profile is created");

        let fixture = Self {
            directory,
            store,
            project,
            account,
            trigger: serde_json::from_str(TRIGGER_FIXTURE).expect("the trigger fixture parses"),
        };
        fixture.store_trigger(&fixture.trigger.clone());
        fixture
    }

    /// A trigger's pins are foreign keys, so what it names has to exist first.
    fn store_trigger(&self, spec: &TriggerSpec) {
        let pack: serde_json::Value =
            serde_json::from_str(PROFILE_FIXTURE).expect("the profile pack parses");
        let mut profile: WorkProfileSpec = serde_json::from_value(
            pack.pointer("/profiles/0")
                .expect("the pack ships a work profile")
                .clone(),
        )
        .expect("the work profile parses");
        profile.id = spec.work_profile.clone();
        profile.version = spec.work_profile_version;
        let _ = self.store.insert_work_profile(self.project, &profile);
        let _ = self.store.insert_team_template(
            self.project,
            &TeamTemplateRevision {
                template_id: spec.team_template.template_id,
                version: spec.team_template.version,
                name: name("Intake team"),
                definition: document("team"),
                role_authority: Vec::new(),
            },
        );
        self.store
            .insert_trigger_spec(self.project, spec)
            .expect("the trigger revision is stored");
    }

    /// Give one task an active workflow, so it is assembled as a candidate.
    ///
    /// The profile is the one the trigger already pins, read back from the
    /// store: a candidate assembled against a profile the trigger does not name
    /// would prove nothing about intake's own lineage.
    fn with_workflow(&self, task: TaskId) {
        let profile = self
            .store
            .get_work_profile(
                self.project,
                &self.trigger.work_profile,
                self.trigger.work_profile_version,
            )
            .expect("the read succeeds")
            .expect("the trigger's pinned profile is stored");
        let first_phase = profile.phases[0].id.clone();
        let snapshot = ResolvedWorkProfileSnapshot::resolve(&profile, at(DECIDED_AT))
            .expect("the profile resolves");
        self.store
            .create_task_workflow(&NewTaskWorkflow {
                id: kontor_core::id::TaskWorkflowId::generate(),
                project_id: self.project,
                task_id: task,
                snapshot,
                current_phase: first_phase,
                created_at: at(DECIDED_AT),
            })
            .expect("the workflow is created");
    }

    /// A healthy, fully reconciled runtime, so nothing in the assembly is
    /// refused for a reason this suite is not about.
    fn runtime(&self) -> kontor_scheduler::model::RuntimeAdmissionEvidence {
        use kontor_runtime::capability::{
            RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
        };
        use kontor_scheduler::model::{ReconciliationEvidence, ReconciliationScope, RuntimeHealth};
        let runtime_kind =
            kontor_core::id::RuntimeKindKey::parse("sa.runtime").expect("a legal runtime key");
        kontor_scheduler::model::RuntimeAdmissionEvidence {
            runtime_kind: runtime_kind.clone(),
            host: name("intake-host"),
            generation: 1,
            capabilities: RuntimeCapabilities {
                trust_grade: TrustGrade::A,
                supported: RuntimeCapability::ALL.iter().copied().collect(),
                account_env: true,
                limits: RuntimeLimits {
                    max_message_bytes: 4_096,
                    max_history_page: 64,
                    max_concurrent_sessions: 8,
                },
            },
            required: std::collections::BTreeSet::new(),
            health: RuntimeHealth::Healthy,
            reconciliation: ReconciliationEvidence {
                epoch_completed: true,
                scope: ReconciliationScope {
                    project_id: self.project,
                    runtime_kind,
                    host: name("intake-host"),
                    generation: 1,
                },
                open_replay_gap: false,
                divergence: false,
                orphan_ambiguity: false,
                stale_lost_contact: false,
            },
            last_confirmed_at: Some(at(DECIDED_AT)),
        }
    }

    fn path(&self) -> std::path::PathBuf {
        self.directory.path().join("kontor.db")
    }

    fn census(&self) -> BTreeMap<&'static str, i64> {
        let connection = rusqlite::Connection::open(self.path()).expect("a census connection");
        [
            "source_events",
            "intake_receipts",
            "intake_decisions",
            "intake_created_work",
            "tasks",
            "mini_projects",
        ]
        .into_iter()
        .map(|table| {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("the count succeeds");
            (table, count)
        })
        .collect()
    }

    /// A command receipt that authorizes deciding an intake proposal.
    fn approval_receipt(&self, label: &str) -> CommandReceiptId {
        let receipt = CommandReceiptId::generate();
        self.store
            .record_intent(&NewCommandIntent {
                project_id: self.project,
                receipt_id: receipt,
                idempotency_key: key(label),
                kind: CommandKind::ApproveIntake,
                target: AggregateRef::Project {
                    project_id: self.project,
                },
                target_revision: AggregateRevision::INITIAL,
                intent: document(label),
                payload: document(label),
                desired: None,
                not_before: at(DECIDED_AT),
                created_at: at(DECIDED_AT),
            })
            .expect("the decision receipt is recorded");
        receipt
    }

    /// An execution authorization over the whole project, and the receipt that
    /// granted it.
    fn authorization(&self, label: &str) -> (ExecutionAuthorizationId, CommandReceiptId) {
        let capability_receipt = CommandReceiptId::generate();
        self.store
            .record_intent(&NewCommandIntent {
                project_id: self.project,
                receipt_id: capability_receipt,
                idempotency_key: key(label),
                kind: CommandKind::AuthorizeExecution,
                target: AggregateRef::Project {
                    project_id: self.project,
                },
                target_revision: AggregateRevision::INITIAL,
                intent: document(label),
                payload: document(label),
                desired: None,
                not_before: at(DECIDED_AT),
                created_at: at(DECIDED_AT),
            })
            .expect("the capability receipt is recorded");
        let id = ExecutionAuthorizationId::generate();
        self.store
            .insert_authorization(&ExecutionAuthorization {
                id,
                project_id: self.project,
                scope: WorkScope::Project,
                selected_tasks: Vec::new(),
                allowed_start: TimeRange {
                    start: at("2026-08-12T00:00:00Z"),
                    end: at("2026-08-13T00:00:00Z"),
                },
                max_concurrency: 4,
                budget: budget(),
                created_by: self.account,
                capability_receipt,
                created_at: at(DECIDED_AT),
            })
            .expect("the authorization is stored");
        (id, capability_receipt)
    }
}

/// One canonical event carrying the two pointers the shipped trigger uses.
fn event(external_event_id: &str, marker: &str) -> CanonicalSourceEvent {
    CanonicalSourceEvent {
        id: SourceEventId::generate(),
        identity: SourceIdentity {
            source_kind: kontor_core::id::SourceKindKey::parse("webhook")
                .expect("a legal source kind"),
            source_connection: kontor_core::id::SourceConnectionKey::parse("conn.alpha")
                .expect("a legal connection"),
            external_event_id: kontor_core::id::ExternalId::parse(external_event_id)
                .expect("a legal external id"),
        },
        envelope: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "kind": "request.created",
            "external_id": marker,
        }))
        .expect("a canonical envelope"),
        external_observed_at: at(DECIDED_AT),
        ingested_at: at(DECIDED_AT),
        processing_state: SourceProcessingState::Received,
    }
}

fn proposal(trigger: &TriggerSpec, event: &CanonicalSourceEvent, label: &str) -> IntakeReceipt {
    IntakeReceipt {
        id: IntakeReceiptId::generate(),
        source_event_id: event.id,
        source_event_hash: event.envelope.hash().clone(),
        trigger: trigger.id.clone(),
        trigger_version: trigger.version,
        result: IntakeResult::Proposed,
        approval: None,
        proposed: None,
        idempotency_key: key(label),
        dedup_key: trigger
            .dedup
            .evaluate(&event.envelope)
            .expect("the trigger's dedup expression resolves"),
        duplicate_of: None,
        predecessor_receipt_id: None,
        decided_at: at(DECIDED_AT),
    }
}

fn work(project: ProjectId, goal: Option<MiniProjectId>, tasks: &[TaskId]) -> IntakeWorkPlan {
    IntakeWorkPlan {
        mini_project: goal.map(|id| NewMiniProject {
            id,
            project_id: project,
            name: name("Intake goal"),
            created_at: at(DECIDED_AT),
        }),
        tasks: tasks
            .iter()
            .map(|id| NewTask {
                id: *id,
                project_id: project,
                mini_project_id: goal,
                title: name("Work intake created"),
                module: None,
                state: TaskState::Ready,
                created_at: at(DECIDED_AT),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Ingestion is durable before anything evaluates it
// ---------------------------------------------------------------------------

#[test]
fn an_event_is_durable_before_a_decision_exists() {
    let fixture = Fixture::open();
    let incoming = event("ext-1", "payload");

    let ingested = fixture
        .store
        .ingest_source_event(fixture.project, &incoming)
        .expect("the event is committed");
    assert!(matches!(ingested, SourceEventIngest::Recorded(_)));
    let census = fixture.census();
    assert_eq!(census.get("source_events"), Some(&1));
    assert_eq!(
        census.get("intake_receipts"),
        Some(&0),
        "nothing has decided about it yet"
    );

    // This is the state a crash between the two commits leaves behind, and it
    // is resumable rather than lost or duplicated.
    assert!(
        matches!(
            fixture
                .store
                .ingest_source_event(fixture.project, &incoming)
                .expect("re-ingesting is not an error"),
            SourceEventIngest::Unevaluated(_)
        ),
        "an undecided stored event is resumed, not treated as a duplicate"
    );

    let receipt = proposal(&fixture.trigger, &incoming, "intake-resumed");
    let outcome = fixture
        .store
        .record_intake_decision(&NewIntakeDecision {
            project_id: fixture.project,
            source_event_id: incoming.id,
            source_event_hash: incoming.envelope.hash().clone(),
            receipt: receipt.clone(),
        })
        .expect("the resumed evaluation records its decision");
    assert!(matches!(outcome, IntakeOutcome::Recorded(_)));

    // And now the same delivery arriving a third time is a duplicate, holding
    // the original decision.
    match fixture
        .store
        .ingest_source_event(fixture.project, &incoming)
        .expect("a duplicate is not an error")
    {
        SourceEventIngest::Decided(original) => assert_eq!(original.id, receipt.id),
        other => panic!("expected the original decision, got {other:?}"),
    }
    let census = fixture.census();
    assert_eq!(census.get("source_events"), Some(&1));
    assert_eq!(census.get("intake_receipts"), Some(&1));
}

#[test]
fn the_same_identity_with_different_bytes_is_a_conflict_and_writes_nothing() {
    let fixture = Fixture::open();
    fixture
        .store
        .ingest_source_event(fixture.project, &event("ext-1", "payload"))
        .expect("the first delivery is committed");
    let before = fixture.census();

    let contradiction = event("ext-1", "a different payload");
    assert!(
        fixture
            .store
            .ingest_source_event(fixture.project, &contradiction)
            .is_err(),
        "upstream changing what it said under an id it already used is a conflict"
    );
    assert_eq!(
        fixture.census(),
        before,
        "a refused ingestion writes nothing at all"
    );
}

#[test]
fn one_event_delivered_concurrently_produces_one_event_one_decision_and_one_graph() {
    let fixture = Fixture::open();
    let path = fixture.path();
    let project = fixture.project;
    let trigger = fixture.trigger.clone();
    let incoming = event("ext-race", "payload");

    // Eight racing deliveries of the same event, each through its own
    // connection to the same file, each doing exactly what a delivery does:
    // ingest, then decide.
    let winners: Vec<IntakeReceiptId> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let path = path.clone();
                let trigger = trigger.clone();
                let incoming = incoming.clone();
                scope.spawn(move || {
                    let store = SqliteStore::open(&path).expect("the store opens");
                    let receipt = proposal(&trigger, &incoming, &format!("intake-race-{index}"));
                    match store.record_source_event(&NewSourceEvent {
                        project_id: project,
                        event: incoming,
                        receipt,
                    }) {
                        Ok(
                            IntakeOutcome::Recorded(receipt) | IntakeOutcome::Duplicate(receipt),
                        ) => Some(receipt.id),
                        // A loser may be refused outright by the uniqueness
                        // constraint it collided with; what it must never do is
                        // succeed with a *second* decision.
                        Err(_) => None,
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().expect("no thread panicked"))
            .collect()
    });

    let census = fixture.census();
    assert_eq!(census.get("source_events"), Some(&1), "one event");
    assert_eq!(census.get("intake_receipts"), Some(&1), "one decision");
    assert!(!winners.is_empty(), "at least one delivery must have won");
    let distinct: std::collections::BTreeSet<_> = winners.iter().collect();
    assert_eq!(
        distinct.len(),
        1,
        "every delivery that answered must name the same decision"
    );
}

// ---------------------------------------------------------------------------
// Terminal decisions
// ---------------------------------------------------------------------------

/// Store one proposal and return it.
fn stored_proposal(fixture: &Fixture, external: &str, label: &str) -> IntakeReceipt {
    let incoming = event(external, label);
    let receipt = proposal(&fixture.trigger, &incoming, label);
    fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: incoming,
            receipt: receipt.clone(),
        })
        .expect("the proposal is stored");
    receipt
}

#[test]
fn an_approval_creates_one_graph_with_lineage_and_a_replay_creates_none() {
    let fixture = Fixture::open();
    let proposal = stored_proposal(&fixture, "ext-approve", "intake-approve");
    let command_receipt = fixture.approval_receipt("approve-intake");
    let goal = MiniProjectId::generate();
    let tasks = vec![TaskId::generate(), TaskId::generate()];
    let request = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: fixture.project,
        receipt_id: proposal.id,
        authority: IntakeAuthority::Approval {
            authority: fixture.account,
            command_receipt,
        },
        work: Some(work(fixture.project, Some(goal), &tasks)),
        decided_at: at(DECIDED_AT),
    };

    let decision = fixture
        .store
        .commit_intake_decision(&request)
        .expect("an authorized approval creates the work it names");
    assert_eq!(decision.outcome, IntakeDecisionOutcome::Approved);
    assert_eq!(decision.created_work.len(), 2);
    for lineage in &decision.created_work {
        assert_eq!(lineage.receipt_id, proposal.id);
        assert_eq!(lineage.source_event_id, proposal.source_event_id);
        assert_eq!(lineage.source_event_hash, proposal.source_event_hash);
        assert_eq!(lineage.trigger, proposal.trigger);
        assert_eq!(lineage.trigger_version, proposal.trigger_version);
        assert_eq!(lineage.authority, IntakeDecisionOutcome::Approved);
        assert!(
            lineage.execution_authorization.is_none(),
            "an approval is a human's authority, not an authorization's"
        );
    }
    let after = fixture.census();
    assert_eq!(after.get("tasks"), Some(&2));
    assert_eq!(after.get("mini_projects"), Some(&1));
    assert_eq!(after.get("intake_created_work"), Some(&2));
    assert_eq!(after.get("intake_decisions"), Some(&1));

    // The same approval again: the stored decision, and not one new row.
    let replayed = fixture
        .store
        .commit_intake_decision(&request)
        .expect("a replayed approval is not an error");
    assert_eq!(replayed.id, decision.id);
    assert_eq!(fixture.census(), after, "a replay attached no second graph");

    // A *different* decision about the same proposal is refused outright.
    let second = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        authority: IntakeAuthority::Rejection {
            authority: fixture.account,
            command_receipt: fixture.approval_receipt("reject-after-approve"),
            reason: name("changed my mind"),
        },
        work: None,
        ..request
    };
    assert!(
        fixture.store.commit_intake_decision(&second).is_err(),
        "a proposal has exactly one terminal decision"
    );
    assert_eq!(fixture.census(), after);
}

#[test]
fn an_approval_receipt_must_authorize_this_intake_decision() {
    let fixture = Fixture::open();
    let proposal = stored_proposal(&fixture, "ext-wrong-receipt", "intake-wrong-receipt");
    let (_, wrong_kind_receipt) = fixture.authorization("authorize-not-approve");
    let before = fixture.census();

    let request = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: fixture.project,
        receipt_id: proposal.id,
        authority: IntakeAuthority::Approval {
            authority: fixture.account,
            command_receipt: wrong_kind_receipt,
        },
        work: Some(work(fixture.project, None, &[TaskId::generate()])),
        decided_at: at(DECIDED_AT),
    };

    assert!(
        fixture.store.commit_intake_decision(&request).is_err(),
        "an execution-authorization receipt is not an intake-approval receipt"
    );
    assert_eq!(fixture.census(), before, "the refusal writes nothing");
}

#[test]
fn a_rejection_is_terminal_receipt_backed_and_creates_nothing() {
    let fixture = Fixture::open();
    let proposal = stored_proposal(&fixture, "ext-reject", "intake-reject");
    let before = fixture.census();

    // A rejection that carries work is refused before any row is touched.
    let carrying_work = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: fixture.project,
        receipt_id: proposal.id,
        authority: IntakeAuthority::Rejection {
            authority: fixture.account,
            command_receipt: fixture.approval_receipt("reject-with-work"),
            reason: name("not this one"),
        },
        work: Some(work(fixture.project, None, &[TaskId::generate()])),
        decided_at: at(DECIDED_AT),
    };
    assert!(
        fixture
            .store
            .commit_intake_decision(&carrying_work)
            .is_err()
    );

    // And a decision citing a receipt that authorizes nothing here is refused
    // too: existing in the project is not consent.
    let unauthorized = NewIntakeDecisionRecord {
        authority: IntakeAuthority::Rejection {
            authority: fixture.account,
            command_receipt: CommandReceiptId::generate(),
            reason: name("not this one"),
        },
        work: None,
        ..carrying_work
    };
    assert!(fixture.store.commit_intake_decision(&unauthorized).is_err());
    assert_eq!(fixture.census(), before);

    let rejection = NewIntakeDecisionRecord {
        authority: IntakeAuthority::Rejection {
            authority: fixture.account,
            command_receipt: fixture.approval_receipt("reject-intake"),
            reason: name("out of scope for this quarter"),
        },
        work: None,
        ..unauthorized
    };
    let decision = fixture
        .store
        .commit_intake_decision(&rejection)
        .expect("an authorized rejection is recorded");
    assert_eq!(decision.outcome, IntakeDecisionOutcome::Rejected);
    assert_eq!(
        decision.reason.as_ref().map(ExternalName::as_str),
        Some("out of scope for this quarter"),
        "a rejection says why, so it can be read years later"
    );
    assert!(decision.created_work.is_empty());

    let after = fixture.census();
    assert_eq!(after.get("tasks"), before.get("tasks"));
    assert_eq!(after.get("intake_created_work"), Some(&0));

    // Terminal: an approval afterwards is refused, and the refusal writes
    // nothing.
    let approval = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        authority: IntakeAuthority::Approval {
            authority: fixture.account,
            command_receipt: fixture.approval_receipt("approve-after-reject"),
        },
        work: Some(work(fixture.project, None, &[TaskId::generate()])),
        ..rejection
    };
    assert!(
        fixture.store.commit_intake_decision(&approval).is_err(),
        "a rejection is terminal"
    );
    assert_eq!(fixture.census().get("tasks"), before.get("tasks"));
}

// ---------------------------------------------------------------------------
// Bounded auto-arm, re-checked by the transaction itself
// ---------------------------------------------------------------------------

/// A project whose trigger auto-arms under a real, stored authorization.
struct Armed {
    fixture: Fixture,
    proposal: IntakeReceipt,
    authorization: ExecutionAuthorizationId,
    capability_receipt: CommandReceiptId,
}

fn armed_project(concurrency: u32, budget_of_policy: BudgetBounds) -> Armed {
    let fixture = Fixture::open();
    let (authorization, capability_receipt) = fixture.authorization("authorize-auto-arm");
    let mut spec = fixture.trigger.clone();
    spec.id = kontor_core::id::TriggerKey::parse("trigger.armed").expect("a legal trigger key");
    spec.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: fixture.account,
            execution_authorization: authorization,
        },
        max_concurrency: concurrency,
        budget: budget_of_policy,
    };
    fixture.store_trigger(&spec);

    let incoming = event("ext-armed", "armed-payload");
    let receipt = IntakeReceipt {
        trigger: spec.id.clone(),
        ..proposal(&spec, &incoming, "intake-armed")
    };
    fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: incoming,
            receipt: receipt.clone(),
        })
        .expect("the proposal is stored");
    Armed {
        fixture,
        proposal: receipt,
        authorization,
        capability_receipt,
    }
}

fn auto_arm_request(armed: &Armed, tasks: &[TaskId]) -> NewIntakeDecisionRecord {
    NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: armed.fixture.project,
        receipt_id: armed.proposal.id,
        authority: IntakeAuthority::BoundedAutoArm {
            caller: armed.fixture.account,
            command_receipt: armed.capability_receipt,
        },
        work: Some(work(armed.fixture.project, None, tasks)),
        decided_at: at(DECIDED_AT),
    }
}

#[test]
fn a_bounded_auto_arm_creates_work_with_its_authorization_in_the_lineage() {
    let armed = armed_project(2, budget());
    let tasks = vec![TaskId::generate()];
    let decision = armed
        .fixture
        .store
        .commit_intake_decision(&auto_arm_request(&armed, &tasks))
        .expect("every bound is met");
    assert_eq!(decision.outcome, IntakeDecisionOutcome::AutoArmed);
    assert_eq!(
        decision.capability.map(|c| c.execution_authorization),
        Some(armed.authorization)
    );
    let lineage = armed
        .fixture
        .store
        .intake_lineage_of_task(armed.fixture.project, tasks[0])
        .expect("the read succeeds")
        .expect("the task has lineage");
    assert_eq!(lineage.authority, IntakeDecisionOutcome::AutoArmed);
    assert_eq!(lineage.execution_authorization, Some(armed.authorization));
    assert_eq!(lineage.receipt_id, armed.proposal.id);
}

#[test]
fn the_store_refuses_every_auto_arm_the_policy_refuses() {
    // Over-concurrency: three tasks under a policy ceiling of two.
    let armed = armed_project(2, budget());
    let too_many = vec![TaskId::generate(), TaskId::generate(), TaskId::generate()];
    let before = armed.fixture.census();
    assert!(
        armed
            .fixture
            .store
            .commit_intake_decision(&auto_arm_request(&armed, &too_many))
            .is_err(),
        "the transaction re-checks the concurrency bound rather than trusting the caller"
    );
    assert_eq!(armed.fixture.census(), before, "a refusal writes nothing");

    // Over-budget: a policy budget wider than the authorization's grant.
    let generous = armed_project(
        2,
        BudgetBounds {
            max_tokens: u64::MAX,
            ..budget()
        },
    );
    let tasks = vec![TaskId::generate()];
    assert!(
        generous
            .fixture
            .store
            .commit_intake_decision(&auto_arm_request(&generous, &tasks))
            .is_err(),
        "a policy cannot widen its own authorization by naming a larger number"
    );

    // Wrong caller: an account the capability was not granted to.
    let armed = armed_project(2, budget());
    let mut wrong_caller = auto_arm_request(&armed, &[TaskId::generate()]);
    wrong_caller.authority = IntakeAuthority::BoundedAutoArm {
        caller: AccountProfileId::generate(),
        command_receipt: armed.capability_receipt,
    };
    assert!(
        armed
            .fixture
            .store
            .commit_intake_decision(&wrong_caller)
            .is_err(),
        "only the account the capability names may exercise it"
    );

    // Out of window: a decision instant outside the authorization's range.
    let mut expired = auto_arm_request(&armed, &[TaskId::generate()]);
    expired.decided_at = at("2026-08-14T09:00:00Z");
    assert!(
        armed
            .fixture
            .store
            .commit_intake_decision(&expired)
            .is_err(),
        "an authorization arms nothing outside its own window"
    );

    // A receipt that is not the one that granted the authorization.
    let mut foreign_receipt = auto_arm_request(&armed, &[TaskId::generate()]);
    foreign_receipt.authority = IntakeAuthority::BoundedAutoArm {
        caller: armed.fixture.account,
        command_receipt: armed.fixture.approval_receipt("some-other-receipt"),
    };
    assert!(
        armed
            .fixture
            .store
            .commit_intake_decision(&foreign_receipt)
            .is_err(),
        "a bounded auto-arm cites the receipt that granted its authorization"
    );
}

#[test]
fn an_approval_required_trigger_cannot_be_auto_armed() {
    let fixture = Fixture::open();
    let proposal = stored_proposal(&fixture, "ext-not-armed", "intake-not-armed");
    let (_, capability_receipt) = fixture.authorization("authorize-nothing");
    let request = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: fixture.project,
        receipt_id: proposal.id,
        authority: IntakeAuthority::BoundedAutoArm {
            caller: fixture.account,
            command_receipt: capability_receipt,
        },
        work: Some(work(fixture.project, None, &[TaskId::generate()])),
        decided_at: at(DECIDED_AT),
    };
    let refusal = fixture
        .store
        .commit_intake_decision(&request)
        .expect_err("the shipped trigger requires approval");
    assert!(
        refusal.to_string().contains("policy_requires_approval"),
        "the refusal names the bound that was not met, got {refusal}"
    );
}

// ---------------------------------------------------------------------------
// What the scheduler is handed
// ---------------------------------------------------------------------------

#[test]
fn intake_created_work_reaches_the_scheduler_as_event_origin() {
    let fixture = Fixture::open();
    let proposal = stored_proposal(&fixture, "ext-origin", "intake-origin");
    let task = TaskId::generate();
    fixture
        .store
        .commit_intake_decision(&NewIntakeDecisionRecord {
            id: IntakeDecisionId::generate(),
            project_id: fixture.project,
            receipt_id: proposal.id,
            authority: IntakeAuthority::Approval {
                authority: fixture.account,
                command_receipt: fixture.approval_receipt("approve-origin"),
            },
            work: Some(work(fixture.project, None, &[task])),
            decided_at: at(DECIDED_AT),
        })
        .expect("the approval creates the task");

    // A manually created task in the same project, as the control.
    let manual = TaskId::generate();
    fixture
        .store
        .create_task(&NewTask {
            id: manual,
            project_id: fixture.project,
            mini_project_id: None,
            title: name("Work an operator created"),
            module: None,
            state: TaskState::Ready,
            created_at: at(DECIDED_AT),
        })
        .expect("the manual task is created");
    fixture.with_workflow(task);
    fixture.with_workflow(manual);

    let assembly = fixture
        .store
        .scheduling_candidates(fixture.project, &fixture.runtime())
        .expect("the candidates assemble");
    assert_eq!(assembly.candidates.len(), 2);
    let origin = |wanted: TaskId| {
        assembly
            .candidates
            .iter()
            .find(|candidate| candidate.task_id == wanted)
            .map(|candidate| candidate.origin.clone())
            .expect("the candidate is assembled")
    };

    // The manual task's provenance is not invented: nothing created it through
    // intake, so nothing claims one for it.
    assert_eq!(origin(manual), TaskOrigin::Manual);

    let TaskOrigin::Event { lineage } = origin(task) else {
        panic!("intake created this task, so it is event-origin");
    };
    let lineage = lineage.expect("the receipt that armed it resolves");
    assert_eq!(lineage.receipt_id, proposal.id);
    assert_eq!(lineage.armed_task_id, task);
    assert_eq!(
        lineage.result,
        IntakeResult::Approved,
        "the scheduler receives the decision's resolved status, not the proposal's"
    );
    assert!(
        lineage.auto_arm_authorization.is_none(),
        "an approval arms on a human's authority and names no authorization"
    );

    // And that is all it receives: an approved lineage admits the task it armed
    // and nothing else.
    assert!(origin(task).admits(task).is_ok());
    assert!(
        origin(task).admits(manual).is_err(),
        "a receipt that armed other work is not this task's authority"
    );
}

#[test]
fn auto_armed_work_still_faces_every_ordinary_blocker() {
    // Arming is not launching. An auto-armed task is an ordinary candidate the
    // moment it exists, and each of the scheduler's own blockers has to be able
    // to refuse it on its own — otherwise "bounded auto-arm" would be a way past
    // admission rather than a way into the queue.
    let armed = armed_project(2, budget());
    let task = TaskId::generate();
    armed
        .fixture
        .store
        .commit_intake_decision(&auto_arm_request(&armed, &[task]))
        .expect("the bounded auto-arm creates the work");
    armed.fixture.with_workflow(task);

    let assembly = armed
        .fixture
        .store
        .scheduling_candidates(armed.fixture.project, &armed.fixture.runtime())
        .expect("the candidates assemble");
    let candidate = assembly
        .candidates
        .iter()
        .find(|candidate| candidate.task_id == task)
        .expect("the armed task is a candidate")
        .clone();
    assert!(
        matches!(candidate.origin, TaskOrigin::Event { .. }),
        "the candidate carries its intake origin"
    );

    let snapshot = |candidate: kontor_scheduler::model::Candidate,
                    taken_at: Timestamp,
                    completed: std::collections::BTreeSet<TaskId>| {
        kontor_scheduler::model::SchedulingSnapshot {
            schema_version: kontor_core::id::SCHEMA_VERSION,
            taken_at,
            candidates: vec![candidate],
            in_flight_tasks: std::collections::BTreeSet::new(),
            completed_tasks: completed,
            module_leases: Vec::new(),
            worktree_leases: std::collections::BTreeSet::new(),
            usage: kontor_scheduler::model::CapacityUsage::default(),
            capacity: kontor_scheduler::model::CapacityConfig {
                global_max_in_flight: 10,
                project_max_in_flight: 10,
                mission_max_in_flight: 10,
                account_max_in_flight: 10,
                provider_max_in_flight: 10,
                runtime_max_in_flight: 10,
                adaptive: kontor_scheduler::model::AdaptiveWindowConfig {
                    initial: 10,
                    floor: 1,
                    ceiling: 10,
                    growth_step: 1,
                },
            },
            adaptive_window: kontor_scheduler::model::AdaptiveWindow::start(
                kontor_scheduler::model::AdaptiveWindowConfig {
                    initial: 10,
                    floor: 1,
                    ceiling: 10,
                    growth_step: 1,
                },
            ),
            freshness: jiff::SignedDuration::from_secs(3_600),
        }
    };
    let refusals = |candidate: kontor_scheduler::model::Candidate, taken_at: Timestamp| {
        let snapshot = snapshot(candidate.clone(), taken_at, Default::default());
        kontor_scheduler::explain(&snapshot, &candidate)
            .expect("the snapshot is admissible")
            .iter()
            .map(|refused| refused.blocker)
            .collect::<Vec<_>>()
    };

    // Readiness: a draft task is refused however it was armed.
    let mut draft = candidate.clone();
    draft.state = TaskState::Draft;
    assert!(
        refusals(draft, at(DECIDED_AT)).contains(&kontor_scheduler::Blocker::Readiness),
        "an auto-armed task that is not ready is still not ready"
    );

    // Dependencies: an unfinished predecessor blocks it.
    let mut blocked = candidate.clone();
    blocked.depends_on = [TaskId::generate()].into_iter().collect();
    assert!(
        refusals(blocked, at(DECIDED_AT)).contains(&kontor_scheduler::Blocker::Dependencies),
        "an auto-armed task waits for its dependencies like any other"
    );

    // Authorization: the same task, judged outside the authorization's window.
    assert!(
        refusals(candidate.clone(), at("2026-08-14T09:00:00Z"))
            .contains(&kontor_scheduler::Blocker::Authorization),
        "arming does not extend the window the work may start in"
    );

    // Calendar: a closed answer refuses it.
    let mut closed = candidate.clone();
    closed.calendar = kontor_scheduler::model::CalendarAdmission {
        state: kontor_core::calendar::EffectiveCalendarState::Closed,
        policy: Some(kontor_scheduler::model::CalendarPolicyEvidence {
            profile_id: kontor_core::id::CalendarProfileId::generate(),
            policy_revision: SpecVersion::FIRST,
            timezone: kontor_core::calendar::IanaTimeZone::parse("Europe/Oslo")
                .expect("a legal zone"),
            matched_window: None,
        }),
        override_id: None,
        next_opening: None,
    };
    assert!(
        refusals(closed, at(DECIDED_AT)).contains(&kontor_scheduler::Blocker::Calendar),
        "an auto-armed task obeys the calendar answer it was handed"
    );

    // Runtime: an unavailable runtime refuses it.
    let mut unreachable = candidate.clone();
    unreachable.runtime.health = kontor_scheduler::model::RuntimeHealth::Unavailable;
    assert!(
        refusals(unreachable, at(DECIDED_AT)).contains(&kontor_scheduler::Blocker::Runtime),
        "an auto-armed task is not started on a runtime that is not answering"
    );

    // And the origin blocker is *not* what refuses any of them: the lineage is
    // sound, which is precisely why the other blockers are the ones speaking.
    assert!(
        !refusals(candidate, at(DECIDED_AT)).contains(&kontor_scheduler::Blocker::Origin),
        "a real auto-arm lineage is not an origin refusal"
    );
}

// ---------------------------------------------------------------------------
// Re-evaluation under a newer revision
// ---------------------------------------------------------------------------

#[test]
fn a_newer_trigger_revision_appends_a_successor_and_mutates_nothing() {
    let fixture = Fixture::open();
    let proposal = stored_proposal(&fixture, "ext-successor", "intake-successor");

    let mut newer = fixture.trigger.clone();
    newer.version = SpecVersion::parse(2).expect("a legal revision");
    fixture
        .store
        .insert_trigger_spec(fixture.project, &newer)
        .expect("the newer revision is stored");

    let successor = IntakeReceipt {
        id: IntakeReceiptId::generate(),
        trigger_version: newer.version,
        idempotency_key: key("intake-successor-2"),
        ..proposal.clone()
    };
    let outcome = fixture
        .store
        .reevaluate_source_event(&NewIntakeReevaluation {
            project_id: fixture.project,
            source_event_id: proposal.source_event_id,
            source_event_hash: proposal.source_event_hash.clone(),
            receipt: successor,
        })
        .expect("a strictly newer revision may supersede");
    let kontor_core::repository::ReevaluationOutcome::Superseded(successor) = outcome else {
        panic!("expected a successor");
    };
    assert_eq!(successor.predecessor_receipt_id, Some(proposal.id));

    // The predecessor is untouched: superseding is appending, not rewriting.
    let stored = fixture
        .store
        .get_intake_receipt(fixture.project, proposal.id)
        .expect("the read succeeds")
        .expect("the original is still there");
    assert_eq!(stored, proposal);
    assert_eq!(fixture.census().get("intake_receipts"), Some(&2));
}

// ---------------------------------------------------------------------------
// Nothing here launches anything
// ---------------------------------------------------------------------------

#[test]
fn the_intake_module_starts_no_run_and_dispatches_no_command() {
    let source = std::fs::read_to_string("src/intake.rs").expect("the module is readable");
    let executable: String = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "agent_runs",
        "team_runs",
        "command_outbox",
        "runtime_events",
        "LaunchRun",
        "DesiredRunState",
    ] {
        assert!(
            !executable.contains(forbidden),
            "intake names `{forbidden}`. It creates work and lineage; starting that work \
             is the scheduler's decision and the runtime's effect"
        );
    }
}

#[test]
fn a_decision_about_an_unknown_or_changed_event_is_refused() {
    let fixture = Fixture::open();
    let incoming = event("ext-unknown", "payload");
    let receipt = proposal(&fixture.trigger, &incoming, "intake-unknown");

    assert!(
        fixture
            .store
            .record_intake_decision(&NewIntakeDecision {
                project_id: fixture.project,
                source_event_id: incoming.id,
                source_event_hash: incoming.envelope.hash().clone(),
                receipt: receipt.clone(),
            })
            .is_err(),
        "a decision about an event nobody stored decides nothing"
    );

    fixture
        .store
        .ingest_source_event(fixture.project, &incoming)
        .expect("the event is committed");
    assert!(
        fixture
            .store
            .record_intake_decision(&NewIntakeDecision {
                project_id: fixture.project,
                source_event_id: incoming.id,
                source_event_hash: ContentHash::of(b"some other bytes"),
                receipt,
            })
            .is_err(),
        "a decision citing another digest of the event is about another event"
    );
    assert_eq!(fixture.census().get("intake_receipts"), Some(&0));
}
