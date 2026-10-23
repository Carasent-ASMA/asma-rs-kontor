//! ASMA-8454: one disposable realm answers HTTP, MCP and the generated CLI.
//! Only an in-process scripted adapter is registered; no runtime service,
//! native session, Jira connection or durable seat is configured.

use assert_cmd::Command;
use kontor_api::state::RuntimeRegistry;
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_mcp::{CallerTier, connect};
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::ScriptedFakeRuntime;
use serde_json::{Value, json};

fn credential(root: &std::path::Path) -> String {
    let value: Value = serde_json::from_slice(
        &std::fs::read(root.join("credentials.json")).expect("fixture credentials"),
    )
    .expect("credentials JSON");
    value["admin"].as_str().expect("admin").to_owned()
}

async fn post(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    path: &str,
    key: &str,
    body: &Value,
) -> (u16, Value) {
    let response = client
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .header("idempotency-key", key)
        .json(body)
        .send()
        .await
        .expect("fixture HTTP");
    (
        response.status().as_u16(),
        response.json().await.expect("JSON"),
    )
}

async fn revision(client: &reqwest::Client, base: &str, token: &str, project: &str) -> u64 {
    let body: Value = client
        .get(format!("{base}/v1/projects/{project}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("project")
        .json()
        .await
        .expect("JSON");
    body["revision"].as_u64().expect("project revision")
}

fn cli(
    root: &std::path::Path,
    base: &str,
    project: &str,
    key: &str,
    target: &str,
    revision: &str,
) -> (i32, Value) {
    let output = Command::cargo_bin("kontor")
        .expect("built CLI")
        .env_remove("KONTOR_AUTH")
        .args([
            "--state-root",
            root.to_str().expect("path"),
            "--base-url",
            base,
            "--tier",
            "admin",
            "topology-ensure",
            "--project-id",
            project,
            "--idempotency-key",
            key,
            "--target",
            target,
            "--expected-revision",
            revision,
        ])
        .output()
        .expect("CLI runs");
    (
        output.status.code().expect("exit code"),
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("CLI JSON: {}", String::from_utf8_lossy(&output.stderr))),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desk_ensure_and_undeclared_refusal_match_http_mcp_and_cli() {
    let root = tempfile::tempdir().expect("disposable realm");
    let fake = std::sync::Arc::new(ScriptedFakeRuntime::new(RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }));
    let daemon = Daemon::start(
        DaemonConfig::at(root.path()).with_port(0),
        RuntimeRegistry::new().with(
            kontor_core::id::RuntimeKindKey::parse("fake.runtime").expect("fixture family"),
            fake.clone(),
        ),
    )
    .expect("fixture daemon");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("isolated loopback");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let router = daemon.router();
    let server =
        tokio::spawn(async move { axum::serve(listener, router).await.expect("fixture server") });
    let token = credential(root.path());
    let client = reqwest::Client::new();
    let (status, created) = post(
        &client,
        &base,
        &token,
        "/v1/projects:ensure",
        "parity-project",
        &json!({
            "name":"Qualification surface", "root_path":root.path().join("repository"),
            "memory_origin":"kontor_native", "backlog_origin":"kontor_native"
        }),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let project = created["project_id"].as_str().expect("project");
    // Reading a semantic target publishes the bundled immutable revisions.
    // The initial desk-less selection refuses before creating any desk.
    let (_, initial) = post(
        &client,
        &base,
        &token,
        &format!("/v1/projects/{project}/topology:ensure"),
        "parity-initial-refusal",
        &json!({
            "target":{"scope":"desk","desk_key":"not-declared"},
            "expected_revision":revision(&client,&base,&token,project).await
        }),
    )
    .await;
    assert_eq!(initial["code"], "placement_blocked", "{initial}");
    let published = client
        .get(format!("{base}/v1/projects/{project}/team-definitions"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("bundled publication read");
    assert!(published.status().is_success());
    let domain = kontor_profiles::bundled_operational_domain().expect("shipped definitions");
    // Selection requires a published domain, even when no native seat is configured.
    let catalog = &domain.role_catalogs[0];
    let core_team = kontor_daemon::orchestration::propose_core_team(catalog, &domain)
        .expect("fixture core team");
    let orchestration =
        kontor_daemon::orchestration::propose_orchestration().expect("fixture orchestration");
    let mut bundle = kontor_daemon::orchestration::resolve_bundle(
        &kontor_daemon::orchestration::BundleSources {
            orchestration: &orchestration,
            fleet: include_str!("../../../config/examples/orchestration/fleet.yml"),
            core_team: &core_team,
        },
        catalog,
        &domain,
    )
    .expect("fixture bundle");
    let document = serde_json::to_string(&domain).expect("JSON is valid YAML");
    let pin = kontor_fleet_activation::OperationalDomainPin {
        source: "packs/parity-domain.yml".to_owned(),
        source_hash: kontor_core::id::ContentHash::of(document.as_bytes()),
    };
    bundle
        .manifest
        .sources
        .insert(pin.source.clone(), pin.source_hash.clone());
    bundle.manifest.operational_domain = Some(pin);
    bundle.operational_domain = Some(document);
    let source = kontor_daemon::fleet::FleetSource::at(root.path());
    let published = source
        .publish_bundle(&bundle)
        .expect("publish fixture domain");
    source
        .activate_bundle(
            &published.bundle_hash,
            &kontor_daemon::fleet::ActivationFence::default(),
        )
        .expect("activate fixture domain");
    let definition = domain
        .team_definitions
        .iter()
        .filter(|spec| !spec.desks.is_empty())
        .max_by_key(|spec| spec.version)
        .expect("desk successor");
    let (status, preview) = post(
        &client,
        &base,
        &token,
        &format!("/v1/projects/{project}/team-definition-selection:preview"),
        "parity-preview",
        &json!({
            "target_definition":{"id":definition.definition_id,"version":definition.version}
        }),
    )
    .await;
    assert_eq!(status, 200, "{preview}");
    let (status, selected) = post(
        &client,
        &base,
        &token,
        &format!("/v1/projects/{project}/team-definition-selection:apply"),
        "parity-select",
        &json!({
            "preview_hash":preview["preview_hash"],
            "expected_revision":revision(&client,&base,&token,project).await
        }),
    )
    .await;
    assert_eq!(status, 200, "{selected}");
    let dispatcher = connect(root.path(), Some(&base), CallerTier::Admin).expect("fixture MCP");
    for (desk, success) in [("adam", true), ("not-declared", false)] {
        let key = format!("parity-{desk}");
        let target = json!({"scope":"desk", "desk_key":desk});
        let expected = revision(&client, &base, &token, project).await;
        let body = json!({"target":target,"expected_revision":expected});
        let path = format!("/v1/projects/{project}/topology:ensure");
        let (status, first) = post(&client, &base, &token, &path, &key, &body).await;
        assert_eq!(status == 200, success, "{first}");
        let (_, http) = post(&client, &base, &token, &path, &key, &body).await;
        let mcp = dispatcher.call("kontor_topology_ensure", &json!({
            "project_id":project,"idempotency_key":key,"target":target,"expected_revision":expected
        })).await.expect("MCP envelope");
        let (exit, shell) = cli(
            root.path(),
            &base,
            project,
            &key,
            &target.to_string(),
            &expected.to_string(),
        );
        assert_eq!(mcp.body, http);
        assert_eq!(shell["body"], http);
        assert_eq!(exit == 0, success, "{shell}");
        if success {
            assert_eq!(http["receipt"]["applied"], "unchanged");
        } else {
            assert_eq!(http["code"], "placement_blocked");
            assert_eq!(exit, 3, "placement requires an explicit operator decision");
            assert_eq!(revision(&client, &base, &token, project).await, expected);
            println!("ASMA-8454 undeclared desk CLI refusal exit: {exit}");
        }
    }
    server.abort();
    assert_eq!(
        fake.minted_natives(),
        0,
        "ensure is a logical-only operation"
    );
    daemon.shutdown();
}
