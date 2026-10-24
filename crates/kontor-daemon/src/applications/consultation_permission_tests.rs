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

#[test]
fn recovery_permission_requires_an_exact_frozen_policy_and_preserves_profile_source() {
    let world = World::with_layout(None, true, true, true, Some("managed/<key-slug>/<module>"));
    let policy = world.services.fleet.publish(POLICY).unwrap();
    let rung = selected_route();
    let legacy = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version":1, "ordered_rungs":[rung],
        // A launch-provenance match alone is not a permission pin.
        "constraint_policy_hash":policy.hash,
    }))
    .unwrap();
    let source = "seat_recovery_profile";
    let legacy_provenance = world
        .services
        .recovery_route_provenance(source, &legacy)
        .unwrap();
    assert_eq!(legacy_provenance.permission_policy_hash, None);
    let error = world
        .services
        .qualify_consultation_permissions(legacy_provenance, &rung)
        .unwrap_err();
    assert_eq!(error.code, ApiErrorCode::PlacementBlocked);
    assert!(error.rule.contains("consultation_permission_unpinned"));
    let profile = |hash: &ContentHash, source: &str| {
        CanonicalDocument::from_value(&serde_json::json!({
            "schema_version":1,"ordered_rungs":[rung],
            "consultation_permission_policy":{"source":source,"policy_hash":hash},
        }))
        .unwrap()
    };
    let pinned = profile(&policy.hash, "fleet_configuration");
    let provenance = world
        .services
        .recovery_route_provenance(source, &pinned)
        .unwrap();
    assert_eq!(
        provenance.source,
        ConsultationRouteSource::SeatRecoveryProfile
    );
    assert_eq!(provenance.evidence_hash, *pinned.hash());
    assert_ne!(provenance.evidence_hash, policy.hash);
    let qualified = world
        .services
        .qualify_consultation_permissions(provenance, &rung)
        .unwrap();
    assert_eq!(qualified.permission_policy_hash, Some(policy.hash.clone()));
    let different = world
        .services
        .fleet
        .publish(&POLICY.replace("effort: xhigh, mode:", "effort: high, mode:"))
        .unwrap();
    for document in [
        profile(&different.hash, "fleet_configuration"),
        profile(&ContentHash::of(b"missing"), "fleet_configuration"),
    ] {
        let provenance = world
            .services
            .recovery_route_provenance(source, &document)
            .unwrap();
        assert!(
            world
                .services
                .qualify_consultation_permissions(provenance, &rung)
                .is_err()
        );
    }
    assert!(
        world
            .services
            .recovery_route_provenance(source, &profile(&policy.hash, "template"))
            .is_err()
    );
    for malformed in [
        serde_json::Value::Null,
        serde_json::json!({"source":"fleet_configuration"}),
        serde_json::json!({"source":"fleet_configuration","policy_hash":policy.hash,"extra":true}),
    ] {
        assert!(
            frozen_recovery_permission_hash(
                &serde_json::json!({"consultation_permission_policy":malformed})
            )
            .is_err()
        );
    }
}

#[test]
fn recovery_permission_rejects_conflicting_frozen_route_policy() {
    let world = World::with_layout(None, true, true, true, Some("managed/<key-slug>/<module>"));
    let policy = world.services.fleet.publish(POLICY).unwrap();
    for source in [None, Some("fleet_configuration")] {
        let mut value = serde_json::json!({
            "schema_version":1,"ordered_rungs":[selected_route()],
            "consultation_permission_policy":{"source":"fleet_configuration","policy_hash":policy.hash},
            "fleet_provenance":{"policy_hash":ContentHash::of(b"different route policy"),
                "binding_key":"committee/example/reviewer","chain":"review","step":1,"sub_step":1,"vendor":"example"},
        });
        if let Some(source) = source {
            value["consultation_route_source"] = serde_json::json!(source);
        }
        let profile = CanonicalDocument::from_value(&value).unwrap();
        let error = world
            .services
            .recovery_route_provenance("seat_recovery_profile", &profile)
            .unwrap_err();
        assert_eq!(error.code, ApiErrorCode::PlacementBlocked);
        assert!(error.rule.contains("consultation_permission_mismatch"));
    }
}

#[test]
fn catalog_authority_is_explicit_and_frozen_compatibility_cannot_authorize_a_new_route() {
    let policy = kontor_fleet::FleetSnapshot::parse_policy(POLICY).unwrap();
    let declared = selected_route();
    let compiled = ModelRung {
        provider: ProviderRef("codex".into()),
        model: ModelRef("gpt-5.6-sol".into()),
        effort: None,
    };
    assert!(model_route_is_catalogued(
        &compiled,
        CatalogAuthority::CompiledOnly
    ));
    assert!(!model_route_is_catalogued(
        &compiled,
        CatalogAuthority::PinnedFleet(Some(&policy))
    ));
    assert!(!model_route_is_catalogued(
        &compiled,
        CatalogAuthority::PinnedFleet(None)
    ));
    assert!(model_route_is_catalogued(
        &declared,
        CatalogAuthority::PinnedFleet(Some(&policy))
    ));
    assert!(model_route_is_catalogued(
        &compiled,
        CatalogAuthority::FrozenRecovery {
            snapshot: Some(&policy),
            route: &compiled
        }
    ));
    assert!(!model_route_is_catalogued(
        &declared,
        CatalogAuthority::FrozenRecovery {
            snapshot: Some(&policy),
            route: &compiled
        }
    ));
    assert!(!model_route_is_catalogued(
        &declared,
        CatalogAuthority::CompiledOnly
    ));
    assert_eq!(
        parse_runtime_model_route(&runtime_model_route_dto(&compiled), Some(&policy)).unwrap(),
        compiled
    );
    assert!(
        parse_catalogued_model_route(
            &runtime_model_route_dto(&compiled),
            CatalogAuthority::PinnedFleet(None)
        )
        .is_err()
    );
}
