//! The document that says what a snapshot file is.
//!
//! A snapshot without a manifest is a SQLite file of unknown provenance: it
//! carries a Realm id inside it, but nothing states which Realm it was *taken
//! for*, how long it was when it was verified, or what its bytes hashed to.
//! The manifest is that statement, written after the copy is verified and
//! renamed into place in the same operation as the copy itself.
//!
//! It is deliberately small and boring: five facts and a format version, in a
//! plain JSON document. Every one of them is checked before a restore touches a
//! destination.

use std::path::Path;

use kontor_core::id::{ContentHash, RealmId, Timestamp, format_utc_timestamp};
use serde::{Deserialize, Serialize};

use crate::backup::{BackupError, io};

/// The manifest generation this build writes and is willing to read.
pub const MANIFEST_FORMAT_VERSION: u32 = 1;

/// What a snapshot file is, and which Realm it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// The manifest generation. A later one is refused rather than misread.
    pub format_version: u32,
    /// The Realm whose database this is.
    pub realm_id: RealmId,
    /// The `user_version` the copied database carries.
    pub database_schema_version: i64,
    /// When the snapshot was taken.
    pub created_at: Timestamp,
    /// How long the verified snapshot file is, in bytes.
    pub byte_length: u64,
    /// SHA-256 of the whole snapshot file.
    pub content_hash: ContentHash,
}

impl SnapshotManifest {
    /// The manifest path that belongs to a snapshot path.
    #[must_use]
    pub fn path_for(snapshot: &Path) -> std::path::PathBuf {
        let mut name = snapshot.as_os_str().to_os_string();
        name.push(".manifest.json");
        std::path::PathBuf::from(name)
    }

    /// Serialize to the bytes that are written to disk.
    ///
    /// # Errors
    /// Returns [`BackupError::Manifest`] when the document cannot be rendered,
    /// which can only happen if the type stops being serializable.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BackupError> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(|_| BackupError::Manifest {
            detail: "the manifest could not be rendered",
        })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Read and parse a manifest.
    ///
    /// # Errors
    /// Returns [`BackupError::Io`] when the file cannot be read and
    /// [`BackupError::Manifest`] when it is not a manifest this build reads —
    /// including a future `format_version`, which is refused rather than
    /// interpreted.
    pub fn read(path: &Path) -> Result<Self, BackupError> {
        let bytes = std::fs::read(path).map_err(io("read"))?;
        let manifest: Self = serde_json::from_slice(&bytes).map_err(|_| BackupError::Manifest {
            detail: "the manifest is not a document this build wrote",
        })?;
        if manifest.format_version != MANIFEST_FORMAT_VERSION {
            return Err(BackupError::Manifest {
                detail: "the manifest declares a format version this build does not read",
            });
        }
        Ok(manifest)
    }

    /// Prove that `snapshot` is the file this manifest describes.
    ///
    /// Length first, then the digest: a truncated file is the common corruption
    /// and naming it as such is more useful than "the hash differs".
    ///
    /// # Errors
    /// Returns [`BackupError::Io`] when the file cannot be read and
    /// [`BackupError::Verification`] when it is not the described file.
    pub fn verify_file(&self, snapshot: &Path) -> Result<(), BackupError> {
        let bytes = std::fs::read(snapshot).map_err(io("read"))?;
        if bytes.len() as u64 != self.byte_length {
            return Err(BackupError::Verification {
                detail: "the snapshot file's length does not match its manifest",
            });
        }
        if ContentHash::of(&bytes) != self.content_hash {
            return Err(BackupError::Verification {
                detail: "the snapshot file's digest does not match its manifest",
            });
        }
        Ok(())
    }

    /// Describe an already-verified snapshot file.
    ///
    /// # Errors
    /// Returns [`BackupError::Io`] when the file cannot be read.
    pub fn describe(
        snapshot: &Path,
        realm_id: RealmId,
        database_schema_version: i64,
        created_at: Timestamp,
    ) -> Result<Self, BackupError> {
        let bytes = std::fs::read(snapshot).map_err(io("read"))?;
        Ok(Self {
            format_version: MANIFEST_FORMAT_VERSION,
            realm_id,
            database_schema_version,
            created_at,
            byte_length: bytes.len() as u64,
            content_hash: ContentHash::of(&bytes),
        })
    }

    /// The canonical, sortable stem a snapshot of this Realm is named with.
    ///
    /// Realm-qualified on purpose: a directory shared by two Realms — which
    /// happens the first time somebody points two backups at one network share
    /// — must not let one Realm's retention see the other's files.
    #[must_use]
    pub fn file_stem(realm_id: RealmId, created_at: Timestamp) -> String {
        let instant: String = format_utc_timestamp(created_at)
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        format!("kontor-{realm_id}-{instant}")
    }

    /// Recover the Realm a snapshot file name claims, without opening it.
    ///
    /// Retention uses this to ignore another Realm's files cheaply; it is never
    /// the basis for keeping or deleting anything on its own, because a name is
    /// not evidence. The manifest beside the file is.
    #[must_use]
    pub fn realm_in_name(name: &str) -> Option<RealmId> {
        let rest = name.strip_prefix("kontor-")?;
        // A UUID is 36 characters and the separator that follows it is ours.
        let (candidate, _) = rest.split_at_checked(36)?;
        RealmId::parse(candidate).ok()
    }
}
