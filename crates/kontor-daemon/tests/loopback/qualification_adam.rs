//! TASK-003 carry-over: several carriers cannot prove a lost launch safe.
use super::*;
use kontor_core::durable_seat::{DurableSeatActivity, DurableSeatCarrier};

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
