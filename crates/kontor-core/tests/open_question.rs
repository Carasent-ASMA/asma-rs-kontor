//! Open-question ledger: disposition truth table, corrections, authority and
//! the report-only detector boundary.
//!
//! The mutants this suite exists to kill:
//!
//! * closing a question without a disposition, or inventing a fourth way to
//!   close one;
//! * deferring without naming a concrete trigger that could reopen it;
//! * reopening on the wrong trigger, or ignoring the right one;
//! * correcting a round or a disposition by rewriting the predecessor instead
//!   of appending a successor;
//! * checking authority when a seat *raises* a question, or skipping it when a
//!   seat *closes* one;
//! * letting the detector resolve, reopen or otherwise touch a question;
//! * returning findings whose order depends on the order of the observations.

use kontor_core::DomainError;
use kontor_core::id::{
    BoundedText, ContentHash, MiniProjectId, OpenQuestionId, ProjectId, RoleKey, SeatBindingId,
    TaskId, Timestamp, TriggerKey, parse_utc_timestamp,
};
use kontor_core::open_question::{
    AcceptedDecision, CloserPolicy, DecisionCitation, DetectorObservations, DispositionKind,
    DispositionOutcome, OpenQuestion, OpenQuestionAttachment, OpenQuestionFinding,
    OpenQuestionStatus, QuestionScope, ReopeningTrigger, detect,
};
use kontor_core::receipt::AggregateRef;
use kontor_core::spec::{ShareabilityClass, ShareabilityProvenance};

fn now() -> Timestamp {
    parse_utc_timestamp("2026-08-19T10:00:00Z").expect("canonical timestamp")
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

fn architecture_closer() -> RoleKey {
    RoleKey::parse("lead-software-architect").expect("role key")
}

fn process_closer() -> RoleKey {
    RoleKey::parse("technical-program-manager").expect("role key")
}

fn policy() -> CloserPolicy {
    CloserPolicy {
        architecture_closer: architecture_closer(),
        process_closer: process_closer(),
    }
}

fn attachment() -> OpenQuestionAttachment {
    OpenQuestionAttachment::Record(AggregateRef::Task {
        task_id: TaskId::generate(),
    })
}

fn raise_with(scope: QuestionScope) -> OpenQuestion {
    OpenQuestion::raise(
        OpenQuestionId::generate(),
        ProjectId::generate(),
        MiniProjectId::generate(),
        text("whether the mirror is authoritative"),
        scope,
        attachment(),
        SeatBindingId::generate(),
        text("two documents disagree and neither cites the other"),
        vec![
            text("treat the mirror as authoritative"),
            text("refuse the read"),
        ],
        now(),
    )
    .expect("a valid question is raised")
}

fn raise() -> OpenQuestion {
    raise_with(QuestionScope::Architecture)
}

fn trigger(key: &str) -> ReopeningTrigger {
    ReopeningTrigger {
        key: TriggerKey::parse(key).expect("trigger key"),
        condition: text("the canonical mirror ships to production"),
    }
}

fn citation(revision: &str) -> DecisionCitation {
    DecisionCitation {
        record: AggregateRef::Task {
            task_id: TaskId::generate(),
        },
        revision: ContentHash::parse(revision).expect("content hash"),
    }
}

fn hash(byte: u8) -> String {
    std::iter::repeat_n(format!("{byte:02x}"), 32).collect()
}

// ---------------------------------------------------------------------------
// The disposition truth table
// ---------------------------------------------------------------------------

#[test]
fn an_undispositioned_question_is_open_and_blocks_completion() {
    let question = raise();
    assert_eq!(question.status(), OpenQuestionStatus::Open);
    assert!(
        question.status().blocks_completion(),
        "an open question is not a valid end state"
    );
}

#[test]
fn each_disposition_releases_the_gate_and_reports_its_own_status() {
    let cases = [
        (
            DispositionOutcome::Resolved(citation(&hash(0xab))),
            OpenQuestionStatus::Resolved,
            DispositionKind::Resolved,
        ),
        (
            DispositionOutcome::Deferred(trigger("canonical-mirror-shipped")),
            OpenQuestionStatus::Deferred,
            DispositionKind::Deferred,
        ),
        (
            DispositionOutcome::NotRelevant(text("the surface was withdrawn")),
            OpenQuestionStatus::NotRelevant,
            DispositionKind::NotRelevant,
        ),
    ];
    for (outcome, expected_status, expected_kind) in cases {
        let mut question = raise();
        assert_eq!(outcome.kind(), expected_kind);
        question
            .dispose(
                SeatBindingId::generate(),
                &architecture_closer(),
                &policy(),
                outcome,
                None,
                now(),
            )
            .expect("the configured closer may close");
        assert_eq!(question.status(), expected_status);
        assert!(
            !question.status().blocks_completion(),
            "a dispositioned question releases the completion gate"
        );
    }
}

#[test]
fn there_are_exactly_three_dispositions() {
    assert_eq!(DispositionKind::ALL.len(), 3);
    for kind in DispositionKind::ALL {
        assert_eq!(
            DispositionKind::parse(kind.as_str()).expect("round trips"),
            *kind
        );
    }
    for rejected in ["closed", "wontfix", "done", "resolved_ish", ""] {
        assert!(
            DispositionKind::parse(rejected).is_err(),
            "`{rejected}` must not be a way to close a question"
        );
    }
}

#[test]
fn a_disposition_outcome_survives_a_json_round_trip() {
    let outcomes = [
        DispositionOutcome::Resolved(citation(&hash(0x11))),
        DispositionOutcome::Deferred(trigger("mirror-shipped")),
        DispositionOutcome::NotRelevant(text("withdrawn")),
    ];
    for outcome in outcomes {
        let json = serde_json::to_string(&outcome).expect("serializes");
        let decoded: DispositionOutcome = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(decoded, outcome);
        assert_eq!(decoded.kind(), outcome.kind());
    }
}

#[test]
fn deferring_requires_a_concrete_non_empty_trigger() {
    let mut question = raise();
    let empty = ReopeningTrigger {
        key: TriggerKey::parse("mirror-shipped").expect("trigger key"),
        condition: text("   "),
    };
    let refusal = question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Deferred(empty),
            None,
            now(),
        )
        .expect_err("a deferral with no condition is an abandoned question");
    assert!(matches!(refusal, DomainError::Invalid { .. }));
    assert_eq!(
        question.status(),
        OpenQuestionStatus::Open,
        "a refused disposition leaves the question open"
    );
}

#[test]
fn not_relevant_requires_a_reason() {
    let mut question = raise();
    assert!(
        question
            .dispose(
                SeatBindingId::generate(),
                &architecture_closer(),
                &policy(),
                DispositionOutcome::NotRelevant(text("  ")),
                None,
                now(),
            )
            .is_err(),
        "`not_relevant` without a reason says nothing"
    );
}

// ---------------------------------------------------------------------------
// Reopening
// ---------------------------------------------------------------------------

fn deferred_question(key: &str) -> OpenQuestion {
    let mut question = raise();
    question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Deferred(trigger(key)),
            None,
            now(),
        )
        .expect("deferral is recorded");
    question
}

#[test]
fn the_matching_trigger_reopens_without_erasing_the_deferral() {
    let mut question = deferred_question("canonical-mirror-shipped");
    let deferral_bytes = serde_json::to_string(&question.dispositions).expect("serializes");

    question
        .fire_trigger(
            &TriggerKey::parse("canonical-mirror-shipped").expect("trigger key"),
            SeatBindingId::generate(),
            now(),
        )
        .expect("the exact trigger reopens");

    assert_eq!(question.status(), OpenQuestionStatus::Reopened);
    assert!(
        question.status().blocks_completion(),
        "a reopened question blocks completion again"
    );
    assert_eq!(
        serde_json::to_string(&question.dispositions).expect("serializes"),
        deferral_bytes,
        "reopening neither deletes nor rewrites the deferred disposition"
    );
    assert_eq!(question.firings.len(), 1);
}

#[test]
fn a_mismatched_trigger_is_refused() {
    let mut question = deferred_question("canonical-mirror-shipped");
    let refusal = question
        .fire_trigger(
            &TriggerKey::parse("something-else-entirely").expect("trigger key"),
            SeatBindingId::generate(),
            now(),
        )
        .expect_err("only the trigger the deferral named reopens it");
    assert!(matches!(refusal, DomainError::Invalid { .. }));
    assert_eq!(question.status(), OpenQuestionStatus::Deferred);
    assert!(question.firings.is_empty());
}

#[test]
fn a_trigger_cannot_fire_twice_against_one_deferral() {
    let mut question = deferred_question("mirror-shipped");
    let key = TriggerKey::parse("mirror-shipped").expect("trigger key");
    question
        .fire_trigger(&key, SeatBindingId::generate(), now())
        .expect("first firing");
    assert!(
        question
            .fire_trigger(&key, SeatBindingId::generate(), now())
            .is_err(),
        "one deferral reopens once"
    );
    assert_eq!(question.firings.len(), 1);
}

#[test]
fn a_resolved_or_undispositioned_question_has_no_trigger_to_fire() {
    let key = TriggerKey::parse("mirror-shipped").expect("trigger key");

    let mut open = raise();
    assert!(
        open.fire_trigger(&key, SeatBindingId::generate(), now())
            .is_err(),
        "an undispositioned question has no deferral"
    );

    let mut resolved = raise();
    resolved
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Resolved(citation(&hash(0x22))),
            None,
            now(),
        )
        .expect("resolved");
    assert!(
        resolved
            .fire_trigger(&key, SeatBindingId::generate(), now())
            .is_err(),
        "only a deferred question reopens on a trigger"
    );
}

#[test]
fn a_reopened_question_can_be_dispositioned_again() {
    let mut question = deferred_question("mirror-shipped");
    question
        .fire_trigger(
            &TriggerKey::parse("mirror-shipped").expect("trigger key"),
            SeatBindingId::generate(),
            now(),
        )
        .expect("reopened");
    question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Resolved(citation(&hash(0x33))),
            Some(1),
            now(),
        )
        .expect("the reopened question is closed for good");
    assert_eq!(question.status(), OpenQuestionStatus::Resolved);
    assert!(!question.status().blocks_completion());
}

// ---------------------------------------------------------------------------
// Corrections append; they never rewrite
// ---------------------------------------------------------------------------

#[test]
fn a_round_correction_appends_and_leaves_the_predecessor_byte_identical() {
    let mut question = raise();
    let before = serde_json::to_string(&question.rounds[0]).expect("serializes");

    let ordinal = question
        .append_round(
            SeatBindingId::generate(),
            text("the earlier reading missed the tenant column"),
            vec![text("scope the read by project")],
            Some(1),
            now(),
        )
        .expect("a correction appends");

    assert_eq!(ordinal, 2);
    assert_eq!(question.rounds.len(), 2);
    assert_eq!(
        serde_json::to_string(&question.rounds[0]).expect("serializes"),
        before,
        "the superseded round keeps its exact bytes"
    );
    assert_eq!(question.rounds[1].supersedes, Some(1));
}

#[test]
fn a_disposition_correction_appends_and_supersedes_by_name() {
    let mut question = raise();
    question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::NotRelevant(text("thought the surface was gone")),
            None,
            now(),
        )
        .expect("first disposition");
    let before = serde_json::to_string(&question.dispositions[0]).expect("serializes");

    question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Resolved(citation(&hash(0x44))),
            Some(1),
            now(),
        )
        .expect("the correction appends");

    assert_eq!(question.dispositions.len(), 2);
    assert_eq!(
        serde_json::to_string(&question.dispositions[0]).expect("serializes"),
        before,
        "the superseded disposition keeps its exact bytes"
    );
    assert_eq!(question.dispositions[1].supersedes, Some(1));
    assert_eq!(
        question.status(),
        OpenQuestionStatus::Resolved,
        "the latest disposition is the current one"
    );
}

#[test]
fn a_correction_must_name_a_predecessor_that_exists() {
    let mut question = raise();
    assert!(
        question
            .append_round(
                SeatBindingId::generate(),
                text("why"),
                vec![text("option")],
                Some(9),
                now(),
            )
            .is_err(),
        "a round correction cannot point at nothing"
    );
    assert!(
        question
            .dispose(
                SeatBindingId::generate(),
                &architecture_closer(),
                &policy(),
                DispositionOutcome::NotRelevant(text("reason")),
                Some(9),
                now(),
            )
            .is_err(),
        "a disposition correction cannot point at nothing"
    );
}

// ---------------------------------------------------------------------------
// Raising is unprivileged; closing is not
// ---------------------------------------------------------------------------

#[test]
fn any_seat_may_raise_a_question() {
    for _ in 0..4 {
        let question = OpenQuestion::raise(
            OpenQuestionId::generate(),
            ProjectId::generate(),
            MiniProjectId::generate(),
            text("whether this assumption holds"),
            QuestionScope::Routing,
            attachment(),
            SeatBindingId::generate(),
            text("nothing states which service owns the write"),
            vec![text("assume the caller owns it")],
            now(),
        );
        assert!(
            question.is_ok(),
            "raising is not gated: the seat that trips over the ambiguity reports it"
        );
    }
}

#[test]
fn a_round_must_record_why_and_at_least_one_option() {
    assert!(
        OpenQuestion::raise(
            OpenQuestionId::generate(),
            ProjectId::generate(),
            MiniProjectId::generate(),
            text("subject"),
            QuestionScope::Product,
            attachment(),
            SeatBindingId::generate(),
            text("   "),
            vec![text("an option")],
            now(),
        )
        .is_err(),
        "a round must say why the state is ambiguous"
    );
    assert!(
        OpenQuestion::raise(
            OpenQuestionId::generate(),
            ProjectId::generate(),
            MiniProjectId::generate(),
            text("subject"),
            QuestionScope::Product,
            attachment(),
            SeatBindingId::generate(),
            text("why"),
            Vec::new(),
            now(),
        )
        .is_err(),
        "a round must record the options that were seen"
    );
}

#[test]
fn the_configured_closer_split_governs_disposition() {
    let architecture_scopes = [QuestionScope::Architecture, QuestionScope::Product];
    let process_scopes = [QuestionScope::Process, QuestionScope::Routing];

    for scope in architecture_scopes {
        let mut wrong = raise_with(scope);
        let refusal = wrong
            .dispose(
                SeatBindingId::generate(),
                &process_closer(),
                &policy(),
                DispositionOutcome::NotRelevant(text("reason")),
                None,
                now(),
            )
            .expect_err("the process closer may not close an architecture/product question");
        assert!(matches!(refusal, DomainError::MissingAuthority { .. }));
        assert_eq!(wrong.status(), OpenQuestionStatus::Open);

        let mut right = raise_with(scope);
        right
            .dispose(
                SeatBindingId::generate(),
                &architecture_closer(),
                &policy(),
                DispositionOutcome::NotRelevant(text("reason")),
                None,
                now(),
            )
            .expect("the architecture closer may");
    }

    for scope in process_scopes {
        let mut wrong = raise_with(scope);
        assert!(
            wrong
                .dispose(
                    SeatBindingId::generate(),
                    &architecture_closer(),
                    &policy(),
                    DispositionOutcome::NotRelevant(text("reason")),
                    None,
                    now(),
                )
                .is_err(),
            "the architecture closer may not close a process/routing question"
        );

        let mut right = raise_with(scope);
        right
            .dispose(
                SeatBindingId::generate(),
                &process_closer(),
                &policy(),
                DispositionOutcome::NotRelevant(text("reason")),
                None,
                now(),
            )
            .expect("the process closer may");
    }
}

#[test]
fn the_closer_split_never_names_a_role_code() {
    // The policy is data: renaming both closers changes who may close and
    // nothing else. A core that branched on a literal role code would keep
    // authorizing the old spelling.
    let renamed = CloserPolicy {
        architecture_closer: RoleKey::parse("chief-boundary-officer").expect("role key"),
        process_closer: RoleKey::parse("delivery-conductor").expect("role key"),
    };
    let mut question = raise_with(QuestionScope::Architecture);
    assert!(
        question
            .dispose(
                SeatBindingId::generate(),
                &architecture_closer(),
                &renamed,
                DispositionOutcome::NotRelevant(text("reason")),
                None,
                now(),
            )
            .is_err(),
        "the previous closer has no standing under a new policy"
    );
    question
        .dispose(
            SeatBindingId::generate(),
            &renamed.architecture_closer,
            &renamed,
            DispositionOutcome::NotRelevant(text("reason")),
            None,
            now(),
        )
        .expect("the configured closer may, whatever it is called");
}

// ---------------------------------------------------------------------------
// Shareability
// ---------------------------------------------------------------------------

#[test]
fn a_question_is_project_shared_by_the_tier_default() {
    let question = raise();
    assert_eq!(
        question.shareability.class,
        ShareabilityClass::ProjectShared,
        "an open question is project knowledge"
    );
    assert_eq!(
        question.shareability.provenance,
        ShareabilityProvenance::TypeDefault,
        "no human was consulted to get the default"
    );
}

#[test]
fn nothing_reclassifies_a_question() {
    let mut question = raise();
    let stamped = question.shareability.clone();
    question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Resolved(citation(&hash(0x55))),
            None,
            now(),
        )
        .expect("resolved");
    question
        .append_round(
            SeatBindingId::generate(),
            text("a later reading"),
            vec![text("an option")],
            None,
            now(),
        )
        .expect("appended");
    assert_eq!(
        question.shareability, stamped,
        "the write-time stamp is immutable across the whole history"
    );
}

// ---------------------------------------------------------------------------
// The detector reports and changes nothing
// ---------------------------------------------------------------------------

#[test]
fn contradicting_accepted_decisions_are_reported() {
    let subject = text("who owns the journal write");
    let decisions = vec![
        AcceptedDecision {
            subject: subject.clone(),
            record: AggregateRef::Task {
                task_id: TaskId::generate(),
            },
            revision: ContentHash::parse(&hash(0x01)).expect("hash"),
            superseded: false,
        },
        AcceptedDecision {
            subject: subject.clone(),
            record: AggregateRef::Task {
                task_id: TaskId::generate(),
            },
            revision: ContentHash::parse(&hash(0x02)).expect("hash"),
            superseded: false,
        },
    ];
    let findings = detect(&DetectorObservations {
        questions: &[],
        decisions: &decisions,
        fired_triggers: &[],
    });
    assert_eq!(findings.len(), 1);
    match &findings[0] {
        OpenQuestionFinding::ContradictingDecisions {
            subject: reported,
            revisions,
        } => {
            assert_eq!(reported, &subject);
            assert_eq!(revisions.len(), 2, "both accepted revisions are named");
        }
        other => panic!("expected a contradiction finding, got {other:?}"),
    }
}

#[test]
fn a_superseded_decision_is_not_a_contradiction() {
    let subject = text("who owns the journal write");
    let decisions = vec![
        AcceptedDecision {
            subject: subject.clone(),
            record: AggregateRef::Task {
                task_id: TaskId::generate(),
            },
            revision: ContentHash::parse(&hash(0x01)).expect("hash"),
            superseded: true,
        },
        AcceptedDecision {
            subject,
            record: AggregateRef::Task {
                task_id: TaskId::generate(),
            },
            revision: ContentHash::parse(&hash(0x02)).expect("hash"),
            superseded: false,
        },
    ];
    assert!(
        detect(&DetectorObservations {
            questions: &[],
            decisions: &decisions,
            fired_triggers: &[],
        })
        .is_empty(),
        "being replaced is the opposite of a contradiction"
    );
}

#[test]
fn a_citation_of_a_superseded_revision_is_reported() {
    let cited = citation(&hash(0x77));
    let mut question = raise();
    question
        .dispose(
            SeatBindingId::generate(),
            &architecture_closer(),
            &policy(),
            DispositionOutcome::Resolved(cited.clone()),
            None,
            now(),
        )
        .expect("resolved");

    let decisions = vec![AcceptedDecision {
        subject: text("who owns the journal write"),
        record: cited.record,
        revision: cited.revision.clone(),
        superseded: true,
    }];
    let questions = vec![question];
    let findings = detect(&DetectorObservations {
        questions: &questions,
        decisions: &decisions,
        fired_triggers: &[],
    });
    assert!(matches!(
        findings.as_slice(),
        [OpenQuestionFinding::SupersededCitation { .. }]
    ));
}

#[test]
fn a_fired_trigger_on_a_deferred_question_is_reported_not_applied() {
    let question = deferred_question("canonical-mirror-shipped");
    let before = serde_json::to_string(&question).expect("serializes");
    let questions = vec![question];
    let fired = vec![TriggerKey::parse("canonical-mirror-shipped").expect("trigger key")];

    let findings = detect(&DetectorObservations {
        questions: &questions,
        decisions: &[],
        fired_triggers: &fired,
    });

    assert!(matches!(
        findings.as_slice(),
        [OpenQuestionFinding::DeferredTriggerFired { .. }]
    ));
    assert_eq!(
        serde_json::to_string(&questions[0]).expect("serializes"),
        before,
        "detection is byte-identical on its input: it reports, it does not reopen"
    );
    assert_eq!(
        questions[0].status(),
        OpenQuestionStatus::Deferred,
        "only the explicit authorized command reopens a question"
    );
}

#[test]
fn an_unfired_trigger_reports_nothing() {
    let questions = vec![deferred_question("canonical-mirror-shipped")];
    assert!(
        detect(&DetectorObservations {
            questions: &questions,
            decisions: &[],
            fired_triggers: &[TriggerKey::parse("a-different-trigger").expect("trigger key")],
        })
        .is_empty()
    );
}

#[test]
fn an_already_reopened_question_is_not_reported_again() {
    let mut question = deferred_question("mirror-shipped");
    let key = TriggerKey::parse("mirror-shipped").expect("trigger key");
    question
        .fire_trigger(&key, SeatBindingId::generate(), now())
        .expect("reopened");
    let questions = vec![question];
    assert!(
        detect(&DetectorObservations {
            questions: &questions,
            decisions: &[],
            fired_triggers: &[key],
        })
        .is_empty(),
        "the firing is already durable; the status already says reopened"
    );
}

#[test]
fn finding_order_does_not_depend_on_observation_order() {
    let subject_a = text("aaa which service owns the write");
    let subject_b = text("bbb which queue carries the event");
    let mut decisions = Vec::new();
    for (subject, first, second) in [(&subject_a, 0x10, 0x11), (&subject_b, 0x20, 0x21)] {
        for revision in [first, second] {
            decisions.push(AcceptedDecision {
                subject: subject.clone(),
                record: AggregateRef::Task {
                    task_id: TaskId::generate(),
                },
                revision: ContentHash::parse(&hash(revision)).expect("hash"),
                superseded: false,
            });
        }
    }
    let questions = vec![
        deferred_question("mirror-shipped"),
        deferred_question("mirror-shipped"),
    ];
    let fired = vec![TriggerKey::parse("mirror-shipped").expect("trigger key")];

    let forward = detect(&DetectorObservations {
        questions: &questions,
        decisions: &decisions,
        fired_triggers: &fired,
    });

    let mut shuffled_decisions = decisions.clone();
    shuffled_decisions.reverse();
    let mut shuffled_questions = questions.clone();
    shuffled_questions.reverse();
    let reversed = detect(&DetectorObservations {
        questions: &shuffled_questions,
        decisions: &shuffled_decisions,
        fired_triggers: &fired,
    });

    assert_eq!(
        forward, reversed,
        "the finding order is stable, not an artifact of input order"
    );
    assert_eq!(
        forward.len(),
        4,
        "two contradictions and two fired triggers"
    );
}
