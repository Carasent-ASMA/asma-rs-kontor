//! The daemon's native-task boundary: claim, intent, Git, correlated readback.
use super::*;
use kontor_core::repository::{NewMiniProject, NewProject, NewTask};
use kontor_store::{
    JiraIntentKind, JiraItemKind, NewJiraMaterializationBatch, NewJiraMaterializationItem,
};
use std::path::Path;

#[path = "../../tests/support/config.rs"]
mod config;
#[path = "../../tests/support/checkouts.rs"]
mod fixture;

struct World {
    _directory: tempfile::TempDir,
    root: PathBuf,
    folder: crate::config_folder::ConfigFolder,
    services: Arc<Services>,
    state: ApiState,
    project: ProjectId,
    task: TaskId,
    claim: ExternalName,
}

fn compose(
    home: &Path,
    folder: crate::config_folder::ConfigFolder,
    store: kontor_store::SqliteStore,
) -> (Arc<Services>, ApiState) {
    let services = Services::new(
        store.realm_id(),
        crate::DEFAULT_CAPACITY,
        kontor_jira::JiraConnectors::default(),
        home.join("runtime-roots"),
        crate::usage::UsagePoller::discover(home),
        Vec::new(),
        None,
        Arc::new(crate::fleet::FleetSource::at(home)),
        Arc::new(crate::succession::UnavailableSuccessionSummarizer),
        Vec::new(),
        Some(folder),
    )
    .unwrap();
    let state = ApiState::new(kontor_api::state::ApiParts {
        store,
        credentials: crate::credentials::open_or_create(home).unwrap(),
        ingress: kontor_api::auth::IngressPolicy {
            allowed_origins: Vec::new(),
        },
        runtimes: kontor_api::state::RuntimeRegistry::new().with(
            RuntimeKindKey::parse("fake.runtime").unwrap(),
            Arc::new(
                kontor_runtime::fake::ScriptedFakeRuntime::requiring_a_plane(
                    kontor_runtime::capability::RuntimeCapabilities {
                        trust_grade: kontor_runtime::capability::TrustGrade::A,
                        supported: kontor_runtime::capability::RuntimeCapability::ALL
                            .iter()
                            .copied()
                            .collect(),
                        account_env: true,
                        limits: kontor_runtime::capability::RuntimeLimits {
                            max_message_bytes: 4096,
                            max_history_page: 64,
                            max_concurrent_sessions: 8,
                            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
                        },
                    },
                ),
            ),
        ),
        sessions: kontor_api::state::SessionRegistry::new(),
        barrier: kontor_api::state::SchedulingBarrier::new(),
        signals: kontor_api::state::StreamSignals::new(),
        evidence_window_seconds: 3600,
        derived_read_deadline: std::time::Duration::from_secs(5),
        applications: services.clone(),
    });
    services.attach(state.clone());
    (services, state)
}

impl World {
    fn new(module: Option<&str>, confirmed: bool) -> Self {
        Self::with_claim(module, confirmed, true)
    }

    fn with_claim(module: Option<&str>, confirmed: bool, declared: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let project_root = fixture::repository(&root.join("checkout"));
        let folder = config::write_config_folder(&root);
        let entry = folder.join(crate::config_folder::ENTRY);
        let mut value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
        value["project"]["forge"]["base_branch"] = fixture::BASE.into();
        std::fs::write(entry, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
        config::write_runtime_planes(
            &folder,
            &serde_json::json!({"schema_version":5,"runtimes":[]}),
        );
        let mut folder = crate::config_folder::ConfigFolder::select(Some(&folder), None).unwrap();
        folder.checkouts = Some(fixture::inputs(&project_root));
        let home = root.join("state");
        std::fs::create_dir(&home).unwrap();
        let store = kontor_store::SqliteStore::open(&home.join("store.db")).unwrap();
        let project = ProjectId::generate();
        let epic = MiniProjectId::generate();
        let task = TaskId::generate();
        let now = kontor_api::now();
        store
            .create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Example project").unwrap(),
                root_path: ExternalName::parse(project_root.to_str().unwrap()).unwrap(),
                created_at: now,
            })
            .unwrap();
        store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: project,
                name: ExternalName::parse("Checkout epic").unwrap(),
                created_at: now,
            })
            .unwrap();
        store
            .create_task(&NewTask {
                id: task,
                project_id: project,
                mini_project_id: Some(epic),
                title: ExternalName::parse("Placed task").unwrap(),
                module: module.map(|m| ModuleKey::parse(m).unwrap()),
                state: TaskState::Ready,
                created_at: now,
            })
            .unwrap();
        let claim = if declared {
            ExternalName::parse(root.join("explicit-task").to_str().unwrap()).unwrap()
        } else {
            crate::task_checkouts::derived_claim(
                &folder,
                project_root.to_str().unwrap(),
                &ModuleKey::parse(module.unwrap()).unwrap(),
                &TrackerKey::parse("EXM-2").unwrap(),
            )
            .unwrap()
        };
        if declared {
            store.set_task_worktree(project, task, &claim).unwrap();
        }
        if confirmed {
            let batch = ExternalId::parse(&uuid::Uuid::now_v7().to_string()).unwrap();
            store
                .plan_jira_materialization(
                    &NewJiraMaterializationBatch {
                        id: batch.clone(),
                        project_id: project,
                        epic_id: epic,
                        idempotency_key: "checkout-fixture".to_owned(),
                        preview_hash: ContentHash::of(b"checkout"),
                        expected_revision: AggregateRevision::INITIAL,
                        created_at: now,
                    },
                    &[NewJiraMaterializationItem {
                        id: ExternalId::parse(&uuid::Uuid::now_v7().to_string()).unwrap(),
                        batch_id: batch.clone(),
                        project_id: project,
                        epic_id: epic,
                        task_id: Some(task),
                        link_id: Some(kontor_core::id::TicketLinkId::generate()),
                        ordinal: 0,
                        item_kind: JiraItemKind::Task,
                        intent_kind: JiraIntentKind::Link,
                        requested_key: Some(ExternalId::parse("EXM-2").unwrap()),
                        marker: ExternalId::parse("checkout-fixture-marker").unwrap(),
                    }],
                )
                .unwrap();
            let item = store
                .jira_materialization_items(project, &batch)
                .unwrap()
                .remove(0);
            store
                .confirm_jira_materialization_item(
                    &item,
                    &ExternalId::parse("EXM-2").unwrap(),
                    &ExternalId::parse("20002").unwrap(),
                    &ContentHash::of(b"readback"),
                    now,
                )
                .unwrap();
            store
                .confirm_jira_materialization_batch(project, &batch, now)
                .unwrap();
        }
        let (services, state) = compose(&home, folder.clone(), store);
        Self {
            _directory: directory,
            root,
            folder,
            services,
            state,
            project,
            task,
            claim,
        }
    }
    fn pin(&self) -> kontor_store::checkouts::TaskCheckoutPlacement {
        self.state
            .with_store(|store| store.task_checkout_placement(self.project, self.task, &self.claim))
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn root_only_explicit_task_is_prepared_and_replay_ignores_host_edits() {
    let mut world = World::new(None, true);
    world
        .services
        .prepare_task_checkout(world.project, world.task)
        .await
        .unwrap();
    let pin = world.pin();
    assert!(pin.readback.is_some());
    let project = world
        .state
        .with_store(|s| s.get_project(world.project))
        .unwrap()
        .unwrap();
    let expected = fixture::snapshot(
        world.folder.checkouts.as_ref().unwrap(),
        Path::new(project.root_path.as_str()),
        None,
        Path::new(world.claim.as_str()),
    );
    assert_eq!(
        pin.intent,
        CanonicalDocument::from_serializable(&expected).unwrap()
    );
    let frozen: kontor_runtime::checkout::TaskCheckout =
        serde_json::from_str(pin.intent.json()).unwrap();
    kontor_runtime::checkout::verify(&frozen).unwrap();
    assert_eq!(frozen.module, None);
    let other = fixture::repository(&world.root.join("unrelated"));
    world.folder.checkouts = Some(fixture::inputs(&other));
    let services = Services::new(
        world.state.realm_id(),
        crate::DEFAULT_CAPACITY,
        kontor_jira::JiraConnectors::default(),
        world.root.join("runtime-roots"),
        crate::usage::UsagePoller::discover(&world.root),
        Vec::new(),
        None,
        Arc::new(crate::fleet::FleetSource::at(&world.root)),
        Arc::new(crate::succession::UnavailableSuccessionSummarizer),
        Vec::new(),
        Some(world.folder.clone()),
    )
    .unwrap();
    services.attach(world.state.clone());
    services
        .prepare_task_checkout(world.project, world.task)
        .await
        .unwrap();
    assert_eq!(world.pin().intent, pin.intent);
    assert_eq!(world.pin().readback, pin.readback);
    // A successful binding is not silently recreated after its destination disappears.
    fixture::git(
        Path::new(&frozen.source.declaration.path),
        &["worktree", "remove", &frozen.destination],
    );
    assert!(
        services
            .prepare_task_checkout(world.project, world.task)
            .await
            .is_err()
    );
    assert!(!Path::new(&frozen.destination).exists());
}

#[tokio::test]
async fn failed_git_effect_retains_pending_intent_and_retries_the_frozen_base() {
    let world = World::new(Some("components/worker"), true);
    let project = world
        .state
        .with_store(|s| s.get_project(world.project))
        .unwrap()
        .unwrap();
    let source = Path::new(project.root_path.as_str());
    let oid = fixture::git(source, &["rev-parse", "HEAD"]);
    fixture::git(
        source,
        &[
            "update-ref",
            "refs/remotes/custom/feat/EXM-2-placed-task",
            &oid,
        ],
    );
    let error = world
        .services
        .prepare_task_checkout(world.project, world.task)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("checkout.remote_task_branch_input_required"));
    let pending = world.pin();
    assert!(pending.readback.is_none());
    assert!(!Path::new(world.claim.as_str()).exists());
    fixture::git(
        source,
        &[
            "update-ref",
            "-d",
            "refs/remotes/custom/feat/EXM-2-placed-task",
        ],
    );
    std::fs::write(source.join("advance.txt"), "later").unwrap();
    fixture::git(source, &["add", "."]);
    fixture::git(source, &["commit", "-m", "advance"]);
    world
        .services
        .prepare_task_checkout(world.project, world.task)
        .await
        .unwrap();
    assert_eq!(world.pin().intent, pending.intent);
    assert!(world.pin().readback.is_some());
    assert_eq!(
        fixture::git(Path::new(world.claim.as_str()), &["rev-parse", "HEAD"]),
        oid
    );
}

#[tokio::test]
async fn missing_source_or_confirmed_key_refuses_without_git_effect() {
    let mut world = World::new(None, true);
    world.folder.checkouts = None;
    let project = world
        .state
        .with_store(|s| s.get_project(world.project))
        .unwrap()
        .unwrap();
    let task = world
        .state
        .with_store(|s| s.get_task(world.project, world.task))
        .unwrap()
        .unwrap();
    assert_eq!(
        crate::task_checkouts::plan(
            &world.folder,
            &project,
            &task,
            &world.claim,
            &[TrackerKey::parse("EXM-2").unwrap()]
        )
        .unwrap_err()
        .0,
        "host.checkouts.not_configured"
    );
    let unconfirmed = World::new(None, false);
    assert!(
        format!(
            "{:?}",
            unconfirmed
                .services
                .prepare_task_checkout(unconfirmed.project, unconfirmed.task)
                .await
                .unwrap_err()
        )
        .contains("checkout.binding_unconfirmed_or_ambiguous")
    );
    assert!(!Path::new(unconfirmed.claim.as_str()).exists());
}

#[test]
fn full_module_layout_root_only_and_claim_correction_require_their_own_inputs() {
    let world = World::new(Some("components/worker"), true);
    let project = world
        .state
        .with_store(|s| s.get_project(world.project))
        .unwrap()
        .unwrap();
    let module = ModuleKey::parse("components/worker").unwrap();
    let title = ExternalName::parse("Placed task").unwrap();
    let links = vec![EpicTicketLink {
        connector: ConnectorKey::parse("connector.jira").unwrap(),
        external_issue_key: ExternalId::parse("EXM-2").unwrap(),
    }];
    let (_, derived) = world
        .services
        .place_task_branch(
            project.root_path.as_str(),
            None,
            &title,
            &title,
            &links,
            None,
            Some(&module),
        )
        .unwrap();
    assert!(
        derived.is_none(),
        "requested links cannot establish a derived claim"
    );
    let rendered = crate::task_checkouts::derived_claim(
        &world.folder,
        project.root_path.as_str(),
        &module,
        &TrackerKey::parse("EXM-2").unwrap(),
    )
    .unwrap();
    assert!(rendered.as_str().ends_with("/exm-2/components/worker"));
    assert!(
        world
            .services
            .place_task_branch(
                project.root_path.as_str(),
                None,
                &title,
                &title,
                &links,
                None,
                None
            )
            .is_err()
    );
    assert_eq!(
        world
            .services
            .place_task_branch(
                project.root_path.as_str(),
                None,
                &title,
                &title,
                &links,
                Some(world.claim.clone()),
                None
            )
            .unwrap()
            .0,
        Some(world.claim.clone())
    );
    assert_eq!(
        world
            .services
            .place_task_branch(
                project.root_path.as_str(),
                None,
                &title,
                &title,
                &[],
                None,
                Some(&module)
            )
            .unwrap(),
        (None, None)
    );
}

#[tokio::test]
async fn correction_freezes_its_source_with_claim_and_rejects_changed_preview() {
    let world = World::new(Some("components/worker"), true);
    let project = world
        .state
        .with_store(|s| s.get_project(world.project))
        .unwrap()
        .unwrap();
    let (target, _) = crate::project_paths::render_layout(
        &world.folder.project.worktree_layout,
        project.root_path.as_str(),
        &TrackerKey::parse("EXM-2").unwrap(),
        &ModuleKey::parse("components/worker").unwrap(),
    )
    .unwrap();
    let target = ExternalName::parse(&target).unwrap();
    let prepare = || {
        world.services.prepare_worktree_claim_correction(
            world.project,
            world.task,
            AggregateRevision::INITIAL,
            &world.claim,
            &target,
        )
    };
    let preview = prepare().unwrap();
    assert!(!Path::new(target.as_str()).exists(), "preview is read-only");
    let source = Path::new(project.root_path.as_str());
    std::fs::write(source.join("advance.txt"), "later").unwrap();
    fixture::git(source, &["add", "."]);
    fixture::git(source, &["commit", "-m", "advance"]);
    let key = IdempotencyKey::parse("checkout-correction").unwrap();
    let mut request = WorktreeClaimCorrectionApplyRequest {
        expected_revision: AggregateRevision::INITIAL,
        old_worktree: world.claim.clone(),
        new_worktree: target.clone(),
        preview_hash: preview.preview_hash.as_str().to_owned(),
    };
    assert!(
        world
            .services
            .apply_worktree_claim_correction(&key, world.project, world.task, &request)
            .await
            .is_err()
    );
    assert_eq!(
        world
            .state
            .with_store(|s| s.task_worktree(world.project, world.task))
            .unwrap(),
        Some(world.claim.clone())
    );
    request.preview_hash = prepare().unwrap().preview_hash.as_str().to_owned();
    world
        .services
        .apply_worktree_claim_correction(&key, world.project, world.task, &request)
        .await
        .unwrap();
    let pin = world
        .state
        .with_store(|s| s.task_checkout_placement(world.project, world.task, &target))
        .unwrap()
        .unwrap();
    assert!(pin.readback.is_none());
    assert!(!Path::new(target.as_str()).exists());
    world
        .services
        .prepare_task_checkout(world.project, world.task)
        .await
        .unwrap();
    assert!(
        world
            .state
            .with_store(|s| s.task_checkout_placement(world.project, world.task, &target))
            .unwrap()
            .unwrap()
            .readback
            .is_some()
    );
    let stale = world
        .services
        .prepare_worktree_claim_correction(
            world.project,
            world.task,
            AggregateRevision::parse(2).unwrap(),
            &world.claim,
            &target,
        )
        .err()
        .unwrap();
    assert!(format!("{stale:?}").contains("task moved"));
}

#[test]
fn root_only_claim_is_not_eligible_for_deterministic_correction() {
    let world = World::new(None, true);
    let target = ExternalName::parse(world.root.join("different").to_str().unwrap()).unwrap();
    let result = world.services.prepare_worktree_claim_correction(
        world.project,
        world.task,
        AggregateRevision::INITIAL,
        &world.claim,
        &target,
    );
    assert!(format!("{:?}", result.err().unwrap()).contains("worktree_module_missing"));
    assert_eq!(
        world
            .state
            .with_store(|s| s.task_worktree(world.project, world.task))
            .unwrap(),
        Some(world.claim.clone())
    );
}

#[tokio::test]
async fn public_task_materialization_creates_from_host_source_before_native_binding_and_replays() {
    for world in [
        World::new(None, true),
        World::with_claim(Some("components/worker"), true, false),
    ] {
        let epic = world
            .state
            .with_store(|s| s.get_task(world.project, world.task))
            .unwrap()
            .unwrap()
            .mini_project_id
            .unwrap();
        world.state.with_store(|store| {
            let now = kontor_api::now();
            store
                .declare_epic_execution_scope(
                    world.project,
                    epic,
                    &kontor_store::EpicExecutionScopeDeclaration {
                        external_epic_key: ExternalId::parse("EXM-1").unwrap(),
                        short_title: ExternalName::parse("Checkout epic").unwrap(),
                        kontor_backlog_code: None,
                        ai_short_name: None,
                    },
                    now,
                )
                .unwrap();
            store
                .assign_epic_backlog_code(world.project, epic, None, now)
                .unwrap();
            let batch = ExternalId::parse(&uuid::Uuid::now_v7().to_string()).unwrap();
            store
                .plan_jira_materialization(
                    &NewJiraMaterializationBatch {
                        id: batch.clone(),
                        project_id: world.project,
                        epic_id: epic,
                        idempotency_key: "confirm-epic".to_owned(),
                        preview_hash: ContentHash::of(b"epic"),
                        expected_revision: AggregateRevision::INITIAL,
                        created_at: now,
                    },
                    &[NewJiraMaterializationItem {
                        id: ExternalId::parse(&uuid::Uuid::now_v7().to_string()).unwrap(),
                        batch_id: batch.clone(),
                        project_id: world.project,
                        epic_id: epic,
                        task_id: None,
                        link_id: None,
                        ordinal: 0,
                        item_kind: JiraItemKind::Epic,
                        intent_kind: JiraIntentKind::Link,
                        requested_key: Some(ExternalId::parse("EXM-1").unwrap()),
                        marker: ExternalId::parse("checkout-epic-marker").unwrap(),
                    }],
                )
                .unwrap();
            let item = store
                .jira_materialization_items(world.project, &batch)
                .unwrap()
                .remove(0);
            store
                .confirm_jira_materialization_item(
                    &item,
                    &ExternalId::parse("EXM-1").unwrap(),
                    &ExternalId::parse("20001").unwrap(),
                    &ContentHash::of(b"epic-readback"),
                    now,
                )
                .unwrap();
            store
                .confirm_jira_materialization_batch(world.project, &batch, now)
                .unwrap();
        });
        let control = SemanticTopologyRequest {
            target: SemanticTopologyTargetDto::EpicControl { epic_id: epic },
            expected_revision: AggregateRevision::INITIAL,
        };
        world
            .services
            .materialize_topology(
                &IdempotencyKey::parse("checkout-control").unwrap(),
                world.project,
                &control,
            )
            .await
            .unwrap();
        assert!(
            !Path::new(world.claim.as_str()).exists(),
            "epic control does not create task worktrees"
        );
        let request = SemanticTopologyRequest {
            target: SemanticTopologyTargetDto::Ticket {
                task_id: world.task,
            },
            expected_revision: AggregateRevision::INITIAL,
        };
        let key = IdempotencyKey::parse("checkout-native-task").unwrap();
        world
            .services
            .materialize_topology(&key, world.project, &request)
            .await
            .unwrap();
        let pin = world.pin();
        assert!(pin.readback.is_some());
        let node = world
            .state
            .with_store(|s| s.get_task_topology_node(world.project, world.task))
            .unwrap()
            .unwrap();
        let native = world
            .state
            .with_store(|s| s.get_topology_node_container(world.project, node.id))
            .unwrap()
            .unwrap();
        assert_eq!(native.canonical_cwd, Some(world.claim.clone()));
        let adapter = world
            .state
            .runtimes()
            .get(&RuntimeKindKey::parse("fake.runtime").unwrap())
            .unwrap();
        let scope = world
            .services
            .execution_scope(world.project, epic, Some(world.task), adapter.as_ref())
            .unwrap();
        assert!(scope.task_checkout().unwrap().snapshot.is_some());
        world
            .services
            .materialize_topology(&key, world.project, &request)
            .await
            .unwrap();
        assert_eq!(world.pin().intent, pin.intent);
        assert_eq!(
            world
                .state
                .with_store(|s| s.get_topology_node_container(world.project, node.id))
                .unwrap()
                .unwrap()
                .identity,
            native.identity
        );
    }
}
