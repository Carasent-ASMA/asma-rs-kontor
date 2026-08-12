//! The operator-facing backup, restore, export and import operations.
//!
//! The store owns the mechanics; this module owns the two decisions the store
//! cannot make, because both are about the *process*:
//!
//! * **which operations need the state root to itself.** A snapshot and an
//!   export read a live database and are safe while the daemon serves — that is
//!   the point of `VACUUM INTO`. A restore and an import replace or extend the
//!   file a running daemon has open and its Realm identity cached, so they take
//!   the same exclusive lock a daemon does, and a running daemon makes them fail
//!   cleanly instead of racing it.
//! * **where the files live.** Snapshots go in `backups/` inside the state root
//!   unless the operator names somewhere else, so the default backup of a Realm
//!   is beside the Realm and moves with it.
//!
//! After a restore the scheduling barrier is shut, because a restored Realm is a
//! process that has not started yet: the next daemon opens the file, takes the
//! usual inventory of receipts and bindings, and only then opens scheduling. No
//! path here opens it, and no path here dispatches anything.

use std::path::{Path, PathBuf};

use kontor_core::id::{ProjectId, Timestamp};
use kontor_store::SqliteStore;
use kontor_store::backup::{
    BackupError, ImportPlan, ImportReport, KontorExportV1, RestorePlan, SnapshotOutcome,
    create_snapshot, export_realm, import_export, prune_snapshots, restore_snapshot,
};
use tracing::info;

use crate::lock::{LockError, StateRootLock};
use crate::{DATABASE_FILE, credentials};

/// The directory snapshots are written to inside a state root.
pub const BACKUP_DIRECTORY: &str = "backups";

/// Why an operator-facing recovery operation could not run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RecoveryError {
    /// Another process owns the state root, so an offline operation cannot run.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// The backup operation itself refused.
    #[error(transparent)]
    Backup(#[from] BackupError),
    /// The database could not be opened.
    #[error("the control-plane database could not be opened: {source}")]
    Store {
        /// The underlying failure.
        #[source]
        source: kontor_store::StoreError,
    },
    /// The export document could not be read or written.
    #[error("the export file could not be {action}: {source}")]
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// The Realm's credentials could not be rotated.
    #[error(transparent)]
    Credentials(#[from] credentials::CredentialError),
}

impl RecoveryError {
    /// The stable, loggable category of this refusal.
    #[must_use]
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Lock(_) => "state_root_locked",
            Self::Backup(error) => error.category(),
            Self::Store { .. } => "store",
            Self::Io { .. } => "io",
            Self::Credentials(_) => "credentials",
        }
    }
}

/// The database file inside a state root.
#[must_use]
pub fn database_in(state_root: &Path) -> PathBuf {
    state_root.join(DATABASE_FILE)
}

/// The snapshot directory for a state root.
#[must_use]
pub fn backups_in(state_root: &Path) -> PathBuf {
    state_root.join(BACKUP_DIRECTORY)
}

/// Take a verified snapshot and prune this Realm's stale ones.
///
/// Safe while the daemon serves. Pruning happens only after the new snapshot is
/// published, so a failed backup can never be the reason an old one was removed.
///
/// # Errors
/// Returns [`RecoveryError::Backup`] when the database fails verification or the
/// snapshot cannot be published. Nothing is pruned in that case.
pub fn snapshot(
    state_root: &Path,
    into: Option<&Path>,
    now: Timestamp,
) -> Result<(SnapshotOutcome, Vec<PathBuf>), RecoveryError> {
    let directory = into.map_or_else(|| backups_in(state_root), Path::to_path_buf);
    let outcome = create_snapshot(&database_in(state_root), &directory, now)?;
    let removed = prune_snapshots(&directory, outcome.manifest.realm_id)?;
    info!(
        realm_id = %outcome.manifest.realm_id,
        snapshot = %outcome.snapshot.display(),
        pruned = removed.len(),
        "snapshot published"
    );
    Ok((outcome, removed))
}

/// Restore a snapshot into a state root, offline.
///
/// # Errors
/// Returns [`RecoveryError::Lock`] when a daemon still owns the state root, and
/// [`RecoveryError::Backup`] when the snapshot is not verified or the
/// destination holds a different Realm. The destination is unchanged in both
/// cases.
pub fn restore(
    state_root: &Path,
    snapshot: &Path,
    now: Timestamp,
) -> Result<RestorePlan, RecoveryError> {
    std::fs::create_dir_all(state_root).map_err(|source| RecoveryError::Io {
        action: "created",
        source,
    })?;
    // The lock is the proof that this is offline. It is released when this
    // returns, so the next daemon start is the thing that opens the database.
    let _claim = StateRootLock::acquire(state_root)?;
    let plan = restore_snapshot(snapshot, &database_in(state_root), now)?;
    info!(
        realm_id = %plan.realm_id,
        state_root = %state_root.display(),
        reconciliation_required = plan.reconciliation_required,
        "realm restored; scheduling stays shut until the next start reconciles"
    );
    Ok(plan)
}

/// Export a Realm's redacted state.
///
/// Safe while the daemon serves: the document is a read.
///
/// # Errors
/// Returns [`RecoveryError::Store`] when the database cannot be opened and
/// [`RecoveryError::Backup`] when the canary scan refuses the document.
pub fn export(state_root: &Path, now: Timestamp) -> Result<KontorExportV1, RecoveryError> {
    let store = SqliteStore::open(&database_in(state_root))
        .map_err(|source| RecoveryError::Store { source })?;
    let export = export_realm(&store, now)?;
    info!(
        realm_id = %export.source_realm_id,
        records_hash = %export.records_hash,
        record_count = export.continuity_summary.record_counts.values().sum::<u64>(),
        "realm exported"
    );
    Ok(export)
}

/// Import a foreign export into this Realm, offline.
///
/// # Errors
/// Returns [`RecoveryError::Lock`] when a daemon owns the state root,
/// [`RecoveryError::Io`] when the document cannot be read, and
/// [`RecoveryError::Backup`] when it is not a document this build takes or the
/// destination refuses it.
pub fn import(
    state_root: &Path,
    document: &Path,
    destination_project: ProjectId,
    now: Timestamp,
) -> Result<ImportReport, RecoveryError> {
    let _claim = StateRootLock::acquire(state_root)?;
    let bytes = std::fs::read(document).map_err(|source| RecoveryError::Io {
        action: "read",
        source,
    })?;
    let export = KontorExportV1::parse(&bytes)?;
    let store = SqliteStore::open(&database_in(state_root))
        .map_err(|source| RecoveryError::Store { source })?;
    let report = import_export(
        &store,
        &export,
        &ImportPlan::redacted_import_into(destination_project),
        now,
    )?;
    info!(
        realm_id = %store.realm_id(),
        source_realm_id = %report.source_realm_id,
        import_id = %report.import_id,
        record_count = report.record_count,
        materialized = report.materialized,
        reconciliation_required = report.reconciliation_required,
        "foreign export imported under a new destination receipt"
    );
    Ok(report)
}

/// Rotate the Realm's credentials on disk, offline.
///
/// The running daemon's own rotation is [`crate::Daemon::rotate_credentials`],
/// which swaps the in-memory set in the same operation. This one is for a
/// stopped Realm, and the lock is what proves it is stopped: rotating the file
/// under a live process would leave that process answering to tokens that no
/// longer exist on disk.
///
/// # Errors
/// Returns [`RecoveryError::Lock`] when a daemon owns the state root and
/// [`RecoveryError::Credentials`] when the new set cannot be written. The
/// previous credentials stay in force in both cases.
pub fn rotate_credentials(state_root: &Path) -> Result<(), RecoveryError> {
    let _claim = StateRootLock::acquire(state_root)?;
    let _rotated = credentials::rotate(state_root)?;
    info!(
        state_root = %state_root.display(),
        "realm credentials rotated; every previously issued token is refused from now on"
    );
    Ok(())
}
