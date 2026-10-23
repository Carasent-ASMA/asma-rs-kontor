//! Pure, read-only composition preview over canonical component inputs.
//!
//! This projection does not accept folders or establish capture provenance,
//! native identity, topology, activation or write authority. Those boundaries
//! remain with their owners. Supplied pins are retained, never authenticated.

use std::collections::BTreeMap;

use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalId, RoleSlotId, SCHEMA_VERSION, Timestamp,
};
use kontor_core::naming::{NameSeparator, NativeNameTemplate, NativeNameValues};
use kontor_core::{DomainError, DomainResult};
use kontor_fleet::{Eligibility, FleetSnapshot};
use kontor_profiles::pack::{PackCategoryKey, ProfilePackSpec, RolePrompt, resolve_profile};
use serde::Serialize;
use serde_json::json;

/// Exact subject identifiers supplied by the caller, not identity attestation.
#[derive(Debug, Clone, Serialize)]
pub struct CompositionSubject {
    /// Native project ID; never a title or cwd.
    pub project: ExternalId,
    /// Native workspace ID.
    pub workspace: ExternalId,
    /// Expected parent ID, with absence explicit.
    pub parent: Option<ExternalId>,
    /// Declared role slot.
    pub slot: RoleSlotId,
}

/// Canonical values supplied to a preview; no input grants runtime authority.
#[derive(Debug, Clone)]
pub struct CompositionInput {
    /// Exact subject context to bind into the result.
    pub subject: CompositionSubject,
    /// Explicit profile category resolved by the profile owner.
    pub category: PackCategoryKey,
    /// Explicit fleet binding resolved by the fleet owner.
    pub fleet_binding: String,
    /// Typed naming template.
    pub template: NativeNameTemplate,
    /// Typed name separator.
    pub separator: NameSeparator,
    /// Naming values; their identity/topology provenance is unverified here.
    pub values: NativeNameValues,
    /// Intended role persona; no prompt content is emitted by the preview.
    pub persona: RolePrompt,
    /// Explicit availability and vendor exclusions.
    pub eligibility: Eligibility,
    /// Explicit evaluation time; the preview never reads the clock.
    pub resolved_at: Timestamp,
    /// Optional supplied source pins keyed by logical source identifier.
    /// Empty means absent, not a default source. No file is read or accepted.
    pub source_pins: BTreeMap<ExternalId, ContentHash>,
    /// Optional supplied policy pins; presence does not establish activation.
    pub policy_pins: BTreeMap<ExternalId, ContentHash>,
}

/// Immutable canonical preview and its digest, never a creation capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionSnapshot {
    document: CanonicalDocument,
}

impl CompositionSnapshot {
    /// The canonical projection, including coverage limits and planned inputs.
    #[must_use]
    pub fn document(&self) -> &CanonicalDocument {
        &self.document
    }

    /// Digest of the complete canonical document, excluding only this digest.
    #[must_use]
    pub fn hash(&self) -> &ContentHash {
        self.document.hash()
    }
}

/// Compose a partial preview using the existing profile, fleet and naming owners.
///
/// Inputs must already have been captured/parsed by their owners. This function
/// neither substitutes for accepted capture nor verifies supplied pins against
/// a folder. A no-route fleet selection remains an explicit blocked selection.
/// The output always denies native actuation and marks effective loading unknown.
///
/// # Errors
/// Returns the existing component validation or canonicalization error, or an
/// invalid-input error when the explicit fleet binding does not exist.
pub fn preview_composition(
    input: &CompositionInput,
    profiles: &ProfilePackSpec,
    fleet: &FleetSnapshot,
) -> DomainResult<CompositionSnapshot> {
    let profile = resolve_profile(profiles, &input.category, input.resolved_at)?;
    profile.verify()?;
    let title = input.template.render(&input.separator, &input.values)?;
    let resolution = fleet
        .resolve(&input.fleet_binding)
        .ok_or_else(|| DomainError::invalid("CompositionSnapshot", "unknown fleet binding"))?;
    let selection = resolution.select(&input.eligibility);
    let profile_source = CanonicalDocument::from_serializable(profiles)?;
    let document = CanonicalDocument::from_value(&json!({
        "schema_version": SCHEMA_VERSION,
        "contract": "composition-preview@1",
        "coverage": "profile_fleet_naming_projection",
        "subject": &input.subject,
        "inputs": { "profile_semantics": profile_source.hash(), "fleet_bytes": fleet.hash() },
        "source_pins": &input.source_pins,
        "policy_pins": &input.policy_pins,
        "naming": { "template": &input.template, "separator": &input.separator, "title": title },
        "profile": &profile,
        "fleet": selection,
        "attachments": {
            "persona": {
                "state": "planned",
                "role": &input.persona.role_code,
                "prompt_hash": ContentHash::of(input.persona.prompt.as_str().as_bytes()),
            },
            "skills": { "state": "planned", "references": &profile.skills },
        },
        "unverified": [
            "selected_folder_capture", "supplied_pins_and_activation", "native_identity",
            "topology_roster_role_join", "persona_catalog_binding", "host_route_qualification",
            "skill_content", "effective_loading", "caller_authority",
        ],
        "native_actuation_authorized": false,
        "effective_loading": "not_observed",
    }))?;
    Ok(CompositionSnapshot { document })
}
