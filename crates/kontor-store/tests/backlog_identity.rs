//! Durable epic namespaces and derived Jira item-code store behavior.

use kontor_core::backlog_identity::EpicBacklogCode;
use kontor_core::id::{ExternalName, MiniProjectId, ProjectId, Timestamp};
use kontor_core::repository::{NewMiniProject, NewProject, ProjectRepository};
use kontor_store::SqliteStore;
use std::collections::BTreeSet;
use std::sync::{Arc, Barrier};

fn seed_epic(store: &SqliteStore, project_id: ProjectId, name: &str) -> MiniProjectId {
    let epic_id = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: epic_id,
            project_id,
            name: ExternalName::parse(name).expect("epic name"),
            created_at: Timestamp::now(),
        })
        .expect("epic");
    epic_id
}

fn seed_project(store: &SqliteStore, name: &str) -> ProjectId {
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: ExternalName::parse(name).expect("project name"),
            root_path: ExternalName::parse(&format!("/tmp/{project_id}")).expect("root"),
            created_at: Timestamp::now(),
        })
        .expect("project");
    project_id
}

#[test]
fn a_manual_epic_code_is_durable_immutable_and_project_scoped() {
    let directory = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&directory.path().join("kontor.db")).expect("store");
    let first_project = seed_project(&store, "First project");
    let first_epic = seed_epic(&store, first_project, "Kontor Operational MVP");
    let kop = EpicBacklogCode::parse("KOP").expect("manual override");

    let assigned = store
        .assign_epic_backlog_code(first_project, first_epic, Some(&kop), Timestamp::now())
        .expect("manual code assigns");

    assert_eq!(assigned, kop);
    assert_eq!(
        store
            .epic_backlog_code(first_project, first_epic)
            .expect("readback"),
        Some(kop.clone())
    );
    assert!(
        store
            .assign_epic_backlog_code(
                first_project,
                first_epic,
                Some(&EpicBacklogCode::parse("OTHER").expect("other code")),
                Timestamp::now(),
            )
            .is_err(),
        "an assignment cannot be changed"
    );
    let colliding_epic = seed_epic(&store, first_project, "Kontor Operations Platform");
    assert!(
        store
            .assign_epic_backlog_code(first_project, colliding_epic, Some(&kop), Timestamp::now(),)
            .is_err(),
        "a manual code cannot be assigned twice inside one project"
    );

    let second_project = seed_project(&store, "Second project");
    let second_epic = seed_epic(&store, second_project, "Kontor Other Project");
    assert_eq!(
        store
            .assign_epic_backlog_code(second_project, second_epic, Some(&kop), Timestamp::now())
            .expect("another project may reuse the namespace"),
        kop
    );
}

#[test]
fn automatic_codes_expand_deterministically_inside_one_project() {
    let directory = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&directory.path().join("kontor.db")).expect("store");
    let project = seed_project(&store, "Project");
    let first = seed_epic(&store, project, "Kontor Backlog Identities");
    let second = seed_epic(&store, project, "Knowledge Base Integration");

    assert_eq!(
        store
            .assign_epic_backlog_code(project, first, None, Timestamp::now())
            .expect("first allocation")
            .as_str(),
        "KBI"
    );
    assert_eq!(
        store
            .assign_epic_backlog_code(project, second, None, Timestamp::now())
            .expect("collision expansion")
            .as_str(),
        "KBIN"
    );
}

#[test]
fn database_triggers_forbid_updating_or_deleting_an_assignment() {
    let directory = tempfile::tempdir().expect("state root");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store");
    let project = seed_project(&store, "Project");
    let epic = seed_epic(&store, project, "Kontor Backlog Identities");
    store
        .assign_epic_backlog_code(project, epic, None, Timestamp::now())
        .expect("assignment");
    drop(store);

    let connection = rusqlite::Connection::open(path).expect("raw readback");
    assert!(
        connection
            .execute(
                "UPDATE epic_backlog_codes SET code = 'OTHER'
                 WHERE project_id = ?1 AND mini_project_id = ?2",
                rusqlite::params![project.to_string(), epic.to_string()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "DELETE FROM epic_backlog_codes
                 WHERE project_id = ?1 AND mini_project_id = ?2",
                rusqlite::params![project.to_string(), epic.to_string()],
            )
            .is_err()
    );
}

#[test]
fn derived_item_codes_are_not_persisted_as_a_second_jira_identity() {
    let directory = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&directory.path().join("kontor.db")).expect("store");
    drop(store);
    let connection =
        rusqlite::Connection::open(directory.path().join("kontor.db")).expect("raw readback");
    let mut statement = connection
        .prepare("SELECT sql FROM sqlite_schema WHERE type = 'table' AND sql IS NOT NULL")
        .expect("schema query");
    let table_definitions = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("schema rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("schema definitions");

    assert!(
        table_definitions
            .iter()
            .all(|definition| !definition.to_ascii_lowercase().contains("item_code")),
        "the Jira-derived item code is a projection, never another persisted identity"
    );
}

#[test]
fn racing_automatic_allocators_commit_distinct_deterministic_codes() {
    let directory = tempfile::tempdir().expect("state root");
    let path = directory.path().join("kontor.db");
    let first_store = SqliteStore::open(&path).expect("first store");
    let project = seed_project(&first_store, "Project");
    let first_epic = seed_epic(&first_store, project, "Kontor Backlog Identities");
    let second_epic = seed_epic(&first_store, project, "Knowledge Base Integration");
    let second_store = SqliteStore::open(&path).expect("second store");
    let barrier = Arc::new(Barrier::new(2));

    let first_barrier = Arc::clone(&barrier);
    let first = std::thread::spawn(move || {
        first_barrier.wait();
        first_store
            .assign_epic_backlog_code(project, first_epic, None, Timestamp::now())
            .expect("first allocation")
            .to_string()
    });
    let second_barrier = Arc::clone(&barrier);
    let second = std::thread::spawn(move || {
        second_barrier.wait();
        second_store
            .assign_epic_backlog_code(project, second_epic, None, Timestamp::now())
            .expect("second allocation")
            .to_string()
    });

    let committed = BTreeSet::from([
        first.join().expect("first allocator joins"),
        second.join().expect("second allocator joins"),
    ]);
    assert_eq!(committed.len(), 2);
    assert!(committed.contains("KBI"));
    assert!(
        committed.contains("KBIN") || committed.contains("KBIO"),
        "the loser expands its own title after observing the winner: {committed:?}"
    );
}

#[test]
fn migration_preserves_valid_legacy_codes_and_quarantines_duplicates_and_invalid_values() {
    let connection = rusqlite::Connection::open_in_memory().expect("migration fixture");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE mini_projects (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id),
                 UNIQUE (project_id, id)
             ) STRICT;
             CREATE TABLE epic_native_name_tokens (
                 project_id TEXT NOT NULL,
                 mini_project_id TEXT NOT NULL,
                 kontor_backlog_code TEXT NOT NULL,
                 ai_short_name TEXT NULL,
                 declared_at TEXT NOT NULL,
                 PRIMARY KEY (project_id, mini_project_id),
                 FOREIGN KEY (project_id, mini_project_id)
                     REFERENCES mini_projects(project_id, id)
             ) STRICT;
             INSERT INTO projects VALUES ('project');
             INSERT INTO mini_projects VALUES
                 ('valid', 'project'),
                 ('duplicate-a', 'project'),
                 ('duplicate-b', 'project'),
                 ('invalid', 'project');
             INSERT INTO epic_native_name_tokens VALUES
                 ('project', 'valid', 'KOP', NULL, '2026-08-01T00:00:00Z'),
                 ('project', 'duplicate-a', 'DUP', NULL, '2026-08-02T00:00:00Z'),
                 ('project', 'duplicate-b', 'DUP', NULL, '2026-08-03T00:00:00Z'),
                 ('project', 'invalid', 'bad-code', NULL, '2026-08-04T00:00:00Z');",
        )
        .expect("the v71 identity inputs are seeded");
    connection
        .execute_batch(include_str!(
            "../migrations/0072_epic_backlog_identities.sql"
        ))
        .expect("v72 migrates legacy identity evidence");

    let mut statement = connection
        .prepare(
            "SELECT mini_project_id, code, provenance, status
             FROM epic_backlog_codes ORDER BY mini_project_id",
        )
        .expect("migration rows query");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("migration rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("migration rows collect");
    assert_eq!(
        rows,
        vec![
            (
                "duplicate-a".to_owned(),
                "DUP".to_owned(),
                "legacy".to_owned(),
                "legacy_duplicate".to_owned(),
            ),
            (
                "duplicate-b".to_owned(),
                "DUP".to_owned(),
                "legacy".to_owned(),
                "legacy_duplicate".to_owned(),
            ),
            (
                "invalid".to_owned(),
                "bad-code".to_owned(),
                "legacy".to_owned(),
                "legacy_invalid".to_owned(),
            ),
            (
                "valid".to_owned(),
                "KOP".to_owned(),
                "legacy".to_owned(),
                "active".to_owned(),
            ),
        ]
    );
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version");
    assert_eq!(version, 72);
}

#[test]
fn a_quarantined_legacy_value_does_not_block_a_new_active_assignment() {
    let directory = tempfile::tempdir().expect("state root");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store");
    let project = seed_project(&store, "Project");
    let epic = seed_epic(&store, project, "Kontor Operational MVP");
    drop(store);
    let connection = rusqlite::Connection::open(&path).expect("legacy evidence connection");
    connection
        .execute(
            "INSERT INTO epic_backlog_codes
                 (project_id, mini_project_id, code, provenance, status, assigned_at)
             VALUES (?1, ?2, 'bad-code', 'legacy', 'legacy_invalid', ?3)",
            rusqlite::params![
                project.to_string(),
                epic.to_string(),
                "2026-08-01T00:00:00Z"
            ],
        )
        .expect("legacy evidence is preserved");
    drop(connection);

    let store = SqliteStore::open(&path).expect("store reopens");
    let assigned = store
        .assign_epic_backlog_code(
            project,
            epic,
            Some(&EpicBacklogCode::parse("KOP").expect("replacement namespace")),
            Timestamp::now(),
        )
        .expect("quarantined evidence does not occupy the active assignment slot");
    assert_eq!(assigned.as_str(), "KOP");
    drop(store);

    let connection = rusqlite::Connection::open(path).expect("readback connection");
    let rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM epic_backlog_codes
             WHERE project_id = ?1 AND mini_project_id = ?2",
            rusqlite::params![project.to_string(), epic.to_string()],
            |row| row.get(0),
        )
        .expect("evidence and active assignment read back");
    assert_eq!(
        rows, 2,
        "legacy evidence remains beside the active assignment"
    );
}
