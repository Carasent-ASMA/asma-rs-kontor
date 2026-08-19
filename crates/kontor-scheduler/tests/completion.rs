//! Completion compiler and restart-safe state-machine scenarios.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::id::{ContentHash, ExternalName, SeatBindingId, TaskId};
use kontor_policy::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, TicketEvidence, TicketRequirement,
};
use kontor_scheduler::{
    CommitteeVerdict, CompiledCompletion, CompletionCommand, CompletionEdgeCondition,
    CompletionNodeKey, CompletionObservation, CompletionPhase, CompletionProfile, CompletionSignal,
    CompletionState, IntegrationRecord, PollingFallback, RemediationApproval,
    RemediationAuthorization, RepositoryOutcome, SignalDelivery, advance, compile,
    operational_default, outstanding, start,
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
        CompletionObservation::CloseoutRecorded(CloseoutEvidence::default()),
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
        CompletionObservation::CloseoutRecorded(full_closeout()),
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
