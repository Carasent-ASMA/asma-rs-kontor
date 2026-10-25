//! The catalog's own SQLite database.
//!
//! It shares no table, connection or transaction with `kontor.db`. That is the
//! point of the crate: a catalog outage degrades discovery and nothing else.
//!
//! # The page/promote split
//!
//! A scan commits pages. A page is durable evidence a sweep saw something; it
//! is not the catalog's answer. Only the final transaction of a sweep that read
//! every configured repository promotes pages into the canonical tables,
//! advances the active generation and — only there — tombstones what the
//! complete sweep proved absent.
//!
//! That split is what makes the crash cases honest:
//!
//! * crash before a page commit: the page never happened;
//! * crash after a page but before the final commit: its observations are
//!   staged, the last complete generation is untouched;
//! * crash after the final commit: the generation and its checkpoint are one
//!   committed fact.
//!
//! An incomplete sweep may still insert a *new* observation, but only hidden
//! (`visible_generation` is NULL), so a reader never sees a generation that
//! did not finish. It never updates or removes an existing row.

use std::path::Path;
use std::sync::Mutex;

use kontor_core::id::{SignedDuration, Timestamp};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::contract::{
    Alias, AliasKind, Association, AssociationKind, AssociationRelation, CatalogCounts,
    CatalogEntry, CollectionState, CollectionStatus, ConfigOrigin, Conflict, ConflictKind,
    ContentKind, ContentRevision, DanglingAssociation, Diagnostic, DiagnosticSeverity, EntryKind,
    EntryState, Occurrence, RegistryState, RegistryStatus, RepositoryCoverage, RepositoryState,
    RevisionKind,
};

/// The schema generation this build implements.
pub const CATALOG_SCHEMA_VERSION: i64 = 2;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_catalog.sql"),
    include_str!("../migrations/0002_jira_evidence.sql"),
];

const BUSY_TIMEOUT_MS: u64 = 30_000;

/// One observation staged by a scan, before promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedEntry {
    /// The generated or explicit logical plan id.
    pub logical_plan_id: String,
    /// A validated explicit id, when the plan declared one.
    pub explicit_id: Option<String>,
    /// Whether the id came from an explicit operator source and must not be
    /// re-identified by rename detection.
    #[serde(default)]
    pub preserve_id: bool,
    /// A plan or a QNR execution unit.
    pub kind: EntryKind,
    /// The display title.
    pub title: String,
    /// Every occurrence seen in this sweep.
    pub occurrences: Vec<ObservedOccurrence>,
    /// Explicit and declared associations.
    pub associations: Vec<ObservedAssociation>,
    /// Structural diagnostics for this entry.
    pub diagnostics: Vec<Diagnostic>,
    /// Visible conflicts for this entry.
    pub conflicts: Vec<Conflict>,
    /// A rename alias to persist when a predecessor identity was retained.
    #[serde(default)]
    pub rename_alias: Option<Alias>,
}

/// One occurrence staged by a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedOccurrence {
    /// The occurrence's stable id.
    pub occurrence_id: String,
    /// The repository identity.
    pub repository_id: String,
    /// The worktree root.
    pub repository_root: String,
    /// The path relative to the repository root.
    pub relative_path: String,
    /// How it was found.
    pub content_kind: ContentKind,
    /// The operator's declared revision intent.
    pub revision_kind: RevisionKind,
    /// The exact content revision.
    pub content: ContentRevision,
    /// Explicit source ids that name this occurrence.
    pub source_ids: Vec<String>,
    /// The declared pin, when one exists.
    pub expected_sha256: Option<String>,
}

/// One association staged by a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedAssociation {
    /// The relation.
    pub relation: AssociationRelation,
    /// The relation's value.
    pub value: String,
    /// Who stated it.
    pub kind: AssociationKind,
    /// The operator source id, when explicit.
    pub source_id: Option<String>,
    /// A structural note.
    pub note: Option<String>,
}

/// The final facts a collector commits for one sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Whether every configured repository was read.
    pub complete: bool,
    /// Per-repository coverage.
    pub coverage: Vec<RepositoryCoverage>,
    /// Attempt-level structural diagnostics.
    pub diagnostics: Vec<String>,
    /// The registry projection from this sweep.
    pub registry: RegistryStatus,
    /// Files considered.
    pub files_seen: usize,
    /// Documents observed.
    pub entries_seen: usize,
    /// A structural failure, when the attempt could not finish.
    pub error: Option<String>,
    /// Repository ids whose complete contents were read in this sweep.
    ///
    /// Only these can prove absence. A failed or unreached root keeps every
    /// fact it last supplied.
    pub scanned_repository_ids: Vec<String>,
}

/// One in-flight sweep, which is also its fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepId {
    /// The single host scope.
    pub scope: String,
    /// The monotonic attempt generation.
    pub generation: u64,
    /// The owner that may write pages for it.
    pub owner_id: String,
}

/// The current owner lease.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerStatus {
    /// The lease holder.
    pub owner: Option<String>,
    /// When the lease expires.
    pub lease_until: Option<String>,
}

/// Everything a status projection needs from storage.
#[derive(Debug, Clone)]
pub struct StoreStatus {
    /// The host-local catalog id.
    pub catalog_id: String,
    /// The active complete generation.
    pub generation: u64,
    /// The latest attempted generation.
    pub attempted_generation: u64,
    /// The active generation's completion instant.
    pub as_of: Option<String>,
    /// The current owner lease.
    pub owner: OwnerStatus,
    /// The latest collection projection.
    pub collection: CollectionStatus,
    /// Current counts.
    pub counts: CatalogCounts,
    /// Current registry projection.
    pub registry: RegistryStatus,
    /// Current diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Current conflicts.
    pub conflicts: Vec<Conflict>,
    /// Retained dangling explicit associations.
    pub dangling: Vec<DanglingAssociation>,
}

/// A database read result for entries.
#[derive(Debug, Clone)]
pub struct StoreEntries {
    /// The active generation.
    pub generation: u64,
    /// Its completion instant.
    pub as_of: Option<String>,
    /// Every entry at that generation, before caller filtering.
    pub entries: Vec<CatalogEntry>,
}

/// Everything the catalog store can refuse.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// SQLite refused.
    #[error("catalog sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database was written by a newer catalog.
    #[error("catalog schema version {found} is newer than this binary understands ({expected})")]
    SchemaTooNew {
        /// The version found.
        found: i64,
        /// The version this build implements.
        expected: i64,
    },
    /// A live owner already holds the scope.
    #[error("catalog scope is already owned by `{owner}`")]
    OwnerConflict {
        /// The current owner id.
        owner: String,
    },
    /// This process does not hold the scope lease.
    #[error("this process does not hold the catalog scope lease")]
    NotOwner,
    /// A newer sweep or owner displaced the writer.
    #[error("the sweep or owner was displaced; the observation was fenced")]
    Fenced,
    /// The sweep row does not exist.
    #[error("no such catalog sweep exists")]
    MissingSweep,
    /// Staged evidence could not be decoded.
    #[error("staged catalog evidence is corrupt: {0}")]
    Corrupt(String),
    /// JSON encoding or decoding failed.
    #[error("catalog JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The settings document could not be materialized.
    #[error("catalog settings could not be materialized: {0}")]
    Io(#[from] std::io::Error),
}

/// The catalog database.
#[derive(Debug)]
pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    /// Open, configure and migrate one catalog database.
    ///
    /// # Errors
    /// Any SQLite refusal, a too-new schema, or an unusable directory.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))?;
        let journal: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(StoreError::Corrupt(
                "catalog journal mode did not become wal".to_owned(),
            ));
        }
        connection.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut connection)?;
        let now = Timestamp::now().to_string();
        connection.execute(
            "INSERT OR IGNORE INTO catalog_scope
                 (scope, catalog_id, owner_id, lease_until, generation,
                  active_generation, registry_json, updated_at)
             VALUES (?1, ?2, NULL, NULL, 0, 0, NULL, ?3)",
            params![
                crate::contract::HOST_SCOPE,
                uuid::Uuid::now_v7().to_string(),
                now
            ],
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// The schema version read from the file.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        let connection = self.lock();
        Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    /// Claim or renew the one collector lease.
    ///
    /// # Errors
    /// [`StoreError::OwnerConflict`] while another owner's lease is live.
    pub fn acquire_owner(
        &self,
        owner_id: &str,
        lease_seconds: u64,
        now: &Timestamp,
    ) -> Result<OwnerStatus, StoreError> {
        let now_text = now.to_string();
        let lease_until = (*now + SignedDuration::from_secs(lease_seconds as i64)).to_string();
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let held: Option<(Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT owner_id, lease_until FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((current_owner, current_until)) = held else {
            return Err(StoreError::Corrupt(
                "the catalog scope row is missing".to_owned(),
            ));
        };
        if let Some(current_owner) = &current_owner
            && current_owner != owner_id
            && current_until
                .as_deref()
                .is_some_and(|until| until > now_text.as_str())
        {
            return Err(StoreError::OwnerConflict {
                owner: current_owner.clone(),
            });
        }
        transaction.execute(
            "UPDATE catalog_scope SET owner_id = ?1, lease_until = ?2, updated_at = ?3
             WHERE scope = ?4",
            params![owner_id, lease_until, now_text, crate::contract::HOST_SCOPE],
        )?;
        transaction.commit()?;
        Ok(OwnerStatus {
            owner: Some(owner_id.to_owned()),
            lease_until: Some(lease_until),
        })
    }

    /// Release this owner's lease, when it still holds it.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn release_owner(&self, owner_id: &str, now: &Timestamp) -> Result<(), StoreError> {
        let connection = self.lock();
        connection.execute(
            "UPDATE catalog_scope SET owner_id = NULL, lease_until = NULL, updated_at = ?1
             WHERE scope = ?2 AND owner_id = ?3",
            params![now.to_string(), crate::contract::HOST_SCOPE, owner_id],
        )?;
        Ok(())
    }

    /// The current lease.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn owner_status(&self) -> Result<OwnerStatus, StoreError> {
        let connection = self.lock();
        let row: Option<(Option<String>, Option<String>)> = connection
            .query_row(
                "SELECT owner_id, lease_until FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(row
            .map(|(owner, lease_until)| OwnerStatus { owner, lease_until })
            .unwrap_or_default())
    }

    /// Begin one sweep, allocating the next generation.
    ///
    /// # Errors
    /// [`StoreError::NotOwner`] when this process no longer holds the lease.
    pub fn begin_sweep(&self, owner_id: &str, now: &Timestamp) -> Result<SweepId, StoreError> {
        let now_text = now.to_string();
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current_owner, lease_until, generation): (Option<String>, Option<String>, i64) =
            transaction.query_row(
                "SELECT owner_id, lease_until, generation FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        if current_owner.as_deref() != Some(owner_id)
            || !lease_until.is_some_and(|until| until > now_text)
        {
            return Err(StoreError::NotOwner);
        }
        let generation = u64::try_from(generation)
            .map_err(|_| StoreError::Corrupt("negative generation".to_owned()))?
            + 1;
        transaction.execute(
            "INSERT INTO catalog_sweeps
                 (scope, generation, owner_id, state, started_at, pages_committed,
                  files_seen, entries_seen, coverage_json, diagnostics_json)
             VALUES (?1, ?2, ?3, 'running', ?4, 0, 0, 0, '[]', '[]')",
            params![
                crate::contract::HOST_SCOPE,
                i64::try_from(generation).unwrap_or(i64::MAX),
                owner_id,
                now_text
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_scope SET generation = ?1, updated_at = ?2 WHERE scope = ?3",
            params![
                i64::try_from(generation).unwrap_or(i64::MAX),
                now_text,
                crate::contract::HOST_SCOPE
            ],
        )?;
        transaction.commit()?;
        Ok(SweepId {
            scope: crate::contract::HOST_SCOPE.to_owned(),
            generation,
            owner_id: owner_id.to_owned(),
        })
    }

    /// Commit one page of observations plus its page checkpoint.
    ///
    /// # Errors
    /// [`StoreError::Fenced`] when a newer sweep or owner displaced this one.
    pub fn commit_page(
        &self,
        sweep: &SweepId,
        page_index: u32,
        observed_at: &Timestamp,
        entries: &[ObservedEntry],
        files_seen: usize,
    ) -> Result<(), StoreError> {
        let entries_json = serde_json::to_string(entries)?;
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<(String, String)> = transaction
            .query_row(
                "SELECT state, owner_id FROM catalog_sweeps
                 WHERE scope = ?1 AND generation = ?2",
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match state {
            Some((state, owner)) if state == "running" && owner == sweep.owner_id => {}
            Some(_) => return Err(StoreError::Fenced),
            None => return Err(StoreError::MissingSweep),
        }
        transaction.execute(
            "INSERT INTO catalog_sweep_pages
                 (scope, generation, page_index, observed_at, entries_json,
                  files_seen, entries_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (scope, generation, page_index) DO UPDATE SET
                 observed_at = excluded.observed_at,
                 entries_json = excluded.entries_json,
                 files_seen = excluded.files_seen,
                 entries_seen = excluded.entries_seen",
            params![
                sweep.scope,
                generation_to_sql(sweep.generation),
                i64::from(page_index),
                observed_at.to_string(),
                entries_json,
                i64::try_from(files_seen).unwrap_or(i64::MAX),
                i64::try_from(entries.len()).unwrap_or(i64::MAX)
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_sweeps
             SET pages_committed = (SELECT COUNT(*) FROM catalog_sweep_pages
                                    WHERE scope = ?1 AND generation = ?2),
                 files_seen = (SELECT COALESCE(SUM(files_seen), 0) FROM catalog_sweep_pages
                               WHERE scope = ?1 AND generation = ?2),
                 entries_seen = (SELECT COALESCE(SUM(entries_seen), 0) FROM catalog_sweep_pages
                                 WHERE scope = ?1 AND generation = ?2)
             WHERE scope = ?1 AND generation = ?2",
            params![sweep.scope, generation_to_sql(sweep.generation)],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Finish one sweep and commit its checkpoint.
    ///
    /// A complete sweep promotes every staged page, advances the active
    /// generation and tombstones what the complete sweep proved absent. An
    /// incomplete sweep only inserts previously unknown rows, hidden from
    /// readers, and never updates or removes an existing fact.
    ///
    /// # Errors
    /// [`StoreError::Fenced`], [`StoreError::MissingSweep`] or a SQL/JSON
    /// refusal.
    pub fn finalize_sweep(
        &self,
        sweep: &SweepId,
        outcome: &SweepOutcome,
        now: &Timestamp,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<(String, String)> = transaction
            .query_row(
                "SELECT state, owner_id FROM catalog_sweeps
                 WHERE scope = ?1 AND generation = ?2",
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match state {
            Some((state, owner)) if state == "running" && owner == sweep.owner_id => {}
            Some(_) => return Err(StoreError::Fenced),
            None => return Err(StoreError::MissingSweep),
        }

        let mut staged = Vec::new();
        {
            let mut statement = transaction.prepare(
                "SELECT entries_json FROM catalog_sweep_pages
                 WHERE scope = ?1 AND generation = ?2 ORDER BY page_index",
            )?;
            let rows = statement.query_map(
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| row.get::<_, String>(0),
            )?;
            for row in rows {
                let json = row?;
                let entries: Vec<ObservedEntry> = serde_json::from_str(&json)
                    .map_err(|error| StoreError::Corrupt(error.to_string()))?;
                staged.extend(entries);
            }
        }

        let complete = outcome.complete;
        if complete {
            promote_complete(
                &transaction,
                sweep.generation,
                now,
                &mut staged,
                &outcome.scanned_repository_ids,
            )?;
        } else {
            promote_incomplete(&transaction, sweep.generation, now, &staged)?;
        }

        let coverage_json = serde_json::to_string(&outcome.coverage)?;
        let diagnostics_json = serde_json::to_string(&outcome.diagnostics)?;
        transaction.execute(
            "UPDATE catalog_sweeps
             SET state = ?1, finished_at = ?2, error = ?3, coverage_json = ?4,
                 diagnostics_json = ?5,
                 files_seen = MAX(files_seen, ?6), entries_seen = MAX(entries_seen, ?7)
             WHERE scope = ?8 AND generation = ?9",
            params![
                if complete { "complete" } else { "incomplete" },
                now.to_string(),
                outcome.error,
                coverage_json,
                diagnostics_json,
                i64::try_from(outcome.files_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.entries_seen).unwrap_or(i64::MAX),
                sweep.scope,
                generation_to_sql(sweep.generation)
            ],
        )?;
        if complete {
            transaction.execute(
                "UPDATE catalog_scope
                 SET active_generation = ?1, registry_json = ?2, updated_at = ?3
                 WHERE scope = ?4",
                params![
                    generation_to_sql(sweep.generation),
                    serde_json::to_string(&outcome.registry)?,
                    now.to_string(),
                    sweep.scope
                ],
            )?;
        }
        let previous: Option<(i64, String)> = transaction
            .query_row(
                "SELECT generation, started_at FROM catalog_sweeps
                 WHERE scope = ?1 AND generation = ?2",
                params![sweep.scope, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let started = previous.map(|(_, started)| started);
        let duration_ms = started
            .as_deref()
            .and_then(|started| started.parse::<Timestamp>().ok())
            .and_then(|started| {
                u64::try_from(now.as_millisecond() - started.as_millisecond()).ok()
            });
        let healthy = outcome
            .coverage
            .iter()
            .all(|coverage| coverage.state == RepositoryState::Scanned);
        let state_text = if complete && healthy {
            "complete"
        } else {
            "incomplete"
        };
        transaction.execute(
            "INSERT INTO catalog_collection
                 (scope, generation, attempt_generation, state, started_at, finished_at,
                  duration_ms, repositories_configured, repositories_scanned,
                  files_seen, entries_observed, error, coverage_json, registry_json)
             VALUES (?1,
                     CASE WHEN ?2 = 1 THEN ?3 ELSE COALESCE(
                         (SELECT generation FROM catalog_collection WHERE scope = ?1), 0) END,
                     ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (scope) DO UPDATE SET
                 generation = CASE WHEN ?2 = 1 THEN ?3 ELSE catalog_collection.generation END,
                 attempt_generation = ?3,
                 state = ?4,
                 started_at = ?5,
                 finished_at = ?6,
                 duration_ms = ?7,
                 repositories_configured = ?8,
                 repositories_scanned = ?9,
                 files_seen = ?10,
                 entries_observed = ?11,
                 error = ?12,
                 coverage_json = ?13,
                 registry_json = CASE WHEN ?2 = 1 THEN ?14
                                      ELSE catalog_collection.registry_json END",
            params![
                sweep.scope,
                if complete { 1 } else { 0 },
                generation_to_sql(sweep.generation),
                state_text,
                started,
                now.to_string(),
                duration_ms.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                i64::try_from(outcome.coverage.len()).unwrap_or(i64::MAX),
                i64::try_from(
                    outcome
                        .coverage
                        .iter()
                        .filter(|coverage| coverage.state == RepositoryState::Scanned)
                        .count()
                )
                .unwrap_or(i64::MAX),
                i64::try_from(outcome.files_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.entries_seen).unwrap_or(i64::MAX),
                outcome.error,
                coverage_json,
                serde_json::to_string(&outcome.registry)?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Record a sweep that could not finish and commit no observations.
    ///
    /// # Errors
    /// [`StoreError::Fenced`], [`StoreError::MissingSweep`] or a SQL refusal.
    pub fn fail_sweep(
        &self,
        sweep: &SweepId,
        error: &str,
        diagnostics: &[String],
        now: &Timestamp,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE catalog_sweeps
             SET state = 'failed', finished_at = ?1, error = ?2, diagnostics_json = ?3
             WHERE scope = ?4 AND generation = ?5 AND owner_id = ?6 AND state = 'running'",
            params![
                now.to_string(),
                bounded(error),
                serde_json::to_string(diagnostics)?,
                sweep.scope,
                generation_to_sql(sweep.generation),
                sweep.owner_id
            ],
        )?;
        if updated == 0 {
            return Err(StoreError::Fenced);
        }
        transaction.execute(
            "INSERT INTO catalog_collection
                 (scope, generation, attempt_generation, state, started_at, finished_at,
                  duration_ms, repositories_configured, repositories_scanned,
                  files_seen, entries_observed, error, coverage_json, registry_json)
             VALUES (?1, 0, ?2, 'failed', ?3, ?3, 0, 0, 0, 0, 0, ?4, '[]', NULL)
             ON CONFLICT (scope) DO UPDATE SET
                 attempt_generation = ?2,
                 state = 'failed',
                 finished_at = ?3,
                 error = ?4",
            params![
                sweep.scope,
                generation_to_sql(sweep.generation),
                now.to_string(),
                bounded(error)
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Read the current status projection.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn read_status(&self) -> Result<StoreStatus, StoreError> {
        let connection = self.lock();
        let (catalog_id, generation, active, registry_json): (String, i64, i64, Option<String>) =
            connection.query_row(
                "SELECT catalog_id, generation, active_generation, registry_json
                 FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        let generation = u64::try_from(generation)
            .map_err(|_| StoreError::Corrupt("negative generation".to_owned()))?;
        let active = u64::try_from(active)
            .map_err(|_| StoreError::Corrupt("negative active generation".to_owned()))?;
        let as_of: Option<String> = if active == 0 {
            None
        } else {
            connection
                .query_row(
                    "SELECT finished_at FROM catalog_sweeps
                     WHERE scope = ?1 AND generation = ?2 AND state = 'complete'",
                    params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
                    |row| row.get(0),
                )
                .optional()?
                .flatten()
        };
        let owner: Option<(Option<String>, Option<String>)> = connection
            .query_row(
                "SELECT owner_id, lease_until FROM catalog_scope WHERE scope = ?1",
                [crate::contract::HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let owner = owner
            .map(|(owner, lease_until)| OwnerStatus { owner, lease_until })
            .unwrap_or_default();
        let collection = read_collection(&connection, active)?;
        let counts = read_counts(&connection, active)?;
        let registry = registry_json
            .and_then(|json| serde_json::from_str::<RegistryStatus>(&json).ok())
            .unwrap_or_else(|| RegistryStatus {
                path: None,
                revision: None,
                state: RegistryState::Absent,
                workstreams: 0,
                diagnostics: Vec::new(),
            });
        let diagnostics = read_diagnostics(&connection, active)?;
        let conflicts = read_conflicts(&connection, active)?;
        let dangling = read_dangling(&connection, active)?;
        Ok(StoreStatus {
            catalog_id,
            generation: active,
            attempted_generation: generation,
            as_of,
            owner,
            collection,
            counts,
            registry,
            diagnostics,
            conflicts,
            dangling,
        })
    }

    /// Read every entry at the active generation in one read transaction.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn read_entries(&self, include_removed: bool) -> Result<StoreEntries, StoreError> {
        let connection = self.lock();
        let active: i64 = connection.query_row(
            "SELECT active_generation FROM catalog_scope WHERE scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| row.get(0),
        )?;
        let active = u64::try_from(active)
            .map_err(|_| StoreError::Corrupt("negative active generation".to_owned()))?;
        if active == 0 {
            return Ok(StoreEntries {
                generation: 0,
                as_of: None,
                entries: Vec::new(),
            });
        }
        let as_of: Option<String> = connection
            .query_row(
                "SELECT finished_at FROM catalog_sweeps
                 WHERE scope = ?1 AND generation = ?2 AND state = 'complete'",
                params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let entries = read_entries_at(&connection, active, include_removed)?;
        Ok(StoreEntries {
            generation: active,
            as_of,
            entries,
        })
    }

    /// Record which effective configuration a sweep used.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn record_config(
        &self,
        origin: ConfigOrigin,
        revision: Option<&str>,
        imported_at: &str,
        settings_json: &str,
    ) -> Result<(), StoreError> {
        let origin = match origin {
            ConfigOrigin::CatalogFile => "catalog_file",
            ConfigOrigin::MigratedPluginEnv => "migrated_plugin_env",
            ConfigOrigin::MigratedPluginFile => "migrated_plugin_file",
            ConfigOrigin::None => "none",
        };
        let connection = self.lock();
        connection.execute(
            "INSERT INTO catalog_config
                 (singleton, origin, revision, imported_at, settings_json)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT (singleton) DO UPDATE SET
                 origin = excluded.origin,
                 revision = excluded.revision,
                 imported_at = excluded.imported_at,
                 settings_json = excluded.settings_json",
            params![origin, revision, imported_at, settings_json],
        )?;
        Ok(())
    }

    /// Write the projection used by the fallback snapshot.
    ///
    /// # Errors
    /// SQLite or JSON refused.
    pub fn snapshot_projection(
        &self,
    ) -> Result<(u64, Option<String>, Vec<CatalogEntry>), StoreError> {
        let entries = self.read_entries(true)?;
        Ok((entries.generation, entries.as_of, entries.entries))
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn rename_alias(value: &str, observed_at: &str) -> Alias {
    Alias {
        kind: AliasKind::RenamedFrom,
        value: value.to_owned(),
        observed_at: observed_at.to_owned(),
    }
}

pub(crate) fn generation_to_sql(generation: u64) -> i64 {
    i64::try_from(generation).unwrap_or(i64::MAX)
}

pub(crate) fn bounded(text: &str) -> String {
    text.chars().take(4096).collect()
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > CATALOG_SCHEMA_VERSION {
        return Err(StoreError::SchemaTooNew {
            found: version,
            expected: CATALOG_SCHEMA_VERSION,
        });
    }
    if version == CATALOG_SCHEMA_VERSION {
        return Ok(());
    }
    let pending = usize::try_from(version)
        .map_err(|_| StoreError::Corrupt("negative schema version".to_owned()))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for migration in &MIGRATIONS[pending..] {
        transaction.execute_batch(migration)?;
    }
    let applied: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if applied != CATALOG_SCHEMA_VERSION {
        return Err(StoreError::Corrupt(
            "the catalog migration chain did not record its version".to_owned(),
        ));
    }
    transaction.commit()?;
    Ok(())
}

/// One previous plan used only to decide whether a new path is its rename.
struct PreviousPlan {
    logical_plan_id: String,
    kind: EntryKind,
    epic: Option<String>,
    occurrences: Vec<PreviousOccurrence>,
}

struct PreviousOccurrence {
    repository_id: String,
    revision_id: String,
    relative_path: String,
}

fn promote_complete(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    staged: &mut [ObservedEntry],
    scanned_repository_ids: &[String],
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    let active: i64 = transaction.query_row(
        "SELECT active_generation FROM catalog_scope WHERE scope = ?1",
        [crate::contract::HOST_SCOPE],
        |row| row.get(0),
    )?;
    let previous = load_previous(transaction, active)?;
    let previous_ids: std::collections::BTreeSet<String> = previous
        .iter()
        .map(|plan| plan.logical_plan_id.clone())
        .collect();
    let staged_ids: std::collections::BTreeSet<String> = staged
        .iter()
        .map(|entry| entry.logical_plan_id.clone())
        .collect();

    // A rename is only claimed when exactly one absent predecessor in the same
    // repository carries the same content revision and the same declared epic.
    // Two candidates are a visible conflict, never a coin toss.
    let mut remaps = std::collections::BTreeMap::<String, (String, String)>::new();
    let mut claimed = std::collections::BTreeSet::<String>::new();
    for entry in staged.iter_mut() {
        if entry.preserve_id || previous_ids.contains(&entry.logical_plan_id) {
            continue;
        }
        let mut candidates: Vec<(&PreviousPlan, &str)> = Vec::new();
        for candidate in &previous {
            if staged_ids.contains(&candidate.logical_plan_id)
                || candidate.kind != entry.kind
                || candidate.epic.is_none()
            {
                continue;
            }
            for occurrence in &entry.occurrences {
                let Some(epic) = occurrence.content.epic.as_deref() else {
                    continue;
                };
                if candidate.epic.as_deref() != Some(epic) {
                    continue;
                }
                if let Some(previous_occurrence) = candidate.occurrences.iter().find(|previous| {
                    previous.repository_id == occurrence.repository_id
                        && previous.revision_id == occurrence.content.revision_id
                }) {
                    candidates.push((candidate, previous_occurrence.relative_path.as_str()));
                    break;
                }
            }
        }
        match candidates.as_slice() {
            [(candidate, old_path)] if !claimed.contains(&candidate.logical_plan_id) => {
                claimed.insert(candidate.logical_plan_id.clone());
                remaps.insert(
                    entry.logical_plan_id.clone(),
                    (candidate.logical_plan_id.clone(), (*old_path).to_owned()),
                );
            }
            [] => {}
            _ => entry.conflicts.push(Conflict {
                kind: ConflictKind::AmbiguousRename,
                logical_plan_id: Some(entry.logical_plan_id.clone()),
                message: "More than one absent plan matched this plan's repository, content revision and epic; no rename was claimed."
                    .to_owned(),
                observed_at: observed_at.clone(),
            }),
        }
    }
    for entry in staged.iter_mut() {
        if let Some((old_id, old_path)) = remaps.get(&entry.logical_plan_id).cloned() {
            entry.logical_plan_id = old_id;
            entry.diagnostics.push(Diagnostic {
                code: "rename_claimed".to_owned(),
                severity: DiagnosticSeverity::Info,
                message: "One absent predecessor matched this repository, content revision and epic; its identity was retained.".to_owned(),
                subject: Some(old_path.clone()),
                logical_plan_id: Some(entry.logical_plan_id.clone()),
                observed_at: observed_at.clone(),
            });
            entry.rename_alias = Some(rename_alias(&old_path, &observed_at));
        }
    }

    // Explicit ids must be unique. Two files declaring one id remain visible
    // and produce a conflict; neither silently wins.
    let mut explicit_ids = std::collections::BTreeMap::<String, usize>::new();
    for entry in staged.iter() {
        if let Some(id) = &entry.explicit_id {
            *explicit_ids.entry(id.clone()).or_insert(0) += 1;
        }
    }
    for entry in staged.iter_mut() {
        if let Some(id) = &entry.explicit_id
            && explicit_ids.get(id).copied().unwrap_or(0) > 1
        {
            entry.conflicts.push(Conflict {
                kind: ConflictKind::DuplicateExplicitId,
                logical_plan_id: Some(entry.logical_plan_id.clone()),
                message: format!("explicit plan id `{id}` is declared more than once"),
                observed_at: observed_at.clone(),
            });
        }
    }

    let mut aliases_by_plan: std::collections::BTreeMap<String, Vec<Alias>> =
        std::collections::BTreeMap::new();
    for entry in staged.iter() {
        promote_entry(transaction, generation, now, entry)?;
        if let Some(alias) = &entry.rename_alias {
            aliases_by_plan
                .entry(entry.logical_plan_id.clone())
                .or_default()
                .push(alias.clone());
        }
    }
    for (logical_plan_id, aliases) in &aliases_by_plan {
        for alias in aliases {
            transaction.execute(
                "INSERT OR IGNORE INTO catalog_aliases
                     (logical_plan_id, alias_kind, alias_value, observed_at)
                 VALUES (?1, 'renamed_from', ?2, ?3)",
                params![logical_plan_id, alias.value, alias.observed_at],
            )?;
        }
    }

    // Tombstone only after every staged observation is written, and only
    // because this sweep read every configured repository.
    for repository_id in scanned_repository_ids {
        transaction.execute(
            "UPDATE catalog_occurrences
             SET state = 'removed', removed_at = ?1, visible_generation = ?2
             WHERE repository_id = ?3 AND state = 'active'
               AND last_seen_generation <> ?2",
            params![observed_at, generation_to_sql(generation), repository_id],
        )?;
    }
    transaction.execute(
        "UPDATE catalog_logical_plans
         SET state = 'removed', removed_at = ?1, visible_generation = ?2
         WHERE state = 'active'
           AND NOT EXISTS (
               SELECT 1 FROM catalog_occurrences o
               WHERE o.logical_plan_id = catalog_logical_plans.logical_plan_id
                 AND o.state = 'active'
           )",
        params![observed_at, generation_to_sql(generation)],
    )?;
    transaction.execute(
        "UPDATE catalog_associations
         SET dangling = 1
         WHERE association_kind = 'explicit_operator'
           AND logical_plan_id IS NOT NULL
           AND logical_plan_id IN (
               SELECT logical_plan_id FROM catalog_logical_plans WHERE state = 'removed'
           )",
        [],
    )?;
    transaction.execute(
        "UPDATE catalog_associations
         SET dangling = 0
         WHERE association_kind = 'explicit_operator'
           AND logical_plan_id IN (
               SELECT logical_plan_id FROM catalog_logical_plans WHERE state = 'active'
           )",
        [],
    )?;

    // Current-generation diagnostics and conflicts. Older generations remain as
    // history; the status projection selects the active one.
    for entry in staged.iter() {
        for diagnostic in &entry.diagnostics {
            let mut diagnostic = diagnostic.clone();
            if diagnostic.logical_plan_id.is_none() {
                diagnostic.logical_plan_id = Some(entry.logical_plan_id.clone());
            }
            insert_diagnostic(transaction, generation, &diagnostic)?;
        }
        for conflict in &entry.conflicts {
            let mut conflict = conflict.clone();
            if conflict.logical_plan_id.is_none() {
                conflict.logical_plan_id = Some(entry.logical_plan_id.clone());
            }
            insert_conflict(transaction, generation, &conflict)?;
        }
    }

    // Both divergent occurrences and duplicate epics are computed over the
    // promoted set, not over one page, so a per-page view cannot hide them.
    let mut revision_roots =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<String, String>>::new();
    let mut epics = std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for entry in staged.iter() {
        for occurrence in &entry.occurrences {
            revision_roots
                .entry(entry.logical_plan_id.clone())
                .or_default()
                .insert(
                    occurrence.repository_root.clone(),
                    occurrence.content.revision_id.clone(),
                );
            if let Some(epic) = &occurrence.content.epic {
                epics
                    .entry(epic.clone())
                    .or_default()
                    .insert(entry.logical_plan_id.clone());
            }
        }
    }
    for (logical_plan_id, roots) in &revision_roots {
        let revisions: std::collections::BTreeSet<&String> = roots.values().collect();
        if roots.len() > 1 && revisions.len() > 1 {
            insert_conflict(
                transaction,
                generation,
                &Conflict {
                    kind: ConflictKind::DivergentWorktree,
                    logical_plan_id: Some(logical_plan_id.clone()),
                    message: "The same logical plan has different content revisions in different worktrees."
                        .to_owned(),
                    observed_at: observed_at.clone(),
                },
            )?;
        }
    }
    for (epic, plans) in &epics {
        if plans.len() > 1 {
            insert_conflict(
                transaction,
                generation,
                &Conflict {
                    kind: ConflictKind::DuplicateEpic,
                    logical_plan_id: None,
                    message: format!(
                        "{} logical plans declare epic {epic}; operator choice remains explicit.",
                        plans.len()
                    ),
                    observed_at: observed_at.clone(),
                },
            )?;
        }
    }
    for entry in staged.iter() {
        for conflict in &entry.conflicts {
            if conflict.kind == ConflictKind::PinMismatch
                || conflict.kind == ConflictKind::AssociationConflict
            {
                insert_conflict(transaction, generation, conflict)?;
            }
        }
    }
    Ok(())
}

fn promote_incomplete(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    staged: &[ObservedEntry],
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    for entry in staged {
        // Hidden insert only. An existing row is not updated, because an
        // incomplete scan has not seen the whole scope and therefore cannot
        // say what changed.
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_logical_plans
                 (logical_plan_id, kind, title, state, first_seen, last_seen,
                  first_seen_generation, last_seen_generation, visible_generation, removed_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?4, ?5, ?5, NULL, NULL)",
            params![
                entry.logical_plan_id,
                entry.kind.as_str(),
                entry.title,
                observed_at,
                generation_to_sql(generation)
            ],
        )?;
        for occurrence in &entry.occurrences {
            insert_content_revision(transaction, occurrence, &observed_at)?;
            transaction.execute(
                "INSERT OR IGNORE INTO catalog_occurrences
                     (occurrence_id, logical_plan_id, repository_id, repository_root,
                      relative_path, content_kind, revision_kind, revision_id,
                      expected_sha256, state, first_seen, last_seen,
                      first_seen_generation, last_seen_generation, visible_generation,
                      removed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10, ?11, ?11, NULL, NULL)",
                params![
                    occurrence.occurrence_id,
                    entry.logical_plan_id,
                    occurrence.repository_id,
                    occurrence.repository_root,
                    occurrence.relative_path,
                    content_kind_text(occurrence.content_kind),
                    revision_kind_text(occurrence.revision_kind),
                    occurrence.content.revision_id,
                    occurrence.expected_sha256,
                    observed_at,
                    generation_to_sql(generation)
                ],
            )?;
            for source_id in &occurrence.source_ids {
                transaction.execute(
                    "INSERT OR IGNORE INTO catalog_occurrence_sources
                         (occurrence_id, source_id) VALUES (?1, ?2)",
                    params![occurrence.occurrence_id, source_id],
                )?;
            }
        }
        for association in &entry.associations {
            insert_association(
                transaction,
                generation,
                &entry.logical_plan_id,
                association,
                &observed_at,
            )?;
        }
    }
    Ok(())
}

fn promote_entry(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    entry: &ObservedEntry,
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    transaction.execute(
        "INSERT INTO catalog_logical_plans
             (logical_plan_id, kind, title, state, first_seen, last_seen,
              first_seen_generation, last_seen_generation, visible_generation, removed_at)
         VALUES (?1, ?2, ?3, 'active', ?4, ?4, ?5, ?5, ?5, NULL)
         ON CONFLICT (logical_plan_id) DO UPDATE SET
             kind = excluded.kind,
             title = excluded.title,
             state = 'active',
             last_seen = excluded.last_seen,
             last_seen_generation = excluded.last_seen_generation,
             visible_generation = excluded.visible_generation,
             removed_at = NULL",
        params![
            entry.logical_plan_id,
            entry.kind.as_str(),
            entry.title,
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    if let Some(explicit) = &entry.explicit_id {
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_aliases
                 (logical_plan_id, alias_kind, alias_value, observed_at)
             VALUES (?1, 'explicit_id', ?2, ?3)",
            params![entry.logical_plan_id, explicit, observed_at],
        )?;
    }
    for occurrence in &entry.occurrences {
        insert_content_revision(transaction, occurrence, &observed_at)?;
        transaction.execute(
            "INSERT INTO catalog_occurrences
                 (occurrence_id, logical_plan_id, repository_id, repository_root,
                  relative_path, content_kind, revision_kind, revision_id,
                  expected_sha256, state, first_seen, last_seen,
                  first_seen_generation, last_seen_generation, visible_generation,
                  removed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10, ?11, ?11, ?11, NULL)
             ON CONFLICT (occurrence_id) DO UPDATE SET
                 logical_plan_id = excluded.logical_plan_id,
                 repository_id = excluded.repository_id,
                 repository_root = excluded.repository_root,
                 relative_path = excluded.relative_path,
                 content_kind = excluded.content_kind,
                 revision_kind = excluded.revision_kind,
                 revision_id = excluded.revision_id,
                 expected_sha256 = excluded.expected_sha256,
                 state = 'active',
                 last_seen = excluded.last_seen,
                 last_seen_generation = excluded.last_seen_generation,
                 visible_generation = excluded.visible_generation,
                 removed_at = NULL",
            params![
                occurrence.occurrence_id,
                entry.logical_plan_id,
                occurrence.repository_id,
                occurrence.repository_root,
                occurrence.relative_path,
                content_kind_text(occurrence.content_kind),
                revision_kind_text(occurrence.revision_kind),
                occurrence.content.revision_id,
                occurrence.expected_sha256,
                observed_at,
                generation_to_sql(generation)
            ],
        )?;
        transaction.execute(
            "DELETE FROM catalog_occurrence_sources WHERE occurrence_id = ?1",
            [&occurrence.occurrence_id],
        )?;
        for source_id in &occurrence.source_ids {
            transaction.execute(
                "INSERT OR IGNORE INTO catalog_occurrence_sources
                     (occurrence_id, source_id) VALUES (?1, ?2)",
                params![occurrence.occurrence_id, source_id],
            )?;
        }
    }
    for association in &entry.associations {
        insert_association(
            transaction,
            generation,
            &entry.logical_plan_id,
            association,
            &observed_at,
        )?;
    }
    Ok(())
}

fn insert_content_revision(
    transaction: &rusqlite::Transaction<'_>,
    occurrence: &ObservedOccurrence,
    observed_at: &str,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO catalog_content_revisions
             (revision_id, byte_length, modified_at, title, epic, workstream,
              first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT (revision_id) DO UPDATE SET
             last_seen = excluded.last_seen,
             title = excluded.title,
             epic = excluded.epic,
             workstream = excluded.workstream",
        params![
            occurrence.content.revision_id,
            i64::try_from(occurrence.content.byte_length).unwrap_or(i64::MAX),
            occurrence.content.modified_at,
            occurrence.content.title,
            occurrence.content.epic,
            occurrence.content.workstream,
            observed_at
        ],
    )?;
    Ok(())
}

fn insert_association(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    logical_plan_id: &str,
    association: &ObservedAssociation,
    observed_at: &str,
) -> Result<(), StoreError> {
    let association_id = association_id(
        logical_plan_id,
        association.relation,
        &association.value,
        association.kind,
        association.source_id.as_deref(),
    );
    transaction.execute(
        "INSERT INTO catalog_associations
             (association_id, logical_plan_id, relation, value, association_kind,
              source_id, verified, dangling, note, first_seen, last_seen,
              first_seen_generation, last_seen_generation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7, ?8, ?8, ?9, ?9)
         ON CONFLICT (association_id) DO UPDATE SET
             last_seen = excluded.last_seen,
             note = excluded.note,
             dangling = 0,
             last_seen_generation = excluded.last_seen_generation",
        params![
            association_id,
            logical_plan_id,
            relation_text(association.relation),
            association.value,
            association_kind_text(association.kind),
            association.source_id,
            association.note,
            observed_at,
            generation_to_sql(generation)
        ],
    )?;
    Ok(())
}

fn insert_diagnostic(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    diagnostic: &Diagnostic,
) -> Result<(), StoreError> {
    let id = crate::config::sha256(
        format!(
            "{}|{}|{}|{}",
            generation,
            diagnostic.code,
            diagnostic.subject.as_deref().unwrap_or_default(),
            diagnostic.message
        )
        .as_bytes(),
    );
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_diagnostics
             (diagnostic_id, scope, generation, code, severity, message, subject,
              logical_plan_id, observed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            crate::contract::HOST_SCOPE,
            generation_to_sql(generation),
            diagnostic.code,
            severity_text(diagnostic.severity),
            bounded(&diagnostic.message),
            diagnostic.subject,
            diagnostic.logical_plan_id,
            diagnostic.observed_at
        ],
    )?;
    Ok(())
}

fn insert_conflict(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    conflict: &Conflict,
) -> Result<(), StoreError> {
    let id = crate::config::sha256(
        format!(
            "{}|{:?}|{}|{}",
            generation,
            conflict.kind,
            conflict.logical_plan_id.as_deref().unwrap_or_default(),
            conflict.message
        )
        .as_bytes(),
    );
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_conflicts
             (conflict_id, scope, generation, kind, logical_plan_id, message, observed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            crate::contract::HOST_SCOPE,
            generation_to_sql(generation),
            conflict_kind_text(conflict.kind),
            conflict.logical_plan_id,
            bounded(&conflict.message),
            conflict.observed_at
        ],
    )?;
    Ok(())
}

fn load_previous(
    transaction: &rusqlite::Transaction<'_>,
    active: i64,
) -> Result<Vec<PreviousPlan>, StoreError> {
    if active == 0 {
        return Ok(Vec::new());
    }
    let mut statement = transaction.prepare(
        "SELECT p.logical_plan_id, p.kind, r.epic, o.repository_id, o.revision_id,
                o.relative_path
         FROM catalog_logical_plans p
         JOIN catalog_occurrences o ON o.logical_plan_id = p.logical_plan_id
         JOIN catalog_content_revisions r ON r.revision_id = o.revision_id
         WHERE p.visible_generation = ?1 AND p.state = 'active'
           AND o.visible_generation = ?1 AND o.state = 'active'
         ORDER BY p.logical_plan_id, o.relative_path",
    )?;
    let rows = statement.query_map([active], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut plans: Vec<PreviousPlan> = Vec::new();
    for row in rows {
        let (logical_plan_id, kind, epic, repository_id, revision_id, relative_path) = row?;
        if let Some(plan) = plans
            .iter_mut()
            .find(|plan| plan.logical_plan_id == logical_plan_id)
        {
            plan.occurrences.push(PreviousOccurrence {
                repository_id,
                revision_id,
                relative_path,
            });
            continue;
        }
        plans.push(PreviousPlan {
            logical_plan_id,
            kind: match kind.as_str() {
                "execution_unit" => EntryKind::ExecutionUnit,
                _ => EntryKind::Plan,
            },
            epic,
            occurrences: vec![PreviousOccurrence {
                repository_id,
                revision_id,
                relative_path,
            }],
        });
    }
    Ok(plans)
}

type CollectionRow = (
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    i64,
    i64,
    i64,
    i64,
    Option<String>,
    String,
);

fn read_collection(connection: &Connection, active: u64) -> Result<CollectionStatus, StoreError> {
    let row: Option<CollectionRow> = connection
        .query_row(
            "SELECT generation, attempt_generation, state, started_at, finished_at,
                        duration_ms, repositories_configured, repositories_scanned,
                        files_seen, entries_observed, error, coverage_json
                 FROM catalog_collection WHERE scope = ?1",
            [crate::contract::HOST_SCOPE],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                ))
            },
        )
        .optional()?;
    let Some((
        generation,
        attempt_generation,
        state,
        started_at,
        finished_at,
        duration_ms,
        repositories_configured,
        repositories_scanned,
        files_seen,
        entries_observed,
        error,
        coverage_json,
    )) = row
    else {
        return Ok(CollectionStatus {
            state: CollectionState::NeverCollected,
            generation: active,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            repositories_configured: 0,
            repositories_scanned: 0,
            files_seen: 0,
            entries_observed: 0,
            error: None,
            diagnostics: Vec::new(),
            coverage: Vec::new(),
        });
    };
    let _ = generation;
    let coverage: Vec<RepositoryCoverage> =
        serde_json::from_str(&coverage_json).unwrap_or_default();
    let state = match state.as_str() {
        "complete" => CollectionState::Complete,
        "incomplete" => CollectionState::Incomplete,
        "failed" => CollectionState::Failed,
        _ => CollectionState::NeverCollected,
    };
    let diagnostics: Vec<String> = connection
        .query_row(
            "SELECT diagnostics_json FROM catalog_sweeps
             WHERE scope = ?1 AND generation = ?2",
            params![crate::contract::HOST_SCOPE, attempt_generation],
            |row| row.get(0),
        )
        .optional()?
        .and_then(|json: String| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    Ok(CollectionStatus {
        state,
        generation: u64::try_from(attempt_generation).unwrap_or(0),
        started_at,
        finished_at,
        duration_ms: duration_ms.and_then(|value| u64::try_from(value).ok()),
        repositories_configured: usize_from(repositories_configured),
        repositories_scanned: usize_from(repositories_scanned),
        files_seen: usize_from(files_seen),
        entries_observed: usize_from(entries_observed),
        error,
        diagnostics,
        coverage,
    })
}

fn read_counts(connection: &Connection, active: u64) -> Result<CatalogCounts, StoreError> {
    if active == 0 {
        return Ok(CatalogCounts::default());
    }
    let logical_plans: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_logical_plans WHERE visible_generation = ?1",
        [generation_to_sql(active)],
        |row| row.get(0),
    )?;
    let occurrences: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_occurrences
         WHERE visible_generation = ?1 AND state = 'active'",
        [generation_to_sql(active)],
        |row| row.get(0),
    )?;
    let content_revisions: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_content_revisions",
        [],
        |row| row.get(0),
    )?;
    let tombstones: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_logical_plans
         WHERE visible_generation = ?1 AND state = 'removed'",
        [generation_to_sql(active)],
        |row| row.get(0),
    )?;
    let dangling: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_associations WHERE dangling = 1",
        [],
        |row| row.get(0),
    )?;
    let diagnostics: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_diagnostics
         WHERE scope = ?1 AND generation = ?2",
        params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
        |row| row.get(0),
    )?;
    let conflicts: i64 = connection.query_row(
        "SELECT COUNT(*) FROM catalog_conflicts
         WHERE scope = ?1 AND generation = ?2",
        params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
        |row| row.get(0),
    )?;
    Ok(CatalogCounts {
        logical_plans: usize_from(logical_plans),
        occurrences: usize_from(occurrences),
        content_revisions: usize_from(content_revisions),
        tombstones: usize_from(tombstones),
        dangling_associations: usize_from(dangling),
        diagnostics: usize_from(diagnostics),
        conflicts: usize_from(conflicts),
    })
}

fn read_diagnostics(connection: &Connection, active: u64) -> Result<Vec<Diagnostic>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT code, severity, message, subject, logical_plan_id, observed_at
         FROM catalog_diagnostics
         WHERE scope = ?1 AND generation = ?2
         ORDER BY diagnostic_id",
    )?;
    let rows = statement.query_map(
        params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
        |row| {
            let severity: String = row.get(1)?;
            Ok(Diagnostic {
                code: row.get(0)?,
                severity: match severity.as_str() {
                    "info" => DiagnosticSeverity::Info,
                    "error" => DiagnosticSeverity::Error,
                    _ => DiagnosticSeverity::Warning,
                },
                message: row.get(2)?,
                subject: row.get(3)?,
                logical_plan_id: row.get(4)?,
                observed_at: row.get(5)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_conflicts(connection: &Connection, active: u64) -> Result<Vec<Conflict>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT kind, logical_plan_id, message, observed_at
         FROM catalog_conflicts
         WHERE scope = ?1 AND generation = ?2
         ORDER BY conflict_id",
    )?;
    let rows = statement.query_map(
        params![crate::contract::HOST_SCOPE, generation_to_sql(active)],
        |row| {
            let kind: String = row.get(0)?;
            Ok(Conflict {
                kind: parse_conflict_kind(&kind),
                logical_plan_id: row.get(1)?,
                message: row.get(2)?,
                observed_at: row.get(3)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_dangling(
    connection: &Connection,
    _active: u64,
) -> Result<Vec<DanglingAssociation>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT relation, value, source_id, logical_plan_id, last_seen
         FROM catalog_associations
         WHERE dangling = 1
         ORDER BY association_id",
    )?;
    let rows = statement.query_map([], |row| {
        let relation: String = row.get(0)?;
        Ok(DanglingAssociation {
            relation: parse_relation(&relation),
            value: row.get(1)?,
            source_id: row.get(2)?,
            logical_plan_id: row.get(3)?,
            last_seen: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_entries_at(
    connection: &Connection,
    active: u64,
    include_removed: bool,
) -> Result<Vec<CatalogEntry>, StoreError> {
    let mut plans_statement = connection.prepare(
        "SELECT logical_plan_id, kind, title, state, first_seen, last_seen
         FROM catalog_logical_plans
         WHERE visible_generation = ?1
         ORDER BY kind, logical_plan_id",
    )?;
    let plans = plans_statement
        .query_map([generation_to_sql(active)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = Vec::with_capacity(plans.len());
    for (logical_plan_id, kind, title, state, first_seen, last_seen) in plans {
        if !include_removed && state == "removed" {
            continue;
        }
        let occurrences = read_occurrences(connection, active, &logical_plan_id, include_removed)?;
        let aliases = read_aliases(connection, &logical_plan_id)?;
        let associations = read_associations(connection, active, &logical_plan_id)?;
        let diagnostics = read_entry_diagnostics(connection, active, &logical_plan_id)?;
        let conflicts = read_entry_conflicts(connection, active, &logical_plan_id)?;
        result.push(CatalogEntry {
            logical_plan_id,
            kind: match kind.as_str() {
                "execution_unit" => EntryKind::ExecutionUnit,
                _ => EntryKind::Plan,
            },
            title,
            state: match state.as_str() {
                "removed" => EntryState::Removed,
                _ => EntryState::Active,
            },
            aliases,
            associations,
            occurrences,
            diagnostics,
            conflicts,
            first_seen,
            last_seen,
        });
    }
    Ok(result)
}

fn read_occurrences(
    connection: &Connection,
    active: u64,
    logical_plan_id: &str,
    include_removed: bool,
) -> Result<Vec<Occurrence>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT o.occurrence_id, o.repository_id, o.repository_root, o.relative_path,
                o.content_kind, o.revision_kind, o.expected_sha256, o.state,
                o.first_seen, o.last_seen,
                r.revision_id, r.byte_length, r.modified_at, r.title, r.epic, r.workstream
         FROM catalog_occurrences o
         JOIN catalog_content_revisions r ON r.revision_id = o.revision_id
         WHERE o.logical_plan_id = ?1 AND o.visible_generation = ?2
         ORDER BY o.repository_root, o.relative_path",
    )?;
    let rows = statement.query_map(params![logical_plan_id, generation_to_sql(active)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, String>(12)?,
            row.get::<_, String>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<String>>(15)?,
        ))
    })?;
    let mut occurrences = Vec::new();
    for row in rows {
        let (
            occurrence_id,
            repository_id,
            repository_root,
            relative_path,
            content_kind,
            revision_kind,
            expected_sha256,
            state,
            first_seen,
            last_seen,
            revision_id,
            byte_length,
            modified_at,
            title,
            epic,
            workstream,
        ) = row?;
        if !include_removed && state == "removed" {
            continue;
        }
        let pin_matches = expected_sha256
            .as_deref()
            .map(|expected| expected == revision_id);
        let source_ids = read_occurrence_sources(connection, &occurrence_id)?;
        occurrences.push(Occurrence {
            occurrence_id,
            repository_id,
            repository_root,
            relative_path,
            content_kind: match content_kind.as_str() {
                "untracked_draft" => ContentKind::UntrackedDraft,
                "external_source" => ContentKind::ExternalSource,
                _ => ContentKind::Tracked,
            },
            revision_kind: match revision_kind.as_str() {
                "saved_draft" => RevisionKind::SavedDraft,
                "approved_revision" => RevisionKind::ApprovedRevision,
                _ => RevisionKind::WorkingCopy,
            },
            content: ContentRevision {
                revision_id,
                byte_length: u64::try_from(byte_length).unwrap_or(0),
                modified_at,
                title,
                epic,
                workstream,
            },
            source_ids,
            expected_sha256,
            pin_matches,
            state: if state == "removed" {
                EntryState::Removed
            } else {
                EntryState::Active
            },
            first_seen,
            last_seen,
        });
    }
    Ok(occurrences)
}

fn read_occurrence_sources(
    connection: &Connection,
    occurrence_id: &str,
) -> Result<Vec<String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT source_id FROM catalog_occurrence_sources
         WHERE occurrence_id = ?1 ORDER BY source_id",
    )?;
    let rows = statement.query_map([occurrence_id], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_aliases(connection: &Connection, logical_plan_id: &str) -> Result<Vec<Alias>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT alias_kind, alias_value, observed_at FROM catalog_aliases
         WHERE logical_plan_id = ?1 ORDER BY alias_kind, alias_value",
    )?;
    let rows = statement.query_map([logical_plan_id], |row| {
        let kind: String = row.get(0)?;
        Ok(Alias {
            kind: match kind.as_str() {
                "renamed_from" => AliasKind::RenamedFrom,
                "explicit_id" => AliasKind::ExplicitId,
                _ => AliasKind::SourceId,
            },
            value: row.get(1)?,
            observed_at: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_associations(
    connection: &Connection,
    active: u64,
    logical_plan_id: &str,
) -> Result<Vec<Association>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT relation, value, association_kind, source_id, verified, dangling, note
         FROM catalog_associations
         WHERE logical_plan_id = ?1
           AND (last_seen_generation = ?2 OR dangling = 1)
         ORDER BY relation, value, association_kind",
    )?;
    let rows = statement.query_map(params![logical_plan_id, generation_to_sql(active)], |row| {
        let relation: String = row.get(0)?;
        let kind: String = row.get(2)?;
        let verified: i64 = row.get(4)?;
        let dangling: i64 = row.get(5)?;
        Ok(Association {
            relation: parse_relation(&relation),
            value: row.get(1)?,
            kind: if kind == "explicit_operator" {
                AssociationKind::ExplicitOperator
            } else {
                AssociationKind::PlanDeclaration
            },
            source_id: row.get(3)?,
            verified: verified != 0,
            dangling: dangling != 0,
            note: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_entry_diagnostics(
    connection: &Connection,
    active: u64,
    logical_plan_id: &str,
) -> Result<Vec<Diagnostic>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT code, severity, message, subject, logical_plan_id, observed_at
         FROM catalog_diagnostics
         WHERE scope = ?1 AND generation = ?2 AND logical_plan_id = ?3
         ORDER BY diagnostic_id",
    )?;
    let rows = statement.query_map(
        params![
            crate::contract::HOST_SCOPE,
            generation_to_sql(active),
            logical_plan_id
        ],
        |row| {
            let severity: String = row.get(1)?;
            Ok(Diagnostic {
                code: row.get(0)?,
                severity: match severity.as_str() {
                    "info" => DiagnosticSeverity::Info,
                    "error" => DiagnosticSeverity::Error,
                    _ => DiagnosticSeverity::Warning,
                },
                message: row.get(2)?,
                subject: row.get(3)?,
                logical_plan_id: row.get(4)?,
                observed_at: row.get(5)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_entry_conflicts(
    connection: &Connection,
    active: u64,
    logical_plan_id: &str,
) -> Result<Vec<Conflict>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT kind, logical_plan_id, message, observed_at
         FROM catalog_conflicts
         WHERE scope = ?1 AND generation = ?2 AND logical_plan_id = ?3
         ORDER BY conflict_id",
    )?;
    let rows = statement.query_map(
        params![
            crate::contract::HOST_SCOPE,
            generation_to_sql(active),
            logical_plan_id
        ],
        |row| {
            let kind: String = row.get(0)?;
            Ok(Conflict {
                kind: parse_conflict_kind(&kind),
                logical_plan_id: row.get(1)?,
                message: row.get(2)?,
                observed_at: row.get(3)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn association_id(
    logical_plan_id: &str,
    relation: AssociationRelation,
    value: &str,
    kind: AssociationKind,
    source_id: Option<&str>,
) -> String {
    crate::config::sha256(
        format!(
            "{logical_plan_id}|{}|{value}|{}|{}",
            relation_text(relation),
            association_kind_text(kind),
            source_id.unwrap_or_default()
        )
        .as_bytes(),
    )
}

fn usize_from(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0)
}

fn content_kind_text(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Tracked => "tracked",
        ContentKind::UntrackedDraft => "untracked_draft",
        ContentKind::ExternalSource => "external_source",
    }
}

fn revision_kind_text(kind: RevisionKind) -> &'static str {
    match kind {
        RevisionKind::WorkingCopy => "working_copy",
        RevisionKind::SavedDraft => "saved_draft",
        RevisionKind::ApprovedRevision => "approved_revision",
    }
}

fn relation_text(relation: AssociationRelation) -> &'static str {
    match relation {
        AssociationRelation::Workstream => "workstream",
        AssociationRelation::Epic => "epic",
        AssociationRelation::Project => "project",
    }
}

fn parse_relation(text: &str) -> AssociationRelation {
    match text {
        "epic" => AssociationRelation::Epic,
        "project" => AssociationRelation::Project,
        _ => AssociationRelation::Workstream,
    }
}

fn association_kind_text(kind: AssociationKind) -> &'static str {
    match kind {
        AssociationKind::ExplicitOperator => "explicit_operator",
        AssociationKind::PlanDeclaration => "plan_declaration",
    }
}

fn severity_text(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Error => "error",
    }
}

fn conflict_kind_text(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::DivergentWorktree => "divergent_worktree",
        ConflictKind::AmbiguousRename => "ambiguous_rename",
        ConflictKind::DuplicateExplicitId => "duplicate_explicit_id",
        ConflictKind::DuplicateEpic => "duplicate_epic",
        ConflictKind::PinMismatch => "pin_mismatch",
        ConflictKind::AssociationConflict => "association_conflict",
    }
}

fn parse_conflict_kind(text: &str) -> ConflictKind {
    match text {
        "ambiguous_rename" => ConflictKind::AmbiguousRename,
        "duplicate_explicit_id" => ConflictKind::DuplicateExplicitId,
        "duplicate_epic" => ConflictKind::DuplicateEpic,
        "pin_mismatch" => ConflictKind::PinMismatch,
        "association_conflict" => ConflictKind::AssociationConflict,
        _ => ConflictKind::DivergentWorktree,
    }
}
