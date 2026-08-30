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
    JiraIntentKind, JiraItemKind, NewJiraMaterializationBatch, NewJiraMaterializationItem,
    SqliteStore,
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
