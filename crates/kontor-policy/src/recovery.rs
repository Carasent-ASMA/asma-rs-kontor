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

use kontor_core::id::{
    AgentRunId, AggregateRevision, BoundedText, ContentHash, RoleKey, Timestamp,
};
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

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
    /// Hand the episode to a human, naming which of the five causes applies and
    /// what the operator is being asked to confirm.
    Escalate(Escalation),
}

/// What an escalation must carry before a human is asked anything.
///
/// OP-REQ-036: human attention is a last resort, and an entry into
/// [`RecoveryStatus::NeedsHuman`] states a **recommended resolution** with its
/// author. The rule exists because the expensive failure is not being asked — it
/// is being asked a bare question. An operator handed "this is stuck" has to
/// reconstruct what was already tried and invent an answer; an operator handed
/// "this is stuck, here is what I would do, here is what I already tried" has
/// only to agree or not.
///
/// The *tried deliberation path* is deliberately **not** a field. The episode
/// already records it — `advisor_used`, `committee_used`, `effective_followups`
/// and the appended steps — so asking a caller to restate it would invite a
/// second, unverified account of the same facts, and the one place they could
/// disagree is the one place an operator is relying on them.
/// [`DeliberationPath::of`] derives it instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Escalation {
    /// Which of the five causes applies.
    pub cause: EscalationCause,
    /// What the author believes should happen, in enough words to act on.
    pub recommendation: BoundedText,
    /// The role that is recommending it.
    ///
    /// A recommendation nobody is named for is an anonymous suggestion, and an
    /// operator cannot weigh it against what else they know.
    pub recommended_by: RoleKey,
}

/// What was already tried before a human was asked.
///
/// Derived from the episode rather than declared, so it cannot disagree with the
/// episode's own steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliberationPath {
    /// Whether deterministic inspection and repair ran.
    pub deterministic_repair: bool,
    /// Whether the one advisor consultation was spent.
    pub advisor: bool,
    /// Whether the one committee was convened.
    pub committee: bool,
    /// How many follow-ups were actually dispatched.
    pub followups: u32,
}

impl DeliberationPath {
    /// Read the path an episode has actually walked.
    #[must_use]
    pub const fn of(episode: &RecoveryEpisode) -> Self {
        Self {
            // Every route out of `Open` other than escalation passes through the
            // deterministic step, so anything past `Open` has run it.
            deterministic_repair: !matches!(episode.status, RecoveryStatus::Open),
            advisor: episode.advisor_used,
            committee: episode.committee_used,
            followups: episode.effective_followups,
        }
    }

    /// Whether anything at all was tried before the human was reached.
    ///
    /// Not a refusal on its own: [`EscalationCause::UnsafeState`] is exactly the
    /// case where trying *is* the mistake. It is reported so an operator can see
    /// which kind of escalation they are holding.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.deterministic_repair && !self.advisor && !self.committee && self.followups == 0
    }
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
    /// What the operator is being asked to confirm, when a human was reached.
    ///
    /// Present exactly when `escalation_cause` is: OP-REQ-036 makes the two
    /// halves of an escalation inseparable, so a transition carrying a cause and
    /// no recommendation is not representable through [`plan`].
    pub escalation_brief: Option<Escalation>,
    /// What had already been tried when the human was reached.
    pub deliberation_path: Option<DeliberationPath>,
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
        escalation_brief: None,
        deliberation_path: None,
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
                // The one escalation the machine raises on its own, so it is the
                // one that has to write its own brief. An empty deliberation path
                // is correct here and not a gap: an unsafe state is precisely the
                // case where trying more things first is the mistake.
                Ok(escalated(
                    &settled,
                    episode,
                    &unsafe_state_escalation(),
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

        RecoveryAction::Escalate(escalation) => Ok(escalated(
            &settled,
            episode,
            escalation,
            request.occurred_at,
        )),
    }
}

/// The role `kontord` records its own recommendations under.
///
/// A machine-raised escalation still names an author, because "who is
/// recommending this" is the question an operator weighs the recommendation
/// with, and "the daemon, from a deterministic rule" is a genuinely different
/// answer from "a seat, from its own judgement".
const SYSTEM_AUTHOR: &str = "kontord";

/// The brief the machine writes when deterministic repair finds an unsafe state.
fn unsafe_state_escalation() -> Escalation {
    Escalation {
        cause: EscalationCause::UnsafeState,
        recommendation: BoundedText::parse(
            "Deterministic inspection found the workspace or runtime state unsafe to act on, so \
             nothing was attempted. Inspect the seat's worktree and runtime session, then either \
             clear the unsafe condition and re-open the episode, or cancel the work. Do not \
             dispatch a follow-up until the state is understood: this escalation exists because \
             acting on it is what would cause damage.",
        )
        .expect("a compiled-in recommendation is bounded, printable and carries no secret"),
        recommended_by: RoleKey::parse(SYSTEM_AUTHOR)
            .expect("a compiled-in role key is an open key"),
    }
}

/// The one construction of a `needs_human` transition.
///
/// Every escalation goes through here, so [`RecoveryStatus::NeedsHuman`] cannot
/// be reached without one of the five [`EscalationCause`] values attached — and,
/// since OP-REQ-036, without the recommended resolution and the deliberation
/// path that make the operator's cheapest correct action a confirmation.
///
/// The recommendation is required by the *type*: [`Escalation`] has no
/// constructor that omits it, so "escalate with a bare cause" is not something a
/// caller can express and then be refused for. That is the difference between a
/// rule and a check — a check has a path around it.
fn escalated(
    settled: &RecoveryTransition,
    episode: &RecoveryEpisode,
    escalation: &Escalation,
    occurred_at: Timestamp,
) -> RecoveryTransition {
    RecoveryTransition {
        status: RecoveryStatus::NeedsHuman,
        escalation_cause: Some(escalation.cause),
        escalation_brief: Some(escalation.clone()),
        deliberation_path: Some(DeliberationPath::of(episode)),
        closed_at: Some(occurred_at),
        ..settled.clone()
    }
}
