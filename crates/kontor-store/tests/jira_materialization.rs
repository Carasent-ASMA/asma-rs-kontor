//! Durable Jira materialization and activation behavior.

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ConnectorKey, ContentHash, ExternalId,
    IdempotencyKey, MiniProjectId, ProjectId, TaskId, TicketLinkId, Timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CommandRepository, NewLocalCommand, NewMiniProject, NewProject, NewTask, NewTicketLink,
    ProjectRepository, RepositoryError, TicketRepository,
};
use kontor_core::state::TaskState;
use kontor_store::{
    JiraBindingState, JiraBindingSubject, JiraIntentKind, JiraItemKind,
    JiraMaterializationRecoveryItem, NewJiraMaterializationBatch, NewJiraMaterializationItem,
    SqliteStore,
};

/// A stable immutable Jira issue id for a key.
///
/// Real Jira ids are opaque numerics unrelated to the key; deriving one here
/// only keeps distinct keys in a project on distinct identities, so a test that
/// means "another issue" does not accidentally say "the same issue renamed".
fn issue_id(key: &str) -> ExternalId {
    let digits: String = key.chars().filter(char::is_ascii_digit).collect();
    external(format!("90{digits}"))
}

fn external(value: impl AsRef<str>) -> ExternalId {
    ExternalId::parse(value.as_ref()).expect("external id")
}

fn seed_graph(store: &SqliteStore) -> (ProjectId, MiniProjectId, TaskId, Timestamp) {
    seed_named_graph(store, "Project")
}

/// Seed a second, independent project so cross-project isolation can be proved.
///
/// Name and root path are unique per project, so a fixture that wants two of
/// them has to say which is which rather than reusing one spelling twice.
fn seed_named_graph(
    store: &SqliteStore,
    label: &str,
) -> (ProjectId, MiniProjectId, TaskId, Timestamp) {
    let project_id = ProjectId::generate();
    let epic_id = MiniProjectId::generate();
    let task_id = TaskId::generate();
    let now = Timestamp::now();
    store
        .create_project(&NewProject {
            id: project_id,
            name: kontor_core::id::ExternalName::parse(label).expect("name"),
            root_path: kontor_core::id::ExternalName::parse(&format!(
                "/tmp/{}",
                label.to_lowercase()
            ))
            .expect("path"),
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
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
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
            .jira_epic_binding_state(project_id, epic_id)
            .expect("draft epic binding state"),
        JiraBindingState::AwaitingJiraBinding
    );
    assert_eq!(
        store
            .jira_task_binding_state(project_id, task_id)
            .expect("draft task binding state"),
        JiraBindingState::AwaitingJiraBinding
    );
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "ASMA-1"),
        Err(RepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "asma-1"),
        Err(RepositoryError::Domain(_))
    ));
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, ""),
        Err(RepositoryError::Domain(_))
    ));
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "ASMA-999"),
        Err(RepositoryError::NotFound { .. })
    ));
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
                &issue_id(key),
                &ContentHash::of(key.as_bytes()),
                now,
            )
            .expect("readback is confirmed");
    }
    let epic_binding = store
        .resolve_confirmed_jira_key(project_id, "ASMA-1")
        .expect("exact epic key resolves");
    assert_eq!(epic_binding.subject, JiraBindingSubject::Epic(epic_id));
    assert_eq!(epic_binding.jira_key.as_str(), "ASMA-1");
    assert_eq!(epic_binding.readback_hash, ContentHash::of(b"ASMA-1"));
    assert_eq!(epic_binding.confirmed_at, now);
    assert_eq!(epic_binding.revision, AggregateRevision::INITIAL);
    let task_binding = store
        .resolve_confirmed_jira_key(project_id, "ASMA-2")
        .expect("exact task key resolves");
    assert_eq!(task_binding.subject, JiraBindingSubject::Task(task_id));
    assert_eq!(task_binding.jira_key.as_str(), "ASMA-2");
    assert_eq!(task_binding.readback_hash, ContentHash::of(b"ASMA-2"));
    assert_eq!(task_binding.confirmed_at, now);
    assert_eq!(task_binding.revision, AggregateRevision::INITIAL);
    assert!(matches!(
        store.jira_epic_binding_state(project_id, epic_id),
        Ok(JiraBindingState::Confirmed(binding))
            if binding.subject == JiraBindingSubject::Epic(epic_id)
    ));
    assert!(matches!(
        store.jira_task_binding_state(project_id, task_id),
        Ok(JiraBindingState::Confirmed(binding))
            if binding.subject == JiraBindingSubject::Task(task_id)
    ));
    assert!(matches!(
        store.resolve_confirmed_jira_key(ProjectId::generate(), "ASMA-1"),
        Err(RepositoryError::NotFound { .. })
    ));
    assert!(
        store
            .confirm_jira_materialization_item(
                &items[0],
                &external("ASMA-999"),
                &issue_id("ASMA-999"),
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

    let duplicate_epic_id = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: duplicate_epic_id,
            project_id,
            name: kontor_core::id::ExternalName::parse("Duplicate epic").expect("name"),
            created_at: now,
        })
        .expect("duplicate epic fixture");
    let duplicate_batch_id = external(uuid::Uuid::now_v7().to_string());
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: duplicate_batch_id.clone(),
                project_id,
                epic_id: duplicate_epic_id,
                idempotency_key: "duplicate-cross-subject-key".to_owned(),
                preview_hash: ContentHash::of(b"duplicate-cross-subject-key"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: duplicate_batch_id.clone(),
                project_id,
                epic_id: duplicate_epic_id,
                task_id: None,
                link_id: None,
                ordinal: 0,
                item_kind: JiraItemKind::Epic,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external("ASMA-2")),
                marker: external("kontor-duplicate-cross-subject-key"),
            }],
        )
        .expect("duplicate confirmation plan");
    let duplicate = store
        .jira_materialization_items(project_id, &duplicate_batch_id)
        .expect("duplicate item")
        .remove(0);
    assert!(matches!(
        store.confirm_jira_materialization_item(
            &duplicate,
            &external("ASMA-2"),
            &issue_id("ASMA-2"),
            &ContentHash::of(b"duplicate-ASMA-2"),
            now,
        ),
        Err(RepositoryError::Conflict { .. })
    ));

    drop(store);
    let connection = rusqlite::Connection::open(&path).expect("corrupt fixture opens");
    connection
        .execute(
            "UPDATE jira_epic_bindings SET external_issue_key = 'ASMA-2'
             WHERE project_id = ?1 AND epic_id = ?2",
            rusqlite::params![project_id.to_string(), epic_id.to_string()],
        )
        .expect("a pre-validation cross-subject duplicate is seeded");
    drop(connection);
    let store = SqliteStore::open(&path).expect("corrupt fixture reopens");
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "ASMA-2"),
        Err(RepositoryError::Conflict { .. })
    ));
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
            &issue_id("ASMA-8050"),
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
fn confirmation_adopts_a_migrated_legacy_jira_alias_binding() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let legacy_link_id = TicketLinkId::generate();

    let mut connection = rusqlite::Connection::open(&path).expect("database opens directly");
    connection
        .execute_batch(
            "DROP INDEX ux_status_conflicts_one_open_kind;
             DROP TRIGGER canonical_jira_task_links_permanent;
             DROP TRIGGER canonical_jira_task_links_key_change_requires_proof;
             DROP TRIGGER canonical_jira_task_links_identity_immutable;
             DROP TRIGGER jira_links_require_canonical_jira_update;
             DROP TRIGGER jira_links_require_canonical_jira_insert;
             DROP TABLE canonical_jira_task_links;
             PRAGMA user_version = 80;",
        )
        .expect("the fixture is reduced to its legacy shape");
    connection
        .execute(
            "INSERT INTO jira_links
                 (id, project_id, task_id, connector, external_issue_key, revision, created_at)
             VALUES (?1, ?2, ?3, 'jira', 'ASMA-8051', 1, ?4)",
            rusqlite::params![
                legacy_link_id.to_string(),
                project_id.to_string(),
                task_id.to_string(),
                now.to_string(),
            ],
        )
        .expect("the historical alias binding is planted");
    let migration = connection
        .transaction()
        .expect("the migration transaction starts");
    migration
        .execute_batch(include_str!(
            "../migrations/0081_canonical_jira_task_link_ledger.sql"
        ))
        .expect("the legacy alias is selected by the canonical ledger");
    migration.commit().expect("the migration commits");
    drop(connection);

    let batch_id = external(uuid::Uuid::now_v7().to_string());
    let planned_link_id = TicketLinkId::generate();
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "materialize-legacy-alias-recovery".to_owned(),
                preview_hash: ContentHash::of(b"legacy-alias-recovery-preview"),
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
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external("ASMA-8051")),
                marker: external("kontor-task-legacy-alias-fixture"),
            }],
        )
        .expect("the recovery plan is durable");

    let planned = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("planned item")
        .into_iter()
        .next()
        .expect("task item");
    store
        .confirm_jira_materialization_item(
            &planned,
            &external("ASMA-8051"),
            &issue_id("ASMA-8051"),
            &ContentHash::of(b"ASMA-8051-readback"),
            now,
        )
        .expect("the migrated legacy binding is adopted");

    let confirmed = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("confirmed item")
        .into_iter()
        .next()
        .expect("task item");
    assert_eq!(confirmed.link_id, Some(legacy_link_id));
    assert_eq!(
        confirmed.confirmed_key.as_ref().map(ExternalId::as_str),
        Some("ASMA-8051")
    );
    let links = store
        .list_task_ticket_links(project_id, task_id)
        .expect("task links");
    assert_eq!(links.len(), 1, "recovery creates no duplicate link");
    assert_eq!(links[0].id, legacy_link_id);
    assert_eq!(links[0].connector.as_str(), "connector.jira");
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
            &issue_id("ASMA-8049"),
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
                &issue_id("ASMA-9999"),
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
                "project_id": project_id.to_string(),
                "epic_id": epic_id.to_string(),
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
    let foreign_epic_id = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: foreign_epic_id,
            project_id,
            name: kontor_core::id::ExternalName::parse("Foreign epic").expect("name"),
            created_at: now,
        })
        .expect("foreign epic");
    let foreign_receipt_id = CommandReceiptId::generate();
    store
        .record_local_command(&NewLocalCommand {
            project_id,
            receipt_id: foreign_receipt_id,
            idempotency_key: IdempotencyKey::parse("foreign-epic-jira-recovery")
                .expect("foreign key"),
            kind: CommandKind::MaterializeJira,
            target: AggregateRef::MiniProject {
                mini_project_id: foreign_epic_id,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "operation": "jira_materialization_apply",
                "project_id": project_id.to_string(),
                "epic_id": epic_id.to_string(),
                "preview_hash": recovery_preview_hash.as_str(),
            }))
            .expect("foreign recovery intent"),
            created_at: now,
        })
        .expect("foreign recovery command");
    assert!(
        store
            .recover_pending_jira_materialization(
                project_id,
                epic_id,
                foreign_receipt_id,
                &recovery_preview_hash,
                &recovery_items,
                now,
            )
            .is_err(),
        "a same-project receipt for another epic has no recovery authority"
    );
    assert_eq!(
        store
            .jira_materialization_items(project_id, &original_batch_id)
            .expect("original batch remains")
            .len(),
        2,
        "foreign authority refusal must not mutate the pending batch"
    );
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
    let foreign_recoveries: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_recoveries
             WHERE recovery_receipt_id = ?1",
            [foreign_receipt_id.to_string()],
            |row| row.get(0),
        )
        .expect("foreign recovery count");
    assert_eq!(batches, 1, "recovery creates no replacement batch");
    assert_eq!(recoveries, 2, "every adopted item is durably ledgered");
    assert_eq!(
        foreign_recoveries, 0,
        "foreign authority wrote no ledger row"
    );
}

#[test]
fn recovery_adopts_exact_non_overlapping_legacy_batch_fragments_without_rewriting_them() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, first_task_id, now) = seed_graph(&store);
    let second_task_id = TaskId::generate();
    store
        .create_task(&NewTask {
            id: second_task_id,
            project_id,
            mini_project_id: Some(epic_id),
            title: kontor_core::id::ExternalName::parse("Second task").expect("title"),
            module: None,
            state: TaskState::Ready,
            created_at: now,
        })
        .expect("second task");

    let canonical_batch_id = external(uuid::Uuid::now_v7().to_string());
    let fragment_batch_id = external(uuid::Uuid::now_v7().to_string());
    let canonical_created_at: Timestamp =
        "2026-08-31T00:00:00Z".parse().expect("canonical instant");
    let epic_marker = external("kontor-epic-fragment-recovery");
    let first_task_marker = external("kontor-task-first-fragment");
    let second_task_marker = external("kontor-task-second-fragment");
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: canonical_batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "fragment-recovery-canonical".to_owned(),
                preview_hash: ContentHash::of(b"fragment-recovery-canonical"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: canonical_created_at,
            },
            &[NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: canonical_batch_id.clone(),
                project_id,
                epic_id,
                task_id: None,
                link_id: None,
                ordinal: 0,
                item_kind: JiraItemKind::Epic,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external("ASMA-8049")),
                marker: epic_marker.clone(),
            }],
        )
        .expect("first legacy fragment");
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: fragment_batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "fragment-recovery-tail".to_owned(),
                preview_hash: ContentHash::of(b"fragment-recovery-tail"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: "2026-08-31T00:00:01Z".parse().expect("later instant"),
            },
            &[
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: fragment_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: Some(first_task_id),
                    link_id: Some(TicketLinkId::generate()),
                    ordinal: 0,
                    item_kind: JiraItemKind::Task,
                    intent_kind: JiraIntentKind::Create,
                    requested_key: None,
                    marker: first_task_marker.clone(),
                },
                NewJiraMaterializationItem {
                    id: external(uuid::Uuid::now_v7().to_string()),
                    batch_id: fragment_batch_id.clone(),
                    project_id,
                    epic_id,
                    task_id: Some(second_task_id),
                    link_id: Some(TicketLinkId::generate()),
                    ordinal: 1,
                    item_kind: JiraItemKind::Task,
                    intent_kind: JiraIntentKind::Create,
                    requested_key: None,
                    marker: second_task_marker.clone(),
                },
            ],
        )
        .expect("second legacy fragment");
    drop(store);

    let connection = rusqlite::Connection::open(&path).expect("legacy database opens");
    connection
        .execute(
            "UPDATE jira_materialization_items SET ordinal = ordinal + 10 WHERE batch_id = ?1",
            [fragment_batch_id.as_str()],
        )
        .and_then(|_| {
            connection.execute(
                "UPDATE jira_materialization_items SET ordinal = ordinal - 9 WHERE batch_id = ?1",
                [fragment_batch_id.as_str()],
            )
        })
        .expect("fixture reproduces a legacy tail fragment");
    drop(connection);

    let store = SqliteStore::open(&path).expect("store reopens");
    let recovery_receipt_id = CommandReceiptId::generate();
    let recovery_preview_hash = ContentHash::of(b"complete-fragment-recovery-preview");
    store
        .record_local_command(&NewLocalCommand {
            project_id,
            receipt_id: recovery_receipt_id,
            idempotency_key: IdempotencyKey::parse("fragment-recovery-command")
                .expect("recovery key"),
            kind: CommandKind::MaterializeJira,
            target: AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "operation": "jira_materialization_apply",
                "project_id": project_id.to_string(),
                "epic_id": epic_id.to_string(),
                "preview_hash": recovery_preview_hash.as_str(),
            }))
            .expect("recovery intent"),
            created_at: now,
        })
        .expect("recovery command");
    let recovery = vec![
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
            task_id: Some(first_task_id),
            requested_key: first_task_marker.clone(),
            marker: first_task_marker,
        },
        JiraMaterializationRecoveryItem {
            ordinal: 2,
            item_kind: JiraItemKind::Task,
            task_id: Some(second_task_id),
            requested_key: second_task_marker.clone(),
            marker: second_task_marker,
        },
    ];
    assert!(
        store
            .recover_pending_jira_materialization(
                project_id,
                epic_id,
                recovery_receipt_id,
                &recovery_preview_hash,
                &recovery[..2],
                now,
            )
            .is_err(),
        "incomplete fragments must fail closed"
    );
    drop(store);
    let connection = rusqlite::Connection::open(&path).expect("incomplete refusal readback");
    let incomplete_recovery_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_recoveries
             WHERE recovery_receipt_id = ?1",
            [recovery_receipt_id.to_string()],
            |row| row.get(0),
        )
        .expect("incomplete recovery row count");
    let incomplete_batches: (i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM jira_materialization_items WHERE batch_id = ?1),
                 (SELECT count(*) FROM jira_materialization_items WHERE batch_id = ?2)",
            rusqlite::params![canonical_batch_id.as_str(), fragment_batch_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("unchanged incomplete batch ownership");
    assert_eq!(incomplete_recovery_rows, 0);
    assert_eq!(incomplete_batches, (1, 2));

    let duplicate_epic_item_id = external(uuid::Uuid::now_v7().to_string());
    connection
        .execute(
            "INSERT INTO jira_materialization_items
                 (id, batch_id, project_id, epic_id, task_id, link_id, ordinal,
                  item_kind, intent_kind, requested_key, marker, status)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0,
                     'epic', 'link', 'ASMA-8049', ?5, 'planned')",
            rusqlite::params![
                duplicate_epic_item_id.as_str(),
                fragment_batch_id.as_str(),
                project_id.to_string(),
                epic_id.to_string(),
                epic_marker.as_str(),
            ],
        )
        .expect("fixture adds an overlapping link fragment");
    drop(connection);

    let store = SqliteStore::open(&path).expect("store reopens with overlap");
    assert!(
        store
            .recover_pending_jira_materialization(
                project_id,
                epic_id,
                recovery_receipt_id,
                &recovery_preview_hash,
                &recovery,
                now,
            )
            .is_err(),
        "overlapping fragments must fail closed"
    );
    drop(store);
    let connection = rusqlite::Connection::open(&path).expect("refusal readback");
    let recovery_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_recoveries
             WHERE recovery_receipt_id = ?1",
            [recovery_receipt_id.to_string()],
            |row| row.get(0),
        )
        .expect("recovery row count");
    let unchanged_batches: (i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM jira_materialization_items WHERE batch_id = ?1),
                 (SELECT count(*) FROM jira_materialization_items WHERE batch_id = ?2)",
            rusqlite::params![canonical_batch_id.as_str(), fragment_batch_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("unchanged batch ownership");
    assert_eq!(
        recovery_rows, 0,
        "refusal persists no partial recovery ledger"
    );
    assert_eq!(unchanged_batches, (1, 3));
    connection
        .execute(
            "DELETE FROM jira_materialization_items WHERE id = ?1",
            [duplicate_epic_item_id.as_str()],
        )
        .expect("fixture removes the deliberate overlap");
    drop(connection);

    let store = SqliteStore::open(&path).expect("store reopens after fixture correction");
    let recovered = store
        .recover_pending_jira_materialization(
            project_id,
            epic_id,
            recovery_receipt_id,
            &recovery_preview_hash,
            &recovery,
            now,
        )
        .expect("fragment recovery succeeds")
        .expect("the exact fragments are found");
    assert_eq!(recovered.batch_id, canonical_batch_id);
    assert_eq!(
        recovered.batch_ids,
        vec![canonical_batch_id.clone(), fragment_batch_id.clone()]
    );
    assert_eq!(
        recovered
            .items
            .iter()
            .map(|item| item.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let replayed = store
        .recover_pending_jira_materialization(
            project_id,
            epic_id,
            recovery_receipt_id,
            &recovery_preview_hash,
            &recovery,
            now,
        )
        .expect("fragment recovery replays")
        .expect("the canonical batch remains recoverable");
    assert_eq!(replayed, recovered);
    drop(store);

    let connection = rusqlite::Connection::open(path).expect("recovery readback");
    let canonical_items: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_items WHERE batch_id = ?1",
            [canonical_batch_id.as_str()],
            |row| row.get(0),
        )
        .expect("canonical item count");
    let fragment: (String, i64) = connection
        .query_row(
            "SELECT status,
                    (SELECT count(*) FROM jira_materialization_items WHERE batch_id = ?1)
             FROM jira_materialization_batches WHERE id = ?1",
            [fragment_batch_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("fragment history");
    let provenance_batches: i64 = connection
        .query_row(
            "SELECT count(DISTINCT batch_id) FROM jira_materialization_recoveries
             WHERE recovery_receipt_id = ?1",
            [recovery_receipt_id.to_string()],
            |row| row.get(0),
        )
        .expect("recovery provenance");
    assert_eq!(canonical_items, 1);
    assert_eq!(
        fragment,
        ("planned".to_owned(), 2),
        "recovery must not rewrite or discard the original fragment"
    );
    assert_eq!(
        provenance_batches, 2,
        "the immutable recovery ledger retains both original batch identities"
    );
}

#[test]
fn recovery_postcondition_failure_rolls_back_ledger_and_legacy_items() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, _task_id, now) = seed_graph(&store);
    let batch_id = external(uuid::Uuid::now_v7().to_string());
    let item_id = external(uuid::Uuid::now_v7().to_string());
    let marker = external("kontor-epic-atomic-recovery");
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: "atomic-recovery-original".to_owned(),
                preview_hash: ContentHash::of(b"atomic-recovery-original"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[NewJiraMaterializationItem {
                id: item_id.clone(),
                batch_id: batch_id.clone(),
                project_id,
                epic_id,
                task_id: None,
                link_id: None,
                ordinal: 0,
                item_kind: JiraItemKind::Epic,
                intent_kind: JiraIntentKind::Create,
                requested_key: None,
                marker: marker.clone(),
            }],
        )
        .expect("original plan");
    let recovery_receipt_id = CommandReceiptId::generate();
    let recovery_preview_hash = ContentHash::of(b"atomic-recovery-preview");
    store
        .record_local_command(&NewLocalCommand {
            project_id,
            receipt_id: recovery_receipt_id,
            idempotency_key: IdempotencyKey::parse("atomic-recovery-command")
                .expect("recovery key"),
            kind: CommandKind::MaterializeJira,
            target: AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "operation": "jira_materialization_apply",
                "project_id": project_id.to_string(),
                "epic_id": epic_id.to_string(),
                "preview_hash": recovery_preview_hash.as_str(),
            }))
            .expect("recovery intent"),
            created_at: now,
        })
        .expect("recovery command");
    drop(store);

    let connection = rusqlite::Connection::open(&path).expect("fixture database opens");
    connection
        .execute_batch(
            "CREATE TRIGGER corrupt_recovery_postcondition
             AFTER INSERT ON jira_materialization_recoveries
             BEGIN
               UPDATE jira_materialization_items
               SET marker = 'kontor-epic-corrupted-after-ledger'
               WHERE id = NEW.item_id;
             END;",
        )
        .expect("fixture injects a postcondition mismatch");
    drop(connection);

    let store = SqliteStore::open(&path).expect("store reopens");
    let recovery = [JiraMaterializationRecoveryItem {
        ordinal: 0,
        item_kind: JiraItemKind::Epic,
        task_id: None,
        requested_key: external("ASMA-8049"),
        marker: marker.clone(),
    }];
    assert!(
        store
            .recover_pending_jira_materialization(
                project_id,
                epic_id,
                recovery_receipt_id,
                &recovery_preview_hash,
                &recovery,
                now,
            )
            .is_err(),
        "a postcondition mismatch must refuse recovery"
    );
    drop(store);

    let connection = rusqlite::Connection::open(path).expect("rollback readback");
    let stored_marker: String = connection
        .query_row(
            "SELECT marker FROM jira_materialization_items WHERE id = ?1",
            [item_id.as_str()],
            |row| row.get(0),
        )
        .expect("item marker");
    let recovery_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM jira_materialization_recoveries
             WHERE recovery_receipt_id = ?1",
            [recovery_receipt_id.to_string()],
            |row| row.get(0),
        )
        .expect("recovery row count");
    assert_eq!(stored_marker, marker.as_str());
    assert_eq!(
        recovery_rows, 0,
        "a refused postcondition must roll back the recovery ledger"
    );
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

/// Plan and confirm one epic and one task binding, each carrying its own
/// immutable Jira issue id, and return the task's link identity.
fn confirm_epic_and_task(
    store: &SqliteStore,
    project_id: ProjectId,
    epic_id: MiniProjectId,
    task_id: TaskId,
    now: Timestamp,
    epic_key: &str,
    task_key: &str,
) -> (TicketLinkId, ExternalId) {
    let batch_id = external(uuid::Uuid::now_v7().to_string());
    let link_id = TicketLinkId::generate();
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: format!("materialize-{}", uuid::Uuid::now_v7()),
                preview_hash: ContentHash::of(b"rename-preview"),
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
                    requested_key: Some(external(epic_key)),
                    marker: external("kontor-epic-rename-fixture"),
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
                    requested_key: Some(external(task_key)),
                    marker: external("kontor-task-rename-fixture"),
                },
            ],
        )
        .expect("the plan is durable");
    let items = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("planned items");
    for (item, key) in items.iter().zip([epic_key, task_key]) {
        store
            .confirm_jira_materialization_item(
                item,
                &external(key),
                &issue_id(key),
                &ContentHash::of(key.as_bytes()),
                now,
            )
            .expect("the readback confirms");
    }
    (link_id, batch_id)
}

#[test]
fn a_same_issue_rename_moves_the_key_without_moving_the_subject() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let (link_id, _batch_id) = confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );
    let before_epic = store
        .resolve_confirmed_jira_key(project_id, "ASMA-1")
        .expect("the epic resolves before the rename");
    let before_task = store
        .resolve_confirmed_jira_key(project_id, "ASMA-2")
        .expect("the task resolves before the rename");

    let renamed_epic = store
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-1"),
            &external("NEW-11"),
            &ContentHash::of(b"renamed-epic-readback"),
            now,
        )
        .expect("the same epic issue reconciles under its new key");
    let renamed_task = store
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-2"),
            &external("NEW-22"),
            &ContentHash::of(b"renamed-task-readback"),
            now,
        )
        .expect("the same task issue reconciles under its new key");

    // What the issue is called changed. Which subject it names, and the
    // revision other records were pinned against, did not.
    assert_eq!(renamed_epic.subject, JiraBindingSubject::Epic(epic_id));
    assert_eq!(renamed_epic.subject, before_epic.subject);
    assert_eq!(renamed_epic.revision, before_epic.revision);
    assert_eq!(renamed_epic.jira_key.as_str(), "NEW-11");
    assert_eq!(renamed_task.subject, JiraBindingSubject::Task(task_id));
    assert_eq!(renamed_task.subject, before_task.subject);
    assert_eq!(renamed_task.revision, before_task.revision);
    assert_eq!(renamed_task.jira_key.as_str(), "NEW-22");

    // The old keys stop resolving and the new ones take over, on both ledgers.
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "ASMA-1"),
        Err(RepositoryError::NotFound { .. })
    ));
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "ASMA-2"),
        Err(RepositoryError::NotFound { .. })
    ));
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("epic key")
            .map(|key| key.as_str().to_owned()),
        Some("NEW-11".to_owned())
    );
    assert_eq!(
        store
            .confirmed_jira_task_key(project_id, task_id)
            .expect("task key")
            .map(|key| key.as_str().to_owned()),
        Some("NEW-22".to_owned())
    );

    // The rename is not a re-link: the task keeps the one link it had.
    let links = store
        .list_task_ticket_links(project_id, task_id)
        .expect("task links");
    assert_eq!(links.len(), 1, "a rename never creates a second link");
    assert_eq!(
        links[0].id, link_id,
        "the link identity survives the rename"
    );

    // Replaying the identical reconciliation is not a second rename.
    let replayed = store
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-1"),
            &external("NEW-11"),
            &ContentHash::of(b"renamed-epic-readback"),
            now,
        )
        .expect("an exact replay settles on the same binding");
    assert_eq!(replayed.subject, renamed_epic.subject);
    assert_eq!(replayed.jira_key.as_str(), "NEW-11");

    // Restart evidence: the identity survives closing and reopening the file.
    drop(store);
    let reopened = SqliteStore::open(&path).expect("the store reopens");
    let after_restart = reopened
        .resolve_confirmed_jira_key(project_id, "NEW-11")
        .expect("the renamed epic still resolves after a restart");
    assert_eq!(after_restart.subject, JiraBindingSubject::Epic(epic_id));
    assert_eq!(
        reopened
            .resolve_confirmed_jira_key(project_id, "NEW-22")
            .expect("the renamed task still resolves after a restart")
            .subject,
        JiraBindingSubject::Task(task_id)
    );
}

#[test]
fn a_different_immutable_issue_cannot_take_over_a_confirmed_binding() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let (_link_id, batch_id) = confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    // An exact replay of the key and hash, but reporting another immutable id,
    // is a different Jira issue wearing a familiar key. The matching key is not
    // permission to adopt it.
    let items = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("confirmed items");
    assert!(
        matches!(
            store.confirm_jira_materialization_item(
                &items[0],
                &external("ASMA-1"),
                &external("909999"),
                &ContentHash::of(b"ASMA-1"),
                now,
            ),
            Err(RepositoryError::Conflict { .. })
        ),
        "a replay whose readback names another immutable issue is refused"
    );

    // No binding carries this immutable id, so there is nothing to rename.
    assert!(matches!(
        store.reconcile_confirmed_jira_key(
            project_id,
            &external("90404"),
            &external("NEW-11"),
            &ContentHash::of(b"unknown-issue"),
            now,
        ),
        Err(RepositoryError::NotFound { .. })
    ));

    // Two distinct issues cannot converge on one key. The epic's issue may not
    // take the key the task's issue already holds; this is also the shape a
    // lost concurrent race takes, with the ledger constraint as the boundary.
    assert!(matches!(
        store.reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-1"),
            &external("ASMA-2"),
            &ContentHash::of(b"colliding-rename"),
            now,
        ),
        Err(RepositoryError::Conflict { .. })
    ));
    // The refused rename left both bindings exactly as they were.
    assert_eq!(
        store
            .resolve_confirmed_jira_key(project_id, "ASMA-1")
            .expect("the epic is untouched")
            .subject,
        JiraBindingSubject::Epic(epic_id)
    );
    assert_eq!(
        store
            .resolve_confirmed_jira_key(project_id, "ASMA-2")
            .expect("the task is untouched")
            .subject,
        JiraBindingSubject::Task(task_id)
    );
}

#[test]
fn a_binding_without_an_immutable_issue_stays_fail_closed_for_renames() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let (_link_id, _batch_id) = confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    // Reduce the epic binding to its pre-v94 shape. Dropping the guard first is
    // the only way to reach that state, which is itself the point: an
    // established id cannot be erased through the supported path.
    let connection = rusqlite::Connection::open(&path).expect("database opens directly");
    connection
        .execute_batch("DROP TRIGGER jira_epic_binding_issue_id_immutable;")
        .expect("the guard is removed for the fixture");
    connection
        .execute(
            "UPDATE jira_epic_bindings SET external_issue_id = NULL
             WHERE project_id = ?1 AND epic_id = ?2",
            rusqlite::params![project_id.to_string(), epic_id.to_string()],
        )
        .expect("the legacy shape is planted");
    drop(connection);

    // The binding still resolves — it is confirmed evidence — but it can no
    // longer prove sameness, so no key change is authorized through it.
    assert_eq!(
        store
            .resolve_confirmed_jira_key(project_id, "ASMA-1")
            .expect("a legacy binding still resolves")
            .subject,
        JiraBindingSubject::Epic(epic_id)
    );
    assert!(
        matches!(
            store.reconcile_confirmed_jira_key(
                project_id,
                &issue_id("ASMA-1"),
                &external("NEW-11"),
                &ContentHash::of(b"legacy-rename"),
                now,
            ),
            Err(RepositoryError::NotFound { .. })
        ),
        "a migrated binding without an immutable id cannot authorize a rename"
    );
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("epic key")
            .map(|key| key.as_str().to_owned()),
        Some("ASMA-1".to_owned()),
        "the refused rename changed nothing"
    );
}

#[test]
fn a_canonical_task_key_cannot_be_changed_by_direct_sql() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let (link_id, _batch_id) = confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    let connection = rusqlite::Connection::open(&path).expect("database opens directly");
    // A key change that the link ledger does not corroborate is not the tail of
    // a proven rename, whatever it claims.
    assert!(
        connection
            .execute(
                "UPDATE canonical_jira_task_links SET external_issue_key = 'NEW-99'
                 WHERE project_id = ?1 AND task_id = ?2",
                rusqlite::params![project_id.to_string(), task_id.to_string()],
            )
            .is_err(),
        "an uncorroborated canonical key change is refused"
    );
    // Identity itself is still frozen outright.
    assert!(
        connection
            .execute(
                "UPDATE canonical_jira_task_links SET link_id = ?3
                 WHERE project_id = ?1 AND task_id = ?2",
                rusqlite::params![
                    project_id.to_string(),
                    task_id.to_string(),
                    TicketLinkId::generate().to_string()
                ],
            )
            .is_err(),
        "a canonical link may not be repointed at another link"
    );
    assert!(
        connection
            .execute(
                "DELETE FROM canonical_jira_task_links WHERE project_id = ?1 AND task_id = ?2",
                rusqlite::params![project_id.to_string(), task_id.to_string()],
            )
            .is_err(),
        "canonical Jira task links remain permanent"
    );
    // An established immutable id is refused a rewrite through the guard.
    assert!(
        connection
            .execute(
                "UPDATE jira_task_binding_confirmations SET external_issue_id = '909999'
                 WHERE project_id = ?1 AND link_id = ?2",
                rusqlite::params![project_id.to_string(), link_id.to_string()],
            )
            .is_err(),
        "an established immutable Jira issue id is never rewritten"
    );
    drop(connection);

    assert_eq!(
        store
            .confirmed_jira_task_key(project_id, task_id)
            .expect("task key")
            .map(|key| key.as_str().to_owned()),
        Some("ASMA-2".to_owned()),
        "every refused write left the binding as it was"
    );
}

#[test]
fn two_direct_sql_updates_cannot_forge_the_tail_of_a_proven_rename() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let (link_id, _batch) = confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    // The whole forge, in one raw transaction: update the mutable link row to
    // the new key first, then update the canonical row to match it. An earlier
    // guard treated that agreement as proof, which meant two ordinary updates
    // could mint a rename nobody ever read back from Jira.
    let mut connection = rusqlite::Connection::open(&path).expect("database opens directly");
    let forge = connection
        .transaction()
        .expect("the forging transaction starts");
    forge
        .execute(
            "UPDATE jira_links SET external_issue_key = 'FORGED-9'
             WHERE project_id = ?1 AND id = ?2",
            rusqlite::params![project_id.to_string(), link_id.to_string()],
        )
        .expect("the mutable link row is writable on its own");
    let refused = forge.execute(
        "UPDATE canonical_jira_task_links SET external_issue_key = 'FORGED-9'
         WHERE project_id = ?1 AND task_id = ?2",
        rusqlite::params![project_id.to_string(), task_id.to_string()],
    );
    assert!(
        refused.is_err(),
        "direct SQL must not forge the tail of a proven same-issue rename"
    );
    drop(forge);
    drop(connection);

    // Nothing moved: the task still answers to the key it was confirmed under.
    assert_eq!(
        store
            .confirmed_jira_task_key(project_id, task_id)
            .expect("task key")
            .map(|key| key.as_str().to_owned()),
        Some("ASMA-2".to_owned())
    );
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "FORGED-9"),
        Err(RepositoryError::NotFound { .. })
    ));

    // And the supported path still works, which is what makes the guard a
    // guard rather than a wall.
    let renamed = store
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-2"),
            &external("NEW-22"),
            &ContentHash::of(b"authorized-rename"),
            now,
        )
        .expect("an authorized same-issue rename still succeeds");
    assert_eq!(renamed.subject, JiraBindingSubject::Task(task_id));
    assert_eq!(renamed.jira_key.as_str(), "NEW-22");
}

#[test]
fn resolution_is_project_scoped_and_never_reaches_a_foreign_projects_binding() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    let (other_project, other_epic, other_task, _) = seed_named_graph(&store, "Other");
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );
    confirm_epic_and_task(
        &store,
        other_project,
        other_epic,
        other_task,
        now,
        "ASMA-3",
        "ASMA-4",
    );

    // Each project sees only its own confirmed bindings. A key that is real,
    // confirmed and unambiguous *somewhere else* is simply absent here, and
    // must not resolve across the boundary on the strength of being well formed.
    assert!(matches!(
        store.resolve_confirmed_jira_key(project_id, "ASMA-3"),
        Err(RepositoryError::NotFound { .. })
    ));
    assert!(matches!(
        store.resolve_confirmed_jira_key(other_project, "ASMA-1"),
        Err(RepositoryError::NotFound { .. })
    ));
    assert_eq!(
        store
            .resolve_confirmed_jira_key(other_project, "ASMA-3")
            .expect("its own project still resolves it")
            .subject,
        JiraBindingSubject::Epic(other_epic)
    );
}

#[test]
fn an_ambiguous_cross_ledger_key_is_a_typed_conflict_rather_than_a_backend_error() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    // Corrupt the ledgers behind the application guard so one key names both an
    // epic and a task. The guard makes this unreachable through the supported
    // path, which is exactly why the resolver must still answer for it: a
    // migrated or hand-edited database is the case where an ambiguous read
    // would otherwise escape as a bare `rusqlite` row error.
    let connection = rusqlite::Connection::open(&path).expect("database opens directly");
    connection
        .execute(
            "UPDATE jira_epic_bindings SET external_issue_key = 'ASMA-2'
             WHERE project_id = ?1 AND epic_id = ?2",
            rusqlite::params![project_id.to_string(), epic_id.to_string()],
        )
        .expect("the ambiguous state is planted");
    drop(connection);

    let refusal = store.resolve_confirmed_jira_key(project_id, "ASMA-2");
    assert!(
        matches!(refusal, Err(RepositoryError::Conflict { .. })),
        "an ambiguous key is a typed domain conflict, not a backend error: {refusal:?}"
    );
    // The same ambiguity must not leak through the per-subject readers either.
    assert!(matches!(
        store.confirmed_jira_task_key(project_id, task_id),
        Err(RepositoryError::Conflict { .. })
    ));
}

#[test]
fn resolution_returns_the_immutable_kontor_uuid_and_reads_legacy_state_without_writing() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    // The resolver's answer is the Kontor identity other records point at, not
    // the Jira spelling that happens to be current.
    let epic = store
        .resolve_confirmed_jira_key(project_id, "ASMA-1")
        .expect("epic resolves");
    assert_eq!(epic.subject, JiraBindingSubject::Epic(epic_id));
    assert_eq!(epic.project_id, project_id);
    assert_eq!(epic.jira_key.as_str(), "ASMA-1");
    let task = store
        .resolve_confirmed_jira_key(project_id, "ASMA-2")
        .expect("task resolves");
    assert_eq!(task.subject, JiraBindingSubject::Task(task_id));

    // Evidence and revision travel with the answer: a caller can prove what was
    // read back and against which aggregate revision, without a second query.
    assert_eq!(
        epic.readback_hash,
        ContentHash::of(b"ASMA-1"),
        "the durable readback evidence is the resolver's own output"
    );
    assert_eq!(epic.confirmed_at, now);

    // Reading is read-only. Resolving repeatedly changes no row, so a lookup
    // can never become the thing that establishes a binding.
    let before = binding_fingerprint(&path, project_id);
    for _ in 0..3 {
        store
            .resolve_confirmed_jira_key(project_id, "ASMA-1")
            .expect("repeated resolution");
        store
            .jira_task_binding_state(project_id, task_id)
            .expect("repeated state read");
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("repeated legacy-compatible read");
    }
    assert_eq!(
        binding_fingerprint(&path, project_id),
        before,
        "resolution and the legacy-compatible readers never write"
    );
}

/// Every confirmed-binding row, as raw text, for proving a read wrote nothing.
fn binding_fingerprint(path: &std::path::Path, project_id: ProjectId) -> Vec<String> {
    let connection = rusqlite::Connection::open(path).expect("database opens directly");
    let mut rows = Vec::new();
    let mut statement = connection
        .prepare(
            "SELECT epic_id, external_issue_key, external_issue_id, readback_hash, confirmed_at
             FROM jira_epic_bindings WHERE project_id = ?1 ORDER BY epic_id",
        )
        .expect("epic bindings readable");
    let mut cursor = statement
        .query(rusqlite::params![project_id.to_string()])
        .expect("query");
    while let Some(row) = cursor.next().expect("row") {
        rows.push(format!(
            "epic:{:?}:{:?}:{:?}:{:?}:{:?}",
            row.get::<_, String>(0).ok(),
            row.get::<_, String>(1).ok(),
            row.get::<_, Option<String>>(2).ok(),
            row.get::<_, String>(3).ok(),
            row.get::<_, String>(4).ok(),
        ));
    }
    drop(cursor);
    drop(statement);
    let mut statement = connection
        .prepare(
            "SELECT link_id, external_issue_id, readback_hash, confirmed_at
             FROM jira_task_binding_confirmations WHERE project_id = ?1 ORDER BY link_id",
        )
        .expect("task confirmations readable");
    let mut cursor = statement
        .query(rusqlite::params![project_id.to_string()])
        .expect("query");
    while let Some(row) = cursor.next().expect("row") {
        rows.push(format!(
            "task:{:?}:{:?}:{:?}:{:?}",
            row.get::<_, String>(0).ok(),
            row.get::<_, Option<String>>(1).ok(),
            row.get::<_, String>(2).ok(),
            row.get::<_, String>(3).ok(),
        ));
    }
    rows
}

#[test]
fn a_confirmed_binding_and_its_immutable_issue_survive_a_backup_and_restore() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );
    // Rename first, so the backup carries a binding whose current key differs
    // from the one it was originally confirmed under. A backup that silently
    // restored the original key would still look plausible without this.
    store
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-1"),
            &external("NEW-11"),
            &ContentHash::of(b"renamed-epic-readback"),
            now,
        )
        .expect("the epic issue reconciles");

    let outcome = kontor_store::backup::create_snapshot(
        &path,
        &root.path().join("backups"),
        Timestamp::now(),
    )
    .expect("the snapshot is published");

    let restored = SqliteStore::open(&outcome.snapshot).expect("the snapshot reopens");
    let epic = restored
        .resolve_confirmed_jira_key(project_id, "NEW-11")
        .expect("the renamed epic resolves out of the backup");
    assert_eq!(epic.subject, JiraBindingSubject::Epic(epic_id));
    assert_eq!(
        restored
            .resolve_confirmed_jira_key(project_id, "ASMA-2")
            .expect("the task resolves out of the backup")
            .subject,
        JiraBindingSubject::Task(task_id)
    );
    assert!(
        matches!(
            restored.resolve_confirmed_jira_key(project_id, "ASMA-1"),
            Err(RepositoryError::NotFound { .. })
        ),
        "the superseded key does not come back to life in a restore"
    );

    // The immutable identity survived too, which is what lets the restored
    // Realm keep reconciling: a backup that dropped it would leave every
    // binding fail-closed against its next rename.
    let renamed_again = restored
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-1"),
            &external("NEW-12"),
            &ContentHash::of(b"renamed-again"),
            now,
        )
        .expect("the restored binding still proves its immutable issue");
    assert_eq!(renamed_again.subject, JiraBindingSubject::Epic(epic_id));
}

#[test]
fn two_issues_racing_for_one_key_settle_on_exactly_one_typed_winner() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );
    drop(store);

    // Two different immutable issues, each reconciling onto the same new key at
    // the same moment. One confirmed Jira issue may name only one subject, so
    // exactly one of these may win — and the loser must lose in the domain's
    // own vocabulary rather than as a raw uniqueness failure from the backend.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let racers: Vec<_> = ["ASMA-1", "ASMA-2"]
        .into_iter()
        .map(|key| {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let store = SqliteStore::open(&path).expect("the racer opens the store");
                barrier.wait();
                store.reconcile_confirmed_jira_key(
                    project_id,
                    &issue_id(key),
                    &external("NEW-77"),
                    &ContentHash::of(b"contested-rename"),
                    now,
                )
            })
        })
        .collect();
    let outcomes: Vec<_> = racers
        .into_iter()
        .map(|racer| racer.join().expect("the racer does not panic"))
        .collect();

    let winners = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    assert_eq!(
        winners, 1,
        "exactly one issue may hold the key: {outcomes:?}"
    );
    for outcome in &outcomes {
        if let Err(error) = outcome {
            assert!(
                matches!(error, RepositoryError::Conflict { .. }),
                "the loser is refused in typed terms, not as a backend error: {error:?}"
            );
        }
    }

    // The realm is left consistent: the key names exactly one subject, and the
    // issue that lost still holds the binding it arrived with.
    let store = SqliteStore::open(&path).expect("store reopens");
    let held = store
        .resolve_confirmed_jira_key(project_id, "NEW-77")
        .expect("the contested key resolves to its one winner");
    assert!(
        held.subject == JiraBindingSubject::Epic(epic_id)
            || held.subject == JiraBindingSubject::Task(task_id)
    );
    let survivor = match held.subject {
        JiraBindingSubject::Epic(_) => store.resolve_confirmed_jira_key(project_id, "ASMA-2"),
        JiraBindingSubject::Task(_) => store.resolve_confirmed_jira_key(project_id, "ASMA-1"),
    };
    assert!(
        survivor.is_ok(),
        "the issue that lost the race keeps its original binding: {survivor:?}"
    );
}

/// Plan and confirm one epic-only binding, returning its batch.
fn confirm_epic_only(
    store: &SqliteStore,
    project_id: ProjectId,
    epic_id: MiniProjectId,
    now: Timestamp,
    key: &str,
) -> ExternalId {
    let batch_id = external(uuid::Uuid::now_v7().to_string());
    store
        .plan_jira_materialization(
            &NewJiraMaterializationBatch {
                id: batch_id.clone(),
                project_id,
                epic_id,
                idempotency_key: format!("epic-only-{}", uuid::Uuid::now_v7()),
                preview_hash: ContentHash::of(b"epic-only-preview"),
                expected_revision: AggregateRevision::INITIAL,
                created_at: now,
            },
            &[NewJiraMaterializationItem {
                id: external(uuid::Uuid::now_v7().to_string()),
                batch_id: batch_id.clone(),
                project_id,
                epic_id,
                task_id: None,
                link_id: None,
                ordinal: 0,
                item_kind: JiraItemKind::Epic,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(external(key)),
                marker: external(format!("kontor-epic-{key}")),
            }],
        )
        .expect("the plan is durable");
    let item = store
        .jira_materialization_items(project_id, &batch_id)
        .expect("planned item")
        .remove(0);
    store
        .confirm_jira_materialization_item(
            &item,
            &external(key),
            &issue_id(key),
            &ContentHash::of(key.as_bytes()),
            now,
        )
        .expect("the readback confirms");
    batch_id
}

#[test]
fn a_uniqueness_violation_establishing_an_immutable_issue_is_a_typed_conflict() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, first_epic, _task_id, now) = seed_graph(&store);
    let second_epic = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: second_epic,
            project_id,
            name: kontor_core::id::ExternalName::parse("Second epic").expect("name"),
            created_at: now,
        })
        .expect("second epic");
    let first_batch = confirm_epic_only(&store, project_id, first_epic, now, "ASMA-1");
    let second_batch = confirm_epic_only(&store, project_id, second_epic, now, "ASMA-2");

    // Reduce both bindings to their pre-v95 shape, so each one's next supported
    // readback is what establishes its immutable id. That write is the one with
    // no application pre-check in front of it: nothing has claimed the id yet,
    // so only the ledger's own uniqueness can refuse a second claim on it.
    let connection = rusqlite::Connection::open(&path).expect("database opens directly");
    connection
        .execute_batch("DROP TRIGGER jira_epic_binding_issue_id_immutable;")
        .expect("the guard is removed for the fixture");
    connection
        .execute(
            "UPDATE jira_epic_bindings SET external_issue_id = NULL WHERE project_id = ?1",
            rusqlite::params![project_id.to_string()],
        )
        .expect("the legacy shape is planted");
    drop(connection);

    let contested = external("905000");
    let second_item = store
        .jira_materialization_items(project_id, &second_batch)
        .expect("second epic item")
        .remove(0);
    store
        .confirm_jira_materialization_item(
            &second_item,
            &external("ASMA-2"),
            &contested,
            &ContentHash::of(b"ASMA-2"),
            now,
        )
        .expect("the supported readback establishes the immutable id");

    // A different subject now claims the very same immutable issue. This
    // reaches the ledger's uniqueness index, and must surface as a domain
    // conflict rather than as a backend error carrying SQLite's own text.
    let first_item = store
        .jira_materialization_items(project_id, &first_batch)
        .expect("first epic item")
        .remove(0);
    let refusal = store.confirm_jira_materialization_item(
        &first_item,
        &external("ASMA-1"),
        &contested,
        &ContentHash::of(b"ASMA-1"),
        now,
    );
    // Naming the subject is the point. `backend` already turns any constraint
    // violation into a generic `storage` conflict, so asserting merely that
    // this is *a* conflict would pass even if the domain mapping were deleted.
    // What the caller needs is which rule refused and what to do about it.
    match &refusal {
        Err(RepositoryError::Conflict { subject, rule }) => {
            assert_eq!(
                *subject, "confirmed Jira binding",
                "the refusal names the domain subject, not the storage layer: {refusal:?}"
            );
            assert!(
                rule.contains("immutable Jira issue"),
                "the refusal states the rule that refused: {rule}"
            );
        }
        other => panic!("a uniqueness violation must be a typed domain conflict: {other:?}"),
    }
}

#[test]
fn a_legacy_exact_replay_cannot_claim_an_issue_already_bound_in_the_other_ledger() {
    for (legacy_subject, contested_from) in [("epic", "ASMA-2"), ("task", "ASMA-1")] {
        let root = tempfile::tempdir().expect("state root");
        let path = root.path().join("kontor.db");
        let store = SqliteStore::open(&path).expect("store opens");
        let (project_id, epic_id, task_id, now) = seed_graph(&store);
        let (_link_id, batch_id) = confirm_epic_and_task(
            &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
        );

        // Reduce only the replaying subject to the migrated pre-v95 shape, so
        // its exact replay takes the branch that establishes an id.
        let connection = rusqlite::Connection::open(&path).expect("database opens directly");
        connection
            .execute_batch(
                "DROP TRIGGER jira_epic_binding_issue_id_immutable;
                 DROP TRIGGER jira_task_binding_issue_id_immutable;",
            )
            .expect("guards removed for the fixture");
        let table = if legacy_subject == "epic" {
            "jira_epic_bindings"
        } else {
            "jira_task_binding_confirmations"
        };
        connection
            .execute(
                &format!("UPDATE {table} SET external_issue_id = NULL WHERE project_id = ?1"),
                rusqlite::params![project_id.to_string()],
            )
            .expect("the legacy shape is planted");
        drop(connection);

        // Replay the legacy subject's own exact key and hash, but present the
        // immutable issue the *other* ledger already holds. One confirmed Jira
        // issue may name only one subject, and an exact replay is not a licence
        // to break that.
        let items = store
            .jira_materialization_items(project_id, &batch_id)
            .expect("confirmed items");
        let item = items
            .iter()
            .find(|item| (legacy_subject == "epic") == (item.item_kind == JiraItemKind::Epic))
            .expect("the replaying subject's item");
        let own_key = item
            .confirmed_key
            .as_ref()
            .map(ExternalId::as_str)
            .expect("a confirmed key")
            .to_owned();
        let refusal = store.confirm_jira_materialization_item(
            item,
            &external(&own_key),
            &issue_id(contested_from),
            &ContentHash::of(own_key.as_bytes()),
            now,
        );
        assert!(
            matches!(refusal, Err(RepositoryError::Conflict { .. })),
            "a legacy {legacy_subject} replay must not claim the immutable issue of {contested_from}: {refusal:?}"
        );

        // And nothing was committed: the contested issue still names exactly
        // one subject.
        let connection = rusqlite::Connection::open(&path).expect("database opens directly");
        let holders: i64 = connection
            .query_row(
                "SELECT (SELECT count(*) FROM jira_epic_bindings
                          WHERE project_id = ?1 AND external_issue_id = ?2)
                      + (SELECT count(*) FROM jira_task_binding_confirmations
                          WHERE project_id = ?1 AND external_issue_id = ?2)",
                rusqlite::params![project_id.to_string(), issue_id(contested_from).as_str()],
                |row| row.get(0),
            )
            .expect("holders readable");
        assert_eq!(
            holders, 1,
            "the contested immutable issue still names exactly one subject"
        );
    }
}

#[test]
fn a_committed_rename_is_never_reported_as_a_failure_by_its_own_caller() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );
    drop(store);

    // Two writers renaming the *same* immutable issue, repeatedly. Renaming an
    // issue twice is legitimate, so both may commit; what must never happen is
    // a caller committing a rename and then being told about somebody else's.
    //
    // The window this guards is small — it opens only between a commit and a
    // read taken after it — so one interleaving proves nothing. Sustained
    // contention is what makes the window reachable, and the assertion is a
    // property every single round has to satisfy.
    const ROUNDS: usize = 40;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let racers: Vec<_> = ["AAA", "BBB"]
        .into_iter()
        .map(|tag| {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let store = SqliteStore::open(&path).expect("the racer opens the store");
                barrier.wait();
                let mut committed = 0_usize;
                for round in 0..ROUNDS {
                    let requested = format!("{tag}-{}", round + 1);
                    match store.reconcile_confirmed_jira_key(
                        project_id,
                        &issue_id("ASMA-1"),
                        &external(&requested),
                        &ContentHash::of(requested.as_bytes()),
                        now,
                    ) {
                        Ok(binding) => {
                            committed += 1;
                            assert_eq!(
                                binding.jira_key.as_str(),
                                requested,
                                "a caller that committed a rename is told its own key, not a later one"
                            );
                            assert_eq!(
                                binding.subject,
                                JiraBindingSubject::Epic(epic_id),
                                "a rename never moves the binding to another subject"
                            );
                        }
                        // Losing the key to the other writer is legitimate.
                        Err(RepositoryError::Conflict { .. }) => {}
                        // `NotFound` is not. This fixture always has a binding
                        // carrying the immutable issue, so the only way to see
                        // it is a caller reading *after* its own commit and
                        // finding the key already renamed by somebody else —
                        // reporting a failure for a write that succeeded.
                        Err(RepositoryError::NotFound { .. }) => panic!(
                            "a committed rename was reported as NotFound: the answer was read \
                             after the commit instead of inside it"
                        ),
                        other => panic!("unexpected refusal shape: {other:?}"),
                    }
                }
                committed
            })
        })
        .collect();

    let committed: usize = racers
        .into_iter()
        .map(|racer| racer.join().expect("the racer does not panic"))
        .sum();
    assert!(
        committed > 0,
        "the contention fixture must actually commit renames"
    );

    // The realm settles coherently: the immutable issue holds exactly one key,
    // and it still names the epic it started on.
    let store = SqliteStore::open(&path).expect("store reopens");
    let state = store
        .jira_epic_binding_state(project_id, epic_id)
        .expect("the epic still has a readable binding state");
    match state {
        JiraBindingState::Confirmed(binding) => {
            assert_eq!(binding.subject, JiraBindingSubject::Epic(epic_id));
            assert_eq!(
                store
                    .resolve_confirmed_jira_key(project_id, binding.jira_key.as_str())
                    .expect("the settled key resolves")
                    .subject,
                JiraBindingSubject::Epic(epic_id)
            );
        }
        JiraBindingState::AwaitingJiraBinding => {
            panic!("a confirmed binding never becomes awaiting through renames")
        }
    }
}

#[test]
fn a_rename_reports_what_it_committed_even_when_the_row_moves_underneath_it() {
    let root = tempfile::tempdir().expect("state root");
    let path = root.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens");
    let (project_id, epic_id, task_id, now) = seed_graph(&store);
    confirm_epic_and_task(
        &store, project_id, epic_id, task_id, now, "ASMA-1", "ASMA-2",
    );

    // Stand in for the competing writer, deterministically. A real race puts
    // another rename between this caller's commit and a read taken after it —
    // a window of microseconds that no test can schedule. This trigger makes
    // the same thing true of the durable row without depending on timing: the
    // key this caller committed is not the key the table holds afterwards.
    //
    // Nothing in production is relaxed to arrange it. The trigger lives only in
    // this fixture, and the property under test is exactly the one the window
    // threatens: a caller must be told about its own write.
    let connection = rusqlite::Connection::open(&path).expect("database opens directly");
    connection
        .execute_batch(
            "CREATE TRIGGER test_competing_writer
             AFTER UPDATE OF external_issue_key ON jira_epic_bindings
             WHEN NEW.external_issue_key = 'RACE-1'
             BEGIN
                 UPDATE jira_epic_bindings SET external_issue_key = 'RACE-2'
                 WHERE project_id = NEW.project_id AND epic_id = NEW.epic_id;
             END;",
        )
        .expect("the competing writer is installed");
    drop(connection);

    let settled = store
        .reconcile_confirmed_jira_key(
            project_id,
            &issue_id("ASMA-1"),
            &external("RACE-1"),
            &ContentHash::of(b"raced-rename"),
            now,
        )
        .expect("a committed rename is never reported as a failure");

    // The answer describes this caller's own write, derived inside the
    // transaction that made it. Reading it back afterwards would find `RACE-2`
    // and report `NotFound` for a rename that in fact succeeded.
    assert_eq!(settled.jira_key.as_str(), "RACE-1");
    assert_eq!(settled.subject, JiraBindingSubject::Epic(epic_id));

    // And the durable row really did move on, so the fixture proved what it
    // claims rather than quietly agreeing with the caller.
    assert_eq!(
        store
            .confirmed_jira_epic_key(project_id, epic_id)
            .expect("epic key")
            .map(|key| key.as_str().to_owned()),
        Some("RACE-2".to_owned()),
        "the competing writer really did move the row"
    );
}
