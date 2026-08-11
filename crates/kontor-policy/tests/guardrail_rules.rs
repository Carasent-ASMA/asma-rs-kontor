//! The seven architecture rules, decided.
//!
//! Every profile in this suite is built here, in code, with names no bundled
//! pack uses (`qq.*`, `xylo.*`). That is deliberate and it is the point of half
//! these tests: if any rule recognized a seed profile id, a phase name or a role
//! name, this suite would be the thing that fails.
//!
//! The mutants these tests exist to kill:
//!
//! * a rejection counter keyed on the run instead of the principal, so a
//!   relaunch resets it and the second rejection never parks;
//! * a counter reset by `started`, `waived` or another reviewer's pass;
//! * a "first plausible candidate" worktree resolution;
//! * an approval matched on its id rather than on the action it names;
//! * a rung check that lets degraded evidence through when the gate is not one
//!   the code recognizes.

use kontor_core::id::{
    AccountProfileId, AgentRunId, ArtifactKey, CanonicalDocument, ContentHash, CurrencyCode,
    EventCursor, ExternalId, ExternalName, GateKey, GuardrailEvaluationId, ModuleKey, Money,
    PhaseKey, ProjectId, RoleKey, RuntimeKindKey, SCHEMA_VERSION, SpecVersion, TaskId,
    TaskWorkflowId, Timestamp, WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::repository::GateEvaluation;
use kontor_core::spec::{
    ArtifactContentType, ArtifactContractSpec, BudgetBounds, GateSpec, PhaseEdge, PhaseSpec,
    ResolvedWorkProfileSnapshot, RoleRef, RuntimeRoutingRef, WorkProfileSpec,
};
use kontor_core::state::GateVerdict;
use kontor_policy::model::{
    ActionDomain, ActionEffect, ActionIntent, ActorContext, ApprovalReceipt, ApprovalReceiptId,
    ApprovalScopeKind, ArtifactEvidence, ArtifactEvidenceId, AuthoritySource, EvaluationRequest,
    EvidenceRef, GuardrailRule, GuardrailRuleKey, ModuleClaim, PersonaActor, PolicyVerdict,
    ReasonCode, RequestedAction, RunContext, RuntimeObservationRef, SubjectKind, VerdictRung,
    WorkspaceEvidence,
};
use kontor_policy::{decide, evaluate, rejections_since_pass};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn now() -> Timestamp {
    at("2026-08-11T09:00:00Z")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a valid external id")
}

fn phase(text: &str) -> PhaseKey {
    PhaseKey::parse(text).expect("a valid phase key")
}

fn gate(text: &str) -> GateKey {
    GateKey::parse(text).expect("a valid gate key")
}

fn role(text: &str) -> RoleKey {
    RoleKey::parse(text).expect("a valid role key")
}

fn artifact(text: &str) -> ArtifactKey {
    ArtifactKey::parse(text).expect("a valid artifact key")
}

fn digest(marker: &str) -> ContentHash {
    ContentHash::of(marker.as_bytes())
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

/// A profile whose every name is invented for this suite.
///
/// Two gates — `qq.check` and `qq.audit` — so the "QA and audit count
/// independently" claim has two independent gates to be true about, and neither
/// is named anything a rule could recognize.
fn custom_profile(prefix: &str) -> WorkProfileSpec {
    let p = |suffix: &str| phase(&format!("{prefix}.{suffix}"));
    let g = |suffix: &str| gate(&format!("{prefix}.{suffix}"));
    let r = |suffix: &str| role(&format!("{prefix}.{suffix}"));
    let a = |suffix: &str| artifact(&format!("{prefix}.{suffix}"));
    WorkProfileSpec {
        schema_version: SCHEMA_VERSION,
        id: WorkProfileKey::parse(&format!("{prefix}.flow")).expect("a valid profile key"),
        version: SpecVersion::FIRST,
        name: name("An invented profile"),
        phases: vec![
            PhaseSpec {
                id: p("build"),
                label: name("Build"),
                required_artifacts: vec![a("output"), a("notes")],
                gates: Vec::new(),
                rejection_route: None,
            },
            PhaseSpec {
                id: p("verify"),
                label: name("Verify"),
                required_artifacts: Vec::new(),
                gates: vec![g("check"), g("audit")],
                rejection_route: Some(p("build")),
            },
            PhaseSpec {
                id: p("ship"),
                label: name("Ship"),
                required_artifacts: Vec::new(),
                gates: Vec::new(),
                rejection_route: None,
            },
        ],
        edges: vec![
            PhaseEdge {
                from: p("build"),
                to: p("verify"),
                handoff_role: None,
            },
            PhaseEdge {
                from: p("verify"),
                to: p("ship"),
                handoff_role: None,
            },
        ],
        entry_phase: p("build"),
        terminal_phases: vec![p("ship")],
        roles: vec![
            RoleRef {
                role: r("maker"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: r("reviewer"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: r("auditor"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: r("forgiver"),
                version: SpecVersion::FIRST,
            },
        ],
        skills: Vec::new(),
        team_template: None,
        artifacts: vec![
            ArtifactContractSpec {
                key: a("output"),
                label: name("Output"),
                producer_phase: p("build"),
                content_type: ArtifactContentType::Report,
                evidence_required: true,
            },
            ArtifactContractSpec {
                key: a("notes"),
                label: name("Notes"),
                producer_phase: p("build"),
                content_type: ArtifactContentType::Document,
                evidence_required: true,
            },
        ],
        gates: vec![
            GateSpec {
                id: g("check"),
                phase: p("verify"),
                evaluator_roles: vec![r("reviewer")],
                required_evidence: vec![a("output")],
                rejection_target: p("build"),
                waiver_allowed: true,
                waiver_roles: vec![r("forgiver")],
            },
            GateSpec {
                id: g("audit"),
                phase: p("verify"),
                evaluator_roles: vec![r("auditor")],
                required_evidence: vec![a("notes")],
                rejection_target: p("build"),
                waiver_allowed: false,
                waiver_roles: Vec::new(),
            },
        ],
        runtime_routing: RuntimeRoutingRef {
            runtime_kind: RuntimeKindKey::parse("qq.runtime").expect("a valid runtime key"),
            version: SpecVersion::FIRST,
        },
        budget_defaults: BudgetBounds {
            max_tokens: 1_000,
            max_commands: 10,
            max_duration_seconds: 600,
            max_cost: Money {
                minor_units: 100,
                currency: CurrencyCode::parse("NOK").expect("a valid currency"),
            },
        },
        calendar_policy: None,
        external_workflow: None,
    }
}

fn snapshot(prefix: &str) -> ResolvedWorkProfileSnapshot {
    ResolvedWorkProfileSnapshot::resolve(&custom_profile(prefix), now())
        .expect("the invented profile validates")
}

/// One request, with every field at its least interesting value.
///
/// Each test changes only what it is about, so a verdict that moves is a verdict
/// the change caused.
struct Case {
    request: EvaluationRequest,
    prefix: &'static str,
}

impl Case {
    fn new(prefix: &'static str, rule: GuardrailRuleKey) -> Self {
        let snapshot = snapshot(prefix);
        let account = AccountProfileId::generate();
        Self {
            prefix,
            request: EvaluationRequest {
                schema_version: SCHEMA_VERSION,
                rule: GuardrailRule {
                    key: rule,
                    version: SpecVersion::FIRST,
                },
                current_phase: snapshot.definition.entry_phase.clone(),
                workflow: snapshot,
                gate: None,
                actor: ActorContext {
                    account,
                    principal: external("principal-alpha"),
                    role: role(&format!("{prefix}.reviewer")),
                    verdict_rung: VerdictRung::VERDICT_THRESHOLD,
                    persona: None,
                },
                run: RunContext {
                    project_id: ProjectId::generate(),
                    task_id: TaskId::generate(),
                    workflow_id: TaskWorkflowId::generate(),
                    module: None,
                    team_run_id: None,
                    agent_run_id: Some(AgentRunId::generate()),
                    parent_agent_run_id: None,
                    pinned_account: Some(account),
                    recorded_worktree: None,
                    requested_action: RequestedAction {
                        domain: ActionDomain::ControlPlane,
                        intent: ActionIntent::Inspect,
                        effect: ActionEffect::Read,
                        operation: name("inspect"),
                        target: external("target-1"),
                        digest: digest("action-1"),
                        dry_run_supported: false,
                        dry_run: false,
                    },
                    rule_set_revision: SpecVersion::FIRST,
                },
                workspace: WorkspaceEvidence::default(),
                artifacts: Vec::new(),
                approval: None,
                prior_gate_evaluations: Vec::new(),
                terminal_observation: None,
                evaluated_at: now(),
            },
        }
    }

    fn gate(&self, suffix: &str) -> GateKey {
        gate(&format!("{}.{suffix}", self.prefix))
    }

    fn role(&self, suffix: &str) -> RoleKey {
        role(&format!("{}.{suffix}", self.prefix))
    }

    fn artifact(&self, suffix: &str) -> ArtifactKey {
        artifact(&format!("{}.{suffix}", self.prefix))
    }

    fn phase(&self, suffix: &str) -> PhaseKey {
        phase(&format!("{}.{suffix}", self.prefix))
    }

    fn verdict(&self) -> (PolicyVerdict, ReasonCode) {
        let decision = decide(&self.request).expect("the request decides");
        (decision.verdict, decision.reason_code)
    }

    fn evidence(&self) -> Vec<EvidenceRef> {
        decide(&self.request)
            .expect("the request decides")
            .evidence_refs
    }
}

/// One recorded gate verdict, as the store would have stored it.
fn verdict_row(
    gate: &GateKey,
    sequence: u32,
    verdict: GateVerdict,
    principal: Option<&str>,
    run: Option<AgentRunId>,
) -> GateEvaluation {
    GateEvaluation {
        project_id: ProjectId::generate(),
        workflow_id: TaskWorkflowId::generate(),
        gate: gate.clone(),
        sequence,
        verdict,
        evaluator_role: role("qq.reviewer"),
        evaluator_account: AccountProfileId::generate(),
        evidence: Vec::new(),
        agent_run_id: run,
        reviewer_principal: principal.map(external),
        policy_evaluation_id: None,
        recorded_at: now(),
    }
}

// ---------------------------------------------------------------------------
// Determinism and name-freedom
// ---------------------------------------------------------------------------

#[test]
fn every_rule_is_deterministic_for_identical_canonical_inputs() {
    for rule in GuardrailRuleKey::ALL {
        let case = Case::new("qq", *rule);
        let first = evaluate(&case.request, GuardrailEvaluationId::generate())
            .expect("the first evaluation succeeds");
        let second = evaluate(&case.request, GuardrailEvaluationId::generate())
            .expect("the second evaluation succeeds");

        assert_eq!(
            first.inputs_hash, second.inputs_hash,
            "{rule} must canonicalize identical requests identically"
        );
        assert_eq!(
            first.verdict, second.verdict,
            "{rule} verdict must be stable"
        );
        assert_eq!(
            first.reason_code, second.reason_code,
            "{rule} reason must be stable"
        );
        assert_eq!(
            first.evidence_refs, second.evidence_refs,
            "{rule} evidence must be stable"
        );
        assert_eq!(
            first.subject, second.subject,
            "{rule} must file both evaluations against the same subject"
        );
        // Only the caller-supplied identity differs, which is what keeps the
        // record immutable-per-evaluation without making the decision vary.
        assert_ne!(first.id, second.id);
        assert_eq!(first.recorded_at, second.recorded_at);
    }
}

#[test]
fn an_arbitrary_custom_profile_decides_the_same_as_any_other() {
    // Two profiles that differ in nothing but their names. A rule that
    // recognized a name would part company here.
    for rule in GuardrailRuleKey::ALL {
        let one = Case::new("qq", *rule);
        let other = Case::new("xylo", *rule);
        assert_eq!(
            one.verdict(),
            other.verdict(),
            "{rule} must not depend on profile, phase, gate or role names"
        );
    }
}

#[test]
fn a_tampered_pinned_snapshot_is_refused_before_any_rule_reads_it() {
    let mut case = Case::new("qq", GuardrailRuleKey::DegradedVerdictDenied);
    case.request.workflow.definition.name = name("Renamed after pinning");
    assert!(
        decide(&case.request).is_err(),
        "a snapshot that no longer matches its digest must not be evaluated"
    );
}

// ---------------------------------------------------------------------------
// 1. worktree_sticky
// ---------------------------------------------------------------------------

#[test]
fn the_exact_recorded_worktree_passes_and_a_different_one_parks() {
    let mut case = Case::new("qq", GuardrailRuleKey::WorktreeSticky);
    let pinned = name("/tmp/worktrees/alpha");
    case.request.run.recorded_worktree = Some(pinned.clone());
    case.request.workspace.claimed_worktree = Some(pinned.clone());
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::WorktreeMatchesPin)
    );
    assert_eq!(
        case.evidence(),
        vec![EvidenceRef::Worktree { worktree: pinned }]
    );

    case.request.workspace.claimed_worktree = Some(name("/tmp/worktrees/beta"));
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Park, ReasonCode::WorktreeMoved),
        "a run that moved tree parks; it is not quietly re-pinned"
    );
}

#[test]
fn an_ambiguous_or_absent_worktree_parks_rather_than_picking_one() {
    let mut case = Case::new("qq", GuardrailRuleKey::WorktreeSticky);
    let alpha = name("/tmp/worktrees/alpha");
    let beta = name("/tmp/worktrees/beta");

    // Nothing pinned, two candidates: exactly the case where taking the first
    // would silently bind the run to the wrong tree for its whole life.
    case.request.workspace.claimed_worktree = Some(alpha.clone());
    case.request.workspace.candidate_worktrees = vec![alpha.clone(), beta];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Park, ReasonCode::WorktreeAmbiguous)
    );

    // Nothing pinned, exactly one candidate, and it is the claimed one.
    case.request.workspace.candidate_worktrees = vec![alpha.clone()];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::WorktreeFirstClaim)
    );

    // A claim the workspace layer never offered.
    case.request.workspace.claimed_worktree = Some(name("/tmp/worktrees/gamma"));
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Park, ReasonCode::WorktreeMoved)
    );

    // No claim at all.
    case.request.workspace.claimed_worktree = None;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Park, ReasonCode::WorktreeUnclaimed)
    );
}

// ---------------------------------------------------------------------------
// 2. module_collision
// ---------------------------------------------------------------------------

#[test]
fn a_module_held_by_another_task_blocks_unless_worktrees_separate_them() {
    let mut case = Case::new("qq", GuardrailRuleKey::ModuleCollision);
    let module = ModuleKey::parse("qq.module").expect("a valid module key");
    let other_task = TaskId::generate();
    case.request.run.module = Some(module.clone());
    case.request.run.recorded_worktree = Some(name("/tmp/worktrees/alpha"));

    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ModuleFree),
        "nobody else holds it"
    );

    // Same module, same tree, another task, still live.
    case.request.workspace.module_claims = vec![ModuleClaim {
        module: module.clone(),
        task_id: other_task,
        worktree: Some(name("/tmp/worktrees/alpha")),
        in_flight: true,
    }];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::ModuleInFlight)
    );
    assert_eq!(
        case.evidence(),
        vec![EvidenceRef::ModuleClaim {
            module: module.clone(),
            task_id: other_task,
        }]
    );

    // A different tree is the whole exception.
    case.request.workspace.module_claims[0].worktree = Some(name("/tmp/worktrees/beta"));
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ModuleIsolatedByWorktree)
    );

    // A contender with no tree is not isolated from anything.
    case.request.workspace.module_claims[0].worktree = None;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::ModuleInFlight)
    );

    // A finished claim contends with nothing.
    case.request.workspace.module_claims[0].in_flight = false;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ModuleFree)
    );
}

#[test]
fn a_task_does_not_collide_with_its_own_module_claim() {
    let mut case = Case::new("qq", GuardrailRuleKey::ModuleCollision);
    let module = ModuleKey::parse("qq.module").expect("a valid module key");
    case.request.run.module = Some(module.clone());
    case.request.workspace.module_claims = vec![ModuleClaim {
        module,
        task_id: case.request.run.task_id,
        worktree: None,
        in_flight: true,
    }];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ModuleFree)
    );
}

// ---------------------------------------------------------------------------
// 3. second_rejection_parks
// ---------------------------------------------------------------------------

#[test]
fn two_rejections_by_one_principal_park_even_across_different_runs() {
    let mut case = Case::new("qq", GuardrailRuleKey::SecondRejectionParks);
    let check = case.gate("check");
    case.request.gate = Some(check.clone());
    case.request.run.requested_action.intent = ActionIntent::RecordGateRejection;

    // The first rejection warns.
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Warn, ReasonCode::FirstRejection)
    );

    // The same principal, a *different* agent run — a relaunch. Keying the
    // counter on the run instead of the principal would reset here, and the
    // work would never park.
    case.request.prior_gate_evaluations = vec![verdict_row(
        &check,
        1,
        GateVerdict::Rejected,
        Some("principal-alpha"),
        Some(AgentRunId::generate()),
    )];
    case.request.run.agent_run_id = Some(AgentRunId::generate());
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Park, ReasonCode::SecondRejectionParks)
    );
    assert_eq!(
        case.evidence(),
        vec![EvidenceRef::GateVerdict {
            gate: check,
            sequence: 1,
        }]
    );
}

#[test]
fn another_reviewer_or_another_gate_leaves_this_counter_alone() {
    let mut case = Case::new("qq", GuardrailRuleKey::SecondRejectionParks);
    let check = case.gate("check");
    let audit = case.gate("audit");
    case.request.gate = Some(check.clone());
    case.request.run.requested_action.intent = ActionIntent::RecordGateRejection;

    case.request.prior_gate_evaluations = vec![
        // Somebody else rejecting the same gate.
        verdict_row(
            &check,
            1,
            GateVerdict::Rejected,
            Some("principal-beta"),
            None,
        ),
        // This principal rejecting a different gate.
        verdict_row(
            &audit,
            1,
            GateVerdict::Rejected,
            Some("principal-alpha"),
            None,
        ),
        // A verdict attributable to nobody.
        verdict_row(&check, 2, GateVerdict::Rejected, None, None),
    ];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Warn, ReasonCode::FirstRejection),
        "only this principal's rejections on this gate count"
    );
}

#[test]
fn qa_and_audit_gates_count_independently() {
    let case = Case::new("qq", GuardrailRuleKey::SecondRejectionParks);
    let check = case.gate("check");
    let audit = case.gate("audit");
    let principal = external("principal-alpha");
    let history = vec![
        verdict_row(
            &check,
            1,
            GateVerdict::Rejected,
            Some("principal-alpha"),
            None,
        ),
        verdict_row(
            &check,
            2,
            GateVerdict::Rejected,
            Some("principal-alpha"),
            None,
        ),
        verdict_row(
            &audit,
            1,
            GateVerdict::Rejected,
            Some("principal-alpha"),
            None,
        ),
    ];
    assert_eq!(rejections_since_pass(&history, &check, &principal), 2);
    assert_eq!(rejections_since_pass(&history, &audit, &principal), 1);
}

#[test]
fn only_a_pass_by_the_same_principal_on_the_same_gate_resets_the_stream() {
    let case = Case::new("qq", GuardrailRuleKey::SecondRejectionParks);
    let check = case.gate("check");
    let alpha = external("principal-alpha");

    let rejected = verdict_row(
        &check,
        1,
        GateVerdict::Rejected,
        Some("principal-alpha"),
        None,
    );
    assert_eq!(
        rejections_since_pass(std::slice::from_ref(&rejected), &check, &alpha),
        1
    );

    // The reset.
    let passed = verdict_row(
        &check,
        2,
        GateVerdict::Passed,
        Some("principal-alpha"),
        None,
    );
    assert_eq!(
        rejections_since_pass(&[rejected.clone(), passed], &check, &alpha),
        0
    );

    // Everything that is *not* a reset. `started` reopens work, `waived`
    // forgives without accepting, `parked` defers — none of them is this
    // reviewer saying the work is now good.
    for verdict in [
        GateVerdict::Started,
        GateVerdict::Waived,
        GateVerdict::Parked,
    ] {
        let row = verdict_row(&check, 2, verdict, Some("principal-alpha"), None);
        assert_eq!(
            rejections_since_pass(&[rejected.clone(), row], &check, &alpha),
            1,
            "{verdict} must not reset a rejection stream"
        );
    }

    // Another principal's pass is not this principal's.
    let foreign_pass = verdict_row(&check, 2, GateVerdict::Passed, Some("principal-beta"), None);
    assert_eq!(
        rejections_since_pass(&[rejected, foreign_pass], &check, &alpha),
        1
    );
}

#[test]
fn a_rejection_stream_at_the_threshold_keeps_parking_without_a_new_rejection() {
    let mut case = Case::new("qq", GuardrailRuleKey::SecondRejectionParks);
    let check = case.gate("check");
    case.request.gate = Some(check.clone());
    // Not a rejection this time — merely acting at a gate this reviewer has
    // already rejected twice.
    case.request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
    case.request.prior_gate_evaluations = vec![
        verdict_row(
            &check,
            1,
            GateVerdict::Rejected,
            Some("principal-alpha"),
            None,
        ),
        verdict_row(
            &check,
            2,
            GateVerdict::Rejected,
            Some("principal-alpha"),
            None,
        ),
    ];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Park, ReasonCode::SecondRejectionParks)
    );
}

// ---------------------------------------------------------------------------
// 4. degraded_verdict_denied
// ---------------------------------------------------------------------------

#[test]
fn rung_one_cannot_write_a_verdict_on_any_gate_the_profile_declares() {
    let mut case = Case::new("qq", GuardrailRuleKey::DegradedVerdictDenied);
    case.request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
    case.request.actor.verdict_rung = VerdictRung::parse(1).expect("rung 1 is valid");

    // Both gates, including the one the fixture calls an audit — and the rule
    // reaches the same answer without knowing which is which.
    for suffix in ["check", "audit"] {
        case.request.gate = Some(case.gate(suffix));
        case.request.actor.role = case.role(if suffix == "check" {
            "reviewer"
        } else {
            "auditor"
        });
        assert_eq!(
            case.verdict(),
            (PolicyVerdict::Block, ReasonCode::VerdictRungDegraded),
            "degraded evidence must not decide {suffix}"
        );
    }

    // The same actor at full rung is admitted.
    case.request.actor.verdict_rung = VerdictRung::VERDICT_THRESHOLD;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::VerdictAuthorityHeld)
    );

    // And a degraded actor doing something that is not a verdict is untouched.
    case.request.actor.verdict_rung = VerdictRung::parse(1).expect("rung 1 is valid");
    case.request.run.requested_action.intent = ActionIntent::ProduceArtifact;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::NotApplicable)
    );
}

#[test]
fn an_unauthorized_evaluator_or_waiver_role_is_refused() {
    let mut case = Case::new("qq", GuardrailRuleKey::DegradedVerdictDenied);
    case.request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
    case.request.gate = Some(case.gate("check"));

    // The gate's own evaluator.
    case.request.actor.role = case.role("reviewer");
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::VerdictAuthorityHeld)
    );

    // The gate's own waiver authority, which the profile declares separately.
    case.request.actor.role = case.role("forgiver");
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::VerdictAuthorityHeld)
    );

    // The other gate's evaluator has no authority here.
    case.request.actor.role = case.role("auditor");
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::RoleNotAuthorized)
    );

    // A waiver role on a gate that forbids waiving is nobody's authority: the
    // audit gate declares `waiver_allowed = false`.
    case.request.gate = Some(case.gate("audit"));
    case.request.actor.role = case.role("forgiver");
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::RoleNotAuthorized)
    );
}

#[test]
fn a_simulated_persona_cannot_decide_any_gate_least_of_all_its_own() {
    let mut case = Case::new("qq", GuardrailRuleKey::DegradedVerdictDenied);
    case.request.run.requested_action.intent = ActionIntent::RecordGateVerdict;
    case.request.actor.persona = Some(PersonaActor {
        persona: kontor_core::id::PersonaKey::parse("qq.persona").expect("a valid persona key"),
        gate_under_test: case.gate("check"),
        actor_role: case.role("maker"),
    });

    case.request.gate = Some(case.gate("check"));
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::PersonaSelfApproval),
        "a persona must never sign off the gate it is under test for"
    );

    case.request.gate = Some(case.gate("audit"));
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::PersonaCannotEvaluate),
        "a persona produces evidence, never verdicts"
    );
}

// ---------------------------------------------------------------------------
// 5. destructive_requires_approval
// ---------------------------------------------------------------------------

fn approval_for(case: &Case) -> ApprovalReceipt {
    let action = &case.request.run.requested_action;
    ApprovalReceipt {
        id: ApprovalReceiptId::generate(),
        scope_kind: ApprovalScopeKind::Task,
        project_id: case.request.run.project_id,
        task_id: Some(case.request.run.task_id),
        action_domain: action.domain,
        action_intent: action.intent,
        action_effect: action.effect,
        action_digest: action.digest.clone(),
        approver_principal: external("principal-operator"),
        approver_role: case.role("forgiver"),
        approver_account: AccountProfileId::generate(),
        authority_source: AuthoritySource::Operator,
        evidence: document("approval"),
        issued_at: at("2026-08-11T08:00:00Z"),
        expires_at: at("2026-08-11T10:00:00Z"),
        consumed_at: None,
    }
}

fn destructive_case() -> Case {
    let mut case = Case::new("qq", GuardrailRuleKey::DestructiveRequiresApproval);
    case.request.run.requested_action = RequestedAction {
        domain: ActionDomain::Filesystem,
        intent: ActionIntent::Mutate,
        effect: ActionEffect::Destroy,
        operation: name("remove-tree"),
        target: external("/tmp/worktrees/alpha"),
        digest: digest("rm -rf alpha"),
        dry_run_supported: true,
        dry_run: false,
    };
    case
}

#[test]
fn a_destructive_action_without_an_exactly_matching_approval_is_blocked() {
    let mut case = destructive_case();

    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::ApprovalMissing)
    );

    let approval = approval_for(&case);
    case.request.approval = Some(approval.clone());
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ApprovalBound)
    );
    assert_eq!(
        case.evidence(),
        vec![EvidenceRef::Approval {
            id: approval.id,
            action_digest: approval.action_digest.clone(),
        }]
    );

    // Every binding, broken one at a time. Each must be its own refusal, so an
    // audit can tell "wrong command" from "wrong project" from "already spent".
    /// One way of breaking a binding, and the refusal it must produce.
    type Mismatch = (&'static str, fn(&mut ApprovalReceipt), ReasonCode);
    let mismatches: Vec<Mismatch> = vec![
        (
            "a different action",
            |receipt: &mut ApprovalReceipt| {
                receipt.action_digest = digest("rm -rf beta");
            },
            ReasonCode::ApprovalActionMismatch,
        ),
        (
            "a different effect",
            |receipt: &mut ApprovalReceipt| {
                receipt.action_effect = ActionEffect::Mutate;
            },
            ReasonCode::ApprovalActionMismatch,
        ),
        (
            "a different project",
            |receipt: &mut ApprovalReceipt| {
                receipt.project_id = ProjectId::generate();
            },
            ReasonCode::ApprovalScopeMismatch,
        ),
        (
            "a different task",
            |receipt: &mut ApprovalReceipt| {
                receipt.task_id = Some(TaskId::generate());
            },
            ReasonCode::ApprovalScopeMismatch,
        ),
        (
            "an already spent receipt",
            |receipt: &mut ApprovalReceipt| {
                receipt.consumed_at = Some(at("2026-08-11T08:30:00Z"));
            },
            ReasonCode::ApprovalConsumed,
        ),
        (
            "an expired receipt",
            |receipt: &mut ApprovalReceipt| {
                receipt.expires_at = at("2026-08-11T08:30:00Z");
            },
            ReasonCode::ApprovalExpired,
        ),
        (
            "advice dressed up as authority",
            |receipt: &mut ApprovalReceipt| {
                receipt.authority_source = AuthoritySource::RecoveryAdvice;
            },
            ReasonCode::ApprovalFromRecoveryAdvice,
        ),
    ];
    for (label, break_it, expected) in mismatches {
        let mut broken = approval.clone();
        break_it(&mut broken);
        case.request.approval = Some(broken);
        assert_eq!(
            case.verdict(),
            (PolicyVerdict::Block, expected),
            "{label} must not authorize this action"
        );
    }
}

#[test]
fn a_dry_run_needs_no_approval_and_a_non_destructive_action_is_untouched() {
    let mut case = destructive_case();
    case.request.run.requested_action.dry_run = true;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ActionDryRun)
    );

    case.request.run.requested_action.dry_run = false;
    case.request.run.requested_action.effect = ActionEffect::Mutate;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ActionNonDestructive)
    );
}

#[test]
fn a_destructive_action_files_its_evaluation_against_the_action_digest() {
    let case = destructive_case();
    let evaluation = evaluate(&case.request, GuardrailEvaluationId::generate())
        .expect("the evaluation succeeds");
    assert_eq!(evaluation.subject.kind, SubjectKind::Action);
    assert_eq!(
        evaluation.subject.id.as_str(),
        case.request.run.requested_action.digest.as_str()
    );
}

// ---------------------------------------------------------------------------
// 6. account_pin_required
// ---------------------------------------------------------------------------

#[test]
fn a_run_acts_as_the_account_it_was_pinned_to_or_not_at_all() {
    let mut case = Case::new("qq", GuardrailRuleKey::AccountPinRequired);
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::AccountPinMatches)
    );

    case.request.run.pinned_account = Some(AccountProfileId::generate());
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::AccountPinMismatch)
    );

    case.request.run.pinned_account = None;
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::AccountPinMissing),
        "an unpinned run must not be allowed to act as whoever happened to launch it"
    );
}

// ---------------------------------------------------------------------------
// 7. terminal_evidence_required
// ---------------------------------------------------------------------------

#[test]
fn a_phase_cannot_complete_while_an_artifact_it_declares_is_missing() {
    let mut case = Case::new("qq", GuardrailRuleKey::TerminalEvidenceRequired);
    case.request.run.requested_action.intent = ActionIntent::CompletePhase;
    case.request.current_phase = case.phase("build");

    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::ArtifactEvidenceIncomplete)
    );

    let maker = case.role("maker");
    let produced = |key: ArtifactKey| ArtifactEvidence {
        id: ArtifactEvidenceId::generate(),
        key,
        locator: document("locator"),
        producer_role: maker.clone(),
        producer_account: AccountProfileId::generate(),
        recorded_at: now(),
    };

    // One of the two declared artifacts is still not enough.
    case.request.artifacts = vec![produced(case.artifact("output"))];
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::ArtifactEvidenceIncomplete)
    );

    case.request
        .artifacts
        .push(produced(case.artifact("notes")));
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ArtifactEvidenceComplete)
    );
    assert_eq!(case.evidence().len(), 2);

    // A phase declaring nothing completes with nothing.
    case.request.current_phase = case.phase("verify");
    case.request.artifacts.clear();
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::ArtifactEvidenceComplete)
    );
}

#[test]
fn closing_a_run_requires_a_terminal_observation_of_that_run() {
    let mut case = Case::new("qq", GuardrailRuleKey::TerminalEvidenceRequired);
    case.request.run.requested_action.intent = ActionIntent::CloseRun;
    let run = case.request.run.agent_run_id.expect("the case names a run");

    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::TerminalEvidenceMissing)
    );

    // Somebody else's observation.
    case.request.terminal_observation = Some(RuntimeObservationRef {
        agent_run_id: AgentRunId::generate(),
        cursor: EventCursor::parse(7).expect("a valid cursor"),
        evidence_hash: digest("observation"),
    });
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Block, ReasonCode::TerminalEvidenceForeign)
    );

    case.request.terminal_observation = Some(RuntimeObservationRef {
        agent_run_id: run,
        cursor: EventCursor::parse(7).expect("a valid cursor"),
        evidence_hash: digest("observation"),
    });
    assert_eq!(
        case.verdict(),
        (PolicyVerdict::Pass, ReasonCode::TerminalEvidencePresent)
    );
}
