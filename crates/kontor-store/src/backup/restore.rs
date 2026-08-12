//! Putting a verified snapshot back, and refusing to put it anywhere else.
//!
//! Restore is the raw operation: it reinstates a Realm's own bytes, so command
//! receipts, runtime bindings, continuity evidence and every identity in the
//! file come back exactly as they were. That is what makes it the right tool
//! for disaster recovery of *this* Realm and the wrong tool for everything
//! else — moving work between Realms is [`crate::backup::import_export`], which
//! creates new destination receipts instead of reviving somebody else's.
//!
//! Two rules follow from that, and neither has an escape hatch:
//!
//! * the snapshot is validated **completely** before the destination is
//!   touched — manifest, length, digest, integrity check, schema version and
//!   Realm identity;
//! * the destination must be uninitialized or the **same** Realm. A different
//!   initialized Realm is refused with a typed error before any rename, and
//!   there is no `--force` that changes it. A flag that lets an operator
//!   overwrite realm B with realm A's database at 3am is not a recovery
//!   feature.
//!
//! Restore is also offline. This module cannot prove that on its own — the
//! exclusive claim on a state root is the daemon's lock — so the caller holds
//! that lock, and the returned outcome says in as many words that scheduling
//! must stay shut until reconciliation has run.

use std::path::{Path, PathBuf};

use kontor_core::id::{RealmId, Timestamp, format_utc_timestamp};
use rusqlite::{Connection, OpenFlags};

use crate::backup::manifest::SnapshotManifest;
use crate::backup::snapshot::{sync_file, verify_database_file};
use crate::backup::{BackupError, io, sync_directory};

/// What one successful restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    /// The Realm that now lives in the destination.
    pub realm_id: RealmId,
    /// The database file that was published.
    pub restored: PathBuf,
    /// Where the previous database was moved, when there was one. It is never
    /// deleted: a restore that turns out to have been the wrong call must still
    /// have something to go back to.
    pub superseded: Option<PathBuf>,
    /// The manifest of the snapshot that was restored.
    pub manifest: SnapshotManifest,
    /// Always true, and stated rather than implied: a restored Realm knows
    /// nothing about what its runtimes did while the snapshot was on the shelf,
    /// so scheduling stays shut until reconciliation has completed.
    pub reconciliation_required: bool,
}

/// Restore `snapshot` into `destination`, offline.
///
/// # Errors
/// Returns [`BackupError::Manifest`] or [`BackupError::Verification`] when the
/// snapshot is not a whole, verified database of a single Realm;
/// [`BackupError::DestinationInitialized`] when the destination already holds a
/// *different* Realm; and [`BackupError::Io`] for filesystem failures. Every one
/// of them leaves the destination exactly as it was.
pub fn restore_snapshot(
    snapshot: &Path,
    destination: &Path,
    now: Timestamp,
) -> Result<RestorePlan, BackupError> {
    // 1. The snapshot is judged on its own, completely, before anything else is
    //    opened. A snapshot that fails here never reaches the destination.
    let manifest = SnapshotManifest::read(&SnapshotManifest::path_for(snapshot))?;
    manifest.verify_file(snapshot)?;
    let identity = verify_database_file(snapshot)?;
    if identity.realm_id != manifest.realm_id {
        return Err(BackupError::CrossRealm {
            found: identity.realm_id,
            expected: manifest.realm_id,
        });
    }
    if identity.schema_version != manifest.database_schema_version {
        return Err(BackupError::Manifest {
            detail: "the manifest's schema version is not the snapshot's",
        });
    }

    // 2. The destination is classified read-only, so the cross-realm refusal
    //    happens before a single byte of it changes.
    let occupant = classify_destination(destination)?;
    if let Some(found) = occupant
        && found != identity.realm_id
    {
        return Err(BackupError::DestinationInitialized { found });
    }

    // 3. Copy beside the destination, then publish by rename. Same directory, so
    //    the rename is atomic on every filesystem this runs on.
    let directory = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(directory).map_err(io("created"))?;
    let temporary = directory.join(format!(
        "{}.{}.restore.partial",
        file_name(destination),
        std::process::id()
    ));
    if temporary.exists() {
        return Err(BackupError::AlreadyExists { path: temporary });
    }
    std::fs::copy(snapshot, &temporary).map_err(io("written"))?;
    let published = publish(&temporary, destination, occupant.is_some(), now);
    if published.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    let superseded = published?;

    Ok(RestorePlan {
        realm_id: identity.realm_id,
        restored: destination.to_path_buf(),
        superseded,
        manifest,
        reconciliation_required: true,
    })
}

/// Fold the outgoing database's WAL into it, move it aside and publish the copy.
fn publish(
    temporary: &Path,
    destination: &Path,
    occupied: bool,
    now: Timestamp,
) -> Result<Option<PathBuf>, BackupError> {
    // The copy is verified in its new home before it replaces anything: a
    // truncated `fs::copy` is exactly the failure this catches.
    verify_database_file(temporary)?;
    sync_file(temporary)?;

    let mut superseded = None;
    if occupied {
        // Checkpointing first is what makes the file we move aside a *whole*
        // database: a WAL left beside a database that is about to be renamed
        // away carries committed transactions the moved file would not have.
        checkpoint(destination);
        let instant: String = format_utc_timestamp(now)
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let mut aside = destination.as_os_str().to_os_string();
        aside.push(format!(".superseded-{instant}"));
        let aside = PathBuf::from(aside);
        std::fs::rename(destination, &aside).map_err(io("published"))?;
        superseded = Some(aside);
    }
    // Whatever WAL and shared-memory files are left belong to the database that
    // just moved aside. Leaving them here is the one way a restored file can be
    // corrupted after the fact: SQLite would try to recover a stranger's WAL
    // into it on the next open.
    for residue in ["-wal", "-shm"] {
        let mut path = destination.as_os_str().to_os_string();
        path.push(residue);
        let _ = std::fs::remove_file(PathBuf::from(path));
    }

    std::fs::rename(temporary, destination).map_err(io("published"))?;
    if let Some(directory) = destination.parent() {
        sync_directory(directory);
    }
    Ok(superseded)
}

/// Which Realm, if any, the destination already holds.
///
/// A destination that exists but cannot be identified is refused rather than
/// treated as empty: "I could not read it" is not "there is nothing there", and
/// the difference is somebody's database.
fn classify_destination(destination: &Path) -> Result<Option<RealmId>, BackupError> {
    let metadata = match std::fs::metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io("read")(error)),
    };
    if metadata.len() == 0 {
        // A zero-length file is what an interrupted create leaves behind, and
        // SQLite reads it as an empty database. There is no Realm in it.
        return Ok(None);
    }
    verify_database_file(destination).map(|identity| Some(identity.realm_id))
}

/// Fold a database's WAL into its main file, best effort.
///
/// Best effort on purpose: this runs on the file that is about to be moved
/// aside for evidence. If it cannot be opened, the restore still proceeds — the
/// snapshot is what is being published, and the moved-aside file is a courtesy.
fn checkpoint(database: &Path) {
    let Ok(connection) = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE)
    else {
        return;
    };
    let _ = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
}

/// The file name of a path, as text, for building sibling names.
fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("kontor.db")
        .to_owned()
}
