//! Guardrails against a real store and a real runtime adapter.
//!
//! The rule suites in `kontor-policy` prove what a verdict *is*. This one proves
//! what a verdict *does* — that a refusal reaches the runtime as nothing at all.
//! Every assertion about "no effect" is made against
//! [`ScriptedFakeRuntime::take_calls`], which records every adapter call the
//! moment it is made, so "the runtime was never asked" is checked rather than
//! assumed.
//!
//! The mutants this suite exists to kill:
//!
//! * a guardrail evaluated *after* dispatch, so a blocked destructive command
//!   has already run by the time it is refused;
//! * a park that leaves the task launchable, so the next builder starts on work
//!   that is already parked;
//! * a deterministic repair that runs again on every recovery tick.

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CanonicalDocument,
    CommandReceiptId, ContentHash, CredentialAlias, CurrencyCode, ExternalId, ExternalName,
    GateKey, GuardrailEvaluationId, IdempotencyKey, Money, PhaseKey, ProjectId, RoleKey,
    RuntimeKindKey, SCHEMA_VERSION, SpecVersion, TaskId, TaskWorkflowId, TeamRunId, Timestamp,
    WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::repository::{
    CredentialReference, CredentialReferenceKind, NewAccountProfile, NewAgentRun, NewProject,
    NewTask, NewTaskWorkflow, NewTeamRun, ProjectRepository, RunRepository, SpecRepository,
    WorkflowRepository,
};
use kontor_core::spec::{
    ArtifactContentType, ArtifactContractSpec, BudgetBounds, GateSpec, PhaseEdge, PhaseSpec,
    ResolvedWorkProfileSnapshot, RoleAuthority, RoleRef, RuntimeRoutingRef, TeamRunSnapshot,
    TeamTemplateRevision, WorkProfileSpec,
};
use kontor_core::state::TaskState;
use kontor_policy::model::{
    ActionDomain, ActionEffect, ActionIntent, ActorContext, ApprovalReceipt, ApprovalReceiptId,
    ApprovalScopeKind, AuthoritySource, EvaluationRequest, EvaluationSubject, GuardrailEvaluation,
    GuardrailRule, GuardrailRuleKey, PolicyVerdict, ReasonCode, RecoveryEpisodeId, RecoveryStatus,
    RequestedAction, RunContext, SubjectKind, VerdictRung, WorkspaceEvidence,
};
use kontor_policy::recovery::{RecoveryAction, RecoveryRequest};
use kontor_policy::{Decision, decide};
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::workspace::{WorkspaceBindingId, WorkspacePrepareRequest, WorkspaceRoot};
use kontor_runtime::{RuntimeAdapter, ScriptedFakeRuntime};
use kontor_store::{GateRejection, ParkPlan, SqliteStore};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn now() -> Timestamp {
    parse_utc_timestamp("2026-08-11T09:00:00Z").expect("a canonical UTC timestamp")
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

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 16,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

fn work_profile() -> WorkProfileSpec {
    WorkProfileSpec {
        schema_version: SCHEMA_VERSION,
        id: WorkProfileKey::parse("gg.flow").expect("a valid profile key"),
        version: SpecVersion::FIRST,
        name: name("Guardrail integration profile"),
        phases: vec![
            PhaseSpec {
                id: phase("gg.build"),
                label: name("Build"),
                required_artifacts: vec![artifact("gg.output")],
                gates: Vec::new(),
                rejection_route: None,
            },
            PhaseSpec {
                id: phase("gg.verify"),
                label: name("Verify"),
                required_artifacts: Vec::new(),
                gates: vec![gate("gg.check")],
                rejection_route: Some(phase("gg.build")),
            },
            PhaseSpec {
                id: phase("gg.ship"),
                label: name("Ship"),
                required_artifacts: Vec::new(),
                gates: Vec::new(),
                rejection_route: None,
            },
        ],
        edges: vec![
            PhaseEdge {
                from: phase("gg.build"),
                to: phase("gg.verify"),
                handoff_role: None,
            },
            PhaseEdge {
                from: phase("gg.verify"),
                to: phase("gg.ship"),
                handoff_role: None,
            },
        ],
        entry_phase: phase("gg.build"),
        terminal_phases: vec![phase("gg.ship")],
        roles: vec![
            RoleRef {
                role: role("gg.maker"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("gg.reviewer"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("gg.forgiver"),
                version: SpecVersion::FIRST,
            },
        ],
        skills: Vec::new(),
        team_template: None,
        artifacts: vec![ArtifactContractSpec {
            key: artifact("gg.output"),
            label: name("Output"),
            producer_phase: phase("gg.build"),
            content_type: ArtifactContentType::Report,
            evidence_required: true,
        }],
        gates: vec![GateSpec {
            id: gate("gg.check"),
            phase: phase("gg.verify"),
            evaluator_roles: vec![role("gg.reviewer")],
            required_evidence: vec![artifact("gg.output")],
            rejection_target: phase("gg.build"),
            waiver_allowed: true,
            waiver_roles: vec![role("gg.forgiver")],
        }],
        runtime_routing: RuntimeRoutingRef {
            runtime_kind: RuntimeKindKey::parse("gg.runtime").expect("a valid runtime key"),
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
        context_window: None,
    }
}

struct Fixture {
    _directory: TempDir,
    store: SqliteStore,
    fake: ScriptedFakeRuntime,
    project: ProjectId,
    task: TaskId,
    workflow: TaskWorkflowId,
    team_run: TeamRunId,
    account: AccountProfileId,
    snapshot: ResolvedWorkProfileSnapshot,
    worktree: ExternalName,
}

async fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");

    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Guardrail integration"),
            root_path: name("/tmp/guardrail-integration"),
            created_at: now(),
        })
        .expect("a project is created");

    let account = AccountProfileId::generate();
    store
        .create_account_profile(&NewAccountProfile {
            id: account,
            project_id: project,
            label: name("Pinned account"),
            external_account_id: Some(external("acct-1")),
            harness: RuntimeKindKey::parse("gg.runtime").expect("a valid runtime key"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::ConfigHome,
                alias: CredentialAlias::parse("gg-alpha").expect("a valid alias"),
            },
            environment: document("environment"),
            routing: document("routing"),
            capability: document("capability"),
            provider_identity: None,
            enabled: true,
            created_at: now(),
        })
        .expect("an account profile is created");

    let task = TaskId::generate();
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: None,
            title: name("A guarded task"),
            module: None,
            state: TaskState::InProgress,
            created_at: now(),
        })
        .expect("a task is created");

    let profile = work_profile();
    store
        .insert_work_profile(project, &profile)
        .expect("the profile is stored");
    let snapshot =
        ResolvedWorkProfileSnapshot::resolve(&profile, now()).expect("the profile resolves");
    let workflow = TaskWorkflowId::generate();
    store
        .create_task_workflow(&NewTaskWorkflow {
            id: workflow,
            project_id: project,
            task_id: task,
            snapshot: snapshot.clone(),
            current_phase: phase("gg.build"),
            created_at: now(),
        })
        .expect("a workflow is created");

    let template = TeamTemplateRevision {
        template_id: kontor_core::id::TeamTemplateId::generate(),
        version: SpecVersion::FIRST,
        name: name("Guardrail team"),
        definition: document("team"),
        role_authority: vec![RoleAuthority {
            role: role("gg.reviewer"),
            may_evaluate: vec![gate("gg.check")],
            may_waive: Vec::new(),
        }],
    };
    store
        .insert_team_template(project, &template)
        .expect("the template is stored");
    let team_run = TeamRunId::generate();
    store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: project,
            task_id: task,
            snapshot: TeamRunSnapshot::from_revision(&template, SCHEMA_VERSION),
            created_at: now(),
        })
        .expect("a team run is created");

    let fake = ScriptedFakeRuntime::new(capabilities());
    // The worktree the guardrail pins is a real task tree under the root the
    // runtime actually reports, not a string this test invented:
    // `worktree_sticky` is only meaningful if the two sides agree about what a
    // tree is, and the adapter refuses a workspace that is the root itself.
    let worktree = name(&format!(
        "{}/guarded-task",
        fake.runtime_root().as_str().trim_end_matches('/')
    ));
    fake.take_calls();

    Fixture {
        _directory: directory,
        store,
        fake,
        project,
        task,
        workflow,
        team_run,
        account,
        snapshot,
        worktree,
    }
}

impl Fixture {
    fn agent_run(&self) -> AgentRunId {
        let id = AgentRunId::generate();
        self.store
            .create_agent_run(&NewAgentRun {
                id,
                project_id: self.project,
                team_run_id: self.team_run,
                parent_agent_run_id: None,
                role: role("gg.maker"),
                account_profile_id: Some(self.account),
                binding: None,
                created_at: now(),
            })
            .expect("an agent run is created");
        id
    }

    /// A request with every guardrail input in its admitted position.
    fn request(&self, rule: GuardrailRuleKey, run: AgentRunId) -> EvaluationRequest {
        EvaluationRequest {
            schema_version: SCHEMA_VERSION,
            rule: GuardrailRule {
                key: rule,
                version: SpecVersion::FIRST,
            },
            workflow: self.snapshot.clone(),
            current_phase: phase("gg.build"),
            gate: None,
            actor: ActorContext {
                account: self.account,
                principal: external("principal-alpha"),
                role: role("gg.maker"),
                verdict_rung: VerdictRung::VERDICT_THRESHOLD,
                persona: None,
            },
            run: RunContext {
                project_id: self.project,
                task_id: self.task,
                workflow_id: self.workflow,
                module: None,
                team_run_id: Some(self.team_run),
                agent_run_id: Some(run),
                parent_agent_run_id: None,
                pinned_account: Some(self.account),
                recorded_worktree: Some(self.worktree.clone()),
                requested_action: RequestedAction {
                    domain: ActionDomain::Runtime,
                    intent: ActionIntent::Inspect,
                    effect: ActionEffect::Read,
                    operation: name("prepare-workspace"),
                    target: external("workspace"),
                    digest: ContentHash::of(b"prepare-workspace"),
                    dry_run_supported: false,
                    dry_run: false,
                },
                rule_set_revision: SpecVersion::FIRST,
            },
            workspace: WorkspaceEvidence {
                claimed_worktree: Some(self.worktree.clone()),
                candidate_worktrees: vec![self.worktree.clone()],
                module_claims: Vec::new(),
            },
            artifacts: Vec::new(),
            approval: None,
            prior_gate_evaluations: Vec::new(),
            terminal_observation: None,
            evaluated_at: now(),
        }
    }

    /// Every rule, then the runtime — and only if every rule admitted.
    ///
    /// This is the shape the whole guardrail layer exists to have: the adapter
    /// call is *inside* the `if`, so a refusal is not a rollback, it is an
    /// absence.
    async fn guarded_prepare_workspace(&self, request: &EvaluationRequest) -> Result<(), Decision> {
        // Persistence proof first: a destructive action is admitted only when
        // its approval receipt actually exists in `approval_receipts`,
        // unexpired and unconsumed, bound to this exact action digest. An
        // in-memory receipt object that was never persisted authorizes
        // nothing — the store is the authority, not the request.
        if request.run.requested_action.effect == ActionEffect::Destroy
            && !request.run.requested_action.dry_run
        {
            match request.approval.as_ref() {
                None => {
                    return Err(Decision::bare(
                        PolicyVerdict::Block,
                        ReasonCode::ApprovalMissing,
                    ));
                }
                Some(approval) => {
                    let persisted = self.store.verify_approval_receipt(
                        approval.project_id,
                        approval.id,
                        &approval.action_digest,
                        now(),
                    );
                    if persisted.is_err() {
                        return Err(Decision::bare(
                            PolicyVerdict::Block,
                            ReasonCode::ApprovalMissing,
                        ));
                    }
                }
            }
        }
        for rule in GuardrailRuleKey::ALL {
            let mut scoped = request.clone();
            scoped.rule = GuardrailRule {
                key: *rule,
                version: SpecVersion::FIRST,
            };
            let decision = decide(&scoped).expect("the request decides");
            if !decision.admits() {
                return Err(decision);
            }
        }
        self.fake
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id: self.team_run,
                task_id: self.task,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: WorkspaceRoot::parse(self.worktree.as_str()).expect("an absolute path"),
                requested_at: now(),
            })
            .await
            .expect("the runtime prepares the workspace");
        Ok(())
    }

    fn task_state(&self) -> TaskState {
        self.store
            .get_task(self.project, self.task)
            .expect("the read succeeds")
            .expect("the task exists")
            .state
    }
}

fn park_plan(marker: &str) -> ParkPlan {
    let inputs = document("second-rejection");
    let inputs_hash = inputs.hash().clone();
    ParkPlan {
        evaluation: GuardrailEvaluation {
            id: GuardrailEvaluationId::generate(),
            rule_key: GuardrailRuleKey::SecondRejectionParks,
            rule_version: SpecVersion::FIRST,
            subject: EvaluationSubject {
                kind: SubjectKind::Gate,
                id: external("gg.check"),
            },
            inputs,
            inputs_hash,
            verdict: PolicyVerdict::Park,
            reason_code: ReasonCode::SecondRejectionParks,
            evidence_refs: Vec::new(),
            recorded_at: now(),
        },
        episode_id: RecoveryEpisodeId::generate(),
        closure_receipt_id: CommandReceiptId::generate(),
        closure_key: IdempotencyKey::parse(&format!("park-{marker}")).expect("a valid key"),
        closure_intent: document(&format!("parked-auto-triage-{marker}")),
    }
}

fn rejection(fixture: &Fixture, run: AgentRunId, marker: &str) -> GateRejection {
    GateRejection {
        project_id: fixture.project,
        workflow_id: fixture.workflow,
        gate: gate("gg.check"),
        evaluator_role: role("gg.reviewer"),
        evaluator_account: fixture.account,
        reviewer_principal: external("principal-reviewer"),
        agent_run_id: Some(run),
        evidence: Vec::new(),
        recorded_at: now(),
        park: park_plan(marker),
    }
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_exact_recorded_worktree_is_admitted_and_reaches_the_runtime() {
    let fixture = fixture().await;
    let run = fixture.agent_run();
    let request = fixture.request(GuardrailRuleKey::WorktreeSticky, run);

    fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect("every guardrail admits the pinned worktree");
    assert_eq!(
        fixture.fake.take_calls().len(),
        1,
        "an admitted request reaches the runtime exactly once"
    );
    assert_eq!(fixture.fake.workspace_count(), 1);
}

#[tokio::test]
async fn a_wrong_or_ambiguous_worktree_parks_with_no_runtime_effect() {
    let fixture = fixture().await;
    let run = fixture.agent_run();

    // A tree that is not the pinned one.
    let mut moved = fixture.request(GuardrailRuleKey::WorktreeSticky, run);
    moved.workspace.claimed_worktree = Some(name("/w/somewhere-else"));
    let decision = fixture
        .guarded_prepare_workspace(&moved)
        .await
        .expect_err("a moved worktree is refused");
    assert_eq!(decision.verdict, PolicyVerdict::Park);
    assert_eq!(decision.reason_code, ReasonCode::WorktreeMoved);

    // Two trees could be meant, and nothing is pinned yet.
    let mut ambiguous = fixture.request(GuardrailRuleKey::WorktreeSticky, run);
    ambiguous.run.recorded_worktree = None;
    ambiguous.workspace.candidate_worktrees =
        vec![fixture.worktree.clone(), name("/w/somewhere-else")];
    let decision = fixture
        .guarded_prepare_workspace(&ambiguous)
        .await
        .expect_err("an ambiguous worktree is refused");
    assert_eq!(decision.verdict, PolicyVerdict::Park);
    assert_eq!(decision.reason_code, ReasonCode::WorktreeAmbiguous);

    assert!(
        fixture.fake.take_calls().is_empty(),
        "a parked worktree decision must never reach the runtime"
    );
    assert_eq!(fixture.fake.workspace_count(), 0);
}

#[tokio::test]
async fn a_destructive_action_without_its_approval_has_no_runtime_effect() {
    let fixture = fixture().await;
    let run = fixture.agent_run();
    let mut request = fixture.request(GuardrailRuleKey::DestructiveRequiresApproval, run);
    request.run.requested_action = RequestedAction {
        domain: ActionDomain::Filesystem,
        intent: ActionIntent::Mutate,
        effect: ActionEffect::Destroy,
        operation: name("remove-tree"),
        target: external("/w/task"),
        digest: ContentHash::of(b"rm -rf /w/task"),
        dry_run_supported: true,
        dry_run: false,
    };

    let decision = fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect_err("an unapproved destructive action is refused");
    assert_eq!(decision.verdict, PolicyVerdict::Block);
    assert_eq!(decision.reason_code, ReasonCode::ApprovalMissing);
    assert!(
        fixture.fake.take_calls().is_empty(),
        "the refusal happened before dispatch, so there is nothing to undo"
    );

    // A receipt object for another digest that was never persisted is refused
    // at the persistence proof — the store has no such approval.
    let mut wrong = approval(&fixture, ContentHash::of(b"rm -rf /w/other"));
    wrong.action_digest = ContentHash::of(b"rm -rf /w/other");
    request.approval = Some(wrong);
    let decision = fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect_err("an approval for another command authorizes nothing");
    assert_eq!(decision.verdict, PolicyVerdict::Block);
    assert!(fixture.fake.take_calls().is_empty());

    // A *persisted* receipt bound to a different digest is caught by the
    // evaluator's own binding check.
    let mut persisted_wrong = approval(&fixture, ContentHash::of(b"rm -rf /w/other"));
    persisted_wrong.action_digest = ContentHash::of(b"rm -rf /w/other");
    fixture
        .store
        .issue_approval_receipt(&persisted_wrong)
        .expect("the wrong-digest receipt is issued into the store");
    request.approval = Some(persisted_wrong);
    let decision = fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect_err("a persisted approval for another command authorizes nothing");
    assert_eq!(decision.reason_code, ReasonCode::ApprovalActionMismatch);
    assert!(fixture.fake.take_calls().is_empty());

    // A fabricated receipt — an object never written to `approval_receipts` —
    // must also be refused: persistence is the proof, not the object's shape.
    let fabricated = approval(&fixture, ContentHash::of(b"rm -rf /w/task"));
    request.approval = Some(fabricated);
    let decision = fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect_err("an unpersisted approval receipt authorizes nothing");
    assert_eq!(decision.verdict, PolicyVerdict::Block);
    assert!(fixture.fake.take_calls().is_empty());

    // The real persistence path: issue the receipt into the store, then the
    // exactly-bound action is admitted — and only once.
    let approved = approval(&fixture, ContentHash::of(b"rm -rf /w/task"));
    fixture
        .store
        .issue_approval_receipt(&approved)
        .expect("the receipt is issued into the store");
    request.approval = Some(approved);
    fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect("a persisted, exactly-bound approval admits the action");
    assert_eq!(fixture.fake.take_calls().len(), 1);
}

fn approval(fixture: &Fixture, digest: ContentHash) -> ApprovalReceipt {
    ApprovalReceipt {
        id: ApprovalReceiptId::generate(),
        scope_kind: ApprovalScopeKind::Task,
        project_id: fixture.project,
        task_id: Some(fixture.task),
        action_domain: ActionDomain::Filesystem,
        action_intent: ActionIntent::Mutate,
        action_effect: ActionEffect::Destroy,
        action_digest: digest,
        approver_principal: external("principal-operator"),
        approver_role: role("gg.forgiver"),
        approver_account: fixture.account,
        authority_source: AuthoritySource::Operator,
        evidence: document("approval"),
        issued_at: parse_utc_timestamp("2026-08-11T08:00:00Z").expect("a timestamp"),
        expires_at: parse_utc_timestamp("2026-08-11T10:00:00Z").expect("a timestamp"),
        consumed_at: None,
    }
}

#[tokio::test]
async fn two_rejections_park_before_another_builder_can_launch() {
    let fixture = fixture().await;
    let first_run = fixture.agent_run();

    fixture
        .store
        .record_gate_rejection(&rejection(&fixture, first_run, "one"))
        .expect("the first rejection is recorded");
    assert_eq!(fixture.task_state(), TaskState::InProgress);

    // The builder is relaunched — a new run, the same reviewer.
    let second_run = fixture.agent_run();
    let parked = fixture
        .store
        .record_gate_rejection(&rejection(&fixture, second_run, "two"))
        .expect("the second rejection is recorded")
        .parked
        .expect("the second rejection parks");
    assert_eq!(fixture.task_state(), TaskState::Parked);

    // A third builder now tries to start. The guardrail layer refuses it on the
    // rejection stream alone, and the runtime is never asked.
    let mut request = fixture.request(GuardrailRuleKey::SecondRejectionParks, fixture.agent_run());
    request.gate = Some(gate("gg.check"));
    request.actor.principal = external("principal-reviewer");
    request.prior_gate_evaluations = fixture
        .store
        .list_gate_evaluations(fixture.project, fixture.workflow)
        .expect("the gate history reads");

    let decision = fixture
        .guarded_prepare_workspace(&request)
        .await
        .expect_err("a parked stream admits no further work");
    assert_eq!(decision.verdict, PolicyVerdict::Park);
    assert_eq!(decision.reason_code, ReasonCode::SecondRejectionParks);
    assert!(
        fixture.fake.take_calls().is_empty(),
        "no responsible role was launched between the park and its recovery"
    );

    // And the episode the park opened is waiting, untouched.
    let episode = fixture
        .store
        .get_recovery_episode(fixture.project, parked.episode_id)
        .expect("the read succeeds")
        .expect("the park opened an episode");
    assert_eq!(episode.status, RecoveryStatus::Open);
    assert_eq!(episode.parked_agent_run_id, second_run);
}

#[tokio::test]
async fn safe_deterministic_repair_happens_once_per_episode() {
    let fixture = fixture().await;
    let run = fixture.agent_run();
    fixture
        .store
        .record_gate_rejection(&rejection(&fixture, run, "one"))
        .expect("the first rejection is recorded");
    let parked = fixture
        .store
        .record_gate_rejection(&rejection(&fixture, run, "two"))
        .expect("the second rejection is recorded")
        .parked
        .expect("the second rejection parks");

    let repair = |revision: AggregateRevision| RecoveryRequest {
        expected_revision: revision,
        action: RecoveryAction::DeterministicRepair { safe: true },
        input_hash: ContentHash::of(b"inspect"),
        output_hash: None,
        occurred_at: now(),
    };

    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            parked.episode_id,
            &repair(AggregateRevision::INITIAL),
        )
        .expect("the deterministic pass runs");
    assert_eq!(episode.status, RecoveryStatus::DeterministicRepair);

    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            parked.episode_id,
            &repair(episode.revision),
        )
        .expect_err("a second deterministic pass is not available");

    assert_eq!(
        fixture
            .store
            .list_recovery_steps(fixture.project, parked.episode_id)
            .expect("the steps read")
            .len(),
        1,
        "the repair is on record exactly once"
    );
    assert!(
        fixture.fake.take_calls().is_empty(),
        "deterministic repair inspected records, not the runtime"
    );
}
