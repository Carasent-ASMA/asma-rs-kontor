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
use kontor_core::backlog_identity::EpicBacklogCode;
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
    ProjectId, ProviderUsageObservationId, QuickSessionId, QuotaObservationProvenanceId, RealmId,
    RoleCatalogId, RoleCode, RoleKey, RoleSlotId, RuntimeBindingId, RuntimeKindKey,
    ScheduleOverrideId, SeatBindingId, SignedDuration, SpecVersion, StatusConflictId,
    SuccessionAttemptId, TaskId, TaskWorkflowId, TeamDefinitionId, TeamDefinitionMigrationId,
    TeamRunId, TeamTemplateId, TicketLinkId, Timestamp, TopologyKindKey, TopologyNodeId,
    TopologySpecId, TriggerKey, WorkCalendarId, WorkProfileKey, format_utc_timestamp,
    parse_utc_timestamp,
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
    CompletionWrite, ConnectorSpecSelector, CredentialReference, CredentialReferenceKind,
    GateEvaluation, HistoryGapKind, HistoryGapMarker, IntakeCreatedWork, IntakeDecisionRecord,
    IntakeOutcome, IntakeRepository, MiniProject, MiniProjectTopologySnapshot, NewAbandonReceipt,
    NewAccountProfile, NewAdaptiveAdmissionState, NewAgentRun, NewAvailabilityOverride,
    NewCapacityObservation, NewCommandIntent, NewConsultationMaterializationReroute,
    NewConsultationRecoveryAttempt, NewGateEvaluation, NewIntakeDecision, NewIntakeDecisionRecord,
    NewIntakeReevaluation, NewLocalCommand, NewMiniProject, NewNativeContainerBinding,
    NewObservation, NewProject, NewProviderQuotaState, NewProviderUsageObservation,
    NewRuntimeEvent, NewSeatBinding, NewSessionTopologyNode, NewSourceEvent, NewTask,
    NewTaskPersonaSnapshot, NewTaskWorkflow, NewTeamRun, NewTicketLink, PhaseAdvance, Project,
    ProjectRepository, ProjectTopologyDefault, ProviderQuotaState, ProviderUsageObservation,
    QuotaObservationProvenance, RealmEventPage, RealmRepository, ReceiptAdvance,
    ReevaluationOutcome, RepositoryError, RepositoryResult, RunClosure, RunInspection,
    RunRepository, RuntimeBinding, RuntimeEvent, SeatLivenessObservation, SessionVerdictEvidence,
    SourceDisposition, SourceEventIngest, SpecRepository, StoredAdvisorAdvice,
    StoredCapacityConfiguration, StoredCommitteeFinding, StoredCompletionProfile,
    StoredCompletionWake, StoredCompletionWakeDelivery, StoredConsultationMaterializationReroute,
    StoredConsultationProfileRevision, StoredConsultationRecoveryAttempt, StoredConsultationRun,
    StoredConsultationSeat, StoredCoreTeamRevision, StoredEpicCompletion, StoredEpicRoster,
    StoredHostedTopologySeat, StoredPromotion, StoredQuickSession, StoredRemediationProposal,
    StoredTopologyContainerRecovery, SuccessionRepository, Task, TaskInspection,
    TaskTransitionRequest, TaskWorkflow, TeamRun, TeamRunAdvance, TeamRunClosure, TicketLink,
    TicketRepository, TopologyRepository, WorkflowRepository, validate_dependency_graph,
};
use kontor_core::repository::{
    LegacyEpicBacklogCodeCorrection, LiveNativeSubject, MigrationObjectKind,
    MiniProjectTeamDefinitionSnapshot, NativePlacement, NewTeamDefinitionMigration,
    ProjectTeamDefinitionDefault, StoredTeamDefinitionMigration,
    TeamDefinitionMigrationObservation, TeamDefinitionMigrationState,
    TeamDefinitionMigrationSubject, TeamDefinitionMigrationTarget,
    TeamDefinitionMigrationTargetState, TeamDefinitionRepository, TopologyContainerRecovery,
};
use kontor_core::spec::{
    CanonicalSourceEvent, CatalogRoleRef, IntakeReceipt, ModelRung, NodeProjectionCapability,
    PersonaScenarioSnapshot, PersonaScenarioSpec, ProjectSessionTopologySpec, ProviderQuotaKind,
    ProviderQuotaSource, ResolvedWorkProfileSnapshot, RoleCatalogRevision, Shareability,
    ShareabilityClass, ShareabilityClassifier, ShareabilityProvenance, ShareabilityTier,
    SourceIdentity, TeamDefinitionSnapshot, TeamDefinitionSpec, TeamRunSnapshot,
    TeamTemplateRevision, TopologySnapshot, TriggerSpec, WorkProfileSpec,
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
use kontor_core::succession::{
    NewSuccessionAttempt, SuccessionAttempt, SuccessionAttemptAdvance, SuccessionAttemptState,
    SuccessionConfirmation, SuccessionDeferredRefresh, SuccessionHandoff, SuccessionHandoffRecord,
    SuccessionReceipt, SuccessionRefusal, SuccessionRefusalReason, SuccessionSuccessorObservation,
    SuccessionSuccessorRecord,
};
use kontor_core::ticket::{
    EpicStatusConflict, EpicStatusTransitionIntent, ExternalCommentRevision,
    ExternalTicketObservation, ExternalWorkflowSpec, StatusConflict, StatusTransitionReceipt,
    TicketFieldSpec, TicketSyncProjection,
};

type EpicTransitionIntentRow = (
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
);
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

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

pub(crate) fn is_jira_connector(connector: &kontor_core::id::ConnectorKey) -> bool {
    matches!(connector.as_str(), "jira" | "connector.jira")
}

pub(crate) fn canonical_jira_connector() -> kontor_core::id::ConnectorKey {
    kontor_core::id::ConnectorKey::parse("connector.jira")
        .expect("the built-in Jira connector key is valid")
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
        topic,
    ) = columns;
    // A stored NULL stays None. Nothing here reconstructs a topic from the
    // question beside it, which is exactly the inference the contract forbids.
    let topic = topic.as_deref().map(ExternalName::parse).transpose()?;
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
        topic,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemediationCommandAction {
    LsaProposal,
    TpmRoute,
}

impl RemediationCommandAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::LsaProposal => "lsa_proposal",
            Self::TpmRoute => "tpm_route",
        }
    }
}

fn remediation_command_request<'a>(
    store: &SqliteStore,
    envelope: &'a ReceiptEnvelope<NewLocalCommand>,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
) -> RepositoryResult<&'a NewLocalCommand> {
    let request = envelope.peek(store.realm_id())?;
    if request.project_id != project_id
        || request.kind != CommandKind::RemediateCompletion
        || request.target != (AggregateRef::MiniProject { mini_project_id })
    {
        return Err(conflict(
            "completion remediation command",
            "the receipt authority does not name this epic remediation",
        ));
    }
    Ok(request)
}

fn remediation_claim(
    transaction: &Transaction<'_>,
    request: &NewLocalCommand,
    mini_project_id: MiniProjectId,
    completion_generation: u32,
    round: u8,
    action: RemediationCommandAction,
    effect_revision: Option<AggregateRevision>,
) -> RepositoryResult<bool> {
    let stored: Option<(String, String, Option<i64>)> = transaction
        .query_row(
            "SELECT idempotency_key, intent_hash, effect_revision
               FROM epic_completion_remediation_command_claims
              WHERE project_id = ?1 AND mini_project_id = ?2
                AND completion_generation = ?3 AND round = ?4 AND action = ?5",
            params![
                request.project_id.to_string(),
                mini_project_id.to_string(),
                i64::from(completion_generation),
                i64::from(round),
                action.as_str(),
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(backend)?;
    let effect_revision =
        effect_revision.map(|revision| i64::try_from(revision.get()).unwrap_or(i64::MAX));
    if let Some((key, intent_hash, stored_effect_revision)) = stored {
        if key != request.idempotency_key.as_str()
            || intent_hash != request.intent.hash().as_str()
            || stored_effect_revision != effect_revision
        {
            return Err(conflict(
                "completion remediation command claim",
                "this remediation action is already bound to a different key, intent, or effect revision",
            ));
        }
        return Ok(true);
    }
    transaction
        .execute(
            "INSERT INTO epic_completion_remediation_command_claims
                 (project_id, mini_project_id, completion_generation, round, action,
                  idempotency_key, intent_hash, effect_revision, claimed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                request.project_id.to_string(),
                mini_project_id.to_string(),
                i64::from(completion_generation),
                i64::from(round),
                action.as_str(),
                request.idempotency_key.as_str(),
                request.intent.hash().as_str(),
                effect_revision,
                text(request.created_at),
            ],
        )
        .map_err(backend)?;
    Ok(false)
}

fn local_command_receipt(
    transaction: &Transaction<'_>,
    key: &IdempotencyKey,
) -> RepositoryResult<CommandReceipt> {
    transaction
        .query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM command_receipts WHERE idempotency_key = ?1"),
            params![key.as_str()],
            |row| Ok(crate::commands::receipts::read_receipt_row(row)),
        )
        .map_err(backend)?
}

fn remediation_proposal_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    completion_generation: u32,
    round: u8,
) -> RepositoryResult<Option<StoredRemediationProposal>> {
    transaction
        .query_row(
            "SELECT failed_round_evidence, proposal, lsa_seat_binding_id,
                    lsa_occupancy_generation, proposed_at
               FROM epic_completion_remediation_proposals
              WHERE project_id = ?1 AND mini_project_id = ?2
                AND completion_generation = ?3 AND round = ?4",
            params![
                project_id.to_string(),
                mini_project_id.to_string(),
                i64::from(completion_generation),
                i64::from(round),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?
        .map(|columns| {
            Ok(StoredRemediationProposal {
                project_id,
                mini_project_id,
                completion_generation,
                round,
                failed_round_evidence: ContentHash::parse(&columns.0)?,
                proposal: ContentHash::parse(&columns.1)?,
                lsa_seat_binding_id: SeatBindingId::parse(&columns.2)?,
                lsa_occupancy_generation: u64::try_from(columns.3).map_err(|_| {
                    RepositoryError::Backend {
                        detail: "an LSA proposal occupancy generation is invalid".to_owned(),
                    }
                })?,
                proposed_at: read_timestamp(&columns.4)?,
            })
        })
        .transpose()
}

fn same_remediation_proposal(
    stored: &StoredRemediationProposal,
    requested: &StoredRemediationProposal,
) -> bool {
    stored.project_id == requested.project_id
        && stored.mini_project_id == requested.mini_project_id
        && stored.completion_generation == requested.completion_generation
        && stored.round == requested.round
        && stored.failed_round_evidence == requested.failed_round_evidence
        && stored.proposal == requested.proposal
        && stored.lsa_seat_binding_id == requested.lsa_seat_binding_id
        && stored.lsa_occupancy_generation == requested.lsa_occupancy_generation
}

fn insert_remediation_proposal_in(
    transaction: &Transaction<'_>,
    proposal: &StoredRemediationProposal,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, completion_generation, round,
                  failed_round_evidence, proposal, lsa_seat_binding_id,
                  lsa_occupancy_generation, proposed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                proposal.project_id.to_string(),
                proposal.mini_project_id.to_string(),
                i64::from(proposal.completion_generation),
                i64::from(proposal.round),
                proposal.failed_round_evidence.as_str(),
                proposal.proposal.as_str(),
                proposal.lsa_seat_binding_id.to_string(),
                i64::try_from(proposal.lsa_occupancy_generation).unwrap_or(i64::MAX),
                text(proposal.proposed_at),
            ],
        )
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(failure, _)
                if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                conflict(
                    "remediation proposal",
                    "one failed round has one bounded proposal",
                )
            }
            other => backend(other),
        })?;
    Ok(())
}

fn epic_completion_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
) -> RepositoryResult<Option<StoredEpicCompletion>> {
    transaction
        .query_row(
            "SELECT profile_id, profile_version, definition_hash, state, revision, updated_at
               FROM epic_completion
              WHERE project_id = ?1 AND mini_project_id = ?2",
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
                revision: AggregateRevision::parse(u64::try_from(columns.4).unwrap_or_default())?,
                updated_at: read_timestamp(&columns.5)?,
            })
        })
        .transpose()
}

fn ensure_completion_wake_in(
    transaction: &Transaction<'_>,
    wake: &StoredCompletionWake,
) -> RepositoryResult<()> {
    let existing: Option<String> = transaction
        .query_row(
            "SELECT receipt FROM epic_completion_wakes
              WHERE project_id = ?1 AND mini_project_id = ?2
                AND completion_revision = ?3 AND reason = ?4 AND seat_binding_id = ?5",
            params![
                wake.project_id.to_string(),
                wake.mini_project_id.to_string(),
                i64::try_from(wake.completion_revision.get()).unwrap_or(i64::MAX),
                wake.reason.as_str(),
                wake.seat_binding_id.to_string(),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(receipt) = existing {
        if receipt != wake.receipt.as_str() {
            return Err(conflict(
                "completion wake intent",
                "the existing wake names different durable evidence",
            ));
        }
        return Ok(());
    }
    transaction
        .execute(
            "INSERT INTO epic_completion_wakes
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
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn read_completion_wake_delivery(
    connection: &Connection,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    completion_revision: AggregateRevision,
    reason: &ExternalName,
    seat_binding_id: SeatBindingId,
    occupancy_generation: u64,
    native_id: &ExternalId,
) -> RepositoryResult<Option<StoredCompletionWakeDelivery>> {
    let row = connection
        .query_row(
            "SELECT d.occupancy_generation, d.runtime_kind, d.host,
                    d.runtime_generation, d.message_id, d.body, d.body_hash,
                    d.created_at, d.acknowledged_at, d.timeline_epoch,
                    d.timeline_sequence, w.receipt, w.appended_at, w.acknowledged_at
             FROM epic_completion_wake_deliveries d
             JOIN epic_completion_wakes w
               ON w.project_id = d.project_id
              AND w.mini_project_id = d.mini_project_id
              AND w.completion_revision = d.completion_revision
              AND w.reason = d.reason
              AND w.seat_binding_id = d.seat_binding_id
             WHERE d.project_id = ?1 AND d.mini_project_id = ?2
               AND d.completion_revision = ?3 AND d.reason = ?4
               AND d.seat_binding_id = ?5 AND d.occupancy_generation = ?6
               AND d.native_id = ?7",
            params![
                project_id.to_string(),
                mini_project_id.to_string(),
                i64::try_from(completion_revision.get()).unwrap_or(i64::MAX),
                reason.as_str(),
                seat_binding_id.to_string(),
                i64::try_from(occupancy_generation).unwrap_or(i64::MAX),
                native_id.as_str(),
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    row.map(|columns| {
        Ok(StoredCompletionWakeDelivery {
            wake: StoredCompletionWake {
                project_id,
                mini_project_id,
                completion_revision,
                reason: reason.clone(),
                seat_binding_id,
                receipt: ContentHash::parse(&columns.11)?,
                appended_at: read_timestamp(&columns.12)?,
                acknowledged_at: columns.13.as_deref().map(read_timestamp).transpose()?,
            },
            occupancy_generation: u64::try_from(columns.0).map_err(|_| {
                RepositoryError::Backend {
                    detail: "a completion wake occupancy generation is invalid".to_owned(),
                }
            })?,
            native_identity: NativeRuntimeIdentity {
                runtime_kind: RuntimeKindKey::parse(&columns.1)?,
                host: ExternalName::parse(&columns.2)?,
                generation: u64::try_from(columns.3).map_err(|_| RepositoryError::Backend {
                    detail: "a completion wake runtime generation is invalid".to_owned(),
                })?,
                native_id: native_id.clone(),
            },
            message_id: columns.4,
            body: BoundedText::parse(&columns.5)?,
            body_hash: ContentHash::parse(&columns.6)?,
            created_at: read_timestamp(&columns.7)?,
            acknowledged_at: columns.8.as_deref().map(read_timestamp).transpose()?,
            timeline_epoch: columns
                .9
                .map(|value| {
                    u64::try_from(value).map_err(|_| RepositoryError::Backend {
                        detail: "a completion wake timeline epoch is invalid".to_owned(),
                    })
                })
                .transpose()?,
            timeline_sequence: columns
                .10
                .map(|value| {
                    u64::try_from(value).map_err(|_| RepositoryError::Backend {
                        detail: "a completion wake timeline sequence is invalid".to_owned(),
                    })
                })
                .transpose()?,
        })
    })
    .transpose()
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
                      updated_at, settled_at, topic)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
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
                    run.topic.as_ref().map(ExternalName::as_str),
                ],
            )
            .map_err(backend)?;

        if let Some(provenance) = run
            .context
            .get("re_review")
            .filter(|value| !value.is_null())
        {
            if run.id.family() != ConsultationFamily::Committee {
                return Err(conflict(
                    "consultation re-review provenance",
                    "only a Committee run may claim completion re-review lineage",
                ));
            }
            let provenance = CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "re_review": provenance,
            }))?;
            transaction
                .execute(
                    "INSERT INTO committee_re_review_claims
                         (project_id, mini_project_id, provenance, provenance_hash,
                          committee_run_id, claimed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        run.project_id.to_string(),
                        run.mini_project_id.to_string(),
                        provenance.json(),
                        provenance.hash().as_str(),
                        run.id.as_text(),
                        text(run.created_at),
                    ],
                )
                .map_err(|error| match error {
                    rusqlite::Error::SqliteFailure(failure, _)
                        if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        conflict(
                            "Committee re-review provenance",
                            "this completion freeze already has one clean Committee re-review",
                        )
                    }
                    other => backend(other),
                })?;
        }

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
                        updated_at, settled_at, topic
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
                        row.get::<_, Option<String>>(20)?,
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

    /// The consultation occupying one ASW/CSW topology node, if any.
    ///
    /// This is the lookup Team Definition rendering needs: a container's name
    /// carries the consultation's topic, and the node is the only thing the
    /// renderer holds when it is asked for that container's title. The node is
    /// unique across consultation runs, so at most one can answer.
    pub fn get_consultation_run_by_topology_node(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<StoredConsultationRun>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT run_id, family FROM consultation_runs
                 WHERE project_id = ?1 AND topology_node_id = ?2",
                params![project_id.to_string(), topology_node_id.to_string()],
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

    /// Supply the explicit topic one legacy consultation was missing.
    ///
    /// This is the only lawful way a consultation recorded before topics
    /// existed acquires one: an operator states it and the migration carries
    /// it. Nothing here reads the question, profile, title or node id, because
    /// deriving a topic from any of those is exactly what the naming contract
    /// forbids.
    ///
    /// Write-once, and only over an absent value. Supplying the same topic
    /// again is the replay of a migration step and returns the same run;
    /// supplying a different one is a conflict, because the first value has
    /// already been rendered into a native title and treated as authoritative.
    ///
    /// The intent must still be in flight: a topic is supplied *before* the new
    /// pin becomes current, so a confirmed migration has nothing left to say
    /// about what its containers are called.
    ///
    /// # Errors
    /// Refuses an unknown or foreign consultation, an intent from another
    /// project or epic, a settled intent, and a different existing topic.
    pub fn supply_legacy_consultation_topic(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        intent_id: TeamDefinitionMigrationId,
        topic: &ExternalName,
        supplied_at: Timestamp,
    ) -> RepositoryResult<StoredConsultationRun> {
        let transaction = self.begin()?;
        let intent = team_definition_migration_in(&transaction, project_id, intent_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        if intent.state.is_terminal() {
            return Err(conflict(
                "consultation topic",
                "a settled migration cannot supply a topic",
            ));
        }
        // The consultation is addressed by project *and* node, so a node from
        // another project cannot be reached even if its id is known.
        let found: Option<(String, String, String, Option<String>)> = transaction
            .query_row(
                "SELECT run_id, family, mini_project_id, topic FROM consultation_runs
                 WHERE project_id = ?1 AND topology_node_id = ?2",
                params![project_id.to_string(), topology_node_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        let (run_id, family, mini_project_id, existing) =
            found.ok_or(RepositoryError::NotFound {
                subject: "consultation run",
            })?;
        if MiniProjectId::parse(&mini_project_id)? != intent.mini_project_id {
            return Err(conflict(
                "consultation topic",
                "the consultation belongs to a different epic than the migration",
            ));
        }
        let run_id = consultation_run_id(ConsultationFamily::parse(&family)?, &run_id)?;
        match existing.as_deref() {
            // Already answered, and answered the same way: this is the replay
            // of a migration step, not a second decision.
            Some(current) if current == topic.as_str() => {
                transaction.commit().map_err(backend)?;
                return self.get_consultation_run(project_id, run_id)?.ok_or(
                    RepositoryError::NotFound {
                        subject: "consultation run",
                    },
                );
            }
            Some(_) => {
                return Err(conflict(
                    "consultation topic",
                    "this consultation already has a different authoritative topic",
                ));
            }
            None => {}
        }
        transaction
            .execute(
                "UPDATE consultation_runs SET topic = ?1
                 WHERE project_id = ?2 AND topology_node_id = ?3 AND topic IS NULL",
                params![
                    topic.as_str(),
                    project_id.to_string(),
                    topology_node_id.to_string(),
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO consultation_topic_migration_provenance
                     (project_id, run_id, intent_id, topic, supplied_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project_id.to_string(),
                    run_id.as_text(),
                    intent_id.to_string(),
                    topic.as_str(),
                    text(supplied_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        self.get_consultation_run(project_id, run_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "consultation run",
            })
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

    /// Read the immutable route/profile basis for one native-less successor.
    pub fn get_consultation_materialization_route_provenance(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
        role_slot_id: &RoleSlotId,
        successor_generation: u64,
    ) -> RepositoryResult<Option<(kontor_core::spec::ModelRung, ContentHash)>> {
        let generation = i64::try_from(successor_generation).map_err(|_| {
            conflict(
                "materialization reroute",
                "the successor generation cannot be represented",
            )
        })?;
        self.connection
            .query_row(
                "SELECT successor_model_rung, recovery_profile_hash
                 FROM consultation_seat_materialization_reroutes
                 WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
                   AND successor_generation = ?4",
                params![
                    project_id.to_string(),
                    run_id.as_text(),
                    role_slot_id.as_str(),
                    generation,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(backend)?
            .map(|(route, hash)| {
                Ok((
                    serde_json::from_str(&route).map_err(|_| {
                        conflict("materialization reroute", "the successor route is invalid")
                    })?,
                    ContentHash::parse(&hash)?,
                ))
            })
            .transpose()
    }

    /// Read an already-committed native-less reroute by its exact intent.
    pub fn get_consultation_materialization_reroute_by_intent(
        &self,
        project_id: ProjectId,
        request_intent_hash: &ContentHash,
    ) -> RepositoryResult<Option<StoredConsultationMaterializationReroute>> {
        let row = self
            .connection
            .query_row(
                "SELECT run_id, role_slot_id, seat_binding_id,
                    predecessor_generation, successor_generation,
                    predecessor_model_rung, successor_model_rung, reason,
                    recovery_profile, recovery_profile_hash, idempotency_key,
                    headroom_account_profile_id, headroom_observation_id,
                    headroom_evidence_hash, predecessor_revision,
                    successor_revision, rerouted_at
             FROM consultation_seat_materialization_reroutes
             WHERE project_id = ?1 AND request_intent_hash = ?2",
                params![project_id.to_string(), request_intent_hash.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                        row.get::<_, i64>(14)?,
                        row.get::<_, i64>(15)?,
                        row.get::<_, String>(16)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(
            |(
                run_id,
                role_slot_id,
                seat_binding_id,
                predecessor_generation,
                successor_generation,
                predecessor_model_rung,
                successor_model_rung,
                reason,
                recovery_profile,
                recovery_profile_hash,
                idempotency_key,
                headroom_account_profile_id,
                headroom_observation_id,
                headroom_evidence_hash,
                predecessor_revision,
                successor_revision,
                rerouted_at,
            )| {
                Ok(StoredConsultationMaterializationReroute {
                    project_id,
                    run_id: ConsultationRunId::Committee(CommitteeRunId::parse(&run_id)?),
                    role_slot_id: RoleSlotId::parse(&role_slot_id)?,
                    seat_binding_id: SeatBindingId::parse(&seat_binding_id)?,
                    predecessor_occupancy_generation: u64::try_from(predecessor_generation)
                        .map_err(|_| {
                            conflict(
                                "materialization reroute",
                                "the predecessor generation is invalid",
                            )
                        })?,
                    successor_occupancy_generation: u64::try_from(successor_generation).map_err(
                        |_| {
                            conflict(
                                "materialization reroute",
                                "the successor generation is invalid",
                            )
                        },
                    )?,
                    predecessor_model_rung: serde_json::from_str(&predecessor_model_rung).map_err(
                        |_| {
                            conflict(
                                "materialization reroute",
                                "the predecessor route is invalid",
                            )
                        },
                    )?,
                    successor_model_rung: serde_json::from_str(&successor_model_rung).map_err(
                        |_| conflict("materialization reroute", "the successor route is invalid"),
                    )?,
                    reason,
                    recovery_profile: serde_json::from_str(&recovery_profile).map_err(|_| {
                        conflict("materialization reroute", "the recovery profile is invalid")
                    })?,
                    recovery_profile_hash: ContentHash::parse(&recovery_profile_hash)?,
                    request_intent_hash: request_intent_hash.clone(),
                    idempotency_key: IdempotencyKey::parse(&idempotency_key)?,
                    headroom_account_profile_id: AccountProfileId::parse(
                        &headroom_account_profile_id,
                    )?,
                    headroom_observation_id: ProviderUsageObservationId::parse(
                        &headroom_observation_id,
                    )?,
                    headroom_evidence_hash: ContentHash::parse(&headroom_evidence_hash)?,
                    predecessor_run_revision: AggregateRevision::parse(
                        u64::try_from(predecessor_revision).map_err(|_| {
                            conflict(
                                "materialization reroute",
                                "the predecessor revision is invalid",
                            )
                        })?,
                    )?,
                    successor_run_revision: AggregateRevision::parse(
                        u64::try_from(successor_revision).map_err(|_| {
                            conflict(
                                "materialization reroute",
                                "the successor revision is invalid",
                            )
                        })?,
                    )?,
                    rerouted_at: read_timestamp(&rerouted_at)?,
                })
            },
        )
        .transpose()
    }

    /// Atomically reroute one still-native-less materializing Committee seat.
    pub fn reroute_unmaterialized_consultation_seat(
        &self,
        request: &NewConsultationMaterializationReroute,
    ) -> RepositoryResult<StoredConsultationMaterializationReroute> {
        if request.reason != "permission_mode_unsupported" {
            return Err(conflict(
                "materialization reroute",
                "the recovery reason is unsupported",
            ));
        }
        if let Some(existing) = self.get_consultation_materialization_reroute_by_intent(
            request.project_id,
            &request.request_intent_hash,
        )? {
            if existing.idempotency_key != request.idempotency_key {
                return Err(conflict(
                    "materialization reroute",
                    "the committed reroute belongs to another idempotency key",
                ));
            }
            return Ok(existing);
        }
        let successor_generation = request
            .predecessor
            .occupancy_generation
            .checked_add(1)
            .ok_or_else(|| {
                conflict(
                    "materialization reroute",
                    "the occupancy generation overflowed",
                )
            })?;
        let successor_revision = request.expected_revision.next()?;
        let predecessor_model =
            serde_json::to_string(&request.predecessor.model_rung).map_err(|_| {
                conflict(
                    "materialization reroute",
                    "the predecessor route could not be encoded",
                )
            })?;
        let successor_model =
            serde_json::to_string(&request.successor_model_rung).map_err(|_| {
                conflict(
                    "materialization reroute",
                    "the successor route could not be encoded",
                )
            })?;
        let transaction = self.begin()?;
        let observation = transaction
            .query_row(
                &format!(
                    "SELECT {PROVIDER_USAGE_OBSERVATION_COLUMNS}
                     FROM provider_usage_observations
                     WHERE id = ?1 AND project_id = ?2 AND account_profile_id = ?3
                       AND provider = ?4"
                ),
                params![
                    request.headroom_observation.id.to_string(),
                    request.project_id.to_string(),
                    request.headroom_observation.account_profile_id.to_string(),
                    request.successor_model_rung.provider.0.as_str(),
                ],
                |row| {
                    read_provider_usage_observation(row)
                        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
                },
            )
            .optional()
            .map_err(backend)?
            .ok_or_else(|| {
                conflict(
                    "materialization reroute headroom",
                    "the selected provider usage observation is absent or foreign",
                )
            })?;
        let latest_observation_id: String = transaction
            .query_row(
                "SELECT id FROM provider_usage_observations
                 WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3
                 ORDER BY observed_at DESC, id DESC LIMIT 1",
                params![
                    request.project_id.to_string(),
                    observation.account_profile_id.to_string(),
                    observation.provider.as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if observation != request.headroom_observation
            || latest_observation_id != observation.id.to_string()
            || observation.state != ProviderQuotaKind::Available
            || observation.observed_at < request.headroom_fresh_after
        {
            return Err(conflict(
                "materialization reroute headroom",
                "the selected provider observation is not the latest fresh available evidence",
            ));
        }
        let current: Option<RepositoryResult<ProviderQuotaState>> = transaction
            .query_row(
                &format!(
                    "SELECT {PROVIDER_QUOTA_STATE_COLUMNS} FROM provider_quota_states
                     WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3"
                ),
                params![
                    request.project_id.to_string(),
                    observation.account_profile_id.to_string(),
                    observation.provider.as_str(),
                ],
                |row| Ok(read_provider_quota_state(row)),
            )
            .optional()
            .map_err(backend)?;
        let mut current = current.transpose()?.ok_or_else(|| {
            conflict(
                "materialization reroute headroom",
                "the current provider-report projection is absent",
            )
        })?;
        attach_provider_quota_windows(&transaction, &mut current)?;
        let projection_matches = current.source == ProviderQuotaSource::ProviderReport
            && current.state == ProviderQuotaKind::Available
            && current.evidence_hash == observation.evidence_hash
            && current.state == observation.state
            && current.resets_at == observation.resets_at
            && current.windows == observation.windows;
        if !projection_matches {
            return Err(conflict(
                "materialization reroute headroom",
                "the current provider report does not match the selected fresh observation",
            ));
        }
        let run_ok: i64 = transaction
            .query_row(
                "SELECT count(*) FROM consultation_runs
             WHERE project_id = ?1 AND run_id = ?2 AND family = 'committee'
               AND state = 'materializing' AND result IS NULL AND result_hash IS NULL
               AND revision = ?3",
                params![
                    request.project_id.to_string(),
                    request.predecessor.run_id.as_text(),
                    i64::try_from(request.expected_revision.get()).unwrap_or(i64::MAX)
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let findings: i64 = transaction.query_row(
            "SELECT count(*) FROM committee_findings WHERE project_id = ?1 AND committee_run_id = ?2",
            params![request.project_id.to_string(), request.predecessor.run_id.as_text()],
            |row| row.get(0),
        ).map_err(backend)?;
        if run_ok != 1 || findings != 0 {
            return Err(conflict(
                "materialization reroute",
                "only an unchanged result-less and findings-free materializing Committee may be rerouted",
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE consultation_seats
             SET model_rung = ?7, occupancy_generation = ?8
             WHERE project_id = ?1 AND run_id = ?2 AND role_slot_id = ?3
               AND seat_binding_id = ?4 AND model_rung = ?5 AND occupancy_generation = ?6
               AND runtime_kind IS NULL AND host IS NULL AND generation IS NULL
               AND native_id IS NULL AND provider_session_id IS NULL AND observed_at IS NULL",
                params![
                    request.project_id.to_string(),
                    request.predecessor.run_id.as_text(),
                    request.predecessor.role_slot_id.as_str(),
                    request.predecessor.seat_binding_id.to_string(),
                    predecessor_model,
                    i64::try_from(request.predecessor.occupancy_generation).unwrap_or(i64::MAX),
                    successor_model,
                    i64::try_from(successor_generation).unwrap_or(i64::MAX)
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "materialization reroute",
                "the target seat route, generation, binding, or native-less state moved",
            ));
        }
        transaction
            .execute(
                "INSERT INTO consultation_seat_materialization_reroutes
             (project_id, run_id, role_slot_id, seat_binding_id,
              predecessor_generation, successor_generation,
              predecessor_model_rung, successor_model_rung, reason,
              recovery_profile, recovery_profile_hash, request_intent_hash, idempotency_key,
              headroom_account_profile_id, headroom_observation_id,
              headroom_evidence_hash, predecessor_revision, successor_revision, rerouted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                params![
                    request.project_id.to_string(),
                    request.predecessor.run_id.as_text(),
                    request.predecessor.role_slot_id.as_str(),
                    request.predecessor.seat_binding_id.to_string(),
                    i64::try_from(request.predecessor.occupancy_generation).unwrap_or(i64::MAX),
                    i64::try_from(successor_generation).unwrap_or(i64::MAX),
                    predecessor_model,
                    successor_model,
                    request.reason,
                    request.recovery_profile.json(),
                    request.recovery_profile.hash().as_str(),
                    request.request_intent_hash.as_str(),
                    request.idempotency_key.as_str(),
                    observation.account_profile_id.to_string(),
                    observation.id.to_string(),
                    observation.evidence_hash.as_str(),
                    i64::try_from(request.expected_revision.get()).unwrap_or(i64::MAX),
                    i64::try_from(successor_revision.get()).unwrap_or(i64::MAX),
                    text(request.rerouted_at)
                ],
            )
            .map_err(backend)?;
        let run_changed = transaction
            .execute(
                "UPDATE consultation_runs SET revision = ?4, updated_at = ?5
             WHERE project_id = ?1 AND run_id = ?2 AND revision = ?3 AND state = 'materializing'",
                params![
                    request.project_id.to_string(),
                    request.predecessor.run_id.as_text(),
                    i64::try_from(request.expected_revision.get()).unwrap_or(i64::MAX),
                    i64::try_from(successor_revision.get()).unwrap_or(i64::MAX),
                    text(request.rerouted_at)
                ],
            )
            .map_err(backend)?;
        if run_changed != 1 {
            return Err(conflict(
                "materialization reroute",
                "the Committee revision moved during reroute",
            ));
        }
        transaction.commit().map_err(backend)?;
        self.get_consultation_materialization_reroute_by_intent(
            request.project_id,
            &request.request_intent_hash,
        )?
        .ok_or_else(|| {
            conflict(
                "materialization reroute",
                "the committed lineage could not be read back",
            )
        })
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

    /// Read the current native occupancy generation of a persistent topology
    /// seat. Hosted-seat history is append-only, so `history + 1` is a durable
    /// generation fence without overloading the runtime daemon generation.
    pub fn hosted_topology_seat_occupancy_generation(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
    ) -> RepositoryResult<Option<u64>> {
        let generation = self
            .connection
            .query_row(
                "SELECT 1 + (
                     SELECT COUNT(*) FROM hosted_topology_seat_history h
                     WHERE h.project_id = s.project_id
                       AND h.seat_binding_id = s.seat_binding_id
                 )
                 FROM hosted_topology_seats s
                 WHERE s.project_id = ?1 AND s.seat_binding_id = ?2",
                params![project_id.to_string(), seat_binding_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(backend)?;
        generation
            .map(|generation| {
                u64::try_from(generation).map_err(|_| RepositoryError::Backend {
                    detail: "a hosted-seat occupancy generation is invalid".to_owned(),
                })
            })
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

    /// Read every immutable predecessor of one persistent topology seat.
    ///
    /// The ordered native ids are passed to the runtime only as a negative
    /// admission fence: a stale native projection may be ignored during a new
    /// launch only when Kontor has already committed that exact identity to
    /// append-only route history. Nothing here makes a historical native
    /// current or driveable again.
    pub fn list_hosted_topology_seat_history_native_ids(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
    ) -> RepositoryResult<Vec<ExternalId>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT native_id
                 FROM hosted_topology_seat_history
                 WHERE project_id = ?1 AND seat_binding_id = ?2
                 ORDER BY retired_at, native_id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(
                params![project_id.to_string(), seat_binding_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .map_err(backend)?;
        let mut history = Vec::new();
        for row in rows {
            history.push(ExternalId::parse(&row.map_err(backend)?)?);
        }
        Ok(history)
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

    /// Read the one advice artifact a single-seat Advisor run produced.
    ///
    /// An ASW may hold several independently reporting seats, and "the advice
    /// of the run" is then not a single artifact. Rather than silently return
    /// one of them and let a caller under-report the others, this refuses a
    /// multi-seat run: use [`SqliteStore::list_advisor_advice`], which is what
    /// a disposition over all configured seats needs anyway.
    pub fn get_advisor_advice(
        &self,
        project_id: ProjectId,
        advisor_run_id: AdvisorRunId,
    ) -> RepositoryResult<Option<StoredAdvisorAdvice>> {
        let mut advice = self.list_advisor_advice(project_id, advisor_run_id)?;
        if advice.len() > 1 {
            return Err(RepositoryError::Conflict {
                subject: "Advisor advice",
                rule: "a multi-seat Advisor run has no single advice artifact",
            });
        }
        Ok(advice.pop())
    }

    /// Every advice artifact one Advisor run has produced, by seat.
    ///
    /// Ordered by seat so a disposition over the set is deterministic. An
    /// Advisor that has not reported yet simply has fewer artifacts than it has
    /// seats; nothing here infers a missing one.
    pub fn list_advisor_advice(
        &self,
        project_id: ProjectId,
        advisor_run_id: AdvisorRunId,
    ) -> RepositoryResult<Vec<StoredAdvisorAdvice>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT seat_binding_id, document, document_hash, recorded_at
                 FROM advisor_advice_artifacts
                 WHERE project_id = ?1 AND advisor_run_id = ?2
                 ORDER BY seat_binding_id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), advisor_run_id.to_string()])
            .map_err(backend)?;
        let mut advice = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            advice.push(read_advisor_advice(
                project_id,
                advisor_run_id,
                (
                    row.get::<_, String>(0).map_err(backend)?,
                    row.get::<_, String>(1).map_err(backend)?,
                    row.get::<_, String>(2).map_err(backend)?,
                    row.get::<_, String>(3).map_err(backend)?,
                ),
            )?);
        }
        Ok(advice)
    }

    /// Atomically append one Advisor *seat's* immutable output and advance the
    /// run's revision.
    ///
    /// Idempotency is per exact seat: an identical document from the same seat
    /// is a replay, and a different document from that seat can never replace
    /// what it already said. Another seat of the same run is a different
    /// artifact, not a conflict — an ASW holds one or more independently
    /// reporting seats.
    ///
    /// This deliberately does not settle the run. One seat reporting is one
    /// seat reporting; requiring every configured seat before a disposition is
    /// the disposition authority's rule, and it has no operation that writes
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
                 WHERE project_id = ?1 AND advisor_run_id = ?2 AND seat_binding_id = ?3",
                params![
                    project_id.to_string(),
                    advisor_run_id.to_string(),
                    seat_binding_id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if let Some(existing_hash) = existing {
            if existing_hash != document_hash.as_str() {
                return Err(RepositoryError::Conflict {
                    subject: "Advisor advice",
                    rule: "this Advisor seat already submitted different immutable output",
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

    /// Record one bounded Committee remediation and terminally settle its
    /// failed round. A re-review is a separate Committee run; this transition
    /// never mutates the failed run into a new mutable round.
    #[allow(clippy::too_many_arguments)]
    pub fn remediate_committee_run(
        &self,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
        expected_revision: AggregateRevision,
        from_round: u32,
        recommendation: &BoundedText,
        tried_path: &BoundedText,
        document: &serde_json::Value,
        document_hash: &ContentHash,
        failed_result: &serde_json::Value,
        failed_result_hash: &ContentHash,
        recorded_at: Timestamp,
    ) -> RepositoryResult<StoredConsultationRun> {
        let encoded = canonical_json(document, "Committee remediation")?;
        if ContentHash::of(encoded.as_bytes()) != *document_hash {
            return Err(RepositoryError::Conflict {
                subject: "Committee remediation",
                rule: "the remediation bytes do not match their stored hash",
            });
        }
        let failed_result_encoded = canonical_json(failed_result, "Committee result")?;
        if ContentHash::of(failed_result_encoded.as_bytes()) != *failed_result_hash {
            return Err(RepositoryError::Conflict {
                subject: "Committee result",
                rule: "the failed result bytes do not match their stored hash",
            });
        }
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO committee_remediations
                     (committee_run_id, project_id, from_round, recommendation,
                      tried_path, document, document_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    committee_run_id.to_string(),
                    project_id.to_string(),
                    i64::from(from_round),
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
                 SET state = 'settled', result = ?4, result_hash = ?5,
                     revision = revision + 1, updated_at = ?6, settled_at = ?6
                 WHERE project_id = ?1 AND run_id = ?2 AND family = 'committee'
                   AND round = ?7 AND state = 'awaiting_judge' AND revision = ?3",
                params![
                    project_id.to_string(),
                    committee_run_id.to_string(),
                    i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    failed_result_encoded,
                    failed_result_hash.as_str(),
                    text(recorded_at),
                    i64::from(from_round),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "Committee remediation",
                rule: "only the expected awaiting-judge round may be terminally settled",
            });
        }
        transaction.commit().map_err(backend)?;
        self.get_consultation_run(project_id, ConsultationRunId::Committee(committee_run_id))?
            .ok_or(RepositoryError::NotFound {
                subject: "consultation run",
            })
    }

    /// Read the immutable remediation document, when one was recorded.
    pub fn get_committee_remediation(
        &self,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
    ) -> RepositoryResult<Option<serde_json::Value>> {
        Ok(self
            .get_committee_remediation_with_hash(project_id, committee_run_id)?
            .map(|(document, _)| document))
    }

    /// Read the immutable remediation and prove its stored bytes match the
    /// stored digest before exposing either to a caller.
    pub fn get_committee_remediation_with_hash(
        &self,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
    ) -> RepositoryResult<Option<(serde_json::Value, ContentHash)>> {
        let encoded = self
            .connection
            .query_row(
                "SELECT document, document_hash FROM committee_remediations
                 WHERE project_id = ?1 AND committee_run_id = ?2",
                params![project_id.to_string(), committee_run_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(backend)?;
        encoded
            .map(|(document, hash)| {
                let hash = ContentHash::parse(&hash)?;
                let canonical = CanonicalDocument::from_stored(&document, &hash)?;
                let value = serde_json::from_str(canonical.json()).map_err(|error| {
                    RepositoryError::Backend {
                        detail: format!("a Committee remediation could not be decoded: {error}"),
                    }
                })?;
                Ok((value, hash))
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

    /// Read one bounded, stable page of completion runs for resident
    /// reconciliation.
    pub fn list_epic_completions_after(
        &self,
        after: Option<(ProjectId, MiniProjectId)>,
        limit: u32,
    ) -> RepositoryResult<Vec<StoredEpicCompletion>> {
        let (project_cursor, epic_cursor) = after.map_or_else(
            || (String::new(), String::new()),
            |(project, epic)| (project.to_string(), epic.to_string()),
        );
        let mut statement = self
            .connection
            .prepare(
                "SELECT project_id, mini_project_id, profile_id, profile_version,
                        definition_hash, state, revision, updated_at
                 FROM epic_completion
                 WHERE (project_id, mini_project_id) > (?1, ?2)
                 ORDER BY project_id, mini_project_id
                 LIMIT ?3",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_cursor, epic_cursor, i64::from(limit)])
            .map_err(backend)?;
        let mut runs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let state: String = row.get(5).map_err(backend)?;
            runs.push(StoredEpicCompletion {
                project_id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                mini_project_id: MiniProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                profile_id: ExternalName::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                profile_version: read_version(row.get::<_, i64>(3).map_err(backend)?)?,
                definition_hash: ContentHash::parse(&row.get::<_, String>(4).map_err(backend)?)?,
                state: serde_json::from_str(&state).map_err(|error| RepositoryError::Backend {
                    detail: format!("a stored completion state is unreadable: {error}"),
                })?,
                revision: revision_of(row.get::<_, i64>(6).map_err(backend)?)?,
                updated_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
            });
        }
        Ok(runs)
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

    /// Commit one completion transition, every derived wake, and its command.
    ///
    /// These are one effect: a completion revision without its wake is stranded
    /// state, while a receipt without the revision falsely claims success.
    pub fn commit_epic_completion(
        &self,
        completion: &StoredEpicCompletion,
        write: CompletionWrite,
        wakes: &[StoredCompletionWake],
        command: &NewLocalCommand,
    ) -> RepositoryResult<CommandReceiptId> {
        self.commit_epic_completion_with_profile(completion, write, None, wakes, command)
    }

    /// As [`Self::commit_epic_completion`], additionally publishing the exact
    /// derived profile the new completion pin names in the same transaction.
    pub fn commit_epic_completion_with_profile(
        &self,
        completion: &StoredEpicCompletion,
        write: CompletionWrite,
        derived_profile: Option<&StoredCompletionProfile>,
        wakes: &[StoredCompletionWake],
        command: &NewLocalCommand,
    ) -> RepositoryResult<CommandReceiptId> {
        let transaction = self.begin()?;
        let target = AggregateRef::MiniProject {
            mini_project_id: completion.mini_project_id,
        };
        if command.project_id != completion.project_id
            || command.kind != CommandKind::AdvanceCompletion
            || command.target != target
        {
            return Err(conflict(
                "epic completion",
                "the completion command must advance this exact project and epic",
            ));
        }
        if wakes.iter().any(|wake| {
            wake.project_id != completion.project_id
                || wake.mini_project_id != completion.mini_project_id
                || wake.completion_revision != completion.revision
                || wake.receipt != completion.definition_hash
        }) {
            return Err(conflict(
                "epic completion wake",
                "every wake must describe this exact completion revision and definition",
            ));
        }
        if let Some(profile) = derived_profile {
            if profile.project_id != completion.project_id
                || profile.id != completion.profile_id
                || profile.version != completion.profile_version
                || profile.definition_hash != completion.definition_hash
            {
                return Err(conflict(
                    "completion profile revision",
                    "the derived profile must be the exact new completion pin",
                ));
            }
            let serialized_definition = profile.definition.to_string();
            let existing = transaction
                .query_row(
                    "SELECT name, definition, definition_hash
                     FROM completion_profile_revisions
                     WHERE project_id = ?1 AND id = ?2 AND version = ?3",
                    params![
                        profile.project_id.to_string(),
                        profile.id.as_str(),
                        version_column(profile.version),
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(backend)?;
            match existing {
                Some((name, definition, hash))
                    if name == profile.name.as_str()
                        && definition == serialized_definition
                        && hash == profile.definition_hash.as_str() => {}
                Some(_) => {
                    return Err(conflict(
                        "completion profile revision",
                        "this derived profile identity already names different content",
                    ));
                }
                None => {
                    transaction
                        .execute(
                            "INSERT INTO completion_profile_revisions
                                 (project_id, id, version, name, definition, definition_hash,
                                  published_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            params![
                                profile.project_id.to_string(),
                                profile.id.as_str(),
                                version_column(profile.version),
                                profile.name.as_str(),
                                serialized_definition,
                                profile.definition_hash.as_str(),
                                text(profile.published_at),
                            ],
                        )
                        .map_err(backend)?;
                }
            }
        }
        match write {
            CompletionWrite::Create => {
                transaction
                    .execute(
                        "INSERT INTO epic_completion
                         (project_id, mini_project_id, profile_id, profile_version,
                          definition_hash, state, revision, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            completion.project_id.to_string(),
                            completion.mini_project_id.to_string(),
                            completion.profile_id.as_str(),
                            version_column(completion.profile_version),
                            completion.definition_hash.as_str(),
                            completion.state.to_string(),
                            revision_column(completion.revision)?,
                            text(completion.updated_at),
                        ],
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::SqliteFailure(failure, _)
                            if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                        {
                            conflict("epic completion", "one epic has exactly one completion run")
                        }
                        other => backend(other),
                    })?;
            }
            CompletionWrite::Advance(expected) => {
                let changed = transaction
                    .execute(
                        "UPDATE epic_completion
                     SET profile_id = ?3, profile_version = ?4, definition_hash = ?5,
                         state = ?6, revision = ?7, updated_at = ?8
                     WHERE project_id = ?1 AND mini_project_id = ?2 AND revision = ?9",
                        params![
                            completion.project_id.to_string(),
                            completion.mini_project_id.to_string(),
                            completion.profile_id.as_str(),
                            version_column(completion.profile_version),
                            completion.definition_hash.as_str(),
                            completion.state.to_string(),
                            revision_column(completion.revision)?,
                            text(completion.updated_at),
                            revision_column(expected)?,
                        ],
                    )
                    .map_err(backend)?;
                if changed != 1 {
                    let exists = transaction
                        .query_row(
                            "SELECT 1 FROM epic_completion
                         WHERE project_id = ?1 AND mini_project_id = ?2",
                            params![
                                completion.project_id.to_string(),
                                completion.mini_project_id.to_string()
                            ],
                            |_| Ok(()),
                        )
                        .optional()
                        .map_err(backend)?
                        .is_some();
                    return Err(if exists {
                        RepositoryError::Conflict {
                            subject: "epic completion",
                            rule: "the completion run moved since the caller read it",
                        }
                    } else {
                        RepositoryError::NotFound {
                            subject: "epic completion",
                        }
                    });
                }
            }
            CompletionWrite::Unchanged => {
                let stored = epic_completion_in(
                    &transaction,
                    completion.project_id,
                    completion.mini_project_id,
                )?
                .ok_or(RepositoryError::NotFound {
                    subject: "epic completion",
                })?;
                if stored != *completion {
                    return Err(conflict(
                        "epic completion",
                        "a no-change completion commit must exactly match persisted state",
                    ));
                }
            }
        }
        for wake in wakes {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO epic_completion_wakes
                         (project_id, mini_project_id, completion_revision, reason,
                          seat_binding_id, receipt, appended_at, acknowledged_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        wake.project_id.to_string(),
                        wake.mini_project_id.to_string(),
                        revision_column(wake.completion_revision)?,
                        wake.reason.as_str(),
                        wake.seat_binding_id.to_string(),
                        wake.receipt.as_str(),
                        text(wake.appended_at),
                        wake.acknowledged_at.map(text),
                    ],
                )
                .map_err(backend)?;
        }
        let receipt_id = match crate::commands::intent::insert_local_command(&transaction, command)?
        {
            Some(existing) => existing.id,
            None => command.receipt_id,
        };
        ensure_receipt_authorizes(
            &transaction,
            "epic completion",
            completion.project_id,
            receipt_id,
            CommandKind::AdvanceCompletion,
            target,
        )?;
        transaction.commit().map_err(backend)?;
        Ok(receipt_id)
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

    /// Read the newest logical wake for one persistent TPM seat.
    ///
    /// Older unacknowledged rows are intentional audit history: the newest
    /// completion projection subsumes them and is the only one a recovered
    /// native successor needs to receive.
    pub fn latest_completion_wake(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        seat_binding_id: SeatBindingId,
    ) -> RepositoryResult<Option<StoredCompletionWake>> {
        let row = self
            .connection
            .query_row(
                "SELECT completion_revision, reason, receipt, appended_at, acknowledged_at
                 FROM epic_completion_wakes
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND seat_binding_id = ?3
                 ORDER BY completion_revision DESC, appended_at DESC, reason DESC
                 LIMIT 1",
                params![
                    project_id.to_string(),
                    mini_project_id.to_string(),
                    seat_binding_id.to_string(),
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(|columns| {
            Ok(StoredCompletionWake {
                project_id,
                mini_project_id,
                completion_revision: AggregateRevision::parse(
                    u64::try_from(columns.0).unwrap_or_default(),
                )?,
                reason: ExternalName::parse(&columns.1)?,
                seat_binding_id,
                receipt: ContentHash::parse(&columns.2)?,
                appended_at: read_timestamp(&columns.3)?,
                acknowledged_at: columns.4.as_deref().map(read_timestamp).transpose()?,
            })
        })
        .transpose()
    }

    /// Claim or replay one exact-native delivery of the newest wake.
    ///
    /// The transaction rechecks both moving facts: the wake must still be the
    /// newest for this epic/seat and the hosted-seat row must still name this
    /// native identity and occupancy generation. A racing caller reads back the
    /// winner's stable message id/body instead of minting a second effect.
    pub fn claim_completion_wake_delivery(
        &self,
        candidate: &StoredCompletionWakeDelivery,
    ) -> RepositoryResult<StoredCompletionWakeDelivery> {
        let parsed_message_id = Uuid::try_parse(&candidate.message_id).map_err(|_| {
            conflict(
                "completion wake delivery",
                "the message id must be a canonical UUIDv7",
            )
        })?;
        if parsed_message_id.get_version_num() != 7
            || parsed_message_id.as_hyphenated().to_string() != candidate.message_id
        {
            return Err(conflict(
                "completion wake delivery",
                "the message id must be a canonical UUIDv7",
            ));
        }
        if ContentHash::of(candidate.body.as_str().as_bytes()) != candidate.body_hash {
            return Err(conflict(
                "completion wake delivery",
                "the frozen body does not match its digest",
            ));
        }
        if candidate.acknowledged_at.is_some()
            || candidate.timeline_epoch.is_some()
            || candidate.timeline_sequence.is_some()
        {
            return Err(conflict(
                "completion wake delivery",
                "a new delivery claim must be pending",
            ));
        }
        let transaction = self.begin()?;
        let newest: Option<(i64, String, String, String)> = transaction
            .query_row(
                "SELECT completion_revision, reason, receipt, appended_at
                 FROM epic_completion_wakes
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND seat_binding_id = ?3
                 ORDER BY completion_revision DESC, appended_at DESC, reason DESC
                 LIMIT 1",
                params![
                    candidate.wake.project_id.to_string(),
                    candidate.wake.mini_project_id.to_string(),
                    candidate.wake.seat_binding_id.to_string(),
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        if newest
            != Some((
                i64::try_from(candidate.wake.completion_revision.get()).unwrap_or(i64::MAX),
                candidate.wake.reason.as_str().to_owned(),
                candidate.wake.receipt.as_str().to_owned(),
                text(candidate.wake.appended_at),
            ))
        {
            return Err(RepositoryError::Conflict {
                subject: "completion wake delivery",
                rule: "the named wake is no longer the newest completion projection",
            });
        }
        let active: Option<(String, String, i64, String, i64)> = transaction
            .query_row(
                "SELECT runtime_kind, host, generation, native_id,
                        1 + (SELECT COUNT(*) FROM hosted_topology_seat_history h
                             WHERE h.project_id = s.project_id
                               AND h.seat_binding_id = s.seat_binding_id)
                 FROM hosted_topology_seats s
                 WHERE project_id = ?1 AND seat_binding_id = ?2",
                params![
                    candidate.wake.project_id.to_string(),
                    candidate.wake.seat_binding_id.to_string(),
                ],
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
        let expected = (
            candidate.native_identity.runtime_kind.as_str().to_owned(),
            candidate.native_identity.host.as_str().to_owned(),
            i64::try_from(candidate.native_identity.generation).unwrap_or(i64::MAX),
            candidate.native_identity.native_id.as_str().to_owned(),
            i64::try_from(candidate.occupancy_generation).unwrap_or(i64::MAX),
        );
        if active != Some(expected) {
            return Err(RepositoryError::Conflict {
                subject: "completion wake delivery",
                rule: "the exact hosted TPM native occupancy moved before delivery",
            });
        }
        transaction
            .execute(
                "INSERT INTO epic_completion_wake_deliveries
                     (project_id, mini_project_id, completion_revision, reason,
                      seat_binding_id, occupancy_generation, runtime_kind, host,
                      runtime_generation, native_id, message_id, body, body_hash,
                      created_at, acknowledged_at, timeline_epoch, timeline_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         NULL, NULL, NULL)
                 ON CONFLICT (project_id, mini_project_id, completion_revision, reason,
                              seat_binding_id, occupancy_generation, native_id) DO NOTHING",
                params![
                    candidate.wake.project_id.to_string(),
                    candidate.wake.mini_project_id.to_string(),
                    i64::try_from(candidate.wake.completion_revision.get()).unwrap_or(i64::MAX),
                    candidate.wake.reason.as_str(),
                    candidate.wake.seat_binding_id.to_string(),
                    i64::try_from(candidate.occupancy_generation).unwrap_or(i64::MAX),
                    candidate.native_identity.runtime_kind.as_str(),
                    candidate.native_identity.host.as_str(),
                    i64::try_from(candidate.native_identity.generation).unwrap_or(i64::MAX),
                    candidate.native_identity.native_id.as_str(),
                    candidate.message_id,
                    candidate.body.as_str(),
                    candidate.body_hash.as_str(),
                    text(candidate.created_at),
                ],
            )
            .map_err(backend)?;
        let delivery = read_completion_wake_delivery(
            &transaction,
            candidate.wake.project_id,
            candidate.wake.mini_project_id,
            candidate.wake.completion_revision,
            &candidate.wake.reason,
            candidate.wake.seat_binding_id,
            candidate.occupancy_generation,
            &candidate.native_identity.native_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "completion wake delivery",
        })?;
        transaction.commit().map_err(backend)?;
        Ok(delivery)
    }

    /// Record canonical timeline evidence for one exact delivery.
    pub fn acknowledge_completion_wake_delivery(
        &self,
        delivery: &StoredCompletionWakeDelivery,
        acknowledged_at: Timestamp,
        timeline_epoch: u64,
        timeline_sequence: u64,
    ) -> RepositoryResult<()> {
        if timeline_epoch == 0 || timeline_sequence == 0 {
            return Err(conflict(
                "completion wake delivery",
                "canonical timeline coordinates must be positive",
            ));
        }
        let transaction = self.begin()?;
        let existing = read_completion_wake_delivery(
            &transaction,
            delivery.wake.project_id,
            delivery.wake.mini_project_id,
            delivery.wake.completion_revision,
            &delivery.wake.reason,
            delivery.wake.seat_binding_id,
            delivery.occupancy_generation,
            &delivery.native_identity.native_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "completion wake delivery",
        })?;
        if existing.message_id != delivery.message_id
            || existing.body != delivery.body
            || existing.body_hash != delivery.body_hash
            || existing.native_identity != delivery.native_identity
            || existing.wake.receipt != delivery.wake.receipt
            || existing.wake.appended_at != delivery.wake.appended_at
        {
            return Err(conflict(
                "completion wake delivery",
                "the acknowledgement does not name the frozen delivery",
            ));
        }
        if let Some(existing_at) = existing.acknowledged_at
            && (existing_at != acknowledged_at
                || existing.timeline_epoch != Some(timeline_epoch)
                || existing.timeline_sequence != Some(timeline_sequence))
        {
            return Err(conflict(
                "completion wake delivery",
                "canonical acknowledgement evidence cannot be replaced",
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE epic_completion_wake_deliveries
                 SET acknowledged_at = COALESCE(acknowledged_at, ?8),
                     timeline_epoch = COALESCE(timeline_epoch, ?9),
                     timeline_sequence = COALESCE(timeline_sequence, ?10)
                 WHERE project_id = ?1 AND mini_project_id = ?2
                   AND completion_revision = ?3 AND reason = ?4
                   AND seat_binding_id = ?5 AND occupancy_generation = ?12
                   AND native_id = ?6 AND message_id = ?7 AND body_hash = ?11",
                params![
                    delivery.wake.project_id.to_string(),
                    delivery.wake.mini_project_id.to_string(),
                    i64::try_from(delivery.wake.completion_revision.get()).unwrap_or(i64::MAX),
                    delivery.wake.reason.as_str(),
                    delivery.wake.seat_binding_id.to_string(),
                    delivery.native_identity.native_id.as_str(),
                    delivery.message_id,
                    text(acknowledged_at),
                    i64::try_from(timeline_epoch).unwrap_or(i64::MAX),
                    i64::try_from(timeline_sequence).unwrap_or(i64::MAX),
                    delivery.body_hash.as_str(),
                    i64::try_from(delivery.occupancy_generation).unwrap_or(i64::MAX),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::NotFound {
                subject: "completion wake delivery",
            });
        }
        transaction
            .execute(
                "UPDATE epic_completion_wakes SET acknowledged_at = COALESCE(acknowledged_at, ?6)
                 WHERE project_id = ?1 AND mini_project_id = ?2
                   AND completion_revision = ?3 AND reason = ?4 AND seat_binding_id = ?5",
                params![
                    delivery.wake.project_id.to_string(),
                    delivery.wake.mini_project_id.to_string(),
                    i64::try_from(delivery.wake.completion_revision.get()).unwrap_or(i64::MAX),
                    delivery.wake.reason.as_str(),
                    delivery.wake.seat_binding_id.to_string(),
                    text(acknowledged_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)
    }

    /// Every exact-native delivery retained for one epic, in wake/native order.
    pub fn list_completion_wake_deliveries(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<StoredCompletionWakeDelivery>> {
        let wakes = self.list_completion_wakes(project_id, mini_project_id)?;
        let mut deliveries = Vec::new();
        for wake in wakes {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT occupancy_generation, native_id
                     FROM epic_completion_wake_deliveries
                     WHERE project_id = ?1 AND mini_project_id = ?2
                       AND completion_revision = ?3 AND reason = ?4 AND seat_binding_id = ?5
                     ORDER BY occupancy_generation, native_id",
                )
                .map_err(backend)?;
            let rows = statement
                .query_map(
                    params![
                        project_id.to_string(),
                        mini_project_id.to_string(),
                        i64::try_from(wake.completion_revision.get()).unwrap_or(i64::MAX),
                        wake.reason.as_str(),
                        wake.seat_binding_id.to_string(),
                    ],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(backend)?;
            for row in rows {
                let (occupancy_generation, native_id) = row.map_err(backend)?;
                let occupancy_generation =
                    u64::try_from(occupancy_generation).map_err(|_| RepositoryError::Backend {
                        detail: "a completion wake occupancy generation is invalid".to_owned(),
                    })?;
                let native_id = ExternalId::parse(&native_id)?;
                if let Some(delivery) = read_completion_wake_delivery(
                    &self.connection,
                    project_id,
                    mini_project_id,
                    wake.completion_revision,
                    &wake.reason,
                    wake.seat_binding_id,
                    occupancy_generation,
                    &native_id,
                )? {
                    deliveries.push(delivery);
                }
            }
        }
        Ok(deliveries)
    }

    /// Distinct epic scopes that hold at least one Completion wake.
    pub fn completion_wake_scopes(&self) -> RepositoryResult<Vec<(ProjectId, MiniProjectId)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT DISTINCT project_id, mini_project_id
                 FROM epic_completion_wakes ORDER BY project_id, mini_project_id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;
        let mut scopes = Vec::new();
        for row in rows {
            let (project, epic) = row.map_err(backend)?;
            scopes.push((ProjectId::parse(&project)?, MiniProjectId::parse(&epic)?));
        }
        Ok(scopes)
    }

    /// The distinct artifact-contract keys one task has durable evidence for.
    ///
    /// Keys only. The completion ticket gate asks whether a declared artifact is
    /// evidenced, not what its locator is, and handing it the locators would put
    /// this read in the position of deciding which of several records for one key
    /// counts — a decision the gate does not need and must not make twice.
    ///
    /// Two producer-owned sources are unioned because an artifact leaves a
    /// durable trace in two ordinary delivery paths:
    ///
    /// - `artifact_evidence` — the addressable record: a key plus a locator
    ///   someone can follow. Nothing in the delivery path writes it today.
    /// - `role_turns.artifacts` — the settling role's own declaration of what its
    ///   turn produced.
    ///
    /// Gate evaluations are intentionally absent. Their `evidence` field cites
    /// already-produced artifacts; admitting the citation as production would
    /// let a gate request manufacture the evidence it is meant to inspect.
    /// Evidence drawn from a producer turn is still gated independently by the
    /// profile's required gate states.
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

    /// Check a proposal's existing replay authority before semantic validation.
    ///
    /// This ordering matters for a used key whose caller changes only the failed
    /// evidence hash: it is a conflicting command, not a fresh malformed
    /// proposal. `false` means no claim exists and ordinary validation may
    /// continue.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] for reused authority, a poisoned
    /// partial effect or a proposal already owned by different content.
    pub fn check_remediation_proposal_claim(
        &self,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        completion_generation: u32,
        round: u8,
        effect_revision: AggregateRevision,
    ) -> RepositoryResult<bool> {
        let request = remediation_command_request(self, envelope, project_id, mini_project_id)?;
        let transaction = self.begin()?;
        let stored: Option<(String, String, Option<i64>)> = transaction
            .query_row(
                "SELECT idempotency_key, intent_hash, effect_revision
                   FROM epic_completion_remediation_command_claims
                  WHERE project_id = ?1 AND mini_project_id = ?2
                    AND completion_generation = ?3 AND round = ?4
                    AND action = 'lsa_proposal'",
                params![
                    project_id.to_string(),
                    mini_project_id.to_string(),
                    i64::from(completion_generation),
                    i64::from(round),
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((key, intent_hash, stored_effect_revision)) = stored else {
            return Ok(false);
        };
        if key != request.idempotency_key.as_str()
            || intent_hash != request.intent.hash().as_str()
            || stored_effect_revision
                != Some(i64::try_from(effect_revision.get()).unwrap_or(i64::MAX))
        {
            return Err(conflict(
                "completion remediation command claim",
                "this LSA proposal is already bound to a different key, intent, or effect revision",
            ));
        }
        Ok(true)
    }

    /// Atomically bind and record one LSA proposal with its local command
    /// receipt. An exact claim plus proposal without a receipt is recovered by
    /// materializing the missing receipt without duplicating the proposal.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] for reused authority, a poisoned
    /// partial effect or a proposal already owned by different content.
    pub fn commit_remediation_proposal(
        &self,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
        proposal: &StoredRemediationProposal,
        effect_revision: AggregateRevision,
        permit_create: bool,
    ) -> RepositoryResult<(CommandReceipt, Applied)> {
        let request = remediation_command_request(
            self,
            envelope,
            proposal.project_id,
            proposal.mini_project_id,
        )?;
        let transaction = self.begin()?;
        let claim_existed = remediation_claim(
            &transaction,
            request,
            proposal.mini_project_id,
            proposal.completion_generation,
            proposal.round,
            RemediationCommandAction::LsaProposal,
            Some(effect_revision),
        )?;
        let receipt = crate::commands::intent::insert_local_command(&transaction, request)?;
        let stored = remediation_proposal_in(
            &transaction,
            proposal.project_id,
            proposal.mini_project_id,
            proposal.completion_generation,
            proposal.round,
        )?;
        let applied = match (claim_existed, stored) {
            (true, Some(stored)) if same_remediation_proposal(&stored, proposal) => {
                Applied::Unchanged
            }
            (true, _) => {
                return Err(conflict(
                    "completion remediation command",
                    "the replay claim has no exact durable LSA proposal effect",
                ));
            }
            (false, None) if permit_create && receipt.is_none() => {
                insert_remediation_proposal_in(&transaction, proposal)?;
                Applied::Created
            }
            (false, _) => {
                return Err(conflict(
                    "completion remediation command",
                    "a new proposal cannot claim an existing effect, receipt, or closed phase",
                ));
            }
        };
        let receipt = match receipt {
            Some(receipt) => receipt,
            None => local_command_receipt(&transaction, &request.idempotency_key)?,
        };
        transaction.commit().map_err(backend)?;
        Ok((receipt, applied))
    }

    /// Atomically move completion into remediation, append its TPM wake and
    /// record the exact command receipt.
    ///
    /// When `effect_already_present` is true, recovery is allowed only for an
    /// existing matching command claim and an exact stored completion state at
    /// the claimed effect revision. This is the former effect-without-receipt
    /// crash boundary; a current caller cannot mint replay authority for an
    /// unrelated projection.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] for stale state, mismatched replay
    /// authority, missing effect or a wake that names different evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_remediation_route(
        &self,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
        completion_generation: u32,
        round: u8,
        next: &StoredEpicCompletion,
        expected_revision: AggregateRevision,
        effect_revision: AggregateRevision,
        wake: &StoredCompletionWake,
        effect_already_present: bool,
    ) -> RepositoryResult<(StoredEpicCompletion, CommandReceipt, Applied)> {
        let request =
            remediation_command_request(self, envelope, next.project_id, next.mini_project_id)?;
        if wake.project_id != next.project_id
            || wake.mini_project_id != next.mini_project_id
            || wake.completion_revision != effect_revision
        {
            return Err(conflict(
                "completion remediation route",
                "the completion effect and TPM wake do not describe one revision",
            ));
        }
        let transaction = self.begin()?;
        let claim_existed = remediation_claim(
            &transaction,
            request,
            next.mini_project_id,
            completion_generation,
            round,
            RemediationCommandAction::TpmRoute,
            Some(effect_revision),
        )?;
        let receipt = crate::commands::intent::insert_local_command(&transaction, request)?;
        let current = epic_completion_in(&transaction, next.project_id, next.mini_project_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "epic completion",
            })?;
        let applied = if claim_existed {
            if receipt.is_none()
                && (!effect_already_present
                    || current.revision != effect_revision
                    || current.state != next.state)
            {
                return Err(conflict(
                    "completion remediation command",
                    "the replay claim has no exact durable TPM route effect",
                ));
            }
            Applied::Unchanged
        } else {
            if effect_already_present || receipt.is_some() {
                return Err(conflict(
                    "completion remediation command",
                    "a new TPM route cannot claim an existing effect or receipt",
                ));
            }
            if current.revision != expected_revision {
                return Err(conflict(
                    "epic completion",
                    "the completion run moved since the caller read it",
                ));
            }
            let changed = transaction
                .execute(
                    "UPDATE epic_completion
                        SET state = ?3, revision = ?4, updated_at = ?5
                      WHERE project_id = ?1 AND mini_project_id = ?2 AND revision = ?6",
                    params![
                        next.project_id.to_string(),
                        next.mini_project_id.to_string(),
                        next.state.to_string(),
                        i64::try_from(next.revision.get()).unwrap_or(i64::MAX),
                        text(next.updated_at),
                        i64::try_from(expected_revision.get()).unwrap_or(i64::MAX),
                    ],
                )
                .map_err(backend)?;
            if changed != 1 {
                return Err(conflict(
                    "epic completion",
                    "the completion run moved since the caller read it",
                ));
            }
            Applied::Created
        };
        ensure_completion_wake_in(&transaction, wake)?;
        let receipt = match receipt {
            Some(receipt) => receipt,
            None => local_command_receipt(&transaction, &request.idempotency_key)?,
        };
        let stored = epic_completion_in(&transaction, next.project_id, next.mini_project_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "epic completion",
            })?;
        transaction.commit().map_err(backend)?;
        Ok((stored, receipt, applied))
    }

    /// Read the proposal standing for one epic's failed round.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_remediation_proposal(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        completion_generation: u32,
        round: u8,
    ) -> RepositoryResult<Option<StoredRemediationProposal>> {
        self.connection
            .query_row(
                "SELECT failed_round_evidence, proposal, lsa_seat_binding_id,
                        lsa_occupancy_generation, proposed_at
                 FROM epic_completion_remediation_proposals
                 WHERE project_id = ?1 AND mini_project_id = ?2
                   AND completion_generation = ?3 AND round = ?4",
                params![
                    project_id.to_string(),
                    mini_project_id.to_string(),
                    i64::from(completion_generation),
                    i64::from(round)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(|columns| {
                Ok(StoredRemediationProposal {
                    project_id,
                    mini_project_id,
                    completion_generation,
                    round,
                    failed_round_evidence: ContentHash::parse(&columns.0)?,
                    proposal: ContentHash::parse(&columns.1)?,
                    lsa_seat_binding_id: SeatBindingId::parse(&columns.2)?,
                    lsa_occupancy_generation: u64::try_from(columns.3).map_err(|_| {
                        RepositoryError::Backend {
                            detail: "an LSA proposal occupancy generation is invalid".to_owned(),
                        }
                    })?,
                    proposed_at: read_timestamp(&columns.4)?,
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
                // The unscoped project root outlives individual epic pins.
                // Its direct epic boundary may span revisions of one lineage
                // only when both immutable specifications permit that edge.
                let historical_project_root = if parent.topology != request.topology
                    && parent.lifecycle == TopologyLifecycle::Active
                    && parent.topology.spec_id == request.topology.spec_id
                    && parent.mini_project_id.is_none()
                    && parent.parent_id.is_none()
                    && request.mini_project_id.is_some()
                    && request.task_id.is_none()
                    && parent.kind == spec.root_kind
                {
                    let parent_spec =
                        topology_spec_in(&transaction, request.project_id, &parent.topology)?;
                    parent.kind == parent_spec.root_kind
                        && parent_spec.node_kinds.iter().any(|kind| {
                            kind.kind == request.kind && kind.allowed_parents.contains(&parent.kind)
                        })
                } else {
                    false
                };
                if parent.lifecycle.is_terminal()
                    || parent
                        .mini_project_id
                        .is_some_and(|scope| Some(scope) != request.mini_project_id)
                    || (parent.topology != request.topology && !historical_project_root)
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

        ensure_no_live_native_migration_for_node(&transaction, project_id, id)?;

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
        if observation.released_at.is_some() {
            let topology_node_id: Option<String> = transaction
                .query_row(
                    "SELECT topology_node_id FROM seat_bindings
                     WHERE project_id = ?1 AND id = ?2",
                    params![project_id.to_string(), id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let topology_node_id = topology_node_id.ok_or(RepositoryError::NotFound {
                subject: "seat binding",
            })?;
            ensure_no_live_native_migration_for_node(
                &transaction,
                project_id,
                TopologyNodeId::parse(&topology_node_id)?,
            )?;
        }
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
    credit_reserve_minor_units, credit_currency, provenance_id";

/// The window set for one pair, ordered by kind so a stored row and a re-read of
/// it are byte-identical.
const PROVIDER_QUOTA_WINDOW_COLUMNS: &str = "kind, resets_at, used_percent";

const PROVIDER_USAGE_OBSERVATION_COLUMNS: &str = "id, project_id, account_profile_id, provider, \
    evidence_hash, state, resets_at, windows, observed_at";

/// A `u64` this database can store without loss.
///
/// Clamping to `i64::MAX` was worse than a refusal: it wrote a number nobody
/// observed and made the row look authoritative. Provenance that cannot be
/// stored exactly is not provenance.
fn storable_u64(field: &'static str, value: u64) -> RepositoryResult<i64> {
    i64::try_from(value).map_err(|_| {
        RepositoryError::from(DomainError::invalid_at(
            "QuotaObservationProvenance",
            field.to_owned(),
            "exceeds what this database stores exactly",
        ))
    })
}

/// A stored integer read back as `u64`, refusing a corrupt negative.
fn stored_u64(field: &'static str, value: i64) -> RepositoryResult<u64> {
    u64::try_from(value).map_err(|_| {
        RepositoryError::from(DomainError::invalid_at(
            "QuotaObservationProvenance",
            field.to_owned(),
            "is stored negative and cannot be read back",
        ))
    })
}

/// A stored integer read back as `u32`, refusing anything out of range.
fn stored_u32(field: &'static str, value: i64) -> RepositoryResult<u32> {
    u32::try_from(value).map_err(|_| {
        RepositoryError::from(DomainError::invalid_at(
            "QuotaObservationProvenance",
            field.to_owned(),
            "is stored outside the range it can be read back in",
        ))
    })
}

fn read_quota_observation_provenance(
    row: &Row<'_>,
    source_sequences: Vec<(u64, u64)>,
) -> RepositoryResult<QuotaObservationProvenance> {
    let parsed_resets_at: Option<String> = row.get(19).map_err(backend)?;
    let reset_zone: Option<String> = row.get(20).map_err(backend)?;
    // What the record declared its set to be, held against what was read. The
    // schema seals the collection, and this is the reader refusing to present a
    // set that somehow disagrees with the seal rather than quietly shrinking it.
    let declared = stored_u64(
        "source_range_count",
        row.get::<_, i64>(21).map_err(backend)?,
    )?;
    if declared != source_sequences.len() as u64 {
        return Err(DomainError::invalid_at(
            "QuotaObservationProvenance",
            "source_range_count".to_owned(),
            "the stored range set must be exactly the set the record declared",
        )
        .into());
    }
    Ok(QuotaObservationProvenance {
        record: kontor_core::repository::NewQuotaObservationProvenance {
            id: QuotaObservationProvenanceId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
            project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
            account_profile_id: AccountProfileId::parse(
                &row.get::<_, String>(2).map_err(backend)?,
            )?,
            provider: row.get::<_, String>(3).map_err(backend)?,
            signal_id: row.get::<_, String>(4).map_err(backend)?,
            signal_version: kontor_core::id::SpecVersion::parse(stored_u32(
                "signal_version",
                row.get::<_, i64>(5).map_err(backend)?,
            )?)?,
            signal_definition_hash: ContentHash::parse(&row.get::<_, String>(6).map_err(backend)?)?,
            agent_run_id: AgentRunId::parse(&row.get::<_, String>(7).map_err(backend)?)?,
            runtime_binding_id: kontor_core::id::RuntimeBindingId::parse(
                &row.get::<_, String>(8).map_err(backend)?,
            )?,
            native_id: ExternalId::parse(&row.get::<_, String>(9).map_err(backend)?)?,
            binding_generation: stored_u64(
                "binding_generation",
                row.get::<_, i64>(10).map_err(backend)?,
            )?,
            runtime_observation_cursor: row
                .get::<_, Option<i64>>(11)
                .map_err(backend)?
                .map(EventCursor::parse)
                .transpose()?,
            item_epoch: stored_u64("item_epoch", row.get::<_, i64>(12).map_err(backend)?)?,
            item_seq_start: stored_u64("item_seq_start", row.get::<_, i64>(13).map_err(backend)?)?,
            item_seq_end: stored_u64("item_seq_end", row.get::<_, i64>(14).map_err(backend)?)?,
            source_sequences,
            item_kind: row.get::<_, String>(15).map_err(backend)?,
            item_observed_at: read_timestamp(&row.get::<_, String>(16).map_err(backend)?)?,
            decision_basis: kontor_core::spec::QuotaDecisionBasis::parse(
                &row.get::<_, String>(17).map_err(backend)?,
            )?,
            decided_state: ProviderQuotaKind::parse(&row.get::<_, String>(18).map_err(backend)?)?,
            parsed_resets_at: parsed_resets_at
                .as_deref()
                .map(read_timestamp)
                .transpose()?,
            reset_zone,
            evidence_digest: ContentHash::parse(&row.get::<_, String>(22).map_err(backend)?)?,
            recorded_at: read_timestamp(&row.get::<_, String>(23).map_err(backend)?)?,
        },
    })
}

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
        provenance_id: row
            .get::<_, Option<String>>(13)
            .map_err(backend)?
            .as_deref()
            .map(QuotaObservationProvenanceId::parse)
            .transpose()?,
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

fn read_provider_usage_observation(row: &Row<'_>) -> RepositoryResult<ProviderUsageObservation> {
    let resets_at: Option<String> = row.get(6).map_err(backend)?;
    let windows: String = row.get(7).map_err(backend)?;
    let windows = serde_json::from_str(&windows).map_err(|_| RepositoryError::Conflict {
        subject: "provider usage observation",
        rule: "the stored window document is not readable",
    })?;
    Ok(ProviderUsageObservation {
        id: ProviderUsageObservationId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        account_profile_id: AccountProfileId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        provider: row.get(3).map_err(backend)?,
        evidence_hash: ContentHash::parse(&row.get::<_, String>(4).map_err(backend)?)?,
        state: ProviderQuotaKind::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        resets_at: resets_at.as_deref().map(read_timestamp).transpose()?,
        windows,
        observed_at: read_timestamp(&row.get::<_, String>(8).map_err(backend)?)?,
    })
}

fn validate_provider_quota_state(request: &NewProviderQuotaState) -> RepositoryResult<()> {
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
    if let Some(credit) = request.credit
        && credit.remaining.currency != credit.reserve.currency
    {
        return Err(RepositoryError::Domain(DomainError::invalid(
            "CreditBalance",
            "a balance and its reserve must be in one currency; they are never converted",
        )));
    }
    let mut kinds: Vec<QuotaWindowKind> =
        request.windows.iter().map(|window| window.kind).collect();
    kinds.sort_unstable();
    if kinds.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(RepositoryError::Domain(DomainError::invalid(
            "ProviderQuotaState",
            "one window kind may be observed only once per account and provider",
        )));
    }
    if request
        .windows
        .iter()
        .any(|window| window.used_percent > 100)
    {
        return Err(RepositoryError::Domain(DomainError::invalid(
            "QuotaWindow",
            "a consumption share must be a percentage",
        )));
    }
    Ok(())
}

/// Append one immutable provenance record.
///
/// Insert-only by construction: the table's own triggers refuse an update or a
/// delete, so a repeated id is a genuine conflict rather than something to
/// paper over.
fn insert_quota_observation_provenance_in(
    transaction: &Transaction<'_>,
    record: &kontor_core::repository::NewQuotaObservationProvenance,
    assigned_cursor: Option<EventCursor>,
) -> RepositoryResult<()> {
    // The exact set, in configured order, checked against the envelope before
    // anything lands. Without this an empty or inconsistent child set could sit
    // beside authoritative envelope scalars and nobody would notice.
    let refuse = |rule: &'static str| -> RepositoryError {
        DomainError::invalid_at(
            "QuotaObservationProvenance",
            "source_sequences".to_owned(),
            rule,
        )
        .into()
    };
    if record.source_sequences.is_empty() {
        return Err(refuse("an item covers at least one sequence range"));
    }
    let mut previous_end: Option<u64> = None;
    for (start, end) in &record.source_sequences {
        if start > end {
            return Err(refuse("a range ends before it starts"));
        }
        if previous_end.is_some_and(|last| *start <= last) {
            return Err(refuse("ranges must be ordered and disjoint"));
        }
        previous_end = Some(*end);
    }
    let first = record.source_sequences[0].0;
    let last = record
        .source_sequences
        .last()
        .map(|(_, end)| *end)
        .unwrap_or(first);
    if first != record.item_seq_start || last != record.item_seq_end {
        return Err(refuse(
            "the envelope must be exactly the first and last bound",
        ));
    }
    let runtime_observation_cursor = match (record.runtime_observation_cursor, assigned_cursor) {
        (Some(recorded), Some(assigned)) if recorded != assigned => {
            return Err(DomainError::invalid_at(
                "QuotaObservationProvenance",
                "runtime_observation_cursor".to_owned(),
                "must equal the control event allocated by the observation transaction",
            )
            .into());
        }
        (Some(recorded), _) => recorded,
        (None, Some(assigned)) => assigned,
        (None, None) => {
            return Err(DomainError::MissingEvidence {
                subject: "quota observation provenance",
                rule: "a runtime observation must cite its exact control event cursor",
            }
            .into());
        }
    };
    transaction
        .execute(
            "INSERT INTO provider_quota_observation_provenance
                 (id, project_id, account_profile_id, provider, signal_id, signal_version,
                  signal_definition_hash, agent_run_id, runtime_binding_id, native_id,
                  binding_generation, runtime_observation_cursor, item_epoch, item_seq_start,
                  item_seq_end, item_kind, item_observed_at, decision_basis, decided_state,
                  parsed_resets_at, reset_zone, source_range_count, evidence_digest, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                     ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            params![
                record.id.to_string(),
                record.project_id.to_string(),
                record.account_profile_id.to_string(),
                record.provider.as_str(),
                record.signal_id.as_str(),
                i64::from(record.signal_version.get()),
                record.signal_definition_hash.as_str(),
                record.agent_run_id.to_string(),
                record.runtime_binding_id.to_string(),
                record.native_id.as_str(),
                storable_u64("binding_generation", record.binding_generation)?,
                runtime_observation_cursor.get(),
                storable_u64("item_epoch", record.item_epoch)?,
                storable_u64("item_seq_start", record.item_seq_start)?,
                storable_u64("item_seq_end", record.item_seq_end)?,
                record.item_kind.as_str(),
                text(record.item_observed_at),
                record.decision_basis.as_str(),
                record.decided_state.as_str(),
                record.parsed_resets_at.map(text),
                record.reset_zone.as_deref(),
                storable_u64("source_range_count", record.source_sequences.len() as u64)?,
                record.evidence_digest.as_str(),
                text(record.recorded_at),
            ],
        )
        .map_err(backend)?;
    for (ordinal, (start, end)) in record.source_sequences.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO provider_quota_observation_source_ranges
                     (provenance_id, project_id, ordinal, seq_start, seq_end)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    record.id.to_string(),
                    record.project_id.to_string(),
                    storable_u64("ordinal", ordinal as u64)?,
                    storable_u64("seq_start", *start)?,
                    storable_u64("seq_end", *end)?,
                ],
            )
            .map_err(backend)?;
    }
    Ok(())
}

pub(crate) fn set_provider_quota_state_in(
    transaction: &Transaction<'_>,
    request: &NewProviderQuotaState,
    runtime_observation_cursor: Option<EventCursor>,
) -> RepositoryResult<ProviderQuotaState> {
    validate_provider_quota_state(request)?;
    if read_account_profile_in(transaction, request.project_id, request.account_profile_id)?
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

    // Cross-producer recency. A run's native sequence orders that run's own
    // events and says nothing about this `(account, provider)` pair, which
    // several producers write: the usage poller's `ProviderReport`, an
    // operator's override, and any run that happens to hold the account. So a
    // refusal can be the newest reducible event *for its run* and still be
    // older than a `ProviderReport` that already restored availability — and
    // taking the current revision, as a caller must, would quietly overwrite
    // the newer truth with the older one.
    //
    // Only a `RuntimeObservation` is fenced here, and deliberately: it is the
    // one source that learns anything only *after* something was refused, so it
    // is always describing a moment that has already passed. A `ProviderReport`
    // is a structured answer about now and is the only source that can move a
    // state back to available without a human, so on an equal instant it wins.
    // An operator override is a judgement and is never fenced by a machine.
    //
    // Regression is a no-op rather than an error: the caller's conclusion was
    // true when it was observed, it is simply no longer current, and failing
    // the whole observation transaction over stale-but-honest evidence would
    // lose the runtime event as well.
    if request.source == kontor_core::spec::ProviderQuotaSource::RuntimeObservation
        && let Some(existing) = current.as_ref()
        && (existing.observed_at > request.observed_at
            || (existing.observed_at == request.observed_at
                && existing.source == kontor_core::spec::ProviderQuotaSource::ProviderReport))
    {
        return Ok(existing.clone());
    }

    let next = match &current {
        Some(existing) => {
            existing
                .revision
                .expect("provider quota state", request.expected_revision)?;
            existing.revision.next()?
        }
        None => {
            AggregateRevision::INITIAL.expect("provider quota state", request.expected_revision)?;
            AggregateRevision::INITIAL
        }
    };
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
    // Provenance first, in this same transaction. The row's reference is a real
    // foreign key, so the record has to exist before the row can point at it --
    // and a failure anywhere below rolls both back together, which is the whole
    // point: a block whose cited evidence never landed is a block nobody can
    // explain.
    // A record that does not describe *this* decision is not provenance for it.
    // Every field the two share is compared before either lands, so a mismatched
    // record cannot be attached to a row it never authorized.
    if let Some(record) = request.provenance.as_ref() {
        let disagreement = if record.project_id != request.project_id {
            Some("project_id")
        } else if record.account_profile_id != request.account_profile_id {
            Some("account_profile_id")
        } else if record.provider != request.provider {
            Some("provider")
        } else if record.decided_state != request.state {
            Some("decided_state")
        } else if record.parsed_resets_at != request.resets_at {
            Some("parsed_resets_at")
        } else if record.evidence_digest != request.evidence_hash {
            Some("evidence_digest")
        } else {
            None
        };
        if let Some(field) = disagreement {
            return Err(DomainError::invalid_at(
                "QuotaObservationProvenance",
                field.to_owned(),
                "does not match the quota state it would authorize",
            )
            .into());
        }
        // Only a runtime observation produces one. A provider report or an
        // operator assertion citing a runtime item would be a claim neither of
        // them made.
        if request.source != kontor_core::spec::ProviderQuotaSource::RuntimeObservation {
            return Err(DomainError::invalid_at(
                "NewProviderQuotaState",
                "provenance".to_owned(),
                "only a runtime observation may write provenance",
            )
            .into());
        }
    } else if request.source == kontor_core::spec::ProviderQuotaSource::RuntimeObservation {
        // The converse, and it holds whether or not a row already exists. An
        // accepted runtime observation that carried none would clear
        // `provenance_id` and leave a *new* runtime decision nobody can
        // explain. A stale one never reaches here: the recency rule above
        // returns before any write.
        return Err(DomainError::invalid_at(
            "NewProviderQuotaState",
            "provenance".to_owned(),
            "a runtime observation must carry the provenance it rests on",
        )
        .into());
    }
    let provenance_id = match request.provenance.as_ref() {
        Some(record) => {
            insert_quota_observation_provenance_in(
                transaction,
                record,
                runtime_observation_cursor,
            )?;
            Some(record.id.to_string())
        }
        None => None,
    };
    transaction
        .execute(
            "INSERT INTO provider_quota_states
                 (project_id, account_profile_id, provider, state, resets_at, evidence_hash,
                  source, observed_at, revision, updated_at, credit_minor_units,
                  credit_reserve_minor_units, credit_currency, provenance_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
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
                 credit_currency = excluded.credit_currency,
                 provenance_id = excluded.provenance_id",
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
                provenance_id,
            ],
        )
        .map_err(backend)?;
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
    attach_provider_quota_windows(transaction, &mut stored)?;
    Ok(stored)
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
        let transaction = self.begin()?;
        let stored = set_provider_quota_state_in(&transaction, request, None)?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn get_quota_observation_provenance(
        &self,
        project_id: ProjectId,
        id: QuotaObservationProvenanceId,
    ) -> RepositoryResult<Option<QuotaObservationProvenance>> {
        // The exact set first: a record read without it would report an envelope
        // as though it were the sequences the item carried.
        let mut statement = self
            .connection
            .prepare(
                "SELECT seq_start, seq_end
                   FROM provider_quota_observation_source_ranges
                  WHERE project_id = ?1 AND provenance_id = ?2
                  ORDER BY ordinal",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), id.to_string()])
            .map_err(backend)?;
        let mut ranges: Vec<(u64, u64)> = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            ranges.push((
                stored_u64("seq_start", row.get::<_, i64>(0).map_err(backend)?)?,
                stored_u64("seq_end", row.get::<_, i64>(1).map_err(backend)?)?,
            ));
        }
        drop(rows);
        drop(statement);
        self.connection
            .query_row(
                "SELECT id, project_id, account_profile_id, provider, signal_id, signal_version,
                        signal_definition_hash, agent_run_id, runtime_binding_id, native_id,
                        binding_generation, runtime_observation_cursor, item_epoch,
                        item_seq_start, item_seq_end, item_kind,
                        item_observed_at, decision_basis, decided_state, parsed_resets_at,
                        reset_zone, source_range_count, evidence_digest, recorded_at
                   FROM provider_quota_observation_provenance
                  WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_quota_observation_provenance(row, ranges.clone())),
            )
            .optional()
            .map_err(backend)?
            .transpose()
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

    fn record_provider_usage_observation(
        &self,
        request: &NewProviderUsageObservation,
    ) -> RepositoryResult<ProviderUsageObservation> {
        let observation = &request.observation;
        if request.idempotency_key.is_some() != request.intent_hash.is_some() {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "ProviderUsageObservation",
                "an explicit probe key and its canonical intent hash must be present together",
            )));
        }
        let paired = match observation.state {
            ProviderQuotaKind::Exhausted => observation.resets_at.is_some(),
            _ => observation.resets_at.is_none(),
        };
        if !paired {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "ProviderUsageObservation",
                "only an exhausted observation carries a reset instant, and it must carry one",
            )));
        }
        let mut kinds: Vec<QuotaWindowKind> = observation
            .windows
            .iter()
            .map(|window| window.kind)
            .collect();
        kinds.sort_unstable();
        if kinds.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "ProviderUsageObservation",
                "one window kind may be observed only once per account and provider",
            )));
        }
        if observation
            .windows
            .iter()
            .any(|window| window.used_percent > 100)
        {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "ProviderUsageObservation",
                "a consumption share must be a percentage",
            )));
        }
        if let Some(quota) = request.quota_state.as_ref() {
            let matching = quota.project_id == observation.project_id
                && quota.account_profile_id == observation.account_profile_id
                && quota.provider == observation.provider
                && quota.evidence_hash == observation.evidence_hash
                && quota.state == observation.state
                && quota.resets_at == observation.resets_at
                && quota.source == ProviderQuotaSource::ProviderReport
                && quota.observed_at == observation.observed_at
                && (observation.windows.is_empty() || quota.windows == observation.windows);
            if !matching {
                return Err(RepositoryError::Domain(DomainError::invalid(
                    "ProviderUsageObservation",
                    "the quota projection does not describe this exact provider report",
                )));
            }
        }

        let transaction = self.begin()?;
        if read_account_profile_in(
            &transaction,
            observation.project_id,
            observation.account_profile_id,
        )?
        .is_none()
        {
            return Err(RepositoryError::NotFound {
                subject: "account profile",
            });
        }
        if let Some(key) = request.idempotency_key.as_ref() {
            let replay: Option<(ProviderUsageObservation, String)> = transaction
                .query_row(
                    &format!(
                        "SELECT {PROVIDER_USAGE_OBSERVATION_COLUMNS}, intent_hash
                         FROM provider_usage_observations
                         WHERE idempotency_key = ?1"
                    ),
                    params![key.as_str()],
                    |row| {
                        let observation =
                            read_provider_usage_observation(row).map_err(|error| {
                                rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                            })?;
                        Ok((observation, row.get(9)?))
                    },
                )
                .optional()
                .map_err(backend)?;
            if let Some((stored, intent_hash)) = replay {
                let intent_hash = ContentHash::parse(&intent_hash)?;
                let same_subject = stored.project_id == observation.project_id
                    && stored.account_profile_id == observation.account_profile_id
                    && stored.provider == observation.provider;
                if !same_subject || Some(&intent_hash) != request.intent_hash.as_ref() {
                    return Err(RepositoryError::Conflict {
                        subject: "provider usage probe",
                        rule: "the idempotency key was already used for a different operation",
                    });
                }
                return Ok(stored);
            }
        }

        if let Some(quota) = request.quota_state.as_ref() {
            let _ = set_provider_quota_state_in(&transaction, quota, None)?;
        } else {
            let current: Option<RepositoryResult<ProviderQuotaState>> = transaction
                .query_row(
                    &format!(
                        "SELECT {PROVIDER_QUOTA_STATE_COLUMNS} FROM provider_quota_states
                         WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3"
                    ),
                    params![
                        observation.project_id.to_string(),
                        observation.account_profile_id.to_string(),
                        observation.provider.as_str(),
                    ],
                    |row| Ok(read_provider_quota_state(row)),
                )
                .optional()
                .map_err(backend)?;
            let mut current = current.transpose()?.ok_or(RepositoryError::Conflict {
                subject: "provider usage observation",
                rule: "an unchanged heartbeat must match an existing provider report",
            })?;
            attach_provider_quota_windows(&transaction, &mut current)?;
            if current.source != ProviderQuotaSource::ProviderReport
                || current.evidence_hash != observation.evidence_hash
                || current.state != observation.state
                || current.resets_at != observation.resets_at
                || (!observation.windows.is_empty() && current.windows != observation.windows)
            {
                return Err(RepositoryError::Conflict {
                    subject: "provider usage observation",
                    rule: "an unchanged heartbeat must match the current provider report",
                });
            }
        }

        let windows = serde_json::to_string(&observation.windows).map_err(|_| {
            RepositoryError::Domain(DomainError::invalid(
                "ProviderUsageObservation",
                "the derived window set could not be serialized",
            ))
        })?;
        transaction
            .execute(
                "INSERT INTO provider_usage_observations
                     (id, project_id, account_profile_id, provider, evidence_hash, state,
                      resets_at, windows, observed_at, idempotency_key, intent_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    observation.id.to_string(),
                    observation.project_id.to_string(),
                    observation.account_profile_id.to_string(),
                    observation.provider.as_str(),
                    observation.evidence_hash.as_str(),
                    observation.state.as_str(),
                    observation.resets_at.map(text),
                    windows,
                    text(observation.observed_at),
                    request.idempotency_key.as_ref().map(IdempotencyKey::as_str),
                    request.intent_hash.as_ref().map(ContentHash::as_str),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(observation.clone())
    }

    fn latest_provider_usage_observation(
        &self,
        project_id: ProjectId,
        account_profile_id: AccountProfileId,
        provider: &str,
    ) -> RepositoryResult<Option<ProviderUsageObservation>> {
        let observation: Option<RepositoryResult<ProviderUsageObservation>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {PROVIDER_USAGE_OBSERVATION_COLUMNS}
                     FROM provider_usage_observations
                     WHERE project_id = ?1 AND account_profile_id = ?2 AND provider = ?3
                     ORDER BY observed_at DESC, id DESC LIMIT 1"
                ),
                params![
                    project_id.to_string(),
                    account_profile_id.to_string(),
                    provider
                ],
                |row| Ok(read_provider_usage_observation(row)),
            )
            .optional()
            .map_err(backend)?;
        observation.transpose()
    }

    fn provider_usage_observation_by_key(
        &self,
        key: &IdempotencyKey,
    ) -> RepositoryResult<Option<(ProviderUsageObservation, ContentHash)>> {
        let observation: Option<RepositoryResult<(ProviderUsageObservation, ContentHash)>> = self
            .connection
            .query_row(
                &format!(
                    "SELECT {PROVIDER_USAGE_OBSERVATION_COLUMNS}, intent_hash
                     FROM provider_usage_observations
                     WHERE idempotency_key = ?1"
                ),
                params![key.as_str()],
                |row| {
                    Ok(
                        read_provider_usage_observation(row).and_then(|observation| {
                            Ok((
                                observation,
                                ContentHash::parse(&row.get::<_, String>(9).map_err(backend)?)?,
                            ))
                        }),
                    )
                },
            )
            .optional()
            .map_err(backend)?;
        observation.transpose()
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

fn topology_container_recovery_by_receipt(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
) -> RepositoryResult<StoredTopologyContainerRecovery> {
    transaction
        .query_row(
            "SELECT receipt_id, project_id, topology_node_id, container_binding_id,
                    prior_runtime_kind, prior_host, prior_generation, prior_native_id,
                    next_runtime_kind, next_host, next_generation, next_native_id,
                    parent_native_id, observed_kind, canonical_cwd, observed_title,
                    recovered_at
             FROM topology_container_recoveries
             WHERE project_id = ?1 AND receipt_id = ?2",
            params![project_id.to_string(), receipt_id.to_string()],
            |row| {
                let prior_generation: i64 = row.get(6)?;
                let next_generation: i64 = row.get(10)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    prior_generation,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    next_generation,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, String>(16)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?
        .ok_or(RepositoryError::NotFound {
            subject: "topology container recovery evidence",
        })
        .and_then(
            |(
                receipt_id,
                project_id,
                topology_node_id,
                container_binding_id,
                prior_runtime_kind,
                prior_host,
                prior_generation,
                prior_native_id,
                next_runtime_kind,
                next_host,
                next_generation,
                next_native_id,
                parent_native_id,
                observed_kind,
                canonical_cwd,
                observed_title,
                recovered_at,
            )| {
                Ok(StoredTopologyContainerRecovery {
                    receipt_id: CommandReceiptId::parse(&receipt_id)?,
                    project_id: ProjectId::parse(&project_id)?,
                    topology_node_id: TopologyNodeId::parse(&topology_node_id)?,
                    container_binding_id: ExternalId::parse(&container_binding_id)?,
                    prior_identity: NativeRuntimeIdentity {
                        runtime_kind: RuntimeKindKey::parse(&prior_runtime_kind)?,
                        host: ExternalName::parse(&prior_host)?,
                        generation: u64::try_from(prior_generation).map_err(|_| {
                            DomainError::invalid(
                                "prior native generation",
                                "is outside the stored range",
                            )
                        })?,
                        native_id: ExternalId::parse(&prior_native_id)?,
                    },
                    replacement_identity: NativeRuntimeIdentity {
                        runtime_kind: RuntimeKindKey::parse(&next_runtime_kind)?,
                        host: ExternalName::parse(&next_host)?,
                        generation: u64::try_from(next_generation).map_err(|_| {
                            DomainError::invalid(
                                "replacement native generation",
                                "is outside the stored range",
                            )
                        })?,
                        native_id: ExternalId::parse(&next_native_id)?,
                    },
                    parent_native_id: ExternalId::parse(&parent_native_id)?,
                    observed_kind: ObservedContainerKind::parse(&observed_kind)?,
                    canonical_cwd: canonical_cwd
                        .as_deref()
                        .map(ExternalName::parse)
                        .transpose()?,
                    observed_title: ExternalName::parse(&observed_title)?,
                    recovered_at: read_timestamp(&recovered_at)?,
                })
            },
        )
}

impl SqliteStore {
    /// Correct one legacy-imported epic code and record the authority atomically.
    pub fn correct_legacy_epic_backlog_code_with_intent(
        &self,
        correction: &LegacyEpicBacklogCodeCorrection,
        expected_revision: AggregateRevision,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
    ) -> RepositoryResult<(EpicBacklogCode, CommandReceipt, crate::graph::Applied)> {
        let intent = envelope.peek(self.realm_id())?;
        let target = AggregateRef::MiniProject {
            mini_project_id: correction.mini_project_id,
        };
        if intent.project_id != correction.project_id
            || intent.kind != CommandKind::CorrectEpicBacklogCode
            || intent.target != target
            || intent.target_revision != expected_revision
        {
            return Err(DomainError::invalid(
                "CommandReceipt",
                "the local command authority does not match the epic-code correction",
            )
            .into());
        }
        let transaction = self.begin()?;
        if let Some(existing) = crate::commands::intent::insert_local_command(&transaction, intent)?
        {
            let code: String = transaction
                .query_row(
                    "SELECT corrected_code FROM epic_backlog_code_corrections
                     WHERE project_id = ?1 AND receipt_id = ?2",
                    params![correction.project_id.to_string(), existing.id.to_string()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            return Ok((
                EpicBacklogCode::parse(code)?,
                existing,
                crate::graph::Applied::Unchanged,
            ));
        }

        let source: Option<(String, String)> = transaction
            .query_row(
                "SELECT code, provenance FROM epic_backlog_codes
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND status = 'active'",
                params![
                    correction.project_id.to_string(),
                    correction.mini_project_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((prior_code, provenance)) = source else {
            return Err(RepositoryError::NotFound {
                subject: "active epic backlog code",
            });
        };
        if provenance != "legacy" {
            return Err(conflict(
                "epic backlog code correction",
                "only a legacy-imported code may be corrected",
            ));
        }
        if prior_code != correction.expected_prior_code.as_str() {
            return Err(conflict(
                "epic backlog code correction",
                "the active legacy code moved since preview",
            ));
        }
        let pinned: i64 = transaction
            .query_row(
                "SELECT count(*) FROM mini_project_team_definition_snapshots
                 WHERE project_id = ?1 AND mini_project_id = ?2",
                params![
                    correction.project_id.to_string(),
                    correction.mini_project_id.to_string()
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if pinned != 0 {
            return Err(conflict(
                "epic backlog code correction",
                "an epic that already pins a Team Definition cannot change its item-code namespace",
            ));
        }
        let collision: i64 = transaction
            .query_row(
                "SELECT count(*) FROM (
                     SELECT code AS value FROM epic_backlog_codes
                      WHERE project_id = ?1 AND status = 'active'
                     UNION ALL
                     SELECT corrected_code AS value FROM epic_backlog_code_corrections
                      WHERE project_id = ?1
                 ) WHERE value = ?2 COLLATE NOCASE",
                params![
                    correction.project_id.to_string(),
                    correction.corrected_code.as_str()
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if collision != 0 {
            return Err(conflict(
                "epic backlog code correction",
                "the corrected code is already reserved in this project",
            ));
        }

        let receipt = command_receipt_by_key(&transaction, &intent.idempotency_key)?.ok_or(
            RepositoryError::NotFound {
                subject: "command receipt",
            },
        )?;
        transaction
            .execute(
                "INSERT INTO epic_backlog_code_corrections
                     (project_id, mini_project_id, prior_code, corrected_code,
                      reason, receipt_id, corrected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    correction.project_id.to_string(),
                    correction.mini_project_id.to_string(),
                    correction.expected_prior_code.as_str(),
                    correction.corrected_code.as_str(),
                    correction.reason.as_str(),
                    receipt.id.to_string(),
                    text(correction.corrected_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok((
            correction.corrected_code.clone(),
            receipt,
            crate::graph::Applied::Created,
        ))
    }

    /// Replace one stale native container identity and record the authority atomically.
    pub fn recover_topology_container_with_intent(
        &self,
        recovery: &TopologyContainerRecovery,
        expected_revision: AggregateRevision,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
    ) -> RepositoryResult<(
        StoredTopologyContainerRecovery,
        CommandReceipt,
        crate::graph::Applied,
    )> {
        let intent = envelope.peek(self.realm_id())?;
        let target = AggregateRef::Project {
            project_id: recovery.expected.project_id,
        };
        if intent.project_id != recovery.expected.project_id
            || intent.kind != CommandKind::RecoverTopologyContainer
            || intent.target != target
            || intent.target_revision != expected_revision
        {
            return Err(DomainError::invalid(
                "CommandReceipt",
                "the local command authority does not match the container recovery",
            )
            .into());
        }
        let transaction = self.begin()?;
        if let Some(existing) = crate::commands::intent::insert_local_command(&transaction, intent)?
        {
            let evidence = topology_container_recovery_by_receipt(
                &transaction,
                recovery.expected.project_id,
                existing.id,
            )?;
            return Ok((evidence, existing, crate::graph::Applied::Unchanged));
        }
        if recovery.replacement.project_id != recovery.expected.project_id
            || recovery.replacement.topology_node_id != recovery.expected.topology_node_id
            || recovery.replacement.container_binding_id != recovery.expected.container_binding_id
            || recovery.replacement.identity == recovery.expected.identity
        {
            return Err(DomainError::invalid(
                "topology container recovery",
                "the replacement must preserve project, node and logical binding while changing native identity",
            )
            .into());
        }

        let collision: i64 = transaction
            .query_row(
                "SELECT count(*) FROM topology_node_containers
                 WHERE project_id = ?1 AND topology_node_id <> ?2
                   AND runtime_kind = ?3 AND host = ?4 AND generation = ?5 AND native_id = ?6",
                params![
                    recovery.expected.project_id.to_string(),
                    recovery.expected.topology_node_id.to_string(),
                    recovery.replacement.identity.runtime_kind.as_str(),
                    recovery.replacement.identity.host.as_str(),
                    i64::try_from(recovery.replacement.identity.generation).map_err(|_| {
                        DomainError::invalid(
                            "replacement native generation",
                            "is outside the storable range",
                        )
                    })?,
                    recovery.replacement.identity.native_id.as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if collision != 0 {
            return Err(conflict(
                "topology container recovery",
                "the replacement native container is already bound to another topology node",
            ));
        }

        let receipt = command_receipt_by_key(&transaction, &intent.idempotency_key)?.ok_or(
            RepositoryError::NotFound {
                subject: "command receipt",
            },
        )?;
        let changed = transaction
            .execute(
                "UPDATE topology_node_containers
                 SET runtime_kind = ?1, host = ?2, generation = ?3, native_id = ?4,
                     observed_kind = ?5, canonical_cwd = ?6, bound_at = ?7,
                     last_readback_at = ?7, revision = revision + 1
                 WHERE project_id = ?8 AND topology_node_id = ?9
                   AND container_binding_id = ?10
                   AND runtime_kind = ?11 AND host = ?12 AND generation = ?13
                   AND native_id = ?14 AND revision = ?15",
                params![
                    recovery.replacement.identity.runtime_kind.as_str(),
                    recovery.replacement.identity.host.as_str(),
                    i64::try_from(recovery.replacement.identity.generation).map_err(|_| {
                        DomainError::invalid(
                            "replacement native generation",
                            "is outside the storable range",
                        )
                    })?,
                    recovery.replacement.identity.native_id.as_str(),
                    recovery.replacement.observed_kind.as_str(),
                    recovery
                        .replacement
                        .canonical_cwd
                        .as_ref()
                        .map(ExternalName::as_str),
                    text(recovery.replacement.observed_at),
                    recovery.expected.project_id.to_string(),
                    recovery.expected.topology_node_id.to_string(),
                    recovery.expected.container_binding_id.as_str(),
                    recovery.expected.identity.runtime_kind.as_str(),
                    recovery.expected.identity.host.as_str(),
                    i64::try_from(recovery.expected.identity.generation).map_err(|_| {
                        DomainError::invalid(
                            "prior native generation",
                            "is outside the storable range",
                        )
                    })?,
                    recovery.expected.identity.native_id.as_str(),
                    revision_column(recovery.expected.revision)?,
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "topology container recovery",
                "the stale binding moved since preview",
            ));
        }
        transaction
            .execute(
                "INSERT INTO topology_container_recoveries
                     (receipt_id, project_id, topology_node_id, container_binding_id,
                      prior_runtime_kind, prior_host, prior_generation, prior_native_id,
                      next_runtime_kind, next_host, next_generation, next_native_id,
                      parent_native_id, observed_kind, canonical_cwd, observed_title,
                      recovered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                         ?13, ?14, ?15, ?16, ?17)",
                params![
                    receipt.id.to_string(),
                    recovery.expected.project_id.to_string(),
                    recovery.expected.topology_node_id.to_string(),
                    recovery.expected.container_binding_id.as_str(),
                    recovery.expected.identity.runtime_kind.as_str(),
                    recovery.expected.identity.host.as_str(),
                    i64::try_from(recovery.expected.identity.generation).map_err(|_| {
                        DomainError::invalid(
                            "prior native generation",
                            "is outside the storable range",
                        )
                    })?,
                    recovery.expected.identity.native_id.as_str(),
                    recovery.replacement.identity.runtime_kind.as_str(),
                    recovery.replacement.identity.host.as_str(),
                    i64::try_from(recovery.replacement.identity.generation).map_err(|_| {
                        DomainError::invalid(
                            "replacement native generation",
                            "is outside the storable range",
                        )
                    })?,
                    recovery.replacement.identity.native_id.as_str(),
                    recovery.parent_native_id.as_str(),
                    recovery.replacement.observed_kind.as_str(),
                    recovery
                        .replacement
                        .canonical_cwd
                        .as_ref()
                        .map(ExternalName::as_str),
                    recovery.observed_title.as_str(),
                    text(recovery.replacement.observed_at),
                ],
            )
            .map_err(backend)?;
        let evidence = topology_container_recovery_by_receipt(
            &transaction,
            recovery.expected.project_id,
            receipt.id,
        )?;
        transaction.commit().map_err(backend)?;
        Ok((evidence, receipt, crate::graph::Applied::Created))
    }
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

    fn list_open_team_runs(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<TeamRun>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id FROM team_runs
                 WHERE project_id = ?1 AND task_id = ?2 AND closed_at IS NULL
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let ids: Vec<String> = statement
            .query_map(
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        drop(statement);
        let mut runs = Vec::new();
        for id in ids {
            if let Some(run) = self.get_team_run(project_id, TeamRunId::parse(&id)?)? {
                runs.push(run);
            }
        }
        Ok(runs)
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
// Quota-blocked seat succession
// ---------------------------------------------------------------------------

const SUCCESSION_ATTEMPT_COLUMNS: &str = "id, project_id, task_id, team_run_id, role_key, \
    predecessor_agent_run_id, predecessor_runtime_binding_id, predecessor_runtime_kind, \
    predecessor_host, predecessor_native_id, predecessor_generation, expected_task_revision, \
    expected_team_revision, expected_predecessor_revision, runtime_observation_cursor, \
    quota_provenance_id, quota_state_revision, quota_evidence_hash, quota_provider, \
    successor_model_rung, successor_model_rung_hash, successor_account_profile_id, \
    idempotency_key, intent_hash, state, deferred_until, handoff, handoff_hash, \
    successor_agent_run_id, successor_runtime_binding_id, successor_runtime_kind, \
    successor_host, successor_native_id, successor_generation, successor_observation_cursor, \
    successor_observed_at, refusal_reason, revision, created_at, updated_at, \
    predecessor_retired_at, confirmed_at, refused_at, successor_planned_at";

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSuccessionModelRung {
    schema_version: u32,
    model_rung: ModelRung,
}

struct StoredSuccessionQuotaEvidence {
    account_profile_id: String,
    provider: String,
    agent_run_id: String,
    runtime_observation_cursor: i64,
    runtime_binding_id: String,
    native_id: String,
    binding_generation: i64,
    evidence_hash: String,
    state: String,
    resets_at: Option<String>,
}

fn succession_model_rung_document(model_rung: &ModelRung) -> RepositoryResult<CanonicalDocument> {
    model_rung.validate()?;
    Ok(CanonicalDocument::from_serializable(
        &StoredSuccessionModelRung {
            schema_version: 1,
            model_rung: model_rung.clone(),
        },
    )?)
}

fn read_succession_attempt(row: &Row<'_>) -> RepositoryResult<SuccessionAttempt> {
    let predecessor_generation =
        u64::try_from(row.get::<_, i64>(10).map_err(backend)?).map_err(|_| {
            RepositoryError::Backend {
                detail: "a succession predecessor generation is stored negative".to_owned(),
            }
        })?;
    let route_json: Option<String> = row.get(19).map_err(backend)?;
    let route_hash: Option<String> = row.get(20).map_err(backend)?;
    let successor_account: Option<String> = row.get(21).map_err(backend)?;
    let (successor_model_rung, successor_account_profile_id) =
        match (route_json, route_hash, successor_account) {
            (Some(json), Some(hash), Some(account)) => {
                let route_document = stored_payload(&json, &hash)?;
                let stored_route: StoredSuccessionModelRung = route_document.deserialize()?;
                if stored_route.schema_version != 1 {
                    return Err(DomainError::invalid(
                        "stored succession model rung",
                        "schema_version must be 1",
                    )
                    .into());
                }
                stored_route.model_rung.validate()?;
                (
                    Some(stored_route.model_rung),
                    Some(AccountProfileId::parse(&account)?),
                )
            }
            (None, None, None) => (None, None),
            _ => {
                return Err(RepositoryError::Backend {
                    detail: "a succession successor route is only partly stored".to_owned(),
                });
            }
        };

    let handoff_json: Option<String> = row.get(26).map_err(backend)?;
    let handoff_hash: Option<String> = row.get(27).map_err(backend)?;
    let (handoff, handoff_hash) = match (handoff_json, handoff_hash) {
        (Some(json), Some(hash)) => {
            let document = stored_payload(&json, &hash)?;
            let handoff: SuccessionHandoff = document.deserialize()?;
            handoff.validate()?;
            (Some(handoff), Some(ContentHash::parse(&hash)?))
        }
        (None, None) => (None, None),
        _ => {
            return Err(RepositoryError::Backend {
                detail: "a succession handoff is only partly stored".to_owned(),
            });
        }
    };

    let successor_run: Option<String> = row.get(28).map_err(backend)?;
    let successor_binding: Option<String> = row.get(29).map_err(backend)?;
    let successor_runtime: Option<String> = row.get(30).map_err(backend)?;
    let successor_host: Option<String> = row.get(31).map_err(backend)?;
    let successor_native: Option<String> = row.get(32).map_err(backend)?;
    let successor_generation: Option<i64> = row.get(33).map_err(backend)?;
    let successor_cursor: Option<i64> = row.get(34).map_err(backend)?;
    let successor_observed_at: Option<String> = row.get(35).map_err(backend)?;
    let successor = match (
        successor_run,
        successor_binding,
        successor_runtime,
        successor_host,
        successor_native,
        successor_generation,
        successor_cursor,
        successor_observed_at,
    ) {
        (
            Some(run),
            Some(binding),
            Some(runtime_kind),
            Some(host),
            Some(native_id),
            Some(generation),
            Some(cursor),
            Some(observed_at),
        ) => Some(SuccessionSuccessorObservation {
            agent_run_id: AgentRunId::parse(&run)?,
            runtime_binding_id: RuntimeBindingId::parse(&binding)?,
            native_identity: NativeRuntimeIdentity {
                runtime_kind: RuntimeKindKey::parse(&runtime_kind)?,
                host: ExternalName::parse(&host)?,
                generation: u64::try_from(generation).map_err(|_| RepositoryError::Backend {
                    detail: "a succession successor generation is stored negative".to_owned(),
                })?,
                native_id: ExternalId::parse(&native_id)?,
            },
            runtime_observation_cursor: EventCursor::parse(cursor)?,
            observed_at: read_timestamp(&observed_at)?,
        }),
        (None, None, None, None, None, None, None, None) => None,
        _ => {
            return Err(RepositoryError::Backend {
                detail: "a succession successor observation is only partly stored".to_owned(),
            });
        }
    };

    let deferred_until: Option<String> = row.get(25).map_err(backend)?;
    let refusal_reason: Option<String> = row.get(36).map_err(backend)?;
    let predecessor_retired_at: Option<String> = row.get(40).map_err(backend)?;
    let confirmed_at: Option<String> = row.get(41).map_err(backend)?;
    let refused_at: Option<String> = row.get(42).map_err(backend)?;
    Ok(SuccessionAttempt {
        request: NewSuccessionAttempt {
            id: SuccessionAttemptId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
            project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
            task_id: TaskId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
            team_run_id: TeamRunId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
            role: RoleKey::parse(&row.get::<_, String>(4).map_err(backend)?)?,
            predecessor_agent_run_id: AgentRunId::parse(
                &row.get::<_, String>(5).map_err(backend)?,
            )?,
            predecessor_runtime_binding_id: RuntimeBindingId::parse(
                &row.get::<_, String>(6).map_err(backend)?,
            )?,
            predecessor_native_identity: NativeRuntimeIdentity {
                runtime_kind: RuntimeKindKey::parse(&row.get::<_, String>(7).map_err(backend)?)?,
                host: ExternalName::parse(&row.get::<_, String>(8).map_err(backend)?)?,
                native_id: ExternalId::parse(&row.get::<_, String>(9).map_err(backend)?)?,
                generation: predecessor_generation,
            },
            expected_task_revision: revision_of(row.get::<_, i64>(11).map_err(backend)?)?,
            expected_team_revision: revision_of(row.get::<_, i64>(12).map_err(backend)?)?,
            expected_predecessor_revision: revision_of(row.get::<_, i64>(13).map_err(backend)?)?,
            runtime_observation_cursor: EventCursor::parse(
                row.get::<_, i64>(14).map_err(backend)?,
            )?,
            quota_provenance_id: QuotaObservationProvenanceId::parse(
                &row.get::<_, String>(15).map_err(backend)?,
            )?,
            quota_state_revision: revision_of(row.get::<_, i64>(16).map_err(backend)?)?,
            quota_evidence_hash: ContentHash::parse(&row.get::<_, String>(17).map_err(backend)?)?,
            quota_provider: row.get(18).map_err(backend)?,
            successor_model_rung,
            successor_account_profile_id,
            idempotency_key: IdempotencyKey::parse(&row.get::<_, String>(22).map_err(backend)?)?,
            intent_hash: ContentHash::parse(&row.get::<_, String>(23).map_err(backend)?)?,
            deferred_until: deferred_until.as_deref().map(read_timestamp).transpose()?,
            created_at: read_timestamp(&row.get::<_, String>(38).map_err(backend)?)?,
        },
        state: SuccessionAttemptState::parse(&row.get::<_, String>(24).map_err(backend)?)?,
        handoff,
        handoff_hash,
        successor,
        refusal_reason: refusal_reason
            .as_deref()
            .map(SuccessionRefusalReason::parse)
            .transpose()?,
        revision: revision_of(row.get::<_, i64>(37).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(39).map_err(backend)?)?,
        successor_planned_at: row
            .get::<_, Option<String>>(43)
            .map_err(backend)?
            .as_deref()
            .map(read_timestamp)
            .transpose()?,
        predecessor_retired_at: predecessor_retired_at
            .as_deref()
            .map(read_timestamp)
            .transpose()?,
        confirmed_at: confirmed_at.as_deref().map(read_timestamp).transpose()?,
        refused_at: refused_at.as_deref().map(read_timestamp).transpose()?,
    })
}

fn read_succession_attempt_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: SuccessionAttemptId,
) -> RepositoryResult<Option<SuccessionAttempt>> {
    let row: Option<RepositoryResult<SuccessionAttempt>> = transaction
        .query_row(
            &format!(
                "SELECT {SUCCESSION_ATTEMPT_COLUMNS} FROM succession_attempts
                 WHERE project_id = ?1 AND id = ?2"
            ),
            params![project_id.to_string(), id.to_string()],
            |row| Ok(read_succession_attempt(row)),
        )
        .optional()
        .map_err(backend)?;
    row.transpose()
}

fn read_succession_by_key_in(
    transaction: &Transaction<'_>,
    key: &IdempotencyKey,
) -> RepositoryResult<Option<SuccessionAttempt>> {
    let row: Option<RepositoryResult<SuccessionAttempt>> = transaction
        .query_row(
            &format!(
                "SELECT {SUCCESSION_ATTEMPT_COLUMNS} FROM succession_attempts
                 WHERE idempotency_key = ?1"
            ),
            params![key.as_str()],
            |row| Ok(read_succession_attempt(row)),
        )
        .optional()
        .map_err(backend)?;
    row.transpose()
}

fn same_succession_intent(stored: &NewSuccessionAttempt, request: &NewSuccessionAttempt) -> bool {
    stored.project_id == request.project_id
        && stored.task_id == request.task_id
        && stored.team_run_id == request.team_run_id
        && stored.role == request.role
        && stored.predecessor_agent_run_id == request.predecessor_agent_run_id
        && stored.predecessor_runtime_binding_id == request.predecessor_runtime_binding_id
        && stored.predecessor_native_identity == request.predecessor_native_identity
        && stored.expected_task_revision == request.expected_task_revision
        && stored.expected_team_revision == request.expected_team_revision
        && stored.expected_predecessor_revision == request.expected_predecessor_revision
        && stored.runtime_observation_cursor == request.runtime_observation_cursor
        && stored.quota_provenance_id == request.quota_provenance_id
        && stored.quota_state_revision == request.quota_state_revision
        && stored.quota_evidence_hash == request.quota_evidence_hash
        && stored.quota_provider == request.quota_provider
        && (request.deferred_until.is_some()
            || (stored.successor_model_rung == request.successor_model_rung
                && stored.successor_account_profile_id == request.successor_account_profile_id))
        && stored.intent_hash == request.intent_hash
        && stored.deferred_until == request.deferred_until
}

fn succession_attempts_query(
    connection: &Connection,
    predicate: &str,
    parameters: impl rusqlite::Params,
) -> RepositoryResult<Vec<SuccessionAttempt>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {SUCCESSION_ATTEMPT_COLUMNS} FROM succession_attempts
             WHERE {predicate}"
        ))
        .map_err(backend)?;
    let mut rows = statement.query(parameters).map_err(backend)?;
    let mut attempts = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        attempts.push(read_succession_attempt(row)?);
    }
    Ok(attempts)
}

fn validate_successor_observation(
    transaction: &Transaction<'_>,
    attempt: &SuccessionAttempt,
    observation: &SuccessionSuccessorObservation,
) -> RepositoryResult<()> {
    let successor_account =
        attempt
            .request
            .successor_account_profile_id
            .ok_or(DomainError::MissingEvidence {
                subject: "succession successor",
                rule: "the attempt has no frozen successor account",
            })?;
    if observation.agent_run_id == attempt.request.predecessor_agent_run_id {
        return Err(conflict(
            "succession successor",
            "the predecessor cannot be installed as its own successor",
        ));
    }
    let run = read_agent_run(
        transaction,
        attempt.request.project_id,
        observation.agent_run_id,
    )?
    .ok_or(RepositoryError::NotFound {
        subject: "succession successor run",
    })?;
    let binding = run.binding.as_ref().ok_or(DomainError::MissingEvidence {
        subject: "succession successor",
        rule: "the successor has no runtime binding",
    })?;
    if run.team_run_id != attempt.request.team_run_id
        || run.role != attempt.request.role
        || run.parent_agent_run_id != Some(attempt.request.predecessor_agent_run_id)
        || run.account_profile_id != Some(successor_account)
        || binding.id != observation.runtime_binding_id
        || binding.identity != observation.native_identity
        || run.projection.last_cursor != Some(observation.runtime_observation_cursor)
        || !matches!(
            run.projection.observed,
            ObservedRunState::Running | ObservedRunState::WaitingInput
        )
    {
        return Err(DomainError::MissingEvidence {
            subject: "succession successor",
            rule: "the readback does not match the frozen slot, account, lineage and binding",
        }
        .into());
    }
    let confirmed: i64 = transaction
        .query_row(
            "SELECT count(*) FROM runtime_events
             WHERE project_id = ?1 AND cursor = ?2 AND event_kind = 'runtime_observation'
               AND agent_run_id = ?3 AND runtime_kind = ?4 AND host = ?5
               AND generation = ?6 AND native_id = ?7
               AND observed_state IN ('running', 'waiting_input')",
            params![
                attempt.request.project_id.to_string(),
                observation.runtime_observation_cursor.get(),
                observation.agent_run_id.to_string(),
                observation.native_identity.runtime_kind.as_str(),
                observation.native_identity.host.as_str(),
                generation_column(observation.native_identity.generation)?,
                observation.native_identity.native_id.as_str(),
            ],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if confirmed != 1 {
        return Err(DomainError::MissingEvidence {
            subject: "succession successor",
            rule: "the cited cursor is not the exact confirmed successor observation",
        }
        .into());
    }
    Ok(())
}

impl SuccessionRepository for SqliteStore {
    fn create_succession_attempt(
        &self,
        request: &NewSuccessionAttempt,
    ) -> RepositoryResult<SuccessionAttempt> {
        let state = request.initial_state()?;
        let route = request
            .successor_model_rung
            .as_ref()
            .map(succession_model_rung_document)
            .transpose()?;
        let transaction = self.begin()?;
        if let Some(existing) = read_succession_by_key_in(&transaction, &request.idempotency_key)? {
            if same_succession_intent(&existing.request, request) {
                return Ok(existing);
            }
            return Err(conflict(
                "succession idempotency key",
                "the key already names a different frozen succession intent",
            ));
        }

        let task_revision: Option<i64> = transaction
            .query_row(
                "SELECT revision FROM tasks WHERE project_id = ?1 AND id = ?2",
                params![request.project_id.to_string(), request.task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let task_revision = task_revision.ok_or(RepositoryError::NotFound {
            subject: "succession task",
        })?;
        revision_of(task_revision)?.expect("succession task", request.expected_task_revision)?;

        let team: Option<(String, i64, Option<String>)> = transaction
            .query_row(
                "SELECT task_id, revision, closed_at FROM team_runs
                 WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.team_run_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let (team_task, team_revision, team_closed) = team.ok_or(RepositoryError::NotFound {
            subject: "succession team run",
        })?;
        if TaskId::parse(&team_task)? != request.task_id || team_closed.is_some() {
            return Err(conflict(
                "succession team run",
                "the team is not the open team serving the frozen task",
            ));
        }
        revision_of(team_revision)?
            .expect("succession team run", request.expected_team_revision)?;

        let predecessor = read_agent_run(
            &transaction,
            request.project_id,
            request.predecessor_agent_run_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "succession predecessor run",
        })?;
        predecessor.revision.expect(
            "succession predecessor",
            request.expected_predecessor_revision,
        )?;
        let predecessor_binding =
            predecessor
                .binding
                .as_ref()
                .ok_or(DomainError::MissingEvidence {
                    subject: "succession predecessor",
                    rule: "the quota-blocked run has no runtime binding",
                })?;
        if predecessor.team_run_id != request.team_run_id
            || predecessor.role != request.role
            || predecessor_binding.id != request.predecessor_runtime_binding_id
            || predecessor_binding.identity != request.predecessor_native_identity
            || predecessor.projection.observed != ObservedRunState::Blocked
            || predecessor.projection.last_cursor != Some(request.runtime_observation_cursor)
            || predecessor.terminal.is_some()
        {
            return Err(DomainError::MissingEvidence {
                subject: "succession predecessor",
                rule: "the current slot, binding and latest blocked cursor do not match the frozen decision",
            }
            .into());
        }
        let predecessor_account =
            predecessor
                .account_profile_id
                .ok_or(DomainError::MissingEvidence {
                    subject: "succession predecessor",
                    rule: "a quota-blocked predecessor must be pinned to an account",
                })?;

        let evidence: Option<StoredSuccessionQuotaEvidence> = transaction
            .query_row(
                "SELECT p.account_profile_id, p.provider, p.agent_run_id,
                            p.runtime_observation_cursor, p.runtime_binding_id, p.native_id,
                            p.binding_generation, q.evidence_hash, q.state, q.resets_at
                     FROM provider_quota_observation_provenance p
                     JOIN provider_quota_states q
                       ON q.project_id = p.project_id
                      AND q.account_profile_id = p.account_profile_id
                      AND q.provider = p.provider
                      AND q.provenance_id = p.id
                     WHERE p.project_id = ?1 AND p.id = ?2 AND q.revision = ?3",
                params![
                    request.project_id.to_string(),
                    request.quota_provenance_id.to_string(),
                    revision_column(request.quota_state_revision)?,
                ],
                |row| {
                    Ok(StoredSuccessionQuotaEvidence {
                        account_profile_id: row.get(0)?,
                        provider: row.get(1)?,
                        agent_run_id: row.get(2)?,
                        runtime_observation_cursor: row.get(3)?,
                        runtime_binding_id: row.get(4)?,
                        native_id: row.get(5)?,
                        binding_generation: row.get(6)?,
                        evidence_hash: row.get(7)?,
                        state: row.get(8)?,
                        resets_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(backend)?;
        let Some(evidence) = evidence else {
            return Err(DomainError::MissingEvidence {
                subject: "succession quota",
                rule: "the current quota row does not cite the frozen provenance and revision",
            }
            .into());
        };
        let resets_at = evidence
            .resets_at
            .as_deref()
            .map(read_timestamp)
            .transpose()?;
        let quota_blocks = match ProviderQuotaKind::parse(&evidence.state)? {
            ProviderQuotaKind::Exhausted => {
                resets_at.is_some_and(|reset| request.created_at < reset)
            }
            ProviderQuotaKind::Drained | ProviderQuotaKind::Unknown => true,
            ProviderQuotaKind::Available | ProviderQuotaKind::CannotReport => false,
        };
        if AccountProfileId::parse(&evidence.account_profile_id)? != predecessor_account
            || evidence.provider != request.quota_provider
            || AgentRunId::parse(&evidence.agent_run_id)? != request.predecessor_agent_run_id
            || EventCursor::parse(evidence.runtime_observation_cursor)?
                != request.runtime_observation_cursor
            || RuntimeBindingId::parse(&evidence.runtime_binding_id)?
                != request.predecessor_runtime_binding_id
            || ExternalId::parse(&evidence.native_id)?
                != request.predecessor_native_identity.native_id
            || u64::try_from(evidence.binding_generation).ok()
                != Some(request.predecessor_native_identity.generation)
            || ContentHash::parse(&evidence.evidence_hash)? != request.quota_evidence_hash
            || !quota_blocks
        {
            return Err(DomainError::MissingEvidence {
                subject: "succession quota",
                rule: "the current blocking quota evidence does not exactly match the predecessor observation",
            }
            .into());
        }
        if let Some(account_profile_id) = request.successor_account_profile_id
            && read_account_profile_in(&transaction, request.project_id, account_profile_id)?
                .is_none()
        {
            return Err(RepositoryError::NotFound {
                subject: "succession successor account",
            });
        }

        transaction
            .execute(
                "INSERT INTO succession_attempts
                    (id, project_id, task_id, team_run_id, role_key,
                     predecessor_agent_run_id, predecessor_runtime_binding_id,
                     predecessor_runtime_kind, predecessor_host, predecessor_native_id,
                     predecessor_generation, expected_task_revision, expected_team_revision,
                     expected_predecessor_revision, runtime_observation_cursor,
                     quota_provenance_id, quota_state_revision, quota_evidence_hash,
                     quota_provider, successor_model_rung, successor_model_rung_hash,
                     successor_account_profile_id, idempotency_key, intent_hash, state,
                     deferred_until, revision, created_at, updated_at, successor_planned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                         ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                         ?25, ?26, 1, ?27, ?27, ?28)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.team_run_id.to_string(),
                    request.role.as_str(),
                    request.predecessor_agent_run_id.to_string(),
                    request.predecessor_runtime_binding_id.to_string(),
                    request.predecessor_native_identity.runtime_kind.as_str(),
                    request.predecessor_native_identity.host.as_str(),
                    request.predecessor_native_identity.native_id.as_str(),
                    generation_column(request.predecessor_native_identity.generation)?,
                    revision_column(request.expected_task_revision)?,
                    revision_column(request.expected_team_revision)?,
                    revision_column(request.expected_predecessor_revision)?,
                    request.runtime_observation_cursor.get(),
                    request.quota_provenance_id.to_string(),
                    revision_column(request.quota_state_revision)?,
                    request.quota_evidence_hash.as_str(),
                    request.quota_provider.as_str(),
                    route.as_ref().map(CanonicalDocument::json),
                    route.as_ref().map(|document| document.hash().as_str()),
                    request
                        .successor_account_profile_id
                        .map(|account| account.to_string()),
                    request.idempotency_key.as_str(),
                    request.intent_hash.as_str(),
                    state.as_str(),
                    request.deferred_until.map(text),
                    text(request.created_at),
                    (state == SuccessionAttemptState::Planned).then(|| text(request.created_at)),
                ],
            )
            .map_err(backend)?;
        let attempt = read_succession_attempt_in(&transaction, request.project_id, request.id)?
            .ok_or(RepositoryError::NotFound {
                subject: "created succession attempt",
            })?;
        transaction.commit().map_err(backend)?;
        Ok(attempt)
    }

    fn get_succession_attempt(
        &self,
        project_id: ProjectId,
        id: SuccessionAttemptId,
    ) -> RepositoryResult<Option<SuccessionAttempt>> {
        let transaction = self.begin()?;
        read_succession_attempt_in(&transaction, project_id, id)
    }

    fn succession_attempt_by_key(
        &self,
        key: &IdempotencyKey,
    ) -> RepositoryResult<Option<SuccessionAttempt>> {
        let transaction = self.begin()?;
        read_succession_by_key_in(&transaction, key)
    }

    fn active_succession_attempt(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        role: &RoleKey,
    ) -> RepositoryResult<Option<SuccessionAttempt>> {
        let mut attempts = succession_attempts_query(
            &self.connection,
            "project_id = ?1 AND team_run_id = ?2 AND role_key = ?3
             AND state IN ('planned', 'deferred', 'predecessor_retired', 'successor_observed')
             ORDER BY created_at, id",
            params![
                project_id.to_string(),
                team_run_id.to_string(),
                role.as_str()
            ],
        )?;
        Ok(attempts.pop())
    }

    fn list_nonterminal_succession_attempts(
        &self,
        limit: u32,
    ) -> RepositoryResult<Vec<SuccessionAttempt>> {
        if limit == 0 {
            return Err(
                DomainError::invalid("succession inventory limit", "must be positive").into(),
            );
        }
        succession_attempts_query(
            &self.connection,
            "state IN ('planned', 'deferred', 'predecessor_retired', 'successor_observed')
             ORDER BY created_at, id LIMIT ?1",
            params![i64::from(limit)],
        )
    }

    fn list_due_succession_attempts(
        &self,
        now: Timestamp,
        limit: u32,
    ) -> RepositoryResult<Vec<SuccessionAttempt>> {
        if limit == 0 {
            return Err(
                DomainError::invalid("succession inventory limit", "must be positive").into(),
            );
        }
        succession_attempts_query(
            &self.connection,
            "state IN ('planned', 'predecessor_retired', 'successor_observed')
             OR (state = 'deferred' AND deferred_until <= ?1)
             ORDER BY created_at, id LIMIT ?2",
            params![text(now), i64::from(limit)],
        )
    }

    fn refresh_deferred_succession_evidence(
        &self,
        request: &SuccessionDeferredRefresh,
    ) -> RepositoryResult<SuccessionAttempt> {
        let resulting_state = request.resulting_state()?;
        let route = request
            .successor_model_rung
            .as_ref()
            .map(succession_model_rung_document)
            .transpose()?;
        let transaction = self.begin()?;
        let attempt =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        attempt
            .revision
            .expect("succession attempt", request.expected_revision)?;
        if attempt.state != SuccessionAttemptState::Deferred {
            return Err(conflict(
                "succession deferred refresh",
                "only a deferred attempt can refresh its authority",
            ));
        }
        if !attempt.is_due(request.refreshed_at) {
            return Err(conflict(
                "succession deferred refresh",
                "the deferred attempt is not due",
            ));
        }

        let predecessor = read_agent_run(
            &transaction,
            request.project_id,
            attempt.request.predecessor_agent_run_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "succession predecessor run",
        })?;
        predecessor.revision.expect(
            "succession predecessor",
            request.expected_predecessor_revision,
        )?;
        let predecessor_binding =
            predecessor
                .binding
                .as_ref()
                .ok_or(DomainError::MissingEvidence {
                    subject: "succession predecessor",
                    rule: "the due predecessor has no runtime binding",
                })?;
        if predecessor.team_run_id != attempt.request.team_run_id
            || predecessor.role != attempt.request.role
            || predecessor_binding.id != attempt.request.predecessor_runtime_binding_id
            || predecessor_binding.identity != attempt.request.predecessor_native_identity
            || predecessor.projection.observed != ObservedRunState::Blocked
            || predecessor.projection.last_cursor != Some(request.runtime_observation_cursor)
            || predecessor.terminal.is_some()
        {
            return Err(DomainError::MissingEvidence {
                subject: "succession predecessor",
                rule: "the refreshed cursor is not the latest blocked observation on the original slot and binding",
            }
            .into());
        }
        let predecessor_account =
            predecessor
                .account_profile_id
                .ok_or(DomainError::MissingEvidence {
                    subject: "succession predecessor",
                    rule: "a quota-blocked predecessor must remain pinned to an account",
                })?;

        let exact_observation: i64 = transaction
            .query_row(
                "SELECT count(*) FROM runtime_events
                 WHERE project_id = ?1 AND cursor = ?2
                   AND event_kind = 'runtime_observation' AND agent_run_id = ?3
                   AND runtime_kind = ?4 AND host = ?5 AND generation = ?6
                   AND native_id = ?7 AND observed_state = 'blocked'",
                params![
                    request.project_id.to_string(),
                    request.runtime_observation_cursor.get(),
                    attempt.request.predecessor_agent_run_id.to_string(),
                    attempt
                        .request
                        .predecessor_native_identity
                        .runtime_kind
                        .as_str(),
                    attempt.request.predecessor_native_identity.host.as_str(),
                    generation_column(attempt.request.predecessor_native_identity.generation)?,
                    attempt
                        .request
                        .predecessor_native_identity
                        .native_id
                        .as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if exact_observation != 1 {
            return Err(DomainError::MissingEvidence {
                subject: "succession predecessor",
                rule: "the refreshed cursor is not an exact blocked observation on the original binding",
            }
            .into());
        }

        let evidence: Option<StoredSuccessionQuotaEvidence> = transaction
            .query_row(
                "SELECT p.account_profile_id, p.provider, p.agent_run_id,
                        p.runtime_observation_cursor, p.runtime_binding_id, p.native_id,
                        p.binding_generation, q.evidence_hash, q.state, q.resets_at
                 FROM provider_quota_observation_provenance p
                 JOIN provider_quota_states q
                   ON q.project_id = p.project_id
                  AND q.account_profile_id = p.account_profile_id
                  AND q.provider = p.provider
                  AND q.provenance_id = p.id
                 WHERE p.project_id = ?1 AND p.id = ?2 AND q.revision = ?3
                   AND p.decision_basis = 'runtime_refusal'
                   AND p.decided_state = q.state
                   AND p.parsed_resets_at IS q.resets_at
                   AND p.evidence_digest = q.evidence_hash",
                params![
                    request.project_id.to_string(),
                    request.quota_provenance_id.to_string(),
                    revision_column(request.quota_state_revision)?,
                ],
                |row| {
                    Ok(StoredSuccessionQuotaEvidence {
                        account_profile_id: row.get(0)?,
                        provider: row.get(1)?,
                        agent_run_id: row.get(2)?,
                        runtime_observation_cursor: row.get(3)?,
                        runtime_binding_id: row.get(4)?,
                        native_id: row.get(5)?,
                        binding_generation: row.get(6)?,
                        evidence_hash: row.get(7)?,
                        state: row.get(8)?,
                        resets_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(backend)?;
        let Some(evidence) = evidence else {
            return Err(DomainError::MissingEvidence {
                subject: "succession quota",
                rule: "the current quota row does not cite the refreshed provenance and revision",
            }
            .into());
        };
        if AccountProfileId::parse(&evidence.account_profile_id)? != predecessor_account
            || evidence.provider != request.quota_provider
            || AgentRunId::parse(&evidence.agent_run_id)?
                != attempt.request.predecessor_agent_run_id
            || EventCursor::parse(evidence.runtime_observation_cursor)?
                != request.runtime_observation_cursor
            || RuntimeBindingId::parse(&evidence.runtime_binding_id)?
                != attempt.request.predecessor_runtime_binding_id
            || ExternalId::parse(&evidence.native_id)?
                != attempt.request.predecessor_native_identity.native_id
            || u64::try_from(evidence.binding_generation).ok()
                != Some(attempt.request.predecessor_native_identity.generation)
            || ContentHash::parse(&evidence.evidence_hash)? != request.quota_evidence_hash
        {
            return Err(DomainError::MissingEvidence {
                subject: "succession quota",
                rule: "the refreshed quota evidence does not exactly match the predecessor observation",
            }
            .into());
        }
        // Deliberately do not require `evidence.state` to block at `refreshed_at`.
        // At an exhausted reset boundary, the current row no longer blocks a
        // new launch while the fresh reachable Blocked predecessor remains the
        // authority for replanning this already-durable attempt.
        let _ = ProviderQuotaKind::parse(&evidence.state)?;
        let _ = evidence
            .resets_at
            .as_deref()
            .map(read_timestamp)
            .transpose()?;

        if let Some(account_profile_id) = request.successor_account_profile_id
            && read_account_profile_in(&transaction, request.project_id, account_profile_id)?
                .is_none()
        {
            return Err(RepositoryError::NotFound {
                subject: "succession successor account",
            });
        }

        let next = attempt.revision.next()?;
        let changed = match resulting_state {
            SuccessionAttemptState::Deferred => transaction.execute(
                "UPDATE succession_attempts
                 SET expected_predecessor_revision = ?1, runtime_observation_cursor = ?2,
                     quota_provenance_id = ?3, quota_state_revision = ?4,
                     quota_evidence_hash = ?5, quota_provider = ?6,
                     deferred_until = ?7, revision = ?8, updated_at = ?9
                 WHERE project_id = ?10 AND id = ?11 AND revision = ?12
                   AND state = 'deferred'",
                params![
                    revision_column(request.expected_predecessor_revision)?,
                    request.runtime_observation_cursor.get(),
                    request.quota_provenance_id.to_string(),
                    revision_column(request.quota_state_revision)?,
                    request.quota_evidence_hash.as_str(),
                    request.quota_provider.as_str(),
                    request.deferred_until.map(text),
                    revision_column(next)?,
                    text(request.refreshed_at),
                    request.project_id.to_string(),
                    request.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            ),
            SuccessionAttemptState::Planned => transaction.execute(
                "UPDATE succession_attempts
                 SET state = 'planned', expected_predecessor_revision = ?1,
                     runtime_observation_cursor = ?2, quota_provenance_id = ?3,
                     quota_state_revision = ?4, quota_evidence_hash = ?5,
                     quota_provider = ?6, successor_model_rung = ?7,
                     successor_model_rung_hash = ?8, successor_account_profile_id = ?9,
                     deferred_until = NULL, successor_planned_at = ?10,
                     revision = ?11, updated_at = ?10
                 WHERE project_id = ?12 AND id = ?13 AND revision = ?14
                   AND state = 'deferred'",
                params![
                    revision_column(request.expected_predecessor_revision)?,
                    request.runtime_observation_cursor.get(),
                    request.quota_provenance_id.to_string(),
                    revision_column(request.quota_state_revision)?,
                    request.quota_evidence_hash.as_str(),
                    request.quota_provider.as_str(),
                    route.as_ref().map(CanonicalDocument::json),
                    route.as_ref().map(|document| document.hash().as_str()),
                    request
                        .successor_account_profile_id
                        .map(|account| account.to_string()),
                    text(request.refreshed_at),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            ),
            _ => unreachable!("deferred refresh validates only Deferred or Planned"),
        }
        .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "succession deferred refresh",
                "the attempt revision or state moved during the write",
            ));
        }
        let stored =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn record_succession_handoff(
        &self,
        request: &SuccessionHandoffRecord,
    ) -> RepositoryResult<SuccessionAttempt> {
        let document = request.handoff.canonicalize()?;
        let transaction = self.begin()?;
        let attempt =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        if let Some(existing_hash) = attempt.handoff_hash.as_ref() {
            if existing_hash == document.hash() {
                return Ok(attempt);
            }
            return Err(conflict(
                "succession handoff",
                "this attempt already carries different handoff evidence",
            ));
        }
        attempt
            .revision
            .expect("succession attempt", request.expected_revision)?;
        if attempt.state != SuccessionAttemptState::Planned
            || request.handoff.attempt_id != request.attempt_id
            || request.handoff.predecessor_agent_run_id != attempt.request.predecessor_agent_run_id
            || request.handoff.predecessor_runtime_binding_id
                != attempt.request.predecessor_runtime_binding_id
            || request.handoff.predecessor_native_identity
                != attempt.request.predecessor_native_identity
        {
            return Err(conflict(
                "succession handoff",
                "the evidence does not name the exact active predecessor decision",
            ));
        }
        let next = attempt.revision.next()?;
        transaction
            .execute(
                "UPDATE succession_attempts
                 SET handoff = ?1, handoff_hash = ?2, revision = ?3, updated_at = ?4
                 WHERE project_id = ?5 AND id = ?6 AND revision = ?7",
                params![
                    document.json(),
                    document.hash().as_str(),
                    revision_column(next)?,
                    text(request.recorded_at),
                    request.project_id.to_string(),
                    request.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            )
            .map_err(backend)?;
        let stored =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn mark_succession_predecessor_retired(
        &self,
        request: &SuccessionAttemptAdvance,
    ) -> RepositoryResult<SuccessionAttempt> {
        let transaction = self.begin()?;
        let attempt =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        if matches!(
            attempt.state,
            SuccessionAttemptState::PredecessorRetired
                | SuccessionAttemptState::SuccessorObserved
                | SuccessionAttemptState::Confirmed
        ) {
            return Ok(attempt);
        }
        attempt
            .revision
            .expect("succession attempt", request.expected_revision)?;
        attempt
            .state
            .ensure_advance_to(SuccessionAttemptState::PredecessorRetired)?;
        let predecessor = read_agent_run(
            &transaction,
            attempt.request.project_id,
            attempt.request.predecessor_agent_run_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "succession predecessor run",
        })?;
        let terminal = predecessor
            .terminal
            .as_ref()
            .ok_or(DomainError::MissingEvidence {
                subject: "succession predecessor retirement",
                rule: "the predecessor must carry runtime-observed cancellation before the attempt advances",
            })?;
        if terminal.outcome != TerminalOutcome::Cancelled
            || !matches!(
                terminal.source,
                TerminalEvidenceSource::RuntimeObservation { .. }
            )
        {
            return Err(DomainError::MissingEvidence {
                subject: "succession predecessor retirement",
                rule: "only runtime-observed cancellation may advance the attempt",
            }
            .into());
        }
        if attempt.handoff.is_none() {
            return Err(DomainError::MissingEvidence {
                subject: "succession predecessor retirement",
                rule: "summary-or-degraded handoff evidence must be durable before release",
            }
            .into());
        }
        let next = attempt.revision.next()?;
        transaction
            .execute(
                "UPDATE succession_attempts
                 SET state = 'predecessor_retired', predecessor_retired_at = ?1,
                     revision = ?2, updated_at = ?1
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    text(request.occurred_at),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            )
            .map_err(backend)?;
        let stored =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn mark_succession_successor_observed(
        &self,
        request: &SuccessionSuccessorRecord,
    ) -> RepositoryResult<SuccessionAttempt> {
        let transaction = self.begin()?;
        let attempt =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        if let Some(existing) = attempt.successor.as_ref() {
            if existing == &request.observation {
                return Ok(attempt);
            }
            return Err(conflict(
                "succession successor observation",
                "this attempt already observed a different successor",
            ));
        }
        attempt
            .revision
            .expect("succession attempt", request.expected_revision)?;
        attempt
            .state
            .ensure_advance_to(SuccessionAttemptState::SuccessorObserved)?;
        validate_successor_observation(&transaction, &attempt, &request.observation)?;
        let identity = &request.observation.native_identity;
        let next = attempt.revision.next()?;
        transaction
            .execute(
                "UPDATE succession_attempts
                 SET state = 'successor_observed', successor_agent_run_id = ?1,
                     successor_runtime_binding_id = ?2, successor_runtime_kind = ?3,
                     successor_host = ?4, successor_native_id = ?5, successor_generation = ?6,
                     successor_observation_cursor = ?7, successor_observed_at = ?8,
                     revision = ?9, updated_at = ?8
                 WHERE project_id = ?10 AND id = ?11 AND revision = ?12",
                params![
                    request.observation.agent_run_id.to_string(),
                    request.observation.runtime_binding_id.to_string(),
                    identity.runtime_kind.as_str(),
                    identity.host.as_str(),
                    identity.native_id.as_str(),
                    generation_column(identity.generation)?,
                    request.observation.runtime_observation_cursor.get(),
                    text(request.observation.observed_at),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            )
            .map_err(backend)?;
        let stored =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn confirm_succession(
        &self,
        request: &SuccessionConfirmation,
    ) -> RepositoryResult<SuccessionReceipt> {
        let receipt_document = request.receipt.canonicalize()?;
        let transaction = self.begin()?;
        if let Some(existing) = read_succession_receipt_in(
            &transaction,
            request.receipt.project_id,
            request.receipt.attempt_id,
        )? {
            if existing == request.receipt {
                return Ok(existing);
            }
            return Err(conflict(
                "succession receipt",
                "this attempt already confirmed under a different receipt",
            ));
        }
        let attempt = read_succession_attempt_in(
            &transaction,
            request.receipt.project_id,
            request.receipt.attempt_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "succession attempt",
        })?;
        attempt
            .revision
            .expect("succession attempt", request.expected_revision)?;
        attempt
            .state
            .ensure_advance_to(SuccessionAttemptState::Confirmed)?;
        request.receipt.validate_against(&attempt)?;
        if request.receipt.confirmed_at < attempt.updated_at {
            return Err(conflict(
                "succession confirmation",
                "confirmation predates the successor readback",
            ));
        }
        let successor = attempt
            .successor
            .as_ref()
            .ok_or(DomainError::MissingEvidence {
                subject: "succession confirmation",
                rule: "the successor was not observed",
            })?;
        validate_successor_observation(&transaction, &attempt, successor)?;
        let next = attempt.revision.next()?;
        transaction
            .execute(
                "UPDATE succession_attempts
                 SET state = 'confirmed', confirmed_at = ?1, revision = ?2, updated_at = ?1
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    text(request.receipt.confirmed_at),
                    revision_column(next)?,
                    request.receipt.project_id.to_string(),
                    request.receipt.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO succession_receipts
                    (id, project_id, attempt_id, receipt, receipt_hash, confirmed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    request.receipt.id.to_string(),
                    request.receipt.project_id.to_string(),
                    request.receipt.attempt_id.to_string(),
                    receipt_document.json(),
                    receipt_document.hash().as_str(),
                    text(request.receipt.confirmed_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(request.receipt.clone())
    }

    fn refuse_succession(
        &self,
        request: &SuccessionRefusal,
    ) -> RepositoryResult<SuccessionAttempt> {
        let transaction = self.begin()?;
        let attempt =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        if attempt.state == SuccessionAttemptState::Refused {
            if attempt.refusal_reason == Some(request.reason) {
                return Ok(attempt);
            }
            return Err(conflict(
                "succession refusal",
                "this attempt already carries a different refusal",
            ));
        }
        attempt
            .revision
            .expect("succession attempt", request.expected_revision)?;
        attempt
            .state
            .ensure_advance_to(SuccessionAttemptState::Refused)?;
        let next = attempt.revision.next()?;
        transaction
            .execute(
                "UPDATE succession_attempts
                 SET state = 'refused', refusal_reason = ?1, refused_at = ?2,
                     revision = ?3, updated_at = ?2
                 WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
                params![
                    request.reason.as_str(),
                    text(request.refused_at),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.attempt_id.to_string(),
                    revision_column(attempt.revision)?,
                ],
            )
            .map_err(backend)?;
        let stored =
            read_succession_attempt_in(&transaction, request.project_id, request.attempt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "succession attempt",
                })?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn succession_receipt_for_attempt(
        &self,
        project_id: ProjectId,
        attempt_id: SuccessionAttemptId,
    ) -> RepositoryResult<Option<SuccessionReceipt>> {
        let transaction = self.begin()?;
        read_succession_receipt_in(&transaction, project_id, attempt_id)
    }
}

fn read_succession_receipt_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    attempt_id: SuccessionAttemptId,
) -> RepositoryResult<Option<SuccessionReceipt>> {
    let row: Option<(String, String)> = transaction
        .query_row(
            "SELECT receipt, receipt_hash FROM succession_receipts
             WHERE project_id = ?1 AND attempt_id = ?2",
            params![project_id.to_string(), attempt_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    row.map(|(json, hash)| {
        let document = stored_payload(&json, &hash)?;
        let receipt: SuccessionReceipt = document.deserialize()?;
        receipt.canonicalize()?;
        Ok(receipt)
    })
    .transpose()
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
        let connector = if is_jira_connector(&request.connector) {
            canonical_jira_connector()
        } else {
            request.connector.clone()
        };

        if is_jira_connector(&connector) {
            let by_task = transaction
                .query_row(
                    "SELECT link.id, ledger.external_issue_key, link.revision, link.created_at
                     FROM canonical_jira_task_links AS ledger
                     JOIN jira_links AS link
                       ON link.project_id = ledger.project_id AND link.id = ledger.link_id
                     WHERE ledger.project_id = ?1 AND ledger.task_id = ?2",
                    params![request.project_id.to_string(), request.task_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()
                .map_err(backend)?;
            if let Some((id, issue_key, revision, created_at)) = by_task {
                if issue_key != request.external_issue_key.as_str() {
                    return Err(conflict(
                        "Jira task link",
                        "one task cannot be linked to more than one Jira issue",
                    ));
                }
                return Ok(TicketLink {
                    id: TicketLinkId::parse(&id)?,
                    project_id: request.project_id,
                    task_id: request.task_id,
                    connector,
                    external_issue_key: ExternalId::parse(&issue_key)?,
                    revision: revision_of(revision)?,
                    created_at: parse_utc_timestamp(&created_at)?,
                });
            }

            let task_for_key = transaction
                .query_row(
                    "SELECT task_id FROM canonical_jira_task_links
                     WHERE project_id = ?1 AND external_issue_key = ?2",
                    params![
                        request.project_id.to_string(),
                        request.external_issue_key.as_str()
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(backend)?;
            if task_for_key.is_some() {
                return Err(conflict(
                    "Jira task link",
                    "one Jira issue cannot be linked to more than one task",
                ));
            }
        }

        transaction
            .execute(
                "INSERT INTO jira_links
                     (id, project_id, task_id, connector, external_issue_key, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    connector.as_str(),
                    request.external_issue_key.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        if is_jira_connector(&connector) {
            transaction
                .execute(
                    "INSERT INTO canonical_jira_task_links
                         (project_id, task_id, external_issue_key, link_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        request.project_id.to_string(),
                        request.task_id.to_string(),
                        request.external_issue_key.as_str(),
                        request.id.to_string()
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(TicketLink {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            connector,
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
        let existing = transaction
            .query_row(
                "SELECT observation_id, task_revision, spec_version, milestone
                 FROM status_conflicts
                 WHERE project_id = ?1 AND link_id = ?2 AND kind = ?3
                   AND resolved_at IS NULL",
                params![
                    project_id.to_string(),
                    conflict_record.link_id.to_string(),
                    conflict_record.kind.as_str()
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if let Some((observation_id, task_revision, spec_version, milestone)) = existing {
            let exact = observation_id == conflict_record.observation_id.to_string()
                && task_revision == revision_column(conflict_record.task_revision)?
                && spec_version == version_column(conflict_record.spec_version)
                && milestone.as_deref()
                    == conflict_record
                        .milestone
                        .as_ref()
                        .map(kontor_core::id::SemanticMilestoneKey::as_str);
            if exact {
                return Ok(());
            }
            return Err(conflict(
                "status conflict",
                "an open conflict of this kind already carries different evidence",
            ));
        }
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

// ---------------------------------------------------------------------------
// Team Definition
// ---------------------------------------------------------------------------

/// Read one published Team Definition revision and prove it is the exact bytes
/// the caller pinned.
///
/// The hash comparison is the whole point: the `(project, definition, version)`
/// reference proves the revision exists, and this proves the caller is talking
/// about the same document rather than a lineage position that has since been
/// republished under different bytes.
fn team_definition_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    snapshot: &TeamDefinitionSnapshot,
) -> RepositoryResult<TeamDefinitionSpec> {
    let found: Option<(String, String)> = transaction
        .query_row(
            "SELECT definition, definition_hash FROM team_definitions
             WHERE project_id = ?1 AND definition_id = ?2 AND version = ?3",
            params![
                project_id.to_string(),
                snapshot.definition_id.to_string(),
                version_column(snapshot.version)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (json, hash) = found.ok_or(RepositoryError::NotFound {
        subject: "team definition",
    })?;
    let hash = ContentHash::parse(&hash)?;
    if hash != snapshot.canonical_hash {
        return Err(conflict(
            "team definition",
            "the pinned canonical hash does not match the published revision",
        ));
    }
    stored_document(&json, hash.as_str())
}

/// Read the exact topology document a Team Definition names as its validator.
///
/// Returned as the parsed document rather than as a bare existence check, so
/// publication can compose the definition against the legality rules the
/// project actually published.
fn team_definition_validator_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    definition: &TeamDefinitionSpec,
) -> RepositoryResult<ProjectSessionTopologySpec> {
    let found: Option<(String, String)> = transaction
        .query_row(
            "SELECT definition, definition_hash FROM topology_specs
             WHERE project_id = ?1 AND spec_id = ?2 AND version = ?3",
            params![
                project_id.to_string(),
                definition.topology.spec_id.to_string(),
                version_column(definition.topology.version)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (json, hash) = found.ok_or(RepositoryError::NotFound {
        subject: "topology specification",
    })?;
    stored_document(&json, &hash)
}

/// Read one migration intent and its complete target set inside a transaction.
/// Refuse a topology or seat lifecycle write while its epic's exact native
/// census is frozen by an unsettled Team Definition migration.
///
/// This runs inside the same `IMMEDIATE` transaction as the lifecycle write.
/// The migration recorder uses that same serialization boundary, so either the
/// lifecycle change commits first and the later census observes history, or the
/// migration commits first and this write is refused. There is no check/write
/// race in which a newly historical native can still be retitled.
fn ensure_no_live_native_migration_for_node(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    topology_node_id: TopologyNodeId,
) -> RepositoryResult<()> {
    let fenced: Option<i64> = transaction
        .query_row(
            "SELECT 1
               FROM topology_nodes AS node
               JOIN team_definition_migration_intents AS migration
                 ON migration.project_id = node.project_id
                AND migration.mini_project_id = node.mini_project_id
                AND migration.state IN ('recorded', 'applying')
              WHERE node.project_id = ?1 AND node.id = ?2",
            params![project_id.to_string(), topology_node_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if fenced.is_some() {
        return Err(conflict(
            "team definition migration",
            "topology and seat lifecycle are fenced while native names migrate",
        ));
    }
    Ok(())
}

type TeamDefinitionMigrationRow = (
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
);

fn team_definition_migration_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: TeamDefinitionMigrationId,
) -> RepositoryResult<Option<StoredTeamDefinitionMigration>> {
    let row: Option<TeamDefinitionMigrationRow> = transaction
        .query_row(
            "SELECT mini_project_id, idempotency_key, from_definition_id, from_version,
                    from_canonical_hash, to_definition_id, to_version, to_canonical_hash,
                    state, recorded_at, updated_at, fingerprint
             FROM team_definition_migration_intents
             WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), id.to_string()],
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
                    row.get(10)?,
                    row.get(11)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((
        mini_project_id,
        idempotency_key,
        from_definition_id,
        from_version,
        from_hash,
        to_definition_id,
        to_version,
        to_hash,
        state,
        recorded_at,
        updated_at,
        fingerprint,
    )) = row
    else {
        return Ok(None);
    };
    let from = match (from_definition_id, from_version, from_hash) {
        (Some(definition_id), Some(version), Some(hash)) => Some(TeamDefinitionSnapshot {
            definition_id: TeamDefinitionId::parse(&definition_id)?,
            version: read_version(version)?,
            canonical_hash: ContentHash::parse(&hash)?,
        }),
        _ => None,
    };
    Ok(Some(StoredTeamDefinitionMigration {
        id,
        project_id,
        mini_project_id: MiniProjectId::parse(&mini_project_id)?,
        idempotency_key: IdempotencyKey::parse(&idempotency_key)?,
        fingerprint: ContentHash::parse(&fingerprint)?,
        command_intent_hash: {
            let command_intent: Option<(Option<String>, String)> = transaction
                .query_row(
                    "SELECT intent_hash, source
                     FROM team_definition_migration_command_intents
                     WHERE project_id = ?1 AND intent_id = ?2",
                    params![project_id.to_string(), id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(backend)?;
            match command_intent {
                Some((Some(hash), source)) if source == "issued" || source == "legacy_receipt" => {
                    ContentHash::parse(&hash)?
                }
                Some((None, source)) if source == "legacy_unrecoverable" => {
                    return Err(conflict(
                        "team definition migration",
                        "the pre-v80 migration has no provable exact command intent",
                    ));
                }
                Some(_) => {
                    return Err(conflict(
                        "team definition migration command intent",
                        "the command-intent source and hash disagree",
                    ));
                }
                None => {
                    return Err(RepositoryError::NotFound {
                        subject: "team definition migration command intent",
                    });
                }
            }
        },
        from,
        to: TeamDefinitionSnapshot {
            definition_id: TeamDefinitionId::parse(&to_definition_id)?,
            version: read_version(to_version)?,
            canonical_hash: ContentHash::parse(&to_hash)?,
        },
        state: TeamDefinitionMigrationState::parse(&state)?,
        receipt_id: transaction
            .query_row(
                "SELECT receipt_id FROM team_definition_migration_receipts
                 WHERE project_id = ?1 AND intent_id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(backend)?
            .as_deref()
            .map(CommandReceiptId::parse)
            .transpose()?,
        targets: team_definition_migration_targets_in(transaction, id)?,
        recorded_at: read_timestamp(&recorded_at)?,
        updated_at: read_timestamp(&updated_at)?,
    }))
}

/// Read one intent's targets in deterministic node order.
fn team_definition_migration_targets_in(
    transaction: &Transaction<'_>,
    intent_id: TeamDefinitionMigrationId,
) -> RepositoryResult<Vec<TeamDefinitionMigrationTarget>> {
    let mut statement = transaction
        .prepare(
            "SELECT subject_kind, topology_node_id, seat_binding_id,
                    runtime_kind, native_host, native_generation, native_id,
                    desired_title, desired_parent_native_id, desired_kind, desired_cwd,
                    observed_title, observed_parent_native_id, observed_kind, observed_cwd,
                    state, updated_at
             FROM team_definition_migration_targets
             WHERE intent_id = ?1 ORDER BY target_key",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![intent_id.to_string()])
        .map_err(backend)?;
    let mut targets = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let topology_node_id = TopologyNodeId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
        let seat: Option<String> = row.get(2).map_err(backend)?;
        let subject = match row.get::<_, String>(0).map_err(backend)?.as_str() {
            "seat" => TeamDefinitionMigrationSubject::Seat {
                topology_node_id,
                seat_binding_id: SeatBindingId::parse(&seat.ok_or_else(|| {
                    conflict(
                        "team definition migration target",
                        "a seat target must name its seat",
                    )
                })?)?,
            },
            _ => TeamDefinitionMigrationSubject::Container { topology_node_id },
        };
        let generation = u64::try_from(row.get::<_, i64>(5).map_err(backend)?).map_err(|_| {
            RepositoryError::Backend {
                detail: "a stored native generation is negative".to_owned(),
            }
        })?;
        let identity = NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(&row.get::<_, String>(3).map_err(backend)?)?,
            host: ExternalName::parse(&row.get::<_, String>(4).map_err(backend)?)?,
            generation,
            native_id: ExternalId::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        };
        let desired_parent: Option<String> = row.get(8).map_err(backend)?;
        let desired_cwd: Option<String> = row.get(10).map_err(backend)?;
        let desired = NativePlacement {
            title: ExternalName::parse(&row.get::<_, String>(7).map_err(backend)?)?,
            parent_native_id: desired_parent
                .as_deref()
                .map(ExternalId::parse)
                .transpose()?,
            kind: MigrationObjectKind::parse(&row.get::<_, String>(9).map_err(backend)?)?,
            canonical_cwd: desired_cwd
                .as_deref()
                .map(ExternalName::parse)
                .transpose()?,
        };
        let observed_title: Option<String> = row.get(11).map_err(backend)?;
        let observed_parent: Option<String> = row.get(12).map_err(backend)?;
        let observed_kind: Option<String> = row.get(13).map_err(backend)?;
        let observed_cwd: Option<String> = row.get(14).map_err(backend)?;
        let observed = match (observed_title, observed_kind) {
            (Some(title), Some(kind)) => Some(NativePlacement {
                title: ExternalName::parse(&title)?,
                parent_native_id: observed_parent
                    .as_deref()
                    .map(ExternalId::parse)
                    .transpose()?,
                kind: MigrationObjectKind::parse(&kind)?,
                canonical_cwd: observed_cwd
                    .as_deref()
                    .map(ExternalName::parse)
                    .transpose()?,
            }),
            _ => None,
        };
        targets.push(TeamDefinitionMigrationTarget {
            intent_id,
            subject,
            identity,
            desired,
            observed,
            state: TeamDefinitionMigrationTargetState::parse(
                &row.get::<_, String>(15).map_err(backend)?,
            )?,
            updated_at: read_timestamp(&row.get::<_, String>(16).map_err(backend)?)?,
        });
    }
    Ok(targets)
}

/// Prove one migration's target set covers the epic's live natives exactly.
///
/// Two obligations, both pre-mutation. The target definition has to be able to
/// *name* every live node kind, or the epic would end up pinned to a document
/// that cannot describe part of itself. And every live subject has to be
/// enumerated, or the apply would leave that object rendering the old pin's
/// name while the epic claims the new one.
fn prove_migration_covers_live_natives(
    store: &SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    definition: &TeamDefinitionSpec,
    enumerated: &BTreeMap<TeamDefinitionMigrationSubject, NativeRuntimeIdentity>,
) -> RepositoryResult<()> {
    let live_subjects = store.list_live_native_subjects(project_id, mini_project_id)?;
    let mut observed = BTreeMap::new();
    for live in live_subjects {
        let container = definition.container(&live.node_kind).ok_or_else(|| {
            conflict(
                "team definition migration",
                "the target definition does not declare a live native node kind",
            )
        })?;
        if matches!(live.subject, TeamDefinitionMigrationSubject::Seat { .. })
            && container.seat_name_template.is_none()
        {
            return Err(conflict(
                "team definition migration",
                "the target definition cannot name a live seat of this kind",
            ));
        }
        if observed
            .insert(live.subject, live.identity.clone())
            .is_some()
        {
            return Err(conflict(
                "team definition migration",
                "the live census enumerates one native subject more than once",
            ));
        }
        if enumerated.get(&live.subject) != Some(&live.identity) {
            return Err(conflict(
                "team definition migration",
                "the migration does not match every live native subject and identity of the epic",
            ));
        }
    }
    if observed.len() != enumerated.len() {
        return Err(conflict(
            "team definition migration",
            "the migration enumerates a subject that is not in the live native census",
        ));
    }
    Ok(())
}

impl TeamDefinitionRepository for SqliteStore {
    fn publish_team_definition(
        &self,
        project_id: ProjectId,
        definition: &TeamDefinitionSpec,
        published_at: Timestamp,
    ) -> RepositoryResult<ContentHash> {
        // Canonicalization validates first, so an invalid definition is refused
        // before it can occupy a lineage position it would then hold forever.
        let document = definition.canonicalize()?;
        let transaction = self.begin()?;
        // Compose against the exact topology document, not merely against the
        // existence of a row at that version. `canonicalize` proves the
        // definition is internally complete; only the topology can say whether
        // the kinds, parents, capabilities and read-only policy it asks for are
        // legal, and only these bytes can say the hash it cites is really them.
        // Without this, a definition carrying a forged hash or an illegal
        // parent could become immutable, selected and epic-pinned.
        let topology = team_definition_validator_in(&transaction, project_id, definition)?;
        definition.validate_against(&topology)?;
        transaction
            .execute(
                "INSERT INTO team_definitions
                     (project_id, definition_id, version, name, topology_spec_id,
                      topology_version, definition, definition_hash, published_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    project_id.to_string(),
                    definition.definition_id.to_string(),
                    version_column(definition.version),
                    definition.name.as_str(),
                    definition.topology.spec_id.to_string(),
                    version_column(definition.topology.version),
                    document.json(),
                    document.hash().as_str(),
                    text(published_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_team_definition(
        &self,
        project_id: ProjectId,
        definition_id: TeamDefinitionId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TeamDefinitionSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM team_definitions
                 WHERE project_id = ?1 AND definition_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    definition_id.to_string(),
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

    fn list_team_definitions(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<TeamDefinitionSpec>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT definition, definition_hash FROM team_definitions
                 WHERE project_id = ?1 ORDER BY definition_id, version",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut definitions = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let document = stored_payload(
                &row.get::<_, String>(0).map_err(backend)?,
                &row.get::<_, String>(1).map_err(backend)?,
            )?;
            definitions.push(document.deserialize::<TeamDefinitionSpec>()?);
        }
        Ok(definitions)
    }

    fn set_project_team_definition_default(
        &self,
        selection: &ProjectTeamDefinitionDefault,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        team_definition_in(&transaction, selection.project_id, &selection.definition)?;
        // Compare-and-swap. The apply binds the selection its preview saw, in
        // the same transaction that writes, so a bootstrap that observed "no
        // default" cannot silently overwrite an explicit selection made between
        // its read and its write.
        let current: Option<(String, i64, String)> = transaction
            .query_row(
                "SELECT definition_id, version, canonical_hash
                 FROM project_team_definition_defaults WHERE project_id = ?1",
                params![selection.project_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let current = current
            .map(|(definition_id, version, hash)| {
                Ok::<_, RepositoryError>(TeamDefinitionSnapshot {
                    definition_id: TeamDefinitionId::parse(&definition_id)?,
                    version: read_version(version)?,
                    canonical_hash: ContentHash::parse(&hash)?,
                })
            })
            .transpose()?;
        if current.as_ref() != selection.expected.as_ref() {
            return Err(conflict(
                "project team definition default",
                "the selection changed since the preview it was applied from",
            ));
        }
        transaction
            .execute(
                "INSERT INTO project_team_definition_defaults
                     (project_id, definition_id, version, canonical_hash, selected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project_id) DO UPDATE SET
                     definition_id = excluded.definition_id,
                     version = excluded.version,
                     canonical_hash = excluded.canonical_hash,
                     selected_at = excluded.selected_at",
                params![
                    selection.project_id.to_string(),
                    selection.definition.definition_id.to_string(),
                    version_column(selection.definition.version),
                    selection.definition.canonical_hash.as_str(),
                    text(selection.selected_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn get_project_team_definition_default(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<ProjectTeamDefinitionDefault>> {
        let found: Option<(String, i64, String, String)> = self
            .connection
            .query_row(
                "SELECT definition_id, version, canonical_hash, selected_at
                 FROM project_team_definition_defaults WHERE project_id = ?1",
                params![project_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(definition_id, version, hash, selected_at)| {
                let definition = TeamDefinitionSnapshot {
                    definition_id: TeamDefinitionId::parse(&definition_id)?,
                    version: read_version(version)?,
                    canonical_hash: ContentHash::parse(&hash)?,
                };
                Ok(ProjectTeamDefinitionDefault {
                    project_id,
                    // A read reports itself as its own expectation, so a
                    // caller that previews and then applies passes back
                    // exactly what it saw.
                    expected: Some(definition.clone()),
                    definition,
                    selected_at: read_timestamp(&selected_at)?,
                })
            })
            .transpose()
    }

    fn pin_mini_project_team_definition(
        &self,
        snapshot: &MiniProjectTeamDefinitionSnapshot,
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
        team_definition_in(&transaction, snapshot.project_id, &snapshot.definition)?;
        transaction
            .execute(
                "INSERT INTO mini_project_team_definition_snapshots
                     (mini_project_id, project_id, definition_id, version, canonical_hash,
                      pinned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    snapshot.mini_project_id.to_string(),
                    snapshot.project_id.to_string(),
                    snapshot.definition.definition_id.to_string(),
                    version_column(snapshot.definition.version),
                    snapshot.definition.canonical_hash.as_str(),
                    text(snapshot.pinned_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn get_mini_project_team_definition(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<MiniProjectTeamDefinitionSnapshot>> {
        let found: Option<(String, i64, String, String)> = self
            .connection
            .query_row(
                "SELECT definition_id, version, canonical_hash, pinned_at
                 FROM mini_project_team_definition_snapshots
                 WHERE project_id = ?1 AND mini_project_id = ?2",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(definition_id, version, hash, pinned_at)| {
                Ok(MiniProjectTeamDefinitionSnapshot {
                    project_id,
                    mini_project_id,
                    definition: TeamDefinitionSnapshot {
                        definition_id: TeamDefinitionId::parse(&definition_id)?,
                        version: read_version(version)?,
                        canonical_hash: ContentHash::parse(&hash)?,
                    },
                    pinned_at: read_timestamp(&pinned_at)?,
                })
            })
            .transpose()
    }

    fn list_live_native_subjects(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<LiveNativeSubject>> {
        // Containers first, then seats, each ordered by identity, so the census
        // is the same list every time it is taken.
        let mut statement = self
            .connection
            .prepare(
                "SELECT 'container' AS subject_kind, node.id, NULL AS seat_binding_id, node.kind,
                        container.runtime_kind, container.host, container.generation,
                        container.native_id
                  FROM topology_node_containers AS container
                   JOIN topology_nodes AS node ON node.id = container.topology_node_id
                  WHERE container.project_id = ?1 AND node.mini_project_id = ?2
                    AND node.lifecycle = 'active'
                 UNION ALL
                 SELECT 'seat', node.id, seat.id, node.kind,
                        hosted.runtime_kind, hosted.host, hosted.generation, hosted.native_id
                   FROM hosted_topology_seats AS hosted
                   JOIN seat_bindings AS seat ON seat.id = hosted.seat_binding_id
                   JOIN topology_nodes AS node ON node.id = seat.topology_node_id
                  WHERE hosted.project_id = ?1 AND node.mini_project_id = ?2
                    AND node.lifecycle = 'active' AND seat.lifecycle = 'active'
                 UNION ALL
                 SELECT 'seat', node.id, seat.id, node.kind,
                        consultation.runtime_kind, consultation.host,
                        consultation.generation, consultation.native_id
                   FROM consultation_seats AS consultation
                   JOIN seat_bindings AS seat ON seat.id = consultation.seat_binding_id
                   JOIN topology_nodes AS node ON node.id = seat.topology_node_id
                  WHERE consultation.project_id = ?1 AND node.mini_project_id = ?2
                    AND consultation.native_id IS NOT NULL
                    AND node.lifecycle = 'active' AND seat.lifecycle = 'active'
                 UNION ALL
                 -- Delivery seats. Their native session is held by the agent
                 -- run's runtime binding rather than by any seat table, so a
                 -- census that reads only containers and consultation seats
                 -- misses every live delivery seat in the epic.
                 --
                 -- The slot address is the exact shared key: a run persists
                 -- `role_key` from its slot (`RoleSlotId::as_role_key`, a
                 -- transparent wrapper), and the seat persists that same slot in
                 -- `role_slot_id`. Joining on the team run alone would pair
                 -- every seat with every session, which is why an ordinary
                 -- multi-seat TeamRun must be matched slot by slot.
                 SELECT 'seat', node.id, seat.id, node.kind,
                        binding.runtime_kind, binding.host,
                        binding.generation, binding.native_id
                   FROM runtime_bindings AS binding
                   JOIN agent_runs AS run
                     ON run.id = binding.agent_run_id AND run.project_id = binding.project_id
                   JOIN seat_bindings AS seat
                     ON seat.team_run_id = run.team_run_id
                    AND seat.role_slot_id = run.role_key
                    AND seat.project_id = binding.project_id
                    AND seat.lifecycle = 'active'
                   JOIN topology_nodes AS node ON node.id = seat.topology_node_id
                  WHERE binding.project_id = ?1 AND node.mini_project_id = ?2
                    AND node.lifecycle = 'active'
                 ORDER BY 1, 8",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), mini_project_id.to_string()])
            .map_err(backend)?;
        let mut subjects = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let topology_node_id =
                TopologyNodeId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
            let seat: Option<String> = row.get(2).map_err(backend)?;
            let subject = match row.get::<_, String>(0).map_err(backend)?.as_str() {
                "seat" => TeamDefinitionMigrationSubject::Seat {
                    topology_node_id,
                    seat_binding_id: SeatBindingId::parse(&seat.ok_or_else(|| {
                        conflict("live native subject", "a seat subject must name its seat")
                    })?)?,
                },
                _ => TeamDefinitionMigrationSubject::Container { topology_node_id },
            };
            let generation =
                u64::try_from(row.get::<_, i64>(6).map_err(backend)?).map_err(|_| {
                    RepositoryError::Backend {
                        detail: "a stored native generation is negative".to_owned(),
                    }
                })?;
            subjects.push(LiveNativeSubject {
                subject,
                node_kind: TopologyKindKey::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                identity: NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse(
                        &row.get::<_, String>(4).map_err(backend)?,
                    )?,
                    host: ExternalName::parse(&row.get::<_, String>(5).map_err(backend)?)?,
                    generation,
                    native_id: ExternalId::parse(&row.get::<_, String>(7).map_err(backend)?)?,
                },
            });
        }
        // The slot join is exact, so an ordinary multi-seat TeamRun censuses
        // cleanly. These checks remain for state the join cannot make sense of,
        // and both fail closed rather than skipping a live native.
        let mut owners: BTreeMap<(String, String, u64, String), TeamDefinitionMigrationSubject> =
            BTreeMap::new();
        for live in &subjects {
            if !matches!(live.subject, TeamDefinitionMigrationSubject::Seat { .. }) {
                continue;
            }
            let key = (
                live.identity.runtime_kind.as_str().to_owned(),
                live.identity.host.as_str().to_owned(),
                live.identity.generation,
                live.identity.native_id.as_str().to_owned(),
            );
            if owners
                .insert(key, live.subject)
                .is_some_and(|previous| previous != live.subject)
            {
                return Err(conflict(
                    "live native subject",
                    "one native session is claimed by two seats of this epic",
                ));
            }
        }
        // Every live delivery session on active (or missing) task topology must
        // resolve to exactly one active seat at its slot. Zero is the case
        // observed live — bound scope/implement/verify/audit runs whose active
        // TSW carries no seat rows at all — and more than one is corrupt. Both
        // are refused: excluding such a session silently is precisely the skip
        // this census exists to prevent, and it would let the epic pin move
        // over natives nobody enumerated. A run whose exact task topology is
        // only retired/archived is immutable history and stays outside the
        // migration together with that TSW.
        //
        // Scoped through the run's task rather than through a seat, because a
        // session with no seat has no node to be found by.
        let unresolved: i64 = self
            .connection
            .query_row(
                "SELECT count(*)
                   FROM (
                     SELECT run.id,
                            sum(CASE
                                WHEN seat.id IS NOT NULL
                                 AND seat.lifecycle = 'active'
                                 AND node.lifecycle = 'active'
                                THEN 1 ELSE 0 END) AS active_seats,
                            sum(CASE
                                WHEN seat.id IS NOT NULL
                                 AND (seat.lifecycle <> 'active'
                                      OR node.lifecycle <> 'active')
                                THEN 1 ELSE 0 END) AS historical_seats
                       FROM runtime_bindings AS binding
                       JOIN agent_runs AS run
                         ON run.id = binding.agent_run_id
                        AND run.project_id = binding.project_id
                       JOIN team_runs AS team ON team.id = run.team_run_id
                       JOIN tasks AS task ON task.id = team.task_id
                       LEFT JOIN topology_nodes AS task_node
                         ON task_node.project_id = binding.project_id
                        AND task_node.task_id = task.id
                        AND task_node.lifecycle = 'active'
                       LEFT JOIN seat_bindings AS seat
                         ON seat.team_run_id = run.team_run_id
                        AND seat.role_slot_id = run.role_key
                        AND seat.project_id = binding.project_id
                       LEFT JOIN topology_nodes AS node
                         ON node.id = seat.topology_node_id
                        AND node.project_id = seat.project_id
                      WHERE binding.project_id = ?1
                        AND task.mini_project_id = ?2
                        AND run.lifecycle NOT IN ('succeeded', 'failed', 'cancelled')
                        AND (
                            task_node.id IS NOT NULL
                            OR NOT EXISTS (
                                SELECT 1
                                  FROM topology_nodes AS historical_task_node
                                 WHERE historical_task_node.project_id = binding.project_id
                                   AND historical_task_node.task_id = task.id
                            )
                        )
                      GROUP BY run.id
                   ) AS candidate
                  WHERE candidate.active_seats <> 1
                    AND NOT (
                        candidate.active_seats = 0
                        AND candidate.historical_seats > 0
                    )",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if unresolved > 0 {
            return Err(conflict(
                "live native subject",
                "a live delivery session has no single active seat at its slot",
            ));
        }
        Ok(subjects)
    }

    fn get_team_definition_migration_by_key(
        &self,
        project_id: ProjectId,
        idempotency_key: &IdempotencyKey,
    ) -> RepositoryResult<Option<StoredTeamDefinitionMigration>> {
        let transaction = self.begin()?;
        let found: Option<String> = transaction
            .query_row(
                "SELECT id FROM team_definition_migration_intents
                 WHERE project_id = ?1 AND idempotency_key = ?2",
                params![project_id.to_string(), idempotency_key.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let stored = match found {
            Some(id) => team_definition_migration_in(
                &transaction,
                project_id,
                TeamDefinitionMigrationId::parse(&id)?,
            )?,
            None => None,
        };
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn bind_team_definition_migration_receipt(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        receipt_id: CommandReceiptId,
        bound_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        if stored.state != TeamDefinitionMigrationState::Confirmed {
            return Err(conflict(
                "team definition migration receipt",
                "only a confirmed migration has a command to receipt",
            ));
        }
        match stored.receipt_id {
            // The replay of a recovery that already completed.
            Some(existing) if existing == receipt_id => {
                transaction.commit().map_err(backend)?;
                return Ok(());
            }
            Some(_) => {
                return Err(conflict(
                    "team definition migration receipt",
                    "this migration was already commanded under a different receipt",
                ));
            }
            None => {}
        }
        transaction
            .execute(
                "INSERT INTO team_definition_migration_receipts
                     (intent_id, project_id, receipt_id, bound_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    id.to_string(),
                    project_id.to_string(),
                    receipt_id.to_string(),
                    text(bound_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn record_team_definition_migration(
        &self,
        migration: &NewTeamDefinitionMigration,
    ) -> RepositoryResult<StoredTeamDefinitionMigration> {
        if migration.targets.is_empty() {
            return Err(conflict(
                "team definition migration",
                "a migration must enumerate at least one target",
            ));
        }
        let mut subjects = BTreeMap::new();
        if !migration.targets.iter().all(|target| {
            subjects
                .insert(target.subject, target.identity.clone())
                .is_none()
        }) {
            return Err(conflict(
                "team definition migration",
                "a migration must not enumerate one native subject twice",
            ));
        }
        // Two subjects claiming one native object would make the identity
        // proof ambiguous: the readback could satisfy either of them.
        let mut natives = BTreeSet::new();
        if !migration.targets.iter().all(|target| {
            natives.insert((
                target.identity.runtime_kind.clone(),
                target.identity.host.clone(),
                target.identity.generation,
                target.identity.native_id.clone(),
            ))
        }) {
            return Err(conflict(
                "team definition migration",
                "a migration must not enumerate one native id twice",
            ));
        }
        for target in &migration.targets {
            target.desired.validate()?;
            if !target.desired.matches_subject(target.subject) {
                return Err(conflict(
                    "team definition migration target",
                    "the recorded object kind does not describe the subject it is about",
                ));
            }
        }
        let transaction = self.begin()?;
        // Same key, same migration. A resumed apply continues the intent it
        // already recorded rather than recording a rival to it, so the replay
        // is safe without the caller having to check first.
        let existing: Option<String> = transaction
            .query_row(
                "SELECT id FROM team_definition_migration_intents
                 WHERE project_id = ?1 AND idempotency_key = ?2",
                params![
                    migration.project_id.to_string(),
                    migration.idempotency_key.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let fingerprint = migration.fingerprint();
        if let Some(id) = existing {
            let id = TeamDefinitionMigrationId::parse(&id)?;
            let stored = team_definition_migration_in(&transaction, migration.project_id, id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "team definition migration",
                })?;
            // A replay is the same request asked again, not merely the same
            // key. A key reused for a different epic, target set, identity,
            // title or destination revision is a conflict: reporting it as
            // success would silently discard the request that was actually
            // made.
            if stored.fingerprint != fingerprint {
                return Err(conflict(
                    "team definition migration",
                    "this idempotency key already records a different migration",
                ));
            }
            // Same plan is not the same command. A retry carrying a different
            // preview hash or legacy-topic map is a different request, and
            // answering it with this migration's outcome would report success
            // for something nobody asked for.
            if stored.command_intent_hash != migration.command_intent_hash {
                return Err(conflict(
                    "team definition migration",
                    "this idempotency key was issued under a different command intent",
                ));
            }
            transaction.commit().map_err(backend)?;
            return Ok(stored);
        }
        // The `from` the caller recorded has to be the pin the epic actually
        // holds. A migration that starts from a position the epic left is a
        // migration of something else.
        let current =
            self.get_mini_project_team_definition(migration.project_id, migration.mini_project_id)?;
        if current.as_ref().map(|pin| &pin.definition) != migration.from.as_ref() {
            return Err(conflict(
                "team definition migration",
                "the recorded prior pin is not the epic's current pin",
            ));
        }
        let target = team_definition_in(&transaction, migration.project_id, &migration.to)?;
        // The census proof, before the first external effect: the target
        // definition must be able to name every live node kind, and every live
        // native subject must be enumerated. A migration that silently skips a
        // kind would move the pin while part of the epic still renders the old
        // one's names.
        prove_migration_covers_live_natives(
            self,
            migration.project_id,
            migration.mini_project_id,
            &target,
            &subjects,
        )?;
        // Every target must be a node of *this* epic in *this* project. The
        // composite foreign key proves the project; the epic is proved here,
        // because a node's epic is nullable and a foreign node would otherwise
        // let one migration retitle another epic's natives.
        for target in &migration.targets {
            let owned = transaction
                .query_row(
                    "SELECT 1 FROM topology_nodes
                     WHERE project_id = ?1 AND id = ?2 AND mini_project_id = ?3",
                    params![
                        migration.project_id.to_string(),
                        target.subject.topology_node_id().to_string(),
                        migration.mini_project_id.to_string(),
                    ],
                    |_| Ok(()),
                )
                .optional()
                .map_err(backend)?
                .is_some();
            if !owned {
                return Err(conflict(
                    "team definition migration target",
                    "a target node does not belong to this project and epic",
                ));
            }
        }
        transaction
            .execute(
                "INSERT INTO team_definition_migration_intents
                     (id, project_id, mini_project_id, idempotency_key, fingerprint,
                      from_definition_id, from_version, from_canonical_hash, to_definition_id,
                      to_version, to_canonical_hash, state, recorded_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?12, ?5, ?6, ?7, ?8, ?9, ?10, 'recorded', ?11, ?11)",
                params![
                    migration.id.to_string(),
                    migration.project_id.to_string(),
                    migration.mini_project_id.to_string(),
                    migration.idempotency_key.as_str(),
                    migration
                        .from
                        .as_ref()
                        .map(|from| from.definition_id.to_string()),
                    migration
                        .from
                        .as_ref()
                        .map(|from| version_column(from.version)),
                    migration
                        .from
                        .as_ref()
                        .map(|from| from.canonical_hash.as_str().to_owned()),
                    migration.to.definition_id.to_string(),
                    version_column(migration.to.version),
                    migration.to.canonical_hash.as_str(),
                    text(migration.recorded_at),
                    fingerprint.as_str(),
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO team_definition_migration_command_intents
                     (intent_id, project_id, intent_hash, source, recorded_at)
                 VALUES (?1, ?2, ?3, 'issued', ?4)",
                params![
                    migration.id.to_string(),
                    migration.project_id.to_string(),
                    migration.command_intent_hash.as_str(),
                    text(migration.recorded_at),
                ],
            )
            .map_err(backend)?;
        for target in &migration.targets {
            transaction
                .execute(
                    "INSERT INTO team_definition_migration_targets
                         (intent_id, project_id, target_key, subject_kind, topology_node_id,
                          seat_binding_id, runtime_kind, native_host, native_generation,
                          native_id, desired_title, desired_parent_native_id, desired_kind,
                          desired_cwd, observed_title, observed_parent_native_id,
                          observed_kind, observed_cwd, state, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                             NULL, NULL, NULL, NULL, 'pending', ?15)",
                    params![
                        migration.id.to_string(),
                        migration.project_id.to_string(),
                        target.subject.target_key(),
                        match target.subject {
                            TeamDefinitionMigrationSubject::Container { .. } => "container",
                            TeamDefinitionMigrationSubject::Seat { .. } => "seat",
                        },
                        target.subject.topology_node_id().to_string(),
                        target
                            .subject
                            .seat_binding_id()
                            .map(|seat| seat.to_string()),
                        target.identity.runtime_kind.as_str(),
                        target.identity.host.as_str(),
                        i64::try_from(target.identity.generation).unwrap_or(i64::MAX),
                        target.identity.native_id.as_str(),
                        target.desired.title.as_str(),
                        target
                            .desired
                            .parent_native_id
                            .as_ref()
                            .map(ExternalId::as_str),
                        target.desired.kind.as_str(),
                        target
                            .desired
                            .canonical_cwd
                            .as_ref()
                            .map(ExternalName::as_str),
                        text(migration.recorded_at),
                    ],
                )
                .map_err(backend)?;
        }
        let stored =
            team_definition_migration_in(&transaction, migration.project_id, migration.id)?.ok_or(
                RepositoryError::NotFound {
                    subject: "team definition migration",
                },
            )?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn get_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
    ) -> RepositoryResult<Option<StoredTeamDefinitionMigration>> {
        let transaction = self.begin()?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn get_in_flight_team_definition_migration(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<StoredTeamDefinitionMigration>> {
        let transaction = self.begin()?;
        let found: Option<String> = transaction
            .query_row(
                "SELECT id FROM team_definition_migration_intents
                 WHERE project_id = ?1 AND mini_project_id = ?2
                   AND state IN ('recorded', 'applying')",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let stored = match found {
            Some(id) => team_definition_migration_in(
                &transaction,
                project_id,
                TeamDefinitionMigrationId::parse(&id)?,
            )?,
            None => None,
        };
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn observe_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        observations: &[TeamDefinitionMigrationObservation],
        observed_at: Timestamp,
    ) -> RepositoryResult<StoredTeamDefinitionMigration> {
        let transaction = self.begin()?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        if stored.state.is_terminal() {
            return Err(conflict(
                "team definition migration",
                "a settled migration does not take further observations",
            ));
        }
        for observation in observations {
            let target = stored
                .targets
                .iter()
                .find(|target| target.subject == observation.subject)
                .ok_or(RepositoryError::NotFound {
                    subject: "team definition migration target",
                })?;
            // The identity check that makes this migration identity-preserving.
            // A readback carrying a different native id did not observe the
            // object we asked about, so it can never be recorded against it.
            // The identity check that makes this migration identity-preserving.
            // A readback carrying a different runtime, host, generation or
            // native id did not observe the object we asked about, so it can
            // never be recorded against it.
            if target.identity != observation.identity {
                return Err(conflict(
                    "team definition migration target",
                    "the observed native identity is not the one the migration enumerated",
                ));
            }
            // A success is a readback, not a label. Claiming `renamed` or
            // `unchanged` without the exact desired title and unchanged
            // placement would move the epic pin to bytes the natives do not
            // render, which is precisely what this migration exists to prevent.
            let succeeded = matches!(
                observation.state,
                TeamDefinitionMigrationTargetState::Renamed
                    | TeamDefinitionMigrationTargetState::Unchanged
            );
            if succeeded && observation.observed.as_ref() != Some(&target.desired) {
                return Err(conflict(
                    "team definition migration target",
                    "a success state requires the exact desired title and unchanged placement",
                ));
            }
            let observed = observation.observed.as_ref();
            transaction
                .execute(
                    "UPDATE team_definition_migration_targets
                     SET observed_title = ?1, observed_parent_native_id = ?2,
                         observed_kind = ?3, observed_cwd = ?4, state = ?5, updated_at = ?6
                     WHERE intent_id = ?7 AND target_key = ?8",
                    params![
                        observed.map(|placement| placement.title.as_str()),
                        observed.and_then(|placement| placement
                            .parent_native_id
                            .as_ref()
                            .map(ExternalId::as_str)),
                        observed.map(|placement| placement.kind.as_str()),
                        observed.and_then(|placement| placement
                            .canonical_cwd
                            .as_ref()
                            .map(ExternalName::as_str)),
                        observation.state.as_str(),
                        text(observation.observed_at),
                        id.to_string(),
                        observation.subject.target_key(),
                    ],
                )
                .map_err(backend)?;
        }
        transaction
            .execute(
                "UPDATE team_definition_migration_intents
                 SET state = 'applying', updated_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                params![text(observed_at), project_id.to_string(), id.to_string()],
            )
            .map_err(backend)?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn confirm_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<StoredTeamDefinitionMigration> {
        let transaction = self.begin()?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        if stored.state.is_terminal() {
            return Err(conflict(
                "team definition migration",
                "a settled migration cannot be confirmed again",
            ));
        }
        // Every target, without exception. `rename_pending` is the state that
        // exists precisely so a partial apply cannot be confirmed: the pin does
        // not move until the natives already render what it says they do.
        if !stored.targets.iter().all(|target| {
            matches!(
                target.state,
                TeamDefinitionMigrationTargetState::Renamed
                    | TeamDefinitionMigrationTargetState::Unchanged
            )
        }) {
            return Err(conflict(
                "team definition migration",
                "every target must read back its desired title before the pin moves",
            ));
        }
        // And the epic must still be where the intent left it. Anything else
        // moved the pin behind this migration's back.
        let current = self.get_mini_project_team_definition(project_id, stored.mini_project_id)?;
        if current.as_ref().map(|pin| &pin.definition) != stored.from.as_ref() {
            return Err(conflict(
                "team definition migration",
                "the epic's pin moved while the migration was in flight",
            ));
        }
        let target = team_definition_in(&transaction, project_id, &stored.to)?;
        // Re-prove the census. A native that appeared between preview and
        // confirmation is not covered by this migration, and moving the pin
        // over it would leave it rendering a name the new pin does not describe.
        prove_migration_covers_live_natives(
            self,
            project_id,
            stored.mini_project_id,
            &target,
            &stored
                .targets
                .iter()
                .map(|target| (target.subject, target.identity.clone()))
                .collect::<BTreeMap<_, _>>(),
        )?;
        // The pin and the confirmation commit together: neither half of a
        // migration is allowed to become visible on its own.
        if stored.from.is_some() {
            transaction
                .execute(
                    "UPDATE mini_project_team_definition_snapshots
                     SET definition_id = ?1, version = ?2, canonical_hash = ?3, pinned_at = ?4
                     WHERE project_id = ?5 AND mini_project_id = ?6",
                    params![
                        stored.to.definition_id.to_string(),
                        version_column(stored.to.version),
                        stored.to.canonical_hash.as_str(),
                        text(confirmed_at),
                        project_id.to_string(),
                        stored.mini_project_id.to_string(),
                    ],
                )
                .map_err(backend)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO mini_project_team_definition_snapshots
                         (mini_project_id, project_id, definition_id, version, canonical_hash,
                          pinned_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        stored.mini_project_id.to_string(),
                        project_id.to_string(),
                        stored.to.definition_id.to_string(),
                        version_column(stored.to.version),
                        stored.to.canonical_hash.as_str(),
                        text(confirmed_at),
                    ],
                )
                .map_err(backend)?;
        }
        transaction
            .execute(
                "UPDATE team_definition_migration_intents
                 SET state = 'confirmed', updated_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                params![text(confirmed_at), project_id.to_string(), id.to_string()],
            )
            .map_err(backend)?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    fn fail_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        failed_at: Timestamp,
    ) -> RepositoryResult<StoredTeamDefinitionMigration> {
        let transaction = self.begin()?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        if stored.state.is_terminal() {
            return Err(conflict(
                "team definition migration",
                "a settled migration cannot be abandoned again",
            ));
        }
        // Abandonment is only honest before the first external effect. Once any
        // target has moved, part of the runtime renders the new titles while
        // the epic still holds the old pin, and going terminal would drop the
        // fence and let new materialization resume under a pin that no longer
        // describes the natives. Such a migration stays non-terminal and
        // fenced, and is resumed under the same key.
        if stored
            .targets
            .iter()
            .any(|target| target.state != TeamDefinitionMigrationTargetState::Pending)
        {
            return Err(conflict(
                "team definition migration",
                "a migration with runtime effects stays resumable and fenced",
            ));
        }
        transaction
            .execute(
                "UPDATE team_definition_migration_intents
                 SET state = 'failed', updated_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                params![text(failed_at), project_id.to_string(), id.to_string()],
            )
            .map_err(backend)?;
        let stored = team_definition_migration_in(&transaction, project_id, id)?.ok_or(
            RepositoryError::NotFound {
                subject: "team definition migration",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }
}

impl SqliteStore {
    /// Record one unresolved epic Jira conflict, de-duplicated by epic and kind.
    ///
    /// The first observation remains the evidence an operator resolves. Later
    /// controller passes do not rewrite it or create alert storms.
    pub fn insert_epic_status_conflict(
        &self,
        project_id: ProjectId,
        record: &EpicStatusConflict,
    ) -> RepositoryResult<bool> {
        if record.resolved_at.is_some() || record.resolution_receipt_id.is_some() {
            return Err(RepositoryError::Conflict {
                subject: "epic Jira conflict",
                rule: "a new conflict must be unresolved",
            });
        }
        let transaction = self.begin()?;
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM epic_status_conflicts
                     WHERE project_id = ?1 AND epic_id = ?2 AND kind = ?3
                       AND resolved_at IS NULL
                 )",
                params![
                    project_id.to_string(),
                    record.epic_id.to_string(),
                    record.kind.as_str()
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if exists {
            let existing: (
                String,
                String,
                String,
                String,
                String,
                i64,
                i64,
                Option<String>,
            ) = transaction
                .query_row(
                    "SELECT external_issue_key, observed_status_id, observed_status_name,
                            observed_at, payload_hash, epic_revision, spec_version, milestone
                     FROM epic_status_conflicts
                     WHERE project_id = ?1 AND epic_id = ?2 AND kind = ?3
                       AND resolved_at IS NULL",
                    params![
                        project_id.to_string(),
                        record.epic_id.to_string(),
                        record.kind.as_str()
                    ],
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
                        ))
                    },
                )
                .map_err(backend)?;
            let exact = existing.0 == record.external_issue_key.as_str()
                && existing.1 == record.observed_status.status_id.as_str()
                && existing.2 == record.observed_status.status_name.as_str()
                && existing.3 == text(record.observed_at)
                && existing.4 == record.payload_hash.as_str()
                && existing.5 == revision_column(record.epic_revision)?
                && existing.6 == version_column(record.spec_version)
                && existing.7.as_deref()
                    == record
                        .milestone
                        .as_ref()
                        .map(kontor_core::id::SemanticMilestoneKey::as_str);
            if exact {
                transaction.commit().map_err(backend)?;
                return Ok(false);
            }
            return Err(conflict(
                "epic Jira conflict",
                "an open conflict of this kind already carries different evidence",
            ));
        }
        transaction
            .execute(
                "INSERT INTO epic_status_conflicts
                     (id, project_id, epic_id, kind, external_issue_key,
                      observed_status_id, observed_status_name, observed_at,
                      payload_hash, epic_revision, spec_version, milestone,
                      detected_at, resolved_at, resolution_receipt_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, NULL, NULL)",
                params![
                    record.id.to_string(),
                    project_id.to_string(),
                    record.epic_id.to_string(),
                    record.kind.as_str(),
                    record.external_issue_key.as_str(),
                    record.observed_status.status_id.as_str(),
                    record.observed_status.status_name.as_str(),
                    text(record.observed_at),
                    record.payload_hash.as_str(),
                    revision_column(record.epic_revision)?,
                    version_column(record.spec_version),
                    record
                        .milestone
                        .as_ref()
                        .map(kontor_core::id::SemanticMilestoneKey::as_str),
                    text(record.detected_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(true)
    }

    /// Read every epic Jira conflict in stable detection order.
    pub fn list_epic_status_conflicts(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> RepositoryResult<Vec<EpicStatusConflict>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, kind, external_issue_key, observed_status_id,
                        observed_status_name, observed_at, payload_hash,
                        epic_revision, spec_version, milestone, detected_at,
                        resolved_at, resolution_receipt_id
                 FROM epic_status_conflicts
                 WHERE project_id = ?1 AND epic_id = ?2
                 ORDER BY detected_at, id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(
                params![project_id.to_string(), epic_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, Option<String>>(12)?,
                    ))
                },
            )
            .map_err(backend)?;
        rows.map(|row| {
            let (
                id,
                kind,
                issue_key,
                status_id,
                status_name,
                observed_at,
                payload_hash,
                epic_revision,
                spec_version,
                milestone,
                detected_at,
                resolved_at,
                resolution_receipt_id,
            ) = row.map_err(backend)?;
            Ok(EpicStatusConflict {
                id: StatusConflictId::parse(&id)?,
                epic_id,
                kind: kontor_core::ticket::StatusConflictKind::parse(&kind)?,
                external_issue_key: ExternalId::parse(&issue_key)?,
                observed_status: kontor_core::ticket::StatusSelector {
                    status_id: ExternalId::parse(&status_id)?,
                    status_name: ExternalName::parse(&status_name)?,
                },
                observed_at: read_timestamp(&observed_at)?,
                payload_hash: ContentHash::parse(&payload_hash)?,
                epic_revision: revision_of(epic_revision)?,
                spec_version: read_version(spec_version)?,
                milestone: milestone
                    .as_deref()
                    .map(kontor_core::id::SemanticMilestoneKey::parse)
                    .transpose()?,
                detected_at: read_timestamp(&detected_at)?,
                resolved_at: resolved_at.as_deref().map(read_timestamp).transpose()?,
                resolution_receipt_id: resolution_receipt_id
                    .as_deref()
                    .map(CommandReceiptId::parse)
                    .transpose()?,
            })
        })
        .collect()
    }

    /// Resolve one epic Jira conflict using authority aimed at that exact epic.
    pub fn resolve_epic_status_conflict(
        &self,
        project_id: ProjectId,
        conflict_id: StatusConflictId,
        receipt: CommandReceiptId,
        resolved_at: Timestamp,
    ) -> RepositoryResult<EpicStatusConflict> {
        let transaction = self.begin()?;
        let epic: Option<String> = transaction
            .query_row(
                "SELECT epic_id FROM epic_status_conflicts
                 WHERE project_id = ?1 AND id = ?2 AND resolved_at IS NULL",
                params![project_id.to_string(), conflict_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(epic) = epic else {
            return Err(conflict(
                "epic Jira conflict",
                "the conflict is unknown or already resolved",
            ));
        };
        ensure_receipt_authorizes(
            &transaction,
            "EpicStatusConflict",
            project_id,
            receipt,
            CommandKind::ResolveStatusConflict,
            AggregateRef::MiniProject {
                mini_project_id: MiniProjectId::parse(&epic)?,
            },
        )?;
        let changed = transaction
            .execute(
                "UPDATE epic_status_conflicts
                 SET resolved_at = ?1, resolution_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4 AND resolved_at IS NULL",
                params![
                    text(resolved_at),
                    receipt.to_string(),
                    project_id.to_string(),
                    conflict_id.to_string(),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "epic Jira conflict",
                "the conflict is unknown or already resolved",
            ));
        }
        transaction.commit().map_err(backend)?;
        self.list_epic_status_conflicts(project_id, MiniProjectId::parse(&epic)?)?
            .into_iter()
            .find(|candidate| candidate.id == conflict_id)
            .ok_or_else(|| {
                conflict(
                    "epic Jira conflict",
                    "the resolved conflict did not read back",
                )
            })
    }

    /// Persist authority for one epic Jira transition before the external call.
    pub fn insert_epic_transition_intent(
        &self,
        project_id: ProjectId,
        intent: &EpicStatusTransitionIntent,
    ) -> RepositoryResult<CommandReceiptId> {
        if intent.confirmed_at.is_some() || intent.confirmation_payload_hash.is_some() {
            return Err(RepositoryError::Conflict {
                subject: "epic Jira transition intent",
                rule: "a new transition intent must be unconfirmed",
            });
        }
        let transaction = self.begin()?;
        let existing: Option<EpicTransitionIntentRow> = transaction
            .query_row(
                "SELECT id, external_issue_key, intent_hash, epic_revision,
                        spec_version, milestone, target_status_id,
                        target_status_name, destination_status_id,
                        destination_status_name, prior_payload_hash
                 FROM epic_jira_transition_intents
                 WHERE project_id = ?1 AND idempotency_key = ?2",
                params![project_id.to_string(), intent.idempotency_key.as_str()],
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
                        row.get(10)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if let Some(existing) = existing {
            let exact = existing.1 == intent.external_issue_key.as_str()
                && existing.2 == intent.intent_hash.as_str()
                && existing.3 == revision_column(intent.epic_revision)?
                && existing.4 == version_column(intent.spec_version)
                && existing.5 == intent.milestone.as_str()
                && existing.6 == intent.target.status_id.as_str()
                && existing.7 == intent.target.status_name.as_str()
                && existing.8 == intent.destination.status_id.as_str()
                && existing.9 == intent.destination.status_name.as_str()
                && existing.10 == intent.prior_payload_hash.as_str();
            if !exact {
                return Err(conflict(
                    "epic Jira transition intent",
                    "an idempotency key already authorizes another transition",
                ));
            }
            transaction.commit().map_err(backend)?;
            return CommandReceiptId::parse(&existing.0).map_err(RepositoryError::from);
        }
        transaction
            .execute(
                "INSERT INTO epic_jira_transition_intents
                     (id, project_id, epic_id, external_issue_key,
                      idempotency_key, intent_hash, epic_revision, spec_version,
                      milestone, target_status_id, target_status_name,
                      destination_status_id, destination_status_name,
                      prior_payload_hash, planned_at, confirmed_at,
                      confirmation_payload_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, ?14, ?15, NULL, NULL)",
                params![
                    intent.id.to_string(),
                    project_id.to_string(),
                    intent.epic_id.to_string(),
                    intent.external_issue_key.as_str(),
                    intent.idempotency_key.as_str(),
                    intent.intent_hash.as_str(),
                    revision_column(intent.epic_revision)?,
                    version_column(intent.spec_version),
                    intent.milestone.as_str(),
                    intent.target.status_id.as_str(),
                    intent.target.status_name.as_str(),
                    intent.destination.status_id.as_str(),
                    intent.destination.status_name.as_str(),
                    intent.prior_payload_hash.as_str(),
                    text(intent.planned_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(intent.id)
    }

    /// Attach connector-confirmed readback to an existing epic Jira intent.
    pub fn confirm_epic_transition_intent(
        &self,
        project_id: ProjectId,
        id: CommandReceiptId,
        payload_hash: &ContentHash,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let existing: Option<(Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT confirmed_at, confirmation_payload_hash
                 FROM epic_jira_transition_intents
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        match existing {
            None => {
                return Err(RepositoryError::NotFound {
                    subject: "epic Jira transition intent",
                });
            }
            Some((Some(stored_at), Some(stored_hash))) => {
                if stored_at != text(confirmed_at) || stored_hash != payload_hash.as_str() {
                    return Err(conflict(
                        "epic Jira transition intent",
                        "the transition already carries another confirmation",
                    ));
                }
                transaction.commit().map_err(backend)?;
                return Ok(());
            }
            Some((None, None)) => {}
            Some(_) => {
                return Err(conflict(
                    "epic Jira transition intent",
                    "the stored confirmation is incomplete",
                ));
            }
        }
        transaction
            .execute(
                "UPDATE epic_jira_transition_intents
                 SET confirmed_at = ?1, confirmation_payload_hash = ?2
                 WHERE project_id = ?3 AND id = ?4
                   AND confirmed_at IS NULL",
                params![
                    text(confirmed_at),
                    payload_hash.as_str(),
                    project_id.to_string(),
                    id.to_string()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Recover any interrupted epic transition whose destination is now the
    /// freshly observed Jira status.
    pub fn confirm_matching_epic_transition_intents(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        external_issue_key: &ExternalId,
        destination_status_id: &ExternalId,
        payload_hash: &ContentHash,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<usize> {
        let transaction = self.begin()?;
        let changed = transaction
            .execute(
                "UPDATE epic_jira_transition_intents
                 SET confirmed_at = ?1, confirmation_payload_hash = ?2
                 WHERE project_id = ?3 AND epic_id = ?4
                   AND external_issue_key = ?5
                   AND destination_status_id = ?6
                   AND confirmed_at IS NULL",
                params![
                    text(confirmed_at),
                    payload_hash.as_str(),
                    project_id.to_string(),
                    epic_id.to_string(),
                    external_issue_key.as_str(),
                    destination_status_id.as_str(),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(changed)
    }
}
