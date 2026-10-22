//! Preview authority includes the resolved snapshot, not only its effect summary.
use super::*;
use kontor_core::repository::NewProject;

fn world() -> (tempfile::TempDir, crate::Daemon) {
    let root = tempfile::tempdir().unwrap();
    let daemon = crate::Daemon::start(
        crate::DaemonConfig::at(root.path()).with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap();
    (root, daemon)
}

fn roster() -> CoreTeamRevision {
    let catalog = kontor_profiles::bundled_operational_domain()
        .unwrap()
        .role_catalogs
        .remove(0);
    CoreTeamRevision::resolve(SpecVersion::FIRST, &catalog, &[]).unwrap()
}

fn frozen(revision: CoreTeamRevision) -> FrozenRoster {
    FrozenRoster {
        revision,
        revision_of_epic: AggregateRevision::INITIAL,
        quick_session_id: None,
    }
}

#[test]
fn roster_preview_hashes_bind_source_and_target_metadata_without_visible_effect_changes() {
    let (_root, daemon) = world();
    let services = daemon.jira_reconciler();
    let project = ProjectId::generate();
    let epic = MiniProjectId::generate();
    let current = frozen(roster());
    let mut target = current.revision.clone();
    target.version = target.version.next().unwrap();
    let mut edited = target.clone();
    edited.seats[0].role.custom_display_name =
        Some(ExternalName::parse("A different label").unwrap());
    let effects = core_team_effects(Some(&current.revision), &target).unwrap();
    assert_eq!(
        effects,
        core_team_effects(Some(&current.revision), &edited).unwrap()
    );
    assert_ne!(
        services
            .core_team_hash(project, &target, &effects, None)
            .unwrap(),
        services
            .core_team_hash(project, &edited, &effects, None)
            .unwrap(),
    );
    let effects = roster_upgrade_effects(&current, &target).unwrap();
    assert_eq!(effects, roster_upgrade_effects(&current, &edited).unwrap());
    let hash = services
        .roster_upgrade_hash(project, epic, &current, false, &target, &effects)
        .unwrap();
    assert_ne!(
        hash,
        services
            .roster_upgrade_hash(project, epic, &current, false, &edited, &effects)
            .unwrap()
    );
    let mut changed_source = current.revision.clone();
    changed_source.seats[0].ad_hoc_allowed = !changed_source.seats[0].ad_hoc_allowed;
    let changed_source = frozen(changed_source);
    assert_eq!(
        effects,
        roster_upgrade_effects(&changed_source, &target).unwrap()
    );
    assert_ne!(
        hash,
        services
            .roster_upgrade_hash(project, epic, &changed_source, false, &target, &effects)
            .unwrap()
    );
}

#[test]
fn promotion_preview_hash_binds_the_exact_frozen_roster() {
    let (_root, daemon) = world();
    let services = daemon.jira_reconciler();
    let baseline = frozen(roster());
    let mut edited = baseline.revision.clone();
    edited.seats[0].role.custom_display_name =
        Some(ExternalName::parse("Changed snapshot").unwrap());
    let edited = frozen(edited);
    let seat = &baseline.revision.seats[0];
    let quick = StoredQuickSession {
        id: QuickSessionId::generate(),
        project_id: ProjectId::generate(),
        role: seat.role.clone(),
        role_slot_id: seat.role_slot_id.clone(),
        topology_node_id: TopologyNodeId::generate(),
        seat_binding_id: SeatBindingId::generate(),
        psw_topology_node_id: TopologyNodeId::generate(),
        psw_native_id: None,
        purpose: BoundedText::parse("Synthetic promotion hash fixture").unwrap(),
        intent_hash: ContentHash::of(b"synthetic quick intent"),
        disposition: SourceDisposition::Idle,
        revision: AggregateRevision::INITIAL,
        created_at: kontor_api::now(),
    };
    assert_ne!(
        services.promotion_hash(&quick, &baseline, &[]).unwrap(),
        services.promotion_hash(&quick, &edited, &[]).unwrap(),
    );
}

#[tokio::test]
async fn an_apply_cannot_substitute_resolved_metadata_under_an_old_preview_hash() {
    let (_root, daemon) = world();
    let state = daemon.state();
    let services = daemon.jira_reconciler();
    let project = ProjectId::generate();
    state
        .with_store(|store| {
            store.create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Snapshot authorization fixture").unwrap(),
                root_path: ExternalName::parse("/example/project").unwrap(),
                created_at: kontor_api::now(),
            })
        })
        .unwrap();
    let mut seats: Vec<_> = roster()
        .seats
        .into_iter()
        .map(|seat| CoreTeamSeatSelectionDto {
            role: kontor_api::applications::RoleSelectionDto {
                catalog_revision: RevisionRefDto {
                    id: seat.role.catalog_id.to_string(),
                    version: seat.role.catalog_revision,
                },
                role_code: seat.role.role_code,
                custom_display_name: seat.role.custom_display_name,
            },
            presence: seat.presence,
            ad_hoc_allowed: seat.ad_hoc_allowed,
        })
        .collect();
    let request = CoreTeamPreviewRequest {
        seats: Some(seats.clone()),
        source_bundle_hash: None,
    };
    let preview = services.preview_core_team(project, &request).unwrap();
    let expected_revision = AggregateRevision::INITIAL;
    seats[0].role.custom_display_name = Some(ExternalName::parse("Unpreviewed change").unwrap());
    let error = services
        .apply_core_team(
            &IdempotencyKey::parse("stale-snapshot").unwrap(),
            project,
            &CoreTeamApplyRequest {
                seats: Some(seats),
                source_bundle_hash: None,
                preview_hash: preview.preview_hash.clone(),
                expected_revision,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ApiErrorCode::InvalidRequest);
    assert!(services.stored_core_team(project).unwrap().is_none());
    assert!(
        state
            .with_store(
                |store| store.get_receipt_by_key(&IdempotencyKey::parse("stale-snapshot").unwrap())
            )
            .unwrap()
            .is_none()
    );
    services
        .apply_core_team(
            &IdempotencyKey::parse("exact-snapshot").unwrap(),
            project,
            &CoreTeamApplyRequest {
                seats: request.seats,
                source_bundle_hash: None,
                preview_hash: preview.preview_hash,
                expected_revision,
            },
        )
        .await
        .unwrap();
    assert!(services.stored_core_team(project).unwrap().is_some());
}
