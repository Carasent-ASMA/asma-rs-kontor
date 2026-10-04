//! A planning pair member's frozen launch context, derived purely from facts
//! the service read (ASMA-8282 B1a).
//!
//! The derivation is staged so the service keeps reading each fact where it
//! always did: the frozen member first, then the epic's pinned Team
//! Definition, then the epic's topology pin, then the build. Each stage takes
//! exactly the fact read before it and refuses in the service's own order.
//! Nothing here reads state; the launch and the recovery derive their context
//! through this one implementation.

use std::collections::BTreeSet;

use kontor_core::DomainError;
use kontor_core::consultation::ConsultationRunId;
use kontor_core::id::{CanonicalDocument, ContentHash, PlanningPairRunId};
use kontor_core::planning_pair::{
    PlanningPairMember, PlanningPairMembers, PlanningPairPin, PlanningPairSlot,
};
use kontor_core::repository::{StoredConsultationRun, StoredConsultationSeat};
use kontor_core::spec::{RoleCatalogRevision, TeamDefinitionSnapshot, TopologySnapshot};
use kontor_core::state::SessionTopologyNode;

use super::{PlanningPairCatalogPin, PlanningPairLaunchContext};
use crate::provenance::{FleetLaunchProvenance, LaunchEligibility};
use crate::workspace::WorkspaceRoot;

/// Why a member's frozen context cannot be derived. The service maps each one
/// to its own refusal; no variant carries wire text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextRefusal {
    /// The run is not a planning pair.
    NotPlanningPair,
    /// The document pin differs from the one the run was frozen under.
    DocumentPinDiffers,
    /// The seat's slot is absent from the frozen placement.
    SeatAbsentFromPlacement,
    /// The seat's route differs from the route its placement froze.
    RouteDiffers,
    /// The seat has no occupancy generation.
    NoOccupancyGeneration,
    /// The frozen run names another placement.
    PlacementDiffers,
    /// The epic has no pinned Team Definition.
    NoPinnedTeamDefinition,
    /// The epic's pinned Team Definition differs from the frozen one.
    TeamDefinitionDiffers,
    /// The container's topology differs from the epic's pinned topology.
    TopologyDiffers,
    /// The frozen placement names no fleet provenance for the member.
    NoFleetProvenance,
}

/// Why the final build failed: a typed refusal, or a domain error from
/// canonicalizing the epic's role catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextBuildError {
    /// A typed refusal.
    Refused(ContextRefusal),
    /// The role catalog could not be canonicalized.
    Domain(DomainError),
}

/// The member as the frozen run, document pin and placement name it: the
/// first stage, before any further fact is read.
#[derive(Debug, Clone, Copy)]
pub struct FrozenMember<'a> {
    run: &'a StoredConsultationRun,
    run_id: PlanningPairRunId,
    pin: &'a PlanningPairPin,
    member: &'a PlanningPairMember,
    seat: &'a StoredConsultationSeat,
    slot: PlanningPairSlot,
    placement_hash: &'a ContentHash,
}

/// Hold one member seat to the frozen run, its document pin and its
/// placement.
///
/// # Errors
/// In order: not a planning pair, another document pin, a slot absent from
/// the placement, another route, no occupancy generation, another placement.
pub fn frozen_member<'a>(
    run: &'a StoredConsultationRun,
    pin: &'a PlanningPairPin,
    members: &'a PlanningPairMembers,
    placement_hash: &'a ContentHash,
    seat: &'a StoredConsultationSeat,
    slot: PlanningPairSlot,
) -> Result<FrozenMember<'a>, ContextRefusal> {
    let ConsultationRunId::PlanningPair(run_id) = run.id else {
        return Err(ContextRefusal::NotPlanningPair);
    };
    if pin.profile_id.to_string() != run.profile_id
        || pin.version != run.profile_version
        || pin.definition_hash != run.definition_hash
    {
        return Err(ContextRefusal::DocumentPinDiffers);
    }
    let member = members
        .members()
        .iter()
        .find(|member| member.slot == slot)
        .ok_or(ContextRefusal::SeatAbsentFromPlacement)?;
    if member.route != seat.model_rung {
        return Err(ContextRefusal::RouteDiffers);
    }
    if seat.occupancy_generation == 0 {
        return Err(ContextRefusal::NoOccupancyGeneration);
    }
    if frozen(run, "placement_hash") != Some(serde_json::json!(placement_hash.as_str())) {
        return Err(ContextRefusal::PlacementDiffers);
    }
    Ok(FrozenMember {
        run,
        run_id,
        pin,
        member,
        seat,
        slot,
        placement_hash,
    })
}

fn frozen(run: &StoredConsultationRun, key: &str) -> Option<serde_json::Value> {
    run.context.get(key).cloned()
}

impl FrozenMember<'_> {
    /// The second stage: the epic's pinned Team Definition, as read now, is
    /// the one the run was frozen under.
    ///
    /// # Errors
    /// No pinned Team Definition, then a different one.
    pub fn require_team_definition(
        &self,
        pinned: Option<TeamDefinitionSnapshot>,
    ) -> Result<TeamDefinitionSnapshot, ContextRefusal> {
        let pinned = pinned.ok_or(ContextRefusal::NoPinnedTeamDefinition)?;
        if frozen(self.run, "team_definition_id")
            != Some(serde_json::json!(pinned.definition_id.to_string()))
            || frozen(self.run, "team_definition_version")
                != Some(serde_json::json!(pinned.version.get()))
            || frozen(self.run, "team_definition_hash")
                != Some(serde_json::json!(pinned.canonical_hash.as_str()))
        {
            return Err(ContextRefusal::TeamDefinitionDiffers);
        }
        Ok(pinned)
    }

    /// The final stage: the context itself, from the member container's node
    /// and cwd, the epic's role catalog and the member's requested
    /// provenance.
    ///
    /// # Errors
    /// The catalog's canonicalization, then a missing fleet provenance.
    pub fn into_context(
        self,
        team_definition: TeamDefinitionSnapshot,
        node: &SessionTopologyNode,
        cwd: &WorkspaceRoot,
        catalog: &RoleCatalogRevision,
        requested: Option<&FleetLaunchProvenance>,
    ) -> Result<PlanningPairLaunchContext, ContextBuildError> {
        let catalog_hash = catalog
            .canonicalize()
            .map_err(ContextBuildError::Domain)?
            .hash()
            .clone();
        let requested_fleet_provenance = requested.cloned().ok_or(ContextBuildError::Refused(
            ContextRefusal::NoFleetProvenance,
        ))?;
        Ok(PlanningPairLaunchContext {
            run_id: self.run_id,
            seat_binding_id: self.seat.seat_binding_id,
            slot: self.slot,
            occupancy_generation: self.seat.occupancy_generation,
            profile: self.pin.clone(),
            topology: node.topology.clone(),
            team_definition,
            role_catalog: PlanningPairCatalogPin {
                catalog_id: catalog.catalog_id,
                version: catalog.version,
                canonical_hash: catalog_hash,
            },
            topology_node_id: node.id,
            cwd: cwd.clone(),
            route: self.seat.model_rung.clone(),
            vendor: self.member.vendor.clone(),
            placement_hash: self.placement_hash.clone(),
            requested_fleet_provenance,
        })
    }
}

/// The third stage: the member container's topology, as read now, is the
/// epic's pinned topology.
///
/// # Errors
/// [`ContextRefusal::TopologyDiffers`] when it is not.
pub fn require_container_topology(
    epic_pin: &TopologySnapshot,
    node: &SessionTopologyNode,
) -> Result<(), ContextRefusal> {
    if *epic_pin == node.topology {
        Ok(())
    } else {
        Err(ContextRefusal::TopologyDiffers)
    }
}

impl PlanningPairLaunchContext {
    /// The canonical hash of this frozen context, which a member's known
    /// native claim and its recovery intent both name. Nothing in it is
    /// secret.
    ///
    /// # Errors
    /// The document's canonicalization.
    pub fn frozen_hash(&self) -> Result<ContentHash, DomainError> {
        let document = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "planning_pair_run_id": self.run_id.to_string(),
            "seat_binding_id": self.seat_binding_id.to_string(),
            "slot": self.slot.as_str(),
            "occupancy_generation": self.occupancy_generation,
            "profile": [
                self.profile.profile_id.to_string(),
                self.profile.version.get(),
                self.profile.definition_hash.as_str(),
            ],
            "topology": [
                self.topology.spec_id.to_string(),
                self.topology.version.get(),
                self.topology.canonical_hash.as_str(),
            ],
            "team_definition": [
                self.team_definition.definition_id.to_string(),
                self.team_definition.version.get(),
                self.team_definition.canonical_hash.as_str(),
            ],
            "role_catalog": [
                self.role_catalog.catalog_id.to_string(),
                self.role_catalog.version.get(),
                self.role_catalog.canonical_hash.as_str(),
            ],
            "topology_node_id": self.topology_node_id.to_string(),
            "cwd": self.cwd.as_str(),
            "route": self.route,
            "vendor": self.vendor,
            "placement_hash": self.placement_hash.as_str(),
            "requested_fleet_provenance": self.requested_fleet_provenance,
        }))?;
        Ok(document.hash().clone())
    }
}

/// The fleet provenance one member's launch requests, read from the frozen
/// shared-allocator receipt and never re-derived.
#[must_use]
pub fn requested_fleet_provenance(
    receipt: &serde_json::Value,
    slot: PlanningPairSlot,
) -> Option<FleetLaunchProvenance> {
    let selection = receipt.get("selection")?;
    let provenance = selection.get("provenance")?;
    let member = selection
        .get("slots")?
        .as_array()?
        .iter()
        .find(|candidate| {
            candidate.get("slot_id").and_then(serde_json::Value::as_str) == Some(slot.as_str())
        })?;
    let selected = member.get("selected")?;
    let strings = |value: Option<&serde_json::Value>| -> BTreeSet<String> {
        value
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    Some(FleetLaunchProvenance {
        policy_hash: ContentHash::parse(provenance.get("policy_hash")?.as_str()?).ok()?,
        source_bundle_hash: provenance
            .get("source_bundle_hash")
            .and_then(serde_json::Value::as_str)
            .and_then(|hash| ContentHash::parse(hash).ok()),
        binding_key: member.get("binding_key")?.as_str()?.to_owned(),
        chain: member.get("chain")?.as_str()?.to_owned(),
        step: u16::try_from(selected.get("step")?.as_u64()?).ok()?,
        sub_step: u16::try_from(selected.get("sub_step")?.as_u64()?).ok()?,
        vendor: selected.get("vendor")?.as_str()?.to_owned(),
        eligibility: Some(LaunchEligibility {
            unavailable_accounts: strings(
                member
                    .get("eligibility")
                    .and_then(|value| value.get("unavailable_accounts")),
            ),
            excluded_vendors: strings(
                member
                    .get("eligibility")
                    .and_then(|value| value.get("excluded_vendors")),
            ),
        }),
    })
}

#[cfg(test)]
mod tests;
