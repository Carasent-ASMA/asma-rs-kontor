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
use kontor_core::planning_pair::{PlanningPairPin, PlanningPairSlot};
use kontor_core::spec::{ModelRung, TeamDefinitionSnapshot, TopologySnapshot};
use kontor_core::state::NativeRuntimeIdentity;

use crate::adapter::{
    ConsultationLaunchOutcome, ConsultationLaunchRequest, ConsultationRouteProvenance,
    RuntimeError, RuntimeResult,
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

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::id::{
        ExternalId, ExternalName, PlanningPairProfileId, RuntimeKindKey, TeamDefinitionId,
        TopologySpecId,
    };
    use kontor_core::spec::{ModelRef, ProviderRef};

    fn identity(native_id: &str) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("a runtime kind"),
            host: ExternalName::parse("fake-host").expect("a host"),
            generation: 3,
            native_id: ExternalId::parse(native_id).expect("a native id"),
        }
    }

    fn request() -> PlanningPairMemberReconcileRequest {
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
}
