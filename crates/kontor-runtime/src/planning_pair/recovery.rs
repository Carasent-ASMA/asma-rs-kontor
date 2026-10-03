//! A planning pair member's same-native recovery, decided purely from facts the
//! service read (ASMA-8282 B1b).
//!
//! The shared W3b application sequence calls these pure decisions after its
//! owner has:
//! - authenticated the frozen caller at its current hosted generation;
//! - derived the member's frozen context and its hash;
//! - canonicalized the intent;
//! - answered an exact replay.
//!
//! Then, stage by stage in the same order, the sequence calls:
//! 1. [`require_recoverable`], before it holds the run to the expected revision;
//! 2. [`plan`], once it has;
//! 3. [`RecoveryPlan::outcome`], on the runtime's answer to the readback the plan names.
//!
//! The sequence reaches every read, route refusal, readback, withdrawal and
//! compare-and-swap through `application::commands::recovery::RecoveryOwner`.
//! The daemon remains the sole production owner and writer. Nothing here
//! creates, replaces, discovers or substitutes a session, and a plan is not
//! a credential or a lease.

use kontor_core::DomainError;
use kontor_core::consultation::ConsultationRunState;
use kontor_core::id::{ContentHash, ExternalId};
use kontor_core::planning_pair::PlanningPairReadbackRefusal;
use kontor_core::repository::{
    StoredConsultationRun, StoredConsultationSeat, StoredPlanningPairKnownNative,
};
use kontor_core::state::NativeRuntimeIdentity;

use super::{PlanningPairMemberReconcileRequest, qualify_member_readback};
use crate::adapter::{ConsultationLaunchOutcome, RuntimeError, RuntimeResult};

/// A disposed pair is immutable to a new recovery.
///
/// # Errors
/// [`DomainError::Terminal`] for a disposed run.
pub fn require_recoverable(state: ConsultationRunState) -> Result<(), DomainError> {
    if state == ConsultationRunState::Disposed {
        return Err(DomainError::Terminal {
            subject: "PlanningPairRun",
        });
    }
    Ok(())
}

/// Why a recovery request does not name the member's known session. The
/// service maps each one to its own refusal; no variant carries wire text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryRefusal {
    /// The member's occupancy generation is not the one the caller read.
    MemberGenerationMoved,
    /// The member has neither a bound session nor a kept claim.
    NoKnownSession,
    /// The asserted native session is not the member's known one.
    NativeSessionDiffers,
    /// The asserted provider conversation is not the known session's.
    ProviderConversationDiffers,
    /// The member's frozen context, derived now, differs from its claim's.
    ContextDiffers,
}

/// What the service read for one member's recovery.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryFacts<'a> {
    /// The run, as read.
    pub run: &'a StoredConsultationRun,
    /// The member seat, as read.
    pub seat: &'a StoredConsultationSeat,
    /// The run's kept known-native claims, as read.
    pub known: &'a [StoredPlanningPairKnownNative],
    /// The member's frozen context hash, as derived now.
    pub context_hash: &'a ContentHash,
    /// The frozen placement's hash.
    pub placement_hash: &'a ContentHash,
}

/// What the recovery request asserts; it only asserts, and names nothing the
/// service then uses in place of what it read.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryAssertion<'a> {
    /// The member generation the caller read.
    pub member_occupancy_generation: u64,
    /// The native session the caller read.
    pub native_identity: &'a NativeRuntimeIdentity,
    /// The provider conversation the caller read, if any.
    pub provider_session_id: Option<&'a ExternalId>,
}

/// The member's known session, held to the request, and the claim the
/// readback is verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryPlan {
    verified: StoredPlanningPairKnownNative,
    qualified: bool,
}

/// Hold one member's known session to what the request asserts.
///
/// The known session is the bound seat's, or else its kept claim's; never one
/// the request names.
///
/// # Errors
/// In order:
/// 1. another member generation;
/// 2. no known session;
/// 3. another native session;
/// 4. another provider conversation;
/// 5. a claim launched under another frozen context.
pub fn plan(
    facts: RecoveryFacts<'_>,
    asserted: RecoveryAssertion<'_>,
) -> Result<RecoveryPlan, RecoveryRefusal> {
    let seat = facts.seat;
    if seat.occupancy_generation != asserted.member_occupancy_generation {
        return Err(RecoveryRefusal::MemberGenerationMoved);
    }
    let claim = facts
        .known
        .iter()
        .find(|known| known.seat_binding_id == seat.seat_binding_id);
    let (identity, provider_session_id) = match (&seat.native_identity, claim) {
        (Some(identity), _) => (identity.clone(), seat.provider_session_id.clone()),
        (None, Some(claim)) => (claim.identity.clone(), claim.provider_session_id.clone()),
        (None, None) => return Err(RecoveryRefusal::NoKnownSession),
    };
    if identity != *asserted.native_identity {
        return Err(RecoveryRefusal::NativeSessionDiffers);
    }
    if provider_session_id.as_ref() != asserted.provider_session_id {
        return Err(RecoveryRefusal::ProviderConversationDiffers);
    }
    if claim.is_some_and(|claim| claim.context_hash != *facts.context_hash) {
        return Err(RecoveryRefusal::ContextDiffers);
    }
    Ok(RecoveryPlan {
        verified: StoredPlanningPairKnownNative {
            run_id: facts.run.id,
            project_id: facts.run.project_id,
            seat_binding_id: seat.seat_binding_id,
            occupancy_generation: seat.occupancy_generation,
            identity,
            provider_session_id,
            context_hash: facts.context_hash.clone(),
            placement_hash: facts.placement_hash.clone(),
            readback_refusal: claim.and_then(|claim| claim.readback_refusal),
            observed_at: claim
                .map(|claim| claim.observed_at)
                .or(seat.observed_at)
                .unwrap_or(facts.run.updated_at),
        },
        qualified: seat.native_identity.is_some(),
    })
}

/// What the runtime's readback of the known session means for the member.
#[derive(Debug)]
pub enum RecoveryOutcome {
    /// The same session, read back qualified: the service may bind it.
    Requalify(Box<ConsultationLaunchOutcome>),
    /// An adverse answer: the service withdraws only this member's current
    /// qualification, if it held one, and refuses.
    Withdraw(WithdrawReason),
    /// No observation at all: the service answers the runtime's own error.
    NoObservation(RuntimeError),
}

/// Why an adverse readback withdraws the member's qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithdrawReason {
    /// The runtime's own stale-binding rule.
    Runtime(&'static str),
    /// The session no longer carries its frozen correlation.
    CorrelationLost,
    /// The runtime answered with another session, or with a create.
    NotTheKnownSession,
    /// The readback named another provider conversation for the session.
    AnotherProviderConversation,
    /// The readback did not qualify the member.
    Unqualified(PlanningPairReadbackRefusal),
}

impl RecoveryPlan {
    /// The member's known native session.
    #[must_use]
    pub const fn identity(&self) -> &NativeRuntimeIdentity {
        &self.verified.identity
    }

    /// The known session's provider conversation, if one is recorded.
    #[must_use]
    pub const fn provider_session_id(&self) -> Option<&ExternalId> {
        self.verified.provider_session_id.as_ref()
    }

    /// The claim the readback is verified against.
    #[must_use]
    pub const fn verified(&self) -> &StoredPlanningPairKnownNative {
        &self.verified
    }

    /// Whether the member holds a current qualification an adverse readback
    /// withdraws: only a bound seat does.
    #[must_use]
    pub const fn holds_qualification(&self) -> bool {
        self.qualified
    }

    /// Classify the runtime's answer to `request`, the readback of this
    /// plan's known session under the member's frozen context.
    #[must_use]
    pub fn outcome(
        &self,
        request: &PlanningPairMemberReconcileRequest,
        answer: RuntimeResult<ConsultationLaunchOutcome>,
    ) -> RecoveryOutcome {
        let outcome = match answer {
            Err(RuntimeError::StaleBinding { rule }) => {
                return RecoveryOutcome::Withdraw(WithdrawReason::Runtime(rule));
            }
            Err(RuntimeError::CorrelationFailed) => {
                return RecoveryOutcome::Withdraw(WithdrawReason::CorrelationLost);
            }
            Err(other) => return RecoveryOutcome::NoObservation(other),
            Ok(outcome) => outcome,
        };
        if request.require_same_native(&outcome).is_err() {
            return RecoveryOutcome::Withdraw(WithdrawReason::NotTheKnownSession);
        }
        let known_session = self.provider_session_id();
        if known_session.is_some() && outcome.provider_session_id.as_ref() != known_session {
            return RecoveryOutcome::Withdraw(WithdrawReason::AnotherProviderConversation);
        }
        match qualify_member_readback(&outcome, Some(&request.context.requested_fleet_provenance)) {
            Ok(()) => RecoveryOutcome::Requalify(Box::new(outcome)),
            Err(refusal) => RecoveryOutcome::Withdraw(WithdrawReason::Unqualified(refusal)),
        }
    }

    /// The claim a qualified readback binds: the verified one, as observed now.
    #[must_use]
    pub fn requalified(
        &self,
        outcome: &ConsultationLaunchOutcome,
    ) -> StoredPlanningPairKnownNative {
        StoredPlanningPairKnownNative {
            provider_session_id: outcome.provider_session_id.clone(),
            readback_refusal: None,
            observed_at: outcome.observed_at,
            ..self.verified.clone()
        }
    }
}

#[cfg(test)]
mod tests;
