//! Source-only typed memory HTTP tests against disposable fake realms.
#[allow(dead_code)]
mod harness;

use harness::{Call, World, at};
use kontor_core::id::{CanonicalDocument, TaskWorkflowId};
use kontor_core::repository::{NewTaskWorkflow, WorkflowRepository};
use kontor_core::spec::ResolvedWorkProfileSnapshot;
use kontor_profiles::pack::{PackAvailability, resolve_profile};
use kontor_store::memory::MemoryProvenance;
use serde_json::{Value, json};

async fn world() -> World {
    let world = World::open().await;
    world.daemon.reconcile().await;
    let pack = kontor_profiles::seeds::bundled_pack().unwrap();
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == PackAvailability::Seeded)
        .unwrap();
    let bundle = resolve_profile(&pack, &entry.category, at("2026-08-10T09:00:00Z")).unwrap();
    let definition = &bundle.profile.definition;
    world
        .daemon
        .state()
        .with_store(|store| {
            store.create_task_workflow(&NewTaskWorkflow {
                id: TaskWorkflowId::generate(),
                project_id: world.project,
                task_id: world.task,
                snapshot: ResolvedWorkProfileSnapshot::resolve(
                    definition,
                    at("2026-08-10T09:00:00Z"),
                )
                .unwrap(),
                current_phase: definition.entry_phase.clone(),
                created_at: at("2026-08-10T09:00:00Z"),
            })
        })
        .unwrap();
    world
}
fn proposal() -> Value {
    let mut document: Value = serde_json::from_str(include_str!(
        "../../kontor-core/tests/fixtures/experience-v1.json"
    ))
    .unwrap();
    document["future_cues"] = json!(["loopback task"]);
    json!({"item_id":"typed","expected_revision":0,"document":document,"provenance":MemoryProvenance { source:"synthetic-fixture".into(),source_id:None,legacy_last_write_wins:false,history_unavailable:false },"proposed_by":"fixture-author"})
}
#[tokio::test]
async fn proposal_approval_preview_freeze_readback_replay_and_purge_refusal() {
    let world = world().await;
    let base = format!("/v1/projects/{}/memory", world.project);
    let body = proposal();
    let denied = Call::post(format!("{base}/experiences:propose"), &body)
        .signed_as(&world, "observer")
        .with_key("denied")
        .send(&world)
        .await;
    assert_eq!(denied.status, 403);
    let proposed = Call::post(format!("{base}/experiences:propose"), &body)
        .signed_as(&world, "operator")
        .with_key("typed-propose")
        .send(&world)
        .await;
    assert_eq!(proposed.status, 200, "{}", proposed.body);
    assert_eq!(proposed.json()["revision"]["approved"], false);
    let replay = Call::post(format!("{base}/experiences:propose"), &body)
        .signed_as(&world, "operator")
        .with_key("typed-propose")
        .send(&world)
        .await;
    assert_eq!(proposed.json(), replay.json());
    let preview_request = json!({"task_id":world.task});
    let pending = Call::post(format!("{base}/recall:preview"), &preview_request)
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(pending.status, 200, "{}", pending.body);
    assert_eq!(pending.json()["recall"]["canonical_block"], "[]");
    let revision = proposed.json()["revision"]["revision_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let approved = Call::post(
        format!("{base}/revisions/{revision}/approval"),
        &json!({"item_id":"typed","expected_revision":1,"approved_by":"fixture-reviewer"}),
    )
    .signed_as(&world, "admin")
    .with_key("typed-approve")
    .send(&world)
    .await;
    assert_eq!(approved.status, 200);
    let preview = Call::post(format!("{base}/recall:preview"), &preview_request)
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    assert_eq!(
        preview.json()["recall"]["metadata"]["mode"],
        "lexical_degraded"
    );
    assert_eq!(preview.json()["recall"]["metadata"]["reason"], "absent");
    assert_eq!(
        preview.json()["recall"]["metadata"]["identities"][0]["revision_id"],
        revision
    );
    let run = world.unbound_run();
    let freeze_body = json!({"task_id":world.task,"agent_run_id":run});
    let frozen = Call::post(format!("{base}/recall:freeze"), &freeze_body)
        .signed_as(&world, "operator")
        .with_key("freeze")
        .send(&world)
        .await;
    assert_eq!(frozen.status, 200, "{}", frozen.body);
    let hash = frozen.json()["recall"]["binding"]["result_hash"].clone();
    let block = frozen.json()["recall"]["canonical_block"].clone();
    let another_run = world.unbound_run();
    let conflict = Call::post(
        format!("{base}/recall:freeze"),
        &json!({"task_id":world.task,"agent_run_id":another_run}),
    )
    .signed_as(&world, "operator")
    .with_key("freeze")
    .send(&world)
    .await;
    assert_eq!(conflict.status, 409);
    assert_eq!(conflict.json()["code"], "binding_conflict");
    let tomb = Call::post(
        format!("{base}/typed/tombstone"),
        &json!({"expected_revision":2,"by":"fixture-reviewer","reason":"retired"}),
    )
    .signed_as(&world, "admin")
    .with_key("tomb")
    .send(&world)
    .await;
    assert_eq!(tomb.status, 200);
    let replay = Call::post(format!("{base}/recall:freeze"), &freeze_body)
        .signed_as(&world, "operator")
        .with_key("freeze-retry")
        .send(&world)
        .await;
    assert_eq!(replay.status, 200);
    assert_eq!(replay.json()["recall"]["binding"]["result_hash"], hash);
    assert_eq!(replay.json()["recall"]["canonical_block"], block);
    assert_eq!(replay.json()["recall"]["replayed"], true);
    let read = Call::get(format!("{base}/recall/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(read.json()["recall"]["canonical_block"], block);
    let purge = Call::post(
        format!("{base}/typed/purge"),
        &json!({"by":"fixture-admin"}),
    )
    .signed_as(&world, "admin")
    .with_key("purge")
    .send(&world)
    .await;
    assert_eq!(purge.status, 200);
    let gone = Call::get(format!("{base}/recall/{run}"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(gone.status, 410);
    assert_eq!(gone.json()["code"], "frozen_payload_purged");
    assert_eq!(
        world
            .daemon
            .state()
            .with_store(|store| store.memory_binding(world.project, &run.to_string()))
            .unwrap()
            .unwrap()
            .result_hash
            .as_str(),
        hash.as_str().unwrap()
    );
}
#[tokio::test]
async fn secret_canaries_hidden_queries_and_projection_unavailable_are_static() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || Capture(writer.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let world = world().await;
    let base = format!("/v1/projects/{}/memory", world.project);
    let canary = "password=HTTP_NO_LOG_CANARY";
    for place in ["text", "nested", "key", "evidence"] {
        let mut body = proposal();
        match place {
            "text" => body["document"]["lesson"] = json!(canary),
            "nested" => body["document"]["actions"] = json!([canary]),
            "key" => body["document"][canary] = json!("safe"),
            _ => body["document"]["evidence_refs"][0]["locator"] = json!(canary),
        }
        let refusal = Call::post(format!("{base}/experiences:propose"), &body)
            .signed_as(&world, "operator")
            .with_key(format!("secret-{place}"))
            .send(&world)
            .await;
        assert_eq!(refusal.status, 400);
        assert!(!refusal.body.contains("HTTP_NO_LOG_CANARY"));
    }
    let hidden = Call::post(
        format!("{base}/recall:preview"),
        &json!({"task_id":world.task,"query":"hidden prompt"}),
    )
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(hidden.status, 400);
    let preview = Call::get(format!("{base}/projection:preview"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(preview.status, 200);
    let value = preview.json();
    let rebuild_body = json!({"expected_generation":value["preview"]["active_generation"],"expected_memory_cursor":value["preview"]["snapshot"]["memory_cursor"],"preview_digest":value["preview"]["snapshot"]["digest"]});
    let unavailable = Call::post(format!("{base}/projection:rebuild"), &rebuild_body)
        .signed_as(&world, "operator")
        .with_key("rebuild")
        .send(&world)
        .await;
    assert_eq!(unavailable.status, 503);
    assert_eq!(unavailable.json()["code"], "projection_unavailable");
    let after = Call::get(format!("{base}/projection"))
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert!(after.json()["projection"]["active"].is_null());
    let mut invalid = proposal();
    invalid["document"]["kind"] = json!("operational_gap");
    let refusal = Call::post(format!("{base}/experiences:propose"), &invalid)
        .signed_as(&world, "operator")
        .with_key("invalid-kind")
        .send(&world)
        .await;
    assert_eq!(refusal.json()["code"], "invalid_experience");
    let document = CanonicalDocument::from_value(&proposal()["document"]).unwrap();
    assert!(kontor_core::memory::is_recall_eligible(&document));
    assert!(
        !String::from_utf8(logs.lock().unwrap().clone())
            .unwrap()
            .contains("HTTP_NO_LOG_CANARY")
    );
}
