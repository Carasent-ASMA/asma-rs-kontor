//! A planning pair member launch at the runtime boundary (ASMA-8282 D-3).
//!
//! A member is launched through the ordinary consultation port, carrying one
//! additional, typed and non-secret value: everything the launch is frozen to,
//! as the service derived it from durable state. A runtime holds the request
//! to that value before any native effect — a request whose run, seat, slot,
//! route, workspace or provenance disagrees with it is refused — and an
//! Advisor or Committee launch that carries one is refused too, so the two
//! families never borrow each other's surface.
//!
//! Nothing here is secret. The member's credential stays the request's opaque
//! [`crate::adapter::ScopedSeatCredential`], delivered only through the
//! runtime's process-environment channel; its occupancy generation is the one
//! stated here, taken from the frozen seat, never a default and never read
//! out of the secret.

use kontor_core::consultation::ConsultationRunId;
use kontor_core::id::{
    ContentHash, PlanningPairRunId, RoleCatalogId, SeatBindingId, SpecVersion, TopologyNodeId,
};
use kontor_core::planning_pair::{PlanningPairPin, PlanningPairSlot};
use kontor_core::spec::{ModelRung, TeamDefinitionSnapshot, TopologySnapshot};

use crate::adapter::{
    ConsultationLaunchRequest, ConsultationRouteProvenance, RuntimeError, RuntimeResult,
};
use crate::provenance::FleetLaunchProvenance;
use crate::workspace::WorkspaceRoot;

/// The role catalog revision a member's registered role was resolved in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairCatalogPin {
    /// Catalog identity.
    pub catalog_id: RoleCatalogId,
    /// The selected revision.
    pub version: SpecVersion,
    /// Canonical hash of that revision's persisted bytes.
    pub canonical_hash: ContentHash,
}

/// Everything one planning pair member launch is frozen to.
///
/// Derived by the service from the frozen run, seat, placement and pins, and
/// never from the credential, a caller-supplied label or a default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairLaunchContext {
    /// The planning pair run, in its own family.
    pub run_id: PlanningPairRunId,
    /// The exact member SeatBinding.
    pub seat_binding_id: SeatBindingId,
    /// The member's slot.
    pub slot: PlanningPairSlot,
    /// The seat's actual occupancy generation, which its credential is minted
    /// for.
    pub occupancy_generation: u64,
    /// The pinned document revision.
    pub profile: PlanningPairPin,
    /// The topology revision the member's container node is pinned to.
    pub topology: TopologySnapshot,
    /// The epic's pinned Team Definition revision.
    pub team_definition: TeamDefinitionSnapshot,
    /// The role catalog the epic selected.
    pub role_catalog: PlanningPairCatalogPin,
    /// The member container's topology node.
    pub topology_node_id: TopologyNodeId,
    /// The member container's working directory.
    pub cwd: WorkspaceRoot,
    /// The frozen route.
    pub route: ModelRung,
    /// The actual vendor the shared allocator placed the route on.
    pub vendor: String,
    /// The canonical hash of the shared allocator's placement receipt.
    pub placement_hash: ContentHash,
    /// The fleet provenance this member's launch requests.
    pub requested_fleet_provenance: FleetLaunchProvenance,
}

/// One member's frozen route, as a runtime is asked whether it can compose the
/// closed member surface for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairMemberRoute {
    /// The member's slot.
    pub slot: PlanningPairSlot,
    /// The frozen route.
    pub model_rung: ModelRung,
}

impl PlanningPairMemberRoute {
    /// Refuse anything but the pair's two routes, `seat-a` then `seat-b`.
    ///
    /// # Errors
    /// [`RuntimeError::LaunchNotAdmitted`] for any other list.
    pub fn require_pair(routes: &[Self]) -> RuntimeResult<()> {
        let slots: Vec<PlanningPairSlot> = routes.iter().map(|route| route.slot).collect();
        if slots != PlanningPairSlot::ALL {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a planning pair surface is asked about its two routes, seat-a then seat-b",
            });
        }
        Ok(())
    }
}

/// Whether one field of a member's native surface was read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberSurfaceField {
    /// Read back and equal to what was requested.
    Matched,
    /// No surface of this runtime reports it, so nothing is claimed about it.
    Unsupported,
}

/// What a runtime observed of one planning pair member's native surface.
///
/// Every field is a readback. A field the runtime cannot report is stated as
/// [`MemberSurfaceField::Unsupported`], never as matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairMemberObservation {
    /// The surface it was read from. A fake runtime's or a recorded fixture's
    /// surface is source-contract evidence only, never a live qualification.
    pub surface: String,
    /// Run, SeatBinding, slot, occupancy generation, document hash and
    /// placement hash, read back from the native correlation labels.
    pub correlation: MemberSurfaceField,
    /// Provider, model, effective effort and permission mode, read back.
    pub route: MemberSurfaceField,
    /// The closed member tool restriction the session was created under.
    pub tool_restrictions: MemberSurfaceField,
}

impl ConsultationLaunchRequest {
    /// The planning pair context this launch is held to, once its family and
    /// its context are proved to agree.
    ///
    /// # Errors
    /// [`RuntimeError::LaunchNotAdmitted`] when a planning pair launch carries
    /// no context, an Advisor or Committee launch carries one, or the context
    /// disagrees with the request it travels on.
    pub fn planning_pair_context(&self) -> RuntimeResult<Option<&PlanningPairLaunchContext>> {
        match (self.run_id, self.planning_pair.as_ref()) {
            (ConsultationRunId::PlanningPair(_), None) => Err(RuntimeError::LaunchNotAdmitted {
                rule: "a planning pair member launch requires its complete frozen context",
            }),
            (ConsultationRunId::Advisor(_) | ConsultationRunId::Committee(_), Some(_)) => {
                Err(RuntimeError::LaunchNotAdmitted {
                    rule: "an Advisor or Committee launch carries no planning pair context",
                })
            }
            (ConsultationRunId::Advisor(_) | ConsultationRunId::Committee(_), None) => Ok(None),
            (ConsultationRunId::PlanningPair(run_id), Some(context)) => {
                context.validate_for(self, run_id)?;
                Ok(Some(context))
            }
        }
    }
}

impl PlanningPairLaunchContext {
    fn validate_for(
        &self,
        request: &ConsultationLaunchRequest,
        run_id: PlanningPairRunId,
    ) -> RuntimeResult<()> {
        let refuse = |rule: &'static str| Err(RuntimeError::LaunchNotAdmitted { rule });
        if self.run_id != run_id {
            return refuse("the planning pair context names another run");
        }
        if self.seat_binding_id != request.seat_binding_id {
            return refuse("the planning pair context names another member seat");
        }
        if self.slot.as_str() != request.role_slot_id.as_str() {
            return refuse("the planning pair context names another member slot");
        }
        if self.occupancy_generation == 0 {
            return refuse("the planning pair context names no occupancy generation");
        }
        if self.route != request.model_rung {
            return refuse("the planning pair context froze another route");
        }
        let vendor = self.vendor.trim();
        if vendor.is_empty() || vendor == "unknown" {
            return refuse("the planning pair context names no actual vendor");
        }
        if self.cwd != request.cwd || self.topology_node_id != request.container.topology_node_id()
        {
            return refuse("the planning pair context names another member container");
        }
        if request.route_provenance
            != ConsultationRouteProvenance::fleet_configuration(self.placement_hash.clone())
        {
            return refuse("the member route's provenance is not the frozen placement");
        }
        if request.fleet_provenance.as_ref() != Some(&self.requested_fleet_provenance)
            || self.requested_fleet_provenance.vendor != self.vendor
        {
            return refuse("the member launch requests another fleet provenance");
        }
        Ok(())
    }
}
