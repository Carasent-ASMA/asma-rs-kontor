//! The Agent Orchestrator adapter: capabilities, refusals, normalization,
//! correlation and replay continuity, in one place.
//!
//! Everything AO can prove and everything it cannot lives here together on
//! purpose. Split across a factory, a policy object and a persistence port, the
//! interesting question — *may Kontor conclude this?* — stops being answerable by
//! reading one file, and that question is the whole ticket.
//!
//! # What AO is trusted for
//!
//! Grade B: a stable session id, exact branch correlation, fresh inspect, control
//! and a durable global replay log. That is enough to launch, resume, follow up,
//! cancel, inspect and reconcile.
//!
//! # What it is not
//!
//! AO 0.12.1 has no public semantic transcript API, no structured permission
//! request/response surface, no prepare-only worktree API that exposes a
//! canonical path, no durable native parent link, and no per-run account
//! environment. Each of those is declared unsupported and fails *before* a single
//! request is dispatched. None is filled in with a guess:
//!
//! * a stream close, a timeout, a missing process, an idle agent, a `no_signal`
//!   read, a replay gap and a daemon restart are all uncertainty, never
//!   completion — this adapter can never emit `Succeeded` or `Failed`, and
//!   [`ao_run_state_is_never_a_verdict`](#) pins that shut;
//! * a merged pull request is product workflow, not execution;
//! * a lost acknowledgement is answered from the ledger, never by repeating the
//!   effect.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_core::id::{
    AgentRunId, CanonicalDocument, ContentHash, ExternalId, ExternalName, RuntimeBindingId,
    RuntimeKindKey, Timestamp,
};
use kontor_core::repository::RuntimeBinding;
use kontor_core::state::{NativeRuntimeIdentity, ObservedRunState, RuntimeContact};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::adapter::{
    LaunchOutcome, MessageAck, PermissionAck, RuntimeAdapter, RuntimeError, RuntimeResult,
};
use kontor_runtime::admission::{
    AdmissionLedger, AdmissionOutcome, AdmissionRequest, OccupiedSeat, SeatFacts,
};
use kontor_runtime::capability::{
    IssuedBinding, IssuedBindingRegistry, LimitDemand, OperationContext, RuntimeBindingSnapshot,
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade, preflight,
};
use kontor_runtime::observation::{
    ControlPlaneObservation, CorrelationEvidence, NativeSession, ObservationSource,
    ReconciliationFinding, ReconciliationReport, reconcile,
};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, CorrelationLabel, HistoryRequest, InspectRequest, LaunchRequest,
    LiveSubscribeRequest, MessageId, PermissionResponseRequest, ResumeRequest, SendMessageRequest,
};
use kontor_runtime::timeline::{
    Admission, HistoryPage, LiveSubscription, MessageLedger, SessionEvent, SessionEventKind,
    TimelineBreak, TimelineGuard, TimelinePosition, TimelineStep,
};
use kontor_runtime::workspace::{WorkspaceOutcome, WorkspacePrepareRequest, WorkspaceRoot};

use crate::client::{AoCall, AoReply, AoTransport};
use crate::wire::{
    AoActivityState, AoCdcEvent, AoCdcEventType, AoHarness, AoKillSessionResponse,
    AoListAgentsResponse, AoListSessionsResponse, AoProjectGetResponse, AoRestoreSessionResponse,
    AoResumeAgentResponse, AoSendSessionMessageResponse, AoSessionKind, AoSessionStatus,
    AoSessionView, AoSpawnSessionResponse, MAX_MESSAGE_BYTES, parse_sse_events,
    parse_wire_timestamp,
};

/// The AO version this adapter's DTOs, fixtures and argv evidence are pinned to.
pub const AO_VERSION: &str = "0.12.1";

/// The operations AO 0.12.1 can actually prove.
const SUPPORTED: &[RuntimeCapability] = &[
    RuntimeCapability::Discovery,
    RuntimeCapability::Launch,
    RuntimeCapability::Resume,
    RuntimeCapability::SendMessage,
    RuntimeCapability::Cancel,
    RuntimeCapability::Inspect,
];

/// The operations AO 0.12.1 cannot, each with the reason it is refused.
///
/// This table is the adapter's public admission of what it does not know, and the
/// contract suite walks it: every entry owes a typed refusal issued before any
/// request. Filling one of these in with a plausible answer — an empty history
/// page, a permission answer typed into a terminal — is the failure mode the
/// whole design is arranged against.
pub const UNSUPPORTED: &[(RuntimeCapability, &str)] = &[
    (
        RuntimeCapability::PrepareWorkspace,
        "AO creates a worktree only as part of spawning a session and never exposes its \
         canonical path, so Kontor cannot verify a shared task workspace",
    ),
    (
        RuntimeCapability::Adopt,
        "AO cannot plant Kontor's full immutable correlation label into a branch that \
         already exists, so an existing session cannot be proven to belong to a run",
    ),
    (
        RuntimeCapability::History,
        "AO 0.12.1 exposes no versioned semantic transcript or cursor surface",
    ),
    (
        RuntimeCapability::LiveEvents,
        "AO's SSE stream is lifecycle change data and its /mux stream is character-level \
         PTY bytes; neither is the semantic session content the timeline contract is about",
    ),
    (
        RuntimeCapability::PermissionResponse,
        "AO 0.12.1 has no structured permission request/response API, and a keystroke \
         injected into a terminal could answer a dialog other than the intended one",
    ),
];

// ---------------------------------------------------------------------------
// Lane configuration
// ---------------------------------------------------------------------------

/// One configured AO lane: one endpoint, one project, one kind, one harness.
///
/// The harness is lane configuration rather than a launch parameter because
/// `LaunchRequest` carries no harness selector — so Claude, Codex, Cursor and
/// OpenCode are four lanes with four distinct runtime-kind keys, not one lane
/// with a runtime choice. That is also what keeps a Codex-specific safety guard
/// from silently applying to, or being skipped for, another client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AoLane {
    /// The Kontor runtime-kind key for this lane, e.g. `ao.claude-code`.
    pub runtime_kind: RuntimeKindKey,
    /// The host label that owns the generation.
    pub host: ExternalName,
    /// The AO project id every session in this lane belongs to.
    pub project_id: String,
    /// The project path AO works in. A launch must claim exactly this directory:
    /// AO owns the per-session worktree beneath it and never reveals its path, so
    /// this is the only working directory Kontor can verify.
    pub project_path: WorkspaceRoot,
    /// Worker or orchestrator.
    pub kind: AoSessionKind,
    /// The client this lane drives.
    pub harness: AoHarness,
    /// The most sessions Kontor will hold open in this lane at once.
    ///
    /// AO declares no concurrency limit of its own, so this is a Kontor-side
    /// policy bound rather than a discovered one. It is counted over the
    /// adapter's own issued bindings, which needs no request and therefore cannot
    /// turn a refusal into a round trip.
    pub max_concurrent_sessions: u32,
}

impl AoLane {
    /// The capabilities every binding in this lane freezes.
    #[must_use]
    pub fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            trust_grade: TrustGrade::B,
            supported: SUPPORTED.iter().copied().collect(),
            // AO runs one ambient project environment. A per-run coding account
            // cannot be proven, and an ambient one must never be promoted into
            // account routing just because it happens to work.
            account_env: false,
            limits: RuntimeLimits {
                max_message_bytes: MAX_MESSAGE_BYTES,
                // No semantic history surface exists, so no page size can be
                // honored. `History` is refused before this is ever consulted.
                max_history_page: 0,
                max_concurrent_sessions: self.max_concurrent_sessions,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Normalized lifecycle and attention
// ---------------------------------------------------------------------------

/// What an AO session is waiting for, in AO's own terms.
///
/// [`ControlPlaneObservation`] deliberately carries only a run state and a
/// contact fact, so this AO-specific distinction travels in canonical evidence
/// instead. It has to travel *somewhere*: `waiting_input` and `blocked` both
/// render as AO's `needs_input`, and they demand opposite automation — one is safe
/// to send a requested instruction into, the other is an agent stopped on a
/// pending decision where any input answers the dialog on the operator's behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AoAttention {
    /// Nothing is waiting.
    None,
    /// Reusable but unfinished; AO has no work signal.
    Idle,
    /// At an empty prompt, awaiting its next instruction. Safe to send to.
    AwaitingInstruction,
    /// Stopped on a pending decision. Kontor never sends into this state and
    /// never injects a keystroke to answer it.
    AwaitingDecision,
    /// AO's derived `needs_input` with no stronger raw state behind it. It is
    /// *not* known whether a decision is pending, so it must never be upgraded
    /// into a permission request.
    NeedsInputUnclassified,
    /// Stale, partial or absent evidence. Triggers inspect and reconciliation.
    Unknown,
}

impl AoAttention {
    /// The stable spelling recorded in canonical evidence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Idle => "idle",
            Self::AwaitingInstruction => "awaiting_instruction",
            Self::AwaitingDecision => "awaiting_decision",
            Self::NeedsInputUnclassified => "needs_input_unclassified",
            Self::Unknown => "unknown",
        }
    }

    /// Whether Kontor may deliver an automated message into this state.
    #[must_use]
    pub const fn accepts_automated_message(self) -> bool {
        !matches!(self, Self::AwaitingDecision)
    }
}

/// One AO session view, reduced to what Kontor may believe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AoLifecycle {
    /// What AO said about the work.
    pub state: ObservedRunState,
    /// What AO said about the channel and the process.
    pub contact: RuntimeContact,
    /// What the session is waiting for.
    pub attention: AoAttention,
}

/// Reduce an AO session view to a normalized lifecycle.
///
/// The raw hook-reported `activity.state` outranks the derived `status`, because
/// `status` is a display projection that folds pull-request facts in with
/// execution. `status` is consulted only where the raw state says nothing
/// stronger, and a source-control value never moves the run state at all.
///
/// Nothing here can produce [`ObservedRunState::Succeeded`] or
/// [`ObservedRunState::Failed`]. AO 0.12.1 has no trustworthy success or failure
/// verdict to offer, so inventing one from `idle`, `exited` or a merged pull
/// request is the single most consequential mistake available in this adapter.
#[must_use]
pub fn normalize_lifecycle(view: &AoSessionView) -> AoLifecycle {
    // An explicit termination flag is the strongest fact AO exposes. Whether it
    // may *close* a run is not decided here: that needs a fresh-inspect source at
    // a grade allowed to evidence it, which is the shared contract's judgement.
    if view.is_terminated {
        return AoLifecycle {
            state: ObservedRunState::Cancelled,
            contact: RuntimeContact::Reachable,
            attention: AoAttention::None,
        };
    }
    match view.activity.state {
        AoActivityState::Active => AoLifecycle {
            state: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            attention: AoAttention::None,
        },
        AoActivityState::WaitingInput => AoLifecycle {
            state: ObservedRunState::WaitingInput,
            contact: RuntimeContact::Reachable,
            attention: AoAttention::AwaitingInstruction,
        },
        AoActivityState::Blocked => AoLifecycle {
            state: ObservedRunState::Blocked,
            contact: RuntimeContact::Reachable,
            attention: AoAttention::AwaitingDecision,
        },
        // The agent process is gone. That is lost contact, and it is emphatically
        // not a verdict: a client that crashed and one that finished cleanly look
        // identical from here.
        AoActivityState::Exited => AoLifecycle {
            state: ObservedRunState::Unknown,
            contact: RuntimeContact::ProcessMissing,
            attention: AoAttention::Unknown,
        },
        // `idle` is the weak raw state: it means "no signal to report", so the
        // derived status is allowed to refine it — and only here.
        AoActivityState::Idle => match view.status {
            AoSessionStatus::Exited | AoSessionStatus::Terminated => AoLifecycle {
                state: ObservedRunState::Unknown,
                contact: RuntimeContact::ProcessMissing,
                attention: AoAttention::Unknown,
            },
            AoSessionStatus::NeedsInput => AoLifecycle {
                state: ObservedRunState::WaitingInput,
                contact: RuntimeContact::Reachable,
                attention: AoAttention::NeedsInputUnclassified,
            },
            AoSessionStatus::NoSignal => AoLifecycle {
                state: ObservedRunState::Unknown,
                contact: RuntimeContact::Reachable,
                attention: AoAttention::Unknown,
            },
            // `working` here means AO's projection disagrees with the raw hook.
            // The projection is the weaker witness, so this stays reusable rather
            // than being promoted to running.
            AoSessionStatus::Working | AoSessionStatus::Idle => AoLifecycle {
                state: ObservedRunState::Running,
                contact: RuntimeContact::Reachable,
                attention: AoAttention::Idle,
            },
            // Source control. A merged pull request is not a finished run.
            status if status.is_source_control() => AoLifecycle {
                state: ObservedRunState::Running,
                contact: RuntimeContact::Reachable,
                attention: AoAttention::Idle,
            },
            _ => AoLifecycle {
                state: ObservedRunState::Unknown,
                contact: RuntimeContact::Reachable,
                attention: AoAttention::Unknown,
            },
        },
    }
}

// ---------------------------------------------------------------------------
// Replay continuity
// ---------------------------------------------------------------------------

/// One accepted AO change event, normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AoObservedEvent {
    /// AO's global change-log sequence.
    pub seq: u64,
    /// The adapter generation the sequence belongs to.
    pub generation: u64,
    /// The session, when the change is session-scoped.
    pub session_id: Option<String>,
    /// The change type.
    pub event_type: AoCdcEventType,
    /// When AO recorded it.
    pub observed_at: Timestamp,
    /// The canonical raw envelope, persisted before any consequence is applied.
    pub evidence: CanonicalDocument,
}

impl AoObservedEvent {
    /// The deduplication key KON-MVP-03 storage uses:
    /// `(runtime kind, host, generation, AO seq)`.
    ///
    /// The generation is part of it because AO's sequence only means anything
    /// inside one continuous change log. Dropping it would make a post-reset
    /// `seq 4` collide with the `seq 4` of the previous log and silently
    /// disappear as a duplicate.
    #[must_use]
    pub fn dedup_key(&self, lane: &AoLane) -> String {
        format!(
            "{}|{}|{}|{}",
            lane.runtime_kind, lane.host, self.generation, self.seq
        )
    }
}

/// The global replay cursor.
///
/// AO's sequence is global, not per-session, so continuity is validated over
/// *every* event before anything is filtered by session. Filtering first is the
/// classic defect here: another session's `seq 5` would read as a hole in this
/// session's numbering and manufacture a gap that never happened.
///
/// The continuity policy itself is the shared [`TimelineGuard`] rather than a
/// second copy: AO's `(generation, seq)` maps exactly onto `(epoch, sequence)`,
/// and a break has to stay broken here for the same reason it does there.
#[derive(Debug)]
struct AoEventCursor {
    guard: TimelineGuard,
    /// The rehydration point: the sequence a *previous* adapter instance had
    /// already persisted. Fixed for this cursor's lifetime — it is a fact about
    /// what storage holds, not a moving cursor. Advancing it as events are
    /// accepted was a defect worth naming: it turned every ordinary reconnect
    /// replay into a regression, because each accepted event moved the floor up
    /// under the events AO was about to redeliver.
    boundary_seq: u64,
    boundary_digest: Option<ContentHash>,
    /// The digest of the newest accepted event, for the next checkpoint.
    last_digest: Option<ContentHash>,
}

impl AoEventCursor {
    fn new(generation: u64, last_seq: u64, last_digest: Option<ContentHash>) -> Self {
        Self {
            guard: TimelineGuard::starting_after(TimelinePosition {
                epoch: generation,
                sequence: last_seq,
            }),
            boundary_seq: last_seq,
            boundary_digest: last_digest.clone(),
            last_digest,
        }
    }

    /// Validate one event against the cursor.
    ///
    /// Three ranges, and the middle one is the only interesting one:
    ///
    /// * **below** the rehydration boundary — no digest was ever persisted for
    ///   that sequence, so there is nothing to compare and no trusted state to
    ///   advance. It is dropped rather than judged, exactly as the shared guard
    ///   drops a position validated before it existed. Pretending to an opinion
    ///   here would refuse ordinary reconnects.
    /// * **at** the boundary — this is the one sequence AO is certain to
    ///   redeliver when replaying from a persisted cursor, and the persisted
    ///   digest is the one thing that can tell "the same log, continuing" from "a
    ///   different log that also starts counting". A mismatch here is a reset.
    /// * **above** it — the shared guard's business: replay, contradiction and
    ///   gap, judged against digests this cursor itself accepted.
    fn admit(&mut self, event: &SessionEvent) -> RuntimeResult<TimelineStep> {
        let sequence = event.position.sequence;
        if sequence < self.boundary_seq {
            return Ok(TimelineStep::DuplicateIgnored);
        }
        if sequence == self.boundary_seq {
            return match &self.boundary_digest {
                Some(digest) if digest == event.digest() => Ok(TimelineStep::DuplicateIgnored),
                Some(_) => Err(RuntimeError::TimelineRefetchRequired {
                    reason: TimelineBreak::EpochChanged,
                }),
                // Sequence 0 with no persisted digest is the start of a log, not
                // a redelivery of anything.
                None => Ok(TimelineStep::DuplicateIgnored),
            };
        }
        self.guard.admit_event(event)
    }
}

/// Bridge the shared guard's accept step, so the AO cursor states its intent
/// once rather than at each call site.
trait GuardAdmit {
    fn admit_event(&mut self, event: &SessionEvent) -> RuntimeResult<TimelineStep>;
}

impl GuardAdmit for TimelineGuard {
    fn admit_event(&mut self, event: &SessionEvent) -> RuntimeResult<TimelineStep> {
        self.accept(event)
    }
}

// ---------------------------------------------------------------------------
// Delivery ledger and checkpoint
// ---------------------------------------------------------------------------

/// What became of one delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AoDelivery {
    /// AO acknowledged it.
    Acknowledged(MessageAck),
    /// AO may or may not have accepted it, and no public AO surface can settle
    /// which.
    ///
    /// This identifier is never POSTed again. Choosing a possibly-missing
    /// follow-up over a possible duplicate is deliberate: a duplicated
    /// instruction is an action taken twice in someone's repository, while a
    /// missing one is a stall an operator can see and resolve.
    ConfirmationUnknown,
}

/// Everything the adapter needs to be rebuilt after a Kontor restart.
///
/// The adapter defines no storage interface and opens no database. This is a
/// plain value the existing KON-MVP-03 binding, runtime-event and command-receipt
/// tables already hold; the constructor takes it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AoCheckpoint {
    /// The adapter generation these bindings belong to.
    pub generation: u64,
    /// The last AO change-log sequence accepted in that generation.
    pub last_event_seq: u64,
    /// The digest of the event at that sequence, which is what makes a reset
    /// change log distinguishable from a benign replay.
    pub last_event_digest: Option<ContentHash>,
    /// Every binding the adapter issued and has not invalidated.
    pub bindings: Vec<RuntimeBindingSnapshot>,
    /// Every team-run seat holding one of those sessions.
    ///
    /// Persisted because AC-4 is a rule about seats and a binding does not name
    /// one: without this, a restarted adapter would admit a second launch into a
    /// seat that is already working. Only *occupied* seats round-trip — a
    /// reservation exists only between an admission and the launch that spends
    /// it, and its authority cannot be serialized either, so a caller that
    /// restarts mid-admission asks again.
    pub seats: Vec<OccupiedSeat>,
    /// The message ledger, in commit order.
    pub deliveries: Vec<(MessageId, ContentHash, AoDelivery)>,
    /// The next adapter-local content position per binding.
    pub positions: Vec<(RuntimeBindingId, u64)>,
}

impl AoCheckpoint {
    /// A fresh lane with no history, in `generation`.
    #[must_use]
    pub fn fresh(generation: u64) -> Self {
        Self {
            generation,
            last_event_seq: 0,
            last_event_digest: None,
            bindings: Vec::new(),
            seats: Vec::new(),
            deliveries: Vec::new(),
            positions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AoState {
    generation: u64,
    events: AoEventCursor,
    bindings: IssuedBindingRegistry,
    /// The seat rule, from the shared ledger rather than restated here. Every
    /// read and write of it happens under this adapter's one state lock, which is
    /// what makes "check the seat, then claim it" a single step.
    admissions: AdmissionLedger,
    messages: MessageLedger<AoDelivery>,
    /// The commit-ordered projection of `messages`, which the shared ledger does
    /// not expose. Both are written through one helper so they cannot drift.
    deliveries: Vec<(MessageId, ContentHash, AoDelivery)>,
    positions: BTreeMap<RuntimeBindingId, u64>,
}

/// This adapter's answers to the two questions the shared ledger cannot answer.
///
/// Borrowed out of [`AoState`], so both are read in the same critical section
/// that claims the seat.
struct AoSeatFacts<'a> {
    bindings: &'a IssuedBindingRegistry,
    generation: u64,
}

impl SeatFacts for AoSeatFacts<'_> {
    fn issued_binding(&self, binding_id: RuntimeBindingId) -> Option<RuntimeBindingSnapshot> {
        self.bindings.get(binding_id).cloned()
    }

    /// What AO can prove synchronously, which is retirement and not completion.
    ///
    /// A binding from an older generation, or one this adapter no longer holds, is
    /// retired: it cannot keep a seat, and saying so needs no request.
    ///
    /// Completion is the half AO cannot answer here. It is Grade B — a run closes
    /// only on a *fresh inspect*, which is an async call this adapter must not make
    /// while holding its state lock, and the alternative would be reporting a
    /// session's last known state as terminality. That is exactly the guess this
    /// adapter is built to refuse, so a replacement over a live current-generation
    /// seat is refused as not evidenced rather than admitted on a stale read.
    /// Recovery reaches such a seat through the generation change that retires its
    /// binding, or through reconciliation.
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

/// The AO runtime adapter for one lane.
#[derive(Debug)]
pub struct AoAdapter {
    lane: AoLane,
    transport: Box<dyn AoTransport>,
    state: Mutex<AoState>,
}

impl AoAdapter {
    /// Build an adapter for `lane` over `transport`, rehydrated from
    /// `checkpoint`.
    #[must_use]
    pub fn new(lane: AoLane, transport: Box<dyn AoTransport>, checkpoint: AoCheckpoint) -> Self {
        let mut messages = MessageLedger::new();
        for (id, hash, delivery) in &checkpoint.deliveries {
            messages.record(*id, hash.clone(), delivery.clone());
        }
        Self {
            lane,
            transport,
            state: Mutex::new(AoState {
                generation: checkpoint.generation,
                events: AoEventCursor::new(
                    checkpoint.generation,
                    checkpoint.last_event_seq,
                    checkpoint.last_event_digest.clone(),
                ),
                bindings: {
                    let mut registry = IssuedBindingRegistry::new();
                    for snapshot in &checkpoint.bindings {
                        registry.record(snapshot.clone());
                    }
                    registry
                },
                admissions: {
                    let mut ledger = AdmissionLedger::new();
                    for seat in checkpoint.seats.iter().cloned() {
                        ledger.restore_occupied(seat);
                    }
                    ledger
                },
                messages,
                deliveries: checkpoint.deliveries.clone(),
                positions: checkpoint.positions.iter().copied().collect(),
            }),
        }
    }

    /// The lane this adapter drives.
    #[must_use]
    pub const fn lane(&self) -> &AoLane {
        &self.lane
    }

    /// The current adapter generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// The persistable state, for the existing KON-MVP-03 tables.
    #[must_use]
    pub fn checkpoint(&self) -> AoCheckpoint {
        let state = self.lock();
        AoCheckpoint {
            generation: state.generation,
            last_event_seq: state.events.guard.position().sequence,
            last_event_digest: state.events.last_digest.clone(),
            bindings: state.bindings.snapshots().cloned().collect(),
            seats: state.admissions.occupied_seats().collect(),
            deliveries: state.deliveries.clone(),
            positions: state.positions.iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AoState> {
        self.state.lock().expect("the AO adapter lock is intact")
    }

    fn identity(&self, native_id: ExternalId, generation: u64) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: self.lane.runtime_kind.clone(),
            host: self.lane.host.clone(),
            generation,
            native_id,
        }
    }

    /// Refuse an operation AO cannot perform, before anything is dispatched.
    ///
    /// Routing every unsupported method through the shared preflight — rather
    /// than returning the error directly — is what makes the refusal the *same*
    /// refusal the control plane gets from any other adapter, and what keeps the
    /// declared capability set and the behavior from disagreeing.
    fn refuse_unsupported(&self, capability: RuntimeCapability) -> RuntimeError {
        preflight(
            &self.lane.capabilities(),
            &OperationContext::new(capability),
        )
        .expect_err("this capability is declared unsupported")
    }

    // -- REST helpers -------------------------------------------------------

    async fn get_project(&self) -> RuntimeResult<crate::wire::AoProject> {
        let reply = self
            .transport
            .call(&AoCall::project(&self.lane.project_id))
            .await?;
        let envelope: AoProjectGetResponse = reply.parse("AoProjectGetResponse")?;
        let project = envelope.resolved()?;
        // Identity before contents. Everything read out of this envelope — the
        // path below, and the permission mode the Codex guard resolves — is only
        // evidence about the project it actually describes, while the spawn that
        // follows names `lane.project_id`. So an envelope about another project is
        // refused here rather than consulted: otherwise a foreign project with an
        // approval-gated config, reported under this lane's path, would clear the
        // guard on behalf of a project whose real mode is an approvals bypass.
        if project.id != self.lane.project_id {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "AO answered about a different project than the one this lane addressed",
            });
        }
        // A project whose canonical path is not the one this lane was configured
        // for is a different project that happens to share an id, and launching
        // into it would edit an unverified tree.
        let configured = self.lane.project_path.as_str();
        let reported = WorkspaceRoot::parse(&project.path)?;
        if reported.as_str() != configured {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "AO's project path is not the source directory this lane was configured for",
            });
        }
        Ok(project.clone())
    }

    /// Refuse an unsafe Codex launch before any mutating request.
    ///
    /// Codex is the one AO 0.12.1 client whose empty, `default` and
    /// `bypass-permissions` modes all resolve to
    /// `--dangerously-bypass-approvals-and-sandbox`. Only `accept-edits` and
    /// `auto` provably keep an approval gate, so only those two proceed.
    ///
    /// This runs before **every** call that starts a client — spawn, restore and
    /// resume-agent alike. Guarding spawn alone would leave restore as an
    /// unguarded route to the same unsandboxed process, which is the more likely
    /// path in practice: restore is what recovery does.
    async fn guard_codex_permissions(&self) -> RuntimeResult<()> {
        if !self.lane.harness.needs_permission_guard() {
            return Ok(());
        }
        let project = self.get_project().await?;
        let mode = project.config.effective_permission_mode(self.lane.kind);
        if !mode.is_approval_gated() {
            // The project config is never rewritten to make this safe: that would
            // be Kontor editing another owner's state, and it would do it behind
            // an operator who chose the mode.
            return Err(RuntimeError::Domain(DomainError::invalid(
                "AoCodexPermissions",
                "AO resolves this project's Codex permission mode to an approvals-and-sandbox \
                 bypass; only accept-edits or auto may be launched",
            )));
        }
        Ok(())
    }

    /// A fresh read of one session, with the raw envelope it came in.
    ///
    /// The raw body travels with the parsed view because KON-MVP-03 persists
    /// evidence before applying any normalized consequence — so the thing stored
    /// has to be what AO said, not this adapter's re-encoding of what it
    /// understood.
    ///
    /// This is the only place a single session is read, and it is where both
    /// checks on the answer live: the id AO replied about, and the lane it says
    /// the session belongs to. Every caller therefore branches on a validated
    /// view — which matters because the branch is what decides whether a client
    /// gets relaunched.
    async fn fetch_session(&self, native_id: &str) -> RuntimeResult<(AoSessionView, AoReply)> {
        let reply = self.transport.call(&AoCall::session(native_id)).await?;
        let view: AoSessionView = reply.parse("AoSessionView")?;
        if view.id != native_id {
            return Err(RuntimeError::CorrelationFailed);
        }
        self.ensure_lane_membership(&view)?;
        Ok((view, reply))
    }

    async fn fetch_inventory(&self) -> RuntimeResult<(Vec<AoSessionView>, AoReply)> {
        let reply = self.transport.call(&AoCall::sessions()).await?;
        let envelope: AoListSessionsResponse = reply.parse("AoListSessionsResponse")?;
        Ok((envelope.sessions, reply))
    }

    /// The AO clients this daemon has installed and recently authorized.
    ///
    /// Discovery evidence only. AO documents its `authorized` list as advisory
    /// and stale-prone with spawn as the authoritative check, so this can say
    /// "the binary resolved" and can never say which account it runs as.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when AO cannot be reached, and
    /// [`RuntimeError::Domain`] for a foreign envelope.
    pub async fn discover_clients(&self) -> RuntimeResult<AoListAgentsResponse> {
        let reply = self.transport.call(&AoCall::agents()).await?;
        reply.parse("AoListAgentsResponse")
    }

    // -- Provenance --------------------------------------------------------

    /// Resolve a presented binding to the runtime's **own** copy, before any
    /// effect.
    ///
    /// Every operation that addresses an existing session goes through here, and
    /// then uses the returned snapshot rather than the one it was handed.
    ///
    /// A [`RuntimeBindingSnapshot`] is a plain value with public fields, so a
    /// self-consistent one costs nothing to fabricate. `preflight` cannot catch
    /// that: it checks a snapshot against *itself* — the label names the run, the
    /// correlation names the identity — and is satisfied by any internally coherent
    /// forgery. Only the registry knows what this runtime actually issued, so this
    /// is the check that stands between a fabricated snapshot and a real session.
    ///
    /// Addressing follows from the same copy: the native id an operation sends to
    /// AO comes from the registry, never from the request, so a doctored snapshot
    /// cannot redirect a message into another session.
    ///
    /// It runs *before* `preflight` so that the capability, trust and limit
    /// verdicts are computed from the snapshot the runtime issued rather than from
    /// the caller's claims about it. That ordering is about honest diagnostics and
    /// defence in depth, not about a bypass: a mutation that moves attestation
    /// after `preflight` survives, because `preflight` produces no effect and this
    /// check still gates every request. Claiming otherwise would overstate what the
    /// ordering buys.
    ///
    /// # Errors
    /// Returns [`RuntimeError::StaleBinding`] for a binding this runtime never
    /// issued and for one that differs in any field from what it issued.
    fn attested(&self, claimed: &RuntimeBindingSnapshot) -> RuntimeResult<RuntimeBindingSnapshot> {
        self.lock()
            .bindings
            .attest(claimed)
            .map(|issued| issued.snapshot().clone())
    }

    /// Refuse a session AO returned that is not the one this lane asked for.
    ///
    /// A lane is exactly one project, one harness and one kind. A response naming
    /// any other is AO answering about work that is not this one's — a different
    /// project's repository, a client this lane never verified, or an orchestrator
    /// where a worker was requested. Binding it would hand the run a session in
    /// somebody else's tree, and the correlation branch alone cannot catch it
    /// because AO echoes back whatever branch was asked for.
    ///
    /// # Errors
    /// Returns [`RuntimeError::CorrelationFailed`], because what is missing is
    /// exactly proof that this native session belongs to this request.
    fn ensure_lane_membership(&self, view: &AoSessionView) -> RuntimeResult<()> {
        if !view.belongs_to(&self.lane.project_id, self.lane.harness) || view.kind != self.lane.kind
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(())
    }

    // -- Evidence ----------------------------------------------------------

    /// Canonicalize the raw AO envelope together with the values the mapping
    /// read.
    ///
    /// The raw body goes in first and unmodified: KON-MVP-03 persists evidence
    /// before any normalized consequence is applied, so a mapping that later
    /// turns out to be wrong can be re-derived from what AO actually said rather
    /// than from what this adapter concluded.
    fn session_evidence(
        raw: &str,
        view: &AoSessionView,
        lifecycle: AoLifecycle,
    ) -> DomainResult<CanonicalDocument> {
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "ao_version": AO_VERSION,
            "raw": raw,
            "raw_digest": ContentHash::of(raw.as_bytes()).as_str(),
            "read": {
                "activity_state": format!("{:?}", view.activity.state),
                "derived_status": format!("{:?}", view.status),
                "is_terminated": view.is_terminated,
            },
            "normalized": {
                "attention": lifecycle.attention.as_str(),
                "run_state": lifecycle.state.as_str(),
                "contact": lifecycle.contact.as_str(),
            },
        }))
    }

    fn observation(
        &self,
        agent_run_id: AgentRunId,
        identity: NativeRuntimeIdentity,
        raw: &str,
        view: &AoSessionView,
        source: ObservationSource,
    ) -> RuntimeResult<ControlPlaneObservation> {
        let lifecycle = normalize_lifecycle(view);
        Ok(ControlPlaneObservation {
            agent_run_id,
            contact: lifecycle.contact,
            state: lifecycle.state,
            identity,
            native_event_id: None,
            // AO exposes no per-session ordering on its REST surface. Its only
            // monotonic ordering is the global change log, which is validated in
            // `observe_events` and does not belong to a single read.
            native_sequence: 0,
            observed_at: view.observed_at()?,
            evidence: Self::session_evidence(raw, view, lifecycle)?,
            source,
        })
    }

    /// The observation a bound session's fresh read produces.
    fn bound_observation(
        &self,
        binding: &RuntimeBindingSnapshot,
        reply: &AoReply,
        view: &AoSessionView,
        source: ObservationSource,
    ) -> RuntimeResult<ControlPlaneObservation> {
        self.observation(
            binding.agent_run_id(),
            binding.identity().clone(),
            &reply.body,
            view,
            source,
        )
    }

    // -- Correlation -------------------------------------------------------

    /// Recover a launch whose acknowledgement was lost.
    ///
    /// The branch carries the run's full correlation label, so exactly one
    /// session in this lane can legitimately match. The three outcomes are all
    /// refusals to guess:
    ///
    /// * one match — bind it; the POST did land;
    /// * several — the lane has diverged, and picking one would bind a run to a
    ///   session that may belong to another. No launch.
    /// * none — it is *not* known whether AO created a session. The receipt stays
    ///   confirmation-unknown and reconciliation looks again. A blind relaunch
    ///   here is how one run ends up with two agents editing one repository.
    async fn recover_launch(
        &self,
        label: &CorrelationLabel,
    ) -> RuntimeResult<(AoSessionView, AoReply)> {
        let wanted = label.to_string();
        let (sessions, reply) = self.fetch_inventory().await?;
        let mut matches = sessions
            .into_iter()
            .filter(|view| {
                view.belongs_to(&self.lane.project_id, self.lane.harness)
                    && view.kind == self.lane.kind
                    && view.branch.as_deref() == Some(wanted.as_str())
            })
            .collect::<Vec<_>>();
        match matches.len() {
            1 => Ok((matches.remove(0), reply)),
            0 => Err(RuntimeError::Transport {
                rule: "acknowledgement was lost and no session carries this run's correlation \
                       branch yet",
            }),
            _ => Err(RuntimeError::CorrelationFailed),
        }
    }

    fn bind(
        &self,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
        view: &AoSessionView,
        at: Timestamp,
        generation: u64,
    ) -> RuntimeResult<RuntimeBindingSnapshot> {
        let identity = self.identity(view.native_id()?, generation);
        // The branch is raw runtime text. `establish` accepts it only when it is
        // exactly the label Kontor planted for this run, so a native session id
        // or another run's label is a refusal rather than a silent bind.
        let correlation = CorrelationEvidence::establish(
            agent_run_id,
            view.branch.as_deref().unwrap_or_default(),
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
            capabilities: self.lane.capabilities(),
            correlation,
        })
    }

    // -- Replay ------------------------------------------------------------

    /// Apply one AO SSE recording to the global replay cursor.
    ///
    /// Returns the events that moved the cursor forward, with exact replays
    /// dropped. Every AO event is validated, including the ones for other
    /// sessions and the pull-request ones this adapter does not act on, because
    /// the sequence is global: skipping an event before validating it is what
    /// turns another session's change into this session's gap.
    ///
    /// A break stops consumption and is reported typed. It never produces a
    /// terminal state, and the caller is expected to block scheduling and run a
    /// full inventory-and-inspect reconciliation.
    ///
    /// # Errors
    /// * `EpochChanged` — the change log regressed or was reset. The generation
    ///   advances, every binding from the old generation is invalidated, and
    ///   nothing is rebound.
    /// * `SequenceGap` — events are missing.
    /// * `ConflictingDuplicate` — an accepted sequence arrived with other content.
    /// * [`RuntimeError::Domain`] — the recording is not AO 0.12.1.
    pub fn observe_events(&self, recording: &str) -> RuntimeResult<Vec<AoObservedEvent>> {
        let events = parse_sse_events(recording)?;
        let mut state = self.lock();
        let generation = state.generation;
        let mut accepted = Vec::new();
        for event in &events {
            let (normalized, session_event) = Self::normalize_event(event, generation)?;
            match state.events.admit(&session_event) {
                Ok(TimelineStep::Accepted) => {
                    state.events.last_digest = Some(session_event.digest().clone());
                    accepted.push(normalized);
                }
                Ok(TimelineStep::DuplicateIgnored) => {}
                Err(error) => {
                    // A reset log, a missing continuation and a contradiction at
                    // an accepted sequence are different reports of the same
                    // predicament: Kontor can no longer prove what it holds. Each
                    // starts a new generation and invalidates every binding, so
                    // scheduling is blocked until reconciliation has reclassified
                    // them. Nothing is re-pointed or rebound — a repeated native
                    // id in a new generation is a different session.
                    state.generation = generation.saturating_add(1);
                    state.bindings.clear();
                    state.events = AoEventCursor::new(state.generation, 0, None);
                    return Err(error);
                }
            }
        }
        Ok(accepted)
    }

    fn normalize_event(
        event: &AoCdcEvent,
        generation: u64,
    ) -> RuntimeResult<(AoObservedEvent, SessionEvent)> {
        let observed_at = parse_wire_timestamp("AoCdcEvent.createdAt", &event.created_at)?;
        let evidence = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "ao_version": AO_VERSION,
            "seq": event.seq,
            "project_id": event.project_id,
            "session_id": event.session_id,
            "event_type": format!("{:?}", event.event_type),
            "created_at": event.created_at,
        }))?;
        let session_event = SessionEvent {
            // A lifecycle change, never session content: AO's change log says
            // that a row changed, not what the agent said.
            kind: SessionEventKind::StateChange,
            position: TimelinePosition {
                epoch: generation,
                sequence: event.seq,
            },
            subject: kontor_runtime::timeline::EventSubject::None,
            native_event_id: Some(ExternalId::parse(&event.seq.to_string())?),
            emitted_at: observed_at,
            payload: evidence.clone(),
        };
        Ok((
            AoObservedEvent {
                seq: event.seq,
                generation,
                session_id: event.session_id.clone(),
                event_type: event.event_type,
                observed_at,
                evidence,
            },
            session_event,
        ))
    }

    fn record_delivery(
        state: &mut AoState,
        message_id: MessageId,
        body_hash: ContentHash,
        delivery: AoDelivery,
    ) {
        state
            .messages
            .record(message_id, body_hash.clone(), delivery.clone());
        state.deliveries.push((message_id, body_hash, delivery));
    }

    fn next_position(state: &mut AoState, binding_id: RuntimeBindingId) -> TimelinePosition {
        let sequence = state.positions.entry(binding_id).or_insert(0);
        *sequence = sequence.saturating_add(1);
        TimelinePosition {
            // The epoch is the adapter generation, and the sequence counts what
            // this adapter delivered. AO exposes no semantic content position, so
            // this is explicitly adapter-local evidence of ordering — not a claim
            // about where the message sits in the agent's transcript.
            epoch: state.generation,
            sequence: *sequence,
        }
    }
    /// Everything a launch does once its seat has agreed to it.
    ///
    /// Separate from [`RuntimeAdapter::launch`] so one place decides what a
    /// failure costs: every `?` here is a refusal that happens after the seat was
    /// claimed, and all of them are answered by the single release at the call
    /// site.
    async fn launch_admitted(
        &self,
        request: &LaunchRequest,
        capabilities: &RuntimeCapabilities,
        generation: u64,
        held: usize,
    ) -> RuntimeResult<LaunchOutcome> {
        preflight(
            capabilities,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: request.account_profile_id().is_some(),
                binding: None,
                workspace: Some(request.workspace_claim()),
                current_generation: Some(generation),
                demand: Some(LimitDemand::ConcurrentSessions(
                    u32::try_from(held).unwrap_or(u32::MAX).saturating_add(1),
                )),
            },
        )?;

        // AO owns the per-session worktree and never publishes its path, so a
        // Kontor workspace binding cannot describe where this session will work.
        // Accepting one would mean advertising a verified shared task workspace
        // that nothing verified — and the shared preflight cannot catch it here,
        // because it only checks a workspace claim for a runtime that declares
        // `PrepareWorkspace`.
        if request.workspace().is_some() {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "AO owns its per-session worktree and exposes no canonical path, so it \
                       cannot accept a prepared Kontor task workspace",
            });
        }
        if *request.cwd() != self.lane.project_path {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the claimed working directory is not this lane's configured AO project",
            });
        }

        // Project resolution and the Codex guard both run before the spawn, so a
        // refusal costs no session.
        self.get_project().await?;
        self.guard_codex_permissions().await?;

        let label = request.correlation();
        let body = serde_json::json!({
            "projectId": self.lane.project_id,
            "kind": self.lane.kind.as_str(),
            "harness": self.lane.harness.as_str(),
            // The full correlation label rides the branch. AO's `displayName` is
            // capped at 20 characters, which a Kontor label exceeds, so a display
            // name could only ever hold a prefix — and a truncated label is not
            // correlation evidence.
            "branch": label.to_string(),
            "prompt": request.prompt().as_str(),
        })
        .to_string();

        let (view, raw) = match self.transport.call(&AoCall::spawn(body)).await {
            Ok(reply) => {
                let spawned: AoSpawnSessionResponse = reply.parse("AoSpawnSessionResponse")?;
                (spawned.session, reply)
            }
            // The POST may have landed. Searching by correlation before deciding
            // anything is the whole of §10.3's recovery rule; retrying the POST
            // would be how one run acquires two agents.
            Err(RuntimeError::Transport { .. }) => self.recover_launch(&label).await?,
            Err(other) => return Err(other),
        };

        // AO echoes back whatever branch it was asked for, so the correlation
        // label alone cannot prove the response is about work in this lane. A
        // session naming another project, another client or another kind is
        // refused before it becomes a binding: binding it would point the run at a
        // session in a tree Kontor never verified.
        self.ensure_lane_membership(&view)?;
        let snapshot = self.bind(
            request.agent_run_id(),
            request.binding_id(),
            &view,
            request.requested_at(),
            generation,
        )?;
        let observation = self.observation(
            request.agent_run_id(),
            snapshot.identity().clone(),
            &raw.body,
            &view,
            // A spawn is an acknowledgement. Whatever state AO reports in it, a
            // command acknowledgement can never close a run.
            ObservationSource::CommandAck,
        )?;
        // The reservation is spent in the same critical section that records the
        // binding, so there is no instant at which this adapter owns a session and
        // its seat is still reservable.
        {
            let state = &mut *self.lock();
            state.admissions.occupy(request, view.native_id()?);
            state.bindings.record(snapshot.clone());
        }
        Ok(LaunchOutcome {
            snapshot,
            observation,
        })
    }
}

#[async_trait]
impl RuntimeAdapter for AoAdapter {
    async fn discover_capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        // The capability set is a fixed, audited statement about AO 0.12.1 rather
        // than something probed, so this call proves only that the daemon is
        // answering. `/healthz` carries no boot id, which is exactly why a fresh
        // generation can never be minted from it — see `observe_events`.
        let reply = self.transport.call(&AoCall::healthz()).await?;
        if !(200..300).contains(&reply.status) {
            return Err(RuntimeError::Transport {
                rule: "runtime is not answering its liveness probe",
            });
        }
        Ok(self.lane.capabilities())
    }

    async fn issued_binding(
        &self,
        claimed: &RuntimeBindingSnapshot,
    ) -> RuntimeResult<IssuedBinding> {
        // The shared registry does the whole-value comparison, so this adapter
        // cannot get it subtly wrong: a clone with a promoted trust grade is
        // refused as "not the binding the runtime issued" rather than vouched for
        // on a matching id.
        self.lock().bindings.attest(claimed)
    }

    /// Admission is bookkeeping about seats: it starts nothing, reaches no AO
    /// surface, and is deliberately not recorded in the call ledger, so "the
    /// daemon was never called" keeps meaning what it says.
    async fn admit_launch(&self, request: &AdmissionRequest) -> RuntimeResult<AdmissionOutcome> {
        let state = &mut *self.lock();
        let facts = AoSeatFacts {
            bindings: &state.bindings,
            generation: state.generation,
        };
        state.admissions.admit(request, &facts)
    }

    async fn prepare_workspace(
        &self,
        _request: &WorkspacePrepareRequest,
    ) -> RuntimeResult<WorkspaceOutcome> {
        Err(self.refuse_unsupported(RuntimeCapability::PrepareWorkspace))
    }

    async fn launch(&self, request: &LaunchRequest) -> RuntimeResult<LaunchOutcome> {
        // Admission first, before the project read and long before the spawn.
        //
        // A `LaunchRequest` is a value: it can be held and presented twice. What it
        // cannot do is restate a fact about *this adapter* — whether this seat is
        // still holding the reservation this authority was issued for. A replay
        // finds it spent, an authority for another seat finds the wrong one, and
        // freshly minted run and binding ids do not help because the seat is the
        // key. Reading the table is not an effect, so this refusal costs nothing
        // and creates no session.
        //
        // The run-keyed companion is checked alongside it: it is not implied by the
        // seat rule, because one run admitted into two *different* seats passes
        // admission twice.
        let (capabilities, generation, held) = {
            let state = self.lock();
            state.admissions.ensure_admitted(request)?;
            if state
                .bindings
                .snapshots()
                .any(|snapshot| snapshot.agent_run_id() == request.agent_run_id())
            {
                return Err(RuntimeError::SessionAlreadyBound {
                    rule: "recovery launches a successor run, never the same run twice",
                });
            }
            (
                self.lane.capabilities(),
                state.generation,
                state.bindings.len(),
            )
        };

        // From here the reservation is claimed, so every remaining refusal has to
        // hand the seat back. A refused launch leaves no session either way; what
        // it must not also leave is a seat holding a reservation nobody can spend
        // or replace. One wrapper does it for every `?` below, because spelling the
        // release out per path is how one of them ends up forgotten.
        let outcome = self
            .launch_admitted(request, &capabilities, generation, held)
            .await;
        if outcome.is_err() {
            self.lock().admissions.release(request);
        }
        outcome
    }

    async fn resume(&self, request: &ResumeRequest) -> RuntimeResult<ControlPlaneObservation> {
        // Provenance first: preflight would otherwise judge this against the
        // caller's own capability snapshot.
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.lane.capabilities(),
            &OperationContext {
                operation: RuntimeCapability::Resume,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: None,
            },
        )?;

        // Inspect first. Resume is only allowed to do as much as the session
        // actually needs: relaunching a client that is already running would
        // discard live work, and AO's restore and resume-agent are not idempotent
        // reads.
        let native_id = binding.identity().native_id.as_str().to_owned();
        let (fresh, fresh_reply) = self.fetch_session(&native_id).await?;

        if fresh.is_terminated {
            self.guard_codex_permissions().await?;
            let reply = self.transport.call(&AoCall::restore(&native_id)).await?;
            let restored: AoRestoreSessionResponse = reply.parse("AoRestoreSessionResponse")?;
            if !restored.ok || restored.session_id != native_id {
                return Err(RuntimeError::CorrelationFailed);
            }
            self.ensure_lane_membership(&restored.session)?;
            return self.bound_observation(
                &binding,
                &reply,
                &restored.session,
                ObservationSource::CommandAck,
            );
        }

        let client_gone = matches!(fresh.activity.state, AoActivityState::Exited)
            || matches!(fresh.status, AoSessionStatus::Exited);
        if client_gone {
            self.guard_codex_permissions().await?;
            let reply = self
                .transport
                .call(&AoCall::resume_agent(&native_id))
                .await?;
            let resumed: AoResumeAgentResponse = reply.parse("AoResumeAgentResponse")?;
            if !resumed.ok || resumed.session_id != native_id {
                return Err(RuntimeError::CorrelationFailed);
            }
            self.ensure_lane_membership(&resumed.session)?;
            return self.bound_observation(
                &binding,
                &reply,
                &resumed.session,
                ObservationSource::CommandAck,
            );
        }

        // Already live. The fresh read is the answer, and no client is relaunched:
        // restore and resume-agent both start a client, and starting one that is
        // already working would discard live work.
        //
        // `fresh` is lane-validated by `fetch_session`, so this answer is subject
        // to the same membership check as the two branches above. Reporting an
        // unvalidated one would be the quietest of the three failures: no client
        // is started, so nothing looks wrong, and the run simply carries an
        // observation about somebody else's session as its own running state.
        self.bound_observation(&binding, &fresh_reply, &fresh, ObservationSource::Inspect)
    }

    async fn send(&self, request: &SendMessageRequest) -> RuntimeResult<MessageAck> {
        // Provenance first, so the limit below is the one AO really declared for
        // this binding rather than the one the caller's snapshot claims.
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.lane.capabilities(),
            &OperationContext {
                operation: RuntimeCapability::SendMessage,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::MessageBytes(request.body_bytes())),
            },
        )?;

        let body_hash = request.body_hash();
        // The ledger is consulted before the wire, so a retry answers from
        // recorded evidence and never becomes a second delivery.
        match self
            .lock()
            .messages
            .admit(&request.message_id, &body_hash)?
        {
            Admission::Replay(AoDelivery::Acknowledged(original)) => return Ok(original),
            Admission::Replay(AoDelivery::ConfirmationUnknown) => {
                return Err(RuntimeError::Transport {
                    rule: "delivery of this message is unconfirmed and it will not be sent again",
                });
            }
            Admission::New => {}
        }

        let native_id = binding.identity().native_id.as_str().to_owned();
        let payload = serde_json::json!({ "message": request.body.as_str() }).to_string();
        let outcome = self
            .transport
            .call(&AoCall::send(&native_id, payload))
            .await;

        let reply = match outcome {
            Ok(reply) => reply,
            // The channel died. AO may already have delivered it, and no public
            // AO surface can establish which — so the identifier is burned rather
            // than risked.
            Err(error) => {
                Self::record_delivery(
                    &mut self.lock(),
                    request.message_id,
                    body_hash,
                    AoDelivery::ConfirmationUnknown,
                );
                return Err(error);
            }
        };

        // A 4xx is AO answering "no": nothing was delivered, so the identifier
        // stays usable. A 5xx may have been a failure *after* acceptance, which
        // is the same uncertainty as a dead channel.
        if (400..500).contains(&reply.status) {
            return Err(RuntimeError::Transport {
                rule: "runtime refused the request",
            });
        }
        let acknowledged: RuntimeResult<AoSendSessionMessageResponse> =
            reply.parse("AoSendSessionMessageResponse");
        match acknowledged {
            // AO must echo this exact session and this exact body back. Accepting
            // a bare `ok` would let an acknowledgement for another message satisfy
            // this one.
            Ok(response)
                if response.ok
                    && response.session_id == native_id
                    && response.message == request.body.as_str() => {}
            Ok(response) if !response.ok => {
                return Err(RuntimeError::Transport {
                    rule: "runtime refused the request",
                });
            }
            // AO answered, but not about this message or this session. Something
            // may have been accepted, so this cannot be retried either.
            _ => {
                Self::record_delivery(
                    &mut self.lock(),
                    request.message_id,
                    body_hash,
                    AoDelivery::ConfirmationUnknown,
                );
                return Err(RuntimeError::Transport {
                    rule: "runtime acknowledged something other than this message",
                });
            }
        }

        let mut state = self.lock();
        let binding_id = binding.binding_id();
        let position = Self::next_position(&mut state, binding_id);
        let acknowledgement = MessageAck {
            message_id: request.message_id,
            binding_id,
            position,
            accepted_at: request.sent_at,
        };
        Self::record_delivery(
            &mut state,
            request.message_id,
            body_hash,
            AoDelivery::Acknowledged(acknowledgement.clone()),
        );
        Ok(acknowledgement)
    }

    async fn cancel(&self, request: &CancelRequest) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.lane.capabilities(),
            &OperationContext {
                operation: RuntimeCapability::Cancel,
                autonomous: true,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: None,
            },
        )?;
        let native_id = binding.identity().native_id.as_str().to_owned();
        let reply = self.transport.call(&AoCall::kill(&native_id)).await?;
        let killed: AoKillSessionResponse = reply.parse("AoKillSessionResponse")?;
        if !killed.ok || killed.session_id != native_id {
            return Err(RuntimeError::CorrelationFailed);
        }
        // AO acknowledges that it accepted the request. It has not said the
        // session stopped, and this adapter does not ask it to: the observation
        // carries `CommandAck`, which no trust grade may close a run on. Only a
        // later fresh inspect reading an explicit `isTerminated` can.
        let evidence = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "ao_version": AO_VERSION,
            "raw": reply.body,
            "raw_digest": ContentHash::of(reply.body.as_bytes()).as_str(),
            "normalized": { "attention": AoAttention::Unknown.as_str() },
        }))?;
        Ok(ControlPlaneObservation {
            agent_run_id: binding.agent_run_id(),
            contact: RuntimeContact::Reachable,
            state: ObservedRunState::Cancelled,
            identity: binding.identity().clone(),
            native_event_id: None,
            native_sequence: 0,
            observed_at: request.requested_at,
            evidence,
            source: ObservationSource::CommandAck,
        })
    }

    async fn inspect(&self, request: &InspectRequest) -> RuntimeResult<ControlPlaneObservation> {
        let binding = self.attested(&request.binding)?;
        let generation = self.generation();
        preflight(
            &self.lane.capabilities(),
            &OperationContext {
                operation: RuntimeCapability::Inspect,
                autonomous: false,
                account_pinned: false,
                binding: Some(&binding),
                workspace: None,
                current_generation: Some(generation),
                demand: None,
            },
        )?;
        let native_id = binding.identity().native_id.as_str().to_owned();
        let (view, reply) = self.fetch_session(&native_id).await?;
        self.bound_observation(&binding, &reply, &view, ObservationSource::Inspect)
    }

    async fn adopt(&self, _request: &AdoptRequest) -> RuntimeResult<LaunchOutcome> {
        // Discovery still proposes AO sessions for the adoption inbox; what
        // cannot happen is the mutation, because AO offers no way to plant the
        // full immutable label into an existing session's branch.
        Err(self.refuse_unsupported(RuntimeCapability::Adopt))
    }

    async fn discover_sessions(&self) -> RuntimeResult<Vec<NativeSession>> {
        preflight(
            &self.lane.capabilities(),
            &OperationContext {
                operation: RuntimeCapability::Discovery,
                autonomous: false,
                account_pinned: false,
                binding: None,
                workspace: None,
                current_generation: None,
                demand: None,
            },
        )?;
        let generation = self.generation();
        let mut found = Vec::new();
        for view in self.fetch_inventory().await?.0 {
            if !view.belongs_to(&self.lane.project_id, self.lane.harness)
                || view.kind != self.lane.kind
            {
                continue;
            }
            found.push(NativeSession {
                identity: self.identity(view.native_id()?, generation),
                // A branch that is not a Kontor label yields `None`, which is what
                // sends a foreign session to the adoption inbox unlinked. AO
                // 0.12.1 has no durable parent field, and a parent is never
                // inferred from a timestamp, a name or branch proximity.
                correlation: view
                    .branch
                    .as_deref()
                    .and_then(|branch| CorrelationLabel::parse(branch).ok()),
                state: normalize_lifecycle(&view).state,
                observed_at: view.observed_at()?,
            });
        }
        Ok(found)
    }

    async fn reconcile(
        &self,
        bindings: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<ReconciliationReport> {
        let sessions = self.discover_sessions().await?;

        // Provenance first here too, and for a sharper reason than on the driving
        // operations: reconciliation's own output is the authority. `Matched`
        // carries the action `Keep`, so a fabricated snapshot naming a session AO
        // really has would come back endorsed — and it is endorsed *as* the
        // binding to keep, which is how a forged binding would outlive the
        // reconciliation that exists to catch exactly that.
        //
        // Attested snapshots are then classified as the runtime's own copies, so a
        // clone with edited capabilities cannot even be matched by its own values.
        // The unattested ones are reported rather than dropped: silently omitting
        // them would leave the control plane holding bindings that no finding
        // mentions, and an unmentioned binding is one nothing ever reviews.
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

    async fn history(&self, _request: &HistoryRequest) -> RuntimeResult<HistoryPage> {
        // Not an empty page. An empty page would read as "the session said
        // nothing", which is a claim about the work; this is a claim about AO.
        Err(self.refuse_unsupported(RuntimeCapability::History))
    }

    async fn subscribe_live(
        &self,
        _request: &LiveSubscribeRequest,
    ) -> RuntimeResult<LiveSubscription> {
        Err(self.refuse_unsupported(RuntimeCapability::LiveEvents))
    }

    async fn respond_permission(
        &self,
        _request: &PermissionResponseRequest,
    ) -> RuntimeResult<PermissionAck> {
        Err(self.refuse_unsupported(RuntimeCapability::PermissionResponse))
    }
}
