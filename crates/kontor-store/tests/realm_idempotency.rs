//! Realm-scoped idempotency bindings, and the one operation v135 widened the
//! closed set with (ASMA-7968 / GAP-D).

use kontor_core::id::{ContentHash, parse_utc_timestamp};
use kontor_store::{Applied, IdempotencyBinding, SqliteStore};
use tempfile::TempDir;

fn store() -> (TempDir, SqliteStore) {
    let directory = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");
    (directory, store)
}

fn binding(key: &str, operation: &'static str, document: &[u8]) -> IdempotencyBinding {
    IdempotencyBinding {
        key: key.to_owned(),
        operation,
        fingerprint: ContentHash::of(document),
        bound_at: parse_utc_timestamp("2026-10-02T09:00:00Z").expect("an instant"),
    }
}

#[test]
fn the_provider_marker_operation_is_bindable_and_a_replay_is_the_same_act() {
    let (_directory, store) = store();
    let first = binding("marker-1", "clear_provider_marker", b"first");
    assert_eq!(
        store
            .realm_idempotency_binding("marker-1")
            .expect("the table reads"),
        None
    );
    assert_eq!(
        store
            .bind_realm_operation(&first)
            .expect("the widened check accepts the provider-marker operation"),
        Applied::Created
    );
    let recorded = store
        .realm_idempotency_binding("marker-1")
        .expect("the table reads")
        .expect("the binding is durable");
    assert_eq!(recorded.operation, "clear_provider_marker");
    assert_eq!(recorded.fingerprint, first.fingerprint.as_str());

    assert_eq!(
        store
            .bind_realm_operation(&first)
            .expect("the identical binding is the same act"),
        Applied::Unchanged
    );
    let conflicting = binding("marker-1", "clear_provider_marker", b"second");
    assert!(
        store.bind_realm_operation(&conflicting).is_err(),
        "the same key may not bind a different operation"
    );
}

#[test]
fn an_operation_the_closed_set_does_not_name_is_refused_at_the_boundary() {
    let (_directory, store) = store();
    let refusal = store.bind_realm_operation(&binding("marker-2", "invented_operation", b"first"));
    assert!(
        refusal.is_err(),
        "a value the code cannot interpret is not a row the schema accepts"
    );
}
