//! The bounded parked-recovery state machine.
//!
//! ```text
//! OPEN -> DETERMINISTIC_REPAIR -> [ADVISOR] -> [COMMITTEE] -> FOLLOWUP*
//!                                                          -> RECOVERED
//!  (any non-terminal state)                                -> NEEDS_HUMAN
//! ```
//!
//! Every arrow is bounded and every bound is a constant, not a setting: one
//! advisor, one committee, two effective follow-ups. An episode that runs out of
//! moves escalates; it does not quietly keep trying, and it does not widen its
//! own budget.
//!
//! Four properties this module exists to hold, none of which are comments:
//!
//! * **A parked run is never resumed in place.** Nothing here can return the
//!   parked run to work. A follow-up carries a *successor* run id, and
//!   [`plan`] refuses one that is the parked run itself.
//! * **Advice is not authority.** [`RecoveryAction::Advisor`] and
//!   [`RecoveryAction::Committee`] cannot carry a successor run, so the one step
//!   that starts work is unrepresentable from a read-only consultation. They
//!   also touch no counter: an advisor cannot reset a rejection stream by being
//!   consulted. Approval is refused at the other end too — the evaluator rejects
//!   [`crate::model::AuthoritySource::RecoveryAdvice`] outright.
//! * **A refused dispatch is free.** Only a follow-up that was actually
//!   dispatched consumes budget. A preflight that refused it changed nothing, so
//!   charging for it would spend the episode on attempts that never ran — while
//!   a dispatch that *was* accepted always counts, whatever it then produced.
//! * **Replay cannot double-spend.** Every transition is revision-checked
//!   against the episode it was computed from, so a restart that re-runs the
//!   same step finds the revision has moved and is refused. The steps themselves
//!   are append-only.

use kontor_core::id::{AgentRunId, AggregateRevision, ContentHash, Timestamp};
use kontor_core::{DomainError, DomainResult};

use crate::model::{EscalationCause, RecoveryEpisode, RecoveryStatus, RecoveryStepKind};

/// How many advisor consultations one episode may have.
pub const MAX_ADVISOR_CONSULTATIONS: u32 = 1;

/// How many committees one episode may convene.
pub const MAX_COMMITTEE_CONSULTATIONS: u32 = 1;

/// How many *dispatched* follow-ups one episode may have.
pub const MAX_EFFECTIVE_FOLLOWUPS: u32 = 2;

/// What a recovery episode is being asked to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Inspect and, where it is safe to, repair deterministically.
    ///
    /// `safe` is the caller's classification of the state it found. An unsafe
    /// state is not repaired on a best guess: it escalates.
    DeterministicRepair {
        /// Whether the state was safe to act on at all.
        safe: bool,
    },
    /// Consult an advisor. Read-only, and available once.
    Advisor,
    /// Convene a committee. Read-only, and available once.
    Committee,
    /// Dispatch a follow-up to a linked successor run.
    Followup {
        /// Whether the dispatch was actually accepted by the runtime.
        ///
        /// `false` is a refused preflight: nothing ran, so nothing is charged.
        dispatched: bool,
        /// The successor run this follow-up runs as. Never the parked run.
        successor: AgentRunId,
    },
    /// Declare the work recovered.
    Recover,
    /// Hand the episode to a human, naming which of the five causes applies.
    Escalate(EscalationCause),
}

impl RecoveryAction {
    /// The step kind this action appends.
    #[must_use]
    pub const fn step_kind(&self) -> RecoveryStepKind {
        match self {
            Self::DeterministicRepair { .. } => RecoveryStepKind::DeterministicRepair,
            Self::Advisor => RecoveryStepKind::Advisor,
            Self::Committee => RecoveryStepKind::Committee,
            Self::Followup { .. } => RecoveryStepKind::FollowupExecution,
            // A recovery is the closing step of whatever produced it, and an
            // escalation is its own step; both are recorded as escalation-class
            // bookkeeping only when they end the episode.
            Self::Recover | Self::Escalate(_) => RecoveryStepKind::Escalation,
        }
    }
}

/// One revision-checked request to move an episode forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRequest {
    /// The revision the caller believes is current.
    pub expected_revision: AggregateRevision,
    /// What to do.
    pub action: RecoveryAction,
    /// Digest of what the step was given.
    pub input_hash: ContentHash,
    /// Digest of what it produced, once it has produced anything.
    pub output_hash: Option<ContentHash>,
    /// When it happened.
    pub occurred_at: Timestamp,
}

/// The episode state one accepted action produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryTransition {
    /// Where the episode now is.
    pub status: RecoveryStatus,
    /// The step to append.
    pub step: RecoveryStepKind,
    /// Whether the advisor budget is now spent.
    pub advisor_used: bool,
    /// Whether the committee budget is now spent.
    pub committee_used: bool,
    /// How many follow-ups have now actually been dispatched.
    pub effective_followups: u32,
    /// The episode's latest linked successor run, once one exists.
    ///
    /// Cumulative: it carries forward across steps that dispatched nothing.
    pub successor_agent_run_id: Option<AgentRunId>,
    /// The successor *this step* dispatched, if it dispatched one.
    ///
    /// Deliberately distinct from [`RecoveryTransition::successor_agent_run_id`].
    /// The step row records this one, so a refused dispatch records no run at
    /// all rather than re-recording the previous attempt's — which would both
    /// misreport what happened and collide with the successor uniqueness the
    /// schema enforces.
    pub dispatched_successor: Option<AgentRunId>,
    /// Why it escalated, when it did.
    pub escalation_cause: Option<EscalationCause>,
    /// When it closed, when it did.
    pub closed_at: Option<Timestamp>,
}

/// Plan the next state of a recovery episode.
///
/// Pure: it decides, it does not write. The store applies the returned
/// transition together with the appended step in one transaction, so a step and
/// the state it produced cannot exist apart.
///
/// # Errors
/// * [`DomainError::Terminal`] when the episode has already closed.
/// * [`DomainError::RevisionConflict`] when the episode moved underneath the
///   caller — which is how a replayed advisor, committee or follow-up is
///   refused instead of being spent twice.
/// * [`DomainError::IllegalTransition`] when the action is not available from
///   the current status.
/// * [`DomainError::Invalid`] when a bounded budget is exhausted, when a
///   follow-up names the parked run as its own successor, or when it reuses the
///   successor an earlier follow-up already dispatched.
pub fn plan(
    episode: &RecoveryEpisode,
    request: &RecoveryRequest,
) -> DomainResult<RecoveryTransition> {
    if episode.status.is_terminal() {
        return Err(DomainError::Terminal {
            subject: "recovery episode",
        });
    }
    episode
        .revision
        .expect("recovery episode", request.expected_revision)?;

    let illegal = |to: &'static str| DomainError::IllegalTransition {
        subject: "recovery episode",
        from: episode.status.as_str(),
        to,
    };
    let settled = RecoveryTransition {
        status: episode.status,
        step: request.action.step_kind(),
        advisor_used: episode.advisor_used,
        committee_used: episode.committee_used,
        effective_followups: episode.effective_followups,
        successor_agent_run_id: episode.successor_agent_run_id,
        dispatched_successor: None,
        escalation_cause: None,
        closed_at: None,
    };

    match &request.action {
        // Deterministic inspection and repair is the first move and happens
        // once. An unsafe state is not a thing to repair harder: it is one of
        // the five escalation causes, and it is raised here rather than left to
        // a caller that might decide to try anyway.
        RecoveryAction::DeterministicRepair { safe } => {
            if episode.status != RecoveryStatus::Open {
                return Err(illegal(RecoveryStatus::DeterministicRepair.as_str()));
            }
            if *safe {
                Ok(RecoveryTransition {
                    status: RecoveryStatus::DeterministicRepair,
                    ..settled
                })
            } else {
                Ok(escalated(
                    &settled,
                    EscalationCause::UnsafeState,
                    request.occurred_at,
                ))
            }
        }

        // Read-only consultations. Each once, each only after the deterministic
        // pass has actually happened — consulting an advisor about a state
        // nobody has inspected is how an episode spends its budget on guesses.
        // The spent-budget check comes before the status check on purpose. A
        // second consultation is refused as "already spent" whatever state the
        // episode has since moved to, so the refusal an operator reads names
        // the bound rather than the arrow.
        RecoveryAction::Advisor => {
            if episode.advisor_used {
                return Err(DomainError::invalid(
                    "recovery episode",
                    "the advisor consultation for this episode is already spent",
                ));
            }
            if episode.status != RecoveryStatus::DeterministicRepair {
                return Err(illegal(RecoveryStatus::Advisor.as_str()));
            }
            Ok(RecoveryTransition {
                status: RecoveryStatus::Advisor,
                advisor_used: true,
                ..settled
            })
        }
        RecoveryAction::Committee => {
            if episode.committee_used {
                return Err(DomainError::invalid(
                    "recovery episode",
                    "the committee for this episode is already convened",
                ));
            }
            if !matches!(
                episode.status,
                RecoveryStatus::DeterministicRepair | RecoveryStatus::Advisor
            ) {
                return Err(illegal(RecoveryStatus::Committee.as_str()));
            }
            Ok(RecoveryTransition {
                status: RecoveryStatus::Committee,
                committee_used: true,
                ..settled
            })
        }

        RecoveryAction::Followup {
            dispatched,
            successor,
        } => {
            if episode.status == RecoveryStatus::Open {
                return Err(illegal(RecoveryStatus::Followup.as_str()));
            }
            if *successor == episode.parked_agent_run_id {
                return Err(DomainError::invalid(
                    "recovery follow-up",
                    "a follow-up runs as a successor, never as the parked run",
                ));
            }
            // Each dispatch is its own attempt, so it gets its own run. Handing
            // back the run the previous follow-up already used would spend two
            // of a two-deep budget on one session — a second ledger entry for
            // work that is really the first attempt continuing. With the budget
            // capped at two, the episode's current successor is the only run a
            // second follow-up could reuse, so this one comparison is the whole
            // rule; the store re-proves it against every step it has recorded,
            // and the schema makes it unrepresentable.
            if episode.successor_agent_run_id == Some(*successor) {
                return Err(DomainError::invalid(
                    "recovery follow-up",
                    "each follow-up dispatches its own successor, not the one already running",
                ));
            }
            if episode.effective_followups >= MAX_EFFECTIVE_FOLLOWUPS {
                return Err(DomainError::invalid(
                    "recovery episode",
                    "the follow-up budget for this episode is exhausted",
                ));
            }
            if *dispatched {
                Ok(RecoveryTransition {
                    status: RecoveryStatus::Followup,
                    effective_followups: episode.effective_followups + 1,
                    successor_agent_run_id: Some(*successor),
                    dispatched_successor: Some(*successor),
                    ..settled
                })
            } else {
                // Refused before it could do anything. The attempt is still
                // appended as a step — an audit needs to see it was tried — but
                // it buys no state change and no successor.
                Ok(settled)
            }
        }

        RecoveryAction::Recover => {
            if episode.status == RecoveryStatus::Open {
                return Err(illegal(RecoveryStatus::Recovered.as_str()));
            }
            Ok(RecoveryTransition {
                status: RecoveryStatus::Recovered,
                closed_at: Some(request.occurred_at),
                ..settled
            })
        }

        RecoveryAction::Escalate(cause) => Ok(escalated(&settled, *cause, request.occurred_at)),
    }
}

/// The one construction of a `needs_human` transition.
///
/// Every escalation goes through here, so [`RecoveryStatus::NeedsHuman`] cannot
/// be reached without one of the five [`EscalationCause`] values attached.
fn escalated(
    settled: &RecoveryTransition,
    cause: EscalationCause,
    occurred_at: Timestamp,
) -> RecoveryTransition {
    RecoveryTransition {
        status: RecoveryStatus::NeedsHuman,
        escalation_cause: Some(cause),
        closed_at: Some(occurred_at),
        ..settled.clone()
    }
}
