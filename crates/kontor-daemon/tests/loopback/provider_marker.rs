//! The provider-marker correction, over the real router and a real state root.
//!
//! The correction is the one supported way to delete a stale
//! `unavailable_providers` entry from `<state-root>/runtimes.json`. What these
//! tests exist to prove, besides the before/after and the refusals, is the
//! boundary: the operation writes that one member and nothing else — no runtime
//! call, no run, no binding, no topology node.

use super::*;

/// A fleet document an operator could have written.
///
/// It carries a member this build does not know (`future_member`) and the one
/// sensitive field (`host_target`), so a test can prove the correction preserves
/// both.
fn marker_document() -> String {
    serde_json::json!({
        "schema_version": 5,
        "runtimes": [
            {
                "family": "paseo",
                "runtime_kind": fake_family().to_string(),
                "host_key": "paseo-host",
                "future_member": { "kept": true },
                "mini_project_id": "01890000-0000-7000-8000-000000000001",
                "jira_epic_key": "ASMA-7968",
                "mini_project_short_title": "Provider marker",
                "plan_item_key": "KON-OP-20",
                "jira_issue_key": "ASMA-7968",
                "ticket_short_code": "KON-OP-20",
                "seat_display_roles": { "implement": { "role": "Implement" } },
                "project_root_cwd": "/w/epic",
                "canonical_worktree_cwd": "/w/task",
                "orchestrator_agent_id": "agent-1",
                "max_concurrent_sessions": 2,
                "unavailable_providers": ["claude", "codex"],
                "executable": "paseo",
                "host_target": "https://user:hunter2@paseo.example",
                "endpoint": "ws://127.0.0.1:6767/ws",
                "client_id": "kontor-mini-1",
                "timeout_seconds": 30
            }
        ]
    })
    .to_string()
}

fn fixture(world: &World) -> std::path::PathBuf {
    let path = world.directory.path().join("runtimes.json");
    std::fs::write(&path, marker_document()).expect("the fixture document is installed");
    path
}

fn preview_body() -> serde_json::Value {
    serde_json::json!({
        "runtime_family": fake_family().to_string(),
        "provider": "claude",
    })
}

#[tokio::test]
async fn a_marker_correction_is_previewed_applied_replayed_and_moves_no_seat() {
    let world = World::open().await;
    let path = fixture(&world);
    // The running daemon still holds the marker, exactly as an outage left it.
    world.fake.provider_outage("claude", None);
    let (run, _) = world.launch().await;
    let calls = world.fake.calls().len();
    let (task_before, team_before, run_before, nodes_before) =
        world.daemon.state().with_store(|store| {
            (
                store.get_task(world.project, world.task).unwrap(),
                store.get_team_run(world.project, world.team_run).unwrap(),
                store.get_agent_run(world.project, run).unwrap(),
                store.list_topology_nodes(world.project, None).unwrap(),
            )
        });

    let preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let preview_json = preview.json();
    assert_eq!(
        preview_json["before"],
        serde_json::json!(["claude", "codex"]),
        "before is the document's own set"
    );
    assert_eq!(preview_json["after"], serde_json::json!(["codex"]));
    assert_eq!(preview_json["changed"], true);
    assert_eq!(preview_json["host_key"], "paseo-host");
    assert_eq!(
        preview_json["effects"],
        serde_json::json!(["marker_removed", "restart_required"]),
        "the running daemon still holds the marker, so a start is still owed"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "a preview rewrites nothing"
    );
    let revision = preview_json["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview_json["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();

    let applied = Call::post(
        "/v1/provider-markers:apply",
        &serde_json::json!({
            "runtime_family": fake_family().to_string(),
            "provider": "claude",
            "expected_revision": revision,
            "preview_hash": preview_hash,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("provider-marker-clear")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let applied_json = applied.json();
    assert_eq!(applied_json["outcome"], "cleared");
    assert_eq!(applied_json["changed"], true);
    assert_eq!(
        applied_json["before"],
        serde_json::json!(["claude", "codex"])
    );
    assert_eq!(applied_json["after"], serde_json::json!(["codex"]));
    assert_ne!(
        applied_json["revision"].as_str().expect("a revision"),
        revision,
        "the applied document is a new revision"
    );

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("the document reads"))
            .expect("the corrected document is JSON");
    assert_eq!(
        written["runtimes"][0]["unavailable_providers"],
        serde_json::json!(["codex"])
    );
    assert_eq!(
        written["runtimes"][0]["host_target"], "https://user:hunter2@paseo.example",
        "the one sensitive field is preserved untouched"
    );
    assert_eq!(
        written["runtimes"][0]["future_member"],
        serde_json::json!({ "kept": true }),
        "a member this build does not know survives the correction"
    );

    // The same key is the same act: answered from the durable binding, with no
    // second write, even though its revision is now stale.
    let corrected = std::fs::read_to_string(&path).expect("the document reads");
    let replayed = Call::post(
        "/v1/provider-markers:apply",
        &serde_json::json!({
            "runtime_family": fake_family().to_string(),
            "provider": "claude",
            "expected_revision": revision,
            "preview_hash": preview_hash,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("provider-marker-clear")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["outcome"], "replayed");
    assert_eq!(replayed.json()["changed"], false);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        corrected,
        "a replay writes nothing"
    );

    // The other provider is still marked, and a fresh preview corrects it.
    let second = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(second.json()["changed"], false);
    assert_eq!(second.json()["before"], serde_json::json!(["codex"]));
    assert_eq!(second.json()["after"], serde_json::json!(["codex"]));

    // Nothing about the realm's work moved: the correction reached no runtime,
    // retired no seat, replaced no run and touched no topology node.
    assert_eq!(
        world.fake.calls().len(),
        calls,
        "a configuration correction must not reach the runtime"
    );
    world.daemon.state().with_store(|store| {
        assert_eq!(
            store.get_task(world.project, world.task).unwrap(),
            task_before
        );
        assert_eq!(
            store.get_team_run(world.project, world.team_run).unwrap(),
            team_before
        );
        assert_eq!(store.get_agent_run(world.project, run).unwrap(), run_before);
        assert_eq!(
            store.list_topology_nodes(world.project, None).unwrap(),
            nodes_before
        );
    });
}

#[tokio::test]
async fn a_completed_correction_does_not_remove_a_marker_restored_for_a_later_outage() {
    let world = World::open().await;
    let path = fixture(&world);
    let preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": fake_family().to_string(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });
    let cleared = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-restored")
        .send(&world)
        .await;
    assert_eq!(cleared.status, 200, "{}", cleared.body);
    assert_eq!(cleared.json()["outcome"], "cleared");

    // The outage marker comes back: an operator restores the exact document the
    // completed correction read. This is a new fact about the realm, not a
    // retry of the correction, and it must survive the retry.
    std::fs::write(&path, marker_document()).expect("the operator restores the marker");

    let replayed = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-restored")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["outcome"], "replayed");
    assert_eq!(replayed.json()["changed"], false);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "a completed key never performs its write a second time"
    );
}

#[tokio::test]
async fn a_completed_correction_replays_after_its_setting_and_document_are_removed() {
    let world = World::open().await;
    let path = fixture(&world);
    let preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": fake_family().to_string(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });
    let cleared = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-completed-gone")
        .send(&world)
        .await;
    assert_eq!(cleared.status, 200, "{}", cleared.body);
    assert_eq!(cleared.json()["outcome"], "cleared");

    // The operator removes the addressed setting after the correction
    // completed. The key is still the same completed act, so its retry is
    // answered from the durable completion, not from the moved document.
    std::fs::write(
        &path,
        serde_json::json!({ "schema_version": 5, "runtimes": [] }).to_string(),
    )
    .expect("the addressed setting is removed");
    let setting_gone = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-completed-gone")
        .send(&world)
        .await;
    assert_eq!(setting_gone.status, 200, "{}", setting_gone.body);
    assert_eq!(setting_gone.json()["outcome"], "replayed");
    assert_eq!(setting_gone.json()["changed"], false);
    assert_eq!(
        setting_gone.json()["before"],
        serde_json::json!(["claude", "codex"])
    );
    assert_eq!(setting_gone.json()["after"], serde_json::json!(["codex"]));

    // The whole document is gone too. A completed key is still `replayed`,
    // which is exactly what the documentation promises a retry.
    std::fs::remove_file(&path).expect("the document is removed");
    let document_gone = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-completed-gone")
        .send(&world)
        .await;
    assert_eq!(document_gone.status, 200, "{}", document_gone.body);
    assert_eq!(document_gone.json()["outcome"], "replayed");
    assert_eq!(document_gone.json()["changed"], false);
    assert_eq!(
        document_gone.json()["revision"],
        cleared.json()["revision"],
        "the replay names the revision the correction left behind"
    );

    // A fresh preview still reports the missing document as not found.
    let fresh = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(fresh.status, 404, "{}", fresh.body);
    assert_eq!(fresh.code(), "not_found");
}

#[tokio::test]
async fn a_marker_correction_refuses_unknown_stale_malformed_and_unauthorized_calls() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    let preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply_body = |revision: &str, preview_hash: &str| {
        serde_json::json!({
            "runtime_family": family.clone(),
            "provider": "claude",
            "expected_revision": revision,
            "preview_hash": preview_hash,
        })
    };

    // A configuration route is admin work, and an operator reaches nothing.
    let refused = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&revision, &preview_hash),
    )
    .signed_as(&world, "operator")
    .with_key("marker-operator")
    .send(&world)
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);

    // An unknown family is not found on either half of the pair.
    let unknown_preview = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": "paseo.other",
            "provider": "claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(unknown_preview.status, 404, "{}", unknown_preview.body);
    assert_eq!(unknown_preview.code(), "not_found");
    let unknown_apply = Call::post(
        "/v1/provider-markers:apply",
        &serde_json::json!({
            "runtime_family": "paseo.other",
            "provider": "claude",
            "expected_revision": revision,
            "preview_hash": preview_hash,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("marker-unknown")
    .send(&world)
    .await;
    assert_eq!(unknown_apply.status, 404, "{}", unknown_apply.body);
    assert_eq!(unknown_apply.code(), "not_found");

    // A provider name that is not an open key never reaches the document.
    let invalid = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "Claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(invalid.status, 400, "{}", invalid.body);
    assert_eq!(invalid.code(), "invalid_request");

    // A body that is not this route's schema is one typed invalid_request.
    let malformed = Call::post_raw("/v1/provider-markers:preview", "{\"runtime_family\": }")
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(malformed.status, 400, "{}", malformed.body);
    assert_eq!(malformed.code(), "invalid_request");

    // Every mutation must carry a key.
    let keyless = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&revision, &preview_hash),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(keyless.status, 400, "{}", keyless.body);
    assert_eq!(keyless.code(), "invalid_request");

    // A hash that does not describe this correction is refused without writing.
    let wrong_hash = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&revision, &"0".repeat(64)),
    )
    .signed_as(&world, "admin")
    .with_key("marker-wrong-hash")
    .send(&world)
    .await;
    assert_eq!(wrong_hash.status, 400, "{}", wrong_hash.body);
    assert_eq!(wrong_hash.code(), "invalid_request");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "a refused apply rewrites nothing"
    );

    // The document moved after the preview: the stale revision is the conflict.
    std::fs::write(&path, format!("{}\n", marker_document())).expect("a competing edit");
    let stale = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&revision, &preview_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-stale")
    .send(&world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");

    // Restore the original revision, clear the marker once, then prove that a
    // fresh correction of an absent marker is a stable already-cleared, and
    // that replaying *that* key is a stable replayed.
    std::fs::write(&path, marker_document()).expect("the original document is restored");
    let cleared = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&revision, &preview_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-first")
    .send(&world)
    .await;
    assert_eq!(cleared.status, 200, "{}", cleared.body);
    assert_eq!(cleared.json()["outcome"], "cleared");
    let after_clear = std::fs::read_to_string(&path).expect("the document reads");

    let fresh = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(fresh.status, 200, "{}", fresh.body);
    assert_eq!(fresh.json()["changed"], false);
    let fresh_revision = fresh.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let fresh_hash = fresh.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let already = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&fresh_revision, &fresh_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-already")
    .send(&world)
    .await;
    assert_eq!(already.status, 200, "{}", already.body);
    assert_eq!(already.json()["outcome"], "already_cleared");
    assert_eq!(already.json()["changed"], false);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        after_clear
    );
    let replayed = Call::post(
        "/v1/provider-markers:apply",
        &apply_body(&fresh_revision, &fresh_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-already")
    .send(&world)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["outcome"], "replayed");
    assert_eq!(replayed.json()["changed"], false);
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_document_is_not_reported_as_a_document_to_repair() {
    use std::os::unix::fs::PermissionsExt;

    let world = World::open().await;
    let path = fixture(&world);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .expect("the document becomes unreadable");

    // The bytes are never parsed: the read itself failed, so the advice names
    // permissions and disk space rather than asking for a repair.
    let unreadable = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(unreadable.status, 503, "{}", unreadable.body);
    assert_eq!(
        unreadable.json()["rule"],
        "the runtime settings document could not be read"
    );
    let read_action = unreadable.json()["action"]
        .as_str()
        .expect("an action")
        .to_owned();
    assert!(read_action.contains("permissions"), "{read_action}");
    assert!(!read_action.contains("repair"), "{read_action}");

    // A document that is readable but not a fleet description stays the repair
    // case, so the two failures are never conflated.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("the document is readable again");
    std::fs::write(&path, b"{\"schema_version\": 5, \"runtimes\": \"no\"}")
        .expect("a document this build cannot compose");
    let malformed = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(malformed.status, 503, "{}", malformed.body);
    assert_eq!(
        malformed.json()["rule"],
        "the runtime settings document could not be read as a fleet description"
    );
    let malformed_action = malformed.json()["action"]
        .as_str()
        .expect("an action")
        .to_owned();
    assert!(malformed_action.contains("repair"), "{malformed_action}");
}

#[tokio::test]
async fn a_reused_provider_marker_key_is_an_idempotency_conflict_and_writes_nothing() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    let preview = |provider: &str| {
        serde_json::json!({
            "runtime_family": family.clone(),
            "provider": provider,
        })
    };
    let apply = |provider: &str, revision: &str, hash: &str| {
        serde_json::json!({
            "runtime_family": family.clone(),
            "provider": provider,
            "expected_revision": revision,
            "preview_hash": hash,
        })
    };

    let first = Call::post("/v1/provider-markers:preview", &preview("claude"))
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let revision = first.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = first.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let cleared = Call::post(
        "/v1/provider-markers:apply",
        &apply("claude", &revision, &preview_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-conflict")
    .send(&world)
    .await;
    assert_eq!(cleared.status, 200, "{}", cleared.body);
    assert_eq!(cleared.json()["outcome"], "cleared");
    let after_clear = std::fs::read(&path).expect("the document reads");

    // The same key names a different provider, freshly previewed. The key is
    // already bound to the Claude correction, so this is a conflict rather
    // than a second correction under one act.
    let codex = Call::post("/v1/provider-markers:preview", &preview("codex"))
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(codex.status, 200, "{}", codex.body);
    let codex_revision = codex.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let codex_hash = codex.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let different_provider = Call::post(
        "/v1/provider-markers:apply",
        &apply("codex", &codex_revision, &codex_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-conflict")
    .send(&world)
    .await;
    assert_eq!(
        different_provider.status, 409,
        "{}",
        different_provider.body
    );
    assert_eq!(different_provider.code(), "idempotency_conflict");
    assert_eq!(
        std::fs::read(&path).expect("the document reads"),
        after_clear,
        "a conflicting key rewrites nothing"
    );

    // The same key and provider, but a fresh revision and hash after the
    // correction moved the document: the fingerprint differs, so this too is a
    // conflict rather than a replay.
    let fresh = Call::post("/v1/provider-markers:preview", &preview("claude"))
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(fresh.status, 200, "{}", fresh.body);
    assert_eq!(fresh.json()["changed"], false);
    let fresh_revision = fresh.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let fresh_hash = fresh.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    assert_ne!(
        fresh_revision, revision,
        "the correction moved the document"
    );
    let fresh_use = Call::post(
        "/v1/provider-markers:apply",
        &apply("claude", &fresh_revision, &fresh_hash),
    )
    .signed_as(&world, "admin")
    .with_key("marker-conflict")
    .send(&world)
    .await;
    assert_eq!(fresh_use.status, 409, "{}", fresh_use.body);
    assert_eq!(fresh_use.code(), "idempotency_conflict");
    assert_eq!(
        std::fs::read(&path).expect("the document reads"),
        after_clear
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_write_leaves_a_claim_that_a_fresh_preview_and_key_finish() {
    use std::os::unix::fs::symlink;

    let world = World::open().await;
    let path = world.directory.path().join("runtimes.json");
    let outside = tempfile::tempdir().expect("a directory outside the state root");
    std::fs::write(outside.path().join("runtimes.json"), marker_document())
        .expect("the symlink target");
    symlink(outside.path().join("runtimes.json"), &path).expect("the settings path is a symlink");

    let preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": fake_family().to_string(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });

    // The claim is durable before the write, and the write refuses the symlink:
    // the caller sees the refusal while the key already names this correction.
    let refused = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-write-failed")
        .send(&world)
        .await;
    assert_eq!(refused.status, 503, "{}", refused.body);
    assert_eq!(refused.code(), "unavailable");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads through the link"),
        marker_document(),
        "the failed write left the document alone"
    );
    let binding = world
        .daemon
        .state()
        .with_store(|store| store.realm_idempotency_binding("provider-marker-write-failed"))
        .expect("the binding reads")
        .expect("the key was claimed before the write was attempted");
    assert_eq!(
        binding.operation, "clear_provider_marker",
        "the claim precedes the failed write"
    );
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.provider_marker_write_started("provider-marker-write-failed"))
            .expect("the write phase reads"),
        "the key records that its write started"
    );

    // The operator replaces the symlink with the same bytes. The key cannot
    // tell a write that never landed from one whose marker was restored, so it
    // asks for a fresh preview and key instead of rewriting the document.
    std::fs::remove_file(&path).expect("the symlink is removed");
    std::fs::write(&path, marker_document()).expect("the regular document is installed");
    let stale_key = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-write-failed")
        .send(&world)
        .await;
    assert_eq!(stale_key.status, 409, "{}", stale_key.body);
    assert_eq!(stale_key.code(), "revision_conflict");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "the retry left the document alone"
    );

    // The fresh preview names the same revision, and a fresh key finishes.
    let fresh_preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(fresh_preview.status, 200, "{}", fresh_preview.body);
    let fresh_revision = fresh_preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let fresh_hash = fresh_preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let fresh_apply = serde_json::json!({
        "runtime_family": fake_family().to_string(),
        "provider": "claude",
        "expected_revision": fresh_revision,
        "preview_hash": fresh_hash,
    });
    let finished = Call::post("/v1/provider-markers:apply", &fresh_apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-write-finished")
        .send(&world)
        .await;
    assert_eq!(finished.status, 200, "{}", finished.body);
    assert_eq!(finished.json()["outcome"], "cleared");
    assert_eq!(finished.json()["changed"], true);
    let cleared = std::fs::read_to_string(&path).expect("the document reads");
    assert!(
        !cleared.contains("\"claude\""),
        "the fresh key removed the marker"
    );

    let replayed = Call::post("/v1/provider-markers:apply", &fresh_apply)
        .signed_as(&world, "admin")
        .with_key("provider-marker-write-finished")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["outcome"], "replayed");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        cleared,
        "a replay writes nothing"
    );
}

#[tokio::test]
async fn a_replay_that_finds_the_marker_gone_completes_the_key_and_never_reopens_it() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    let preview = Call::post("/v1/provider-markers:preview", &preview_body())
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": family,
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });

    // The first attempt claimed the key and its write landed, but its
    // completion insert failed: the binding stands, the marker is gone and no
    // completion row names the key.
    let fingerprint = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": "provider_marker_apply",
        "runtime_family": fake_family().as_str(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    }))
    .expect("the intent canonicalizes")
    .hash()
    .clone();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.bind_realm_operation(&IdempotencyBinding {
                key: "marker-completion-lost".to_owned(),
                operation: "clear_provider_marker",
                fingerprint,
                bound_at: at("2026-10-02T09:00:00Z"),
            })
        })
        .expect("the first attempt's claim is durable");
    let corrected = marker_document().replace("[\"claude\",\"codex\"]", "[\"codex\"]");
    std::fs::write(&path, &corrected).expect("the first attempt's write landed");
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.provider_marker_completion("marker-completion-lost"))
            .expect("the completion reads"),
        None,
        "the completion insert never landed"
    );

    // The replay answers the effect that already stands and records the
    // completion the first attempt lost.
    let recovered = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("marker-completion-lost")
        .send(&world)
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(recovered.json()["outcome"], "replayed");
    assert_eq!(recovered.json()["changed"], false);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        corrected
    );
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.provider_marker_completion("marker-completion-lost"))
            .expect("the completion reads")
            .is_some(),
        "the recovery replay records the completion the first attempt lost"
    );

    // A later outage restores the exact bytes the first attempt read. The key
    // is complete now, so the restored marker survives the replay.
    std::fs::write(&path, marker_document()).expect("the operator restores the marker");
    let during_new_outage = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("marker-completion-lost")
        .send(&world)
        .await;
    assert_eq!(during_new_outage.status, 200, "{}", during_new_outage.body);
    assert_eq!(during_new_outage.json()["outcome"], "replayed");
    assert_eq!(during_new_outage.json()["changed"], false);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "a key completed by its recovery replay never removes a marker restored for a new outage"
    );
}

#[tokio::test]
async fn a_started_write_without_completion_never_clears_a_restored_marker() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    let preview = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": fake_family().to_string(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });
    // The first attempt claimed the key, recorded that its write started,
    // landed the corrected document and died before recording completion.
    let fingerprint = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": "provider_marker_apply",
        "runtime_family": fake_family().as_str(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    }))
    .expect("the intent canonicalizes")
    .hash()
    .clone();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.bind_realm_operation(&IdempotencyBinding {
                key: "marker-write-started".to_owned(),
                operation: "clear_provider_marker",
                fingerprint,
                bound_at: at("2026-10-02T09:00:00Z"),
            })?;
            store.record_provider_marker_write_start(
                "marker-write-started",
                at("2026-10-02T09:00:01Z"),
            )
        })
        .expect("the first attempt's claim and write phase are durable");
    let corrected = marker_document().replace("[\"claude\",\"codex\"]", "[\"codex\"]");
    std::fs::write(&path, &corrected).expect("the first attempt's write landed");
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.provider_marker_completion("marker-write-started"))
            .expect("the completion reads"),
        None,
        "the completion insert never landed"
    );

    // A new outage restores the marker before the retry. The key cannot tell
    // this from a write that never landed, so it refuses rather than removing
    // the restored marker.
    std::fs::write(&path, marker_document()).expect("the operator restores the marker");
    let retried = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("marker-write-started")
        .send(&world)
        .await;
    assert_eq!(retried.status, 409, "{}", retried.body);
    assert_eq!(retried.code(), "revision_conflict");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "the retry never removes a marker restored for a new outage"
    );
    assert!(
        world
            .daemon
            .state()
            .with_store(|store| store.provider_marker_completion("marker-write-started"))
            .expect("the completion reads")
            .is_none(),
        "the refused retry records no completion"
    );

    // A fresh preview and key are the supported way forward.
    let fresh_preview = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(fresh_preview.status, 200, "{}", fresh_preview.body);
    let fresh_revision = fresh_preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let fresh_hash = fresh_preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let finished = Call::post(
        "/v1/provider-markers:apply",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
            "expected_revision": fresh_revision,
            "preview_hash": fresh_hash,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("marker-write-started-fresh")
    .send(&world)
    .await;
    assert_eq!(finished.status, 200, "{}", finished.body);
    assert_eq!(finished.json()["outcome"], "cleared");
    assert!(
        !std::fs::read_to_string(&path)
            .expect("the document reads")
            .contains("\"claude\""),
        "the fresh key removed the marker"
    );
}

#[tokio::test]
async fn a_provider_marker_key_bound_by_another_realm_operation_is_a_conflict() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.bind_realm_operation(&IdempotencyBinding {
                key: "marker-foreign".to_owned(),
                operation: "publish_fleet_policy",
                fingerprint: ContentHash::of(b"the other operation"),
                bound_at: at("2026-10-02T09:00:00Z"),
            })
        })
        .expect("the other operation's key is bound");

    let preview = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = Call::post(
        "/v1/provider-markers:apply",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
            "expected_revision": revision,
            "preview_hash": preview_hash,
        }),
    )
    .signed_as(&world, "admin")
    .with_key("marker-foreign")
    .send(&world)
    .await;
    assert_eq!(apply.status, 409, "{}", apply.body);
    assert_eq!(apply.code(), "idempotency_conflict");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "another operation's key never rewrites the settings document"
    );
}

#[tokio::test]
async fn a_competing_claim_between_the_pre_check_and_the_bind_is_a_conflict() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    let preview = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": family,
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });

    // The apply passes its replay pre-check, then waits before its durable
    // claim. Another operation bound to the same key lands in that window.
    let hold = world.daemon.hold_provider_marker_bind();
    let held = async {
        Call::post("/v1/provider-markers:apply", &apply)
            .signed_as(&world, "admin")
            .with_key("marker-bind-race")
            .send(&world)
            .await
    };
    let race = async {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while hold.waiting() < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the apply reaches the window before its claim");
        world
            .daemon
            .state()
            .with_store(|store| {
                store.bind_realm_operation(&IdempotencyBinding {
                    key: "marker-bind-race".to_owned(),
                    operation: "publish_fleet_policy",
                    fingerprint: ContentHash::of(b"the other operation"),
                    bound_at: at("2026-10-02T09:00:00Z"),
                })
            })
            .expect("the competing claim lands between the pre-check and the bind");
        hold.release();
    };
    let (applied, ()) = tokio::join!(held, race);
    assert_eq!(applied.status, 409, "{}", applied.body);
    assert_eq!(
        applied.code(),
        "idempotency_conflict",
        "a permanently bound key is not a revision to re-read and retry: {}",
        applied.body
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "the losing apply rewrites nothing"
    );
    let binding = world
        .daemon
        .state()
        .with_store(|store| store.realm_idempotency_binding("marker-bind-race"))
        .expect("the binding reads")
        .expect("the competing operation's claim stands");
    assert_eq!(binding.operation, "publish_fleet_policy");
}

#[tokio::test]
async fn a_retry_finishes_a_provider_marker_correction_claimed_before_the_write() {
    let world = World::open().await;
    let path = fixture(&world);
    let family = fake_family().to_string();
    let preview = Call::post(
        "/v1/provider-markers:preview",
        &serde_json::json!({
            "runtime_family": family,
            "provider": "claude",
        }),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let revision = preview.json()["revision"]
        .as_str()
        .expect("a revision")
        .to_owned();
    let preview_hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    let apply = serde_json::json!({
        "runtime_family": family,
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    });
    // The first attempt claimed the key durably and then died before writing:
    // the binding is the exact intent the request presents, and the document
    // still holds the marker at the revision the request names.
    let fingerprint = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "operation": "provider_marker_apply",
        "runtime_family": fake_family().as_str(),
        "provider": "claude",
        "expected_revision": revision,
        "preview_hash": preview_hash,
    }))
    .expect("the intent canonicalizes")
    .hash()
    .clone();
    world
        .daemon
        .state()
        .with_store(|store| {
            store.bind_realm_operation(&IdempotencyBinding {
                key: "marker-interrupted".to_owned(),
                operation: "clear_provider_marker",
                fingerprint,
                bound_at: at("2026-10-02T09:00:00Z"),
            })
        })
        .expect("the first attempt's claim is durable");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the document reads"),
        marker_document(),
        "an interrupted attempt leaves the marker standing"
    );

    // The retry does not refuse over the stale revision its own first attempt
    // presented: it finishes the correction under the same key.
    let finished = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("marker-interrupted")
        .send(&world)
        .await;
    assert_eq!(finished.status, 200, "{}", finished.body);
    assert_eq!(finished.json()["outcome"], "cleared");
    assert_eq!(finished.json()["changed"], true);
    let cleared = std::fs::read(&path).expect("the document reads");
    assert!(
        !String::from_utf8_lossy(&cleared).contains("\"claude\""),
        "the finished correction removed the marker"
    );

    let replayed = Call::post("/v1/provider-markers:apply", &apply)
        .signed_as(&world, "admin")
        .with_key("marker-interrupted")
        .send(&world)
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["outcome"], "replayed");
    assert_eq!(std::fs::read(&path).expect("the document reads"), cleared);
}
