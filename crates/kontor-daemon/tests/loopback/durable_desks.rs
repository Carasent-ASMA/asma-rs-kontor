//! Durable desks through the supported daemon operations (ASMA-8450).
//!
//! A desk is materialized, read back, drifted and replayed through the same
//! semantic `/v1` topology operations every other scope uses, with no Jira epic
//! anywhere in the realm. The desk-capable definition is published and selected
//! through the supported Admin surfaces rather than taken from a bundled
//! revision, so these tests prove the operation independently of which lineage
//! version a successor is shipped at.

use super::*;
use kontor_core::spec::TeamDefinitionSpec;

const DESK_KINDS: &str = r#"[
  {"kind": "DESK", "allowed_parents": ["PSW"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["native_root"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Durable Desk", "meaning": "A desk.",
                 "category": "session_topology", "lifecycle": "current"}},
  {"kind": "DWS", "allowed_parents": ["DESK"], "cardinality": {"minimum": 1, "maximum": 1},
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Desk Workspace", "meaning": "A desk's workspace.",
                 "category": "session_topology", "lifecycle": "current"}}
]"#;

const DESK_CONTAINERS: &str = r#"[
  {"kind": "DESK", "parent": null, "prefix": "DESK", "projection_capabilities": ["native_root"],
   "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"},
                                  {"kind": "token", "value": "DESK_NAME"}]}},
  {"kind": "DWS", "parent": "DESK", "prefix": "ADAM",
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]}}
]"#;

const DESKS: &str = r#"[
  {"desk_key": "adam", "display_name": "ADAM", "kind": "DESK", "workspace_kind": "DWS"},
  {"desk_key": "pr-review", "display_name": "PR REVIEW", "kind": "DESK", "workspace_kind": "DWS"}
]"#;

/// A project with a runtime plane and no epic at all.
async fn desk_world(slug: &str) -> (World, String) {
    let world = World::open_empty_with_a_plane().await;
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let created = ensure_project(&world, slug, "Desks", &format!("/tmp/kontor-{slug}")).await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("project id")
        .to_owned();
    (world, project)
}

/// The project's selected Team Definition revision, as stored.
fn selected_definition(world: &World, project: &str) -> TeamDefinitionSpec {
    let project_id = ProjectId::parse(project).expect("a project id");
    world.daemon.state().with_store(|store| {
        let selected = store
            .get_project_team_definition_default(project_id)
            .expect("the selection reads")
            .expect("the project has a selection");
        store
            .get_team_definition(
                project_id,
                selected.definition.definition_id,
                selected.definition.version,
            )
            .expect("the selected revision reads")
            .expect("the selected revision exists")
    })
}

/// Publish a desk-capable successor of the project's topology and Team
/// Definition through the Admin surfaces, and select the definition for the
/// project. Selection is the explicit, previewed act a desk requires.
async fn select_desk_definition(world: &World, project: &str) -> TeamDefinitionSpec {
    let selected = selected_definition(world, project);
    // The newest published revision of the lineage, whichever version that
    // is: a build may already have seeded a successor of its own.
    let lineage = selected.topology.spec_id;
    let mut newest = selected.topology.version.get();
    let mut current = Call::get(format!(
        "/v1/projects/{project}/topology-specs/{lineage}/{newest}"
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(current.status, 200, "{}", current.body);
    loop {
        let next = Call::get(format!(
            "/v1/projects/{project}/topology-specs/{lineage}/{}",
            newest + 1
        ))
        .signed_as(world, "admin")
        .send(world)
        .await;
        if next.status != 200 {
            break;
        }
        newest += 1;
        current = next;
    }
    let mut node_kinds = current.json()["document"]["node_kinds"].clone();
    let missing: Vec<serde_json::Value> =
        serde_json::from_str::<Vec<serde_json::Value>>(DESK_KINDS)
            .expect("kinds")
            .into_iter()
            .filter(|kind| {
                node_kinds
                    .as_array()
                    .expect("node kinds")
                    .iter()
                    .all(|declared| declared["kind"] != kind["kind"])
            })
            .collect();
    let topology = if missing.is_empty() {
        current.json()["spec"].clone()
    } else {
        node_kinds
            .as_array_mut()
            .expect("node kinds")
            .extend(missing);
        let drafted = Call::post(
            format!("/v1/projects/{project}/topology-specs:draft"),
            &serde_json::json!({
                "base": {"id": lineage.to_string(), "version": newest},
                "name": "Desk vocabulary",
                "root_kind": current.json()["document"]["root_kind"],
                "node_kinds": node_kinds,
                "historical_codes": current.json()["document"]["historical_codes"],
            }),
        )
        .signed_as(world, "admin")
        .with_key("desk-topology-draft")
        .send(world)
        .await;
        assert_eq!(drafted.status, 200, "{}", drafted.body);
        let published = Call::post(
            format!("/v1/projects/{project}/topology-specs:publish"),
            &serde_json::json!({
                "candidate": drafted.json()["candidate"],
                "validation_hash": drafted.json()["candidate_hash"],
                "expected_revision": current_project_revision(world, project).await,
            }),
        )
        .signed_as(world, "admin")
        .with_key("desk-topology-publish")
        .send(world)
        .await;
        assert_eq!(published.status, 200, "{}", published.body);
        published.json()["spec"].clone()
    };

    let catalog = Call::get(format!("/v1/projects/{project}/team-definitions"))
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    let latest = catalog.json()["definitions"]
        .as_array()
        .expect("the Team Definition catalog")
        .iter()
        .filter(|entry| entry["definition"]["id"] == selected.definition_id.to_string())
        .filter_map(|entry| entry["definition"]["version"].as_u64())
        .max()
        .expect("the selected lineage is catalogued");
    let mut definition = serde_json::to_value(&selected).expect("serializes");
    let containers: Vec<serde_json::Value> =
        serde_json::from_str(DESK_CONTAINERS).expect("containers");
    definition["containers"]
        .as_array_mut()
        .expect("containers")
        .extend(containers);
    definition["desks"] = serde_json::from_str(DESKS).expect("desks");
    definition["version"] = serde_json::json!(latest + 1);
    definition["topology"] = serde_json::json!({
        "spec_id": topology["id"],
        "version": topology["version"],
        "canonical_hash": topology["canonical_hash"],
    });
    let validated = Call::post(
        format!("/v1/projects/{project}/team-definitions:validate"),
        &serde_json::json!({"candidate": definition}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(validated.status, 200, "{}", validated.body);
    assert_eq!(
        validated.json()["violations"],
        serde_json::json!([]),
        "{}",
        validated.body
    );
    let published = Call::post(
        format!("/v1/projects/{project}/team-definitions:publish"),
        &serde_json::json!({
            "candidate": definition,
            "validation_hash": validated.json()["validation_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("desk-definition-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    let preview = Call::post(
        format!("/v1/projects/{project}/team-definition-selection:preview"),
        &serde_json::json!({"target_definition": {
            "id": selected.definition_id.to_string(),
            "version": latest + 1,
        }}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let selected = Call::post(
        format!("/v1/projects/{project}/team-definition-selection:apply"),
        &serde_json::json!({
            "preview_hash": preview.json()["preview_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("desk-definition-select")
    .send(world)
    .await;
    assert_eq!(selected.status, 200, "{}", selected.body);
    serde_json::from_value(definition).expect("the published desk definition")
}

/// One semantic topology write against one desk.
async fn desk_call(world: &World, project: &str, operation: &str, desk: &str, key: &str) -> Answer {
    Call::post(
        format!("/v1/projects/{project}/topology:{operation}"),
        &serde_json::json!({
            "target": {"scope": "desk", "desk_key": desk},
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await
}

/// The nodes of one projection that realize `desk`, desk first.
fn desk_nodes(projection: &serde_json::Value, desk: &str) -> Vec<serde_json::Value> {
    let mut nodes: Vec<serde_json::Value> = projection["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter(|node| node["desk_key"] == desk)
        .cloned()
        .collect();
    nodes.sort_by_key(|node| node["kind_key"] != "DESK");
    nodes
}

fn node_id(node: &serde_json::Value) -> TopologyNodeId {
    TopologyNodeId::parse(node["topology_node_id"].as_str().expect("a node id")).expect("an id")
}

#[tokio::test]
async fn an_undeclared_desk_is_refused_before_any_desk_state_or_native_effect() {
    let (world, project) = desk_world("desk-undeclared").await;
    let calls_before = world.fake.calls().len();

    // The bundled default declares no desk, and nothing selects one silently.
    for operation in ["ensure", "materialize"] {
        let refused = desk_call(
            &world,
            &project,
            operation,
            "adam",
            &format!("desk-undeclared-{operation}"),
        )
        .await;
        assert_eq!(
            refused.json()["code"],
            "placement_blocked",
            "{}",
            refused.body
        );
        assert!(
            refused.body.contains("declares no such desk"),
            "{}",
            refused.body
        );
    }
    // Selected, but still not a key the selection declares.
    select_desk_definition(&world, &project).await;
    let refused = desk_call(
        &world,
        &project,
        "materialize",
        "nope",
        "desk-undeclared-key",
    )
    .await;
    assert_eq!(
        refused.json()["code"],
        "placement_blocked",
        "{}",
        refused.body
    );

    let project_id = ProjectId::parse(&project).expect("a project id");
    world.daemon.state().with_store(|store| {
        assert!(store.list_desks(project_id).expect("lists").is_empty());
        assert!(
            store
                .list_project_topology_nodes(project_id)
                .expect("lists")
                .iter()
                .all(|node| !["DESK", "DWS"].contains(&node.kind.as_str())),
            "no desk node exists"
        );
    });
    assert!(
        world.fake.calls()[calls_before..]
            .iter()
            .all(|call| !matches!(call, AdapterCall::PrepareContainer(_))),
        "a refused desk reaches no native container operation"
    );
}

#[tokio::test]
async fn both_desks_materialize_read_back_and_replay_without_an_epic() {
    let (world, project) = desk_world("desk-materialize").await;
    select_desk_definition(&world, &project).await;
    let project_id = ProjectId::parse(&project).expect("a project id");

    // Ensure is logical only.
    let calls_before = world.fake.calls().len();
    let ensured = desk_call(&world, &project, "ensure", "adam", "desk-adam-ensure").await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let nodes = desk_nodes(&ensured.json()["projection"], "adam");
    assert_eq!(nodes.len(), 2, "{}", ensured.body);
    assert_eq!(nodes[0]["kind_key"], "DESK");
    assert_eq!(nodes[1]["kind_key"], "DWS");
    assert_eq!(
        nodes[1]["parent_topology_node_id"],
        nodes[0]["topology_node_id"]
    );
    assert!(nodes.iter().all(|node| node["observed_binding"].is_null()));
    assert!(
        world.fake.calls()[calls_before..]
            .iter()
            .all(|call| !matches!(call, AdapterCall::PrepareContainer(_))),
        "ensuring a desk has no native effect"
    );

    let mut placed = BTreeMap::new();
    for (desk, title) in [("adam", "DESK • ADAM"), ("pr-review", "DESK • PR REVIEW")] {
        let materialized = desk_call(
            &world,
            &project,
            "materialize",
            desk,
            &format!("desk-{desk}-materialize"),
        )
        .await;
        assert_eq!(materialized.status, 200, "{}", materialized.body);
        assert_eq!(materialized.json()["receipt"]["applied"], "created");
        let nodes = desk_nodes(&materialized.json()["projection"], desk);
        assert_eq!(nodes.len(), 2, "{}", materialized.body);
        let (desk_node, workspace) = (node_id(&nodes[0]), node_id(&nodes[1]));
        assert_eq!(
            world.fake.container_title(desk_node).as_deref(),
            Some(title)
        );
        assert_eq!(
            world.fake.container_title(workspace).as_deref(),
            Some("ADAM")
        );
        let desk_cwd = nodes[0]["observed_binding"]["cwd"].clone();
        assert_eq!(
            nodes[1]["observed_binding"]["cwd"], desk_cwd,
            "the desk project and its workspace share the desk's directory"
        );
        assert!(
            desk_cwd
                .as_str()
                .expect("a cwd")
                .ends_with(&format!("desk-{desk_node}")),
            "{desk_cwd}"
        );
        placed.insert(desk, (desk_node, workspace, nodes));
    }
    assert_ne!(
        placed["adam"].2[0]["observed_binding"]["cwd"],
        placed["pr-review"].2[0]["observed_binding"]["cwd"],
        "two desks never share a directory"
    );

    world.daemon.state().with_store(|store| {
        assert_eq!(store.list_desks(project_id).expect("lists").len(), 2);
        assert!(
            store
                .list_project_topology_nodes(project_id)
                .expect("lists")
                .iter()
                .all(|node| node.mini_project_id.is_none()),
            "nothing in this realm belongs to an epic"
        );
    });

    // A replay is the original receipt and no second native effect.
    let calls_before = world.fake.calls().len();
    let replayed = desk_call(
        &world,
        &project,
        "materialize",
        "adam",
        "desk-adam-materialize",
    )
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert!(
        world.fake.calls()[calls_before..]
            .iter()
            .all(|call| !matches!(call, AdapterCall::PrepareContainer(_))),
        "a replay prepares nothing"
    );

    // A fresh key reconciles the same two containers by their exact ids.
    let (adam_desk, adam_workspace, _) = placed["adam"].clone();
    let native = (
        world.fake.container_native_id(adam_desk),
        world.fake.container_native_id(adam_workspace),
    );
    let again = desk_call(&world, &project, "materialize", "adam", "desk-adam-again").await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(
        (
            world.fake.container_native_id(adam_desk),
            world.fake.container_native_id(adam_workspace),
        ),
        native,
        "a second materialization keeps both native identities"
    );
    assert_eq!(desk_nodes(&again.json()["projection"], "adam").len(), 2);

    // Drift reads back exactly this desk's two natives: not the other desk,
    // and not the project root above it.
    let calls_before = world.fake.calls().len();
    let drifted = desk_call(&world, &project, "drift", "adam", "desk-adam-drift").await;
    assert_eq!(drifted.status, 200, "{}", drifted.body);
    let inspected: BTreeSet<TopologyNodeId> = world.fake.calls()[calls_before..]
        .iter()
        .filter_map(|call| match call {
            AdapterCall::InspectContainer(node) => Some(*node),
            _ => None,
        })
        .collect();
    assert_eq!(inspected, BTreeSet::from([adam_desk, adam_workspace]));
}

#[tokio::test]
async fn a_restarted_daemon_reads_back_and_reconciles_the_same_desks() {
    let (world, project) = desk_world("desk-restart").await;
    select_desk_definition(&world, &project).await;
    let materialized = desk_call(
        &world,
        &project,
        "materialize",
        "adam",
        "desk-restart-materialize",
    )
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let before = desk_nodes(&materialized.json()["projection"], "adam");
    let (desk_node, workspace) = (node_id(&before[0]), node_id(&before[1]));
    let native = (
        world.fake.container_native_id(desk_node),
        world.fake.container_native_id(workspace),
    );
    let operator = secret(&world, "operator");
    let observer = secret(&world, "observer");

    let World {
        directory,
        daemon,
        router,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(router);
    drop(daemon);
    fake.rebuild_adapter_state();
    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    assert_eq!(restarted.reconcile().await, BarrierState::Open);
    let router = restarted.router();

    let read = Call::get(format!("/v1/projects/{project}/topology:inspect"))
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    let after = desk_nodes(&read.json(), "adam");
    assert_eq!(after.len(), 2, "{}", read.body);
    for (node_before, node_after) in before.iter().zip(&after) {
        for field in [
            "topology_node_id",
            "kind_key",
            "parent_topology_node_id",
            "desk_key",
        ] {
            assert_eq!(node_before[field], node_after[field], "{field}");
        }
        assert_eq!(
            node_before["observed_binding"]["native_id"],
            node_after["observed_binding"]["native_id"]
        );
        assert_eq!(
            node_before["observed_binding"]["cwd"],
            node_after["observed_binding"]["cwd"]
        );
    }

    let project_read = Call::get(format!("/v1/projects/{project}"))
        .with_token(&observer)
        .send_to(&router)
        .await;
    assert_eq!(project_read.status, 200, "{}", project_read.body);
    for (operation, key) in [
        ("drift", "desk-restart-drift"),
        ("materialize", "desk-restart-again"),
    ] {
        let answer = Call::post(
            format!("/v1/projects/{project}/topology:{operation}"),
            &serde_json::json!({
                "target": {"scope": "desk", "desk_key": "adam"},
                "expected_revision": project_read.json()["revision"],
            }),
        )
        .with_token(&operator)
        .with_key(key)
        .send_to(&router)
        .await;
        assert_eq!(answer.status, 200, "{operation}: {}", answer.body);
    }
    assert_eq!(
        (
            fake.container_native_id(desk_node),
            fake.container_native_id(workspace),
        ),
        native,
        "the restarted daemon reconciles the desks it already placed"
    );
    restarted.state().signals().stop();
    drop(directory);
}

#[tokio::test]
async fn no_epic_operation_can_retire_or_archive_a_desk() {
    let (world, project) = desk_world("desk-closeout").await;
    select_desk_definition(&world, &project).await;
    let materialized = desk_call(
        &world,
        &project,
        "materialize",
        "adam",
        "desk-closeout-materialize",
    )
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let nodes = desk_nodes(&materialized.json()["projection"], "adam");

    // An epic beside the desk, closed down through the same node routes.
    let revision = current_project_revision(&world, &project).await;
    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Epic beside a desk",
            &category,
            serde_json::json!([{"title": "A task"}]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("desk-closeout-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("epic id")
        .to_owned();
    confirm_test_epic_identity(&world, &project, &epic);
    let ensured = Call::post(
        format!("/v1/projects/{project}/topology:ensure"),
        &serde_json::json!({
            "target": {"scope": "epic_control", "epic_id": epic},
            "expected_revision": current_project_revision(&world, &project).await,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("desk-closeout-epic-ensure")
    .send(&world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let epic_scoped = Call::get(format!(
        "/v1/projects/{project}/topology:inspect?epic_id={epic}"
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(epic_scoped.status, 200, "{}", epic_scoped.body);
    assert!(
        epic_scoped.json()["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .all(|node| node["desk_key"].is_null()),
        "an epic's own topology never includes a desk: {}",
        epic_scoped.body
    );
    let control = epic_scoped.json()["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["kind_key"] == "ECP")
        .cloned()
        .expect("the epic control plane");
    // Conclude the epic's leadership pair first, as the node-retire test does,
    // so the control plane is the leaf an epic closeout moves.
    let connection =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .expect("the realm database opens");
    let concluded = connection
        .execute(
            "UPDATE seat_bindings SET lifecycle = 'retired'
             WHERE project_id = ?1 AND topology_node_id = ?2 AND lifecycle = 'active'",
            rusqlite::params![
                project,
                control["topology_node_id"].as_str().expect("an id")
            ],
        )
        .expect("the control seats conclude");
    assert_eq!(concluded, 2, "the epic was born with its leadership pair");
    drop(connection);
    let project_id = ProjectId::parse(&project).expect("a project id");
    let mut control_revision = serde_json::json!(world.daemon.state().with_store(|store| {
        store
            .get_topology_node(project_id, node_id(&control))
            .expect("reads")
            .expect("the control plane exists")
            .revision
            .get()
    }));
    for action in ["retire", "archive"] {
        let moved = Call::post(
            format!(
                "/v1/projects/{project}/topology/nodes/{}/{action}",
                control["topology_node_id"].as_str().expect("an id")
            ),
            &serde_json::json!({
                "expected_revision": control_revision,
                "reason": "Epic closeout",
            }),
        )
        .signed_as(&world, "operator")
        .with_key(format!("desk-closeout-epic-{action}"))
        .send(&world)
        .await;
        assert_eq!(moved.status, 200, "{action}: {}", moved.body);
        control_revision = moved.json()["projection"]["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .find(|node| node["topology_node_id"] == control["topology_node_id"])
            .expect("the control plane is still projected")["revision"]
            .clone();
        // Moving the epic's own node never moves a desk's.
        let whole = Call::get(format!("/v1/projects/{project}/topology:inspect"))
            .signed_as(&world, "observer")
            .send(&world)
            .await;
        assert_eq!(whole.status, 200, "{}", whole.body);
        assert_eq!(
            desk_nodes(&whole.json(), "adam")
                .iter()
                .map(|node| node["lifecycle"].clone())
                .collect::<Vec<_>>(),
            vec![serde_json::json!("active"); 2],
            "{action}: {}",
            whole.body
        );
    }

    // Neither desk node may be moved by the node routes at all. The workspace
    // goes first: it is a leaf, so nothing but the desk rule stands between it
    // and retirement.
    let calls_before = world.fake.calls().len();
    for node in nodes.iter().rev() {
        for action in ["retire", "archive"] {
            // The revision the node stands at now, so that only the desk rule
            // can refuse the move.
            let revision = world.daemon.state().with_store(|store| {
                store
                    .get_topology_node(project_id, node_id(node))
                    .expect("reads")
                    .expect("the desk node exists")
                    .revision
                    .get()
            });
            let refused = Call::post(
                format!(
                    "/v1/projects/{project}/topology/nodes/{}/{action}",
                    node["topology_node_id"].as_str().expect("an id")
                ),
                &serde_json::json!({
                    "expected_revision": revision,
                    "reason": "Epic closeout",
                }),
            )
            .signed_as(&world, "operator")
            .with_key(format!(
                "desk-closeout-{action}-{}",
                node["topology_node_id"].as_str().expect("an id")
            ))
            .send(&world)
            .await;
            // The effect first: a refusal that answered after moving the node
            // would still read as a refusal.
            let stored = world.daemon.state().with_store(|store| {
                store
                    .get_topology_node(project_id, node_id(node))
                    .expect("reads")
                    .expect("the desk node exists")
                    .lifecycle
            });
            assert_eq!(
                stored,
                TopologyLifecycle::Active,
                "{action} {} moved a desk node: {}",
                node["kind_key"],
                refused.body
            );
            assert!(
                world.fake.calls()[calls_before..]
                    .iter()
                    .all(|call| !matches!(call, AdapterCall::ArchiveContainer(_))),
                "{action} {} archived a desk container: {}",
                node["kind_key"],
                refused.body
            );
            assert_eq!(
                refused.json()["code"],
                "placement_blocked",
                "{action} {}: {}",
                node["kind_key"],
                refused.body
            );
        }
    }
    world.daemon.state().with_store(|store| {
        for node in &nodes {
            let stored = store
                .get_topology_node(project_id, node_id(node))
                .expect("reads")
                .expect("the desk node exists");
            assert_eq!(stored.lifecycle, TopologyLifecycle::Active);
        }
    });
}
