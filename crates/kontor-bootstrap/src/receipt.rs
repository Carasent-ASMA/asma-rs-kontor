//! Typed, redacted bootstrap receipts.
//!
//! A receipt is the durable answer to "what did bootstrap do": one disposition
//! per artifact and per client. It carries names, versions, digests, typed
//! states and typed reasons only — never a path, an argv, a credential value or
//! any secret material. The types make that structural rather than a promise.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::artifact::install::{ArtifactStatus, InstallOutcome};
use crate::artifact::manifest::{ArtifactManifest, ArtifactName};
use crate::client::{AdapterResult, ClientId, EntryState};
use crate::service::ServiceReceipt;

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
}

/// One whole bootstrap run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Why a receipt could not be produced or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
    /// The receipt could not be serialized.
    #[error("the bootstrap receipt could not be serialized")]
    Serialize,
    /// The state root is relative, which bootstrap refuses.
    #[error("the state root must be an absolute path")]
    RelativeStateRoot,
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
                let current = statuses
                    .iter()
                    .find(|status| status.name == entry.name)
                    .is_some_and(|status| status.is_current(manifest));
                ArtifactReceipt {
                    name: entry.name,
                    version: entry.version.clone(),
                    sha256: entry.sha256.clone(),
                    disposition: if current {
                        ArtifactDisposition::AlreadyCurrent
                    } else {
                        ArtifactDisposition::Absent
                    },
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

    /// The one-document JSON form.
    ///
    /// # Errors
    /// [`ReceiptError::Serialize`].
    pub fn to_json(&self) -> Result<String, ReceiptError> {
        serde_json::to_string_pretty(self).map_err(|_| ReceiptError::Serialize)
    }

    /// Write the receipt under an injected state root.
    ///
    /// # Errors
    /// A relative state root, or a filesystem failure.
    pub fn write_to(&self, state_root: &Path) -> Result<PathBuf, ReceiptError> {
        if !state_root.is_absolute() {
            return Err(ReceiptError::RelativeStateRoot);
        }
        std::fs::create_dir_all(state_root).map_err(|_| ReceiptError::Io)?;
        let path = state_root.join(RECEIPT_FILE_NAME);
        let document = self.to_json()?;
        std::fs::write(&path, document).map_err(|_| ReceiptError::Io)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::manifest::MANIFEST_SCHEMA_VERSION;
    use crate::client::{ClientReceipt, ConflictReason, ObservedHash};

    fn manifest() -> ArtifactManifest {
        ArtifactManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            release: "1.2.3".to_owned(),
            artifacts: ArtifactName::ALL
                .into_iter()
                .map(|name| crate::artifact::manifest::ArtifactEntry {
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
        let document = receipt.to_json().expect("serialize");
        assert!(document.contains("\"schema_version\": 1"));
        assert!(document.contains("\"installed\""));
        assert!(document.contains("\"concurrent_change\""));
        assert!(document.contains("\"client_absent\""));
        for forbidden in ["/Users", "/home", "program", "args", "KONTOR_AUTH"] {
            assert!(!document.contains(forbidden), "receipt leaked {forbidden}");
        }
    }

    #[test]
    fn a_relative_state_root_is_refused() {
        let receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Installed);
        assert_eq!(
            receipt.write_to(Path::new("relative")),
            Err(ReceiptError::RelativeStateRoot)
        );
    }

    #[test]
    fn a_receipt_round_trips() {
        let receipt = BootstrapReceipt::from_install(&manifest(), InstallOutcome::Updated);
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
