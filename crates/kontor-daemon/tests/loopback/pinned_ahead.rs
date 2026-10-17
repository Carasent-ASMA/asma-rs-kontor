//! The pinned-ahead epic: an authorized per-epic upgrade refuses placement
//! until the project selection is moved through its own preview/apply pair.
//!
//! `docs/evidence/KON-OP-08/REVIEW-NOTES.md` F1 recorded the shape: an
//! authorized zero-effect topology upgrade repins only the epic, and both
//! `pin_epic_topology` and `ensure_task_node` refuse when that pin differs from
//! the project's selected default. The Jira gap is not the refusal — that is
//! deliberate and preserved — but the missing proof that the supported project
//! selection pair recovers placement, and that neither a stale nor a replayed
//! selection apply quietly moves another epic's pin.
//!
//! Fixture-only: every setup and every assertion goes through the public
//! loopback API over the fake runtime. No live project, epic pin, seat,
//! TeamRun or provider is touched.

use super::*;

#[tokio::test]
async fn a_pinned_ahead_epic_is_refused_until_the_project_selection_moves() {
    let composed = compose_realm("/tmp/kontor-pinned-ahead").await;
    let world = &composed.world;

    let pinned_before = Call::get(format!(
        "/v1/projects/{}/topology:inspect?epic_id={}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(pinned_before.status, 200, "{}", pinned_before.body);
    assert_eq!(pinned_before.json()["pinned_spec"]["version"], 4);
    let bundled = pinned_before.json()["pinned_spec"]["id"]
        .as_str()
        .expect("a spec id")
        .to_owned();

    let task = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(task.status, 200, "{}", task.body);
    let task = task.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("the composed task")
        .to_owned();

    // An unrelated epic is pinned by the same supported apply while the project
    // still selects v4; the selection move below must never touch it.
    let category = first_category(world).await;
    let unrelated = Call::post(
        format!("/v1/projects/{}/epics:apply", composed.project),
        &epic_body(
            composed.project_revision,
            "Pinned ahead unrelated",
            &category,
            serde_json::json!([{"title": "An unrelated task", "short_code": "UNREL-1"}]),
        ),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-unrelated")
    .send(world)
    .await;
    assert_eq!(unrelated.status, 200, "{}", unrelated.body);
    let unrelated = unrelated.json()["epic_id"]
        .as_str()
        .expect("an unrelated epic")
        .to_owned();
    confirm_test_epic_identity(world, &composed.project, &unrelated);

    // Both scopes materialize at the selected revision before the upgrade.
    materialize_execution_topology(
        world,
        &composed.project,
        composed.project_revision,
        &composed.epic,
        &task,
        "pinned-ahead-before",
    )
    .await;

    // Publish one zero-effect revision of the same lineage.
    let current = Call::get(format!(
        "/v1/projects/{}/topology-specs/{bundled}/4",
        composed.project
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(current.status, 200, "{}", current.body);
    let mut node_kinds = current.json()["document"]["node_kinds"].clone();
    node_kinds
        .as_array_mut()
        .expect("node kinds")
        .iter_mut()
        .find(|kind| kind["kind"] == "ESW")
        .expect("the ESW kind")["name_template"] = serde_json::json!({
        "segments": [
            {"kind": "literal", "value": "Pinned ahead ESW"},
            {"kind": "token", "value": "JIRA_CODE"},
            {"kind": "token", "value": "KONTOR_BACKLOG_CODE"}
        ]
    });
    let drafted = Call::post(
        format!("/v1/projects/{}/topology-specs:draft", composed.project),
        &serde_json::json!({
            "base": {"id": bundled, "version": 4},
            "name": "Pinned ahead vocabulary",
            "root_kind": "PSW",
            "node_kinds": node_kinds,
            "historical_codes": current.json()["document"]["historical_codes"],
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-draft")
    .send(world)
    .await;
    assert_eq!(drafted.status, 200, "{}", drafted.body);
    assert_eq!(
        drafted.json()["candidate"]["version"],
        5,
        "an edit drafts the next version of the lineage: {}",
        drafted.body
    );
    let candidate = drafted.json()["candidate"].clone();
    let validation = drafted.json()["candidate_hash"]
        .as_str()
        .expect("a candidate hash")
        .to_owned();
    let published = Call::post(
        format!("/v1/projects/{}/topology-specs:publish", composed.project),
        &serde_json::json!({
            "candidate": candidate,
            "validation_hash": validation,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);

    // The authorized upgrade moves the epic ahead of the project selection.
    let upgrade_preview = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-preview",
            composed.project, composed.epic
        ),
        &serde_json::json!({"target_spec": {"id": bundled, "version": 5}}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(upgrade_preview.status, 200, "{}", upgrade_preview.body);
    assert!(
        upgrade_preview.json()["effects"]
            .as_array()
            .expect("effects")
            .is_empty(),
        "a name-template-only upgrade preserves every topology subject: {}",
        upgrade_preview.body
    );
    let upgrade_hash = upgrade_preview.json()["preview_hash"]
        .as_str()
        .expect("an upgrade hash")
        .to_owned();
    let epic = Call::get(format!(
        "/v1/projects/{}/epics/{}",
        composed.project, composed.epic
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    let epic_revision = epic.json()["revision"].as_u64().expect("an epic revision");
    let upgraded = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-apply",
            composed.project, composed.epic
        ),
        &serde_json::json!({
            "preview_hash": upgrade_hash,
            "expected_revision": epic_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-upgrade")
    .send(world)
    .await;
    assert_eq!(upgraded.status, 200, "{}", upgraded.body);
    assert_eq!(upgraded.json()["pinned_spec"]["version"], 5);

    // A replay of the authorized upgrade is the same act and moves no pin.
    let upgrade_replay = Call::post(
        format!(
            "/v1/projects/{}/epics/{}/topology:upgrade-apply",
            composed.project, composed.epic
        ),
        &serde_json::json!({
            "preview_hash": upgrade_hash,
            "expected_revision": epic_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-upgrade")
    .send(world)
    .await;
    assert_eq!(upgrade_replay.status, 200, "{}", upgrade_replay.body);
    assert_eq!(upgrade_replay.json()["receipt"]["applied"], "unchanged");

    // The epic is ahead of the selection; placement refuses with the exact rule.
    let refused = Call::post(
        format!("/v1/projects/{}/topology:materialize", composed.project),
        &serde_json::json!({
            "target": {"scope": "ticket", "task_id": task},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("pinned-ahead-refused")
    .send(world)
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.code(), "placement_blocked");
    assert_eq!(
        refused.json()["rule"],
        "this epic is pinned to another topology revision than the project selects",
        "the refusal names the exact mismatch rule: {}",
        refused.body
    );

    // The supported recovery is the project selection pair.
    let selection_preview = Call::post(
        format!(
            "/v1/projects/{}/topology-selection:preview",
            composed.project
        ),
        &serde_json::json!({"target_spec": {"id": bundled, "version": 5}}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(selection_preview.status, 200, "{}", selection_preview.body);
    assert_eq!(selection_preview.json()["current_spec"]["version"], 4);
    assert_eq!(selection_preview.json()["target_spec"]["version"], 5);
    let selection_hash = selection_preview.json()["preview_hash"]
        .as_str()
        .expect("a selection hash")
        .to_owned();

    // A stale apply is refused and moves neither the selection nor a pin.
    let stale = Call::post(
        format!("/v1/projects/{}/topology-selection:apply", composed.project),
        &serde_json::json!({
            "preview_hash": selection_hash,
            "expected_revision": composed.project_revision + 1,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-selection-stale")
    .send(world)
    .await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.code(), "revision_conflict");
    let still_selected = Call::get(format!(
        "/v1/projects/{}/topology:inspect",
        composed.project
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(
        still_selected.json()["pinned_spec"]["version"],
        4,
        "a stale selection apply moved the project selection: {}",
        still_selected.body
    );

    let selected = Call::post(
        format!("/v1/projects/{}/topology-selection:apply", composed.project),
        &serde_json::json!({
            "preview_hash": selection_hash,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-selection")
    .send(world)
    .await;
    assert_eq!(selected.status, 200, "{}", selected.body);
    assert_eq!(selected.json()["selected_spec"]["version"], 5);
    assert_eq!(selected.json()["receipt"]["applied"], "updated");

    let selection_replay = Call::post(
        format!("/v1/projects/{}/topology-selection:apply", composed.project),
        &serde_json::json!({
            "preview_hash": selection_hash,
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pinned-ahead-selection")
    .send(world)
    .await;
    assert_eq!(selection_replay.status, 200, "{}", selection_replay.body);
    assert_eq!(selection_replay.json()["receipt"]["applied"], "unchanged");

    // Neither the successful move nor its replay silently repins another epic.
    let unrelated_after = Call::get(format!(
        "/v1/projects/{}/topology:inspect?epic_id={}",
        composed.project, unrelated
    ))
    .signed_as(world, "observer")
    .send(world)
    .await;
    assert_eq!(
        unrelated_after.json()["pinned_spec"]["version"],
        4,
        "a project selection move or its replay moved an unrelated epic pin: {}",
        unrelated_after.body
    );

    // The aligned epic places again.
    let recovered = Call::post(
        format!("/v1/projects/{}/topology:materialize", composed.project),
        &serde_json::json!({
            "target": {"scope": "ticket", "task_id": task},
            "expected_revision": composed.project_revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("pinned-ahead-recovered")
    .send(world)
    .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
}
