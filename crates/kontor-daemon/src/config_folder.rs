//! Explicit, read-only selection of a project's YAML authoring folder.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// The entry point shared with the workflow pack's direct resolver.
pub const ENTRY: &str = "orchestration/orchestration.yml";

/// A named refusal, deliberately excluding parser messages and source values.
#[derive(Debug, thiserror::Error)]
#[error("{problem}: {path}")]
pub struct ConfigError {
    /// The setting or file that needs attention.
    pub path: PathBuf,
    /// A stable description without potentially secret document contents.
    pub problem: &'static str,
}

/// Bootstrap project identity. Activated workflow policy remains in its artifacts.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectSettings {
    /// Confirmed tracker project key.
    pub key: String,
    /// Human-readable project name.
    pub name: String,
    /// Repository names by logical resource root.
    pub resource_roots: BTreeMap<String, String>,
    /// Tracker identity.
    pub tracker: TrackerSettings,
    /// Forge identity.
    pub forge: ForgeSettings,
    /// Project worktree convention.
    pub worktree_layout: String,
    /// Configured check-run names.
    pub check_runs: Vec<String>,
}

/// Tracker values needed to prevent cross-project selection.
#[derive(Debug, Clone, Deserialize)]
pub struct TrackerSettings {
    /// The tracker project must agree with the public project key.
    pub project_key: String,
}

/// Project-owned forge identity.
#[derive(Debug, Clone, Deserialize)]
pub struct ForgeSettings {
    /// Governed owner/name repositories.
    pub repositories: Vec<String>,
    /// The only permitted publication base.
    pub base_branch: String,
}

#[derive(Deserialize)]
struct Entry {
    schema_version: u32,
    fleet: String,
    core_team: String,
    project: ProjectSettings,
}

/// One canonical selection, independent of subsequent changes to the consumer cwd.
#[derive(Debug, Clone)]
pub struct ConfigFolder {
    root: PathBuf,
    /// The project's bootstrap settings from the selected entry file.
    pub project: ProjectSettings,
    /// The selected fleet authoring path; activation state stays in the state root.
    pub fleet: PathBuf,
}

impl ConfigFolder {
    /// Select the flag before the environment, resolving relative paths once.
    ///
    /// # Errors
    /// Refuses an absent selection, missing folder, invalid YAML or bad reference.
    pub fn select(flag: Option<&Path>, environment: Option<&Path>) -> Result<Self, ConfigError> {
        let selected = flag
            .or(environment)
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| {
                refusal(
                    "--config-dir / KONTOR_CONFIG_DIR",
                    "no config folder selected",
                )
            })?;
        let root = selected
            .canonicalize()
            .map_err(|_| refusal(selected, "config folder does not exist or cannot be read"))?;
        if !root.is_dir() {
            return Err(refusal(root, "config folder is not a directory"));
        }
        let entry_path = root.join(ENTRY);
        let entry: Entry = read_yaml(&entry_path)?;
        if entry.schema_version != 1 {
            return Err(refusal(
                &entry_path,
                "unsupported orchestration schema_version",
            ));
        }
        let project = &entry.project;
        if !project
            .key
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_uppercase)
            || project.key != project.tracker.project_key
            || !project
                .key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            || project.name.trim().is_empty()
            || project.forge.base_branch.trim().is_empty()
            || project.forge.repositories.is_empty()
            || project.resource_roots.is_empty()
            || project.check_runs.is_empty()
            || project.check_runs.iter().any(|v| v.trim().is_empty())
            || project.worktree_layout.trim().is_empty()
            || project
                .forge
                .repositories
                .iter()
                .any(|v| v.split('/').count() != 2 || v.split('/').any(str::is_empty))
            || project
                .resource_roots
                .values()
                .any(|v| !project.forge.repositories.contains(v))
        {
            return Err(refusal(
                &entry_path,
                "invalid project identity in orchestration.yml",
            ));
        }
        let folder = Self {
            root,
            project: entry.project,
            fleet: PathBuf::new(),
        };
        let fleet = folder.reference(&entry.fleet)?;
        folder.reference(&entry.core_team)?;
        let raw: serde_yaml_ng::Value = read_yaml(&entry_path)?;
        for key in ["team_pack", "fleet", "supervision"] {
            if let Some(value) = raw["project"][key].as_str() {
                folder.reference(value)?;
            }
        }
        for (section, key) in [("tracker", "spec"), ("pr_desk", "settings")] {
            if let Some(value) = raw["project"][section][key].as_str() {
                folder.reference(value)?;
            }
        }
        folder.validate_tree(&folder.root)?;
        Ok(Self { fleet, ..folder })
    }

    /// The resolved absolute folder path, for service launch and readback.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Runtime planes are local bootstrap settings in `host.runtime.planes`.
    ///
    /// # Errors
    /// Refuses unsupported host or plane schemas without echoing endpoint values.
    pub fn runtimes(&self) -> Result<crate::runtimes::RuntimeSettings, ConfigError> {
        #[derive(Deserialize)]
        struct Host {
            schema_version: u32,
            #[serde(default)]
            runtime: HostRuntime,
        }
        #[derive(Default, Deserialize)]
        struct HostRuntime {
            #[serde(default)]
            planes: Option<crate::runtimes::RuntimeSettings>,
        }
        let Some(host) = self.optional::<Host>("host.yml")? else {
            return Ok(crate::runtimes::RuntimeSettings::default());
        };
        if host.schema_version != 1 {
            return Err(refusal(
                self.root.join("host.yml"),
                "unsupported host schema_version",
            ));
        }
        let planes = host.runtime.planes.unwrap_or_default();
        if ![4, 5].contains(&planes.schema_version) {
            return Err(refusal(
                self.root.join("host.yml"),
                "unsupported runtime planes schema_version",
            ));
        }
        Ok(planes)
    }

    /// Resolve an existing YAML reference inside this folder, including symlinks.
    ///
    /// # Errors
    /// Refuses absolute paths, traversal, escapes, JSON and missing files by name.
    pub fn reference(&self, value: &str) -> Result<PathBuf, ConfigError> {
        let relative = Path::new(value);
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
            || relative.as_os_str().is_empty()
        {
            return Err(refusal(
                self.root.join(value),
                "config reference must be folder-relative without traversal",
            ));
        }
        let path = self.root.join(relative);
        if !matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("yml" | "yaml")
        ) {
            return Err(refusal(path, "config authoring source must be YAML"));
        }
        let resolved = path
            .canonicalize()
            .map_err(|_| refusal(&path, "referenced config file is missing or unreadable"))?;
        if !resolved.starts_with(&self.root) || !resolved.is_file() {
            return Err(refusal(
                path,
                "config reference must name a file inside the selected folder",
            ));
        }
        let _: serde_yaml_ng::Value = read_yaml(&resolved)?;
        Ok(resolved)
    }

    /// Read an optional YAML bootstrap file, without consulting another folder.
    ///
    /// # Errors
    /// An existing invalid or unreadable file is a named refusal, never absence.
    pub fn optional<T: DeserializeOwned>(&self, relative: &str) -> Result<Option<T>, ConfigError> {
        let path = self.root.join(relative);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(refusal(path, "config file cannot be read")),
            Ok(_) => read_yaml(&self.reference(relative)?).map(Some),
        }
    }

    /// Read an optional YAML file through its owning contract's validator.
    ///
    /// # Errors
    /// Refuses a present invalid source by filename, without quoting its contents.
    pub fn parse_optional<T, E>(
        &self,
        relative: &str,
        parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<Option<T>, ConfigError> {
        if self.optional::<serde_yaml_ng::Value>(relative)?.is_none() {
            return Ok(None);
        }
        let path = self.reference(relative)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|_| refusal(&path, "config file cannot be read"))?;
        parse(&text).map(Some).map_err(|_| {
            refusal(
                path,
                "invalid YAML configuration or unsupported document shape",
            )
        })
    }

    /// Refuse old editable sources before claiming or migrating a realm.
    ///
    /// # Errors
    /// Even equivalent copies require an explicit migration; two sources are unsafe.
    pub fn refuse_legacy(&self, state_root: &Path) -> Result<(), ConfigError> {
        for name in [
            "jira.json",
            "config/github-app.json",
            "runtimes.json",
            "memory-cognee.json",
            "succession-summarizer.json",
            "fleet.yml",
            "supervision.yml",
            "attention.yml",
            "quota-signals.yml",
        ] {
            let path = state_root.join(name);
            match std::fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(refusal(path, "legacy config source cannot be checked")),
                Ok(_) => {
                    return Err(refusal(
                        path,
                        "legacy config source conflicts with selected folder; migrate authoring inputs explicitly or use an empty state root",
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_tree(&self, directory: &Path) -> Result<(), ConfigError> {
        for entry in std::fs::read_dir(directory)
            .map_err(|_| refusal(directory, "config folder cannot be read"))?
        {
            let entry = entry.map_err(|_| refusal(directory, "config folder cannot be read"))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|_| refusal(&path, "config file cannot be read"))?;
            if kind.is_dir() {
                self.validate_tree(&path)?;
            } else if matches!(
                path.extension().and_then(|v| v.to_str()),
                Some("yml" | "yaml")
            ) {
                let relative = path
                    .strip_prefix(&self.root)
                    .expect("walk stays under selected root");
                self.reference(
                    relative
                        .to_str()
                        .ok_or_else(|| refusal(&path, "config filename must be UTF-8"))?,
                )?;
            } else if path.extension().is_some_and(|v| v == "json") {
                return Err(refusal(
                    path,
                    "legacy JSON authoring source; migrate to YAML",
                ));
            }
        }
        Ok(())
    }
}

fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path)
        .map_err(|_| refusal(path, "config file is missing or unreadable"))?;
    serde_yaml_ng::from_str(&text).map_err(|_| {
        refusal(
            path,
            "invalid YAML configuration or unsupported document shape",
        )
    })
}

fn refusal(path: impl Into<PathBuf>, problem: &'static str) -> ConfigError {
    ConfigError {
        path: path.into(),
        problem,
    }
}
