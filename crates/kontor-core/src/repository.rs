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
    CalendarExceptionRevision, CalendarProfileSpec, ExecutionAuthorization, HolidaySourceRevision,
    OverrideRevocation, ScheduleOverride, WorkCalendarAssignment,
};
use crate::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CalendarExceptionId,
    CalendarProfileId, CanonicalDocument, CommandReceiptId, ConnectorKey, ContentHash,
    CredentialAlias, EventCursor, ExternalId, ExternalIssueTypeKey, ExternalName,
    ExternalProjectKey, GateKey, IdempotencyKey, IntakeReceiptId, MiniProjectId, ModuleKey,
    PersonaScenarioId, PhaseKey, ProjectId, RealmId, RoleKey, RuntimeBindingId, RuntimeKindKey,
    ScheduleOverrideId, SourceEventId, SpecVersion, StatusConflictId, TaskId, TaskWorkflowId,
    TeamRunId, TeamTemplateId, TicketLinkId, Timestamp, TriggerKey, WorkCalendarId, WorkProfileKey,
};
use crate::realm::{EventEnvelope, RealmCursor, ReceiptEnvelope, SnapshotEnvelope};
use crate::receipt::{
    AggregateRef, CommandKind, CommandOutboxEntry, CommandReceipt, CommandReceiptState,
    NoEffectEvidence,
};
use crate::spec::{
    CanonicalSourceEvent, IntakeReceipt, PersonaScenarioSnapshot, PersonaScenarioSpec,
    ResolvedWorkProfileSnapshot, SourceIdentity, TeamRunSnapshot, TeamTemplateRevision,
    TriggerSpec, WorkProfileSpec,
};
use crate::state::{
    DesiredRunState, GateState, GateVerdict, NativeRuntimeIdentity, RunLifecycle, RunProjection,
    TaskState, TeamTerminalEvidence, TerminalEvidence, TerminalOutcome,
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
    /// When it was recorded.
    pub recorded_at: Timestamp,
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
    /// How the task's run closed, for a failure transition.
    pub run_outcome: Option<TerminalOutcome>,
    /// Artifacts produced, for a completion transition.
    pub produced_artifacts: BTreeSet<ArtifactKey>,
    /// Phases recorded complete, for a completion transition.
    pub completed_phases: BTreeSet<PhaseKey>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSourceEvent {
    /// Owning project.
    pub project_id: ProjectId,
    /// The canonical event.
    pub event: CanonicalSourceEvent,
    /// The decision. Written in the same transaction as the event.
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
    /// receipt and a completion without profile closure.
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
    /// Persist a canonical source event and its intake decision atomically.
    ///
    /// A repeated source identity or a repeated canonical hash on the same
    /// connection returns the original decision and creates no second work
    /// graph.
    ///
    /// # Errors
    /// Refuses an inconsistent decision and a cross-project work graph.
    fn record_source_event(&self, request: &NewSourceEvent) -> RepositoryResult<IntakeOutcome>;

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
}
