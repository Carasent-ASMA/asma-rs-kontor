//! How the planning pair service answers each typed refusal of the shared
//! runtime core (ASMA-8282 B1a): the status, code and rule it answered before
//! those decisions moved, and nothing else. The runtime names no wire text.

use kontor_api::error::ApiErrorCode;
use kontor_runtime::planning_pair::caller::{CallerRefusal, FrozenCallerAct};
use kontor_runtime::planning_pair::context::ContextRefusal;
use kontor_runtime::planning_pair::recovery::{RecoveryRefusal, WithdrawReason};

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

/// The refusal a recovery whose request does not name the member's known
/// session is answered with (ASMA-8282 B1b).
pub(super) const fn recovery_refusal_rule(
    refusal: RecoveryRefusal,
) -> (ApiErrorCode, &'static str) {
    match refusal {
        RecoveryRefusal::MemberGenerationMoved => (
            ApiErrorCode::StaleBinding,
            "the member's occupancy generation is not the one the caller read",
        ),
        RecoveryRefusal::NoKnownSession => (
            ApiErrorCode::Unavailable,
            "the planning pair member has no known native session; nothing is discovered, created or substituted",
        ),
        RecoveryRefusal::NativeSessionDiffers => (
            ApiErrorCode::StaleBinding,
            "the expected native session is not the member's known one",
        ),
        RecoveryRefusal::ProviderConversationDiffers => (
            ApiErrorCode::StaleBinding,
            "the expected provider conversation is not the one recorded for the member's known session",
        ),
        RecoveryRefusal::ContextDiffers => (
            ApiErrorCode::PlacementBlocked,
            "the member's frozen context differs from the one its known session was launched under",
        ),
    }
}

/// The rule an adverse readback is refused with; always `Unavailable`.
pub(super) const fn withdraw_rule(reason: WithdrawReason) -> &'static str {
    match reason {
        WithdrawReason::Runtime(rule) => rule,
        WithdrawReason::CorrelationLost => {
            "the planning pair member's known native session no longer carries its frozen correlation"
        }
        WithdrawReason::NotTheKnownSession => {
            "the runtime did not answer with the member's known native session"
        }
        WithdrawReason::AnotherProviderConversation => {
            "the readback reported another provider conversation for the member's known session"
        }
        WithdrawReason::Unqualified(refusal) => super::readback_refusal_rule(Some(refusal)),
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

    /// Each recovery refusal against the literal the service answered at
    /// `8cd2305f`, copied with its line in `applications/planning_pair.rs`.
    #[test]
    fn every_recovery_refusal_answers_its_baseline_code_and_rule() {
        let baseline: [(RecoveryRefusal, ApiErrorCode, &str); 5] = [
            (
                RecoveryRefusal::MemberGenerationMoved,
                ApiErrorCode::StaleBinding,
                // 8cd2305f:1728
                "the member's occupancy generation is not the one the caller read",
            ),
            (
                RecoveryRefusal::NoKnownSession,
                ApiErrorCode::Unavailable,
                // 8cd2305f:1744
                "the planning pair member has no known native session; nothing is discovered, created or substituted",
            ),
            (
                RecoveryRefusal::NativeSessionDiffers,
                ApiErrorCode::StaleBinding,
                // 8cd2305f:1751
                "the expected native session is not the member's known one",
            ),
            (
                RecoveryRefusal::ProviderConversationDiffers,
                ApiErrorCode::StaleBinding,
                // 8cd2305f:1757
                "the expected provider conversation is not the one recorded for the member's known session",
            ),
            (
                RecoveryRefusal::ContextDiffers,
                ApiErrorCode::PlacementBlocked,
                // 8cd2305f:1766
                "the member's frozen context differs from the one its known session was launched under",
            ),
        ];
        for (refusal, code, rule) in baseline {
            assert_eq!(recovery_refusal_rule(refusal), (code, rule), "{refusal:?}");
        }
    }

    /// Every adverse readback was `Unavailable` at `8cd2305f`, with the
    /// runtime's own rule or these.
    #[test]
    fn every_withdrawal_answers_its_baseline_rule() {
        let baseline: [(WithdrawReason, &str); 6] = [
            (
                WithdrawReason::Runtime("the runtime's own rule"),
                "the runtime's own rule",
            ),
            // 8cd2305f:1800
            (
                WithdrawReason::CorrelationLost,
                "the planning pair member's known native session no longer carries its frozen correlation",
            ),
            // 8cd2305f:1805
            (
                WithdrawReason::NotTheKnownSession,
                "the runtime did not answer with the member's known native session",
            ),
            // 8cd2305f:1808
            (
                WithdrawReason::AnotherProviderConversation,
                "the readback reported another provider conversation for the member's known session",
            ),
            // 8cd2305f:1816, through `readback_refusal_rule`
            (
                WithdrawReason::Unqualified(
                    kontor_core::planning_pair::PlanningPairReadbackRefusal::ToolRestrictionUnobserved,
                ),
                "the planning pair member's readback did not observe its closed tool restriction",
            ),
            (
                WithdrawReason::Unqualified(
                    kontor_core::planning_pair::PlanningPairReadbackRefusal::ProvenanceUnconfirmed,
                ),
                "the planning pair member's native readback did not confirm its frozen provenance, so it is not qualified to contribute",
            ),
        ];
        for (reason, rule) in baseline {
            assert_eq!(withdraw_rule(reason), rule, "{reason:?}");
        }
    }
}
