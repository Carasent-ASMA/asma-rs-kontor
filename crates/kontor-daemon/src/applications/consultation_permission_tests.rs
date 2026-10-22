//! Existing pins select immutable permissions; raw YAML never grants a new exception.
use super::*;

const POLICY: &str = include_str!("../../../../tests/fixtures/config/consultation-permissions.yml");

fn selected_route() -> ModelRung {
    ModelRung {
        provider: ProviderRef("cursor".into()),
        model: ModelRef("review-model".into()),
        effort: Some(EffortLevel::Xhigh),
    }
}

#[test]
fn consultation_permissions_survive_later_activation_and_restart_without_widening() {
    let world = World::with_layout(None, true, true, true, Some("managed/<key-slug>/<module>"));
    let policy = world.services.fleet.publish(POLICY).unwrap();
    let home = world._directory.path().join("state");
    let standing = kontor_fleet_activation::read_activation(&home).unwrap();
    world
        .services
        .fleet
        .activate(&policy.hash, Some(&standing.policy_hash))
        .unwrap();
    let frozen = ConsultationRouteProvenance::fleet_configuration(policy.hash.clone());
    let rung = selected_route();
    assert!(
        world
            .services
            .qualify_consultation_permissions(frozen.clone(), &rung)
            .is_ok()
    );
    let altered = POLICY.replacen("effort: xhigh, mode:", "effort: high, mode:", 1);
    let next = world.services.fleet.publish(&altered).unwrap();
    world
        .services
        .fleet
        .activate(&next.hash, Some(&policy.hash))
        .unwrap();
    let old = world
        .services
        .qualify_consultation_permissions(frozen.clone(), &rung)
        .unwrap();
    assert!(old.fleet_permissions.unwrap().permits(&policy.hash, &rung));
    let high = ModelRung {
        effort: Some(EffortLevel::High),
        ..rung.clone()
    };
    assert!(
        world
            .services
            .qualify_consultation_permissions(frozen, &high)
            .is_err()
    );
    let reopened = crate::fleet::FleetSource::at(&home);
    assert!(
        reopened
            .consultation_permissions(&policy.hash)
            .unwrap()
            .permits(&policy.hash, &rung)
    );
    let path = kontor_fleet_activation::policy_path(&home, &policy.hash);
    assert!(path.exists());
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &altered).unwrap();
    assert!(
        world
            .services
            .qualify_consultation_permissions(
                ConsultationRouteProvenance::fleet_configuration(policy.hash.clone()),
                &rung
            )
            .is_err()
    );
    std::fs::write(&path, bytes).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        world
            .services
            .qualify_consultation_permissions(
                ConsultationRouteProvenance::fleet_configuration(policy.hash),
                &rung
            )
            .is_err()
    );
}

#[test]
fn consultation_permissions_refuse_new_recovery_and_missing_policy_before_effects() {
    let world = World::with_layout(None, true, true, true, Some("managed/<key-slug>/<module>"));
    let rung = selected_route();
    let epic = world
        .state
        .with_store(|s| s.get_task(world.project, world.task))
        .unwrap()
        .unwrap()
        .mini_project_id
        .unwrap();
    let before = world
        .state
        .with_store(|s| s.list_consultation_runs(world.project, epic, ConsultationFamily::Advisor))
        .unwrap()
        .len();
    assert_eq!(before, 0);
    let error = world
        .services
        .qualify_consultation_permissions(
            ConsultationRouteProvenance::operator_accepted_initial_recovery_profile(
                ContentHash::of(b"profile"),
            ),
            &rung,
        )
        .unwrap_err();
    assert_eq!(error.code, ApiErrorCode::PlacementBlocked);
    assert!(
        world
            .services
            .qualify_consultation_permissions(
                ConsultationRouteProvenance::fleet_configuration(ContentHash::of(b"missing")),
                &rung
            )
            .is_err()
    );
    assert_eq!(
        before,
        world
            .state
            .with_store(|s| s.list_consultation_runs(
                world.project,
                epic,
                ConsultationFamily::Advisor
            ))
            .unwrap()
            .len()
    );
}
