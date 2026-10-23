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
    /// Optional consultation convention; a candidate until its bundle is activated.
    pub consultation_layout: Option<String>,
    /// Configured check-run names.
    pub check_runs: Vec<String>,
    /// Explicit project countries available to holiday imports; no built-in default.
    #[serde(skip)]
    pub holiday_countries: Vec<kontor_core::calendar::CountryCode>,
    /// Validated authoring projection; publication reads only its activated bundle.
    #[serde(skip)]
    pub publication_policy: Option<kontor_core::publication::PublicationPolicy>,
    /// Validated selected branch templates; publication uses the activated copy.
    #[serde(skip)]
    pub branch_rules: Option<kontor_core::branch::ProjectBranchRules>,
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
    /// Explicit integration bindings constrain publication; they never authorize it.
    pub(crate) publication_consumers: Vec<kontor_core::id::ProjectId>,
    /// Optional explicit Git sources, snapshotted from this folder's host.yml.
    pub checkouts: Option<kontor_runtime::checkout::CheckoutInputs>,
    /// Validated selected tracker authoring, cached without installing any revision.
    pub tracker_specs: kontor_jira::jira::SpecCatalog,
    /// Selected profile/domain/preset candidates; loading activates nothing.
    pub realm_pack: Option<crate::realm_packs::RealmPack>,
    /// Host route bootstrap snapshot; never re-read for an active launch.
    pub host_routes: Option<kontor_fleet::host_routes::HostRoutes>,
    /// Exact selected qualification candidate bytes; publication/activation is separate.
    pub qualification_source: Option<String>,
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
        let entry_path = resolve_reference(&root, ENTRY)?;
        let text = std::fs::read_to_string(&entry_path)
            .map_err(|_| refusal(&entry_path, "config file cannot be read"))?;
        let branch_rules = crate::project_branch::parse(&text)
            .map_err(|problem| refusal(&entry_path, problem))?
            .ok_or_else(|| refusal(&entry_path, "missing project.branch"))?;
        let raw: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)
            .map_err(|_| refusal(&entry_path, "invalid YAML configuration"))?;
        crate::project_paths::validate(&raw["project"])
            .map_err(|problem| refusal(&entry_path, problem))?;
        crate::project_paths::consultation_policy(&text)
            .map_err(|problem| refusal(&entry_path, problem))?;
        let mut entry: Entry = serde_yaml_ng::from_str(&text).map_err(|_| {
            refusal(
                &entry_path,
                "invalid YAML configuration or unsupported document shape",
            )
        })?;
        let countries = raw["project"]["holiday_countries"]
            .as_sequence()
            .filter(|countries| !countries.is_empty())
            .ok_or_else(|| refusal(&entry_path, "missing or empty project.holiday_countries"))?;
        entry.project.holiday_countries = countries
            .iter()
            .map(|country| {
                country
                    .as_str()
                    .and_then(|code| kontor_core::calendar::CountryCode::parse(code).ok())
                    .ok_or_else(|| refusal(&entry_path, "invalid project.holiday_countries"))
            })
            .collect::<Result<_, _>>()?;
        entry.project.branch_rules = Some(branch_rules);
        entry.project.publication_policy =
            crate::project_policy::parse(&text).map_err(|problem| refusal(&entry_path, problem))?;
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
            || project.check_runs.is_empty()
            || project.check_runs.iter().any(|v| v.trim().is_empty())
        {
            return Err(refusal(
                &entry_path,
                "invalid project identity in orchestration.yml",
            ));
        }
        if !kontor_core::branch::valid_base_branch(&project.forge.base_branch) {
            return Err(refusal(&entry_path, "invalid project.forge.base_branch"));
        }
        let folder = Self {
            root,
            project: entry.project,
            fleet: PathBuf::new(),
            publication_consumers: Vec::new(),
            checkouts: None,
            tracker_specs: kontor_jira::jira::SpecCatalog::empty(),
            realm_pack: None,
            host_routes: None,
            qualification_source: None,
        };
        let fleet = folder.reference(&entry.fleet)?;
        folder.reference(&entry.core_team)?;
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
        let mut folder = Self { fleet, ..folder };
        folder.publication_consumers = folder.publication_consumer_projects(&raw)?;
        folder.checkouts = crate::task_checkouts::read(&folder)?;
        folder.host_routes = folder
            .parse_optional("host.yml", |text| {
                let raw: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).map_err(|_| ())?;
                if raw.get("provider_accounts").is_none() && raw.get("routes").is_none() {
                    return Ok(None);
                }
                kontor_fleet::host_routes::HostRoutes::parse(text)
                    .map(Some)
                    .map_err(|_| ())
            })?
            .flatten();
        folder.qualification_source = folder.parse_optional(
            kontor_fleet_activation::routes::QUALIFICATION_SOURCE,
            |text| Ok::<_, ()>(text.to_owned()),
        )?;
        if !raw["project"]["tracker"]["spec"].is_null()
            && raw["project"]["tracker"]["spec"].as_str().is_none()
        {
            return Err(refusal(&entry_path, "invalid project.tracker.spec"));
        }
        folder.tracker_specs =
            crate::realm_specs::read(&folder, raw["project"]["tracker"]["spec"].as_str())?;
        let team_pack = &raw["project"]["team_pack"];
        if !team_pack.is_null() {
            let source = team_pack
                .as_str()
                .ok_or_else(|| refusal(&entry_path, "invalid project.team_pack"))?;
            folder.realm_pack = Some(crate::realm_packs::read(&folder, source)?);
        }
        Ok(folder)
    }

    fn publication_consumer_projects(
        &self,
        entry: &serde_yaml_ng::Value,
    ) -> Result<Vec<kontor_core::id::ProjectId>, ConfigError> {
        let mut projects = Vec::new();
        let sources = [
            entry["project"]["tracker"]["spec"].as_str(),
            Some("tracker/connectors.yml"),
            Some("forge/github-app.yml"),
        ];
        for source in sources.into_iter().flatten() {
            let Some(document) = self.optional::<serde_yaml_ng::Value>(source)? else {
                continue;
            };
            let ids = document.get("project_id").into_iter().chain(
                document
                    .get("projects")
                    .and_then(serde_yaml_ng::Value::as_sequence)
                    .into_iter()
                    .flatten()
                    .filter_map(|project| project.get("project_id")),
            );
            for id in ids {
                let id = id
                    .as_str()
                    .and_then(|text| kontor_core::id::ProjectId::parse(text).ok())
                    .ok_or_else(|| {
                        refusal(self.root.join(source), "invalid consumer project_id")
                    })?;
                if self
                    .project
                    .publication_policy
                    .as_ref()
                    .is_some_and(|policy| policy.project_id != id)
                {
                    return Err(refusal(
                        self.root.join(source),
                        "consumer project_id conflicts with project.project_id",
                    ));
                }
                if !projects.contains(&id) {
                    projects.push(id);
                }
            }
        }
        Ok(projects)
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
        let resolved = self.reference_path(value)?;
        let _: serde_yaml_ng::Value = read_yaml(&resolved)?;
        Ok(resolved)
    }

    fn reference_path(&self, value: &str) -> Result<PathBuf, ConfigError> {
        resolve_reference(&self.root, value)
    }

    /// Capture bounded, owner-checked pack bytes once for parsing and hashing.
    pub(crate) fn pack_source(&self, source: &str) -> Result<String, ConfigError> {
        let path = self.reference_path(source)?;
        let guard = kontor_fleet_activation::Guard {
            symlink: "pack source is not a regular file",
            writable: "pack source is group or world writable",
            owner: "pack source owner differs from selected folder",
            oversized: "pack source exceeds the size limit",
            swapped: "pack source changed during read",
            utf8: "pack source is not UTF-8",
            limit: kontor_fleet_activation::domain::MAX_DOMAIN_BYTES,
            forbidden_mode: 0o022,
        };
        kontor_fleet_activation::read_guarded(&self.root, &path, &guard).map_err(|error| {
            refusal(
                path,
                match error {
                    kontor_fleet::FleetError::Invalid { rule } => rule,
                    _ => "pack source cannot be read",
                },
            )
        })
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

fn resolve_reference(root: &Path, value: &str) -> Result<PathBuf, ConfigError> {
    let relative = Path::new(value);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || relative.as_os_str().is_empty()
    {
        return Err(refusal(
            root.join(value),
            "config reference must be folder-relative without traversal",
        ));
    }
    let path = root.join(relative);
    if !matches!(
        path.extension().and_then(|v| v.to_str()),
        Some("yml" | "yaml")
    ) {
        return Err(refusal(path, "config authoring source must be YAML"));
    }
    let resolved = path
        .canonicalize()
        .map_err(|_| refusal(&path, "referenced config file is missing or unreadable"))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(refusal(
            path,
            "config reference must name a file inside the selected folder",
        ));
    }
    Ok(resolved)
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
