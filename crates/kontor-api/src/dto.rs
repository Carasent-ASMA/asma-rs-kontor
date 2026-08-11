//! The shapes that cross the HTTP boundary.
//!
//! Two rules decide what may appear in this module, and both are structural
//! rather than conventional:
//!
//! 1. **Realm-qualified.** Every successful body carries the Realm it came from,
//!    either as a field or by being one of `kontor-core`'s own envelopes. A bare
//!    id, cursor or receipt never leaves this process.
//! 2. **Nothing the daemon holds in confidence.** There is no field here for a
//!    bearer token, a runtime endpoint, a `CODEX_HOME`, a credential value or an
//!    adapter's client configuration. A DTO cannot leak what it has nowhere to
//!    put.
//!
//! Session *content* is different from session *state*, and this is the one place
//! the distinction is visible: a timeline item carries the runtime's own payload,
//! because reading a transcript is what an operator console is for. That payload
//! is never persisted — see `kontor-store`'s control-metadata allowlist.

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, EventCursor, ExternalId,
    ExternalName, GateKey, PhaseKey, ProjectId, RealmId, RoleKey, RuntimeBindingId, RuntimeKindKey,
    SchemaVersion, SpecVersion, TaskId, TeamRunId, TeamTemplateId, Timestamp, WorkProfileKey,
};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::receipt::{AggregateRef, CommandKind, CommandReceipt, CommandReceiptState};
use kontor_core::repository::{
    HistoryGapKind, HistoryGapMarker, RunInspection, RuntimeEvent, TaskInspection,
};
use kontor_core::state::{
    DesiredRunState, Freshness, GateState, ObservedRunState, RunLifecycle, TaskState,
    TerminalOutcome,
};
use kontor_runtime::adapter::{MessageAck, PermissionAck};
use kontor_runtime::request::PermissionDecision;
use kontor_runtime::timeline::{
    EventSubject, HistoryCursor, HistoryPage, SessionEvent, SessionEventKind, TimelinePosition,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::BarrierState;

/// Liveness, identity and how far startup has got.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HealthDto {
    /// The Realm answering.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// Always true: reaching a handler is what liveness means.
    pub live: bool,
    /// The persisted schema generation.
    pub schema_version: i64,
    /// Whether startup reconciliation finished, and therefore whether scheduling
    /// is open.
    pub reconciliation: BarrierState,
    /// Whether work may be dispatched right now.
    pub scheduling_open: bool,
    /// The runtime families this daemon was configured with.
    #[schema(value_type = Vec<String>)]
    pub runtimes: Vec<RuntimeKindKey>,
}

/// This Realm's immutable identity.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RealmDto {
    /// The Realm.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// The envelope contract it was created under.
    #[schema(value_type = u32)]
    pub schema_version: SchemaVersion,
    /// When it was created.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    /// Its optional non-secret label.
    #[schema(value_type = Option<String>)]
    pub display_label: Option<ExternalName>,
}

/// A recorded discontinuity a reader is owed.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GapDto {
    /// Which cursor space the hole is in.
    #[schema(value_type = String)]
    pub kind: HistoryGapKind,
    /// The runtime's content epoch, for a content gap.
    pub content_epoch: Option<u64>,
    /// The sequence that was expected next.
    pub expected_sequence: u64,
    /// The sequence that actually arrived.
    pub received_sequence: u64,
    /// The control-plane position the hole was noticed at.
    #[schema(value_type = i64)]
    pub detected_cursor: EventCursor,
    /// When it was noticed.
    #[schema(value_type = String, format = DateTime)]
    pub detected_at: Timestamp,
}

impl From<&HistoryGapMarker> for GapDto {
    fn from(marker: &HistoryGapMarker) -> Self {
        Self {
            kind: marker.kind,
            content_epoch: marker.content_epoch,
            expected_sequence: marker.expected_sequence,
            received_sequence: marker.received_sequence,
            detected_cursor: marker.detected_cursor,
            detected_at: marker.detected_at,
        }
    }
}

/// The native session one run is bound to, as evidence and never as identity.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BindingDto {
    /// The Kontor binding.
    #[schema(value_type = String)]
    pub binding_id: RuntimeBindingId,
    /// The runtime family. Never an endpoint.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The host label the runtime reports for itself.
    #[schema(value_type = String)]
    pub host: ExternalName,
    /// The runtime generation the session belongs to.
    pub generation: u64,
    /// The runtime's own session id. Correlation evidence.
    #[schema(value_type = String)]
    pub native_id: ExternalId,
    /// When it was bound.
    #[schema(value_type = String, format = DateTime)]
    pub bound_at: Timestamp,
    /// Whether this process still holds the frozen capability snapshot for it,
    /// and can therefore address the session at all.
    pub attached: bool,
}

/// The orthogonal state of one run, plus how old its newest confirmation is.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProjectionDto {
    /// The run's own lifecycle.
    #[schema(value_type = String)]
    pub lifecycle: RunLifecycle,
    /// What Kontor asked for.
    #[schema(value_type = String)]
    pub desired: DesiredRunState,
    /// What the runtime last reported.
    #[schema(value_type = String)]
    pub observed: ObservedRunState,
    /// What Kontor concluded. `terminal` carries its outcome separately.
    pub derived: String,
    /// The outcome, when the run is closed.
    #[schema(value_type = Option<String>)]
    pub outcome: Option<TerminalOutcome>,
    /// When the newest trusted confirmation arrived.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_confirmed_at: Option<Timestamp>,
    /// How old that confirmation is, judged against the daemon's evidence window
    /// at the instant this snapshot was taken.
    #[schema(value_type = String)]
    pub freshness: Freshness,
    /// The control-plane position of the newest reduced event.
    #[schema(value_type = Option<i64>)]
    pub last_cursor: Option<EventCursor>,
}

/// One agent run, as a cross-boundary reader sees it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunDto {
    /// The run.
    #[schema(value_type = String)]
    pub agent_run_id: AgentRunId,
    /// The project it belongs to.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The team run it serves.
    #[schema(value_type = String)]
    pub team_run_id: TeamRunId,
    /// The run it succeeds, for recovery and resume.
    #[schema(value_type = Option<String>)]
    pub parent_agent_run_id: Option<AgentRunId>,
    /// The role slot it fills.
    #[schema(value_type = String)]
    pub role: RoleKey,
    /// The coding account it is pinned to, if any.
    #[schema(value_type = Option<String>)]
    pub account_profile_id: Option<AccountProfileId>,
    /// The native binding, once launched.
    pub binding: Option<BindingDto>,
    /// The orthogonal state.
    pub projection: ProjectionDto,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The team template revision this run's team pinned.
    pub applied: AppliedRevisionsDto,
    /// Every recorded discontinuity, oldest first.
    pub gaps: Vec<GapDto>,
    /// When it was created.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    /// When it closed.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub closed_at: Option<Timestamp>,
}

/// Which pinned specification revisions an aggregate is running under.
///
/// Every one of these is a *frozen* revision copied into the aggregate, not a
/// pointer to whatever the current definition happens to be. That is what makes
/// a snapshot re-checkable rather than merely re-readable.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct AppliedRevisionsDto {
    /// The work profile the task's active workflow pinned.
    #[schema(value_type = Option<String>)]
    pub work_profile: Option<WorkProfileKey>,
    /// That profile's revision.
    #[schema(value_type = Option<u32>)]
    pub work_profile_version: Option<SpecVersion>,
    /// The team template the run's team pinned.
    #[schema(value_type = Option<String>)]
    pub team_template: Option<TeamTemplateId>,
    /// That template's revision.
    #[schema(value_type = Option<u32>)]
    pub team_template_version: Option<SpecVersion>,
    /// The persona scenario frozen onto the task, if any.
    #[schema(value_type = Option<String>)]
    pub persona_scenario: Option<String>,
    /// That scenario's revision.
    #[schema(value_type = Option<u32>)]
    pub persona_scenario_version: Option<SpecVersion>,
}

/// One task, its active workflow and the gates reduced from its evaluations.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The project it belongs to.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// Its title.
    #[schema(value_type = String)]
    pub title: ExternalName,
    /// Its lifecycle state.
    #[schema(value_type = String)]
    pub state: TaskState,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The phase the active workflow is in.
    #[schema(value_type = Option<String>)]
    pub current_phase: Option<PhaseKey>,
    /// The gate states, keyed by gate.
    #[schema(value_type = Object)]
    pub gates: std::collections::BTreeMap<GateKey, GateState>,
    /// The pinned specification revisions in force.
    pub applied: AppliedRevisionsDto,
    /// When it last changed.
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: Timestamp,
}

/// A point-in-time value and the position it is consistent with.
///
/// It is the wire spelling of `kontor-core`'s own `SnapshotEnvelope`: a
/// subscriber resumes strictly after `snapshot_cursor`, in the same Realm.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SnapshotDto<T> {
    /// The Realm this snapshot came from.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// The position it is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: EventCursor,
    /// The value.
    pub value: T,
}

/// One durable control-plane event.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventDto {
    /// The Realm it was recorded in.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// Its control-plane position.
    #[schema(value_type = i64)]
    pub cursor: EventCursor,
    /// The project that owns it.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The run it concerns.
    #[schema(value_type = String)]
    pub agent_run_id: AgentRunId,
    /// The runtime family that reported it.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The runtime generation it belongs to.
    pub generation: u64,
    /// The runtime's own session id.
    #[schema(value_type = String)]
    pub native_id: ExternalId,
    /// The runtime's own event id, when it provides one.
    #[schema(value_type = Option<String>)]
    pub native_event_id: Option<ExternalId>,
    /// The runtime's own ordering.
    pub native_sequence: u64,
    /// The stored control metadata. Held to the store's positive allowlist, so it
    /// is facts about the session and never the session's content.
    #[schema(value_type = Object)]
    pub payload: CanonicalDocument,
    /// When the runtime emitted it.
    #[schema(value_type = String, format = DateTime)]
    pub observed_at: Timestamp,
    /// When this Realm stored it.
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: Timestamp,
}

impl EventDto {
    /// Wrap one stored event for delivery.
    #[must_use]
    pub fn of(realm_id: RealmId, event: &RuntimeEvent) -> Self {
        Self {
            realm_id,
            cursor: event.cursor,
            project_id: event.project_id,
            agent_run_id: event.agent_run_id,
            runtime_kind: event.identity.runtime_kind.clone(),
            generation: event.identity.generation,
            native_id: event.identity.native_id.clone(),
            native_event_id: event.native_event_id.clone(),
            native_sequence: event.native_sequence,
            payload: event.payload.clone(),
            observed_at: event.observed_at,
            recorded_at: event.recorded_at,
        }
    }
}

/// What a command asks for, as a caller states it.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CommandRequest {
    /// The acting project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The aggregate the command targets.
    #[schema(value_type = Object)]
    pub target: AggregateRef,
    /// The revision the caller computed the intent against.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The desired run state, for the commands that carry one.
    #[schema(value_type = Option<String>)]
    pub desired_state: Option<DesiredRunState>,
    /// The canonical intent document. Must carry a `schema_version`.
    #[schema(value_type = Object)]
    pub intent: serde_json::Value,
    /// The canonical dispatch payload. Must carry a `schema_version`.
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
}

/// The durable record of one command.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReceiptDto {
    /// The receipt.
    #[schema(value_type = String)]
    pub receipt_id: String,
    /// The project that owns it.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The caller's idempotency key.
    pub idempotency_key: String,
    /// What was asked for.
    #[schema(value_type = String)]
    pub kind: CommandKind,
    /// Which aggregate it targets.
    #[schema(value_type = Object)]
    pub target: AggregateRef,
    /// The revision the intent was computed against.
    #[schema(value_type = u64)]
    pub target_revision: AggregateRevision,
    /// How far it has got. An acknowledgement is never a completion.
    #[schema(value_type = String)]
    pub state: CommandReceiptState,
    /// How many dispatch attempts have been made.
    pub attempts: u32,
    /// When the intent was recorded.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    /// When the receipt last changed.
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: Timestamp,
}

impl From<&CommandReceipt> for ReceiptDto {
    /// The intent document, the correlation token and the native identity are
    /// deliberately absent: an intent can carry operator prose, and a correlation
    /// is the dispatcher's private handle on a foreign system.
    fn from(receipt: &CommandReceipt) -> Self {
        Self {
            receipt_id: receipt.id.to_string(),
            project_id: receipt.project_id,
            idempotency_key: receipt.idempotency_key.as_str().to_owned(),
            kind: receipt.kind,
            target: receipt.target,
            target_revision: receipt.target_revision,
            state: receipt.state,
            attempts: receipt.attempts,
            created_at: receipt.created_at,
            updated_at: receipt.updated_at,
        }
    }
}

/// A receipt, and whether this call is the one that recorded it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReceiptResponse {
    /// The Realm-qualified receipt envelope.
    #[serde(flatten)]
    #[schema(value_type = Object)]
    pub envelope: ReceiptEnvelope<ReceiptDto>,
    /// `true` when the idempotency key had already recorded this exact command,
    /// so nothing was written and the original receipt is being returned.
    pub replayed: bool,
}

/// One item of session content.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TimelineItemDto {
    /// What kind of thing happened.
    #[schema(value_type = String)]
    pub kind: SessionEventKind,
    /// Its position inside the epoch.
    pub epoch: u64,
    /// Its position inside that epoch.
    pub sequence: u64,
    /// The runtime's permission request id, when the item is about one.
    #[schema(value_type = Option<String>)]
    pub permission_id: Option<ExternalId>,
    /// The Kontor message id, when the item is about one.
    pub message_id: Option<String>,
    /// The runtime's own event id, when it provides one.
    #[schema(value_type = Option<String>)]
    pub native_event_id: Option<ExternalId>,
    /// When the runtime emitted it.
    #[schema(value_type = String, format = DateTime)]
    pub emitted_at: Timestamp,
    /// The runtime's own payload. Never persisted by this control plane.
    #[schema(value_type = Object)]
    pub payload: CanonicalDocument,
}

impl From<&SessionEvent> for TimelineItemDto {
    fn from(event: &SessionEvent) -> Self {
        let (permission_id, message_id) = match &event.subject {
            EventSubject::None => (None, None),
            EventSubject::Permission(id) => (Some(id.clone()), None),
            EventSubject::Message(id) => (None, Some(id.to_string())),
        };
        Self {
            kind: event.kind,
            epoch: event.position.epoch,
            sequence: event.position.sequence,
            permission_id,
            message_id,
            native_event_id: event.native_event_id.clone(),
            emitted_at: event.emitted_at,
            payload: event.payload.clone(),
        }
    }
}

/// One page of a session's recorded content.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TimelineDto {
    /// The Realm the session belongs to.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// The run whose session was read.
    #[schema(value_type = String)]
    pub agent_run_id: AgentRunId,
    /// The runtime's content epoch every item belongs to.
    pub epoch: u64,
    /// The items, in ascending sequence order.
    pub items: Vec<TimelineItemDto>,
    /// Where to continue, or `null` when the history is exhausted.
    pub next: Option<String>,
    /// The last position this page covers. A live subscription must start
    /// strictly after it.
    pub end_epoch: u64,
    /// The sequence half of that position.
    pub end_sequence: u64,
    /// The cursor a live subscription resumes from. The same position as
    /// `end_epoch`/`end_sequence`, in the form `/stream` accepts.
    pub anchor: String,
}

impl TimelineDto {
    /// Wrap one validated page.
    #[must_use]
    pub fn of(
        realm_id: RealmId,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
        page: &HistoryPage,
        end: TimelinePosition,
    ) -> Self {
        Self {
            realm_id,
            agent_run_id,
            epoch: page.epoch,
            items: page.items.iter().map(TimelineItemDto::from).collect(),
            next: page.next.as_ref().map(|cursor| cursor.as_str().to_owned()),
            end_epoch: end.epoch,
            end_sequence: end.sequence,
            anchor: HistoryCursor::issue(binding_id, end).as_str().to_owned(),
        }
    }
}

/// What a caller sends into a session.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct MessageRequest {
    /// The message body.
    pub body: String,
}

/// The runtime's answer to one delivered message.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MessageAckDto {
    /// The Kontor identifier the message was sent under. It is the
    /// `Idempotency-Key` the caller presented.
    pub message_id: String,
    /// The binding it was delivered into.
    #[schema(value_type = String)]
    pub binding_id: RuntimeBindingId,
    /// The content epoch it landed in.
    pub epoch: u64,
    /// Where it landed inside that epoch.
    pub sequence: u64,
    /// When the runtime accepted it.
    #[schema(value_type = String, format = DateTime)]
    pub accepted_at: Timestamp,
}

impl From<&MessageAck> for MessageAckDto {
    fn from(ack: &MessageAck) -> Self {
        Self {
            message_id: ack.message_id.to_string(),
            binding_id: ack.binding_id,
            epoch: ack.position.epoch,
            sequence: ack.position.sequence,
            accepted_at: ack.accepted_at,
        }
    }
}

/// How a permission request is answered.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PermissionRequestBody {
    /// The answer.
    #[schema(value_type = String)]
    pub decision: PermissionDecision,
}

/// The runtime's answer to one permission response.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PermissionAckDto {
    /// The runtime's own identifier for the request that was answered.
    #[schema(value_type = String)]
    pub permission_id: ExternalId,
    /// The Kontor identifier the answer was sent under.
    pub response_id: String,
    /// The binding whose session raised the request.
    #[schema(value_type = String)]
    pub binding_id: RuntimeBindingId,
    /// The answer that was applied.
    #[schema(value_type = String)]
    pub decision: PermissionDecision,
    /// The content epoch the resolution landed in.
    pub epoch: u64,
    /// Where it landed inside that epoch.
    pub sequence: u64,
    /// When the runtime accepted it.
    #[schema(value_type = String, format = DateTime)]
    pub accepted_at: Timestamp,
}

impl From<&PermissionAck> for PermissionAckDto {
    fn from(ack: &PermissionAck) -> Self {
        Self {
            permission_id: ack.permission_id.clone(),
            response_id: ack.response_id.to_string(),
            binding_id: ack.binding_id,
            decision: ack.decision,
            epoch: ack.position.epoch,
            sequence: ack.position.sequence,
            accepted_at: ack.accepted_at,
        }
    }
}

/// One frame of live session content.
///
/// The item is wrapped rather than sent bare so that *every* frame of the stream
/// carries the Realm and the run it belongs to — a console holding two sessions
/// open must never have to infer which one a frame came from.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StreamFrameDto {
    /// The Realm the session belongs to.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// The run whose session produced it.
    #[schema(value_type = String)]
    pub agent_run_id: AgentRunId,
    /// The content item.
    pub item: TimelineItemDto,
}

/// A frame the live-content stream emits instead of an item when the timeline can
/// no longer be followed.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StreamRefusalDto {
    /// The Realm the session belongs to.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// Always `timeline_refetch_required`.
    pub code: &'static str,
    /// A static description of what broke.
    pub rule: &'static str,
}

/// Build the applied-revision view of one run.
#[must_use]
pub fn run_revisions(inspection: &RunInspection) -> AppliedRevisionsDto {
    AppliedRevisionsDto {
        team_template: inspection.team_template.map(|(id, _)| id),
        team_template_version: inspection.team_template.map(|(_, version)| version),
        ..AppliedRevisionsDto::default()
    }
}

/// Build the applied-revision view of one task.
#[must_use]
pub fn task_revisions(inspection: &TaskInspection) -> AppliedRevisionsDto {
    let profile = inspection
        .workflow
        .as_ref()
        .map(|workflow| &workflow.snapshot.definition);
    let persona = inspection
        .persona
        .as_ref()
        .map(|snapshot| &snapshot.definition);
    AppliedRevisionsDto {
        work_profile: profile.map(|definition| definition.id.clone()),
        work_profile_version: profile.map(|definition| definition.version),
        persona_scenario: persona.map(|definition| definition.scenario_id.to_string()),
        persona_scenario_version: persona.map(|definition| definition.version),
        ..AppliedRevisionsDto::default()
    }
}
