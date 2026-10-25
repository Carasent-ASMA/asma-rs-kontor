//! Effective operator configuration, resolved once and then owned by the
//! catalog.
//!
//! # Why this module exists
//!
//! The plugin reads its source catalog from an environment variable and then a
//! host file, and reads the workstream registry from a second environment
//! variable and a second host file. `launchd` does not inherit a terminal's
//! environment, so a daemon that re-derived configuration from today's
//! environment would silently collect nothing on the machine the operator
//! actually runs.
//!
//! The catalog therefore resolves the **effective** plugin configuration once,
//! exactly in the plugin's precedence order, and materializes the result into
//! its own `<state_root>/catalog.json`. From then on that file is the one
//! operator-owned answer. Editing it is how a host changes its catalog scope;
//! changing the plugin's environment no longer moves a running daemon's scope
//! behind its back.
//!
//! Zero configured repositories is a valid catalog. It is not an error and not
//! a reason to refuse startup.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::contract::ConfigOrigin;

/// The settings file the catalog owns inside its state root.
pub const CATALOG_CONFIG_FILE: &str = "catalog.json";
/// The database file the catalog owns inside its state root.
pub const CATALOG_DATABASE_FILE: &str = "catalog.db";
/// The atomic fallback snapshot the catalog owns inside its state root.
pub const CATALOG_SNAPSHOT_FILE: &str = "catalog-snapshot.json";

/// The default collection cadence. The acceptance target is discovery within
/// 90 seconds at a 60-second cadence, so the cadence is the configured floor.
pub const DEFAULT_CADENCE_SECONDS: u64 = 60;
/// The default per-sweep budget. A healthy local sweep is far below this; the
/// budget is what makes an unreadable checkout a bounded failure rather than a
/// stuck collector.
pub const DEFAULT_SCAN_BUDGET_SECONDS: u64 = 30;

/// A source catalog entry in the plugin's existing shape.
///
/// It is preserved rather than re-modelled: an operator's IDs, pins and
/// associations survive the migration byte for byte in meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperatorSource {
    /// The operator-owned id. Stable across catalog upgrades.
    pub id: String,
    /// A display title.
    #[serde(default)]
    pub title: Option<String>,
    /// What the source is.
    pub kind: SourceKind,
    /// The absolute file path.
    pub path: PathBuf,
    /// An explicit workstream key.
    #[serde(default)]
    pub workstream: Option<String>,
    /// An explicit epic key.
    #[serde(default)]
    pub epic: Option<String>,
    /// An explicit project id.
    #[serde(default)]
    pub project_id: Option<String>,
    /// The operator's declared revision intent.
    #[serde(default)]
    pub revision_kind: SourceRevisionKind,
    /// An optional expected SHA-256 pin.
    #[serde(default)]
    pub expected_sha256: Option<String>,
    /// A preserved logical plan id from the existing source catalog.
    #[serde(default)]
    pub logical_plan_id: Option<String>,
}

/// The source kinds the existing catalog can declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A plan Markdown document.
    Plan,
    /// A teams records document.
    Teams,
    /// A delivery records document.
    Delivery,
    /// A decisions records document.
    Decisions,
    /// A historical Jira projection.
    Jira,
}

/// The operator's declared revision intent, with the plugin's spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceRevisionKind {
    /// A saved draft.
    #[serde(rename = "saved draft")]
    SavedDraft,
    /// A working copy.
    #[serde(rename = "working copy")]
    #[default]
    WorkingCopy,
    /// An approved revision.
    #[serde(rename = "approved revision")]
    ApprovedRevision,
}

/// One configured repository or worktree root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryConfig {
    /// An absolute host path.
    pub root: PathBuf,
    /// Whether `.github/plans` is explicitly permitted for this root.
    ///
    /// The existing plugin excludes the directory entirely. The catalog only
    /// reads it where an operator has said so, and the setting travels with the
    /// root rather than with the process.
    #[serde(default)]
    pub include_github_plans: bool,
}

/// The durable catalog settings document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogSettings {
    /// Must be `1`.
    pub schema_version: u32,
    /// Explicit repositories and worktrees to discover.
    #[serde(default)]
    pub repositories: Vec<RepositoryConfig>,
    /// Explicit operator sources imported from the existing catalog.
    #[serde(default)]
    pub sources: Vec<OperatorSource>,
    /// The selected workstream registry path.
    #[serde(default)]
    pub registry_path: Option<PathBuf>,
    /// The collection cadence.
    #[serde(default = "default_cadence")]
    pub cadence_seconds: u64,
    /// The per-sweep scan budget.
    #[serde(default = "default_budget")]
    pub scan_budget_seconds: u64,
    /// Provenance: how this document was first produced.
    #[serde(default)]
    pub migrated_from: Option<ConfigOrigin>,
    /// Provenance: when it was materialized.
    #[serde(default)]
    pub imported_at: Option<String>,
    /// Explicit read-only Jira observation. Absent means no Jira collection.
    ///
    /// This section is additive to catalog schema 1. It names the configured
    /// scope; it never carries a credential value.
    #[serde(default)]
    pub jira: Option<JiraSettings>,
}

/// Explicit read-only Jira observation settings inside `catalog.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraSettings {
    /// Whether this section is active. A present section defaults to active.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The Jira project key whose observations are collected.
    pub project: String,
    /// The configured scope JQL. It is the only scope authority.
    pub scope_jql: String,
    /// Operator-declared issue keys, directly verified every sweep.
    #[serde(default)]
    pub declared_keys: Vec<String>,
    /// The base URL, when the catalog document owns it. Otherwise the
    /// matching `jira.json` project supplies it.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// The keychain alias, when the catalog document owns it. Otherwise the
    /// matching `jira.json` project supplies it.
    #[serde(default)]
    pub credential_alias: Option<String>,
    /// The Jira observation cadence.
    #[serde(default = "default_jira_cadence")]
    pub cadence_seconds: u64,
    /// How often a complete full census replaces incremental windows.
    #[serde(default = "default_jira_full_sweep")]
    pub full_sweep_seconds: u64,
    /// The overlap subtracted from the last complete instant for an
    /// incremental `updated` window.
    #[serde(default = "default_jira_overlap")]
    pub overlap_seconds: u64,
    /// The enhanced-search page bound.
    #[serde(default = "default_jira_max_pages")]
    pub max_pages: usize,
    /// The issue bound for one logical search.
    #[serde(default = "default_jira_max_issues")]
    pub max_issues: usize,
    /// The maximum previously observed issues rechecked in one sweep.
    #[serde(default = "default_jira_max_recheck")]
    pub max_recheck: usize,
    /// The enhanced-search page size, clamped to `1..=100`.
    #[serde(default = "default_jira_page_size")]
    pub page_size: u32,
    /// The bounded retry attempt count.
    #[serde(default = "default_jira_max_attempts")]
    pub max_attempts: u32,
    /// Whether the configured scope is an Epic hierarchy scope. When true a
    /// directly re-read issue at hierarchy level 1 is still in scope even when
    /// a search page omitted it, because search consistency lag is not a
    /// deletion.
    #[serde(default = "default_true")]
    pub scope_is_epics: bool,
    /// The maximum parent-resolution rounds.
    #[serde(default = "default_jira_parent_rounds")]
    pub parent_rounds: u32,
}

const fn default_true() -> bool {
    true
}
const fn default_jira_cadence() -> u64 {
    120
}
const fn default_jira_full_sweep() -> u64 {
    900
}
const fn default_jira_overlap() -> u64 {
    180
}
const fn default_jira_max_pages() -> usize {
    200
}
const fn default_jira_max_issues() -> usize {
    5000
}
const fn default_jira_max_recheck() -> usize {
    500
}
const fn default_jira_page_size() -> u32 {
    100
}
const fn default_jira_max_attempts() -> u32 {
    3
}
const fn default_jira_parent_rounds() -> u32 {
    5
}

const fn default_cadence() -> u64 {
    DEFAULT_CADENCE_SECONDS
}

const fn default_budget() -> u64 {
    DEFAULT_SCAN_BUDGET_SECONDS
}

impl Default for CatalogSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            repositories: Vec::new(),
            sources: Vec::new(),
            registry_path: None,
            cadence_seconds: DEFAULT_CADENCE_SECONDS,
            scan_budget_seconds: DEFAULT_SCAN_BUDGET_SECONDS,
            migrated_from: None,
            imported_at: None,
            jira: None,
        }
    }
}

/// The environment a resolution may read.
///
/// Tests inject this rather than mutating the process environment, which is
/// shared with every other test in the binary.
pub trait Environment {
    /// Read one variable.
    fn var(&self, key: &str) -> Option<String>;
    /// The operator's home directory.
    fn home(&self) -> Option<String>;
}

/// The real process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn home(&self) -> Option<String> {
        std::env::var("HOME").ok()
    }
}

/// A resolution result, valid or refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfiguration {
    /// The effective settings.
    pub settings: CatalogSettings,
    /// Where they came from.
    pub origin: ConfigOrigin,
    /// The SHA-256 of the exact bytes read, or of canonical JSON when built
    /// from the environment.
    pub revision: Option<String>,
    /// Structural diagnostics.
    pub diagnostics: Vec<String>,
    /// Whether the settings must be materialized to `catalog.json`.
    pub materialize: bool,
}

/// Resolve the effective configuration for one catalog root.
///
/// The order is the plugin's own:
///
/// 1. `KONTOR_PLUGIN_READ_SOURCES` when non-empty, otherwise
///    `<paseo>/plugin-config/kontor/read-sources.json`.
/// 2. `KONTOR_PLUGIN_ASMA_WORKSPACE`, otherwise
///    `<paseo>/plugin-config/kontor/registry.json`'s `workspaceRoot`.
/// 3. The registry is `<workspace>/config/orchestration/workstreams.json`.
///
/// A catalog-owned `catalog.json` wins over all of it, because it is the
/// migrated, launchd-safe answer.
#[must_use]
pub fn resolve(
    state_root: &Path,
    environment: &impl Environment,
    now: &str,
) -> ResolvedConfiguration {
    let file = state_root.join(CATALOG_CONFIG_FILE);
    if let Some(resolved) = read_settings_file(&file) {
        return resolved;
    }
    resolve_plugin_configuration(environment, now)
}

fn read_settings_file(file: &Path) -> Option<ResolvedConfiguration> {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(ResolvedConfiguration {
                settings: CatalogSettings::default(),
                origin: ConfigOrigin::CatalogFile,
                revision: None,
                diagnostics: vec![format!(
                    "The catalog settings file could not be read ({})",
                    error.kind()
                )],
                materialize: false,
            });
        }
    };
    let revision = sha256(&bytes);
    let settings: CatalogSettings = match serde_json::from_slice(&bytes) {
        Ok(settings) => settings,
        Err(error) => {
            return Some(ResolvedConfiguration {
                settings: CatalogSettings::default(),
                origin: ConfigOrigin::CatalogFile,
                revision: Some(revision),
                diagnostics: vec![format!(
                    "The catalog settings document is invalid at line {} column {}",
                    error.line(),
                    error.column()
                )],
                materialize: false,
            });
        }
    };
    let mut diagnostics = Vec::new();
    let settings = sanitize(settings, &mut diagnostics);
    Some(ResolvedConfiguration {
        settings,
        origin: ConfigOrigin::CatalogFile,
        revision: Some(revision),
        diagnostics,
        materialize: false,
    })
}

fn resolve_plugin_configuration(
    environment: &impl Environment,
    now: &str,
) -> ResolvedConfiguration {
    let mut diagnostics = Vec::new();
    let env_sources = environment
        .var("KONTOR_PLUGIN_READ_SOURCES")
        .filter(|value| !value.trim().is_empty());
    let (sources, source_origin) = match env_sources {
        Some(raw) => (
            parse_sources(&raw, "KONTOR_PLUGIN_READ_SOURCES", &mut diagnostics),
            ConfigOrigin::MigratedPluginEnv,
        ),
        None => match plugin_directory(environment) {
            Some(directory) => {
                let file = directory.join("read-sources.json");
                match std::fs::read(&file) {
                    Ok(bytes) => (
                        parse_sources(
                            &String::from_utf8_lossy(&bytes),
                            "the plugin source catalog",
                            &mut diagnostics,
                        ),
                        ConfigOrigin::MigratedPluginFile,
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        (Vec::new(), ConfigOrigin::None)
                    }
                    Err(error) => {
                        diagnostics.push(format!(
                            "The plugin source catalog could not be read ({})",
                            error.kind()
                        ));
                        (Vec::new(), ConfigOrigin::MigratedPluginFile)
                    }
                }
            }
            None => (Vec::new(), ConfigOrigin::None),
        },
    };
    let (registry_path, registry_origin, registry_diagnostics) = resolve_registry(environment);
    diagnostics.extend(registry_diagnostics);
    let origin = match (source_origin, registry_origin) {
        (ConfigOrigin::MigratedPluginEnv, _) | (_, ConfigOrigin::MigratedPluginEnv) => {
            ConfigOrigin::MigratedPluginEnv
        }
        (ConfigOrigin::MigratedPluginFile, _) | (_, ConfigOrigin::MigratedPluginFile) => {
            ConfigOrigin::MigratedPluginFile
        }
        _ => ConfigOrigin::None,
    };
    let repositories = derive_repositories(&sources, &mut diagnostics);
    let mut settings = CatalogSettings {
        schema_version: 1,
        repositories,
        sources,
        registry_path,
        cadence_seconds: DEFAULT_CADENCE_SECONDS,
        scan_budget_seconds: DEFAULT_SCAN_BUDGET_SECONDS,
        migrated_from: (origin != ConfigOrigin::None).then_some(origin),
        imported_at: (origin != ConfigOrigin::None).then(|| now.to_owned()),
        jira: None,
    };
    let revision = (origin != ConfigOrigin::None).then(|| {
        sha256(
            serde_json::to_string(&settings)
                .unwrap_or_default()
                .as_bytes(),
        )
    });
    let mut sanitized_diagnostics = Vec::new();
    settings = sanitize(settings, &mut sanitized_diagnostics);
    diagnostics.append(&mut sanitized_diagnostics);
    if origin != ConfigOrigin::None {
        diagnostics.push(format!(
            "The effective plugin configuration was migrated to {}; later changes require editing that file.",
            CATALOG_CONFIG_FILE
        ));
    }
    ResolvedConfiguration {
        settings,
        origin,
        revision,
        diagnostics,
        materialize: origin != ConfigOrigin::None,
    }
}

fn plugin_directory(environment: &impl Environment) -> Option<PathBuf> {
    let paseo_home = environment
        .var("PASEO_HOME")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let home = environment.home().filter(|value| !value.is_empty());
    let paseo = paseo_home
        .map(PathBuf::from)
        .or_else(|| home.map(|home| Path::new(&home).join(".paseo")))?;
    Some(paseo.join("plugin-config").join("kontor"))
}

fn resolve_registry(
    environment: &impl Environment,
) -> (Option<PathBuf>, ConfigOrigin, Vec<String>) {
    let mut diagnostics = Vec::new();
    let workspace = environment
        .var("KONTOR_PLUGIN_ASMA_WORKSPACE")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if let Some(workspace) = workspace {
        let root = PathBuf::from(workspace);
        if !root.is_absolute() {
            diagnostics.push(
                "KONTOR_PLUGIN_ASMA_WORKSPACE is not an absolute path; the registry is absent."
                    .to_owned(),
            );
            return (None, ConfigOrigin::None, diagnostics);
        }
        return (
            Some(root.join("config/orchestration/workstreams.json")),
            ConfigOrigin::MigratedPluginEnv,
            diagnostics,
        );
    }
    let Some(directory) = plugin_directory(environment) else {
        return (None, ConfigOrigin::None, diagnostics);
    };
    let file = directory.join("registry.json");
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (None, ConfigOrigin::None, diagnostics);
        }
        Err(error) => {
            diagnostics.push(format!(
                "The plugin registry configuration could not be read ({})",
                error.kind()
            ));
            return (None, ConfigOrigin::None, diagnostics);
        }
    };
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct RegistryConfig {
        workspace_root: PathBuf,
    }
    match serde_json::from_slice::<RegistryConfig>(&bytes) {
        Ok(config) if config.workspace_root.is_absolute() => (
            Some(
                config
                    .workspace_root
                    .join("config/orchestration/workstreams.json"),
            ),
            ConfigOrigin::MigratedPluginFile,
            diagnostics,
        ),
        Ok(_) => {
            diagnostics.push(
                "The plugin registry workspace root is not absolute; the registry is absent."
                    .to_owned(),
            );
            (None, ConfigOrigin::None, diagnostics)
        }
        Err(_) => {
            diagnostics.push(
                "The plugin registry configuration is invalid; the registry is absent.".to_owned(),
            );
            (None, ConfigOrigin::None, diagnostics)
        }
    }
}

fn parse_sources(raw: &str, origin: &str, diagnostics: &mut Vec<String>) -> Vec<OperatorSource> {
    match serde_json::from_str::<Vec<OperatorSource>>(raw) {
        Ok(sources) => sources,
        Err(error) => {
            diagnostics.push(format!(
                "The {origin} is invalid at line {} column {}; no operator sources were imported.",
                error.line(),
                error.column()
            ));
            Vec::new()
        }
    }
}

/// Infer a repository root for every explicit plan source that sits inside a
/// git working tree.
///
/// This is a migration convenience, not a heuristic identity: the inferred root
/// is a directory the operator already pointed an exact source at. It never
/// widens the scan to a parent of the source's checkout.
fn derive_repositories(
    sources: &[OperatorSource],
    diagnostics: &mut Vec<String>,
) -> Vec<RepositoryConfig> {
    let mut seen = BTreeSet::new();
    let mut repositories = Vec::new();
    for source in sources {
        if source.kind != SourceKind::Plan || !source.path.is_absolute() {
            continue;
        }
        let Some(root) = git_worktree_root(&source.path) else {
            continue;
        };
        let key = root.to_string_lossy().into_owned();
        if seen.insert(key) {
            repositories.push(RepositoryConfig {
                root,
                include_github_plans: false,
            });
        }
    }
    if repositories.len() > 16 {
        diagnostics.push(format!(
            "{} inferred repositories exceed the 16-root bound; the extra roots were not migrated.",
            repositories.len()
        ));
        repositories.truncate(16);
    }
    repositories
}

fn git_worktree_root(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors().skip(1).take(64) {
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Drop or correct values the contract refuses, with a diagnostic for each.
fn sanitize(mut settings: CatalogSettings, diagnostics: &mut Vec<String>) -> CatalogSettings {
    if settings.schema_version != 1 {
        diagnostics.push(format!(
            "Catalog settings schema_version {} is unsupported; the effective settings are empty.",
            settings.schema_version
        ));
        return CatalogSettings {
            schema_version: 1,
            migrated_from: settings.migrated_from,
            imported_at: settings.imported_at,
            ..CatalogSettings::default()
        };
    }
    settings.cadence_seconds = settings.cadence_seconds.clamp(1, 3600);
    settings.scan_budget_seconds = settings.scan_budget_seconds.clamp(1, 3600);

    let mut roots = BTreeSet::new();
    settings.repositories.retain(|repository| {
        if !repository.root.is_absolute() {
            diagnostics
                .push("A configured repository root is not absolute and was ignored.".to_owned());
            return false;
        }
        let key = repository.root.to_string_lossy().into_owned();
        if !roots.insert(key) {
            diagnostics.push("A configured repository root repeats and was collapsed.".to_owned());
            return false;
        }
        true
    });
    if settings.repositories.len() > 16 {
        diagnostics.push(
            "More than 16 repositories are configured; the extra roots were ignored.".to_owned(),
        );
        settings.repositories.truncate(16);
    }

    if let Some(registry) = &settings.registry_path
        && !registry.is_absolute()
    {
        diagnostics.push(
            "The configured registry path is not absolute; the registry is absent.".to_owned(),
        );
        settings.registry_path = None;
    }

    let mut ids = BTreeSet::new();
    let mut retained = Vec::new();
    for mut source in std::mem::take(&mut settings.sources) {
        if !valid_source_id(&source.id) {
            diagnostics.push("An operator source id is invalid and was ignored.".to_owned());
            continue;
        }
        if !ids.insert(source.id.clone()) {
            diagnostics.push(format!(
                "Operator source id `{}` repeats and the duplicate was ignored.",
                source.id
            ));
            continue;
        }
        if !source.path.is_absolute() {
            diagnostics.push(format!(
                "Operator source `{}` does not have an absolute path and was ignored.",
                source.id
            ));
            continue;
        }
        if source.revision_kind == SourceRevisionKind::ApprovedRevision
            && source.expected_sha256.is_none()
        {
            diagnostics.push(format!(
                "Operator source `{}` claims an approved revision without a pin and was ignored.",
                source.id
            ));
            continue;
        }
        if let Some(pin) = &source.expected_sha256
            && !valid_hash(pin)
        {
            diagnostics.push(format!(
                "Operator source `{}` has an invalid pin and was ignored.",
                source.id
            ));
            continue;
        }
        if let Some(workstream) = &source.workstream
            && !valid_workstream(workstream)
        {
            diagnostics.push(format!(
                "Operator source `{}` has an invalid workstream declaration; the declaration was ignored.",
                source.id
            ));
            source.workstream = None;
        }
        if let Some(epic) = &source.epic
            && !valid_epic(epic)
        {
            diagnostics.push(format!(
                "Operator source `{}` has an invalid epic declaration; the declaration was ignored.",
                source.id
            ));
            source.epic = None;
        }
        if let Some(id) = &source.logical_plan_id
            && !valid_logical_plan_id(id)
        {
            diagnostics.push(format!(
                "Operator source `{}` has an invalid logical plan id; it was ignored.",
                source.id
            ));
            source.logical_plan_id = None;
        }
        retained.push(source);
    }
    if retained.len() > 512 {
        diagnostics.push(
            "More than 512 operator sources are configured; the extra sources were ignored."
                .to_owned(),
        );
        retained.truncate(512);
    }
    settings.sources = retained;
    settings
}

/// Whether a workstream key is the registry's lexical form.
#[must_use]
pub fn valid_workstream(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut previous_dash = false;
    for byte in bytes {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' => previous_dash = false,
            b'-' if !previous_dash => previous_dash = true,
            _ => return false,
        }
    }
    !previous_dash && value.len() <= 128
}

/// Whether an epic key is the catalog's exact ASMA form.
#[must_use]
pub fn valid_epic(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("ASMA-") else {
        return false;
    };
    if rest.is_empty() || rest.starts_with('0') || rest.len() > 12 {
        return false;
    }
    rest.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether an explicit logical plan id is safe to retain.
#[must_use]
pub fn valid_logical_plan_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    value.len() <= 64
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

/// Whether an operator source id matches the existing source-catalog contract.
#[must_use]
pub fn valid_source_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Materialize settings into the catalog-owned file, atomically.
///
/// The temporary file is created in the same directory and renamed over the
/// destination, so a crash leaves either the old complete document or the new
/// one — never a half-written configuration a restart would trust.
pub fn materialize(
    state_root: &Path,
    settings: &CatalogSettings,
) -> Result<PathBuf, std::io::Error> {
    std::fs::create_dir_all(state_root)?;
    let destination = state_root.join(CATALOG_CONFIG_FILE);
    let temporary = state_root.join(format!(
        "{CATALOG_CONFIG_FILE}.{}.tmp",
        uuid::Uuid::now_v7()
    ));
    let bytes = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    {
        use std::io::Write as _;
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, &destination)?;
    if let Ok(directory) = std::fs::File::open(state_root) {
        let _ = directory.sync_all();
    }
    Ok(destination)
}

/// SHA-256 of exact bytes, lowercase hex.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// A structural, content-free configuration refusal.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The settings could not be materialized.
    #[error("catalog settings could not be written: {source}")]
    Write {
        /// The underlying filesystem error.
        #[from]
        source: std::io::Error,
    },
}
