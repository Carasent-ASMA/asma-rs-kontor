//! The exact Agent Orchestrator 0.12.1 wire surfaces, and nothing else.
//!
//! Every type here is pinned to one AO version: the REST envelopes, the CDC SSE
//! frame and the `/mux` terminal frames as AO 0.12.1 serializes them (see
//! `PROVENANCE` in `tests/fixtures/ao-0.12.1/manifest.json`). Two rules decide
//! what is typed and what is not, and they pull in opposite directions on
//! purpose:
//!
//! 1. **A value Kontor would otherwise guess is a closed enum.** `activity.state`
//!    and `status` decide what Kontor believes about the work, so an unknown
//!    value must fail as a typed domain error rather than fall back to a
//!    plausible state. Adding `#[serde(other)]` to either of these would be the
//!    defect: it turns "AO told us something we do not understand" into "the
//!    session is idle".
//! 2. **A value Kontor only routes on is open text.** `harness` has 23 values in
//!    0.12.1 and grows; a lane matches its own harness and ignores the rest, so a
//!    session running `aider` must read as *not this lane* rather than break the
//!    inventory Kontor needs to reconcile with.
//!
//! Unknown *fields* are accepted. `deny_unknown_fields` here would turn any AO
//! patch release that adds a response field into a total adapter outage, which is
//! a worse failure than ignoring a field nobody reads. What is checked is the
//! other direction: every field the adapter acts on is required, so a truncated
//! or foreign envelope fails to parse instead of arriving with silent defaults.

use kontor_core::id::{ExternalId, Timestamp};
use kontor_core::{DomainError, DomainResult};
use serde::Deserialize;

/// The AO REST prefix every versioned surface hangs from.
pub const API_V1: &str = "/api/v1";

/// Largest response body the adapter will read, in bytes.
///
/// AO is a loopback daemon, so this is not a hostile-input bound but a
/// runaway-inventory one: a session list is the largest legitimate response and
/// stays far under this.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Largest follow-up message AO accepts, from `SendSessionMessageRequest`
/// (`maxLength: 4096` in the 0.12.1 OpenAPI document).
pub const MAX_MESSAGE_BYTES: u64 = 4096;

/// Largest `displayName` AO accepts (`maxLength: 20`).
///
/// This is exactly why the correlation label rides `branch` and never the
/// display name: a Kontor label is longer than 20 characters, so a display name
/// could only ever hold a truncated prefix, and a truncated correlation label is
/// not correlation evidence.
pub const MAX_DISPLAY_NAME_CHARS: usize = 20;

/// Reject a wire value the adapter would otherwise have to guess at.
fn refuse(subject: &'static str, rule: &'static str) -> DomainError {
    DomainError::invalid(subject, rule)
}

/// Parse a timestamp as a *foreign* system rendered it.
///
/// This deliberately does not use `kontor_core::id::parse_utc_timestamp`: that
/// one demands text which round-trips to itself, which is the right rule for
/// Kontor's own persisted form and the wrong rule for Go's `time.Time`, whose
/// RFC 3339 output carries a variable fractional part and may carry an offset.
/// The value is what matters here, not AO's spelling of it.
///
/// # Errors
/// Returns [`DomainError::Invalid`] for anything that is not RFC 3339.
pub fn parse_wire_timestamp(subject: &'static str, text: &str) -> DomainResult<Timestamp> {
    text.parse::<Timestamp>()
        .map_err(|_| refuse(subject, "is not an RFC 3339 timestamp"))
}

// ---------------------------------------------------------------------------
// Closed vocabularies
// ---------------------------------------------------------------------------

/// The AO clients this adapter has verified behavior for.
///
/// AO 0.12.1 accepts 23 harness values. Kontor lanes are declared over these
/// four because they are the ones whose launch, follow-up and lifecycle behavior
/// the ticket verified; the rest are discoverable but not drivable from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AoHarness {
    /// Anthropic Claude Code.
    ClaudeCode,
    /// OpenAI Codex.
    Codex,
    /// Cursor's agent CLI.
    Cursor,
    /// OpenCode.
    OpenCode,
}

impl AoHarness {
    /// Every verified harness, in declaration order.
    pub const ALL: &'static [Self] = &[Self::ClaudeCode, Self::Codex, Self::Cursor, Self::OpenCode];

    /// The exact spelling AO 0.12.1 uses on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::OpenCode => "opencode",
        }
    }

    /// Parse an AO harness spelling.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a harness this adapter has not
    /// verified, which is a refusal to drive it rather than a claim it is absent.
    pub fn parse(text: &str) -> DomainResult<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|harness| harness.as_str() == text)
            .ok_or_else(|| refuse("AoHarness", "is not a harness this adapter has verified"))
    }

    /// Whether this harness is the one whose unsafe default must be refused
    /// before every relaunch.
    ///
    /// Codex is the only 0.12.1 adapter that maps an empty, `default` or
    /// `bypass-permissions` permission mode onto
    /// `--dangerously-bypass-approvals-and-sandbox`.
    #[must_use]
    pub const fn needs_permission_guard(self) -> bool {
        matches!(self, Self::Codex)
    }
}

impl std::fmt::Display for AoHarness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// AO's flat session model: a worker or an orchestrator, and nothing else.
///
/// Kontor keeps team, task and parent relationships itself, so this value is
/// lane configuration rather than hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AoSessionKind {
    /// A single working session.
    Worker,
    /// An AO session that may delegate to AO-internal children.
    Orchestrator,
}

impl AoSessionKind {
    /// The exact spelling AO 0.12.1 uses on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Orchestrator => "orchestrator",
        }
    }

    /// Which project-config role override applies to this kind, exactly as AO
    /// 0.12.1's `roleOverride` resolves it.
    #[must_use]
    pub const fn role(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Orchestrator => "orchestrator",
        }
    }
}

/// AO's hook-reported activity state — the *raw* signal.
///
/// This comes from the client's own hook callbacks rather than from scraped
/// transcript output, which is why it is the stronger of the two lifecycle
/// inputs. There is no catch-all variant: an unknown state must be refused, not
/// rounded down to idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AoActivityState {
    /// The agent is working.
    Active,
    /// The agent has signalled nothing to do. Reusable, not finished.
    Idle,
    /// The agent is at an empty prompt awaiting its next instruction.
    WaitingInput,
    /// The agent is stopped on a pending decision — a permission or approval
    /// dialog. A stray keystroke here answers the dialog on the operator's
    /// behalf, which is why Kontor never sends into this state.
    Blocked,
    /// The agent process exited. Lost contact, not an outcome.
    Exited,
}

/// AO's derived display status — the *weaker* signal.
///
/// AO documents this as a display projection that is never stored: it mixes
/// activity with pull-request facts. Kontor reads it only to fill in what the
/// raw activity does not say, and never lets a source-control value move a run
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AoSessionStatus {
    /// Derived from `activity.active`.
    Working,
    /// Derived from `waiting_input` or `blocked`. Which one is not recoverable
    /// from here, which is exactly why it may never be read as a permission
    /// request.
    NeedsInput,
    /// The agent process exited.
    Exited,
    /// No activity signal, and AO does not claim to know why.
    Idle,
    /// A live session whose agent never delivered a hook callback for the
    /// current spawn. AO renders this instead of a confident idle.
    NoSignal,
    /// The session was terminated.
    Terminated,
    /// A pull request is open.
    PrOpen,
    /// The pull request is a draft.
    Draft,
    /// Continuous integration failed.
    CiFailed,
    /// Review is pending.
    ReviewPending,
    /// Changes were requested.
    ChangesRequested,
    /// The pull request was approved.
    Approved,
    /// The pull request is mergeable.
    Mergeable,
    /// The pull request was merged.
    Merged,
}

impl AoSessionStatus {
    /// Whether this value describes source control rather than execution.
    ///
    /// A merged pull request is a product-workflow fact. Reading it as
    /// completion is the single most tempting false terminal in this adapter,
    /// because "merged" reads like "done" in every other context.
    #[must_use]
    pub const fn is_source_control(self) -> bool {
        matches!(
            self,
            Self::PrOpen
                | Self::Draft
                | Self::CiFailed
                | Self::ReviewPending
                | Self::ChangesRequested
                | Self::Approved
                | Self::Mergeable
                | Self::Merged
        )
    }
}

/// AO's permission mode vocabulary, from `domain.PermissionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AoPermissionMode {
    /// `default` — adapters choose their own baseline. For Codex 0.12.1 that
    /// baseline is a full approvals-and-sandbox bypass.
    Default,
    /// `accept-edits`.
    AcceptEdits,
    /// `auto`.
    Auto,
    /// `bypass-permissions`.
    BypassPermissions,
}

impl AoPermissionMode {
    /// The exact spelling AO 0.12.1 stores.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "accept-edits",
            Self::Auto => "auto",
            Self::BypassPermissions => "bypass-permissions",
        }
    }

    /// Resolve one stored permissions value exactly as AO 0.12.1 does.
    ///
    /// `ports.NormalizePermissionMode` collapses empty **and every unrecognized
    /// value** to `default`. Reproducing that collapse rather than treating an
    /// unknown value as its own case is what makes the guard sound: on AO's side
    /// an unknown mode is not "unspecified, so leave the client alone", it is the
    /// mode that emits `--dangerously-bypass-approvals-and-sandbox`.
    #[must_use]
    pub fn normalize(text: &str) -> Self {
        match text {
            "accept-edits" => Self::AcceptEdits,
            "auto" => Self::Auto,
            "bypass-permissions" => Self::BypassPermissions,
            _ => Self::Default,
        }
    }

    /// Whether Kontor may launch, restore or resume Codex under this mode.
    ///
    /// Only the two modes that provably keep an approval gate are allowed.
    #[must_use]
    pub const fn is_approval_gated(self) -> bool {
        matches!(self, Self::AcceptEdits | Self::Auto)
    }

    /// The Codex argv fragment AO 0.12.1 appends for this mode.
    ///
    /// Kept beside the policy it justifies so the refusal and the reason for it
    /// cannot drift apart, and asserted against recorded argv evidence in the
    /// contract suite.
    #[must_use]
    pub const fn codex_approval_argv(self) -> &'static [&'static str] {
        match self {
            Self::Default | Self::BypassPermissions => {
                &["--dangerously-bypass-approvals-and-sandbox"]
            }
            Self::AcceptEdits => &["--ask-for-approval", "on-request"],
            Self::Auto => &[
                "--ask-for-approval",
                "on-request",
                "-c",
                "approvals_reviewer=\"auto_review\"",
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// REST envelopes
// ---------------------------------------------------------------------------

/// AO's locked error envelope (`APIError`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoApiError {
    /// The error class.
    pub error: String,
    /// The stable machine code.
    pub code: String,
    /// The human-readable message. Never surfaced in a Kontor refusal: a
    /// `RuntimeError` payload is structural by contract.
    pub message: String,
}

/// `GET /api/v1/agents` — installed and authenticated clients.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoListAgentsResponse {
    /// Agents this daemon build supports.
    pub supported: Vec<AoAgentInfo>,
    /// Agents whose binary resolved during AO's latest local catalog probe.
    pub installed: Vec<AoAgentInfo>,
    /// Agents whose local auth probe recently passed. AO documents this as
    /// advisory and stale-prone, and spawn as the authoritative check — so it is
    /// discovery evidence and never account identity.
    pub authorized: Vec<AoAgentInfo>,
}

/// One entry of AO's agent catalog.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoAgentInfo {
    /// The harness identifier.
    pub id: String,
    /// The display label.
    pub label: String,
}

/// `GET /api/v1/projects/{id}` — the project, or the reason it could not resolve.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoProjectGetResponse {
    /// `ok` or `degraded`.
    pub status: AoProjectStatus,
    /// The project envelope. A degraded project carries `resolveError` and no
    /// config, so it can never satisfy the pre-spawn security check.
    pub project: AoProjectOrDegraded,
}

/// Whether AO could resolve the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AoProjectStatus {
    /// Fully resolved.
    Ok,
    /// AO knows of the project but could not resolve it.
    Degraded,
}

/// A resolved project or a degraded stub.
///
/// AO's `ProjectOrDegraded` is an untagged `oneOf`: the two arms differ only by
/// `resolveError`, and the resolved arm's remaining fields are a subset of the
/// degraded one's. An untagged enum takes the **first** arm that parses, so the
/// degraded arm has to come first — it is the one with the discriminating
/// required field. Declared the other way round, every degraded project would
/// parse as resolved with its `resolveError` silently dropped, and a project AO
/// could not even resolve would sail through the pre-spawn security check. The
/// variant order here is load-bearing, and
/// `a_degraded_project_cannot_pass_as_a_resolved_one` is what holds it in place.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum AoProjectOrDegraded {
    /// A project AO could not resolve.
    Degraded(AoDegradedProject),
    /// A project AO resolved.
    Resolved(Box<AoProject>),
}

impl AoProjectGetResponse {
    /// The project, only when AO fully resolved it.
    ///
    /// Both halves are checked: the `status` discriminator *and* the arm that
    /// actually parsed. Either alone would be enough on a well-behaved response,
    /// which is the reason to require both — the one case that matters is the
    /// response that is not well-behaved.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a degraded project, and for a
    /// `status`/payload pair that disagrees with itself.
    pub fn resolved(&self) -> DomainResult<&AoProject> {
        match (&self.status, &self.project) {
            (AoProjectStatus::Ok, AoProjectOrDegraded::Resolved(project)) => Ok(project),
            (AoProjectStatus::Degraded, _) => Err(refuse(
                "AoProjectGetResponse",
                "reports a degraded project, which cannot authorize a launch",
            )),
            (AoProjectStatus::Ok, AoProjectOrDegraded::Degraded(_)) => Err(refuse(
                "AoProjectGetResponse",
                "reports ok but carries an unresolved project",
            )),
        }
    }
}

/// A resolved AO project.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoProject {
    /// The project id.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The canonical filesystem path AO works in.
    pub path: String,
    /// Per-project configuration, including the agent config the permission
    /// guard resolves.
    #[serde(default)]
    pub config: AoProjectConfig,
}

/// A project AO could not resolve.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoDegradedProject {
    /// The project id.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The recorded path.
    pub path: String,
    /// Why resolution failed.
    #[serde(rename = "resolveError")]
    pub resolve_error: String,
}

/// The subset of AO's `ProjectConfig` the permission guard reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AoProjectConfig {
    /// The project-wide agent config.
    #[serde(rename = "agentConfig", default)]
    pub agent_config: AoAgentConfig,
    /// The worker role override.
    #[serde(default)]
    pub worker: AoRoleOverride,
    /// The orchestrator role override.
    #[serde(default)]
    pub orchestrator: AoRoleOverride,
}

/// AO's typed per-project agent configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AoAgentConfig {
    /// Model override.
    #[serde(default)]
    pub model: String,
    /// Agent-owned operating mode.
    #[serde(default)]
    pub mode: String,
    /// Starting permission mode. Empty is *not* "unset and therefore safe": AO
    /// normalizes it to `default`.
    #[serde(default)]
    pub permissions: String,
}

/// A per-role harness and agent-config override.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AoRoleOverride {
    /// The role's harness, when it overrides the project's.
    #[serde(default)]
    pub agent: String,
    /// The role's agent config, whose non-empty fields win.
    #[serde(rename = "agentConfig", default)]
    pub agent_config: AoAgentConfig,
}

impl AoProjectConfig {
    /// Resolve the effective agent config for `kind`, exactly as AO 0.12.1's
    /// `effectiveAgentConfig` does: start from the project config, then let each
    /// **non-empty** role-override field win.
    ///
    /// Field-by-field overlay is the part that matters. Replacing the whole
    /// config when any override field is set would let a worker override that
    /// only names a model silently discard the project's safe permission mode.
    #[must_use]
    pub fn effective_agent_config(&self, kind: AoSessionKind) -> AoAgentConfig {
        let override_config = match kind {
            AoSessionKind::Worker => &self.worker.agent_config,
            AoSessionKind::Orchestrator => &self.orchestrator.agent_config,
        };
        let mut merged = self.agent_config.clone();
        if !override_config.model.is_empty() {
            merged.model = override_config.model.clone();
        }
        if !override_config.mode.is_empty() {
            merged.mode = override_config.mode.clone();
        }
        if !override_config.permissions.is_empty() {
            merged.permissions = override_config.permissions.clone();
        }
        merged
    }

    /// The effective permission mode for `kind`, normalized as AO would.
    #[must_use]
    pub fn effective_permission_mode(&self, kind: AoSessionKind) -> AoPermissionMode {
        AoPermissionMode::normalize(&self.effective_agent_config(kind).permissions)
    }
}

/// `GET /api/v1/sessions` — the inventory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoListSessionsResponse {
    /// Every session AO currently owns, across projects and harnesses.
    pub sessions: Vec<AoSessionView>,
}

/// AO's activity reading.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoActivity {
    /// The raw hook-reported state.
    pub state: AoActivityState,
    /// When it was last observed.
    #[serde(rename = "lastActivityAt")]
    pub last_activity_at: String,
}

/// `ControllersSessionView` — the one session envelope AO returns everywhere.
///
/// Only the fields Kontor acts on are declared. AO's preview, review,
/// pull-request, pin and mobile fields are deliberately absent: they are not
/// lifecycle truth, and a field that is never read cannot be accidentally
/// promoted into evidence.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoSessionView {
    /// AO's stable session id.
    pub id: String,
    /// The owning AO project.
    #[serde(rename = "projectId")]
    pub project_id: String,
    /// Worker or orchestrator.
    pub kind: AoSessionKind,
    /// The client. Open text so a lane can decline a harness it has not
    /// verified without failing the whole inventory.
    #[serde(default)]
    pub harness: Option<String>,
    /// The git branch. This carries the Kontor correlation label.
    #[serde(default)]
    pub branch: Option<String>,
    /// The raw activity reading.
    pub activity: AoActivity,
    /// AO's derived display status.
    pub status: AoSessionStatus,
    /// Whether the session is terminated. Only a *fresh* read of this may
    /// evidence cancellation.
    #[serde(rename = "isTerminated")]
    pub is_terminated: bool,
    /// The terminal handle, when AO has one. Recorded as evidence; never used to
    /// read PTY bytes as session content.
    #[serde(rename = "terminalHandleId", default)]
    pub terminal_handle_id: Option<String>,
    /// When AO created the session.
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// When AO last updated it.
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

impl AoSessionView {
    /// The session id as an external identifier.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for an id AO should never emit.
    pub fn native_id(&self) -> DomainResult<ExternalId> {
        ExternalId::parse(&self.id)
    }

    /// When AO last said anything about this session.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the timestamp is not RFC 3339.
    pub fn observed_at(&self) -> DomainResult<Timestamp> {
        parse_wire_timestamp("AoSessionView.updatedAt", &self.updated_at)
    }

    /// Whether this session belongs to `project_id` running `harness`.
    ///
    /// A lane owns exactly one project and one harness, so this is the whole
    /// membership test. An unverified harness value simply fails to match.
    #[must_use]
    pub fn belongs_to(&self, project_id: &str, harness: AoHarness) -> bool {
        self.project_id == project_id
            && self
                .harness
                .as_deref()
                .is_some_and(|found| found == harness.as_str())
    }
}

/// `POST /api/v1/sessions` — the spawn envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoSpawnSessionResponse {
    /// The session AO created.
    pub session: AoSessionView,
}

/// `POST /api/v1/sessions/{id}/send` — the follow-up acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoSendSessionMessageResponse {
    /// Whether AO accepted the message.
    pub ok: bool,
    /// The session it was delivered into.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// The message AO echoed back.
    pub message: String,
}

/// `POST /api/v1/sessions/{id}/kill` — the stop acknowledgement.
///
/// An acknowledgement that AO *accepted* the request. It is not evidence that
/// the session stopped, which is why cancellation still needs a fresh inspect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoKillSessionResponse {
    /// Whether AO accepted the request.
    pub ok: bool,
    /// The session it addressed.
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// `POST /api/v1/sessions/{id}/restore` — restart a terminated session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoRestoreSessionResponse {
    /// Whether AO accepted the request.
    pub ok: bool,
    /// The session it addressed.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// The session as AO now sees it.
    pub session: AoSessionView,
}

/// `POST /api/v1/sessions/{id}/resume-agent` — restart an exited client inside a
/// live session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoResumeAgentResponse {
    /// Whether AO accepted the request.
    pub ok: bool,
    /// The session it addressed.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// The session as AO now sees it.
    pub session: AoSessionView,
}

// ---------------------------------------------------------------------------
// CDC SSE
// ---------------------------------------------------------------------------

/// One AO CDC event, as `GET /api/v1/events` serializes it.
///
/// `seq` is AO's **global** change-log sequence, not a per-session one. Every
/// continuity rule in this adapter validates it before filtering by session,
/// because filtering first turns another session's event into a hole in this
/// one's numbering.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AoCdcEvent {
    /// The global monotonic sequence and idempotency key.
    pub seq: u64,
    /// The project the change belongs to.
    #[serde(rename = "projectId")]
    pub project_id: String,
    /// The session, absent for project-level changes.
    #[serde(rename = "sessionId", default)]
    pub session_id: Option<String>,
    /// The change type.
    #[serde(rename = "type")]
    pub event_type: AoCdcEventType,
    /// When AO recorded it.
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// The change types AO's database triggers emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AoCdcEventType {
    /// A session row was inserted.
    SessionCreated,
    /// A session row changed.
    SessionUpdated,
    /// A pull request was created.
    PrCreated,
    /// A pull request changed.
    PrUpdated,
    /// A pull-request check was recorded.
    PrCheckRecorded,
    /// A pull request moved between sessions.
    PrSessionChanged,
    /// A review thread was added.
    PrReviewThreadAdded,
    /// A review thread was resolved.
    PrReviewThreadResolved,
}

impl AoCdcEventType {
    /// Whether this change type says anything about a session's lifecycle.
    ///
    /// Only the two session types do. The pull-request types are product
    /// workflow: they move a review forward and say nothing about whether the
    /// agent is still running.
    #[must_use]
    pub const fn is_session_lifecycle(self) -> bool {
        matches!(self, Self::SessionCreated | Self::SessionUpdated)
    }
}

/// Parse one AO SSE recording into its events, in wire order.
///
/// Frames are `id: <seq>\nevent: <type>\ndata: <json>\n\n`. The parser reads the
/// `data:` payload and ignores `id:`/`event:`, which are redundant projections of
/// fields already inside it — trusting the frame header over the payload would
/// let a malformed recording disagree with itself and be believed.
///
/// # Errors
/// Returns [`DomainError::Invalid`] when a frame's payload is not an AO 0.12.1
/// event envelope. A comment or keep-alive line is skipped, not refused.
pub fn parse_sse_events(recording: &str) -> DomainResult<Vec<AoCdcEvent>> {
    let mut events = Vec::new();
    for line in recording.lines() {
        let line = line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() {
            continue;
        }
        let event: AoCdcEvent = serde_json::from_str(payload)
            .map_err(|_| refuse("AoCdcEvent", "is not an AO 0.12.1 event envelope"))?;
        events.push(event);
    }
    Ok(events)
}

// ---------------------------------------------------------------------------
// Terminal mux frames (recorded only)
// ---------------------------------------------------------------------------

/// AO's `/mux` WebSocket frames.
///
/// This module exists to prove one negative: that AO's terminal transport is
/// character-level PTY bytes and therefore *cannot* be the semantic session
/// content Kontor's timeline contract is about. The frames are parsed from
/// recordings so the claim is checked against the real protocol rather than
/// asserted in a comment; nothing here opens a socket, and no frame ever becomes
/// a persisted runtime event, a session message or a permission request.
pub mod mux {
    use super::{DomainResult, refuse};
    use serde::Deserialize;

    /// The channels AO multiplexes over one socket.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum AoMuxChannel {
        /// Per-pane PTY byte stream, keyed by an opaque runtime handle id.
        Terminal,
        /// The client opting in to the session-state channel.
        Subscribe,
        /// Server-pushed session-state messages, fed from AO's CDC log.
        Sessions,
        /// Liveness.
        System,
    }

    /// One frame in either direction.
    ///
    /// Client and server share the envelope in AO 0.12.1; `type` is what
    /// distinguishes them. `data` is base64 because PTY output is arbitrary
    /// bytes and need not be valid UTF-8 — which is the whole reason it is not
    /// text Kontor could read as content.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct AoMuxFrame {
        /// The channel.
        pub ch: AoMuxChannel,
        /// The terminal handle, for `terminal` frames.
        #[serde(default)]
        pub id: Option<String>,
        /// The frame type.
        #[serde(rename = "type")]
        pub frame_type: String,
        /// Base64 PTY bytes, for `terminal`/`data` frames.
        #[serde(default)]
        pub data: Option<String>,
        /// Authoritative grid width, for server-pushed `resize`.
        #[serde(default)]
        pub cols: Option<u16>,
        /// Authoritative grid height, for server-pushed `resize`.
        #[serde(default)]
        pub rows: Option<u16>,
        /// The error text, for `error` frames.
        #[serde(default)]
        pub error: Option<String>,
        /// The projected session change, for `sessions`/`snapshot` frames.
        #[serde(default)]
        pub session: Option<AoMuxSessionUpdate>,
    }

    /// The `sessions`/`snapshot` payload: one CDC change projected to the fields
    /// a client needs in order to *refetch* over REST.
    ///
    /// It deliberately carries no change payload, which is the protocol itself
    /// saying the mux is a notification channel and not a content channel.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct AoMuxSessionUpdate {
        /// The global CDC sequence.
        pub seq: u64,
        /// The project.
        #[serde(rename = "projectId")]
        pub project_id: String,
        /// The session, when the change is session-scoped.
        #[serde(rename = "sessionId", default)]
        pub session_id: Option<String>,
        /// The change type.
        #[serde(rename = "eventType")]
        pub event_type: String,
    }

    impl AoMuxFrame {
        /// Whether this frame carries raw PTY bytes.
        ///
        /// Every such frame is dropped: it is neither a message, a tool call, a
        /// permission request nor a lifecycle observation, and translating it
        /// into one would be fabricating session content out of screen paint.
        #[must_use]
        pub fn is_pty_payload(&self) -> bool {
            self.ch == AoMuxChannel::Terminal && matches!(self.frame_type.as_str(), "data")
        }

        /// The base64 payload's decoded length, without decoding it into a value
        /// anything could persist.
        ///
        /// Used only to prove a recording really does carry bytes rather than
        /// text. There is deliberately no accessor that returns the bytes.
        ///
        /// # Errors
        /// Returns [`DomainError::Invalid`] when `data` is not base64.
        pub fn pty_payload_len(&self) -> DomainResult<usize> {
            let encoded = self
                .data
                .as_deref()
                .ok_or_else(|| refuse("AoMuxFrame", "carries no data payload"))?;
            decoded_base64_len(encoded)
        }
    }

    /// The number of bytes `encoded` decodes to, validating the alphabet and
    /// padding without materializing the bytes.
    ///
    /// A dependency for this would be a base64 crate the workspace does not pin,
    /// for a value nothing is allowed to keep.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a length, alphabet or padding a
    /// base64 encoder would never produce.
    pub fn decoded_base64_len(encoded: &str) -> DomainResult<usize> {
        if !encoded.len().is_multiple_of(4) {
            return Err(refuse(
                "AoMuxFrame.data",
                "is not a whole number of base64 quanta",
            ));
        }
        let body = encoded.trim_end_matches('=');
        let padding = encoded.len() - body.len();
        if padding > 2 {
            return Err(refuse("AoMuxFrame.data", "carries more than two pad bytes"));
        }
        if body
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/'))
        {
            return Err(refuse(
                "AoMuxFrame.data",
                "is not in the standard base64 alphabet",
            ));
        }
        Ok(encoded.len() / 4 * 3 - padding)
    }

    /// Parse a recorded mux frame log.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when a frame is not an AO 0.12.1 mux
    /// frame.
    pub fn parse_frames(recording: &str) -> DomainResult<Vec<AoMuxFrame>> {
        serde_json::from_str(recording)
            .map_err(|_| refuse("AoMuxFrame", "is not an AO 0.12.1 mux frame log"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_activity_state_is_refused_rather_than_rounded_down() {
        // The mutant this kills: `#[serde(other)] Unknown` on either lifecycle
        // enum, which would turn "AO said something new" into a confident state.
        assert!(serde_json::from_str::<AoActivityState>("\"active\"").is_ok());
        assert!(serde_json::from_str::<AoActivityState>("\"compacting\"").is_err());
        assert!(serde_json::from_str::<AoSessionStatus>("\"no_signal\"").is_ok());
        assert!(serde_json::from_str::<AoSessionStatus>("\"almost_done\"").is_err());
    }

    #[test]
    fn an_unverified_harness_is_not_this_lane_rather_than_a_parse_failure() {
        // A harness Kontor has not verified must not be drivable...
        assert!(AoHarness::parse("aider").is_err());
        assert_eq!(
            AoHarness::parse("claude-code").expect("a verified harness"),
            AoHarness::ClaudeCode
        );
        // ...but it must still deserialize inside an inventory, or one foreign
        // session would cost Kontor the reconciliation of every other one.
        let view: AoSessionView = serde_json::from_str(
            r#"{"id":"s1","projectId":"p1","kind":"worker","harness":"aider",
                "activity":{"state":"idle","lastActivityAt":"2026-08-10T09:00:00Z"},
                "status":"idle","isTerminated":false,
                "createdAt":"2026-08-10T08:00:00Z","updatedAt":"2026-08-10T09:00:00Z"}"#,
        )
        .expect("a foreign harness still parses");
        assert!(!view.belongs_to("p1", AoHarness::ClaudeCode));
        assert!(!view.belongs_to("p1", AoHarness::Codex));
    }

    #[test]
    fn an_empty_or_unknown_permission_mode_normalizes_to_the_unsafe_default() {
        // AO's own NormalizePermissionMode collapses both to `default`, and
        // Codex maps `default` to a full bypass. A guard that treated either as
        // "unset, so leave it alone" would wave the bypass through.
        for spelling in ["", "default", "nonsense", "DEFAULT"] {
            let mode = AoPermissionMode::normalize(spelling);
            assert_eq!(mode, AoPermissionMode::Default);
            assert!(!mode.is_approval_gated());
            assert!(
                mode.codex_approval_argv()
                    .contains(&"--dangerously-bypass-approvals-and-sandbox")
            );
        }
        for spelling in ["accept-edits", "auto"] {
            let mode = AoPermissionMode::normalize(spelling);
            assert!(mode.is_approval_gated());
            assert!(mode.codex_approval_argv().contains(&"--ask-for-approval"));
            assert!(
                !mode
                    .codex_approval_argv()
                    .contains(&"--dangerously-bypass-approvals-and-sandbox")
            );
        }
        assert!(!AoPermissionMode::normalize("bypass-permissions").is_approval_gated());
    }

    #[test]
    fn a_role_override_wins_field_by_field() {
        let config: AoProjectConfig = serde_json::from_str(
            r#"{"agentConfig":{"permissions":"accept-edits","model":"safe-model"},
                "worker":{"agentConfig":{"model":"other-model"}},
                "orchestrator":{"agentConfig":{"permissions":"bypass-permissions"}}}"#,
        )
        .expect("a project config");
        // The worker override names only a model, so the project's safe
        // permission mode survives.
        let worker = config.effective_agent_config(AoSessionKind::Worker);
        assert_eq!(worker.model, "other-model");
        assert_eq!(worker.permissions, "accept-edits");
        assert!(
            config
                .effective_permission_mode(AoSessionKind::Worker)
                .is_approval_gated()
        );
        // The orchestrator override names only permissions, and takes them
        // somewhere unsafe.
        assert_eq!(
            config.effective_permission_mode(AoSessionKind::Orchestrator),
            AoPermissionMode::BypassPermissions
        );
        assert_eq!(
            config
                .effective_agent_config(AoSessionKind::Orchestrator)
                .model,
            "safe-model"
        );
    }

    #[test]
    fn source_control_status_is_never_execution_state() {
        for status in [
            AoSessionStatus::PrOpen,
            AoSessionStatus::Draft,
            AoSessionStatus::CiFailed,
            AoSessionStatus::ReviewPending,
            AoSessionStatus::ChangesRequested,
            AoSessionStatus::Approved,
            AoSessionStatus::Mergeable,
            AoSessionStatus::Merged,
        ] {
            assert!(status.is_source_control(), "{status:?} is product workflow");
        }
        for status in [
            AoSessionStatus::Working,
            AoSessionStatus::Idle,
            AoSessionStatus::NeedsInput,
            AoSessionStatus::Exited,
            AoSessionStatus::NoSignal,
            AoSessionStatus::Terminated,
        ] {
            assert!(!status.is_source_control());
        }
    }

    #[test]
    fn a_degraded_project_cannot_pass_as_a_resolved_one() {
        // The degraded arm's fields are a superset of the resolved arm's, so an
        // untagged enum declared in the wrong order reads *every* degraded
        // project as resolved and drops the reason. That mutant would let a
        // project AO cannot resolve authorize a Codex launch.
        let degraded: AoProjectGetResponse = serde_json::from_str(
            r#"{"status":"degraded","project":{"id":"p1","name":"n","path":"/w/p",
                "resolveError":"worktree missing"}}"#,
        )
        .expect("a degraded envelope");
        assert_eq!(degraded.status, AoProjectStatus::Degraded);
        assert!(matches!(degraded.project, AoProjectOrDegraded::Degraded(_)));
        assert!(
            degraded.resolved().is_err(),
            "a degraded project authorizes nothing"
        );

        let resolved: AoProjectGetResponse = serde_json::from_str(
            r#"{"status":"ok","project":{"id":"p1","name":"n","path":"/w/p","repo":"r",
                "defaultBranch":"main","kind":"single_repo",
                "config":{"agentConfig":{"permissions":"accept-edits"}}}}"#,
        )
        .expect("a resolved envelope");
        assert_eq!(
            resolved.resolved().expect("a resolved project").path,
            "/w/p"
        );

        // A response that contradicts itself resolves to nothing either way.
        let lying: AoProjectGetResponse = serde_json::from_str(
            r#"{"status":"ok","project":{"id":"p1","name":"n","path":"/w/p",
                "resolveError":"worktree missing"}}"#,
        )
        .expect("a self-contradicting envelope still parses");
        assert!(lying.resolved().is_err());
    }

    #[test]
    fn sse_frames_are_read_from_their_payload_not_their_header() {
        // The `id:` header is a projection of `seq`. A recording where they
        // disagree must be read by the payload, so a header cannot smuggle a
        // sequence the event does not carry.
        let events = parse_sse_events(
            "id: 99\nevent: session_updated\ndata: {\"seq\":7,\"projectId\":\"p1\",\
             \"sessionId\":\"s1\",\"type\":\"session_updated\",\
             \"createdAt\":\"2026-08-10T09:00:00Z\"}\n\n: keep-alive\n\n",
        )
        .expect("the recording parses");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 7);
        assert!(events[0].event_type.is_session_lifecycle());
    }

    #[test]
    fn base64_length_is_computed_without_keeping_the_bytes() {
        assert_eq!(mux::decoded_base64_len("AAAA").expect("4 chars"), 3);
        assert_eq!(mux::decoded_base64_len("AAA=").expect("one pad"), 2);
        assert_eq!(mux::decoded_base64_len("AA==").expect("two pads"), 1);
        assert!(mux::decoded_base64_len("AAA").is_err(), "partial quantum");
        assert!(mux::decoded_base64_len("A=A=").is_err(), "interior pad");
        assert!(mux::decoded_base64_len("A A A").is_err(), "not base64");
    }
}
