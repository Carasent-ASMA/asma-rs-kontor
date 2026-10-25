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
pub mod store;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kontor_core::id::Timestamp;
use tracing::{info, warn};

pub use config::{
    CatalogSettings, Environment, JiraSettings, OperatorSource, ProcessEnvironment,
    RepositoryConfig, SourceKind, SourceRevisionKind,
};
pub use contract::{
    CATALOG_CONTRACT_VERSION, CatalogEntry, CatalogStatus, CollectionState, ConfigOrigin,
    ContentKind, EntriesView, EntryFilter, EntryKind, EntryState, ReadSource, RevisionKind,
};
pub use jira::{JiraCatalog, JiraCatalogError, JiraSweepReport, poll_jira_until_stopped};
pub use jira_contract::{
    EPIC_IDENTITY_SCHEMA, EpicIdentitySource, JiraAlias, JiraCollectionCoverage,
    JiraCollectionState, JiraEvidenceDocument, JiraIssueObservation, JiraJoinDiagnostic,
    JiraObservationState, JiraObservationVia, JiraProjectionError, JiraScope, JiraSweepMode,
    JiraWorkstreamJoin, epic_identity_source, verified_workstream_join,
};
pub use kontor_jira::JiraReadFailure;
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
    /// # Errors
    /// Neither the database nor a compatible snapshot could answer.
    pub fn entries(&self, filter: &EntryFilter) -> Result<EntriesView, CatalogError> {
        match self.database.read_entries(filter.include_removed) {
            Ok(read) => {
                let (entries, truncated) = filter_entries(read.entries, filter);
                Ok(EntriesView {
                    schema_version: CATALOG_CONTRACT_VERSION,
                    generation: read.generation,
                    as_of: read.as_of,
                    source: ReadSource::Database,
                    entries,
                    truncated,
                })
            }
            Err(database_error) => match self.read_snapshot() {
                Ok(Some(snapshot)) => {
                    let (entries, truncated) = filter_entries(snapshot.entries, filter);
                    Ok(EntriesView {
                        schema_version: CATALOG_CONTRACT_VERSION,
                        generation: snapshot.generation,
                        as_of: Some(snapshot.written_at),
                        source: ReadSource::Snapshot,
                        entries,
                        truncated,
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

fn filter_entries(entries: Vec<CatalogEntry>, filter: &EntryFilter) -> (Vec<CatalogEntry>, bool) {
    let limit = if filter.limit == 0 {
        1000
    } else {
        filter.limit.min(1000)
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
    let truncated = matched.len() > limit;
    matched.truncate(limit);
    (matched, truncated)
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
        let (entries, truncated) = filter_entries(Vec::new(), &EntryFilter::default());
        assert!(entries.is_empty());
        assert!(!truncated);
    }
}
