//! The replaceable execution contract: one trait, one error type, and the
//! acknowledgements a runtime hands back.
//!
//! Implementing [`RuntimeAdapter`] is the *only* way a runtime enters Kontor.
//! Two obligations come with it, and the shared contract suite exists to prove
//! them:
//!
//! * Every method calls [`crate::capability::preflight`] **before** it produces
//!   any effect, so an unsupported capability, an insufficient trust grade, a
//!   stale binding, an unavailable account environment or an oversized request
//!   is refused without touching the runtime.
//! * Native identifiers appear only as correlation evidence. A native session
//!   id never lands in a field that means "which run", "which binding" or
//!   "which message".

use async_trait::async_trait;
use kontor_core::DomainError;
use kontor_core::compaction::CompactionReceipt;
use kontor_core::id::{RuntimeBindingId, Timestamp};

use crate::admission::AdmissionRequest;
use crate::capability::{
    IssuedBinding, RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, TrustGrade,
};
use crate::observation::{ControlPlaneObservation, NativeSession, ReconciliationReport};
use crate::request::{
    AdoptRequest, CancelRequest, CompactRequest, HistoryRequest, InspectRequest, LaunchRequest,
    LiveSubscribeRequest, MessageId, PermissionDecision, PermissionResponseRequest, ResumeRequest,
    SendMessageRequest,
};
use crate::timeline::{HistoryPage, LiveSubscription, TimelineBreak, TimelinePosition};
use crate::workspace::{WorkspaceOutcome, WorkspacePrepareRequest};

/// Everything an adapter operation can refuse.
///
/// The payload is structural: static rules, capability names and positions. No
/// variant carries a message body, a prompt or a runtime payload, so a refusal
/// is safe to log.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The runtime never declared this capability.
    #[error("runtime does not support {capability}")]
    UnsupportedCapability {
        /// The operation that was attempted.
        capability: RuntimeCapability,
    },
    /// The runtime's trust grade may not be driven by Kontor's own authority.
    #[error("trust grade {found} cannot autonomously {operation}: {rule}")]
    InsufficientTrust {
        /// The grade the runtime declared.
        found: TrustGrade,
        /// The operation that was attempted.
        operation: RuntimeCapability,
        /// The policy that refused.
        rule: &'static str,
    },
    /// An account-pinned run through a runtime that cannot prove which account
    /// it executes as.
    #[error("runtime cannot prove a per-run account environment")]
    AccountEnvironmentUnavailable,
    /// The request is larger than the runtime declared it can take.
    #[error("{subject} exceeds the runtime limit of {limit}")]
    LimitExceeded {
        /// What was too large.
        subject: &'static str,
        /// The declared bound.
        limit: u64,
    },
    /// A launch presented no verified task workspace binding at all.
    #[error("launch requires a prepared task workspace binding")]
    WorkspaceBindingRequired,
    /// The presented workspace is not the one this work belongs in.
    #[error("task workspace mismatch: {rule}")]
    WorkspaceMismatch {
        /// Why the workspace was refused.
        rule: &'static str,
    },
    /// The binding no longer names a live session in this runtime generation.
    #[error("runtime binding is stale: {rule}")]
    StaleBinding {
        /// Why the binding cannot be used.
        rule: &'static str,
    },
    /// The runtime did not prove that the native session belongs to the run.
    #[error("native session is not correlated with the requested run")]
    CorrelationFailed,
    /// The selected provider has no permission mode Kontor knows how to pin.
    #[error("provider {provider} has no pinned runtime permission mode")]
    PermissionModeUnsupported {
        /// Provider whose mutable default was refused.
        provider: String,
    },
    /// The runtime did not apply the permission mode Kontor selected.
    #[error(
        "runtime permission mode mismatch for {provider}: expected {expected:?}, found {found:?}"
    )]
    PermissionModeMismatch {
        /// Provider whose mode was checked.
        provider: String,
        /// Mode Kontor pinned; `None` means this provider exposes no modes.
        expected: Option<String>,
        /// Mode the runtime read back.
        found: Option<String>,
    },
    /// The seat is already spoken for: it holds a live native session, or a
    /// reservation issued to someone else.
    ///
    /// **This is what makes AC-4 hold.** A seat may hold at most one
    /// non-terminal binding or one outstanding reservation, and the runtime is
    /// the only party that can answer which. Minting a fresh
    /// [`kontor_core::id::AgentRunId`] and
    /// [`kontor_core::id::RuntimeBindingId`] does not evade it, because neither
    /// appears in the key.
    #[error("role slot is already admitted: {rule}")]
    SlotAlreadyAdmitted {
        /// Why admission was refused.
        rule: &'static str,
    },
    /// A launch presented authority that is not the reservation this runtime is
    /// holding for that seat.
    ///
    /// Covers a replayed request whose reservation was already consumed, an
    /// authority issued for another seat, run or binding, and one assembled
    /// without asking a runtime at all.
    #[error("launch was not admitted: {rule}")]
    LaunchNotAdmitted {
        /// Why the authority was refused.
        rule: &'static str,
    },
    /// A replacement cited a predecessor the runtime cannot agree is finished.
    #[error("replacement is not evidenced: {rule}")]
    ReplacementNotEvidenced {
        /// Why the citation was refused.
        rule: &'static str,
    },
    /// A launch was requested for an agent run that already owns a live native
    /// session in this runtime.
    ///
    /// The seat-keyed rule above is the general one; this is the run-keyed
    /// companion, and it is not redundant: one run admitted into *two different*
    /// seats passes admission twice and is stopped here.
    #[error("agent run already owns a live native session: {rule}")]
    SessionAlreadyBound {
        /// Why the launch was refused.
        rule: &'static str,
    },
    /// The session's content can no longer be followed and must be re-read.
    #[error("timeline must be refetched: {reason}")]
    TimelineRefetchRequired {
        /// What broke.
        reason: TimelineBreak,
    },
    /// The continuation cursor cannot be used.
    #[error("history cursor is invalid: it {rule}")]
    InvalidCursor {
        /// Why the cursor was refused.
        rule: &'static str,
    },
    /// The message identifier contradicts an effect it already committed.
    #[error("message identifier {rule}")]
    DuplicateMessage {
        /// Why the identifier was refused.
        rule: &'static str,
    },
    /// The permission request cannot be answered this way.
    #[error("permission request {rule}")]
    PermissionConflict {
        /// Why the answer was refused.
        rule: &'static str,
    },
    /// The compaction would discard work state nothing durable has recorded, or
    /// would happen at a point the runtime cannot prove is safe.
    #[error("compaction is unsafe: {rule}")]
    CompactionUnsafe {
        /// Why the compaction was refused.
        rule: &'static str,
    },
    /// The compaction receipt identifier contradicts an attempt it already
    /// recorded.
    #[error("compaction receipt {rule}")]
    DuplicateCompaction {
        /// Why the identifier was refused.
        rule: &'static str,
    },
    /// The runtime could not be talked to. This is a fact about the channel and
    /// never about the work.
    #[error("runtime transport failed: the {rule}")]
    Transport {
        /// What went wrong with the channel.
        rule: &'static str,
    },
    /// A scripted adapter was called in a way its script does not describe.
    #[error("script mismatch: the script expects {expected} but {called} was called")]
    ScriptMismatch {
        /// The operation the script expects next.
        expected: &'static str,
        /// The operation that was actually called.
        called: &'static str,
    },
    /// A scripted step was pinned to one request and reached by another.
    ///
    /// The payload names only *which identifier* disagreed, never its value and
    /// never the request body, so a scripted refusal is as safe to log as any
    /// other.
    #[error("script mismatch: the queued step belongs to another {subject}")]
    ScriptRequestMismatch {
        /// The identifier that did not match.
        subject: &'static str,
    },
    /// A domain value was rejected before it reached the runtime.
    #[error(transparent)]
    Domain(#[from] DomainError),
}

/// Convenience alias for adapter operations.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// What a launch or adoption produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// The binding, with the capabilities frozen at this moment and the
    /// correlation evidence that ties the native session to the run.
    pub snapshot: RuntimeBindingSnapshot,
    /// The first normalized fact about the session. A launch acknowledgement is
    /// an acknowledgement, not a completion.
    pub observation: ControlPlaneObservation,
}

/// The runtime's answer to one delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAck {
    /// The Kontor identifier the message was sent under.
    pub message_id: MessageId,
    /// The binding it was delivered into.
    pub binding_id: RuntimeBindingId,
    /// Where the message landed in the session's content.
    pub position: TimelinePosition,
    /// When the runtime accepted it.
    pub accepted_at: Timestamp,
}

/// The runtime's answer to one permission response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAck {
    /// The runtime's own identifier for the request that was answered.
    pub permission_id: kontor_core::id::ExternalId,
    /// The Kontor identifier the answer was sent under.
    pub response_id: MessageId,
    /// The binding whose session raised the request.
    pub binding_id: RuntimeBindingId,
    /// The answer that was applied.
    pub decision: PermissionDecision,
    /// Where the resolution landed in the session's content.
    pub position: TimelinePosition,
    /// When the runtime accepted it.
    pub accepted_at: Timestamp,
}

/// One agent runtime, reduced to what Kontor is willing to depend on.
#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    /// What this runtime can currently prove.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the runtime cannot be reached.
    async fn discover_capabilities(&self) -> RuntimeResult<RuntimeCapabilities>;

    /// Vouch for a binding this runtime issued, so it can be judged as
    /// evidence.
    ///
    /// A [`RuntimeBindingSnapshot`] a caller is holding is a plain value with
    /// public fields — a clone with a better trust grade written into it looks
    /// exactly like the original. That is why closing a run
    /// ([`ControlPlaneObservation::terminal_evidence`]) takes an
    /// [`IssuedBinding`] and nothing else: only the runtime that issued a
    /// binding knows what it issued, so only the runtime can hand one back.
    ///
    /// The vouched-for value is the runtime's own copy, never the one that was
    /// presented.
    ///
    /// # Errors
    /// Returns [`RuntimeError::StaleBinding`] for a binding this runtime never
    /// issued, and for one that does not match what it issued.
    async fn issued_binding(
        &self,
        claimed: &RuntimeBindingSnapshot,
    ) -> RuntimeResult<IssuedBinding>;

    /// Make whatever this runtime needs *before* a census or a workspace can be
    /// asked for exist.
    ///
    /// Some runtimes hold a plane-level container — a project, a namespace, a
    /// tenant — that every later operation is addressed inside. Kontor cannot
    /// discover sessions in it, prepare a workspace under it or admit a seat
    /// into it until it exists, and the runtime is the only thing that can
    /// create it. This is where that happens.
    ///
    /// It is **idempotent and cheap**: a runtime that has already prepared its
    /// plane re-attests the binding it holds and creates nothing, so calling it
    /// on every startup census and before every workspace preparation is
    /// correct rather than wasteful.
    ///
    /// The default is `Ok(())`, because most runtimes have no such container and
    /// a lifecycle step that does nothing should not have to be written out. An
    /// implementation that *does* need one overrides this; nothing else about
    /// the contract changes.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the runtime could not be
    /// reached, and a runtime-specific refusal when the plane cannot be
    /// prepared unambiguously. A failure here is the same class of fact as a
    /// census that did not finish: it proves nothing about the plane, so a
    /// caller must treat it as "not ready" rather than as "empty".
    async fn prepare_plane(&self) -> RuntimeResult<()> {
        Ok(())
    }

    /// Take back into this runtime's own registry the bindings a previous
    /// process issued, so a restart does not orphan a live session.
    ///
    /// A binding's authority comes from the issuing runtime's copy of it
    /// ([`crate::capability::IssuedBindingRegistry`]), and that copy lives in the
    /// adapter's memory. After a restart the registry is empty, so every binding
    /// the realm still holds is unattestable — the session is alive, Kontor knows
    /// its identity, and nothing can operate it. This is the path back.
    ///
    /// **It re-records, it does not re-grade.** An implementation confirms the
    /// native session named by each snapshot is still there in the same
    /// generation, and then records *that snapshot, verbatim*: the trust grade,
    /// the limits, the correlation and the native identity are the ones the
    /// original binding was issued under, not ones re-derived from a fresh
    /// discovery. Re-deriving them would be exactly the re-grading the freeze
    /// rule forbids — a session bound at grade C would come back as A because the
    /// runtime happens to answer better today.
    ///
    /// A snapshot whose session is gone, or whose generation has moved, is
    /// **not** restored: a repeated native id in a new generation is a different
    /// session. Those are omitted from the answer rather than reported as errors,
    /// because a binding that did not survive is a reconciliation finding and not
    /// a failure of this call.
    ///
    /// The default restores nothing, which is the old behaviour: a runtime that
    /// cannot confirm its sessions across a restart must not pretend it can.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the runtime could not be reached
    /// at all. Being unable to confirm *any* session is a fact about the channel,
    /// and is different from confirming that none survived.
    async fn restore_bindings(
        &self,
        snapshots: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<Vec<RuntimeBindingSnapshot>> {
        let _ = snapshots;
        Ok(Vec::new())
    }

    /// Make a team run's task workspace exist and be usable.
    ///
    /// This is **idempotent** per team run: preparing the same team run's
    /// workspace again returns the original binding and creates nothing, so a
    /// retry after a lost answer cannot leave a second workspace behind.
    ///
    /// A runtime with a plane-level container expects [`RuntimeAdapter::prepare_plane`]
    /// to have succeeded first; this is not the place that creates one.
    ///
    /// # Errors
    /// Refuses a workspace that is the runtime's shared root, and a second
    /// preparation of the same team run at a different root.
    async fn prepare_workspace(
        &self,
        request: &WorkspacePrepareRequest,
    ) -> RuntimeResult<WorkspaceOutcome>;

    /// Make one topology node's native container exist and be usable.
    ///
    /// This is the placement path every accepted production seat travels.
    /// [`RuntimeAdapter::prepare_workspace`] above is the TeamRun-shaped
    /// predecessor, kept only so the older contract fixtures still describe the
    /// runtime they were written against.
    ///
    /// It is **idempotent per topology node**: preparing the same node again
    /// returns the original binding and creates nothing. That is a stronger
    /// promise than the workspace path's, and deliberately so — a node outlives
    /// every TeamRun inside it, so a retry after a lost answer must find the
    /// same container a week later, not a second one.
    ///
    /// An implementation dispatches on
    /// [`crate::container::ContainerProjection`], never on the node's kind key:
    /// the kind vocabulary belongs to the pinned specification revision, and an
    /// adapter holding its own copy of it is an adapter no specification change
    /// can correct.
    ///
    /// The default refuses. A runtime that cannot place a container must say so
    /// rather than let a caller believe an unbound node was bound.
    ///
    /// # Errors
    /// Returns [`RuntimeError::UnsupportedCapability`] by default, and in an
    /// implementation: [`RuntimeError::WorkspaceMismatch`] for a request whose
    /// shape it cannot build or whose parent it cannot confirm, and
    /// [`RuntimeError::CorrelationFailed`] when the container it read back does
    /// not carry this node's label.
    async fn prepare_container(
        &self,
        request: &crate::container::ContainerRequest,
    ) -> RuntimeResult<crate::container::ContainerOutcome> {
        let _ = request;
        Err(RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::PrepareProject,
        })
    }

    /// Change one already-bound container's visible title, and nothing else.
    ///
    /// The container is addressed by its exact native id inside its exact
    /// generation. Never by title — that is the value being corrected, so it
    /// cannot be the handle — and never by working directory, which several
    /// containers can share. An implementation that searched by either could
    /// rename the wrong container, and there is usually no way to tell
    /// afterwards.
    ///
    /// Parent, working directory, projection and native identity are all
    /// preserved. A runtime that can only achieve a new title by destroying and
    /// recreating the container has *not* got this capability: every binding
    /// Kontor holds resolves by the id that would be destroyed.
    ///
    /// Idempotent, and it says which happened. A container already carrying the
    /// desired title is the goal rather than an error, so a replay reports
    /// `changed: false` instead of refusing.
    ///
    /// The title is read back from the runtime after the change rather than
    /// assumed. An adapter that returned the requested title would make a
    /// silently-ignored rename indistinguishable from a successful one, which
    /// is the failure this whole operation exists to correct.
    ///
    /// The default refuses. Most runtimes fix a container's title at creation,
    /// and a caller must be able to tell "this runtime will not" from "this
    /// runtime did".
    ///
    /// # Errors
    /// Returns [`RuntimeError::UnsupportedCapability`] by default, and in an
    /// implementation: [`RuntimeError::StaleBinding`] when the addressed
    /// container is absent from the named generation, and
    /// [`RuntimeError::CorrelationFailed`] when the container read back does not
    /// carry this node's label.
    async fn retitle_container(
        &self,
        request: &crate::container::RetitleContainerRequest,
    ) -> RuntimeResult<crate::container::RetitleContainerOutcome> {
        let _ = request;
        Err(RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::RetitleContainer,
        })
    }

    /// What [`RuntimeAdapter::retitle_container`] would do, changing nothing.
    ///
    /// Same request, same derivation, same lookup by durable native id — the
    /// difference is that nothing is written. The desired title comes back
    /// because only the plane can render it, and the observed one because only
    /// the runtime knows it; a caller comparing them is what makes an operator's
    /// preview worth reading.
    ///
    /// It refuses for the same reasons the apply does, including
    /// [`RuntimeError::UnsupportedCapability`]. A preview that succeeded against
    /// a runtime that cannot rename would promise an apply that cannot happen.
    ///
    /// # Errors
    /// Returns [`RuntimeError::UnsupportedCapability`] by default, and in an
    /// implementation: [`RuntimeError::StaleBinding`] when the addressed
    /// container is absent from the named generation, and
    /// [`RuntimeError::CorrelationFailed`] when the container read back does not
    /// carry this node's label.
    async fn preview_retitle_container(
        &self,
        request: &crate::container::RetitleContainerRequest,
    ) -> RuntimeResult<crate::container::RetitleContainerOutcome> {
        let _ = request;
        Err(RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::RetitleContainer,
        })
    }

    /// Decide, atomically, whether one seat may be filled — and say so once.
    ///
    /// **This is where AC-4 is enforced.** An implementation keeps a table keyed
    /// by [`crate::admission::RoleSlotKey`] in which each seat holds at most one
    /// non-terminal native binding *or* one outstanding reservation, never both
    /// and never two. Checking and claiming that key happen without an
    /// interleaving, so two callers racing for one seat cannot both be admitted.
    ///
    /// Three answers, and no fourth:
    ///
    /// * the seat is free (or its cited predecessor is genuinely finished) →
    ///   claim it and return [`crate::admission::AdmissionOutcome::Admitted`];
    /// * the seat already holds a live session **for this same run** → return
    ///   [`crate::admission::AdmissionOutcome::Resumed`] with the runtime's own
    ///   binding, because compatible work continues, it does not relaunch;
    /// * anything else → refuse, having changed nothing.
    ///
    /// A repeated request naming the same seat, run and binding while a
    /// reservation is outstanding is the *same* request — re-issue the same
    /// reservation rather than a second one, so a lost answer is recoverable
    /// without ever holding two.
    ///
    /// Admission produces no native effect: nothing is started, and a refusal
    /// leaves nothing to undo.
    ///
    /// # Errors
    /// Returns [`RuntimeError::SlotAlreadyAdmitted`] for a seat held by another
    /// run, another binding or an outstanding reservation, and
    /// [`RuntimeError::ReplacementNotEvidenced`] for a replacement whose cited
    /// predecessor is not the binding this seat holds, is not observed finished,
    /// or is not linked to the run now asking.
    async fn admit_launch(
        &self,
        request: &AdmissionRequest,
    ) -> RuntimeResult<crate::admission::AdmissionOutcome>;

    /// Start a new native session for an agent run.
    ///
    /// **Consume the admission before the first native effect, and revalidate
    /// the seat while doing it.** Resolve the request's
    /// [`crate::admission::LaunchAuthority`] against the reservation this
    /// runtime is holding for the seat *the request names*; it must be that
    /// exact reservation, for that run and that binding. Then take it. A
    /// replayed request finds it spent, an authority aimed at another seat finds
    /// the wrong one, and an authority no runtime issued finds none —
    /// [`RuntimeError::LaunchNotAdmitted`] in every case, with zero sessions and
    /// zero effects.
    ///
    /// Reading and consuming the table is not a native effect, so this must come
    /// first: an implementation that starts a session and then discovers it was
    /// not admitted has already broken AC-4.
    ///
    /// **One live session per agent run**, as the run-keyed companion:
    /// [`RuntimeError::SessionAlreadyBound`] for a run that already owns a
    /// session, which catches one run admitted into two different seats.
    /// Recovery creates a *successor* run and launches that; it never starts a
    /// second session under the same [`kontor_core::id::AgentRunId`].
    ///
    /// # Errors
    /// Refuses before dispatch on admission, capability, trust, account
    /// environment, task workspace, an already-bound run and limits, and after
    /// dispatch when the session cannot be correlated with the requested run. A
    /// launch with no verified workspace binding, or one that claims a working
    /// directory other than the bound root, is refused before the session
    /// exists.
    async fn launch(&self, request: &LaunchRequest) -> RuntimeResult<LaunchOutcome>;

    /// Continue an existing native session in place.
    ///
    /// # Errors
    /// Refuses a stale binding and every preflight failure.
    async fn resume(&self, request: &ResumeRequest) -> RuntimeResult<ControlPlaneObservation>;

    /// Deliver one message into an existing native session.
    ///
    /// # Errors
    /// Refuses an oversized body and an identifier reused for different
    /// content. A retry of the same identifier and body replays the original
    /// acknowledgement instead of delivering twice.
    async fn send(&self, request: &SendMessageRequest) -> RuntimeResult<MessageAck>;

    /// Ask an existing native session to stop.
    ///
    /// # Errors
    /// Refuses every preflight failure. The returned observation acknowledges
    /// the request; it does not evidence that the run closed.
    async fn cancel(&self, request: &CancelRequest) -> RuntimeResult<ControlPlaneObservation>;

    /// Read the current authoritative state of one native session.
    ///
    /// # Errors
    /// Refuses every preflight failure.
    async fn inspect(&self, request: &InspectRequest) -> RuntimeResult<ControlPlaneObservation>;

    /// Bind an already-running native session to an agent run.
    ///
    /// **One live session per agent run here too, and checked before the first
    /// effect.** An [`AdoptRequest`] names no seat, so admission cannot answer
    /// for this door; the run-keyed rule is all there is, and without it a run
    /// that already holds a session picks up a second by being adopted onto
    /// another one. [`RuntimeError::SessionAlreadyBound`], decided against the
    /// sessions the runtime owns rather than against anything the caller
    /// presents — a fresh [`kontor_core::id::RuntimeBindingId`] buys nothing.
    ///
    /// Re-adopting the session a run *already* holds is that binding being
    /// re-issued and stays allowed: it is how a run recovers its own session
    /// after a runtime restart, and the superseded binding stops driving
    /// anything.
    ///
    /// # Errors
    /// Refuses a session that does not already carry this run's correlation
    /// label, one from another runtime generation, and a run that already holds
    /// a different session.
    async fn adopt(&self, request: &AdoptRequest) -> RuntimeResult<LaunchOutcome>;

    /// Enumerate the native sessions the runtime currently owns.
    ///
    /// The result carries no Kontor identity: discovery reports what is there,
    /// it does not decide what it belongs to.
    ///
    /// # Errors
    /// Refuses when discovery is unsupported or the runtime is unreachable.
    async fn discover_sessions(&self) -> RuntimeResult<Vec<NativeSession>>;

    /// Classify the given bindings against what the runtime currently owns.
    ///
    /// # Errors
    /// As [`RuntimeAdapter::discover_sessions`].
    async fn reconcile(
        &self,
        bindings: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<ReconciliationReport>;

    /// Read one page of a session's recorded content.
    ///
    /// # Errors
    /// Refuses an oversized page, a foreign or malformed cursor, and every
    /// preflight failure.
    async fn history(&self, request: &HistoryRequest) -> RuntimeResult<HistoryPage>;

    /// Follow a session's content strictly after a validated history position.
    ///
    /// # Errors
    /// Refuses every preflight failure. Continuity failures surface per event
    /// from [`LiveSubscription::next_event`].
    async fn subscribe_live(
        &self,
        request: &LiveSubscribeRequest,
    ) -> RuntimeResult<LiveSubscription>;

    /// Answer a permission request raised inside a session.
    ///
    /// # Errors
    /// Refuses an unknown request, one raised by another session, and a second
    /// answer that contradicts the first.
    async fn respond_permission(
        &self,
        request: &PermissionResponseRequest,
    ) -> RuntimeResult<PermissionAck>;

    /// Compact one live session's context **in place**.
    ///
    /// Three obligations, and the contract suite proves each of them:
    ///
    /// * **The session survives.** A receipt may only be
    ///   [`kontor_core::compaction::CompactionStatus::Confirmed`] when the runtime can be re-read and
    ///   names the same runtime kind, host, native id *and* generation
    ///   afterwards. Identity that moved is
    ///   [`kontor_core::compaction::CompactionStatus::Failed`] — never an adoption, never a successor,
    ///   and never a replacement dressed up as a compaction.
    /// * **Non-enforcement is said out loud.** A runtime without
    ///   [`RuntimeCapability::Compact`] returns a
    ///   [`kontor_core::compaction::CompactionStatus::NotEnforced`] receipt for
    ///   best-effort policy having touched nothing, or
    ///   refuses outright when the policy required enforcement. It never
    ///   substitutes a reload, an archive, a restart or a prompt that asks the
    ///   model nicely, and it never reports success it cannot attest.
    /// * **Idempotency is by receipt id.** Replaying the same
    ///   [`crate::request::CompactRequest`] returns the original receipt rather
    ///   than compacting twice; the same id with different content is refused.
    ///
    /// Unknown token and cache counters stay unknown. Zero is a measurement.
    ///
    /// # Errors
    /// * [`RuntimeError::CompactionUnsafe`] for a boundary or operator
    ///   compaction with no sealed durable handoff.
    /// * [`RuntimeError::DuplicateCompaction`] for a receipt id reused with
    ///   different content.
    /// * Every preflight failure, refused before any native effect.
    async fn compact(&self, request: &CompactRequest) -> RuntimeResult<CompactionReceipt>;
}
