//! Q-04 sources retained by the existing bundle publisher, never read from live YAML.

use std::path::{Path, PathBuf};

use kontor_core::id::{ContentHash, parse_utc_timestamp};
use kontor_fleet::host_routes::{QualificationSet, RuntimeMatrix};
use kontor_fleet::{FleetError, FleetSnapshot};
use serde::{Deserialize, Serialize};

use super::{BundleManifest, FleetActivation, Guard, invalid, read_named};

/// The one authored qualification source, relative to the selected folder.
pub const QUALIFICATION_SOURCE: &str = "orchestration/route-qualifications.yml";
/// Canonical source key already hash-bound by each new bundle's sources map.
pub const PROJECT_SOURCE: &str = "orchestration/orchestration.yml";
/// Retained source artifacts share the existing bundle history authority.
pub const SOURCE_HISTORY_DIR: &str = "orchestration-history/sources";

/// Optional exact source pin. Activation time belongs only to FleetActivation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationPin {
    /// Selected-folder-relative source, never opened by the runtime reader.
    pub source: String,
    /// SHA-256 of the exact retained UTF-8 bytes.
    pub source_hash: ContentHash,
}

impl QualificationPin {
    /// Check exact source spelling and agreement with the manifest's sources map.
    ///
    /// # Errors
    /// Conflicting/malformed descriptor refuses by name.
    pub fn validate(&self, manifest: &BundleManifest) -> Result<(), FleetError> {
        if self.source != QUALIFICATION_SOURCE {
            return Err(invalid("route_qualification_malformed"));
        }
        if manifest.sources.get(&self.source) != Some(&self.source_hash) {
            return Err(invalid("route_qualification_hash_mismatch"));
        }
        Ok(())
    }
}

const SOURCE_GUARD: Guard = Guard {
    symlink: "host_route_source_not_regular",
    writable: "host_route_source_writable",
    owner: "host_route_source_owner_mismatch",
    oversized: "host_route_source_oversized",
    swapped: "host_route_source_changed_during_read",
    utf8: "host_route_source_not_utf8",
    limit: 1024 * 1024,
    forbidden_mode: 0o022,
};

/// Content address; the author's mutable source path is never a readback path.
#[must_use]
pub fn source_path(root: &Path, hash: &ContentHash) -> PathBuf {
    root.join(SOURCE_HISTORY_DIR).join(format!("{hash}.yml"))
}

/// Read exact hash-bound retained bytes through existing artifact guards.
///
/// # Errors
/// Missing, unsafe, tampered or unsupported source refuses without current-file repair.
pub fn read_source(root: &Path, hash: &ContentHash) -> Result<String, FleetError> {
    let text = read_named(
        root,
        &source_path(root, hash),
        &SOURCE_GUARD,
        "host_route_source_missing",
    )?;
    if ContentHash::of(text.as_bytes()) != *hash {
        return Err(invalid("host_route_source_hash_mismatch"));
    }
    Ok(text)
}

/// Matrix authority from one pinned immutable orchestration source only.
///
/// # Errors
/// Historical bundles without the exact source pin remain readable, but cannot grant routes.
pub fn read_matrix(root: &Path, manifest: &BundleManifest) -> Result<RuntimeMatrix, FleetError> {
    let hash = manifest
        .sources
        .get(PROJECT_SOURCE)
        .ok_or_else(|| invalid("host_route_source_unpinned"))?;
    RuntimeMatrix::parse(&read_source(root, hash)?)
}

/// Candidate qualification validation, not an activation grant.
///
/// # Errors
/// Descriptor/source/record hash and canonical identity disagreements refuse.
pub fn validate_qualifications(
    manifest: &BundleManifest,
    policy: &FleetSnapshot,
    text: &str,
) -> Result<QualificationSet, FleetError> {
    let pin = manifest
        .qualification
        .as_ref()
        .ok_or_else(|| invalid("route_qualification_unproven"))?;
    pin.validate(manifest)?;
    if policy.hash() != &manifest.policy_hash {
        return Err(invalid("route_qualification_unproven"));
    }
    if ContentHash::of(text.as_bytes()) != pin.source_hash {
        return Err(invalid("route_qualification_hash_mismatch"));
    }
    QualificationSet::parse(text, policy)
}

/// Private activation projection joins the existing activation event to its exact manifest.
///
/// # Errors
/// A different/unlinked activation, missing/tampered bytes or invalid records refuse.
pub fn read_qualifications(
    root: &Path,
    manifest: &BundleManifest,
    policy: &FleetSnapshot,
    activation: &FleetActivation,
) -> Result<Option<QualificationSet>, FleetError> {
    let Some(pin) = &manifest.qualification else {
        return Ok(None);
    };
    pin.validate(manifest)?;
    if activation.source_bundle_hash.as_ref() != Some(manifest.canonicalize()?.hash())
        || activation.policy_hash != manifest.policy_hash
        || policy.hash() != &manifest.policy_hash
    {
        return Err(invalid("route_qualification_unproven"));
    }
    let at = parse_utc_timestamp(&activation.activated_at)
        .map_err(|_| invalid("route_qualification_unproven"))?;
    let text = read_source(root, &pin.source_hash)?;
    Ok(Some(QualificationSet::activated(
        &text,
        &pin.source_hash,
        policy,
        at,
    )?))
}
