//! Native consultation placement through the same pinned project/store fixture.
use super::*;
use kontor_runtime::checkout;

fn world() -> (World, MiniProjectId, SessionTopologyNode) {
    let world = World::with_layout(
        None,
        true,
        true,
        true,
        Some("managed/trees/<key-slug>/<module>"),
    );
    let epic = world
        .state
        .with_store(|s| s.get_task(world.project, world.task))
        .unwrap()
        .unwrap()
        .mini_project_id
        .unwrap();
    world
        .state
        .with_store(|s| {
            s.declare_epic_execution_scope(
                world.project,
                epic,
                &kontor_store::EpicExecutionScopeDeclaration {
                    external_epic_key: ExternalId::parse("EXM-1").unwrap(),
                    short_title: ExternalName::parse("Consultation epic").unwrap(),
                    kontor_backlog_code: None,
                    ai_short_name: None,
                },
                kontor_api::now(),
            )
        })
        .unwrap();
    let node = consultation_node(&world, epic);
    (world, epic, node)
}

fn consultation_node(world: &World, epic: MiniProjectId) -> SessionTopologyNode {
    let scope = world
        .services
        .resolve_scope(
            world.project,
            &SemanticTopologyTargetDto::Epic { epic_id: epic },
        )
        .unwrap();
    let parent = world
        .services
        .ensure_scope_chain(world.project, &scope)
        .unwrap();
    let context = world
        .services
        .runtime_domain(world.project, Some(epic))
        .unwrap();
    world
        .state
        .with_store(|s| {
            s.create_topology_node(&NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id: world.project,
                mini_project_id: Some(epic),
                topology: context.topology,
                kind: context.domain.delivery.advisor_kind,
                parent_id: Some(parent.id),
                task_id: None,
                created_at: kontor_api::now(),
            })
        })
        .unwrap()
}

#[tokio::test]
async fn consultation_custom_root_intent_survives_interrupted_git_and_restart_without_relocation() {
    let (mut world, _, node) = world();
    let cwd = world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    assert!(
        cwd.as_str()
            .contains(&format!("/managed/trees/consultations/exm-1/{}", node.id))
    );
    assert!(!Path::new(cwd.as_str()).exists());
    let pin = world
        .services
        .consultation_checkout(world.project, node.id)
        .unwrap()
        .unwrap();
    assert!(
        world
            .state
            .with_store(|s| s.consultation_placement(world.project, node.id))
            .unwrap()
            .unwrap()
            .readback
            .is_none()
    );
    // Lost acknowledgement after Git, before readback. Only the durable intent permits retry.
    checkout::prepare(&pin.checkout).unwrap();
    let second = consultation_node(&world, pin.epic_id);
    let other = world
        .services
        .consultation_root(world.project, second.id)
        .unwrap();
    assert_ne!(cwd, other);
    world.folder.checkouts = None;
    world.folder.project.worktree_layout = "other/<key-slug>/<module>".into();
    let home = world.root.join("state");
    let store = kontor_store::SqliteStore::open(&home.join("store.db")).unwrap();
    let (services, state) = compose(&home, world.folder.clone(), store);
    services
        .prepare_consultation_checkout(world.project, node.id)
        .await
        .unwrap();
    assert_eq!(
        services.consultation_root(world.project, node.id).unwrap(),
        cwd
    );
    let readback = state
        .with_store(|s| s.consultation_placement(world.project, node.id))
        .unwrap()
        .unwrap()
        .readback
        .unwrap();
    assert!(
        readback
            .json()
            .contains(pin.checkout.source.base_commit.as_str())
    );
    let export = state
        .with_store(|s| kontor_store::backup::export_realm(s, kontor_api::now()))
        .unwrap();
    export.verify().unwrap();
    let exported = export
        .records
        .topology_nodes
        .iter()
        .find(|row| row.id == node.id.to_string())
        .unwrap();
    assert_eq!(
        exported.consultation_intent_hash.as_deref(),
        Some(
            state
                .with_store(|s| s.consultation_placement(world.project, node.id))
                .unwrap()
                .unwrap()
                .intent
                .hash()
                .as_str()
        )
    );
    assert_eq!(
        exported.consultation_git_readback.as_deref(),
        Some(readback.json())
    );
    let roundtrip: kontor_store::backup::KontorExportV1 =
        serde_json::from_slice(&serde_json::to_vec(&export).unwrap()).unwrap();
    roundtrip.verify().unwrap();
    assert_eq!(roundtrip.records_hash, export.records_hash);

    services
        .prepare_consultation_checkout(world.project, node.id)
        .await
        .unwrap();
    assert_eq!(
        state
            .with_store(|s| s.consultation_placement(world.project, node.id))
            .unwrap()
            .unwrap()
            .readback
            .unwrap(),
        readback
    );
    fixture::git(
        Path::new(&pin.checkout.source.declaration.path),
        &["worktree", "remove", cwd.as_str()],
    );
    assert!(
        services
            .prepare_consultation_checkout(world.project, node.id)
            .await
            .is_err()
    );
    assert!(
        !Path::new(cwd.as_str()).exists(),
        "confirmed checkout cannot be silently recreated"
    );
}

#[tokio::test]
async fn consultation_source_layout_hash_binding_and_missing_source_fail_before_effect() {
    let (world, _, node) = world();
    let cwd = world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    let mut pin = world
        .services
        .consultation_checkout(world.project, node.id)
        .unwrap()
        .unwrap();
    pin.bundle_hash = ContentHash::of(b"foreign domain");
    let project = world.services.project_row(world.project).unwrap();
    assert!(
        world
            .services
            .validate_consultation_pin(
                &project,
                &node,
                &pin,
                Some(&TrackerKey::parse("EXM-1").unwrap())
            )
            .is_err()
    );
    let mut layout = world
        .services
        .consultation_checkout(world.project, node.id)
        .unwrap()
        .unwrap();
    layout.policy.template = "rooms/<key-slug>/<node>".into();
    let (relative, _, _) = layout
        .policy
        .render(&TrackerKey::parse("EXM-1").unwrap(), node.id)
        .unwrap();
    layout.checkout.destination = format!("{}/{relative}", layout.checkout.project_root);
    layout.checkout.claim = layout.checkout.destination.clone();
    layout.validate().unwrap();
    assert!(
        world
            .services
            .validate_consultation_pin(
                &project,
                &node,
                &layout,
                Some(&TrackerKey::parse("EXM-1").unwrap())
            )
            .is_err(),
        "unpublished layout cannot replace the epic's pinned layout"
    );
    let source = PathBuf::from(&pin.checkout.source.declaration.path);
    std::fs::rename(source.join(".git"), source.join("git-unavailable")).unwrap();
    assert!(
        world
            .services
            .prepare_consultation_checkout(world.project, node.id)
            .await
            .is_err()
    );
    assert!(!Path::new(cwd.as_str()).exists());
    std::fs::rename(source.join("git-unavailable"), source.join(".git")).unwrap();
    world
        .services
        .prepare_consultation_checkout(world.project, node.id)
        .await
        .unwrap();
    let mut altered = world
        .services
        .consultation_checkout(world.project, node.id)
        .unwrap()
        .unwrap();
    altered.policy.template = "other/<node>/<key-slug>".into();
    assert!(altered.verify().is_err());
    altered = world
        .services
        .consultation_checkout(world.project, node.id)
        .unwrap()
        .unwrap();
    altered.checkout.source.common_dir.push_str("-foreign");
    assert!(altered.verify().is_err());
}

#[tokio::test]
async fn consultation_unbound_legacy_directory_and_symlink_escape_are_never_adopted() {
    let (world, _, node) = world();
    let root = world.services.project_row(world.project).unwrap().root_path;
    let legacy = PathBuf::from(root.as_str())
        .join(".worktrees")
        .join(format!("consultation-{}", node.id));
    std::fs::create_dir_all(&legacy).unwrap();
    let refusal = world
        .services
        .consultation_root(world.project, node.id)
        .unwrap_err();
    assert_eq!(refusal.rule, "consultation.legacy_binding_missing");
    assert!(std::fs::read_dir(&legacy).unwrap().next().is_none());
    std::fs::remove_dir(&legacy).unwrap();
    world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    let outside = world.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let managed = PathBuf::from(root.as_str()).join("managed");
    std::os::unix::fs::symlink(&outside, &managed).unwrap();
    assert!(
        world
            .services
            .prepare_consultation_checkout(world.project, node.id)
            .await
            .is_err()
    );
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}

#[tokio::test]
async fn consultation_preexisting_rendered_directory_refuses_without_an_intent() {
    let (world, epic, node) = world();
    let context = world
        .services
        .runtime_domain(world.project, Some(epic))
        .unwrap();
    let policy = world
        .services
        .fleet
        .verify_bundle(&context.bundle_hash)
        .unwrap()
        .manifest
        .consultation_layout
        .unwrap();
    let (relative, _, _) = policy
        .render(&TrackerKey::parse("EXM-1").unwrap(), node.id)
        .unwrap();
    let root = world.services.project_row(world.project).unwrap().root_path;
    std::fs::create_dir_all(PathBuf::from(root.as_str()).join(relative)).unwrap();
    assert!(
        world
            .services
            .consultation_root(world.project, node.id)
            .unwrap_err()
            .rule
            .contains("consultation.destination_unbound")
    );
    assert!(
        world
            .state
            .with_store(|s| s.consultation_placement(world.project, node.id))
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn consultation_bound_legacy_resumes_without_backfill_and_missing_destination_refuses() {
    let (world, _, node) = world();
    let project = world.services.project_row(world.project).unwrap();
    let target = PathBuf::from(project.root_path.as_str())
        .join(".worktrees")
        .join(format!("consultation-{}", node.id));
    let branch = format!("consultation-{}", node.id);
    fixture::git(
        Path::new(project.root_path.as_str()),
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            target.to_str().unwrap(),
            fixture::BASE,
        ],
    );
    // Test-only historical binding: new production placement cannot create/adopt this path.
    let now = kontor_api::now();
    world
        .state
        .with_store(|s| {
            s.bind_topology_node_container(&NewNativeContainerBinding {
                project_id: world.project,
                topology_node_id: node.id,
                container_binding_id: ExternalId::parse(
                    &ContainerBindingId::generate().to_string(),
                )
                .unwrap(),
                identity: kontor_core::state::NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse("fake.runtime").unwrap(),
                    host: ExternalName::parse("test-host").unwrap(),
                    generation: 1,
                    native_id: ExternalId::parse("historical-consultation").unwrap(),
                },
                observed_kind: ObservedContainerKind::Workspace,
                canonical_cwd: Some(ExternalName::parse(target.to_str().unwrap()).unwrap()),
                readback: None,
                bound_at: now,
                observed_at: now,
            })
        })
        .unwrap();
    assert_eq!(
        world
            .services
            .consultation_root(world.project, node.id)
            .unwrap()
            .as_str(),
        target.to_str().unwrap()
    );
    assert!(
        world
            .state
            .with_store(|s| s.consultation_placement(world.project, node.id))
            .unwrap()
            .is_none(),
        "legacy binding is not backfilled"
    );
    fixture::git(
        Path::new(project.root_path.as_str()),
        &["worktree", "remove", target.to_str().unwrap()],
    );
    assert!(
        world
            .services
            .consultation_root(world.project, node.id)
            .is_err()
    );
    assert!(!target.exists());
}

#[tokio::test]
async fn consultation_missing_source_and_durable_collision_refuse_before_git() {
    let (world, _, node) = world();
    let cwd = world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    world
        .state
        .with_store(|s| {
            s.set_task_worktree(
                world.project,
                world.task,
                &ExternalName::parse(cwd.as_str()).unwrap(),
            )
        })
        .unwrap();
    assert!(
        world
            .services
            .prepare_consultation_checkout(world.project, node.id)
            .await
            .is_err()
    );
    assert!(!Path::new(cwd.as_str()).exists());
    let other = consultation_node(&world, node.mini_project_id.unwrap());
    let mut folder = world.folder.clone();
    folder.checkouts = None;
    let home = world.root.join("state");
    let (services, _) = compose(
        &home,
        folder,
        kontor_store::SqliteStore::open(&home.join("store.db")).unwrap(),
    );
    assert_eq!(
        services
            .consultation_root(world.project, other.id)
            .unwrap_err()
            .rule,
        "host.checkouts.root.not_configured"
    );
}

#[tokio::test]
async fn consultation_store_hash_guard_refuses_tampered_source_layout_intent() {
    let (world, _, node) = world();
    world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    let intent = world
        .state
        .with_store(|s| s.consultation_placement(world.project, node.id))
        .unwrap()
        .unwrap()
        .intent;
    let connection = rusqlite::Connection::open(world.root.join("state/store.db")).unwrap();
    assert!(
        connection
            .execute(
                "UPDATE topology_nodes SET consultation_intent='{}' WHERE id=?1",
                [node.id.to_string()]
            )
            .is_err()
    );
    // Simulate disk/import corruption only in this disposable test; product writes stay immutable.
    connection
        .execute("DROP TRIGGER consultation_intent_immutable", [])
        .unwrap();
    connection
        .execute(
            "UPDATE topology_nodes SET consultation_intent_hash=?1 WHERE id=?2",
            rusqlite::params![
                ContentHash::of(b"tampered source/layout").as_str(),
                node.id.to_string()
            ],
        )
        .unwrap();
    assert!(
        world
            .state
            .with_store(|s| s.consultation_placement(world.project, node.id))
            .is_err(),
        "source/layout binding hash must be checked on every stored read"
    );
    assert!(
        world
            .services
            .prepare_consultation_checkout(world.project, node.id)
            .await
            .is_err()
    );
    assert!(
        !Path::new(
            serde_json::from_str::<kontor_runtime::consultation_checkout::ConsultationCheckout>(
                intent.json()
            )
            .unwrap()
            .checkout
            .destination
            .as_str()
        )
        .exists()
    );
}

#[tokio::test]
async fn consultation_two_epics_have_distinct_confirmed_paths_and_foreign_node_refuses() {
    let (world, first, node) = world();
    let first_root = world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    let second = super::super::domain_runtime_tests::epic(
        &world.services,
        world.project,
        "second-consultation-epic",
    );
    world
        .state
        .with_store(|s| {
            s.declare_epic_execution_scope(
                world.project,
                second,
                &kontor_store::EpicExecutionScopeDeclaration {
                    external_epic_key: ExternalId::parse("EXM-9").unwrap(),
                    short_title: ExternalName::parse("Second epic").unwrap(),
                    kontor_backlog_code: None,
                    ai_short_name: None,
                },
                kontor_api::now(),
            )
        })
        .unwrap();
    let second_node = consultation_node(&world, second);
    let second_root = world
        .services
        .consultation_root(world.project, second_node.id)
        .unwrap();
    assert_ne!(first, second);
    assert_ne!(first_root, second_root);
    assert!(second_root.as_str().contains("/consultations/exm-9/"));
    assert!(
        world
            .services
            .consultation_root(ProjectId::generate(), node.id)
            .is_err()
    );
    assert!(!Path::new(first_root.as_str()).exists());
    assert!(!Path::new(second_root.as_str()).exists());
}

#[tokio::test]
async fn consultation_native_binding_requires_both_readbacks_and_exact_persisted_binding() {
    let (world, _, node) = world();
    let target = world
        .services
        .consultation_root(world.project, node.id)
        .unwrap();
    let pin = world
        .services
        .consultation_checkout(world.project, node.id)
        .unwrap()
        .unwrap();
    let now = kontor_api::now();
    let mut binding = NewNativeContainerBinding {
        project_id: world.project,
        topology_node_id: node.id,
        container_binding_id: ExternalId::parse(&pin.container_binding_id).unwrap(),
        identity: kontor_core::state::NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("fake.runtime").unwrap(),
            host: ExternalName::parse("test-host").unwrap(),
            generation: 1,
            native_id: ExternalId::parse("observed-native").unwrap(),
        },
        observed_kind: ObservedContainerKind::Workspace,
        canonical_cwd: Some(ExternalName::parse(target.as_str()).unwrap()),
        readback: None,
        bound_at: now,
        observed_at: now,
    };
    assert!(
        world
            .state
            .with_store(|s| s.bind_topology_node_container(&binding))
            .is_err()
    );
    world
        .services
        .prepare_consultation_checkout(world.project, node.id)
        .await
        .unwrap();
    assert!(
        world
            .state
            .with_store(|s| s.bind_topology_node_container(&binding))
            .is_err(),
        "Git readback alone cannot replace native readback"
    );
    binding.readback = Some(NativeContainerReadback {
        projection: ObservedContainerProjection::NativeChild,
        visible_title: ExternalName::parse("Consultation").unwrap(),
        native_parent: Some(kontor_core::state::NativeRuntimeIdentity {
            native_id: ExternalId::parse("parent-native").unwrap(),
            ..binding.identity.clone()
        }),
        topology_correlation: ExternalName::parse(&format!("kontor-node-{}", node.id)).unwrap(),
    });
    let mut changed = binding.clone();
    changed.container_binding_id =
        ExternalId::parse(&ContainerBindingId::generate().to_string()).unwrap();
    assert!(
        world
            .state
            .with_store(|s| s.bind_topology_node_container(&changed))
            .is_err()
    );
    assert!(
        world
            .state
            .with_store(|s| s.get_topology_node_container(world.project, node.id))
            .unwrap()
            .is_none()
    );
    let bound = world
        .state
        .with_store(|s| s.bind_topology_node_container(&binding))
        .unwrap();
    assert_eq!(bound.identity, binding.identity);
    assert_eq!(
        world
            .state
            .with_store(|s| s.get_topology_node(world.project, node.id))
            .unwrap()
            .unwrap()
            .placement,
        kontor_core::state::PlacementState::Bound
    );
}
