//! Explicit historical graph fixtures for tests of pre-domain migration/recovery.
//!
//! These use the typed store constructors, not the new epic-creation API. No
//! existing pin is erased and no constraint or production refusal is disabled.

use super::*;
use kontor_core::id::{AggregateRevision, ConnectorKey, ModuleKey, SeatBindingId, SpecVersion};
use kontor_core::repository::{NewAdaptiveAdmissionState, NewSeatBinding, StoredEpicRoster};
use kontor_core::spec::{EpicPresence, Shareability, ShareabilityTier};
use kontor_core::state::ImportedTaskState;
use kontor_store::{
    EpicApplication, EpicExecutionScopeDeclaration, EpicTask, EpicTicketLink, TeamTemplateSource,
};

pub(crate) async fn seed(world: &World, project: &str, body: &serde_json::Value) -> Answer {
    let request: kontor_api::applications::ApplyEpicRequest =
        serde_json::from_value(body.clone()).unwrap();
    assert!(request.initial_hold.is_none() && request.desk_adam_origin.is_none());
    assert!(request.tasks.iter().all(|task| task.modules.is_none()
        && task.import_state == kontor_api::applications::EpicImportStateDto::Ready));
    let project = ProjectId::parse(project).unwrap();
    let now = kontor_api::now();
    let pack = bundled_pack().unwrap();
    let bundle = resolve_profile(
        &pack,
        &kontor_profiles::PackCategoryKey::parse(&request.work_profile_category).unwrap(),
        now,
    )
    .unwrap();
    let tasks = request
        .tasks
        .iter()
        .map(|task| EpicTask {
            title: task.title.clone(),
            short_code: task.short_code.clone(),
            ai_short_name: task.ai_short_name.clone(),
            module: task
                .module
                .as_deref()
                .map(ModuleKey::parse)
                .transpose()
                .unwrap(),
            changed_modules: None,
            imported_state: ImportedTaskState::Ready,
            depends_on: task.depends_on.clone(),
            ticket_links: task
                .ticket_links
                .iter()
                .map(|link| EpicTicketLink {
                    connector: ConnectorKey::parse(&link.connector).unwrap(),
                    external_issue_key: ExternalId::parse(&link.external_issue_key).unwrap(),
                })
                .collect(),
            worktree: task
                .worktree
                .as_deref()
                .map(ExternalName::parse)
                .transpose()
                .unwrap(),
            derived_worktree: None,
        })
        .collect::<Vec<_>>();
    let scope = request
        .execution_scope
        .as_ref()
        .map(|scope| EpicExecutionScopeDeclaration {
            external_epic_key: scope.external_epic_key.clone(),
            short_title: scope.short_title.clone(),
            kontor_backlog_code: scope.kontor_backlog_code.clone(),
            ai_short_name: scope.ai_short_name.clone(),
        });
    let domain = kontor_profiles::bundled_operational_domain().unwrap();
    let state = world.daemon.state();
    let applied = state.with_store(|store| {
        let applied = store
            .apply_epic(&EpicApplication {
                project_id: project,
                name: request.name.clone(),
                epic_backlog_code: request.epic_backlog_code.as_ref(),
                execution_scope: scope.as_ref(),
                tasks: &tasks,
                profile: &bundle.profile,
                definition: &bundle.profile.definition,
                team: bundle.team.as_ref(),
                team_source: TeamTemplateSource::Bundled,
                applied_at: now,
            })
            .unwrap();
        assert_eq!(
            applied.applied,
            kontor_store::Applied::Created,
            "historical fixture construction is insert-only"
        );
        let roster = store
            .get_current_core_team(project)
            .unwrap()
            .map(|held| kontor_teams::CoreTeamRevision {
                version: held.version,
                catalog_hash: held.catalog_hash,
                seats: serde_json::from_value(held.seats).unwrap(),
            })
            .unwrap_or_else(|| {
                kontor_teams::CoreTeamRevision::resolve(
                    SpecVersion::FIRST,
                    &domain.role_catalogs[0],
                    &[],
                    domain.delivery.purpose_roles(),
                )
                .unwrap()
            });
        if store
            .get_role_catalog(
                domain.role_catalogs[0].catalog_id,
                domain.role_catalogs[0].version,
            )
            .unwrap()
            .is_none()
        {
            store
                .publish_role_catalog(
                    &domain.role_catalogs[0],
                    &Shareability::default_for(ShareabilityTier::ProjectKnowledge).unwrap(),
                    now,
                )
                .unwrap();
        }
        store
            .put_epic_roster(&StoredEpicRoster {
                project_id: project,
                mini_project_id: applied.mini_project_id,
                core_team_version: roster.version,
                catalog_hash: roster.catalog_hash.clone(),
                seats: serde_json::to_value(&roster.seats).unwrap(),
                quick_session_id: None,
                revision: AggregateRevision::INITIAL,
                pinned_at: now,
            })
            .unwrap();
        let seed = kontor_accounts::AdaptivePosition::initial(DEFAULT_CAPACITY.adaptive);
        store
            .create_adaptive_admission_state(&NewAdaptiveAdmissionState {
                project_id: project,
                mini_project_id: applied.mini_project_id,
                current_window: seed.current_window,
                clean_observation_streak: seed.clean_observation_streak,
                last_observation_id: seed.last_observation_id,
                created_at: now,
            })
            .unwrap();
        applied
    });
    install_geometry(world, project, applied.mini_project_id);
    Answer { status:StatusCode::OK, body:serde_json::json!({
        "project_id":project, "epic_id":applied.mini_project_id, "revision":applied.revision,
        "epic_backlog_code":applied.epic_backlog_code, "applied":"created", "tasks":applied.tasks.iter().map(|task| serde_json::json!({
            "task_id":task.task_id, "title":task.title, "revision":task.revision, "state":task.state.as_str(),
        })).collect::<Vec<_>>(),
    }).to_string() }
}

/// Install only explicitly historical pins/geometry/roster on an existing imported graph.
/// These constructors never create a domain pin or call a production materializer.
pub(crate) fn install_geometry(world: &World, project: ProjectId, epic: MiniProjectId) {
    let state = world.daemon.state();
    let now = kontor_api::now();
    let domain = kontor_profiles::bundled_operational_domain().unwrap();
    let roster = state.with_store(|store| {
        let held = match store.get_epic_roster(project, epic).unwrap() {
            Some(row) => row,
            None => {
                let current = store.get_current_core_team(project).unwrap().unwrap();
                let row = StoredEpicRoster {
                    project_id: project,
                    mini_project_id: epic,
                    core_team_version: current.version,
                    catalog_hash: current.catalog_hash,
                    seats: current.seats,
                    quick_session_id: None,
                    revision: AggregateRevision::INITIAL,
                    pinned_at: now,
                };
                store.put_epic_roster(&row).unwrap();
                row
            }
        };
        kontor_teams::CoreTeamRevision {
            version: held.core_team_version,
            catalog_hash: held.catalog_hash,
            seats: serde_json::from_value(held.seats).unwrap(),
        }
    });
    state.with_store(|store| {
        use kontor_core::repository::{
            MiniProjectTeamDefinitionSnapshot, MiniProjectTopologySnapshot, NewSessionTopologyNode,
        };
        for spec in &domain.topology_specs {
            if store
                .get_topology_spec(project, spec.spec_id, spec.version)
                .unwrap()
                .is_none()
            {
                store
                    .publish_topology_spec(
                        project,
                        spec,
                        &Shareability::default_for(ShareabilityTier::ProjectKnowledge).unwrap(),
                        now,
                    )
                    .unwrap();
            }
        }
        for definition in &domain.team_definitions {
            if store
                .get_team_definition(project, definition.definition_id, definition.version)
                .unwrap()
                .is_none()
            {
                store
                    .publish_team_definition(project, definition, now)
                    .unwrap();
            }
        }
        if store
            .get_project_team_definition_default(project)
            .unwrap()
            .is_none()
        {
            let definition = &domain.team_definitions[0];
            store
                .set_project_team_definition_default(
                    &kontor_core::repository::ProjectTeamDefinitionDefault {
                        project_id: project,
                        expected: None,
                        definition: kontor_core::spec::TeamDefinitionSnapshot::from_revision(
                            definition,
                        )
                        .unwrap(),
                        operational_domain_bundle_hash: None,
                        selected_at: now,
                    },
                    None,
                )
                .unwrap();
        }
        let selected = store
            .get_project_team_definition_default(project)
            .unwrap()
            .unwrap();
        let definition = store
            .get_team_definition(
                project,
                selected.definition.definition_id,
                selected.definition.version,
            )
            .unwrap()
            .unwrap();
        let topology = definition.topology.clone();
        let spec = store
            .get_topology_spec(project, topology.spec_id, topology.version)
            .unwrap()
            .unwrap();
        store
            .pin_mini_project_topology(&MiniProjectTopologySnapshot {
                project_id: project,
                mini_project_id: epic,
                topology: topology.clone(),
                pinned_at: now,
            })
            .unwrap();
        store
            .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
                project_id: project,
                mini_project_id: epic,
                definition: selected.definition,
                operational_domain_bundle_hash: None,
                pinned_at: now,
            })
            .unwrap();
        let existing = store.list_topology_nodes(project, None).unwrap();
        let root = match existing
            .iter()
            .find(|node| node.kind == spec.root_kind && node.parent_id.is_none())
        {
            Some(node) => node.id,
            None => {
                let id = TopologyNodeId::generate();
                store
                    .create_topology_node(&NewSessionTopologyNode {
                        id,
                        project_id: project,
                        mini_project_id: None,
                        topology: topology.clone(),
                        kind: spec.root_kind.clone(),
                        parent_id: None,
                        task_id: None,
                        created_at: now,
                    })
                    .unwrap();
                id
            }
        };
        let epic_node = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: epic_node,
                project_id: project,
                mini_project_id: Some(epic),
                topology: topology.clone(),
                kind: domain.delivery.epic_kind.clone(),
                parent_id: Some(root),
                task_id: None,
                created_at: now,
            })
            .unwrap();
        let control = store
            .create_topology_node(&NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id: project,
                mini_project_id: Some(epic),
                topology,
                kind: domain.delivery.control_kind.clone(),
                parent_id: Some(epic_node),
                task_id: None,
                created_at: now,
            })
            .unwrap();
        for seat in &roster.seats {
            if seat.presence == EpicPresence::OnDemand {
                continue;
            }
            store
                .create_seat_binding(&NewSeatBinding {
                    id: SeatBindingId::generate(),
                    project_id: project,
                    topology_node_id: control.id,
                    role_slot_id: seat.role_slot_id.clone(),
                    role: seat.role.clone(),
                    task_id: None,
                    team_run_id: None,
                    parent_seat_binding_id: None,
                    attach_deadline: now
                        .checked_add(jiff::SignedDuration::from_secs(600))
                        .unwrap(),
                    created_at: now,
                })
                .unwrap();
        }
        assert!(
            store
                .get_mini_project_team_definition(project, epic)
                .unwrap()
                .unwrap()
                .operational_domain_bundle_hash
                .is_none()
        );
    });
}
