//! From a canonical event to exactly one deterministic decision.
//!
//! Evaluation is a pure function of the event and the trigger catalog it was
//! shown. Run it twice on the same pair and you get the same verdict, the same
//! dedup key and the same idempotency key — which is what lets a decision be
//! *resumed* after a crash rather than re-invented, and what lets a stored
//! receipt be re-checked years later against the revision it pinned.
//!
//! Three outcomes, and they are different states rather than three shades of
//! one:
//!
//! * [`Intake::Proposed`] — a trigger matched. The receipt is `proposed` and
//!   carries no work: proposing is not arming, and the decision that arms is
//!   receipt-backed and separate.
//! * [`Intake::Ignored`] — a trigger *addressed* this event (same connection,
//!   same event schema revision) and its filter said no. That is a real verdict
//!   about a real trigger revision, so it is recorded as one.
//! * [`Intake::Unaddressed`] — no trigger in the catalog even addresses this
//!   event. There is no revision to pin, so there is no receipt to write: the
//!   durable evidence is the stored envelope itself. Re-evaluating it is
//!   idempotent, because evaluation reads nothing but its arguments.

use kontor_core::DomainResult;
use kontor_core::calendar::ExecutionAuthorization;
use kontor_core::id::{
    ArtifactKey, CalendarProfileId, ContentHash, IdempotencyKey, IntakeReceiptId, SpecVersion,
    TeamTemplateId, Timestamp, WorkProfileKey,
};
use kontor_core::repository::IntakeWorkPlan;
use kontor_core::spec::{
    AutoArmRefusal, AutoArmRequest, BudgetBounds, CanonicalSourceEvent, ExecutionCapability,
    IntakeReceipt, IntakeResult, TriggerSpec,
};

use crate::matching::match_triggers;

/// The pins a matched trigger hands to whoever builds the work.
///
/// Every one of them is a *revision*, resolved at decision time and copied here
/// so the work that is eventually created is the work this decision described —
/// even if the trigger is edited a minute later. Nothing in here is looked up
/// again downstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkPins {
    /// The work profile the created graph runs under.
    pub work_profile: WorkProfileKey,
    /// Its pinned revision.
    pub work_profile_version: SpecVersion,
    /// The team template the work is staffed from.
    pub team_template: TeamTemplateId,
    /// Its pinned revision.
    pub team_template_version: SpecVersion,
    /// The context-pack template handed to the work.
    pub context_template: ArtifactKey,
    /// Its pinned revision.
    pub context_version: SpecVersion,
    /// The calendar profile the work is admitted against, if the trigger names
    /// one.
    pub calendar_profile: Option<CalendarProfileId>,
    /// That profile's pinned revision.
    pub calendar_version: Option<SpecVersion>,
    /// The scheduling priority the trigger declares.
    pub priority: u32,
    /// The budget bounds the created work is held to.
    pub budget: BudgetBounds,
}

/// One trigger revision that matched, with everything the decision pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedTrigger<'a> {
    /// The exact revision that decided. Borrowed, so it cannot drift from the
    /// document the caller showed the matcher.
    pub trigger: &'a TriggerSpec,
    /// The deterministic dedup key of this event under that revision.
    pub dedup_key: ContentHash,
    /// The rendered pins.
    pub pins: WorkPins,
}

/// What evaluating one event against one catalog produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intake<'a> {
    /// A trigger matched; this is the proposal and its pins.
    Proposed {
        /// The immutable `proposed` decision.
        receipt: Box<IntakeReceipt>,
        /// The revision that decided, and what it pinned.
        matched: Box<MatchedTrigger<'a>>,
    },
    /// A trigger addressed the event and declined it.
    Ignored {
        /// The immutable `ignored` decision.
        receipt: Box<IntakeReceipt>,
    },
    /// No trigger addresses this event at all.
    Unaddressed,
}

/// The deterministic idempotency key of one decision.
///
/// Derived from the pinned revision and the dedup key, and from nothing else:
/// two evaluations of the same event under the same revision must collide, and
/// two different events under the same revision must not.
///
/// # Errors
/// Returns [`DomainError`] when the derived text is not a legal key — which a
/// trigger key that itself reads as sensitive material would produce, and which
/// is refused here rather than weakened.
pub fn idempotency_key(
    trigger: &TriggerSpec,
    dedup_key: &ContentHash,
) -> DomainResult<IdempotencyKey> {
    IdempotencyKey::parse(&format!(
        "intake:{}:{}:{}",
        trigger.id.as_str(),
        trigger.version.get(),
        dedup_key.as_str()
    ))
}

fn pins_of(trigger: &TriggerSpec) -> WorkPins {
    WorkPins {
        work_profile: trigger.work_profile.clone(),
        work_profile_version: trigger.work_profile_version,
        team_template: trigger.team_template.template_id,
        team_template_version: trigger.team_template.version,
        context_template: trigger.context_template.template.clone(),
        context_version: trigger.context_template.version,
        calendar_profile: trigger
            .calendar_policy
            .as_ref()
            .map(|policy| policy.profile_id),
        calendar_version: trigger
            .calendar_policy
            .as_ref()
            .map(|policy| policy.version),
        priority: trigger.limits.priority,
        budget: trigger.limits.budget,
    }
}

/// Decide one already-canonical event against one trigger catalog.
///
/// The event is expected to be durable already: intake commits the envelope
/// first and decides second, so that a crash between the two loses no evidence.
/// This function is what the resumed half runs.
///
/// # Errors
/// Returns [`DomainError`] when the envelope cannot be read, when a matched
/// trigger's dedup expression does not resolve after all, and when the derived
/// idempotency key is not legal.
pub fn evaluate<'a>(
    event: &CanonicalSourceEvent,
    triggers: &'a [TriggerSpec],
    id: IntakeReceiptId,
    decided_at: Timestamp,
) -> DomainResult<Intake<'a>> {
    let matched = match_triggers(event, triggers)?;
    if let Some(trigger) = matched.first() {
        let dedup_key = trigger.dedup.evaluate(&event.envelope)?;
        let receipt = IntakeReceipt {
            id,
            source_event_id: event.id,
            source_event_hash: event.envelope.hash().clone(),
            trigger: trigger.id.clone(),
            trigger_version: trigger.version,
            result: IntakeResult::Proposed,
            // Proposing is not approving, and a proposal that already carried a
            // graph would be work nobody decided to create.
            approval: None,
            proposed: None,
            idempotency_key: idempotency_key(trigger, &dedup_key)?,
            dedup_key: dedup_key.clone(),
            duplicate_of: None,
            predecessor_receipt_id: None,
            decided_at,
        };
        receipt.validate()?;
        return Ok(Intake::Proposed {
            receipt: Box::new(receipt),
            matched: Box::new(MatchedTrigger {
                trigger,
                dedup_key,
                pins: pins_of(trigger),
            }),
        });
    }

    // Nothing matched. A trigger that *addresses* this event — same connection,
    // same pinned event schema — has still made a decision about it, and that
    // decision is auditable evidence. Which one is recorded is deterministic:
    // the first in the same total order the matcher uses.
    let Some(addressed) = addressed_trigger(event, triggers) else {
        return Ok(Intake::Unaddressed);
    };
    // An ignored decision is keyed on the envelope digest rather than on the
    // trigger's dedup expression: the expression is allowed not to resolve for
    // an event this trigger declined, and a key that sometimes exists is not a
    // key.
    let dedup_key = event.envelope.hash().clone();
    let receipt = IntakeReceipt {
        id,
        source_event_id: event.id,
        source_event_hash: event.envelope.hash().clone(),
        trigger: addressed.id.clone(),
        trigger_version: addressed.version,
        result: IntakeResult::Ignored,
        approval: None,
        proposed: None,
        idempotency_key: idempotency_key(addressed, &dedup_key)?,
        dedup_key,
        duplicate_of: None,
        predecessor_receipt_id: None,
        decided_at,
    };
    receipt.validate()?;
    Ok(Intake::Ignored {
        receipt: Box::new(receipt),
    })
}

/// The deterministic first trigger revision that addresses this event.
fn addressed_trigger<'a>(
    event: &CanonicalSourceEvent,
    triggers: &'a [TriggerSpec],
) -> Option<&'a TriggerSpec> {
    let mut addressed: Vec<&TriggerSpec> = triggers
        .iter()
        .filter(|trigger| {
            trigger.source_kind == event.identity.source_kind
                && trigger.source_connection == event.identity.source_connection
        })
        .collect();
    addressed.sort_by(|left, right| {
        left.id
            .as_str()
            .cmp(right.id.as_str())
            .then(right.version.get().cmp(&left.version.get()))
    });
    addressed.first().copied()
}

/// Whether a matched trigger's own policy arms the work a caller proposes.
///
/// The rule itself is [`TriggerSpec::authorize_auto_arm`] in `kontor-core`, and
/// it is called from here rather than restated — the store calls the very same
/// function inside the transaction that creates the work, so a caller that
/// skips this layer is refused by identical bounds rather than by none.
///
/// # Errors
/// Returns the single [`AutoArmRefusal`] that applies.
pub fn authorize_auto_arm(
    trigger: &TriggerSpec,
    work: &IntakeWorkPlan,
    caller: kontor_core::id::AccountProfileId,
    authorization: &ExecutionAuthorization,
    at: Timestamp,
) -> Result<ExecutionCapability, AutoArmRefusal> {
    let task_ids: Vec<kontor_core::id::TaskId> = work.tasks.iter().map(|task| task.id).collect();
    trigger.authorize_auto_arm(&AutoArmRequest {
        caller,
        authorization,
        at,
        mini_project_id: work.mini_project.as_ref().map(|goal| goal.id),
        task_ids: &task_ids,
    })
}
