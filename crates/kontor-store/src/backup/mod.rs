//! Snapshots, restore, redacted export and redacted import.
//!
//! Four operations live here, and the boundary between them is the point of the
//! module:
//!
//! * [`snapshot`] copies one Realm's database with `VACUUM INTO` while the
//!   daemon keeps writing, verifies the copy before publishing it, and writes a
//!   manifest that names the Realm the bytes belong to.
//! * [`restore`] puts a verified snapshot back — offline, and only into an
//!   uninitialized state root or the *same* Realm. There is no flag that widens
//!   that.
//! * [`export`] writes a versioned, redacted, byte-deterministic JSON document
//!   of the control-plane state that may leave the machine.
//! * [`import`] takes such a document into a *different*, separately
//!   initialized Realm, where every source id is a reference and never an
//!   authority: it records new destination receipts and never replays a source
//!   command, transition or dispatch receipt as an executable one.
//!
//! # The rule every path here shares
//!
//! Nothing is published until it has been verified, and nothing valid is
//! removed until its replacement is published. A snapshot is written to a
//! `.partial` path, opened read-only, integrity-checked, matched against its
//! Realm and only then renamed into place — and retention prunes only after
//! that rename succeeded. So an interrupted, corrupt or foreign snapshot costs
//! a disk write and nothing else: the previous backups are still there, and the
//! live database was never touched.

pub mod export;
mod import;
mod manifest;
mod restore;
mod retention;
mod snapshot;

use std::path::PathBuf;

use kontor_core::DomainError;
use kontor_core::id::RealmId;

pub use export::{
    ContinuitySummary, EXPORT_SCHEMA_VERSION, ExportedRecords, KontorExportV1, RecordCounts,
    RecordLineage, RedactionSummary, export_realm,
};
pub use import::{ImportPlan, ImportReceiptRow, ImportReport, ImportedRecordRow, import_export};
pub use manifest::{MANIFEST_FORMAT_VERSION, SnapshotManifest};
pub use restore::{RestorePlan, restore_snapshot};
pub use retention::{RETAINED_SNAPSHOTS, RetainedSnapshot, list_snapshots, prune_snapshots};
pub use snapshot::{SnapshotOutcome, create_snapshot};

/// Everything backup, restore, export and import can refuse.
///
/// Every variant carries a category and a path or an opaque id — never a row
/// value, a credential, a token or a fragment of an exported document. A
/// refusal is meant to be logged.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BackupError {
    /// The source database, or a copy of it, failed its own integrity check.
    ///
    /// This is the fail-closed case: nothing is replaced, nothing is renamed
    /// and nothing is pruned when it is raised.
    #[error("the database failed verification: {detail}")]
    Verification {
        /// Which check refused, never what it found in a row.
        detail: &'static str,
    },
    /// A filesystem operation failed.
    #[error("the backup file could not be {action}: {source}")]
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// A path that must not exist already does.
    ///
    /// Snapshots are never overwritten: a colliding name is a refusal, not a
    /// reason to replace evidence.
    #[error("{path} already exists and a backup never overwrites one")]
    AlreadyExists {
        /// The colliding path.
        path: PathBuf,
    },
    /// A snapshot, manifest or export belongs to a different Realm.
    #[error("this file belongs to realm {found}, not to realm {expected}")]
    CrossRealm {
        /// The Realm the file names.
        found: RealmId,
        /// The Realm the operation is for.
        expected: RealmId,
    },
    /// An export of this Realm was offered to the import path.
    ///
    /// That operation exists and is called restore. Importing your own export
    /// would mint a second, source-referenced copy of this Realm's lineage and
    /// make every id in it ambiguous.
    #[error("realm {realm_id} restores its own export; it never imports it")]
    SameRealmImport {
        /// The Realm that is both source and destination.
        realm_id: RealmId,
    },
    /// The destination is an initialized Realm and the operation would have
    /// overwritten it.
    #[error(
        "the destination is initialized as realm {found} and a raw restore never overwrites another realm"
    )]
    DestinationInitialized {
        /// The Realm already living in the destination.
        found: RealmId,
    },
    /// The manifest is missing, malformed, or does not describe the file beside
    /// it.
    #[error("the snapshot manifest is not one this build wrote: {detail}")]
    Manifest {
        /// Why it was refused.
        detail: &'static str,
    },
    /// The document declares a schema version this build does not read.
    #[error("export schema version {found} is not one this build reads ({expected})")]
    UnsupportedExportVersion {
        /// The version found in the document.
        found: u32,
        /// The version this build implements.
        expected: u32,
    },
    /// A canary matched: the document about to be published carries material
    /// that must never leave the Realm.
    ///
    /// The offending value is never echoed — only the structural path of the
    /// node it was found at, exactly as the domain's own scanner reports it.
    #[error("the document carries material that may not be exported, at {path}")]
    Redaction {
        /// The structural path of the offending node.
        path: String,
    },
    /// The store refused.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
    /// The domain refused.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// A repository rule refused.
    #[error(transparent)]
    Repository(#[from] kontor_core::repository::RepositoryError),
}

impl BackupError {
    /// The stable, loggable category of this refusal.
    ///
    /// A structured log line gets this, an opaque id and nothing else. The
    /// `Display` text is for an operator's terminal, and even that never
    /// carries a stored value.
    #[must_use]
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Verification { .. } => "verification",
            Self::Io { .. } => "io",
            Self::AlreadyExists { .. } => "already_exists",
            Self::CrossRealm { .. } => "cross_realm",
            Self::SameRealmImport { .. } => "same_realm_import",
            Self::DestinationInitialized { .. } => "destination_initialized",
            Self::Manifest { .. } => "manifest",
            Self::UnsupportedExportVersion { .. } => "unsupported_export_version",
            Self::Redaction { .. } => "redaction",
            Self::Store(_) => "store",
            Self::Domain(_) => "domain",
            Self::Repository(_) => "repository",
        }
    }
}

/// Map an I/O failure onto its action without repeating the closure everywhere.
pub(crate) fn io(action: &'static str) -> impl Fn(std::io::Error) -> BackupError {
    move |source| BackupError::Io { action, source }
}

/// Flush a directory entry so a rename that has already returned is durable.
///
/// Renaming publishes the name; only an `fsync` of the *directory* makes the
/// name survive a power cut. On platforms where a directory cannot be opened
/// for this, the call is skipped rather than failed — the data file itself was
/// already synced, so the worst case is a lost name, not a corrupt snapshot.
pub(crate) fn sync_directory(directory: &std::path::Path) {
    if let Ok(handle) = std::fs::File::open(directory) {
        let _ = handle.sync_all();
    }
}
