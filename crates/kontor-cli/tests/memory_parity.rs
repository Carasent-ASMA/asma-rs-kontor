//! Native-memory HTTP and CLI parity against one real loopback Realm.

use assert_cmd::Command;
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{CanonicalDocument, ExternalName, ProjectId, Timestamp};
use kontor_core::repository::{NewProject, ProjectRepository, RepositoryError};
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_store::memory::MemoryProvenance;

fn credential(root: &std::path::Path, tier: &str) -> String {
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("credentials.json")).expect("credentials read"),
    )
    .expect("credentials parse");
    value[tier].as_str().expect("tier exists").to_owned()
}

fn cli(root: &std::path::Path, base: &str, tier: &str, args: &[&str]) -> serde_json::Value {
    let output = Command::cargo_bin("kontor")
        .expect("CLI binary")
        // This fixture exercises the named disk tier in its temporary Realm.
        // A Kontor verification seat legitimately carries a seat-scoped
        // operator credential for another Realm, and the CLI deliberately
        // prefers that credential when it is inherited. Do not let the test
        // harness substitute its caller identity for the fixture's explicit
        // `--state-root` / `--tier` contract.
        .env_remove("KONTOR_AUTH")
        .args(["--state-root", root.to_str().expect("UTF-8 root")])
        .args(["--base-url", base, "--tier", tier])
        .args(args)
        .output()
        .expect("CLI runs");
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI emits stable JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_memory_http_and_cli_share_realm_revision_and_cursor() {
    let root = tempfile::tempdir().expect("state root");
    let daemon = Daemon::start(
        DaemonConfig::at(root.path()).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("daemon starts");
    let project = ProjectId::generate();
    daemon
        .state()
        .with_store(|store| {
            // A project created here is native on both subjects, so it is
            // writable immediately. The empty export, freeze and switch this test
            // used to perform were a ceremony to earn what a fresh project now
            // has by construction.
            store.create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Memory parity").expect("name"),
                root_path: ExternalName::parse("/tmp/memory-parity").expect("path"),
                created_at: Timestamp::now(),
            })?;
            Ok::<_, RepositoryError>(())
        })
        .expect("memory realm is seeded");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback binds");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let router = daemon.router();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server runs");
    });
    let client = reqwest::Client::new();
    let operator = credential(root.path(), "operator");
    let document = CanonicalDocument::from_value(
        &serde_json::json!({"schema_version":1,"text":"CLI native memory"}),
    )
    .expect("document");
    let provenance = MemoryProvenance {
        source: "operator".to_owned(),
        source_id: None,
        legacy_last_write_wins: false,
        history_unavailable: false,
    };
    let project_text = project.to_string();
    let provenance_text = serde_json::to_string(&provenance).expect("provenance");

    let cli_proposal = cli(
        root.path(),
        &base,
        "operator",
        &[
            "memory-propose",
            "--project-id",
            &project_text,
            "--idempotency-key",
            "cli-propose",
            "--item-id",
            "cli-item",
            "--expected-revision",
            "0",
            "--document",
            document.json(),
            "--provenance",
            &provenance_text,
            "--proposed-by",
            "cli-author",
        ],
    );
    let revision_id = cli_proposal["body"]["revision"]["revision_id"]
        .as_str()
        .expect("revision id");
    cli(
        root.path(),
        &base,
        "admin",
        &[
            "memory-approve",
            "--project-id",
            &project_text,
            "--revision-id",
            revision_id,
            "--idempotency-key",
            "cli-approve",
            "--item-id",
            "cli-item",
            "--expected-revision",
            "1",
            "--approved-by",
            "cli-reviewer",
        ],
    );

    let http: serde_json::Value = client
        .get(format!(
            "{base}/v1/projects/{project}/memory/cli-item/history"
        ))
        .bearer_auth(&operator)
        .send()
        .await
        .expect("HTTP read")
        .json()
        .await
        .expect("HTTP JSON");
    let cli_read = cli(
        root.path(),
        &base,
        "observer",
        &[
            "memory-history",
            "--project-id",
            &project_text,
            "--item-id",
            "cli-item",
        ],
    );
    let cli_body = &cli_read["body"];
    assert_eq!(cli_body["realm_id"], http["realm_id"]);
    assert_eq!(cli_body["cursor"], http["cursor"]);
    assert_eq!(
        cli_body["revisions"][0]["revision"],
        http["revisions"][0]["revision"]
    );
    assert_eq!(
        cli_body["revisions"][0]["revision_id"],
        http["revisions"][0]["revision_id"]
    );

    let http_proposal = client
        .post(format!(
            "{base}/v1/projects/{project}/memory/revisions:propose"
        ))
        .bearer_auth(&operator)
        .header("Idempotency-Key", "http-propose")
        .json(&serde_json::json!({
            "item_id":"http-item", "expected_revision":0, "document":document,
            "provenance":provenance, "proposed_by":"http-author"
        }))
        .send()
        .await
        .expect("HTTP write");
    assert!(http_proposal.status().is_success());
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_memory_http_cli_and_mcp_share_proposal_projection_and_classification() {
    let root = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(
        DaemonConfig::at(root.path()).with_port(0),
        RuntimeRegistry::new(),
    )
    .unwrap();
    let project = ProjectId::generate();
    daemon
        .state()
        .with_store(|store| {
            store.create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Typed parity").unwrap(),
                root_path: ExternalName::parse("/tmp/typed-parity").unwrap(),
                created_at: Timestamp::now(),
            })
        })
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = daemon.router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let doc = CanonicalDocument::from_value(
        &serde_json::from_str::<serde_json::Value>(include_str!(
            "../../kontor-core/tests/fixtures/experience-v1.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let project_text = project.to_string();
    let provenance = serde_json::json!({"source":"synthetic","source_id":null,"legacy_last_write_wins":false,"history_unavailable":false});
    let provenance_text = provenance.to_string();
    let args = [
        "experience-propose",
        "--project-id",
        &project_text,
        "--idempotency-key",
        "typed-parity",
        "--item-id",
        "typed",
        "--expected-revision",
        "0",
        "--document",
        doc.json(),
        "--provenance",
        &provenance_text,
        "--proposed-by",
        "fixture",
    ];
    let first = cli(root.path(), &base, "operator", &args);
    let replay = cli(root.path(), &base, "operator", &args);
    assert_eq!(first, replay);
    assert_eq!(first["body"]["revision"]["approved"], false);
    let client = reqwest::Client::new();
    let operator = credential(root.path(), "operator");
    let body = serde_json::json!({"item_id":"typed","expected_revision":0,"document":doc,"provenance":provenance,"proposed_by":"fixture"});
    let http: serde_json::Value = client
        .post(format!(
            "{base}/v1/projects/{project}/memory/experiences:propose"
        ))
        .bearer_auth(&operator)
        .header("Idempotency-Key", "typed-parity")
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(http, first["body"]);
    let dispatcher =
        kontor_mcp::connect(root.path(), Some(&base), kontor_mcp::CallerTier::Observer).unwrap();
    let preview = dispatcher
        .call(
            "kontor_memory_projection_preview",
            &serde_json::json!({"project_id":project_text}),
        )
        .await
        .unwrap();
    let cli_preview = cli(
        root.path(),
        &base,
        "observer",
        &["memory-projection-preview", "--project-id", &project_text],
    );
    assert_eq!(preview.body, cli_preview["body"]);
    assert_eq!(preview.body["preview"]["entries"], serde_json::json!([]));
    cli(
        root.path(),
        &base,
        "admin",
        &[
            "memory-approve",
            "--project-id",
            &project_text,
            "--revision-id",
            first["body"]["revision"]["revision_id"].as_str().unwrap(),
            "--idempotency-key",
            "typed-approved",
            "--item-id",
            "typed",
            "--expected-revision",
            "1",
            "--approved-by",
            "reviewer",
        ],
    );
    let classification = dispatcher
        .call(
            "kontor_experience_classify",
            &serde_json::json!({"project_id":project_text}),
        )
        .await
        .unwrap();
    let cli_classification = cli(
        root.path(),
        &base,
        "observer",
        &["experience-classify", "--project-id", &project_text],
    );
    assert_eq!(classification.body, cli_classification["body"]);
    assert_eq!(
        classification.body["classifications"][0]["recall_eligible"],
        true
    );
    let preview = dispatcher
        .call(
            "kontor_memory_projection_preview",
            &serde_json::json!({"project_id":project_text}),
        )
        .await
        .unwrap();
    assert_eq!(
        preview.body["preview"]["entries"],
        serde_json::json!([]),
        "local_only never projects"
    );
    let operator_dispatcher =
        kontor_mcp::connect(root.path(), Some(&base), kontor_mcp::CallerTier::Operator).unwrap();
    let refused=operator_dispatcher.call("kontor_memory_projection_rebuild",&serde_json::json!({"project_id":project_text,"idempotency_key":"rebuild-parity","expected_generation":preview.body["preview"]["active_generation"],"expected_memory_cursor":preview.body["preview"]["snapshot"]["memory_cursor"],"preview_digest":preview.body["preview"]["snapshot"]["digest"]})).await.unwrap();
    assert_eq!(refused.status, 503);
    assert_eq!(refused.body["code"], "projection_unavailable");
    let hidden=dispatcher.call("kontor_memory_recall_preview",&serde_json::json!({"project_id":project_text,"task_id":kontor_core::id::TaskId::generate(),"query":"hidden"})).await.unwrap_err();
    assert_eq!(hidden.code(), "invalid_request");
    server.abort();
}
