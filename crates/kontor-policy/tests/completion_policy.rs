//! Completion policy invariants.

use std::collections::BTreeSet;

use kontor_core::id::{ContentHash, ExternalName, TaskId};
use kontor_policy::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, NeedsHumanPayload, TicketEvidence,
    TicketGateBlocker, TicketRequirement, closeout_blockers, ticket_gate_blockers,
};

fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).expect("test names are valid")
}

#[test]
fn every_declared_ticket_goal_and_artifact_is_required() {
    let task = TaskId::generate();
    let requirements = [TicketRequirement {
        task_id: task,
        goals: BTreeSet::from([name("accepted")]),
        evidence: BTreeSet::from([name("tests")]),
    }];

    assert_eq!(
        ticket_gate_blockers(&requirements, &[]).expect("the declaration is valid"),
        [TicketGateBlocker::MissingTicket(task)]
    );
    assert_eq!(
        ticket_gate_blockers(
            &requirements,
            &[TicketEvidence {
                task_id: task,
                goals: BTreeSet::from([name("accepted")]),
                evidence: BTreeSet::new(),
            }],
        )
        .expect("the evidence is valid"),
        [TicketGateBlocker::MissingEvidence {
            task_id: task,
            evidence: name("tests"),
        }]
    );
}

#[test]
fn done_requires_every_closeout_receipt_and_a_version_inventory() {
    let mut evidence = CloseoutEvidence::default();
    assert_eq!(closeout_blockers(&evidence), CloseoutRequirement::ALL);

    let receipt = || Some(ContentHash::of(b"receipt"));
    evidence.merge_receipt = receipt();
    evidence.release_receipt = receipt();
    evidence
        .delivered_versions
        .insert(name("kontord"), name("0.1.0"));
    evidence.summary_receipt = receipt();
    evidence.notification_receipt = receipt();
    evidence.archive_receipt = receipt();
    assert!(closeout_blockers(&evidence).is_empty());
}

#[test]
fn needs_human_cannot_be_constructed_or_restored_without_a_tried_path() {
    let recommendation = name("Review the unresolved evidence with the LSA and TPM");
    assert!(NeedsHumanPayload::new(recommendation.clone(), Vec::new()).is_err());

    let invalid = serde_json::json!({
        "recommended_resolution": recommendation,
        "tried_deliberation_path": [],
    });
    assert!(serde_json::from_value::<NeedsHumanPayload>(invalid).is_err());

    let valid = NeedsHumanPayload::new(
        recommendation,
        vec![DeliberationStep {
            role: name("LSA and TPM"),
            consultation: name("independent review"),
            round: 1,
            outcome: name("failed"),
        }],
    )
    .expect("the path is complete");
    let restored: NeedsHumanPayload =
        serde_json::from_value(serde_json::to_value(&valid).expect("serializable"))
            .expect("restorable");
    assert_eq!(restored, valid);
}
