//! Checkout intent/claim atomicity, immutable evidence and cross-task collision guards.
use kontor_core::id::{AggregateRevision, CanonicalDocument, ExternalName, ProjectId, TaskId};
use kontor_core::repository::{NewProject, NewTask, ProjectRepository};
use kontor_core::state::TaskState;
use kontor_store::SqliteStore;
use kontor_store::checkouts::TaskCheckoutPlacement;
use rusqlite::Connection;

fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).unwrap()
}
fn seed(store: &SqliteStore, project: ProjectId) -> TaskId {
    let at = kontor_core::id::Timestamp::now();
    if store.get_project(project).unwrap().is_none() {
        store
            .create_project(&NewProject {
                id: project,
                name: name(&format!("Example {project}")),
                root_path: name(&format!("/fixture/{project}")),
                created_at: at,
            })
            .unwrap();
    }
    let id = TaskId::generate();
    store
        .create_task(&NewTask {
            id,
            project_id: project,
            mini_project_id: None,
            title: name("Task"),
            module: None,
            state: TaskState::Ready,
            created_at: at,
        })
        .unwrap();
    id
}
fn placement(project_id: ProjectId, task_id: TaskId, worktree: &str) -> TaskCheckoutPlacement {
    TaskCheckoutPlacement { project_id, task_id, worktree:name(worktree), destination:name(worktree), task_revision:AggregateRevision::INITIAL, module:None,
        intent:CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"claim":worktree,"destination":worktree,"module":null,"source":{"base_commit":"frozen"}})).unwrap(), readback:None }
}

#[test]
fn intent_is_durable_immutable_and_first_readback_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let store = SqliteStore::open(&path).unwrap();
    let project = ProjectId::generate();
    let task = seed(&store, project);
    let pin = placement(project, task, "/fixture/work");
    store
        .set_task_worktree(project, task, &pin.worktree)
        .unwrap();
    let stored = store
        .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
        .unwrap();
    assert!(stored.readback.is_none());
    let mut changed = pin.clone();
    changed.intent=CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"claim":pin.worktree.as_str(),"destination":pin.destination.as_str(),"module":null,"source":"edited host"})).unwrap();
    assert!(
        store
            .freeze_task_checkout(&changed, kontor_core::id::Timestamp::now())
            .unwrap_err()
            .to_string()
            .contains("checkout.source_pin_conflict")
    );
    let bad = CanonicalDocument::from_value(
        &serde_json::json!({"schema_version":1,"intent_hash":"wrong"}),
    )
    .unwrap();
    assert!(store.confirm_task_checkout(&pin, &bad).is_err());
    let readback=CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"intent_hash":pin.intent.hash(),"observed":{"branch":"feat/EXM-2-task"}})).unwrap();
    store.confirm_task_checkout(&pin, &readback).unwrap();
    store.confirm_task_checkout(&pin, &readback).unwrap();
    drop(store);
    let sql = Connection::open(&path).unwrap();
    assert!(
        sql.execute("UPDATE task_checkout_placements SET intent = '{}'", [])
            .is_err()
    );
    assert!(
        sql.execute("UPDATE task_checkout_placements SET readback = '{}'", [])
            .is_err()
    );
    assert!(
        sql.execute("DELETE FROM task_checkout_placements", [])
            .is_err()
    );
    drop(sql);
    let store = SqliteStore::open(&path).unwrap();
    let loaded = store
        .task_checkout_placement(project, task, &pin.worktree)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.intent, pin.intent);
    assert_eq!(loaded.readback, Some(readback));
}

#[test]
fn stale_pending_intent_and_other_task_claims_refuse_before_effect() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("store.db")).unwrap();
    let project = ProjectId::generate();
    let task = seed(&store, project);
    let pin = placement(project, task, "/fixture/work");
    assert!(
        store
            .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
            .is_err()
    );
    store
        .set_task_worktree(project, task, &pin.worktree)
        .unwrap();
    let mut stale = pin.clone();
    stale.task_revision = AggregateRevision::parse(2).unwrap();
    assert!(
        store
            .freeze_task_checkout(&stale, kontor_core::id::Timestamp::now())
            .is_err()
    );
    store
        .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
        .unwrap();
    let other = seed(&store, ProjectId::generate());
    let foreign = store.checkout_claims().unwrap();
    assert_eq!(foreign.len(), 1);
    // Cross-project claims are rejected even though old claim uniqueness was project-scoped.
    let sql_project = store
        .list_projects()
        .unwrap()
        .into_iter()
        .find(|row| row.project_id != project)
        .unwrap()
        .project_id;
    assert!(
        store
            .set_task_worktree(sql_project, other, &pin.destination)
            .is_err()
    );
    store
        .set_task_worktree(project, task, &name("/fixture/new-claim"))
        .unwrap();
    assert!(
        store
            .guard_task_checkout(&pin)
            .unwrap_err()
            .to_string()
            .contains("checkout.stale_claim_or_intent")
    );
}

fn native_container(
    store: &SqliteStore,
    project: ProjectId,
    cwd: &str,
) -> kontor_core::repository::NewNativeContainerBinding {
    use kontor_core::repository::{NewSessionTopologyNode, TopologyRepository};
    let topology = kontor_profiles::bundled_operational_domain()
        .unwrap()
        .topology_specs
        .remove(0);
    let stamp = kontor_core::spec::Shareability::default_for(
        kontor_core::spec::ShareabilityTier::ProjectKnowledge,
    )
    .unwrap();
    let at = kontor_core::id::Timestamp::now();
    let hash = store
        .publish_topology_spec(project, &topology, &stamp, at)
        .unwrap();
    let id = kontor_core::id::TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id,
            project_id: project,
            mini_project_id: None,
            topology: kontor_core::spec::TopologySnapshot {
                spec_id: topology.spec_id,
                version: topology.version,
                canonical_hash: hash,
            },
            kind: topology.root_kind,
            parent_id: None,
            task_id: None,
            created_at: at,
        })
        .unwrap();
    kontor_core::repository::NewNativeContainerBinding {
        topology_node_id: id,
        project_id: project,
        container_binding_id: kontor_core::id::ExternalId::parse(&id.to_string()).unwrap(),
        identity: kontor_core::state::NativeRuntimeIdentity {
            runtime_kind: kontor_core::id::RuntimeKindKey::parse("fake.runtime").unwrap(),
            host: name("fixture.host"),
            generation: 1,
            native_id: kontor_core::id::ExternalId::parse(&id.to_string()).unwrap(),
        },
        observed_kind: kontor_core::state::ObservedContainerKind::Workspace,
        canonical_cwd: Some(name(cwd)),
        readback: None,
        bound_at: at,
        observed_at: at,
    }
}

#[test]
fn native_non_task_workspace_and_task_pin_cannot_take_each_others_destination() {
    use kontor_core::repository::TopologyRepository;
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("store.db")).unwrap();
    let project = ProjectId::generate();
    let task = seed(&store, project);
    let pin = placement(project, task, "/fixture/work");
    store
        .set_task_worktree(project, task, &pin.worktree)
        .unwrap();
    let container = native_container(&store, project, pin.destination.as_str());
    store.bind_topology_node_container(&container).unwrap();
    assert!(
        store
            .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
            .unwrap_err()
            .to_string()
            .contains("checkout.destination_ownership_conflict")
    );
    let project = ProjectId::generate();
    let task = seed(&store, project);
    let pin = placement(project, task, "/fixture/other-work");
    store
        .set_task_worktree(project, task, &pin.worktree)
        .unwrap();
    store
        .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
        .unwrap();
    let container = native_container(&store, project, pin.destination.as_str());
    assert!(store.bind_topology_node_container(&container).is_err());
}

#[test]
fn additive_upgrade_preserves_existing_graph_without_inventing_checkout_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let store = SqliteStore::open(&path).unwrap();
    let realm = store.realm_id();
    let project = ProjectId::generate();
    let task = seed(&store, project);
    store
        .set_task_worktree(project, task, &name("/fixture/existing"))
        .unwrap();
    drop(store);
    // Exact pre-138 shape of a disposable product fixture; no installation restore.
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("DROP TRIGGER task_checkout_claim_insert; DROP TRIGGER task_checkout_claim_update; DROP TRIGGER task_checkout_container_insert; DROP TRIGGER task_checkout_container_update; DROP TABLE task_checkout_placements; PRAGMA user_version=137;").unwrap();
    drop(sql);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.realm_id(), realm);
    assert_eq!(store.schema_version().unwrap(), 138);
    let claim = store.task_worktree(project, task).unwrap().unwrap();
    assert_eq!(claim.as_str(), "/fixture/existing");
    assert!(
        store
            .task_checkout_placement(project, task, &claim)
            .unwrap()
            .is_none()
    );
}

#[test]
fn first_module_claim_and_intent_commit_together_after_revision_and_payload_checks() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("store.db")).unwrap();
    let project = ProjectId::generate();
    seed(&store, project);
    let task = TaskId::generate();
    let module = kontor_core::id::ModuleKey::parse("components/worker").unwrap();
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: None,
            title: name("Module task"),
            module: Some(module.clone()),
            state: TaskState::Ready,
            created_at: kontor_core::id::Timestamp::now(),
        })
        .unwrap();
    let mut pin = placement(project, task, "/fixture/managed/exm-2/components/worker");
    pin.module = Some(module);
    // Inconsistent metadata cannot leave an initial claim behind.
    assert!(
        store
            .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
            .is_err()
    );
    assert!(store.task_worktree(project, task).unwrap().is_none());
    pin.intent=CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"claim":pin.worktree.as_str(),"destination":pin.destination.as_str(),"module":"components/worker","source":{"base_commit":"frozen"}})).unwrap();
    let mut stale = pin.clone();
    stale.task_revision = AggregateRevision::parse(2).unwrap();
    assert!(
        store
            .freeze_task_checkout(&stale, kontor_core::id::Timestamp::now())
            .is_err()
    );
    assert!(store.task_worktree(project, task).unwrap().is_none());
    store
        .freeze_task_checkout(&pin, kontor_core::id::Timestamp::now())
        .unwrap();
    assert_eq!(
        store.task_worktree(project, task).unwrap(),
        Some(pin.worktree.clone())
    );
    assert_eq!(
        store
            .task_checkout_placement(project, task, &pin.worktree)
            .unwrap()
            .unwrap()
            .intent,
        pin.intent
    );
}
