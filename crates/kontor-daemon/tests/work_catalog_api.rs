//! TASK-001's authenticated read foundation, driven through the real router.
//!
//! The catalog is composed by `Daemon::start`, exactly as production composes
//! it, and the sweeps are run explicitly so the assertions are deterministic.
//! No socket is bound; `tower::ServiceExt::oneshot` exercises the real ingress
//! authentication and handlers.

#[allow(dead_code)]
mod harness;

use std::path::Path;
use std::process::Command;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness::{Call, World};
use kontor_catalog::Catalog;
use kontor_catalog::native::{
    NativeCensus, NativeCensusSource, NativeCheckout, NativePageCounts, NativeProject,
    NativeSession, NativeWorkspace,
};
use tempfile::TempDir;

#[derive(Debug)]
struct NativeSource {
    answers: std::sync::Mutex<std::collections::VecDeque<NativeCensus>>,
}

impl NativeSource {
    fn new(answers: Vec<NativeCensus>) -> Arc<Self> {
        Arc::new(Self {
            answers: std::sync::Mutex::new(answers.into()),
        })
    }
}

#[async_trait]
impl NativeCensusSource for NativeSource {
    fn runtime_instance(&self) -> &str {
        "paseo:ws://127.0.0.1:6767/ws"
    }

    fn client_id(&self) -> &str {
        "kontor-mini-1-census"
    }

    async fn observe(&self) -> NativeCensus {
        self.answers
            .lock()
            .expect("the fixture queue")
            .pop_front()
            .expect("an answer was queued")
    }
}

fn native_census() -> NativeCensus {
    let runtime = "paseo:ws://127.0.0.1:6767/ws".to_owned();
    NativeCensus {
        runtime_instance: runtime.clone(),
        client_id: "kontor-mini-1-census".to_owned(),
        server_id: Some("srv-api".to_owned()),
        server_version: Some("0.11.4".to_owned()),
        protocol_version: 1,
        permissions: Some(vec!["workspace.read".to_owned()]),
        features: BTreeMap::new(),
        include_archived: true,
        complete: true,
        error: None,
        diagnostics: Vec::new(),
        pages: NativePageCounts {
            project_pages: 1,
            workspace_pages: 1,
            session_pages: 1,
        },
        projects: vec![NativeProject {
            runtime_instance: runtime.clone(),
            project_id: "prj_desk".to_owned(),
            display_name: "DESK • ADAM".to_owned(),
            custom_name: None,
            project_key: Some("remote:desk".to_owned()),
            root_path: "/repo".to_owned(),
            project_kind: Some("git".to_owned()),
        }],
        workspaces: vec![NativeWorkspace {
            runtime_instance: runtime.clone(),
            workspace_id: "wks_adam".to_owned(),
            project_id: "prj_desk".to_owned(),
            name: "ADAM".to_owned(),
            title: None,
            directory: "/repo/adam".to_owned(),
            kind: "worktree".to_owned(),
            archiving_at: None,
            worktree_slug: None,
            project_display_name: Some("DESK • ADAM".to_owned()),
            project_custom_name: None,
            project_root_path: Some("/repo".to_owned()),
            project_kind: Some("git".to_owned()),
        }],
        sessions: vec![
            NativeSession {
                runtime_instance: runtime.clone(),
                session_id: "agt_live".to_owned(),
                workspace_id: Some("wks_adam".to_owned()),
                project_id: Some("prj_desk".to_owned()),
                project_name: Some("DESK • ADAM".to_owned()),
                workspace_name: Some("ADAM".to_owned()),
                title: Some("Implement".to_owned()),
                provider: "claude".to_owned(),
                model: Some("opus".to_owned()),
                status: "idle".to_owned(),
                archived_at: None,
                attention_reason: None,
                cwd: "/repo/adam".to_owned(),
                created_at: None,
                updated_at: None,
                last_user_message_at: None,
                parent_session_id: None,
                checkout: Some(NativeCheckout {
                    cwd: Some("/repo/adam".to_owned()),
                    branch: Some("main".to_owned()),
                    worktree_root: None,
                    is_git: Some(true),
                    is_paseo_owned_worktree: None,
                }),
            },
            NativeSession {
                runtime_instance: runtime,
                session_id: "agt_archived".to_owned(),
                workspace_id: Some("wks_adam".to_owned()),
                project_id: Some("prj_desk".to_owned()),
                project_name: Some("DESK • ADAM".to_owned()),
                workspace_name: Some("ADAM".to_owned()),
                title: Some("Implement".to_owned()),
                provider: "claude".to_owned(),
                model: Some("opus".to_owned()),
                status: "closed".to_owned(),
                archived_at: Some("2026-10-24T00:00:00Z".to_owned()),
                attention_reason: None,
                cwd: "/repo/adam".to_owned(),
                created_at: None,
                updated_at: None,
                last_user_message_at: None,
                parent_session_id: None,
                checkout: None,
            },
        ],
    }
}

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn disposable_checkout() -> TempDir {
    let directory = TempDir::new().expect("a disposable checkout");
    git(directory.path(), &["init", "-q", "-b", "main"]);
    git(
        directory.path(),
        &["config", "user.email", "api@example.invalid"],
    );
    git(
        directory.path(),
        &["config", "user.name", "Catalog API Fixture"],
    );
    std::fs::create_dir_all(directory.path().join("plans")).expect("plans directory");
    std::fs::write(
        directory.path().join("plans/api.md"),
        "---\nepic: ASMA-9990\nworkstream: kontor\n---\n# API plan\n\n```plan-graph\n{\"schema_version\":1,\"epic\":{\"jira\":\"ASMA-9990\",\"kontor_id\":null},\"tasks\":[],\"edges\":[],\"external_fences\":[]}\n```\n",
    )
    .expect("the plan");
    git(directory.path(), &["add", "-A"]);
    git(directory.path(), &["commit", "-q", "-m", "plan"]);
    directory
}

#[tokio::test]
async fn the_work_catalog_boundary_is_authenticated_and_generation_consistent() {
    let world = World::open_empty().await;
    let catalog = world
        .daemon
        .catalog()
        .expect("the daemon composes the catalog");
    let checkout = disposable_checkout();
    let settings = kontor_catalog::CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: checkout.path().to_path_buf(),
            include_github_plans: false,
        }],
        ..kontor_catalog::CatalogSettings::default()
    };
    std::fs::write(
        Catalog::settings_path(world.directory.path()),
        serde_json::to_vec_pretty(&settings).expect("settings"),
    )
    .expect("the settings are installed");
    let report = catalog.collect_once().await.expect("the sweep completes");
    assert!(report.finished);

    let anonymous = Call::get("/v1/work-catalog/status")
        .anonymous()
        .send(&world)
        .await;
    assert_eq!(anonymous.status, 401);
    assert_eq!(anonymous.code(), "unauthenticated");

    let forbidden_entries = Call::get("/v1/work-catalog/entries")
        .anonymous()
        .send(&world)
        .await;
    assert_eq!(forbidden_entries.status, 401);

    let status = Call::get("/v1/work-catalog/status")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(status.status, 200, "{}", status.body);
    let status = status.json();
    assert_eq!(status["schema_version"], 1);
    assert_eq!(status["source"], "database");
    assert_eq!(status["generation"], report.generation);
    assert_eq!(status["collection"]["state"], "complete");
    assert_eq!(status["registry"]["state"], "absent");
    assert_eq!(status["counts"]["logical_plans"], 1);
    assert_eq!(status["config"]["repositories"], 1);

    let entries = Call::get("/v1/work-catalog/entries")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(entries.status, 200, "{}", entries.body);
    let entries = entries.json();
    assert_eq!(entries["source"], "database");
    assert_eq!(entries["generation"], status["generation"]);
    assert_eq!(entries["entries"].as_array().map(Vec::len), Some(1));
    let entry = &entries["entries"][0];
    assert_eq!(entry["kind"], "plan");
    assert_eq!(entry["occurrences"][0]["content_kind"], "tracked");
    assert_eq!(entry["occurrences"][0]["relative_path"], "plans/api.md");
    assert_eq!(
        entry["occurrences"][0]["content"]["epic"], "ASMA-9990",
        "the graph fence and front matter agree on the declared epic"
    );

    let filtered = Call::get("/v1/work-catalog/entries?kind=execution_unit")
        .signed_as(&world, "operator")
        .send(&world)
        .await;
    assert_eq!(filtered.status, 200);
    assert_eq!(filtered.json()["entries"].as_array().map(Vec::len), Some(0));

    let invalid = Call::get("/v1/work-catalog/entries?kind=session")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(invalid.status, 400);
    assert_eq!(invalid.code(), "invalid_request");

    let model_catalog = Call::get("/v1/catalog")
        .signed_as(&world, "admin")
        .send(&world)
        .await;
    assert_eq!(
        model_catalog.status, 409,
        "the model catalog namespace answers its own placement refusal, not the work catalog"
    );
    assert_eq!(model_catalog.code(), "placement_blocked");
}

#[tokio::test]
async fn the_native_census_is_visible_through_the_authenticated_work_catalog() {
    let world = World::open_empty().await;
    let catalog = world
        .daemon
        .catalog()
        .expect("the daemon composes the catalog");
    let settings = kontor_catalog::CatalogSettings {
        schema_version: 1,
        ..kontor_catalog::CatalogSettings::default()
    };
    std::fs::write(
        Catalog::settings_path(world.directory.path()),
        serde_json::to_vec_pretty(&settings).expect("settings"),
    )
    .expect("the settings are installed");

    let source = NativeSource::new(vec![native_census()]);
    let sources: Vec<Arc<dyn NativeCensusSource>> = vec![source as Arc<dyn NativeCensusSource>];
    let report = catalog
        .collect_native_once(&sources)
        .await
        .expect("the native census completes");
    assert!(report.complete);
    assert_eq!(report.sessions, 2);
    assert_eq!(report.archived_sessions, 1);

    let anonymous = Call::get("/v1/work-catalog/native/status")
        .anonymous()
        .send(&world)
        .await;
    assert_eq!(anonymous.status, 401);
    assert_eq!(anonymous.code(), "unauthenticated");
    let anonymous_inventory = Call::get("/v1/work-catalog/native/inventory")
        .anonymous()
        .send(&world)
        .await;
    assert_eq!(anonymous_inventory.status, 401);

    let status = Call::get("/v1/work-catalog/native/status")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(status.status, 200, "{}", status.body);
    let status = status.json();
    assert_eq!(status["schema_version"], 1);
    assert_eq!(status["source"], "database");
    assert_eq!(status["generation"], report.generation);
    assert_eq!(status["collection"]["state"], "complete");
    assert_eq!(status["collection"]["include_archived"], true);
    assert_eq!(
        status["collection"]["coverage"][0]["client_id"],
        "kontor-mini-1-census"
    );
    assert_eq!(status["counts"]["sessions"], 2);
    assert_eq!(status["counts"]["archived_sessions"], 1);
    assert_eq!(status["snapshot"]["current"], true);

    let inventory = Call::get("/v1/work-catalog/native/inventory")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(inventory.status, 200, "{}", inventory.body);
    let inventory = inventory.json();
    assert_eq!(inventory["schema_version"], 1);
    assert_eq!(inventory["source"], "database");
    assert_eq!(inventory["archive_scope"], true);
    assert_eq!(inventory["sessions"].as_array().map(Vec::len), Some(2));
    let same_title: Vec<_> = inventory["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .filter(|session| session["title"] == "Implement")
        .collect();
    assert_eq!(same_title.len(), 2);
    assert_ne!(same_title[0]["session_id"], same_title[1]["session_id"]);
    assert!(
        inventory["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["archived_at"] == "2026-10-24T00:00:00Z")
    );

    let plan_status = Call::get("/v1/work-catalog/status")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(plan_status.status, 200);
    assert_eq!(
        plan_status.json()["attempted_generation"],
        0,
        "the native census must not advance plan discovery"
    );
}

#[tokio::test]
async fn entry_cursors_and_per_source_status_are_served_and_fenced() {
    let world = World::open_empty().await;
    let catalog = world
        .daemon
        .catalog()
        .expect("the daemon composes the catalog");
    let checkout = disposable_checkout();
    for (name, epic) in [("beta", "ASMA-9991"), ("gamma", "ASMA-9992")] {
        std::fs::write(
            checkout.path().join(format!("plans/{name}.md")),
            format!(
                "---\nepic: {epic}\nworkstream: kontor\n---\n# {name} plan\n\n```plan-graph\n{{\"schema_version\":1,\"epic\":{{\"jira\":\"{epic}\",\"kontor_id\":null}},\"tasks\":[],\"edges\":[],\"external_fences\":[]}}\n```\n"
            ),
        )
        .expect("the plan");
    }
    git(checkout.path(), &["add", "-A"]);
    git(checkout.path(), &["commit", "-q", "-m", "more plans"]);
    let settings = kontor_catalog::CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: checkout.path().to_path_buf(),
            include_github_plans: false,
        }],
        ..kontor_catalog::CatalogSettings::default()
    };
    std::fs::write(
        Catalog::settings_path(world.directory.path()),
        serde_json::to_vec_pretty(&settings).expect("settings"),
    )
    .expect("the settings are installed");
    let report = catalog.collect_once().await.expect("the sweep completes");
    assert!(report.finished);

    let zero = Call::get("/v1/work-catalog/entries?limit=0")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(zero.status, 400, "{}", zero.body);

    let first = Call::get("/v1/work-catalog/entries?limit=1")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let first = first.json();
    assert_eq!(first["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(first["truncated"], true);
    let cursor = first["next_cursor"]
        .as_str()
        .expect("a continuation")
        .to_owned();

    let second = Call::get(format!(
        "/v1/work-catalog/entries?limit=1&cursor={}",
        cursor.replace(':', "%3A")
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(second.status, 200, "{}", second.body);
    let second = second.json();
    assert_eq!(second["generation"], first["generation"]);
    assert_ne!(
        first["entries"][0]["logical_plan_id"],
        second["entries"][0]["logical_plan_id"]
    );

    let sources = Call::get("/v1/work-catalog/sources")
        .signed_as(&world, "observer")
        .send(&world)
        .await;
    assert_eq!(sources.status, 200, "{}", sources.body);
    let sources = sources.json();
    assert_eq!(sources["schema_version"], 1);
    assert_eq!(sources["sources"].as_array().map(Vec::len), Some(3));
    assert_eq!(sources["sources"][0]["source"], "plan");
    assert_eq!(sources["sources"][0]["state"], "complete");
    assert_eq!(sources["sources"][0]["as_of"], first["as_of"]);
    assert_eq!(sources["sources"][1]["source"], "jira");
    assert_eq!(sources["sources"][1]["state"], "not_configured");
    assert_eq!(sources["sources"][2]["source"], "native");

    let anonymous = Call::get("/v1/work-catalog/sources")
        .anonymous()
        .send(&world)
        .await;
    assert_eq!(anonymous.status, 401);

    let wrong = Call::get(format!(
        "/v1/work-catalog/entries?kind=execution_unit&cursor={}",
        cursor.replace(':', "%3A")
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(wrong.status, 400);
    assert_eq!(wrong.code(), "invalid_request");

    std::fs::write(
        checkout.path().join("plans/api.md"),
        "---\nepic: ASMA-9990\nworkstream: kontor\n---\n# API plan revised\n",
    )
    .expect("the plan changes");
    git(checkout.path(), &["add", "-A"]);
    git(checkout.path(), &["commit", "-q", "-m", "revise"]);
    let later = catalog.collect_once().await.expect("the sweep completes");
    assert_ne!(later.generation, report.generation);

    let stale = Call::get(format!(
        "/v1/work-catalog/entries?limit=1&cursor={}",
        cursor.replace(':', "%3A")
    ))
    .signed_as(&world, "observer")
    .send(&world)
    .await;
    assert_eq!(stale.status, 410);
    assert_eq!(stale.code(), "stale_cursor");
}
