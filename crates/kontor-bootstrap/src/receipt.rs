//! Typed, redacted bootstrap receipts.
//!
//! A receipt is the durable answer to "what did bootstrap do": one disposition
//! per artifact and per client. It carries names, versions, digests, typed
//! states and typed reasons only — never a path, an argv, a credential value or
//! any secret material. Serialization re-validates every free-form field:
//! structural carriers are kept path-free, and an unsafe field is refused with
//! field-name-only text that never reflects the supplied token. Safe-token
//! validation is syntactic charset and path-safety validation, not secret
//! detection: a short syntactically valid token is legitimate metadata and may
//! be emitted as such.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::artifact::install::{ArtifactStatus, InstallOutcome};
use crate::artifact::manifest::{ArtifactManifest, ArtifactName, is_safe_token};
use crate::client::{AdapterResult, ClientId, EntryState, ObservedHash};
use crate::confinement::{ConfinementError, Dir};
use crate::service::{ServiceReceipt, is_safe_identity};

/// The receipt schema this build writes.
pub const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// The file name a receipt carries under an injected state root.
pub const RECEIPT_FILE_NAME: &str = "bootstrap-receipt.json";

/// What happened to one artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactDisposition {
    /// Newly installed.
    Installed,
    /// Replaced an older installation.
    Updated,
    /// Already carried this release's digest.
    AlreadyCurrent,
    /// Content matches, but the installed artifact cannot be executed by its
    /// owner.
    ModeDrift,
    /// Not installed.
    Absent,
    /// Completed by an interrupted-transaction recovery.
    Recovered,
}

/// One artifact's typed outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReceipt {
    /// Which artifact.
    pub name: ArtifactName,
    /// Its release version.
    pub version: String,
    /// Its expected lowercase hex SHA-256.
    pub sha256: String,
    /// What happened.
    pub disposition: ArtifactDisposition,
}

/// Why one client adapter could not complete, as a stable typed token.
///
/// This mirrors [`crate::client::AdapterError`] without carrying text, so a
/// failure report stays redacted by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientFailure {
    /// The injected client or state root is relative.
    RelativeRoot,
    /// The server specification is not installable.
    InvalidSpec,
    /// A filesystem operation failed.
    Io,
    /// Confined filesystem access refused an operation.
    Confinement,
    /// The client's command surface failed.
    CommandFailed,
    /// The target changed between reading and writing it.
    ConcurrentChange,
    /// A fault injector interrupted the client write.
    Injected,
    /// The ownership ledger could not be read.
    OwnershipUnreadable,
    /// A replacement failed and the original entry could not be restored.
    RestoreFailed,
    /// Bootstrap-owned and foreign material could not be separated.
    PreserveBoth,
}

impl ClientFailure {
    /// The stable lowercase name used in receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ClientFailure::RelativeRoot => "relative_root",
            ClientFailure::InvalidSpec => "invalid_spec",
            ClientFailure::Io => "io",
            ClientFailure::Confinement => "confinement",
            ClientFailure::CommandFailed => "command_failed",
            ClientFailure::ConcurrentChange => "concurrent_change",
            ClientFailure::Injected => "injected",
            ClientFailure::OwnershipUnreadable => "ownership_unreadable",
            ClientFailure::RestoreFailed => "restore_failed",
            ClientFailure::PreserveBoth => "preserve_both",
        }
    }
}

impl From<crate::client::AdapterError> for ClientFailure {
    fn from(error: crate::client::AdapterError) -> Self {
        use crate::client::AdapterError;
        match error {
            AdapterError::RelativeRoot => ClientFailure::RelativeRoot,
            AdapterError::InvalidSpec => ClientFailure::InvalidSpec,
            AdapterError::Io => ClientFailure::Io,
            AdapterError::Confinement(_) => ClientFailure::Confinement,
            AdapterError::CommandFailed => ClientFailure::CommandFailed,
            AdapterError::ConcurrentChange => ClientFailure::ConcurrentChange,
            AdapterError::Injected => ClientFailure::Injected,
            AdapterError::OwnershipUnreadable => ClientFailure::OwnershipUnreadable,
            AdapterError::RestoreFailed => ClientFailure::RestoreFailed,
            AdapterError::PreserveBoth => ClientFailure::PreserveBoth,
        }
    }
}

/// One client's outcome for an install or repair run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ClientReport {
    /// An install or repair was attempted.
    Applied {
        /// Which client.
        client: ClientId,
        /// The typed result.
        result: AdapterResult,
    },
    /// Only observation was requested.
    Observed {
        /// Which client.
        client: ClientId,
        /// The typed state.
        state: EntryState,
    },
    /// The client adapter failed, and the run continued with the others.
    Failed {
        /// Which client.
        client: ClientId,
        /// The typed failure.
        reason: ClientFailure,
    },
}

/// One whole bootstrap run.
///
/// Serialization is deliberately not derived: [`BootstrapReceipt::to_json`] is
/// the single path that always validates the free-form fields first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BootstrapReceipt {
    /// Receipt schema version.
    pub schema_version: u32,
    /// The release version of the artifact set.
    pub release: String,
    /// One entry per artifact.
    pub artifacts: Vec<ArtifactReceipt>,
    /// One entry per client.
    pub clients: Vec<ClientReport>,
    /// The service disposition, when a service port was consulted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceReceipt>,
}

/// The validated serialization mirror for [`BootstrapReceipt`].
#[derive(Serialize)]
struct ReceiptWire<'a> {
    schema_version: u32,
    release: &'a str,
    artifacts: &'a [ArtifactReceipt],
    clients: &'a [ClientReport],
    #[serde(skip_serializing_if = "Option::is_none")]
    service: Option<&'a ServiceReceipt>,
}

/// Why a receipt could not be produced or written.
///
/// No variant echoes an offending value: an injection attempt is answered with
/// a typed refusal that carries only the field name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
    /// The receipt could not be serialized.
    #[error("the bootstrap receipt could not be serialized")]
    Serialize,
    /// The state root is relative, which bootstrap refuses.
    #[error("the state root must be an absolute path")]
    RelativeStateRoot,
    /// The state root or receipt path is a symlink, which bootstrap refuses.
    #[error("the receipt destination is not a real directory")]
    UnsafeDestination,
    /// A free-form field violates the safe-token rules.
    #[error("the bootstrap receipt contains an unsafe {field} field")]
    UnsafeField {
        /// The field name only, never its value.
        field: &'static str,
    },
    /// The receipt temporary name is already taken by foreign material.
    #[error("the receipt temporary name is already taken and was preserved")]
    TemporaryTaken,
    /// This target is outside the supported macOS/Linux boundary.
    #[error("the bootstrap receipt destination is unsupported on this target")]
    UnsupportedPlatform,
    /// The receipt could not be written.
    #[error("the bootstrap receipt could not be written")]
    Io,
}

impl BootstrapReceipt {
    /// A receipt for a completed install or update.
    #[must_use]
    pub fn from_install(manifest: &ArtifactManifest, outcome: InstallOutcome) -> Self {
        let disposition = match outcome {
            InstallOutcome::Installed => ArtifactDisposition::Installed,
            InstallOutcome::Updated => ArtifactDisposition::Updated,
            InstallOutcome::AlreadyCurrent => ArtifactDisposition::AlreadyCurrent,
        };
        Self {
            schema_version: RECEIPT_SCHEMA_VERSION,
            release: manifest.release.clone(),
            artifacts: manifest
                .artifacts
                .iter()
                .map(|entry| ArtifactReceipt {
                    name: entry.name,
                    version: entry.version.clone(),
                    sha256: entry.sha256.clone(),
                    disposition,
                })
                .collect(),
            clients: Vec::new(),
            service: None,
        }
    }

    /// A receipt for a read-only status observation.
    #[must_use]
    pub fn from_status(manifest: &ArtifactManifest, statuses: &[ArtifactStatus]) -> Self {
        let artifacts = manifest
            .artifacts
            .iter()
            .map(|entry| {
                let status = statuses.iter().find(|status| status.name == entry.name);
                let content_current = status.is_some_and(|status| {
                    status.is_current(manifest)
                        && status.release.as_deref() == Some(manifest.release.as_str())
                });
                let disposition = match content_current {
                    true if status.is_some_and(ArtifactStatus::is_executable) => {
                        ArtifactDisposition::AlreadyCurrent
                    }
                    true => ArtifactDisposition::ModeDrift,
                    false => ArtifactDisposition::Absent,
                };
                ArtifactReceipt {
                    name: entry.name,
                    version: entry.version.clone(),
                    sha256: entry.sha256.clone(),
                    disposition,
                }
            })
            .collect();
        Self {
            schema_version: RECEIPT_SCHEMA_VERSION,
            release: manifest.release.clone(),
            artifacts,
            clients: Vec::new(),
            service: None,
        }
    }

    /// Attach one client report.
    pub fn push_client(&mut self, report: ClientReport) {
        self.clients.push(report);
    }

    /// Attach the service disposition.
    pub fn set_service(&mut self, service: ServiceReceipt) {
        self.service = Some(service);
    }

    /// Re-validate every free-form field against the safe-token rules.
    ///
    /// # Errors
    /// [`ReceiptError::UnsafeField`] naming only the field.
    pub fn validate(&self) -> Result<(), ReceiptError> {
        if !is_safe_token(&self.release) {
            return Err(ReceiptError::UnsafeField { field: "release" });
        }
        for artifact in &self.artifacts {
            if !is_safe_token(&artifact.version) {
                return Err(ReceiptError::UnsafeField {
                    field: "artifact.version",
                });
            }
            if artifact.sha256.len() != 64
                || !artifact
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(ReceiptError::UnsafeField {
                    field: "artifact.sha256",
                });
            }
        }
        if let Some(service) = &self.service
            && !is_safe_identity(&service.identity)
        {
            return Err(ReceiptError::UnsafeField {
                field: "service.identity",
            });
        }
        for report in &self.clients {
            let observed = match report {
                ClientReport::Applied { result, .. } => match result {
                    AdapterResult::Conflict { observed, .. } => observed.as_ref(),
                    _ => None,
                },
                ClientReport::Observed { state, .. } => state.observed(),
                ClientReport::Failed { .. } => None,
            };
            if let Some(observed) = observed
                && ObservedHash::parse(observed.as_str()).is_none()
            {
                return Err(ReceiptError::UnsafeField {
                    field: "client.observed",
                });
            }
        }
        Ok(())
    }

    /// The one-document JSON form.
    ///
    /// This is the single serialization path; it always validates first. The
    /// public type deliberately derives no `Serialize` so no caller can bypass
    /// validation with a derived implementation.
    ///
    /// # Errors
    /// [`ReceiptError::UnsafeField`] or [`ReceiptError::Serialize`].
    pub fn to_json(&self) -> Result<String, ReceiptError> {
        self.validate()?;
        let wire = ReceiptWire {
            schema_version: self.schema_version,
            release: &self.release,
            artifacts: &self.artifacts,
            clients: &self.clients,
            service: self.service.as_ref(),
        };
        serde_json::to_string_pretty(&wire).map_err(|_| ReceiptError::Serialize)
    }

    /// Write the receipt under an injected state root, confined and with
    /// private permissions from creation.
    ///
    /// # Errors
    /// A relative or symlinked state root, an unsafe field, or a filesystem
    /// failure.
    pub fn write_to(&self, state_root: &Path) -> Result<PathBuf, ReceiptError> {
        let temporary = format!(
            ".{RECEIPT_FILE_NAME}.tmp-{}-{}",
            std::process::id(),
            crate::artifact::install::nonce_suffix()
        );
        self.write_to_with_temporary(state_root, &temporary)
    }

    /// The write path with an explicit temporary name, so a regression can
    /// occupy exactly that name without a public seam.
    fn write_to_with_temporary(
        &self,
        state_root: &Path,
        temporary: &str,
    ) -> Result<PathBuf, ReceiptError> {
        if !crate::confinement::supported_target() {
            return Err(ReceiptError::UnsupportedPlatform);
        }
        if !state_root.is_absolute() {
            return Err(ReceiptError::RelativeStateRoot);
        }
        let document = self.to_json()?;
        let dir = match Dir::open_root(state_root) {
            Ok(dir) => dir,
            Err(ConfinementError::Missing) => {
                std::fs::create_dir_all(state_root).map_err(|_| ReceiptError::Io)?;
                Dir::open_root(state_root).map_err(map_root)?
            }
            Err(error) => return Err(map_root(error)),
        };
        let file = match dir.create_child_file(temporary, document.as_bytes(), 0o600) {
            Ok(file) => file,
            Err(ConfinementError::Exists) => return Err(ReceiptError::TemporaryTaken),
            Err(_) => return Err(ReceiptError::Io),
        };
        drop(file);
        dir.rename_child(temporary, &dir, RECEIPT_FILE_NAME)
            .map_err(|_| ReceiptError::Io)?;
        Ok(state_root.join(RECEIPT_FILE_NAME))
    }
}

fn map_root(error: ConfinementError) -> ReceiptError {
    match error {
        ConfinementError::Missing | ConfinementError::Symlink | ConfinementError::UnsafeName => {
            ReceiptError::UnsafeDestination
        }
        _ => ReceiptError::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::install::ArtifactStatus;
    use crate::artifact::manifest::{ArtifactEntry, MANIFEST_SCHEMA_VERSION};
    use crate::client::{ClientReceipt, ConflictReason, ObservedHash};
    use crate::service::{ServicePlatform, ServiceState};

    fn manifest() -> ArtifactManifest {
        ArtifactManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            release: "1.2.3".to_owned(),
            artifacts: ArtifactName::ALL
                .into_iter()
                .map(|name| ArtifactEntry {
                    name,
                    version: "1.2.3".to_owned(),
                    sha256: "a".repeat(64),
                })
                .collect(),
        }
    }

    #[test]
    fn a_receipt_is_one_typed_document_without_paths() {
        let mut receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        receipt.push_client(ClientReport::Applied {
            client: ClientId::Codex,
            result: AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                observed: Some(ObservedHash::of_bytes(b"observed")),
            },
        });
        receipt.push_client(ClientReport::Observed {
            client: ClientId::Cursor,
            state: EntryState::ClientAbsent,
        });
        receipt.set_service(ServiceReceipt {
            platform: ServicePlatform::Launchd,
            identity: "kontor-daemon".to_owned(),
            state: ServiceState::Absent,
        });
        let document = receipt.to_json().expect("serialize");
        assert!(document.contains("\"schema_version\": 1"));
        assert!(document.contains("\"installed\""));
        assert!(document.contains("\"concurrent_change\""));
        assert!(document.contains("\"client_absent\""));
        for forbidden in ["/Users", "/home", "program", "KONTOR_AUTH", ".."] {
            assert!(!document.contains(forbidden), "receipt leaked {forbidden}");
        }
    }

    #[test]
    fn injection_into_a_free_form_field_is_refused_without_reflection() {
        let mut receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        receipt.release = "../../etc/passwd".to_owned();
        let error = receipt.to_json().expect_err("unsafe release");
        assert_eq!(error, ReceiptError::UnsafeField { field: "release" });
        let reflected = error.to_string();
        assert!(!reflected.contains("etc"));
        assert!(!reflected.contains("passwd"));

        receipt.release = "1.2.3".to_owned();
        receipt.set_service(ServiceReceipt {
            platform: ServicePlatform::Launchd,
            identity: "/Users/me/.ssh/id_rsa".to_owned(),
            state: ServiceState::Absent,
        });
        assert_eq!(
            receipt.to_json().expect_err("unsafe identity"),
            ReceiptError::UnsafeField {
                field: "service.identity"
            }
        );
    }

    #[test]
    fn a_relative_state_root_is_refused() {
        let receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        assert_eq!(
            receipt.write_to(Path::new("relative")),
            Err(ReceiptError::RelativeStateRoot)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_receipt_destination_is_refused() {
        use std::os::unix::fs::symlink;
        let holder = tempfile::tempdir().expect("holder");
        let outside = tempfile::tempdir().expect("outside");
        let link = holder.path().join("state-link");
        symlink(&outside, &link).expect("link");
        let receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        assert_eq!(
            receipt.write_to(&link),
            Err(ReceiptError::UnsafeDestination)
        );
        assert!(
            std::fs::read_dir(outside.path())
                .expect("outside")
                .next()
                .is_none()
        );
    }

    #[test]
    fn an_occupied_receipt_temporary_is_refused_and_preserved() {
        let holder = tempfile::tempdir().expect("holder");
        let state = holder.path().join("state");
        std::fs::create_dir_all(&state).expect("state");
        let temporary = ".bootstrap-receipt.json.tmp-fixed-test";
        std::fs::write(state.join(temporary), "foreign temp").expect("foreign");
        let receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        assert_eq!(
            receipt.write_to_with_temporary(&state, temporary),
            Err(ReceiptError::TemporaryTaken)
        );
        assert_eq!(
            std::fs::read_to_string(state.join(temporary)).expect("read"),
            "foreign temp"
        );
        assert!(!state.join(RECEIPT_FILE_NAME).exists());
    }

    #[test]
    fn an_unsafe_field_refuses_the_write_without_reflection() {
        let holder = tempfile::tempdir().expect("holder");
        let state = holder.path().join("state");
        std::fs::create_dir_all(&state).expect("state");
        // A valid-character synthetic marker that is unsafe because of its
        // length, so only the safe-token rule (not the charset) refuses it.
        let marker = "SYNTHETIC_MARKER_".repeat(5);
        let mut receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        receipt.release = marker.clone();
        let error = receipt.write_to(&state).expect_err("unsafe release");
        assert_eq!(error, ReceiptError::UnsafeField { field: "release" });
        assert!(
            !error.to_string().contains(&marker),
            "the refusal must not reflect the supplied token"
        );
        assert!(!state.join(RECEIPT_FILE_NAME).exists());
        assert_eq!(
            std::fs::read_dir(&state).expect("state").count(),
            0,
            "no temporary or receipt may be created before validation passes"
        );
    }

    #[test]
    fn a_forged_nested_observed_hash_is_refused_by_validation() {
        let mut receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        receipt.push_client(ClientReport::Observed {
            client: ClientId::Codex,
            state: EntryState::Unrelated {
                observed: ObservedHash::unchecked(&"a".repeat(64).to_uppercase()),
            },
        });
        assert_eq!(
            receipt.to_json().expect_err("forged nested hash"),
            ReceiptError::UnsafeField {
                field: "client.observed"
            }
        );
    }

    #[test]
    fn forged_nested_states_are_refused_by_recursive_validation() {
        // Test-only unchecked construction isolates the recursive guard from
        // the checked deserializer. Every carrier state, across the distinct
        // invalid hash shapes (short6, short63, uppercase64, nonhex64), must
        // refuse at to_json and at write_to, and a refused write must create
        // no receipt and no temporary.
        let holder = tempfile::tempdir().expect("holder");
        let state_root = holder.path().join("state");
        std::fs::create_dir_all(&state_root).expect("state");
        let shapes = [
            "abcdef".to_owned(),
            "a".repeat(63),
            "A".repeat(64),
            format!("{}g", "a".repeat(63)),
        ];
        for shape in shapes {
            let carriers = [
                EntryState::Current {
                    observed: ObservedHash::unchecked(&shape),
                },
                EntryState::ManagedStale {
                    observed: ObservedHash::unchecked(&shape),
                },
                EntryState::Unrelated {
                    observed: ObservedHash::unchecked(&shape),
                },
            ];
            for state in carriers {
                let mut receipt =
                    BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
                receipt.push_client(ClientReport::Observed {
                    client: ClientId::Codex,
                    state,
                });
                assert_eq!(
                    receipt.to_json().expect_err("forged observed state"),
                    ReceiptError::UnsafeField {
                        field: "client.observed"
                    },
                    "shape {shape:?}"
                );
                assert_eq!(
                    receipt.write_to(&state_root).expect_err("forged write"),
                    ReceiptError::UnsafeField {
                        field: "client.observed"
                    },
                    "shape {shape:?}"
                );
            }
            let mut receipt =
                BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
            receipt.push_client(ClientReport::Applied {
                client: ClientId::Cursor,
                result: AdapterResult::Conflict {
                    reason: ConflictReason::ConcurrentChange,
                    observed: Some(ObservedHash::unchecked(&shape)),
                },
            });
            assert_eq!(
                receipt.to_json().expect_err("forged applied conflict"),
                ReceiptError::UnsafeField {
                    field: "client.observed"
                },
                "shape {shape:?}"
            );
            assert_eq!(
                receipt.write_to(&state_root).expect_err("forged write"),
                ReceiptError::UnsafeField {
                    field: "client.observed"
                },
                "shape {shape:?}"
            );
        }
        assert!(!state_root.join(RECEIPT_FILE_NAME).exists());
        assert_eq!(
            std::fs::read_dir(&state_root).expect("state").count(),
            0,
            "no receipt and no temporary may be created for a forged carrier"
        );
    }

    #[test]
    fn the_receipt_wire_path_is_single_and_compile_shaped() {
        // Compile-shape guard: `to_json` is the one function that turns a
        // receipt into a document, pinned by signature here. The public type
        // deliberately derives no `Serialize`; a production wire beyond this
        // path is a reviewed source change, and no behavioral kill is claimed
        // for the mere presence of a derive.
        let wire: fn(&BootstrapReceipt) -> Result<String, ReceiptError> = BootstrapReceipt::to_json;
        let receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        assert!(
            wire(&receipt)
                .expect("wire")
                .contains("\"schema_version\": 1")
        );
    }

    #[test]
    fn from_status_requires_the_requested_release() {
        let statuses: Vec<ArtifactStatus> = ArtifactName::ALL
            .into_iter()
            .map(|name| ArtifactStatus {
                name,
                sha256: Some("a".repeat(64)),
                release: Some("1.0.0".to_owned()),
                mode: Some(0o755),
            })
            .collect();
        let requested = ArtifactManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            release: "2.0.0".to_owned(),
            artifacts: ArtifactName::ALL
                .into_iter()
                .map(|name| ArtifactEntry {
                    name,
                    version: "2.0.0".to_owned(),
                    sha256: "a".repeat(64),
                })
                .collect(),
        };
        let receipt = BootstrapReceipt::from_status(&requested, &statuses);
        assert!(
            receipt
                .artifacts
                .iter()
                .all(|artifact| artifact.disposition == ArtifactDisposition::Absent)
        );
    }

    #[test]
    fn from_status_reports_a_non_executable_artifact_as_mode_drift() {
        let requested = manifest();
        let statuses: Vec<ArtifactStatus> = ArtifactName::ALL
            .into_iter()
            .map(|name| ArtifactStatus {
                name,
                sha256: Some("a".repeat(64)),
                release: Some("1.2.3".to_owned()),
                mode: Some(if name == ArtifactName::Mcp {
                    0o644
                } else {
                    0o755
                }),
            })
            .collect();
        let receipt = BootstrapReceipt::from_status(&requested, &statuses);
        for artifact in &receipt.artifacts {
            let expected = if artifact.name == ArtifactName::Mcp {
                ArtifactDisposition::ModeDrift
            } else {
                ArtifactDisposition::AlreadyCurrent
            };
            assert_eq!(artifact.disposition, expected, "{}", artifact.name);
        }
    }

    #[test]
    fn a_receipt_round_trips() {
        let mut receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Updated);
        receipt.push_client(ClientReport::Observed {
            client: ClientId::Codex,
            state: EntryState::Current {
                observed: ObservedHash::of_bytes(b"entry"),
            },
        });
        receipt.push_client(ClientReport::Applied {
            client: ClientId::Cursor,
            result: AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                observed: Some(ObservedHash::of_bytes(b"observed")),
            },
        });
        receipt.push_client(ClientReport::Failed {
            client: ClientId::Copilot,
            reason: ClientFailure::Confinement,
        });
        let document = receipt.to_json().expect("serialize");
        let parsed: BootstrapReceipt = serde_json::from_str(&document).expect("parse");
        assert_eq!(parsed, receipt);
    }

    #[test]
    fn client_receipts_stay_typed() {
        let receipt = ClientReceipt {
            client: ClientId::OpenCode,
            result: AdapterResult::FailedReadback,
        };
        let document = serde_json::to_string(&receipt).expect("serialize");
        assert!(document.contains("\"failed_readback\""));
    }
}
