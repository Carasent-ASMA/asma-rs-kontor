//! External-ticket body observation and content-conflict classification.
//!
//! Status reconciliation answers "where is this ticket". These cases answer the
//! question that went unasked until ASMA-8123: "does its body still say what
//! Kontor published". The two are independent, and the gap this suite closes is
//! that a body could stay a creation placeholder forever while every status
//! reconciliation reported converged.
//!
//! The mutants this suite exists to kill:
//!
//! * treating an absent observation as "no conflict" instead of missing evidence;
//! * treating a present-but-empty body as converged;
//! * failing to recognize the `Kontor <kind> <uuid>: <title>` creation marker,
//!   which is the exact shape that produced the ASMA-8098/8101/8108/8109/8111
//!   placeholder epics;
//! * flagging ordinary authored prose that merely begins with the word "Kontor"
//!   as a placeholder;
//! * attributing a foreign edit to Kontor's own stale projection, which would
//!   license overwriting human-authored content;
//! * reporting a conflict when the observed body already equals the intent.

use kontor_core::id::{BoundedText, CanonicalDocument, ContentHash};
use kontor_core::ticket::{
    ContentConflictKind, ObservedBody, classify_body, classify_observed_body,
};
use serde_json::json;

fn hash_of(text: &str) -> ContentHash {
    CanonicalDocument::from_serializable(&json!({"schema_version": 1, "body": text}))
        .expect("canonical")
        .hash()
        .clone()
}

fn body(text: &str) -> ObservedBody {
    ObservedBody {
        present: true,
        content_hash: hash_of(text),
        plain_text: BoundedText::parse(text).expect("bounded text"),
    }
}

fn absent_body() -> ObservedBody {
    ObservedBody {
        present: false,
        content_hash: hash_of(""),
        plain_text: BoundedText::parse("").expect("bounded text"),
    }
}

#[test]
fn an_observed_body_equal_to_the_intent_is_not_a_conflict() {
    let observed = body("## Goal\nShip the thing.");
    let intended = hash_of("## Goal\nShip the thing.");
    assert_eq!(classify_body(Some(&observed), Some(&intended), None), None);
}

#[test]
fn a_real_body_with_nothing_intended_is_not_kontors_to_judge() {
    // Kontor publishes nothing for this issue, so whatever the body says is not
    // Kontor's to judge. This is what keeps operator-owned issues untouched.
    let observed = body("Entirely human-authored notes.");
    assert_eq!(classify_body(Some(&observed), None, None), None);
    assert_eq!(classify_observed_body(Some(&observed)), None);
}

#[test]
fn reconciliation_judges_a_body_without_any_intended_content() {
    // The question reconciliation asks. It has no authored body to compare
    // against and must still refuse to call these three states agreement,
    // because that combination — converged status, unreadable body — is the
    // whole reported defect.
    let marker = body("Kontor epic 01a0721b-ea30-7fe3-88a5-4d33ca613414: Publication identity");
    assert_eq!(
        classify_observed_body(Some(&marker)),
        Some(ContentConflictKind::PlaceholderBodyOnly)
    );
    assert_eq!(
        classify_observed_body(Some(&absent_body())),
        Some(ContentConflictKind::MissingExternalBody)
    );
    assert_eq!(
        classify_observed_body(None),
        Some(ContentConflictKind::UnreadableExternalBody)
    );
}

#[test]
fn reconciliation_never_reports_divergence() {
    // Without authored content there is nothing to diverge *from*, so this path
    // has no opinion about a body Kontor did not write. Reporting divergence
    // here would flag every legitimately human-authored issue on every pass.
    let observed = body("A body Kontor would never have written.");
    assert_eq!(classify_observed_body(Some(&observed)), None);
}

#[test]
fn a_placeholder_is_a_conflict_even_when_nothing_is_intended() {
    // The ordering that matters: `classify_body` decides missing, empty and
    // placeholder-only first, so a caller holding no authored body still learns
    // that the reader is looking at a marker.
    let marker = body("Kontor task 01a07dcd-bac5-7e42-9530-f5a948ebaaa3: PUB-08");
    assert_eq!(
        classify_body(Some(&marker), None, None),
        Some(ContentConflictKind::PlaceholderBodyOnly)
    );
}

#[test]
fn a_missing_observation_is_missing_evidence_not_agreement() {
    let intended = hash_of("## Goal\nShip the thing.");
    assert_eq!(
        classify_body(None, Some(&intended), None),
        Some(ContentConflictKind::UnreadableExternalBody)
    );
}

#[test]
fn a_present_but_empty_body_is_reported_missing() {
    let intended = hash_of("## Goal\nShip the thing.");
    assert_eq!(
        classify_body(Some(&absent_body()), Some(&intended), None),
        Some(ContentConflictKind::MissingExternalBody)
    );
    // Whitespace is not content either.
    assert_eq!(
        classify_body(Some(&body("   \n  ")), Some(&intended), None),
        Some(ContentConflictKind::MissingExternalBody)
    );
}

#[test]
fn the_creation_marker_is_recognized_as_a_placeholder() {
    // The exact shape observed on ASMA-8098/8101/8108/8109/8111.
    let observed = body(
        "Kontor epic 01a06e13-878a-7aa3-8a08-bfad07cc8c4c: Kontor compatibility and documentation recovery",
    );
    assert!(observed.is_placeholder_only());
    let intended = hash_of("## Summary / purpose\nReal content.");
    assert_eq!(
        classify_body(Some(&observed), Some(&intended), None),
        Some(ContentConflictKind::PlaceholderBodyOnly)
    );
}

#[test]
fn the_task_creation_marker_is_recognized_too() {
    // The exact body Kontor wrote for ASMA-8123 at creation.
    let observed = body(
        "Kontor task 01a07dcd-bac5-7e42-9530-f5a948ebaaa3: PUB-08 Jira description read and update projection with typed content conflicts",
    );
    assert!(observed.is_placeholder_only());
}

#[test]
fn authored_prose_beginning_with_kontor_is_not_a_placeholder() {
    // The discriminator must be the marker's shape, not the first word.
    for text in [
        "Kontor cannot read a Jira description today, which is why this epic exists.",
        "Kontor epic work continues: the body is real prose with a colon.",
        "Kontor task: no identifier at all.",
        "Kontor epic 01a06e13: too short to be a uuid.",
        "Kontor widget 01a06e13-878a-7aa3-8a08-bfad07cc8c4c: unknown kind.",
    ] {
        assert!(
            !body(text).is_placeholder_only(),
            "must not classify authored prose as a placeholder: {text}"
        );
    }
}

#[test]
fn an_empty_body_is_not_reported_as_a_placeholder() {
    // Emptiness has its own conflict kind; conflating the two would hide it.
    assert!(!absent_body().is_placeholder_only());
    assert!(!body("  ").is_placeholder_only());
}

#[test]
fn a_body_still_equal_to_what_kontor_published_is_kontors_own_drift() {
    let published_text = "## Goal\nThe previously published goal.";
    let observed = body(published_text);
    let published = hash_of(published_text);
    let intended = hash_of("## Goal\nA newer goal.");
    assert_eq!(
        classify_body(Some(&observed), Some(&intended), Some(&published)),
        Some(ContentConflictKind::DivergedFromProjection)
    );
}

#[test]
fn a_body_nobody_published_is_treated_as_human_authored() {
    // The safe default: without proof that Kontor wrote it, a divergent body
    // belongs to a human and is never overwritten to satisfy a checklist.
    let observed = body("## Goal\nEdited by a human in the Jira UI.");
    let published = hash_of("## Goal\nWhat Kontor last published.");
    let intended = hash_of("## Goal\nA newer goal.");
    assert_eq!(
        classify_body(Some(&observed), Some(&intended), Some(&published)),
        Some(ContentConflictKind::HumanAuthoredDivergence)
    );
    // Absent publication evidence must reach the same conclusion.
    assert_eq!(
        classify_body(Some(&observed), Some(&intended), None),
        Some(ContentConflictKind::HumanAuthoredDivergence)
    );
}

#[test]
fn every_conflict_kind_round_trips_its_stable_spelling() {
    for kind in ContentConflictKind::ALL {
        assert_eq!(
            ContentConflictKind::parse(kind.as_str()).expect("parse"),
            *kind
        );
    }
    assert!(ContentConflictKind::parse("not_a_kind").is_err());
}
