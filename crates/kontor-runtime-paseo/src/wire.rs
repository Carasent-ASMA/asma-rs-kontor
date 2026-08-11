//! The Paseo 0.2.5 wire model: CLI JSON, daemon protocol frames, and the one
//! place a native timeline entry becomes a [`SessionEvent`].
//!
//! Two surfaces, deliberately typed apart:
//!
//! * **CLI JSON** ([`PaseoCliWorkspace`], [`PaseoCliAgent`]) is what
//!   `paseo … --json` prints. It is *thin on purpose* — the live probe recorded
//!   that workspace output omits `projectId` and agent output omits the label
//!   map and the provider session id. Typing it thin is what makes "trust the
//!   CLI and skip the readback" fail to compile rather than fail in production.
//! * **Protocol frames** ([`PaseoWorkspace`], [`PaseoAgent`],
//!   [`PaseoTimelinePage`]) are the authoritative readback, and every binding,
//!   launch, resume and adoption decision is made from these.
//!
//! Nothing here talks to anything. Normalization is pure so the interesting
//! cases — a collapsed projection, a sequence gap, an epoch change, an unknown
//! frame — are decided by a fixture rather than by a daemon's mood.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{CanonicalDocument, ContentHash, ExternalId, Timestamp, parse_utc_timestamp};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use kontor_runtime::request::MessageId;
use kontor_runtime::timeline::{
    EventSubject, SessionEvent, SessionEventKind, TimelineBreak, TimelinePosition,
};
use serde::{Deserialize, Serialize};

/// The Paseo release this adapter's DTOs, fixtures and argv evidence are pinned
/// to.
pub const PASEO_VERSION: &str = "0.2.5";

/// The most CLI stdout this adapter will read before calling the answer a
/// malfunction.
pub const MAX_OUTPUT_BYTES: usize = 1 << 20;

/// The most bytes one daemon frame may carry.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// The largest message body this adapter will hand to Paseo.
pub const MAX_MESSAGE_BYTES: u64 = 64 * 1024;

/// The largest canonical history page this adapter will ask for.
pub const MAX_HISTORY_PAGE: u32 = 500;

/// The most characters of an unknown timeline frame that reach a `Log` payload.
///
/// An unknown frame is diagnostics, and diagnostics from an agent session can
/// quote the operator's prompt. Bounding it is what keeps "we did not recognize
/// this" from becoming a transcript leak into Kontor's own storage.
pub const MAX_UNKNOWN_FRAME_CHARS: usize = 256;

// ---------------------------------------------------------------------------
// Correlation labels
// ---------------------------------------------------------------------------

/// The label keys Kontor plants on every role agent it launches.
///
/// The full set travels because launch recovery is an *exact-label census*: one
/// compatible agent may be adopted as a lost launch's effect, and "compatible"
/// has to mean every one of these agreeing, not a run id that happened to match.
pub mod label {
    /// The Kontor agent run, carrying [`kontor_runtime::request::CorrelationLabel`].
    pub const AGENT_RUN: &str = "kontor.agent-run";
    /// The Jira issue the task is tracked as.
    pub const JIRA_ISSUE: &str = "jira.issue";
    /// The Jira epic the mini-project is tracked as.
    pub const JIRA_EPIC: &str = "jira.epic";
    /// The Kontor mini-project.
    pub const PROJECT_ID: &str = "kontor.project_id";
    /// The Kontor plan item this seat serves.
    pub const TICKET: &str = "kontor.ticket";
    /// The team run the seat belongs to.
    pub const TEAM_RUN: &str = "kontor.team_run";
    /// The role name, which is display data and never a uniqueness key.
    pub const ROLE: &str = "kontor.role";
    /// The stable role slot, which *is* the uniqueness key.
    pub const ROLE_SLOT: &str = "kontor.role_slot_id";
    /// The Paseo workspace the agent must be placed in.
    pub const WORKSPACE_ID: &str = "kontor.workspace_id";
    /// The canonical task worktree path.
    pub const WORKTREE: &str = "kontor.worktree";
    /// The orchestrator agent this seat was launched under.
    pub const PARENT_AGENT: &str = "paseo.parent-agent-id";

    /// Every label key, in the order they are applied.
    pub const ALL: &[&str] = &[
        AGENT_RUN,
        JIRA_ISSUE,
        JIRA_EPIC,
        PROJECT_ID,
        TICKET,
        TEAM_RUN,
        ROLE,
        ROLE_SLOT,
        WORKSPACE_ID,
        WORKTREE,
        PARENT_AGENT,
    ];
}

// ---------------------------------------------------------------------------
// Server identity and features
// ---------------------------------------------------------------------------

/// One capability the Paseo daemon advertises about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaseoFeature {
    /// Project ids are stable across daemon restarts.
    StableProjectIdentity,
    /// Projects can be enumerated over the protocol.
    ProjectList,
    /// Projects can be created over the protocol.
    ProjectAdd,
    /// One project may hold many workspaces.
    WorkspaceMultiplicity,
    /// A subscription can be narrowed to one agent's timeline.
    SelectiveAgentTimeline,
    /// A project's display name can be changed through a supported operation.
    ///
    /// Paseo 0.2.5 does **not** advertise this. The bundled client contains an
    /// internal rename method, and this adapter never calls it: writing another
    /// owner's internal state to improve a display string is the trade nothing
    /// justifies. Name drift is reported as
    /// [`crate::adapter::PaseoProjectOutcome::ReadyWithRenamePending`] instead.
    ProjectRename,
    /// A session's context can be compacted through a supported operation.
    ///
    /// Also absent in 0.2.5, and also never simulated with a reload or a
    /// replacement.
    Compaction,
}

impl PaseoFeature {
    /// The wire spelling the daemon advertises.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableProjectIdentity => "stableProjectIdentity",
            Self::ProjectList => "projectList",
            Self::ProjectAdd => "projectAdd",
            Self::WorkspaceMultiplicity => "workspaceMultiplicity",
            Self::SelectiveAgentTimeline => "selectiveAgentTimeline",
            Self::ProjectRename => "projectRename",
            Self::Compaction => "compaction",
        }
    }
}

/// The features a daemon must advertise before Kontor drives it autonomously.
pub const REQUIRED_FEATURES: &[PaseoFeature] = &[
    PaseoFeature::StableProjectIdentity,
    PaseoFeature::ProjectList,
    PaseoFeature::ProjectAdd,
    PaseoFeature::WorkspaceMultiplicity,
    PaseoFeature::SelectiveAgentTimeline,
];

/// What the daemon says about itself, read fresh before autonomous operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoServerInfo {
    /// The daemon version string.
    pub version: String,
    /// The advertised feature names, verbatim.
    #[serde(default)]
    pub features: BTreeSet<String>,
    /// The daemon's own boot/generation marker, when it exposes one.
    #[serde(default, rename = "serverId")]
    pub server_id: Option<String>,
}

impl PaseoServerInfo {
    /// Whether `feature` is advertised.
    #[must_use]
    pub fn supports(&self, feature: PaseoFeature) -> bool {
        self.features.contains(feature.as_str())
    }

    /// Every required feature this daemon does not advertise, in policy order.
    #[must_use]
    pub fn missing_required(&self) -> Vec<PaseoFeature> {
        REQUIRED_FEATURES
            .iter()
            .copied()
            .filter(|feature| !self.supports(*feature))
            .collect()
    }

    /// Whether this daemon is the pinned baseline.
    #[must_use]
    pub fn is_pinned_baseline(&self) -> bool {
        self.version == PASEO_VERSION
    }
}

// ---------------------------------------------------------------------------
// Projects, workspaces, agents — protocol readback
// ---------------------------------------------------------------------------

/// One Paseo project, as the protocol reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoProject {
    /// The stable project id. This is the epic binding, and the only one.
    pub id: String,
    /// The display name, which is data and never authority.
    #[serde(default)]
    pub name: String,
    /// The Git remote the project was registered from.
    ///
    /// Read but never matched on: the live daemon holds several projects for one
    /// remote, so selecting by it would bind an epic to somebody else's project
    /// (ALT-003).
    #[serde(default, rename = "projectKey")]
    pub project_key: Option<String>,
}

/// The answer to `project.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoProjectList {
    /// Every project the daemon owns.
    #[serde(default)]
    pub projects: Vec<PaseoProject>,
}

/// What kind of place a workspace is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaseoWorkspaceKind {
    /// A registered Git worktree. The only kind a ticket role may occupy.
    Worktree,
    /// A plain local directory, typically the project root.
    Local,
    /// Anything else this adapter has not audited.
    #[serde(other)]
    Other,
}

/// One Paseo workspace, as the protocol reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoWorkspace {
    /// The native workspace id.
    pub id: String,
    /// The project it belongs to. The CLI omits this, which is the whole reason
    /// the protocol readback is mandatory.
    #[serde(rename = "projectId")]
    pub project_id: String,
    /// The absolute working directory.
    pub cwd: String,
    /// What kind of place it is.
    #[serde(rename = "workspaceKind")]
    pub workspace_kind: PaseoWorkspaceKind,
    /// Whether Paseo provisioned the worktree itself.
    ///
    /// `true` means Paseo made a tree of its own rather than registering the
    /// task worktree Kontor prepared, which is a different place than the one
    /// the run was admitted for.
    #[serde(default, rename = "isPaseoOwnedWorktree")]
    pub is_paseo_owned_worktree: bool,
    /// The display title.
    #[serde(default)]
    pub title: String,
    /// Every correlation label, verbatim.
    ///
    /// This is where [`label::TEAM_RUN`] lives for a workspace, and it is what
    /// [`kontor_runtime::workspace::WorkspaceCorrelationEvidence::establish`]
    /// judges. It has to be a label rather than the title, because the title is
    /// the compact display name an operator reads and a Kontor team label is
    /// neither compact nor for humans.
    ///
    /// **Assumption, and the one this adapter is least sure of.** The live probe
    /// recorded `workspace create --isolation local --path … --project …
    /// --title …` and did not enumerate every flag, so `--label` on a workspace
    /// is inferred from the agent surface, which demonstrably has one. If a live
    /// daemon rejects it, that fails loudly at
    /// [`crate::client::PaseoCommand::workspace_create`] — which is the right
    /// direction. The alternative was to pass Kontor's own computed label in as
    /// the "reported" one, which would make workspace correlation evidence prove
    /// nothing at all while looking exactly like this.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// The answer to a workspace fetch or list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoWorkspaceList {
    /// Every workspace the request covered.
    #[serde(default)]
    pub workspaces: Vec<PaseoWorkspace>,
}

/// What Paseo says an agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaseoAgentStatus {
    /// Working.
    Running,
    /// Alive and reusable, with nothing in flight.
    Idle,
    /// Explicitly stopped; the process is gone but the seat is not retired.
    Stopped,
    /// Retired. Only this, read fresh after an explicit intent, retires a seat.
    Archived,
    /// Something this adapter has not audited.
    #[serde(other)]
    Unknown,
}

impl PaseoAgentStatus {
    /// Whether this agent is a live seat that can take the next role turn.
    ///
    /// `idle` is the one that matters. An idle agent — including one Paseo
    /// decorates with `attentionReason=finished` — is the same seat, waiting.
    /// Reading it as a finished run is how a role acquires a second session per
    /// turn and the hierarchy fills up with dead siblings.
    #[must_use]
    pub const fn is_reusable_seat(self) -> bool {
        matches!(self, Self::Running | Self::Idle)
    }

    /// Whether a process restart is needed before this agent can continue.
    #[must_use]
    pub const fn needs_reload(self) -> bool {
        matches!(self, Self::Stopped)
    }

    /// Whether this agent has been retired out of its seat.
    #[must_use]
    pub const fn is_archived(self) -> bool {
        matches!(self, Self::Archived)
    }
}

/// One Paseo agent, as the protocol reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoAgent {
    /// The native agent id — the session identity Kontor correlates against.
    pub id: String,
    /// The workspace it runs in.
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    /// The project that workspace belongs to.
    #[serde(rename = "projectId")]
    pub project_id: String,
    /// Its working directory.
    pub cwd: String,
    /// The orchestrator agent it was launched under, as Paseo recorded it.
    #[serde(default, rename = "parentAgentId")]
    pub parent_agent_id: Option<String>,
    /// The display title.
    #[serde(default)]
    pub title: String,
    /// Every correlation label, verbatim.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// What Paseo says it is doing.
    pub status: PaseoAgentStatus,
    /// Paseo's own attention hint, e.g. `finished`.
    ///
    /// Carried as evidence and never promoted: `finished` on an idle agent is
    /// Paseo saying the last turn ended, not that the run did.
    #[serde(default, rename = "attentionReason")]
    pub attention_reason: Option<String>,
    /// The provider's own session id, when Paseo exposes one.
    #[serde(default, rename = "providerSessionId")]
    pub provider_session_id: Option<String>,
}

impl PaseoAgent {
    /// One label, if present.
    #[must_use]
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }

    /// Whether every entry of `wanted` is present with exactly that value.
    ///
    /// Exact and total. A census that matched on the run label alone would
    /// happily adopt a session from another team run that shares it after a
    /// replacement, so the whole label set is the key.
    #[must_use]
    pub fn matches_labels(&self, wanted: &BTreeMap<String, String>) -> bool {
        wanted
            .iter()
            .all(|(key, value)| self.label(key) == Some(value.as_str()))
    }
}

/// The answer to an agent list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoAgentList {
    /// Every agent the request covered.
    #[serde(default)]
    pub agents: Vec<PaseoAgent>,
}

// ---------------------------------------------------------------------------
// CLI JSON — deliberately thinner than the protocol
// ---------------------------------------------------------------------------

/// `paseo --version --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliVersion {
    /// The CLI version string.
    pub version: String,
}

/// `paseo workspace create --json`.
///
/// No `projectId`. That absence is load-bearing: a workspace this adapter has
/// only seen through the CLI has not been proved to live in the epic project,
/// so it may not be bound (REQ-016, MUT-016).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliWorkspace {
    /// The native workspace id.
    pub id: String,
    /// The path the CLI reports.
    #[serde(default)]
    pub path: String,
    /// The display title.
    #[serde(default)]
    pub title: String,
}

/// `paseo agent run|inspect|update|reload --json`.
///
/// No labels, no workspace, no provider session. Same rule as above.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliAgent {
    /// The native agent id.
    pub id: String,
    /// The display title.
    #[serde(default)]
    pub title: String,
    /// What the CLI says it is doing.
    #[serde(default = "unknown_status")]
    pub status: PaseoAgentStatus,
}

const fn unknown_status() -> PaseoAgentStatus {
    PaseoAgentStatus::Unknown
}

/// `paseo agent stop|archive`, and `paseo workspace archive`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliAck {
    /// Whether Paseo accepted the request.
    pub ok: bool,
    /// The id it acknowledged, echoed back.
    #[serde(default)]
    pub id: String,
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

/// Which projection of a session's content to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaseoProjection {
    /// One entry per native sequence. The only projection Kontor cursors on.
    Canonical,
    /// A display projection that may collapse a whole tool lifecycle into one
    /// entry. Reading it would hand Kontor a cursor whose gaps are invisible.
    Projected,
}

impl PaseoProjection {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Projected => "projected",
        }
    }
}

/// One canonical timeline entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoTimelineEntry {
    /// The native sequence inside the epoch. 1-based and contiguous.
    pub seq: u64,
    /// The native entry kind, verbatim.
    #[serde(rename = "type")]
    pub entry_type: String,
    /// When Paseo emitted it.
    #[serde(rename = "at")]
    pub at: String,
    /// The caller-supplied message id, on a user message.
    #[serde(default, rename = "messageId")]
    pub message_id: Option<String>,
    /// The permission request this entry is about.
    #[serde(default, rename = "permissionId")]
    pub permission_id: Option<String>,
    /// Paseo's own entry id.
    #[serde(default, rename = "entryId")]
    pub entry_id: Option<String>,
    /// A digest of the entry body, which Paseo computes so Kontor never has to
    /// hold the body itself.
    #[serde(default, rename = "bodyDigest")]
    pub body_digest: Option<String>,
    /// How many native sequences this entry covers.
    ///
    /// Always 1 under [`PaseoProjection::Canonical`]. A `projected` read can
    /// report a range here, and the normalizer refuses it rather than paging
    /// over a hole.
    #[serde(default = "one")]
    pub span: u64,
}

const fn one() -> u64 {
    1
}

/// One page of canonical timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoTimelinePage {
    /// The agent this page is about.
    #[serde(rename = "agentId")]
    pub agent_id: String,
    /// The raw epoch, which Paseo spells as a UUID.
    pub epoch: String,
    /// The entries, in ascending sequence order.
    #[serde(default)]
    pub entries: Vec<PaseoTimelineEntry>,
    /// The native sequence to continue after, or `None` when exhausted.
    #[serde(default, rename = "nextAfter")]
    pub next_after: Option<u64>,
}

/// A control signal a live stream can carry instead of content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaseoStreamControl {
    /// The daemon renumbered this session's content.
    Reset,
    /// The cursor the subscription started from no longer exists.
    StaleCursor,
    /// The daemon knows it dropped entries.
    Gap,
}

impl PaseoStreamControl {
    /// The timeline break this signal is.
    ///
    /// Every one of them ends delivery and demands a canonical refetch. None of
    /// them says anything about the run, which is why this maps to a break and
    /// never to a lifecycle state.
    #[must_use]
    pub const fn as_break(self) -> TimelineBreak {
        match self {
            Self::Reset | Self::StaleCursor => TimelineBreak::EpochChanged,
            Self::Gap => TimelineBreak::SequenceGap,
        }
    }
}

/// One frame off the selective `agent_stream` subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoStreamFrame {
    /// The agent the frame belongs to.
    #[serde(rename = "agentId")]
    pub agent_id: String,
    /// The raw epoch the entry is numbered in.
    pub epoch: String,
    /// The entry, when the frame carries content.
    #[serde(default)]
    pub entry: Option<PaseoTimelineEntry>,
    /// The control signal, when the frame carries one instead.
    #[serde(default)]
    pub control: Option<PaseoStreamControl>,
}

/// The answer to `send_agent_message_request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoSendAccepted {
    /// The agent it was delivered into.
    #[serde(rename = "agentId")]
    pub agent_id: String,
    /// The caller-supplied message id, echoed back.
    #[serde(rename = "messageId")]
    pub message_id: String,
}

/// The answer to `agent_permission_response`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoPermissionAccepted {
    /// The agent whose session raised the request.
    #[serde(rename = "agentId")]
    pub agent_id: String,
    /// The permission request that was answered.
    #[serde(rename = "permissionId")]
    pub permission_id: String,
    /// The answer Paseo recorded, verbatim.
    pub decision: String,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Parse a Paseo wire timestamp.
///
/// # Errors
/// Returns [`DomainError`] when the value is not canonical UTC.
pub fn parse_wire_timestamp(subject: &'static str, text: &str) -> DomainResult<Timestamp> {
    parse_utc_timestamp(text)
        .map_err(|_| DomainError::invalid(subject, "is not a canonical UTC timestamp"))
}

/// The event kind one native entry type maps to.
///
/// Everything unrecognized becomes [`SessionEventKind::Log`] rather than being
/// dropped. Dropping would silently renumber the caller's view of a session,
/// which is the one thing the continuity guard cannot detect for itself.
#[must_use]
pub fn classify_entry(entry_type: &str) -> SessionEventKind {
    match entry_type {
        "user_message" | "assistant_message" | "message" => SessionEventKind::Message,
        "tool_call" | "tool_result" => SessionEventKind::ToolCall,
        "permission_request" => SessionEventKind::PermissionRequest,
        "permission_resolved" => SessionEventKind::PermissionResolved,
        "state_change" | "agent_state" => SessionEventKind::StateChange,
        _ => SessionEventKind::Log,
    }
}

/// Turn one canonical entry into a [`SessionEvent`] inside `epoch`.
///
/// `epoch` is the Kontor-side `u64` the raw UUID resolved to; this function
/// never allocates one, because inventing an epoch is exactly how a restored
/// cursor stops meaning anything.
///
/// The native sequence and the native entry type are preserved in the payload,
/// so an audit can re-derive this mapping from what Paseo actually said rather
/// than from what the adapter concluded.
///
/// # Errors
/// * [`RuntimeError::TimelineRefetchRequired`] with
///   [`TimelineBreak::SequenceGap`] when the entry covers more than one native
///   sequence, which is what a `projected` read looks like — a collapsed range
///   is a hole a canonical cursor cannot page over.
/// * [`RuntimeError::Domain`] for a sequence of zero, an unusable timestamp, or
///   an identifier that does not parse.
pub fn normalize_entry(entry: &PaseoTimelineEntry, epoch: u64) -> RuntimeResult<SessionEvent> {
    if entry.span != 1 {
        return Err(RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap,
        });
    }
    if entry.seq == 0 {
        return Err(RuntimeError::Domain(DomainError::invalid(
            "PaseoTimelineEntry.seq",
            "native sequences are 1-based inside an epoch",
        )));
    }
    let emitted_at = parse_wire_timestamp("PaseoTimelineEntry.at", &entry.at)?;
    let kind = classify_entry(&entry.entry_type);
    let subject = match (&entry.permission_id, &entry.message_id) {
        (Some(permission), _) => EventSubject::Permission(ExternalId::parse(permission)?),
        // A native message id is not a Kontor one. When Paseo echoes back the id
        // Kontor supplied it parses and the event is addressable; when the
        // message came from somewhere else it does not, and the event is simply
        // not about a Kontor message.
        (None, Some(message)) => match MessageId::parse(message) {
            Ok(id) => EventSubject::Message(id),
            Err(_) => EventSubject::None,
        },
        (None, None) => EventSubject::None,
    };
    let payload = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "paseo_version": PASEO_VERSION,
        "native": {
            "seq": entry.seq,
            "type": bounded(&entry.entry_type),
            "entry_id": entry.entry_id,
            "body_digest": entry.body_digest,
        },
        "normalized": {
            "kind": format!("{kind:?}"),
        },
    }))?;
    Ok(SessionEvent {
        kind,
        position: TimelinePosition {
            epoch,
            sequence: entry.seq,
        },
        subject,
        native_event_id: entry
            .entry_id
            .as_deref()
            .map(ExternalId::parse)
            .transpose()?,
        emitted_at,
        payload,
    })
}

/// Bound and strip an unaudited native string before it reaches storage.
fn bounded(text: &str) -> String {
    text.chars()
        .take(MAX_UNKNOWN_FRAME_CHARS)
        .filter(|c| !c.is_control())
        .collect()
}

/// The digest of a message body, as the delivery ledger compares retries.
#[must_use]
pub fn body_digest(body: &str) -> ContentHash {
    ContentHash::of(body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, entry_type: &str) -> PaseoTimelineEntry {
        PaseoTimelineEntry {
            seq,
            entry_type: entry_type.to_owned(),
            at: "2026-08-10T09:00:00Z".to_owned(),
            message_id: None,
            permission_id: None,
            entry_id: None,
            body_digest: None,
            span: 1,
        }
    }

    #[test]
    fn a_collapsed_projected_range_is_a_gap_rather_than_one_event() {
        // The `projected` projection folds a whole tool lifecycle into one
        // entry. Accepting it would advance the cursor past sequences that were
        // never delivered, and the guard downstream cannot see what it was
        // never handed.
        let mut collapsed = entry(4, "tool_call");
        collapsed.span = 3;
        assert_eq!(
            normalize_entry(&collapsed, 1).expect_err("a range is not an event"),
            RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::SequenceGap
            }
        );
        normalize_entry(&entry(4, "tool_call"), 1).expect("one sequence is one event");
    }

    #[test]
    fn an_unknown_frame_becomes_a_bounded_log_rather_than_disappearing() {
        let mut unknown = entry(2, "something_paseo_added_later");
        unknown.entry_type = "x".repeat(MAX_UNKNOWN_FRAME_CHARS * 4);
        let event = normalize_entry(&unknown, 1).expect("an unknown frame is still an event");
        assert_eq!(event.kind, SessionEventKind::Log);
        assert_eq!(event.position.sequence, 2);
        assert!(
            event.payload.json().len() < MAX_UNKNOWN_FRAME_CHARS * 4,
            "an unaudited native string is bounded before it is persisted"
        );
    }

    #[test]
    fn a_native_message_id_does_not_become_a_kontor_one() {
        let mut native = entry(1, "user_message");
        native.message_id = Some("msg_01HZY8QF".to_owned());
        let event = normalize_entry(&native, 1).expect("a foreign message is still content");
        assert_eq!(event.subject, EventSubject::None);

        let kontor = MessageId::generate();
        let mut ours = entry(2, "user_message");
        ours.message_id = Some(kontor.to_string());
        assert_eq!(
            normalize_entry(&ours, 1)
                .expect("our own id round-trips")
                .subject,
            EventSubject::Message(kontor)
        );
    }

    #[test]
    fn an_idle_or_finished_agent_is_a_seat_and_not_a_verdict() {
        assert!(PaseoAgentStatus::Idle.is_reusable_seat());
        assert!(PaseoAgentStatus::Running.is_reusable_seat());
        assert!(!PaseoAgentStatus::Stopped.is_reusable_seat());
        assert!(PaseoAgentStatus::Stopped.needs_reload());
        assert!(PaseoAgentStatus::Archived.is_archived());
        assert!(!PaseoAgentStatus::Idle.is_archived());
    }

    #[test]
    fn the_pinned_baseline_advertises_every_required_feature_and_neither_optional_one() {
        let info = PaseoServerInfo {
            version: PASEO_VERSION.to_owned(),
            features: REQUIRED_FEATURES
                .iter()
                .map(|feature| feature.as_str().to_owned())
                .collect(),
            server_id: None,
        };
        assert!(info.is_pinned_baseline());
        assert!(info.missing_required().is_empty());
        assert!(!info.supports(PaseoFeature::ProjectRename));
        assert!(!info.supports(PaseoFeature::Compaction));

        let degraded = PaseoServerInfo {
            version: PASEO_VERSION.to_owned(),
            features: BTreeSet::new(),
            server_id: None,
        };
        assert_eq!(degraded.missing_required().len(), REQUIRED_FEATURES.len());
    }

    #[test]
    fn a_label_census_is_exact_and_total() {
        let agent = PaseoAgent {
            id: "agt_1".to_owned(),
            workspace_id: "wks_1".to_owned(),
            project_id: "prj_1".to_owned(),
            cwd: "/w/task-1".to_owned(),
            parent_agent_id: Some("agt_orchestrator".to_owned()),
            title: "KON-MVP-11 Implement".to_owned(),
            labels: [
                (label::ROLE.to_owned(), "implement".to_owned()),
                (label::ROLE_SLOT.to_owned(), "implement-a".to_owned()),
            ]
            .into_iter()
            .collect(),
            status: PaseoAgentStatus::Idle,
            attention_reason: Some("finished".to_owned()),
            provider_session_id: None,
        };
        let mut wanted: BTreeMap<String, String> = BTreeMap::new();
        wanted.insert(label::ROLE.to_owned(), "implement".to_owned());
        assert!(agent.matches_labels(&wanted));
        // The slot is the key, so a same-role agent in another slot is not this
        // seat however well the role name agrees.
        wanted.insert(label::ROLE_SLOT.to_owned(), "implement-b".to_owned());
        assert!(!agent.matches_labels(&wanted));
    }
}
