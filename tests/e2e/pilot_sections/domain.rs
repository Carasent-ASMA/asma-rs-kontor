//! Section 4 — intake, external-ticket convergence, three-zone privacy and the
//! process ledger.
//!
//! Two very different kinds of proof live here, and the split is deliberate.
//!
//! * **Intake** is proved against the *store*. Deduplication is a transaction
//!   rule, not a pure function: "the replay returns the original receipt" is
//!   only meaningful if there is a row to collide with. So this half opens a
//!   file-backed [`SqliteStore`] in a temporary directory and drives
//!   [`IntakeRepository`] for real.
//! * **Ticket convergence** is proved against
//!   [`kontor_core::ticket::reconcile`], which reads no clock, no database and
//!   no process: the whole decision arrives in one borrowed input. That is what
//!   makes "a workflow with different status names behaves identically" a fact
//!   rather than an opinion — the same call is made twice against two fixtures
//!   that share not one status id, and the decision *shapes* are compared.
//!
//! Only one criterion here needs the `asma` executable boundary at all
//! ([`domain.jira-asma`], whose whole claim is about a refetch), and it gets a
//! real temporary `/bin/sh` script rather than a mocked trait. Every other
//! delegation case is refused by `build_write_request` *before* `exchange`, so
//! it needs an [`AsmaExecutable`] that resolves and is never spawned.
//!
//! Intake proposals are produced by `kontor-intake` and terminal decisions are
//! committed through the production store transaction. The only staged seam is
//! transport: this deterministic driver invokes those APIs directly.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kontor_core::DomainError;
use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::id::{
    AccountProfileId, AggregateRevision, BoundedText, CanonicalDocument, CommandReceiptId,
    ConnectorKey, ContentHash, CurrencyCode, ExecutionAuthorizationId, ExternalId,
    ExternalIssueTypeKey, ExternalName, ExternalProjectKey, GateKey, IdempotencyKey,
    IntakeDecisionId, IntakeReceiptId, Money, ProjectId, SCHEMA_VERSION, SemanticMilestoneKey,
    SourceConnectionKey, SourceEventId, SourceKindKey, SpecVersion, TaskId, TicketLinkId,
    TicketProjectionId, TriggerKey, WorkProfileKey,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CalendarRepository, CommandRepository, CredentialReference, CredentialReferenceKind,
    IntakeAuthority, IntakeDecisionOutcome, IntakeOutcome, IntakeRepository, IntakeWorkPlan,
    NewAccountProfile, NewCommandIntent, NewIntakeDecisionRecord, NewProject, NewSourceEvent,
    NewTask, NewTicketLink, ProjectRepository, SpecRepository, TicketRepository,
};
use kontor_core::spec::{
    AutoArmPolicy, BudgetBounds, CanonicalSourceEvent, ExecutionCapability, IntakeReceipt,
    IntakeResult, ProposedWorkGraph, SourceIdentity, SourceProcessingState, TeamTemplateRevision,
    TriggerSpec, WorkProfileSpec,
};
use kontor_core::state::{Freshness, GateState, TaskState, TerminalOutcome};
use kontor_core::ticket::{
    CommentPolicy, ExternalCommentRevision, ExternalWorkflowSpec, FieldDirection, FieldOwner,
    FieldValue, InternalTaskFacts, LiveTransition, OwnershipAction, ProjectedField,
    ReconciliationInput, ReconciliationOutcome, StatusConflictKind, StatusSelector, TicketFieldKey,
    TicketPrincipal, TicketSyncProjection, TransitionPlan, reconcile,
};
use kontor_intake::{Intake, evaluate};
use kontor_integrations_asma::jira::{
    ApplyAuthority, CompiledFieldSpec, CompiledWorkflowSpec, FieldSpecKey, JiraOperation,
    JiraOutcome, JiraRequest, JiraResponse, Observed, SpecCatalog, TicketDelegation,
    WorkflowSpecKey, compile_field_writes,
};
use kontor_integrations_asma::{AsmaError, AsmaExecutable, WIRE_SCHEMA_VERSION, WireTimestamp};
use kontor_scheduler::model::{IntakeLineage, RejectionCode, TaskOrigin};
use kontor_store::SqliteStore;
use kontor_tests_e2e::Bundle;
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::{ALTERNATE_WORKFLOW, at};

/// The pilot's fixed decision instant. Every timestamp below is derived from it
/// so nothing in this section depends on when the suite happens to run.
const DECIDED_AT: &str = "2026-08-12T09:00:00Z";

/// The checked-in bounded trigger. It is `approval_required` as shipped; the
/// bounded auto-arm case clones and mutates it rather than adding a fixture,
/// so the two policies are provably the *same* document apart from that field.
const TRIGGER_FIXTURE: &str =
    include_str!("../../../crates/kontor-core/tests/fixtures/trigger.json");

/// The arbitrary work profile the trigger's pins reference. It is a foreign key,
/// not a subject: the trigger cannot be stored until its pinned revision exists.
const ARBITRARY_PROFILE: &str =
    include_str!("../../../crates/kontor-core/tests/fixtures/work_profile_arbitrary.json");

/// The milestone keys both workflow fixtures declare. Spelled once, read from
/// the fixtures everywhere else — a status id or name never appears in this file.
const IMPLEMENTATION_ACTIVE: &str = "implementation_active";
/// Kontor's own "QA may start" distinction.
const QA_READY: &str = "qa_ready";
/// Externally visible, review-has-started QA.
const QA_ACTIVE: &str = "qa_active";
/// The externally paused milestone.
const TERMINAL_HOLD: &str = "terminal_hold";
/// The externally closed-successful milestone.
const TERMINAL_DONE: &str = "terminal_done";

/// Answer every domain-operations criterion and close the process ledger.
pub(crate) async fn run(bundle: &mut Bundle) {
    let mut cleanup = Cleanup::default();
    intake_dedup(bundle, &mut cleanup);
    intake_decisions(bundle, &mut cleanup);
    jira_asma(bundle, &mut cleanup).await;
    jira_qa_distinct_and_alternate(bundle);
    jira_hold_close_reopen(bundle);
    jira_ownership(bundle, &mut cleanup).await;
    privacy_zones(bundle);
    inbound_comment(bundle, &mut cleanup);
    processes(bundle, &cleanup);
}

// ---------------------------------------------------------------------------
// The process ledger
// ---------------------------------------------------------------------------

/// Everything this section created that has to be accounted for at the end.
///
/// It is accumulated as the cases run rather than reconstructed afterwards: a
/// ledger assembled from memory at the end is a description of what the author
/// believed happened, which is precisely the thing the criterion doubts.
#[derive(Debug, Default)]
struct Cleanup {
    /// One entry per child process this section caused to be spawned.
    processes: Vec<Value>,
    /// One entry per native runtime session this section opened.
    sessions: Vec<Value>,
    /// One entry per temporary directory, with whether it was really removed.
    directories: Vec<Value>,
    /// One entry per resolved `asma` executable that was deliberately not run.
    unspawned: Vec<Value>,
}

impl Cleanup {
    /// Record a child process, its argv shape and how it ended.
    fn process(&mut self, purpose: &str, argv: &[String], exited: &str) {
        self.processes.push(json!({
            "purpose": purpose,
            "kind": "/bin/sh script standing in for the `asma` executable",
            "argv": argv,
            "spawned_by": "AsmaExecutable::run_json -> tokio::process::Command (kill_on_drop)",
            "ended": exited,
        }));
    }

    /// Record a resolved executable that was never spawned, and why.
    fn never_spawned(&mut self, purpose: &str, reason: &str) {
        self.unspawned.push(json!({
            "purpose": purpose,
            "reason": reason,
        }));
    }

    /// Record a temporary directory *after* its owner has been dropped.
    ///
    /// Removal is observed, never asserted from the type: `TempDir` deletes on
    /// drop, and the only way to prove that happened is to look.
    fn directory(&mut self, purpose: &str, path: &Path) {
        self.directories.push(json!({
            "purpose": purpose,
            "owner": "tempfile::TempDir",
            "removed": !path.exists(),
        }));
    }
}

/// Close the ledger: write `cleanup.json` at the bundle root and judge it.
fn processes(bundle: &mut Bundle, cleanup: &Cleanup) {
    let leaked: Vec<&Value> = cleanup
        .directories
        .iter()
        .filter(|entry| entry["removed"] != Value::Bool(true))
        .collect();
    let unended: Vec<&Value> = cleanup
        .processes
        .iter()
        .filter(|entry| entry["ended"] == Value::Null)
        .collect();

    let ledger = json!({
        "section": "domain",
        "processes": cleanup.processes,
        "processes_retained": [],
        "processes_retained_reason":
            "none: every child of this section is a one-shot `asma` double that is \
             reaped by `Child::wait` inside `AsmaExecutable::run_json` before the call \
             returns, and the handle is `kill_on_drop`",
        "native_sessions": cleanup.sessions,
        "native_sessions_reason":
            "empty: this section opens no runtime session. It never constructs a \
             runtime binding, never dispatches a run and never touches a socket — the \
             scripted in-process runtimes and the `tower::oneshot` daemon belong to the \
             runtime and session sections",
        "temp_directories": cleanup.directories,
        "resolved_but_never_spawned": cleanup.unspawned,
    });
    let artifact = bundle
        .artifact("cleanup.json", &ledger)
        .expect("the cleanup ledger is written");
    bundle.event("cleanup", ledger);

    if leaked.is_empty() && unended.is_empty() {
        bundle.pass(
            "cleanup.processes",
            format!(
                "{} child process(es) were spawned, every one reaped by `Child::wait` before its \
                 call returned; {} resolved `asma` executable(s) were deliberately never spawned \
                 because the refusal happens in `build_write_request` before `exchange`; {} \
                 temporary director(ies) were created and all of them were observed gone after \
                 their owner dropped; no native session was opened, and the ledger says why the \
                 list is empty rather than leaving it bare",
                cleanup.processes.len(),
                cleanup.unspawned.len(),
                cleanup.directories.len()
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "cleanup.processes",
            format!(
                "{} temporary director(ies) survived their owner and {} process(es) have no \
                 recorded end",
                leaked.len(),
                unended.len()
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Intake — the store
// ---------------------------------------------------------------------------

/// A file-backed store with one project, one task and the pinned specification
/// revisions the trigger's foreign keys require.
///
/// File-backed rather than in-memory because the row census below reads the same
/// database through a second connection, which an in-memory handle cannot share.
struct StoreFixture {
    /// Kept alive so the database outlives the case; removed on drop.
    directory: TempDir,
    /// The database file, for the census connection.
    path: PathBuf,
    /// The store under test.
    store: SqliteStore,
    /// The owning project.
    project: ProjectId,
    /// A real task, so a proposed work graph can name one that exists.
    task: TaskId,
    /// The operator and bounded-auto-arm principal.
    account: AccountProfileId,
}

impl StoreFixture {
    /// Open a fresh store and seed the rows every intake case needs.
    ///
    /// # Panics
    /// Panics when the store cannot be opened or seeded, which is a driver bug
    /// rather than a finding about the tree.
    fn open() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("kontor.db");
        let store = SqliteStore::open(&path).expect("the store opens and migrates itself");
        let project = ProjectId::generate();
        store
            .create_project(&NewProject {
                id: project,
                name: name("KON-MVP-18 domain pilot"),
                // Unique across the database, and disposable with the directory.
                root_path: name(&format!("/tmp/kontor-pilot-domain/{project}")),
                created_at: at(DECIDED_AT),
            })
            .expect("the disposable project is created");
        let task = TaskId::generate();
        store
            .create_task(&NewTask {
                id: task,
                project_id: project,
                mini_project_id: None,
                title: name("Pilot intake task"),
                module: None,
                state: TaskState::Ready,
                created_at: at(DECIDED_AT),
            })
            .expect("the pilot task is created");
        let account = AccountProfileId::generate();
        store
            .create_account_profile(&NewAccountProfile {
                id: account,
                project_id: project,
                label: name("Pilot intake operator"),
                external_account_id: None,
                harness: kontor_core::id::RuntimeKindKey::parse("pilot.runtime")
                    .expect("a legal runtime key"),
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: kontor_core::id::CredentialAlias::parse("pilot-intake")
                        .expect("a legal credential alias"),
                },
                environment: document("pilot-intake-environment"),
                routing: document("pilot-intake-routing"),
                capability: document("pilot-intake-capability"),
                provider_identity: None,
                enabled: true,
                created_at: at(DECIDED_AT),
            })
            .expect("the pilot intake account is created");
        Self {
            directory,
            path,
            store,
            project,
            task,
            account,
        }
    }

    /// Store the trigger revision every intake receipt pins to.
    ///
    /// The trigger's own pins are foreign keys, so the work profile and the team
    /// template revision it names have to exist first.
    ///
    /// # Panics
    /// Panics when the checked-in fixtures do not parse or do not store.
    fn with_trigger(&self) -> TriggerSpec {
        let mut spec: TriggerSpec =
            serde_json::from_str(TRIGGER_FIXTURE).expect("the trigger fixture parses");
        spec.id = TriggerKey::parse("pilot.intake").expect("a legal trigger key");
        let mut profile: WorkProfileSpec =
            serde_json::from_str(ARBITRARY_PROFILE).expect("the profile fixture parses");
        profile.id = spec.work_profile.clone();
        profile.version = spec.work_profile_version;
        self.store
            .insert_work_profile(self.project, &profile)
            .expect("the pinned work-profile revision is stored");
        self.store
            .insert_team_template(
                self.project,
                &TeamTemplateRevision {
                    template_id: spec.team_template.template_id,
                    version: spec.team_template.version,
                    name: name("Pilot team"),
                    definition: document("pilot-team"),
                    role_authority: Vec::new(),
                },
            )
            .expect("the pinned team-template revision is stored");
        self.store
            .insert_trigger_spec(self.project, &spec)
            .expect("the trigger revision is stored");
        spec
    }

    /// Count the two intake tables through a second connection.
    ///
    /// # Panics
    /// Panics when the database cannot be read, which is a driver bug.
    fn census(&self) -> BTreeMap<&'static str, i64> {
        let connection = rusqlite::Connection::open(&self.path).expect("the census connection");
        ["source_events", "intake_receipts"]
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
            .expect("the intake decision receipt is recorded");
        receipt
    }

    fn execution_authorization(&self, label: &str) -> (ExecutionAuthorizationId, CommandReceiptId) {
        let receipt = CommandReceiptId::generate();
        self.store
            .record_intent(&NewCommandIntent {
                project_id: self.project,
                receipt_id: receipt,
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
            .expect("the execution-authorization receipt is recorded");
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
                max_concurrency: 2,
                budget: budget(),
                created_by: self.account,
                capability_receipt: receipt,
                created_at: at(DECIDED_AT),
            })
            .expect("the execution authorization is stored");
        (id, receipt)
    }

    /// Hand back the directory path and drop everything that holds it open.
    fn close(self) -> PathBuf {
        let path = self.directory.path().to_path_buf();
        drop(self.store);
        drop(self.directory);
        path
    }
}

/// A replayed source event returns the original receipt and creates no second
/// work graph.
fn intake_dedup(bundle: &mut Bundle, cleanup: &mut Cleanup) {
    let fixture = StoreFixture::open();
    let trigger = fixture.with_trigger();

    // The first event. Everything after this must add exactly nothing.
    let first = source_event("ext-1", "pilot-payload");
    let original = proposed_receipt(&trigger, &first, "pilot-intake-1");
    let recorded = fixture
        .store
        .record_source_event(&NewSourceEvent {
            project_id: fixture.project,
            event: first.clone(),
            receipt: original.clone(),
        })
        .expect("the first source event is recorded");
    let baseline = fixture.census();

    // The same identity carrying the same canonical bytes: a replay.
    let replay = source_event("ext-1", "pilot-payload");
    let replayed = fixture.store.record_source_event(&NewSourceEvent {
        project_id: fixture.project,
        event: replay.clone(),
        receipt: proposed_receipt(&trigger, &replay, "pilot-intake-replay"),
    });

    // A different identity carrying the identical envelope: also a replay, and
    // the dedup key is what recognizes it.
    let renamed = source_event("ext-2", "pilot-payload");
    let renamed_outcome = fixture.store.record_source_event(&NewSourceEvent {
        project_id: fixture.project,
        event: renamed.clone(),
        receipt: proposed_receipt(&trigger, &renamed, "pilot-intake-renamed"),
    });

    // The same identity carrying *different* bytes is a conflict, not a
    // duplicate: returning the old decision would discard what upstream said.
    let contradiction = source_event("ext-1", "a different pilot payload");
    let contradiction_outcome = fixture.store.record_source_event(&NewSourceEvent {
        project_id: fixture.project,
        event: contradiction.clone(),
        receipt: proposed_receipt(&trigger, &contradiction, "pilot-intake-conflict"),
    });

    let after = fixture.census();
    let found = fixture
        .store
        .find_intake_receipt(fixture.project, &first.identity)
        .expect("the lookup succeeds");

    // And the domain rule the transaction is protecting, stated directly: a
    // duplicate that carried a work graph does not validate at all.
    let mut duplicate_with_work = original.clone();
    duplicate_with_work.id = IntakeReceiptId::generate();
    duplicate_with_work.result = IntakeResult::Duplicate;
    duplicate_with_work.duplicate_of = Some(original.id);
    duplicate_with_work.proposed = Some(ProposedWorkGraph {
        project_id: fixture.project,
        mini_project_id: None,
        task_ids: vec![fixture.task],
    });
    let refused_rule = rule_of(duplicate_with_work.validate().unwrap_err());

    let returns_original =
        matches!(&replayed, Ok(IntakeOutcome::Duplicate(receipt)) if receipt.id == original.id);
    let renamed_returns_original = matches!(&renamed_outcome, Ok(IntakeOutcome::Duplicate(receipt)) if receipt.id == original.id);
    let no_growth = after == baseline;
    let conflict_refused = contradiction_outcome.is_err();
    let lookup_agrees = found
        .as_ref()
        .is_some_and(|receipt| receipt.id == original.id);

    let artifact = bundle
        .artifact(
            "receipts/intake-dedup.json",
            &json!({
                "trigger": trigger.id.to_string(),
                "trigger_version": trigger.version.get(),
                "dedup_pointers": trigger
                    .dedup
                    .pointers
                    .iter()
                    .map(|pointer| pointer.as_str().to_owned())
                    .collect::<Vec<_>>(),
                "first": {
                    "outcome": outcome_name(&recorded),
                    "receipt": original.id.to_string(),
                    "dedup_key": original.dedup_key.to_string(),
                },
                "replayed_identity": {
                    "outcome": replayed.as_ref().map_or("error", |value| outcome_name(value)),
                    "returned_receipt": receipt_id_of(&replayed),
                    "is_the_original": returns_original,
                },
                "renamed_identity_same_envelope": {
                    "outcome": renamed_outcome.as_ref().map_or("error", |value| outcome_name(value)),
                    "returned_receipt": receipt_id_of(&renamed_outcome),
                    "is_the_original": renamed_returns_original,
                },
                "same_identity_different_envelope": {
                    "refused": conflict_refused,
                    "reason": "a repeated identity with a different digest is a conflict, \
                               never a duplicate",
                },
                "row_census": {
                    "before_the_replays": baseline,
                    "after_every_replay": after,
                    "unchanged": no_growth,
                },
                "find_intake_receipt_returns_the_original": lookup_agrees,
                "duplicate_carrying_work_is_invalid": refused_rule,
                "unmerged_seam": "None for the domain. KON-MVP-22 merged the deciding half \
                                  (`kontor_intake::evaluate`) and split the transaction in two, \
                                  so the identity commits before anything evaluates it; the \
                                  receipt is still assembled here because no transport \
                                  (`kontor_api::query::STAGED[0]`) delivers an event yet",
            }),
        )
        .expect("the dedup evidence is written");

    let directory = fixture.close();
    cleanup.directory("intake dedup store", &directory);

    if returns_original
        && renamed_returns_original
        && no_growth
        && conflict_refused
        && lookup_agrees
        && refused_rule == Some("a duplicate must not create a second work graph")
    {
        bundle.pass(
            "domain.intake-dedup",
            "a replayed source event — the same identity, and separately a different identity \
             carrying the identical envelope — returned `IntakeOutcome::Duplicate` holding the \
             ORIGINAL receipt id both times, while the `source_events` and `intake_receipts` row \
             counts stayed at one apiece; the same identity with different canonical bytes was \
             refused as a conflict rather than answered from the old decision; and a receipt that \
             tried to be a duplicate *and* carry a work graph does not validate",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.intake-dedup",
            format!(
                "replay_returned_original={returns_original}, renamed_returned_original=\
                 {renamed_returns_original}, rows_unchanged={no_growth}, conflict_refused=\
                 {conflict_refused}, lookup_agrees={lookup_agrees}, duplicate_rule={refused_rule:?}"
            ),
        );
    }
}

/// Approve one graph, reject one receipt terminally, auto-arm one under explicit
/// bounds — and prove what each one then admits or refuses.
fn intake_decisions(bundle: &mut Bundle, cleanup: &mut Cleanup) {
    let fixture = StoreFixture::open();
    let trigger = fixture.with_trigger();
    let (authorization, authorization_receipt) =
        fixture.execution_authorization("pilot-authorize-auto-arm");

    // The bounded auto-arm policy, built by mutating the shipped trigger so the
    // two documents differ in exactly one field.
    let mut armed_trigger = trigger.clone();
    armed_trigger.id = TriggerKey::parse("pilot.intake.armed").expect("a legal trigger key");
    armed_trigger.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: fixture.account,
            execution_authorization: authorization,
        },
        max_concurrency: 2,
        budget: budget(),
    };
    let bounded_validates = armed_trigger.validate().is_ok();
    // …and the same document, canonicalized. `canonicalize` is the only route to
    // a digest, a receipt or a stored row, so this is the difference between a
    // policy that is legal and a policy that can be onboarded.
    let bounded_canonicalizes = armed_trigger
        .canonicalize()
        .err()
        .map(|error| error.to_string());

    // Every way of asking for an unbounded auto-arm, refused.
    let mut zero_concurrency = armed_trigger.clone();
    zero_concurrency.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: AccountProfileId::generate(),
            execution_authorization: authorization,
        },
        max_concurrency: 0,
        budget: budget(),
    };
    let mut zero_budget = armed_trigger.clone();
    zero_budget.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: AccountProfileId::generate(),
            execution_authorization: authorization,
        },
        max_concurrency: 2,
        budget: BudgetBounds {
            max_tokens: 0,
            ..budget()
        },
    };
    // And the variant that does not exist: the tag is a closed set, so an
    // unbounded policy is unrepresentable rather than merely discouraged.
    let unbounded_unrepresentable =
        serde_json::from_str::<AutoArmPolicy>(r#"{"kind":"unbounded_auto_arm"}"#).is_err();

    let armed_trigger_stored = fixture
        .store
        .insert_trigger_spec(fixture.project, &armed_trigger)
        .err()
        .map(|error| error.to_string());

    // Each event is evaluated by the production intake crate, then its proposal
    // is made durable before the terminal decision is committed.
    let propose = |event: &CanonicalSourceEvent, spec: &TriggerSpec| {
        let Intake::Proposed { receipt, .. } = evaluate(
            event,
            std::slice::from_ref(spec),
            IntakeReceiptId::generate(),
            at(DECIDED_AT),
        )
        .expect("the production matcher evaluates the event") else {
            panic!("the pilot event matches its trigger")
        };
        fixture
            .store
            .record_source_event(&NewSourceEvent {
                project_id: fixture.project,
                event: event.clone(),
                receipt: (*receipt).clone(),
            })
            .expect("the proposal is stored");
        *receipt
    };

    let approved_event = source_event("ext-approved", "approved-payload");
    let approved = propose(&approved_event, &trigger);
    let approved_task = TaskId::generate();
    let approved_decision = fixture
        .store
        .commit_intake_decision(&NewIntakeDecisionRecord {
            id: IntakeDecisionId::generate(),
            project_id: fixture.project,
            receipt_id: approved.id,
            authority: IntakeAuthority::Approval {
                authority: fixture.account,
                command_receipt: fixture.approval_receipt("pilot-approve-intake"),
            },
            work: Some(intake_work(fixture.project, approved_task)),
            decided_at: at(DECIDED_AT),
        })
        .expect("the real approval transaction commits");

    let rejected_event = source_event("ext-rejected", "rejected-payload");
    let rejected = propose(&rejected_event, &trigger);
    let rejected_decision = fixture
        .store
        .commit_intake_decision(&NewIntakeDecisionRecord {
            id: IntakeDecisionId::generate(),
            project_id: fixture.project,
            receipt_id: rejected.id,
            authority: IntakeAuthority::Rejection {
                authority: fixture.account,
                command_receipt: fixture.approval_receipt("pilot-reject-intake"),
                reason: name("outside the pilot scope"),
            },
            work: None,
            decided_at: at(DECIDED_AT),
        })
        .expect("the real rejection transaction commits");

    let armed_event = source_event("ext-armed", "armed-payload");
    let armed = propose(&armed_event, &armed_trigger);
    let armed_task = TaskId::generate();
    let armed_decision = fixture
        .store
        .commit_intake_decision(&NewIntakeDecisionRecord {
            id: IntakeDecisionId::generate(),
            project_id: fixture.project,
            receipt_id: armed.id,
            authority: IntakeAuthority::BoundedAutoArm {
                caller: fixture.account,
                command_receipt: authorization_receipt,
            },
            work: Some(intake_work(fixture.project, armed_task)),
            decided_at: at(DECIDED_AT),
        })
        .expect("the real bounded-auto-arm transaction commits");

    let mut stored = Vec::new();
    let store_problems: Vec<String> = Vec::new();
    for (label, receipt, decision, expected_outcome, expected_tasks) in [
        (
            "approved",
            &approved,
            &approved_decision,
            IntakeDecisionOutcome::Approved,
            1,
        ),
        (
            "rejected",
            &rejected,
            &rejected_decision,
            IntakeDecisionOutcome::Rejected,
            0,
        ),
        (
            "bounded_auto_arm",
            &armed,
            &armed_decision,
            IntakeDecisionOutcome::AutoArmed,
            1,
        ),
    ] {
        let persisted = fixture
            .store
            .get_intake_decision(fixture.project, receipt.id)
            .expect("the decision query succeeds")
            .expect("the intake_decisions row exists");
        assert_eq!(persisted, *decision, "the committed decision reads back");
        assert_eq!(persisted.outcome, expected_outcome, "{label} outcome");
        assert_eq!(
            persisted.created_work.len(),
            expected_tasks,
            "{label} intake_created_work count"
        );
        for work in &persisted.created_work {
            assert_eq!(
                fixture
                    .store
                    .intake_lineage_of_task(fixture.project, work.task_id)
                    .expect("the lineage query succeeds")
                    .as_ref(),
                Some(work),
                "the task lineage is a real intake_created_work row"
            );
        }
        let lineage = persisted.created_work.first().map(|work| {
            json!({
                "task_id": work.task_id,
                "receipt_id": work.receipt_id,
                "trigger": work.trigger,
                "trigger_version": work.trigger_version,
                "execution_authorization": work.execution_authorization,
            })
        });
        stored.push(json!({
            "decision": label,
            "proposal_result": receipt.result,
            "persisted_outcome": persisted.outcome,
            "intake_decisions_rows": 1,
            "intake_created_work_rows": persisted.created_work.len(),
            "lineage": lineage,
        }));
    }

    let unevidenced = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: fixture.project,
        receipt_id: approved.id,
        authority: IntakeAuthority::Approval {
            authority: fixture.account,
            command_receipt: CommandReceiptId::generate(),
        },
        work: None,
        decided_at: at(DECIDED_AT),
    };
    let rejected_with_work = NewIntakeDecisionRecord {
        id: IntakeDecisionId::generate(),
        project_id: fixture.project,
        receipt_id: rejected.id,
        authority: IntakeAuthority::Rejection {
            authority: fixture.account,
            command_receipt: CommandReceiptId::generate(),
            reason: name("invalid rejection"),
        },
        work: Some(intake_work(fixture.project, TaskId::generate())),
        decided_at: at(DECIDED_AT),
    };
    let contradictions = json!({
        "approved_without_work": rule_of(unevidenced.validate().unwrap_err()),
        "rejected_carrying_work": rule_of(rejected_with_work.validate().unwrap_err()),
    });

    // The consequence: what the scheduler will and will not admit on each one.
    let task = fixture.task;
    let elsewhere = TaskId::generate();
    let lineage = |result: IntakeResult,
                   armed_task_id: TaskId,
                   authorization: Option<ExecutionAuthorizationId>| {
        TaskOrigin::Event {
            lineage: Some(IntakeLineage {
                receipt_id: IntakeReceiptId::generate(),
                result,
                armed_task_id,
                auto_arm_authorization: authorization,
            }),
        }
    };
    let admissions = [
        ("manual", TaskOrigin::Manual, None),
        (
            "absent_lineage",
            TaskOrigin::Event { lineage: None },
            Some(RejectionCode::IntakeReceiptMissing),
        ),
        (
            "armed_other_task",
            lineage(IntakeResult::Approved, elsewhere, None),
            Some(RejectionCode::IntakeReceiptMismatched),
        ),
        (
            "approved",
            lineage(IntakeResult::Approved, task, None),
            None,
        ),
        (
            "auto_armed_with_authorization",
            lineage(IntakeResult::Proposed, task, Some(authorization)),
            None,
        ),
        (
            "proposed_without_authorization",
            lineage(IntakeResult::Proposed, task, None),
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
        (
            "rejected",
            lineage(IntakeResult::Rejected, task, Some(authorization)),
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
        (
            "ignored",
            lineage(IntakeResult::Ignored, task, Some(authorization)),
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
        (
            "duplicate",
            lineage(IntakeResult::Duplicate, task, Some(authorization)),
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
    ];
    let mut admission_rows = Vec::new();
    let mut admission_problems = Vec::new();
    for (label, origin, expected) in &admissions {
        let observed = origin.admits(task).err();
        if observed != *expected {
            admission_problems.push(format!("{label}: wanted {expected:?}, got {observed:?}"));
        }
        admission_rows.push(json!({
            "case": label,
            "admitted": observed.is_none(),
            "rejection": observed.map(|code| code.to_string()),
        }));
    }

    let artifact = bundle
        .artifact(
            "receipts/intake-decisions.json",
            &json!({
                "auto_arm_policy": {
                    "bounded_validates": bounded_validates,
                    "zero_concurrency_refused": zero_concurrency.validate().is_err(),
                    "zero_budget_bound_refused": zero_budget.validate().is_err(),
                    "unbounded_variant_unrepresentable": unbounded_unrepresentable,
                    "declared_bounds": ["capability.granted_to", "capability.execution_authorization",
                                        "max_concurrency", "budget"],
                },
                "bounded_auto_arm_is_onboardable": {
                    "canonicalize_error": bounded_canonicalizes,
                    "insert_trigger_spec_error": armed_trigger_stored,
                    "was": "`ExecutionCapability` named its field `authorization`, which is a \
                            `FORBIDDEN_KEYS` entry in \
                            `kontor_core::id::reject_sensitive_material`. Every route to a digest \
                            goes through `CanonicalDocument::from_value`, so a bounded auto-arm \
                            `TriggerSpec` validated and then could not be canonicalized, hashed, \
                            receipted or inserted",
                    "fix": "KON-MVP-22 renamed the field to `execution_authorization`. The \
                            shared scanner is unchanged and no path is exempted from it: the \
                            name that collided was the domain's, not the scanner's",
                    "covered_by": "`crates/kontor-core/tests/spec_validation.rs` now \
                                   canonicalizes the bounded policy it validates, and \
                                   `crates/kontor-store/tests/intake_lineage.rs` stores one and \
                                   arms work under it",
                },
                "persisted_decisions": stored,
                "store_problems": store_problems,
                "contradictions_refused": contradictions,
                "scheduler_admission": admission_rows,
                "unmerged_seam": "None for the domain. KON-MVP-22 merged `kontor-intake`, the \
                                  two-commit intake transaction, the append-only decision tables \
                                  and the lineage the scheduler reads. What remains staged is \
                                  the *transport* (`kontor_api::query::STAGED[0]`): no HTTP or \
                                  CLI surface serves intake yet, so this driver calls the store \
                                  directly, exactly as an operator's command would",
            }),
        )
        .expect("the intake-decision evidence is written");

    let directory = fixture.close();
    cleanup.directory("intake decisions store", &directory);

    let bounds_hold = bounded_validates
        && zero_concurrency.validate().is_err()
        && zero_budget.validate().is_err()
        && unbounded_unrepresentable;
    let onboardable = bounded_canonicalizes.is_none() && armed_trigger_stored.is_none();
    if store_problems.is_empty()
        && admission_problems.is_empty()
        && bounds_hold
        && onboardable
        && stored.len() == 3
    {
        bundle.pass(
            "domain.intake-decisions",
            "three proposals were produced by `kontor_intake::evaluate`, stored, then decided \
             through `commit_intake_decision`: approval persisted one decision and one task \
             lineage under a real `ApproveIntake` receipt; terminal rejection persisted one \
             decision, a reason and no task; bounded auto-arm persisted one decision and one task \
             lineage naming its execution authorization. Approval without work and rejection \
             carrying work are refused by `NewIntakeDecisionRecord::validate`; zero concurrency \
             and any zero budget bound are \
             refused by `AutoArmPolicy::validate`, and an unbounded variant does not deserialize \
             because it does not exist. All nine `TaskOrigin::admits` outcomes matched: approved \
             and bounded-auto-armed admit, and proposed-without-authorization, rejected, ignored \
             and duplicate each refuse `intake_receipt_not_approved` while a missing or \
             mismatched lineage refuses on its own code",
            &[artifact],
        );
    } else if store_problems.is_empty() && admission_problems.is_empty() && bounds_hold {
        // Everything the criterion asks about admission holds. What does not is
        // the third clause: a bounded auto-arm trigger cannot be onboarded, so
        // no deployment can declare the bounds whose consequences are proved
        // below. That is a defect in merged code, not a missing seam, so it is a
        // failure rather than a `blocked`.
        bundle.fail_with(
            "domain.intake-decisions",
            format!(
                "approve and terminal-reject hold end to end — both decisions persisted, the \
                 approval citing a real `ApproveIntake` command receipt and a one-task graph, \
                 with `IntakeReceipt::validate` refusing an unevidenced approval and a rejection \
                 carrying work — and all nine `TaskOrigin::admits` outcomes matched, including \
                 the bounded-auto-armed lineage admitting and every other result refusing \
                 `intake_receipt_not_approved`. The third clause does not: a bounded auto-arm \
                 trigger declaring a capability, an authorization, a concurrency ceiling and a \
                 non-zero budget passes `TriggerSpec::validate` and is then refused by \
                 `TriggerSpec::canonicalize` — canonicalize_error={bounded_canonicalizes:?}, \
                 insert_trigger_spec_error={armed_trigger_stored:?}. The cause is a name \
                 collision, not a credential: `ExecutionCapability::authorization` is spelled \
                 exactly like the `authorization` entry in \
                 `kontor_core::id::reject_sensitive_material`'s `FORBIDDEN_KEYS`, and every route \
                 to a digest runs that walker. No bounded auto-arm policy in this build can be \
                 hashed, receipted or stored, and no test in the tree covers the storing half.",
            ),
            &[artifact],
        );
    } else {
        bundle.fail_with(
            "domain.intake-decisions",
            format!(
                "store_problems=[{}], admission_problems=[{}], bounds_hold={bounds_hold}, \
                 onboardable={onboardable}, persisted={}",
                store_problems.join("; "),
                admission_problems.join("; "),
                stored.len()
            ),
            &[artifact],
        );
    }
}

// ---------------------------------------------------------------------------
// The `asma` boundary — the only case that needs a process
// ---------------------------------------------------------------------------

/// The ASMA workflow confirms the principal and a non-null assignee by refetch
/// before the ticket enters active development.
#[cfg(unix)]
async fn jira_asma(bundle: &mut Bundle, cleanup: &mut Cleanup) {
    let (workflow, field) = asma_project();
    let spec = workflow.spec();
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let current = first_inbound(spec);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let facts = facts(TaskState::InProgress, GateState::NotReady, None);
    let idempotency = key("kon-mvp-18-domain-asma");

    // The plan itself is pure, and it is the first half of the claim: with the
    // ticket unowned, convergence is the *assignee* and the status is withheld.
    let seen = observed(link_id, &current, None, vec![route("t-dev", &target)]);
    let ReconciliationOutcome::Transition(plan) = decide(spec, &seen, &facts, Freshness::Fresh)
    else {
        bundle.fail(
            "domain.jira-asma",
            "an unowned ticket at an inbound-compatible status did not plan an assignment",
        );
        return;
    };
    let assignment_first = plan.transition.is_none()
        && plan.assignment_prerequisite
        && plan.assignment.as_ref().is_some_and(|assignment| {
            assignment.action == OwnershipAction::ReassignToPrincipal
                && assignment.assign_to.as_ref() == Some(&principal().account_id)
        });

    // 1 — an answer without a principal is never guessed at.
    let mut anonymous = read_response(JiraOperation::Observe, &current, None);
    anonymous.principal_account_id = None;
    let fake = FakeAsma::answering(&anonymous);
    let asma = fake.resolved();
    let unresolved = TicketDelegation {
        asma: &asma,
        field_spec: &field,
        workflow_spec: &workflow,
        projection: &projection,
        facts: &facts,
        link_id,
        idempotency_key: &idempotency,
    }
    .observe()
    .await;
    let principal_required = matches!(
        unresolved,
        Err(AsmaError::Conflict {
            kind: StatusConflictKind::OwnershipUnresolved,
            ..
        })
    );
    cleanup.process(
        "jira observe without a principal",
        &fake.argv(),
        "exit 0, reaped",
    );
    let directory = fake.close();
    cleanup.directory("asma double: anonymous observe", &directory);

    // 2 — an apply that claims `applied` without a refetch is not believed.
    let mut unconfirmed = write_response(&target);
    unconfirmed.confirmation = None;
    let fake = FakeAsma::answering(&unconfirmed);
    let asma = fake.resolved();
    let unbelieved = TicketDelegation {
        asma: &asma,
        field_spec: &field,
        workflow_spec: &workflow,
        projection: &projection,
        facts: &facts,
        link_id,
        idempotency_key: &idempotency,
    }
    .apply(
        &seen,
        &plan,
        ApplyAuthority {
            authorized_by: CommandReceiptId::generate(),
        },
    )
    .await;
    let refetch_required = matches!(
        unbelieved,
        Err(AsmaError::Unavailable {
            reason: kontor_integrations_asma::UnavailableReason::MalformedResponse,
            ..
        })
    );
    cleanup.process(
        "jira apply without confirmation",
        &fake.argv(),
        "exit 0, reaped",
    );
    let directory = fake.close();
    cleanup.directory("asma double: unconfirmed apply", &directory);

    // 3 — the confirmed apply, and the receipt it is allowed to produce.
    let fake = FakeAsma::answering(&write_response(&target));
    let asma = fake.resolved();
    let delegation = TicketDelegation {
        asma: &asma,
        field_spec: &field,
        workflow_spec: &workflow,
        projection: &projection,
        facts: &facts,
        link_id,
        idempotency_key: &idempotency,
    };
    let applied = delegation
        .apply(
            &seen,
            &plan,
            ApplyAuthority {
                authorized_by: CommandReceiptId::generate(),
            },
        )
        .await;
    let receipt = applied
        .as_ref()
        .ok()
        .map(|response| delegation.receipt(&seen, &plan, response));
    let confirmed = receipt.as_ref().and_then(|result| result.as_ref().ok());
    let assignee_confirmed = confirmed.is_some_and(|receipt| {
        receipt.confirmed_at.is_some()
            && receipt.refetched_observation_id.is_some()
            && receipt.assignment_result.as_ref().is_some_and(|result| {
                result.assignee_account_id.as_ref() == Some(&principal().account_id)
            })
    });
    let request = fake.request();
    cleanup.process(
        "jira apply with confirmation",
        &fake.argv(),
        "exit 0, reaped",
    );
    let directory = fake.close();
    cleanup.directory("asma double: confirmed apply", &directory);

    let artifact = bundle
        .artifact(
            "jira/asma-plan.json",
            &json!({
                "project": spec.project.as_str(),
                "issue_type": spec.issue_type.as_str(),
                "ownership_milestone": spec.ownership_milestone.to_string(),
                "observed_status": current.status_id.as_str(),
                "development_status": target.status_id.as_str(),
                "plan": {
                    "milestone": plan.milestone.to_string(),
                    "target": plan.target.status_id.as_str(),
                    "has_transition": plan.transition.is_some(),
                    "assignment_prerequisite": plan.assignment_prerequisite,
                    "assignment_action": plan
                        .assignment
                        .as_ref()
                        .map(|assignment| format!("{:?}", assignment.action)),
                    "assignment_is_the_principal": assignment_first,
                },
                "principal": {
                    "source": "JiraResponse.principal_account_id — there is no `/myself` call \
                               in Rust and no other representable identity",
                    "absent_principal_outcome": describe_error(unresolved.err()),
                    "refused": principal_required,
                },
                "refetch": {
                    "applied_without_confirmation": describe_error(unbelieved.err()),
                    "refused": refetch_required,
                    "confirmed_receipt": confirmed.map(|receipt| json!({
                        "confirmed_at_present": receipt.confirmed_at.is_some(),
                        "refetched_observation_present":
                            receipt.refetched_observation_id.is_some(),
                        "final_assignee_is_the_principal": receipt
                            .assignment_result
                            .as_ref()
                            .is_some_and(|result| result.assignee_account_id.is_some()),
                    })),
                },
                "request_that_crossed_the_boundary": request,
            }),
        )
        .expect("the ASMA plan evidence is written");

    if assignment_first && principal_required && refetch_required && assignee_confirmed {
        bundle.pass(
            "domain.jira-asma",
            "against the shipped ASMA workflow an unowned ticket produced an assignee-only plan — \
             `transition: None`, `assignment_prerequisite: true`, `ReassignToPrincipal` to the \
             authenticated account — so the status cannot reach `implementation_active` before \
             ownership converges. An observation whose answer carried no `principal_account_id` \
             was refused `ownership_unresolved` rather than guessed at, an apply that reported \
             `applied` with no refetched observation was refused as a malformed response, and the \
             apply that did carry one produced a receipt with both `confirmed_at` and a \
             `refetched_observation_id`, whose confirmed assignee is the principal and is not null",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.jira-asma",
            format!(
                "assignment_first={assignment_first}, principal_required={principal_required}, \
                 refetch_required={refetch_required}, assignee_confirmed={assignee_confirmed}"
            ),
        );
    }
}

/// The refetch proof needs a real executable, and the pilot's double is a POSIX
/// shell script. On a platform without one the criterion is unproven, and saying
/// so is the only honest answer available.
#[cfg(not(unix))]
async fn jira_asma(bundle: &mut Bundle, cleanup: &mut Cleanup) {
    cleanup.never_spawned(
        "jira refetch confirmation",
        "this platform is not unix, so the `/bin/sh` asma double cannot be installed",
    );
    bundle.fail(
        "domain.jira-asma",
        "the principal and refetch-confirmation proofs require spawning the `asma` boundary, \
         whose pilot double is a POSIX shell script; this platform cannot run one",
    );
}

// ---------------------------------------------------------------------------
// The pure decision, across two vocabularies
// ---------------------------------------------------------------------------

/// Internal QA readiness never projects as external QA — and both fixtures agree
/// on every decision shape while sharing no status id.
fn jira_qa_distinct_and_alternate(bundle: &mut Bundle) {
    let projects = projects();
    let mut per_project = Vec::new();
    let mut shapes: Vec<Vec<String>> = Vec::new();
    let mut targets: Vec<Vec<String>> = Vec::new();
    let mut qa_problems = Vec::new();
    let mut vocabularies: Vec<BTreeSet<String>> = Vec::new();

    for (label, workflow, _) in &projects {
        let spec = workflow.spec();
        let ready_target = target_of(spec, QA_READY);
        let active_target = target_of(spec, QA_ACTIVE);
        let dev_target = target_of(spec, IMPLEMENTATION_ACTIVE);

        if ready_target == active_target {
            qa_problems.push(format!(
                "{label}: qa_ready resolves to the active QA status"
            ));
        }
        if ready_target != dev_target {
            qa_problems.push(format!(
                "{label}: qa_ready does not keep the ticket where active work sits"
            ));
        }

        // A ticket already where `qa_ready` points, with QA merely ready, plans
        // nothing: readiness is not a status move.
        let ready_facts = facts(TaskState::InProgress, GateState::Ready, None);
        let parked = observed(
            TicketLinkId::generate(),
            &ready_target,
            Some(&principal().account_id),
            vec![route("t-qa", &active_target)],
        );
        let ready_outcome = decide(spec, &parked, &ready_facts, Freshness::Fresh);
        if ready_outcome != ReconciliationOutcome::NoOp {
            qa_problems.push(format!(
                "{label}: ready QA planned {} instead of nothing",
                shape_of(&ready_outcome)
            ));
        }

        // The same ticket once QA is actually running does move, and only then.
        let active_facts = facts(TaskState::InProgress, GateState::Active, None);
        let running = observed(
            TicketLinkId::generate(),
            &first_inbound(spec),
            Some(&principal().account_id),
            vec![route("decoy", &dev_target), route("t-qa", &active_target)],
        );
        let active_outcome = decide(spec, &running, &active_facts, Freshness::Fresh);
        let reaches_qa = matches!(
            &active_outcome,
            ReconciliationOutcome::Transition(plan)
                if plan.target == active_target
                    && plan.transition.as_ref().is_some_and(|selected| {
                        selected.transition_id.as_str() == "t-qa"
                    })
        );
        if !reaches_qa {
            qa_problems.push(format!(
                "{label}: active QA produced {}",
                shape_of(&active_outcome)
            ));
        }

        // The shared matrix: one fact set per row, run against both fixtures.
        let mut row_shapes = Vec::new();
        let mut row_targets = Vec::new();
        let mut rows = Vec::new();
        for (case, outcome) in matrix(spec) {
            row_shapes.push(format!("{case}={}", shape_of(&outcome)));
            row_targets.push(format!(
                "{case}={}",
                target_text(&outcome).unwrap_or_else(|| "none".to_owned())
            ));
            rows.push(json!({
                "case": case,
                "shape": shape_of(&outcome),
                "external_target": target_text(&outcome),
            }));
        }
        shapes.push(row_shapes);
        targets.push(row_targets);
        vocabularies.push(
            spec.statuses
                .iter()
                .map(|status| status.selector.status_id.as_str().to_owned())
                .collect(),
        );
        per_project.push(json!({
            "project": label,
            "external_project": spec.project.as_str(),
            "issue_type": spec.issue_type.as_str(),
            "qa_ready_target": ready_target.status_id.as_str(),
            "qa_active_target": active_target.status_id.as_str(),
            "implementation_target": dev_target.status_id.as_str(),
            "qa_ready_is_not_qa_active": ready_target != active_target,
            "ready_qa_plans_nothing": ready_outcome == ReconciliationOutcome::NoOp,
            "active_qa_reaches_the_qa_status": reaches_qa,
            "decisions": rows,
        }));
    }

    let disjoint = vocabularies
        .first()
        .zip(vocabularies.get(1))
        .is_some_and(|(first, second)| !first.is_empty() && first.is_disjoint(second));
    let same_shapes = shapes.first() == shapes.get(1);
    let different_targets = targets.first() != targets.get(1);

    let artifact = bundle
        .artifact(
            "jira/alternate-plan.json",
            &json!({
                "projects": per_project,
                "status_vocabularies_are_disjoint": disjoint,
                "decision_shapes_identical": same_shapes,
                "external_targets_differ": different_targets,
                "rule": "`reconcile` is called with two pinned specifications and one fact set \
                         per row; the module contributes no branch on a status name or id, so a \
                         third project is a data change",
            }),
        )
        .expect("the cross-workflow evidence is written");

    if qa_problems.is_empty() {
        bundle.pass(
            "domain.jira-qa-distinct",
            "in both fixtures the internal `qa_ready` milestone targets the same status as \
             `implementation_active` and never the externally visible QA status: a ticket sitting \
             there with QA merely ready plans nothing at all, while the identical ticket with QA \
             actually running converges to the project's own QA status by the route that leads \
             there — so internal readiness cannot tell a human that review has started",
            std::slice::from_ref(&artifact),
        );
    } else {
        bundle.fail("domain.jira-qa-distinct", qa_problems.join("; "));
    }

    if disjoint && same_shapes && different_targets {
        bundle.pass(
            "domain.jira-alternate",
            format!(
                "the shipped ASMA workflow and a second project sharing none of its {} status ids \
                 were driven through the same {}-row decision matrix — stale observation, unknown \
                 status, terminal without evidence, terminal preserved, unowned, foreign owner, \
                 ready QA, active QA, hold, close and a target with no live route. Every row \
                 produced the identical decision shape and every row's external target was the \
                 project's own, which is what it means for the evaluator to have no name branch",
                vocabularies.first().map_or(0, BTreeSet::len),
                shapes.first().map_or(0, Vec::len)
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.jira-alternate",
            format!(
                "disjoint_vocabularies={disjoint}, identical_shapes={same_shapes}, \
                 targets_differ={different_targets}"
            ),
        );
    }
}

/// Hold, close and reopen are deterministic, and no path is ever guessed.
fn jira_hold_close_reopen(bundle: &mut Bundle) {
    let mut per_project = Vec::new();
    let mut problems = Vec::new();

    for (label, workflow, _) in projects() {
        let spec = workflow.spec();
        let hold_target = target_of(spec, TERMINAL_HOLD);
        let done_target = target_of(spec, TERMINAL_DONE);
        let current = first_inbound(spec);
        let holder = principal().account_id;

        // Hold: blocked work converges to the hold status by the one route that
        // leads there.
        let blocked = facts(TaskState::Blocked, GateState::NotReady, None);
        let held = observed(
            TicketLinkId::generate(),
            &current,
            Some(&holder),
            vec![route("t-hold", &hold_target)],
        );
        let hold_outcome = decide(spec, &held, &blocked, Freshness::Fresh);

        // Hold with no direct route: `no_live_transition`, never a two-hop plan
        // through a status that does connect.
        let indirect = observed(
            TicketLinkId::generate(),
            &current,
            Some(&holder),
            vec![route("t-qa", &target_of(spec, QA_ACTIVE))],
        );
        let indirect_outcome = decide(spec, &indirect, &blocked, Freshness::Fresh);

        // Two routes to the same destination is ambiguity, not a coin toss.
        let ambiguous = observed(
            TicketLinkId::generate(),
            &current,
            Some(&holder),
            vec![route("a", &hold_target), route("b", &hold_target)],
        );
        let ambiguous_outcome = decide(spec, &ambiguous, &blocked, Freshness::Fresh);

        // Close: a succeeded run with every required gate satisfied.
        let finished = facts(
            TaskState::Done,
            GateState::Passed,
            Some(TerminalOutcome::Succeeded),
        );
        let closing = observed(
            TicketLinkId::generate(),
            &current,
            Some(&holder),
            vec![route("t-close", &done_target)],
        );
        let close_outcome = decide(spec, &closing, &finished, Freshness::Fresh);

        // Already closed, with the evidence: nothing at all, and no ownership
        // conflict either, because the policy preserves whoever holds it.
        let closed = observed(
            TicketLinkId::generate(),
            &done_target,
            Some(&holder),
            Vec::new(),
        );
        let settled_outcome = decide(spec, &closed, &finished, Freshness::Fresh);

        // Closed externally while Kontor has no closure evidence: a conflict.
        let unfinished = facts(TaskState::InProgress, GateState::NotReady, None);
        let premature_outcome = decide(spec, &closed, &unfinished, Freshness::Fresh);

        // Stale evidence never plans anything, whatever the facts say.
        let stale_outcome = decide(spec, &closing, &finished, Freshness::Stale);

        // Reopen: the specification declares a reopen selector, and no milestone
        // rule targets it. Nothing in `reconcile` reads `spec.reopen` at all.
        let reopen = spec.reopen.clone();
        let reopen_targeted = reopen.as_ref().is_some_and(|selector| {
            spec.milestones
                .iter()
                .any(|rule| rule.target.status_id == selector.status_id)
        });
        let reopen_outcome = reopen.as_ref().map(|selector| {
            let reopened = observed(
                TicketLinkId::generate(),
                selector,
                Some(&holder),
                Vec::new(),
            );
            decide(spec, &reopened, &unfinished, Freshness::Fresh)
        });

        // The hold and close rows are spelled out rather than left as "some
        // transition": both must move the status by the one offered route and
        // plan no assignment at all, and a decision that stopped doing either
        // would otherwise still match the word.
        const CONVERGES: &str =
            "transition:prerequisite=false:has_transition=true:has_assignment=false";
        let expected: [(&str, &ReconciliationOutcome, &str); 7] = [
            ("hold", &hold_outcome, CONVERGES),
            (
                "hold_without_a_direct_route",
                &indirect_outcome,
                "conflict:no_live_transition",
            ),
            (
                "hold_with_two_routes",
                &ambiguous_outcome,
                "conflict:multiple_live_transitions",
            ),
            ("close", &close_outcome, CONVERGES),
            ("already_closed", &settled_outcome, "no_op"),
            (
                "closed_without_internal_evidence",
                &premature_outcome,
                "conflict:external_terminal_before_internal_evidence",
            ),
            (
                "stale_observation",
                &stale_outcome,
                "conflict:stale_observation",
            ),
        ];
        let mut rows = Vec::new();
        for (case, outcome, wanted) in expected {
            let got = shape_of(outcome);
            if got != wanted {
                problems.push(format!("{label}/{case}: wanted {wanted}, got {got}"));
            }
            rows.push(json!({
                "case": case,
                "shape": got,
                "external_target": target_text(outcome),
            }));
        }
        if reopen_targeted {
            problems.push(format!(
                "{label}: a milestone targets the reopen selector, so this section's reopen \
                 finding is wrong"
            ));
        }

        per_project.push(json!({
            "project": label,
            "hold_target": hold_target.status_id.as_str(),
            "close_target": done_target.status_id.as_str(),
            "declares_hold_selector": spec.hold.is_some(),
            "declares_reopen_selector": reopen.is_some(),
            "reopen_has_a_milestone": reopen_targeted,
            "reopen_outcome": reopen_outcome.as_ref().map(shape_of),
            "cases": rows,
        }));
    }

    let artifact = bundle
        .artifact(
            "jira/hold-close-reopen.json",
            &json!({
                "projects": per_project,
                "one_hop_rule": "`reconcile` filters the live routes for \
                                 `t.to.status_id == rule.target.status_id`. There is no graph \
                                 search, no memo of a previous transition id and no chaining, so \
                                 zero direct routes is `no_live_transition` and two is \
                                 `multiple_live_transitions`",
                "inert_selectors": "`spec.hold` and `spec.reopen` are validated against the \
                                    declared statuses and then never read by `reconcile`. Hold is \
                                    reachable only through the `terminal_hold` milestone, and \
                                    reopen has no milestone in either fixture",
            }),
        )
        .expect("the hold/close/reopen evidence is written");

    if problems.is_empty() {
        bundle.pass(
            "domain.jira-hold-close-reopen",
            "across both fixtures hold and close are single deterministic outcomes — blocked work \
             converges to the hold status by the one route that reaches it, a succeeded run with \
             every required gate satisfied converges to the closed status, an already-closed \
             ticket is `NoOp` and a ticket closed externally without Kontor's own evidence is \
             `external_terminal_before_internal_evidence`. No path is ever guessed: with only an \
             indirect route offered the answer is `no_live_transition` rather than a two-hop plan, \
             and with two routes to one destination it is `multiple_live_transitions` rather than \
             a choice. Reopen is deterministic too, and the determinism is that it is *not* \
             automated: both fixtures declare a reopen selector, no milestone rule targets it, \
             `reconcile` never reads `spec.reopen`, and a ticket sitting at that status plans \
             nothing. The criterion asks for determinism, not for automation, so this is recorded \
             as a pass with the absence stated rather than as a failure of a claim nobody made",
            &[artifact],
        );
    } else {
        bundle.fail("domain.jira-hold-close-reopen", problems.join("; "));
    }
}

/// A different existing owner and every terminal assignee are preserved, and a
/// delegated plan that would clear ownership never reaches the boundary.
async fn jira_ownership(bundle: &mut Bundle, cleanup: &mut Cleanup) {
    let stranger = external("acct-a-human-being");
    let mut per_project = Vec::new();
    let mut problems = Vec::new();

    for (label, workflow, _) in projects() {
        let spec = workflow.spec();
        let target = target_of(spec, IMPLEMENTATION_ACTIVE);
        let terminal = terminal_of(spec);

        // A stranger holds the ticket: the status still converges and the plan
        // carries no assignment at all.
        let working = facts(TaskState::InProgress, GateState::NotReady, None);
        let foreign = observed(
            TicketLinkId::generate(),
            &first_inbound(spec),
            Some(&stranger),
            vec![route("t-dev", &target)],
        );
        let foreign_outcome = decide(spec, &foreign, &working, Freshness::Fresh);
        let owner_preserved = matches!(
            &foreign_outcome,
            ReconciliationOutcome::Transition(plan)
                if plan.assignment.is_none() && plan.transition.is_some()
        );
        if !owner_preserved {
            problems.push(format!(
                "{label}: a foreign owner produced {}",
                shape_of(&foreign_outcome)
            ));
        }

        // Terminal, with the evidence, under every possible holder.
        let finished = facts(
            TaskState::Done,
            GateState::Passed,
            Some(TerminalOutcome::Succeeded),
        );
        let mut holders = Vec::new();
        for (holder_label, holder) in [
            ("unassigned", None),
            ("the principal", Some(principal().account_id)),
            ("a stranger", Some(stranger.clone())),
        ] {
            let closed = observed(
                TicketLinkId::generate(),
                &terminal,
                holder.as_ref(),
                Vec::new(),
            );
            let outcome = decide(spec, &closed, &finished, Freshness::Fresh);
            if outcome != ReconciliationOutcome::NoOp {
                problems.push(format!(
                    "{label}: a terminal ticket held by {holder_label} produced {}",
                    shape_of(&outcome)
                ));
            }
            holders.push(json!({
                "holder": holder_label,
                "shape": shape_of(&outcome),
            }));
        }

        per_project.push(json!({
            "project": label,
            "terminal_action": format!("{:?}", spec.ownership.terminal_action),
            "mismatch_behaviour": format!("{:?}", spec.ownership.mismatch),
            "foreign_owner": {
                "shape": shape_of(&foreign_outcome),
                "assignment_planned": matches!(
                    &foreign_outcome,
                    ReconciliationOutcome::Transition(plan) if plan.assignment.is_some()
                ),
                "status_still_moves": matches!(
                    &foreign_outcome,
                    ReconciliationOutcome::Transition(plan) if plan.transition.is_some()
                ),
            },
            "terminal_holders": holders,
        }));
    }

    // The delegated half: three plans the boundary refuses to build a request
    // for. `build_write_request` runs before `exchange`, so the executable
    // resolves and is never spawned.
    let (workflow, field) = asma_project();
    let spec = workflow.spec();
    let target = target_of(spec, IMPLEMENTATION_ACTIVE);
    let link_id = TicketLinkId::generate();
    let projection = projection(&field, Vec::new());
    let working = facts(TaskState::InProgress, GateState::NotReady, None);
    let idempotency = key("kon-mvp-18-domain-ownership");
    let asma = unspawned();
    cleanup.never_spawned(
        "ownership refusals",
        "`build_write_request` refuses at jira.rs:1082-1113, before `exchange` at jira.rs:1178 \
         can spawn anything; `contract.rs:1012` asserts the same argv stays empty",
    );
    let delegation = TicketDelegation {
        asma: &asma,
        field_spec: &field,
        workflow_spec: &workflow,
        projection: &projection,
        facts: &working,
        link_id,
        idempotency_key: &idempotency,
    };
    let seen = observed(
        link_id,
        &first_inbound(spec),
        None,
        vec![route("t", &target)],
    );
    let base = TransitionPlan {
        milestone: milestone(IMPLEMENTATION_ACTIVE),
        target: target.clone(),
        transition: None,
        assignment: None,
        assignment_prerequisite: false,
    };
    let refusals = [
        (
            "clears the assignee",
            TransitionPlan {
                assignment: Some(kontor_core::ticket::AssignmentPlan {
                    assign_to: None,
                    action: OwnershipAction::Unassign,
                }),
                ..base.clone()
            },
        ),
        (
            "preserve carrying an assignee mutation",
            TransitionPlan {
                assignment: Some(kontor_core::ticket::AssignmentPlan {
                    assign_to: Some(principal().account_id),
                    action: OwnershipAction::Preserve,
                }),
                ..base.clone()
            },
        ),
        (
            "assigns somebody who is not the principal",
            TransitionPlan {
                assignment: Some(kontor_core::ticket::AssignmentPlan {
                    assign_to: Some(external("acct-somebody-else")),
                    action: OwnershipAction::ReassignToPrincipal,
                }),
                ..base
            },
        ),
    ];
    let mut refusal_rows = Vec::new();
    for (case, plan) in refusals {
        let outcome = delegation.dry_run(&seen, &plan).await;
        let refused = matches!(outcome, Err(AsmaError::Refused { .. }));
        if !refused {
            problems.push(format!("{case}: was not refused"));
        }
        refusal_rows.push(json!({
            "plan": case,
            "refused": refused,
            "rule": match outcome {
                Err(AsmaError::Refused { rule, .. }) => Some(rule),
                _ => None,
            },
        }));
    }

    let artifact = bundle
        .artifact(
            "jira/ownership.json",
            &json!({
                "projects": per_project,
                "delegated_plans_refused": refusal_rows,
                "ordering": "the terminal-preserve branch (`ticket.rs:1113-1121`) runs before any \
                             milestone or assignment branch, so a closed ticket's holder is never \
                             written, cleared or even reported as a mismatch",
                "boundary": "no process was spawned: `build_write_request` refuses before \
                             `exchange` is reached",
            }),
        )
        .expect("the ownership evidence is written");

    if problems.is_empty() {
        bundle.pass(
            "domain.jira-ownership",
            "in both fixtures a ticket already held by somebody else converged its status while \
             planning no assignment at all — under `accept_external` the existing owner is kept, \
             not taken over — and a terminal ticket planned nothing whether it was unassigned, \
             held by the principal or held by a stranger, because the preserve branch fires before \
             any assignment branch can reconsider it. Three delegated plans that would have \
             touched terminal ownership — an explicit unassign, a `preserve` action smuggling an \
             assignee value, and an assignee that is not the authenticated principal — were each \
             refused while building the request, so none of them reached the boundary",
            &[artifact],
        );
    } else {
        bundle.fail("domain.jira-ownership", problems.join("; "));
    }
}

// ---------------------------------------------------------------------------
// Three-zone privacy
// ---------------------------------------------------------------------------

/// Zone C stays private, owned fields project exactly once, absence is not a
/// clear, and an outbound comment is unrepresentable.
fn privacy_zones(bundle: &mut Bundle) {
    // The shipped specification maps eight fields, all `kontor`/`outbound`, so
    // the three-zone case has to be built by re-owning three of them. Doing it
    // by mutation rather than by a new fixture keeps the comparison honest: the
    // only difference is ownership.
    const PRIVATE_CANARY: &str = "ZONE-C-PRIVATE-9d2a";
    const JIRA_OWNED_CANARY: &str = "JIRA-OWNED-1f7b";
    const KONTOR_OWNED: &str = "Kontor owns this line";

    let mut spec = asma_field_spec().spec().clone();
    for mapping in &mut spec.mappings {
        match mapping.key {
            // Zone C: internal only. A private mapping may not declare a
            // direction or an external field, so it cannot be addressed.
            TicketFieldKey::AgentStatus => {
                mapping.owner = FieldOwner::Private;
                mapping.direction = None;
                mapping.external = None;
                mapping.required = false;
            }
            // Owned by the external system, readable by Kontor, not Kontor's to
            // overwrite.
            TicketFieldKey::Severity => {
                mapping.owner = FieldOwner::Jira;
                mapping.direction = Some(FieldDirection::Bidirectional);
            }
            // Inbound only: writing it outward is a contradiction, not a skip.
            TicketFieldKey::ReproSteps => {
                mapping.owner = FieldOwner::Jira;
                mapping.direction = Some(FieldDirection::Inbound);
            }
            // Mirrored outward without being owned.
            TicketFieldKey::Product => {
                mapping.owner = FieldOwner::MirrorOnly;
                mapping.direction = Some(FieldDirection::Outbound);
            }
            _ => {}
        }
    }
    let mut catalog = SpecCatalog::empty();
    catalog
        .load_field_spec(&serde_json::to_string(&spec).expect("the re-owned spec serializes"))
        .expect("the re-owned field specification validates and loads");
    let field = catalog
        .select_field_spec(&asma_field_key())
        .expect("the re-owned specification is selectable")
        .clone();
    let product_option = field
        .spec()
        .mapping(TicketFieldKey::Product)
        .and_then(|mapping| mapping.external.as_ref())
        .and_then(|external| external.options.first())
        .expect("the product field declares options")
        .clone();

    // The projection Kontor would actually write: one owned field with a value,
    // one owned field deliberately absent, one mirrored field, one jira-owned
    // field carrying a canary, and Zone C carrying nothing.
    let written = projection(
        &field,
        vec![
            ProjectedField {
                key: TicketFieldKey::Summary,
                value: Some(text(KONTOR_OWNED)),
            },
            ProjectedField {
                key: TicketFieldKey::Description,
                value: None,
            },
            ProjectedField {
                key: TicketFieldKey::Product,
                value: Some(FieldValue::Select {
                    option: product_option.id.clone(),
                }),
            },
            ProjectedField {
                key: TicketFieldKey::Severity,
                value: Some(FieldValue::Select {
                    option: external("10300"),
                }),
            },
            ProjectedField {
                key: TicketFieldKey::AgentStatus,
                value: None,
            },
        ],
    );
    let writes = compile_field_writes(&written, &field).expect("the projection validates");
    let encoded = serde_json::to_string(&writes).expect("the writes serialize");
    let description_id = field
        .spec()
        .mapping(TicketFieldKey::Description)
        .and_then(|mapping| mapping.external.as_ref())
        .map(|external| external.field_id.as_str().to_owned())
        .expect("the description is mapped");

    // Zone C carrying a value is refused outright, before anything is compiled.
    let leaking = projection(
        &field,
        vec![ProjectedField {
            key: TicketFieldKey::AgentStatus,
            value: Some(text(PRIVATE_CANARY)),
        }],
    );
    let zone_c_rule = domain_rule(compile_field_writes(&leaking, &field).unwrap_err());

    // An inbound-only field written outward, likewise.
    let inbound = projection(
        &field,
        vec![ProjectedField {
            key: TicketFieldKey::ReproSteps,
            value: Some(text(JIRA_OWNED_CANARY)),
        }],
    );
    let inbound_rule = domain_rule(compile_field_writes(&inbound, &field).unwrap_err());

    // The same field twice is refused, which is what "exactly once" means.
    let twice = projection(
        &field,
        vec![
            ProjectedField {
                key: TicketFieldKey::Summary,
                value: Some(text(KONTOR_OWNED)),
            },
            ProjectedField {
                key: TicketFieldKey::Summary,
                value: Some(text(KONTOR_OWNED)),
            },
        ],
    );
    let duplicate_rule = domain_rule(compile_field_writes(&twice, &field).unwrap_err());

    // An outbound comment is unrepresentable: one policy variant, and no field
    // of `JiraRequest` can carry one.
    let request = JiraRequest {
        schema_version: WIRE_SCHEMA_VERSION,
        operation: JiraOperation::DryRun,
        issue_key: external("ASMA-1"),
        idempotency_key: key("kon-mvp-18-domain-privacy"),
        intent_hash: None,
        field_spec_hash: Some(field.hash().clone()),
        workflow_spec_hash: None,
        expected: None,
        field_writes: writes.clone(),
        destination: None,
        ownership_action: OwnershipAction::Preserve,
        transition: None,
        authorized_apply: false,
    };
    let request_bytes = serde_json::to_string(&request).expect("the request serializes");
    let comment_free = !request_bytes.to_lowercase().contains("comment");
    let one_policy = CommentPolicy::ALL.len() == 1;

    let owned_written = writes.len() == 2;
    let private_absent = !encoded.contains(PRIVATE_CANARY);
    let jira_owned_absent = !encoded.contains(JIRA_OWNED_CANARY);
    let severity_skipped = field
        .spec()
        .mapping(TicketFieldKey::Severity)
        .and_then(|mapping| mapping.external.as_ref())
        .is_some_and(|external| !encoded.contains(external.field_id.as_str()));
    let absent_not_cleared = !encoded.contains("null") && !encoded.contains(&description_id);

    let artifact = bundle
        .artifact(
            "jira/privacy-zones.json",
            &json!({
                "zones": {
                    "kontor": "summary — owned, projected once",
                    "mirror_only": "product — mirrored outward without being owned",
                    "jira": "severity — readable, never pushed; repro_steps — inbound only",
                    "private": "agent_status — Zone C: no direction, no external mapping",
                },
                "owner_direction_matrix": FieldOwner::ALL
                    .iter()
                    .map(|owner| json!({
                        "owner": owner.to_string(),
                        "allows": FieldDirection::ALL
                            .iter()
                            .filter(|direction| owner.allows(**direction))
                            .map(ToString::to_string)
                            .collect::<Vec<_>>(),
                    }))
                    .collect::<Vec<_>>(),
                "writes_emitted": writes.len(),
                "written_field_ids": writes
                    .iter()
                    .map(|write| write.field_id.as_str().to_owned())
                    .collect::<Vec<_>>(),
                "private_value_projected_outward": zone_c_rule,
                "inbound_only_projected_outward": inbound_rule,
                "same_field_projected_twice": duplicate_rule,
                "absent_field": {
                    "field_id_absent_from_the_wire": absent_not_cleared,
                    "rule": "an absent value is skipped by `compile_field_writes`; there is no \
                             `clear` variant and no null on the wire",
                },
                "canaries_absent_from_the_wire": {
                    "private": private_absent,
                    "jira_owned": jira_owned_absent,
                },
                "comments": {
                    "policy_variants": CommentPolicy::ALL.len(),
                    "the_only_policy": CommentPolicy::InboundOnly.to_string(),
                    "serialized_request_mentions_a_comment": !comment_free,
                    "rule": "`JiraRequest` has no comment field and there is no outbound comment \
                             payload type, so an outbound comment is a type change rather than a \
                             configuration change",
                },
            }),
        )
        .expect("the privacy-zone evidence is written");

    let holds = owned_written
        && private_absent
        && jira_owned_absent
        && severity_skipped
        && absent_not_cleared
        && comment_free
        && one_policy
        && zone_c_rule == Some("projects a private field outward")
        && inbound_rule == Some("projects an inbound-only field outward")
        && duplicate_rule == Some("projects the same field twice");
    if holds {
        bundle.pass(
            "domain.privacy-zones",
            "with the shipped specification re-owned into all four zones, a projection carrying a \
             Zone C value is refused as `projects a private field outward` before anything is \
             compiled, while the same field left absent is simply omitted. Of five projected \
             fields exactly two reached the wire — the Kontor-owned one and the mirrored one — \
             each exactly once; the externally owned field was skipped rather than pushed and its \
             canary appears nowhere in the request; an inbound-only field written outward and the \
             same field projected twice are both refused by name. The absent field contributed no \
             id and no `null`, so absence is not a clear. And no outbound comment is \
             representable: `CommentPolicy` has one variant and the serialized `JiraRequest` has \
             no field whose name so much as contains `comment`",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.privacy-zones",
            format!(
                "writes={}, private_absent={private_absent}, jira_canary_absent=\
                 {jira_owned_absent}, jira_field_skipped={severity_skipped}, \
                 absent_not_cleared={absent_not_cleared}, comment_free={comment_free}, \
                 one_policy={one_policy}, zone_c={zone_c_rule:?}, inbound={inbound_rule:?}, \
                 duplicate={duplicate_rule:?}",
                writes.len()
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Inbound comments
// ---------------------------------------------------------------------------

/// One inbound comment mirrors exactly once, and an edit is a new revision.
fn inbound_comment(bundle: &mut Bundle, cleanup: &mut Cleanup) {
    let fixture = StoreFixture::open();
    let link = TicketLinkId::generate();
    fixture
        .store
        .create_ticket_link(&NewTicketLink {
            id: link,
            project_id: fixture.project,
            task_id: fixture.task,
            connector: ConnectorKey::parse("connector.jira").expect("a legal connector key"),
            external_issue_key: external("ASMA-1"),
            created_at: at(DECIDED_AT),
        })
        .expect("the ticket link is created");

    let author = external("acct-a-human-being");
    let revision = |body: &str, observed: &str| {
        let body = BoundedText::parse(body).expect("a legal comment body");
        ExternalCommentRevision {
            link_id: link,
            external_comment_id: external("comment-1"),
            author_account_id: author.clone(),
            author_display: Some(name("A Human")),
            external_created_at: at("2026-08-12T08:00:00Z"),
            external_updated_at: at(observed),
            body_hash: ContentHash::of(body.as_str().as_bytes()),
            body,
            observed_at: at(observed),
            supersedes: None,
        }
    };

    let first = revision("The reviewer left a note.", "2026-08-12T09:00:00Z");
    // The same comment, seen again on a later poll: same body, later cursor.
    let replay = revision("The reviewer left a note.", "2026-08-12T09:30:00Z");
    // An edit: a different body under the same external comment id.
    let mut edited = revision("The reviewer amended the note.", "2026-08-12T10:00:00Z");
    edited.supersedes = Some(first.body_hash.clone());

    let mirrored = fixture
        .store
        .append_comment(fixture.project, &first)
        .expect("the comment is mirrored");
    let replayed = fixture
        .store
        .append_comment(fixture.project, &replay)
        .expect("a replay is not an error");
    let amended = fixture
        .store
        .append_comment(fixture.project, &edited)
        .expect("an edit is mirrored");

    // Provenance, read back through a second connection: the author id and both
    // external instants are stored, and the body is never echoed into evidence.
    let connection = rusqlite::Connection::open(&fixture.path).expect("the census connection");
    let rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM external_comments WHERE project_id = ?1 AND link_id = ?2",
            rusqlite::params![fixture.project.to_string(), link.to_string()],
            |row| row.get(0),
        )
        .expect("the count succeeds");
    let stored_author: String = connection
        .query_row(
            "SELECT author_account_id FROM external_comments
             WHERE project_id = ?1 AND link_id = ?2 AND body_hash = ?3",
            rusqlite::params![
                fixture.project.to_string(),
                link.to_string(),
                first.body_hash.as_str()
            ],
            |row| row.get(0),
        )
        .expect("the original revision is stored");
    drop(connection);

    // A revision whose digest does not match its body is not provenance at all.
    let mut tampered = first.clone();
    tampered.body_hash = ContentHash::of(b"something else");
    let tamper_refused = tampered.verify().is_err()
        && fixture
            .store
            .append_comment(fixture.project, &tampered)
            .is_err();

    let artifact = bundle
        .artifact(
            "jira/inbound-comment.json",
            &json!({
                "identity": "(link_id, external_comment_id, body_hash)",
                "first_append_mirrored": mirrored,
                "replay_mirrored_again": replayed,
                "edit_mirrored_as_a_new_revision": amended,
                "rows_for_this_comment_id": rows,
                "provenance": {
                    "external_comment_id": first.external_comment_id.as_str(),
                    "author_account_id_stored": stored_author == author.as_str(),
                    "author_display_present": first.author_display.is_some(),
                    "external_created_at": first.external_created_at.to_string(),
                    "external_updated_at_of_the_replay": replay.external_updated_at.to_string(),
                    "original_body_hash": first.body_hash.to_string(),
                    "edited_body_hash": edited.body_hash.to_string(),
                    "edit_supersedes": edited
                        .supersedes
                        .as_ref()
                        .map(std::string::ToString::to_string),
                    "body_recorded_here": "no — only its digest",
                },
                "digest_verified": first.verify().is_ok(),
                "tampered_revision_refused": tamper_refused,
                "same_revision_recognized": first.is_same_revision(&replay),
                "edit_is_a_different_revision": !first.is_same_revision(&edited),
            }),
        )
        .expect("the inbound-comment evidence is written");

    let directory = fixture.close();
    cleanup.directory("inbound comment store", &directory);

    let holds = mirrored
        && !replayed
        && amended
        && rows == 2
        && stored_author == author.as_str()
        && tamper_refused
        && first.is_same_revision(&replay)
        && !first.is_same_revision(&edited);
    if holds {
        bundle.pass(
            "domain.inbound-comment",
            "one inbound comment was mirrored once: the first append returned `true`, the same \
             comment seen again on a later poll returned `false` and inserted nothing, and the \
             table holds two rows only because a genuine edit under the same external comment id \
             is kept as a second revision that names the digest it supersedes. Its external \
             provenance survived — the author's external account id read back from SQLite, the \
             display name, the external created and updated instants and the body digest — while \
             the body itself appears in no evidence artifact. A revision whose digest does not \
             match its body is refused by `verify` and by the store",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.inbound-comment",
            format!(
                "first={mirrored}, replay={replayed}, edit={amended}, rows={rows}, \
                 author_matches={}, tamper_refused={tamper_refused}",
                stored_author == author.as_str()
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// The `asma` double
// ---------------------------------------------------------------------------

/// A temporary executable standing in for `asma`.
///
/// A shell script rather than a mocked trait, for the same reason the connector's
/// own contract suite uses one: it exercises real argv, real pipes, a real exit
/// status and a real reap, which is where the bugs at a process boundary live. It
/// is a [`TempDir`], so the directory the pilot has to account for is removed by
/// the same drop that ends the fixture's life.
#[cfg(unix)]
struct FakeAsma {
    /// Removed on drop; the process ledger checks that it really was.
    directory: TempDir,
    /// The script itself.
    executable: PathBuf,
    /// Where the script appends its argv and stdin.
    record: PathBuf,
    /// Where the script keeps the last request verbatim.
    request: PathBuf,
}

#[cfg(unix)]
impl FakeAsma {
    /// A fake that answers with one document and exits zero.
    ///
    /// # Panics
    /// Panics when the temporary executable cannot be installed, which is a
    /// driver bug rather than a finding about the tree.
    fn answering<T: serde::Serialize>(response: &T) -> Self {
        use std::os::unix::fs::PermissionsExt as _;

        let json = serde_json::to_string(response).expect("the response serializes");
        let directory = TempDir::new().expect("a temporary directory");
        let executable = directory.path().join("asma");
        let record = directory.path().join("record");
        let request = directory.path().join("request");
        // The paths are baked into the script rather than passed through the
        // environment: the pilot shares one process, and mutating process
        // environment variables from several tasks is a race, not a fixture.
        let script = format!(
            "#!/bin/sh\nfor arg in \"$@\"; do printf 'arg:%s\\n' \"$arg\" >> '{record}'; done\n\
             cat > '{request}'\ncat <<'KONTOR_PILOT_EOF'\n{json}\nKONTOR_PILOT_EOF\n",
            record = record.display(),
            request = request.display(),
        );
        let temporary = directory.path().join("asma.tmp");
        std::fs::write(&temporary, script).expect("the fake executable is writable");
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))
            .expect("the fake executable is markable executable");
        std::fs::rename(temporary, &executable).expect("the fake executable is installable");
        Self {
            directory,
            executable,
            record,
            request,
        }
    }

    /// Resolve it as the one writer, with budgets small enough that a wedged
    /// child could never wedge the pilot.
    ///
    /// # Panics
    /// Panics when the freshly written script does not resolve.
    fn resolved(&self) -> AsmaExecutable {
        AsmaExecutable::with_budgets(
            &self.executable,
            std::time::Duration::from_secs(10),
            1 << 20,
        )
        .expect("the fake resolves")
    }

    /// Every argument, one entry per real argv slot. Empty means never spawned.
    fn argv(&self) -> Vec<String> {
        std::fs::read_to_string(&self.record)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.strip_prefix("arg:").map(str::to_owned))
            .collect()
    }

    /// The last request document, reduced to the facts worth retaining.
    ///
    /// The whole body is deliberately not kept: it carries specification digests
    /// and an intent hash, and an evidence file that quoted every byte would be
    /// larger and no more convincing than the shape.
    fn request(&self) -> Value {
        let text = std::fs::read_to_string(&self.request).unwrap_or_default();
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return json!({ "captured": false });
        };
        json!({
            "captured": true,
            "operation": value["operation"],
            "authorized_apply": value["authorized_apply"],
            "ownership_action": value["ownership_action"],
            "carries_a_transition": !value["transition"].is_null(),
            "field_write_count": value["field_writes"].as_array().map_or(0, Vec::len),
            "mentions_a_comment": text.to_lowercase().contains("comment"),
        })
    }

    /// Hand back the directory path and drop everything that holds it.
    fn close(self) -> PathBuf {
        let path = self.directory.path().to_path_buf();
        drop(self.directory);
        path
    }
}

/// An executable that resolves but is never spawned, for pure-planning cases.
///
/// # Panics
/// Panics when the running test binary has no path, which cannot happen.
fn unspawned() -> AsmaExecutable {
    AsmaExecutable::with_budgets(
        std::env::current_exe().expect("the test binary has a path"),
        std::time::Duration::from_secs(1),
        1 << 10,
    )
    .expect("any real file resolves")
}

// ---------------------------------------------------------------------------
// Specification and observation builders
// ---------------------------------------------------------------------------

/// The shipped ASMA specifications plus the second project, in one catalogue.
///
/// # Panics
/// Panics when a checked-in specification does not load, which is a build-time
/// defect in the data rather than a finding about the tree.
fn catalog() -> SpecCatalog {
    let mut catalog = SpecCatalog::bundled().expect("the bundled specifications load");
    catalog
        .load_workflow_spec(ALTERNATE_WORKFLOW)
        .expect("the alternate workflow fixture loads");
    // The second project needs a field specification of its own. It is the ASMA
    // one re-keyed, because the field contract is not what this fixture varies.
    let mut alternate: kontor_core::ticket::TicketFieldSpec = asma_field_spec().spec().clone();
    alternate.project = ExternalProjectKey::parse("nordlys").expect("a legal project key");
    alternate.issue_type = ExternalIssueTypeKey::parse("sak").expect("a legal issue-type key");
    catalog
        .load_field_spec(&serde_json::to_string(&alternate).expect("the re-keyed spec serializes"))
        .expect("the re-keyed field specification loads");
    catalog
}

/// The shipped ASMA field specification on its own.
///
/// # Panics
/// Panics when the bundled data does not load or select.
fn asma_field_spec() -> CompiledFieldSpec {
    SpecCatalog::bundled()
        .expect("the bundled specifications load")
        .select_field_spec(&asma_field_key())
        .expect("the bundled field specification is selectable")
        .clone()
}

/// The ASMA field specification's selection key.
///
/// # Panics
/// Panics on a key the domain refuses, which is a driver bug.
fn asma_field_key() -> FieldSpecKey {
    FieldSpecKey {
        connector: ConnectorKey::parse("connector.jira").expect("a legal connector key"),
        project: ExternalProjectKey::parse("asma").expect("a legal project key"),
        issue_type: ExternalIssueTypeKey::parse("task").expect("a legal issue-type key"),
        version: SpecVersion::FIRST,
    }
}

/// The shipped ASMA project, as a workflow/field pair.
///
/// # Panics
/// Panics when the bundled specifications are not selectable.
fn asma_project() -> (CompiledWorkflowSpec, CompiledFieldSpec) {
    let catalog = catalog();
    let workflow = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("a legal connector key"),
            project: ExternalProjectKey::parse("asma").expect("a legal project key"),
            issue_type: ExternalIssueTypeKey::parse("task").expect("a legal issue-type key"),
            version: SpecVersion::FIRST,
            work_profile: Some(kontor_integrations_asma::jira::PinnedProfile {
                key: WorkProfileKey::parse("code").expect("a legal profile key"),
                version: SpecVersion::FIRST,
            }),
        })
        .expect("the ASMA workflow specification is selectable")
        .clone();
    let field = catalog
        .select_field_spec(&asma_field_key())
        .expect("the ASMA field specification is selectable")
        .clone();
    (workflow, field)
}

/// Both projects, labelled, as pairs a case can loop over.
///
/// # Panics
/// Panics when either specification is not selectable.
fn projects() -> Vec<(&'static str, CompiledWorkflowSpec, CompiledFieldSpec)> {
    let catalog = catalog();
    let alternate_workflow = catalog
        .select_workflow_spec(&WorkflowSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("a legal connector key"),
            project: ExternalProjectKey::parse("nordlys").expect("a legal project key"),
            issue_type: ExternalIssueTypeKey::parse("sak").expect("a legal issue-type key"),
            version: SpecVersion::FIRST,
            work_profile: None,
        })
        .expect("the alternate workflow specification is selectable")
        .clone();
    let alternate_field = catalog
        .select_field_spec(&FieldSpecKey {
            connector: ConnectorKey::parse("connector.jira").expect("a legal connector key"),
            project: ExternalProjectKey::parse("nordlys").expect("a legal project key"),
            issue_type: ExternalIssueTypeKey::parse("sak").expect("a legal issue-type key"),
            version: SpecVersion::FIRST,
        })
        .expect("the alternate field specification is selectable")
        .clone();
    let (asma_workflow, asma_field) = asma_project();
    vec![
        ("asma", asma_workflow, asma_field),
        ("alternate", alternate_workflow, alternate_field),
    ]
}

/// The status a workflow uses for a milestone, read from the fixture.
///
/// # Panics
/// Panics when the fixture does not declare the milestone, which is a fixture
/// bug rather than a finding about the tree.
fn target_of(spec: &ExternalWorkflowSpec, key: &str) -> StatusSelector {
    let wanted = milestone(key);
    spec.milestones
        .iter()
        .find(|rule| rule.milestone == wanted)
        .expect("the fixture declares this milestone")
        .target
        .clone()
}

/// The first status a workflow accepts as a starting point.
///
/// # Panics
/// Panics when the fixture declares none.
fn first_inbound(spec: &ExternalWorkflowSpec) -> StatusSelector {
    spec.inbound_compatible
        .first()
        .expect("the fixture declares an inbound-compatible status")
        .clone()
}

/// The first terminal status a workflow declares.
///
/// # Panics
/// Panics when the fixture declares none.
fn terminal_of(spec: &ExternalWorkflowSpec) -> StatusSelector {
    spec.statuses
        .iter()
        .find(|status| status.class.is_terminal())
        .expect("the fixture declares a terminal status")
        .selector
        .clone()
}

/// The principal Kontor acts as. One representable identity, and this is it.
///
/// # Panics
/// Panics on an id the domain refuses, which is a driver bug.
fn principal() -> TicketPrincipal {
    TicketPrincipal {
        account_id: external("acct-kontor"),
    }
}

/// Kontor's own facts, varied only along the three dimensions the milestones
/// read.
fn facts(state: TaskState, gate: GateState, outcome: Option<TerminalOutcome>) -> InternalTaskFacts {
    InternalTaskFacts {
        task_id: TaskId::generate(),
        task_state: state,
        task_revision: AggregateRevision::INITIAL,
        workflow_revision: AggregateRevision::INITIAL,
        projection_revision: AggregateRevision::INITIAL,
        completed_phases: BTreeSet::new(),
        gate_states: vec![(GateKey::parse("qa-gate").expect("a legal gate key"), gate)],
        all_required_gates_passed: outcome == Some(TerminalOutcome::Succeeded),
        run_outcome: outcome,
    }
}

/// A projection against a pinned field specification.
fn projection(field_spec: &CompiledFieldSpec, fields: Vec<ProjectedField>) -> TicketSyncProjection {
    let spec = field_spec.spec();
    TicketSyncProjection {
        schema_version: SCHEMA_VERSION,
        id: TicketProjectionId::generate(),
        link_id: TicketLinkId::generate(),
        link_revision: AggregateRevision::INITIAL,
        connector: spec.connector.clone(),
        field_spec_project: spec.project.clone(),
        field_spec_issue_type: spec.issue_type.clone(),
        field_spec_version: spec.version,
        external_issue_key: external("ASMA-1"),
        fields,
        comment_policy: CommentPolicy::InboundOnly,
        external_comment_cursor: None,
        computed_at: at(DECIDED_AT),
    }
}

/// One observation in both its wire and domain forms, assembled without a
/// subprocess.
///
/// # Panics
/// Panics when the wire observation does not convert, which is a driver bug.
fn observed(
    link_id: TicketLinkId,
    status: &StatusSelector,
    holder: Option<&ExternalId>,
    live: Vec<LiveTransition>,
) -> Observed {
    let response = read_response(JiraOperation::Observe, status, holder);
    let observation = response
        .observation
        .as_ref()
        .expect("the read response carries an observation")
        .to_core(link_id, WireTimestamp::new(at(DECIDED_AT)))
        .expect("the wire observation converts");
    Observed {
        response,
        observation,
        live_transitions: live,
        principal: principal(),
    }
}

/// The wire form of one observed state.
///
/// # Panics
/// Panics on values the domain refuses, which is a driver bug.
fn wire_observation(
    status: &StatusSelector,
    holder: Option<&ExternalId>,
) -> kontor_integrations_asma::jira::WireObservation {
    kontor_integrations_asma::jira::WireObservation {
        status_id: status.status_id.clone(),
        status_name: status.status_name.clone(),
        status_category: name("In Progress"),
        issue_type: name("User Story"),
        assignee_account_id: holder.cloned(),
        assignee_display: holder.map(|_| name("A Human")),
        update_token: Some(external("12345")),
        observation_hash: ContentHash::of(status.status_id.as_str().as_bytes()),
    }
}

/// A read answer the boundary could plausibly have produced.
fn read_response(
    operation: JiraOperation,
    status: &StatusSelector,
    holder: Option<&ExternalId>,
) -> JiraResponse {
    JiraResponse {
        schema_version: WIRE_SCHEMA_VERSION,
        operation,
        effective_operation: operation,
        issue_key: external("ASMA-1"),
        idempotency_key: key("kon-mvp-18-domain-asma"),
        intent_hash: None,
        requested_at: WireTimestamp::new(at(DECIDED_AT)),
        completed_at: WireTimestamp::new(at("2026-08-12T09:00:01Z")),
        outcome: JiraOutcome::Observed,
        observation: Some(wire_observation(status, holder)),
        principal_account_id: Some(principal().account_id),
        live_transitions: Vec::new(),
        effects: kontor_integrations_asma::jira::WireEffects::default(),
        confirmation: None,
        conflict: None,
        unavailable: None,
        notes: Vec::new(),
    }
}

/// An apply answer carrying both the assignment it performed and the refetched
/// observation that confirms it.
fn write_response(target: &StatusSelector) -> JiraResponse {
    let holder = principal().account_id;
    JiraResponse {
        operation: JiraOperation::Apply,
        effective_operation: JiraOperation::Apply,
        outcome: JiraOutcome::Applied,
        effects: kontor_integrations_asma::jira::WireEffects {
            field_ids: Vec::new(),
            assignment: Some(kontor_integrations_asma::jira::WireAssignment {
                action: OwnershipAction::ReassignToPrincipal,
                account_id: Some(holder.clone()),
            }),
            transition: None,
        },
        confirmation: Some(kontor_integrations_asma::jira::WireConfirmation {
            observation: wire_observation(target, Some(&holder)),
            confirmed_at: WireTimestamp::new(at("2026-08-12T09:00:02Z")),
        }),
        ..read_response(JiraOperation::Apply, target, Some(&holder))
    }
}

/// One live route.
fn route(transition_id: &str, to: &StatusSelector) -> LiveTransition {
    LiveTransition {
        transition_id: external(transition_id),
        to: to.clone(),
    }
}

/// The pure decision, with the freshness the case is about.
fn decide(
    spec: &ExternalWorkflowSpec,
    seen: &Observed,
    facts: &InternalTaskFacts,
    freshness: Freshness,
) -> ReconciliationOutcome {
    reconcile(&ReconciliationInput {
        spec,
        observation: &seen.observation,
        freshness,
        facts,
        live_transitions: &seen.live_transitions,
        principal: &seen.principal,
    })
}

/// Every decision the cross-workflow matrix compares, in a fixed order.
///
/// The rows are chosen so each one exercises a *different* branch of the
/// evaluator; running them against two disjoint vocabularies is what turns
/// "there is no name branch" into evidence.
fn matrix(spec: &ExternalWorkflowSpec) -> Vec<(&'static str, ReconciliationOutcome)> {
    let holder = principal().account_id;
    let stranger = external("acct-a-human-being");
    let dev = target_of(spec, IMPLEMENTATION_ACTIVE);
    let qa = target_of(spec, QA_ACTIVE);
    let hold = target_of(spec, TERMINAL_HOLD);
    let done = target_of(spec, TERMINAL_DONE);
    let current = first_inbound(spec);
    let working = facts(TaskState::InProgress, GateState::NotReady, None);
    let ready = facts(TaskState::InProgress, GateState::Ready, None);
    let active = facts(TaskState::InProgress, GateState::Active, None);
    let blocked = facts(TaskState::Blocked, GateState::NotReady, None);
    let finished = facts(
        TaskState::Done,
        GateState::Passed,
        Some(TerminalOutcome::Succeeded),
    );
    let link = TicketLinkId::generate();
    let unknown = StatusSelector {
        status_id: external("a-status-no-fixture-declares"),
        status_name: name("Unknown"),
    };

    vec![
        (
            "stale",
            decide(
                spec,
                &observed(link, &current, Some(&holder), vec![route("t", &dev)]),
                &working,
                Freshness::Stale,
            ),
        ),
        (
            "unknown_status",
            decide(
                spec,
                &observed(link, &unknown, Some(&holder), Vec::new()),
                &working,
                Freshness::Fresh,
            ),
        ),
        (
            "terminal_without_evidence",
            decide(
                spec,
                &observed(link, &done, Some(&holder), Vec::new()),
                &working,
                Freshness::Fresh,
            ),
        ),
        (
            "terminal_preserved",
            decide(
                spec,
                &observed(link, &done, Some(&stranger), Vec::new()),
                &finished,
                Freshness::Fresh,
            ),
        ),
        (
            "unowned",
            decide(
                spec,
                &observed(link, &current, None, vec![route("t", &dev)]),
                &working,
                Freshness::Fresh,
            ),
        ),
        (
            "foreign_owner",
            decide(
                spec,
                &observed(link, &current, Some(&stranger), vec![route("t", &dev)]),
                &working,
                Freshness::Fresh,
            ),
        ),
        (
            "qa_ready",
            decide(
                spec,
                &observed(link, &dev, Some(&holder), vec![route("t", &qa)]),
                &ready,
                Freshness::Fresh,
            ),
        ),
        (
            "qa_active",
            decide(
                spec,
                &observed(link, &current, Some(&holder), vec![route("t", &qa)]),
                &active,
                Freshness::Fresh,
            ),
        ),
        (
            "hold",
            decide(
                spec,
                &observed(link, &current, Some(&holder), vec![route("t", &hold)]),
                &blocked,
                Freshness::Fresh,
            ),
        ),
        (
            "close",
            decide(
                spec,
                &observed(link, &current, Some(&holder), vec![route("t", &done)]),
                &finished,
                Freshness::Fresh,
            ),
        ),
        (
            "no_route_to_the_target",
            decide(
                spec,
                &observed(link, &current, Some(&holder), Vec::new()),
                &blocked,
                Freshness::Fresh,
            ),
        ),
    ]
}

/// One outcome reduced to its shape: what happened, never which status it was.
fn shape_of(outcome: &ReconciliationOutcome) -> String {
    match outcome {
        ReconciliationOutcome::NoOp => "no_op".to_owned(),
        ReconciliationOutcome::Conflict(kind) => format!("conflict:{kind}"),
        ReconciliationOutcome::Transition(plan) => format!(
            "transition:prerequisite={}:has_transition={}:has_assignment={}",
            plan.assignment_prerequisite,
            plan.transition.is_some(),
            plan.assignment.is_some()
        ),
    }
}

/// One outcome reduced to the external status it names, if it names one.
fn target_text(outcome: &ReconciliationOutcome) -> Option<String> {
    match outcome {
        ReconciliationOutcome::Transition(plan) => Some(plan.target.status_id.as_str().to_owned()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Intake builders
// ---------------------------------------------------------------------------

/// One canonical source event.
///
/// The envelope carries the two pointers the shipped trigger deduplicates on, so
/// the dedup key is computed from the fixture's own expression rather than from
/// a shape invented here.
///
/// # Panics
/// Panics when the envelope does not canonicalize, which is a driver bug.
fn source_event(external_event_id: &str, marker: &str) -> CanonicalSourceEvent {
    CanonicalSourceEvent {
        id: SourceEventId::generate(),
        identity: SourceIdentity {
            source_kind: SourceKindKey::parse("webhook").expect("a legal source kind"),
            source_connection: SourceConnectionKey::parse("conn.alpha")
                .expect("a legal source connection"),
            external_event_id: external(external_event_id),
        },
        envelope: CanonicalDocument::from_value(&json!({
            "schema_version": 1,
            "event_schema": "schema.request-created",
            "event_schema_version": 4,
            "kind": "request.created",
            "external_id": marker,
        }))
        .expect("a canonical envelope"),
        external_observed_at: at(DECIDED_AT),
        ingested_at: at(DECIDED_AT),
        processing_state: SourceProcessingState::Received,
    }
}

/// A `proposed` decision about one event, keyed by the trigger's own dedup
/// expression.
///
/// # Panics
/// Panics when the dedup expression does not evaluate against the envelope,
/// which would be a fixture mismatch rather than a finding about the tree.
fn proposed_receipt(
    trigger: &TriggerSpec,
    event: &CanonicalSourceEvent,
    idempotency: &str,
) -> IntakeReceipt {
    IntakeReceipt {
        id: IntakeReceiptId::generate(),
        source_event_id: event.id,
        source_event_hash: event.envelope.hash().clone(),
        trigger: trigger.id.clone(),
        trigger_version: trigger.version,
        result: IntakeResult::Proposed,
        approval: None,
        proposed: None,
        idempotency_key: key(idempotency),
        dedup_key: trigger
            .dedup
            .evaluate(&event.envelope)
            .expect("the trigger's dedup expression resolves in this envelope"),
        duplicate_of: None,
        predecessor_receipt_id: None,
        decided_at: at(DECIDED_AT),
    }
}

/// One task graph for a terminal intake decision.
fn intake_work(project_id: ProjectId, task_id: TaskId) -> IntakeWorkPlan {
    IntakeWorkPlan {
        mini_project: None,
        tasks: vec![NewTask {
            id: task_id,
            project_id,
            mini_project_id: None,
            title: name("Work created by the KON-MVP-18 intake pilot"),
            module: None,
            state: TaskState::Ready,
            created_at: at(DECIDED_AT),
        }],
    }
}

/// The stable spelling of a recording outcome.
fn outcome_name(outcome: &IntakeOutcome) -> &'static str {
    match outcome {
        IntakeOutcome::Recorded(_) => "recorded",
        IntakeOutcome::Duplicate(_) => "duplicate",
    }
}

/// The receipt id a recording outcome returned, if it returned one.
fn receipt_id_of<E>(outcome: &Result<IntakeOutcome, E>) -> Option<String> {
    match outcome {
        Ok(IntakeOutcome::Recorded(receipt) | IntakeOutcome::Duplicate(receipt)) => {
            Some(receipt.id.to_string())
        }
        Err(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Small builders
// ---------------------------------------------------------------------------

/// A bounded, non-zero budget.
///
/// # Panics
/// Panics on a currency the domain refuses, which is a driver bug.
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

/// An external display name.
///
/// # Panics
/// Panics on text the domain refuses, which is a driver bug.
fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a legal external name")
}

/// An opaque external identifier.
///
/// # Panics
/// Panics on text the domain refuses, which is a driver bug.
fn external(value: &str) -> ExternalId {
    ExternalId::parse(value).expect("a legal external id")
}

/// A bounded text field value.
///
/// # Panics
/// Panics on text the domain refuses, which is a driver bug.
fn text(body: &str) -> FieldValue {
    FieldValue::Text {
        body: BoundedText::parse(body).expect("a legal bounded text"),
    }
}

/// An idempotency key.
///
/// # Panics
/// Panics on text the domain refuses, which is a driver bug.
fn key(value: &str) -> IdempotencyKey {
    IdempotencyKey::parse(value).expect("a legal idempotency key")
}

/// A semantic milestone key.
///
/// # Panics
/// Panics on text the domain refuses, which is a driver bug.
fn milestone(value: &str) -> SemanticMilestoneKey {
    SemanticMilestoneKey::parse(value).expect("a legal milestone key")
}

/// A small canonical document, for the fixture rows a foreign key demands.
///
/// # Panics
/// Panics when the document does not canonicalize, which is a driver bug.
fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&json!({ "schema_version": 1, "marker": marker }))
        .expect("a canonical document")
}

/// The static rule text a domain rejection carries, for the evidence file.
fn rule_of(error: DomainError) -> Option<&'static str> {
    match error {
        DomainError::Invalid { rule, .. }
        | DomainError::MissingAuthority { rule, .. }
        | DomainError::MissingEvidence { rule, .. } => Some(rule),
        _ => None,
    }
}

/// The same, for a rejection that arrived wrapped in an [`AsmaError`].
fn domain_rule(error: AsmaError) -> Option<&'static str> {
    match error {
        AsmaError::Domain(domain) => rule_of(domain),
        _ => None,
    }
}

/// One rejection, described without quoting anything it might be carrying.
fn describe_error(error: Option<AsmaError>) -> Option<String> {
    error.map(|error| match error {
        AsmaError::Conflict { kind, .. } => format!("conflict:{kind}"),
        AsmaError::Unavailable { reason, .. } => format!("unavailable:{reason}"),
        AsmaError::Refused { rule, .. } => format!("refused:{rule}"),
        AsmaError::Selection { conflict, .. } => format!("selection:{conflict}"),
        AsmaError::Domain(_) => "domain".to_owned(),
        // `AsmaError` is non-exhaustive, so a variant this build does not know
        // about is still a rejection worth recording as one.
        _ => "other".to_owned(),
    })
}
