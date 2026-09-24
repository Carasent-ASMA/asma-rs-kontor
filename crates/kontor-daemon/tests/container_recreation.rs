//! Frozen ASMA-8239 evidence for the `recreate_absent` container disposition.
//!
//! Every test here drives the *existing* Admin container-recovery routes. There
//! is no separate recreation endpoint to exercise, which is the point: the
//! authority to build a native belongs to one operation, and recreation is the
//! second answer that operation can reach.
//!
//! The create counter — [`World::container_creates`] — is the spine of the
//! whole file. "exactly one" and "exactly zero" are the two claims the frozen
//! record asks for, and both are assertions about natives the runtime actually
//! built rather than about calls it received.

// This binary uses part of the shared harness; the rest is not dead code.
#[allow(dead_code)]
mod harness;

use harness::{Call, NATIVE_CHILD_TITLE, World};
use kontor_core::repository::TopologyRepository;
use kontor_core::state::ObservedContainerKind;

/// The project revision a caller must present, read the way a caller would.
async fn project_revision(world: &World) -> u64 {
    let read = Call::get(format!("/v1/projects/{}", world.project))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    read.json()["revision"]
        .as_u64()
        .expect("a project revision")
}

fn preview_uri(world: &World, node: &kontor_core::id::TopologyNodeId) -> String {
    format!(
        "/v1/projects/{}/topology/nodes/{node}/container:recovery-preview",
        world.project
    )
}

fn apply_uri(world: &World, node: &kontor_core::id::TopologyNodeId) -> String {
    format!(
        "/v1/projects/{}/topology/nodes/{node}/container:recovery-apply",
        world.project
    )
}

/// Evidence item 1 (daemon/API half): a vacant canonical path previews as
/// `recreate_absent`, applies with exactly one create, and preserves every
/// identity the node already owned.
#[tokio::test]
async fn recreate_absent_previews_then_applies_with_exactly_one_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let previewed = preview.json();
    assert_eq!(previewed["disposition"], "recreate_absent");
    assert!(
        previewed["replacement_native_id"].is_null(),
        "a preview must not predict an identity no runtime has minted: {}",
        preview.body
    );
    assert_eq!(previewed["observed_title"], NATIVE_CHILD_TITLE);
    assert_eq!(
        previewed["canonical_cwd"],
        fixture.canonical_cwd.as_str(),
        "the preserved canonical path is the persisted one"
    );
    assert_eq!(
        previewed["parent_native_id"],
        fixture.parent_native_id.as_str()
    );
    assert_eq!(
        previewed["stale_native_id"],
        fixture.stale_identity.native_id.as_str()
    );
    assert_eq!(
        world.container_creates(),
        0,
        "a preview writes nothing at all"
    );

    let applied = Call::post(
        apply_uri(&world, &fixture.node),
        &serde_json::json!({
            "expected_revision": revision,
            "preview_hash": previewed["preview_hash"],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-recreate-success")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let result = applied.json();
    assert_eq!(result["recovery"]["disposition"], "recreate_absent");
    let replacement = result["recovery"]["replacement_native_id"]
        .as_str()
        .expect("an applied result always names what it bound");
    assert_ne!(
        replacement,
        fixture.stale_identity.native_id.as_str(),
        "the replacement must not be the identity proved absent"
    );
    assert_eq!(
        world.container_creates(),
        1,
        "exactly one native may be built"
    );

    // The durable binding: logical identity preserved, native identity moved.
    let stored = world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(world.project, fixture.node))
        .expect("the binding reads back")
        .expect("the node still holds a container");
    assert_eq!(
        stored.container_binding_id, fixture.binding_id,
        "the logical container-binding identity is preserved"
    );
    assert_eq!(stored.topology_node_id, fixture.node);
    assert_eq!(stored.observed_kind, ObservedContainerKind::Workspace);
    assert_eq!(
        stored
            .canonical_cwd
            .as_ref()
            .map(kontor_core::id::ExternalName::as_str),
        Some(fixture.canonical_cwd.as_str()),
        "the canonical working directory is preserved"
    );
    assert_eq!(stored.identity.native_id.as_str(), replacement);
}

/// Evidence item 4: the same key replays to the same receipt and evidence, and
/// never reaches the runtime a second time.
#[tokio::test]
async fn a_same_key_apply_replays_without_a_second_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let hash = preview.json()["preview_hash"].clone();

    let body = serde_json::json!({
        "expected_revision": revision,
        "preview_hash": hash,
    });
    let first = Call::post(apply_uri(&world, &fixture.node), &body)
        .signed_as(&world, "admin")
        .with_key("asma-8239-replay")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(world.container_creates(), 1);

    let second = Call::post(apply_uri(&world, &fixture.node), &body)
        .signed_as(&world, "admin")
        .with_key("asma-8239-replay")
        .send(&world)
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(
        world.container_creates(),
        1,
        "a replay must not reach the runtime at all"
    );

    let (first, second) = (first.json(), second.json());
    assert_eq!(first["recovery"]["disposition"], "recreate_absent");
    assert_eq!(
        second["recovery"]["disposition"], first["recovery"]["disposition"],
        "a replay preserves the original authorized recovery disposition"
    );
    assert_eq!(
        first["receipt"]["receipt_id"], second["receipt"]["receipt_id"],
        "a replay returns the original receipt"
    );
    assert_eq!(
        first["recovery"]["replacement_native_id"], second["recovery"]["replacement_native_id"],
        "a replay returns the identity the original apply bound"
    );
    assert_eq!(second["receipt"]["applied"], "unchanged");
}

/// Evidence item 4, second half: the same key carrying a different intent is an
/// idempotency conflict rather than a second effect.
#[tokio::test]
async fn a_same_key_apply_with_a_changed_intent_is_refused() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    let hash = preview.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();

    let first = Call::post(
        apply_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision, "preview_hash": hash}),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-changed-intent")
    .send(&world)
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(world.container_creates(), 1);

    // Same key, different preview digest: the caller is asking for something
    // else under a key that already settled.
    //
    // The substituted character must be guaranteed different from the one it
    // replaces. Hard-coding a digit made this pass roughly fifteen runs in
    // sixteen and silently assert nothing on the sixteenth, because the digest
    // is content-derived and its last character varies per run.
    let last = hash.chars().last().expect("a non-empty digest");
    let replacement = if last == '0' { '1' } else { '0' };
    let altered = format!("{}{replacement}", &hash[..hash.len() - 1]);
    assert_ne!(altered, hash, "the altered digest must actually differ");
    let second = Call::post(
        apply_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision, "preview_hash": altered}),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-changed-intent")
    .send(&world)
    .await;
    assert!(
        second.status.is_client_error(),
        "a changed intent under a settled key must be refused: {} {}",
        second.status,
        second.body
    );
    assert_eq!(
        world.container_creates(),
        1,
        "a refused replay must not build a second native"
    );
}

/// Evidence item 2 (daemon half): a subject that is not a persisted
/// `NativeChild` is refused before any runtime effect.
#[tokio::test]
async fn a_non_native_child_subject_is_refused_before_any_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    // The ancestor is bound as a native *project*, which container recovery
    // has never been allowed to act on.
    let preview = Call::post(
        preview_uri(&world, &fixture.parent_node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert!(
        preview.status.is_client_error(),
        "a native root is not a recoverable child: {} {}",
        preview.status,
        preview.body
    );
    assert_eq!(
        world.container_creates(),
        0,
        "a refused subject must never reach a create"
    );
}

/// Evidence item 2: a stale project revision is refused, and builds nothing.
#[tokio::test]
async fn a_stale_project_revision_is_refused_before_any_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision + 41}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert!(
        preview.status.is_client_error(),
        "a revision the caller never read must be refused: {} {}",
        preview.status,
        preview.body
    );
    assert_eq!(world.container_creates(), 0);
}

/// Evidence item 2: an apply carrying a preview digest that does not match the
/// current census is refused.
#[tokio::test]
async fn an_apply_with_a_foreign_preview_hash_is_refused_before_any_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let applied = Call::post(
        apply_uri(&world, &fixture.node),
        &serde_json::json!({
            "expected_revision": revision,
            "preview_hash": "0".repeat(64),
        }),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-foreign-hash")
    .send(&world)
    .await;
    assert!(
        applied.status.is_client_error(),
        "an unmatched preview must not authorize a create: {} {}",
        applied.status,
        applied.body
    );
    assert_eq!(world.container_creates(), 0);
}

/// Evidence item 2: the operation is Admin-only, and an under-privileged caller
/// is refused before the runtime is consulted.
#[tokio::test]
async fn a_non_admin_caller_cannot_reach_a_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    for tier in ["observer", "operator"] {
        let applied = Call::post(
            apply_uri(&world, &fixture.node),
            &serde_json::json!({"expected_revision": revision, "preview_hash": "0".repeat(64)}),
        )
        .signed_as(&world, tier)
        .with_key(format!("asma-8239-tier-{tier}"))
        .send(&world)
        .await;
        assert!(
            applied.status.is_client_error(),
            "{tier} must not reach container recovery: {} {}",
            applied.status,
            applied.body
        );
    }
    assert_eq!(world.container_creates(), 0);
}

// ---------------------------------------------------------------------------
// Evidence item 6 — direct ASMA-8188 applicability, at the existing surfaces
// ---------------------------------------------------------------------------
//
// These are the negatives the frozen record asks for, and they are deliberately
// *direct*: each one drives the real route and counts the natives the runtime
// built. There is no predicate to interrogate, because there is no predicate —
// the authority to build a native lives in one operation, and these two are not
// it.
//
// Both worlds seed a recoverable `NativeChild` first. That matters: it makes the
// recreation path genuinely *available* in the world under test, so a create
// counter of zero means "this operation never reached it" rather than "there was
// nothing to reach".

/// Nothing in the refusal may be about containers.
///
/// The record requires these operations to answer with *their own* typed
/// refusal. A container or NativeChild refusal coming back from a gate route
/// would mean the container precondition had leaked into a subject it has no
/// business judging — which is the symmetric half of the requirement, and just
/// as wrong as the leak in the other direction.
fn refusal_is_the_operations_own(body: &str) {
    let lowered = body.to_ascii_lowercase();
    for leaked in [
        "native_child",
        "nativechild",
        "container",
        "recreate",
        "canonical working directory",
    ] {
        assert!(
            !lowered.contains(leaked),
            "a gate/evaluator refusal must not mention `{leaked}`: {body}"
        );
    }
}

/// ASMA-8188 gate-rejection recovery never reaches container recreation.
#[tokio::test]
async fn gate_rejection_recovery_never_creates_a_native() {
    let world = World::open().await;
    // A subject that container recreation *would* accept, present in the same
    // world, so zero creates is a statement about this operation's reach.
    let fixture = world.seed_native_child_container();
    let before = world.container_creates();
    assert_eq!(before, 0);

    let response = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/gates/quality/rejections:recover",
            world.project, world.task
        ),
        &serde_json::json!({
            "rejection_receipt_id": kontor_core::id::CommandReceiptId::generate().to_string(),
            "sequence": 1,
            "expected_task_revision": 1,
            "expected_workflow_revision": 1,
            "expected_current_phase": "review",
            "expected_rejection_target": "build",
        }),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-8188-gate-rejection")
    .send(&world)
    .await;

    // It may succeed or refuse on its own terms; what it may never do is build
    // a native or be blocked by a NativeChild precondition.
    if response.status.is_client_error() {
        refusal_is_the_operations_own(&response.body);
    }
    assert_eq!(
        world.container_creates(),
        0,
        "gate-rejection recovery must never reach a container create: {}",
        response.body
    );

    // And the subject it never touched is untouched.
    let stored = world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(world.project, fixture.node))
        .expect("the binding reads back")
        .expect("the node still holds a container");
    assert_eq!(
        stored.identity.native_id, fixture.stale_identity.native_id,
        "an unrelated recovery must not rebind another node's container"
    );
}

/// Evaluator recovery never reaches container recreation either.
#[tokio::test]
async fn evaluator_recovery_never_creates_a_native() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();

    let response = Call::post(
        format!(
            "/v1/projects/{}/tasks/{}/gates/quality/record",
            world.project, world.task
        ),
        &serde_json::json!({
            "expected_revision": 1,
            "verdict": "pass",
            "evaluator_role": "QA",
            "evaluator_account": "default",
            "evidence": [],
            // The evaluator-recovery path: a verdict transcribed on behalf of a
            // closed evaluator seat.
            "recovery_agent_run_id": kontor_core::id::AgentRunId::generate().to_string(),
            "recovery_session_digest": "0".repeat(64),
        }),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-8188-evaluator-recovery")
    .send(&world)
    .await;

    if response.status.is_client_error() {
        refusal_is_the_operations_own(&response.body);
    }
    assert_eq!(
        world.container_creates(),
        0,
        "evaluator recovery must never reach a container create: {}",
        response.body
    );

    let stored = world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(world.project, fixture.node))
        .expect("the binding reads back")
        .expect("the node still holds a container");
    assert_eq!(stored.identity.native_id, fixture.stale_identity.native_id);
}

/// The positive half of the same matrix, stated as reachability rather than as
/// a predicate: the one operation that *may* build a native does, in the very
/// same world shape the two negatives above leave untouched.
#[tokio::test]
async fn only_container_recovery_reaches_a_create_in_the_same_world_shape() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);

    let applied = Call::post(
        apply_uri(&world, &fixture.node),
        &serde_json::json!({
            "expected_revision": revision,
            "preview_hash": preview.json()["preview_hash"],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-8188-positive")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(
        world.container_creates(),
        1,
        "the applicable operation builds exactly one native"
    );
}

/// Evidence item 3 (daemon half): a create whose acknowledgement was lost is
/// adopted by the retry, and the create counter stays where it was.
///
/// This is the failure the whole ordering exists for. Kontor holds no native id
/// — the durable binding still names the one that vanished — so its receipt
/// cannot answer the retry, and only the runtime's own census can tell the
/// difference between "nothing is there" and "my previous attempt is there".
#[tokio::test]
async fn a_lost_create_acknowledgement_is_adopted_rather_than_rebuilt() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    // Stage the lost acknowledgement: the runtime already holds a container at
    // the node's canonical path, under its exact rendered title, that Kontor
    // never learned the id of.
    world.fake.seed_container(
        fixture.node,
        kontor_runtime::container::ContainerBindingId::parse(fixture.binding_id.as_str())
            .expect("the fixture binding id is canonical"),
        kontor_core::id::ExternalId::parse("wks-created-but-unacknowledged").expect("a native id"),
        kontor_runtime::workspace::WorkspaceRoot::parse(fixture.canonical_cwd.as_str())
            .expect("a canonical root"),
        NATIVE_CHILD_TITLE,
        harness::at("2026-08-10T10:00:00Z"),
    );
    assert_eq!(world.container_creates(), 0, "seeding is not creating");

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    // With a live exact candidate present, the census reaches the *adoption*
    // disposition — which is exactly right: there is something to adopt.
    assert_eq!(preview.json()["disposition"], "adopt_existing");

    let applied = Call::post(
        apply_uri(&world, &fixture.node),
        &serde_json::json!({
            "expected_revision": revision,
            "preview_hash": preview.json()["preview_hash"],
        }),
    )
    .signed_as(&world, "admin")
    .with_key("asma-8239-lost-ack")
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(
        applied.json()["recovery"]["replacement_native_id"],
        "wks-created-but-unacknowledged",
        "the retry binds the exact native the lost attempt created"
    );
    assert_eq!(
        world.container_creates(),
        0,
        "a lost acknowledgement must never produce a second native"
    );

    let stored = world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(world.project, fixture.node))
        .expect("the binding reads back")
        .expect("the node still holds a container");
    assert_eq!(
        stored.container_binding_id, fixture.binding_id,
        "the logical binding identity survives the adoption"
    );
    assert_eq!(
        stored.identity.native_id.as_str(),
        "wks-created-but-unacknowledged"
    );
}

/// Evidence item 4, after restart: the durable receipt answers the replay even
/// when the process that wrote it is gone.
///
/// The in-memory idempotency path is not what makes a replay safe — the stored
/// receipt is. Dropping the daemon and reopening the same state root is the
/// only way to prove the difference, because a fresh realm would satisfy a
/// weaker assertion for entirely the wrong reason.
#[tokio::test]
async fn a_same_key_apply_replays_across_a_restart_without_a_second_create() {
    let world = World::open().await;
    let fixture = world.seed_native_child_container();
    let revision = project_revision(&world).await;

    let preview = Call::post(
        preview_uri(&world, &fixture.node),
        &serde_json::json!({"expected_revision": revision}),
    )
    .signed_as(&world, "admin")
    .send(&world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let body = serde_json::json!({
        "expected_revision": revision,
        "preview_hash": preview.json()["preview_hash"],
    });

    let first = Call::post(apply_uri(&world, &fixture.node), &body)
        .signed_as(&world, "admin")
        .with_key("asma-8239-restart-replay")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(world.container_creates(), 1);
    let first = first.json();

    // Reopen the same state root. The directory is deliberately kept alive.
    let admin = harness::secret(&world, "admin");
    let project = world.project;
    let node = fixture.node;
    let harness::World {
        directory, daemon, ..
    } = world;
    let state_root = directory.path().to_owned();
    drop(daemon);

    let restarted = kontor_daemon::Daemon::start(
        kontor_daemon::DaemonConfig::at(&state_root).with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .expect("the same state root reopens");
    restarted.state().signals().stop();
    let router = restarted.router();

    let replayed = Call::post(
        format!("/v1/projects/{project}/topology/nodes/{node}/container:recovery-apply"),
        &body,
    )
    .with_token(&admin)
    .with_key("asma-8239-restart-replay")
    .send_to(&router)
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    let replayed = replayed.json();
    assert_eq!(first["recovery"]["disposition"], "recreate_absent");
    assert_eq!(
        replayed["recovery"]["disposition"], first["recovery"]["disposition"],
        "restart replay preserves whether recovery authorized creation"
    );
    assert_eq!(
        replayed["receipt"]["receipt_id"], first["receipt"]["receipt_id"],
        "the durable receipt answers the replay after a restart"
    );
    assert_eq!(
        replayed["recovery"]["replacement_native_id"], first["recovery"]["replacement_native_id"],
        "and it returns the identity the original apply bound"
    );
    assert_eq!(replayed["receipt"]["applied"], "unchanged");

    // The restarted daemon has no runtime registered at all, so reaching one
    // would have failed outright. That is the strongest available statement
    // that this replay never touched a runtime.
}
