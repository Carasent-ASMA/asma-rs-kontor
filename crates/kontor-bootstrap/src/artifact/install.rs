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
//! journal is refused before any effect. Recovery removes or replaces bytes
//! only through observation-bound checks, and an active uncooperative same-UID
//! writer can still change, displace or delete bytes between an observation and
//! the by-name effect; that residual is accepted, not eliminated.
//!
//! Directory sync calls narrow the process-interruption window at the durable
//! name transitions this module claims. They are not a power-loss durability
//! qualification: the fixture injections observe process interruption, not a
//! storage stack that loses acknowledged writes.

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
/// A bootstrap-owned ownership receipt inside a staging directory.
const STAGING_OWNER_FILE: &str = ".kontor-staging-owner";
/// Schema of the staging ownership receipt.
const STAGING_OWNER_SCHEMA_VERSION: u32 = 1;
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
    /// Bootstrap-owned and foreign material could not be separated safely at
    /// an observed boundary, so both sides were left in place, a typed
    /// conflict was returned, and nothing was unlinked or overwritten by any
    /// observation this branch made.
    PreservedConflict,
}

/// One artifact's current digest in the active generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactStatus {
    /// Which artifact.
    pub name: ArtifactName,
    /// Lowercase hex SHA-256 of the active bytes, or `None` when absent.
    pub sha256: Option<String>,
    /// The release actually declared by the active generation's marker, when
    /// one is active. `None` means no active generation was observed; this
    /// operand is the release attribution that travels with the digest.
    pub release: Option<String>,
    /// The permission bits observed on the active artifact, or `None` when no
    /// active generation was observed. Content plus release is not enough:
    /// a binary its owner cannot execute is reported as mode drift.
    pub mode: Option<u32>,
}

impl ArtifactStatus {
    /// An artifact with no active generation: no observed digest and no
    /// observed release. Absence has no provenance and invents none.
    fn absent(name: ArtifactName) -> Self {
        Self {
            name,
            sha256: None,
            release: None,
            mode: None,
        }
    }

    /// An artifact observed in the active generation: the digest and the
    /// release actually declared by that generation's marker travel together.
    fn observed(
        name: ArtifactName,
        sha256: Option<String>,
        release: Option<String>,
        mode: Option<u32>,
    ) -> Self {
        Self {
            name,
            sha256,
            release,
            mode,
        }
    }

    /// Whether this artifact already carries the manifest's digest.
    #[must_use]
    pub fn is_current(&self, manifest: &ArtifactManifest) -> bool {
        manifest
            .entry(self.name)
            .is_some_and(|entry| self.sha256.as_deref() == Some(entry.sha256.as_str()))
    }

    /// Whether the observed mode lets the owner execute this artifact.
    #[must_use]
    pub fn is_executable(&self) -> bool {
        self.mode.is_some_and(|mode| mode & 0o100 != 0)
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
    /// A bootstrap temporary name is already taken by foreign material.
    #[error("a bootstrap temporary name is already taken and was preserved")]
    TemporaryTaken,
    /// The staging name already exists and is never adopted.
    #[error("the staging name already exists and was not adopted")]
    StagingTaken,
    /// The activation pointer or its temporary changed; recovery is required.
    #[error("the activation pointer changed and needs explicit reconciliation")]
    ActivationChanged,
    /// This target is outside the supported macOS/Linux boundary.
    #[error("confined install is unsupported on this target")]
    UnsupportedPlatform,
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
    modes: Vec<(ArtifactName, u32)>,
    executable: bool,
}

impl ActiveGeneration {
    fn digest(&self, name: ArtifactName) -> Option<&str> {
        self.hashes
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, digest)| digest.as_str())
    }

    fn mode(&self, name: ArtifactName) -> Option<u32> {
        self.modes
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, mode)| *mode)
    }

    fn matches(&self, manifest: &ArtifactManifest) -> bool {
        // The release must match too: equal digests under a different release
        // are a new coherent install, so the receipt names the active release.
        if self.release != manifest.release {
            return false;
        }
        ArtifactName::ALL.into_iter().all(|name| {
            manifest
                .entry(name)
                .is_some_and(|entry| self.digest(name) == Some(entry.sha256.as_str()))
        })
    }
}

/// The only creation-provenance claim bootstrap writes into a staging
/// directory: schema, journal nonce, generation and manifest digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagingOwner {
    schema_version: u32,
    nonce: String,
    generation: String,
    manifest_sha256: String,
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
        if !crate::confinement::supported_target() {
            return Err(InstallError::UnsupportedPlatform);
        }
        manifest.validate()?;
        let root = self.open_root_if_exists()?;
        let Some(root) = root else {
            return Ok(ArtifactName::ALL
                .into_iter()
                .map(ArtifactStatus::absent)
                .collect());
        };
        let active = self.read_active(&root)?;
        Ok(ArtifactName::ALL
            .into_iter()
            .map(|name| match &active {
                Some(active) => ArtifactStatus::observed(
                    name,
                    active.digest(name).map(str::to_owned),
                    Some(active.release.clone()),
                    active.mode(name),
                ),
                None => ArtifactStatus::absent(name),
            })
            .collect())
    }

    /// The verified active generation name, if one is active.
    ///
    /// # Errors
    /// [`InstallError`].
    pub fn active_generation(&self) -> Result<Option<String>, InstallError> {
        if !crate::confinement::supported_target() {
            return Err(InstallError::UnsupportedPlatform);
        }
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
        if !crate::confinement::supported_target() {
            return Err(InstallError::UnsupportedPlatform);
        }
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
        if let Some(active) = active.as_ref().filter(|active| active.matches(manifest)) {
            // A verified artifact whose content is exact but whose mode lost
            // its execute bits would fail at execution. Repair the modes
            // before reporting the generation as already current.
            if !active.executable {
                self.repair_modes(&root, &active.generation)?;
            }
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
        let staging = match internal.create_child_dir_exclusive(&staging_name, INTERNAL_MODE) {
            Ok(staging) => staging,
            Err(ConfinementError::Exists) => return Err(InstallError::StagingTaken),
            Err(error) => return Err(error.into()),
        };
        let staging_identity = staging.dir_identity()?;
        let owner_bytes = serde_json::to_vec(&StagingOwner {
            schema_version: STAGING_OWNER_SCHEMA_VERSION,
            nonce: nonce.clone(),
            generation: generation.clone(),
            manifest_sha256: manifest_sha256(manifest)?,
        })
        .map_err(|_| InstallError::Io)?;
        let owner_file = staging.create_child_file(STAGING_OWNER_FILE, &[], PRIVATE_MODE)?;
        std::io::Write::write_all(&mut &owner_file, &owner_bytes).map_err(|_| InstallError::Io)?;
        owner_file.sync_all().map_err(|_| InstallError::Io)?;
        // The ownership receipt's directory entry is durable before the first
        // artifact effect; later marker and name-transition syncs are kept.
        staging.sync()?;
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
                    self.discard_own_staging(&internal, &staging_name, &journal)?;
                    return Err(InstallError::Manifest(ManifestError::SourceMissing {
                        name,
                    }));
                }
            };
            if sha256_hex(&bytes) != entry.sha256 {
                self.discard_own_staging(&internal, &staging_name, &journal)?;
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
        staging.sync()?;

        let mut journal = journal;
        journal.state = JournalState::Staged;
        self.write_journal(&root, &journal)?;

        let releases = root.open_child_or_create(RELEASES_DIR, BINARY_MODE)?;
        if releases.kind_child(&generation)?.is_some() {
            self.discard_own_staging(&internal, &staging_name, &journal)?;
            return Err(InstallError::GenerationCollision);
        }
        faults.hit(FailPoint::BeforeSwap(ArtifactName::Mcp), &self.root)?;
        // F2(1)/F2A: the last tamper boundary preceded this observation; the
        // held staging directory is verified and its name is bound to that
        // exact inode immediately before promotion, so a substituted
        // directory cannot enter releases.
        self.verify_marker(&staging, &journal)?;
        if internal.child_identity(&staging_name)? != Some(staging_identity) {
            return Err(InstallError::ActiveUnverified);
        }
        match internal.rename_child_noreplace(&staging_name, &releases, &generation) {
            Ok(()) => {}
            Err(ConfinementError::Exists) => return Err(InstallError::GenerationCollision),
            Err(error) => return Err(error.into()),
        }
        internal.sync()?;
        releases.sync()?;
        faults.hit(FailPoint::AfterPromote, &self.root)?;
        let promoted = releases.open_child(&generation)?;
        let promoted_identity = promoted.dir_identity()?;
        self.verify_marker(&promoted, &journal)?;
        journal.state = JournalState::Promoted;
        self.write_journal(&root, &journal)?;
        faults.hit(FailPoint::BeforeActivate, &self.root)?;
        // F2(2)/F2B: the promoted generation is verified again after the last
        // boundary before activation, with its name bound to the held inode;
        // invalid bytes are never activated.
        if releases.child_identity(&generation)? != Some(promoted_identity)
            || self.verify_marker(&promoted, &journal).is_err()
        {
            return Err(InstallError::ActiveUnverified);
        }
        self.activate(&root, &journal, faults)?;
        root.sync()?;

        faults.hit(FailPoint::BeforeFinalize, &self.root)?;
        // F2(3)/F2C: success binds the exact pointer name and the journal
        // manifest after the final boundary; a coherent marker+artifact
        // tamper is refused and the journal is preserved.
        if self.verify_active(&root, &journal).is_err() {
            return Err(InstallError::ActiveUnverified);
        }
        internal.remove_child(JOURNAL_FILE)?;
        // The journal-removal name transition is synced before the final
        // boundary, like every other durable name transition here.
        internal.sync()?;
        faults.hit(FailPoint::AfterFinalize, &self.root)?;
        Ok(if clean {
            InstallOutcome::Installed
        } else {
            InstallOutcome::Updated
        })
    }

    /// Finish or unwind an interrupted transaction, observing every name it
    /// would remove or replace and preserving anything it cannot prove is
    /// bootstrap's own at that observed boundary. An accepted same-UID
    /// observation-to-effect residual remains: bytes can change between an
    /// observation and the by-name effect, and omission or cleanup after a
    /// detected mismatch is never authorized.
    ///
    /// # Errors
    /// [`InstallError`].
    pub fn recover(&self, faults: &mut dyn FaultInjector) -> Result<RecoveryOutcome, InstallError> {
        if !crate::confinement::supported_target() {
            return Err(InstallError::UnsupportedPlatform);
        }
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
                if !self.discard_own_staging(&internal, &staging_name, &journal)? {
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                internal.remove_child(JOURNAL_FILE)?;
                internal.sync()?;
                Ok(RecoveryOutcome::Nothing)
            }
            JournalState::Staged => {
                if !self.staging_is_complete(&internal, &staging_name, &journal)? {
                    // A generation promoted before the journal update is
                    // completed, not rolled back; an unverified one is
                    // preserved.
                    if releases.kind_child(&journal.generation)?.is_some() {
                        let generation_dir = releases.open_child(&journal.generation)?;
                        match self.verify_marker(&generation_dir, &journal) {
                            Ok(()) => match self.activate(root, &journal, faults) {
                                Ok(()) => {
                                    if self.verify_active(root, &journal).is_err() {
                                        return Ok(RecoveryOutcome::PreservedConflict);
                                    }
                                    root.sync()?;
                                    internal.sync()?;
                                    internal.remove_child(JOURNAL_FILE)?;
                                    internal.sync()?;
                                    return Ok(RecoveryOutcome::Completed);
                                }
                                Err(InstallError::ActiveUnverified)
                                | Err(InstallError::ActivationChanged) => {
                                    return Ok(RecoveryOutcome::PreservedConflict);
                                }
                                Err(error) => return Err(error),
                            },
                            Err(InstallError::ActiveUnverified) => {
                                return Ok(RecoveryOutcome::PreservedConflict);
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    if self.discard_own_staging(&internal, &staging_name, &journal)? {
                        internal.remove_child(JOURNAL_FILE)?;
                        internal.sync()?;
                        return Ok(RecoveryOutcome::RolledBack);
                    }
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                // F2A: bind the staging name to the verified held directory
                // immediately before promotion.
                let staging_dir = internal.open_child(&staging_name)?;
                let staging_identity = staging_dir.dir_identity()?;
                if internal.child_identity(&staging_name)? != Some(staging_identity)
                    || self.verify_marker(&staging_dir, &journal).is_err()
                {
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                if releases.kind_child(&journal.generation)?.is_none() {
                    match internal.rename_child_noreplace(
                        &staging_name,
                        &releases,
                        &journal.generation,
                    ) {
                        Ok(()) => {}
                        Err(ConfinementError::Exists) => {
                            return Err(InstallError::GenerationCollision);
                        }
                        Err(error) => return Err(error.into()),
                    }
                    // Recovery name-transition durability ordering: the
                    // promotion's directory entries are synced before the
                    // activation and journal-removal boundaries.
                    internal.sync()?;
                    releases.sync()?;
                }
                let generation_dir = releases.open_child(&journal.generation)?;
                match self.verify_marker(&generation_dir, &journal) {
                    Ok(()) => {}
                    Err(InstallError::ActiveUnverified) => {
                        return Ok(RecoveryOutcome::PreservedConflict);
                    }
                    Err(error) => return Err(error),
                }
                match self.activate(root, &journal, faults) {
                    Ok(()) => {}
                    Err(InstallError::ActiveUnverified) | Err(InstallError::ActivationChanged) => {
                        return Ok(RecoveryOutcome::PreservedConflict);
                    }
                    Err(error) => return Err(error),
                }
                if self.verify_active(root, &journal).is_err() {
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                root.sync()?;
                internal.sync()?;
                internal.remove_child(JOURNAL_FILE)?;
                internal.sync()?;
                Ok(RecoveryOutcome::Completed)
            }
            JournalState::Promoted => {
                let generation = releases.open_child(&journal.generation)?;
                match self.verify_marker(&generation, &journal) {
                    Ok(()) => {}
                    Err(InstallError::ActiveUnverified) => {
                        return Ok(RecoveryOutcome::PreservedConflict);
                    }
                    Err(error) => return Err(error),
                }
                match self.activate(root, &journal, faults) {
                    Ok(()) => {}
                    Err(InstallError::ActiveUnverified) | Err(InstallError::ActivationChanged) => {
                        return Ok(RecoveryOutcome::PreservedConflict);
                    }
                    Err(error) => return Err(error),
                }
                if self.verify_active(root, &journal).is_err() {
                    return Ok(RecoveryOutcome::PreservedConflict);
                }
                root.sync()?;
                internal.sync()?;
                internal.remove_child(JOURNAL_FILE)?;
                internal.sync()?;
                Ok(RecoveryOutcome::Completed)
            }
        }
    }

    /// Whether a staging directory is complete and provably bootstrap's own:
    /// the ownership receipt matches the journal, every entry is an allowed
    /// basename, the marker matches, and every artifact is digest-bound.
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
        if !self.owner_receipt_matches(&staging, journal)? {
            return Ok(false);
        }
        let Ok(entries) = staging.entries() else {
            return Ok(false);
        };
        for entry in entries {
            let ours = ArtifactName::ALL
                .into_iter()
                .any(|name| name.file_name() == entry)
                || entry == MARKER_FILE
                || entry == STAGING_OWNER_FILE;
            if !ours {
                return Ok(false);
            }
        }
        let marker_bytes = match staging.read_child(MARKER_FILE, MAX_DOCUMENT_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(_) => return Ok(false),
        };
        let Ok(marker) = serde_json::from_slice::<GenerationMarker>(&marker_bytes) else {
            return Ok(false);
        };
        if marker.schema_version != MARKER_SCHEMA_VERSION
            || marker.nonce != journal.nonce
            || marker.generation != journal.generation
            || marker.release != journal.manifest.release
            || marker.artifacts != journal.manifest.artifacts
        {
            return Ok(false);
        }
        for name in ArtifactName::ALL {
            let Some(entry) = journal.manifest.entry(name) else {
                return Ok(false);
            };
            let Ok(Some(bytes)) = staging.read_child(
                name.file_name(),
                crate::artifact::manifest::MAX_ARTIFACT_BYTES,
            ) else {
                return Ok(false);
            };
            if sha256_hex(&bytes) != entry.sha256 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Remove a staging directory only when the ownership receipt, allowed
    /// basenames and digest-bound artifacts all match.
    ///
    /// The receipt/digest check and the recursive delete are separate by-name
    /// effects: under an active uncooperative same-UID writer bytes can change
    /// between them. Any detected uncertainty preserves; there is no
    /// speculative cleanup and no reclaim of legacy receipt-less staging.
    fn discard_own_staging(
        &self,
        internal: &Dir,
        staging_name: &str,
        journal: &Journal,
    ) -> Result<bool, InstallError> {
        // Observation errors are uncertainty, and uncertainty preserves: only
        // a clean "absent" permits the caller to treat the staging name as
        // removed.
        match internal.kind_child(staging_name) {
            Ok(None) => return Ok(true),
            Ok(Some(kind)) if kind != crate::confinement::NodeKind::Directory => {
                return Ok(false);
            }
            Ok(Some(_)) => {}
            Err(_) => return Ok(false),
        }
        let staging = internal.open_child(staging_name)?;
        if !self.owner_receipt_matches(&staging, journal)? {
            return Ok(false);
        }
        let Ok(entries) = staging.entries() else {
            return Ok(false);
        };
        for entry in entries {
            let allowed = ArtifactName::ALL
                .into_iter()
                .any(|name| name.file_name() == entry)
                || entry == MARKER_FILE
                || entry == STAGING_OWNER_FILE;
            if !allowed {
                return Ok(false);
            }
            if let Some(name) = ArtifactName::ALL
                .into_iter()
                .find(|name| name.file_name() == entry)
            {
                let Some(entry_def) = journal.manifest.entry(name) else {
                    return Ok(false);
                };
                let Ok(Some(bytes)) =
                    staging.read_child(&entry, crate::artifact::manifest::MAX_ARTIFACT_BYTES)
                else {
                    return Ok(false);
                };
                if sha256_hex(&bytes) != entry_def.sha256 {
                    return Ok(false);
                }
            }
        }
        if !self.marker_matches_if_present(&staging, journal)? {
            return Ok(false);
        }
        internal.remove_tree_child(staging_name)?;
        Ok(true)
    }

    /// Whether a staging directory carries this transaction's ownership
    /// receipt: schema, journal nonce, generation and manifest digest all
    /// matching. A legacy or partial directory without it is never ours.
    fn owner_receipt_matches(
        &self,
        staging: &Dir,
        journal: &Journal,
    ) -> Result<bool, InstallError> {
        let bytes = match staging.read_child(STAGING_OWNER_FILE, MAX_DOCUMENT_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(_) => return Ok(false),
        };
        let Ok(owner) = serde_json::from_slice::<StagingOwner>(&bytes) else {
            return Ok(false);
        };
        Ok(owner.schema_version == STAGING_OWNER_SCHEMA_VERSION
            && owner.nonce == journal.nonce
            && owner.generation == journal.generation
            && owner.manifest_sha256 == manifest_sha256(&journal.manifest)?)
    }

    /// Whether an optional generation marker inside a staging directory
    /// matches this journal; absence is not a mismatch here because a partial
    /// staging may not have reached the marker yet.
    fn marker_matches_if_present(
        &self,
        staging: &Dir,
        journal: &Journal,
    ) -> Result<bool, InstallError> {
        let bytes = match staging.read_child(MARKER_FILE, MAX_DOCUMENT_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(true),
            // A marker that is a symlink, directory, oversized or unreadable is
            // uncertainty, and uncertainty preserves.
            Err(_) => return Ok(false),
        };
        let Ok(marker) = serde_json::from_slice::<GenerationMarker>(&bytes) else {
            return Ok(false);
        };
        Ok(marker.schema_version == MARKER_SCHEMA_VERSION
            && marker.nonce == journal.nonce
            && marker.generation == journal.generation
            && marker.release == journal.manifest.release
            && marker.artifacts == journal.manifest.artifacts)
    }

    /// Exchange-and-verify the activation pointer against the journal's
    /// expected previous pointer.
    ///
    /// The exchange is an atomic swap of names; verification observes both
    /// sides afterwards. A mismatch is not resolved by a blind exchange-back:
    /// the swapped-out side is treated as the previous pointer only when the
    /// exchange outcome and that side's observed target match the journal's
    /// expected previous generation, and both names are preserved when
    /// anything is uncertain. This is exchange-and-verify at observed
    /// boundaries, never kernel compare-and-swap.
    fn activate(
        &self,
        root: &Dir,
        journal: &Journal,
        faults: &mut dyn FaultInjector,
    ) -> Result<(), InstallError> {
        let nonce = &journal.nonce;
        let generation = &journal.generation;
        let target = format!("{RELEASES_DIR}/{generation}");
        let existing = root.read_link_child(CURRENT_POINTER)?;
        let expected_previous = journal
            .previous
            .as_ref()
            .map(|previous| format!("{RELEASES_DIR}/{}", previous.generation));
        if existing.as_deref() == Some(target.as_str()) {
            // Verified already-target replay: the pointer must still bind to
            // the journal manifest.
            self.verify_active(root, journal)?;
            return Ok(());
        }
        // Full Option comparison: an expected previous pointer that is now
        // absent is a change, never clean-install admission.
        if existing != expected_previous {
            return Err(InstallError::ActivationChanged);
        }
        if existing.is_some() {
            // A pointer bootstrap cannot verify is foreign: preserve it.
            self.read_active(root)?;
        }
        let temporary = format!(".current-{nonce}.tmp");
        if root.kind_child(&temporary)?.is_some() {
            // Reclaim only an activation temporary this journal created: a
            // symlink whose target is this generation, under the nonce-derived
            // name. Everything else is preserved with a typed refusal.
            let mine = root.kind_child(&temporary)? == Some(crate::confinement::NodeKind::Symlink)
                && root.read_link_child(&temporary)?.as_deref() == Some(target.as_str());
            if !mine {
                return Err(InstallError::ActivationChanged);
            }
            if let Some(identity) = root.child_identity(&temporary)?
                && !root.remove_child_if(&temporary, identity)?
            {
                return Err(InstallError::ActivationChanged);
            }
        }
        match root.create_child_symlink(&format!("{RELEASES_DIR}/{generation}"), &temporary) {
            Ok(()) => {}
            Err(ConfinementError::Exists) => return Err(InstallError::ActivationChanged),
            Err(error) => return Err(error.into()),
        }
        let temporary_identity = root.child_identity(&temporary)?;
        faults.hit(FailPoint::AfterActivationTemp, &self.root)?;
        // A: after the last boundary, re-observe the retained creation-bound
        // temporary identity/target and the requested generation's identity
        // and journal-manifest content before either activation branch.
        let temporary_ok = root.kind_child(&temporary)?
            == Some(crate::confinement::NodeKind::Symlink)
            && root.read_link_child(&temporary)?.as_deref() == Some(target.as_str())
            && root.child_identity(&temporary)? == temporary_identity;
        let releases = root.open_child(RELEASES_DIR)?;
        let generation_dir = releases.open_child(generation)?;
        let generation_identity = generation_dir.dir_identity()?;
        let generation_ok = releases.child_identity(generation)? == Some(generation_identity)
            && self.verify_marker(&generation_dir, journal).is_ok();
        if !(temporary_ok && generation_ok) {
            return Err(InstallError::ActiveUnverified);
        }
        match &existing {
            Some(previous) => {
                // E: retained displaced identity, captured before the exchange.
                let previous_identity = root.child_identity(CURRENT_POINTER)?;
                root.exchange_children(&temporary, root, CURRENT_POINTER)?;
                let displaced = root.read_link_child(&temporary)?;
                let installed = root.read_link_child(CURRENT_POINTER)?;
                if displaced.as_deref() != Some(previous.as_str())
                    || installed.as_deref() != Some(target.as_str())
                {
                    return Err(InstallError::ActivationChanged);
                }
                if let Some(identity) = previous_identity {
                    let _ = root.remove_child_if(&temporary, identity);
                }
            }
            None => {
                // Clean install: there is no pointer to exchange; the
                // no-replace rename covers pointer absence only.
                root.rename_child_noreplace(&temporary, root, CURRENT_POINTER)?;
            }
        }
        faults.hit(FailPoint::AfterSwap(ArtifactName::Mcp), &self.root)?;
        Ok(())
    }

    /// Verify that the active pointer names this journal's generation and that
    /// the active generation binds to the journal manifest. Used after every
    /// recovery activation, before the journal may be removed.
    fn verify_active(&self, root: &Dir, journal: &Journal) -> Result<(), InstallError> {
        let expected_pointer = format!("{RELEASES_DIR}/{}", journal.generation);
        if root.read_link_child(CURRENT_POINTER)?.as_deref() != Some(expected_pointer.as_str()) {
            return Err(InstallError::ActiveUnverified);
        }
        let releases = root.open_child(RELEASES_DIR)?;
        let active = releases.open_child(&journal.generation)?;
        self.verify_marker(&active, journal)
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
        // A verified active generation carries complete coherent marker
        // metadata: schema, safe release, generated nonce, the release/nonce
        // construction of its own name, and exactly the four declared
        // artifacts whose versions and hashes agree with the marker. An
        // incoherent marker is a path-free typed refusal and is never
        // returned or persisted as verified state.
        if marker.schema_version != MARKER_SCHEMA_VERSION
            || marker.generation != generation
            || !is_nonce(&marker.nonce)
            || marker.generation != format!("{}-{}", marker.release, marker.nonce)
            || !crate::artifact::manifest::is_safe_token(&marker.release)
            || marker.artifacts.len() != ArtifactName::ALL.len()
        {
            return Err(InstallError::ActiveUnverified);
        }
        let mut hashes = Vec::with_capacity(ArtifactName::ALL.len());
        let mut modes = Vec::with_capacity(ArtifactName::ALL.len());
        let mut executable = true;
        for name in ArtifactName::ALL {
            let entry = marker
                .artifacts
                .iter()
                .find(|entry| entry.name == name)
                .ok_or(InstallError::ActiveUnverified)?;
            if entry.version != marker.release {
                return Err(InstallError::ActiveUnverified);
            }
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
            let mode = generation_dir
                .child_mode(name.file_name())?
                .ok_or(InstallError::ActiveUnverified)?;
            // Bootstrap-owned binaries are executed by their owner, so a bit
            // that only some other class may execute is not usable.
            executable &= mode & 0o100 != 0;
            modes.push((name, mode));
            hashes.push((name, digest));
        }
        Ok(Some(ActiveGeneration {
            generation: generation.to_owned(),
            release: marker.release,
            hashes,
            modes,
            executable,
        }))
    }

    /// Restore the installed executable mode on a verified active generation.
    ///
    /// Only an artifact whose owner cannot execute it is repaired, and it is
    /// opened read-only: `fchmod` needs no write access to the bytes, so a
    /// read-only executable sibling or a running Linux binary does not block
    /// the repair.
    fn repair_modes(&self, root: &Dir, generation: &str) -> Result<(), InstallError> {
        let releases = root.open_child(RELEASES_DIR)?;
        let generation_dir = releases.open_child(generation)?;
        for name in ArtifactName::ALL {
            let mode = generation_dir
                .child_mode(name.file_name())?
                .ok_or(InstallError::ActiveUnverified)?;
            if mode & 0o100 != 0 {
                continue;
            }
            let file = generation_dir.open_child_file_ro(name.file_name())?;
            Dir::set_file_mode(&file, BINARY_MODE)?;
        }
        Ok(())
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
        let file = match internal.create_child_file(&temporary, &bytes, PRIVATE_MODE) {
            Ok(file) => file,
            Err(ConfinementError::Exists) => return Err(InstallError::TemporaryTaken),
            Err(error) => return Err(error.into()),
        };
        drop(file);
        internal.rename_child(&temporary, &internal, JOURNAL_FILE)?;
        internal.sync()?;
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

/// The digest of one typed manifest, hashed through its deterministic serde
/// representation. It binds a staging ownership receipt to the exact journal
/// manifest and is the one operand shared by staging-owner creation and
/// recovery comparison.
fn manifest_sha256(manifest: &ArtifactManifest) -> Result<String, InstallError> {
    let bytes = serde_json::to_vec(manifest).map_err(|_| InstallError::Io)?;
    Ok(sha256_hex(&bytes))
}

pub(crate) fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}
