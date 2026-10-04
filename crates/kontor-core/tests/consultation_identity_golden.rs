//! Golden bytes of the shared consultation identity (ASMA-8113, consumed by
//! ASMA-8282).
//!
//! Adding the `planning_pair` family must not move a single Advisor or
//! Committee identity. These digests were captured from the unmodified
//! identity function at `832e60cce695c620c5576c7523ef5ebac3d1773a`, before
//! the third family existed, and every later revision is held to them byte
//! for byte.

use kontor_core::consultation::{ConsultationFamily, ConsultationIdentity};
use kontor_core::id::{ContentHash, ExternalName, MiniProjectId, ProjectId, SpecVersion, TaskId};

fn fixed() -> (ProjectId, MiniProjectId, TaskId, ContentHash, ExternalName) {
    (
        ProjectId::parse("01991c00-0000-7000-8000-000000000001").expect("project"),
        MiniProjectId::parse("01991c00-0000-7000-8000-000000000002").expect("epic"),
        TaskId::parse("01991c00-0000-7000-8000-000000000003").expect("task"),
        ContentHash::of(b"immutable profile"),
        ExternalName::parse("operational completion").expect("topic"),
    )
}

fn identity(
    family: ConsultationFamily,
    profile_id: &str,
    task: bool,
    re_review: Option<&ContentHash>,
) -> String {
    let (project, epic, task_id, definition, topic) = fixed();
    ConsultationIdentity {
        project_id: project,
        epic_id: epic,
        task_id: task.then_some(task_id),
        family,
        profile_id,
        profile_version: SpecVersion::FIRST,
        definition_hash: &definition,
        topic: &topic,
        re_review_provenance_hash: re_review,
    }
    .hash()
    .expect("identity")
    .to_string()
}

/// Captured at `832e60cc` from the unmodified identity function.
const GOLDEN: [(&str, &str); 4] = [
    (
        "advisor/epic",
        "3d4aefb47ec1ef9bd31d0c2f9bf1110e56120a3e70840d6a2cb9ee417d0576e2",
    ),
    (
        "advisor/task",
        "18c44786c9fe007fe56f161648ee01d79f2b73d1ffdad027f8f6c24823ab86a7",
    ),
    (
        "committee/task",
        "c07113ac7a968f027b7b675d8040e69c2c4b143d0fe755cc5e862a07087c9113",
    ),
    (
        "committee/re-review",
        "a75eeedcd2b45d6705443b56c1a0a43da2e9d9407b92b8ee4e9f73866ad442c5",
    ),
];

const ADVISOR: &str = "01991c00-0000-7000-8000-0000000000ad";
const COMMITTEE: &str = "01991c00-0000-7000-8000-0000000000c0";

fn current() -> [(&'static str, String); 4] {
    let lineage = ContentHash::of(b"authorized re-review");
    [
        (
            "advisor/epic",
            identity(ConsultationFamily::Advisor, ADVISOR, false, None),
        ),
        (
            "advisor/task",
            identity(ConsultationFamily::Advisor, ADVISOR, true, None),
        ),
        (
            "committee/task",
            identity(ConsultationFamily::Committee, COMMITTEE, true, None),
        ),
        (
            "committee/re-review",
            identity(
                ConsultationFamily::Committee,
                COMMITTEE,
                true,
                Some(&lineage),
            ),
        ),
    ]
}

#[test]
fn advisor_and_committee_identities_keep_their_golden_bytes() {
    for ((name, golden), (current_name, digest)) in GOLDEN.iter().zip(current()) {
        assert_eq!(*name, current_name);
        assert_eq!(digest, *golden, "{name}");
    }
}

#[test]
fn a_planning_pair_identity_is_its_own_family_and_still_ignores_retry_mechanics() {
    const PAIR: &str = "01991c00-0000-7000-8000-0000000000a1";
    assert_eq!(ConsultationFamily::PlanningPair.as_str(), "planning_pair");
    let pair = identity(ConsultationFamily::PlanningPair, PAIR, true, None);
    assert_eq!(
        pair,
        identity(ConsultationFamily::PlanningPair, PAIR, true, None),
        "the same logical planning pair has one identity"
    );
    // The same scope, profile id, revision, definition and topic under another
    // family is another consultation: the family string is a hashed input.
    for family in [ConsultationFamily::Advisor, ConsultationFamily::Committee] {
        assert_ne!(identity(family, PAIR, true, None), pair, "{family}");
    }
    assert_ne!(
        identity(ConsultationFamily::PlanningPair, PAIR, false, None),
        pair,
        "the epic and one of its tickets are different subjects"
    );
    // And the third family moved none of the golden Advisor or Committee bytes.
    for ((name, golden), (_, digest)) in GOLDEN.iter().zip(current()) {
        assert_eq!(digest, *golden, "{name}");
    }
}
