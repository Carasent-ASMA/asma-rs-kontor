//! Frozen purpose authority, selected sources and the existing explicit upgrade path.
use super::config_fixture as config;
use super::*;
use kontor_core::repository::{NewMiniProject, NewProject};
use std::path::{Path, PathBuf};

fn read(path: &Path) -> serde_json::Value {
    serde_yaml_ng::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_yaml_ng::to_string(value).unwrap()).unwrap();
}
fn start(root: &Path, folder: &Path) -> crate::Daemon {
    crate::Daemon::start(
        crate::DaemonConfig::at(root)
            .with_config_dir(folder)
            .with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap()
}
fn source(root: &Path) -> PathBuf {
    config::write_config_folder(root)
}
fn selections(services: &Services) -> Vec<CoreTeamSeatSelectionDto> {
    let catalog = services.published_catalog().unwrap();
    ["LSA", "TPM", "SA", "QA"]
        .into_iter()
        .map(|code| CoreTeamSeatSelectionDto {
            role: kontor_api::applications::RoleSelectionDto {
                catalog_revision: RevisionRefDto {
                    id: catalog.catalog_id.to_string(),
                    version: catalog.version,
                },
                role_code: RoleCode::parse(code).unwrap(),
                custom_display_name: Some(
                    ExternalName::parse("control architecture_closer process_closer").unwrap(),
                ),
            },
            presence: EpicPresence::Required,
            ad_hoc_allowed: true,
        })
        .collect()
}
async fn publish(
    services: &Services,
    project: ProjectId,
    seats: &[CoreTeamSeatSelectionDto],
    key: &str,
) -> CoreTeamRevision {
    let preview = services
        .preview_core_team(
            project,
            &CoreTeamPreviewRequest {
                seats: Some(seats.to_vec()),
                source_bundle_hash: None,
            },
        )
        .unwrap();
    let current = services.stored_core_team(project).unwrap();
    services
        .apply_core_team(
            &IdempotencyKey::parse(key).unwrap(),
            project,
            &CoreTeamApplyRequest {
                seats: Some(seats.to_vec()),
                source_bundle_hash: None,
                preview_hash: preview.preview_hash,
                expected_revision: core_team_revision_of(current.as_ref()),
            },
        )
        .await
        .unwrap();
    services.stored_core_team(project).unwrap().unwrap()
}
fn business(daemon: &crate::Daemon) -> (ProjectId, MiniProjectId) {
    let project = ProjectId::generate();
    let epic = MiniProjectId::generate();
    daemon.state().with_store(|store| {
        store
            .create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Purpose fixture").unwrap(),
                root_path: ExternalName::parse("/example/project").unwrap(),
                created_at: kontor_api::now(),
            })
            .unwrap();
        store
            .create_mini_project(&NewMiniProject {
                id: epic,
                project_id: project,
                name: ExternalName::parse("Purpose epic").unwrap(),
                created_at: kontor_api::now(),
            })
            .unwrap();
    });
    (project, epic)
}

#[tokio::test]
async fn domain_unpinned_legacy_refuses_roster_materialization_without_changing_frozen_seats() {
    use CoreTeamSeatPurpose::*;
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let state_root = directory.path().join("state");
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    let (project, epic) = business(&daemon);
    let seats = selections(&services);
    let tagged = publish(&services, project, &seats, "initial-purpose-publication").await;
    assert_eq!(
        tagged.seat_for_purpose(ProcessCloser).unwrap().purposes,
        [ProcessCloser, Control]
    );
    let mut legacy = tagged.clone();
    for seat in &mut legacy.seats {
        seat.purposes.clear();
    }
    let legacy = FrozenRoster {
        revision: legacy,
        revision_of_epic: AggregateRevision::INITIAL,
        quick_session_id: None,
    };
    services
        .freeze_roster(project, epic, &legacy, None, kontor_api::now())
        .unwrap();
    assert!(
        services
            .core_team_seat_dtos(&legacy.revision)
            .unwrap()
            .iter()
            .all(|seat| seat.purposes.is_empty())
    );
    for purpose in CoreTeamSeatPurpose::ALL {
        assert!(
            services
                .epic_purpose(project, epic, *purpose)
                .unwrap_err()
                .rule
                .contains("role_purpose_unpinned")
        );
    }
    let stale_preview = services
        .preview_core_team(
            project,
            &CoreTeamPreviewRequest {
                seats: Some(seats.clone()),
                source_bundle_hash: None,
            },
        )
        .unwrap();
    let expected = core_team_revision_of(Some(&tagged));
    drop(services);
    drop(daemon);
    let path = folder.join("packs/operational-domain.yml");
    let mut domain = read(&path);
    domain["delivery"]["architecture_closer_code"] = "SA".into();
    domain["delivery"]["process_closer_code"] = "QA".into();
    domain["delivery"]["control_role_code"] = "QA".into();
    write(&path, &domain);
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    assert!(
        services
            .apply_core_team(
                &IdempotencyKey::parse("stale-purpose-source").unwrap(),
                project,
                &CoreTeamApplyRequest {
                    seats: Some(seats.clone()),
                    source_bundle_hash: None,
                    preview_hash: stale_preview.preview_hash,
                    expected_revision: expected,
                }
            )
            .await
            .is_err()
    );
    assert_eq!(services.stored_core_team(project).unwrap().unwrap(), tagged);
    assert!(
        daemon
            .state()
            .with_store(|store| store
                .get_receipt_by_key(&IdempotencyKey::parse("stale-purpose-source").unwrap())
                .unwrap())
            .is_none(),
        "stale purpose previews consume no receipt"
    );
    let next = publish(&services, project, &seats, "new-purpose-revision").await;
    assert_eq!(
        next.seat_for_purpose(ArchitectureCloser)
            .unwrap()
            .role
            .role_code
            .as_str(),
        "SA"
    );
    assert_eq!(
        next.seat_for_purpose(ProcessCloser)
            .unwrap()
            .role
            .role_code
            .as_str(),
        "QA"
    );
    assert!(
        services
            .epic_purpose(project, epic, Control)
            .unwrap_err()
            .rule
            .contains("role_purpose_unpinned"),
        "publication alone cannot tag an existing epic"
    );
    let preview = services
        .preview_roster_upgrade(
            project,
            epic,
            &RosterUpgradePreviewRequest {
                target: RevisionRefDto {
                    id: "core-team".to_owned(),
                    version: next.version,
                },
            },
        )
        .unwrap();
    let current = services.frozen_roster(project, epic).unwrap();
    let refused = services
        .apply_roster_upgrade(
            &IdempotencyKey::parse("explicit-purpose-upgrade").unwrap(),
            project,
            epic,
            &TopologyUpgradeApplyRequest {
                preview_hash: preview.preview_hash,
                expected_revision: current.revision_of_epic,
            },
        )
        .await
        .unwrap_err();
    assert!(refused.rule.contains("operational_domain_unpinned"));
    assert_eq!(
        services.frozen_roster(project, epic).unwrap().revision,
        legacy.revision
    );
    assert!(
        daemon
            .state()
            .with_store(|store| store.list_topology_nodes(project, Some(epic)))
            .unwrap()
            .is_empty()
    );
    assert!(
        daemon
            .state()
            .with_store(|store| store
                .get_receipt_by_key(&IdempotencyKey::parse("explicit-purpose-upgrade").unwrap()))
            .unwrap()
            .is_none()
    );
    drop(services);
    drop(daemon);
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    assert_eq!(
        services.frozen_roster(project, epic).unwrap().revision,
        legacy.revision
    );
    for purpose in CoreTeamSeatPurpose::ALL {
        assert!(
            services
                .epic_purpose(project, epic, *purpose)
                .unwrap_err()
                .rule
                .contains("role_purpose_unpinned")
        );
    }
}

#[tokio::test]
async fn explicit_roster_upgrade_on_a_domain_pinned_epic_enables_only_its_frozen_purposes() {
    use CoreTeamSeatPurpose::*;
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let root = directory.path().join("state");
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    let (project, _) = business(&daemon);
    let tagged = publish(
        &services,
        project,
        &selections(&services),
        "pinned-purpose-publication",
    )
    .await;
    super::epic_domain_tests::select_existing(&services, &folder, project).await;
    let epic = super::domain_runtime_tests::epic(&services, project, "purpose-upgrade");
    // Historical roster metadata, with the independently frozen domain retained.
    let mut legacy = services.frozen_roster(project, epic).unwrap();
    for seat in &mut legacy.revision.seats {
        seat.purposes.clear();
    }
    services
        .freeze_roster(project, epic, &legacy, None, kontor_api::now())
        .unwrap();
    assert!(
        services
            .epic_purpose(project, epic, Control)
            .unwrap_err()
            .rule
            .contains("role_purpose_unpinned")
    );
    let preview = services
        .preview_roster_upgrade(
            project,
            epic,
            &RosterUpgradePreviewRequest {
                target: RevisionRefDto {
                    id: "core-team".into(),
                    version: tagged.version,
                },
            },
        )
        .unwrap();
    let before = services.frozen_roster(project, epic).unwrap();
    services
        .apply_roster_upgrade(
            &IdempotencyKey::parse("pin-purpose-upgrade").unwrap(),
            project,
            epic,
            &TopologyUpgradeApplyRequest {
                preview_hash: preview.preview_hash,
                expected_revision: before.revision_of_epic,
            },
        )
        .await
        .unwrap();
    let architecture = services
        .epic_control_seat(project, epic, ArchitectureCloser)
        .unwrap();
    let process = services
        .epic_control_seat(project, epic, ProcessCloser)
        .unwrap();
    assert_eq!(
        services.epic_control_seat(project, epic, Control).unwrap(),
        process
    );
    assert_ne!(architecture, process);
    let frozen = services.frozen_roster(project, epic).unwrap();
    drop(services);
    drop(daemon);
    let path = folder.join("packs/operational-domain.yml");
    let mut domain = read(&path);
    domain["delivery"]["architecture_closer_code"] = "SA".into();
    domain["delivery"]["process_closer_code"] = "QA".into();
    domain["delivery"]["control_role_code"] = "QA".into();
    write(&path, &domain);
    let daemon = start(&root, &folder);
    let services = daemon.jira_reconciler();
    assert_eq!(
        services.frozen_roster(project, epic).unwrap().revision,
        frozen.revision
    );
    assert_eq!(
        services
            .epic_control_seat(project, epic, ArchitectureCloser)
            .unwrap(),
        architecture
    );
    assert_eq!(
        services.epic_control_seat(project, epic, Control).unwrap(),
        process
    );
    let scope = services
        .resolve_scope(
            project,
            &SemanticTopologyTargetDto::EpicControl { epic_id: epic },
        )
        .unwrap();
    let control = services
        .scope_nodes(project, &scope)
        .unwrap()
        .into_iter()
        .find(|node| Some(&node.kind) == scope.kind.as_ref())
        .unwrap();
    let wrong = daemon
        .state()
        .with_store(|store| store.list_seat_bindings(project, control.id))
        .unwrap()
        .into_iter()
        .find(|seat| seat.role.role_code.as_str() == "SA")
        .unwrap();
    assert_eq!(
        services
            .require_completion_authority(project, epic, ArchitectureCloser, wrong.id, 1)
            .unwrap_err()
            .code,
        ApiErrorCode::Forbidden
    );
    assert!(
        legacy
            .revision
            .seats
            .iter()
            .all(|seat| seat.purposes.is_empty())
    );
}

#[tokio::test]
async fn control_slot_requires_exact_frozen_role_for_reuse_and_release() {
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let daemon = start(&directory.path().join("state"), &folder);
    let services = daemon.jira_reconciler();
    let (project, _) = business(&daemon);
    let tagged = publish(
        &services,
        project,
        &selections(&services),
        "control-binding-publication",
    )
    .await;
    super::epic_domain_tests::select_existing(&services, &folder, project).await;
    let epic = super::domain_runtime_tests::epic(&services, project, "exact-control-binding");
    let owner = tagged
        .seat_for_purpose(CoreTeamSeatPurpose::Control)
        .unwrap()
        .clone();
    let wrong_role = tagged
        .seat_for_purpose(CoreTeamSeatPurpose::ArchitectureCloser)
        .unwrap()
        .role
        .clone();
    services
        .freeze_roster(
            project,
            epic,
            &FrozenRoster {
                revision: tagged,
                revision_of_epic: AggregateRevision::INITIAL,
                quick_session_id: None,
            },
            None,
            kontor_api::now(),
        )
        .unwrap();
    let scope = services
        .resolve_scope(
            project,
            &SemanticTopologyTargetDto::EpicControl { epic_id: epic },
        )
        .unwrap();
    let control = services.ensure_scope_chain(project, &scope).unwrap();
    let now = kontor_api::now();
    let wrong = daemon
        .state()
        .with_store(|store| {
            store.create_seat_binding(&NewSeatBinding {
                id: SeatBindingId::generate(),
                project_id: project,
                topology_node_id: control.id,
                role_slot_id: owner.role_slot_id,
                role: wrong_role,
                task_id: None,
                team_run_id: None,
                attach_deadline: now,
                parent_seat_binding_id: None,
                created_at: now,
            })
        })
        .unwrap();
    for error in [
        services
            .ensure_epic_control_seat(project, &control)
            .unwrap_err(),
        services
            .release_epic_control_seat(project, epic)
            .unwrap_err(),
    ] {
        assert_eq!(error.code, ApiErrorCode::RoleSlotUnbound);
        assert!(error.rule.contains("role_slot_binding_mismatch"));
    }
    let unchanged = daemon
        .state()
        .with_store(|store| store.list_seat_bindings(project, control.id))
        .unwrap();
    assert_eq!(unchanged, vec![wrong.clone()]);
    assert!(unchanged[0].is_non_terminal());

    // Explicitly retire the conflicting test binding, then exercise the exact owner.
    daemon
        .state()
        .with_store(|store| {
            store.observe_seat_binding(
                project,
                wrong.id,
                &SeatLivenessObservation {
                    released_at: Some(now),
                    ..SeatLivenessObservation::default()
                },
                now,
            )
        })
        .unwrap();
    let correct = services
        .ensure_epic_control_seat(project, &control)
        .unwrap()
        .unwrap();
    assert_eq!(
        services
            .ensure_epic_control_seat(project, &control)
            .unwrap(),
        Some(correct)
    );
    services.release_epic_control_seat(project, epic).unwrap();
    let released = daemon
        .state()
        .with_store(|store| store.list_seat_bindings(project, control.id))
        .unwrap();
    assert_eq!(released.len(), 2);
    assert!(released.iter().all(|seat| !seat.is_non_terminal()));
}

#[test]
fn purpose_only_changes_bind_core_team_and_both_sides_of_upgrade_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let daemon = start(&directory.path().join("state"), &folder);
    let services = daemon.jira_reconciler();
    let domain = services.purpose_source().unwrap();
    let catalog = &domain.role_catalogs[0];
    let selected: Vec<_> = selections(&services)
        .into_iter()
        .map(|seat| CoreTeamSeatSelection {
            role_code: seat.role.role_code,
            custom_display_name: seat.role.custom_display_name,
            presence: seat.presence,
            ad_hoc_allowed: seat.ad_hoc_allowed,
        })
        .collect();
    let first = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        catalog,
        &selected,
        domain.delivery.purpose_roles(),
    )
    .unwrap();
    let mut edited = domain.delivery.clone();
    edited.control_role_code = RoleCode::parse("SA").unwrap();
    let next = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        catalog,
        &selected,
        edited.purpose_roles(),
    )
    .unwrap();
    let project = ProjectId::generate();
    let epic = MiniProjectId::generate();
    assert_ne!(
        services.core_team_hash(project, &first, &[], None).unwrap(),
        services.core_team_hash(project, &next, &[], None).unwrap()
    );
    let before = FrozenRoster {
        revision: first.clone(),
        revision_of_epic: AggregateRevision::INITIAL,
        quick_session_id: None,
    };
    let after = FrozenRoster {
        revision: next.clone(),
        revision_of_epic: AggregateRevision::INITIAL,
        quick_session_id: None,
    };
    let hash = services
        .roster_upgrade_hash(project, epic, &before, false, &first, &[])
        .unwrap();
    assert_ne!(
        hash,
        services
            .roster_upgrade_hash(project, epic, &before, false, &next, &[])
            .unwrap()
    );
    assert_ne!(
        hash,
        services
            .roster_upgrade_hash(project, epic, &after, false, &first, &[])
            .unwrap()
    );
}

#[tokio::test]
async fn selected_source_catalog_conflict_and_caller_claims_refuse_without_publication() {
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let state_root = directory.path().join("state");
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    let (project, _) = business(&daemon);
    let seats = selections(&services);
    let first = publish(&services, project, &seats, "original-catalog").await;
    let catalog = services.published_catalog().unwrap();
    let mut request = serde_json::json!({"seats":[{
        "role":{"catalog_revision":{"id":catalog.catalog_id.to_string(),"version":catalog.version},"role_code":"LSA","custom_display_name":null},
        "presence":"required","ad_hoc_allowed":true
    }]});
    assert!(serde_json::from_value::<CoreTeamPreviewRequest>(request.clone()).is_ok());
    request["seats"][0]["purposes"] = serde_json::json!(["control"]);
    assert!(serde_json::from_value::<CoreTeamPreviewRequest>(request).is_err());
    drop(services);
    drop(daemon);
    let path = folder.join("packs/operational-domain.yml");
    let mut domain = read(&path);
    domain["role_catalogs"][0]["roles"][0]["standard_title"] =
        "Conflicting immutable catalog".into();
    write(&path, &domain);
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    assert!(
        services
            .preview_core_team(
                project,
                &CoreTeamPreviewRequest {
                    seats: Some(seats),
                    source_bundle_hash: None
                }
            )
            .unwrap_err()
            .rule
            .contains("role_purpose_source_catalog_mismatch")
    );
    assert_eq!(services.stored_core_team(project).unwrap().unwrap(), first);
}

#[test]
fn catalog_and_resolved_roster_publication_roll_back_together_on_stale_version() {
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let daemon = start(&directory.path().join("state"), &folder);
    let services = daemon.jira_reconciler();
    let (project, _) = business(&daemon);
    let domain = services.purpose_source().unwrap();
    let catalog = &domain.role_catalogs[0];
    let roster = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        catalog,
        &[],
        domain.delivery.purpose_roles(),
    )
    .unwrap();
    let mut row = StoredCoreTeamRevision {
        project_id: project,
        version: SpecVersion::parse(2).unwrap(),
        catalog_hash: roster.catalog_hash.clone(),
        seats: serde_json::to_value(&roster.seats).unwrap(),
        published_at: kontor_api::now(),
    };
    daemon.state().with_store(|store| {
        assert!(
            store
                .get_role_catalog(catalog.catalog_id, catalog.version)
                .unwrap()
                .is_none()
        );
        assert!(store.publish_core_team_revision(&row, catalog).is_err());
        assert!(store.get_current_core_team(project).unwrap().is_none());
        assert!(
            store
                .get_role_catalog(catalog.catalog_id, catalog.version)
                .unwrap()
                .is_none(),
            "the catalog insert rolls back with the stale roster"
        );
        row.version = SpecVersion::FIRST;
        let mut legacy = roster.clone();
        for seat in &mut legacy.seats {
            seat.purposes.clear();
        }
        row.seats = serde_json::to_value(&legacy.seats).unwrap();
        assert!(store.publish_core_team_revision(&row, catalog).is_err());
        assert!(
            store
                .get_role_catalog(catalog.catalog_id, catalog.version)
                .unwrap()
                .is_none()
        );
        row.seats = serde_json::to_value(&roster.seats).unwrap();
        store.publish_core_team_revision(&row, catalog).unwrap();
        assert_eq!(
            store.get_current_core_team(project).unwrap().unwrap().seats,
            row.seats
        );
        assert_eq!(
            store
                .get_role_catalog(catalog.catalog_id, catalog.version)
                .unwrap()
                .unwrap(),
            *catalog
        );
    });
}

#[tokio::test]
async fn published_bundle_purposes_and_source_pins_survive_unactivated_delivery_edits() {
    use crate::orchestration::{CoreTeamSource, CoreTeamSourceSeat};
    use CoreTeamSeatPurpose::*;
    let directory = tempfile::tempdir().unwrap();
    let folder = source(directory.path());
    let state_root = directory.path().join("state");
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    let (project, _) = business(&daemon);
    let domain = services.purpose_source().unwrap();
    let catalog = &domain.role_catalogs[0];
    let roster = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        catalog,
        &[],
        domain.delivery.purpose_roles(),
    )
    .unwrap();
    let core_team = serde_yaml_ng::to_string(&CoreTeamSource {
        schema_version: 1,
        version: roster.version,
        role_catalog: kontor_fleet_activation::RoleCatalogPin {
            catalog_id: catalog.catalog_id,
            version: catalog.version,
            content_hash: roster.catalog_hash.clone(),
        },
        seats: roster
            .seats
            .iter()
            .map(|seat| CoreTeamSourceSeat {
                role_slot_id: seat.role_slot_id.clone(),
                role_code: seat.role.role_code.clone(),
                custom_display_name: seat.role.custom_display_name.clone(),
                presence: seat.presence,
                ad_hoc_allowed: seat.ad_hoc_allowed,
            })
            .collect(),
    })
    .unwrap();
    let orchestration = std::fs::read_to_string(folder.join(crate::config_folder::ENTRY)).unwrap();
    let fleet = std::fs::read_to_string(folder.join("orchestration/fleet.yml")).unwrap();
    let (bundle, _, _) = services
        .resolve_fleet_bundle(&orchestration, &fleet, &core_team)
        .unwrap();
    for (path, hash) in &services
        .selected_config
        .as_ref()
        .unwrap()
        .realm_pack
        .as_ref()
        .unwrap()
        .sources
    {
        assert_eq!(bundle.manifest.sources.get(path), Some(hash));
    }
    let published = services.fleet.publish_bundle(&bundle).unwrap();
    let original_sources = bundle.manifest.sources.clone();
    drop(services);
    drop(daemon);
    let path = folder.join("packs/operational-domain.yml");
    let mut domain = read(&path);
    domain["delivery"]["architecture_closer_code"] = "SA".into();
    domain["delivery"]["process_closer_code"] = "QA".into();
    domain["delivery"]["control_role_code"] = "QA".into();
    write(&path, &domain);
    let daemon = start(&state_root, &folder);
    let services = daemon.jira_reconciler();
    let verified = services
        .fleet
        .verify_bundle(&published.bundle_hash)
        .unwrap();
    assert_eq!(verified.manifest.sources, original_sources);
    let preview = services
        .preview_core_team(
            project,
            &CoreTeamPreviewRequest {
                seats: None,
                source_bundle_hash: Some(published.bundle_hash.clone()),
            },
        )
        .unwrap();
    let request = CoreTeamApplyRequest {
        seats: None,
        source_bundle_hash: Some(published.bundle_hash),
        preview_hash: preview.preview_hash,
        expected_revision: AggregateRevision::INITIAL,
    };
    let key = IdempotencyKey::parse("frozen-bundle-purposes").unwrap();
    services
        .apply_core_team(&key, project, &request)
        .await
        .unwrap();
    let frozen = services.stored_core_team(project).unwrap().unwrap();
    assert_eq!(frozen, roster);
    assert_eq!(
        frozen
            .seat_for_purpose(ArchitectureCloser)
            .unwrap()
            .role
            .role_code
            .as_str(),
        "LSA"
    );
    assert_eq!(
        frozen
            .seat_for_purpose(Control)
            .unwrap()
            .role
            .role_code
            .as_str(),
        "TPM"
    );
    let replay = services
        .apply_core_team(&key, project, &request)
        .await
        .unwrap();
    assert_eq!(replay.receipt.applied, AppliedDto::Unchanged);
}
