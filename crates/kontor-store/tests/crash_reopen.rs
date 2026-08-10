//! Durability across an abrupt process exit.
//!
//! A child process is re-invoked from this same test binary, writes one
//! committed row and one uncommitted one, and then calls `std::process::abort`.
//! Nothing is closed, nothing is checkpointed and no destructor runs — which is
//! exactly the situation a killed daemon leaves behind.
//!
//! On reopen: the committed row must be there, the uncommitted one must not, and
//! `integrity_check`, `quick_check` and `foreign_key_check` must all pass.

use std::path::Path;
use std::process::Command;

use kontor_core::id::{ExternalName, ProjectId, Timestamp, parse_utc_timestamp};
use kontor_core::repository::{NewProject, ProjectRepository};
use kontor_store::SqliteStore;
use rusqlite::Connection;
use tempfile::TempDir;

/// Set by the parent to tell the child which database to write to and destroy.
const CHILD_ENV: &str = "KONTOR_CRASH_REOPEN_DB";
/// Set by the parent to tell the child to abort *inside* the v1 migration.
const MIGRATION_CHILD_ENV: &str = "KONTOR_CRASH_MIGRATION_DB";

/// The exact schema this store applies, so the child interrupts the real thing
/// rather than a stand-in.
const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");

/// Committed before the abort: must survive.
const COMMITTED: &str = "0193f000-0000-7000-8000-0000000000c1";
/// Written inside an open transaction that is never committed: must not survive.
const UNCOMMITTED: &str = "0193f000-0000-7000-8000-0000000000c2";

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

/// The child half of the crash test.
///
/// In an ordinary test run the environment variable is absent and this is a
/// no-op; the parent re-invokes the test binary with it set.
#[test]
fn crash_child_worker() {
    let Ok(path) = std::env::var(CHILD_ENV) else {
        return;
    };
    let path = Path::new(&path);

    let store = SqliteStore::open(path).expect("the child migrates the database");
    store
        .create_project(&NewProject {
            id: ProjectId::parse(COMMITTED).expect("a canonical id"),
            name: ExternalName::parse("Committed").expect("a valid name"),
            root_path: ExternalName::parse("/tmp/committed").expect("a valid path"),
            created_at: at("2026-08-09T10:00:00Z"),
        })
        .expect("the committed project is written");

    // A second connection opens a write transaction and never commits it.
    let connection = Connection::open(path).expect("a raw connection opens");
    connection
        .busy_timeout(std::time::Duration::from_millis(5_000))
        .expect("the busy timeout applies");
    connection
        .execute_batch(&format!(
            "BEGIN IMMEDIATE;
             INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('{UNCOMMITTED}', 'Uncommitted', '/tmp/uncommitted', 1,
                     '2026-08-09T10:00:00Z');"
        ))
        .expect("the uncommitted row is staged");

    // No unwinding, no destructors, no checkpoint: the process simply stops.
    std::process::abort();
}

#[test]
fn committed_work_survives_an_abrupt_exit_and_an_interrupted_transaction_does_not() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");

    let status = Command::new(std::env::current_exe().expect("the test binary path"))
        .args(["crash_child_worker", "--exact", "--nocapture"])
        .env(CHILD_ENV, &path)
        .status()
        .expect("the child process runs");
    assert!(
        !status.success(),
        "the child must die abruptly, not exit cleanly"
    );

    // Nothing was checkpointed on the way out: the write-ahead log is still
    // there, and reopening must replay it.
    let wal = directory.path().join("kontor.db-wal");
    assert!(
        wal.exists(),
        "an aborted process cannot have checkpointed the WAL"
    );

    let store = SqliteStore::open(&path).expect("the database reopens");
    assert_eq!(store.schema_version().expect("readable"), 1);
    assert!(
        store
            .get_project(ProjectId::parse(COMMITTED).expect("a canonical id"))
            .expect("the read succeeds")
            .is_some(),
        "committed work must survive an abrupt exit"
    );
    assert!(
        store
            .get_project(ProjectId::parse(UNCOMMITTED).expect("a canonical id"))
            .expect("the read succeeds")
            .is_none(),
        "an interrupted transaction must leave nothing behind"
    );

    store.integrity_check().expect("integrity_check passes");
    store.quick_check().expect("quick_check passes");
    store.foreign_key_check().expect("foreign_key_check passes");
    assert!(store.foreign_keys_enabled().expect("readable"));
    assert_eq!(store.busy_timeout_ms().expect("readable"), 5_000);
    assert_eq!(
        store.journal_mode().expect("readable").to_lowercase(),
        "wal"
    );
}

#[test]
fn two_connections_may_share_one_file() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");

    let first = SqliteStore::open(&path).expect("the first connection opens");
    let second = SqliteStore::open(&path).expect("the second connection opens");
    assert_eq!(second.schema_version().expect("readable"), 1);
    assert!(
        second.foreign_keys_enabled().expect("readable"),
        "the second connection must enforce references too"
    );

    let id = ProjectId::generate();
    first
        .create_project(&NewProject {
            id,
            name: ExternalName::parse("Shared").expect("a valid name"),
            root_path: ExternalName::parse("/tmp/shared").expect("a valid path"),
            created_at: at("2026-08-09T10:00:00Z"),
        })
        .expect("the first connection writes");

    assert!(
        second.get_project(id).expect("the read succeeds").is_some(),
        "a committed write is visible to the other connection"
    );
    second.integrity_check().expect("integrity_check passes");
}

/// The child half of the interrupted-migration test.
#[test]
fn migration_crash_child_worker() {
    let Ok(path) = std::env::var(MIGRATION_CHILD_ENV) else {
        return;
    };
    let mut connection = Connection::open(&path).expect("a raw connection opens");
    connection
        .busy_timeout(std::time::Duration::from_millis(5_000))
        .expect("the busy timeout applies");
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .expect("WAL applies");

    // Execute the exact v1 migration inside an IMMEDIATE transaction — every
    // table, index and trigger, and `PRAGMA user_version = 1` — and then die
    // before committing.
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("the migration transaction opens");
    transaction
        .execute_batch(MIGRATION_0001)
        .expect("the migration batch runs");
    transaction
        .execute(
            "INSERT INTO realm_metadata
                 (singleton, realm_id, schema_version, created_at, display_label)
             VALUES (1, '0193f000-0000-7000-8000-0000000000d1', 1, '2026-08-09T10:00:00Z', NULL)",
            [],
        )
        .expect("the realm row is staged");

    std::process::abort();
}

#[test]
fn an_interrupted_migration_leaves_no_schema_or_realm_and_reopens_cleanly() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");

    let status = Command::new(std::env::current_exe().expect("the test binary path"))
        .args(["migration_crash_child_worker", "--exact", "--nocapture"])
        .env(MIGRATION_CHILD_ENV, &path)
        .status()
        .expect("the child process runs");
    assert!(!status.success(), "the child must die mid-migration");

    // Nothing survives an uncommitted migration: no version bump, no table, no
    // index, no trigger and — critically — no realm identity.
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("readable");
        assert_eq!(
            version, 0,
            "an interrupted migration must not bump the version"
        );

        let objects: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(objects, 0, "no table, index or trigger may survive");

        let realm_table: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'realm_metadata'",
                [],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(realm_table, 0, "no realm identity may survive");
    }

    // Reopening the wreckage is an ordinary first open: it migrates cleanly and
    // mints a brand-new realm rather than resurrecting the aborted one.
    let store = SqliteStore::open(&path).expect("the database reopens and migrates");
    assert_eq!(store.schema_version().expect("readable"), 1);
    assert_ne!(
        store.realm_id().to_string(),
        "0193f000-0000-7000-8000-0000000000d1",
        "the aborted realm identity must not be resurrected"
    );
    store.integrity_check().expect("integrity_check passes");
    store.quick_check().expect("quick_check passes");
    store.foreign_key_check().expect("foreign_key_check passes");

    // And it is stable from here on.
    let realm = store.realm_metadata().clone();
    drop(store);
    let reopened = SqliteStore::open(&path).expect("the database reopens");
    assert_eq!(reopened.realm_metadata(), &realm);
}
