//! The bounded recovery state machine.
//!
//! The mutants these tests exist to kill:
//!
//! * a budget that counts refused preflights, so an episode is spent on
//!   follow-ups that never ran;
//! * a budget that does *not* count an accepted dispatch whose work then failed,
//!   so a loop can retry forever;
//! * an advisor or committee that can be consulted twice on replay;
//! * a follow-up that resumes the parked run instead of a successor;
//! * a `needs_human` reachable without one of the five declared causes.

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, AggregateRevision, ContentHash, GuardrailEvaluationId, ProjectId, TaskId,
    TaskWorkflowId, Timestamp, parse_utc_timestamp,
};
use kontor_policy::model::{EscalationCause, RecoveryEpisode, RecoveryStatus, RecoveryStepKind};
use kontor_policy::recovery::{
    DeliberationPath, Escalation, MAX_EFFECTIVE_FOLLOWUPS, RecoveryAction, RecoveryRequest,
    RecoveryTransition, plan,
};

fn now() -> Timestamp {
    parse_utc_timestamp("2026-08-11T09:00:00Z").expect("a canonical UTC timestamp")
}

fn digest(marker: &str) -> ContentHash {
    ContentHash::of(marker.as_bytes())
}

fn episode() -> RecoveryEpisode {
    RecoveryEpisode {
        id: kontor_policy::model::RecoveryEpisodeId::generate(),
        project_id: ProjectId::generate(),
        task_id: TaskId::generate(),
        workflow_id: TaskWorkflowId::generate(),
        parked_agent_run_id: AgentRunId::generate(),
        status: RecoveryStatus::Open,
        cause_evaluation_id: GuardrailEvaluationId::generate(),
        advisor_used: false,
        committee_used: false,
        effective_followups: 0,
        successor_agent_run_id: None,
        escalation_cause: None,
        escalation_brief: None,
        deliberation_path: None,
        revision: AggregateRevision::INITIAL,
        created_at: now(),
        closed_at: None,
    }
}

fn request(episode: &RecoveryEpisode, action: RecoveryAction) -> RecoveryRequest {
    RecoveryRequest {
        expected_revision: episode.revision,
        action,
        input_hash: digest("input"),
        output_hash: None,
        occurred_at: now(),
    }
}

/// Apply a planned transition to an episode, as the store would.
fn apply(episode: &RecoveryEpisode, transition: &RecoveryTransition) -> RecoveryEpisode {
    RecoveryEpisode {
        status: transition.status,
        advisor_used: transition.advisor_used,
        committee_used: transition.committee_used,
        effective_followups: transition.effective_followups,
        successor_agent_run_id: transition.successor_agent_run_id,
        escalation_cause: transition.escalation_cause,
        escalation_brief: transition.escalation_brief.clone(),
        deliberation_path: transition.deliberation_path,
        revision: episode.revision.next().expect("the revision advances"),
        closed_at: transition.closed_at,
        ..episode.clone()
    }
}

fn advance(episode: &RecoveryEpisode, action: RecoveryAction) -> RecoveryEpisode {
    let transition = plan(episode, &request(episode, action)).expect("the transition is legal");
    apply(episode, &transition)
}

/// An episode that has had its deterministic pass and nothing else.
fn inspected() -> RecoveryEpisode {
    advance(
        &episode(),
        RecoveryAction::DeterministicRepair { safe: true },
    )
}

// ---------------------------------------------------------------------------

#[test]
fn deterministic_repair_comes_first_and_happens_once() {
    let open = episode();
    let repaired = advance(&open, RecoveryAction::DeterministicRepair { safe: true });
    assert_eq!(repaired.status, RecoveryStatus::DeterministicRepair);

    let error = plan(
        &repaired,
        &request(
            &repaired,
            RecoveryAction::DeterministicRepair { safe: true },
        ),
    )
    .expect_err("a second deterministic pass is not available");
    assert!(matches!(error, DomainError::IllegalTransition { .. }));

    // An advisor before anything has been inspected is refused: the budget is
    // not spent on guesses about a state nobody has looked at.
    let error = plan(&open, &request(&open, RecoveryAction::Advisor))
        .expect_err("an advisor before inspection is not available");
    assert!(matches!(error, DomainError::IllegalTransition { .. }));
}

#[test]
fn an_unsafe_state_escalates_instead_of_being_repaired_harder() {
    let open = episode();
    let escalated = advance(&open, RecoveryAction::DeterministicRepair { safe: false });
    assert_eq!(escalated.status, RecoveryStatus::NeedsHuman);
    assert_eq!(
        escalated.escalation_cause,
        Some(EscalationCause::UnsafeState)
    );
    assert!(escalated.closed_at.is_some());
}

#[test]
fn the_advisor_and_the_committee_are_read_only_and_each_available_once() {
    let advised = advance(&inspected(), RecoveryAction::Advisor);
    assert_eq!(advised.status, RecoveryStatus::Advisor);
    assert!(advised.advisor_used);
    // Consulting nobody launched anything and nothing was spent from the
    // follow-up budget.
    assert_eq!(advised.successor_agent_run_id, None);
    assert_eq!(advised.effective_followups, 0);

    let error = plan(&advised, &request(&advised, RecoveryAction::Advisor))
        .expect_err("the advisor budget is one");
    assert!(matches!(error, DomainError::Invalid { .. }));

    let convened = advance(&advised, RecoveryAction::Committee);
    assert_eq!(convened.status, RecoveryStatus::Committee);
    assert!(convened.committee_used);
    assert_eq!(convened.successor_agent_run_id, None);
    assert_eq!(convened.effective_followups, 0);

    let error = plan(&convened, &request(&convened, RecoveryAction::Committee))
        .expect_err("the committee budget is one");
    assert!(matches!(error, DomainError::Invalid { .. }));

    assert!(RecoveryStepKind::Advisor.is_read_only());
    assert!(RecoveryStepKind::Committee.is_read_only());
    assert!(!RecoveryStepKind::FollowupExecution.is_read_only());
}

#[test]
fn the_committee_may_be_convened_without_an_advisor() {
    let convened = advance(&inspected(), RecoveryAction::Committee);
    assert_eq!(convened.status, RecoveryStatus::Committee);
    assert!(convened.committee_used);
    assert!(!convened.advisor_used);
}

#[test]
fn only_dispatched_follow_ups_are_charged_and_only_two_of_them() {
    let mut current = inspected();

    // A refused preflight: nothing ran, so nothing is charged and no successor
    // is linked. Charging it here would spend the episode on attempts that
    // never happened.
    let refused = plan(
        &current,
        &request(
            &current,
            RecoveryAction::Followup {
                dispatched: false,
                successor: AgentRunId::generate(),
            },
        ),
    )
    .expect("a refused dispatch is a legal, free step");
    assert_eq!(refused.effective_followups, 0);
    assert_eq!(refused.successor_agent_run_id, None);
    assert_eq!(refused.status, RecoveryStatus::DeterministicRepair);
    current = apply(&current, &refused);

    let mut successors = Vec::new();
    for expected in 1..=MAX_EFFECTIVE_FOLLOWUPS {
        let successor = AgentRunId::generate();
        successors.push(successor);
        current = advance(
            &current,
            RecoveryAction::Followup {
                dispatched: true,
                successor,
            },
        );
        assert_eq!(current.effective_followups, expected);
        assert_eq!(current.status, RecoveryStatus::Followup);
        assert_eq!(current.successor_agent_run_id, Some(successor));
    }

    // An accepted dispatch counts whatever it then produced, so the third is
    // refused however badly the first two went.
    let error = plan(
        &current,
        &request(
            &current,
            RecoveryAction::Followup {
                dispatched: true,
                successor: AgentRunId::generate(),
            },
        ),
    )
    .expect_err("the follow-up budget is two");
    assert!(matches!(error, DomainError::Invalid { .. }));
}

#[test]
fn a_follow_up_never_resumes_the_parked_run_in_place() {
    let current = inspected();
    let error = plan(
        &current,
        &request(
            &current,
            RecoveryAction::Followup {
                dispatched: true,
                successor: current.parked_agent_run_id,
            },
        ),
    )
    .expect_err("the parked run is never its own successor");
    assert!(matches!(error, DomainError::Invalid { .. }));
}

#[test]
fn a_replayed_step_is_refused_rather_than_spent_twice() {
    let inspected = inspected();
    let stale = request(&inspected, RecoveryAction::Advisor);
    let advised = apply(
        &inspected,
        &plan(&inspected, &stale).expect("the advisor is available"),
    );

    // The very same request again, computed against the revision it saw. A
    // restart that re-runs its last step lands exactly here.
    let error = plan(&advised, &stale).expect_err("a replayed transition is refused");
    assert!(matches!(error, DomainError::RevisionConflict { .. }));
}

/// A well-formed escalation brief, for tests about something other than the
/// brief itself.
fn brief(cause: EscalationCause) -> Escalation {
    Escalation {
        cause,
        recommendation: kontor_core::id::BoundedText::parse(
            "Confirm the seat may be cancelled and the ticket re-planned.",
        )
        .expect("bounded text"),
        recommended_by: kontor_core::id::RoleKey::parse("lsa").expect("an open key"),
    }
}

#[test]
fn a_closed_episode_accepts_nothing_further() {
    let recovered = advance(&inspected(), RecoveryAction::Recover);
    assert_eq!(recovered.status, RecoveryStatus::Recovered);
    assert!(recovered.closed_at.is_some());

    for action in [
        RecoveryAction::Advisor,
        RecoveryAction::Committee,
        RecoveryAction::Recover,
        RecoveryAction::Escalate(brief(EscalationCause::BudgetExhausted)),
    ] {
        let error = plan(&recovered, &request(&recovered, action))
            .expect_err("a closed episode is terminal");
        assert!(matches!(error, DomainError::Terminal { .. }));
    }
}

#[test]
fn recovery_is_not_declared_before_anything_has_been_attempted() {
    let open = episode();
    let error = plan(&open, &request(&open, RecoveryAction::Recover))
        .expect_err("an untouched episode has not recovered");
    assert!(matches!(error, DomainError::IllegalTransition { .. }));
}

#[test]
fn exactly_the_five_declared_causes_reach_needs_human() {
    assert_eq!(
        EscalationCause::ALL.len(),
        5,
        "the escalation vocabulary is closed at five"
    );
    for cause in EscalationCause::ALL {
        let current = inspected();
        let escalated = advance(&current, RecoveryAction::Escalate(brief(*cause)));
        assert_eq!(escalated.status, RecoveryStatus::NeedsHuman);
        assert_eq!(escalated.escalation_cause, Some(*cause));
        assert!(escalated.closed_at.is_some());
    }

    // Every other action that closes an episode closes it as recovered, so
    // `needs_human` has no second route in.
    let recovered = advance(&inspected(), RecoveryAction::Recover);
    assert_eq!(recovered.escalation_cause, None);
}

/// OP-REQ-036: no escalation reaches a human as a bare question.
///
/// Every entry into `needs_human` carries a recommended resolution with its
/// author and the deliberation path already walked, so the operator's cheapest
/// correct action is confirmation rather than reconstruction. The rule is held
/// by the *type* — `RecoveryAction::Escalate` takes an `Escalation`, which has no
/// field-free form — so this test is about the transition actually recording
/// both halves rather than accepting them and dropping them.
#[test]
fn no_escalation_reaches_a_human_without_a_recommendation_and_a_path() {
    let current = inspected();
    let escalation = brief(EscalationCause::IncompleteEvidence);
    let escalated = advance(&current, RecoveryAction::Escalate(escalation.clone()));

    assert_eq!(escalated.status, RecoveryStatus::NeedsHuman);
    assert_eq!(
        escalated.escalation_brief.as_ref(),
        Some(&escalation),
        "the recommendation the operator is owed was accepted and then dropped"
    );
    assert_eq!(
        escalated.deliberation_path,
        Some(DeliberationPath {
            deterministic_repair: true,
            advisor: false,
            committee: false,
            followups: 0,
        }),
        "the path is derived from the episode's own steps"
    );

    // The two halves are inseparable: a cause without a brief is what the rule
    // exists to forbid, so no reachable transition may carry one and not the
    // other.
    assert_eq!(
        escalated.escalation_cause.is_some(),
        escalated.escalation_brief.is_some(),
        "a cause and its recommendation must travel together"
    );
}

/// The path is *observed*, not asserted — so it grows as the episode does.
///
/// The mutant this kills is a derivation that reports a constant: an operator
/// reading "advisor and committee already tried" on an episode that consulted
/// neither would take the escalation far more seriously than it deserves, and
/// one that under-reports invites re-running work that already failed.
#[test]
fn the_deliberation_path_reports_what_the_episode_actually_spent() {
    let mut current = inspected();
    current = advance(&current, RecoveryAction::Advisor);
    current = advance(&current, RecoveryAction::Committee);

    let escalated = advance(
        &current,
        RecoveryAction::Escalate(brief(EscalationCause::CommitteeDisagreement)),
    );
    let path = escalated
        .deliberation_path
        .expect("an escalation has a path");
    assert!(path.deterministic_repair);
    assert!(
        path.advisor,
        "the advisor was consulted and the path says so"
    );
    assert!(
        path.committee,
        "the committee was convened and the path says so"
    );
    assert!(!path.is_empty());
}

/// An unsafe state escalates with a brief the machine wrote itself.
///
/// This is the one escalation no seat authors, so it is the one that would most
/// easily reach an operator bare. An empty deliberation path is correct here and
/// is not a gap: acting further on an unsafe state is the mistake.
#[test]
fn a_machine_raised_escalation_still_carries_a_recommendation() {
    let open = episode();
    let escalated = advance(&open, RecoveryAction::DeterministicRepair { safe: false });

    assert_eq!(escalated.status, RecoveryStatus::NeedsHuman);
    let brief = escalated
        .escalation_brief
        .expect("even a machine-raised escalation recommends something");
    assert_eq!(brief.cause, EscalationCause::UnsafeState);
    assert!(
        brief.recommendation.as_str().len() > 40,
        "a recommendation an operator can act on is more than a restatement"
    );
    assert_eq!(
        brief.recommended_by.as_str(),
        "kontord",
        "a machine-authored recommendation says so, so an operator can weigh it"
    );
    assert!(
        escalated
            .deliberation_path
            .expect("a path is always recorded")
            .is_empty(),
        "nothing was tried, and claiming otherwise would be a lie about an unsafe state"
    );
}

#[test]
fn each_follow_up_dispatches_its_own_successor() {
    let mut current = inspected();
    let first = AgentRunId::generate();
    current = advance(
        &current,
        RecoveryAction::Followup {
            dispatched: true,
            successor: first,
        },
    );
    assert_eq!(current.effective_followups, 1);
    assert_eq!(current.successor_agent_run_id, Some(first));

    // The same run handed back for the second follow-up. With a budget of two,
    // this is the only reuse the episode can express, and it is the one that
    // would spend both dispatches on a single session — the ledger would say two
    // attempts where only one thing ever ran.
    let error = plan(
        &current,
        &request(
            &current,
            RecoveryAction::Followup {
                dispatched: true,
                successor: first,
            },
        ),
    )
    .expect_err("a follow-up may not reuse the successor already dispatched");
    assert!(matches!(error, DomainError::Invalid { .. }));

    // A distinct run is admitted, so the rule refuses reuse and not the second
    // follow-up itself.
    let second = AgentRunId::generate();
    let current = advance(
        &current,
        RecoveryAction::Followup {
            dispatched: true,
            successor: second,
        },
    );
    assert_eq!(current.effective_followups, 2);
    assert_eq!(current.successor_agent_run_id, Some(second));
}

#[test]
fn only_an_accepted_dispatch_names_a_run_on_its_step() {
    let current = inspected();
    let successor = AgentRunId::generate();

    // A refused preflight leaves the episode's cumulative successor alone *and*
    // records nothing as dispatched, so the step it appends names no run.
    let refused = plan(
        &current,
        &request(
            &current,
            RecoveryAction::Followup {
                dispatched: false,
                successor,
            },
        ),
    )
    .expect("a refused dispatch is a legal step");
    assert_eq!(refused.dispatched_successor, None);
    assert_eq!(refused.successor_agent_run_id, None);

    let accepted = plan(
        &current,
        &request(
            &current,
            RecoveryAction::Followup {
                dispatched: true,
                successor,
            },
        ),
    )
    .expect("an accepted dispatch is admitted");
    assert_eq!(accepted.dispatched_successor, Some(successor));
    assert_eq!(accepted.successor_agent_run_id, Some(successor));

    // A read-only consultation never dispatches, whatever else it does.
    let advised = plan(&current, &request(&current, RecoveryAction::Advisor))
        .expect("the advisor is available");
    assert_eq!(advised.dispatched_successor, None);
}
