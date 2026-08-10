//! Typed requests, and the two Kontor-owned identifiers a runtime never gets to
//! mint.
//!
//! Every request names the work with Kontor identifiers from `kontor-core`. A
//! native identifier appears only inside [`NativeRuntimeIdentity`], which is
//! correlation evidence — it is never accepted in a field that means "which run
//! is this", "which binding is this" or "which message is this".

use std::collections::BTreeSet;
use std::fmt;

use kontor_core::id::{
    AccountProfileId, AgentRunId, BoundedText, ContentHash, ExternalId, RuntimeBindingId, TaskId,
    TeamRunId, Timestamp,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::capability::RuntimeBindingSnapshot;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        Self(Uuid::now_v7())
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

/// Start a new native session for an agent run.
///
/// Every role of a same-runtime team run launches through the *same* verified
/// task workspace binding, and says where it will work. Both are checked before
/// the session exists, because an edit in the wrong tree cannot be undone by
/// noticing it afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    /// The run being launched.
    pub agent_run_id: AgentRunId,
    /// The team run the role belongs to.
    pub team_run_id: TeamRunId,
    /// The task the role serves.
    pub task_id: TaskId,
    /// The binding id Kontor has already minted for the session to come.
    pub binding_id: RuntimeBindingId,
    /// The verified task workspace every role of this team run shares. `None`
    /// is a launch that skipped preparation, and a runtime that prepares
    /// workspaces refuses it.
    pub workspace: Option<WorkspaceBindingSnapshot>,
    /// Where this role says it will work. It must be the bound workspace root.
    pub cwd: WorkspaceRoot,
    /// The coding account this run is pinned to, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// What the session starts with.
    pub prompt: BoundedText,
    /// When the launch was requested.
    pub requested_at: Timestamp,
}

impl LaunchRequest {
    /// The label the runtime must report back for this launch.
    #[must_use]
    pub const fn correlation(&self) -> CorrelationLabel {
        CorrelationLabel::for_run(self.agent_run_id)
    }

    /// What this role claims about where it will work.
    #[must_use]
    pub fn workspace_claim(&self) -> WorkspaceClaim<'_> {
        WorkspaceClaim {
            binding: self.workspace.as_ref(),
            team_run_id: self.team_run_id,
            task_id: self.task_id,
            cwd: &self.cwd,
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
