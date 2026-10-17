//! The versioned, coherent artifact set one bootstrap installs.
//!
//! A release is coherent when it names exactly the four executables that make
//! up a working installation — `kontor`, `kontord`, `kontor-mcp` and
//! `kontor-bootstrap` — once each, all at the release version, each with a
//! SHA-256 digest. Nothing else is a valid release: a missing artifact, a
//! duplicate, a stray name or a version that disagrees with the release is
//! refused before any filesystem effect.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::confinement::{ConfinementError, Dir, MAX_DOCUMENT_BYTES};

/// The largest artifact source this build will read or stage.
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[cfg(test)]
type PreReadSeamCallback = std::cell::RefCell<Option<Box<dyn FnMut(&std::path::Path)>>>;

// Test-only seam: runs after the confined parent directory has been opened
// and before the manifest bytes are read, so a regression can swap the
// pathname's parent and prove the already-opened descriptor is used.
#[cfg(test)]
thread_local! {
    static PRE_READ_SEAM: PreReadSeamCallback = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_pre_read_seam(action: impl FnMut(&std::path::Path) + 'static) {
    PRE_READ_SEAM.with(|seam| *seam.borrow_mut() = Some(Box::new(action)));
}

#[cfg(test)]
fn run_pre_read_seam(path: &std::path::Path) {
    PRE_READ_SEAM.with(|seam| {
        if let Some(action) = seam.borrow_mut().as_mut() {
            action(path);
        }
    });
}

/// The manifest schema this build understands. Unknown versions are refused
/// rather than interpreted field-by-field.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// One executable of the Kontor artifact set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactName {
    /// The `kontor` command-line client.
    Cli,
    /// The `kontord` daemon executable.
    Daemon,
    /// The `kontor-mcp` MCP server executable.
    Mcp,
    /// The `kontor-bootstrap` executable itself.
    Bootstrap,
}

impl ArtifactName {
    /// Every artifact a coherent release must declare, in install order.
    pub const ALL: [ArtifactName; 4] = [
        ArtifactName::Cli,
        ArtifactName::Daemon,
        ArtifactName::Mcp,
        ArtifactName::Bootstrap,
    ];

    /// The stable lowercase name used in manifests and receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ArtifactName::Cli => "cli",
            ArtifactName::Daemon => "daemon",
            ArtifactName::Mcp => "mcp",
            ArtifactName::Bootstrap => "bootstrap",
        }
    }

    /// The file name this artifact carries in a source set and an install root.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            ArtifactName::Cli => "kontor",
            ArtifactName::Daemon => "kontord",
            ArtifactName::Mcp => "kontor-mcp",
            ArtifactName::Bootstrap => "kontor-bootstrap",
        }
    }
}

impl fmt::Display for ArtifactName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One declared artifact: identity, version and expected content digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactEntry {
    /// Which executable this is.
    pub name: ArtifactName,
    /// The release version this artifact must carry.
    pub version: String,
    /// Lowercase hex SHA-256 of the exact bytes to install.
    pub sha256: String,
}

/// A complete, versioned artifact set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    /// Manifest schema version; [`MANIFEST_SCHEMA_VERSION`] is the only one
    /// this build interprets.
    pub schema_version: u32,
    /// The one version every artifact in this set must carry.
    pub release: String,
    /// The declared artifacts.
    pub artifacts: Vec<ArtifactEntry>,
}

/// A source artifact verified against the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSource {
    /// The artifact this file provides.
    pub name: ArtifactName,
    /// The verified lowercase hex SHA-256.
    pub sha256: String,
}

/// Why a manifest or a source set was refused.
///
/// Every variant names a logical artifact, never a filesystem path: an error
/// can travel into a redacted receipt or a stderr line without leaking a host
/// location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The document was not valid JSON or not the declared shape.
    #[error("the artifact manifest is not a valid document")]
    InvalidDocument,
    /// The schema version is not this build's.
    #[error("the artifact manifest schema version {found} is not supported")]
    UnsupportedSchema {
        /// The version that was found.
        found: u32,
    },
    /// The release version is not a safe bounded token.
    #[error("the artifact manifest declares an unsafe release version")]
    InvalidRelease,
    /// An artifact version is not a safe bounded token.
    #[error("artifact {name} declares an unsafe version token")]
    InvalidArtifactVersion {
        /// The offending artifact.
        name: ArtifactName,
    },
    /// The artifact source root itself is unusable.
    #[error("the artifact source root is unusable")]
    SourceRootInvalid,
    /// A source artifact exceeds the configured size bound.
    #[error("source artifact {name} exceeds the configured size bound")]
    SourceTooLarge {
        /// The oversized artifact.
        name: ArtifactName,
    },
    /// The artifact list does not hold exactly the four set members.
    #[error("the artifact manifest does not declare exactly the four set artifacts")]
    IncompleteSet,
    /// An artifact name appeared more than once.
    #[error("the artifact manifest declares {name} more than once")]
    DuplicateArtifact {
        /// The duplicated artifact.
        name: ArtifactName,
    },
    /// An artifact's version disagrees with the release version.
    ///
    /// The supplied token values are deliberately not part of this public
    /// error: a syntactically valid but sensitive-looking token must not be
    /// reflected through output just because its charset is safe.
    #[error("artifact {name} version does not match the manifest release")]
    IncoherentVersion {
        /// The offending artifact.
        name: ArtifactName,
    },
    /// This target is outside the supported macOS/Linux boundary.
    #[error("confined manifest access is unsupported on this target")]
    UnsupportedPlatform,
    /// A digest is not 64 lowercase hex characters.
    #[error("artifact {name} declares an invalid SHA-256 digest")]
    InvalidDigest {
        /// The offending artifact.
        name: ArtifactName,
    },
    /// A source file for a declared artifact is missing.
    #[error("source artifact {name} is missing")]
    SourceMissing {
        /// The missing artifact.
        name: ArtifactName,
    },
    /// A source file is a symlink, which an install never follows.
    #[error("source artifact {name} is a symlink and is refused")]
    SourceSymlink {
        /// The refused artifact.
        name: ArtifactName,
    },
    /// A source file could not be read.
    #[error("source artifact {name} cannot be read")]
    SourceUnreadable {
        /// The unreadable artifact.
        name: ArtifactName,
    },
    /// A source file's bytes do not match its declared digest.
    #[error("source artifact {name} does not match its declared SHA-256 digest")]
    SourceDigestMismatch {
        /// The mismatching artifact.
        name: ArtifactName,
    },
}

impl ArtifactManifest {
    /// Parse and validate one manifest document.
    ///
    /// # Errors
    /// Any [`ManifestError`]; validation runs before the value is returned, so
    /// a caller never holds a manifest that claims a release it cannot honour.
    pub fn parse(document: &str) -> Result<Self, ManifestError> {
        let manifest =
            serde_json::from_str::<Self>(document).map_err(|_| ManifestError::InvalidDocument)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Read and validate a manifest from a file.
    ///
    /// A symlinked manifest is refused: the file this program reads must be the
    /// file a release published, not an indirection through mutable state. The
    /// document is size-bounded before parsing.
    ///
    /// # Errors
    /// [`ManifestError::InvalidDocument`] when the path is a symlink, too
    /// large, or cannot be read, plus every [`ManifestError`] `parse` can
    /// return.
    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        if !crate::confinement::supported_target() {
            return Err(ManifestError::UnsupportedPlatform);
        }
        // Confined, fd-bound read: the parent directory is opened once and the
        // manifest is read relative to that descriptor with no-follow and a
        // size bound. A swapped or symlinked path is refused.
        let parent = path.parent().ok_or(ManifestError::InvalidDocument)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ManifestError::InvalidDocument)?;
        let directory = Dir::open_root(parent).map_err(map_source_root)?;
        #[cfg(test)]
        run_pre_read_seam(path);
        let bytes = directory
            .read_child(name, MAX_DOCUMENT_BYTES)
            .map_err(|_| ManifestError::InvalidDocument)?
            .ok_or(ManifestError::InvalidDocument)?;
        let text = String::from_utf8(bytes).map_err(|_| ManifestError::InvalidDocument)?;
        Self::parse(&text)
    }

    /// Refuse a structurally incoherent set.
    ///
    /// # Errors
    /// Every structural [`ManifestError`].
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        if !is_safe_token(&self.release) {
            return Err(ManifestError::InvalidRelease);
        }
        if self.artifacts.len() != ArtifactName::ALL.len() {
            return Err(ManifestError::IncompleteSet);
        }
        for name in ArtifactName::ALL {
            let mut matches = self.artifacts.iter().filter(|entry| entry.name == name);
            let Some(entry) = matches.next() else {
                return Err(ManifestError::IncompleteSet);
            };
            if matches.next().is_some() {
                return Err(ManifestError::DuplicateArtifact { name });
            }
            if !is_safe_token(&entry.version) {
                return Err(ManifestError::InvalidArtifactVersion { name });
            }
            if entry.version != self.release {
                return Err(ManifestError::IncoherentVersion { name });
            }
            if !is_lower_hex_digest(&entry.sha256) {
                return Err(ManifestError::InvalidDigest { name });
            }
        }
        Ok(())
    }

    /// The declared entry for one artifact.
    #[must_use]
    pub fn entry(&self, name: ArtifactName) -> Option<&ArtifactEntry> {
        self.artifacts.iter().find(|entry| entry.name == name)
    }

    /// Verify every declared artifact against its source directory.
    ///
    /// The source directory is opened once and every artifact is read relative
    /// to that held descriptor with no-follow semantics, so a symlinked source
    /// artifact (or a swapped source directory) is refused rather than read.
    /// This is a pure read: no root is modified.
    ///
    /// # Errors
    /// Missing, symlinked, oversized, unreadable or mismatching source
    /// artifacts.
    pub fn verify_sources(&self, source_dir: &Path) -> Result<Vec<VerifiedSource>, ManifestError> {
        if !crate::confinement::supported_target() {
            return Err(ManifestError::UnsupportedPlatform);
        }
        let root = Dir::open_root(source_dir).map_err(map_source_root)?;
        self.verify_sources_in(&root)
    }

    /// Verify every declared artifact against an already-open source directory.
    ///
    /// # Errors
    /// The same refusals as [`ArtifactManifest::verify_sources`].
    pub fn verify_sources_in(&self, source: &Dir) -> Result<Vec<VerifiedSource>, ManifestError> {
        let mut verified = Vec::with_capacity(ArtifactName::ALL.len());
        for name in ArtifactName::ALL {
            let Some(entry) = self.entry(name) else {
                return Err(ManifestError::IncompleteSet);
            };
            let bytes = match source.read_child(name.file_name(), MAX_ARTIFACT_BYTES) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return Err(ManifestError::SourceMissing { name }),
                Err(ConfinementError::Symlink) => {
                    return Err(ManifestError::SourceSymlink { name });
                }
                Err(ConfinementError::TooLarge) => {
                    return Err(ManifestError::SourceTooLarge { name });
                }
                Err(ConfinementError::Missing) => {
                    return Err(ManifestError::SourceMissing { name });
                }
                Err(_) => return Err(ManifestError::SourceUnreadable { name }),
            };
            let digest = sha256_hex(&bytes);
            if !digest.eq_ignore_ascii_case(&entry.sha256) {
                return Err(ManifestError::SourceDigestMismatch { name });
            }
            verified.push(VerifiedSource {
                name,
                sha256: digest,
            });
        }
        Ok(verified)
    }
}

/// The lowercase hex SHA-256 of one byte slice.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Whether a string is a bounded, path-safe version token.
///
/// This is a charset and path-safety validator, not a redaction mechanism: it
/// excludes separators, traversal markers and control bytes so a version
/// accepted here cannot carry a path into an error message or a receipt.
/// Receipt fields remain structurally typed; this check is not a general
/// secret detector.
pub fn is_safe_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && !token.contains("..")
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn map_source_root(error: ConfinementError) -> ManifestError {
    match error {
        ConfinementError::Missing => ManifestError::SourceRootInvalid,
        ConfinementError::Symlink => ManifestError::SourceRootInvalid,
        ConfinementError::UnsafeName => ManifestError::SourceRootInvalid,
        _ => ManifestError::SourceRootInvalid,
    }
}

/// Whether a string is exactly 64 lowercase hex characters.
fn is_lower_hex_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: ArtifactName) -> ArtifactEntry {
        ArtifactEntry {
            name,
            version: "1.2.3".to_owned(),
            sha256: "a".repeat(64),
        }
    }

    fn manifest() -> ArtifactManifest {
        ArtifactManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            release: "1.2.3".to_owned(),
            artifacts: ArtifactName::ALL.into_iter().map(entry).collect(),
        }
    }

    #[test]
    fn a_swapped_manifest_parent_does_not_redirect_the_opened_bytes() {
        let holder = tempfile::tempdir().expect("holder");
        let parent = holder.path().join("release");
        std::fs::create_dir_all(&parent).expect("parent");
        let manifest_path = parent.join("manifest.json");
        let original = manifest();
        std::fs::write(&manifest_path, serde_json::to_vec(&original).expect("json"))
            .expect("write");
        let decoy = holder.path().join("decoy");
        std::fs::create_dir_all(&decoy).expect("decoy");
        let mut replacement = manifest();
        replacement.release = "9.9.9".to_owned();
        for entry in &mut replacement.artifacts {
            entry.version = "9.9.9".to_owned();
        }
        std::fs::write(
            decoy.join("manifest.json"),
            serde_json::to_vec(&replacement).expect("json"),
        )
        .expect("write");
        let moved = holder.path().join("release-moved");
        let parent_path = parent.clone();
        let moved_path = moved.clone();
        let decoy_path = decoy.clone();
        set_pre_read_seam(move |_| {
            std::fs::rename(&parent_path, &moved_path).expect("move");
            std::fs::rename(&decoy_path, &parent_path).expect("swap");
        });
        let parsed = ArtifactManifest::read(&manifest_path).expect("read");
        assert_eq!(
            parsed.release, "1.2.3",
            "the already-opened parent directory supplies the bytes"
        );
        set_pre_read_seam(|_| {});
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn unsupported_target_read_and_source_verification_are_typed_refusals() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            ArtifactManifest::read(&dir.path().join("manifest.json")),
            Err(ManifestError::UnsupportedPlatform)
        );
        let manifest = manifest();
        assert_eq!(
            manifest.verify_sources(dir.path()),
            Err(ManifestError::UnsupportedPlatform)
        );
    }

    #[test]
    fn a_manifest_round_trips_and_validates() {
        let document = serde_json::to_string(&manifest()).expect("serialize");
        let parsed = ArtifactManifest::parse(&document).expect("parse");
        assert_eq!(parsed, manifest());
        assert_eq!(
            parsed.entry(ArtifactName::Mcp).expect("entry").version,
            "1.2.3"
        );
    }

    #[test]
    fn a_missing_stray_or_duplicated_artifact_is_refused() {
        let mut missing = manifest();
        missing.artifacts.pop();
        assert_eq!(missing.validate(), Err(ManifestError::IncompleteSet));

        let mut duplicate = manifest();
        duplicate.artifacts[3].name = ArtifactName::Cli;
        assert_eq!(
            duplicate.validate(),
            Err(ManifestError::DuplicateArtifact {
                name: ArtifactName::Cli
            })
        );
    }

    #[test]
    fn an_incoherent_version_is_refused() {
        let mut drifted = manifest();
        drifted.artifacts[1].version = "9.9.9".to_owned();
        assert_eq!(
            drifted.validate(),
            Err(ManifestError::IncoherentVersion {
                name: ArtifactName::Daemon,
            })
        );
    }

    #[test]
    fn refusal_texts_never_reflect_supplied_tokens() {
        // Valid-character synthetic markers: the public texts are constant and
        // carry only the logical artifact name, never the supplied token.
        let mut drifted = manifest();
        drifted.artifacts[0].version = "SYNTHETICMARKER_TOKEN".to_owned();
        let error = drifted.validate().expect_err("incoherent version");
        assert_eq!(
            error,
            ManifestError::IncoherentVersion {
                name: ArtifactName::Cli
            }
        );
        assert!(!error.to_string().contains("SYNTHETICMARKER_TOKEN"));

        let mut unsafe_release = manifest();
        unsafe_release.release = "SYNTHETIC..RELEASE".to_owned();
        let error = unsafe_release.validate().expect_err("unsafe release");
        assert_eq!(error, ManifestError::InvalidRelease);
        assert!(!error.to_string().contains("SYNTHETIC"));
    }

    #[test]
    fn an_unsupported_schema_and_bad_digest_are_refused() {
        let mut future = manifest();
        future.schema_version = 2;
        assert_eq!(
            future.validate(),
            Err(ManifestError::UnsupportedSchema { found: 2 })
        );

        let mut bad = manifest();
        bad.artifacts[0].sha256 = "ABC".to_owned();
        assert_eq!(
            bad.validate(),
            Err(ManifestError::InvalidDigest {
                name: ArtifactName::Cli
            })
        );
    }

    #[test]
    fn unknown_fields_and_non_json_documents_are_refused() {
        let document = r#"{"schema_version":1,"release":"1","artifacts":[],"extra":1}"#;
        assert_eq!(
            ArtifactManifest::parse(document),
            Err(ManifestError::InvalidDocument)
        );
        assert_eq!(
            ArtifactManifest::parse("not json"),
            Err(ManifestError::InvalidDocument)
        );
    }

    #[test]
    fn source_verification_accepts_matching_bytes_and_refuses_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut manifest = manifest();
        for name in ArtifactName::ALL {
            let bytes = format!("{} bytes", name.as_str());
            let digest = sha256_hex(bytes.as_bytes());
            manifest
                .artifacts
                .iter_mut()
                .find(|entry| entry.name == name)
                .expect("entry")
                .sha256 = digest;
            std::fs::write(dir.path().join(name.file_name()), bytes).expect("write");
        }
        let verified = manifest.verify_sources(dir.path()).expect("verified");
        assert_eq!(verified.len(), 4);
        assert_eq!(verified[0].name, ArtifactName::Cli);

        std::fs::write(dir.path().join("kontor"), "tampered").expect("write");
        assert_eq!(
            manifest.verify_sources(dir.path()),
            Err(ManifestError::SourceDigestMismatch {
                name: ArtifactName::Cli
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_source_artifact_is_refused() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let mut manifest = manifest();
        let bytes = b"real bytes";
        let digest = sha256_hex(bytes);
        manifest.artifacts[0].sha256 = digest;
        std::fs::write(outside.path().join("kontor"), bytes).expect("write");
        symlink(outside.path().join("kontor"), dir.path().join("kontor")).expect("symlink");
        assert_eq!(
            manifest.verify_sources(dir.path()),
            Err(ManifestError::SourceSymlink {
                name: ArtifactName::Cli
            })
        );
    }

    #[test]
    fn a_missing_source_artifact_is_named_without_a_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = manifest().verify_sources(dir.path()).expect_err("missing");
        assert_eq!(
            error,
            ManifestError::SourceMissing {
                name: ArtifactName::Cli
            }
        );
        assert!(
            !error
                .to_string()
                .contains(dir.path().to_str().expect("utf8"))
        );
    }
}
