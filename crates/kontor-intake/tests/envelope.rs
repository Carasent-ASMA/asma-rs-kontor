//! One envelope, whatever the source, and the same bytes every time.
//!
//! The mutants this suite exists to kill:
//!
//! * an envelope whose bytes depend on the order an adapter happened to insert
//!   its attributes in, which would give the same event two digests and defeat
//!   deduplication;
//! * a source kind that changes the *shape* of the envelope rather than one of
//!   its values, which is how source-specific handling gets back in downstream;
//! * an adapter that forwards credential- or identity-bearing material, which
//!   the shared scanner must refuse at canonicalization rather than at review;
//! * an unbounded attribute map, which would let a source system decide this
//!   database's row sizes.

mod fixture;

use fixture::{at, event, inbound};
use kontor_core::id::SourceEventId;
use kontor_intake::{adapter::MAX_ATTRIBUTES, canonicalize};

/// The exact bytes one canonical envelope has. A golden, so a change to the
/// envelope is a decision somebody makes rather than one that happens.
const GOLDEN: &str = concat!(
    r#"{"attributes":{"kind":"work.requested","module":"delivery"},"#,
    r#""event_schema":"schema.work-requested","event_schema_version":2,"#,
    r#""external_event_id":"ext-1","observed_at":"2026-08-12T09:00:00Z","#,
    r#""schema_version":1,"source_connection":"conn.alpha","source_kind":"manual","#,
    r#""subject":"A unit of work someone asked for"}"#
);

fn attributes() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("kind", serde_json::json!("work.requested")),
        ("module", serde_json::json!("delivery")),
    ]
}

#[test]
fn the_canonical_envelope_is_byte_exact() {
    let event = event("manual", "conn.alpha", "ext-1", &attributes());
    assert_eq!(event.envelope.json(), GOLDEN);
    assert_eq!(
        event.envelope.hash().as_str(),
        kontor_core::id::ContentHash::of(GOLDEN.as_bytes()).as_str(),
        "the recorded digest is the digest of exactly these bytes"
    );
}

#[test]
fn five_source_kinds_produce_one_shape() {
    // Manual, pull request, CI, monitoring and bug report. The only difference
    // between the five envelopes is the value of `source_kind` — which is data.
    let kinds = ["manual", "pull_request", "ci", "monitoring", "bug_report"];
    let mut shapes = std::collections::BTreeSet::new();
    for kind in kinds {
        let event = event(kind, "conn.alpha", "ext-1", &attributes());
        let value: serde_json::Value =
            serde_json::from_str(event.envelope.json()).expect("the envelope is JSON");
        let keys: Vec<String> = value
            .as_object()
            .expect("the envelope is an object")
            .keys()
            .cloned()
            .collect();
        shapes.insert(keys);
        assert_eq!(
            value.pointer("/source_kind").and_then(|v| v.as_str()),
            Some(kind),
            "the source kind survives as a value"
        );
        assert_eq!(
            event.envelope.json().replace(kind, "manual"),
            GOLDEN,
            "nothing but the source kind differs between the five"
        );
    }
    assert_eq!(
        shapes.len(),
        1,
        "five source kinds produced more than one envelope shape"
    );
}

#[test]
fn attribute_insertion_order_does_not_change_the_digest() {
    let forwards = event("ci", "conn.alpha", "ext-1", &attributes());
    let mut reversed = attributes();
    reversed.reverse();
    let backwards = event("ci", "conn.alpha", "ext-1", &reversed);
    assert_eq!(forwards.envelope.json(), backwards.envelope.json());
    assert_eq!(forwards.envelope.hash(), backwards.envelope.hash());
}

#[test]
fn an_adapter_cannot_forward_sensitive_material() {
    for (name, value) in [
        ("authorization", serde_json::json!("Bearer abc")),
        ("api_key", serde_json::json!("k-123")),
        ("cookie", serde_json::json!("session=1")),
    ] {
        let inbound = inbound("monitoring", "conn.alpha", "ext-1", &[(name, value)]);
        assert!(
            canonicalize(
                &inbound,
                SourceEventId::generate(),
                at(fixture::INGESTED_AT)
            )
            .is_err(),
            "`{name}` must be refused before a digest exists"
        );
    }
}

#[test]
fn attribute_names_and_counts_are_bounded() {
    let odd = inbound(
        "bug_report",
        "conn.alpha",
        "ext-1",
        &[("Not A Key", serde_json::json!(1))],
    );
    assert!(
        canonicalize(&odd, SourceEventId::generate(), at(fixture::INGESTED_AT)).is_err(),
        "an attribute name that is not a lexical key must be refused"
    );

    let many: Vec<(String, serde_json::Value)> = (0..=MAX_ATTRIBUTES)
        .map(|index| (format!("a{index}"), serde_json::json!(index)))
        .collect();
    let mut event = inbound("bug_report", "conn.alpha", "ext-1", &[]);
    event.attributes = many.into_iter().collect();
    assert!(
        canonicalize(&event, SourceEventId::generate(), at(fixture::INGESTED_AT)).is_err(),
        "more attributes than the bound must be refused"
    );
}

#[test]
fn a_stored_event_starts_unevaluated() {
    let event = event("manual", "conn.alpha", "ext-1", &attributes());
    assert_eq!(
        event.processing_state,
        kontor_core::spec::SourceProcessingState::Received,
        "nothing has decided about it yet, and the envelope says so"
    );
}
