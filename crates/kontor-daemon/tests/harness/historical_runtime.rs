//! Test-only pre-domain-pin state. No production writer can recreate it.
//!
//! Migration recovery needs an unpinned historical epic: a new pinned epic
//! correctly refuses definition-only migration (`operational_domain_upgrade_unsupported`).
//! Constructors and the scripted runtime model effects that already existed.
use super::*;
use kontor_core::id::SeatBindingId;
use kontor_core::repository::{NewSeatBinding, TopologyRepository, WorkflowRepository};
use kontor_core::spec::{CatalogRoleRef, SeatAutonomy};
use kontor_runtime::container::{
    ContainerBindingId, ContainerInspectRequest, ContainerProjection, ContainerRequest,
};
use std::path::PathBuf;

pub(crate) fn scope(
    world: &World,
    project: ProjectId,
    epic: MiniProjectId,
    task: Option<TaskId>,
) -> ExecutionScope {
    world.daemon.state().with_store(|store| {
        let declared = store
            .get_epic_execution_scope(project, epic)
            .unwrap()
            .unwrap();
        let epic_scope = EpicScope {
            mini_project_id: epic,
            external_epic_key: declared.external_epic_key,
            short_title: declared.short_title,
        };
        match task {
            None => ExecutionScope::for_epic(epic_scope),
            Some(task) => ExecutionScope::for_task(
                epic_scope,
                TaskScope {
                    task_id: task,
                    external_issue_key: store
                        .confirmed_jira_task_key(project, task)
                        .unwrap()
                        .unwrap(),
                    short_code: store.task_short_code(project, task).unwrap(),
                    worktree: WorkspaceRoot::parse(
                        std::fs::canonicalize(
                            store
                                .task_worktree(project, task)
                                .unwrap()
                                .unwrap()
                                .as_str(),
                        )
                        .unwrap()
                        .to_str()
                        .unwrap(),
                    )
                    .unwrap(),
                },
            ),
        }
    })
}

fn historical_container_title(
    world: &World,
    node: &kontor_core::state::SessionTopologyNode,
    spec: &kontor_core::spec::ProjectSessionTopologySpec,
    scope: Option<&ExecutionScope>,
) -> ExternalName {
    use kontor_core::naming::NativeNameValues;
    let state = world.daemon.state();
    let declared = spec
        .node_kinds
        .iter()
        .find(|k| k.kind == node.kind)
        .unwrap();
    let definition = node.mini_project_id.and_then(|epic| {
        state.with_store(|store| {
            store
                .get_mini_project_team_definition(node.project_id, epic)
                .unwrap()
                .map(|pin| {
                    store
                        .get_team_definition(
                            node.project_id,
                            pin.definition.definition_id,
                            pin.definition.version,
                        )
                        .unwrap()
                        .unwrap()
                })
        })
    });
    let (template, separator, prefix) = match &definition {
        Some(definition) => {
            let c = definition.container(&node.kind).unwrap();
            (&c.name_template, &definition.separator, c.prefix.as_str())
        }
        None => (
            &declared.name_template,
            &spec.name_separator,
            node.kind.as_str(),
        ),
    };
    let mut values = NativeNameValues::new()
        .with_area_code(node.kind.as_str())
        .with_prefix(prefix);
    let mut item = None;
    if let Some(scope) = scope {
        let key = scope
            .task
            .as_ref()
            .map_or(&scope.epic.external_epic_key, |task| {
                &task.external_issue_key
            });
        values = values
            .with_jira_code(key.as_str())
            .with_epic_jira_key(
                &kontor_core::backlog_identity::ConfirmedJiraKey::parse(
                    &scope.epic.external_epic_key,
                )
                .unwrap(),
            )
            .with_scope_jira_key(
                &kontor_core::backlog_identity::ConfirmedJiraKey::parse(key).unwrap(),
            );
        if let Some(task) = &scope.task {
            values = values.with_task_jira_key(
                &kontor_core::backlog_identity::ConfirmedJiraKey::parse(&task.external_issue_key)
                    .unwrap(),
            );
        }
        item = state
            .with_store(|store| {
                store.epic_backlog_code(node.project_id, scope.epic.mini_project_id)
            })
            .unwrap()
            .map(|backlog| {
                kontor_core::backlog_identity::JiraItemCode::derive(&backlog, key).unwrap()
            });
    }
    if let Some(item) = &item {
        values = values
            .with_item_code(item.as_str())
            .with_epic_item_code(item.as_str())
            .with_task_item_code(item.as_str())
            .with_scope_item_code(item.as_str());
    }
    template.render(separator, &values).unwrap()
}

pub(crate) async fn containers(
    world: &World,
    project: ProjectId,
    epic: MiniProjectId,
    task: TaskId,
) {
    let state = world.daemon.state();
    assert!(
        state
            .with_store(|s| s.get_mini_project_team_definition(project, epic))
            .unwrap()
            .is_none_or(|pin| pin.operational_domain_bundle_hash.is_none())
    );
    let (topology, spec, epic_node) = state.with_store(|store| {
        let topology = store
            .get_mini_project_topology(project, epic)
            .unwrap()
            .unwrap()
            .topology;
        let spec = store
            .get_topology_spec(project, topology.spec_id, topology.version)
            .unwrap()
            .unwrap();
        let node = store
            .list_topology_nodes(project, Some(epic))
            .unwrap()
            .into_iter()
            .find(|n| n.kind.as_str() == "ESW")
            .unwrap();
        (topology, spec, node)
    });
    if state
        .with_store(|s| s.get_task_topology_node(project, task))
        .unwrap()
        .is_none()
    {
        state
            .with_store(|s| {
                s.create_topology_node(&NewSessionTopologyNode {
                    id: TopologyNodeId::generate(),
                    project_id: project,
                    mini_project_id: Some(epic),
                    topology,
                    kind: TopologyKindKey::parse("TSW").unwrap(),
                    parent_id: Some(epic_node.id),
                    task_id: Some(task),
                    created_at: kontor_api::now(),
                })
            })
            .unwrap();
    }
    let mut nodes = state
        .with_store(|s| s.list_topology_nodes(project, None))
        .unwrap();
    nodes.extend(
        state
            .with_store(|s| s.list_topology_nodes(project, Some(epic)))
            .unwrap(),
    );
    nodes.sort_by_key(|n| match n.kind.as_str() {
        "PSW" => 0,
        "ESW" => 1,
        "ECP" => 2,
        _ => 3,
    });
    let mut bindings = std::collections::BTreeMap::<
        TopologyNodeId,
        kontor_runtime::container::ContainerBinding,
    >::new();
    for node in nodes {
        if node.mini_project_id.is_some_and(|id| id != epic)
            || node.task_id.is_some_and(|id| id != task)
        {
            continue;
        }
        let capabilities = spec
            .node_kinds
            .iter()
            .find(|k| k.kind == node.kind)
            .unwrap()
            .projection_capabilities
            .clone();
        let projection = ContainerProjection::resolve(&capabilities).unwrap();
        if projection == ContainerProjection::LogicalOnly {
            continue;
        }
        let node_scope = node
            .mini_project_id
            .map(|_| scope(world, project, epic, node.task_id));
        let directory = if node.task_id.is_some() {
            PathBuf::from(
                node_scope
                    .as_ref()
                    .unwrap()
                    .require_task()
                    .unwrap()
                    .worktree
                    .as_str(),
            )
        } else {
            world
                .directory
                .path()
                .join("historical-runtime")
                .join(node.id.to_string())
        };
        std::fs::create_dir_all(&directory).unwrap();
        let cwd =
            WorkspaceRoot::parse(directory.canonicalize().unwrap().to_str().unwrap()).unwrap();
        let held = state
            .with_store(|s| s.get_topology_node_container(project, node.id))
            .unwrap();
        let request = ContainerRequest {
            container_binding_id: held
                .as_ref()
                .map(|h| ContainerBindingId::parse(h.container_binding_id.as_str()).unwrap())
                .unwrap_or_else(ContainerBindingId::generate),
            topology_node_id: node.id,
            topology: node.topology.clone(),
            capabilities,
            display_name: historical_container_title(world, &node, &spec, node_scope.as_ref()),
            parent: if projection == ContainerProjection::NativeChild {
                Some(bindings.get(&node.parent_id.unwrap()).unwrap().clone())
            } else {
                None
            },
            cwd: Some(cwd),
            bound_native_id: held.map(|h| h.identity.native_id),
            task_id: node.task_id,
            team_run_id: None,
            scope: node_scope,
            epic_container: projection == ContainerProjection::NativeRoot
                && node.mini_project_id.is_some(),
            requested_at: kontor_api::now(),
        };
        let prepared = world
            .fake
            .prepare_container(&request)
            .await
            .unwrap()
            .snapshot;
        let inspection = world
            .fake
            .inspect_container(&ContainerInspectRequest {
                binding: prepared.binding,
                native_parent: request.parent.map(|p| p.identity),
                scope: request.scope,
                epic_container: request.epic_container,
                requested_at: kontor_api::now(),
            })
            .await
            .unwrap();
        state
            .with_store(|s| {
                s.bind_topology_node_container(&NewNativeContainerBinding {
                    topology_node_id: node.id,
                    project_id: project,
                    container_binding_id: ExternalId::parse(&inspection.binding.id.to_string())
                        .unwrap(),
                    identity: inspection.binding.identity.clone(),
                    observed_kind: inspection.observed_kind,
                    canonical_cwd: inspection
                        .canonical_cwd
                        .as_ref()
                        .map(|p| ExternalName::parse(p.as_str()).unwrap()),
                    readback: Some(kontor_core::state::NativeContainerReadback {
                        projection: if projection == ContainerProjection::NativeRoot {
                            kontor_core::state::ObservedContainerProjection::NativeRoot
                        } else {
                            kontor_core::state::ObservedContainerProjection::NativeChild
                        },
                        visible_title: ExternalName::parse(&inspection.visible_title).unwrap(),
                        native_parent: inspection.native_parent.clone(),
                        topology_correlation: ExternalName::parse(
                            &inspection.correlation.label.to_string(),
                        )
                        .unwrap(),
                    }),
                    bound_at: inspection.binding.bound_at,
                    observed_at: inspection.observed_at,
                })
            })
            .unwrap();
        bindings.insert(node.id, inspection.binding);
    }
}

pub(crate) async fn seated_team(
    world: &World,
    project: ProjectId,
    epic: MiniProjectId,
    account: kontor_core::id::AccountProfileId,
) -> serde_json::Value {
    let state = world.daemon.state();
    let (task, snapshot, template, node, parent) = state.with_store(|store| {
        let task = store
            .list_tasks(project)
            .unwrap()
            .into_iter()
            .find(|task| task.mini_project_id == Some(epic))
            .unwrap();
        let workflow = store
            .get_active_task_workflow(project, task.id)
            .unwrap()
            .unwrap();
        let pin = workflow.snapshot.definition.team_template.unwrap();
        let revision = store
            .get_team_template(project, pin.template_id, pin.version)
            .unwrap()
            .unwrap();
        let snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION);
        let template = kontor_teams::spec::TeamTemplateSpec::from_revision(&revision).unwrap();
        let node = store
            .get_task_topology_node(project, task.id)
            .unwrap()
            .unwrap();
        let control = store
            .list_topology_nodes(project, Some(epic))
            .unwrap()
            .into_iter()
            .find(|n| n.kind.as_str() == "ECP")
            .unwrap();
        let parent = store
            .list_seat_bindings(project, control.id)
            .unwrap()
            .into_iter()
            .find(|s| s.role.role_code.as_str() == "TPM")
            .unwrap()
            .id;
        (task, snapshot, template, node, parent)
    });
    let team = TeamRunId::generate();
    state
        .with_store(|store| {
            store.create_team_run(&NewTeamRun {
                id: team,
                project_id: project,
                task_id: task.id,
                snapshot,
                created_at: kontor_api::now(),
            })
        })
        .unwrap();
    let scope = scope(world, project, epic, Some(task.id));
    let host = state
        .with_store(|store| store.get_topology_node_container(project, node.id))
        .unwrap()
        .unwrap();
    let binding = kontor_runtime::container::ContainerBinding {
        id: ContainerBindingId::parse(host.container_binding_id.as_str()).unwrap(),
        topology_node_id: node.id,
        projection: ContainerProjection::NativeChild,
        identity: host.identity.clone(),
        root: host
            .canonical_cwd
            .as_ref()
            .map(|p| WorkspaceRoot::parse(p.as_str()).unwrap()),
        bound_at: host.bound_at,
    };
    let inspection = world
        .fake
        .inspect_container(&ContainerInspectRequest {
            binding,
            native_parent: host.readback.unwrap().native_parent,
            scope: Some(scope.clone()),
            epic_container: false,
            requested_at: kontor_api::now(),
        })
        .await
        .unwrap();
    let container = kontor_runtime::container::ContainerBindingSnapshot {
        binding: inspection.binding,
        capabilities: world.fake.capabilities(),
        correlation: inspection.correlation,
    };
    let domain = kontor_profiles::bundled_operational_domain().unwrap();
    let definition = state.with_store(|store| {
        let pin = store
            .get_mini_project_team_definition(project, epic)
            .unwrap()
            .unwrap();
        store
            .get_team_definition(
                project,
                pin.definition.definition_id,
                pin.definition.version,
            )
            .unwrap()
            .unwrap()
    });
    let mut seats = Vec::new();
    for slot in &template.slots {
        let run = AgentRunId::generate();
        let binding = RuntimeBindingId::generate();
        let code = definition
            .team_slot(&node.kind, &slot.id)
            .unwrap()
            .role_code
            .clone()
            .unwrap();
        let catalog = &domain.role_catalogs[0];
        let authority = world
            .fake
            .admit_launch(&AdmissionRequest {
                slot: RoleSlotKey::new(team, slot.id.clone()),
                agent_run_id: run,
                binding_id: binding,
                replaces: None,
                requested_at: kontor_api::now(),
            })
            .await
            .unwrap()
            .into_authority()
            .unwrap();
        let outcome = world
            .fake
            .launch(
                &authority.into_request(LaunchParts {
                    scope: scope.clone(),
                    display_name: definition
                        .container(&node.kind)
                        .unwrap()
                        .seat_name_template
                        .as_ref()
                        .unwrap()
                        .render(
                            &definition.separator,
                            &kontor_core::naming::NativeNameValues::new()
                                .with_role_code(code.as_str()),
                        )
                        .unwrap(),
                    agent_run_id: run,
                    team_run_id: team,
                    role_slot_id: slot.id.clone(),
                    task_id: task.id,
                    binding_id: binding,
                    placement: Some(LaunchPlacement::Container(container.clone())),
                    cwd: scope.require_task().unwrap().worktree.clone(),
                    account_profile_id: Some(account),
                    prompt: BoundedText::parse("Previously authorized historical work").unwrap(),
                    model_rung: kontor_core::spec::ModelRung {
                        provider: kontor_core::spec::ProviderRef("test".into()),
                        model: kontor_core::spec::ModelRef("test".into()),
                        effort: None,
                    },
                    context_policy: kontor_core::spec::ContextPolicySnapshot::standard(
                        &kontor_core::spec::ContextWindowBounds::unknown(),
                        true,
                        SCHEMA_VERSION,
                        kontor_api::now(),
                    )
                    .unwrap(),
                    autonomy: SeatAutonomy::standard(),
                    requested_at: kontor_api::now(),
                }),
            )
            .await
            .unwrap();
        state.with_store(|store| {
            store
                .create_agent_run(&NewAgentRun {
                    id: run,
                    project_id: project,
                    team_run_id: team,
                    parent_agent_run_id: None,
                    role: slot.id.clone().into_role_key(),
                    account_profile_id: Some(account),
                    binding: Some(RuntimeBinding {
                        id: outcome.snapshot.binding_id(),
                        agent_run_id: run,
                        identity: outcome.snapshot.identity().clone(),
                        bound_at: kontor_api::now(),
                    }),
                    created_at: kontor_api::now(),
                })
                .unwrap();
            store
                .persist_binding_snapshot(
                    outcome.snapshot.binding_id(),
                    run,
                    &serde_json::to_string(&outcome.snapshot).unwrap(),
                )
                .unwrap();
            store
                .create_seat_binding(&NewSeatBinding {
                    id: SeatBindingId::generate(),
                    project_id: project,
                    topology_node_id: node.id,
                    role_slot_id: slot.id.clone(),
                    role: CatalogRoleRef {
                        catalog_id: catalog.catalog_id,
                        catalog_revision: catalog.version,
                        standard_title: catalog.role(&code).unwrap().standard_title.clone(),
                        role_code: code,
                        custom_display_name: None,
                    },
                    task_id: Some(task.id),
                    team_run_id: Some(team),
                    attach_deadline: kontor_api::now()
                        .checked_add(jiff::SignedDuration::from_secs(600))
                        .unwrap(),
                    parent_seat_binding_id: Some(parent),
                    created_at: kontor_api::now(),
                })
                .unwrap();
        });
        state.sessions().record(outcome.snapshot.clone());
        seats.push(serde_json::json!({"agent_run_id":run,"team_run_id":team,"task_id":task.id,"role_slot":slot.id,"runtime_kind":outcome.snapshot.identity().runtime_kind,"native_id":outcome.snapshot.identity().native_id,"applied":"created"}));
    }
    serde_json::json!(seats)
}
