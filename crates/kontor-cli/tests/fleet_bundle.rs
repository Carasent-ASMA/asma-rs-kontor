//! ASMA-8280 S-1/S-2: the orchestration bundle operations are ordinary
//! registered commands. An operator proposes, previews, publishes and
//! activates a bundle through a live loopback Realm with the generated CLI,
//! and the daemon-free local resolution then reads exactly what was activated.

use assert_cmd::Command;
use kontor_api::state::RuntimeRegistry;
use kontor_daemon::{Daemon, DaemonConfig};

fn kontor(
    root: &std::path::Path,
    base: Option<&str>,
    tier: &str,
    args: &[&str],
) -> (i32, serde_json::Value) {
    let mut command = Command::cargo_bin("kontor").expect("CLI binary");
    // The fixture's own tier file decides authority, not an inherited seat
    // credential for another Realm.
    command
        .env_remove("KONTOR_AUTH")
        .args(["--state-root", root.to_str().expect("UTF-8 root")])
        .args(["--tier", tier]);
    if let Some(base) = base {
        command.args(["--base-url", base]);
    }
    let output = command.args(args).output().expect("CLI runs");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    let document = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("one JSON document, got {stdout:?}: {error}"));
    (output.status.code().expect("an exit code"), document)
}

fn policy(roster: &str) -> String {
    format!(
        "schema_version: 2\n\
         domains:\n  codex: {{ provider: codex, accounts: [codex-work] }}\n\
         models:\n  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}\n\
         chains:\n  lead:\n    - [sol@xhigh]\n\
         bindings:\n  leadership/{roster}/lsa: lead\n  leadership/{roster}/tpm: lead\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bundle_is_proposed_published_and_activated_through_the_generated_cli() {
    let root = tempfile::tempdir().expect("state root");
    let daemon = Daemon::start(
        DaemonConfig::at(root.path()).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("daemon starts");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback binds");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let router = daemon.router();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server runs");
    });
    let root_path = root.path().to_path_buf();
    let base_url = base.clone();
    let run = move |tier: &'static str, args: Vec<String>| {
        let root = root_path.clone();
        let base = base_url.clone();
        tokio::task::spawn_blocking(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            kontor(&root, Some(&base), tier, &args)
        })
    };

    // The bundle family is admin configuration.
    let (exit, refused) = run("operator", vec!["fleet-bundle-propose".into()])
        .await
        .expect("CLI task");
    assert_eq!(exit, 3, "{refused}");
    let (exit, proposal) = run("admin", vec!["fleet-bundle-propose".into()])
        .await
        .expect("CLI task");
    assert_eq!(exit, 0, "{proposal}");
    let orchestration = proposal["body"]["orchestration"]
        .as_str()
        .expect("orchestration")
        .to_owned();
    let core_team = proposal["body"]["core_team"]
        .as_str()
        .expect("core team")
        .to_owned();
    assert!(proposal["body"]["role_catalog"]["content_hash"].is_string());

    let preview = |fleet: String| {
        vec![
            "fleet-bundle-preview".to_owned(),
            "--orchestration".to_owned(),
            orchestration.clone(),
            "--fleet".to_owned(),
            fleet,
            "--core-team".to_owned(),
            core_team.clone(),
        ]
    };
    // A first preview learns the exact roster the proposal resolves to.
    let (exit, probe) = run("admin", preview(policy("0".repeat(64).as_str())))
        .await
        .expect("CLI task");
    assert_eq!(exit, 0, "{probe}");
    let roster = probe["body"]["manifest"]["core_team_revision_hash"]
        .as_str()
        .expect("the roster")
        .to_owned();
    let fleet = policy(&roster);
    let (exit, previewed) = run("admin", preview(fleet.clone()))
        .await
        .expect("CLI task");
    assert_eq!(exit, 0, "{previewed}");
    let preview_hash = previewed["body"]["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();

    let mut publish = preview(fleet.clone());
    publish[0] = "fleet-bundle-publish".to_owned();
    publish.extend([
        "--preview-hash".to_owned(),
        preview_hash,
        "--idempotency-key".to_owned(),
        "cli-bundle-publish".to_owned(),
    ]);
    let (exit, published) = run("admin", publish).await.expect("CLI task");
    assert_eq!(exit, 0, "{published}");
    assert_eq!(published["body"]["applied"], "created");
    let bundle = published["body"]["manifest"]["source_bundle_hash"]
        .as_str()
        .expect("a bundle")
        .to_owned();

    let (exit, activated) = run(
        "admin",
        vec![
            "fleet-bundle-activate".into(),
            "--source-bundle-hash".into(),
            bundle.clone(),
            "--idempotency-key".into(),
            "cli-bundle-activate".into(),
        ],
    )
    .await
    .expect("CLI task");
    assert_eq!(exit, 0, "{activated}");
    assert_eq!(
        activated["body"]["activation"]["source_bundle_hash"],
        bundle.as_str()
    );

    let (exit, read) = run("admin", vec!["fleet-bundle-get".into()])
        .await
        .expect("CLI task");
    assert_eq!(exit, 0, "{read}");
    assert_eq!(read["body"]["activation_schema_version"], 2);
    assert_eq!(
        read["body"]["manifest"]["source_bundle_hash"],
        bundle.as_str()
    );

    // The local operation reads the same activation with no daemon at all.
    let key = format!("leadership/{roster}/lsa");
    let (exit, resolved) = tokio::task::spawn_blocking({
        let root = root.path().to_path_buf();
        move || {
            kontor(
                &root,
                None,
                "operator",
                &["fleet-policy-resolve", "--binding-key", &key],
            )
        }
    })
    .await
    .expect("CLI task");
    assert_eq!(exit, 0, "{resolved}");
    assert_eq!(
        resolved["body"]["provenance"]["policy_hash"],
        activated["body"]["activation"]["policy_hash"]
    );
    assert_eq!(
        resolved["body"]["selected"]["rung"]["provider"],
        "codex-work"
    );
    server.abort();
}
