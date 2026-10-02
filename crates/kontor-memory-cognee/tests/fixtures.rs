//! Real loopback HTTP plus disposable canonical ledger; no provider service.
use kontor_core::id::{CanonicalDocument, ExternalName, ProjectId, Timestamp};
use kontor_core::memory::{DegradedReason, MemoryCandidate, RecallIntent, RetrievalMode};
use kontor_core::repository::{NewProject, ProjectRepository};
use kontor_memory_cognee::{Client, Config, MAX_RESPONSE_BYTES};
use kontor_store::{
    SqliteStore,
    memory::{MemoryProvenance, ProjectionPreview, ProjectionQualification},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

struct Ledger {
    _dir: tempfile::TempDir,
    store: SqliteStore,
    project: ProjectId,
    foreign: ProjectId,
}
impl Ledger {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("fixture.db")).unwrap();
        let project = ProjectId::generate();
        let foreign = ProjectId::generate();
        for id in [project, foreign] {
            store
                .create_project(&NewProject {
                    id,
                    name: ExternalName::parse(&format!("Synthetic {id}")).unwrap(),
                    root_path: ExternalName::parse(&format!("/tmp/synthetic-{id}")).unwrap(),
                    created_at: Timestamp::now(),
                })
                .unwrap();
        }
        Self {
            _dir: dir,
            store,
            project,
            foreign,
        }
    }
    fn approve(
        &self,
        project: ProjectId,
        item: &str,
        provider: bool,
        cue: &str,
    ) -> MemoryCandidate {
        let candidate = self.propose(project, item, provider, cue);
        self.store
            .approve_memory_revision(project, item, &candidate.revision_id, 1, "reviewer")
            .unwrap();
        candidate
    }
    fn propose(
        &self,
        project: ProjectId,
        item: &str,
        provider: bool,
        cue: &str,
    ) -> MemoryCandidate {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../kontor-core/tests/fixtures/experience-v1.json"
        ))
        .unwrap();
        value["projection_policy"] = json!(if provider {
            "provider_eligible"
        } else {
            "local_only"
        });
        value["future_cues"] = json!([cue]);
        let document = CanonicalDocument::from_value(&value).unwrap();
        let (revision, _) = self
            .store
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
        MemoryCandidate {
            project_id: project,
            item_id: item.into(),
            revision_id: revision.revision_id,
            content_hash: document.hash().clone(),
            score: 1.0,
        }
    }
    fn active(&self) -> ProjectionPreview {
        let preview = self.store.stage_projection(self.project).unwrap();
        self.store
            .activate_projection(
                self.project,
                preview.snapshot.memory_cursor,
                preview.active_generation,
                &ProjectionQualification {
                    digest: preview.snapshot.digest.clone(),
                    added: true,
                    cognified: true,
                    canary_passed: true,
                },
            )
            .unwrap();
        preview
    }
}
fn intent(title: &str) -> RecallIntent {
    RecallIntent {
        schema_version: 1,
        task_id: "synthetic-task".into(),
        task_title: title.into(),
        module: None,
        declared_scope: vec![],
        phase: "qualification".into(),
    }
}
fn client(server: &MockServer, timeout_ms: u64) -> Client {
    Client::new(
        Config {
            enabled: true,
            endpoint: Some(server.uri()),
            timeout_ms,
            ..Config::default()
        },
        None,
    )
    .unwrap()
}
fn chunk(candidate: &MemoryCandidate, lesson: &str, distance: f64) -> Value {
    json!({"text":serde_json::to_string(&json!({"identity":{
        "project_id":candidate.project_id,"item_id":candidate.item_id,"revision_id":candidate.revision_id,"content_hash":candidate.content_hash
    },"cues":["upstream cue"],"lesson":lesson})).unwrap(),"score":distance})
}
fn chunks(values: Vec<Value>) -> Value {
    json!([{"dataset_id":"00000000-0000-0000-0000-000000000001","dataset_name":"possibly-leaked-dataset","search_result":values}])
}

#[tokio::test]
async fn add_cognify_canary_flow_projects_minimal_provider_payload_only() {
    let ledger = Ledger::new();
    let provider = ledger.approve(ledger.project, "provider", true, "needle");
    ledger.approve(ledger.project, "local", false, "LOCAL_ONLY_CANARY");
    let preview = ledger.store.stage_projection(ledger.project).unwrap();
    assert_eq!(preview.entries.len(), 1);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/add"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"added":true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/cognify"))
        .and(body_json(
            json!({"datasets":[preview.snapshot.dataset],"run_in_background":false}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"00000000-0000-0000-0000-000000000001":{"status":"PipelineRunCompleted"}}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST")).and(path("/api/v1/search"))
        .and(body_json(json!({"query":preview.entries[0].lesson,"search_type":"CHUNKS","datasets":[preview.snapshot.dataset],"top_k":64})))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunks(vec![chunk(&provider,"UPSTREAM_CHANGED_LESSON",0.4)]))).expect(1).mount(&server).await;
    let configured = Client::new(
        Config {
            enabled: true,
            endpoint: Some(server.uri()),
            credential_alias: Some("fixture-transport".into()),
            ..Config::default()
        },
        Some(secrecy::SecretString::from("SYNTHETIC_BEARER_CANARY")),
    )
    .unwrap();
    let qualified = configured.qualify(&preview).await.unwrap();
    assert!(qualified.added && qualified.cognified && qualified.canary_passed);
    assert_eq!(qualified.digest, preview.snapshot.digest);
    assert!(
        ledger
            .store
            .projection_readback(ledger.project)
            .unwrap()
            .active
            .is_none(),
        "transport does not activate"
    );
    let requests = server.received_requests().await.unwrap();
    let body = String::from_utf8(requests[0].body.clone()).unwrap();
    assert_eq!(
        requests[0].headers.get("authorization").unwrap(),
        "Bearer SYNTHETIC_BEARER_CANARY"
    );
    assert!(!body.contains("SYNTHETIC_BEARER_CANARY"));
    assert!(body.contains("name=\"datasetName\"") && body.contains(&preview.snapshot.dataset));
    assert!(body.contains("name=\"data\"") && body.contains(provider.content_hash.as_str()));
    assert!(!body.contains("LOCAL_ONLY_CANARY"));
    for forbidden in [
        "evidence_refs",
        "went_well",
        "actions",
        "approval",
        "proposed_by",
    ] {
        assert!(!body.contains(forbidden));
    }
}

#[tokio::test]
async fn authoritative_rehydration_rejects_leakage_and_never_trusts_cognee_text() {
    let ledger = Ledger::new();
    let valid = ledger.approve(ledger.project, "valid", true, "needle");
    let foreign = ledger.approve(ledger.foreign, "foreign", true, "needle");
    let local = ledger.approve(ledger.project, "local", false, "needle");
    let pending = ledger.propose(ledger.project, "pending", true, "needle");
    let tomb = ledger.approve(ledger.project, "tomb", true, "needle");
    ledger
        .store
        .tombstone_memory(ledger.project, "tomb", 2, "reviewer", "retired")
        .unwrap();
    let stale = ledger.approve(ledger.project, "stale", true, "needle");
    let mut updated: Value = serde_json::from_str(include_str!(
        "../../kontor-core/tests/fixtures/experience-v1.json"
    ))
    .unwrap();
    updated["projection_policy"] = json!("provider_eligible");
    updated["future_cues"] = json!(["revision replaced"]);
    let (updated, _) = ledger
        .store
        .propose_experience(
            ledger.project,
            "stale",
            2,
            &CanonicalDocument::from_value(&updated).unwrap(),
            &MemoryProvenance {
                source: "synthetic".into(),
                source_id: None,
                legacy_last_write_wins: false,
                history_unavailable: false,
            },
            "author",
        )
        .unwrap();
    ledger
        .store
        .approve_memory_revision(ledger.project, "stale", &updated.revision_id, 3, "reviewer")
        .unwrap();
    let generic_doc =
        CanonicalDocument::from_value(&json!({"schema_version":1,"text":"generic needle"}))
            .unwrap();
    let (generic, _) = ledger
        .store
        .propose_memory_revision(
            ledger.project,
            "generic",
            0,
            &generic_doc,
            &MemoryProvenance {
                source: "synthetic".into(),
                source_id: None,
                legacy_last_write_wins: false,
                history_unavailable: false,
            },
            "author",
        )
        .unwrap();
    ledger
        .store
        .approve_memory_revision(
            ledger.project,
            "generic",
            &generic.revision_id,
            1,
            "reviewer",
        )
        .unwrap();
    let generic = MemoryCandidate {
        project_id: ledger.project,
        item_id: "generic".into(),
        revision_id: generic.revision_id,
        content_hash: generic_doc.hash().clone(),
        score: 1.0,
    };
    let preview = ledger.active();
    let mut mismatched = valid.clone();
    mismatched.content_hash = kontor_core::id::ContentHash::of(b"wrong-hash");
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunks(vec![
            chunk(&foreign, "FOREIGN_CANARY", 0.0),
            chunk(&local, "LOCAL_COGNEE_CANARY", 0.1),
            chunk(&mismatched, "HASH_CANARY", 0.2),
            chunk(&valid, "COGNEE_TEXT_CANARY", 0.3),
            chunk(&valid, "DUPLICATE_CANARY", 0.4),
            chunk(&pending, "PENDING_CANARY", 0.0),
            chunk(&tomb, "TOMBSTONE_CANARY", 0.0),
            chunk(&stale, "STALE_REVISION_CANARY", 0.0),
            chunk(&generic, "GENERIC_CANARY", 0.0),
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let recalled = client(&server, 1500)
        .recall(&preview.snapshot, &intent("needle"), |semantic| {
            ledger.store.recall_experiences(
                ledger.project,
                "rehydrated",
                &intent("needle"),
                semantic,
            )
        })
        .await
        .unwrap();
    assert_eq!(recalled.metadata.mode, RetrievalMode::Semantic);
    assert_eq!(recalled.metadata.exclusions.invalid, 7);
    assert_eq!(recalled.metadata.exclusions.duplicate, 1);
    assert_eq!(recalled.metadata.identities.len(), 1);
    let authoritative = ledger
        .store
        .memory_history(ledger.project, "valid")
        .unwrap()[0]
        .document
        .json()
        .to_owned();
    assert_eq!(recalled.canonical_block, format!("[{authoritative}]"));
    for forbidden in [
        "COGNEE_TEXT_CANARY",
        "FOREIGN_CANARY",
        "LOCAL_COGNEE_CANARY",
        "HASH_CANARY",
        "DUPLICATE_CANARY",
    ] {
        assert!(!recalled.canonical_block.contains(forbidden));
    }
}

#[tokio::test]
async fn malformed_empty_foreign_timeout_and_http_failure_degrade_with_no_corpus_fallback() {
    let ledger = Ledger::new();
    ledger.approve(ledger.project, "lexical", false, "needle");
    ledger.approve(ledger.project, "unrelated", false, "UNRELATED_CANARY");
    let foreign = ledger.approve(ledger.foreign, "foreign", true, "needle");
    let preview = ledger.active();
    let cases = [
        (
            ResponseTemplate::new(200).set_body_string("malformed"),
            DegradedReason::Malformed,
        ),
        (
            ResponseTemplate::new(200).set_body_json(json!([])),
            DegradedReason::Empty,
        ),
        (
            ResponseTemplate::new(200).set_body_json(chunks(vec![chunk(
                &foreign,
                "FOREIGN_CANARY",
                0.1,
            )])),
            DegradedReason::NoEligibleCandidates,
        ),
        (
            ResponseTemplate::new(200)
                .set_body_json(json!([]))
                .set_delay(std::time::Duration::from_millis(200)),
            DegradedReason::Timeout,
        ),
        (
            ResponseTemplate::new(503).set_body_string("password=UPSTREAM_NO_LOG_CANARY"),
            DegradedReason::Unavailable,
        ),
        (
            ResponseTemplate::new(200).set_body_string("x".repeat(MAX_RESPONSE_BYTES + 1)),
            DegradedReason::Malformed,
        ),
        (
            ResponseTemplate::new(200)
                .set_body_json(chunks(vec![json!({"text":"garbled", "score":0.1})])),
            DegradedReason::Malformed,
        ),
        (
            ResponseTemplate::new(200)
                .set_body_json(chunks(vec![json!({"text":"{}", "score":"NaN"})])),
            DegradedReason::Malformed,
        ),
        (
            ResponseTemplate::new(200).set_body_json(chunks(vec![chunk(&foreign, "x", 0.1); 65])),
            DegradedReason::Malformed,
        ),
    ];
    for (index, (response, reason)) in cases.into_iter().enumerate() {
        let server = MockServer::start().await;
        Mock::given(path("/api/v1/search"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let timeout = if reason == DegradedReason::Timeout {
            50
        } else {
            1500
        };
        let started = std::time::Instant::now();
        let recalled = client(&server, timeout)
            .recall(&preview.snapshot, &intent("needle"), |semantic| {
                ledger.store.recall_experiences(
                    ledger.project,
                    &format!("degraded-{index}"),
                    &intent("needle"),
                    semantic,
                )
            })
            .await
            .unwrap();
        assert_eq!(recalled.metadata.reason, Some(reason));
        assert_eq!(recalled.metadata.mode, RetrievalMode::LexicalDegraded);
        assert_eq!(recalled.metadata.identities.len(), 1);
        assert_eq!(recalled.metadata.identities[0].item_id, "lexical");
        assert!(!recalled.canonical_block.contains("UNRELATED_CANARY"));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}

#[tokio::test]
async fn projection_failure_stops_each_phase_without_touching_active_pointer() {
    let ledger = Ledger::new();
    let old = ledger.approve(ledger.project, "old", true, "needle");
    let previous = ledger.active();
    ledger.approve(ledger.project, "new", true, "needle");
    let preview = ledger.store.stage_projection(ledger.project).unwrap();
    for phase in ["add", "cognify", "canary", "pending"] {
        let server = MockServer::start().await;
        Mock::given(path("/api/v1/add"))
            .respond_with(if phase == "add" {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(json!({}))
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/api/v1/cognify")).respond_with(if phase=="cognify" {ResponseTemplate::new(503)} else {ResponseTemplate::new(200).set_body_json(json!({"run":{"status":if phase=="pending" {"PipelineRunStarted"} else {"PipelineRunCompleted"}}}))}).expect(if phase=="add" {0} else {1}).mount(&server).await;
        Mock::given(path("/api/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chunks(vec![])))
            .expect(if phase == "canary" { 1 } else { 0 })
            .mount(&server)
            .await;
        assert!(client(&server, 1500).qualify(&preview).await.is_err());
        assert_eq!(
            ledger
                .store
                .projection_readback(ledger.project)
                .unwrap()
                .active
                .unwrap()
                .digest,
            previous.snapshot.digest
        );
    }
    assert_eq!(old.item_id, "old");
}

#[test]
fn configuration_is_disabled_by_default_and_refusals_are_static() {
    assert!(!Config::default().enabled);
    for config in [
        Config {
            enabled: true,
            ..Config::default()
        },
        Config {
            timeout_ms: 1501,
            ..Config::default()
        },
        Config {
            endpoint: Some("http://user:SECRET@localhost".into()),
            ..Config::default()
        },
        Config {
            endpoint: Some("http://remote.example".into()),
            ..Config::default()
        },
        Config {
            dataset_prefix: "foreign".into(),
            ..Config::default()
        },
    ] {
        let error = config.validate().unwrap_err();
        assert_eq!(error.to_string(), "cognee_malformed");
    }
    let config = Config {
        enabled: true,
        endpoint: Some("http://127.0.0.1:1".into()),
        credential_alias: Some("fixture".into()),
        ..Config::default()
    };
    assert_eq!(
        Client::new(config, None).unwrap_err().to_string(),
        "cognee_unavailable"
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory-cognee.json");
    assert!(Config::read(&path).unwrap().is_none());
    std::fs::write(&path, b"{}").unwrap();
    assert!(!Config::read(&path).unwrap().unwrap().enabled);
    for body in ["{\"unrecognized\":true}".into(), "x".repeat(8193)] {
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            Config::read(&path).unwrap_err().to_string(),
            "cognee_malformed"
        );
    }
}
