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
