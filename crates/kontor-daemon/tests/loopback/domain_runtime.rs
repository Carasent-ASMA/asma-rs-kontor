//! Pinned-domain refusals precede new runtime effects; historical reads remain available.
use super::*;

#[tokio::test]
async fn changed_domain_artifact_refuses_materialization_and_admission_without_runtime_effects() {
    let composed = compose_realm("/tmp/kontor-domain-runtime-refusal").await;
    let world = &composed.world;
    let project = &composed.project;
    let epic = &composed.epic;
    let hash = world
        .daemon
        .state()
        .with_store(|store| {
            store.get_mini_project_team_definition(
                project_id_of(project),
                MiniProjectId::parse(epic).unwrap(),
            )
        })
        .unwrap()
        .unwrap()
        .operational_domain_bundle_hash
        .unwrap();
    let manifest = kontor_fleet_activation::verify_bundle(world.directory.path(), &hash)
        .unwrap()
        .manifest;
    let path = kontor_fleet_activation::domain::domain_path(
        world.directory.path(),
        &manifest.operational_domain.unwrap().source_hash,
    );
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(
        &path,
        [bytes.as_slice(), b"\n# altered artifact\n"].concat(),
    )
    .unwrap();
    let before = world.fake.calls().len();
    for (uri, body) in [
        (
            format!("/v1/projects/{project}/topology:ensure"),
            serde_json::json!({"target":{"scope":"epic_control","epic_id":epic},"expected_revision":1}),
        ),
        (
            format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
            serde_json::json!({}),
        ),
    ] {
        let refused = Call::post(uri, &body)
            .signed_as(world, "operator")
            .with_key("changed-domain")
            .send(world)
            .await;
        assert!(refused.status.is_client_error(), "{}", refused.body);
        assert!(
            refused
                .body
                .contains("operational_domain_artifact_hash_mismatch"),
            "{}",
            refused.body
        );
        assert_eq!(world.fake.calls().len(), before);
    }
    let read = Call::get(format!("/v1/projects/{project}/epics/{epic}/core-team"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    assert_eq!(world.fake.calls().len(), before);
    std::fs::write(&path, bytes).unwrap();
    assert!(kontor_fleet_activation::verify_bundle(world.directory.path(), &hash).is_ok());
}
