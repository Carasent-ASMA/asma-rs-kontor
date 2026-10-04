//! ASMA-8158: fixture-only transport and the actual normal root launch boundary.
use super::*;
use kontor_core::memory::{DegradedReason, MemoryCandidate, RetrievalMode};
use kontor_memory_cognee::{Client, Config};
use kontor_store::memory::{MemoryProvenance, ProjectionQualification};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

fn seed(
    world: &World,
    project: ProjectId,
    item: &str,
    provider: bool,
    cue: &str,
) -> MemoryCandidate {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../kontor-core/tests/fixtures/experience-v1.json"
    ))
    .unwrap();
    value["projection_policy"] = json!(if provider {
        "provider_eligible"
    } else {
        "local_only"
    });
    value["future_cues"] = json!([cue]);
    // Keep distractors independent of the fixture's profile/module vocabulary.
    value["situation"] = json!("zzq scenario");
    value["intent"] = json!("zzq objective");
    value["actions"] = json!(["zzq action"]);
    value["outcome"]["summary"] = json!("zzq result");
    value["went_well"] = json!([]);
    value["went_wrong"] = json!([]);
    value["lesson"] = json!("zzq settled lesson");
    value["avoid"] = json!([]);
    value["domains"] = json!(["zzq"]);
    value["evidence_refs"][0]["locator"] = json!("synthetic/zzq.json");
    let document = CanonicalDocument::from_value(&value).unwrap();
    world.daemon.state().with_store(|store| {
        let (revision, _) = store
            .propose_experience(
                project,
                item,
                0,
                &document,
                &MemoryProvenance {
                    source: "synthetic-fixture".into(),
                    source_id: None,
                    legacy_last_write_wins: false,
                    history_unavailable: false,
                },
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, item, &revision.revision_id, 1, "reviewer")
            .unwrap();
        MemoryCandidate {
            project_id: project,
            item_id: item.into(),
            revision_id: revision.revision_id,
            content_hash: document.hash().clone(),
            score: 1.0,
        }
    })
}
fn search_response(candidate: &MemoryCandidate, text: &str) -> Value {
    json!([{"dataset_name":"wrong-upstream-dataset","search_result":[{"score":0.1,"text":serde_json::to_string(&json!({
        "identity":{"project_id":candidate.project_id,"item_id":candidate.item_id,"revision_id":candidate.revision_id,"content_hash":candidate.content_hash},
        "cues":["upstream"],"lesson":text
    })).unwrap()}]}])
}
fn active(world: &World, project: ProjectId) {
    world.daemon.state().with_store(|store| {
        let preview = store.stage_projection(project).unwrap();
        store
            .activate_projection(
                project,
                preview.snapshot.memory_cursor,
                preview.active_generation,
                &ProjectionQualification {
                    digest: preview.snapshot.digest,
                    added: true,
                    cognified: true,
                    canary_passed: true,
                },
            )
            .unwrap();
    });
}
fn attach(world: &World, server: &MockServer, timeout_ms: u64) {
    world.daemon.jira_reconciler().attach_memory_cognee(
        Client::new(
            Config {
                enabled: true,
                endpoint: Some(server.uri()),
                timeout_ms,
                ..Config::default()
            },
            None,
        )
        .unwrap(),
    );
}
async fn setup(slug: &'static str) -> (World, ProjectId, String, TaskId) {
    let world = World::open_empty_with_a_plane().await;
    world.script(HISTORY_LIVE);
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let (project, epic, _) = armed_and_planned(&world, slug).await;
    let project = ProjectId::parse(&project).unwrap();
    let task = world
        .daemon
        .state()
        .with_store(|store| store.list_tasks(project).unwrap()[0].id);
    let db = rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
        .unwrap();
    db.execute(
        "UPDATE tasks SET title='needle launch' WHERE id=?1",
        [task.to_string()],
    )
    .unwrap();
    (world, project, epic, task)
}
async fn start(world: &World, project: ProjectId, epic: &str, key: &str) -> harness::Answer {
    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(plan.status, 200, "{}", plan.body);
    Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:start"),
        &json!({"plan_hash":plan.json()["plan_hash"]}),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await
}
fn root_recall(
    world: &World,
    project: ProjectId,
    answer: &harness::Answer,
) -> kontor_store::memory::RecalledMemory {
    let run = AgentRunId::parse(
        answer.json()["started"][0]["agent_run_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    world
        .daemon
        .state()
        .with_store(|store| store.recalled_memory(project, &run.to_string()))
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn root_launch_contains_exact_frozen_canonical_bytes_and_downstream_cites_same_binding() {
    let (world, project, epic, _task) = setup("memory-launch").await;
    let candidate = seed(&world, project, "selected", true, "needle launch");
    seed(
        &world,
        project,
        "unrelated",
        false,
        "IRRELEVANT_CORPUS_CANARY",
    );
    active(&world, project);
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(search_response(&candidate, "COGNEE_LAUNCH_TEXT_CANARY")),
        )
        .expect(1)
        .mount(&server)
        .await;
    attach(&world, &server, 1500);
    let answer = start(&world, project, &epic, "memory-launch-start").await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert!(
        !answer.json()["started"].as_array().unwrap().is_empty(),
        "{}",
        answer.body
    );
    let recall = root_recall(&world, project, &answer);
    assert_eq!(recall.metadata.mode, RetrievalMode::Semantic);
    assert_eq!(recall.metadata.identities[0].item_id, "selected");
    let run = AgentRunId::parse(&recall.binding.run_id).unwrap();
    let prompt = world.fake.launched_prompt(run).unwrap();
    assert!(prompt.as_str().starts_with("begin the admitted task."));
    assert!(prompt.as_str().contains(&format!(
        "<experience_memory>\n{}\n</experience_memory>",
        recall.canonical_block
    )));
    assert!(
        prompt
            .as_str()
            .contains(recall.binding.result_hash.as_str())
    );
    assert!(!prompt.as_str().contains("COGNEE_LAUNCH_TEXT_CANARY"));
    assert!(!prompt.as_str().contains("IRRELEVANT_CORPUS_CANARY"));
    let mut downstream = 0;
    for seat in answer.json()["started"].as_array().unwrap() {
        let run = AgentRunId::parse(seat["agent_run_id"].as_str().unwrap()).unwrap();
        let prompt = world.fake.launched_prompt(run).unwrap();
        assert!(
            prompt
                .as_str()
                .contains(&format!("run={}", recall.binding.run_id))
        );
        assert!(
            prompt
                .as_str()
                .contains(recall.binding.result_hash.as_str())
        );
        if prompt.as_str().starts_with("wait:") {
            downstream += 1;
            assert!(!prompt.as_str().contains("<experience_memory>"));
        }
    }
    assert!(
        downstream > 0,
        "fixture must exercise a downstream delivery seat"
    );
    let requests = server.received_requests().await.unwrap();
    let request: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let intent: Value = serde_json::from_str(request["query"].as_str().unwrap()).unwrap();
    assert_eq!(intent["task_title"], "needle launch");
    assert_eq!(intent["phase"], "implementation");
    assert_eq!(request["search_type"], "CHUNKS");
    assert_eq!(request["top_k"], 64);
    assert!(intent.get("prompt").is_none());
}

#[tokio::test]
async fn configured_live_opt_in_refuses_without_an_explicit_transport_and_never_looks_up_alias() {
    let directory = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    std::fs::write(
        directory.path().join("memory-cognee.json"),
        serde_json::to_vec(&Config {
            enabled: true,
            endpoint: Some(server.uri()),
            credential_alias: Some("SYNTHETIC_UNRESOLVED_ALIAS".into()),
            ..Config::default()
        })
        .unwrap(),
    )
    .unwrap();
    let refused = kontor_daemon::Daemon::start(
        kontor_daemon::DaemonConfig::at(directory.path()).with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap_err();
    assert!(matches!(
        refused,
        kontor_daemon::StartupError::MemoryCognee {
            source: kontor_memory_cognee::Error(DegradedReason::Unavailable)
        }
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn degraded_launch_never_lists_the_corpus_and_timeout_keeps_store_unlocked() {
    let (world, project, epic, task) = setup("memory-degraded").await;
    seed(&world, project, "relevant", false, "needle launch");
    seed(
        &world,
        project,
        "irrelevant",
        false,
        "IRRELEVANT_CORPUS_CANARY",
    );
    world.daemon.state().with_store(|store| {
        let doc = CanonicalDocument::from_value(
            &json!({"schema_version":1,"text":"needle launch GENERIC_CORPUS_CANARY"}),
        )
        .unwrap();
        let (revision, _) = store
            .propose_memory_revision(
                project,
                "generic",
                0,
                &doc,
                &MemoryProvenance {
                    source: "synthetic".into(),
                    source_id: None,
                    legacy_last_write_wins: false,
                    history_unavailable: false,
                },
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, "generic", &revision.revision_id, 1, "reviewer")
            .unwrap();
    });
    active(&world, project);
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([]))
                .set_delay(std::time::Duration::from_millis(500)),
        )
        .expect(1)
        .mount(&server)
        .await;
    attach(&world, &server, 150);
    let (answer, ()) = tokio::join!(
        start(&world, project, &epic, "memory-degraded-start"),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            assert!(
                world
                    .daemon
                    .state()
                    .with_store(|store| store.get_task(project, task))
                    .unwrap()
                    .is_some()
            );
        }
    );
    assert_eq!(answer.status, 200, "{}", answer.body);
    let recall = root_recall(&world, project, &answer);
    assert_eq!(recall.metadata.mode, RetrievalMode::LexicalDegraded);
    assert_eq!(recall.metadata.reason, Some(DegradedReason::Timeout));
    assert_eq!(
        recall.metadata.identities.len(),
        1,
        "{}",
        recall.binding.selection_spec.json()
    );
    assert_eq!(recall.metadata.identities[0].item_id, "relevant");
    let run = AgentRunId::parse(&recall.binding.run_id).unwrap();
    let prompt = world.fake.launched_prompt(run).unwrap();
    assert!(!prompt.as_str().contains("IRRELEVANT_CORPUS_CANARY"));
    assert!(!prompt.as_str().contains("GENERIC_CORPUS_CANARY"));
    assert!(prompt.as_str().contains(&recall.canonical_block));
}

#[tokio::test]
async fn stale_snapshot_and_no_match_launch_without_network_or_full_corpus() {
    let (world, project, epic, _task) = setup("memory-stale").await;
    seed(
        &world,
        project,
        "unrelated",
        true,
        "IRRELEVANT_CORPUS_CANARY",
    );
    active(&world, project);
    seed(
        &world,
        project,
        "new-unrelated",
        false,
        "NEW_IRRELEVANT_CANARY",
    );
    let server = MockServer::start().await;
    attach(&world, &server, 1500);
    let answer = start(&world, project, &epic, "memory-stale-start").await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let recall = root_recall(&world, project, &answer);
    assert_eq!(
        recall.metadata.mode,
        RetrievalMode::None,
        "{}",
        recall.binding.selection_spec.json()
    );
    assert_eq!(recall.metadata.reason, Some(DegradedReason::Stale));
    assert_eq!(recall.canonical_block, "[]");
    assert!(server.received_requests().await.unwrap().is_empty());
    let run = AgentRunId::parse(&recall.binding.run_id).unwrap();
    assert!(
        world
            .fake
            .launched_prompt(run)
            .unwrap()
            .as_str()
            .contains("<experience_memory>\n[]\n</experience_memory>")
    );
}

#[tokio::test]
async fn lost_caller_ack_replay_reuses_frozen_bytes_after_tombstone_and_rebuild() {
    let (world, project, epic, _task) = setup("memory-replay").await;
    let selected = seed(&world, project, "selected", true, "needle launch");
    active(&world, project);
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(search_response(&selected, "UNTRUSTED_REPLAY_TEXT")),
        )
        .expect(1)
        .mount(&server)
        .await;
    attach(&world, &server, 1500);
    let db = rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
        .unwrap();
    let plan = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/scheduler:plan"),
        &json!({}),
    )
    .signed_as(&world, "operator")
    .send(&world)
    .await;
    let body = json!({"plan_hash":plan.json()["plan_hash"]});
    let uri = format!("/v1/projects/{project}/epics/{epic}/scheduler:start");
    // Treat the successful response as lost at the caller. The original native
    // effects and frozen selection must survive a retry under the same key.
    let _lost_response = Call::post(&uri, &body)
        .signed_as(&world, "operator")
        .with_key("memory-replay-start")
        .send(&world)
        .await;
    let (run,_team):(String,String)=db.query_row("SELECT m.run_id,a.team_run_id FROM memory_recall_metadata m JOIN agent_runs a ON a.id=m.run_id WHERE m.project_id=?1",[project.to_string()],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
    let root = AgentRunId::parse(&run).unwrap();
    let original = world
        .daemon
        .state()
        .with_store(|store| store.recalled_memory(project, &run))
        .unwrap()
        .unwrap();
    let prompt = world.fake.launched_prompt(root).unwrap();
    let model = world.fake.launched_model(root).unwrap();
    assert!(prompt.as_str().contains(&original.canonical_block));
    world
        .daemon
        .state()
        .with_store(|store| store.tombstone_memory(project, "selected", 2, "reviewer", "retired"))
        .unwrap();
    seed(&world, project, "replacement", true, "needle launch");
    active(&world, project);
    // A separately reopened store reconstructs historical bytes, not the new
    // approved/current projection, while the native session survives lost ack.
    let reopened =
        kontor_store::SqliteStore::open(&world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    let replay = reopened.recalled_memory(project, &run).unwrap().unwrap();
    assert_eq!(replay.canonical_block, original.canonical_block);
    assert_eq!(replay.binding.result_hash, original.binding.result_hash);
    assert!(replay.replayed);
    drop(reopened);
    world.daemon.reconcile().await;
    let recovered = Call::post(&uri, &body)
        .signed_as(&world, "operator")
        .with_key("memory-replay-start")
        .send(&world)
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert!(
        recovered.json()["blocked"].as_array().unwrap().is_empty(),
        "{}",
        recovered.body
    );
    assert_eq!(world.fake.launched_prompt(root).unwrap(), prompt);
    assert_eq!(world.fake.launched_model(root).unwrap(), model);
    assert_eq!(
        world
            .fake
            .calls()
            .iter()
            .filter(|call| matches!(call,AdapterCall::Launch(id) if *id==root))
            .count(),
        1
    );
    let final_binding = world
        .daemon
        .state()
        .with_store(|store| store.recalled_memory(project, &run))
        .unwrap()
        .unwrap();
    assert_eq!(final_binding.canonical_block, original.canonical_block);
    assert_eq!(
        final_binding.binding.result_hash,
        original.binding.result_hash
    );
    for seat in recovered.json()["started"].as_array().unwrap() {
        let run = AgentRunId::parse(seat["agent_run_id"].as_str().unwrap()).unwrap();
        let launched = world.fake.launched_prompt(run).unwrap();
        assert!(
            launched
                .as_str()
                .contains(original.binding.result_hash.as_str())
        );
        assert!(!launched.as_str().contains("UNTRUSTED_REPLAY_TEXT"));
    }
}
