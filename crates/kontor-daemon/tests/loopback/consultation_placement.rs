//! Selected consultation source/layout survives restart; refusals precede native effects.
use super::*;

#[tokio::test]
async fn native_consultation_restart_keeps_source_layout_binding_and_refuses_missing_source() {
    native_consultation(false).await;
}

#[tokio::test]
async fn native_consultation_task_subject_recovery_keeps_its_own_root() {
    native_consultation(true).await;
}

async fn native_consultation(task_subject: bool) {
    let fixture = jira_key_consultation_fixture("native-consultation-restart").await;
    let ConsultationFixture {
        composed,
        caller,
        task_id,
        ..
    } = fixture;
    let Composed {
        world,
        project,
        epic,
        ..
    } = composed;
    let uri = format!("/v1/projects/{project}/epics/{epic}/advisor-runs:invoke");
    let epic_read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(epic_read.status, 200, "{}", epic_read.body);
    let body = serde_json::json!({"profile":{"id":ADVISOR_PROFILE,"version":1},"topic":"Pinned native placement","question":"Does the native binding keep its exact source?","caller_seat_binding_id":caller,"task_id":task_subject.then_some(task_id),"expected_revision":epic_read.json()["revision"]});
    prepare_fake_provider_headroom(&world, &project).await;
    let invoked = Call::post(uri, &body)
        .signed_as(&world, "operator")
        .with_key("native-placement-invoke")
        .send(&world)
        .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let id =
        kontor_core::id::AdvisorRunId::parse(invoked.json()["advisor_run_id"].as_str().unwrap())
            .unwrap();
    let project_id = ProjectId::parse(&project).unwrap();
    let run = world
        .daemon
        .state()
        .with_store(|s| {
            s.get_consultation_run(
                project_id,
                kontor_core::consultation::ConsultationRunId::Advisor(id),
            )
        })
        .unwrap()
        .unwrap();
    let node = run.topology_node_id;
    let before = world
        .daemon
        .state()
        .with_store(|s| s.consultation_placement(project_id, node))
        .unwrap()
        .unwrap();
    let pin: kontor_runtime::consultation_checkout::ConsultationCheckout =
        serde_json::from_str(before.intent.json()).unwrap();
    assert!(pin.checkout.destination.contains(&format!(
        "/managed/trees/consultations/{}/{}",
        pin.key.to_ascii_lowercase(),
        node
    )));
    assert!(before.readback.is_some());
    let bound = world
        .daemon
        .state()
        .with_store(|s| s.get_topology_node_container(project_id, node))
        .unwrap()
        .unwrap();
    assert_eq!(
        bound.container_binding_id.as_str(),
        pin.container_binding_id
    );
    let entry = world
        .directory
        .path()
        .join("selected config with spaces/orchestration/orchestration.yml");
    let mut yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    yaml["project"]["consultation_layout"] = "unactivated/<node>/<key-slug>".into();
    std::fs::write(entry, serde_yaml_ng::to_string(&yaml).unwrap()).unwrap();
    let world = world.restart_native_consultations().await;
    let materialize = |revision| serde_json::json!({"target":{"scope":"advisor_consultation","advisor_run_id":id},"expected_revision":revision});
    let rematerialized = Call::post(
        format!("/v1/projects/{project}/topology:materialize"),
        &materialize(1),
    )
    .signed_as(&world, "operator")
    .with_key("native-placement-restart")
    .send(&world)
    .await;
    assert_eq!(rematerialized.status, 200, "{}", rematerialized.body);
    let after = world
        .daemon
        .state()
        .with_store(|s| s.consultation_placement(project_id, node))
        .unwrap()
        .unwrap();
    assert_eq!(after.intent, before.intent);
    assert_eq!(after.readback, before.readback);
    let resumed = world
        .daemon
        .state()
        .with_store(|s| s.get_topology_node_container(project_id, node))
        .unwrap()
        .unwrap();
    assert_eq!(resumed.identity, bound.identity);
    assert_eq!(resumed.canonical_cwd, bound.canonical_cwd);
    // Adopt the sole exact native replacement through the existing guarded recovery.
    world.fake.drift_container(
        node,
        kontor_runtime::fake::FakeContainerDrift::NativeId(
            ExternalId::parse("replacement-consultation-native").unwrap(),
        ),
    );
    let preview = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-preview"),
        &serde_json::json!({"expected_revision":1}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let recover = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-apply"),
        &serde_json::json!({"expected_revision":1,"preview_hash":preview.json()["preview_hash"]}),
    )
    .signed_as(&world, "admin")
    .with_key("native-consultation-recover")
    .send(&world)
    .await;
    assert_eq!(recover.status, 200, "{}", recover.body);
    let recovered = world
        .daemon
        .state()
        .with_store(|s| s.get_topology_node_container(project_id, node))
        .unwrap()
        .unwrap();
    assert_eq!(recovered.container_binding_id, bound.container_binding_id);
    assert_ne!(recovered.identity, bound.identity);
    assert_eq!(recovered.canonical_cwd, bound.canonical_cwd);
    let kept = world
        .daemon
        .state()
        .with_store(|s| s.consultation_placement(project_id, node))
        .unwrap()
        .unwrap();
    assert_eq!(kept.intent, before.intent);
    assert_eq!(kept.readback, before.readback);
    let source = std::path::Path::new(&pin.checkout.source.declaration.path);
    std::fs::rename(source.join(".git"), source.join("missing-git")).unwrap();
    world.fake.take_calls();
    let refused = Call::post(
        format!("/v1/projects/{project}/topology:materialize"),
        &materialize(current_project_revision(&world, &project).await),
    )
    .signed_as(&world, "operator")
    .with_key("native-placement-missing-source")
    .send(&world)
    .await;
    assert!(refused.status.is_client_error(), "{}", refused.body);
    assert!(!world.fake.calls().iter().any(|call| matches!(
        call,
        AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
    )));
    let read = Call::get(format!("/v1/projects/{project}/advisor-runs/{id}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    std::fs::rename(source.join("missing-git"), source.join(".git")).unwrap();
}
