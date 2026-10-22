//! Creation context is frozen with the graph, receipt and existing roster pins.

use super::domain_bundle_tests::{resolve, start};
use super::domain_selection_tests::{activate, project, request, select};
use super::*;
use kontor_store::{EpicApplyCommand, EpicDomainBinding};

pub(super) async fn selected(
    services: &Services,
    folder: &std::path::Path,
) -> (ProjectId, crate::orchestration::ResolvedBundle, ContentHash) {
    let (bundle, hash, _) = resolve(services, folder);
    let domain = services.purpose_source().unwrap().clone();
    let project = project(services, &domain);
    services.fleet.publish_bundle(&bundle).unwrap();
    activate(services, &hash);
    services
        .state()
        .unwrap()
        .with_store(|store| {
            store.publish_core_team_revision(
                &StoredCoreTeamRevision {
                    project_id: project,
                    version: bundle.revision.version,
                    catalog_hash: bundle.revision.catalog_hash.clone(),
                    seats: serde_json::to_value(&bundle.revision.seats).unwrap(),
                    published_at: kontor_api::now(),
                },
                &domain.role_catalogs[0],
            )
        })
        .unwrap();
    let preview = services
        .preview_project_team_definition_selection(project, &request(&domain))
        .unwrap();
    select(
        services,
        project,
        preview.preview_hash,
        &format!("select-{project}"),
    )
    .await
    .unwrap();
    (project, bundle, hash)
}

pub(super) async fn select_existing(
    services: &Services,
    folder: &std::path::Path,
    project: ProjectId,
) {
    let (bundle, hash, _) = resolve(services, folder);
    let domain = services.purpose_source().unwrap().clone();
    services.fleet.publish_bundle(&bundle).unwrap();
    activate(services, &hash);
    services.state().unwrap().with_store(|store| {
        for topology in &domain.topology_specs {
            store
                .publish_topology_spec(
                    project,
                    topology,
                    &Shareability::default_for(ShareabilityTier::ProjectKnowledge).unwrap(),
                    kontor_api::now(),
                )
                .unwrap();
        }
        for definition in &domain.team_definitions {
            store
                .publish_team_definition(project, definition, kontor_api::now())
                .unwrap();
        }
        if store.get_current_core_team(project).unwrap().is_none() {
            store
                .publish_core_team_revision(
                    &StoredCoreTeamRevision {
                        project_id: project,
                        version: bundle.revision.version,
                        catalog_hash: bundle.revision.catalog_hash.clone(),
                        seats: serde_json::to_value(&bundle.revision.seats).unwrap(),
                        published_at: kontor_api::now(),
                    },
                    &domain.role_catalogs[0],
                )
                .unwrap();
        }
    });
    let preview = services
        .preview_project_team_definition_selection(project, &request(&domain))
        .unwrap();
    select(
        services,
        project,
        preview.preview_hash,
        &format!("select-{project}"),
    )
    .await
    .unwrap();
}

pub(super) fn command(
    services: &Services,
    project: ProjectId,
    binding: &EpicDomainBinding,
    key: &str,
) -> ReceiptEnvelope<EpicApplyCommand> {
    let revision = services.project_row(project).unwrap().revision;
    ReceiptEnvelope::new(
        services.state().unwrap().realm_id(),
        EpicApplyCommand {
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            expected_revision: revision,
            intent: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version":1, "operation":"epics_apply", "project_id":project,
                "expected_project_revision":revision, "domain_binding":binding,
            }))
            .unwrap(),
        },
    )
}

#[tokio::test]
async fn epic_domain_target_hash_and_legacy_refusals_precede_effects() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let daemon = start(&directory.path().join("state"), &folder);
    let services = daemon.jira_reconciler();
    let (project_id, _, hash) = selected(&services, &folder).await;
    let name = ExternalName::parse("Domain epic").unwrap();
    let binding = services
        .epic_domain_binding(project_id, &name, Some(&hash))
        .unwrap()
        .0;
    let moved = ContentHash::of(b"another selected domain");
    assert!(
        services
            .epic_domain_binding(project_id, &name, Some(&moved))
            .unwrap_err()
            .rule
            .contains("operational_domain_epic_target_moved")
    );
    assert_eq!(binding.operational_domain_bundle_hash, hash);
    let legacy = project(&services, services.purpose_source().unwrap());
    assert!(
        services
            .epic_domain_binding(legacy, &name, None)
            .unwrap_err()
            .rule
            .contains("operational_domain_unpinned")
    );
    let legacy_epic = MiniProjectId::generate();
    services.state().unwrap().with_store(|store| {
        store
            .create_mini_project(&NewMiniProject {
                id: legacy_epic,
                project_id: legacy,
                name: name.clone(),
                created_at: kontor_api::now(),
            })
            .unwrap();
        store
            .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
                project_id: legacy,
                mini_project_id: legacy_epic,
                definition: binding.definition.clone(),
                operational_domain_bundle_hash: None,
                pinned_at: kontor_api::now(),
            })
            .unwrap();
    });
    assert!(
        services
            .epic_domain_binding(legacy, &name, None)
            .unwrap_err()
            .rule
            .contains("operational_domain_unpinned")
    );
    assert!(
        services
            .state()
            .unwrap()
            .with_store(|store| store.list_mini_projects(project_id))
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn epic_domain_creation_rolls_back_every_pin_receipt_and_roster_then_reopens_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, _, hash) = selected(&services, &folder).await;
    let name = ExternalName::parse("Atomic domain epic").unwrap();
    let (binding, selection) = services
        .epic_domain_binding(project, &name, Some(&hash))
        .unwrap();
    let now = kontor_api::now();
    let (profile, source) = services.bundle_with_team_source("code", now).unwrap();
    let application = EpicApplication {
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
    };
    let envelope = command(&services, project, &binding, "atomic-domain-epic");
    let connection = rusqlite::Connection::open(root.join(crate::DATABASE_FILE)).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_epic_result BEFORE INSERT ON local_command_results BEGIN SELECT RAISE(ABORT, 'fixture epic result failure'); END;").unwrap();
    let state = services.state().unwrap();
    let failure = state
        .with_store(|store| {
            store.apply_epic_with_domain(&application, &binding, selection.as_ref(), &envelope)
        })
        .unwrap_err();
    assert!(
        matches!(
            failure,
            RepositoryError::Conflict {
                subject: "storage",
                ..
            }
        ),
        "{failure:?}"
    );
    for table in [
        "mini_projects",
        "mini_project_team_definition_snapshots",
        "mini_project_topology_snapshots",
        "epic_rosters",
    ] {
        assert_eq!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE project_id = ?1"),
                    [project.to_string()],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0,
            "{table}"
        );
    }
    assert!(
        state
            .with_store(|store| store
                .get_receipt_by_key(&IdempotencyKey::parse("atomic-domain-epic").unwrap()))
            .unwrap()
            .is_none()
    );
    connection
        .execute_batch("DROP TRIGGER fail_epic_result;")
        .unwrap();
    let preview = state
        .with_store(|store| {
            store.preview_epic_with_domain(&application, &binding, selection.as_ref())
        })
        .unwrap();
    assert_eq!(preview.applied, Applied::Created);
    assert!(
        state
            .with_store(|store| store.list_mini_projects(project))
            .unwrap()
            .is_empty()
    );
    let applied = state
        .with_store(|store| {
            store.apply_epic_with_domain(&application, &binding, selection.as_ref(), &envelope)
        })
        .unwrap();
    let applied = applied.unwrap();
    let pin = state
        .with_store(|store| {
            store.get_mini_project_team_definition(project, applied.mini_project_id)
        })
        .unwrap()
        .unwrap();
    assert_eq!(pin.operational_domain_bundle_hash, Some(hash.clone()));
    assert_eq!(pin.definition, binding.definition);
    let receipt = state
        .with_store(|store| {
            store.get_receipt_by_key(&IdempotencyKey::parse("atomic-domain-epic").unwrap())
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        state
            .with_store(|store| store.epic_domain_creation_result(&receipt))
            .unwrap(),
        binding
    );
    assert_eq!(
        receipt.state,
        kontor_core::receipt::CommandReceiptState::IntentPersisted
    );
    let roster = state
        .with_store(|store| store.get_epic_roster(project, applied.mini_project_id))
        .unwrap()
        .unwrap();
    assert_eq!(
        roster.seats,
        serde_json::to_value(&binding.roster.seats).unwrap()
    );
    let reopened = SqliteStore::open(&root.join(crate::DATABASE_FILE)).unwrap();
    assert_eq!(
        reopened
            .get_mini_project_team_definition(project, applied.mini_project_id)
            .unwrap(),
        Some(pin)
    );
    assert_eq!(
        reopened.epic_domain_creation_result(&receipt).unwrap(),
        binding
    );
    // Generic product backup coverage uses only this disposable fixture.
    let copy = kontor_store::backup::create_snapshot(
        &root.join(crate::DATABASE_FILE),
        &directory.path().join("product-backup-test"),
        now,
    )
    .unwrap();
    let restored_path = directory.path().join("restored-test.db");
    kontor_store::backup::restore_snapshot(&copy.snapshot, &restored_path, now).unwrap();
    let restored = SqliteStore::open(&restored_path).unwrap();
    assert_eq!(
        restored.epic_domain_creation_result(&receipt).unwrap(),
        binding
    );
    assert_eq!(
        restored
            .get_epic_roster(project, applied.mini_project_id)
            .unwrap(),
        Some(roster)
    );

    let mut concurrent = binding.clone();
    concurrent.operational_domain_bundle_hash = ContentHash::of(b"concurrent newer selection");
    let retry = command(&services, project, &concurrent, "atomic-domain-epic");
    assert!(
        state
            .with_store(|store| store.apply_epic_with_domain(
                &application,
                &concurrent,
                None,
                &retry
            ))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        state
            .with_store(|store| store.list_mini_projects(project))
            .unwrap()
            .len(),
        1
    );

    // A changed intent cannot claim the already-created target and no second graph survives.
    let mut wrong = binding.clone();
    wrong.operational_domain_bundle_hash = ContentHash::of(b"wrong domain");
    assert!(
        state
            .with_store(|store| store.apply_epic_with_domain(
                &application,
                &wrong,
                selection.as_ref(),
                &envelope
            ))
            .unwrap_err()
            .to_string()
            .contains("creation_intent_mismatch")
    );
}

#[tokio::test]
async fn epic_domain_selection_race_refuses_and_old_epic_keeps_its_pin() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, mut bundle, hash_a) = selected(&services, &folder).await;
    let name = ExternalName::parse("Pinned before selection change").unwrap();
    let (binding, old_selection) = services
        .epic_domain_binding(project, &name, Some(&hash_a))
        .unwrap();
    let now = kontor_api::now();
    let (profile, source) = services.bundle_with_team_source("code", now).unwrap();
    let mut application = EpicApplication {
        project_id: project,
        name: name.clone(),
        epic_backlog_code: None,
        execution_scope: None,
        tasks: &[],
        profile: &profile.profile,
        definition: &profile.profile.definition,
        team: profile.team.as_ref(),
        team_source: source,
        applied_at: now,
    };
    let state = services.state().unwrap();
    let original_command = command(&services, project, &binding, "old-domain-epic");
    state
        .with_store(|store| {
            store.apply_epic_with_domain(
                &application,
                &binding,
                old_selection.as_ref(),
                &original_command,
            )
        })
        .unwrap();
    // Format-only changes still have distinct source/bundle identity.
    bundle
        .operational_domain
        .as_mut()
        .unwrap()
        .push_str("\n# next exact domain bytes\n");
    let descriptor = bundle.manifest.operational_domain.as_mut().unwrap();
    descriptor.source_hash =
        ContentHash::of(bundle.operational_domain.as_ref().unwrap().as_bytes());
    bundle
        .manifest
        .sources
        .insert(descriptor.source.clone(), descriptor.source_hash.clone());
    let next = services.fleet.publish_bundle(&bundle).unwrap().bundle_hash;
    activate(&services, &next);
    assert_eq!(
        services
            .epic_domain_binding(
                project,
                &ExternalName::parse("new before selection").unwrap(),
                None
            )
            .unwrap()
            .0
            .operational_domain_bundle_hash,
        hash_a
    );
    let preview = services
        .preview_project_team_definition_selection(
            project,
            &request(services.purpose_source().unwrap()),
        )
        .unwrap();
    select(
        &services,
        project,
        preview.preview_hash,
        "move-domain-default",
    )
    .await
    .unwrap();
    application.name = ExternalName::parse("stale prepared creation").unwrap();
    let envelope = command(&services, project, &binding, "stale-creation");
    assert!(
        state
            .with_store(|store| store.apply_epic_with_domain(
                &application,
                &binding,
                old_selection.as_ref(),
                &envelope
            ))
            .unwrap_err()
            .to_string()
            .contains("operational_domain_selection_moved")
    );
    assert_eq!(
        state
            .with_store(|store| store.list_mini_projects(project))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        services
            .epic_domain_binding(project, &name, Some(&hash_a))
            .unwrap()
            .0,
        binding
    );
    assert!(
        services
            .epic_domain_binding(project, &application.name, Some(&hash_a))
            .unwrap_err()
            .rule
            .contains("operational_domain_epic_target_moved")
    );
    assert_eq!(
        services
            .epic_domain_binding(project, &application.name, Some(&next))
            .unwrap()
            .0
            .operational_domain_bundle_hash,
        next
    );
}

#[tokio::test]
async fn epic_creation_refuses_a_changed_roster_and_missing_published_domain() {
    let directory = tempfile::tempdir().unwrap();
    let folder = config_fixture::write_config_folder(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, bundle, hash) = selected(&services, &folder).await;
    let name = ExternalName::parse("Roster race").unwrap();
    let (binding, selection) = services
        .epic_domain_binding(project, &name, Some(&hash))
        .unwrap();
    let now = kontor_api::now();
    let (profile, source) = services.bundle_with_team_source("code", now).unwrap();
    let application = EpicApplication {
        project_id: project,
        name: name.clone(),
        epic_backlog_code: None,
        execution_scope: None,
        tasks: &[],
        profile: &profile.profile,
        definition: &profile.profile.definition,
        team: profile.team.as_ref(),
        team_source: source,
        applied_at: now,
    };
    let envelope = command(&services, project, &binding, "roster-race");
    let state = services.state().unwrap();
    state
        .with_store(|store| {
            store.publish_core_team_revision(
                &StoredCoreTeamRevision {
                    project_id: project,
                    version: SpecVersion::parse(2).unwrap(),
                    catalog_hash: binding.roster.catalog_hash.clone(),
                    seats: serde_json::to_value(&binding.roster.seats).unwrap(),
                    published_at: now,
                },
                &services.purpose_source().unwrap().role_catalogs[0],
            )
        })
        .unwrap();
    assert!(
        state
            .with_store(|store| store.apply_epic_with_domain(
                &application,
                &binding,
                selection.as_ref(),
                &envelope
            ))
            .unwrap_err()
            .to_string()
            .contains("operational_domain_roster_moved")
    );
    assert!(
        state
            .with_store(|store| store.list_mini_projects(project))
            .unwrap()
            .is_empty()
    );
    let moved = services
        .epic_domain_binding(project, &name, Some(&hash))
        .unwrap()
        .0;
    assert_ne!(
        command(&services, project, &moved, "roster-race")
            .peek(state.realm_id())
            .unwrap()
            .intent
            .hash(),
        envelope.peek(state.realm_id()).unwrap().intent.hash()
    );
    let artifact = kontor_fleet_activation::domain::domain_path(
        &root,
        &bundle.manifest.operational_domain.unwrap().source_hash,
    );
    std::fs::remove_file(artifact).unwrap();
    assert!(
        services
            .epic_domain_binding(project, &name, Some(&hash))
            .is_err()
    );
    assert!(
        state
            .with_store(|store| store.list_mini_projects(project))
            .unwrap()
            .is_empty()
    );
}
