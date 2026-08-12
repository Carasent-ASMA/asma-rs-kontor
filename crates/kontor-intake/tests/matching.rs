//! Matching is data, deduplication is deterministic, and neither reads a kind.
//!
//! The mutants this suite exists to kill:
//!
//! * a filter that passes when a clause's pointer does not resolve at all,
//!   which would fire a trigger on an envelope that never carried the field;
//! * a match that ignores the pinned event-schema revision, so a trigger
//!   written against revision 2 silently fires on revision 3;
//! * a dedup key that depends on pointer *order* being lost, or that two
//!   different envelopes can share;
//! * an ordering that depends on the order the caller listed the triggers, so
//!   two machines pick different revisions for one event;
//! * a proposal that arrives already approved, or already carrying work.

mod fixture;

use fixture::{at, event, trigger};
use kontor_core::id::{IntakeReceiptId, SpecVersion};
use kontor_core::spec::IntakeResult;
use kontor_intake::{Intake, evaluate, match_triggers};
use proptest::prelude::*;

fn attributes(kind: &str) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("kind", serde_json::json!(kind)),
        ("module", serde_json::json!("delivery")),
    ]
}

fn decide<'a>(
    event: &kontor_core::spec::CanonicalSourceEvent,
    triggers: &'a [kontor_core::spec::TriggerSpec],
) -> Intake<'a> {
    evaluate(
        event,
        triggers,
        IntakeReceiptId::generate(),
        at(fixture::INGESTED_AT),
    )
    .expect("evaluation reads only its arguments")
}

#[test]
fn a_matching_trigger_produces_a_proposal_and_nothing_more() {
    let event = event(
        "pull_request",
        "conn.alpha",
        "ext-1",
        &attributes("work.requested"),
    );
    let triggers = vec![trigger(
        "trigger.delivery",
        "pull_request",
        "conn.alpha",
        &[("/attributes/kind", "work.requested")],
    )];
    let Intake::Proposed { receipt, matched } = decide(&event, &triggers) else {
        panic!("the trigger matches this event");
    };
    assert_eq!(receipt.result, IntakeResult::Proposed);
    assert!(
        receipt.approval.is_none() && receipt.proposed.is_none(),
        "proposing is not arming: a proposal carries no approval and no work"
    );
    assert_eq!(receipt.trigger_version, triggers[0].version);
    assert_eq!(
        receipt.dedup_key,
        triggers[0]
            .dedup
            .evaluate(&event.envelope)
            .expect("the fixture dedup expression resolves"),
        "the receipt's key is the trigger's own expression, not a copy of it"
    );
    assert_eq!(matched.pins.work_profile, triggers[0].work_profile);
    assert_eq!(matched.pins.budget, triggers[0].limits.budget);
}

#[test]
fn a_clause_that_does_not_resolve_does_not_match() {
    let event = event("ci", "conn.alpha", "ext-1", &attributes("work.requested"));
    let absent = trigger(
        "trigger.absent",
        "ci",
        "conn.alpha",
        &[("/attributes/never_sent", "anything")],
    );
    assert!(
        match_triggers(&event, std::slice::from_ref(&absent))
            .expect("the envelope is readable")
            .is_empty(),
        "an unresolvable clause is not a satisfied clause"
    );
    // It still *addresses* the event, so the decision is a recorded `ignored`
    // rather than silence.
    let Intake::Ignored { receipt } = decide(&event, std::slice::from_ref(&absent)) else {
        panic!("a trigger on this connection declined the event");
    };
    assert_eq!(receipt.result, IntakeResult::Ignored);
    assert_eq!(receipt.trigger, absent.id);
    assert!(
        receipt.proposed.is_none(),
        "an ignored event creates no work"
    );
}

#[test]
fn nothing_addressing_the_event_writes_no_receipt() {
    let event = event(
        "monitoring",
        "conn.beta",
        "ext-1",
        &attributes("work.requested"),
    );
    let elsewhere = trigger(
        "trigger.elsewhere",
        "monitoring",
        "conn.alpha",
        &[("/attributes/kind", "work.requested")],
    );
    assert!(
        matches!(
            decide(&event, std::slice::from_ref(&elsewhere)),
            Intake::Unaddressed
        ),
        "there is no trigger revision to pin, so there is no decision to record"
    );
}

#[test]
fn the_pinned_event_schema_revision_is_part_of_the_match() {
    let event = event(
        "manual",
        "conn.alpha",
        "ext-1",
        &attributes("work.requested"),
    );
    let mut newer = trigger(
        "trigger.delivery",
        "manual",
        "conn.alpha",
        &[("/attributes/kind", "work.requested")],
    );
    newer.event_schema_version = SpecVersion::parse(3).expect("a legal revision");
    assert!(
        match_triggers(&event, std::slice::from_ref(&newer))
            .expect("the envelope is readable")
            .is_empty(),
        "a trigger pinned to another schema revision does not fire on this one"
    );
}

#[test]
fn the_match_order_does_not_depend_on_the_callers_order() {
    let event = event("ci", "conn.alpha", "ext-1", &attributes("work.requested"));
    let clause = [("/attributes/kind", "work.requested")];
    let first = trigger("trigger.aaa", "ci", "conn.alpha", &clause);
    let mut newer = trigger("trigger.zzz", "ci", "conn.alpha", &clause);
    newer.version = SpecVersion::parse(2).expect("a legal revision");
    let older = trigger("trigger.zzz", "ci", "conn.alpha", &clause);

    let forwards = vec![first.clone(), newer.clone(), older.clone()];
    let backwards = vec![older, newer, first];
    let left = match_triggers(&event, &forwards).expect("readable");
    let right = match_triggers(&event, &backwards).expect("readable");
    let render = |matched: &[&kontor_core::spec::TriggerSpec]| -> Vec<String> {
        matched
            .iter()
            .map(|spec| format!("{}@{}", spec.id.as_str(), spec.version.get()))
            .collect()
    };
    assert_eq!(render(&left), render(&right));
    assert_eq!(
        render(&left),
        vec!["trigger.aaa@1", "trigger.zzz@2", "trigger.zzz@1"],
        "key ascending, then revision descending — the newest revision first"
    );
}

#[test]
fn the_same_event_and_catalog_decide_identically_every_time() {
    let event = event(
        "manual",
        "conn.alpha",
        "ext-1",
        &attributes("work.requested"),
    );
    let triggers = vec![trigger(
        "trigger.delivery",
        "manual",
        "conn.alpha",
        &[("/attributes/kind", "work.requested")],
    )];
    let (
        Intake::Proposed { receipt: first, .. },
        Intake::Proposed {
            receipt: second, ..
        },
    ) = (decide(&event, &triggers), decide(&event, &triggers))
    else {
        panic!("both evaluations propose");
    };
    // The receipt id and nothing else differs: two evaluations of one event
    // under one revision are the same decision, which is what makes a resumed
    // intake a replay rather than a second verdict.
    assert!(first.decides_the_same_as(&second));
    assert_eq!(first.idempotency_key, second.idempotency_key);
}

proptest! {
    /// Two envelopes agree on a dedup key exactly when they agree on every
    /// pointer the expression names — never merely when they look similar.
    #[test]
    fn the_dedup_key_is_a_function_of_the_named_pointers_only(
        left_kind in "[a-z]{1,12}",
        right_kind in "[a-z]{1,12}",
        left_id in "[a-z0-9-]{1,12}",
        right_id in "[a-z0-9-]{1,12}",
        left_noise in "[a-z]{1,12}",
        right_noise in "[a-z]{1,12}",
    ) {
        let spec = trigger("trigger.delivery", "manual", "conn.alpha", &[]);
        let left = event("manual", "conn.alpha", &left_id, &[
            ("kind", serde_json::json!(left_kind)),
            ("noise", serde_json::json!(left_noise)),
        ]);
        let right = event("manual", "conn.alpha", &right_id, &[
            ("kind", serde_json::json!(right_kind)),
            ("noise", serde_json::json!(right_noise)),
        ]);
        let left_key = spec.dedup.evaluate(&left.envelope).expect("resolves");
        let right_key = spec.dedup.evaluate(&right.envelope).expect("resolves");
        let named_fields_agree = left_kind == right_kind && left_id == right_id;
        prop_assert_eq!(left_key == right_key, named_fields_agree);
    }

    /// A filter clause holds exactly when the pointer resolves to its literal.
    #[test]
    fn a_filter_holds_exactly_when_the_pointer_equals_the_literal(
        sent in "[a-z.]{1,16}",
        wanted in "[a-z.]{1,16}",
    ) {
        let event = event("manual", "conn.alpha", "ext-1", &[
            ("kind", serde_json::json!(sent)),
        ]);
        let spec = trigger(
            "trigger.delivery",
            "manual",
            "conn.alpha",
            &[("/attributes/kind", &wanted)],
        );
        prop_assert_eq!(
            spec.matches(&event.envelope).expect("readable"),
            sent == wanted
        );
    }
}
