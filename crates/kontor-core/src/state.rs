//! Orthogonal lifecycle state.
//!
//! Kontor keeps eight independent dimensions and never collapses one into
//! another:
//!
//! | Dimension | Type |
//! | --- | --- |
//! | task lifecycle | [`TaskState`] |
//! | profile phase | `PhaseKey` (see [`crate::spec`]) |
//! | gate | [`GateState`] |
//! | run lifecycle | [`RunLifecycle`] |
//! | what we asked the runtime for | [`DesiredRunState`] |
//! | what the runtime told us | [`ObservedRunState`] |
//! | what we may conclude | [`DerivedRunState`] |
//! | how old that conclusion is | [`Freshness`] |
//!
//! External ticket status is a ninth dimension and lives in [`crate::ticket`].
//!
//! The single most important rule here: **uncertainty is not completion.** A
//! missing process, a closed stream, a timeout or an unreachable runtime
//! produces [`DerivedRunState::LostContact`] or
//! [`DerivedRunState::RuntimeUnavailable`] — never a [`TerminalOutcome`].

use serde::{Deserialize, Serialize};

use crate::id::{
    AgentRunId, AggregateRevision, CommandReceiptId, ContentHash, EventCursor, ExternalId,
    ExternalName, MiniProjectId, ProjectId, RoleSlotId, RuntimeKindKey, SeatBindingId, TaskId,
    TeamRunId, Timestamp, TopologyKindKey, TopologyNodeId,
};
use crate::spec::{CatalogRoleRef, TopologySnapshot};
use crate::{DomainError, DomainResult};

closed_enum! {
    /// The lifecycle of a task, independent of any run or external ticket.
    TaskState, "TaskState" {
        /// Captured but not yet accepted into the backlog.
        Draft => "draft",
        /// Accepted, but dependencies or inputs are not resolved yet.
        Todo => "todo",
        /// Eligible for scheduling (arming and admission are separate checks).
        Ready => "ready",
        /// A run is currently the task's active work.
        InProgress => "in_progress",
        /// Externally blocked; returns to `ready` only through a command receipt.
        Blocked => "blocked",
        /// Deliberately set aside; returns to `ready` only through a command receipt.
        Parked => "parked",
        /// Waiting for a human decision; returns to `ready` only through a command receipt.
        NeedsHuman => "needs_human",
        /// Terminal: every required phase, gate and artifact passed or was waived.
        Done => "done",
        /// Terminal: the task failed and its run closed failed.
        Failed => "failed",
        /// Terminal: the task was cancelled.
        Cancelled => "cancelled",
    }
}

impl TaskState {
    /// Whether the state is terminal and therefore immutable.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    /// Whether returning to [`TaskState::Ready`] requires an explicit command.
    #[must_use]
    pub const fn requires_resume_command(self) -> bool {
        matches!(self, Self::Blocked | Self::Parked | Self::NeedsHuman)
    }

    /// Whether a terminal task in this state may be explicitly reopened.
    ///
    /// Only [`TaskState::Done`], and the asymmetry is the point. A completed task
    /// carries a *claim* — that its declared work is finished — and new evidence
    /// can falsify that claim without contradicting anything else it recorded:
    /// the gates that passed still passed, and reopening says the completion no
    /// longer covers the work, not that the history was wrong.
    ///
    /// A `failed` or `cancelled` task carries an *outcome* instead. Reopening one
    /// would mean deciding that a run which closed failed did not, or that work
    /// somebody withdrew is live again — neither of which is a claim later
    /// evidence can settle, and both of which have a successor task as the honest
    /// answer. So they stay refused, and the refusal names the transition rather
    /// than the terminality.
    #[must_use]
    pub const fn is_reopenable(self) -> bool {
        matches!(self, Self::Done)
    }

    /// Whether `next` is structurally reachable from `self`.
    ///
    /// Structural legality is necessary but not sufficient: see
    /// [`apply_task_transition`] for the evidence and authority rules.
    ///
    /// `Ready -> Done` is legal because closure is proved by evidence, not by
    /// the state a task happens to be sitting in. A task can reach `ready` with
    /// all of its work already finished — a reconcile that resumes a task whose
    /// seats have gone, or a run that closes before the row is moved on — and
    /// under the old table those tasks were unfinishable: every gate passed,
    /// every slot settled, and no legal transition left. Structural legality is
    /// not the safeguard here and never was. [`apply_task_transition`] demands a
    /// [`TaskClosureCertificate`] for `Done` whatever the previous state, so a
    /// `ready` task completes on exactly the evidence an `in_progress` one does.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use TaskState::{
            Blocked, Cancelled, Done, Draft, Failed, InProgress, NeedsHuman, Parked, Ready, Todo,
        };
        matches!(
            (self, next),
            (Draft, Todo | Cancelled)
                | (Todo, Ready | Blocked | Parked | NeedsHuman | Cancelled)
                | (
                    Ready,
                    InProgress | Blocked | Parked | NeedsHuman | Todo | Done | Cancelled
                )
                | (
                    InProgress,
                    Blocked | Parked | NeedsHuman | Done | Failed | Cancelled | Ready
                )
                | (Blocked | Parked | NeedsHuman, Ready | Cancelled)
                // `Done -> Ready` is legal *structurally* and reachable only with
                // an explicit reopen authority — see
                // [`apply_task_transition`], which refuses every terminal source
                // that does not present one. The pair is in the table because a
                // reopen is a real transition of this aggregate rather than a
                // special case bolted beside it.
                | (Done, Ready)
        )
    }
}

/// Proof that every required phase, gate and artifact of the pinned work
/// profile passed or was waived with evidence.
///
/// The only way to obtain one is
/// [`crate::spec::ResolvedWorkProfileSnapshot::certify_closure`], so a task
/// cannot reach [`TaskState::Done`] by asserting completion in the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskClosureCertificate {
    /// Private field: constructed only inside this crate.
    certified: (),
}

impl TaskClosureCertificate {
    pub(crate) const fn issue() -> Self {
        Self { certified: () }
    }

    /// Present the certificate (its existence is the proof).
    #[must_use]
    pub const fn is_certified(&self) -> bool {
        let () = self.certified;
        true
    }
}

closed_enum! {
    /// What Kontor may conclude about one seat's attachment to its runtime.
    ///
    /// This is a *conclusion drawn from stored evidence*, never a value copied
    /// from a runtime's own report. A runtime that calls itself running is
    /// making a claim about its process, not about the work: a seat can hold an
    /// open turn for hours while parked on an unanswered prompt.
    SeatAttachment, "SeatAttachment" {
        /// Created; its attachment deadline has not passed yet.
        Pending => "pending",
        /// Observed attached, with activity inside the declared bound.
        Attached => "attached",
        /// The attachment deadline passed with no attached observation.
        AttachmentFailed => "attachment_failed",
        /// The owning epic seat is closed or has been replaced.
        Orphaned => "orphaned",
        /// Attached, but with no observed activity inside the declared bound.
        Stalled => "stalled",
        /// Deliberately released or reaped.
        Released => "released",
    }
}

impl SeatAttachment {
    /// Whether this seat may be counted as executing a task's work.
    #[must_use]
    pub const fn is_executing(self) -> bool {
        matches!(self, Self::Attached)
    }

    /// Whether this seat must be excluded from admission, capacity and every
    /// task-state computation.
    #[must_use]
    pub const fn is_excluded(self) -> bool {
        matches!(
            self,
            Self::Orphaned | Self::AttachmentFailed | Self::Released
        )
    }

    /// Whether this conclusion is a stall that must reach a human.
    #[must_use]
    pub const fn requires_human(self) -> bool {
        matches!(self, Self::Stalled)
    }

    /// Whether this seat may hold a task in [`TaskState::InProgress`].
    ///
    /// [`Self::Pending`] qualifies deliberately: a seat dispatched a moment ago
    /// has not been confirmed yet, and refusing that would make every legitimate
    /// launch illegal. The bound is the attachment deadline — once it passes,
    /// the same silence becomes [`Self::AttachmentFailed`] and stops counting.
    /// That is what separates a starting task from the thirteen-hour phantom.
    #[must_use]
    pub const fn can_hold_progress(self) -> bool {
        matches!(self, Self::Attached | Self::Pending)
    }
}

/// What Kontor has stored about one seat's attachment.
///
/// Every field is a recorded deadline or a recorded observation. Nothing here
/// is read live from a runtime at evaluation time, which is what makes
/// [`evaluate_seat_attachment`] deterministic and replayable.
#[derive(Debug, Clone, Copy)]
pub struct SeatAttachmentObservation {
    /// The instant by which this seat must have been observed attached, fixed
    /// when the seat was created. Without it, a seat that never attached is
    /// indistinguishable from one that is merely slow — forever.
    pub attach_deadline: Timestamp,
    /// When the seat was last observed attached to its runtime session.
    pub last_attached_at: Option<Timestamp>,
    /// When the seat last produced observable activity.
    pub last_activity_at: Option<Timestamp>,
    /// Whether the owning epic seat is closed or has been replaced. Derived
    /// from that seat's lifecycle, never from the runtime's own parent field.
    pub parent_closed: bool,
    /// Whether the seat was deliberately released or reaped.
    pub released: bool,
    /// What the runtime says about itself. Carried so an escalation can quote
    /// it; deliberately **not** consulted when concluding attachment.
    pub runtime_reported: ObservedRunState,
}

/// Conclude one seat's attachment from stored evidence alone.
///
/// Order matters. A released seat is released whatever else is true; an orphan
/// is an orphan even if its runtime looks healthy, because its owner is gone
/// and nothing is steering it.
#[must_use]
pub fn evaluate_seat_attachment(
    observation: &SeatAttachmentObservation,
    now: Timestamp,
    max_idle: jiff::SignedDuration,
) -> SeatAttachment {
    if observation.released {
        return SeatAttachment::Released;
    }
    if observation.parent_closed {
        return SeatAttachment::Orphaned;
    }
    if observation.last_attached_at.is_none() {
        return if now > observation.attach_deadline {
            SeatAttachment::AttachmentFailed
        } else {
            SeatAttachment::Pending
        };
    }
    // Liveness is judged on observed *activity*, never on the runtime's
    // self-reported state, and never-observed activity is a stall rather than a
    // pass: `Freshness::Unknown` must not read as healthy.
    match Freshness::evaluate(observation.last_activity_at, now, max_idle) {
        Freshness::Fresh => SeatAttachment::Attached,
        Freshness::Stale | Freshness::Unknown => SeatAttachment::Stalled,
    }
}

/// Proof that a task's claim of progress is backed by a dispatched run with at
/// least one attached seat.
///
/// Like [`TaskClosureCertificate`], it has no public constructor: the only way
/// to obtain one is [`certify_task_progress`], so a caller cannot assert that
/// work is underway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskProgressEvidence {
    /// Private field: constructed only inside this crate.
    certified: (),
}

impl TaskProgressEvidence {
    pub(crate) const fn issue() -> Self {
        Self { certified: () }
    }

    /// Present the certificate (its existence is the proof).
    #[must_use]
    pub const fn is_certified(&self) -> bool {
        let () = self.certified;
        true
    }
}

/// Certify that a task may claim [`TaskState::InProgress`].
///
/// The discriminator is **time, not lifecycle**. A run is legitimately `queued`
/// for the moment between admission creating its seats and the runtime
/// acknowledging them, so refusing every queued run would make admission
/// illegal. What separates that moment from a phantom is whether any seat is
/// still inside its attachment deadline.
///
/// # Errors
/// [`DomainError::MissingEvidence`] when the run has closed, or when no seat
/// can hold progress.
pub fn certify_task_progress(
    run: RunLifecycle,
    seats: &[SeatAttachment],
) -> DomainResult<TaskProgressEvidence> {
    if run.is_terminal() {
        return Err(DomainError::MissingEvidence {
            subject: "task progress",
            rule: "a closed run is not work in progress",
        });
    }
    // A run whose seats have not been materialized yet is the same transient
    // moment as a `Pending` seat, so an empty list is not by itself a refusal.
    // A run that never materializes one is a real gap, but it is a *seat
    // materialization* deadline rather than a task-state rule, and it belongs
    // with the rest of the binding work in OP-02.
    if !seats.is_empty() && !seats.iter().any(|seat| seat.can_hold_progress()) {
        return Err(DomainError::MissingEvidence {
            subject: "task progress",
            rule: "every seat has failed to attach, orphaned, stalled or been released",
        });
    }
    Ok(TaskProgressEvidence::issue())
}

/// What a task presents about the team that did its work, when it is asked to
/// become terminal.
///
/// A task's profile and its team carry two independent sets of obligations: the
/// profile says which phases, gates and artifacts must exist, the team says
/// which role slots must have finished. Satisfying one says nothing about the
/// other, so a terminal transition names both.
///
/// [`TaskTeamClosure::Certified`] deliberately carries only *identity* — which
/// team run, and the digest of the policy that was proved about it. It is a
/// citation, not the evidence: the store re-proves the substance against its own
/// rows (the team run closed, it serves this task, none of its runs is still
/// open), exactly as a run closure re-proves the event it cites. A fabricated
/// citation therefore buys nothing.
///
/// The supported way to obtain one is
/// `kontor_teams::run::TeamClosureCertificate::task_team_closure`, which is the
/// only thing that can prove every *declared* role slot — including one that
/// never produced a run — is accounted for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskTeamClosure {
    /// The task's pinned profile prescribes no team, so there are no role slots
    /// to account for.
    NoTeam,
    /// The task ran through this team run, whose closure was certified.
    Certified {
        /// The team run being cited.
        team_run_id: TeamRunId,
        /// Digest of the declared-slot policy that was proved about it.
        policy_digest: ContentHash,
    },
}

/// Authority to reopen one terminal task.
///
/// Carries the receipt it was granted by, so the intent to reopen cannot be
/// expressed without the durable command that authorizes it: there is no way to
/// build one from nothing, and the receipt inside it is what an audit reads.
///
/// It is deliberately *not* the same thing as a resume receipt. A resume lets a
/// held task continue; this reopens something the Realm already concluded, which
/// is a different decision and is recorded as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskReopenAuthority(CommandReceiptId);

impl TaskReopenAuthority {
    /// Grant the authority the receipt `receipt` recorded.
    #[must_use]
    pub const fn granted_by(receipt: CommandReceiptId) -> Self {
        Self(receipt)
    }

    /// The receipt this authority was granted by.
    #[must_use]
    pub const fn receipt(self) -> CommandReceiptId {
        self.0
    }
}

/// A requested task transition together with the evidence it requires.
#[derive(Debug, Clone, Copy)]
pub struct TaskTransition<'a> {
    /// Requested next state.
    pub to: TaskState,
    /// Receipt of the command that resumes a blocked, parked or human-held task.
    pub resume_receipt: Option<CommandReceiptId>,
    /// Authority to reopen a terminal task, when that is what this is.
    ///
    /// Absent on every ordinary transition, which is why a terminal task stays
    /// immutable unless something explicitly reopens it.
    pub reopen: Option<TaskReopenAuthority>,
    /// How the task's current run closed, if it closed.
    pub run_outcome: Option<TerminalOutcome>,
    /// Proof of profile closure, required for [`TaskState::Done`].
    pub closure: Option<&'a TaskClosureCertificate>,
    /// Proof of an attached seat, required for [`TaskState::InProgress`].
    pub progress: Option<&'a TaskProgressEvidence>,
}

impl TaskTransition<'_> {
    /// A transition that needs no additional evidence.
    #[must_use]
    pub const fn to(state: TaskState) -> Self {
        Self {
            to: state,
            resume_receipt: None,
            reopen: None,
            run_outcome: None,
            closure: None,
            progress: None,
        }
    }
}

/// Apply a task transition, enforcing terminality, resume receipts, run closure
/// and profile closure.
///
/// A terminal task is immutable, with exactly one bounded exception: a `done`
/// task presenting a [`TaskReopenAuthority`] may return to
/// [`TaskState::Ready`]. Everything about that exception is narrow on purpose —
/// one source state, one target state, and a durable receipt — because the
/// alternative to a bounded reopen is an unbounded one, and the alternative to
/// *any* reopen is an advertised operation that cannot work.
///
/// Reopening changes the task's state and nothing else. It closes no run, opens
/// none, and touches no gate: the history a completion was granted on stays
/// exactly as it was recorded, which is what makes the reopen auditable rather
/// than a rewrite.
///
/// # Errors
/// * [`DomainError::Terminal`] when `from` is terminal and nothing reopens it.
/// * [`DomainError::IllegalTransition`] when the pair is not in the table, and
///   when a reopen names a terminal state that may not be reopened or a target
///   other than [`TaskState::Ready`].
/// * [`DomainError::MissingAuthority`] when a resume has no command receipt.
/// * [`DomainError::MissingEvidence`] when closure or run-failure evidence is
///   absent.
pub fn apply_task_transition(
    from: TaskState,
    transition: &TaskTransition<'_>,
) -> DomainResult<TaskState> {
    if from.is_terminal() {
        // The authority is consulted *before* terminality is enforced, which is
        // the whole correction: the previous order refused every terminal source
        // without ever looking at what was being asked for, so an operation the
        // surface advertises could not reach the domain rule written for it.
        let Some(_authority) = transition.reopen else {
            return Err(DomainError::Terminal { subject: "task" });
        };
        if !from.is_reopenable() {
            return Err(DomainError::IllegalTransition {
                subject: "task reopen",
                from: from.as_str(),
                to: transition.to.as_str(),
            });
        }
        if transition.to != TaskState::Ready {
            return Err(DomainError::IllegalTransition {
                subject: "task reopen",
                from: from.as_str(),
                to: transition.to.as_str(),
            });
        }
    } else if transition.reopen.is_some() {
        // Nothing to reopen. Answering "done" for a task that is still open would
        // let a reopen stand in for a resume and skip the receipt rule that
        // governs one.
        return Err(DomainError::IllegalTransition {
            subject: "task reopen",
            from: from.as_str(),
            to: transition.to.as_str(),
        });
    }
    if !from.can_transition_to(transition.to) {
        return Err(DomainError::IllegalTransition {
            subject: "task",
            from: from.as_str(),
            to: transition.to.as_str(),
        });
    }
    if from.requires_resume_command()
        && transition.to == TaskState::Ready
        && transition.resume_receipt.is_none()
    {
        return Err(DomainError::MissingAuthority {
            subject: "task resume",
            rule: "leaving blocked, parked or needs_human requires a command receipt",
        });
    }
    match transition.to {
        TaskState::Done if transition.closure.is_none() => {
            return Err(DomainError::MissingEvidence {
                subject: "task completion",
                rule: "every required phase, gate and artifact must pass or be waived",
            });
        }
        TaskState::Failed if transition.run_outcome != Some(TerminalOutcome::Failed) => {
            return Err(DomainError::MissingEvidence {
                subject: "task failure",
                rule: "the current run must be closed failed",
            });
        }
        TaskState::InProgress if transition.progress.is_none() => {
            return Err(DomainError::MissingEvidence {
                subject: "task progress",
                rule: "a dispatched run with at least one attached seat must be observed",
            });
        }
        _ => {}
    }
    Ok(transition.to)
}

closed_enum! {
    /// The state of one gate in a work profile.
    GateState, "GateState" {
        /// Its phase has not been reached, or prerequisites are missing.
        NotReady => "not_ready",
        /// Prerequisites are met; evaluation may start.
        Ready => "ready",
        /// Evaluation is in progress.
        Active => "active",
        /// Passed by an authorized evaluator with evidence.
        Passed => "passed",
        /// Rejected; the profile's rejection route decides where work returns to.
        Rejected => "rejected",
        /// Waived by a distinct waiver authority with evidence.
        Waived => "waived",
        /// Held without a verdict.
        Parked => "parked",
    }
}

impl GateState {
    /// Whether this state satisfies a required gate for task closure.
    #[must_use]
    pub const fn satisfies_requirement(self) -> bool {
        matches!(self, Self::Passed | Self::Waived)
    }
}

closed_enum! {
    /// The verdict recorded by one append-only gate evaluation.
    GateVerdict, "GateVerdict" {
        /// The evaluator started work on the gate.
        Started => "started",
        /// The evaluator passed the gate.
        Passed => "passed",
        /// The evaluator rejected the gate.
        Rejected => "rejected",
        /// A waiver authority waived the gate.
        Waived => "waived",
        /// The gate was parked without a verdict.
        Parked => "parked",
    }
}

impl GateVerdict {
    /// The gate state this verdict produces.
    #[must_use]
    pub const fn resulting_state(self) -> GateState {
        match self {
            Self::Started => GateState::Active,
            Self::Passed => GateState::Passed,
            Self::Rejected => GateState::Rejected,
            Self::Waived => GateState::Waived,
            Self::Parked => GateState::Parked,
        }
    }

    /// Whether this verdict may only be recorded with evidence.
    #[must_use]
    pub const fn requires_evidence(self) -> bool {
        matches!(self, Self::Passed | Self::Waived)
    }
}

closed_enum! {
    /// The lifecycle of one run (a team run or an agent run).
    ///
    /// These values are not interchangeable with [`TaskState`]: a failed run does
    /// not by itself fail a task, and a succeeded run does not close one.
    RunLifecycle, "RunLifecycle" {
        /// Accepted and waiting to be launched.
        Queued => "queued",
        /// Launch has been dispatched but not acknowledged.
        Launching => "launching",
        /// Executing.
        Running => "running",
        /// Executing but waiting for input.
        WaitingInput => "waiting_input",
        /// Executing but blocked on an external condition.
        Blocked => "blocked",
        /// Terminal: completed successfully.
        Succeeded => "succeeded",
        /// Terminal: completed unsuccessfully.
        Failed => "failed",
        /// Terminal: cancelled.
        Cancelled => "cancelled",
        /// Terminal: parked and closed without a verdict.
        Parked => "parked",
    }
}

impl RunLifecycle {
    /// Whether this lifecycle value is terminal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Parked
        )
    }

    /// Whether the run has actually been dispatched to a runtime.
    ///
    /// A queued run has been accepted, not started. Treating the two as the
    /// same is how a task comes to claim progress over work nobody launched.
    #[must_use]
    pub const fn is_dispatched(self) -> bool {
        !self.is_terminal() && !matches!(self, Self::Queued)
    }

    /// Whether `next` is a legal non-terminal advance from `self`.
    ///
    /// Closure is deliberately *not* in this table: a run reaches a terminal
    /// value only through evidence-bearing closure, never through an advance.
    #[must_use]
    pub const fn can_advance_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Launching)
                | (
                    Self::Launching,
                    Self::Running | Self::WaitingInput | Self::Blocked
                )
                | (Self::Running, Self::WaitingInput | Self::Blocked)
                | (Self::WaitingInput, Self::Running | Self::Blocked)
                | (Self::Blocked, Self::Running | Self::WaitingInput)
        )
    }

    /// The terminal outcome this lifecycle value denotes, if any.
    #[must_use]
    pub const fn terminal_outcome(self) -> Option<TerminalOutcome> {
        match self {
            Self::Succeeded => Some(TerminalOutcome::Succeeded),
            Self::Failed => Some(TerminalOutcome::Failed),
            Self::Cancelled => Some(TerminalOutcome::Cancelled),
            Self::Parked => Some(TerminalOutcome::Parked),
            _ => None,
        }
    }
}

closed_enum! {
    /// What Kontor has asked the runtime to do. Never derived from observation.
    DesiredRunState, "DesiredRunState" {
        /// No intent has been recorded.
        NoIntent => "no_intent",
        /// A launch has been requested.
        RunRequested => "run_requested",
        /// A cancellation has been requested.
        CancelRequested => "cancel_requested",
        /// A park has been requested.
        ParkRequested => "park_requested",
        /// An operator abandon has been requested.
        AbandonRequested => "abandon_requested",
    }
}

closed_enum! {
    /// What the runtime actually reported. Never inferred, never written by a
    /// client, and never promoted into [`DerivedRunState`] verbatim.
    ObservedRunState, "ObservedRunState" {
        /// Nothing trustworthy has been observed yet.
        Unknown => "unknown",
        /// The runtime reports the session queued.
        Queued => "queued",
        /// The runtime reports the session launching.
        Launching => "launching",
        /// The runtime reports the session running.
        Running => "running",
        /// The runtime reports the session waiting for input.
        WaitingInput => "waiting_input",
        /// The runtime reports the session blocked.
        Blocked => "blocked",
        /// The runtime reports the session succeeded.
        Succeeded => "succeeded",
        /// The runtime reports the session failed.
        Failed => "failed",
        /// The runtime reports the session cancelled.
        Cancelled => "cancelled",
    }
}

impl ObservedRunState {
    /// The terminal outcome this observation *evidences*, if any.
    ///
    /// Only a trusted, explicit runtime report of completion qualifies. There is
    /// deliberately no mapping from a missing process or a closed stream.
    #[must_use]
    pub const fn observed_terminal_outcome(self) -> Option<TerminalOutcome> {
        match self {
            Self::Succeeded => Some(TerminalOutcome::Succeeded),
            Self::Failed => Some(TerminalOutcome::Failed),
            Self::Cancelled => Some(TerminalOutcome::Cancelled),
            _ => None,
        }
    }
}

closed_enum! {
    /// How a run finally closed.
    TerminalOutcome, "TerminalOutcome" {
        /// Closed successfully on trusted runtime evidence.
        Succeeded => "succeeded",
        /// Closed unsuccessfully on trusted runtime evidence.
        Failed => "failed",
        /// Closed because a cancellation was carried out.
        Cancelled => "cancelled",
        /// Closed because it was parked and released.
        Parked => "parked",
        /// Closed by an explicit operator abandon receipt.
        Abandoned => "abandoned",
    }
}

impl TerminalOutcome {
    /// The run lifecycle value that corresponds to this outcome.
    #[must_use]
    pub const fn lifecycle(self) -> RunLifecycle {
        match self {
            Self::Succeeded => RunLifecycle::Succeeded,
            Self::Failed => RunLifecycle::Failed,
            Self::Cancelled => RunLifecycle::Cancelled,
            // An abandoned run is closed without a runtime verdict; it is parked
            // in the lifecycle dimension and abandoned in the outcome dimension.
            Self::Parked | Self::Abandoned => RunLifecycle::Parked,
        }
    }
}

/// What Kontor may conclude about a run right now.
///
/// Every uncertainty variant is non-terminal by construction; only
/// [`DerivedRunState::Terminal`] closes a run, and only
/// [`derive_run_state`] with valid [`TerminalEvidence`] can produce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DerivedRunState {
    /// A command was dispatched; the runtime has not confirmed it.
    PendingConfirmation,
    /// Observation agrees with intent and is fresh.
    Confirmed,
    /// The last observation is too old to act on.
    Stale,
    /// Intent and observation disagree.
    Diverged,
    /// The runtime itself could not be reached.
    RuntimeUnavailable,
    /// A native session exists that Kontor did not launch, or whose generation
    /// no longer matches the binding.
    Orphaned,
    /// The process, session or event stream disappeared without a verdict.
    LostContact,
    /// Closed, with evidence.
    Terminal {
        /// How the run closed.
        outcome: TerminalOutcome,
    },
}

impl DerivedRunState {
    /// Whether this conclusion closes the run.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal { .. })
    }

    /// Whether this conclusion expresses uncertainty rather than fact.
    #[must_use]
    pub const fn is_uncertain(self) -> bool {
        matches!(
            self,
            Self::PendingConfirmation
                | Self::Stale
                | Self::Diverged
                | Self::RuntimeUnavailable
                | Self::Orphaned
                | Self::LostContact
        )
    }

    /// The stable spelling used in JSON and SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingConfirmation => "pending_confirmation",
            Self::Confirmed => "confirmed",
            Self::Stale => "stale",
            Self::Diverged => "diverged",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::Orphaned => "orphaned",
            Self::LostContact => "lost_contact",
            Self::Terminal { .. } => "terminal",
        }
    }
}

closed_enum! {
    /// How old the newest trusted observation is.
    Freshness, "Freshness" {
        /// Within the configured confirmation window.
        Fresh => "fresh",
        /// Older than the configured confirmation window.
        Stale => "stale",
        /// Nothing has been confirmed yet.
        Unknown => "unknown",
    }
}

impl Freshness {
    /// Classify the age of the last confirmation.
    #[must_use]
    pub fn evaluate(
        last_confirmed_at: Option<Timestamp>,
        now: Timestamp,
        max_age: jiff::SignedDuration,
    ) -> Self {
        match last_confirmed_at {
            None => Self::Unknown,
            Some(confirmed) if now.duration_since(confirmed) <= max_age => Self::Fresh,
            Some(_) => Self::Stale,
        }
    }
}

closed_enum! {
    /// The transport-level result of the most recent attempt to contact a
    /// runtime. This is evidence about *the channel*, never about the work.
    RuntimeContact, "RuntimeContact" {
        /// The runtime answered.
        Reachable => "reachable",
        /// The runtime could not be reached at all.
        Unavailable => "unavailable",
        /// The runtime answered, but the native process or session is gone.
        ProcessMissing => "process_missing",
        /// The event stream closed without a terminal event.
        StreamClosed => "stream_closed",
    }
}

/// The identity of a native runtime session.
///
/// Uniqueness is scoped to `(runtime_kind, host, generation)`: a native id is
/// only meaningful inside the runtime generation that issued it, so a restarted
/// runtime cannot resurrect a stale binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeRuntimeIdentity {
    /// Runtime family (an open key: the core never branches on its value).
    pub runtime_kind: RuntimeKindKey,
    /// Host or endpoint that owns the generation.
    pub host: ExternalName,
    /// Monotonic generation of that runtime instance.
    pub generation: u64,
    /// Native session identifier inside this generation.
    pub native_id: ExternalId,
}

impl NativeRuntimeIdentity {
    /// Whether two identities name the same session in the same generation.
    #[must_use]
    pub fn same_session(&self, other: &Self) -> bool {
        self == other
    }

    /// Whether `other` is the same session reported by a *different* runtime
    /// generation, which makes the binding orphaned rather than confirmed.
    #[must_use]
    pub fn generation_changed(&self, other: &Self) -> bool {
        self.runtime_kind == other.runtime_kind
            && self.host == other.host
            && self.generation != other.generation
    }
}

/// One trusted observation of a native runtime session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeObservation {
    /// The observed run.
    pub agent_run_id: AgentRunId,
    /// What the runtime reported.
    pub state: ObservedRunState,
    /// Which native session reported it.
    pub identity: NativeRuntimeIdentity,
    /// Cursor of the raw event this observation reduces.
    pub cursor: EventCursor,
    /// When the runtime reported it.
    pub observed_at: Timestamp,
    /// Digest of the canonical raw event.
    pub evidence_hash: ContentHash,
}

/// Where a run's closure evidence lives.
///
/// This is a *pointer into persisted evidence*, not a copy of it. The store
/// resolves it inside the closing transaction and re-proves that it belongs to
/// the run being closed, so a caller cannot hand over a plausible-looking blob
/// and have it accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalEvidenceSource {
    /// A trusted runtime observation, addressed by its local event cursor.
    RuntimeObservation {
        /// The cursor of the stored runtime event.
        cursor: EventCursor,
    },
    /// An explicit operator decision, addressed by its command receipt.
    OperatorAbandon {
        /// The receipt that recorded the abandon decision.
        receipt_id: CommandReceiptId,
    },
}

/// The immutable evidence that closed a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEvidence {
    /// How the run closed.
    pub outcome: TerminalOutcome,
    /// Where the proof is stored.
    pub source: TerminalEvidenceSource,
    /// Digest of that stored evidence. The store compares it with the row it
    /// loads, so citing the wrong event or receipt fails.
    pub evidence_hash: ContentHash,
    /// When the run closed.
    pub closed_at: Timestamp,
}

impl TerminalEvidence {
    /// Validate what can be decided without touching storage.
    ///
    /// # Errors
    /// Returns [`DomainError::MissingAuthority`] when an operator receipt claims
    /// an outcome only a runtime can report. An operator may abandon a run; an
    /// operator may **not** declare it cancelled, parked, succeeded or failed.
    /// Cancellation needs a trusted cancelled observation, and a park request
    /// stays pending until some trusted terminal fact exists.
    pub fn validate(&self) -> DomainResult<()> {
        match self.source {
            TerminalEvidenceSource::OperatorAbandon { .. }
                if self.outcome != TerminalOutcome::Abandoned =>
            {
                Err(DomainError::MissingAuthority {
                    subject: "run closure",
                    rule: "an operator receipt can only evidence an abandoned run",
                })
            }
            _ => Ok(()),
        }
    }

    /// Prove a loaded runtime observation actually evidences this closure.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] when the cited event is not terminal,
    ///   evidences a different outcome, or has a different payload digest.
    /// * [`DomainError::Invalid`] when the run is recorded as closing before the
    ///   evidence was observed.
    pub fn verify_observation(
        &self,
        observed: ObservedRunState,
        observed_at: Timestamp,
        payload_hash: &ContentHash,
    ) -> DomainResult<()> {
        let evidenced =
            observed
                .observed_terminal_outcome()
                .ok_or(DomainError::MissingEvidence {
                    subject: "run closure",
                    rule: "the cited event is not a terminal runtime report",
                })?;
        if evidenced != self.outcome {
            return Err(DomainError::MissingEvidence {
                subject: "run closure",
                rule: "the cited event evidences a different outcome",
            });
        }
        if payload_hash != &self.evidence_hash {
            return Err(DomainError::MissingEvidence {
                subject: "run closure",
                rule: "the cited event has a different payload digest",
            });
        }
        if self.closed_at < observed_at {
            return Err(DomainError::invalid(
                "TerminalEvidence",
                "closes the run before its evidence was observed",
            ));
        }
        Ok(())
    }

    /// Prove a loaded operator receipt actually evidences this closure.
    ///
    /// `closing_revision` is the run revision this closure is writing over. The
    /// receipt must have been decided against that exact revision: an abandon
    /// intent aimed at an earlier revision is a decision about work that has
    /// since moved on.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the receipt is of the wrong kind, targets
    /// another aggregate or revision, cites a different intent digest, or the
    /// closure predates it.
    pub fn verify_abandon(
        &self,
        closing_revision: AggregateRevision,
        facts: &AbandonReceiptFacts,
    ) -> DomainResult<()> {
        self.validate()?;
        facts.verify(
            "run closure",
            closing_revision,
            &self.evidence_hash,
            self.closed_at,
        )
    }
}

/// Where a team run's closure evidence lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamEvidenceSource {
    /// The immutable terminal rows of the team's own child runs.
    ChildEvidence {
        /// The team those children must belong to. The store proves it.
        team_run_id: TeamRunId,
    },
    /// An explicit operator decision, addressed by its command receipt.
    OperatorAbandon {
        /// The receipt that recorded the abandon decision.
        receipt_id: CommandReceiptId,
    },
    /// Every declared role slot settled its final bounded Kontor turn.
    ///
    /// A separate source, and separate on purpose. [`Self::ChildEvidence`] closes
    /// a team because its child *runs* ended; this closes one because Kontor's
    /// own work in every declared slot is finished, which is a different fact
    /// about a different thing. A seat is persistent: its native session is
    /// expected to still be live when the team closes, and reading that as a run
    /// ending — or casting the run terminal to make the arithmetic work — would
    /// be a claim about the runtime that nothing observed.
    ///
    /// The store re-proves it from the immutable `role_turns` rows of this very
    /// team, so a certificate cannot assert closure the rows do not support.
    SettledTurns {
        /// The team whose declared slots must be accounted for. The store proves
        /// it.
        team_run_id: TeamRunId,
    },
    /// Every declared role slot is accounted for by *exactly one* disposition:
    /// a settled turn, or an authorized waiver of a slot that was never bound.
    ///
    /// Distinct from [`Self::SettledTurns`], which can only speak for slots that
    /// produced work. A slot that never got a seat settles nothing, and the two
    /// ways to close such a team without this source are both untrue: invent an
    /// `AgentRun` and cast it terminal, or let the closure skip the slot in
    /// silence. A waiver is neither — it is the frozen template's own permission,
    /// exercised by a role the template authorized, with the evidence it demanded.
    ///
    /// The store re-proves the whole disposition set from the immutable
    /// `role_turns` and `role_slot_waivers` rows of this very team, and recomputes
    /// the digest rather than trusting the one it is handed.
    RoleSlotDispositions {
        /// The team whose declared slots must each carry exactly one source.
        team_run_id: TeamRunId,
    },
}

/// The one way a declared role slot is accounted for at closure.
///
/// "Exactly one" is the point: a slot that both settled a turn and was waived is
/// a contradiction the closure refuses rather than picks a winner from, and a
/// slot with neither is the gap the waiver design exists to name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SlotDisposition {
    /// Kontor finished bounded work in a real bound seat.
    SettledTurn {
        /// The digest of the final settled turn's identifying content.
        evidence_hash: ContentHash,
    },
    /// The slot was never bound and the frozen template's policy excused it.
    WaivedUnbound {
        /// The digest the waiver was recorded under.
        evidence_hash: ContentHash,
    },
}

#[derive(Debug, Serialize)]
struct DispositionDigestInput<'a> {
    schema_version: crate::id::SchemaVersion,
    team_run_id: TeamRunId,
    template_id: crate::id::TeamTemplateId,
    template_version: crate::id::SpecVersion,
    template_hash: &'a ContentHash,
    slots: Vec<DispositionSlotEntry<'a>>,
}

#[derive(Debug, Serialize)]
struct DispositionSlotEntry<'a> {
    slot: &'a crate::id::RoleSlotId,
    disposition: &'a SlotDisposition,
}

/// The canonical digest of a team's role-slot dispositions.
///
/// Lives here, and is the *only* implementation, because two of them would be
/// the whole defect: `kontor-teams` computes it to build a certificate and the
/// store recomputes it to re-prove one, and each deriving its own shape from its
/// own inputs is precisely how they could disagree without either being wrong on
/// its own terms.
///
/// `slots` is in the frozen definition's order and must carry every declared
/// slot exactly once; both callers walk the declaration to build it.
///
/// # Errors
/// [`DomainError`] if the canonical document cannot be built.
pub fn role_slot_disposition_digest(
    schema_version: crate::id::SchemaVersion,
    team_run_id: TeamRunId,
    template_id: crate::id::TeamTemplateId,
    template_version: crate::id::SpecVersion,
    template_hash: &ContentHash,
    slots: &[(crate::id::RoleSlotId, SlotDisposition)],
) -> DomainResult<ContentHash> {
    Ok(
        crate::id::CanonicalDocument::from_serializable(&DispositionDigestInput {
            schema_version,
            team_run_id,
            template_id,
            template_version,
            template_hash,
            slots: slots
                .iter()
                .map(|(slot, disposition)| DispositionSlotEntry { slot, disposition })
                .collect(),
        })?
        .hash()
        .clone(),
    )
}

/// The immutable evidence that closed a team run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTerminalEvidence {
    /// How the team closed.
    pub outcome: TerminalOutcome,
    /// Where the proof is stored.
    pub source: TeamEvidenceSource,
    /// Digest of that stored evidence.
    pub evidence_hash: ContentHash,
    /// When the team closed.
    pub closed_at: Timestamp,
}

/// One child run's immutable terminal row, as a team closure reads it.
///
/// This is the *only* input a computed team outcome is allowed to rest on. It
/// carries the child's own bound closure digest so the team digest transitively
/// covers every piece of evidence underneath it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamChildEvidence {
    /// The child run.
    pub agent_run_id: AgentRunId,
    /// Its terminal (or still-open) lifecycle value.
    pub lifecycle: RunLifecycle,
    /// The child's own closure digest. `None` while the child is still open.
    pub evidence_hash: Option<ContentHash>,
}

/// The canonical digest of a team's child terminal evidence.
///
/// Order-independent by construction: the children are sorted by run id before
/// hashing, so the digest depends on the *set* of immutable child rows and not
/// on the order SQLite happened to return them in. Because each entry includes
/// the child's own `evidence_hash`, substituting a different child closure
/// changes the team digest.
///
/// # Errors
/// Returns [`DomainError`] only if the children cannot be canonicalized, which
/// requires a serialization failure rather than bad data.
pub fn team_child_evidence_digest(children: &[TeamChildEvidence]) -> DomainResult<ContentHash> {
    let mut sorted = children.to_vec();
    sorted.sort_by_key(|child| child.agent_run_id);
    let document =
        crate::id::CanonicalDocument::from_serializable(&TeamChildEvidenceDigestInput {
            schema_version: crate::id::SCHEMA_VERSION,
            children: sorted,
        })?;
    Ok(document.hash().clone())
}

/// The exact shape hashed by [`team_child_evidence_digest`].
#[derive(Debug, Serialize)]
struct TeamChildEvidenceDigestInput {
    schema_version: crate::id::SchemaVersion,
    children: Vec<TeamChildEvidence>,
}

/// The stored command receipt a closure cites, reduced to the facts it may use.
///
/// The store loads this inside the closing transaction; nothing here is supplied
/// by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbandonReceiptFacts {
    /// Whether the stored receipt kind is `abandon_run`.
    pub kind_is_abandon: bool,
    /// Whether the stored target names the aggregate being closed.
    pub targets_aggregate: bool,
    /// The aggregate revision the intent was computed against.
    pub target_revision: AggregateRevision,
    /// The stored canonical intent digest.
    pub intent_hash: ContentHash,
    /// When the receipt was recorded.
    pub recorded_at: Timestamp,
}

impl AbandonReceiptFacts {
    /// Prove a loaded receipt authorizes abandoning this exact aggregate
    /// revision with this exact digest.
    ///
    /// # Errors
    /// * [`DomainError::MissingAuthority`] — wrong kind, wrong aggregate, or a
    ///   revision other than the one being closed.
    /// * [`DomainError::MissingEvidence`] — a different intent digest.
    /// * [`DomainError::Invalid`] — the closure predates the receipt.
    pub fn verify(
        &self,
        subject: &'static str,
        closing_revision: AggregateRevision,
        evidence_hash: &ContentHash,
        closed_at: Timestamp,
    ) -> DomainResult<()> {
        if !self.kind_is_abandon {
            return Err(DomainError::MissingAuthority {
                subject,
                rule: "the cited receipt is not an abandon command",
            });
        }
        if !self.targets_aggregate {
            return Err(DomainError::MissingAuthority {
                subject,
                rule: "the cited receipt targets a different aggregate",
            });
        }
        // An abandon decision is made against a specific revision. Closing a
        // revision the operator never saw would let a stale decision close work
        // that has moved on since.
        if self.target_revision != closing_revision {
            return Err(DomainError::MissingAuthority {
                subject,
                rule: "the cited receipt targets a different revision of this aggregate",
            });
        }
        if &self.intent_hash != evidence_hash {
            return Err(DomainError::MissingEvidence {
                subject,
                rule: "the cited receipt has a different intent digest",
            });
        }
        if closed_at < self.recorded_at {
            return Err(DomainError::invalid(
                "TerminalEvidence",
                "closes the aggregate before its receipt was recorded",
            ));
        }
        Ok(())
    }
}

impl TeamTerminalEvidence {
    /// Validate what can be decided without touching storage.
    ///
    /// # Errors
    /// Returns [`DomainError::MissingAuthority`] when an operator receipt claims
    /// anything other than `abandoned`.
    pub fn validate(&self) -> DomainResult<()> {
        match self.source {
            TeamEvidenceSource::OperatorAbandon { .. }
                if self.outcome != TerminalOutcome::Abandoned =>
            {
                Err(DomainError::MissingAuthority {
                    subject: "team closure",
                    rule: "an operator receipt can only evidence an abandoned team",
                })
            }
            _ => Ok(()),
        }
    }

    /// Prove a child-evidence closure against the team's own persisted children.
    ///
    /// Both the outcome *and* the digest are recomputed here. Recomputing only
    /// the outcome would still let a caller persist an arbitrary
    /// `evidence_hash`, leaving the stored evidence unbound to anything.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] — the evidence names another team, the
    ///   children compute a different outcome, or the digest does not match the
    ///   children.
    /// * [`DomainError::MissingAuthority`] — the source is an operator receipt,
    ///   which this path does not verify.
    pub fn verify_children(
        &self,
        team_run_id: TeamRunId,
        children: &[TeamChildEvidence],
    ) -> DomainResult<()> {
        let TeamEvidenceSource::ChildEvidence { team_run_id: cited } = self.source else {
            return Err(DomainError::MissingAuthority {
                subject: "team closure",
                rule: "this closure is not evidenced by child runs",
            });
        };
        if cited != team_run_id {
            return Err(DomainError::MissingEvidence {
                subject: "team closure",
                rule: "the cited child evidence belongs to a different team",
            });
        }
        let lifecycles: Vec<RunLifecycle> = children.iter().map(|child| child.lifecycle).collect();
        if reduce_team_outcome(&lifecycles)? != self.outcome {
            return Err(DomainError::MissingEvidence {
                subject: "team closure",
                rule: "the children compute a different outcome",
            });
        }
        if team_child_evidence_digest(children)? != self.evidence_hash {
            return Err(DomainError::MissingEvidence {
                subject: "team closure",
                rule: "the digest does not match the team's own child evidence",
            });
        }
        Ok(())
    }

    /// Prove an operator-abandon closure against the loaded receipt.
    ///
    /// `closing_revision` is the team revision this closure writes over, exactly
    /// as for an agent run.
    ///
    /// # Errors
    /// As [`AbandonReceiptFacts::verify`], plus [`DomainError::MissingAuthority`]
    /// when the source is not an operator receipt.
    pub fn verify_abandon(
        &self,
        closing_revision: AggregateRevision,
        facts: &AbandonReceiptFacts,
    ) -> DomainResult<()> {
        self.validate()?;
        if !matches!(self.source, TeamEvidenceSource::OperatorAbandon { .. }) {
            return Err(DomainError::MissingAuthority {
                subject: "team closure",
                rule: "this closure is not evidenced by an operator receipt",
            });
        }
        facts.verify(
            "team closure",
            closing_revision,
            &self.evidence_hash,
            self.closed_at,
        )
    }
}

/// Decide a non-terminal team-run advance.
///
/// Closure is deliberately not reachable from here: a terminal value is only
/// ever written by [`plan_team_closure`], which requires evidence.
///
/// # Errors
/// * [`DomainError::Terminal`] — the team already closed.
/// * [`DomainError::RevisionConflict`] — the caller's expectation is stale.
/// * [`DomainError::IllegalTransition`] — the move is not in the declared table.
pub fn plan_team_advance(
    current: RunLifecycle,
    stored_revision: AggregateRevision,
    expected: AggregateRevision,
    to: RunLifecycle,
) -> DomainResult<AggregateRevision> {
    if current.is_terminal() {
        return Err(DomainError::Terminal {
            subject: "team run",
        });
    }
    stored_revision.expect("team run", expected)?;
    if !current.can_advance_to(to) {
        return Err(DomainError::IllegalTransition {
            subject: "team run",
            from: current.as_str(),
            to: to.as_str(),
        });
    }
    stored_revision.next()
}

/// Decide a team-run closure against the evidence the store loaded for it.
///
/// # Errors
/// * [`DomainError::Terminal`] — the team already closed.
/// * [`DomainError::RevisionConflict`] — the caller's expectation is stale.
/// * Anything [`TeamTerminalEvidence::verify_children`] or
///   [`TeamTerminalEvidence::verify_abandon`] returns.
pub fn plan_team_closure(
    current: RunLifecycle,
    stored_revision: AggregateRevision,
    expected: AggregateRevision,
    team_run_id: TeamRunId,
    evidence: &TeamTerminalEvidence,
    children: &[TeamChildEvidence],
    receipt: Option<&AbandonReceiptFacts>,
) -> DomainResult<AggregateRevision> {
    evidence.validate()?;
    if current.is_terminal() {
        return Err(DomainError::Terminal {
            subject: "team run",
        });
    }
    stored_revision.expect("team run", expected)?;
    match evidence.source {
        TeamEvidenceSource::ChildEvidence { .. } => {
            evidence.verify_children(team_run_id, children)?
        }
        TeamEvidenceSource::OperatorAbandon { .. } => {
            let facts = receipt.ok_or(DomainError::MissingEvidence {
                subject: "team closure",
                rule: "the cited receipt is not stored in this project",
            })?;
            evidence.verify_abandon(stored_revision, facts)?;
        }
        // Deliberately *not* verified against child runs. The whole point of
        // this source is that the children are expected to still be live: a
        // persistent seat outlives the Kontor work taken in it. What must be
        // re-proved is that every declared role slot settled its final bounded
        // turn, and that is a question about the `role_turns` rows rather than
        // about run lifecycles — so the store proves it, where those rows are,
        // and this function refuses to pretend it can.
        TeamEvidenceSource::SettledTurns { team_run_id: cited } => {
            if cited != team_run_id {
                return Err(DomainError::MissingEvidence {
                    subject: "team closure",
                    rule: "the settled-turn evidence names another team run",
                });
            }
        }
        // Same shape, same reason: which slots are disposed of, and how, is a
        // question about `role_turns` and `role_slot_waivers` rows. The store
        // answers it where those rows are.
        TeamEvidenceSource::RoleSlotDispositions { team_run_id: cited } => {
            if cited != team_run_id {
                return Err(DomainError::MissingEvidence {
                    subject: "team closure",
                    rule: "the role-slot disposition evidence names another team run",
                });
            }
        }
    }
    stored_revision.next()
}

/// Reduce a team's outcome from its children's immutable terminal rows.
///
/// A team succeeds only if it actually had children and every one of them
/// succeeded. Anything else is computed, never asserted: the caller cannot claim
/// an outcome the children do not support.
///
/// # Errors
/// Returns [`DomainError::MissingEvidence`] when the team has no children or one
/// of them is still open.
pub fn reduce_team_outcome(children: &[RunLifecycle]) -> DomainResult<TerminalOutcome> {
    if children.is_empty() {
        return Err(DomainError::MissingEvidence {
            subject: "team closure",
            rule: "a team with no child runs has no evidence to close on",
        });
    }
    if children.iter().any(|child| !child.is_terminal()) {
        return Err(DomainError::MissingEvidence {
            subject: "team closure",
            rule: "every child run must be terminal before the team closes",
        });
    }
    if children.contains(&RunLifecycle::Failed) {
        return Ok(TerminalOutcome::Failed);
    }
    if children.contains(&RunLifecycle::Cancelled) {
        return Ok(TerminalOutcome::Cancelled);
    }
    if children.contains(&RunLifecycle::Parked) {
        return Ok(TerminalOutcome::Parked);
    }
    Ok(TerminalOutcome::Succeeded)
}

/// Everything needed to derive the current conclusion about a run.
#[derive(Debug, Clone)]
pub struct RunDerivation<'a> {
    /// What Kontor asked for.
    pub desired: DesiredRunState,
    /// The newest trusted observation, if any.
    pub observation: Option<&'a RuntimeObservation>,
    /// The binding Kontor believes it owns, if any.
    pub binding: Option<&'a NativeRuntimeIdentity>,
    /// How old the last confirmation is.
    pub freshness: Freshness,
    /// The transport result of the most recent contact attempt.
    pub contact: RuntimeContact,
    /// Validated closure evidence, if the run is closed.
    pub terminal: Option<&'a TerminalEvidence>,
}

/// Reduce intent, observation, contact and freshness into a conclusion.
///
/// Terminal is reachable **only** through `terminal` evidence that passes
/// [`TerminalEvidence::validate`]. Every other input — a missing process, a
/// closed stream, an unreachable runtime, a stale observation — yields an
/// uncertainty variant that retains the run's last known lifecycle for the
/// caller to display.
///
/// # Errors
/// Returns [`DomainError`] when closure evidence is present but invalid.
pub fn derive_run_state(input: &RunDerivation<'_>) -> DomainResult<DerivedRunState> {
    if let Some(evidence) = input.terminal {
        evidence.validate()?;
        return Ok(DerivedRunState::Terminal {
            outcome: evidence.outcome,
        });
    }

    // Uncertainty about the channel is decided before anything else: an
    // unreachable runtime cannot confirm or deny intent.
    match input.contact {
        RuntimeContact::Unavailable => return Ok(DerivedRunState::RuntimeUnavailable),
        RuntimeContact::ProcessMissing | RuntimeContact::StreamClosed => {
            return Ok(DerivedRunState::LostContact);
        }
        RuntimeContact::Reachable => {}
    }

    // Nothing trustworthy has arrived yet, whatever the intent was.
    let Some(observation) = input.observation else {
        return Ok(DerivedRunState::PendingConfirmation);
    };

    if let Some(binding) = input.binding {
        if binding.generation_changed(&observation.identity)
            || !binding.same_session(&observation.identity)
        {
            return Ok(DerivedRunState::Orphaned);
        }
    } else {
        // A native session reported against a run Kontor has no binding for.
        return Ok(DerivedRunState::Orphaned);
    }

    if input.freshness != Freshness::Fresh {
        return Ok(DerivedRunState::Stale);
    }

    let diverged = match input.desired {
        DesiredRunState::NoIntent => !matches!(observation.state, ObservedRunState::Unknown),
        DesiredRunState::RunRequested => matches!(
            observation.state,
            ObservedRunState::Cancelled | ObservedRunState::Unknown
        ),
        DesiredRunState::CancelRequested | DesiredRunState::AbandonRequested => matches!(
            observation.state,
            ObservedRunState::Running
                | ObservedRunState::Launching
                | ObservedRunState::Queued
                | ObservedRunState::WaitingInput
        ),
        DesiredRunState::ParkRequested => matches!(observation.state, ObservedRunState::Running),
    };
    if diverged {
        return Ok(DerivedRunState::Diverged);
    }

    if observation.state == ObservedRunState::Unknown {
        return Ok(DerivedRunState::PendingConfirmation);
    }
    Ok(DerivedRunState::Confirmed)
}

/// The full, orthogonal state of one run as Kontor stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunProjection {
    /// The run's own lifecycle.
    pub lifecycle: RunLifecycle,
    /// What Kontor asked for.
    pub desired: DesiredRunState,
    /// What the runtime last reported.
    pub observed: ObservedRunState,
    /// What Kontor concluded.
    pub derived: DerivedRunState,
    /// When the last trusted confirmation arrived.
    pub last_confirmed_at: Option<Timestamp>,
    /// Cursor of the newest reduced event.
    pub last_cursor: Option<EventCursor>,
}

impl RunProjection {
    /// Whether an observation carrying `incoming` may reduce state.
    ///
    /// Only a *strictly newer* native sequence may move observed/derived state,
    /// the reduced cursor and the revision. A replay or a late-arriving older
    /// event is still appended as evidence, but it must leave the projection
    /// exactly as it was — otherwise a duplicated delivery silently rewrites
    /// history.
    #[must_use]
    pub const fn may_reduce(last_applied: Option<u64>, incoming: u64) -> bool {
        match last_applied {
            None => true,
            Some(last) => incoming > last,
        }
    }

    /// Whether this run may be resumed in place.
    ///
    /// It may not: recovery always creates a successor run with
    /// `parent_agent_run_id` pointing at the closed one.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.lifecycle.is_terminal()
    }

    /// Guard every mutation of a closed run.
    ///
    /// # Errors
    /// Returns [`DomainError::Terminal`] when the run is already closed.
    pub fn ensure_open(&self, subject: &'static str) -> DomainResult<()> {
        if self.is_closed() {
            return Err(DomainError::Terminal { subject });
        }
        Ok(())
    }
}

closed_enum! {
    /// Durable lifecycle shared by topology nodes and their seat bindings.
    TopologyLifecycle, "TopologyLifecycle" {
        /// Available for materialization and reconciliation.
        Active => "active",
        /// Preserved but no longer eligible for new work.
        Retired => "retired",
        /// Historical and immutable.
        Archived => "archived",
    }
}

impl TopologyLifecycle {
    /// Whether the lifecycle may still be mutated.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Archived)
    }
}

closed_enum! {
    /// Derived placement condition of one logical topology node.
    PlacementState, "PlacementState" {
        /// No native container is bound yet.
        Unbound => "unbound",
        /// Exact native identity has been read back.
        Bound => "bound",
        /// Desired and observed native state differ.
        Drifted => "drifted",
        /// Placement refused and requires explicit reconciliation.
        PlacementBlocked => "placement_blocked",
    }
}

/// One durable logical element in a project session-topology instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTopologyNode {
    /// Durable node identity.
    pub id: TopologyNodeId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Epic/goal scope when this node belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mini_project_id: Option<MiniProjectId>,
    /// Exact immutable topology specification revision/hash.
    pub topology: TopologySnapshot,
    /// Data-defined kind declared by that specification.
    pub kind: TopologyKindKey,
    /// Logical parent; absent only for the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<TopologyNodeId>,
    /// The delivery task this node serves, for the task-scoped kinds.
    ///
    /// How admission locates the node before any seat binding exists. Absent
    /// for every node that is not a delivery: a project root, an epic and a
    /// control plane serve no task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Logical lifecycle.
    pub lifecycle: TopologyLifecycle,
    /// Derived native-placement state.
    pub placement: PlacementState,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Creation instant.
    pub created_at: Timestamp,
    /// Last mutation instant.
    pub updated_at: Timestamp,
}

/// One persistent role session owned by a topology node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatBinding {
    /// Binding identity.
    pub id: SeatBindingId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Hosting topology node.
    pub topology_node_id: TopologyNodeId,
    /// Stable role-slot address within that node.
    pub role_slot_id: RoleSlotId,
    /// Typed standard role snapshot.
    pub role: CatalogRoleRef,
    /// Optional delivery task reference for TSW seats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Optional delivery TeamRun reference for TSW seats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_run_id: Option<TeamRunId>,
    /// Binding lifecycle.
    pub lifecycle: TopologyLifecycle,
    /// The instant this seat must have been observed attached by, fixed when
    /// the seat was created (OP-REQ-039a).
    ///
    /// Persisted rather than derived. A deadline recomputed at read time moves
    /// whenever the grace constant does, which retroactively forgives every
    /// seat that already failed to attach — the thirteen-hour phantom is
    /// exactly that forgiveness applied once too often.
    pub attach_deadline: Timestamp,
    /// When this seat was last observed attached to its runtime session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attached_at: Option<Timestamp>,
    /// When this seat last produced *observed activity* (OP-REQ-039c).
    ///
    /// Written only from an observed runtime event or turn position. A
    /// successful readback may confirm attachment and must never land here: a
    /// generic confirmation is Kontor asking, not the seat working, and reading
    /// one as activity makes a wedged seat look busy for as long as anything
    /// keeps polling it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<Timestamp>,
    /// The exact owning epic seat, when this seat has one.
    ///
    /// Orphanhood is derived from *this* seat's Kontor lifecycle, never from a
    /// runtime's own parent field: a runtime that has lost its parent is the
    /// least reliable witness to whether the parent is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_seat_binding_id: Option<SeatBindingId>,
    /// When the seat was deliberately released or reaped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<Timestamp>,
    /// The seat that replaced this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<SeatBindingId>,
    /// What the runtime last said about itself.
    ///
    /// Carried so an escalation can quote it, and deliberately not consulted
    /// when concluding attachment: `running` is a claim about a process, not
    /// evidence that a seat is doing anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_reported: Option<ObservedRunState>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Creation instant.
    pub created_at: Timestamp,
    /// Last mutation instant.
    pub updated_at: Timestamp,
}

impl SeatBinding {
    /// Whether this binding occupies the unique `(node, role slot)` key.
    #[must_use]
    pub const fn is_non_terminal(&self) -> bool {
        matches!(self.lifecycle, TopologyLifecycle::Active)
    }

    /// Whether this seat's closure orphans the seats it owns.
    ///
    /// A replaced seat counts as closed even while its row is still active: the
    /// work moved to the successor, so anything still steering by this seat is
    /// steering by nothing.
    #[must_use]
    pub const fn closes_children(&self) -> bool {
        self.replaced_by.is_some()
            || self.released_at.is_some()
            || !matches!(self.lifecycle, TopologyLifecycle::Active)
    }

    /// Assemble the OP-REQ-039 observation from persisted evidence alone.
    ///
    /// `parent_closed` is the caller's join, because it is a fact about
    /// *another* row: whoever holds the store looks up
    /// [`Self::parent_seat_binding_id`] and asks that seat
    /// [`Self::closes_children`]. A seat with no parent is not an orphan — it
    /// is a root, and roots answer to Kontor lifecycle instead.
    #[must_use]
    pub const fn attachment_observation(&self, parent_closed: bool) -> SeatAttachmentObservation {
        SeatAttachmentObservation {
            attach_deadline: self.attach_deadline,
            last_attached_at: self.last_attached_at,
            last_activity_at: self.last_activity_at,
            parent_closed,
            released: self.released_at.is_some(),
            runtime_reported: match self.runtime_reported {
                Some(state) => state,
                // Nothing heard is not a healthy report, and it is not an
                // unhealthy one either. It is carried through as unknown so the
                // conclusion rests on the recorded observations above.
                None => ObservedRunState::Unknown,
            },
        }
    }
}

crate::closed_enum! {
    /// What a runtime read back a container as.
    ///
    /// Stored beside the shape that was desired, because "we asked for a
    /// project and got a workspace" is only detectable if both are recorded.
    ObservedContainerKind, "ObservedContainerKind" {
        /// A native root/project.
        Project => "project",
        /// A container below a native root.
        Workspace => "workspace",
    }
}

/// The one current native container bound to a topology node.
///
/// There is no history here on purpose. A node that is rebound has exactly one
/// binding, and the native id it used to hold is evidence in the event stream —
/// a second row would be indistinguishable from a live second placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeContainerBinding {
    /// The node that owns the container.
    pub topology_node_id: TopologyNodeId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The runtime-issued binding id this placement was made under.
    pub container_binding_id: ExternalId,
    /// The exact native identity, in the four parts that make it one.
    ///
    /// A native id means nothing on its own: it is only that container inside
    /// one generation, on one host, of one runtime family. Reconciling on any
    /// subset of these matches a container a restart has already replaced.
    pub identity: NativeRuntimeIdentity,
    /// What the runtime said the container is, read back rather than assumed.
    pub observed_kind: ObservedContainerKind,
    /// The container's canonical working directory, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_cwd: Option<ExternalName>,
    /// When the binding was established.
    pub bound_at: Timestamp,
    /// When it was last confirmed against the runtime.
    ///
    /// Distinct from `bound_at` so a stale binding reads as stale instead of
    /// looking as fresh as the day it was made.
    pub last_readback_at: Timestamp,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
}

/// Persisted adaptive-admission state for one MiniProject.
///
/// The scheduler still owns the decision. This record only makes its current
/// window and replay cursor survive restart/export/restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveAdmissionState {
    /// Owning project.
    pub project_id: ProjectId,
    /// MiniProject whose active TeamRun envelope budget this controls.
    pub mini_project_id: MiniProjectId,
    /// Current maximum new admissions for one scheduling pass.
    pub current_window: u32,
    /// Distinct clean observations accumulated at this width.
    pub clean_observation_streak: u32,
    /// Last observation already applied; replay must not advance state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observation_id: Option<ExternalId>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Last mutation instant.
    pub updated_at: Timestamp,
}

impl AdaptiveAdmissionState {
    /// Validate persisted bounds without re-implementing scheduler policy.
    ///
    /// # Errors
    /// Rejects a zero/out-of-range window or a streak larger than the two
    /// observations Operational requires before growth.
    pub fn validate(&self, floor: u32, ceiling: u32) -> DomainResult<()> {
        if floor == 0
            || floor > ceiling
            || self.current_window < floor
            || self.current_window > ceiling
            || self.clean_observation_streak > 1
        {
            return Err(DomainError::invalid(
                "AdaptiveAdmissionState",
                "is outside the configured window or streak bounds",
            ));
        }
        Ok(())
    }
}
