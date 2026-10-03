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
    ContentHash, PlanningPairRunId, RoleCatalogId, SeatBindingId, SpecVersion, Timestamp,
    TopologyNodeId,
};
use kontor_core::planning_pair::{PlanningPairPin, PlanningPairReadbackRefusal, PlanningPairSlot};
use kontor_core::spec::{ModelRung, TeamDefinitionSnapshot, TopologySnapshot};
use kontor_core::state::NativeRuntimeIdentity;

use crate::adapter::{
    ConsultationLaunchOutcome, ConsultationLaunchRequest, ConsultationRouteProvenance,
    RuntimeError, RuntimeResult,
};
use crate::provenance::FleetLaunchProvenance;
use crate::workspace::WorkspaceRoot;

pub mod application;
pub mod caller;
pub mod context;
pub mod intent;
pub mod recovery;

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

/// Why a runtime cannot establish a planning pair member's closed surface for
/// one route. Each is its own refusal: no route is ever passed on a blanket
/// answer, substituted, or launched under a weaker surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberSurfaceGap {
    /// The harness can be composed, but the runtime acknowledges no applied
    /// closed tool restriction for the created session, so the restriction
    /// could never be observed (Claude through Paseo today).
    RestrictionUnacknowledged,
    /// No supported closed tool restriction exists for the route: its
    /// sandbox and approval policy alone are not one, and its home's own MCP
    /// servers are not excluded (Codex).
    ClosedToolsUnavailable,
    /// The route's read-only mode is behavioral, not an enforced boundary
    /// (Cursor, OpenCode and their historical fallbacks).
    ReadOnlyUnenforced,
    /// This runtime composes no member surface for the route's provider.
    NotComposed,
}

impl MemberSurfaceGap {
    /// The stable name a refusal carries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RestrictionUnacknowledged => "restriction_unacknowledged",
            Self::ClosedToolsUnavailable => "closed_tools_unavailable",
            Self::ReadOnlyUnenforced => "read_only_unenforced",
            Self::NotComposed => "not_composed",
        }
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

/// The fields a member must have observed, every one of them, before it is
/// bound and may contribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MandatoryMemberField {
    /// [`PlanningPairMemberObservation::correlation`].
    Correlation,
    /// [`PlanningPairMemberObservation::route`].
    Route,
    /// [`PlanningPairMemberObservation::tool_restrictions`].
    ToolRestrictions,
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
    /// Whether the session runs under the account its credential belongs to.
    ///
    /// An account-qualified provider label is not credential ownership, and no
    /// runtime reports this, so it is stated separately and is never a field a
    /// member qualifies on.
    pub account_authority: MemberSurfaceField,
}

impl PlanningPairMemberObservation {
    /// The first mandatory field this observation did not match, or `None`
    /// when every one was.
    #[must_use]
    pub fn unmatched_mandatory(&self) -> Option<MandatoryMemberField> {
        [
            (MandatoryMemberField::Correlation, self.correlation),
            (MandatoryMemberField::Route, self.route),
            (
                MandatoryMemberField::ToolRestrictions,
                self.tool_restrictions,
            ),
        ]
        .into_iter()
        .find(|(_, field)| *field != MemberSurfaceField::Matched)
        .map(|(name, _)| name)
    }
}

/// Whether one member readback qualifies the member, or the typed refusal
/// naming the first thing it did not observe.
///
/// The one rule every consumer holds a member to: a member-surface
/// observation in which correlation, route and the closed tool restriction
/// are each matched, and a fleet provenance observed exactly as requested.
/// Account authority is never part of it, and nothing here is authority to
/// act.
///
/// # Errors
/// The first refusal, in that order.
pub fn qualify_member_readback(
    outcome: &ConsultationLaunchOutcome,
    requested: Option<&crate::provenance::FleetLaunchProvenance>,
) -> Result<(), PlanningPairReadbackRefusal> {
    let observation = outcome
        .planning_pair
        .as_ref()
        .ok_or(PlanningPairReadbackRefusal::NoMemberSurface)?;
    match observation.unmatched_mandatory() {
        Some(MandatoryMemberField::Correlation) => {
            return Err(PlanningPairReadbackRefusal::CorrelationUnobserved);
        }
        Some(MandatoryMemberField::Route) => {
            return Err(PlanningPairReadbackRefusal::RouteUnobserved);
        }
        Some(MandatoryMemberField::ToolRestrictions) => {
            return Err(PlanningPairReadbackRefusal::ToolRestrictionUnobserved);
        }
        None => {}
    }
    let observed = matches!(
        &outcome.fleet_provenance,
        crate::provenance::FleetProvenanceObservation::Observed { provenance, .. }
            if Some(provenance) == requested
    );
    if !observed {
        return Err(PlanningPairReadbackRefusal::ProvenanceUnconfirmed);
    }
    Ok(())
}

/// Reconcile one planning pair member's exact known native session in place
/// (ASMA-8282 frontier A).
///
/// Everything stays what it was frozen to: the logical SeatBinding, its slot
/// and occupancy generation, the route and actual vendor, the document,
/// catalog and placement pins, and the native session itself. A runtime reads
/// that session back, member surface and provenance included, and where it
/// supports one resumes that same session where it is. It never creates,
/// replaces, archives or reroutes a member here, and never moves its
/// generation.
///
/// No credential travels. The same session keeps the process environment it
/// was created with, which holds the credential of the generation stated
/// here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairMemberReconcileRequest {
    /// Everything the member is frozen to, derived from durable state.
    pub context: PlanningPairLaunchContext,
    /// The exact native session the member is known by.
    pub identity: NativeRuntimeIdentity,
    /// Reconciliation instant.
    pub requested_at: Timestamp,
}

impl PlanningPairMemberReconcileRequest {
    /// Refuse a request whose own context cannot name one exact member.
    ///
    /// # Errors
    /// [`RuntimeError::LaunchNotAdmitted`] for a context with no occupancy
    /// generation, no actual vendor, or a requested provenance on another
    /// vendor.
    pub fn validate(&self) -> RuntimeResult<()> {
        let refuse = |rule: &'static str| Err(RuntimeError::LaunchNotAdmitted { rule });
        if self.context.occupancy_generation == 0 {
            return refuse("a planning pair member reconcile names no occupancy generation");
        }
        let vendor = self.context.vendor.trim();
        if vendor.is_empty() || vendor == "unknown" {
            return refuse("a planning pair member reconcile names no actual vendor");
        }
        if self.context.requested_fleet_provenance.vendor != self.context.vendor {
            return refuse("a planning pair member reconcile requests another fleet provenance");
        }
        Ok(())
    }

    /// Hold a runtime's answer to this request: that same native session,
    /// never a created one.
    ///
    /// # Errors
    /// [`RuntimeError::LaunchNotAdmitted`] when the runtime reports it created
    /// a session, and [`RuntimeError::CorrelationFailed`] for any session but
    /// the one requested.
    pub fn require_same_native(&self, outcome: &ConsultationLaunchOutcome) -> RuntimeResult<()> {
        if outcome.created {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a planning pair member reconcile never creates a native session",
            });
        }
        if !outcome.identity.same_session(&self.identity) {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(())
    }
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

// ---------------------------------------------------------------------------
// Readiness (ASMA-8282 frontier C, slice C-M): a diagnostic, never authority.
// ---------------------------------------------------------------------------

/// What one consumer may conclude about a planning pair, as three separate
/// planes: the policy the shared allocator selected, the authenticated
/// application caller, and each qualified native member. It is effect-free
/// and read-only: building one creates no run, receipt, credential, binding,
/// configuration, claim or native session.
///
/// It is a diagnostic, never a credential, a permission or a receipt: it has
/// no wire form a request could carry back, and
/// [`PlanningPairReadiness::native_actuation_authorized`] is always false. A
/// selected policy is not authority to invoke, freeze, bind or launch, and a
/// matched member on a fake or fixture surface is source-contract evidence
/// only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningPairReadiness {
    /// The policy plane.
    pub policy: PolicyPlane,
    /// The application caller plane.
    pub caller: CallerPlane,
    /// The native member plane, `seat-a` then `seat-b`.
    pub members: [MemberReadiness; 2],
}

/// The shared allocator's selection, as it froze it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PolicyPlane {
    /// Both members were placed on distinct actual vendors.
    Selected {
        /// The canonical placement hash every member is frozen to.
        placement_hash: ContentHash,
        /// The placed members, `seat-a` then `seat-b`.
        members: Vec<kontor_core::planning_pair::PlanningPairMember>,
    },
    /// No complete placement exists, so nothing is frozen from it.
    Blocked,
}

/// The application caller plane.
///
/// Only a trusted service boundary authenticates the exact frozen caller at
/// its current hosted generation under its selected role authority. This
/// seam reaches none: nothing a consumer can hand it — a CLI argument, a
/// parent label, a native prompt, a receipt echo, an account alias, a
/// self-report or an object read from JSON — establishes the plane, so it
/// takes no caller input at all and answers the plane unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CallerPlane {
    /// This consumer has no authenticated caller-generation capability.
    Unsupported {
        /// Why.
        gap: CallerGap,
    },
}

/// Why the caller plane is not established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerGap {
    /// No authenticated current caller generation reaches this consumer.
    NoAuthenticatedCallerGeneration,
}

/// One member's native plane.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemberReadiness {
    /// The member slot.
    pub slot: PlanningPairSlot,
    /// What the trusted runtime and claim boundaries report of it.
    pub plane: MemberPlane,
}

/// What the trusted runtime and claim boundaries report of one member. It is
/// never inferred from a label, a request or a report the member made of
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemberPlane {
    /// The runtime cannot establish the closed member surface for the
    /// member's route, answered before any effect.
    RouteUnsupported {
        /// The route's provider.
        provider: String,
        /// The stable gap name.
        gap: &'static str,
    },
    /// The runtime named an earlier member's route as the first blocker, so
    /// this member's route was not assessed.
    RouteUnassessed,
    /// No trusted readback of the member's session is held here.
    Unobserved,
    /// A readback is held and does not qualify the member.
    Unqualified {
        /// The first thing it did not establish.
        reason: MemberReadinessGap,
    },
    /// Every mandatory field matched and exactly the requested provenance
    /// was observed, on the surface named. Never authority to act.
    Matched {
        /// The surface the readback came from.
        surface: String,
    },
}

/// Why one held member readback does not qualify the member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberReadinessGap {
    /// The frozen context is not this placement's member.
    ContextMismatch,
    /// No trusted claim names the member's native session.
    NoKnownNative,
    /// The claim is another run's, seat's, generation's or placement's.
    StaleClaim,
    /// The readback is of another session, or another provider conversation.
    OtherSession,
    /// The readback reports a created session.
    CreatedSession,
    /// The runtime reported no member-surface observation.
    NoMemberSurface,
    /// The native correlation was not observed as matched.
    CorrelationUnobserved,
    /// The route was not observed as matched.
    RouteUnobserved,
    /// The closed tool restriction was not observed as matched.
    ToolRestrictionUnobserved,
    /// The readback did not observe exactly the requested provenance.
    ProvenanceUnconfirmed,
}

impl From<PlanningPairReadbackRefusal> for MemberReadinessGap {
    fn from(refusal: PlanningPairReadbackRefusal) -> Self {
        match refusal {
            PlanningPairReadbackRefusal::NoMemberSurface => Self::NoMemberSurface,
            PlanningPairReadbackRefusal::CorrelationUnobserved => Self::CorrelationUnobserved,
            PlanningPairReadbackRefusal::RouteUnobserved => Self::RouteUnobserved,
            PlanningPairReadbackRefusal::ToolRestrictionUnobserved => {
                Self::ToolRestrictionUnobserved
            }
            PlanningPairReadbackRefusal::ProvenanceUnconfirmed => Self::ProvenanceUnconfirmed,
        }
    }
}

/// What the trusted boundaries hold about one member: its frozen context,
/// derived from durable state; its known native claim, read from the claim
/// boundary; and a readback of that session, from the runtime boundary.
#[derive(Debug, Clone, Copy)]
pub struct MemberEvidence<'a> {
    /// The member's frozen context.
    pub context: &'a PlanningPairLaunchContext,
    /// The member's known native claim at its generation, if one is kept.
    pub known: Option<&'a kontor_core::repository::StoredPlanningPairKnownNative>,
    /// A trusted readback of the member's session, if one is held.
    pub readback: Option<&'a ConsultationLaunchOutcome>,
}

impl PlanningPairReadiness {
    /// What a direct consumer holding only the shared allocator's placement
    /// may conclude: the policy plane, an unsupported caller plane, and two
    /// unobserved members.
    #[must_use]
    pub fn policy_only(
        placement: Option<&kontor_core::planning_pair::PlanningPairMembers>,
    ) -> Self {
        Self::assess(placement, &Ok(()), [None, None])
    }

    /// Assess one pair from the placement, the runtime's pre-effect answer for
    /// both member routes, and whatever trusted evidence is held for each
    /// member, `seat-a` then `seat-b`.
    ///
    /// The route answer is the pair's: the first refused route names its
    /// provider, an earlier member passed, and a later one was not assessed.
    #[must_use]
    pub fn assess(
        placement: Option<&kontor_core::planning_pair::PlanningPairMembers>,
        routes: &RuntimeResult<()>,
        evidence: [Option<MemberEvidence<'_>>; 2],
    ) -> Self {
        let policy = placement.map_or(PolicyPlane::Blocked, |placement| PolicyPlane::Selected {
            placement_hash: placement.placement_hash().clone(),
            members: placement.members().to_vec(),
        });
        let [seat_a, seat_b] = evidence;
        let assess =
            |index: usize, slot: PlanningPairSlot, evidence: Option<MemberEvidence<'_>>| {
                let plane = match placement {
                    None => MemberPlane::Unobserved,
                    Some(placement) => member_plane(placement, index, routes, evidence),
                };
                MemberReadiness { slot, plane }
            };
        Self {
            policy,
            caller: CallerPlane::Unsupported {
                gap: CallerGap::NoAuthenticatedCallerGeneration,
            },
            members: [
                assess(0, PlanningPairSlot::SeatA, seat_a),
                assess(1, PlanningPairSlot::SeatB, seat_b),
            ],
        }
    }

    /// Never: a readiness diagnostic authorizes no native actuation.
    #[must_use]
    pub const fn native_actuation_authorized(&self) -> bool {
        false
    }

    /// Whether every plane is established: a selected policy, an
    /// authenticated caller and two matched members. The caller plane is
    /// never established here, so this is never true; it is still not
    /// authority to act.
    #[must_use]
    pub fn every_plane_established(&self) -> bool {
        let caller = match self.caller {
            CallerPlane::Unsupported { .. } => false,
        };
        matches!(self.policy, PolicyPlane::Selected { .. })
            && caller
            && self
                .members
                .iter()
                .all(|member| matches!(member.plane, MemberPlane::Matched { .. }))
    }

    /// The diagnostic document a consumer may print. It is not a receipt and
    /// carries nothing a request could present as authority.
    #[must_use]
    pub fn document(&self) -> serde_json::Value {
        serde_json::json!({
            "diagnostic": "planning_pair_readiness",
            "policy": self.policy,
            "caller": self.caller,
            "members": self.members,
            "every_plane_established": self.every_plane_established(),
            "native_actuation_authorized": self.native_actuation_authorized(),
        })
    }
}

/// One placed member's native plane, from the pair's route answer and the
/// evidence held for it.
fn member_plane(
    placement: &kontor_core::planning_pair::PlanningPairMembers,
    index: usize,
    routes: &RuntimeResult<()>,
    evidence: Option<MemberEvidence<'_>>,
) -> MemberPlane {
    let placed = &placement.members()[index];
    match routes {
        Ok(()) => {}
        // The pair's answer names its first refused route. That provider is
        // refused for every member on it; a member before it passed, and a
        // member after it on another provider was not assessed.
        Err(RuntimeError::PlanningPairMemberSurfaceUnsupported { provider, gap }) => {
            if placed.route.provider.0 == *provider {
                return MemberPlane::RouteUnsupported {
                    provider: provider.clone(),
                    gap: gap.as_str(),
                };
            }
            let blocker = placement
                .members()
                .iter()
                .position(|member| member.route.provider.0 == *provider);
            if blocker.is_none_or(|blocker| blocker < index) {
                return MemberPlane::RouteUnassessed;
            }
        }
        // A runtime that composes no member surface refuses every route.
        Err(_) => {
            return MemberPlane::RouteUnsupported {
                provider: placed.route.provider.0.clone(),
                gap: MemberSurfaceGap::NotComposed.as_str(),
            };
        }
    }
    let Some(evidence) = evidence else {
        return MemberPlane::Unobserved;
    };
    let context = evidence.context;
    if context.slot != placed.slot
        || context.route != placed.route
        || context.vendor != placed.vendor
        || &context.placement_hash != placement.placement_hash()
    {
        return MemberPlane::Unqualified {
            reason: MemberReadinessGap::ContextMismatch,
        };
    }
    let Some(readback) = evidence.readback else {
        return MemberPlane::Unobserved;
    };
    let unqualified = |reason| MemberPlane::Unqualified { reason };
    let Some(known) = evidence.known else {
        return unqualified(MemberReadinessGap::NoKnownNative);
    };
    if known.run_id != ConsultationRunId::PlanningPair(context.run_id)
        || known.seat_binding_id != context.seat_binding_id
        || known.occupancy_generation != context.occupancy_generation
        || known.placement_hash != context.placement_hash
    {
        return unqualified(MemberReadinessGap::StaleClaim);
    }
    if readback.created {
        return unqualified(MemberReadinessGap::CreatedSession);
    }
    if !readback.identity.same_session(&known.identity)
        || known
            .provider_session_id
            .as_ref()
            .is_some_and(|session| readback.provider_session_id.as_ref() != Some(session))
    {
        return unqualified(MemberReadinessGap::OtherSession);
    }
    if let Err(refusal) =
        qualify_member_readback(readback, Some(&context.requested_fleet_provenance))
    {
        return unqualified(refusal.into());
    }
    MemberPlane::Matched {
        surface: readback
            .planning_pair
            .as_ref()
            .map(|observation| observation.surface.clone())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::id::{
        ExternalId, ExternalName, PlanningPairProfileId, RuntimeKindKey, TeamDefinitionId,
        TopologySpecId,
    };
    use kontor_core::spec::{ModelRef, ProviderRef};

    pub(super) fn identity(native_id: &str) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("a runtime kind"),
            host: ExternalName::parse("fake-host").expect("a host"),
            generation: 3,
            native_id: ExternalId::parse(native_id).expect("a native id"),
        }
    }

    pub(super) fn request() -> PlanningPairMemberReconcileRequest {
        let provenance = crate::provenance::FleetLaunchProvenance {
            policy_hash: ContentHash::of(b"activated policy"),
            source_bundle_hash: None,
            binding_key: "advisor/pair".to_owned(),
            chain: "pair".to_owned(),
            step: 1,
            sub_step: 1,
            vendor: "anthropic".to_owned(),
            eligibility: None,
        };
        PlanningPairMemberReconcileRequest {
            context: PlanningPairLaunchContext {
                run_id: PlanningPairRunId::generate(),
                seat_binding_id: SeatBindingId::generate(),
                slot: PlanningPairSlot::SeatB,
                occupancy_generation: 2,
                profile: PlanningPairPin {
                    profile_id: PlanningPairProfileId::generate(),
                    version: SpecVersion::FIRST,
                    definition_hash: ContentHash::of(b"planning pair document"),
                },
                topology: TopologySnapshot {
                    spec_id: TopologySpecId::generate(),
                    version: SpecVersion::FIRST,
                    canonical_hash: ContentHash::of(b"topology"),
                },
                team_definition: TeamDefinitionSnapshot {
                    definition_id: TeamDefinitionId::generate(),
                    version: SpecVersion::FIRST,
                    canonical_hash: ContentHash::of(b"team definition"),
                },
                role_catalog: PlanningPairCatalogPin {
                    catalog_id: RoleCatalogId::generate(),
                    version: SpecVersion::FIRST,
                    canonical_hash: ContentHash::of(b"role catalog"),
                },
                topology_node_id: TopologyNodeId::generate(),
                cwd: WorkspaceRoot::parse("/realm/pair").expect("a member cwd"),
                route: ModelRung {
                    provider: ProviderRef("claude-personal".to_owned()),
                    model: ModelRef("claude-opus-5".to_owned()),
                    effort: None,
                },
                vendor: "anthropic".to_owned(),
                placement_hash: ContentHash::of(b"placement"),
                requested_fleet_provenance: provenance,
            },
            identity: identity("native-member-b"),
            requested_at: "2026-10-02T09:10:00Z".parse().expect("an instant"),
        }
    }

    fn answer(native: NativeRuntimeIdentity, created: bool) -> ConsultationLaunchOutcome {
        ConsultationLaunchOutcome {
            identity: native.clone(),
            provider_session_id: None,
            observed_at: "2026-10-02T09:10:01Z".parse().expect("an instant"),
            created,
            fleet_provenance: crate::provenance::FleetProvenanceObservation::Unsupported {
                surface: "fixture".to_owned(),
                native_id: native.native_id,
            },
            planning_pair: None,
        }
    }

    fn refused_rule(result: RuntimeResult<()>) -> &'static str {
        match result {
            Err(RuntimeError::LaunchNotAdmitted { rule }) => rule,
            other => panic!("expected a typed refusal, got {other:?}"),
        }
    }

    /// A reconcile names one exact member: its own generation, an actual
    /// vendor, and the provenance requested on that vendor.
    #[test]
    fn a_reconcile_request_names_one_exact_member() {
        assert_eq!(request().validate(), Ok(()));

        let mut fenceless = request();
        fenceless.context.occupancy_generation = 0;
        assert_eq!(
            refused_rule(fenceless.validate()),
            "a planning pair member reconcile names no occupancy generation"
        );

        for vendor in ["", "  ", "unknown"] {
            let mut vendorless = request();
            vendorless.context.vendor = vendor.to_owned();
            vendorless.context.requested_fleet_provenance.vendor = vendor.to_owned();
            assert_eq!(
                refused_rule(vendorless.validate()),
                "a planning pair member reconcile names no actual vendor",
                "{vendor:?}"
            );
        }

        let mut elsewhere = request();
        elsewhere.context.requested_fleet_provenance.vendor = "openai".to_owned();
        assert_eq!(
            refused_rule(elsewhere.validate()),
            "a planning pair member reconcile requests another fleet provenance"
        );
    }

    /// The answer is the requested session itself, never a created one and
    /// never another session, generation or host.
    #[test]
    fn a_reconcile_answer_is_the_same_native_and_never_a_created_one() {
        let request = request();
        assert_eq!(
            request.require_same_native(&answer(request.identity.clone(), false)),
            Ok(())
        );
        assert_eq!(
            refused_rule(request.require_same_native(&answer(request.identity.clone(), true))),
            "a planning pair member reconcile never creates a native session"
        );
        let mut other_session = request.identity.clone();
        other_session.native_id = ExternalId::parse("native-member-b-2").expect("a native id");
        let mut other_generation = request.identity.clone();
        other_generation.generation += 1;
        let mut other_host = request.identity.clone();
        other_host.host = ExternalName::parse("another-host").expect("a host");
        for other in [other_session, other_generation, other_host] {
            assert_eq!(
                request.require_same_native(&answer(other.clone(), false)),
                Err(RuntimeError::CorrelationFailed),
                "{other:?}"
            );
        }
    }

    /// No secret travels: the request has no credential, so neither its
    /// value nor its debug form can carry one.
    #[test]
    fn a_reconcile_request_carries_no_credential() {
        let rendered = format!("{:?}", request());
        assert!(!rendered.contains("REDACTED"), "{rendered}");
        assert!(!rendered.contains("credential"), "{rendered}");
    }

    fn placed(
        slot: PlanningPairSlot,
        provider: &str,
        model: &str,
        vendor: &str,
    ) -> kontor_core::planning_pair::PlanningPairMember {
        kontor_core::planning_pair::PlanningPairMember {
            slot,
            binding_key: format!("pair/{}", slot.as_str()),
            route: ModelRung {
                provider: ProviderRef(provider.to_owned()),
                model: ModelRef(model.to_owned()),
                effort: None,
            },
            vendor: vendor.to_owned(),
        }
    }

    fn placement() -> kontor_core::planning_pair::PlanningPairMembers {
        kontor_core::planning_pair::PlanningPairMembers::freeze(
            ContentHash::of(b"placement"),
            vec![
                placed(
                    PlanningPairSlot::SeatA,
                    "codex-work",
                    "gpt-5.6-sol",
                    "openai",
                ),
                placed(
                    PlanningPairSlot::SeatB,
                    "claude-personal",
                    "claude-opus-5",
                    "anthropic",
                ),
            ],
        )
        .expect("two members on distinct vendors")
    }

    /// One member's frozen context on [`placement`], at generation 2.
    fn context_of(index: usize) -> PlanningPairLaunchContext {
        let placement = placement();
        let member = &placement.members()[index];
        let mut context = request().context;
        context.slot = member.slot;
        context.seat_binding_id = SeatBindingId::generate();
        context.route = member.route.clone();
        context.vendor = member.vendor.clone();
        context.requested_fleet_provenance.vendor = member.vendor.clone();
        context.requested_fleet_provenance.binding_key = member.binding_key.clone();
        context
    }

    fn claim_of(
        context: &PlanningPairLaunchContext,
        native: &str,
    ) -> kontor_core::repository::StoredPlanningPairKnownNative {
        kontor_core::repository::StoredPlanningPairKnownNative {
            run_id: kontor_core::consultation::ConsultationRunId::PlanningPair(context.run_id),
            project_id: kontor_core::id::ProjectId::generate(),
            seat_binding_id: context.seat_binding_id,
            occupancy_generation: context.occupancy_generation,
            identity: identity(native),
            provider_session_id: Some(ExternalId::parse("provider-1").expect("a session")),
            context_hash: ContentHash::of(b"context"),
            placement_hash: context.placement_hash.clone(),
            readback_refusal: None,
            observed_at: "2026-10-02T09:10:00Z".parse().expect("an instant"),
        }
    }

    /// A readback of `native` that matched every field and observed exactly
    /// the context's requested provenance, on the fake's surfaces.
    pub(super) fn matched_of(
        context: &PlanningPairLaunchContext,
        native: &str,
    ) -> ConsultationLaunchOutcome {
        ConsultationLaunchOutcome {
            identity: identity(native),
            provider_session_id: Some(ExternalId::parse("provider-1").expect("a session")),
            observed_at: "2026-10-02T09:10:01Z".parse().expect("an instant"),
            created: false,
            fleet_provenance: crate::provenance::FleetProvenanceObservation::Observed {
                surface: "fake.runtime.labels".to_owned(),
                provenance: context.requested_fleet_provenance.clone(),
            },
            planning_pair: Some(PlanningPairMemberObservation {
                surface: "fake.runtime".to_owned(),
                correlation: MemberSurfaceField::Matched,
                route: MemberSurfaceField::Matched,
                tool_restrictions: MemberSurfaceField::Matched,
                account_authority: MemberSurfaceField::Unsupported,
            }),
        }
    }

    /// Readiness of [`placement`] with both members matched, after `edit`
    /// changes seat B's held evidence.
    fn seat_b_after(
        routes: &RuntimeResult<()>,
        edit: impl FnOnce(
            &mut PlanningPairLaunchContext,
            &mut Option<kontor_core::repository::StoredPlanningPairKnownNative>,
            &mut Option<ConsultationLaunchOutcome>,
        ),
    ) -> PlanningPairReadiness {
        let placement = placement();
        let a = context_of(0);
        let a_claim = claim_of(&a, "native-member-a");
        let a_readback = matched_of(&a, "native-member-a");
        let mut b = context_of(1);
        let mut b_claim = Some(claim_of(&b, "native-member-b"));
        let mut b_readback = Some(matched_of(&b, "native-member-b"));
        edit(&mut b, &mut b_claim, &mut b_readback);
        PlanningPairReadiness::assess(
            Some(&placement),
            routes,
            [
                Some(MemberEvidence {
                    context: &a,
                    known: Some(&a_claim),
                    readback: Some(&a_readback),
                }),
                Some(MemberEvidence {
                    context: &b,
                    known: b_claim.as_ref(),
                    readback: b_readback.as_ref(),
                }),
            ],
        )
    }

    fn matched() -> MemberPlane {
        MemberPlane::Matched {
            surface: "fake.runtime".to_owned(),
        }
    }

    /// Frontier C, the three planes: the shared allocator's selection alone
    /// establishes neither an authenticated caller nor a qualified member, and
    /// authorizes nothing.
    #[test]
    fn a_policy_selection_alone_is_never_caller_or_member_readiness() {
        let placement = placement();
        let readiness = PlanningPairReadiness::policy_only(Some(&placement));
        assert_eq!(
            readiness.policy,
            PolicyPlane::Selected {
                placement_hash: ContentHash::of(b"placement"),
                members: placement.members().to_vec(),
            }
        );
        assert_eq!(
            readiness.caller,
            CallerPlane::Unsupported {
                gap: CallerGap::NoAuthenticatedCallerGeneration,
            }
        );
        for member in &readiness.members {
            assert_eq!(member.plane, MemberPlane::Unobserved, "{:?}", member.slot);
        }
        assert!(!readiness.every_plane_established());
        assert!(!readiness.native_actuation_authorized());
        let document = readiness.document();
        assert_eq!(document["native_actuation_authorized"], false);
        assert_eq!(document["every_plane_established"], false);
        assert_eq!(
            document["caller"],
            serde_json::json!({"state": "unsupported", "gap": "no_authenticated_caller_generation"})
        );
        assert_eq!(document["policy"]["state"], "selected");
        assert_eq!(document["members"][1]["slot"], "seat-b");
        assert_eq!(document["members"][1]["plane"]["state"], "unobserved");
        for key in [
            "launchable",
            "authorized",
            "verdict",
            "receipt",
            "credential",
        ] {
            assert!(document.get(key).is_none(), "{key}: {document}");
        }

        let blocked = PlanningPairReadiness::policy_only(None);
        assert_eq!(blocked.policy, PolicyPlane::Blocked);
        assert!(
            blocked
                .members
                .iter()
                .all(|member| member.plane == MemberPlane::Unobserved)
        );
        assert!(!blocked.native_actuation_authorized());
    }

    /// A fully matched member on the fake's surface is hypothetical
    /// source-contract evidence: both members matched still leave the caller
    /// plane unsupported, so no plane set is established and nothing is
    /// authorized.
    #[test]
    fn a_fully_matched_hypothetical_pair_still_authorizes_nothing() {
        let readiness = seat_b_after(&Ok(()), |_, _, _| {});
        assert_eq!(readiness.members[0].plane, matched());
        assert_eq!(readiness.members[1].plane, matched());
        assert_eq!(
            readiness.caller,
            CallerPlane::Unsupported {
                gap: CallerGap::NoAuthenticatedCallerGeneration,
            }
        );
        assert!(!readiness.every_plane_established());
        assert!(!readiness.native_actuation_authorized());
        assert_eq!(
            readiness.document()["members"][0]["plane"],
            serde_json::json!({"state": "matched", "surface": "fake.runtime"})
        );
    }

    /// Every mandatory member check holds a readback to the exact frozen
    /// member and its exact known session; each failure is its own typed
    /// gap, and the peer is assessed on its own evidence.
    #[test]
    fn every_member_check_holds_a_readback_to_the_exact_known_session() {
        use MemberReadinessGap as Gap;
        type Edit = fn(
            &mut PlanningPairLaunchContext,
            &mut Option<kontor_core::repository::StoredPlanningPairKnownNative>,
            &mut Option<ConsultationLaunchOutcome>,
        );
        let unobserved = |field: MandatoryMemberField| {
            move |readback: &mut Option<ConsultationLaunchOutcome>| {
                let observation = readback
                    .as_mut()
                    .and_then(|readback| readback.planning_pair.as_mut())
                    .expect("an observation");
                match field {
                    MandatoryMemberField::Correlation => {
                        observation.correlation = MemberSurfaceField::Unsupported;
                    }
                    MandatoryMemberField::Route => {
                        observation.route = MemberSurfaceField::Unsupported;
                    }
                    MandatoryMemberField::ToolRestrictions => {
                        observation.tool_restrictions = MemberSurfaceField::Unsupported;
                    }
                }
            }
        };
        let cases: Vec<(&str, Edit, Gap)> = vec![
            (
                "another route",
                |context, _, _| context.route.model.0.push_str("-next"),
                Gap::ContextMismatch,
            ),
            (
                "another vendor",
                |context, _, _| context.vendor = "openai".to_owned(),
                Gap::ContextMismatch,
            ),
            (
                "another placement",
                |context, _, _| context.placement_hash = ContentHash::of(b"another placement"),
                Gap::ContextMismatch,
            ),
            (
                "no known native",
                |_, claim, _| *claim = None,
                Gap::NoKnownNative,
            ),
            (
                "a claim of another generation",
                |_, claim, _| claim.as_mut().expect("a claim").occupancy_generation += 1,
                Gap::StaleClaim,
            ),
            (
                "a claim of another seat",
                |_, claim, _| {
                    claim.as_mut().expect("a claim").seat_binding_id = SeatBindingId::generate()
                },
                Gap::StaleClaim,
            ),
            (
                "a claim of another run",
                |_, claim, _| {
                    claim.as_mut().expect("a claim").run_id =
                        kontor_core::consultation::ConsultationRunId::PlanningPair(
                            PlanningPairRunId::generate(),
                        );
                },
                Gap::StaleClaim,
            ),
            (
                "a claim on another placement",
                |_, claim, _| {
                    claim.as_mut().expect("a claim").placement_hash = ContentHash::of(b"another")
                },
                Gap::StaleClaim,
            ),
            (
                "a created session",
                |_, _, readback| readback.as_mut().expect("a readback").created = true,
                Gap::CreatedSession,
            ),
            (
                "another native session",
                |_, _, readback| {
                    readback.as_mut().expect("a readback").identity.native_id =
                        ExternalId::parse("native-member-c").expect("a native id");
                },
                Gap::OtherSession,
            ),
            (
                "another runtime generation",
                |_, _, readback| readback.as_mut().expect("a readback").identity.generation += 1,
                Gap::OtherSession,
            ),
            (
                "another provider conversation",
                |_, _, readback| {
                    readback.as_mut().expect("a readback").provider_session_id =
                        Some(ExternalId::parse("provider-2").expect("a session"));
                },
                Gap::OtherSession,
            ),
            (
                "no member-surface observation",
                |_, _, readback| readback.as_mut().expect("a readback").planning_pair = None,
                Gap::NoMemberSurface,
            ),
            (
                "an unconfirmed provenance",
                |_, _, readback| {
                    readback.as_mut().expect("a readback").fleet_provenance =
                        crate::provenance::FleetProvenanceObservation::Unsupported {
                            surface: "fake.runtime.labels".to_owned(),
                            native_id: ExternalId::parse("native-member-b").expect("a native id"),
                        };
                },
                Gap::ProvenanceUnconfirmed,
            ),
            (
                "another observed provenance",
                |_, _, readback| {
                    if let Some(ConsultationLaunchOutcome {
                        fleet_provenance:
                            crate::provenance::FleetProvenanceObservation::Observed {
                                provenance, ..
                            },
                        ..
                    }) = readback.as_mut()
                    {
                        provenance.binding_key.push_str("-other");
                    }
                },
                Gap::ProvenanceUnconfirmed,
            ),
        ];
        for (why, edit, gap) in cases {
            let readiness = seat_b_after(&Ok(()), edit);
            assert_eq!(
                readiness.members[1].plane,
                MemberPlane::Unqualified { reason: gap },
                "{why}"
            );
            assert_eq!(
                readiness.members[0].plane,
                matched(),
                "{why}: seat A stands alone"
            );
            assert!(!readiness.native_actuation_authorized(), "{why}");
        }
        for (field, gap) in [
            (
                MandatoryMemberField::Correlation,
                Gap::CorrelationUnobserved,
            ),
            (MandatoryMemberField::Route, Gap::RouteUnobserved),
            (
                MandatoryMemberField::ToolRestrictions,
                Gap::ToolRestrictionUnobserved,
            ),
        ] {
            let edit = unobserved(field);
            let readiness = seat_b_after(&Ok(()), |_, _, readback| edit(readback));
            assert_eq!(
                readiness.members[1].plane,
                MemberPlane::Unqualified { reason: gap },
                "{field:?}"
            );
        }
        let unread = seat_b_after(&Ok(()), |_, _, readback| *readback = None);
        assert_eq!(unread.members[1].plane, MemberPlane::Unobserved);
    }

    /// The runtime's pre-effect route answer is the pair's: its first refused
    /// route names one provider, a member before it was assessed, and a
    /// member after it on another provider was not. A runtime that composes
    /// no member surface refuses both routes.
    #[test]
    fn a_route_answer_is_attributed_to_its_first_blocker_only() {
        let refused = |provider: &str, gap: MemberSurfaceGap| {
            Err(RuntimeError::PlanningPairMemberSurfaceUnsupported {
                provider: provider.to_owned(),
                gap,
            })
        };
        let first = seat_b_after(
            &refused("codex-work", MemberSurfaceGap::ClosedToolsUnavailable),
            |_, _, _| {},
        );
        assert_eq!(
            first.members[0].plane,
            MemberPlane::RouteUnsupported {
                provider: "codex-work".to_owned(),
                gap: "closed_tools_unavailable",
            }
        );
        assert_eq!(first.members[1].plane, MemberPlane::RouteUnassessed);
        let second = seat_b_after(
            &refused(
                "claude-personal",
                MemberSurfaceGap::RestrictionUnacknowledged,
            ),
            |_, _, _| {},
        );
        assert_eq!(second.members[0].plane, matched(), "seat A's route passed");
        assert_eq!(
            second.members[1].plane,
            MemberPlane::RouteUnsupported {
                provider: "claude-personal".to_owned(),
                gap: "restriction_unacknowledged",
            }
        );
        let uncomposed = seat_b_after(
            &Err(RuntimeError::UnsupportedCapability {
                capability: crate::capability::RuntimeCapability::Launch,
            }),
            |_, _, _| {},
        );
        assert_eq!(
            uncomposed.members[0].plane,
            MemberPlane::RouteUnsupported {
                provider: "codex-work".to_owned(),
                gap: "not_composed",
            }
        );
        assert_eq!(
            uncomposed.members[1].plane,
            MemberPlane::RouteUnsupported {
                provider: "claude-personal".to_owned(),
                gap: "not_composed",
            }
        );
        let foreign = seat_b_after(&refused("pi", MemberSurfaceGap::NotComposed), |_, _, _| {});
        assert!(
            foreign
                .members
                .iter()
                .all(|member| member.plane == MemberPlane::RouteUnassessed),
            "an answer naming neither member's provider assesses neither"
        );
    }
}
