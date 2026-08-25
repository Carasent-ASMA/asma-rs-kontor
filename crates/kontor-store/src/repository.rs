//! SQLite implementations of the [`kontor_core::repository`] ports.
//!
//! Every mutation opens exactly one transaction. Reads always carry the project
//! id in the predicate, so a valid id belonging to another project resolves to
//! `None` rather than to somebody else's row.
//!
//! The transactions are opened with [`rusqlite::Connection::unchecked_transaction`]
//! so the methods can take `&self`. That is sound here precisely because the
//! connection is private and no method ever calls another method that opens a
//! transaction: each one is a single flat unit of work.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, ChildCalendarWindows, ExceptionKind,
    ExceptionProvenance, ExecutionAuthorization, HolidayImportBatch, HolidayImportKind,
    HolidayProviderKind, HolidaySourceRevision, IanaTimeZone, OverrideExpiry, OverrideRevocation,
    ScheduleOverride, WeeklyWindow, WorkCalendarAssignment, WorkScope,
};
use kontor_core::consultation::{
    CommitteeRole, CommitteeVerdict, ConsultationFamily, ConsultationRunId, ConsultationRunState,
};
use kontor_core::id::{
    AccountProfileId, AdvisorRunId, AgentRunId, AggregateRevision, ArtifactKey, BoundedText,
    CalendarExceptionId, CalendarProfileId, CanonicalDocument, CapacityObservationId,
    CommandReceiptId, CommitteeRunId, ContentHash, CredentialAlias, CurrencyCode, EventCursor,
    ExternalId, ExternalName, GateKey, GuardrailEvaluationId, HolidaySourceId, IdempotencyKey,
    IntakeReceiptId, MiniProjectId, ModuleKey, Money, OpenQuestionId, PersonaScenarioId, PhaseKey,
    ProjectId, QuickSessionId, RealmId, RoleCatalogId, RoleCode, RoleKey, RoleSlotId,
    RuntimeBindingId, RuntimeKindKey, ScheduleOverrideId, SeatBindingId, SignedDuration,
    SpecVersion, StatusConflictId, TaskId, TaskWorkflowId, TeamRunId, TeamTemplateId, TicketLinkId,
    Timestamp, TopologyKindKey, TopologyNodeId, TopologySpecId, TriggerKey, WorkCalendarId,
    WorkProfileKey, format_utc_timestamp, parse_utc_timestamp,
};
use kontor_core::open_question::{
    AmbiguityRound, Disposition, DispositionKind, DispositionOutcome, OpenQuestion,
    OpenQuestionAttachment, OpenQuestionSummary, QuestionScope, TriggerFiring,
};
use kontor_core::quota::{CreditBalance, QuotaWindow, QuotaWindowKind};
use kontor_core::realm::{EventEnvelope, RealmCursor, ReceiptEnvelope, SnapshotEnvelope};
use kontor_core::receipt::{
    AggregateRef, CommandKind, CommandOutboxEntry, CommandReceipt, ReceiptAuthority,
};
use kontor_core::repository::OpenQuestionRepository;
use kontor_core::repository::{
    AccountProfile, AccountProfileUpdate, AdaptiveAdmissionAdvance, AgentRun, AvailabilityOverride,
    CalendarRepository, CapacityObservation, CapacityRepository, CommandRepository,
    ConnectorSpecSelector, CredentialReference, CredentialReferenceKind, GateEvaluation,
    HistoryGapKind, HistoryGapMarker, IntakeCreatedWork, IntakeDecisionRecord, IntakeOutcome,
    IntakeRepository, MiniProject, MiniProjectTopologySnapshot, NewAbandonReceipt,
    NewAccountProfile, NewAdaptiveAdmissionState, NewAgentRun, NewAvailabilityOverride,
    NewCapacityObservation, NewCommandIntent, NewConsultationRecoveryAttempt, NewGateEvaluation,
    NewIntakeDecision, NewIntakeDecisionRecord, NewIntakeReevaluation, NewLocalCommand,
    NewMiniProject, NewNativeContainerBinding, NewObservation, NewProject, NewProviderQuotaState,
    NewRuntimeEvent, NewSeatBinding, NewSessionTopologyNode, NewSourceEvent, NewTask,
    NewTaskPersonaSnapshot, NewTaskWorkflow, NewTeamRun, NewTicketLink, PhaseAdvance, Project,
    ProjectRepository, ProjectTopologyDefault, ProviderQuotaState, RealmEventPage, RealmRepository,
    ReceiptAdvance, ReevaluationOutcome, RepositoryError, RepositoryResult, RunClosure,
    RunInspection, RunRepository, RuntimeBinding, RuntimeEvent, SeatLivenessObservation,
    SessionVerdictEvidence, SourceDisposition, SourceEventIngest, SpecRepository,
    StoredAdvisorAdvice, StoredCapacityConfiguration, StoredCommitteeFinding,
    StoredCompletionProfile, StoredCompletionWake, StoredConsultationProfileRevision,
    StoredConsultationRecoveryAttempt, StoredConsultationRun, StoredConsultationSeat,
    StoredCoreTeamRevision, StoredEpicCompletion, StoredEpicRoster, StoredHostedTopologySeat,
    StoredPromotion, StoredQuickSession, StoredRemediationProposal, Task, TaskInspection,
    TaskTransitionRequest, TaskWorkflow, TeamRun, TeamRunAdvance, TeamRunClosure, TicketLink,
    TicketRepository, TopologyRepository, WorkflowRepository, validate_dependency_graph,
};
use kontor_core::spec::{
    CanonicalSourceEvent, CatalogRoleRef, IntakeReceipt, NodeProjectionCapability,
    PersonaScenarioSnapshot, PersonaScenarioSpec, ProjectSessionTopologySpec, ProviderQuotaKind,
    ProviderQuotaSource, ResolvedWorkProfileSnapshot, RoleCatalogRevision, Shareability,
    ShareabilityClass, ShareabilityClassifier, ShareabilityProvenance, ShareabilityTier,
    SourceIdentity, TeamRunSnapshot, TeamTemplateRevision, TopologySnapshot, TriggerSpec,
    WorkProfileSpec,
};
use kontor_core::state::{
    AbandonReceiptFacts, AdaptiveAdmissionState, DerivedRunState, DesiredRunState, GateState,
    GateVerdict, ImportedTaskState, NativeContainerBinding, NativeRuntimeIdentity,
    ObservedContainerKind, ObservedRunState, PlacementState, RunLifecycle, RunProjection,
    SeatAttachment, SeatAttachmentObservation, SeatBinding, SessionTopologyNode,
    TaskProgressEvidence, TaskReopenAuthority, TaskState, TaskTeamClosure, TaskTransition,
    TeamChildEvidence, TeamEvidenceSource, TeamTerminalEvidence, TerminalEvidence,
    TerminalEvidenceSource, TerminalOutcome, TopologyLifecycle, certify_task_progress,
    evaluate_seat_attachment, plan_team_advance, plan_team_closure,
};
use kontor_core::ticket::{
    ExternalCommentRevision, ExternalTicketObservation, ExternalWorkflowSpec, StatusConflict,
    StatusTransitionReceipt, TicketFieldSpec, TicketSyncProjection,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::SqliteStore;
use crate::events::append::stored_payload;
use crate::events::replay::{EVENT_COLUMNS, read_event};
use crate::graph::{Applied, IdempotencyBinding};

/// Maximum length of an agent-run parent chain that is walked when checking for
/// a lineage cycle.
const MAX_PARENT_CHAIN: usize = 1_024;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn backend(error: rusqlite::Error) -> RepositoryError {
    if let rusqlite::Error::SqliteFailure(failure, _) = &error
        && failure.code == rusqlite::ErrorCode::ConstraintViolation
    {
        // Constraint text is deliberately not propagated: SQLite includes the
        // offending row's context in some messages and this crate never lets a
        // persisted value reach an error string.
        return RepositoryError::Conflict {
            subject: "storage",
            rule: "a uniqueness, check or immutability constraint refused the write",
        };
    }
    RepositoryError::Backend {
        detail: error.to_string(),
    }
}

pub(crate) fn conflict(subject: &'static str, rule: &'static str) -> RepositoryError {
    RepositoryError::Conflict { subject, rule }
}

pub(crate) fn text(timestamp: Timestamp) -> String {
    format_utc_timestamp(timestamp)
}

fn canonical_json(value: &serde_json::Value, subject: &'static str) -> RepositoryResult<String> {
    CanonicalDocument::from_serializable(value)
        .map(|document| document.json().to_owned())
        .map_err(|_| RepositoryError::Conflict {
            subject,
            rule: "the document cannot be canonicalized",
        })
}

type ConsultationRunColumns = (
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    i64,
    String,
    String,
    Option<String>,
);

fn consultation_run_id(
    family: ConsultationFamily,
    value: &str,
) -> RepositoryResult<ConsultationRunId> {
    match family {
        ConsultationFamily::Advisor => AdvisorRunId::parse(value).map(ConsultationRunId::Advisor),
        ConsultationFamily::Committee => {
            CommitteeRunId::parse(value).map(ConsultationRunId::Committee)
        }
    }
    .map_err(RepositoryError::from)
}

fn read_consultation_run(
    project_id: ProjectId,
    run_id: ConsultationRunId,
    columns: ConsultationRunColumns,
) -> RepositoryResult<StoredConsultationRun> {
    let (
        mini_project_id,
        profile_id,
        profile_version,
        definition_hash,
        question,
        question_hash,
        context,
        context_hash,
        caller_seat_binding_id,
        topology_node_id,
        invoke_key,
        invoke_intent_hash,
        state,
        round,
        result,
        result_hash,
        revision,
        created_at,
        updated_at,
        settled_at,
    ) = columns;
    let question = BoundedText::parse(&question)?;
    let question_hash = ContentHash::parse(&question_hash)?;
    if ContentHash::of(question.as_str().as_bytes()) != question_hash {
        return Err(RepositoryError::Conflict {
            subject: "consultation run",
            rule: "the stored question no longer matches its digest",
        });
    }
    let context_hash = ContentHash::parse(&context_hash)?;
    let context = CanonicalDocument::from_stored(&context, &context_hash)?;
    let result_hash = result_hash
        .map(|hash| ContentHash::parse(&hash))
        .transpose()?;
    let result = match (result, result_hash.as_ref()) {
        (Some(json), Some(hash)) => Some(
            serde_json::from_str(CanonicalDocument::from_stored(&json, hash)?.json()).map_err(
                |error| RepositoryError::Backend {
                    detail: format!("a consultation result could not be decoded: {error}"),
                },
            )?,
        ),
        (None, None) => None,
        _ => {
            return Err(RepositoryError::Conflict {
                subject: "consultation run",
                rule: "result bytes and digest must be present together",
            });
        }
    };
    Ok(StoredConsultationRun {
        id: run_id,
        project_id,
        mini_project_id: MiniProjectId::parse(&mini_project_id)?,
        profile_id,
        profile_version: read_version(profile_version)?,
        definition_hash: ContentHash::parse(&definition_hash)?,
        question,
        question_hash,
        context: serde_json::from_str(context.json()).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a consultation context could not be decoded: {error}"),
            }
        })?,
        context_hash,
        caller_seat_binding_id: SeatBindingId::parse(&caller_seat_binding_id)?,
        topology_node_id: TopologyNodeId::parse(&topology_node_id)?,
        invoke_key: IdempotencyKey::parse(&invoke_key)?,
        invoke_intent_hash: ContentHash::parse(&invoke_intent_hash)?,
        state: ConsultationRunState::parse(&state)?,
        round: u32::try_from(round).map_err(|_| RepositoryError::Conflict {
            subject: "consultation run",
            rule: "round is outside the supported range",
        })?,
        result,
        result_hash,
        revision: revision_of(revision)?,
        created_at: read_timestamp(&created_at)?,
        updated_at: read_timestamp(&updated_at)?,
        settled_at: settled_at.map(|value| read_timestamp(&value)).transpose()?,
    })
}

type ConsultationSeatColumns = (
    String,
    Option<String>,
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn read_consultation_seat(
    run_id: ConsultationRunId,
    columns: ConsultationSeatColumns,
) -> RepositoryResult<StoredConsultationSeat> {
    let (
        role_slot_id,
        committee_role,
        logical_role,
        seat_binding_id,
        model_rung,
        occupancy_generation,
        runtime_kind,
        host,
        generation,
        native_id,
        provider_session_id,
        observed_at,
    ) = columns;
    let native_identity = match (runtime_kind, host, generation, native_id) {
        (Some(runtime_kind), Some(host), Some(generation), Some(native_id)) => {
            Some(NativeRuntimeIdentity {
                runtime_kind: RuntimeKindKey::parse(&runtime_kind)?,
                host: ExternalName::parse(&host)?,
                generation: u64::try_from(generation).map_err(|_| RepositoryError::Conflict {
                    subject: "consultation seat",
                    rule: "runtime generation is negative",
                })?,
                native_id: ExternalId::parse(&native_id)?,
            })
        }
        (None, None, None, None) => None,
        _ => {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat",
                rule: "native identity is only partially present",
            });
        }
    };
    Ok(StoredConsultationSeat {
        run_id,
        role_slot_id: RoleSlotId::parse(&role_slot_id)?,
        committee_role: committee_role
            .map(|role| CommitteeRole::parse(&role))
            .transpose()?,
        logical_role: RoleKey::parse(&logical_role)?,
        seat_binding_id: SeatBindingId::parse(&seat_binding_id)?,
        model_rung: serde_json::from_str(&model_rung).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a consultation model rung could not be decoded: {error}"),
            }
        })?,
        occupancy_generation: u64::try_from(occupancy_generation).map_err(|_| {
            RepositoryError::Conflict {
                subject: "consultation seat",
                rule: "occupancy generation is not positive",
            }
        })?,
        native_identity,
        provider_session_id: provider_session_id
            .map(|value| ExternalId::parse(&value))
            .transpose()?,
        observed_at: observed_at
            .map(|value| read_timestamp(&value))
            .transpose()?,
    })
}

type CommitteeFindingColumns = (String, String, String, i64, String, String, String);

fn read_committee_finding(
    committee_run_id: CommitteeRunId,
    round: u32,
    columns: CommitteeFindingColumns,
) -> RepositoryResult<StoredCommitteeFinding> {
    let (role_slot_id, role, verdict, complete, document, document_hash, recorded_at) = columns;
    let document_hash = ContentHash::parse(&document_hash)?;
    let document = CanonicalDocument::from_stored(&document, &document_hash)?;
    Ok(StoredCommitteeFinding {
        committee_run_id,
        round,
        role_slot_id: RoleSlotId::parse(&role_slot_id)?,
        role: CommitteeRole::parse(&role)?,
        verdict: CommitteeVerdict::parse(&verdict)?,
        evidence_complete: complete == 1,
        document: serde_json::from_str(document.json()).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a Committee finding could not be decoded: {error}"),
            }
        })?,
        document_hash,
        recorded_at: read_timestamp(&recorded_at)?,
    })
}

type AdvisorAdviceColumns = (String, String, String, String);

fn read_advisor_advice(
    project_id: ProjectId,
    advisor_run_id: AdvisorRunId,
    columns: AdvisorAdviceColumns,
) -> RepositoryResult<StoredAdvisorAdvice> {
    let (seat_binding_id, document, document_hash, recorded_at) = columns;
    let document_hash = ContentHash::parse(&document_hash)?;
    let document = CanonicalDocument::from_stored(&document, &document_hash)?;
    Ok(StoredAdvisorAdvice {
        advisor_run_id,
        project_id,
        seat_binding_id: SeatBindingId::parse(&seat_binding_id)?,
        document: serde_json::from_str(document.json()).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("Advisor advice could not be decoded: {error}"),
            }
        })?,
        document_hash,
        recorded_at: read_timestamp(&recorded_at)?,
    })
}

pub(crate) fn read_timestamp(value: &str) -> RepositoryResult<Timestamp> {
    Ok(parse_utc_timestamp(value)?)
}

pub(crate) fn to_json<T: Serialize>(value: &T) -> RepositoryResult<String> {
    serde_json::to_string(value).map_err(|_| RepositoryError::Backend {
        detail: "value could not be serialized as JSON".to_owned(),
    })
}

pub(crate) fn from_json<T: DeserializeOwned>(value: &str) -> RepositoryResult<T> {
    serde_json::from_str(value).map_err(|_| RepositoryError::Backend {
        detail: "stored JSON does not match the expected shape".to_owned(),
    })
}

pub(crate) fn revision_of(value: i64) -> RepositoryResult<AggregateRevision> {
    let unsigned = u64::try_from(value).map_err(|_| RepositoryError::Backend {
        detail: "stored revision is negative".to_owned(),
    })?;
    Ok(AggregateRevision::parse(unsigned)?)
}

pub(crate) fn revision_column(revision: AggregateRevision) -> RepositoryResult<i64> {
    i64::try_from(revision.get()).map_err(|_| RepositoryError::Backend {
        detail: "revision exceeds the storable range".to_owned(),
    })
}

pub(crate) fn version_column(version: SpecVersion) -> i64 {
    i64::from(version.get())
}

pub(crate) fn read_version(value: i64) -> RepositoryResult<SpecVersion> {
    let narrowed = u32::try_from(value).map_err(|_| RepositoryError::Backend {
        detail: "stored version is out of range".to_owned(),
    })?;
    Ok(SpecVersion::parse(narrowed)?)
}

/// Read a versioned document back, verifying its canonical bytes and digest
/// before it is trusted.
pub(crate) fn stored_document<T: DeserializeOwned>(json: &str, hash: &str) -> RepositoryResult<T> {
    let digest = ContentHash::parse(hash)?;
    let document = CanonicalDocument::from_stored(json, &digest)?;
    Ok(document.deserialize::<T>()?)
}

pub(crate) fn scope_columns(scope: WorkScope) -> (&'static str, Option<String>, Option<String>) {
    match scope {
        WorkScope::Project => ("project", None, None),
        WorkScope::MiniProject { mini_project_id } => {
            ("mini_project", Some(mini_project_id.to_string()), None)
        }
        WorkScope::Task { task_id } => ("task", None, Some(task_id.to_string())),
    }
}

pub(crate) fn read_scope(
    kind: &str,
    mini_project: Option<String>,
    task: Option<String>,
) -> RepositoryResult<WorkScope> {
    match kind {
        "project" => Ok(WorkScope::Project),
        "mini_project" => {
            let id = mini_project.ok_or_else(|| RepositoryError::Backend {
                detail: "scope row is missing its goal".to_owned(),
            })?;
            Ok(WorkScope::MiniProject {
                mini_project_id: MiniProjectId::parse(&id)?,
            })
        }
        "task" => {
            let id = task.ok_or_else(|| RepositoryError::Backend {
                detail: "scope row is missing its task".to_owned(),
            })?;
            Ok(WorkScope::Task {
                task_id: TaskId::parse(&id)?,
            })
        }
        _ => Err(RepositoryError::Backend {
            detail: "scope row has an unknown kind".to_owned(),
        }),
    }
}

/// Split a typed target into its discriminator and its seven mutually exclusive
/// id columns.
pub(crate) fn target_columns(target: &AggregateRef) -> (&'static str, [Option<String>; 7]) {
    let mut columns: [Option<String>; 7] = Default::default();
    let kind = match target {
        AggregateRef::Project { project_id } => {
            columns[0] = Some(project_id.to_string());
            "project"
        }
        AggregateRef::MiniProject { mini_project_id } => {
            columns[1] = Some(mini_project_id.to_string());
            "mini_project"
        }
        AggregateRef::Task { task_id } => {
            columns[2] = Some(task_id.to_string());
            "task"
        }
        AggregateRef::TeamRun { team_run_id } => {
            columns[3] = Some(team_run_id.to_string());
            "team_run"
        }
        AggregateRef::AgentRun { agent_run_id } => {
            columns[4] = Some(agent_run_id.to_string());
            "agent_run"
        }
        AggregateRef::TicketLink { link_id } => {
            columns[5] = Some(link_id.to_string());
            "ticket_link"
        }
        AggregateRef::WorkCalendar { work_calendar_id } => {
            columns[6] = Some(work_calendar_id.to_string());
            "work_calendar"
        }
    };
    (kind, columns)
}

pub(crate) fn target_project(target: &AggregateRef) -> Option<ProjectId> {
    match target {
        AggregateRef::Project { project_id } => Some(*project_id),
        _ => None,
    }
}

impl SqliteStore {
    /// Open one short transaction.
    ///
    /// `IMMEDIATE`, not the default deferred behaviour, and that matters under
    /// concurrency rather than in a single-process test. A deferred transaction
    /// takes its read snapshot first and only asks for the write lock when it
    /// reaches its first write — and in WAL mode, if anyone committed in
    /// between, SQLite refuses that upgrade with `SQLITE_BUSY` *immediately*,
    /// without consulting the busy timeout, because retrying could deadlock.
    /// Two appenders would then fail each other rather than queue.
    ///
    /// Taking the write lock up front means the second writer waits out the
    /// bounded busy timeout and proceeds. Every transaction in this store is
    /// short and none is held across a native call, so serializing them is the
    /// cheap half of the trade.
    pub(crate) fn begin(&self) -> RepositoryResult<Transaction<'_>> {
        Transaction::new_unchecked(&self.connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)
    }
}

// ---------------------------------------------------------------------------
// Projects, goals and tasks
// ---------------------------------------------------------------------------

pub(crate) fn read_project(row: &Row<'_>) -> RepositoryResult<Project> {
    Ok(Project {
        id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        name: ExternalName::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        root_path: ExternalName::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(3).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
    })
}

pub(crate) fn read_task(row: &Row<'_>) -> RepositoryResult<Task> {
    let mini_project: Option<String> = row.get(2).map_err(backend)?;
    let module: Option<String> = row.get(4).map_err(backend)?;
    let imported_state: Option<String> = row.get(9).map_err(backend)?;
    Ok(Task {
        id: TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        mini_project_id: mini_project
            .as_deref()
            .map(MiniProjectId::parse)
            .transpose()?,
        title: ExternalName::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        module: module.as_deref().map(ModuleKey::parse).transpose()?,
        state: TaskState::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        imported_state: imported_state
            .as_deref()
            .map(ImportedTaskState::parse)
            .transpose()?,
        revision: revision_of(row.get::<_, i64>(6).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(8).map_err(backend)?)?,
    })
}

pub(crate) const TASK_COLUMNS: &str = "id, project_id, mini_project_id, title, module_key, state, revision, created_at, updated_at, \
     imported_state";

const ACCOUNT_PROFILE_COLUMNS: &str = "id, project_id, label, external_account_id, created_at, \
     harness, credential_ref_kind, credential_ref_alias, environment_refs, environment_refs_hash, \
     routing, routing_hash, capability, capability_hash, provider_identity, enabled, revision, \
     updated_at";

/// A column that schema v2 requires but a schema v1 row never had.
///
/// A row missing one of these is not repaired and not defaulted: an account
/// profile with no harness, no credential reference, no enabled flag or no
/// revision is not a profile anything may launch through, and inventing the
/// missing half here would be exactly the silent fallback the migration refused
/// to make. Reading fails loudly instead — including in `list`, because quietly
/// omitting a profile from a security-relevant listing is worse than an error.
fn required_account_column<T>(value: Option<T>) -> RepositoryResult<T> {
    value.ok_or(RepositoryError::Conflict {
        subject: "account profile",
        rule: "the stored profile predates its non-secret credential identity",
    })
}

fn read_account_profile(row: &Row<'_>) -> RepositoryResult<AccountProfile> {
    let external_account_id: Option<String> = row.get(3).map_err(backend)?;
    let harness: Option<String> = row.get(5).map_err(backend)?;
    let kind: Option<String> = row.get(6).map_err(backend)?;
    let alias: Option<String> = row.get(7).map_err(backend)?;
    let environment: Option<String> = row.get(8).map_err(backend)?;
    let environment_hash: Option<String> = row.get(9).map_err(backend)?;
    let routing: Option<String> = row.get(10).map_err(backend)?;
    let routing_hash: Option<String> = row.get(11).map_err(backend)?;
    let capability: Option<String> = row.get(12).map_err(backend)?;
    let capability_hash: Option<String> = row.get(13).map_err(backend)?;
    let provider_identity: Option<String> = row.get(14).map_err(backend)?;
    let enabled: Option<i64> = row.get(15).map_err(backend)?;
    let revision: Option<i64> = row.get(16).map_err(backend)?;
    let updated_at: Option<String> = row.get(17).map_err(backend)?;

    Ok(AccountProfile {
        id: AccountProfileId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        label: ExternalName::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        external_account_id: external_account_id
            .as_deref()
            .map(ExternalId::parse)
            .transpose()?,
        harness: RuntimeKindKey::parse(&required_account_column(harness)?)?,
        credential_ref: CredentialReference {
            kind: CredentialReferenceKind::parse(&required_account_column(kind)?)?,
            alias: CredentialAlias::parse(&required_account_column(alias)?)?,
        },
        // Each document is re-admitted through its recorded digest, so a row
        // edited underneath the store fails to load instead of being trusted.
        environment: stored_payload(
            &required_account_column(environment)?,
            &required_account_column(environment_hash)?,
        )?,
        routing: stored_payload(
            &required_account_column(routing)?,
            &required_account_column(routing_hash)?,
        )?,
        capability: stored_payload(
            &required_account_column(capability)?,
            &required_account_column(capability_hash)?,
        )?,
        provider_identity: provider_identity
            .as_deref()
            .map(ExternalId::parse)
            .transpose()?,
        enabled: required_account_column(enabled)? != 0,
        revision: revision_of(required_account_column(revision)?)?,
        created_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
        updated_at: read_timestamp(&required_account_column(updated_at)?)?,
    })
}

/// Read a profile from inside an open transaction, so a write and the value it
/// returns come from the same unit of work.
fn read_account_profile_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: AccountProfileId,
) -> RepositoryResult<Option<AccountProfile>> {
    transaction
        .query_row(
            &format!(
                "SELECT {ACCOUNT_PROFILE_COLUMNS} FROM account_profiles
                 WHERE project_id = ?1 AND id = ?2"
            ),
            params![project_id.to_string(), id.to_string()],
            |row| Ok(read_account_profile(row)),
        )
        .optional()
        .map_err(backend)?
        .transpose()
}

/// The stored revision of one profile, for telling "absent" from "moved" after
/// a compare-and-swap wrote nothing.
///
/// Three outcomes, not two. A row migrated forward from schema v1 has a `NULL`
/// revision, which no compare-and-swap can ever match — so it is neither absent
/// nor moved, and it is reported as the same incomplete-profile conflict that
/// reading it produces rather than being flattened into either.
fn account_profile_revision(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: AccountProfileId,
) -> RepositoryResult<Option<AggregateRevision>> {
    let found: Option<Option<i64>> = transaction
        .query_row(
            "SELECT revision FROM account_profiles WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    match found {
        None => Ok(None),
        Some(revision) => Ok(Some(revision_of(required_account_column(revision)?)?)),
    }
}

impl ProjectRepository for SqliteStore {
    fn create_project(&self, request: &NewProject) -> RepositoryResult<Project> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![
                    request.id.to_string(),
                    request.name.as_str(),
                    request.root_path.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        crate::authority::create_subject_authorities(
            &transaction,
            request.id,
            crate::authority::SubjectOrigins::native(),
        )
        .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(Project {
            id: request.id,
            name: request.name.clone(),
            root_path: request.root_path.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn get_project(&self, id: ProjectId) -> RepositoryResult<Option<Project>> {
        self.connection
            .query_row(
                "SELECT id, name, root_path, revision, created_at FROM projects WHERE id = ?1",
                params![id.to_string()],
                |row| Ok(read_project(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn create_mini_project(&self, request: &NewMiniProject) -> RepositoryResult<MiniProject> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO mini_projects (id, project_id, name, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.name.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(MiniProject {
            id: request.id,
            project_id: request.project_id,
            name: request.name.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn create_task(&self, request: &NewTask) -> RepositoryResult<Task> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO tasks
                     (id, project_id, mini_project_id, title, module_key, state,
                      revision, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.mini_project_id.map(|id| id.to_string()),
                    request.title.as_str(),
                    request.module.as_ref().map(ModuleKey::as_str),
                    request.state.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(Task {
            id: request.id,
            project_id: request.project_id,
            mini_project_id: request.mini_project_id,
            title: request.title.clone(),
            module: request.module.clone(),
            state: request.state,
            imported_state: None,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
            updated_at: request.created_at,
        })
    }

    fn get_task(&self, project_id: ProjectId, id: TaskId) -> RepositoryResult<Option<Task>> {
        self.connection
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1 AND id = ?2"),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_task(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn list_tasks(&self, project_id: ProjectId) -> RepositoryResult<Vec<Task>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            tasks.push(read_task(row)?);
        }
        Ok(tasks)
    }

    fn set_task_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        depends_on: &[TaskId],
    ) -> RepositoryResult<()> {
        if depends_on.contains(&task_id) {
            return Err(DomainError::invalid(
                "task dependency",
                "a task must not depend on itself",
            )
            .into());
        }
        let transaction = self.begin()?;
        transaction
            .execute(
                "DELETE FROM task_dependencies WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
            )
            .map_err(backend)?;
        let now = text(Timestamp::now());
        for dependency in depends_on {
            transaction
                .execute(
                    "INSERT INTO task_dependencies
                         (project_id, task_id, depends_on_task_id, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        project_id.to_string(),
                        task_id.to_string(),
                        dependency.to_string(),
                        now
                    ],
                )
                .map_err(backend)?;
        }

        // Acyclicity is checked over the whole project graph inside this
        // transaction: SQLite can enforce the pair and the self-edge, but not
        // reachability.
        let mut edges: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();
        {
            let mut statement = transaction
                .prepare(
                    "SELECT task_id, depends_on_task_id FROM task_dependencies
                     WHERE project_id = ?1",
                )
                .map_err(backend)?;
            let mut rows = statement
                .query(params![project_id.to_string()])
                .map_err(backend)?;
            while let Some(row) = rows.next().map_err(backend)? {
                let task = TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?;
                let dependency = TaskId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
                edges.entry(task).or_default().insert(dependency);
            }
        }
        validate_dependency_graph(&edges)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn create_account_profile(
        &self,
        request: &NewAccountProfile,
    ) -> RepositoryResult<AccountProfile> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO account_profiles
                     (id, project_id, label, external_account_id, created_at,
                      harness, credential_ref_kind, credential_ref_alias,
                      environment_refs, environment_refs_hash, routing, routing_hash,
                      capability, capability_hash, provider_identity,
                      enabled, revision, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, 1, ?5)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.label.as_str(),
                    request.external_account_id.as_ref().map(ExternalId::as_str),
                    text(request.created_at),
                    request.harness.as_str(),
                    request.credential_ref.kind.as_str(),
                    request.credential_ref.alias.as_str(),
                    request.environment.json(),
                    request.environment.hash().as_str(),
                    request.routing.json(),
                    request.routing.hash().as_str(),
                    request.capability.json(),
                    request.capability.hash().as_str(),
                    request.provider_identity.as_ref().map(ExternalId::as_str),
                    i64::from(request.enabled),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(AccountProfile {
            id: request.id,
            project_id: request.project_id,
            label: request.label.clone(),
            external_account_id: request.external_account_id.clone(),
            harness: request.harness.clone(),
            credential_ref: request.credential_ref.clone(),
            environment: request.environment.clone(),
            routing: request.routing.clone(),
            capability: request.capability.clone(),
            provider_identity: request.provider_identity.clone(),
            enabled: request.enabled,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
            updated_at: request.created_at,
        })
    }

    fn get_account_profile(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
    ) -> RepositoryResult<Option<AccountProfile>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {ACCOUNT_PROFILE_COLUMNS} FROM account_profiles
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_account_profile(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn list_account_profiles(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<AccountProfile>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {ACCOUNT_PROFILE_COLUMNS} FROM account_profiles
                 WHERE project_id = ?1 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut profiles = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            profiles.push(read_account_profile(row)?);
        }
        Ok(profiles)
    }

    fn update_account_profile(
        &self,
        request: &AccountProfileUpdate,
    ) -> RepositoryResult<AccountProfile> {
        let transaction = self.begin()?;
        // The revision comparison lives in the `WHERE` clause, so the read and
        // the write are the same statement and nothing can move between them.
        // A profile that is absent and a profile whose revision moved are
        // distinguished afterwards, from inside the same transaction.
        let changed = transaction
            .execute(
                "UPDATE account_profiles
                 SET label = ?1, enabled = ?2, updated_at = ?3, revision = revision + 1
                 WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
                params![
                    request.label.as_str(),
                    i64::from(request.enabled),
                    text(request.updated_at),
                    request.project_id.to_string(),
                    request.id.to_string(),
                    revision_column(request.expected_revision)?,
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            let found = account_profile_revision(&transaction, request.project_id, request.id)?;
            // Rolls back nothing, because nothing was written — but the drop is
            // explicit so the refusal cannot be read as a partial success.
            drop(transaction);
            return match found {
                None => Err(RepositoryError::NotFound {
                    subject: "account profile",
                }),
                Some(found) => Err(found
                    .expect("account profile", request.expected_revision)
                    .expect_err("a matching revision would have updated exactly one row")
                    .into()),
            };
        }
        let profile = read_account_profile_in(&transaction, request.project_id, request.id)?
            .ok_or(RepositoryError::NotFound {
                subject: "account profile",
            })?;
        transaction.commit().map_err(backend)?;
        Ok(profile)
    }

    fn delete_account_profile(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
        expected_revision: AggregateRevision,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        // The revision is compared in the `DELETE` itself for the same reason
        // the update compares it in the `UPDATE`. Referential safety is not
        // checked here at all: the four `ON DELETE RESTRICT` references schema
        // v1 already declares are what refuse a profile some run, gate
        // evaluation or override still names, and re-implementing that check in
        // Rust would be a second, weaker copy of it.
        let deleted = match transaction.execute(
            "DELETE FROM account_profiles
             WHERE project_id = ?1 AND id = ?2 AND revision = ?3",
            params![
                project_id.to_string(),
                id.to_string(),
                revision_column(expected_revision)?,
            ],
        ) {
            Ok(deleted) => deleted,
            Err(error) => {
                drop(transaction);
                return Err(match backend(error) {
                    RepositoryError::Conflict { .. } => conflict(
                        "account profile",
                        "a referenced profile is disabled, never deleted",
                    ),
                    other => other,
                });
            }
        };
        if deleted != 1 {
            let found = account_profile_revision(&transaction, project_id, id)?;
            drop(transaction);
            return match found {
                None => Err(RepositoryError::NotFound {
                    subject: "account profile",
                }),
                Some(found) => Err(found
                    .expect("account profile", expected_revision)
                    .expect_err("a matching revision would have deleted exactly one row")
                    .into()),
            };
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Operational topology
// ---------------------------------------------------------------------------

const TOPOLOGY_NODE_COLUMNS: &str = "id, project_id, mini_project_id, spec_id, spec_version, \
    spec_hash, kind, parent_id, lifecycle, placement, revision, created_at, updated_at, task_id";
const SEAT_BINDING_COLUMNS: &str = "id, project_id, topology_node_id, role_slot_id, \
    role_catalog_id, role_catalog_version, role_code, standard_title, custom_display_name, \
    task_id, team_run_id, lifecycle, attach_deadline, last_attached_at, last_activity_at, \
    parent_seat_binding_id, released_at, replaced_by_seat_binding_id, runtime_reported, \
    revision, created_at, updated_at";
const NATIVE_CONTAINER_COLUMNS: &str = "topology_node_id, project_id, container_binding_id, \
    runtime_kind, host, generation, native_id, observed_kind, canonical_cwd, bound_at, \
    last_readback_at, revision";
const ADAPTIVE_ADMISSION_COLUMNS: &str = "project_id, mini_project_id, current_window, \
    clean_observation_streak, last_observation_id, revision, updated_at";

fn read_topology_node(row: &Row<'_>) -> RepositoryResult<SessionTopologyNode> {
    let mini_project_id: Option<String> = row.get(2).map_err(backend)?;
    let parent_id: Option<String> = row.get(7).map_err(backend)?;
    Ok(SessionTopologyNode {
        id: TopologyNodeId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        mini_project_id: mini_project_id
            .as_deref()
            .map(MiniProjectId::parse)
            .transpose()?,
        topology: TopologySnapshot {
            spec_id: TopologySpecId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
            version: read_version(row.get::<_, i64>(4).map_err(backend)?)?,
            canonical_hash: ContentHash::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        },
        kind: TopologyKindKey::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        parent_id: parent_id
            .as_deref()
            .map(TopologyNodeId::parse)
            .transpose()?,
        lifecycle: TopologyLifecycle::parse(&row.get::<_, String>(8).map_err(backend)?)?,
        placement: PlacementState::parse(&row.get::<_, String>(9).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(10).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(11).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(12).map_err(backend)?)?,
        task_id: row
            .get::<_, Option<String>>(13)
            .map_err(backend)?
            .as_deref()
            .map(TaskId::parse)
            .transpose()?,
    })
}

fn read_seat_binding(row: &Row<'_>) -> RepositoryResult<SeatBinding> {
    let custom_display_name: Option<String> = row.get(8).map_err(backend)?;
    let task_id: Option<String> = row.get(9).map_err(backend)?;
    let team_run_id: Option<String> = row.get(10).map_err(backend)?;
    Ok(SeatBinding {
        id: SeatBindingId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        topology_node_id: TopologyNodeId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        role_slot_id: RoleSlotId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        role: CatalogRoleRef {
            catalog_id: RoleCatalogId::parse(&row.get::<_, String>(4).map_err(backend)?)?,
            catalog_revision: read_version(row.get::<_, i64>(5).map_err(backend)?)?,
            role_code: RoleCode::parse(&row.get::<_, String>(6).map_err(backend)?)?,
            standard_title: ExternalName::parse(&row.get::<_, String>(7).map_err(backend)?)?,
            custom_display_name: custom_display_name
                .as_deref()
                .map(ExternalName::parse)
                .transpose()?,
        },
        task_id: task_id.as_deref().map(TaskId::parse).transpose()?,
        team_run_id: team_run_id.as_deref().map(TeamRunId::parse).transpose()?,
        lifecycle: TopologyLifecycle::parse(&row.get::<_, String>(11).map_err(backend)?)?,
        attach_deadline: read_timestamp(&row.get::<_, String>(12).map_err(backend)?)?,
        last_attached_at: read_optional_timestamp(row, 13)?,
        last_activity_at: read_optional_timestamp(row, 14)?,
        parent_seat_binding_id: row
            .get::<_, Option<String>>(15)
            .map_err(backend)?
            .as_deref()
            .map(SeatBindingId::parse)
            .transpose()?,
        released_at: read_optional_timestamp(row, 16)?,
        replaced_by: row
            .get::<_, Option<String>>(17)
            .map_err(backend)?
            .as_deref()
            .map(SeatBindingId::parse)
            .transpose()?,
        runtime_reported: row
            .get::<_, Option<String>>(18)
            .map_err(backend)?
            .as_deref()
            .map(ObservedRunState::parse)
            .transpose()?,
        revision: revision_of(row.get::<_, i64>(19).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(20).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(21).map_err(backend)?)?,
    })
}

/// Read one nullable timestamp column.
fn read_optional_timestamp(row: &Row<'_>, index: usize) -> RepositoryResult<Option<Timestamp>> {
    row.get::<_, Option<String>>(index)
        .map_err(backend)?
        .as_deref()
        .map(read_timestamp)
        .transpose()
}

fn read_native_container_binding(row: &Row<'_>) -> RepositoryResult<NativeContainerBinding> {
    let canonical_cwd: Option<String> = row.get(8).map_err(backend)?;
    let generation: i64 = row.get(5).map_err(backend)?;
    Ok(NativeContainerBinding {
        topology_node_id: TopologyNodeId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        container_binding_id: ExternalId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        identity: NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(&row.get::<_, String>(3).map_err(backend)?)?,
            host: ExternalName::parse(&row.get::<_, String>(4).map_err(backend)?)?,
            generation: u64::try_from(generation).map_err(|_| {
                DomainError::invalid("native container generation", "is outside the stored range")
            })?,
            native_id: ExternalId::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        },
        observed_kind: ObservedContainerKind::parse(&row.get::<_, String>(7).map_err(backend)?)?,
        canonical_cwd: canonical_cwd
            .as_deref()
            .map(ExternalName::parse)
            .transpose()?,
        bound_at: read_timestamp(&row.get::<_, String>(9).map_err(backend)?)?,
        last_readback_at: read_timestamp(&row.get::<_, String>(10).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(11).map_err(backend)?)?,
    })
}

fn read_adaptive_admission(row: &Row<'_>) -> RepositoryResult<AdaptiveAdmissionState> {
    let last_observation_id: Option<String> = row.get(4).map_err(backend)?;
    Ok(AdaptiveAdmissionState {
        project_id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        mini_project_id: MiniProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        current_window: u32::try_from(row.get::<_, i64>(2).map_err(backend)?).map_err(|_| {
            RepositoryError::Backend {
                detail: "stored adaptive window is out of range".to_owned(),
            }
        })?,
        clean_observation_streak: u32::try_from(row.get::<_, i64>(3).map_err(backend)?).map_err(
            |_| RepositoryError::Backend {
                detail: "stored clean-observation streak is out of range".to_owned(),
            },
        )?,
        last_observation_id: last_observation_id
            .as_deref()
            .map(ExternalId::parse)
            .transpose()?,
        revision: revision_of(row.get::<_, i64>(5).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
    })
}

fn validate_adaptive_values(
    current_window: u32,
    clean_observation_streak: u32,
) -> RepositoryResult<()> {
    if current_window == 0 || clean_observation_streak > 1 {
        return Err(DomainError::invalid(
            "AdaptiveAdmissionState",
            "requires a positive window and a clean-observation streak no greater than one",
        )
        .into());
    }
    Ok(())
}

/// A published topology specification is project configuration, never
/// operational state, so it is classifiable and defaults to `project_shared`.
const TOPOLOGY_SPEC_TIER: ShareabilityTier = ShareabilityTier::ProjectKnowledge;

/// A published role catalog is project configuration on the same footing.
const ROLE_CATALOG_TIER: ShareabilityTier = ShareabilityTier::ProjectKnowledge;

/// Rebuild one stamp from its three stored columns.
///
/// The pairing is re-proved on the way out as well as on the way in, so a row
/// edited around the repository cannot read back as a valid classification.
fn stored_shareability(
    (class, classifier, provenance): (String, Option<String>, String),
) -> RepositoryResult<Shareability> {
    let stamp = Shareability {
        class: ShareabilityClass::parse(&class)?,
        classifier: match classifier {
            None => ShareabilityClassifier::TypeDefaultRule,
            Some(name) => ShareabilityClassifier::Human(ExternalName::parse(&name)?),
        },
        provenance: ShareabilityProvenance::parse(&provenance)?,
    };
    stamp.validate_for(ShareabilityTier::ProjectKnowledge)?;
    Ok(stamp)
}

fn topology_spec_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    snapshot: &TopologySnapshot,
) -> RepositoryResult<ProjectSessionTopologySpec> {
    let found: Option<(String, String)> = transaction
        .query_row(
            "SELECT definition, definition_hash FROM topology_specs
             WHERE project_id = ?1 AND spec_id = ?2 AND version = ?3",
            params![
                project_id.to_string(),
                snapshot.spec_id.to_string(),
                version_column(snapshot.version)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (json, hash) = found.ok_or(RepositoryError::NotFound {
        subject: "topology specification",
    })?;
    let hash = ContentHash::parse(&hash)?;
    if hash != snapshot.canonical_hash {
        return Err(conflict(
            "topology specification",
            "the pinned canonical hash does not match the published revision",
        ));
    }
    stored_document(&json, hash.as_str())
}

/// Move every existing node in one epic onto the target immutable topology
/// stamp, after proving that the target can still represent the exact hierarchy,
/// native containers and seats already held.
///
/// This deliberately changes no node identity, lifecycle, placement or revision.
/// A topology upgrade changes the vocabulary those same nodes cite; it is not a
/// second node mutation and must never make optimistic-concurrency clients think
/// a seat or container moved.
fn repin_mini_project_nodes_in(
    transaction: &Transaction<'_>,
    snapshot: &MiniProjectTopologySnapshot,
) -> RepositoryResult<usize> {
    let spec = topology_spec_in(transaction, snapshot.project_id, &snapshot.topology)?;
    let mut statement = transaction
        .prepare(&format!(
            "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
             WHERE project_id = ?1 AND mini_project_id = ?2
             ORDER BY created_at, id"
        ))
        .map_err(backend)?;
    let mut rows = statement
        .query(params![
            snapshot.project_id.to_string(),
            snapshot.mini_project_id.to_string()
        ])
        .map_err(backend)?;
    let mut nodes = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        nodes.push(read_topology_node(row)?);
    }
    drop(rows);
    drop(statement);

    for node in &nodes {
        let declared = spec
            .node_kinds
            .iter()
            .find(|candidate| candidate.kind == node.kind)
            .ok_or_else(|| {
                conflict(
                    "topology upgrade",
                    "the target specification does not declare an existing epic node kind",
                )
            })?;

        match node.parent_id {
            Some(parent_id) => {
                let parent_kind: Option<String> = transaction
                    .query_row(
                        "SELECT kind FROM topology_nodes WHERE project_id = ?1 AND id = ?2",
                        params![snapshot.project_id.to_string(), parent_id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?;
                let parent_kind = parent_kind
                    .as_deref()
                    .map(TopologyKindKey::parse)
                    .transpose()?
                    .ok_or(RepositoryError::NotFound {
                        subject: "topology parent",
                    })?;
                if !declared.allowed_parents.contains(&parent_kind) {
                    return Err(conflict(
                        "topology upgrade",
                        "the target specification does not permit an existing parent-child relation",
                    ));
                }
            }
            None if node.kind != spec.root_kind => {
                return Err(conflict(
                    "topology upgrade",
                    "the target specification does not permit an existing root relation",
                ));
            }
            None => {}
        }

        let observed_kind: Option<String> = transaction
            .query_row(
                "SELECT observed_kind FROM topology_node_containers
                 WHERE project_id = ?1 AND topology_node_id = ?2",
                params![snapshot.project_id.to_string(), node.id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let required_projection = observed_kind
            .as_deref()
            .map(ObservedContainerKind::parse)
            .transpose()?
            .map(|observed| match observed {
                ObservedContainerKind::Project => NodeProjectionCapability::NativeRoot,
                ObservedContainerKind::Workspace => NodeProjectionCapability::NativeChild,
            });
        if required_projection
            .is_some_and(|required| !declared.projection_capabilities.contains(&required))
        {
            return Err(conflict(
                "topology upgrade",
                "the target specification cannot project an existing native container",
            ));
        }

        let seats: i64 = transaction
            .query_row(
                "SELECT count(*) FROM seat_bindings
                 WHERE project_id = ?1 AND topology_node_id = ?2",
                params![snapshot.project_id.to_string(), node.id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if seats > 0
            && !declared
                .projection_capabilities
                .contains(&NodeProjectionCapability::SessionHost)
        {
            return Err(conflict(
                "topology upgrade",
                "the target specification cannot host an existing seat",
            ));
        }
    }

    transaction
        .execute(
            "UPDATE topology_nodes
             SET spec_id = ?1, spec_version = ?2, spec_hash = ?3, updated_at = ?4
             WHERE project_id = ?5 AND mini_project_id = ?6
               AND (spec_id <> ?1 OR spec_version <> ?2 OR spec_hash <> ?3)",
            params![
                snapshot.topology.spec_id.to_string(),
                version_column(snapshot.topology.version),
                snapshot.topology.canonical_hash.as_str(),
                text(snapshot.pinned_at),
                snapshot.project_id.to_string(),
                snapshot.mini_project_id.to_string(),
            ],
        )
        .map_err(backend)
}

impl SqliteStore {
    /// Repair a legacy partial topology upgrade whose epic pin committed but
    /// whose existing nodes still cite the previous immutable revision.
    ///
    /// This is intentionally a startup-only convergence primitive. It creates
    /// no node, seat, native binding, event or command receipt and changes no
    /// aggregate revision. Every repair is revalidated through the same
    /// hierarchy/capability proof as a fresh atomic repin; an incompatible
    /// mismatch fails closed and rolls back the whole sweep.
    ///
    /// # Errors
    /// Returns the repository's typed refusal if a target specification is
    /// missing, incompatible, or the repair cannot be committed atomically.
    pub fn reconcile_mini_project_topology_nodes(
        &self,
    ) -> RepositoryResult<Vec<MiniProjectTopologySnapshot>> {
        let transaction = self.begin()?;
        let mut statement = transaction
            .prepare(
                "SELECT snapshot.project_id, snapshot.mini_project_id, snapshot.spec_id,
                        snapshot.version, snapshot.canonical_hash, snapshot.pinned_at
                 FROM mini_project_topology_snapshots snapshot
                 WHERE EXISTS (
                     SELECT 1 FROM topology_nodes node
                     WHERE node.project_id = snapshot.project_id
                       AND node.mini_project_id = snapshot.mini_project_id
                       AND (node.spec_id <> snapshot.spec_id
                            OR node.spec_version <> snapshot.version
                            OR node.spec_hash <> snapshot.canonical_hash)
                 )
                 ORDER BY snapshot.project_id, snapshot.mini_project_id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut mismatches = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            mismatches.push(MiniProjectTopologySnapshot {
                project_id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                mini_project_id: MiniProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                topology: TopologySnapshot {
                    spec_id: TopologySpecId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                    version: read_version(row.get::<_, i64>(3).map_err(backend)?)?,
                    canonical_hash: ContentHash::parse(&row.get::<_, String>(4).map_err(backend)?)?,
                },
                pinned_at: read_timestamp(&row.get::<_, String>(5).map_err(backend)?)?,
            });
        }
        drop(rows);
        drop(statement);

        for snapshot in &mismatches {
            repin_mini_project_nodes_in(&transaction, snapshot)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(mismatches)
    }
}

fn role_catalog_in(
    transaction: &Transaction<'_>,
    catalog_id: RoleCatalogId,
    version: SpecVersion,
) -> RepositoryResult<RoleCatalogRevision> {
    let found: Option<(String, String)> = transaction
        .query_row(
            "SELECT definition, definition_hash FROM role_catalog_revisions
             WHERE catalog_id = ?1 AND version = ?2",
            params![catalog_id.to_string(), version_column(version)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (json, hash) = found.ok_or(RepositoryError::NotFound {
        subject: "role catalog",
    })?;
    stored_document(&json, &hash)
}

/// Project Core Team configuration.
///
/// Inherent rather than a trait: there is one implementation, and a port here
/// would be a second thing to keep in agreement with the two statements below.
impl SqliteStore {
    /// Publish the next immutable Core Team revision for one project.
    ///
    /// The version is checked against what is already stored inside the same
    /// transaction rather than trusted from the caller. Two applies racing on
    /// the same project would otherwise both read version *n*, both compute
    /// *n+1*, and the second would land on the primary key with a message about
    /// a unique index rather than about the roster it failed to publish.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the version is not the next
    /// one for this project.
    pub fn publish_core_team_revision(
        &self,
        revision: &StoredCoreTeamRevision,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let current: Option<i64> = transaction
            .query_row(
                "SELECT MAX(version) FROM core_team_revisions WHERE project_id = ?1",
                params![revision.project_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?
            .flatten();
        let expected = current.map_or(1, |version| version.saturating_add(1));
        if version_column(revision.version) != expected {
            return Err(RepositoryError::Conflict {
                subject: "core team revision",
                rule: "must be the next revision for this project",
            });
        }
        transaction
            .execute(
                "INSERT INTO core_team_revisions
                     (project_id, version, catalog_hash, seats, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    revision.project_id.to_string(),
                    version_column(revision.version),
                    revision.catalog_hash.as_str(),
                    revision.seats.to_string(),
                    text(revision.published_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Publish the next immutable revision of one Advisor profile or Committee
    /// template.
    ///
    /// The gap check is per `(project, family, profile_id)`: version one starts
    /// a profile, and every later version must be exactly the next one. A caller
    /// that skipped a version would publish a revision whose predecessor never
    /// existed, and a run pinning "the previous revision" would then have
    /// nothing to read.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the version is not the next
    /// one for that profile, or a backend error.
    pub fn publish_consultation_profile_revision(
        &self,
        revision: &StoredConsultationProfileRevision,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let current: Option<i64> = transaction
            .query_row(
                "SELECT MAX(version) FROM consultation_profile_revisions
                 WHERE project_id = ?1 AND family = ?2 AND profile_id = ?3",
                params![
                    revision.project_id.to_string(),
                    revision.family.as_str(),
                    revision.profile_id.as_str(),
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?
            .flatten();
        let expected = current.map_or(1, |version| version.saturating_add(1));
        if version_column(revision.version) != expected {
            return Err(RepositoryError::Conflict {
                subject: "consultation profile revision",
                rule: "must be the next revision for this profile",
            });
        }
        transaction
            .execute(
                "INSERT INTO consultation_profile_revisions
                     (project_id, family, profile_id, version, name, definition,
                      definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    revision.project_id.to_string(),
                    revision.family.as_str(),
                    revision.profile_id.as_str(),
                    version_column(revision.version),
                    revision.name.as_str(),
                    revision.definition.as_str(),
                    revision.definition_hash.as_str(),
                    text(revision.published_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Every published revision of one consultation family in a project.
    ///
    /// Ordered by profile then version, oldest first, so a catalog read is a
    /// stable projection rather than whatever order the pages happen to be in.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_consultation_profile_revisions(
        &self,
        project_id: ProjectId,
        family: ConsultationFamily,
    ) -> RepositoryResult<Vec<StoredConsultationProfileRevision>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT profile_id, version, name, definition, definition_hash, created_at
                 FROM consultation_profile_revisions
                 WHERE project_id = ?1 AND family = ?2
                 ORDER BY profile_id ASC, version ASC",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(params![project_id.to_string(), family.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(
                |(profile_id, version, name, definition, definition_hash, created_at)| {
                    // Re-admitted rather than merely parsed: `from_stored`
                    // proves the bytes are still canonical *and* that they
                    // still hash to the digest published with them. A silently
                    // rewritten definition would otherwise be served to a run
                    // that pinned the original.
                    let digest = ContentHash::parse(&definition_hash)?;
                    let document = CanonicalDocument::from_stored(&definition, &digest)?;
                    Ok(StoredConsultationProfileRevision {
                        project_id,
                        family,
                        profile_id,
                        version: read_version(version)?,
                        name: ExternalName::parse(&name)?,
                        definition: document.json().to_owned(),
                        definition_hash: digest,
                        published_at: read_timestamp(&created_at)?,
                    })
                },
            )
            .collect()
    }

    /// Atomically freeze one consultation, its dedicated topology node and all
    /// template-declared logical seats before any native launch.
    ///
    /// # Errors
    /// Refuses any duplicate run/node/seat, cross-project reference or backend
    /// failure. No partial topology survives a refused insert.
    pub fn create_consultation_run(
        &self,
        run: &StoredConsultationRun,
        node: &NewSessionTopologyNode,
        seats: &[(&StoredConsultationSeat, &NewSeatBinding)],
    ) -> RepositoryResult<()> {
        if run.project_id != node.project_id
            || run.topology_node_id != node.id
            || node.mini_project_id != Some(run.mini_project_id)
        {
            return Err(RepositoryError::Conflict {
                subject: "consultation run",
                rule: "the frozen run and topology node do not describe one scope",
            });
        }
        let verified_context = CanonicalDocument::from_serializable(&run.context)?;
        if ContentHash::of(run.question.as_str().as_bytes()) != run.question_hash
            || verified_context.hash() != &run.context_hash
        {
            return Err(RepositoryError::Conflict {
                subject: "consultation run",
                rule: "frozen input does not match its digest",
            });
        }
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO topology_nodes
                     (id, project_id, mini_project_id, spec_id, spec_version, spec_hash,
                      kind, parent_id, lifecycle, placement, task_id, revision,
                      created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                         'active', 'unbound', NULL, 1, ?9, ?9)",
                params![
                    node.id.to_string(),
                    node.project_id.to_string(),
                    node.mini_project_id.map(|id| id.to_string()),
                    node.topology.spec_id.to_string(),
                    version_column(node.topology.version),
                    node.topology.canonical_hash.as_str(),
                    node.kind.as_str(),
                    node.parent_id.map(|id| id.to_string()),
                    text(node.created_at),
                ],
            )
            .map_err(backend)?;

        let context = canonical_json(&run.context, "consultation context")?;
        let result = run
            .result
            .as_ref()
            .map(|value| canonical_json(value, "consultation result"))
            .transpose()?;
        transaction
            .execute(
                "INSERT INTO consultation_runs
                     (run_id, project_id, mini_project_id, family, profile_id,
                      profile_version, definition_hash, question, question_hash,
                      context, context_hash, caller_seat_binding_id, topology_node_id,
                      invoke_key, invoke_intent_hash, state, round, result, result_hash, revision, created_at,
                      updated_at, settled_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                params![
                    run.id.as_text(),
                    run.project_id.to_string(),
                    run.mini_project_id.to_string(),
                    run.id.family().as_str(),
                    run.profile_id,
                    version_column(run.profile_version),
                    run.definition_hash.as_str(),
                    run.question.as_str(),
                    run.question_hash.as_str(),
                    context,
                    run.context_hash.as_str(),
                    run.caller_seat_binding_id.to_string(),
                    run.topology_node_id.to_string(),
                    run.invoke_key.as_str(),
                    run.invoke_intent_hash.as_str(),
                    run.state.as_str(),
                    i64::from(run.round),
                    result,
                    run.result_hash.as_ref().map(ContentHash::as_str),
                    i64::try_from(run.revision.get()).unwrap_or(i64::MAX),
                    text(run.created_at),
                    text(run.updated_at),
                    run.settled_at.map(text),
                ],
            )
            .map_err(backend)?;

        for (seat, binding) in seats {
            if seat.run_id != run.id
                || binding.id != seat.seat_binding_id
                || binding.project_id != run.project_id
                || binding.topology_node_id != node.id
                || binding.role_slot_id != seat.role_slot_id
            {
                return Err(RepositoryError::Conflict {
                    subject: "consultation seat",
                    rule: "the frozen seat and SeatBinding do not describe one slot",
                });
            }
            transaction
                .execute(
                    "INSERT INTO seat_bindings
                         (id, project_id, topology_node_id, role_slot_id,
                          role_catalog_id, role_catalog_version, role_code,
                          standard_title, custom_display_name, task_id, team_run_id,
                          lifecycle, attach_deadline, last_attached_at, last_activity_at,
                          parent_seat_binding_id, released_at,
                          replaced_by_seat_binding_id, runtime_reported,
                          revision, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                             NULL, NULL, 'active', ?10, NULL, NULL, ?11,
                             NULL, NULL, NULL, 1, ?12, ?12)",
                    params![
                        binding.id.to_string(),
                        binding.project_id.to_string(),
                        binding.topology_node_id.to_string(),
                        binding.role_slot_id.as_str(),
                        binding.role.catalog_id.to_string(),
                        version_column(binding.role.catalog_revision),
                        binding.role.role_code.as_str(),
                        binding.role.standard_title.as_str(),
                        binding
                            .role
                            .custom_display_name
                            .as_ref()
                            .map(ExternalName::as_str),
                        text(binding.attach_deadline),
                        binding.parent_seat_binding_id.map(|id| id.to_string()),
                        text(binding.created_at),
                    ],
                )
                .map_err(backend)?;
            let model = serde_json::to_string(&seat.model_rung).map_err(|error| {
                RepositoryError::Backend {
                    detail: format!("a consultation model rung could not be encoded: {error}"),
                }
            })?;
            transaction
                .execute(
                    "INSERT INTO consultation_seats
                         (run_id, project_id, role_slot_id, committee_role,
                          logical_role, seat_binding_id, model_rung, occupancy_generation,
                          runtime_kind, host, generation, native_id,
                          provider_session_id, observed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                             NULL, NULL, NULL, NULL, NULL, NULL)",
                    params![
                        run.id.as_text(),
                        run.project_id.to_string(),
                        seat.role_slot_id.as_str(),
                        seat.committee_role.map(CommitteeRole::as_str),
                        seat.logical_role.as_str(),
                        seat.seat_binding_id.to_string(),
                        model,
                        i64::try_from(seat.occupancy_generation).unwrap_or(i64::MAX),
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// One family-qualified consultation in one project.
    pub fn get_consultation_run(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
    ) -> RepositoryResult<Option<StoredConsultationRun>> {
        let row = self
            .connection
            .query_row(
                "SELECT mini_project_id, profile_id, profile_version,
                        definition_hash, question, question_hash, context,
                        context_hash, caller_seat_binding_id, topology_node_id,
                        invoke_key, invoke_intent_hash, state, round, result, result_hash, revision, created_at,
                        updated_at, settled_at
                 FROM consultation_runs
                 WHERE project_id = ?1 AND run_id = ?2 AND family = ?3",
                params![project_id.to_string(), run_id.as_text(), run_id.family().as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, i64>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row.get::<_, Option<String>>(15)?,
                        row.get::<_, i64>(16)?,
                        row.get::<_, String>(17)?,
                        row.get::<_, String>(18)?,
                        row.get::<_, Option<String>>(19)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(|columns| read_consultation_run(project_id, run_id, columns))
            .transpose()
    }

    /// The run an invocation key already froze, if any.
    pub fn get_consultation_run_by_key(
        &self,
        project_id: ProjectId,
        key: &IdempotencyKey,
    ) -> RepositoryResult<Option<StoredConsultationRun>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT run_id, family FROM consultation_runs
                 WHERE project_id = ?1 AND invoke_key = ?2",
                params![project_id.to_string(), key.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((run_id, family)) = found else {
            return Ok(None);
        };
        let family = ConsultationFamily::parse(&family)?;
        self.get_consultation_run(project_id, consultation_run_id(family, &run_id)?)
    }

    /// Every run of one family in an epic, oldest first.
    pub fn list_consultation_runs(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        family: ConsultationFamily,
    ) -> RepositoryResult<Vec<StoredConsultationRun>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT run_id FROM consultation_runs
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND family = ?3
                 ORDER BY created_at, run_id",
            )
            .map_err(backend)?;
        let ids = statement
            .query_map(
                params![
                    project_id.to_string(),
                    mini_project_id.to_string(),
                    family.as_str()
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        ids.into_iter()
            .map(|id| {
                let run_id = consultation_run_id(family, &id)?;
                self.get_consultation_run(project_id, run_id)?
                    .ok_or(RepositoryError::NotFound {
                        subject: "consultation run",
                    })
            })
            .collect()
    }

    /// The frozen seats of one consultation, in slot order.
    pub fn list_consultation_seats(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
    ) -> RepositoryResult<Vec<StoredConsultationSeat>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT role_slot_id, committee_role, logical_role,
                        seat_binding_id, model_rung, occupancy_generation, runtime_kind, host,
                        generation, native_id, provider_session_id, observed_at
                 FROM consultation_seats
                 WHERE project_id = ?1 AND run_id = ?2
                 ORDER BY role_slot_id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(params![project_id.to_string(), run_id.as_text()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| read_consultation_seat(run_id, row))
            .collect()
    }

    /// Read one consultation seat by its persistent topology SeatBinding.
    pub fn get_consultation_seat_by_binding(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
    ) -> RepositoryResult<Option<StoredConsultationSeat>> {
        let row = self
            .connection
            .query_row(
                "SELECT s.run_id, r.family, s.role_slot_id, s.committee_role,
                        s.logical_role, s.seat_binding_id, s.model_rung,
                        s.occupancy_generation, s.runtime_kind, s.host, s.generation, s.native_id,
                        s.provider_session_id, s.observed_at
                 FROM consultation_seats AS s
                 JOIN consultation_runs AS r
                   ON r.project_id = s.project_id AND r.run_id = s.run_id
                 WHERE s.project_id = ?1 AND s.seat_binding_id = ?2",
                params![project_id.to_string(), seat_binding_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        (
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, i64>(7)?,
                            row.get::<_, Option<String>>(8)?,
                            row.get::<_, Option<String>>(9)?,
                            row.get::<_, Option<i64>>(10)?,
                            row.get::<_, Option<String>>(11)?,
                            row.get::<_, Option<String>>(12)?,
                            row.get::<_, Option<String>>(13)?,
                        ),
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(|(run_id, family, columns)| {
            let family = kontor_core::consultation::ConsultationFamily::parse(&family)?;
            read_consultation_seat(consultation_run_id(family, &run_id)?, columns)
        })
        .transpose()
    }

    /// Persist the exact native identity a consultation launch read back.
    /// Repeating the same observation is a replay; a different identity for the
    /// same frozen seat is a conflict.
    pub fn bind_consultation_seat(
        &self,
        project_id: ProjectId,
        seat: &StoredConsultationSeat,
    ) -> RepositoryResult<()> {
        let identity = seat
            .native_identity
            .as_ref()
            .ok_or(RepositoryError::Conflict {
                subject: "consultation seat",
                rule: "a native readback is required before binding",
            })?;
        let transaction = self.begin()?;
        let existing: Option<(String, String, i64, String)> = transaction
            .query_row(
                "SELECT runtime_kind, host, generation, native_id
                 FROM consultation_seats
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND runtime_kind IS NOT NULL",
                params![
                    project_id.to_string(),
                    seat.run_id.as_text(),
                    seat.role_slot_id.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((runtime, host, generation, native)) = existing {
            if runtime != identity.runtime_kind.as_str()
                || host != identity.host.as_str()
                || u64::try_from(generation).ok() != Some(identity.generation)
                || native != identity.native_id.as_str()
            {
                return Err(RepositoryError::Conflict {
                    subject: "consultation seat",
                    rule: "a frozen seat cannot move to another native session",
                });
            }
            transaction.commit().map_err(backend)?;
            return Ok(());
        }
        let changed = transaction
            .execute(
                "UPDATE consultation_seats
                 SET runtime_kind = ?4, host = ?5, generation = ?6,
                     native_id = ?7, provider_session_id = ?8, observed_at = ?9
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND runtime_kind IS NULL",
                params![
                    project_id.to_string(),
                    seat.run_id.as_text(),
                    seat.role_slot_id.as_str(),
                    identity.runtime_kind.as_str(),
                    identity.host.as_str(),
                    i64::try_from(identity.generation).unwrap_or(i64::MAX),
                    identity.native_id.as_str(),
                    seat.provider_session_id.as_ref().map(ExternalId::as_str),
                    seat.observed_at.map(text),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::NotFound {
                subject: "consultation seat",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Read the receipt-first recovery attempt for one exact predecessor.
    pub fn get_consultation_recovery_attempt(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
        role_slot_id: &RoleSlotId,
        predecessor_native_id: &ExternalId,
    ) -> RepositoryResult<Option<StoredConsultationRecoveryAttempt>> {
        let row = self
            .connection
            .query_row(
                "SELECT seat_binding_id, predecessor_occupancy_generation,
                        successor_occupancy_generation, predecessor_run_revision,
                        prepared_run_revision, recovery_reason, request_intent_hash,
                        recovery_profile, recovery_profile_hash, selected_model_rung,
                        state, successor_runtime_kind, successor_host,
                        successor_generation, successor_native_id,
                        successor_provider_session, successor_observed_at,
                        prepared_at, retired_at, installed_at
                 FROM consultation_seat_recovery_attempts
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND predecessor_native_id = ?4",
                params![
                    project_id.to_string(),
                    run_id.as_text(),
                    role_slot_id.as_str(),
                    predecessor_native_id.as_str(),
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, Option<String>>(12)?,
                        row.get::<_, Option<i64>>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row.get::<_, Option<String>>(15)?,
                        row.get::<_, Option<String>>(16)?,
                        row.get::<_, String>(17)?,
                        row.get::<_, Option<String>>(18)?,
                        row.get::<_, Option<String>>(19)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(
            |(
                seat_binding_id,
                predecessor_occupancy_generation,
                successor_occupancy_generation,
                predecessor_run_revision,
                prepared_run_revision,
                recovery_reason,
                request_intent_hash,
                recovery_profile,
                recovery_profile_hash,
                selected_model_rung,
                state,
                successor_runtime_kind,
                successor_host,
                successor_generation,
                successor_native_id,
                successor_provider_session,
                successor_observed_at,
                prepared_at,
                retired_at,
                installed_at,
            )| {
                let successor_native_identity = match (
                    successor_runtime_kind,
                    successor_host,
                    successor_generation,
                    successor_native_id,
                ) {
                    (Some(runtime_kind), Some(host), Some(generation), Some(native_id)) => {
                        Some(NativeRuntimeIdentity {
                            runtime_kind: RuntimeKindKey::parse(&runtime_kind)?,
                            host: ExternalName::parse(&host)?,
                            generation: u64::try_from(generation).map_err(|_| {
                                RepositoryError::Conflict {
                                    subject: "consultation recovery attempt",
                                    rule: "successor runtime generation is negative",
                                }
                            })?,
                            native_id: ExternalId::parse(&native_id)?,
                        })
                    }
                    (None, None, None, None) => None,
                    _ => {
                        return Err(RepositoryError::Conflict {
                            subject: "consultation recovery attempt",
                            rule: "successor native identity is partially present",
                        });
                    }
                };
                let profile_hash = ContentHash::parse(&recovery_profile_hash)?;
                let profile = CanonicalDocument::from_stored(&recovery_profile, &profile_hash)?;
                Ok(StoredConsultationRecoveryAttempt {
                    project_id,
                    run_id,
                    role_slot_id: role_slot_id.clone(),
                    seat_binding_id: SeatBindingId::parse(&seat_binding_id)?,
                    predecessor_native_id: predecessor_native_id.clone(),
                    predecessor_occupancy_generation: u64::try_from(
                        predecessor_occupancy_generation,
                    )
                    .map_err(|_| RepositoryError::Conflict {
                        subject: "consultation recovery attempt",
                        rule: "predecessor occupancy generation is negative",
                    })?,
                    successor_occupancy_generation: u64::try_from(
                        successor_occupancy_generation,
                    )
                    .map_err(|_| RepositoryError::Conflict {
                        subject: "consultation recovery attempt",
                        rule: "successor occupancy generation is negative",
                    })?,
                    predecessor_run_revision: AggregateRevision::parse(
                        u64::try_from(predecessor_run_revision).map_err(|_| {
                            RepositoryError::Conflict {
                                subject: "consultation recovery attempt",
                                rule: "predecessor run revision is negative",
                            }
                        })?,
                    )?,
                    prepared_run_revision: AggregateRevision::parse(
                        u64::try_from(prepared_run_revision).map_err(|_| {
                            RepositoryError::Conflict {
                                subject: "consultation recovery attempt",
                                rule: "prepared run revision is negative",
                            }
                        })?,
                    )?,
                    recovery_reason,
                    request_intent_hash: ContentHash::parse(&request_intent_hash)?,
                    recovery_profile: serde_json::from_str(profile.json()).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!("a recovery profile could not be decoded: {error}"),
                        }
                    })?,
                    recovery_profile_hash: profile_hash,
                    selected_model_rung: serde_json::from_str(&selected_model_rung).map_err(
                        |error| RepositoryError::Backend {
                            detail: format!(
                                "a selected consultation recovery route could not be decoded: {error}"
                            ),
                        },
                    )?,
                    state,
                    successor_native_identity,
                    successor_provider_session_id: successor_provider_session
                        .map(|value| ExternalId::parse(&value))
                        .transpose()?,
                    successor_observed_at: successor_observed_at
                        .map(|value| read_timestamp(&value))
                        .transpose()?,
                    prepared_at: read_timestamp(&prepared_at)?,
                    retired_at: retired_at.map(|value| read_timestamp(&value)).transpose()?,
                    installed_at: installed_at
                        .map(|value| read_timestamp(&value))
                        .transpose()?,
                })
            },
        )
        .transpose()
    }

    /// Fence one exact predecessor and persist the selected recovery policy
    /// before any runtime archive or launch effect.
    pub fn prepare_consultation_recovery_attempt(
        &self,
        request: &NewConsultationRecoveryAttempt,
    ) -> RepositoryResult<StoredConsultationRecoveryAttempt> {
        let project_id = request.project_id;
        let predecessor = &request.predecessor;
        let expected_revision = request.expected_revision;
        let recovery_reason = request.recovery_reason.as_str();
        let recovery_profile = &request.recovery_profile;
        let selected_model_rung = &request.selected_model_rung;
        let prepared_at = request.prepared_at;
        let predecessor_identity =
            predecessor
                .native_identity
                .as_ref()
                .ok_or(RepositoryError::Conflict {
                    subject: "consultation recovery attempt",
                    rule: "the predecessor has no native identity",
                })?;
        if let Some(existing) = self.get_consultation_recovery_attempt(
            project_id,
            predecessor.run_id,
            &predecessor.role_slot_id,
            &predecessor_identity.native_id,
        )? {
            if existing.recovery_reason != recovery_reason
                || existing.request_intent_hash != request.request_intent_hash
                || existing.recovery_profile_hash != *recovery_profile.hash()
                || existing.selected_model_rung != *selected_model_rung
            {
                return Err(RepositoryError::Conflict {
                    subject: "consultation recovery attempt",
                    rule: "the predecessor already has a different fenced recovery intent",
                });
            }
            return Ok(existing);
        }
        let successor_occupancy_generation = predecessor
            .occupancy_generation
            .checked_add(1)
            .ok_or(RepositoryError::Conflict {
                subject: "consultation recovery attempt",
                rule: "the occupancy generation overflowed",
            })?;
        let prepared_run_revision = expected_revision.next()?;
        let selected_model_rung = serde_json::to_string(selected_model_rung).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a selected recovery route could not be encoded: {error}"),
            }
        })?;
        let transaction = self.begin()?;
        let active: Option<(String, i64)> = transaction
            .query_row(
                "SELECT native_id, occupancy_generation FROM consultation_seats
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3",
                params![
                    project_id.to_string(),
                    predecessor.run_id.as_text(),
                    predecessor.role_slot_id.as_str(),
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if active
            .as_ref()
            .map(|(native, generation)| (native.as_str(), u64::try_from(*generation).ok()))
            != Some((
                predecessor_identity.native_id.as_str(),
                Some(predecessor.occupancy_generation),
            ))
        {
            return Err(RepositoryError::Conflict {
                subject: "consultation recovery attempt",
                rule: "the active predecessor or occupancy generation moved",
            });
        }
        transaction
            .execute(
                "INSERT INTO consultation_seat_recovery_attempts
                     (project_id, run_id, role_slot_id, seat_binding_id,
                      predecessor_native_id, predecessor_occupancy_generation,
                      successor_occupancy_generation, predecessor_run_revision,
                      prepared_run_revision, recovery_reason, request_intent_hash,
                      recovery_profile, recovery_profile_hash, selected_model_rung,
                      state, prepared_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, ?14, 'prepared', ?15)",
                params![
                    project_id.to_string(),
                    predecessor.run_id.as_text(),
                    predecessor.role_slot_id.as_str(),
                    predecessor.seat_binding_id.to_string(),
                    predecessor_identity.native_id.as_str(),
                    i64::try_from(predecessor.occupancy_generation).unwrap_or(i64::MAX),
                    i64::try_from(successor_occupancy_generation).unwrap_or(i64::MAX),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    i64::try_from(prepared_run_revision.get()).unwrap_or(i64::MAX),
                    recovery_reason,
                    request.request_intent_hash.as_str(),
                    recovery_profile.json(),
                    recovery_profile.hash().as_str(),
                    selected_model_rung,
                    text(prepared_at),
                ],
            )
            .map_err(backend)?;
        let seat_changed = transaction
            .execute(
                "UPDATE consultation_seats
                 SET occupancy_generation = ?4
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND native_id = ?5 AND occupancy_generation = ?6",
                params![
                    project_id.to_string(),
                    predecessor.run_id.as_text(),
                    predecessor.role_slot_id.as_str(),
                    i64::try_from(successor_occupancy_generation).unwrap_or(i64::MAX),
                    predecessor_identity.native_id.as_str(),
                    i64::try_from(predecessor.occupancy_generation).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        let run_changed = transaction
            .execute(
                "UPDATE consultation_runs SET revision = revision + 1, updated_at = ?4
                 WHERE project_id = ?1 AND run_id = ?2 AND revision = ?3",
                params![
                    project_id.to_string(),
                    predecessor.run_id.as_text(),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    text(prepared_at),
                ],
            )
            .map_err(backend)?;
        if seat_changed != 1 || run_changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "consultation recovery attempt",
                rule: "the seat or Committee revision moved during fencing",
            });
        }
        transaction.commit().map_err(backend)?;
        self.get_consultation_recovery_attempt(
            project_id,
            predecessor.run_id,
            &predecessor.role_slot_id,
            &predecessor_identity.native_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "prepared consultation recovery attempt",
        })
    }

    /// Persist exact predecessor retirement for replay diagnostics.
    pub fn mark_consultation_recovery_predecessor_retired(
        &self,
        attempt: &StoredConsultationRecoveryAttempt,
        retired_at: Timestamp,
    ) -> RepositoryResult<()> {
        self.connection
            .execute(
                "UPDATE consultation_seat_recovery_attempts
                 SET state = CASE WHEN state = 'prepared' THEN 'predecessor_retired' ELSE state END,
                     retired_at = COALESCE(retired_at, ?5)
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND predecessor_native_id = ?4",
                params![
                    attempt.project_id.to_string(),
                    attempt.run_id.as_text(),
                    attempt.role_slot_id.as_str(),
                    attempt.predecessor_native_id.as_str(),
                    text(retired_at),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Persist exact successor readback before installing it as active.
    pub fn mark_consultation_recovery_successor_observed(
        &self,
        attempt: &StoredConsultationRecoveryAttempt,
        successor: &StoredConsultationSeat,
    ) -> RepositoryResult<()> {
        let identity = successor
            .native_identity
            .as_ref()
            .ok_or(RepositoryError::Conflict {
                subject: "consultation recovery attempt",
                rule: "the successor has no native identity",
            })?;
        let observed_at = successor.observed_at.ok_or(RepositoryError::Conflict {
            subject: "consultation recovery attempt",
            rule: "the successor has no observation",
        })?;
        self.connection
            .execute(
                "UPDATE consultation_seat_recovery_attempts
                 SET state = CASE WHEN state IN ('prepared', 'predecessor_retired')
                                  THEN 'successor_observed' ELSE state END,
                     successor_runtime_kind = COALESCE(successor_runtime_kind, ?5),
                     successor_host = COALESCE(successor_host, ?6),
                     successor_generation = COALESCE(successor_generation, ?7),
                     successor_native_id = COALESCE(successor_native_id, ?8),
                     successor_provider_session = COALESCE(successor_provider_session, ?9),
                     successor_observed_at = COALESCE(successor_observed_at, ?10)
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND predecessor_native_id = ?4
                   AND (successor_native_id IS NULL OR successor_native_id = ?8)",
                params![
                    attempt.project_id.to_string(),
                    attempt.run_id.as_text(),
                    attempt.role_slot_id.as_str(),
                    attempt.predecessor_native_id.as_str(),
                    identity.runtime_kind.as_str(),
                    identity.host.as_str(),
                    i64::try_from(identity.generation).unwrap_or(i64::MAX),
                    identity.native_id.as_str(),
                    successor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(observed_at),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Read the active successor of an already-recorded consultation recovery.
    ///
    /// The predecessor native id is the replay key. A matching history row is
    /// not enough on its own: the current seat must still be the exact
    /// successor that row names.
    pub fn get_consultation_recovery_successor(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
        role_slot_id: &RoleSlotId,
        predecessor_native_id: &ExternalId,
    ) -> RepositoryResult<Option<StoredConsultationSeat>> {
        let recovered: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT seat_binding_id, successor_native_id
                 FROM consultation_seat_recoveries
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND predecessor_native_id = ?4",
                params![
                    project_id.to_string(),
                    run_id.as_text(),
                    role_slot_id.as_str(),
                    predecessor_native_id.as_str(),
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((seat_binding_id, successor_native_id)) = recovered else {
            return Ok(None);
        };
        let seat_binding_id = SeatBindingId::parse(&seat_binding_id)?;
        let seat = self.get_consultation_seat_by_binding(project_id, seat_binding_id)?;
        Ok(seat.filter(|seat| {
            seat.run_id == run_id
                && seat.role_slot_id == *role_slot_id
                && seat
                    .native_identity
                    .as_ref()
                    .is_some_and(|identity| identity.native_id.as_str() == successor_native_id)
        }))
    }

    /// Atomically archive one exact consultation predecessor and install its
    /// successor as the active filler of the same logical SeatBinding.
    pub fn replace_consultation_seat(
        &self,
        project_id: ProjectId,
        predecessor: &StoredConsultationSeat,
        successor: &StoredConsultationSeat,
        expected_revision: AggregateRevision,
        retired_at: Timestamp,
        recovery_reason: &str,
    ) -> RepositoryResult<Applied> {
        if predecessor.run_id != successor.run_id
            || predecessor.role_slot_id != successor.role_slot_id
            || predecessor.seat_binding_id != successor.seat_binding_id
        {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "a recovery cannot move the logical consultation SeatBinding",
            });
        }
        if successor.occupancy_generation <= predecessor.occupancy_generation {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "the successor occupancy generation must fence the predecessor",
            });
        }
        if !matches!(
            recovery_reason,
            "credential_propagation" | "provider_unavailable"
        ) {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "the recovery reason is not supported",
            });
        }
        let predecessor_identity =
            predecessor
                .native_identity
                .as_ref()
                .ok_or(RepositoryError::Conflict {
                    subject: "consultation seat recovery",
                    rule: "the predecessor has no native identity",
                })?;
        let successor_identity =
            successor
                .native_identity
                .as_ref()
                .ok_or(RepositoryError::Conflict {
                    subject: "consultation seat recovery",
                    rule: "the successor has no native identity",
                })?;
        let predecessor_observed_at = predecessor.observed_at.ok_or(RepositoryError::Conflict {
            subject: "consultation seat recovery",
            rule: "the predecessor has no native observation",
        })?;
        let successor_observed_at = successor.observed_at.ok_or(RepositoryError::Conflict {
            subject: "consultation seat recovery",
            rule: "the successor has no native observation",
        })?;
        let predecessor_model =
            serde_json::to_string(&predecessor.model_rung).map_err(|error| {
                RepositoryError::Backend {
                    detail: format!(
                        "a consultation predecessor route could not be encoded: {error}"
                    ),
                }
            })?;
        let successor_model = serde_json::to_string(&successor.model_rung).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a consultation successor route could not be encoded: {error}"),
            }
        })?;
        let transaction = self.begin()?;
        let active_native: Option<String> = transaction
            .query_row(
                "SELECT native_id FROM consultation_seats
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3",
                params![
                    project_id.to_string(),
                    predecessor.run_id.as_text(),
                    predecessor.role_slot_id.as_str(),
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if active_native.as_deref() == Some(successor_identity.native_id.as_str()) {
            transaction.rollback().map_err(backend)?;
            return Ok(Applied::Unchanged);
        }
        if active_native.as_deref() != Some(predecessor_identity.native_id.as_str()) {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "the active native predecessor differs from the recovery request",
            });
        }
        transaction
            .execute(
                "INSERT INTO consultation_seat_recoveries
                     (project_id, run_id, role_slot_id, seat_binding_id,
                      predecessor_model_rung, predecessor_runtime_kind,
                      predecessor_host, predecessor_generation,
                      predecessor_native_id, predecessor_provider_session,
                      predecessor_observed_at, successor_model_rung,
                      predecessor_occupancy_generation,
                      successor_runtime_kind, successor_host,
                      successor_generation, successor_native_id,
                      successor_provider_session, successor_observed_at,
                      successor_occupancy_generation, retired_at, recovery_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                         ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                         ?21, ?22)",
                params![
                    project_id.to_string(),
                    predecessor.run_id.as_text(),
                    predecessor.role_slot_id.as_str(),
                    predecessor.seat_binding_id.to_string(),
                    predecessor_model,
                    predecessor_identity.runtime_kind.as_str(),
                    predecessor_identity.host.as_str(),
                    i64::try_from(predecessor_identity.generation).unwrap_or(i64::MAX),
                    predecessor_identity.native_id.as_str(),
                    predecessor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(predecessor_observed_at),
                    successor_model,
                    i64::try_from(predecessor.occupancy_generation).unwrap_or(i64::MAX),
                    successor_identity.runtime_kind.as_str(),
                    successor_identity.host.as_str(),
                    i64::try_from(successor_identity.generation).unwrap_or(i64::MAX),
                    successor_identity.native_id.as_str(),
                    successor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(successor_observed_at),
                    i64::try_from(successor.occupancy_generation).unwrap_or(i64::MAX),
                    text(retired_at),
                    recovery_reason,
                ],
            )
            .map_err(backend)?;
        let changed = transaction
            .execute(
                "UPDATE consultation_seats
                 SET model_rung = ?4, occupancy_generation = ?5,
                     runtime_kind = ?6, host = ?7,
                     generation = ?8, native_id = ?9,
                     provider_session_id = ?10, observed_at = ?11
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND native_id = ?12 AND occupancy_generation = ?5",
                params![
                    project_id.to_string(),
                    successor.run_id.as_text(),
                    successor.role_slot_id.as_str(),
                    serde_json::to_string(&successor.model_rung).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!(
                                "a consultation successor route could not be encoded: {error}"
                            ),
                        }
                    })?,
                    i64::try_from(successor.occupancy_generation).unwrap_or(i64::MAX),
                    successor_identity.runtime_kind.as_str(),
                    successor_identity.host.as_str(),
                    i64::try_from(successor_identity.generation).unwrap_or(i64::MAX),
                    successor_identity.native_id.as_str(),
                    successor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(successor_observed_at),
                    predecessor_identity.native_id.as_str(),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "the active native predecessor moved during recovery",
            });
        }
        let run_changed = transaction
            .execute(
                "UPDATE consultation_runs
                 SET revision = revision + 1, updated_at = ?4
                 WHERE project_id = ?1 AND run_id = ?2 AND revision = ?3",
                params![
                    project_id.to_string(),
                    successor.run_id.as_text(),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    text(successor_observed_at),
                ],
            )
            .map_err(backend)?;
        if run_changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "the Committee revision moved during recovery",
            });
        }
        let attempt_changed = transaction
            .execute(
                "UPDATE consultation_seat_recovery_attempts
                 SET state = 'installed', installed_at = ?5
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND predecessor_native_id = ?4
                   AND successor_occupancy_generation = ?6
                   AND state IN ('successor_observed', 'installed')",
                params![
                    project_id.to_string(),
                    successor.run_id.as_text(),
                    successor.role_slot_id.as_str(),
                    predecessor_identity.native_id.as_str(),
                    text(successor_observed_at),
                    i64::try_from(successor.occupancy_generation).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        if attempt_changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "consultation seat recovery",
                rule: "the prepared recovery attempt was not ready to install",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(Applied::Created)
    }

    /// Read the exact native identity filling one persistent topology seat.
    pub fn get_hosted_topology_seat(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
    ) -> RepositoryResult<Option<StoredHostedTopologySeat>> {
        let row = self
            .connection
            .query_row(
                "SELECT model_rung, runtime_kind, host, generation, native_id,
                        provider_session_id, observed_at
                 FROM hosted_topology_seats
                 WHERE project_id = ?1 AND seat_binding_id = ?2",
                params![project_id.to_string(), seat_binding_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(
            |(model, runtime, host, generation, native, provider, observed)| {
                Ok(StoredHostedTopologySeat {
                    project_id,
                    seat_binding_id,
                    model_rung: serde_json::from_str(&model).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!(
                                "a hosted-seat model rung could not be decoded: {error}"
                            ),
                        }
                    })?,
                    native_identity: NativeRuntimeIdentity {
                        runtime_kind: RuntimeKindKey::parse(&runtime)?,
                        host: ExternalName::parse(&host)?,
                        generation: u64::try_from(generation).map_err(|_| {
                            RepositoryError::Backend {
                                detail: "a hosted-seat generation is negative".to_owned(),
                            }
                        })?,
                        native_id: ExternalId::parse(&native)?,
                    },
                    provider_session_id: provider.as_deref().map(ExternalId::parse).transpose()?,
                    observed_at: read_timestamp(&observed)?,
                })
            },
        )
        .transpose()
    }

    /// Freeze the first exact native readback for one persistent topology seat.
    /// The same native agent and route may report a newer provider conversation
    /// handle after a supported runtime resume; that observation refreshes in
    /// place. Moving the SeatBinding, route, or native agent is a conflict.
    pub fn bind_hosted_topology_seat(
        &self,
        seat: &StoredHostedTopologySeat,
    ) -> RepositoryResult<()> {
        if let Some(existing) =
            self.get_hosted_topology_seat(seat.project_id, seat.seat_binding_id)?
        {
            if existing.model_rung == seat.model_rung
                && existing.native_identity == seat.native_identity
            {
                self.connection
                    .execute(
                        "UPDATE hosted_topology_seats
                         SET provider_session_id = ?3, observed_at = ?4
                         WHERE project_id = ?1 AND seat_binding_id = ?2",
                        params![
                            seat.project_id.to_string(),
                            seat.seat_binding_id.to_string(),
                            seat.provider_session_id.as_ref().map(ExternalId::as_str),
                            text(seat.observed_at),
                        ],
                    )
                    .map_err(backend)?;
                return Ok(());
            }
            return Err(RepositoryError::Conflict {
                subject: "hosted topology seat",
                rule: "a persistent seat cannot change its route or native identity",
            });
        }
        let model =
            serde_json::to_string(&seat.model_rung).map_err(|error| RepositoryError::Backend {
                detail: format!("a hosted-seat model rung could not be encoded: {error}"),
            })?;
        self.connection
            .execute(
                "INSERT INTO hosted_topology_seats
                     (seat_binding_id, project_id, model_rung, runtime_kind, host,
                      generation, native_id, provider_session_id, observed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    seat.seat_binding_id.to_string(),
                    seat.project_id.to_string(),
                    model,
                    seat.native_identity.runtime_kind.as_str(),
                    seat.native_identity.host.as_str(),
                    i64::try_from(seat.native_identity.generation).unwrap_or(i64::MAX),
                    seat.native_identity.native_id.as_str(),
                    seat.provider_session_id.as_ref().map(ExternalId::as_str),
                    text(seat.observed_at),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Move one exact hosted-seat predecessor into immutable route history.
    ///
    /// The logical SeatBinding is untouched. Repeating the same move after a
    /// crash is unchanged; naming another active native identity is conflict.
    pub fn archive_hosted_topology_seat_route(
        &self,
        predecessor: &StoredHostedTopologySeat,
        retired_at: Timestamp,
        reason: &str,
    ) -> RepositoryResult<Applied> {
        let transaction = self.begin()?;
        let active = transaction
            .query_row(
                "SELECT model_rung, runtime_kind, host, generation, native_id,
                        provider_session_id, observed_at
                 FROM hosted_topology_seats
                 WHERE project_id = ?1 AND seat_binding_id = ?2",
                params![
                    predecessor.project_id.to_string(),
                    predecessor.seat_binding_id.to_string(),
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        let expected_model = serde_json::to_string(&predecessor.model_rung).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a hosted-seat model rung could not be encoded: {error}"),
            }
        })?;
        if let Some((model, runtime, host, generation, native, provider, observed)) = active {
            if model != expected_model
                || runtime != predecessor.native_identity.runtime_kind.as_str()
                || host != predecessor.native_identity.host.as_str()
                || u64::try_from(generation).ok() != Some(predecessor.native_identity.generation)
                || native != predecessor.native_identity.native_id.as_str()
                || provider.as_deref()
                    != predecessor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str)
                || observed != text(predecessor.observed_at)
            {
                return Err(RepositoryError::Conflict {
                    subject: "hosted topology seat",
                    rule: "the active native predecessor differs from the route correction",
                });
            }
            transaction
                .execute(
                    "INSERT INTO hosted_topology_seat_history
                         (seat_binding_id, project_id, generation, model_rung,
                          runtime_kind, host, native_id, provider_session_id,
                          observed_at, retired_at, retirement_reason)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        predecessor.seat_binding_id.to_string(),
                        predecessor.project_id.to_string(),
                        i64::try_from(predecessor.native_identity.generation).unwrap_or(i64::MAX),
                        expected_model,
                        predecessor.native_identity.runtime_kind.as_str(),
                        predecessor.native_identity.host.as_str(),
                        predecessor.native_identity.native_id.as_str(),
                        predecessor
                            .provider_session_id
                            .as_ref()
                            .map(ExternalId::as_str),
                        text(predecessor.observed_at),
                        text(retired_at),
                        reason,
                    ],
                )
                .map_err(backend)?;
            transaction
                .execute(
                    "DELETE FROM hosted_topology_seats
                     WHERE project_id = ?1 AND seat_binding_id = ?2",
                    params![
                        predecessor.project_id.to_string(),
                        predecessor.seat_binding_id.to_string(),
                    ],
                )
                .map_err(backend)?;
            transaction.commit().map_err(backend)?;
            return Ok(Applied::Created);
        }
        let archived: bool = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM hosted_topology_seat_history
                     WHERE project_id = ?1 AND seat_binding_id = ?2 AND native_id = ?3
                 )",
                params![
                    predecessor.project_id.to_string(),
                    predecessor.seat_binding_id.to_string(),
                    predecessor.native_identity.native_id.as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if !archived {
            return Err(RepositoryError::NotFound {
                subject: "hosted topology seat predecessor",
            });
        }
        transaction.rollback().map_err(backend)?;
        Ok(Applied::Unchanged)
    }

    /// Read one exact hosted-seat predecessor from immutable route history.
    pub fn get_hosted_topology_seat_history(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        native_id: &ExternalId,
    ) -> RepositoryResult<Option<StoredHostedTopologySeat>> {
        let row = self
            .connection
            .query_row(
                "SELECT model_rung, runtime_kind, host, generation,
                        provider_session_id, observed_at
                 FROM hosted_topology_seat_history
                 WHERE project_id = ?1 AND seat_binding_id = ?2 AND native_id = ?3",
                params![
                    project_id.to_string(),
                    seat_binding_id.to_string(),
                    native_id.as_str(),
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(|(model, runtime, host, generation, provider, observed)| {
            Ok(StoredHostedTopologySeat {
                project_id,
                seat_binding_id,
                model_rung: serde_json::from_str(&model).map_err(|error| {
                    RepositoryError::Backend {
                        detail: format!(
                            "a hosted-seat history model rung could not be decoded: {error}"
                        ),
                    }
                })?,
                native_identity: NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse(&runtime)?,
                    host: ExternalName::parse(&host)?,
                    generation: u64::try_from(generation).map_err(|_| {
                        RepositoryError::Backend {
                            detail: "a hosted-seat history generation is negative".to_owned(),
                        }
                    })?,
                    native_id: native_id.clone(),
                },
                provider_session_id: provider.as_deref().map(ExternalId::parse).transpose()?,
                observed_at: read_timestamp(&observed)?,
            })
        })
        .transpose()
    }

    /// Atomically move the exact predecessor to history and make its successor
    /// the active native filler of the same logical SeatBinding.
    pub fn replace_hosted_topology_seat_route(
        &self,
        predecessor: &StoredHostedTopologySeat,
        successor: &StoredHostedTopologySeat,
        retired_at: Timestamp,
        reason: &str,
    ) -> RepositoryResult<Applied> {
        if predecessor.project_id != successor.project_id
            || predecessor.seat_binding_id != successor.seat_binding_id
        {
            return Err(RepositoryError::Conflict {
                subject: "hosted topology seat route",
                rule: "a route correction cannot move the logical SeatBinding",
            });
        }
        let transaction = self.begin()?;
        let active_native: Option<String> = transaction
            .query_row(
                "SELECT native_id FROM hosted_topology_seats
                 WHERE project_id = ?1 AND seat_binding_id = ?2",
                params![
                    predecessor.project_id.to_string(),
                    predecessor.seat_binding_id.to_string(),
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if active_native.as_deref() == Some(successor.native_identity.native_id.as_str()) {
            transaction.rollback().map_err(backend)?;
            return Ok(Applied::Unchanged);
        }
        if active_native.as_deref() != Some(predecessor.native_identity.native_id.as_str()) {
            return Err(RepositoryError::Conflict {
                subject: "hosted topology seat route",
                rule: "the active native predecessor differs from the correction",
            });
        }
        let predecessor_model =
            serde_json::to_string(&predecessor.model_rung).map_err(|error| {
                RepositoryError::Backend {
                    detail: format!("a hosted-seat model rung could not be encoded: {error}"),
                }
            })?;
        transaction
            .execute(
                "INSERT INTO hosted_topology_seat_history
                     (seat_binding_id, project_id, generation, model_rung,
                      runtime_kind, host, native_id, provider_session_id,
                      observed_at, retired_at, retirement_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    predecessor.seat_binding_id.to_string(),
                    predecessor.project_id.to_string(),
                    i64::try_from(predecessor.native_identity.generation).unwrap_or(i64::MAX),
                    predecessor_model,
                    predecessor.native_identity.runtime_kind.as_str(),
                    predecessor.native_identity.host.as_str(),
                    predecessor.native_identity.native_id.as_str(),
                    predecessor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(predecessor.observed_at),
                    text(retired_at),
                    reason,
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "DELETE FROM hosted_topology_seats
                 WHERE project_id = ?1 AND seat_binding_id = ?2",
                params![
                    predecessor.project_id.to_string(),
                    predecessor.seat_binding_id.to_string(),
                ],
            )
            .map_err(backend)?;
        let successor_model = serde_json::to_string(&successor.model_rung).map_err(|error| {
            RepositoryError::Backend {
                detail: format!("a hosted-seat model rung could not be encoded: {error}"),
            }
        })?;
        transaction
            .execute(
                "INSERT INTO hosted_topology_seats
                     (seat_binding_id, project_id, model_rung, runtime_kind, host,
                      generation, native_id, provider_session_id, observed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    successor.seat_binding_id.to_string(),
                    successor.project_id.to_string(),
                    successor_model,
                    successor.native_identity.runtime_kind.as_str(),
                    successor.native_identity.host.as_str(),
                    i64::try_from(successor.native_identity.generation).unwrap_or(i64::MAX),
                    successor.native_identity.native_id.as_str(),
                    successor
                        .provider_session_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(successor.observed_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(Applied::Updated)
    }

    /// Read the immutable advice artifact one Advisor seat submitted.
    pub fn get_advisor_advice(
        &self,
        project_id: ProjectId,
        advisor_run_id: AdvisorRunId,
    ) -> RepositoryResult<Option<StoredAdvisorAdvice>> {
        let columns = self
            .connection
            .query_row(
                "SELECT seat_binding_id, document, document_hash, recorded_at
                 FROM advisor_advice_artifacts
                 WHERE project_id = ?1 AND advisor_run_id = ?2",
                params![project_id.to_string(), advisor_run_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        columns
            .map(|columns| read_advisor_advice(project_id, advisor_run_id, columns))
            .transpose()
    }

    /// Atomically append one Advisor's immutable output and advance its run.
    ///
    /// An exact existing document is a replay. A different document can never
    /// replace it, and the disposition authority has no operation that writes
    /// this table.
    #[allow(clippy::too_many_arguments)]
    pub fn record_advisor_advice(
        &self,
        project_id: ProjectId,
        advisor_run_id: AdvisorRunId,
        seat_binding_id: SeatBindingId,
        document: &serde_json::Value,
        document_hash: &ContentHash,
        expected_revision: AggregateRevision,
        recorded_at: Timestamp,
    ) -> RepositoryResult<(StoredConsultationRun, bool)> {
        let encoded = canonical_json(document, "Advisor advice")?;
        let transaction = self.begin()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT document_hash FROM advisor_advice_artifacts
                 WHERE project_id = ?1 AND advisor_run_id = ?2",
                params![project_id.to_string(), advisor_run_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if let Some(existing_hash) = existing {
            if existing_hash != document_hash.as_str() {
                return Err(RepositoryError::Conflict {
                    subject: "Advisor advice",
                    rule: "the Advisor already submitted different immutable output",
                });
            }
            transaction.commit().map_err(backend)?;
            let run = self
                .get_consultation_run(project_id, ConsultationRunId::Advisor(advisor_run_id))?
                .ok_or(RepositoryError::NotFound {
                    subject: "Advisor run",
                })?;
            return Ok((run, false));
        }
        transaction
            .execute(
                "INSERT INTO advisor_advice_artifacts
                     (advisor_run_id, project_id, seat_binding_id,
                      document, document_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    advisor_run_id.to_string(),
                    project_id.to_string(),
                    seat_binding_id.to_string(),
                    encoded,
                    document_hash.as_str(),
                    text(recorded_at),
                ],
            )
            .map_err(backend)?;
        let changed = transaction
            .execute(
                "UPDATE consultation_runs
                 SET revision = revision + 1, updated_at = ?4
                 WHERE project_id = ?1 AND run_id = ?2 AND family = 'advisor'
                   AND state = 'running' AND result IS NULL AND revision = ?3",
                params![
                    project_id.to_string(),
                    advisor_run_id.to_string(),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    text(recorded_at),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "Advisor run",
                rule: "only the current running revision can accept its advice",
            });
        }
        transaction.commit().map_err(backend)?;
        let run = self
            .get_consultation_run(project_id, ConsultationRunId::Advisor(advisor_run_id))?
            .ok_or(RepositoryError::NotFound {
                subject: "Advisor run",
            })?;
        Ok((run, true))
    }

    /// Append one immutable Committee finding. Returns `true` when inserted and
    /// `false` for an exact replay.
    pub fn append_committee_finding(
        &self,
        project_id: ProjectId,
        finding: &StoredCommitteeFinding,
    ) -> RepositoryResult<bool> {
        let document = canonical_json(&finding.document, "Committee finding")?;
        let transaction = self.begin()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT document_hash FROM committee_findings
                 WHERE project_id = ?1 AND committee_run_id = ?2
                   AND round = ?3 AND role_slot_id = ?4",
                params![
                    project_id.to_string(),
                    finding.committee_run_id.to_string(),
                    i64::from(finding.round),
                    finding.role_slot_id.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if let Some(hash) = existing {
            if hash != finding.document_hash.as_str() {
                return Err(RepositoryError::Conflict {
                    subject: "Committee finding",
                    rule: "a slot already recorded a different finding for this round",
                });
            }
            transaction.commit().map_err(backend)?;
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO committee_findings
                     (committee_run_id, project_id, round, role_slot_id, role,
                      verdict, evidence_complete, document, document_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    finding.committee_run_id.to_string(),
                    project_id.to_string(),
                    i64::from(finding.round),
                    finding.role_slot_id.as_str(),
                    finding.role.as_str(),
                    finding.verdict.as_str(),
                    i64::from(finding.evidence_complete),
                    document,
                    finding.document_hash.as_str(),
                    text(finding.recorded_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(true)
    }

    /// Atomically append one immutable finding and advance the owning run.
    /// An exact existing document is a replay and leaves the revision alone;
    /// a different document for the frozen slot conflicts.
    pub fn record_committee_finding(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
        finding: &StoredCommitteeFinding,
        expected_revision: AggregateRevision,
        next_state: ConsultationRunState,
        updated_at: Timestamp,
    ) -> RepositoryResult<(StoredConsultationRun, bool)> {
        let ConsultationRunId::Committee(committee_run_id) = run_id else {
            return Err(RepositoryError::Conflict {
                subject: "Committee finding",
                rule: "a finding can only belong to a Committee run",
            });
        };
        if committee_run_id != finding.committee_run_id {
            return Err(RepositoryError::Conflict {
                subject: "Committee finding",
                rule: "the finding and run identity differ",
            });
        }
        let document = canonical_json(&finding.document, "Committee finding")?;
        let transaction = self.begin()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT document_hash FROM committee_findings
                 WHERE project_id = ?1 AND committee_run_id = ?2
                   AND round = ?3 AND role_slot_id = ?4",
                params![
                    project_id.to_string(),
                    committee_run_id.to_string(),
                    i64::from(finding.round),
                    finding.role_slot_id.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if let Some(hash) = existing {
            if hash != finding.document_hash.as_str() {
                return Err(RepositoryError::Conflict {
                    subject: "Committee finding",
                    rule: "a slot already recorded a different finding for this round",
                });
            }
            transaction.commit().map_err(backend)?;
            let run = self.get_consultation_run(project_id, run_id)?.ok_or(
                RepositoryError::NotFound {
                    subject: "consultation run",
                },
            )?;
            return Ok((run, false));
        }
        transaction
            .execute(
                "INSERT INTO committee_findings
                     (committee_run_id, project_id, round, role_slot_id, role,
                      verdict, evidence_complete, document, document_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    finding.committee_run_id.to_string(),
                    project_id.to_string(),
                    i64::from(finding.round),
                    finding.role_slot_id.as_str(),
                    finding.role.as_str(),
                    finding.verdict.as_str(),
                    i64::from(finding.evidence_complete),
                    document,
                    finding.document_hash.as_str(),
                    text(finding.recorded_at),
                ],
            )
            .map_err(backend)?;
        let changed = transaction
            .execute(
                "UPDATE consultation_runs
                 SET state = ?4, revision = revision + 1, updated_at = ?5
                 WHERE project_id = ?1 AND run_id = ?2 AND family = ?3
                   AND revision = ?6 AND result IS NULL",
                params![
                    project_id.to_string(),
                    run_id.as_text(),
                    run_id.family().as_str(),
                    next_state.as_str(),
                    text(updated_at),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "consultation run",
                rule: "the run moved since it was read",
            });
        }
        transaction.commit().map_err(backend)?;
        let run =
            self.get_consultation_run(project_id, run_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "consultation run",
                })?;
        Ok((run, true))
    }

    /// Every immutable finding in one Committee round, in slot order.
    pub fn list_committee_findings(
        &self,
        project_id: ProjectId,
        committee_run_id: kontor_core::id::CommitteeRunId,
        round: u32,
    ) -> RepositoryResult<Vec<StoredCommitteeFinding>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT role_slot_id, role, verdict, evidence_complete,
                        document, document_hash, recorded_at
                 FROM committee_findings
                 WHERE project_id = ?1 AND committee_run_id = ?2 AND round = ?3
                 ORDER BY role_slot_id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(
                params![
                    project_id.to_string(),
                    committee_run_id.to_string(),
                    i64::from(round)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| read_committee_finding(committee_run_id, round, row))
            .collect()
    }

    /// Advance one consultation under compare-and-swap.
    pub fn advance_consultation_run(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
        expected_revision: AggregateRevision,
        state: ConsultationRunState,
        result: Option<(&serde_json::Value, &ContentHash)>,
        updated_at: Timestamp,
    ) -> RepositoryResult<StoredConsultationRun> {
        let encoded = result
            .map(|(value, _)| canonical_json(value, "consultation result"))
            .transpose()?;
        let transaction = self.begin()?;
        let changed = transaction
            .execute(
                "UPDATE consultation_runs
                 SET state = ?4, result = ?5, result_hash = ?6,
                     revision = revision + 1, updated_at = ?7,
                     settled_at = CASE WHEN ?4 = 'settled' THEN ?7 ELSE NULL END
                 WHERE project_id = ?1 AND run_id = ?2 AND family = ?3
                   AND revision = ?8",
                params![
                    project_id.to_string(),
                    run_id.as_text(),
                    run_id.family().as_str(),
                    state.as_str(),
                    encoded,
                    result.map(|(_, hash)| hash.as_str()),
                    text(updated_at),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "consultation run",
                rule: "the run moved since it was read",
            });
        }
        transaction.commit().map_err(backend)?;
        self.get_consultation_run(project_id, run_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "consultation run",
            })
    }

    /// Record the single bounded Committee remediation and open round two.
    #[allow(clippy::too_many_arguments)]
    pub fn remediate_committee_run(
        &self,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
        expected_revision: AggregateRevision,
        recommendation: &BoundedText,
        tried_path: &BoundedText,
        document: &serde_json::Value,
        document_hash: &ContentHash,
        recorded_at: Timestamp,
    ) -> RepositoryResult<StoredConsultationRun> {
        let encoded = canonical_json(document, "Committee remediation")?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO committee_remediations
                     (committee_run_id, project_id, from_round, recommendation,
                      tried_path, document, document_hash, recorded_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
                params![
                    committee_run_id.to_string(),
                    project_id.to_string(),
                    recommendation.as_str(),
                    tried_path.as_str(),
                    encoded,
                    document_hash.as_str(),
                    text(recorded_at),
                ],
            )
            .map_err(backend)?;
        let changed = transaction
            .execute(
                "UPDATE consultation_runs
                 SET state = 'running', round = 2, revision = revision + 1,
                     updated_at = ?4, settled_at = NULL
                 WHERE project_id = ?1 AND run_id = ?2 AND family = 'committee'
                   AND round = 1 AND state = 'awaiting_judge' AND revision = ?3",
                params![
                    project_id.to_string(),
                    committee_run_id.to_string(),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    text(recorded_at),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "Committee remediation",
                rule: "only a settled-evidence round one may open round two",
            });
        }
        transaction.commit().map_err(backend)?;
        self.get_consultation_run(project_id, ConsultationRunId::Committee(committee_run_id))?
            .ok_or(RepositoryError::NotFound {
                subject: "consultation run",
            })
    }

    /// Read the immutable remediation document, when round two was opened.
    pub fn get_committee_remediation(
        &self,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
    ) -> RepositoryResult<Option<serde_json::Value>> {
        let encoded = self
            .connection
            .query_row(
                "SELECT document FROM committee_remediations
                 WHERE project_id = ?1 AND committee_run_id = ?2",
                params![project_id.to_string(), committee_run_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(backend)?;
        encoded
            .map(|document| {
                serde_json::from_str(&document).map_err(|error| RepositoryError::Backend {
                    detail: format!("a Committee remediation could not be decoded: {error}"),
                })
            })
            .transpose()
    }

    /// Record one Quick session and the ids its placement used.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the session already exists.
    pub fn create_quick_session(&self, session: &StoredQuickSession) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let role =
            serde_json::to_string(&session.role).map_err(|error| RepositoryError::Backend {
                detail: format!("a quick session role could not be encoded: {error}"),
            })?;
        transaction
            .execute(
                "INSERT INTO quick_sessions
                     (id, project_id, role, role_slot_id, topology_node_id, seat_binding_id,
                      psw_topology_node_id, psw_native_id, purpose, intent_hash, disposition,
                      revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    session.id.to_string(),
                    session.project_id.to_string(),
                    role,
                    session.role_slot_id.as_str(),
                    session.topology_node_id.to_string(),
                    session.seat_binding_id.to_string(),
                    session.psw_topology_node_id.to_string(),
                    session.psw_native_id.as_ref().map(ExternalId::as_str),
                    session.purpose.as_str(),
                    session.intent_hash.as_str(),
                    session.disposition.as_str(),
                    i64::try_from(session.revision.get()).unwrap_or(i64::MAX),
                    text(session.created_at),
                ],
            )
            .map_err(|error| match error {
                // Two ensures of the same request racing. The loser has written
                // nothing else yet — the row is deliberately first — so it can
                // simply read the winner's session and return that.
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "quick session",
                        rule: "one command opens one session",
                    }
                }
                other => backend(other),
            })?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// One Quick session in one project.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_quick_session(
        &self,
        project_id: ProjectId,
        quick_session_id: QuickSessionId,
    ) -> RepositoryResult<Option<StoredQuickSession>> {
        self.quick_session_where("id = ?2", project_id, &quick_session_id.to_string())
    }

    /// The Quick session one command opened, if it already opened one.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_quick_session_by_intent(
        &self,
        project_id: ProjectId,
        intent_hash: &ContentHash,
    ) -> RepositoryResult<Option<StoredQuickSession>> {
        self.quick_session_where("intent_hash = ?2", project_id, intent_hash.as_str())
    }

    /// One Quick session, addressed by whichever unique column names it.
    fn quick_session_where(
        &self,
        predicate: &str,
        project_id: ProjectId,
        value: &str,
    ) -> RepositoryResult<Option<StoredQuickSession>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT id, role, role_slot_id, topology_node_id, seat_binding_id,
                            psw_topology_node_id, psw_native_id, purpose, intent_hash,
                            disposition, revision, created_at
                     FROM quick_sessions WHERE project_id = ?1 AND {predicate}"
                ),
                params![project_id.to_string(), value],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, String>(11)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(|columns| {
                Ok(StoredQuickSession {
                    id: QuickSessionId::parse(&columns.0)?,
                    project_id,
                    role: serde_json::from_str(&columns.1).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!("a stored quick session role is unreadable: {error}"),
                        }
                    })?,
                    role_slot_id: RoleSlotId::parse(&columns.2)?,
                    topology_node_id: TopologyNodeId::parse(&columns.3)?,
                    seat_binding_id: SeatBindingId::parse(&columns.4)?,
                    psw_topology_node_id: TopologyNodeId::parse(&columns.5)?,
                    psw_native_id: columns.6.as_deref().map(ExternalId::parse).transpose()?,
                    purpose: BoundedText::parse(&columns.7)?,
                    intent_hash: ContentHash::parse(&columns.8)?,
                    disposition: SourceDisposition::parse(&columns.9)?,
                    revision: AggregateRevision::parse(
                        u64::try_from(columns.10).unwrap_or_default(),
                    )?,
                    created_at: read_timestamp(&columns.11)?,
                })
            })
            .transpose()
    }

    /// Move one Quick session's source disposition, bumping its revision.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] when the session does not exist.
    pub fn set_quick_session_disposition(
        &self,
        project_id: ProjectId,
        quick_session_id: QuickSessionId,
        disposition: SourceDisposition,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let changed = transaction
            .execute(
                "UPDATE quick_sessions SET disposition = ?3, revision = revision + 1
                 WHERE project_id = ?1 AND id = ?2",
                params![
                    project_id.to_string(),
                    quick_session_id.to_string(),
                    disposition.as_str(),
                ],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Err(RepositoryError::NotFound {
                subject: "quick session",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Authorize one promotion, freezing the ids and the roster its effects
    /// will use.
    ///
    /// Both rows are written before the first effect, and in one transaction.
    /// They are the two things a resumed apply reads to know what it is
    /// resuming: the promotion row says which epic, the roster row says which
    /// seats. Writing either one later — or the two separately — leaves a
    /// window where a failure has recorded the source as promoted while the
    /// resume path cannot find what it was promoted into. Since the promotion
    /// row is keyed by its source and nothing deletes it, a source caught in
    /// that window would be permanently unpromotable.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the source is already
    /// promoted.
    pub fn begin_promotion(
        &self,
        promotion: &StoredPromotion,
        roster: &StoredEpicRoster,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO quick_session_promotions
                     (quick_session_id, project_id, mini_project_id, preview_hash,
                      source_disposition, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    promotion.quick_session_id.to_string(),
                    promotion.project_id.to_string(),
                    promotion.mini_project_id.to_string(),
                    promotion.preview_hash.as_str(),
                    promotion.source_disposition.as_str(),
                    text(promotion.created_at),
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "quick session promotion",
                        rule: "a Quick session is promoted exactly once",
                    }
                }
                other => backend(other),
            })?;
        // Legal before the MiniProject exists: `epic_rosters` deliberately
        // carries no foreign key to `mini_projects`, because the roster is what
        // the epic is built *from*.
        transaction
            .execute(
                "INSERT INTO epic_rosters
                     (project_id, mini_project_id, core_team_version, catalog_hash, seats,
                      quick_session_id, revision, pinned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    roster.project_id.to_string(),
                    roster.mini_project_id.to_string(),
                    version_column(roster.core_team_version),
                    roster.catalog_hash.as_str(),
                    roster.seats.to_string(),
                    roster.quick_session_id.map(|id| id.to_string()),
                    i64::try_from(roster.revision.get()).unwrap_or(i64::MAX),
                    text(roster.pinned_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Record that a promotion's handoff reached its seat.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] when no promotion is in flight.
    pub fn complete_promotion(
        &self,
        quick_session_id: QuickSessionId,
        handoff: &serde_json::Value,
        handoff_hash: &ContentHash,
        lsa_seat_binding_id: SeatBindingId,
        completed_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let changed = transaction
            .execute(
                "UPDATE quick_session_promotions
                 SET handoff = ?2, handoff_hash = ?3, lsa_seat_binding_id = ?4, completed_at = ?5
                 WHERE quick_session_id = ?1",
                params![
                    quick_session_id.to_string(),
                    handoff.to_string(),
                    handoff_hash.as_str(),
                    lsa_seat_binding_id.to_string(),
                    text(completed_at),
                ],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Err(RepositoryError::NotFound {
                subject: "quick session promotion",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// The promotion of one Quick session, in flight or complete.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_promotion(
        &self,
        quick_session_id: QuickSessionId,
    ) -> RepositoryResult<Option<StoredPromotion>> {
        self.connection
            .query_row(
                "SELECT project_id, mini_project_id, preview_hash, source_disposition,
                        handoff, handoff_hash, lsa_seat_binding_id, completed_at, created_at
                 FROM quick_session_promotions WHERE quick_session_id = ?1",
                params![quick_session_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(|columns| {
                Ok(StoredPromotion {
                    quick_session_id,
                    project_id: ProjectId::parse(&columns.0)?,
                    mini_project_id: MiniProjectId::parse(&columns.1)?,
                    preview_hash: ContentHash::parse(&columns.2)?,
                    source_disposition: SourceDisposition::parse(&columns.3)?,
                    handoff: columns
                        .4
                        .map(|handoff| serde_json::from_str(&handoff))
                        .transpose()
                        .map_err(|error| RepositoryError::Backend {
                            detail: format!("a stored handoff is unreadable: {error}"),
                        })?,
                    handoff_hash: columns.5.as_deref().map(ContentHash::parse).transpose()?,
                    lsa_seat_binding_id: columns
                        .6
                        .as_deref()
                        .map(SeatBindingId::parse)
                        .transpose()?,
                    completed_at: columns.7.as_deref().map(read_timestamp).transpose()?,
                    created_at: read_timestamp(&columns.8)?,
                })
            })
            .transpose()
    }

    /// Freeze, or move, the roster one epic is staffed from.
    ///
    /// # Errors
    /// Returns a backend error.
    pub fn put_epic_roster(&self, roster: &StoredEpicRoster) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO epic_rosters
                     (project_id, mini_project_id, core_team_version, catalog_hash, seats,
                      quick_session_id, revision, pinned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (project_id, mini_project_id) DO UPDATE SET
                     core_team_version = excluded.core_team_version,
                     catalog_hash = excluded.catalog_hash,
                     seats = excluded.seats,
                     revision = epic_rosters.revision + 1,
                     pinned_at = excluded.pinned_at",
                params![
                    roster.project_id.to_string(),
                    roster.mini_project_id.to_string(),
                    version_column(roster.core_team_version),
                    roster.catalog_hash.as_str(),
                    roster.seats.to_string(),
                    roster.quick_session_id.map(|id| id.to_string()),
                    i64::try_from(roster.revision.get()).unwrap_or(i64::MAX),
                    text(roster.pinned_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// The roster one epic is staffed from.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_epic_roster(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<StoredEpicRoster>> {
        self.connection
            .query_row(
                "SELECT core_team_version, catalog_hash, seats, quick_session_id, revision,
                        pinned_at
                 FROM epic_rosters WHERE project_id = ?1 AND mini_project_id = ?2",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(|columns| {
                Ok(StoredEpicRoster {
                    project_id,
                    mini_project_id,
                    core_team_version: read_version(columns.0)?,
                    catalog_hash: ContentHash::parse(&columns.1)?,
                    seats: serde_json::from_str(&columns.2).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!("a stored epic roster is unreadable: {error}"),
                        }
                    })?,
                    quick_session_id: columns
                        .3
                        .as_deref()
                        .map(QuickSessionId::parse)
                        .transpose()?,
                    revision: AggregateRevision::parse(
                        u64::try_from(columns.4).unwrap_or_default(),
                    )?,
                    pinned_at: read_timestamp(&columns.5)?,
                })
            })
            .transpose()
    }

    /// Read the revision a project is currently configured with.
    ///
    /// The highest published version, which is the only definition of current
    /// this schema has.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_current_core_team(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<StoredCoreTeamRevision>> {
        let found: Option<(i64, String, String, String)> = self
            .connection
            .query_row(
                "SELECT version, catalog_hash, seats, created_at
                 FROM core_team_revisions
                 WHERE project_id = ?1
                 ORDER BY version DESC
                 LIMIT 1",
                params![project_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(version, catalog_hash, seats, created_at)| {
                Ok(StoredCoreTeamRevision {
                    project_id,
                    version: read_version(version)?,
                    catalog_hash: ContentHash::parse(&catalog_hash)?,
                    seats: serde_json::from_str(&seats).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!("stored core team seats are not valid JSON: {error}"),
                        }
                    })?,
                    published_at: read_timestamp(&created_at)?,
                })
            })
            .transpose()
    }

    /// Every published Completion Profile revision, oldest first.
    ///
    /// The built-in `operational_default@1` is deliberately *not* a row here.
    /// It ships with the build, so seeding it per project would mean one copy
    /// per project that a later build could not correct, and a project created
    /// before the seed ran would silently have a different catalog from one
    /// created after. The read path adds it.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_completion_profiles(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<StoredCompletionProfile>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, version, name, definition, definition_hash, published_at
                 FROM completion_profile_revisions
                 WHERE project_id = ?1
                 ORDER BY id, version",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(params![project_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(backend)?;
        let mut published = Vec::new();
        for row in rows {
            let columns = row.map_err(backend)?;
            published.push(StoredCompletionProfile {
                project_id,
                id: ExternalName::parse(&columns.0)?,
                version: read_version(columns.1)?,
                name: ExternalName::parse(&columns.2)?,
                definition: serde_json::from_str(&columns.3).map_err(|error| {
                    RepositoryError::Backend {
                        detail: format!("a stored completion profile is unreadable: {error}"),
                    }
                })?,
                definition_hash: ContentHash::parse(&columns.4)?,
                published_at: read_timestamp(&columns.5)?,
            });
        }
        Ok(published)
    }

    /// Publish one immutable Completion Profile revision.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when that exact revision already
    /// stands. A published revision is immutable, so a second write of one is
    /// never an update.
    pub fn publish_completion_profile(
        &self,
        profile: &StoredCompletionProfile,
    ) -> RepositoryResult<()> {
        self.connection
            .execute(
                "INSERT INTO completion_profile_revisions
                     (project_id, id, version, name, definition, definition_hash, published_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    profile.project_id.to_string(),
                    profile.id.as_str(),
                    version_column(profile.version),
                    profile.name.as_str(),
                    profile.definition.to_string(),
                    profile.definition_hash.as_str(),
                    text(profile.published_at),
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "completion profile revision",
                        rule: "a published revision is immutable and published once",
                    }
                }
                other => backend(other),
            })?;
        Ok(())
    }

    /// Read one epic's durable completion run.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_epic_completion(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<StoredEpicCompletion>> {
        self.connection
            .query_row(
                "SELECT profile_id, profile_version, definition_hash, state, revision, updated_at
                 FROM epic_completion WHERE project_id = ?1 AND mini_project_id = ?2",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(|columns| {
                Ok(StoredEpicCompletion {
                    project_id,
                    mini_project_id,
                    profile_id: ExternalName::parse(&columns.0)?,
                    profile_version: read_version(columns.1)?,
                    definition_hash: ContentHash::parse(&columns.2)?,
                    state: serde_json::from_str(&columns.3).map_err(|error| {
                        RepositoryError::Backend {
                            detail: format!("a stored completion state is unreadable: {error}"),
                        }
                    })?,
                    revision: AggregateRevision::parse(
                        u64::try_from(columns.4).unwrap_or_default(),
                    )?,
                    updated_at: read_timestamp(&columns.5)?,
                })
            })
            .transpose()
    }

    /// Start one epic's completion run.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the epic already has one. A
    /// second run for one epic would be a second immutable round lineage over
    /// the same work.
    pub fn create_epic_completion(
        &self,
        completion: &StoredEpicCompletion,
    ) -> RepositoryResult<()> {
        self.connection
            .execute(
                "INSERT INTO epic_completion
                     (project_id, mini_project_id, profile_id, profile_version, definition_hash,
                      state, revision, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    completion.project_id.to_string(),
                    completion.mini_project_id.to_string(),
                    completion.profile_id.as_str(),
                    version_column(completion.profile_version),
                    completion.definition_hash.as_str(),
                    completion.state.to_string(),
                    i64::try_from(completion.revision.get()).unwrap_or(i64::MAX),
                    text(completion.updated_at),
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "epic completion",
                        rule: "one epic has exactly one completion run",
                    }
                }
                other => backend(other),
            })?;
        Ok(())
    }

    /// Store one epic's next completion state under an optimistic-concurrency
    /// check.
    ///
    /// The expected revision is compared *in the `UPDATE`* rather than by a read
    /// followed by a write: two callers advancing one epic from the same revision
    /// would both pass a separate check, and the second would overwrite the
    /// first's transition along with the effects it had already planned.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the run has moved since the
    /// caller read it, and [`RepositoryError::NotFound`] when it does not exist.
    pub fn update_epic_completion(
        &self,
        completion: &StoredEpicCompletion,
        expected_revision: AggregateRevision,
    ) -> RepositoryResult<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE epic_completion
                    SET state = ?3, revision = ?4, updated_at = ?5
                  WHERE project_id = ?1 AND mini_project_id = ?2 AND revision = ?6",
                params![
                    completion.project_id.to_string(),
                    completion.mini_project_id.to_string(),
                    completion.state.to_string(),
                    i64::try_from(completion.revision.get()).unwrap_or(i64::MAX),
                    text(completion.updated_at),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        if changed == 1 {
            return Ok(());
        }
        // Nothing changed: either the run is gone or it moved. Which one it is
        // decides the caller's answer, so it is read rather than guessed.
        let exists = self
            .connection
            .query_row(
                "SELECT 1 FROM epic_completion WHERE project_id = ?1 AND mini_project_id = ?2",
                params![
                    completion.project_id.to_string(),
                    completion.mini_project_id.to_string()
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(backend)?
            .is_some();
        Err(if exists {
            RepositoryError::Conflict {
                subject: "epic completion",
                rule: "the completion run moved since the caller read it",
            }
        } else {
            RepositoryError::NotFound {
                subject: "epic completion",
            }
        })
    }

    /// Append one wake intent, returning `false` when it already stood.
    ///
    /// `false` is the replay answer: the intent for this exact
    /// `(epic, revision, reason, seat)` is already recorded, so the caller reuses
    /// its receipt rather than opening a second turn.
    ///
    /// # Errors
    /// Returns a backend error.
    pub fn append_completion_wake(&self, wake: &StoredCompletionWake) -> RepositoryResult<bool> {
        let inserted = self
            .connection
            .execute(
                "INSERT OR IGNORE INTO epic_completion_wakes
                     (project_id, mini_project_id, completion_revision, reason, seat_binding_id,
                      receipt, appended_at, acknowledged_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    wake.project_id.to_string(),
                    wake.mini_project_id.to_string(),
                    i64::try_from(wake.completion_revision.get()).unwrap_or(i64::MAX),
                    wake.reason.as_str(),
                    wake.seat_binding_id.to_string(),
                    wake.receipt.as_str(),
                    text(wake.appended_at),
                    wake.acknowledged_at.map(text),
                ],
            )
            .map_err(backend)?;
        Ok(inserted == 1)
    }

    /// Record that the runtime took the turn one wake intent asked for.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] when no such intent stands.
    pub fn acknowledge_completion_wake(
        &self,
        wake: &StoredCompletionWake,
        acknowledged_at: Timestamp,
    ) -> RepositoryResult<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE epic_completion_wakes
                    SET acknowledged_at = ?6
                  WHERE project_id = ?1 AND mini_project_id = ?2 AND completion_revision = ?3
                    AND reason = ?4 AND seat_binding_id = ?5",
                params![
                    wake.project_id.to_string(),
                    wake.mini_project_id.to_string(),
                    i64::try_from(wake.completion_revision.get()).unwrap_or(i64::MAX),
                    wake.reason.as_str(),
                    wake.seat_binding_id.to_string(),
                    text(acknowledged_at),
                ],
            )
            .map_err(backend)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(RepositoryError::NotFound {
                subject: "completion wake intent",
            })
        }
    }

    /// The distinct artifact-contract keys one task has durable evidence for.
    ///
    /// Keys only. The completion ticket gate asks whether a declared artifact is
    /// evidenced, not what its locator is, and handing it the locators would put
    /// this read in the position of deciding which of several records for one key
    /// counts — a decision the gate does not need and must not make twice.
    ///
    /// Three sources, unioned, because an artifact leaves a durable trace in
    /// three different places and only the first of them was ever read:
    ///
    /// - `artifact_evidence` — the addressable record: a key plus a locator
    ///   someone can follow. Nothing in the delivery path writes it today.
    /// - `task_gate_evaluations.evidence` on a **passed** gate — the artifacts an
    ///   authorized evaluator cited when accepting the work. This is the strongest
    ///   of the three: an independent role attested the artifact while passing.
    ///   A rejected verdict is excluded; citing an artifact while refusing the
    ///   work is not evidence the contract was met.
    /// - `role_turns.artifacts` — the settling role's own declaration of what its
    ///   turn produced.
    ///
    /// The producer's own declaration is admitted alongside the evaluator's
    /// because it does not lower the bar: the profile requires the gates to pass
    /// as `goals` independently, so evidence drawn from a turn is still gated.
    /// Reading only `artifact_evidence` made the ticket gate unsatisfiable for
    /// every task closed through ordinary delivery, which is all of them.
    ///
    /// Unparseable entries are skipped rather than raised. `role_turns.artifacts`
    /// is open data — turns legitimately cite commit shas, filenames and one-off
    /// labels beside contract keys — and a value that is not a well-formed name
    /// cannot satisfy a declared artifact anyway. Failing the whole read on one
    /// such label would deny the gate the keys that *are* present.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_task_artifact_keys(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<BTreeSet<ExternalName>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT DISTINCT artifact_key FROM (
                     SELECT artifact_key
                       FROM artifact_evidence
                      WHERE project_id = ?1 AND task_id = ?2
                     UNION
                     SELECT cited.value AS artifact_key
                       FROM task_workflows AS flow
                       JOIN task_gate_evaluations AS gate
                         ON gate.project_id = flow.project_id
                        AND gate.workflow_id = flow.id
                       JOIN json_each(
                                CASE WHEN json_valid(gate.evidence)
                                     THEN gate.evidence ELSE '[]' END
                            ) AS cited
                      WHERE flow.project_id = ?1 AND flow.task_id = ?2
                        AND gate.verdict = 'passed'
                        AND cited.type = 'text'
                     UNION
                     SELECT entry.value AS artifact_key
                       FROM role_turns AS turn
                       JOIN json_each(
                                CASE WHEN json_valid(turn.artifacts)
                                     THEN turn.artifacts ELSE '[]' END
                            ) AS entry
                      WHERE turn.project_id = ?1 AND turn.task_id = ?2
                        AND entry.type = 'text'
                 )
                 ORDER BY artifact_key",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .map_err(backend)?;
        let mut keys = BTreeSet::new();
        for row in rows {
            if let Ok(key) = ExternalName::parse(&row.map_err(backend)?) {
                keys.insert(key);
            }
        }
        Ok(keys)
    }

    /// Record one epic LSA remediation proposal for a failed round.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when a proposal already stands for
    /// that round. Replacing it would change the bounded correction the TPM is
    /// about to route, after the round it answers was already fixed.
    pub fn insert_remediation_proposal(
        &self,
        proposal: &StoredRemediationProposal,
    ) -> RepositoryResult<()> {
        self.connection
            .execute(
                "INSERT INTO epic_completion_remediation_proposals
                     (project_id, mini_project_id, round, failed_round_evidence, proposal,
                      lsa_seat_binding_id, proposed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    proposal.project_id.to_string(),
                    proposal.mini_project_id.to_string(),
                    i64::from(proposal.round),
                    proposal.failed_round_evidence.as_str(),
                    proposal.proposal.as_str(),
                    proposal.lsa_seat_binding_id.to_string(),
                    text(proposal.proposed_at),
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "remediation proposal",
                        rule: "one failed round has one bounded proposal",
                    }
                }
                other => backend(other),
            })?;
        Ok(())
    }

    /// Read the proposal standing for one epic's failed round.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_remediation_proposal(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        round: u8,
    ) -> RepositoryResult<Option<StoredRemediationProposal>> {
        self.connection
            .query_row(
                "SELECT failed_round_evidence, proposal, lsa_seat_binding_id, proposed_at
                 FROM epic_completion_remediation_proposals
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND round = ?3",
                params![
                    project_id.to_string(),
                    mini_project_id.to_string(),
                    i64::from(round)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(|columns| {
                Ok(StoredRemediationProposal {
                    project_id,
                    mini_project_id,
                    round,
                    failed_round_evidence: ContentHash::parse(&columns.0)?,
                    proposal: ContentHash::parse(&columns.1)?,
                    lsa_seat_binding_id: SeatBindingId::parse(&columns.2)?,
                    proposed_at: read_timestamp(&columns.3)?,
                })
            })
            .transpose()
    }

    /// Every wake intent for one epic, oldest revision first.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_completion_wakes(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<StoredCompletionWake>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT completion_revision, reason, seat_binding_id, receipt, appended_at,
                        acknowledged_at
                 FROM epic_completion_wakes
                 WHERE project_id = ?1 AND mini_project_id = ?2
                 ORDER BY completion_revision, reason, seat_binding_id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .map_err(backend)?;
        let mut wakes = Vec::new();
        for row in rows {
            let columns = row.map_err(backend)?;
            wakes.push(StoredCompletionWake {
                project_id,
                mini_project_id,
                completion_revision: AggregateRevision::parse(
                    u64::try_from(columns.0).unwrap_or_default(),
                )?,
                reason: ExternalName::parse(&columns.1)?,
                seat_binding_id: SeatBindingId::parse(&columns.2)?,
                receipt: ContentHash::parse(&columns.3)?,
                appended_at: read_timestamp(&columns.4)?,
                acknowledged_at: columns.5.as_deref().map(read_timestamp).transpose()?,
            });
        }
        Ok(wakes)
    }
}

impl TopologyRepository for SqliteStore {
    fn publish_topology_spec(
        &self,
        project_id: ProjectId,
        spec: &ProjectSessionTopologySpec,
        shareability: &Shareability,
        published_at: Timestamp,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        shareability.validate_for(TOPOLOGY_SPEC_TIER)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO topology_specs
                     (project_id, spec_id, version, name, root_kind, definition,
                      definition_hash, published_at, shareability_class,
                      shareability_classifier, shareability_provenance)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    project_id.to_string(),
                    spec.spec_id.to_string(),
                    version_column(spec.version),
                    spec.name.as_str(),
                    spec.root_kind.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(published_at),
                    shareability.class.as_str(),
                    shareability.classifier.identity().map(ExternalName::as_str),
                    shareability.provenance.as_str(),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_topology_spec_shareability(
        &self,
        project_id: ProjectId,
        spec_id: TopologySpecId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<Shareability>> {
        let found: Option<(String, Option<String>, String)> = self
            .connection
            .query_row(
                "SELECT shareability_class, shareability_classifier,
                        shareability_provenance
                 FROM topology_specs
                 WHERE project_id = ?1 AND spec_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    spec_id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        found.map(stored_shareability).transpose()
    }

    fn get_topology_spec(
        &self,
        project_id: ProjectId,
        spec_id: TopologySpecId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<ProjectSessionTopologySpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM topology_specs
                 WHERE project_id = ?1 AND spec_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    spec_id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document(&json, &hash))
            .transpose()
    }

    fn list_topology_specs(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<ProjectSessionTopologySpec>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT definition, definition_hash FROM topology_specs
                 WHERE project_id = ?1 ORDER BY spec_id, version",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut specs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let document = stored_payload(
                &row.get::<_, String>(0).map_err(backend)?,
                &row.get::<_, String>(1).map_err(backend)?,
            )?;
            specs.push(document.deserialize::<ProjectSessionTopologySpec>()?);
        }
        Ok(specs)
    }

    fn set_project_topology_default(
        &self,
        selection: &ProjectTopologyDefault,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        topology_spec_in(&transaction, selection.project_id, &selection.topology)?;
        transaction
            .execute(
                "INSERT INTO project_topology_defaults
                     (project_id, spec_id, version, canonical_hash, selected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project_id) DO UPDATE SET
                     spec_id = excluded.spec_id,
                     version = excluded.version,
                     canonical_hash = excluded.canonical_hash,
                     selected_at = excluded.selected_at",
                params![
                    selection.project_id.to_string(),
                    selection.topology.spec_id.to_string(),
                    version_column(selection.topology.version),
                    selection.topology.canonical_hash.as_str(),
                    text(selection.selected_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn get_project_topology_default(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<ProjectTopologyDefault>> {
        let found: Option<(String, i64, String, String)> = self
            .connection
            .query_row(
                "SELECT spec_id, version, canonical_hash, selected_at
                 FROM project_topology_defaults WHERE project_id = ?1",
                params![project_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(spec_id, version, hash, selected_at)| {
                Ok(ProjectTopologyDefault {
                    project_id,
                    topology: TopologySnapshot {
                        spec_id: TopologySpecId::parse(&spec_id)?,
                        version: read_version(version)?,
                        canonical_hash: ContentHash::parse(&hash)?,
                    },
                    selected_at: read_timestamp(&selected_at)?,
                })
            })
            .transpose()
    }

    fn pin_mini_project_topology(
        &self,
        snapshot: &MiniProjectTopologySnapshot,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let owns_mini_project = transaction
            .query_row(
                "SELECT 1 FROM mini_projects WHERE project_id = ?1 AND id = ?2",
                params![
                    snapshot.project_id.to_string(),
                    snapshot.mini_project_id.to_string()
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(backend)?
            .is_some();
        if !owns_mini_project {
            return Err(RepositoryError::NotFound {
                subject: "mini project",
            });
        }
        topology_spec_in(&transaction, snapshot.project_id, &snapshot.topology)?;
        transaction
            .execute(
                "INSERT INTO mini_project_topology_snapshots
                     (mini_project_id, project_id, spec_id, version, canonical_hash, pinned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    snapshot.mini_project_id.to_string(),
                    snapshot.project_id.to_string(),
                    snapshot.topology.spec_id.to_string(),
                    version_column(snapshot.topology.version),
                    snapshot.topology.canonical_hash.as_str(),
                    text(snapshot.pinned_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn repin_mini_project_topology(
        &self,
        snapshot: &MiniProjectTopologySnapshot,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        // The pin must already exist. An epic with none has not been upgraded,
        // it has never been placed, and creating the first pin here would let an
        // upgrade stand in for the placement it is supposed to be moving.
        let pinned = transaction
            .query_row(
                "SELECT 1 FROM mini_project_topology_snapshots
                 WHERE project_id = ?1 AND mini_project_id = ?2",
                params![
                    snapshot.project_id.to_string(),
                    snapshot.mini_project_id.to_string()
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(backend)?
            .is_some();
        if !pinned {
            return Err(RepositoryError::NotFound {
                subject: "mini-project topology snapshot",
            });
        }
        // And the revision it moves to must be one this project published and
        // able to represent the exact nodes, hierarchy, native containers and
        // seats the epic already owns. The node stamps and the epic pin move in
        // this one transaction; neither half may become visible on its own.
        repin_mini_project_nodes_in(&transaction, snapshot)?;
        transaction
            .execute(
                "UPDATE mini_project_topology_snapshots
                 SET spec_id = ?1, version = ?2, canonical_hash = ?3, pinned_at = ?4
                 WHERE project_id = ?5 AND mini_project_id = ?6",
                params![
                    snapshot.topology.spec_id.to_string(),
                    version_column(snapshot.topology.version),
                    snapshot.topology.canonical_hash.as_str(),
                    text(snapshot.pinned_at),
                    snapshot.project_id.to_string(),
                    snapshot.mini_project_id.to_string(),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn get_mini_project_topology(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<MiniProjectTopologySnapshot>> {
        let found: Option<(String, i64, String, String)> = self
            .connection
            .query_row(
                "SELECT spec_id, version, canonical_hash, pinned_at
                 FROM mini_project_topology_snapshots
                 WHERE project_id = ?1 AND mini_project_id = ?2",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(spec_id, version, hash, pinned_at)| {
                Ok(MiniProjectTopologySnapshot {
                    project_id,
                    mini_project_id,
                    topology: TopologySnapshot {
                        spec_id: TopologySpecId::parse(&spec_id)?,
                        version: read_version(version)?,
                        canonical_hash: ContentHash::parse(&hash)?,
                    },
                    pinned_at: read_timestamp(&pinned_at)?,
                })
            })
            .transpose()
    }

    fn publish_role_catalog(
        &self,
        catalog: &RoleCatalogRevision,
        shareability: &Shareability,
        published_at: Timestamp,
    ) -> RepositoryResult<ContentHash> {
        let document = catalog.canonicalize()?;
        shareability.validate_for(ROLE_CATALOG_TIER)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO role_catalog_revisions
                     (catalog_id, version, name, definition, definition_hash, published_at,
                      shareability_class, shareability_classifier, shareability_provenance)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    catalog.catalog_id.to_string(),
                    version_column(catalog.version),
                    catalog.name.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(published_at),
                    shareability.class.as_str(),
                    shareability.classifier.identity().map(ExternalName::as_str),
                    shareability.provenance.as_str(),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_role_catalog_shareability(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<Shareability>> {
        let found: Option<(String, Option<String>, String)> = self
            .connection
            .query_row(
                "SELECT shareability_class, shareability_classifier,
                        shareability_provenance
                 FROM role_catalog_revisions
                 WHERE catalog_id = ?1 AND version = ?2",
                params![catalog_id.to_string(), version_column(version)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        found.map(stored_shareability).transpose()
    }

    fn get_role_catalog(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<RoleCatalogRevision>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM role_catalog_revisions
                 WHERE catalog_id = ?1 AND version = ?2",
                params![catalog_id.to_string(), version_column(version)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document(&json, &hash))
            .transpose()
    }

    fn create_topology_node(
        &self,
        request: &NewSessionTopologyNode,
    ) -> RepositoryResult<SessionTopologyNode> {
        let transaction = self.begin()?;
        let spec = topology_spec_in(&transaction, request.project_id, &request.topology)?;
        let declared = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == request.kind)
            .ok_or(DomainError::Invalid {
                subject: "SessionTopologyNode",
                rule: "names a kind absent from the pinned topology specification",
            })?;

        if let Some(mini_project_id) = request.mini_project_id {
            let pinned: Option<(String, i64, String)> = transaction
                .query_row(
                    "SELECT spec_id, version, canonical_hash
                     FROM mini_project_topology_snapshots
                     WHERE project_id = ?1 AND mini_project_id = ?2",
                    params![request.project_id.to_string(), mini_project_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(backend)?;
            let Some((spec_id, version, hash)) = pinned else {
                return Err(RepositoryError::NotFound {
                    subject: "mini-project topology snapshot",
                });
            };
            if TopologySpecId::parse(&spec_id)? != request.topology.spec_id
                || read_version(version)? != request.topology.version
                || ContentHash::parse(&hash)? != request.topology.canonical_hash
            {
                return Err(conflict(
                    "topology node",
                    "the node topology differs from the MiniProject snapshot",
                ));
            }
        }

        match request.parent_id {
            None if request.kind != spec.root_kind => {
                return Err(DomainError::invalid(
                    "SessionTopologyNode",
                    "only the declared root kind may omit a parent",
                )
                .into());
            }
            Some(_) if request.kind == spec.root_kind => {
                return Err(DomainError::invalid(
                    "SessionTopologyNode",
                    "the declared root kind must not have a parent",
                )
                .into());
            }
            Some(parent_id) => {
                let parent: Option<RepositoryResult<SessionTopologyNode>> = transaction
                    .query_row(
                        &format!(
                            "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                             WHERE project_id = ?1 AND id = ?2"
                        ),
                        params![request.project_id.to_string(), parent_id.to_string()],
                        |row| Ok(read_topology_node(row)),
                    )
                    .optional()
                    .map_err(backend)?;
                let parent = parent.transpose()?.ok_or(RepositoryError::NotFound {
                    subject: "topology parent",
                })?;
                if parent.lifecycle.is_terminal()
                    || parent
                        .mini_project_id
                        .is_some_and(|scope| Some(scope) != request.mini_project_id)
                    || parent.topology != request.topology
                    || !declared.allowed_parents.contains(&parent.kind)
                {
                    return Err(conflict(
                        "topology node",
                        "the parent is terminal, outside the node scope, or has an illegal kind",
                    ));
                }
            }
            None => {}
        }

        if let Some(maximum) = declared.cardinality.maximum {
            let count: i64 = transaction
                .query_row(
                    "SELECT count(*) FROM topology_nodes
                     WHERE project_id = ?1
                       AND mini_project_id IS ?2
                       AND parent_id IS ?3
                       AND kind = ?4
                       AND lifecycle <> 'archived'",
                    params![
                        request.project_id.to_string(),
                        request.mini_project_id.map(|id| id.to_string()),
                        request.parent_id.map(|id| id.to_string()),
                        request.kind.as_str(),
                    ],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if count >= i64::from(maximum) {
                return Err(conflict(
                    "topology node",
                    "the declared maximum cardinality is already occupied",
                ));
            }
        }

        transaction
            .execute(
                "INSERT INTO topology_nodes
                     (id, project_id, mini_project_id, spec_id, spec_version, spec_hash,
                      kind, parent_id, lifecycle, placement, revision, created_at, updated_at,
                      task_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', 'unbound', 1, ?9, ?9, ?10)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.mini_project_id.map(|id| id.to_string()),
                    request.topology.spec_id.to_string(),
                    version_column(request.topology.version),
                    request.topology.canonical_hash.as_str(),
                    request.kind.as_str(),
                    request.parent_id.map(|id| id.to_string()),
                    text(request.created_at),
                    request.task_id.map(|id| id.to_string()),
                ],
            )
            .map_err(backend)?;
        let node = transaction
            .query_row(
                &format!(
                    "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![request.project_id.to_string(), request.id.to_string()],
                |row| Ok(read_topology_node(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(node)
    }

    fn list_topology_nodes(
        &self,
        project_id: ProjectId,
        mini_project_id: Option<MiniProjectId>,
    ) -> RepositoryResult<Vec<SessionTopologyNode>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                 WHERE project_id = ?1 AND mini_project_id IS ?2
                 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                mini_project_id.map(|id| id.to_string())
            ])
            .map_err(backend)?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            nodes.push(read_topology_node(row)?);
        }
        Ok(nodes)
    }

    fn list_project_topology_nodes(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<SessionTopologyNode>> {
        // Created-at order puts parents before children without a recursive
        // walk: a child cannot be created before the parent it names.
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                 WHERE project_id = ?1 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            nodes.push(read_topology_node(row)?);
        }
        Ok(nodes)
    }

    fn get_topology_node(
        &self,
        project_id: ProjectId,
        id: TopologyNodeId,
    ) -> RepositoryResult<Option<SessionTopologyNode>> {
        let node: Option<RepositoryResult<SessionTopologyNode>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_topology_node(row)),
            )
            .optional()
            .map_err(backend)?;
        node.transpose()
    }

    fn transition_topology_node(
        &self,
        project_id: ProjectId,
        id: TopologyNodeId,
        lifecycle: TopologyLifecycle,
        expected_revision: AggregateRevision,
        updated_at: Timestamp,
    ) -> RepositoryResult<SessionTopologyNode> {
        let transaction = self.begin()?;
        let current: Option<RepositoryResult<SessionTopologyNode>> = transaction
            .query_row(
                &format!(
                    "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_topology_node(row)),
            )
            .optional()
            .map_err(backend)?;
        let current = current.transpose()?.ok_or(RepositoryError::NotFound {
            subject: "topology node",
        })?;
        current
            .revision
            .expect("topology node", expected_revision)?;

        let advances = matches!(
            (current.lifecycle, lifecycle),
            (TopologyLifecycle::Active, TopologyLifecycle::Retired)
                | (TopologyLifecycle::Retired, TopologyLifecycle::Archived)
        );
        if !advances {
            return Err(conflict(
                "topology node",
                "the lifecycle only advances active to retired to archived",
            ));
        }

        // Retiring a node concludes that everything below it is finished with.
        // A node with a live child or a non-terminal seat is not, and retiring
        // it would leave both addressable under a parent nothing may use.
        if lifecycle == TopologyLifecycle::Retired {
            let live_children: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM topology_nodes
                     WHERE project_id = ?1 AND parent_id = ?2 AND lifecycle = 'active'",
                    params![project_id.to_string(), id.to_string()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if live_children > 0 {
                return Err(conflict(
                    "topology node",
                    "the node still has active children",
                ));
            }
            let live_seats: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM seat_bindings
                     WHERE project_id = ?1 AND topology_node_id = ?2 AND lifecycle = 'active'",
                    params![project_id.to_string(), id.to_string()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if live_seats > 0 {
                return Err(conflict(
                    "topology node",
                    "the node still hosts active seats",
                ));
            }
        }

        transaction
            .execute(
                "UPDATE topology_nodes SET lifecycle = ?1, revision = revision + 1, updated_at = ?2
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    lifecycle.as_str(),
                    text(updated_at),
                    project_id.to_string(),
                    id.to_string(),
                    revision_column(expected_revision)?,
                ],
            )
            .map_err(backend)?;
        let updated = transaction
            .query_row(
                &format!(
                    "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_topology_node(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(updated)
    }

    fn create_seat_binding(&self, request: &NewSeatBinding) -> RepositoryResult<SeatBinding> {
        let transaction = self.begin()?;
        let node: Option<RepositoryResult<SessionTopologyNode>> = transaction
            .query_row(
                &format!(
                    "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![
                    request.project_id.to_string(),
                    request.topology_node_id.to_string()
                ],
                |row| Ok(read_topology_node(row)),
            )
            .optional()
            .map_err(backend)?;
        let node = node.transpose()?.ok_or(RepositoryError::NotFound {
            subject: "topology node",
        })?;
        if node.lifecycle.is_terminal() {
            return Err(conflict("seat binding", "the topology node is archived"));
        }
        let spec = topology_spec_in(&transaction, request.project_id, &node.topology)?;
        let hosts_sessions = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == node.kind)
            .is_some_and(|declared| {
                declared
                    .projection_capabilities
                    .contains(&kontor_core::spec::NodeProjectionCapability::SessionHost)
            });
        if !hosts_sessions {
            return Err(conflict(
                "seat binding",
                "the topology node kind is not declared as a session host",
            ));
        }
        let catalog = role_catalog_in(
            &transaction,
            request.role.catalog_id,
            request.role.catalog_revision,
        )?;
        request.role.validate_against(&catalog)?;

        if let Some(task_id) = request.task_id {
            let task_scope: Option<Option<String>> = transaction
                .query_row(
                    "SELECT mini_project_id FROM tasks WHERE project_id = ?1 AND id = ?2",
                    params![request.project_id.to_string(), task_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let task_scope = task_scope.ok_or(RepositoryError::NotFound { subject: "task" })?;
            if task_scope
                .as_deref()
                .map(MiniProjectId::parse)
                .transpose()?
                != node.mini_project_id
            {
                return Err(conflict(
                    "seat binding",
                    "the task and topology node belong to different MiniProject scopes",
                ));
            }
        }
        if let Some(team_run_id) = request.team_run_id {
            let task_id: Option<String> = transaction
                .query_row(
                    "SELECT task_id FROM team_runs WHERE project_id = ?1 AND id = ?2",
                    params![request.project_id.to_string(), team_run_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let task_id = task_id.ok_or(RepositoryError::NotFound {
                subject: "team run",
            })?;
            if request.task_id != Some(TaskId::parse(&task_id)?) {
                return Err(conflict(
                    "seat binding",
                    "the TeamRun does not belong to the selected task",
                ));
            }
        }

        transaction
            .execute(
                "INSERT INTO seat_bindings
                     (id, project_id, topology_node_id, role_slot_id, role_catalog_id,
                      role_catalog_version, role_code, standard_title, custom_display_name,
                      task_id, team_run_id, lifecycle, attach_deadline,
                      parent_seat_binding_id, revision, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         'active', ?12, ?13, 1, ?14, ?14)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.topology_node_id.to_string(),
                    request.role_slot_id.to_string(),
                    request.role.catalog_id.to_string(),
                    version_column(request.role.catalog_revision),
                    request.role.role_code.as_str(),
                    request.role.standard_title.as_str(),
                    request
                        .role
                        .custom_display_name
                        .as_ref()
                        .map(ExternalName::as_str),
                    request.task_id.map(|id| id.to_string()),
                    request.team_run_id.map(|id| id.to_string()),
                    text(request.attach_deadline),
                    request.parent_seat_binding_id.map(|id| id.to_string()),
                    text(request.created_at),
                ],
            )
            .map_err(backend)?;
        let binding = transaction
            .query_row(
                &format!(
                    "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![request.project_id.to_string(), request.id.to_string()],
                |row| Ok(read_seat_binding(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(binding)
    }

    fn get_task_topology_node(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<SessionTopologyNode>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {TOPOLOGY_NODE_COLUMNS} FROM topology_nodes
                     WHERE project_id = ?1 AND task_id = ?2 AND lifecycle = 'active'"
                ),
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok(read_topology_node(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn observe_seat_binding(
        &self,
        project_id: ProjectId,
        id: SeatBindingId,
        observation: &SeatLivenessObservation,
        observed_at: Timestamp,
    ) -> RepositoryResult<SeatBinding> {
        if observation.replaced_by == Some(id) {
            return Err(conflict(
                "seat binding",
                "a seat cannot be its own replacement",
            ));
        }
        let transaction = self.begin()?;
        // COALESCE, never assignment: an observation carries what was seen and
        // nothing else, so recording an attachment must not erase an activity
        // instant recorded a moment earlier by a different observer. The one
        // exception is `runtime_reported`, which is a *current* self-report and
        // is meant to be replaced by the latest one.
        let changed = transaction
            .execute(
                // Releasing also retires the row. A released seat is finished,
                // and leaving it `active` would keep it occupying the unique
                // `(node, role_slot)` key — so the slot it no longer holds could
                // never be filled again. Its evidence is untouched: every
                // conclusion below still reads `released_at`, and
                // `closes_children` was already true either way.
                "UPDATE seat_bindings SET
                     last_attached_at = COALESCE(?3, last_attached_at),
                     last_activity_at = COALESCE(?4, last_activity_at),
                     runtime_reported = COALESCE(?5, runtime_reported),
                     released_at = COALESCE(released_at, ?6),
                     replaced_by_seat_binding_id =
                         COALESCE(replaced_by_seat_binding_id, ?7),
                     lifecycle = CASE
                         WHEN COALESCE(released_at, ?6) IS NOT NULL THEN 'retired'
                         ELSE lifecycle
                     END,
                     revision = revision + 1,
                     updated_at = ?8
                 WHERE project_id = ?1 AND id = ?2",
                params![
                    project_id.to_string(),
                    id.to_string(),
                    observation.attached_at.map(text),
                    observation.activity_at.map(text),
                    observation.runtime_reported.map(|it| it.as_str()),
                    observation.released_at.map(text),
                    observation.replaced_by.map(|it| it.to_string()),
                    text(observed_at),
                ],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Err(RepositoryError::NotFound {
                subject: "seat binding",
            });
        }
        let binding = transaction
            .query_row(
                &format!(
                    "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_seat_binding(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(binding)
    }

    fn bind_topology_node_container(
        &self,
        request: &NewNativeContainerBinding,
    ) -> RepositoryResult<NativeContainerBinding> {
        let transaction = self.begin()?;
        let owning_project: Option<String> = transaction
            .query_row(
                "SELECT project_id FROM topology_nodes WHERE id = ?1",
                params![request.topology_node_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let owning_project = owning_project.ok_or(RepositoryError::NotFound {
            subject: "topology node",
        })?;
        if ProjectId::parse(&owning_project)? != request.project_id {
            return Err(conflict(
                "native container binding",
                "the topology node belongs to another project",
            ));
        }

        let existing: Option<NativeContainerBinding> = transaction
            .query_row(
                &format!(
                    "SELECT {NATIVE_CONTAINER_COLUMNS} FROM topology_node_containers
                     WHERE topology_node_id = ?1"
                ),
                params![request.topology_node_id.to_string()],
                |row| Ok(read_native_container_binding(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()?;

        if let Some(existing) = existing {
            // Re-confirming the binding this node already holds advances the
            // readback instant and nothing else. A *different* identity is a
            // disagreement between Kontor and the runtime, and this is not the
            // place that resolves one: silently rewriting the row would make
            // the node point at whatever was seen last, which is precisely the
            // repair OP-02 forbids.
            if existing.identity != request.identity {
                return Err(conflict(
                    "native container binding",
                    "this topology node is bound to another native container",
                ));
            }
            transaction
                .execute(
                    "UPDATE topology_node_containers
                     SET last_readback_at = ?2, observed_kind = ?3, revision = revision + 1
                     WHERE topology_node_id = ?1",
                    params![
                        request.topology_node_id.to_string(),
                        text(request.observed_at),
                        request.observed_kind.as_str(),
                    ],
                )
                .map_err(backend)?;
        } else {
            // A native container already owned by another node is a collision,
            // and it is refused here rather than discovered later — by then
            // both nodes have been treated as placed.
            transaction
                .execute(
                    "INSERT INTO topology_node_containers
                         (topology_node_id, project_id, container_binding_id, runtime_kind,
                          host, generation, native_id, observed_kind, canonical_cwd,
                          bound_at, last_readback_at, revision)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, 1)",
                    params![
                        request.topology_node_id.to_string(),
                        request.project_id.to_string(),
                        request.container_binding_id.as_str(),
                        request.identity.runtime_kind.as_str(),
                        request.identity.host.as_str(),
                        i64::try_from(request.identity.generation).map_err(|_| {
                            DomainError::invalid(
                                "native container generation",
                                "is outside the storable range",
                            )
                        })?,
                        request.identity.native_id.as_str(),
                        request.observed_kind.as_str(),
                        request.canonical_cwd.as_ref().map(ExternalName::as_str),
                        text(request.observed_at),
                    ],
                )
                .map_err(|error| match &error {
                    rusqlite::Error::SqliteFailure(failure, _)
                        if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        conflict(
                            "native container binding",
                            "this native container is already bound to another topology node",
                        )
                    }
                    _ => backend(error),
                })?;
        }

        // The container row and the node's derived placement are one fact.
        // Persist them in this transaction so no projection can claim a node is
        // unbound while simultaneously returning its exact native identity.
        // Re-attesting an already-bound node is idempotent for node revision;
        // only the container readback timestamp advances above.
        transaction
            .execute(
                "UPDATE topology_nodes
                 SET placement = 'bound', revision = revision + 1, updated_at = ?3
                 WHERE project_id = ?1 AND id = ?2 AND placement <> 'bound'",
                params![
                    request.project_id.to_string(),
                    request.topology_node_id.to_string(),
                    text(request.observed_at),
                ],
            )
            .map_err(backend)?;

        let binding = transaction
            .query_row(
                &format!(
                    "SELECT {NATIVE_CONTAINER_COLUMNS} FROM topology_node_containers
                     WHERE topology_node_id = ?1"
                ),
                params![request.topology_node_id.to_string()],
                |row| Ok(read_native_container_binding(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(binding)
    }

    fn list_seat_attachments(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        now: Timestamp,
    ) -> RepositoryResult<Vec<SeatAttachment>> {
        let transaction = self.begin()?;
        let mut statement = transaction
            .prepare(&format!(
                "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
                 WHERE project_id = ?1 AND topology_node_id = ?2 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                topology_node_id.to_string()
            ])
            .map_err(backend)?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            bindings.push(read_seat_binding(row)?);
        }
        drop(rows);
        drop(statement);
        conclude_seat_attachments(&transaction, project_id, &bindings, now)
    }

    fn get_topology_node_container(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<NativeContainerBinding>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {NATIVE_CONTAINER_COLUMNS} FROM topology_node_containers
                     WHERE project_id = ?1 AND topology_node_id = ?2"
                ),
                params![project_id.to_string(), topology_node_id.to_string()],
                |row| Ok(read_native_container_binding(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn list_seat_bindings(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Vec<SeatBinding>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
                 WHERE project_id = ?1 AND topology_node_id = ?2
                 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                topology_node_id.to_string()
            ])
            .map_err(backend)?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            bindings.push(read_seat_binding(row)?);
        }
        Ok(bindings)
    }

    fn get_seat_binding(
        &self,
        project_id: ProjectId,
        id: SeatBindingId,
    ) -> RepositoryResult<Option<SeatBinding>> {
        let binding: Option<RepositoryResult<SeatBinding>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_seat_binding(row)),
            )
            .optional()
            .map_err(backend)?;
        binding.transpose()
    }

    fn create_adaptive_admission_state(
        &self,
        request: &NewAdaptiveAdmissionState,
    ) -> RepositoryResult<AdaptiveAdmissionState> {
        validate_adaptive_values(request.current_window, request.clean_observation_streak)?;
        let transaction = self.begin()?;
        let owns_mini_project = transaction
            .query_row(
                "SELECT 1 FROM mini_projects WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.mini_project_id.to_string()
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(backend)?
            .is_some();
        if !owns_mini_project {
            return Err(RepositoryError::NotFound {
                subject: "mini project",
            });
        }
        transaction
            .execute(
                "INSERT INTO adaptive_admission_state
                     (project_id, mini_project_id, current_window, clean_observation_streak,
                      last_observation_id, revision, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![
                    request.project_id.to_string(),
                    request.mini_project_id.to_string(),
                    i64::from(request.current_window),
                    i64::from(request.clean_observation_streak),
                    request.last_observation_id.as_ref().map(ExternalId::as_str),
                    text(request.created_at),
                ],
            )
            .map_err(backend)?;
        let state = transaction
            .query_row(
                &format!(
                    "SELECT {ADAPTIVE_ADMISSION_COLUMNS} FROM adaptive_admission_state
                     WHERE project_id = ?1 AND mini_project_id = ?2"
                ),
                params![
                    request.project_id.to_string(),
                    request.mini_project_id.to_string()
                ],
                |row| Ok(read_adaptive_admission(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(state)
    }

    fn advance_adaptive_admission_state(
        &self,
        request: &AdaptiveAdmissionAdvance,
    ) -> RepositoryResult<AdaptiveAdmissionState> {
        validate_adaptive_values(request.current_window, request.clean_observation_streak)?;
        let transaction = self.begin()?;
        let current: Option<RepositoryResult<AdaptiveAdmissionState>> = transaction
            .query_row(
                &format!(
                    "SELECT {ADAPTIVE_ADMISSION_COLUMNS} FROM adaptive_admission_state
                     WHERE project_id = ?1 AND mini_project_id = ?2"
                ),
                params![
                    request.project_id.to_string(),
                    request.mini_project_id.to_string()
                ],
                |row| Ok(read_adaptive_admission(row)),
            )
            .optional()
            .map_err(backend)?;
        let current = current.transpose()?.ok_or(RepositoryError::NotFound {
            subject: "adaptive admission state",
        })?;
        current
            .revision
            .expect("adaptive admission state", request.expected_revision)?;
        if request.last_observation_id.is_some()
            && request.last_observation_id == current.last_observation_id
        {
            return Err(conflict(
                "adaptive admission state",
                "the observation was already applied",
            ));
        }
        transaction
            .execute(
                "UPDATE adaptive_admission_state
                 SET current_window = ?1, clean_observation_streak = ?2,
                     last_observation_id = ?3, revision = revision + 1, updated_at = ?4
                 WHERE project_id = ?5 AND mini_project_id = ?6 AND revision = ?7",
                params![
                    i64::from(request.current_window),
                    i64::from(request.clean_observation_streak),
                    request.last_observation_id.as_ref().map(ExternalId::as_str),
                    text(request.updated_at),
                    request.project_id.to_string(),
                    request.mini_project_id.to_string(),
                    revision_column(request.expected_revision)?,
                ],
            )
            .map_err(backend)?;
        let state = transaction
            .query_row(
                &format!(
                    "SELECT {ADAPTIVE_ADMISSION_COLUMNS} FROM adaptive_admission_state
                     WHERE project_id = ?1 AND mini_project_id = ?2"
                ),
                params![
                    request.project_id.to_string(),
                    request.mini_project_id.to_string()
                ],
                |row| Ok(read_adaptive_admission(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(state)
    }

    fn list_adaptive_admission_states(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<AdaptiveAdmissionState>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {ADAPTIVE_ADMISSION_COLUMNS} FROM adaptive_admission_state
                 WHERE project_id = ?1 ORDER BY mini_project_id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut states = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            states.push(read_adaptive_admission(row)?);
        }
        Ok(states)
    }

    fn get_adaptive_admission_state(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<AdaptiveAdmissionState>> {
        let state: Option<RepositoryResult<AdaptiveAdmissionState>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {ADAPTIVE_ADMISSION_COLUMNS} FROM adaptive_admission_state
                     WHERE project_id = ?1 AND mini_project_id = ?2"
                ),
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| Ok(read_adaptive_admission(row)),
            )
            .optional()
            .map_err(backend)?;
        state.transpose()
    }
}

// ---------------------------------------------------------------------------
// Account-owned capacity evidence
// ---------------------------------------------------------------------------

const CAPACITY_OBSERVATION_COLUMNS: &str = "id, project_id, account_profile_id, observed_at, \
    reading, reading_hash, available, pressure, cooling_until";

const AVAILABILITY_OVERRIDE_COLUMNS: &str = "project_id, account_profile_id, available, reason, \
    expires_at, revision, updated_at";

const PROVIDER_QUOTA_STATE_COLUMNS: &str = "project_id, account_profile_id, provider, state, \
    resets_at, evidence_hash, source, observed_at, revision, updated_at, credit_minor_units, \
    credit_reserve_minor_units, credit_currency";

/// The window set for one pair, ordered by kind so a stored row and a re-read of
/// it are byte-identical.
const PROVIDER_QUOTA_WINDOW_COLUMNS: &str = "kind, resets_at, used_percent";

/// Read one header row. `windows` is filled by the caller, which holds the
/// connection the set has to be read through.
fn read_provider_quota_state(row: &Row<'_>) -> RepositoryResult<ProviderQuotaState> {
    let resets_at: Option<String> = row.get(4).map_err(backend)?;
    Ok(ProviderQuotaState {
        project_id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        account_profile_id: AccountProfileId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        provider: row.get::<_, String>(2).map_err(backend)?,
        state: ProviderQuotaKind::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        resets_at: resets_at.as_deref().map(read_timestamp).transpose()?,
        windows: Vec::new(),
        credit: read_credit_balance(row)?,
        evidence_hash: ContentHash::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        source: ProviderQuotaSource::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        observed_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(8).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(9).map_err(backend)?)?,
    })
}

/// The balance and its floor, which the schema keeps in one currency.
///
/// All three columns are null together or present together — a CHECK, not a
/// convention — so a half-written balance is unreadable rather than silently
/// defaulted to zero, which would refuse every launch on the provider.
fn read_credit_balance(row: &Row<'_>) -> RepositoryResult<Option<CreditBalance>> {
    let remaining: Option<i64> = row.get(10).map_err(backend)?;
    let reserve: Option<i64> = row.get(11).map_err(backend)?;
    let currency: Option<String> = row.get(12).map_err(backend)?;
    let (Some(remaining), Some(reserve), Some(currency)) = (remaining, reserve, currency) else {
        return Ok(None);
    };
    let currency = CurrencyCode::parse(&currency)?;
    Ok(Some(CreditBalance {
        remaining: Money {
            minor_units: minor_units_of(remaining)?,
            currency,
        },
        reserve: Money {
            minor_units: minor_units_of(reserve)?,
            currency,
        },
    }))
}

/// A non-negative minor-unit amount as the schema stores it.
fn minor_units_of(stored: i64) -> RepositoryResult<u64> {
    u64::try_from(stored).map_err(|_| {
        RepositoryError::Domain(DomainError::invalid(
            "CreditBalance",
            "a stored minor-unit amount is negative",
        ))
    })
}

fn read_provider_quota_window(row: &Row<'_>) -> RepositoryResult<QuotaWindow> {
    let used_percent: i64 = row.get(2).map_err(backend)?;
    Ok(QuotaWindow {
        kind: QuotaWindowKind::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        resets_at: read_timestamp(&row.get::<_, String>(1).map_err(backend)?)?,
        used_percent: u8::try_from(used_percent).map_err(|_| {
            RepositoryError::Domain(DomainError::invalid(
                "QuotaWindow",
                "a stored consumption share is not a percentage",
            ))
        })?,
    })
}

/// Attach every window belonging to one already-read header row.
fn attach_provider_quota_windows(
    connection: &Connection,
    state: &mut ProviderQuotaState,
) -> RepositoryResult<()> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {PROVIDER_QUOTA_WINDOW_COLUMNS} FROM provider_quota_windows
             WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3
             ORDER BY kind"
        ))
        .map_err(backend)?;
    let mut rows = statement
        .query(params![
            state.project_id.to_string(),
            state.account_profile_id.to_string(),
            state.provider.as_str(),
        ])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        state.windows.push(read_provider_quota_window(row)?);
    }
    Ok(())
}

fn read_capacity_observation(row: &Row<'_>) -> RepositoryResult<CapacityObservation> {
    let cooling_until: Option<String> = row.get(8).map_err(backend)?;
    Ok(CapacityObservation {
        id: CapacityObservationId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        account_profile_id: AccountProfileId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        observed_at: read_timestamp(&row.get::<_, String>(3).map_err(backend)?)?,
        reading: stored_payload(
            &row.get::<_, String>(4).map_err(backend)?,
            &row.get::<_, String>(5).map_err(backend)?,
        )?,
        available: row.get::<_, i64>(6).map_err(backend)? != 0,
        pressure: row.get::<_, i64>(7).map_err(backend)? != 0,
        cooling_until: cooling_until.as_deref().map(read_timestamp).transpose()?,
    })
}

fn read_capacity_configuration(row: &Row<'_>) -> RepositoryResult<StoredCapacityConfiguration> {
    Ok(StoredCapacityConfiguration {
        ceilings: stored_payload(
            &row.get::<_, String>(0).map_err(backend)?,
            &row.get::<_, String>(1).map_err(backend)?,
        )?,
        revision: revision_of(row.get::<_, i64>(2).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(3).map_err(backend)?)?,
    })
}

fn read_availability_override(row: &Row<'_>) -> RepositoryResult<AvailabilityOverride> {
    let expires_at: Option<String> = row.get(4).map_err(backend)?;
    Ok(AvailabilityOverride {
        project_id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        account_profile_id: AccountProfileId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        available: row.get::<_, i64>(2).map_err(backend)? != 0,
        reason: ExternalName::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        expires_at: expires_at.as_deref().map(read_timestamp).transpose()?,
        revision: revision_of(row.get::<_, i64>(5).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
    })
}

impl CapacityRepository for SqliteStore {
    fn record_capacity_observation(
        &self,
        request: &NewCapacityObservation,
    ) -> RepositoryResult<CapacityObservation> {
        let transaction = self.begin()?;
        // The account is proved to exist here rather than left to the foreign
        // key, so a collector that read an account this project does not own is
        // told which thing was wrong instead of getting a generic constraint
        // refusal.
        if read_account_profile_in(&transaction, request.project_id, request.account_profile_id)?
            .is_none()
        {
            return Err(RepositoryError::NotFound {
                subject: "account profile",
            });
        }
        transaction
            .execute(
                "INSERT INTO capacity_observations
                     (id, project_id, account_profile_id, observed_at, reading, reading_hash,
                      available, pressure, cooling_until)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.account_profile_id.to_string(),
                    text(request.observed_at),
                    request.reading.json(),
                    request.reading.hash().as_str(),
                    i64::from(request.available),
                    i64::from(request.pressure),
                    request.cooling_until.map(text),
                ],
            )
            .map_err(backend)?;
        let observation = transaction
            .query_row(
                &format!(
                    "SELECT {CAPACITY_OBSERVATION_COLUMNS} FROM capacity_observations
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![request.project_id.to_string(), request.id.to_string()],
                |row| Ok(read_capacity_observation(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(observation)
    }

    fn get_capacity_observation(
        &self,
        project_id: ProjectId,
        id: CapacityObservationId,
    ) -> RepositoryResult<Option<CapacityObservation>> {
        let observation: Option<RepositoryResult<CapacityObservation>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {CAPACITY_OBSERVATION_COLUMNS} FROM capacity_observations
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_capacity_observation(row)),
            )
            .optional()
            .map_err(backend)?;
        observation.transpose()
    }

    fn latest_capacity_observations(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<CapacityObservation>> {
        // Latest per account by observation instant, with the id as the
        // tie-break: two readings taken in the same second still have a stable
        // order, because the id is time-ordered.
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {CAPACITY_OBSERVATION_COLUMNS} FROM capacity_observations AS outer_row
                 WHERE project_id = ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM capacity_observations AS newer
                       WHERE newer.project_id = outer_row.project_id
                         AND newer.account_profile_id = outer_row.account_profile_id
                         AND (newer.observed_at, newer.id) > (outer_row.observed_at, outer_row.id)
                   )
                 ORDER BY account_profile_id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut observations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            observations.push(read_capacity_observation(row)?);
        }
        Ok(observations)
    }

    fn set_availability_override(
        &self,
        request: &NewAvailabilityOverride,
    ) -> RepositoryResult<AvailabilityOverride> {
        let transaction = self.begin()?;
        if read_account_profile_in(&transaction, request.project_id, request.account_profile_id)?
            .is_none()
        {
            return Err(RepositoryError::NotFound {
                subject: "account profile",
            });
        }
        let current: Option<RepositoryResult<AvailabilityOverride>> = transaction
            .query_row(
                &format!(
                    "SELECT {AVAILABILITY_OVERRIDE_COLUMNS} FROM availability_overrides
                     WHERE project_id = ?1 AND account_profile_id = ?2"
                ),
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string()
                ],
                |row| Ok(read_availability_override(row)),
            )
            .optional()
            .map_err(backend)?;
        let current = current.transpose()?;
        // The first judgement about an account is written at revision one, and
        // a caller has to say so. Letting any revision create the first record
        // would make "I read it as absent" indistinguishable from "I read a
        // record that has since been replaced".
        let next = match &current {
            Some(existing) => {
                existing
                    .revision
                    .expect("availability override", request.expected_revision)?;
                existing.revision.next()?
            }
            None => {
                AggregateRevision::INITIAL
                    .expect("availability override", request.expected_revision)?;
                AggregateRevision::INITIAL
            }
        };
        transaction
            .execute(
                "INSERT INTO availability_overrides
                     (project_id, account_profile_id, available, reason, expires_at, revision,
                      updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (project_id, account_profile_id) DO UPDATE SET
                     available = excluded.available,
                     reason = excluded.reason,
                     expires_at = excluded.expires_at,
                     revision = excluded.revision,
                     updated_at = excluded.updated_at",
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string(),
                    i64::from(request.available),
                    request.reason.as_str(),
                    request.expires_at.map(text),
                    revision_column(next)?,
                    text(request.updated_at),
                ],
            )
            .map_err(backend)?;
        let stored = transaction
            .query_row(
                &format!(
                    "SELECT {AVAILABILITY_OVERRIDE_COLUMNS} FROM availability_overrides
                     WHERE project_id = ?1 AND account_profile_id = ?2"
                ),
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string()
                ],
                |row| Ok(read_availability_override(row)),
            )
            .map_err(backend)??;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn set_provider_quota_state(
        &self,
        request: &NewProviderQuotaState,
    ) -> RepositoryResult<ProviderQuotaState> {
        // The pairing the database also enforces. Checked here too so a caller
        // gets a typed refusal naming the rule rather than a CHECK violation
        // surfacing as an opaque backend error.
        let paired = match request.state {
            ProviderQuotaKind::Exhausted => request.resets_at.is_some(),
            _ => request.resets_at.is_none(),
        };
        if !paired {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "ProviderQuotaState",
                "only an exhausted allowance carries a reset instant, and it must carry one",
            )));
        }
        // One currency, two amounts — the schema's own rule, checked here so a
        // caller gets a typed refusal naming it instead of a CHECK violation
        // arriving as an opaque backend error. Rescaling is never the answer: a
        // rate is a fact about a market at an instant, and a scheduling decision
        // taken through one would move with the market rather than the account.
        if let Some(credit) = request.credit
            && credit.remaining.currency != credit.reserve.currency
        {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "CreditBalance",
                "a balance and its reserve must be in one currency; they are never converted",
            )));
        }
        // Two windows of one kind is not a richer reading, it is two readings
        // one of which is stale. The primary key refuses the second write; this
        // refuses the ambiguous request before it becomes one.
        let mut kinds: Vec<QuotaWindowKind> = request.windows.iter().map(|w| w.kind).collect();
        kinds.sort_unstable();
        let duplicated = kinds.windows(2).any(|pair| pair[0] == pair[1]);
        if duplicated {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "ProviderQuotaState",
                "one window kind may be observed only once per account and provider",
            )));
        }
        if request.windows.iter().any(|w| w.used_percent > 100) {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "QuotaWindow",
                "a consumption share must be a percentage",
            )));
        }
        let transaction = self.begin()?;
        if read_account_profile_in(&transaction, request.project_id, request.account_profile_id)?
            .is_none()
        {
            return Err(RepositoryError::NotFound {
                subject: "account profile",
            });
        }
        let current: Option<RepositoryResult<ProviderQuotaState>> = transaction
            .query_row(
                &format!(
                    "SELECT {PROVIDER_QUOTA_STATE_COLUMNS} FROM provider_quota_states
                     WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3"
                ),
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string(),
                    request.provider.as_str(),
                ],
                |row| Ok(read_provider_quota_state(row)),
            )
            .optional()
            .map_err(backend)?;
        let current = current.transpose()?;
        // Same first-write rule as an availability override, for the same
        // reason: revision one is how "I read it as absent" is stated, and
        // accepting any revision there would make it unsayable.
        let next = match &current {
            Some(existing) => {
                existing
                    .revision
                    .expect("provider quota state", request.expected_revision)?;
                existing.revision.next()?
            }
            None => {
                AggregateRevision::INITIAL
                    .expect("provider quota state", request.expected_revision)?;
                AggregateRevision::INITIAL
            }
        };
        // Reuses `money_columns`, so a balance is stored exactly the way every
        // other monetary amount in this schema is.
        let credit = request
            .credit
            .map(|credit| -> RepositoryResult<(i64, i64, String)> {
                let (remaining, currency) = money_columns(credit.remaining)?;
                let (reserve, _) = money_columns(credit.reserve)?;
                Ok((remaining, reserve, currency))
            })
            .transpose()?;
        let credit_remaining = credit.as_ref().map(|(remaining, _, _)| *remaining);
        let credit_reserve = credit.as_ref().map(|(_, reserve, _)| *reserve);
        let credit_currency = credit.map(|(_, _, currency)| currency);
        transaction
            .execute(
                "INSERT INTO provider_quota_states
                     (project_id, account_profile_id, provider, state, resets_at, evidence_hash,
                      source, observed_at, revision, updated_at, credit_minor_units,
                      credit_reserve_minor_units, credit_currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT (project_id, account_profile_id, provider) DO UPDATE SET
                     state = excluded.state,
                     resets_at = excluded.resets_at,
                     evidence_hash = excluded.evidence_hash,
                     source = excluded.source,
                     observed_at = excluded.observed_at,
                     revision = excluded.revision,
                     updated_at = excluded.updated_at,
                     credit_minor_units = excluded.credit_minor_units,
                     credit_reserve_minor_units = excluded.credit_reserve_minor_units,
                     credit_currency = excluded.credit_currency",
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string(),
                    request.provider.as_str(),
                    request.state.as_str(),
                    request.resets_at.map(text),
                    request.evidence_hash.as_str(),
                    request.source.as_str(),
                    text(request.observed_at),
                    revision_column(next)?,
                    text(request.updated_at),
                    credit_remaining,
                    credit_reserve,
                    credit_currency,
                ],
            )
            .map_err(backend)?;
        // The window set is replaced wholesale, never merged. A collector reports
        // what a provider holds *now*; a merge would keep a window the provider
        // has stopped offering and let a scheduler route on it forever.
        transaction
            .execute(
                "DELETE FROM provider_quota_windows
                 WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3",
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string(),
                    request.provider.as_str(),
                ],
            )
            .map_err(backend)?;
        for window in &request.windows {
            transaction
                .execute(
                    "INSERT INTO provider_quota_windows
                         (project_id, account_profile_id, provider, kind, resets_at, used_percent)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        request.project_id.to_string(),
                        request.account_profile_id.to_string(),
                        request.provider.as_str(),
                        window.kind.as_str(),
                        text(window.resets_at),
                        i64::from(window.used_percent),
                    ],
                )
                .map_err(backend)?;
        }
        let mut stored = transaction
            .query_row(
                &format!(
                    "SELECT {PROVIDER_QUOTA_STATE_COLUMNS} FROM provider_quota_states
                     WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3"
                ),
                params![
                    request.project_id.to_string(),
                    request.account_profile_id.to_string(),
                    request.provider.as_str(),
                ],
                |row| Ok(read_provider_quota_state(row)),
            )
            .map_err(backend)??;
        attach_provider_quota_windows(&transaction, &mut stored)?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn list_provider_quota_states(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<ProviderQuotaState>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {PROVIDER_QUOTA_STATE_COLUMNS} FROM provider_quota_states
                 WHERE project_id = ?1 ORDER BY account_profile_id, provider"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut states = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            states.push(read_provider_quota_state(row)?);
        }
        // Attached after the header scan rather than inside it: the window read
        // needs the same connection, and SQLite will not run a second statement
        // while these rows are still being walked.
        for state in &mut states {
            attach_provider_quota_windows(&self.connection, state)?;
        }
        Ok(states)
    }

    fn list_availability_overrides(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<AvailabilityOverride>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {AVAILABILITY_OVERRIDE_COLUMNS} FROM availability_overrides
                 WHERE project_id = ?1 ORDER BY account_profile_id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut overrides = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            overrides.push(read_availability_override(row)?);
        }
        Ok(overrides)
    }
}

/// The Realm's capacity ceilings.
///
/// Inherent rather than on [`CapacityRepository`] because the configuration is
/// realm-scoped: there is no aggregate for a command receipt to name, so a
/// replay is answered through the realm binding table `0015` built for exactly
/// this class of operation.
impl SqliteStore {
    /// The stored ceilings, if an operator has set any.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn get_capacity_configuration(
        &self,
    ) -> RepositoryResult<Option<StoredCapacityConfiguration>> {
        let stored: Option<RepositoryResult<StoredCapacityConfiguration>> = self
            .connection
            .query_row(
                "SELECT ceilings, ceilings_hash, revision, updated_at FROM capacity_configuration
                 WHERE id = 1",
                [],
                |row| Ok(read_capacity_configuration(row)),
            )
            .optional()
            .map_err(backend)?;
        stored.transpose()
    }

    /// Replace the ceilings under compare-and-swap, answering a replay from
    /// what is already durable.
    ///
    /// The key is judged before the revision, and that order is the whole
    /// idempotency story: a retry of a call that already succeeded presents the
    /// revision it read *before* the write, which as a bare compare-and-swap
    /// would be stale. Recognising the key first turns that retry into the
    /// original answer instead of a conflict.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] for a key already bound to
    /// different content, and a revision conflict for a genuinely stale write.
    /// On refusal nothing is written.
    pub fn set_capacity_configuration(
        &self,
        ceilings: &CanonicalDocument,
        binding: &IdempotencyBinding,
        expected_revision: AggregateRevision,
    ) -> RepositoryResult<StoredCapacityConfiguration> {
        let transaction = self.begin()?;
        let bound: Option<(String, String)> = transaction
            .query_row(
                "SELECT operation, fingerprint FROM realm_idempotency_bindings
                 WHERE idempotency_key = ?1",
                params![binding.key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let read_current = |transaction: &Transaction<'_>| {
            let current: Option<RepositoryResult<StoredCapacityConfiguration>> = transaction
                .query_row(
                    "SELECT ceilings, ceilings_hash, revision, updated_at
                     FROM capacity_configuration WHERE id = 1",
                    [],
                    |row| Ok(read_capacity_configuration(row)),
                )
                .optional()
                .map_err(backend)?;
            current.transpose()
        };
        match bound {
            Some((operation, fingerprint))
                if operation == binding.operation
                    && fingerprint == binding.fingerprint.as_str() =>
            {
                return read_current(&transaction)?.ok_or(RepositoryError::NotFound {
                    subject: "capacity configuration",
                });
            }
            Some(_) => {
                return Err(conflict(
                    "idempotency key",
                    "this key is already bound to a different operation",
                ));
            }
            None => {}
        }

        let next = match read_current(&transaction)? {
            Some(existing) => {
                existing
                    .revision
                    .expect("capacity configuration", expected_revision)?;
                existing.revision.next()?
            }
            None => {
                AggregateRevision::INITIAL.expect("capacity configuration", expected_revision)?;
                AggregateRevision::INITIAL
            }
        };
        transaction
            .execute(
                "INSERT INTO realm_idempotency_bindings
                     (idempotency_key, operation, fingerprint, bound_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    binding.key,
                    binding.operation,
                    binding.fingerprint.as_str(),
                    text(binding.bound_at)
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO capacity_configuration (id, ceilings, ceilings_hash, revision, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET
                     ceilings = excluded.ceilings,
                     ceilings_hash = excluded.ceilings_hash,
                     revision = excluded.revision,
                     updated_at = excluded.updated_at",
                params![
                    ceilings.json(),
                    ceilings.hash().as_str(),
                    revision_column(next)?,
                    text(binding.bound_at),
                ],
            )
            .map_err(backend)?;
        let stored = read_current(&transaction)?.ok_or(RepositoryError::NotFound {
            subject: "capacity configuration",
        })?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }
}

// ---------------------------------------------------------------------------
// Specification revisions
// ---------------------------------------------------------------------------

fn install_external_workflow_spec_in_transaction(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    expected_revision: AggregateRevision,
    spec: &ExternalWorkflowSpec,
    created_at: Timestamp,
) -> RepositoryResult<(ContentHash, AggregateRevision, crate::graph::Applied)> {
    let document = spec.canonicalize()?;
    let current: i64 = transaction
        .query_row(
            "SELECT revision FROM projects WHERE id = ?1",
            params![project_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?
        .ok_or(RepositoryError::NotFound { subject: "project" })?;
    let current = revision_of(current)?;
    current.expect("project", expected_revision)?;

    let existing: Option<(String, String)> = transaction
        .query_row(
            "SELECT definition, definition_hash FROM external_workflow_specs
             WHERE project_id = ?1 AND connector = ?2 AND external_project = ?3
               AND issue_type = ?4 AND version = ?5",
            params![
                project_id.to_string(),
                spec.connector.as_str(),
                spec.project.as_str(),
                spec.issue_type.as_str(),
                version_column(spec.version)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    if let Some((json, hash)) = existing {
        let installed = stored_document::<ExternalWorkflowSpec>(&json, &hash)?;
        if installed != *spec || hash != document.hash().as_str() {
            return Err(RepositoryError::Conflict {
                subject: "external workflow specification",
                rule: "the installed immutable revision has different canonical bytes",
            });
        }
        return Ok((
            document.hash().clone(),
            current,
            crate::graph::Applied::Unchanged,
        ));
    }

    transaction
        .execute(
            "INSERT INTO external_workflow_specs
                 (project_id, connector, external_project, issue_type, version,
                  work_profile_key, work_profile_version, definition, definition_hash,
                  created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                project_id.to_string(),
                spec.connector.as_str(),
                spec.project.as_str(),
                spec.issue_type.as_str(),
                version_column(spec.version),
                spec.work_profile.as_ref().map(WorkProfileKey::as_str),
                spec.work_profile_version.map(version_column),
                document.json(),
                document.hash().as_str(),
                text(created_at)
            ],
        )
        .map_err(backend)?;
    let next = current.next()?;
    let changed = transaction
        .execute(
            "UPDATE projects SET revision = ?1 WHERE id = ?2 AND revision = ?3",
            params![
                revision_column(next)?,
                project_id.to_string(),
                revision_column(current)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "project",
            "the project revision moved during workflow installation",
        ));
    }
    Ok((
        document.hash().clone(),
        next,
        crate::graph::Applied::Created,
    ))
}

fn workflow_install_result_payload(
    intent: &CanonicalDocument,
    revision: AggregateRevision,
    applied: crate::graph::Applied,
) -> RepositoryResult<CanonicalDocument> {
    let mut payload: serde_json::Value = from_json(intent.json())?;
    let fields = payload
        .as_object_mut()
        .ok_or_else(|| DomainError::invalid("workflow installation intent", "must be an object"))?;
    fields.insert(
        "result".to_owned(),
        serde_json::json!({
            "intent_hash": intent.hash().as_str(),
            "resulting_revision": revision.get(),
            "applied": applied.as_str(),
        }),
    );
    Ok(CanonicalDocument::from_value(&payload)?)
}

fn parse_workflow_install_result(
    json: &str,
    hash: &str,
    receipt: &CommandReceipt,
) -> RepositoryResult<AggregateRevision> {
    let result: serde_json::Value = stored_document(json, hash)?;
    if result["operation"] != "install_workflow_spec"
        || result["result"]["intent_hash"].as_str() != Some(receipt.intent.hash().as_str())
    {
        return Err(RepositoryError::Conflict {
            subject: "workflow installation receipt",
            rule: "the persisted result does not belong to the recorded intent",
        });
    }
    let revision =
        result["result"]["resulting_revision"]
            .as_u64()
            .ok_or(RepositoryError::Conflict {
                subject: "workflow installation receipt",
                rule: "the persisted result has no resulting project revision",
            })?;
    Ok(AggregateRevision::parse(revision)?)
}

fn workflow_install_result_in_transaction(
    transaction: &Transaction<'_>,
    receipt: &CommandReceipt,
) -> RepositoryResult<AggregateRevision> {
    let stored: Option<(String, String)> = transaction
        .query_row(
            "SELECT payload, payload_hash FROM command_outbox
             WHERE project_id = ?1 AND receipt_id = ?2",
            params![receipt.project_id.to_string(), receipt.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (json, hash) = stored.ok_or(RepositoryError::NotFound {
        subject: "workflow installation result",
    })?;
    parse_workflow_install_result(&json, &hash, receipt)
}

/// The durable public answer produced by one task lifecycle command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskTransitionResult {
    pub task_id: TaskId,
    pub state: TaskState,
    pub revision: AggregateRevision,
}

fn task_transition_result_payload(
    intent: &CanonicalDocument,
    task: &Task,
) -> RepositoryResult<CanonicalDocument> {
    let mut payload: serde_json::Value = from_json(intent.json())?;
    let fields = payload
        .as_object_mut()
        .ok_or_else(|| DomainError::invalid("task lifecycle intent", "must be an object"))?;
    fields.insert(
        "result".to_owned(),
        serde_json::json!({
            "intent_hash": intent.hash().as_str(),
            "task_id": task.id.to_string(),
            "state": task.state.as_str(),
            "resulting_revision": task.revision.get(),
        }),
    );
    Ok(CanonicalDocument::from_value(&payload)?)
}

fn parse_task_transition_result(
    json: &str,
    hash: &str,
    receipt: &CommandReceipt,
) -> RepositoryResult<TaskTransitionResult> {
    let result: serde_json::Value = stored_document(json, hash)?;
    if result["operation"] != "lifecycle"
        || result["result"]["intent_hash"].as_str() != Some(receipt.intent.hash().as_str())
    {
        return Err(RepositoryError::Conflict {
            subject: "task lifecycle receipt",
            rule: "the persisted result does not belong to the recorded intent",
        });
    }
    let task_id = result["result"]["task_id"]
        .as_str()
        .ok_or(RepositoryError::Conflict {
            subject: "task lifecycle receipt",
            rule: "the persisted result has no task identity",
        })
        .and_then(|value| TaskId::parse(value).map_err(Into::into))?;
    if receipt.target != (AggregateRef::Task { task_id }) {
        return Err(RepositoryError::Conflict {
            subject: "task lifecycle receipt",
            rule: "the persisted result does not name the receipt target",
        });
    }
    let state = result["result"]["state"]
        .as_str()
        .ok_or(RepositoryError::Conflict {
            subject: "task lifecycle receipt",
            rule: "the persisted result has no task state",
        })
        .and_then(|value| TaskState::parse(value).map_err(Into::into))?;
    let revision =
        result["result"]["resulting_revision"]
            .as_u64()
            .ok_or(RepositoryError::Conflict {
                subject: "task lifecycle receipt",
                rule: "the persisted result has no resulting task revision",
            })?;
    Ok(TaskTransitionResult {
        task_id,
        state,
        revision: AggregateRevision::parse(revision)?,
    })
}

fn task_transition_result_in_transaction(
    transaction: &Transaction<'_>,
    receipt: &CommandReceipt,
) -> RepositoryResult<TaskTransitionResult> {
    let stored: Option<(String, String)> = transaction
        .query_row(
            "SELECT payload, payload_hash FROM command_outbox
             WHERE project_id = ?1 AND receipt_id = ?2",
            params![receipt.project_id.to_string(), receipt.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (json, hash) = stored.ok_or(RepositoryError::NotFound {
        subject: "task lifecycle result",
    })?;
    parse_task_transition_result(&json, &hash, receipt)
}

impl SqliteStore {
    /// Pin one immutable external-workflow revision to a project.
    ///
    /// The specification row and project revision move in one transaction. An
    /// already-installed byte-identical revision is an unchanged result; the
    /// same selector with different canonical bytes is an immutable conflict.
    ///
    /// # Errors
    /// Returns the domain revision conflict for stale project state, and a
    /// repository conflict when an installed selector names different bytes.
    pub fn install_external_workflow_spec(
        &self,
        project_id: ProjectId,
        expected_revision: AggregateRevision,
        spec: &ExternalWorkflowSpec,
    ) -> RepositoryResult<(ContentHash, AggregateRevision, crate::graph::Applied)> {
        let transaction = self.begin()?;
        let installed = install_external_workflow_spec_in_transaction(
            &transaction,
            project_id,
            expected_revision,
            spec,
            Timestamp::now(),
        )?;
        transaction.commit().map_err(backend)?;
        Ok(installed)
    }

    /// Install one workflow revision and record its Admin authority atomically.
    ///
    /// The command payload retains the original resulting project revision, so
    /// a later replay never substitutes whatever revision the project has now.
    pub fn install_external_workflow_spec_with_intent(
        &self,
        project_id: ProjectId,
        expected_revision: AggregateRevision,
        spec: &ExternalWorkflowSpec,
        envelope: &ReceiptEnvelope<NewCommandIntent>,
    ) -> RepositoryResult<(
        ContentHash,
        AggregateRevision,
        crate::graph::Applied,
        CommandReceipt,
    )> {
        let intent = envelope.peek(self.realm_id())?;
        let target = AggregateRef::Project { project_id };
        ensure_atomic_intent_matches(
            intent,
            project_id,
            CommandKind::InstallWorkflowSpec,
            &target,
            expected_revision,
        )?;
        let transaction = self.begin()?;
        if let Some(existing) = command_receipt_by_key(&transaction, &intent.idempotency_key)? {
            ensure_atomic_replay(&existing, intent)?;
            let revision = workflow_install_result_in_transaction(&transaction, &existing)?;
            let hash = spec.canonicalize()?.hash().clone();
            return Ok((hash, revision, crate::graph::Applied::Unchanged, existing));
        }

        let (hash, revision, applied) = install_external_workflow_spec_in_transaction(
            &transaction,
            project_id,
            expected_revision,
            spec,
            intent.created_at,
        )?;
        let mut recorded = intent.clone();
        recorded.payload = workflow_install_result_payload(&intent.intent, revision, applied)?;
        let replayed = crate::commands::intent::insert_intent(&transaction, &recorded)?;
        if replayed.is_some() {
            return Err(conflict(
                "command receipt",
                "the idempotency key appeared during one atomic workflow installation",
            ));
        }
        let receipt = command_receipt_by_key(&transaction, &intent.idempotency_key)?.ok_or(
            RepositoryError::NotFound {
                subject: "command receipt",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok((hash, revision, applied, receipt))
    }

    /// Read the original resulting revision retained by a workflow-install receipt.
    pub fn workflow_install_result_revision(
        &self,
        receipt: &CommandReceipt,
    ) -> RepositoryResult<AggregateRevision> {
        let stored: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT payload, payload_hash FROM command_outbox
                 WHERE project_id = ?1 AND receipt_id = ?2",
                params![receipt.project_id.to_string(), receipt.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let (json, hash) = stored.ok_or(RepositoryError::NotFound {
            subject: "workflow installation result",
        })?;
        parse_workflow_install_result(&json, &hash, receipt)
    }

    /// Read the original state and revision retained by a task lifecycle receipt.
    pub fn task_transition_result(
        &self,
        receipt: &CommandReceipt,
    ) -> RepositoryResult<TaskTransitionResult> {
        let stored: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT payload, payload_hash FROM command_outbox
                 WHERE project_id = ?1 AND receipt_id = ?2",
                params![receipt.project_id.to_string(), receipt.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let (json, hash) = stored.ok_or(RepositoryError::NotFound {
            subject: "task lifecycle result",
        })?;
        parse_task_transition_result(&json, &hash, receipt)
    }
}

impl SpecRepository for SqliteStore {
    fn insert_work_profile(
        &self,
        project_id: ProjectId,
        spec: &WorkProfileSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO work_profiles
                     (project_id, profile_key, version, definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project_id.to_string(),
                    spec.id.as_str(),
                    version_column(spec.version),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_work_profile(
        &self,
        project_id: ProjectId,
        id: &WorkProfileKey,
        version: SpecVersion,
    ) -> RepositoryResult<Option<WorkProfileSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM work_profiles
                 WHERE project_id = ?1 AND profile_key = ?2 AND version = ?3",
                params![project_id.to_string(), id.as_str(), version_column(version)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<WorkProfileSpec>(&json, &hash))
            .transpose()
    }

    fn insert_team_template(
        &self,
        project_id: ProjectId,
        revision: &TeamTemplateRevision,
    ) -> RepositoryResult<ContentHash> {
        let authority = to_json(&revision.role_authority)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO team_templates
                     (project_id, template_id, version, name, definition, definition_hash,
                      role_authority, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id.to_string(),
                    revision.template_id.to_string(),
                    version_column(revision.version),
                    revision.name.as_str(),
                    revision.definition.json(),
                    revision.definition.hash().as_str(),
                    authority,
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(revision.definition.hash().clone())
    }

    fn get_team_template(
        &self,
        project_id: ProjectId,
        id: TeamTemplateId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TeamTemplateRevision>> {
        let found: Option<(String, String, String, String)> = self
            .connection
            .query_row(
                "SELECT name, definition, definition_hash, role_authority FROM team_templates
                 WHERE project_id = ?1 AND template_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((name, definition, hash, authority)) = found else {
            return Ok(None);
        };
        let digest = ContentHash::parse(&hash)?;
        Ok(Some(TeamTemplateRevision {
            template_id: id,
            version,
            name: ExternalName::parse(&name)?,
            definition: CanonicalDocument::from_stored(&definition, &digest)?,
            role_authority: from_json(&authority)?,
        }))
    }

    fn insert_persona_scenario(
        &self,
        project_id: ProjectId,
        spec: &PersonaScenarioSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO persona_scenarios
                     (project_id, scenario_id, version, persona_key, gate_key, definition,
                      definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id.to_string(),
                    spec.scenario_id.to_string(),
                    version_column(spec.version),
                    spec.persona.as_str(),
                    spec.gate_under_test.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_persona_scenario(
        &self,
        project_id: ProjectId,
        id: PersonaScenarioId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<PersonaScenarioSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM persona_scenarios
                 WHERE project_id = ?1 AND scenario_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<PersonaScenarioSpec>(&json, &hash))
            .transpose()
    }

    fn insert_trigger_spec(
        &self,
        project_id: ProjectId,
        spec: &TriggerSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO trigger_specs
                     (project_id, trigger_key, version, source_kind, source_connection,
                      work_profile_key, work_profile_version, team_template_id,
                      team_template_version, context_template, context_version,
                      calendar_profile_id, calendar_version,
                      definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    project_id.to_string(),
                    spec.id.as_str(),
                    version_column(spec.version),
                    spec.source_kind.as_str(),
                    spec.source_connection.as_str(),
                    spec.work_profile.as_str(),
                    version_column(spec.work_profile_version),
                    spec.team_template.template_id.to_string(),
                    version_column(spec.team_template.version),
                    spec.context_template.template.as_str(),
                    version_column(spec.context_template.version),
                    spec.calendar_policy
                        .as_ref()
                        .map(|policy| policy.profile_id.to_string()),
                    spec.calendar_policy
                        .as_ref()
                        .map(|policy| version_column(policy.version)),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_trigger_spec(
        &self,
        project_id: ProjectId,
        id: &TriggerKey,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TriggerSpec>> {
        type TriggerRow = (
            String,
            String,
            String,
            i64,
            String,
            i64,
            String,
            i64,
            Option<String>,
            Option<i64>,
        );
        let found: Option<TriggerRow> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash, work_profile_key, work_profile_version,
                        team_template_id, team_template_version, context_template,
                        context_version, calendar_profile_id, calendar_version
                 FROM trigger_specs
                 WHERE project_id = ?1 AND trigger_key = ?2 AND version = ?3",
                params![project_id.to_string(), id.as_str(), version_column(version)],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|row| {
                let spec = stored_document::<TriggerSpec>(&row.0, &row.1)?;
                // The canonical definition is authoritative, but the normalized
                // columns are what the foreign keys act on. If the two ever
                // disagreed, the pins SQLite enforced would not be the pins the
                // domain believes in — so a disagreement is a hard read failure
                // rather than a silent preference for one side.
                let agrees = spec.work_profile.as_str() == row.2
                    && version_column(spec.work_profile_version) == row.3
                    && spec.team_template.template_id.to_string() == row.4
                    && version_column(spec.team_template.version) == row.5
                    && spec.context_template.template.as_str() == row.6
                    && version_column(spec.context_template.version) == row.7
                    && spec
                        .calendar_policy
                        .as_ref()
                        .map(|policy| policy.profile_id.to_string())
                        == row.8
                    && spec
                        .calendar_policy
                        .as_ref()
                        .map(|policy| version_column(policy.version))
                        == row.9;
                if !agrees {
                    return Err(RepositoryError::from(DomainError::invalid(
                        "TriggerSpec",
                        "the stored pins disagree with the canonical definition",
                    )));
                }
                Ok(spec)
            })
            .transpose()
    }

    fn insert_calendar_profile(&self, spec: &CalendarProfileSpec) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO calendar_profiles
                     (profile_id, version, name, definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    spec.profile_id.to_string(),
                    version_column(spec.version),
                    spec.name.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_calendar_profile(
        &self,
        id: CalendarProfileId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<CalendarProfileSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM calendar_profiles
                 WHERE profile_id = ?1 AND version = ?2",
                params![id.to_string(), version_column(version)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<CalendarProfileSpec>(&json, &hash))
            .transpose()
    }

    fn insert_ticket_field_spec(
        &self,
        project_id: ProjectId,
        spec: &TicketFieldSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO ticket_field_specs
                     (project_id, connector, external_project, issue_type, version,
                      definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id.to_string(),
                    spec.connector.as_str(),
                    spec.project.as_str(),
                    spec.issue_type.as_str(),
                    version_column(spec.version),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_ticket_field_spec(
        &self,
        selector: &ConnectorSpecSelector,
    ) -> RepositoryResult<Option<TicketFieldSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM ticket_field_specs
                 WHERE project_id = ?1 AND connector = ?2 AND external_project = ?3
                   AND issue_type = ?4 AND version = ?5",
                params![
                    selector.project_id.to_string(),
                    selector.connector.as_str(),
                    selector.project.as_str(),
                    selector.issue_type.as_str(),
                    version_column(selector.version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<TicketFieldSpec>(&json, &hash))
            .transpose()
    }

    fn insert_external_workflow_spec(
        &self,
        project_id: ProjectId,
        spec: &ExternalWorkflowSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO external_workflow_specs
                     (project_id, connector, external_project, issue_type, version,
                      work_profile_key, work_profile_version, definition, definition_hash,
                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    project_id.to_string(),
                    spec.connector.as_str(),
                    spec.project.as_str(),
                    spec.issue_type.as_str(),
                    version_column(spec.version),
                    spec.work_profile.as_ref().map(WorkProfileKey::as_str),
                    spec.work_profile_version.map(version_column),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_external_workflow_spec(
        &self,
        selector: &ConnectorSpecSelector,
    ) -> RepositoryResult<Option<ExternalWorkflowSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM external_workflow_specs
                 WHERE project_id = ?1 AND connector = ?2 AND external_project = ?3
                   AND issue_type = ?4 AND version = ?5",
                params![
                    selector.project_id.to_string(),
                    selector.connector.as_str(),
                    selector.project.as_str(),
                    selector.issue_type.as_str(),
                    version_column(selector.version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<ExternalWorkflowSpec>(&json, &hash))
            .transpose()
    }
}

// ---------------------------------------------------------------------------
// Workflows, gates and the task lifecycle
// ---------------------------------------------------------------------------

pub(crate) fn load_workflow(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    workflow_id: TaskWorkflowId,
) -> RepositoryResult<(TaskWorkflow, AggregateRevision)> {
    let row: Option<(String, String, String, i64, String, i64, String)> = transaction
        .query_row(
            "SELECT task_id, snapshot, snapshot_hash, active, current_phase, revision, created_at
             FROM task_workflows WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), workflow_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((task, snapshot, hash, active, phase, revision, created_at)) = row else {
        return Err(RepositoryError::NotFound {
            subject: "task workflow",
        });
    };
    let snapshot: ResolvedWorkProfileSnapshot = stored_document(&snapshot, &hash)?;
    snapshot.verify()?;
    let revision = revision_of(revision)?;
    Ok((
        TaskWorkflow {
            id: workflow_id,
            project_id,
            task_id: TaskId::parse(&task)?,
            snapshot,
            current_phase: PhaseKey::parse(&phase)?,
            active: active == 1,
            revision,
            created_at: read_timestamp(&created_at)?,
        },
        revision,
    ))
}

/// How long a seat has to attach before its silence becomes a recorded finding
/// rather than an open wait (OP-REQ-039b).
const SEAT_ATTACH_GRACE: SignedDuration = SignedDuration::from_mins(10);

/// How long a seat may go without observed activity before it is concluded
/// stalled rather than working (OP-REQ-039c).
const SEAT_MAX_IDLE: SignedDuration = SignedDuration::from_mins(30);

/// One `agent_runs` row exactly as [`read_seat_attachments`] selects it:
/// lifecycle, observed state, last confirmed instant, creation and closure.
type SeatAttachmentRow = (String, String, Option<String>, String, Option<String>);

/// Conclude each seat of one team run from persisted OP-REQ-039 evidence.
///
/// The evidence lives on the logical [`SeatBinding`], and every input is read
/// rather than derived: the deadline was fixed when the seat was created, the
/// activity instant was written by an observed runtime event, and orphanhood
/// comes from the owning epic seat's own Kontor lifecycle. Deriving any of the
/// three at read time is what let a seat that never attached stay
/// indistinguishable from one that was merely slow.
///
/// A team run with no seat bindings falls through to
/// [`read_legacy_seat_attachments`]. That is the pre-OP-02 world — nothing
/// creates seat bindings on the production path until the admission work in
/// checkpoint 4 — and refusing to conclude anything there would remove the
/// phantom-seat guard rather than improve it.
fn read_seat_attachments(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    team_run_id: &str,
    now: Timestamp,
) -> RepositoryResult<Vec<SeatAttachment>> {
    let mut statement = transaction
        .prepare(&format!(
            "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
             WHERE project_id = ?1 AND team_run_id = ?2 ORDER BY created_at, id"
        ))
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), team_run_id])
        .map_err(backend)?;
    let mut bindings = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        bindings.push(read_seat_binding(row)?);
    }
    drop(rows);
    drop(statement);

    if bindings.is_empty() {
        return read_legacy_seat_attachments(transaction, project_id, team_run_id, now);
    }
    conclude_seat_attachments(transaction, project_id, &bindings, now)
}

/// Conclude a set of seats from their persisted evidence and their owners'.
///
/// The one place the OP-REQ-039 join lives, so the team-run reader above and the
/// node-keyed read below cannot drift into two different answers about the same
/// seat.
fn conclude_seat_attachments(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    bindings: &[SeatBinding],
    now: Timestamp,
) -> RepositoryResult<Vec<SeatAttachment>> {
    let mut attachments = Vec::with_capacity(bindings.len());
    for binding in bindings {
        // Orphanhood is a fact about the *owner*, so it is read from the owner's
        // row. A seat with no parent is a root rather than an orphan.
        let parent_closed = match binding.parent_seat_binding_id {
            Some(parent_id) => transaction
                .query_row(
                    &format!(
                        "SELECT {SEAT_BINDING_COLUMNS} FROM seat_bindings
                         WHERE project_id = ?1 AND id = ?2"
                    ),
                    params![project_id.to_string(), parent_id.to_string()],
                    |row| Ok(read_seat_binding(row)),
                )
                .optional()
                .map_err(backend)?
                .transpose()?
                // A parent that is not there at all is gone, and a seat whose
                // owner cannot be found is steered by nobody. Reading a missing
                // owner as "still open" is the assumption that keeps orphans
                // counted as capacity.
                .is_none_or(|parent| parent.closes_children()),
            None => false,
        };
        attachments.push(evaluate_seat_attachment(
            &binding.attachment_observation(parent_closed),
            now,
            SEAT_MAX_IDLE,
        ));
    }
    Ok(attachments)
}

/// Conclude each seat of one team run from its `agent_runs` row.
///
/// The pre-OP-02 path, kept only for team runs that have no seat bindings yet.
/// Its three known weaknesses are exactly what OP-REQ-039 names: the deadline is
/// derived from `created_at` at read time, a generic confirmation stands in for
/// observed activity, and an orphan cannot be seen at all. Checkpoint 4 retires
/// this function by giving every production seat a binding.
fn read_legacy_seat_attachments(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    team_run_id: &str,
    now: Timestamp,
) -> RepositoryResult<Vec<SeatAttachment>> {
    let mut statement = transaction
        .prepare(
            "SELECT lifecycle, observed_state, last_confirmed_at, created_at, closed_at
             FROM agent_runs WHERE project_id = ?1 AND team_run_id = ?2",
        )
        .map_err(backend)?;
    let rows: Vec<SeatAttachmentRow> = statement
        .query_map(params![project_id.to_string(), team_run_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?;
    drop(statement);

    rows.into_iter()
        .map(|(lifecycle, observed, last_confirmed, created, closed)| {
            let lifecycle = RunLifecycle::parse(&lifecycle)?;
            let observed = ObservedRunState::parse(&observed)?;
            let created_at = read_timestamp(&created)?;
            let last_confirmed_at = last_confirmed.as_deref().map(read_timestamp).transpose()?;
            // A confirmation only evidences attachment once the run was
            // actually dispatched. A queued seat has been accepted, not
            // started, however recently it was confirmed.
            let last_attached_at = if lifecycle.is_dispatched() {
                last_confirmed_at
            } else {
                None
            };
            let attach_deadline = created_at.checked_add(SEAT_ATTACH_GRACE).map_err(|_| {
                DomainError::invalid("seat attach deadline", "overflows the timestamp range")
            })?;
            Ok(evaluate_seat_attachment(
                &SeatAttachmentObservation {
                    attach_deadline,
                    last_attached_at,
                    last_activity_at: last_confirmed_at,
                    // OP-02 derives this from the owning epic seat's lifecycle.
                    // Until it does, this path never *concludes* an orphan — it
                    // simply cannot see one, which is honest rather than wrong.
                    parent_closed: false,
                    released: closed.is_some(),
                    runtime_reported: observed,
                },
                now,
                SEAT_MAX_IDLE,
            ))
        })
        .collect()
}

/// Certify a task's claim of progress from persisted rows only.
fn certify_task_progress_from_store(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    now: Timestamp,
) -> RepositoryResult<TaskProgressEvidence> {
    let mut statement = transaction
        .prepare(
            "SELECT id, lifecycle FROM team_runs
             WHERE project_id = ?1 AND task_id = ?2 AND closed_at IS NULL",
        )
        .map_err(backend)?;
    let runs: Vec<(String, String)> = statement
        .query_map(
            params![project_id.to_string(), task_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?;
    drop(statement);

    // A task that has never had a team run is being worked without
    // orchestration — a profile that pins no team has no seats to attach, so
    // there is no attachment evidence to demand and none to fake. The rule
    // bites where the incident happened: a task that *does* have a run, none of
    // whose seats can hold it.
    if runs.is_empty() {
        return Ok(certify_task_progress(
            RunLifecycle::Running,
            &[SeatAttachment::Attached],
        )?);
    }

    for (team_run_id, lifecycle) in runs {
        let lifecycle = RunLifecycle::parse(&lifecycle)?;
        let seats = read_seat_attachments(transaction, project_id, &team_run_id, now)?;
        if let Ok(evidence) = certify_task_progress(lifecycle, &seats) {
            return Ok(evidence);
        }
    }
    Err(DomainError::MissingEvidence {
        subject: "task progress",
        rule: "every open team run of this task has lost all of its seats",
    }
    .into())
}

fn reduce_gate_states(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    workflow_id: TaskWorkflowId,
) -> RepositoryResult<BTreeMap<GateKey, GateState>> {
    let mut statement = transaction
        .prepare(
            "SELECT gate_key, verdict FROM task_gate_evaluations
             WHERE project_id = ?1 AND workflow_id = ?2 ORDER BY gate_key, sequence",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), workflow_id.to_string()])
        .map_err(backend)?;
    let mut states = BTreeMap::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let gate = GateKey::parse(&row.get::<_, String>(0).map_err(backend)?)?;
        let verdict = GateVerdict::parse(&row.get::<_, String>(1).map_err(backend)?)?;
        states.insert(gate, verdict.resulting_state());
    }
    Ok(states)
}

fn transition_task_in_transaction(
    transaction: &Transaction<'_>,
    request: &TaskTransitionRequest,
) -> RepositoryResult<Task> {
    crate::authority::require_backlog_authority(transaction, request.project_id)?;
    let row: Option<(String, i64)> = transaction
        .query_row(
            "SELECT state, revision FROM tasks WHERE project_id = ?1 AND id = ?2",
            params![request.project_id.to_string(), request.task_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((state, revision)) = row else {
        return Err(RepositoryError::NotFound { subject: "task" });
    };
    let current = TaskState::parse(&state)?;
    let revision = revision_of(revision)?;
    revision.expect("task", request.expected_revision)?;

    if request.to == TaskState::Withdrawn {
        let runs: i64 = transaction
            .query_row(
                "SELECT count(*) FROM team_runs WHERE project_id = ?1 AND task_id = ?2",
                params![request.project_id.to_string(), request.task_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if runs != 0 {
            return Err(conflict(
                "task withdrawal",
                "a task with TeamRun history is not never-started",
            ));
        }
        let mut statement = transaction
            .prepare(
                "SELECT dependent.state
                 FROM task_dependencies dependency
                 JOIN tasks dependent
                   ON dependent.project_id = dependency.project_id
                  AND dependent.id = dependency.task_id
                 WHERE dependency.project_id = ?1
                   AND dependency.depends_on_task_id = ?2",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                request.project_id.to_string(),
                request.task_id.to_string()
            ])
            .map_err(backend)?;
        while let Some(row) = rows.next().map_err(backend)? {
            let dependent = TaskState::parse(&row.get::<_, String>(0).map_err(backend)?)?;
            if !dependent.is_terminal() {
                return Err(conflict(
                    "task withdrawal",
                    "an unresolved dependent task still requires it",
                ));
            }
        }
    }

    // A task closing with an execution outcome answers for two independent
    // things: its pinned profile's phases, gates and artifacts, and its team's
    // role slots. Withdrawal instead proves here that no TeamRun ever existed.
    let mut certificate = None;
    if request.to.is_terminal() && request.to != TaskState::Withdrawn {
        let workflow_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM task_workflows
                 WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                params![request.project_id.to_string(), request.task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;

        let pinned = match workflow_id {
            Some(id) => {
                let workflow_id = TaskWorkflowId::parse(&id)?;
                let (workflow, _) = load_workflow(transaction, request.project_id, workflow_id)?;
                Some((workflow_id, workflow))
            }
            None => None,
        };

        ensure_team_accounted_for(
            transaction,
            request,
            pinned
                .as_ref()
                .is_some_and(|(_, workflow)| workflow.snapshot.definition.team_template.is_some()),
        )?;

        if request.to == TaskState::Done {
            let (workflow_id, workflow) = pinned.ok_or(DomainError::MissingEvidence {
                subject: "task completion",
                rule: "a task without an active workflow has no closure evidence",
            })?;
            let states = reduce_gate_states(transaction, request.project_id, workflow_id)?;
            certificate = Some(workflow.snapshot.certify_closure(
                &request.completed_phases,
                &states,
                &request.produced_artifacts,
            )?);
        }
    }

    let mut progress = None;
    if request.to == TaskState::InProgress {
        progress = Some(certify_task_progress_from_store(
            transaction,
            request.project_id,
            request.task_id,
            request.occurred_at,
        )?);
    }

    let transition = TaskTransition {
        to: request.to,
        resume_receipt: request.resume_receipt,
        reopen: match (request.reopen, request.resume_receipt) {
            (true, Some(receipt)) => Some(TaskReopenAuthority::granted_by(receipt)),
            (true, None) => {
                return Err(DomainError::MissingAuthority {
                    subject: "task reopen",
                    rule: "reopening a terminal task requires a command receipt",
                }
                .into());
            }
            (false, _) => None,
        },
        run_outcome: request.run_outcome,
        closure: certificate.as_ref(),
        progress: progress.as_ref(),
    };
    let next_state = kontor_core::state::apply_task_transition(current, &transition)?;
    let next_revision = revision.next()?;
    let changed = transaction
        .execute(
            "UPDATE tasks SET state = ?1, imported_state = NULL, revision = ?2, updated_at = ?3
             WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
            params![
                next_state.as_str(),
                revision_column(next_revision)?,
                text(request.occurred_at),
                request.project_id.to_string(),
                request.task_id.to_string(),
                revision_column(revision)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict("task", "the task revision moved during the write"));
    }
    transaction
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1 AND id = ?2"),
            params![request.project_id.to_string(), request.task_id.to_string()],
            |row| Ok(read_task(row)),
        )
        .optional()
        .map_err(backend)?
        .transpose()?
        .ok_or(RepositoryError::NotFound { subject: "task" })
}

fn command_receipt_by_key(
    transaction: &Transaction<'_>,
    key: &IdempotencyKey,
) -> RepositoryResult<Option<CommandReceipt>> {
    transaction
        .query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM command_receipts WHERE idempotency_key = ?1"),
            params![key.as_str()],
            |row| Ok(crate::commands::receipts::read_receipt_row(row)),
        )
        .optional()
        .map_err(backend)?
        .transpose()
}

fn ensure_atomic_replay(
    existing: &CommandReceipt,
    request: &NewCommandIntent,
) -> RepositoryResult<()> {
    if existing.project_id != request.project_id {
        return Err(RepositoryError::CrossProject {
            subject: "command receipt",
        });
    }
    existing.ensure_replay(&request.target, &request.intent)?;
    if existing.kind != request.kind || existing.target_revision != request.target_revision {
        return Err(DomainError::invalid(
            "CommandReceipt",
            "idempotency key reused for a different command or target revision",
        )
        .into());
    }
    Ok(())
}

fn ensure_atomic_intent_matches(
    request: &NewCommandIntent,
    project_id: ProjectId,
    kind: CommandKind,
    target: &AggregateRef,
    target_revision: AggregateRevision,
) -> RepositoryResult<()> {
    if request.project_id != project_id
        || request.kind != kind
        || &request.target != target
        || request.target_revision != target_revision
    {
        return Err(DomainError::invalid(
            "CommandReceipt",
            "the atomic command authority does not match the operation it accompanies",
        )
        .into());
    }
    Ok(())
}

impl WorkflowRepository for SqliteStore {
    fn create_task_workflow(&self, request: &NewTaskWorkflow) -> RepositoryResult<TaskWorkflow> {
        request.snapshot.verify()?;
        if !request
            .snapshot
            .definition
            .phases
            .iter()
            .any(|phase| phase.id == request.current_phase)
        {
            return Err(DomainError::invalid(
                "task workflow",
                "the starting phase is not declared by the pinned profile",
            )
            .into());
        }
        let document = CanonicalDocument::from_serializable(&request.snapshot)?;
        let transaction = self.begin()?;
        // The snapshot is self-consistent by construction, which says nothing
        // about whether the revision it pins was ever stored. Prove it here, in
        // the same transaction, so the failure is a domain error rather than a
        // bare foreign-key violation — and so the digest is checked too, which
        // no foreign key can do.
        let pinned: Option<String> = transaction
            .query_row(
                "SELECT definition_hash FROM work_profiles
                 WHERE project_id = ?1 AND profile_key = ?2 AND version = ?3",
                params![
                    request.project_id.to_string(),
                    request.snapshot.definition.id.as_str(),
                    version_column(request.snapshot.definition.version)
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(pinned) = pinned else {
            return Err(DomainError::MissingEvidence {
                subject: "task workflow",
                rule: "the pinned work-profile revision is not stored in this project",
            }
            .into());
        };
        if ContentHash::parse(&pinned)? != request.snapshot.definition_hash {
            return Err(DomainError::MissingEvidence {
                subject: "task workflow",
                rule: "the pinned work-profile revision has a different canonical digest",
            }
            .into());
        }
        transaction
            .execute(
                "INSERT INTO task_workflows
                     (id, project_id, task_id, profile_key, profile_version, snapshot,
                      snapshot_hash, current_phase, active, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1, ?9)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.snapshot.definition.id.as_str(),
                    version_column(request.snapshot.definition.version),
                    document.json(),
                    document.hash().as_str(),
                    request.current_phase.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(TaskWorkflow {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            snapshot: request.snapshot.clone(),
            current_phase: request.current_phase.clone(),
            active: true,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn get_active_task_workflow(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<TaskWorkflow>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM task_workflows
                 WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(id) = found else {
            return Ok(None);
        };
        let transaction = self.begin()?;
        let (workflow, _) = load_workflow(&transaction, project_id, TaskWorkflowId::parse(&id)?)?;
        Ok(Some(workflow))
    }

    fn advance_phase(&self, request: &PhaseAdvance) -> RepositoryResult<AggregateRevision> {
        let transaction = self.begin()?;
        let (workflow, revision) =
            load_workflow(&transaction, request.project_id, request.workflow_id)?;
        revision.expect("task workflow", request.expected_revision)?;
        let declared = workflow
            .snapshot
            .definition
            .edges
            .iter()
            .any(|edge| edge.from == workflow.current_phase && edge.to == request.next_phase);
        if !declared {
            return Err(DomainError::invalid(
                "phase advance",
                "the pinned profile declares no edge between these phases",
            )
            .into());
        }
        let next = revision.next()?;
        transaction
            .execute(
                "UPDATE task_workflows SET current_phase = ?1, revision = ?2
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    request.next_phase.as_str(),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(next)
    }

    fn append_gate_evaluation(&self, request: &NewGateEvaluation) -> RepositoryResult<u32> {
        let transaction = self.begin()?;
        let (workflow, _) = load_workflow(&transaction, request.project_id, request.workflow_id)?;
        let gate = workflow
            .snapshot
            .definition
            .gate(&request.gate)
            .ok_or(RepositoryError::NotFound { subject: "gate" })?;

        let authorized = if request.verdict == GateVerdict::Waived {
            gate.waiver_allowed && gate.waiver_roles.contains(&request.evaluator_role)
        } else {
            gate.evaluator_roles.contains(&request.evaluator_role)
        };
        if !authorized {
            return Err(DomainError::MissingAuthority {
                subject: "gate evaluation",
                rule: "the acting role is not an authority for this gate",
            }
            .into());
        }
        if request.verdict.requires_evidence() {
            if request.evidence.is_empty() {
                return Err(DomainError::MissingEvidence {
                    subject: "gate evaluation",
                    rule: "passing or waiving a gate requires evidence",
                }
                .into());
            }
            let provided: BTreeSet<&ArtifactKey> = request.evidence.iter().collect();
            if !gate
                .required_evidence
                .iter()
                .all(|required| provided.contains(required))
            {
                return Err(DomainError::MissingEvidence {
                    subject: "gate evaluation",
                    rule: "the evidence required by the pinned profile is incomplete",
                }
                .into());
            }
        }

        let previous: Option<i64> = transaction
            .query_row(
                "SELECT MAX(sequence) FROM task_gate_evaluations
                 WHERE project_id = ?1 AND workflow_id = ?2 AND gate_key = ?3",
                params![
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    request.gate.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?
            .flatten();
        let sequence = previous.unwrap_or(0) + 1;
        let evidence = to_json(&request.evidence)?;
        let session_run = request
            .session_evidence
            .as_ref()
            .map(|citation| citation.agent_run_id.to_string());
        let session_digest = request
            .session_evidence
            .as_ref()
            .map(|citation| citation.digest.as_str().to_owned());
        transaction
            .execute(
                "INSERT INTO task_gate_evaluations
                     (project_id, workflow_id, gate_key, sequence, verdict, evaluator_role,
                      evaluator_account, evidence, recorded_at, agent_run_id, reviewer_principal,
                      policy_evaluation_id, session_evidence_agent_run, session_evidence_digest)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    request.gate.as_str(),
                    sequence,
                    request.verdict.as_str(),
                    request.evaluator_role.as_str(),
                    request.evaluator_account.to_string(),
                    evidence,
                    text(request.recorded_at),
                    request.agent_run_id.map(|run| run.to_string()),
                    request.reviewer_principal.as_ref().map(ExternalId::as_str),
                    request
                        .policy_evaluation_id
                        .map(|evaluation| evaluation.to_string()),
                    session_run,
                    session_digest,
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        u32::try_from(sequence).map_err(|_| RepositoryError::Backend {
            detail: "gate evaluation sequence exceeded its range".to_owned(),
        })
    }

    fn gate_states(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
    ) -> RepositoryResult<BTreeMap<GateKey, GateState>> {
        let transaction = self.begin()?;
        reduce_gate_states(&transaction, project_id, workflow_id)
    }

    fn list_gate_evaluations(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
    ) -> RepositoryResult<Vec<GateEvaluation>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT gate_key, sequence, verdict, evaluator_role, evaluator_account,
                        evidence, recorded_at, agent_run_id, reviewer_principal,
                        policy_evaluation_id, session_evidence_agent_run, session_evidence_digest
                 FROM task_gate_evaluations
                 WHERE project_id = ?1 AND workflow_id = ?2
                 ORDER BY recorded_at, gate_key, sequence",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), workflow_id.to_string()])
            .map_err(backend)?;
        let mut evaluations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let sequence: i64 = row.get(1).map_err(backend)?;
            let agent_run: Option<String> = row.get(7).map_err(backend)?;
            let principal: Option<String> = row.get(8).map_err(backend)?;
            let evaluation: Option<String> = row.get(9).map_err(backend)?;
            let session_run: Option<String> = row.get(10).map_err(backend)?;
            let session_digest: Option<String> = row.get(11).map_err(backend)?;
            let session_evidence = match (session_run, session_digest) {
                (Some(run), Some(digest)) => Some(SessionVerdictEvidence {
                    agent_run_id: AgentRunId::parse(&run)?,
                    digest: ContentHash::parse(&digest)?,
                }),
                // A row written before session evidence existed, or a row whose
                // pair the store never wrote, carries no citation. A half pair
                // cannot occur through this store, which writes the two columns
                // in one statement.
                _ => None,
            };
            evaluations.push(GateEvaluation {
                project_id,
                workflow_id,
                gate: GateKey::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                sequence: u32::try_from(sequence).unwrap_or(u32::MAX),
                verdict: GateVerdict::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                evaluator_role: RoleKey::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                evaluator_account: AccountProfileId::parse(
                    &row.get::<_, String>(4).map_err(backend)?,
                )?,
                evidence: from_json(&row.get::<_, String>(5).map_err(backend)?)?,
                agent_run_id: agent_run.as_deref().map(AgentRunId::parse).transpose()?,
                session_evidence,
                reviewer_principal: principal.as_deref().map(ExternalId::parse).transpose()?,
                policy_evaluation_id: evaluation
                    .as_deref()
                    .map(GuardrailEvaluationId::parse)
                    .transpose()?,
                recorded_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
            });
        }
        Ok(evaluations)
    }

    fn create_task_persona_snapshot(
        &self,
        request: &NewTaskPersonaSnapshot,
    ) -> RepositoryResult<PersonaScenarioSnapshot> {
        let transaction = self.begin()?;

        // The workflow must be this task's, in this project: that is what makes
        // the pinned profile the right place to resolve authority from.
        let (workflow, _) = load_workflow(&transaction, request.project_id, request.workflow_id)?;
        if workflow.task_id != request.task_id {
            return Err(RepositoryError::CrossProject {
                subject: "persona workflow",
            });
        }

        let stored: Option<(String, String)> = transaction
            .query_row(
                "SELECT definition, definition_hash FROM persona_scenarios
                 WHERE project_id = ?1 AND scenario_id = ?2 AND version = ?3",
                params![
                    request.project_id.to_string(),
                    request.scenario_id.to_string(),
                    version_column(request.version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((json, hash)) = stored else {
            return Err(RepositoryError::NotFound {
                subject: "persona scenario revision",
            });
        };
        let spec: PersonaScenarioSpec = stored_document(&json, &hash)?;

        // Authority is proved against the gate the pinned profile declares, not
        // against anything the scenario asserts about itself.
        let snapshot = PersonaScenarioSnapshot::freeze_onto_task(&spec, &workflow.snapshot)?;
        let document = CanonicalDocument::from_serializable(&snapshot)?;

        transaction
            .execute(
                "INSERT INTO task_persona_snapshots
                     (project_id, task_id, scenario_id, version, workflow_id, gate_key,
                      snapshot, snapshot_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.scenario_id.to_string(),
                    version_column(request.version),
                    request.workflow_id.to_string(),
                    spec.gate_under_test.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(snapshot)
    }

    fn get_task_persona_snapshot(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        scenario_id: PersonaScenarioId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<PersonaScenarioSnapshot>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT snapshot, snapshot_hash FROM task_persona_snapshots
                 WHERE project_id = ?1 AND task_id = ?2 AND scenario_id = ?3 AND version = ?4",
                params![
                    project_id.to_string(),
                    task_id.to_string(),
                    scenario_id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<PersonaScenarioSnapshot>(&json, &hash))
            .transpose()
    }

    fn transition_task(&self, request: &TaskTransitionRequest) -> RepositoryResult<Task> {
        let transaction = self.begin()?;
        let moved = transition_task_in_transaction(&transaction, request)?;
        transaction.commit().map_err(backend)?;
        Ok(moved)
    }
}

impl SqliteStore {
    /// Move one task and record the authority for that move in the same transaction.
    ///
    /// A refused transition rolls the receipt back with it. An exact concurrent
    /// replay reads the original durable result and receipt without trying the
    /// compare-and-swap again or substituting the task's current state.
    pub fn transition_task_with_intent(
        &self,
        request: &TaskTransitionRequest,
        envelope: &ReceiptEnvelope<NewCommandIntent>,
    ) -> RepositoryResult<(TaskTransitionResult, CommandReceipt, crate::graph::Applied)> {
        let intent = envelope.peek(self.realm_id())?;
        let target = AggregateRef::Task {
            task_id: request.task_id,
        };
        ensure_atomic_intent_matches(
            intent,
            request.project_id,
            if request.to == TaskState::Withdrawn {
                CommandKind::WithdrawTask
            } else if request.resume_receipt.is_some() {
                CommandKind::ResumeTask
            } else {
                CommandKind::TransitionTask
            },
            &target,
            request.expected_revision,
        )?;
        let transaction = self.begin()?;
        if let Some(existing) = command_receipt_by_key(&transaction, &intent.idempotency_key)? {
            ensure_atomic_replay(&existing, intent)?;
            let result = task_transition_result_in_transaction(&transaction, &existing)?;
            return Ok((result, existing, crate::graph::Applied::Unchanged));
        }

        let moved = transition_task_in_transaction(&transaction, request)?;
        let mut recorded = intent.clone();
        recorded.payload = task_transition_result_payload(&intent.intent, &moved)?;
        let replayed = crate::commands::intent::insert_intent(&transaction, &recorded)?;
        if replayed.is_some() {
            return Err(conflict(
                "command receipt",
                "the idempotency key appeared during one atomic task transition",
            ));
        }
        let receipt = command_receipt_by_key(&transaction, &intent.idempotency_key)?.ok_or(
            RepositoryError::NotFound {
                subject: "command receipt",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok((
            TaskTransitionResult {
                task_id: moved.id,
                state: moved.state,
                revision: moved.revision,
            },
            receipt,
            crate::graph::Applied::Created,
        ))
    }
}

// ---------------------------------------------------------------------------
// Runs, runtime events and closure
// ---------------------------------------------------------------------------

fn read_binding(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    agent_run_id: AgentRunId,
) -> RepositoryResult<Option<RuntimeBinding>> {
    let row: Option<(String, String, String, i64, String, String)> = transaction
        .query_row(
            "SELECT id, runtime_kind, host, generation, native_id, bound_at
             FROM runtime_bindings WHERE project_id = ?1 AND agent_run_id = ?2",
            params![project_id.to_string(), agent_run_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((id, kind, host, generation, native, bound_at)) = row else {
        return Ok(None);
    };
    Ok(Some(RuntimeBinding {
        id: RuntimeBindingId::parse(&id)?,
        agent_run_id,
        identity: NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(&kind)?,
            host: ExternalName::parse(&host)?,
            generation: u64::try_from(generation).unwrap_or_default(),
            native_id: ExternalId::parse(&native)?,
        },
        bound_at: read_timestamp(&bound_at)?,
    }))
}

const AGENT_RUN_COLUMNS: &str = "team_run_id, parent_agent_run_id, role_key, account_profile_id, \
     lifecycle, desired_state, observed_state, derived_state, last_confirmed_at, last_cursor, \
     terminal_outcome, terminal_source_kind, terminal_event_cursor, terminal_receipt_id, \
     terminal_evidence_hash, closed_at, revision, created_at";
const AGENT_RUN_COLUMN_COUNT: usize = 18;

#[allow(clippy::too_many_lines)]
pub(crate) fn read_agent_run(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: AgentRunId,
) -> RepositoryResult<Option<AgentRun>> {
    let row: Option<Vec<rusqlite::types::Value>> = transaction
        .query_row(
            &format!(
                "SELECT {AGENT_RUN_COLUMNS} FROM agent_runs WHERE project_id = ?1 AND id = ?2"
            ),
            params![project_id.to_string(), id.to_string()],
            |row| {
                let mut values = Vec::new();
                for index in 0..AGENT_RUN_COLUMN_COUNT {
                    values.push(row.get::<_, rusqlite::types::Value>(index)?);
                }
                Ok(values)
            },
        )
        .optional()
        .map_err(backend)?;
    let Some(values) = row else {
        return Ok(None);
    };

    let as_text = |index: usize| -> RepositoryResult<String> {
        match &values[index] {
            rusqlite::types::Value::Text(text) => Ok(text.clone()),
            _ => Err(RepositoryError::Backend {
                detail: "agent run column is not text".to_owned(),
            }),
        }
    };
    let as_optional_text = |index: usize| -> Option<String> {
        match &values[index] {
            rusqlite::types::Value::Text(text) => Some(text.clone()),
            _ => None,
        }
    };
    let as_integer = |index: usize| -> Option<i64> {
        match &values[index] {
            rusqlite::types::Value::Integer(value) => Some(*value),
            _ => None,
        }
    };

    // Terminal evidence is rebuilt from its normalized, FK-bound columns rather
    // than from a blob, so what the run claims and what the database can prove
    // are the same thing.
    let terminal_evidence: Option<TerminalEvidence> = match as_optional_text(11) {
        None => None,
        Some(kind) => {
            let outcome = TerminalOutcome::parse(&as_text(10)?)?;
            let evidence_hash = ContentHash::parse(&as_text(14)?)?;
            let source = match kind.as_str() {
                "runtime_observation" => TerminalEvidenceSource::RuntimeObservation {
                    cursor: EventCursor::parse(as_integer(12).unwrap_or_default())?,
                },
                "operator_abandon" => TerminalEvidenceSource::OperatorAbandon {
                    receipt_id: CommandReceiptId::parse(&as_text(13)?)?,
                },
                _ => {
                    return Err(RepositoryError::Backend {
                        detail: "agent run has an unknown terminal evidence source".to_owned(),
                    });
                }
            };
            Some(TerminalEvidence {
                outcome,
                source,
                evidence_hash,
                closed_at: read_timestamp(&as_text(15)?)?,
            })
        }
    };
    let derived = match as_text(7)?.as_str() {
        "terminal" => {
            let evidence = terminal_evidence.as_ref().ok_or(RepositoryError::Backend {
                detail: "a terminal run is stored without evidence".to_owned(),
            })?;
            DerivedRunState::Terminal {
                outcome: evidence.outcome,
            }
        }
        "pending_confirmation" => DerivedRunState::PendingConfirmation,
        "confirmed" => DerivedRunState::Confirmed,
        "stale" => DerivedRunState::Stale,
        "diverged" => DerivedRunState::Diverged,
        "runtime_unavailable" => DerivedRunState::RuntimeUnavailable,
        "orphaned" => DerivedRunState::Orphaned,
        "lost_contact" => DerivedRunState::LostContact,
        _ => {
            return Err(RepositoryError::Backend {
                detail: "agent run has an unknown derived state".to_owned(),
            });
        }
    };

    let last_confirmed_at = as_optional_text(8)
        .as_deref()
        .map(read_timestamp)
        .transpose()?;
    let last_cursor = as_integer(9).map(EventCursor::parse).transpose()?;
    let closed_at = as_optional_text(15)
        .as_deref()
        .map(read_timestamp)
        .transpose()?;

    Ok(Some(AgentRun {
        id,
        project_id,
        team_run_id: TeamRunId::parse(&as_text(0)?)?,
        parent_agent_run_id: as_optional_text(1)
            .as_deref()
            .map(AgentRunId::parse)
            .transpose()?,
        role: RoleKey::parse(&as_text(2)?)?,
        account_profile_id: as_optional_text(3)
            .as_deref()
            .map(AccountProfileId::parse)
            .transpose()?,
        binding: read_binding(transaction, project_id, id)?,
        projection: RunProjection {
            lifecycle: RunLifecycle::parse(&as_text(4)?)?,
            desired: DesiredRunState::parse(&as_text(5)?)?,
            observed: ObservedRunState::parse(&as_text(6)?)?,
            derived,
            last_confirmed_at,
            last_cursor,
        },
        terminal: terminal_evidence,
        revision: revision_of(as_integer(16).unwrap_or_default())?,
        created_at: read_timestamp(&as_text(17)?)?,
        closed_at,
    }))
}

pub(crate) fn generation_column(generation: u64) -> RepositoryResult<i64> {
    i64::try_from(generation).map_err(|_| RepositoryError::Backend {
        detail: "runtime generation exceeds the storable range".to_owned(),
    })
}

impl RunRepository for SqliteStore {
    fn create_team_run(&self, request: &NewTeamRun) -> RepositoryResult<TeamRun> {
        let document = CanonicalDocument::from_serializable(&request.snapshot)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO team_runs
                     (id, project_id, task_id, template_id, template_version, snapshot,
                      snapshot_hash, lifecycle, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', 1, ?8)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.snapshot.template_id.to_string(),
                    version_column(request.snapshot.template_version),
                    document.json(),
                    document.hash().as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(TeamRun {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            snapshot: request.snapshot.clone(),
            lifecycle: RunLifecycle::Queued,
            terminal: None,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
            closed_at: None,
        })
    }

    fn advance_team_run(&self, request: &TeamRunAdvance) -> RepositoryResult<AggregateRevision> {
        if request.to.is_terminal() {
            return Err(DomainError::invalid(
                "team run advance",
                "a terminal value is reached through evidence-bearing closure, not an advance",
            )
            .into());
        }
        let transaction = self.begin()?;
        let row: Option<(String, i64)> = transaction
            .query_row(
                "SELECT lifecycle, revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.team_run_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((lifecycle, revision)) = row else {
            return Err(RepositoryError::NotFound {
                subject: "team run",
            });
        };
        let lifecycle = RunLifecycle::parse(&lifecycle)?;
        let revision = revision_of(revision)?;
        // Terminality, the CAS and the legal transition table are all decided by
        // the domain, so the rule this store enforces is the same one the core
        // unit tests exercise.
        let next = plan_team_advance(lifecycle, revision, request.expected_revision, request.to)?;
        let changed = transaction
            .execute(
                "UPDATE team_runs SET lifecycle = ?1, revision = ?2
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    request.to.as_str(),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.team_run_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "team run",
                "the run revision moved during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(next)
    }

    fn close_team_run(&self, request: &TeamRunClosure) -> RepositoryResult<()> {
        request.evidence.validate()?;
        let transaction = self.begin()?;
        let row: Option<(String, i64)> = transaction
            .query_row(
                "SELECT lifecycle, revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.team_run_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((lifecycle, revision)) = row else {
            return Err(RepositoryError::NotFound {
                subject: "team run",
            });
        };
        let lifecycle = RunLifecycle::parse(&lifecycle)?;
        let revision = revision_of(revision)?;

        // Both kinds of evidence are *loaded* here and decided by the domain.
        // The child digest in particular is recomputed from the persisted child
        // rows: recomputing only the outcome would still let a caller store an
        // arbitrary `evidence_hash`, leaving the stored evidence bound to
        // nothing.
        let children =
            read_team_child_evidence(&transaction, request.project_id, request.team_run_id)?;
        let receipt_column = match request.evidence.source {
            TeamEvidenceSource::ChildEvidence { .. }
            // A settled-turn closure cites no receipt: its evidence is the team's
            // own immutable `role_turns` rows, which the transition check
            // re-proves rather than a receipt attesting.
            | TeamEvidenceSource::SettledTurns { .. }
            // Nor does a disposition closure: its evidence is this team's own
            // `role_turns` and `role_slot_waivers` rows.
            | TeamEvidenceSource::RoleSlotDispositions { .. } => None,
            TeamEvidenceSource::OperatorAbandon { receipt_id } => Some(receipt_id),
        };
        let receipt = receipt_column
            .map(|receipt_id| {
                read_abandon_receipt(
                    &transaction,
                    request.project_id,
                    receipt_id,
                    &AggregateRef::TeamRun {
                        team_run_id: request.team_run_id,
                    },
                )
            })
            .transpose()?;
        let next = plan_team_closure(
            lifecycle,
            revision,
            request.expected_revision,
            request.team_run_id,
            &request.evidence,
            &children,
            receipt.as_ref(),
        )?;
        let receipt_column = receipt_column.map(|id| id.to_string());

        let changed = transaction
            .execute(
                "UPDATE team_runs
                 SET lifecycle = ?1, terminal_outcome = ?2, terminal_source_kind = ?3,
                     terminal_receipt_id = ?4, terminal_evidence_hash = ?5, closed_at = ?6,
                     revision = ?7
                 WHERE project_id = ?8 AND id = ?9 AND revision = ?10",
                params![
                    request.evidence.outcome.lifecycle().as_str(),
                    request.evidence.outcome.as_str(),
                    match request.evidence.source {
                        TeamEvidenceSource::ChildEvidence { .. } => "child_evidence",
                        TeamEvidenceSource::OperatorAbandon { .. } => "operator_abandon",
                        TeamEvidenceSource::SettledTurns { .. } => "settled_turns",
                        TeamEvidenceSource::RoleSlotDispositions { .. } => "role_slot_dispositions",
                    },
                    receipt_column,
                    request.evidence.evidence_hash.as_str(),
                    text(request.evidence.closed_at),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.team_run_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "team run",
                "the run revision moved during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn create_agent_run(&self, request: &NewAgentRun) -> RepositoryResult<AgentRun> {
        let transaction = self.begin()?;

        // Lineage must be a chain, not a ring: walk it before inserting.
        let mut ancestor = request.parent_agent_run_id;
        let mut walked = 0usize;
        while let Some(current) = ancestor {
            if current == request.id {
                return Err(DomainError::invalid(
                    "agent run lineage",
                    "the parent chain would form a cycle",
                )
                .into());
            }
            walked += 1;
            if walked > MAX_PARENT_CHAIN {
                return Err(DomainError::invalid(
                    "agent run lineage",
                    "the parent chain is longer than the bound allows",
                )
                .into());
            }
            let parent: Option<Option<String>> = transaction
                .query_row(
                    "SELECT parent_agent_run_id FROM agent_runs
                     WHERE project_id = ?1 AND id = ?2",
                    params![request.project_id.to_string(), current.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let Some(parent) = parent else {
                return Err(RepositoryError::CrossProject {
                    subject: "parent agent run",
                });
            };
            ancestor = parent.as_deref().map(AgentRunId::parse).transpose()?;
        }

        transaction
            .execute(
                "INSERT INTO agent_runs
                     (id, project_id, team_run_id, parent_agent_run_id, role_key,
                      account_profile_id, lifecycle, desired_state, observed_state,
                      derived_state, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', 'no_intent', 'unknown',
                         'pending_confirmation', 1, ?7)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.team_run_id.to_string(),
                    request.parent_agent_run_id.map(|id| id.to_string()),
                    request.role.as_str(),
                    request.account_profile_id.map(|id| id.to_string()),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;

        if let Some(binding) = &request.binding {
            transaction
                .execute(
                    "INSERT INTO runtime_bindings
                         (id, project_id, agent_run_id, runtime_kind, host, generation,
                          native_id, bound_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        binding.id.to_string(),
                        request.project_id.to_string(),
                        request.id.to_string(),
                        binding.identity.runtime_kind.as_str(),
                        binding.identity.host.as_str(),
                        generation_column(binding.identity.generation)?,
                        binding.identity.native_id.as_str(),
                        text(binding.bound_at)
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;

        let transaction = self.begin()?;
        read_agent_run(&transaction, request.project_id, request.id)?.ok_or(
            RepositoryError::NotFound {
                subject: "agent run",
            },
        )
    }

    fn get_team_run(
        &self,
        project_id: ProjectId,
        id: TeamRunId,
    ) -> RepositoryResult<Option<TeamRun>> {
        let row: Option<RepositoryResult<TeamRun>> = self
            .connection
            .query_row(
                "SELECT task_id, snapshot, snapshot_hash, lifecycle, created_at,
                        terminal_outcome, revision, closed_at, terminal_source_kind,
                        terminal_receipt_id, terminal_evidence_hash
                 FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| {
                    let build = || -> RepositoryResult<TeamRun> {
                        let outcome: Option<String> = row.get(5).map_err(backend)?;
                        let closed_at: Option<String> = row.get(7).map_err(backend)?;
                        let source_kind: Option<String> = row.get(8).map_err(backend)?;
                        let receipt: Option<String> = row.get(9).map_err(backend)?;
                        let evidence_hash: Option<String> = row.get(10).map_err(backend)?;
                        Ok(TeamRun {
                            id,
                            project_id,
                            task_id: TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                            snapshot: team_run_snapshot(
                                &row.get::<_, String>(1).map_err(backend)?,
                                &row.get::<_, String>(2).map_err(backend)?,
                            )?,
                            lifecycle: RunLifecycle::parse(
                                &row.get::<_, String>(3).map_err(backend)?,
                            )?,
                            // Rebuilt from normalized, FK-bound columns, exactly
                            // like an agent run's.
                            terminal: match (source_kind, outcome, evidence_hash) {
                                (Some(kind), Some(outcome), Some(hash)) => {
                                    Some(TeamTerminalEvidence {
                                        outcome: TerminalOutcome::parse(&outcome)?,
                                        // Every stored kind decodes to its own
                                        // variant. A catch-all that collapsed the
                                        // unrecognized ones into `ChildEvidence`
                                        // silently undid the separate typing on
                                        // reload: a team closed on settled turns
                                        // came back claiming its children had
                                        // ended, which is a claim about runs that
                                        // are deliberately still live.
                                        source: match kind.as_str() {
                                            "operator_abandon" => {
                                                TeamEvidenceSource::OperatorAbandon {
                                                    receipt_id: CommandReceiptId::parse(
                                                        receipt.as_deref().unwrap_or_default(),
                                                    )?,
                                                }
                                            }
                                            "settled_turns" => {
                                                TeamEvidenceSource::SettledTurns { team_run_id: id }
                                            }
                                            "role_slot_dispositions" => {
                                                TeamEvidenceSource::RoleSlotDispositions {
                                                    team_run_id: id,
                                                }
                                            }
                                            "child_evidence" => TeamEvidenceSource::ChildEvidence {
                                                team_run_id: id,
                                            },
                                            // The column's `CHECK` admits exactly
                                            // four values, so anything else is a
                                            // row this binary cannot read — and
                                            // guessing which closure it meant is
                                            // how the defect above happened.
                                            _ => {
                                                return Err(RepositoryError::Backend {
                                                    detail: "a team run carries an \
                                                             unreadable terminal source"
                                                        .to_owned(),
                                                });
                                            }
                                        },
                                        evidence_hash: ContentHash::parse(&hash)?,
                                        closed_at: read_timestamp(
                                            closed_at.as_deref().unwrap_or_default(),
                                        )?,
                                    })
                                }
                                _ => None,
                            },
                            revision: revision_of(row.get::<_, i64>(6).map_err(backend)?)?,
                            created_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
                            closed_at: closed_at.as_deref().map(read_timestamp).transpose()?,
                        })
                    };
                    Ok(build())
                },
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }

    fn get_agent_run(
        &self,
        project_id: ProjectId,
        id: AgentRunId,
    ) -> RepositoryResult<Option<AgentRun>> {
        let transaction = self.begin()?;
        read_agent_run(&transaction, project_id, id)
    }

    fn record_run_context_policy(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        snapshot: &kontor_core::spec::ContextPolicySnapshot,
    ) -> RepositoryResult<()> {
        // Both halves are re-verified against their own digests before anything
        // is written: a snapshot whose bytes and hash disagree is not a record
        // of anything, and storing it would launder the disagreement.
        snapshot.verify()?;
        let requested = CanonicalDocument::from_serializable(&snapshot.requested)?;
        let effective = CanonicalDocument::from_serializable(&snapshot.effective)?;

        let transaction = self.begin()?;
        // Realm isolation is the file; *project* isolation is this lookup. A run
        // another project owns is not found rather than written to.
        if read_agent_run(&transaction, project_id, agent_run_id)?.is_none() {
            return Err(RepositoryError::NotFound {
                subject: "agent run",
            });
        }

        if let Some(existing) = read_run_context_policy(&transaction, agent_run_id)? {
            // A replay of the identical pair is the same act. Anything else is a
            // second answer to a question already answered.
            if existing.requested_hash == snapshot.requested_hash
                && existing.effective_hash == snapshot.effective_hash
            {
                return Ok(());
            }
            return Err(conflict(
                "run context policy",
                "this run was already launched under a different context policy",
            ));
        }

        transaction
            .execute(
                "INSERT INTO run_context_policies
                     (agent_run_id, source, requested_class, requested_tokens, effective_tokens,
                      enforcement, capability, clamp, requested, requested_hash, effective,
                      effective_hash, resolved_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    agent_run_id.to_string(),
                    snapshot.requested.source.as_str(),
                    snapshot.requested.policy.class.as_str(),
                    snapshot
                        .requested
                        .trigger_tokens
                        .map(i64::try_from)
                        .transpose()
                        .ok()
                        .flatten(),
                    snapshot
                        .effective
                        .trigger_tokens
                        .map(i64::try_from)
                        .transpose()
                        .ok()
                        .flatten(),
                    snapshot.requested.policy.enforcement.as_str(),
                    snapshot.effective.capability.as_str(),
                    snapshot.effective.clamp.as_str(),
                    requested.json(),
                    requested.hash().as_str(),
                    effective.json(),
                    effective.hash().as_str(),
                    text(snapshot.resolved_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn get_run_context_policy(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<Option<kontor_core::spec::ContextPolicySnapshot>> {
        let transaction = self.begin()?;
        if read_agent_run(&transaction, project_id, agent_run_id)?.is_none() {
            return Ok(None);
        }
        read_run_context_policy(&transaction, agent_run_id)
    }

    fn record_compaction_receipt(
        &self,
        project_id: ProjectId,
        receipt: &kontor_core::compaction::CompactionReceipt,
    ) -> RepositoryResult<kontor_core::compaction::CompactionReceipt> {
        // `canonicalize` validates first, so a receipt claiming `confirmed`
        // without unchanged identity or without evidence never reaches storage.
        let document = receipt.canonicalize()?;

        let transaction = self.begin()?;
        if read_agent_run(&transaction, project_id, receipt.agent_run_id)?.is_none() {
            return Err(RepositoryError::NotFound {
                subject: "agent run",
            });
        }

        if let Some((stored, hash)) = read_compaction_receipt(&transaction, receipt.id)? {
            if &hash == document.hash() {
                return Ok(stored);
            }
            return Err(conflict(
                "compaction receipt",
                "this receipt id already records a different attempt",
            ));
        }

        transaction
            .execute(
                "INSERT INTO compaction_receipts
                     (id, agent_run_id, binding_id, trigger_kind, status, native_before,
                      native_after, generation_before, generation_after, receipt, receipt_hash,
                      recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    receipt.id.to_string(),
                    receipt.agent_run_id.to_string(),
                    receipt.binding_id.to_string(),
                    receipt.trigger.as_str(),
                    receipt.status.as_str(),
                    receipt.native_before.native_id.as_str(),
                    receipt
                        .native_after
                        .as_ref()
                        .map(|after| after.native_id.as_str().to_owned()),
                    i64::try_from(receipt.native_before.generation).unwrap_or(i64::MAX),
                    receipt
                        .native_after
                        .as_ref()
                        .map(|after| i64::try_from(after.generation).unwrap_or(i64::MAX)),
                    document.json(),
                    document.hash().as_str(),
                    text(receipt.recorded_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(receipt.clone())
    }

    fn latest_compaction_receipt(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<Option<kontor_core::compaction::CompactionReceipt>> {
        let transaction = self.begin()?;
        if read_agent_run(&transaction, project_id, agent_run_id)?.is_none() {
            return Ok(None);
        }
        let row: Option<(String, String)> = transaction
            .query_row(
                "SELECT receipt, receipt_hash FROM compaction_receipts
                 WHERE agent_run_id = ?1
                 ORDER BY recorded_at DESC, id DESC
                 LIMIT 1",
                params![agent_run_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        row.map(|(json, hash)| decode_compaction_receipt(&json, &hash))
            .transpose()
    }

    fn append_runtime_event(&self, request: &NewRuntimeEvent) -> RepositoryResult<EventCursor> {
        crate::events::append::append_runtime_event(self, request)
    }

    fn record_observation(&self, request: &NewObservation) -> RepositoryResult<RunProjection> {
        crate::events::append::record_observation(self, request)
    }

    fn record_abandon_receipt(
        &self,
        request: &NewAbandonReceipt,
    ) -> RepositoryResult<CommandReceiptId> {
        let transaction = self.begin()?;
        // A repeat of the same decision cites the first receipt. The comparison
        // is on what the receipt is *for*, so a key reused for a different run or
        // a different document is refused rather than silently answered with an
        // unrelated authorization.
        let existing: Option<(String, String, String, String)> = transaction
            .query_row(
                "SELECT id, kind, target, intent_hash FROM command_receipts
                 WHERE idempotency_key = ?1",
                params![request.idempotency_key.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        let target = request.target;
        if let Some((id, kind, stored_target, intent_hash)) = existing {
            let stored_target: AggregateRef = from_json(&stored_target)?;
            if kind != CommandKind::AbandonRun.as_str()
                || stored_target != target
                || intent_hash != request.intent.hash().as_str()
            {
                return Err(DomainError::invalid(
                    "CommandReceipt",
                    "an idempotency key may not be reused for a different command",
                )
                .into());
            }
            return CommandReceiptId::parse(&id).map_err(Into::into);
        }

        // Born `confirmed`, not `intent_persisted`. Nothing is dispatched here:
        // the closure is already committed in this same transaction, so there is
        // no outbox entry and never will be. `intent_persisted` is the one state
        // that authorizes a launch, and a restart's recovery scan reads it as
        // "this was queued and never sent" — it then demands the outbox row that
        // by design does not exist, and the whole startup inventory fails.
        transaction
            .execute(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision, intent,
                      intent_hash, state, attempts, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'confirmed', 0, ?9, ?9)",
                params![
                    request.receipt_id.to_string(),
                    request.project_id.to_string(),
                    request.idempotency_key.as_str(),
                    CommandKind::AbandonRun.as_str(),
                    to_json(&target)?,
                    revision_column(request.target_revision)?,
                    request.intent.json(),
                    request.intent.hash().as_str(),
                    text(request.recorded_at)
                ],
            )
            .map_err(backend)?;
        let (kind, columns) = target_columns(&target);
        transaction
            .execute(
                "INSERT INTO command_targets
                     (project_id, receipt_id, target_kind, target_project_id,
                      target_mini_project_id, target_task_id, target_team_run_id,
                      target_agent_run_id, target_ticket_link_id, target_work_calendar_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    request.project_id.to_string(),
                    request.receipt_id.to_string(),
                    kind,
                    columns[0],
                    columns[1],
                    columns[2],
                    columns[3],
                    columns[4],
                    columns[5],
                    columns[6]
                ],
            )
            .map_err(backend)?;
        // The intent document is the evidence: it is what the decision was, and
        // the transitions table refuses a confirmation that cites none.
        let evidence = ExternalId::parse(request.intent.hash().as_str())?;
        crate::commands::receipts::append_transition(
            &transaction,
            request.project_id,
            request.receipt_id,
            1,
            kontor_core::receipt::CommandReceiptState::Confirmed,
            None,
            None,
            Some(&evidence),
            request.recorded_at,
        )?;
        transaction.commit().map_err(backend)?;
        Ok(request.receipt_id)
    }

    fn close_agent_run(&self, request: &RunClosure) -> RepositoryResult<()> {
        request.evidence.validate()?;
        let transaction = self.begin()?;
        let run = read_agent_run(&transaction, request.project_id, request.agent_run_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "agent run",
            },
        )?;
        run.projection.ensure_open("agent run")?;
        run.revision
            .expect("agent run", request.expected_revision)?;

        // The cited evidence is loaded and re-proved here, inside the closing
        // transaction. A caller cannot close a run with a plausible-looking
        // blob, with another run's event, or with another project's receipt.
        let (source_kind, cursor_column, receipt_column) = match request.evidence.source {
            TerminalEvidenceSource::RuntimeObservation { cursor } => {
                type EvidenceRow = (
                    String,
                    String,
                    String,
                    String,
                    i64,
                    String,
                    String,
                    Option<String>,
                    i64,
                );
                let found: Option<EvidenceRow> = transaction
                    .query_row(
                        "SELECT agent_run_id, runtime_kind, host, native_id, generation,
                                    payload_hash, observed_at, observed_state, native_sequence
                             FROM runtime_events
                             WHERE project_id = ?1 AND cursor = ?2
                               AND event_kind = 'runtime_observation'",
                        params![request.project_id.to_string(), cursor.get()],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                                row.get(6)?,
                                row.get(7)?,
                                row.get(8)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(backend)?;
                let Some((
                    event_run,
                    kind,
                    host,
                    native,
                    generation,
                    payload_hash,
                    observed_at,
                    observed_state,
                    native_sequence,
                )) = found
                else {
                    return Err(RepositoryError::NotFound {
                        subject: "terminal evidence event",
                    });
                };

                // 1. the event belongs to this run, in this project
                if AgentRunId::parse(&event_run)? != request.agent_run_id {
                    return Err(DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event belongs to a different run",
                    }
                    .into());
                }
                // 2. it was emitted by the run's immutable binding
                let binding = run.binding.as_ref().ok_or(DomainError::MissingEvidence {
                    subject: "run closure",
                    rule: "a runtime closure requires the run to be bound",
                })?;
                let identity = NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse(&kind)?,
                    host: ExternalName::parse(&host)?,
                    generation: u64::try_from(generation).unwrap_or_default(),
                    native_id: ExternalId::parse(&native)?,
                };
                if !binding.identity.same_session(&identity) {
                    return Err(DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event was not emitted by this run's binding",
                    }
                    .into());
                }
                let observed = ObservedRunState::parse(&observed_state.ok_or(
                    DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event records no observed state",
                    },
                )?)?;
                // 3. the event must be the one the projection actually
                //    reduced. A late, older terminal event is still appended as
                //    evidence, but the monotonic guard refused to reduce it —
                //    so it never became this run's observed truth and must not
                //    be able to close it either. Anything newer has not been
                //    reduced yet.
                let reduced: Option<i64> = transaction
                    .query_row(
                        "SELECT last_native_sequence FROM agent_runs
                         WHERE project_id = ?1 AND id = ?2",
                        params![
                            request.project_id.to_string(),
                            request.agent_run_id.to_string()
                        ],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?
                    .flatten();
                if reduced != Some(native_sequence) {
                    return Err(DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event never reduced this run's projection",
                    }
                    .into());
                }
                // 4-6. digest, terminal outcome and closure ordering
                request.evidence.verify_observation(
                    observed,
                    read_timestamp(&observed_at)?,
                    &ContentHash::parse(&payload_hash)?,
                )?;
                ("runtime_observation", Some(cursor.get()), None)
            }
            TerminalEvidenceSource::OperatorAbandon { receipt_id } => {
                // The receipt must target this exact run *and* the exact
                // revision being closed, not merely exist.
                let facts = read_abandon_receipt(
                    &transaction,
                    request.project_id,
                    receipt_id,
                    &AggregateRef::AgentRun {
                        agent_run_id: request.agent_run_id,
                    },
                )?;
                request.evidence.verify_abandon(run.revision, &facts)?;
                ("operator_abandon", None, Some(receipt_id.to_string()))
            }
        };

        let next_revision = run.revision.next()?;
        let changed = transaction
            .execute(
                "UPDATE agent_runs
                 SET lifecycle = ?1, derived_state = 'terminal', terminal_outcome = ?2,
                     terminal_source_kind = ?3, terminal_event_cursor = ?4,
                     terminal_receipt_id = ?5, terminal_evidence_hash = ?6, closed_at = ?7,
                     revision = ?8
                 WHERE project_id = ?9 AND id = ?10 AND revision = ?11",
                params![
                    request.evidence.outcome.lifecycle().as_str(),
                    request.evidence.outcome.as_str(),
                    source_kind,
                    cursor_column,
                    receipt_column,
                    request.evidence.evidence_hash.as_str(),
                    text(request.evidence.closed_at),
                    revision_column(next_revision)?,
                    request.project_id.to_string(),
                    request.agent_run_id.to_string(),
                    revision_column(run.revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "agent run",
                "the run revision moved during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn read_runtime_events(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        after: Option<EventCursor>,
    ) -> RepositoryResult<Vec<RuntimeEvent>> {
        crate::events::replay::read_runtime_events(self, project_id, agent_run_id, after)
    }
}

// ---------------------------------------------------------------------------
// Source events and intake
// ---------------------------------------------------------------------------

impl IntakeRepository for SqliteStore {
    fn ingest_source_event(
        &self,
        project_id: ProjectId,
        event: &CanonicalSourceEvent,
    ) -> RepositoryResult<SourceEventIngest> {
        crate::intake::ingest_source_event(self, project_id, event)
    }

    fn record_intake_decision(
        &self,
        request: &NewIntakeDecision,
    ) -> RepositoryResult<IntakeOutcome> {
        crate::intake::record_intake_decision(self, request)
    }

    fn record_source_event(&self, request: &NewSourceEvent) -> RepositoryResult<IntakeOutcome> {
        crate::intake::record_source_event(self, request)
    }

    fn commit_intake_decision(
        &self,
        request: &NewIntakeDecisionRecord,
    ) -> RepositoryResult<IntakeDecisionRecord> {
        crate::intake::commit_intake_decision(self, request)
    }

    fn get_intake_decision(
        &self,
        project_id: ProjectId,
        receipt_id: IntakeReceiptId,
    ) -> RepositoryResult<Option<IntakeDecisionRecord>> {
        crate::intake::get_intake_decision(self, project_id, receipt_id)
    }

    fn intake_lineage_of_task(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<IntakeCreatedWork>> {
        crate::intake::intake_lineage_of_task(self, project_id, task_id)
    }

    fn reevaluate_source_event(
        &self,
        request: &NewIntakeReevaluation,
    ) -> RepositoryResult<ReevaluationOutcome> {
        request.receipt.validate()?;
        let transaction = self.begin()?;

        // The event must exist in *this* project and still hash to what the
        // caller believes; a changed digest means they are deciding about
        // something else.
        let stored_hash: Option<String> = transaction
            .query_row(
                "SELECT envelope_hash FROM source_events WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.source_event_id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(stored_hash) = stored_hash else {
            return Err(RepositoryError::NotFound {
                subject: "source event",
            });
        };
        if ContentHash::parse(&stored_hash)? != request.source_event_hash {
            return Err(DomainError::invalid(
                "IntakeReevaluation",
                "the source event no longer has the cited digest",
            )
            .into());
        }

        // The newest decision so far, and the revision it used.
        let latest: Option<(String, i64, String)> = transaction
            .query_row(
                "SELECT id, trigger_version, receipt FROM intake_receipts
                 WHERE project_id = ?1 AND source_event_id = ?2 AND trigger_key = ?3
                 ORDER BY trigger_version DESC LIMIT 1",
                params![
                    request.project_id.to_string(),
                    request.source_event_id.to_string(),
                    request.receipt.trigger.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((latest_id, latest_version, latest_json)) = latest else {
            return Err(RepositoryError::NotFound {
                subject: "existing intake receipt",
            });
        };
        let latest_version = read_version(latest_version)?;
        let incoming = request.receipt.trigger_version;

        // Same revision: the decision has already been made. It is only a replay
        // if it is the *same* decision — a trigger revision is deterministic, so
        // a differing verdict, idempotency key or proposed graph under the same
        // revision is a contradiction rather than a repeat.
        if incoming == latest_version {
            let stored: IntakeReceipt = from_json(&latest_json)?;
            if !request.receipt.decides_the_same_as(&stored) {
                return Err(conflict(
                    "intake re-evaluation",
                    "the same trigger revision already recorded a different decision",
                ));
            }
            return Ok(ReevaluationOutcome::AlreadyDecided(Box::new(stored)));
        }
        if incoming.get() < latest_version.get() {
            return Err(DomainError::invalid(
                "IntakeReevaluation",
                "a trigger revision older than the latest decision cannot supersede it",
            )
            .into());
        }

        // The successor must decide the very event this request named, at the
        // digest the request proved, before it is linked to anything.
        request
            .receipt
            .ensure_decides(request.source_event_id, &request.source_event_hash)?;

        // The successor is pinned to a revision that must actually exist.
        let predecessor = IntakeReceiptId::parse(&latest_id)?;
        let successor = IntakeReceipt {
            predecessor_receipt_id: Some(predecessor),
            ..request.receipt.clone()
        };
        successor.validate()?;
        let receipt_json = to_json(&successor)?;
        transaction
            .execute(
                "INSERT INTO intake_receipts
                     (id, project_id, source_event_id, source_event_hash, trigger_key,
                      trigger_version, result, receipt, idempotency_key, dedup_key,
                      duplicate_of, predecessor_receipt_id, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    successor.id.to_string(),
                    request.project_id.to_string(),
                    successor.source_event_id.to_string(),
                    successor.source_event_hash.as_str(),
                    successor.trigger.as_str(),
                    version_column(successor.trigger_version),
                    successor.result.as_str(),
                    receipt_json,
                    successor.idempotency_key.as_str(),
                    successor.dedup_key.as_str(),
                    successor.duplicate_of.map(|id| id.to_string()),
                    predecessor.to_string(),
                    text(successor.decided_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(ReevaluationOutcome::Superseded(Box::new(successor)))
    }

    fn find_intake_receipt(
        &self,
        project_id: ProjectId,
        identity: &SourceIdentity,
    ) -> RepositoryResult<Option<IntakeReceipt>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT r.receipt FROM intake_receipts r
                 JOIN source_events e ON e.id = r.source_event_id AND e.project_id = r.project_id
                 WHERE r.project_id = ?1 AND e.source_kind = ?2 AND e.source_connection = ?3
                   AND e.external_event_id = ?4
                 ORDER BY r.decided_at LIMIT 1",
                params![
                    project_id.to_string(),
                    identity.source_kind.as_str(),
                    identity.source_connection.as_str(),
                    identity.external_event_id.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|json| from_json::<IntakeReceipt>(&json))
            .transpose()
    }

    fn get_intake_receipt(
        &self,
        project_id: ProjectId,
        id: IntakeReceiptId,
    ) -> RepositoryResult<Option<IntakeReceipt>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT receipt FROM intake_receipts WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|json| from_json::<IntakeReceipt>(&json))
            .transpose()
    }
}

// ---------------------------------------------------------------------------
// Commands and outbox
// ---------------------------------------------------------------------------

pub(crate) const RECEIPT_COLUMNS: &str = "id, project_id, idempotency_key, kind, target, target_revision, \
     intent, intent_hash, state, correlation, native_identity, result_ref, attempts, created_at, \
     updated_at";

impl CommandRepository for SqliteStore {
    fn record_intent(&self, request: &NewCommandIntent) -> RepositoryResult<CommandReceipt> {
        crate::commands::intent::record_intent(self, request)
    }

    fn record_local_command(&self, request: &NewLocalCommand) -> RepositoryResult<CommandReceipt> {
        crate::commands::intent::record_local_command(self, request)
    }

    fn record_local_command_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
    ) -> RepositoryResult<CommandReceipt> {
        let request = envelope.peek(self.realm_id())?;
        self.record_local_command(request)
    }

    fn complete_local_command(
        &self,
        key: &IdempotencyKey,
        completed_at: Timestamp,
    ) -> RepositoryResult<Option<CommandReceipt>> {
        crate::commands::intent::complete_local_command(self, key, completed_at)
    }

    fn get_receipt_by_key(&self, key: &IdempotencyKey) -> RepositoryResult<Option<CommandReceipt>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {RECEIPT_COLUMNS} FROM command_receipts WHERE idempotency_key = ?1"
                ),
                params![key.as_str()],
                |row| Ok(crate::commands::receipts::read_receipt_row(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn advance_receipt(&self, request: &ReceiptAdvance) -> RepositoryResult<CommandReceipt> {
        crate::commands::receipts::advance_receipt(self, request)
    }

    fn claim_outbox(
        &self,
        project_id: ProjectId,
        now: Timestamp,
        limit: u32,
    ) -> RepositoryResult<Vec<CommandOutboxEntry>> {
        crate::commands::intent::read_outbox(self, project_id, now, limit)
    }
}

// ---------------------------------------------------------------------------
// External tickets
// ---------------------------------------------------------------------------

impl TicketRepository for SqliteStore {
    fn create_ticket_link(&self, request: &NewTicketLink) -> RepositoryResult<TicketLink> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO jira_links
                     (id, project_id, task_id, connector, external_issue_key, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.connector.as_str(),
                    request.external_issue_key.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(TicketLink {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            connector: request.connector.clone(),
            external_issue_key: request.external_issue_key.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn insert_projection(
        &self,
        project_id: ProjectId,
        projection: &TicketSyncProjection,
        spec: &TicketFieldSpec,
    ) -> RepositoryResult<()> {
        // `canonicalize` already checks the projection against the mapping. The
        // projection also has to *name* that exact specification, or the pin it
        // persists would point somewhere the check never looked.
        if projection.field_spec_project != spec.project
            || projection.field_spec_issue_type != spec.issue_type
            || projection.field_spec_version != spec.version
            || projection.connector != spec.connector
        {
            return Err(DomainError::invalid(
                "TicketSyncProjection",
                "the pinned field specification is not the one it was checked against",
            )
            .into());
        }
        let document = projection.canonicalize(spec)?;
        let fields = to_json(&projection.fields)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO ticket_sync_projections
                     (id, project_id, link_id, link_revision, connector, field_spec_project,
                      field_spec_issue_type, field_spec_version, external_issue_key,
                      fields, comment_policy, external_comment_cursor, projection_hash,
                      computed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    projection.id.to_string(),
                    project_id.to_string(),
                    projection.link_id.to_string(),
                    revision_column(projection.link_revision)?,
                    projection.connector.as_str(),
                    projection.field_spec_project.as_str(),
                    projection.field_spec_issue_type.as_str(),
                    version_column(projection.field_spec_version),
                    projection.external_issue_key.as_str(),
                    fields,
                    projection.comment_policy.as_str(),
                    projection
                        .external_comment_cursor
                        .as_ref()
                        .map(ExternalId::as_str),
                    document.hash().as_str(),
                    text(projection.computed_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn append_observation(
        &self,
        project_id: ProjectId,
        observation: &ExternalTicketObservation,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO external_ticket_observations
                     (id, project_id, link_id, status_id, status_name, status_category,
                      issue_type, assignee_account_id, assignee_display, external_version,
                      observed_at, payload_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    observation.id.to_string(),
                    project_id.to_string(),
                    observation.link_id.to_string(),
                    observation.status.status_id.as_str(),
                    observation.status.status_name.as_str(),
                    observation.status_category.as_str(),
                    observation.issue_type.as_str(),
                    observation
                        .assignee_account_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    observation
                        .assignee_display
                        .as_ref()
                        .map(ExternalName::as_str),
                    observation
                        .external_version
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(observation.observed_at),
                    observation.payload_hash.as_str()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn append_comment(
        &self,
        project_id: ProjectId,
        comment: &ExternalCommentRevision,
    ) -> RepositoryResult<bool> {
        comment.verify()?;
        let transaction = self.begin()?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO external_comments
                     (project_id, link_id, external_comment_id, body_hash, author_account_id,
                      author_display, external_created_at, external_updated_at, body,
                      observed_at, supersedes_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    project_id.to_string(),
                    comment.link_id.to_string(),
                    comment.external_comment_id.as_str(),
                    comment.body_hash.as_str(),
                    comment.author_account_id.as_str(),
                    comment.author_display.as_ref().map(ExternalName::as_str),
                    text(comment.external_created_at),
                    text(comment.external_updated_at),
                    comment.body.as_str(),
                    text(comment.observed_at),
                    comment.supersedes.as_ref().map(ContentHash::as_str)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(inserted == 1)
    }

    fn insert_conflict(
        &self,
        project_id: ProjectId,
        conflict_record: &StatusConflict,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO status_conflicts
                     (id, project_id, link_id, kind, observation_id, task_revision,
                      spec_version, milestone, detected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    conflict_record.id.to_string(),
                    project_id.to_string(),
                    conflict_record.link_id.to_string(),
                    conflict_record.kind.as_str(),
                    conflict_record.observation_id.to_string(),
                    revision_column(conflict_record.task_revision)?,
                    version_column(conflict_record.spec_version),
                    conflict_record
                        .milestone
                        .as_ref()
                        .map(kontor_core::id::SemanticMilestoneKey::as_str),
                    text(conflict_record.detected_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn resolve_conflict(
        &self,
        project_id: ProjectId,
        conflict_id: StatusConflictId,
        receipt: CommandReceiptId,
        resolved_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        // A conflict is a disagreement about one ticket link, so the receipt
        // that resolves it must be a resolution aimed at that same link. The
        // link is read from the stored conflict rather than taken on trust.
        let link: Option<String> = transaction
            .query_row(
                "SELECT link_id FROM status_conflicts
                 WHERE project_id = ?1 AND id = ?2 AND resolved_at IS NULL",
                params![project_id.to_string(), conflict_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(link) = link else {
            return Err(conflict(
                "status conflict",
                "the conflict is unknown or already resolved",
            ));
        };
        ensure_receipt_authorizes(
            &transaction,
            "StatusConflict",
            project_id,
            receipt,
            CommandKind::ResolveStatusConflict,
            AggregateRef::TicketLink {
                link_id: TicketLinkId::parse(&link)?,
            },
        )?;
        let changed = transaction
            .execute(
                "UPDATE status_conflicts SET resolved_at = ?1, resolution_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4 AND resolved_at IS NULL",
                params![
                    text(resolved_at),
                    receipt.to_string(),
                    project_id.to_string(),
                    conflict_id.to_string()
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "status conflict",
                "the conflict is unknown or already resolved",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn insert_transition_receipt(
        &self,
        project_id: ProjectId,
        receipt: &StatusTransitionReceipt,
    ) -> RepositoryResult<()> {
        receipt.validate()?;
        let plan = to_json(&receipt.plan)?;
        let assignment = receipt
            .assignment_result
            .as_ref()
            .map(to_json)
            .transpose()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO status_transition_receipts
                     (id, project_id, link_id, task_id, task_revision, workflow_revision,
                      projection_revision, spec_version, prior_observation_id, milestone,
                      target_status_id, transition_id, principal_account_id,
                      assignment_prerequisite, assignment_result, plan, idempotency_key,
                      dispatched_at, acknowledged_at, confirmed_at, refetched_observation_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21)",
                params![
                    receipt.id.to_string(),
                    project_id.to_string(),
                    receipt.link_id.to_string(),
                    receipt.task_id.to_string(),
                    revision_column(receipt.task_revision)?,
                    revision_column(receipt.workflow_revision)?,
                    revision_column(receipt.projection_revision)?,
                    version_column(receipt.spec_version),
                    receipt.prior_observation_id.to_string(),
                    receipt.plan.milestone.as_str(),
                    receipt.plan.target.status_id.as_str(),
                    receipt
                        .plan
                        .transition
                        .as_ref()
                        .map(|selected| selected.transition_id.as_str()),
                    receipt.principal.account_id.as_str(),
                    i64::from(receipt.plan.assignment_prerequisite),
                    assignment,
                    plan,
                    receipt.idempotency_key.as_str(),
                    text(receipt.dispatched_at),
                    receipt.acknowledged_at.map(text),
                    receipt.confirmed_at.map(text),
                    receipt.refetched_observation_id.map(|id| id.to_string())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Calendars, authorizations and overrides
// ---------------------------------------------------------------------------

fn money_columns(money: Money) -> RepositoryResult<(i64, String)> {
    let minor = i64::try_from(money.minor_units).map_err(|_| RepositoryError::Backend {
        detail: "monetary amount exceeds the storable range".to_owned(),
    })?;
    Ok((minor, money.currency.as_str().to_owned()))
}

fn budget_columns(
    budget: kontor_core::spec::BudgetBounds,
) -> RepositoryResult<(i64, i64, i64, i64, String)> {
    let (cost, currency) = money_columns(budget.max_cost)?;
    Ok((
        i64::try_from(budget.max_tokens).unwrap_or(i64::MAX),
        i64::try_from(budget.max_commands).unwrap_or(i64::MAX),
        i64::try_from(budget.max_duration_seconds).unwrap_or(i64::MAX),
        cost,
        currency,
    ))
}

pub(crate) fn read_budget(
    tokens: i64,
    commands: i64,
    duration: i64,
    cost: i64,
    currency: &str,
) -> RepositoryResult<kontor_core::spec::BudgetBounds> {
    Ok(kontor_core::spec::BudgetBounds {
        max_tokens: u64::try_from(tokens).unwrap_or_default(),
        max_commands: u64::try_from(commands).unwrap_or_default(),
        max_duration_seconds: u64::try_from(duration).unwrap_or_default(),
        max_cost: Money {
            minor_units: u64::try_from(cost).unwrap_or_default(),
            currency: CurrencyCode::parse(currency)?,
        },
    })
}

fn read_assignment(row: &Row<'_>) -> RepositoryResult<WorkCalendarAssignment> {
    let window_override: Option<String> = row.get(5).map_err(backend)?;
    let retired_at: Option<String> = row.get(8).map_err(backend)?;
    let active: i64 = row.get(6).map_err(backend)?;
    Ok(WorkCalendarAssignment {
        id: WorkCalendarId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        profile_id: CalendarProfileId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        profile_version: read_version(row.get::<_, i64>(3).map_err(backend)?)?,
        timezone: IanaTimeZone::parse(&row.get::<_, String>(4).map_err(backend)?)?,
        window_override: window_override
            .as_deref()
            .map(from_json::<Vec<WeeklyWindow>>)
            .transpose()?,
        active: active == 1,
        created_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
        retired_at: retired_at.as_deref().map(read_timestamp).transpose()?,
    })
}

const ASSIGNMENT_COLUMNS: &str = "id, project_id, profile_id, profile_version, timezone, \
     window_override, active, created_at, retired_at";

impl CalendarRepository for SqliteStore {
    fn assign_calendar(&self, assignment: &WorkCalendarAssignment) -> RepositoryResult<()> {
        assignment.validate()?;
        let window_override = assignment
            .window_override
            .as_ref()
            .map(to_json)
            .transpose()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "UPDATE work_calendars SET active = 0, retired_at = ?1
                 WHERE project_id = ?2 AND active = 1",
                params![
                    text(assignment.created_at),
                    assignment.project_id.to_string()
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO work_calendars
                     (id, project_id, profile_id, profile_version, timezone, window_override,
                      active, created_at, retired_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    assignment.id.to_string(),
                    assignment.project_id.to_string(),
                    assignment.profile_id.to_string(),
                    version_column(assignment.profile_version),
                    assignment.timezone.as_str(),
                    window_override,
                    i64::from(assignment.active),
                    text(assignment.created_at),
                    assignment.retired_at.map(text)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn retire_calendar(
        &self,
        project_id: ProjectId,
        id: WorkCalendarId,
        retired_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "UPDATE work_calendars SET active = 0, retired_at = ?1
                 WHERE project_id = ?2 AND id = ?3 AND active = 1",
                params![text(retired_at), project_id.to_string(), id.to_string()],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn active_assignment(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<WorkCalendarAssignment>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {ASSIGNMENT_COLUMNS} FROM work_calendars
                     WHERE project_id = ?1 AND active = 1"
                ),
                params![project_id.to_string()],
                |row| Ok(read_assignment(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn append_child_windows(&self, revision: &ChildCalendarWindows) -> RepositoryResult<()> {
        revision.validate()?;
        let (scope_kind, mini_project, task) = scope_columns(revision.scope);
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO child_calendar_windows
                     (project_id, work_calendar_id, scope_kind, mini_project_id, task_id,
                      version, windows, supersedes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    revision.project_id.to_string(),
                    revision.work_calendar_id.to_string(),
                    scope_kind,
                    mini_project,
                    task,
                    version_column(revision.version),
                    to_json(&revision.windows)?,
                    revision.supersedes.map(version_column),
                    text(revision.created_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn active_child_windows(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
        scope: WorkScope,
    ) -> RepositoryResult<Option<ChildCalendarWindows>> {
        if scope == WorkScope::Project {
            return Err(DomainError::invalid(
                "ChildCalendarWindows",
                "must name a mini-project or task scope",
            )
            .into());
        }
        let (scope_kind, mini_project, task) = scope_columns(scope);
        let row: Option<RepositoryResult<ChildCalendarWindows>> = self
            .connection
            .query_row(
                "SELECT version, windows, supersedes, created_at
                   FROM child_calendar_windows AS current
                  WHERE project_id = ?1 AND work_calendar_id = ?2 AND scope_kind = ?3
                    AND mini_project_id IS ?4 AND task_id IS ?5
                    AND NOT EXISTS (
                        SELECT 1 FROM child_calendar_windows AS later
                         WHERE later.project_id = current.project_id
                           AND later.work_calendar_id = current.work_calendar_id
                           AND later.scope_kind = current.scope_kind
                           AND later.mini_project_id IS current.mini_project_id
                           AND later.task_id IS current.task_id
                           AND later.supersedes = current.version)",
                params![
                    project_id.to_string(),
                    work_calendar_id.to_string(),
                    scope_kind,
                    mini_project,
                    task,
                ],
                |row| {
                    Ok((|| -> RepositoryResult<ChildCalendarWindows> {
                        let previous: Option<i64> = row.get(2).map_err(backend)?;
                        Ok(ChildCalendarWindows {
                            project_id,
                            work_calendar_id,
                            scope,
                            version: SpecVersion::parse(
                                u32::try_from(row.get::<_, i64>(0).map_err(backend)?)
                                    .unwrap_or_default(),
                            )?,
                            windows: from_json(&row.get::<_, String>(1).map_err(backend)?)?,
                            supersedes: previous
                                .map(|value| {
                                    SpecVersion::parse(u32::try_from(value).unwrap_or_default())
                                })
                                .transpose()?,
                            created_at: read_timestamp(&row.get::<_, String>(3).map_err(backend)?)?,
                        })
                    })())
                },
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }

    fn append_exception(&self, exception: &CalendarExceptionRevision) -> RepositoryResult<()> {
        exception.validate()?;
        let provenance = to_json(&exception.provenance)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO calendar_exceptions
                     (id, project_id, work_calendar_id, start_date, end_date, kind, label,
                      provenance, supersedes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    exception.id.to_string(),
                    exception.project_id.to_string(),
                    exception.work_calendar_id.to_string(),
                    exception.start_date.to_string(),
                    exception.end_date.to_string(),
                    exception.kind.as_str(),
                    exception.label.as_str(),
                    provenance,
                    exception.supersedes.map(|id| id.to_string()),
                    text(exception.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn list_exceptions(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Vec<CalendarExceptionRevision>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, start_date, end_date, kind, label, provenance, supersedes, created_at
                 FROM calendar_exceptions
                 WHERE project_id = ?1 AND work_calendar_id = ?2
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                work_calendar_id.to_string()
            ])
            .map_err(backend)?;
        let mut exceptions = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let supersedes: Option<String> = row.get(6).map_err(backend)?;
            exceptions.push(CalendarExceptionRevision {
                id: CalendarExceptionId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                project_id,
                work_calendar_id,
                start_date: row
                    .get::<_, String>(1)
                    .map_err(backend)?
                    .parse()
                    .map_err(|_| RepositoryError::Backend {
                        detail: "stored calendar date is not a civil date".to_owned(),
                    })?,
                end_date: row
                    .get::<_, String>(2)
                    .map_err(backend)?
                    .parse()
                    .map_err(|_| RepositoryError::Backend {
                        detail: "stored calendar date is not a civil date".to_owned(),
                    })?,
                kind: ExceptionKind::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                label: ExternalName::parse(&row.get::<_, String>(4).map_err(backend)?)?,
                provenance: from_json::<ExceptionProvenance>(
                    &row.get::<_, String>(5).map_err(backend)?,
                )?,
                supersedes: supersedes
                    .as_deref()
                    .map(CalendarExceptionId::parse)
                    .transpose()?,
                created_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
            });
        }
        Ok(exceptions)
    }

    fn get_exception(
        &self,
        project_id: ProjectId,
        id: CalendarExceptionId,
    ) -> RepositoryResult<Option<CalendarExceptionRevision>> {
        let calendar: Option<String> = self
            .connection
            .query_row(
                "SELECT work_calendar_id FROM calendar_exceptions
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(calendar) = calendar else {
            return Ok(None);
        };
        let calendar = WorkCalendarId::parse(&calendar)?;
        Ok(self
            .list_exceptions(project_id, calendar)?
            .into_iter()
            .find(|exception| exception.id == id))
    }

    fn insert_holiday_source(&self, revision: &HolidaySourceRevision) -> RepositoryResult<()> {
        revision.validate()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO holiday_sources
                     (id, profile_id, profile_version, provider, country, subdivision,
                      reference, range_start, range_end, retrieved_at, raw_hash,
                      normalized_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    revision.id.to_string(),
                    revision.profile_id.to_string(),
                    version_column(revision.profile_version),
                    provider_column(revision.provider),
                    revision.country.as_str(),
                    revision.subdivision.as_ref().map(ExternalName::as_str),
                    revision.reference.as_str(),
                    revision.range_start.to_string(),
                    revision.range_end.to_string(),
                    text(revision.retrieved_at),
                    revision.raw_hash.as_str(),
                    revision.normalized_hash.as_str()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn insert_authorization(&self, authorization: &ExecutionAuthorization) -> RepositoryResult<()> {
        authorization.validate()?;
        let (kind, mini_project, task) = scope_columns(authorization.scope);
        let selected = to_json(&authorization.selected_tasks)?;
        let (tokens, commands, duration, cost, currency) = budget_columns(authorization.budget)?;
        let transaction = self.begin()?;
        // The capability receipt must be a receipt that actually grants this
        // capability over this scope. Existing in the project is not consent.
        ensure_receipt_authorizes(
            &transaction,
            "ExecutionAuthorization",
            authorization.project_id,
            authorization.capability_receipt,
            CommandKind::AuthorizeExecution,
            authorization.scope.aggregate(authorization.project_id),
        )?;
        // Every selected task must lie inside the authorization's own scope. A
        // task-scoped authorization may only arm that task; a goal-scoped one
        // may only arm tasks that belong to that goal — which is a fact about
        // the task row, not about the scope value, so it is read here in the
        // same transaction rather than assumed.
        for task in &authorization.selected_tasks {
            let inside = match authorization.scope {
                // The composite foreign key on the child rows already proves
                // every selected task belongs to this project.
                WorkScope::Project => true,
                WorkScope::Task { task_id } => task_id == *task,
                WorkScope::MiniProject { mini_project_id } => {
                    let owner: Option<Option<String>> = transaction
                        .query_row(
                            "SELECT mini_project_id FROM tasks WHERE project_id = ?1 AND id = ?2",
                            params![authorization.project_id.to_string(), task.to_string()],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(backend)?;
                    owner.flatten().as_deref() == Some(mini_project_id.to_string().as_str())
                }
            };
            if !inside {
                return Err(DomainError::invalid(
                    "ExecutionAuthorization",
                    "a selected task lies outside the authorization scope",
                )
                .into());
            }
        }
        transaction
            .execute(
                "INSERT INTO execution_authorizations
                     (id, project_id, scope_kind, scope_mini_project_id, scope_task_id,
                      selected_tasks, allowed_start, allowed_end, max_concurrency, max_tokens,
                      max_commands, max_duration_seconds, max_cost_minor_units, cost_currency,
                      created_by, capability_receipt_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17)",
                params![
                    authorization.id.to_string(),
                    authorization.project_id.to_string(),
                    kind,
                    mini_project,
                    task,
                    selected,
                    text(authorization.allowed_start.start),
                    text(authorization.allowed_start.end),
                    i64::from(authorization.max_concurrency),
                    tokens,
                    commands,
                    duration,
                    cost,
                    currency,
                    authorization.created_by.to_string(),
                    authorization.capability_receipt.to_string(),
                    text(authorization.created_at)
                ],
            )
            .map_err(backend)?;

        // The child set must equal the canonical value exactly, so it is written
        // from that value and nowhere else.
        for task in &authorization.selected_tasks {
            transaction
                .execute(
                    "INSERT INTO execution_authorization_tasks
                         (project_id, authorization_id, task_id)
                     VALUES (?1, ?2, ?3)",
                    params![
                        authorization.project_id.to_string(),
                        authorization.id.to_string(),
                        task.to_string()
                    ],
                )
                .map_err(backend)?;
        }
        // Re-read it and prove the agreement rather than assuming it.
        let stored: i64 = transaction
            .query_row(
                "SELECT count(*) FROM execution_authorization_tasks
                 WHERE project_id = ?1 AND authorization_id = ?2",
                params![
                    authorization.project_id.to_string(),
                    authorization.id.to_string()
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let expected: BTreeSet<TaskId> = authorization.selected_tasks.iter().copied().collect();
        if usize::try_from(stored).unwrap_or_default() != expected.len() {
            return Err(DomainError::invalid(
                "ExecutionAuthorization",
                "the stored task set does not match the canonical value",
            )
            .into());
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn insert_override(&self, schedule_override: &ScheduleOverride) -> RepositoryResult<()> {
        schedule_override.validate()?;
        let (kind, mini_project, task) = scope_columns(schedule_override.scope);
        let (expiry_kind, expiry_at, expiry_goal) = match schedule_override.expiry {
            OverrideExpiry::FixedAt { at } => ("fixed_at", Some(text(at)), None),
            OverrideExpiry::GoalBound { mini_project_id } => {
                ("goal_bound", None, Some(mini_project_id.to_string()))
            }
        };
        let (tokens, commands, duration, cost, currency) =
            budget_columns(schedule_override.budget)?;
        let transaction = self.begin()?;
        // An override is only approved if an approval receipt says so, over
        // this exact scope.
        ensure_receipt_authorizes(
            &transaction,
            "ScheduleOverride",
            schedule_override.project_id,
            schedule_override.approval_receipt,
            CommandKind::ApproveScheduleOverride,
            schedule_override
                .scope
                .aggregate(schedule_override.project_id),
        )?;
        transaction
            .execute(
                "INSERT INTO schedule_overrides
                     (id, project_id, scope_kind, scope_mini_project_id, scope_task_id, reason,
                      start_at, expiry_kind, expiry_at, expiry_mini_project_id, hard_ceiling,
                      max_concurrency, max_tokens, max_commands, max_duration_seconds,
                      max_cost_minor_units, cost_currency, approved_by, approval_receipt_id,
                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20)",
                params![
                    schedule_override.id.to_string(),
                    schedule_override.project_id.to_string(),
                    kind,
                    mini_project,
                    task,
                    schedule_override.reason.as_str(),
                    text(schedule_override.start),
                    expiry_kind,
                    expiry_at,
                    expiry_goal,
                    text(schedule_override.hard_ceiling),
                    i64::from(schedule_override.max_concurrency),
                    tokens,
                    commands,
                    duration,
                    cost,
                    currency,
                    schedule_override.approved_by.to_string(),
                    schedule_override.approval_receipt.to_string(),
                    text(schedule_override.start)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn revoke_override(
        &self,
        project_id: ProjectId,
        id: ScheduleOverrideId,
        revocation: &OverrideRevocation,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        // The revocation receipt has to match the scope of the override it
        // revokes, so the scope is read from the stored row — the caller only
        // supplies an id. A live override is required for the read to succeed,
        // which is the same condition the update below enforces.
        let scope: Option<RepositoryResult<WorkScope>> = transaction
            .query_row(
                "SELECT scope_kind, scope_mini_project_id, scope_task_id
                 FROM schedule_overrides
                 WHERE project_id = ?1 AND id = ?2 AND revoked_at IS NULL",
                params![project_id.to_string(), id.to_string()],
                |row| {
                    let build = || -> RepositoryResult<WorkScope> {
                        read_scope(
                            &row.get::<_, String>(0).map_err(backend)?,
                            row.get(1).map_err(backend)?,
                            row.get(2).map_err(backend)?,
                        )
                    };
                    Ok(build())
                },
            )
            .optional()
            .map_err(backend)?;
        let Some(scope) = scope else {
            return Err(conflict(
                "schedule override",
                "the override is unknown or already revoked",
            ));
        };
        // A revocation is its own command: an approval receipt is not
        // permission to undo the thing it approved.
        ensure_receipt_authorizes(
            &transaction,
            "OverrideRevocation",
            project_id,
            revocation.receipt,
            CommandKind::RevokeScheduleOverride,
            scope?.aggregate(project_id),
        )?;
        let changed = transaction
            .execute(
                "UPDATE schedule_overrides
                 SET revoked_at = ?1, revoked_by = ?2, revocation_receipt_id = ?3
                 WHERE project_id = ?4 AND id = ?5 AND revoked_at IS NULL",
                params![
                    text(revocation.revoked_at),
                    revocation.revoked_by.to_string(),
                    revocation.receipt.to_string(),
                    project_id.to_string(),
                    id.to_string()
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "schedule override",
                "the override is unknown or already revoked",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn get_override(
        &self,
        project_id: ProjectId,
        id: ScheduleOverrideId,
    ) -> RepositoryResult<Option<ScheduleOverride>> {
        let row: Option<RepositoryResult<ScheduleOverride>> = self
            .connection
            .query_row(
                "SELECT scope_kind, scope_mini_project_id, scope_task_id, reason, start_at,
                        expiry_kind, expiry_at, expiry_mini_project_id, hard_ceiling,
                        max_concurrency, max_tokens, max_commands, max_duration_seconds,
                        max_cost_minor_units, cost_currency, approved_by, approval_receipt_id,
                        revoked_at, revoked_by, revocation_receipt_id
                 FROM schedule_overrides WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| {
                    let build = || -> RepositoryResult<ScheduleOverride> {
                        let scope = read_scope(
                            &row.get::<_, String>(0).map_err(backend)?,
                            row.get(1).map_err(backend)?,
                            row.get(2).map_err(backend)?,
                        )?;
                        let expiry_kind: String = row.get(5).map_err(backend)?;
                        let expiry_at: Option<String> = row.get(6).map_err(backend)?;
                        let expiry_goal: Option<String> = row.get(7).map_err(backend)?;
                        let expiry = match expiry_kind.as_str() {
                            "fixed_at" => OverrideExpiry::FixedAt {
                                at: read_timestamp(expiry_at.as_deref().unwrap_or_default())?,
                            },
                            _ => OverrideExpiry::GoalBound {
                                mini_project_id: MiniProjectId::parse(
                                    expiry_goal.as_deref().unwrap_or_default(),
                                )?,
                            },
                        };
                        let revoked_at: Option<String> = row.get(17).map_err(backend)?;
                        let revoked_by: Option<String> = row.get(18).map_err(backend)?;
                        let revocation_receipt: Option<String> = row.get(19).map_err(backend)?;
                        let revocations = match (revoked_at, revoked_by, revocation_receipt) {
                            (Some(at), Some(by), Some(receipt)) => vec![OverrideRevocation {
                                revoked_at: read_timestamp(&at)?,
                                revoked_by: AccountProfileId::parse(&by)?,
                                receipt: CommandReceiptId::parse(&receipt)?,
                            }],
                            _ => Vec::new(),
                        };
                        Ok(ScheduleOverride {
                            id,
                            project_id,
                            scope,
                            reason: ExternalName::parse(
                                &row.get::<_, String>(3).map_err(backend)?,
                            )?,
                            start: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
                            expiry,
                            hard_ceiling: read_timestamp(
                                &row.get::<_, String>(8).map_err(backend)?,
                            )?,
                            max_concurrency: u32::try_from(row.get::<_, i64>(9).map_err(backend)?)
                                .unwrap_or(u32::MAX),
                            budget: read_budget(
                                row.get(10).map_err(backend)?,
                                row.get(11).map_err(backend)?,
                                row.get(12).map_err(backend)?,
                                row.get(13).map_err(backend)?,
                                &row.get::<_, String>(14).map_err(backend)?,
                            )?,
                            approved_by: AccountProfileId::parse(
                                &row.get::<_, String>(15).map_err(backend)?,
                            )?,
                            approval_receipt: CommandReceiptId::parse(
                                &row.get::<_, String>(16).map_err(backend)?,
                            )?,
                            revocations,
                        })
                    };
                    Ok(build())
                },
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }

    fn apply_holiday_import(
        &self,
        batch: &HolidayImportBatch,
        revision: &HolidaySourceRevision,
        exceptions: &[CalendarExceptionRevision],
    ) -> RepositoryResult<HolidayImportBatch> {
        batch.validate()?;
        revision.validate()?;
        if batch.source_id != revision.id {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "the provenance and the source revision name different revisions",
            )
            .into());
        }
        if usize::try_from(batch.applied_exceptions).unwrap_or(usize::MAX) != exceptions.len() {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "the recorded exception count is not the number of exceptions applied",
            )
            .into());
        }
        for exception in exceptions {
            exception.validate()?;
            if exception.project_id != batch.project_id
                || exception.work_calendar_id != batch.work_calendar_id
            {
                return Err(RepositoryError::CrossProject {
                    subject: "imported calendar exception",
                });
            }
            // Every exception this import writes must cite *this* revision. An
            // import that wrote an exception attributed to another source would
            // be attributing its closures to provenance it did not retrieve.
            if exception.provenance
                != (ExceptionProvenance::HolidaySource {
                    source_id: revision.id,
                })
            {
                return Err(DomainError::invalid(
                    "imported calendar exception",
                    "must cite the source revision this import applied",
                )
                .into());
            }
        }

        // Replay first, and outside the write path: the same key for the same
        // calendar returns what the original apply wrote and touches nothing.
        if let Some(original) = self.import_by_key(
            batch.project_id,
            batch.work_calendar_id,
            &batch.idempotency_key,
        )? {
            return Ok(original);
        }

        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO holiday_sources
                     (id, profile_id, profile_version, provider, country, subdivision,
                      reference, range_start, range_end, retrieved_at, raw_hash,
                      normalized_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    revision.id.to_string(),
                    revision.profile_id.to_string(),
                    version_column(revision.profile_version),
                    provider_column(revision.provider),
                    revision.country.as_str(),
                    revision.subdivision.as_ref().map(ExternalName::as_str),
                    revision.reference.as_str(),
                    revision.range_start.to_string(),
                    revision.range_end.to_string(),
                    text(revision.retrieved_at),
                    revision.raw_hash.as_str(),
                    revision.normalized_hash.as_str()
                ],
            )
            .map_err(backend)?;
        for exception in exceptions {
            transaction
                .execute(
                    "INSERT INTO calendar_exceptions
                         (id, project_id, work_calendar_id, start_date, end_date, kind, label,
                          provenance, supersedes, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        exception.id.to_string(),
                        exception.project_id.to_string(),
                        exception.work_calendar_id.to_string(),
                        exception.start_date.to_string(),
                        exception.end_date.to_string(),
                        exception.kind.as_str(),
                        exception.label.as_str(),
                        to_json(&exception.provenance)?,
                        exception.supersedes.map(|id| id.to_string()),
                        text(exception.created_at)
                    ],
                )
                .map_err(backend)?;
        }
        transaction
            .execute(
                "INSERT INTO holiday_import_batches
                     (source_id, project_id, work_calendar_id, import_kind, requested_start,
                      requested_end, categories, warnings, applied_exceptions, supersedes,
                      idempotency_key, applied_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    batch.source_id.to_string(),
                    batch.project_id.to_string(),
                    batch.work_calendar_id.to_string(),
                    batch.kind.as_str(),
                    batch.requested_start.to_string(),
                    batch.requested_end.to_string(),
                    to_json(&batch.categories)?,
                    to_json(&batch.warnings)?,
                    i64::from(batch.applied_exceptions),
                    batch.supersedes.map(|id| id.to_string()),
                    batch.idempotency_key.as_str(),
                    text(batch.applied_at)
                ],
            )
            .map_err(|error| {
                // The supersession trigger is the one refusal here that is a
                // caller's mistake rather than a backend failure: it fires when a
                // second import tries to become current without replacing the
                // import that already is.
                if matches!(
                    &error,
                    rusqlite::Error::SqliteFailure(failure, _)
                        if failure.code == rusqlite::ErrorCode::ConstraintViolation
                ) {
                    conflict(
                        "holiday import",
                        "an import must supersede the calendar's current import",
                    )
                } else {
                    backend(error)
                }
            })?;
        transaction.commit().map_err(backend)?;
        Ok(batch.clone())
    }

    fn applied_import(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Option<HolidayImportBatch>> {
        let row: Option<RepositoryResult<HolidayImportBatch>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {IMPORT_BATCH_COLUMNS} FROM holiday_import_batches AS current
                      WHERE current.project_id = ?1 AND current.work_calendar_id = ?2
                        AND NOT EXISTS (SELECT 1 FROM holiday_import_batches AS later
                                         WHERE later.supersedes = current.source_id)"
                ),
                params![project_id.to_string(), work_calendar_id.to_string()],
                |row| Ok(read_import_batch(row)),
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }

    fn applied_exceptions(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Vec<CalendarExceptionRevision>> {
        let applied = self
            .applied_import(project_id, work_calendar_id)?
            .map(|batch| batch.source_id);
        Ok(self
            .list_exceptions(project_id, work_calendar_id)?
            .into_iter()
            .filter(|exception| match exception.provenance {
                // A human's revision is never dropped by an import refresh.
                ExceptionProvenance::Manual { .. } => true,
                // A superseded import's rows stay in the table as history and
                // stop being policy the moment a newer import replaces them.
                ExceptionProvenance::HolidaySource { source_id } => Some(source_id) == applied,
            })
            .collect())
    }
}

/// The v1 provider spelling for a holiday source.
///
/// v1 has three: a retrieved feed, a human and a shipped set. Which *importer*
/// read a retrieved feed — iCalendar, Nager or GOV.UK — is
/// [`kontor_core::calendar::HolidayImportKind`] on the import batch, because
/// SQLite cannot widen the v1 `CHECK` this column carries.
fn provider_column(provider: HolidayProviderKind) -> &'static str {
    match provider {
        HolidayProviderKind::Ical => "ical",
        HolidayProviderKind::Manual => "manual",
        HolidayProviderKind::Bundled => "bundled",
    }
}

const IMPORT_BATCH_COLUMNS: &str = "source_id, project_id, work_calendar_id, import_kind, \
     requested_start, requested_end, categories, warnings, applied_exceptions, supersedes, \
     idempotency_key, applied_at";

fn read_import_batch(row: &Row<'_>) -> RepositoryResult<HolidayImportBatch> {
    let supersedes: Option<String> = row.get(9).map_err(backend)?;
    Ok(HolidayImportBatch {
        source_id: HolidaySourceId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        work_calendar_id: WorkCalendarId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        kind: HolidayImportKind::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        requested_start: read_civil_date(&row.get::<_, String>(4).map_err(backend)?)?,
        requested_end: read_civil_date(&row.get::<_, String>(5).map_err(backend)?)?,
        categories: from_json(&row.get::<_, String>(6).map_err(backend)?)?,
        warnings: from_json(&row.get::<_, String>(7).map_err(backend)?)?,
        applied_exceptions: u32::try_from(row.get::<_, i64>(8).map_err(backend)?).map_err(
            |_| RepositoryError::Backend {
                detail: "stored import exception count is out of range".to_owned(),
            },
        )?,
        supersedes: supersedes
            .as_deref()
            .map(HolidaySourceId::parse)
            .transpose()?,
        idempotency_key: IdempotencyKey::parse(&row.get::<_, String>(10).map_err(backend)?)?,
        applied_at: read_timestamp(&row.get::<_, String>(11).map_err(backend)?)?,
    })
}

/// Parse a stored `YYYY-MM-DD`, whichever civil-date type the field expects.
fn read_civil_date<T: std::str::FromStr>(value: &str) -> RepositoryResult<T> {
    value.parse().map_err(|_| RepositoryError::Backend {
        detail: "stored calendar date is not a civil date".to_owned(),
    })
}

impl SqliteStore {
    /// The import a calendar already applied under one idempotency key.
    fn import_by_key(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
        key: &IdempotencyKey,
    ) -> RepositoryResult<Option<HolidayImportBatch>> {
        let row: Option<RepositoryResult<HolidayImportBatch>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {IMPORT_BATCH_COLUMNS} FROM holiday_import_batches
                      WHERE project_id = ?1 AND work_calendar_id = ?2 AND idempotency_key = ?3"
                ),
                params![
                    project_id.to_string(),
                    work_calendar_id.to_string(),
                    key.as_str()
                ],
                |row| Ok(read_import_batch(row)),
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }
}

/// Rebuild a team-run snapshot from a stored row. Kept next to the run
/// repository so the read path and the write path share one shape.
pub(crate) fn team_run_snapshot(json: &str, hash: &str) -> RepositoryResult<TeamRunSnapshot> {
    stored_document(json, hash)
}

/// Re-admit one stored compaction receipt, bytes and digest both re-checked.
pub(crate) fn decode_compaction_receipt(
    json: &str,
    hash: &str,
) -> RepositoryResult<kontor_core::compaction::CompactionReceipt> {
    let receipt: kontor_core::compaction::CompactionReceipt = stored_document(json, hash)?;
    // The document round-tripped; its *claims* are checked separately, so a row
    // written before a rule existed cannot be read back as if it satisfied it.
    receipt.validate()?;
    Ok(receipt)
}

/// Read one run's frozen context-window pair, re-verified end to end.
///
/// The two halves are stored as separate canonical documents, so this rebuilds
/// the snapshot and then asks it to verify itself — which re-derives both hashes
/// from the bytes rather than trusting the columns beside them.
fn read_run_context_policy(
    transaction: &rusqlite::Transaction<'_>,
    agent_run_id: AgentRunId,
) -> RepositoryResult<Option<kontor_core::spec::ContextPolicySnapshot>> {
    let row: Option<(String, String, String, String, String)> = transaction
        .query_row(
            "SELECT requested, requested_hash, effective, effective_hash, resolved_at
             FROM run_context_policies WHERE agent_run_id = ?1",
            params![agent_run_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((requested_json, requested_hash, effective_json, effective_hash, resolved_at)) = row
    else {
        return Ok(None);
    };

    let requested: kontor_core::spec::RequestedContextPolicy =
        stored_document(&requested_json, &requested_hash)?;
    let effective: kontor_core::spec::EffectiveContextPolicy =
        stored_document(&effective_json, &effective_hash)?;
    let snapshot = kontor_core::spec::ContextPolicySnapshot {
        schema_version: requested.schema_version,
        requested,
        requested_hash: ContentHash::parse(&requested_hash)?,
        effective,
        effective_hash: ContentHash::parse(&effective_hash)?,
        resolved_at: parse_utc_timestamp(&resolved_at)?,
    };
    snapshot.verify()?;
    Ok(Some(snapshot))
}

/// The most recent recorded compaction attempt for one run.
fn read_latest_compaction_receipt(
    transaction: &rusqlite::Transaction<'_>,
    agent_run_id: AgentRunId,
) -> RepositoryResult<Option<kontor_core::compaction::CompactionReceipt>> {
    let row: Option<(String, String)> = transaction
        .query_row(
            "SELECT receipt, receipt_hash FROM compaction_receipts
             WHERE agent_run_id = ?1
             ORDER BY recorded_at DESC, id DESC
             LIMIT 1",
            params![agent_run_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    row.map(|(json, hash)| decode_compaction_receipt(&json, &hash))
        .transpose()
}

/// Read one stored compaction receipt by id, with the digest it was stored under.
fn read_compaction_receipt(
    transaction: &rusqlite::Transaction<'_>,
    id: kontor_core::id::CompactionReceiptId,
) -> RepositoryResult<Option<(kontor_core::compaction::CompactionReceipt, ContentHash)>> {
    let row: Option<(String, String)> = transaction
        .query_row(
            "SELECT receipt, receipt_hash FROM compaction_receipts WHERE id = ?1",
            params![id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    row.map(|(json, hash)| {
        let receipt = decode_compaction_receipt(&json, &hash)?;
        Ok((receipt, ContentHash::parse(&hash)?))
    })
    .transpose()
}

/// Prove a terminal task has accounted for the team that did its work.
///
/// The caller's [`TaskTeamClosure`] is a *citation*, not evidence: it names
/// which team run was certified. Everything that matters is re-proved here from
/// the store's own rows — the cited team serves this task, it has closed, and
/// none of its runs is still open — so a fabricated citation buys nothing, in
/// the same way a run closure re-proves the event it cites.
///
/// The one thing only `kontor-teams` can prove is that every *declared* role
/// slot is accounted for, including one that never produced a run. That is what
/// obtaining the certificate required, and it is why the citation is the
/// supported way in.
fn ensure_team_accounted_for(
    transaction: &Transaction<'_>,
    request: &TaskTransitionRequest,
    prescribes_team: bool,
) -> RepositoryResult<()> {
    if !prescribes_team {
        // No pinned team means no role slots to answer for. Claiming otherwise
        // is a confusion worth refusing rather than ignoring.
        if request.team_closure != TaskTeamClosure::NoTeam {
            return Err(DomainError::invalid(
                "task transition",
                "team closure was cited for a task whose profile prescribes no team",
            )
            .into());
        }
        return Ok(());
    }

    let TaskTeamClosure::Certified {
        team_run_id,
        // Load-bearing for a disposition closure, which re-proves the caller is
        // citing the digest this team actually closed with. The other sources
        // prove themselves from rows and ignore it.
        ref policy_digest,
    } = request.team_closure
    else {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "a task whose profile prescribes a team must cite that team's closure",
        }
        .into());
    };

    let cited: Option<(String, String)> = transaction
        .query_row(
            "SELECT task_id, lifecycle FROM team_runs WHERE project_id = ?1 AND id = ?2",
            params![request.project_id.to_string(), team_run_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((task_id, lifecycle)) = cited else {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "the cited team run is not stored in this project",
        }
        .into());
    };
    if TaskId::parse(&task_id)? != request.task_id {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "the cited team run serves a different task",
        }
        .into());
    }
    if !RunLifecycle::parse(&lifecycle)?.is_terminal() {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "the cited team run has not closed",
        }
        .into());
    }

    // How the team closed decides what "accounted for" means, and the two are
    // not interchangeable. A child-evidence closure is proved by every child run
    // having ended. A settled-turn closure cannot be — its children are expected
    // to still be live — so it is proved from this team's own immutable
    // `role_turns` rows instead.
    let source_kind: Option<String> = transaction
        .query_row(
            "SELECT terminal_source_kind FROM team_runs WHERE project_id = ?1 AND id = ?2",
            params![request.project_id.to_string(), team_run_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?
        .flatten();
    if source_kind.as_deref() == Some("settled_turns") {
        return ensure_declared_slots_settled(transaction, request.project_id, team_run_id);
    }
    if source_kind.as_deref() == Some("role_slot_dispositions") {
        return ensure_declared_slots_disposed(
            transaction,
            request.project_id,
            team_run_id,
            policy_digest,
        );
    }

    // The open-slot check, decided from the rows rather than from the citation.
    let children = read_team_child_evidence(transaction, request.project_id, team_run_id)?;
    if children.iter().any(|child| !child.lifecycle.is_terminal()) {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "a role slot of the cited team run is still open",
        }
        .into());
    }
    Ok(())
}

/// Re-prove that every role slot the frozen template declares settled a turn.
///
/// The declared slots come from `team_runs.snapshot` — the template the run was
/// pinned to — so the set being checked is the one the team actually froze, not
/// whatever rows happen to exist. A slot that never ran is therefore still
/// required to be accounted for, which is the property the whole declared-slot
/// walk exists for.
///
/// A live seat is *not* a failure here. That is the entire difference from the
/// child-evidence path: the seat is expected to outlive the turn taken in it.
/// Re-prove a disposition closure: every declared slot carries exactly one
/// source, and the digest they hash to is the one the closure was recorded with
/// *and* the one the caller cited.
///
/// The two digest comparisons are not redundant. `terminal_evidence_hash` is
/// what the team was closed with, so matching it proves the rows still say what
/// they said at closure. `TaskTeamClosure::Certified.policy_digest` is what the
/// *caller* is citing right now, so matching it proves the caller is closing the
/// task against the team it thinks it is. Checking only the first would let a
/// caller cite any digest at all; checking only the second would let the rows
/// drift away from the closure.
fn ensure_declared_slots_disposed(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    team_run_id: TeamRunId,
    cited_digest: &ContentHash,
) -> RepositoryResult<()> {
    use crate::query::column_text;
    use kontor_core::state::SlotDisposition;

    let (snapshot, recorded): (String, Option<String>) = transaction
        .query_row(
            "SELECT snapshot, terminal_evidence_hash FROM team_runs
             WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), team_run_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(backend)?;
    let snapshot: TeamRunSnapshot = from_json(&snapshot)?;
    let declared = snapshot.ordered_role_slots()?;

    // The *final* turn per slot, which is the one the digest is taken over: a
    // slot may take many bounded turns and only the last one accounts for it.
    let mut settled: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut statement = transaction
        .prepare(
            "SELECT role_slot_id, evidence_hash FROM role_turns
             WHERE project_id = ?1 AND team_run_id = ?2
             ORDER BY role_slot_id, turn_ordinal",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), team_run_id.to_string()])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        settled.insert(column_text(row, 0)?, column_text(row, 1)?);
    }

    let mut waived: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut statement = transaction
        .prepare(
            "SELECT role_slot_id, evidence_hash FROM role_slot_waivers
             WHERE project_id = ?1 AND team_run_id = ?2",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), team_run_id.to_string()])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        waived.insert(column_text(row, 0)?, column_text(row, 1)?);
    }

    let mut dispositions: Vec<(kontor_core::id::RoleSlotId, SlotDisposition)> = Vec::new();
    for slot in &declared {
        let key = slot.as_role_key().as_str();
        match (settled.get(key), waived.get(key)) {
            (Some(_), Some(_)) => {
                return Err(DomainError::invalid(
                    "task transition",
                    "a declared role slot both settled a turn and was waived",
                )
                .into());
            }
            (None, None) => {
                return Err(DomainError::MissingEvidence {
                    subject: "task transition",
                    rule: "a declared role slot of the cited team run is neither settled nor waived",
                }
                .into());
            }
            (Some(hash), None) => dispositions.push((
                slot.clone(),
                SlotDisposition::SettledTurn {
                    evidence_hash: ContentHash::parse(hash)?,
                },
            )),
            (None, Some(hash)) => dispositions.push((
                slot.clone(),
                SlotDisposition::WaivedUnbound {
                    evidence_hash: ContentHash::parse(hash)?,
                },
            )),
        }
    }

    let digest = kontor_core::state::role_slot_disposition_digest(
        kontor_core::id::SCHEMA_VERSION,
        team_run_id,
        snapshot.template_id,
        snapshot.template_version,
        snapshot.definition.hash(),
        &dispositions,
    )?;
    if recorded.as_deref() != Some(digest.as_str()) {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "the cited team run's dispositions no longer hash to its closure evidence",
        }
        .into());
    }
    if cited_digest != &digest {
        return Err(DomainError::MissingEvidence {
            subject: "task transition",
            rule: "the cited policy digest is not the one this team run closed with",
        }
        .into());
    }
    Ok(())
}

fn ensure_declared_slots_settled(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    team_run_id: TeamRunId,
) -> RepositoryResult<()> {
    let snapshot: String = transaction
        .query_row(
            "SELECT snapshot FROM team_runs WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), team_run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    let snapshot: TeamRunSnapshot = from_json(&snapshot)?;
    let declared = snapshot.declared_role_slots()?;

    let mut settled = std::collections::BTreeSet::new();
    let mut statement = transaction
        .prepare(
            "SELECT DISTINCT role_slot_id FROM role_turns
             WHERE project_id = ?1 AND team_run_id = ?2",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), team_run_id.to_string()])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        settled.insert(row.get::<_, String>(0).map_err(backend)?);
    }
    for slot in &declared {
        if !settled.contains(slot.as_role_key().as_str()) {
            return Err(DomainError::MissingEvidence {
                subject: "task transition",
                rule: "a declared role slot of the cited team run settled no turn",
            }
            .into());
        }
    }
    Ok(())
}

/// Load a team's own child runs as immutable evidence rows.
///
/// Scoped by project *and* team, so a globally valid run id belonging to another
/// team or project is simply not in the result set.
fn read_team_child_evidence(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    team_run_id: TeamRunId,
) -> RepositoryResult<Vec<TeamChildEvidence>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, lifecycle, terminal_evidence_hash FROM agent_runs
             WHERE project_id = ?1 AND team_run_id = ?2 ORDER BY id",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), team_run_id.to_string()])
        .map_err(backend)?;
    let mut children = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        children.push(TeamChildEvidence {
            agent_run_id: AgentRunId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
            lifecycle: RunLifecycle::parse(&row.get::<_, String>(1).map_err(backend)?)?,
            evidence_hash: row
                .get::<_, Option<String>>(2)
                .map_err(backend)?
                .as_deref()
                .map(ContentHash::parse)
                .transpose()?,
        });
    }
    Ok(children)
}

/// Load the facts an operator-abandon closure is proved against.
///
/// Only *facts* are returned; whether they authorize the closure is decided by
/// [`AbandonReceiptFacts::verify`] in the domain, so the agent and team paths
/// cannot drift apart.
pub(crate) fn read_abandon_receipt(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
    expected_target: &AggregateRef,
) -> RepositoryResult<AbandonReceiptFacts> {
    let found: Option<(String, String, String, i64, String)> = transaction
        .query_row(
            "SELECT kind, intent_hash, target, target_revision, created_at FROM command_receipts
             WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), receipt_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((kind, intent_hash, target, target_revision, created_at)) = found else {
        return Err(RepositoryError::NotFound {
            subject: "abandon receipt",
        });
    };
    let target: AggregateRef = from_json(&target)?;
    Ok(AbandonReceiptFacts {
        kind_is_abandon: kind == CommandKind::AbandonRun.as_str(),
        targets_aggregate: &target == expected_target,
        target_revision: revision_of(target_revision)?,
        intent_hash: ContentHash::parse(&intent_hash)?,
        recorded_at: read_timestamp(&created_at)?,
    })
}

/// Prove a cited receipt actually authorizes `kind` against `target`, inside
/// the transaction that is about to consume it.
///
/// The foreign key already proves the receipt exists in this project. It says
/// nothing about *what the receipt is for*, and a receipt for one command
/// against one aggregate is not permission to do a different thing elsewhere.
/// The check re-reads the stored row rather than trusting anything the caller
/// passed alongside the id.
pub(crate) fn ensure_receipt_authorizes(
    transaction: &Transaction<'_>,
    subject: &'static str,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
    kind: CommandKind,
    target: AggregateRef,
) -> RepositoryResult<()> {
    let found: Option<(String, String, String)> = transaction
        .query_row(
            "SELECT project_id, kind, target FROM command_receipts WHERE id = ?1",
            params![receipt_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((stored_project, stored_kind, stored_target)) = found else {
        return Err(RepositoryError::NotFound {
            subject: "authorizing receipt",
        });
    };
    let authority = ReceiptAuthority {
        project_id: ProjectId::parse(&stored_project)?,
        kind: CommandKind::parse(&stored_kind)?,
        target: from_json(&stored_target)?,
    };
    authority.authorizes(subject, project_id, kind, target)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Realm ingress
// ---------------------------------------------------------------------------

/// The control-plane positions this Realm still holds, read inside `transaction`.
///
/// `(oldest, newest)`, both clamped to the reserved origin (cursor 1, which names
/// no row) so an empty log answers with a position rather than with nothing.
fn control_window(transaction: &Transaction<'_>) -> RepositoryResult<(EventCursor, EventCursor)> {
    let (oldest, newest): (i64, i64) = transaction
        .query_row(
            "SELECT COALESCE(MIN(cursor), 0), COALESCE(MAX(cursor), 0) FROM runtime_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(backend)?;
    Ok((
        EventCursor::parse(oldest.max(1))?,
        EventCursor::parse(newest.max(1))?,
    ))
}

/// Every discontinuity recorded against one run, oldest first.
///
/// Control and content gaps are read together and stay labelled: a caller is
/// owed both markers, and merging them would turn "refetch this transcript" into
/// "a control fact is missing", which is a different and much stronger claim.
fn read_gaps(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    agent_run_id: AgentRunId,
) -> RepositoryResult<Vec<HistoryGapMarker>> {
    let mut markers = Vec::new();
    let mut control = transaction
        .prepare(
            "SELECT expected_sequence, received_sequence, detected_cursor, detected_at
             FROM runtime_control_gaps
             WHERE project_id = ?1 AND agent_run_id = ?2 ORDER BY detected_cursor",
        )
        .map_err(backend)?;
    let mut rows = control
        .query(params![project_id.to_string(), agent_run_id.to_string()])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        markers.push(HistoryGapMarker {
            kind: HistoryGapKind::Control,
            content_epoch: None,
            expected_sequence: stored_sequence(row.get(0).map_err(backend)?),
            received_sequence: stored_sequence(row.get(1).map_err(backend)?),
            detected_cursor: EventCursor::parse(row.get(2).map_err(backend)?)?,
            detected_at: read_timestamp(&row.get::<_, String>(3).map_err(backend)?)?,
        });
    }
    drop(rows);

    let mut content = transaction
        .prepare(
            "SELECT content_epoch, expected_content_sequence, received_content_sequence,
                    detected_cursor, detected_at
             FROM runtime_content_gaps
             WHERE project_id = ?1 AND agent_run_id = ?2 ORDER BY detected_cursor",
        )
        .map_err(backend)?;
    let mut rows = content
        .query(params![project_id.to_string(), agent_run_id.to_string()])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        markers.push(HistoryGapMarker {
            kind: HistoryGapKind::Content,
            content_epoch: Some(stored_sequence(row.get(0).map_err(backend)?)),
            expected_sequence: stored_sequence(row.get(1).map_err(backend)?),
            received_sequence: stored_sequence(row.get(2).map_err(backend)?),
            detected_cursor: EventCursor::parse(row.get(3).map_err(backend)?)?,
            detected_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
        });
    }
    markers.sort_by_key(|marker| marker.detected_cursor);
    Ok(markers)
}

/// Read a stored non-negative sequence column back into its domain width.
///
/// Every one of these columns carries `CHECK (… >= 0)`, so a negative value is
/// not a case to interpret; it is a database this binary did not write, and 0 is
/// the honest reading of it rather than a wrapped-around maximum.
fn stored_sequence(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

impl RealmRepository for SqliteStore {
    fn realm(&self) -> RealmId {
        self.realm_id()
    }

    fn record_intent_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewCommandIntent>,
    ) -> RepositoryResult<CommandReceipt> {
        // The Realm is proved before a transaction opens, so a foreign envelope
        // never reaches SQL at all.
        let request = envelope.peek(self.realm_id())?;
        self.record_intent(request)
    }

    fn record_observation_in_realm(
        &self,
        envelope: &EventEnvelope<NewObservation>,
    ) -> RepositoryResult<RunProjection> {
        let request = envelope.peek(self.realm_id())?;
        self.record_observation(request)
    }

    fn record_source_event_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewSourceEvent>,
    ) -> RepositoryResult<IntakeOutcome> {
        let request = envelope.peek(self.realm_id())?;
        self.record_source_event(request)
    }

    fn reevaluate_source_event_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewIntakeReevaluation>,
    ) -> RepositoryResult<ReevaluationOutcome> {
        let request = envelope.peek(self.realm_id())?;
        self.reevaluate_source_event(request)
    }

    fn import_receipt_in_realm(
        &self,
        envelope: &ReceiptEnvelope<CommandReceipt>,
    ) -> RepositoryResult<CommandReceipt> {
        let presented = envelope.peek(self.realm_id())?;
        // A receipt is *found*, never re-created: importing one is a lookup of
        // something this Realm already minted. An id minted elsewhere simply has
        // no row here, which is the isolation argument working as intended.
        let stored = self.get_receipt_by_key(&presented.idempotency_key)?.ok_or(
            RepositoryError::NotFound {
                subject: "command receipt",
            },
        )?;
        if stored.id != presented.id
            || stored.project_id != presented.project_id
            || stored.kind != presented.kind
            || stored.target != presented.target
            || stored.intent.hash() != presented.intent.hash()
        {
            return Err(DomainError::invalid(
                "CommandReceipt",
                "an idempotency key may not be reused for a different command",
            )
            .into());
        }
        Ok(stored)
    }

    fn read_events_after(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        after: Option<RealmCursor>,
    ) -> RepositoryResult<Vec<EventEnvelope<RuntimeEvent>>> {
        let realm = self.realm_id();
        // A cursor from another Realm counts in a different space entirely;
        // resolving it here is the whole point of the qualified pair.
        let resolved = after.map(|cursor| cursor.resolve(realm)).transpose()?;
        let events = self.read_runtime_events(project_id, agent_run_id, resolved)?;
        Ok(events
            .into_iter()
            .map(|event| EventEnvelope::new(realm, event.cursor, event))
            .collect())
    }

    fn snapshot_agent_run(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AgentRun>>> {
        let transaction = self.begin()?;
        let run = read_agent_run(&transaction, project_id, agent_run_id)?;
        // The snapshot is taken with the highest allocated cursor so a
        // subscriber can resume strictly after it.
        let highest: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(cursor), 0) FROM runtime_events WHERE project_id = ?1",
                params![project_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let cursor = EventCursor::parse(highest.max(1))?;
        Ok(SnapshotEnvelope::new(self.realm_id(), cursor, run))
    }

    fn snapshot_account_profile(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AccountProfile>>> {
        let transaction = self.begin()?;
        let profile = read_account_profile_in(&transaction, project_id, id)?;
        let highest: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(cursor), 0) FROM runtime_events WHERE project_id = ?1",
                params![project_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let cursor = EventCursor::parse(highest.max(1))?;
        Ok(SnapshotEnvelope::new(self.realm_id(), cursor, profile))
    }

    fn update_account_profile_in_realm(
        &self,
        envelope: &ReceiptEnvelope<AccountProfileUpdate>,
    ) -> RepositoryResult<AccountProfile> {
        // The Realm is proved before `update_account_profile` opens its
        // transaction, so a foreign envelope never reaches a `WHERE` clause.
        let request = envelope.peek(self.realm_id())?;
        self.update_account_profile(request)
    }

    fn realm_event_page(
        &self,
        after: Option<RealmCursor>,
        limit: u32,
    ) -> RepositoryResult<RealmEventPage> {
        let realm = self.realm_id();
        let resolved = after.map(|cursor| cursor.resolve(realm)).transpose()?;
        let limit = crate::events::types::page_limit(limit)?;
        // The page and the window are read in one transaction: a caller decides
        // "caught up" versus "that position no longer exists" from them together,
        // and two reads could disagree about a commit that landed between them.
        let transaction = self.begin()?;
        let (oldest_retained, newest) = control_window(&transaction)?;
        // Only the kinds a `RuntimeEvent` can express are delivered. A command
        // intent row carries no native identity and an orphan census row carries
        // no run, so neither can be reconstructed into one — and inventing a
        // placeholder identity to fit them into the shape would be a lie in a
        // durable feed.
        let mut statement = transaction
            .prepare(&format!(
                "SELECT {EVENT_COLUMNS} FROM runtime_events
                 WHERE event_kind IN ('runtime_observation', 'census_observation')
                   AND agent_run_id IS NOT NULL
                   AND cursor > ?1
                 ORDER BY cursor LIMIT ?2"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![resolved.map_or(0, EventCursor::get), limit])
            .map_err(backend)?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let event = read_event(row)?;
            events.push(EventEnvelope::new(realm, event.cursor, event));
        }
        Ok(RealmEventPage {
            events,
            oldest_retained: RealmCursor::new(realm, oldest_retained),
            newest: RealmCursor::new(realm, newest),
        })
    }

    fn snapshot_run_inspection(
        &self,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<RunInspection>>> {
        let transaction = self.begin()?;
        // The run's own id resolves its project inside the same transaction the
        // rest of the snapshot is read in, so the scope cannot move underneath
        // the reads that follow it.
        let project: Option<String> = transaction
            .query_row(
                "SELECT project_id FROM agent_runs WHERE id = ?1",
                params![agent_run_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let (_, newest) = control_window(&transaction)?;
        let realm = self.realm_id();
        let Some(project) = project else {
            return Ok(SnapshotEnvelope::new(realm, newest, None));
        };
        let project_id = ProjectId::parse(&project)?;
        let Some(run) = read_agent_run(&transaction, project_id, agent_run_id)? else {
            return Ok(SnapshotEnvelope::new(realm, newest, None));
        };
        let team_template: Option<(String, String)> = transaction
            .query_row(
                "SELECT snapshot, snapshot_hash FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), run.team_run_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let team_template = team_template
            .map(|(json, hash)| team_run_snapshot(&json, &hash))
            .transpose()?
            .map(|snapshot| (snapshot.template_id, snapshot.template_version));
        let gaps = read_gaps(&transaction, project_id, agent_run_id)?;
        // Read inside the same transaction as everything else, so the snapshot
        // is one consistent view rather than three reads that could interleave.
        let context_policy = read_run_context_policy(&transaction, agent_run_id)?;
        let latest_compaction = read_latest_compaction_receipt(&transaction, agent_run_id)?;
        Ok(SnapshotEnvelope::new(
            realm,
            newest,
            Some(RunInspection {
                project_id,
                run,
                team_template,
                gaps,
                context_policy,
                latest_compaction,
            }),
        ))
    }

    fn snapshot_task_inspection(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<TaskInspection>>> {
        let transaction = self.begin()?;
        let task: Option<RepositoryResult<Task>> = transaction
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1 AND id = ?2"),
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok(read_task(row)),
            )
            .optional()
            .map_err(backend)?;
        let (_, newest) = control_window(&transaction)?;
        let realm = self.realm_id();
        let Some(task) = task.transpose()? else {
            return Ok(SnapshotEnvelope::new(realm, newest, None));
        };
        let workflow_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM task_workflows
                 WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let (workflow, gates) = match workflow_id {
            None => (None, BTreeMap::new()),
            Some(id) => {
                let id = TaskWorkflowId::parse(&id)?;
                let (workflow, _) = load_workflow(&transaction, project_id, id)?;
                let gates = reduce_gate_states(&transaction, project_id, id)?;
                (Some(workflow), gates)
            }
        };
        // A task carries at most one persona snapshot per scenario revision; the
        // newest is the one in force, and it is read here rather than by scenario
        // id because a reader inspecting a task does not yet know which scenario
        // was frozen onto it.
        let persona: Option<(String, String)> = transaction
            .query_row(
                "SELECT snapshot, snapshot_hash FROM task_persona_snapshots
                 WHERE project_id = ?1 AND task_id = ?2
                 ORDER BY version DESC, scenario_id LIMIT 1",
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let persona = persona
            .map(|(json, hash)| stored_document::<PersonaScenarioSnapshot>(&json, &hash))
            .transpose()?;
        Ok(SnapshotEnvelope::new(
            realm,
            newest,
            Some(TaskInspection {
                task,
                workflow,
                gates,
                persona,
            }),
        ))
    }

    fn snapshot_target_revision(
        &self,
        project_id: ProjectId,
        target: &AggregateRef,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AggregateRevision>>> {
        // One row per target kind, addressed relationally exactly as
        // `command_targets` addresses it, so this read cannot resolve a target
        // the write path would refuse.
        let (sql, id) = match target {
            // The row has to be *both* the addressed project and the acting one:
            // a command naming another project's row is refused by finding
            // nothing, the same way every other target is.
            AggregateRef::Project {
                project_id: target_project,
            } => (
                "SELECT revision FROM projects WHERE id = ?2 AND id = ?1",
                target_project.to_string(),
            ),
            AggregateRef::MiniProject { mini_project_id } => (
                "SELECT revision FROM mini_projects WHERE project_id = ?1 AND id = ?2",
                mini_project_id.to_string(),
            ),
            AggregateRef::Task { task_id } => (
                "SELECT revision FROM tasks WHERE project_id = ?1 AND id = ?2",
                task_id.to_string(),
            ),
            AggregateRef::TeamRun { team_run_id } => (
                "SELECT revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
                team_run_id.to_string(),
            ),
            AggregateRef::AgentRun { agent_run_id } => (
                "SELECT revision FROM agent_runs WHERE project_id = ?1 AND id = ?2",
                agent_run_id.to_string(),
            ),
            AggregateRef::TicketLink { link_id } => (
                "SELECT revision FROM jira_links WHERE project_id = ?1 AND id = ?2",
                link_id.to_string(),
            ),
            // A calendar assignment has no revision column: it is retired and
            // replaced rather than updated in place, so its witness can only ever
            // be the initial one. Reporting the initial revision for a row that
            // exists is the honest reading; inventing a moving number would
            // suggest a compare-and-swap that has nothing to swap.
            AggregateRef::WorkCalendar { work_calendar_id } => (
                "SELECT 1 FROM work_calendars WHERE project_id = ?1 AND id = ?2",
                work_calendar_id.to_string(),
            ),
        };
        let transaction = self.begin()?;
        let found: Option<i64> = transaction
            .query_row(sql, params![project_id.to_string(), id], |row| row.get(0))
            .optional()
            .map_err(backend)?;
        let (_, newest) = control_window(&transaction)?;
        let revision = found.map(revision_of).transpose()?;
        Ok(SnapshotEnvelope::new(self.realm_id(), newest, revision))
    }
}

// ---------------------------------------------------------------------------
// The open-question ledger
// ---------------------------------------------------------------------------

/// An open question is project knowledge, on the same footing as a published
/// decision or a glossary entry.
const OPEN_QUESTION_TIER: ShareabilityTier = ShareabilityTier::ProjectKnowledge;

/// The head columns, in one place so every read decodes the same shape.
const OPEN_QUESTION_COLUMNS: &str = "question_id, mini_project_id, subject, scope, attachment, \
     author_seat_id, shareability_class, shareability_classifier, shareability_provenance, \
     created_at, revision";

/// One question header exactly as its columns arrive from SQLite.
///
/// The row is drained into owned values before anything is parsed, so the row
/// borrow is over by the time the child reads want the same transaction.
type OpenQuestionRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
);

fn open_question_row(row: &Row<'_>) -> rusqlite::Result<OpenQuestionRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

/// Rebuild one question header from its row, then load its history.
fn read_open_question(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    row: OpenQuestionRow,
) -> RepositoryResult<OpenQuestion> {
    let (
        question_id,
        mini_project_id,
        subject,
        scope,
        attachment,
        author,
        class,
        classifier,
        provenance,
        created_at,
        revision,
    ) = row;
    let question_id = OpenQuestionId::parse(&question_id)?;
    let attachment: OpenQuestionAttachment = serde_json::from_str(&attachment).map_err(|_| {
        DomainError::invalid(
            "OpenQuestion attachment",
            "the stored attachment cannot be read by this build",
        )
    })?;
    Ok(OpenQuestion {
        question_id,
        project_id,
        mini_project_id: MiniProjectId::parse(&mini_project_id)?,
        subject: BoundedText::parse(&subject)?,
        scope: QuestionScope::parse(&scope)?,
        attachment,
        author: SeatBindingId::parse(&author)?,
        shareability: stored_shareability((class, classifier, provenance))?,
        created_at: parse_utc_timestamp(&created_at)?,
        revision: AggregateRevision::parse(u64::try_from(revision).map_err(|_| {
            DomainError::invalid("OpenQuestion", "the stored revision is out of range")
        })?)?,
        rounds: read_open_question_rounds(transaction, project_id, question_id)?,
        dispositions: read_open_question_dispositions(transaction, project_id, question_id)?,
        firings: read_open_question_firings(transaction, project_id, question_id)?,
    })
}

fn read_open_question_rounds(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    question_id: OpenQuestionId,
) -> RepositoryResult<Vec<AmbiguityRound>> {
    let mut statement = transaction
        .prepare(
            "SELECT ordinal, author_seat_id, why_ambiguous, options, supersedes, recorded_at
             FROM open_question_rounds
             WHERE project_id = ?1 AND question_id = ?2
             ORDER BY ordinal",
        )
        .map_err(backend)?;
    let rows = statement
        .query_map(
            params![project_id.to_string(), question_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(backend)?;
    let mut rounds = Vec::new();
    for row in rows {
        let (ordinal, author, why, options, supersedes, recorded_at) = row.map_err(backend)?;
        let options: Vec<String> = serde_json::from_str(&options).map_err(|_| {
            DomainError::invalid(
                "OpenQuestion round",
                "the stored options cannot be read by this build",
            )
        })?;
        rounds.push(AmbiguityRound {
            ordinal: ordinal_value("OpenQuestion round", ordinal)?,
            author: SeatBindingId::parse(&author)?,
            why_ambiguous: BoundedText::parse(&why)?,
            options: options
                .iter()
                .map(|option| BoundedText::parse(option))
                .collect::<Result<Vec<_>, _>>()?,
            supersedes: supersedes
                .map(|value| ordinal_value("OpenQuestion round", value))
                .transpose()?,
            recorded_at: parse_utc_timestamp(&recorded_at)?,
        });
    }
    Ok(rounds)
}

fn read_open_question_dispositions(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    question_id: OpenQuestionId,
) -> RepositoryResult<Vec<Disposition>> {
    let mut statement = transaction
        .prepare(
            "SELECT ordinal, author_seat_id, kind, payload, supersedes, recorded_at
             FROM open_question_dispositions
             WHERE project_id = ?1 AND question_id = ?2
             ORDER BY ordinal",
        )
        .map_err(backend)?;
    let rows = statement
        .query_map(
            params![project_id.to_string(), question_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(backend)?;
    let mut dispositions = Vec::new();
    for row in rows {
        let (ordinal, author, kind, payload, supersedes, recorded_at) = row.map_err(backend)?;
        let outcome: DispositionOutcome = serde_json::from_str(&payload).map_err(|_| {
            DomainError::invalid(
                "OpenQuestion disposition",
                "the stored disposition cannot be read by this build",
            )
        })?;
        // The discriminator column and the payload are written together and must
        // still agree on the way out: a row edited around this repository does
        // not get to read back as a different kind of closing.
        if outcome.kind() != DispositionKind::parse(&kind)? {
            return Err(RepositoryError::Conflict {
                subject: "OpenQuestion disposition",
                rule: "the stored kind and payload disagree",
            });
        }
        dispositions.push(Disposition {
            ordinal: ordinal_value("OpenQuestion disposition", ordinal)?,
            author: SeatBindingId::parse(&author)?,
            outcome,
            supersedes: supersedes
                .map(|value| ordinal_value("OpenQuestion disposition", value))
                .transpose()?,
            recorded_at: parse_utc_timestamp(&recorded_at)?,
        });
    }
    Ok(dispositions)
}

fn read_open_question_firings(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    question_id: OpenQuestionId,
) -> RepositoryResult<Vec<TriggerFiring>> {
    let mut statement = transaction
        .prepare(
            "SELECT ordinal, disposition_ordinal, trigger_key, observed_by_seat_id, recorded_at
             FROM open_question_trigger_firings
             WHERE project_id = ?1 AND question_id = ?2
             ORDER BY ordinal",
        )
        .map_err(backend)?;
    let rows = statement
        .query_map(
            params![project_id.to_string(), question_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(backend)?;
    let mut firings = Vec::new();
    for row in rows {
        let (ordinal, disposition_ordinal, trigger, observed_by, recorded_at) =
            row.map_err(backend)?;
        firings.push(TriggerFiring {
            ordinal: ordinal_value("OpenQuestion trigger", ordinal)?,
            disposition_ordinal: ordinal_value("OpenQuestion trigger", disposition_ordinal)?,
            trigger: TriggerKey::parse(&trigger)?,
            observed_by: SeatBindingId::parse(&observed_by)?,
            recorded_at: parse_utc_timestamp(&recorded_at)?,
        });
    }
    Ok(firings)
}

/// Narrow a stored ordinal without silently wrapping it.
fn ordinal_value(subject: &'static str, stored: i64) -> RepositoryResult<u32> {
    u32::try_from(stored)
        .map_err(|_| DomainError::invalid(subject, "the stored ordinal is out of range").into())
}

/// Advance the head revision under compare-and-swap.
///
/// Every append goes through this, so an appended child and a moved head are one
/// atomic step: a caller working from a stale revision writes neither.
fn bump_open_question(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    question_id: OpenQuestionId,
    expected: AggregateRevision,
) -> RepositoryResult<AggregateRevision> {
    let changed = transaction
        .execute(
            "UPDATE open_questions SET revision = revision + 1
             WHERE project_id = ?1 AND question_id = ?2 AND revision = ?3",
            params![
                project_id.to_string(),
                question_id.to_string(),
                i64::try_from(expected.get()).unwrap_or(i64::MAX),
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(RepositoryError::Conflict {
            subject: "OpenQuestion",
            rule: "only the current revision of a question in this project may be appended to",
        });
    }
    Ok(expected.next()?)
}

impl OpenQuestionRepository for SqliteStore {
    fn raise_question(
        &self,
        project_id: ProjectId,
        question: &OpenQuestion,
    ) -> RepositoryResult<()> {
        if question.project_id != project_id {
            return Err(RepositoryError::Conflict {
                subject: "OpenQuestion",
                rule: "a question is raised in the project it names",
            });
        }
        question.shareability.validate_for(OPEN_QUESTION_TIER)?;
        let Some(first) = question.rounds.first() else {
            return Err(DomainError::invalid(
                "OpenQuestion",
                "a raised question carries its first round",
            )
            .into());
        };
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO open_questions
                     (question_id, project_id, mini_project_id, subject, scope, attachment,
                      author_seat_id, shareability_class, shareability_classifier,
                      shareability_provenance, created_at, revision)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    question.question_id.to_string(),
                    project_id.to_string(),
                    question.mini_project_id.to_string(),
                    question.subject.as_str(),
                    question.scope.as_str(),
                    serde_json::to_string(&question.attachment).map_err(|_| {
                        DomainError::invalid("OpenQuestion attachment", "does not serialize")
                    })?,
                    question.author.to_string(),
                    question.shareability.class.as_str(),
                    question
                        .shareability
                        .classifier
                        .identity()
                        .map(ExternalName::as_str),
                    question.shareability.provenance.as_str(),
                    text(question.created_at),
                    i64::try_from(question.revision.get()).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        insert_open_question_round(&transaction, project_id, question.question_id, first)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn get_question(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
    ) -> RepositoryResult<Option<OpenQuestion>> {
        let transaction = self.begin()?;
        let row: Option<OpenQuestionRow> = transaction
            .query_row(
                &format!(
                    "SELECT {OPEN_QUESTION_COLUMNS} FROM open_questions
                     WHERE project_id = ?1 AND question_id = ?2"
                ),
                params![project_id.to_string(), question_id.to_string()],
                open_question_row,
            )
            .optional()
            .map_err(backend)?;
        let found = row
            .map(|row| read_open_question(&transaction, project_id, row))
            .transpose()?;
        transaction.commit().map_err(backend)?;
        Ok(found)
    }

    fn list_questions_for_epic(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<OpenQuestion>> {
        let transaction = self.begin()?;
        let rows: Vec<OpenQuestionRow> = {
            let mut statement = transaction
                .prepare(&format!(
                    "SELECT {OPEN_QUESTION_COLUMNS} FROM open_questions
                     WHERE project_id = ?1 AND mini_project_id = ?2
                     ORDER BY created_at, question_id"
                ))
                .map_err(backend)?;
            statement
                .query_map(
                    params![project_id.to_string(), mini_project_id.to_string()],
                    open_question_row,
                )
                .map_err(backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(backend)?
        };
        let mut questions = Vec::with_capacity(rows.len());
        for row in rows {
            questions.push(read_open_question(&transaction, project_id, row)?);
        }
        transaction.commit().map_err(backend)?;
        Ok(questions)
    }

    fn summarize_questions_for_epic(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<OpenQuestionSummary>> {
        // Derived from the same loaded aggregates rather than from a status
        // column: a stored status could disagree with the history that produced
        // it, and this is the read a completion gate trusts.
        Ok(self
            .list_questions_for_epic(project_id, mini_project_id)?
            .iter()
            .map(OpenQuestion::summary)
            .collect())
    }

    fn append_question_round(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
        expected: AggregateRevision,
        round: &AmbiguityRound,
    ) -> RepositoryResult<AggregateRevision> {
        let transaction = self.begin()?;
        insert_open_question_round(&transaction, project_id, question_id, round)?;
        let revision = bump_open_question(&transaction, project_id, question_id, expected)?;
        transaction.commit().map_err(backend)?;
        Ok(revision)
    }

    fn append_question_disposition(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
        expected: AggregateRevision,
        disposition: &Disposition,
    ) -> RepositoryResult<AggregateRevision> {
        disposition.outcome.validate()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO open_question_dispositions
                     (project_id, question_id, ordinal, author_seat_id, kind, trigger_key,
                      payload, supersedes, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    project_id.to_string(),
                    question_id.to_string(),
                    i64::from(disposition.ordinal),
                    disposition.author.to_string(),
                    disposition.outcome.kind().as_str(),
                    disposition
                        .outcome
                        .deferred_trigger()
                        .map(|trigger| trigger.key.as_str()),
                    serde_json::to_string(&disposition.outcome).map_err(|_| {
                        DomainError::invalid("OpenQuestion disposition", "does not serialize")
                    })?,
                    disposition.supersedes.map(i64::from),
                    text(disposition.recorded_at),
                ],
            )
            .map_err(backend)?;
        let revision = bump_open_question(&transaction, project_id, question_id, expected)?;
        transaction.commit().map_err(backend)?;
        Ok(revision)
    }

    fn fire_deferred_trigger(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
        expected: AggregateRevision,
        firing: &TriggerFiring,
    ) -> RepositoryResult<AggregateRevision> {
        let transaction = self.begin()?;
        // The schema refuses a firing that names a trigger its deferral did not,
        // and refuses a second firing against one deferral. What it cannot see is
        // whether that deferral is still the *current* disposition, so that is
        // checked here.
        let current: Option<i64> = transaction
            .query_row(
                "SELECT MAX(ordinal) FROM open_question_dispositions
                 WHERE project_id = ?1 AND question_id = ?2",
                params![project_id.to_string(), question_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?
            .flatten();
        if current != Some(i64::from(firing.disposition_ordinal)) {
            return Err(RepositoryError::Conflict {
                subject: "OpenQuestion trigger",
                rule: "only the question's current deferral can be reopened",
            });
        }
        transaction
            .execute(
                "INSERT INTO open_question_trigger_firings
                     (project_id, question_id, ordinal, disposition_ordinal, trigger_key,
                      observed_by_seat_id, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    project_id.to_string(),
                    question_id.to_string(),
                    i64::from(firing.ordinal),
                    i64::from(firing.disposition_ordinal),
                    firing.trigger.as_str(),
                    firing.observed_by.to_string(),
                    text(firing.recorded_at),
                ],
            )
            .map_err(backend)?;
        let revision = bump_open_question(&transaction, project_id, question_id, expected)?;
        transaction.commit().map_err(backend)?;
        Ok(revision)
    }
}

fn insert_open_question_round(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    question_id: OpenQuestionId,
    round: &AmbiguityRound,
) -> RepositoryResult<()> {
    let options: Vec<&str> = round
        .options
        .iter()
        .map(kontor_core::id::BoundedText::as_str)
        .collect();
    transaction
        .execute(
            "INSERT INTO open_question_rounds
                 (project_id, question_id, ordinal, author_seat_id, why_ambiguous, options,
                  supersedes, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                project_id.to_string(),
                question_id.to_string(),
                i64::from(round.ordinal),
                round.author.to_string(),
                round.why_ambiguous.as_str(),
                serde_json::to_string(&options).map_err(|_| {
                    DomainError::invalid("OpenQuestion round", "the options do not serialize")
                })?,
                round.supersedes.map(i64::from),
                text(round.recorded_at),
            ],
        )
        .map_err(backend)?;
    Ok(())
}
