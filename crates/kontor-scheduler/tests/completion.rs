//! Completion compiler and restart-safe state-machine scenarios.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::id::{
    BoundedText, ContentHash, ExternalName, OpenQuestionId, SeatBindingId, TaskId,
};
use kontor_core::open_question::{OpenQuestionStatus, OpenQuestionSummary};
use kontor_policy::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, OpenQuestionBlocker, TicketEvidence,
    TicketRequirement,
};
use kontor_scheduler::{
    CommitteeVerdict, CompiledCompletion, CompletionBlocker, CompletionCommand,
    CompletionEdgeCondition, CompletionNodeKey, CompletionObservation, CompletionPhase,
    CompletionProfile, CompletionSignal, CompletionState, IntegrationRecord, PollingFallback,
    RemediationApproval, RemediationAuthorization, RepositoryOutcome, SignalDelivery, advance,
    blockers, compile, operational_default, outstanding, start,
};

fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).expect("test names are valid")
}

fn digest(value: &str) -> ContentHash {
    ContentHash::of(value.as_bytes())
}

fn requirement(task_id: TaskId) -> TicketRequirement {
    TicketRequirement {
        task_id,
        goals: BTreeSet::from([name("accepted")]),
        evidence: BTreeSet::from([name("tests")]),
    }
}

fn ticket_evidence(task_id: TaskId) -> TicketEvidence {
    TicketEvidence {
        task_id,
        goals: BTreeSet::from([name("accepted")]),
        evidence: BTreeSet::from([name("tests")]),
    }
}

fn integration(marker: &str) -> IntegrationRecord {
    IntegrationRecord {
        receipt: digest(&format!("{marker}-receipt")),
        repositories: vec![
            RepositoryOutcome {
                repository: name("kontor"),
                pull_request: name(&format!("PR-{marker}-1")),
                module_revision: name(&format!("module-{marker}-1")),
                root_pointer_revision: Some(name(&format!("root-{marker}-1"))),
            },
            RepositoryOutcome {
                repository: name("asma-cli"),
                pull_request: name(&format!("PR-{marker}-2")),
                module_revision: name(&format!("module-{marker}-2")),
                root_pointer_revision: None,
            },
        ],
    }
}

fn deliberation(round: u8, outcome: &str) -> Vec<DeliberationStep> {
    vec![DeliberationStep {
        role: name("Committee members and Judge"),
        consultation: name("independent review"),
        round,
        outcome: name(outcome),
    }]
}

fn full_closeout() -> CloseoutEvidence {
    CloseoutEvidence {
        merge_receipt: Some(digest("merge")),
        release_receipt: Some(digest("release")),
        delivered_versions: BTreeMap::from([
            (name("kontord"), name("0.1.0")),
            (name("console"), name("0.1.0")),
        ]),
        summary_receipt: Some(digest("summary")),
        notification_receipt: Some(digest("notification")),
        archive_receipt: Some(digest("archive")),
    }
}

fn callback(
    state: &CompletionState,
    marker: &str,
    observation: CompletionObservation,
) -> CompletionSignal {
    CompletionSignal {
        id: digest(marker),
        expected_revision: state.revision,
        delivery: SignalDelivery::Callback,
        observation,
    }
}

fn apply_and_restart(
    compiled: &CompiledCompletion,
    state: &CompletionState,
    signal: &CompletionSignal,
) -> (CompletionState, Vec<CompletionCommand>) {
    let transition = advance(compiled, state, signal).expect("the scenario transition is legal");
    let restored: CompletionState =
        serde_json::from_slice(&serde_json::to_vec(&transition.state).expect("state serializes"))
            .expect("state restores at this stage");
    assert_eq!(restored, transition.state);
    (restored, transition.commands)
}

fn assert_one_tpm_wake(commands: &[CompletionCommand], tpm: SeatBindingId) {
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(
                command,
                CompletionCommand::WakeTpm { seat_binding_id } if *seat_binding_id == tpm
            ))
            .count(),
        1,
        "one callback wakes the same existing TPM exactly once"
    );
}

#[test]
fn operational_default_compiles_the_bounded_conditional_dag() {
    let compiled = compile(operational_default().expect("the seed is valid")).expect("compiles");
    assert_eq!(compiled.profile.max_remediation_rounds, 1);

    let has_edge = |from, to, condition| {
        compiled
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to && edge.condition == condition)
    };
    assert!(has_edge(
        CompletionNodeKey::Verdict(1),
        CompletionNodeKey::Remediation(1),
        CompletionEdgeCondition::Fail,
    ));
    assert!(has_edge(
        CompletionNodeKey::Verdict(2),
        CompletionNodeKey::NeedsHuman,
        CompletionEdgeCondition::Fail,
    ));
    for round in [1, 2] {
        assert!(has_edge(
            CompletionNodeKey::Verdict(round),
            CompletionNodeKey::Closeout(CloseoutRequirement::Merge),
            CompletionEdgeCondition::Pass,
        ));
    }
    assert!(has_edge(
        CompletionNodeKey::Closeout(CloseoutRequirement::Archive),
        CompletionNodeKey::Done,
        CompletionEdgeCondition::Success,
    ));
}

#[test]
fn fail_remediate_pass_survives_restart_at_every_stage_and_closes_only_with_evidence() {
    let compiled = compile(operational_default().expect("the seed is valid")).expect("compiles");
    let tpm = SeatBindingId::generate();
    let task = TaskId::generate();
    let mut state = start(&compiled, tpm, vec![requirement(task)]).expect("starts");
    assert!(!outstanding(&state).expect("projects").is_empty());

    let (next, commands) = apply_and_restart(
        &compiled,
        &state,
        &callback(
            &state,
            "tickets",
            CompletionObservation::TicketsClosed(vec![ticket_evidence(task)]),
        ),
    );
    assert!(matches!(
        commands[0],
        CompletionCommand::StartIntegration { .. }
    ));
    assert_one_tpm_wake(&commands, tpm);
    state = next;

    let (next, commands) = apply_and_restart(
        &compiled,
        &state,
        &callback(
            &state,
            "integration",
            CompletionObservation::IntegrationCompleted(integration("initial")),
        ),
    );
    assert!(matches!(
        commands[0],
        CompletionCommand::InvokeCommittee { round: 1, .. }
    ));
    assert_one_tpm_wake(&commands, tpm);
    state = next;

    let (next, commands) = apply_and_restart(
        &compiled,
        &state,
        &callback(
            &state,
            "verdict-1",
            CompletionObservation::VerdictRecorded {
                round: 1,
                verdict: CommitteeVerdict::Fail,
                evidence: digest("failed-finding-1"),
                committee_run_id: None,
                result_hash: None,
                remediation_hash: None,
                deliberation: deliberation(1, "failed"),
            },
        ),
    );
    assert!(matches!(
        commands[0],
        CompletionCommand::DeliverFailureToLsa { round: 1, .. }
    ));
    assert_one_tpm_wake(&commands, tpm);
    state = next;

    let premature = advance(
        &compiled,
        &state,
        &callback(
            &state,
            "unapproved-remediation",
            CompletionObservation::RemediationCompleted(integration("unapproved")),
        ),
    )
    .expect_err("remediation cannot launch or finish without both authority receipts");
    assert!(matches!(premature, DomainError::IllegalTransition { .. }));

    let authorization = RemediationAuthorization {
        lsa_proposal: digest("lsa-proposal"),
        tpm_routing: digest("tpm-routing"),
    };
    let (next, commands) = apply_and_restart(
        &compiled,
        &state,
        &callback(
            &state,
            "remediation-approved",
            CompletionObservation::RemediationApproved(RemediationApproval {
                round: 1,
                authorization: authorization.clone(),
            }),
        ),
    );
    assert_eq!(
        commands[0],
        CompletionCommand::LaunchRemediation {
            round: 1,
            authorization,
        }
    );
    assert_one_tpm_wake(&commands, tpm);
    state = next;

    let (next, commands) = apply_and_restart(
        &compiled,
        &state,
        &callback(
            &state,
            "remediation-completed",
            CompletionObservation::RemediationCompleted(integration("remediation")),
        ),
    );
    assert!(matches!(
        commands[0],
        CompletionCommand::InvokeCommittee { round: 2, .. }
    ));
    assert_one_tpm_wake(&commands, tpm);
    state = next;

    let second_failure = advance(
        &compiled,
        &state,
        &callback(
            &state,
            "verdict-2-failed",
            CompletionObservation::VerdictRecorded {
                round: 2,
                verdict: CommitteeVerdict::Fail,
                evidence: digest("failed-finding-2"),
                committee_run_id: None,
                result_hash: None,
                remediation_hash: None,
                deliberation: deliberation(2, "failed"),
            },
        ),
    )
    .expect("the bounded failure becomes human attention");
    assert_eq!(second_failure.state.phase, CompletionPhase::NeedsHuman);
    let escalation = second_failure
        .state
        .needs_human
        .as_ref()
        .expect("needs_human always carries context");
    assert_eq!(escalation.tried_deliberation_path().len(), 2);

    let first_round = state.rounds[0].clone();
    let (next, commands) = apply_and_restart(
        &compiled,
        &state,
        &callback(
            &state,
            "verdict-2-pass",
            CompletionObservation::VerdictRecorded {
                round: 2,
                verdict: CommitteeVerdict::Pass,
                evidence: digest("passed-finding-2"),
                committee_run_id: None,
                result_hash: None,
                remediation_hash: None,
                deliberation: deliberation(2, "passed"),
            },
        ),
    );
    assert_one_tpm_wake(&commands, tpm);
    state = next;
    assert_eq!(state.rounds[0], first_round, "prior rounds are immutable");
    assert_eq!(state.remediations.len(), 1);
    assert_eq!(state.integrations.len(), 2);
    assert_eq!(state.integrations[0].repositories.len(), 2);
    assert!(
        state.integrations[0].repositories[1]
            .root_pointer_revision
            .is_none()
    );

    let partial = callback(
        &state,
        "partial-closeout",
        CompletionObservation::CloseoutRecorded {
            evidence: CloseoutEvidence::default(),
            open_questions: Vec::new(),
        },
    );
    let (next, commands) = apply_and_restart(&compiled, &state, &partial);
    assert_one_tpm_wake(&commands, tpm);
    state = next;
    assert_eq!(state.phase, CompletionPhase::Closeout);
    assert_eq!(
        outstanding(&state).expect("projects"),
        CloseoutRequirement::ALL
            .into_iter()
            .map(|item| item.as_str().to_owned())
            .collect::<Vec<_>>()
    );

    let final_signal = callback(
        &state,
        "full-closeout",
        CompletionObservation::CloseoutRecorded {
            evidence: full_closeout(),
            open_questions: Vec::new(),
        },
    );
    let (done, commands) = apply_and_restart(&compiled, &state, &final_signal);
    assert_eq!(done.phase, CompletionPhase::Done);
    assert!(commands.contains(&CompletionCommand::MarkDone));
    assert_one_tpm_wake(&commands, tpm);

    let replay = advance(&compiled, &done, &final_signal).expect("a lost acknowledgement replays");
    assert!(replay.replayed);
    assert!(
        replay.commands.is_empty(),
        "a replay wakes no duplicate TPM"
    );
}

#[test]
fn polling_is_used_only_when_declared_and_exhaustion_has_human_context() {
    let CompletionProfile {
        id,
        version,
        name: profile_name,
        integration_team,
        verdict_committee,
        max_remediation_rounds,
        ..
    } = operational_default().expect("the seed is valid");
    let compiled = compile(CompletionProfile {
        id,
        version,
        name: profile_name,
        integration_team,
        verdict_committee,
        max_remediation_rounds,
        polling_fallback: Some(PollingFallback { max_attempts: 2 }),
    })
    .expect("compiles");
    let mut state = start(&compiled, SeatBindingId::generate(), Vec::new()).expect("starts");

    for attempt in 1..=2 {
        let signal = CompletionSignal {
            id: digest(&format!("poll-{attempt}")),
            expected_revision: state.revision,
            delivery: SignalDelivery::Polling,
            observation: CompletionObservation::Attention,
        };
        let transition = advance(&compiled, &state, &signal).expect("bounded poll is legal");
        assert_eq!(
            transition.commands,
            [CompletionCommand::SchedulePoll {
                attempt,
                max_attempts: 2,
            }]
        );
        state = transition.state;
    }

    let exhausted = advance(
        &compiled,
        &state,
        &CompletionSignal {
            id: digest("poll-exhausted"),
            expected_revision: state.revision,
            delivery: SignalDelivery::Polling,
            observation: CompletionObservation::Attention,
        },
    )
    .expect("exhaustion is a typed terminal transition");
    assert_eq!(exhausted.state.phase, CompletionPhase::NeedsHuman);
    assert!(exhausted.state.needs_human.is_some());
    assert!(exhausted.commands.is_empty());
}

#[test]
fn an_incomplete_ticket_gate_cannot_start_integration() {
    let compiled = compile(operational_default().expect("the seed is valid")).expect("compiles");
    let task = TaskId::generate();
    let state = start(
        &compiled,
        SeatBindingId::generate(),
        vec![requirement(task)],
    )
    .expect("starts");
    let error = advance(
        &compiled,
        &state,
        &callback(
            &state,
            "missing-ticket-evidence",
            CompletionObservation::TicketsClosed(Vec::new()),
        ),
    )
    .expect_err("the ticket gate is closed");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));
}

// ---------------------------------------------------------------------------
// The open-question completion gate (OP-REQ-038)
// ---------------------------------------------------------------------------

fn question(status: OpenQuestionStatus) -> OpenQuestionSummary {
    OpenQuestionSummary {
        question_id: OpenQuestionId::generate(),
        subject: BoundedText::parse("whether the mirror is authoritative").expect("text"),
        status,
    }
}

/// Drive one completion to the closeout phase with nothing else outstanding.
fn at_closeout() -> (CompiledCompletion, CompletionState, SeatBindingId) {
    let compiled = compile(operational_default().expect("bundled profile")).expect("compiles");
    let tpm = SeatBindingId::generate();
    let task = TaskId::generate();
    let mut state = start(&compiled, tpm, vec![requirement(task)]).expect("starts");

    // Each signal needs its own id: `handled_signals` makes a repeated id a
    // replay, which would silently leave the phase where it was.
    for (id, observation) in [
        (
            "tickets-closed",
            CompletionObservation::TicketsClosed(vec![ticket_evidence(task)]),
        ),
        (
            "integration-done",
            CompletionObservation::IntegrationCompleted(integration("integration")),
        ),
    ] {
        let signal = callback(&state, id, observation);
        state = advance(&compiled, &state, &signal).expect("advances").state;
    }
    let verdict = callback(
        &state,
        "verdict-1-pass",
        CompletionObservation::VerdictRecorded {
            round: 1,
            verdict: CommitteeVerdict::Pass,
            evidence: digest("finding"),
            committee_run_id: None,
            result_hash: None,
            remediation_hash: None,
            deliberation: deliberation(1, "passed"),
        },
    );
    state = advance(&compiled, &state, &verdict)
        .expect("advances")
        .state;
    assert_eq!(state.phase, CompletionPhase::Closeout);
    (compiled, state, tpm)
}

fn record_closeout(
    compiled: &CompiledCompletion,
    state: &CompletionState,
    id: &str,
    open_questions: Vec<OpenQuestionSummary>,
) -> (CompletionState, Vec<CompletionCommand>) {
    let signal = callback(
        state,
        id,
        CompletionObservation::CloseoutRecorded {
            evidence: full_closeout(),
            open_questions,
        },
    );
    let transition = advance(compiled, state, &signal).expect("closeout applies");
    (transition.state, transition.commands)
}

#[test]
fn an_undispositioned_question_keeps_the_epic_out_of_done() {
    let (compiled, state, _) = at_closeout();
    let open = question(OpenQuestionStatus::Open);
    let (next, commands) = record_closeout(&compiled, &state, "closeout", vec![open.clone()]);

    assert_eq!(
        next.phase,
        CompletionPhase::Closeout,
        "an unresolved ambiguity leaves the completion non-terminal"
    );
    assert!(
        !commands.contains(&CompletionCommand::MarkDone),
        "MarkDone is refused while a question is undispositioned"
    );
    assert_eq!(
        blockers(&next).expect("projects"),
        vec![CompletionBlocker::OpenQuestion(
            OpenQuestionBlocker::Undispositioned {
                question_id: open.question_id,
                subject: open.subject.clone(),
            }
        )],
        "the blocker names the question and its subject as data"
    );
    assert_eq!(
        outstanding(&next).expect("projects"),
        vec![format!("open_question:{}", open.question_id)]
    );
}

#[test]
fn a_reopened_question_keeps_the_epic_out_of_done() {
    let (compiled, state, _) = at_closeout();
    let reopened = question(OpenQuestionStatus::Reopened);
    let (next, commands) = record_closeout(&compiled, &state, "closeout", vec![reopened.clone()]);

    assert_eq!(next.phase, CompletionPhase::Closeout);
    assert!(!commands.contains(&CompletionCommand::MarkDone));
    assert_eq!(
        blockers(&next).expect("projects"),
        vec![CompletionBlocker::OpenQuestion(
            OpenQuestionBlocker::Reopened {
                question_id: reopened.question_id,
                subject: reopened.subject,
            }
        )]
    );
}

#[test]
fn every_disposition_releases_the_gate() {
    for status in [
        OpenQuestionStatus::Resolved,
        OpenQuestionStatus::Deferred,
        OpenQuestionStatus::NotRelevant,
    ] {
        let (compiled, state, _) = at_closeout();
        let (next, commands) =
            record_closeout(&compiled, &state, "closeout", vec![question(status)]);
        assert_eq!(
            next.phase,
            CompletionPhase::Done,
            "`{status}` is a disposition and releases the gate"
        );
        assert!(commands.contains(&CompletionCommand::MarkDone));
        assert!(blockers(&next).expect("projects").is_empty());
    }
}

#[test]
fn a_question_raised_after_completion_started_still_blocks_done() {
    // The question set is not frozen when completion starts: this run began
    // with none and acquires one during closeout.
    let (compiled, state, _) = at_closeout();
    assert!(
        state.open_questions.is_empty(),
        "the run started with no open questions"
    );

    let late = question(OpenQuestionStatus::Open);
    let (next, commands) = record_closeout(&compiled, &state, "late-question", vec![late.clone()]);
    assert_eq!(next.phase, CompletionPhase::Closeout);
    assert!(!commands.contains(&CompletionCommand::MarkDone));

    // And once that question is dispositioned, the next closeout signal passes.
    let dispositioned = OpenQuestionSummary {
        status: OpenQuestionStatus::Resolved,
        ..late
    };
    let (done, commands) =
        record_closeout(&compiled, &next, "closed-question", vec![dispositioned]);
    assert_eq!(
        done.phase,
        CompletionPhase::Done,
        "release happens once the current question is dispositioned"
    );
    assert!(commands.contains(&CompletionCommand::MarkDone));
}

#[test]
fn one_blocking_question_among_many_is_enough_to_hold_the_epic() {
    let (compiled, state, _) = at_closeout();
    let blocking = question(OpenQuestionStatus::Reopened);
    let questions = vec![
        question(OpenQuestionStatus::Resolved),
        blocking.clone(),
        question(OpenQuestionStatus::NotRelevant),
        question(OpenQuestionStatus::Deferred),
    ];
    let (next, commands) = record_closeout(&compiled, &state, "closeout", questions);

    assert_eq!(next.phase, CompletionPhase::Closeout);
    assert!(!commands.contains(&CompletionCommand::MarkDone));
    assert_eq!(
        blockers(&next).expect("projects"),
        vec![CompletionBlocker::OpenQuestion(
            OpenQuestionBlocker::Reopened {
                question_id: blocking.question_id,
                subject: blocking.subject,
            }
        )],
        "only the blocking question is reported"
    );
}

#[test]
fn closeout_receipts_and_questions_are_independent_gates() {
    let (compiled, state, _) = at_closeout();
    let signal = callback(
        &state,
        "partial-and-question",
        CompletionObservation::CloseoutRecorded {
            evidence: CloseoutEvidence::default(),
            open_questions: vec![question(OpenQuestionStatus::Open)],
        },
    );
    let next = advance(&compiled, &state, &signal).expect("applies").state;
    let projected = blockers(&next).expect("projects");

    assert_eq!(next.phase, CompletionPhase::Closeout);
    assert_eq!(
        projected.len(),
        CloseoutRequirement::ALL.len() + 1,
        "both gates report at once rather than masking each other"
    );
    assert!(
        projected
            .iter()
            .any(|blocker| matches!(blocker, CompletionBlocker::OpenQuestion(_)))
    );
}
