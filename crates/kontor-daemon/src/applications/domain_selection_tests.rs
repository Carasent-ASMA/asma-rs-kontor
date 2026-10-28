//! Project selection binds exact domain context and replays the original result.

use super::domain_bundle_tests::{resolve, start};
use super::*;
use kontor_core::repository::NewProject;

pub(super) fn project(services: &Services, domain: &OperationalDomainPack) -> ProjectId {
    let id = ProjectId::generate();
    services.state().unwrap().with_store(|store| {
        store
            .create_project(&NewProject {
                id,
                name: ExternalName::parse("Domain fixture").unwrap(),
                root_path: ExternalName::parse(&format!("/example/domain/{id}")).unwrap(),
                created_at: kontor_api::now(),
            })
            .unwrap();
        for topology in &domain.topology_specs {
            store
                .publish_topology_spec(
                    id,
                    topology,
                    &Shareability::default_for(ShareabilityTier::ProjectKnowledge).unwrap(),
                    kontor_api::now(),
                )
                .unwrap();
        }
        for definition in &domain.team_definitions {
            store
                .publish_team_definition(id, definition, kontor_api::now())
                .unwrap();
        }
    });
    id
}
pub(super) fn request(
    domain: &OperationalDomainPack,
) -> ProjectTeamDefinitionSelectionPreviewRequest {
    ProjectTeamDefinitionSelectionPreviewRequest {
        target_definition: TeamDefinitionRefDto {
            id: domain.team_definitions[0].definition_id,
            version: domain.team_definitions[0].version,
        },
    }
}
pub(super) async fn select(
    services: &Services,
    project: ProjectId,
    preview: ContentHash,
    key: &str,
) -> Result<AppliedProjectTeamDefinitionSelectionDto, ApiError> {
    services
        .apply_project_team_definition_selection(
            &IdempotencyKey::parse(key).unwrap(),
            project,
            &ProjectTeamDefinitionSelectionApplyRequest {
                preview_hash: preview,
                expected_revision: services.project_row(project).unwrap().revision,
            },
        )
        .await
}
fn pin(services: &Services, project: ProjectId) -> Option<ProjectTeamDefinitionDefault> {
    services
        .state()
        .unwrap()
        .with_store(|store| store.get_project_team_definition_default(project))
        .unwrap()
}
pub(super) fn activate(services: &Services, hash: &ContentHash) {
    let standing = services.fleet.status().record;
    services
        .fleet
        .activate_bundle(
            hash,
            &crate::fleet::ActivationFence {
                policy_hash: standing.as_ref().map(|r| r.policy_hash.clone()),
                source_bundle_hash: standing.and_then(|r| r.source_bundle_hash),
            },
        )
        .unwrap();
}

#[tokio::test]
async fn domain_selection_requires_activation_and_domain_only_edits_stale_previews() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (mut bundle, hash_a, _) = resolve(&services, &folder);
    let domain = services.purpose_source().unwrap().clone();
    let first = project(&services, &domain);
    let second = project(&services, &domain);
    let request = request(&domain);
    services.fleet.publish_bundle(&bundle).unwrap();
    assert!(
        services
            .preview_project_team_definition_selection(first, &request)
            .unwrap_err()
            .rule
            .contains("operational_domain_unpinned")
    );
    assert!(pin(&services, first).is_none());
    activate(&services, &hash_a);
    let preview_a = services
        .preview_project_team_definition_selection(first, &request)
        .unwrap();
    assert_eq!(
        preview_a.target_operational_domain_bundle_hash,
        Some(hash_a.clone())
    );
    let applied_a = select(
        &services,
        first,
        preview_a.preview_hash.clone(),
        "domain-first",
    )
    .await
    .unwrap();
    let stale = services
        .preview_project_team_definition_selection(second, &request)
        .unwrap();
    let original = pin(&services, first).unwrap();

    let mut standing_preview = stale.preview_hash.clone();
    for (index, edit) in ["prompt", "delivery"].into_iter().enumerate() {
        let mut value: serde_json::Value =
            serde_yaml_ng::from_str(bundle.operational_domain.as_ref().unwrap()).unwrap();
        if edit == "prompt" {
            value["role_prompts"] =
                serde_json::json!([{"role_code":"LSA", "prompt":"Exact selected prompt"}]);
        } else {
            value["delivery"]["task_kind"] = "ASW".into();
        }
        let text = serde_yaml_ng::to_string(&value).unwrap();
        let descriptor = bundle.manifest.operational_domain.as_mut().unwrap();
        descriptor.source_hash = ContentHash::of(text.as_bytes());
        bundle
            .manifest
            .sources
            .insert(descriptor.source.clone(), descriptor.source_hash.clone());
        bundle.operational_domain = Some(text);
        let published = services.fleet.publish_bundle(&bundle).unwrap();
        // Publication and mutable source edits alone do not change the proposal.
        std::fs::write(
            folder.join("packs/operational-domain.yml"),
            "unactivated: []\n",
        )
        .unwrap();
        assert_eq!(
            services
                .preview_project_team_definition_selection(second, &request)
                .unwrap()
                .preview_hash,
            standing_preview
        );
        let before = services
            .preview_project_team_definition_selection(second, &request)
            .unwrap();
        activate(&services, &published.bundle_hash);
        assert!(
            services
                .fleet
                .with_active_domain_bundle(&hash_a, || panic!(
                    "a stale activation must not reach the store effect"
                ))
                .is_err()
        );
        let after = services
            .preview_project_team_definition_selection(second, &request)
            .unwrap();
        assert_ne!(before.preview_hash, after.preview_hash);
        standing_preview = after.preview_hash.clone();
        assert!(
            select(
                &services,
                second,
                before.preview_hash,
                &format!("stale-{index}")
            )
            .await
            .is_err()
        );
        assert!(pin(&services, second).is_none());
        let reread = services
            .fleet
            .operational_domain(original.operational_domain_bundle_hash.as_ref().unwrap())
            .unwrap()
            .1;
        assert!(reread.role_prompts.is_empty());
        assert_eq!(reread.delivery.task_kind, domain.delivery.task_kind);
        assert_eq!(pin(&services, first).unwrap(), original);
    }
    let second_preview = services
        .preview_project_team_definition_selection(second, &request)
        .unwrap();
    select(
        &services,
        second,
        second_preview.preview_hash,
        "second-project",
    )
    .await
    .unwrap();
    assert_ne!(
        pin(&services, first)
            .unwrap()
            .operational_domain_bundle_hash,
        pin(&services, second)
            .unwrap()
            .operational_domain_bundle_hash
    );
    let preview_b = services
        .preview_project_team_definition_selection(first, &request)
        .unwrap();
    assert_eq!(
        preview_b.current_definition,
        Some(preview_b.target_definition.clone())
    );
    let applied_b = select(&services, first, preview_b.preview_hash, "domain-second")
        .await
        .unwrap();
    assert_ne!(
        applied_a.operational_domain_bundle_hash,
        applied_b.operational_domain_bundle_hash
    );
    let replay = select(
        &services,
        first,
        preview_a.preview_hash.clone(),
        "domain-first",
    )
    .await
    .unwrap();
    assert_eq!(replay.operational_domain_bundle_hash, Some(hash_a));
    assert_eq!(replay.receipt.receipt_id, applied_a.receipt.receipt_id);
    assert_eq!(
        pin(&services, first)
            .unwrap()
            .operational_domain_bundle_hash,
        applied_b.operational_domain_bundle_hash
    );
    assert!(
        services
            .preview_project_team_definition_selection(ProjectId::generate(), &request)
            .is_err()
    );
    let before_restart = pin(&services, first).unwrap();
    std::fs::write(
        folder.join("packs/operational-domain.yml"),
        format!(
            "# unactivated edit\n{}",
            bundle.operational_domain.as_ref().unwrap()
        ),
    )
    .unwrap();
    drop(services);
    drop(daemon);
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    assert_eq!(pin(&services, first).unwrap(), before_restart);
    let replay = select(&services, first, preview_a.preview_hash, "domain-first")
        .await
        .unwrap();
    assert_eq!(
        replay.operational_domain_bundle_hash,
        applied_a.operational_domain_bundle_hash
    );
}

#[tokio::test]
async fn domain_selection_rolls_back_with_receipt_failure_and_refuses_unrelated_definition() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (bundle, hash, _) = resolve(&services, &folder);
    let domain = services.purpose_source().unwrap().clone();
    let project = project(&services, &domain);
    services.fleet.publish_bundle(&bundle).unwrap();
    activate(&services, &hash);
    let request = request(&domain);
    let preview = services
        .preview_project_team_definition_selection(project, &request)
        .unwrap();
    let connection = rusqlite::Connection::open(root.join(crate::DATABASE_FILE)).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_selection_result BEFORE INSERT ON local_command_results BEGIN SELECT RAISE(ABORT, 'injected result failure'); END;").unwrap();
    assert!(
        select(
            &services,
            project,
            preview.preview_hash.clone(),
            "atomic-selection"
        )
        .await
        .is_err()
    );
    assert!(pin(&services, project).is_none());
    assert!(
        services
            .state()
            .unwrap()
            .with_store(|store| store
                .get_receipt_by_key(&IdempotencyKey::parse("atomic-selection").unwrap()))
            .unwrap()
            .is_none()
    );
    connection
        .execute_batch("DROP TRIGGER reject_selection_result;")
        .unwrap();
    select(&services, project, preview.preview_hash, "atomic-selection")
        .await
        .unwrap();
    let mut foreign = domain.team_definitions[0].clone();
    foreign.definition_id = TeamDefinitionId::generate();
    services
        .state()
        .unwrap()
        .with_store(|store| store.publish_team_definition(project, &foreign, kontor_api::now()))
        .unwrap();
    let error = services
        .preview_project_team_definition_selection(
            project,
            &ProjectTeamDefinitionSelectionPreviewRequest {
                target_definition: TeamDefinitionRefDto {
                    id: foreign.definition_id,
                    version: foreign.version,
                },
            },
        )
        .unwrap_err();
    assert!(
        error
            .rule
            .contains("operational_domain_definition_mismatch")
    );
    let artifact = kontor_fleet_activation::domain::domain_path(
        &root,
        &bundle.manifest.operational_domain.unwrap().source_hash,
    );
    std::fs::remove_file(artifact).unwrap();
    assert!(
        services
            .preview_project_team_definition_selection(project, &request)
            .is_err()
    );
}

#[tokio::test]
async fn selection_binds_frozen_roster_and_atomic_store_refuses_a_roster_race() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (bundle, hash, _) = resolve(&services, &folder);
    let domain = services.purpose_source().unwrap().clone();
    let project = project(&services, &domain);
    services.fleet.publish_bundle(&bundle).unwrap();
    activate(&services, &hash);
    let publish = |revision: &CoreTeamRevision| {
        services
            .state()
            .unwrap()
            .with_store(|store| {
                store.publish_core_team_revision(
                    &StoredCoreTeamRevision {
                        project_id: project,
                        version: revision.version,
                        catalog_hash: revision.catalog_hash.clone(),
                        seats: serde_json::to_value(&revision.seats).unwrap(),
                        published_at: kontor_api::now(),
                    },
                    &domain.role_catalogs[0],
                )
            })
            .unwrap();
    };
    publish(&bundle.revision);
    let request = request(&domain);
    let prior = services
        .project_team_definition_selection(project, &request.target_definition)
        .unwrap();
    let mut moved = bundle.revision.clone();
    moved.version = SpecVersion::parse(2).unwrap();
    publish(&moved);
    let preview = services
        .preview_project_team_definition_selection(project, &request)
        .unwrap();
    assert_ne!(preview.preview_hash, prior.preview_hash);
    assert!(
        select(
            &services,
            project,
            prior.preview_hash.clone(),
            "stale-roster"
        )
        .await
        .is_err()
    );
    let error = services.state().unwrap().with_store(|store| store.select_project_team_definition(
        &prior.target, None, prior.roster_hash.as_ref(),
        &ReceiptEnvelope::new(services.realm_id, NewLocalCommand {
            project_id: project, receipt_id: CommandReceiptId::generate(), idempotency_key: IdempotencyKey::parse("racing-roster").unwrap(),
            kind: CommandKind::SelectProjectTeamDefinition, target: AggregateRef::Project { project_id: project },
            target_revision: AggregateRevision::INITIAL,
            intent: CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"operation":"project_team_definition_selection_apply","project":project,"preview":prior.preview_hash})).unwrap(),
            created_at: kontor_api::now(),
        }),
    )).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("operational_domain_roster_moved")
    );
    assert!(pin(&services, project).is_none());
    select(&services, project, preview.preview_hash, "current-roster")
        .await
        .unwrap();
    let mut legacy = moved;
    for seat in &mut legacy.seats {
        seat.purposes.clear();
    }
    assert!(
        kontor_fleet_activation::domain::validate_roster(
            &domain,
            &bundle.manifest.role_catalog,
            &legacy
        )
        .unwrap_err()
        .to_string()
        .contains("role_purpose_unpinned")
    );
}
