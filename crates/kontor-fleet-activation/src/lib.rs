//! `kontor-fleet-activation` — read one activated fleet policy bundle, and
//! nothing else, without the daemon.
//!
//! The daemon's placement path and the `kontor` CLI's local resolution read the
//! same state-root files through this crate, so the file rules, the record
//! format and the order of verification cannot fork between them. Parsing,
//! validation and resolution stay in [`kontor_fleet`]; this crate adds only
//! the guarded reads and the chain of content addresses:
//!
//! 1. `fleet-activation.json`, the one atomically replaced pointer;
//! 2. for a schema_version 2 record, `orchestration-history/<source-bundle-hash>.json`,
//!    the immutable canonical bundle manifest, which must agree with the record;
//! 3. `fleet-history/<policy-hash>.yml`, admitted only through
//!    [`FleetSnapshot::activated`];
//! 4. for a schema_version 2 record, `core-team-history/<core-team-revision-hash>.json`,
//!    the exact canonical Core Team revision the leadership bindings name,
//!    resolved against the role catalog the manifest pins.
//!
//! It writes nothing, caches nothing and has no legacy behaviour: there is no
//! `fleet.yml` fallback and no last-valid snapshot here. Those are the daemon's.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use kontor_core::id::{CanonicalDocument, ContentHash, RoleCatalogId, RoleSlotId, SpecVersion};
use kontor_fleet::{
    Eligibility, FleetError, FleetResolution, FleetSelection, FleetSnapshot, LeadershipKey,
    MAX_FILE_BYTES,
};
use serde::{Deserialize, Serialize};

/// The generated record naming the activated fleet policy.
pub const FLEET_ACTIVATION_FILE: &str = "fleet-activation.json";

/// Immutable published fleet policies, named by their content hash.
pub const FLEET_HISTORY_DIR: &str = "fleet-history";

/// Immutable canonical Core Team revisions, named by their content hash.
pub const CORE_TEAM_HISTORY_DIR: &str = "core-team-history";

/// Immutable canonical orchestration bundle manifests, named by their hash.
pub const ORCHESTRATION_HISTORY_DIR: &str = "orchestration-history";

/// Largest activation record this build reads, in bytes.
pub const MAX_ACTIVATION_BYTES: u64 = 4 * 1024;

/// Largest bundle manifest this build reads, in bytes.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Largest canonical Core Team revision this build reads, in bytes.
pub const MAX_ROSTER_BYTES: u64 = 256 * 1024;

/// The activation record format of the unmigrated single-policy activation.
pub const ACTIVATION_V1: u32 = 1;

/// The activation record format of an aligned bundle activation.
pub const ACTIVATION_V2: u32 = 2;

/// The only bundle manifest format this build reads.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The stable rule texts a refusal names. Each is the whole answer: no
/// configured value, path or hash is ever echoed.
#[allow(
    missing_docs,
    reason = "each constant is documented by the verbatim text it holds"
)]
pub mod rule {
    pub const P01: &str = "a published fleet policy must be a regular file, not a symlink";
    pub const P02: &str = "a published fleet policy must not be writable by group or others";
    pub const P03: &str = "a published fleet policy must be owned by the state root's owner";
    pub const P04: &str = "a published fleet policy exceeds 256 KiB";
    pub const P05: &str = "a published fleet policy changed while it was being read";
    pub const P06: &str = "a published fleet policy is not UTF-8";

    pub const A01: &str = "fleet-activation.json must be a regular file, not a symlink";
    pub const A02: &str = "fleet-activation.json must not be writable by group or others";
    pub const A03: &str = "fleet-activation.json must be owned by the state root's owner";
    pub const A04: &str = "fleet-activation.json exceeds 4 KiB";
    pub const A05: &str = "fleet-activation.json changed while it was being read";
    pub const A06: &str = "fleet-activation.json is not UTF-8";
    pub const A07: &str = "fleet-activation.json is not a schema_version 1 or 2 activation record";
    pub const A08: &str = "the activated fleet policy is not published";
    pub const A10: &str = "no fleet activation record exists";
    pub const A11: &str = "the activation record and its bundle manifest disagree";

    pub const M01: &str = "a published bundle manifest must be a regular file, not a symlink";
    pub const M02: &str = "a published bundle manifest must not be writable by group or others";
    pub const M03: &str = "a published bundle manifest must be owned by the state root's owner";
    pub const M04: &str = "a published bundle manifest exceeds 64 KiB";
    pub const M05: &str = "a published bundle manifest changed while it was being read";
    pub const M06: &str = "a published bundle manifest is not UTF-8";
    pub const M07: &str = "the activated bundle manifest is not published";
    pub const M08: &str =
        "a published bundle manifest is not canonical or does not hash to its content address";
    pub const M09: &str = "a bundle manifest is not a schema_version 1 manifest";
    pub const M10: &str = "a bundle manifest pins another role catalog than its Core Team revision was resolved against";

    pub const C01: &str = "a published Core Team revision must be a regular file, not a symlink";
    pub const C02: &str = "a published Core Team revision must not be writable by group or others";
    pub const C03: &str = "a published Core Team revision must be owned by the state root's owner";
    pub const C04: &str = "a published Core Team revision exceeds 256 KiB";
    pub const C05: &str = "a published Core Team revision changed while it was being read";
    pub const C06: &str = "a published Core Team revision is not UTF-8";
    pub const C07: &str = "the selected Core Team revision is not published";
    pub const C08: &str =
        "a published Core Team revision is not canonical or does not hash to its content address";

    pub const D01: &str = "a schema_version 1 activation selects no Core Team roster, so it resolves no leadership binding";
    pub const D02: &str =
        "the leadership binding names a Core Team revision the activation does not select";
    pub const D03: &str = "the activated policy binds no chain to this key";
}

use rule::{
    A01, A02, A03, A04, A05, A06, A07, A08, A10, A11, C01, C02, C03, C04, C05, C06, C07, C08, D01,
    D02, D03, M01, M02, M03, M04, M05, M06, M07, M08, M09, M10, P01, P02, P03, P04, P05, P06,
};

/// The refusals one guarded read names, in the order the checks run.
#[derive(Debug, Clone, Copy)]
pub struct Guard {
    /// Not a regular file (a symlink, a directory, a device).
    pub symlink: &'static str,
    /// Writable by group or others.
    pub writable: &'static str,
    /// Owned by someone other than the state root's owner.
    pub owner: &'static str,
    /// Larger than [`Guard::limit`].
    pub oversized: &'static str,
    /// Replaced between the check and the read.
    pub swapped: &'static str,
    /// Not UTF-8.
    pub utf8: &'static str,
    /// Largest accepted size, in bytes.
    pub limit: u64,
}

/// The file rules of a published fleet policy.
pub const POLICY_GUARD: Guard = Guard {
    symlink: P01,
    writable: P02,
    owner: P03,
    oversized: P04,
    swapped: P05,
    utf8: P06,
    limit: MAX_FILE_BYTES,
};

/// The file rules of the activation record.
pub const ACTIVATION_GUARD: Guard = Guard {
    symlink: A01,
    writable: A02,
    owner: A03,
    oversized: A04,
    swapped: A05,
    utf8: A06,
    limit: MAX_ACTIVATION_BYTES,
};

/// The file rules of a published bundle manifest.
pub const MANIFEST_GUARD: Guard = Guard {
    symlink: M01,
    writable: M02,
    owner: M03,
    oversized: M04,
    swapped: M05,
    utf8: M06,
    limit: MAX_MANIFEST_BYTES,
};

/// The file rules of a published Core Team revision.
pub const ROSTER_GUARD: Guard = Guard {
    symlink: C01,
    writable: C02,
    owner: C03,
    oversized: C04,
    swapped: C05,
    utf8: C06,
    limit: MAX_ROSTER_BYTES,
};

/// Read one state-root file under its own refusal texts.
///
/// The file must be a regular file, not writable by group or others, owned by
/// the state root's owner, within the size limit, the same inode when opened
/// as when checked, and UTF-8.
///
/// # Errors
/// [`FleetError::Read`] when the file cannot be read at all (including when it
/// is absent), otherwise the guard's rule for the first check that fails.
pub fn read_guarded(state_root: &Path, path: &Path, guard: &Guard) -> Result<String, FleetError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| FleetError::Read { source })?;
    if !metadata.file_type().is_file() {
        return Err(invalid(guard.symlink));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(invalid(guard.writable));
    }
    let root =
        std::fs::symlink_metadata(state_root).map_err(|source| FleetError::Read { source })?;
    if metadata.uid() != root.uid() {
        return Err(invalid(guard.owner));
    }
    if metadata.len() > guard.limit {
        return Err(invalid(guard.oversized));
    }
    let file = std::fs::File::open(path).map_err(|source| FleetError::Read { source })?;
    let opened = file
        .metadata()
        .map_err(|source| FleetError::Read { source })?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(invalid(guard.swapped));
    }
    let mut bytes = Vec::new();
    file.take(guard.limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| FleetError::Read { source })?;
    if bytes.len() as u64 > guard.limit {
        return Err(invalid(guard.oversized));
    }
    String::from_utf8(bytes).map_err(|_| invalid(guard.utf8))
}

/// The generated activation record: which published policy, and for an
/// aligned activation which bundle and Core Team roster, placement reads.
///
/// A runtime projection, never authoring input. A schema_version 1 record
/// names one policy, exactly as the unmigrated single-policy activation always
/// has; it carries neither of the optional fields, so its bytes are unchanged.
/// A schema_version 2 record names the source bundle and the selected Core
/// Team revision as well, and only it can cover leadership in direct mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetActivation {
    /// The format of this record: [`ACTIVATION_V1`] or [`ACTIVATION_V2`].
    pub schema_version: u32,
    /// The canonical bundle manifest the policy and roster came from (v2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bundle_hash: Option<ContentHash>,
    /// SHA-256 of exactly the activated policy bytes.
    pub policy_hash: ContentHash,
    /// The schema those bytes validate under.
    pub policy_schema_version: u32,
    /// The canonical Core Team revision the leadership bindings name (v2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_team_revision_hash: Option<ContentHash>,
    /// When activation replaced the record.
    pub activated_at: String,
}

impl FleetActivation {
    /// Parse and check one record's bytes.
    ///
    /// # Errors
    /// A-07 for anything but a well-formed v1 or v2 record: unknown fields, a
    /// v1 record naming a bundle or roster, or a v2 record missing either.
    pub fn parse(text: &str) -> Result<Self, FleetError> {
        let record: Self = serde_json::from_str(text).map_err(|_| invalid(A07))?;
        let bundle = record.source_bundle_hash.is_some();
        let roster = record.core_team_revision_hash.is_some();
        match record.schema_version {
            ACTIVATION_V1 if !bundle && !roster => Ok(record),
            ACTIVATION_V2 if bundle && roster => Ok(record),
            _ => Err(invalid(A07)),
        }
    }
}

/// The pinned role catalog a Core Team revision was resolved against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleCatalogPin {
    /// The catalog's stable identity.
    pub catalog_id: RoleCatalogId,
    /// The catalog revision.
    pub version: SpecVersion,
    /// The canonical content hash of that catalog revision.
    pub content_hash: ContentHash,
}

/// The immutable canonical manifest of one published orchestration bundle.
///
/// Its canonical content hash is the `source_bundle_hash` an aligned
/// activation names. It records what the publisher read and what it produced,
/// so an auditor can walk from authoring bytes to the artifacts placement read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    /// The manifest format: [`MANIFEST_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The resolver that validated and resolved the bundle.
    pub resolver: String,
    /// SHA-256 of each authoring source's exact bytes, by bundle-relative path.
    pub sources: BTreeMap<String, ContentHash>,
    /// The published fleet policy.
    pub policy_hash: ContentHash,
    /// The schema that policy validates under.
    pub policy_schema_version: u32,
    /// The role catalog the Core Team revision was resolved against.
    pub role_catalog: RoleCatalogPin,
    /// The published canonical Core Team revision.
    pub core_team_revision_hash: ContentHash,
}

impl BundleManifest {
    /// The canonical document whose hash is the bundle's identity.
    ///
    /// # Errors
    /// M-09 when the manifest names another format or cannot be canonical.
    pub fn canonicalize(&self) -> Result<CanonicalDocument, FleetError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(invalid(M09));
        }
        CanonicalDocument::from_serializable(self).map_err(|_| invalid(M09))
    }
}

/// The path of the published policy `hash`.
#[must_use]
pub fn policy_path(state_root: &Path, hash: &ContentHash) -> PathBuf {
    state_root
        .join(FLEET_HISTORY_DIR)
        .join(format!("{hash}.yml"))
}

/// The path of the published Core Team revision `hash`.
#[must_use]
pub fn roster_path(state_root: &Path, hash: &ContentHash) -> PathBuf {
    state_root
        .join(CORE_TEAM_HISTORY_DIR)
        .join(format!("{hash}.json"))
}

/// The path of the published bundle manifest `hash`.
#[must_use]
pub fn manifest_path(state_root: &Path, hash: &ContentHash) -> PathBuf {
    state_root
        .join(ORCHESTRATION_HISTORY_DIR)
        .join(format!("{hash}.json"))
}

/// Read and check the activation record.
///
/// # Errors
/// A-10 when there is no record, otherwise its file rules and A-07.
pub fn read_activation(state_root: &Path) -> Result<FleetActivation, FleetError> {
    let text = read_named(
        state_root,
        &state_root.join(FLEET_ACTIVATION_FILE),
        &ACTIVATION_GUARD,
        A10,
    )?;
    FleetActivation::parse(&text)
}

/// The bytes published under policy `hash`, under the artifact's file rules.
///
/// # Errors
/// A-08 when nothing is published under `hash`, otherwise the file rules.
pub fn read_policy(state_root: &Path, hash: &ContentHash) -> Result<String, FleetError> {
    read_named(
        state_root,
        &policy_path(state_root, hash),
        &POLICY_GUARD,
        A08,
    )
}

/// The published policy `hash`, re-read, re-hashed and re-validated.
///
/// # Errors
/// As [`read_policy`], then as [`FleetSnapshot::published`].
pub fn verify_policy(state_root: &Path, hash: &ContentHash) -> Result<FleetSnapshot, FleetError> {
    FleetSnapshot::published(hash, &read_policy(state_root, hash)?)
}

/// The published Core Team revision `hash`: canonical bytes, their address,
/// and the shape of a Core Team revision.
///
/// # Errors
/// C-07 when nothing is published under `hash`, its file rules, C-08 when the
/// bytes are not canonical or not that address, and L-01/L-02 when they are
/// not a Core Team revision with unique slots.
pub fn verify_roster(
    state_root: &Path,
    hash: &ContentHash,
) -> Result<CanonicalDocument, FleetError> {
    let text = read_named(
        state_root,
        &roster_path(state_root, hash),
        &ROSTER_GUARD,
        C07,
    )?;
    let roster = CanonicalDocument::from_stored(&text, hash).map_err(|_| invalid(C08))?;
    LeadershipKey::pinned_slots(&roster)?;
    Ok(roster)
}

/// The published bundle manifest `hash`: canonical bytes, their address and
/// the manifest format.
///
/// # Errors
/// M-07 when nothing is published under `hash`, its file rules, M-08 and M-09.
pub fn verify_manifest(
    state_root: &Path,
    hash: &ContentHash,
) -> Result<BundleManifest, FleetError> {
    let text = read_named(
        state_root,
        &manifest_path(state_root, hash),
        &MANIFEST_GUARD,
        M07,
    )?;
    let document = CanonicalDocument::from_stored(&text, hash).map_err(|_| invalid(M08))?;
    let manifest: BundleManifest = document.deserialize().map_err(|_| invalid(M09))?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(invalid(M09));
    }
    Ok(manifest)
}

/// One activation, with every artifact it names verified.
#[derive(Debug)]
pub struct Activated {
    /// The activation record.
    pub record: FleetActivation,
    /// The activated policy, admitted through [`FleetSnapshot::activated`].
    pub policy: FleetSnapshot,
    /// The bundle manifest and selected roster, for a v2 record.
    pub bundle: Option<ActivatedBundle>,
}

/// The aligned half of a v2 activation.
#[derive(Debug)]
pub struct ActivatedBundle {
    /// The manifest the record names.
    pub manifest: BundleManifest,
    /// The exact canonical Core Team revision the manifest and record name.
    pub roster: CanonicalDocument,
}

/// One published bundle with every artifact its manifest names verified.
#[derive(Debug)]
pub struct VerifiedBundle {
    /// The manifest.
    pub manifest: BundleManifest,
    /// The policy it names, admitted through [`FleetSnapshot::activated`]
    /// under the manifest's own hash and schema.
    pub policy: FleetSnapshot,
    /// The exact canonical Core Team revision it names.
    pub roster: CanonicalDocument,
}

/// Verify one published bundle in full: manifest, then policy, then roster,
/// then that the roster was resolved against the role catalog the manifest
/// pins. Activation runs this before it replaces the record, so a record never
/// names a bundle that did not verify when it was written.
///
/// # Errors
/// The first check that fails.
pub fn verify_bundle(state_root: &Path, hash: &ContentHash) -> Result<VerifiedBundle, FleetError> {
    let manifest = verify_manifest(state_root, hash)?;
    let (policy, roster) = verify_contents(state_root, &manifest)?;
    Ok(VerifiedBundle {
        manifest,
        policy,
        roster,
    })
}

/// Read and verify the activation and every artifact it names, in order:
/// record, then (v2) manifest, then policy, then (v2) roster.
///
/// # Errors
/// The first check that fails; A-11 when a v2 record and its manifest name
/// different policy, schema or roster.
pub fn load(state_root: &Path) -> Result<Activated, FleetError> {
    let record = read_activation(state_root)?;
    let Some(bundle_hash) = record.source_bundle_hash.as_ref() else {
        let policy = FleetSnapshot::activated(
            &record.policy_hash,
            record.policy_schema_version,
            &read_policy(state_root, &record.policy_hash)?,
        )?;
        return Ok(Activated {
            record,
            policy,
            bundle: None,
        });
    };
    let manifest = verify_manifest(state_root, bundle_hash)?;
    let agrees = manifest.policy_hash == record.policy_hash
        && manifest.policy_schema_version == record.policy_schema_version
        && Some(&manifest.core_team_revision_hash) == record.core_team_revision_hash.as_ref();
    if !agrees {
        return Err(invalid(A11));
    }
    let (policy, roster) = verify_contents(state_root, &manifest)?;
    Ok(Activated {
        record,
        policy,
        bundle: Some(ActivatedBundle { manifest, roster }),
    })
}

/// The policy and roster one verified manifest names.
fn verify_contents(
    state_root: &Path,
    manifest: &BundleManifest,
) -> Result<(FleetSnapshot, CanonicalDocument), FleetError> {
    let policy = FleetSnapshot::activated(
        &manifest.policy_hash,
        manifest.policy_schema_version,
        &read_policy(state_root, &manifest.policy_hash)?,
    )?;
    let roster = verify_roster(state_root, &manifest.core_team_revision_hash)?;
    let resolved: CatalogPinView = roster.deserialize().map_err(|_| invalid(M10))?;
    let pinned = &manifest.role_catalog;
    let agrees = resolved.value.catalog_hash == pinned.content_hash
        && resolved.value.seats.iter().all(|seat| {
            seat.role.catalog_id == pinned.catalog_id
                && seat.role.catalog_revision == pinned.version
        });
    if !agrees {
        return Err(invalid(M10));
    }
    Ok((policy, roster))
}

/// The catalog facts of a canonical Core Team revision, and nothing else.
#[derive(Deserialize)]
struct CatalogPinView {
    value: CatalogPinRoster,
}

#[derive(Deserialize)]
struct CatalogPinRoster {
    catalog_hash: ContentHash,
    seats: Vec<CatalogPinSeat>,
}

#[derive(Deserialize)]
struct CatalogPinSeat {
    role: CatalogPinRole,
}

#[derive(Deserialize)]
struct CatalogPinRole {
    catalog_id: RoleCatalogId,
    catalog_revision: SpecVersion,
}

impl Activated {
    /// The leadership key of one slot of the selected roster.
    ///
    /// # Errors
    /// D-01 for a v1 activation, which selects no roster; L-02 when the roster
    /// does not pin the slot exactly once.
    pub fn leadership_key(&self, role_slot_id: &RoleSlotId) -> Result<LeadershipKey, FleetError> {
        let bundle = self.bundle.as_ref().ok_or_else(|| invalid(D01))?;
        LeadershipKey::for_pinned_slot(&bundle.roster, role_slot_id)
    }

    /// Resolve one canonical binding key against the activated policy.
    ///
    /// A team, committee or advisor key resolves through
    /// [`FleetSnapshot::resolve`]. A leadership key is never resolved from its
    /// text: its revision hash must be the selected roster's, and the key is
    /// rebuilt from that verified roster and the slot before it is looked up.
    ///
    /// # Errors
    /// V-32 for a key outside the five families, D-01 and D-02 for a leadership
    /// key the activation cannot cover, and D-03 when nothing binds the key.
    pub fn resolve(&self, binding_key: &str) -> Result<FleetResolution, FleetError> {
        if let Some(rest) = binding_key.strip_prefix("leadership/") {
            let mut parts = rest.split('/');
            let (Some(hash), Some(slot), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(invalid(kontor_fleet::rule::V32));
            };
            let hash = ContentHash::parse(hash).map_err(|_| invalid(kontor_fleet::rule::V32))?;
            let slot = RoleSlotId::parse(slot).map_err(|_| invalid(kontor_fleet::rule::V32))?;
            let bundle = self.bundle.as_ref().ok_or_else(|| invalid(D01))?;
            if bundle.roster.hash() != &hash {
                return Err(invalid(D02));
            }
            let key = self.leadership_key(&slot)?;
            return self
                .policy
                .resolve_leadership(&key)
                .ok_or_else(|| invalid(D03));
        }
        if !binding_key.starts_with("team/")
            && !binding_key.starts_with("committee/")
            && !binding_key.starts_with("advisor/")
        {
            return Err(invalid(kontor_fleet::rule::V32));
        }
        self.policy.resolve(binding_key).ok_or_else(|| invalid(D03))
    }
}

/// The direct-mode read: load and verify the activation, resolve one binding
/// and choose under the stated eligibility.
///
/// Fails closed at every step; there is no `fleet.yml` and no last-valid
/// fallback. A choice with no eligible route is returned as the defined block
/// result (`selected` is `None`), which the caller must not launch.
///
/// # Errors
/// As [`load`] and [`Activated::resolve`].
pub fn resolve(
    state_root: &Path,
    binding_key: &str,
    eligibility: &Eligibility,
) -> Result<FleetSelection, FleetError> {
    Ok(load(state_root)?.resolve(binding_key)?.select(eligibility))
}

fn read_named(
    state_root: &Path,
    path: &Path,
    guard: &Guard,
    absent: &'static str,
) -> Result<String, FleetError> {
    match read_guarded(state_root, path, guard) {
        Err(FleetError::Read { source }) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(invalid(absent))
        }
        other => other,
    }
}

fn invalid(rule: &'static str) -> FleetError {
    FleetError::Invalid { rule }
}

#[cfg(test)]
mod tests;
