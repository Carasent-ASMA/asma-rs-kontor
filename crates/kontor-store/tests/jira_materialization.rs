//! Durable Jira materialization and activation behavior.

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId,
    IdempotencyKey, MiniProjectId, ProjectId, TaskId, TicketLinkId, Timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CommandRepository, NewLocalCommand, NewMiniProject, NewProject, NewTask, ProjectRepository,
};
use kontor_core::state::TaskState;
use kontor_store::{
    JiraIntentKind, JiraItemKind, NewJiraMaterializationBatch, NewJiraMaterializationItem,
    SqliteStore,
};

fn external(value: impl AsRef<str>) -> ExternalId {
    ExternalId::parse(value.as_ref()).expect("external id")
}

#[test]
fn activation_requires_every_confirmed_binding_and_survives_readback() {
    let root = tempfile::tempdir().expect("state root");
    let store = SqliteStore::open(&root.path().join("kontor.db")).expect("store opens");
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
    store
        .confirm_jira_materialization_batch(project_id, &batch_id, now)
        .expect("batch confirms");
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
