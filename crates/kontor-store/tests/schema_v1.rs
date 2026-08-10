//! Migration, connection and schema-shape verification.
//!
//! These tests use a file-backed database throughout. WAL is a file property but
//! `foreign_keys` and `busy_timeout` are not: they are per-connection, so an
//! `:memory:` database would prove nothing about a reopened file.
//!
//! The mutants this suite exists to kill:
//!
//! * applying the migration outside a transaction, so a failure leaves half a
//!   schema behind;
//! * opening a database written by a newer binary;
//! * forgetting to re-apply the connection pragmas on reopen;
//! * turning an append-only table into an updatable one, or letting direct SQL
//!   reopen a terminal run.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use kontor_store::{SCHEMA_VERSION, SqliteStore, StoreError};
use rusqlite::{Connection, TransactionBehavior};
use tempfile::TempDir;

/// Every table schema v1 owns. The list is spelled out so that adding or
/// removing one is a deliberate, reviewed change.
const EXPECTED_TABLES: &[&str] = &[
    "account_profiles",
    "agent_runs",
    "calendar_exceptions",
    "calendar_profiles",
    "command_outbox",
    "command_receipts",
    "command_targets",
    "context_packs",
    "execution_authorization_tasks",
    "execution_authorizations",
    "external_comments",
    "external_ticket_observations",
    "external_workflow_specs",
    "guardrail_evaluations",
    "handoffs",
    "holiday_sources",
    "intake_receipts",
    "jira_links",
    "mini_projects",
    "persona_scenarios",
    "projects",
    "realm_metadata",
    "resource_leases",
    "runtime_bindings",
    "runtime_events",
    "runtime_reconciliation_epochs",
    "schedule_overrides",
    "source_events",
    "status_conflicts",
    "status_transition_receipts",
    "task_dependencies",
    "task_gate_evaluations",
    "task_persona_snapshots",
    "task_workflows",
    "tasks",
    "team_runs",
    "team_templates",
    "ticket_field_specs",
    "ticket_sync_projections",
    "trigger_specs",
    "work_calendars",
    "work_profiles",
];

/// A minimal project → task → workflow → team run → agent run chain, inserted
/// with direct SQL so the schema's own constraints are what is under test.
const RUN_FIXTURE: &str = "\
INSERT INTO projects (id, name, root_path, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1, '2026-08-09T10:00:00Z'); \
INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at) \
VALUES ('0193f000-0000-7000-8000-000000000010', '0193f000-0000-7000-8000-000000000001', \
        'T', 'in_progress', 1, '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z'); \
INSERT INTO team_templates (project_id, template_id, version, name, definition, \
        definition_hash, role_authority, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000001', '0193f000-0000-7000-8000-000000000020', 1, \
        'Team', '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '[]', \
        '2026-08-09T10:00:00Z'); \
INSERT INTO work_profiles (project_id, profile_key, version, definition, definition_hash, \
        created_at) \
VALUES ('0193f000-0000-7000-8000-000000000001', 'q7.delivery', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
        '2026-08-09T10:00:00Z'); \
INSERT INTO task_workflows (id, project_id, task_id, profile_key, profile_version, snapshot, \
        snapshot_hash, current_phase, active, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000030', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000010', 'q7.delivery', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'q7.capture', 1, \
        1, '2026-08-09T10:00:00Z'); \
INSERT INTO team_runs (id, project_id, task_id, template_id, template_version, snapshot, \
        snapshot_hash, lifecycle, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000035', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000010', '0193f000-0000-7000-8000-000000000020', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'running', 1, \
        '2026-08-09T10:00:00Z'); \
INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle, desired_state, \
        observed_state, derived_state, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000040', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000035', 'maker.primary', 'running', 'run_requested', \
        'running', 'confirmed', 1, '2026-08-09T10:00:00Z');";

fn temp() -> TempDir {
    TempDir::new().expect("a temporary directory")
}

fn open(directory: &TempDir) -> SqliteStore {
    SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens and migrates")
}

fn raw(directory: &TempDir) -> Connection {
    let connection =
        Connection::open(directory.path().join("kontor.db")).expect("a raw connection opens");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys can be enabled");
    connection
}

fn table_names(connection: &Connection) -> BTreeSet<String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("the catalogue is readable");
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("the catalogue is readable");
    names.map(|name| name.expect("a table name")).collect()
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

#[test]
fn an_empty_database_migrates_to_schema_version_one() {
    let directory = temp();
    let store = open(&directory);
    assert_eq!(
        store.schema_version().expect("the version is readable"),
        SCHEMA_VERSION
    );
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn every_connection_reports_wal_foreign_keys_and_a_bounded_busy_timeout() {
    let directory = temp();
    let store = open(&directory);
    assert_eq!(
        store.journal_mode().expect("readable").to_lowercase(),
        "wal"
    );
    assert!(store.foreign_keys_enabled().expect("readable"));
    assert_eq!(store.busy_timeout_ms().expect("readable"), 5_000);

    // Reopening must re-apply the per-connection pragmas, not inherit them.
    drop(store);
    let reopened = open(&directory);
    assert!(
        reopened.foreign_keys_enabled().expect("readable"),
        "foreign keys must be re-enabled on every connection"
    );
    assert_eq!(reopened.busy_timeout_ms().expect("readable"), 5_000);
    assert_eq!(
        reopened.journal_mode().expect("readable").to_lowercase(),
        "wal"
    );
}

#[test]
fn opening_an_already_migrated_database_is_idempotent() {
    let directory = temp();
    let first = open(&directory);
    let before = table_names(&raw(&directory));
    drop(first);

    let second = open(&directory);
    assert_eq!(second.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(table_names(&raw(&directory)), before);
}

#[test]
fn a_failing_migration_leaves_version_zero_and_no_partial_schema() {
    let directory = temp();
    let path = directory.path().join("kontor.db");

    // Seed a conflicting object so `CREATE TABLE projects` inside the migration
    // fails part-way through the batch.
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .execute_batch("CREATE TABLE projects (unrelated TEXT);")
            .expect("the conflicting table is created");
    }

    let error = SqliteStore::open(&path).expect_err("the migration must fail");
    assert!(
        matches!(error, StoreError::Sqlite(_)),
        "expected a SQLite failure, got {error:?}"
    );

    let connection = Connection::open(&path).expect("a raw connection opens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version is readable");
    assert_eq!(version, 0, "a failed migration must not bump the version");

    let tables = table_names(&connection);
    assert_eq!(
        tables.len(),
        1,
        "only the pre-existing table may remain, found {tables:?}"
    );
    for table in EXPECTED_TABLES {
        if *table == "projects" {
            continue;
        }
        assert!(
            !tables.contains(*table),
            "`{table}` must have been rolled back"
        );
    }
    let triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger'",
            [],
            |row| row.get(0),
        )
        .expect("the catalogue is readable");
    assert_eq!(
        triggers, 0,
        "no trigger may survive a rolled-back migration"
    );
}

#[test]
fn a_newer_schema_is_refused_rather_than_downgraded() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    {
        let store = SqliteStore::open(&path).expect("the store migrates");
        assert_eq!(store.schema_version().expect("readable"), 1);
    }
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .pragma_update(None, "user_version", 2_i64)
            .expect("the version can be forced forward");
    }

    let error = SqliteStore::open(&path).expect_err("a newer schema must be refused");
    match error {
        StoreError::DatabaseTooNew { found, expected } => {
            assert_eq!(found, 2);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        other => panic!("expected DatabaseTooNew, got {other:?}"),
    }

    // Nothing was truncated on the way out.
    let connection = Connection::open(&path).expect("a raw connection opens");
    assert!(table_names(&connection).contains("projects"));
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(version, 2, "a refused open must not rewrite the version");
}

// ---------------------------------------------------------------------------
// Realm identity
// ---------------------------------------------------------------------------

#[test]
fn an_empty_database_creates_exactly_one_immutable_realm() {
    let directory = temp();
    let store = open(&directory);

    let realm = store.realm_metadata();
    assert_eq!(realm.schema_version.get(), 1);
    assert!(
        realm.display_label.is_none(),
        "a freshly initialized realm carries no label"
    );
    assert_eq!(realm.realm_id.as_uuid().get_version_num(), 7);
    // The identity is stable for the lifetime of the store.
    assert_eq!(store.realm_metadata(), realm);
    assert_eq!(store.realm_id(), realm.realm_id);

    let connection = raw(&directory);
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(rows, 1, "exactly one realm row");
    let (singleton, stored_id): (i64, String) = connection
        .query_row(
            "SELECT singleton, realm_id FROM realm_metadata",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("readable");
    assert_eq!(singleton, 1);
    assert_eq!(stored_id, realm.realm_id.to_string());

    // Two separate databases are two separate Realms.
    let other = temp();
    let other_store = open(&other);
    assert_ne!(
        other_store.realm_id(),
        store.realm_id(),
        "each database file is its own realm"
    );
}

#[test]
fn realm_identity_survives_reopen_and_cannot_be_replaced() {
    let directory = temp();
    let original = {
        let store = open(&directory);
        store.realm_metadata().clone()
    };

    for _ in 0..3 {
        let reopened = open(&directory);
        assert_eq!(
            reopened.realm_metadata(),
            &original,
            "reopening must load the same realm byte-for-byte, never regenerate it"
        );
    }

    // Even after the schema is already at v1, nothing re-runs initialization.
    let connection = raw(&directory);
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(rows, 1);
}

#[test]
fn realm_metadata_rejects_update_delete_and_duplicate() {
    let directory = temp();
    let store = open(&directory);
    let original = store.realm_metadata().clone();
    drop(store);
    let connection = raw(&directory);

    for statement in [
        "UPDATE realm_metadata SET realm_id = '0193f000-0000-7000-8000-0000000000ff'",
        "UPDATE realm_metadata SET display_label = 'renamed'",
        "UPDATE realm_metadata SET created_at = '2020-01-01T00:00:00Z'",
        "UPDATE realm_metadata SET schema_version = 1",
        "DELETE FROM realm_metadata",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "realm identity must refuse: {statement}"
        );
    }

    // A second row is impossible: the singleton primary key already holds 1, and
    // any other value fails its check.
    for singleton in ["1", "2"] {
        assert!(
            connection
                .execute(
                    &format!(
                        "INSERT INTO realm_metadata
                             (singleton, realm_id, schema_version, created_at, display_label)
                         VALUES ({singleton}, '0193f000-0000-7000-8000-0000000000fe', 1,
                                 '2026-08-09T10:00:00Z', NULL)"
                    ),
                    [],
                )
                .is_err(),
            "a second realm row must be impossible (singleton {singleton})"
        );
    }
    // An upsert is not a loophole either.
    assert!(
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, '0193f000-0000-7000-8000-0000000000fd', 1, '2026-08-09T10:00:00Z', NULL)
                 ON CONFLICT(singleton) DO UPDATE SET realm_id = excluded.realm_id",
                [],
            )
            .is_err(),
        "an upsert must not replace realm identity"
    );

    // Nothing above changed anything.
    let reopened = open(&directory);
    assert_eq!(reopened.realm_metadata(), &original);
}

// ---------------------------------------------------------------------------
// Contention
// ---------------------------------------------------------------------------

#[test]
fn a_busy_writer_waits_then_times_out_without_partial_state() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    let _store = SqliteStore::open(&path).expect("the store opens");

    let insert = |connection: &Connection, id: &str, root: &str| {
        connection.execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, 'P', ?2, 1, '2026-08-09T10:00:00Z')",
            rusqlite::params![id, root],
        )
    };

    // A writer that releases well inside the timeout is simply waited for.
    {
        let writer = Connection::open(&path).expect("a raw connection opens");
        writer
            .busy_timeout(Duration::from_millis(5_000))
            .expect("timeout applies");

        // The holder owns its whole connection inside the thread: a rusqlite
        // transaction is not `Send`.
        let (locked, is_locked) = std::sync::mpsc::channel();
        let holder_path = path.clone();
        let released = std::thread::spawn(move || {
            let mut holder = Connection::open(&holder_path).expect("a raw connection opens");
            holder
                .busy_timeout(Duration::from_millis(5_000))
                .expect("timeout applies");
            let transaction = holder
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .expect("the lock is taken");
            locked.send(()).expect("the parent is listening");
            std::thread::sleep(Duration::from_millis(250));
            transaction.commit().expect("the holder releases");
        });
        is_locked.recv().expect("the holder takes the lock first");

        insert(&writer, "0193f000-0000-7000-8000-0000000000a1", "/tmp/a1")
            .expect("a released writer is eventually followed");
        released.join().expect("the holder thread finishes");
    }

    // A writer that holds the lock past the deadline yields a typed busy
    // failure, and the blocked write leaves nothing behind.
    let mut holder = Connection::open(&path).expect("a raw connection opens");
    let writer = Connection::open(&path).expect("a raw connection opens");
    writer
        .busy_timeout(Duration::from_millis(5_000))
        .expect("timeout applies");
    let transaction = holder
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("the lock is taken");
    transaction
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-0000000000b1', 'Holder', '/tmp/b1', 1,
                     '2026-08-09T10:00:00Z')",
            [],
        )
        .expect("the holder writes inside its own transaction");

    let started = Instant::now();
    let blocked = insert(&writer, "0193f000-0000-7000-8000-0000000000a2", "/tmp/a2");
    let waited = started.elapsed();

    let error = blocked.expect_err("a writer held past the deadline must fail");
    let busy = matches!(
        &error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::DatabaseBusy
                || failure.code == rusqlite::ErrorCode::DatabaseLocked
    );
    assert!(busy, "expected a typed busy failure, got {error:?}");
    // Deliberately a conservative lower bound rather than an exact duration.
    assert!(
        waited >= Duration::from_millis(4_000),
        "the writer must actually wait for the timeout, waited {waited:?}"
    );

    // Roll the holder back; neither the blocked write nor the holder's own
    // uncommitted row may survive.
    drop(transaction);
    let reader = Connection::open(&path).expect("a raw connection opens");
    let count: i64 = reader
        .query_row(
            "SELECT count(*) FROM projects WHERE id IN
                 ('0193f000-0000-7000-8000-0000000000a2', '0193f000-0000-7000-8000-0000000000b1')",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(count, 0, "a busy failure must leave no partial state");
}

// ---------------------------------------------------------------------------
// Schema shape
// ---------------------------------------------------------------------------

#[test]
fn the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    let found = table_names(&connection);
    let expected: BTreeSet<String> = EXPECTED_TABLES.iter().map(|t| (*t).to_owned()).collect();
    // `sqlite_sequence` is created by AUTOINCREMENT and is filtered out above.
    assert_eq!(found, expected);

    let mut statement = connection
        .prepare(
            "SELECT name FROM pragma_table_list
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND strict = 0",
        )
        .expect("pragma_table_list is available");
    let lax: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("readable")
        .map(|name| name.expect("a name"))
        .collect();
    assert!(lax.is_empty(), "every table must be STRICT, found {lax:?}");
}

#[test]
fn the_schema_has_no_outbound_comment_representation() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // No table or column anywhere may express an outbound comment. Adding one
    // would have to be a numbered migration, which is exactly the point.
    let mut statement = connection
        .prepare("SELECT name, COALESCE(sql, '') FROM sqlite_master")
        .expect("the catalogue is readable");
    let objects: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("readable")
        .map(|row| row.expect("a catalogue row"))
        .collect();
    for (name, sql) in &objects {
        // Strip comments before scanning: the prose explains the rule, the
        // executable schema must not contain the concept.
        let executable: String = sql
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        assert!(
            !executable.contains("outbound"),
            "`{name}` mentions an outbound comment representation"
        );
    }

    // The only comment policy the projection accepts is the inbound one.
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z');
             INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000010',
                     '0193f000-0000-7000-8000-000000000001', 'T', 'draft', 1,
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z');
             INSERT INTO jira_links
                 (id, project_id, task_id, connector, external_issue_key, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000060',
                     '0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000010', 'connector.alpha', 'ABC-1', 1,
                     '2026-08-09T10:00:00Z');",
        )
        .expect("a ticket link inserts");

    for policy in ["outbound", "bidirectional", "inbound_only "] {
        assert!(
            connection
                .execute(
                    "INSERT INTO ticket_sync_projections
                         (id, project_id, link_id, link_revision, connector, external_issue_key,
                          fields, comment_policy, projection_hash, computed_at)
                     VALUES ('0193f000-0000-7000-8000-000000000061',
                             '0193f000-0000-7000-8000-000000000001',
                             '0193f000-0000-7000-8000-000000000060', 1, 'connector.alpha',
                             'ABC-1', '[]', ?1,
                             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                             '2026-08-09T10:00:00Z')",
                    rusqlite::params![policy],
                )
                .is_err(),
            "`{policy}` must not be a storable comment policy"
        );
    }
}

#[test]
fn the_integrity_and_foreign_key_checks_pass_on_a_fresh_database() {
    let directory = temp();
    let store = open(&directory);
    store.integrity_check().expect("integrity_check passes");
    store.quick_check().expect("quick_check passes");
    store.foreign_key_check().expect("foreign_key_check passes");
}

#[test]
fn strict_typing_and_check_constraints_reject_impossible_rows() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // A STRICT table refuses a value of the wrong storage class.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 'not-a-number', ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "P",
                    "/tmp/p",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err(),
        "STRICT must refuse a text revision"
    );

    // A revision below one is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "P",
                    "/tmp/p",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err()
    );

    // A non-canonical timestamp is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, '2026-08-09 10:00:00')",
                rusqlite::params!["0193f000-0000-7000-8000-000000000001", "P", "/tmp/p"],
            )
            .is_err()
    );

    connection
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000001",
                "P",
                "/tmp/p",
                "2026-08-09T10:00:00Z"
            ],
        )
        .expect("a well-formed project inserts");

    // A duplicate root path is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000002",
                    "Q",
                    "/tmp/p",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err()
    );

    // An unknown task state is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
                 VALUES (?1, ?2, 'T', 'almost_done', 1, ?3, ?3)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000010",
                    "0193f000-0000-7000-8000-000000000001",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err()
    );

    // A dangling parent is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
                 VALUES (?1, ?2, 'T', 'draft', 1, ?3, ?3)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000011",
                    "0193f000-0000-7000-8000-0000000000ff",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err(),
        "foreign keys must be enforced"
    );
}

#[test]
fn a_task_may_not_depend_on_itself() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z');
             INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000010',
                     '0193f000-0000-7000-8000-000000000001', 'T', 'draft', 1,
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z');",
        )
        .expect("the fixture rows insert");

    assert!(
        connection
            .execute(
                "INSERT INTO task_dependencies
                     (project_id, task_id, depends_on_task_id, created_at)
                 VALUES (?1, ?2, ?2, ?3)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "0193f000-0000-7000-8000-000000000010",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err(),
        "a self dependency must be impossible in SQL as well as in Rust"
    );
}

#[test]
fn append_only_tables_reject_update_and_delete_from_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z');
             INSERT INTO work_profiles
                 (project_id, profile_key, version, definition, definition_hash, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'q7.delivery', 1, '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     '2026-08-09T10:00:00Z');",
        )
        .expect("a specification revision inserts");

    assert!(
        connection
            .execute(
                "UPDATE work_profiles SET definition = '{\"a\":1}' WHERE profile_key = 'q7.delivery'",
                [],
            )
            .is_err(),
        "an immutable revision must refuse UPDATE"
    );
    assert!(
        connection
            .execute(
                "DELETE FROM work_profiles WHERE profile_key = 'q7.delivery'",
                []
            )
            .is_err(),
        "an immutable revision must refuse DELETE"
    );

    // A duplicate (id, version) is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO work_profiles
                     (project_id, profile_key, version, definition, definition_hash, created_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'q7.delivery', 1, '{}',
                         'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                         '2026-08-09T10:00:00Z')",
                [],
            )
            .is_err()
    );
}

#[test]
fn the_runtime_event_cursor_is_monotonic_and_never_reused() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    let insert = |native: &str| -> i64 {
        connection
            .execute(
                "INSERT INTO runtime_events
                     (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                      native_id, native_event_id, native_sequence, payload, payload_hash,
                      observed_at, recorded_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                         '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                         'session-abc', ?1, 1, '{}',
                         'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                         '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
                rusqlite::params![native],
            )
            .expect("an event appends");
        connection.last_insert_rowid()
    };

    let first = insert("n-1");
    let second = insert("n-2");
    assert!(second > first);

    // The same native event id inside the same generation is a duplicate.
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_events
                     (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                      native_id, native_event_id, native_sequence, payload, payload_hash,
                      observed_at, recorded_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                         '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                         'session-abc', 'n-1', 2, '{}',
                         'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
                         '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
                [],
            )
            .is_err()
    );

    assert!(
        connection
            .execute(
                "DELETE FROM runtime_events WHERE cursor = ?1",
                rusqlite::params![first]
            )
            .is_err(),
        "the event log is append-only"
    );
    assert!(
        connection
            .execute(
                "UPDATE runtime_events SET payload = '{}' WHERE cursor = ?1",
                rusqlite::params![first]
            )
            .is_err()
    );

    // After a generation change the same native event id is a different event.
    let third = connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                  native_id, native_event_id, native_sequence, payload, payload_hash,
                  observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 2,
                     'session-abc', 'n-1', 3, '{}',
                     'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            [],
        )
        .map(|_| connection.last_insert_rowid())
        .expect("a new generation is a new event");
    assert!(third > second);
}

#[test]
fn a_terminal_run_cannot_be_reopened_or_edited_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    // A terminal lifecycle without evidence is impossible.
    assert!(
        connection
            .execute(
                "UPDATE agent_runs SET lifecycle = 'succeeded'
                 WHERE id = '0193f000-0000-7000-8000-000000000040'",
                [],
            )
            .is_err(),
        "closing a run without evidence must be impossible"
    );

    connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation, native_id,
                  native_event_id, native_sequence, observed_state, payload, payload_hash,
                  observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                     'session-abc', 'n-close', 9, 'succeeded', '{}',
                     'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            [],
        )
        .expect("the terminal event inserts");
    let cursor = connection.last_insert_rowid();
    connection
        .execute(
            "UPDATE agent_runs
             SET lifecycle = 'succeeded', derived_state = 'terminal',
                 terminal_outcome = 'succeeded', terminal_source_kind = 'runtime_observation',
                 terminal_event_cursor = ?1,
                 terminal_evidence_hash =
                     'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                 closed_at = '2026-08-09T11:00:00Z'
             WHERE id = '0193f000-0000-7000-8000-000000000040'",
            rusqlite::params![cursor],
        )
        .expect("an evidenced closure succeeds");

    for statement in [
        "UPDATE agent_runs SET lifecycle = 'running' WHERE id = '0193f000-0000-7000-8000-000000000040'",
        "UPDATE agent_runs SET terminal_evidence_hash = 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff' WHERE id = '0193f000-0000-7000-8000-000000000040'",
        "UPDATE agent_runs SET closed_at = NULL WHERE id = '0193f000-0000-7000-8000-000000000040'",
        "DELETE FROM agent_runs WHERE id = '0193f000-0000-7000-8000-000000000040'",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "a closed run must refuse: {statement}"
        );
    }
}

#[test]
fn a_terminal_team_run_cannot_be_reopened_or_edited_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    const TEAM_RUN: &str = "0193f000-0000-7000-8000-000000000035";

    // A terminal lifecycle without a closure time and evidence is impossible.
    assert!(
        connection
            .execute(
                "UPDATE team_runs SET lifecycle = 'succeeded' WHERE id = ?1",
                rusqlite::params![TEAM_RUN],
            )
            .is_err(),
        "closing a team run without evidence must be impossible"
    );
    assert!(
        connection
            .execute(
                "UPDATE team_runs SET lifecycle = 'succeeded', closed_at = '2026-08-09T11:00:00Z'
                 WHERE id = ?1",
                rusqlite::params![TEAM_RUN],
            )
            .is_err(),
        "a closure time alone is not evidence"
    );

    connection
        .execute(
            "UPDATE team_runs
             SET lifecycle = 'succeeded', terminal_outcome = 'succeeded',
                 terminal_source_kind = 'child_evidence',
                 terminal_evidence_hash =
                     'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                 closed_at = '2026-08-09T11:00:00Z'
             WHERE id = ?1",
            rusqlite::params![TEAM_RUN],
        )
        .expect("an evidenced closure succeeds");

    for statement in [
        "UPDATE team_runs SET lifecycle = 'running' WHERE id = ?1",
        "UPDATE team_runs SET lifecycle = 'queued' WHERE id = ?1",
        "UPDATE team_runs SET terminal_evidence_hash = 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff' WHERE id = ?1",
        "UPDATE team_runs SET terminal_outcome = 'failed' WHERE id = ?1",
        "UPDATE team_runs SET closed_at = NULL WHERE id = ?1",
        "UPDATE team_runs SET revision = revision + 1 WHERE id = ?1",
        "DELETE FROM team_runs WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TEAM_RUN])
                .is_err(),
            "a closed team run must refuse: {statement}"
        );
    }
}

#[test]
fn a_pinned_snapshot_cannot_be_rewritten_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    const WORKFLOW: &str = "0193f000-0000-7000-8000-000000000030";
    const TEAM_RUN: &str = "0193f000-0000-7000-8000-000000000035";

    // The work-profile snapshot a task is running is frozen.
    for statement in [
        "UPDATE task_workflows SET snapshot = '{\"a\":1}' WHERE id = ?1",
        "UPDATE task_workflows SET snapshot_hash =
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' WHERE id = ?1",
        "UPDATE task_workflows SET profile_key = 'other.profile' WHERE id = ?1",
        "UPDATE task_workflows SET profile_version = 2 WHERE id = ?1",
        "DELETE FROM task_workflows WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![WORKFLOW])
                .is_err(),
            "a pinned work profile snapshot must refuse: {statement}"
        );
    }

    // Advancing the phase and the revision is exactly what a live workflow is
    // allowed to do, so the trigger is not simply blocking every update.
    connection
        .execute(
            "UPDATE task_workflows SET current_phase = 'q7.shape', revision = revision + 1
             WHERE id = ?1",
            rusqlite::params![WORKFLOW],
        )
        .expect("a live workflow may advance");

    // The team definition a run started with is frozen the same way, and it is
    // frozen while the run is still open — not only once it has closed.
    for statement in [
        "UPDATE team_runs SET snapshot = '{\"a\":1}' WHERE id = ?1",
        "UPDATE team_runs SET snapshot_hash =
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' WHERE id = ?1",
        "UPDATE team_runs SET template_version = 2 WHERE id = ?1",
        "UPDATE team_runs SET task_id = '0193f000-0000-7000-8000-0000000000ee' WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TEAM_RUN])
                .is_err(),
            "a pinned team snapshot must refuse: {statement}"
        );
    }

    // A still-open team run may still move its lifecycle forward.
    connection
        .execute(
            "UPDATE team_runs SET lifecycle = 'waiting_input' WHERE id = ?1",
            rusqlite::params![TEAM_RUN],
        )
        .expect("an open team run may change lifecycle");

    // The persona snapshot table is immutable outright.
    connection
        .execute_batch(
            "INSERT INTO persona_scenarios
                 (project_id, scenario_id, version, persona_key, gate_key, definition,
                  definition_hash, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000050', 1, 'persona.x', 'zz.gate', '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     '2026-08-09T10:00:00Z');
             INSERT INTO task_persona_snapshots
                 (project_id, task_id, scenario_id, version, workflow_id, gate_key, snapshot,
                  snapshot_hash, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000010',
                     '0193f000-0000-7000-8000-000000000050', 1,
                     '0193f000-0000-7000-8000-000000000030', 'zz.gate', '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     '2026-08-09T10:00:00Z');",
        )
        .expect("a persona snapshot inserts");
    assert!(
        connection
            .execute(
                "UPDATE task_persona_snapshots SET snapshot = '{\"a\":1}'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM task_persona_snapshots", [])
            .is_err()
    );
}

#[test]
fn a_terminal_task_cannot_be_changed_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    const TASK: &str = "0193f000-0000-7000-8000-000000000010";

    // An open task moves freely.
    connection
        .execute(
            "UPDATE tasks SET state = 'blocked', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("an open task may change state");

    connection
        .execute(
            "UPDATE tasks SET state = 'done', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("a task may close");

    for statement in [
        "UPDATE tasks SET state = 'ready' WHERE id = ?1",
        "UPDATE tasks SET state = 'in_progress' WHERE id = ?1",
        "UPDATE tasks SET state = 'cancelled' WHERE id = ?1",
        "UPDATE tasks SET title = 'renamed' WHERE id = ?1",
        "UPDATE tasks SET revision = revision + 1 WHERE id = ?1",
        "DELETE FROM tasks WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TASK])
                .is_err(),
            "a terminal task must refuse: {statement}"
        );
    }
}

#[test]
fn a_derived_state_may_only_be_terminal_together_with_an_outcome() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    // Uncertainty is representable and never terminal.
    for uncertain in [
        "pending_confirmation",
        "stale",
        "diverged",
        "runtime_unavailable",
        "orphaned",
        "lost_contact",
    ] {
        connection
            .execute(
                "UPDATE agent_runs SET derived_state = ?1
                 WHERE id = '0193f000-0000-7000-8000-000000000040'",
                [uncertain],
            )
            .unwrap_or_else(|_| panic!("`{uncertain}` must be storable"));
    }

    // `terminal` without an outcome is impossible.
    assert!(
        connection
            .execute(
                "UPDATE agent_runs SET derived_state = 'terminal'
                 WHERE id = '0193f000-0000-7000-8000-000000000040'",
                [],
            )
            .is_err(),
        "a derived terminal state requires an outcome"
    );
}

#[test]
fn only_one_active_workflow_and_one_active_calendar_may_exist() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    assert!(
        connection
            .execute(
                "INSERT INTO task_workflows
                     (id, project_id, task_id, profile_key, profile_version, snapshot,
                      snapshot_hash, current_phase, active, revision, created_at)
                 VALUES ('0193f000-0000-7000-8000-000000000031',
                         '0193f000-0000-7000-8000-000000000001',
                         '0193f000-0000-7000-8000-000000000010', 'q7.delivery', 1, '{}',
                         'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                         'q7.capture', 1, 1, '2026-08-09T10:00:00Z')",
                [],
            )
            .is_err(),
        "a task may have only one active workflow"
    );
}

#[test]
fn all_logical_relationships_are_project_scoped_and_fk_backed() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // Every logical relationship the plan names, spelled out: source table,
    // source columns, target table, target columns.
    //
    // This is an enumeration on purpose. A count ("at least N composite keys")
    // passes just as happily when the wrong N keys are present, so removing one
    // required relationship and adding an unrelated one would go unnoticed. Here
    // a missing key names itself.
    //
    // Order within a key matters and is asserted, because
    // `(project_id, task_id) -> tasks(project_id, id)` and a key that happens to
    // mention the same two columns in another order are not the same constraint.
    type Relationship = (
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static [&'static str],
    );
    const REQUIRED: &[Relationship] = &[
        // --- structure -----------------------------------------------------
        ("mini_projects", &["project_id"], "projects", &["id"]),
        (
            "tasks",
            &["project_id", "mini_project_id"],
            "mini_projects",
            &["project_id", "id"],
        ),
        (
            "task_dependencies",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_dependencies",
            &["project_id", "depends_on_task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        // --- pinned specification revisions --------------------------------
        (
            "task_workflows",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_workflows",
            &["project_id", "profile_key", "profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "task_gate_evaluations",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        (
            "task_gate_evaluations",
            &["project_id", "evaluator_account"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "task_persona_snapshots",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_persona_snapshots",
            &["project_id", "scenario_id", "version"],
            "persona_scenarios",
            &["project_id", "scenario_id", "version"],
        ),
        (
            "task_persona_snapshots",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        // --- trigger pins ---------------------------------------------------
        (
            "trigger_specs",
            &["project_id", "work_profile_key", "work_profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "trigger_specs",
            &["project_id", "team_template_id", "team_template_version"],
            "team_templates",
            &["project_id", "template_id", "version"],
        ),
        (
            "trigger_specs",
            &["calendar_profile_id", "calendar_version"],
            "calendar_profiles",
            &["profile_id", "version"],
        ),
        // --- intake ---------------------------------------------------------
        ("source_events", &["project_id"], "projects", &["id"]),
        (
            "intake_receipts",
            &["project_id", "source_event_id"],
            "source_events",
            &["project_id", "id"],
        ),
        (
            "intake_receipts",
            &["project_id", "trigger_key", "trigger_version"],
            "trigger_specs",
            &["project_id", "trigger_key", "version"],
        ),
        (
            "intake_receipts",
            &["project_id", "predecessor_receipt_id"],
            "intake_receipts",
            &["project_id", "id"],
        ),
        // --- calendar and authorization -------------------------------------
        (
            "work_calendars",
            &["profile_id", "profile_version"],
            "calendar_profiles",
            &["profile_id", "version"],
        ),
        (
            "calendar_exceptions",
            &["project_id", "work_calendar_id"],
            "work_calendars",
            &["project_id", "id"],
        ),
        (
            "execution_authorizations",
            &["project_id", "scope_task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "execution_authorizations",
            &["project_id", "created_by"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "execution_authorizations",
            &["project_id", "capability_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "execution_authorization_tasks",
            &["project_id", "authorization_id"],
            "execution_authorizations",
            &["project_id", "id"],
        ),
        (
            "execution_authorization_tasks",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "approved_by"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "approval_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "revoked_by"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "revocation_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        // --- runs, bindings and events --------------------------------------
        (
            "team_runs",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "team_runs",
            &["project_id", "template_id", "template_version"],
            "team_templates",
            &["project_id", "template_id", "version"],
        ),
        (
            "agent_runs",
            &["project_id", "team_run_id"],
            "team_runs",
            &["project_id", "id"],
        ),
        (
            "agent_runs",
            &["project_id", "parent_agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "agent_runs",
            &["project_id", "terminal_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "runtime_bindings",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_events",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_events",
            &["project_id", "command_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "guardrail_evaluations",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "resource_leases",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "resource_leases",
            &["project_id", "release_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "handoffs",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        (
            "handoffs",
            &["project_id", "context_pack_id"],
            "context_packs",
            &["project_id", "id"],
        ),
        (
            "context_packs",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        // --- external tickets -----------------------------------------------
        (
            "jira_links",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "external_workflow_specs",
            &["project_id", "work_profile_key", "work_profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "ticket_sync_projections",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "ticket_sync_projections",
            &[
                "project_id",
                "connector",
                "field_spec_project",
                "field_spec_issue_type",
                "field_spec_version",
            ],
            "ticket_field_specs",
            &[
                "project_id",
                "connector",
                "external_project",
                "issue_type",
                "version",
            ],
        ),
        (
            "external_comments",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "external_ticket_observations",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "status_transition_receipts",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "status_transition_receipts",
            &["project_id", "prior_observation_id"],
            "external_ticket_observations",
            &["project_id", "id"],
        ),
        (
            "status_conflicts",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "status_conflicts",
            &["project_id", "observation_id"],
            "external_ticket_observations",
            &["project_id", "id"],
        ),
        (
            "status_conflicts",
            &["project_id", "resolution_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        // --- commands --------------------------------------------------------
        (
            "command_outbox",
            &["project_id", "receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_team_run_id"],
            "team_runs",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_ticket_link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_work_calendar_id"],
            "work_calendars",
            &["project_id", "id"],
        ),
    ];

    /// Every foreign key on `table`, as (target table, from-columns, to-columns).
    fn foreign_keys(
        connection: &Connection,
        table: &str,
    ) -> Vec<(String, Vec<String>, Vec<String>)> {
        let mut statement = connection
            .prepare(
                "SELECT id, seq, \"table\", \"from\", \"to\"
                 FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
            )
            .expect("the catalogue is readable");
        let rows: Vec<(i64, String, String, Option<String>)> = statement
            .query_map(rusqlite::params![table], |row| {
                Ok((row.get(0)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .expect("the catalogue is readable")
            .map(|row| row.expect("a foreign key row"))
            .collect();
        let mut grouped: std::collections::BTreeMap<i64, (String, Vec<String>, Vec<String>)> =
            std::collections::BTreeMap::new();
        for (id, target, from, to) in rows {
            let entry = grouped
                .entry(id)
                .or_insert_with(|| (target, Vec::new(), Vec::new()));
            entry.1.push(from);
            // A NULL `to` means the key targets the primary key of the target.
            entry.2.push(to.unwrap_or_default());
        }
        grouped.into_values().collect()
    }

    let mut missing = Vec::new();
    for (table, from, target, to) in REQUIRED {
        let keys = foreign_keys(&connection, table);
        let found = keys.iter().any(|(actual_target, actual_from, actual_to)| {
            actual_target == target
                && actual_from.as_slice() == *from
                && actual_to.as_slice() == *to
        });
        if !found {
            missing.push(format!(
                "{table} ({}) -> {target} ({})",
                from.join(", "),
                to.join(", ")
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "these required relationships are not foreign-key backed:\n  {}",
        missing.join("\n  ")
    );

    // Every relationship above is also *project-scoped*, except the two that
    // genuinely are not: calendar profiles are workspace-level, so their pins
    // carry no `project_id`. Naming the exceptions explicitly means a third one
    // cannot appear by accident.
    const WORKSPACE_LEVEL: &[(&str, &str)] = &[
        ("trigger_specs", "calendar_profiles"),
        ("work_calendars", "calendar_profiles"),
    ];
    for (table, from, target, _to) in REQUIRED {
        // A reference to `projects` *is* the scope; there is nothing to
        // compose it with.
        if WORKSPACE_LEVEL.contains(&(table, target)) || *target == "projects" {
            continue;
        }
        assert_eq!(
            from.first().copied(),
            Some("project_id"),
            "{table} -> {target} must lead with project_id"
        );
        assert!(
            from.len() > 1,
            "{table} -> {target} must be composite: a single-column key would let a \
             globally valid UUID from another project resolve"
        );
    }

    // The two normalization tables the audit required exist and are keyed the
    // way the plan specifies.
    for (table, key) in [
        ("command_targets", "receipt_id"),
        ("execution_authorization_tasks", "authorization_id"),
    ] {
        let present: i64 = connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
                rusqlite::params![table, key],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(present, 1, "`{table}` must key on `{key}`");
    }

    // A command target names exactly one typed id.
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    connection
        .execute_batch(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000070',
                     '0193f000-0000-7000-8000-000000000001', 'k-1', 'resume_task', '{}', 1, '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     'intent_persisted', 0, '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z');",
        )
        .expect("a receipt inserts");

    // Zero typed ids, and two typed ids, are both impossible.
    for tail in [
        "'task', NULL, NULL, NULL, NULL, NULL, NULL, NULL",
        "'task', NULL, NULL, '0193f000-0000-7000-8000-000000000010', \
         '0193f000-0000-7000-8000-000000000035', NULL, NULL, NULL",
    ] {
        assert!(
            connection
                .execute(
                    &format!(
                        "INSERT INTO command_targets
                             (project_id, receipt_id, target_kind, target_project_id,
                              target_mini_project_id, target_task_id, target_team_run_id,
                              target_agent_run_id, target_ticket_link_id, target_work_calendar_id)
                         VALUES ('0193f000-0000-7000-8000-000000000001',
                                 '0193f000-0000-7000-8000-000000000070', {tail})"
                    ),
                    [],
                )
                .is_err(),
            "a command target must name exactly one typed id"
        );
    }
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
