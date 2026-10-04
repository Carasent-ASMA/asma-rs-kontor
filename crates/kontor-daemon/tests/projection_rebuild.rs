//! ASMA-8159: composed application/HTTP qualification, synthetic material only.
#[allow(dead_code)]
mod harness;

use async_trait::async_trait;
use harness::{Call, World};
use kontor_api::memory::ProjectionRebuildRequest;
use kontor_core::id::{CanonicalDocument, IdempotencyKey};
use kontor_core::memory::DegradedReason;
use kontor_daemon::applications::ProjectionQualifier;
use kontor_memory_cognee::{Client, Config, Error};
use kontor_store::memory::{MemoryProvenance, ProjectionPreview, ProjectionQualification};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn client(server: &MockServer, timeout_ms: u64) -> Client {
    Client::new(
        Config {
            enabled: true,
            endpoint: Some(server.uri()),
            credential_alias: Some("synthetic-cognee".into()),
            timeout_ms,
            ..Config::default()
        },
        Some(SecretString::from("SYNTHETIC_COMPOSITION_CANARY")),
    )
    .unwrap()
}

fn seed(world: &World, item: &str, provider: bool) {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../kontor-core/tests/fixtures/experience-v1.json"
    ))
    .unwrap();
    value["projection_policy"] = json!(if provider {
        "provider_eligible"
    } else {
        "local_only"
    });
    if !provider {
        value["lesson"] = json!("LOCAL_ONLY_COMPOSITION_CANARY");
    }
    let document = CanonicalDocument::from_value(&value).unwrap();
    world.daemon.state().with_store(|store| {
        let (revision, _) = store
            .propose_experience(
                world.project,
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
            .approve_memory_revision(world.project, item, &revision.revision_id, 1, "reviewer")
            .unwrap();
    });
}

fn preview(world: &World) -> ProjectionPreview {
    world
        .daemon
        .state()
        .with_store(|store| store.projection_preview(world.project))
        .unwrap()
}
fn input(preview: &ProjectionPreview) -> Value {
    json!({ "expected_generation": preview.active_generation,
        "expected_memory_cursor": preview.snapshot.memory_cursor, "preview_digest": preview.snapshot.digest })
}
fn pointer(world: &World) -> Value {
    serde_json::to_value(
        world
            .daemon
            .state()
            .with_store(|store| store.projection_readback(world.project))
            .unwrap(),
    )
    .unwrap()
}
async fn rebuild(world: &World, body: &Value, key: &str) -> harness::Answer {
    Call::post(
        format!("/v1/projects/{}/memory/projection:rebuild", world.project),
        body,
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await
}
async fn mocks(server: &MockServer, preview: &ProjectionPreview, good: bool, delay: Duration) {
    Mock::given(method("POST"))
        .and(path("/api/v1/add"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"added":true})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/cognify"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"synthetic-run":{"status":"PipelineRunCompleted"}})),
        )
        .mount(server)
        .await;
    let identity = &preview.entries[0].identity;
    let chunks = if good {
        vec![
            json!({"score":0.1,"text":serde_json::to_string(&json!({"identity":identity,"cues":["canary"],"lesson":"UPSTREAM_NEVER_CANONICAL"})).unwrap()}),
        ]
    } else {
        vec![]
    };
    Mock::given(method("POST"))
        .and(path("/api/v1/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{"search_result":chunks}]))
                .set_delay(delay),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn composed_http_success_local_exclusion_idempotent_replay_and_restart() {
    let server = MockServer::start().await;
    let world = World::open_with_memory_cognee(client(&server, 1500)).await;
    seed(&world, "provider", true);
    seed(&world, "local", false);
    let staged = preview(&world);
    assert_eq!(staged.entries.len(), 1);
    mocks(&server, &staged, true, Duration::ZERO).await;
    let body = input(&staged);
    let denied = Call::post(
        format!("/v1/projects/{}/memory/projection:rebuild", world.project),
        &body,
    )
    .signed_as(&world, "observer")
    .with_key("observer")
    .send(&world)
    .await;
    assert_eq!(denied.status, 403);
    let first = rebuild(&world, &body, "composed-success").await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["projection"]["generation"], 1);
    assert_eq!(
        first.json()["projection"]["active"]["digest"],
        json!(staged.snapshot.digest)
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.headers.get("authorization").unwrap()
                == "Bearer SYNTHETIC_COMPOSITION_CANARY")
    );
    let add = String::from_utf8_lossy(&requests[0].body);
    assert!(add.contains(staged.snapshot.dataset.as_str()));
    assert!(!add.contains("LOCAL_ONLY_COMPOSITION_CANARY"));
    assert!(!first.body.contains("SYNTHETIC_COMPOSITION_CANARY"));
    // A new ledger head cannot substitute a current readback for the original response.
    seed(&world, "later-provider", true);
    let replay = rebuild(&world, &body, "composed-success").await;
    assert_eq!(replay.json(), first.json());
    let conflict = rebuild(&world, &input(&preview(&world)), "composed-success").await;
    assert_eq!(conflict.status, 409);
    assert_eq!(conflict.json()["code"], "binding_conflict");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    // Ordinary startup with no client can still replay durable completed results.
    let World {
        directory,
        daemon,
        router,
        fake,
        project,
        task,
        team_run,
    } = world;
    drop(router);
    drop(daemon);
    let daemon = kontor_daemon::Daemon::start(
        kontor_daemon::DaemonConfig::at(directory.path()).with_port(0),
        kontor_api::state::RuntimeRegistry::new(),
    )
    .unwrap();
    let world = World {
        router: daemon.router(),
        directory,
        daemon,
        fake,
        project,
        task,
        team_run,
    };
    assert_eq!(
        rebuild(&world, &body, "composed-success").await.json(),
        first.json()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn canary_failure_and_transport_failures_preserve_prior_active_pointer() {
    for failure in ["canary", "add", "cognify", "malformed", "timeout"] {
        let server = MockServer::start().await;
        let world = World::open_with_memory_cognee(client(&server, 200)).await;
        seed(&world, "provider", true);
        let initial = preview(&world);
        mocks(&server, &initial, true, Duration::ZERO).await;
        assert_eq!(
            rebuild(&world, &input(&initial), "prior-active")
                .await
                .status,
            200
        );
        let before = pointer(&world);
        server.reset().await;
        mocks(
            &server,
            &preview(&world),
            failure != "canary",
            if failure == "timeout" {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            },
        )
        .await;
        let route = match failure {
            "add" => "/api/v1/add",
            "cognify" => "/api/v1/cognify",
            _ => "/api/v1/search",
        };
        if matches!(failure, "add" | "cognify" | "malformed") {
            Mock::given(path(route))
                .respond_with(if failure == "malformed" {
                    ResponseTemplate::new(200).set_body_string("SYNTHETIC_UPSTREAM_ERROR_CANARY")
                } else {
                    ResponseTemplate::new(503).set_body_string("SYNTHETIC_UPSTREAM_ERROR_CANARY")
                })
                .with_priority(1)
                .mount(&server)
                .await;
        }
        let failed = rebuild(&world, &input(&preview(&world)), failure).await;
        assert_eq!(failed.status, 503, "{failure}: {}", failed.body);
        assert_eq!(failed.json()["code"], "projection_unavailable");
        assert!(!failed.body.contains("SYNTHETIC_UPSTREAM_ERROR_CANARY"));
        assert_eq!(pointer(&world), before, "{failure} cannot activate");
    }
}

#[tokio::test]
async fn stale_preview_is_refused_before_egress_and_inflight_ledger_change_conflicts() {
    let server = MockServer::start().await;
    let world = World::open_with_memory_cognee(client(&server, 1500)).await;
    seed(&world, "provider", true);
    let old = preview(&world);
    seed(&world, "second", true);
    let refused = rebuild(&world, &input(&old), "stale").await;
    assert_eq!(refused.status, 409);
    assert!(server.received_requests().await.unwrap().is_empty());
    let current = preview(&world);
    mocks(&server, &current, true, Duration::from_millis(150)).await;
    let body = input(&current);
    let pending = rebuild(&world, &body, "inflight");
    let change = async {
        tokio::time::timeout(Duration::from_secs(1), async {
            while server.received_requests().await.unwrap().len() < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // This write succeeds during the network await: the store mutex is released.
        seed(&world, "inflight-new-head", true);
    };
    let (answer, ()) = tokio::join!(pending, change);
    assert_eq!(answer.status, 409, "{}", answer.body);
    assert_eq!(answer.json()["code"], "projection_conflict");
    assert_eq!(pointer(&world)["generation"], 0);
}

#[tokio::test]
async fn concurrent_same_key_replays_once_and_distinct_keys_use_late_cas() {
    for same_key in [true, false] {
        let server = MockServer::start().await;
        let world = World::open_with_memory_cognee(client(&server, 1500)).await;
        seed(&world, "provider", true);
        let staged = preview(&world);
        let body = input(&staged);
        mocks(&server, &staged, true, Duration::from_millis(100)).await;
        let (a, b) = tokio::join!(
            rebuild(&world, &body, "contender-a"),
            rebuild(
                &world,
                &body,
                if same_key {
                    "contender-a"
                } else {
                    "contender-b"
                }
            )
        );
        if same_key {
            assert_eq!(a.status, 200);
            assert_eq!(a.json(), b.json());
        } else {
            let mut statuses = [a.status, b.status];
            statuses.sort();
            assert_eq!(statuses, [200, 409]);
        }
        assert_eq!(pointer(&world)["generation"], 1);
    }
}

struct FailedCanary;
#[tokio::test]
async fn result_receipt_failure_rolls_back_activation_and_retry_commits_once() {
    let server = MockServer::start().await;
    let world = World::open_with_memory_cognee(client(&server, 1500)).await;
    seed(&world, "provider", true);
    let staged = preview(&world);
    mocks(&server, &staged, true, Duration::ZERO).await;
    let database =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    database.execute_batch("CREATE TRIGGER synthetic_result_failure BEFORE INSERT ON memory_projection_rebuild_results BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
    let failed = rebuild(&world, &input(&staged), "atomic-retry").await;
    assert_eq!(failed.status, 400, "{}", failed.body);
    assert_eq!(
        pointer(&world)["generation"],
        0,
        "a result failure must roll back the pointer"
    );
    database
        .execute_batch("DROP TRIGGER synthetic_result_failure;")
        .unwrap();
    let succeeded = rebuild(&world, &input(&staged), "atomic-retry").await;
    assert_eq!(succeeded.status, 200, "{}", succeeded.body);
    assert_eq!(pointer(&world)["generation"], 1);
    assert_eq!(
        rebuild(&world, &input(&staged), "atomic-retry")
            .await
            .json(),
        succeeded.json()
    );
    for table in [
        "memory_projection_rebuild_keys",
        "memory_projection_rebuild_results",
    ] {
        let count: i64 = database
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        assert!(
            database
                .execute(&format!("DELETE FROM {table}"), [])
                .is_err()
        );
    }
}

#[async_trait]
impl ProjectionQualifier for FailedCanary {
    async fn qualify(&self, preview: &ProjectionPreview) -> Result<ProjectionQualification, Error> {
        // Explicit adverse certificate permitted by the LSA; never live proof.
        Ok(ProjectionQualification {
            digest: preview.snapshot.digest.clone(),
            added: true,
            cognified: true,
            canary_passed: false,
        })
    }
}
#[tokio::test]
async fn composed_adverse_qualification_cannot_activate_before_canary() {
    let server = MockServer::start().await;
    let world = World::open_with_memory_cognee(client(&server, 1500)).await;
    seed(&world, "provider", true);
    let staged = preview(&world);
    mocks(&server, &staged, true, Duration::ZERO).await;
    assert_eq!(rebuild(&world, &input(&staged), "prior").await.status, 200);
    let before = pointer(&world);
    let request: ProjectionRebuildRequest =
        serde_json::from_value(input(&preview(&world))).unwrap();
    let result = world
        .daemon
        .jira_reconciler()
        .rebuild_memory_projection_with_qualifier(
            world.project,
            &IdempotencyKey::parse("adverse-canary").unwrap(),
            &request,
            Some(&FailedCanary),
        )
        .await;
    let after = pointer(&world);
    assert_eq!(
        after, before,
        "MUT-007: adverse canary must leave the prior active pointer unchanged"
    );
    assert!(
        result.is_err(),
        "MUT-007: application caller must refuse a failed canary"
    );
    assert_eq!(
        result.unwrap_err().code,
        kontor_api::error::ApiErrorCode::ProjectionUnavailable
    );
}

#[tokio::test]
async fn absent_and_disabled_composition_refuse_without_lookup_or_network() {
    let server = MockServer::start().await;
    for world in [
        World::open().await,
        World::open_with_memory_cognee(
            Client::new(
                Config {
                    endpoint: Some(server.uri()),
                    ..Config::default()
                },
                None,
            )
            .unwrap(),
        )
        .await,
    ] {
        seed(&world, "provider", true);
        let answer = rebuild(&world, &input(&preview(&world)), "unavailable").await;
        assert_eq!(answer.status, 503);
        assert_eq!(answer.json()["code"], "projection_unavailable");
        assert_eq!(pointer(&world)["generation"], 0);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("memory-cognee.json"),
        serde_json::to_vec(&Config {
            enabled: true,
            endpoint: Some(server.uri()),
            credential_alias: Some("UNRESOLVED_SYNTHETIC_ALIAS".into()),
            ..Config::default()
        })
        .unwrap(),
    )
    .unwrap();
    let start = kontor_daemon::Daemon::start(
        kontor_daemon::DaemonConfig::at(root.path()),
        kontor_api::state::RuntimeRegistry::new(),
    );
    assert!(matches!(
        start,
        Err(kontor_daemon::StartupError::MemoryCognee { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(
        Client::new(
            Config {
                enabled: true,
                endpoint: Some(server.uri()),
                credential_alias: Some("synthetic-cognee".into()),
                ..Config::default()
            },
            None
        )
        .is_err()
    );
    assert_eq!(
        Error(DegradedReason::Unavailable).to_string(),
        "cognee_unavailable"
    );
}
