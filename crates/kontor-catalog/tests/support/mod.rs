#![allow(dead_code)]

//! Shared fixtures for the catalog suites.
//!
//! Every repository here is a real git repository. The scanner's only honest
//! input is `git ls-files`, so a fixture that skipped git would test a different
//! program.

use std::path::{Path, PathBuf};
use std::process::Command;

use kontor_catalog::{Catalog, CatalogSettings, RepositoryConfig};
use tempfile::TempDir;

/// A disposable git repository.
pub(crate) struct Repo {
    directory: TempDir,
}

impl Repo {
    /// Create an empty repository with an identity git accepts.
    pub(crate) fn new() -> Self {
        let directory = TempDir::new().expect("a temporary repository");
        let repo = Self { directory };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "catalog@example.invalid"]);
        repo.git(&["config", "user.name", "Catalog Fixture"]);
        repo
    }

    /// The repository root.
    pub(crate) fn root(&self) -> &Path {
        self.directory.path()
    }

    /// Write a file, creating its parents.
    pub(crate) fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.root().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the parent directory");
        }
        std::fs::write(&path, content).expect("the fixture file");
        path
    }

    /// Remove a file.
    pub(crate) fn remove(&self, relative: &str) {
        std::fs::remove_file(self.root().join(relative)).expect("the fixture file is removed");
    }

    /// Stage everything and commit.
    pub(crate) fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.head()
    }

    /// Stage everything without committing.
    pub(crate) fn stage_all(&self) -> String {
        self.git(&["add", "-A"]);
        self.git(&["write-tree"])
    }

    /// The current commit.
    pub(crate) fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// Porcelain status, for proving a scan wrote nothing.
    pub(crate) fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }

    /// Add a linked worktree on a new branch.
    pub(crate) fn worktree(&self, into: &Path, branch: &str) -> PathBuf {
        self.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            &into.display().to_string(),
        ]);
        into.to_path_buf()
    }

    fn git(&self, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.root())
            .args(arguments)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

/// A plan document with exact front matter, a title and an optional graph fence.
pub(crate) fn plan(
    title: &str,
    epic: Option<&str>,
    workstream: Option<&str>,
    extra: &str,
) -> String {
    let mut front = String::from("---\n");
    if let Some(epic) = epic {
        front.push_str(&format!("epic: {epic}\n"));
    }
    if let Some(workstream) = workstream {
        front.push_str(&format!("workstream: {workstream}\n"));
    }
    front.push_str(extra);
    front.push_str("---\n\n");
    front.push_str(&format!("# {title}\n\nBody for {title}.\n"));
    if let Some(epic) = epic {
        front.push_str(&format!(
            "\n```plan-graph\n{{\"schema_version\":1,\"epic\":{{\"jira\":\"{epic}\",\"kontor_id\":null}},\"tasks\":[],\"edges\":[],\"external_fences\":[]}}\n```\n"
        ));
    }
    front
}

/// A registry document with exactly the named workstreams.
pub(crate) fn registry(keys: &[&str]) -> String {
    let workstreams = keys
        .iter()
        .map(|key| format!("\"{key}\":{{\"name\":\"{key}\"}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"schema_version\":1,\"workstreams\":{{{workstreams}}},\"desks\":{{}}}}\n")
}

/// A catalog with an explicit settings document and its own state root.
pub(crate) struct Fixture {
    /// The state root, so its contents can be asserted.
    pub(crate) state: TempDir,
    /// The open catalog.
    pub(crate) catalog: Catalog,
}

impl Fixture {
    /// Open a catalog configured with `repositories` and an optional registry.
    pub(crate) fn open(
        repositories: &[(PathBuf, bool)],
        registry_path: Option<PathBuf>,
        budget_seconds: u64,
    ) -> Self {
        let settings = CatalogSettings {
            schema_version: 1,
            repositories: repositories
                .iter()
                .map(|(root, include_github_plans)| RepositoryConfig {
                    root: root.clone(),
                    include_github_plans: *include_github_plans,
                })
                .collect(),
            registry_path,
            scan_budget_seconds: budget_seconds,
            cadence_seconds: 60,
            ..CatalogSettings::default()
        };
        Self::open_settings(settings)
    }

    /// Open a catalog with a complete, caller-owned settings document.
    pub(crate) fn open_settings(settings: CatalogSettings) -> Self {
        let state = TempDir::new().expect("a catalog state root");
        std::fs::write(
            Catalog::settings_path(state.path()),
            serde_json::to_vec_pretty(&settings).expect("settings JSON"),
        )
        .expect("the fixture settings");
        let catalog = Catalog::open_with_owner(state.path(), "fixture-owner").expect("a catalog");
        Self { state, catalog }
    }

    /// Replace the settings document while the catalog is open.
    pub(crate) fn rewrite_settings(&self, settings: &CatalogSettings) {
        std::fs::write(
            Catalog::settings_path(self.state.path()),
            serde_json::to_vec_pretty(settings).expect("settings JSON"),
        )
        .expect("the fixture settings");
    }
}

/// Run one sweep and return its report.
pub(crate) async fn sweep(catalog: &Catalog) -> kontor_catalog::SweepReport {
    catalog.collect_once().await.expect("the sweep completes")
}
