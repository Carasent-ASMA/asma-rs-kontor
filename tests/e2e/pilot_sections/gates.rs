//! Section 3 — gate authority, the rejection stream and what survives a restart.
//!
//! Every case here is about a refusal that must not be talked around. The
//! guardrail decisions come from `kontor_policy::decide`, which reads no clock,
//! no database and no environment; the consequences are then checked against a
//! real file-backed `kontor_store::SqliteStore`, because "the rule said no" and
//! "nothing was written" are two different claims and only the second one is
//! what an operator actually gets.
//!
//! The guarded shape used throughout is the control plane's own: the store call
//! sits *inside* the `if decision.admits()`, so a refusal is an absence rather
//! than a rollback. A case that recorded the write and then undid it would be
//! proving a weaker thing.
//!
//! The pinned profile is the bundled `ux-ui-layout` one — the same profile the
//! pilot's `pilot-ux` task resolves in section 1. Its gate and phase names are
//! deployment data, and this section names them the way a deployment does: as
//! fixture input to generic rules, never as something the rules recognize.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CanonicalDocument,
    CommandReceiptId, ContentHash, CredentialAlias, EventCursor, ExternalId, ExternalName, GateKey,
    GuardrailEvaluationId, IdempotencyKey, PhaseKey, ProjectId, RoleKey, RuntimeKindKey,
    SCHEMA_VERSION, SpecVersion, TaskId, TaskWorkflowId, TeamRunId,
};
use kontor_core::repository::{
    AgentRun, CredentialReference, CredentialReferenceKind, GateEvaluation, NewAccountProfile,
    NewAgentRun, NewGateEvaluation, NewProject, NewTask, NewTaskWorkflow, NewTeamRun, PhaseAdvance,
    ProjectRepository, RunRepository, SpecRepository, WorkflowRepository,
};
use kontor_core::spec::{PersonaScenarioSpec, ResolvedWorkProfileSnapshot, TeamRunSnapshot};
use kontor_core::state::{
    DerivedRunState, DesiredRunState, GateState, GateVerdict, ObservedRunState, RunLifecycle,
    RunProjection, TaskState, TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
};
use kontor_policy::model::{
    ActionDomain, ActionEffect, ActionIntent, ActorContext, Decision, EvaluationRequest,
    GuardrailRule, GuardrailRuleKey, PersonaActor, PolicyVerdict, ReasonCode, RequestedAction,
    RunContext, VerdictRung, WorkspaceEvidence,
};
use kontor_policy::{REJECTIONS_BEFORE_PARK, decide, evaluate, rejections_since_pass};
use kontor_profiles::pack::{
    GateWaiver, PackCategoryKey, ResolvedProfileBundle, TaskTeamEvidence, certify_task_closure,
    resolve_profile,
};
use kontor_profiles::seeds::bundled_pack;
use kontor_store::{EvaluationBinding, GateRejection, NewArtifactEvidence, ParkPlan, SqliteStore};
use kontor_teams::run::{TeamClosureCertificate, TeamRunLease, TeamRunSlots};
use kontor_teams::spec::TeamTemplateSpec;
use kontor_tests_e2e::Bundle;
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::at;

/// The pilot's fixed decision instant, shared with every other section so the
/// bundle reads as one run rather than six clocks.
const DECIDED_AT: &str = "2026-08-12T09:00:00Z";

/// The bundled category the pilot's UX task pins.
const UX_CATEGORY: &str = "ux-ui-layout";

/// The first review gate of that profile. Two reviewers and two gates are needed
/// to show that a reset is narrow, and this is the one the reset stream uses.
const REVIEW_GATE: &str = "design-review-gate";

/// A second gate of the same profile, authorized to the same role. Sharing the
/// role is deliberate: if the counter were keyed on anything coarser than
/// (gate, principal), this gate would be the one that leaks.
const CODE_REVIEW_GATE: &str = "layout-code-review-gate";

/// The role the pinned profile authorizes over both review gates.
const REVIEWER_ROLE: &str = "inspector";

/// The reviewer whose rejection stream is under test.
const REVIEWER_ALPHA: &str = "pilot-reviewer-alpha";

/// A second, independent reviewer of the same gate.
const REVIEWER_BETA: &str = "pilot-reviewer-beta";

/// The worktree the pilot's UX task is pinned to.
const PILOT_WORKTREE: &str = "/tmp/kontor-pilot/pilot-ux-tree";

/// Answer every gate-authority, rejection-stream and durability criterion.
pub(crate) async fn run(bundle: &mut Bundle) {
    rejection_loop(bundle);
    rejection_reset(bundle);
    degraded_verdict(bundle);
    worktree_park(bundle);
    persona_self_approval(bundle);
    ux_gate_order(bundle);
    profile_durability(bundle);
}

// ---------------------------------------------------------------------------
// 1. The rejection loop
// ---------------------------------------------------------------------------

/// The same reviewer rejecting the same gate twice parks the work, and the third
/// run is never launched.
///
/// The two rejections are recorded from two *different* agent runs, the second a
/// child of the first: that is the relaunch a counter keyed on the run would
/// silently forgive. The park plan handed to the store is not hand-written — it
/// carries the guardrail evaluation `kontor_policy` actually produced for that
/// moment, so the pure rule and the transaction that acts on it are proved to
/// agree rather than assumed to.
fn rejection_loop(bundle: &mut Bundle) {
    let fixture = GateFixture::open("rejection-loop");
    let target = gate(REVIEW_GATE);
    let reviewer = external(REVIEWER_ALPHA);

    let first_run = fixture.agent_run(None);
    let first = fixture.reject(&target, &reviewer, first_run, "first");
    let after_first = fixture.task_state();

    // The builder is relaunched: a new run, descending from the parked one, the
    // same human behind it.
    let second_run = fixture.agent_run(Some(first_run));
    let second = fixture.reject(&target, &reviewer, second_run, "second");
    let after_second = fixture.task_state();
    let second_lifecycle = fixture.run_lifecycle(second_run);

    // The third attempt. Guarded exactly as a launcher would be: the decision
    // first, the launch only inside the `if`.
    let history = fixture.history();
    let mut third = fixture
        .subject
        .request(GuardrailRuleKey::SecondRejectionParks);
    third.gate = Some(target.clone());
    third.actor.principal = reviewer.clone();
    third.run.requested_action.intent = ActionIntent::RecordGateRejection;
    third.prior_gate_evaluations = history.clone();
    let verdict = decide(&third).expect("the request decides");
    let runs_before = fixture.count("agent_runs");
    if verdict.admits() {
        fixture.agent_run(Some(second_run));
    }
    let runs_after = fixture.count("agent_runs");

    let park = second.as_ref().ok().and_then(|outcome| outcome.parked);
    let receipt = bundle
        .artifact(
            "receipts/gate-park.json",
            &json!({
                "gate": target.as_str(),
                "reviewer_principal": reviewer.as_str(),
                "rejections_before_park": REJECTIONS_BEFORE_PARK,
                "evaluation_id": park.map(|park| park.evaluation_id.to_string()),
                "recovery_episode_id": park.map(|park| park.episode_id.to_string()),
                "closure_receipt_id": park.map(|park| park.closure_receipt_id.to_string()),
                "parked_agent_run_id": park.map(|park| park.parked_agent_run_id.to_string()),
                "task_state_after": after_second.to_string(),
                "run_lifecycle_after": second_lifecycle.map(|state| state.to_string()),
            }),
        )
        .expect("the park receipt is written");

    let snapshot = bundle
        .artifact(
            "snapshots/gate-rejection-loop.json",
            &json!({
                "gate": target.as_str(),
                "reviewer_principal": reviewer.as_str(),
                "attempts": [
                    outcome_json("first", first_run, None, first.as_ref(), after_first),
                    outcome_json("second", second_run, Some(first_run), second.as_ref(), after_second),
                ],
                "third_launch": {
                    "guardrail": GuardrailRuleKey::SecondRejectionParks.to_string(),
                    "verdict": verdict.verdict.to_string(),
                    "reason_code": verdict.reason_code.to_string(),
                    "admits": verdict.admits(),
                    "agent_runs_before": runs_before,
                    "agent_runs_after": runs_after,
                },
                "history": history.iter().map(evaluation_json).collect::<Vec<_>>(),
            }),
        )
        .expect("the rejection-loop snapshot is written");

    bundle.event(
        "gate-park",
        json!({
            "gate": target.as_str(),
            "reason_code": verdict.reason_code.to_string(),
            "task_state": after_second.to_string(),
        }),
    );

    let mut problems = Vec::new();
    match first.as_ref() {
        Ok(outcome) => {
            if outcome.sequence != 1 || outcome.rejections_since_pass != 1 {
                problems.push(format!(
                    "the first rejection recorded sequence {} and count {}",
                    outcome.sequence, outcome.rejections_since_pass
                ));
            }
            if outcome.parked.is_some() {
                problems.push("one rejection parked the work".to_owned());
            }
        }
        Err(error) => problems.push(format!("the first rejection was refused: {error}")),
    }
    match second.as_ref() {
        Ok(outcome) => {
            if outcome.rejections_since_pass != REJECTIONS_BEFORE_PARK {
                problems.push(format!(
                    "the relaunched rejection counted {} rather than {REJECTIONS_BEFORE_PARK}",
                    outcome.rejections_since_pass
                ));
            }
            if outcome.parked.is_none() {
                problems.push("the second rejection did not park".to_owned());
            }
        }
        Err(error) => problems.push(format!("the second rejection was refused: {error}")),
    }
    if after_first != TaskState::InProgress {
        problems.push(format!(
            "the task left `in_progress` after one rejection: {after_first}"
        ));
    }
    if after_second != TaskState::Parked {
        problems.push(format!("the parked task reads `{after_second}`"));
    }
    if second_lifecycle != Some(RunLifecycle::Parked) {
        problems.push(format!("the second run closed as {second_lifecycle:?}"));
    }
    if verdict.verdict != PolicyVerdict::Park
        || verdict.reason_code != ReasonCode::SecondRejectionParks
    {
        problems.push(format!(
            "the third attempt decided {} / {}",
            verdict.verdict, verdict.reason_code
        ));
    }
    if runs_after != runs_before {
        problems.push(format!(
            "a third run was launched: {runs_before} -> {runs_after}"
        ));
    }

    if problems.is_empty() {
        bundle.pass(
            "negative.rejection-loop",
            format!(
                "one reviewer rejected `{}` from two linked runs; the second rejection parked the \
                 task, closed its run as `parked` and opened a recovery episode in one committed \
                 unit, and the guarded third launch decided `{}` / `{}` so the run count stayed at \
                 {runs_after}",
                target,
                PolicyVerdict::Park,
                ReasonCode::SecondRejectionParks
            ),
            &[snapshot, receipt],
        );
    } else {
        bundle.fail("negative.rejection-loop", problems.join("; "));
    }
}

// ---------------------------------------------------------------------------
// 2. The reset is narrow
// ---------------------------------------------------------------------------

/// A pass clears one reviewer's stream on one gate and nothing else.
///
/// Six recorded verdicts, and after each one all four (gate, principal) streams
/// are re-derived from the append-only history. The last step is the real
/// mutation-killer: the reviewer who already rejected once and then passed
/// rejects again, and must *not* park — a reset that quietly failed to reset
/// would park the task right there.
fn rejection_reset(bundle: &mut Bundle) {
    let fixture = GateFixture::open("rejection-reset");
    let reviewed = gate(REVIEW_GATE);
    let coded = gate(CODE_REVIEW_GATE);
    let alpha = external(REVIEWER_ALPHA);
    let beta = external(REVIEWER_BETA);
    let run = fixture.agent_run(None);

    let mut steps = Vec::new();
    let mut problems = Vec::new();
    let mut parked_early = Vec::new();

    // Step 1: alpha rejects the review gate.
    match fixture.reject(&reviewed, &alpha, run, "alpha-one") {
        Ok(outcome) if outcome.parked.is_some() => parked_early.push("alpha's first rejection"),
        Ok(_) => {}
        Err(error) => problems.push(format!("alpha's first rejection was refused: {error}")),
    }
    steps.push(fixture.streams(
        "alpha rejects the review gate",
        &reviewed,
        &coded,
        &alpha,
        &beta,
    ));

    // Step 2: alpha starts the gate again. Starting is not a decision about the
    // work, so the stream must be exactly where it was.
    if let Err(error) = fixture.append(&reviewed, GateVerdict::Started, &alpha, run) {
        problems.push(format!("alpha could not start the gate: {error}"));
    }
    steps.push(fixture.streams(
        "alpha starts the gate again",
        &reviewed,
        &coded,
        &alpha,
        &beta,
    ));

    // Step 3: alpha passes it. This is the only event that clears the stream.
    if let Err(error) = fixture.append(&reviewed, GateVerdict::Passed, &alpha, run) {
        problems.push(format!("alpha could not pass the gate: {error}"));
    }
    steps.push(fixture.streams(
        "alpha passes the review gate",
        &reviewed,
        &coded,
        &alpha,
        &beta,
    ));

    // Step 4: beta rejects the same gate. Alpha's pass was not beta's.
    match fixture.reject(&reviewed, &beta, run, "beta-one") {
        Ok(outcome) if outcome.parked.is_some() => parked_early.push("beta's first rejection"),
        Ok(_) => {}
        Err(error) => problems.push(format!("beta's rejection was refused: {error}")),
    }
    steps.push(fixture.streams(
        "beta rejects the same gate",
        &reviewed,
        &coded,
        &alpha,
        &beta,
    ));

    // Step 5: alpha rejects a different gate. Two gates, one stream each.
    match fixture.reject(&coded, &alpha, run, "alpha-other-gate") {
        Ok(outcome) if outcome.parked.is_some() => {
            parked_early.push("alpha's rejection of the second gate")
        }
        Ok(_) => {}
        Err(error) => problems.push(format!(
            "alpha's second-gate rejection was refused: {error}"
        )),
    }
    steps.push(fixture.streams(
        "alpha rejects the code-review gate",
        &reviewed,
        &coded,
        &alpha,
        &beta,
    ));

    // Step 6: alpha rejects the reset gate again. One, not two — and no park.
    let reprise = fixture.reject(&reviewed, &alpha, run, "alpha-after-reset");
    match reprise.as_ref() {
        Ok(outcome) => {
            if outcome.parked.is_some() {
                problems.push(
                    "the rejection after a pass parked, so the pass never reset the stream"
                        .to_owned(),
                );
            }
            if outcome.rejections_since_pass != 1 {
                problems.push(format!(
                    "the rejection after a pass counted {} rather than 1",
                    outcome.rejections_since_pass
                ));
            }
        }
        Err(error) => problems.push(format!("the rejection after a pass was refused: {error}")),
    }
    steps.push(fixture.streams(
        "alpha rejects the reset gate again",
        &reviewed,
        &coded,
        &alpha,
        &beta,
    ));

    for early in parked_early {
        problems.push(format!("{early} parked before the threshold"));
    }

    // The store derives the counts in SQL and `kontor-policy` derives them in
    // Rust from the same append-only rows. Two derivations, one answer, or the
    // audit and the guardrail disagree about who rejected what.
    let history = fixture.history();
    let mut disagreements = Vec::new();
    for (label, target, principal) in [
        ("alpha@review", &reviewed, &alpha),
        ("beta@review", &reviewed, &beta),
        ("alpha@code-review", &coded, &alpha),
        ("beta@code-review", &coded, &beta),
    ] {
        let stored = fixture.stream(target, principal);
        let derived = rejections_since_pass(&history, target, principal);
        if stored != derived {
            disagreements.push(format!("{label}: store {stored}, policy {derived}"));
        }
    }
    problems.extend(disagreements.iter().cloned());

    let final_streams = steps.last().cloned().unwrap_or_else(|| json!({}));
    let artifact = bundle
        .artifact(
            "snapshots/gate-rejection-reset.json",
            &json!({
                "reset_gate": reviewed.as_str(),
                "untouched_gate": coded.as_str(),
                "reviewers": [alpha.as_str(), beta.as_str()],
                "rule": "only a `passed` verdict by the same principal on the same gate clears a \
                         stream; `started`, `waived` and `parked` are not decisions about the work",
                "steps": steps,
                "store_and_policy_agree": disagreements.is_empty(),
                "history": history.iter().map(evaluation_json).collect::<Vec<_>>(),
            }),
        )
        .expect("the rejection-reset snapshot is written");

    // What the whole case comes down to, read off the final streams.
    let expected = json!({ "alpha@review": 1, "beta@review": 1, "alpha@code-review": 1, "beta@code-review": 0 });
    if let Some(counts) = final_streams.get("streams")
        && counts != &expected
    {
        problems.push(format!("final streams are {counts} rather than {expected}"));
    }

    if problems.is_empty() {
        bundle.pass(
            "negative.rejection-reset",
            "alpha's pass on the review gate cleared alpha's stream on that gate only: beta's \
             count on the same gate and alpha's count on the code-review gate were untouched, a \
             `started` verdict moved nothing, and alpha's next rejection of the reset gate counted \
             one and did not park — which it would have, had the reset not happened",
            &[artifact],
        );
    } else {
        bundle.fail("negative.rejection-reset", problems.join("; "));
    }
}

// ---------------------------------------------------------------------------
// 3. Degraded authority
// ---------------------------------------------------------------------------

/// A rung-1 binding cannot write a gate verdict, and nothing moves when it tries.
///
/// Two disposable realms rather than one, so neither end state is ambiguous: the
/// degraded realm finishes with no verdict at all, and the contrast realm — same
/// actor, same gate, same evidence, one rung higher — finishes with the verdict
/// written. That is what makes the empty realm evidence about the rung instead of
/// evidence about a writer that never worked.
fn degraded_verdict(bundle: &mut Bundle) {
    let target = gate(REVIEW_GATE);
    let degraded_rung = VerdictRung::parse(1).expect("rung 1 is a legal, degraded rung");

    let refused = GateFixture::open("degraded-verdict");
    let (denial, denied_write) = refused.guarded_pass(&target, degraded_rung);
    let refused_states = refused
        .store
        .gate_states(refused.subject.project, refused.subject.workflow);
    let refused_gate = refused_states
        .as_ref()
        .ok()
        .and_then(|states| states.get(&target).copied())
        .unwrap_or(GateState::NotReady);
    let refused_task = refused.task_state();
    let refused_rows = refused.count("task_gate_evaluations");

    let allowed = GateFixture::open("degraded-verdict-contrast");
    let (grant, granted_write) = allowed.guarded_pass(&target, VerdictRung::VERDICT_THRESHOLD);
    let allowed_gate = allowed
        .store
        .gate_states(allowed.subject.project, allowed.subject.workflow)
        .ok()
        .and_then(|states| states.get(&target).copied())
        .unwrap_or(GateState::NotReady);

    let artifact = bundle
        .artifact(
            "receipts/degraded-verdict-refusal.json",
            &json!({
                "gate": target.as_str(),
                "guardrail": GuardrailRuleKey::DegradedVerdictDenied.to_string(),
                "intent": ActionIntent::RecordGateVerdict.to_string(),
                "verdict_threshold": VerdictRung::VERDICT_THRESHOLD.get(),
                "degraded": {
                    "verdict_rung": degraded_rung.get(),
                    "verdict": denial.verdict.to_string(),
                    "reason_code": denial.reason_code.to_string(),
                    "admits": denial.admits(),
                    "sequence_written": denied_write,
                    "gate_evaluation_rows": refused_rows,
                    "gate_state": refused_gate.to_string(),
                    "gate_satisfies_closure": refused_gate.satisfies_requirement(),
                    "task_state": refused_task.to_string(),
                    "task_is_terminal": refused_task.is_terminal(),
                },
                "contrast": {
                    "verdict_rung": VerdictRung::VERDICT_THRESHOLD.get(),
                    "verdict": grant.verdict.to_string(),
                    "reason_code": grant.reason_code.to_string(),
                    "admits": grant.admits(),
                    "sequence_written": granted_write,
                    "gate_state": allowed_gate.to_string(),
                },
            }),
        )
        .expect("the degraded-verdict receipt is written");

    let denied = denial.verdict == PolicyVerdict::Block
        && denial.reason_code == ReasonCode::VerdictRungDegraded
        && denied_write.is_none()
        && refused_rows == 0;
    let non_terminal = !refused_gate.satisfies_requirement() && !refused_task.is_terminal();
    let contrast = grant.verdict == PolicyVerdict::Pass
        && grant.reason_code == ReasonCode::VerdictAuthorityHeld
        && granted_write == Some(1)
        && allowed_gate == GateState::Passed;

    if denied && non_terminal && contrast {
        bundle.pass(
            "negative.degraded-verdict",
            format!(
                "a rung-{} binding holding a role the gate authorizes was refused `{}` / `{}` and \
                 wrote nothing: the gate stayed `{refused_gate}` and the task stayed \
                 `{refused_task}`, both non-terminal. The identical request one rung higher was \
                 admitted `{}` and did write the verdict, so the refusal is about the rung",
                degraded_rung.get(),
                PolicyVerdict::Block,
                ReasonCode::VerdictRungDegraded,
                ReasonCode::VerdictAuthorityHeld
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "negative.degraded-verdict",
            format!(
                "denied={denied} (verdict {} / {}, wrote {denied_write:?}, {refused_rows} rows), \
                 non_terminal={non_terminal} (gate {refused_gate}, task {refused_task}), \
                 contrast={contrast} (verdict {} / {}, wrote {granted_write:?}, gate {allowed_gate})",
                denial.verdict, denial.reason_code, grant.verdict, grant.reason_code
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 4. The worktree parks
// ---------------------------------------------------------------------------

/// A wrong, unclaimed or ambiguous worktree parks; it never merely blocks.
///
/// The distinction is the case. `Block` refuses one action and leaves the run
/// free to try another; `Park` stops the work and hands it to recovery. A run
/// that has lost track of which tree it is editing is in no state to be trusted
/// with a tidy-up either, so every refusal below has to be the second one.
fn worktree_park(bundle: &mut Bundle) {
    let subject = Subject::new(ux_snapshot());
    let pinned = name(PILOT_WORKTREE);
    let elsewhere = name("/tmp/kontor-pilot/some-other-tree");
    let sibling = name("/tmp/kontor-pilot/pilot-code-tree");

    // (label, recorded pin, claim, candidates, expected verdict, expected reason)
    let table = [
        (
            "matches the pin",
            Some(pinned.clone()),
            Some(pinned.clone()),
            vec![pinned.clone()],
            PolicyVerdict::Pass,
            ReasonCode::WorktreeMatchesPin,
        ),
        (
            "moved away from the pin",
            Some(pinned.clone()),
            Some(elsewhere.clone()),
            vec![pinned.clone(), elsewhere.clone()],
            PolicyVerdict::Park,
            ReasonCode::WorktreeMoved,
        ),
        (
            "claims nothing at all",
            Some(pinned.clone()),
            None,
            vec![pinned.clone()],
            PolicyVerdict::Park,
            ReasonCode::WorktreeUnclaimed,
        ),
        (
            "unpinned and claims nothing",
            None,
            None,
            vec![pinned.clone()],
            PolicyVerdict::Park,
            ReasonCode::WorktreeUnclaimed,
        ),
        (
            "unpinned, claims a tree nobody offered",
            None,
            Some(elsewhere.clone()),
            vec![pinned.clone()],
            PolicyVerdict::Park,
            ReasonCode::WorktreeMoved,
        ),
        (
            "unpinned, two plausible trees",
            None,
            Some(pinned.clone()),
            vec![pinned.clone(), sibling],
            PolicyVerdict::Park,
            ReasonCode::WorktreeAmbiguous,
        ),
        (
            "unpinned, exactly one candidate",
            None,
            Some(pinned.clone()),
            vec![pinned.clone()],
            PolicyVerdict::Pass,
            ReasonCode::WorktreeFirstClaim,
        ),
    ];

    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (label, recorded, claimed, candidates, want_verdict, want_reason) in table {
        let mut request = subject.request(GuardrailRuleKey::WorktreeSticky);
        request.run.recorded_worktree = recorded.clone();
        request.workspace = WorkspaceEvidence {
            claimed_worktree: claimed.clone(),
            candidate_worktrees: candidates.clone(),
            module_claims: Vec::new(),
        };
        let decision = decide(&request).expect("the request decides");
        rows.push(json!({
            "case": label,
            "recorded_worktree": recorded.as_ref().map(ExternalName::as_str),
            "claimed_worktree": claimed.as_ref().map(ExternalName::as_str),
            "candidate_worktrees": candidates.iter().map(ExternalName::as_str).collect::<Vec<_>>(),
            "verdict": decision.verdict.to_string(),
            "reason_code": decision.reason_code.to_string(),
            "admits": decision.admits(),
        }));
        if decision.verdict != want_verdict || decision.reason_code != want_reason {
            problems.push(format!(
                "{label}: wanted {want_verdict} / {want_reason}, got {} / {}",
                decision.verdict, decision.reason_code
            ));
        }
        if want_verdict == PolicyVerdict::Park && decision.verdict == PolicyVerdict::Block {
            problems.push(format!("{label}: blocked where it had to park"));
        }
    }

    let artifact = bundle
        .artifact(
            "snapshots/worktree-park.json",
            &json!({
                "guardrail": GuardrailRuleKey::WorktreeSticky.to_string(),
                "pinned_worktree": pinned.as_str(),
                "rule": "a run acts in the tree it was first recorded in or it parks; ambiguity is \
                         never resolved by preferring the first, the newest or the shortest \
                         candidate",
                "cases": rows,
            }),
        )
        .expect("the worktree truth table is written");

    if problems.is_empty() {
        bundle.pass(
            "negative.worktree-park",
            format!(
                "all seven worktree positions decided as declared: a moved tree and a tree nobody \
                 offered park as `{}`, an unclaimed one as `{}`, two plausible trees as `{}`, and \
                 only an exact pin or a single offered candidate passes — every refusal is `{}`, \
                 never `{}`",
                ReasonCode::WorktreeMoved,
                ReasonCode::WorktreeUnclaimed,
                ReasonCode::WorktreeAmbiguous,
                PolicyVerdict::Park,
                PolicyVerdict::Block
            ),
            &[artifact],
        );
    } else {
        bundle.fail("negative.worktree-park", problems.join("; "));
    }
}

// ---------------------------------------------------------------------------
// 5. The persona cannot approve itself
// ---------------------------------------------------------------------------

/// A simulated persona never records the verdict on the gate it is under test for.
///
/// The scenario is not invented here: it is the persona the bundled pack itself
/// binds to this profile, so the gate under test and the acting role are the ones
/// a deployment shipped. The acting role is one the gate *does* authorize and the
/// rung is at threshold, which is what leaves the persona as the only possible
/// explanation for the refusal.
fn persona_self_approval(bundle: &mut Bundle) {
    let fixture = GateFixture::open("persona-self-approval");
    let scenario = ux_persona();
    let under_test = scenario.gate_under_test.clone();
    let other = gate(REVIEW_GATE);
    let evaluator = scenario
        .evaluator_roles
        .first()
        .cloned()
        .expect("a validated scenario names an independent evaluator");

    let persona = PersonaActor {
        persona: scenario.persona.clone(),
        gate_under_test: under_test.clone(),
        actor_role: scenario.actor_role.clone(),
    };

    let judge = |target: &GateKey| {
        let mut request = fixture
            .subject
            .request(GuardrailRuleKey::DegradedVerdictDenied);
        request.gate = Some(target.clone());
        request.actor.role = evaluator.clone();
        request.actor.verdict_rung = VerdictRung::VERDICT_THRESHOLD;
        request.actor.persona = Some(persona.clone());
        request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
        decide(&request).expect("the request decides")
    };

    let own = judge(&under_test);
    let foreign = judge(&other);

    // And the guarded write, so the refusal is an absence in the store too.
    let attempted = if own.admits() {
        fixture
            .append(
                &under_test,
                GateVerdict::Passed,
                &external("pilot-persona"),
                fixture.agent_run(None),
            )
            .ok()
    } else {
        None
    };
    let rows = fixture.count("task_gate_evaluations");

    let artifact = bundle
        .artifact(
            "receipts/persona-self-approval.json",
            &json!({
                "scenario_id": scenario.scenario_id.to_string(),
                "scenario_version": scenario.version.get(),
                "persona": scenario.persona.to_string(),
                "actor_role": scenario.actor_role.as_str(),
                "gate_under_test": under_test.as_str(),
                "independent_evaluator_roles": scenario
                    .evaluator_roles
                    .iter()
                    .map(RoleKey::as_str)
                    .collect::<Vec<_>>(),
                "acting_role": evaluator.as_str(),
                "verdict_rung": VerdictRung::VERDICT_THRESHOLD.get(),
                "own_gate": {
                    "gate": under_test.as_str(),
                    "verdict": own.verdict.to_string(),
                    "reason_code": own.reason_code.to_string(),
                },
                "any_other_gate": {
                    "gate": other.as_str(),
                    "verdict": foreign.verdict.to_string(),
                    "reason_code": foreign.reason_code.to_string(),
                },
                "sequence_written": attempted,
                "gate_evaluation_rows": rows,
            }),
        )
        .expect("the persona receipt is written");

    let self_refused =
        own.verdict == PolicyVerdict::Block && own.reason_code == ReasonCode::PersonaSelfApproval;
    let never_evaluates = foreign.verdict == PolicyVerdict::Block
        && foreign.reason_code == ReasonCode::PersonaCannotEvaluate;
    if self_refused && never_evaluates && attempted.is_none() && rows == 0 {
        bundle.pass(
            "domain.persona-self-approval",
            format!(
                "the pack's own persona `{}`, acting as `{evaluator}` — a role `{under_test}` \
                 authorizes — at the verdict rung, was refused `{}` / `{}` on the gate it is under \
                 test for, and `{}` on every other gate; no verdict row exists",
                scenario.persona,
                PolicyVerdict::Block,
                ReasonCode::PersonaSelfApproval,
                ReasonCode::PersonaCannotEvaluate
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.persona-self-approval",
            format!(
                "own gate decided {} / {}, other gate {} / {}, wrote {attempted:?} with {rows} rows",
                own.verdict, own.reason_code, foreign.verdict, foreign.reason_code
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 6. The UX task closes last
// ---------------------------------------------------------------------------

/// The bundled UX task cannot close until functionality QA, design QA and the
/// final audit have each passed.
///
/// Closure is asked for four times with everything else satisfied — every phase
/// complete, every artifact produced, the team's own closure certificate in hand
/// — and only the three QA-and-audit gates varied. Each withheld gate is left
/// `active` rather than absent: the work started, it simply was not decided, and
/// "started" is not "passed".
///
/// The escape hatch is checked too. A waiver is only an alternative where the
/// profile allows one, and this profile allows none on these gates, so the
/// attempt is refused rather than quietly honoured.
fn ux_gate_order(bundle: &mut Bundle) {
    let resolved = ux_bundle();
    let profile = &resolved.profile;
    let phases: BTreeSet<PhaseKey> = profile
        .definition
        .phases
        .iter()
        .map(|phase| phase.id.clone())
        .collect();
    let artifacts: BTreeSet<ArtifactKey> = profile
        .definition
        .artifacts
        .iter()
        .map(|contract| contract.key.clone())
        .collect();
    let all_passed: BTreeMap<GateKey, GateState> = profile
        .definition
        .gates
        .iter()
        .map(|gate| (gate.id.clone(), GateState::Passed))
        .collect();

    let (team_run_id, certificate) = team_closure(&resolved);
    let team = TaskTeamEvidence::Certified {
        team_run_id,
        certificate: &certificate,
    };

    let late = [
        gate("functionality-qa-gate"),
        gate("design-qa-gate"),
        gate("final-audit-gate"),
    ];

    let mut attempts = Vec::new();
    let mut problems = Vec::new();

    // One withheld gate at a time, then all three at once.
    let mut withheld_sets: Vec<(String, Vec<GateKey>)> = late
        .iter()
        .map(|key| (format!("only {key} withheld"), vec![key.clone()]))
        .collect();
    withheld_sets.push(("all three withheld".to_owned(), late.to_vec()));

    for (label, withheld) in withheld_sets {
        let mut states = all_passed.clone();
        for key in &withheld {
            states.insert(key.clone(), GateState::Active);
        }
        let outcome = certify_task_closure(profile, team, &phases, &states, &artifacts, &[]);
        attempts.push(json!({
            "case": label,
            "withheld_gates": withheld.iter().map(GateKey::as_str).collect::<Vec<_>>(),
            "withheld_state": GateState::Active.to_string(),
            "closed": outcome.is_ok(),
            "refusal": outcome.as_ref().err().map(ToString::to_string),
        }));
        if outcome.is_ok() {
            problems.push(format!("{label}: the task closed anyway"));
        }
    }

    // A waiver is not an alternative here: the profile forbids waiving these
    // gates, so the receipt is refused rather than accepted as authority.
    let waived_gate = late[0].clone();
    let spec = profile
        .definition
        .gate(&waived_gate)
        .expect("the pinned profile declares its own gate");
    let mut waived_states = all_passed.clone();
    waived_states.insert(waived_gate.clone(), GateState::Waived);
    let waiver = GateWaiver {
        gate: waived_gate.clone(),
        authorized_by: role("architect"),
        evidence: spec.required_evidence.clone(),
        recorded_at: at(DECIDED_AT),
    };
    let waived = certify_task_closure(
        profile,
        team,
        &phases,
        &waived_states,
        &artifacts,
        std::slice::from_ref(&waiver),
    );
    if waived.is_ok() {
        problems.push(format!(
            "{waived_gate}: a waiver closed a gate the profile forbids waiving"
        ));
    }

    // Everything passed, and only then.
    let closed = certify_task_closure(profile, team, &phases, &all_passed, &artifacts, &[]);
    if let Err(error) = closed.as_ref() {
        problems.push(format!("a fully satisfied UX task did not close: {error}"));
    }

    let artifact = bundle
        .artifact(
            "snapshots/ux-closure.json",
            &json!({
                "category": resolved.category.to_string(),
                "profile": profile.definition.id.to_string(),
                "profile_version": profile.definition.version.get(),
                "definition_hash": profile.definition_hash.to_string(),
                "bundle_hash": resolved.bundle_hash.to_string(),
                "phases": profile
                    .definition
                    .phases
                    .iter()
                    .map(|phase| phase.id.to_string())
                    .collect::<Vec<_>>(),
                "gates": profile
                    .definition
                    .gates
                    .iter()
                    .map(|gate| json!({
                        "gate": gate.id.as_str(),
                        "phase": gate.phase.as_str(),
                        "evaluator_roles": gate.evaluator_roles.iter().map(RoleKey::as_str).collect::<Vec<_>>(),
                        "waiver_allowed": gate.waiver_allowed,
                    }))
                    .collect::<Vec<_>>(),
                "team_run_id": team_run_id.to_string(),
                "team_policy_digest": certificate.policy_digest().to_string(),
                "refused": attempts,
                "waiver_attempt": {
                    "gate": waived_gate.as_str(),
                    "authorized_by": waiver.authorized_by.as_str(),
                    "cited_evidence": waiver.evidence.iter().map(ArtifactKey::as_str).collect::<Vec<_>>(),
                    "waiver_allowed_by_profile": spec.waiver_allowed,
                    "closed": waived.is_ok(),
                    "refusal": waived.as_ref().err().map(ToString::to_string),
                },
                "closed_when_everything_passed": closed.is_ok(),
            }),
        )
        .expect("the UX closure evidence is written");

    if problems.is_empty() {
        bundle.pass(
            "domain.ux-gate-order",
            format!(
                "with every phase complete, every artifact produced and the team's closure \
                 certificate presented, `{}` refused to close while functionality QA, design QA or \
                 the final audit were merely `{}` — one at a time and all three together — and \
                 refused a waiver of the QA gate because the pinned profile allows none; it closed \
                 only once all six gates read `{}`",
                resolved.category,
                GateState::Active,
                GateState::Passed
            ),
            &[artifact],
        );
    } else {
        bundle.fail("domain.ux-gate-order", problems.join("; "));
    }
}

// ---------------------------------------------------------------------------
// 7. Durability
// ---------------------------------------------------------------------------

/// A restart changes nothing about what a task has proved.
///
/// The realm is seeded with a pinned profile revision, a real phase advance, a
/// gate history carrying its evidence and the guardrail evaluation that
/// authorized it, and artifact-evidence rows. Then the store is dropped — the
/// connection closed, nothing cached — and reopened from the same path. The whole
/// probe is compared as one canonical document, so a single changed field
/// anywhere fails the case rather than only the fields somebody remembered to
/// assert on.
fn profile_durability(bundle: &mut Bundle) {
    let fixture = GateFixture::open("profile-durability");
    let target = gate(REVIEW_GATE);
    let reviewer = external(REVIEWER_ALPHA);
    let run = fixture.agent_run(None);
    let mut problems = Vec::new();

    // A real advance along an edge the profile declares.
    let before_advance = fixture
        .store
        .get_active_task_workflow(fixture.subject.project, fixture.subject.task)
        .ok()
        .flatten()
        .map_or(AggregateRevision::INITIAL, |workflow| workflow.revision);
    if let Err(error) = fixture.store.advance_phase(&PhaseAdvance {
        project_id: fixture.subject.project,
        workflow_id: fixture.subject.workflow,
        expected_revision: before_advance,
        next_phase: phase("design-review"),
        advanced_at: at(DECIDED_AT),
    }) {
        problems.push(format!("the phase did not advance: {error}"));
    }

    // The artifacts the gate's evidence points at, as references rather than
    // content — the store holds a locator, never the artifact itself.
    let evidence = fixture.gate_evidence(&target);
    for key in &evidence {
        if let Err(error) = fixture
            .store
            .record_artifact_evidence(&NewArtifactEvidence {
                id: kontor_policy::ArtifactEvidenceId::generate(),
                binding: fixture.binding(Some(run)),
                key: key.clone(),
                locator: document(&format!("locator-{key}")),
                producer_role: role(REVIEWER_ROLE),
                producer_account: fixture.subject.account,
                recorded_at: at(DECIDED_AT),
            })
        {
            problems.push(format!("artifact evidence for {key} was refused: {error}"));
        }
    }

    // The authority behind the verdict, recorded as the guardrail evaluation it
    // actually was and linked from the verdict row.
    let mut request = fixture
        .subject
        .request(GuardrailRuleKey::DegradedVerdictDenied);
    request.gate = Some(target.clone());
    request.actor.principal = reviewer.clone();
    request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
    request.run.agent_run_id = Some(run);
    let authority = evaluate(&request, GuardrailEvaluationId::generate())
        .expect("the authority evaluation is produced");
    if let Err(error) = fixture
        .store
        .append_policy_evaluation(&fixture.binding(Some(run)), &authority)
    {
        problems.push(format!("the authority evidence was refused: {error}"));
    }

    for verdict in [GateVerdict::Started, GateVerdict::Passed] {
        if let Err(error) = fixture.store.append_gate_evaluation(&NewGateEvaluation {
            project_id: fixture.subject.project,
            workflow_id: fixture.subject.workflow,
            gate: target.clone(),
            verdict,
            evaluator_role: role(REVIEWER_ROLE),
            evaluator_account: fixture.subject.account,
            evidence: evidence.clone(),
            agent_run_id: Some(run),
            session_evidence: None,
            reviewer_principal: Some(reviewer.clone()),
            policy_evaluation_id: Some(authority.id),
            recorded_at: at(DECIDED_AT),
        }) {
            problems.push(format!("the {verdict} verdict was refused: {error}"));
        }
    }

    let before = fixture.probe();

    // The restart. Dropping the store closes the connection; the reopened one
    // shares nothing with it but the file.
    let GateFixture {
        _directory,
        path,
        store,
        team_run,
        subject,
    } = fixture;
    drop(store);
    let reopened = GateFixture {
        _directory,
        path: path.clone(),
        store: SqliteStore::open(&path).expect("the store reopens from the same path"),
        team_run,
        subject,
    };
    let after = reopened.probe();

    let identical = before == after;
    let artifact = bundle
        .artifact(
            "snapshots/profile-durability.json",
            &json!({
                "database": "file-backed SQLite, closed and reopened from the same path",
                "before_restart": before,
                "after_restart": after,
                "identical": identical,
            }),
        )
        .expect("the durability snapshot is written");

    if !identical {
        problems.push("the probe read back differently after the restart".to_owned());
    }
    let survived = after
        .get("gate_evaluations")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if survived != 2 {
        problems.push(format!(
            "{survived} gate evaluations survived rather than two"
        ));
    }
    if after.get("snapshot_reverifies") != Some(&Value::Bool(true)) {
        problems.push("the reloaded snapshot did not re-derive its pinned hash".to_owned());
    }
    if after.get("current_phase") != Some(&json!("design-review")) {
        problems.push(format!(
            "the phase read back as {:?}",
            after.get("current_phase")
        ));
    }

    if problems.is_empty() {
        bundle.pass(
            "domain.profile-durability",
            format!(
                "after closing and reopening the database, the task's pinned profile revision and \
                 `definition_hash` re-derived intact, the advanced phase, both gate verdicts with \
                 their sequences, principals, cited evidence and linked authority evaluation, the \
                 {} artifact-evidence rows and the guardrail evaluation all read back byte-identical",
                evidence.len()
            ),
            &[artifact],
        );
    } else {
        bundle.fail("domain.profile-durability", problems.join("; "));
    }
}

// ---------------------------------------------------------------------------
// The disposable realm
// ---------------------------------------------------------------------------

/// The ids and pinned snapshot every guardrail request has to name.
///
/// Separate from the store fixture so the pure cases — the worktree table has no
/// database in it at all — can build a legal request without opening one.
struct Subject {
    /// The frozen profile every rule reads authority out of.
    snapshot: ResolvedWorkProfileSnapshot,
    /// The disposable project.
    project: ProjectId,
    /// The task under test.
    task: TaskId,
    /// Its workflow.
    workflow: TaskWorkflowId,
    /// The account every actor authenticates through.
    account: AccountProfileId,
}

impl Subject {
    /// Fresh ids over a pinned snapshot.
    fn new(snapshot: ResolvedWorkProfileSnapshot) -> Self {
        Self {
            snapshot,
            project: ProjectId::generate(),
            task: TaskId::generate(),
            workflow: TaskWorkflowId::generate(),
            account: AccountProfileId::generate(),
        }
    }

    /// A request with every guardrail input in its admitted position.
    ///
    /// Each case then moves exactly the one input it is about, so a refusal names
    /// the thing under test rather than the setup. The shape is the one
    /// `tests/contract/guardrails.rs` uses, for the same reason.
    fn request(&self, rule: GuardrailRuleKey) -> EvaluationRequest {
        let worktree = name(PILOT_WORKTREE);
        EvaluationRequest {
            schema_version: SCHEMA_VERSION,
            rule: GuardrailRule {
                key: rule,
                version: SpecVersion::FIRST,
            },
            workflow: self.snapshot.clone(),
            current_phase: phase("design-review"),
            gate: None,
            actor: ActorContext {
                account: self.account,
                principal: external(REVIEWER_ALPHA),
                role: role(REVIEWER_ROLE),
                verdict_rung: VerdictRung::VERDICT_THRESHOLD,
                persona: None,
            },
            run: RunContext {
                project_id: self.project,
                task_id: self.task,
                workflow_id: self.workflow,
                module: None,
                team_run_id: None,
                agent_run_id: None,
                parent_agent_run_id: None,
                pinned_account: Some(self.account),
                recorded_worktree: Some(worktree.clone()),
                requested_action: RequestedAction {
                    domain: ActionDomain::ControlPlane,
                    intent: ActionIntent::Inspect,
                    effect: ActionEffect::Read,
                    operation: name("evaluate-gate"),
                    target: external("gate"),
                    digest: ContentHash::of(b"evaluate-gate"),
                    dry_run_supported: false,
                    dry_run: false,
                },
                rule_set_revision: SpecVersion::FIRST,
            },
            workspace: WorkspaceEvidence {
                claimed_worktree: Some(worktree.clone()),
                candidate_worktrees: vec![worktree],
                module_claims: Vec::new(),
            },
            artifacts: Vec::new(),
            approval: None,
            prior_gate_evaluations: Vec::new(),
            terminal_observation: None,
            evaluated_at: at(DECIDED_AT),
        }
    }
}

/// One disposable realm: a real file-backed store holding one UX task.
struct GateFixture {
    /// Kept alive so the database file outlives the fixture.
    _directory: TempDir,
    /// The database path, so a case can reopen exactly these bytes.
    path: PathBuf,
    /// The open store.
    store: SqliteStore,
    /// The team run every agent run hangs off.
    team_run: TeamRunId,
    /// What every guardrail request names.
    subject: Subject,
}

impl GateFixture {
    /// Seed a realm. `label` only distinguishes the root paths in evidence.
    ///
    /// # Panics
    /// Panics when the store will not open or the seed will not persist. Both
    /// are driver bugs — the criteria are about what happens *after* a realm
    /// exists, and a realm that cannot be built proves nothing about them.
    fn open(label: &str) -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("kontor.db");
        let store = SqliteStore::open(&path).expect("the store opens and migrates itself");
        let resolved = ux_bundle();
        let subject = Subject::new(resolved.profile.clone());
        let runtime = RuntimeKindKey::parse("fake.runtime").expect("a legal runtime key");

        store
            .create_project(&NewProject {
                id: subject.project,
                name: name("Kontor pilot gates"),
                root_path: name(&format!("/tmp/kontor-pilot/gates/{label}")),
                created_at: at(DECIDED_AT),
            })
            .expect("the disposable project is created");
        store
            .create_account_profile(&NewAccountProfile {
                id: subject.account,
                project_id: subject.project,
                label: name("Pilot reviewer account"),
                external_account_id: Some(external("pilot-reviewer")),
                harness: runtime,
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: CredentialAlias::parse("pilot-reviewer").expect("a legal alias"),
                },
                environment: document("environment"),
                routing: document("routing"),
                capability: document("capability"),
                provider_identity: None,
                enabled: true,
                created_at: at(DECIDED_AT),
            })
            .expect("the reviewer account is created");
        store
            .create_task(&NewTask {
                id: subject.task,
                project_id: subject.project,
                mini_project_id: None,
                title: name("Pilot UX/UI/layout task"),
                module: None,
                state: TaskState::InProgress,
                created_at: at(DECIDED_AT),
            })
            .expect("the UX task is created");
        store
            .insert_work_profile(subject.project, &subject.snapshot.definition)
            .expect("the pinned profile is stored");
        store
            .create_task_workflow(&NewTaskWorkflow {
                id: subject.workflow,
                project_id: subject.project,
                task_id: subject.task,
                snapshot: subject.snapshot.clone(),
                current_phase: subject.snapshot.definition.entry_phase.clone(),
                created_at: at(DECIDED_AT),
            })
            .expect("the workflow freezes the profile");

        let template = resolved
            .team
            .clone()
            .expect("the bundled UX profile pins a team");
        store
            .insert_team_template(subject.project, &template)
            .expect("the pinned team revision is stored");
        let team_run = TeamRunId::generate();
        store
            .create_team_run(&NewTeamRun {
                id: team_run,
                project_id: subject.project,
                task_id: subject.task,
                snapshot: TeamRunSnapshot::from_revision(&template, SCHEMA_VERSION),
                created_at: at(DECIDED_AT),
            })
            .expect("the team run is created");

        Self {
            _directory: directory,
            path,
            store,
            team_run,
            subject,
        }
    }

    /// One agent run, optionally succeeding `parent`.
    ///
    /// # Panics
    /// Panics when the run cannot be created, which is a seeding failure.
    fn agent_run(&self, parent: Option<AgentRunId>) -> AgentRunId {
        let id = AgentRunId::generate();
        self.store
            .create_agent_run(&NewAgentRun {
                id,
                project_id: self.subject.project,
                team_run_id: self.team_run,
                parent_agent_run_id: parent,
                role: role(REVIEWER_ROLE),
                account_profile_id: Some(self.subject.account),
                binding: None,
                created_at: at(DECIDED_AT),
            })
            .expect("an agent run is created");
        id
    }

    /// Where a policy or evidence record belongs.
    fn binding(&self, agent_run_id: Option<AgentRunId>) -> EvaluationBinding {
        EvaluationBinding {
            project_id: self.subject.project,
            task_id: self.subject.task,
            workflow_id: self.subject.workflow,
            team_run_id: Some(self.team_run),
            agent_run_id,
        }
    }

    /// Every artifact the pinned profile makes a gate's evidence.
    fn gate_evidence(&self, target: &GateKey) -> Vec<ArtifactKey> {
        self.subject
            .snapshot
            .definition
            .gate(target)
            .map(|spec| spec.required_evidence.clone())
            .unwrap_or_default()
    }

    /// Record a rejection, carrying the park plan `kontor-policy` would produce
    /// for this exact moment.
    ///
    /// The plan is prepared unconditionally because the caller cannot know the
    /// count — the store derives it inside the transaction — and it is only
    /// validated and written when the park actually falls due. The evaluation
    /// inside it is the real one, computed over the stored history, so a park
    /// receipt can never cite a verdict the rule did not reach.
    fn reject(
        &self,
        target: &GateKey,
        principal: &ExternalId,
        run: AgentRunId,
        marker: &str,
    ) -> Result<kontor_store::RejectionOutcome, String> {
        let mut request = self.subject.request(GuardrailRuleKey::SecondRejectionParks);
        request.gate = Some(target.clone());
        request.actor.principal = principal.clone();
        request.run.agent_run_id = Some(run);
        request.run.requested_action.intent = ActionIntent::RecordGateRejection;
        request.prior_gate_evaluations = self.history();
        let evaluation = evaluate(&request, GuardrailEvaluationId::generate())
            .expect("the rejection evaluation is produced");

        self.store
            .record_gate_rejection(&GateRejection {
                project_id: self.subject.project,
                workflow_id: self.subject.workflow,
                gate: target.clone(),
                evaluator_role: self.gate_role(target),
                evaluator_account: self.subject.account,
                reviewer_principal: principal.clone(),
                agent_run_id: Some(run),
                evidence: Vec::new(),
                recorded_at: at(DECIDED_AT),
                park: ParkPlan {
                    evaluation,
                    episode_id: kontor_policy::RecoveryEpisodeId::generate(),
                    closure_receipt_id: CommandReceiptId::generate(),
                    closure_key: IdempotencyKey::parse(&format!("pilot-park-{marker}"))
                        .expect("a legal idempotency key"),
                    closure_intent: document(&format!("parked-{marker}")),
                },
            })
            .map_err(|error| error.to_string())
    }

    /// The first role the pinned profile authorizes over a gate.
    ///
    /// Read out of the snapshot rather than assumed, so a helper used on two
    /// different gates cannot accidentally make "the role was wrong" look like
    /// "the guardrail refused".
    fn gate_role(&self, target: &GateKey) -> RoleKey {
        self.subject
            .snapshot
            .definition
            .gate(target)
            .and_then(|spec| spec.evaluator_roles.first().cloned())
            .unwrap_or_else(|| role(REVIEWER_ROLE))
    }

    /// Append a non-rejecting verdict, citing whatever evidence the gate requires.
    fn append(
        &self,
        target: &GateKey,
        verdict: GateVerdict,
        principal: &ExternalId,
        run: AgentRunId,
    ) -> Result<u32, String> {
        self.store
            .append_gate_evaluation(&NewGateEvaluation {
                project_id: self.subject.project,
                workflow_id: self.subject.workflow,
                gate: target.clone(),
                verdict,
                evaluator_role: self.gate_role(target),
                evaluator_account: self.subject.account,
                evidence: self.gate_evidence(target),
                agent_run_id: Some(run),
                session_evidence: None,
                reviewer_principal: Some(principal.clone()),
                policy_evaluation_id: None,
                recorded_at: at(DECIDED_AT),
            })
            .map_err(|error| error.to_string())
    }

    /// Decide `degraded_verdict_denied` at `rung` and write the pass only if it
    /// admits.
    ///
    /// This is the guarded shape itself: the store call is inside the `if`, so a
    /// refusal leaves no row to undo.
    fn guarded_pass(&self, target: &GateKey, rung: VerdictRung) -> (Decision, Option<u32>) {
        let mut request = self
            .subject
            .request(GuardrailRuleKey::DegradedVerdictDenied);
        request.gate = Some(target.clone());
        request.actor.role = self.gate_role(target);
        request.actor.verdict_rung = rung;
        request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
        let decision = decide(&request).expect("the request decides");
        let written = if decision.admits() {
            let run = self.agent_run(None);
            self.append(target, GateVerdict::Passed, &external(REVIEWER_ALPHA), run)
                .ok()
        } else {
            None
        };
        (decision, written)
    }

    /// The whole append-only gate history.
    fn history(&self) -> Vec<GateEvaluation> {
        self.store
            .list_gate_evaluations(self.subject.project, self.subject.workflow)
            .unwrap_or_default()
    }

    /// One reviewer's rejection count on one gate, as the store derives it.
    fn stream(&self, target: &GateKey, principal: &ExternalId) -> u32 {
        self.store
            .rejections_since_pass(
                self.subject.project,
                self.subject.workflow,
                target,
                principal,
            )
            .unwrap_or_default()
    }

    /// All four streams after one step, labelled for the evidence trail.
    fn streams(
        &self,
        label: &str,
        reviewed: &GateKey,
        coded: &GateKey,
        alpha: &ExternalId,
        beta: &ExternalId,
    ) -> Value {
        json!({
            "step": label,
            "streams": {
                "alpha@review": self.stream(reviewed, alpha),
                "beta@review": self.stream(reviewed, beta),
                "alpha@code-review": self.stream(coded, alpha),
                "beta@code-review": self.stream(coded, beta),
            },
            "task_state": self.task_state().to_string(),
        })
    }

    /// The task's lifecycle state.
    fn task_state(&self) -> TaskState {
        self.store
            .get_task(self.subject.project, self.subject.task)
            .ok()
            .flatten()
            .map_or(TaskState::Draft, |task| task.state)
    }

    /// One run's lifecycle, when the run is readable.
    fn run_lifecycle(&self, id: AgentRunId) -> Option<RunLifecycle> {
        self.store
            .get_agent_run(self.subject.project, id)
            .ok()
            .flatten()
            .map(|run| run.projection.lifecycle)
    }

    /// How many rows one table holds, read through a raw connection.
    fn count(&self, table: &str) -> i64 {
        Connection::open(&self.path)
            .and_then(|connection| {
                connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
            })
            .unwrap_or(-1)
    }

    /// Everything this realm has proved, as one canonical document.
    ///
    /// Deliberately a single value rather than a list of assertions: comparing
    /// the whole probe before and after a restart fails on any field that moved,
    /// including the ones nobody thought to check.
    fn probe(&self) -> Value {
        let workflow = self
            .store
            .get_active_task_workflow(self.subject.project, self.subject.task)
            .ok()
            .flatten();
        let states = self
            .store
            .gate_states(self.subject.project, self.subject.workflow)
            .unwrap_or_default();
        json!({
            "profile": workflow
                .as_ref()
                .map(|workflow| workflow.snapshot.definition.id.to_string()),
            "profile_version": workflow
                .as_ref()
                .map(|workflow| workflow.snapshot.definition.version.get()),
            "definition_hash": workflow
                .as_ref()
                .map(|workflow| workflow.snapshot.definition_hash.to_string()),
            "snapshot_reverifies": workflow
                .as_ref()
                .is_some_and(|workflow| workflow.snapshot.verify().is_ok()),
            "current_phase": workflow
                .as_ref()
                .map(|workflow| workflow.current_phase.to_string()),
            "revision": workflow.as_ref().map(|workflow| workflow.revision.get()),
            "gate_states": states
                .iter()
                .map(|(gate, state)| json!({ "gate": gate.as_str(), "state": state.to_string() }))
                .collect::<Vec<_>>(),
            "gate_evaluations": self.history().iter().map(evaluation_json).collect::<Vec<_>>(),
            "artifact_evidence": self.column(
                "SELECT artifact_key FROM artifact_evidence WHERE workflow_id = ?1 \
                 ORDER BY artifact_key",
            ),
            "authority_evidence": self.column(
                "SELECT rule_key || ' ' || verdict || ' ' || reason_code || ' ' || inputs_hash \
                 FROM policy_evaluations WHERE workflow_id = ?1 ORDER BY inputs_hash",
            ),
        })
    }

    /// One text column of this workflow's rows, straight off the file.
    fn column(&self, sql: &str) -> Vec<String> {
        let Ok(connection) = Connection::open(&self.path) else {
            return Vec::new();
        };
        let Ok(mut statement) = connection.prepare(sql) else {
            return Vec::new();
        };
        let Ok(rows) = statement.query_map([self.subject.workflow.to_string()], |row| {
            row.get::<_, String>(0)
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }
}

// ---------------------------------------------------------------------------
// Fixture data
// ---------------------------------------------------------------------------

/// The bundled UX profile, resolved.
///
/// # Panics
/// Panics when the bundled pack does not load or the category does not resolve.
/// Section 1 already answers for that as a criterion; here it is a broken
/// fixture.
fn ux_bundle() -> ResolvedProfileBundle {
    let pack = bundled_pack().expect("the bundled profile pack loads");
    resolve_profile(
        &pack,
        &PackCategoryKey::parse(UX_CATEGORY).expect("a legal category key"),
        at(DECIDED_AT),
    )
    .expect("the bundled UX category resolves")
}

/// Just its pinned snapshot.
fn ux_snapshot() -> ResolvedWorkProfileSnapshot {
    ux_bundle().profile
}

/// The persona scenario the bundled pack binds to that profile.
///
/// # Panics
/// Panics when the pack binds none, which would make the criterion untestable
/// against shipped data rather than false.
fn ux_persona() -> PersonaScenarioSpec {
    let pack = bundled_pack().expect("the bundled profile pack loads");
    pack.personas
        .iter()
        .find(|persona| persona.profile.as_str() == UX_CATEGORY)
        .map(|persona| persona.scenario.clone())
        .expect("the bundled pack binds a persona scenario to the UX profile")
}

/// A real team closure certificate: every declared seat run once and closed.
///
/// Obtained the only way it can be — from `certify_team_closure` over a hydrated
/// roster — so the task half of closure cannot be faked here any more than it can
/// in production.
///
/// # Panics
/// Panics when the roster will not hydrate, which is a fixture bug.
fn team_closure(resolved: &ResolvedProfileBundle) -> (TeamRunId, TeamClosureCertificate) {
    let revision = resolved
        .team
        .clone()
        .expect("the bundled UX profile pins a team");
    let team = TeamTemplateSpec::from_revision(&revision).expect("the team reads back");
    let snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION);
    let team_run_id = TeamRunId::generate();
    let rows: Vec<AgentRun> = team
        .slots
        .iter()
        .map(|slot| closed_run(team_run_id, slot.id.as_role_key()))
        .collect();

    let lease = TeamRunLease::acquire(team_run_id).expect("this is the only writer");
    let certificate = TeamRunSlots::hydrate(lease, &snapshot, &rows, &[])
        .expect("a complete roster hydrates")
        .certify_team_closure(&[])
        .expect("every declared seat closed");
    (team_run_id, certificate)
}

/// One closed attempt at one seat.
fn closed_run(team_run_id: TeamRunId, seat: &RoleKey) -> AgentRun {
    let id = AgentRunId::generate();
    AgentRun {
        id,
        project_id: ProjectId::generate(),
        team_run_id,
        parent_agent_run_id: None,
        role: seat.clone(),
        account_profile_id: None,
        binding: None,
        projection: RunProjection {
            lifecycle: RunLifecycle::Succeeded,
            desired: DesiredRunState::RunRequested,
            observed: ObservedRunState::Succeeded,
            derived: DerivedRunState::Terminal {
                outcome: TerminalOutcome::Succeeded,
            },
            last_confirmed_at: Some(at(DECIDED_AT)),
            last_cursor: None,
        },
        terminal: Some(TerminalEvidence {
            outcome: TerminalOutcome::Succeeded,
            source: TerminalEvidenceSource::RuntimeObservation {
                cursor: EventCursor::parse(7).expect("a positive cursor"),
            },
            evidence_hash: ContentHash::of(id.to_string().as_bytes()),
            closed_at: at(DECIDED_AT),
        }),
        revision: AggregateRevision::INITIAL,
        created_at: at(DECIDED_AT),
        closed_at: Some(at(DECIDED_AT)),
    }
}

// ---------------------------------------------------------------------------
// Evidence shapes and parsers
// ---------------------------------------------------------------------------

/// One stored verdict, as evidence.
fn evaluation_json(evaluation: &GateEvaluation) -> Value {
    json!({
        "gate": evaluation.gate.as_str(),
        "sequence": evaluation.sequence,
        "verdict": evaluation.verdict.to_string(),
        "resulting_state": evaluation.verdict.resulting_state().to_string(),
        "evaluator_role": evaluation.evaluator_role.as_str(),
        "reviewer_principal": evaluation
            .reviewer_principal
            .as_ref()
            .map(ExternalId::as_str),
        "evidence": evaluation
            .evidence
            .iter()
            .map(ArtifactKey::as_str)
            .collect::<Vec<_>>(),
        "policy_evaluation_id": evaluation
            .policy_evaluation_id
            .map(|id| id.to_string()),
    })
}

/// One rejection attempt, as evidence.
fn outcome_json(
    label: &str,
    run: AgentRunId,
    parent: Option<AgentRunId>,
    outcome: Result<&kontor_store::RejectionOutcome, &String>,
    task_state: TaskState,
) -> Value {
    json!({
        "attempt": label,
        "agent_run_id": run.to_string(),
        "parent_agent_run_id": parent.map(|id| id.to_string()),
        "sequence": outcome.map(|outcome| outcome.sequence).ok(),
        "rejections_since_pass": outcome.map(|outcome| outcome.rejections_since_pass).ok(),
        "parked": outcome.map(|outcome| outcome.parked.is_some()).ok(),
        "refusal": outcome.err(),
        "task_state_after": task_state.to_string(),
    })
}

/// A bounded external name.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a legal external name")
}

/// A stable external identifier.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a legal external id")
}

/// A gate key.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn gate(text: &str) -> GateKey {
    GateKey::parse(text).expect("a legal gate key")
}

/// A phase key.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn phase(text: &str) -> PhaseKey {
    PhaseKey::parse(text).expect("a legal phase key")
}

/// A role key.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn role(text: &str) -> RoleKey {
    RoleKey::parse(text).expect("a legal role key")
}

/// A small canonical document. A marker, never content and never a secret.
///
/// # Panics
/// Panics when the document will not canonicalize, which is a fixture bug.
fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&json!({ "schema_version": 1, "marker": marker }))
        .expect("a canonical document")
}
