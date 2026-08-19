//! The consultation presets this build ships.
//!
//! The bundled data is loaded through the same parser a deployment would use
//! for a pack of its own, so a preset that could not be published by an Admin
//! cannot be shipped either.

use kontor_core::consultation::{AggregationProtocol, CommitteeRole, DiversityRule};
use kontor_profiles::seeds::bundled_consultation_presets;

#[test]
fn the_bundled_presets_parse_and_validate() {
    bundled_consultation_presets().expect("the checked-in presets are publishable");
}

#[test]
fn exactly_one_production_preset_is_shipped() {
    // Jury, quorum and deliberative panel are protocol deferrals. Shipping a
    // speculative preset would publish authority nobody asked for, and shipping
    // a second conjunctive one would invite a service to branch on which.
    let pack = bundled_consultation_presets().expect("the presets load");
    assert_eq!(pack.committee_templates.len(), 1);
}

#[test]
fn the_preset_is_an_independent_conjunction() {
    let pack = bundled_consultation_presets().expect("the presets load");
    let template = &pack.committee_templates[0];
    assert_eq!(template.aggregation, AggregationProtocol::Conjunctive);
    assert_eq!(template.diversity, DiversityRule::DistinctProviderPerSlot);
    assert_eq!(template.reviewer_slots().len(), 2);
    assert!(
        template.judge_slot().is_some(),
        "the preset freezes a Judge to explain the recomputed outcome"
    );
}

#[test]
fn the_presets_reviewers_reach_contrasting_providers() {
    // Validation already refuses a shared provider, so this asserts the shipped
    // data actually exercises that rule rather than passing it by declaring one
    // reviewer.
    let pack = bundled_consultation_presets().expect("the presets load");
    let template = &pack.committee_templates[0];
    let providers: Vec<&str> = template
        .slots
        .iter()
        .filter(|slot| slot.role == CommitteeRole::Reviewer)
        .flat_map(|slot| {
            slot.models
                .rungs
                .iter()
                .map(|rung| rung.provider.0.as_str())
        })
        .collect();
    let distinct: std::collections::BTreeSet<&str> = providers.iter().copied().collect();
    assert_eq!(
        providers.len(),
        distinct.len(),
        "two reviewers reaching one provider would not be independent"
    );
}

#[test]
fn the_preset_allows_one_remediation_round_at_most() {
    let pack = bundled_consultation_presets().expect("the presets load");
    assert_eq!(pack.committee_templates[0].round_limit, 2);
}
