//! The Paseo 0.3.1 wire model: session frames, CLI JSON, and the one place a
//! native timeline entry becomes a [`SessionEvent`].
//!
//! Two surfaces, deliberately typed apart:
//!
//! * **Session frames** ([`PaseoWorkspace`], [`PaseoAgent`],
//!   [`PaseoTimelinePage`]) are the authoritative readback, and every binding,
//!   launch, resume and adoption decision is made from these.
//! * **CLI JSON** ([`PaseoCliWorkspaceCreated`], [`PaseoCliAgentStarted`]) is
//!   what `paseo … --json` prints. It is *thin on purpose* — 0.3.1's
//!   `workspace create --json` prints five display columns and no project id,
//!   and `agent run --json` prints an id, a status, a provider, a cwd and a
//!   title, with no workspace, no labels and no parent. Typing it thin is what
//!   makes "trust the CLI and skip the readback" fail to compile rather than
//!   fail in production.
//!
//! Nothing here talks to anything. Normalization is pure so the interesting
//! cases — a collapsed projection, a declared gap, an epoch change, an unknown
//! item — are decided by a fixture rather than by a daemon's mood.
//!
//! # What 0.3.1 changed, and what that costs
//!
//! * The WebSocket protocol number and the application version are independent
//!   pins ([`PASEO_WS_PROTOCOL_VERSION`], [`PASEO_APP_VERSION`]).
//! * An agent snapshot carries **no** `projectId` and **no** `parentAgentId`
//!   field. Placement in a project is proved through the agent's workspace, and
//!   native parentage is only ever the [`label::PARENT_AGENT`] label. Kontor
//!   launches top-level agents into an already-attested workspace, so any such
//!   label on a Kontor seat is foreign ownership and must be refused.
//! * A workspace carries **no labels at all**. Kontor keeps native bindings in
//!   its own durable store and leaves the title human-readable.
//! * The lifecycle enum is `initializing | idle | running | error | closed`;
//!   retirement is the `archivedAt` stamp rather than a status.
//! * A timeline entry spans `seqStart..=seqEnd` over explicit
//!   `sourceSeqRanges`, and the page — not the stream — declares `reset`,
//!   `staleCursor` and `gap`.

use std::collections::BTreeMap;

use kontor_core::id::{CanonicalDocument, ContentHash, ExternalId, Timestamp};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use kontor_runtime::request::MessageId;
use kontor_runtime::timeline::{
    EventSubject, SessionEvent, SessionEventKind, TimelineBreak, TimelinePosition,
};
use serde::{Deserialize, Serialize};

/// The WebSocket protocol number this adapter speaks, sent in the hello.
///
/// Independent of [`PASEO_APP_VERSION`]: the daemon closes the socket outright
/// on a protocol mismatch, while an app/daemon version it does not recognize is
/// this adapter's own refusal.
pub const PASEO_WS_PROTOCOL_VERSION: u64 = 1;

/// The Paseo application release this adapter's DTOs, fixtures and argv
/// evidence are recorded from, and the **lowest** release it will drive.
///
/// A floor rather than an equality, and the distinction is not cosmetic. An
/// exact pin cannot tell "a release we have not seen" from "a release we cannot
/// speak to", so every upgrade of the app degraded the entire fleet at once:
/// bindings frozen under the previous release assert capabilities the
/// now-unrecognized daemon is not credited with, and
/// [`crate::adapter::PaseoAdapter`] then refuses to attest a single one of them.
/// Paseo `0.4.0` did exactly that, and nothing about the wire had changed.
///
/// It stays at the release the fixtures are recorded from, because that is the
/// oldest daemon this adapter is *proven* against — raising it would strand a
/// daemon known to work. A newer one is driven at full capability until
/// something it actually removed proves otherwise, which is `REQUIRED_FEATURES`'
/// job: that check is per-feature and per-connection, so a genuine removal still
/// degrades correctly without a version ever being named.
pub const PASEO_APP_VERSION: &str = "0.3.1";

/// The first Paseo release carrying the correlated project-rename envelope.
///
/// Paseo 0.4.0 implements `project.rename.request` but its server-info feature
/// object does not advertise `projectRename`. Keep the general protocol floor
/// at the recorded 0.3.1 fixture baseline while recognizing this one optional
/// operation at the release that introduced it. An explicit future feature
/// flag remains authoritative too.
pub const PASEO_PROJECT_RENAME_VERSION: &str = "0.4.0";

/// The release that gained per-agent environment on `agent run`.
///
/// `paseo agent run --help` on 0.6.1 documents
/// `--env <key=value>  Set environment variable(s) for the agent process (can be
/// used multiple times)`. There is no `status/server_info` flag for it, so the
/// release floor is the compatibility evidence — and it is read from the
/// *daemon's* reported version, never from the CLI's: a CLI that accepts a flag
/// an older daemon ignores would launch a seat with none of its environment and
/// no error to say so.
pub const PASEO_SEAT_ENVIRONMENT_VERSION: &str = "0.6.1";

/// The release whose `create_agent_request` carries typed per-agent
/// `providerOptions`.
///
/// A **separate contract** from [`PASEO_SEAT_ENVIRONMENT_VERSION`], and
/// deliberately not folded into it: `--env` sets a process environment, while
/// this is a validated provider-native policy the daemon persists and replays
/// into every turn. A daemon could plausibly have one and not the other, and
/// letting an environment capability stand in for provider-options support
/// would be exactly the kind of substitution that launches a seat under a
/// policy nothing carried.
///
/// Read out of the installed 0.6.1 bundle: `AgentSessionConfigSchema` carries
/// `providerOptions`, `applyProviderConfiguration` validates it against the
/// provider's own schema, and `opencode-agent.js` replays it into
/// `session.promptAsync`. Earlier releases were not inspected, so the floor is
/// the version this was actually read from and anything below it fails closed.
pub const PASEO_PROVIDER_OPTIONS_VERSION: &str = "0.6.1";

/// The client type this adapter announces in the hello.
pub const PASEO_CLIENT_TYPE: &str = "cli";

/// The client capability that narrows agent streams to an explicit viewed set.
///
/// Spelled exactly as the daemon's `CLIENT_CAPS` table does — snake_case, not
/// the camelCase the *server* feature list uses for the same idea. Sending the
/// wrong spelling is silently accepted (the capability object is passthrough)
/// and silently does nothing, which is the worst of both.
pub const PASEO_CAP_SELECTIVE_AGENT_TIMELINE: &str = "selective_agent_timeline";

/// The most CLI stdout this adapter will read before calling the answer a
/// malfunction.
pub const MAX_OUTPUT_BYTES: usize = 1 << 20;

/// The most bytes one daemon frame may carry.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// The largest message body this adapter will hand to Paseo.
pub const MAX_MESSAGE_BYTES: u64 = 64 * 1024;

/// The largest canonical history page this adapter will ask for.
pub const MAX_HISTORY_PAGE: u32 = 500;

/// The largest directory page this adapter will ask for.
///
/// The daemon caps `page.limit` at 200 and refuses more, so this is its number
/// rather than a preference.
pub const MAX_DIRECTORY_PAGE: u32 = 200;

/// How many directory pages one bounded enumeration will follow.
pub const MAX_DIRECTORY_PAGES: usize = 8;

/// How many unsolicited frames one agent's queue holds before the oldest are
/// dropped and the drain reports a gap.
pub const MAX_STREAM_QUEUE: usize = 512;

/// The most characters of an unaudited native string that reach a payload.
///
/// An unknown item type is diagnostics, and diagnostics from an agent session
/// can quote the operator's prompt. Bounding it is what keeps "we did not
/// recognize this" from becoming a transcript leak into Kontor's own storage.
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
    /// Native Paseo parentage, when Paseo launched the agent under another
    /// native agent.
    ///
    /// Paseo's own key, and the only place 0.3.1 records parentage at all. It
    /// is deliberately absent from [`ALL`]: Kontor owns the logical seat and
    /// launches it top-level into the exact attested workspace. A value here
    /// therefore identifies foreign native ownership rather than a label for
    /// Kontor to plant.
    pub const PARENT_AGENT: &str = "paseo.parent-agent-id";
    /// Family-qualified Advisor/Committee run id.
    pub const CONSULTATION_RUN: &str = "kontor.consultation_run";
    /// Exact persistent consultation seat.
    pub const SEAT_BINDING: &str = "kontor.seat_binding_id";
    /// Persistent non-delivery topology seat (for example LSA/TPM).
    pub const HOSTED_SEAT: &str = "kontor.hosted_seat";
    /// The digest of the posture, owned config and environment a seat launched
    /// under.
    ///
    /// A hash, never the values: labels are readable by anyone who can list
    /// agents. It exists so a launch whose acknowledgement was lost can only be
    /// adopted by a census that finds *this* posture — an agent carrying the
    /// right task and slot but no matching digest was launched under something
    /// else, or by something else, and is not this seat.
    pub const SEAT_POSTURE: &str = "kontor.seat_posture";
    /// The digest of one exact launch intent.
    ///
    /// Binding, agent run, place, slot and posture, hashed together. A census
    /// looking for a launch whose acknowledgement was lost matches on this, so
    /// an agent that merely shares a task, a workspace or a slot — a
    /// predecessor, a neighbouring seat, a similarly-labelled leftover — is not
    /// mistaken for the one this launch created. A hash, never the values.
    pub const LAUNCH_INTENT: &str = "kontor.launch_intent";
    /// The logical seat a still-live predecessor formerly filled.
    ///
    /// Paseo's public metadata update surface patches string values and cannot
    /// delete a label. A takeover therefore releases the canonical
    /// `SEAT_BINDING` value and records its provenance here rather than writing
    /// internal daemon state to erase it.
    pub const FORMER_SEAT_BINDING: &str = "kontor.former_seat_binding_id";
    /// Seat whose canonical title this non-owning session released.
    ///
    /// This marker makes a partially-applied title cleanup discoverable on a
    /// retry, so the preview hash remains stable after a lost acknowledgement.
    pub const TITLE_RELEASED_FOR: &str = "kontor.title_released_for_seat_binding_id";
    /// Explicit non-mutating authority marker for consultation sessions.
    pub const READ_ONLY: &str = "kontor.read_only";
    /// Canonical profile hash for a risk-accepted behavioral fallback.
    ///
    /// Its presence deliberately replaces `READ_ONLY`; the two labels are
    /// mutually exclusive so metadata never claims OS containment for a route
    /// whose restriction depends on model behavior.
    pub const OPERATOR_ACCEPTED_FALLBACK: &str = "kontor.operator_accepted_fallback";

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
    /// Paseo 0.3.1 does **not** advertise this. The bundled client contains an
    /// internal rename method, and this adapter never calls it: writing another
    /// owner's internal state to improve a display string is the trade nothing
    /// justifies. Name drift is reported as
    /// [`crate::adapter::PaseoProjectOutcome::ReadyWithRenamePending`] instead.
    ProjectRename,
    /// A session's context can be compacted through a supported operation.
    ///
    /// Also absent in 0.3.1, and also never simulated with a reload or a
    /// replacement.
    Compaction,
}

impl PaseoFeature {
    /// The wire spelling the daemon advertises in `status/server_info`.
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

/// What the daemon says about itself, pushed as `status/server_info` right
/// after the hello.
///
/// Not a request. 0.3.1 volunteers this exactly once per accepted connection,
/// so it is connection identity: the adapter holds the pushed copy and refuses
/// to drive anything until it has one that agrees with the pins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoServerInfo {
    /// The daemon's own identity for this boot.
    #[serde(default, rename = "serverId")]
    pub server_id: String,
    /// The daemon version string, when it reports one.
    #[serde(default)]
    pub version: Option<String>,
    /// The daemon's hostname, carried as evidence and never matched on.
    #[serde(default)]
    pub hostname: Option<String>,
    /// The advertised feature flags, verbatim.
    ///
    /// An object of booleans in 0.3.1, so "advertised" means *present and
    /// true*: a daemon that reports `projectList: false` has answered the
    /// question, and reading mere presence as support would drive it anyway.
    #[serde(default)]
    pub features: BTreeMap<String, bool>,
}

impl PaseoServerInfo {
    /// Whether `feature` is advertised as available.
    #[must_use]
    pub fn supports(&self, feature: PaseoFeature) -> bool {
        self.features.get(feature.as_str()) == Some(&true)
    }

    /// Whether this exact daemon connection supports native project retitle.
    ///
    /// The explicit feature flag is preferred. Paseo 0.4.0 shipped the typed,
    /// correlated request/response handler without adding that flag to
    /// `status/server_info`, so the release floor is the compatibility evidence
    /// for that build. Older, pre-release and unparseable versions still fail
    /// closed.
    #[must_use]
    pub fn supports_project_rename(&self) -> bool {
        self.supports(PaseoFeature::ProjectRename)
            || self
                .version
                .as_deref()
                .is_some_and(|version| version_at_least(version, PASEO_PROJECT_RENAME_VERSION))
    }

    /// Whether this daemon applies per-agent environment given on `agent run`.
    ///
    /// Fails closed on an absent, pre-release or unparseable version: a seat
    /// whose posture rides on `--env` must not launch where the flag might be
    /// accepted and dropped.
    #[must_use]
    pub fn supports_seat_environment(&self) -> bool {
        self.version
            .as_deref()
            .is_some_and(|version| version_at_least(version, PASEO_SEAT_ENVIRONMENT_VERSION))
    }

    /// Whether this daemon accepts typed per-agent `providerOptions`.
    ///
    /// Fails closed on an absent, pre-release or unparseable version. Asked
    /// separately from [`Self::supports_seat_environment`]: a seat whose policy
    /// rides in `providerOptions` must not launch because the daemon happens to
    /// support a different mechanism.
    #[must_use]
    pub fn supports_provider_options(&self) -> bool {
        self.version
            .as_deref()
            .is_some_and(|version| version_at_least(version, PASEO_PROVIDER_OPTIONS_VERSION))
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

    /// Whether this daemon is at or above the supported application baseline.
    ///
    /// A daemon that reports no version at all is *not* supported. Absence is
    /// not agreement, and every DTO below was recorded from a build that says
    /// which one it is.
    #[must_use]
    pub fn is_supported_baseline(&self) -> bool {
        self.version
            .as_deref()
            .is_some_and(|version| version_at_least(version, PASEO_APP_VERSION))
    }
}

/// Whether `reported` is at least `floor`, as `MAJOR.MINOR.PATCH`.
///
/// Numeric per component rather than lexical, because text order is wrong
/// exactly where it costs most: `"0.10.0" < "0.4.0"` as strings, so a fleet
/// comparing that way would degrade itself on the tenth minor release — the
/// same outage the floor exists to prevent.
///
/// A pre-release sorts *below* the release it is named for, so `0.4.0-beta.2`
/// does not satisfy a floor of `0.4.0`. Anything that does not parse is not at
/// least anything.
#[must_use]
pub fn version_at_least(reported: &str, floor: &str) -> bool {
    let (Some(left), Some(right)) = (release_triple(reported), release_triple(floor)) else {
        return false;
    };
    if left != right {
        return left > right;
    }
    // The same release: only a final build clears a floor set at it.
    !reported.contains('-')
}

/// `MAJOR.MINOR.PATCH` as numbers, discarding any pre-release or build suffix.
///
/// A fourth component is rejected rather than truncated: it is not a version
/// this adapter knows how to order, and ordering it anyway would be a guess.
fn release_triple(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// The answer to `daemon.get_status.request`.
///
/// The version readback, and the one operation whose whole purpose is to ask
/// the daemon what it is a second time, over a correlated request rather than a
/// push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoDaemonStatus {
    /// The daemon's own identity for this boot.
    #[serde(default, rename = "serverId")]
    pub server_id: String,
    /// The daemon version string.
    #[serde(default)]
    pub version: Option<String>,
    /// Where it listens, when it says.
    #[serde(default)]
    pub listen: Option<String>,
}

// ---------------------------------------------------------------------------
// Projects, workspaces, agents — session readback
// ---------------------------------------------------------------------------

/// One Paseo project, as `project.list.response` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoProject {
    /// The stable project id. This is the epic binding, and the only one.
    #[serde(rename = "projectId")]
    pub id: String,
    /// The resolved display name, which is data and never authority.
    #[serde(default, rename = "projectDisplayName")]
    pub display_name: String,
    /// The Git remote the project was registered from.
    ///
    /// Read but never matched on: the live daemon holds several projects for
    /// one remote, so selecting by it would bind an epic to somebody else's
    /// project (ALT-003).
    #[serde(default, rename = "projectKey")]
    pub project_key: Option<String>,
    /// The project's root directory.
    #[serde(default, rename = "projectRootPath")]
    pub root_path: String,
}

/// The answer to `project.list.request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoProjectList {
    /// Every project the daemon owns.
    #[serde(default)]
    pub projects: Vec<PaseoProject>,
}

/// The answer to `project.add.request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoProjectAdded {
    /// The project, when the daemon made one.
    #[serde(default)]
    pub project: Option<PaseoProject>,
    /// The daemon's own error text, when it refused.
    #[serde(default)]
    pub error: Option<String>,
}

/// The answer to `project.rename.request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoProjectRenamed {
    /// Correlated request id echoed by Paseo.
    #[serde(rename = "requestId")]
    pub request_id: String,
    /// Exact project whose custom title was addressed.
    #[serde(rename = "projectId")]
    pub project_id: String,
    /// Whether Paseo accepted the supported rename operation.
    pub accepted: bool,
    /// Stored custom title after the operation.
    #[serde(default, rename = "customName")]
    pub custom_name: Option<String>,
    /// Native refusal detail, when not accepted.
    #[serde(default)]
    pub error: Option<String>,
}

/// What kind of place a workspace is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaseoWorkspaceKind {
    /// A registered Git worktree. The only kind a ticket role may occupy.
    Worktree,
    /// A checkout Paseo tracks as a branch of the project.
    Checkout,
    /// A plain local checkout, typically the project root.
    LocalCheckout,
    /// A plain directory.
    Directory,
    /// Anything else this adapter has not audited.
    #[serde(other)]
    Other,
}

/// The Git facts a workspace descriptor carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoGitRuntime {
    /// Whether Paseo provisioned the worktree itself.
    #[serde(default, rename = "isPaseoOwnedWorktree")]
    pub is_paseo_owned_worktree: bool,
}

/// One Paseo workspace, as `fetch_workspaces_response` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoWorkspace {
    /// The native workspace id.
    #[serde(default)]
    pub id: String,
    /// The project it belongs to. The CLI omits this, which is the whole reason
    /// the session readback is mandatory.
    #[serde(default, rename = "projectId")]
    pub project_id: String,
    /// The absolute working directory.
    #[serde(default, rename = "workspaceDirectory")]
    pub workspace_directory: String,
    /// What kind of place it is.
    #[serde(rename = "workspaceKind")]
    pub workspace_kind: PaseoWorkspaceKind,
    /// The resolved display name.
    #[serde(default)]
    pub name: String,
    /// The raw title override, when the workspace has one.
    #[serde(default)]
    pub title: Option<String>,
    /// Git facts, including whether Paseo made this tree for itself.
    ///
    /// `true` means Paseo provisioned a tree of its own rather than registering
    /// the task worktree Kontor prepared, which is a different place than the
    /// one the run was admitted for.
    #[serde(default, rename = "gitRuntime")]
    pub git_runtime: Option<PaseoGitRuntime>,
}

impl PaseoWorkspace {
    /// Whether Paseo provisioned this worktree for itself.
    #[must_use]
    pub fn is_paseo_owned_worktree(&self) -> bool {
        self.git_runtime
            .as_ref()
            .is_some_and(|git| git.is_paseo_owned_worktree)
    }

    /// The title a human sees for this workspace.
    ///
    /// The raw override when it has one, and the resolved name otherwise.
    #[must_use]
    pub fn visible_title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// One page of `fetch_workspaces_response`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoWorkspacePage {
    /// The workspaces on this page.
    #[serde(default)]
    pub entries: Vec<PaseoWorkspace>,
    /// Where the next page starts, when there is one.
    #[serde(default, rename = "pageInfo")]
    pub page_info: PaseoPageInfo,
}

/// A directory page's continuation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoPageInfo {
    /// The cursor the next page starts at.
    #[serde(default, rename = "nextCursor", alias = "afterCursor")]
    pub next_cursor: Option<String>,
    /// Whether the daemon says more rows exist.
    #[serde(default, rename = "hasMore", alias = "hasMoreAfter")]
    pub has_more: bool,
}

impl PaseoPageInfo {
    /// The cursor to continue from, or `None` when the enumeration is done.
    ///
    /// Both halves have to agree. A daemon that says `hasMore` without a cursor
    /// has not given a way on, and following a cursor it did not offer is how a
    /// bounded loop stops being bounded.
    #[must_use]
    pub fn next(&self) -> Option<&str> {
        match (&self.next_cursor, self.has_more) {
            (Some(cursor), true) if !cursor.is_empty() => Some(cursor),
            _ => None,
        }
    }
}

/// What Paseo says an agent is doing.
///
/// The whole 0.3.1 lifecycle enum. Retirement is *not* in it — an archived
/// agent is one with an `archivedAt` stamp, whatever its status says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaseoAgentStatus {
    /// Starting up.
    Initializing,
    /// Alive and reusable, with nothing in flight.
    Idle,
    /// Working.
    Running,
    /// The session reported an error and is waiting for attention.
    Error,
    /// The underlying process is gone; the seat is not retired.
    Closed,
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
        matches!(self, Self::Running | Self::Idle | Self::Initializing)
    }

    /// Whether a process restart is needed before this agent can continue.
    #[must_use]
    pub const fn needs_reload(self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Whether the native runtime is presently executing or retaining a live
    /// process for this seat.
    ///
    /// Persistent `idle` seats are resumable identities, not simultaneous work.
    /// Counting them against launch capacity makes every completed turn spend a
    /// slot forever and eventually prevents unrelated epics from starting.
    #[must_use]
    pub const fn occupies_concurrent_capacity(self) -> bool {
        matches!(self, Self::Initializing | Self::Running | Self::Error)
    }
}

/// One permission request an agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoPendingPermission {
    /// The request id, which is what an answer is bound to.
    pub id: String,
}

/// The provider handle an agent session is running under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoPersistence {
    /// The provider's own session id.
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
}

/// One Paseo agent, as a session readback reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoAgent {
    /// The native agent id — the session identity Kontor correlates against.
    pub id: String,
    /// The provider Paseo actually selected.
    #[serde(default)]
    pub provider: String,
    /// The model Paseo actually selected.
    #[serde(default)]
    pub model: String,
    /// The requested thinking option, when one was selected.
    #[serde(default, rename = "thinkingOptionId")]
    pub thinking_option_id: Option<String>,
    /// The effective thinking option Paseo actually applied.
    #[serde(default, rename = "effectiveThinkingOptionId")]
    pub effective_thinking_option_id: Option<String>,
    /// The provider-native permission mode Paseo actually applied.
    #[serde(default, rename = "currentModeId")]
    pub current_mode_id: Option<String>,
    /// The workspace it runs in.
    ///
    /// Optional on the wire, and its absence is a refusal rather than a
    /// default: an agent with no workspace has no provable project, and every
    /// placement rule here is decided through the workspace.
    #[serde(default, rename = "workspaceId")]
    pub workspace_id: Option<String>,
    /// Its working directory.
    #[serde(default)]
    pub cwd: String,
    /// The display title.
    #[serde(default)]
    pub title: Option<String>,
    /// Every correlation label, verbatim.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// What Paseo says it is doing.
    pub status: PaseoAgentStatus,
    /// When it was retired, if it was. The only retirement evidence 0.3.1 has.
    #[serde(default, rename = "archivedAt")]
    pub archived_at: Option<String>,
    /// Paseo's own attention hint, e.g. `finished`.
    ///
    /// Carried as evidence and never promoted: `finished` on an idle agent is
    /// Paseo saying the last turn ended, not that the run did.
    #[serde(default, rename = "attentionReason")]
    pub attention_reason: Option<String>,
    /// The provider handle, when Paseo exposes one.
    #[serde(default)]
    pub persistence: Option<PaseoPersistence>,
    /// Every permission request this session is waiting on.
    ///
    /// 0.3.1's canonical timeline carries no permission items at all, so this
    /// list *is* the permission ledger's evidence: a request that is here is
    /// open, and one that has left is resolved.
    #[serde(default, rename = "pendingPermissions")]
    pub pending_permissions: Vec<PaseoPendingPermission>,
}

impl PaseoAgent {
    /// One label, if present.
    #[must_use]
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }

    /// The orchestrator this agent was launched under, as Paseo recorded it.
    ///
    /// A label rather than a field, because 0.3.1's agent snapshot has no
    /// `parentAgentId`. The 0.2.5 adapter checked the raw field *and* the
    /// planted label and called the seat proven only when both agreed; that
    /// second, independent half no longer exists on this wire, and pretending
    /// otherwise by reading one value twice would be a check that cannot fail.
    #[must_use]
    pub fn parent_agent_id(&self) -> Option<&str> {
        self.label(label::PARENT_AGENT)
    }

    /// The provider's own session id, when Paseo exposes one.
    #[must_use]
    pub fn provider_session_id(&self) -> Option<&str> {
        self.persistence
            .as_ref()
            .and_then(|handle| handle.session_id.as_deref())
    }

    /// Whether this agent has been retired out of its seat.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
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

/// The answer to `send_agent_message_request`.
///
/// `accepted` is the acknowledgement a delivery seat is admitted on. The
/// installed 0.6.1 replays the agent's persisted `providerOptions.permission`
/// into `session.promptAsync` for that turn, and OpenCode installs those rules
/// on the session before it evaluates a tool call — so a turn the daemon accepts
/// is a turn that ran under the policy Kontor sent, and a seat is bound on
/// nothing weaker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoMessageAccepted {
    /// The correlation id of the send.
    #[serde(default, rename = "requestId")]
    pub request_id: String,
    /// The agent the daemon accepted it for.
    #[serde(default, rename = "agentId")]
    pub agent_id: String,
    /// Whether the daemon took the turn.
    #[serde(default)]
    pub accepted: bool,
    /// The daemon's own refusal text, when it did not.
    #[serde(default)]
    pub error: Option<String>,
}

impl PaseoMessageAccepted {
    /// Whether this answer authorizes binding the seat it was sent for.
    ///
    /// `accepted` alone is not enough. It is a boolean on a frame, and a frame
    /// that belongs to a different send — a retry, a neighbouring seat, a
    /// late-arriving answer to a request this launch already gave up on —
    /// carries exactly the same `true`. Binding on that would admit a seat whose
    /// first turn nobody watched. So the correlation is checked first, exactly:
    /// the answer must name the request that was sent and the agent it was sent
    /// to, and only then does acceptance mean anything.
    #[must_use]
    pub fn authorizes(&self, request_id: &str, agent_id: &str) -> bool {
        self.request_id == request_id && self.agent_id == agent_id && self.accepted
    }
}

/// The answer to `fetch_agent_request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoAgentAnswer {
    /// The agent, when the daemon holds one under that id.
    #[serde(default)]
    pub agent: Option<PaseoAgent>,
    /// The daemon's own error text, when it does not.
    #[serde(default)]
    pub error: Option<String>,
}

/// One row of `fetch_agents_response`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoAgentEntry {
    /// The agent.
    pub agent: PaseoAgent,
}

/// One page of `fetch_agents_response`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoAgentPage {
    /// The rows on this page.
    #[serde(default)]
    pub entries: Vec<PaseoAgentEntry>,
    /// Where the next page starts, when there is one.
    #[serde(default, rename = "pageInfo")]
    pub page_info: PaseoPageInfo,
}

/// The answer to `send_agent_message_request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoSendAccepted {
    /// The agent it was delivered into, as the daemon resolved it.
    #[serde(default, rename = "agentId")]
    pub agent_id: String,
    /// Whether the daemon took it.
    #[serde(default)]
    pub accepted: bool,
    /// Why not, when it did not.
    #[serde(default)]
    pub error: Option<String>,
}

/// The answer to `agent.timeline.set_subscription.request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoSubscriptionAck {
    /// Exactly the agents this connection is now subscribed to.
    #[serde(default, rename = "agentIds")]
    pub agent_ids: Vec<String>,
}

/// The resolution `agent_permission_resolved` reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoPermissionResolution {
    /// `allow` or `deny`, verbatim.
    #[serde(default)]
    pub behavior: String,
}

/// The frame that answers an `agent_permission_response`.
///
/// Correlated by the *permission request id* rather than by a separate
/// correlation id, because that is the only id 0.3.1 carries on both halves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoPermissionResolved {
    /// The agent whose session raised the request.
    #[serde(default, rename = "agentId")]
    pub agent_id: String,
    /// The answer Paseo recorded.
    #[serde(default)]
    pub resolution: PaseoPermissionResolution,
}

// ---------------------------------------------------------------------------
// CLI JSON — deliberately thinner than the session protocol
// ---------------------------------------------------------------------------

/// `paseo workspace create --json`.
///
/// No `projectId`, no labels. That absence is load-bearing: a workspace this
/// adapter has only seen through the CLI has not been proved to live in the
/// epic project, so it may not be bound (REQ-016, MUT-016).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliWorkspaceCreated {
    /// The native workspace id.
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    /// The resolved display name.
    #[serde(default)]
    pub name: String,
    /// The directory the CLI reports.
    #[serde(default)]
    pub cwd: String,
}

/// `paseo agent run --json`.
///
/// No workspace, no labels, no parent, no provider session. Same rule as above.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliAgentStarted {
    /// The native agent id.
    #[serde(rename = "agentId")]
    pub agent_id: String,
    /// `created` or `running`, as the CLI spells it.
    #[serde(default)]
    pub status: String,
}

/// `paseo agent update --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliAgentUpdated {
    /// The native agent id the CLI acknowledged.
    #[serde(rename = "agentId")]
    pub agent_id: String,
}

/// `paseo agent reload --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliAgentReloaded {
    /// The native agent id the CLI acknowledged.
    #[serde(rename = "agentId")]
    pub agent_id: String,
    /// `reloaded`, as the CLI spells it.
    #[serde(default)]
    pub status: String,
}

/// `paseo agent archive --json` and `paseo workspace archive --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliArchived {
    /// The agent id, on an agent archive.
    #[serde(default, rename = "agentId")]
    pub agent_id: Option<String>,
    /// The workspace id, on a workspace archive.
    #[serde(default, rename = "workspaceId")]
    pub workspace_id: Option<String>,
    /// When Paseo says it was archived. Absent means it was not.
    #[serde(default, rename = "archivedAt")]
    pub archived_at: Option<String>,
}

/// `paseo agent stop --json`.
///
/// A count and the ids, because 0.3.1's stop is a bulk operation whose single-id
/// form is one row of the same answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoCliStopped {
    /// Every agent the CLI interrupted.
    #[serde(default, rename = "agentIds")]
    pub agent_ids: Vec<String>,
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

/// Which way a timeline read runs from its cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaseoDirection {
    /// The newest window, with no cursor.
    Tail,
    /// Strictly older than the cursor.
    Before,
    /// Strictly newer than the cursor. The resume read.
    After,
}

impl PaseoDirection {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tail => "tail",
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

/// A position in one agent's content, as 0.3.1 spells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoTimelineCursor {
    /// The raw epoch, which Paseo spells as a UUID.
    pub epoch: String,
    /// The native sequence inside that epoch.
    pub seq: u64,
}

/// One contiguous run of native sequences an entry was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoSeqRange {
    /// First native sequence, inclusive.
    #[serde(rename = "startSeq")]
    pub start_seq: u64,
    /// Last native sequence, inclusive.
    #[serde(rename = "endSeq")]
    pub end_seq: u64,
}

/// The content of one timeline entry, typed down to what Kontor may read.
///
/// Deliberately partial. `text`, tool detail, reasoning and todo bodies are the
/// operator's work; nothing here can carry them into a Kontor payload because
/// nothing here has a field for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoTimelineItem {
    /// The native item kind, verbatim.
    #[serde(default, rename = "type")]
    pub item_type: String,
    /// The caller-supplied id, echoed on the resulting user message.
    ///
    /// This is Paseo's idempotency echo and the whole basis of exactly-once
    /// delivery: `send_agent_message_request.messageId` comes back *here*, not
    /// in `messageId`, which is the provider's own.
    #[serde(default, rename = "clientMessageId")]
    pub client_message_id: Option<String>,
    /// The provider's own message id, which is never a Kontor one.
    #[serde(default, rename = "messageId")]
    pub message_id: Option<String>,
    /// The tool call this entry is about.
    #[serde(default, rename = "callId")]
    pub call_id: Option<String>,
}

/// One canonical timeline entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoTimelineEntry {
    /// The content.
    #[serde(default)]
    pub item: PaseoTimelineItem,
    /// When Paseo emitted it.
    #[serde(default)]
    pub timestamp: String,
    /// First native sequence this entry covers.
    #[serde(rename = "seqStart")]
    pub seq_start: u64,
    /// Last native sequence this entry covers.
    #[serde(rename = "seqEnd")]
    pub seq_end: u64,
    /// Exactly which native sequences went into it.
    #[serde(default, rename = "sourceSeqRanges")]
    pub source_seq_ranges: Vec<PaseoSeqRange>,
    /// Which merges the projection applied, if any.
    #[serde(default)]
    pub collapsed: Vec<String>,
}

impl PaseoTimelineEntry {
    /// Whether this entry is exactly one native sequence, built from exactly
    /// that sequence.
    ///
    /// The 0.3.1 shape of "is this canonical?". A `projected` read folds a whole
    /// tool lifecycle into one entry spanning two sequences, and the source
    /// ranges say so — so the check is on the ranges rather than on the
    /// projection field the daemon echoed back, which is only a claim about
    /// what was asked for.
    #[must_use]
    pub fn is_single_sequence(&self) -> bool {
        self.collapsed.is_empty()
            && self.seq_start == self.seq_end
            && self.source_seq_ranges.len() == 1
            && self.source_seq_ranges[0].start_seq == self.seq_start
            && self.source_seq_ranges[0].end_seq == self.seq_end
    }
}

/// One page of canonical timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoTimelinePage {
    /// The agent this page is about.
    #[serde(default, rename = "agentId")]
    pub agent_id: String,
    /// Which way the read ran.
    pub direction: PaseoDirection,
    /// Which projection answered.
    pub projection: PaseoProjection,
    /// The raw epoch this page's sequences are numbered in.
    #[serde(default)]
    pub epoch: String,
    /// The daemon renumbered this session's content.
    #[serde(default)]
    pub reset: bool,
    /// The cursor the read started from no longer exists.
    #[serde(default, rename = "staleCursor")]
    pub stale_cursor: bool,
    /// The daemon knows it dropped entries.
    #[serde(default)]
    pub gap: bool,
    /// The entries, in ascending sequence order.
    #[serde(default)]
    pub entries: Vec<PaseoTimelineEntry>,
    /// The first position on this page.
    #[serde(default, rename = "startCursor")]
    pub start_cursor: Option<PaseoTimelineCursor>,
    /// The last position on this page — the exact strict-after anchor.
    #[serde(default, rename = "endCursor")]
    pub end_cursor: Option<PaseoTimelineCursor>,
    /// Whether older content exists before this window.
    #[serde(default, rename = "hasOlder")]
    pub has_older: bool,
    /// Whether newer content exists after this window.
    #[serde(default, rename = "hasNewer")]
    pub has_newer: bool,
    /// The daemon's own error text, when the read failed.
    #[serde(default)]
    pub error: Option<String>,
}

impl PaseoTimelinePage {
    /// The break this page declares, if any.
    ///
    /// Every one of them ends delivery and demands a canonical refetch. None of
    /// them says anything about the run, which is why this maps to a break and
    /// never to a lifecycle state.
    #[must_use]
    pub const fn declared_break(&self) -> Option<TimelineBreak> {
        if self.reset || self.stale_cursor {
            Some(TimelineBreak::EpochChanged)
        } else if self.gap {
            Some(TimelineBreak::SequenceGap)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Unsolicited stream frames
// ---------------------------------------------------------------------------

/// The permission request an `agent_stream` event carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoStreamPermissionRequest {
    /// The request id an answer must be bound to.
    pub id: String,
}

/// What one `agent_stream` frame is about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoStreamEvent {
    /// The native event kind, verbatim.
    #[serde(default, rename = "type")]
    pub event_type: String,
    /// The timeline content, on a `timeline` event.
    #[serde(default)]
    pub item: Option<PaseoTimelineItem>,
    /// The permission request, on a `permission_requested` event.
    #[serde(default)]
    pub request: Option<PaseoStreamPermissionRequest>,
    /// The permission request id, on a `permission_resolved` event.
    #[serde(default, rename = "requestId")]
    pub request_id: Option<String>,
}

/// One frame off the selective `agent_stream` subscription.
///
/// Never a request answer. These arrive unsolicited on the same socket as every
/// response, which is why the transport routes them by `agentId` into a bounded
/// per-agent queue instead of handing them to whichever request is pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaseoStreamFrame {
    /// The agent the frame belongs to.
    #[serde(default, rename = "agentId")]
    pub agent_id: String,
    /// What happened.
    #[serde(default)]
    pub event: PaseoStreamEvent,
    /// When Paseo emitted it.
    #[serde(default)]
    pub timestamp: String,
    /// The native sequence, on a timeline event.
    #[serde(default)]
    pub seq: Option<u64>,
    /// The raw epoch that sequence is numbered in, on a timeline event.
    #[serde(default)]
    pub epoch: Option<String>,
}

impl PaseoStreamFrame {
    /// This frame as a canonical timeline entry, when it carries one.
    ///
    /// A stream frame with no `seq`/`epoch` is a notification rather than
    /// content — a permission lifecycle, an attention hint, a turn boundary.
    /// Numbering one of those would invent a position in a transcript Paseo
    /// owns.
    #[must_use]
    pub fn as_entry(&self) -> Option<(String, PaseoTimelineEntry)> {
        if self.event.event_type != "timeline" {
            return None;
        }
        let (seq, epoch, item) = (self.seq?, self.epoch.clone()?, self.event.item.clone()?);
        Some((
            epoch,
            PaseoTimelineEntry {
                item,
                timestamp: self.timestamp.clone(),
                seq_start: seq,
                seq_end: seq,
                source_seq_ranges: vec![PaseoSeqRange {
                    start_seq: seq,
                    end_seq: seq,
                }],
                collapsed: Vec::new(),
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Parse a Paseo wire timestamp.
///
/// Accepts any RFC 3339 instant rather than only Kontor's canonical spelling.
/// The live daemon stamps entries with millisecond precision
/// (`2026-08-12T23:03:47.670Z`), and a whole-second timestamp round-trips to a
/// different string than it arrived as — so demanding canonical form here would
/// reject Paseo's own output about half the time.
///
/// # Errors
/// Returns [`DomainError`] when the value is not an RFC 3339 instant.
pub fn parse_wire_timestamp(subject: &'static str, text: &str) -> DomainResult<Timestamp> {
    text.parse::<Timestamp>()
        .map_err(|_| DomainError::invalid(subject, "is not an RFC 3339 UTC timestamp"))
}

/// The event kind one native item type maps to.
///
/// Everything unrecognized becomes [`SessionEventKind::Log`] rather than being
/// dropped. Dropping would silently renumber the caller's view of a session,
/// which is the one thing the continuity guard cannot detect for itself.
///
/// There is no permission mapping, and that is not an omission: 0.3.1's
/// canonical timeline carries no permission items. That lifecycle arrives on
/// the stream and in [`PaseoAgent::pending_permissions`], and
/// [`classify_stream_event`] is where it is read.
#[must_use]
pub fn classify_item(item_type: &str) -> SessionEventKind {
    match item_type {
        "user_message" | "assistant_message" => SessionEventKind::Message,
        "tool_call" => SessionEventKind::ToolCall,
        "compaction" => SessionEventKind::StateChange,
        _ => SessionEventKind::Log,
    }
}

/// The event kind one unsolicited stream event maps to, when it is one Kontor
/// records.
#[must_use]
pub fn classify_stream_event(event_type: &str) -> Option<SessionEventKind> {
    match event_type {
        "permission_requested" => Some(SessionEventKind::PermissionRequest),
        "permission_resolved" => Some(SessionEventKind::PermissionResolved),
        _ => None,
    }
}

/// Turn one canonical entry into a [`SessionEvent`] inside `epoch`.
///
/// `epoch` is the Kontor-side `u64` the raw UUID resolved to; this function
/// never allocates one, because inventing an epoch is exactly how a restored
/// cursor stops meaning anything.
///
/// The native sequence and the native item type are preserved in the payload,
/// so an audit can re-derive this mapping from what Paseo actually said rather
/// than from what the adapter concluded.
///
/// # Errors
/// * [`RuntimeError::TimelineRefetchRequired`] with
///   [`TimelineBreak::SequenceGap`] when the entry is not exactly one native
///   sequence — a collapsed or non-contiguous range is a hole a canonical
///   cursor cannot page over.
/// * [`RuntimeError::Domain`] for a sequence of zero or an unusable timestamp.
pub fn normalize_entry(entry: &PaseoTimelineEntry, epoch: u64) -> RuntimeResult<SessionEvent> {
    if !entry.is_single_sequence() {
        return Err(RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap,
        });
    }
    if entry.seq_start == 0 {
        return Err(RuntimeError::Domain(DomainError::invalid(
            "PaseoTimelineEntry.seqStart",
            "native sequences are 1-based inside an epoch",
        )));
    }
    let emitted_at = parse_wire_timestamp("PaseoTimelineEntry.timestamp", &entry.timestamp)?;
    let kind = classify_item(&entry.item.item_type);
    // A native message id is not a Kontor one. Only `clientMessageId` — the id
    // this adapter supplied on the send — can be ours; when it parses the event
    // is addressable, and when it came from somewhere else the event is simply
    // not about a Kontor message.
    let subject = match entry
        .item
        .client_message_id
        .as_deref()
        .map(MessageId::parse)
    {
        Some(Ok(id)) => EventSubject::Message(id),
        _ => EventSubject::None,
    };
    let payload = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "paseo_version": PASEO_APP_VERSION,
        "native": {
            "seq": entry.seq_start,
            "type": bounded(&entry.item.item_type),
            "call_id": entry.item.call_id,
        },
        "normalized": {
            "kind": format!("{kind:?}"),
        },
    }))?;
    Ok(SessionEvent {
        kind,
        position: TimelinePosition {
            epoch,
            sequence: entry.seq_start,
        },
        subject,
        // 0.3.1 gives a timeline entry no id of its own; its identity is its
        // position. Minting one here would be a Kontor id wearing a native
        // name.
        native_event_id: None,
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

/// The permission this stream event is about, when it is about one.
#[must_use]
pub fn stream_permission_id(event: &PaseoStreamEvent) -> Option<String> {
    match event.event_type.as_str() {
        "permission_requested" => event.request.as_ref().map(|request| request.id.clone()),
        "permission_resolved" => event.request_id.clone(),
        _ => None,
    }
}

/// The permission this stream event is about, as an external id.
///
/// # Errors
/// Returns [`DomainError`] when Paseo's id is not a usable external id.
pub fn stream_permission_external_id(event: &PaseoStreamEvent) -> DomainResult<Option<ExternalId>> {
    stream_permission_id(event)
        .map(|id| ExternalId::parse(&id))
        .transpose()
}

/// The digest of a message body, as the delivery ledger compares retries.
#[must_use]
pub fn body_digest(body: &str) -> ContentHash {
    ContentHash::of(body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, item_type: &str) -> PaseoTimelineEntry {
        PaseoTimelineEntry {
            item: PaseoTimelineItem {
                item_type: item_type.to_owned(),
                ..PaseoTimelineItem::default()
            },
            timestamp: "2026-08-10T09:00:00.000Z".to_owned(),
            seq_start: seq,
            seq_end: seq,
            source_seq_ranges: vec![PaseoSeqRange {
                start_seq: seq,
                end_seq: seq,
            }],
            collapsed: Vec::new(),
        }
    }

    #[test]
    fn a_millisecond_daemon_timestamp_is_accepted_as_it_arrived() {
        // The live daemon stamps entries with milliseconds, and a whole second
        // renders back without them. Demanding Kontor's canonical spelling here
        // would refuse Paseo's own output.
        for stamp in [
            "2026-08-10T09:00:00Z",
            "2026-08-10T09:00:00.000Z",
            "2026-08-12T23:03:47.670Z",
        ] {
            parse_wire_timestamp("PaseoTimelineEntry.timestamp", stamp)
                .unwrap_or_else(|_| panic!("{stamp} is a timestamp Paseo really sends"));
        }
        assert!(parse_wire_timestamp("PaseoTimelineEntry.timestamp", "yesterday").is_err());
    }

    #[test]
    fn a_collapsed_or_split_range_is_a_gap_rather_than_one_event() {
        // The `projected` projection folds a whole tool lifecycle into one
        // entry. Accepting it would advance the cursor past sequences that were
        // never delivered, and the guard downstream cannot see what it was
        // never handed.
        let mut collapsed = entry(4, "tool_call");
        collapsed.seq_end = 5;
        collapsed.source_seq_ranges = vec![PaseoSeqRange {
            start_seq: 4,
            end_seq: 5,
        }];
        collapsed.collapsed = vec!["tool_lifecycle".to_owned()];
        assert_eq!(
            normalize_entry(&collapsed, 1).expect_err("a range is not an event"),
            RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::SequenceGap
            }
        );

        // …and so is an entry whose source ranges are not contiguous with the
        // span it claims, which is the same hole reported a different way.
        let mut split = entry(4, "assistant_message");
        split.seq_end = 6;
        split.source_seq_ranges = vec![
            PaseoSeqRange {
                start_seq: 4,
                end_seq: 4,
            },
            PaseoSeqRange {
                start_seq: 6,
                end_seq: 6,
            },
        ];
        assert!(normalize_entry(&split, 1).is_err());

        normalize_entry(&entry(4, "tool_call"), 1).expect("one sequence is one event");
    }

    #[test]
    fn an_unknown_item_becomes_a_bounded_log_rather_than_disappearing() {
        let mut unknown = entry(2, "something_paseo_added_later");
        unknown.item.item_type = "x".repeat(MAX_UNKNOWN_FRAME_CHARS * 4);
        let event = normalize_entry(&unknown, 1).expect("an unknown item is still an event");
        assert_eq!(event.kind, SessionEventKind::Log);
        assert_eq!(event.position.sequence, 2);
        assert!(
            event.payload.json().len() < MAX_UNKNOWN_FRAME_CHARS * 4,
            "an unaudited native string is bounded before it is persisted"
        );
    }

    #[test]
    fn only_the_client_message_id_addresses_a_kontor_message() {
        // Paseo echoes the caller's own id as `clientMessageId`; `messageId` is
        // the provider's. Reading the wrong one would make every send look
        // unconfirmed and every retry look safe.
        let kontor = MessageId::generate();
        let mut ours = entry(2, "user_message");
        ours.item.client_message_id = Some(kontor.to_string());
        ours.item.message_id = Some("msg_01HZY8QF".to_owned());
        assert_eq!(
            normalize_entry(&ours, 1)
                .expect("our own id round-trips")
                .subject,
            EventSubject::Message(kontor)
        );

        let mut theirs = entry(3, "user_message");
        theirs.item.message_id = Some(kontor.to_string());
        assert_eq!(
            normalize_entry(&theirs, 1)
                .expect("a provider message is still content")
                .subject,
            EventSubject::None,
            "a provider's own id is not an acknowledgement of our send"
        );
    }

    #[test]
    fn an_idle_or_finished_agent_is_a_seat_and_not_a_verdict() {
        assert!(PaseoAgentStatus::Idle.is_reusable_seat());
        assert!(PaseoAgentStatus::Running.is_reusable_seat());
        assert!(PaseoAgentStatus::Initializing.is_reusable_seat());
        assert!(!PaseoAgentStatus::Closed.is_reusable_seat());
        assert!(PaseoAgentStatus::Closed.needs_reload());
        assert!(!PaseoAgentStatus::Idle.needs_reload());
    }

    #[test]
    fn retirement_is_the_archive_stamp_rather_than_a_status() {
        let mut agent = PaseoAgent {
            id: "agt_1".to_owned(),
            provider: "claude".to_owned(),
            model: "claude-opus-5".to_owned(),
            thinking_option_id: None,
            effective_thinking_option_id: None,
            current_mode_id: Some("auto".to_owned()),
            workspace_id: Some("wks_1".to_owned()),
            cwd: "/w/task-1".to_owned(),
            title: Some("KON-MVP-11 Implement".to_owned()),
            labels: [
                (label::ROLE.to_owned(), "implement".to_owned()),
                (label::ROLE_SLOT.to_owned(), "implement-a".to_owned()),
                (
                    label::PARENT_AGENT.to_owned(),
                    "agt_orchestrator".to_owned(),
                ),
            ]
            .into_iter()
            .collect(),
            status: PaseoAgentStatus::Idle,
            archived_at: None,
            attention_reason: Some("finished".to_owned()),
            persistence: None,
            pending_permissions: Vec::new(),
        };
        assert!(!agent.is_archived());
        agent.archived_at = Some("2026-08-10T09:00:00.000Z".to_owned());
        assert!(agent.is_archived(), "0.3.1 has no `archived` status");
        assert_eq!(agent.parent_agent_id(), Some("agt_orchestrator"));
    }

    #[test]
    fn only_process_occupying_states_spend_concurrent_capacity() {
        assert!(PaseoAgentStatus::Initializing.occupies_concurrent_capacity());
        assert!(PaseoAgentStatus::Running.occupies_concurrent_capacity());
        assert!(PaseoAgentStatus::Error.occupies_concurrent_capacity());
        assert!(!PaseoAgentStatus::Idle.occupies_concurrent_capacity());
        assert!(!PaseoAgentStatus::Closed.occupies_concurrent_capacity());
        assert!(!PaseoAgentStatus::Unknown.occupies_concurrent_capacity());
    }

    #[test]
    fn a_label_census_is_exact_and_total() {
        let agent = PaseoAgent {
            id: "agt_1".to_owned(),
            provider: "claude".to_owned(),
            model: "claude-opus-5".to_owned(),
            thinking_option_id: None,
            effective_thinking_option_id: None,
            current_mode_id: Some("auto".to_owned()),
            workspace_id: Some("wks_1".to_owned()),
            cwd: "/w/task-1".to_owned(),
            title: None,
            labels: [
                (label::ROLE.to_owned(), "implement".to_owned()),
                (label::ROLE_SLOT.to_owned(), "implement-a".to_owned()),
            ]
            .into_iter()
            .collect(),
            status: PaseoAgentStatus::Idle,
            archived_at: None,
            attention_reason: None,
            persistence: None,
            pending_permissions: Vec::new(),
        };
        let mut wanted: BTreeMap<String, String> = BTreeMap::new();
        wanted.insert(label::ROLE.to_owned(), "implement".to_owned());
        assert!(agent.matches_labels(&wanted));
        // The slot is the key, so a same-role agent in another slot is not this
        // seat however well the role name agrees.
        wanted.insert(label::ROLE_SLOT.to_owned(), "implement-b".to_owned());
        assert!(!agent.matches_labels(&wanted));
    }

    #[test]
    fn the_pinned_baseline_advertises_every_required_feature_and_neither_optional_one() {
        let info = PaseoServerInfo {
            server_id: "srv_1".to_owned(),
            version: Some(PASEO_APP_VERSION.to_owned()),
            hostname: None,
            features: REQUIRED_FEATURES
                .iter()
                .map(|feature| (feature.as_str().to_owned(), true))
                .collect(),
        };
        assert!(info.is_supported_baseline());
        assert!(info.missing_required().is_empty());
        assert!(!info.supports(PaseoFeature::ProjectRename));
        assert!(!info.supports(PaseoFeature::Compaction));

        // `false` is an answer, not an absence.
        let mut refused = info.clone();
        refused
            .features
            .insert(PaseoFeature::ProjectList.as_str().to_owned(), false);
        assert_eq!(refused.missing_required(), vec![PaseoFeature::ProjectList]);

        let unversioned = PaseoServerInfo {
            version: None,
            ..info
        };
        assert!(
            !unversioned.is_supported_baseline(),
            "a daemon that will not say which build it is has not agreed with the pin"
        );
    }

    #[test]
    fn a_newer_daemon_clears_the_baseline_and_an_older_one_does_not() {
        let at = |version: &str| PaseoServerInfo {
            server_id: "srv_1".to_owned(),
            version: Some(version.to_owned()),
            hostname: None,
            features: REQUIRED_FEATURES
                .iter()
                .map(|feature| (feature.as_str().to_owned(), true))
                .collect(),
        };

        assert!(at(PASEO_APP_VERSION).is_supported_baseline());
        // The regression this floor was introduced for: 0.4.0 is newer than the
        // recorded baseline, and an equality pin degraded the whole fleet on it.
        assert!(at("0.4.0").is_supported_baseline());
        assert!(at("0.4.1").is_supported_baseline());
        assert!(at("1.0.0").is_supported_baseline());
        assert!(!at(PASEO_APP_VERSION).supports_project_rename());
        assert!(at("0.4.0").supports_project_rename());
        assert!(at("0.4.1").supports_project_rename());
        assert!(at("1.0.0").supports_project_rename());
        assert!(!at("0.4.0-beta.2").supports_project_rename());
        assert!(!at("not-a-version").supports_project_rename());
        assert!(
            !at("0.3.0").is_supported_baseline(),
            "a release below the recorded baseline is observed, never driven"
        );
        assert!(!at("0.2.9").is_supported_baseline());
    }

    #[test]
    fn the_baseline_orders_versions_as_numbers_rather_than_text() {
        // The regression the floor exists for: `"0.10.0" < "0.4.0"` as text, so
        // a lexical compare would degrade the fleet on the tenth minor release.
        assert!(version_at_least("0.10.0", "0.4.0"));
        assert!(version_at_least("0.4.10", "0.4.9"));
        assert!(!version_at_least("0.4.0", "0.10.0"));
    }

    #[test]
    fn a_pre_release_does_not_clear_the_release_it_is_named_for() {
        assert!(!version_at_least("0.4.0-beta.2", "0.4.0"));
        // ...but it does clear everything that release supersedes, which is why
        // a 0.4.0 beta is still driven against a 0.3.1 floor.
        assert!(version_at_least("0.4.0-beta.2", PASEO_APP_VERSION));
        assert!(version_at_least("0.4.1-beta.1", "0.4.0"));
    }

    #[test]
    fn an_unreadable_version_is_not_at_least_anything() {
        for reported in ["", "0.4", "0.4.0.1", "next", "v0.4.0", "0.x.0"] {
            assert!(
                !version_at_least(reported, "0.4.0"),
                "{reported} is not a version this adapter can order"
            );
        }
    }

    #[test]
    fn a_stream_notification_is_never_numbered_as_content() {
        let mut frame = PaseoStreamFrame {
            agent_id: "agt_1".to_owned(),
            event: PaseoStreamEvent {
                event_type: "permission_requested".to_owned(),
                request: Some(PaseoStreamPermissionRequest {
                    id: "perm_1".to_owned(),
                }),
                ..PaseoStreamEvent::default()
            },
            timestamp: "2026-08-10T09:00:00.000Z".to_owned(),
            seq: None,
            epoch: None,
        };
        assert!(frame.as_entry().is_none());
        assert_eq!(
            stream_permission_id(&frame.event).as_deref(),
            Some("perm_1")
        );

        frame.event = PaseoStreamEvent {
            event_type: "timeline".to_owned(),
            item: Some(PaseoTimelineItem {
                item_type: "assistant_message".to_owned(),
                ..PaseoTimelineItem::default()
            }),
            ..PaseoStreamEvent::default()
        };
        frame.seq = Some(7);
        frame.epoch = Some("epoch-1".to_owned());
        let (epoch, entry) = frame.as_entry().expect("a timeline event is content");
        assert_eq!(epoch, "epoch-1");
        assert_eq!(entry.seq_start, 7);
        assert!(entry.is_single_sequence());
    }

    #[test]
    fn a_page_declares_its_own_break() {
        let mut page = PaseoTimelinePage {
            agent_id: "agt_1".to_owned(),
            direction: PaseoDirection::After,
            projection: PaseoProjection::Canonical,
            epoch: "epoch-1".to_owned(),
            reset: false,
            stale_cursor: false,
            gap: false,
            entries: Vec::new(),
            start_cursor: None,
            end_cursor: None,
            has_older: false,
            has_newer: false,
            error: None,
        };
        assert_eq!(page.declared_break(), None);
        page.gap = true;
        assert_eq!(page.declared_break(), Some(TimelineBreak::SequenceGap));
        page.gap = false;
        page.stale_cursor = true;
        assert_eq!(page.declared_break(), Some(TimelineBreak::EpochChanged));
        page.reset = true;
        assert_eq!(page.declared_break(), Some(TimelineBreak::EpochChanged));
    }

    #[test]
    fn a_page_cursor_is_followed_only_when_both_halves_agree() {
        let info = |cursor: Option<&str>, has_more: bool| PaseoPageInfo {
            next_cursor: cursor.map(str::to_owned),
            has_more,
        };
        assert_eq!(info(Some("cur_1"), true).next(), Some("cur_1"));
        assert_eq!(info(Some("cur_1"), false).next(), None);
        assert_eq!(info(None, true).next(), None);
        assert_eq!(info(Some(""), true).next(), None);
    }

    #[test]
    fn paseo_0_4_directory_cursor_names_are_backward_compatible() {
        let page_info: PaseoPageInfo = serde_json::from_value(serde_json::json!({
            "afterCursor": "cur_2",
            "hasMoreAfter": true
        }))
        .expect("Paseo 0.4 page info");

        assert_eq!(page_info.next(), Some("cur_2"));
    }

    /// Acceptance authorizes a binding only when the answer is *this* answer.
    #[test]
    fn acceptance_requires_exact_correlation() {
        let answer = PaseoMessageAccepted {
            request_id: "req-1".to_owned(),
            agent_id: "agt-1".to_owned(),
            accepted: true,
            error: None,
        };
        assert!(answer.authorizes("req-1", "agt-1"));
        assert!(
            !answer.authorizes("req-2", "agt-1"),
            "an answer to another request must not bind this seat"
        );
        assert!(
            !answer.authorizes("req-1", "agt-2"),
            "an answer about another agent must not bind this seat"
        );

        let refused = PaseoMessageAccepted {
            accepted: false,
            ..answer.clone()
        };
        assert!(!refused.authorizes("req-1", "agt-1"));

        // A frame that carried no correlation at all deserializes to empty
        // strings, and empty strings match nothing this launch sent.
        let bare = PaseoMessageAccepted {
            request_id: String::new(),
            agent_id: String::new(),
            accepted: true,
            error: None,
        };
        assert!(!bare.authorizes("req-1", "agt-1"));
    }
}
