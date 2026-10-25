//! `kontor-catalog` — the host-local observational work catalog.
//!
//! # One owner, one database, one answer
//!
//! This crate owns `<state_root>/catalog.db`, its migrations, the collector
//! lease, the page/promote protocol and the atomic fallback snapshot. It shares
//! no table with `kontor.db`: an orchestration store migration cannot rewrite an
//! observation, and a failed discovery cannot touch governed state.
//!
//! The public read contract is [`contract`]. Its types are frozen at contract
//! version [`contract::CATALOG_CONTRACT_VERSION`]; storage may evolve behind
//! them. The contract note is
//! `_docs/architecture/2026-10-25-11-00-architecture-shared-observational-catalog-contract.md`.
//!
//! # What this crate does not do
//!
//! It does not read Jira, Paseo or any network. It does not write a plan, create
//! a registry entry, enroll a project, activate a pin or mutate a session. It
//! does not spawn `asma`: the plugin keeps exact-source graph export. And it
//! does not watch the filesystem — a watch may accelerate a sweep in a later
//! task, but correctness must survive its loss, so none is installed here.
//!
//! # Independence from governed execution
//!
//! The catalog opens its own SQLite file and needs no governed project, no
//! scheduler admission and no execution reconciliation. Zero configured
//! repositories is a valid catalog that reports `complete` with zero coverage.

pub mod config;
pub mod contract;
pub mod discovery;
pub mod jira;
pub mod jira_contract;
pub mod jira_store;
pub mod native;
pub mod store;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kontor_core::id::{SignedDuration, Timestamp};
use tracing::{info, warn};

pub use config::{
    CatalogSettings, Environment, JiraSettings, OperatorSource, ProcessEnvironment,
    RepositoryConfig, SourceKind, SourceRevisionKind,
};
pub use contract::{
    CATALOG_CONTRACT_VERSION, CatalogEntry, CatalogSource, CatalogSourceConflict,
    CatalogSourceCoverage, CatalogSourceState, CatalogSourceStatus, CatalogSourcesView,
    CatalogStatus, CollectionState, ConfigOrigin, ContentKind, EntriesView, EntryFilter, EntryKind,
    EntryState, ReadSource, RevisionKind,
};
pub use jira::{JiraCatalog, JiraCatalogError, JiraSweepReport, poll_jira_until_stopped};
pub use jira_contract::{
    EPIC_IDENTITY_SCHEMA, EpicIdentitySource, JiraAlias, JiraCollectionCoverage,
    JiraCollectionState, JiraEvidenceDocument, JiraIssueObservation, JiraJoinDiagnostic,
    JiraObservationState, JiraObservationVia, JiraProjectionError, JiraScope, JiraSweepMode,
    JiraWorkstreamJoin, epic_identity_source, verified_workstream_join,
};
pub use kontor_jira::JiraReadFailure;
pub use native::{
    NativeCensus, NativeCensusSource, NativeCollectionStatus, NativeInventoryFilter,
    NativeInventoryView, NativeProject, NativeProjectRecord, NativeSession, NativeSessionRecord,
    NativeSweepReport, NativeWorkspace, NativeWorkspaceRecord,
};
pub use store::{ObservedEntry, ObservedOccurrence, StoreError};

/// Everything the catalog can refuse.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CatalogError {
    /// The catalog database refused.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Another live collector owns this catalog scope.
    #[error("catalog scope is already owned by `{owner}`")]
    OwnerConflict {
        /// The current owner.
        owner: String,
    },
    /// The settings could not be read or written.
    #[error("catalog configuration refused: {detail}")]
    Config {
        /// A structural explanation; never file bytes.
        detail: String,
    },
    /// A cursor names a generation that is no longer the active answer.
    #[error("the cursor generation {generation} is not the active generation {active}")]
    StaleCursor {
        /// The generation carried by the cursor.
        generation: u64,
        /// The generation this read would answer from.
        active: u64,
    },
    /// A cursor is malformed or was issued for another filter.
    #[error("the catalog cursor is not valid for this read: {detail}")]
    InvalidCursor {
        /// A structural explanation.
        detail: String,
    },
    /// Neither the database nor a snapshot could answer.
    #[error("the catalog is unavailable: {detail}")]
    Unavailable {
        /// A structural explanation.
        detail: String,
    },
}

impl From<config::ConfigError> for CatalogError {
    fn from(error: config::ConfigError) -> Self {
        Self::Config {
            detail: error.to_string(),
        }
    }
}

/// Effective settings and their provenance.
#[derive(Debug, Clone)]
pub struct EffectiveSettings {
    /// The settings a sweep uses.
    pub settings: CatalogSettings,
    /// Where they came from.
    pub origin: ConfigOrigin,
    /// The SHA-256 of the effective document, when one exists.
    pub revision: Option<String>,
    /// Structural resolution diagnostics.
    pub diagnostics: Vec<String>,
}

/// The result of one collection attempt.
#[derive(Debug, Clone)]
pub struct SweepReport {
    /// The generation attempted.
    pub generation: u64,
    /// Whether the sweep finished every configured root.
    pub finished: bool,
    /// The collection state this attempt recorded.
    pub state: CollectionState,
    /// Plan documents observed.
    pub entries: usize,
    /// Files considered.
    pub files_seen: usize,
    /// Measured duration.
    pub duration_ms: u64,
    /// Structural attempt diagnostics.
    pub diagnostics: Vec<String>,
    /// Whether the fallback snapshot was replaced.
    pub snapshot_written: bool,
}

/// One host-local catalog.
pub struct Catalog {
    database: store::Database,
    state_root: PathBuf,
    owner_id: String,
    /// The owner id is written by one process; the release method requires it,
    /// so it is held outside the connection rather than re-read from the file.
    released: Mutex<bool>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Catalog")
            .field("state_root", &self.state_root)
            .field("owner_id", &self.owner_id)
            .finish_non_exhaustive()
    }
}

impl Catalog {
    /// Open the catalog under a state root.
    ///
    /// # Errors
    /// A SQLite refusal or an unusable state root.
    pub fn open(state_root: impl AsRef<Path>) -> Result<Self, CatalogError> {
        Self::open_with_owner(state_root, format!("catalog-{}", uuid::Uuid::now_v7()))
    }

    /// Open with an explicit owner id.
    ///
    /// This exists for qualification: a second catalog in one process must be
    /// able to prove the one-owner refusal without pretending to be a second
    /// host process.
    ///
    /// # Errors
    /// A SQLite refusal or an unusable state root.
    pub fn open_with_owner(
        state_root: impl AsRef<Path>,
        owner_id: impl Into<String>,
    ) -> Result<Self, CatalogError> {
        let state_root = state_root.as_ref().to_path_buf();
        std::fs::create_dir_all(&state_root).map_err(|error| CatalogError::Config {
            detail: format!("the state root could not be created ({})", error.kind()),
        })?;
        let database = store::Database::open(&Self::database_path(&state_root))?;
        Ok(Self {
            database,
            state_root,
            owner_id: owner_id.into(),
            released: Mutex::new(false),
        })
    }

    /// The catalog database path inside a state root.
    #[must_use]
    pub fn database_path(state_root: &Path) -> PathBuf {
        state_root.join(config::CATALOG_DATABASE_FILE)
    }

    /// The fallback snapshot path inside a state root.
    #[must_use]
    pub fn snapshot_path(state_root: &Path) -> PathBuf {
        state_root.join(config::CATALOG_SNAPSHOT_FILE)
    }

    /// The settings path inside a state root.
    #[must_use]
    pub fn settings_path(state_root: &Path) -> PathBuf {
        state_root.join(config::CATALOG_CONFIG_FILE)
    }

    /// The state root this catalog owns.
    #[must_use]
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// The catalog's own database, for the collectors in this crate.
    #[must_use]
    pub(crate) fn store(&self) -> &store::Database {
        &self.database
    }

    /// Take or renew the one collector lease for a collector that does not run
    /// the local scan, such as the Jira observation.
    ///
    /// # Errors
    /// [`CatalogError::OwnerConflict`] for a live competing owner, or a store
    /// refusal.
    pub(crate) fn acquire_owner_at(
        &self,
        now: &Timestamp,
        lease_seconds: u64,
    ) -> Result<(), CatalogError> {
        self.database
            .acquire_owner(&self.owner_id, lease_seconds, now)
            .map(|_| ())
            .map_err(|error| match error {
                StoreError::OwnerConflict { owner } => CatalogError::OwnerConflict { owner },
                other => CatalogError::Store(other),
            })?;
        {
            let mut released = self
                .released
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *released = false;
        }
        Ok(())
    }

    /// This process's collector owner id.
    #[must_use]
    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }

    /// The database's schema version.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn schema_version(&self) -> Result<i64, CatalogError> {
        Ok(self.database.schema_version()?)
    }

    /// Resolve the effective settings, materializing a migrated plugin
    /// configuration exactly once.
    ///
    /// # Errors
    /// Never for an unreadable settings document: that is a diagnostic with
    /// zero configured roots, so a broken file cannot stop the daemon or erase
    /// a previous generation.
    pub fn effective_settings(&self) -> EffectiveSettings {
        let now = Timestamp::now().to_string();
        let mut resolved = config::resolve(&self.state_root, &ProcessEnvironment, &now);
        let mut diagnostics = resolved.diagnostics.clone();
        let mut origin = resolved.origin;
        let mut revision = resolved.revision.clone();
        if resolved.materialize {
            let migrated_from = resolved
                .settings
                .migrated_from
                .unwrap_or(ConfigOrigin::MigratedPluginFile);
            resolved.settings.migrated_from = Some(migrated_from);
            if resolved.settings.imported_at.is_none() {
                resolved.settings.imported_at = Some(now.clone());
            }
            match config::materialize(&self.state_root, &resolved.settings) {
                Ok(path) => {
                    if let Ok(bytes) = std::fs::read(&path) {
                        revision = Some(config::sha256(&bytes));
                    }
                    origin = migrated_from;
                }
                Err(error) => {
                    diagnostics.push(format!(
                        "The migrated configuration could not be written ({}); the in-memory settings were used for this sweep.",
                        error.kind()
                    ));
                }
            }
        } else if let Some(migrated) = resolved.settings.migrated_from {
            origin = migrated;
        }
        let settings_json =
            serde_json::to_string(&resolved.settings).unwrap_or_else(|_| "{}".to_owned());
        if let Err(error) = self.record_config(origin, revision.as_deref(), &now, &settings_json) {
            diagnostics.push(format!(
                "The configuration provenance could not be recorded ({}).",
                match &error {
                    StoreError::Sqlite(_) => "sqlite",
                    StoreError::Json(_) => "json",
                    StoreError::Io(_) => "io",
                    _ => "catalog",
                }
            ));
        }
        EffectiveSettings {
            settings: resolved.settings,
            origin,
            revision,
            diagnostics: {
                diagnostics.sort();
                diagnostics.dedup();
                diagnostics
            },
        }
    }

    /// Run one collection sweep.
    ///
    /// The sweep takes or renews the one owner lease. A competing owner is
    /// refused before any scan begins.
    ///
    /// # Errors
    /// [`CatalogError::OwnerConflict`] for a live competing owner, or a store
    /// refusal.
    pub async fn collect_once(&self) -> Result<SweepReport, CatalogError> {
        let effective = self.effective_settings();
        let started = Instant::now();
        let now = Timestamp::now();
        let lease_seconds = (effective.settings.cadence_seconds.saturating_mul(2))
            .saturating_add(effective.settings.scan_budget_seconds)
            .max(90);
        self.database
            .acquire_owner(&self.owner_id, lease_seconds, &now)
            .map_err(|error| match error {
                StoreError::OwnerConflict { owner } => CatalogError::OwnerConflict { owner },
                other => CatalogError::Store(other),
            })?;
        {
            let mut released = self
                .released
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *released = false;
        }
        let sweep = self.database.begin_sweep(&self.owner_id, &now)?;
        let discovered = discovery::discover(&effective.settings, now).await;

        let mut diagnostics = effective.diagnostics;
        diagnostics.extend(discovered.diagnostics.clone());
        let mut page_index = 0u32;
        for page in discovered.entries.chunks(128) {
            let files = if page_index == 0 {
                discovered.files_seen
            } else {
                0
            };
            if let Err(error) = self
                .database
                .commit_page(&sweep, page_index, &now, page, files)
            {
                let detail = format!("A catalog page could not be committed ({error}).");
                diagnostics.push(detail.clone());
                let _ = self
                    .database
                    .fail_sweep(&sweep, &detail, &diagnostics, &now);
                return Err(CatalogError::Store(error));
            }
            page_index += 1;
        }
        // A sweep with no pages still records its checkpoint.
        if page_index == 0
            && let Err(error) =
                self.database
                    .commit_page(&sweep, 0, &now, &[], discovered.files_seen)
        {
            let detail = format!("The catalog checkpoint could not be committed ({error}).");
            diagnostics.push(detail.clone());
            let _ = self
                .database
                .fail_sweep(&sweep, &detail, &diagnostics, &now);
            return Err(CatalogError::Store(error));
        }

        let healthy = discovered
            .coverage
            .iter()
            .all(|coverage| coverage.state == contract::RepositoryState::Scanned);
        let error = if discovered.finished {
            (!healthy).then(|| {
                format!(
                    "{} configured repositories could not be read; their prior facts were retained.",
                    discovered
                        .coverage
                        .iter()
                        .filter(|coverage| coverage.state != contract::RepositoryState::Scanned)
                        .count()
                )
            })
        } else {
            Some(
                "The scan budget expired before every configured root was read; no prior fact was removed."
                    .to_owned(),
            )
        };
        let outcome = store::SweepOutcome {
            complete: discovered.finished,
            coverage: discovered.coverage,
            diagnostics: diagnostics.clone(),
            registry: discovered.registry,
            files_seen: discovered.files_seen,
            entries_seen: discovered.entries.len(),
            error: error.clone(),
            scanned_repository_ids: discovered.scanned_repository_ids,
        };
        self.database.finalize_sweep(&sweep, &outcome, &now)?;

        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let state = if discovered.finished && healthy {
            CollectionState::Complete
        } else {
            CollectionState::Incomplete
        };
        let mut snapshot_written = false;
        if discovered.finished {
            match self.write_snapshot() {
                Ok(()) => snapshot_written = true,
                Err(error) => diagnostics.push(format!(
                    "The fallback snapshot could not be written ({error})."
                )),
            }
        }
        info!(
            generation = sweep.generation,
            entries = discovered.entries.len(),
            files = discovered.files_seen,
            duration_ms,
            ?state,
            "work catalog sweep finished"
        );
        Ok(SweepReport {
            generation: sweep.generation,
            finished: discovered.finished,
            state,
            entries: discovered.entries.len(),
            files_seen: discovered.files_seen,
            duration_ms,
            diagnostics,
            snapshot_written,
        })
    }

    /// Run one headless native census over every configured source.
    ///
    /// This shares the collector lease with plan discovery but not its
    /// generation, collection row or snapshot. A Paseo outage therefore leaves
    /// local plan freshness untouched, and a plan sweep cannot make native
    /// observations look fresher.
    ///
    /// # Errors
    /// [`CatalogError::OwnerConflict`] for a live competing owner, or a store
    /// refusal.
    pub async fn collect_native_once(
        &self,
        sources: &[Arc<dyn native::NativeCensusSource>],
    ) -> Result<native::NativeSweepReport, CatalogError> {
        let effective = self.effective_settings();
        let started = Instant::now();
        let now = Timestamp::now();
        let lease_seconds = (effective.settings.cadence_seconds.saturating_mul(2))
            .saturating_add(effective.settings.scan_budget_seconds)
            .max(90);
        self.database
            .acquire_owner(&self.owner_id, lease_seconds, &now)
            .map_err(|error| match error {
                StoreError::OwnerConflict { owner } => CatalogError::OwnerConflict { owner },
                other => CatalogError::Store(other),
            })?;
        {
            let mut released = self
                .released
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *released = false;
        }
        let sweep = self.database.begin_native_sweep(&self.owner_id, &now)?;

        let mut projects = Vec::new();
        let mut workspaces = Vec::new();
        let mut sessions = Vec::new();
        let mut coverage = Vec::new();
        let mut diagnostics = effective.diagnostics.clone();
        let mut first_error: Option<String> = None;
        let mut include_archived = !sources.is_empty();
        for source in sources {
            let census = source.observe().await;
            if census.runtime_instance != source.runtime_instance() {
                let detail = format!(
                    "A native source answered as `{}` while configured as `{}`; the observation was not promoted.",
                    census.runtime_instance,
                    source.runtime_instance()
                );
                diagnostics.push(detail.clone());
                include_archived = false;
                if first_error.is_none() {
                    first_error = Some(detail.clone());
                }
                let mut failed = native::NativeRuntimeCoverage::from_census(&census);
                failed.runtime_instance = source.runtime_instance().to_owned();
                failed.client_id = source.client_id().to_owned();
                failed.state = native::NativeRuntimeState::Failed;
                failed.project_pages = 0;
                failed.workspace_pages = 0;
                failed.session_pages = 0;
                failed.projects = 0;
                failed.workspaces = 0;
                failed.sessions = 0;
                failed.archived_sessions = 0;
                failed.error = Some(detail);
                coverage.push(failed);
                continue;
            }
            if !census.include_archived {
                include_archived = false;
            }
            if first_error.is_none() {
                first_error = census.error.clone();
            }
            diagnostics.extend(
                census
                    .diagnostics
                    .iter()
                    .map(|detail| format!("{}: {detail}", census.runtime_instance)),
            );
            coverage.push(native::NativeRuntimeCoverage::from_census(&census));
            projects.extend(census.projects);
            workspaces.extend(census.workspaces);
            sessions.extend(census.sessions);
        }
        let archived_sessions = sessions
            .iter()
            .filter(|session| session.archived_at.is_some())
            .count();
        let complete = coverage
            .iter()
            .all(|runtime| runtime.state == native::NativeRuntimeState::Scanned);
        let retry_at = (!complete)
            .then(|| native::retry_after(&now, effective.settings.cadence_seconds.clamp(1, 3600)));

        let mut page_index = 0u32;
        let mut commit = |kind: &str, items_json: &[serde_json::Value]| {
            let result = self
                .database
                .commit_native_page(&sweep, page_index, kind, &now, items_json);
            if result.is_ok() {
                page_index += 1;
            }
            result
        };
        let project_pages: Vec<Vec<serde_json::Value>> = projects
            .chunks(128)
            .map(|page| page.iter().map(|item| serde_json::json!(item)).collect())
            .collect();
        let workspace_pages: Vec<Vec<serde_json::Value>> = workspaces
            .chunks(128)
            .map(|page| page.iter().map(|item| serde_json::json!(item)).collect())
            .collect();
        let session_pages: Vec<Vec<serde_json::Value>> = sessions
            .chunks(128)
            .map(|page| page.iter().map(|item| serde_json::json!(item)).collect())
            .collect();
        for page in &project_pages {
            if let Err(error) = commit("projects", page) {
                let detail = format!("A native project page could not be committed ({error}).");
                diagnostics.push(detail.clone());
                let _ = self.database.fail_native_sweep(
                    &sweep,
                    &detail,
                    &diagnostics,
                    &coverage,
                    retry_at.as_deref(),
                    &now,
                );
                return Err(CatalogError::Store(error));
            }
        }
        for page in &workspace_pages {
            if let Err(error) = commit("workspaces", page) {
                let detail = format!("A native workspace page could not be committed ({error}).");
                diagnostics.push(detail.clone());
                let _ = self.database.fail_native_sweep(
                    &sweep,
                    &detail,
                    &diagnostics,
                    &coverage,
                    retry_at.as_deref(),
                    &now,
                );
                return Err(CatalogError::Store(error));
            }
        }
        for page in &session_pages {
            if let Err(error) = commit("sessions", page) {
                let detail = format!("A native session page could not be committed ({error}).");
                diagnostics.push(detail.clone());
                let _ = self.database.fail_native_sweep(
                    &sweep,
                    &detail,
                    &diagnostics,
                    &coverage,
                    retry_at.as_deref(),
                    &now,
                );
                return Err(CatalogError::Store(error));
            }
        }
        // A source with no observations still records its checkpoint.
        if page_index == 0
            && let Err(error) = self.database.commit_native_page::<serde_json::Value>(
                &sweep,
                0,
                "projects",
                &now,
                &[],
            )
        {
            let detail = format!("The native checkpoint could not be committed ({error}).");
            diagnostics.push(detail.clone());
            let _ = self.database.fail_native_sweep(
                &sweep,
                &detail,
                &diagnostics,
                &coverage,
                retry_at.as_deref(),
                &now,
            );
            return Err(CatalogError::Store(error));
        }

        let outcome = native::NativeSweepOutcome {
            complete,
            coverage,
            projects_seen: projects.len(),
            workspaces_seen: workspaces.len(),
            sessions_seen: sessions.len(),
            archived_sessions_seen: archived_sessions,
            include_archived,
            diagnostics: diagnostics.clone(),
            error: first_error.clone(),
            retry_at: retry_at.clone(),
        };
        self.database
            .finalize_native_sweep(&sweep, &outcome, &now)?;

        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let state = if complete {
            CollectionState::Complete
        } else if !outcome.coverage.is_empty()
            && outcome
                .coverage
                .iter()
                .all(|runtime| runtime.state == native::NativeRuntimeState::Failed)
        {
            CollectionState::Failed
        } else {
            CollectionState::Incomplete
        };
        let mut snapshot_written = false;
        if complete {
            match self.write_native_snapshot(&now.to_string()) {
                Ok(()) => snapshot_written = true,
                Err(error) => diagnostics.push(format!(
                    "The native fallback snapshot could not be written ({error})."
                )),
            }
        }
        info!(
            generation = sweep.generation,
            projects = projects.len(),
            workspaces = workspaces.len(),
            sessions = sessions.len(),
            duration_ms,
            ?state,
            "native work census finished"
        );
        Ok(native::NativeSweepReport {
            generation: sweep.generation,
            complete,
            state,
            projects: projects.len(),
            workspaces: workspaces.len(),
            sessions: sessions.len(),
            archived_sessions,
            pages: page_index,
            duration_ms,
            diagnostics,
            snapshot_written,
        })
    }

    /// The native census's freshness, coverage, ownership and counts.
    ///
    /// # Errors
    /// Neither the database nor a compatible native snapshot could answer.
    pub fn native_status(&self) -> Result<native::NativeCatalogStatus, CatalogError> {
        match self.database.read_native_status() {
            Ok(stored) => {
                let mut status = native::NativeCatalogStatus {
                    schema_version: native::NATIVE_CENSUS_VERSION,
                    source: ReadSource::Database,
                    generation: stored.generation,
                    attempted_generation: stored.attempted_generation,
                    as_of: stored.as_of,
                    collector: contract::CollectorStatus {
                        owner: stored.owner.owner.clone(),
                        lease_until: stored.owner.lease_until.clone(),
                        owner_here: stored.owner.owner.as_deref() == Some(self.owner_id.as_str()),
                        last_error: stored.collection.error.clone(),
                    },
                    collection: stored.collection,
                    counts: stored.counts,
                    snapshot: contract::SnapshotStatus {
                        present: false,
                        generation: None,
                        written_at: None,
                        current: false,
                        error: None,
                    },
                };
                status.snapshot = self.native_snapshot_status(status.generation);
                Ok(status)
            }
            Err(database_error) => match self.read_native_snapshot() {
                Ok(Some(snapshot)) => {
                    let mut status = snapshot.status;
                    status.source = ReadSource::Snapshot;
                    status.snapshot = self.native_snapshot_status(status.generation);
                    Ok(status)
                }
                Ok(None) => Err(CatalogError::Unavailable {
                    detail: format!(
                        "the native census could not be read ({}) and no fallback snapshot exists",
                        error_class(&database_error)
                    ),
                }),
                Err(snapshot_error) => Err(CatalogError::Unavailable {
                    detail: format!(
                        "the native census ({}) and its fallback snapshot ({snapshot_error}) could not be read",
                        error_class(&database_error)
                    ),
                }),
            },
        }
    }

    /// One generation-consistent native inventory.
    ///
    /// # Errors
    /// Neither the database nor a compatible native snapshot could answer.
    pub fn native_inventory(
        &self,
        filter: &native::NativeInventoryFilter,
    ) -> Result<native::NativeInventoryView, CatalogError> {
        match self
            .database
            .read_native_inventory(filter.include_removed, filter.runtime_instance.as_deref())
        {
            Ok(stored) => native_inventory_view(
                native::NATIVE_CENSUS_VERSION,
                stored.generation,
                stored.as_of,
                ReadSource::Database,
                stored.archive_scope,
                stored.projects,
                stored.workspaces,
                stored.sessions,
                filter,
            ),
            Err(database_error) => match self.read_native_snapshot() {
                Ok(Some(snapshot)) => {
                    let archive_scope = snapshot.status.collection.include_archived;
                    native_inventory_view(
                        snapshot.schema_version,
                        snapshot.generation,
                        Some(snapshot.written_at),
                        ReadSource::Snapshot,
                        archive_scope,
                        snapshot.projects,
                        snapshot.workspaces,
                        snapshot.sessions,
                        filter,
                    )
                }
                Ok(None) => Err(CatalogError::Unavailable {
                    detail: format!(
                        "the native census could not be read ({}) and no fallback snapshot exists",
                        error_class(&database_error)
                    ),
                }),
                Err(snapshot_error) => Err(CatalogError::Unavailable {
                    detail: format!(
                        "the native census ({}) and its fallback snapshot ({snapshot_error}) could not be read",
                        error_class(&database_error)
                    ),
                }),
            },
        }
    }

    /// Read the native fallback snapshot, refusing an incompatible document.
    ///
    /// # Errors
    /// A read, parse or version refusal.
    pub fn read_native_snapshot(
        &self,
    ) -> Result<Option<native::NativeCatalogSnapshot>, CatalogError> {
        let path = Self::native_snapshot_path(&self.state_root);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(CatalogError::Unavailable {
                    detail: format!("the native snapshot could not be read ({})", error.kind()),
                });
            }
        };
        let snapshot: native::NativeCatalogSnapshot =
            serde_json::from_slice(&bytes).map_err(|_| CatalogError::Unavailable {
                detail: "the native snapshot is not a valid census document".to_owned(),
            })?;
        if snapshot.schema_version != native::NATIVE_CENSUS_VERSION {
            return Err(CatalogError::Unavailable {
                detail: format!(
                    "the native snapshot version {} is not supported ({})",
                    snapshot.schema_version,
                    native::NATIVE_CENSUS_VERSION
                ),
            });
        }
        Ok(Some(snapshot))
    }

    /// The native fallback snapshot path inside a state root.
    #[must_use]
    pub fn native_snapshot_path(state_root: &Path) -> PathBuf {
        state_root.join(native::NATIVE_SNAPSHOT_FILE)
    }

    fn write_native_snapshot(&self, written_at: &str) -> Result<(), CatalogError> {
        let mut status = self.native_status()?;
        let projection = self.database.native_snapshot_projection()?;
        let generation = projection.generation;
        status.source = ReadSource::Database;
        status.snapshot = contract::SnapshotStatus {
            present: true,
            generation: Some(generation),
            written_at: Some(written_at.to_owned()),
            current: true,
            error: None,
        };
        let snapshot = native::NativeCatalogSnapshot {
            schema_version: native::NATIVE_CENSUS_VERSION,
            generation,
            written_at: written_at.to_owned(),
            status,
            projects: projection.projects,
            workspaces: projection.workspaces,
            sessions: projection.sessions,
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(|error| CatalogError::Config {
            detail: format!("the native snapshot could not be encoded ({error})"),
        })?;
        let path = Self::native_snapshot_path(&self.state_root);
        let temporary = self.state_root.join(format!(
            "{}.{}.tmp",
            native::NATIVE_SNAPSHOT_FILE,
            uuid::Uuid::now_v7()
        ));
        {
            use std::io::Write as _;
            let mut file =
                std::fs::File::create(&temporary).map_err(|error| CatalogError::Config {
                    detail: format!(
                        "the native snapshot could not be created ({})",
                        error.kind()
                    ),
                })?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| CatalogError::Config {
                    detail: format!(
                        "the native snapshot could not be written ({})",
                        error.kind()
                    ),
                })?;
        }
        std::fs::rename(&temporary, &path).map_err(|error| CatalogError::Config {
            detail: format!(
                "the native snapshot could not be published ({})",
                error.kind()
            ),
        })?;
        Ok(())
    }

    fn native_snapshot_status(&self, active: u64) -> contract::SnapshotStatus {
        match self.read_native_snapshot() {
            Ok(Some(snapshot)) => contract::SnapshotStatus {
                present: true,
                generation: Some(snapshot.generation),
                written_at: Some(snapshot.written_at),
                current: snapshot.generation == active,
                error: None,
            },
            Ok(None) => contract::SnapshotStatus {
                present: false,
                generation: None,
                written_at: None,
                current: false,
                error: None,
            },
            Err(error) => contract::SnapshotStatus {
                present: true,
                generation: None,
                written_at: None,
                current: false,
                error: Some(error.to_string()),
            },
        }
    }

    /// Release this process's lease, if it still holds it.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn release_owner(&self) -> Result<(), CatalogError> {
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *released {
            return Ok(());
        }
        self.database
            .release_owner(&self.owner_id, &Timestamp::now())?;
        *released = true;
        Ok(())
    }

    /// The current status, from the database or the fallback snapshot.
    ///
    /// # Errors
    /// Neither the database nor a compatible snapshot could answer.
    pub fn status(&self) -> Result<CatalogStatus, CatalogError> {
        let effective = self.effective_settings();
        match self.database.read_status() {
            Ok(store_status) => {
                let mut status = self.project_status(store_status, &effective);
                status.snapshot = self.snapshot_status(status.generation);
                Ok(status)
            }
            Err(database_error) => {
                warn!(detail = %database_error, "catalog database read failed; trying the fallback snapshot");
                match self.read_snapshot() {
                    Ok(Some(snapshot)) => {
                        let mut status = snapshot.status;
                        status.source = ReadSource::Snapshot;
                        status.snapshot = self.snapshot_status(status.generation);
                        Ok(status)
                    }
                    Ok(None) => Err(CatalogError::Unavailable {
                        detail: format!(
                            "the catalog database could not be read ({}) and no fallback snapshot exists",
                            error_class(&database_error)
                        ),
                    }),
                    Err(snapshot_error) => Err(CatalogError::Unavailable {
                        detail: format!(
                            "the catalog database ({}) and its fallback snapshot ({snapshot_error}) could not be read",
                            error_class(&database_error)
                        ),
                    }),
                }
            }
        }
    }

    /// One generation-consistent entries answer, with the basic filters.
    ///
    /// A cursor is bound to the generation and filter that issued it. Presenting
    /// it after the active generation moved is refused rather than answered from
    /// two generations, because a page that spans a sweep is not one observation.
    ///
    /// # Errors
    /// Neither the database nor a compatible snapshot could answer, the cursor
    /// belongs to another generation, or it is malformed.
    pub fn entries(&self, filter: &EntryFilter) -> Result<EntriesView, CatalogError> {
        match self.database.read_entries(filter.include_removed) {
            Ok(read) => {
                let (entries, truncated, next_cursor) =
                    filter_entries(read.entries, filter, read.generation)?;
                Ok(EntriesView {
                    schema_version: CATALOG_CONTRACT_VERSION,
                    generation: read.generation,
                    as_of: read.as_of,
                    source: ReadSource::Database,
                    entries,
                    truncated,
                    next_cursor,
                })
            }
            Err(database_error) => match self.read_snapshot() {
                Ok(Some(snapshot)) => {
                    let (entries, truncated, next_cursor) =
                        filter_entries(snapshot.entries, filter, snapshot.generation)?;
                    Ok(EntriesView {
                        schema_version: CATALOG_CONTRACT_VERSION,
                        generation: snapshot.generation,
                        as_of: Some(snapshot.written_at),
                        source: ReadSource::Snapshot,
                        entries,
                        truncated,
                        next_cursor,
                    })
                }
                Ok(None) => Err(CatalogError::Unavailable {
                    detail: format!(
                        "the catalog database could not be read ({}) and no fallback snapshot exists",
                        error_class(&database_error)
                    ),
                }),
                Err(snapshot_error) => Err(CatalogError::Unavailable {
                    detail: format!(
                        "the catalog database ({}) and its fallback snapshot ({snapshot_error}) could not be read",
                        error_class(&database_error)
                    ),
                }),
            },
        }
    }

    /// Per-source freshness, coverage, errors, retry deadlines and conflicts.
    ///
    /// A source that cannot be read is reported as failed in its own row rather
    /// than failing the whole answer: the observer surface exists to say which
    /// observation is missing. It never substitutes the request instant for a
    /// source's completion instant.
    ///
    /// # Errors
    /// The catalog settings could not be read. An unreadable source is a row.
    pub fn sources(&self) -> Result<CatalogSourcesView, CatalogError> {
        let effective = self.effective_settings();
        let sources = vec![
            self.plan_source_status(&effective),
            self.jira_source_status(&effective),
            self.native_source_status(),
        ];
        Ok(CatalogSourcesView {
            schema_version: CATALOG_CONTRACT_VERSION,
            sources,
        })
    }

    fn plan_source_status(&self, effective: &EffectiveSettings) -> CatalogSourceStatus {
        let pending_verification = self.database.count_unverified_associations().unwrap_or(0);
        let mut coverage = Vec::new();
        let mut conflicts = Vec::new();
        match self.status() {
            Ok(status) => {
                let read = match status.source {
                    ReadSource::Database => "database",
                    ReadSource::Snapshot => "fallback snapshot",
                };
                coverage.push(CatalogSourceCoverage {
                    subject: "collection".to_owned(),
                    state: collection_state_name(status.collection.state).to_owned(),
                    detail: format!(
                        "{} of {} repositories read, {} files, {} entries, read from {read}",
                        status.collection.repositories_scanned,
                        status.collection.repositories_configured,
                        status.collection.files_seen,
                        status.collection.entries_observed
                    ),
                });
                for repository in status
                    .collection
                    .coverage
                    .iter()
                    .take(MAX_SOURCE_COVERAGE_ROWS - PLAN_SUMMARY_COVERAGE_ROWS)
                {
                    coverage.push(CatalogSourceCoverage {
                        subject: repository.root.clone(),
                        state: repository_state_name(repository.state).to_owned(),
                        detail: format!(
                            "{} files, {} entries{}",
                            repository.files_seen,
                            repository.entries_observed,
                            repository
                                .error
                                .as_ref()
                                .map(|error| format!("; {error}"))
                                .unwrap_or_default()
                        ),
                    });
                }
                coverage.push(CatalogSourceCoverage {
                    subject: "registry".to_owned(),
                    state: registry_state_name(status.registry.state).to_owned(),
                    detail: format!(
                        "{} workstreams; {} diagnostics",
                        status.registry.workstreams,
                        status.registry.diagnostics.len()
                    ),
                });
                coverage.push(CatalogSourceCoverage {
                    subject: "snapshot".to_owned(),
                    state: if status.snapshot.current {
                        "current"
                    } else if status.snapshot.present {
                        "stale"
                    } else {
                        "absent"
                    }
                    .to_owned(),
                    detail: status
                        .snapshot
                        .written_at
                        .as_ref()
                        .map(|written| format!("written {written}"))
                        .unwrap_or_else(|| "no fallback written".to_owned()),
                });
                conflicts = status
                    .conflicts
                    .into_iter()
                    .take(MAX_SOURCE_CONFLICTS)
                    .map(|conflict| CatalogSourceConflict {
                        kind: conflict_kind_name(conflict.kind).to_owned(),
                        subject: conflict.logical_plan_id,
                        message: conflict.message,
                        observed_at: Some(conflict.observed_at),
                    })
                    .collect();
                CatalogSourceStatus {
                    source: CatalogSource::Plan,
                    state: catalog_source_state(status.collection.state),
                    generation: status.generation,
                    attempted_generation: status.attempted_generation,
                    as_of: status.as_of.clone(),
                    error: status.collection.error.clone(),
                    retry_at: match status.collection.state {
                        CollectionState::Complete | CollectionState::NeverCollected => None,
                        _ => retry_after(
                            status.collection.finished_at.as_deref(),
                            effective.settings.cadence_seconds,
                        ),
                    },
                    coverage,
                    conflicts,
                    pending_verification,
                }
            }
            Err(error) => CatalogSourceStatus {
                source: CatalogSource::Plan,
                state: CatalogSourceState::Failed,
                generation: 0,
                attempted_generation: 0,
                as_of: None,
                error: Some(error.to_string()),
                retry_at: None,
                coverage,
                conflicts,
                pending_verification,
            },
        }
    }

    fn jira_source_status(&self, effective: &EffectiveSettings) -> CatalogSourceStatus {
        let configured = effective
            .settings
            .jira
            .as_ref()
            .is_some_and(|jira| jira.enabled);
        if !configured {
            return CatalogSourceStatus {
                source: CatalogSource::Jira,
                state: CatalogSourceState::NotConfigured,
                generation: 0,
                attempted_generation: 0,
                as_of: None,
                error: None,
                retry_at: None,
                coverage: vec![CatalogSourceCoverage {
                    subject: "configuration".to_owned(),
                    state: "not_configured".to_owned(),
                    detail: "No enabled Jira section is configured for this catalog.".to_owned(),
                }],
                conflicts: Vec::new(),
                pending_verification: 0,
            };
        }
        match self.database.jira_checkpoint().and_then(|checkpoint| {
            self.database
                .read_jira_evidence(false)
                .map(|evidence| (checkpoint, evidence))
        }) {
            Ok((checkpoint, evidence)) => {
                let mut coverage = Vec::new();
                // Declared keys are Jira's own coverage gap, not an association:
                // the frozen v1 contract records associations only for plan
                // declarations and operator choices.
                let pending_verification = 0;
                if let Some(coverage_value) = &evidence.coverage {
                    coverage.push(CatalogSourceCoverage {
                        subject: "scope".to_owned(),
                        state: coverage_value.state.as_str().to_owned(),
                        detail: format!(
                            "{:?} sweep, {} scope issues, {} pages, complete={}, census={}",
                            coverage_value.mode,
                            coverage_value.scope_issues,
                            coverage_value.pages,
                            coverage_value.complete,
                            coverage_value.census
                        )
                        .to_lowercase(),
                    });
                    coverage.push(CatalogSourceCoverage {
                        subject: "declared keys".to_owned(),
                        state: if coverage_value.missing.is_empty()
                            && coverage_value.unresolved.is_empty()
                        {
                            "complete"
                        } else {
                            "pending"
                        }
                        .to_owned(),
                        detail: format!(
                            "{} of {} found, {} missing, {} out of scope, {} unresolved parents",
                            coverage_value.found,
                            coverage_value.declared,
                            coverage_value.missing.len(),
                            coverage_value.out_of_scope.len(),
                            coverage_value.unresolved.len()
                        ),
                    });
                    // Declared keys are Jira's own coverage gap, not an
                    // association: the frozen v1 contract records
                    // associations only for plan declarations and operator
                    // choices, so Jira contributes no pending-verification
                    // associations.
                    coverage.push(CatalogSourceCoverage {
                        subject: "schedule".to_owned(),
                        state: if coverage_value.overdue {
                            "overdue"
                        } else {
                            "on_schedule"
                        }
                        .to_owned(),
                        detail: format!(
                            "last full sweep {}, next full sweep {}",
                            coverage_value
                                .last_full_sweep_at
                                .as_deref()
                                .unwrap_or("none"),
                            coverage_value
                                .next_full_sweep_at
                                .as_deref()
                                .unwrap_or("none")
                        ),
                    });
                }
                let conflicts = evidence
                    .aliases
                    .into_iter()
                    .take(MAX_SOURCE_CONFLICTS)
                    .map(|alias| CatalogSourceConflict {
                        kind: "moved_key".to_owned(),
                        subject: Some(alias.old_key.clone()),
                        message: format!(
                            "Jira answered {} with its current key {}.",
                            alias.old_key, alias.current_key
                        ),
                        observed_at: Some(alias.observed_at),
                    })
                    .collect();
                CatalogSourceStatus {
                    source: CatalogSource::Jira,
                    state: match evidence.latest_state {
                        jira_contract::JiraCollectionState::NeverCollected => {
                            CatalogSourceState::NeverCollected
                        }
                        jira_contract::JiraCollectionState::Complete => {
                            CatalogSourceState::Complete
                        }
                        jira_contract::JiraCollectionState::Incomplete => {
                            CatalogSourceState::Incomplete
                        }
                        jira_contract::JiraCollectionState::Failed => CatalogSourceState::Failed,
                    },
                    generation: evidence.generation,
                    attempted_generation: checkpoint.generation,
                    as_of: evidence.as_of,
                    error: evidence.latest_error,
                    retry_at: evidence.backoff_until,
                    coverage,
                    conflicts,
                    pending_verification,
                }
            }
            Err(error) => CatalogSourceStatus {
                source: CatalogSource::Jira,
                state: CatalogSourceState::Failed,
                generation: 0,
                attempted_generation: 0,
                as_of: None,
                error: Some(error.to_string()),
                retry_at: None,
                coverage: Vec::new(),
                conflicts: Vec::new(),
                pending_verification: 0,
            },
        }
    }

    fn native_source_status(&self) -> CatalogSourceStatus {
        match self.native_status() {
            Ok(status) => {
                let mut coverage: Vec<CatalogSourceCoverage> = status
                    .collection
                    .coverage
                    .into_iter()
                    .take(MAX_SOURCE_COVERAGE_ROWS - NATIVE_SUMMARY_COVERAGE_ROWS)
                    .map(|runtime| CatalogSourceCoverage {
                        subject: runtime.runtime_instance.clone(),
                        state: native_runtime_state_name(runtime.state).to_owned(),
                        detail: format!(
                            "{} projects, {} workspaces, {} sessions ({} archived), pages {}/{}/{}",
                            runtime.projects,
                            runtime.workspaces,
                            runtime.sessions,
                            runtime.archived_sessions,
                            runtime.project_pages,
                            runtime.workspace_pages,
                            runtime.session_pages
                        ),
                    })
                    .collect();
                coverage.push(CatalogSourceCoverage {
                    subject: "archive coverage".to_owned(),
                    state: if status.collection.include_archived {
                        "covered"
                    } else {
                        "not_covered"
                    }
                    .to_owned(),
                    detail: format!(
                        "{} of {} runtimes complete",
                        status.collection.runtimes_complete, status.collection.runtimes_configured
                    ),
                });
                coverage.push(CatalogSourceCoverage {
                    subject: "read source".to_owned(),
                    state: match status.source {
                        ReadSource::Database => "database",
                        ReadSource::Snapshot => "fallback_snapshot",
                    }
                    .to_owned(),
                    detail: "The native census answered from this read source.".to_owned(),
                });
                CatalogSourceStatus {
                    source: CatalogSource::Native,
                    state: catalog_source_state(status.collection.state),
                    generation: status.generation,
                    attempted_generation: status.attempted_generation,
                    as_of: status.as_of,
                    error: status.collection.error,
                    retry_at: status.collection.retry_at,
                    coverage,
                    conflicts: Vec::new(),
                    pending_verification: 0,
                }
            }
            Err(error) => CatalogSourceStatus {
                source: CatalogSource::Native,
                state: CatalogSourceState::Failed,
                generation: 0,
                attempted_generation: 0,
                as_of: None,
                error: Some(error.to_string()),
                retry_at: None,
                coverage: Vec::new(),
                conflicts: Vec::new(),
                pending_verification: 0,
            },
        }
    }

    /// Read the fallback snapshot, refusing an incompatible contract.
    ///
    /// # Errors
    /// A read, parse or contract-version refusal.
    pub fn read_snapshot(&self) -> Result<Option<contract::CatalogSnapshot>, CatalogError> {
        let path = Self::snapshot_path(&self.state_root);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(CatalogError::Unavailable {
                    detail: format!("the snapshot could not be read ({})", error.kind()),
                });
            }
        };
        let snapshot: contract::CatalogSnapshot =
            serde_json::from_slice(&bytes).map_err(|_| CatalogError::Unavailable {
                detail: "the snapshot is not a valid catalog document".to_owned(),
            })?;
        if snapshot.schema_version != CATALOG_CONTRACT_VERSION {
            return Err(CatalogError::Unavailable {
                detail: format!(
                    "the snapshot contract version {} is not supported ({CATALOG_CONTRACT_VERSION})",
                    snapshot.schema_version
                ),
            });
        }
        Ok(Some(snapshot))
    }

    fn write_snapshot(&self) -> Result<(), CatalogError> {
        let status = self.status()?;
        let (generation, _as_of, entries) = self.database.snapshot_projection()?;
        let snapshot = contract::CatalogSnapshot {
            schema_version: CATALOG_CONTRACT_VERSION,
            generation,
            written_at: Timestamp::now().to_string(),
            status,
            entries,
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(|error| CatalogError::Config {
            detail: format!("the snapshot could not be encoded ({error})"),
        })?;
        let path = Self::snapshot_path(&self.state_root);
        let temporary = self.state_root.join(format!(
            "{}.{}.tmp",
            config::CATALOG_SNAPSHOT_FILE,
            uuid::Uuid::now_v7()
        ));
        {
            use std::io::Write as _;
            let mut file =
                std::fs::File::create(&temporary).map_err(|error| CatalogError::Config {
                    detail: format!("the snapshot could not be created ({})", error.kind()),
                })?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| CatalogError::Config {
                    detail: format!("the snapshot could not be written ({})", error.kind()),
                })?;
        }
        std::fs::rename(&temporary, &path).map_err(|error| CatalogError::Config {
            detail: format!("the snapshot could not be published ({})", error.kind()),
        })?;
        Ok(())
    }

    fn snapshot_status(&self, active: u64) -> contract::SnapshotStatus {
        match self.read_snapshot() {
            Ok(Some(snapshot)) => contract::SnapshotStatus {
                present: true,
                generation: Some(snapshot.generation),
                written_at: Some(snapshot.written_at),
                current: snapshot.generation == active,
                error: None,
            },
            Ok(None) => contract::SnapshotStatus {
                present: false,
                generation: None,
                written_at: None,
                current: false,
                error: None,
            },
            Err(error) => contract::SnapshotStatus {
                present: true,
                generation: None,
                written_at: None,
                current: false,
                error: Some(error.to_string()),
            },
        }
    }

    fn project_status(
        &self,
        store_status: store::StoreStatus,
        effective: &EffectiveSettings,
    ) -> CatalogStatus {
        let settings = &effective.settings;
        CatalogStatus {
            schema_version: CATALOG_CONTRACT_VERSION,
            source: ReadSource::Database,
            catalog_id: store_status.catalog_id,
            generation: store_status.generation,
            attempted_generation: store_status.attempted_generation,
            as_of: store_status.as_of,
            collector: contract::CollectorStatus {
                owner: store_status.owner.owner.clone(),
                lease_until: store_status.owner.lease_until.clone(),
                owner_here: store_status.owner.owner.as_deref() == Some(self.owner_id.as_str()),
                last_error: store_status.collection.error.clone(),
            },
            collection: store_status.collection,
            counts: store_status.counts,
            registry: store_status.registry,
            config: contract::ConfigStatus {
                origin: effective.origin,
                revision: effective.revision.clone(),
                repositories: settings.repositories.len(),
                explicit_sources: settings.sources.len(),
                imported_at: settings.imported_at.clone(),
                diagnostics: effective.diagnostics.clone(),
            },
            snapshot: contract::SnapshotStatus {
                present: false,
                generation: None,
                written_at: None,
                current: false,
                error: None,
            },
            diagnostics: store_status.diagnostics,
            conflicts: store_status.conflicts,
            dangling_associations: store_status.dangling,
        }
    }

    fn record_config(
        &self,
        origin: ConfigOrigin,
        revision: Option<&str>,
        imported_at: &str,
        settings_json: &str,
    ) -> Result<(), StoreError> {
        self.database
            .record_config(origin, revision, imported_at, settings_json)
    }
}

#[allow(clippy::too_many_arguments)]
fn native_inventory_view(
    schema_version: u32,
    generation: u64,
    as_of: Option<String>,
    source: ReadSource,
    archive_scope: bool,
    projects: Vec<native::NativeProjectRecord>,
    workspaces: Vec<native::NativeWorkspaceRecord>,
    sessions: Vec<native::NativeSessionRecord>,
    filter: &native::NativeInventoryFilter,
) -> Result<native::NativeInventoryView, CatalogError> {
    let limit = if filter.limit == 0 {
        10_000
    } else {
        filter.limit.min(10_000)
    };
    let runtime = filter.runtime_instance.as_deref();
    let matches = |candidate: &str| runtime.is_none_or(|runtime| runtime == candidate);
    let projects: Vec<_> = projects
        .into_iter()
        .filter(|project| matches(&project.runtime_instance))
        .collect();
    let workspaces: Vec<_> = workspaces
        .into_iter()
        .filter(|workspace| matches(&workspace.runtime_instance))
        .collect();
    let sessions: Vec<_> = sessions
        .into_iter()
        .filter(|session| matches(&session.runtime_instance))
        .collect();
    let (project_offset, workspace_offset, session_offset) = match filter.cursor.as_deref() {
        Some(cursor) => parse_native_cursor(cursor, filter, generation)?,
        None => (0, 0, 0),
    };
    let page = |len: usize, offset: usize| {
        let start = offset.min(len);
        let end = start.saturating_add(limit).min(len);
        (start, end)
    };
    let (project_start, project_end) = page(projects.len(), project_offset);
    let (workspace_start, workspace_end) = page(workspaces.len(), workspace_offset);
    let (session_start, session_end) = page(sessions.len(), session_offset);
    let next_cursor = (project_end < projects.len()
        || workspace_end < workspaces.len()
        || session_end < sessions.len())
    .then(|| encode_native_cursor(generation, project_end, workspace_end, session_end, filter));
    Ok(native::NativeInventoryView {
        schema_version,
        generation,
        as_of,
        source,
        archive_scope,
        projects: projects[project_start..project_end].to_vec(),
        workspaces: workspaces[workspace_start..workspace_end].to_vec(),
        sessions: sessions[session_start..session_end].to_vec(),
        truncated: next_cursor.is_some(),
        next_cursor,
    })
}

const NATIVE_CURSOR_VERSION: &str = "n1";

fn native_cursor_fingerprint(filter: &native::NativeInventoryFilter) -> String {
    let removed = if filter.include_removed { "1" } else { "0" };
    let runtime = filter.runtime_instance.as_deref().unwrap_or("");
    let digest = config::sha256(format!("{removed}\u{0}{runtime}").as_bytes());
    digest[..16].to_owned()
}

fn encode_native_cursor(
    generation: u64,
    project_offset: usize,
    workspace_offset: usize,
    session_offset: usize,
    filter: &native::NativeInventoryFilter,
) -> String {
    format!(
        "{NATIVE_CURSOR_VERSION}:{generation}:{project_offset}:{workspace_offset}:{session_offset}:{}",
        native_cursor_fingerprint(filter)
    )
}

fn parse_native_cursor(
    cursor: &str,
    filter: &native::NativeInventoryFilter,
    generation: u64,
) -> Result<(usize, usize, usize), CatalogError> {
    let mut parts = cursor.split(':');
    let version = parts.next();
    let cursor_generation = parts.next().and_then(|value| value.parse::<u64>().ok());
    let project_offset = parts.next().and_then(|value| value.parse::<usize>().ok());
    let workspace_offset = parts.next().and_then(|value| value.parse::<usize>().ok());
    let session_offset = parts.next().and_then(|value| value.parse::<usize>().ok());
    let fingerprint = parts.next();
    let (
        Some(NATIVE_CURSOR_VERSION),
        Some(cursor_generation),
        Some(project_offset),
        Some(workspace_offset),
        Some(session_offset),
        Some(fingerprint),
    ) = (
        version,
        cursor_generation,
        project_offset,
        workspace_offset,
        session_offset,
        fingerprint,
    )
    else {
        return Err(CatalogError::InvalidCursor {
            detail: "the cursor is not a native catalog continuation".to_owned(),
        });
    };
    if parts.next().is_some() || fingerprint != native_cursor_fingerprint(filter) {
        return Err(CatalogError::InvalidCursor {
            detail: "the cursor was issued for another native filter".to_owned(),
        });
    }
    if cursor_generation != generation {
        return Err(CatalogError::StaleCursor {
            generation: cursor_generation,
            active: generation,
        });
    }
    Ok((project_offset, workspace_offset, session_offset))
}

const ENTRY_CURSOR_VERSION: &str = "v1";
const MAX_ENTRY_PAGE: usize = 1000;
const MAX_SOURCE_CONFLICTS: usize = 64;
const MAX_SOURCE_COVERAGE_ROWS: usize = 64;
const PLAN_SUMMARY_COVERAGE_ROWS: usize = 3;
const NATIVE_SUMMARY_COVERAGE_ROWS: usize = 2;

fn filter_entries(
    entries: Vec<CatalogEntry>,
    filter: &EntryFilter,
    generation: u64,
) -> Result<(Vec<CatalogEntry>, bool, Option<String>), CatalogError> {
    let limit = if filter.limit == 0 {
        MAX_ENTRY_PAGE
    } else {
        filter.limit.min(MAX_ENTRY_PAGE)
    };
    let scope = filter.scope.as_deref().filter(|scope| !scope.is_empty());
    let mut matched = Vec::new();
    for entry in entries {
        if filter.kind.is_some_and(|kind| kind != entry.kind) {
            continue;
        }
        if let Some(scope) = scope
            && !entry_matches_scope(&entry, scope)
        {
            continue;
        }
        matched.push(entry);
    }
    let offset = match filter.cursor.as_deref() {
        Some(cursor) => parse_cursor(cursor, filter, generation)?,
        None => 0,
    };
    if offset >= matched.len() {
        return Ok((Vec::new(), false, None));
    }
    let page_end = offset.saturating_add(limit).min(matched.len());
    let next_cursor =
        (page_end < matched.len()).then(|| encode_cursor(generation, page_end, filter));
    let page = matched.into_iter().skip(offset).take(limit).collect();
    Ok((page, next_cursor.is_some(), next_cursor))
}

fn cursor_filter_fingerprint(filter: &EntryFilter) -> String {
    let kind = filter.kind.map(EntryKind::as_str).unwrap_or("");
    let scope = filter.scope.as_deref().unwrap_or("");
    let removed = if filter.include_removed { "1" } else { "0" };
    let digest = config::sha256(format!("{kind}\u{0}{scope}\u{0}{removed}").as_bytes());
    digest[..16].to_owned()
}

fn encode_cursor(generation: u64, offset: usize, filter: &EntryFilter) -> String {
    format!(
        "{ENTRY_CURSOR_VERSION}:{generation}:{offset}:{}",
        cursor_filter_fingerprint(filter)
    )
}

fn parse_cursor(
    cursor: &str,
    filter: &EntryFilter,
    generation: u64,
) -> Result<usize, CatalogError> {
    let mut parts = cursor.split(':');
    let version = parts.next();
    let cursor_generation = parts.next().and_then(|value| value.parse::<u64>().ok());
    let offset = parts.next().and_then(|value| value.parse::<usize>().ok());
    let fingerprint = parts.next();
    let (Some(ENTRY_CURSOR_VERSION), Some(cursor_generation), Some(offset), Some(fingerprint)) =
        (version, cursor_generation, offset, fingerprint)
    else {
        return Err(CatalogError::InvalidCursor {
            detail: "the cursor is not a catalog continuation".to_owned(),
        });
    };
    if parts.next().is_some() || fingerprint != cursor_filter_fingerprint(filter) {
        return Err(CatalogError::InvalidCursor {
            detail: "the cursor was issued for another filter".to_owned(),
        });
    }
    if cursor_generation != generation {
        return Err(CatalogError::StaleCursor {
            generation: cursor_generation,
            active: generation,
        });
    }
    Ok(offset)
}

fn catalog_source_state(state: CollectionState) -> CatalogSourceState {
    match state {
        CollectionState::NeverCollected => CatalogSourceState::NeverCollected,
        CollectionState::Complete => CatalogSourceState::Complete,
        CollectionState::Incomplete => CatalogSourceState::Incomplete,
        CollectionState::Failed => CatalogSourceState::Failed,
    }
}

fn collection_state_name(state: CollectionState) -> &'static str {
    match state {
        CollectionState::NeverCollected => "never_collected",
        CollectionState::Complete => "complete",
        CollectionState::Incomplete => "incomplete",
        CollectionState::Failed => "failed",
    }
}

fn repository_state_name(state: contract::RepositoryState) -> &'static str {
    match state {
        contract::RepositoryState::Scanned => "scanned",
        contract::RepositoryState::NotReached => "not_reached",
        contract::RepositoryState::Failed => "failed",
    }
}

fn registry_state_name(state: contract::RegistryState) -> &'static str {
    match state {
        contract::RegistryState::Absent => "absent",
        contract::RegistryState::Ready => "ready",
        contract::RegistryState::Invalid => "invalid",
    }
}

fn conflict_kind_name(kind: contract::ConflictKind) -> &'static str {
    match kind {
        contract::ConflictKind::DivergentWorktree => "divergent_worktree",
        contract::ConflictKind::AmbiguousRename => "ambiguous_rename",
        contract::ConflictKind::DuplicateExplicitId => "duplicate_explicit_id",
        contract::ConflictKind::DuplicateEpic => "duplicate_epic",
        contract::ConflictKind::PinMismatch => "pin_mismatch",
        contract::ConflictKind::AssociationConflict => "association_conflict",
    }
}

fn native_runtime_state_name(state: native::NativeRuntimeState) -> &'static str {
    match state {
        native::NativeRuntimeState::Scanned => "scanned",
        native::NativeRuntimeState::Incomplete => "incomplete",
        native::NativeRuntimeState::Failed => "failed",
    }
}

fn retry_after(finished_at: Option<&str>, cadence_seconds: u64) -> Option<String> {
    let finished = finished_at?.parse::<Timestamp>().ok()?;
    let seconds = i64::try_from(cadence_seconds).ok()?;
    Some((finished + SignedDuration::from_secs(seconds)).to_string())
}

fn entry_matches_scope(entry: &CatalogEntry, scope: &str) -> bool {
    entry.logical_plan_id == scope
        || entry.occurrences.iter().any(|occurrence| {
            occurrence.repository_id == scope
                || occurrence.repository_root == scope
                || occurrence.relative_path == scope
                || occurrence.relative_path.starts_with(&format!("{scope}/"))
        })
        || entry.associations.iter().any(|association| {
            association.value == scope
                && matches!(
                    association.relation,
                    contract::AssociationRelation::Workstream | contract::AssociationRelation::Epic
                )
        })
}

fn error_class(error: &StoreError) -> &'static str {
    match error {
        StoreError::Sqlite(_) => "sqlite",
        StoreError::SchemaTooNew { .. } => "schema_too_new",
        StoreError::OwnerConflict { .. } => "owner_conflict",
        StoreError::NotOwner => "not_owner",
        StoreError::Fenced => "fenced",
        StoreError::MissingSweep => "missing_sweep",
        StoreError::Corrupt(_) => "corrupt",
        StoreError::Json(_) => "json",
        StoreError::Io(_) => "io",
    }
}

/// Poll the catalog until shutdown, sleeping one configured cadence between
/// sweeps.
///
/// Watches are deliberately absent. A sweep at the cadence is the completeness
/// proof; nothing here depends on an event stream being delivered.
pub async fn poll_until_stopped(
    catalog: Arc<Catalog>,
    mut stops: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let effective = catalog.effective_settings();
        let cadence = Duration::from_secs(effective.settings.cadence_seconds.clamp(1, 3600));
        match catalog.collect_once().await {
            Ok(report) => {
                if !report.finished {
                    warn!(
                        generation = report.generation,
                        files = report.files_seen,
                        "work catalog sweep was incomplete; prior facts were retained"
                    );
                }
            }
            Err(CatalogError::OwnerConflict { owner }) => {
                warn!(owner = %owner, "another collector owns the work catalog scope");
            }
            Err(error) => {
                warn!(detail = %error, "work catalog sweep failed");
            }
        }
        tokio::select! {
            () = tokio::time::sleep(cadence) => {}
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow() {
                    if let Err(error) = catalog.release_owner() {
                        warn!(detail = %error, "the catalog lease could not be released");
                    }
                    return;
                }
            }
        }
    }
}

/// Poll the native census until shutdown, sleeping one configured cadence
/// between attempts.
///
/// A failed or incomplete census is retried at the same cadence. Nothing here
/// retries inside one attempt: a half-walk that is retried immediately would
/// multiply the load on a runtime that is already refusing.
pub async fn poll_native_until_stopped(
    catalog: Arc<Catalog>,
    sources: Vec<Arc<dyn native::NativeCensusSource>>,
    mut stops: tokio::sync::watch::Receiver<bool>,
) {
    if sources.is_empty() {
        return;
    }
    loop {
        let effective = catalog.effective_settings();
        let cadence = Duration::from_secs(effective.settings.cadence_seconds.clamp(1, 3600));
        match catalog.collect_native_once(&sources).await {
            Ok(report) => {
                if !report.complete {
                    warn!(
                        generation = report.generation,
                        projects = report.projects,
                        workspaces = report.workspaces,
                        sessions = report.sessions,
                        "native census was incomplete; prior observations were retained"
                    );
                }
            }
            Err(CatalogError::OwnerConflict { owner }) => {
                warn!(owner = %owner, "another collector owns the work catalog scope");
            }
            Err(error) => {
                warn!(detail = %error, "native census failed");
            }
        }
        tokio::select! {
            () = tokio::time::sleep(cadence) => {}
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow() {
                    if let Err(error) = catalog.release_owner() {
                        warn!(detail = %error, "the catalog lease could not be released");
                    }
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_matches_a_workstream_without_touching_ids() {
        let entry = CatalogEntry {
            logical_plan_id: "plan-1".to_owned(),
            kind: EntryKind::Plan,
            title: "A plan".to_owned(),
            state: contract::EntryState::Active,
            aliases: Vec::new(),
            associations: vec![contract::Association {
                relation: contract::AssociationRelation::Workstream,
                value: "kontor".to_owned(),
                kind: contract::AssociationKind::PlanDeclaration,
                source_id: None,
                verified: false,
                dangling: false,
                note: None,
            }],
            occurrences: Vec::new(),
            diagnostics: Vec::new(),
            conflicts: Vec::new(),
            first_seen: "2026-10-25T00:00:00Z".to_owned(),
            last_seen: "2026-10-25T00:00:00Z".to_owned(),
        };
        assert!(entry_matches_scope(&entry, "kontor"));
        assert!(!entry_matches_scope(&entry, "mobile-app"));
    }

    #[test]
    fn a_zero_limit_uses_the_bounded_default() {
        let (entries, truncated, cursor) =
            filter_entries(Vec::new(), &EntryFilter::default(), 0).expect("an empty page");
        assert!(entries.is_empty());
        assert!(!truncated);
        assert!(cursor.is_none());
    }

    #[test]
    fn a_cursor_is_bound_to_its_generation_and_filter() {
        let filter = EntryFilter {
            kind: Some(EntryKind::Plan),
            limit: 1,
            ..EntryFilter::default()
        };
        let cursor = encode_cursor(7, 1, &filter);
        assert_eq!(
            parse_cursor(&cursor, &filter, 7).expect("the same generation"),
            1
        );
        assert!(matches!(
            parse_cursor(&cursor, &filter, 8),
            Err(CatalogError::StaleCursor { .. })
        ));
        let other = EntryFilter {
            kind: Some(EntryKind::ExecutionUnit),
            ..filter
        };
        assert!(matches!(
            parse_cursor(&cursor, &other, 7),
            Err(CatalogError::InvalidCursor { .. })
        ));
    }
}
