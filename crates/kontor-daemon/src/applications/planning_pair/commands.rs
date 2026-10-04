//! The daemon's existing boundary for the shared W3b command sequences.
//!
//! These ports retain the exact authenticated actor, store/CAS and receipt
//! writers. No second production owner or new authorization path is added.

use super::*;
use kontor_runtime::planning_pair::application::Applied;
use kontor_runtime::planning_pair::application::commands::contribution::ContributionOwner;
use kontor_runtime::planning_pair::application::commands::recovery::{Answer, RecoveryOwner};
use kontor_runtime::planning_pair::application::commands::{CommandOwner, Replay};
use kontor_runtime::planning_pair::recovery::{RecoveryRefusal, WithdrawReason};

pub(super) struct CommandPorts<'a>(pub(super) &'a Services);

impl CommandOwner for CommandPorts<'_> {
    type Error = ApiError;
    type Actor = PlanningPairSeat;

    fn domain_error(&self, error: &kontor_core::DomainError) -> ApiError {
        self.0.refuse_domain(error)
    }

    fn now(&self) -> Timestamp {
        kontor_api::now()
    }

    fn stored_run(
        &self,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, ApiError> {
        self.0.stored_planning_pair(project_id, run_id)
    }

    fn pair_state(&self, run: &StoredConsultationRun) -> Result<PairState, ApiError> {
        self.0.planning_pair_state(run)
    }

    fn presented(&self, actor: PlanningPairSeat) -> SeatGeneration {
        presented(actor)
    }

    fn canonical(&self, document: &serde_json::Value) -> Result<CanonicalDocument, ApiError> {
        self.0.intent(document)
    }

    fn replayed(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        target: &AggregateRef,
    ) -> Result<Option<Replay>, ApiError> {
        Ok(self
            .0
            .replayed(key, intent, Some(target))?
            .map(|receipt| Replay {
                id: receipt.id,
                revision: receipt.target_revision,
            }))
    }

    fn expect_revision(
        &self,
        run: &StoredConsultationRun,
        expected: AggregateRevision,
    ) -> Result<(), ApiError> {
        self.0.expect_planning_pair_revision(run, expected)
    }
}

impl ContributionOwner for CommandPorts<'_> {
    type Output = PlanningPairRunDto;

    fn authenticate_member(
        &self,
        pair: &PairState,
        member: PlanningPairSeat,
    ) -> Result<PlanningPairSlot, ApiError> {
        self.0.authenticated_member(pair, member, true)
    }

    fn authenticate_caller(
        &self,
        run: &StoredConsultationRun,
        caller: PlanningPairSeat,
    ) -> Result<(), ApiError> {
        self.0.authenticated_caller(run, caller)
    }

    fn missing_clarification(&self) -> ApiError {
        self.0.deny(
            ApiErrorCode::Unavailable,
            "the accepted answer has no stored clarification",
        )
    }

    fn append_revision(
        &self,
        pair: &PairState,
        state: ConsultationRunState,
        contribution: Option<&StoredPlanningPairContribution>,
    ) -> Result<StoredConsultationRun, ApiError> {
        self.0
            .append_planning_pair_revision(pair, state, contribution)
    }

    fn record(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        kind: CommandKind,
        target: AggregateRef,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> Result<CommandReceiptId, ApiError> {
        self.0
            .record(key, project_id, kind, target, revision, intent)
    }

    fn render(
        &self,
        pair: &PairState,
        viewer: kontor_runtime::planning_pair::application::commands::Viewer,
        receipt: CommandReceiptId,
        applied: Applied,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let viewer = match viewer {
            kontor_runtime::planning_pair::application::commands::Viewer::Caller => Viewer::Caller,
            kontor_runtime::planning_pair::application::commands::Viewer::Member(slot) => {
                Viewer::Member(slot)
            }
        };
        self.0
            .planning_pair_dto(pair, viewer, Some((receipt, applied_dto(applied))))
    }
}

#[async_trait::async_trait]
impl<'a> RecoveryOwner for CommandPorts<'a> {
    type Activity = tokio::sync::RwLockReadGuard<'a, ()>;
    type Output = PlanningPairSeatRecoveryDto;

    fn begin_native_activity(&self) -> Result<Self::Activity, ApiError> {
        self.0.native_activity()
    }

    fn authenticate_recovering_caller(
        &self,
        run: &StoredConsultationRun,
        spec: &PlanningPairSpec,
        caller: PlanningPairSeat,
    ) -> Result<(), ApiError> {
        self.0.authenticated_recovering_caller(run, spec, caller)
    }

    fn no_such_member(&self) -> ApiError {
        self.0.deny(
            ApiErrorCode::NotFound,
            "the planning pair has no such member seat",
        )
    }

    fn member_context(
        &self,
        run: &StoredConsultationRun,
        pair: &PairState,
        seat: &StoredConsultationSeat,
        slot: PlanningPairSlot,
    ) -> Result<kontor_runtime::planning_pair::PlanningPairLaunchContext, ApiError> {
        self.0.planning_pair_member_context(run, pair, seat, slot)
    }

    fn recovery_refusal(&self, refusal: RecoveryRefusal) -> ApiError {
        let (code, rule) = recovery_refusal_rule(refusal);
        self.0.deny(code, rule)
    }

    fn ensure_state(&self) -> Result<(), ApiError> {
        self.0.state().map(|_| ())
    }

    fn runtime(&self) -> Result<std::sync::Arc<dyn kontor_runtime::RuntimeAdapter>, ApiError> {
        self.0.planning_pair_runtime()
    }

    fn require_member_routes(
        &self,
        runtime: &dyn kontor_runtime::RuntimeAdapter,
        pair: &PairState,
    ) -> Result<(), ApiError> {
        self.0
            .require_planning_pair_member_routes(runtime, pair.pair.members().members())
    }

    fn runtime_error(&self, error: &kontor_runtime::RuntimeError) -> ApiError {
        ApiError::from_runtime(self.0.realm_id, error)
    }

    fn disqualify(&self, readback: &PlanningPairMemberReadback) -> Result<(), ApiError> {
        self.0
            .state()?
            .with_store(|store| store.disqualify_planning_pair_member(readback))
            .map(|_| ())
            .map_err(|error| self.0.refuse_recovery_write(&error))
    }

    fn withdrawn(&self, reason: WithdrawReason, identity: &NativeRuntimeIdentity) -> ApiError {
        self.0.deny(ApiErrorCode::Unavailable, withdraw_rule(reason))
            .about("planning pair member readback")
            .located_at(format!("native/{}", identity.native_id.as_str()))
            .advising("confirmation unknown: the member's known native session is kept and is not qualified now; nothing was created, replaced or archived")
    }

    async fn recovery_hold_point(&self) {
        self.0.planning_pair_recovery_hold_point().await;
    }

    fn requalify(
        &self,
        readback: &PlanningPairMemberReadback,
        key: &IdempotencyKey,
        target: AggregateRef,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> Result<(StoredConsultationRun, CommandReceiptId, bool), ApiError> {
        let state = self.0.state()?;
        let envelope = ReceiptEnvelope::new(
            state.realm_id(),
            NewLocalCommand {
                project_id: readback.project_id,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key.clone(),
                kind: CommandKind::RecoverPlanningPairSeat,
                target,
                target_revision: revision,
                intent: intent.clone(),
                created_at: readback.applied_at,
            },
        );
        let answer = state
            .with_store(|store| store.requalify_planning_pair_member(readback, &envelope))
            .map_err(|error| self.0.refuse_recovery_write(&error))?;
        state.signals().appended();
        Ok(answer)
    }

    fn render(&self, answer: Answer<'_>) -> Result<PlanningPairSeatRecoveryDto, ApiError> {
        self.0.planning_pair_recovery_dto(
            answer.pair,
            answer.seat,
            answer.slot,
            answer.identity,
            answer.provider_session_id,
            answer.revision,
            (answer.receipt, applied_dto(answer.applied)),
        )
    }
}

const fn applied_dto(applied: Applied) -> AppliedDto {
    match applied {
        Applied::Created => AppliedDto::Created,
        Applied::Unchanged => AppliedDto::Unchanged,
    }
}
