//! Frozen-caller eligibility for a planning pair (ASMA-8282 B1a).
//!
//! These are the pure decisions the service applies, stage by stage and in
//! its own order, to facts it read itself, after it authenticated the bearer
//! at its own trusted boundary. Nothing here reads state, verifies a bearer or
//! mints one: the presented seat and generation are what that boundary
//! produced, and a seat id or generation that merely names itself
//! authenticates nothing.
//!
//! A passing result is not a credential, capability, permission or generation
//! lease. It never makes a direct consumer's caller plane established: the
//! readiness seam's caller plane stays
//! [`super::CallerPlane::Unsupported`].

use kontor_core::consultation::ConsultationScope;
use kontor_core::id::{MiniProjectId, SeatBindingId};
use kontor_core::planning_pair::PlanningPairSpec;
use kontor_core::state::{SeatBinding, SessionTopologyNode, TopologyLifecycle};

/// Why a presented caller is not eligible. The service maps each one to its
/// own refusal; no variant carries wire text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerRefusal {
    /// The presented seat is not the pair's frozen caller.
    NotFrozenCaller(FrozenCallerAct),
    /// The requested ticket belongs to another epic.
    TicketOutsideEpic,
    /// The pinned document does not permit ticket-scoped invocation.
    TicketScopeNotPermitted,
    /// The pinned document does not permit epic-scoped invocation.
    EpicScopeNotPermitted,
    /// The presented seat is absent, terminal or closed.
    SeatInactive,
    /// The presented seat's topology node is absent or in another epic.
    SeatOutsideEpic,
    /// The presented seat's topology node is not active.
    NodeInactive,
    /// The presented seat has no active hosted native occupancy.
    NoHostedOccupancy,
    /// The presented generation is not the seat's current hosted one.
    FencedGeneration,
    /// Neither the seat's slot role nor its catalog role is an allowed caller
    /// role of the pinned document.
    RoleNotPermitted,
}

/// What the frozen caller was presented to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrozenCallerAct {
    /// Ask the clarification or record the disposition.
    Decide,
    /// Recover one of the pair's members.
    Recover,
}

/// Where an invocation is scoped, as the service read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationScope {
    /// The epic itself.
    Epic,
    /// One ticket, with the epic the stored ticket belongs to.
    Ticket {
        /// The stored ticket's epic, if it has one.
        ticket_epic: Option<MiniProjectId>,
    },
}

/// Only the pair's frozen caller acts for it.
///
/// # Errors
/// [`CallerRefusal::NotFrozenCaller`] for any other seat.
pub fn require_frozen_caller(
    frozen: SeatBindingId,
    presented: SeatBindingId,
    act: FrozenCallerAct,
) -> Result<(), CallerRefusal> {
    if presented == frozen {
        Ok(())
    } else {
        Err(CallerRefusal::NotFrozenCaller(act))
    }
}

/// The invocation's scope is one the pinned document permits: a ticket of
/// this very epic when one is named, the epic otherwise.
///
/// # Errors
/// The ticket's epic first, then the permitted scope.
pub fn require_invocation_scope(
    spec: &PlanningPairSpec,
    epic: MiniProjectId,
    scope: InvocationScope,
) -> Result<(), CallerRefusal> {
    match scope {
        InvocationScope::Ticket { ticket_epic } => {
            if ticket_epic != Some(epic) {
                return Err(CallerRefusal::TicketOutsideEpic);
            }
            if !spec.allowed_scopes.contains(&ConsultationScope::Ticket) {
                return Err(CallerRefusal::TicketScopeNotPermitted);
            }
        }
        InvocationScope::Epic => {
            if !spec.allowed_scopes.contains(&ConsultationScope::Epic) {
                return Err(CallerRefusal::EpicScopeNotPermitted);
            }
        }
    }
    Ok(())
}

/// The presented seat, as read, is active and open.
///
/// # Errors
/// [`CallerRefusal::SeatInactive`] for an absent, terminal or closed seat.
pub fn require_active_seat(seat: Option<SeatBinding>) -> Result<SeatBinding, CallerRefusal> {
    seat.filter(|seat| seat.is_non_terminal() && !seat.closes_children())
        .ok_or(CallerRefusal::SeatInactive)
}

/// The seat's topology node, as read, belongs to this epic and is active.
///
/// # Errors
/// The epic first, then the lifecycle.
pub fn require_active_epic_node(
    node: Option<&SessionTopologyNode>,
    epic: MiniProjectId,
) -> Result<(), CallerRefusal> {
    let node = node
        .filter(|node| node.mini_project_id == Some(epic))
        .ok_or(CallerRefusal::SeatOutsideEpic)?;
    if node.lifecycle != TopologyLifecycle::Active {
        return Err(CallerRefusal::NodeInactive);
    }
    Ok(())
}

/// The presented generation is the seat's current hosted occupancy, as read.
///
/// # Errors
/// No occupancy first, then a fenced generation.
pub fn require_current_generation(
    presented: u64,
    current: Option<u64>,
) -> Result<(), CallerRefusal> {
    let current = current.ok_or(CallerRefusal::NoHostedOccupancy)?;
    if presented != current {
        return Err(CallerRefusal::FencedGeneration);
    }
    Ok(())
}

/// The seat's slot role or catalog role is an allowed caller role of the
/// pinned document, compared without case.
///
/// # Errors
/// [`CallerRefusal::RoleNotPermitted`] when neither is.
pub fn require_caller_role(
    spec: &PlanningPairSpec,
    seat: &SeatBinding,
) -> Result<(), CallerRefusal> {
    let slot_role = seat.role_slot_id.as_role_key().as_str();
    let catalog_role = seat.role.role_code.as_str();
    if spec.allowed_caller_roles.iter().any(|allowed| {
        allowed.as_str().eq_ignore_ascii_case(slot_role)
            || allowed.as_str().eq_ignore_ascii_case(catalog_role)
    }) {
        Ok(())
    } else {
        Err(CallerRefusal::RoleNotPermitted)
    }
}

#[cfg(test)]
mod tests {
    //! Hypothetical pure fixtures: plain facts, as if a service had read them
    //! after authenticating a bearer. None of them is evidence that any seat
    //! is authenticated; the trusted path is proved in the daemon's tests.

    use super::*;

    const EPIC: &str = "01991c00-0000-7000-8000-0000000000e1";
    const OTHER_EPIC: &str = "01991c00-0000-7000-8000-0000000000e2";

    fn epic(id: &str) -> MiniProjectId {
        MiniProjectId::parse(id).expect("an epic id")
    }

    fn spec(roles: &[&str], scopes: &[&str]) -> PlanningPairSpec {
        let member = |slot: &str| {
            serde_json::json!({
                "slot": slot, "role_code": "SA", "specialty": "planning",
                "behavior": "Give one finding; change nothing.",
                "context": {"skills": [], "files": [], "memory": "none"},
            })
        };
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "protocol": "planning_pair@1",
            "profile_id": "01991c00-0000-7000-8000-0000000000b1",
            "version": 1,
            "name": "Planning pair",
            "charter": "Is this plan the smallest sound next step?",
            "container_kind": "PPW",
            "members": [member("seat-a"), member("seat-b")],
            "allowed_caller_roles": roles,
            "allowed_scopes": scopes,
            "budget": {"max_tokens": 1000, "max_commands": 4, "max_duration_seconds": 60,
                       "max_cost": {"minor_units": 100, "currency": "NOK"}},
        }))
        .expect("a planning pair document")
    }

    fn seat(slot: &str, role_code: &str, edit: impl FnOnce(&mut serde_json::Value)) -> SeatBinding {
        let mut seat = serde_json::json!({
            "id": "01991c00-0000-7000-8000-0000000000a1",
            "project_id": "01991c00-0000-7000-8000-0000000000f1",
            "topology_node_id": "01991c00-0000-7000-8000-0000000000c1",
            "role_slot_id": slot,
            "role": {"catalog_id": "01991c00-0000-7000-8000-0000000000d1",
                     "catalog_revision": 1, "role_code": role_code,
                     "standard_title": "Lead Solution Architect"},
            "lifecycle": "active",
            "attach_deadline": "2026-10-02T10:00:00Z",
            "revision": 1,
            "created_at": "2026-10-02T10:00:00Z",
            "updated_at": "2026-10-02T10:00:00Z",
        });
        edit(&mut seat);
        serde_json::from_value(seat).expect("a seat binding")
    }

    fn node(mini_project: &str, lifecycle: &str) -> SessionTopologyNode {
        serde_json::from_value(serde_json::json!({
            "id": "01991c00-0000-7000-8000-0000000000c1",
            "project_id": "01991c00-0000-7000-8000-0000000000f1",
            "mini_project_id": mini_project,
            "topology": {"spec_id": "01991c00-0000-7000-8000-0000000000c2", "version": 1,
                         "canonical_hash": "a".repeat(64)},
            "kind": "ECP",
            "lifecycle": lifecycle,
            "placement": "bound",
            "revision": 1,
            "created_at": "2026-10-02T10:00:00Z",
            "updated_at": "2026-10-02T10:00:00Z",
        }))
        .expect("a topology node")
    }

    #[test]
    fn only_the_frozen_caller_acts_for_the_pair() {
        let frozen = SeatBindingId::generate();
        let other = SeatBindingId::generate();
        for act in [FrozenCallerAct::Decide, FrozenCallerAct::Recover] {
            assert_eq!(require_frozen_caller(frozen, frozen, act), Ok(()));
            assert_eq!(
                require_frozen_caller(frozen, other, act),
                Err(CallerRefusal::NotFrozenCaller(act))
            );
        }
    }

    #[test]
    fn the_scope_is_this_epic_or_one_of_its_tickets_as_the_document_permits() {
        let epic_only = spec(&["lsa"], &["epic"]);
        let ticket_only = spec(&["lsa"], &["ticket"]);
        let here = InvocationScope::Ticket {
            ticket_epic: Some(epic(EPIC)),
        };
        assert_eq!(
            require_invocation_scope(&epic_only, epic(EPIC), InvocationScope::Epic),
            Ok(())
        );
        assert_eq!(
            require_invocation_scope(&ticket_only, epic(EPIC), InvocationScope::Epic),
            Err(CallerRefusal::EpicScopeNotPermitted)
        );
        assert_eq!(
            require_invocation_scope(&ticket_only, epic(EPIC), here),
            Ok(())
        );
        assert_eq!(
            require_invocation_scope(&epic_only, epic(EPIC), here),
            Err(CallerRefusal::TicketScopeNotPermitted)
        );
        // The ticket's epic is held first, even where ticket scope is refused.
        for ticket_epic in [Some(epic(OTHER_EPIC)), None] {
            for document in [&epic_only, &ticket_only] {
                assert_eq!(
                    require_invocation_scope(
                        document,
                        epic(EPIC),
                        InvocationScope::Ticket { ticket_epic }
                    ),
                    Err(CallerRefusal::TicketOutsideEpic)
                );
            }
        }
    }

    #[test]
    fn only_an_active_open_seat_is_a_caller() {
        let active = seat("lsa", "LSA", |_| {});
        assert_eq!(require_active_seat(Some(active.clone())), Ok(active));
        assert_eq!(require_active_seat(None), Err(CallerRefusal::SeatInactive));
        for (why, edit) in [
            ("retired", serde_json::json!({"lifecycle": "retired"})),
            ("archived", serde_json::json!({"lifecycle": "archived"})),
            (
                "released",
                serde_json::json!({"released_at": "2026-10-02T11:00:00Z"}),
            ),
            (
                "replaced",
                serde_json::json!({"replaced_by": "01991c00-0000-7000-8000-0000000000a2"}),
            ),
        ] {
            let closed = seat("lsa", "LSA", |seat| {
                for (key, value) in edit.as_object().expect("an edit") {
                    seat[key] = value.clone();
                }
            });
            assert_eq!(
                require_active_seat(Some(closed)),
                Err(CallerRefusal::SeatInactive),
                "{why}"
            );
        }
    }

    #[test]
    fn the_seat_node_is_this_epics_and_active_in_that_order() {
        assert_eq!(
            require_active_epic_node(Some(&node(EPIC, "active")), epic(EPIC)),
            Ok(())
        );
        assert_eq!(
            require_active_epic_node(None, epic(EPIC)),
            Err(CallerRefusal::SeatOutsideEpic)
        );
        assert_eq!(
            require_active_epic_node(Some(&node(OTHER_EPIC, "active")), epic(EPIC)),
            Err(CallerRefusal::SeatOutsideEpic)
        );
        assert_eq!(
            require_active_epic_node(Some(&node(OTHER_EPIC, "retired")), epic(EPIC)),
            Err(CallerRefusal::SeatOutsideEpic),
            "another epic's node is refused as foreign before its lifecycle"
        );
        assert_eq!(
            require_active_epic_node(Some(&node(EPIC, "retired")), epic(EPIC)),
            Err(CallerRefusal::NodeInactive)
        );
    }

    #[test]
    fn only_the_current_hosted_generation_is_eligible() {
        assert_eq!(require_current_generation(3, Some(3)), Ok(()));
        assert_eq!(
            require_current_generation(3, None),
            Err(CallerRefusal::NoHostedOccupancy)
        );
        for (presented, current) in [(2, 3), (4, 3), (1, 2)] {
            assert_eq!(
                require_current_generation(presented, Some(current)),
                Err(CallerRefusal::FencedGeneration),
                "{presented} against {current}"
            );
        }
    }

    #[test]
    fn the_slot_or_catalog_role_is_an_allowed_caller_role_without_case() {
        let document = spec(&["lsa"], &["epic"]);
        assert_eq!(
            require_caller_role(&document, &seat("lsa", "XYZ", |_| {})),
            Ok(()),
            "slot role"
        );
        assert_eq!(
            require_caller_role(&document, &seat("other", "LSA", |_| {})),
            Ok(()),
            "catalog role, compared without case"
        );
        assert_eq!(
            require_caller_role(&document, &seat("tpm", "TPM", |_| {})),
            Err(CallerRefusal::RoleNotPermitted)
        );
    }
}
