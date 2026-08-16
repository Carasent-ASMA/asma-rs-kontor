//! Typed requests, and the two Kontor-owned identifiers a runtime never gets to
//! mint.
//!
//! Every request names the work with Kontor identifiers from `kontor-core`. A
//! native identifier appears only inside [`NativeRuntimeIdentity`], which is
//! correlation evidence — it is never accepted in a field that means "which run
//! is this", "which binding is this" or "which message is this".

use std::collections::BTreeSet;
use std::fmt;

use kontor_core::compaction::{
    CompactionReceipt, CompactionStatus, CompactionTelemetry, CompactionTrigger,
};
use kontor_core::id::{
    AccountProfileId, AgentRunId, BoundedText, CanonicalDocument, CompactionReceiptId, ContentHash,
    ExternalId, RoleSlotId, RuntimeBindingId, TaskId, TeamRunId, Timestamp,
};
use kontor_core::spec::{ContextEnforcement, ContextPolicySnapshot, ModelRung};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adapter::{RuntimeError, RuntimeResult};
use crate::admission::{LaunchAuthority, RoleSlotKey};
use crate::capability::{RuntimeBindingSnapshot, RuntimeCapabilities};
use crate::container::{ContainerBindingSnapshot, ContainerClaim};
use crate::timeline::{HistoryCursor, SessionEventKind, TimelinePosition};
use crate::workspace::{WorkspaceBindingSnapshot, WorkspaceClaim, WorkspaceRoot};

/// The prefix every Kontor correlation label carries.
pub const CORRELATION_PREFIX: &str = "kontor-run-";

/// Parse one Kontor-minted identifier in its canonical text form.
///
/// The rule is deliberately the same as `kontor-core`'s entity ids: lowercase
/// hyphenated UUID v7 and nothing else. It is what makes a native id fail to
/// parse into a Kontor identifier instead of being accepted as one.
pub(crate) fn parse_kontor_uuid(subject: &'static str, text: &str) -> DomainResult<Uuid> {
    let uuid = Uuid::try_parse(text)
        .map_err(|_| DomainError::invalid(subject, "not a UUID in canonical text form"))?;
    if uuid.get_version_num() != 7 {
        return Err(DomainError::invalid(subject, "not a version 7 UUID"));
    }
    if uuid.as_hyphenated().to_string() != text {
        return Err(DomainError::invalid(
            subject,
            "not lowercase hyphenated canonical form",
        ));
    }
    Ok(uuid)
}

/// A label Kontor plants in a runtime so a native session can be tied back to
/// the run that asked for it.
///
/// The label *is* an [`AgentRunId`] by construction, so a native session id can
/// never be parsed into one. That is the structural half of "native ids never
/// replace Kontor ids"; [`crate::observation::CorrelationEvidence::establish`]
/// is the behavioral half.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct CorrelationLabel(AgentRunId);

impl CorrelationLabel {
    /// The label for one agent run.
    #[must_use]
    pub const fn for_run(agent_run_id: AgentRunId) -> Self {
        Self(agent_run_id)
    }

    /// Parse a label a runtime reported back.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the text is not the Kontor prefix
    /// followed by a canonical [`AgentRunId`]. A native session id fails here.
    pub fn parse(text: &str) -> DomainResult<Self> {
        let tail = text.strip_prefix(CORRELATION_PREFIX).ok_or_else(|| {
            DomainError::invalid("CorrelationLabel", "does not carry the Kontor run prefix")
        })?;
        AgentRunId::parse(tail).map(Self)
    }

    /// The run this label names.
    #[must_use]
    pub const fn agent_run_id(self) -> AgentRunId {
        self.0
    }
}

impl fmt::Display for CorrelationLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{CORRELATION_PREFIX}{}", self.0)
    }
}

/// A Kontor-generated message identity, used as the idempotency key for
/// everything a caller pushes into a session.
///
/// Both a session message and a permission response carry one, so a lost
/// acknowledgement is answered from the ledger instead of by repeating the
/// effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Generate a fresh, time-ordered identifier.
    #[must_use]
    pub fn generate() -> Self {
        Self(kontor_core::id::generate_uuid_v7())
    }

    /// Parse the canonical lowercase hyphenated text form.
    ///
    /// # Errors
    /// Rejects any non-canonical spelling and any UUID that is not version 7,
    /// which is what stops a native session id from being read as a message id.
    pub fn parse(text: &str) -> DomainResult<Self> {
        parse_kontor_uuid("MessageId", text).map(Self)
    }

    /// Borrow the underlying UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.as_hyphenated(), f)
    }
}

/// The verified native place a launch presents, and how it was keyed.
///
/// Exactly one of these travels with a launch, which is the point of making it
/// an enum rather than two optional fields: a launch cannot present both, and
/// "which key was this place found by" is answered by reading the value rather
/// than by inspecting which field happened to be filled in.
///
/// [`Self::Container`] is what an Operational placement uses. The place is
/// keyed by [`kontor_core::id::TopologyNodeId`], so a second delivery attempt at
/// the same ticket resolves to the same container and a container no ticket owns
/// is still addressable. [`Self::Workspace`] is the older TeamRun-keyed task
/// workspace, kept for runtimes and fixtures that have no topology to place
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchPlacement {
    /// A TeamRun-keyed task workspace.
    Workspace(WorkspaceBindingSnapshot),
    /// A topology-node-keyed native container.
    Container(ContainerBindingSnapshot),
}

impl LaunchPlacement {
    /// The container binding, when this placement is one.
    #[must_use]
    pub const fn container(&self) -> Option<&ContainerBindingSnapshot> {
        match self {
            Self::Container(snapshot) => Some(snapshot),
            Self::Workspace(_) => None,
        }
    }

    /// The task workspace binding, when this placement is one.
    #[must_use]
    pub const fn workspace(&self) -> Option<&WorkspaceBindingSnapshot> {
        match self {
            Self::Workspace(snapshot) => Some(snapshot),
            Self::Container(_) => None,
        }
    }
}

/// What a launch claims about the verified place it will work in.
///
/// One claim covers both keyings so a caller cannot present a coherent value of
/// the wrong kind and have it go unchecked: an absent placement is refused here
/// rather than treated as "nothing to verify".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementClaim<'a> {
    /// The verified placement, when the caller has one.
    pub placement: Option<&'a LaunchPlacement>,
    /// The team run the role belongs to.
    pub team_run_id: TeamRunId,
    /// The task the role serves.
    pub task_id: TaskId,
    /// Where the role says it will work.
    pub cwd: &'a WorkspaceRoot,
}

impl PlacementClaim<'_> {
    /// Verify the claim before anything can be edited.
    ///
    /// # Errors
    /// [`RuntimeError::WorkspaceBindingRequired`] when nothing was presented,
    /// otherwise whatever the presented placement's own claim refuses.
    pub fn verify(&self, current_generation: Option<u64>) -> RuntimeResult<()> {
        match self.placement {
            None => Err(RuntimeError::WorkspaceBindingRequired),
            Some(LaunchPlacement::Workspace(snapshot)) => WorkspaceClaim {
                binding: Some(snapshot),
                team_run_id: self.team_run_id,
                task_id: self.task_id,
                cwd: self.cwd,
            }
            .verify(current_generation),
            Some(LaunchPlacement::Container(snapshot)) => ContainerClaim {
                binding: Some(snapshot),
                cwd: self.cwd,
            }
            .verify(current_generation),
        }
    }
}

/// Everything a launch names, before anything has authorized it.
///
/// This is a plain value and building one is deliberately harmless: it names a
/// run, a seat, a workspace and a prompt, and it cannot be launched. Only
/// pairing it with a runtime-issued [`LaunchAuthority`] produces a
/// [`LaunchRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchParts {
    /// The run being launched.
    pub agent_run_id: AgentRunId,
    /// The team run the role belongs to.
    pub team_run_id: TeamRunId,
    /// The seat this launch fills. Together with `team_run_id` it is the key
    /// admission is decided on.
    pub role_slot_id: RoleSlotId,
    /// The task the role serves.
    pub task_id: TaskId,
    /// The binding id Kontor has already minted for the session to come.
    pub binding_id: RuntimeBindingId,
    /// The verified place this launch will work in. `None` is a launch that
    /// skipped preparation, and a runtime that prepares places refuses it.
    pub placement: Option<LaunchPlacement>,
    /// Where this role says it will work. It must be the bound placement root.
    pub cwd: WorkspaceRoot,
    /// The coding account this run is pinned to, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// What the session starts with.
    pub prompt: BoundedText,
    /// The exact provider/model/effort rung selected for this launch.
    pub model_rung: ModelRung,
    /// The immutable context-window policy this seat runs under.
    ///
    /// Both halves are already frozen and hashed: the launch carries the record,
    /// it does not compute it, so a runtime cannot influence what Kontor says it
    /// asked for.
    pub context_policy: ContextPolicySnapshot,
    /// When the launch was requested.
    pub requested_at: Timestamp,
}

/// Start a new native session for an agent run.
///
/// Every role of a same-runtime team run launches through the *same* verified
/// task workspace binding, and says where it will work. Both are checked before
/// the session exists, because an edit in the wrong tree cannot be undone by
/// noticing it afterwards.
///
/// ## Where this comes from, and why it cannot come from anywhere else
///
/// The only way to obtain one is [`LaunchAuthority::into_request`], and the only
/// way to obtain a [`LaunchAuthority`] is
/// [`crate::adapter::RuntimeAdapter::admit_launch`] — a runtime call that
/// atomically claims the seat this launch names. There is no struct literal
/// (every field is private), no `Clone`, no `Deserialize`, and no feature that
/// unlocks a back door.
///
/// That closes the construction hole, but it is not what the guarantee rests on.
/// A request is still a value: it can be held and handed to
/// [`crate::adapter::RuntimeAdapter::launch`] twice. The guarantee rests on the
/// runtime re-reading its reservation table before its first native effect and
/// consuming the reservation there, so the second call finds nothing to spend —
/// see [`crate::admission`].
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchRequest {
    authority: LaunchAuthority,
    parts: LaunchParts,
}

impl LaunchRequest {
    /// Pair runtime-issued authority with what the launch says.
    ///
    /// Crate-private and reached through [`LaunchAuthority::into_request`]: the
    /// authority is consumed, so one admission produces one request.
    ///
    /// It deliberately does *not* validate `parts` against `authority`. The
    /// comparison belongs where the reservation table is, so an adapter that
    /// skipped the table cannot look correct.
    pub(crate) const fn admitted(authority: LaunchAuthority, parts: LaunchParts) -> Self {
        Self { authority, parts }
    }

    /// The authority this launch is spending.
    #[must_use]
    pub const fn authority(&self) -> &LaunchAuthority {
        &self.authority
    }

    /// The seat this launch claims to fill, as *the launch* names it.
    ///
    /// Read from [`LaunchParts`], never from the authority: the two are compared
    /// by the runtime, so reading the answer off the thing being checked would
    /// make the check vacuous.
    #[must_use]
    pub fn slot(&self) -> RoleSlotKey {
        RoleSlotKey::new(self.parts.team_run_id, self.parts.role_slot_id.clone())
    }

    /// The seat this launch fills.
    #[must_use]
    pub const fn role_slot_id(&self) -> &RoleSlotId {
        &self.parts.role_slot_id
    }

    /// The run being launched.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.parts.agent_run_id
    }

    /// The team run the role belongs to.
    #[must_use]
    pub const fn team_run_id(&self) -> TeamRunId {
        self.parts.team_run_id
    }

    /// The task the role serves.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.parts.task_id
    }

    /// The binding id Kontor minted for the session to come.
    #[must_use]
    pub const fn binding_id(&self) -> RuntimeBindingId {
        self.parts.binding_id
    }

    /// The verified place this launch presents, if any.
    #[must_use]
    pub const fn placement(&self) -> Option<&LaunchPlacement> {
        self.parts.placement.as_ref()
    }

    /// The verified task workspace this launch presents, if it presents one.
    #[must_use]
    pub fn workspace(&self) -> Option<&WorkspaceBindingSnapshot> {
        self.parts
            .placement
            .as_ref()
            .and_then(LaunchPlacement::workspace)
    }

    /// The verified node-keyed container this launch presents, if it presents
    /// one.
    #[must_use]
    pub fn container(&self) -> Option<&ContainerBindingSnapshot> {
        self.parts
            .placement
            .as_ref()
            .and_then(LaunchPlacement::container)
    }

    /// Where this role says it will work.
    #[must_use]
    pub const fn cwd(&self) -> &WorkspaceRoot {
        &self.parts.cwd
    }

    /// The coding account this run is pinned to, if any.
    #[must_use]
    pub const fn account_profile_id(&self) -> Option<AccountProfileId> {
        self.parts.account_profile_id
    }

    /// What the session starts with.
    #[must_use]
    pub const fn prompt(&self) -> &BoundedText {
        &self.parts.prompt
    }

    /// The exact provider/model/effort rung selected for this launch.
    #[must_use]
    pub const fn model_rung(&self) -> &ModelRung {
        &self.parts.model_rung
    }

    /// The immutable context-window policy this seat runs under.
    #[must_use]
    pub const fn context_policy(&self) -> &ContextPolicySnapshot {
        &self.parts.context_policy
    }

    /// When the launch was requested.
    #[must_use]
    pub const fn requested_at(&self) -> Timestamp {
        self.parts.requested_at
    }

    /// The label the runtime must report back for this launch.
    #[must_use]
    pub const fn correlation(&self) -> CorrelationLabel {
        CorrelationLabel::for_run(self.parts.agent_run_id)
    }

    /// What this role claims about where it will work.
    #[must_use]
    pub const fn placement_claim(&self) -> PlacementClaim<'_> {
        PlacementClaim {
            placement: self.parts.placement.as_ref(),
            team_run_id: self.parts.team_run_id,
            task_id: self.parts.task_id,
            cwd: &self.parts.cwd,
        }
    }
}

/// Continue an existing native session in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeRequest {
    /// The binding to resume, with its frozen capability snapshot.
    pub binding: RuntimeBindingSnapshot,
    /// When the resume was requested.
    pub requested_at: Timestamp,
}

/// Deliver one message into an existing native session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageRequest {
    /// The binding to send into.
    pub binding: RuntimeBindingSnapshot,
    /// The Kontor-generated idempotency key for this message.
    pub message_id: MessageId,
    /// The message body.
    pub body: BoundedText,
    /// When the message was sent.
    pub sent_at: Timestamp,
}

impl SendMessageRequest {
    /// The digest the idempotency ledger compares retries against.
    #[must_use]
    pub fn body_hash(&self) -> ContentHash {
        ContentHash::of(self.body.as_str().as_bytes())
    }

    /// The size this request consumes against the runtime's message limit.
    #[must_use]
    pub fn body_bytes(&self) -> u64 {
        self.body.as_str().len() as u64
    }
}

/// Ask an existing native session to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelRequest {
    /// The binding to cancel.
    pub binding: RuntimeBindingSnapshot,
    /// When the cancellation was requested.
    pub requested_at: Timestamp,
}

/// Read the current authoritative state of one native session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectRequest {
    /// The binding to inspect.
    pub binding: RuntimeBindingSnapshot,
    /// When the inspection was requested.
    pub requested_at: Timestamp,
}

/// Bind an already-running native session to an agent run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptRequest {
    /// The run that will own the session.
    pub agent_run_id: AgentRunId,
    /// The binding id Kontor has minted for the adoption.
    pub binding_id: RuntimeBindingId,
    /// The native session being adopted. Evidence, never identity.
    pub native: NativeRuntimeIdentity,
    /// When the adoption was requested.
    pub adopted_at: Timestamp,
}

impl AdoptRequest {
    /// The label the discovered session must already carry.
    #[must_use]
    pub const fn correlation(&self) -> CorrelationLabel {
        CorrelationLabel::for_run(self.agent_run_id)
    }
}

/// Page through a session's recorded content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRequest {
    /// The binding whose content is read.
    pub binding: RuntimeBindingSnapshot,
    /// Where to continue from. `None` starts at the beginning of the epoch.
    pub cursor: Option<HistoryCursor>,
    /// How many items to return at most.
    pub page_size: u32,
}

/// Follow a session's content as it is produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSubscribeRequest {
    /// The binding to follow.
    pub binding: RuntimeBindingSnapshot,
    /// The event kinds the caller wants delivered. Continuity is still checked
    /// over *every* event, so filtering cannot manufacture a sequence gap.
    pub kinds: BTreeSet<SessionEventKind>,
    /// The last position history validated. Delivery starts strictly after it.
    pub strict_after: TimelinePosition,
}

/// Ask a live session to compact its own context, in place.
///
/// The request carries no summary and no transcript. Kontor says *why* it is
/// asking, *under which policy*, and *which durable evidence already exists*;
/// producing the summary is the runtime's job, and the durable record of the
/// work is the handoff this request names, never the summary.
///
/// The binding is the one the runtime issued, so compaction addresses an
/// existing session and has no spelling that could create a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactRequest {
    /// The session to compact, with its frozen capability snapshot.
    pub binding: RuntimeBindingSnapshot,
    /// This attempt's identity, which is also its idempotency key.
    pub receipt_id: CompactionReceiptId,
    /// Why compaction was requested.
    pub trigger: CompactionTrigger,
    /// The seat's immutable requested/effective policy pair.
    pub policy: ContextPolicySnapshot,
    /// The immutable Context Pack the run was frozen against.
    pub context_pack_hash: ContentHash,
    /// The sealed durable handoff this attempt proceeds on.
    ///
    /// Required for a boundary or operator compaction, because those happen at
    /// a point where the work state *is* expressible and losing it would be a
    /// choice. A threshold compaction is the runtime protecting itself and
    /// cannot wait for a scope to close.
    pub handoff_hash: Option<ContentHash>,
    /// When the compaction was requested.
    pub requested_at: Timestamp,
}

impl CompactRequest {
    /// Prove this request may proceed at all.
    ///
    /// # Errors
    /// Returns [`RuntimeError::CompactionUnsafe`] when a boundary or operator
    /// compaction presents no sealed handoff, which would discard work state
    /// nothing else has recorded.
    pub fn validate(&self) -> RuntimeResult<()> {
        if self.trigger.requires_durable_handoff() && self.handoff_hash.is_none() {
            return Err(RuntimeError::CompactionUnsafe {
                rule: "a boundary or operator compaction requires a sealed durable handoff",
            });
        }
        Ok(())
    }

    /// The receipt an adapter returns when it cannot compact at all.
    ///
    /// This is the capability-honest answer, and it is deliberately `Ok`: a
    /// runtime that cannot compact has not failed, it has told the truth. The
    /// two outcomes differ in what they permit afterwards —
    /// [`CompactionStatus::Pending`] for a `required` policy, which blocks reuse
    /// until somebody attests enforcement, and
    /// [`CompactionStatus::NotEnforced`] for `best_effort`, which is visible and
    /// lets the work continue.
    ///
    /// Neither touches the runtime, and neither is ever success. There is no
    /// path here that reloads, archives, restarts or replaces a session as a
    /// substitute.
    ///
    /// # Errors
    /// As [`capability_document`].
    pub fn unsupported_receipt(
        &self,
        capabilities: &RuntimeCapabilities,
        recorded_at: Timestamp,
    ) -> DomainResult<CompactionReceipt> {
        let status = match self.policy.requested.policy.enforcement {
            ContextEnforcement::Required => CompactionStatus::Pending,
            ContextEnforcement::BestEffort => CompactionStatus::NotEnforced,
        };
        Ok(CompactionReceipt {
            schema_version: self.policy.schema_version,
            id: self.receipt_id,
            agent_run_id: self.binding.agent_run_id(),
            binding_id: self.binding.binding_id(),
            native_before: self.binding.identity().clone(),
            // Nothing was done, so there is nothing to have re-read. Copying the
            // "before" identity here would fabricate an observation.
            native_after: None,
            requested: self.policy.requested,
            effective: self.policy.effective,
            trigger: self.trigger,
            capabilities: capability_document(capabilities)?,
            status,
            telemetry: CompactionTelemetry::unknown(),
            context_pack_hash: self.context_pack_hash.clone(),
            handoff_hash: self.handoff_hash.clone(),
            evidence: None,
            recorded_at,
        })
    }

    /// The digest the idempotency ledger compares retries against.
    ///
    /// Covers everything that decides what the runtime is being asked to do, so
    /// the same receipt id with different content is refused rather than
    /// silently replaying somebody else's attempt.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the request cannot be canonicalized.
    pub fn content_hash(&self) -> DomainResult<ContentHash> {
        #[derive(Serialize)]
        struct Digest<'a> {
            schema_version: kontor_core::id::SchemaVersion,
            binding_id: String,
            agent_run_id: String,
            receipt_id: String,
            trigger: CompactionTrigger,
            requested_hash: &'a ContentHash,
            effective_hash: &'a ContentHash,
            context_pack_hash: &'a ContentHash,
            handoff_hash: Option<&'a ContentHash>,
        }
        Ok(CanonicalDocument::from_serializable(&Digest {
            schema_version: self.policy.schema_version,
            binding_id: self.binding.binding_id().to_string(),
            agent_run_id: self.binding.agent_run_id().to_string(),
            receipt_id: self.receipt_id.to_string(),
            trigger: self.trigger,
            requested_hash: &self.policy.requested_hash,
            effective_hash: &self.policy.effective_hash,
            context_pack_hash: &self.context_pack_hash,
            handoff_hash: self.handoff_hash.as_ref(),
        })?
        .hash()
        .clone())
    }
}

/// Freeze a runtime's capability snapshot into the receipt's canonical form.
///
/// [`CompactionReceipt`] lives in `kontor-core` so the store and every client
/// can project it without linking an adapter crate, which means it cannot name
/// [`RuntimeCapabilities`]. The adapter that acted freezes its own snapshot
/// here instead — canonical, hashed, and subject to the same redaction rule as
/// any other stored document.
///
/// # Errors
/// Returns [`DomainError`] when the snapshot cannot be canonicalized.
pub fn capability_document(capabilities: &RuntimeCapabilities) -> DomainResult<CanonicalDocument> {
    let capabilities = serde_json::to_value(capabilities)
        .map_err(|_| DomainError::invalid("RuntimeCapabilities", "is not serializable as JSON"))?;
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": kontor_core::id::SCHEMA_VERSION.get(),
        "capabilities": capabilities,
    }))
}

/// Which way a permission request was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// The action may proceed.
    Allow,
    /// The action is refused.
    Deny,
}

/// Answer a permission request raised inside a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionResponseRequest {
    /// The binding whose session raised the request.
    pub binding: RuntimeBindingSnapshot,
    /// The runtime's own identifier for the request being answered.
    pub permission_id: ExternalId,
    /// The Kontor-generated idempotency key for this answer.
    pub response_id: MessageId,
    /// The answer.
    pub decision: PermissionDecision,
    /// When the answer was given.
    pub responded_at: Timestamp,
}

impl PermissionResponseRequest {
    /// The stable spelling of the answer, as it is recorded in session content.
    #[must_use]
    pub const fn decision_body(&self) -> &'static str {
        match self.decision {
            PermissionDecision::Allow => "allow",
            PermissionDecision::Deny => "deny",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_native_id_cannot_be_read_as_a_kontor_identifier() {
        let native = "sess_01HZY8QF";
        assert!(CorrelationLabel::parse(native).is_err());
        assert!(MessageId::parse(native).is_err());
        assert!(AgentRunId::parse(native).is_err());
    }

    #[test]
    fn a_correlation_label_round_trips_through_its_text_form() {
        let run = AgentRunId::generate();
        let label = CorrelationLabel::for_run(run);
        let parsed = CorrelationLabel::parse(&label.to_string()).expect("a Kontor label parses");
        assert_eq!(parsed.agent_run_id(), run);
    }

    #[test]
    fn a_message_id_rejects_a_non_v7_uuid() {
        assert!(MessageId::parse("00000000-0000-4000-8000-000000000000").is_err());
        let generated = MessageId::generate();
        assert_eq!(
            MessageId::parse(&generated.to_string()).expect("canonical form parses"),
            generated
        );
    }
}
