//! Lifecycle, orthogonality, revision and terminal-immutability cases.
//!
//! The mutants this suite exists to kill:
//!
//! * collapsing the task lifecycle into the profile phase;
//! * writing an observed state straight into the derived state;
//! * treating a disappeared process, a closed stream or an unreachable runtime
//!   as completion;
//! * reopening a terminal run instead of creating a successor;
//! * resuming a held task without a command receipt;
//! * closing a task before its profile says it may close;
//! * reusing an idempotency key as a new command, or retrying after an unknown
//!   dispatch result without reconciling first.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::id::RealmId;
use kontor_core::id::{
    AgentRunId, AggregateRevision, ArtifactKey, CanonicalDocument, CommandReceiptId, ContentHash,
    EventCursor, ExternalId, ExternalName, GateKey, IdempotencyKey, MiniProjectId, PhaseKey,
    ProjectId, RuntimeKindKey, SpecVersion, TaskId, TeamRunId, TicketLinkId, Timestamp,
    WorkCalendarId, parse_utc_timestamp,
};
use kontor_core::realm::{
    EventEnvelope, ExportEnvelope, RealmCursor, RealmMetadata, ReceiptEnvelope, SnapshotEnvelope,
    ensure_realm,
};
use kontor_core::receipt::{
    AggregateKind, AggregateRef, CommandKind, CommandReceipt, CommandReceiptState,
    DesiredStateRule, NoEffectEvidence, RevisionRule,
};
use kontor_core::spec::{ResolvedWorkProfileSnapshot, WorkProfileSpec};
use kontor_core::state::{
    AbandonReceiptFacts, DerivedRunState, DesiredRunState, Freshness, GateState, GateVerdict,
    NativeRuntimeIdentity, ObservedRunState, RunDerivation, RunLifecycle, RuntimeContact,
    RuntimeObservation, SeatAttachment, SeatAttachmentObservation, TaskState, TaskTransition,
    TeamChildEvidence, TeamEvidenceSource, TeamTerminalEvidence, TerminalEvidence,
    TerminalEvidenceSource, TerminalOutcome, apply_task_transition, certify_task_progress,
    derive_run_state, evaluate_seat_attachment, plan_team_advance, plan_team_closure,
    reduce_run_lifecycle, reduce_team_outcome, team_child_evidence_digest,
};

const ARBITRARY_PROFILE: &str = include_str!("fixtures/work_profile_arbitrary.json");

/// A seat observation that is healthy in every respect, so each test below can
/// spoil exactly one thing and attribute the conclusion to that one thing.
fn healthy_seat() -> SeatAttachmentObservation {
    SeatAttachmentObservation {
        attach_deadline: at("2026-08-16T08:00:00Z"),
        last_attached_at: Some(at("2026-08-16T09:55:00Z")),
        last_activity_at: Some(at("2026-08-16T09:55:00Z")),
        parent_closed: false,
        released: false,
        runtime_reported: ObservedRunState::Running,
    }
}

const SEAT_IDLE_BOUND: jiff::SignedDuration = jiff::SignedDuration::from_mins(30);

fn conclude(observation: &SeatAttachmentObservation, now: &str) -> SeatAttachment {
    evaluate_seat_attachment(observation, at(now), SEAT_IDLE_BOUND)
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

fn hash() -> ContentHash {
    ContentHash::of(b"evidence")
}

fn identity(generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("generic.runtime").expect("valid runtime key"),
        host: ExternalName::parse("host-1").expect("valid host"),
        generation,
        native_id: ExternalId::parse("session-abc").expect("valid native id"),
    }
}

fn observation(state: ObservedRunState, generation: u64) -> RuntimeObservation {
    RuntimeObservation {
        agent_run_id: kontor_core::id::AgentRunId::generate(),
        state,
        identity: identity(generation),
        cursor: EventCursor::parse(7).expect("positive cursor"),
        observed_at: at("2026-08-09T10:00:00Z"),
        evidence_hash: hash(),
    }
}

fn profile() -> WorkProfileSpec {
    serde_json::from_str(ARBITRARY_PROFILE).expect("the arbitrary profile fixture parses")
}

// ---------------------------------------------------------------------------
// Task lifecycle
// ---------------------------------------------------------------------------

#[test]
fn task_transitions_follow_the_declared_table() {
    assert!(TaskState::Draft.can_transition_to(TaskState::Todo));
    assert!(TaskState::Ready.can_transition_to(TaskState::InProgress));
    assert!(TaskState::InProgress.can_transition_to(TaskState::Blocked));

    // A draft cannot jump straight into work. Nothing leaves a terminal state
    // except the one structurally legal reopen, `done -> ready`, which
    // `apply_task_transition` then refuses unless it is explicitly authorized.
    assert!(!TaskState::Draft.can_transition_to(TaskState::InProgress));
    for terminal in [TaskState::Done, TaskState::Failed, TaskState::Cancelled] {
        assert!(terminal.is_terminal());
        for target in TaskState::ALL {
            let reopen = terminal == TaskState::Done && *target == TaskState::Ready;
            assert_eq!(
                terminal.can_transition_to(*target),
                reopen,
                "{terminal} -> {target} is not the reachability the table declares"
            );
        }
    }
    // And only a completed task is reopenable at all.
    assert!(TaskState::Done.is_reopenable());
    assert!(!TaskState::Failed.is_reopenable());
    assert!(!TaskState::Cancelled.is_reopenable());
}

/// A completed task can be reopened, and only by something that says so.
///
/// The bounded exception, stated as its own oracle: one source, one target, and
/// an authority carrying the receipt it was granted by. Everything else about a
/// terminal task stays immutable.
#[test]
fn a_completed_task_reopens_only_under_an_explicit_authority() {
    let receipt = CommandReceiptId::generate();
    let authority = kontor_core::state::TaskReopenAuthority::granted_by(receipt);
    assert_eq!(
        authority.receipt(),
        receipt,
        "the authority carries the receipt an audit reads"
    );

    let reopen = TaskTransition {
        reopen: Some(authority),
        resume_receipt: Some(receipt),
        ..TaskTransition::to(TaskState::Ready)
    };
    assert_eq!(
        apply_task_transition(TaskState::Done, &reopen).expect("an authorized reopen is allowed"),
        TaskState::Ready
    );

    // The same authority cannot resurrect an outcome: a failed or cancelled task
    // has a successor, not a second life. The refusal names the transition rather
    // than the terminality, because the terminality is not what stopped it.
    for outcome in [TaskState::Failed, TaskState::Cancelled] {
        let error = apply_task_transition(outcome, &reopen)
            .expect_err("only a completed task may be reopened");
        assert_eq!(
            error,
            DomainError::IllegalTransition {
                subject: "task reopen",
                from: outcome.as_str(),
                to: "ready",
            }
        );
    }

    // And it reopens to `ready` and nowhere else.
    for target in [TaskState::InProgress, TaskState::Todo, TaskState::Blocked] {
        let error = apply_task_transition(
            TaskState::Done,
            &TaskTransition {
                reopen: Some(authority),
                ..TaskTransition::to(target)
            },
        )
        .expect_err("a reopen returns a task to ready");
        assert!(matches!(
            error,
            DomainError::IllegalTransition {
                subject: "task reopen",
                ..
            }
        ));
    }

    // A reopen of something that is not terminal is not a resume in disguise.
    let error = apply_task_transition(
        TaskState::Blocked,
        &TaskTransition {
            reopen: Some(authority),
            resume_receipt: Some(receipt),
            ..TaskTransition::to(TaskState::Ready)
        },
    )
    .expect_err("there is nothing to reopen");
    assert!(matches!(
        error,
        DomainError::IllegalTransition {
            subject: "task reopen",
            ..
        }
    ));
}

#[test]
fn a_terminal_task_is_immutable() {
    for terminal in [TaskState::Done, TaskState::Failed, TaskState::Cancelled] {
        let error = apply_task_transition(terminal, &TaskTransition::to(TaskState::Ready))
            .expect_err("a terminal task must not transition");
        assert_eq!(error, DomainError::Terminal { subject: "task" });
    }

    // A resume receipt is not a reopen authority. The two are carried by the same
    // kind of receipt, so this is the assertion that keeps an ordinary resume from
    // walking a completed task back open.
    let error = apply_task_transition(
        TaskState::Done,
        &TaskTransition {
            resume_receipt: Some(CommandReceiptId::generate()),
            ..TaskTransition::to(TaskState::Ready)
        },
    )
    .expect_err("a resume must not reopen a completed task");
    assert_eq!(error, DomainError::Terminal { subject: "task" });
}

#[test]
fn leaving_a_held_state_requires_a_command_receipt() {
    for held in [TaskState::Blocked, TaskState::Parked, TaskState::NeedsHuman] {
        let error = apply_task_transition(held, &TaskTransition::to(TaskState::Ready))
            .expect_err("a held task must not resume itself");
        assert!(matches!(error, DomainError::MissingAuthority { .. }));

        let with_receipt = TaskTransition {
            resume_receipt: Some(CommandReceiptId::generate()),
            ..TaskTransition::to(TaskState::Ready)
        };
        assert_eq!(
            apply_task_transition(held, &with_receipt).expect("a receipt authorizes the resume"),
            TaskState::Ready
        );
    }
}

#[test]
fn failing_a_task_requires_its_run_to_have_closed_failed() {
    let error = apply_task_transition(
        TaskState::InProgress,
        &TaskTransition::to(TaskState::Failed),
    )
    .expect_err("a task cannot fail without run evidence");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));

    let succeeded = TaskTransition {
        run_outcome: Some(TerminalOutcome::Succeeded),
        ..TaskTransition::to(TaskState::Failed)
    };
    assert!(
        apply_task_transition(TaskState::InProgress, &succeeded).is_err(),
        "a succeeded run cannot evidence a failed task"
    );

    let failed = TaskTransition {
        run_outcome: Some(TerminalOutcome::Failed),
        ..TaskTransition::to(TaskState::Failed)
    };
    assert_eq!(
        apply_task_transition(TaskState::InProgress, &failed).expect("evidenced failure"),
        TaskState::Failed
    );
}

#[test]
fn closing_a_task_requires_the_profile_to_certify_closure() {
    let snapshot = ResolvedWorkProfileSnapshot::resolve(&profile(), at("2026-08-09T09:00:00Z"))
        .expect("the fixture profile resolves");

    let all_phases: BTreeSet<PhaseKey> = snapshot
        .definition
        .phases
        .iter()
        .map(|phase| phase.id.clone())
        .collect();
    let all_artifacts: BTreeSet<ArtifactKey> = snapshot
        .definition
        .artifacts
        .iter()
        .map(|artifact| artifact.key.clone())
        .collect();
    let gate = GateKey::parse("q7.attest.sign").expect("valid gate key");

    // An outstanding gate blocks closure.
    let outstanding: BTreeMap<GateKey, GateState> =
        [(gate.clone(), GateState::Active)].into_iter().collect();
    assert!(
        snapshot
            .certify_closure(&all_phases, &outstanding, &all_artifacts)
            .is_err(),
        "an active gate must not certify closure"
    );

    // So does an incomplete phase set.
    let passed: BTreeMap<GateKey, GateState> =
        [(gate.clone(), GateState::Passed)].into_iter().collect();
    assert!(
        snapshot
            .certify_closure(&BTreeSet::new(), &passed, &all_artifacts)
            .is_err(),
        "an unfinished phase must not certify closure"
    );

    // And so does a missing artifact.
    assert!(
        snapshot
            .certify_closure(&all_phases, &passed, &BTreeSet::new())
            .is_err(),
        "a missing required artifact must not certify closure"
    );

    let certificate = snapshot
        .certify_closure(&all_phases, &passed, &all_artifacts)
        .expect("a complete profile certifies closure");
    assert!(certificate.is_certified());

    assert!(
        apply_task_transition(TaskState::InProgress, &TaskTransition::to(TaskState::Done)).is_err(),
        "closure cannot be asserted without a certificate"
    );
    let closing = TaskTransition {
        closure: Some(&certificate),
        ..TaskTransition::to(TaskState::Done)
    };
    assert_eq!(
        apply_task_transition(TaskState::InProgress, &closing).expect("certified closure"),
        TaskState::Done
    );
}

#[test]
fn a_ready_task_completes_on_the_same_certificate_an_in_progress_one_needs() {
    // A task can reach `ready` with its work already finished: a reconcile that
    // resumes a task whose seats have gone, or a run that closes before the row
    // is moved on. Under a table without this arm those tasks were unfinishable
    // — every gate passed, every slot settled, and no legal transition left.
    assert!(
        TaskState::Ready.can_transition_to(TaskState::Done),
        "a ready task must have a legal way to complete"
    );

    // Structural legality is not what protects closure, and never was. Without a
    // certificate the transition is refused for exactly the same reason, and
    // with exactly the same error, as it is from `in_progress`.
    let bare = apply_task_transition(TaskState::Ready, &TaskTransition::to(TaskState::Done))
        .expect_err("a ready task cannot assert its own closure");
    assert!(
        matches!(bare, DomainError::MissingEvidence { .. }),
        "the refusal names missing evidence, not an illegal transition: {bare:?}"
    );
    let from_in_progress =
        apply_task_transition(TaskState::InProgress, &TaskTransition::to(TaskState::Done))
            .expect_err("an in-progress task cannot assert its own closure either");
    assert_eq!(
        format!("{bare:?}"),
        format!("{from_in_progress:?}"),
        "both states are refused on the same evidence rule"
    );

    let snapshot = ResolvedWorkProfileSnapshot::resolve(&profile(), at("2026-08-09T09:00:00Z"))
        .expect("the fixture profile resolves");
    let all_phases: BTreeSet<PhaseKey> = snapshot
        .definition
        .phases
        .iter()
        .map(|phase| phase.id.clone())
        .collect();
    let all_artifacts: BTreeSet<ArtifactKey> = snapshot
        .definition
        .artifacts
        .iter()
        .map(|artifact| artifact.key.clone())
        .collect();
    let passed: BTreeMap<GateKey, GateState> = [(
        GateKey::parse("q7.attest.sign").expect("valid gate key"),
        GateState::Passed,
    )]
    .into_iter()
    .collect();
    let certificate = snapshot
        .certify_closure(&all_phases, &passed, &all_artifacts)
        .expect("a complete profile certifies closure");

    let closing = TaskTransition {
        closure: Some(&certificate),
        ..TaskTransition::to(TaskState::Done)
    };
    assert_eq!(
        apply_task_transition(TaskState::Ready, &closing).expect("certified closure from ready"),
        TaskState::Done
    );

    // The arm is narrow: it adds a way to *finish* a ready task, not a way to
    // skip the evidence any other terminal state demands.
    assert!(
        apply_task_transition(TaskState::Ready, &TaskTransition::to(TaskState::Failed)).is_err(),
        "a ready task still cannot fail without run evidence"
    );
}

#[test]
fn a_waiver_the_profile_forbids_cannot_certify_closure() {
    let mut definition = profile();
    definition.gates[0].waiver_allowed = false;
    definition.gates[0].waiver_roles.clear();
    let snapshot = ResolvedWorkProfileSnapshot::resolve(&definition, at("2026-08-09T09:00:00Z"))
        .expect("the profile still resolves without a waiver route");

    let phases: BTreeSet<PhaseKey> = snapshot
        .definition
        .phases
        .iter()
        .map(|phase| phase.id.clone())
        .collect();
    let artifacts: BTreeSet<ArtifactKey> = snapshot
        .definition
        .artifacts
        .iter()
        .map(|artifact| artifact.key.clone())
        .collect();
    let waived: BTreeMap<GateKey, GateState> = [(
        GateKey::parse("q7.attest.sign").expect("valid gate key"),
        GateState::Waived,
    )]
    .into_iter()
    .collect();

    let error = snapshot
        .certify_closure(&phases, &waived, &artifacts)
        .expect_err("waiving a non-waivable gate must not close a task");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));
}

// ---------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------

#[test]
fn gate_verdicts_and_states_stay_distinct() {
    assert_eq!(GateVerdict::Passed.resulting_state(), GateState::Passed);
    assert_eq!(GateVerdict::Started.resulting_state(), GateState::Active);
    assert!(GateVerdict::Passed.requires_evidence());
    assert!(GateVerdict::Waived.requires_evidence());
    assert!(!GateVerdict::Started.requires_evidence());

    assert!(GateState::Passed.satisfies_requirement());
    assert!(GateState::Waived.satisfies_requirement());
    for unsatisfied in [
        GateState::NotReady,
        GateState::Ready,
        GateState::Active,
        GateState::Rejected,
        GateState::Parked,
    ] {
        assert!(!unsatisfied.satisfies_requirement());
    }
}

// ---------------------------------------------------------------------------
// Orthogonality of the run dimensions
// ---------------------------------------------------------------------------

#[test]
fn desired_observed_and_derived_can_all_differ_at_once() {
    let binding = identity(3);
    let observed = observation(ObservedRunState::Running, 3);
    let derived = derive_run_state(&RunDerivation {
        desired: DesiredRunState::CancelRequested,
        observation: Some(&observed),
        binding: Some(&binding),
        freshness: Freshness::Fresh,
        contact: RuntimeContact::Reachable,
        terminal: None,
    })
    .expect("derivation without closure evidence succeeds");

    assert_eq!(derived, DerivedRunState::Diverged);
    assert_eq!(observed.state, ObservedRunState::Running);
    assert!(derived.is_uncertain());
    assert!(!derived.is_terminal());
    // The three dimensions hold three different values simultaneously.
    assert_ne!(
        DesiredRunState::CancelRequested.as_str(),
        observed.state.as_str()
    );
    assert_ne!(observed.state.as_str(), derived.as_str());
}

#[test]
fn uncertainty_is_never_completion() {
    let binding = identity(1);
    let observed = observation(ObservedRunState::Running, 1);

    let cases = [
        (RuntimeContact::ProcessMissing, DerivedRunState::LostContact),
        (RuntimeContact::StreamClosed, DerivedRunState::LostContact),
        (
            RuntimeContact::Unavailable,
            DerivedRunState::RuntimeUnavailable,
        ),
    ];
    for (contact, expected) in cases {
        let derived = derive_run_state(&RunDerivation {
            desired: DesiredRunState::RunRequested,
            observation: Some(&observed),
            binding: Some(&binding),
            freshness: Freshness::Fresh,
            contact,
            terminal: None,
        })
        .expect("derivation succeeds");
        assert_eq!(derived, expected);
        assert!(!derived.is_terminal(), "{contact} must not close a run");
    }
}

#[test]
fn a_stale_observation_and_a_changed_generation_are_not_confirmations() {
    let binding = identity(1);

    let stale = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: Some(&observation(ObservedRunState::Running, 1)),
        binding: Some(&binding),
        freshness: Freshness::Stale,
        contact: RuntimeContact::Reachable,
        terminal: None,
    })
    .expect("derivation succeeds");
    assert_eq!(stale, DerivedRunState::Stale);

    // The runtime restarted: the native id belongs to a generation we never
    // bound to, so the session is orphaned rather than confirmed.
    let orphaned = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: Some(&observation(ObservedRunState::Running, 2)),
        binding: Some(&binding),
        freshness: Freshness::Fresh,
        contact: RuntimeContact::Reachable,
        terminal: None,
    })
    .expect("derivation succeeds");
    assert_eq!(orphaned, DerivedRunState::Orphaned);

    // A native session reported against a run with no binding at all.
    let unbound = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: Some(&observation(ObservedRunState::Running, 1)),
        binding: None,
        freshness: Freshness::Fresh,
        contact: RuntimeContact::Reachable,
        terminal: None,
    })
    .expect("derivation succeeds");
    assert_eq!(unbound, DerivedRunState::Orphaned);
}

#[test]
fn a_confirmed_run_needs_intent_observation_binding_and_freshness_to_agree() {
    let binding = identity(1);
    let derived = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: Some(&observation(ObservedRunState::Running, 1)),
        binding: Some(&binding),
        freshness: Freshness::Fresh,
        contact: RuntimeContact::Reachable,
        terminal: None,
    })
    .expect("derivation succeeds");
    assert_eq!(derived, DerivedRunState::Confirmed);
}

#[test]
fn terminal_evidence_must_belong_to_the_closed_run_and_match_its_hash() {
    let cursor = EventCursor::parse(9).expect("a positive cursor");
    let evidence = TerminalEvidence {
        outcome: TerminalOutcome::Succeeded,
        source: TerminalEvidenceSource::RuntimeObservation { cursor },
        evidence_hash: hash(),
        closed_at: at("2026-08-09T11:00:00Z"),
    };
    evidence
        .validate()
        .expect("a runtime source is structurally fine");

    // The cited event must actually report a terminal state...
    evidence
        .verify_observation(
            ObservedRunState::Succeeded,
            at("2026-08-09T10:00:00Z"),
            &hash(),
        )
        .expect("a matching terminal observation closes the run");
    assert!(
        evidence
            .verify_observation(
                ObservedRunState::Running,
                at("2026-08-09T10:00:00Z"),
                &hash()
            )
            .is_err(),
        "a running event is not closure evidence"
    );
    // ...report *this* outcome...
    assert!(
        evidence
            .verify_observation(
                ObservedRunState::Failed,
                at("2026-08-09T10:00:00Z"),
                &hash()
            )
            .is_err(),
        "an event evidencing a different outcome must be refused"
    );
    // ...and hash to what the closure claims.
    assert!(
        evidence
            .verify_observation(
                ObservedRunState::Succeeded,
                at("2026-08-09T10:00:00Z"),
                &ContentHash::of(b"a different payload")
            )
            .is_err(),
        "a mismatched payload digest must be refused"
    );
    // A run cannot close before the evidence it cites was observed.
    assert!(
        evidence
            .verify_observation(
                ObservedRunState::Succeeded,
                at("2026-08-09T12:00:00Z"),
                &hash()
            )
            .is_err(),
        "closing before the evidence exists must be refused"
    );
}

#[test]
fn operator_abandon_can_only_claim_abandoned() {
    let receipt_id = CommandReceiptId::generate();
    let closing = AggregateRevision::parse(4).expect("a positive revision");
    let abandoned = TerminalEvidence {
        outcome: TerminalOutcome::Abandoned,
        source: TerminalEvidenceSource::OperatorAbandon { receipt_id },
        evidence_hash: hash(),
        closed_at: at("2026-08-09T11:00:00Z"),
    };
    let stored = AbandonReceiptFacts {
        kind_is_abandon: true,
        targets_aggregate: true,
        target_revision: closing,
        intent_hash: hash(),
        recorded_at: at("2026-08-09T10:00:00Z"),
    };
    abandoned.validate().expect("an operator may abandon a run");
    abandoned
        .verify_abandon(closing, &stored)
        .expect("a matching abandon receipt closes the run");

    // Everything else an operator might wish for is refused: cancellation needs
    // a trusted cancelled observation, and a park stays pending until some
    // trusted terminal fact exists.
    for outcome in [
        TerminalOutcome::Succeeded,
        TerminalOutcome::Failed,
        TerminalOutcome::Cancelled,
        TerminalOutcome::Parked,
    ] {
        let forged = TerminalEvidence {
            outcome,
            source: TerminalEvidenceSource::OperatorAbandon { receipt_id },
            evidence_hash: hash(),
            closed_at: at("2026-08-09T11:00:00Z"),
        };
        let error = forged
            .validate()
            .expect_err("an operator receipt cannot evidence a runtime verdict");
        assert!(
            matches!(error, DomainError::MissingAuthority { .. }),
            "{outcome} must not be claimable by an operator"
        );
        assert!(forged.verify_abandon(closing, &stored).is_err());
    }

    // The stored receipt must be an abandon command, aimed at this aggregate at
    // this exact revision, carrying this intent digest, recorded before the
    // closure. Each fact is falsified on its own so no single one can be
    // dropped without a failure here.
    for (label, broken) in [
        (
            "a receipt that is not an abandon command",
            AbandonReceiptFacts {
                kind_is_abandon: false,
                ..stored.clone()
            },
        ),
        (
            "a receipt targeting another aggregate",
            AbandonReceiptFacts {
                targets_aggregate: false,
                ..stored.clone()
            },
        ),
        (
            "a receipt decided against another revision",
            AbandonReceiptFacts {
                target_revision: AggregateRevision::parse(3).expect("a positive revision"),
                ..stored.clone()
            },
        ),
        (
            "a receipt with a different intent digest",
            AbandonReceiptFacts {
                intent_hash: ContentHash::of(b"another intent"),
                ..stored.clone()
            },
        ),
        (
            "a receipt recorded after the closure",
            AbandonReceiptFacts {
                recorded_at: at("2026-08-09T11:30:00Z"),
                ..stored.clone()
            },
        ),
    ] {
        assert!(
            abandoned.verify_abandon(closing, &broken).is_err(),
            "{label} must be refused"
        );
    }
}

#[test]
fn closure_evidence_is_the_only_route_to_terminal() {
    let evidence = TerminalEvidence {
        outcome: TerminalOutcome::Failed,
        source: TerminalEvidenceSource::RuntimeObservation {
            cursor: EventCursor::parse(10).expect("a positive cursor"),
        },
        evidence_hash: hash(),
        closed_at: at("2026-08-09T11:30:00Z"),
    };
    let derived = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: None,
        binding: None,
        freshness: Freshness::Unknown,
        contact: RuntimeContact::ProcessMissing,
        terminal: Some(&evidence),
    })
    .expect("valid evidence closes the run");
    assert_eq!(
        derived,
        DerivedRunState::Terminal {
            outcome: TerminalOutcome::Failed
        }
    );
}

// ---------------------------------------------------------------------------
// Team runs
// ---------------------------------------------------------------------------

/// One closed child run, with the digest its own closure was bound to.
fn child(outcome: TerminalOutcome, marker: &[u8]) -> TeamChildEvidence {
    TeamChildEvidence {
        agent_run_id: AgentRunId::generate(),
        lifecycle: outcome.lifecycle(),
        evidence_hash: Some(ContentHash::of(marker)),
    }
}

/// Evidence whose digest is genuinely computed from the children it cites.
fn child_evidence(team: TeamRunId, children: &[TeamChildEvidence]) -> TeamTerminalEvidence {
    let lifecycles: Vec<RunLifecycle> = children.iter().map(|c| c.lifecycle).collect();
    TeamTerminalEvidence {
        outcome: reduce_team_outcome(&lifecycles).unwrap_or(TerminalOutcome::Failed),
        source: TeamEvidenceSource::ChildEvidence { team_run_id: team },
        evidence_hash: team_child_evidence_digest(children).expect("the children digest"),
        closed_at: at("2026-08-09T12:00:00Z"),
    }
}

/// The team digest, recomputed here without touching the production helper.
///
/// Deliberately duplicated. If the expected value came from
/// [`team_child_evidence_digest`] then any change to that function — including
/// dropping its sort — would move the expectation and the actual value together,
/// and the assertion would hold no matter what the function did.
///
/// This spells out the contract independently: SHA-256 over the canonical JSON
/// of the children ordered by run id, with object keys in sorted order. Children
/// are ordered here by the canonical UUID *text*, which for same-length
/// lowercase hyphenated UUIDs is the same order as their byte values — reached
/// without relying on the production type's `Ord`.
fn independent_team_digest(children: &[TeamChildEvidence]) -> ContentHash {
    let mut ordered: Vec<&TeamChildEvidence> = children.iter().collect();
    ordered.sort_by_key(|child| child.agent_run_id.to_string());
    let entries: Vec<String> = ordered
        .iter()
        .map(|child| {
            let hash = child
                .evidence_hash
                .as_ref()
                .map_or_else(|| "null".to_owned(), |h| format!("\"{}\"", h.as_str()));
            format!(
                "{{\"agent_run_id\":\"{}\",\"evidence_hash\":{},\"lifecycle\":\"{}\"}}",
                child.agent_run_id, hash, child.lifecycle
            )
        })
        .collect();
    ContentHash::of(
        format!(
            "{{\"children\":[{}],\"schema_version\":1}}",
            entries.join(",")
        )
        .as_bytes(),
    )
}

#[test]
fn the_team_child_digest_is_order_independent_and_matches_an_independent_hash() {
    // UUIDv7 is time-ordered, so these are generated in ascending id order.
    let first = child(TerminalOutcome::Succeeded, b"child-a");
    let second = child(TerminalOutcome::Failed, b"child-b");
    let third = TeamChildEvidence {
        // An open child, to pin down how `None` is encoded as well.
        agent_run_id: AgentRunId::generate(),
        lifecycle: RunLifecycle::Running,
        evidence_hash: None,
    };
    let ascending = vec![first.clone(), second.clone(), third.clone()];

    // Guard the guard: permuting a one-element set proves nothing, and neither
    // does permuting a set whose elements happen to be equal.
    assert!(ascending.len() > 2);
    let distinct: BTreeSet<_> = ascending.iter().map(|c| c.agent_run_id).collect();
    assert_eq!(distinct.len(), ascending.len(), "the children must differ");

    let expected = independent_team_digest(&ascending);
    assert_eq!(
        team_child_evidence_digest(&ascending).expect("the digest computes"),
        expected,
        "the production digest must match the contract spelled out independently"
    );

    // Every permutation of the same set yields that same digest. At least one of
    // these is not in ascending order, so a digest that hashed its input as
    // given would disagree with `expected` here.
    let permutations = [
        (
            "reversed",
            vec![third.clone(), second.clone(), first.clone()],
        ),
        (
            "rotated",
            vec![second.clone(), third.clone(), first.clone()],
        ),
        (
            "swapped tail",
            vec![first.clone(), third.clone(), second.clone()],
        ),
    ];
    for (label, permutation) in &permutations {
        assert_ne!(
            permutation, &ascending,
            "the {label} case must actually be a different input order"
        );
        assert_eq!(
            team_child_evidence_digest(permutation).expect("the digest computes"),
            expected,
            "the {label} order must produce the same digest as the set it permutes"
        );
    }

    // The digest still depends on the *content*: dropping a child, changing a
    // lifecycle, or substituting a child's own closure digest all move it.
    for (label, altered) in [
        ("a missing child", vec![first.clone(), second.clone()]),
        (
            "a changed lifecycle",
            vec![
                TeamChildEvidence {
                    lifecycle: RunLifecycle::Cancelled,
                    ..first.clone()
                },
                second.clone(),
                third.clone(),
            ],
        ),
        (
            "a substituted child closure",
            vec![
                TeamChildEvidence {
                    evidence_hash: Some(ContentHash::of(b"a different closure")),
                    ..first.clone()
                },
                second.clone(),
                third.clone(),
            ],
        ),
    ] {
        assert_ne!(
            team_child_evidence_digest(&altered).expect("the digest computes"),
            expected,
            "{label} must change the team digest"
        );
    }
}

fn abandon_facts(revision: AggregateRevision, hash: &ContentHash) -> AbandonReceiptFacts {
    AbandonReceiptFacts {
        kind_is_abandon: true,
        targets_aggregate: true,
        target_revision: revision,
        intent_hash: hash.clone(),
        recorded_at: at("2026-08-09T11:00:00Z"),
    }
}

#[test]
fn fresh_runtime_evidence_converges_non_terminal_lifecycle_without_regression() {
    assert_eq!(
        reduce_run_lifecycle(RunLifecycle::Queued, ObservedRunState::Launching),
        RunLifecycle::Launching
    );
    assert_eq!(
        reduce_run_lifecycle(RunLifecycle::Queued, ObservedRunState::Running),
        RunLifecycle::Running,
        "a lost launch acknowledgement cannot keep a proven live run queued"
    );
    assert_eq!(
        reduce_run_lifecycle(RunLifecycle::Queued, ObservedRunState::WaitingInput),
        RunLifecycle::WaitingInput
    );
    assert_eq!(
        reduce_run_lifecycle(RunLifecycle::WaitingInput, ObservedRunState::Running),
        RunLifecycle::Running
    );
    assert_eq!(
        reduce_run_lifecycle(RunLifecycle::Running, ObservedRunState::Launching),
        RunLifecycle::Running,
        "late launch evidence never regresses active work"
    );
    assert_eq!(
        reduce_run_lifecycle(RunLifecycle::Running, ObservedRunState::Succeeded),
        RunLifecycle::Running,
        "terminal observations require the evidence-bearing closure path"
    );
}

#[test]
fn team_run_advances_and_closes_with_cas_and_bound_evidence() {
    let team = TeamRunId::generate();
    let first = AggregateRevision::INITIAL;

    // --- CAS advance -------------------------------------------------------
    // A terminal value is never reached through an advance...
    assert_eq!(
        plan_team_advance(RunLifecycle::Queued, first, first, RunLifecycle::Succeeded)
            .expect_err("closure is evidence-bearing, not an advance"),
        DomainError::IllegalTransition {
            subject: "team run",
            from: "queued",
            to: "succeeded"
        }
    );
    // ...nor through an illegal one.
    assert!(
        plan_team_advance(
            RunLifecycle::Queued,
            first,
            first,
            RunLifecycle::WaitingInput
        )
        .is_err(),
        "queued cannot jump straight to waiting_input"
    );

    let second = plan_team_advance(RunLifecycle::Queued, first, first, RunLifecycle::Launching)
        .expect("a declared advance succeeds");
    assert_eq!(second.get(), first.get() + 1);

    // A stale expectation is a revision conflict, not a silent no-op.
    assert_eq!(
        plan_team_advance(
            RunLifecycle::Launching,
            second,
            first,
            RunLifecycle::Running
        )
        .expect_err("a stale revision must be refused"),
        DomainError::RevisionConflict {
            subject: "team run",
            expected: first.get(),
            found: second.get()
        }
    );
    let running = plan_team_advance(
        RunLifecycle::Launching,
        second,
        second,
        RunLifecycle::Running,
    )
    .expect("the team runs");

    // --- closure on computed, bound child evidence -------------------------
    let close = |current: RunLifecycle,
                 stored: AggregateRevision,
                 expected: AggregateRevision,
                 evidence: &TeamTerminalEvidence,
                 children: &[TeamChildEvidence],
                 receipt: Option<&AbandonReceiptFacts>| {
        plan_team_closure(current, stored, expected, team, evidence, children, receipt)
    };

    let succeeded = vec![
        child(TerminalOutcome::Succeeded, b"child-a"),
        child(TerminalOutcome::Succeeded, b"child-b"),
    ];
    let evidence = child_evidence(team, &succeeded);
    assert_eq!(evidence.outcome, TerminalOutcome::Succeeded);

    // An open child blocks closure, and a team with no children has nothing to
    // close on at all: the outcome is computed, never asserted.
    let mut open = succeeded.clone();
    open[1].lifecycle = RunLifecycle::Running;
    assert_eq!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &evidence,
            &open,
            None
        )
        .expect_err("an open child must block team closure"),
        DomainError::MissingEvidence {
            subject: "team closure",
            rule: "every child run must be terminal before the team closes"
        }
    );
    assert!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &evidence,
            &[],
            None
        )
        .is_err()
    );

    // The digest is recomputed from the persisted children, so an arbitrary
    // hash cannot be stored as if it were evidence. This is the mutant that
    // "recompute the outcome only" leaves alive.
    let forged = TeamTerminalEvidence {
        evidence_hash: ContentHash::of(b"anything at all"),
        ..evidence.clone()
    };
    assert_eq!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &forged,
            &succeeded,
            None
        )
        .expect_err("an unbound digest must be refused"),
        DomainError::MissingEvidence {
            subject: "team closure",
            rule: "the digest does not match the team's own child evidence"
        }
    );
    // Substituting a child's own closure digest changes the team digest, so the
    // binding reaches all the way down to each child's evidence.
    let mut swapped = succeeded.clone();
    swapped[0].evidence_hash = Some(ContentHash::of(b"a different child closure"));
    assert!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &evidence,
            &swapped,
            None
        )
        .is_err(),
        "the digest must cover each child's own bound evidence"
    );

    // An outcome the children do not compute is refused...
    let claimed_failed = TeamTerminalEvidence {
        outcome: TerminalOutcome::Failed,
        ..evidence.clone()
    };
    assert!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &claimed_failed,
            &succeeded,
            None
        )
        .is_err(),
        "the claimed outcome must match what the children compute"
    );
    // ...and so is evidence citing another team, even with a valid digest.
    let other_team = TeamTerminalEvidence {
        source: TeamEvidenceSource::ChildEvidence {
            team_run_id: TeamRunId::generate(),
        },
        ..evidence.clone()
    };
    assert_eq!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &other_team,
            &succeeded,
            None
        )
        .expect_err("child evidence must belong to the team being closed"),
        DomainError::MissingEvidence {
            subject: "team closure",
            rule: "the cited child evidence belongs to a different team"
        }
    );
    // A stale expectation is refused on the closure path too.
    assert!(
        close(
            RunLifecycle::Running,
            running,
            second,
            &evidence,
            &succeeded,
            None
        )
        .is_err(),
        "closure is a compare-and-swap like any other write"
    );

    let closed = close(
        RunLifecycle::Running,
        running,
        running,
        &evidence,
        &succeeded,
        None,
    )
    .expect("computed, bound child evidence closes the team");
    assert_eq!(closed.get(), running.get() + 1);

    // --- operator abandon --------------------------------------------------
    let intent = ContentHash::of(b"abandon-intent");
    let abandon = TeamTerminalEvidence {
        outcome: TerminalOutcome::Abandoned,
        source: TeamEvidenceSource::OperatorAbandon {
            receipt_id: CommandReceiptId::generate(),
        },
        evidence_hash: intent.clone(),
        closed_at: at("2026-08-09T12:00:00Z"),
    };
    let facts = abandon_facts(running, &intent);
    close(
        RunLifecycle::Running,
        running,
        running,
        &abandon,
        &[],
        Some(&facts),
    )
    .expect("a matching abandon receipt closes the team");

    // An operator may only ever abandon.
    assert!(
        close(
            RunLifecycle::Running,
            running,
            running,
            &TeamTerminalEvidence {
                outcome: TerminalOutcome::Succeeded,
                ..abandon.clone()
            },
            &succeeded,
            Some(&facts),
        )
        .is_err(),
        "an operator receipt can only evidence an abandoned team"
    );
    // The receipt must exist, be an abandon, target this team, cite this
    // revision and carry this digest.
    assert!(
        close(RunLifecycle::Running, running, running, &abandon, &[], None).is_err(),
        "the cited receipt must actually be stored"
    );
    for (label, broken) in [
        (
            "another kind of command",
            AbandonReceiptFacts {
                kind_is_abandon: false,
                ..facts.clone()
            },
        ),
        (
            "another aggregate",
            AbandonReceiptFacts {
                targets_aggregate: false,
                ..facts.clone()
            },
        ),
        (
            "another revision of this team",
            AbandonReceiptFacts {
                target_revision: second,
                ..facts.clone()
            },
        ),
        (
            "another intent digest",
            AbandonReceiptFacts {
                intent_hash: ContentHash::of(b"some other intent"),
                ..facts.clone()
            },
        ),
        (
            "a receipt recorded after the closure",
            AbandonReceiptFacts {
                recorded_at: at("2026-08-09T13:00:00Z"),
                ..facts.clone()
            },
        ),
    ] {
        assert!(
            close(
                RunLifecycle::Running,
                running,
                running,
                &abandon,
                &[],
                Some(&broken)
            )
            .is_err(),
            "{label} must not close the team"
        );
    }

    // --- immutability after terminal ---------------------------------------
    // A closed team neither advances nor closes again, and the refusal is
    // terminality rather than a revision conflict.
    for terminal in [
        RunLifecycle::Succeeded,
        RunLifecycle::Failed,
        RunLifecycle::Cancelled,
        RunLifecycle::Parked,
    ] {
        assert_eq!(
            plan_team_advance(terminal, closed, closed, RunLifecycle::Running)
                .expect_err("a terminal team never reopens"),
            DomainError::Terminal {
                subject: "team run"
            }
        );
        assert_eq!(
            close(terminal, closed, closed, &evidence, &succeeded, None)
                .expect_err("a terminal team never closes twice"),
            DomainError::Terminal {
                subject: "team run"
            }
        );
    }
}

#[test]
fn run_lifecycle_and_task_lifecycle_are_not_interchangeable() {
    for lifecycle in RunLifecycle::ALL {
        let terminal = lifecycle.is_terminal();
        assert_eq!(terminal, lifecycle.terminal_outcome().is_some());
        // No run lifecycle spelling is also a task state spelling that means the
        // same thing: `parked` is terminal for a run and non-terminal for a
        // task.
        if *lifecycle == RunLifecycle::Parked {
            assert!(terminal);
            assert!(!TaskState::Parked.is_terminal());
        }
    }
    assert_eq!(
        TerminalOutcome::Abandoned.lifecycle(),
        RunLifecycle::Parked,
        "an abandoned run closes without claiming a runtime verdict"
    );
}

#[test]
fn freshness_is_measured_not_assumed() {
    let now = at("2026-08-09T12:00:00Z");
    let window = jiff::SignedDuration::from_secs(60);
    assert_eq!(Freshness::evaluate(None, now, window), Freshness::Unknown);
    assert_eq!(
        Freshness::evaluate(Some(at("2026-08-09T11:59:30Z")), now, window),
        Freshness::Fresh
    );
    assert_eq!(
        Freshness::evaluate(Some(at("2026-08-09T11:55:00Z")), now, window),
        Freshness::Stale
    );
}

// ---------------------------------------------------------------------------
// Revisions
// ---------------------------------------------------------------------------

#[test]
fn revisions_start_at_one_and_only_move_under_a_compare_and_swap() {
    assert!(AggregateRevision::parse(0).is_err());
    let first = AggregateRevision::INITIAL;
    assert_eq!(first.get(), 1);
    let second = first.next().expect("revision increments");
    assert_eq!(second.get(), 2);

    first.expect("task", first).expect("matching revision");
    let error = second
        .expect("task", first)
        .expect_err("a stale expectation must be refused");
    assert_eq!(
        error,
        DomainError::RevisionConflict {
            subject: "task",
            expected: 1,
            found: 2
        }
    );
    assert!(SpecVersion::parse(0).is_err());
    assert!(EventCursor::parse(0).is_err());
    assert!(EventCursor::parse(-1).is_err());
}

// ---------------------------------------------------------------------------
// Command receipts
// ---------------------------------------------------------------------------

fn intent(marker: &str) -> CanonicalDocument {
    let value = serde_json::json!({ "schema_version": 1, "marker": marker });
    CanonicalDocument::from_value(&value).expect("a canonical intent")
}

fn receipt(state: CommandReceiptState, target: AggregateRef) -> CommandReceipt {
    CommandReceipt {
        id: CommandReceiptId::generate(),
        project_id: ProjectId::generate(),
        idempotency_key: IdempotencyKey::parse("key-1").expect("valid key"),
        kind: CommandKind::LaunchRun,
        target,
        target_revision: AggregateRevision::INITIAL,
        intent: intent("launch"),
        state,
        correlation: Some(ExternalId::parse("corr-1").expect("valid correlation")),
        native_identity: None,
        result_ref: None,
        attempts: 1,
        created_at: at("2026-08-09T10:00:00Z"),
        updated_at: at("2026-08-09T10:00:00Z"),
    }
}

#[test]
fn an_idempotency_key_is_a_replay_only_for_the_same_command() {
    let target = AggregateRef::Task {
        task_id: kontor_core::id::TaskId::generate(),
    };
    let stored = receipt(CommandReceiptState::Dispatched, target);

    stored
        .ensure_replay(&target, &intent("launch"))
        .expect("a byte-identical intent is a replay");

    assert!(
        stored.ensure_replay(&target, &intent("cancel")).is_err(),
        "a different intent under the same key must fail"
    );
    let other = AggregateRef::Task {
        task_id: kontor_core::id::TaskId::generate(),
    };
    assert!(
        stored.ensure_replay(&other, &intent("launch")).is_err(),
        "a different target under the same key must fail"
    );
}

#[test]
fn acknowledgement_is_not_completion_and_unknown_forbids_a_blind_retry() {
    let target = AggregateRef::AgentRun {
        agent_run_id: kontor_core::id::AgentRunId::generate(),
    };
    let acknowledged = receipt(CommandReceiptState::Acknowledged, target);
    assert!(!acknowledged.state.is_terminal());
    acknowledged
        .transition(CommandReceiptState::Confirmed)
        .expect("confirmation follows acknowledgement");

    let unknown = receipt(CommandReceiptState::ConfirmationUnknown, target);
    let error = unknown
        .transition(CommandReceiptState::DispatchPending)
        .expect_err("a blind retry must be refused");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));

    let evidence = NoEffectEvidence {
        correlation: ExternalId::parse("corr-1").expect("valid correlation"),
        searched_identity: Some(identity(1)),
        reconciled_at: at("2026-08-09T10:05:00Z"),
        evidence_hash: hash(),
    };
    assert_eq!(
        unknown
            .authorize_retry(&evidence)
            .expect("reconciliation authorizes one retry"),
        CommandReceiptState::DispatchPending
    );

    let wrong = NoEffectEvidence {
        correlation: ExternalId::parse("corr-2").expect("valid correlation"),
        ..evidence
    };
    assert!(
        unknown.authorize_retry(&wrong).is_err(),
        "evidence for another correlation proves nothing"
    );
}

#[test]
fn a_settled_command_receipt_is_immutable() {
    let target = AggregateRef::Task {
        task_id: kontor_core::id::TaskId::generate(),
    };
    for settled in [CommandReceiptState::Confirmed, CommandReceiptState::Failed] {
        let stored = receipt(settled, target);
        let error = stored
            .transition(CommandReceiptState::DispatchPending)
            .expect_err("a settled receipt must not move");
        assert_eq!(
            error,
            DomainError::Terminal {
                subject: "command receipt"
            }
        );
    }
}

// ---------------------------------------------------------------------------
// Realm envelopes
// ---------------------------------------------------------------------------

#[test]
fn realm_envelopes_require_matching_realm_and_qualify_cursors() {
    let realm_a = RealmId::generate();
    let realm_b = RealmId::generate();
    assert_ne!(realm_a, realm_b);

    let metadata = RealmMetadata::create(realm_a, at("2026-08-09T10:00:00Z"));
    metadata
        .validate()
        .expect("freshly created metadata is valid");
    assert_eq!(metadata.schema_version, kontor_core::id::SCHEMA_VERSION);
    assert!(metadata.display_label.is_none());
    metadata
        .ensure_matches(realm_a)
        .expect("its own realm matches");
    assert!(matches!(
        metadata.ensure_matches(realm_b),
        Err(DomainError::RealmMismatch { .. })
    ));

    let cursor = EventCursor::parse(41).expect("a positive cursor");

    // Every envelope kind refuses to open under the wrong Realm.
    let snapshot = SnapshotEnvelope::new(realm_a, cursor, "state");
    assert_eq!(*snapshot.peek(realm_a).expect("same realm"), "state");
    assert!(matches!(
        snapshot.peek(realm_b),
        Err(DomainError::RealmMismatch { .. })
    ));
    assert_eq!(snapshot.cursor(), RealmCursor::new(realm_a, cursor));

    let event = EventEnvelope::new(realm_a, cursor, "event");
    assert_eq!(event.realm_cursor(), RealmCursor::new(realm_a, cursor));
    assert_eq!(event.clone().open(realm_a).expect("same realm"), "event");
    assert!(matches!(
        event.open(realm_b),
        Err(DomainError::RealmMismatch { .. })
    ));

    let receipt = ReceiptEnvelope::new(realm_a, "receipt");
    assert!(receipt.peek(realm_b).is_err());
    assert_eq!(*receipt.peek(realm_a).expect("same realm"), "receipt");

    let export = ExportEnvelope::new(realm_a, at("2026-08-09T11:00:00Z"), "export");
    assert_eq!(export.schema_version, kontor_core::id::SCHEMA_VERSION);
    assert!(export.peek(realm_b).is_err());

    // A bare cursor only resolves in the Realm that counts it. Realm A's
    // position 41 and Realm B's position 41 are different places.
    let qualified = RealmCursor::new(realm_a, cursor);
    assert_eq!(qualified.resolve(realm_a).expect("same realm"), cursor);
    let error = qualified
        .resolve(realm_b)
        .expect_err("a foreign cursor must not resolve");
    match error {
        DomainError::RealmMismatch { expected, found } => {
            assert_eq!(expected, realm_b);
            assert_eq!(found, realm_a);
        }
        other => panic!("expected a realm mismatch, got {other:?}"),
    }

    // The mismatch error names the two realms and nothing else: no payload.
    let rendered = ensure_realm(realm_a, realm_b).unwrap_err().to_string();
    assert!(rendered.contains(&realm_a.to_string()));
    assert!(!rendered.contains("state"));
    assert!(!rendered.contains("receipt"));

    // Envelopes round-trip through JSON with the realm attached, which is what
    // KON-MVP-11 will put on every API response and SSE event.
    let json =
        serde_json::to_string(&EventEnvelope::new(realm_a, cursor, "event")).expect("serializes");
    assert!(json.contains(&realm_a.to_string()));
    let restored: EventEnvelope<String> = serde_json::from_str(&json).expect("round-trips");
    assert_eq!(restored.realm_id, realm_a);
    assert_eq!(restored.cursor, cursor);
}

#[test]
fn replayed_or_out_of_order_native_events_never_regress_projection_or_revision() {
    use kontor_core::state::RunProjection;

    // Nothing applied yet: anything the runtime sends is progress.
    assert!(RunProjection::may_reduce(None, 0));
    assert!(RunProjection::may_reduce(None, 7));

    // Strictly newer is the only thing that reduces.
    assert!(RunProjection::may_reduce(Some(5), 6));
    assert!(RunProjection::may_reduce(Some(5), u64::MAX));

    // A replay of the same sequence, and anything behind it, does not.
    assert!(
        !RunProjection::may_reduce(Some(5), 5),
        "a replay is not progress"
    );
    assert!(
        !RunProjection::may_reduce(Some(5), 4),
        "an older event is not progress"
    );
    assert!(!RunProjection::may_reduce(Some(5), 0));
    assert!(!RunProjection::may_reduce(Some(u64::MAX), u64::MAX));

    // The rule is total and monotone: once a sequence has been applied, no
    // value at or below it can ever reduce again.
    for applied in 0u64..8 {
        for incoming in 0u64..8 {
            assert_eq!(
                RunProjection::may_reduce(Some(applied), incoming),
                incoming > applied,
                "applied {applied}, incoming {incoming}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Command kind / target compatibility
// ---------------------------------------------------------------------------

/// Every legal (command kind, aggregate kind) pair, written out by hand.
///
/// Deliberately a flat literal table rather than a second implementation of the
/// production match: an expectation derived from the code under test only ever
/// proves the code equals itself. Columns are the stable spellings of the
/// command kind, the aggregate kind, the revision rule, and the desired run
/// state the command must carry (`None` when it must carry none). Every pair
/// absent from this table must be refused outright.
const LEGAL_COMMAND_TARGETS: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "launch_run",
        "agent_run",
        "compare_and_swap",
        Some("run_requested"),
    ),
    ("launch_run", "team_run", "witness", None),
    (
        "cancel_run",
        "agent_run",
        "compare_and_swap",
        Some("cancel_requested"),
    ),
    ("cancel_run", "team_run", "witness", None),
    (
        "park_run",
        "agent_run",
        "compare_and_swap",
        Some("park_requested"),
    ),
    ("park_run", "team_run", "witness", None),
    (
        "abandon_run",
        "agent_run",
        "compare_and_swap",
        Some("abandon_requested"),
    ),
    ("abandon_run", "team_run", "witness", None),
    ("resume_task", "task", "witness", None),
    ("record_gate_verdict", "task", "witness", None),
    // A proposal is decided before the work it proposes exists, so the project
    // is the only aggregate there is to name at that moment; approving an
    // already-created graph still names that graph.
    ("approve_intake", "project", "witness", None),
    ("approve_intake", "mini_project", "witness", None),
    ("approve_intake", "task", "witness", None),
    ("sync_ticket", "ticket_link", "witness", None),
    ("assign_ticket", "ticket_link", "witness", None),
    ("transition_ticket", "ticket_link", "witness", None),
    ("resolve_status_conflict", "ticket_link", "witness", None),
    ("authorize_execution", "project", "witness", None),
    ("authorize_execution", "mini_project", "witness", None),
    ("authorize_execution", "task", "witness", None),
    ("approve_schedule_override", "project", "witness", None),
    ("approve_schedule_override", "mini_project", "witness", None),
    ("approve_schedule_override", "task", "witness", None),
    ("revoke_schedule_override", "project", "witness", None),
    ("revoke_schedule_override", "mini_project", "witness", None),
    ("revoke_schedule_override", "task", "witness", None),
    ("assign_work_calendar", "work_calendar", "witness", None),
    ("revoke_execution_authorization", "project", "witness", None),
    (
        "revoke_execution_authorization",
        "mini_project",
        "witness",
        None,
    ),
    ("revoke_execution_authorization", "task", "witness", None),
    ("ensure_project", "project", "witness", None),
    ("ensure_account_profile", "project", "witness", None),
    ("import_backlog", "project", "witness", None),
    ("apply_epic_graph", "mini_project", "witness", None),
    ("transition_epic", "mini_project", "witness", None),
    ("start_scheduled_work", "mini_project", "witness", None),
    ("materialize_jira", "mini_project", "witness", None),
    ("activate_asma_epic", "mini_project", "witness", None),
    ("transition_task", "task", "witness", None),
    ("withdraw_task", "task", "witness", None),
    ("resolve_context", "task", "witness", None),
    ("select_task_profile", "task", "witness", None),
    ("select_task_team", "task", "witness", None),
    ("select_task_account", "task", "witness", None),
    ("reconcile_ticket", "task", "witness", None),
    ("settle_runtime", "agent_run", "witness", None),
    ("replace_seat", "team_run", "witness", None),
    // Intake decides about the project's inbound events, and about no narrower
    // aggregate: a decision that creates no work graph has none to name.
    ("submit_intake", "project", "witness", None),
    ("pull_ticket_comments", "task", "witness", None),
    ("claim_ticket", "task", "witness", None),
    // OP-03 CP3. Capacity is a fact about the project's fleet, and a seat is
    // not an aggregate a command may name, so all four witness the project.
    // Spelled out here a second time on purpose: this table is an independent
    // declaration of the matrix, not a mirror of it.
    ("refresh_capacity", "project", "witness", None),
    ("override_availability", "project", "witness", None),
    ("observe_seat", "project", "witness", None),
    ("retire_seat", "project", "witness", None),
    // Publication is authority over the project's vocabulary; an upgrade moves
    // one epic's pin, so the epic is what it names. Whole-estate native-name
    // reconciliation is likewise authorized against exactly that epic.
    ("publish_topology_spec", "project", "witness", None),
    ("select_project_topology", "project", "witness", None),
    ("upgrade_topology", "mini_project", "witness", None),
    ("reconcile_native_names", "mini_project", "witness", None),
    // A native container is not an aggregate a command may name, and the node it
    // belongs to is not one either. The project is what the authority is over.
    ("retitle_container", "project", "witness", None),
    // Project configuration, and deliberately nothing else: publishing a roster
    // seats no epic, so no epic aggregate is a legal target for it.
    ("apply_core_team", "project", "witness", None),
    // Same shape, same reason: publishing a consultation policy document creates
    // no ASW, no CSW and no seat, so no epic or run aggregate is legal for it.
    ("apply_advisor_profile", "project", "witness", None),
    ("apply_committee_template", "project", "witness", None),
    ("ensure_quick_session", "project", "witness", None),
    // Promotion and the two roster commands are about one epic.
    ("promote_quick_session", "mini_project", "witness", None),
    ("materialize_core_team", "mini_project", "witness", None),
    ("correct_core_team_route", "mini_project", "witness", None),
    ("claim_core_team_seat", "mini_project", "witness", None),
    ("upgrade_epic_roster", "mini_project", "witness", None),
    // Publishing a Completion Profile is project configuration, for the same
    // reason as `apply_core_team`: it deliberately does not move any running
    // epic's frozen pin, so no epic aggregate is a legal target for it.
    ("apply_completion_profile", "project", "witness", None),
    // Consultation execution is frozen inside one epic. The native runtime
    // seats and CSW are evidence for that epic, not independent aggregates.
    ("invoke_advisor_run", "mini_project", "witness", None),
    ("settle_advisor_run", "mini_project", "witness", None),
    ("invoke_committee_run", "mini_project", "witness", None),
    ("recover_consultation_seat", "mini_project", "witness", None),
    ("record_committee_findings", "mini_project", "witness", None),
    ("settle_committee_run", "mini_project", "witness", None),
    // The two completion writes are about one epic's own frozen run.
    ("advance_completion", "mini_project", "witness", None),
    ("remediate_completion", "mini_project", "witness", None),
    // Publishing installs an immutable document into the project and names no
    // row inside it: the revision it creates is addressed by `(id, version)`,
    // not by an aggregate carrying a revision of its own.
    ("publish_trigger", "project", "witness", None),
    ("install_workflow_spec", "project", "witness", None),
];

/// One concrete reference per aggregate kind.
fn reference_of(kind: AggregateKind) -> AggregateRef {
    match kind {
        AggregateKind::Project => AggregateRef::Project {
            project_id: ProjectId::generate(),
        },
        AggregateKind::MiniProject => AggregateRef::MiniProject {
            mini_project_id: MiniProjectId::generate(),
        },
        AggregateKind::Task => AggregateRef::Task {
            task_id: TaskId::generate(),
        },
        AggregateKind::TeamRun => AggregateRef::TeamRun {
            team_run_id: TeamRunId::generate(),
        },
        AggregateKind::AgentRun => AggregateRef::AgentRun {
            agent_run_id: AgentRunId::generate(),
        },
        AggregateKind::TicketLink => AggregateRef::TicketLink {
            link_id: TicketLinkId::generate(),
        },
        AggregateKind::WorkCalendar => AggregateRef::WorkCalendar {
            work_calendar_id: WorkCalendarId::generate(),
        },
    }
}

#[test]
fn every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state() {
    let expected: BTreeMap<(&str, &str), (&str, Option<&str>)> = LEGAL_COMMAND_TARGETS
        .iter()
        .map(|(kind, target, revision, desired)| ((*kind, *target), (*revision, *desired)))
        .collect();
    assert_eq!(
        expected.len(),
        LEGAL_COMMAND_TARGETS.len(),
        "the expected table must not name the same pair twice"
    );

    // Both closed sets are walked in full: every command against every
    // aggregate, and every one of those decisions is asserted.
    let mut checked = 0_usize;
    for kind in CommandKind::ALL {
        for target_kind in AggregateKind::ALL {
            checked += 1;
            let target = reference_of(*target_kind);
            let found = kind.rule_for(*target_kind);
            let expectation = expected
                .get(&(kind.as_str(), target_kind.as_str()))
                .copied();

            let Some((revision, required_desired)) = expectation else {
                assert!(
                    found.is_none(),
                    "{kind} must not be able to target a {target_kind}"
                );
                // An illegal pair stays illegal whatever desired state rides
                // along with it.
                for desired in
                    std::iter::once(None).chain(DesiredRunState::ALL.iter().copied().map(Some))
                {
                    assert!(
                        kind.ensure_compatible(&target, desired).is_err(),
                        "{kind} against a {target_kind} must be refused"
                    );
                }
                continue;
            };

            let rule = found.unwrap_or_else(|| panic!("{kind} must accept a {target_kind}"));
            assert_eq!(
                match rule.revision {
                    RevisionRule::CompareAndSwap => "compare_and_swap",
                    RevisionRule::Witness => "witness",
                },
                revision,
                "{kind} against a {target_kind} follows the wrong revision rule"
            );

            match required_desired {
                Some(required) => {
                    assert_eq!(
                        match rule.desired {
                            DesiredStateRule::Requires(state) => Some(state.as_str()),
                            DesiredStateRule::Forbidden => None,
                        },
                        Some(required),
                        "{kind} against a {target_kind} carries the wrong desired state"
                    );
                    // Exactly one desired state is accepted; absence and every
                    // other state are refused.
                    assert!(kind.ensure_compatible(&target, None).is_err());
                    for desired in DesiredRunState::ALL {
                        let accepted = kind.ensure_compatible(&target, Some(*desired)).is_ok();
                        assert_eq!(
                            accepted,
                            desired.as_str() == required,
                            "{kind} against a {target_kind} accepted the wrong desired state"
                        );
                    }
                }
                None => {
                    assert_eq!(
                        rule.desired,
                        DesiredStateRule::Forbidden,
                        "{kind} against a {target_kind} must carry no desired state"
                    );
                    kind.ensure_compatible(&target, None)
                        .expect("a legal pair carrying no desired state is accepted");
                    for desired in DesiredRunState::ALL {
                        assert!(
                            kind.ensure_compatible(&target, Some(*desired)).is_err(),
                            "{kind} against a {target_kind} must refuse a desired state"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(
        checked,
        CommandKind::ALL.len() * AggregateKind::ALL.len(),
        "the whole matrix must be walked"
    );

    // No aggregate is unreachable and no command is inert: a target kind that
    // no command can name would be a column of dead schema, and a command that
    // can target nothing could never be recorded.
    for target_kind in AggregateKind::ALL {
        assert!(
            CommandKind::ALL
                .iter()
                .any(|kind| kind.rule_for(*target_kind).is_some()),
            "no command can target a {target_kind}"
        );
    }
    for kind in CommandKind::ALL {
        assert!(
            AggregateKind::ALL
                .iter()
                .any(|target| kind.rule_for(*target).is_some()),
            "{kind} can target nothing at all"
        );
    }
}

// ---------------------------------------------------------------------------
// OP-REQ-039 — a task never claims progress its evidence does not support.
//
// The incident these cases exist to make impossible: OP-01 read `in_progress`
// for thirteen hours while its only team run was `queued` with five unattached
// seats, one of which reported itself `running` the whole time because a turn
// was open on an unanswered permission prompt.
// ---------------------------------------------------------------------------

#[test]
fn a_task_cannot_claim_progress_without_an_attached_seat() {
    let error = apply_task_transition(TaskState::Ready, &TaskTransition::to(TaskState::InProgress))
        .expect_err("progress needs evidence, exactly like closure does");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));

    let evidence = certify_task_progress(RunLifecycle::Running, &[SeatAttachment::Attached])
        .expect("a dispatched run with an attached seat evidences progress");
    let dispatched = TaskTransition {
        progress: Some(&evidence),
        ..TaskTransition::to(TaskState::InProgress)
    };
    assert_eq!(
        apply_task_transition(TaskState::Ready, &dispatched).expect("evidenced progress"),
        TaskState::InProgress
    );
}

#[test]
fn the_exact_incident_shape_cannot_certify_progress() {
    // The state that was allowed to read `in_progress` for thirteen hours on
    // 2026-08-16: a queued team run whose five seats never attached. By then
    // every deadline had long passed.
    let abandoned = [SeatAttachment::AttachmentFailed; 5];
    let error = certify_task_progress(RunLifecycle::Queued, &abandoned)
        .expect_err("seats that never attached evidence nothing");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));
    assert!(
        certify_task_progress(RunLifecycle::Running, &abandoned).is_err(),
        "and a run calling itself running does not rescue them"
    );

    // Admission legitimately produces the *same shape* one second in: a queued
    // run whose seats have not reported yet. Refusing this would make every
    // launch illegal, so the deadline — not the lifecycle — is the guard.
    let starting = [SeatAttachment::Pending; 5];
    assert!(
        certify_task_progress(RunLifecycle::Queued, &starting).is_ok(),
        "a seat inside its attachment deadline is starting, not phantom"
    );

    // A closed run holds nothing, whatever its seats last looked like.
    for closed in [RunLifecycle::Succeeded, RunLifecycle::Cancelled] {
        assert!(
            certify_task_progress(closed, &[SeatAttachment::Attached]).is_err(),
            "{closed} is over"
        );
    }

    // A run whose seats are not materialized yet is the same transient moment
    // as a pending seat, so it is allowed — but a closed one still is not.
    assert!(certify_task_progress(RunLifecycle::Running, &[]).is_ok());
    assert!(
        certify_task_progress(RunLifecycle::Succeeded, &[]).is_err(),
        "a closed run holds nothing, seats or no seats"
    );

    // And a stalled or orphaned seat can never hold progress, deadline or not.
    for spoiled in [SeatAttachment::Stalled, SeatAttachment::Orphaned] {
        assert!(
            certify_task_progress(RunLifecycle::Running, &[spoiled]).is_err(),
            "{spoiled} must not hold a task in progress"
        );
    }
}

#[test]
fn a_queued_run_is_accepted_but_not_dispatched() {
    assert!(!RunLifecycle::Queued.is_dispatched());
    for dispatched in [
        RunLifecycle::Launching,
        RunLifecycle::Running,
        RunLifecycle::WaitingInput,
        RunLifecycle::Blocked,
    ] {
        assert!(dispatched.is_dispatched(), "{dispatched} is executing");
    }
    for terminal in [
        RunLifecycle::Succeeded,
        RunLifecycle::Failed,
        RunLifecycle::Cancelled,
        RunLifecycle::Parked,
    ] {
        assert!(!terminal.is_dispatched(), "{terminal} is over");
    }
}

#[test]
fn an_unattached_seat_becomes_a_finding_once_its_deadline_passes() {
    let never_attached = SeatAttachmentObservation {
        last_attached_at: None,
        last_activity_at: None,
        ..healthy_seat()
    };
    assert_eq!(
        conclude(&never_attached, "2026-08-16T07:59:00Z"),
        SeatAttachment::Pending,
        "before the deadline a silent seat is merely young"
    );
    assert_eq!(
        conclude(&never_attached, "2026-08-16T08:00:01Z"),
        SeatAttachment::AttachmentFailed,
        "after it, silence is a recorded finding rather than an open wait"
    );
}

#[test]
fn a_self_reported_running_runtime_does_not_prove_activity() {
    // The builder seat reported `running` for two and a half hours while parked
    // on a permission prompt. An open turn is a fact about a process, not about
    // the work, so only observed activity may conclude `Attached`.
    let parked_on_a_prompt = SeatAttachmentObservation {
        last_attached_at: Some(at("2026-08-16T07:53:00Z")),
        last_activity_at: Some(at("2026-08-16T07:53:00Z")),
        runtime_reported: ObservedRunState::Running,
        ..healthy_seat()
    };
    assert_eq!(
        conclude(&parked_on_a_prompt, "2026-08-16T10:26:00Z"),
        SeatAttachment::Stalled
    );
    assert!(SeatAttachment::Stalled.requires_human());
    assert!(!SeatAttachment::Stalled.is_executing());

    // And inside the bound the very same runtime report concludes healthily,
    // which proves the verdict came from the activity timestamp, not the label.
    assert_eq!(
        conclude(&parked_on_a_prompt, "2026-08-16T08:20:00Z"),
        SeatAttachment::Attached
    );
}

#[test]
fn an_attached_seat_that_never_showed_activity_is_stalled_not_healthy() {
    let silent = SeatAttachmentObservation {
        last_attached_at: Some(at("2026-08-16T07:53:00Z")),
        last_activity_at: None,
        ..healthy_seat()
    };
    assert_eq!(
        conclude(&silent, "2026-08-16T07:54:00Z"),
        SeatAttachment::Stalled,
        "unknown activity must not read as fresh"
    );
}

#[test]
fn an_orphan_is_an_orphan_however_healthy_its_runtime_looks() {
    let orphan = SeatAttachmentObservation {
        parent_closed: true,
        ..healthy_seat()
    };
    assert_eq!(
        conclude(&orphan, "2026-08-16T09:56:00Z"),
        SeatAttachment::Orphaned
    );
    assert!(SeatAttachment::Orphaned.is_excluded());
    assert!(!SeatAttachment::Orphaned.is_executing());
    assert!(
        certify_task_progress(RunLifecycle::Running, &[SeatAttachment::Orphaned]).is_err(),
        "an orphan cannot hold a task in progress"
    );

    // Release outranks even orphanhood: a reaped seat is simply gone.
    let released = SeatAttachmentObservation {
        released: true,
        ..orphan
    };
    assert_eq!(
        conclude(&released, "2026-08-16T09:56:00Z"),
        SeatAttachment::Released
    );
}

#[test]
fn a_healthy_seat_is_the_only_one_that_counts_as_executing() {
    assert_eq!(
        conclude(&healthy_seat(), "2026-08-16T09:56:00Z"),
        SeatAttachment::Attached
    );
    for state in SeatAttachment::ALL {
        assert_eq!(
            state.is_executing(),
            *state == SeatAttachment::Attached,
            "{state} must not be mistaken for executing work"
        );
    }
    for excluded in [
        SeatAttachment::Orphaned,
        SeatAttachment::AttachmentFailed,
        SeatAttachment::Released,
    ] {
        assert!(excluded.is_excluded(), "{excluded} is out of every count");
    }
}
