//! The member/caller write sequences, with authentication before replay.

use kontor_core::consultation::ConsultationRunState;
use kontor_core::id::{
    AggregateRevision, BoundedText, CanonicalDocument, CommandReceiptId, ContentHash,
    IdempotencyKey, PlanningPairRunId, ProjectId,
};
use kontor_core::planning_pair::{
    ClarificationRequest, PlanningPairActor, PlanningPairDisposition, PlanningPairRound,
    PlanningPairSlot, RecordedClarification, RecordedContribution,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{StoredConsultationRun, StoredPlanningPairContribution};

use super::{CommandOwner, Viewer};
use crate::planning_pair::application::{Applied, PairState};
use crate::planning_pair::intent as fingerprint;

/// The owner's contribution-specific boundary and writer.
pub trait ContributionOwner: CommandOwner {
    /// The owner's rendered answer.
    type Output;

    /// Authorize the exact member, its generation and native observation.
    fn authenticate_member(
        &self,
        pair: &PairState,
        actor: Self::Actor,
    ) -> Result<PlanningPairSlot, Self::Error>;
    /// Authorize the frozen caller at its current hosted generation.
    fn authenticate_caller(
        &self,
        run: &StoredConsultationRun,
        actor: Self::Actor,
    ) -> Result<(), Self::Error>;
    /// The baseline refusal for an accepted answer with no stored question.
    fn missing_clarification(&self) -> Self::Error;
    /// Append the domain record and optional contribution under the run CAS.
    fn append_revision(
        &self,
        pair: &PairState,
        state: ConsultationRunState,
        contribution: Option<&StoredPlanningPairContribution>,
    ) -> Result<StoredConsultationRun, Self::Error>;
    /// Write the existing command receipt after the record append.
    #[allow(
        clippy::too_many_arguments,
        reason = "all receipt inputs remain explicit"
    )]
    fn record(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        kind: CommandKind,
        target: AggregateRef,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> Result<CommandReceiptId, Self::Error>;
    /// Render only the authenticated viewer's permitted projection.
    fn render(
        &self,
        pair: &PairState,
        viewer: Viewer,
        receipt: CommandReceiptId,
        applied: Applied,
    ) -> Result<Self::Output, Self::Error>;
}

/// Record one member's finding or clarification answer.
///
/// # Errors
/// The owner's authentication, replay, revision or persistence refusal, or
/// the unchanged domain transition refusal.
#[allow(
    clippy::too_many_arguments,
    reason = "the existing command inputs stay explicit"
)]
pub fn record<O: ContributionOwner>(
    owner: &O,
    key: &IdempotencyKey,
    project_id: ProjectId,
    run_id: PlanningPairRunId,
    member: O::Actor,
    round: PlanningPairRound,
    advice: &BoundedText,
    expected_revision: AggregateRevision,
) -> Result<O::Output, O::Error> {
    let run = owner.stored_run(project_id, run_id)?;
    let mut pair = owner.pair_state(&run)?;
    let slot = owner.authenticate_member(&pair, member)?;
    let kind = match round {
        PlanningPairRound::Findings => CommandKind::RecordPlanningPairFinding,
        PlanningPairRound::Clarification => CommandKind::RecordPlanningPairAnswer,
    };
    let presented = owner.presented(member);
    let intent = owner.canonical(
        &fingerprint::Contribution {
            project_id,
            run_id: run.id,
            round,
            slot,
            advice,
            member: presented,
        }
        .document(),
    )?;
    let target = AggregateRef::MiniProject {
        mini_project_id: run.mini_project_id,
    };
    if let Some(receipt) = owner.replayed(key, &intent, &target)? {
        return owner.render(&pair, Viewer::Member(slot), receipt.id, Applied::Unchanged);
    }
    owner.expect_revision(&run, expected_revision)?;
    let document_hash = record_transition(owner, &mut pair, slot, round, advice)?;
    let next = run
        .revision
        .next()
        .map_err(|error| owner.domain_error(&error))?;
    let now = owner.now();
    let stored = owner.append_revision(
        &pair,
        run.state,
        Some(&StoredPlanningPairContribution {
            run_id: run.id,
            round,
            slot,
            document_hash,
            seat_binding_id: presented.seat_binding_id,
            occupancy_generation: presented.occupancy_generation,
            record_revision: next,
            created_at: now,
        }),
    )?;
    let receipt = owner.record(key, project_id, kind, target, stored.revision, &intent)?;
    let pair = owner.pair_state(&stored)?;
    owner.render(&pair, Viewer::Member(slot), receipt, Applied::Created)
}

fn record_transition<O: ContributionOwner>(
    owner: &O,
    pair: &mut PairState,
    slot: PlanningPairSlot,
    round: PlanningPairRound,
    advice: &BoundedText,
) -> Result<ContentHash, O::Error> {
    let actor = PlanningPairActor::Member(slot);
    let document_hash = match round {
        PlanningPairRound::Findings => pair.pair.record_finding(actor, slot, advice.clone()),
        PlanningPairRound::Clarification => pair.pair.record_answer(actor, slot, advice.clone()),
    }
    .map_err(|error| owner.domain_error(&error))?;
    let contribution = RecordedContribution {
        slot,
        advice: advice.clone(),
        document_hash: document_hash.clone(),
    };
    match round {
        PlanningPairRound::Findings => in_slot_order(&mut pair.record.findings, contribution),
        PlanningPairRound::Clarification => {
            let clarification = pair
                .record
                .clarification
                .as_mut()
                .ok_or_else(|| owner.missing_clarification())?;
            in_slot_order(&mut clarification.answers, contribution);
        }
    }
    Ok(document_hash)
}

/// Request the caller's one clarification round.
///
/// # Errors
/// Authentication/replay/CAS failures or the unchanged domain refusal.
#[allow(
    clippy::too_many_arguments,
    reason = "the existing command inputs stay explicit"
)]
pub fn clarify<O: ContributionOwner>(
    owner: &O,
    key: &IdempotencyKey,
    project_id: ProjectId,
    run_id: PlanningPairRunId,
    caller: O::Actor,
    question: &BoundedText,
    addressed: &[PlanningPairSlot],
    expected_revision: AggregateRevision,
) -> Result<O::Output, O::Error> {
    let run = owner.stored_run(project_id, run_id)?;
    let mut pair = owner.pair_state(&run)?;
    owner.authenticate_caller(&run, caller)?;
    let intent = owner.canonical(
        &fingerprint::Clarification {
            project_id,
            run_id: run.id,
            question,
            addressed,
            caller: owner.presented(caller),
        }
        .document(),
    )?;
    let target = AggregateRef::MiniProject {
        mini_project_id: run.mini_project_id,
    };
    if let Some(receipt) = owner.replayed(key, &intent, &target)? {
        return owner.render(&pair, Viewer::Caller, receipt.id, Applied::Unchanged);
    }
    owner.expect_revision(&run, expected_revision)?;
    let clarification = ClarificationRequest {
        question: question.clone(),
        addressed: addressed.to_vec(),
    };
    pair.pair
        .request_clarification(PlanningPairActor::Caller, clarification.clone())
        .map_err(|error| owner.domain_error(&error))?;
    pair.record.clarification = Some(RecordedClarification {
        request: clarification,
        answers: Vec::new(),
    });
    finish(
        owner,
        key,
        project_id,
        &pair,
        run.state,
        CommandKind::RequestPlanningPairClarification,
        target,
        &intent,
    )
}

/// Record the caller's disposition, retaining the domain's dissent rules.
///
/// # Errors
/// Authentication/replay/CAS failures or the unchanged domain refusal.
#[allow(
    clippy::too_many_arguments,
    reason = "the existing command inputs stay explicit"
)]
pub fn decide<O: ContributionOwner>(
    owner: &O,
    key: &IdempotencyKey,
    project_id: ProjectId,
    run_id: PlanningPairRunId,
    caller: O::Actor,
    disposition: &PlanningPairDisposition,
    expected_revision: AggregateRevision,
) -> Result<O::Output, O::Error> {
    let run = owner.stored_run(project_id, run_id)?;
    let mut pair = owner.pair_state(&run)?;
    owner.authenticate_caller(&run, caller)?;
    let intent = owner.canonical(
        &fingerprint::Disposition {
            project_id,
            run_id: run.id,
            disposition,
            caller: owner.presented(caller),
        }
        .document(),
    )?;
    let target = AggregateRef::MiniProject {
        mini_project_id: run.mini_project_id,
    };
    if let Some(receipt) = owner.replayed(key, &intent, &target)? {
        return owner.render(&pair, Viewer::Caller, receipt.id, Applied::Unchanged);
    }
    owner.expect_revision(&run, expected_revision)?;
    pair.pair
        .record_disposition(PlanningPairActor::Caller, disposition.clone())
        .map_err(|error| owner.domain_error(&error))?;
    pair.record.disposition = Some(disposition.clone());
    finish(
        owner,
        key,
        project_id,
        &pair,
        ConsultationRunState::Disposed,
        CommandKind::RecordPlanningPairDisposition,
        target,
        &intent,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "all record and receipt inputs stay explicit"
)]
fn finish<O: ContributionOwner>(
    owner: &O,
    key: &IdempotencyKey,
    project_id: ProjectId,
    pair: &PairState,
    state: ConsultationRunState,
    kind: CommandKind,
    target: AggregateRef,
    intent: &CanonicalDocument,
) -> Result<O::Output, O::Error> {
    let stored = owner.append_revision(pair, state, None)?;
    let receipt = owner.record(key, project_id, kind, target, stored.revision, intent)?;
    let pair = owner.pair_state(&stored)?;
    owner.render(&pair, Viewer::Caller, receipt, Applied::Created)
}

/// Findings and answers stay in slot order, whatever order they arrive in.
fn in_slot_order(list: &mut Vec<RecordedContribution>, contribution: RecordedContribution) {
    let at = list
        .iter()
        .position(|kept| kept.slot > contribution.slot)
        .unwrap_or(list.len());
    list.insert(at, contribution);
}
