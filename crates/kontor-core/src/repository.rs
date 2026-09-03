//! Aggregate entities and the repository ports that persist them.
//!
//! These are *ports*, not a data-access layer: one trait per aggregate, only the
//! operations this ticket needs, and no generic CRUD. Every read is explicitly
//! project-scoped, because a globally unique UUID is not tenant isolation — one
//! database holds many projects, and a valid id from another project must not
//! resolve.
//!
//! No implementation here exposes a raw SQL escape hatch. Everything a later
//! service needs must arrive as a named operation on one of these traits.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, ChildCalendarWindows, ExecutionAuthorization,
    HolidayImportBatch, HolidaySourceRevision, OverrideRevocation, ScheduleOverride,
    WorkCalendarAssignment, WorkScope,
};
use crate::consultation::{
    CommitteeRole, CommitteeVerdict, ConsultationFamily, ConsultationRunId, ConsultationRunState,
};
use crate::id::{
    AccountProfileId, AdvisorRunId, AgentRunId, AggregateRevision, ArtifactKey, BoundedText,
    CalendarExceptionId, CalendarProfileId, CanonicalDocument, CapacityObservationId,
    CommandReceiptId, ConnectorKey, ContentHash, CredentialAlias, EventCursor,
    ExecutionAuthorizationId, ExternalId, ExternalIssueTypeKey, ExternalName, ExternalProjectKey,
    GateKey, GuardrailEvaluationId, IdempotencyKey, IntakeDecisionId, IntakeReceiptId,
    MiniProjectId, ModuleKey, OpenQuestionId, PersonaScenarioId, PhaseKey, ProjectId,
    ProviderUsageObservationId, QuickSessionId, RealmId, RoleCatalogId, RoleKey, RoleSlotId,
    RuntimeBindingId, RuntimeKindKey, ScheduleOverrideId, SeatBindingId, SourceEventId,
    SpecVersion, StatusConflictId, TaskId, TaskWorkflowId, TeamDefinitionId,
    TeamDefinitionMigrationId, TeamRunId, TeamTemplateId, TicketLinkId, Timestamp, TopologyKindKey,
    TopologyNodeId, TopologySpecId, TriggerKey, WorkCalendarId, WorkProfileKey,
};
use crate::open_question::{
    AmbiguityRound, Disposition, OpenQuestion, OpenQuestionSummary, TriggerFiring,
};
use crate::realm::{EventEnvelope, RealmCursor, ReceiptEnvelope, SnapshotEnvelope};
use crate::receipt::{
    AggregateRef, CommandKind, CommandOutboxEntry, CommandReceipt, CommandReceiptState,
    NoEffectEvidence,
};
use crate::spec::{
    CanonicalSourceEvent, CatalogRoleRef, ExecutionCapability, IntakeReceipt,
    PersonaScenarioSnapshot, PersonaScenarioSpec, ProjectSessionTopologySpec,
    ResolvedWorkProfileSnapshot, RoleCatalogRevision, Shareability, SourceIdentity,
    TeamDefinitionSnapshot, TeamDefinitionSpec, TeamRunSnapshot, TeamTemplateRevision,
    TopologySnapshot, TriggerSpec, WorkProfileSpec,
};
use crate::state::{
    AdaptiveAdmissionState, DesiredRunState, GateState, GateVerdict, NativeContainerBinding,
    NativeRuntimeIdentity, ObservedContainerKind, ObservedRunState, RunLifecycle, RunProjection,
    SeatAttachment, SeatBinding, SessionTopologyNode, TaskState, TaskTeamClosure,
    TeamTerminalEvidence, TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
    TopologyLifecycle,
};
use crate::ticket::{
    ExternalCommentRevision, ExternalTicketObservation, ExternalWorkflowSpec, StatusConflict,
    StatusTransitionReceipt, TicketFieldSpec, TicketSyncProjection,
};
use crate::{DomainError, DomainResult};

/// Everything a repository can refuse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RepositoryError {
    /// The domain rejected the value before any row was written.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// The requested row does not exist in the requested project.
    #[error("{subject} not found in this project")]
    NotFound {
        /// What was looked up.
        subject: &'static str,
    },
    /// A uniqueness, immutability or ordering rule refused the write.
    #[error("{subject} conflict: {rule}")]
    Conflict {
        /// The rule's subject.
        subject: &'static str,
        /// The rule that refused.
        rule: &'static str,
    },
    /// A configured concurrency ceiling is already spent.
    ///
    /// Deliberately *not* a [`RepositoryError::Conflict`]. A conflict says the
    /// caller worked from a state that has moved, and the way out of one is to
    /// re-read and retry against the current revision. A spent ceiling is the
    /// opposite: the presented state was fine and re-reading changes nothing —
    /// it clears only when other work finishes. Collapsing the two tells an
    /// operator to retry in a way that can never succeed.
    ///
    /// `scope` is diagnostic and stays off the wire: it names which ceiling
    /// bound, which is a fact about this realm's configuration and its current
    /// load. See `ApiError::from_repository`, which maps this variant to one
    /// static rule naming no scope at all.
    #[error("the {scope} concurrency ceiling is already spent")]
    CapacityExhausted {
        /// Which ceiling bound. Never disclosed.
        scope: &'static str,
    },
    /// A legacy system still owns the subject this write belongs to.
    ///
    /// Deliberately *not* a [`RepositoryError::Conflict`], for the same reason
    /// [`RepositoryError::CapacityExhausted`] is not one: a conflict says the
    /// caller worked from a state that has moved, and its way out is to re-read
    /// and retry. Withheld authority is not moved state — re-reading returns the
    /// same answer, and it clears only when that subject is imported and switched.
    #[error("{subject} authority for this project is not Kontor's yet")]
    AuthorityWithheld {
        /// Which subject withheld the write: `memory` or `backlog`.
        subject: &'static str,
    },
    /// A reference pointed at a row owned by a different project.
    #[error("{subject} references another project")]
    CrossProject {
        /// The reference that crossed the boundary.
        subject: &'static str,
    },
    /// The storage backend failed. The message never carries row values.
    #[error("storage backend error: {detail}")]
    Backend {
        /// Backend detail, free of persisted payloads.
        detail: String,
    },
}

/// Convenience alias for repository operations.
pub type RepositoryResult<T> = Result<T, RepositoryError>;

// ---------------------------------------------------------------------------
// Aggregates
// ---------------------------------------------------------------------------

/// A project: the tenant boundary inside one Kontor database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// The project.
    pub id: ProjectId,
    /// Human name.
    pub name: ExternalName,
    /// Absolute root path on disk. Unique across the database.
    pub root_path: ExternalName,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
}

/// A new project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    /// The id to use.
    pub id: ProjectId,
    /// Human name.
    pub name: ExternalName,
    /// Absolute root path on disk.
    pub root_path: ExternalName,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// A goal-sized unit of work inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniProject {
    /// The goal.
    pub id: MiniProjectId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Human name.
    pub name: ExternalName,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
}

/// A new goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMiniProject {
    /// The id to use.
    pub id: MiniProjectId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Human name.
    pub name: ExternalName,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// The selected topology revision for future project scopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTopologyDefault {
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact published revision and hash.
    pub topology: TopologySnapshot,
    /// Selection instant.
    pub selected_at: Timestamp,
}

/// One immutable published Project Core Team revision, as it is stored.
///
/// The seats are held as the canonical document the application layer resolved,
/// rather than as columns this layer would have to re-validate. The store's
/// obligation is that the revision it returns is byte-identical to the one that
/// was published — not that it can independently re-derive a role's standard
/// title, which is the catalog's job and is already pinned by `catalog_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCoreTeamRevision {
    /// Owning project.
    pub project_id: ProjectId,
    /// Monotonic revision within the project.
    pub version: SpecVersion,
    /// Canonical hash of the exact role catalog the seats resolved against.
    pub catalog_hash: ContentHash,
    /// The resolved seats, in their published order.
    pub seats: serde_json::Value,
    /// Publication instant.
    pub published_at: Timestamp,
}

/// One published Advisor profile or Committee template revision, as it is
/// stored.
///
/// The definition is held as the canonical document the domain produced, for
/// the same reason `StoredCoreTeamRevision` holds its seats that way: the
/// store's obligation is that what it returns is byte-identical to what was
/// published, and `definition_hash` already pins the typed value it was
/// canonicalized from. Re-deriving whether the document is publishable is the
/// specification's job, and it has already been done once, before the write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConsultationProfileRevision {
    /// Owning project.
    pub project_id: ProjectId,
    /// Which family this revision belongs to.
    pub family: ConsultationFamily,
    /// The profile or template identity shared by every revision of it.
    pub profile_id: String,
    /// Monotonic version within `profile_id`.
    pub version: SpecVersion,
    /// The label frozen at publish.
    pub name: ExternalName,
    /// The canonical definition, byte-for-byte as published.
    ///
    /// Held as the canonical text rather than as a re-serialized value: a
    /// `serde_json::Value` round-trip is only incidentally byte-stable, and the
    /// digest below is over these exact bytes.
    pub definition: String,
    /// Digest of that canonical definition.
    pub definition_hash: ContentHash,
    /// Publication instant.
    pub published_at: Timestamp,
}

/// One repository-backed Advisor or Committee invocation.
///
/// The exact policy revision, question, context document and topology id are
/// frozen before the first runtime effect. `result` is absent until settlement
/// and, once present, is immutable by repository rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConsultationRun {
    /// Family-qualified run identity.
    pub id: ConsultationRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Epic the consultation belongs to.
    pub mini_project_id: MiniProjectId,
    /// Exact published profile/template identity.
    pub profile_id: String,
    /// Exact published profile/template version.
    pub profile_version: SpecVersion,
    /// Digest of the pinned definition.
    pub definition_hash: ContentHash,
    /// The bounded topic this consultation is about, when one is authoritative.
    ///
    /// This is what the ASW/CSW name templates render, and it is deliberately
    /// not derived from [`Self::question`]: the naming contract forbids
    /// inferring a topic from prose, a profile, a title, a UUID or an AI label.
    /// `None` is the honest state of every consultation recorded before the
    /// topic existed — those rows stay readable and keep their historical
    /// names, and a migration that would have to render one of them fails
    /// closed until an operator supplies the mapping. New invocations carry a
    /// topic from the start.
    pub topic: Option<ExternalName>,
    /// Bounded question asked at invocation.
    pub question: BoundedText,
    /// Digest of the question bytes.
    pub question_hash: ContentHash,
    /// Canonical frozen input and provenance.
    pub context: serde_json::Value,
    /// Digest of the canonical context document.
    pub context_hash: ContentHash,
    /// Exact caller seat whose role was authorized by the pinned definition.
    pub caller_seat_binding_id: SeatBindingId,
    /// Dedicated ASW/CSW node, stable across retries and restarts.
    pub topology_node_id: TopologyNodeId,
    /// Invocation idempotency key, persisted before native effects.
    pub invoke_key: IdempotencyKey,
    /// Canonical invocation intent digest bound to that key.
    pub invoke_intent_hash: ContentHash,
    /// Current lifecycle.
    pub state: ConsultationRunState,
    /// One-based Committee round; Advisors remain on round one.
    pub round: u32,
    /// Immutable family-specific result after settlement.
    pub result: Option<serde_json::Value>,
    /// Digest of `result`, when settled.
    pub result_hash: Option<ContentHash>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Creation instant.
    pub created_at: Timestamp,
    /// Last durable change.
    pub updated_at: Timestamp,
    /// Settlement instant.
    pub settled_at: Option<Timestamp>,
}

/// The immutable evidence authored by one Advisor seat before disposition.
///
/// This is deliberately not the consultation result: the requester or owning
/// LSA records that later, after considering these frozen bytes. Keeping the
/// artifact separate makes it impossible for the disposition authority to
/// rewrite the Advisor's output in the same command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAdvisorAdvice {
    /// Advisor invocation the evidence belongs to.
    pub advisor_run_id: AdvisorRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact attested Advisor SeatBinding that submitted it.
    pub seat_binding_id: SeatBindingId,
    /// Canonical evidence document.
    pub document: serde_json::Value,
    /// Digest of the canonical evidence bytes.
    pub document_hash: ContentHash,
    /// Append instant.
    pub recorded_at: Timestamp,
}

/// One template-declared native consultation seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConsultationSeat {
    /// Owning consultation.
    pub run_id: ConsultationRunId,
    /// Stable template slot.
    pub role_slot_id: RoleSlotId,
    /// Committee function. Advisors have no Committee role.
    pub committee_role: Option<CommitteeRole>,
    /// Logical role used for policy and display.
    pub logical_role: RoleKey,
    /// Exact persistent topology seat.
    pub seat_binding_id: SeatBindingId,
    /// Exact selected provider/model/effort rung.
    pub model_rung: crate::spec::ModelRung,
    /// Monotonic native-filler occupancy generation. A scoped credential is
    /// valid only while this exact generation remains active.
    pub occupancy_generation: u64,
    /// Runtime readback after launch/recovery.
    pub native_identity: Option<NativeRuntimeIdentity>,
    /// Provider-native conversation id, when the runtime exposes one.
    pub provider_session_id: Option<ExternalId>,
    /// When the native identity was last read back.
    pub observed_at: Option<Timestamp>,
}

/// Durable receipt-first intent for replacing one consultation native filler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConsultationRecoveryAttempt {
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning consultation.
    pub run_id: ConsultationRunId,
    /// Stable template slot.
    pub role_slot_id: RoleSlotId,
    /// Immutable logical seat identity.
    pub seat_binding_id: SeatBindingId,
    /// Exact predecessor fenced by this attempt.
    pub predecessor_native_id: ExternalId,
    /// Credential generation held by the predecessor.
    pub predecessor_occupancy_generation: u64,
    /// Credential generation reserved for the successor.
    pub successor_occupancy_generation: u64,
    /// Committee revision on which the recovery was authorized.
    pub predecessor_run_revision: AggregateRevision,
    /// Committee revision committed by the durable fence.
    pub prepared_run_revision: AggregateRevision,
    /// Stable recovery reason.
    pub recovery_reason: String,
    /// Digest of the exact API intent that prepared this attempt.
    pub request_intent_hash: ContentHash,
    /// Canonical immutable ordered recovery profile.
    pub recovery_profile: serde_json::Value,
    /// Digest of the canonical recovery profile.
    pub recovery_profile_hash: ContentHash,
    /// First admissible route selected from the profile.
    pub selected_model_rung: crate::spec::ModelRung,
    /// Forward-only saga state.
    pub state: String,
    /// Exact observed successor, once launched/adopted.
    pub successor_native_identity: Option<NativeRuntimeIdentity>,
    /// Provider-native successor conversation, when exposed.
    pub successor_provider_session_id: Option<ExternalId>,
    /// Successor observation instant.
    pub successor_observed_at: Option<Timestamp>,
    /// Preparation instant.
    pub prepared_at: Timestamp,
    /// Exact predecessor retirement instant, once observed.
    pub retired_at: Option<Timestamp>,
    /// Installation instant, once committed.
    pub installed_at: Option<Timestamp>,
}

/// Inputs frozen before one consultation recovery reaches the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConsultationRecoveryAttempt {
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact active predecessor the caller read.
    pub predecessor: StoredConsultationSeat,
    /// Consultation revision read before fencing.
    pub expected_revision: AggregateRevision,
    /// Stable recovery reason.
    pub recovery_reason: String,
    /// Digest of the exact API intent being prepared.
    pub request_intent_hash: ContentHash,
    /// Canonical ordered recovery profile.
    pub recovery_profile: CanonicalDocument,
    /// First admissible route selected from that profile.
    pub selected_model_rung: crate::spec::ModelRung,
    /// Preparation instant.
    pub prepared_at: Timestamp,
}

/// One immutable native-less materialization reroute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConsultationMaterializationReroute {
    /// Owning project.
    pub project_id: ProjectId,
    /// Preserved consultation run.
    pub run_id: ConsultationRunId,
    /// Preserved template slot.
    pub role_slot_id: RoleSlotId,
    /// Preserved logical seat.
    pub seat_binding_id: SeatBindingId,
    /// Credential generation fenced by the reroute.
    pub predecessor_occupancy_generation: u64,
    /// New active credential generation.
    pub successor_occupancy_generation: u64,
    /// Route that failed before native launch.
    pub predecessor_model_rung: crate::spec::ModelRung,
    /// Exact governed replacement route.
    pub successor_model_rung: crate::spec::ModelRung,
    /// Typed recovery reason.
    pub reason: String,
    /// Canonical ordered recovery policy.
    pub recovery_profile: serde_json::Value,
    /// Digest of the recovery policy.
    pub recovery_profile_hash: ContentHash,
    /// Digest of the exact command intent.
    pub request_intent_hash: ContentHash,
    /// Stable command replay key.
    pub idempotency_key: IdempotencyKey,
    /// Exact enabled account whose fresh provider report admitted the route.
    pub headroom_account_profile_id: AccountProfileId,
    /// Immutable successful provider-usage observation used at commit.
    pub headroom_observation_id: ProviderUsageObservationId,
    /// Evidence digest shared by the observation and current projection.
    pub headroom_evidence_hash: ContentHash,
    /// Run revision fenced by the reroute.
    pub predecessor_run_revision: AggregateRevision,
    /// Run revision after the reroute.
    pub successor_run_revision: AggregateRevision,
    /// Commit instant.
    pub rerouted_at: Timestamp,
}

/// Compare-and-swap input for a native-less materialization reroute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConsultationMaterializationReroute {
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact active native-less seat read by the caller.
    pub predecessor: StoredConsultationSeat,
    /// Committee revision read by the caller.
    pub expected_revision: AggregateRevision,
    /// Selected governed successor route.
    pub successor_model_rung: crate::spec::ModelRung,
    /// Typed recovery reason.
    pub reason: String,
    /// Canonical ordered recovery policy.
    pub recovery_profile: CanonicalDocument,
    /// Digest of the exact command intent.
    pub request_intent_hash: ContentHash,
    /// Stable command replay key.
    pub idempotency_key: IdempotencyKey,
    /// Exact fresh immutable headroom observation selected by policy.
    pub headroom_observation: ProviderUsageObservation,
    /// Oldest provider observation that is still admissible at commit.
    pub headroom_fresh_after: Timestamp,
    /// Commit instant.
    pub rerouted_at: Timestamp,
}

/// Exact runtime readback filling a persistent non-delivery topology seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredHostedTopologySeat {
    /// Owning project.
    pub project_id: ProjectId,
    /// Logical seat identity preserved across native recovery.
    pub seat_binding_id: SeatBindingId,
    /// Frozen provider/model/effort route.
    pub model_rung: crate::spec::ModelRung,
    /// Exact native runtime identity.
    pub native_identity: NativeRuntimeIdentity,
    /// Provider conversation id, when exposed.
    pub provider_session_id: Option<ExternalId>,
    /// Runtime readback instant.
    pub observed_at: Timestamp,
}

/// One immutable Committee finding or Judge aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCommitteeFinding {
    /// Owning Committee.
    pub committee_run_id: crate::id::CommitteeRunId,
    /// One-based round.
    pub round: u32,
    /// Exact template slot that submitted it.
    pub role_slot_id: RoleSlotId,
    /// Whether this is an independent finding or the Judge aggregate.
    pub role: CommitteeRole,
    /// Typed verdict. The Judge must match the server-recomputed value.
    pub verdict: CommitteeVerdict,
    /// Whether every required evidence reference was supplied.
    pub evidence_complete: bool,
    /// Canonical bounded evidence document.
    pub document: serde_json::Value,
    /// Digest of the exact document.
    pub document_hash: ContentHash,
    /// Submission instant.
    pub recorded_at: Timestamp,
}

/// One durable Quick session, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredQuickSession {
    /// Session identity.
    pub id: QuickSessionId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The exact catalog role its seat fills.
    pub role: CatalogRoleRef,
    /// The stable slot the role occupies.
    pub role_slot_id: RoleSlotId,
    /// The QSW hosting it.
    pub topology_node_id: TopologyNodeId,
    /// Its one seat.
    pub seat_binding_id: SeatBindingId,
    /// The session base it was placed under.
    pub psw_topology_node_id: TopologyNodeId,
    /// The native project observed for that base at placement, when one had
    /// been observed by then.
    pub psw_native_id: Option<ExternalId>,
    /// What the session is for. Recorded, never interpreted.
    pub purpose: BoundedText,
    /// The canonical intent of the command that opened it, which is how a
    /// retry that lost its answer finds this row instead of opening a second.
    pub intent_hash: ContentHash,
    /// What has become of the source.
    pub disposition: SourceDisposition,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// What promotion does with the Quick session it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDisposition {
    /// Keep the source durable and idle. The default, and the only one a
    /// promotion that was not asked to archive may produce.
    Idle,
    /// Archive the source, after the handoff has been delivered.
    Archive,
}

impl SourceDisposition {
    /// The stable spelling used in JSON and SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Archive => "archive",
        }
    }

    /// Parse the stable spelling.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for any other text.
    pub fn parse(text: &str) -> DomainResult<Self> {
        match text {
            "idle" => Ok(Self::Idle),
            "archive" => Ok(Self::Archive),
            _ => Err(DomainError::invalid(
                "SourceDisposition",
                "is not a known value",
            )),
        }
    }
}

/// One promotion of one Quick session into an epic.
///
/// Written before the first effect, carrying the ids those effects use, so a
/// resumed apply reconciles the same MiniProject rather than building a second.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPromotion {
    /// The source session.
    pub quick_session_id: QuickSessionId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic this promotion creates, frozen at authorization.
    pub mini_project_id: MiniProjectId,
    /// The digest of the plan this apply was authorized against.
    pub preview_hash: ContentHash,
    /// What the source becomes once delivery succeeds.
    pub source_disposition: SourceDisposition,
    /// The exact delivered handoff, once it has been delivered.
    pub handoff: Option<serde_json::Value>,
    /// That handoff's digest.
    pub handoff_hash: Option<ContentHash>,
    /// The seat it was delivered to.
    pub lsa_seat_binding_id: Option<SeatBindingId>,
    /// When delivery completed. Absent while the promotion is still in flight.
    pub completed_at: Option<Timestamp>,
    /// Authorization instant.
    pub created_at: Timestamp,
}

/// The roster one epic is staffed from, frozen at promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEpicRoster {
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic.
    pub mini_project_id: MiniProjectId,
    /// The Core Team revision this epic pinned.
    pub core_team_version: SpecVersion,
    /// The catalog that revision resolved against.
    pub catalog_hash: ContentHash,
    /// The frozen seats, in their published order.
    pub seats: serde_json::Value,
    /// The session this epic was promoted from, when it was.
    pub quick_session_id: Option<QuickSessionId>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Freezing instant.
    pub pinned_at: Timestamp,
}

/// One published, immutable epic Completion Profile revision.
///
/// The definition is the canonical document rather than a decoded profile: this
/// crate holds the persistence vocabulary and `kontor-scheduler` holds the
/// completion types, so decoding here would invert that dependency. The digest
/// travels beside the bytes so a reader can prove the two agree without
/// re-serializing — which is what a re-serialize would silently paper over if a
/// stored row had drifted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCompletionProfile {
    /// Owning project.
    pub project_id: ProjectId,
    /// Stable logical profile id.
    pub id: ExternalName,
    /// Immutable revision.
    pub version: SpecVersion,
    /// Frozen human label.
    pub name: ExternalName,
    /// The canonical published definition.
    pub definition: serde_json::Value,
    /// Digest of the canonical definition.
    pub definition_hash: ContentHash,
    /// Publication instant.
    pub published_at: Timestamp,
}

/// What an atomic completion commit does to the run row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionWrite {
    /// Create the run already carrying its first transition.
    Create,
    /// Advance the run standing at this revision.
    Advance(AggregateRevision),
    /// Verify exact state and record only a replay receipt.
    Unchanged,
}

/// One epic's durable completion run.
///
/// The pinned profile identity is stored as columns beside the state document,
/// so a read can prove which revision this run froze without decoding the
/// state. That matters on restore: a state whose pin disagreed with the profile
/// it is being compiled against has to refuse, and it cannot refuse on a field
/// it needed the profile to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEpicCompletion {
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic.
    pub mini_project_id: MiniProjectId,
    /// The profile this run froze.
    pub profile_id: ExternalName,
    /// The exact pinned revision.
    pub profile_version: SpecVersion,
    /// The pinned definition's digest.
    pub definition_hash: ContentHash,
    /// The canonical completion state document.
    pub state: serde_json::Value,
    /// Optimistic-concurrency revision, mirroring the state's own.
    pub revision: AggregateRevision,
    /// Last transition instant.
    pub updated_at: Timestamp,
}

/// One epic LSA remediation proposal awaiting its TPM route.
///
/// The first half of a two-authority approval. It is a row of its own rather
/// than a field on the completion state because the state records an
/// authorization only when it is complete — a half-filled one stored there would
/// read as approved to everything that consumes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRemediationProposal {
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic.
    pub mini_project_id: MiniProjectId,
    /// Reopening era whose failed round this proposal answers.
    pub completion_generation: u32,
    /// The failed round this answers.
    pub round: u8,
    /// That round's evidence digest, as the proposer read it.
    pub failed_round_evidence: ContentHash,
    /// The proposed bounded correction.
    pub proposal: ContentHash,
    /// The exact seat that proposed.
    pub lsa_seat_binding_id: SeatBindingId,
    /// Native occupancy generation that authenticated the proposal.
    pub lsa_occupancy_generation: u64,
    /// Proposal instant.
    pub proposed_at: Timestamp,
}

/// One recorded intent to wake an epic's existing TPM seat.
///
/// The primary key is `(epic, completion revision, reason, seat)` because that
/// is exactly what "one wake per observation" means. A duplicate observation or
/// a replayed callback collides with the row already there and reuses its
/// receipt, so it cannot open a second turn for one completion revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCompletionWake {
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic whose completion moved.
    pub mini_project_id: MiniProjectId,
    /// The completion revision this wake reports.
    pub completion_revision: AggregateRevision,
    /// Why the seat is being woken.
    pub reason: ExternalName,
    /// The existing seat to wake. Never a seat this wake created.
    pub seat_binding_id: SeatBindingId,
    /// The receipt the wake was recorded under.
    pub receipt: ContentHash,
    /// When the intent was appended.
    pub appended_at: Timestamp,
    /// When the runtime acknowledged the turn, once it has.
    pub acknowledged_at: Option<Timestamp>,
}

/// One durable delivery of a logical Completion wake to an exact hosted TPM
/// native occupancy.
///
/// Replacement preserves [`SeatBindingId`] but changes native identity. The
/// delivery key therefore includes the occupancy generation and native id: a
/// successor receives the newest wake once, while every predecessor attempt
/// remains audit evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCompletionWakeDelivery {
    /// The logical wake being delivered.
    pub wake: StoredCompletionWake,
    /// Persistent-seat occupancy generation used by scoped authority.
    pub occupancy_generation: u64,
    /// Exact native recipient frozen when the delivery was claimed.
    pub native_identity: NativeRuntimeIdentity,
    /// Stable runtime message id, stored as canonical UUID text to keep the
    /// core crate independent of runtime request types.
    pub message_id: String,
    /// Frozen bounded body; every retry must present these exact bytes.
    pub body: BoundedText,
    /// Digest of the frozen body.
    pub body_hash: ContentHash,
    /// Claim instant.
    pub created_at: Timestamp,
    /// Canonical runtime acknowledgement, once observed.
    pub acknowledged_at: Option<Timestamp>,
    /// Canonical timeline epoch of the acknowledgement.
    pub timeline_epoch: Option<u64>,
    /// Canonical timeline sequence of the acknowledgement.
    pub timeline_sequence: Option<u64>,
}

/// One immutable topology snapshot pinned to a MiniProject/epic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniProjectTopologySnapshot {
    /// Owning project.
    pub project_id: ProjectId,
    /// Target MiniProject.
    pub mini_project_id: MiniProjectId,
    /// Exact published revision and hash.
    pub topology: TopologySnapshot,
    /// Pinning instant.
    pub pinned_at: Timestamp,
}

/// A new logical topology node. Native placement is owned by the runtime ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSessionTopologyNode {
    /// Node identity.
    pub id: TopologyNodeId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Optional epic/MiniProject scope.
    pub mini_project_id: Option<MiniProjectId>,
    /// Exact published topology revision/hash.
    pub topology: TopologySnapshot,
    /// Data-defined kind.
    pub kind: TopologyKindKey,
    /// Logical parent.
    pub parent_id: Option<TopologyNodeId>,
    /// The delivery task this node serves, for the task-scoped kinds.
    pub task_id: Option<TaskId>,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// A new logical seat binding. Native identities are added by the placement
/// boundary, not by this repository request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSeatBinding {
    /// Binding identity.
    pub id: SeatBindingId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Hosting logical node.
    pub topology_node_id: TopologyNodeId,
    /// Stable role-slot address.
    pub role_slot_id: RoleSlotId,
    /// Typed catalog role snapshot.
    pub role: CatalogRoleRef,
    /// Optional delivery task reference.
    pub task_id: Option<TaskId>,
    /// Optional delivery TeamRun reference.
    pub team_run_id: Option<TeamRunId>,
    /// The instant this seat must be observed attached by (OP-REQ-039a).
    ///
    /// Supplied by the caller rather than computed here, and then never
    /// recomputed: fixing it at creation is the whole point of the column.
    pub attach_deadline: Timestamp,
    /// The exact owning epic seat, when this seat has one.
    pub parent_seat_binding_id: Option<SeatBindingId>,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// One observed change to a seat's OP-REQ-039 evidence.
///
/// Every field is `None` for "nothing observed about this", so one call records
/// exactly what was seen and silently overwrites nothing else. The separation
/// between `attached_at` and `activity_at` is the requirement, not a
/// convenience: a readback may prove attachment, and only an observed runtime
/// event or turn position may prove activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeatLivenessObservation {
    /// The seat was observed attached at this instant.
    pub attached_at: Option<Timestamp>,
    /// The seat was observed *doing something* at this instant.
    ///
    /// Only an observed runtime event or turn position may fill this in. A
    /// successful inspect belongs in `attached_at`.
    pub activity_at: Option<Timestamp>,
    /// The runtime's self-report, recorded so an escalation can quote it.
    pub runtime_reported: Option<ObservedRunState>,
    /// The seat was deliberately released or reaped at this instant.
    pub released_at: Option<Timestamp>,
    /// The seat was replaced by this one.
    pub replaced_by: Option<SeatBindingId>,
}

/// A native container to bind to one topology node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNativeContainerBinding {
    /// The node that owns the container.
    pub topology_node_id: TopologyNodeId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The runtime-issued binding id this placement was made under.
    pub container_binding_id: ExternalId,
    /// The exact native identity read back from the runtime.
    pub identity: NativeRuntimeIdentity,
    /// What the runtime said the container is.
    pub observed_kind: ObservedContainerKind,
    /// The container's canonical working directory, where it has one.
    pub canonical_cwd: Option<ExternalName>,
    /// When the binding was established or last confirmed.
    pub observed_at: Timestamp,
}

/// Initial persisted adaptive-admission state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAdaptiveAdmissionState {
    /// Owning project.
    pub project_id: ProjectId,
    /// Target MiniProject.
    pub mini_project_id: MiniProjectId,
    /// Initial/current scheduler-decided window.
    pub current_window: u32,
    /// Current clean-observation streak.
    pub clean_observation_streak: u32,
    /// Last applied observation, if any.
    pub last_observation_id: Option<ExternalId>,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// Compare-and-swap update of persisted adaptive-admission state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdaptiveAdmissionAdvance {
    /// Owning project.
    pub project_id: ProjectId,
    /// Target MiniProject.
    pub mini_project_id: MiniProjectId,
    /// Scheduler-decided new window.
    pub current_window: u32,
    /// Scheduler-decided clean-observation streak.
    pub clean_observation_streak: u32,
    /// Last observation already applied.
    pub last_observation_id: Option<ExternalId>,
    /// Expected persisted revision.
    pub expected_revision: AggregateRevision,
    /// Mutation instant.
    pub updated_at: Timestamp,
}

/// A task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// The task.
    pub id: TaskId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning goal, if any.
    pub mini_project_id: Option<MiniProjectId>,
    /// Human title.
    pub title: ExternalName,
    /// The module this task contends for, if any.
    pub module: Option<ModuleKey>,
    /// Lifecycle state.
    pub state: TaskState,
    /// The source lifecycle fact this task was imported with, until the first
    /// native lifecycle transition takes ownership of the state.
    ///
    /// In particular, historical `Completed` distinguishes imported
    /// terminality from a native [`TaskState::Done`] closure certificate.
    pub imported_state: Option<crate::state::ImportedTaskState>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it last changed.
    pub updated_at: Timestamp,
}

/// A new task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    /// The id to use.
    pub id: TaskId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning goal, if any.
    pub mini_project_id: Option<MiniProjectId>,
    /// Human title.
    pub title: ExternalName,
    /// The module this task contends for, if any.
    pub module: Option<ModuleKey>,
    /// Initial lifecycle state.
    pub state: TaskState,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// Which family of approved reference a credential alias is resolved through.
///
/// The set is closed on purpose. A reference is a *kind plus an alias*, never a
/// URI, a path, a shell fragment or a keychain service/account pair, so nothing
/// a profile carries can widen what the resolver is willing to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialReferenceKind {
    /// An approved per-account configuration home the coding client reads its
    /// own credentials out of. Kontor never opens the files inside it.
    ConfigHome,
    /// An approved OS-keychain entry, read through the resolver's narrow backend
    /// port.
    Keychain,
}

impl CredentialReferenceKind {
    /// The stable spelling used in JSON and SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigHome => "config_home",
            Self::Keychain => "keychain",
        }
    }

    /// Parse the stable spelling.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for any other text.
    pub fn parse(text: &str) -> DomainResult<Self> {
        match text {
            "config_home" => Ok(Self::ConfigHome),
            "keychain" => Ok(Self::Keychain),
            _ => Err(DomainError::invalid(
                "CredentialReferenceKind",
                "is not a known value",
            )),
        }
    }
}

/// The whole of what Kontor persists about a profile's credentials.
///
/// Everything that would let a reader *use* the credential — the approved
/// directory, the keychain service and account, the token itself — lives in the
/// resolver's policy and never in this type, so persisting, listing, exporting
/// or logging one of these cannot disclose credential material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialReference {
    /// Which approved family the alias belongs to.
    pub kind: CredentialReferenceKind,
    /// The opaque alias. Meaningful only to a resolver policy that already
    /// approves it.
    pub alias: CredentialAlias,
}

/// A coding-account profile a run can be pinned to.
///
/// Every field here is non-secret by construction. The credential-affecting
/// fields — harness, reference, environment map, routing, capability, provider
/// identity — are immutable for the life of the profile: rotating any of them
/// is a new [`AccountProfileId`], which is what keeps a queued, active or
/// historical run's pin meaningful without storing a profile revision on the
/// run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    /// The profile.
    pub id: AccountProfileId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Human label. Mutable under a compare-and-swap.
    pub label: ExternalName,
    /// The external account id this profile authenticates as, if any.
    pub external_account_id: Option<ExternalId>,
    /// The runtime family this account authenticates against. Immutable.
    pub harness: RuntimeKindKey,
    /// The opaque approved reference the resolver looks the credential up
    /// under. Immutable.
    pub credential_ref: CredentialReference,
    /// Environment variable *names* mapped to opaque reference aliases — never
    /// to values. Immutable.
    pub environment: CanonicalDocument,
    /// Non-secret routing metadata (provider, model preferences, …). Immutable.
    pub routing: CanonicalDocument,
    /// Non-secret declared account capabilities. Immutable.
    pub capability: CanonicalDocument,
    /// A non-secret provider identity hint, if the deployment records one.
    /// Immutable.
    pub provider_identity: Option<ExternalId>,
    /// Whether launches may select this profile. Mutable under a
    /// compare-and-swap.
    pub enabled: bool,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it last changed.
    pub updated_at: Timestamp,
}

/// A new account profile. Persisted at [`AggregateRevision::INITIAL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccountProfile {
    /// The id to use.
    pub id: AccountProfileId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Human label.
    pub label: ExternalName,
    /// The external account id this profile authenticates as, if any.
    pub external_account_id: Option<ExternalId>,
    /// The runtime family this account authenticates against.
    pub harness: RuntimeKindKey,
    /// The opaque approved reference.
    pub credential_ref: CredentialReference,
    /// Environment variable names mapped to opaque reference aliases.
    pub environment: CanonicalDocument,
    /// Non-secret routing metadata.
    pub routing: CanonicalDocument,
    /// Non-secret declared account capabilities.
    pub capability: CanonicalDocument,
    /// A non-secret provider identity hint, if any.
    pub provider_identity: Option<ExternalId>,
    /// Whether launches may select it from the start.
    pub enabled: bool,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// A revision-checked change to the only two mutable fields a profile has.
///
/// There is deliberately no variant of this that can reach a credential-bearing
/// field: rotating one is a new profile, so a queued run's pin cannot change
/// meaning underneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountProfileUpdate {
    /// Owning project.
    pub project_id: ProjectId,
    /// The profile.
    pub id: AccountProfileId,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// The label to store.
    pub label: ExternalName,
    /// Whether launches may select it.
    pub enabled: bool,
    /// When the change happened.
    pub updated_at: Timestamp,
}

/// The resolved work profile a task is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWorkflow {
    /// The workflow.
    pub id: TaskWorkflowId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task it belongs to.
    pub task_id: TaskId,
    /// The frozen profile. Immutable for the life of the workflow.
    pub snapshot: ResolvedWorkProfileSnapshot,
    /// The phase currently in progress.
    pub current_phase: PhaseKey,
    /// Whether this is the task's active workflow.
    pub active: bool,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
}

/// A new task workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTaskWorkflow {
    /// The id to use.
    pub id: TaskWorkflowId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// The profile to freeze.
    pub snapshot: ResolvedWorkProfileSnapshot,
    /// The phase to start in.
    pub current_phase: PhaseKey,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// A revision-checked phase advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseAdvance {
    /// Owning project.
    pub project_id: ProjectId,
    /// The workflow.
    pub workflow_id: TaskWorkflowId,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// The phase to move to.
    pub next_phase: PhaseKey,
    /// When it happened.
    pub advanced_at: Timestamp,
}

/// One append-only gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateEvaluation {
    /// Owning project.
    pub project_id: ProjectId,
    /// The workflow.
    pub workflow_id: TaskWorkflowId,
    /// The gate.
    pub gate: GateKey,
    /// Position in the gate's append-only history, starting at 1.
    pub sequence: u32,
    /// The verdict.
    pub verdict: GateVerdict,
    /// The role that recorded it.
    pub evaluator_role: RoleKey,
    /// The account that recorded it.
    pub evaluator_account: AccountProfileId,
    /// Artifacts cited as evidence.
    pub evidence: Vec<ArtifactKey>,
    /// The agent run the reviewer was acting inside, when there was one.
    ///
    /// Correlation only. It is deliberately *not* the reviewer's identity: a
    /// counter keyed on a run id resets itself every time the same reviewer is
    /// relaunched, which is exactly the reset a rejection counter must not have.
    pub agent_run_id: Option<AgentRunId>,
    /// The session record a recovery verdict was transcribed from, when the
    /// evaluation was recorded on behalf of a closed evaluator seat.
    pub session_evidence: Option<SessionVerdictEvidence>,
    /// The stable authenticated principal that recorded this verdict.
    ///
    /// `None` for a row written before the principal was recorded. Such a row is
    /// attributable to nobody, so it neither advances nor resets any reviewer's
    /// rejection stream rather than being folded into a plausible one.
    pub reviewer_principal: Option<ExternalId>,
    /// The guardrail evaluation this verdict was recorded under, if any.
    pub policy_evaluation_id: Option<GuardrailEvaluationId>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// The session record a gate verdict was transcribed from on the recovery path.
///
/// The recovery path records a verdict on behalf of an evaluator seat whose
/// runtime is closed or unreachable. The control plane does not hold session
/// transcripts, so `digest` is the operator's binding attestation of the verdict
/// content the named session record rendered; what is mechanically validated is
/// that the citation names the evaluator seat's own session record and that the
/// seat cannot act any more. Both halves are persisted on the evaluation row so
/// the citation is durable evidence, never a free-text aside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionVerdictEvidence {
    /// The evaluator's own agent run whose session record holds the verdict.
    pub agent_run_id: AgentRunId,
    /// A digest of the verdict content as that session record rendered it.
    pub digest: ContentHash,
}

/// A request to record a gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGateEvaluation {
    /// Owning project.
    pub project_id: ProjectId,
    /// The workflow.
    pub workflow_id: TaskWorkflowId,
    /// The gate.
    pub gate: GateKey,
    /// The verdict.
    pub verdict: GateVerdict,
    /// The role recording it. Checked against the pinned profile's authority.
    pub evaluator_role: RoleKey,
    /// The account recording it.
    pub evaluator_account: AccountProfileId,
    /// Artifacts cited as evidence.
    pub evidence: Vec<ArtifactKey>,
    /// The agent run the reviewer was acting inside, when there was one.
    pub agent_run_id: Option<AgentRunId>,
    /// The session record this verdict was transcribed from, on the recovery
    /// path. `Some` only for a verdict recorded on behalf of a closed evaluator
    /// seat, and then `agent_run_id` names the same run.
    pub session_evidence: Option<SessionVerdictEvidence>,
    /// The stable authenticated principal recording it.
    ///
    /// Required for the verdict to count towards — or reset — a reviewer's
    /// rejection stream. Omitting it records the verdict and attributes it to
    /// nobody; it never silently falls back to the run or the display name.
    pub reviewer_principal: Option<ExternalId>,
    /// The guardrail evaluation this verdict was recorded under, if any.
    pub policy_evaluation_id: Option<GuardrailEvaluationId>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// A revision-checked task transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTransitionRequest {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// The state to move to.
    pub to: TaskState,
    /// The command receipt that authorizes a resume.
    pub resume_receipt: Option<CommandReceiptId>,
    /// Whether this transition is an explicit reopen of a terminal task.
    ///
    /// Kept apart from `resume_receipt` because the two are different decisions
    /// carried by the same kind of receipt: a resume lets a held task continue,
    /// and a reopen contradicts a conclusion the Realm already recorded. A store
    /// that inferred one from the other would let any resume walk a completed task
    /// back open.
    pub reopen: bool,
    /// How the task's run closed, for a failure transition.
    pub run_outcome: Option<TerminalOutcome>,
    /// Artifacts produced, for a completion transition.
    pub produced_artifacts: BTreeSet<ArtifactKey>,
    /// Phases recorded complete, for a completion transition.
    pub completed_phases: BTreeSet<PhaseKey>,
    /// What the task presents about its team, for a terminal transition.
    ///
    /// Required for *every* terminal target, not only completion: a task that
    /// fails or is cancelled while a role slot still holds a live native session
    /// is exactly as wrong as one that succeeds that way.
    pub team_closure: TaskTeamClosure,
    /// When it happened.
    pub occurred_at: Timestamp,
}

/// The binding between an agent run and a native runtime session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBinding {
    /// The binding.
    pub id: RuntimeBindingId,
    /// The run it binds.
    pub agent_run_id: AgentRunId,
    /// The native session.
    pub identity: NativeRuntimeIdentity,
    /// When it was bound.
    pub bound_at: Timestamp,
}

/// One run of a team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRun {
    /// The run.
    pub id: TeamRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task it serves.
    pub task_id: TaskId,
    /// The frozen team definition.
    pub snapshot: TeamRunSnapshot,
    /// Lifecycle.
    pub lifecycle: RunLifecycle,
    /// Closure evidence, once closed.
    pub terminal: Option<TeamTerminalEvidence>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it closed.
    pub closed_at: Option<Timestamp>,
}

/// A new team run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTeamRun {
    /// The id to use.
    pub id: TeamRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task it serves.
    pub task_id: TaskId,
    /// The team definition to freeze.
    pub snapshot: TeamRunSnapshot,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// One run of a single agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    /// The run.
    pub id: AgentRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The team run it belongs to.
    pub team_run_id: TeamRunId,
    /// The run this one succeeds, for recovery and resume.
    pub parent_agent_run_id: Option<AgentRunId>,
    /// The role it acts as.
    pub role: RoleKey,
    /// The pinned coding account, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// The native binding, once launched.
    pub binding: Option<RuntimeBinding>,
    /// The orthogonal state projection.
    pub projection: RunProjection,
    /// Closure evidence, once closed.
    pub terminal: Option<TerminalEvidence>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it closed.
    pub closed_at: Option<Timestamp>,
}

impl AgentRun {
    /// True when this row is an operator-abandoned launch that never bound a native.
    ///
    /// Those rows stay as evidence, but they are not a reusable replacement
    /// target and they do not occupy the role slot's successor chain.
    #[must_use]
    pub fn is_operator_abandoned_unbound(&self) -> bool {
        self.binding.is_none()
            && matches!(
                self.terminal.as_ref(),
                Some(evidence)
                    if evidence.outcome == TerminalOutcome::Abandoned
                        && matches!(
                            evidence.source,
                            TerminalEvidenceSource::OperatorAbandon { .. }
                        )
            )
    }
}

/// A new agent run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentRun {
    /// The id to use.
    pub id: AgentRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The team run it belongs to.
    pub team_run_id: TeamRunId,
    /// The run this one succeeds, for recovery and resume.
    pub parent_agent_run_id: Option<AgentRunId>,
    /// The role it acts as.
    pub role: RoleKey,
    /// The pinned coding account, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// The native binding, if it is already known.
    pub binding: Option<RuntimeBinding>,
    /// Creation instant.
    pub created_at: Timestamp,
}

/// A raw runtime event, appended before any state is reduced from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRuntimeEvent {
    /// Owning project.
    pub project_id: ProjectId,
    /// The run it concerns.
    pub agent_run_id: AgentRunId,
    /// The native session that emitted it.
    pub identity: NativeRuntimeIdentity,
    /// The native event id, when the runtime provides one.
    pub native_event_id: Option<ExternalId>,
    /// The runtime's own monotonic ordering for this event.
    ///
    /// A state-reducing observation must carry one: without it there is no way
    /// to tell a replay from progress, and an adapter that cannot provide one
    /// may append raw evidence but must not overwrite observed state.
    pub native_sequence: u64,
    /// The canonical payload.
    pub payload: CanonicalDocument,
    /// When the runtime emitted it.
    pub observed_at: Timestamp,
}

/// A stored runtime event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvent {
    /// Monotonic, never-reused cursor.
    pub cursor: EventCursor,
    /// Owning project.
    pub project_id: ProjectId,
    /// The run it concerns.
    pub agent_run_id: AgentRunId,
    /// The native session that emitted it.
    pub identity: NativeRuntimeIdentity,
    /// The native event id, when the runtime provides one.
    pub native_event_id: Option<ExternalId>,
    /// The runtime's own ordering for this event.
    pub native_sequence: u64,
    /// The canonical payload.
    pub payload: CanonicalDocument,
    /// When the runtime emitted it.
    pub observed_at: Timestamp,
    /// When Kontor stored it.
    pub recorded_at: Timestamp,
}

/// A request to reduce one runtime event into observed and derived state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewObservation {
    /// The raw event to append first.
    pub event: NewRuntimeEvent,
    /// What the runtime reported.
    pub observed: crate::state::ObservedRunState,
    /// The transport result of the contact that produced it.
    pub contact: crate::state::RuntimeContact,
    /// How old the newest confirmation is.
    pub freshness: crate::state::Freshness,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
}

/// A request to close a run with evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunClosure {
    /// Owning project.
    pub project_id: ProjectId,
    /// The run to close.
    pub agent_run_id: AgentRunId,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// The closure evidence.
    pub evidence: TerminalEvidence,
}

/// A source event together with the intake decision it produced.
///
/// The two halves are *not* written in one transaction. Ingestion commits the
/// canonical identity first ([`IntakeRepository::ingest_source_event`]) and the
/// decision second ([`IntakeRepository::record_intake_decision`]), so a crash
/// between them leaves a stored, unevaluated event rather than a lost one. This
/// request is the composition of those two steps, and it validates the decision
/// before either of them runs: an inconsistent receipt persists no event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSourceEvent {
    /// Owning project.
    pub project_id: ProjectId,
    /// The canonical event.
    pub event: CanonicalSourceEvent,
    /// The decision, recorded once the event itself is durable.
    pub receipt: IntakeReceipt,
}

/// What committing the canonical identity of a source event produced.
///
/// The three answers are the three states intake can resume from, and they are
/// deliberately distinct: "I stored it, evaluate it", "someone stored it and
/// nobody has evaluated it yet, evaluate it" and "this was already decided,
/// here is that decision". Collapsing the middle one into either neighbour is
/// how a crash between the two commits either loses evidence or creates a
/// second work graph for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceEventIngest {
    /// The event is new and now durable. Nothing has evaluated it.
    Recorded(Box<CanonicalSourceEvent>),
    /// The event was already stored and still carries no decision — a resumed
    /// intake rather than a duplicate.
    Unevaluated(Box<CanonicalSourceEvent>),
    /// The event repeats one that has already been decided; this is that
    /// original decision, and no second one is written.
    Decided(Box<IntakeReceipt>),
}

/// One deterministic decision about an already-durable source event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIntakeDecision {
    /// Owning project.
    pub project_id: ProjectId,
    /// The stored event being decided. It is never mutated.
    pub source_event_id: SourceEventId,
    /// The digest that event must still have.
    pub source_event_hash: ContentHash,
    /// The decision.
    pub receipt: IntakeReceipt,
}

/// What recording a source event produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeOutcome {
    /// The event was new; this is its decision.
    Recorded(Box<IntakeReceipt>),
    /// The event repeated one already recorded; this is the *original*
    /// decision, and no second work graph was created.
    Duplicate(Box<IntakeReceipt>),
}

closed_enum! {
    /// How a proposed intake receipt reached its terminal state.
    IntakeDecisionOutcome, "IntakeDecisionOutcome" {
        /// An authorized operator approved it, and work was created.
        Approved => "approved",
        /// An authorized operator rejected it. Terminal, and creates no work.
        Rejected => "rejected",
        /// The trigger's own bounded auto-arm policy armed it.
        AutoArmed => "auto_armed",
    }
}

/// Under what authority a proposal became terminal.
///
/// Every variant is receipt-backed: a decision that names no command receipt is
/// an assertion, and an assertion is not evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeAuthority {
    /// An authorized operator approved the proposal.
    Approval {
        /// Who approved.
        authority: AccountProfileId,
        /// The `ApproveIntake` receipt that recorded it.
        command_receipt: CommandReceiptId,
    },
    /// An authorized operator rejected the proposal, terminally.
    Rejection {
        /// Who rejected.
        authority: AccountProfileId,
        /// The `RejectIntake` receipt that recorded it.
        command_receipt: CommandReceiptId,
        /// Why. Recorded so a rejection can be read years later.
        reason: ExternalName,
    },
    /// The trigger's pinned bounded auto-arm policy armed the work itself.
    BoundedAutoArm {
        /// The account whose capability was exercised.
        caller: AccountProfileId,
        /// The receipt that recorded the arming.
        command_receipt: CommandReceiptId,
    },
}

impl IntakeAuthority {
    /// Which terminal state this authority produces.
    #[must_use]
    pub const fn outcome(&self) -> IntakeDecisionOutcome {
        match self {
            Self::Approval { .. } => IntakeDecisionOutcome::Approved,
            Self::Rejection { .. } => IntakeDecisionOutcome::Rejected,
            Self::BoundedAutoArm { .. } => IntakeDecisionOutcome::AutoArmed,
        }
    }

    /// The account that acted.
    #[must_use]
    pub const fn actor(&self) -> AccountProfileId {
        match self {
            Self::Approval { authority, .. }
            | Self::Rejection { authority, .. }
            | Self::BoundedAutoArm {
                caller: authority, ..
            } => *authority,
        }
    }

    /// The command receipt backing it.
    #[must_use]
    pub const fn command_receipt(&self) -> CommandReceiptId {
        match self {
            Self::Approval {
                command_receipt, ..
            }
            | Self::Rejection {
                command_receipt, ..
            }
            | Self::BoundedAutoArm {
                command_receipt, ..
            } => *command_receipt,
        }
    }
}

/// The work one intake decision creates.
///
/// Intake creates work and lineage. It never launches a runtime: the created
/// tasks are ordinary candidates that go through the scheduler's own admission
/// like every other task in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeWorkPlan {
    /// The goal to create, when the graph has one.
    pub mini_project: Option<NewMiniProject>,
    /// The tasks to create. At least one.
    pub tasks: Vec<NewTask>,
}

/// A terminal decision about one proposed intake receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIntakeDecisionRecord {
    /// The decision's own id.
    pub id: IntakeDecisionId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The proposal being decided. It is never mutated.
    pub receipt_id: IntakeReceiptId,
    /// Under what authority.
    pub authority: IntakeAuthority,
    /// The work to create, in the same transaction. Required for an approval or
    /// a bounded auto-arm, and refused on a rejection.
    pub work: Option<IntakeWorkPlan>,
    /// When it was decided.
    pub decided_at: Timestamp,
}

impl NewIntakeDecisionRecord {
    /// Validate the decision's own shape, before any row is read.
    ///
    /// # Errors
    /// Refuses a rejection that carries work, an approval or auto-arm that
    /// carries none, an empty work plan and a cross-project graph.
    pub fn validate(&self) -> DomainResult<()> {
        match (&self.authority, &self.work) {
            (IntakeAuthority::Rejection { .. }, Some(_)) => {
                return Err(DomainError::invalid(
                    "IntakeDecision",
                    "a rejection is terminal and creates no work",
                ));
            }
            (IntakeAuthority::Approval { .. } | IntakeAuthority::BoundedAutoArm { .. }, None) => {
                return Err(DomainError::MissingEvidence {
                    subject: "intake decision",
                    rule: "an approval or a bounded auto-arm arms the work it names",
                });
            }
            _ => {}
        }
        let Some(work) = &self.work else {
            return Ok(());
        };
        if work.tasks.is_empty() {
            return Err(DomainError::invalid(
                "IntakeWorkPlan",
                "must create at least one task",
            ));
        }
        if work
            .mini_project
            .as_ref()
            .is_some_and(|goal| goal.project_id != self.project_id)
            || work
                .tasks
                .iter()
                .any(|task| task.project_id != self.project_id)
        {
            return Err(DomainError::invalid(
                "IntakeWorkPlan",
                "creates work in another project",
            ));
        }
        // A task may only be filed under the goal this very decision creates:
        // attaching created work to a pre-existing goal would let one intake
        // decision reach into a graph it did not author.
        let goal = work.mini_project.as_ref().map(|goal| goal.id);
        if work.tasks.iter().any(|task| task.mini_project_id != goal) {
            return Err(DomainError::invalid(
                "IntakeWorkPlan",
                "every created task belongs to the goal the decision creates",
            ));
        }
        Ok(())
    }
}

/// One task an intake decision created, and everything that authorized it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeCreatedWork {
    /// Owning project.
    pub project_id: ProjectId,
    /// The receipt the work came from.
    pub receipt_id: IntakeReceiptId,
    /// The decision that created it.
    pub decision_id: IntakeDecisionId,
    /// The goal it belongs to, if any.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task.
    pub task_id: TaskId,
    /// The event that caused it.
    pub source_event_id: SourceEventId,
    /// That event's digest at the moment of the decision.
    pub source_event_hash: ContentHash,
    /// The trigger that decided.
    pub trigger: TriggerKey,
    /// The pinned trigger revision.
    pub trigger_version: SpecVersion,
    /// Whether an operator approved it or the trigger armed it.
    pub authority: IntakeDecisionOutcome,
    /// The execution authorization a bounded auto-arm acted under.
    pub execution_authorization: Option<ExecutionAuthorizationId>,
    /// When the work was created.
    pub created_at: Timestamp,
}

/// One recorded terminal decision, with the work it created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeDecisionRecord {
    /// The decision's id.
    pub id: IntakeDecisionId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The proposal it decided.
    pub receipt_id: IntakeReceiptId,
    /// What it decided.
    pub outcome: IntakeDecisionOutcome,
    /// Who acted.
    pub actor: AccountProfileId,
    /// The command receipt backing it.
    pub command_receipt: CommandReceiptId,
    /// Why, on a rejection.
    pub reason: Option<ExternalName>,
    /// The capability a bounded auto-arm exercised.
    pub capability: Option<ExecutionCapability>,
    /// The lineage of every task it created, task id ascending.
    pub created_work: Vec<IntakeCreatedWork>,
    /// When it was decided.
    pub decided_at: Timestamp,
}

/// A persona scenario being frozen onto a task.
///
/// The workflow is named explicitly so the authority check has a pinned profile
/// to resolve the gate against; a scenario alone cannot assert who may evaluate
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTaskPersonaSnapshot {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task the scenario attaches to.
    pub task_id: TaskId,
    /// The task's active workflow, whose pinned profile carries the gate.
    pub workflow_id: TaskWorkflowId,
    /// The scenario revision being frozen.
    pub scenario_id: PersonaScenarioId,
    /// That revision's version.
    pub version: SpecVersion,
    /// When it was frozen.
    pub created_at: Timestamp,
}

/// A re-evaluation of an already-stored source event under a newer trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIntakeReevaluation {
    /// Owning project.
    pub project_id: ProjectId,
    /// The event being re-evaluated. It is never mutated.
    pub source_event_id: SourceEventId,
    /// The digest that event must still have.
    pub source_event_hash: ContentHash,
    /// The new, deterministic decision.
    pub receipt: IntakeReceipt,
}

/// What a re-evaluation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReevaluationOutcome {
    /// A successor receipt, linked to the one it supersedes.
    Superseded(Box<IntakeReceipt>),
    /// The same trigger revision had already decided; this is that decision.
    AlreadyDecided(Box<IntakeReceipt>),
}

/// A revision-checked, non-terminal team-run advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamRunAdvance {
    /// Owning project.
    pub project_id: ProjectId,
    /// The team run.
    pub team_run_id: TeamRunId,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// The lifecycle value to move to. Never a terminal one.
    pub to: RunLifecycle,
    /// When it happened.
    pub occurred_at: Timestamp,
}

/// A revision-checked team-run closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRunClosure {
    /// Owning project.
    pub project_id: ProjectId,
    /// The team run to close.
    pub team_run_id: TeamRunId,
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// The closure evidence, bound to this team.
    pub evidence: TeamTerminalEvidence,
}

/// The operator receipt that authorizes abandoning one run.
///
/// Deliberately *not* a [`NewCommandIntent`]. An intent moves a run's desired
/// state under compare-and-swap, which bumps the very revision the closure
/// receipt has to stay bound to — the abandon evidence is verified against the
/// revision present when the run closes, so an intent would invalidate its own
/// receipt. This records the decision and nothing else; the closure is a
/// separate act that cites it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAbandonReceipt {
    /// Owning project.
    pub project_id: ProjectId,
    /// The receipt id to mint.
    pub receipt_id: CommandReceiptId,
    /// The caller's stable key.
    pub idempotency_key: IdempotencyKey,
    /// The aggregate being abandoned: a run, or the team run whose every run
    /// has ended.
    pub target: AggregateRef,
    /// The revision the operator decided against, which is the revision the
    /// closure must be made at.
    pub target_revision: AggregateRevision,
    /// The canonical decision document. Its digest is the closure evidence.
    pub intent: CanonicalDocument,
    /// When the decision was recorded.
    pub recorded_at: Timestamp,
}

/// A synchronous control-plane command whose effect is applied by the
/// application service rather than handed to an external dispatcher.
///
/// It deliberately has no payload, `not_before`, or desired-state write. Those
/// fields belong to an outbox dispatch. A local command first records this
/// durable identity, then moves to `confirmed` only after its application
/// operation has returned successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLocalCommand {
    /// Owning project.
    pub project_id: ProjectId,
    /// The receipt id to use.
    pub receipt_id: CommandReceiptId,
    /// The caller's idempotency key.
    pub idempotency_key: IdempotencyKey,
    /// What is being applied locally.
    pub kind: CommandKind,
    /// Which aggregate it targets.
    pub target: AggregateRef,
    /// The revision the operation was computed against.
    pub target_revision: AggregateRevision,
    /// The canonical operation identity.
    pub intent: CanonicalDocument,
    /// When the operation was recorded.
    pub created_at: Timestamp,
}

/// A new command intent, written atomically with desired state, the outbox entry
/// and the intent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommandIntent {
    /// Owning project.
    pub project_id: ProjectId,
    /// The receipt id to use.
    pub receipt_id: CommandReceiptId,
    /// The caller's idempotency key.
    pub idempotency_key: IdempotencyKey,
    /// What is being asked for.
    pub kind: CommandKind,
    /// Which aggregate it targets.
    pub target: AggregateRef,
    /// The revision the intent was computed against.
    pub target_revision: AggregateRevision,
    /// The canonical intent.
    pub intent: CanonicalDocument,
    /// The canonical dispatch payload.
    pub payload: CanonicalDocument,
    /// The desired run state this intent sets, when it targets a run.
    pub desired: Option<DesiredRunState>,
    /// The earliest instant the command may be dispatched.
    pub not_before: Timestamp,
    /// When the intent was recorded.
    pub created_at: Timestamp,
}

/// A request to move a command receipt forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptAdvance {
    /// Owning project.
    pub project_id: ProjectId,
    /// The receipt.
    pub receipt_id: CommandReceiptId,
    /// The state to move to.
    pub to: CommandReceiptState,
    /// The dispatcher's correlation token, recorded on dispatch.
    pub correlation: Option<ExternalId>,
    /// The native identity the command addressed or created.
    pub native_identity: Option<NativeRuntimeIdentity>,
    /// Reference to the result or failure evidence.
    pub result_ref: Option<ExternalId>,
    /// Proof of no effect, required to re-dispatch after an unknown result.
    pub no_effect: Option<NoEffectEvidence>,
    /// When it happened.
    pub occurred_at: Timestamp,
}

/// A new link between a task and an external ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTicketLink {
    /// The link id to use.
    pub id: TicketLinkId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// The connector.
    pub connector: ConnectorKey,
    /// The external issue key.
    pub external_issue_key: ExternalId,
    /// When it was created.
    pub created_at: Timestamp,
}

/// A link between a task and an external ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketLink {
    /// The link.
    pub id: TicketLinkId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// The connector.
    pub connector: ConnectorKey,
    /// The external issue key.
    pub external_issue_key: ExternalId,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
}

/// Selector for one pinned connector specification revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorSpecSelector {
    /// Owning project.
    pub project_id: ProjectId,
    /// The connector.
    pub connector: ConnectorKey,
    /// The external project.
    pub project: ExternalProjectKey,
    /// The external issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// The pinned revision.
    pub version: SpecVersion,
}

// ---------------------------------------------------------------------------
// Cross-boundary inspection
// ---------------------------------------------------------------------------

/// Which cursor space a recorded discontinuity belongs to.
///
/// The two are never merged. A control gap says a *control-plane fact* never
/// arrived; a content gap says some transcript must be read again from the
/// runtime. Only the first is evidence about the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryGapKind {
    /// A hole in the runtime's own control sequence.
    Control,
    /// A hole in the runtime's session-content epoch or sequence.
    Content,
}

/// One recorded discontinuity, as the marker a reader is owed.
///
/// It carries positions and instants only: a gap is the statement that something
/// is missing, never a copy of what it said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryGapMarker {
    /// Which cursor space the hole is in.
    pub kind: HistoryGapKind,
    /// The runtime's content epoch, for a content gap.
    pub content_epoch: Option<u64>,
    /// The sequence that was expected next.
    pub expected_sequence: u64,
    /// The sequence that actually arrived.
    pub received_sequence: u64,
    /// The control-plane position the hole was noticed at.
    pub detected_cursor: EventCursor,
    /// When it was noticed.
    pub detected_at: Timestamp,
}

/// One page of this Realm's durable control-plane log, with the window it was
/// read against.
///
/// The window is read in the *same* transaction as the page, so a caller can
/// tell "you are caught up" from "the position you asked for is no longer
/// retained" without a second, later read that could disagree with the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealmEventPage {
    /// The events, ascending, all strictly after the requested position.
    pub events: Vec<EventEnvelope<RuntimeEvent>>,
    /// The oldest position still retained, or the reserved origin when the log
    /// is empty.
    pub oldest_retained: RealmCursor,
    /// The newest allocated position, or the reserved origin when the log is
    /// empty.
    pub newest: RealmCursor,
}

/// Everything a cross-boundary reader is told about one agent run.
///
/// The parts are read together, so the projection, the binding, the pinned team
/// revision and the recorded gaps all describe one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInspection {
    /// The project the run resolved into. A run is addressed by its own id and
    /// answers with the scope it belongs to, so nothing downstream has to guess
    /// which project's rows it may read.
    pub project_id: ProjectId,
    /// The run, with its binding, projection and revision.
    pub run: AgentRun,
    /// The team run's pinned template revision, when the team is readable.
    pub team_template: Option<(TeamTemplateId, SpecVersion)>,
    /// Every recorded discontinuity for this run, oldest first.
    pub gaps: Vec<HistoryGapMarker>,
    /// The immutable context window this seat was launched under, once frozen.
    pub context_policy: Option<crate::spec::ContextPolicySnapshot>,
    /// The most recent recorded attempt to compact this seat.
    pub latest_compaction: Option<crate::compaction::CompactionReceipt>,
}

/// Everything a cross-boundary reader is told about one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInspection {
    /// The task, with its state and revision.
    pub task: Task,
    /// The active workflow and the profile revision it pinned.
    pub workflow: Option<TaskWorkflow>,
    /// The gate states reduced from the workflow's append-only evaluations.
    pub gates: BTreeMap<GateKey, GateState>,
    /// The persona scenario frozen onto the task, when there is one.
    pub persona: Option<PersonaScenarioSnapshot>,
}

// ---------------------------------------------------------------------------
// Dependency graph
// ---------------------------------------------------------------------------

/// Validate a task dependency graph inside the write transaction.
///
/// `edges` maps a task to the tasks it depends on. Self dependencies, duplicate
/// edges and cycles of any length are rejected; SQLite can enforce the first two
/// but not the third, which is why this runs in the same transaction as the
/// write.
///
/// # Errors
/// Returns [`DomainError::Invalid`] naming the rule that failed.
pub fn validate_dependency_graph(edges: &BTreeMap<TaskId, BTreeSet<TaskId>>) -> DomainResult<()> {
    for (task, dependencies) in edges {
        if dependencies.contains(task) {
            return Err(DomainError::invalid(
                "task dependency",
                "a task must not depend on itself",
            ));
        }
    }

    // Kahn's algorithm: a graph with a cycle never drains.
    let mut indegree: BTreeMap<&TaskId, usize> = BTreeMap::new();
    for (task, dependencies) in edges {
        indegree.entry(task).or_insert(0);
        for dependency in dependencies {
            indegree.entry(dependency).or_insert(0);
        }
        *indegree.entry(task).or_insert(0) += dependencies.len();
    }
    let mut ready: Vec<&TaskId> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(task, _)| *task)
        .collect();
    let mut resolved = 0usize;
    while let Some(task) = ready.pop() {
        resolved += 1;
        for (dependent, dependencies) in edges {
            if dependencies.contains(task)
                && let Some(degree) = indegree.get_mut(dependent)
            {
                *degree -= 1;
                if *degree == 0 {
                    ready.push(dependent);
                }
            }
        }
    }
    if resolved == indegree.len() {
        Ok(())
    } else {
        Err(DomainError::invalid(
            "task dependency",
            "the dependency graph contains a cycle",
        ))
    }
}

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

/// Projects, goals, tasks and the dependency graph between them.
pub trait ProjectRepository {
    /// Create a project.
    ///
    /// # Errors
    /// Refuses a duplicate id or a duplicate root path.
    fn create_project(&self, request: &NewProject) -> RepositoryResult<Project>;

    /// Read a project.
    ///
    /// # Errors
    /// Backend failures only; a missing project is `Ok(None)`.
    fn get_project(&self, id: ProjectId) -> RepositoryResult<Option<Project>>;

    /// Create a goal.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project parent.
    fn create_mini_project(&self, request: &NewMiniProject) -> RepositoryResult<MiniProject>;

    /// Create a task.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project parent.
    fn create_task(&self, request: &NewTask) -> RepositoryResult<Task>;

    /// Read a task inside a project.
    ///
    /// # Errors
    /// Backend failures only; a task from another project is `Ok(None)`.
    fn get_task(&self, project_id: ProjectId, id: TaskId) -> RepositoryResult<Option<Task>>;

    /// List a project's tasks.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_tasks(&self, project_id: ProjectId) -> RepositoryResult<Vec<Task>>;

    /// Replace one task's dependencies, validating the whole graph atomically.
    ///
    /// # Errors
    /// Refuses self dependencies, duplicates, cross-project edges and cycles;
    /// on refusal no edge is written.
    fn set_task_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        depends_on: &[TaskId],
    ) -> RepositoryResult<()>;

    /// Create a coding-account profile.
    ///
    /// # Errors
    /// Refuses a duplicate id or a cross-project reference.
    fn create_account_profile(
        &self,
        request: &NewAccountProfile,
    ) -> RepositoryResult<AccountProfile>;

    /// Read a coding-account profile inside a project.
    ///
    /// # Errors
    /// Backend failures only; a profile from another project is `Ok(None)`.
    fn get_account_profile(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
    ) -> RepositoryResult<Option<AccountProfile>>;

    /// List a project's coding-account profiles.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_account_profiles(&self, project_id: ProjectId)
    -> RepositoryResult<Vec<AccountProfile>>;

    /// Change a profile's label and enabled state under a compare-and-swap.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] for an unknown profile in this
    /// project and [`DomainError::RevisionConflict`] when the revision moved.
    /// On refusal no column is written.
    fn update_account_profile(
        &self,
        request: &AccountProfileUpdate,
    ) -> RepositoryResult<AccountProfile>;

    /// Physically delete an *unreferenced* profile under a compare-and-swap.
    ///
    /// A profile any run, gate evaluation or override still names is retained:
    /// the schema's `ON DELETE RESTRICT` references refuse the delete, and the
    /// only supported way to retire such a profile is to disable it. Deleting
    /// it would strand the audit trail of every run pinned to it.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] for an unknown profile,
    /// [`DomainError::RevisionConflict`] for a stale revision and
    /// [`RepositoryError::Conflict`] when the profile is still referenced.
    fn delete_account_profile(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
        expected_revision: AggregateRevision,
    ) -> RepositoryResult<()>;
}

/// One raw capacity reading, exactly as a collector observed it.
///
/// The `reading` is opaque here on purpose. `kontor-accounts` owns what a
/// reading means; this layer's whole job is to persist it unaltered and hand it
/// back, and a store that could parse it would eventually be tempted to derive
/// from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityObservation {
    /// The observation.
    pub id: CapacityObservationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The account it concerns.
    pub account_profile_id: AccountProfileId,
    /// When the collector read it.
    pub observed_at: Timestamp,
    /// The collector's evidence, verbatim.
    pub reading: CanonicalDocument,
    /// What the account layer derived from it, in the same transaction.
    pub available: bool,
    /// Whether the reading indicated the provider pushing back.
    pub pressure: bool,
    /// When any cooldown this reading started lifts.
    pub cooling_until: Option<Timestamp>,
}

/// One raw capacity reading to persist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCapacityObservation {
    /// The observation's identity, minted by the collector.
    pub id: CapacityObservationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The account it concerns.
    pub account_profile_id: AccountProfileId,
    /// When the collector read it.
    pub observed_at: Timestamp,
    /// The collector's evidence, verbatim.
    pub reading: CanonicalDocument,
    /// Derived availability.
    pub available: bool,
    /// Derived pressure.
    pub pressure: bool,
    /// Derived cooldown expiry, if the reading started one.
    pub cooling_until: Option<Timestamp>,
}

/// An operator's standing judgement about one account's availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityOverride {
    /// Owning project.
    pub project_id: ProjectId,
    /// The account it concerns.
    pub account_profile_id: AccountProfileId,
    /// What the operator asserts.
    pub available: bool,
    /// Why. Recorded, never interpreted.
    pub reason: ExternalName,
    /// When it lapses on its own.
    pub expires_at: Option<Timestamp>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Last mutation instant.
    pub updated_at: Timestamp,
}

impl AvailabilityOverride {
    /// Whether this judgement still stands at `now`.
    #[must_use]
    pub fn is_standing(&self, now: Timestamp) -> bool {
        self.expires_at.is_none_or(|expiry| now < expiry)
    }
}

/// An operator judgement to record, under the account's expected revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAvailabilityOverride {
    /// Owning project.
    pub project_id: ProjectId,
    /// The account it concerns.
    pub account_profile_id: AccountProfileId,
    /// What the operator asserts.
    pub available: bool,
    /// Why.
    pub reason: ExternalName,
    /// When it lapses.
    pub expires_at: Option<Timestamp>,
    /// The override revision the caller believes is current; `1` for the first.
    pub expected_revision: AggregateRevision,
    /// Mutation instant.
    pub updated_at: Timestamp,
}

/// One account's quota state for one provider.
///
/// Distinct from [`AvailabilityOverride`] and from [`CapacityObservation`], both
/// of which are keyed on the account alone. Under Paseo a single account profile
/// serves every provider, so "Codex is exhausted and Claude is fine" is not a
/// fact either of those can hold — and it is the fact a rung advance turns on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQuotaState {
    /// Owning project.
    pub project_id: ProjectId,
    /// The account the state is about.
    pub account_profile_id: AccountProfileId,
    /// The provider, spelled as the model catalog spells it.
    pub provider: String,
    /// What the quota is doing.
    pub state: crate::spec::ProviderQuotaKind,
    /// When an exhausted allowance returns. `None` for every other state, which
    /// the database enforces rather than trusting call sites.
    pub resets_at: Option<Timestamp>,
    /// Every concurrent window observed on this pair.
    ///
    /// A set and not a field, because a provider holds several allowances at
    /// once and one `resets_at` above cannot describe two of them. Ordered by
    /// kind so a stored row and a re-read of it are byte-identical.
    pub windows: Vec<crate::quota::QuotaWindow>,
    /// The depleting balance and its floor, where this provider has one.
    ///
    /// `None` is the ordinary case: a subscription provider has windows and no
    /// balance, and inventing a zero balance for it would refuse every launch.
    pub credit: Option<crate::quota::CreditBalance>,
    /// Digest of the evidence. Never the evidence: a provider's own message
    /// carries account hints and URLs, and nothing here needs to keep them.
    pub evidence_hash: ContentHash,
    /// Who concluded it.
    pub source: crate::spec::ProviderQuotaSource,
    /// When it was concluded.
    pub observed_at: Timestamp,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Last mutation instant.
    pub updated_at: Timestamp,
}

impl ProviderQuotaState {
    /// Whether this state still holds back a launch at `now`.
    ///
    /// An exhausted allowance whose reset instant has passed no longer blocks —
    /// the row is stale rather than wrong, and waiting for a collector to
    /// rewrite it would keep work parked past the moment it could have run.
    /// `Drained` never expires on its own, which is the whole difference.
    #[must_use]
    pub fn blocks_at(&self, now: Timestamp) -> bool {
        match self.state {
            crate::spec::ProviderQuotaKind::Available => false,
            crate::spec::ProviderQuotaKind::Exhausted => {
                self.resets_at.is_some_and(|resets_at| now < resets_at)
            }
            crate::spec::ProviderQuotaKind::Drained | crate::spec::ProviderQuotaKind::Unknown => {
                true
            }
            // A provider that cannot report headroom is used until it refuses.
            // Blocking here would be failing closed on a number this provider
            // was never going to produce, which retires it permanently.
            crate::spec::ProviderQuotaKind::CannotReport => false,
        }
    }

    /// Every concurrent window this account holds on this provider, and the
    /// credit balance beside them.
    ///
    /// Both are read from the same row because they are two dimensions of one
    /// account's standing on one provider — never two candidate answers to the
    /// same question. [`Self::headroom`] is where they are judged, and it judges
    /// each on its own dimension.
    #[must_use]
    pub fn windows(&self) -> &[crate::quota::QuotaWindow] {
        &self.windows
    }

    /// Whether this `(account, provider)` pair admits a **new** seat at `now`.
    ///
    /// Three independent gates, in order, none of which can excuse another:
    /// the recorded state, then every window against its own threshold, then any
    /// credit against its own reserve. Window headroom never satisfies a credit
    /// floor and a credit floor never excuses a spent window.
    #[must_use]
    pub fn headroom(
        &self,
        thresholds: &crate::quota::HeadroomThresholds,
        now: Timestamp,
    ) -> ProviderHeadroom {
        if self.blocks_at(now) {
            // A `drained` or `unknown` row has no reset to name; an `exhausted`
            // one does, and it is the state's own instant rather than a window's.
            return match self.resets_at {
                Some(blocked_until) => ProviderHeadroom::Blocked { blocked_until },
                None => ProviderHeadroom::Unavailable,
            };
        }
        if let crate::quota::WindowOutlook::Blocked { blocked_until } =
            crate::quota::window_outlook(&self.windows, thresholds)
        {
            return ProviderHeadroom::Blocked { blocked_until };
        }
        // Credit is judged after the windows and never in place of them: on a
        // subscription the windows are the capacity that actually runs out,
        // while the credit is the money guarded behind them.
        match self.credit.map(|credit| credit.clears_reserve()) {
            Some(Err(_)) => ProviderHeadroom::Unavailable,
            Some(Ok(())) | None => ProviderHeadroom::Admissible,
        }
    }
}

/// What one `(account, provider)` pair will accept right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderHeadroom {
    /// New seats may be admitted.
    Admissible,
    /// Nothing may be admitted until this instant, which is known.
    Blocked {
        /// When it clears.
        blocked_until: Timestamp,
    },
    /// Nothing may be admitted and no clock will change that — a drained
    /// balance, a currency that cannot be compared, or a refusal nobody parsed.
    /// Only money or an operator lifts it.
    Unavailable,
}

/// One provider quota state to record, under its expected revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProviderQuotaState {
    /// Owning project.
    pub project_id: ProjectId,
    /// The account the state is about.
    pub account_profile_id: AccountProfileId,
    /// The provider.
    pub provider: String,
    /// What the quota is doing.
    pub state: crate::spec::ProviderQuotaKind,
    /// When an exhausted allowance returns.
    pub resets_at: Option<Timestamp>,
    /// Every concurrent window observed on this pair.
    pub windows: Vec<crate::quota::QuotaWindow>,
    /// The depleting balance and its floor, where this provider has one.
    pub credit: Option<crate::quota::CreditBalance>,
    /// Digest of the evidence.
    pub evidence_hash: ContentHash,
    /// Who concluded it.
    pub source: crate::spec::ProviderQuotaSource,
    /// When it was concluded.
    pub observed_at: Timestamp,
    /// The revision the caller believes is current; `1` for the first.
    pub expected_revision: AggregateRevision,
    /// Mutation instant.
    pub updated_at: Timestamp,
}

/// One immutable proof that an exact configured account answered its vendor's
/// usage endpoint successfully.
///
/// This is deliberately separate from [`ProviderQuotaState`]. That projection
/// changes only when the provider's answer changes, while this append-only row
/// proves *when* an unchanged answer was observed again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsageObservation {
    /// The observation.
    pub id: ProviderUsageObservationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The exact configured account that was polled.
    pub account_profile_id: AccountProfileId,
    /// The exact selectable provider route the reading was projected onto.
    pub provider: String,
    /// Digest of the provider response after provider-specific parsing.
    pub evidence_hash: ContentHash,
    /// State derived from that response.
    pub state: crate::spec::ProviderQuotaKind,
    /// Reset instant derived for an exhausted response.
    pub resets_at: Option<Timestamp>,
    /// Concurrent windows derived from the response.
    pub windows: Vec<crate::quota::QuotaWindow>,
    /// When the successful response was observed.
    pub observed_at: Timestamp,
}

/// One successful usage poll to append, optionally accompanied by a changed
/// provider-quota projection in the same transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProviderUsageObservation {
    /// Immutable observation body.
    pub observation: ProviderUsageObservation,
    /// A changed provider-report projection. `None` means the digest is
    /// unchanged and only the freshness heartbeat is appended.
    pub quota_state: Option<NewProviderQuotaState>,
    /// Explicit probe replay key. Background polls have no command key.
    pub idempotency_key: Option<IdempotencyKey>,
    /// Canonical explicit-probe intent paired with the key above.
    pub intent_hash: Option<ContentHash>,
}

/// Account-owned capacity evidence and the judgement standing beside it.
///
/// Raw first: [`CapacityRepository::record_capacity_observation`] is the only
/// way a reading enters the Realm, and the row it writes can never be updated
/// or deleted. An override is a separate record precisely so that recording one
/// cannot silently rewrite what a provider reported.
pub trait CapacityRepository {
    /// Persist one raw reading together with what was derived from it.
    ///
    /// # Errors
    /// Refuses an unknown or cross-project account profile, and a duplicate
    /// observation id — a replayed collector reading is not a second fact.
    fn record_capacity_observation(
        &self,
        request: &NewCapacityObservation,
    ) -> RepositoryResult<CapacityObservation>;

    /// Read one raw observation.
    ///
    /// # Errors
    /// Backend failures only; another project's observation is `Ok(None)`.
    fn get_capacity_observation(
        &self,
        project_id: ProjectId,
        id: CapacityObservationId,
    ) -> RepositoryResult<Option<CapacityObservation>>;

    /// The most recent observation for each of a project's accounts.
    ///
    /// # Errors
    /// Backend failures only.
    fn latest_capacity_observations(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<CapacityObservation>>;

    /// Record or replace one account's standing operator judgement.
    ///
    /// The first judgement about an account is written at
    /// [`AggregateRevision::INITIAL`] and must be presented as such, because a
    /// revision cannot be zero and so "there is no record" has no other
    /// spelling. The consequence is deliberate and narrow: two operators who
    /// both read *no* override may both write, and the second wins. Every
    /// subsequent write is an ordinary compare-and-swap.
    ///
    /// # Errors
    /// Refuses an unknown account profile and a stale expected revision. On
    /// refusal nothing is written — in particular no observation is touched.
    fn set_availability_override(
        &self,
        request: &NewAvailabilityOverride,
    ) -> RepositoryResult<AvailabilityOverride>;

    /// Record or replace one account's quota state for one provider.
    ///
    /// Same first-write rule as [`Self::set_availability_override`]: the first
    /// state for a `(account, provider)` pair is written at
    /// [`AggregateRevision::INITIAL`] and must be presented as such.
    ///
    /// # Errors
    /// Refuses an unknown account profile, a stale expected revision, and a
    /// state whose reset instant contradicts it — an exhausted allowance without
    /// one, or any other state with one.
    fn set_provider_quota_state(
        &self,
        request: &NewProviderQuotaState,
    ) -> RepositoryResult<ProviderQuotaState>;

    /// Every provider quota state in one project.
    ///
    /// Returned whole rather than filtered by provider: a rung walk asks about
    /// each provider in a chain in turn, and one read it can index is cheaper
    /// and more consistent than one query per rung.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_provider_quota_states(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<ProviderQuotaState>>;

    /// Atomically apply a changed provider-report projection, when present, and
    /// append the successful usage observation even when its digest is unchanged.
    ///
    /// # Errors
    /// Refuses an unknown/cross-project account, mismatched observation and
    /// projection identities, stale quota revision, duplicate observation id,
    /// or an explicit probe key reused for different canonical intent. Every
    /// refusal writes neither table.
    fn record_provider_usage_observation(
        &self,
        request: &NewProviderUsageObservation,
    ) -> RepositoryResult<ProviderUsageObservation>;

    /// Read the most recent successful observation for one exact route.
    ///
    /// This is the freshness seam used by admission: callers still judge the
    /// matching current [`ProviderQuotaState`] for headroom, and must require
    /// provider-report source plus the same evidence hash.
    fn latest_provider_usage_observation(
        &self,
        project_id: ProjectId,
        account_profile_id: AccountProfileId,
        provider: &str,
    ) -> RepositoryResult<Option<ProviderUsageObservation>>;

    /// Resolve one explicit probe replay globally without touching the provider.
    ///
    /// Probe keys are globally unique. The returned observation therefore
    /// carries the stored project, account and provider that a caller must
    /// compare before resolving any credential or contacting a vendor.
    fn provider_usage_observation_by_key(
        &self,
        key: &IdempotencyKey,
    ) -> RepositoryResult<Option<(ProviderUsageObservation, ContentHash)>>;

    /// A project's standing operator judgements, including lapsed ones.
    ///
    /// Expiry is a fact the caller applies, not a row the store deletes: a
    /// lapsed judgement is still a record that someone made it.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_availability_overrides(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<AvailabilityOverride>>;
}

/// The Realm's capacity ceilings as an operator set them.
///
/// Realm-scoped rather than project-scoped, so the operations that read and
/// replace it are inherent store methods rather than [`CapacityRepository`]
/// ones: a realm-wide write has no aggregate to name and is made idempotent
/// through the realm binding table instead of a command receipt.
///
/// The `ceilings` document is opaque here: `kontor-scheduler` owns what a
/// ceiling means, and a store that could parse one would eventually validate it
/// twice, differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCapacityConfiguration {
    /// The ceilings, verbatim.
    pub ceilings: CanonicalDocument,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Last mutation instant.
    pub updated_at: Timestamp,
}

/// Generic topology, typed seat and persisted adaptive-window state.
pub trait TopologyRepository {
    /// Publish one immutable project topology specification revision.
    ///
    /// A published topology specification is project configuration, so it is
    /// tier B and always carries a write-time classification. The stamp lives
    /// beside the document rather than inside it: the canonical hash keeps
    /// identifying the specification text alone, so withholding a revision
    /// never changes the hash an epic pinned.
    ///
    /// # Errors
    /// Refuses an invalid document or stamp, dangling project or duplicate
    /// revision.
    fn publish_topology_spec(
        &self,
        project_id: ProjectId,
        spec: &ProjectSessionTopologySpec,
        shareability: &Shareability,
        published_at: Timestamp,
    ) -> RepositoryResult<ContentHash>;

    /// Read one topology specification revision's immutable classification.
    ///
    /// # Errors
    /// Backend/domain failures only; a missing revision is `Ok(None)`.
    fn get_topology_spec_shareability(
        &self,
        project_id: ProjectId,
        spec_id: TopologySpecId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<Shareability>>;

    /// Read one topology specification revision and re-prove its digest.
    ///
    /// # Errors
    /// Backend/domain failures only; a missing revision is `Ok(None)`.
    fn get_topology_spec(
        &self,
        project_id: ProjectId,
        spec_id: TopologySpecId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<ProjectSessionTopologySpec>>;

    /// Every topology specification revision published in one project.
    ///
    /// The set an upgrade may move an epic to. Ordered by identity and version
    /// so a search over it is deterministic — a preview digest that resolved to
    /// a different revision depending on row order would not be a digest of
    /// anything.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_topology_specs(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<ProjectSessionTopologySpec>>;

    /// Select an already-published topology revision for future project scopes.
    ///
    /// # Errors
    /// Refuses a missing/hash-mismatched revision.
    fn set_project_topology_default(
        &self,
        selection: &ProjectTopologyDefault,
    ) -> RepositoryResult<()>;

    /// Read the selected project default.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_project_topology_default(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<ProjectTopologyDefault>>;

    /// Move one already-pinned epic to a different published revision.
    ///
    /// Deliberately not the same call as
    /// [`TopologyRepository::pin_mini_project_topology`]: an epic with no pin
    /// yet is refused here, so this operation can never quietly create the
    /// first pin, and the first pin can never be an upgrade of nothing. What
    /// moves is the epic's current position; the revisions on either side stay
    /// immutable, and the move itself is audited by the receipt that carries it.
    ///
    /// # Errors
    /// Refuses an unpinned or cross-project epic and a target revision that is
    /// not published in this project.
    fn repin_mini_project_topology(
        &self,
        snapshot: &MiniProjectTopologySnapshot,
    ) -> RepositoryResult<()>;

    /// Pin one immutable topology revision/hash to a MiniProject.
    ///
    /// # Errors
    /// Refuses a cross-project/missing MiniProject, missing revision, changed
    /// hash or a second pin.
    fn pin_mini_project_topology(
        &self,
        snapshot: &MiniProjectTopologySnapshot,
    ) -> RepositoryResult<()>;

    /// Read one MiniProject's pinned topology revision.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_mini_project_topology(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<MiniProjectTopologySnapshot>>;

    /// Publish one immutable standard-role catalog revision.
    ///
    /// A published role catalog is project configuration, so it is tier B and
    /// always carries a write-time classification.
    ///
    /// # Errors
    /// Refuses an invalid or duplicate revision, or an invalid stamp.
    fn publish_role_catalog(
        &self,
        catalog: &RoleCatalogRevision,
        shareability: &Shareability,
        published_at: Timestamp,
    ) -> RepositoryResult<ContentHash>;

    /// Read one role catalog revision's immutable classification.
    ///
    /// # Errors
    /// Backend/domain failures only; a missing revision is `Ok(None)`.
    fn get_role_catalog_shareability(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<Shareability>>;

    /// Read one standard-role catalog revision and re-prove its digest.
    ///
    /// # Errors
    /// Backend/domain failures only; a missing revision is `Ok(None)`.
    fn get_role_catalog(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<RoleCatalogRevision>>;

    /// Create one logical topology node in the declared tree.
    ///
    /// # Errors
    /// Refuses undeclared kinds, illegal/dangling parents, cross-project
    /// references and maximum-cardinality violations.
    fn create_topology_node(
        &self,
        request: &NewSessionTopologyNode,
    ) -> RepositoryResult<SessionTopologyNode>;

    /// List all logical nodes in one project and optional MiniProject scope.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_topology_nodes(
        &self,
        project_id: ProjectId,
        mini_project_id: Option<MiniProjectId>,
    ) -> RepositoryResult<Vec<SessionTopologyNode>>;

    /// Every logical node in one project, parents before children.
    ///
    /// Distinct from [`TopologyRepository::list_topology_nodes`], whose `None`
    /// scope means "the nodes that belong to no epic" rather than "all of
    /// them". A whole-project inspection needs the second reading, and reading
    /// it as the first is how a projection quietly reports only the root.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_project_topology_nodes(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<SessionTopologyNode>>;

    /// Read one logical node by its durable identity.
    ///
    /// # Errors
    /// Backend failures only; another project's node is `Ok(None)`.
    fn get_topology_node(
        &self,
        project_id: ProjectId,
        id: TopologyNodeId,
    ) -> RepositoryResult<Option<SessionTopologyNode>>;

    /// Move one node's lifecycle under a compare-and-swap.
    ///
    /// The order is fixed and one-way — active, retired, archived — because a
    /// node that could go back to active would resurrect the seats and the
    /// native container its retirement concluded were finished with.
    ///
    /// # Errors
    /// Refuses an unknown node, a stale revision, a backwards or repeated
    /// transition, and retiring a node that still has children or non-terminal
    /// seats. On refusal no column is written.
    fn transition_topology_node(
        &self,
        project_id: ProjectId,
        id: TopologyNodeId,
        lifecycle: TopologyLifecycle,
        expected_revision: AggregateRevision,
        updated_at: Timestamp,
    ) -> RepositoryResult<SessionTopologyNode>;

    /// Create one active logical seat binding.
    ///
    /// # Errors
    /// Refuses a dangling/cross-project node, role/catalog mismatch, optional
    /// task/TeamRun mismatch or duplicate non-terminal `(node, slot)` key.
    fn create_seat_binding(&self, request: &NewSeatBinding) -> RepositoryResult<SeatBinding>;

    /// List seat bindings hosted by one logical node.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_seat_bindings(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Vec<SeatBinding>>;

    /// Read one seat by its exact binding identity.
    ///
    /// The read behind every exact-seat operation. It takes the binding id and
    /// nothing else addressable — no name, no `cwd`, no scan — which is what
    /// makes "this seat" mean one row rather than the first row that looked
    /// like it. A binding from another project is `Ok(None)`.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_seat_binding(
        &self,
        project_id: ProjectId,
        id: SeatBindingId,
    ) -> RepositoryResult<Option<SeatBinding>>;

    /// The active topology node serving one delivery task.
    ///
    /// The read admission makes before it places anything. `None` means this
    /// task has no node, which is a refusal for a project running an
    /// Operational topology and simply normal for one that is not.
    ///
    /// # Errors
    /// Backend failures only. A second active node for one task cannot be
    /// stored, so this never has to choose between two.
    fn get_task_topology_node(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<SessionTopologyNode>>;

    /// Record what was observed about one seat's attachment.
    ///
    /// Advances only the fields the observation carries, so recording an
    /// attachment cannot silently clear an activity instant, and neither can
    /// overwrite a release.
    ///
    /// # Errors
    /// Refuses an unknown seat and a replacement citing the seat itself.
    fn observe_seat_binding(
        &self,
        project_id: ProjectId,
        id: SeatBindingId,
        observation: &SeatLivenessObservation,
        observed_at: Timestamp,
    ) -> RepositoryResult<SeatBinding>;

    /// Bind one topology node to the native container read back for it.
    ///
    /// Idempotent per node *for the same native identity*: re-confirming a
    /// binding advances its readback instant and creates nothing. A node whose
    /// stored identity differs is a disagreement to report, never a rebinding
    /// to perform silently — reconciliation refuses invalid state rather than
    /// making one side match the other.
    ///
    /// # Errors
    /// Refuses a dangling/cross-project node, a native container already bound
    /// to a different node, and a rebinding of a node that already holds
    /// another identity.
    fn bind_topology_node_container(
        &self,
        request: &NewNativeContainerBinding,
    ) -> RepositoryResult<NativeContainerBinding>;

    /// Conclude the attachment of every seat hosted by one topology node.
    ///
    /// The read a watch, reap or stale path uses: it resolves exact Kontor
    /// bindings and concludes from recorded evidence, and it consults no
    /// runtime and no AgentsRoom file to do it.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_seat_attachments(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        now: Timestamp,
    ) -> RepositoryResult<Vec<SeatAttachment>>;

    /// Read the current native container binding of one topology node.
    ///
    /// # Errors
    /// Backend failures only. An unbound node is `None` rather than an error:
    /// "not placed yet" is a normal state, and the caller that must refuse it
    /// says so in its own words.
    fn get_topology_node_container(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<NativeContainerBinding>>;

    /// Create one MiniProject's persisted adaptive-admission state.
    ///
    /// # Errors
    /// Refuses invalid values, a dangling/cross-project MiniProject or a
    /// duplicate state row.
    fn create_adaptive_admission_state(
        &self,
        request: &NewAdaptiveAdmissionState,
    ) -> RepositoryResult<AdaptiveAdmissionState>;

    /// Advance adaptive-admission state under compare-and-swap.
    ///
    /// The scheduler owns the decision; this operation only persists it and
    /// refuses replay of the last observation id.
    ///
    /// # Errors
    /// Refuses a stale revision, replayed observation or invalid bounds.
    fn advance_adaptive_admission_state(
        &self,
        request: &AdaptiveAdmissionAdvance,
    ) -> RepositoryResult<AdaptiveAdmissionState>;

    /// Read persisted adaptive-admission state.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_adaptive_admission_state(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<AdaptiveAdmissionState>>;

    /// Every epic in one project that has an adaptive position.
    ///
    /// A capacity observation is about a provider account, and the accounts a
    /// project uses serve every epic in it — so one reading moves each epic's
    /// position, and this is the list of positions there are to move. An epic
    /// with no row is one that was never applied through this build.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_adaptive_admission_states(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<AdaptiveAdmissionState>>;
}

// ---------------------------------------------------------------------------
// Team Definition
// ---------------------------------------------------------------------------

/// The selected Team Definition revision for future project scopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTeamDefinitionDefault {
    /// Owning project.
    pub project_id: ProjectId,
    /// The selection this apply was previewed against.
    ///
    /// `None` asserts the project had no default when the preview was taken.
    /// The apply binds this expectation in the same transaction that writes, so
    /// a bootstrap that observed "no default" cannot overwrite an explicit
    /// selection made in between.
    pub expected: Option<TeamDefinitionSnapshot>,
    /// Exact published revision and hash.
    pub definition: TeamDefinitionSnapshot,
    /// Selection instant.
    pub selected_at: Timestamp,
}

/// One immutable Team Definition revision frozen by a MiniProject/epic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniProjectTeamDefinitionSnapshot {
    /// Owning project.
    pub project_id: ProjectId,
    /// Target MiniProject.
    pub mini_project_id: MiniProjectId,
    /// Exact published revision and hash.
    pub definition: TeamDefinitionSnapshot,
    /// Pinning instant.
    pub pinned_at: Timestamp,
}

crate::closed_enum! {
    /// How far one durable Team Definition migration has got.
    ///
    /// The epic keeps its old pin through `Recorded` and `Applying`; only
    /// `Confirmed` has moved it, and only after every target read back.
    TeamDefinitionMigrationState, "TeamDefinitionMigrationState" {
        /// Persisted before any runtime effect, with its complete target set.
        Recorded => "recorded",
        /// At least one retitle has been attempted.
        Applying => "applying",
        /// Every target read back its exact desired title under an unchanged
        /// native id, and the governed pins have moved.
        Confirmed => "confirmed",
        /// Abandoned. The epic keeps the definition its natives still render.
        Failed => "failed",
    }
}

impl TeamDefinitionMigrationState {
    /// Whether the migration may still be advanced.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Failed)
    }

    /// Whether this state fences new materialization for the affected epic.
    #[must_use]
    pub const fn is_in_flight(self) -> bool {
        matches!(self, Self::Recorded | Self::Applying)
    }
}

crate::closed_enum! {
    /// What was actually observed for one native object in a migration.
    ///
    /// `RenamePending` exists so a target that was asked and not confirmed is
    /// recorded as exactly that, rather than as a success it never had.
    TeamDefinitionMigrationTargetState, "TeamDefinitionMigrationTargetState" {
        /// Enumerated by the preview; no effect attempted yet.
        Pending => "pending",
        /// Already carried the desired title; nothing was asked of the runtime.
        Unchanged => "unchanged",
        /// Retitled and read back exactly, under an unchanged native id.
        Renamed => "renamed",
        /// A rename was asked for and the desired title has not read back.
        RenamePending => "rename_pending",
        /// The runtime refused, or the readback contradicted the request.
        Failed => "failed",
    }
}

/// Which native object of a topology node one migration target is about.
///
/// A node is not one native object. An ECP node carries its own container and
/// the LSA and TPM seats inside it; a CSW node carries its container plus
/// `SEAT A`, `SEAT B` and `JUDGE`. Naming the subject is what keeps every one
/// of them a target of its own instead of collapsing them into the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamDefinitionMigrationSubject {
    /// The container the node projects.
    Container {
        /// The node whose container is retitled.
        topology_node_id: TopologyNodeId,
    },
    /// One seat inside that container.
    Seat {
        /// The node hosting the seat.
        topology_node_id: TopologyNodeId,
        /// The exact seat being retitled.
        seat_binding_id: SeatBindingId,
    },
}

impl TeamDefinitionMigrationSubject {
    /// The node this subject belongs to, whichever kind it is.
    #[must_use]
    pub const fn topology_node_id(self) -> TopologyNodeId {
        match self {
            Self::Container { topology_node_id }
            | Self::Seat {
                topology_node_id, ..
            } => topology_node_id,
        }
    }

    /// The seat, when this subject is one.
    #[must_use]
    pub const fn seat_binding_id(self) -> Option<SeatBindingId> {
        match self {
            Self::Container { .. } => None,
            Self::Seat {
                seat_binding_id, ..
            } => Some(seat_binding_id),
        }
    }

    /// The stable storage key, derived from durable identity.
    ///
    /// Never a title and never an ordinal: a resumed apply has to address the
    /// same target it enumerated, and a display name is not identity.
    #[must_use]
    pub fn target_key(self) -> String {
        match self {
            Self::Container { topology_node_id } => format!("container:{topology_node_id}"),
            Self::Seat {
                seat_binding_id, ..
            } => format!("seat:{seat_binding_id}"),
        }
    }
}

crate::closed_enum! {
    /// What kind of native object one migration target is.
    ///
    /// Deliberately not [`ObservedContainerKind`], which knows only about
    /// containers. A migration retitles seats as well, and calling a seat a
    /// "workspace" so that it fits a container vocabulary would put a false
    /// statement into the evidence a retitle is supposed to prove.
    MigrationObjectKind, "MigrationObjectKind" {
        /// A native root/project container.
        ProjectContainer => "project_container",
        /// A container below a native root.
        WorkspaceContainer => "workspace_container",
        /// One seat inside a container.
        Seat => "seat",
    }
}

impl MigrationObjectKind {
    /// Whether this kind is a container rather than a seat.
    #[must_use]
    pub const fn is_container(self) -> bool {
        matches!(self, Self::ProjectContainer | Self::WorkspaceContainer)
    }
}

/// Where one native object sits and what it is called.
///
/// A title on its own cannot prove a retitle happened to the object we meant:
/// a replaced container can carry the desired title while being a different
/// object in a different place. Recording the placement at preview and proving
/// it again at readback is what makes the migration identity-preserving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePlacement {
    /// Exact rendered title.
    pub title: ExternalName,
    /// Native id of the container this object sits in.
    ///
    /// Absent only for a native root. A seat always has one: proving a seat was
    /// retitled means proving it is still the same session in the same
    /// container on the same host, not merely that something somewhere now
    /// carries the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_native_id: Option<ExternalId>,
    /// What the runtime says this object is.
    pub kind: MigrationObjectKind,
    /// Canonical working directory, where the object has one.
    ///
    /// Containers below a root have one; seats need not, so this stays optional
    /// rather than forcing a seat to invent a directory it does not own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_cwd: Option<ExternalName>,
}

impl NativePlacement {
    /// Check the placement is internally coherent for its kind.
    ///
    /// # Errors
    /// Refuses a root that claims a parent, and a workspace or seat that has
    /// none: in both cases the recorded placement could not be re-proved
    /// against the runtime it claims to describe.
    pub fn validate(&self) -> DomainResult<()> {
        match self.kind {
            MigrationObjectKind::ProjectContainer if self.parent_native_id.is_some() => Err(
                DomainError::invalid("NativePlacement", "a native root has no parent container"),
            ),
            MigrationObjectKind::WorkspaceContainer | MigrationObjectKind::Seat
                if self.parent_native_id.is_none() =>
            {
                Err(DomainError::invalid(
                    "NativePlacement",
                    "a workspace or seat must name the container it sits in",
                ))
            }
            _ => Ok(()),
        }
    }

    /// Whether this placement describes the subject it is recorded against.
    #[must_use]
    pub const fn matches_subject(&self, subject: TeamDefinitionMigrationSubject) -> bool {
        matches!(
            (subject, self.kind),
            (
                TeamDefinitionMigrationSubject::Seat { .. },
                MigrationObjectKind::Seat
            ) | (
                TeamDefinitionMigrationSubject::Container { .. },
                MigrationObjectKind::ProjectContainer | MigrationObjectKind::WorkspaceContainer
            )
        )
    }
}

/// One native object a migration must retitle, as it is recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamDefinitionMigrationTarget {
    /// Owning intent.
    pub intent_id: TeamDefinitionMigrationId,
    /// Exactly which native object this target is.
    pub subject: TeamDefinitionMigrationSubject,
    /// The four-part native identity recorded before the effect and re-proved
    /// after it. A bare native id is not an identity: it names that object only
    /// inside one generation, on one host, of one runtime family.
    pub identity: NativeRuntimeIdentity,
    /// The placement the pinned definition requires for this object.
    pub desired: NativePlacement,
    /// The placement actually read back, when one has been.
    pub observed: Option<NativePlacement>,
    /// What was observed.
    pub state: TeamDefinitionMigrationTargetState,
    /// Last observation instant.
    pub updated_at: Timestamp,
}

/// A new durable migration intent, recorded before the first runtime effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTeamDefinitionMigration {
    /// Intent identity.
    pub id: TeamDefinitionMigrationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic whose pin will move once every target reads back.
    pub mini_project_id: MiniProjectId,
    /// The key this migration replays under. Same key, same migration.
    pub idempotency_key: IdempotencyKey,
    /// Digest of the exact canonical command intent this migration is issued
    /// under, including its preview hash and legacy-topic map.
    ///
    /// Recorded before any external effect, because it is what a retry after a
    /// crash is compared against. The fingerprint cannot stand in for it: a
    /// retry can carry a different preview or topic map and still fingerprint
    /// identically, since those are inputs to the command rather than parts of
    /// the enumerated plan.
    pub command_intent_hash: ContentHash,
    /// The pin the epic holds now; absent when it is being pinned first.
    pub from: Option<TeamDefinitionSnapshot>,
    /// The pin the epic moves to on confirmation.
    pub to: TeamDefinitionSnapshot,
    /// The complete target set the preview produced.
    pub targets: Vec<NewTeamDefinitionMigrationTarget>,
    /// Recording instant.
    pub recorded_at: Timestamp,
}

/// One enumerated target of a new migration intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTeamDefinitionMigrationTarget {
    /// Exactly which native object this target is.
    pub subject: TeamDefinitionMigrationSubject,
    /// The four-part native identity observed at preview time.
    pub identity: NativeRuntimeIdentity,
    /// The placement the target definition requires.
    pub desired: NativePlacement,
}

impl NewTeamDefinitionMigration {
    /// Digest of every semantic part of this request.
    ///
    /// Two requests share a fingerprint exactly when they ask for the same
    /// migration: the same epic, moving from the same pin to the same pin, over
    /// the same set of native objects, each to the same placement. The intent
    /// id and the recording instant are excluded because they are retry
    /// bookkeeping rather than what was asked for; the idempotency key is
    /// excluded because it is the question being asked, not the answer.
    ///
    /// Targets are folded in sorted order so a caller that enumerates them in a
    /// different sequence still replays rather than conflicts.
    #[must_use]
    pub fn fingerprint(&self) -> ContentHash {
        fn snapshot_text(snapshot: Option<&TeamDefinitionSnapshot>) -> String {
            snapshot.map_or_else(
                || "none".to_owned(),
                |snapshot| {
                    format!(
                        "{}:{}:{}",
                        snapshot.definition_id,
                        snapshot.version.get(),
                        snapshot.canonical_hash.as_str()
                    )
                },
            )
        }
        fn optional(value: Option<&str>) -> &str {
            value.unwrap_or("\x00none")
        }

        let mut parts = vec![
            format!("epic:{}", self.mini_project_id),
            format!("from:{}", snapshot_text(self.from.as_ref())),
            format!("to:{}", snapshot_text(Some(&self.to))),
        ];
        let mut targets: Vec<String> = self
            .targets
            .iter()
            .map(|target| {
                format!(
                    "target:{}\x1e{}\x1e{}\x1e{}\x1e{}\x1e{}\x1e{}\x1e{}\x1e{}",
                    target.subject.target_key(),
                    target.identity.runtime_kind.as_str(),
                    target.identity.host.as_str(),
                    target.identity.generation,
                    target.identity.native_id.as_str(),
                    target.desired.title.as_str(),
                    optional(
                        target
                            .desired
                            .parent_native_id
                            .as_ref()
                            .map(ExternalId::as_str)
                    ),
                    target.desired.kind.as_str(),
                    optional(
                        target
                            .desired
                            .canonical_cwd
                            .as_ref()
                            .map(ExternalName::as_str)
                    ),
                )
            })
            .collect();
        targets.sort();
        parts.extend(targets);
        ContentHash::of(parts.join("\x1d").as_bytes())
    }
}

/// One native object of an epic that a migration is obliged to cover.
///
/// "Live" means the object exists natively right now: a topology node holding a
/// native container, or a seat holding a native session. These are exactly the
/// things whose titles the epic's pin claims to describe, so a migration that
/// leaves one out would move the pin to a definition that part of the epic does
/// not render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveNativeSubject {
    /// Which native object this is.
    pub subject: TeamDefinitionMigrationSubject,
    /// The topology kind of the node it belongs to.
    ///
    /// Carried so a preflight can ask the target definition whether it can name
    /// this kind at all, rather than discovering mid-apply that it cannot.
    pub node_kind: TopologyKindKey,
    /// Its exact four-part native identity.
    pub identity: NativeRuntimeIdentity,
}

/// One durable migration intent as it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTeamDefinitionMigration {
    /// Intent identity.
    pub id: TeamDefinitionMigrationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic whose pin moves on confirmation.
    pub mini_project_id: MiniProjectId,
    /// The key this migration replays under.
    pub idempotency_key: IdempotencyKey,
    /// Digest of everything that makes this request the request it is.
    pub fingerprint: ContentHash,
    /// Digest of the exact canonical command intent it was issued under.
    pub command_intent_hash: ContentHash,
    /// The pin held when the intent was recorded.
    pub from: Option<TeamDefinitionSnapshot>,
    /// The pin the epic moves to on confirmation.
    pub to: TeamDefinitionSnapshot,
    /// How far it has got.
    pub state: TeamDefinitionMigrationState,
    /// The command receipt this migration was commanded under, once one exists.
    ///
    /// `None` on a `Confirmed` migration is the crash window made visible: the
    /// pin moved and the receipt did not get written. Recovery completes the
    /// receipt rather than repeating any native effect.
    pub receipt_id: Option<CommandReceiptId>,
    /// Its complete target set, in deterministic node order.
    pub targets: Vec<TeamDefinitionMigrationTarget>,
    /// Recording instant.
    pub recorded_at: Timestamp,
    /// Last transition instant.
    pub updated_at: Timestamp,
}

/// One observation an apply records about a single native object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamDefinitionMigrationObservation {
    /// The target being reported.
    pub subject: TeamDefinitionMigrationSubject,
    /// The four-part native identity that came back. Any difference is a
    /// refusal: it did not observe the object the migration enumerated.
    pub identity: NativeRuntimeIdentity,
    /// The placement that came back, when the object answered at all.
    pub observed: Option<NativePlacement>,
    /// What was observed.
    pub state: TeamDefinitionMigrationTargetState,
    /// Observation instant.
    pub observed_at: Timestamp,
}

/// The immutable Team Definition, its selection, its epic pins and the durable
/// intent an identity-preserving retitle applies under.
///
/// Deliberately separate from [`TopologyRepository`]. The topology remains a
/// legality validator, and giving naming its own repository is what keeps a
/// topology publication from ever becoming a second current naming authority.
pub trait TeamDefinitionRepository {
    /// Publish one immutable Team Definition revision.
    ///
    /// # Errors
    /// Refuses an invalid document, a dangling project, a topology revision
    /// this project has not published, and a duplicate revision.
    fn publish_team_definition(
        &self,
        project_id: ProjectId,
        definition: &TeamDefinitionSpec,
        published_at: Timestamp,
    ) -> RepositoryResult<ContentHash>;

    /// Read one Team Definition revision and re-prove its digest.
    ///
    /// # Errors
    /// Backend/domain failures only; a missing revision is `Ok(None)`.
    fn get_team_definition(
        &self,
        project_id: ProjectId,
        definition_id: TeamDefinitionId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TeamDefinitionSpec>>;

    /// Every Team Definition revision published in one project.
    ///
    /// Ordered by identity and version so a search over it is deterministic.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_team_definitions(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<TeamDefinitionSpec>>;

    /// Select an already-published revision for future project scopes.
    ///
    /// Existing epic pins are immutable under this call; moving one is the
    /// separate migration authority below.
    ///
    /// # Errors
    /// Refuses a missing or hash-mismatched revision.
    fn set_project_team_definition_default(
        &self,
        selection: &ProjectTeamDefinitionDefault,
    ) -> RepositoryResult<()>;

    /// Read the selected project default.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_project_team_definition_default(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<ProjectTeamDefinitionDefault>>;

    /// Freeze one immutable revision to a MiniProject for the first time.
    ///
    /// # Errors
    /// Refuses a cross-project/missing MiniProject, a missing revision, a
    /// changed hash or a second pin.
    fn pin_mini_project_team_definition(
        &self,
        snapshot: &MiniProjectTeamDefinitionSnapshot,
    ) -> RepositoryResult<()>;

    /// Read one MiniProject's frozen revision.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_mini_project_team_definition(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<MiniProjectTeamDefinitionSnapshot>>;

    /// Every live native-bearing subject of one epic.
    ///
    /// The census a migration preflight is proved against. Ordered
    /// deterministically so a preview digest over it means something.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_live_native_subjects(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<LiveNativeSubject>>;

    /// Find one migration by the key it was recorded under.
    ///
    /// This is the recovery entry point. After a crash between the pin commit
    /// and the receipt write, the idempotency key is the only handle a retrying
    /// caller still holds, and the migration it names is terminal — so no apply
    /// operation will return it.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_team_definition_migration_by_key(
        &self,
        project_id: ProjectId,
        idempotency_key: &IdempotencyKey,
    ) -> RepositoryResult<Option<StoredTeamDefinitionMigration>>;

    /// Bind the command receipt one migration was commanded under.
    ///
    /// Write-once. Binding the same receipt again is the replay of a recovery;
    /// binding a different one would claim the migration was commanded twice.
    /// This touches no target and no pin: by the time it runs, the native
    /// effects have already happened and been read back.
    ///
    /// # Errors
    /// Refuses an unknown migration, a migration that has not been confirmed,
    /// and a second, different receipt.
    fn bind_team_definition_migration_receipt(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        receipt_id: CommandReceiptId,
        bound_at: Timestamp,
    ) -> RepositoryResult<()>;

    /// Record one migration intent and its complete target set before any
    /// runtime effect.
    ///
    /// Replaying the same idempotency key returns the migration already
    /// recorded rather than recording a second one, so a resumed apply
    /// continues the original intent instead of creating a rival to it.
    ///
    /// # Errors
    /// Refuses an empty target set, a target set with a duplicate subject or
    /// native id, a target set that omits any live native subject, a target
    /// definition that cannot name a live node kind, a
    /// migration for an epic that already has one in flight, a `from` that is
    /// not the epic's current pin, and a `to` this project has not published.
    fn record_team_definition_migration(
        &self,
        migration: &NewTeamDefinitionMigration,
    ) -> RepositoryResult<StoredTeamDefinitionMigration>;

    /// Read one migration by identity.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
    ) -> RepositoryResult<Option<StoredTeamDefinitionMigration>>;

    /// Read the migration currently fencing one epic, if any.
    ///
    /// This is the question materialization asks: an epic with an in-flight
    /// migration does not place new containers under a pin that is about to
    /// move.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_in_flight_team_definition_migration(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<StoredTeamDefinitionMigration>>;

    /// Record what one apply actually observed for a set of targets.
    ///
    /// Moves the intent to `applying`. Recording an observation whose native id
    /// is not the one enumerated is refused: a retitle that moved identity did
    /// not retitle the target it was asked about.
    ///
    /// # Errors
    /// Refuses a terminal migration, an unknown target and a native-id
    /// contradiction.
    fn observe_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        observations: &[TeamDefinitionMigrationObservation],
        observed_at: Timestamp,
    ) -> RepositoryResult<StoredTeamDefinitionMigration>;

    /// Move the epic's pin, once, after every target has read back exactly.
    ///
    /// This is the only call that makes the new definition current, and it
    /// refuses while any target is `pending`, `rename_pending` or `failed`. The
    /// pin and the intent's confirmation commit together.
    ///
    /// # Errors
    /// Refuses a terminal migration, an unconfirmed target set, an epic whose
    /// pin is no longer the `from` the intent recorded, and a live census that
    /// has gained a subject this migration does not cover.
    fn confirm_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<StoredTeamDefinitionMigration>;

    /// Abandon one migration, leaving the epic on the pin its natives render.
    ///
    /// # Errors
    /// Refuses a migration that has already reached a terminal state.
    fn fail_team_definition_migration(
        &self,
        project_id: ProjectId,
        id: TeamDefinitionMigrationId,
        failed_at: Timestamp,
    ) -> RepositoryResult<StoredTeamDefinitionMigration>;
}

/// Immutable specification revisions.
pub trait SpecRepository {
    /// Insert one work-profile revision.
    ///
    /// # Errors
    /// Refuses an invalid profile and any duplicate `(id, version)`.
    fn insert_work_profile(
        &self,
        project_id: ProjectId,
        spec: &WorkProfileSpec,
    ) -> RepositoryResult<ContentHash>;

    /// Read one work-profile revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_work_profile(
        &self,
        project_id: ProjectId,
        id: &WorkProfileKey,
        version: SpecVersion,
    ) -> RepositoryResult<Option<WorkProfileSpec>>;

    /// Insert one team-template revision.
    ///
    /// # Errors
    /// Refuses any duplicate `(id, version)`.
    fn insert_team_template(
        &self,
        project_id: ProjectId,
        revision: &TeamTemplateRevision,
    ) -> RepositoryResult<ContentHash>;

    /// Read one team-template revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_team_template(
        &self,
        project_id: ProjectId,
        id: TeamTemplateId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TeamTemplateRevision>>;

    /// Insert one persona-scenario revision.
    ///
    /// # Errors
    /// Refuses a self-approving scenario, a production reference and any
    /// duplicate `(id, version)`.
    fn insert_persona_scenario(
        &self,
        project_id: ProjectId,
        spec: &PersonaScenarioSpec,
    ) -> RepositoryResult<ContentHash>;

    /// Read one persona-scenario revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_persona_scenario(
        &self,
        project_id: ProjectId,
        id: PersonaScenarioId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<PersonaScenarioSpec>>;

    /// Insert one trigger revision.
    ///
    /// # Errors
    /// Refuses an unbounded trigger and any duplicate `(id, version)`.
    fn insert_trigger_spec(
        &self,
        project_id: ProjectId,
        spec: &TriggerSpec,
    ) -> RepositoryResult<ContentHash>;

    /// Read one trigger revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_trigger_spec(
        &self,
        project_id: ProjectId,
        id: &TriggerKey,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TriggerSpec>>;

    /// Insert one calendar-profile revision. Calendar profiles are
    /// workspace-level, not project-scoped.
    ///
    /// # Errors
    /// Refuses invalid windows and any duplicate `(id, version)`.
    fn insert_calendar_profile(&self, spec: &CalendarProfileSpec) -> RepositoryResult<ContentHash>;

    /// Read one calendar-profile revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_calendar_profile(
        &self,
        id: CalendarProfileId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<CalendarProfileSpec>>;

    /// Insert one ticket-field-mapping revision.
    ///
    /// # Errors
    /// Refuses an invalid mapping set and any duplicate revision.
    fn insert_ticket_field_spec(
        &self,
        project_id: ProjectId,
        spec: &TicketFieldSpec,
    ) -> RepositoryResult<ContentHash>;

    /// Read one ticket-field-mapping revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_ticket_field_spec(
        &self,
        selector: &ConnectorSpecSelector,
    ) -> RepositoryResult<Option<TicketFieldSpec>>;

    /// Insert one external-workflow revision.
    ///
    /// # Errors
    /// Refuses an invalid workflow and any duplicate revision.
    fn insert_external_workflow_spec(
        &self,
        project_id: ProjectId,
        spec: &ExternalWorkflowSpec,
    ) -> RepositoryResult<ContentHash>;

    /// Read one external-workflow revision.
    ///
    /// # Errors
    /// Refuses a stored document that no longer matches its digest.
    fn get_external_workflow_spec(
        &self,
        selector: &ConnectorSpecSelector,
    ) -> RepositoryResult<Option<ExternalWorkflowSpec>>;
}

/// Task workflows, phases, gates and the task lifecycle.
pub trait WorkflowRepository {
    /// Freeze a resolved profile onto a task.
    ///
    /// # Errors
    /// Refuses a second active workflow for the same task.
    fn create_task_workflow(&self, request: &NewTaskWorkflow) -> RepositoryResult<TaskWorkflow>;

    /// Read a task's active workflow.
    ///
    /// # Errors
    /// Refuses a snapshot that no longer matches its pinned digest.
    fn get_active_task_workflow(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<TaskWorkflow>>;

    /// Advance the current phase under a compare-and-swap.
    ///
    /// # Errors
    /// Refuses an unknown phase, an edge the profile does not declare and a
    /// stale revision.
    fn advance_phase(&self, request: &PhaseAdvance) -> RepositoryResult<AggregateRevision>;

    /// Append a gate evaluation.
    ///
    /// # Errors
    /// Refuses an unauthorized evaluator, a waiver the profile forbids and a
    /// pass or waiver without evidence.
    fn append_gate_evaluation(&self, request: &NewGateEvaluation) -> RepositoryResult<u32>;

    /// The current state of every gate in a workflow, reduced from the
    /// append-only evaluations.
    ///
    /// # Errors
    /// Backend failures only.
    fn gate_states(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
    ) -> RepositoryResult<BTreeMap<GateKey, GateState>>;

    /// Every evaluation recorded for a workflow, in order.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_gate_evaluations(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
    ) -> RepositoryResult<Vec<GateEvaluation>>;

    /// Freeze a persona scenario onto a task, proving its authority against the
    /// gate the task's pinned profile declares.
    ///
    /// # Errors
    /// Refuses a cross-project task/scenario, a workflow that is not the task's,
    /// a gate the pinned profile does not declare, and any actor/evaluator or
    /// evaluator/waiver authority overlap.
    fn create_task_persona_snapshot(
        &self,
        request: &NewTaskPersonaSnapshot,
    ) -> RepositoryResult<PersonaScenarioSnapshot>;

    /// Read a frozen persona snapshot, revalidating its digest and pins.
    ///
    /// # Errors
    /// Refuses a snapshot whose stored bytes no longer match their digest.
    fn get_task_persona_snapshot(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        scenario_id: PersonaScenarioId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<PersonaScenarioSnapshot>>;

    /// Move a task's lifecycle state under a compare-and-swap.
    ///
    /// # Errors
    /// Refuses a terminal task, an illegal transition, a resume without a
    /// receipt, a completion without profile closure, and any terminal
    /// transition whose team obligations are unaccounted for — a cited team run
    /// that is not this task's, has not closed, or still has an open run.
    fn transition_task(&self, request: &TaskTransitionRequest) -> RepositoryResult<Task>;
}

/// Team runs, agent runs, runtime events and closure.
pub trait RunRepository {
    /// Create a team run.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project task.
    fn create_team_run(&self, request: &NewTeamRun) -> RepositoryResult<TeamRun>;

    /// Create an agent run, optionally as the successor of a closed one.
    ///
    /// # Errors
    /// Refuses a dangling parent, a cross-project parent and a parent cycle.
    fn create_agent_run(&self, request: &NewAgentRun) -> RepositoryResult<AgentRun>;

    /// Every team run of one task that has not closed.
    ///
    /// The set a logical repair has to consider: a run still open is one whose
    /// declared slots ought to have seats, whatever happened at launch.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_open_team_runs(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<TeamRun>>;

    /// Read a team run inside a project.
    ///
    /// # Errors
    /// Refuses a snapshot that no longer matches its pinned digest.
    fn get_team_run(
        &self,
        project_id: ProjectId,
        id: TeamRunId,
    ) -> RepositoryResult<Option<TeamRun>>;

    /// Read an agent run inside a project.
    ///
    /// # Errors
    /// Backend failures only; a run from another project is `Ok(None)`.
    fn get_agent_run(
        &self,
        project_id: ProjectId,
        id: AgentRunId,
    ) -> RepositoryResult<Option<AgentRun>>;

    /// Freeze one run's requested/effective context-window pair.
    ///
    /// Written once, before the native session exists. Writing the *same* pair
    /// again is a replay and returns quietly; writing a different one under the
    /// same run is a contradiction, because the record of what a run was
    /// launched under cannot be revised after the fact.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] for a run this project does not own.
    /// * [`RepositoryError::Conflict`] when a different pair is already frozen
    ///   for this run.
    fn record_run_context_policy(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        snapshot: &crate::spec::ContextPolicySnapshot,
    ) -> RepositoryResult<()>;

    /// Read back the frozen pair, re-verified against its own digests.
    ///
    /// # Errors
    /// Refuses stored bytes that no longer match their recorded hashes.
    fn get_run_context_policy(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<Option<crate::spec::ContextPolicySnapshot>>;

    /// Record one compaction attempt.
    ///
    /// Idempotent by receipt id: replaying the identical receipt returns the
    /// stored one, and the same id carrying different content is a conflict.
    /// Because rows are immutable, a late or out-of-order write cannot regress
    /// a receipt that already recorded a terminal outcome.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] for a run this project does not own.
    /// * [`RepositoryError::Conflict`] for a reused id with different content.
    fn record_compaction_receipt(
        &self,
        project_id: ProjectId,
        receipt: &crate::compaction::CompactionReceipt,
    ) -> RepositoryResult<crate::compaction::CompactionReceipt>;

    /// The most recent compaction attempt for one run, if there is one.
    ///
    /// # Errors
    /// Refuses stored bytes that no longer match their recorded digest.
    fn latest_compaction_receipt(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<Option<crate::compaction::CompactionReceipt>>;

    /// Append a raw runtime event, deduplicating replays.
    ///
    /// Returns the cursor of the stored event, or of the existing one when the
    /// event is a duplicate.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project run.
    fn append_runtime_event(&self, request: &NewRuntimeEvent) -> RepositoryResult<EventCursor>;

    /// Append a raw event and reduce it into observed and derived state, in one
    /// transaction and in that order.
    ///
    /// # Errors
    /// Refuses a closed run and a stale revision.
    fn record_observation(&self, request: &NewObservation) -> RepositoryResult<RunProjection>;

    /// Advance a team run's lifecycle under a compare-and-swap.
    ///
    /// # Errors
    /// Refuses a terminal target (closure is a separate, evidence-bearing
    /// operation), an illegal transition, a closed team and a stale revision.
    fn advance_team_run(&self, request: &TeamRunAdvance) -> RepositoryResult<AggregateRevision>;

    /// Close a team run with evidence bound to that team.
    ///
    /// # Errors
    /// Refuses evidence citing another team, an outcome the children do not
    /// compute, an operator receipt claiming anything but `abandoned`, a stale
    /// revision and an already closed team.
    fn close_team_run(&self, request: &TeamRunClosure) -> RepositoryResult<()>;

    /// Close a run with evidence.
    ///
    /// # Errors
    /// Refuses invalid evidence, a stale revision and any attempt to close an
    /// already closed run.
    fn close_agent_run(&self, request: &RunClosure) -> RepositoryResult<()>;

    /// Record the operator decision that authorizes abandoning one run.
    ///
    /// Returns the existing receipt when the key was already used for this exact
    /// decision, so a retry cites the first one rather than minting a second.
    ///
    /// # Errors
    /// Refuses a key already used for a different command, and a run that is not
    /// in this project.
    fn record_abandon_receipt(
        &self,
        request: &NewAbandonReceipt,
    ) -> RepositoryResult<CommandReceiptId>;

    /// Read a run's raw event history from *after* a cursor.
    ///
    /// # Errors
    /// Backend failures only.
    fn read_runtime_events(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        after: Option<EventCursor>,
    ) -> RepositoryResult<Vec<RuntimeEvent>>;
}

/// Inbound source events and intake decisions.
pub trait IntakeRepository {
    /// Commit the canonical identity of one source event, before anything has
    /// evaluated it.
    ///
    /// This is the durability boundary of intake. The event is stored on its
    /// own, so the answer to "did we already see this?" is a database
    /// uniqueness constraint rather than a decision some evaluator may or may
    /// not have reached: `(project, source kind, connection, external id)` and
    /// `(project, connection, envelope digest)` are the two identities, and
    /// SQLite is the concurrency authority for both.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the same source identity is
    /// already stored carrying *different* canonical bytes — upstream changed
    /// what it said under an id it had already used, which is a contradiction a
    /// human has to look at rather than a replay.
    fn ingest_source_event(
        &self,
        project_id: ProjectId,
        event: &CanonicalSourceEvent,
    ) -> RepositoryResult<SourceEventIngest>;

    /// Record the deterministic decision about an already-durable event.
    ///
    /// Replaying the same `(event, trigger revision)` returns the stored
    /// decision instead of writing a second one, and a *different* verdict
    /// under the same revision is a conflict: one pinned revision deciding one
    /// stored event twice, differently, is a contradiction.
    ///
    /// # Errors
    /// Refuses an inconsistent decision, an unknown event, a changed digest and
    /// a contradicting replay.
    fn record_intake_decision(
        &self,
        request: &NewIntakeDecision,
    ) -> RepositoryResult<IntakeOutcome>;

    /// Persist a canonical source event and then its intake decision.
    ///
    /// The composition of [`IntakeRepository::ingest_source_event`] and
    /// [`IntakeRepository::record_intake_decision`], in that order and in two
    /// transactions. The decision is validated *before* either of them, so an
    /// inconsistent receipt stores no event.
    ///
    /// A repeated source identity or a repeated canonical hash on the same
    /// connection returns the original decision and creates no second work
    /// graph.
    ///
    /// # Errors
    /// Refuses an inconsistent decision and a cross-project work graph.
    fn record_source_event(&self, request: &NewSourceEvent) -> RepositoryResult<IntakeOutcome>;

    /// Commit a terminal decision about a proposal, with the work it creates.
    ///
    /// Approval, rejection and bounded auto-arm are all recorded here, and all
    /// three are append-only: the proposal receipt is never rewritten. The
    /// decision row, the created goal and tasks and one lineage row per task
    /// commit together, so work without lineage — or lineage without a decision
    /// — is not a state this method can produce. A replayed decision returns
    /// the stored one and attaches no second graph.
    ///
    /// A bounded auto-arm is re-checked here against the trigger revision the
    /// proposal pinned and the stored execution authorization, through the same
    /// [`crate::spec::TriggerSpec::authorize_auto_arm`] the intake layer used to
    /// decide: skipping the decision layer does not skip the bounds.
    ///
    /// # Errors
    /// Refuses a rejection carrying work, an approval or auto-arm carrying
    /// none, an unknown or already-decided proposal, a proposal that is not
    /// `proposed`, and every [`crate::spec::AutoArmRefusal`].
    fn commit_intake_decision(
        &self,
        request: &NewIntakeDecisionRecord,
    ) -> RepositoryResult<IntakeDecisionRecord>;

    /// Read the terminal decision about one proposal, if it has one.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_intake_decision(
        &self,
        project_id: ProjectId,
        receipt_id: IntakeReceiptId,
    ) -> RepositoryResult<Option<IntakeDecisionRecord>>;

    /// The intake lineage of one task, if intake created it.
    ///
    /// # Errors
    /// Backend failures only.
    fn intake_lineage_of_task(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<IntakeCreatedWork>>;

    /// Re-evaluate an already-stored source event under a newer trigger
    /// revision.
    ///
    /// Inserts **only** a successor receipt linked to the decision it
    /// supersedes. The source event and every earlier receipt are untouched, and
    /// no second work graph is created.
    ///
    /// # Errors
    /// Refuses an older or equal-but-different revision, a missing trigger
    /// revision, a cross-project event and a changed source digest.
    fn reevaluate_source_event(
        &self,
        request: &NewIntakeReevaluation,
    ) -> RepositoryResult<ReevaluationOutcome>;

    /// Find the decision recorded for a source identity.
    ///
    /// # Errors
    /// Backend failures only.
    fn find_intake_receipt(
        &self,
        project_id: ProjectId,
        identity: &SourceIdentity,
    ) -> RepositoryResult<Option<IntakeReceipt>>;

    /// Read one intake decision.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_intake_receipt(
        &self,
        project_id: ProjectId,
        id: IntakeReceiptId,
    ) -> RepositoryResult<Option<IntakeReceipt>>;
}

/// Command intent, outbox and confirmation.
pub trait CommandRepository {
    /// Record intent, desired state, the outbox entry and the intent event in
    /// one transaction.
    ///
    /// Replaying an idempotency key with a byte-identical intent against the
    /// same target returns the original receipt and writes nothing.
    ///
    /// # Errors
    /// Refuses key reuse with a different target or intent.
    fn record_intent(&self, request: &NewCommandIntent) -> RepositoryResult<CommandReceipt>;

    /// Record a synchronous control-plane command: a durable receipt identity
    /// with no outbox entry.
    ///
    /// The receipt is born `intent_persisted` and only its application operation
    /// may confirm it. Writing an outbox row here is what made every successful
    /// local operation look like an undispatched command forever.
    ///
    /// # Errors
    /// Refuses a reused idempotency key and an unknown project.
    fn record_local_command(&self, request: &NewLocalCommand) -> RepositoryResult<CommandReceipt>;

    /// The realm-scoped form, for a local command with no owning project.
    ///
    /// # Errors
    /// Refuses a reused idempotency key.
    fn record_local_command_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewLocalCommand>,
    ) -> RepositoryResult<CommandReceipt>;

    /// Confirm a local command after its application operation returned
    /// successfully.
    ///
    /// `None` means no receipt carries that key, which is a successful no-op:
    /// the caller cannot distinguish "already completed and pruned" from "never
    /// recorded", and neither warrants an error.
    ///
    /// # Errors
    /// Refuses a receipt whose current state cannot legally reach `confirmed`.
    fn complete_local_command(
        &self,
        key: &IdempotencyKey,
        completed_at: Timestamp,
    ) -> RepositoryResult<Option<CommandReceipt>>;

    /// Find a receipt by idempotency key.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_receipt_by_key(&self, key: &IdempotencyKey) -> RepositoryResult<Option<CommandReceipt>>;

    /// Move a receipt forward.
    ///
    /// # Errors
    /// Refuses an illegal transition and a re-dispatch after an unknown result
    /// without proof of no effect.
    fn advance_receipt(&self, request: &ReceiptAdvance) -> RepositoryResult<CommandReceipt>;

    /// Claim outbox entries that are due.
    ///
    /// # Errors
    /// Backend failures only.
    fn claim_outbox(
        &self,
        project_id: ProjectId,
        now: Timestamp,
        limit: u32,
    ) -> RepositoryResult<Vec<CommandOutboxEntry>>;
}

/// External ticket links, projections, observations, comments and conflicts.
pub trait TicketRepository {
    /// Link a task to an external ticket.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project task and a duplicate link.
    fn create_ticket_link(&self, request: &NewTicketLink) -> RepositoryResult<TicketLink>;

    /// Append an immutable projection revision.
    ///
    /// # Errors
    /// Refuses a projection that contradicts its pinned field specification.
    fn insert_projection(
        &self,
        project_id: ProjectId,
        projection: &TicketSyncProjection,
        spec: &TicketFieldSpec,
    ) -> RepositoryResult<()>;

    /// Append an immutable external observation.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project link.
    fn append_observation(
        &self,
        project_id: ProjectId,
        observation: &ExternalTicketObservation,
    ) -> RepositoryResult<()>;

    /// Append an inbound external comment revision.
    ///
    /// Returns `false` when the revision was already stored, so a cursor replay
    /// mirrors nothing twice while an edit is kept as a new revision.
    ///
    /// # Errors
    /// Refuses a revision whose digest does not match its body.
    fn append_comment(
        &self,
        project_id: ProjectId,
        comment: &ExternalCommentRevision,
    ) -> RepositoryResult<bool>;

    /// Record a reconciliation conflict with its inputs.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project link or observation.
    fn insert_conflict(
        &self,
        project_id: ProjectId,
        conflict: &StatusConflict,
    ) -> RepositoryResult<()>;

    /// Mark a conflict resolved, appending evidence and leaving its inputs
    /// untouched.
    ///
    /// # Errors
    /// Refuses resolving an already resolved conflict.
    fn resolve_conflict(
        &self,
        project_id: ProjectId,
        conflict_id: StatusConflictId,
        receipt: CommandReceiptId,
        resolved_at: Timestamp,
    ) -> RepositoryResult<()>;

    /// Record one convergence attempt.
    ///
    /// # Errors
    /// Refuses a receipt with neither a transition nor an assignment.
    fn insert_transition_receipt(
        &self,
        project_id: ProjectId,
        receipt: &StatusTransitionReceipt,
    ) -> RepositoryResult<()>;
}

/// The open-question ledger.
///
/// Deliberately narrow. There is no `update_question`, no `delete_question` and
/// no generic "save the aggregate" here: every mutating operation below appends
/// one immutable child row and moves the head revision, and that is the only
/// shape of change this ledger has. A port with a general update would let a
/// later caller rewrite a round or drop a disposition without the schema ever
/// getting the chance to refuse it.
pub trait OpenQuestionRepository {
    /// Raise a question, storing its header and first round.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project epic, author seat or attachment, and
    /// a question id that already exists.
    fn raise_question(
        &self,
        project_id: ProjectId,
        question: &OpenQuestion,
    ) -> RepositoryResult<()>;

    /// Read one question with its whole append-only history.
    ///
    /// # Errors
    /// Propagates storage failures. A question belonging to another project
    /// resolves to `None` rather than to somebody else's row.
    fn get_question(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
    ) -> RepositoryResult<Option<OpenQuestion>>;

    /// Every question attached to one epic, in creation order.
    ///
    /// # Errors
    /// Propagates storage failures.
    fn list_questions_for_epic(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<OpenQuestion>>;

    /// The identity, subject and derived status of one epic's questions.
    ///
    /// This is the read the completion gate takes immediately before `MarkDone`.
    /// It exists as its own operation because the gate must not be tempted to
    /// reuse a snapshot: a question may be raised, or a deferral's trigger may
    /// fire, during a later completion phase.
    ///
    /// # Errors
    /// Propagates storage failures.
    fn summarize_questions_for_epic(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<OpenQuestionSummary>>;

    /// Append a correcting round, leaving every earlier round untouched.
    ///
    /// Returns the head revision the append wrote.
    ///
    /// # Errors
    /// Refuses a stale `expected` revision, an unknown or cross-project
    /// question, a duplicate ordinal and a predecessor that does not exist.
    fn append_question_round(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
        expected: AggregateRevision,
        round: &AmbiguityRound,
    ) -> RepositoryResult<AggregateRevision>;

    /// Append a disposition.
    ///
    /// One operation records both a first closing and a correction: a
    /// supersede *is* an appended disposition that names the one it replaces.
    /// Splitting them into two operations would imply the second could edit the
    /// first, which is exactly what this ledger does not do.
    ///
    /// Returns the head revision the append wrote.
    ///
    /// # Errors
    /// Refuses a stale `expected` revision, an unknown or cross-project
    /// question, a duplicate ordinal and a superseded ordinal that does not
    /// exist.
    fn append_question_disposition(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
        expected: AggregateRevision,
        disposition: &Disposition,
    ) -> RepositoryResult<AggregateRevision>;

    /// Record that a deferral's named trigger fired, reopening the question.
    ///
    /// The deferred disposition is not deleted or rewritten; the firing stands
    /// alongside it.
    ///
    /// Returns the head revision the append wrote.
    ///
    /// # Errors
    /// Refuses a stale `expected` revision, an unknown or cross-project
    /// question, a firing against a disposition that is not the current
    /// deferral, a trigger the deferral did not name, and a second firing
    /// against one deferral.
    fn fire_deferred_trigger(
        &self,
        project_id: ProjectId,
        question_id: OpenQuestionId,
        expected: AggregateRevision,
        firing: &TriggerFiring,
    ) -> RepositoryResult<AggregateRevision>;
}

/// Calendars, authorizations and overrides.
pub trait CalendarRepository {
    /// Assign a calendar profile revision to a project, retiring any previous
    /// active assignment in the same transaction.
    ///
    /// # Errors
    /// Refuses an unknown profile revision and an invalid window override.
    fn assign_calendar(&self, assignment: &WorkCalendarAssignment) -> RepositoryResult<()>;

    /// Retire a project's active assignment. A project with none is unrestricted,
    /// which is not an error.
    ///
    /// # Errors
    /// Backend failures only.
    fn retire_calendar(
        &self,
        project_id: ProjectId,
        id: WorkCalendarId,
        retired_at: Timestamp,
    ) -> RepositoryResult<()>;

    /// Read a project's active assignment, if it has one.
    ///
    /// # Errors
    /// Backend failures only; no assignment is `Ok(None)`.
    fn active_assignment(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<WorkCalendarAssignment>>;

    /// Append one immutable child-scope window revision.
    fn append_child_windows(&self, revision: &ChildCalendarWindows) -> RepositoryResult<()>;

    /// Read the current window revision for one child scope.
    fn active_child_windows(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
        scope: WorkScope,
    ) -> RepositoryResult<Option<ChildCalendarWindows>>;

    /// Append a calendar exception revision.
    ///
    /// # Errors
    /// Refuses an inverted range and a dangling assignment.
    fn append_exception(&self, exception: &CalendarExceptionRevision) -> RepositoryResult<()>;

    /// List a calendar's exception revisions in recorded order.
    ///
    /// # Errors
    /// Backend failures only.
    fn list_exceptions(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Vec<CalendarExceptionRevision>>;

    /// Insert a holiday source revision.
    ///
    /// # Errors
    /// Refuses an inverted range and an unknown profile revision.
    fn insert_holiday_source(&self, revision: &HolidaySourceRevision) -> RepositoryResult<()>;

    /// Insert an execution authorization.
    ///
    /// # Errors
    /// Refuses unbounded budgets and a cross-project scope.
    fn insert_authorization(&self, authorization: &ExecutionAuthorization) -> RepositoryResult<()>;

    /// Insert a schedule override.
    ///
    /// # Errors
    /// Refuses a missing hard ceiling, an expiry beyond it and unbounded
    /// budgets.
    fn insert_override(&self, schedule_override: &ScheduleOverride) -> RepositoryResult<()>;

    /// Append a revocation to an override.
    ///
    /// # Errors
    /// Refuses revoking an unknown or already revoked override.
    fn revoke_override(
        &self,
        project_id: ProjectId,
        id: ScheduleOverrideId,
        revocation: &OverrideRevocation,
    ) -> RepositoryResult<()>;

    /// Read one override, including its revocation history.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_override(
        &self,
        project_id: ProjectId,
        id: ScheduleOverrideId,
    ) -> RepositoryResult<Option<ScheduleOverride>>;

    /// Read one calendar exception revision.
    ///
    /// # Errors
    /// Backend failures only.
    fn get_exception(
        &self,
        project_id: ProjectId,
        id: CalendarExceptionId,
    ) -> RepositoryResult<Option<CalendarExceptionRevision>>;

    /// Apply one holiday import: the source revision, its provenance and every
    /// normalized exception it produced, in **one** transaction.
    ///
    /// Applying is all-or-nothing on purpose. A half-applied import is a calendar
    /// that closes some of a holiday set and not the rest, which is worse than
    /// one that closes none of it — and a source revision without its exceptions
    /// is provenance for work that never happened.
    ///
    /// Replaying the same `idempotency_key` for the same calendar returns the
    /// original apply and writes nothing.
    ///
    /// # Errors
    /// Refuses an invalid batch or revision, exceptions that do not belong to the
    /// named calendar, and a batch whose superseded revision is not the one
    /// currently applied.
    fn apply_holiday_import(
        &self,
        batch: &HolidayImportBatch,
        revision: &HolidaySourceRevision,
        exceptions: &[CalendarExceptionRevision],
    ) -> RepositoryResult<HolidayImportBatch>;

    /// The import currently applied to one calendar, if one is.
    ///
    /// # Errors
    /// Backend failures only.
    fn applied_import(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Option<HolidayImportBatch>>;

    /// The exception revisions resolution is allowed to read: every manual one,
    /// plus the imported ones belonging to the currently applied import.
    ///
    /// Superseded imports stay in the table as history and are simply not
    /// returned here, which is how a refreshed import drops the holidays its
    /// source no longer lists without deleting evidence that it once did.
    ///
    /// # Errors
    /// Backend failures only.
    fn applied_exceptions(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Vec<CalendarExceptionRevision>>;
}

// ---------------------------------------------------------------------------
// Realm ingress
// ---------------------------------------------------------------------------

/// The Realm-qualified ingress every cross-boundary write and read goes through.
///
/// These are the methods a transport, cache or import path must use. Each one
/// proves the envelope's Realm **before** a transaction opens, so a value from
/// another Realm is refused without touching a single row. An id from another
/// Realm smuggled under the local Realm id still fails, because the row it names
/// is simply absent from this database — there is no fallback lookup and no
/// cross-database attach.
pub trait RealmRepository {
    /// The Realm this store is bound to, for the lifetime of the store.
    fn realm(&self) -> RealmId;

    /// Prove an incoming Realm id matches this store.
    ///
    /// # Errors
    /// Returns [`DomainError::RealmMismatch`] naming both ids and nothing else.
    fn ensure_realm(&self, found: RealmId) -> RepositoryResult<()> {
        crate::realm::ensure_realm(self.realm(), found)?;
        Ok(())
    }

    /// Record a command intent carried in a Realm-qualified envelope.
    ///
    /// # Errors
    /// Refuses a foreign Realm before any write, then as `record_intent`.
    fn record_intent_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewCommandIntent>,
    ) -> RepositoryResult<CommandReceipt>;

    /// Reduce an observation carried in a Realm-qualified envelope.
    ///
    /// # Errors
    /// Refuses a foreign Realm before any write, then as `record_observation`.
    fn record_observation_in_realm(
        &self,
        envelope: &EventEnvelope<NewObservation>,
    ) -> RepositoryResult<RunProjection>;

    /// Ingest a source event carried in a Realm-qualified envelope.
    ///
    /// # Errors
    /// Refuses a foreign Realm before any write, then as `record_source_event`.
    fn record_source_event_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewSourceEvent>,
    ) -> RepositoryResult<IntakeOutcome>;

    /// Re-evaluate a source event carried in a Realm-qualified envelope.
    ///
    /// Re-evaluation is an ingress path in its own right: it accepts a source
    /// event id and a digest from outside, so it needs the same Realm proof as
    /// the initial intake. Without it, a re-evaluation minted in Realm A could
    /// be replayed into Realm B and either resolve nothing or, worse, collide
    /// with a locally valid id.
    ///
    /// # Errors
    /// Refuses a foreign Realm before any write, then as
    /// `reevaluate_source_event`.
    fn reevaluate_source_event_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewIntakeReevaluation>,
    ) -> RepositoryResult<ReevaluationOutcome>;

    /// Import or replay a command receipt carried in a Realm-qualified envelope.
    ///
    /// A receipt is the one value that travels furthest from the store that
    /// minted it — through an outbox, a dispatcher and back — so replaying one
    /// is exactly where a cross-Realm mix-up would surface.
    ///
    /// # Errors
    /// Refuses a foreign Realm before any read, then
    /// [`RepositoryError::NotFound`] when no such receipt exists in this Realm,
    /// or [`DomainError::Invalid`] when the stored receipt differs from the one
    /// presented.
    fn import_receipt_in_realm(
        &self,
        envelope: &ReceiptEnvelope<CommandReceipt>,
    ) -> RepositoryResult<CommandReceipt>;

    /// Resume a run's event stream strictly after a Realm-qualified cursor.
    ///
    /// # Errors
    /// Refuses a cursor from another Realm before reading.
    fn read_events_after(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        after: Option<RealmCursor>,
    ) -> RepositoryResult<Vec<EventEnvelope<RuntimeEvent>>>;

    /// Take a Realm-qualified snapshot of one agent run.
    ///
    /// The snapshot carries the cursor it is consistent with, so a subscriber
    /// resumes strictly after it without a gap or a duplicate.
    ///
    /// # Errors
    /// Backend failures only.
    fn snapshot_agent_run(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AgentRun>>>;

    /// Take a Realm-qualified snapshot of one account profile.
    ///
    /// The carried value is the non-secret stored record, which is the same
    /// thing a later API, doctor or export projection is built from — there is
    /// no second, richer shape that a cross-boundary reader could ask for.
    ///
    /// # Errors
    /// Backend failures only.
    fn snapshot_account_profile(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AccountProfile>>>;

    /// Apply a profile change carried in a Realm-qualified envelope.
    ///
    /// # Errors
    /// Refuses a foreign Realm before any write, then as
    /// `update_account_profile`.
    fn update_account_profile_in_realm(
        &self,
        envelope: &ReceiptEnvelope<AccountProfileUpdate>,
    ) -> RepositoryResult<AccountProfile>;

    /// Read one page of this Realm's whole control-plane log, strictly after a
    /// Realm-qualified cursor.
    ///
    /// This is the Realm-wide companion to `read_events_after`, which pages one
    /// run. A durable subscriber follows the Realm — it has no run to name before
    /// it has read the events that mention one — and it needs the retained window
    /// alongside the page to know whether the position it asked for still exists.
    ///
    /// # Errors
    /// Refuses a cursor from another Realm before reading, and
    /// [`DomainError::Invalid`] for a page limit of zero.
    fn realm_event_page(
        &self,
        after: Option<RealmCursor>,
        limit: u32,
    ) -> RepositoryResult<RealmEventPage>;

    /// Take a Realm-qualified inspection snapshot of one agent run.
    ///
    /// The run is addressed by its own id rather than by `(project, run)`,
    /// because a session is addressed that way at every boundary above this one
    /// and a Realm *is* the isolation boundary — a run id from another Realm has
    /// no row here. The resolved [`ProjectId`] travels in the answer, so every
    /// later read is project-scoped as usual.
    ///
    /// # Errors
    /// Backend failures only.
    fn snapshot_run_inspection(
        &self,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<RunInspection>>>;

    /// Take a Realm-qualified inspection snapshot of one task.
    ///
    /// # Errors
    /// Backend failures only; a task from another project is `Ok(None)`.
    fn snapshot_task_inspection(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<TaskInspection>>>;

    /// The revision one command target currently stands at, or `None` when the
    /// target does not exist in this project.
    ///
    /// A caller that presents a stale revision is owed the current one, and it
    /// has to be readable for *every* target kind — otherwise a conflict on one
    /// aggregate would answer with a number and a conflict on another with a
    /// shrug. The compare-and-swap inside the write is still what makes the
    /// refusal safe; this only makes it informative.
    ///
    /// # Errors
    /// Backend failures only.
    fn snapshot_target_revision(
        &self,
        project_id: ProjectId,
        target: &AggregateRef,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AggregateRevision>>>;
}
