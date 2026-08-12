//! Keeping the newest verified snapshots of one Realm, and nothing else.
//!
//! Retention is the operation most likely to turn a small problem into an
//! unrecoverable one, so it is written to be timid:
//!
//! * it only ever looks at files whose *name* is this Realm's and whose
//!   *manifest* says the same — a partial, a corrupt file, a foreign Realm's
//!   snapshot and anything a human dropped in the directory are all invisible
//!   to it, and none of them is deleted;
//! * a snapshot counts as retained only after it verified against its manifest,
//!   so an unverifiable file never displaces a good one;
//! * it never deletes the newest verified snapshot, whatever `keep` says;
//! * it is called *after* a new snapshot is published, never before.
//!
//! What it deletes is therefore always: a verified snapshot of this Realm, that
//! is older than at least [`RETAINED_SNAPSHOTS`] other verified snapshots of
//! this Realm.

use std::path::{Path, PathBuf};

use kontor_core::id::RealmId;

use crate::backup::manifest::SnapshotManifest;
use crate::backup::{BackupError, io, sync_directory};

/// How many verified snapshots per Realm are kept.
pub const RETAINED_SNAPSHOTS: usize = 7;

/// One published, verified snapshot of one Realm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedSnapshot {
    /// The snapshot file.
    pub snapshot: PathBuf,
    /// Its manifest.
    pub manifest: SnapshotManifest,
}

/// Every verified snapshot of `realm_id` in `directory`, newest first.
///
/// A file that cannot be read, has no manifest, has a manifest this build does
/// not understand, does not match its manifest's length and digest, or belongs
/// to another Realm is skipped in silence: this is a listing, not a validator,
/// and the caller must not be able to turn "unreadable" into "deletable".
///
/// # Errors
/// Returns [`BackupError::Io`] only when the directory itself cannot be read.
pub fn list_snapshots(
    directory: &Path,
    realm_id: RealmId,
) -> Result<Vec<RetainedSnapshot>, BackupError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io("read")(error)),
    };

    let mut retained = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // Only published snapshots are candidates. `.partial` never matches, and
        // neither does a manifest, a lock file or anything else in the way.
        if !name.ends_with(".db") || SnapshotManifest::realm_in_name(name) != Some(realm_id) {
            continue;
        }
        let Ok(manifest) = SnapshotManifest::read(&SnapshotManifest::path_for(&path)) else {
            continue;
        };
        if manifest.realm_id != realm_id || manifest.verify_file(&path).is_err() {
            continue;
        }
        retained.push(RetainedSnapshot {
            snapshot: path,
            manifest,
        });
    }

    // Newest first by the manifest's own instant, with the file name as the
    // tiebreaker so the order is total and reproducible.
    retained.sort_by(|left, right| {
        right
            .manifest
            .created_at
            .cmp(&left.manifest.created_at)
            .then_with(|| right.snapshot.cmp(&left.snapshot))
    });
    Ok(retained)
}

/// Delete this Realm's verified snapshots beyond the newest [`RETAINED_SNAPSHOTS`].
///
/// Returns the snapshots that were removed. Call it only after a new snapshot
/// has been published, never before: pruning first is how a failed backup
/// becomes a lost one.
///
/// # Errors
/// Returns [`BackupError::Io`] when the directory cannot be read or a file that
/// was selected for removal could not be removed.
pub fn prune_snapshots(directory: &Path, realm_id: RealmId) -> Result<Vec<PathBuf>, BackupError> {
    let retained = list_snapshots(directory, realm_id)?;
    // Belt and braces: `skip` already keeps the newest, and an explicit floor of
    // one means a mistake in `RETAINED_SNAPSHOTS` still cannot empty a directory.
    let keep = RETAINED_SNAPSHOTS.max(1);
    let mut removed = Vec::new();
    for stale in retained.into_iter().skip(keep) {
        std::fs::remove_file(&stale.snapshot).map_err(io("removed"))?;
        // The manifest goes second: a manifest without its snapshot is ignored
        // by every reader here, while the reverse would leave a file that looks
        // published and is not.
        let _ = std::fs::remove_file(SnapshotManifest::path_for(&stale.snapshot));
        removed.push(stale.snapshot);
    }
    if !removed.is_empty() {
        sync_directory(directory);
    }
    Ok(removed)
}
