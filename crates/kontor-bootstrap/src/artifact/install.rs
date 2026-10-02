//! Atomic installation of one whole artifact generation into an injected root.
//!
//! Round one renamed the four binaries one by one, so an interruption could
//! expose a mixed active set. A generation commit removes that window: every
//! artifact is staged into one directory that is validated as a whole, promoted
//! with a single `renameat`, and activated by replacing one symlink pointer
//! (`current`) with a single `renameat`. There is no moment at which the active
//! set is half old and half new.
//!
//! Layout under the install root:
//!
//! ```text
//! <root>/current -> releases/<generation>            the activation pointer
//! <root>/releases/<generation>/                      one validated generation
//! <root>/.kontor-bootstrap/lock                      exclusive holder
//! <root>/.kontor-bootstrap/journal.json              the transaction
//! <root>/.kontor-bootstrap/staging-<nonce>/          generation being built
//! ```
//!
//! Every path operation is relative to a held directory descriptor
//! ([`crate::confinement::Dir`]); a symlink at any component inside the root and
//! a name that would leave it are typed refusals. A malformed or partial
//! journal is refused before any effect. Recovery never removes or overwrites
//! bytes it cannot prove were bootstrap's own.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};

use crate::artifact::manifest::{
    ArtifactEntry, ArtifactManifest, ArtifactName, ManifestError, VerifiedSource, sha256_hex,
};
use crate::confinement::{ConfinementError, Dir, MAX_DOCUMENT_BYTES};
use crate::fault::{FailPoint, FaultInjector};

const INTERNAL_DIR: &str = ".kontor-bootstrap";
const RELEASES_DIR: &str = "releases";
const CURRENT_POINTER: &str = "current";
const JOURNAL_FILE: &str = "journal.json";
const LOCK_FILE: &str = "lock";
const MARKER_FILE: &str = ".kontor-generation.json";
/// Journal schema: generation-based transactions.
const JOURNAL_SCHEMA_VERSION: u32 = 2;
/// Generation marker schema.
const MARKER_SCHEMA_VERSION: u32 = 1;
const INTERNAL_MODE: u32 = 0o700;
const BINARY_MODE: u32 = 0o755;
const PRIVATE_MODE: u32 = 0o600;

/// What one install or update did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// No active generation existed; a new one is active.
    Installed,
    /// The active generation was replaced wholesale by this release.
    Updated,
    /// The active generation already carries this release's digests.
    AlreadyCurrent,
}

/// What one recovery found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// No journal was present, or it had nothing left to do.
    Nothing,
    /// An interrupted transaction was completed so the new generation is
    /// active.
    Completed,
    /// An interrupted transaction was unwound; the previous generation stays
    /// active.
    RolledBack,
    /// Bootstrap-owned and foreign material could not be separated safely, so
    /// everything was preserved and a typed conflict returned.
    PreservedConflict,
}

/// One artifact's current digest in the active generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactStatus {
    /// Which artifact.
    pub name: ArtifactName,
    /// Lowercase hex SHA-256 of the active bytes, or `None` when absent.
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
    /// A confined filesystem operation was refused.
    #[error(transparent)]
    Confinement(#[from] ConfinementError),
    /// The install root must be an absolute path.
    #[error("the install root must be an absolute path")]
    RelativeRoot,
    /// The install root itself (or its internal state directory) is a symlink.
    #[error("the install root may not be a symlink")]
    SymlinkRoot,
    /// Another bootstrap holds the install root.
    #[error("another bootstrap holds this install root")]
    Locked,
    /// The journal exists but is not a document this build interprets.
    #[error("the install journal is not a supported document")]
    JournalUnsupported,
    /// The journal is malformed, inconsistent, or would escape the root.
    #[error("the install journal is invalid and was refused before any effect")]
    JournalInvalid,
    /// The active pointer exists but cannot be proven to be bootstrap's own.
    #[error("the active generation cannot be verified as bootstrap's own")]
    ActiveUnverified,
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
    /// A generation name collided with material bootstrap does not own.
    #[error("the generation directory already exists and is not bootstrap's own")]
    GenerationCollision,
    /// Recovery found foreign material it will not remove or overwrite.
    #[error("recovery preserved foreign material and needs explicit reconciliation")]
    RecoveryPreserved,
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
    _file: std::fs::File,
}

/// One install root this process may transact on.
#[derive(Debug, Clone)]
pub struct Installer {
    root: PathBuf,
}

/// A validated journal. Construction is the only way to obtain one, so an
/// invalid journal can never reach an effect.
#[derive(Debug, Clone)]
struct Journal {
    nonce: String,
    generation: String,
    state: JournalState,
    manifest: ArtifactManifest,
    previous: Option<PreviousGeneration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalState {
    Prepared,
    Staged,
    Promoted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousGeneration {
    generation: String,
    release: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalDocument {
    schema_version: u32,
    nonce: String,
    generation: String,
    state: JournalState,
    manifest: ArtifactManifest,
    previous: Option<PreviousGeneration>,
}

/// The verified active generation.
#[derive(Debug, Clone)]
struct ActiveGeneration {
    generation: String,
    release: String,
    hashes: Vec<(ArtifactName, String)>,
}

impl ActiveGeneration {
    fn digest(&self, name: ArtifactName) -> Option<&str> {
        self.hashes
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, digest)| digest.as_str())
    }

    fn matches(&self, manifest: &ArtifactManifest) -> bool {
        ArtifactName::ALL.into_iter().all(|name| {
            manifest
                .entry(name)
                .is_some_and(|entry| self.digest(name) == Some(entry.sha256.as_str()))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationMarker {
    schema_version: u32,
    generation: String,
    release: String,
    nonce: String,
    artifacts: Vec<ArtifactEntry>,
}

impl Installer {
    /// An installer for one explicit root.
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

    /// The stable path an MCP client should be registered with.
    #[must_use]
    pub fn active_mcp_path(&self) -> PathBuf {
        self.root
            .join(CURRENT_POINTER)
            .join(ArtifactName::Mcp.file_name())
    }

    /// Read each artifact's digest in the active generation.
    ///
    /// A pointer that cannot be verified as bootstrap's own is a typed
    /// conflict, never a silent absence.
    ///
    /// # Errors
    /// [`InstallError`].
    pub fn status(&self, manifest: &ArtifactManifest) -> Result<Vec<ArtifactStatus>, InstallError> {
        manifest.validate()?;
        let root = self.open_root_if_exists()?;
        let Some(root) = root else {
            return Ok(ArtifactName::ALL
                .into_iter()
                .map(|name| ArtifactStatus { name, sha256: None })
                .collect());
        };
        let active = self.read_active(&root)?;
        Ok(ArtifactName::ALL
            .into_iter()
            .map(|name| ArtifactStatus {
                name,
                sha256: active
                    .as_ref()
                    .and_then(|active| active.digest(name))
                    .map(str::to_owned),
            })
            .collect())
    }

    /// The verified active generation name, if one is active.
    ///
    /// # Errors
    /// [`InstallError`].
    pub fn active_generation(&self) -> Result<Option<String>, InstallError> {
        let Some(root) = self.open_root_if_exists()? else {
            return Ok(None);
        };
        Ok(self.read_active(&root)?.map(|active| active.generation))
    }

    /// Install, update or confirm one coherent artifact generation.
    ///
    /// # Errors
    /// Every [`InstallError`]. Nothing is written before the manifest, the
    /// source set and any existing journal are validated.
    pub fn install(
        &self,
        manifest: &ArtifactManifest,
        sources: &[VerifiedSource],
        source: &Dir,
        faults: &mut dyn FaultInjector,
    ) -> Result<InstallOutcome, InstallError> {
        manifest.validate()?;
        self.accept_sources(manifest, sources)?;

        // A malformed or partial journal is refused before any effect at all;
        // a valid one is recovered under the lock below.
        let _ = self.peek_journal()?;

        let root = self.open_or_create_root()?;
        faults.hit(FailPoint::BeforeLock, &self.root)?;
        let _lock = self.lock(&root)?;
        faults.hit(FailPoint::AfterLock, &self.root)?;

        if self.recover_locked(&root, faults)? == RecoveryOutcome::PreservedConflict {
            return Err(InstallError::RecoveryPreserved);
        }

        let active = self.read_active(&root)?;
        let previous = active.as_ref().map(|active| PreviousGeneration {
            generation: active.generation.clone(),
            release: active.release.clone(),
        });
        if active
            .as_ref()
            .is_some_and(|active| active.matches(manifest))
        {
            return Ok(InstallOutcome::AlreadyCurrent);
        }
        let clean = active.is_none();

        let nonce = nonce();
        let generation = format!("{}-{nonce}", manifest.release);
        if !crate::confinement::is_simple_name(&generation) {
            return Err(InstallError::JournalInvalid);
        }
        let journal = Journal {
            nonce: nonce.clone(),
            generation: generation.clone(),
            state: JournalState::Prepared,
            manifest: manifest.clone(),
            previous,
        };
        self.write_journal(&root, &journal)?;

        faults.hit(FailPoint::BeforeStage, &self.root)?;
        let internal = root.open_child(INTERNAL_DIR)?;
        let staging_name = format!("staging-{nonce}");
        let staging = internal.open_child_or_create(&staging_name, INTERNAL_MODE)?;
        for name in ArtifactName::ALL {
            let entry = manifest
                .entry(name)
                .ok_or(InstallError::Manifest(ManifestError::IncompleteSet))?;
            let bytes = match source.read_child(
                name.file_name(),
                crate::artifact::manifest::MAX_ARTIFACT_BYTES,
            )? {
                Some(bytes) => bytes,
                None => {
                    self.discard_own_staging(&internal, &staging_name)?;
                    return Err(InstallError::Manifest(ManifestError::SourceMissing {
                        name,
                    }));
                }
            };
            if sha256_hex(&bytes) != entry.sha256 {
                self.discard_own_staging(&internal, &staging_name)?;
                return Err(InstallError::Manifest(
                    ManifestError::SourceDigestMismatch { name },
                ));
            }
            let file = staging.create_child_file(name.file_name(), &[], PRIVATE_MODE)?;
            std::io::Write::write_all(&mut &file, &bytes).map_err(|_| InstallError::Io)?;
            file.sync_all().map_err(|_| InstallError::Io)?;
            Dir::set_file_mode(&file, BINARY_MODE)?;
            faults.hit(FailPoint::AfterStage(name), &self.root)?;
        }
        let marker = GenerationMarker {
            schema_version: MARKER_SCHEMA_VERSION,
            generation: generation.clone(),
            release: manifest.release.clone(),
            nonce: nonce.clone(),
            artifacts: manifest.artifacts.clone(),
        };
        let marker_bytes = serde_json::to_vec(&marker).map_err(|_| InstallError::Io)?;
        let marker_file = staging.create_child_file(MARKER_FILE, &[], PRIVATE_MODE)?;
        std::io::Write::write_all(&mut &marker_file, &marker_bytes)
            .map_err(|_| InstallError::Io)?;
        marker_file.sync_all().map_err(|_| InstallError::Io)?;

        let mut journal = journal;
        journal.state = JournalState::Staged;
        self.write_journal(&root, &journal)?;

        let releases = root.open_child_or_create(RELEASES_DIR, BINARY_MODE)?;
        if releases.kind_child(&generation)?.is_some() {
            self.discard_own_staging(&internal, &staging_name)?;
            return Err(InstallError::GenerationCollision);
        }
        faults.hit(FailPoint::BeforeSwap(ArtifactName::Mcp), &self.root)?;
        internal.rename_child(&staging_name, &releases, &generation)?;
        journal.state = JournalState::Promoted;
        self.write_journal(&root, &journal)?;
        self.activate(&root, &nonce, &generation, faults)?;

        faults.hit(FailPoint::BeforeFinalize, &self.root)?;
        internal.remove_child(JOURNAL_FILE)?;
        faults.hit(FailPoint::AfterFinalize, &self.root)?;
        Ok(if clean {
            InstallOutcome::Installed
        } else {
            InstallOutcome::Updated
        })
    }

    /// Finish or unwind an interrupted transaction without touching foreign
    /// bytes.
    ///
    /// # Errors
    /// [`InstallError`].
    pub fn recover(&self, faults: &mut dyn FaultInjector) -> Result<RecoveryOutcome, InstallError> {
        let root = self.open_root_if_exists()?;
        let Some(root) = root else {
            return Ok(RecoveryOutcome::Nothing);
        };
        // Validate the journal before the lock exists: a malformed journal is
        // refused with no effect at all.
        let _ = self.peek_journal_at(&root)?;
        faults.hit(FailPoint::BeforeLock, &self.root)?;
        let _lock = self.lock(&root)?;
        faults.hit(FailPoint::AfterLock, &self.root)?;
        self.recover_locked(&root, faults)
    }

    fn recover_locked(
        &self,
        root: &Dir,
        faults: &mut dyn FaultInjector,
    ) -> Result<RecoveryOutcome, InstallError> {
        let internal = match root.open_child(INTERNAL_DIR) {
            Ok(internal) => internal,
            Err(ConfinementError::Missing) => return Ok(RecoveryOutcome::Nothing),
            Err(error) => return Err(error.into()),
        };
        let Some(journal) = self.read_journal(&internal)? else {
            return Ok(RecoveryOutcome::Nothing);
        };
        let staging_name = format!("staging-{}", journal.nonce);
        let releases = root.open_child_or_create(RELEASES_DIR, BINARY_MODE)?;
        match journal.state {
            JournalState::Prepared => {
                if !self.discard_own_staging(&internal, &staging_name)? {
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                internal.remove_child(JOURNAL_FILE)?;
                Ok(RecoveryOutcome::Nothing)
            }
            JournalState::Staged => {
                if !self.staging_is_complete(&internal, &staging_name, &journal)? {
                    if self.discard_own_staging(&internal, &staging_name)? {
                        internal.remove_child(JOURNAL_FILE)?;
                        return Ok(RecoveryOutcome::RolledBack);
                    }
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                if releases.kind_child(&journal.generation)?.is_none() {
                    internal.rename_child(&staging_name, &releases, &journal.generation)?;
                }
                let generation_dir = releases.open_child(&journal.generation)?;
                self.verify_marker(&generation_dir, &journal)?;
                match self.activate(root, &journal.nonce, &journal.generation, faults) {
                    Ok(()) => {}
                    Err(InstallError::ActiveUnverified) => {
                        return Ok(RecoveryOutcome::PreservedConflict);
                    }
                    Err(error) => return Err(error),
                }
                internal.remove_child(JOURNAL_FILE)?;
                Ok(RecoveryOutcome::Completed)
            }
            JournalState::Promoted => {
                let generation = releases.open_child(&journal.generation)?;
                self.verify_marker(&generation, &journal)?;
                match self.activate(root, &journal.nonce, &journal.generation, faults) {
                    Ok(()) => {}
                    Err(InstallError::ActiveUnverified) => {
                        return Ok(RecoveryOutcome::PreservedConflict);
                    }
                    Err(error) => return Err(error),
                }
                internal.remove_child(JOURNAL_FILE)?;
                Ok(RecoveryOutcome::Completed)
            }
        }
    }

    /// Whether an unfinished staging directory is provably bootstrap's own:
    /// its name matches the journal nonce and every entry is one of ours.
    /// Unknown material makes it foreign and it is preserved.
    fn staging_is_complete(
        &self,
        internal: &Dir,
        staging_name: &str,
        journal: &Journal,
    ) -> Result<bool, InstallError> {
        let Ok(Some(kind)) = internal.kind_child(staging_name) else {
            return Ok(false);
        };
        if kind != crate::confinement::NodeKind::Directory {
            return Ok(false);
        }
        let staging = internal.open_child(staging_name)?;
        for entry in staging.entries()? {
            let ours = ArtifactName::ALL
                .into_iter()
                .any(|name| name.file_name() == entry)
                || entry == MARKER_FILE;
            if !ours {
                return Ok(false);
            }
        }
        if staging
            .read_child(MARKER_FILE, MAX_DOCUMENT_BYTES)?
            .is_none()
        {
            return Ok(false);
        }
        for name in ArtifactName::ALL {
            let Some(entry) = journal.manifest.entry(name) else {
                return Ok(false);
            };
            let Some(bytes) = staging.read_child(
                name.file_name(),
                crate::artifact::manifest::MAX_ARTIFACT_BYTES,
            )?
            else {
                return Ok(false);
            };
            if sha256_hex(&bytes) != entry.sha256 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Remove an unfinished staging directory only when every entry is ours.
    ///
    /// Returns `Ok(false)` when foreign material is present; the directory is
    /// then preserved untouched.
    fn discard_own_staging(
        &self,
        internal: &Dir,
        staging_name: &str,
    ) -> Result<bool, InstallError> {
        let Ok(Some(kind)) = internal.kind_child(staging_name) else {
            return Ok(true);
        };
        if kind != crate::confinement::NodeKind::Directory {
            return Ok(false);
        }
        let staging = internal.open_child(staging_name)?;
        for entry in staging.entries()? {
            let ours = ArtifactName::ALL
                .into_iter()
                .any(|name| name.file_name() == entry)
                || entry == MARKER_FILE;
            if !ours {
                return Ok(false);
            }
        }
        internal.remove_tree_child(staging_name)?;
        Ok(true)
    }

    /// Replace the activation pointer in one atomic rename.
    fn activate(
        &self,
        root: &Dir,
        nonce: &str,
        generation: &str,
        faults: &mut dyn FaultInjector,
    ) -> Result<(), InstallError> {
        let target = format!("{RELEASES_DIR}/{generation}");
        if let Some(existing) = root.read_link_child(CURRENT_POINTER)? {
            if existing == target {
                return Ok(());
            }
            // A pointer bootstrap cannot verify is foreign: preserve it.
            self.read_active(root)?;
        }
        let temporary = format!(".current-{nonce}.tmp");
        let _ = root.remove_child(&temporary);
        root.create_child_symlink(&format!("{RELEASES_DIR}/{generation}"), &temporary)?;
        root.rename_child(&temporary, root, CURRENT_POINTER)?;
        faults.hit(FailPoint::AfterSwap(ArtifactName::Mcp), &self.root)?;
        Ok(())
    }

    /// Verify the generation marker and every artifact digest in one promoted
    /// generation before it is trusted.
    fn verify_marker(&self, generation: &Dir, journal: &Journal) -> Result<(), InstallError> {
        let bytes = generation
            .read_child(MARKER_FILE, MAX_DOCUMENT_BYTES)?
            .ok_or(InstallError::ActiveUnverified)?;
        let marker: GenerationMarker =
            serde_json::from_slice(&bytes).map_err(|_| InstallError::ActiveUnverified)?;
        if marker.schema_version != MARKER_SCHEMA_VERSION
            || marker.generation != journal.generation
            || marker.nonce != journal.nonce
            || marker.release != journal.manifest.release
            || marker.artifacts != journal.manifest.artifacts
        {
            return Err(InstallError::ActiveUnverified);
        }
        for name in ArtifactName::ALL {
            let entry = journal
                .manifest
                .entry(name)
                .ok_or(InstallError::ActiveUnverified)?;
            let bytes = generation
                .read_child(
                    name.file_name(),
                    crate::artifact::manifest::MAX_ARTIFACT_BYTES,
                )?
                .ok_or(InstallError::ActiveUnverified)?;
            if sha256_hex(&bytes) != entry.sha256 {
                return Err(InstallError::ActiveUnverified);
            }
        }
        Ok(())
    }

    /// Resolve and verify the active generation.
    fn read_active(&self, root: &Dir) -> Result<Option<ActiveGeneration>, InstallError> {
        let Some(target) = root.read_link_child(CURRENT_POINTER)? else {
            return Ok(None);
        };
        let mut parts = target.split('/');
        let (Some(RELEASES_DIR), Some(generation), None) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(InstallError::ActiveUnverified);
        };
        if !crate::confinement::is_simple_name(generation) {
            return Err(InstallError::ActiveUnverified);
        }
        let releases = match root.open_child(RELEASES_DIR) {
            Ok(dir) => dir,
            Err(ConfinementError::Missing) => return Err(InstallError::ActiveUnverified),
            Err(error) => return Err(error.into()),
        };
        let generation_dir = match releases.open_child(generation) {
            Ok(dir) => dir,
            Err(ConfinementError::Missing) => return Err(InstallError::ActiveUnverified),
            Err(error) => return Err(error.into()),
        };
        let bytes = generation_dir
            .read_child(MARKER_FILE, MAX_DOCUMENT_BYTES)?
            .ok_or(InstallError::ActiveUnverified)?;
        let marker: GenerationMarker =
            serde_json::from_slice(&bytes).map_err(|_| InstallError::ActiveUnverified)?;
        if marker.schema_version != MARKER_SCHEMA_VERSION
            || marker.generation != generation
            || marker.artifacts.len() != ArtifactName::ALL.len()
        {
            return Err(InstallError::ActiveUnverified);
        }
        let mut hashes = Vec::with_capacity(ArtifactName::ALL.len());
        for name in ArtifactName::ALL {
            let entry = marker
                .artifacts
                .iter()
                .find(|entry| entry.name == name)
                .ok_or(InstallError::ActiveUnverified)?;
            let bytes = generation_dir
                .read_child(
                    name.file_name(),
                    crate::artifact::manifest::MAX_ARTIFACT_BYTES,
                )?
                .ok_or(InstallError::ActiveUnverified)?;
            let digest = sha256_hex(&bytes);
            if digest != entry.sha256 {
                return Err(InstallError::ActiveUnverified);
            }
            hashes.push((name, digest));
        }
        Ok(Some(ActiveGeneration {
            generation: generation.to_owned(),
            release: marker.release,
            hashes,
        }))
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

    fn open_root_if_exists(&self) -> Result<Option<Dir>, InstallError> {
        match Dir::open_root(&self.root) {
            Ok(dir) => Ok(Some(dir)),
            Err(ConfinementError::Missing) => Ok(None),
            Err(ConfinementError::Symlink) => Err(InstallError::SymlinkRoot),
            Err(error) => Err(error.into()),
        }
    }

    fn open_or_create_root(&self) -> Result<Dir, InstallError> {
        if let Some(root) = self.open_root_if_exists()? {
            return Ok(root);
        }
        std::fs::create_dir_all(&self.root).map_err(|_| InstallError::Io)?;
        self.open_root_if_exists()?.ok_or(InstallError::Io)
    }

    fn lock(&self, root: &Dir) -> Result<InstallLock, InstallError> {
        let internal = root.open_child_or_create(INTERNAL_DIR, INTERNAL_MODE)?;
        let file = match internal.kind_child(LOCK_FILE)? {
            Some(crate::confinement::NodeKind::Symlink) => {
                return Err(InstallError::SymlinkRoot);
            }
            Some(crate::confinement::NodeKind::File) => internal.open_child_file_rw(LOCK_FILE)?,
            Some(_) => {
                return Err(InstallError::Unreadable {
                    name: ArtifactName::Cli,
                });
            }
            None => internal.create_child_file(LOCK_FILE, &[], PRIVATE_MODE)?,
        };
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(InstallLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(InstallError::Locked),
            Err(TryLockError::Error(_)) => Err(InstallError::Io),
        }
    }

    /// Read, parse and fully validate the journal without any effect.
    fn peek_journal(&self) -> Result<Option<Journal>, InstallError> {
        let Some(root) = self.open_root_if_exists()? else {
            return Ok(None);
        };
        self.peek_journal_at(&root)
    }

    fn peek_journal_at(&self, root: &Dir) -> Result<Option<Journal>, InstallError> {
        let internal = match root.open_child(INTERNAL_DIR) {
            Ok(internal) => internal,
            Err(ConfinementError::Missing) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        self.read_journal(&internal)
    }

    fn read_journal(&self, internal: &Dir) -> Result<Option<Journal>, InstallError> {
        let Some(bytes) = internal.read_child(JOURNAL_FILE, MAX_DOCUMENT_BYTES)? else {
            return Ok(None);
        };
        let document: JournalDocument =
            serde_json::from_slice(&bytes).map_err(|_| InstallError::JournalUnsupported)?;
        self.validate_journal(document).map(Some)
    }

    fn validate_journal(&self, document: JournalDocument) -> Result<Journal, InstallError> {
        if document.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(InstallError::JournalUnsupported);
        }
        if !is_nonce(&document.nonce) {
            return Err(InstallError::JournalInvalid);
        }
        document
            .manifest
            .validate()
            .map_err(|_| InstallError::JournalInvalid)?;
        if document.generation != format!("{}-{}", document.manifest.release, document.nonce)
            || !crate::confinement::is_simple_name(&document.generation)
        {
            return Err(InstallError::JournalInvalid);
        }
        if let Some(previous) = &document.previous
            && (!crate::confinement::is_simple_name(&previous.generation)
                || !crate::artifact::manifest::is_safe_token(&previous.release))
        {
            return Err(InstallError::JournalInvalid);
        }
        Ok(Journal {
            nonce: document.nonce,
            generation: document.generation,
            state: document.state,
            manifest: document.manifest,
            previous: document.previous,
        })
    }

    fn write_journal(&self, root: &Dir, journal: &Journal) -> Result<(), InstallError> {
        let internal = root.open_child_or_create(INTERNAL_DIR, INTERNAL_MODE)?;
        let document = JournalDocument {
            schema_version: JOURNAL_SCHEMA_VERSION,
            nonce: journal.nonce.clone(),
            generation: journal.generation.clone(),
            state: journal.state,
            manifest: journal.manifest.clone(),
            previous: journal.previous.clone(),
        };
        let bytes = serde_json::to_vec(&document).map_err(|_| InstallError::Io)?;
        let temporary = format!("{JOURNAL_FILE}.tmp-{}", journal.nonce);
        if internal.kind_child(&temporary)?.is_some() {
            internal.remove_child(&temporary)?;
        }
        let file = internal.create_child_file(&temporary, &bytes, PRIVATE_MODE)?;
        drop(file);
        internal.rename_child(&temporary, &internal, JOURNAL_FILE)?;
        Ok(())
    }
}

/// Whether a nonce has the exact generated shape: two decimal runs.
fn is_nonce(nonce: &str) -> bool {
    let mut parts = nonce.split('-');
    let (Some(pid), Some(nanos), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let digits = |part: &str| {
        !part.is_empty() && part.len() <= 20 && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    digits(pid) && digits(nanos)
}

/// A process-unique suffix, shared with the client writer's temp names.
#[must_use]
pub fn nonce_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

pub(crate) fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}
