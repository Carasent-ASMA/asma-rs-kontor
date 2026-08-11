//! One daemon per state root, enforced by the filesystem.
//!
//! The lock is taken on a file *inside* the state root, so the boundary the lock
//! defends is the same boundary the Realm is: two daemons on one root collide, and
//! two daemons on different roots are two Realms that never see each other. There
//! is no port check, no PID file and no registry — those all answer a different
//! question than "who owns this database".

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs4::{FileExt, TryLockError};

/// The lock file's name inside a state root.
pub const LOCK_FILE: &str = "kontor.lock";

/// Why a state root could not be claimed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    /// The lock file could not be created or opened.
    #[error("the state root's lock file could not be opened: {source}")]
    Open {
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// Another daemon already holds this state root.
    #[error("another kontor daemon already holds this state root")]
    Held,
}

/// An exclusive claim on one state root, released when this value is dropped.
///
/// The handle is kept rather than discarded on purpose: an advisory lock lives as
/// long as the descriptor that holds it, so dropping the file would silently
/// release the claim while the daemon carried on believing it was alone.
#[derive(Debug)]
pub struct StateRootLock {
    path: PathBuf,
    handle: File,
}

impl StateRootLock {
    /// Claim `state_root` exclusively, without waiting.
    ///
    /// # Errors
    /// Returns [`LockError::Held`] when another process holds the root — the
    /// second daemon fails cleanly rather than blocking or corrupting — and
    /// [`LockError::Open`] when the file itself cannot be created.
    pub fn acquire(state_root: &Path) -> Result<Self, LockError> {
        let path = state_root.join(LOCK_FILE);
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| LockError::Open { source })?;
        match FileExt::try_lock(&handle) {
            Ok(()) => Ok(Self { path, handle }),
            // Already held. The second daemon fails here, immediately and
            // cleanly, rather than waiting for a lock it must not get.
            Err(TryLockError::WouldBlock) => Err(LockError::Held),
            Err(TryLockError::Error(source)) => Err(LockError::Open { source }),
        }
    }

    /// The lock file this claim is held on.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StateRootLock {
    fn drop(&mut self) {
        // A failed release is not actionable: the descriptor is closing anyway,
        // which releases the advisory lock regardless.
        let _ = FileExt::unlock(&self.handle);
    }
}
