//! Operator preflight validates existing identity without initialization/migration.
use kontor_store::SqliteStore;
#[test]
fn realm_preflight_reads_existing_identity_without_changing_database_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kontor.sqlite3");
    let store = SqliteStore::open(&path).unwrap();
    let expected = store.realm_metadata().clone();
    drop(store);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(SqliteStore::read_existing_realm(&path).unwrap(), expected);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn realm_preflight_refuses_absent_empty_or_unsupported_databases_without_repair() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent.sqlite3");
    assert!(SqliteStore::read_existing_realm(&absent).is_err());
    assert!(!absent.exists());
    let empty = root.path().join("empty.sqlite3");
    drop(rusqlite::Connection::open(&empty).unwrap());
    let before = std::fs::read(&empty).unwrap();
    assert!(SqliteStore::read_existing_realm(&empty).is_err());
    assert_eq!(std::fs::read(&empty).unwrap(), before);
    let path = root.path().join("existing.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 0).unwrap();
    drop(connection);
    let before = std::fs::read(&path).unwrap();
    assert!(SqliteStore::read_existing_realm(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

// ---------------------------------------------------------------------------
// ASMA-8187 × ASMA-8015 — the read-only preflight meets a raised schema.
//
// ASMA-8187 takes this binary from schema 119 to 122. The operator preflight
// that installs a credential into a *stopped* Realm opens read-only and refuses
// anything it cannot vouch for, so the question these tests settle is what the joined 124
// binary does when it meets a Realm that stopped at 119: it must refuse, and it
// must not migrate on the way.
// ---------------------------------------------------------------------------

/// The joined schema is the one the preflight accepts.
#[test]
fn realm_preflight_accepts_the_joined_schema_version() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kontor.sqlite3");
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        store.schema_version().unwrap(),
        kontor_store::SCHEMA_VERSION
    );
    assert_eq!(kontor_store::SCHEMA_VERSION, 124);
    let expected = store.realm_metadata().clone();
    drop(store);

    let before = std::fs::read(&path).unwrap();
    assert_eq!(SqliteStore::read_existing_realm(&path).unwrap(), expected);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// A Realm stopped at 119 is refused, and is still at 119 afterwards.
///
/// This is the one interaction the earlier audits could not have covered: the
/// read-only path arrived while this lane was raising the schema. Refusing is
/// the designed behaviour — the preflight exists to decline legacy databases
/// rather than repair them — and the point of the test is that declining stays
/// *read-only*. A binary that quietly migrated here would upgrade a Realm its
/// operator had deliberately stopped, through a command that only meant to read
/// an identity.
#[test]
fn realm_preflight_refuses_a_realm_stopped_before_the_8187_migrations() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kontor.sqlite3");
    drop(SqliteStore::open(&path).unwrap());

    // Put the Realm back where one stopped before 0120 would be: the two
    // upstream tables and the later memory tables are gone; `user_version` says 119.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE memory_projection_rebuild_results;
             DROP TABLE memory_projection_rebuild_keys;
             DROP TABLE memory_recall_keys;
             DROP TABLE memory_recall_metadata;
             DROP TABLE memory_experience_proposals;
             DROP TABLE memory_projection_active;
             DROP TABLE memory_projection_snapshots;
             DROP TABLE memory_experience_eligibility;
             DROP TABLE imported_record_evidence;
             DROP TABLE core_team_route_successions;
             PRAGMA user_version = 119;",
        )
        .unwrap();
    drop(connection);

    let before = std::fs::read(&path).unwrap();
    assert!(
        SqliteStore::read_existing_realm(&path).is_err(),
        "a 124 binary vouched for a Realm stopped at 119"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the read-only preflight changed a stopped Realm's bytes"
    );

    // And specifically: it did not migrate.
    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        version, 119,
        "the read-only preflight migrated a stopped Realm"
    );
    let readded: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
              WHERE type = 'table' AND name = 'core_team_route_successions'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        readded, 0,
        "the read-only preflight recreated a migration's table"
    );
}

/// A Realm newer than this binary is refused the same way.
#[test]
fn realm_preflight_refuses_a_newer_schema_without_touching_it() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kontor.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "user_version", kontor_store::SCHEMA_VERSION + 1)
        .unwrap();
    drop(connection);

    let before = std::fs::read(&path).unwrap();
    assert!(SqliteStore::read_existing_realm(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
