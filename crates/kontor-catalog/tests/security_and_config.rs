//! TEST-006: configured-root confinement, bounded reads, credential redaction
//! and zero upstream effects.
//!
//! "Zero upstream effects" is not a network mock here: the collector has no
//! network client at all. The test still proves the local half — the fixture
//! repository's index, HEAD and worktree are byte-identical after a sweep.

mod support;

use std::path::PathBuf;

use kontor_catalog::config::{Environment, resolve};
use kontor_catalog::{Catalog, CatalogSettings, EntryFilter};
use support::{Fixture, Repo, plan, sweep};

#[tokio::test]
async fn a_symlink_out_of_the_root_is_refused_and_its_target_never_appears() {
    let repo = Repo::new();
    let outside = tempfile::TempDir::new().expect("an outside directory");
    let target = outside.path().join("secret.md");
    let secret = plan(
        "SECRET_TARGET_TITLE",
        Some("ASMA-9960"),
        Some("kontor"),
        "token: SECRET_TARGET_CONTENT\n",
    );
    std::fs::write(&target, &secret).expect("the outside file");
    let link = repo.root().join("_docs/plans/link.md");
    std::fs::create_dir_all(link.parent().expect("parent")).expect("parent directory");
    std::os::unix::fs::symlink(&target, &link).expect("the symlink");
    repo.commit("a symlink is tracked");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(
        entries.entries.is_empty(),
        "no entry may be created from a symlink"
    );
    let rendered = serde_json::to_string(&entries).unwrap();
    assert!(!rendered.contains("SECRET_TARGET_TITLE"));
    assert!(!rendered.contains("SECRET_TARGET_CONTENT"));
    let status = fixture.catalog.status().unwrap();
    assert!(
        status
            .collection
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("symlink"))
    );
}

#[tokio::test]
async fn an_oversized_plan_is_bounded_and_visible_as_a_diagnostic() {
    let repo = Repo::new();
    let mut content = plan("Huge plan", Some("ASMA-9961"), Some("kontor"), "");
    content.push_str(&"x".repeat(3 * 1024 * 1024));
    repo.write("plans/huge.md", &content);
    repo.commit("huge");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(entries.entries.is_empty());
    let status = fixture.catalog.status().unwrap();
    assert!(
        status
            .collection
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("2 MiB"))
    );
}

#[tokio::test]
async fn source_bytes_and_secret_shaped_values_never_reach_a_diagnostic() {
    let repo = Repo::new();
    repo.write(
        "plans/malformed.md",
        "---\nepic: [unterminated\npassword: SECRET_SENTINEL_VALUE\n---\n# Visible\n",
    );
    repo.commit("malformed");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let status = fixture.catalog.status().unwrap();
    let rendered = format!(
        "{}{}",
        serde_json::to_string(&entries).unwrap(),
        serde_json::to_string(&status).unwrap()
    );
    assert!(
        !rendered.contains("SECRET_SENTINEL_VALUE"),
        "diagnostics are structural; source bytes never enter them"
    );
    assert!(
        entries.entries[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "frontmatter_invalid")
    );
}

#[tokio::test]
async fn a_sweep_makes_no_change_to_the_discovered_repository() {
    let repo = Repo::new();
    repo.write(
        "plans/observable.md",
        &plan("Observable", Some("ASMA-9962"), Some("kontor"), ""),
    );
    repo.commit("observable");
    let head = repo.head();
    let status_before = repo.status();
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    assert_eq!(repo.head(), head, "the collector never moves HEAD");
    assert_eq!(
        repo.status(),
        status_before,
        "the collector writes no worktree or index state"
    );
    assert!(!repo.root().join("catalog.db").exists());
}

struct FakeEnvironment {
    values: Vec<(String, String)>,
    home: Option<PathBuf>,
}

impl Environment for FakeEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    fn home(&self) -> Option<String> {
        self.home.as_ref().map(|home| home.display().to_string())
    }
}

#[test]
fn effective_configuration_prefers_the_plugin_environment_then_the_plugin_file() {
    let paseo = tempfile::TempDir::new().expect("paseo home");
    let plugin = paseo.path().join("plugin-config/kontor");
    std::fs::create_dir_all(&plugin).expect("plugin config directory");
    std::fs::write(
        plugin.join("read-sources.json"),
        serde_json::json!([{
            "id": "file-source",
            "kind": "plan",
            "path": paseo.path().join("plan.md"),
            "workstream": null,
            "epic": null,
            "projectId": null,
            "revisionKind": "working copy"
        }])
        .to_string(),
    )
    .expect("the plugin source catalog");
    std::fs::write(
        plugin.join("registry.json"),
        serde_json::json!({"workspaceRoot": paseo.path()}).to_string(),
    )
    .expect("the plugin registry config");
    let env_source = serde_json::json!([{
        "id": "env-source",
        "kind": "plan",
        "path": paseo.path().join("env-plan.md"),
        "workstream": null,
        "epic": null,
        "projectId": null,
        "revisionKind": "working copy"
    }])
    .to_string();
    let environment = FakeEnvironment {
        values: vec![
            ("PASEO_HOME".to_owned(), paseo.path().display().to_string()),
            ("KONTOR_PLUGIN_READ_SOURCES".to_owned(), env_source),
        ],
        home: None,
    };
    let state = tempfile::TempDir::new().expect("state root");
    let resolved = resolve(state.path(), &environment, "2026-10-25T00:00:00Z");
    assert_eq!(
        resolved.origin,
        kontor_catalog::ConfigOrigin::MigratedPluginEnv
    );
    assert_eq!(resolved.settings.sources.len(), 1);
    assert_eq!(resolved.settings.sources[0].id, "env-source");
    assert_eq!(
        resolved.settings.registry_path,
        Some(paseo.path().join("config/orchestration/workstreams.json"))
    );
    assert!(resolved.materialize);

    // Without the environment variable, the plugin file is the effective source.
    let environment = FakeEnvironment {
        values: vec![("PASEO_HOME".to_owned(), paseo.path().display().to_string())],
        home: None,
    };
    let resolved = resolve(state.path(), &environment, "2026-10-25T00:00:00Z");
    assert_eq!(
        resolved.origin,
        kontor_catalog::ConfigOrigin::MigratedPluginFile
    );
    assert_eq!(resolved.settings.sources[0].id, "file-source");

    // Once the catalog-owned document exists, it wins over both.
    std::fs::write(
        Catalog::settings_path(state.path()),
        serde_json::to_vec_pretty(&CatalogSettings {
            schema_version: 1,
            sources: Vec::new(),
            ..CatalogSettings::default()
        })
        .expect("settings"),
    )
    .expect("catalog settings");
    let environment = FakeEnvironment {
        values: vec![
            ("PASEO_HOME".to_owned(), paseo.path().display().to_string()),
            (
                "KONTOR_PLUGIN_READ_SOURCES".to_owned(),
                "not json".to_owned(),
            ),
        ],
        home: None,
    };
    let resolved = resolve(state.path(), &environment, "2026-10-25T00:00:00Z");
    assert_eq!(
        resolved.origin,
        kontor_catalog::ConfigOrigin::CatalogFile,
        "the durable migrated document is the one operator answer"
    );
    assert!(!resolved.materialize);
}

#[test]
fn an_absent_plugin_configuration_is_a_valid_empty_catalog() {
    let home = tempfile::TempDir::new().expect("home");
    let state = tempfile::TempDir::new().expect("state root");
    let environment = FakeEnvironment {
        values: Vec::new(),
        home: Some(home.path().to_path_buf()),
    };
    let resolved = resolve(state.path(), &environment, "2026-10-25T00:00:00Z");
    assert_eq!(resolved.origin, kontor_catalog::ConfigOrigin::None);
    assert!(resolved.settings.repositories.is_empty());
    assert!(resolved.settings.sources.is_empty());
    assert!(!resolved.materialize);
}
