//! The same-native recovery sequence, behind the existing owner's boundary.

use std::sync::Arc;

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId,
    IdempotencyKey, PlanningPairRunId, ProjectId, SeatBindingId,
};
use kontor_core::planning_pair::{PlanningPairSlot, PlanningPairSpec};
use kontor_core::receipt::AggregateRef;
use kontor_core::repository::{
    PlanningPairMemberReadback, StoredConsultationRun, StoredConsultationSeat,
};
use kontor_core::state::NativeRuntimeIdentity;

use super::CommandOwner;
use crate::adapter::{RuntimeAdapter, RuntimeError};
use crate::planning_pair::application::{Applied, PairState};
use crate::planning_pair::intent as fingerprint;
use crate::planning_pair::recovery::{
    self as decision, RecoveryOutcome, RecoveryRefusal, WithdrawReason,
};
use crate::planning_pair::{PlanningPairLaunchContext, PlanningPairMemberReconcileRequest};

/// The assertion made by a recovery body, never a substitute for known facts.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The run revision the caller read.
    pub expected_revision: AggregateRevision,
    /// The member occupancy the caller read.
    pub expected_member_generation: u64,
    /// The known native identity that the request asserts.
    pub expected_identity: &'a NativeRuntimeIdentity,
    /// The known provider conversation that the request asserts.
    pub expected_provider_session_id: Option<&'a ExternalId>,
}

/// The unchanged facts rendered by the recovery's owner.
pub struct Answer<'a> {
    /// The run's current restored state.
    pub pair: &'a PairState,
    /// The member whose unchanged seat is being reported.
    pub seat: &'a StoredConsultationSeat,
    /// The member's frozen slot.
    pub slot: PlanningPairSlot,
    /// The exact native identity being reported.
    pub identity: &'a NativeRuntimeIdentity,
    /// The provider conversation being reported.
    pub provider_session_id: Option<&'a ExternalId>,
    /// The original applied revision, including on replay.
    pub revision: AggregateRevision,
    /// The applied receipt's id.
    pub receipt: CommandReceiptId,
    /// Whether this request wrote the receipt.
    pub applied: Applied,
}

/// The same-native recovery's authenticated writer and runtime boundary.
#[async_trait::async_trait]
pub trait RecoveryOwner: CommandOwner {
    /// The owner's native-activity guard, held through the whole command.
    type Activity: Send;
    /// The owner's answer.
    type Output: Send;

    /// Hold the native-activity guard.
    fn begin_native_activity(&self) -> Result<Self::Activity, Self::Error>;
    /// Authenticate the frozen caller under its pinned scope and roles.
    fn authenticate_recovering_caller(
        &self,
        run: &StoredConsultationRun,
        spec: &PlanningPairSpec,
        actor: Self::Actor,
    ) -> Result<(), Self::Error>;
    /// Refuse a seat which is not one of the frozen members.
    fn no_such_member(&self) -> Self::Error;
    /// Derive this member's frozen context from the owner's durable state.
    fn member_context(
        &self,
        run: &StoredConsultationRun,
        pair: &PairState,
        seat: &StoredConsultationSeat,
        slot: PlanningPairSlot,
    ) -> Result<PlanningPairLaunchContext, Self::Error>;
    /// Refuse the pure recovery plan's unchanged reason.
    fn recovery_refusal(&self, refusal: RecoveryRefusal) -> Self::Error;
    /// Check the owner's existing state boundary before obtaining its runtime.
    fn ensure_state(&self) -> Result<(), Self::Error>;
    /// Obtain the existing runtime adapter.
    fn runtime(&self) -> Result<Arc<dyn RuntimeAdapter>, Self::Error>;
    /// Refuse unsupported routes before the readback or any effect.
    fn require_member_routes(
        &self,
        runtime: &dyn RuntimeAdapter,
        pair: &PairState,
    ) -> Result<(), Self::Error>;
    /// Answer the runtime's error, with the owner's existing realm mapping.
    fn runtime_error(&self, error: &RuntimeError) -> Self::Error;
    /// Withdraw only this member's qualification under the existing CAS.
    fn disqualify(&self, readback: &PlanningPairMemberReadback) -> Result<(), Self::Error>;
    /// Render the baseline adverse-readback refusal.
    fn withdrawn(&self, reason: WithdrawReason, identity: &NativeRuntimeIdentity) -> Self::Error;
    /// Wait at the existing test-only recovery write barrier.
    async fn recovery_hold_point(&self);
    /// Requalify and write its receipt in the owner's single transaction.
    fn requalify(
        &self,
        readback: &PlanningPairMemberReadback,
        key: &IdempotencyKey,
        target: AggregateRef,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> Result<(StoredConsultationRun, CommandReceiptId, bool), Self::Error>;
    /// Render the current state and unchanged member/session facts.
    fn render(&self, answer: Answer<'_>) -> Result<Self::Output, Self::Error>;
}

/// Requalify the caller's member on its exact already-known native session.
///
/// # Errors
/// The unchanged owner/domain/readback refusals, before any unauthorized
/// native effect or persistence. This operation creates no native session.
#[allow(
    clippy::too_many_arguments,
    reason = "all existing recovery inputs remain explicit"
)]
pub async fn recover<O: RecoveryOwner>(
    owner: &O,
    key: &IdempotencyKey,
    project_id: ProjectId,
    run_id: PlanningPairRunId,
    seat_binding_id: SeatBindingId,
    caller: O::Actor,
    input: Input<'_>,
) -> Result<O::Output, O::Error> {
    let _native_activity = owner.begin_native_activity()?;
    let run = owner.stored_run(project_id, run_id)?;
    let pair = owner.pair_state(&run)?;
    owner.authenticate_recovering_caller(&run, &pair.spec, caller)?;
    let seat = pair
        .seats
        .iter()
        .find(|seat| seat.seat_binding_id == seat_binding_id)
        .cloned()
        .ok_or_else(|| owner.no_such_member())?;
    let slot = PlanningPairSlot::parse(seat.role_slot_id.as_str())
        .map_err(|error| owner.domain_error(&error))?;
    let context = owner.member_context(&run, &pair, &seat, slot)?;
    let context_hash = context
        .frozen_hash()
        .map_err(|error| owner.domain_error(&error))?;
    let placement_hash = pair.placement.placement.hash().clone();
    let intent = recovery_intent(
        owner,
        project_id,
        &pair,
        &seat,
        slot,
        caller,
        input,
        &context_hash,
        &placement_hash,
    )?;
    let target = AggregateRef::MiniProject {
        mini_project_id: run.mini_project_id,
    };
    if let Some(receipt) = owner.replayed(key, &intent, &target)? {
        return owner.render(Answer {
            pair: &pair,
            seat: &seat,
            slot,
            identity: input.expected_identity,
            provider_session_id: input.expected_provider_session_id,
            revision: receipt.revision,
            receipt: receipt.id,
            applied: Applied::Unchanged,
        });
    }
    decision::require_recoverable(run.state).map_err(|error| owner.domain_error(&error))?;
    owner.expect_revision(&run, input.expected_revision)?;
    let plan = decision::plan(
        decision::RecoveryFacts {
            run: &run,
            seat: &seat,
            known: &pair.known,
            context_hash: &context_hash,
            placement_hash: &placement_hash,
        },
        decision::RecoveryAssertion {
            member_occupancy_generation: input.expected_member_generation,
            native_identity: input.expected_identity,
            provider_session_id: input.expected_provider_session_id,
        },
    )
    .map_err(|refusal| owner.recovery_refusal(refusal))?;
    complete(
        owner, key, &pair, &seat, slot, context, plan, target, &intent,
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "all authenticated recovery facts stay explicit"
)]
async fn complete<O: RecoveryOwner>(
    owner: &O,
    key: &IdempotencyKey,
    pair: &PairState,
    seat: &StoredConsultationSeat,
    slot: PlanningPairSlot,
    context: PlanningPairLaunchContext,
    plan: decision::RecoveryPlan,
    target: AggregateRef,
    intent: &CanonicalDocument,
) -> Result<O::Output, O::Error> {
    let run = &pair.run;
    let project_id = run.project_id;
    let seat_binding_id = seat.seat_binding_id;
    owner.ensure_state()?;
    let adapter = owner.runtime()?;
    owner.require_member_routes(adapter.as_ref(), pair)?;
    let request = PlanningPairMemberReconcileRequest {
        context,
        identity: plan.identity().clone(),
        requested_at: owner.now(),
    };
    let readback = adapter.reconcile_planning_pair_member(&request).await;
    let outcome = match plan.outcome(&request, readback) {
        RecoveryOutcome::Requalify(outcome) => *outcome,
        RecoveryOutcome::NoObservation(error) => return Err(owner.runtime_error(&error)),
        RecoveryOutcome::Withdraw(reason) => {
            if plan.holds_qualification() {
                owner.disqualify(&PlanningPairMemberReadback {
                    project_id,
                    run_id: run.id,
                    seat_binding_id,
                    expected_revision: run.revision,
                    occupancy_generation: seat.occupancy_generation,
                    verified: plan.verified().clone(),
                    applied_at: owner.now(),
                })?;
            }
            return Err(owner.withdrawn(reason, plan.identity()));
        }
    };
    owner.recovery_hold_point().await;
    let now = owner.now();
    let next = run
        .revision
        .next()
        .map_err(|error| owner.domain_error(&error))?;
    let readback = PlanningPairMemberReadback {
        project_id,
        run_id: run.id,
        seat_binding_id,
        expected_revision: run.revision,
        occupancy_generation: seat.occupancy_generation,
        verified: plan.requalified(&outcome),
        applied_at: now,
    };
    let (after, receipt, created) = owner.requalify(&readback, key, target, next, intent)?;
    let pair = owner.pair_state(&after)?;
    owner.render(Answer {
        pair: &pair,
        seat,
        slot,
        identity: plan.identity(),
        provider_session_id: outcome.provider_session_id.as_ref(),
        revision: next,
        receipt,
        applied: if created {
            Applied::Created
        } else {
            Applied::Unchanged
        },
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "every existing fingerprint fact stays explicit"
)]
fn recovery_intent<O: RecoveryOwner>(
    owner: &O,
    project_id: ProjectId,
    pair: &PairState,
    seat: &StoredConsultationSeat,
    slot: PlanningPairSlot,
    caller: O::Actor,
    input: Input<'_>,
    context_hash: &ContentHash,
    placement_hash: &ContentHash,
) -> Result<CanonicalDocument, O::Error> {
    owner.canonical(
        &fingerprint::Recovery {
            project_id,
            run_id: pair.run.id,
            member_seat_binding_id: seat.seat_binding_id,
            slot,
            caller: owner.presented(caller),
            expected_run_revision: input.expected_revision,
            expected_member_occupancy_generation: input.expected_member_generation,
            expected_native_identity: input.expected_identity,
            expected_provider_session_id: input.expected_provider_session_id,
            member_context_hash: context_hash,
            placement_hash,
        }
        .document(),
    )
}
