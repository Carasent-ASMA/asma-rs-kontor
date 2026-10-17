//! Repair a legacy task container without replacing its logical delivery graph.
use super::*;
use kontor_core::state::NativeContainerBinding;
use kontor_runtime::fake::FakeContainerDrift;
use kontor_runtime::workspace::WorkspaceRoot;

fn container(f: &SeatFillWorld) -> NativeContainerBinding {
    f.world.daemon.state().with_store(|s| {
        s.get_topology_node_container(f.project, f.node)
            .unwrap()
            .unwrap()
    })
}

fn revision(f: &SeatFillWorld) -> AggregateRevision {
    f.world
        .daemon
        .state()
        .with_store(|s| s.get_project(f.project).unwrap().unwrap().revision)
}

fn uri(f: &SeatFillWorld, action: &str) -> String {
    format!(
        "/v1/projects/{}/topology/nodes/{}/container:recovery-{action}",
        f.project, f.node
    )
}

async fn preview(f: &SeatFillWorld) -> Answer {
    Call::post(
        uri(f, "preview"),
        &serde_json::json!({"expected_revision":revision(f)}),
    )
    .signed_as(&f.world, "admin")
    .send(&f.world)
    .await
}

async fn apply(f: &SeatFillWorld, hash: &serde_json::Value, key: &str) -> Answer {
    Call::post(
        uri(f, "apply"),
        &serde_json::json!({"expected_revision":revision(f),"preview_hash":hash}),
    )
    .signed_as(&f.world, "admin")
    .with_key(key)
    .send(&f.world)
    .await
}

/// The original bug's state: the same node and memberships survived an older
/// binding rooted in the superproject while the task claimed its module tree.
async fn misplaced() -> (SeatFillWorld, NativeContainerBinding) {
    let f = seat_fill_world_with_recovery(false, 1, false).await;
    let adopted = adopt_slot(
        &f,
        "verify",
        &adoption_body(&f),
        "root-adopt-qa",
        "operator",
    )
    .await;
    assert_eq!(adopted.status, 200, "{}", adopted.body);
    let old = f.world.directory.path().join("old-superproject");
    std::fs::create_dir(&old).unwrap();
    let old = std::fs::canonicalize(old).unwrap();
    let db = rusqlite::Connection::open(f.world.directory.path().join("kontor.db")).unwrap();
    db.execute("UPDATE topology_node_containers SET canonical_cwd=?1, revision=revision+1 WHERE topology_node_id=?2",
        rusqlite::params![old.to_str().unwrap(),f.node.to_string()]).unwrap();
    f.world.fake.drift_container(
        f.node,
        FakeContainerDrift::Root(WorkspaceRoot::parse(old.to_str().unwrap()).unwrap()),
    );
    let binding = container(&f);
    (f, binding)
}

async fn release_materialized(f: &SeatFillWorld, archive: bool) {
    for seat in f
        .world
        .daemon
        .state()
        .with_store(|s| s.list_seat_bindings(f.project, f.node).unwrap())
    {
        let Some(run) = f
            .members()
            .into_iter()
            .find(|r| r.role == *seat.role_slot_id.as_role_key() && r.binding.is_some())
        else {
            continue;
        };
        let retired = Call::post(format!("/v1/projects/{}/seat-bindings/{}/retire", f.project, seat.id),
            &serde_json::json!({"expected_revision":seat.revision,"reason":"Explicitly release the fixture's completed predecessor, retaining its history"}))
            .signed_as(&f.world,"operator").with_key(format!("root-retire-{}",seat.id)).send(&f.world).await;
        assert_eq!(retired.status, 200, "{}", retired.body);
        if archive {
            f.world
                .fake
                .archive_persistent_seat(&run.binding.unwrap().identity.native_id);
        }
    }
}

#[tokio::test]
async fn corrected_root_recovery_preserves_the_existing_queued_qa_and_team() {
    let (f, old) = misplaced().await;
    release_materialized(&f, true).await;
    f.world.fake.forget_container(f.node);
    let before = f.members();
    let seats = f
        .world
        .daemon
        .state()
        .with_store(|s| s.list_seat_bindings(f.project, f.node).unwrap());
    let claim = f
        .world
        .daemon
        .state()
        .with_store(|s| s.task_worktree(f.project, f.task).unwrap().unwrap());
    let cwd = std::fs::canonicalize(claim.as_str()).unwrap();
    let creates = f.world.container_creates();
    let p = preview(&f).await;
    assert_eq!(p.status, 200, "{}", p.body);
    assert_eq!(
        p.json()["canonical_cwd"],
        cwd.to_str().unwrap(),
        "recovery must use the current task claim"
    );
    assert_eq!(
        f.world.container_creates(),
        creates,
        "preview creates no native"
    );
    assert_eq!(container(&f), old, "preview cannot rewrite the old binding");
    let result = apply(&f, &p.json()["preview_hash"], "root-recover").await;
    assert_eq!(result.status, 200, "{}", result.body);
    assert_eq!(f.world.container_creates(), creates + 1);
    let replacement = container(&f);
    assert_eq!(replacement.container_binding_id, old.container_binding_id);
    assert_eq!(replacement.topology_node_id, old.topology_node_id);
    assert_ne!(replacement.identity.native_id, old.identity.native_id);
    assert_eq!(
        replacement.canonical_cwd.unwrap().as_str(),
        cwd.to_str().unwrap()
    );
    assert_eq!(
        f.members(),
        before,
        "recovery cannot replace historical runs or queued QA"
    );
    assert_eq!(
        f.world
            .daemon
            .state()
            .with_store(|s| s.list_seat_bindings(f.project, f.node).unwrap()),
        seats
    );
    let replay = apply(&f, &p.json()["preview_hash"], "root-recover").await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(
        replay.json()["receipt"]["receipt_id"],
        result.json()["receipt"]["receipt_id"]
    );
    assert_eq!(f.world.container_creates(), creates + 1);
    let before_fill = f.world.fake.calls().len();
    f.world
        .fake
        .allowing_launch_of(&RoleSlotId::parse("verify").unwrap());
    let filled = f
        .fill("verify", f.task_revision(), "root-fill-existing-qa")
        .await;
    assert_eq!(filled.status, 200, "{}", filled.body);
    assert_eq!(
        filled.json()["agent_run_id"],
        before
            .iter()
            .find(|r| r.role.as_str() == "verify")
            .unwrap()
            .id
            .to_string()
    );
    assert_eq!(f.members().len(), before.len());
    let queued_qa = AgentRunId::parse(filled.json()["agent_run_id"].as_str().unwrap()).unwrap();
    assert!(
        f.world
            .fake
            .launched_prompt(queued_qa)
            .unwrap()
            .as_str()
            .starts_with("wait:")
    );
    assert!(
        !f.world.fake.calls()[before_fill..]
            .iter()
            .any(|call| matches!(call, AdapterCall::Send(..)))
    );
    assert!(
        filled.json()["dispatches"].as_array().unwrap().is_empty(),
        "no fabricated handoff"
    );
}

#[tokio::test]
async fn corrected_root_recovery_refuses_live_native_and_wrong_root_with_membership_preserved() {
    for replace_native in [false, true] {
        let (f, old) = misplaced().await;
        release_materialized(&f, true).await;
        if replace_native {
            f.world.fake.drift_container(
                f.node,
                FakeContainerDrift::NativeId(
                    ExternalId::parse("other-native-at-old-root").unwrap(),
                ),
            );
        }
        let members = f.members();
        let count = seats_on(&f.world, f.project, f.node);
        let creates = f.world.container_creates();
        let p = preview(&f).await;
        let expected_status = if replace_native { 422 } else { 409 };
        assert_eq!(p.status, expected_status, "{}", p.body);
        let a = apply(&f, &serde_json::json!("0".repeat(64)), "root-live-refusal").await;
        assert_eq!(a.status, expected_status, "{}", a.body);
        assert_eq!(container(&f), old);
        assert_eq!(f.world.container_creates(), creates);
        assert_eq!(seats_on(&f.world, f.project, f.node), count);
        assert_eq!(f.members(), members);
        assert_eq!(
            adoption_rows(&f, "root-live-refusal").1,
            0,
            "refusal consumes no key"
        );
    }
}

#[tokio::test]
async fn corrected_root_recovery_requires_release_and_positive_archive_of_every_bound_seat() {
    for state in ["unreleased", "live", "missing", "last_live"] {
        let (f, old) = misplaced().await;
        if state != "unreleased" {
            release_materialized(&f, false).await;
        }
        let members = f.members();
        for run in &members {
            if let Some(binding) = &run.binding {
                match state {
                    "unreleased" => f
                        .world
                        .fake
                        .archive_persistent_seat(&binding.identity.native_id),
                    "missing" => f.world.fake.forget_seat(&binding.identity.native_id),
                    "last_live" if run.role.as_str() == "scope" => f
                        .world
                        .fake
                        .archive_persistent_seat(&binding.identity.native_id),
                    _ => {}
                }
            }
        }
        f.world.fake.forget_container(f.node);
        let creates = f.world.container_creates();
        let p = preview(&f).await;
        assert_eq!(p.status, 409, "{state}: {}", p.body);
        assert!(
            p.body.contains(if state == "unreleased" {
                "explicit release"
            } else {
                "archived readback"
            }),
            "{}",
            p.body
        );
        assert_eq!(f.world.container_creates(), creates);
        assert_eq!(container(&f), old);
        assert_eq!(f.members(), members);
    }
}

#[tokio::test]
async fn corrected_root_recovery_freezes_task_revision_claim_canonical_root_and_old_binding() {
    for drift in [
        "task_revision",
        "claim_alias",
        "canonical_root",
        "old_binding",
    ] {
        let (f, _) = misplaced().await;
        release_materialized(&f, true).await;
        f.world.fake.forget_container(f.node);
        let db = rusqlite::Connection::open(f.world.directory.path().join("kontor.db")).unwrap();
        let claim = f
            .world
            .daemon
            .state()
            .with_store(|s| s.task_worktree(f.project, f.task).unwrap().unwrap());
        let alias = f.world.directory.path().join("task-claim");
        std::os::unix::fs::symlink(claim.as_str(), &alias).unwrap();
        if drift == "canonical_root" {
            db.execute(
                "UPDATE task_worktrees SET worktree=?1 WHERE task_id=?2",
                rusqlite::params![alias.to_str().unwrap(), f.task.to_string()],
            )
            .unwrap();
        }
        let p = preview(&f).await;
        assert_eq!(p.status, 200, "{}", p.body);
        let creates = f.world.container_creates();
        match drift {
            "task_revision" => {
                db.execute(
                    "UPDATE tasks SET revision=revision+1 WHERE id=?1",
                    [f.task.to_string()],
                )
                .unwrap();
            }
            "claim_alias" => {
                db.execute(
                    "UPDATE task_worktrees SET worktree=?1 WHERE task_id=?2",
                    rusqlite::params![alias.to_str().unwrap(), f.task.to_string()],
                )
                .unwrap();
            }
            "canonical_root" => {
                std::fs::remove_file(&alias).unwrap();
                let other = f.world.directory.path().join("moved-task-root");
                std::fs::create_dir(&other).unwrap();
                std::os::unix::fs::symlink(other, &alias).unwrap();
            }
            "old_binding" => {
                db.execute("UPDATE topology_node_containers SET canonical_cwd='/tmp/different-old-root' WHERE topology_node_id=?1",[f.node.to_string()]).unwrap();
            }
            _ => unreachable!(),
        }
        let old = container(&f);
        let a = apply(&f, &p.json()["preview_hash"], "root-stale-preview").await;
        assert_eq!(a.status, 409, "{drift}: {}", a.body);
        assert_eq!(a.json()["code"], "revision_conflict");
        assert_eq!(
            f.world.container_creates(),
            creates,
            "{drift}: no create after stale authority"
        );
        assert_eq!(container(&f), old);
        assert_eq!(adoption_rows(&f, "root-stale-preview").1, 0);
    }
}

#[tokio::test]
async fn corrected_root_recovery_adopts_one_unacknowledged_candidate_at_the_current_claim() {
    let (f, old) = misplaced().await;
    release_materialized(&f, true).await;
    let claim = f
        .world
        .daemon
        .state()
        .with_store(|s| s.task_worktree(f.project, f.task).unwrap().unwrap());
    let root = std::fs::canonicalize(claim.as_str()).unwrap();
    // Obtain the rendered title through an honest vacant preview, then stage
    // the successful create whose acknowledgement was lost before binding.
    f.world.fake.forget_container(f.node);
    let vacant = preview(&f).await;
    assert_eq!(vacant.status, 200, "{}", vacant.body);
    f.world.fake.seed_container(
        f.node,
        kontor_runtime::container::ContainerBindingId::parse(old.container_binding_id.as_str())
            .unwrap(),
        ExternalId::parse("recreated-at-correct-task-root").unwrap(),
        WorkspaceRoot::parse(root.to_str().unwrap()).unwrap(),
        vacant.json()["observed_title"].as_str().unwrap(),
        kontor_api::now(),
    );
    let creates = f.world.container_creates();
    let p = preview(&f).await;
    assert_eq!(p.status, 200, "{}", p.body);
    assert_eq!(p.json()["disposition"], "adopt_existing");
    assert_eq!(p.json()["canonical_cwd"], root.to_str().unwrap());
    let a = apply(&f, &p.json()["preview_hash"], "root-adopt-lost-ack").await;
    assert_eq!(a.status, 200, "{}", a.body);
    assert_eq!(
        container(&f).identity.native_id.as_str(),
        "recreated-at-correct-task-root"
    );
    assert_eq!(
        f.world.container_creates(),
        creates,
        "adopt without creating another native"
    );
}

/// A task with a module and no live run, placed and materialized, whose
/// persisted container binding still names an older directory than the claim.
async fn claim_recovery_race_fixture() -> (World, ProjectId, TaskId, TopologyNodeId) {
    let world = World::open_empty_with_a_plane().await;
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let created = ensure_project(
        &world,
        "claim-recovery-race",
        "Claim recovery race",
        "/tmp/kontor-claim-recovery-race",
    )
    .await;
    let project = ProjectId::parse(created.json()["project_id"].as_str().expect("project"))
        .expect("a canonical project id");
    let revision = created.json()["revision"].as_u64().expect("revision");
    let category = first_category(&world).await;
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            revision,
            "Claim recovery race",
            &category,
            serde_json::json!([{
                "title": "Moved container",
                "module": "_tools/asma-rs-kontor",
            }]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key("claim-race-epic")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"].as_str().expect("epic").to_owned();
    let task = TaskId::parse(
        applied.json()["tasks"][0]["task_id"]
            .as_str()
            .expect("task"),
    )
    .expect("a canonical task id");
    confirm_test_epic_identity(&world, &project.to_string(), &epic);
    materialize_test_provider_topology(
        &world,
        &project.to_string(),
        revision,
        &epic,
        &task.to_string(),
        "claim-race",
    )
    .await;
    let node = world.daemon.state().with_store(|store| {
        store
            .get_task_topology_node(project, task)
            .expect("node reads")
            .expect("the task has a topology node")
            .id
    });
    (world, project, task, node)
}

/// A worktree-claim correction cannot overtake a recovery that froze the claim.
///
/// The recovery reads the claim, then builds a native at it. If a correction
/// could land between those two steps, the recovery would successfully bind a
/// directory the claim no longer names. Both writers now take the same native
/// lifecycle lock, so the correction waits here while the recovery is held at
/// its create, and applies only after the recovery's result is durable.
#[tokio::test]
async fn a_worktree_claim_correction_waits_for_a_container_recovery_in_flight() {
    let (world, project, task, node) = claim_recovery_race_fixture().await;
    let claim = world
        .daemon
        .state()
        .with_store(|store| store.task_worktree(project, task))
        .expect("claim reads")
        .expect("the task claims a worktree");
    let task_revision = world
        .daemon
        .state()
        .with_store(|store| store.get_task(project, task))
        .expect("task reads")
        .expect("task exists")
        .revision;
    let jira_key = world
        .daemon
        .state()
        .with_store(|store| store.confirmed_jira_task_key(project, task))
        .expect("Jira binding reads")
        .expect("the task has a confirmed Jira key");
    let corrected = format!(
        "/tmp/kontor-claim-recovery-race/.worktrees/{}/asma-rs-kontor",
        jira_key.as_str().to_ascii_lowercase()
    );

    // The binding names a directory the task no longer claims, and the native
    // is gone, so recovery is a positively vacant recreation at the claim it
    // freezes.
    let previous = world.directory.path().join("previous-container-root");
    std::fs::create_dir(&previous).expect("the previous root exists");
    let previous = std::fs::canonicalize(&previous).expect("the previous root canonicalizes");
    let db = rusqlite::Connection::open(world.directory.path().join("kontor.db")).expect("db");
    db.execute(
        "UPDATE topology_node_containers SET canonical_cwd=?1, revision=revision+1 WHERE topology_node_id=?2",
        rusqlite::params![previous.to_str().expect("utf-8"), node.to_string()],
    )
    .expect("the stale binding moves");
    world.fake.forget_container(node);

    let project_revision = world
        .daemon
        .state()
        .with_store(|store| store.get_project(project))
        .expect("project reads")
        .expect("project exists")
        .revision;
    let preview = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["disposition"], "recreate_absent");
    assert_eq!(
        preview.json()["canonical_cwd"],
        std::fs::canonicalize(claim.as_str())
            .expect("the claim canonicalizes")
            .to_str()
            .expect("utf-8")
    );

    let correction_uri = format!("/v1/projects/{project}/tasks/{task}/worktree-claim");
    let correction_body = serde_json::json!({
        "expected_revision": task_revision,
        "old_worktree": claim.as_str(),
        "new_worktree": corrected,
    });
    let correction_preview = Call::post(format!("{correction_uri}:preview"), &correction_body)
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(
        correction_preview.status, 200,
        "{}",
        correction_preview.body
    );
    assert_eq!(correction_preview.json()["new_worktree"], corrected);
    let mut correction_apply = correction_body.clone();
    correction_apply["preview_hash"] = correction_preview.json()["preview_hash"].clone();

    let pause = world.fake.pause_next_container_create();
    let recovery = async {
        Call::post(
            format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-apply"),
            &serde_json::json!({
                "expected_revision": project_revision,
                "preview_hash": preview.json()["preview_hash"],
            }),
        )
        .signed_as(&world, "admin")
        .with_key("claim-race-recovery")
        .send(&world)
        .await
    };
    let correction = async {
        pause.entered().await;
        Call::post(format!("{correction_uri}:apply"), &correction_apply)
            .signed_as(&world, "operator")
            .with_key("claim-race-correction")
            .send(&world)
            .await
    };
    let release = async {
        pause.entered().await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let held = world
            .daemon
            .state()
            .with_store(|store| store.task_worktree(project, task))
            .expect("claim reads")
            .expect("the task still claims its worktree");
        assert_eq!(
            held, claim,
            "the correction changed the claim while the recovery held the native lifecycle lock"
        );
        pause.release();
    };
    let (recovered, corrected_answer, ()) = tokio::join!(recovery, correction, release);
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(corrected_answer.status, 200, "{}", corrected_answer.body);

    let bound = world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(project, node))
        .expect("binding reads")
        .expect("the node is bound");
    assert_eq!(
        bound
            .canonical_cwd
            .expect("the replacement names its root")
            .as_str(),
        std::fs::canonicalize(claim.as_str())
            .expect("the claim canonicalizes")
            .to_str()
            .expect("utf-8")
    );
    let stored = world
        .daemon
        .state()
        .with_store(|store| store.task_worktree(project, task))
        .expect("claim reads")
        .expect("the task claims a worktree");
    assert_eq!(stored.as_str(), corrected);
}

/// An epic re-apply cannot overtake a recovery that froze the task's claim.
///
/// `epics:apply` rewrites the declared worktree of an existing task. If that
/// write could land between a recovery reading the claim and creating the
/// native at it, the recovery would successfully bind a directory the claim no
/// longer names. Apply now takes the same native lifecycle lock the recovery
/// holds, so it waits here while the recovery is held at its create, and writes
/// only after the recovery's result is durable.
#[tokio::test]
async fn an_epic_apply_waits_for_a_container_recovery_in_flight() {
    let (world, project, task, node) = claim_recovery_race_fixture().await;
    let claim = world
        .daemon
        .state()
        .with_store(|store| store.task_worktree(project, task))
        .expect("claim reads")
        .expect("the task claims a worktree");
    let jira_key = world
        .daemon
        .state()
        .with_store(|store| store.confirmed_jira_task_key(project, task))
        .expect("Jira binding reads")
        .expect("the task has a confirmed Jira key");
    let corrected = format!(
        "/tmp/kontor-claim-recovery-race/.worktrees/{}/asma-rs-kontor",
        jira_key.as_str().to_ascii_lowercase()
    );

    // The binding names a directory the task no longer claims, and the native
    // is gone, so recovery is a positively vacant recreation at the claim it
    // freezes.
    let previous = world.directory.path().join("previous-container-root");
    std::fs::create_dir(&previous).expect("the previous root exists");
    let previous = std::fs::canonicalize(&previous).expect("the previous root canonicalizes");
    let db = rusqlite::Connection::open(world.directory.path().join("kontor.db")).expect("db");
    db.execute(
        "UPDATE topology_node_containers SET canonical_cwd=?1, revision=revision+1 WHERE topology_node_id=?2",
        rusqlite::params![previous.to_str().expect("utf-8"), node.to_string()],
    )
    .expect("the stale binding moves");
    world.fake.forget_container(node);

    let project_revision = world
        .daemon
        .state()
        .with_store(|store| store.get_project(project))
        .expect("project reads")
        .expect("project exists")
        .revision;
    let preview = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["disposition"], "recreate_absent");
    assert_eq!(
        preview.json()["canonical_cwd"],
        std::fs::canonicalize(claim.as_str())
            .expect("the claim canonicalizes")
            .to_str()
            .expect("utf-8")
    );

    let category = first_category(&world).await;
    let reapply_body = epic_body(
        project_revision.get(),
        "Claim recovery race",
        &category,
        serde_json::json!([{
            "title": "Moved container",
            "module": "_tools/asma-rs-kontor",
            "worktree": corrected,
        }]),
    );

    let pause = world.fake.pause_next_container_create();
    let recovery = async {
        Call::post(
            format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-apply"),
            &serde_json::json!({
                "expected_revision": project_revision,
                "preview_hash": preview.json()["preview_hash"],
            }),
        )
        .signed_as(&world, "admin")
        .with_key("claim-race-epic-recovery")
        .send(&world)
        .await
    };
    let reapply = async {
        pause.entered().await;
        Call::post(format!("/v1/projects/{project}/epics:apply"), &reapply_body)
            .signed_as(&world, "admin")
            .with_key("claim-race-epic-reapply")
            .send(&world)
            .await
    };
    let release = async {
        pause.entered().await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let held = world
            .daemon
            .state()
            .with_store(|store| store.task_worktree(project, task))
            .expect("claim reads")
            .expect("the task still claims its worktree");
        assert_eq!(
            held, claim,
            "the epic apply changed the claim while the recovery held the native lifecycle lock"
        );
        pause.release();
    };
    let (recovered, reapplied, ()) = tokio::join!(recovery, reapply, release);
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(reapplied.status, 200, "{}", reapplied.body);

    let bound = world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(project, node))
        .expect("binding reads")
        .expect("the node is bound");
    assert_eq!(
        bound
            .canonical_cwd
            .expect("the replacement names its root")
            .as_str(),
        std::fs::canonicalize(claim.as_str())
            .expect("the claim canonicalizes")
            .to_str()
            .expect("utf-8")
    );
    let stored = world
        .daemon
        .state()
        .with_store(|store| store.task_worktree(project, task))
        .expect("claim reads")
        .expect("the task claims a worktree");
    assert_eq!(stored.as_str(), corrected);
}

/// An unchanged epic re-apply does not queue behind a container recovery.
///
/// Only a write that moves an existing task's worktree claim can race a
/// recovery's frozen placement, so only that write takes the native lifecycle
/// lock. A queued writer refuses every new native operation in the realm until
/// it gets its turn, and an apply that declares the claim it already stores
/// must not become that writer. Here the recovery is held at its create while
/// the unchanged re-apply reaches governance and is refused there without
/// waiting for the recovery's lock.
#[tokio::test]
async fn an_unchanged_epic_apply_does_not_wait_for_a_container_recovery_in_flight() {
    let (world, project, task, node) = claim_recovery_race_fixture().await;
    let claim = world
        .daemon
        .state()
        .with_store(|store| store.task_worktree(project, task))
        .expect("claim reads")
        .expect("the task claims a worktree");

    // The binding names a directory the task no longer claims, and the native
    // is gone, so recovery is a positively vacant recreation at the claim it
    // freezes.
    let previous = world.directory.path().join("previous-container-root");
    std::fs::create_dir(&previous).expect("the previous root exists");
    let previous = std::fs::canonicalize(&previous).expect("the previous root canonicalizes");
    let db = rusqlite::Connection::open(world.directory.path().join("kontor.db")).expect("db");
    db.execute(
        "UPDATE topology_node_containers SET canonical_cwd=?1, revision=revision+1 WHERE topology_node_id=?2",
        rusqlite::params![previous.to_str().expect("utf-8"), node.to_string()],
    )
    .expect("the stale binding moves");
    world.fake.forget_container(node);

    let project_revision = world
        .daemon
        .state()
        .with_store(|store| store.get_project(project))
        .expect("project reads")
        .expect("project exists")
        .revision;
    let preview = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["disposition"], "recreate_absent");

    let category = first_category(&world).await;
    let unchanged = epic_body(
        project_revision.get(),
        "Claim recovery race",
        &category,
        serde_json::json!([{
            "title": "Moved container",
            "module": "_tools/asma-rs-kontor",
        }]),
    );

    let pause = world.fake.pause_next_container_create();
    let recovery = async {
        Call::post(
            format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-apply"),
            &serde_json::json!({
                "expected_revision": project_revision,
                "preview_hash": preview.json()["preview_hash"],
            }),
        )
        .signed_as(&world, "admin")
        .with_key("unchanged-apply-recovery")
        .send(&world)
        .await
    };
    let reapply = async {
        pause.entered().await;
        Call::post(format!("/v1/projects/{project}/epics:apply"), &unchanged)
            .signed_as(&world, "admin")
            .with_key("unchanged-epic-reapply")
            .send(&world)
            .await
    };
    let release = async {
        pause.entered().await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let held = world
            .daemon
            .state()
            .with_store(|store| store.task_worktree(project, task))
            .expect("claim reads")
            .expect("the task still claims its worktree");
        assert_eq!(held, claim, "the unchanged apply moved the claim");
        pause.release();
    };
    let (recovered, reapplied, ()) = tokio::join!(recovery, reapply, release);
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(
        reapplied.status, 409,
        "the unchanged apply waited for the recovery instead of reaching governance: {}",
        reapplied.body
    );
    assert_eq!(reapplied.code(), "placement_blocked");
    let stored = world
        .daemon
        .state()
        .with_store(|store| store.task_worktree(project, task))
        .expect("claim reads")
        .expect("the task claims a worktree");
    assert_eq!(stored, claim);
}

/// A brand-new epic apply does not queue behind a container recovery either.
///
/// A new epic writes no task claim, so it has no claim a recovery could have
/// frozen and must not become the queued native lifecycle writer. While the
/// recovery is held at its create, the apply reaches governance and is refused
/// there; an apply that always took the lock would wait for the recovery and
/// answer `200` once it finished instead.
#[tokio::test]
async fn a_new_epic_apply_does_not_wait_for_a_container_recovery_in_flight() {
    let (world, project, _task, node) = claim_recovery_race_fixture().await;

    // The binding names a directory the task no longer claims, and the native
    // is gone, so recovery is a positively vacant recreation at the claim it
    // freezes.
    let previous = world.directory.path().join("previous-container-root");
    std::fs::create_dir(&previous).expect("the previous root exists");
    let previous = std::fs::canonicalize(&previous).expect("the previous root canonicalizes");
    let db = rusqlite::Connection::open(world.directory.path().join("kontor.db")).expect("db");
    db.execute(
        "UPDATE topology_node_containers SET canonical_cwd=?1, revision=revision+1 WHERE topology_node_id=?2",
        rusqlite::params![previous.to_str().expect("utf-8"), node.to_string()],
    )
    .expect("the stale binding moves");
    world.fake.forget_container(node);

    let project_revision = world
        .daemon
        .state()
        .with_store(|store| store.get_project(project))
        .expect("project reads")
        .expect("project exists")
        .revision;
    let preview = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-preview"),
        &serde_json::json!({"expected_revision": project_revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(preview.json()["disposition"], "recreate_absent");

    let category = first_category(&world).await;
    let fresh = epic_body(
        project_revision.get(),
        "Fresh epic apply race",
        &category,
        serde_json::json!([{
            "title": "Fresh task",
            "module": "_tools/asma-rs-kontor",
        }]),
    );

    let pause = world.fake.pause_next_container_create();
    let recovery = async {
        Call::post(
            format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-apply"),
            &serde_json::json!({
                "expected_revision": project_revision,
                "preview_hash": preview.json()["preview_hash"],
            }),
        )
        .signed_as(&world, "admin")
        .with_key("fresh-epic-recovery")
        .send(&world)
        .await
    };
    let apply = async {
        pause.entered().await;
        Call::post(format!("/v1/projects/{project}/epics:apply"), &fresh)
            .signed_as(&world, "admin")
            .with_key("fresh-epic-apply")
            .send(&world)
            .await
    };
    let release = async {
        pause.entered().await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        pause.release();
    };
    let (recovered, applied, ()) = tokio::join!(recovery, apply, release);
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(
        applied.status, 409,
        "the fresh apply waited for the recovery instead of reaching governance: {}",
        applied.body
    );
    assert_eq!(applied.code(), "placement_blocked");
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.list_mini_projects(project))
            .expect("epics read")
            .iter()
            .any(|epic| epic.name.as_str() == "Fresh epic apply race"),
        "the fresh apply wrote no epic before governance refused it"
    );
}
