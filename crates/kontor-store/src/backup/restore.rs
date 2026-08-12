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
//!   initialized Realm is refused with a typed error before anything at the
//!   destination is touched, and there is no `--force` that changes it. A flag that lets an operator
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
use crate::backup::{BackupError, io, link_exclusively, sync_directory};

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

    // 3. Copy beside the destination, then publish it into place. Same
    //    directory, so the publishing link is atomic and cannot cross a
    //    filesystem boundary.
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

/// Fold the outgoing database's WAL into it, set it aside and publish the copy.
///
/// # The order, and why each step can be undone
///
/// ```text
/// checkpoint the outgoing database   (so the file set aside is whole)
///   → link it to `.superseded-<instant>`   (no-clobber; refuses a collision)
///     → unlink the destination name        (the data is safe under the new name)
///       → link the restored copy into the destination name (no-clobber)
///         → drop the outgoing WAL/shm, fsync the directory
/// ```
///
/// The destination is *never* displaced by a failure. Between the second and
/// third step the data has two names and losing either loses nothing; between
/// the third and fourth it has exactly one, the superseded one, which is a real
/// file with a name an operator can act on rather than a temporary. If the
/// publish fails, the original is linked straight back and the superseded name
/// is dropped, so the caller sees an error and a destination byte-for-byte where
/// it was.
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

    let superseded = if occupied {
        // (see `displace_and_publish` for the rollback-safe sequence)
        // Checkpointing first is what makes the file set aside a *whole*
        // database: a WAL left beside a database that is about to lose its name
        // carries committed transactions the set-aside file would not have.
        checkpoint(destination);
        let instant: String = format_utc_timestamp(now)
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let mut aside = destination.as_os_str().to_os_string();
        aside.push(format!(".superseded-{instant}"));
        let aside = PathBuf::from(aside);
        displace_and_publish(temporary, destination, &aside)?;
        Some(aside)
    } else {
        // An empty file is the one thing at the destination that is not a Realm
        // and still holds the name — an interrupted create leaves exactly that,
        // and `classify_destination` reads it as "nothing there". It is dropped
        // rather than superseded, because there is no database in it to keep.
        if std::fs::metadata(destination).is_ok_and(|found| found.len() == 0) {
            std::fs::remove_file(destination).map_err(io("published"))?;
        }
        link_exclusively(temporary, destination)?;
        None
    };

    // The restored data now answers to the destination's name, so the copy's own
    // name is redundant. Removing it is what makes a successful restore leave a
    // state root with nothing in it but the files that belong there.
    let _ = std::fs::remove_file(temporary);

    // Whatever WAL and shared-memory files are left belong to the database that
    // was just set aside. Leaving them here is the one way a restored file can
    // be corrupted after the fact: SQLite would try to recover a stranger's WAL
    // into it on the next open. They go only once the publish has succeeded — a
    // rollback leaves the original and its own residue exactly as they were.
    for residue in ["-wal", "-shm"] {
        let mut path = destination.as_os_str().to_os_string();
        path.push(residue);
        let _ = std::fs::remove_file(PathBuf::from(path));
    }

    if let Some(directory) = destination.parent() {
        sync_directory(directory);
    }
    Ok(superseded)
}

/// Set the occupant aside under `aside` and put `temporary` in its place.
///
/// Every step is no-clobber and every failure undoes the steps before it, so
/// this either fully succeeds or leaves the destination exactly as it found it.
///
/// # Errors
/// Returns [`BackupError::AlreadyExists`] when the superseded name is already
/// taken — a second restore in the same second, or a name an operator created —
/// and [`BackupError::Io`] when a link or an unlink fails. The destination is
/// unchanged in both cases.
fn displace_and_publish(
    temporary: &Path,
    destination: &Path,
    aside: &Path,
) -> Result<(), BackupError> {
    // A second name for the occupant. `rename` would have replaced whatever was
    // already called that — and what is already called that is, by construction,
    // the database some earlier restore set aside.
    link_exclusively(destination, aside)?;

    // The occupant now has two names, so dropping this one loses nothing.
    if let Err(source) = std::fs::remove_file(destination) {
        let _ = std::fs::remove_file(aside);
        return Err(BackupError::Io {
            action: "published",
            source,
        });
    }

    match link_exclusively(temporary, destination) {
        Ok(()) => {
            // The occupant keeps only the superseded name from here, which is
            // the point of it: it is evidence, not a temporary.
            Ok(())
        }
        Err(error) => {
            // Put the original back under its own name. On success the
            // destination is the same inode it always was and the superseded
            // name goes away, so a failed restore leaves no trace at all.
            if link_exclusively(aside, destination).is_ok() {
                let _ = std::fs::remove_file(aside);
            }
            // If even that failed, the superseded file is deliberately *kept*:
            // it is the only remaining name for the original database, and
            // removing it to tidy up would be the one action here that loses
            // data. The error tells the operator which name to look for.
            Err(error)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A publish that fails after the occupant has been set aside must put the
    /// occupant back, under its own name, with its own bytes.
    ///
    /// The failure is injected the only honest way there is at this level: the
    /// thing being published is a *directory*, and a directory cannot be hard
    /// linked. That reaches the one branch a filesystem will not otherwise
    /// produce on demand — a successful supersede followed by a failed publish —
    /// which is exactly the branch that decides whether a failed restore leaves
    /// an operator with a database or with a hole where one used to be.
    #[test]
    fn a_failed_publish_puts_the_occupant_back_and_leaves_no_superseded_name() {
        let home = tempfile::TempDir::new().expect("a temporary directory");
        let destination = home.path().join("kontor.db");
        let original = b"the database that was already here";
        std::fs::write(&destination, original).expect("the occupant exists");

        let unpublishable = home.path().join("not-a-file");
        std::fs::create_dir(&unpublishable).expect("a directory stands in for a copy");
        let aside = home.path().join("kontor.db.superseded-20260810T120000Z");

        let refused = displace_and_publish(&unpublishable, &destination, &aside)
            .expect_err("a directory cannot be published as a database");
        assert!(
            matches!(refused, BackupError::Io { .. }),
            "the refusal is the link failure, not something else: {refused:?}"
        );

        assert_eq!(
            std::fs::read(&destination).expect("the occupant is still there"),
            original,
            "a failed publish must not displace the destination"
        );
        assert!(
            !aside.exists(),
            "a failed publish must not leave a superseded name behind"
        );
    }

    /// The superseded name is claimed atomically, so a name that is already
    /// taken is refused rather than replaced — and the refusal changes nothing.
    #[test]
    fn a_taken_superseded_name_is_refused_and_nothing_moves() {
        let home = tempfile::TempDir::new().expect("a temporary directory");
        let destination = home.path().join("kontor.db");
        let occupant = b"the database that was already here";
        std::fs::write(&destination, occupant).expect("the occupant exists");
        let copy = home.path().join("kontor.db.restore.partial");
        std::fs::write(&copy, b"the database being restored").expect("the copy exists");

        let aside = home.path().join("kontor.db.superseded-20260810T120000Z");
        let earlier = b"a database an earlier restore set aside";
        std::fs::write(&aside, earlier).expect("the superseded name is already taken");

        let refused = displace_and_publish(&copy, &destination, &aside)
            .expect_err("a taken superseded name is never overwritten");
        match refused {
            BackupError::AlreadyExists { path } => assert_eq!(path, aside),
            other => panic!("the refusal must name the collision: {other:?}"),
        }
        assert_eq!(
            std::fs::read(&aside).expect("the earlier file is still there"),
            earlier,
            "an earlier restore's evidence must not be overwritten"
        );
        assert_eq!(
            std::fs::read(&destination).expect("the occupant is still there"),
            occupant
        );
    }
}
