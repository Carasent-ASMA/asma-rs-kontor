//! Native-memory HTTP and CLI parity against one real loopback Realm.

use assert_cmd::Command;
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{CanonicalDocument, ContentHash, ExternalName, ProjectId, Timestamp};
use kontor_core::repository::{NewProject, ProjectRepository, RepositoryError};
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_store::memory::{AgentsRoomExport, MemoryProvenance};

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
            store.create_project(&NewProject {
                id: project,
                name: ExternalName::parse("Memory parity").expect("name"),
                root_path: ExternalName::parse("/tmp/memory-parity").expect("path"),
                created_at: Timestamp::now(),
            })?;
            let mut export = AgentsRoomExport {
                schema_version: 1,
                source: "agentsroom".to_owned(),
                project_id: project,
                entries: Vec::new(),
                export_hash: ContentHash::of(b"pending"),
            };
            export.export_hash = export.calculate_hash().expect("hash");
            store.freeze_agentsroom_writes().expect("freeze");
            store
                .apply_agentsroom_import(&export)
                .expect("empty import");
            store
                .switch_memory_authority(project, "agentsroom", &export.export_hash)
                .expect("switch");
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
