//! Exact permission policy is frozen before effects, including interrupted launch/restart.
use super::*;

async fn advisor(realm: &ConsultationRealm) {
    let mut definition = advisor_definition(ADVISOR_PROFILE, 1);
    definition["allowed_caller_roles"] = serde_json::json!(["lsa"]);
    let preview = Call::post(
        format!("/v1/projects/{}/advisor-profiles:preview", realm.project),
        &serde_json::json!({"definition":definition}),
    )
    .signed_as(&realm.world, "admin")
    .send(&realm.world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let applied = Call::post(format!("/v1/projects/{}/advisor-profiles:apply",realm.project),
        &serde_json::json!({"definition":definition,"preview_hash":preview.json()["preview_hash"],"expected_revision":1}))
        .signed_as(&realm.world,"admin").with_key("permission-advisor").send(&realm.world).await;
    assert_eq!(applied.status, 200, "{}", applied.body);
}

#[tokio::test]
async fn consultation_permission_omission_refuses_before_run_or_native_effects() {
    let realm =
        consultation_realm("/tmp/kontor-permission-omission", &[("Cursor", "cursor")]).await;
    advisor(&realm).await;
    let policy = committee_fleet_yaml(&[(&advisor_fleet_key(), "cursor-grok")]).replacen(
        "schema_version: 1",
        "schema_version: 2",
        1,
    );
    activate_through_the_registry(&realm.world, &policy, "permission-omitted").await;
    let before = realm.world.fake.calls().len();
    let refused = invoke_activated_advisor(&realm, "permission-omitted-invoke").await;
    assert_eq!(refused.code(), "placement_blocked", "{}", refused.body);
    assert!(
        refused.body.contains("consultation_permission_unproven"),
        "{}",
        refused.body
    );
    assert_eq!(before, realm.world.fake.calls().len());
    let runs = realm
        .world
        .daemon
        .state()
        .with_store(|s| {
            s.list_consultation_runs(
                project_id_of(&realm.project),
                MiniProjectId::parse(&realm.epic).unwrap(),
                ConsultationFamily::Advisor,
            )
        })
        .unwrap();
    assert!(runs.is_empty());
}

#[tokio::test]
async fn consultation_permission_pinned_history_survives_activation_and_restart_and_refuses_tampering()
 {
    let realm = consultation_realm("/tmp/kontor-permission-retry", &[("Cursor", "cursor")]).await;
    advisor(&realm).await;
    let policy = permitted_committee_fleet_yaml(&[(&advisor_fleet_key(), "cursor-grok")]);
    let hash = activate_through_the_registry(&realm.world, &policy, "permission-selected").await;
    let slot = RoleSlotId::parse("auditor").unwrap();
    realm.world.fake.refusing_launch_of(&slot);
    let first = invoke_activated_advisor(&realm, "permission-retry").await;
    assert_eq!(first.status, 503, "{}", first.body);
    let pid = project_id_of(&realm.project);
    let run = realm
        .world
        .daemon
        .state()
        .with_store(|s| {
            s.get_consultation_run_by_key(pid, &IdempotencyKey::parse("permission-retry").unwrap())
        })
        .unwrap()
        .unwrap();
    let context = run.context.clone();
    assert_eq!(context["admission"]["profile_hash"], hash);
    let next = committee_fleet_yaml(&[(&advisor_fleet_key(), "codex-sol")]);
    publish_through_the_registry(&realm.world, &next, "permission-unactivated").await;
    write_fleet(&realm.world, &next);
    let path = kontor_fleet_activation::policy_path(
        realm.world.directory.path(),
        &ContentHash::parse(&hash).unwrap(),
    );
    let before = realm.world.fake.calls().len();
    std::fs::write(&path, format!("{policy}\n# tampered\n")).unwrap();
    let refused = invoke_activated_advisor(&realm, "permission-retry").await;
    assert_ne!(refused.status, 200, "{}", refused.body);
    assert_eq!(before, realm.world.fake.calls().len());
    std::fs::remove_file(&path).unwrap();
    let missing = invoke_activated_advisor(&realm, "permission-retry").await;
    assert_ne!(missing.status, 200, "{}", missing.body);
    assert_eq!(before, realm.world.fake.calls().len());
    std::fs::write(&path, &policy).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    activate_through_the_registry(&realm.world, &next, "permission-next").await;
    realm.world.fake.allowing_launch_of(&slot);
    let ConsultationRealm {
        world,
        project,
        epic,
        caller,
    } = realm;
    let world = world.restart_native_consultations().await;
    let realm = ConsultationRealm {
        world,
        project,
        epic,
        caller,
    };
    let retried = invoke_activated_advisor(&realm, "permission-retry").await;
    assert_eq!(retried.status, 200, "{}", retried.body);
    assert_eq!(
        retried.json()["seats"][0]["model_route"]["provider"],
        "cursor"
    );
    let stored = realm
        .world
        .daemon
        .state()
        .with_store(|s| s.get_consultation_run(pid, run.id))
        .unwrap()
        .unwrap();
    assert_eq!(stored.context, context);
    assert_eq!(stored.context_hash, run.context_hash);
    let after = realm.world.fake.calls().len();
    let replay = invoke_activated_advisor(&realm, "permission-retry").await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(realm.world.fake.calls().len(), after);
}
