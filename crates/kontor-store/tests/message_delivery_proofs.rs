//! Message-proof anchors compare the recorded instant, not timestamp text.
//!
//! A canonical UTC timestamp keeps the fractional precision it was written
//! with, so one issuance can be `...05.724Z` and a later one `...05.724434Z`.
//! Text order puts the later instant first.

mod support;

use kontor_core::id::parse_utc_timestamp;
use kontor_store::{MessageIssuance, SqliteStore};

fn issuance(message_id: &str, binding: &str, key: &str, issued_at: &str) -> MessageIssuance {
    MessageIssuance {
        message_id: message_id.to_owned(),
        runtime_kind: "fake.runtime".to_owned(),
        host: "fake-host".to_owned(),
        runtime_binding_id: binding.to_owned(),
        native_session_id: "native-session".to_owned(),
        idempotency_key: key.to_owned(),
        provenance: "session_message_send".to_owned(),
        issued_at: parse_utc_timestamp(issued_at).expect("a canonical instant"),
        boundary_at: None,
        delivered_at: None,
    }
}

#[test]
fn an_earlier_issuance_with_fewer_fractional_digits_is_still_the_anchor() {
    let home = support::state_root();
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let binding = "01a12b8f-7a7a-7f73-90c3-89d27add6793";
    let prior = issuance(
        "01a12b8f-7a7b-7d80-8bf9-e1df35e9eb66",
        binding,
        "prior",
        "2026-10-11T15:23:05.724Z",
    );
    store
        .record_message_issuance(&prior)
        .expect("the prior issuance is recorded");
    store
        .record_message_delivery(&prior.message_id, 1, 1)
        .expect("the prior issuance is delivered");
    let current = issuance(
        "01a12b8f-7a7c-7cf3-b542-f57559872a2a",
        binding,
        "current",
        "2026-10-11T15:23:05.724434Z",
    );
    store
        .record_message_issuance(&current)
        .expect("the current issuance is recorded");
    let anchor = store
        .message_delivery_proof_anchor(&current)
        .expect("the anchor reads")
        .expect("the earlier delivered issuance is the anchor");
    assert_eq!(anchor.message_id, prior.message_id);
    assert_eq!((anchor.epoch, anchor.sequence), (1, 1));
}

#[test]
fn an_equal_instant_does_not_displace_the_first_delivered_anchor() {
    let home = support::state_root();
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let binding = "01a12b8f-7a7a-7f73-90c3-89d27add6793";
    let first = issuance(
        "01a12b8f-7a7b-7d80-8bf9-e1df35e9eb67",
        binding,
        "first",
        "2026-10-11T15:23:05.724Z",
    );
    store
        .record_message_issuance(&first)
        .expect("the first issuance is recorded");
    store
        .record_message_delivery(&first.message_id, 1, 1)
        .expect("the first issuance is delivered");
    let second = issuance(
        "01a12b8f-7a7b-7d80-8bf9-e1df35e9eb68",
        binding,
        "second",
        "2026-10-11T15:23:05.724Z",
    );
    store
        .record_message_issuance(&second)
        .expect("the second issuance is recorded");
    store
        .record_message_delivery(&second.message_id, 1, 2)
        .expect("the second issuance is delivered");
    let later = issuance(
        "01a12b8f-7a7c-7cf3-b542-f57559872a2b",
        binding,
        "later",
        "2026-10-11T15:23:06Z",
    );
    let anchor = store
        .message_delivery_proof_anchor(&later)
        .expect("the anchor reads")
        .expect("a delivered issuance is the anchor");
    assert_eq!(
        anchor.message_id, first.message_id,
        "an equal instant must not displace the first delivered anchor"
    );
}
