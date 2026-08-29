//! The Paseo adapter: hierarchy, admission, continuity and session content, in
//! one place.
//!
//! Everything Paseo can prove and everything it cannot lives here together on
//! purpose. Split across a factory, a policy object and a persistence port, the
//! interesting question — *may Kontor conclude this?* — stops being answerable
//! by reading one file, and that question is the whole ticket.
//!
//! # The hierarchy this exists to keep compact
//!
//! One Kontor mini-project (one Jira epic) is one Paseo project. One task
//! worktree is one workspace in that project. One `(team_run, role_slot)` is one
//! persistent agent in that workspace, for the life of the seat. Every rule
//! below falls out of those three sentences:
//!
//! * an idle agent — including one Paseo decorates `attentionReason=finished` —
//!   is that same seat waiting for its next turn, so the next turn is a message
//!   to the same agent id and never a second agent;
//! * a name that drifts is a name, so it is reported as
//!   [`PaseoProjectOutcome::ReadyWithRenamePending`] rather than repaired by
//!   writing Paseo's internal state or by creating a better-named twin;
//! * every id is read back from the daemon protocol before it is believed,
//!   because the CLI's JSON omits exactly the fields the placement rules are
//!   about.
//!
//! # What Paseo 0.3.1 cannot do
//!
//! No supported compaction and no per-run coding
//! account. None is filled in with a guess: the first two are typed adapter
//! outcomes, and the third is declared unsupported so an account-pinned run is
//! refused before dispatch.
//!
//! Three more absences are 0.3.1's own and are handled here rather than papered
//! over:
//!
//! * workspace titles are human-readable aliases; durable Kontor bindings hold
//!   the native identities used after first discovery;
//! * an agent snapshot has no `projectId`, so "is this agent in the epic
//!   project?" is answered through its workspace, which does carry one;
//! * the canonical timeline carries no permission items, so a permission's fate
//!   is read from [`crate::wire::PaseoAgent::pending_permissions`] and from the
//!   unsolicited stream rather than from history.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_core::compaction::CompactionReceipt;
use kontor_core::id::{
    AgentRunId, CanonicalDocument, ContentHash, ExternalId, ExternalName, RoleSlotId,
    RuntimeBindingId, RuntimeKindKey, SeatBindingId, TaskId, TeamRunId, Timestamp, TopologyNodeId,
};
use kontor_core::repository::RuntimeBinding;
use kontor_core::spec::{EffortLevel, ModelRef, ModelRung, ProviderRef, SeatAutonomy};
use kontor_core::state::{NativeRuntimeIdentity, ObservedRunState, RuntimeContact};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::adapter::{
    ConsultationLaunchOutcome, ConsultationLaunchRequest, ConsultationMessageRequest,
    ConsultationRouteProvenance, ConsultationSeatRetireOutcome, ConsultationSeatRetireRequest,
    HostedSeatClaimOutcome, HostedSeatClaimPredecessor, HostedSeatClaimPreview,
    HostedSeatClaimRequest, HostedSeatInspectRequest, HostedSeatInspection,
    HostedSeatLaunchRequest, HostedSeatMessageOutcome, HostedSeatMessageRequest,
    HostedSeatNativeState, HostedSeatRetireOutcome, HostedSeatRetireRequest,
    HostedSeatTitleConflict, LaunchOutcome, MessageAck, PermissionAck, RetitleSeatOutcome,
    RetitleSeatRequest, RuntimeAdapter, RuntimeError, RuntimeResult,
};
use kontor_runtime::admission::{
    AdmissionLedger, AdmissionOutcome, AdmissionRequest, ClaimedSeat, OccupiedSeat, RoleSlotKey,
    SeatFacts,
};
use kontor_runtime::capability::{
    IssuedBinding, IssuedBindingRegistry, LimitDemand, OperationContext, RuntimeBindingSnapshot,
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade, preflight,
};
use kontor_runtime::container::{
    ContainerBinding, ContainerBindingSnapshot, ContainerCorrelationEvidence, ContainerOutcome,
    ContainerProjection, ContainerRequest, RetitleContainerOutcome, RetitleContainerRequest,
};
use kontor_runtime::observation::{
    ControlPlaneObservation, CorrelationEvidence, NativeSession, ObservationSource,
    ReconciliationFinding, ReconciliationReport, reconcile,
};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, CompactRequest, CorrelationLabel, HistoryRequest, InspectRequest,
    LaunchRequest, LiveSubscribeRequest, MessageId, PermissionDecision, PermissionResponseRequest,
    ReconcileSessionLabelsRequest, ReconciledSessionLabels, ResumeRequest, SendMessageRequest,
};
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::timeline::{
    Admission, EventSubject, HistoryCursor, HistoryPage, LiveSubscription, MessageLedger,
    PermissionLedger, SessionEvent, TimelineBreak, TimelinePosition,
};
use kontor_runtime::workspace::{
    WorkspaceBinding, WorkspaceBindingSnapshot, WorkspaceCorrelationEvidence, WorkspaceLabel,
    WorkspaceOutcome, WorkspacePrepareRequest, WorkspaceRoot,
};

use crate::client::{PaseoCommand, PaseoRpc, PaseoTransport, ensure_frame_bounded, paseo_mode};
use crate::mcp::{PaseoMcp, RENAME_WORKSPACE_TOOL};
use crate::wire::{
    MAX_DIRECTORY_PAGE, MAX_DIRECTORY_PAGES, MAX_HISTORY_PAGE, MAX_MESSAGE_BYTES,
    PASEO_APP_VERSION, PaseoAgent, PaseoAgentAnswer, PaseoAgentPage, PaseoAgentStatus,
    PaseoCliAgentReloaded, PaseoCliAgentStarted, PaseoCliAgentUpdated, PaseoCliArchived,
    PaseoCliStopped, PaseoCliWorkspaceCreated, PaseoDirection, PaseoPermissionResolved,
    PaseoProject, PaseoProjectAdded, PaseoProjectList, PaseoProjectRenamed, PaseoProjection,
    PaseoSendAccepted, PaseoServerInfo, PaseoStreamFrame, PaseoSubscriptionAck,
    PaseoTimelineCursor, PaseoTimelinePage, PaseoWorkspace, PaseoWorkspaceKind, PaseoWorkspacePage,
    label, normalize_entry, stream_permission_external_id,
};

/// Everything Paseo 0.3.1 can prove at trust grade A.
const SUPPORTED: &[RuntimeCapability] = &[
    RuntimeCapability::Discovery,
    RuntimeCapability::PrepareWorkspace,
    // 0.3.1's `project.add` registers a project from a directory and the
    // project list reads it back by exact id, which is what a native root
    // needs: something to create and something to prove afterwards.
    RuntimeCapability::PrepareProject,
    RuntimeCapability::Launch,
    RuntimeCapability::Resume,
    RuntimeCapability::SendMessage,
    RuntimeCapability::Cancel,
    RuntimeCapability::Retire,
    RuntimeCapability::Inspect,
    RuntimeCapability::Adopt,
    RuntimeCapability::History,
    RuntimeCapability::LiveEvents,
    RuntimeCapability::PermissionResponse,
];

/// What is left when the daemon does not advertise every required feature.
///
/// Reads only. A daemon that cannot prove stable project identity or selective
/// timelines may still be looked at — that is what keeps an unknown Paseo build
/// visible in the adoption inbox — but Kontor never drives it, because every
/// placement rule below depends on features it did not claim.
const DEGRADED: &[RuntimeCapability] = &[RuntimeCapability::Discovery, RuntimeCapability::Inspect];

/// How many canonical pages a reconciliation scan will read before giving up.
///
/// Bounded because a reconcile runs on the *unhappy* path — a lost
/// acknowledgement — and an unbounded scan there turns one bad delivery into a
/// walk of the whole transcript.
const RECONCILE_PAGE_BUDGET: usize = 4;

/// How many complete cursor-free origin walks may be attempted after Paseo
/// invalidates the numbering mid-walk.
///
/// A retry always starts at a fresh tail and discards every cursor and item
/// from the invalidated attempt. The bound prevents a runtime that keeps
/// renumbering from turning one read into an unbounded request.
const CURSOR_FREE_REFETCH_ATTEMPTS: usize = 3;

// ---------------------------------------------------------------------------
// Configuration and scope
// ---------------------------------------------------------------------------

/// The exact, validated fields every Paseo command variable is resolved from.
///
/// These are fields, not lookups. Resolving `plan_item_key` by searching display
/// names is how two tickets with similar titles end up sharing a workspace, so
/// the display names below are *derived from* these values and never the other
/// way round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoExecutionScope {
    /// The Jira epic the mini-project is tracked as, e.g. `ASMA-7744`.
    pub jira_epic_key: ExternalId,
    /// The persisted compact epic title, e.g. `Kontor MVP`.
    pub mini_project_short_title: ExternalName,
    /// The Kontor plan item, e.g. `KON-MVP-11`.
    pub plan_item_key: ExternalId,
    /// The Jira issue for the ticket, e.g. `ASMA-7755`.
    pub jira_issue_key: ExternalId,
    /// The runtime-neutral short ticket code, e.g. `KON-11`.
    pub ticket_short_code: ExternalId,
    /// The visible role and optional stable suffix for every declared slot.
    pub seat_display_roles: BTreeMap<RoleSlotId, (ExternalName, Option<ExternalName>)>,
    /// The repository root the **epic's project** is registered from.
    ///
    /// Distinct from [`Self::canonical_worktree_cwd`], and 0.3.1 is why. Its
    /// `project.add.request` takes a `cwd` and nothing else: the daemon runs
    /// `findOrCreateProjectForDirectory` and names the project after that
    /// directory. Registering the *task worktree* would therefore mint one
    /// project per task — the exact opposite of one epic, one project, many task
    /// workspaces — and it would do it silently, because each such project is
    /// perfectly valid on its own.
    ///
    /// So the project root travels separately and is the only thing
    /// [`PaseoAdapter::prepare_project`] ever registers. A live Grade-A run
    /// caught this: the first composed attempt created a second project rooted
    /// at the worktree.
    pub project_root_cwd: WorkspaceRoot,
    /// The filesystem-canonical task worktree. Authority, never display data.
    pub canonical_worktree_cwd: WorkspaceRoot,
    /// Explicit ticket scopes served by this epic runtime.
    ///
    /// Empty preserves the original single-ticket configuration. Once present,
    /// every task must be named so one runtime family can serve several TSWs
    /// without silently falling back to another ticket's worktree or titles.
    pub task_scopes: BTreeMap<TaskId, PaseoTaskScope>,
    /// Legacy host-level caller id retained for configuration compatibility.
    ///
    /// Kontor no longer sends this value to Paseo. A runtime configuration is
    /// shared by multiple epics, while native caller parentage is scoped to one
    /// Paseo agent tree; combining those scopes leaked an unrelated epic's TPM
    /// into a launch. Kontor's durable ECP and SeatBinding records are the
    /// ownership authority, and the native launch is top-level in the exact
    /// workspace already attested for the ticket.
    pub orchestrator_agent_id: ExternalId,
}

/// The ticket-specific half of one epic-scoped Paseo runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoTaskScope {
    /// The Kontor plan item.
    pub plan_item_key: ExternalId,
    /// The Jira issue for the ticket.
    pub jira_issue_key: ExternalId,
    /// The runtime-neutral short ticket code.
    pub ticket_short_code: ExternalId,
    /// The filesystem-canonical task worktree.
    pub canonical_worktree_cwd: WorkspaceRoot,
}

impl PaseoExecutionScope {
    fn configured_task_scope(&self, task_id: TaskId) -> Option<PaseoTaskScope> {
        if self.task_scopes.is_empty() {
            return Some(PaseoTaskScope {
                plan_item_key: self.plan_item_key.clone(),
                jira_issue_key: self.jira_issue_key.clone(),
                ticket_short_code: self.ticket_short_code.clone(),
                canonical_worktree_cwd: self.canonical_worktree_cwd.clone(),
            });
        }
        self.task_scopes.get(&task_id).cloned()
    }

    #[cfg(test)]
    fn task_scope(&self, task_id: TaskId) -> RuntimeResult<PaseoTaskScope> {
        self.configured_task_scope(task_id)
            .ok_or(RuntimeError::WorkspaceMismatch {
                rule: "the task has no configured Paseo workspace scope",
            })
    }
}

/// One configured Paseo execution plane: one host, one epic, many task worktrees.
///
/// Ticket scope stays explicit inside the adapter because the runtime registry
/// has one authority entry per runtime family. The task id selects the exact
/// worktree and display metadata before any native effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoConfig {
    /// The Kontor runtime-kind key, e.g. `paseo.agent`.
    pub runtime_kind: RuntimeKindKey,
    /// The **non-secret** host key. The endpoint and its credential are resolved
    /// inside the transport and never appear here, in a checkpoint, or in a
    /// binding.
    pub host_key: ExternalName,
    /// The Kontor mini-project this plane serves.
    pub mini_project_id: ExternalId,
    /// The validated command variables.
    pub scope: PaseoExecutionScope,
    /// The most sessions Kontor will hold open on this plane at once.
    pub max_concurrent_sessions: u32,
    /// Temporarily ineligible providers, selected by explicit fleet
    /// configuration rather than inferred from a failed prompt.
    pub unavailable_providers: BTreeSet<String>,
    /// Temporary provider outage routes, keyed by the unavailable provider.
    /// They are explicit fleet configuration, not a permanent model-policy
    /// rewrite and not inferred from a failed prompt.
    pub provider_fallbacks: BTreeMap<String, ModelRung>,
    /// Native containers this plane **adopts** rather than creates, by the
    /// topology node each one already belongs to.
    ///
    /// Keyed by node id and not by kind, because the kind vocabulary belongs to
    /// the pinned specification and an adapter holding a copy of it is one no
    /// specification revision can correct. The configured project workspace is
    /// simply one row here.
    ///
    /// Adoption is by **exact readback of the configured id** and carries no
    /// authority over the container: it is never renamed, never archived, and
    /// children of it remain outside Kontor unless a topology binding names
    /// their exact native ids.
    pub adopted_containers: BTreeMap<TopologyNodeId, ExternalId>,
    /// Whether this deployment has registered one Paseo provider alias per
    /// coding account, so `--provider` selects the account.
    ///
    /// This is the governed account pin, and it is declared rather than
    /// inferred. Paseo takes `--provider` and no account parameter, so two
    /// aliases may be two logins or two spellings of one, and Kontor cannot tell
    /// which by looking. Guessing permissively is the expensive error: it would
    /// report a per-run account guarantee this runtime does not make, and every
    /// launch receipt written under it would attest to something unverified.
    ///
    /// When it is `true`, [`Self::capabilities`] reports `account_env` and the
    /// pin is enforced by the readback this adapter already performs —
    /// `verify_agent_route` compares the provider Paseo reports against the
    /// provider that was requested and fails correlation if they differ. When it
    /// is `false`, `kontor_runtime::capability::preflight` refuses every
    /// account-pinned launch on this plane, which is the v1.0 behaviour.
    pub provider_selects_account: bool,
    /// Worktree-local MCP composition for Claude seats, when the daemon
    /// enabled it.
    ///
    /// `None` means no composition at all — the daemon's `KONTOR_SEAT_MCP=off`
    /// kill switch, or a caller that never configured it. See [`crate::seat_mcp`].
    pub seat_mcp: Option<crate::seat_mcp::SeatMcp>,
}

impl PaseoConfig {
    /// The capabilities a binding freezes when the daemon proved every required
    /// feature.
    #[must_use]
    pub fn capabilities(&self) -> RuntimeCapabilities {
        self.capabilities_at(TrustGrade::A, SUPPORTED)
    }

    /// The capabilities of a daemon that did not.
    #[must_use]
    pub fn degraded_capabilities(&self) -> RuntimeCapabilities {
        self.capabilities_at(TrustGrade::C, DEGRADED)
    }

    fn capabilities_at(
        &self,
        trust_grade: TrustGrade,
        supported: &[RuntimeCapability],
    ) -> RuntimeCapabilities {
        RuntimeCapabilities {
            trust_grade,
            supported: supported.iter().copied().collect(),
            // Paseo runs one ambient environment per host, so a per-run coding
            // account is not provable *by default* and an ambient one must never
            // be promoted into account routing just because it happens to work.
            // A deployment that has registered one provider alias per account
            // declares it, and then the alias readback in `verify_agent_route`
            // is what proves the pin.
            account_env: self.provider_selects_account,
            limits: RuntimeLimits {
                max_message_bytes: MAX_MESSAGE_BYTES,
                max_history_page: MAX_HISTORY_PAGE,
                max_concurrent_sessions: self.max_concurrent_sessions,
                context_window: kontor_core::spec::ContextWindowBounds::unknown(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Hierarchy bindings and adapter-specific outcomes
// ---------------------------------------------------------------------------

/// The persisted `(mini_project, host) -> project` binding.
///
/// The project id is the epic identity. The repository `projectKey` is read and
/// never matched on, because the live daemon holds several projects for one Git
/// remote and selecting by it would bind an epic to somebody else's work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoProjectBinding {
    /// The Kontor mini-project.
    pub mini_project_id: ExternalId,
    /// The non-secret host key.
    pub host_key: ExternalName,
    /// The Paseo project id.
    pub project_id: ExternalId,
    /// The display name as it was last read back.
    pub observed_name: String,
}

/// What preparing the epic project produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaseoProjectOutcome {
    /// The project exists and its display name is the desired one.
    Ready {
        /// The binding.
        binding: PaseoProjectBinding,
    },
    /// The project exists and its display name has drifted.
    ///
    /// Paseo 0.3.1 advertises no `projectRename`, and the bundled client's
    /// internal rename is not a supported operation. So the drift is reported
    /// and persisted rather than repaired: writing another owner's internal
    /// state can corrupt the identity everything else here is keyed on, and
    /// creating a better-named second project would split the epic in two.
    ReadyWithRenamePending {
        /// The binding, which is usable exactly as it is.
        binding: PaseoProjectBinding,
        /// The name Kontor would use.
        desired_name: String,
        /// The name Paseo actually holds.
        observed_name: String,
    },
}

impl PaseoProjectOutcome {
    /// The binding, whichever outcome this is. A pending rename never blocks
    /// work: a display string is not authority.
    #[must_use]
    pub const fn binding(&self) -> &PaseoProjectBinding {
        match self {
            Self::Ready { binding } | Self::ReadyWithRenamePending { binding, .. } => binding,
        }
    }

    /// Whether the display name drifted.
    #[must_use]
    pub const fn rename_pending(&self) -> bool {
        matches!(self, Self::ReadyWithRenamePending { .. })
    }
}

/// What Kontor can say about compacting one Paseo session.
///
/// Adapter-specific rather than a shared capability, because no sibling runtime
/// needs the concept yet and widening the trait for one adapter makes every
/// other one answer a question it was not asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaseoCompaction {
    /// The inspected 0.3.1 surface exposes no compaction operation at all.
    ///
    /// Not "it failed": there is nothing to call. A reload restarts a process
    /// and a replacement starts a different session; neither compacts anything,
    /// and reporting either as success would let a policy that requires a
    /// compacted seat proceed on a seat that never was.
    Unsupported,
    /// Policy requires a confirmed compaction before this seat is reused, and
    /// none can be obtained. The seat is blocked rather than quietly reused.
    Pending,
}

/// The native place a launch resolved to, and the Kontor binding that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchPlace {
    /// The native workspace the seat is placed in.
    workspace_id: String,
    /// The directory that workspace must be rooted at.
    root: WorkspaceRoot,
    /// The Kontor binding this place was made under, in canonical text.
    binding: String,
}

/// The whole correlation chain one seat is persisted under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoSeatRecord {
    /// The Kontor mini-project.
    pub mini_project_id: ExternalId,
    /// The Jira epic.
    pub jira_epic_key: ExternalId,
    /// The Kontor plan item.
    pub plan_item_key: ExternalId,
    /// The task the seat serves.
    pub task_id: TaskId,
    /// The team run.
    pub team_run_id: TeamRunId,
    /// The stable role slot.
    pub role_slot_id: RoleSlotId,
    /// The agent run.
    pub agent_run_id: AgentRunId,
    /// The Kontor session binding.
    pub binding_id: RuntimeBindingId,
    /// The Kontor binding this seat's place was made under, in canonical text.
    ///
    /// Text rather than one of the two binding id types, because a seat may be
    /// placed in a TeamRun-keyed workspace or in a topology-node-keyed
    /// container and this record is evidence about *which place*, not about
    /// which keying produced it.
    pub placement_binding_id: String,
    /// The canonical task worktree.
    pub canonical_worktree_cwd: WorkspaceRoot,
    /// The non-secret host key.
    pub host_key: ExternalName,
    /// The Paseo project.
    pub project_id: ExternalId,
    /// The Paseo workspace.
    pub workspace_id: ExternalId,
    /// The Paseo agent.
    pub agent_id: ExternalId,
    /// The provider confirmed by Paseo's authoritative readback.
    pub provider: String,
    /// The model confirmed by Paseo's authoritative readback.
    pub model: String,
    /// The effective reasoning option confirmed by Paseo, if selected.
    pub thinking: Option<String>,
    /// The provider's own session id, when Paseo exposed one.
    pub provider_session_id: Option<ExternalId>,
    /// The native parent, when one was explicitly attested.
    ///
    /// Kontor-created seats are top-level and record `None`; logical ownership
    /// lives in Kontor's ECP/SeatBinding topology rather than a process-global
    /// Paseo caller.
    pub parent_agent_id: Option<ExternalId>,
    /// The adapter generation the native ids belong to.
    pub generation: u64,
    /// The retired predecessor this seat replaced, when it replaced one.
    pub previous_agent_id: Option<ExternalId>,
}

/// An operator's explicit authorization to adopt one foreign Paseo session.
///
/// Discovery is read-only. Adoption is the one path that writes labels onto a
/// session Kontor did not start, and a session Kontor did not start may well be
/// a human's — so it happens only against a recorded intent naming the exact
/// native agent and the exact seat it will fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoAdoptionIntent {
    /// The exact Paseo agent to adopt.
    pub native_agent_id: ExternalId,
    /// The team run whose seat it will fill.
    pub team_run_id: TeamRunId,
    /// The stable role slot it will fill.
    pub role_slot_id: RoleSlotId,
    /// The task that seat serves.
    pub task_id: TaskId,
    /// Durable epic/task identity the adopted seat will carry.
    pub scope: ExecutionScope,
}

/// What one declared role slot needs before it can take a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaseoSlotPlan {
    /// One compatible agent already holds this seat; send it the next turn.
    ///
    /// This is what keeps the hierarchy compact. An idle or finished agent is
    /// reported here, not as a vacancy.
    Reuse {
        /// The seat.
        slot: RoleSlotKey,
        /// The agent that holds it.
        agent_id: ExternalId,
        /// Whether it needs a process restart before it can continue.
        needs_reload: bool,
    },
    /// The seat is empty and must be materialized exactly once, through the
    /// ordinary admitted launch path with its bootstrap role prompt.
    Materialize {
        /// The seat.
        slot: RoleSlotKey,
    },
    /// The seat cannot be acted on, and no edit or verdict may proceed from it.
    Blocked {
        /// The seat.
        slot: RoleSlotKey,
        /// Why, structurally.
        rule: &'static str,
    },
}

/// What became of one delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaseoDelivery {
    /// Paseo acknowledged it and canonical history showed it.
    Acknowledged(MessageAck),
    /// Paseo may or may not have accepted it, and canonical history did not
    /// settle which inside the reconcile budget.
    ///
    /// The identifier stays usable — Paseo's `messageId` is the idempotency key,
    /// so resending *the same one* after reconciliation proves no matching entry
    /// exists cannot duplicate the effect. What is never done is resending a
    /// different id, or resending without looking first.
    ConfirmationUnknown,
}

// ---------------------------------------------------------------------------
// Epoch registry
// ---------------------------------------------------------------------------

/// The persisted `raw Paseo epoch UUID -> Kontor u64 epoch` mapping.
///
/// Paseo numbers a session's content inside an epoch it spells as a UUID; the
/// shared timeline contract uses a `u64`. Hashing one into the other would be
/// smaller and is exactly wrong: two epochs colliding would silently splice two
/// numberings into one cursor. So the mapping is allocated once, persisted, and
/// restored — and an un-restorable mapping blocks content rather than being
/// re-derived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EpochRegistry {
    by_raw: BTreeMap<String, u64>,
    next: u64,
}

impl EpochRegistry {
    /// Rebuild from a checkpoint, refusing a mapping that is not injective.
    fn restore(pairs: &[(String, u64)]) -> RuntimeResult<Self> {
        let mut by_raw = BTreeMap::new();
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        for (raw, epoch) in pairs {
            if *epoch == 0 {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.epochs",
                    "epoch 0 is the anchor before every event and names no numbering",
                )));
            }
            if !seen.insert(*epoch) {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.epochs",
                    "two raw epochs map to one Kontor epoch",
                )));
            }
            if by_raw.insert(raw.clone(), *epoch).is_some() {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.epochs",
                    "one raw epoch maps to two Kontor epochs",
                )));
            }
        }
        let next = seen.iter().next_back().copied().unwrap_or(0);
        Ok(Self { by_raw, next })
    }

    /// The Kontor epoch for `raw`, allocating one only for an epoch never seen.
    fn resolve(&mut self, raw: &str) -> u64 {
        if let Some(known) = self.by_raw.get(raw) {
            return *known;
        }
        self.next = self.next.saturating_add(1);
        self.by_raw.insert(raw.to_owned(), self.next);
        self.next
    }

    /// The Kontor epoch for `raw`, without allocating.
    fn known(&self, raw: &str) -> Option<u64> {
        self.by_raw.get(raw).copied()
    }

    /// The raw epoch a Kontor one was allocated for.
    ///
    /// The inverse lookup exists because 0.3.1 cursors are `{epoch, seq}` pairs
    /// in Paseo's own spelling: reading strictly after a stored position means
    /// naming the raw epoch again, and a Kontor `u64` cannot be turned back into
    /// a UUID by anything but this map.
    fn raw(&self, epoch: u64) -> Option<String> {
        self.by_raw
            .iter()
            .find(|(_, mapped)| **mapped == epoch)
            .map(|(raw, _)| raw.clone())
    }

    fn pairs(&self) -> Vec<(String, u64)> {
        self.by_raw
            .iter()
            .map(|(raw, epoch)| (raw.clone(), *epoch))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Checkpoint
// ---------------------------------------------------------------------------

/// Everything the adapter needs to be rebuilt after a Kontor restart.
///
/// The adapter defines no storage interface and opens no database. This is a
/// plain value the existing KON-MVP-03/05 tables already hold, and
/// [`PaseoAdapter::new`] takes it back — validating it rather than trusting it,
/// because a checkpoint reassembled from separate tables can disagree with
/// itself in ways a live adapter never could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoCheckpoint {
    /// The adapter generation the native ids belong to.
    pub generation: u64,
    /// The host key these bindings were made against.
    pub host_key: ExternalName,
    /// The epic project binding.
    pub project: Option<PaseoProjectBinding>,
    /// Additional epic project bindings held by this host-level adapter.
    ///
    /// `project` remains the compatibility slot for the one epic declared by
    /// older fleet settings. New epics are keyed here so a checkpoint never
    /// collapses several native projects back into one on restart.
    pub projects: Vec<PaseoProjectBinding>,
    /// Every prepared task workspace, by team run.
    pub workspaces: Vec<WorkspaceBindingSnapshot>,
    /// Every session binding the adapter issued and has not invalidated.
    pub bindings: Vec<RuntimeBindingSnapshot>,
    /// Every team-run seat holding one of those sessions.
    pub seats: Vec<OccupiedSeat>,
    /// Every seat whose launch was in flight when this was taken.
    pub claims: Vec<ClaimedSeat>,
    /// The full correlation chain per seat.
    pub records: Vec<PaseoSeatRecord>,
    /// The Paseo workspace each live seat is placed in.
    pub placements: Vec<(RuntimeBindingId, ExternalId)>,
    /// The message delivery ledger, in commit order.
    pub deliveries: Vec<(MessageId, ContentHash, PaseoDelivery)>,
    /// Every permission request observed still pending, with the session that
    /// raised it.
    pub pending_permissions: Vec<(RuntimeBindingId, ExternalId)>,
    /// Every permission answer already committed.
    pub permission_acks: Vec<(ExternalId, PermissionAck)>,
    /// Every request canonical history shows already answered.
    ///
    /// Distinct from [`PaseoCheckpoint::permission_acks`], which holds only the
    /// answers *Kontor* sent. A request the operator answered in Paseo's own UI
    /// produces no acknowledgement to record — there is no response id and no
    /// decision to invent — but answering it again would still act a second
    /// time on someone else's behalf, so the fact has to survive a restart on
    /// its own.
    pub resolved_in_history: Vec<ExternalId>,
    /// The raw-epoch registry.
    pub epochs: Vec<(String, u64)>,
    /// The last canonical position served per binding.
    pub cursors: Vec<(RuntimeBindingId, TimelinePosition)>,
}

impl PaseoCheckpoint {
    /// A fresh plane with no history, in `generation`, against `host_key`.
    #[must_use]
    pub fn fresh(generation: u64, host_key: ExternalName) -> Self {
        Self {
            generation,
            host_key,
            project: None,
            projects: Vec::new(),
            workspaces: Vec::new(),
            bindings: Vec::new(),
            seats: Vec::new(),
            claims: Vec::new(),
            records: Vec::new(),
            placements: Vec::new(),
            deliveries: Vec::new(),
            pending_permissions: Vec::new(),
            permission_acks: Vec::new(),
            resolved_in_history: Vec::new(),
            epochs: Vec::new(),
            cursors: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct PaseoState {
    generation: u64,
    server: Option<PaseoServerInfo>,
    projects: BTreeMap<ExternalId, PaseoProjectBinding>,
    workspaces: BTreeMap<TeamRunId, WorkspaceBindingSnapshot>,
    /// One native container per topology node.
    ///
    /// Keyed by node rather than by team run: a node outlives every TeamRun
    /// inside it, so a ledger keyed by the run would make the second run of one
    /// ticket look like a new place to put things.
    containers: BTreeMap<TopologyNodeId, ContainerBindingSnapshot>,
    /// Consultation launches currently between exact-label census and readback.
    /// The native census is the durable uniqueness proof; this closes the
    /// in-process race while that proof is being established.
    consultation_claims: BTreeSet<SeatBindingId>,
    /// Persistent topology-seat launches currently between their exact-label
    /// census and readback.
    hosted_seat_claims: BTreeSet<SeatBindingId>,
    bindings: IssuedBindingRegistry,
    admissions: AdmissionLedger,
    records: BTreeMap<RuntimeBindingId, PaseoSeatRecord>,
    /// The Paseo workspace each live seat is placed in.
    ///
    /// Narrower than [`PaseoSeatRecord`] on purpose. The record is the whole
    /// correlation chain a launch wrote and carries Kontor-side facts — the task,
    /// the team run, the role slot — that this adapter cannot re-derive from the
    /// runtime. Placement is the one part of it every *driving* operation needs,
    /// and it is the one part the live runtime can still answer for on its own:
    /// an agent reports which workspace it sits in. Keeping it separately is what
    /// lets a restart recover the ability to drive a seat without fabricating a
    /// correlation chain nobody recorded.
    placements: BTreeMap<RuntimeBindingId, ExternalId>,
    messages: MessageLedger<PaseoDelivery>,
    deliveries: Vec<(MessageId, ContentHash, PaseoDelivery)>,
    permissions: PermissionLedger,
    /// The session that raised each request still awaiting an answer.
    ///
    /// The shared [`PermissionLedger`] decides; this remembers. It exists
    /// because the ledger does not expose *who* a pending request belongs to,
    /// and a checkpoint that cannot name the session cannot restore the request.
    permission_owners: BTreeMap<ExternalId, RuntimeBindingId>,
    /// Requests canonical history shows answered, whoever answered them.
    ///
    /// The shared ledger closes a request only against a [`PermissionAck`], and
    /// an answer Kontor did not send has none. So the close is recorded here
    /// instead, and [`PaseoAdapter::respond_permission`] consults it *after* the
    /// ledger has had its say: a replay of Kontor's own answer is idempotent,
    /// and anything else is refused before it reaches the wire.
    resolved_in_history: BTreeSet<ExternalId>,
    permission_acks: Vec<(ExternalId, PermissionAck)>,
    adoptions: BTreeMap<ExternalId, PaseoAdoptionIntent>,
    epochs: EpochRegistry,
    cursors: BTreeMap<RuntimeBindingId, TimelinePosition>,
    request_seq: u64,
}

/// This adapter's answers to the two questions the shared ledger cannot answer.
struct PaseoSeatFacts<'a> {
    bindings: &'a IssuedBindingRegistry,
    generation: u64,
}

impl SeatFacts for PaseoSeatFacts<'_> {
    fn issued_binding(&self, binding_id: RuntimeBindingId) -> Option<RuntimeBindingSnapshot> {
        self.bindings.get(binding_id).cloned()
    }

    /// What Paseo can prove synchronously, which is retirement and not
    /// completion.
    ///
    /// A binding from an older generation, or one this adapter no longer holds,
    /// is retired: it cannot keep a seat, and saying so needs no request.
    /// Completion is the half that cannot be answered here — it needs a fresh
    /// archived readback, which is an `await` this adapter must not make while
    /// holding its state lock. So a replacement over a live current-generation
    /// seat is refused as not evidenced rather than admitted on a stale read;
    /// [`PaseoAdapter::retire`] is the path that produces the evidence first.
    fn holder_is_finished_or_retired(
        &self,
        binding_id: RuntimeBindingId,
        _native_id: &ExternalId,
    ) -> bool {
        match self.bindings.get(binding_id) {
            None => true,
            Some(snapshot) => snapshot.identity().generation != self.generation,
        }
    }
}

/// The Paseo runtime adapter for one ticket's execution plane.
#[derive(Debug)]
pub struct PaseoAdapter {
    config: PaseoConfig,
    transport: Box<dyn PaseoTransport>,
    /// The daemon's MCP facade, when this plane has a route to it.
    ///
    /// Optional because it is a *third* surface, and an adapter that cannot
    /// reach it must say so rather than fail at the moment of use: with no route
    /// the retitle capability is undeclared, so a caller can tell before asking.
    mcp: Option<Box<dyn PaseoMcp>>,
    state: Mutex<PaseoState>,
}

impl PaseoAdapter {
    fn ensure_provider_available(&self, provider: &str) -> RuntimeResult<()> {
        if self.config.unavailable_providers.contains(provider) {
            return Err(RuntimeError::ProviderUnavailable {
                provider: provider.to_owned(),
            });
        }
        Ok(())
    }

    /// Build an adapter for `config` over `transport`, rehydrated from
    /// `checkpoint`.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for a checkpoint whose host key is not
    /// this plane's, whose bindings belong to another generation, that holds two
    /// seats for one role slot or two bindings for one native agent, or whose
    /// epoch registry is not injective. Every one of those describes a runtime
    /// state that cannot have existed, and restoring it would make the rules
    /// below decide against a fiction.
    pub fn new(
        config: PaseoConfig,
        transport: Box<dyn PaseoTransport>,
        checkpoint: PaseoCheckpoint,
    ) -> RuntimeResult<Self> {
        // `mini_project_id` selects which durable epic may receive this
        // plane's legacy rendering overrides. Validate that boundary even for
        // embedders that construct an adapter without the daemon fleet loader;
        // otherwise a Jira key remains latent until the first admission.
        kontor_core::id::MiniProjectId::parse(config.mini_project_id.as_str()).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                "PaseoConfig.mini_project_id",
                "must be a durable Kontor MiniProjectId",
            ))
        })?;
        if checkpoint.host_key != config.host_key {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "PaseoCheckpoint.host_key",
                "was taken against another Paseo host",
            )));
        }
        let mut checkpoint_projects = checkpoint.projects.clone();
        if let Some(project) = &checkpoint.project {
            checkpoint_projects.push(project.clone());
        }
        let mut project_keys = BTreeSet::new();
        let mut project_ids = BTreeSet::new();
        for project in &checkpoint_projects {
            if project.host_key != config.host_key {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.projects",
                    "binds a project from another host",
                )));
            }
            if !project_keys.insert(project.mini_project_id.clone())
                || !project_ids.insert(project.project_id.clone())
            {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.projects",
                    "binds one epic or one native project more than once",
                )));
            }
        }

        let mut natives: BTreeSet<&str> = BTreeSet::new();
        for snapshot in &checkpoint.bindings {
            if snapshot.identity().generation != checkpoint.generation {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.bindings",
                    "carries a binding from another runtime generation",
                )));
            }
            if !natives.insert(snapshot.identity().native_id.as_str()) {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.bindings",
                    "binds one native agent to two runs",
                )));
            }
        }

        let mut slots: BTreeSet<RoleSlotKey> = BTreeSet::new();
        for seat in &checkpoint.seats {
            if !slots.insert(seat.slot.clone()) {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCheckpoint.seats",
                    "holds one role slot twice",
                )));
            }
        }

        let epochs = EpochRegistry::restore(&checkpoint.epochs)?;

        let mut messages = MessageLedger::new();
        for (id, hash, delivery) in &checkpoint.deliveries {
            messages.record(*id, hash.clone(), delivery.clone());
        }
        let resolved_in_history: BTreeSet<ExternalId> =
            checkpoint.resolved_in_history.iter().cloned().collect();
        let mut permissions = PermissionLedger::new();
        let mut permission_owners = BTreeMap::new();
        // Pending first, then resolved: `record` closes a pending entry, so
        // replaying them the other way round would reopen an answered request.
        for (binding_id, permission_id) in &checkpoint.pending_permissions {
            permissions.open(*binding_id, permission_id.clone());
            permission_owners.insert(permission_id.clone(), *binding_id);
        }
        for (permission_id, acknowledgement) in &checkpoint.permission_acks {
            permissions.record(permission_id.clone(), acknowledgement.clone());
            permission_owners.remove(permission_id);
        }

        Ok(Self {
            config,
            transport,
            mcp: None,
            state: Mutex::new(PaseoState {
                generation: checkpoint.generation,
                server: None,
                projects: checkpoint_projects
                    .into_iter()
                    .map(|project| (project.mini_project_id.clone(), project))
                    .collect(),
                workspaces: checkpoint
                    .workspaces
                    .iter()
                    .map(|snapshot| (snapshot.binding.team_run_id, snapshot.clone()))
                    .collect(),
                // Deliberately empty on every build, including a restore: a
                // rebuilt adapter has no container ledger, and the way back to
                // a node's container is the id Kontor persisted, not a copy
                // this process kept. Seeding it from a checkpoint would hide
                // the one path the restart fixture exists to exercise.
                containers: BTreeMap::new(),
                consultation_claims: BTreeSet::new(),
                hosted_seat_claims: BTreeSet::new(),
                placements: checkpoint.placements.iter().cloned().collect(),
                bindings: {
                    let mut registry = IssuedBindingRegistry::new();
                    for snapshot in &checkpoint.bindings {
                        registry.record(snapshot.clone());
                    }
                    registry
                },
                admissions: {
                    let mut ledger = AdmissionLedger::new();
                    // Claims first, so a recorded session wins over a claim for
                    // the same seat: of the two readings the occupancy is the
                    // evidenced one.
                    for claim in checkpoint.claims.iter().cloned() {
                        ledger.restore_claimed(claim);
                    }
                    for seat in checkpoint.seats.iter().cloned() {
                        ledger.restore_occupied(seat);
                    }
                    ledger
                },
                records: checkpoint
                    .records
                    .iter()
                    .map(|record| (record.binding_id, record.clone()))
                    .collect(),
                messages,
                deliveries: checkpoint.deliveries.clone(),
                permissions,
                permission_owners,
                resolved_in_history,
                permission_acks: checkpoint.permission_acks.clone(),
                adoptions: BTreeMap::new(),
                epochs,
                cursors: checkpoint.cursors.iter().copied().collect(),
                request_seq: 0,
            }),
        })
    }

    /// Give this adapter a route to the daemon's MCP facade.
    ///
    /// A builder rather than a constructor argument, because every other
    /// operation this adapter performs works without it: the facade is needed for
    /// the one verb the CLI and the session socket do not have. An adapter built
    /// without it is fully functional and honestly declares one capability
    /// fewer.
    #[must_use]
    pub fn with_mcp(mut self, mcp: Box<dyn PaseoMcp>) -> Self {
        self.mcp = Some(mcp);
        self
    }

    /// The plane this adapter drives.
    #[must_use]
    pub const fn config(&self) -> &PaseoConfig {
        &self.config
    }

    /// The current adapter generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// The persistable state.
    #[must_use]
    pub fn checkpoint(&self) -> PaseoCheckpoint {
        let state = self.lock();
        PaseoCheckpoint {
            generation: state.generation,
            host_key: self.config.host_key.clone(),
            project: state.projects.get(&self.config.mini_project_id).cloned(),
            projects: state
                .projects
                .iter()
                .filter(|(epic, _)| *epic != &self.config.mini_project_id)
                .map(|(_, project)| project.clone())
                .collect(),
            workspaces: state.workspaces.values().cloned().collect(),
            bindings: state.bindings.snapshots().cloned().collect(),
            seats: state.admissions.occupied_seats().collect(),
            claims: state.admissions.claimed_seats().collect(),
            records: state.records.values().cloned().collect(),
            placements: state
                .placements
                .iter()
                .map(|(binding, workspace)| (*binding, workspace.clone()))
                .collect(),
            deliveries: state.deliveries.clone(),
            pending_permissions: state
                .permission_owners
                .iter()
                .map(|(permission_id, binding_id)| (*binding_id, permission_id.clone()))
                .collect(),
            permission_acks: state.permission_acks.clone(),
            resolved_in_history: state.resolved_in_history.iter().cloned().collect(),
            epochs: state.epochs.pairs(),
            cursors: state.cursors.iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }

    /// The correlation chain recorded for one binding.
    #[must_use]
    pub fn seat_record(&self, binding_id: RuntimeBindingId) -> Option<PaseoSeatRecord> {
        self.lock().records.get(&binding_id).cloned()
    }

    /// The Paseo workspace one live seat is placed in.
    #[must_use]
    pub fn placement(&self, binding_id: RuntimeBindingId) -> Option<ExternalId> {
        self.lock().placements.get(&binding_id).cloned()
    }

    /// The container binding this plane currently holds for one topology node.
    ///
    /// Absent after a restart even while the container exists, which is the
    /// distinction the whole reconcile-by-stored-id path rests on: this answers
    /// "does *this process* know where the node is", never "does the node have
    /// a container".
    #[must_use]
    pub fn container_binding(&self, node: TopologyNodeId) -> Option<ContainerBindingSnapshot> {
        self.lock().containers.get(&node).cloned()
    }

    /// The epic project binding, once preparation has established one.
    #[must_use]
    pub fn project_binding(&self) -> Option<PaseoProjectBinding> {
        self.lock()
            .projects
            .get(&self.config.mini_project_id)
            .cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PaseoState> {
        self.state.lock().expect("the Paseo adapter lock is intact")
    }

    /// A fresh, unique protocol correlation id.
    fn next_request_id(&self) -> String {
        let state = &mut *self.lock();
        state.request_seq = state.request_seq.saturating_add(1);
        format!("kon-{}-{}", state.generation, state.request_seq)
    }

    fn identity(&self, native_id: ExternalId, generation: u64) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: self.config.runtime_kind.clone(),
            host: self.config.host_key.clone(),
            generation,
            native_id,
        }
    }

    /// Refuse an operation this plane cannot perform, before anything is
    /// dispatched.
    fn refuse(
        &self,
        capability: RuntimeCapability,
        declared: &RuntimeCapabilities,
    ) -> RuntimeError {
        preflight(declared, &OperationContext::new(capability))
            .expect_err("this capability is not declared at this grade")
    }

    /// Resolve a presented binding to the runtime's **own** copy, before any
    /// effect.
    ///
    /// A [`RuntimeBindingSnapshot`] is a plain value with public fields, so a
    /// self-consistent one costs nothing to fabricate and `preflight` cannot
    /// catch it: it checks a snapshot against itself. Only the registry knows
    /// what this runtime issued. Addressing follows from the same copy, so a
    /// doctored snapshot cannot redirect a message into another session.
    fn attested(&self, claimed: &RuntimeBindingSnapshot) -> RuntimeResult<RuntimeBindingSnapshot> {
        self.lock()
            .bindings
            .attest(claimed)
            .map(|issued| issued.snapshot().clone())
    }

    // -- Fresh reads --------------------------------------------------------

    /// The daemon identity this connection was gated on.
    ///
    /// 0.3.1 pushes `status/server_info` once per accepted connection instead of
    /// answering a `server_info` request, so the transport holds it and this
    /// reads that copy. It is still a claim about the daemon Kontor is driving
    /// *now*: a reconnect re-gates, and an ungated transport refuses here rather
    /// than returning a stale one.
    async fn fetch_server_info(&self) -> RuntimeResult<PaseoServerInfo> {
        self.transport.server_identity().await
    }

    async fn fetch_projects(&self) -> RuntimeResult<Vec<PaseoProject>> {
        let request = PaseoRpc::project_list(self.next_request_id());
        let frame = self.transport.request(&request).await?;
        let listed: PaseoProjectList = frame.resolve(&request, "PaseoProjectList")?;
        Ok(listed.projects)
    }

    /// Every workspace in one project, over a bounded number of bounded pages.
    ///
    /// The page budget is a refusal rather than a truncation: an enumeration
    /// that ran out of budget has *not* seen every workspace, and "no workspace
    /// claims this path" read off a partial list is how a second workspace gets
    /// created at a path that already had one.
    async fn fetch_workspaces(&self, project_id: &str) -> RuntimeResult<Vec<PaseoWorkspace>> {
        let mut found = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_DIRECTORY_PAGES {
            let request = PaseoRpc::workspace_list(
                self.next_request_id(),
                project_id,
                MAX_DIRECTORY_PAGE,
                cursor.as_deref(),
            );
            let frame = self.transport.request(&request).await?;
            let page: PaseoWorkspacePage = frame.resolve(&request, "PaseoWorkspacePage")?;
            found.extend(page.entries);
            match page.page_info.next() {
                Some(next) => cursor = Some(next.to_owned()),
                None => return Ok(found),
            }
        }
        Err(RuntimeError::Transport {
            rule: "the workspace directory did not end within the bounded page budget",
        })
    }

    /// The authoritative readback of one workspace, by exact id.
    ///
    /// 0.3.1 has no fetch-one-workspace request, so this is the project's
    /// directory plus an exact-id select. The select is exact on purpose: a
    /// prefix or a name match would resolve to whichever workspace sorted first.
    async fn fetch_workspace_in(
        &self,
        project: &PaseoProjectBinding,
        workspace_id: &str,
    ) -> RuntimeResult<PaseoWorkspace> {
        self.fetch_workspaces(project.project_id.as_str())
            .await?
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or(RuntimeError::CorrelationFailed)
    }

    async fn fetch_workspace(&self, workspace_id: &str) -> RuntimeResult<PaseoWorkspace> {
        let projects = self.lock().projects.values().cloned().collect::<Vec<_>>();
        let mut found = Vec::new();
        for project in &projects {
            found.extend(
                self.fetch_workspaces(project.project_id.as_str())
                    .await?
                    .into_iter()
                    .filter(|workspace| workspace.id == workspace_id),
            );
        }
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(RuntimeError::CorrelationFailed),
            _ => Err(RuntimeError::WorkspaceMismatch {
                rule: "one workspace id was reported under several epic projects",
            }),
        }
    }

    /// Every agent carrying exactly `labels`, over a bounded number of pages.
    ///
    /// The filter is the daemon's, and the exact-label check is still this
    /// adapter's: [`PaseoAgent::matches_labels`] runs again at every decision
    /// point, so a daemon that widened its filter cannot widen a census.
    async fn fetch_agents(
        &self,
        labels: &BTreeMap<String, String>,
        include_archived: bool,
    ) -> RuntimeResult<Vec<PaseoAgent>> {
        let mut found = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_DIRECTORY_PAGES {
            let request = PaseoRpc::agent_list(
                self.next_request_id(),
                labels,
                include_archived,
                MAX_DIRECTORY_PAGE,
                cursor.as_deref(),
            );
            let frame = self.transport.request(&request).await?;
            let page: PaseoAgentPage = frame.resolve(&request, "PaseoAgentPage")?;
            found.extend(page.entries.into_iter().map(|entry| entry.agent));
            match page.page_info.next() {
                Some(next) => cursor = Some(next.to_owned()),
                None => return Ok(found),
            }
        }
        Err(RuntimeError::Transport {
            rule: "the agent directory did not end within the bounded page budget",
        })
    }

    /// Every agent this plane's epic project holds, placed through its
    /// workspaces.
    ///
    /// A 0.3.1 agent snapshot has no `projectId`, and the directory's own
    /// project filter keys on `projectKey` — the Git remote, which the live
    /// daemon shares between several projects, and which this adapter refuses to
    /// bind an epic by (ALT-003). So the project census is: every workspace in
    /// the bound project, then every agent sitting in one of them.
    async fn fetch_project_agents(
        &self,
        project: &PaseoProjectBinding,
        include_archived: bool,
    ) -> RuntimeResult<Vec<PaseoAgent>> {
        let workspaces: BTreeSet<String> = self
            .fetch_workspaces(project.project_id.as_str())
            .await?
            .into_iter()
            .map(|workspace| workspace.id)
            .collect();
        Ok(self
            .fetch_agents(&BTreeMap::new(), include_archived)
            .await?
            .into_iter()
            .filter(|agent| {
                agent
                    .workspace_id
                    .as_deref()
                    .is_some_and(|id| workspaces.contains(id))
            })
            .collect())
    }

    /// The authoritative readback of one agent, by exact id.
    ///
    /// This is the only place a single agent is read, and it is where the answer
    /// is checked against the id that was asked for. Every caller therefore
    /// branches on a validated view, which matters because the branch is what
    /// decides whether a process is restarted.
    ///
    /// 0.3.1 resolves `agentId` by full id, unique prefix *or* exact title, and
    /// answers an unknown one with `agent: null` and an error string. Both are
    /// refusals here, but they are not the same refusal: a null agent means the
    /// exact persisted binding is stale, while a prefix that resolved to
    /// somebody else's session is correlation drift.
    async fn fetch_agent(&self, agent_id: &str) -> RuntimeResult<PaseoAgent> {
        let request = PaseoRpc::agent_fetch(self.next_request_id(), agent_id);
        let frame = self.transport.request(&request).await?;
        let answer: PaseoAgentAnswer = frame.resolve(&request, "PaseoAgentAnswer")?;
        let agent = answer.agent.ok_or(RuntimeError::StaleBinding {
            rule: "the exact native agent no longer exists",
        })?;
        if agent.id != agent_id {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(agent)
    }

    /// Read one exact agent even after Paseo has moved it out of the active
    /// lookup surface.
    ///
    /// Paseo's exact `agent.fetch` returns `agent: null` once an agent is
    /// archived, while the paginated directory still returns that same exact
    /// identity when `includeArchived` is set. Hosted-seat retirement must
    /// prove the archive stamp after issuing the archive command, and a retry
    /// must be able to prove it again. Keep this fallback private to terminal
    /// retirement so an archived session can never become driveable through a
    /// normal exact lookup.
    async fn fetch_agent_including_archived(&self, agent_id: &str) -> RuntimeResult<PaseoAgent> {
        self.find_agent_including_archived(agent_id)
            .await?
            .ok_or(RuntimeError::StaleBinding {
                rule: "the exact native agent no longer exists",
            })
    }

    async fn find_agent_including_archived(
        &self,
        agent_id: &str,
    ) -> RuntimeResult<Option<PaseoAgent>> {
        match self.fetch_agent(agent_id).await {
            Ok(agent) => Ok(Some(agent)),
            Err(RuntimeError::StaleBinding { .. }) => Ok(self
                .fetch_agents(&BTreeMap::new(), true)
                .await?
                .into_iter()
                .find(|agent| agent.id == agent_id)),
            Err(error) => Err(error),
        }
    }

    async fn hosted_seat_agent(
        &self,
        request: &HostedSeatInspectRequest,
    ) -> RuntimeResult<Option<PaseoAgent>> {
        if request.identity.runtime_kind != self.config.runtime_kind
            || request.identity.host != self.config.host_key
            || request.identity.generation > self.generation()
        {
            return Err(RuntimeError::StaleBinding {
                rule: "the hosted topology predecessor belongs to another runtime",
            });
        }
        let Some(agent) = self
            .find_agent_including_archived(request.identity.native_id.as_str())
            .await?
        else {
            return Ok(None);
        };
        let expected_seat = request.seat_binding_id.to_string();
        if agent.label(label::SEAT_BINDING) != Some(expected_seat.as_str())
            || agent.label(label::HOSTED_SEAT) != Some("true")
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        if !agent.is_archived() {
            Self::verify_agent_route(&agent, &request.model_rung, SeatAutonomy::Supervised)?;
        }
        Ok(Some(agent))
    }

    async fn fetch_agent_with_archive(
        &self,
        agent_id: &str,
        correlation: &CorrelationLabel,
    ) -> RuntimeResult<PaseoAgent> {
        let agent = self.fetch_agent(agent_id).await?;
        if agent.is_archived() {
            return Ok(agent);
        }
        let correlation = correlation.to_string();
        let project = self.project_for_agent(&agent)?;
        Ok(self
            .fetch_project_agents(&project, true)
            .await?
            .into_iter()
            .find(|candidate| {
                candidate.id == agent_id
                    && candidate.label(label::AGENT_RUN) == Some(correlation.as_str())
                    && candidate.is_archived()
            })
            .unwrap_or(agent))
    }

    // -- Placement ----------------------------------------------------------

    fn external_epic_id(scope: &ExecutionScope) -> RuntimeResult<ExternalId> {
        ExternalId::parse(&scope.epic.mini_project_id.to_string()).map_err(Into::into)
    }

    fn configured_epic_id(&self) -> RuntimeResult<kontor_core::id::MiniProjectId> {
        kontor_core::id::MiniProjectId::parse(self.config.mini_project_id.as_str())
            .map_err(Into::into)
    }

    /// Apply the old fleet document only as a rendering compatibility override.
    ///
    /// Durable request scope remains the authority for which epic and task are
    /// addressed. The override is limited to that exact configured epic and
    /// preserves the existing native workspace/seat titles while the fleet
    /// moves off its historical per-task spelling.
    fn effective_scope(&self, scope: &ExecutionScope) -> RuntimeResult<ExecutionScope> {
        if scope.epic.mini_project_id != self.configured_epic_id()? {
            return Ok(scope.clone());
        }
        let mut effective = scope.clone();
        effective.epic.external_epic_key = self.config.scope.jira_epic_key.clone();
        effective.epic.short_title = self.config.scope.mini_project_short_title.clone();
        if let Some(task) = effective.task.as_mut()
            && let Some(configured) = self.config.scope.configured_task_scope(task.task_id)
        {
            task.external_issue_key = configured.jira_issue_key;
            task.short_code = configured.ticket_short_code;
        }
        Ok(effective)
    }

    fn scoped_plan_item_key(&self, scope: &ExecutionScope) -> RuntimeResult<ExternalId> {
        if scope.epic.mini_project_id == self.configured_epic_id()?
            && let Some(configured) = self
                .config
                .scope
                .configured_task_scope(scope.require_task()?.task_id)
        {
            return Ok(configured.plan_item_key);
        }
        Ok(scope.require_task()?.external_issue_key.clone())
    }

    /// The epic project binding, or a refusal that names what is missing.
    fn require_project_for(&self, scope: &ExecutionScope) -> RuntimeResult<PaseoProjectBinding> {
        let key = Self::external_epic_id(scope)?;
        self.lock()
            .projects
            .get(&key)
            .cloned()
            .ok_or(RuntimeError::WorkspaceMismatch {
                rule: "this epic's project has not been prepared on this Paseo host",
            })
    }

    fn project_for_agent(&self, agent: &PaseoAgent) -> RuntimeResult<PaseoProjectBinding> {
        let key = agent
            .label(label::PROJECT_ID)
            .ok_or(RuntimeError::CorrelationFailed)?;
        let key = ExternalId::parse(key)?;
        self.lock()
            .projects
            .get(&key)
            .cloned()
            .ok_or(RuntimeError::WorkspaceMismatch {
                rule: "the agent names an epic project this adapter has not prepared",
            })
    }

    fn project_for_native_id(&self, native_id: &str) -> RuntimeResult<PaseoProjectBinding> {
        self.lock()
            .projects
            .values()
            .find(|project| project.project_id.as_str() == native_id)
            .cloned()
            .ok_or(RuntimeError::WorkspaceMismatch {
                rule: "the native project is not bound to an epic on this adapter",
            })
    }

    async fn recover_project_for_agent(
        &self,
        agent: &PaseoAgent,
    ) -> RuntimeResult<PaseoProjectBinding> {
        if let Ok(project) = self.project_for_agent(agent) {
            return Ok(project);
        }
        let epic_id = agent
            .label(label::PROJECT_ID)
            .ok_or(RuntimeError::CorrelationFailed)?;
        let epic_id = ExternalId::parse(epic_id)?;
        let workspace_id = agent
            .workspace_id
            .as_deref()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        let mut owners = Vec::new();
        for project in self.fetch_projects().await? {
            if self
                .fetch_workspaces(&project.id)
                .await?
                .iter()
                .any(|workspace| workspace.id == workspace_id)
            {
                owners.push(project);
            }
        }
        let project = match owners.len() {
            1 => owners.remove(0),
            0 => return Err(RuntimeError::CorrelationFailed),
            _ => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "one agent workspace was reported under several native projects",
                });
            }
        };
        let observed = project.display_name.clone();
        let _ = self.settle_project(
            epic_id.clone(),
            ExternalId::parse(&project.id)?,
            observed.clone(),
            observed,
        );
        self.lock()
            .projects
            .get(&epic_id)
            .cloned()
            .ok_or(RuntimeError::CorrelationFailed)
    }

    /// Refuse a workspace that is not exactly the task worktree this plane
    /// serves.
    ///
    /// Five checks, and each one is a different way a role ends up editing the
    /// wrong tree: another epic's project, a directory that is not the task
    /// worktree, the project root or any other non-worktree place, a tree Paseo
    /// provisioned for itself instead of registering the one Kontor prepared,
    /// and a workspace with no id at all.
    ///
    /// The one ownership exception is an operator-configured exact native id.
    /// That is the same explicit adoption boundary used for existing projects:
    /// path, project and kind still have to match, while the immutable native
    /// identity is named ahead of the readback. An ownership bit alone never
    /// widens placement.
    fn verify_workspace_placement(
        &self,
        workspace: &PaseoWorkspace,
        project: &PaseoProjectBinding,
        expected_root: &WorkspaceRoot,
    ) -> RuntimeResult<()> {
        if workspace.id.is_empty() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "Paseo reported a workspace with no id",
            });
        }
        if workspace.project_id != project.project_id.as_str() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the workspace belongs to another epic project",
            });
        }
        let reported = WorkspaceRoot::parse(&workspace.workspace_directory)?;
        if &reported != expected_root {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the workspace is not the canonical task worktree",
            });
        }
        if workspace.workspace_kind != PaseoWorkspaceKind::Worktree {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "a ticket role may not be placed in a root or plain local workspace",
            });
        }
        let configured_adoption = ExternalId::parse(&workspace.id).is_ok_and(|workspace_id| {
            self.config
                .adopted_containers
                .values()
                .any(|native_id| native_id == &workspace_id)
        });
        if workspace.is_paseo_owned_worktree() && !configured_adoption {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "Paseo provisioned its own worktree instead of registering the prepared one",
            });
        }
        Ok(())
    }

    /// Refuse a persistent epic leadership seat outside its exact ECP.
    ///
    /// An ECP is intentionally a local/directory workspace rooted at the
    /// epic's stable runtime directory. It is not a ticket worktree. Keeping
    /// this validator separate preserves the stricter ticket-role rule above
    /// instead of weakening it for every launch.
    fn verify_hosted_workspace_placement(
        &self,
        workspace: &PaseoWorkspace,
        project: &PaseoProjectBinding,
        expected_root: &WorkspaceRoot,
    ) -> RuntimeResult<()> {
        if workspace.id.is_empty() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "Paseo reported an ECP workspace with no id",
            });
        }
        if workspace.project_id != project.project_id.as_str() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the ECP workspace belongs to another epic project",
            });
        }
        if WorkspaceRoot::parse(&workspace.workspace_directory)? != *expected_root {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the ECP workspace is not rooted at the epic's canonical directory",
            });
        }
        if !matches!(
            workspace.workspace_kind,
            PaseoWorkspaceKind::Directory | PaseoWorkspaceKind::LocalCheckout
        ) {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "a Core Team seat may be placed only in the epic's local ECP workspace",
            });
        }
        Ok(())
    }

    /// The native workspace a launch will be placed in, and the directory it
    /// must be rooted at.
    ///
    /// A node-keyed container is resolved by the node it names, so a second
    /// delivery attempt at the same ticket lands in the same place and the
    /// TeamRun never participates in finding it. The workspace-keyed branch
    /// below it is the older path; no Kontor application service reaches it any
    /// more, and it is kept for direct adapter callers — contract fixtures and
    /// the runtimes that prepare workspaces but not projects — which have no
    /// topology to place against.
    ///
    /// The expected root is read off the binding rather than recomputed from the
    /// configured task scope: the container was created at a directory this
    /// adapter read back, and re-deriving it here would compare the scope
    /// against itself.
    fn launch_placement(&self, request: &LaunchRequest) -> RuntimeResult<LaunchPlace> {
        if let Some(presented) = request.container() {
            let prepared = self
                .lock()
                .containers
                .get(&presented.topology_node_id())
                .cloned()
                .ok_or(RuntimeError::WorkspaceBindingRequired)?;
            if &prepared != presented {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this is not the container this runtime prepared for that node",
                });
            }
            let root = prepared
                .binding
                .root
                .clone()
                .ok_or(RuntimeError::WorkspaceMismatch {
                    rule: "the bound container has no working directory to launch a seat in",
                })?;
            return Ok(LaunchPlace {
                workspace_id: prepared.binding.identity.native_id.as_str().to_owned(),
                root,
                binding: prepared.binding_id().to_string(),
            });
        }

        let presented = request
            .workspace()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        let prepared = self
            .lock()
            .workspaces
            .get(&request.team_run_id())
            .cloned()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        if &prepared != presented {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "this is not the task workspace this runtime prepared",
            });
        }
        Ok(LaunchPlace {
            workspace_id: prepared.binding.identity.native_id.as_str().to_owned(),
            root: self
                .effective_scope(request.scope())?
                .require_task()?
                .worktree
                .clone(),
            binding: prepared.binding_id().to_string(),
        })
    }

    /// The full label set one seat's agent must carry, exactly.
    fn seat_labels(
        &self,
        agent_run_id: AgentRunId,
        team_run_id: TeamRunId,
        role_slot_id: &RoleSlotId,
        execution_scope: &ExecutionScope,
        project: &PaseoProjectBinding,
        workspace_id: &str,
    ) -> RuntimeResult<BTreeMap<String, String>> {
        let scope = self.effective_scope(execution_scope)?;
        let task = scope.require_task()?;
        Ok([
            (
                label::AGENT_RUN,
                CorrelationLabel::for_run(agent_run_id).to_string(),
            ),
            (
                label::JIRA_ISSUE,
                task.external_issue_key.as_str().to_owned(),
            ),
            (
                label::JIRA_EPIC,
                scope.epic.external_epic_key.as_str().to_owned(),
            ),
            (
                label::PROJECT_ID,
                project.mini_project_id.as_str().to_owned(),
            ),
            (label::TICKET, task.task_id.to_string()),
            // The same spelling a workspace carries, so one label key means one
            // thing everywhere. A bare team-run UUID here and a `kontor-team-`
            // prefixed one on the workspace would be two encodings of one fact,
            // which is how a census comes to match on the wrong half.
            (
                label::TEAM_RUN,
                WorkspaceLabel::for_team_run(team_run_id).to_string(),
            ),
            (label::ROLE, role_slot_id.as_str().to_owned()),
            (label::ROLE_SLOT, role_slot_id.as_str().to_owned()),
            (label::WORKSPACE_ID, workspace_id.to_owned()),
            (label::WORKTREE, task.worktree.as_str().to_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect())
    }

    /// The one label every seat of a team run carries, for a run-wide census.
    fn team_labels(&self, team_run_id: TeamRunId) -> BTreeMap<String, String> {
        [(
            label::TEAM_RUN.to_owned(),
            WorkspaceLabel::for_team_run(team_run_id).to_string(),
        )]
        .into_iter()
        .collect()
    }

    /// The subset of labels that identifies one seat, for a census.
    fn slot_labels(
        &self,
        team_run_id: TeamRunId,
        role_slot_id: &RoleSlotId,
    ) -> BTreeMap<String, String> {
        [
            (
                label::TEAM_RUN.to_owned(),
                WorkspaceLabel::for_team_run(team_run_id).to_string(),
            ),
            (
                label::ROLE_SLOT.to_owned(),
                role_slot_id.as_str().to_owned(),
            ),
        ]
        .into_iter()
        .collect()
    }

    /// Refuse an agent Paseo returned that is not the one this launch asked for.
    ///
    /// Kontor-created sessions are top-level inside an explicitly attested
    /// workspace. A parent label therefore proves drift, not ownership: it can
    /// only have come from a foreign/legacy launch and accepting it would permit
    /// cross-epic attachment.
    fn verify_agent_placement(
        &self,
        agent: &PaseoAgent,
        workspace_id: &str,
        wanted_labels: &BTreeMap<String, String>,
    ) -> RuntimeResult<()> {
        let expected_root = wanted_labels
            .get(label::WORKTREE)
            .ok_or(RuntimeError::CorrelationFailed)?;
        let expected_root = WorkspaceRoot::parse(expected_root)?;
        self.verify_agent_location(agent, workspace_id, &expected_root)?;
        if agent.parent_agent_id().is_some() {
            return Err(RuntimeError::CorrelationFailed);
        }
        if !agent.matches_labels(wanted_labels) {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(())
    }

    /// Where an agent is, judged on its own — no labels, no parent.
    ///
    /// Split out of [`PaseoAdapter::verify_agent_placement`] because the two
    /// questions have different lifetimes. Labels and parentage are settled once,
    /// when a seat is created; *location* has to be re-proved on every turn,
    /// because Paseo can move an agent between one turn and the next and the
    /// labels would follow it unchanged.
    ///
    /// The project half is not checked here and cannot be: a 0.3.1 agent
    /// snapshot has no `projectId`. It is proved one level up, by the workspace
    /// this agent must be in being verified as the bound project's — which is
    /// why every caller of this pairs it with
    /// [`PaseoAdapter::verify_workspace_placement`] on the same workspace id.
    fn verify_agent_location(
        &self,
        agent: &PaseoAgent,
        workspace_id: &str,
        expected_root: &WorkspaceRoot,
    ) -> RuntimeResult<()> {
        if agent.id.is_empty() {
            return Err(RuntimeError::CorrelationFailed);
        }
        if agent.workspace_id.as_deref() != Some(workspace_id) {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the agent is placed in another workspace",
            });
        }
        let reported = WorkspaceRoot::parse(&agent.cwd)?;
        if &reported != expected_root {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the agent is working outside the canonical task worktree",
            });
        }
        Ok(())
    }

    /// Re-prove one live seat's placement from fresh readbacks, before anything
    /// acts on it.
    ///
    /// A binding records where a session *was*. Every operation that drives an
    /// existing seat asks again, because both halves can change underneath it:
    /// Paseo can move an agent to another workspace, and the workspace itself
    /// can be re-registered as a project root, a plain local directory, or
    /// replaced by a worktree Paseo provisioned for itself. Either one turns the
    /// next message into an edit in somebody else's tree, and neither is visible
    /// from labels — which move with the agent and stay true.
    ///
    /// Called before the effect, never after: a refusal that arrives once the
    /// message is delivered is a description of the damage.
    async fn verify_seat_placement(
        &self,
        binding: &RuntimeBindingSnapshot,
        agent: &PaseoAgent,
    ) -> RuntimeResult<()> {
        let project = self.project_for_agent(agent)?;
        let workspace_id = self
            .placement(binding.binding_id())
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        let expected_root = agent
            .label(label::WORKTREE)
            .ok_or(RuntimeError::CorrelationFailed)?;
        let expected_root = WorkspaceRoot::parse(expected_root)?;
        self.verify_agent_location(agent, workspace_id.as_str(), &expected_root)?;
        // …and the workspace itself, which is where the project half of the
        // agent's placement is proved on this wire.
        let workspace = self
            .fetch_workspace_in(&project, workspace_id.as_str())
            .await?;
        self.verify_workspace_placement(&workspace, &project, &expected_root)?;
        // A live launch/adoption owns a full seat record and therefore freezes
        // its route. Restart restoration deliberately reconstructs only facts
        // Paseo can re-attest; its launch-time route remains in observation
        // evidence rather than being fabricated into a new seat record.
        if let Some(record) = self.seat_record(binding.binding_id())
            && (agent.provider != record.provider
                || agent.model != record.model
                || agent.effective_thinking_option_id != record.thinking)
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(())
    }

    fn verify_agent_route_with_mode(
        agent: &PaseoAgent,
        requested: &ModelRung,
        consultation_provenance: Option<&ConsultationRouteProvenance>,
        autonomy: SeatAutonomy,
    ) -> RuntimeResult<()> {
        if agent.provider != requested.provider.0 || agent.model != requested.model.0 {
            return Err(RuntimeError::CorrelationFailed);
        }
        if let Some(effort) = requested.effort
            && agent.effective_thinking_option_id.as_deref() != Some(effort.as_str())
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        let expected_mode = if let Some(provenance) = consultation_provenance {
            super::client::consultation_route_permission_mode(requested, provenance)?
        } else {
            paseo_mode(&requested.provider.0, autonomy)?
        };
        if agent.current_mode_id.as_deref() != expected_mode {
            return Err(RuntimeError::PermissionModeMismatch {
                provider: requested.provider.0.clone(),
                expected: expected_mode.map(str::to_owned),
                found: agent.current_mode_id.clone(),
            });
        }
        Ok(())
    }

    fn verify_agent_route(
        agent: &PaseoAgent,
        requested: &ModelRung,
        autonomy: SeatAutonomy,
    ) -> RuntimeResult<()> {
        Self::verify_agent_route_with_mode(agent, requested, None, autonomy)
    }

    // -- Evidence -----------------------------------------------------------

    /// Reduce one agent readback to a normalized lifecycle.
    ///
    /// Nothing here can produce [`ObservedRunState::Succeeded`] or
    /// [`ObservedRunState::Failed`]. Paseo has no success or failure verdict to
    /// offer about the *work*; it has a process and an attention hint. Inventing
    /// a verdict from `idle`, from `attentionReason=finished`, or from a stopped
    /// process is the single most consequential mistake available here, and it
    /// is also how a seat that is simply waiting gets replaced.
    ///
    /// The one terminal mapping is the archive stamp, and it only ever closes a
    /// run when a *fresh* read reports it — which by construction can only
    /// follow an explicit archive intent, since nothing else archives an agent.
    #[must_use]
    pub fn normalize_agent(agent: &PaseoAgent) -> (ObservedRunState, RuntimeContact) {
        // Retirement is a stamp rather than a status in 0.3.1, and it outranks
        // whatever the lifecycle says: an archived agent may still report
        // `idle`, and reading that as a live seat would resume a retired one.
        if agent.is_archived() {
            return (ObservedRunState::Cancelled, RuntimeContact::Reachable);
        }
        match agent.status {
            PaseoAgentStatus::Running => (ObservedRunState::Running, RuntimeContact::Reachable),
            // Alive, reusable, nothing in flight. `WaitingInput` is the honest
            // reading of a seat between turns, and it is non-terminal.
            PaseoAgentStatus::Idle => (ObservedRunState::WaitingInput, RuntimeContact::Reachable),
            PaseoAgentStatus::Initializing => {
                (ObservedRunState::Launching, RuntimeContact::Reachable)
            }
            // The session is stuck on something. `Blocked` is non-terminal and
            // says nothing about the work: a provider error, a refused tool and
            // an expired credential all land here, and none of them is a run
            // that failed.
            PaseoAgentStatus::Error => (ObservedRunState::Blocked, RuntimeContact::Reachable),
            // The process is gone. That is lost contact, and emphatically not a
            // verdict: an agent someone stopped and one that crashed look
            // identical from here.
            PaseoAgentStatus::Closed => (ObservedRunState::Unknown, RuntimeContact::ProcessMissing),
            PaseoAgentStatus::Unknown => (ObservedRunState::Unknown, RuntimeContact::Reachable),
        }
    }

    /// Canonicalize the raw readback together with the values the mapping read.
    ///
    /// The raw agent goes in first: evidence is persisted before any normalized
    /// consequence is applied, so a mapping that later turns out to be wrong can
    /// be re-derived from what Paseo actually said. The prompt and the transcript
    /// are not in an agent readback, so nothing here quotes the work.
    fn agent_evidence(agent: &PaseoAgent) -> DomainResult<CanonicalDocument> {
        let (state, contact) = Self::normalize_agent(agent);
        let raw = serde_json::to_string(agent)
            .map_err(|_| DomainError::invalid("PaseoAgent", "is not serializable as JSON"))?;
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "paseo_version": PASEO_APP_VERSION,
            "raw_digest": ContentHash::of(raw.as_bytes()).as_str(),
            "read": {
                "agent_id": agent.id,
                "provider": agent.provider,
                "model": agent.model,
                "thinking": agent.thinking_option_id,
                "effective_thinking": agent.effective_thinking_option_id,
                "permission_mode": agent.current_mode_id,
                "workspace_id": agent.workspace_id,
                "status": format!("{:?}", agent.status),
                "archived_at": agent.archived_at,
                "attention_reason": agent.attention_reason,
                "parent_agent_id": agent.parent_agent_id(),
                "provider_session_id": agent.provider_session_id(),
                "pending_permissions": agent
                    .pending_permissions
                    .iter()
                    .map(|pending| pending.id.as_str())
                    .collect::<Vec<_>>(),
            },
            "normalized": {
                "run_state": state.as_str(),
                "contact": contact.as_str(),
            },
        }))
    }

    fn observation(
        &self,
        agent_run_id: AgentRunId,
        identity: NativeRuntimeIdentity,
        agent: &PaseoAgent,
        observed_at: Timestamp,
        source: ObservationSource,
    ) -> RuntimeResult<ControlPlaneObservation> {
        let (state, contact) = Self::normalize_agent(agent);
        let native_sequence = u64::try_from(observed_at.as_microsecond()).map_err(|_| {
            DomainError::invalid(
                "Paseo observation",
                "timestamp cannot identify a current runtime read",
            )
        })?;
        Ok(ControlPlaneObservation {
            agent_run_id,
            contact,
            state,
            identity,
            native_event_id: None,
            // Paseo exposes no revision for an agent readback. Use the read's
            // timestamp as its control-plane order; content keeps its separate
            // canonical timeline sequence.
            native_sequence,
            observed_at,
            evidence: Self::agent_evidence(agent)?,
            source,
        })
    }

    fn bind(
        &self,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
        agent: &PaseoAgent,
        at: Timestamp,
        generation: u64,
        capabilities: RuntimeCapabilities,
    ) -> RuntimeResult<RuntimeBindingSnapshot> {
        let identity = self.identity(ExternalId::parse(&agent.id)?, generation);
        // The label is raw runtime text. `establish` accepts it only when it is
        // exactly the label Kontor planted for this run, so a native agent id or
        // another run's label is a refusal rather than a silent bind.
        let correlation = CorrelationEvidence::establish(
            agent_run_id,
            agent.label(label::AGENT_RUN).unwrap_or_default(),
            identity.clone(),
            at,
        )?;
        Ok(RuntimeBindingSnapshot {
            binding: RuntimeBinding {
                id: binding_id,
                agent_run_id,
                identity,
                bound_at: at,
            },
            capabilities,
            correlation,
        })
    }
}

// ---------------------------------------------------------------------------
// Adapter-specific operations: project, roster, adoption, retirement, compaction
// ---------------------------------------------------------------------------

impl PaseoAdapter {
    /// Make the epic project exist, idempotently, and say whether its name
    /// drifted.
    ///
    /// `command_id` is the durable Kontor command this preparation belongs to,
    /// and it becomes the `project.add` request id — so a redelivery of the same
    /// intent carries the same correlation and cannot be mistaken for a second
    /// project.
    ///
    /// # Errors
    /// * [`RuntimeError::Transport`] — the daemon could not be reached, or a
    ///   persisted binding no longer names a project it holds.
    /// * [`RuntimeError::WorkspaceMismatch`] — the daemon answered about another
    ///   project.
    pub async fn prepare_project(
        &self,
        command_id: &str,
        desired: &ExternalName,
    ) -> RuntimeResult<PaseoProjectOutcome> {
        // A persisted binding is authoritative, and it is attested by exact id
        // rather than re-derived. Re-deriving would re-open the very question
        // the binding exists to close.
        let bound = self
            .lock()
            .projects
            .get(&self.config.mini_project_id)
            .cloned();
        if let Some(binding) = bound {
            let project = self.read_project_by_id(binding.project_id.as_str()).await?;
            return Ok(self.settle_project(
                self.config.mini_project_id.clone(),
                binding.project_id,
                project.display_name,
                desired.as_str().to_owned(),
            ));
        }

        let projects = self.fetch_projects().await?;
        // Only an exact display-name match on a *fresh* list is treated as this
        // adapter's own prior effect, and only when there is exactly one. That
        // is the correlated-prior-effect test: `project.add` carries no label,
        // so the name is the one correlation available, and an ambiguous answer
        // must not become a second project.
        let mut correlated = projects
            .iter()
            .filter(|project| project.display_name == desired.as_str())
            .collect::<Vec<_>>();
        let project = match correlated.len() {
            1 => correlated.remove(0).clone(),
            0 => {
                let request = PaseoRpc::project_add(
                    command_id.to_owned(),
                    // The epic's repository root, never the task worktree: see
                    // `PaseoExecutionScope::project_root_cwd`.
                    self.config.scope.project_root_cwd.as_str(),
                );
                let frame = self.transport.request(&request).await?;
                let added: PaseoProjectAdded = frame.resolve(&request, "PaseoProjectAdded")?;
                let added = added.project.ok_or(RuntimeError::Transport {
                    rule: "runtime refused to register the epic project",
                })?;
                // Read back by exact id: the answer to `add` is an
                // acknowledgement, and a binding is made from a readback.
                self.read_project_by_id(&added.id).await?
            }
            _ => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "several Paseo projects carry this epic's display name",
                });
            }
        };

        let project_id = ExternalId::parse(&project.id)?;
        Ok(self.settle_project(
            self.config.mini_project_id.clone(),
            project_id,
            project.display_name,
            desired.as_str().to_owned(),
        ))
    }

    /// Re-attest the container a node is already bound to, by its exact id.
    ///
    /// The only question asked is "is *this* container still there". A readback
    /// that comes back empty is a refusal, never a licence to create a
    /// replacement: the container may be perfectly alive and merely unreachable,
    /// and a second one created beside it is not recoverable by observing it
    /// afterwards.
    async fn reconcile_container_by_id(
        &self,
        request: &ContainerRequest,
        projection: ContainerProjection,
        native_id: &ExternalId,
        scope: Option<&ExternalId>,
        declared: &RuntimeCapabilities,
        generation: u64,
    ) -> RuntimeResult<ContainerBindingSnapshot> {
        let (identity, correlation) = match projection {
            ContainerProjection::NativeRoot => {
                let project = self.read_project_by_id(native_id.as_str()).await?;
                let identity = self.identity(ExternalId::parse(&project.id)?, generation);
                // A project carries no label surface, so the readback of the
                // exact id *is* the correlation. Its display name is compared
                // nowhere: an operator who renamed it did not move it.
                (
                    identity.clone(),
                    ContainerCorrelationEvidence::by_exact_id(
                        request.topology_node_id,
                        identity,
                        request.requested_at,
                    ),
                )
            }
            ContainerProjection::NativeChild => {
                let project_id = scope.ok_or(RuntimeError::WorkspaceMismatch {
                    rule: "a native_child requires the exact native parent binding",
                })?;
                let workspace = self
                    .fetch_workspaces(project_id.as_str())
                    .await?
                    .into_iter()
                    .find(|workspace| workspace.id == native_id.as_str())
                    .ok_or(RuntimeError::StaleBinding {
                        rule: "the bound container is not in the parent this node is placed under",
                    })?;
                let identity = self.identity(ExternalId::parse(&workspace.id)?, generation);
                (
                    identity.clone(),
                    ContainerCorrelationEvidence::by_exact_id(
                        request.topology_node_id,
                        identity,
                        request.requested_at,
                    ),
                )
            }
            ContainerProjection::LogicalOnly => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "a logical_only node has no native container to reconcile",
                });
            }
        };
        Ok(ContainerBindingSnapshot {
            binding: ContainerBinding {
                id: request.container_binding_id,
                topology_node_id: request.topology_node_id,
                projection,
                identity,
                root: request.cwd.clone(),
                bound_at: request.requested_at,
            },
            capabilities: declared.clone(),
            correlation,
        })
    }

    /// Bind a node to a native project for the first time.
    ///
    /// Adoption happens **only** by configured exact id. There is no
    /// adopt-by-display-name branch, because a project's name is a mutable
    /// string: matching on it would let this node take over a project somebody
    /// else named the same way, and the failure is silent in both directions.
    async fn bind_native_root(
        &self,
        request: &ContainerRequest,
        declared: &RuntimeCapabilities,
        generation: u64,
    ) -> RuntimeResult<(NativeRuntimeIdentity, ContainerCorrelationEvidence, bool)> {
        let _ = declared;
        if let Some(adopted) = self
            .config
            .adopted_containers
            .get(&request.topology_node_id)
            .cloned()
        {
            // Adopted by exact readback and nothing else. A configured id the
            // daemon does not hold is a configuration error to report, not a
            // project to create.
            let project = self.read_project_by_id(adopted.as_str()).await?;
            let identity = self.identity(ExternalId::parse(&project.id)?, generation);
            return Ok((
                identity.clone(),
                ContainerCorrelationEvidence::by_exact_id(
                    request.topology_node_id,
                    identity,
                    request.requested_at,
                ),
                false,
            ));
        }

        // Paseo identifies a registered project by directory. A root with no
        // directory therefore has no native identity to create, and falling
        // back to the host checkout would make every epic resolve to the first
        // project registered there.
        let cwd = request
            .cwd
            .as_ref()
            .ok_or(RuntimeError::WorkspaceMismatch {
                rule: "a native_root requires its own declared directory",
            })?;
        let command = PaseoRpc::project_add(self.next_request_id(), cwd.as_str());
        let frame = self.transport.request(&command).await?;
        let added: PaseoProjectAdded = frame.resolve(&command, "PaseoProjectAdded")?;
        let added = added.project.ok_or(RuntimeError::Transport {
            rule: "runtime refused to register this node's project",
        })?;
        // The answer to `add` is an acknowledgement. A binding is made from a
        // readback.
        let project = self.read_project_by_id(&added.id).await?;
        let identity = self.identity(ExternalId::parse(&project.id)?, generation);
        Ok((
            identity.clone(),
            ContainerCorrelationEvidence::by_exact_id(
                request.topology_node_id,
                identity,
                request.requested_at,
            ),
            true,
        ))
    }

    async fn remember_epic_project(
        &self,
        request: &ContainerRequest,
        native_id: &ExternalId,
    ) -> RuntimeResult<()> {
        if !request.epic_container {
            return Ok(());
        }
        let scope = self.effective_scope(&request.scope)?;
        let project = self.read_project_by_id(native_id.as_str()).await?;
        let epic_id = Self::external_epic_id(&scope)?;
        let _ = self.settle_project(
            epic_id,
            ExternalId::parse(&project.id)?,
            project.display_name,
            request.display_name.as_str().to_owned(),
        );
        Ok(())
    }

    /// Bind a node to a container below the exact bound parent.
    ///
    /// Before a durable native id exists, the human-readable title is the
    /// logical alias inside the exact bound project and canonical task path.
    /// After binding, reconciliation is always by the stored native id.
    async fn bind_native_child(
        &self,
        request: &ContainerRequest,
        project_id: &ExternalId,
        generation: u64,
    ) -> RuntimeResult<(NativeRuntimeIdentity, ContainerCorrelationEvidence, bool)> {
        if let Some(task_id) = request.task_id {
            let scope = self.effective_scope(&request.scope)?;
            if scope.require_task()?.task_id != task_id {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "the container task does not match its durable execution scope",
                });
            }
        }
        let title = request.display_name.as_str().to_owned();
        let cwd = request
            .cwd
            .as_ref()
            .ok_or(RuntimeError::WorkspaceMismatch {
                rule: "a native_child must say which directory it works in",
            })?;
        crate::checkout::prepare_managed_worktree(&self.config.scope.project_root_cwd, cwd).await?;
        let existing = self.fetch_workspaces(project_id.as_str()).await?;
        let same_path = existing
            .iter()
            .filter(|workspace| {
                workspace.project_id == project_id.as_str()
                    && WorkspaceRoot::parse(&workspace.workspace_directory)
                        .is_ok_and(|root| &root == cwd)
            })
            .collect::<Vec<_>>();
        let (workspace, created) = match same_path.as_slice() {
            // A prior effect of ours whose answer we lost. Adopting it is what
            // stops a lost acknowledgement from leaving two containers behind.
            [workspace] if workspace.visible_title() == title => ((*workspace).clone(), false),
            [_, _, ..] => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "several Paseo workspaces claim this canonical container path",
                });
            }
            [_] => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this canonical path already has a differently titled workspace; retitle it before admission",
                });
            }
            [] => {
                let command =
                    PaseoCommand::workspace_create(cwd.as_str(), project_id.as_str(), &title);
                let output = self.transport.run(&command).await?;
                let created: PaseoCliWorkspaceCreated = output.parse("PaseoCliWorkspaceCreated")?;
                // The CLI answer omits `projectId`, so the readback is what says
                // which project this actually landed in.
                let workspace = self
                    .fetch_workspaces(project_id.as_str())
                    .await?
                    .into_iter()
                    .find(|workspace| workspace.id == created.workspace_id)
                    .ok_or(RuntimeError::CorrelationFailed)?;
                (workspace, true)
            }
        };

        if workspace.project_id != project_id.as_str() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the container was not created inside the bound parent project",
            });
        }
        let identity = self.identity(ExternalId::parse(&workspace.id)?, generation);
        let correlation = ContainerCorrelationEvidence::by_exact_id(
            request.topology_node_id,
            identity.clone(),
            request.requested_at,
        );
        Ok((identity, correlation, created))
    }

    /// Read one project by exact native id.
    ///
    /// Display names are deliberately ignored for selection: the daemon owns
    /// their rendered value and may be repairing it in place.
    async fn read_project_by_id(&self, project_id: &str) -> RuntimeResult<PaseoProject> {
        self.fetch_projects()
            .await?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or(RuntimeError::Transport {
                rule: "the bound epic project is not among the projects this daemon holds",
            })
    }

    fn settle_project(
        &self,
        mini_project_id: ExternalId,
        project_id: ExternalId,
        observed_name: String,
        desired_name: String,
    ) -> PaseoProjectOutcome {
        let binding = PaseoProjectBinding {
            mini_project_id: mini_project_id.clone(),
            host_key: self.config.host_key.clone(),
            project_id,
            observed_name: observed_name.clone(),
        };
        self.lock()
            .projects
            .insert(mini_project_id, binding.clone());
        if observed_name == desired_name {
            PaseoProjectOutcome::Ready { binding }
        } else {
            PaseoProjectOutcome::ReadyWithRenamePending {
                binding,
                desired_name,
                observed_name,
            }
        }
    }

    /// Reconcile every role slot the snapshotted team template declares.
    ///
    /// Returns one plan per slot and mutates nothing. Materializing a slot is
    /// the caller's business precisely because it goes through the ordinary
    /// admitted launch path — an adapter that launched from here would be a
    /// second admission route, and AC-4 holds because there is only one.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the census cannot be taken, and
    /// [`RuntimeError::WorkspaceMismatch`] when the epic project is unprepared.
    pub async fn reconcile_role_slots(
        &self,
        team_run_id: TeamRunId,
        declared: &[RoleSlotId],
    ) -> RuntimeResult<Vec<PaseoSlotPlan>> {
        // Every agent of *this team run*, wherever it sits — not every agent of
        // the project. The seat rules are about labels, and an agent carrying
        // this run's slot label that has been moved out of the task workspace is
        // exactly the case that must block rather than vanish: a census scoped
        // to the project's own workspaces would filter it out and then
        // cheerfully materialize a second seat beside it.
        let agents = self
            .fetch_agents(&self.team_labels(team_run_id), false)
            .await?;
        let prepared = self
            .lock()
            .workspaces
            .get(&team_run_id)
            .map(|prepared| prepared.binding.identity.native_id.as_str().to_owned());
        let project = match prepared.as_deref() {
            Some(workspace_id) => {
                let workspace = self.fetch_workspace(workspace_id).await?;
                // A workspace id may survive being re-registered below a
                // different native project. That is a blocked placement, not a
                // transport failure and not permission to bind the foreign
                // project to whichever epic asked first.
                self.project_for_native_id(&workspace.project_id).ok()
            }
            None => Some(self.lock().projects.values().next().cloned().ok_or(
                RuntimeError::WorkspaceMismatch {
                    rule: "the epic project has not been prepared on this Paseo host",
                },
            )?),
        };
        // A workspace id survives a re-registration. The same `wks_…` comes back
        // as the project root, a plain local directory, or a worktree Paseo
        // provisioned for itself, and an agent sitting in it still answers this
        // census with the right workspace id and the right labels. So the
        // workspace is read fresh here rather than trusted from the binding that
        // prepared it: `Reuse` is a plan to drive the seat's next turn *there*,
        // and leaving that to the resume or send which acts on the plan is a
        // check that arrives after the caller has already committed to it.
        let task_workspace = match (&prepared, project.as_ref()) {
            (Some(workspace_id), Some(project)) => {
                let workspace = self.fetch_workspace(workspace_id).await?;
                let root = WorkspaceRoot::parse(&workspace.workspace_directory)?;
                self.verify_workspace_placement(&workspace, project, &root)
                    .is_ok()
                    .then(|| (workspace_id.clone(), root))
            }
            (Some(_), None) | (None, _) => None,
        };
        Ok(declared
            .iter()
            .map(|role_slot_id| {
                let slot = RoleSlotKey::new(team_run_id, role_slot_id.clone());
                // Nothing may be planned into a place that is not the task
                // worktree — not a reuse, and not a materialize either, since
                // materializing is how a seat would be created there.
                let Some((workspace_id, expected_root)) = &task_workspace else {
                    return PaseoSlotPlan::Blocked {
                        slot,
                        rule: "this team run has no proven task workspace to plan a seat in",
                    };
                };
                let wanted = self.slot_labels(team_run_id, role_slot_id);
                let live = agents
                    .iter()
                    .filter(|agent| agent.matches_labels(&wanted) && !agent.is_archived());
                // Labels alone do not say *where*. They travel with an agent, so
                // one that has been moved to another workspace or another tree
                // still answers this census wearing the right name — and reusing
                // it would drive the seat's next turn in the wrong repository.
                let (placed, misplaced): (Vec<_>, Vec<_>) =
                    live.partition(|agent| {
                        self.verify_agent_location(agent, workspace_id, expected_root)
                            .is_ok()
                    });
                match (placed.as_slice(), misplaced.is_empty()) {
                    // Two live agents for one seat is the state AC-4 forbids.
                    // Picking one would bind a run to a session that may belong
                    // to the other, and both would keep editing.
                    ([_, _, ..], _) => PaseoSlotPlan::Blocked {
                        slot,
                        rule: "two live Paseo agents carry this role slot's labels",
                    },
                    // Materializing alongside a misplaced twin would leave two
                    // live agents answering for one seat — the same forbidden
                    // state, reached by not looking.
                    (_, false) => PaseoSlotPlan::Blocked {
                        slot,
                        rule: "a live Paseo agent carries this role slot's labels outside the task workspace",
                    },
                    ([agent], true) => PaseoSlotPlan::Reuse {
                        slot,
                        agent_id: ExternalId::parse(&agent.id).unwrap_or_else(|_| {
                            ExternalId::parse("unparseable-native-id")
                                .expect("a fixed fallback id is valid")
                        }),
                        needs_reload: agent.status.needs_reload(),
                    },
                    ([], true) => PaseoSlotPlan::Materialize { slot },
                }
            })
            .collect())
    }

    /// Record an operator's authorization to adopt one foreign session.
    pub fn authorize_adoption(&self, intent: PaseoAdoptionIntent) {
        self.lock()
            .adoptions
            .insert(intent.native_agent_id.clone(), intent);
    }

    /// Retire one role session, with the archived state read back before
    /// anything may replace it.
    ///
    /// The predecessor's transcript is untouched: archiving ends a seat's tenure
    /// and does not delete its content, which is what makes the evidence still
    /// there when a successor cites it.
    ///
    /// # Errors
    /// * [`RuntimeError::StaleBinding`] — a binding this runtime never issued.
    /// * [`RuntimeError::Transport`] — Paseo refused, or acknowledged another id.
    /// * [`RuntimeError::CorrelationFailed`] — the fresh readback does not report
    ///   the agent archived, so nothing may cite it as finished.
    async fn retire_session(
        &self,
        binding: &RuntimeBindingSnapshot,
        at: Timestamp,
    ) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(binding)?;
        let native_id = binding.identity().native_id.as_str().to_owned();
        let command = PaseoCommand::agent_archive(&native_id);
        let output = self.transport.run(&command).await?;
        let ack: PaseoCliArchived = output.parse("PaseoCliArchived")?;
        if ack.agent_id.as_deref() != Some(native_id.as_str()) || ack.archived_at.is_none() {
            return Err(RuntimeError::CorrelationFailed);
        }
        // The acknowledgement is not the evidence. A fresh readback carrying an
        // archive stamp is, and until it does no successor may be admitted.
        let correlation = CorrelationLabel::for_run(binding.agent_run_id());
        let agent = self
            .fetch_agent_with_archive(&native_id, &correlation)
            .await?;
        if !agent.is_archived() {
            return Err(RuntimeError::CorrelationFailed);
        }
        let observation = self.observation(
            binding.agent_run_id(),
            binding.identity().clone(),
            &agent,
            at,
            ObservationSource::Inspect,
        )?;
        self.lock()
            .admissions
            .retire(binding.binding_id(), &binding.identity().native_id);
        Ok(observation)
    }

    /// Link a successor seat to the predecessor it replaced.
    ///
    /// # Errors
    /// Returns [`RuntimeError::StaleBinding`] when either binding is unknown to
    /// this runtime.
    pub fn link_predecessor(
        &self,
        successor: RuntimeBindingId,
        predecessor_agent_id: ExternalId,
    ) -> RuntimeResult<()> {
        let state = &mut *self.lock();
        let record = state
            .records
            .get_mut(&successor)
            .ok_or(RuntimeError::StaleBinding {
                rule: "this runtime holds no seat record for that binding",
            })?;
        record.previous_agent_id = Some(predecessor_agent_id);
        Ok(())
    }

    /// What Kontor can say about compacting one seat.
    ///
    /// Always a refusal to fabricate. `Pending` when a policy requires a
    /// confirmed compaction — which blocks reuse — and `Unsupported` when it
    /// does not, so an operator can see the difference between "we will not
    /// proceed" and "there is nothing to call".
    #[must_use]
    pub const fn compaction_status(&self, policy_requires_compaction: bool) -> PaseoCompaction {
        if policy_requires_compaction {
            PaseoCompaction::Pending
        } else {
            PaseoCompaction::Unsupported
        }
    }
}

// ---------------------------------------------------------------------------
// Session content helpers
// ---------------------------------------------------------------------------

impl PaseoAdapter {
    /// One canonical page for `agent_id`, strictly after `cursor`.
    ///
    /// A page that declares `reset`, `staleCursor` or `gap` is a break rather
    /// than a page: 0.3.1 puts those flags on the *response*, so this is the one
    /// place they have to be read, and reading its entries anyway would page
    /// over the hole the daemon just declared. Same for the daemon's own
    /// `error`: a page that failed is not an empty transcript.
    async fn fetch_canonical(
        &self,
        agent_id: &str,
        direction: PaseoDirection,
        cursor: Option<&PaseoTimelineCursor>,
        limit: u32,
        projection: PaseoProjection,
    ) -> RuntimeResult<PaseoTimelinePage> {
        let request = PaseoRpc::timeline_fetch(
            self.next_request_id(),
            agent_id,
            projection,
            direction,
            cursor,
            limit,
        );
        let frame = self.transport.request(&request).await?;
        let page: PaseoTimelinePage = frame.resolve(&request, "PaseoTimelinePage")?;
        if page.agent_id != agent_id {
            return Err(RuntimeError::CorrelationFailed);
        }
        if let Some(reason) = page.declared_break() {
            return Err(RuntimeError::TimelineRefetchRequired { reason });
        }
        if page.direction != direction {
            return Err(RuntimeError::CorrelationFailed);
        }
        if page.error.is_some() {
            return Err(RuntimeError::Transport {
                rule: "runtime refused the canonical timeline read",
            });
        }
        Ok(page)
    }

    /// The cursor a stored position reads from, in Paseo's own spelling.
    ///
    /// `None` when this adapter has never mapped that epoch, which is not a
    /// cursor it can express: asking `after` a raw epoch it cannot name would
    /// mean inventing one.
    fn wire_cursor(&self, position: TimelinePosition) -> Option<PaseoTimelineCursor> {
        self.lock()
            .epochs
            .raw(position.epoch)
            .map(|epoch| PaseoTimelineCursor {
                epoch,
                seq: position.sequence,
            })
    }

    /// The Kontor epoch a raw one resolves to, given what the cursor expects.
    ///
    /// With a cursor in hand the mapping must already exist *and* agree: a raw
    /// epoch that resolves to a different number than the cursor's is Paseo
    /// having renumbered, which is a break rather than a new allocation. This is
    /// the whole of MUT-013 — allocating a fresh epoch here would make every
    /// restored cursor silently point into a numbering that no longer exists.
    fn resolve_epoch(&self, raw: &str, expected: Option<u64>) -> RuntimeResult<u64> {
        let state = &mut *self.lock();
        match expected {
            None => Ok(state.epochs.resolve(raw)),
            Some(expected) => match state.epochs.known(raw) {
                Some(known) if known == expected => Ok(known),
                _ => Err(RuntimeError::TimelineRefetchRequired {
                    reason: TimelineBreak::EpochChanged,
                }),
            },
        }
    }

    /// Normalize a page's entries.
    ///
    /// No permission bookkeeping happens here, and that is 0.3.1's doing rather
    /// than an omission: its canonical timeline has no permission items at all.
    /// The lifecycle is read from [`PaseoAdapter::observe_permissions`] — a
    /// fresh agent readback — and from the unsolicited stream, which are the two
    /// places this wire records it.
    fn normalize_page(
        &self,
        page: &PaseoTimelinePage,
        epoch: u64,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        page.entries
            .iter()
            .map(|entry| normalize_entry(entry, epoch))
            .collect()
    }

    /// Reconcile the permission ledger against one fresh agent readback.
    ///
    /// `pendingPermissions` is a complete list, so it settles both halves at
    /// once: a request that is present is open, and one this adapter believed
    /// open that is *absent* has been answered — by the operator in Paseo's UI,
    /// by a provider timeout, or by Kontor itself. Absence is the only
    /// resolution evidence 0.3.1 offers, and it is enough for the rule that
    /// matters: never answer a request a second time.
    fn observe_permissions(&self, binding_id: RuntimeBindingId, agent: &PaseoAgent) {
        let live: BTreeSet<String> = agent
            .pending_permissions
            .iter()
            .map(|pending| pending.id.clone())
            .collect();
        for pending in &agent.pending_permissions {
            if let Ok(permission_id) = ExternalId::parse(&pending.id) {
                self.open_permission(binding_id, &permission_id);
            }
        }
        let vanished: Vec<ExternalId> = self
            .lock()
            .permission_owners
            .iter()
            .filter(|(permission_id, owner)| {
                **owner == binding_id && !live.contains(permission_id.as_str())
            })
            .map(|(permission_id, _)| permission_id.clone())
            .collect();
        for permission_id in &vanished {
            self.close_permission(permission_id);
        }
    }

    /// Note that `binding_id`'s session is waiting on `permission_id`.
    ///
    /// Re-reading content that has already been answered must not resurrect the
    /// request: history is replayed on every reconciliation, so an open that
    /// ignored the close would make the answered state last exactly until the
    /// next page fetch.
    fn open_permission(&self, binding_id: RuntimeBindingId, permission_id: &ExternalId) {
        let state = &mut *self.lock();
        if state.resolved_in_history.contains(permission_id) {
            return;
        }
        state.permissions.open(binding_id, permission_id.clone());
        state
            .permission_owners
            .entry(permission_id.clone())
            .or_insert(binding_id);
    }

    /// Note that Paseo shows `permission_id` no longer awaiting an answer.
    fn close_permission(&self, permission_id: &ExternalId) {
        let state = &mut *self.lock();
        state.resolved_in_history.insert(permission_id.clone());
        state.permission_owners.remove(permission_id);
    }

    /// Scan a bounded run of canonical history for one predicate.
    ///
    /// Used by both delivery reconciliations. It is the honest answer to "did
    /// the effect land?": the timeline is where Paseo records what happened, so
    /// a lost acknowledgement is settled by looking rather than by retrying.
    ///
    /// The scan is held to the same epoch continuity as an ordinary read. Once
    /// this session's content has been read even once, the epoch that content
    /// came under is what a reconciliation must find; a raw epoch that is
    /// unknown, or that resolves to a different number, means Paseo renumbered
    /// the session under us. Allocating a fresh epoch here would be the worst
    /// possible place to do it — the answer to "did my message land?" would then
    /// be read out of a numbering the question was never asked in, and a `no`
    /// from it authorizes a resend.
    ///
    /// "Read even once" includes this scan's own first page: a multi-page scan
    /// is one read, and its later pages have to continue the transcript its
    /// first page came from.
    async fn scan_canonical<F>(
        &self,
        binding: &RuntimeBindingSnapshot,
        mut matches: F,
    ) -> RuntimeResult<Option<(TimelinePosition, usize)>>
    where
        F: FnMut(&SessionEvent) -> bool,
    {
        let native_id = binding.identity().native_id.as_str().to_owned();
        // `None` only before this session has ever been read: there is no
        // continuity to keep with content that was never fetched.
        let mut expected = self
            .lock()
            .cursors
            .get(&binding.binding_id())
            .map(|position| position.epoch);
        let mut before: Option<PaseoTimelineCursor> = None;
        let mut found: Option<TimelinePosition> = None;
        let mut hits = 0usize;
        let mut complete = false;
        for _ in 0..RECONCILE_PAGE_BUDGET {
            let page = self
                .fetch_canonical(
                    &native_id,
                    if before.is_some() {
                        PaseoDirection::Before
                    } else {
                        PaseoDirection::Tail
                    },
                    before.as_ref(),
                    MAX_HISTORY_PAGE,
                    PaseoProjection::Canonical,
                )
                .await?;
            let epoch = self.resolve_epoch(&page.epoch, expected)?;
            // The first page this scan resolves is the continuity the rest of
            // the *same* scan is held to, before any later page is fetched.
            // Judging every page against only what was persisted before the
            // scan began lets a renumbering *between* two pages allocate a
            // second epoch instead of breaking, and page two of a different
            // transcript is then reconciled as though it continued page one —
            // which is exactly the `no` that authorizes a resend.
            expected = Some(epoch);
            for event in self.normalize_page(&page, epoch)? {
                if matches(&event) {
                    hits += 1;
                    found.get_or_insert(event.position);
                }
            }
            // A reconciliation starts at the newest window because that is the
            // only cursor-free read Paseo exposes, then walks *backward*. A busy
            // turn can emit more than one page before the send acknowledgement
            // is settled; walking forward from the newest page would never see
            // the accepted user message and could authorize a duplicate send.
            // Use Paseo's own start cursor rather than deriving an anchor from
            // an entry, so projection or paging changes cannot create overlap.
            match (page.has_older, page.start_cursor) {
                (true, Some(start)) => before = Some(start),
                (false, _) => {
                    complete = true;
                    break;
                }
                (true, None) => {
                    return Err(RuntimeError::TimelineRefetchRequired {
                        reason: TimelineBreak::SequenceGap,
                    });
                }
            }
        }
        if !complete {
            return Err(RuntimeError::DeliveryConfirmationUnknown {
                rule: "canonical history reconciliation reached its safety limit before proving whether the message landed; do not resend",
            });
        }
        // The whole scan came back under one epoch, so from here on there *is*
        // an epoch to be continuous with and every later reconciliation is held
        // to it. Without this the expectation above is only ever set by a
        // `history` call, so a plane that is driven rather than read keeps
        // allocating a fresh epoch for every raw one it meets and the check
        // above never fires. Only reached on success — a scan that broke
        // mid-way saw two numberings and has no single epoch to claim.
        //
        // Only the epoch is claimed. A scan pages to the end of the transcript
        // hunting one entry, which is not a read position any caller asked to
        // resume from, so an existing cursor is left exactly where `history`
        // put it.
        if let Some(epoch) = expected {
            self.lock()
                .cursors
                .entry(binding.binding_id())
                .or_insert_with(|| TimelinePosition::start_of(epoch));
        }
        Ok(found.map(|position| (position, hits)))
    }
}

/// Everything a retitle decides before it renames anything.
///
/// Held as one value so the preview and the apply cannot disagree: they share the
/// derivation that produced it, and the apply's only extra step is the call.
struct PaseoRetitlePlan {
    /// The binding this container is repaired under.
    snapshot: ContainerBindingSnapshot,
    /// The native project as the daemon reports it now, for an ESW root.
    before_project: Option<PaseoProject>,
    /// The workspace as the daemon reports it now, for an ECP/TSW child.
    before_workspace: Option<PaseoWorkspace>,
    /// The title it must end up carrying.
    desired: ExternalName,
    /// Whether that differs from what it carries now.
    changed: bool,
}

/// Fresh native facts behind one persistent-seat claim.
///
/// The plan owns complete agent snapshots so apply can preserve provider
/// sessions and verify every metadata mutation against the exact object it
/// addressed. It is never persisted; the daemon persists only the normalized
/// claim outcome and its preview hash.
struct PaseoHostedSeatClaimPlan {
    claimant: PaseoAgent,
    claimant_labels: BTreeMap<String, String>,
    predecessor: Option<PaseoAgent>,
    predecessor_reference: Option<HostedSeatClaimPredecessor>,
    title_conflicts: Vec<(PaseoAgent, HostedSeatTitleConflict)>,
    model_rung: ModelRung,
    already_claimed: bool,
}

impl PaseoAdapter {
    /// Locate the addressed container, derive its title, and decide nothing else.
    ///
    /// The lookup is by durable native id inside the *bound* project. Not by
    /// title, which is the value being corrected, and not by `cwd`, which several
    /// workspaces legitimately share — a rename that searched by either could
    /// rename the wrong tree, and there is usually no way to tell afterwards.
    ///
    /// The generation is a bound rather than an equality. A Paseo workspace
    /// outlives the Kontor process that bound it, and this repair exists
    /// precisely for containers bound before a restart, so an *older* generation
    /// in the request is the normal case. A *newer* one names a generation this
    /// adapter has never reached, which cannot describe anything it bound.
    ///
    /// # Errors
    /// * [`RuntimeError::UnsupportedCapability`] — the plane does not declare the
    ///   capability.
    /// * [`RuntimeError::StaleBinding`] — the request names a generation ahead of
    ///   this adapter's, the bound project no longer holds the native id, or this
    ///   adapter's own ledger has the node bound to a different container.
    /// * [`RuntimeError::WorkspaceMismatch`] — the plane holds no scope for the
    ///   node's task, so no title can be derived.
    /// * [`RuntimeError::CorrelationFailed`] — the workspace found does not carry
    ///   this node's label.
    async fn retitle_plan(
        &self,
        request: &RetitleContainerRequest,
    ) -> RuntimeResult<PaseoRetitlePlan> {
        let declared = self.declared().await?;
        if !declared.supports(RuntimeCapability::RetitleContainer) {
            return Err(self.refuse(RuntimeCapability::RetitleContainer, &declared));
        }
        let generation = self.generation();
        if request.generation > generation {
            return Err(RuntimeError::StaleBinding {
                rule: "the request names a runtime generation this plane has not reached",
            });
        }
        // An in-ledger binding for this node must be the one being addressed.
        // After a restart there is none — that is the contract, and it is why the
        // native id Kontor persisted is what finds the container again.
        if let Some(held) = self.lock().containers.get(&request.topology_node_id)
            && held.binding.identity.native_id != request.bound_native_id
        {
            return Err(RuntimeError::StaleBinding {
                rule: "this node is bound to a different native container on this plane",
            });
        }

        if request.projection == ContainerProjection::NativeRoot {
            let info = self.fetch_server_info().await?;
            if !info.supports_project_rename() {
                return Err(RuntimeError::UnsupportedCapability {
                    capability: RuntimeCapability::RetitleContainer,
                });
            }
            let before = self
                .fetch_projects()
                .await?
                .into_iter()
                .find(|project| project.id == request.bound_native_id.as_str())
                .ok_or(RuntimeError::StaleBinding {
                    rule: "the bound native project no longer exists on this runtime",
                })?;
            let identity = self.identity(ExternalId::parse(&before.id)?, generation);
            let correlation = ContainerCorrelationEvidence::by_exact_id(
                request.topology_node_id,
                identity.clone(),
                request.requested_at,
            );
            let desired = request.desired_title.clone();
            let changed = before.display_name != desired.as_str();
            return Ok(PaseoRetitlePlan {
                snapshot: ContainerBindingSnapshot {
                    binding: ContainerBinding {
                        id: request.container_binding_id,
                        topology_node_id: request.topology_node_id,
                        projection: ContainerProjection::NativeRoot,
                        identity,
                        root: WorkspaceRoot::parse(&before.root_path).ok(),
                        bound_at: request.requested_at,
                    },
                    capabilities: declared,
                    correlation,
                },
                before_project: Some(before),
                before_workspace: None,
                desired,
                changed,
            });
        }
        if request.projection != ContainerProjection::NativeChild {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "only a bound native project or workspace can be retitled",
            });
        }

        let project_id =
            request
                .bound_project_native_id
                .as_ref()
                .ok_or(RuntimeError::WorkspaceMismatch {
                    rule: "a native child retitle names no persisted native project",
                })?;
        let before = self
            .fetch_workspaces(project_id.as_str())
            .await?
            .into_iter()
            .find(|workspace| workspace.id == request.bound_native_id.as_str())
            .ok_or(RuntimeError::StaleBinding {
                rule: "the bound project no longer holds the addressed native container",
            })?;
        if before.project_id != project_id.as_str() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the addressed native container came back under another project",
            });
        }

        let identity = self.identity(ExternalId::parse(&before.id)?, generation);
        let correlation = ContainerCorrelationEvidence::by_exact_id(
            request.topology_node_id,
            identity.clone(),
            request.requested_at,
        );
        let desired = request.desired_title.clone();
        let changed = before.visible_title() != desired.as_str();
        Ok(PaseoRetitlePlan {
            snapshot: ContainerBindingSnapshot {
                binding: ContainerBinding {
                    id: request.container_binding_id,
                    topology_node_id: request.topology_node_id,
                    projection: ContainerProjection::NativeChild,
                    identity,
                    root: WorkspaceRoot::parse(&before.workspace_directory).ok(),
                    bound_at: request.requested_at,
                },
                capabilities: declared,
                correlation,
            },
            desired,
            changed,
            before_project: None,
            before_workspace: Some(before),
        })
    }

    /// Read the addressed container back after a rename.
    ///
    /// By the same native id inside the same bound project. The project it came
    /// back under is checked here; the directory is checked by the caller, which
    /// is the only place that holds what it was before.
    ///
    /// # Errors
    /// * [`RuntimeError::StaleBinding`] — the native id is gone, which a rename
    ///   must never cause.
    /// * [`RuntimeError::WorkspaceMismatch`] — the container came back under
    ///   another project.
    async fn retitle_readback(
        &self,
        request: &RetitleContainerRequest,
    ) -> RuntimeResult<PaseoWorkspace> {
        let project_id =
            request
                .bound_project_native_id
                .as_ref()
                .ok_or(RuntimeError::WorkspaceMismatch {
                    rule: "a native child retitle names no persisted native project",
                })?;
        let after = self
            .fetch_workspaces(project_id.as_str())
            .await?
            .into_iter()
            .find(|workspace| workspace.id == request.bound_native_id.as_str())
            .ok_or(RuntimeError::StaleBinding {
                rule: "the addressed native container is gone after the rename",
            })?;
        if after.project_id != project_id.as_str() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the renamed container came back under another project",
            });
        }
        Ok(after)
    }
}

// ---------------------------------------------------------------------------
// RuntimeAdapter
// ---------------------------------------------------------------------------

impl PaseoAdapter {
    /// The capability set to judge an operation by, read fresh.
    ///
    /// The gate is the identity the daemon *pushed* on the connection this
    /// adapter is about to use. 0.3.1 volunteers `status/server_info` right
    /// after the hello and answers no `server_info` request, so there is nothing
    /// to ask: the transport holds what it gated on, and a connection it could
    /// not gate is a transport error rather than a low grade.
    ///
    /// A daemon that does not advertise every feature the placement rules depend
    /// on is observed rather than driven — the shared preflight then refuses
    /// each undeclared operation before it can produce an effect. So is one off
    /// the pinned application version. Every DTO, argv and label spelling here
    /// was recorded against Paseo 0.3.1, and a feature list is not a version: a
    /// daemon can advertise all five required features and still have renamed a
    /// field this adapter reads a placement rule out of. Grade A says "believe
    /// these readbacks", and that claim is only underwritten for the version
    /// they were recorded from. An unrecognized build is observed instead — the
    /// same honest degradation as a daemon that cannot be reached at all.
    ///
    /// The protocol number is the other half of the pin and is not checked here,
    /// because it cannot fail this far in: the daemon closes the socket on a
    /// protocol mismatch, so the transport never produces a gated connection to
    /// grade.
    async fn declared(&self) -> RuntimeResult<RuntimeCapabilities> {
        let mut capabilities = match self.fetch_server_info().await {
            Ok(info) => {
                let degraded = !info.missing_required().is_empty() || !info.is_supported_baseline();
                self.lock().server = Some(info);
                if degraded {
                    self.config.degraded_capabilities()
                } else {
                    self.config.capabilities()
                }
            }
            Err(_) => self.config.degraded_capabilities(),
        };
        // Retitling is the one operation that does not travel over either surface
        // the rest of this adapter speaks, so it is declared from whether this
        // plane has a route to the daemon's MCP facade rather than from the
        // version gate. Declared at every grade on purpose: a rename sets a title
        // and reads it back, so there is no readback a degraded daemon could
        // mislead a *placement* decision with.
        let supports_project_rename = self
            .lock()
            .server
            .as_ref()
            .is_some_and(PaseoServerInfo::supports_project_rename);
        if self.mcp.is_some() || supports_project_rename {
            capabilities
                .supported
                .insert(RuntimeCapability::RetitleContainer);
        }
        Ok(capabilities)
    }

    /// Everything a launch does once its seat has agreed to it.
    ///
    /// Separate from [`RuntimeAdapter::launch`] so one place decides what a
    /// failure costs: every `?` here happens after the seat was claimed, and all
    /// of them are answered by the single release at the call site.
    async fn launch_admitted(
        &self,
        request: &LaunchRequest,
        declared: &RuntimeCapabilities,
        generation: u64,
    ) -> RuntimeResult<LaunchOutcome> {
        self.ensure_provider_available(request.model_rung().provider.0.as_str())?;
        let effective_scope = self.effective_scope(request.scope())?;
        let project = self.require_project_for(&effective_scope)?;
        // Whichever way the place was keyed, the presented binding must be the
        // one *this* adapter prepared, not merely a self-consistent value naming
        // the right thing. Preflight compares a snapshot against itself and is
        // satisfied by any coherent forgery; only the adapter's own ledger
        // distinguishes a binding it made from one that merely looks like it.
        let place = self.launch_placement(request)?;
        let workspace_id = place.workspace_id.clone();
        let task_scope = effective_scope.require_task()?;
        if task_scope.task_id != request.task_id() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the launch task does not match its durable execution scope",
            });
        }

        // Rerun the placement checks against a *fresh* readback. A binding made
        // ten minutes ago is evidence about ten minutes ago, and the whole point
        // of this gate is that nothing has moved since.
        let workspace = self.fetch_workspace_in(&project, &workspace_id).await?;
        self.verify_workspace_placement(&workspace, &project, &place.root)?;

        let labels = self.seat_labels(
            request.agent_run_id(),
            request.team_run_id(),
            request.role_slot_id(),
            &effective_scope,
            &project,
            &workspace_id,
        )?;
        let slot_labels = self.slot_labels(request.team_run_id(), request.role_slot_id());

        // A native census before the first effect. An exact full-label match is
        // a launch whose acknowledgement was lost; adopt it before the broader
        // occupied-slot guard so a retry never creates a second session.
        let census = self.fetch_agents(&slot_labels, false).await?;
        let exact = census
            .iter()
            .filter(|agent| agent.matches_labels(&labels) && !agent.is_archived())
            .collect::<Vec<_>>();
        let recovered_id = match exact.as_slice() {
            [agent] => Some(agent.id.clone()),
            [] => None,
            _ => return Err(RuntimeError::CorrelationFailed),
        };
        if recovered_id.is_none()
            && census
                .iter()
                .any(|agent| agent.matches_labels(&slot_labels) && !agent.is_archived())
        {
            return Err(RuntimeError::SlotAlreadyAdmitted {
                rule: "a live Paseo agent already carries this role slot's labels",
            });
        }

        // Capacity means processes executing now, not persistent seat
        // identities. Idle and closed agents are resumable records and must not
        // spend one slot forever. An exact-label recovery also adds no new
        // process, so it is checked at the observed count rather than count+1.
        let held = self
            .fetch_project_agents(&project, false)
            .await?
            .into_iter()
            .filter(|agent| !agent.is_archived() && agent.status.occupies_concurrent_capacity())
            .count();
        let demanded = u32::try_from(held)
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(recovered_id.is_none()));
        preflight(
            declared,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: request.account_profile_id().is_some(),
                binding: None,
                placement: Some(request.placement_claim()),
                current_generation: Some(generation),
                demand: Some(LimitDemand::ConcurrentSessions(demanded)),
                context_policy: Some(request.context_policy()),
            },
        )?;

        // Compose the seat's worktree-local MCP config before anything is
        // spawned, so a Claude seat starts with exactly one kontor server at
        // operator tier under the consultation profile instead of whatever
        // ambient harness config the machine carries. Loud on failure: a seat
        // silently launched without its control-plane surface would fail later,
        // further from the cause.
        crate::seat_mcp::compose_for_seat(
            self.config.seat_mcp.as_ref(),
            request.model_rung().provider.0.as_str(),
            std::path::Path::new(task_scope.worktree.as_str()),
        )
        .map_err(|error| {
            tracing::warn!(%error, "seat MCP composition failed");
            RuntimeError::LaunchNotAdmitted {
                rule: "seat MCP composition failed in the task worktree",
            }
        })?;

        let command = PaseoCommand::agent_run(
            &workspace_id,
            task_scope.worktree.as_str(),
            request.model_rung(),
            request.autonomy(),
            request.display_name().as_str(),
            &labels,
            None,
            request.prompt().as_str(),
        )?;
        let native_id = match recovered_id {
            Some(native_id) => native_id,
            None => match self.transport.run(&command).await {
                Ok(output) => {
                    let started: PaseoCliAgentStarted = output.parse("PaseoCliAgentStarted")?;
                    started.agent_id
                }
                // The command may have landed. An exact-label census before deciding
                // anything is the whole of the recovery rule; running `agent run`
                // again would be how one seat acquires two agents.
                Err(RuntimeError::Transport { .. }) => self.recover_launch(&labels).await?,
                Err(other) => return Err(other),
            },
        };

        // The CLI's answer is an id, a status, a provider and two display
        // strings — no project, no workspace, no labels, no parent. Every
        // placement rule is decided from the session readback, which is why
        // trusting the CLI here is a defect the fixtures name explicitly.
        let agent = self.fetch_agent(&native_id).await?;
        self.verify_agent_placement(&agent, &workspace_id, &labels)?;
        Self::verify_agent_route(&agent, request.model_rung(), request.autonomy())?;

        let snapshot = self.bind(
            request.agent_run_id(),
            request.binding_id(),
            &agent,
            request.requested_at(),
            generation,
            declared.clone(),
        )?;
        let observation = self.observation(
            request.agent_run_id(),
            snapshot.identity().clone(),
            &agent,
            request.requested_at(),
            // A launch is an acknowledgement. Whatever state Paseo reports in
            // it, a command acknowledgement can never close a run.
            ObservationSource::CommandAck,
        )?;
        let record = PaseoSeatRecord {
            mini_project_id: Self::external_epic_id(&effective_scope)?,
            jira_epic_key: effective_scope.epic.external_epic_key.clone(),
            plan_item_key: self.scoped_plan_item_key(&effective_scope)?,
            task_id: request.task_id(),
            team_run_id: request.team_run_id(),
            role_slot_id: request.role_slot_id().clone(),
            agent_run_id: request.agent_run_id(),
            binding_id: request.binding_id(),
            placement_binding_id: place.binding.clone(),
            canonical_worktree_cwd: task_scope.worktree.clone(),
            host_key: self.config.host_key.clone(),
            project_id: project.project_id.clone(),
            workspace_id: ExternalId::parse(&workspace_id)?,
            agent_id: ExternalId::parse(&agent.id)?,
            provider: agent.provider.clone(),
            model: agent.model.clone(),
            thinking: agent.effective_thinking_option_id.clone(),
            provider_session_id: agent
                .provider_session_id()
                .map(ExternalId::parse)
                .transpose()?,
            parent_agent_id: None,
            generation,
            previous_agent_id: None,
        };
        // The claim becomes the session in the same critical section that
        // records the binding, so there is no instant at which this adapter owns
        // a session and its seat is still reservable.
        {
            let state = &mut *self.lock();
            state
                .admissions
                .occupy(request, ExternalId::parse(&agent.id)?)?;
            state.bindings.record(snapshot.clone());
            state
                .placements
                .insert(request.binding_id(), record.workspace_id.clone());
            state.records.insert(request.binding_id(), record);
        }
        Ok(LaunchOutcome {
            snapshot,
            observation,
        })
    }

    /// Recover a launch whose acknowledgement was lost.
    ///
    /// The full label set is planted on the agent, so exactly one agent in this
    /// project can legitimately match. The three outcomes are all refusals to
    /// guess:
    ///
    /// * one match — bind it; the command did land;
    /// * several — the plane has diverged, and picking one would bind a run to a
    ///   session that may belong to another. No launch.
    /// * none — it is *not* known whether Paseo created an agent. The receipt
    ///   stays confirmation-unknown and reconciliation looks again. A blind
    ///   relaunch here is how one seat ends up with two agents editing one tree.
    async fn recover_launch(&self, labels: &BTreeMap<String, String>) -> RuntimeResult<String> {
        let agents = self.fetch_agents(labels, false).await?;
        let mut matches = agents
            .into_iter()
            .filter(|agent| agent.matches_labels(labels) && !agent.is_archived())
            .collect::<Vec<_>>();
        match matches.len() {
            1 => Ok(matches.remove(0).id),
            0 => Err(RuntimeError::Transport {
                rule: "acknowledgement was lost and no agent carries this launch's labels yet",
            }),
            _ => Err(RuntimeError::CorrelationFailed),
        }
    }

    fn consultation_labels(
        &self,
        request: &ConsultationLaunchRequest,
        project: &PaseoProjectBinding,
        workspace_id: &str,
    ) -> RuntimeResult<BTreeMap<String, String>> {
        let scope = self.effective_scope(&request.scope)?;
        let mut labels: BTreeMap<String, String> = [
            (
                label::CONSULTATION_RUN,
                format!(
                    "{}/{}",
                    request.run_id.family().as_str(),
                    request.run_id.as_text()
                ),
            ),
            (
                label::JIRA_EPIC,
                scope.epic.external_epic_key.as_str().to_owned(),
            ),
            (
                label::PROJECT_ID,
                project.mini_project_id.as_str().to_owned(),
            ),
            (label::SEAT_BINDING, request.seat_binding_id.to_string()),
            (label::ROLE, request.role_slot_id.as_str().to_owned()),
            (label::ROLE_SLOT, request.role_slot_id.as_str().to_owned()),
            (label::WORKSPACE_ID, workspace_id.to_owned()),
            (label::WORKTREE, request.cwd.as_str().to_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
        if request.model_rung.provider.0 == "opencode" {
            labels.insert(
                label::OPERATOR_ACCEPTED_FALLBACK.to_owned(),
                request.route_provenance.evidence_hash.as_str().to_owned(),
            );
        } else {
            labels.insert(label::READ_ONLY.to_owned(), "true".to_owned());
        }
        Ok(labels)
    }

    async fn launch_consultation_inner(
        &self,
        request: &ConsultationLaunchRequest,
    ) -> RuntimeResult<ConsultationLaunchOutcome> {
        self.validate_consultation_model_rung(&request.model_rung, &request.route_provenance)?;
        self.ensure_provider_available(request.model_rung.provider.0.as_str())?;
        let declared = self.declared().await?;
        let effective_scope = self.effective_scope(&request.scope)?;
        let project = self.require_project_for(&effective_scope)?;
        let generation = self.generation();
        let held = self
            .fetch_project_agents(&project, false)
            .await?
            .into_iter()
            .filter(|agent| !agent.is_archived() && agent.status.occupies_concurrent_capacity())
            .count();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: false,
                binding: None,
                // The consultation container is verified against the adapter's
                // own ledger immediately below; there is no fake TeamRun/Task
                // pair with which to build a delivery PlacementClaim.
                placement: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::ConcurrentSessions(
                    u32::try_from(held).unwrap_or(u32::MAX).saturating_add(1),
                )),
                context_policy: Some(&request.context_policy),
            },
        )?;

        request.container.ensure_correlated()?;
        request.container.ensure_generation(generation)?;
        request.container.ensure_root(&request.cwd)?;
        let prepared = self
            .lock()
            .containers
            .get(&request.container.topology_node_id())
            .cloned()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        if prepared != request.container {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "this is not the consultation container this runtime prepared",
            });
        }
        let workspace_id = prepared.binding.identity.native_id.as_str().to_owned();
        let workspace = self.fetch_workspace_in(&project, &workspace_id).await?;
        self.verify_workspace_placement(&workspace, &project, &request.cwd)?;

        let labels = self.consultation_labels(request, &project, &workspace_id)?;
        let slot_labels = [(
            label::SEAT_BINDING.to_owned(),
            request.seat_binding_id.to_string(),
        )]
        .into_iter()
        .collect();
        let census = self.fetch_agents(&slot_labels, false).await?;
        let exact = census
            .iter()
            .filter(|agent| agent.matches_labels(&labels) && !agent.is_archived())
            .collect::<Vec<_>>();
        let (native_id, created) = match exact.as_slice() {
            [agent] => (agent.id.clone(), false),
            [] if census.iter().any(|agent| !agent.is_archived()) => {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "a live Paseo agent already carries this consultation seat binding",
                });
            }
            [] => {
                // Claude starts project MCP servers from its cwd. Compose the
                // narrowed worker surface before the agent process exists so
                // its `kontor-mcp` child inherits this consultation seat's
                // already-scoped KONTOR_AUTH instead of the operator console's
                // ambient MCP authority. The config contains no credential.
                crate::seat_mcp::compose_for_seat(
                    self.config.seat_mcp.as_ref(),
                    request.model_rung.provider.0.as_str(),
                    std::path::Path::new(request.cwd.as_str()),
                )
                .map_err(|error| {
                    tracing::warn!(%error, "consultation seat MCP composition failed");
                    RuntimeError::LaunchNotAdmitted {
                        rule: "seat MCP composition failed in the consultation worktree",
                    }
                })?;
                let creation = PaseoRpc::consultation_agent_create(
                    self.next_request_id(),
                    &workspace_id,
                    request.cwd.as_str(),
                    &request.model_rung,
                    &request.route_provenance,
                    request.display_name.as_str(),
                    &labels,
                    request.prompt.as_str(),
                    request.credential.expose_secret(),
                )?;
                // A lost acknowledgement is confirmation-unknown. This call
                // never launches a second process in response; a later replay
                // begins with the exact-label census above and can adopt the
                // one agent the daemon created, if there is one.
                let frame = self.transport.request(&creation).await?;
                let status: serde_json::Value =
                    frame.resolve(&creation, "PaseoConsultationAgentCreated")?;
                if status.get("status").and_then(serde_json::Value::as_str) != Some("agent_created")
                {
                    return Err(RuntimeError::Transport {
                        rule: "runtime refused the consultation agent creation",
                    });
                }
                let native_id = status
                    .get("agent")
                    .and_then(|agent| agent.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        RuntimeError::Domain(kontor_core::DomainError::invalid(
                            "PaseoConsultationAgentCreated",
                            "does not name the created agent",
                        ))
                    })?;
                (native_id, true)
            }
            _ => return Err(RuntimeError::CorrelationFailed),
        };
        let agent = self.fetch_agent(&native_id).await?;
        self.verify_agent_placement(&agent, &workspace_id, &labels)?;
        Self::verify_agent_route_with_mode(
            &agent,
            &request.model_rung,
            Some(&request.route_provenance),
            SeatAutonomy::Advisory,
        )?;
        Ok(ConsultationLaunchOutcome {
            identity: self.identity(ExternalId::parse(&agent.id)?, generation),
            provider_session_id: agent
                .provider_session_id()
                .map(ExternalId::parse)
                .transpose()?,
            observed_at: request.requested_at,
            created,
        })
    }

    fn hosted_labels(
        &self,
        seat_binding_id: SeatBindingId,
        role_slot_id: &RoleSlotId,
        execution_scope: &ExecutionScope,
        project: &PaseoProjectBinding,
        workspace_id: &str,
        cwd: &WorkspaceRoot,
    ) -> RuntimeResult<BTreeMap<String, String>> {
        let scope = self.effective_scope(execution_scope)?;
        Ok([
            (
                label::JIRA_EPIC,
                scope.epic.external_epic_key.as_str().to_owned(),
            ),
            (
                label::PROJECT_ID,
                project.mini_project_id.as_str().to_owned(),
            ),
            (label::SEAT_BINDING, seat_binding_id.to_string()),
            (label::HOSTED_SEAT, "true".to_owned()),
            (label::ROLE, role_slot_id.as_str().to_owned()),
            (label::ROLE_SLOT, role_slot_id.as_str().to_owned()),
            (label::WORKSPACE_ID, workspace_id.to_owned()),
            (label::WORKTREE, cwd.as_str().to_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect())
    }

    fn hosted_seat_labels(
        &self,
        request: &HostedSeatLaunchRequest,
        project: &PaseoProjectBinding,
        workspace_id: &str,
    ) -> RuntimeResult<BTreeMap<String, String>> {
        self.hosted_labels(
            request.seat_binding_id,
            &request.role_slot_id,
            &request.scope,
            project,
            workspace_id,
            &request.cwd,
        )
    }

    fn released_seat_title(
        prefix: &str,
        role_slot_id: &RoleSlotId,
        native_id: &ExternalId,
    ) -> RuntimeResult<ExternalName> {
        ExternalName::parse(&format!(
            "{prefix} · {} · {}",
            role_slot_id.as_str(),
            native_id.as_str(),
        ))
        .map_err(RuntimeError::from)
    }

    fn claimant_has_foreign_ownership(
        claimant: &PaseoAgent,
        wanted: &BTreeMap<String, String>,
    ) -> bool {
        [
            label::AGENT_RUN,
            label::TEAM_RUN,
            label::CONSULTATION_RUN,
            label::READ_ONLY,
            label::OPERATOR_ACCEPTED_FALLBACK,
            label::PARENT_AGENT,
        ]
        .iter()
        .any(|key| claimant.label(key).is_some())
            || [
                label::JIRA_EPIC,
                label::PROJECT_ID,
                label::SEAT_BINDING,
                label::HOSTED_SEAT,
                label::ROLE,
                label::ROLE_SLOT,
                label::WORKSPACE_ID,
                label::WORKTREE,
            ]
            .iter()
            .any(|key| {
                claimant
                    .label(key)
                    .is_some_and(|value| wanted.get(*key).map(String::as_str) != Some(value))
            })
    }

    fn title_conflict_has_kontor_owner(agent: &PaseoAgent) -> bool {
        [
            label::AGENT_RUN,
            label::TEAM_RUN,
            label::CONSULTATION_RUN,
            label::SEAT_BINDING,
            label::HOSTED_SEAT,
            label::READ_ONLY,
            label::OPERATOR_ACCEPTED_FALLBACK,
        ]
        .iter()
        .any(|key| agent.label(key).is_some())
    }

    async fn update_agent_metadata_preserving_session(
        &self,
        before: &PaseoAgent,
        title: &ExternalName,
        labels: &BTreeMap<String, String>,
    ) -> RuntimeResult<PaseoAgent> {
        let output = self
            .transport
            .run(&PaseoCommand::agent_update(
                &before.id,
                Some(title.as_str()),
                labels,
            ))
            .await?;
        let updated: PaseoCliAgentUpdated = output.parse("PaseoCliAgentUpdated")?;
        if updated.agent_id != before.id {
            return Err(RuntimeError::CorrelationFailed);
        }
        let after = self.fetch_agent(&before.id).await?;
        if after.id != before.id
            || after.provider_session_id() != before.provider_session_id()
            || after.workspace_id != before.workspace_id
            || after.cwd != before.cwd
            || after.provider != before.provider
            || after.model != before.model
            || after.thinking_option_id != before.thinking_option_id
            || after.effective_thinking_option_id != before.effective_thinking_option_id
            || after.current_mode_id != before.current_mode_id
            || after.title.as_deref() != Some(title.as_str())
            || !after.matches_labels(labels)
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(after)
    }

    async fn hosted_seat_claim_plan(
        &self,
        request: &HostedSeatClaimRequest,
    ) -> RuntimeResult<PaseoHostedSeatClaimPlan> {
        let declared = self.declared().await?;
        preflight(&declared, &OperationContext::new(RuntimeCapability::Adopt))?;
        let scope = self.effective_scope(&request.scope)?;
        let project = self.require_project_for(&scope)?;
        let workspace_id = request.container_native_id.as_str();
        let workspace = self.fetch_workspace_in(&project, workspace_id).await?;
        self.verify_hosted_workspace_placement(&workspace, &project, &request.cwd)?;

        let claimant = self
            .fetch_agent(request.claimant_native_id.as_str())
            .await?;
        if claimant.is_archived()
            || !matches!(
                claimant.status,
                PaseoAgentStatus::Idle | PaseoAgentStatus::Running | PaseoAgentStatus::Error
            )
            || !claimant.pending_permissions.is_empty()
        {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "the hosted-seat claimant is not a live permission-clear session",
            });
        }
        self.verify_agent_location(&claimant, workspace_id, &request.cwd)?;
        self.ensure_provider_available(&claimant.provider)?;
        if claimant.provider.is_empty() || claimant.model.is_empty() {
            return Err(RuntimeError::CorrelationFailed);
        }
        if request
            .expected_claimant_provider_session_id
            .as_ref()
            .is_some_and(|expected| claimant.provider_session_id() != Some(expected.as_str()))
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        let model_rung = ModelRung {
            provider: ProviderRef(claimant.provider.clone()),
            model: ModelRef(claimant.model.clone()),
            effort: claimant
                .effective_thinking_option_id
                .as_deref()
                .map(|effort| match effort {
                    "off" => Ok(EffortLevel::Off),
                    "low" => Ok(EffortLevel::Low),
                    "medium" => Ok(EffortLevel::Medium),
                    "high" => Ok(EffortLevel::High),
                    "xhigh" => Ok(EffortLevel::Xhigh),
                    "max" => Ok(EffortLevel::Max),
                    "ultra" => Ok(EffortLevel::Ultra),
                    "ultracode" => Ok(EffortLevel::Ultracode),
                    _ => Err(RuntimeError::LaunchNotAdmitted {
                        rule: "the claimant exposes an unknown reasoning effort",
                    }),
                })
                .transpose()?,
        };
        let claimant_labels = self.hosted_labels(
            request.seat_binding_id,
            &request.role_slot_id,
            &scope,
            &project,
            workspace_id,
            &request.cwd,
        )?;
        if Self::claimant_has_foreign_ownership(&claimant, &claimant_labels) {
            return Err(RuntimeError::SlotAlreadyAdmitted {
                rule: "the claimant already belongs to another Kontor or native seat",
            });
        }
        let already_claimed = claimant.matches_labels(&claimant_labels)
            && claimant.title.as_deref() == Some(request.display_name.as_str());

        let seat_text = request.seat_binding_id.to_string();
        let released_seat_text = format!("former:{seat_text}");
        let mut predecessor_agent = None;
        if let Some(expected) = &request.expected_predecessor
            && expected.identity.native_id != request.claimant_native_id
        {
            if expected.identity.runtime_kind != self.config.runtime_kind
                || expected.identity.host != self.config.host_key
                || expected.identity.generation > self.generation()
            {
                return Err(RuntimeError::StaleBinding {
                    rule: "the hosted-seat predecessor belongs to another runtime",
                });
            }
            let predecessor = self
                .fetch_agent(expected.identity.native_id.as_str())
                .await?;
            if predecessor.provider_session_id()
                != expected
                    .provider_session_id
                    .as_ref()
                    .map(ExternalId::as_str)
            {
                return Err(RuntimeError::CorrelationFailed);
            }
            if predecessor.is_archived() {
                return Err(RuntimeError::StaleBinding {
                    rule: "the hosted-seat predecessor is archived rather than available for non-destructive takeover",
                });
            }
            self.verify_agent_location(&predecessor, workspace_id, &request.cwd)?;
            if matches!(
                predecessor.status,
                PaseoAgentStatus::Running
                    | PaseoAgentStatus::Initializing
                    | PaseoAgentStatus::Unknown
            ) || !predecessor.pending_permissions.is_empty()
            {
                return Err(RuntimeError::ReplacementNotEvidenced {
                    rule: "a hosted-seat takeover requires a quiescent predecessor with no pending permission",
                });
            }
            let canonical = predecessor.label(label::SEAT_BINDING) == Some(seat_text.as_str())
                && predecessor.label(label::HOSTED_SEAT) == Some("true");
            let released = predecessor.label(label::SEAT_BINDING)
                == Some(released_seat_text.as_str())
                && predecessor.label(label::FORMER_SEAT_BINDING) == Some(seat_text.as_str());
            if !canonical && !released {
                return Err(RuntimeError::CorrelationFailed);
            }
            predecessor_agent = Some(predecessor);
        }

        let predecessor_id = predecessor_agent.as_ref().map(|agent| agent.id.as_str());
        let mut title_conflicts = Vec::new();
        for agent in self.fetch_project_agents(&project, false).await? {
            if agent.id == claimant.id || predecessor_id == Some(agent.id.as_str()) {
                continue;
            }
            let title_collides = agent.title.as_deref() == Some(request.display_name.as_str());
            let cleanup_replay = agent.label(label::TITLE_RELEASED_FOR) == Some(seat_text.as_str());
            if !title_collides && !cleanup_replay {
                continue;
            }
            if title_collides && Self::title_conflict_has_kontor_owner(&agent) {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "another Kontor-owned session carries the canonical hosted-seat title",
                });
            }
            title_conflicts.push((
                agent.clone(),
                HostedSeatTitleConflict {
                    native_id: ExternalId::parse(&agent.id)?,
                    released_title: Self::released_seat_title(
                        "Unclaimed",
                        &request.role_slot_id,
                        &ExternalId::parse(&agent.id)?,
                    )?,
                },
            ));
        }
        title_conflicts
            .sort_by(|left, right| left.1.native_id.as_str().cmp(right.1.native_id.as_str()));
        Ok(PaseoHostedSeatClaimPlan {
            claimant,
            claimant_labels,
            predecessor: predecessor_agent,
            predecessor_reference: request.expected_predecessor.clone(),
            title_conflicts,
            model_rung,
            already_claimed,
        })
    }

    fn hosted_seat_claim_preview(
        &self,
        request: &HostedSeatClaimRequest,
        plan: &PaseoHostedSeatClaimPlan,
    ) -> RuntimeResult<HostedSeatClaimPreview> {
        Ok(HostedSeatClaimPreview {
            identity: self.identity(ExternalId::parse(&plan.claimant.id)?, self.generation()),
            provider_session_id: plan
                .claimant
                .provider_session_id()
                .map(ExternalId::parse)
                .transpose()?,
            model_rung: plan.model_rung.clone(),
            predecessor: plan.predecessor_reference.clone(),
            title_conflicts: plan
                .title_conflicts
                .iter()
                .map(|(_, conflict)| conflict.clone())
                .collect(),
            already_claimed: plan.already_claimed,
            observed_at: request.requested_at,
        })
    }

    async fn launch_hosted_seat_inner(
        &self,
        request: &HostedSeatLaunchRequest,
    ) -> RuntimeResult<ConsultationLaunchOutcome> {
        self.ensure_provider_available(request.model_rung.provider.0.as_str())?;
        let declared = self.declared().await?;
        let effective_scope = self.effective_scope(&request.scope)?;
        let project = self.require_project_for(&effective_scope)?;
        let generation = self.generation();

        request.container.ensure_correlated()?;
        request.container.ensure_generation(generation)?;
        request.container.ensure_root(&request.cwd)?;
        let prepared = self
            .lock()
            .containers
            .get(&request.container.topology_node_id())
            .cloned()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        if prepared != request.container {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "this is not the hosted seat container this runtime prepared",
            });
        }
        let workspace_id = prepared.binding.identity.native_id.as_str().to_owned();
        let workspace = self.fetch_workspace_in(&project, &workspace_id).await?;
        self.verify_hosted_workspace_placement(&workspace, &project, &request.cwd)?;

        let labels = self.hosted_seat_labels(request, &project, &workspace_id)?;
        let seat_labels = [(
            label::SEAT_BINDING.to_owned(),
            request.seat_binding_id.to_string(),
        )]
        .into_iter()
        .collect();
        let census = self.fetch_agents(&seat_labels, false).await?;
        let fenced_predecessors = request
            .fenced_predecessor_native_ids
            .iter()
            .map(ExternalId::as_str)
            .collect::<BTreeSet<_>>();
        let exact = census
            .iter()
            .filter(|agent| {
                agent.matches_labels(&labels)
                    && !agent.is_archived()
                    && !fenced_predecessors.contains(agent.id.as_str())
            })
            .collect::<Vec<_>>();
        let recovered_id = match exact.as_slice() {
            [agent] => Some(agent.id.clone()),
            [] if census.iter().any(|agent| {
                !agent.is_archived() && !fenced_predecessors.contains(agent.id.as_str())
            }) =>
            {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "a live Paseo agent already carries this hosted seat binding",
                });
            }
            [] => None,
            _ => return Err(RuntimeError::CorrelationFailed),
        };

        let active = self
            .fetch_project_agents(&project, false)
            .await?
            .into_iter()
            .filter(|agent| !agent.is_archived() && agent.status.occupies_concurrent_capacity())
            .count();
        let demanded = u32::try_from(active)
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(recovered_id.is_none()));
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::ConcurrentSessions(demanded)),
                context_policy: Some(&request.context_policy),
            },
        )?;

        let (native_id, created) = match recovered_id {
            Some(native_id) => (native_id, false),
            None => {
                crate::seat_mcp::compose_for_hosted_seat(
                    self.config.seat_mcp.as_ref(),
                    request.model_rung.provider.0.as_str(),
                    std::path::Path::new(request.cwd.as_str()),
                )
                .map_err(|error| {
                    tracing::warn!(%error, "hosted seat MCP composition failed");
                    RuntimeError::LaunchNotAdmitted {
                        rule: "hosted seat MCP composition failed in the ECP",
                    }
                })?;
                let creation = PaseoRpc::hosted_seat_agent_create(
                    self.next_request_id(),
                    &workspace_id,
                    request.cwd.as_str(),
                    &request.model_rung,
                    request.display_name.as_str(),
                    &labels,
                    request.prompt.as_str(),
                    request.credential.expose_secret(),
                )?;
                let frame = self.transport.request(&creation).await?;
                let status: serde_json::Value =
                    frame.resolve(&creation, "PaseoHostedSeatAgentCreated")?;
                if status.get("status").and_then(serde_json::Value::as_str) != Some("agent_created")
                {
                    return Err(RuntimeError::Transport {
                        rule: "runtime refused the hosted seat creation",
                    });
                }
                let native_id = status
                    .get("agent")
                    .and_then(|agent| agent.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        RuntimeError::Domain(kontor_core::DomainError::invalid(
                            "PaseoHostedSeatAgentCreated",
                            "does not name the created agent",
                        ))
                    })?;
                (native_id, true)
            }
        };
        let agent = self.fetch_agent(&native_id).await?;
        self.verify_agent_placement(&agent, &workspace_id, &labels)?;
        Self::verify_agent_route(&agent, &request.model_rung, SeatAutonomy::Supervised)?;
        Ok(ConsultationLaunchOutcome {
            identity: self.identity(ExternalId::parse(&agent.id)?, generation),
            provider_session_id: agent
                .provider_session_id()
                .map(ExternalId::parse)
                .transpose()?,
            observed_at: request.requested_at,
            created,
        })
    }
}

#[async_trait]
impl RuntimeAdapter for PaseoAdapter {
    fn validate_consultation_model_rung(
        &self,
        rung: &ModelRung,
        provenance: &ConsultationRouteProvenance,
    ) -> RuntimeResult<()> {
        super::client::consultation_route_permission_mode(rung, provenance).map(|_| ())
    }

    fn provider_available(&self, provider: &str) -> bool {
        !self.config.unavailable_providers.contains(provider)
    }

    fn fallback_model_rung(&self, requested: &ModelRung) -> Option<ModelRung> {
        self.config
            .provider_fallbacks
            .get(requested.provider.0.as_str())
            .filter(|fallback| self.provider_available(fallback.provider.0.as_str()))
            .cloned()
    }

    fn configured_execution_scope(
        &self,
        epic_id: kontor_core::id::MiniProjectId,
        task_id: Option<TaskId>,
    ) -> Option<ExecutionScope> {
        if self.configured_epic_id().ok()? != epic_id {
            return None;
        }
        let epic = EpicScope {
            mini_project_id: epic_id,
            external_epic_key: self.config.scope.jira_epic_key.clone(),
            short_title: self.config.scope.mini_project_short_title.clone(),
        };
        let Some(task_id) = task_id else {
            return Some(ExecutionScope::for_epic(epic));
        };
        let task = self.config.scope.configured_task_scope(task_id)?;
        Some(ExecutionScope::for_task(
            epic,
            TaskScope {
                task_id,
                external_issue_key: task.jira_issue_key,
                short_code: task.ticket_short_code,
                worktree: task.canonical_worktree_cwd,
            },
        ))
    }

    async fn discover_capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        self.declared().await
    }

    async fn issued_binding(
        &self,
        claimed: &RuntimeBindingSnapshot,
    ) -> RuntimeResult<IssuedBinding> {
        self.lock().bindings.attest(claimed)
    }

    /// Every Paseo operation is addressed inside the epic's project, so this is
    /// the step that has to happen before a census, a workspace or a seat.
    ///
    /// It delegates to [`PaseoAdapter::prepare_project`] unchanged — the
    /// correlated-prior-effect test, the exact-id readback and the persisted
    /// binding are all that method's, and none of them move here. What this adds
    /// is the one thing that was missing: a caller.
    ///
    /// The rename-pending outcome is deliberately *not* an error. A display
    /// string that drifted is not authority, the binding it carries is usable
    /// exactly as it is, and refusing here would shut scheduling over a label.
    /// Confirm each snapshot's session is still there in the same generation,
    /// then re-record it verbatim.
    ///
    /// Verbatim is the whole property. The trust grade, the limits, the
    /// correlation and the native identity all come out of the persisted
    /// snapshot; the live census is consulted only to answer *does this session
    /// still exist here?*. Rebuilding capabilities from a fresh
    /// `discover_capabilities` would re-grade a binding whose session never
    /// changed, which is precisely what the freeze rule exists to stop.
    async fn restore_bindings(
        &self,
        snapshots: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<Vec<RuntimeBindingSnapshot>> {
        if snapshots.is_empty() {
            return Ok(Vec::new());
        }
        // Read every claimed session by exact native id. Project-scoped census
        // used to make restart recovery depend on the one epic stored in fleet
        // settings; exact reads let each surviving seat re-establish its own
        // epic project from the immutable PROJECT_ID label and workspace owner.
        let generation = self.generation();
        let mut live = Vec::new();
        for snapshot in snapshots {
            let Ok(agent) = self
                .fetch_agent(snapshot.identity().native_id.as_str())
                .await
            else {
                continue;
            };
            // A live seat must still prove its project and workspace before we
            // restore anything that can drive it. An explicitly archived seat
            // is different: its exact native identity is useful only for a
            // terminal readback, and an already-retired task workspace may no
            // longer be present in Paseo's active workspace census. Refusing
            // that readback strands the open Kontor run after restart even
            // though Paseo still attests the archive stamp by exact agent id.
            if self.recover_project_for_agent(&agent).await.is_err() && !agent.is_archived() {
                continue;
            }
            let (state, _) = Self::normalize_agent(&agent);
            live.push(NativeSession {
                identity: self.identity(ExternalId::parse(&agent.id)?, generation),
                correlation: agent
                    .label(label::AGENT_RUN)
                    .and_then(|value| CorrelationLabel::parse(value).ok()),
                state,
                observed_at: Timestamp::now(),
            });
        }
        let declared = self.declared().await?;
        // Placement is re-proved from the live agents, not remembered: an agent
        // reports which workspace it sits in, and that workspace is checked
        // against this plane exactly as a driving operation checks it. The seat
        // record a launch wrote carries Kontor-side facts this adapter cannot
        // re-derive — the task, the team run, the role slot — so it is *not*
        // fabricated here. Only the placement is recovered, which is the part
        // every driving operation actually reads and the part the runtime can
        // still answer for.
        let placements = self.reprove_placements(snapshots, &live).await?;
        let mut restored = Vec::new();
        let mut state = self.lock();
        for snapshot in snapshots {
            match attest_restored(snapshot, &live, &declared) {
                Ok(()) => {
                    // A live binding whose placement could not be re-proved is
                    // not restored at all. Recording it without placement would
                    // leave a seat that reads and cannot be driven. An archived
                    // binding is deliberately terminal-only: exact inspection
                    // can settle its run, while every driving operation already
                    // refuses the archive stamp.
                    let workspace_id = placements.get(&snapshot.binding_id());
                    let archived = live.iter().any(|session| {
                        &session.identity == snapshot.identity()
                            && session.state == ObservedRunState::Cancelled
                    });
                    if workspace_id.is_none() && !archived {
                        tracing::warn!(
                            binding = %snapshot.binding_id(),
                            agent_run = %snapshot.agent_run_id(),
                            "refused to restore a binding whose placement could not be re-proved"
                        );
                        continue;
                    }
                    state.bindings.record(snapshot.clone());
                    if let Some(workspace_id) = workspace_id {
                        state
                            .placements
                            .insert(snapshot.binding_id(), workspace_id.clone());
                    }
                    restored.push(snapshot.clone());
                }
                // Refused, and said so. A claim this runtime cannot attest is
                // never silently dropped: it is named here and reported as an
                // `Unattested` finding by the census that follows, because a
                // binding nothing ever reviews is how a forged one survives.
                Err(error) => {
                    tracing::warn!(
                        binding = %snapshot.binding_id(),
                        agent_run = %snapshot.agent_run_id(),
                        detail = %error,
                        "refused to restore a binding this runtime cannot attest"
                    );
                }
            }
        }
        Ok(restored)
    }

    async fn prepare_plane(&self) -> RuntimeResult<()> {
        // Project creation belongs to `prepare_container`, whose request carries
        // the daemon-rendered title from the pinned topology specification.
        // Startup may verify that the runtime answers, but it has no naming
        // authority and therefore emits no project mutation.
        self.discover_capabilities().await?;
        Ok(())
    }

    async fn launch_consultation(
        &self,
        request: &ConsultationLaunchRequest,
    ) -> RuntimeResult<ConsultationLaunchOutcome> {
        {
            let state = &mut *self.lock();
            if !state.consultation_claims.insert(request.seat_binding_id) {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "this consultation seat already has a launch in flight",
                });
            }
        }
        let outcome = self.launch_consultation_inner(request).await;
        self.lock()
            .consultation_claims
            .remove(&request.seat_binding_id);
        outcome
    }

    async fn retire_consultation_seat(
        &self,
        request: &ConsultationSeatRetireRequest,
    ) -> RuntimeResult<ConsultationSeatRetireOutcome> {
        let declared = self.declared().await?;
        preflight(&declared, &OperationContext::new(RuntimeCapability::Retire))?;
        if request.identity.runtime_kind != self.config.runtime_kind
            || request.identity.host != self.config.host_key
            || request.identity.generation > self.generation()
        {
            return Err(RuntimeError::StaleBinding {
                rule: "the consultation predecessor belongs to another runtime",
            });
        }
        let native_id = request.identity.native_id.as_str();
        let before = self.fetch_agent(native_id).await?;
        let expected_seat = request.seat_binding_id.to_string();
        let fallback_hash = request.route_provenance.evidence_hash.as_str();
        let restriction_matches = if request.model_rung.provider.0 == "opencode" {
            before.label(label::READ_ONLY).is_none()
                && before.label(label::OPERATOR_ACCEPTED_FALLBACK) == Some(fallback_hash)
        } else {
            before.label(label::READ_ONLY) == Some("true")
                && before.label(label::OPERATOR_ACCEPTED_FALLBACK).is_none()
        };
        if before.label(label::SEAT_BINDING) != Some(expected_seat.as_str())
            || before.label(label::CONSULTATION_RUN).is_none()
            || !restriction_matches
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        if before.is_archived() {
            return Ok(ConsultationSeatRetireOutcome {
                identity: request.identity.clone(),
                archived_at: request.requested_at,
            });
        }
        Self::verify_agent_route_with_mode(
            &before,
            &request.model_rung,
            Some(&request.route_provenance),
            SeatAutonomy::Advisory,
        )?;
        if before.status != PaseoAgentStatus::Idle || !before.pending_permissions.is_empty() {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "consultation recovery requires an idle predecessor with no pending permission",
            });
        }
        let output = self
            .transport
            .run(&PaseoCommand::agent_archive(native_id))
            .await?;
        let archived: PaseoCliArchived = output.parse("PaseoCliArchived")?;
        if archived.agent_id.as_deref() != Some(native_id) || archived.archived_at.is_none() {
            return Err(RuntimeError::CorrelationFailed);
        }
        let after = self.fetch_agent(native_id).await?;
        if after.id != before.id
            || after.label(label::SEAT_BINDING) != Some(expected_seat.as_str())
            || !after.is_archived()
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(ConsultationSeatRetireOutcome {
            identity: request.identity.clone(),
            archived_at: request.requested_at,
        })
    }

    async fn launch_hosted_seat(
        &self,
        request: &HostedSeatLaunchRequest,
    ) -> RuntimeResult<ConsultationLaunchOutcome> {
        {
            let state = &mut *self.lock();
            if !state.hosted_seat_claims.insert(request.seat_binding_id) {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "this hosted topology seat already has a launch in flight",
                });
            }
        }
        let outcome = self.launch_hosted_seat_inner(request).await;
        self.lock()
            .hosted_seat_claims
            .remove(&request.seat_binding_id);
        outcome
    }

    async fn preview_hosted_seat_claim(
        &self,
        request: &HostedSeatClaimRequest,
    ) -> RuntimeResult<HostedSeatClaimPreview> {
        let plan = self.hosted_seat_claim_plan(request).await?;
        self.hosted_seat_claim_preview(request, &plan)
    }

    async fn claim_hosted_seat(
        &self,
        request: &HostedSeatClaimRequest,
    ) -> RuntimeResult<HostedSeatClaimOutcome> {
        {
            let state = &mut *self.lock();
            if !state.hosted_seat_claims.insert(request.seat_binding_id) {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "this hosted topology seat already has a claim in flight",
                });
            }
        }
        let outcome = async {
            let plan = self.hosted_seat_claim_plan(request).await?;
            let before = self.hosted_seat_claim_preview(request, &plan)?;
            let seat_text = request.seat_binding_id.to_string();
            let mut changed = false;

            if let Some(predecessor) = &plan.predecessor {
                let released_title = Self::released_seat_title(
                    "Former",
                    &request.role_slot_id,
                    &ExternalId::parse(&predecessor.id)?,
                )?;
                let released_seat = format!("former:{seat_text}");
                let released_role = format!("former:{}", request.role_slot_id.as_str());
                let labels = [
                    (label::SEAT_BINDING.to_owned(), released_seat),
                    (label::HOSTED_SEAT.to_owned(), "false".to_owned()),
                    (label::FORMER_SEAT_BINDING.to_owned(), seat_text.clone()),
                    (label::TITLE_RELEASED_FOR.to_owned(), seat_text.clone()),
                    (label::ROLE.to_owned(), "former".to_owned()),
                    (label::ROLE_SLOT.to_owned(), released_role),
                ]
                .into_iter()
                .collect::<BTreeMap<_, _>>();
                if predecessor.title.as_deref() != Some(released_title.as_str())
                    || !predecessor.matches_labels(&labels)
                {
                    self.update_agent_metadata_preserving_session(
                        predecessor,
                        &released_title,
                        &labels,
                    )
                    .await?;
                    changed = true;
                }
            }

            for (conflicting, conflict) in &plan.title_conflicts {
                let labels = [(label::TITLE_RELEASED_FOR.to_owned(), seat_text.clone())]
                    .into_iter()
                    .collect::<BTreeMap<_, _>>();
                if conflicting.title.as_deref() != Some(conflict.released_title.as_str())
                    || !conflicting.matches_labels(&labels)
                {
                    self.update_agent_metadata_preserving_session(
                        conflicting,
                        &conflict.released_title,
                        &labels,
                    )
                    .await?;
                    changed = true;
                }
            }

            if !plan.already_claimed {
                self.update_agent_metadata_preserving_session(
                    &plan.claimant,
                    &request.display_name,
                    &plan.claimant_labels,
                )
                .await?;
                changed = true;
            }

            let mut readback_request = request.clone();
            readback_request.expected_claimant_provider_session_id =
                before.provider_session_id.clone();
            let after_plan = self.hosted_seat_claim_plan(&readback_request).await?;
            let after = self.hosted_seat_claim_preview(&readback_request, &after_plan)?;
            if !after.already_claimed
                || after.identity != before.identity
                || after.provider_session_id != before.provider_session_id
                || after.model_rung != before.model_rung
                || after.predecessor != before.predecessor
                || after.title_conflicts != before.title_conflicts
            {
                return Err(RuntimeError::CorrelationFailed);
            }
            Ok(HostedSeatClaimOutcome {
                claim: after,
                changed,
            })
        }
        .await;
        self.lock()
            .hosted_seat_claims
            .remove(&request.seat_binding_id);
        outcome
    }

    async fn inspect_hosted_seat(
        &self,
        request: &HostedSeatInspectRequest,
    ) -> RuntimeResult<HostedSeatInspection> {
        let declared = self.declared().await?;
        preflight(
            &declared,
            &OperationContext::new(RuntimeCapability::Inspect),
        )?;
        let native = self.hosted_seat_agent(request).await?;
        Ok(HostedSeatInspection {
            identity: request.identity.clone(),
            state: match native {
                Some(agent) if agent.is_archived() => HostedSeatNativeState::Archived,
                Some(_) => HostedSeatNativeState::Live,
                None => HostedSeatNativeState::Missing,
            },
            observed_at: request.requested_at,
        })
    }

    async fn retire_hosted_seat(
        &self,
        request: &HostedSeatRetireRequest,
    ) -> RuntimeResult<HostedSeatRetireOutcome> {
        let declared = self.declared().await?;
        preflight(&declared, &OperationContext::new(RuntimeCapability::Retire))?;
        let native_id = request.identity.native_id.as_str();
        let inspect = HostedSeatInspectRequest {
            seat_binding_id: request.seat_binding_id,
            identity: request.identity.clone(),
            model_rung: request.model_rung.clone(),
            requested_at: request.requested_at,
        };
        let Some(before) = self.hosted_seat_agent(&inspect).await? else {
            return Ok(HostedSeatRetireOutcome {
                identity: request.identity.clone(),
                archived_at: request.requested_at,
            });
        };
        // An archive stamp is the terminal readback this operation exists to
        // prove. Older hosted seats may carry a permission mode that no longer
        // matches the current supervised policy; requiring a live route match
        // before accepting their already-archived state wedges the logical
        // seat permanently. Exact native identity and seat labels above still
        // correlate the predecessor without treating it as driveable.
        if before.is_archived() {
            return Ok(HostedSeatRetireOutcome {
                identity: request.identity.clone(),
                archived_at: request.requested_at,
            });
        }
        if before.status != PaseoAgentStatus::Idle || !before.pending_permissions.is_empty() {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "Core Team route correction requires an idle predecessor with no pending permission",
            });
        }
        let output = self
            .transport
            .run(&PaseoCommand::agent_archive(native_id))
            .await?;
        let archived: PaseoCliArchived = output.parse("PaseoCliArchived")?;
        if archived.agent_id.as_deref() != Some(native_id) || archived.archived_at.is_none() {
            return Err(RuntimeError::CorrelationFailed);
        }
        let expected_seat = request.seat_binding_id.to_string();
        let after = self.fetch_agent_including_archived(native_id).await?;
        if after.id != before.id
            || after.label(label::SEAT_BINDING) != Some(expected_seat.as_str())
            || !after.is_archived()
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(HostedSeatRetireOutcome {
            identity: request.identity.clone(),
            archived_at: request.requested_at,
        })
    }

    async fn message_hosted_seat(
        &self,
        request: &HostedSeatMessageRequest,
    ) -> RuntimeResult<HostedSeatMessageOutcome> {
        if request.identity.runtime_kind != self.config.runtime_kind
            || request.identity.host != self.config.host_key
            || request.identity.generation != self.generation()
        {
            return Err(RuntimeError::StaleBinding {
                rule: "the hosted topology seat belongs to another runtime generation",
            });
        }
        let native_id = request.identity.native_id.as_str();
        let agent = self.fetch_agent(native_id).await?;
        let expected_seat = request.seat_binding_id.to_string();
        if agent.label(label::SEAT_BINDING) != Some(expected_seat.as_str())
            || agent.label(label::HOSTED_SEAT) != Some("true")
            || agent.is_archived()
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        // Hosted seats are deliberately absent from the ordinary issued-binding
        // registry, so their delivery evidence cannot live in that registry's
        // in-memory message ledger. Always settle the stable clientMessageId
        // against this exact native's canonical history before touching the
        // wire. This is inexpensive for the rare leadership wake and is what
        // makes a retry safe after a fresh daemon adapter starts empty.
        if let Some((position, accepted_at)) = self
            .reconcile_hosted_message(native_id, request.message_id)
            .await?
        {
            return Ok(HostedSeatMessageOutcome {
                message_id: request.message_id,
                accepted_at,
                position,
            });
        }
        self.ensure_provider_available(&agent.provider)?;
        let declared = self.declared().await?;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::SendMessage,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: Some(self.generation()),
                demand: Some(LimitDemand::MessageBytes(
                    u64::try_from(request.body.as_str().len()).unwrap_or(u64::MAX),
                )),
                context_policy: None,
            },
        )?;
        let rpc = PaseoRpc::send_message(
            self.next_request_id(),
            native_id,
            &request.message_id.to_string(),
            request.body.as_str(),
        );
        let sent = self.transport.request(&rpc).await;
        if let Ok(frame) = sent {
            let accepted: PaseoSendAccepted = frame.resolve(&rpc, "PaseoSendAccepted")?;
            if accepted.agent_id != native_id || !accepted.accepted {
                return Err(RuntimeError::CorrelationFailed);
            }
        }
        match self
            .reconcile_hosted_message(native_id, request.message_id)
            .await?
        {
            Some((position, accepted_at)) => Ok(HostedSeatMessageOutcome {
                message_id: request.message_id,
                accepted_at,
                position,
            }),
            None => Err(RuntimeError::DeliveryConfirmationUnknown {
                rule: "the hosted-seat message is not yet present in canonical history",
            }),
        }
    }

    async fn message_consultation(
        &self,
        request: &ConsultationMessageRequest,
    ) -> RuntimeResult<()> {
        if request.identity.runtime_kind != self.config.runtime_kind
            || request.identity.host != self.config.host_key
            || request.identity.generation != self.generation()
        {
            return Err(RuntimeError::StaleBinding {
                rule: "the consultation seat belongs to another runtime generation",
            });
        }
        let native_id = request.identity.native_id.as_str();
        let agent = self.fetch_agent(native_id).await?;
        let expected_run = format!(
            "{}/{}",
            request.run_id.family().as_str(),
            request.run_id.as_text()
        );
        let expected_seat = request.seat_binding_id.to_string();
        if agent.label(label::SEAT_BINDING) != Some(expected_seat.as_str())
            || agent.label(label::CONSULTATION_RUN) != Some(expected_run.as_str())
            || agent.is_archived()
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        let rpc = PaseoRpc::send_message(
            self.next_request_id(),
            native_id,
            &request.message_id.to_string(),
            request.body.as_str(),
        );
        let frame = self.transport.request(&rpc).await?;
        let accepted: PaseoSendAccepted = frame.resolve(&rpc, "PaseoSendAccepted")?;
        if accepted.agent_id != native_id || !accepted.accepted {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(())
    }

    /// Admission is bookkeeping about seats: it starts nothing and reaches no
    /// Paseo surface, so "the daemon was never called" keeps meaning what it
    /// says.
    async fn admit_launch(&self, request: &AdmissionRequest) -> RuntimeResult<AdmissionOutcome> {
        let state = &mut *self.lock();
        let facts = PaseoSeatFacts {
            bindings: &state.bindings,
            generation: state.generation,
        };
        state.admissions.admit(request, &facts)
    }

    /// Rename the bound workspace through the daemon's MCP facade, then read the
    /// title back.
    ///
    /// Neither surface the rest of this adapter speaks can do it. The CLI verbs
    /// are `workspace create` and `workspace archive`; the session envelopes are
    /// `project.list`, `project.add`, `fetch_workspaces`, `fetch_agents`,
    /// `fetch_agent`, `fetch_agent_timeline` and `send_agent_message`. None
    /// changes a title, and `agent update-labels` addresses an agent rather than
    /// the workspace holding it. The facade's `rename_workspace` does, addressed
    /// by workspace id, and it is the only supported way — archiving and
    /// recreating would destroy the native id every binding, seat and readback
    /// resolves by, and writing the daemon's own state is an undocumented
    /// surface with no contract and no readback.
    ///
    /// The container is found by its durable native id inside the bound project,
    /// never by title — the value being corrected — and never by `cwd`, which
    /// several workspaces share. The rename carries the id and the new title and
    /// nothing else, so Paseo has nothing to move: parent, directory and every
    /// running terminal stay where they are, and the readback proves it.
    ///
    /// # Errors
    /// * [`RuntimeError::UnsupportedCapability`] — this plane has no route to the
    ///   facade, so it cannot rename anything and says so before trying.
    /// * [`RuntimeError::StaleBinding`] — the addressed container is not in the
    ///   named generation, or the bound project no longer holds it.
    /// * [`RuntimeError::WorkspaceMismatch`] — the readback shows a different
    ///   project, a different directory, or a title the rename did not achieve.
    /// * [`RuntimeError::Transport`] — the facade could not be reached, or
    ///   answered something that is not one result.
    async fn retitle_container(
        &self,
        request: &RetitleContainerRequest,
    ) -> RuntimeResult<RetitleContainerOutcome> {
        let plan = self.retitle_plan(request).await?;
        if let Some(before) = &plan.before_project {
            if plan.changed {
                let rpc = PaseoRpc::project_rename(
                    self.next_request_id(),
                    &before.id,
                    plan.desired.as_str(),
                );
                let frame = self.transport.request(&rpc).await?;
                let renamed: PaseoProjectRenamed = frame.resolve(&rpc, "PaseoProjectRenamed")?;
                if !renamed.accepted
                    || renamed.project_id != before.id
                    || renamed.custom_name.as_deref() != Some(plan.desired.as_str())
                {
                    return Err(RuntimeError::Transport {
                        rule: "Paseo refused or mis-correlated the native project rename",
                    });
                }
            }
            let after = self
                .fetch_projects()
                .await?
                .into_iter()
                .find(|project| project.id == before.id)
                .ok_or(RuntimeError::StaleBinding {
                    rule: "the addressed native project is gone after the rename",
                })?;
            if after.root_path != before.root_path {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "the renamed native project came back rooted in another directory",
                });
            }
            if after.display_name != plan.desired.as_str() {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "the native project did not come back carrying the title it was renamed to",
                });
            }
            return Ok(RetitleContainerOutcome {
                observed_title: after.display_name,
                snapshot: plan.snapshot,
                desired_title: plan.desired,
                changed: plan.changed,
            });
        }

        let before = plan
            .before_workspace
            .as_ref()
            .expect("every non-project retitle plan carries a workspace");
        let mcp = self
            .mcp
            .as_ref()
            .ok_or(RuntimeError::UnsupportedCapability {
                capability: RuntimeCapability::RetitleContainer,
            })?;
        if plan.changed {
            mcp.call(
                RENAME_WORKSPACE_TOOL,
                serde_json::json!({
                    "workspaceId": before.id,
                    "title": plan.desired.as_str(),
                }),
            )
            .await?;
        }

        // Read back rather than echoed. A rename the daemon accepted and did not
        // perform is exactly the failure this operation exists to correct, so the
        // answer is what the facade reports afterwards — including that the
        // container is still in the same project, still in the same directory,
        // and still the same id.
        let after = self.retitle_readback(request).await?;
        if after.workspace_directory != before.workspace_directory {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the renamed container came back working in another directory",
            });
        }
        if after.visible_title() != plan.desired.as_str() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the workspace did not come back carrying the title it was renamed to",
            });
        }
        Ok(RetitleContainerOutcome {
            observed_title: after.visible_title().to_owned(),
            snapshot: plan.snapshot,
            desired_title: plan.desired,
            changed: plan.changed,
        })
    }

    /// What [`Self::retitle_container`] would do, reaching nothing that writes.
    ///
    /// The same route, the same derivation and the same refusals, minus the one
    /// call that changes anything. A preview against a plane with no facade route
    /// refuses too: promising an apply that cannot happen is worse than saying so.
    ///
    /// # Errors
    /// As [`Self::retitle_container`], except that no rename is attempted, so no
    /// post-rename mismatch can be reported.
    async fn preview_retitle_container(
        &self,
        request: &RetitleContainerRequest,
    ) -> RuntimeResult<RetitleContainerOutcome> {
        let plan = self.retitle_plan(request).await?;
        let observed_title = if let Some(before) = &plan.before_project {
            before.display_name.clone()
        } else {
            if self.mcp.is_none() {
                return Err(RuntimeError::UnsupportedCapability {
                    capability: RuntimeCapability::RetitleContainer,
                });
            }
            plan.before_workspace
                .as_ref()
                .expect("every non-project retitle plan carries a workspace")
                .visible_title()
                .to_owned()
        };
        Ok(RetitleContainerOutcome {
            observed_title,
            snapshot: plan.snapshot,
            desired_title: plan.desired,
            changed: plan.changed,
        })
    }

    async fn prepare_container(
        &self,
        request: &ContainerRequest,
    ) -> RuntimeResult<ContainerOutcome> {
        // Capabilities decide the shape, and they decide it before anything is
        // dispatched. Nothing below this line reads the node's kind key.
        let projection = request.validate()?;
        let operation = match projection {
            ContainerProjection::NativeRoot => RuntimeCapability::PrepareProject,
            ContainerProjection::NativeChild => RuntimeCapability::PrepareWorkspace,
            ContainerProjection::LogicalOnly => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "a logical_only node has no native container to prepare",
                });
            }
        };
        let declared = self.declared().await?;
        if !declared.supports(operation) {
            return Err(self.refuse(operation, &declared));
        }
        let generation = self.generation();

        // An in-ledger binding is answered from state, so a retry after a lost
        // answer never reaches the wire at all.
        if let Some(existing) = self
            .lock()
            .containers
            .get(&request.topology_node_id)
            .cloned()
            && existing.binding.identity.generation == generation
        {
            return Ok(ContainerOutcome {
                snapshot: existing,
                created: false,
            });
        }

        // Everything below reconciles or binds against the *exact* native id,
        // and never against a display name or a path. A name is validation
        // evidence: it can disagree and still be the right container, and it
        // can agree while naming somebody else's.
        let scope = match projection {
            ContainerProjection::LogicalOnly => None,
            ContainerProjection::NativeRoot => None,
            ContainerProjection::NativeChild => {
                // The exact bound parent, or nothing. There is deliberately no
                // branch here that reaches for "the project this plane is
                // configured with": a child whose root is missing must stop,
                // because creating one beside the epic is how a task's work
                // ends up in a project nobody is watching.
                let parent = request
                    .parent
                    .as_ref()
                    .ok_or(RuntimeError::WorkspaceMismatch {
                        rule: "a native_child requires the exact native parent binding",
                    })?;
                Some(parent.identity.native_id.clone())
            }
        };

        // A binding Kontor already holds is reconciled by its stored id, which
        // is the whole of the restart path: the adapter's ledger is gone, the
        // container is not.
        let stored = request.bound_native_id.clone().or_else(|| {
            self.lock()
                .containers
                .get(&request.topology_node_id)
                .map(|it| it.binding.identity.native_id.clone())
        });
        if let Some(native_id) = stored {
            let snapshot = self
                .reconcile_container_by_id(
                    request,
                    projection,
                    &native_id,
                    scope.as_ref(),
                    &declared,
                    generation,
                )
                .await?;
            self.remember_epic_project(request, &snapshot.binding.identity.native_id)
                .await?;
            self.lock()
                .containers
                .insert(request.topology_node_id, snapshot.clone());
            return Ok(ContainerOutcome {
                snapshot,
                created: false,
            });
        }

        let (identity, correlation, created) = match projection {
            ContainerProjection::LogicalOnly => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "a logical_only node has no native container to prepare",
                });
            }
            ContainerProjection::NativeRoot => {
                self.bind_native_root(request, &declared, generation)
                    .await?
            }
            ContainerProjection::NativeChild => {
                let project_id = scope.ok_or(RuntimeError::WorkspaceMismatch {
                    rule: "a native_child requires the exact native parent binding",
                })?;
                self.bind_native_child(request, &project_id, generation)
                    .await?
            }
        };

        let snapshot = ContainerBindingSnapshot {
            binding: ContainerBinding {
                id: request.container_binding_id,
                topology_node_id: request.topology_node_id,
                projection,
                identity,
                root: request.cwd.clone(),
                bound_at: request.requested_at,
            },
            capabilities: declared,
            correlation,
        };
        self.remember_epic_project(request, &snapshot.binding.identity.native_id)
            .await?;
        self.lock()
            .containers
            .insert(request.topology_node_id, snapshot.clone());
        Ok(ContainerOutcome { snapshot, created })
    }

    async fn prepare_workspace(
        &self,
        request: &WorkspacePrepareRequest,
    ) -> RuntimeResult<WorkspaceOutcome> {
        let declared = self.declared().await?;
        if !declared.supports(RuntimeCapability::PrepareWorkspace) {
            return Err(self.refuse(RuntimeCapability::PrepareWorkspace, &declared));
        }
        let generation = self.generation();

        // Idempotent per team run, and answered from state before anything is
        // dispatched: a retry after a lost answer cannot leave a second
        // workspace behind if it never reaches the wire.
        if let Some(existing) = self.lock().workspaces.get(&request.team_run_id).cloned() {
            if existing.root() != &request.root {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this team run was already prepared at another root",
                });
            }
            return Ok(WorkspaceOutcome {
                snapshot: existing,
                created: false,
            });
        }

        // The requested root is compared against the canonical worktree this
        // plane serves. `WorkspaceRoot` already refuses `.`, `..` and repeated
        // separators, so two spellings of one place cannot compare unequal here.
        let effective_scope = self.effective_scope(&request.scope)?;
        let task_scope = effective_scope.require_task()?;
        if task_scope.task_id != request.task_id {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the workspace task does not match its durable execution scope",
            });
        }
        if request.root != task_scope.worktree {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the requested root is not the task's durable canonical worktree",
            });
        }
        crate::checkout::prepare_managed_worktree(
            &self.config.scope.project_root_cwd,
            &request.root,
        )
        .await?;
        let project = self.require_project_for(&effective_scope)?;

        let workspace_title = request.display_name.as_str();
        let existing = self.fetch_workspaces(project.project_id.as_str()).await?;
        let same_path = existing
            .iter()
            .filter(|workspace| {
                workspace.project_id == project.project_id.as_str()
                    && WorkspaceRoot::parse(&workspace.workspace_directory)
                        .is_ok_and(|root| root == request.root)
            })
            .collect::<Vec<_>>();
        let (workspace, created) = match same_path.as_slice() {
            [workspace] if workspace.visible_title() == workspace_title => {
                ((*workspace).clone(), false)
            }
            // Two workspaces at one canonical path inside one project is a
            // hierarchy that has diverged, even if one happens to carry the
            // desired title. Picking it would leave the second identity hidden
            // and let later readbacks split the team run between them.
            [_, _, ..] => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "several Paseo workspaces claim this canonical task worktree",
                });
            }
            [_] => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this canonical task worktree already has a differently titled workspace; retitle it before admission",
                });
            }
            [] => {
                let command = PaseoCommand::workspace_create(
                    request.root.as_str(),
                    project.project_id.as_str(),
                    workspace_title,
                );
                let output = self.transport.run(&command).await?;
                let created: PaseoCliWorkspaceCreated = output.parse("PaseoCliWorkspaceCreated")?;
                // The CLI answer omits `projectId`, so it cannot be believed.
                // The readback is what says which project this landed in.
                (
                    self.fetch_workspace_in(&project, &created.workspace_id)
                        .await?,
                    true,
                )
            }
        };

        self.verify_workspace_placement(&workspace, &project, &task_scope.worktree)?;

        let identity = self.identity(ExternalId::parse(&workspace.id)?, generation);
        let correlation = WorkspaceCorrelationEvidence::by_exact_id(
            request.team_run_id,
            identity.clone(),
            request.requested_at,
        );
        let snapshot = WorkspaceBindingSnapshot {
            binding: WorkspaceBinding {
                id: request.workspace_binding_id,
                team_run_id: request.team_run_id,
                task_id: request.task_id,
                root: request.root.clone(),
                identity,
                bound_at: request.requested_at,
            },
            capabilities: declared,
            correlation,
        };
        self.lock()
            .workspaces
            .insert(request.team_run_id, snapshot.clone());
        Ok(WorkspaceOutcome { snapshot, created })
    }

    async fn launch(&self, request: &LaunchRequest) -> RuntimeResult<LaunchOutcome> {
        let declared = self.declared().await?;

        // The seat is taken here: before the readbacks, long before `agent run`,
        // and in one step with the check that it was there to take. Splitting
        // the check from the take is the defect this arrangement exists to
        // prevent — everything below runs with the lock released, because it has
        // to, so a launch that had only *read* its reservation would leave the
        // seat reservable for the length of a native call and two callers would
        // each start an agent.
        let generation = {
            let state = &mut *self.lock();
            state.admissions.claim(request)?;
            if state
                .bindings
                .snapshots()
                .any(|snapshot| snapshot.agent_run_id() == request.agent_run_id())
            {
                state.admissions.release(request);
                return Err(RuntimeError::SessionAlreadyBound {
                    rule: "recovery launches a successor run, never the same run twice",
                });
            }
            state.generation
        };

        let outcome = self.launch_admitted(request, &declared, generation).await;
        if outcome.is_err() {
            self.lock().admissions.release(request);
        }
        outcome
    }

    async fn resume(&self, request: &ResumeRequest) -> RuntimeResult<ControlPlaneObservation> {
        // Provenance first: preflight would otherwise judge this against the
        // caller's own capability snapshot.
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Resume,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;

        let native_id = binding.identity().native_id.as_str().to_owned();
        let fresh = self.fetch_agent(&native_id).await?;
        self.verify_seat_placement(&binding, &fresh).await?;
        self.ensure_provider_available(&fresh.provider)?;
        self.observe_permissions(binding.binding_id(), &fresh);

        if fresh.is_archived() {
            return Err(RuntimeError::StaleBinding {
                rule: "this session has been retired and cannot be resumed",
            });
        }

        if fresh.status.needs_reload() {
            let command = PaseoCommand::agent_reload(&native_id);
            let output = self.transport.run(&command).await?;
            let reloaded: PaseoCliAgentReloaded = output.parse("PaseoCliAgentReloaded")?;
            if reloaded.agent_id != native_id {
                return Err(RuntimeError::CorrelationFailed);
            }
            // The same agent, in the same place, under the same parent, with the
            // same provider session. A reload that came back as anything else is
            // a different session wearing the id.
            let after = self.fetch_agent(&native_id).await?;
            if after.workspace_id != fresh.workspace_id
                || after.parent_agent_id() != fresh.parent_agent_id()
                || (fresh.provider_session_id().is_some()
                    && after.provider_session_id() != fresh.provider_session_id())
            {
                return Err(RuntimeError::CorrelationFailed);
            }
            return self.observation(
                binding.agent_run_id(),
                binding.identity().clone(),
                &after,
                request.requested_at,
                ObservationSource::CommandAck,
            );
        }

        // Already a live seat — running, or idle between turns. Nothing is
        // reloaded and nothing is replaced: the next turn is a message to this
        // same agent id, which is the whole of same-seat continuity. Reloading
        // to "get a fresh turn" would discard live work and inflate nothing.
        self.observation(
            binding.agent_run_id(),
            binding.identity().clone(),
            &fresh,
            request.requested_at,
            ObservationSource::Inspect,
        )
    }

    async fn send(&self, request: &SendMessageRequest) -> RuntimeResult<MessageAck> {
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::SendMessage,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::MessageBytes(request.body_bytes())),
                context_policy: None,
            },
        )?;

        let body_hash = request.body_hash();
        // The ledger is consulted before the wire, so a retry answers from
        // recorded evidence and never becomes a second delivery. It survives a
        // restart because it is in the checkpoint.
        let retrying_unknown = match self
            .lock()
            .messages
            .admit(&request.message_id, &body_hash)?
        {
            Admission::Replay(PaseoDelivery::Acknowledged(original)) => return Ok(original),
            Admission::Replay(PaseoDelivery::ConfirmationUnknown) => true,
            Admission::New => false,
        };

        // Only a *retry of an unconfirmed delivery* asks the timeline first, and
        // it must: resending an id whose effect may already exist is the one
        // move that duplicates an instruction in someone's repository. Paseo
        // records the caller's own `messageId` on the resulting user message, so
        // "did it land?" is a question with an answer, and retrying only after
        // that answer is `no` is what makes the retry safe. A first attempt
        // skips this — there is nothing yet to have landed, and scanning the
        // transcript to discover that would be a round trip that proves nothing.
        if retrying_unknown
            && let Some(acknowledgement) = self.reconcile_message(&binding, request).await?
        {
            return Ok(acknowledgement);
        }

        let native_id = binding.identity().native_id.as_str().to_owned();
        // Last thing before the wire, and deliberately after both short-circuits
        // above: a replay answering from recorded evidence produces no effect, so
        // it has no tree to be wrong about. What follows does.
        let fresh = self.fetch_agent(&native_id).await?;
        self.verify_seat_placement(&binding, &fresh).await?;
        self.ensure_provider_available(&fresh.provider)?;

        let rpc = PaseoRpc::send_message(
            self.next_request_id(),
            &native_id,
            &request.message_id.to_string(),
            request.body.as_str(),
        );
        let sent = self.transport.request(&rpc).await;
        match sent {
            Ok(frame) => {
                let accepted: PaseoSendAccepted = frame.resolve(&rpc, "PaseoSendAccepted")?;
                // Paseo must echo this exact agent and say it took the message.
                // 0.3.1's acknowledgement does not carry the caller's message id
                // back — that echo lands on the resulting user message as
                // `clientMessageId` — so the id half is settled below, by the
                // canonical read, and never by this frame.
                if accepted.agent_id != native_id || !accepted.accepted {
                    self.record_delivery(
                        request.message_id,
                        body_hash,
                        PaseoDelivery::ConfirmationUnknown,
                    );
                    return Err(RuntimeError::Transport {
                        rule: "runtime acknowledged something other than this message",
                    });
                }
            }
            Err(_) => {
                // The channel died after Paseo may have accepted it. Record the
                // uncertainty, then settle it by looking rather than by sending
                // again.
                self.record_delivery(
                    request.message_id,
                    body_hash.clone(),
                    PaseoDelivery::ConfirmationUnknown,
                );
                return match self.reconcile_message(&binding, request).await? {
                    Some(acknowledgement) => Ok(acknowledgement),
                    None => Err(RuntimeError::DeliveryConfirmationUnknown {
                        rule: "the runtime channel failed after delivery began and canonical history does not yet confirm the message",
                    }),
                };
            }
        }

        // The position is the one the *timeline* gives it, never one this
        // adapter counted. An adapter-local counter would be a claim about where
        // the message sits in a transcript Paseo owns.
        match self.reconcile_message(&binding, request).await? {
            Some(acknowledgement) => Ok(acknowledgement),
            None => {
                self.record_delivery(
                    request.message_id,
                    body_hash,
                    PaseoDelivery::ConfirmationUnknown,
                );
                Err(RuntimeError::DeliveryConfirmationUnknown {
                    rule: "runtime accepted the message but its content has not appeared yet",
                })
            }
        }
    }

    async fn cancel(&self, request: &CancelRequest) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Cancel,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let native_id = binding.identity().native_id.as_str().to_owned();
        let command = PaseoCommand::agent_stop(&native_id);
        let output = self.transport.run(&command).await?;
        let ack: PaseoCliStopped = output.parse("PaseoCliStopped")?;
        // 0.3.1's stop is a bulk operation: it reports every agent it
        // interrupted. Exactly this one, and no other, is the only shape that
        // acknowledges *this* cancel — an empty list is a no-op on an idle agent
        // and a longer one means the command reached somebody else's session
        // too.
        if ack.agent_ids != [native_id.clone()] {
            return Err(RuntimeError::CorrelationFailed);
        }
        // Paseo acknowledged that it accepted the request. It has not said the
        // session stopped, and a stopped agent is a *reloadable* seat rather
        // than a finished run — so this observation carries `CommandAck`, which
        // no trust grade may close a run on.
        let agent = self.fetch_agent(&native_id).await?;
        self.observation(
            binding.agent_run_id(),
            binding.identity().clone(),
            &agent,
            request.requested_at,
            ObservationSource::CommandAck,
        )
    }

    async fn retire(
        &self,
        binding: &RuntimeBindingSnapshot,
        at: Timestamp,
    ) -> RuntimeResult<ControlPlaneObservation> {
        self.retire_session(binding, at).await
    }

    async fn retire_unavailable_provider(
        &self,
        binding: &RuntimeBindingSnapshot,
        expected_provider: &str,
        at: Timestamp,
    ) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(binding)?;
        let native_id = binding.identity().native_id.as_str();
        let agent = self.fetch_agent(native_id).await?;
        let correlation = CorrelationLabel::for_run(binding.agent_run_id()).to_string();
        if agent.label(label::AGENT_RUN) != Some(correlation.as_str()) {
            return Err(RuntimeError::CorrelationFailed);
        }
        if agent.provider != expected_provider {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "the native session reports a different provider",
            });
        }
        if self.provider_available(expected_provider) {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "the native session's provider is currently available",
            });
        }
        if agent.status != PaseoAgentStatus::Idle
            || agent.is_archived()
            || !agent.pending_permissions.is_empty()
        {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "provider-unavailable retirement requires an idle unarchived session with no pending permission",
            });
        }
        self.retire_session(&binding, at).await
    }

    async fn reconcile_session_labels(
        &self,
        request: &ReconcileSessionLabelsRequest,
    ) -> RuntimeResult<ReconciledSessionLabels> {
        let binding = self.attested(&request.binding)?;
        let scope = self.effective_scope(&request.scope)?;
        let task = scope.require_task()?;
        let project = self.require_project_for(&scope)?;
        request.container.ensure_correlated()?;
        let prepared = self
            .lock()
            .containers
            .get(&request.container.topology_node_id())
            .cloned()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        if prepared != request.container {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "this is not the task container this runtime prepared",
            });
        }
        let workspace_id = prepared.binding.identity.native_id.as_str().to_owned();
        let workspace = self.fetch_workspace_in(&project, &workspace_id).await?;
        self.verify_workspace_placement(&workspace, &project, &task.worktree)?;

        let native_id = binding.identity().native_id.as_str().to_owned();
        let before = self.fetch_agent(&native_id).await?;
        self.verify_agent_location(&before, &workspace_id, &task.worktree)?;
        let correlation = CorrelationLabel::for_run(binding.agent_run_id()).to_string();
        if before.label(label::AGENT_RUN) != Some(correlation.as_str()) {
            return Err(RuntimeError::CorrelationFailed);
        }
        let labels = self.seat_labels(
            binding.agent_run_id(),
            request.team_run_id,
            &request.role_slot_id,
            &scope,
            &project,
            &workspace_id,
        )?;
        let title = request.desired_title.as_str().to_owned();
        let changed =
            !before.matches_labels(&labels) || before.title.as_deref() != Some(title.as_str());
        if changed {
            let output = self
                .transport
                .run(&PaseoCommand::agent_update(
                    &native_id,
                    Some(&title),
                    &labels,
                ))
                .await?;
            let updated: PaseoCliAgentUpdated = output.parse("PaseoCliAgentUpdated")?;
            if updated.agent_id != native_id {
                return Err(RuntimeError::CorrelationFailed);
            }
        }
        let after = self.fetch_agent(&native_id).await?;
        self.verify_agent_location(&after, &workspace_id, &task.worktree)?;
        if after.id != before.id
            || after.provider_session_id() != before.provider_session_id()
            || !after.matches_labels(&labels)
            || after.title.as_deref() != Some(title.as_str())
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(ReconciledSessionLabels {
            identity: binding.identity().clone(),
            title,
            labels: after.labels,
            changed,
            observed_at: request.requested_at,
        })
    }

    async fn preview_retitle_seat(
        &self,
        request: &RetitleSeatRequest,
    ) -> RuntimeResult<RetitleSeatOutcome> {
        if request.identity.runtime_kind != self.config.runtime_kind
            || request.identity.host != self.config.host_key
            || request.identity.generation > self.generation()
        {
            return Err(RuntimeError::StaleBinding {
                rule: "the persistent seat belongs to a runtime generation this plane has not reached",
            });
        }
        let before = self
            .fetch_agent(request.identity.native_id.as_str())
            .await?;
        let provider_session_id = before
            .provider_session_id()
            .map(ExternalId::parse)
            .transpose()?;
        if before.id != request.identity.native_id.as_str()
            || request
                .provider_session_id
                .as_ref()
                .is_some_and(|expected| provider_session_id.as_ref() != Some(expected))
            || before.workspace_id.as_deref() != Some(request.container_native_id.as_str())
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(RetitleSeatOutcome {
            identity: request.identity.clone(),
            provider_session_id,
            container_native_id: request.container_native_id.clone(),
            observed_title: before.title.clone().unwrap_or_default(),
            changed: before.title.as_deref() != Some(request.desired_title.as_str()),
        })
    }

    async fn retitle_seat(
        &self,
        request: &RetitleSeatRequest,
    ) -> RuntimeResult<RetitleSeatOutcome> {
        let preview = self.preview_retitle_seat(request).await?;
        if preview.changed {
            let output = self
                .transport
                .run(&PaseoCommand::agent_update(
                    request.identity.native_id.as_str(),
                    Some(request.desired_title.as_str()),
                    &BTreeMap::new(),
                ))
                .await?;
            let updated: PaseoCliAgentUpdated = output.parse("PaseoCliAgentUpdated")?;
            if updated.agent_id != request.identity.native_id.as_str() {
                return Err(RuntimeError::CorrelationFailed);
            }
        }
        let after = self.preview_retitle_seat(request).await?;
        if after.identity != request.identity
            || after.provider_session_id != request.provider_session_id
            || after.container_native_id != request.container_native_id
            || after.observed_title != request.desired_title.as_str()
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(RetitleSeatOutcome {
            changed: preview.changed,
            ..after
        })
    }

    async fn inspect(&self, request: &InspectRequest) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Inspect,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let native_id = binding.identity().native_id.as_str().to_owned();
        // Always a fresh session read. A cached answer is a description of the
        // past, and no previous observation may authorize a new edit or verdict.
        let correlation = CorrelationLabel::for_run(binding.agent_run_id());
        let agent = self
            .fetch_agent_with_archive(&native_id, &correlation)
            .await?;
        // An inspect is also the cheapest honest look at the permission
        // lifecycle, which on this wire lives in the agent snapshot rather than
        // in the transcript.
        self.observe_permissions(binding.binding_id(), &agent);
        self.observation(
            binding.agent_run_id(),
            binding.identity().clone(),
            &agent,
            request.requested_at,
            ObservationSource::Inspect,
        )
    }

    async fn adopt(&self, request: &AdoptRequest) -> RuntimeResult<LaunchOutcome> {
        let declared = self.declared().await?;
        if !declared.supports(RuntimeCapability::Adopt) {
            return Err(self.refuse(RuntimeCapability::Adopt, &declared));
        }
        let generation = self.generation();
        let native_id = request.native.native_id.as_str().to_owned();

        // Explicit intent or nothing. Discovery lists foreign sessions so an
        // operator can see them; it does not give Kontor the right to write
        // labels onto one, which for a human's session would be a quiet theft.
        let intent = self
            .lock()
            .adoptions
            .get(&request.native.native_id)
            .cloned()
            .ok_or(RuntimeError::LaunchNotAdmitted {
                rule: "adoption of this session has not been authorized",
            })?;

        let effective_scope = self.effective_scope(&intent.scope)?;
        let project = self.require_project_for(&effective_scope)?;
        let prepared = self
            .lock()
            .workspaces
            .get(&intent.team_run_id)
            .cloned()
            .ok_or(RuntimeError::WorkspaceBindingRequired)?;
        let workspace_id = prepared.binding.identity.native_id.as_str().to_owned();
        let task_scope = effective_scope.require_task()?;
        if task_scope.task_id != intent.task_id {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the adoption task does not match its durable execution scope",
            });
        }

        let before = self.fetch_agent(&native_id).await?;
        // An orphan, and only an orphan: a session already carrying a Kontor run
        // label belongs to a run, and re-labelling it would move it.
        if let Some(existing) = before.label(label::AGENT_RUN)
            && CorrelationLabel::parse(existing).is_ok()
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        if before.is_archived() {
            return Err(RuntimeError::StaleBinding {
                rule: "a retired session cannot be adopted",
            });
        }
        if before.workspace_id.as_deref() != Some(workspace_id.as_str())
            || WorkspaceRoot::parse(&before.cwd)? != task_scope.worktree
        {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the session is not placed in this ticket's task workspace",
            });
        }
        // The workspace itself, read fresh rather than trusted from the binding
        // that prepared it. Adoption is the one path that writes labels onto a
        // session Kontor did not create, so what it is being adopted *into* has
        // to be re-proved a worktree, this project's, at the canonical cwd, and
        // not one Paseo provisioned for itself — before the labels, because a
        // label write is the theft this whole path exists to gate.
        let workspace = self.fetch_workspace_in(&project, &workspace_id).await?;
        self.verify_workspace_placement(&workspace, &project, &task_scope.worktree)?;

        let slot = RoleSlotKey::new(intent.team_run_id, intent.role_slot_id.clone());
        {
            let state = self.lock();
            let taken = state.admissions.occupant(&slot).is_some()
                || state.admissions.is_reserved(&slot)
                || state
                    .admissions
                    .claimed_seats()
                    .any(|seat| seat.slot == slot);
            if taken {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "this seat is not free for an adopted session",
                });
            }
        }

        let labels = self.seat_labels(
            request.agent_run_id,
            intent.team_run_id,
            &intent.role_slot_id,
            &effective_scope,
            &project,
            &workspace_id,
        )?;
        let command = PaseoCommand::agent_update_labels(&native_id, &labels);
        let output = self.transport.run(&command).await?;
        let updated: PaseoCliAgentUpdated = output.parse("PaseoCliAgentUpdated")?;
        if updated.agent_id != native_id {
            return Err(RuntimeError::CorrelationFailed);
        }

        // The identity must be *unchanged*. Adoption binds the session that is
        // already there; anything else means this created one instead, which is
        // a different session with none of the history that made adopting it
        // worth doing.
        //
        // The agent id half is owned by `fetch_agent`, which refuses an answer
        // about any other id — so what is left to check here is the provider
        // session, and it is not a formality: the same Paseo agent id with a
        // rotated provider session is a fresh conversation wearing the old name,
        // and adopting it would bind a run to a transcript that no longer
        // contains the work the operator wanted kept.
        let after = self.fetch_agent(&native_id).await?;
        if after.provider.is_empty() || after.model.is_empty() {
            return Err(RuntimeError::CorrelationFailed);
        }
        if after.provider_session_id() != before.provider_session_id() {
            return Err(RuntimeError::CorrelationFailed);
        }
        self.verify_agent_placement(&after, &workspace_id, &labels)?;

        let snapshot = self.bind(
            request.agent_run_id,
            request.binding_id,
            &after,
            request.adopted_at,
            generation,
            declared,
        )?;
        let observation = self.observation(
            request.agent_run_id,
            snapshot.identity().clone(),
            &after,
            request.adopted_at,
            ObservationSource::Inspect,
        )?;
        let record = PaseoSeatRecord {
            mini_project_id: Self::external_epic_id(&effective_scope)?,
            jira_epic_key: effective_scope.epic.external_epic_key.clone(),
            plan_item_key: self.scoped_plan_item_key(&effective_scope)?,
            task_id: intent.task_id,
            team_run_id: intent.team_run_id,
            role_slot_id: intent.role_slot_id.clone(),
            agent_run_id: request.agent_run_id,
            binding_id: request.binding_id,
            placement_binding_id: prepared.binding_id().to_string(),
            canonical_worktree_cwd: task_scope.worktree.clone(),
            host_key: self.config.host_key.clone(),
            project_id: project.project_id.clone(),
            workspace_id: ExternalId::parse(&workspace_id)?,
            agent_id: ExternalId::parse(&after.id)?,
            provider: after.provider.clone(),
            model: after.model.clone(),
            thinking: after.effective_thinking_option_id.clone(),
            provider_session_id: after
                .provider_session_id()
                .map(ExternalId::parse)
                .transpose()?,
            parent_agent_id: None,
            generation,
            previous_agent_id: None,
        };
        {
            let state = &mut *self.lock();
            state.bindings.record(snapshot.clone());
            state.admissions.restore_occupied(OccupiedSeat {
                slot,
                agent_run_id: request.agent_run_id,
                binding_id: request.binding_id,
                native_id: ExternalId::parse(&after.id)?,
            });
            state
                .placements
                .insert(request.binding_id, record.workspace_id.clone());
            state.records.insert(request.binding_id, record);
            state.adoptions.remove(&request.native.native_id);
        }
        Ok(LaunchOutcome {
            snapshot,
            observation,
        })
    }

    async fn discover_sessions(&self) -> RuntimeResult<Vec<NativeSession>> {
        let declared = self.declared().await?;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Discovery,
                autonomous: false,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: None,
                context_policy: None,
            },
        )?;
        let generation = self.generation();
        let mut found = Vec::new();
        let projects = self.lock().projects.values().cloned().collect::<Vec<_>>();
        for project in projects {
            for agent in self.fetch_project_agents(&project, false).await? {
                let (state, _) = Self::normalize_agent(&agent);
                found.push(NativeSession {
                    identity: self.identity(ExternalId::parse(&agent.id)?, generation),
                    // A label that is not a Kontor one yields `None`, which is what
                    // sends a foreign session to the adoption inbox unlinked. A
                    // parent is never inferred from a title, a timestamp or
                    // proximity.
                    correlation: agent
                        .label(label::AGENT_RUN)
                        .and_then(|value| CorrelationLabel::parse(value).ok()),
                    state,
                    observed_at: Timestamp::now(),
                });
            }
        }
        Ok(found)
    }

    async fn reconcile(
        &self,
        bindings: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<ReconciliationReport> {
        let sessions = self.discover_sessions().await?;

        // Provenance first, and for a sharper reason than on the driving
        // operations: reconciliation's own output is the authority. `Matched`
        // carries the action `Keep`, so a fabricated snapshot naming a session
        // Paseo really has would come back endorsed *as* the binding to keep.
        // The unattested ones are reported rather than dropped, because an
        // unmentioned binding is one nothing ever reviews.
        let mut attested = Vec::with_capacity(bindings.len());
        let mut unattested = Vec::new();
        {
            let state = self.lock();
            for claimed in bindings {
                match state.bindings.attest(claimed) {
                    Ok(issued) => attested.push(issued.snapshot().clone()),
                    Err(_) => unattested.push(claimed),
                }
            }
        }

        let mut report = reconcile(&attested, &sessions, self.generation());
        report
            .findings
            .extend(
                unattested
                    .into_iter()
                    .map(|claimed| ReconciliationFinding::Unattested {
                        agent_run_id: claimed.agent_run_id(),
                        binding_id: claimed.binding_id(),
                        presented: claimed.identity().clone(),
                    }),
            );
        Ok(report)
    }

    async fn history(&self, request: &HistoryRequest) -> RuntimeResult<HistoryPage> {
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::History,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::HistoryPage(request.page_size)),
                context_policy: None,
            },
        )?;

        // The cursor is resolved against *this* binding, so a cursor issued for
        // another session is refused rather than silently treated as "start from
        // the beginning".
        let anchor = request
            .cursor
            .as_ref()
            .map(|cursor| cursor.resolve(binding.binding_id()))
            .transpose()?;

        let native_id = binding.identity().native_id.as_str().to_owned();
        // A cursor this adapter cannot spell in Paseo's own terms is a refusal
        // rather than a read from the start: 0.3.1 addresses a position by raw
        // epoch, and a raw epoch the registry never mapped names a numbering
        // this session was not read in.
        let cursor = match anchor {
            Some(position) => Some(self.wire_cursor(position).ok_or(
                RuntimeError::TimelineRefetchRequired {
                    reason: TimelineBreak::EpochChanged,
                },
            )?),
            None => None,
        };
        let (page, epoch, items) = if let Some(cursor) = cursor.as_ref() {
            let page = self
                .fetch_canonical(
                    &native_id,
                    PaseoDirection::After,
                    Some(cursor),
                    request.page_size,
                    // Canonical, always. `projected` collapses tool lifecycles into
                    // single entries, so a cursor built on it advances over
                    // sequences that were never delivered.
                    PaseoProjection::Canonical,
                )
                .await?;
            let epoch = self.resolve_epoch(&page.epoch, anchor.map(|position| position.epoch))?;
            let items = self.normalize_page(&page, epoch)?;
            (page, epoch, items)
        } else {
            // Paseo's only cursor-free read is a bounded tail window. It is a
            // discovery point, not an origin page: serving it through
            // `HistoryReader::start` would claim every preceding sequence was
            // seen. Follow the runtime's own start cursor backwards until it
            // says no older content remains; the caller then receives that
            // epoch-origin page and every continuation still runs `after`.
            let mut attempt = 0;
            loop {
                attempt += 1;
                let walked = async {
                    let mut page = self
                        .fetch_canonical(
                            &native_id,
                            PaseoDirection::Tail,
                            None,
                            request.page_size,
                            PaseoProjection::Canonical,
                        )
                        .await?;
                    let epoch = self.resolve_epoch(&page.epoch, None)?;
                    let mut items = self.normalize_page(&page, epoch)?;
                    while page.has_older {
                        let before = page.start_cursor.clone().ok_or(
                            RuntimeError::TimelineRefetchRequired {
                                reason: TimelineBreak::SequenceGap,
                            },
                        )?;
                        if before.epoch != page.epoch {
                            return Err(RuntimeError::TimelineRefetchRequired {
                                reason: TimelineBreak::EpochChanged,
                            });
                        }
                        let older = self
                            .fetch_canonical(
                                &native_id,
                                PaseoDirection::Before,
                                Some(&before),
                                request.page_size,
                                PaseoProjection::Canonical,
                            )
                            .await?;
                        self.resolve_epoch(&older.epoch, Some(epoch))?;
                        let older_items = self.normalize_page(&older, epoch)?;
                        if older_items
                            .last()
                            .is_none_or(|event| event.position.sequence >= before.seq)
                        {
                            return Err(RuntimeError::TimelineRefetchRequired {
                                reason: TimelineBreak::SequenceGap,
                            });
                        }
                        page = older;
                        items = older_items;
                    }
                    Ok((page, epoch, items))
                }
                .await;
                match walked {
                    Ok(origin) => break origin,
                    Err(RuntimeError::TimelineRefetchRequired { .. })
                        if attempt < CURSOR_FREE_REFETCH_ATTEMPTS =>
                    {
                        // The runtime invalidated an attempt that had not issued
                        // a Kontor cursor yet. Discard it wholesale and discover
                        // the current numbering from a new tail.
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        let end = items.last().map_or(
            anchor.unwrap_or(TimelinePosition::start_of(epoch)),
            |event| event.position,
        );
        self.reconcile_deliveries_from_history(binding.binding_id(), &items)?;
        self.lock().cursors.insert(binding.binding_id(), end);
        Ok(HistoryPage {
            epoch,
            items,
            next: page
                .has_newer
                .then(|| HistoryCursor::issue(binding.binding_id(), end)),
            end,
        })
    }

    async fn subscribe_live(
        &self,
        request: &LiveSubscribeRequest,
    ) -> RuntimeResult<LiveSubscription> {
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::LiveEvents,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;

        let native_id = binding.identity().native_id.as_str().to_owned();
        // Order matters and is the whole of the history/live race. The
        // subscription is activated *first* so frames start buffering, then the
        // canonical catch-up is fetched, then the two are merged. Fetching first
        // would drop everything Paseo emitted while the socket was being set up.
        // The whole subscribed set travels, because the request replaces this
        // connection's set rather than adding to it.
        let subscribe =
            PaseoRpc::timeline_subscribe(self.next_request_id(), std::slice::from_ref(&native_id));
        let frame = self.transport.request(&subscribe).await?;
        let acknowledged: PaseoSubscriptionAck =
            frame.resolve(&subscribe, "PaseoSubscriptionAck")?;
        // The daemon echoes exactly what it subscribed this connection to. A
        // set that does not contain this agent means the frames about to be
        // drained are somebody else's, and the catch-up would then be merged
        // against a stream that never carried this session.
        if !acknowledged.agent_ids.contains(&native_id) {
            return Err(RuntimeError::CorrelationFailed);
        }

        let buffered = self.transport.drain_stream(&native_id).await?;
        let mut merged: BTreeMap<(u64, u64), SessionEvent> = BTreeMap::new();
        for raw in buffered {
            // Before the shape, before the correlation, before the epoch
            // registry is touched: an oversized frame is refused while it is
            // still only bytes.
            ensure_frame_bounded(&raw)?;
            let envelope: PaseoStreamFrame = serde_json::from_value(
                raw.get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            )
            .map_err(|_| {
                RuntimeError::Domain(DomainError::invalid(
                    "PaseoStreamFrame",
                    "is not the Paseo 0.3.1 frame this adapter is pinned to",
                ))
            })?;
            // The transport routes by agent, so this is the second, independent
            // check that what was drained is this session's. A frame for
            // another agent here means the router is wrong, and continuing
            // would merge somebody else's content into this timeline.
            if envelope.agent_id != native_id {
                return Err(RuntimeError::CorrelationFailed);
            }
            // The permission lifecycle arrives here rather than in the
            // transcript, so this is where it is recorded. It carries no
            // sequence and is never merged into content.
            if let Some(permission_id) = stream_permission_external_id(&envelope.event)? {
                match envelope.event.event_type.as_str() {
                    "permission_requested" => {
                        self.open_permission(binding.binding_id(), &permission_id);
                    }
                    "permission_resolved" => self.close_permission(&permission_id),
                    _ => {}
                }
            }
            if let Some((raw_epoch, entry)) = envelope.as_entry() {
                let epoch = self.resolve_epoch(&raw_epoch, Some(request.strict_after.epoch))?;
                let event = normalize_entry(&entry, epoch)?;
                merged.insert((epoch, entry.seq_start), event);
            }
        }

        // The catch-up fetch closes the window between the history anchor and
        // the first buffered frame. Deduplication is by raw `(epoch, sequence)`,
        // so an entry that arrives both ways is one event.
        let anchor = self.wire_cursor(request.strict_after).ok_or(
            RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::EpochChanged,
            },
        )?;
        let catch_up = self
            .fetch_canonical(
                &native_id,
                PaseoDirection::After,
                Some(&anchor),
                MAX_HISTORY_PAGE,
                PaseoProjection::Canonical,
            )
            .await?;
        let epoch = self.resolve_epoch(&catch_up.epoch, Some(request.strict_after.epoch))?;
        for event in self.normalize_page(&catch_up, epoch)? {
            merged.insert((epoch, event.position.sequence), event);
        }

        let events = merged
            .into_values()
            .filter(|event| event.position.sequence > request.strict_after.sequence)
            .collect::<Vec<_>>();
        Ok(LiveSubscription::new(
            request.kinds.clone(),
            request.strict_after,
            events,
            // The subscription ended because this drain ended, which is a fact
            // about the channel. It is never a completion.
            true,
        ))
    }

    async fn respond_permission(
        &self,
        request: &PermissionResponseRequest,
    ) -> RuntimeResult<PermissionAck> {
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        let generation = self.generation();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::PermissionResponse,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;

        // The ledger decides. An unknown request, one another session raised, a
        // second answer under a different id and the same id carrying a
        // different answer are all refusals; the exact same answer replays.
        match self.lock().permissions.classify(
            binding.binding_id(),
            &request.permission_id,
            request.response_id,
            request.decision,
        )? {
            Admission::Replay(original) => return Ok(original),
            Admission::New => {}
        }

        // The ledger speaks first, so Kontor's own answer still replays. What
        // is left is a request history shows answered *without* Kontor having
        // sent the answer — the operator in Paseo's UI, or a process that ended
        // before it could record its acknowledgement. Dispatching now would act
        // a second time on someone else's decision.
        if self
            .lock()
            .resolved_in_history
            .contains(&request.permission_id)
        {
            return Err(RuntimeError::PermissionConflict {
                rule: "was already resolved in this session's content",
            });
        }

        let native_id = binding.identity().native_id.as_str().to_owned();
        // The correlation id *is* the permission request id: 0.3.1 answers an
        // `agent_permission_response` with an `agent_permission_resolved` whose
        // `requestId` is the permission's, and there is no second id on either
        // half to correlate by.
        let rpc = PaseoRpc::permission_response(
            &native_id,
            request.permission_id.as_str(),
            request.decision == PermissionDecision::Allow,
        );
        let outcome = self.transport.request(&rpc).await;
        match outcome {
            Ok(frame) => {
                let accepted: PaseoPermissionResolved =
                    frame.resolve(&rpc, "PaseoPermissionResolved")?;
                if accepted.agent_id != native_id
                    || accepted.resolution.behavior != request.decision_body()
                {
                    return Err(RuntimeError::Transport {
                        rule: "runtime acknowledged something other than this permission answer",
                    });
                }
            }
            // Unknown delivery is never blindly resent. Whether the answer
            // landed is a question canonical history can answer, and answering
            // it is strictly better than acting twice on someone's behalf.
            Err(error) => {
                return match self.reconcile_permission(&binding, request).await? {
                    Some(acknowledgement) => Ok(acknowledgement),
                    None => Err(error),
                };
            }
        }

        match self.reconcile_permission(&binding, request).await? {
            Some(acknowledgement) => Ok(acknowledgement),
            None => Err(RuntimeError::Transport {
                rule: "runtime accepted the answer but its resolution has not appeared yet",
            }),
        }
    }

    /// Report, honestly, that this daemon cannot compact a seat's context.
    ///
    /// The Paseo 0.3.1 protocol exposes no per-seat context configuration and no
    /// compaction operation, so [`PaseoAdapter::capabilities`] advertises
    /// neither [`RuntimeCapability::ContextPolicy`] nor
    /// [`RuntimeCapability::Compact`], and this method emits **no RPC at all**.
    ///
    /// What it deliberately does not do is the whole point. Paseo *can* reload,
    /// archive, cancel and create agents, and any of those could be dressed up
    /// as "compaction" — a fresh agent with a summarized prompt looks like a
    /// compacted seat from the outside. It would be a different session, the
    /// native id would change, and the receipt would be a lie. So the answer is
    /// a receipt that says `pending` for a `required` policy — which blocks
    /// reuse — or `unsupported` for `best_effort`, which is visible and lets the
    /// work continue. Neither is ever `confirmed`, and no message is sent to the
    /// model in the hope that it compacts itself.
    async fn compact(&self, request: &CompactRequest) -> RuntimeResult<CompactionReceipt> {
        request.validate()?;
        // The binding is still attested, so a stale or forged one is refused
        // rather than answered — reporting a limitation is not a reason to stop
        // checking who is asking.
        let binding = self.attested(&request.binding)?;
        let declared = self.declared().await?;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Inspect,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                placement: None,
                current_generation: Some(self.generation()),
                demand: None,
                context_policy: None,
            },
        )?;
        Ok(request.unsupported_receipt(&declared, request.requested_at)?)
    }
}

// ---------------------------------------------------------------------------
// Delivery reconciliation
// ---------------------------------------------------------------------------

impl PaseoAdapter {
    /// Settle a persistent hosted-seat message directly against one exact
    /// native's canonical history.
    ///
    /// Unlike Delivery AgentRuns, a hosted LSA/TPM has no RuntimeBindingId to
    /// key adapter cursors or an issued-message ledger by. The stable UUIDv7 is
    /// therefore searched from the canonical tail across a bounded complete
    /// history. No hit authorizes one send; one hit is the acknowledgement;
    /// more than one is durable divergence and never authorizes another effect.
    async fn reconcile_hosted_message(
        &self,
        native_id: &str,
        message_id: MessageId,
    ) -> RuntimeResult<Option<(TimelinePosition, Timestamp)>> {
        let mut expected = None;
        let mut before: Option<PaseoTimelineCursor> = None;
        let mut found: Option<(TimelinePosition, Timestamp)> = None;
        let mut hits = 0usize;
        let mut complete = false;
        for _ in 0..RECONCILE_PAGE_BUDGET {
            let page = self
                .fetch_canonical(
                    native_id,
                    if before.is_some() {
                        PaseoDirection::Before
                    } else {
                        PaseoDirection::Tail
                    },
                    before.as_ref(),
                    MAX_HISTORY_PAGE,
                    PaseoProjection::Canonical,
                )
                .await?;
            let epoch = self.resolve_epoch(&page.epoch, expected)?;
            expected = Some(epoch);
            for event in self.normalize_page(&page, epoch)? {
                if event.subject == EventSubject::Message(message_id) {
                    hits += 1;
                    found.get_or_insert((event.position, event.emitted_at));
                }
            }
            match (page.has_older, page.start_cursor) {
                (true, Some(start)) => before = Some(start),
                (false, _) => {
                    complete = true;
                    break;
                }
                (true, None) => {
                    return Err(RuntimeError::TimelineRefetchRequired {
                        reason: TimelineBreak::SequenceGap,
                    });
                }
            }
        }
        if !complete {
            return Err(RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::SequenceGap,
            });
        }
        if hits > 1 {
            return Err(RuntimeError::DuplicateMessage {
                rule: "appears more than once in this hosted seat's canonical content",
            });
        }
        Ok(found)
    }

    fn record_delivery(
        &self,
        message_id: MessageId,
        body_hash: ContentHash,
        delivery: PaseoDelivery,
    ) {
        let state = &mut *self.lock();
        state
            .messages
            .record(message_id, body_hash.clone(), delivery.clone());
        state.deliveries.push((message_id, body_hash, delivery));
    }

    /// Promote confirmation-unknown deliveries when an ordinary history read
    /// encounters their exact native message id.
    ///
    /// A history page is canonical runtime evidence. Keeping an already-seen
    /// message unknown would force a later retry to search backwards from a
    /// moving tail and could turn an incomplete read into a second send. The
    /// page therefore repairs the adapter ledger at the exact epoch/sequence it
    /// returns to the caller.
    fn reconcile_deliveries_from_history(
        &self,
        binding_id: RuntimeBindingId,
        events: &[SessionEvent],
    ) -> RuntimeResult<()> {
        let state = &mut *self.lock();
        for event in events {
            let EventSubject::Message(message_id) = &event.subject else {
                continue;
            };
            let message_id = *message_id;
            let mut acknowledged = false;
            for (_, _, delivery) in state
                .deliveries
                .iter()
                .filter(|(id, _, _)| *id == message_id)
            {
                if let PaseoDelivery::Acknowledged(receipt) = delivery {
                    if receipt.position != event.position {
                        return Err(RuntimeError::DuplicateMessage {
                            rule: "appears more than once in this session's canonical content",
                        });
                    }
                    acknowledged = true;
                }
            }
            if acknowledged {
                continue;
            }
            let Some(body_hash) = state.deliveries.iter().find_map(|(id, hash, delivery)| {
                (*id == message_id && matches!(delivery, PaseoDelivery::ConfirmationUnknown))
                    .then(|| hash.clone())
            }) else {
                continue;
            };
            let receipt = MessageAck {
                message_id,
                binding_id,
                position: event.position,
                accepted_at: event.emitted_at,
            };
            state.deliveries.retain(|(id, _, _)| *id != message_id);
            state.deliveries.push((
                message_id,
                body_hash.clone(),
                PaseoDelivery::Acknowledged(receipt.clone()),
            ));
            state
                .messages
                .record(message_id, body_hash, PaseoDelivery::Acknowledged(receipt));
        }
        Ok(())
    }

    /// Settle one message against canonical history, by exact native id.
    ///
    /// Returns the acknowledgement the *timeline* supports, or `None` when the
    /// message is not there. Two matching entries is divergence, not a luckier
    /// answer: it means the id was delivered twice, and the position of the
    /// second is no more the message's position than the first.
    async fn reconcile_message(
        &self,
        binding: &RuntimeBindingSnapshot,
        request: &SendMessageRequest,
    ) -> RuntimeResult<Option<MessageAck>> {
        let wanted = request.message_id;
        let found = self
            .scan_canonical(binding, |event| {
                event.subject == EventSubject::Message(wanted)
            })
            .await?;
        let Some((position, hits)) = found else {
            return Ok(None);
        };
        if hits > 1 {
            return Err(RuntimeError::DuplicateMessage {
                rule: "appears more than once in this session's canonical content",
            });
        }
        let acknowledgement = MessageAck {
            message_id: request.message_id,
            binding_id: binding.binding_id(),
            position,
            accepted_at: request.sent_at,
        };
        self.record_delivery(
            request.message_id,
            request.body_hash(),
            PaseoDelivery::Acknowledged(acknowledgement.clone()),
        );
        Ok(Some(acknowledgement))
    }

    /// Settle one permission answer against a fresh agent readback.
    ///
    /// Not against canonical history, because 0.3.1's canonical timeline has no
    /// permission items at all: `pendingPermissions` on the agent snapshot is
    /// where an open request lives, so a request that is *gone* from that list
    /// is the resolution evidence this wire offers.
    ///
    /// The position recorded is the session's current end rather than the
    /// resolution's own, because the resolution has no position in a transcript
    /// that does not contain it. Minting one would be an adapter-local number
    /// claiming to be a place in Paseo's content.
    async fn reconcile_permission(
        &self,
        binding: &RuntimeBindingSnapshot,
        request: &PermissionResponseRequest,
    ) -> RuntimeResult<Option<PermissionAck>> {
        let native_id = binding.identity().native_id.as_str().to_owned();
        let agent = self.fetch_agent(&native_id).await?;
        self.observe_permissions(binding.binding_id(), &agent);
        let still_pending = agent
            .pending_permissions
            .iter()
            .any(|pending| pending.id == request.permission_id.as_str());
        if still_pending {
            return Ok(None);
        }
        let position = self
            .lock()
            .cursors
            .get(&binding.binding_id())
            .copied()
            .unwrap_or_else(|| TimelinePosition::start_of(0));
        let acknowledgement = PermissionAck {
            permission_id: request.permission_id.clone(),
            response_id: request.response_id,
            binding_id: binding.binding_id(),
            decision: request.decision,
            position,
            accepted_at: request.responded_at,
        };
        {
            let state = &mut *self.lock();
            state
                .permissions
                .record(request.permission_id.clone(), acknowledgement.clone());
            state
                .permission_acks
                .push((request.permission_id.clone(), acknowledgement.clone()));
            // Nothing to close here: this acknowledgement exists because the
            // request had left `pendingPermissions`, which means
            // `observe_permissions` already closed it on the way past. What this
            // adds is the half Paseo cannot carry — that the answer was
            // *Kontor's*, under this response id — which is what lets the same
            // answer replay.
        }
        Ok(Some(acknowledgement))
    }
}

/// The decision spelling Paseo records, for a caller building a fixture.
#[must_use]
pub const fn decision_body(decision: PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow => "allow",
        PermissionDecision::Deny => "deny",
    }
}

/// Attest one persisted claim against what this runtime independently knows.
///
/// # What the runtime can still source after a restart, and what it cannot
///
/// The registry that recorded what this adapter issued lives in adapter memory
/// and is rebuilt empty on every daemon start, so there is no stored copy of the
/// issued bytes left to compare against. What the *live daemon* can still answer
/// independently is checked here, and nothing is recorded until all of it holds:
///
/// * **whole native identity** — runtime kind, host, generation and native id.
///   A repeated native id in a new generation is a different session.
/// * **the session's own correlation** — the live session must itself report the
///   Kontor label of the run the claim names. This is what refuses a forged but
///   self-consistent claim: an attacker can write any `agent_run_id` into a
///   stored document, and cannot make a running session report a label it does
///   not carry.
/// * **internal consistency** — the correlation travelling inside the snapshot
///   must be evidence *for that snapshot*.
/// * **a capability upper bound** — a claim may be weaker than the live runtime
///   and never stronger, so one asserting a grade, capability or limit beyond
///   what this runtime declares today was never issued by it.
///
/// What it cannot prove is a claim tampered *within* what the runtime can
/// currently do — downgrading a limit, or promoting a grade the runtime already
/// declares. Detecting that needs a runtime-side durable record of the issued
/// bytes, which this adapter has nowhere to keep. The bound above is deliberately
/// one-directional so the dangerous direction — claiming more authority than the
/// runtime has — is the one that is refused.
fn attest_restored(
    snapshot: &RuntimeBindingSnapshot,
    live: &[NativeSession],
    declared: &RuntimeCapabilities,
) -> RuntimeResult<()> {
    snapshot.ensure_correlated()?;
    let session = live
        .iter()
        .find(|session| &session.identity == snapshot.identity())
        .ok_or(RuntimeError::StaleBinding {
            rule: "this runtime holds no session with that native identity in this generation",
        })?;
    if session.correlation != Some(CorrelationLabel::for_run(snapshot.agent_run_id())) {
        return Err(RuntimeError::CorrelationFailed);
    }
    if !snapshot.within(declared) {
        return Err(RuntimeError::StaleBinding {
            rule: "the claim asserts capability this runtime cannot currently prove",
        });
    }
    Ok(())
}

impl PaseoAdapter {
    /// Re-prove where each attested seat is placed, from the live agents.
    ///
    /// The workspace id is taken from the agent's *own* report and then put
    /// through the same two checks a driving operation makes: the agent must be
    /// working in this plane's canonical task worktree, and the workspace it
    /// names must be this plane's project's registration of that worktree. So a
    /// placement is recovered only when the runtime independently confirms it,
    /// on the same terms as if it had never been lost.
    ///
    /// A seat whose agent cannot be found, has moved, or sits in a workspace this
    /// plane does not own is simply absent from the answer — and its binding is
    /// then not restored either.
    async fn reprove_placements(
        &self,
        snapshots: &[RuntimeBindingSnapshot],
        live: &[NativeSession],
    ) -> RuntimeResult<BTreeMap<RuntimeBindingId, ExternalId>> {
        let mut placements = BTreeMap::new();
        for snapshot in snapshots {
            let native = &snapshot.identity().native_id;
            if !live
                .iter()
                .any(|session| &session.identity == snapshot.identity())
            {
                continue;
            }
            let Ok(agent) = self.fetch_agent(native.as_str()).await else {
                continue;
            };
            let Ok(project) = self.recover_project_for_agent(&agent).await else {
                continue;
            };
            let Some(workspace_id) = agent.workspace_id.clone() else {
                continue;
            };
            let Some(root) = agent
                .label(label::WORKTREE)
                .and_then(|root| WorkspaceRoot::parse(root).ok())
            else {
                continue;
            };
            // Both halves, exactly as `verify_seat_placement` asks them.
            if self
                .verify_agent_location(&agent, &workspace_id, &root)
                .is_err()
            {
                continue;
            }
            let Ok(workspace) = self.fetch_workspace_in(&project, &workspace_id).await else {
                continue;
            };
            if self
                .verify_workspace_placement(&workspace, &project, &root)
                .is_err()
            {
                continue;
            }
            if let Ok(parsed) = ExternalId::parse(&workspace_id) {
                placements.insert(snapshot.binding_id(), parsed);
            }
        }
        Ok(placements)
    }
}

#[cfg(test)]
mod task_scope_tests {
    use super::*;

    fn adapter() -> PaseoAdapter {
        PaseoAdapter::new(
            PaseoConfig {
                runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("runtime kind"),
                host_key: ExternalName::parse("fixture-host").expect("host"),
                mini_project_id: ExternalId::parse("01890000-0000-7000-8000-0000000000c1")
                    .expect("epic selector"),
                scope: scope(),
                max_concurrent_sessions: 8,
                unavailable_providers: BTreeSet::new(),
                provider_selects_account: false,
                provider_fallbacks: BTreeMap::new(),
                adopted_containers: BTreeMap::new(),
                seat_mcp: None,
            },
            Box::new(std::sync::Arc::new(crate::fixture::RecordedPaseo::new())),
            PaseoCheckpoint::fresh(
                1,
                ExternalName::parse("fixture-host").expect("checkpoint host"),
            ),
        )
        .expect("the fixture adapter builds")
    }

    fn task(value: &str) -> TaskId {
        TaskId::parse(value).expect("a canonical task id")
    }

    fn scope() -> PaseoExecutionScope {
        PaseoExecutionScope {
            jira_epic_key: ExternalId::parse("ASMA-7869").expect("epic"),
            mini_project_short_title: ExternalName::parse("Kontor Operational MVP").expect("title"),
            plan_item_key: ExternalId::parse("OP-01").expect("plan item"),
            jira_issue_key: ExternalId::parse("ASMA-7870").expect("ticket"),
            ticket_short_code: ExternalId::parse("OP-01").expect("short code"),
            seat_display_roles: BTreeMap::new(),
            project_root_cwd: WorkspaceRoot::parse("/repo").expect("root"),
            canonical_worktree_cwd: WorkspaceRoot::parse("/repo/op-01").expect("worktree"),
            task_scopes: BTreeMap::new(),
            orchestrator_agent_id: ExternalId::parse("orchestrator").expect("agent"),
        }
    }

    #[test]
    fn a_core_team_seat_accepts_the_exact_local_ecp_but_ticket_rules_do_not() {
        let adapter = adapter();
        let project = PaseoProjectBinding {
            mini_project_id: ExternalId::parse("01890000-0000-7000-8000-0000000000c1")
                .expect("epic"),
            host_key: ExternalName::parse("fixture-host").expect("host"),
            project_id: ExternalId::parse("prj_epic").expect("project"),
            observed_name: "Epic · ASMA-7869 · Kontor Operational MVP".to_owned(),
        };
        let root = WorkspaceRoot::parse("/repo").expect("root");
        let workspace = PaseoWorkspace {
            id: "wks_ecp".to_owned(),
            project_id: "prj_epic".to_owned(),
            workspace_directory: "/repo".to_owned(),
            workspace_kind: PaseoWorkspaceKind::Directory,
            name: "ECP · ASMA-7869 · Kontor Operational MVP".to_owned(),
            title: None,
            git_runtime: None,
        };

        adapter
            .verify_hosted_workspace_placement(&workspace, &project, &root)
            .expect("leadership belongs in the exact local ECP");
        assert!(matches!(
            adapter.verify_workspace_placement(&workspace, &project, &root),
            Err(RuntimeError::WorkspaceMismatch { .. })
        ));
    }

    #[test]
    fn only_an_explicitly_configured_owned_worktree_is_adopted() {
        let project = PaseoProjectBinding {
            mini_project_id: ExternalId::parse("01890000-0000-7000-8000-0000000000c1")
                .expect("epic"),
            host_key: ExternalName::parse("fixture-host").expect("host"),
            project_id: ExternalId::parse("prj_epic").expect("project"),
            observed_name: "Epic · ASMA-7869 · Kontor Operational MVP".to_owned(),
        };
        let root = WorkspaceRoot::parse("/repo/op-01").expect("worktree");
        let workspace = PaseoWorkspace {
            id: "wks_existing".to_owned(),
            project_id: "prj_epic".to_owned(),
            workspace_directory: "/repo/op-01".to_owned(),
            workspace_kind: PaseoWorkspaceKind::Worktree,
            name: "TSW · ASMA-7870 · OP-01".to_owned(),
            title: None,
            git_runtime: Some(crate::wire::PaseoGitRuntime {
                is_paseo_owned_worktree: true,
            }),
        };

        assert!(matches!(
            adapter().verify_workspace_placement(&workspace, &project, &root),
            Err(RuntimeError::WorkspaceMismatch { .. })
        ));

        let mut adopted_containers = BTreeMap::new();
        adopted_containers.insert(
            TopologyNodeId::parse("01890000-0000-7000-8000-0000000000d1").expect("node"),
            ExternalId::parse("wks_existing").expect("workspace"),
        );
        let adapter = PaseoAdapter::new(
            PaseoConfig {
                runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("runtime kind"),
                host_key: ExternalName::parse("fixture-host").expect("host"),
                mini_project_id: ExternalId::parse("01890000-0000-7000-8000-0000000000c1")
                    .expect("epic selector"),
                scope: scope(),
                max_concurrent_sessions: 8,
                unavailable_providers: BTreeSet::new(),
                provider_selects_account: false,
                provider_fallbacks: BTreeMap::new(),
                adopted_containers,
                seat_mcp: None,
            },
            Box::new(std::sync::Arc::new(crate::fixture::RecordedPaseo::new())),
            PaseoCheckpoint::fresh(
                1,
                ExternalName::parse("fixture-host").expect("checkpoint host"),
            ),
        )
        .expect("the fixture adapter builds");
        adapter
            .verify_workspace_placement(&workspace, &project, &root)
            .expect("the exact configured native workspace is adoptable");
    }

    /// Imported epics used to leak their Kontor `MiniProjectId` into the public
    /// `jira.epic` label. That made a correctly placed QNR seat undiscoverable
    /// by its Jira identity and made in-place repair reproduce the same defect.
    /// The external Jira key and the internal Kontor project id are deliberately
    /// separate facts and both stay pinned here.
    #[test]
    fn a_second_epics_seat_labels_keep_external_and_internal_project_identity_distinct() {
        let adapter = adapter();
        let qnr_epic =
            kontor_core::id::MiniProjectId::parse("01a019c0-eee7-72a1-a8a7-7fff1ddce8f3")
                .expect("the QNR epic id");
        let qnr_task = task("01a019c0-eee7-72a1-a8a7-8006a40be8c1");
        let qnr_scope = ExecutionScope::for_task(
            EpicScope {
                mini_project_id: qnr_epic,
                external_epic_key: ExternalId::parse("ASMA-7675").expect("the Jira epic"),
                short_title: ExternalName::parse("QNR v2 Nonprod Delivery")
                    .expect("the epic title"),
            },
            TaskScope {
                task_id: qnr_task,
                external_issue_key: ExternalId::parse("ASMA-7676").expect("the Jira task"),
                short_code: ExternalId::parse("grid-column-ops").expect("the ticket code"),
                worktree: WorkspaceRoot::parse("/repo/qnr/asma-7676").expect("the worktree"),
            },
        );
        let project = PaseoProjectBinding {
            mini_project_id: ExternalId::parse(&qnr_epic.to_string())
                .expect("the internal project label"),
            host_key: ExternalName::parse("fixture-host").expect("the host"),
            project_id: ExternalId::parse("prj_qnr").expect("the native project"),
            observed_name: "Epic · ASMA-7675 · QNR v2 Nonprod Delivery".to_owned(),
        };

        let labels = adapter
            .seat_labels(
                AgentRunId::generate(),
                TeamRunId::generate(),
                &RoleSlotId::parse("builder").expect("the role slot"),
                &qnr_scope,
                &project,
                "wks_qnr_7676",
            )
            .expect("the durable scope renders labels");

        assert_eq!(
            labels.get(label::JIRA_EPIC).map(String::as_str),
            Some("ASMA-7675")
        );
        assert_eq!(
            labels.get(label::PROJECT_ID).map(String::as_str),
            Some("01a019c0-eee7-72a1-a8a7-7fff1ddce8f3"),
            "kontor.project_id remains the documented Kontor epic id"
        );
        assert_ne!(labels[label::JIRA_EPIC], labels[label::PROJECT_ID]);
    }

    #[test]
    fn a_direct_adapter_refuses_a_non_kontor_epic_selector() {
        let error = PaseoAdapter::new(
            PaseoConfig {
                runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("runtime kind"),
                host_key: ExternalName::parse("fixture-host").expect("host"),
                mini_project_id: ExternalId::parse("ASMA-7869").expect("legacy selector"),
                scope: scope(),
                max_concurrent_sessions: 8,
                unavailable_providers: BTreeSet::new(),
                provider_selects_account: false,
                provider_fallbacks: BTreeMap::new(),
                adopted_containers: BTreeMap::new(),
                seat_mcp: None,
            },
            Box::new(std::sync::Arc::new(crate::fixture::RecordedPaseo::new())),
            PaseoCheckpoint::fresh(
                1,
                ExternalName::parse("fixture-host").expect("checkpoint host"),
            ),
        )
        .expect_err("the invalid selector is refused during composition");
        assert!(
            matches!(error, RuntimeError::Domain(_)),
            "the refusal stays at the typed adapter boundary: {error:?}"
        );
    }

    #[test]
    fn one_epic_runtime_routes_each_task_to_its_own_worktree() {
        let first = task("01a0074f-671a-7420-b395-163d160d9792");
        let second = task("01a0074f-671d-7560-8a66-6a21972e78ae");
        let mut scope = scope();
        scope.task_scopes.insert(
            first,
            PaseoTaskScope {
                plan_item_key: ExternalId::parse("OP-01").expect("plan item"),
                jira_issue_key: ExternalId::parse("ASMA-7870").expect("ticket"),
                ticket_short_code: ExternalId::parse("OP-01").expect("short code"),
                canonical_worktree_cwd: WorkspaceRoot::parse("/repo/op-01").expect("worktree"),
            },
        );
        scope.task_scopes.insert(
            second,
            PaseoTaskScope {
                plan_item_key: ExternalId::parse("OP-02").expect("plan item"),
                jira_issue_key: ExternalId::parse("ASMA-7871").expect("ticket"),
                ticket_short_code: ExternalId::parse("OP-02").expect("short code"),
                canonical_worktree_cwd: WorkspaceRoot::parse("/repo/op-02").expect("worktree"),
            },
        );

        assert_eq!(
            scope
                .task_scope(first)
                .expect("first task")
                .canonical_worktree_cwd,
            WorkspaceRoot::parse("/repo/op-01").expect("worktree")
        );
        assert_eq!(
            scope
                .task_scope(second)
                .expect("second task")
                .canonical_worktree_cwd,
            WorkspaceRoot::parse("/repo/op-02").expect("worktree")
        );
        assert!(
            scope
                .task_scope(task("01a0074f-6721-7333-b205-4d0108e157b0"))
                .is_err()
        );

        let configured_epic =
            kontor_core::id::MiniProjectId::parse("01890000-0000-7000-8000-0000000000c1")
                .expect("a configured epic id");
        let adapter = PaseoAdapter::new(
            PaseoConfig {
                runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("runtime kind"),
                host_key: ExternalName::parse("fixture-host").expect("host"),
                mini_project_id: ExternalId::parse(&configured_epic.to_string())
                    .expect("configured epic"),
                scope,
                max_concurrent_sessions: 8,
                unavailable_providers: BTreeSet::new(),
                provider_selects_account: false,
                provider_fallbacks: BTreeMap::new(),
                adopted_containers: BTreeMap::new(),
                seat_mcp: None,
            },
            Box::new(std::sync::Arc::new(crate::fixture::RecordedPaseo::new())),
            PaseoCheckpoint::fresh(
                1,
                ExternalName::parse("fixture-host").expect("checkpoint host"),
            ),
        )
        .expect("the fixture adapter builds");

        let foreign = ExecutionScope::for_task(
            EpicScope {
                mini_project_id: kontor_core::id::MiniProjectId::parse(
                    "01890000-0000-7000-8000-0000000000c2",
                )
                .expect("a second epic id"),
                external_epic_key: ExternalId::parse("ASMA-9000").expect("foreign epic key"),
                short_title: ExternalName::parse("QNR V2").expect("foreign title"),
            },
            TaskScope {
                task_id: second,
                external_issue_key: ExternalId::parse("ASMA-9001").expect("foreign ticket"),
                short_code: ExternalId::parse("QNR-01").expect("foreign code"),
                worktree: WorkspaceRoot::parse("/repo/qnr-01").expect("foreign worktree"),
            },
        );
        assert_eq!(
            adapter.effective_scope(&foreign).expect("foreign scope"),
            foreign,
            "a second epic never inherits this plane's compatibility rendering"
        );

        let durable_root = WorkspaceRoot::parse("/repo/durable-op-01").expect("durable worktree");
        let configured = ExecutionScope::for_task(
            EpicScope {
                mini_project_id: configured_epic,
                external_epic_key: ExternalId::parse("legacy-epic").expect("legacy epic"),
                short_title: ExternalName::parse("Legacy title").expect("legacy title"),
            },
            TaskScope {
                task_id: first,
                external_issue_key: ExternalId::parse("legacy-ticket").expect("legacy ticket"),
                short_code: ExternalId::parse("legacy-code").expect("legacy code"),
                worktree: durable_root.clone(),
            },
        );
        let effective = adapter
            .effective_scope(&configured)
            .expect("configured compatibility scope");
        assert_eq!(
            effective.require_task().expect("task").worktree,
            durable_root,
            "compatibility rendering never overrides durable placement"
        );
        assert_eq!(
            effective.require_task().expect("task").short_code.as_str(),
            "OP-01",
            "the existing visible seat spelling remains compatible"
        );

        let dynamic_task = task("01a01b8e-3ea7-74e0-a12e-5fb86d347984");
        let dynamic_root =
            WorkspaceRoot::parse("/repo/.worktrees/feat/ASMA-7952-kontor-remaining-admin-controls")
                .expect("the OP-18 worktree");
        let dynamic = ExecutionScope::for_task(
            EpicScope {
                mini_project_id: configured_epic,
                external_epic_key: ExternalId::parse("legacy-epic").expect("legacy epic"),
                short_title: ExternalName::parse("Legacy title").expect("legacy title"),
            },
            TaskScope {
                task_id: dynamic_task,
                external_issue_key: ExternalId::parse("ASMA-7952").expect("OP-18 issue"),
                short_code: ExternalId::parse("ASMA-7952").expect("OP-18 code"),
                worktree: dynamic_root.clone(),
            },
        );
        let effective = adapter
            .effective_scope(&dynamic)
            .expect("a dynamic task under the configured epic");
        assert_eq!(
            effective.require_task().expect("task").worktree,
            dynamic_root,
            "a task absent from runtimes.json keeps its durable worktree"
        );
        assert_eq!(
            adapter
                .scoped_plan_item_key(&effective)
                .expect("the dynamic plan item")
                .as_str(),
            "ASMA-7952",
            "a task absent from the static overrides uses its durable issue key"
        );
    }
}

#[cfg(test)]
mod governed_pin_tests {
    //! The governed account pin, at the only two places it is decided:
    //! whether the plane declares `account_env`, and whether the provider
    //! readback catches a launch that landed on another alias.

    use kontor_core::spec::{ModelRef, ProviderRef};

    use super::*;

    fn config(provider_selects_account: bool) -> PaseoConfig {
        PaseoConfig {
            runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("runtime kind"),
            host_key: ExternalName::parse("fixture-host").expect("host"),
            mini_project_id: ExternalId::parse("01890000-0000-7000-8000-0000000000c1")
                .expect("epic selector"),
            scope: PaseoExecutionScope {
                jira_epic_key: ExternalId::parse("ASMA-7882").expect("epic"),
                mini_project_short_title: ExternalName::parse("Quota headroom").expect("title"),
                plan_item_key: ExternalId::parse("KON-OP-13").expect("plan item"),
                jira_issue_key: ExternalId::parse("ASMA-7882").expect("issue"),
                ticket_short_code: ExternalId::parse("OP13").expect("short code"),
                seat_display_roles: BTreeMap::new(),
                project_root_cwd: WorkspaceRoot::parse("/w/epic").expect("root"),
                canonical_worktree_cwd: WorkspaceRoot::parse("/w/task").expect("worktree"),
                task_scopes: BTreeMap::new(),
                orchestrator_agent_id: ExternalId::parse("agent-1").expect("orchestrator"),
            },
            max_concurrent_sessions: 8,
            unavailable_providers: BTreeSet::new(),
            provider_selects_account,
            provider_fallbacks: BTreeMap::new(),
            adopted_containers: BTreeMap::new(),
            seat_mcp: None,
        }
    }

    /// An agent Paseo reports back on one route. The provider field is the
    /// account evidence: it is what the readback compares.
    fn agent_on_route(provider: &str, model: &str) -> PaseoAgent {
        // Built through the wire shape rather than a struct literal, so the
        // fixture is an answer Paseo could actually have sent.
        serde_json::from_value(serde_json::json!({
            "id": "native-1",
            "provider": provider,
            "model": model,
            "currentModeId": paseo_mode(provider, SeatAutonomy::standard())
                .expect("the fixture provider has a permission mode"),
            "status": "running",
        }))
        .expect("a deserializable agent readback")
    }

    /// A plane that has not declared the pin must refuse an account-pinned
    /// launch, because a claim it cannot prove is worse than a refusal.
    #[test]
    fn a_plane_that_has_not_declared_the_pin_reports_no_account_environment() {
        let config = config(false);
        assert!(
            !config.capabilities().account_env,
            "absent a declaration, Kontor must not claim a per-run account guarantee"
        );
        assert!(!config.degraded_capabilities().account_env);
    }

    /// One provider alias per coding account is what makes `--provider` an
    /// account selector, and it is the whole mechanism behind account-before-rung
    /// resolution reaching a second Codex login.
    #[test]
    fn a_plane_declaring_one_alias_per_account_reports_an_account_environment() {
        let config = config(true);
        assert!(config.capabilities().account_env);
    }

    /// The pin is enforced by readback, not by hope: a launch that came back on
    /// a different provider alias landed on a different account, and that is a
    /// correlation failure rather than a successful launch.
    #[test]
    fn a_launch_that_returns_a_different_provider_alias_fails_correlation() {
        let requested = ModelRung {
            provider: ProviderRef("codex-work".to_owned()),
            model: ModelRef("gpt-5.6-sol".to_owned()),
            effort: None,
        };
        let mut agent = agent_on_route("codex-work", "gpt-5.6-sol");
        assert!(
            PaseoAdapter::verify_agent_route(&agent, &requested, SeatAutonomy::standard()).is_ok(),
            "the alias that was asked for is the alias that must be reported"
        );
        // The other Codex login. Same provider family — `built_in_provider`
        // normalizes both aliases to `codex`, so the permission mode and every
        // family-level rule still agree — and yet a different account. The
        // readback is the only thing that can tell them apart.
        agent.provider = "codex-personal".to_owned();
        assert!(
            matches!(
                PaseoAdapter::verify_agent_route(&agent, &requested, SeatAutonomy::standard()),
                Err(RuntimeError::CorrelationFailed)
            ),
            "a launch on another account's alias is not this account's launch"
        );
    }
}
