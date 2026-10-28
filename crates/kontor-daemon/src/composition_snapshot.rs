//! Pure, read-only composition preview over canonical component inputs.
//!
//! The component preview retains supplied pins without authenticating them.
//! The selected-folder preview reuses the configuration owner's loader and
//! captured pack pins. Neither establishes native identity or write authority.

use std::collections::BTreeMap;
use std::path::Path;

use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalId, RoleSlotId, SCHEMA_VERSION, Timestamp,
};
use kontor_core::naming::{NameSeparator, NativeNameTemplate, NativeNameValues};
use kontor_core::{DomainError, DomainResult};
use kontor_fleet::{Eligibility, FleetSnapshot};
use kontor_profiles::pack::{PackCategoryKey, ProfilePackSpec, RolePrompt, resolve_profile};
use serde::Serialize;
use serde_json::json;

use crate::config_folder::{ConfigError, ConfigFolder, ENTRY};

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

/// Explicit subject and naming inputs for a selected-folder preview.
///
/// Profile, role, fleet key, persona and skill references come from the selected
/// configuration. Subject IDs and naming remain caller assertions, not evidence
/// that a project, workspace, parent or seat exists.
#[derive(Debug, Clone)]
pub struct SelectedCompositionInput {
    /// Supplied native IDs and the requested team slot.
    pub subject: CompositionSubject,
    /// Explicit profile category; no category is guessed from a folder or title.
    pub category: PackCategoryKey,
    /// Candidate naming template; topology ownership is not established here.
    pub template: NativeNameTemplate,
    /// Candidate naming separator.
    pub separator: NameSeparator,
    /// Candidate values; the role code is replaced with the selected slot's role.
    pub values: NativeNameValues,
    /// Exclusions only, not positive host availability or route qualification.
    pub eligibility: Eligibility,
    /// Explicit evaluation time; no ambient clock read.
    pub resolved_at: Timestamp,
}

/// Read a selected folder through WP08's loader and preview one configured slot.
///
/// The explicit flag wins over the supplied environment selection, as it does
/// for daemon startup. This function never reads the process environment.
/// Captured pack hashes describe the bytes parsed by the pack owner. Fleet is
/// read separately by its owner: this is NOT an atomic folder-generation pin,
/// an activated policy, a registered project or a native creation capability.
/// An absent optional persona stays absent; no default persona is invented.
///
/// # Errors
/// Preserves the loader's named refusals for absent/invalid folders, and refuses
/// unresolved profiles, slots, role mappings, fleet bindings or naming inputs.
/// Errors name the selected source without including its document contents.
pub fn preview_selected_composition(
    flag: Option<&Path>,
    environment: Option<&Path>,
    input: &SelectedCompositionInput,
) -> Result<CompositionSnapshot, ConfigError> {
    let folder = ConfigFolder::select(flag, environment)?;
    let invalid = |problem| ConfigError {
        path: folder.root().join(ENTRY),
        problem,
    };
    let pack = folder
        .realm_pack
        .as_ref()
        .ok_or_else(|| invalid("composition requires project.team_pack"))?;
    let profile = resolve_profile(&pack.profiles, &input.category, input.resolved_at)
        .map_err(|_| invalid("composition profile cannot be resolved"))?;
    profile
        .verify()
        .map_err(|_| invalid("composition profile does not verify"))?;
    let revision = profile
        .team
        .as_ref()
        .ok_or_else(|| invalid("composition profile has no team template"))?;
    let team = kontor_teams::spec::TeamTemplateSpec::from_revision(revision)
        .map_err(|_| invalid("composition team revision does not verify"))?;
    let slot = team
        .slots
        .iter()
        .find(|slot| slot.id == input.subject.slot)
        .ok_or_else(|| invalid("composition slot is not in the selected profile team"))?;
    let role = pack
        .domain
        .delivery
        .role_code(&slot.role.role)
        .ok_or_else(|| invalid("composition slot has no operational role binding"))?;
    let binding = kontor_fleet::team_key(&team.template_id.to_string(), slot.id.as_str());
    let fleet = folder.fleet_snapshot()?;
    let resolution = fleet
        .resolve(&binding)
        .ok_or_else(|| invalid("composition slot has no selected fleet binding"))?;
    let values = input.values.clone().with_role_code(role.as_str());
    let title = input
        .template
        .render(&input.separator, &values)
        .map_err(|_| invalid("composition naming inputs cannot be rendered"))?;
    let persona = match pack.domain.role_prompt(role) {
        Some(prompt) => json!({
            "state": "planned", "role": role,
            "prompt_hash": ContentHash::of(prompt.as_str().as_bytes()),
        }),
        None => json!({"state": "not_configured", "role": role}),
    };
    let project_id = folder
        .project
        .publication_policy
        .as_ref()
        .map(|policy| policy.project_id);
    let project = json!({
        "schema_version": SCHEMA_VERSION,
        "configured_project_id": project_id,
        "tracker_key": folder.project.key,
        "resource_roots": folder.project.resource_roots,
        "registration": "not_observed",
    });
    let project_semantics = CanonicalDocument::from_value(&project)
        .map_err(|_| invalid("composition project cannot be canonicalized"))?;
    let mut write_refusals = vec![
        "seat_bound_issuer_not_qualified",
        "project_workspace_authority_not_observed",
        "tenant_binding_not_supplied",
    ];
    if project_id.is_none() {
        write_refusals.push("internal_project_id_not_configured");
    }
    let document = CanonicalDocument::from_value(&json!({
        "schema_version": SCHEMA_VERSION,
        "contract": "selected-composition-preview@1",
        "coverage": "selected_pack_team_slot_projection",
        "subject": input.subject,
        "project": project,
        "capture": {
            "pack_sources": pack.sources,
            "fleet_bytes": fleet.hash(),
            "project_semantics": project_semantics.hash(),
            "atomic_folder_generation": false,
        },
        "naming": {"template": input.template, "separator": input.separator, "title": title},
        "profile": profile,
        "slot": slot,
        "fleet_binding": binding,
        "fleet": resolution.select(&input.eligibility),
        "attachments": {
            "persona": persona,
            "skills": {"state": "planned", "references": slot.skills},
        },
        "unverified": [
            "atomic_selected_folder_generation", "policy_activation", "project_registration",
            "native_identity", "topology_roster_parentage", "naming_authority",
            "host_route_qualification", "skill_content", "effective_loading", "caller_authority",
        ],
        "write_refusals": write_refusals,
        "native_actuation_authorized": false,
        "effective_loading": "not_observed",
    }))
    .map_err(|_| invalid("composition cannot be canonicalized"))?;
    Ok(CompositionSnapshot { document })
}
