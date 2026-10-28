//! Real request writers freeze routes before effects and replay without current inputs.
use super::*;

#[tokio::test]
async fn q04_consultation_restart_uses_the_initial_private_envelope_after_host_edit() {
    let ConsultationFixture {
        composed, caller, ..
    } = jira_key_consultation_fixture("q04-native-host-restart").await;
    let Composed {
        world,
        project,
        epic,
        ..
    } = composed;
    let read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    prepare_fake_provider_headroom(&world, &project).await;
    let request = serde_json::json!({"profile":{"id":ADVISOR_PROFILE,"version":1},"topic":"Frozen host route","question":"Does restart keep the native selector?","caller_seat_binding_id":caller,"expected_revision":read.json()["revision"]});
    let uri = format!("/v1/projects/{project}/epics/{epic}/advisor-runs:invoke");
    let slot = RoleSlotId::parse("auditor").unwrap();
    world.fake.refusing_launch_of(&slot);
    let first = Call::post(&uri, &request)
        .signed_as(&world, "operator")
        .with_key("q04-native-invoke")
        .send(&world)
        .await;
    assert_ne!(
        first.status, 200,
        "the first effect is deliberately interrupted: {}",
        first.body
    );
    let project_id = project_id_of(&project);
    let key = IdempotencyKey::parse("q04-native-invoke").unwrap();
    let run = world
        .daemon
        .state()
        .with_store(|store| store.get_consultation_run_by_key(project_id, &key))
        .unwrap()
        .unwrap();
    let before = world
        .daemon
        .state()
        .with_store(|store| store.list_consultation_seats(project_id, run.id))
        .unwrap();
    assert!(!before.is_empty());
    for seat in &before {
        assert!(seat.model_rung.frozen().is_some());
    }
    let host_path = world
        .directory
        .path()
        .join("selected config with spaces/host.yml");
    let mut host: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&host_path).unwrap()).unwrap();
    for account in host["provider_accounts"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        account["native_alias"] = serde_json::json!("opaque-edited-host");
    }
    std::fs::write(host_path, serde_yaml_ng::to_string(&host).unwrap()).unwrap();
    std::fs::remove_file(
        world
            .directory
            .path()
            .join(kontor_fleet_activation::FLEET_ACTIVATION_FILE),
    )
    .unwrap();
    world.fake.allowing_launch_of(&slot);
    let world = world.restart_native_consultations().await;
    let replay = Call::post(uri, &request)
        .signed_as(&world, "operator")
        .with_key("q04-native-invoke")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    let after = world
        .daemon
        .state()
        .with_store(|store| store.list_consultation_seats(project_id, run.id))
        .unwrap();
    for (old, new) in before.iter().zip(&after) {
        assert_eq!(old.model_rung, new.model_rung);
        assert_eq!(
            world.fake.consultation_launch_route(new.seat_binding_id),
            Some(old.model_rung.clone())
        );
        let private = old.model_rung.frozen().unwrap();
        assert!(!replay.body.contains(&private.native_alias));
        assert!(!run.context.to_string().contains(&private.native_alias));
    }
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.get_consultation_run(project_id, run.id))
            .unwrap()
            .unwrap()
            .context,
        run.context
    );
}

#[tokio::test]
async fn q04_claim_projects_canonical_identity_and_replays_after_host_edit() {
    let ConsultationFixture {
        composed, caller, ..
    } = jira_key_consultation_fixture("q04-native-claim").await;
    let Composed {
        world,
        project,
        epic,
        ..
    } = composed;
    let project_id = project_id_of(&project);
    let seat_id = SeatBindingId::parse(&caller).unwrap();
    let predecessor = world
        .daemon
        .state()
        .with_store(|store| store.get_hosted_topology_seat(project_id, seat_id))
        .unwrap()
        .unwrap();
    let frozen = world
        .daemon
        .state()
        .with_store(|store| store.get_hosted_seat_launch_intent(project_id, seat_id, 1))
        .unwrap()
        .unwrap()
        .model_rung;
    let private = frozen.frozen().unwrap();
    let control = epic_control_node(&world, project_id, epic_id_of(&epic));
    world
        .fake
        .seed_hosted_seat_claimant(
            control.id,
            ExternalId::parse("q04-claimant").unwrap(),
            Some(ExternalId::parse("q04-conversation").unwrap()),
            ModelRung {
                provider: ProviderRef(private.native_alias.clone()),
                model: ModelRef(private.route_model_id.clone()),
                effort: private.canonical_effort,
            },
            "existing synthetic session",
        )
        .unwrap();
    let read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let route = format!("/v1/projects/{project}/epics/{epic}/core-team/seat-claims");
    let request = serde_json::json!({"expected_revision":read.json()["revision"],"seat_binding_id":seat_id,"claimant_native_id":"q04-claimant","expected_current_native_id":predecessor.native_identity.native_id});
    let preview = Call::post(format!("{route}:preview"), &request)
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(
        preview.json()["claimant_model_route"]["provider"],
        predecessor.model_rung.provider.0
    );
    assert!(!preview.body.contains(&private.native_alias));
    let mut apply = request;
    apply["preview_hash"] = preview.json()["preview_hash"].clone();
    let first = Call::post(format!("{route}:apply"), &apply)
        .signed_as(&world, "admin")
        .with_key("q04-native-claim")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert!(!first.body.contains(&private.native_alias));
    let (active, intent) = world.daemon.state().with_store(|store| {
        (
            store
                .get_hosted_topology_seat(project_id, seat_id)
                .unwrap()
                .unwrap(),
            store
                .get_hosted_seat_launch_intent(project_id, seat_id, 2)
                .unwrap()
                .unwrap(),
        )
    });
    assert_eq!(active.model_rung, predecessor.model_rung);
    assert_eq!(intent.model_rung.canonical_rung(), active.model_rung);
    assert_eq!(
        intent.model_rung.frozen().unwrap().native_alias,
        private.native_alias
    );
    assert_eq!(
        intent.observed_native_id.as_ref().unwrap().as_str(),
        "q04-claimant"
    );
    let host_path = world
        .directory
        .path()
        .join("selected config with spaces/host.yml");
    let host = std::fs::read_to_string(&host_path)
        .unwrap()
        .replace(&private.native_alias, "q04-edited-claim-selector");
    std::fs::write(host_path, host).unwrap();
    std::fs::remove_file(
        world
            .directory
            .path()
            .join(kontor_fleet_activation::FLEET_ACTIVATION_FILE),
    )
    .unwrap();
    let world = world.restart_native_consultations().await;
    let replay = Call::post(format!("{route}:apply"), &apply)
        .signed_as(&world, "admin")
        .with_key("q04-native-claim")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert!(!replay.body.contains(&private.native_alias));
    assert!(!replay.body.contains("q04-edited-claim-selector"));
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.get_hosted_seat_launch_intent(project_id, seat_id, 2))
            .unwrap()
            .unwrap(),
        intent
    );
}
