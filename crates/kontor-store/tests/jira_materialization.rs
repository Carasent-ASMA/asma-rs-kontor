//! Durable Jira materialization and activation behavior.

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ConnectorKey, ContentHash, ExternalId,
    IdempotencyKey, MiniProjectId, ProjectId, TaskId, TicketLinkId, Timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CommandRepository, NewLocalCommand, NewMiniProject, NewProject, NewTask, NewTicketLink,
    ProjectRepository, TicketRepository,
};
use kontor_core::state::TaskState;
use kontor_store::{
    JiraIntentKind, JiraItemKind, JiraMaterializationRecoveryItem, NewJiraMaterializationBatch,
    NewJiraMaterializationItem, SqliteStore,
};

fn external(value: impl AsRef<str>) -> ExternalId {
    ExternalId::parse(value.as_ref()).expect("external id")
}

fn seed_graph(store: &SqliteStore) -> (ProjectId, MiniProjectId, TaskId, Timestamp) {
    let project_id = ProjectId::generate();
    let epic_id = MiniProjectId::generate();
    let task_id = TaskId::generate();
    let now = Timestamp::now();
    store
        .create_project(&NewProject {
            id: project_id,
            name: kontor_core::id::ExternalName::parse("Project").expect("name"),
            root_path: kontor_core::id::ExternalName::parse("/tmp/project").expect("path"),
            created_at: now,
        })
        .expect("project");
    store
        .create_mini_project(&NewMiniProject {
            id: epic_id,
            project_id,
            name: kontor_core::id::ExternalName::parse("Epic").expect("name"),
            created_at: now,
        })
        .expect("epic");
    store
        .create_task(&NewTask {
            id: task_id,
            project_id,
            mini_project_id: Some(epic_id),
            title: kontor_core::id::ExternalName::parse("Task").expect("title"),
            module: None,
            state: TaskState::Ready,
            created_at: now,
        })
        .expect("task");
    (project_id, epic_id, task_id, now)
}

#[test]
fn activation_requires_every_confirmed_binding_and_survives_readback() {
    let root = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&root.path().join("kontor.db")).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);

    let batch_id = external(uuid::Uuid::now_v7().to_string());
    let link_id = TicketLinkId::generate();
    let preview_hash = ContentHash::of(b"preview");
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "materialize-1".to_owned(),
                preview_hash,
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: None,
                    link_id: None,
                    ordinal: 0,
                    item_kind: JiraItemKind::Epic,
                    intent_kind: JiraIntentKind::Link,
                    requested_key: Some(external("ASMA-1")),
                    marker: external("kontor-epic-fixture"),
                },
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: Some(task_id),
                    link_id: Some(link_id),
                    ordinal: 1,
                    item_kind: JiraItemKind::Task,
                    intent_kind: JiraIntentKind::Link,
                    requested_key: Some(external("ASMA-2")),
                    marker: external("kontor-task-fixture"),
                },
            ],
        )
        .expect("plan is durable");
    let items = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("planned items");
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("epic binding query"),
        None
    );
    assert_eq!(
        store
            .confirmed_jira_task_key(project_id, task_id)
            .expect("task binding query"),
        None
    );
    assert!(
        store
            .activate_asma_epic(project_id, epic_id, CommandReceiptId::generate(), now)
            .is_err()
    );
    for (item, key) in items.iter().zip(["ASMA-1", "ASMA-2"]) {
        store
            .confirm_jira_materialization_item(
                item,
                &external(key),
                &ContentHash::of(key.as_bytes()),
                now,
            )
            .expect("readback is confirmed");
    }
    assert!(
        store
            .confirm_jira_materialization_item(
                &items[0],
                &external("ASMA-999"),
                &ContentHash::of(b"different-readback"),
                now,
            )
            .is_err(),
        "a confirmed item cannot be rebound by a stale or hostile retry"
    );
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("preserved epic binding")
            .as_ref()
            .map(ExternalId::as_str),
        Some("ASMA-1")
    );
    store
        .confirm_jira_materialization_batch(project_id, &batch_id, now)
        .expect("batch confirms");
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("confirmed epic key")
            .as_ref()
            .map(ExternalId::as_str),
        Some("ASMA-1")
    );
    assert_eq!(
        store
            .confirmed_jira_task_key(project_id, task_id)
            .expect("confirmed task key")
            .as_ref()
            .map(ExternalId::as_str),
        Some("ASMA-2")
    );
    let receipt_id = CommandReceiptId::generate();
    store
        .record_local_command(&NewLocalCommand {
            project_id,
            receipt_id,
            idempotency_key: IdempotencyKey::parse("activate-1").expect("key"),
            kind: CommandKind::ActivateAsmaEpic,
            target: AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "operation": "activate_asma_epic"
            }))
            .expect("intent"),
            created_at: now,
        })
        .expect("activation receipt");
    store
        .activate_asma_epic(project_id, epic_id, receipt_id, now)
        .expect("complete binding set activates");
    assert!(
        store
            .asma_epic_is_active(project_id, epic_id)
            .expect("readback")
    );
    let links = store
        .list_task_ticket_links(project_id, task_id)
        .expect("task link");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].external_issue_key.as_str(), "ASMA-2");
}

#[test]
fn confirmation_adopts_an_exact_existing_task_binding_after_transport_recovery() {
    let root = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&root.path().join("kontor.db")).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let batch_id = external(uuid::Uuid::now_v7().to_string());
    let planned_link_id = TicketLinkId::generate();
    let recovered_link_id = TicketLinkId::generate();

    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "materialize-recovery".to_owned(),
                preview_hash: ContentHash::of(b"recovery-preview"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: batch_id.clone(),
                project_id,
                epic_id,
                task_id: Some(task_id),
                link_id: Some(planned_link_id),
                ordinal: 0,
                item_kind: JiraItemKind::Task,
                intent_kind: JiraIntentKind::Create,
                requested_key: None,
                marker: external("kontor-task-recovery-fixture"),
            }],
        )
        .expect("plan is durable before transport");
    store
        .create_ticket_link(&NewTicketLink {
            id: recovered_link_id,
            project_id,
            task_id,
            connector: ConnectorKey::parse("connector.jira").expect("Jira connector"),
            external_issue_key: external("ASMA-8050"),
            created_at: now,
        })
        .expect("bounded recovery persisted the exact Jira binding");

    let planned = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("planned item")
        .into_iter()
        .next()
        .expect("task item");
    assert_eq!(planned.link_id, Some(planned_link_id));
    store
        .confirm_jira_materialization_item(
            &planned,
            &external("ASMA-8050"),
            &ContentHash::of(b"ASMA-8050-readback"),
            now,
        )
        .expect("the exact recovered binding is adopted");

    let confirmed = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("confirmed item")
        .into_iter()
        .next()
        .expect("task item");
    assert_eq!(confirmed.link_id, Some(recovered_link_id));
    assert_eq!(
        confirmed.confirmed_key.as_ref().map(ExternalId::as_str),
        Some("ASMA-8050")
    );
    let links = store
        .list_task_ticket_links(project_id, task_id)
        .expect("task links");
    assert_eq!(links.len(), 1, "recovery never creates a duplicate link");
    assert_eq!(links[0].id, recovered_link_id);
}

#[test]
fn a_confirmed_epic_binding_cannot_be_replaced_by_a_later_batch() {
    let root = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&root.path().join("kontor.db")).expect("store opens");
    let (project_id, epic_id, _, now) = seed_graph(&store);

    let first_batch = external(uuid::Uuid::now_v7().to_string());
    let first_item = NewJiraMaterializationItem {
        id: external(uuid::Uuid::now_v7().to_string()),
        batch_id: first_batch.clone(),
        project_id,
        epic_id,
        task_id: None,
        link_id: None,
        ordinal: 0,
        item_kind: JiraItemKind::Epic,
        intent_kind: JiraIntentKind::Link,
        requested_key: Some(external("ASMA-8049")),
        marker: external("kontor-epic-binding-first"),
    };
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: first_batch.clone(),
                project_id,
                epic_id,
                idempotency_key: "epic-binding-first".to_owned(),
                preview_hash: ContentHash::of(b"epic-binding-first"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[first_item],
        )
        .expect("first plan");
    let first = store
        .jira_materialization_items(project_id, &first_batch)
        .expect("first item")
        .remove(0);
    store
        .confirm_jira_materialization_item(
            &first,
            &external("ASMA-8049"),
            &ContentHash::of(b"ASMA-8049"),
            now,
        )
        .expect("first binding confirms");

    let second_batch = external(uuid::Uuid::now_v7().to_string());
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: second_batch.clone(),
                project_id,
                epic_id,
                idempotency_key: "epic-binding-second".to_owned(),
                preview_hash: ContentHash::of(b"epic-binding-second"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: second_batch.clone(),
                project_id,
                epic_id,
                task_id: None,
                link_id: None,
                ordinal: 0,
                item_kind: JiraItemKind::Epic,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external("ASMA-9999")),
                marker: external("kontor-epic-binding-second"),
            }],
        )
        .expect("second plan");
    let second = store
        .jira_materialization_items(project_id, &second_batch)
        .expect("second item")
        .remove(0);
    assert!(
        store
            .confirm_jira_materialization_item(
                &second,
                &external("ASMA-9999"),
                &ContentHash::of(b"ASMA-9999"),
                now,
            )
            .is_err(),
        "a later batch may not replace a confirmed epic identity"
    );
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("original binding")
            .as_ref()
            .map(ExternalId::as_str),
        Some("ASMA-8049")
    );
}

#[test]
fn planning_refuses_a_non_exact_item_set_without_persisting_a_partial_batch() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let batch_id = external(uuid::Uuid::now_v7().to_string());
    let result = store.plan_jira_materialization(
        &NewJiraMaterializationBatch {
            id: batch_id.clone(),
            project_id,
            epic_id,
            idempotency_key: "duplicate-ordinal-plan".to_owned(),
            preview_hash: ContentHash::of(b"duplicate-ordinal-plan"),
            expected_revision: AggregateRevision::INITIAL,
            created_at: now,
        },
        &[
            NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: batch_id.clone(),
                project_id,
                epic_id,
                task_id: None,
                link_id: None,
                ordinal: 0,
                item_kind: JiraItemKind::Epic,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external("ASMA-8049")),
                marker: external("kontor-duplicate-ordinal-epic"),
            },
            NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: batch_id.clone(),
                project_id,
                epic_id,
                task_id: Some(task_id),
                link_id: Some(TicketLinkId::generate()),
                ordinal: 0,
                item_kind: JiraItemKind::Task,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external("ASMA-8050")),
                marker: external("kontor-duplicate-ordinal-task"),
            },
        ],
    );
    assert!(result.is_err(), "duplicate ordinals must be rejected");
    drop(store);

    let connection = rusqlite::Connection::open(path).expect("database reopens");
    let batches: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_batches WHERE id = ?1",
            [batch_id.as_str()],
            |row| row.get(0),
        )
        .expect("batch count");
    assert_eq!(batches, 0, "an invalid item set leaves no partial batch");
}

#[test]
fn link_recovery_adopts_the_original_pending_create_batch_in_place() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let original_batch_id = external(uuid::Uuid::now_v7().to_string());
    let original_epic_item_id = external(uuid::Uuid::now_v7().to_string());
    let original_task_item_id = external(uuid::Uuid::now_v7().to_string());
    let epic_marker = external("kontor-epic-kbi-8050");
    let task_marker = external("kontor-task-kbi-8050");
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: original_batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "kbi-jira-materialize-20260830-v1".to_owned(),
                preview_hash: ContentHash::of(b"failed-create-preview"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[
                NewJiraMaterializationItem {
                    id: original_epic_item_id.clone(),
                    batch_id: original_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: None,
                    link_id: None,
                    ordinal: 0,
                    item_kind: JiraItemKind::Epic,
                    intent_kind: JiraIntentKind::Create,
                    requested_key: None,
                    marker: epic_marker.clone(),
                },
                NewJiraMaterializationItem {
                    id: original_task_item_id.clone(),
                    batch_id: original_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: Some(task_id),
                    link_id: Some(TicketLinkId::generate()),
                    ordinal: 1,
                    item_kind: JiraItemKind::Task,
                    intent_kind: JiraIntentKind::Create,
                    requested_key: None,
                    marker: task_marker.clone(),
                },
            ],
        )
        .expect("original create plan");

    let recovery_receipt_id = CommandReceiptId::generate();
    let recovery_preview_hash = ContentHash::of(b"exact-link-recovery-preview");
    store
        .record_local_command(&NewLocalCommand {
            project_id,
            receipt_id: recovery_receipt_id,
            idempotency_key: IdempotencyKey::parse("kbi-jira-recovery-20260831-v1")
                .expect("recovery key"),
            kind: CommandKind::MaterializeJira,
            target: AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "operation": "jira_materialization_apply",
                "preview_hash": recovery_preview_hash.as_str(),
            }))
            .expect("recovery intent"),
            created_at: now,
        })
        .expect("recovery command");
    let recovery_items = vec![
        JiraMaterializationRecoveryItem {
            ordinal: 0,
            item_kind: JiraItemKind::Epic,
            task_id: None,
            requested_key: external("ASMA-8049"),
            marker: epic_marker.clone(),
        },
        JiraMaterializationRecoveryItem {
            ordinal: 1,
            item_kind: JiraItemKind::Task,
            task_id: Some(task_id),
            requested_key: external("ASMA-8050"),
            marker: task_marker.clone(),
        },
    ];
    let mut wrong_marker = recovery_items.clone();
    wrong_marker[1].marker = external("kontor-task-another-scope");
    assert!(
        store
            .recover_pending_jira_materialization(
                project_id,
                epic_id,
                recovery_receipt_id,
                &recovery_preview_hash,
                &wrong_marker,
                now,
            )
            .is_err(),
        "approximate marker scope may not recover a create batch"
    );
    let recovered = store
        .recover_pending_jira_materialization(
            project_id,
            epic_id,
            recovery_receipt_id,
            &recovery_preview_hash,
            &recovery_items,
            now,
        )
        .expect("exact recovery")
        .expect("pending batch found");
    assert_eq!(recovered.batch_id, original_batch_id);
    assert_eq!(
        recovered
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>(),
        vec![original_epic_item_id, original_task_item_id]
    );
    let replayed = store
        .recover_pending_jira_materialization(
            project_id,
            epic_id,
            recovery_receipt_id,
            &recovery_preview_hash,
            &recovery_items,
            now,
        )
        .expect("recovery replay")
        .expect("the ledger resolves the same original batch");
    assert_eq!(replayed.batch_id, recovered.batch_id);

    drop(store);
    let connection = rusqlite::Connection::open(path).expect("recovery readback");
    let batches: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_batches",
            [],
            |row| row.get(0),
        )
        .expect("batch count");
    let recoveries: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_recoveries",
            [],
            |row| row.get(0),
        )
        .expect("recovery count");
    assert_eq!(batches, 1, "recovery creates no replacement batch");
    assert_eq!(recoveries, 2, "every adopted item is durably ledgered");
}

#[test]
fn a_safe_link_batch_can_recover_the_scope_of_an_unconfirmed_create_batch() {
    let root = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&root.path().join("kontor.db")).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let create_batch_id = external(uuid::Uuid::now_v7().to_string());
    let link_batch_id = external(uuid::Uuid::now_v7().to_string());
    let epic_marker = external("kontor-epic-retry-fixture");
    let task_marker = external("kontor-task-retry-fixture");

    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: create_batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "failed-create-plan".to_owned(),
                preview_hash: ContentHash::of(b"create-preview"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: create_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: None,
                    link_id: None,
                    ordinal: 0,
                    item_kind: JiraItemKind::Epic,
                    intent_kind: JiraIntentKind::Create,
                    requested_key: None,
                    marker: epic_marker.clone(),
                },
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: create_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: Some(task_id),
                    link_id: Some(TicketLinkId::generate()),
                    ordinal: 1,
                    item_kind: JiraItemKind::Task,
                    intent_kind: JiraIntentKind::Create,
                    requested_key: None,
                    marker: task_marker.clone(),
                },
            ],
        )
        .expect("the failed create plan remains durable");

    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: link_batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "safe-link-recovery".to_owned(),
                preview_hash: ContentHash::of(b"link-preview"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: link_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: None,
                    link_id: None,
                    ordinal: 0,
                    item_kind: JiraItemKind::Epic,
                    intent_kind: JiraIntentKind::Link,
                    requested_key: Some(external("ASMA-8049")),
                    marker: epic_marker,
                },
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: link_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: Some(task_id),
                    link_id: Some(TicketLinkId::generate()),
                    ordinal: 1,
                    item_kind: JiraItemKind::Task,
                    intent_kind: JiraIntentKind::Link,
                    requested_key: Some(external("ASMA-8050")),
                    marker: task_marker,
                },
            ],
        )
        .expect("the non-creating recovery plan is durable");

    let recovery = store
        .jira_materialization_items(project_id, &link_batch_id)
        .expect("the recovery items read back");
    assert_eq!(recovery.len(), 2, "no recovery item may be ignored");
    assert!(
        recovery
            .iter()
            .all(|item| item.intent_kind == JiraIntentKind::Link),
        "the recovery remains non-creating"
    );
    assert_eq!(
        store
            .jira_materialization_items(project_id, &create_batch_id)
            .expect("the original attempt remains")
            .len(),
        2,
        "recovery preserves the failed attempt as evidence"
    );
}

#[test]
fn v73_migrates_an_empty_confirmed_link_batch_without_losing_incident_evidence() {
    let connection = rusqlite::Connection::open_in_memory().expect("migration fixture");
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE mini_projects (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id),
                 UNIQUE (project_id, id)
             ) STRICT;
             CREATE TABLE tasks (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id),
                 mini_project_id TEXT REFERENCES mini_projects(id)
             ) STRICT;
             CREATE TABLE jira_materialization_batches (
                 id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
                 project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
                 epic_id TEXT NOT NULL REFERENCES mini_projects (id) ON DELETE RESTRICT,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 preview_hash TEXT NOT NULL CHECK (length(preview_hash) = 64),
                 expected_revision INTEGER NOT NULL CHECK (expected_revision >= 1),
                 status TEXT NOT NULL CHECK (status IN ('planned', 'confirmed', 'conflict')),
                 created_at TEXT NOT NULL,
                 confirmed_at TEXT NULL,
                 UNIQUE (project_id, id),
                 UNIQUE (project_id, epic_id, preview_hash)
             ) STRICT;
             CREATE TABLE jira_materialization_items (
                 id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
                 batch_id TEXT NOT NULL REFERENCES jira_materialization_batches (id) ON DELETE RESTRICT,
                 project_id TEXT NOT NULL,
                 epic_id TEXT NOT NULL,
                 task_id TEXT NULL REFERENCES tasks (id) ON DELETE RESTRICT,
                 link_id TEXT NULL,
                 ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
                 item_kind TEXT NOT NULL CHECK (item_kind IN ('epic', 'task')),
                 intent_kind TEXT NOT NULL CHECK (intent_kind IN ('create', 'link')),
                 requested_key TEXT NULL,
                 marker TEXT NOT NULL UNIQUE CHECK (length(marker) BETWEEN 1 AND 255),
                 status TEXT NOT NULL CHECK (status IN ('planned', 'confirmed', 'conflict')),
                 confirmed_key TEXT NULL,
                 readback_hash TEXT NULL CHECK (readback_hash IS NULL OR length(readback_hash) = 64),
                 confirmed_at TEXT NULL,
                 UNIQUE (batch_id, ordinal),
                 UNIQUE (project_id, epic_id, task_id),
                 CHECK ((item_kind = 'epic' AND task_id IS NULL AND link_id IS NULL)
                     OR (item_kind = 'task' AND task_id IS NOT NULL AND link_id IS NOT NULL)),
                 CHECK ((intent_kind = 'create' AND requested_key IS NULL) OR (intent_kind = 'link' AND requested_key IS NOT NULL)),
                 CHECK ((status = 'confirmed' AND confirmed_key IS NOT NULL AND readback_hash IS NOT NULL AND confirmed_at IS NOT NULL)
                     OR (status <> 'confirmed' AND confirmed_key IS NULL AND readback_hash IS NULL AND confirmed_at IS NULL)),
                 FOREIGN KEY (project_id, epic_id) REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
             ) STRICT;
             INSERT INTO projects VALUES ('project');
             INSERT INTO mini_projects VALUES ('epic', 'project');
             INSERT INTO tasks VALUES ('task', 'project', 'epic');
             INSERT INTO jira_materialization_batches VALUES
                 ('00000000-0000-7000-8000-000000000001', 'project', 'epic',
                  'failed-create', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                  1, 'planned', '2026-08-30T00:00:00Z', NULL),
                 ('00000000-0000-7000-8000-000000000002', 'project', 'epic',
                  'safe-link', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                  1, 'confirmed', '2026-08-30T00:01:00Z', '2026-08-30T00:02:00Z');
             INSERT INTO jira_materialization_items VALUES
                 ('00000000-0000-7000-8000-000000000011',
                  '00000000-0000-7000-8000-000000000001', 'project', 'epic',
                  NULL, NULL, 0, 'epic', 'create', NULL, 'epic-marker', 'planned',
                  NULL, NULL, NULL),
                 ('00000000-0000-7000-8000-000000000012',
                  '00000000-0000-7000-8000-000000000001', 'project', 'epic',
                  'task', '00000000-0000-7000-8000-000000000099', 1, 'task',
                  'create', NULL, 'task-marker', 'planned', NULL, NULL, NULL);
             PRAGMA user_version = 72;",
        )
        .expect("the v72 incident shape is seeded");

    connection
        .execute_batch(include_str!(
            "../migrations/0073_retryable_jira_link_reconciliation.sql"
        ))
        .expect("v73 migrates the incident shape");
    connection
        .execute_batch(
            "INSERT INTO jira_materialization_items VALUES
                 ('00000000-0000-7000-8000-000000000021',
                  '00000000-0000-7000-8000-000000000002', 'project', 'epic',
                  NULL, NULL, 0, 'epic', 'link', 'ASMA-8049', 'epic-marker',
                  'planned', NULL, NULL, NULL),
                 ('00000000-0000-7000-8000-000000000022',
                  '00000000-0000-7000-8000-000000000002', 'project', 'epic',
                  'task', '00000000-0000-7000-8000-000000000098', 1, 'task',
                  'link', 'ASMA-8050', 'task-marker', 'planned', NULL, NULL, NULL);",
        )
        .expect("the exact safe link retry can repopulate its empty batch");

    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version");
    let incident_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_items",
            [],
            |row| row.get(0),
        )
        .expect("incident rows");
    let foreign_key_failures: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign-key check");
    assert_eq!(version, 73);
    assert_eq!(incident_rows, 4, "both attempts remain auditable");
    assert_eq!(foreign_key_failures, 0);
}
