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

use harness::{Call, World};
use kontor_catalog::Catalog;
use tempfile::TempDir;

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
        model_catalog.status, 200,
        "the model catalog namespace is untouched by the work catalog"
    );
}
