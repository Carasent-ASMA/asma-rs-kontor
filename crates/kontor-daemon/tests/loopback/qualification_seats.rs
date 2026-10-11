//! TASK-002 carry-over: generic retirement must not bypass desk protection.
use super::*;

#[tokio::test]
async fn generic_retire_refuses_a_desk_seat_without_native_or_store_effects() {
    let (world, project) = seat_world("qualification-retire").await;
    let binding = declared(&world, &project, &daemon_seat("fixture", "FIXTURE"))
        .await
        .expect("daemon binding");
    let launched = reconcile(&world, &project, &["fixture"], true, "qualification-launch").await;
    assert_eq!(launched.status, 200, "{}", launched.body);
    let report = list(&world, &project).await;
    let before = world.fake.calls().len();
    let revision = world.daemon.state().with_store(|store| {
        store
            .get_seat_binding(ProjectId::parse(&project).expect("project"), binding)
            .expect("seat")
            .expect("binding")
            .revision
            .get()
    });
    let refused = Call::post(
        format!("/v1/projects/{project}/seat-bindings/{binding}/retire"),
        &serde_json::json!({"expected_revision": revision, "reason": "qualification refusal"}),
    )
    .signed_as(&world, "admin")
    .with_key("qualification-retire-refused")
    .send(&world)
    .await;
    assert_eq!(
        refused.json()["code"],
        "placement_blocked",
        "{}",
        refused.body
    );
    assert!(receipt_kind(&world, "qualification-retire-refused").is_none());
    assert!(world.fake.calls()[before..].iter().all(|call| !matches!(
        call,
        AdapterCall::RetireHostedSeat(_) | AdapterCall::LaunchHostedSeat(_)
    )));
    assert_eq!(list(&world, &project).await, report);
}
