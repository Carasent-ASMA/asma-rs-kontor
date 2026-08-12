//! Taking one consistent copy of a live database, and proving it before
//! publishing it.
//!
//! `VACUUM INTO` is the whole copying mechanism. It runs on its own connection,
//! inside SQLite's own read transaction, so the copy is a transactionally
//! consistent point in time even though the daemon keeps writing to the WAL
//! throughout — and it is a *defragmented* copy rather than a byte image, so a
//! snapshot never carries the free pages of the database it came from.
//!
//! There is deliberately no second backup abstraction beside it. `rusqlite`'s
//! online-backup API would copy pages incrementally and hand us the job of
//! deciding what a restart mid-copy means; the MVP plan asks for `VACUUM INTO`,
//! and one statement that either produces a whole consistent file or fails is
//! the smaller thing to reason about.
//!
//! # The order, and why it is this order
//!
//! ```text
//! integrity_check the source        (a damaged source is never copied)
//!   → VACUUM INTO a unique .partial (a name nothing else can be using)
//!     → open the copy read-only, verify it end to end
//!       → fsync the copy, write and fsync the manifest
//!         → rename both into place, fsync the directory
//! ```
//!
//! Every failure before the rename leaves the snapshot directory exactly as it
//! was apart from the partial file, which is removed. Nothing existing is
//! replaced at any point, and retention is a separate call that only ever runs
//! after this one returned success.

use std::path::{Path, PathBuf};

use kontor_core::id::{RealmId, Timestamp};
use rusqlite::{Connection, OpenFlags};

use crate::backup::manifest::SnapshotManifest;
use crate::backup::{BackupError, io, link_exclusively, sync_directory};
use crate::migrations::SCHEMA_VERSION;

/// How long a snapshot waits for a database somebody else is checkpointing.
///
/// Longer than the store's own connection budget, because this operation is not
/// on a request path: nobody is waiting on it, and giving up early only means
/// there is no backup.
const BACKUP_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Distinguishes two snapshots taken by one process at the same instant.
///
/// The published name is a function of the Realm and the instant, so two callers
/// racing for it is a real case; their *partials* must still be different files.
static PUBLISH_ATTEMPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What one successful snapshot produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotOutcome {
    /// The published snapshot file.
    pub snapshot: PathBuf,
    /// The published manifest beside it.
    pub manifest_path: PathBuf,
    /// The manifest's contents.
    pub manifest: SnapshotManifest,
}

/// Copy a live database into `directory`, verified, with its manifest.
///
/// The daemon may keep writing throughout. The copy is the state as of the
/// moment `VACUUM INTO` took its read transaction, which is a point every
/// committed transaction is either wholly inside or wholly outside.
///
/// # Errors
/// Returns [`BackupError::Verification`] when the source or the copy fails an
/// integrity check, does not carry exactly one Realm row, or does not carry the
/// schema version this build wrote; [`BackupError::AlreadyExists`] when a
/// snapshot of that name is already published; and [`BackupError::Io`] for
/// filesystem failures. In every case the snapshot directory keeps every file
/// it already had.
pub fn create_snapshot(
    database: &Path,
    directory: &Path,
    now: Timestamp,
) -> Result<SnapshotOutcome, BackupError> {
    std::fs::create_dir_all(directory).map_err(io("created"))?;

    // The source is judged first. A database that fails its own integrity check
    // is never copied: a verified backup of a corrupt file is a lie that
    // survives the file it came from.
    let source =
        Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|_| {
            BackupError::Verification {
                detail: "the source database could not be opened",
            }
        })?;
    // The daemon is still writing, and a WAL checkpoint briefly excludes
    // readers. A bounded wait is the difference between "the realm was busy for
    // a moment" and a backup that failed for no reason worth reporting.
    source
        .busy_timeout(BACKUP_BUSY_TIMEOUT)
        .map_err(|_| BackupError::Verification {
            detail: "the source database would not accept a busy timeout",
        })?;
    let identity = verify_open_database(&source)?;

    let stem = SnapshotManifest::file_stem(identity.realm_id, now);
    let snapshot = directory.join(format!("{stem}.db"));
    let manifest_path = SnapshotManifest::path_for(&snapshot);
    // A cheap early refusal so an obvious collision does not cost a whole copy.
    // It is *not* what makes publication safe — a check followed by a rename is a
    // race, and the winner of that race would silently replace a published
    // snapshot. `link_exclusively` below is the guarantee; this is the courtesy.
    for published in [&snapshot, &manifest_path] {
        if published.exists() {
            return Err(BackupError::AlreadyExists {
                path: published.clone(),
            });
        }
    }

    // The partial name is unique per attempt, not just per process: two threads
    // of one process take snapshots of the same Realm at the same instant in
    // exactly the situation this is for, and a shared name would turn that into
    // a collision on the *partial* instead of an honest race for the published
    // name. `VACUUM INTO` also refuses a target that already exists.
    let partial = directory.join(format!(
        "{stem}.{}.{}.partial",
        std::process::id(),
        PUBLISH_ATTEMPT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if partial.exists() {
        return Err(BackupError::AlreadyExists { path: partial });
    }

    let copied = source.execute("VACUUM INTO ?1", [path_argument(&partial)?]);
    if let Err(error) = copied {
        // `VACUUM INTO` may have created and then abandoned the target.
        remove_partial(&partial);
        return Err(BackupError::Verification {
            detail: match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::DatabaseCorrupt =>
                {
                    "the source database is corrupt and was not copied"
                }
                _ => "the snapshot copy could not be written",
            },
        });
    }
    drop(source);

    // Everything from here is about the *copy*, and any refusal removes only the
    // partial this call created.
    let published = publish(&partial, &snapshot, &manifest_path, &identity, now);
    if published.is_err() {
        remove_partial(&partial);
    }
    published.map(|manifest| SnapshotOutcome {
        snapshot,
        manifest_path,
        manifest,
    })
}

/// Verify the copy, write its manifest and rename both into place.
fn publish(
    partial: &Path,
    snapshot: &Path,
    manifest_path: &Path,
    identity: &DatabaseIdentity,
    now: Timestamp,
) -> Result<SnapshotManifest, BackupError> {
    let copied = verify_database_file(partial)?;
    if copied.realm_id != identity.realm_id {
        return Err(BackupError::CrossRealm {
            found: copied.realm_id,
            expected: identity.realm_id,
        });
    }
    if copied.schema_version != identity.schema_version {
        return Err(BackupError::Verification {
            detail: "the snapshot copy does not carry the source's schema version",
        });
    }

    sync_file(partial)?;
    let manifest =
        SnapshotManifest::describe(partial, identity.realm_id, identity.schema_version, now)?;

    let manifest_partial = SnapshotManifest::path_for(partial);
    std::fs::write(&manifest_partial, manifest.to_bytes()?).map_err(io("written"))?;
    if let Err(error) = sync_file(&manifest_partial) {
        remove_partial(&manifest_partial);
        return Err(error);
    }

    // The database is published first: a snapshot with no manifest yet is
    // visibly incomplete — every reader here skips it — while a manifest with no
    // snapshot claims a file that is not there.
    if let Err(error) = link_exclusively(partial, snapshot) {
        remove_partial(&manifest_partial);
        return Err(error);
    }
    if let Err(error) = link_exclusively(&manifest_partial, manifest_path) {
        // The database link is one this call created, exclusively, a moment ago,
        // so removing it takes back only our own publication and can never
        // remove somebody else's snapshot.
        remove_partial(snapshot);
        remove_partial(&manifest_partial);
        return Err(error);
    }
    // Both names now exist; the partials are the second link to the same data.
    remove_partial(partial);
    remove_partial(&manifest_partial);
    if let Some(directory) = snapshot.parent() {
        sync_directory(directory);
    }
    Ok(manifest)
}

/// The two facts that make a database file *this* Realm's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DatabaseIdentity {
    /// The Realm the single `realm_metadata` row names.
    pub(crate) realm_id: RealmId,
    /// The `user_version` the file carries.
    pub(crate) schema_version: i64,
}

/// Open a database file read-only and prove it is a whole Kontor database.
///
/// Read-only on purpose: verifying a file must not migrate it, replay a WAL
/// into it or otherwise change the bytes that are about to be hashed.
///
/// # Errors
/// Returns [`BackupError::Verification`] when the file cannot be opened, is not
/// a database, fails `integrity_check`, does not carry exactly one Realm row, or
/// carries a schema version this build did not write.
pub(crate) fn verify_database_file(path: &Path) -> Result<DatabaseIdentity, BackupError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| BackupError::Verification {
        detail: "the snapshot could not be opened as a database",
    })?;
    verify_open_database(&connection)
}

/// The half of the verification that works on an already-open connection.
fn verify_open_database(connection: &Connection) -> Result<DatabaseIdentity, BackupError> {
    let report: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| BackupError::Verification {
            detail: "the database could not be integrity-checked",
        })?;
    if report != "ok" {
        // SQLite's report names pages and sometimes row values; only the fact of
        // the failure crosses this boundary.
        return Err(BackupError::Verification {
            detail: "the database failed its integrity check",
        });
    }

    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|_| BackupError::Verification {
            detail: "the database's schema version could not be read",
        })?;
    if schema_version != SCHEMA_VERSION {
        return Err(BackupError::Verification {
            detail: "the database does not carry the schema version this build writes",
        });
    }

    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .map_err(|_| BackupError::Verification {
            detail: "the database has no readable realm identity",
        })?;
    if rows != 1 {
        return Err(BackupError::Verification {
            detail: "the database does not hold exactly one realm row",
        });
    }
    let realm_id: String = connection
        .query_row(
            "SELECT realm_id FROM realm_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|_| BackupError::Verification {
            detail: "the database's realm identity could not be read",
        })?;
    let realm_id = RealmId::parse(&realm_id).map_err(|_| BackupError::Verification {
        detail: "the database's realm identity is not a canonical realm id",
    })?;
    Ok(DatabaseIdentity {
        realm_id,
        schema_version,
    })
}

/// Render a path for SQLite, which takes text and not bytes.
fn path_argument(path: &Path) -> Result<String, BackupError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or(BackupError::Verification {
            detail: "the snapshot path is not valid UTF-8",
        })
}

/// Flush one file's contents to the device.
pub(crate) fn sync_file(path: &Path) -> Result<(), BackupError> {
    let handle = std::fs::File::open(path).map_err(io("read"))?;
    handle.sync_all().map_err(io("flushed"))
}

/// Remove a file this call created, and only that file.
fn remove_partial(path: &Path) {
    let _ = std::fs::remove_file(path);
}
