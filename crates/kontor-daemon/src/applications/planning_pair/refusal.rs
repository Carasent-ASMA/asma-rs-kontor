//! How the planning pair service answers each typed refusal of the shared
//! runtime core (ASMA-8282 B1a): the status, code and rule it answered before
//! those decisions moved, and nothing else. The runtime names no wire text.

use kontor_api::error::ApiErrorCode;
use kontor_runtime::planning_pair::caller::{CallerRefusal, FrozenCallerAct};
use kontor_runtime::planning_pair::context::ContextRefusal;

/// The refusal one frozen-caller eligibility decision is answered with: the
/// same status, code and rule the service answered before the decision moved
/// to the shared runtime core (ASMA-8282 B1a).
pub(super) const fn caller_refusal_rule(refusal: CallerRefusal) -> (ApiErrorCode, &'static str) {
    match refusal {
        CallerRefusal::NotFrozenCaller(FrozenCallerAct::Decide) => (
            ApiErrorCode::Forbidden,
            "only the planning pair's frozen caller asks for clarification or records the disposition",
        ),
        CallerRefusal::NotFrozenCaller(FrozenCallerAct::Recover) => (
            ApiErrorCode::Forbidden,
            "only the planning pair's frozen caller recovers one of its members",
        ),
        CallerRefusal::TicketOutsideEpic => (
            ApiErrorCode::Forbidden,
            "the requested ticket does not belong to this epic",
        ),
        CallerRefusal::TicketScopeNotPermitted => (
            ApiErrorCode::Forbidden,
            "the pinned planning pair document does not permit ticket-scoped invocation",
        ),
        CallerRefusal::EpicScopeNotPermitted => (
            ApiErrorCode::Forbidden,
            "the pinned planning pair document does not permit epic-scoped invocation",
        ),
        CallerRefusal::SeatInactive => (
            ApiErrorCode::StaleBinding,
            "the authenticated caller seat is not active",
        ),
        CallerRefusal::SeatOutsideEpic => (
            ApiErrorCode::Forbidden,
            "the authenticated caller seat does not belong to this epic",
        ),
        CallerRefusal::NodeInactive => (
            ApiErrorCode::StaleBinding,
            "the authenticated caller seat's topology node is not active",
        ),
        CallerRefusal::NoHostedOccupancy => (
            ApiErrorCode::StaleBinding,
            "the authenticated caller seat has no active hosted native occupancy",
        ),
        CallerRefusal::FencedGeneration => (
            ApiErrorCode::StaleBinding,
            "the caller credential belongs to a fenced native occupancy generation",
        ),
        CallerRefusal::RoleNotPermitted => (
            ApiErrorCode::Forbidden,
            "the authenticated seat's role may not convene this planning pair",
        ),
    }
}

/// The refusal one frozen-context derivation is answered with, as
/// [`caller_refusal_rule`] does for the caller.
pub(super) const fn context_refusal_rule(refusal: ContextRefusal) -> &'static str {
    match refusal {
        ContextRefusal::NotPlanningPair => "a planning pair member launch requires a planning pair",
        ContextRefusal::DocumentPinDiffers => {
            "the member's document pin differs from the one the pair was frozen under"
        }
        ContextRefusal::SeatAbsentFromPlacement => {
            "a frozen planning pair seat is absent from its placement"
        }
        ContextRefusal::RouteDiffers => {
            "the member seat's route differs from the route its placement froze"
        }
        ContextRefusal::NoOccupancyGeneration => "the member seat has no occupancy generation",
        ContextRefusal::PlacementDiffers => "the frozen run names another placement",
        ContextRefusal::NoPinnedTeamDefinition => "the epic has no pinned Team Definition",
        ContextRefusal::TeamDefinitionDiffers => {
            "the epic's pinned Team Definition differs from the one the pair was frozen under"
        }
        ContextRefusal::TopologyDiffers => {
            "the member container's topology differs from the epic's pinned topology"
        }
        ContextRefusal::NoFleetProvenance => {
            "the frozen placement names no fleet provenance for this member"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each refusal against the literal the service answered at `be9537aa`,
    /// copied with its line in `applications/planning_pair.rs` there.
    #[test]
    fn every_caller_refusal_answers_its_baseline_code_and_rule() {
        let baseline: [(CallerRefusal, ApiErrorCode, &str); 11] = [
            (
                CallerRefusal::NotFrozenCaller(FrozenCallerAct::Decide),
                ApiErrorCode::Forbidden,
                // be9537aa:2242
                "only the planning pair's frozen caller asks for clarification or records the disposition",
            ),
            (
                CallerRefusal::NotFrozenCaller(FrozenCallerAct::Recover),
                ApiErrorCode::Forbidden,
                // be9537aa:2077
                "only the planning pair's frozen caller recovers one of its members",
            ),
            (
                CallerRefusal::TicketOutsideEpic,
                ApiErrorCode::Forbidden,
                // be9537aa:568
                "the requested ticket does not belong to this epic",
            ),
            (
                CallerRefusal::TicketScopeNotPermitted,
                ApiErrorCode::Forbidden,
                // be9537aa:574
                "the pinned planning pair document does not permit ticket-scoped invocation",
            ),
            (
                CallerRefusal::EpicScopeNotPermitted,
                ApiErrorCode::Forbidden,
                // be9537aa:580
                "the pinned planning pair document does not permit epic-scoped invocation",
            ),
            (
                CallerRefusal::SeatInactive,
                ApiErrorCode::StaleBinding,
                // be9537aa:614
                "the authenticated caller seat is not active",
            ),
            (
                CallerRefusal::SeatOutsideEpic,
                ApiErrorCode::Forbidden,
                // be9537aa:624
                "the authenticated caller seat does not belong to this epic",
            ),
            (
                CallerRefusal::NodeInactive,
                ApiErrorCode::StaleBinding,
                // be9537aa:630
                "the authenticated caller seat's topology node is not active",
            ),
            (
                CallerRefusal::NoHostedOccupancy,
                ApiErrorCode::StaleBinding,
                // be9537aa:641
                "the authenticated caller seat has no active hosted native occupancy",
            ),
            (
                CallerRefusal::FencedGeneration,
                ApiErrorCode::StaleBinding,
                // be9537aa:647
                "the caller credential belongs to a fenced native occupancy generation",
            ),
            (
                CallerRefusal::RoleNotPermitted,
                ApiErrorCode::Forbidden,
                // be9537aa:592
                "the authenticated seat's role may not convene this planning pair",
            ),
        ];
        for (refusal, code, rule) in baseline {
            assert_eq!(caller_refusal_rule(refusal), (code, rule), "{refusal:?}");
        }
    }

    /// Every context refusal was `PlacementBlocked` at `be9537aa`, through
    /// one local `refuse` closure, with these rules.
    #[test]
    fn every_context_refusal_answers_its_baseline_rule() {
        let baseline: [(ContextRefusal, &str); 10] = [
            // be9537aa:1245
            (
                ContextRefusal::NotPlanningPair,
                "a planning pair member launch requires a planning pair",
            ),
            // be9537aa:1254
            (
                ContextRefusal::DocumentPinDiffers,
                "the member's document pin differs from the one the pair was frozen under",
            ),
            // be9537aa:1263
            (
                ContextRefusal::SeatAbsentFromPlacement,
                "a frozen planning pair seat is absent from its placement",
            ),
            // be9537aa:1266
            (
                ContextRefusal::RouteDiffers,
                "the member seat's route differs from the route its placement froze",
            ),
            // be9537aa:1270
            (
                ContextRefusal::NoOccupancyGeneration,
                "the member seat has no occupancy generation",
            ),
            // be9537aa:1275
            (
                ContextRefusal::PlacementDiffers,
                "the frozen run names another placement",
            ),
            // be9537aa:1283
            (
                ContextRefusal::NoPinnedTeamDefinition,
                "the epic has no pinned Team Definition",
            ),
            // be9537aa:1293
            (
                ContextRefusal::TeamDefinitionDiffers,
                "the epic's pinned Team Definition differs from the one the pair was frozen under",
            ),
            // be9537aa:1298
            (
                ContextRefusal::TopologyDiffers,
                "the member container's topology differs from the epic's pinned topology",
            ),
            // be9537aa:1307
            (
                ContextRefusal::NoFleetProvenance,
                "the frozen placement names no fleet provenance for this member",
            ),
        ];
        for (refusal, rule) in baseline {
            assert_eq!(context_refusal_rule(refusal), rule, "{refusal:?}");
        }
    }
}
