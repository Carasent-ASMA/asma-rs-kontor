//! Atomic installation of one artifact set into an injected install root.
//!
//! The transaction is journaled so an interruption at any boundary has exactly
//! one deterministic reading: either the previous four artifacts are whole, or
//! the new four are. Nothing is ever left in a mixed state.
//!
//! Layout under the install root:
//!
//! ```text
//! <root>/kontor, kontord, kontor-mcp, kontor-bootstrap   the artifacts
//! <root>/.kontor-bootstrap/lock                          exclusive holder
//! <root>/.kontor-bootstrap/journal.json                  the transaction
//! <root>/.kontor-bootstrap/staging-<nonce>/              verified new bytes
//! <root>/.kontor-bootstrap/rollback-<nonce>/             displaced old bytes
//! ```

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};

use crate::artifact::manifest::{
    ArtifactManifest, ArtifactName, ManifestError, VerifiedSource, sha256_hex,
};
use crate::fault::{FailPoint, FaultInjector};

const INTERNAL_DIR: &str = ".kontor-bootstrap";
const JOURNAL_FILE: &str = "journal.json";
const LOCK_FILE: &str = "lock";
const JOURNAL_SCHEMA_VERSION: u32 = 1;

/// What one install or update did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// No artifact was present before; all four are now.
    Installed,
    /// An existing installation was replaced by this release.
    Updated,
    /// Every artifact already carried this release's digest; nothing was
    /// written.
    AlreadyCurrent,
}

/// What one recovery found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// No journal was present (or it had nothing left to do).
    Nothing,
    /// An interrupted transaction was completed so all four artifacts are the
    /// new set.
    Completed,
    /// An interrupted transaction could not be completed and was unwound so
    /// all four artifacts are the previous set.
    RolledBack,
}

/// One artifact's current digest in an install root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactStatus {
    /// Which artifact.
    pub name: ArtifactName,
    /// Lowercase hex SHA-256 of the installed bytes, or `None` when absent.
    pub sha256: Option<String>,
}

impl ArtifactStatus {
    /// Whether this artifact already carries the manifest's digest.
    #[must_use]
    pub fn is_current(&self, manifest: &ArtifactManifest) -> bool {
        manifest
            .entry(self.name)
            .is_some_and(|entry| self.sha256.as_deref() == Some(entry.sha256.as_str()))
    }
}

/// Why an install, recovery or readback was refused.
///
/// Every variant is path-free: an error can be recorded in a redacted receipt
/// without leaking a host location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    /// The manifest or source set was refused.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// The install root must be an absolute path.
    #[error("the install root must be an absolute path")]
    RelativeRoot,
    /// The install root itself (or its internal state directory) is a symlink.
    #[error("the install root may not be a symlink")]
    SymlinkRoot,
    /// Another bootstrap holds the install root.
    #[error("another bootstrap holds this install root")]
    Locked,
    /// The journal exists but is not a supported document.
    #[error("the install journal is not a supported document")]
    JournalUnsupported,
    /// An installed artifact is a symlink and is never written through.
    #[error("artifact {name} is a symlink and is refused")]
    SymlinkTarget {
        /// The refused artifact.
        name: ArtifactName,
    },
    /// An installed artifact is neither a regular file nor absent.
    #[error("artifact {name} cannot be read as a regular file")]
    Unreadable {
        /// The unreadable artifact.
        name: ArtifactName,
    },
    /// An artifact changed while the transaction was running.
    #[error("artifact {name} changed while bootstrap was running")]
    ConcurrentChange {
        /// The artifact that moved.
        name: ArtifactName,
    },
    /// A filesystem operation failed.
    #[error("the install root cannot be written")]
    Io,
    /// A fault injector aborted the transaction at `point`.
    #[error("the transaction was interrupted at {point:?}")]
    Injected {
        /// Where the transaction stopped.
        point: FailPoint,
    },
}

/// The exclusive holder of one install root.
struct InstallLock {
    _file: File,
}

/// One install root this process may transact on.
#[derive(Debug, Clone)]
pub struct Installer {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    nonce: String,
    state: JournalState,
    manifest: ArtifactManifest,
    previous: Vec<PreviousArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalState {
    Prepared,
    Staged,
    Finalized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousArtifact {
    name: ArtifactName,
    sha256: Option<String>,
}

impl Installer {
    /// An installer for one explicit root.
    ///
    /// The root must be absolute; relative paths are refused so a caller can
    /// never reach the process working directory by omission.
    ///
    /// # Errors
    /// [`InstallError::RelativeRoot`] for a relative path.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, InstallError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(InstallError::RelativeRoot);
        }
        Ok(Self { root })
    }

    /// The root this installer transacts on.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read each artifact's current digest.
    ///
    /// # Errors
    /// A symlinked or unreadable artifact, or a filesystem failure.
    pub fn status(&self, manifest: &ArtifactManifest) -> Result<Vec<ArtifactStatus>, InstallError> {
        let mut statuses = Vec::with_capacity(ArtifactName::ALL.len());
        for name in ArtifactName::ALL {
            if manifest.entry(name).is_none() {
                return Err(InstallError::Manifest(ManifestError::IncompleteSet));
            }
            statuses.push(ArtifactStatus {
                name,
                sha256: self.target_digest(name)?,
            });
        }
        Ok(statuses)
    }

    /// Install, update or confirm one coherent artifact set.
    ///
    /// # Errors
    /// Every [`InstallError`]; an injected or concurrent interruption leaves a
    /// journal for [`Installer::recover`] and no partially swapped visible
    /// state beyond the journaled transaction.
    pub fn install(
        &self,
        manifest: &ArtifactManifest,
        sources: &[VerifiedSource],
        faults: &mut dyn FaultInjector,
    ) -> Result<InstallOutcome, InstallError> {
        manifest.validate()?;
        self.accept_sources(manifest, sources)?;
        self.ensure_root()?;
        faults.hit(FailPoint::BeforeLock, &self.root)?;
        let _lock = self.lock()?;
        faults.hit(FailPoint::AfterLock, &self.root)?;

        // An abandoned transaction from an earlier run is finished or unwound
        // before this one computes its own previous state.
        self.recover_locked(faults)?;
        self.remove_orphan_directories()?;

        let mut previous = Vec::with_capacity(ArtifactName::ALL.len());
        let mut already_current = true;
        for name in ArtifactName::ALL {
            let digest = self.target_digest(name)?;
            if digest.as_deref() != manifest.entry(name).map(|entry| entry.sha256.as_str()) {
                already_current = false;
            }
            previous.push(PreviousArtifact {
                name,
                sha256: digest,
            });
        }
        if already_current {
            return Ok(InstallOutcome::AlreadyCurrent);
        }
        let clean = previous.iter().all(|previous| previous.sha256.is_none());

        let nonce = nonce();
        let staging = self.internal_path(&format!("staging-{nonce}"));
        let rollback = self.internal_path(&format!("rollback-{nonce}"));
        let mut journal = Journal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            nonce,
            state: JournalState::Prepared,
            manifest: manifest.clone(),
            previous,
        };
        self.write_journal(&journal)?;

        faults.hit(FailPoint::BeforeStage, &self.root)?;
        std::fs::create_dir(&staging).map_err(|_| InstallError::Io)?;
        for source in sources {
            let bytes = std::fs::read(&source.path).map_err(|_| InstallError::Io)?;
            if sha256_hex(&bytes) != source.sha256 {
                self.rollback_locked(&journal)?;
                return Err(InstallError::Manifest(
                    ManifestError::SourceDigestMismatch { name: source.name },
                ));
            }
            let staged = staging.join(source.name.file_name());
            write_new_file(&staged, &bytes)?;
            if let Ok(metadata) = std::fs::metadata(&source.path) {
                let _ = std::fs::set_permissions(&staged, metadata.permissions());
            }
            faults.hit(FailPoint::AfterStage(source.name), &self.root)?;
        }
        journal.state = JournalState::Staged;
        self.write_journal(&journal)?;
        std::fs::create_dir(&rollback).map_err(|_| InstallError::Io)?;

        for name in ArtifactName::ALL {
            faults.hit(FailPoint::BeforeSwap(name), &self.root)?;
            let observed = self.target_digest(name)?;
            let expected = previous_digest(&journal, name);
            if observed.as_deref() != expected.as_deref() {
                self.rollback_locked(&journal)?;
                return Err(InstallError::ConcurrentChange { name });
            }
            if observed.is_some() {
                std::fs::rename(self.target_path(name), rollback.join(name.file_name()))
                    .map_err(|_| InstallError::Io)?;
            }
            std::fs::rename(staging.join(name.file_name()), self.target_path(name))
                .map_err(|_| InstallError::Io)?;
            faults.hit(FailPoint::AfterSwap(name), &self.root)?;
        }

        journal.state = JournalState::Finalized;
        self.write_journal(&journal)?;
        faults.hit(FailPoint::BeforeFinalize, &self.root)?;
        self.remove_orphan_directories()?;
        faults.hit(FailPoint::AfterFinalize, &self.root)?;
        Ok(if clean {
            InstallOutcome::Installed
        } else {
            InstallOutcome::Updated
        })
    }

    /// Finish or unwind an interrupted transaction.
    ///
    /// # Errors
    /// A journal that cannot be read or reconciled, or an injected abort.
    pub fn recover(&self, faults: &mut dyn FaultInjector) -> Result<RecoveryOutcome, InstallError> {
        self.ensure_root()?;
        faults.hit(FailPoint::BeforeLock, &self.root)?;
        let _lock = self.lock()?;
        faults.hit(FailPoint::AfterLock, &self.root)?;
        self.recover_locked(faults)
    }

    fn recover_locked(
        &self,
        faults: &mut dyn FaultInjector,
    ) -> Result<RecoveryOutcome, InstallError> {
        let Some(mut journal) = self.read_journal()? else {
            self.remove_orphan_directories()?;
            return Ok(RecoveryOutcome::Nothing);
        };
        match journal.state {
            JournalState::Prepared | JournalState::Finalized => {
                self.remove_orphan_directories()?;
                Ok(RecoveryOutcome::Nothing)
            }
            JournalState::Staged => {
                let staging = self.internal_path(&format!("staging-{}", journal.nonce));
                let rollback = self.internal_path(&format!("rollback-{}", journal.nonce));
                for name in ArtifactName::ALL {
                    faults.hit(FailPoint::BeforeSwap(name), &self.root)?;
                    let desired = journal
                        .manifest
                        .entry(name)
                        .map(|entry| entry.sha256.clone());
                    let observed = self.target_digest(name)?;
                    if observed == desired {
                        continue;
                    }
                    if observed.as_deref() != previous_digest(&journal, name).as_deref() {
                        self.rollback_locked(&journal)?;
                        return Err(InstallError::ConcurrentChange { name });
                    }
                    let staged = staging.join(name.file_name());
                    let Some(expected) = desired else {
                        self.rollback_locked(&journal)?;
                        return Ok(RecoveryOutcome::RolledBack);
                    };
                    let staged_bytes = match std::fs::read(&staged) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            self.rollback_locked(&journal)?;
                            return Ok(RecoveryOutcome::RolledBack);
                        }
                    };
                    if sha256_hex(&staged_bytes) != expected {
                        self.rollback_locked(&journal)?;
                        return Ok(RecoveryOutcome::RolledBack);
                    }
                    if observed.is_some() {
                        std::fs::create_dir_all(&rollback).map_err(|_| InstallError::Io)?;
                        std::fs::rename(self.target_path(name), rollback.join(name.file_name()))
                            .map_err(|_| InstallError::Io)?;
                    }
                    std::fs::rename(&staged, self.target_path(name))
                        .map_err(|_| InstallError::Io)?;
                    faults.hit(FailPoint::AfterSwap(name), &self.root)?;
                }
                journal.state = JournalState::Finalized;
                self.write_journal(&journal)?;
                self.remove_orphan_directories()?;
                Ok(RecoveryOutcome::Completed)
            }
        }
    }

    /// Unwind a journaled transaction to the previous whole set.
    ///
    /// An artifact a concurrent writer replaced after this transaction opened
    /// is left exactly as that writer made it: bootstrap never overwrites bytes
    /// it did not write.
    fn rollback_locked(&self, journal: &Journal) -> Result<(), InstallError> {
        let rollback = self.internal_path(&format!("rollback-{}", journal.nonce));
        for name in ArtifactName::ALL {
            let target = self.target_path(name);
            let backup = rollback.join(name.file_name());
            let previous = previous_digest(journal, name);
            if backup.exists() {
                if target.exists() {
                    std::fs::remove_file(&target).map_err(|_| InstallError::Io)?;
                }
                std::fs::rename(&backup, &target).map_err(|_| InstallError::Io)?;
                continue;
            }
            let observed = self.target_digest(name)?;
            let desired = journal
                .manifest
                .entry(name)
                .map(|entry| entry.sha256.as_str());
            if previous.is_none() && observed.as_deref() == desired {
                // The artifact was absent before this transaction; the bytes
                // this transaction swapped in are removed again.
                std::fs::remove_file(&target).map_err(|_| InstallError::Io)?;
            }
        }
        self.remove_orphan_directories()
    }

    fn accept_sources(
        &self,
        manifest: &ArtifactManifest,
        sources: &[VerifiedSource],
    ) -> Result<(), InstallError> {
        if sources.len() != ArtifactName::ALL.len() {
            return Err(InstallError::Manifest(ManifestError::IncompleteSet));
        }
        for name in ArtifactName::ALL {
            let Some(entry) = manifest.entry(name) else {
                return Err(InstallError::Manifest(ManifestError::IncompleteSet));
            };
            let Some(source) = sources.iter().find(|source| source.name == name) else {
                return Err(InstallError::Manifest(ManifestError::IncompleteSet));
            };
            if source.sha256 != entry.sha256 {
                return Err(InstallError::Manifest(
                    ManifestError::SourceDigestMismatch { name },
                ));
            }
        }
        Ok(())
    }

    fn ensure_root(&self) -> Result<(), InstallError> {
        match std::fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(InstallError::SymlinkRoot),
            Ok(metadata) if !metadata.is_dir() => Err(InstallError::Io),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(&self.root).map_err(|_| InstallError::Io)
            }
            Err(_) => Err(InstallError::Io),
        }
    }

    fn lock(&self) -> Result<InstallLock, InstallError> {
        let internal = self.internal_path("");
        match std::fs::symlink_metadata(&internal) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(InstallError::SymlinkRoot);
            }
            Ok(metadata) if !metadata.is_dir() => return Err(InstallError::Io),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&internal).map_err(|_| InstallError::Io)?;
            }
            Err(_) => return Err(InstallError::Io),
        }
        let lock_path = internal.join(LOCK_FILE);
        if std::fs::symlink_metadata(&lock_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(InstallError::SymlinkRoot);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| InstallError::Io)?;
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(InstallLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(InstallError::Locked),
            Err(TryLockError::Error(_)) => Err(InstallError::Io),
        }
    }

    fn target_path(&self, name: ArtifactName) -> PathBuf {
        self.root.join(name.file_name())
    }

    fn internal_path(&self, name: &str) -> PathBuf {
        self.root.join(INTERNAL_DIR).join(name)
    }

    fn target_digest(&self, name: ArtifactName) -> Result<Option<String>, InstallError> {
        let path = self.target_path(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(InstallError::SymlinkTarget { name })
            }
            Ok(metadata) if metadata.is_file() => {
                let bytes = std::fs::read(&path).map_err(|_| InstallError::Unreadable { name })?;
                Ok(Some(sha256_hex(&bytes)))
            }
            Ok(_) => Err(InstallError::Unreadable { name }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(InstallError::Unreadable { name }),
        }
    }

    fn read_journal(&self) -> Result<Option<Journal>, InstallError> {
        let path = self.internal_path(JOURNAL_FILE);
        let document = match std::fs::read_to_string(&path) {
            Ok(document) => document,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(InstallError::Io),
        };
        let journal = serde_json::from_str::<Journal>(&document)
            .map_err(|_| InstallError::JournalUnsupported)?;
        if journal.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(InstallError::JournalUnsupported);
        }
        Ok(Some(journal))
    }

    fn write_journal(&self, journal: &Journal) -> Result<(), InstallError> {
        let path = self.internal_path(JOURNAL_FILE);
        let bytes = serde_json::to_vec(journal).map_err(|_| InstallError::Io)?;
        let temporary = self.internal_path(&format!("{JOURNAL_FILE}.tmp-{}", journal.nonce));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| InstallError::Io)?;
            std::io::Write::write_all(&mut file, &bytes).map_err(|_| InstallError::Io)?;
            file.sync_all().map_err(|_| InstallError::Io)?;
            // A rename replaces the journal atomically on Unix; the fallback
            // keeps the same guarantee on hosts that refuse an existing
            // destination.
            std::fs::rename(&temporary, &path)
                .or_else(|_| {
                    std::fs::remove_file(&path)?;
                    std::fs::rename(&temporary, &path)
                })
                .map_err(|_| InstallError::Io)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    /// Remove staging, rollback and temporary journal files, plus the journal
    /// itself. Only ever called while the transaction is whole.
    fn remove_orphan_directories(&self) -> Result<(), InstallError> {
        let internal = self.internal_path("");
        let entries = match std::fs::read_dir(&internal) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(InstallError::Io),
        };
        for entry in entries {
            let entry = entry.map_err(|_| InstallError::Io)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let remove = name.starts_with("staging-")
                || name.starts_with("rollback-")
                || name.starts_with(&format!("{JOURNAL_FILE}.tmp-"))
                || name == JOURNAL_FILE;
            if !remove {
                continue;
            }
            let file_type = entry.file_type().map_err(|_| InstallError::Io)?;
            if file_type.is_symlink() {
                std::fs::remove_file(entry.path()).map_err(|_| InstallError::Io)?;
            } else if file_type.is_dir() {
                std::fs::remove_dir_all(entry.path()).map_err(|_| InstallError::Io)?;
            } else {
                std::fs::remove_file(entry.path()).map_err(|_| InstallError::Io)?;
            }
        }
        Ok(())
    }
}

fn previous_digest(journal: &Journal, name: ArtifactName) -> Option<String> {
    journal
        .previous
        .iter()
        .find(|previous| previous.name == name)
        .and_then(|previous| previous.sha256.clone())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| InstallError::Io)?;
    std::io::Write::write_all(&mut file, bytes).map_err(|_| InstallError::Io)?;
    file.sync_all().map_err(|_| InstallError::Io)
}

fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::artifact::manifest::{ArtifactEntry, MANIFEST_SCHEMA_VERSION, sha256_hex};
    use crate::fault::{NoFaults, ScriptedFaults};

    fn release(version: &str) -> (ArtifactManifest, tempfile::TempDir, Vec<VerifiedSource>) {
        let dir = tempfile::tempdir().expect("source dir");
        let mut artifacts = Vec::new();
        for name in ArtifactName::ALL {
            let bytes = format!("{}@{version}", name.file_name());
            std::fs::write(dir.path().join(name.file_name()), &bytes).expect("write source");
            artifacts.push(ArtifactEntry {
                name,
                version: version.to_owned(),
                sha256: sha256_hex(bytes.as_bytes()),
            });
        }
        let manifest = ArtifactManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            release: version.to_owned(),
            artifacts,
        };
        let sources = manifest.verify_sources(dir.path()).expect("verified");
        (manifest, dir, sources)
    }

    fn contents(root: &Path) -> BTreeMap<String, String> {
        ArtifactName::ALL
            .into_iter()
            .map(|name| {
                let content = std::fs::read_to_string(root.join(name.file_name()))
                    .unwrap_or_else(|_| "ABSENT".to_owned());
                (name.file_name().to_owned(), content)
            })
            .collect()
    }

    fn internals(root: &Path) -> Vec<String> {
        let mut names = std::fs::read_dir(root.join(INTERNAL_DIR))
            .map(|entries| {
                entries
                    .map(|entry| {
                        entry
                            .expect("entry")
                            .file_name()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn assert_release(root: &Path, version: &str) {
        for (file, content) in contents(root) {
            assert_eq!(
                content,
                format!("{file}@{version}"),
                "{file} must carry release {version}"
            );
        }
    }

    #[test]
    fn a_clean_install_lands_all_four_and_replay_writes_nothing() {
        let root = tempfile::tempdir().expect("install root");
        let (manifest, sources_dir, sources) = release("1.0.0");
        let before = contents(sources_dir.path());
        let installer = Installer::at(root.path()).expect("installer");
        assert_eq!(
            installer.install(&manifest, &sources, &mut NoFaults),
            Ok(InstallOutcome::Installed)
        );
        assert_release(root.path(), "1.0.0");
        assert_eq!(internals(root.path()), vec!["lock".to_owned()]);
        let statuses = installer.status(&manifest).expect("status");
        assert!(statuses.iter().all(|status| status.is_current(&manifest)));

        assert_eq!(
            installer.install(&manifest, &sources, &mut NoFaults),
            Ok(InstallOutcome::AlreadyCurrent)
        );
        assert_eq!(
            contents(sources_dir.path()),
            before,
            "the source inventory is read-only"
        );
    }

    #[test]
    fn an_update_replaces_the_whole_set() {
        let root = tempfile::tempdir().expect("install root");
        let installer = Installer::at(root.path()).expect("installer");
        let (old, _old_dir, old_sources) = release("1.0.0");
        let (new, _new_dir, new_sources) = release("2.0.0");
        installer
            .install(&old, &old_sources, &mut NoFaults)
            .expect("install");
        assert_eq!(
            installer.install(&new, &new_sources, &mut NoFaults),
            Ok(InstallOutcome::Updated)
        );
        assert_release(root.path(), "2.0.0");
        assert_eq!(internals(root.path()), vec!["lock".to_owned()]);
    }

    fn fault_points() -> Vec<FailPoint> {
        let mut points = vec![
            FailPoint::BeforeLock,
            FailPoint::AfterLock,
            FailPoint::BeforeStage,
        ];
        for name in ArtifactName::ALL {
            points.push(FailPoint::AfterStage(name));
            points.push(FailPoint::BeforeSwap(name));
            points.push(FailPoint::AfterSwap(name));
        }
        points.push(FailPoint::BeforeFinalize);
        points.push(FailPoint::AfterFinalize);
        points
    }

    #[test]
    fn a_fault_at_every_boundary_leaves_one_whole_state_and_recovers() {
        for point in fault_points() {
            let root = tempfile::tempdir().expect("install root");
            let installer = Installer::at(root.path()).expect("installer");
            let (old, _old_dir, old_sources) = release("1.0.0");
            let (new, _new_dir, new_sources) = release("2.0.0");
            installer
                .install(&old, &old_sources, &mut NoFaults)
                .expect("baseline install");

            let error = installer
                .install(
                    &new,
                    &new_sources,
                    &mut ScriptedFaults::failing_at(point.clone()),
                )
                .expect_err("the scripted fault aborts");
            assert_eq!(
                error,
                InstallError::Injected {
                    point: point.clone()
                }
            );

            // At the instant of the fault no target is a third thing: each is
            // the old release, the new release, or (after one swap) one of
            // those. The journal, not the visible set, carries the direction.
            for content in contents(root.path()).values() {
                assert!(
                    content.ends_with("@1.0.0") || content.ends_with("@2.0.0"),
                    "unexpected bytes after fault at {point:?}: {content}"
                );
            }

            let outcome = installer.recover(&mut NoFaults).expect("recovery");
            let expect_new = matches!(
                point,
                FailPoint::BeforeSwap(_)
                    | FailPoint::AfterSwap(_)
                    | FailPoint::BeforeFinalize
                    | FailPoint::AfterFinalize
            );
            if expect_new {
                assert_release(root.path(), "2.0.0");
            } else {
                assert_release(root.path(), "1.0.0");
            }
            let expected_outcome =
                if matches!(point, FailPoint::BeforeSwap(_) | FailPoint::AfterSwap(_)) {
                    RecoveryOutcome::Completed
                } else {
                    RecoveryOutcome::Nothing
                };
            assert_eq!(outcome, expected_outcome, "recovery at {point:?}");
            assert_eq!(
                internals(root.path()),
                vec!["lock".to_owned()],
                "cleanup after {point:?}"
            );
            assert_eq!(
                installer.recover(&mut NoFaults),
                Ok(RecoveryOutcome::Nothing),
                "replay after {point:?}"
            );
        }
    }

    #[test]
    fn a_concurrent_writer_is_refused_and_our_bytes_never_land() {
        let root = tempfile::tempdir().expect("install root");
        let installer = Installer::at(root.path()).expect("installer");
        let (old, _old_dir, old_sources) = release("1.0.0");
        let (new, _new_dir, new_sources) = release("2.0.0");
        installer
            .install(&old, &old_sources, &mut NoFaults)
            .expect("baseline install");

        let mut faults = ScriptedFaults::observing(FailPoint::BeforeSwap(ArtifactName::Mcp), {
            |root| {
                std::fs::write(root.join("kontor-mcp"), b"foreign writer")
                    .map_err(|_| InstallError::Io)
            }
        });
        let error = installer
            .install(&new, &new_sources, &mut faults)
            .expect_err("concurrent change is refused");
        assert_eq!(
            error,
            InstallError::ConcurrentChange {
                name: ArtifactName::Mcp
            }
        );
        let contents = contents(root.path());
        assert_eq!(contents["kontor"], "kontor@1.0.0");
        assert_eq!(contents["kontord"], "kontord@1.0.0");
        assert_eq!(contents["kontor-bootstrap"], "kontor-bootstrap@1.0.0");
        assert_eq!(
            contents["kontor-mcp"], "foreign writer",
            "bootstrap never overwrites bytes it did not write"
        );
        assert_eq!(internals(root.path()), vec!["lock".to_owned()]);
    }

    #[test]
    fn only_one_bootstrap_can_transact_on_a_root() {
        let root = tempfile::tempdir().expect("install root");
        let installer = Installer::at(root.path()).expect("installer");
        let (old, _old_dir, old_sources) = release("1.0.0");
        let (new, _new_dir, new_sources) = release("2.0.0");
        installer
            .install(&old, &old_sources, &mut NoFaults)
            .expect("baseline install");

        let mut faults = ScriptedFaults::observing(FailPoint::BeforeStage, |root| {
            let path = root.join(INTERNAL_DIR).join(LOCK_FILE);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|_| InstallError::Io)?;
            match FileExt::try_lock(&file) {
                Err(TryLockError::WouldBlock) => Ok(()),
                _ => Err(InstallError::Locked),
            }
        });
        assert_eq!(
            installer.install(&new, &new_sources, &mut faults),
            Ok(InstallOutcome::Updated)
        );
    }

    #[test]
    fn a_tampered_or_missing_source_is_refused_before_any_write() {
        let root = tempfile::tempdir().expect("install root");
        let installer = Installer::at(root.path()).expect("installer");
        let (mut manifest, _dir, mut sources) = release("1.0.0");
        sources[0].sha256 = "0".repeat(64);
        manifest.artifacts[0].sha256 = "0".repeat(64);
        assert_eq!(
            installer.install(&manifest, &sources, &mut NoFaults),
            Err(InstallError::Manifest(
                ManifestError::SourceDigestMismatch {
                    name: ArtifactName::Cli
                }
            ))
        );
        assert_eq!(contents(root.path())["kontor"], "ABSENT");
        assert_eq!(internals(root.path()), vec!["lock".to_owned()]);
    }

    #[test]
    fn a_relative_root_and_an_unsupported_journal_are_refused() {
        assert_eq!(
            Installer::at("relative/root").expect_err("relative root"),
            InstallError::RelativeRoot
        );
        let root = tempfile::tempdir().expect("install root");
        std::fs::create_dir_all(root.path().join(INTERNAL_DIR)).expect("internal");
        std::fs::write(root.path().join(INTERNAL_DIR).join(JOURNAL_FILE), "{}").expect("journal");
        let installer = Installer::at(root.path()).expect("installer");
        assert_eq!(
            installer.recover(&mut NoFaults),
            Err(InstallError::JournalUnsupported)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_targets_and_roots_are_refused() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("install root");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("kontor-mcp"), "outside bytes").expect("write");
        symlink(
            outside.path().join("kontor-mcp"),
            root.path().join("kontor-mcp"),
        )
        .expect("symlink target");
        let installer = Installer::at(root.path()).expect("installer");
        let (manifest, _dir, sources) = release("1.0.0");
        assert_eq!(
            installer.install(&manifest, &sources, &mut NoFaults),
            Err(InstallError::SymlinkTarget {
                name: ArtifactName::Mcp
            })
        );
        assert_eq!(
            std::fs::read_to_string(outside.path().join("kontor-mcp")).expect("read"),
            "outside bytes"
        );

        let linked = tempfile::tempdir().expect("linked root parent");
        let linked_root = linked.path().join("install");
        symlink(outside.path(), &linked_root).expect("symlink root");
        let installer = Installer::at(&linked_root).expect("installer");
        assert_eq!(
            installer.install(&manifest, &sources, &mut NoFaults),
            Err(InstallError::SymlinkRoot)
        );
    }
}
