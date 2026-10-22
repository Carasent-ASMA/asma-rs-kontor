//! Runtime semantics follow each epic's verified immutable domain selection.

use super::domain_bundle_tests::start;
use super::domain_selection_tests::{activate, request, select};
use super::epic_domain_tests::{command, selected};
use super::*;

pub(super) fn epic(services: &Services, project: ProjectId, name: &str) -> MiniProjectId {
    let name = ExternalName::parse(name).unwrap();
    let (binding, selection) = services.epic_domain_binding(project, &name, None).unwrap();
    let now = kontor_api::now();
    let (profile, source) = services.bundle_with_team_source("code", now).unwrap();
    let envelope = command(services, project, &binding, name.as_str());
    services
        .state()
        .unwrap()
        .with_store(|store| {
            store.apply_epic_with_domain(
                &EpicApplication {
                    project_id: project,
                    name,
                    epic_backlog_code: None,
                    execution_scope: None,
                    tasks: &[],
                    profile: &profile.profile,
                    definition: &profile.profile.definition,
                    team: profile.team.as_ref(),
                    team_source: source,
                    applied_at: now,
                },
                &binding,
                selection.as_ref(),
                &envelope,
            )
        })
        .unwrap()
        .unwrap()
        .mini_project_id
}

#[tokio::test]
async fn runtime_domain_kinds_and_prompts_stay_with_each_epic_across_activation_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, mut bundle, first_hash) = selected(&services, &folder).await;
    let first = epic(&services, project, "first-domain-epic");
    let role = RoleCode::parse("LSA").unwrap();
    assert!(
        services
            .freeze_epic_role_persona(project, first, &role)
            .unwrap()
            .is_none()
    );
    let original = services.runtime_domain(project, Some(first)).unwrap();
    let mut yaml: serde_json::Value =
        serde_yaml_ng::from_str(bundle.operational_domain.as_ref().unwrap()).unwrap();
    yaml["role_prompts"] =
        serde_json::json!([{"role_code":"LSA", "prompt":"Only the second pinned domain"}]);
    yaml["delivery"]["epic_kind"] = "ASW".into();
    yaml["delivery"]["control_kind"] = "CSW".into();
    let bytes = serde_yaml_ng::to_string(&yaml).unwrap();
    let descriptor = bundle.manifest.operational_domain.as_mut().unwrap();
    descriptor.source_hash = ContentHash::of(bytes.as_bytes());
    bundle
        .manifest
        .sources
        .insert(descriptor.source.clone(), descriptor.source_hash.clone());
    bundle.operational_domain = Some(bytes.clone());
    let second_hash = services.fleet.publish_bundle(&bundle).unwrap().bundle_hash;
    std::fs::write(folder.join("packs/operational-domain.yml"), &bytes).unwrap();
    assert_eq!(
        services
            .runtime_domain(project, Some(first))
            .unwrap()
            .bundle_hash,
        first_hash
    );
    assert!(
        services
            .freeze_epic_role_persona(project, first, &role)
            .unwrap()
            .is_none()
    );
    activate(&services, &second_hash);
    let domain = services.fleet.operational_domain(&second_hash).unwrap().1;
    let preview = services
        .preview_project_team_definition_selection(project, &request(&domain))
        .unwrap();
    select(
        &services,
        project,
        preview.preview_hash,
        "runtime-domain-second",
    )
    .await
    .unwrap();
    let second = epic(&services, project, "second-domain-epic");
    for (id, kind, control, prompt) in [
        (
            first,
            original.domain.delivery.epic_kind.as_str(),
            original.domain.delivery.control_kind.as_str(),
            None,
        ),
        (second, "ASW", "CSW", Some("Only the second pinned domain")),
    ] {
        let scope = services
            .resolve_scope(project, &SemanticTopologyTargetDto::Epic { epic_id: id })
            .unwrap();
        assert_eq!(scope.kind.unwrap().as_str(), kind);
        assert_eq!(
            services.epic_control_kind(project, id).unwrap().as_str(),
            control
        );
        assert_eq!(
            services
                .freeze_epic_role_persona(project, id, &role)
                .unwrap()
                .map(|p| p.prompt.to_string())
                .as_deref(),
            prompt
        );
    }
    drop(services);
    drop(daemon);
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    assert_eq!(
        services
            .runtime_domain(project, Some(first))
            .unwrap()
            .bundle_hash,
        first_hash
    );
    assert_eq!(
        services
            .runtime_domain(project, Some(second))
            .unwrap()
            .bundle_hash,
        second_hash
    );
    assert!(
        services
            .freeze_epic_role_persona(project, first, &role)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn runtime_domain_refuses_tampered_hash_missing_artifact_and_unpinned_legacy() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, bundle, _) = selected(&services, &folder).await;
    let epic_id = epic(&services, project, "runtime-tamper-epic");
    let artifact = kontor_fleet_activation::domain::domain_path(
        &root,
        &bundle
            .manifest
            .operational_domain
            .as_ref()
            .unwrap()
            .source_hash,
    );
    let bytes = std::fs::read(&artifact).unwrap();
    // Valid YAML with a different byte hash must not supply runtime semantics.
    std::fs::write(
        &artifact,
        [bytes.as_slice(), b"\n# changed immutable bytes\n"].concat(),
    )
    .unwrap();
    let error = services
        .resolve_scope(project, &SemanticTopologyTargetDto::Epic { epic_id })
        .unwrap_err();
    assert!(error.rule.contains("hash"), "{error:?}");
    std::fs::remove_file(&artifact).unwrap();
    assert!(services.epic_control_kind(project, epic_id).is_err());
    std::fs::write(&artifact, &bytes).unwrap();
    assert!(services.runtime_domain(project, Some(epic_id)).is_ok());
    let legacy_project =
        domain_selection_tests::project(&services, services.purpose_source().unwrap());
    let legacy = MiniProjectId::generate();
    services
        .state()
        .unwrap()
        .with_store(|store| {
            store.create_mini_project(&NewMiniProject {
                id: legacy,
                project_id: legacy_project,
                name: ExternalName::parse("Legacy domain").unwrap(),
                created_at: kontor_api::now(),
            })
        })
        .unwrap();
    let error = services
        .epic_control_kind(legacy_project, legacy)
        .unwrap_err();
    assert!(error.rule.contains("operational_domain_unpinned"));
    assert!(
        services
            .freeze_epic_role_persona(legacy_project, legacy, &RoleCode::parse("LSA").unwrap())
            .unwrap_err()
            .rule
            .contains("operational_domain_unpinned")
    );
    assert!(
        services
            .runtime_domain(legacy_project, None)
            .err()
            .unwrap()
            .rule
            .contains("operational_domain_unpinned")
    );
    assert!(
        services
            .state()
            .unwrap()
            .with_store(|store| store.list_topology_nodes(legacy_project, Some(legacy)))
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn prepared_epic_launch_recovers_exact_persona_and_absence_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let path = folder.join("packs/operational-domain.yml");
    let mut yaml: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    yaml["role_prompts"] =
        serde_json::json!([{"role_code":"LSA", "prompt":"Frozen prepared launch persona"}]);
    std::fs::write(path, serde_yaml_ng::to_string(&yaml).unwrap()).unwrap();
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, _, _) = selected(&services, &folder).await;
    let id = epic(&services, project, "prepared-persona");
    let mut launches = Vec::new();
    for code in ["LSA", "TPM"] {
        let role = RoleCode::parse(code).unwrap();
        let scope = services
            .resolve_scope(
                project,
                &SemanticTopologyTargetDto::EpicControl { epic_id: id },
            )
            .unwrap();
        let control = services.ensure_scope_chain(project, &scope).unwrap();
        let roster = services.frozen_roster(project, id).unwrap();
        let seats = services
            .materialize_roster_seats(project, &control, &roster, kontor_api::now())
            .unwrap();
        let seat = seats
            .iter()
            .find(|(seat, _)| seat.role.role_code == role)
            .unwrap()
            .1;
        let intent = StoredHostedSeatLaunchIntent {
            project_id: project,
            seat_binding_id: seat,
            occupancy_generation: 1,
            autonomy: kontor_core::spec::SeatAutonomy::Supervised,
            model_rung: ModelRung {
                provider: kontor_core::spec::ProviderRef("codex".into()),
                model: kontor_core::spec::ModelRef("gpt-5.6".into()),
                effort: None,
            },
            state: HostedSeatLaunchIntentState::Prepared,
            observed_native_id: None,
            prepared_at: kontor_api::now(),
            installed_at: None,
        };
        let persona = services
            .prepare_epic_seat_launch(id, &role, &intent)
            .unwrap();
        launches.push((id, intent, persona));
    }
    drop(services);
    drop(daemon);
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    for (id, intent, persona) in launches {
        let role = services
            .state()
            .unwrap()
            .with_store(|store| store.get_seat_binding(project, intent.seat_binding_id))
            .unwrap()
            .unwrap()
            .role
            .role_code;
        assert_eq!(persona.is_some(), role.as_str() == "LSA");
        let context = services.runtime_domain(project, Some(id)).unwrap();
        let manifest = services
            .fleet
            .verify_bundle(&context.bundle_hash)
            .unwrap()
            .manifest;
        let artifact = kontor_fleet_activation::domain::domain_path(
            &root,
            &manifest.operational_domain.unwrap().source_hash,
        );
        let bytes = std::fs::read(&artifact).unwrap();
        std::fs::remove_file(&artifact).unwrap();
        assert_eq!(
            services
                .prepare_epic_seat_launch(id, &role, &intent)
                .unwrap(),
            persona
        );
        std::fs::write(artifact, bytes).unwrap();
    }
}

#[tokio::test]
async fn runtime_catalog_follows_the_verified_pin_instead_of_artifact_order() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let daemon = start(&directory.path().join("state"), &folder);
    let services = daemon.jira_reconciler();
    let (project, mut bundle, _) = selected(&services, &folder).await;
    let mut domain: OperationalDomainPack =
        serde_yaml_ng::from_str(bundle.operational_domain.as_ref().unwrap()).unwrap();
    let expected = domain.role_catalogs[0].clone();
    let mut foreign = expected.clone();
    foreign.catalog_id = RoleCatalogId::generate();
    for entry in &mut foreign.roles {
        entry.standard_title = ExternalName::parse("Foreign catalog role").unwrap();
    }
    domain.role_catalogs.insert(0, foreign);
    let bytes = serde_yaml_ng::to_string(&domain).unwrap();
    let descriptor = bundle.manifest.operational_domain.as_mut().unwrap();
    descriptor.source_hash = ContentHash::of(bytes.as_bytes());
    bundle
        .manifest
        .sources
        .insert(descriptor.source.clone(), descriptor.source_hash.clone());
    bundle.operational_domain = Some(bytes);
    let hash = services.fleet.publish_bundle(&bundle).unwrap().bundle_hash;
    activate(&services, &hash);
    let preview = services
        .preview_project_team_definition_selection(project, &request(&domain))
        .unwrap();
    select(
        &services,
        project,
        preview.preview_hash,
        "catalog-order-selection",
    )
    .await
    .unwrap();
    let epic = epic(&services, project, "catalog-order-epic");
    let context = services.runtime_domain(project, Some(epic)).unwrap();
    assert_ne!(
        context.domain.role_catalogs[0].catalog_id,
        expected.catalog_id
    );
    assert_eq!(context.catalog, expected);
    let role = RoleCode::parse("LSA").unwrap();
    let resolved = services.role_in_catalog(&context.catalog, &role).unwrap();
    assert_eq!(resolved.catalog_id, expected.catalog_id);
    assert_eq!(
        resolved.standard_title,
        expected.role(&role).unwrap().standard_title
    );
}
