//! `planning_pair@1` (ASMA-8282): two immutable read-only members on distinct
//! actual vendors, one independent findings round, at most one clarification
//! round the caller asks for, and the caller's disposition with its dissent
//! retained. It has no Judge, quorum, verdict or settlement, and it cannot
//! satisfy a formal review gate.
//!
//! Every refusal below is asserted by its exact rule, so a test cannot pass on
//! an unrelated refusal the fixture happened to trigger.

use kontor_core::DomainError;
use kontor_core::consultation::{
    AdviceDisposition, ConsultationContextPolicy, ConsultationScope, MemoryAccess,
};
use kontor_core::id::{
    BoundedText, ContentHash, CurrencyCode, ExternalName, Money, RoleKey, SCHEMA_VERSION,
};
use kontor_core::planning_pair::{
    ClarificationRequest, ConsultationProtocol, FINDINGS_ROUNDS, MAX_CLARIFICATION_ROUNDS,
    MemberDisposition, PlanningPairActor, PlanningPairDisposition, PlanningPairMember,
    PlanningPairMemberSpec, PlanningPairMembers, PlanningPairRound, PlanningPairRun,
    PlanningPairSlot, PlanningPairSpec, PlanningPairState, rule, select_protocol,
};
use kontor_core::spec::{BudgetBounds, ModelRef, ModelRung, ProviderRef};

use PlanningPairActor::{Caller, Member};
use PlanningPairSlot::{SeatA, SeatB};

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

fn invalid(subject: &'static str, rule: &'static str) -> DomainError {
    DomainError::invalid(subject, rule)
}

fn unauthorized(subject: &'static str, rule: &'static str) -> DomainError {
    DomainError::MissingAuthority { subject, rule }
}

fn missing(rule: &'static str) -> DomainError {
    DomainError::MissingEvidence {
        subject: "PlanningPairRun",
        rule,
    }
}

fn run_refusal(rule: &'static str) -> DomainError {
    invalid("PlanningPairRun", rule)
}

fn member_spec(slot: PlanningPairSlot) -> PlanningPairMemberSpec {
    PlanningPairMemberSpec {
        slot,
        specialty: text("Independent planning perspective"),
        behavior: text("Read the frozen plan and give one finding; change nothing."),
        context: ConsultationContextPolicy {
            skills: Vec::new(),
            files: Vec::new(),
            memory: MemoryAccess::None,
        },
    }
}

fn spec() -> PlanningPairSpec {
    PlanningPairSpec {
        schema_version: SCHEMA_VERSION,
        protocol: ConsultationProtocol::PlanningPair,
        name: ExternalName::parse("Planning pair").expect("name"),
        charter: text("Is this plan the smallest sound next step?"),
        members: vec![member_spec(SeatA), member_spec(SeatB)],
        allowed_caller_roles: vec![RoleKey::parse("lead").expect("role")],
        allowed_scopes: vec![ConsultationScope::Epic, ConsultationScope::Ticket],
        budget: BudgetBounds {
            max_tokens: 200_000,
            max_commands: 40,
            max_duration_seconds: 1_800,
            max_cost: Money {
                minor_units: 5_000,
                currency: CurrencyCode::parse("NOK").expect("currency"),
            },
        },
    }
}

fn member(slot: PlanningPairSlot, account: &str, model: &str, vendor: &str) -> PlanningPairMember {
    PlanningPairMember {
        slot,
        binding_key: format!("advisor/planning-{}", slot.as_str()),
        route: ModelRung {
            provider: ProviderRef(account.to_owned()),
            model: ModelRef(model.to_owned()),
            effort: None,
        },
        vendor: vendor.to_owned(),
    }
}

fn placement() -> ContentHash {
    ContentHash::of(b"the shared allocator's placement receipt")
}

fn members() -> PlanningPairMembers {
    PlanningPairMembers::freeze(
        placement(),
        vec![
            member(SeatA, "claude-personal", "claude-opus-5", "anthropic"),
            member(SeatB, "codex-work", "gpt-5.6-sol", "openai"),
        ],
    )
    .expect("two members on distinct actual vendors")
}

fn run() -> PlanningPairRun {
    PlanningPairRun::admit(&spec(), members(), text("Should the migration land first?"))
        .expect("convenes")
}

/// A pair whose findings are both recorded, with their hashes.
fn with_findings() -> (PlanningPairRun, ContentHash, ContentHash) {
    let mut pair = run();
    let a = pair
        .record_finding(Member(SeatA), SeatA, text("Land the migration first."))
        .expect("seat A finding");
    let b = pair
        .record_finding(Member(SeatB), SeatB, text("Land the reader first."))
        .expect("seat B finding");
    (pair, a, b)
}

fn decide(
    a: (&ContentHash, Option<&ContentHash>, AdviceDisposition),
    b: (&ContentHash, Option<&ContentHash>, AdviceDisposition),
) -> PlanningPairDisposition {
    PlanningPairDisposition {
        members: vec![
            MemberDisposition {
                slot: SeatA,
                finding: a.0.clone(),
                answer: a.1.cloned(),
                disposition: a.2,
            },
            MemberDisposition {
                slot: SeatB,
                finding: b.0.clone(),
                answer: b.1.cloned(),
                disposition: b.2,
            },
        ],
        rationale: text("Seat A's ordering keeps the reader honest."),
    }
}

// ---------------------------------------------------------------------------
// The protocol and its document
// ---------------------------------------------------------------------------

#[test]
fn one_findings_round_one_clarification_then_the_callers_disposition() {
    assert_eq!((FINDINGS_ROUNDS, MAX_CLARIFICATION_ROUNDS), (1, 1));
    let mut pair = run();
    assert_eq!(pair.protocol(), ConsultationProtocol::PlanningPair);
    assert_eq!(
        pair.spec_hash(),
        spec().canonicalize().expect("canonical").hash()
    );
    assert_eq!(pair.state(), PlanningPairState::AwaitingFindings);
    let a = pair
        .record_finding(Member(SeatA), SeatA, text("Land the migration first."))
        .expect("seat A");
    assert!(pair.findings().is_none(), "one finding is not a round");
    let b = pair
        .record_finding(Member(SeatB), SeatB, text("Land the reader first."))
        .expect("seat B");
    assert_ne!(a, b);
    assert_eq!(pair.state(), PlanningPairState::FindingsReleased);
    let [first, second] = pair.findings().expect("released together");
    assert_eq!(
        (first.slot, first.round, first.advice.as_str()),
        (
            SeatA,
            PlanningPairRound::Findings,
            "Land the migration first."
        )
    );
    assert_eq!(
        (second.slot, second.document_hash.clone()),
        (SeatB, b.clone())
    );

    pair.request_clarification(
        Caller,
        ClarificationRequest {
            question: text("What breaks if the reader lands second?"),
            addressed: vec![SeatB],
        },
    )
    .expect("the one clarification");
    assert_eq!(pair.state(), PlanningPairState::AwaitingAnswers);
    assert!(pair.clarification().is_none());
    let answer = pair
        .record_answer(Member(SeatB), SeatB, text("Old rows would be misread."))
        .expect("seat B answers");
    assert_eq!(pair.state(), PlanningPairState::AnswersReleased);
    let (request, answers) = pair.clarification().expect("released");
    assert_eq!(request.addressed, [SeatB]);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].round, PlanningPairRound::Clarification);

    pair.record_disposition(
        Caller,
        decide(
            (&a, None, AdviceDisposition::Accepted),
            (&b, Some(&answer), AdviceDisposition::PartiallyAccepted),
        ),
    )
    .expect("the caller decides");
    assert_eq!(pair.state(), PlanningPairState::Disposed);
    assert_eq!(
        pair.members().member(SeatB).route.provider.0,
        "codex-work",
        "the members are the ones frozen at placement"
    );
    assert_eq!(pair.members().placement_hash(), &placement());
}

#[test]
fn a_planning_pair_has_no_judge_quorum_or_verdict_to_declare() {
    spec().validate().expect("the document validates");
    let mut json = serde_json::to_value(spec()).expect("JSON");
    // Every Committee knob is absent from the document, not accepted and
    // ignored: a planning pair cannot be told to behave like a Committee.
    for (field, value) in [
        ("judge", serde_json::json!({"slot": "judge"})),
        ("aggregation", serde_json::json!("conjunctive")),
        ("quorum", serde_json::json!(2)),
        ("diversity", serde_json::json!("none")),
        ("round_limit", serde_json::json!(2)),
        ("clarification_rounds", serde_json::json!(2)),
    ] {
        json[field] = value;
        assert!(
            serde_json::from_value::<PlanningPairSpec>(json.clone()).is_err(),
            "{field}"
        );
        json.as_object_mut().expect("object").remove(field);
    }
    serde_json::from_value::<PlanningPairSpec>(json).expect("the plain document parses");

    // Nor is it an alias: a document naming another protocol is refused.
    for protocol in [
        ConsultationProtocol::IndependentReview,
        ConsultationProtocol::Advisor,
    ] {
        let mut other = spec();
        other.protocol = protocol;
        assert_eq!(
            other.validate(),
            Err(invalid("PlanningPairSpec", rule::NOT_PLANNING_PAIR)),
            "{protocol}"
        );
    }
    assert_eq!(
        PlanningPairSlot::ALL
            .iter()
            .map(|slot| slot.label())
            .collect::<Vec<_>>(),
        ["SEAT A", "SEAT B"]
    );
}

// ---------------------------------------------------------------------------
// Same vendor
// ---------------------------------------------------------------------------

#[test]
fn members_on_one_actual_vendor_are_refused() {
    // Two accounts, two harnesses, two models — one maker. Distinctness is the
    // actual vendor, not the alias or the provider family.
    for (a, b) in [
        (
            member(SeatA, "claude-personal", "claude-opus-5", "anthropic"),
            member(SeatB, "claude-work", "claude-sonnet-5", "anthropic"),
        ),
        (
            member(SeatA, "cursor", "claude-sonnet-5", "anthropic"),
            member(SeatB, "claude-personal", "claude-opus-5", "anthropic"),
        ),
    ] {
        assert_eq!(
            PlanningPairMembers::freeze(placement(), vec![a, b]),
            Err(invalid("PlanningPairMembers", rule::SAME_VENDOR))
        );
    }
}

#[test]
fn a_member_without_a_known_vendor_is_refused() {
    for vendor in ["unknown", "", "  "] {
        assert_eq!(
            PlanningPairMembers::freeze(
                placement(),
                vec![
                    member(SeatA, "cursor", "auto", vendor),
                    member(SeatB, "codex-work", "gpt-5.6-sol", "openai"),
                ],
            ),
            Err(invalid("PlanningPairMembers", rule::VENDOR_UNKNOWN)),
            "{vendor:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Missing member
// ---------------------------------------------------------------------------

#[test]
fn a_pair_missing_a_member_is_refused() {
    let a = || member(SeatA, "claude-personal", "claude-opus-5", "anthropic");
    let b = || member(SeatB, "codex-work", "gpt-5.6-sol", "openai");
    let c = || member(SeatB, "cursor", "grok-4.6", "xai");
    for members in [
        vec![a()],
        vec![b()],
        vec![],
        vec![b(), a()],
        vec![a(), a()],
        vec![a(), b(), c()],
    ] {
        let slots: Vec<&str> = members.iter().map(|member| member.slot.as_str()).collect();
        assert_eq!(
            PlanningPairMembers::freeze(placement(), members),
            Err(invalid("PlanningPairMembers", rule::MEMBERS)),
            "{slots:?}"
        );
    }
    for members in [
        vec![member_spec(SeatA)],
        vec![member_spec(SeatB), member_spec(SeatA)],
        vec![member_spec(SeatA), member_spec(SeatB), member_spec(SeatB)],
    ] {
        let mut document = spec();
        document.members = members;
        assert_eq!(
            document.validate(),
            Err(invalid("PlanningPairSpec", rule::MEMBERS))
        );
    }
}

#[test]
fn a_findings_round_missing_a_member_releases_nothing() {
    let mut pair = run();
    let a = pair
        .record_finding(Member(SeatA), SeatA, text("Land the migration first."))
        .expect("seat A");
    assert!(pair.findings().is_none(), "seat A's finding stays sealed");
    assert!(pair.retained_dissent().is_empty());
    assert_eq!(pair.state(), PlanningPairState::AwaitingFindings);
    assert_eq!(
        pair.request_clarification(
            Caller,
            ClarificationRequest {
                question: text("Why?"),
                addressed: vec![SeatA],
            },
        ),
        Err(missing(rule::FINDINGS_INCOMPLETE))
    );
    assert_eq!(
        pair.record_disposition(
            Caller,
            decide(
                (&a, None, AdviceDisposition::Accepted),
                (&a, None, AdviceDisposition::Rejected),
            ),
        ),
        Err(missing(rule::FINDINGS_INCOMPLETE)),
        "a caller cannot decide on one member's advice alone"
    );
}

// ---------------------------------------------------------------------------
// Mutation attempt
// ---------------------------------------------------------------------------

#[test]
fn a_member_grant_cannot_name_an_authority() {
    // Read-only is unrepresentable rather than merely unused: a member slot
    // has no field that could grant a write, a command or a gate waiver.
    for (field, value) in [
        ("capabilities", serde_json::json!(["write"])),
        ("allowed_operations", serde_json::json!(["task.update"])),
        ("read_only", serde_json::json!(false)),
        ("gate_waiver", serde_json::json!(true)),
    ] {
        let mut json = serde_json::to_value(spec()).expect("JSON");
        json["members"][1][field] = value.clone();
        assert!(
            serde_json::from_value::<PlanningPairSpec>(json).is_err(),
            "member {field}"
        );
        let mut json = serde_json::to_value(spec()).expect("JSON");
        json["members"][1]["context"][field] = value;
        assert!(
            serde_json::from_value::<PlanningPairSpec>(json).is_err(),
            "context {field}"
        );
    }
}

#[test]
fn a_recorded_finding_or_answer_cannot_be_rewritten() {
    let (mut pair, a, _) = with_findings();
    assert_eq!(
        pair.record_finding(
            Member(SeatA),
            SeatA,
            text("On reflection, the reader first.")
        ),
        Err(run_refusal(rule::FINDING_IMMUTABLE))
    );
    assert_eq!(pair.findings().expect("released")[0].document_hash, a);
    assert_eq!(
        pair.findings().expect("released")[0].advice.as_str(),
        "Land the migration first."
    );
    pair.request_clarification(
        Caller,
        ClarificationRequest {
            question: text("What breaks?"),
            addressed: vec![SeatA, SeatB],
        },
    )
    .expect("asked");
    let answer = pair
        .record_answer(Member(SeatA), SeatA, text("Nothing, if it lands first."))
        .expect("answered");
    assert_eq!(
        pair.record_answer(Member(SeatA), SeatA, text("Everything.")),
        Err(run_refusal(rule::ANSWER_IMMUTABLE))
    );
    assert!(pair.clarification().is_none(), "seat B has not answered");
    pair.record_answer(Member(SeatB), SeatB, text("Old rows."))
        .expect("answered");
    let (_, answers) = pair.clarification().expect("released");
    assert_eq!(answers[0].document_hash, answer);
}

#[test]
fn a_member_cannot_speak_for_the_other_seat_or_the_caller() {
    let mut pair = run();
    for actor in [Member(SeatA), Caller] {
        assert_eq!(
            pair.record_finding(actor, SeatB, text("Seat B agrees.")),
            Err(unauthorized("PlanningPairRun", rule::MEMBER_ONLY)),
            "{actor:?}"
        );
    }
    assert!(pair.findings().is_none());
    let (mut pair, a, b) = with_findings();
    assert_eq!(
        pair.request_clarification(
            Member(SeatA),
            ClarificationRequest {
                question: text("Seat B, reconsider."),
                addressed: vec![SeatB],
            },
        ),
        Err(unauthorized("PlanningPairRun", rule::CALLER_ONLY))
    );
    assert_eq!(
        pair.record_disposition(
            Member(SeatB),
            decide(
                (&a, None, AdviceDisposition::Rejected),
                (&b, None, AdviceDisposition::Accepted),
            ),
        ),
        Err(unauthorized("PlanningPairRun", rule::CALLER_ONLY))
    );
    pair.request_clarification(
        Caller,
        ClarificationRequest {
            question: text("Seat B, what breaks?"),
            addressed: vec![SeatB],
        },
    )
    .expect("asked");
    assert_eq!(
        pair.record_answer(Member(SeatA), SeatB, text("Nothing.")),
        Err(unauthorized("PlanningPairRun", rule::MEMBER_ONLY))
    );
    assert_eq!(pair.state(), PlanningPairState::AwaitingAnswers);
}

#[test]
fn a_disposed_pair_is_immutable() {
    let (mut pair, a, b) = with_findings();
    let disposition = decide(
        (&a, None, AdviceDisposition::Accepted),
        (&b, None, AdviceDisposition::Rejected),
    );
    pair.record_disposition(Caller, disposition.clone())
        .expect("decided");
    let terminal = DomainError::Terminal {
        subject: "PlanningPairRun",
    };
    assert_eq!(
        pair.record_finding(Member(SeatB), SeatB, text("A second opinion.")),
        Err(terminal.clone())
    );
    assert_eq!(
        pair.request_clarification(
            Caller,
            ClarificationRequest {
                question: text("Late question."),
                addressed: vec![SeatA],
            },
        ),
        Err(terminal.clone())
    );
    assert_eq!(
        pair.record_answer(Member(SeatA), SeatA, text("Late answer.")),
        Err(terminal.clone())
    );
    assert_eq!(
        pair.record_disposition(
            Caller,
            decide(
                (&a, None, AdviceDisposition::Rejected),
                (&b, None, AdviceDisposition::Accepted),
            ),
        ),
        Err(terminal)
    );
    assert_eq!(pair.disposition(), Some(&disposition));
}

// ---------------------------------------------------------------------------
// Extra clarification
// ---------------------------------------------------------------------------

#[test]
fn a_second_clarification_round_is_refused() {
    let (mut pair, _, _) = with_findings();
    let ask = |addressed: Vec<PlanningPairSlot>| ClarificationRequest {
        question: text("Say more."),
        addressed,
    };
    pair.request_clarification(Caller, ask(vec![SeatA]))
        .expect("the one clarification");
    assert_eq!(
        pair.request_clarification(Caller, ask(vec![SeatB])),
        Err(run_refusal(rule::EXTRA_CLARIFICATION)),
        "not while the first is open"
    );
    pair.record_answer(Member(SeatA), SeatA, text("More."))
        .expect("answered");
    assert_eq!(
        pair.request_clarification(Caller, ask(vec![SeatA, SeatB])),
        Err(run_refusal(rule::EXTRA_CLARIFICATION)),
        "not after it was answered"
    );
    assert_eq!(pair.state(), PlanningPairState::AnswersReleased);
}

#[test]
fn only_an_addressed_member_answers_and_only_when_the_caller_asked() {
    let (mut pair, a, b) = with_findings();
    assert_eq!(
        pair.record_answer(Member(SeatA), SeatA, text("Unprompted.")),
        Err(run_refusal(rule::NO_CLARIFICATION)),
        "a member cannot open a clarification round"
    );
    for addressed in [vec![], vec![SeatB, SeatB]] {
        assert_eq!(
            pair.request_clarification(
                Caller,
                ClarificationRequest {
                    question: text("Say more."),
                    addressed,
                },
            ),
            Err(run_refusal(rule::ADDRESSEES))
        );
    }
    pair.request_clarification(
        Caller,
        ClarificationRequest {
            question: text("Seat B, say more."),
            addressed: vec![SeatB],
        },
    )
    .expect("asked");
    assert_eq!(
        pair.record_answer(Member(SeatA), SeatA, text("Me too.")),
        Err(run_refusal(rule::NOT_ADDRESSED))
    );
    assert_eq!(
        pair.record_disposition(
            Caller,
            decide(
                (&a, None, AdviceDisposition::Accepted),
                (&b, None, AdviceDisposition::Rejected),
            ),
        ),
        Err(missing(rule::ANSWERS_INCOMPLETE))
    );
}

// ---------------------------------------------------------------------------
// Advice presented as a gate
// ---------------------------------------------------------------------------

#[test]
fn advice_cannot_satisfy_a_formal_review_gate() {
    for protocol in [
        ConsultationProtocol::Advisor,
        ConsultationProtocol::PlanningPair,
    ] {
        assert!(!protocol.is_formal_review());
        assert_eq!(
            protocol.require_formal_review(),
            Err(unauthorized("FormalReviewGate", rule::NOT_FORMAL)),
            "{protocol}"
        );
    }
    ConsultationProtocol::IndependentReview
        .require_formal_review()
        .expect("the formal protocol is admitted");

    // Unanimous advice is still advice.
    let (mut pair, a, b) = with_findings();
    pair.record_disposition(
        Caller,
        decide(
            (&a, None, AdviceDisposition::Accepted),
            (&b, None, AdviceDisposition::Accepted),
        ),
    )
    .expect("decided");
    assert_eq!(
        pair.protocol().require_formal_review(),
        Err(unauthorized("FormalReviewGate", rule::NOT_FORMAL))
    );
}

#[test]
fn a_disposition_cannot_carry_a_verdict() {
    let (_, a, b) = with_findings();
    let json = serde_json::to_value(decide(
        (&a, None, AdviceDisposition::Accepted),
        (&b, None, AdviceDisposition::Rejected),
    ))
    .expect("JSON");
    serde_json::from_value::<PlanningPairDisposition>(json.clone()).expect("a plain decision");
    for (field, value) in [
        ("verdict", serde_json::json!("compliant")),
        ("gate", serde_json::json!("formal_review")),
        ("aggregate", serde_json::json!("pass")),
        ("settled", serde_json::json!(true)),
    ] {
        let mut shaped = json.clone();
        shaped[field] = value;
        assert!(
            serde_json::from_value::<PlanningPairDisposition>(shaped).is_err(),
            "{field}"
        );
    }
}

#[test]
fn protocol_selection_is_explicit_and_never_substituted() {
    use ConsultationProtocol::{Advisor, IndependentReview, PlanningPair};
    assert_eq!(
        select_protocol(None, ConsultationProtocol::ALL),
        Err(invalid("ConsultationProtocol", rule::NO_PROTOCOL))
    );
    for (requested, available) in [
        (IndependentReview, vec![Advisor, PlanningPair]),
        (PlanningPair, vec![Advisor, IndependentReview]),
        (Advisor, vec![PlanningPair, IndependentReview]),
        (PlanningPair, vec![]),
    ] {
        assert_eq!(
            select_protocol(Some(requested), &available),
            Err(invalid("ConsultationProtocol", rule::UNAVAILABLE)),
            "{requested}"
        );
    }
    for requested in ConsultationProtocol::ALL {
        assert_eq!(
            select_protocol(Some(*requested), ConsultationProtocol::ALL),
            Ok(*requested)
        );
        assert_eq!(
            ConsultationProtocol::parse(requested.as_str()),
            Ok(*requested)
        );
    }
    assert_eq!(PlanningPair.as_str(), "planning_pair@1");
}

// ---------------------------------------------------------------------------
// Lost dissent
// ---------------------------------------------------------------------------

#[test]
fn a_disposition_that_omits_or_reorders_a_member_is_refused() {
    let (mut pair, a, b) = with_findings();
    let full = decide(
        (&a, None, AdviceDisposition::Accepted),
        (&b, None, AdviceDisposition::Rejected),
    );
    let mut only_a = full.clone();
    only_a.members.truncate(1);
    let mut reversed = full.clone();
    reversed.members.reverse();
    let mut twice_a = full.clone();
    twice_a.members[1] = twice_a.members[0].clone();
    let mut none = full.clone();
    none.members.clear();
    for disposition in [only_a, reversed, twice_a, none] {
        assert_eq!(
            pair.record_disposition(Caller, disposition),
            Err(run_refusal(rule::DISSENT_LOST))
        );
    }
    assert_eq!(pair.state(), PlanningPairState::FindingsReleased);
    pair.record_disposition(Caller, full)
        .expect("the full decision");
}

#[test]
fn a_disposition_citing_a_rewritten_finding_or_answer_is_refused() {
    let (mut pair, a, b) = with_findings();
    pair.request_clarification(
        Caller,
        ClarificationRequest {
            question: text("Seat B, what breaks?"),
            addressed: vec![SeatB],
        },
    )
    .expect("asked");
    let answer = pair
        .record_answer(Member(SeatB), SeatB, text("Old rows would be misread."))
        .expect("answered");
    let softened = ContentHash::of(b"a gentler seat B");
    for disposition in [
        // Seat B's finding, quietly replaced.
        decide(
            (&a, None, AdviceDisposition::Accepted),
            (&softened, Some(&answer), AdviceDisposition::Rejected),
        ),
        // Seat B's answer, dropped.
        decide(
            (&a, None, AdviceDisposition::Accepted),
            (&b, None, AdviceDisposition::Rejected),
        ),
        // An answer seat A never gave.
        decide(
            (&a, Some(&answer), AdviceDisposition::Accepted),
            (&b, Some(&answer), AdviceDisposition::Rejected),
        ),
        // Seat A's finding cited for seat B.
        decide(
            (&a, None, AdviceDisposition::Accepted),
            (&a, Some(&answer), AdviceDisposition::Rejected),
        ),
    ] {
        assert_eq!(
            pair.record_disposition(Caller, disposition),
            Err(run_refusal(rule::DISSENT_LOST))
        );
    }
    assert_eq!(pair.state(), PlanningPairState::AnswersReleased);
}

#[test]
fn dissent_survives_the_callers_decision() {
    let (mut pair, a, b) = with_findings();
    pair.request_clarification(
        Caller,
        ClarificationRequest {
            question: text("Seat B, what breaks?"),
            addressed: vec![SeatB],
        },
    )
    .expect("asked");
    let answer = pair
        .record_answer(Member(SeatB), SeatB, text("Old rows would be misread."))
        .expect("answered");
    assert_eq!(
        pair.record_disposition(
            Caller,
            decide(
                (&a, None, AdviceDisposition::Accepted),
                (&b, Some(&answer), AdviceDisposition::Superseded),
            ),
        ),
        Err(run_refusal(rule::FIRST_DISPOSITION))
    );
    pair.record_disposition(
        Caller,
        decide(
            (&a, None, AdviceDisposition::Accepted),
            (&b, Some(&answer), AdviceDisposition::Rejected),
        ),
    )
    .expect("the caller sides with seat A");
    let dissent: Vec<(PlanningPairSlot, PlanningPairRound, &str)> = pair
        .retained_dissent()
        .into_iter()
        .map(|contribution| {
            (
                contribution.slot,
                contribution.round,
                contribution.advice.as_str(),
            )
        })
        .collect();
    assert_eq!(
        dissent,
        [
            (SeatB, PlanningPairRound::Findings, "Land the reader first."),
            (
                SeatB,
                PlanningPairRound::Clarification,
                "Old rows would be misread."
            ),
        ]
    );
    let [kept_a, kept_b] = pair.findings().expect("both findings stay on the record");
    assert_eq!((&kept_a.document_hash, &kept_b.document_hash), (&a, &b));
    let (_, answers) = pair.clarification().expect("the answer stays too");
    assert_eq!(answers[0].document_hash, answer);
}
