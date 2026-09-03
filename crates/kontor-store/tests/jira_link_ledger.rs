//! Canonical Jira task-link identity at the store boundary and across upgrades.

use kontor_core::id::{
    ConnectorKey, ExternalId, ExternalName, ProjectId, TaskId, TicketLinkId, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::repository::{
    NewProject, NewTask, NewTicketLink, ProjectRepository, RepositoryError, TicketRepository,
};
use kontor_core::state::TaskState;
use kontor_store::SqliteStore;
use kontor_store::backup::export_realm;
use rusqlite::{Connection, OptionalExtension, params};
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical timestamp")
}

fn connector(text: &str) -> ConnectorKey {
    ConnectorKey::parse(text).expect("a valid connector key")
}

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a valid external id")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
    first_task: TaskId,
    second_task: TaskId,
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");
    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Jira ledger"),
            root_path: name("/tmp/jira-ledger"),
            created_at: at("2026-09-03T08:00:00Z"),
        })
        .expect("the project is created");
    let first_task = TaskId::generate();
    let second_task = TaskId::generate();
    for (id, title) in [(first_task, "First"), (second_task, "Second")] {
        store
            .create_task(&NewTask {
                id,
                project_id: project,
                mini_project_id: None,
                title: name(title),
                module: None,
                state: TaskState::Ready,
                created_at: at("2026-09-03T08:01:00Z"),
            })
            .expect("the task is created");
    }
    Fixture {
        _directory: directory,
        path,
        store,
        project,
        first_task,
        second_task,
    }
}

fn strip_v81(connection: &Connection) {
    connection
        .execute_batch(
            "DROP INDEX ux_status_conflicts_one_open_kind;
             DROP TRIGGER canonical_jira_task_links_permanent;
             DROP TRIGGER canonical_jira_task_links_immutable;
             DROP TRIGGER jira_links_require_canonical_jira_update;
             DROP TRIGGER jira_links_require_canonical_jira_insert;
             DROP TABLE canonical_jira_task_links;
             PRAGMA user_version = 80;",
        )
        .expect("the test database is reduced to its v80 shape");
}

fn plant_link(
    connection: &Connection,
    id: TicketLinkId,
    project: ProjectId,
    task: TaskId,
    connector_key: &str,
    issue_key: &str,
    created_at: &str,
) {
    connection
        .execute(
            "INSERT INTO jira_links
                 (id, project_id, task_id, connector, external_issue_key, revision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![
                id.to_string(),
                project.to_string(),
                task.to_string(),
                connector_key,
                issue_key,
                created_at
            ],
        )
        .expect("the historical link is planted");
}

fn new_link(
    project_id: ProjectId,
    task_id: TaskId,
    connector_key: &str,
    issue_key: &str,
) -> NewTicketLink {
    NewTicketLink {
        id: TicketLinkId::generate(),
        project_id,
        task_id,
        connector: connector(connector_key),
        external_issue_key: external(issue_key),
        created_at: at("2026-09-03T08:02:00Z"),
    }
}

#[test]
fn jira_aliases_share_one_idempotent_task_link_ledger() {
    let fixture = fixture();
    let first = fixture
        .store
        .create_ticket_link(&new_link(
            fixture.project,
            fixture.first_task,
            "jira",
            "ASMA-8100",
        ))
        .expect("the legacy alias is accepted at the boundary");
    assert_eq!(first.connector, connector("connector.jira"));

    let replay = fixture
        .store
        .create_ticket_link(&new_link(
            fixture.project,
            fixture.first_task,
            "connector.jira",
            "ASMA-8100",
        ))
        .expect("the same logical binding is an idempotent replay");
    assert_eq!(replay, first, "the replay returns the persisted identity");

    let links = fixture
        .store
        .list_task_ticket_links(fixture.project, fixture.first_task)
        .expect("the task links are readable");
    assert_eq!(links, vec![first]);
}

#[test]
fn jira_task_and_issue_identity_are_both_exclusive() {
    let fixture = fixture();
    fixture
        .store
        .create_ticket_link(&new_link(
            fixture.project,
            fixture.first_task,
            "connector.jira",
            "ASMA-8101",
        ))
        .expect("the first binding is created");

    let second_key = fixture
        .store
        .create_ticket_link(&new_link(
            fixture.project,
            fixture.first_task,
            "jira",
            "ASMA-8102",
        ))
        .expect_err("one task cannot acquire a second Jira key");
    assert!(matches!(second_key, RepositoryError::Conflict { .. }));

    let second_task = fixture
        .store
        .create_ticket_link(&new_link(
            fixture.project,
            fixture.second_task,
            "connector.jira",
            "ASMA-8101",
        ))
        .expect_err("one Jira key cannot name a second task");
    assert!(matches!(second_task, RepositoryError::Conflict { .. }));

    assert!(
        fixture
            .store
            .list_task_ticket_links(fixture.project, fixture.second_task)
            .expect("the second task links are readable")
            .is_empty(),
        "both refusals are atomic"
    );
}

#[test]
fn v81_selects_an_exact_canonical_duplicate_without_rewriting_legacy_evidence() {
    let fixture = fixture();
    let alias = TicketLinkId::generate();
    let canonical = TicketLinkId::generate();
    let connection = Connection::open(&fixture.path).expect("the database opens directly");
    strip_v81(&connection);
    plant_link(
        &connection,
        alias,
        fixture.project,
        fixture.first_task,
        "jira",
        "ASMA-8103",
        "2026-09-03T08:03:00Z",
    );
    plant_link(
        &connection,
        canonical,
        fixture.project,
        fixture.first_task,
        "connector.jira",
        "ASMA-8103",
        "2026-09-03T08:04:00Z",
    );
    connection
        .execute(
            "INSERT INTO external_comments
                 (project_id, link_id, external_comment_id, body_hash, author_account_id,
                  author_display, external_created_at, external_updated_at, body, observed_at,
                  supersedes_hash)
             VALUES (?1, ?2, 'comment-1', ?3, 'account-1', NULL, ?4, ?4, 'evidence', ?4, NULL)",
            params![
                fixture.project.to_string(),
                alias.to_string(),
                "a".repeat(64),
                "2026-09-03T08:05:00Z"
            ],
        )
        .expect("dependent evidence is attached to the legacy link id");

    let transaction = connection
        .unchecked_transaction()
        .expect("the migration transaction starts");
    transaction
        .execute_batch(include_str!(
            "../migrations/0081_canonical_jira_task_link_ledger.sql"
        ))
        .expect("the exact alias/canonical duplicate migrates");
    transaction.commit().expect("the migration commits");

    let selected: String = connection
        .query_row(
            "SELECT link_id FROM canonical_jira_task_links
             WHERE project_id = ?1 AND task_id = ?2",
            params![fixture.project.to_string(), fixture.first_task.to_string()],
            |row| row.get(0),
        )
        .expect("the canonical ledger row exists");
    assert_eq!(selected, canonical.to_string());
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM jira_links
                 WHERE project_id = ?1 AND external_issue_key = 'ASMA-8103'",
                params![fixture.project.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("historical links are countable"),
        2,
        "neither historical link identity is erased"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT link_id FROM external_comments WHERE external_comment_id = 'comment-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("the dependent evidence survives"),
        alias.to_string(),
        "immutable evidence keeps the exact link id it was observed under"
    );
    assert!(
        connection
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .expect("foreign keys are checkable")
            .is_none()
    );
    drop(connection);

    let visible = fixture
        .store
        .list_task_ticket_links(fixture.project, fixture.first_task)
        .expect("canonical links are readable");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, canonical);
    assert_eq!(visible[0].connector, connector("connector.jira"));

    let export = export_realm(&fixture.store, at("2026-09-03T09:00:00Z"))
        .expect("the canonical ledger is exportable");
    assert_eq!(export.records.canonical_jira_task_links.len(), 1);
    assert_eq!(
        export.records.canonical_jira_task_links[0].link_id,
        canonical.to_string()
    );
    assert_eq!(export.records.jira_links.len(), 2);
}

#[test]
fn v81_keeps_a_legacy_only_confirmed_binding_visible_through_the_canonical_ledger() {
    let fixture = fixture();
    let legacy = TicketLinkId::generate();
    let connection = Connection::open(&fixture.path).expect("the database opens directly");
    strip_v81(&connection);
    plant_link(
        &connection,
        legacy,
        fixture.project,
        fixture.first_task,
        "jira",
        "ASMA-8107",
        "2026-09-03T08:03:00Z",
    );
    connection
        .execute(
            "INSERT INTO jira_task_binding_confirmations
                 (project_id, link_id, readback_hash, confirmed_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                fixture.project.to_string(),
                legacy.to_string(),
                "a".repeat(64),
                "2026-09-03T08:04:00Z"
            ],
        )
        .expect("the historical confirmation is planted");
    let transaction = connection
        .unchecked_transaction()
        .expect("the migration transaction starts");
    transaction
        .execute_batch(include_str!(
            "../migrations/0081_canonical_jira_task_link_ledger.sql"
        ))
        .expect("the legacy-only confirmed binding migrates");
    transaction.commit().expect("the migration commits");
    drop(connection);

    assert_eq!(
        fixture
            .store
            .confirmed_jira_task_key(fixture.project, fixture.first_task)
            .expect("the migrated confirmation reads")
            .as_ref()
            .map(ExternalId::as_str),
        Some("ASMA-8107")
    );
    let visible = fixture
        .store
        .list_task_ticket_links(fixture.project, fixture.first_task)
        .expect("the selected binding reads");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, legacy);
    assert_eq!(visible[0].connector, connector("connector.jira"));
}

fn assert_v81_refuses_conflict(
    first_task_key: (&str, &str),
    second_task_key: (&str, &str),
    second_task: bool,
) {
    let fixture = fixture();
    let connection = Connection::open(&fixture.path).expect("the database opens directly");
    strip_v81(&connection);
    plant_link(
        &connection,
        TicketLinkId::generate(),
        fixture.project,
        fixture.first_task,
        first_task_key.0,
        first_task_key.1,
        "2026-09-03T08:03:00Z",
    );
    plant_link(
        &connection,
        TicketLinkId::generate(),
        fixture.project,
        if second_task {
            fixture.second_task
        } else {
            fixture.first_task
        },
        second_task_key.0,
        second_task_key.1,
        "2026-09-03T08:04:00Z",
    );

    let transaction = connection
        .unchecked_transaction()
        .expect("the migration transaction starts");
    let error = transaction
        .execute_batch(include_str!(
            "../migrations/0081_canonical_jira_task_link_ledger.sql"
        ))
        .expect_err("irreconcilable history must stop the migration");
    assert!(error.to_string().contains("irreconcilable historical Jira"));
    drop(transaction);

    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .expect("the version is readable"),
        80
    );
    assert!(
        connection
            .query_row(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name = 'canonical_jira_task_links'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .expect("the schema is readable")
            .is_none(),
        "the failed migration leaves no partial ledger"
    );
}

#[test]
fn v81_fails_closed_for_each_irreconcilable_historical_identity() {
    assert_v81_refuses_conflict(
        ("jira", "ASMA-8104"),
        ("connector.jira", "ASMA-8105"),
        false,
    );
    assert_v81_refuses_conflict(("jira", "ASMA-8106"), ("connector.jira", "ASMA-8106"), true);
}
