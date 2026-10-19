//! TASK-003 carry-over: several carriers cannot prove a lost launch safe.
use super::*;
use kontor_core::durable_seat::{DurableSeatActivity, DurableSeatCarrier};

/// Reach real persisted completion owners before checking shared-desk authority.
pub(super) async fn prove_completion_authority(
    realm: &DeskLed,
    alpha: &str,
    beta: &str,
    alpha_tpm: SeatBindingId,
    beta_tpm: SeatBindingId,
    durable: SeatBindingId,
    account: &str,
) {
    let task = realm
        .world
        .daemon
        .state()
        .with_store(|store| store.list_tasks(realm.project_id))
        .expect("tasks")
        .into_iter()
        .find(|task| task.mini_project_id == Some(epic_id(beta)))
        .expect("beta task");
    let seed = Bootstrapped {
        project: realm.project.clone(),
        project_revision: current_project_revision(&realm.world, &realm.project).await,
        epic: beta.to_owned(),
        task: task.id.to_string(),
        task_revision: task.revision.get(),
        account: account.to_owned(),
    };
    let runs = seat_existing(&realm.world, &seed, "qualification-beta-delivery").await;
    settle_every_seat(&realm.world, &seed, &runs, "qualification-beta-delivery").await;
    let completed =
        discharge_the_profile_and_complete(&realm.world, &seed, "qualification-beta-delivery")
            .await;
    assert_eq!(completed.status, 200, "{}", completed.body);
    let started = Call::post(
        format!(
            "/v1/projects/{}/epics/{beta}/completion:advance",
            realm.project
        ),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(&realm.world, "operator")
    .with_key("qualification-beta-completion")
    .send(&realm.world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert_eq!(completion_tpm(realm, beta), beta_tpm.to_string());
    assert_eq!(completion_tpm(realm, alpha), alpha_tpm.to_string());
    assert_ne!(alpha_tpm, beta_tpm);

    let now = at("2026-10-11T12:00:00Z");
    let before = realm.world.daemon.state().with_store(|store| {
        kontor_store::backup::export_realm(store, now)
            .expect("before authority export")
            .records_hash
    });
    let calls = realm.world.fake.calls();
    for (epic, own, foreign) in [(alpha, alpha_tpm, beta_tpm), (beta, beta_tpm, alpha_tpm)] {
        for actor in [own, foreign, durable] {
            let generation = realm
                .world
                .daemon
                .state()
                .with_store(|store| {
                    store.hosted_topology_seat_occupancy_generation(realm.project_id, actor)
                })
                .expect("occupancy")
                .expect("hosted actor");
            let token = realm
                .world
                .daemon
                .state()
                .credentials()
                .seat_credential_for_generation(actor, generation);
            let revision = realm
                .world
                .daemon
                .state()
                .with_store(|store| store.get_epic_completion(realm.project_id, epic_id(epic)))
                .expect("completion")
                .expect("persisted completion")
                .revision;
            let key = format!("qualification-authority-{epic}-{actor}");
            let answer = Call::post(
                format!(
                    "/v1/projects/{}/epics/{epic}/completion:remediate",
                    realm.project
                ),
                &serde_json::json!({"expected_revision": revision, "action": {
                    "action": "tpm_route", "round": 1,
                    "route": kontor_core::id::ContentHash::of(b"qualification-route")}}),
            )
            .with_token(token)
            .with_key(&key)
            .send(&realm.world)
            .await;
            if actor == own {
                // The real current TPM reaches semantic validation. There is
                // no failed-round proposal in this successful integration run.
                assert_eq!(answer.status, 400, "{}", answer.body);
                assert_eq!(answer.json()["code"], "invalid_request");
                assert!(answer.body.contains("no LSA proposal"), "{}", answer.body);
            } else {
                assert_eq!(answer.status, 403, "{}", answer.body);
                assert_eq!(answer.json()["code"], "forbidden");
                assert!(
                    answer
                        .body
                        .contains("must be an active seat of this exact epic"),
                    "{}",
                    answer.body
                );
            }
            assert!(!receipt_recorded(realm, &key));
        }
    }
    assert_eq!(realm.world.fake.calls(), calls);
    assert_eq!(
        realm
            .world
            .daemon
            .state()
            .with_store(|store| kontor_store::backup::export_realm(store, now)
                .expect("after authority export")
                .records_hash),
        before,
        "both foreign TPMs and the shared durable seat are refused before any durable or native effect"
    );
}

#[tokio::test]
async fn several_carriers_refuse_origin_binding_without_fencing_or_graph_writes() {
    let realm = desk_led_realm("qualification-several-carriers").await;
    realm.world.fake.lose_next_hosted_launch_ack();
    let lost = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Several carriers", "model_route": claude_route()}),
        "qualification-lost-open",
    )
    .await;
    assert!(!lost.status.is_success(), "{}", lost.body);
    let (origin, lsa) = ad_hoc(&realm, "Several carriers", "qualification-topic").await;
    realm.world.fake.add_hosted_seat_carrier(
        lsa,
        DurableSeatCarrier {
            native_id: ExternalId::parse("qualification-second-carrier").expect("native"),
            archived: false,
            activity: DurableSeatActivity::Idle,
        },
    );
    let intent = ad_hoc_launch_intent(&realm, lsa);
    let nodes = realm.world.daemon.state().with_store(|store| {
        store
            .list_project_topology_nodes(realm.project_id)
            .expect("nodes")
    });
    let before = realm.world.fake.calls().len();
    let minted = realm.world.fake.minted_natives();
    for _ in 0..2 {
        let refused = apply_epic(
            &realm,
            "Ambiguous origin",
            "qualification-bind",
            Some(serde_json::json!({"subject": origin})),
        )
        .await;
        assert_eq!(
            refused.json()["code"],
            "placement_blocked",
            "{}",
            refused.body
        );
        assert!(!receipt_recorded(&realm, "qualification-bind"));
        assert!(!bound_from(&realm, origin));
        assert!(hosted(&realm, lsa).is_none());
        assert_eq!(ad_hoc_launch_intent(&realm, lsa), intent);
        assert_eq!(seat_row(&realm, lsa).lifecycle, TopologyLifecycle::Active);
        assert_eq!(
            realm.world.daemon.state().with_store(|store| store
                .list_project_topology_nodes(realm.project_id)
                .expect("nodes")),
            nodes
        );
    }
    assert_eq!(
        seat_effects_since(&realm, before),
        vec![
            AdapterCall::CensusHostedSeat(lsa),
            AdapterCall::CensusHostedSeat(lsa)
        ]
    );
    assert_eq!(realm.world.fake.minted_natives(), minted);
}
