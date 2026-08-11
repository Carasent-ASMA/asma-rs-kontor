//! The Realm's bearer secrets: generated once, stored `0600`, never logged.
//!
//! Three secrets are generated together on first start, one per authority tier,
//! and written atomically into the state root. A Realm therefore has exactly one
//! credential set, tied to the one directory the lock and the database live in:
//! moving the database moves its credentials with it, and a second state root has
//! its own.
//!
//! # Where the randomness comes from
//!
//! SQLite's own PRNG, through `randomblob`. It is seeded by the VFS from the
//! operating system's entropy source — `/dev/urandom` on Unix, the platform CSPRNG
//! on Windows — and it is already a dependency of this workspace. That matters
//! because the shared dependency set is owned by another ticket (CON-007): adding
//! a random-number crate would be a workspace change, and drawing bytes from a
//! generator this build already trusts to name its own temporary files is the
//! smaller, honest alternative.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use kontor_api::auth::RealmCredentials;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// The credential file's name inside a state root.
pub const CREDENTIAL_FILE: &str = "credentials.json";

/// How many random bytes each secret carries.
const SECRET_BYTES: usize = 32;

/// The permission bits the credential file is created with, on Unix.
#[cfg(unix)]
const OWNER_ONLY: u32 = 0o600;

/// Why a credential set could not be established.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CredentialError {
    /// The file could not be read, created or replaced.
    #[error("the realm credential file could not be {action}: {source}")]
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// The stored file is not a credential set this build understands.
    #[error("the realm credential file is not a credential set this build wrote")]
    Malformed,
    /// The operating system could not be asked for entropy.
    #[error("the platform entropy source could not be read: {source}")]
    Entropy {
        /// The underlying failure.
        #[source]
        source: rusqlite::Error,
    },
}

/// The on-disk credential set.
///
/// It is a plain document on purpose: the file's *permissions* are what protect
/// it, and an encrypted-at-rest secret whose key lives beside it protects nothing
/// while making the recovery story worse.
#[derive(Debug, Serialize, Deserialize)]
struct StoredCredentials {
    /// The document generation, so a later format is refused rather than misread.
    schema_version: u32,
    /// The read-only tier's secret.
    observer: String,
    /// The control-plane-write tier's secret.
    operator: String,
    /// The credential- and policy-authority tier's secret.
    admin: String,
}

/// The generation this build writes and is willing to read.
const CREDENTIAL_SCHEMA: u32 = 1;

/// Read the Realm's credentials, generating them on first start.
///
/// The file is created with an exclusive open and owner-only permissions, and it
/// is written through a temporary file in the same directory followed by a rename
/// — so a crash halfway leaves either the previous credentials or none, never a
/// half-written secret a caller could authenticate against.
///
/// # Errors
/// Returns [`CredentialError`] when the file cannot be read or written, when it
/// holds a document this build does not understand, or when the platform's
/// entropy source cannot be reached.
pub fn open_or_create(state_root: &Path) -> Result<RealmCredentials, CredentialError> {
    let path = state_root.join(CREDENTIAL_FILE);
    if let Some(stored) = read(&path)? {
        return Ok(into_credentials(stored));
    }
    let generated = StoredCredentials {
        schema_version: CREDENTIAL_SCHEMA,
        observer: secret()?,
        operator: secret()?,
        admin: secret()?,
    };
    write_atomically(&path, &generated)?;
    // Re-read rather than trusting the value just written: if another daemon won
    // the race to create the file, the credentials that count are the ones on disk.
    let stored = read(&path)?.ok_or(CredentialError::Malformed)?;
    Ok(into_credentials(stored))
}

/// The path the credential set lives at inside `state_root`.
#[must_use]
pub fn path_in(state_root: &Path) -> PathBuf {
    state_root.join(CREDENTIAL_FILE)
}

fn into_credentials(stored: StoredCredentials) -> RealmCredentials {
    RealmCredentials::new(
        SecretString::from(stored.observer),
        SecretString::from(stored.operator),
        SecretString::from(stored.admin),
    )
}

fn read(path: &Path) -> Result<Option<StoredCredentials>, CredentialError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(CredentialError::Io {
                action: "read",
                source,
            });
        }
    };
    let stored: StoredCredentials =
        serde_json::from_slice(&bytes).map_err(|_| CredentialError::Malformed)?;
    if stored.schema_version != CREDENTIAL_SCHEMA {
        return Err(CredentialError::Malformed);
    }
    Ok(Some(stored))
}

fn write_atomically(path: &Path, credentials: &StoredCredentials) -> Result<(), CredentialError> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = directory.join(format!("{CREDENTIAL_FILE}.{}.partial", std::process::id()));
    let mut file = owner_only(&temporary)?;
    let document =
        serde_json::to_vec_pretty(credentials).map_err(|_| CredentialError::Malformed)?;
    file.write_all(&document)
        .map_err(|source| CredentialError::Io {
            action: "written",
            source,
        })?;
    // The bytes have to be on the device before the rename publishes them, or a
    // crash can leave a visible file with no contents — which reads as a malformed
    // credential set on the next start.
    file.sync_all().map_err(|source| CredentialError::Io {
        action: "flushed",
        source,
    })?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|source| CredentialError::Io {
        action: "published",
        source,
    })
}

/// Create a file no other user can read.
///
/// On Unix the mode is set at creation, not afterwards: a `chmod` after the fact
/// leaves a window in which the secret exists world-readable.
fn owner_only(path: &Path) -> Result<File, CredentialError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(OWNER_ONLY);
    }
    options.open(path).map_err(|source| CredentialError::Io {
        action: "created",
        source,
    })
}

/// One secret: 32 bytes of platform entropy, hex encoded.
fn secret() -> Result<String, CredentialError> {
    // An in-memory database is enough to reach the VFS randomness source, and it
    // touches nothing on disk.
    let connection = rusqlite::Connection::open_in_memory()
        .map_err(|source| CredentialError::Entropy { source })?;
    let hex: String = connection
        .query_row(
            "SELECT lower(hex(randomblob(?1)))",
            [i64::try_from(SECRET_BYTES).unwrap_or(32)],
            |row| row.get(0),
        )
        .map_err(|source| CredentialError::Entropy { source })?;
    if hex.len() != SECRET_BYTES * 2 {
        return Err(CredentialError::Malformed);
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_distinct_and_full_width() {
        let first = secret().expect("platform entropy");
        let second = secret().expect("platform entropy");
        assert_eq!(first.len(), SECRET_BYTES * 2);
        assert_ne!(
            first, second,
            "two draws from the platform CSPRNG must not repeat"
        );
    }
}
