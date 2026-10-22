//! Concurrent first-open verification in its own Cargo integration-test process.
//!
//! Unrelated cold migrations in one process can exhaust the bounded lock wait.
//! Keep this four-opener race separate while retaining every success assertion.

use std::collections::BTreeSet;

use kontor_store::{SCHEMA_VERSION, SqliteStore};
use rusqlite::Connection;
use tempfile::TempDir;

fn temp() -> TempDir {
    TempDir::new().expect("a temporary directory")
}

#[test]
fn a_concurrent_first_open_initializes_exactly_one_realm() {
    // Two processes opening the same brand-new file at once. Only one may run
    // `0001`; the other must wait, notice that the schema now exists, and adopt
    // the Realm the winner created.
    //
    // The mutant this kills is reading `user_version` *before* taking the
    // IMMEDIATE lock and not re-reading after the wait: the loser then replays
    // `0001` against an already-created schema and fails on the first duplicate
    // object, turning a concurrent open into a hard error.
    let directory = temp();
    let path = directory.path().join("kontor.db");

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let openers: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                SqliteStore::open(&path).map(|store| store.realm_id())
            })
        })
        .collect();

    let realms: Vec<_> = openers
        .into_iter()
        .map(|opener| {
            opener
                .join()
                .expect("the opener thread does not panic")
                .expect("every concurrent first open succeeds")
        })
        .collect();

    // All four agree, and the identity is a real one rather than four races
    // each inventing their own.
    let distinct: BTreeSet<_> = realms.iter().collect();
    assert_eq!(
        distinct.len(),
        1,
        "a concurrent first open must not create more than one realm"
    );

    // Exactly one row exists on disk, and reopening still reports the same id.
    let connection = Connection::open(&path).expect("a raw connection opens");
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("the realm table is readable");
    assert_eq!(rows, 1);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version is readable");
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(
        SqliteStore::open(&path)
            .expect("reopening succeeds")
            .realm_id(),
        realms[0],
        "the realm survives the race and the reopen"
    );
}
