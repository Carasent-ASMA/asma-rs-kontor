//! The Codex adapter: account isolation, refusals, normalization and process
//! evidence, in one place.
//!
//! # What this adapter is for
//!
//! One thing: running a task under a **provably chosen coding account**. Codex is
//! the only runtime in the fleet that binds its identity to a directory —
//! `CODEX_HOME` — so it is the only one that can answer "which account executed
//! this?" with something better than "whatever the daemon was logged in as". That
//! is why this adapter exists next to the AO and Paseo ones, which both declare
//! `account_env: false`.
//!
//! # What Codex is trusted for
//!
//! Grade B, and barely. `codex exec --json` is a one-shot child process: it
//! prints JSON Lines to stdout and exits. There is a stable native session id in
//! its launch acknowledgement, a live output channel, a process to kill, and a
//! process to look for. That is enough to prepare a workspace, launch, follow the
//! output, cancel and inspect.
//!
//! # What it is not
//!
//! There is no session server, so there is no durable inventory to discover, no
//! session to adopt, no transcript to page and no way to send a second message or
//! resume a finished turn. Each of those is declared unsupported and fails
//! *before* a process is started. None is filled in with a guess.
//!
//! And there is no verdict. A Codex process that finished its work, one that
//! crashed, one that hit a deadline and one that Kontor killed are
//! indistinguishable from out here — an exit status is a fact about a process,
//! not about the work. So **every** ending is advisory, this adapter can never
//! emit `Succeeded`, `Failed` or `Cancelled` as an observed state, and
//! `terminal_evidence()` returns `None` for everything it produces. Closing a
//! Codex run is a decision the control plane takes on its own evidence, never
//! one this adapter hands it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_accounts::{
    AccountResolver, AdmittedLaunch, AvailabilityObservation, LaunchAdmissionRequest,
    LaunchRefusal, ResolvedAccountEnvironment, admit_pinned_launch,
};
use kontor_core::compaction::{CompactionReceipt, CompactionStatus};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, ContentHash,
    CredentialAlias, ExternalId, ExternalName, ProjectId, RealmId, RuntimeBindingId,
    RuntimeKindKey, SCHEMA_VERSION, SchemaVersion, TeamRunId, Timestamp,
};
use kontor_core::repository::{ProjectRepository, RunRepository, RuntimeBinding};
use kontor_core::state::{NativeRuntimeIdentity, ObservedRunState, RuntimeContact};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::adapter::{
    LaunchOutcome, MessageAck, PermissionAck, RuntimeAdapter, RuntimeError, RuntimeResult,
};
use kontor_runtime::admission::{
    AdmissionLedger, AdmissionOutcome, AdmissionRequest, ClaimedSeat, OccupiedSeat, SeatFacts,
};
use kontor_runtime::capability::{
    IssuedBinding, IssuedBindingRegistry, LimitDemand, OperationContext, RuntimeBindingSnapshot,
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade, preflight,
};
use kontor_runtime::observation::{
    ControlPlaneObservation, CorrelationEvidence, NativeSession, ObservationSource,
    ReconciliationFinding, ReconciliationReport,
};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, CompactRequest, HistoryRequest, InspectRequest, LaunchRequest,
    LiveSubscribeRequest, PermissionResponseRequest, ResumeRequest, SendMessageRequest,
    capability_document,
};
use kontor_runtime::timeline::{
    EventSubject, HistoryPage, LiveSubscription, SessionEvent, TimelineGuard, TimelinePosition,
};
use kontor_runtime::workspace::{
    WorkspaceBinding, WorkspaceBindingSnapshot, WorkspaceCorrelationEvidence, WorkspaceOutcome,
    WorkspacePrepareRequest, WorkspaceRoot,
};
use serde::{Deserialize, Serialize};

use crate::client::{CodexCommand, CodexDrained, CodexTransport, PreparedCommand};
use crate::wire::{
    CODEX_EXEC_SCHEMA, CODEX_HOME, CodexEnding, CodexFrame, CodexHomeMarker, KONTOR_RUN_ENV,
    MARKER_FILE_NAME, MAX_FRAMES_PER_DRAIN, REDACTED,
};

/// The operations a one-shot `codex exec --json` process can actually prove.
///
/// Each one is a claim about the *process*, and every claim is bounded by what a
/// pipe and a pid can support: a workspace this adapter verified itself, a
/// process it started, the stdout it is reading, a kill it can send, and a
/// liveness read it can take.
const SUPPORTED: &[RuntimeCapability] = &[
    RuntimeCapability::PrepareWorkspace,
    RuntimeCapability::Launch,
    RuntimeCapability::Cancel,
    RuntimeCapability::Inspect,
    RuntimeCapability::LiveEvents,
    // Codex takes the auto-compaction trigger and its scope as startup
    // configuration, so this is the one runtime in the fleet whose context
    // policy is actually enforced rather than merely recorded.
    RuntimeCapability::ContextPolicy,
];

/// The operations it cannot, each with the reason it is refused.
///
/// This table is the adapter's public admission of what it does not know, and the
/// contract suite walks it: every entry owes a typed refusal issued before any
/// process is touched. Filling one of these in with a plausible answer — an empty
/// history page, a synthetic permission state, an acknowledgement for a message
/// nothing delivered — is the failure mode this whole design is arranged against.
pub const UNSUPPORTED: &[(RuntimeCapability, &str)] = &[
    (
        RuntimeCapability::Discovery,
        "codex exec keeps no durable session inventory: a process that has exited leaves \
         nothing to enumerate, so Kontor can never claim to know what Codex is running",
    ),
    (
        RuntimeCapability::Resume,
        "codex exec is one shot. A second invocation is a second process with a second \
         session, which is a new launch and must be admitted as one",
    ),
    (
        RuntimeCapability::SendMessage,
        "codex exec takes its whole instruction in argv and reads no conversational stdin, \
         so there is no channel a follow-up could be delivered on",
    ),
    (
        RuntimeCapability::Adopt,
        "a Codex process Kontor did not start carries no Kontor label and cannot be given \
         one, so it can never be proven to belong to a run",
    ),
    (
        RuntimeCapability::History,
        "codex exec replays nothing: its stdout is live output, and a process that has \
         ended has no transcript surface to page",
    ),
    (
        RuntimeCapability::PermissionResponse,
        "codex exec exposes no structured permission request or response surface, and \
         writing into a process's stdin could answer a prompt other than the intended one",
    ),
];

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// One configured Codex execution plane: one executable, one host, one task
/// worktree.
///
/// Scoped to a task worktree rather than to a machine because the whole
/// workspace guarantee this adapter can make is "the process ran in the directory
/// Kontor verified". A second task is a second adapter.
///
/// `Debug` is written out because two of these fields are filesystem paths, and
/// this value is reachable from [`CodexAdapter`]'s own rendering — so a derived
/// one would put an operator's directory layout into any log line that printed
/// the adapter.
#[derive(Clone, PartialEq, Eq)]
pub struct CodexConfig {
    /// The Kontor runtime-kind key, e.g. `codex.exec`.
    pub runtime_kind: RuntimeKindKey,
    /// The **non-secret** host key. No endpoint and no credential appear here, in
    /// a checkpoint, or in a binding.
    pub host_key: ExternalName,
    /// The `codex` executable to dispatch. Trusted local configuration, never a
    /// value that arrives with a request.
    pub executable: String,
    /// The one task worktree this plane prepares and works in.
    pub task_worktree: WorkspaceRoot,
    /// The most Codex processes Kontor will hold open on this plane at once.
    pub max_concurrent_sessions: u32,
}

impl std::fmt::Debug for CodexConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexConfig")
            .field("runtime_kind", &self.runtime_kind)
            .field("host_key", &self.host_key)
            .field("max_concurrent_sessions", &self.max_concurrent_sessions)
            .field("executable", &REDACTED)
            .field("task_worktree", &REDACTED)
            .finish()
    }
}

impl CodexConfig {
    /// The capabilities every binding on this plane freezes.
    #[must_use]
    pub fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            // Grade B, and the incompleteness is explicit: a Codex run's replay
            // is exactly the output that was drained while the process lived.
            // Nothing re-reads it, so an event stream can never be proof — only a
            // fresh inspect could be, and `inspect` on this adapter is careful
            // never to report a terminal state at all.
            trust_grade: TrustGrade::B,
            supported: SUPPORTED.iter().copied().collect(),
            // The whole reason this adapter exists. Codex binds its identity to
            // `CODEX_HOME`, so a per-run coding account is provable here in a way
            // it is not on any other runtime in the fleet.
            account_env: true,
            limits: RuntimeLimits {
                // No follow-up channel exists, so no body size can be honored.
                // `SendMessage` is refused before this is ever consulted.
                max_message_bytes: 0,
                // No replay surface exists, and `History` is likewise refused
                // first.
                max_history_page: 0,
                max_concurrent_sessions: self.max_concurrent_sessions,
                context_window: kontor_core::spec::ContextWindowBounds::unknown(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The account seam
// ---------------------------------------------------------------------------

/// One account-pinned launch, as the adapter asks about it.
#[derive(Debug, Clone, Copy)]
pub struct CodexAccountRequest<'a> {
    /// The run being launched.
    pub agent_run_id: AgentRunId,
    /// The account the launch claims the run is pinned to. It is only ever a
    /// claim: the authority compares it against the run's stored pin.
    pub account_profile_id: AccountProfileId,
    /// What this runtime declares, so the account gate is the runtime's own
    /// preflight rather than a second copy of it.
    pub capabilities: &'a RuntimeCapabilities,
    /// The decision instant.
    pub now: Timestamp,
}

/// An admitted account-pinned launch.
#[derive(Debug)]
pub struct CodexAccountAdmission {
    /// KON-MVP-07's own decision: the non-secret receipt and the short-lived
    /// environment, which is dropped and zeroized with this value.
    pub admitted: AdmittedLaunch,
    /// The approved alias the credential resolved through.
    ///
    /// A name, and the whole of what a Kontor receipt may record about *where* an
    /// account's credentials live. It is immutable for the life of a profile, so
    /// reading it is not a second decision.
    pub credential_alias: CredentialAlias,
}

/// The seam between this adapter and account admission.
///
/// It exists because [`RuntimeAdapter::launch`] takes a
/// [`LaunchRequest`] and nothing else — no store, no resolver, no fleet
/// observation — while the account decision needs all three. Putting the seam
/// here rather than widening the shared trait keeps every other runtime out of a
/// question only this one can answer.
///
/// The production implementation is [`CodexPinnedAccounts`], which is a thin
/// wrapper over [`admit_pinned_launch`]. There is deliberately no second
/// credential path: this adapter resolves nothing itself.
pub trait CodexAccountAuthority: Send + Sync {
    /// Judge one account-pinned launch, resolving only once everything else
    /// agrees.
    ///
    /// # Errors
    /// Returns [`LaunchRefusal`] for an unpinned or mismatched run, a disabled or
    /// unknown profile, an unavailable or stale account, a runtime that cannot
    /// prove an account environment, an unapproved reference, and a profile that
    /// moved during resolution.
    fn admit(
        &self,
        request: &CodexAccountRequest<'_>,
    ) -> Result<CodexAccountAdmission, LaunchRefusal>;
}

/// The production authority: KON-MVP-07's admission, wired to a store, a
/// resolver policy and the fleet's availability boundary.
///
/// It adds no policy of its own. Every ordering rule that matters — the pin is
/// the run's, the profile must be enabled, availability must be fresh, the
/// runtime's own preflight decides the account gate, nothing is resolved until
/// all of that agrees, and the profile is re-read afterwards — lives in
/// [`admit_pinned_launch`] and is proved by KON-MVP-07's own suite.
pub struct CodexPinnedAccounts<'a, S> {
    store: &'a S,
    resolver: &'a AccountResolver<'a>,
    availability:
        &'a (dyn Fn(AccountProfileId, Timestamp) -> AvailabilityObservation + Send + Sync),
    realm_id: RealmId,
    project_id: ProjectId,
}

impl<'a, S> CodexPinnedAccounts<'a, S> {
    /// Wire one authority.
    ///
    /// `availability` is the fleet boundary: `asma fleet` owns cooldown
    /// mechanics, and this adapter never goes looking for them on disk.
    #[must_use]
    pub const fn new(
        store: &'a S,
        resolver: &'a AccountResolver<'a>,
        availability: &'a (
                dyn Fn(AccountProfileId, Timestamp) -> AvailabilityObservation + Send + Sync
            ),
        realm_id: RealmId,
        project_id: ProjectId,
    ) -> Self {
        Self {
            store,
            resolver,
            availability,
            realm_id,
            project_id,
        }
    }
}

impl<S> std::fmt::Debug for CodexPinnedAccounts<'_, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The resolver's policy holds approved paths and keychain targets, so
        // nothing here prints more than the two ids that are already public
        // Kontor identifiers.
        f.debug_struct("CodexPinnedAccounts")
            .field("realm_id", &self.realm_id)
            .field("project_id", &self.project_id)
            .finish_non_exhaustive()
    }
}

impl<S> CodexAccountAuthority for CodexPinnedAccounts<'_, S>
where
    S: ProjectRepository + RunRepository + Send + Sync,
{
    fn admit(
        &self,
        request: &CodexAccountRequest<'_>,
    ) -> Result<CodexAccountAdmission, LaunchRefusal> {
        let observation = (self.availability)(request.account_profile_id, request.now);
        let admitted = admit_pinned_launch(
            self.store,
            self.resolver,
            &LaunchAdmissionRequest {
                realm_id: self.realm_id,
                project_id: self.project_id,
                agent_run_id: request.agent_run_id,
                account_profile_id: request.account_profile_id,
                observation: &observation,
                capabilities: request.capabilities,
                now: request.now,
            },
        )?;
        // Read *after* admission, and safe to: a profile's credential reference
        // is immutable for its lifetime — rotating one is a new profile id — so
        // this cannot disagree with the reference the decision above resolved.
        let profile = self
            .store
            .get_account_profile(self.project_id, request.account_profile_id)?
            .ok_or(LaunchRefusal::ProfileNotFound)?;
        Ok(CodexAccountAdmission {
            admitted,
            credential_alias: profile.credential_ref.alias,
        })
    }
}

/// Translate an account refusal into the shared runtime vocabulary.
///
/// Every arm carries a static rule and never a resolved value, a path, a
/// keychain target or a profile label, so a refusal is as safe to log as any
/// other. `LaunchRefusal` is `#[non_exhaustive]`, and the catch-all is
/// deliberately the *least* informative arm rather than the most: a variant this
/// binary has not seen is a refusal it does not understand, and describing it
/// would be inventing the description.
fn account_refusal(refusal: &LaunchRefusal) -> RuntimeError {
    let rule = match refusal {
        // The runtime's own gate already spoke; hand its verdict straight back
        // rather than re-wrapping it as a domain error.
        LaunchRefusal::Runtime(error) => return error.clone(),
        LaunchRefusal::RunNotFound => "the run this launch names does not exist in this project",
        LaunchRefusal::ProfileNotFound => "the account profile does not exist in this project",
        LaunchRefusal::PinMismatch => {
            "the launch names an account other than the one this run is pinned to"
        }
        LaunchRefusal::ProfileDisabled => "the account profile is disabled",
        LaunchRefusal::ObservationMismatch => "the availability evidence concerns another account",
        LaunchRefusal::Cooling { .. } => "the account is cooling",
        LaunchRefusal::AvailabilityUnknown => "the account's availability is unknown",
        LaunchRefusal::AvailabilityStale => "the account's availability evidence is not fresh",
        LaunchRefusal::Resolution(_) => {
            "the account's credential reference is not one the resolver policy approves"
        }
        LaunchRefusal::ProfileMovedDuringResolution => {
            "the account profile changed while its credentials were being resolved"
        }
        LaunchRefusal::Repository(_) => "the account store refused the read",
        _ => "the account was refused for a reason this adapter does not recognize",
    };
    RuntimeError::Domain(DomainError::invalid("CodexAccountAdmission", rule))
}

// ---------------------------------------------------------------------------
// The account receipt
// ---------------------------------------------------------------------------

/// What this adapter durably records about *which account* a Codex run executed
/// as.
///
/// Every field is a name, an identifier or a digest of something that is
/// non-secret by construction. Note carefully what is **not** here, and why:
///
/// * the config-home path — it identifies where an account's credentials live on
///   an operator's machine, and a receipt is a durable, exportable artefact;
/// * the environment value the child received, for the same reason;
/// * anything read out of `auth.json`, a token file, a cookie or a keychain —
///   this adapter never opens any of them;
/// * a digest of any of the above, because a digest of a secret is still a fact
///   about that secret.
///
/// `marker_digest` is the one hash of file content, and it is admissible for the
/// exact reason the others are not: [`CodexHomeMarker`] denies unknown fields, so
/// the bytes it digests provably contain nothing but a schema version, a profile
/// id and a non-secret provider identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexAccountReceipt {
    /// The envelope contract this receipt was written under.
    pub schema_version: SchemaVersion,
    /// The account the run executed as.
    pub account_profile_id: AccountProfileId,
    /// The profile revision the decision was taken against.
    pub account_profile_revision: AggregateRevision,
    /// The non-secret provider identity both the profile and the home's marker
    /// agreed on.
    pub provider_identity: ExternalId,
    /// The approved alias the credential resolved through. A name, never a place.
    pub credential_alias: CredentialAlias,
    /// The marker contract the home was verified under.
    pub marker_schema_version: u32,
    /// A digest of the home's non-secret marker.
    pub marker_digest: ContentHash,
    /// KON-MVP-07's digest of the resolver policy's approved *names*.
    pub policy_evidence: ContentHash,
    /// KON-MVP-07's digest of the capabilities the runtime declared.
    pub capability_evidence: ContentHash,
    /// When the home was verified.
    pub verified_at: Timestamp,
}

impl CodexAccountReceipt {
    /// Canonicalize the receipt, so it can travel inside an existing command
    /// receipt's safe intent.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the receipt does not canonicalize, which
    /// includes [`DomainError::SensitiveMaterial`] if any field ever came to hold
    /// credential-looking text.
    pub fn to_document(&self) -> DomainResult<CanonicalDocument> {
        CanonicalDocument::from_serializable(self)
    }
}

// ---------------------------------------------------------------------------
// Process evidence and the checkpoint
// ---------------------------------------------------------------------------

/// One Codex process this adapter started, as it survives a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecutionRecord {
    /// The binding whose session this process is.
    pub binding_id: RuntimeBindingId,
    /// The transport handle that addresses it.
    pub exec_id: ExternalId,
    /// The operating-system process identity a cancellation addresses.
    pub process_id: u32,
    /// The adapter generation the handle belongs to. A handle from an older
    /// generation names a process this instance does not own.
    pub generation: u64,
    /// When it was started.
    pub started_at: Timestamp,
    /// How it ended, once it has. Never a verdict — see [`CodexEnding`].
    pub ending: Option<CodexEnding>,
}

/// Everything the adapter needs to be rebuilt after a Kontor restart.
///
/// The adapter defines no storage interface and opens no database. This is a
/// plain value the existing KON-MVP-03 binding, runtime-event and command-receipt
/// tables already hold; the constructor takes it back.
///
/// It carries no config-home path, no environment value, no prompt and no marker
/// text — only names, ids and digests — so a checkpoint scan is one of the places
/// the account-isolation claim is checked.
///
/// One field does hold a path even so: a [`WorkspaceBindingSnapshot`] names the
/// worktree root, and that type is the shared crate's with a derived `Debug`. So
/// this type's `Debug` is written out and renders the workspaces by count and
/// binding id — a checkpoint is exactly the sort of value that ends up in a
/// diagnostic dump.
#[derive(Clone, PartialEq, Eq)]
pub struct CodexCheckpoint {
    /// The adapter generation these bindings belong to.
    pub generation: u64,
    /// Every binding the adapter issued and has not invalidated.
    pub bindings: Vec<RuntimeBindingSnapshot>,
    /// The verified task workspaces, keyed by the team run that owns them.
    pub workspaces: Vec<WorkspaceBindingSnapshot>,
    /// Every team-run seat holding one of those sessions.
    pub seats: Vec<OccupiedSeat>,
    /// Every seat whose launch was in flight when this was taken.
    pub claims: Vec<ClaimedSeat>,
    /// The account receipt for each binding.
    pub receipts: Vec<(RuntimeBindingId, CodexAccountReceipt)>,
    /// The processes those bindings name.
    pub executions: Vec<CodexExecutionRecord>,
    /// The last content position delivered per binding, so a restart continues
    /// the numbering rather than restarting it.
    pub positions: Vec<(RuntimeBindingId, u64)>,
}

impl std::fmt::Debug for CodexCheckpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexCheckpoint")
            .field("generation", &self.generation)
            .field("bindings", &self.bindings)
            .field("workspaces", &self.workspaces.len())
            .field(
                "workspace_binding_ids",
                &self
                    .workspaces
                    .iter()
                    .map(WorkspaceBindingSnapshot::binding_id)
                    .collect::<Vec<_>>(),
            )
            .field("workspace_roots", &REDACTED)
            .field("seats", &self.seats)
            .field("claims", &self.claims)
            .field("receipts", &self.receipts)
            .field("executions", &self.executions)
            .field("positions", &self.positions)
            .finish()
    }
}

impl CodexCheckpoint {
    /// A fresh plane with no history, in `generation`.
    #[must_use]
    pub const fn fresh(generation: u64) -> Self {
        Self {
            generation,
            bindings: Vec::new(),
            workspaces: Vec::new(),
            seats: Vec::new(),
            claims: Vec::new(),
            receipts: Vec::new(),
            executions: Vec::new(),
            positions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// The adapter's own mutable state.
///
/// Private, and its `Debug` is written out anyway: `workspaces` holds the same
/// path-bearing shared snapshots the checkpoint does, and a derived rendering
/// here would be one `dbg!` away from being the leak the rest of this module is
/// arranged to prevent.
struct CodexState {
    generation: u64,
    bindings: IssuedBindingRegistry,
    /// The seat rule, from the shared ledger rather than restated here. Every
    /// read and write of it happens under this adapter's one state lock, which is
    /// what makes "check the seat, then claim it" a single step.
    admissions: AdmissionLedger,
    workspaces: BTreeMap<TeamRunId, WorkspaceBindingSnapshot>,
    receipts: BTreeMap<RuntimeBindingId, CodexAccountReceipt>,
    executions: BTreeMap<RuntimeBindingId, CodexExecutionRecord>,
    /// The shared continuity policy, one guard per binding, rather than a second
    /// copy of it: a skipped frame has to break this stream permanently for the
    /// same reason it does everywhere else.
    guards: BTreeMap<RuntimeBindingId, TimelineGuard>,
}

impl std::fmt::Debug for CodexState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexState")
            .field("generation", &self.generation)
            .field("bindings", &self.bindings.len())
            .field("workspaces", &self.workspaces.len())
            .field("receipts", &self.receipts.len())
            .field("executions", &self.executions.len())
            .field("guards", &self.guards.len())
            .field("workspace_roots", &REDACTED)
            .finish_non_exhaustive()
    }
}

/// This adapter's answers to the two questions the shared ledger cannot answer.
struct CodexSeatFacts<'a> {
    bindings: &'a IssuedBindingRegistry,
    executions: &'a BTreeMap<RuntimeBindingId, CodexExecutionRecord>,
    generation: u64,
}

impl SeatFacts for CodexSeatFacts<'_> {
    fn issued_binding(&self, binding_id: RuntimeBindingId) -> Option<RuntimeBindingSnapshot> {
        self.bindings.get(binding_id).cloned()
    }

    /// What this adapter can prove synchronously, which is that a process
    /// **stopped** — never that the work finished.
    ///
    /// The distinction is the whole ticket. A seat may be refilled once the
    /// process holding it is gone, because two Codex processes editing one
    /// worktree is the concrete harm AC-4 exists to prevent. Whether the
    /// predecessor's *work* succeeded is a different question, it is not asked
    /// here, and nothing in this function's answer travels into a run's terminal
    /// state.
    fn holder_is_finished_or_retired(
        &self,
        binding_id: RuntimeBindingId,
        _native_id: &ExternalId,
    ) -> bool {
        match self.bindings.get(binding_id) {
            // A binding this adapter no longer holds is retired, and needs no
            // request to say so.
            None => true,
            Some(snapshot) if snapshot.identity().generation != self.generation => true,
            Some(_) => self
                .executions
                .get(&binding_id)
                .is_some_and(|execution| execution.ending.is_some()),
        }
    }
}

/// The narrow direct Codex adapter for one task worktree.
/// The hermetic app-server lane that proves the `thread/compact/start` mapping.
///
/// **Not a production path.** [`CodexAdapter::new`] leaves it absent, so a
/// production adapter advertises no [`RuntimeCapability::Compact`] and its
/// `compact` reports `unsupported` having touched nothing. Only
/// [`CodexAdapter::with_app_server`] — which no production construction calls —
/// supplies one, and only a fake app-server ever implements it.
///
/// It is deliberately two methods rather than a JSON-RPC client: send the one
/// request, and re-read the one thread. Everything else the approved mapping
/// needs is already in the shared contract.
#[async_trait::async_trait]
pub trait CodexAppServer: Send + Sync {
    /// Start compaction on one thread and return the lifecycle it emitted.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the app-server cannot be
    /// reached.
    async fn compact_thread(
        &self,
        request: &crate::wire::ThreadCompactStart,
    ) -> RuntimeResult<Vec<crate::wire::ThreadCompactEvent>>;

    /// Re-read one thread's identity after the lifecycle finished.
    ///
    /// This is the evidence a confirmation rests on, and it is a *fresh read*
    /// rather than an echo of the request: a lifecycle that says "completed" is
    /// an acknowledgement, and an acknowledgement is not proof the session
    /// survived.
    ///
    /// # Errors
    /// As [`CodexAppServer::compact_thread`].
    async fn inspect_thread(&self, thread_id: &str) -> RuntimeResult<crate::wire::ThreadIdentity>;
}

/// The Codex runtime, reduced to what Kontor is willing to depend on.
///
/// One account-pinned `codex exec --json` process per run, over a transport the
/// caller supplies. The hermetic app-server lane is optional and absent in
/// production; see [`CodexAppServer`].
pub struct CodexAdapter<'a> {
    config: CodexConfig,
    transport: Box<dyn CodexTransport>,
    accounts: Box<dyn CodexAccountAuthority + 'a>,
    /// Absent in production. See [`CodexAppServer`].
    app_server: Option<std::sync::Arc<dyn CodexAppServer>>,
    state: Mutex<CodexState>,
}

impl std::fmt::Debug for CodexAdapter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written rather than derived, and it prints the configuration and
        // the generation only. A derived rendering would reach the account
        // authority and — through it — a resolver policy holding approved config
        // homes and keychain targets.
        f.debug_struct("CodexAdapter")
            .field("config", &self.config)
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}

impl<'a> CodexAdapter<'a> {
    /// What this adapter can prove, including the hermetic app-server lane when
    /// one was supplied.
    ///
    /// Production supplies none, so `Compact` is absent there — capability
    /// discovery advertises only what the constructed transport can attest.
    #[must_use]
    pub fn declared_capabilities(&self) -> RuntimeCapabilities {
        let mut declared = self.config.capabilities();
        if self.app_server.is_some() {
            declared.supported.insert(RuntimeCapability::Compact);
        }
        declared
    }

    /// Attach a hermetic app-server lane. Test and fixture use only; see
    /// [`CodexAppServer`].
    #[must_use]
    pub fn with_app_server(mut self, app_server: std::sync::Arc<dyn CodexAppServer>) -> Self {
        self.app_server = Some(app_server);
        self
    }

    /// Build an adapter for `config` over `transport`, rehydrated from
    /// `checkpoint`.
    #[must_use]
    pub fn new(
        config: CodexConfig,
        transport: Box<dyn CodexTransport>,
        accounts: Box<dyn CodexAccountAuthority + 'a>,
        checkpoint: CodexCheckpoint,
    ) -> Self {
        let positions: BTreeMap<RuntimeBindingId, u64> =
            checkpoint.positions.iter().copied().collect();
        Self {
            config,
            transport,
            accounts,
            // Production has no app-server lane, so it advertises no `Compact`.
            app_server: None,
            state: Mutex::new(CodexState {
                generation: checkpoint.generation,
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
                    // evidenced one, and it still refuses every second launch.
                    for claim in checkpoint.claims.iter().cloned() {
                        ledger.restore_claimed(claim);
                    }
                    for seat in checkpoint.seats.iter().cloned() {
                        ledger.restore_occupied(seat);
                    }
                    ledger
                },
                workspaces: checkpoint
                    .workspaces
                    .iter()
                    .map(|snapshot| (snapshot.binding.team_run_id, snapshot.clone()))
                    .collect(),
                receipts: checkpoint.receipts.iter().cloned().collect(),
                executions: checkpoint
                    .executions
                    .iter()
                    .map(|record| (record.binding_id, record.clone()))
                    .collect(),
                guards: checkpoint
                    .bindings
                    .iter()
                    .map(|snapshot| {
                        let sequence = positions
                            .get(&snapshot.binding_id())
                            .copied()
                            .unwrap_or_default();
                        (
                            snapshot.binding_id(),
                            TimelineGuard::starting_after(TimelinePosition {
                                epoch: checkpoint.generation,
                                sequence,
                            }),
                        )
                    })
                    .collect(),
            }),
        }
    }

    /// The plane this adapter drives.
    #[must_use]
    pub const fn config(&self) -> &CodexConfig {
        &self.config
    }

    /// The current adapter generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// The account receipt one binding was launched under.
    #[must_use]
    pub fn account_receipt(&self, binding_id: RuntimeBindingId) -> Option<CodexAccountReceipt> {
        self.lock().receipts.get(&binding_id).cloned()
    }

    /// The persistable state, for the existing KON-MVP-03 tables.
    #[must_use]
    pub fn checkpoint(&self) -> CodexCheckpoint {
        let state = self.lock();
        CodexCheckpoint {
            generation: state.generation,
            bindings: state.bindings.snapshots().cloned().collect(),
            workspaces: state.workspaces.values().cloned().collect(),
            seats: state.admissions.occupied_seats().collect(),
            claims: state.admissions.claimed_seats().collect(),
            receipts: state
                .receipts
                .iter()
                .map(|(id, receipt)| (*id, receipt.clone()))
                .collect(),
            executions: state.executions.values().cloned().collect(),
            positions: state
                .guards
                .iter()
                .map(|(id, guard)| (*id, guard.position().sequence))
                .collect(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CodexState> {
        self.state.lock().expect("the Codex adapter lock is intact")
    }

    fn identity(&self, native_id: ExternalId, generation: u64) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: self.config.runtime_kind.clone(),
            host: self.config.host_key.clone(),
            generation,
            native_id,
        }
    }

    /// Refuse an operation Codex cannot perform, before anything is dispatched.
    ///
    /// Routing every unsupported method through the shared preflight — rather
    /// than returning the error directly — is what makes the refusal the *same*
    /// refusal the control plane gets from any other adapter, and what keeps the
    /// declared capability set and the behavior from disagreeing.
    fn refuse_unsupported(&self, capability: RuntimeCapability) -> RuntimeError {
        preflight(
            &self.declared_capabilities(),
            &OperationContext::new(capability),
        )
        .expect_err("this capability is declared unsupported")
    }

    /// Resolve a presented binding to the adapter's **own** copy, before any
    /// effect.
    ///
    /// A [`RuntimeBindingSnapshot`] is a plain value with public fields, so a
    /// self-consistent one costs nothing to fabricate and `preflight` cannot
    /// catch it — it checks a snapshot against itself. Only the registry knows
    /// what this adapter actually issued, and the transport handle every
    /// operation addresses comes from the registry's copy, so a doctored snapshot
    /// cannot redirect a kill into another account's process.
    ///
    /// # Errors
    /// Returns [`RuntimeError::StaleBinding`] for a binding this adapter never
    /// issued and for one that differs in any field from what it issued.
    fn attested(&self, claimed: &RuntimeBindingSnapshot) -> RuntimeResult<RuntimeBindingSnapshot> {
        self.lock()
            .bindings
            .attest(claimed)
            .map(|issued| issued.snapshot().clone())
    }

    /// The transport handle one attested binding addresses.
    fn execution(&self, binding_id: RuntimeBindingId) -> RuntimeResult<CodexExecutionRecord> {
        let state = self.lock();
        let record =
            state
                .executions
                .get(&binding_id)
                .cloned()
                .ok_or(RuntimeError::StaleBinding {
                    rule: "this binding names no process this adapter started",
                })?;
        if record.generation != state.generation {
            return Err(RuntimeError::StaleBinding {
                rule: "the adapter generation changed since this process was started",
            });
        }
        Ok(record)
    }

    // -- Account isolation --------------------------------------------------

    /// Build the child's command and prove the config home it will run under.
    ///
    /// This is where steps 4 and 5 of the admission order happen, and it happens
    /// *before* the seat is claimed and long before a process exists — so a home
    /// that cannot be proven costs no process, no seat and no edit.
    ///
    /// The environment is applied to the real command and then read back off it.
    /// Reading a copy would verify something other than what the child receives,
    /// which is the one thing this check is for.
    fn prepare_invocation(
        &self,
        request: &LaunchRequest,
        environment: &ResolvedAccountEnvironment,
    ) -> RuntimeResult<(CodexCommand, PreparedCommand, CodexHomeMarker, ContentHash)> {
        // Exactly one variable, and it is the config home. A profile that also
        // injects an API key or a proxy variable is not the isolation model this
        // adapter can prove, so it is refused rather than partly honored.
        let names: Vec<String> = environment
            .names()
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect();
        if names != [CODEX_HOME.to_owned()] {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexAccountEnvironment",
                "must resolve exactly one variable, and it must be the config home",
            )));
        }

        let label = request.correlation().to_string();
        // The frozen effective policy decides the startup configuration. It was
        // resolved and hashed before this call, so what the child is configured
        // with is exactly what the run's record says was asked for.
        let command = CodexCommand::exec_with_config(
            &self.config.executable,
            request.cwd().as_str(),
            request.prompt().as_str(),
            names,
            &crate::wire::auto_compact_config(&request.context_policy().effective),
        );
        command.ensure_dispatchable()?;

        let mut process = std::process::Command::new(&self.config.executable);
        process.args(command.argv());
        process.current_dir(command.cwd());
        // Cleared before anything is written, so a `CODEX_HOME` in Kontor's own
        // environment can never be inherited by a run pinned to another account.
        // An ambient home is not a fallback; it is the failure this adapter
        // exists to make impossible.
        process.env_remove(CODEX_HOME);
        process.env(KONTOR_RUN_ENV, &label);
        // The one sanctioned exit from a resolved credential. It writes into this
        // child's environment block and nowhere else.
        environment.apply(&mut process);
        let prepared = PreparedCommand::new(process);

        let Some(home) = prepared.env_value(CODEX_HOME) else {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexAccountEnvironment",
                "resolved no config home for this account",
            )));
        };
        if home.is_empty() || !std::path::Path::new(&home).is_absolute() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexAccountEnvironment",
                "resolved a config home that is not an absolute directory",
            )));
        }

        let (marker, digest) = Self::read_marker(&home)?;
        Ok((command, prepared, marker, digest))
    }

    /// Read the operator's non-secret marker out of one approved config home.
    ///
    /// This is the *only* file this adapter reads inside a config home. It never
    /// opens `auth.json`, a token file, a cookie jar or a keychain entry, and it
    /// never digests one either: the coding client stays the sole reader of its
    /// own credentials.
    fn read_marker(home: &str) -> RuntimeResult<(CodexHomeMarker, ContentHash)> {
        let path = std::path::Path::new(home).join(MARKER_FILE_NAME);
        // The error is dropped unread: a `std::io::Error`'s text names the path
        // it failed on, which is the config home this refusal must not carry.
        let raw = std::fs::read_to_string(&path).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                "CodexHomeMarker",
                "the approved config home carries no readable Kontor profile marker",
            ))
        })?;
        let marker = CodexHomeMarker::parse(&raw)?;
        Ok((marker, ContentHash::of(raw.as_bytes())))
    }

    /// Prove the resolved home is this account's home, and record why.
    ///
    /// Both directions are checked. The profile says which account the run is
    /// pinned to; the marker says which account the *directory* belongs to. Either
    /// alone would be satisfied by a policy that mapped two aliases to one home,
    /// which is exactly how two runs end up sharing one identity.
    fn verified_receipt(
        admission: &CodexAccountAdmission,
        marker: &CodexHomeMarker,
        marker_digest: ContentHash,
        verified_at: Timestamp,
    ) -> RuntimeResult<CodexAccountReceipt> {
        let receipt = &admission.admitted.receipt;
        if marker.account_profile_id != receipt.account_profile_id {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexHomeMarker",
                "the approved config home belongs to another account profile",
            )));
        }
        let Some(expected) = receipt.provider_identity.clone() else {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexAccountAdmission",
                "the account profile records no provider identity to verify the home against",
            )));
        };
        if marker.provider_identity != expected {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexHomeMarker",
                "the approved config home reports another provider identity",
            )));
        }
        Ok(CodexAccountReceipt {
            schema_version: SCHEMA_VERSION,
            account_profile_id: receipt.account_profile_id,
            account_profile_revision: receipt.account_profile_revision,
            provider_identity: expected,
            credential_alias: admission.credential_alias.clone(),
            marker_schema_version: marker.schema_version,
            marker_digest,
            policy_evidence: receipt.policy_evidence.clone(),
            capability_evidence: receipt.capability_evidence.clone(),
            verified_at,
        })
    }

    // -- Evidence -----------------------------------------------------------

    /// Canonicalize a raw Codex frame, unmodified, with the values the mapping
    /// read beside it.
    ///
    /// The raw line goes in first and verbatim: KON-MVP-03 persists evidence
    /// before any normalized consequence is applied, so a mapping that later turns
    /// out to be wrong can be re-derived from what Codex actually printed rather
    /// than from what this adapter concluded.
    fn frame_evidence(raw: &str, frame: &CodexFrame) -> DomainResult<CanonicalDocument> {
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "codex_schema": CODEX_EXEC_SCHEMA,
            "raw": raw,
            "raw_digest": ContentHash::of(raw.as_bytes()).as_str(),
            "read": {
                "frame_id": frame.id,
                "frame_type": frame.msg.kind,
            },
        }))
    }

    /// Canonicalize what a process did, without saying what it meant.
    fn process_evidence(
        record: &CodexExecutionRecord,
        ending: Option<CodexEnding>,
    ) -> DomainResult<CanonicalDocument> {
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "codex_schema": CODEX_EXEC_SCHEMA,
            "process_id": record.process_id,
            "exec_id": record.exec_id.as_str(),
            "generation": record.generation,
            "ending": ending.map(CodexEnding::as_str),
            // Recorded because an operator will want it, and read by nothing:
            // there is no branch in this adapter on a status code.
            "reported_code": ending.and_then(CodexEnding::reported_code),
        }))
    }

    /// The observation a process ending produces, and the one rule about it.
    ///
    /// A process that stopped is a fact about a channel. It is reported as
    /// [`ObservedRunState::Unknown`] from an [`ObservationSource::AdvisoryReport`],
    /// which is doubly refused as terminal evidence: `Unknown` has no terminal
    /// outcome to offer, and an advisory report may not close a run at any trust
    /// grade. Either alone would be enough; both are here because this is the one
    /// conclusion nothing walks back.
    fn ending_observation(
        agent_run_id: AgentRunId,
        identity: NativeRuntimeIdentity,
        record: &CodexExecutionRecord,
        ending: CodexEnding,
        observed_at: Timestamp,
    ) -> RuntimeResult<ControlPlaneObservation> {
        Ok(ControlPlaneObservation {
            agent_run_id,
            contact: RuntimeContact::ProcessMissing,
            state: ObservedRunState::Unknown,
            identity,
            native_event_id: None,
            native_sequence: 0,
            observed_at,
            evidence: Self::process_evidence(record, Some(ending))?,
            source: ObservationSource::AdvisoryReport,
        })
    }

    // -- Session content ----------------------------------------------------

    /// Turn one drain into contiguous, validated session content.
    ///
    /// Sequence numbers are the adapter's own — Codex prints no position — so the
    /// shared [`TimelineGuard`] is what keeps them honest across drains, and what
    /// makes a skipped frame break this stream permanently instead of being
    /// renumbered over.
    fn ingest(
        state: &mut CodexState,
        binding_id: RuntimeBindingId,
        drained: &CodexDrained,
        at: Timestamp,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let generation = state.generation;
        let guard = state
            .guards
            .get_mut(&binding_id)
            .ok_or(RuntimeError::StaleBinding {
                rule: "this binding has no content position in this generation",
            })?;

        if drained.lines.len() > MAX_FRAMES_PER_DRAIN {
            return Err(RuntimeError::Transport {
                rule: "answer exceeded the bounded frame count",
            });
        }

        // A frame the transport could not keep is a hole in the numbering. It is
        // fed to the guard as the gap it is, so the refusal is the shared one and
        // it sticks: every later drain on this binding refuses too, and the caller
        // is told to refetch rather than handed renumbered content.
        if drained.dropped > 0 {
            let missed = guard
                .position()
                .sequence
                .saturating_add(drained.dropped)
                .saturating_add(1);
            let marker = SessionEvent {
                kind: kontor_runtime::timeline::SessionEventKind::Log,
                position: TimelinePosition {
                    epoch: generation,
                    sequence: missed,
                },
                subject: EventSubject::None,
                native_event_id: None,
                emitted_at: at,
                payload: CanonicalDocument::from_value(&serde_json::json!({
                    "schema_version": 1,
                    "codex_schema": CODEX_EXEC_SCHEMA,
                    "dropped": drained.dropped,
                }))?,
            };
            return Err(guard
                .accept(&marker)
                .expect_err("a skipped frame is a gap in the numbering"));
        }

        let mut events = Vec::with_capacity(drained.lines.len());
        for raw in &drained.lines {
            let frame = CodexFrame::parse(raw)?;
            let sequence = guard.position().sequence.saturating_add(1);
            let event = SessionEvent {
                kind: frame.event_kind(),
                position: TimelinePosition {
                    epoch: generation,
                    sequence,
                },
                subject: EventSubject::None,
                native_event_id: ExternalId::parse(&frame.id).ok(),
                emitted_at: at,
                // Evidence first, mapping second.
                payload: Self::frame_evidence(raw, &frame)?,
            };
            guard.accept(&event)?;
            events.push(event);
        }
        Ok(events)
    }

    /// Everything a launch does once its account and its workspace have agreed
    /// to it.
    ///
    /// Separate from [`RuntimeAdapter::launch`] so one place decides what a
    /// failure costs: every `?` here is a refusal that happens after the seat was
    /// claimed, and all of them are answered by the single release at the call
    /// site.
    async fn launch_admitted(
        &self,
        request: &LaunchRequest,
        command: &CodexCommand,
        prepared: PreparedCommand,
        receipt: CodexAccountReceipt,
        generation: u64,
    ) -> RuntimeResult<LaunchOutcome> {
        // The label the child will actually receive, read back off the command
        // that is about to be spawned rather than from the value that built it.
        //
        // Be exact about what this proves. Codex echoes nothing Kontor plants, so
        // this is *positive identity* rather than reported identity: the adapter
        // built this environment, spawned this child, holds its only handle and
        // reads its stdout pipe directly, and this check catches an environment
        // that was assembled wrongly. It is not the runtime vouching for the
        // label — and that is precisely why this adapter is Grade B and why no
        // observation it produces can close a run.
        let planted = prepared.env_value(KONTOR_RUN_ENV).unwrap_or_default();

        let started = self.transport.start(command, prepared).await?;

        // From here a process exists. Every refusal below has to stop it, or the
        // launch leaves an unowned Codex editing the worktree.
        let outcome: RuntimeResult<LaunchOutcome> = async {
            let frame = CodexFrame::parse(&started.launch_ack)?;
            let session = frame
                .launch_ack_session()
                .ok_or(RuntimeError::CorrelationFailed)?;
            let identity = self.identity(ExternalId::parse(session)?, generation);
            let correlation = CorrelationEvidence::establish(
                request.agent_run_id(),
                &planted,
                identity.clone(),
                request.requested_at(),
            )?;
            let snapshot = RuntimeBindingSnapshot {
                binding: RuntimeBinding {
                    id: request.binding_id(),
                    agent_run_id: request.agent_run_id(),
                    identity: identity.clone(),
                    bound_at: request.requested_at(),
                },
                capabilities: self.declared_capabilities(),
                correlation,
            };
            let record = CodexExecutionRecord {
                binding_id: request.binding_id(),
                exec_id: started.exec_id.clone(),
                process_id: started.process_id,
                generation,
                started_at: request.requested_at(),
                ending: None,
            };
            let observation = ControlPlaneObservation {
                agent_run_id: request.agent_run_id(),
                contact: RuntimeContact::Reachable,
                // A process that acknowledged its launch is running. That is an
                // acknowledgement and nothing more: `CommandAck` closes no run at
                // any grade.
                state: ObservedRunState::Running,
                identity,
                native_event_id: ExternalId::parse(&frame.id).ok(),
                native_sequence: 0,
                observed_at: request.requested_at(),
                evidence: Self::frame_evidence(&started.launch_ack, &frame)?,
                source: ObservationSource::CommandAck,
            };

            // The claim becomes the session in the same critical section that
            // records the binding, so there is no instant at which this adapter
            // owns a process and its seat is still reservable.
            {
                let state = &mut *self.lock();
                state
                    .admissions
                    .occupy(request, snapshot.identity().native_id.clone())?;
                state.bindings.record(snapshot.clone());
                state.receipts.insert(request.binding_id(), receipt);
                state.executions.insert(request.binding_id(), record);
                state.guards.insert(
                    request.binding_id(),
                    TimelineGuard::starting_after(TimelinePosition::start_of(generation)),
                );
            }
            Ok(LaunchOutcome {
                snapshot,
                observation,
            })
        }
        .await;

        if outcome.is_err() {
            // Best effort, and unconditional: a process nothing is bound to is a
            // process nothing will ever cancel.
            let _ = self.transport.stop(&started.exec_id).await;
        }
        outcome
    }
}

#[async_trait]
impl RuntimeAdapter for CodexAdapter<'_> {
    /// The capability set is a fixed, audited statement about `codex exec --json`
    /// rather than something probed, and this call deliberately reaches no
    /// process at all.
    ///
    /// There is nothing to probe. Codex has no daemon, no version endpoint that
    /// does not cost a process, and no boot id — so a probe would be a spawn, and
    /// spawning to answer "what can you do?" is how a capability question becomes
    /// a side effect. The real probe is the launch.
    async fn discover_capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Ok(self.declared_capabilities())
    }

    async fn issued_binding(
        &self,
        claimed: &RuntimeBindingSnapshot,
    ) -> RuntimeResult<IssuedBinding> {
        self.lock().bindings.attest(claimed)
    }

    /// Admission is bookkeeping about seats: it starts nothing, reaches no
    /// process, and is deliberately not recorded in the dispatch ledger, so "no
    /// process was started" keeps meaning what it says.
    async fn admit_launch(&self, request: &AdmissionRequest) -> RuntimeResult<AdmissionOutcome> {
        let state = &mut *self.lock();
        let facts = CodexSeatFacts {
            bindings: &state.bindings,
            executions: &state.executions,
            generation: state.generation,
        };
        state.admissions.admit(request, &facts)
    }

    /// Verify the task worktree that already exists, and bind it.
    ///
    /// Codex has no workspace concept of its own — no project, no worktree
    /// provisioning, nothing to create. So this creates nothing: it proves the
    /// directory Kontor prepared is the one this plane serves and that it is
    /// really there, and hands back the binding every role must then present.
    /// `created` is therefore always `false`, which is the honest answer rather
    /// than a cosmetic one.
    async fn prepare_workspace(
        &self,
        request: &WorkspacePrepareRequest,
    ) -> RuntimeResult<WorkspaceOutcome> {
        let declared = self.declared_capabilities();
        preflight(
            &declared,
            &OperationContext::new(RuntimeCapability::PrepareWorkspace),
        )?;
        let generation = self.generation();

        // Idempotent per team run, and answered from state first: a retry after a
        // lost answer cannot bind a second place if it never looks at one.
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

        // `WorkspaceRoot` already refuses `.`, `..` and repeated separators, so
        // two spellings of one place cannot compare unequal here.
        if request.root != self.config.task_worktree {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the requested root is not the task worktree this plane serves",
            });
        }
        // The one thing that can actually be verified about a directory: that it
        // is one. A launch into a path that is not there would otherwise be
        // discovered by the child, after it had already been given an account.
        if !std::path::Path::new(request.root.as_str()).is_dir() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the task worktree does not exist as a directory",
            });
        }

        let identity = self.identity(ExternalId::parse(request.root.as_str())?, generation);
        // Positive identity again, and stated plainly: there is no native
        // workspace to report a label back, so the evidence is this adapter's own
        // verification of the directory a moment ago.
        let correlation = WorkspaceCorrelationEvidence::establish(
            request.team_run_id,
            &request.correlation().to_string(),
            identity.clone(),
            request.requested_at,
        )?;
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
        Ok(WorkspaceOutcome {
            snapshot,
            created: false,
        })
    }

    /// Start one Codex process for one admitted run, under one proven account.
    ///
    /// The order is the security property, and every step must be able to refuse
    /// without the later ones having run:
    ///
    /// 1. the run is account-pinned at all;
    /// 2. the shared preflight — capability, trust, account environment, the
    ///    verified task workspace and the working directory it claims, and the
    ///    concurrency bound;
    /// 3. the workspace binding is the one *this* adapter prepared;
    /// 4. account admission (KON-MVP-07): the pin, the profile, availability, the
    ///    resolver policy, and a re-read afterwards;
    /// 5. exactly one resolved `CODEX_HOME`, absolute, with an inherited one
    ///    cleared;
    /// 6. the home's non-secret marker names this account and this provider
    ///    identity;
    /// 7. **only then** the seat is claimed, and the process is started.
    ///
    /// Steps 1–6 reach no process table. A run that fails any of them costs
    /// nothing to undo, which is the entire reason the claim is last.
    async fn launch(&self, request: &LaunchRequest) -> RuntimeResult<LaunchOutcome> {
        let Some(account_profile_id) = request.account_profile_id() else {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexLaunch",
                "requires an account-pinned run: this adapter exists to prove which account \
                 executed the work",
            )));
        };
        let declared = self.declared_capabilities();
        let (generation, held) = {
            let state = self.lock();
            (state.generation, state.bindings.len())
        };
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: true,
                binding: None,
                workspace: Some(request.workspace_claim()),
                current_generation: Some(generation),
                demand: Some(LimitDemand::ConcurrentSessions(
                    u32::try_from(held).unwrap_or(u32::MAX).saturating_add(1),
                )),
                context_policy: Some(request.context_policy()),
            },
        )?;

        // The shared claim proves the presented binding is internally consistent
        // and names this team run, this task and this root. It cannot prove the
        // binding came from *here* — a fabricated one is self-consistent — so the
        // adapter's own table is what says the directory was ever verified.
        match self.lock().workspaces.get(&request.team_run_id()) {
            Some(prepared) if Some(prepared) == request.workspace() => {}
            Some(_) => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "the presented workspace binding is not the one this adapter prepared",
                });
            }
            None => {
                return Err(RuntimeError::WorkspaceBindingRequired);
            }
        }

        let admission = self
            .accounts
            .admit(&CodexAccountRequest {
                agent_run_id: request.agent_run_id(),
                account_profile_id,
                capabilities: &declared,
                now: request.requested_at(),
            })
            .map_err(|refusal| account_refusal(&refusal))?;
        if admission.admitted.receipt.agent_run_id != request.agent_run_id()
            || admission.admitted.receipt.account_profile_id != account_profile_id
        {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexAccountAdmission",
                "answered about another run or another account than the launch named",
            )));
        }

        let (command, prepared, marker, digest) =
            self.prepare_invocation(request, &admission.admitted.environment)?;
        let receipt = Self::verified_receipt(&admission, &marker, digest, request.requested_at())?;

        // The seat is taken here: after everything that could refuse for free,
        // and in one step with the check that it was there to take. Splitting the
        // check from the take is the defect this arrangement exists to prevent —
        // the spawn below runs with the lock released, because it has to, so a
        // launch that had only *read* its reservation would leave the seat
        // reservable for the length of a process start and two callers would each
        // get a Codex in one worktree.
        {
            let state = &mut *self.lock();
            state.admissions.claim(request)?;
            // The registry's own answer to the run-keyed half, for a process no
            // seat knows about: this value is reassembled from separate tables, so
            // an adapter can come back holding a binding whose seat record did not
            // survive with it.
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
        }

        let outcome = self
            .launch_admitted(request, &command, prepared, receipt, generation)
            .await;
        if outcome.is_err() {
            self.lock().admissions.release(request);
        }
        outcome
    }

    async fn resume(&self, _request: &ResumeRequest) -> RuntimeResult<ControlPlaneObservation> {
        Err(self.refuse_unsupported(RuntimeCapability::Resume))
    }

    async fn send(&self, _request: &SendMessageRequest) -> RuntimeResult<MessageAck> {
        // Not a fabricated acknowledgement. An acknowledgement would say the
        // instruction reached the agent, and nothing delivered it.
        Err(self.refuse_unsupported(RuntimeCapability::SendMessage))
    }

    /// Stop the process this binding names.
    ///
    /// It addresses the adapter's own live child through the handle the transport
    /// issued for the *attested* binding, so a doctored snapshot cannot aim a kill
    /// at another account's process.
    ///
    /// What comes back acknowledges the request. It does not say the run closed,
    /// and it never can: the observation carries `CommandAck` and the state
    /// `Unknown`, and cancelling is not the same as having been cancelled.
    async fn cancel(&self, request: &CancelRequest) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.declared_capabilities(),
            &OperationContext {
                operation: RuntimeCapability::Cancel,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let record = self.execution(binding.binding_id())?;
        let ending = self.transport.stop(&record.exec_id).await?;
        let record = {
            let state = &mut *self.lock();
            let stored = state
                .executions
                .entry(binding.binding_id())
                .or_insert_with(|| record.clone());
            stored.ending.get_or_insert(ending);
            stored.clone()
        };
        Ok(ControlPlaneObservation {
            agent_run_id: binding.agent_run_id(),
            contact: RuntimeContact::ProcessMissing,
            // Not `Cancelled`. A kill that was accepted is not a run that ended
            // where Kontor asked it to, and this adapter cannot tell the two
            // apart.
            state: ObservedRunState::Unknown,
            identity: binding.identity().clone(),
            native_event_id: None,
            native_sequence: 0,
            observed_at: request.requested_at,
            evidence: Self::process_evidence(&record, Some(ending))?,
            source: ObservationSource::CommandAck,
        })
    }

    /// Read whether the process is still there.
    ///
    /// This is the whole of what an inspect can say about a Codex run, and the
    /// limit is deliberate: a `codex exec` that finished its work and one that
    /// crashed both exit, so **no exit status is ever read as an outcome**. A live
    /// process is reported running; a process that ended is reported `Unknown`
    /// from an advisory source, which no trust grade may close a run on.
    ///
    /// That second half matters more than it looks. `Inspect` is the one source a
    /// Grade B runtime *is* allowed to close a run on, so an inspect that reported
    /// a terminal state here would close it — which is exactly the mutant the
    /// suite's ending sweep exists to kill.
    async fn inspect(&self, request: &InspectRequest) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.declared_capabilities(),
            &OperationContext {
                operation: RuntimeCapability::Inspect,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let record = self.execution(binding.binding_id())?;
        let liveness = self.transport.liveness(&record.exec_id).await?;
        if liveness.process_id != record.process_id {
            return Err(RuntimeError::CorrelationFailed);
        }
        let Some(ending) = liveness.ending else {
            return Ok(ControlPlaneObservation {
                agent_run_id: binding.agent_run_id(),
                contact: RuntimeContact::Reachable,
                state: ObservedRunState::Running,
                identity: binding.identity().clone(),
                native_event_id: None,
                native_sequence: 0,
                observed_at: request.requested_at,
                evidence: Self::process_evidence(&record, None)?,
                source: ObservationSource::Inspect,
            });
        };
        let record = {
            let state = &mut *self.lock();
            let stored = state
                .executions
                .entry(binding.binding_id())
                .or_insert_with(|| record.clone());
            stored.ending.get_or_insert(ending);
            stored.clone()
        };
        Self::ending_observation(
            binding.agent_run_id(),
            binding.identity().clone(),
            &record,
            ending,
            request.requested_at,
        )
    }

    async fn adopt(&self, _request: &AdoptRequest) -> RuntimeResult<LaunchOutcome> {
        Err(self.refuse_unsupported(RuntimeCapability::Adopt))
    }

    async fn discover_sessions(&self) -> RuntimeResult<Vec<NativeSession>> {
        // Not an empty inventory. An empty list would read as "Codex is running
        // nothing", which is a claim about the machine; this is a claim about
        // Codex, which keeps no inventory to read.
        Err(self.refuse_unsupported(RuntimeCapability::Discovery))
    }

    /// Classify presented bindings against the processes this adapter started.
    ///
    /// It is not discovery and does not pretend to be: nothing here enumerates
    /// Codex processes on the machine, because there is no inventory to
    /// enumerate. What it can answer is narrower and honest — of the bindings
    /// *this* adapter issued, which still name a live process.
    ///
    /// Provenance comes first, and for a sharper reason than on the driving
    /// operations: `Matched` carries the action `Keep`, so a fabricated snapshot
    /// would come back endorsed as the binding to keep, which is how a forgery
    /// would outlive the reconciliation that exists to catch it.
    async fn reconcile(
        &self,
        bindings: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<ReconciliationReport> {
        let state = self.lock();
        let findings = bindings
            .iter()
            .map(|claimed| match state.bindings.attest(claimed) {
                Err(_) => ReconciliationFinding::Unattested {
                    agent_run_id: claimed.agent_run_id(),
                    binding_id: claimed.binding_id(),
                    presented: claimed.identity().clone(),
                },
                Ok(issued) => {
                    let snapshot = issued.snapshot();
                    let alive =
                        state
                            .executions
                            .get(&snapshot.binding_id())
                            .is_some_and(|record| {
                                record.generation == state.generation && record.ending.is_none()
                            });
                    if alive {
                        ReconciliationFinding::Matched {
                            agent_run_id: snapshot.agent_run_id(),
                            binding_id: snapshot.binding_id(),
                            identity: snapshot.identity().clone(),
                        }
                    } else {
                        // Lost contact, never completion. A process that is gone
                        // says nothing about the work it was doing.
                        ReconciliationFinding::MissingSession {
                            agent_run_id: snapshot.agent_run_id(),
                            binding_id: snapshot.binding_id(),
                            bound: snapshot.identity().clone(),
                        }
                    }
                }
            })
            .collect();
        Ok(ReconciliationReport {
            generation: state.generation,
            findings,
        })
    }

    async fn history(&self, _request: &HistoryRequest) -> RuntimeResult<HistoryPage> {
        // Not an empty page. An empty page would read as "the session said
        // nothing", which is a claim about the work; this is a claim about Codex.
        Err(self.refuse_unsupported(RuntimeCapability::History))
    }

    /// Follow one process's stdout as session content.
    ///
    /// This is the only session content this adapter exposes: what the live
    /// process printed while Kontor was listening. Each accepted frame keeps its
    /// raw JSON as canonical evidence, and the sequence is contiguous or the
    /// stream breaks — there is no third option in which a caller quietly receives
    /// output with a hole in it.
    async fn subscribe_live(
        &self,
        request: &LiveSubscribeRequest,
    ) -> RuntimeResult<LiveSubscription> {
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.declared_capabilities(),
            &OperationContext {
                operation: RuntimeCapability::LiveEvents,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let record = self.execution(binding.binding_id())?;
        let drained = self.transport.drain(&record.exec_id).await?;

        // Codex prints no per-frame timestamp, so what an event records is when
        // this adapter took delivery of it — which is the only instant it can
        // actually witness. It is never read as an ordering: that is the
        // sequence's job, and the sequence is validated.
        let observed_at = Timestamp::now();
        let state = &mut *self.lock();
        let events = Self::ingest(state, binding.binding_id(), &drained, observed_at)?;
        if let Some(ending) = drained.ending
            && let Some(stored) = state.executions.get_mut(&binding.binding_id())
        {
            stored.ending.get_or_insert(ending);
        }
        Ok(LiveSubscription::new(
            request.kinds.clone(),
            request.strict_after,
            events,
            // A closed stream is a fact about the channel, and this is where the
            // shared contract already says so.
            drained.ending.is_some(),
        ))
    }

    async fn respond_permission(
        &self,
        _request: &PermissionResponseRequest,
    ) -> RuntimeResult<PermissionAck> {
        Err(self.refuse_unsupported(RuntimeCapability::PermissionResponse))
    }

    /// Codex enforces its context window at startup, and cannot be asked to
    /// compact a run that is already going.
    ///
    /// `codex exec --json` is one-shot: the process reads a prompt, works and
    /// exits, and its own auto-compaction fires from the
    /// `model_auto_compact_token_limit` this adapter configured at launch.
    /// There is no live thread to address afterwards, which is why
    /// [`RuntimeCapability::Compact`] is not advertised.
    ///
    /// So this reports rather than acts. Re-running `codex exec` with a
    /// summarized prompt would be a *new process and a new session*, and calling
    /// that a compaction would be exactly the substitution the receipt contract
    /// forbids.
    async fn compact(&self, request: &CompactRequest) -> RuntimeResult<CompactionReceipt> {
        request.validate()?;
        let declared = self.declared_capabilities();

        // Production has no app-server lane, so this is the whole answer there:
        // report, touch nothing. Re-running `codex exec` with a summarized
        // prompt would be a new process and a new session, which is exactly the
        // substitution the receipt contract forbids.
        let Some(app_server) = self.app_server.clone() else {
            return Ok(request.unsupported_receipt(&declared, request.requested_at)?);
        };

        let binding = self.attested(&request.binding)?;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Compact,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(self.generation()),
                demand: None,
                context_policy: None,
            },
        )?;

        let before = request.binding.identity().clone();
        let thread_id = before.native_id.as_str().to_owned();
        let lifecycle = app_server
            .compact_thread(&crate::wire::ThreadCompactStart {
                thread_id: thread_id.clone(),
            })
            .await?;

        // A lifecycle about another thread is not evidence about this one.
        if lifecycle.iter().any(|event| event.thread_id() != thread_id) {
            return Err(RuntimeError::CorrelationFailed);
        }
        let completed = lifecycle.iter().rev().find_map(|event| match event {
            crate::wire::ThreadCompactEvent::Completed { tokens, .. } => Some(*tokens),
            _ => None,
        });
        let failed = lifecycle
            .iter()
            .any(|event| matches!(event, crate::wire::ThreadCompactEvent::Failed { .. }));

        // The re-read is what a confirmation rests on. An acknowledgement that
        // compaction "completed" is still only an acknowledgement.
        let after = app_server.inspect_thread(&thread_id).await?;
        let same_session = after.thread_id == thread_id && after.generation == before.generation;

        let observed = kontor_core::state::NativeRuntimeIdentity {
            generation: after.generation,
            native_id: kontor_core::id::ExternalId::parse(&after.thread_id)
                .map_err(RuntimeError::Domain)?,
            ..before.clone()
        };
        let status = if failed || completed.is_none() || !same_session {
            CompactionStatus::Failed
        } else {
            CompactionStatus::Confirmed
        };

        let receipt = CompactionReceipt {
            schema_version: request.policy.schema_version,
            id: request.receipt_id,
            agent_run_id: request.binding.agent_run_id(),
            binding_id: request.binding.binding_id(),
            native_before: before,
            native_after: Some(observed),
            requested: request.policy.requested,
            effective: request.policy.effective,
            trigger: request.trigger,
            capabilities: capability_document(&declared).map_err(RuntimeError::Domain)?,
            status,
            telemetry: kontor_core::compaction::CompactionTelemetry {
                // Only what the runtime actually reported. Codex says nothing
                // about the pre-compaction count or the cache, and inventing a
                // zero there would be a measurement nobody took.
                tokens_before: None,
                tokens_after: completed.flatten(),
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
            context_pack_hash: request.context_pack_hash.clone(),
            handoff_hash: request.handoff_hash.clone(),
            evidence: (status == CompactionStatus::Confirmed)
                .then(|| kontor_core::id::ExternalId::parse(crate::wire::THREAD_COMPACT_START))
                .transpose()
                .map_err(RuntimeError::Domain)?,
            recorded_at: request.requested_at,
        };
        receipt.validate().map_err(RuntimeError::Domain)?;
        Ok(receipt)
    }
}
