//! Deterministic guardrail evaluation.
//!
//! One function per architecture rule, each a pure function of
//! [`EvaluationRequest`]. Same inputs, same verdict, same reason, same evidence
//! pointers — on this machine, on another one, and on a replay a year later.
//! Nothing here reads a clock, a filesystem, an environment variable or a
//! database: the instant an expiry is judged against arrives as
//! [`EvaluationRequest::evaluated_at`], and everything else arrives with it.
//!
//! ## What these rules are not allowed to know
//!
//! No rule branches on a profile id, a phase name, a gate name, a role name or a
//! persona name. Every one of those is deployment data, and a rule that
//! recognized one would work for the profiles somebody happened to write it
//! against and quietly stop working for the next. What the rules read instead is
//! the *shape* of the pinned snapshot: which artifacts a phase declares, which
//! roles a gate authorizes, whether a gate allows a waiver at all. An arbitrary
//! custom profile with names nobody has seen before is therefore evaluated the
//! same way as a bundled one, which the suite asserts directly.
//!
//! ## Authority stays where it already lives
//!
//! These rules do not replace the profile validation and the gate-authority
//! check in `kontor-store`'s `append_gate_evaluation`; that remains the place a
//! gate verdict is admitted or refused. What
//! [`evaluate`] adds is the guardrail layer *in front* of it: a refusal here
//! means the action is never attempted, so nothing has to be undone.

use std::collections::BTreeSet;

use kontor_core::DomainResult;
use kontor_core::id::{
    ArtifactKey, CanonicalDocument, ExternalId, ExternalName, GateKey, GuardrailEvaluationId,
    RoleKey,
};
use kontor_core::repository::GateEvaluation;
use kontor_core::spec::GateSpec;
use kontor_core::state::GateVerdict;

use crate::model::{
    ActionEffect, ActionIntent, Decision, EvaluationRequest, EvaluationSubject, EvidenceRef,
    GuardrailEvaluation, GuardrailRuleKey, ModuleClaim, PolicyVerdict, ReasonCode, SubjectKind,
};

/// The number of rejections by one reviewer on one gate that parks the work.
///
/// Two, and it is a constant rather than a configurable: the whole point of the
/// rule is that a reviewer who has already said no once is not asked to keep
/// saying it while the same work comes back unchanged.
pub const REJECTIONS_BEFORE_PARK: u32 = 2;

/// Evaluate one rule and record the result.
///
/// The returned value is new and immutable every time. `id` is supplied by the
/// caller rather than minted here so that the function stays pure: two calls
/// with the same arguments produce byte-identical records.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the request cannot be canonicalized
/// — which is also where an oversized, non-canonical or secret-bearing input
/// document is refused, before it can be stored as evidence.
pub fn evaluate(
    request: &EvaluationRequest,
    id: GuardrailEvaluationId,
) -> DomainResult<GuardrailEvaluation> {
    let decision = decide(request)?;
    let inputs = CanonicalDocument::from_serializable(request)?;
    let inputs_hash = inputs.hash().clone();
    Ok(GuardrailEvaluation {
        id,
        rule_key: request.rule.key,
        rule_version: request.rule.version,
        subject: subject_of(request)?,
        inputs,
        inputs_hash,
        verdict: decision.verdict,
        reason_code: decision.reason_code,
        evidence_refs: decision.evidence_refs,
        recorded_at: request.evaluated_at,
    })
}

/// Decide one rule without producing a record.
///
/// This is the part the determinism suite compares: everything an evaluation
/// concludes, with none of the identity a caller supplies.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the pinned snapshot does not verify
/// against its own digest. A rule reads authority out of that snapshot, so a
/// snapshot that has been altered is refused before any rule reads it rather
/// than being evaluated as if it were intact.
pub fn decide(request: &EvaluationRequest) -> DomainResult<Decision> {
    request.workflow.verify()?;
    Ok(match request.rule.key {
        GuardrailRuleKey::WorktreeSticky => worktree_sticky(request),
        GuardrailRuleKey::ModuleCollision => module_collision(request),
        GuardrailRuleKey::SecondRejectionParks => second_rejection_parks(request),
        GuardrailRuleKey::DegradedVerdictDenied => degraded_verdict_denied(request),
        GuardrailRuleKey::DestructiveRequiresApproval => destructive_requires_approval(request),
        GuardrailRuleKey::AccountPinRequired => account_pin_required(request),
        GuardrailRuleKey::TerminalEvidenceRequired => terminal_evidence_required(request),
    })
}

/// What the evaluation is about, derived from the rule rather than supplied.
///
/// A caller-chosen subject would let two evaluations of the same rule be filed
/// against different things, which is exactly the drift an audit cannot see.
fn subject_of(request: &EvaluationRequest) -> DomainResult<EvaluationSubject> {
    let run_subject = || match request.run.agent_run_id {
        Some(run) => (SubjectKind::AgentRun, run.to_string()),
        None => match request.run.team_run_id {
            Some(team) => (SubjectKind::TeamRun, team.to_string()),
            None => (
                SubjectKind::TaskWorkflow,
                request.run.workflow_id.to_string(),
            ),
        },
    };
    let gate_subject = || match &request.gate {
        Some(gate) => (SubjectKind::Gate, gate.as_str().to_owned()),
        None => run_subject(),
    };
    let (kind, id) = match request.rule.key {
        GuardrailRuleKey::WorktreeSticky
        | GuardrailRuleKey::AccountPinRequired
        | GuardrailRuleKey::TerminalEvidenceRequired => run_subject(),
        GuardrailRuleKey::ModuleCollision => (SubjectKind::Task, request.run.task_id.to_string()),
        GuardrailRuleKey::SecondRejectionParks | GuardrailRuleKey::DegradedVerdictDenied => {
            gate_subject()
        }
        GuardrailRuleKey::DestructiveRequiresApproval => (
            SubjectKind::Action,
            request.run.requested_action.digest.as_str().to_owned(),
        ),
    };
    Ok(EvaluationSubject {
        kind,
        id: ExternalId::parse(&id)?,
    })
}

// ---------------------------------------------------------------------------
// 1. worktree_sticky
// ---------------------------------------------------------------------------

/// A run acts in the worktree it was first recorded in, or it stops.
///
/// The refusal is [`PolicyVerdict::Park`] rather than [`PolicyVerdict::Block`]
/// on purpose. A run that has lost track of which tree it is editing is not
/// having one operation refused — it is in a state where *no* operation is
/// trustworthy, including the tidy-up an automatic repair would want to do. So
/// it parks and recovery inspects it, rather than the guardrail deciding on its
/// own which of two trees was meant.
fn worktree_sticky(request: &EvaluationRequest) -> Decision {
    let recorded = request.run.recorded_worktree.as_ref();
    let claimed = request.workspace.claimed_worktree.as_ref();
    let candidates = &request.workspace.candidate_worktrees;

    match (recorded, claimed) {
        (Some(pinned), Some(claim)) if pinned == claim => Decision::with_evidence(
            PolicyVerdict::Pass,
            ReasonCode::WorktreeMatchesPin,
            vec![EvidenceRef::Worktree {
                worktree: claim.clone(),
            }],
        ),
        (Some(_), Some(claim)) => Decision::with_evidence(
            PolicyVerdict::Park,
            ReasonCode::WorktreeMoved,
            vec![EvidenceRef::Worktree {
                worktree: claim.clone(),
            }],
        ),
        (Some(_) | None, None) => {
            Decision::bare(PolicyVerdict::Park, ReasonCode::WorktreeUnclaimed)
        }
        (None, Some(claim)) => {
            // Nothing is pinned yet, so this request would be the pin. It is
            // only allowed to become one when the workspace layer offered
            // exactly this tree and no other: "the first plausible candidate"
            // is how a run silently adopts the wrong tree for its whole life.
            if !candidates.contains(claim) {
                Decision::with_evidence(
                    PolicyVerdict::Park,
                    ReasonCode::WorktreeMoved,
                    vec![EvidenceRef::Worktree {
                        worktree: claim.clone(),
                    }],
                )
            } else if candidates.len() != 1 {
                Decision::bare(PolicyVerdict::Park, ReasonCode::WorktreeAmbiguous)
            } else {
                Decision::with_evidence(
                    PolicyVerdict::Pass,
                    ReasonCode::WorktreeFirstClaim,
                    vec![EvidenceRef::Worktree {
                        worktree: claim.clone(),
                    }],
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. module_collision
// ---------------------------------------------------------------------------

/// Whether holding `mine` keeps this work apart from every contender.
///
/// Isolation is only isolation when *both* sides have a tree and the trees
/// differ. A contender with no recorded worktree is not isolated from anything,
/// and neither are we — so "some of the contenders are in other trees" is not
/// isolation either, which is why this is `all` and not `any`.
///
/// It is public because the same question is asked twice about the same fact, at
/// two moments: a guardrail asks it about an action inside a run
/// ([`module_collision`]), and the scheduler asks it about a task it is about to
/// admit, against the module leases held across the whole Realm. Those are
/// different inputs, and it is the same rule — so it is one function rather than
/// two that agree until one of them is edited.
#[must_use]
pub fn module_isolated_by_worktree<'a>(
    mine: Option<&ExternalName>,
    contenders: impl IntoIterator<Item = &'a ModuleClaim>,
) -> bool {
    let Some(mine) = mine else {
        return false;
    };
    contenders
        .into_iter()
        .all(|claim| claim.worktree.as_ref().is_some_and(|theirs| theirs != mine))
}

/// Two tasks do not hold the same module unless separate worktrees keep them
/// apart.
///
/// This one blocks rather than parks: the work is fine, it simply may not start
/// yet. Parking it would turn "wait for the other ticket" into an incident.
fn module_collision(request: &EvaluationRequest) -> Decision {
    let Some(module) = request.run.module.as_ref() else {
        return Decision::bare(PolicyVerdict::Pass, ReasonCode::ModuleFree);
    };
    let contenders: Vec<_> = request
        .workspace
        .module_claims
        .iter()
        .filter(|claim| {
            claim.in_flight
                && claim.module.contends_with(module)
                && claim.task_id != request.run.task_id
        })
        .collect();
    if contenders.is_empty() {
        return Decision::bare(PolicyVerdict::Pass, ReasonCode::ModuleFree);
    }

    let evidence: Vec<EvidenceRef> = contenders
        .iter()
        .map(|claim| EvidenceRef::ModuleClaim {
            module: claim.module.clone(),
            task_id: claim.task_id,
        })
        .collect();

    let mine = request
        .run
        .recorded_worktree
        .as_ref()
        .or(request.workspace.claimed_worktree.as_ref());
    if module_isolated_by_worktree(mine, contenders.iter().copied()) {
        Decision::with_evidence(
            PolicyVerdict::Pass,
            ReasonCode::ModuleIsolatedByWorktree,
            evidence,
        )
    } else {
        Decision::with_evidence(PolicyVerdict::Block, ReasonCode::ModuleInFlight, evidence)
    }
}

// ---------------------------------------------------------------------------
// 3. second_rejection_parks
// ---------------------------------------------------------------------------

/// How many times this reviewer has rejected this gate since they last passed
/// it.
///
/// Derived from the append-only history every time it is needed. There is no
/// counter row to drift, to miss an increment, or to be reset by something that
/// should not have reset it.
///
/// The reset is deliberately narrow. Only a `passed` verdict by the *same*
/// principal on the *same* gate clears the stream. `started`, `waived` and
/// `parked` are not passes; another reviewer's pass is not this reviewer's; and
/// a new agent run is not an event in this stream at all, which is precisely why
/// the key is the principal and not the run.
#[must_use]
pub fn rejections_since_pass(
    history: &[GateEvaluation],
    gate: &GateKey,
    principal: &ExternalId,
) -> u32 {
    let mut relevant: Vec<&GateEvaluation> = history
        .iter()
        .filter(|evaluation| {
            &evaluation.gate == gate && evaluation.reviewer_principal.as_ref() == Some(principal)
        })
        .collect();
    relevant.sort_by_key(|evaluation| evaluation.sequence);
    relevant.iter().fold(0, |count, evaluation| {
        match evaluation.verdict {
            GateVerdict::Rejected => count + 1,
            GateVerdict::Passed => 0,
            // Not decisions about whether the work is acceptable, so they leave
            // the stream exactly where it was.
            GateVerdict::Started | GateVerdict::Waived | GateVerdict::Parked => count,
        }
    })
}

fn second_rejection_parks(request: &EvaluationRequest) -> Decision {
    let Some(gate) = request.gate.as_ref() else {
        return Decision::bare(PolicyVerdict::Pass, ReasonCode::NotApplicable);
    };
    let prior = rejections_since_pass(
        &request.prior_gate_evaluations,
        gate,
        &request.actor.principal,
    );
    let pending =
        u32::from(request.run.requested_action.intent == ActionIntent::RecordGateRejection);
    let total = prior + pending;

    let evidence: Vec<EvidenceRef> = request
        .prior_gate_evaluations
        .iter()
        .filter(|evaluation| {
            &evaluation.gate == gate
                && evaluation.verdict == GateVerdict::Rejected
                && evaluation.reviewer_principal.as_ref() == Some(&request.actor.principal)
        })
        .map(|evaluation| EvidenceRef::GateVerdict {
            gate: evaluation.gate.clone(),
            sequence: evaluation.sequence,
        })
        .collect();

    if total >= REJECTIONS_BEFORE_PARK {
        Decision::with_evidence(
            PolicyVerdict::Park,
            ReasonCode::SecondRejectionParks,
            evidence,
        )
    } else if total == 1 {
        Decision::with_evidence(PolicyVerdict::Warn, ReasonCode::FirstRejection, evidence)
    } else {
        Decision::bare(PolicyVerdict::Pass, ReasonCode::NoRejectionRecorded)
    }
}

// ---------------------------------------------------------------------------
// 4. degraded_verdict_denied
// ---------------------------------------------------------------------------

/// Whether the pinned gate authorizes this role for any verdict at all.
fn gate_authorizes(gate: &GateSpec, role: &RoleKey) -> bool {
    gate.evaluator_roles.contains(role) || (gate.waiver_allowed && gate.waiver_roles.contains(role))
}

/// A verdict needs authority the actor actually has.
///
/// Three refusals, in the order they matter:
///
/// 1. a simulated persona never records a verdict — it produces evidence and
///    recommendations, and the gate it is under test for is exactly the one it
///    must not decide;
/// 2. the pinned profile has to authorize the role, read out of the snapshot
///    rather than out of a list of role names this crate knows;
/// 3. the actor's evidence rung has to reach
///    [`crate::model::VerdictRung::VERDICT_THRESHOLD`].
///
/// The rung rule covers "a degraded actor cannot write QA, audit or final
/// verdicts" without knowing which gates those are: it refuses *every* decisive
/// verdict from degraded evidence, which is the same rule stated in a way an
/// unfamiliar profile cannot escape. Starting a gate is not a decision and is
/// deliberately not covered.
fn degraded_verdict_denied(request: &EvaluationRequest) -> Decision {
    if !request.run.requested_action.intent.writes_gate_verdict() {
        return Decision::bare(PolicyVerdict::Pass, ReasonCode::NotApplicable);
    }
    if let Some(persona) = request.actor.persona.as_ref() {
        let reason = if Some(&persona.gate_under_test) == request.gate.as_ref() {
            ReasonCode::PersonaSelfApproval
        } else {
            ReasonCode::PersonaCannotEvaluate
        };
        return Decision::bare(PolicyVerdict::Block, reason);
    }
    if let Some(gate) = request.gate.as_ref() {
        let authorized = request
            .workflow
            .definition
            .gate(gate)
            .is_some_and(|spec| gate_authorizes(spec, &request.actor.role));
        if !authorized {
            return Decision::bare(PolicyVerdict::Block, ReasonCode::RoleNotAuthorized);
        }
    }
    if request.actor.verdict_rung.may_write_verdict() {
        Decision::bare(PolicyVerdict::Pass, ReasonCode::VerdictAuthorityHeld)
    } else {
        Decision::bare(PolicyVerdict::Block, ReasonCode::VerdictRungDegraded)
    }
}

// ---------------------------------------------------------------------------
// 5. destructive_requires_approval
// ---------------------------------------------------------------------------

/// A destructive action happens only with an approval bound to that exact
/// action.
///
/// "Bound" means every one of: the same canonical action digest, the same
/// domain, intent and effect, the same project, the same task when the approval
/// is task-scoped, unspent, and not yet expired. A generic reusable approval
/// cannot be expressed here, and recovery advice can never supply one — that
/// last refusal is checked first, so an advisor's recommendation is rejected as
/// advice rather than being examined as if it might be authority.
///
/// A dry run is admitted without an approval because it has no effects. That is
/// the "prefer dry-run where supported" half of the rule: the effect-free
/// request is the one that gets through unapproved.
fn destructive_requires_approval(request: &EvaluationRequest) -> Decision {
    let action = &request.run.requested_action;
    if action.effect != ActionEffect::Destroy {
        return Decision::bare(PolicyVerdict::Pass, ReasonCode::ActionNonDestructive);
    }
    if action.dry_run {
        return Decision::bare(PolicyVerdict::Pass, ReasonCode::ActionDryRun);
    }
    let Some(approval) = request.approval.as_ref() else {
        return Decision::bare(PolicyVerdict::Block, ReasonCode::ApprovalMissing);
    };
    if let Some(reason) = approval.refusal_for(
        action,
        request.run.project_id,
        request.run.task_id,
        request.evaluated_at,
    ) {
        return Decision::bare(PolicyVerdict::Block, reason);
    }
    Decision::with_evidence(
        PolicyVerdict::Pass,
        ReasonCode::ApprovalBound,
        vec![EvidenceRef::Approval {
            id: approval.id,
            action_digest: approval.action_digest.clone(),
        }],
    )
}

// ---------------------------------------------------------------------------
// 6. account_pin_required
// ---------------------------------------------------------------------------

/// A run acts as the account it was pinned to, or not at all.
///
/// An unpinned run is refused rather than allowed to act as whoever launched it:
/// the pin is what makes a later audit able to say which account did the work,
/// and inferring it after the fact from the actor would make the record a
/// tautology.
fn account_pin_required(request: &EvaluationRequest) -> Decision {
    match request.run.pinned_account {
        None => Decision::bare(PolicyVerdict::Block, ReasonCode::AccountPinMissing),
        Some(pinned) if pinned != request.actor.account => {
            Decision::bare(PolicyVerdict::Block, ReasonCode::AccountPinMismatch)
        }
        Some(_) => Decision::bare(PolicyVerdict::Pass, ReasonCode::AccountPinMatches),
    }
}

// ---------------------------------------------------------------------------
// 7. terminal_evidence_required
// ---------------------------------------------------------------------------

/// Finishing something requires the evidence that it finished.
///
/// Two shapes, both read out of the pinned snapshot rather than assumed:
/// completing a phase requires every artifact that phase declares, and closing a
/// run requires a terminal observation *of that run*. An observation belonging
/// to a different run is refused with its own reason code, because "somebody
/// cited the wrong run" and "nobody cited anything" are different failures and
/// an audit needs to tell them apart.
fn terminal_evidence_required(request: &EvaluationRequest) -> Decision {
    match request.run.requested_action.intent {
        ActionIntent::CompletePhase => {
            let Some(phase) = request
                .workflow
                .definition
                .phases
                .iter()
                .find(|phase| phase.id == request.current_phase)
            else {
                return Decision::bare(
                    PolicyVerdict::Block,
                    ReasonCode::ArtifactEvidenceIncomplete,
                );
            };
            let produced: BTreeSet<&ArtifactKey> =
                request.artifacts.iter().map(|record| &record.key).collect();
            let missing = phase
                .required_artifacts
                .iter()
                .any(|required| !produced.contains(required));
            if missing {
                Decision::bare(PolicyVerdict::Block, ReasonCode::ArtifactEvidenceIncomplete)
            } else {
                let evidence = request
                    .artifacts
                    .iter()
                    .filter(|record| phase.required_artifacts.contains(&record.key))
                    .map(|record| EvidenceRef::Artifact {
                        key: record.key.clone(),
                        id: record.id,
                    })
                    .collect();
                Decision::with_evidence(
                    PolicyVerdict::Pass,
                    ReasonCode::ArtifactEvidenceComplete,
                    evidence,
                )
            }
        }
        ActionIntent::CloseRun => {
            let Some(observation) = request.terminal_observation.as_ref() else {
                return Decision::bare(PolicyVerdict::Block, ReasonCode::TerminalEvidenceMissing);
            };
            if Some(observation.agent_run_id) != request.run.agent_run_id {
                return Decision::bare(PolicyVerdict::Block, ReasonCode::TerminalEvidenceForeign);
            }
            Decision::with_evidence(
                PolicyVerdict::Pass,
                ReasonCode::TerminalEvidencePresent,
                vec![EvidenceRef::RuntimeObservation {
                    agent_run_id: observation.agent_run_id,
                    cursor: observation.cursor,
                }],
            )
        }
        ActionIntent::Inspect
        | ActionIntent::ProduceArtifact
        | ActionIntent::RecordGateVerdict
        | ActionIntent::RecordGateRejection
        | ActionIntent::Mutate => Decision::bare(PolicyVerdict::Pass, ReasonCode::NotApplicable),
    }
}
