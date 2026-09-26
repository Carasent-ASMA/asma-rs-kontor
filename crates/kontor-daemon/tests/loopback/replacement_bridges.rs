//! Public naming preview must preserve structural abandoned replacement bridges.
use super::*;

async fn fixture(slug: &'static str) -> (World, String, String, kontor_core::repository::AgentRun) {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _, seats) = seated_turns_with_attribution(&world, slug, true).await;
    let predecessor = seats.as_array().unwrap()[1]["agent_run_id"]
        .as_str()
        .unwrap();
    finish_natively(&world, predecessor).await;
    let settled = Call::post(
        format!("/v1/projects/{project}/agent-runs/{predecessor}/runtime:settle"),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key(format!("{slug}-settle"))
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    assert_eq!(settled.json()["observed"], "cancelled");
    let run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(
                ProjectId::parse(&project).unwrap(),
                AgentRunId::parse(predecessor).unwrap(),
            )
            .unwrap()
            .unwrap()
    });
    assert!(run.terminal.is_some());
    (world, project, epic, run)
}

fn successor(
    world: &World,
    parent: &kontor_core::repository::AgentRun,
    native: Option<&str>,
) -> kontor_core::repository::AgentRun {
    let id = AgentRunId::generate();
    // Unknown runtime IDs deliberately remain absent from the scripted runtime.
    // Preview must expose the exact current ID as rename_pending, never borrow
    // the real predecessor's native. This fixture grants no launch evidence.
    let binding = native.map(|native| RuntimeBinding {
        id: RuntimeBindingId::generate(),
        agent_run_id: id,
        identity: NativeRuntimeIdentity {
            runtime_kind: fake_family(),
            host: name("fake-host"),
            generation: 1,
            native_id: ExternalId::parse(native).unwrap(),
        },
        bound_at: kontor_api::now(),
    });
    world.daemon.state().with_store(|store| {
        store
            .create_agent_run(&NewAgentRun {
                id,
                project_id: parent.project_id,
                team_run_id: parent.team_run_id,
                parent_agent_run_id: Some(parent.id),
                role: parent.role.clone(),
                account_profile_id: parent.account_profile_id,
                binding,
                created_at: kontor_api::now(),
            })
            .unwrap()
    })
}

async fn abandon(world: &World, run: &kontor_core::repository::AgentRun) {
    let response = Call::post(format!("/v1/projects/{}/agent-runs/{}/runtime:abandon", run.project_id, run.id),
        &serde_json::json!({"expected_revision":run.revision.get(), "reason":"Never-bound fixture attempt"}))
        .signed_as(world, "operator").with_key(format!("bridge-abandon-{}", run.id)).send(world).await;
    assert_eq!(response.status, 200, "{}", response.body);
    let stored = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(run.project_id, run.id)
            .unwrap()
            .unwrap()
    });
    assert!(stored.is_operator_abandoned_unbound());
}

async fn preview(world: &World, project: &str, epic: &str) -> harness::Answer {
    Call::post(
        format!("/v1/projects/{project}/epics/{epic}/native-names:preview"),
        &serde_json::json!({"expected_revision":current_project_revision(world, project).await}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await
}

#[tokio::test]
async fn cancelled_bound_run_through_abandoned_bridges_names_only_the_current_native() {
    for bridge_count in [1, 2] {
        let (world, project, epic, predecessor) = fixture("preview-bridge").await;
        let old_native = predecessor
            .binding
            .as_ref()
            .unwrap()
            .identity
            .native_id
            .clone();
        let before = preview(&world, &project, &epic).await;
        assert_eq!(before.status, 200, "{}", before.body);
        let original_seat = before.json()["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["agent_run_id"] == predecessor.id.to_string())
            .unwrap()["seat_binding_id"]
            .clone();
        let mut parent = predecessor.clone();
        for _ in 0..bridge_count {
            parent = successor(&world, &parent, None);
            abandon(&world, &parent).await;
        }
        let current = successor(&world, &parent, Some("missing-current-after-bridge"));
        let response = preview(&world, &project, &epic).await;
        assert_eq!(response.status, 200, "{}", response.body);
        let value = response.json();
        let targets = value["targets"].as_array().unwrap();
        let target = targets
            .iter()
            .find(|target| target["agent_run_id"] == current.id.to_string())
            .expect("the exact current run must be a preview target");
        assert_eq!(target["native_id"], "missing-current-after-bridge");
        assert_eq!(target["seat_binding_id"], original_seat);
        assert_eq!(
            target["generation"],
            predecessor.binding.as_ref().unwrap().identity.generation
        );
        assert_eq!(target["capability"], "rename_pending");
        assert!(
            targets
                .iter()
                .all(|target| target["native_id"] != old_native.as_str())
        );
        let stored = world.daemon.state().with_store(|store| {
            store
                .get_agent_run(current.project_id, current.id)
                .unwrap()
                .unwrap()
        });
        assert_eq!(stored.parent_agent_run_id, Some(parent.id));
        assert_eq!(stored.binding, current.binding);
    }
}

#[tokio::test]
async fn trailing_abandoned_bridge_attempts_keep_the_bound_predecessor_named() {
    let (world, project, epic, predecessor) = fixture("preview-trailing-bridge").await;
    let first = successor(&world, &predecessor, None);
    abandon(&world, &first).await;
    let second = successor(&world, &first, None);
    abandon(&world, &second).await;
    let response = preview(&world, &project, &epic).await;
    assert_eq!(response.status, 200, "{}", response.body);
    assert!(
        response.json()["targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target["agent_run_id"] == predecessor.id.to_string())
    );
}

#[tokio::test]
async fn genuine_bound_fork_after_an_abandoned_bridge_still_refuses_preview() {
    for sibling_native in [Some("missing-fork-b"), None] {
        let (world, project, epic, predecessor) = fixture("preview-fork-bridge").await;
        let bridge = successor(&world, &predecessor, None);
        abandon(&world, &bridge).await;
        successor(&world, &bridge, Some("missing-fork-a"));
        successor(&world, &bridge, sibling_native);
        let response = preview(&world, &project, &epic).await;
        assert_eq!(response.status, 409, "{}", response.body);
        assert!(
            response
                .body
                .contains("ambiguous current replacement-chain leaves")
        );
        assert_eq!(
            response.json()["action"],
            "reconcile the replacement lineage with its recorded authority while preserving run, parent, seat and native identities"
        );
    }
}

#[tokio::test]
async fn replacing_another_slot_hydrates_two_abandoned_bridges_without_rewriting_them() {
    let (world, project, epic, predecessor) = fixture("replace-two-bridges").await;
    let first = successor(&world, &predecessor, None);
    abandon(&world, &first).await;
    let second = successor(&world, &first, None);
    abandon(&world, &second).await;
    let current = successor(&world, &second, Some("missing-current-for-hydration"));
    let other = world.daemon.state().with_store(|store| {
        store
            .list_agent_runs_for_team_run(predecessor.project_id, predecessor.team_run_id)
            .unwrap()
            .into_iter()
            .filter(|row| row.role != predecessor.role)
            .map(|row| {
                store
                    .get_agent_run(predecessor.project_id, row.agent_run_id)
                    .unwrap()
                    .unwrap()
            })
            .find(|run| run.binding.is_some() && run.terminal.is_none())
            .unwrap()
    });
    finish_natively(&world, &other.id.to_string()).await;
    let settled = Call::post(
        format!(
            "/v1/projects/{project}/agent-runs/{}/runtime:settle",
            other.id
        ),
        &serde_json::json!({}),
    )
    .signed_as(&world, "operator")
    .with_key("replace-two-bridges-settle-other")
    .send(&world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    let other = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(other.project_id, other.id)
            .unwrap()
            .unwrap()
    });
    let view = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    let response = Call::post(format!("/v1/projects/{project}/agent-runs/{}/successors:replace", other.id),
        &serde_json::json!({"role_slot":other.role.as_str(), "expected_predecessor_revision":other.revision,
            "expected_task_revision":view.json()["tasks"][0]["revision"], "binding_generation":other.binding.as_ref().unwrap().identity.generation}))
        .signed_as(&world, "admin").with_key("replace-two-bridges-other").send(&world).await;
    assert_eq!(response.status, 200, "{}", response.body);
    assert_eq!(response.json()["applied"], "created");
    let replacement =
        AgentRunId::parse(response.json()["successor_agent_run_id"].as_str().unwrap()).unwrap();
    world.daemon.state().with_store(|store| {
        let replaced = store
            .get_agent_run(other.project_id, replacement)
            .unwrap()
            .unwrap();
        assert_eq!(replaced.parent_agent_run_id, Some(other.id));
        assert!(
            replaced.binding.is_some(),
            "the requested other slot actually launches"
        );
        for original in [&predecessor, &first, &second, &current] {
            let stored = store
                .get_agent_run(original.project_id, original.id)
                .unwrap()
                .unwrap();
            assert_eq!(stored.parent_agent_run_id, original.parent_agent_run_id);
            assert_eq!(stored.binding, original.binding);
        }
    });
}
