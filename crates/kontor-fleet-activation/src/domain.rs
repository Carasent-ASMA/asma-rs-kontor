//! Exact published OperationalDomain bytes, read only through their bundle pin.

use std::path::{Path, PathBuf};

use kontor_core::id::{CanonicalDocument, ContentHash};
use kontor_core::spec::{CodeLifecycle, CoreTeamSeatPurpose};
use kontor_fleet::FleetError;
use kontor_profiles::OperationalDomainPack;
use kontor_teams::CoreTeamRevision;
use serde::{Deserialize, Serialize};

use super::{BundleManifest, Guard, invalid, read_named};

/// Immutable authored domain artifacts, under the existing bundle history owner.
pub const DOMAIN_HISTORY_DIR: &str = "orchestration-history/domains";

/// Largest OperationalDomain source retained and read, in bytes.
pub const MAX_DOMAIN_BYTES: u64 = 1024 * 1024;

/// Exact selected source identity; the manifest's sources map must agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalDomainPin {
    /// Selected-folder-relative YAML source, never a runtime lookup path.
    pub source: String,
    /// SHA-256 of the exact UTF-8 source bytes, including formatting.
    pub source_hash: ContentHash,
}

impl OperationalDomainPin {
    /// Validate the authored reference and its agreement with the manifest.
    ///
    /// # Errors
    /// Names an invalid domain source or mismatched source hash.
    pub fn validate(&self, manifest: &BundleManifest) -> Result<(), FleetError> {
        if self
            .source
            .contains(['\\', ':', '<', '>', '*', '?', '[', ']'])
            || self.source.chars().any(char::is_control)
            || !self
                .source
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..") && part.trim() == part)
            || !matches!(
                Path::new(&self.source).extension().and_then(|s| s.to_str()),
                Some("yml" | "yaml")
            )
        {
            return Err(invalid("operational_domain_source_invalid"));
        }
        if manifest.sources.get(&self.source) != Some(&self.source_hash) {
            return Err(invalid("operational_domain_source_hash_mismatch"));
        }
        Ok(())
    }
}

/// The file rules of an immutable domain artifact.
pub const DOMAIN_GUARD: Guard = Guard {
    symlink: "operational_domain_artifact_not_regular",
    writable: "operational_domain_artifact_writable",
    owner: "operational_domain_artifact_owner_mismatch",
    oversized: "operational_domain_artifact_oversized",
    swapped: "operational_domain_artifact_changed_during_read",
    utf8: "operational_domain_artifact_not_utf8",
    limit: MAX_DOMAIN_BYTES,
    forbidden_mode: 0o022,
};

/// Content-addressed artifact path; the authored source path is never opened here.
#[must_use]
pub fn domain_path(root: &Path, hash: &ContentHash) -> PathBuf {
    root.join(DOMAIN_HISTORY_DIR).join(format!("{hash}.yml"))
}

/// Verify previewed bytes against the descriptor, full domain and frozen Core Team.
/// Publication calls this before exposing a manifest; readers repeat it.
///
/// # Errors
/// Refuses unpinned, mismatched, malformed or contradictory domain/roster facts.
pub fn validate_domain(
    manifest: &BundleManifest,
    roster: &CanonicalDocument,
    text: &str,
) -> Result<OperationalDomainPack, FleetError> {
    let pin = manifest
        .operational_domain
        .as_ref()
        .ok_or_else(|| invalid("operational_domain_unpinned"))?;
    pin.validate(manifest)?;
    if text.len() as u64 > MAX_DOMAIN_BYTES {
        return Err(invalid(DOMAIN_GUARD.oversized));
    }
    if ContentHash::of(text.as_bytes()) != pin.source_hash {
        return Err(invalid("operational_domain_artifact_hash_mismatch"));
    }
    let value: serde_json::Value =
        serde_yaml_ng::from_str(text).map_err(|_| invalid("operational_domain_invalid"))?;
    let domain = kontor_profiles::pack::parse_operational_domain_pack(&value.to_string())
        .map_err(|_| invalid("operational_domain_invalid"))?;
    let catalog = domain
        .role_catalogs
        .iter()
        .find(|catalog| {
            catalog.catalog_id == manifest.role_catalog.catalog_id
                && catalog.version == manifest.role_catalog.version
        })
        .ok_or_else(|| invalid("operational_domain_catalog_mismatch"))?;
    if catalog
        .canonicalize()
        .map_err(|_| invalid("operational_domain_invalid"))?
        .hash()
        != &manifest.role_catalog.content_hash
    {
        return Err(invalid("operational_domain_catalog_mismatch"));
    }
    // Delivery and prompts must agree with the *pinned* catalog, including when
    // the document carries more than one historical catalog revision.
    for code in domain
        .delivery
        .role_bindings
        .iter()
        .map(|binding| &binding.role_code)
        .chain(domain.role_prompts.iter().map(|prompt| &prompt.role_code))
        .chain(
            domain
                .team_definitions
                .iter()
                .filter_map(|definition| definition.pr_review.as_ref())
                .flat_map(|review| review.room_roles.iter().map(|role| &role.role_code)),
        )
    {
        if catalog
            .role(code)
            .is_none_or(|role| role.lifecycle != CodeLifecycle::Current)
        {
            return Err(invalid("operational_domain_catalog_mismatch"));
        }
    }
    for topology in &domain.topology_specs {
        for kind in [
            &domain.delivery.epic_kind,
            &domain.delivery.control_kind,
            &domain.delivery.task_kind,
            &domain.delivery.quick_kind,
            &domain.delivery.advisor_kind,
            &domain.delivery.committee_kind,
        ] {
            if !topology.node_kinds.iter().any(|node| &node.kind == kind) {
                return Err(invalid("operational_domain_topology_mismatch"));
            }
        }
    }
    #[derive(Deserialize)]
    struct Envelope {
        value: CoreTeamRevision,
    }
    let resolved: Envelope = roster
        .deserialize()
        .map_err(|_| invalid("operational_domain_roster_mismatch"))?;
    if roster.hash() != &manifest.core_team_revision_hash
        || resolved.value.catalog_hash != manifest.role_catalog.content_hash
        || resolved.value.seats.iter().any(|seat| {
            seat.role.catalog_id != catalog.catalog_id
                || seat.role.catalog_revision != catalog.version
                || catalog
                    .role(&seat.role.role_code)
                    .is_none_or(|role| role.lifecycle != CodeLifecycle::Current)
                || seat.role.validate_against(catalog).is_err()
        })
    {
        return Err(invalid("operational_domain_roster_mismatch"));
    }
    for (purpose, code) in [
        (
            CoreTeamSeatPurpose::ArchitectureCloser,
            &domain.delivery.architecture_closer_code,
        ),
        (
            CoreTeamSeatPurpose::ProcessCloser,
            &domain.delivery.process_closer_code,
        ),
        (
            CoreTeamSeatPurpose::Control,
            &domain.delivery.control_role_code,
        ),
    ] {
        if !resolved
            .value
            .seat_for_purpose(purpose)
            .is_ok_and(|seat| &seat.role.role_code == code)
        {
            return Err(invalid("operational_domain_purpose_mismatch"));
        }
    }
    Ok(domain)
}

/// Read and verify exact domain bytes through a pinned manifest and roster.
///
/// # Errors
/// Missing pins/artifacts and file, hash, domain or reference inconsistencies refuse.
pub fn read_domain(
    root: &Path,
    manifest: &BundleManifest,
    roster: &CanonicalDocument,
) -> Result<OperationalDomainPack, FleetError> {
    let pin = manifest
        .operational_domain
        .as_ref()
        .ok_or_else(|| invalid("operational_domain_unpinned"))?;
    pin.validate(manifest)?;
    let text = read_named(
        root,
        &domain_path(root, &pin.source_hash),
        &DOMAIN_GUARD,
        "operational_domain_artifact_missing",
    )?;
    validate_domain(manifest, roster, &text)
}
