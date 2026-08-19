//! Advisor profile and Committee template validation.
//!
//! These cover the OP-05 proofs that live at the specification layer: a
//! template cannot be published unless it could produce an independent
//! conjunction, cardinality is data rather than a hard-coded three, and the
//! conjunctive rule counts a missing finding and incomplete evidence the way
//! the architecture says it must.

use kontor_core::consultation::{
    AdviceDisposition, AdvisorProfileSpec, AggregationProtocol, CommitteeRole, CommitteeSlotSpec,
    CommitteeTemplateSpec, CommitteeVerdict, ConsultationContextPolicy, ConsultationScope,
    DiversityRule, MAX_COMMITTEE_ROUNDS, MemoryAccess, RecordedFinding, conjunctive_outcome,
};
use kontor_core::id::{
    AdvisorProfileId, BoundedText, CommitteeTemplateId, CurrencyCode, ExternalName, Money, RoleKey,
    RoleSlotId, SCHEMA_VERSION, SpecVersion,
};
use kontor_core::spec::{BudgetBounds, ModelChainPolicy, ModelRef, ModelRung, ProviderRef};

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).expect("external name")
}

fn role(value: &str) -> RoleKey {
    RoleKey::parse(value).expect("role key")
}

fn slot(value: &str) -> RoleSlotId {
    RoleSlotId::parse(value).expect("slot id")
}

fn budget() -> BudgetBounds {
    BudgetBounds {
        max_tokens: 200_000,
        max_commands: 40,
        max_duration_seconds: 1_800,
        max_cost: Money {
            minor_units: 5_000,
            currency: CurrencyCode::parse("NOK").expect("currency"),
        },
    }
}

fn chain(providers: &[&str]) -> ModelChainPolicy {
    ModelChainPolicy {
        rungs: providers
            .iter()
            .map(|provider| ModelRung {
                provider: ProviderRef((*provider).to_owned()),
                model: ModelRef(format!("{provider}-flagship")),
                effort: None,
            })
            .collect(),
    }
}

fn grant() -> ConsultationContextPolicy {
    ConsultationContextPolicy {
        skills: Vec::new(),
        files: Vec::new(),
        memory: MemoryAccess::None,
    }
}

fn advisor() -> AdvisorProfileSpec {
    AdvisorProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id: AdvisorProfileId::generate(),
        version: SpecVersion::FIRST,
        name: name("Data platform advisor"),
        short_name: name("Data"),
        expertise: text("Postgres, CDC and inter-service data flow"),
        behavior: text("Answer the question asked. Cite the evidence you were given."),
        output_requirements: text("A recommendation and the evidence it rests on."),
        models: chain(&["anthropic"]),
        context: grant(),
        allowed_caller_roles: vec![role("lead")],
        allowed_scopes: vec![ConsultationScope::Epic],
        budget: budget(),
        max_consultations: 2,
    }
}

fn reviewer(id: &str, provider: &[&str]) -> CommitteeSlotSpec {
    CommitteeSlotSpec {
        id: slot(id),
        role: CommitteeRole::Reviewer,
        logical_role: role("reviewer"),
        specialty: text("Independent correctness review"),
        behavior: text("Review the frozen evidence and record one finding."),
        models: chain(provider),
        context: grant(),
    }
}

fn judge(id: &str) -> CommitteeSlotSpec {
    CommitteeSlotSpec {
        id: slot(id),
        role: CommitteeRole::Judge,
        logical_role: role("judge"),
        specialty: text("Explains the recomputed outcome"),
        behavior: text("Read both findings and explain the outcome the rule produced."),
        models: chain(&["google"]),
        context: grant(),
    }
}

/// The one production preset: two contrasting reviewers and one Judge.
fn independent_review() -> CommitteeTemplateSpec {
    CommitteeTemplateSpec {
        schema_version: SCHEMA_VERSION,
        template_id: CommitteeTemplateId::generate(),
        version: SpecVersion::FIRST,
        name: name("Independent review"),
        short_name: name("Review"),
        charter: text("Is this change compliant with the plan it claims to implement?"),
        slots: vec![
            reviewer("reviewer-a", &["anthropic"]),
            reviewer("reviewer-b", &["openai"]),
            judge("judge"),
        ],
        aggregation: AggregationProtocol::Conjunctive,
        diversity: DiversityRule::DistinctProviderPerSlot,
        allowed_caller_roles: vec![role("lead")],
        allowed_scopes: vec![ConsultationScope::Epic, ConsultationScope::Ticket],
        budget: budget(),
        round_limit: MAX_COMMITTEE_ROUNDS,
    }
}

#[test]
fn independent_review_is_publishable() {
    independent_review()
        .validate()
        .expect("the preset validates");
}

#[test]
fn canonical_hash_is_stable_across_equal_revisions() {
    let template = independent_review();
    let first = template.canonicalize().expect("canonicalizes");
    let second = template.canonicalize().expect("canonicalizes");
    assert_eq!(first.hash(), second.hash());
}

#[test]
fn reviewers_sharing_a_primary_provider_are_refused() {
    let mut template = independent_review();
    template.slots[1] = reviewer("reviewer-b", &["anthropic"]);
    assert!(
        template.validate().is_err(),
        "two reviewers on one provider are not independent"
    );
}

#[test]
fn reviewers_colliding_only_on_a_fallback_rung_are_refused() {
    // The primary rungs contrast, so a check that read only the first rung
    // would publish this. Under load both reviewers would land on `openai`.
    let mut template = independent_review();
    template.slots[0] = reviewer("reviewer-a", &["anthropic", "openai"]);
    assert!(
        template.validate().is_err(),
        "a shared fallback collapses independence exactly when it is needed"
    );
}

#[test]
fn shared_providers_are_allowed_when_no_distinctness_is_declared() {
    let mut template = independent_review();
    template.diversity = DiversityRule::None;
    template.slots[1] = reviewer("reviewer-b", &["anthropic"]);
    template
        .validate()
        .expect("a fixture may exercise cardinality without claiming independence");
}

#[test]
fn one_reviewer_is_refused() {
    let mut template = independent_review();
    template.slots = vec![reviewer("reviewer-a", &["anthropic"]), judge("judge")];
    assert!(
        template.validate().is_err(),
        "one reviewer has nothing to agree with"
    );
}

#[test]
fn two_judges_are_refused() {
    let mut template = independent_review();
    template.slots.push(judge("judge-2"));
    assert!(template.validate().is_err(), "at most one Judge may read");
}

#[test]
fn duplicate_slot_ids_are_refused() {
    let mut template = independent_review();
    template.slots[1] = reviewer("reviewer-a", &["openai"]);
    assert!(
        template.validate().is_err(),
        "findings are keyed by slot id, so two slots cannot share one"
    );
}

#[test]
fn a_third_round_cannot_be_declared() {
    let mut template = independent_review();
    template.round_limit = MAX_COMMITTEE_ROUNDS + 1;
    assert!(
        template.validate().is_err(),
        "one decision round and at most one re-review"
    );
}

#[test]
fn cardinality_is_data_not_three() {
    // Two-seat and five-seat templates use the same validated path. A service
    // that hard-coded three seats would have to reject one of these.
    let mut two = independent_review();
    two.slots = vec![
        reviewer("reviewer-a", &["anthropic"]),
        reviewer("reviewer-b", &["openai"]),
    ];
    two.validate().expect("two reviewers, no Judge");

    let mut five = independent_review();
    five.diversity = DiversityRule::None;
    five.slots = (0..5)
        .map(|index| reviewer(&format!("reviewer-{index}"), &["anthropic"]))
        .collect();
    five.validate().expect("five reviewers");
    assert_eq!(five.reviewer_slots().len(), 5);
}

#[test]
fn judge_slot_is_reported_when_frozen() {
    let template = independent_review();
    assert_eq!(template.judge_slot(), Some(&slot("judge")));
    assert_eq!(template.reviewer_slots().len(), 2);
}

// ---------------------------------------------------------------------------
// The conjunctive truth table
// ---------------------------------------------------------------------------

fn finding(id: &str, verdict: CommitteeVerdict, evidence_complete: bool) -> RecordedFinding {
    RecordedFinding {
        slot: slot(id),
        verdict,
        evidence_complete,
    }
}

#[test]
fn both_compliant_with_complete_evidence_settles_compliant() {
    let required = vec![slot("reviewer-a"), slot("reviewer-b")];
    let recorded = vec![
        finding("reviewer-a", CommitteeVerdict::Compliant, true),
        finding("reviewer-b", CommitteeVerdict::Compliant, true),
    ];
    assert_eq!(
        conjunctive_outcome(&required, &recorded),
        Some(CommitteeVerdict::Compliant)
    );
}

#[test]
fn one_dissent_settles_non_compliant() {
    let required = vec![slot("reviewer-a"), slot("reviewer-b")];
    let recorded = vec![
        finding("reviewer-a", CommitteeVerdict::Compliant, true),
        finding("reviewer-b", CommitteeVerdict::NonCompliant, true),
    ];
    assert_eq!(
        conjunctive_outcome(&required, &recorded),
        Some(CommitteeVerdict::NonCompliant)
    );
}

#[test]
fn missing_required_evidence_settles_non_compliant() {
    // Counted against the gate, never dropped from the denominator.
    let required = vec![slot("reviewer-a"), slot("reviewer-b")];
    let recorded = vec![
        finding("reviewer-a", CommitteeVerdict::Compliant, true),
        finding("reviewer-b", CommitteeVerdict::Compliant, false),
    ];
    assert_eq!(
        conjunctive_outcome(&required, &recorded),
        Some(CommitteeVerdict::NonCompliant)
    );
}

#[test]
fn a_missing_finding_blocks_settlement_rather_than_passing() {
    let required = vec![slot("reviewer-a"), slot("reviewer-b")];
    let recorded = vec![finding("reviewer-a", CommitteeVerdict::Compliant, true)];
    assert_eq!(
        conjunctive_outcome(&required, &recorded),
        None,
        "an absent finding is not agreement"
    );
}

// ---------------------------------------------------------------------------
// Advisor profiles
// ---------------------------------------------------------------------------

#[test]
fn advisor_profile_is_publishable() {
    advisor().validate().expect("the profile validates");
}

#[test]
fn an_advisor_no_role_may_consult_is_refused() {
    let mut profile = advisor();
    profile.allowed_caller_roles.clear();
    assert!(profile.validate().is_err());
}

#[test]
fn an_advisor_with_no_scope_is_refused() {
    let mut profile = advisor();
    profile.allowed_scopes.clear();
    assert!(profile.validate().is_err());
}

#[test]
fn duplicate_scopes_are_refused_rather_than_deduplicated() {
    let mut profile = advisor();
    profile.allowed_scopes = vec![ConsultationScope::Epic, ConsultationScope::Epic];
    assert!(
        profile.validate().is_err(),
        "two revisions meaning the same thing must not hash differently"
    );
}

#[test]
fn a_zero_consultation_limit_is_refused() {
    let mut profile = advisor();
    profile.max_consultations = 0;
    assert!(profile.validate().is_err());
}

#[test]
fn empty_behavioural_prose_is_refused() {
    let mut profile = advisor();
    profile.behavior = text("   ");
    assert!(profile.validate().is_err());
}

#[test]
fn dispositions_round_trip_their_stable_spelling() {
    for disposition in AdviceDisposition::ALL {
        let parsed = AdviceDisposition::parse(disposition.as_str()).expect("round trips");
        assert_eq!(&parsed, disposition);
    }
}
