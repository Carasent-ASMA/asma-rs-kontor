//! Guardrail evidence and the parking transaction, against a real file-backed
//! database.
//!
//! The mutants this suite exists to kill:
//!
//! * a park that commits the rejection but not the closure, the task move or the
//!   episode — leaving work that looks live and is not, or parked and is not;
//! * a rejection counter that reads a run id, so two rejections across a
//!   relaunch never reach the threshold;
//! * a parked run that can be advanced, reopened or closed again;
//! * an approval spent twice, or spent on a command it was not issued for;
//! * evidence that direct SQL can update or delete.

use std::collections::BTreeMap;

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CanonicalDocument,
    CommandReceiptId, ContentHash, CredentialAlias, CurrencyCode, ExternalId, ExternalName,
    GateKey, GuardrailEvaluationId, IdempotencyKey, Money, PhaseKey, ProjectId, RoleKey,
    RuntimeKindKey, SCHEMA_VERSION, SpecVersion, TaskId, TaskWorkflowId, TeamRunId, Timestamp,
    WorkProfileKey, parse_utc_timestamp,
};
use kontor_core::repository::{
    CredentialReference, CredentialReferenceKind, NewAccountProfile, NewAgentRun,
    NewGateEvaluation, NewProject, NewTask, NewTaskWorkflow, NewTeamRun, ProjectRepository,
    RunRepository, SpecRepository, WorkflowRepository,
};
use kontor_core::spec::{
    ArtifactContentType, ArtifactContractSpec, BudgetBounds, GateSpec, PhaseEdge, PhaseSpec,
    ResolvedWorkProfileSnapshot, RoleAuthority, RoleRef, RuntimeRoutingRef, TeamRunSnapshot,
    TeamTemplateRevision, WorkProfileSpec,
};
use kontor_core::state::{GateVerdict, RunLifecycle, TaskState};
use kontor_policy::model::{
    ActionDomain, ActionEffect, ActionIntent, ApprovalReceipt, ApprovalReceiptId,
    ApprovalScopeKind, ArtifactEvidenceId, AuthoritySource, EvaluationSubject, GateWaiverId,
    GuardrailEvaluation, GuardrailRuleKey, PolicyVerdict, ReasonCode, RecoveryEpisodeId,
    RecoveryStatus, SubjectKind,
};
use kontor_policy::recovery::{RecoveryAction, RecoveryRequest};
use kontor_store::{
    EvaluationBinding, GateRejection, NewArtifactEvidence, NewGateWaiver, ParkPlan, SqliteStore,
};
use rusqlite::Connection;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn now() -> Timestamp {
    parse_utc_timestamp("2026-08-11T09:00:00Z").expect("a canonical UTC timestamp")
}

fn later(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
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

/// A profile whose names exist only here.
fn work_profile() -> WorkProfileSpec {
    WorkProfileSpec {
        schema_version: SCHEMA_VERSION,
        id: WorkProfileKey::parse("zz.guardrail").expect("a valid profile key"),
        version: SpecVersion::FIRST,
        name: name("Guardrail fixture"),
        phases: vec![
            PhaseSpec {
                id: phase("zz.build"),
                label: name("Build"),
                required_artifacts: vec![artifact("zz.output")],
                gates: Vec::new(),
                rejection_route: None,
            },
            PhaseSpec {
                id: phase("zz.verify"),
                label: name("Verify"),
                required_artifacts: Vec::new(),
                gates: vec![gate("zz.check"), gate("zz.audit")],
                rejection_route: Some(phase("zz.build")),
            },
            PhaseSpec {
                id: phase("zz.ship"),
                label: name("Ship"),
                required_artifacts: Vec::new(),
                gates: Vec::new(),
                rejection_route: None,
            },
        ],
        edges: vec![
            PhaseEdge {
                from: phase("zz.build"),
                to: phase("zz.verify"),
                handoff_role: None,
            },
            PhaseEdge {
                from: phase("zz.verify"),
                to: phase("zz.ship"),
                handoff_role: None,
            },
        ],
        entry_phase: phase("zz.build"),
        terminal_phases: vec![phase("zz.ship")],
        roles: vec![
            RoleRef {
                role: role("zz.maker"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("zz.reviewer"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("zz.auditor"),
                version: SpecVersion::FIRST,
            },
            RoleRef {
                role: role("zz.forgiver"),
                version: SpecVersion::FIRST,
            },
        ],
        skills: Vec::new(),
        team_template: None,
        artifacts: vec![ArtifactContractSpec {
            key: artifact("zz.output"),
            label: name("Output"),
            producer_phase: phase("zz.build"),
            content_type: ArtifactContentType::Report,
            evidence_required: true,
        }],
        gates: vec![
            GateSpec {
                id: gate("zz.check"),
                phase: phase("zz.verify"),
                evaluator_roles: vec![role("zz.reviewer")],
                required_evidence: vec![artifact("zz.output")],
                rejection_target: phase("zz.build"),
                waiver_allowed: true,
                waiver_roles: vec![role("zz.forgiver")],
            },
            GateSpec {
                id: gate("zz.audit"),
                phase: phase("zz.verify"),
                evaluator_roles: vec![role("zz.auditor")],
                required_evidence: vec![artifact("zz.output")],
                rejection_target: phase("zz.build"),
                waiver_allowed: false,
                waiver_roles: Vec::new(),
            },
        ],
        runtime_routing: RuntimeRoutingRef {
            runtime_kind: RuntimeKindKey::parse("zz.runtime").expect("a valid runtime key"),
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
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
    task: TaskId,
    workflow: TaskWorkflowId,
    team_run: TeamRunId,
    account: AccountProfileId,
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");

    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Guardrails"),
            root_path: name("/tmp/guardrails"),
            created_at: now(),
        })
        .expect("a project is created");

    let account = AccountProfileId::generate();
    store
        .create_account_profile(&NewAccountProfile {
            id: account,
            project_id: project,
            label: name("Reviewer account"),
            external_account_id: Some(external("acct-reviewer")),
            harness: RuntimeKindKey::parse("zz.runtime").expect("a valid runtime key"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::ConfigHome,
                alias: CredentialAlias::parse("zz-reviewer").expect("a valid alias"),
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
            snapshot,
            current_phase: phase("zz.build"),
            created_at: now(),
        })
        .expect("a workflow is created");

    let template = TeamTemplateRevision {
        template_id: kontor_core::id::TeamTemplateId::generate(),
        version: SpecVersion::FIRST,
        name: name("Guardrail team"),
        definition: document("team"),
        role_authority: vec![RoleAuthority {
            role: role("zz.reviewer"),
            may_evaluate: vec![gate("zz.check")],
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

    Fixture {
        _directory: directory,
        path,
        store,
        project,
        task,
        workflow,
        team_run,
        account,
    }
}

impl Fixture {
    fn agent_run(&self) -> AgentRunId {
        self.run_under(None)
    }

    /// A run that descends from `parent` — what a recovery follow-up dispatches.
    fn successor_of(&self, parent: AgentRunId) -> AgentRunId {
        self.run_under(Some(parent))
    }

    fn run_under(&self, parent: Option<AgentRunId>) -> AgentRunId {
        let id = AgentRunId::generate();
        self.store
            .create_agent_run(&NewAgentRun {
                id,
                project_id: self.project,
                team_run_id: self.team_run,
                parent_agent_run_id: parent,
                role: role("zz.maker"),
                account_profile_id: Some(self.account),
                binding: None,
                created_at: now(),
            })
            .expect("an agent run is created");
        id
    }

    fn binding(&self, agent_run_id: Option<AgentRunId>) -> EvaluationBinding {
        EvaluationBinding {
            project_id: self.project,
            task_id: self.task,
            workflow_id: self.workflow,
            team_run_id: Some(self.team_run),
            agent_run_id,
        }
    }

    fn raw(&self) -> Connection {
        let connection = Connection::open(&self.path).expect("a raw connection opens");
        connection
            .pragma_update(None, "foreign_keys", true)
            .expect("foreign keys can be enabled");
        connection
    }

    fn count(&self, table: &str) -> i64 {
        self.raw()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|_| panic!("`{table}` is countable"))
    }

    fn census(&self) -> BTreeMap<&'static str, i64> {
        [
            "policy_evaluations",
            "recovery_episodes",
            "recovery_steps",
            "run_park_closures",
            "task_gate_evaluations",
            "command_receipts",
        ]
        .into_iter()
        .map(|table| (table, self.count(table)))
        .collect()
    }

    fn task_state(&self) -> TaskState {
        let stored = self
            .store
            .get_task(self.project, self.task)
            .expect("the read succeeds")
            .expect("the task exists");
        stored.state
    }

    fn run_lifecycle(&self, id: AgentRunId) -> RunLifecycle {
        self.store
            .get_agent_run(self.project, id)
            .expect("the read succeeds")
            .expect("the run exists")
            .projection
            .lifecycle
    }
}

/// A `second_rejection_parks` verdict, as `kontor-policy` would have produced it.
fn park_evaluation() -> GuardrailEvaluation {
    let inputs = document("second-rejection-inputs");
    let inputs_hash = inputs.hash().clone();
    GuardrailEvaluation {
        id: GuardrailEvaluationId::generate(),
        rule_key: GuardrailRuleKey::SecondRejectionParks,
        rule_version: SpecVersion::FIRST,
        subject: EvaluationSubject {
            kind: SubjectKind::Gate,
            id: external("zz.check"),
        },
        inputs,
        inputs_hash,
        verdict: PolicyVerdict::Park,
        reason_code: ReasonCode::SecondRejectionParks,
        evidence_refs: Vec::new(),
        recorded_at: now(),
    }
}

fn park_plan(marker: &str) -> ParkPlan {
    ParkPlan {
        evaluation: park_evaluation(),
        episode_id: RecoveryEpisodeId::generate(),
        closure_receipt_id: CommandReceiptId::generate(),
        closure_key: IdempotencyKey::parse(&format!("park-{marker}")).expect("a valid key"),
        closure_intent: document(&format!("parked-auto-triage-{marker}")),
    }
}

fn rejection(
    fixture: &Fixture,
    principal: &str,
    gate_key: &str,
    run: Option<AgentRunId>,
    marker: &str,
) -> GateRejection {
    GateRejection {
        project_id: fixture.project,
        workflow_id: fixture.workflow,
        gate: gate(gate_key),
        evaluator_role: role(if gate_key == "zz.check" {
            "zz.reviewer"
        } else {
            "zz.auditor"
        }),
        evaluator_account: fixture.account,
        reviewer_principal: external(principal),
        agent_run_id: run,
        evidence: Vec::new(),
        recorded_at: now(),
        park: park_plan(marker),
    }
}

// ---------------------------------------------------------------------------
// Parking
// ---------------------------------------------------------------------------

#[test]
fn two_rejections_by_one_principal_park_the_task_even_across_relaunches() {
    let fixture = fixture();
    let first_run = fixture.agent_run();

    let first = fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(first_run),
            "one",
        ))
        .expect("the first rejection is recorded");
    assert_eq!(first.sequence, 1);
    assert_eq!(first.rejections_since_pass, 1);
    assert!(first.parked.is_none(), "one rejection does not park");
    assert_eq!(fixture.task_state(), TaskState::InProgress);
    assert_eq!(fixture.count("recovery_episodes"), 0);

    // The builder is relaunched: a brand new agent run, the same reviewer. A
    // counter keyed on the run would reset here and the work would never park.
    let second_run = fixture.agent_run();
    let outcome = fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(second_run),
            "two",
        ))
        .expect("the second rejection is recorded");
    assert_eq!(outcome.rejections_since_pass, 2);
    let parked = outcome.parked.expect("the second rejection parks");

    // Everything the park promised, in one committed unit.
    assert_eq!(parked.parked_agent_run_id, second_run);
    assert_eq!(fixture.task_state(), TaskState::Parked);
    assert_eq!(fixture.run_lifecycle(second_run), RunLifecycle::Parked);
    assert_eq!(fixture.count("policy_evaluations"), 1);
    assert_eq!(fixture.count("run_park_closures"), 1);

    let episode = fixture
        .store
        .get_recovery_episode(fixture.project, parked.episode_id)
        .expect("the read succeeds")
        .expect("the park opened an episode");
    assert_eq!(episode.status, RecoveryStatus::Open);
    assert_eq!(episode.parked_agent_run_id, second_run);
    assert_eq!(episode.cause_evaluation_id, parked.evaluation_id);
    assert_eq!(episode.effective_followups, 0);
    assert!(!episode.advisor_used);
    assert!(!episode.committee_used);

    // The rejection that caused it is linked to the evaluation, so the two are
    // one record rather than two rows sharing a timestamp.
    let linked: String = fixture
        .raw()
        .query_row(
            "SELECT policy_evaluation_id FROM task_gate_evaluations
             WHERE workflow_id = ?1 AND gate_key = 'zz.check' AND sequence = 2",
            rusqlite::params![fixture.workflow.to_string()],
            |row| row.get(0),
        )
        .expect("the rejection row is readable");
    assert_eq!(linked, parked.evaluation_id.to_string());
}

#[test]
fn a_parked_run_is_terminal_and_never_reopens() {
    let fixture = fixture();
    let run = fixture.agent_run();
    for marker in ["one", "two"] {
        fixture
            .store
            .record_gate_rejection(&rejection(
                &fixture,
                "principal-alpha",
                "zz.check",
                Some(run),
                marker,
            ))
            .expect("the rejection is recorded");
    }
    assert_eq!(fixture.task_state(), TaskState::Parked);

    // Direct SQL cannot walk it back.
    let connection = fixture.raw();
    assert!(
        connection
            .execute(
                "UPDATE agent_runs SET lifecycle = 'running' WHERE id = ?1",
                rusqlite::params![run.to_string()],
            )
            .is_err(),
        "a closed run must not reopen"
    );

    // Nor can a third rejection close it a second time.
    let error = fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(run),
            "three",
        ))
        .expect_err("a closed run cannot be parked again");
    let _ = error;
    assert_eq!(
        fixture.count("recovery_episodes"),
        1,
        "the refused park opened no second episode"
    );
    assert_eq!(fixture.count("run_park_closures"), 1);
}

#[test]
fn another_reviewer_and_another_gate_keep_their_own_counters() {
    let fixture = fixture();
    let run = fixture.agent_run();

    // One rejection each from two principals on the check gate, and one from
    // the first principal on the audit gate. Nothing should park.
    for (principal, gate_key, marker) in [
        ("principal-alpha", "zz.check", "a"),
        ("principal-beta", "zz.check", "b"),
        ("principal-alpha", "zz.audit", "c"),
    ] {
        let outcome = fixture
            .store
            .record_gate_rejection(&rejection(&fixture, principal, gate_key, Some(run), marker))
            .expect("the rejection is recorded");
        assert_eq!(
            outcome.rejections_since_pass, 1,
            "{principal} on {gate_key} is on their first rejection"
        );
        assert!(outcome.parked.is_none());
    }
    assert_eq!(fixture.task_state(), TaskState::InProgress);
    assert_eq!(fixture.count("recovery_episodes"), 0);

    assert_eq!(
        fixture
            .store
            .rejections_since_pass(
                fixture.project,
                fixture.workflow,
                &gate("zz.check"),
                &external("principal-alpha")
            )
            .expect("the count reads"),
        1
    );
    assert_eq!(
        fixture
            .store
            .rejections_since_pass(
                fixture.project,
                fixture.workflow,
                &gate("zz.audit"),
                &external("principal-alpha")
            )
            .expect("the count reads"),
        1,
        "the audit gate counts independently of the check gate"
    );
}

#[test]
fn a_pass_resets_only_its_own_reviewer_and_gate_stream() {
    let fixture = fixture();
    let run = fixture.agent_run();
    fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(run),
            "a",
        ))
        .expect("the rejection is recorded");

    // The same reviewer passes the same gate.
    fixture
        .store
        .append_gate_evaluation(&NewGateEvaluation {
            project_id: fixture.project,
            workflow_id: fixture.workflow,
            gate: gate("zz.check"),
            verdict: GateVerdict::Passed,
            evaluator_role: role("zz.reviewer"),
            evaluator_account: fixture.account,
            evidence: vec![artifact("zz.output")],
            agent_run_id: Some(run),
            reviewer_principal: Some(external("principal-alpha")),
            policy_evaluation_id: None,
            recorded_at: now(),
        })
        .expect("the pass is recorded");
    assert_eq!(
        fixture
            .store
            .rejections_since_pass(
                fixture.project,
                fixture.workflow,
                &gate("zz.check"),
                &external("principal-alpha")
            )
            .expect("the count reads"),
        0
    );

    // So the next rejection is a first one again, and does not park.
    let outcome = fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(run),
            "b",
        ))
        .expect("the rejection is recorded");
    assert_eq!(outcome.rejections_since_pass, 1);
    assert!(outcome.parked.is_none());
    assert_eq!(fixture.task_state(), TaskState::InProgress);
}

#[test]
fn a_started_or_waived_verdict_does_not_reset_a_rejection_stream() {
    let fixture = fixture();
    let run = fixture.agent_run();
    fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(run),
            "a",
        ))
        .expect("the rejection is recorded");

    fixture
        .store
        .append_gate_evaluation(&NewGateEvaluation {
            project_id: fixture.project,
            workflow_id: fixture.workflow,
            gate: gate("zz.check"),
            verdict: GateVerdict::Started,
            evaluator_role: role("zz.reviewer"),
            evaluator_account: fixture.account,
            evidence: Vec::new(),
            agent_run_id: Some(run),
            reviewer_principal: Some(external("principal-alpha")),
            policy_evaluation_id: None,
            recorded_at: now(),
        })
        .expect("starting the gate again is recorded");

    let outcome = fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(run),
            "b",
        ))
        .expect("the rejection is recorded");
    assert_eq!(
        outcome.rejections_since_pass, 2,
        "reopening the gate is not the reviewer accepting the work"
    );
    assert!(outcome.parked.is_some());
}

#[test]
fn an_unauthorized_role_cannot_reject_and_leaves_nothing_behind() {
    let fixture = fixture();
    let run = fixture.agent_run();
    let before = fixture.census();

    let mut request = rejection(&fixture, "principal-alpha", "zz.check", Some(run), "a");
    // The audit gate's evaluator has no authority over the check gate.
    request.evaluator_role = role("zz.auditor");
    fixture
        .store
        .record_gate_rejection(&request)
        .expect_err("an unauthorized role cannot reject");
    assert_eq!(
        fixture.census(),
        before,
        "a refused rejection wrote nothing"
    );
}

#[test]
fn a_park_plan_that_is_not_a_park_verdict_is_refused_and_rolls_the_rejection_back() {
    let fixture = fixture();
    let run = fixture.agent_run();
    fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(run),
            "a",
        ))
        .expect("the first rejection is recorded");
    let before = fixture.census();

    let mut request = rejection(&fixture, "principal-alpha", "zz.check", Some(run), "b");
    request.park.evaluation.verdict = PolicyVerdict::Warn;
    fixture
        .store
        .record_gate_rejection(&request)
        .expect_err("a park needs a park verdict");

    assert_eq!(
        fixture.census(),
        before,
        "the rejection rolled back with the park it could not complete"
    );
    assert_eq!(fixture.task_state(), TaskState::InProgress);
    assert_eq!(fixture.run_lifecycle(run), RunLifecycle::Queued);
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

/// Park a task and return its episode.
fn parked_episode(fixture: &Fixture) -> (AgentRunId, RecoveryEpisodeId) {
    let run = fixture.agent_run();
    let mut episode = None;
    for marker in ["one", "two"] {
        episode = fixture
            .store
            .record_gate_rejection(&rejection(
                fixture,
                "principal-alpha",
                "zz.check",
                Some(run),
                marker,
            ))
            .expect("the rejection is recorded")
            .parked
            .map(|parked| parked.episode_id);
    }
    (run, episode.expect("the second rejection parked"))
}

fn step(revision: AggregateRevision, action: RecoveryAction) -> RecoveryRequest {
    RecoveryRequest {
        expected_revision: revision,
        action,
        input_hash: ContentHash::of(b"input"),
        output_hash: None,
        occurred_at: later("2026-08-11T09:30:00Z"),
    }
}

#[test]
fn recovery_runs_as_a_linked_successor_and_never_touches_the_parked_run() {
    let fixture = fixture();
    let (parked_run, episode_id) = parked_episode(&fixture);

    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                AggregateRevision::INITIAL,
                RecoveryAction::DeterministicRepair { safe: true },
            ),
        )
        .expect("the deterministic pass runs");
    assert_eq!(episode.status, RecoveryStatus::DeterministicRepair);

    let successor = fixture.successor_of(parked_run);
    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                episode.revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor,
                },
            ),
        )
        .expect("the follow-up dispatches");
    assert_eq!(episode.effective_followups, 1);
    assert_eq!(episode.successor_agent_run_id, Some(successor));

    // The parked run is still exactly as the park left it.
    assert_ne!(successor, parked_run);
    assert_eq!(fixture.run_lifecycle(parked_run), RunLifecycle::Parked);

    let steps = fixture
        .store
        .list_recovery_steps(fixture.project, episode_id)
        .expect("the steps read");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[1].agent_run_id, Some(successor));
}

#[test]
fn a_replayed_recovery_step_produces_no_second_effect() {
    let fixture = fixture();
    let (_, episode_id) = parked_episode(&fixture);
    let stale = step(
        AggregateRevision::INITIAL,
        RecoveryAction::DeterministicRepair { safe: true },
    );

    fixture
        .store
        .apply_recovery_transition(fixture.project, episode_id, &stale)
        .expect("the first application succeeds");
    fixture
        .store
        .apply_recovery_transition(fixture.project, episode_id, &stale)
        .expect_err("a replayed step is refused");

    assert_eq!(
        fixture
            .store
            .list_recovery_steps(fixture.project, episode_id)
            .expect("the steps read")
            .len(),
        1,
        "the refused replay appended nothing"
    );
}

#[test]
fn the_advisor_and_committee_budgets_hold_against_the_database() {
    let fixture = fixture();
    let (_, episode_id) = parked_episode(&fixture);

    let mut episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                AggregateRevision::INITIAL,
                RecoveryAction::DeterministicRepair { safe: true },
            ),
        )
        .expect("the deterministic pass runs");
    episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(episode.revision, RecoveryAction::Advisor),
        )
        .expect("the advisor is consulted");
    assert!(episode.advisor_used);
    // A read-only consultation launched nothing.
    assert_eq!(episode.successor_agent_run_id, None);

    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(episode.revision, RecoveryAction::Advisor),
        )
        .expect_err("the advisor budget is one");

    episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(episode.revision, RecoveryAction::Committee),
        )
        .expect("the committee is convened");
    assert!(episode.committee_used);
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(episode.revision, RecoveryAction::Committee),
        )
        .expect_err("the committee budget is one");
}

#[test]
fn only_two_dispatched_follow_ups_are_admitted() {
    let fixture = fixture();
    let (parked_run, episode_id) = parked_episode(&fixture);
    let mut episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                AggregateRevision::INITIAL,
                RecoveryAction::DeterministicRepair { safe: true },
            ),
        )
        .expect("the deterministic pass runs");

    // A refused preflight is free.
    episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                episode.revision,
                RecoveryAction::Followup {
                    dispatched: false,
                    successor: fixture.successor_of(parked_run),
                },
            ),
        )
        .expect("a refused dispatch is recorded and charged nothing");
    assert_eq!(episode.effective_followups, 0);

    for expected in 1..=2 {
        // Each dispatch gets its own successor, chained off the run before it.
        let parent = episode.successor_agent_run_id.unwrap_or(parked_run);
        episode = fixture
            .store
            .apply_recovery_transition(
                fixture.project,
                episode_id,
                &step(
                    episode.revision,
                    RecoveryAction::Followup {
                        dispatched: true,
                        successor: fixture.successor_of(parent),
                    },
                ),
            )
            .expect("the follow-up dispatches");
        assert_eq!(episode.effective_followups, expected);
    }

    let parent = episode.successor_agent_run_id.expect("two were dispatched");
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                episode.revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor: fixture.successor_of(parent),
                },
            ),
        )
        .expect_err("the follow-up budget is two");
    assert_eq!(
        fixture
            .store
            .get_recovery_episode(fixture.project, episode_id)
            .expect("the read succeeds")
            .expect("the episode exists")
            .effective_followups,
        2
    );
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

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
        approver_role: role("zz.forgiver"),
        approver_account: fixture.account,
        authority_source: AuthoritySource::Operator,
        evidence: document("approval"),
        issued_at: now(),
        expires_at: later("2026-08-11T10:00:00Z"),
        consumed_at: None,
    }
}

#[test]
fn an_approval_is_spent_once_and_only_on_the_action_it_names() {
    let fixture = fixture();
    let digest = ContentHash::of(b"rm -rf alpha");
    let receipt = approval(&fixture, digest.clone());
    fixture
        .store
        .issue_approval_receipt(&receipt)
        .expect("the approval is stored");

    // A second approval for the same action is impossible, so a caller cannot
    // mint a fresh receipt to get around a spent one.
    let duplicate = approval(&fixture, digest.clone());
    fixture
        .store
        .issue_approval_receipt(&duplicate)
        .expect_err("one action has one approval");

    // The wrong command cannot spend it.
    fixture
        .store
        .consume_approval_receipt(
            fixture.project,
            receipt.id,
            &ContentHash::of(b"rm -rf beta"),
            now(),
        )
        .expect_err("an approval is bound to its own action");

    fixture
        .store
        .consume_approval_receipt(fixture.project, receipt.id, &digest, now())
        .expect("the approval is spent");
    fixture
        .store
        .consume_approval_receipt(fixture.project, receipt.id, &digest, now())
        .expect_err("an approval is spent once");
}

#[test]
fn an_expired_approval_cannot_be_spent() {
    let fixture = fixture();
    let digest = ContentHash::of(b"rm -rf gamma");
    let receipt = approval(&fixture, digest.clone());
    fixture
        .store
        .issue_approval_receipt(&receipt)
        .expect("the approval is stored");
    fixture
        .store
        .consume_approval_receipt(
            fixture.project,
            receipt.id,
            &digest,
            later("2026-08-11T11:00:00Z"),
        )
        .expect_err("an expired approval authorizes nothing");
}

#[test]
fn recovery_advice_can_never_be_stored_as_an_approval() {
    let fixture = fixture();
    let mut receipt = approval(&fixture, ContentHash::of(b"rm -rf delta"));
    receipt.authority_source = AuthoritySource::RecoveryAdvice;
    fixture
        .store
        .issue_approval_receipt(&receipt)
        .expect_err("advice is not authority");
    assert_eq!(fixture.count("approval_receipts"), 0);

    // And the schema refuses it too, without the Rust layer's help.
    assert!(
        fixture
            .raw()
            .execute(
                "INSERT INTO approval_receipts
                     (id, project_id, scope_kind, task_id, action_domain, action_intent,
                      action_effect, action_digest, approver_principal, approver_role,
                      approver_account, authority_source, evidence, evidence_hash,
                      issued_at, expires_at, consumed_at)
                 VALUES (?1, ?2, 'project', NULL, 'filesystem', 'mutate', 'destroy', ?3,
                         'p', 'r', ?4, 'recovery_advice', '{}', ?5,
                         '2026-08-11T09:00:00Z', '2026-08-11T10:00:00Z', NULL)",
                rusqlite::params![
                    kontor_core::id::ProjectId::generate().to_string(),
                    fixture.project.to_string(),
                    ContentHash::of(b"x").as_str(),
                    fixture.account.to_string(),
                    ContentHash::of(b"{}").as_str()
                ],
            )
            .is_err()
    );
}

// ---------------------------------------------------------------------------
// Evidence immutability
// ---------------------------------------------------------------------------

#[test]
fn stored_evidence_refuses_update_and_delete_from_direct_sql() {
    let fixture = fixture();
    let run = fixture.agent_run();

    let evaluation = park_evaluation();
    fixture
        .store
        .append_policy_evaluation(&fixture.binding(Some(run)), &evaluation)
        .expect("the evaluation is recorded");

    fixture
        .store
        .record_artifact_evidence(&NewArtifactEvidence {
            id: ArtifactEvidenceId::generate(),
            binding: fixture.binding(Some(run)),
            key: artifact("zz.output"),
            locator: document("locator"),
            producer_role: role("zz.maker"),
            producer_account: fixture.account,
            recorded_at: now(),
        })
        .expect("the artifact evidence is recorded");

    let connection = fixture.raw();
    for table in ["policy_evaluations", "artifact_evidence"] {
        assert!(
            connection
                .execute(
                    &format!("UPDATE {table} SET recorded_at = '2026-01-01T00:00:00Z'"),
                    []
                )
                .is_err(),
            "{table} must refuse UPDATE"
        );
        assert!(
            connection
                .execute(&format!("DELETE FROM {table}"), [])
                .is_err(),
            "{table} must refuse DELETE"
        );
    }
}

#[test]
fn a_waiver_receipt_must_name_a_real_waiver() {
    let fixture = fixture();

    // A waiver receipt for a gate evaluation that is a rejection, not a waiver.
    fixture
        .store
        .record_gate_rejection(&rejection(
            &fixture,
            "principal-alpha",
            "zz.check",
            Some(fixture.agent_run()),
            "a",
        ))
        .expect("the rejection is recorded");
    let receipt = |sequence: u32| NewGateWaiver {
        id: GateWaiverId::generate(),
        project_id: fixture.project,
        workflow_id: fixture.workflow,
        gate: gate("zz.check"),
        sequence,
        authorizing_role: role("zz.forgiver"),
        authorizing_account: fixture.account,
        reason: "The dependency ships next week".to_owned(),
        evidence: document("waiver"),
        recorded_at: now(),
    };
    fixture
        .store
        .record_gate_waiver(&receipt(1))
        .expect_err("a rejection is not a waiver");

    // A real waiver, and its receipt.
    fixture
        .store
        .append_gate_evaluation(&NewGateEvaluation {
            project_id: fixture.project,
            workflow_id: fixture.workflow,
            gate: gate("zz.check"),
            verdict: GateVerdict::Waived,
            evaluator_role: role("zz.forgiver"),
            evaluator_account: fixture.account,
            evidence: vec![artifact("zz.output")],
            agent_run_id: None,
            reviewer_principal: Some(external("principal-forgiver")),
            policy_evaluation_id: None,
            recorded_at: now(),
        })
        .expect("the waiver is recorded");
    fixture
        .store
        .record_gate_waiver(&receipt(2))
        .expect("the waiver receipt is recorded");

    // One waiver collects one authority.
    fixture
        .store
        .record_gate_waiver(&receipt(2))
        .expect_err("a waiver has exactly one authority receipt");
}

// ---------------------------------------------------------------------------
// Episode advances are bound to the append-only step history
// ---------------------------------------------------------------------------

#[test]
fn an_episode_cannot_advance_without_appending_the_step_that_caused_it() {
    let fixture = fixture();
    let (_, episode_id) = parked_episode(&fixture);
    let connection = fixture.raw();

    // The whole point: closing an episode by hand, with nothing on record
    // explaining why. Both terminal outcomes, because an unaccounted-for
    // `recovered` and an unaccounted-for `needs_human` are the same defect.
    for (status, cause) in [
        ("recovered", None),
        ("needs_human", Some("committee_disagreement")),
    ] {
        assert!(
            connection
                .execute(
                    "UPDATE recovery_episodes
                     SET status = ?1, escalation_cause = ?2, closed_at = '2026-08-11T10:00:00Z',
                         revision = revision + 1
                     WHERE id = ?3",
                    rusqlite::params![status, cause, episode_id.to_string()],
                )
                .is_err(),
            "an episode must not reach `{status}` without a step accounting for it"
        );
    }

    // Nor can a status move sideways without one.
    assert!(
        connection
            .execute(
                "UPDATE recovery_episodes SET status = 'followup', effective_followups = 1,
                     revision = revision + 1 WHERE id = ?1",
                rusqlite::params![episode_id.to_string()],
            )
            .is_err(),
        "a budget must not be spent without a step accounting for it"
    );

    // Deleting the episode outright is refused too, so "no record" cannot be
    // reached from the other direction.
    assert!(
        connection
            .execute(
                "DELETE FROM recovery_episodes WHERE id = ?1",
                rusqlite::params![episode_id.to_string()],
            )
            .is_err(),
        "recovery episodes are not deletable"
    );

    // The episode is exactly as the park left it.
    let episode = fixture
        .store
        .get_recovery_episode(fixture.project, episode_id)
        .expect("the read succeeds")
        .expect("the episode exists");
    assert_eq!(episode.status, RecoveryStatus::Open);
    assert_eq!(episode.revision, AggregateRevision::INITIAL);
    assert_eq!(episode.closed_at, None);
    assert_eq!(
        fixture
            .store
            .list_recovery_steps(fixture.project, episode_id)
            .expect("the steps read")
            .len(),
        0
    );

    // And the store service's own path still works, because it appends the step
    // first: the guard binds an advance to its step, it does not freeze the row.
    let advanced = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                AggregateRevision::INITIAL,
                RecoveryAction::DeterministicRepair { safe: true },
            ),
        )
        .expect("the store service advances the episode with its step");
    assert_eq!(advanced.status, RecoveryStatus::DeterministicRepair);
    assert_eq!(
        fixture
            .store
            .list_recovery_steps(fixture.project, episode_id)
            .expect("the steps read")
            .len(),
        1
    );
}

#[test]
fn a_step_appended_without_its_advance_does_not_close_the_episode_either() {
    let fixture = fixture();
    let (_, episode_id) = parked_episode(&fixture);

    // The mirror image of the test above: a hand-written step is not itself an
    // advance. The episode still says `open`, so the two halves have to arrive
    // together — which is exactly what the store service's transaction does.
    fixture
        .raw()
        .execute(
            "INSERT INTO recovery_steps
                 (project_id, episode_id, sequence, kind, input_hash, output_hash,
                  agent_run_id, policy_evaluation_id, artifact_evidence_id, recorded_at)
             VALUES (?1, ?2, 1, 'escalation', ?3, NULL, NULL, NULL, NULL,
                     '2026-08-11T10:00:00Z')",
            rusqlite::params![
                fixture.project.to_string(),
                episode_id.to_string(),
                ContentHash::of(b"forged").as_str()
            ],
        )
        .expect("a step row inserts");

    let episode = fixture
        .store
        .get_recovery_episode(fixture.project, episode_id)
        .expect("the read succeeds")
        .expect("the episode exists");
    assert_eq!(episode.status, RecoveryStatus::Open);
    assert_eq!(episode.escalation_cause, None);
    assert_eq!(episode.closed_at, None);
}

// ---------------------------------------------------------------------------
// Follow-up successor uniqueness and lineage
// ---------------------------------------------------------------------------

/// Park a task, run the deterministic pass, and hand back what a follow-up needs.
fn ready_for_followup(fixture: &Fixture) -> (AgentRunId, RecoveryEpisodeId, AggregateRevision) {
    let (parked_run, episode_id) = parked_episode(fixture);
    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                AggregateRevision::INITIAL,
                RecoveryAction::DeterministicRepair { safe: true },
            ),
        )
        .expect("the deterministic pass runs");
    (parked_run, episode_id, episode.revision)
}

#[test]
fn a_second_follow_up_may_not_reuse_the_successor_the_first_one_dispatched() {
    let fixture = fixture();
    let (parked_run, episode_id, revision) = ready_for_followup(&fixture);

    let successor = fixture.successor_of(parked_run);
    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor,
                },
            ),
        )
        .expect("the first follow-up dispatches");
    assert_eq!(episode.effective_followups, 1);

    // The same run again. Two ledger entries for one session is the defect:
    // "two follow-ups were tried" would be a claim about work that only
    // happened once.
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                episode.revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor,
                },
            ),
        )
        .expect_err("a follow-up may not reuse a successor already dispatched");

    let after = fixture
        .store
        .get_recovery_episode(fixture.project, episode_id)
        .expect("the read succeeds")
        .expect("the episode exists");
    assert_eq!(
        after.effective_followups, 1,
        "the refused reuse spent nothing"
    );
    assert_eq!(after.revision, episode.revision, "and moved nothing");
    assert_eq!(
        fixture
            .store
            .list_recovery_steps(fixture.project, episode_id)
            .expect("the steps read")
            .len(),
        2,
        "the refused reuse appended no step"
    );

    // A genuinely distinct successor, chained off the first, is admitted — so
    // the rule refuses reuse rather than the second follow-up itself.
    let second = fixture.successor_of(successor);
    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                episode.revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor: second,
                },
            ),
        )
        .expect("a distinct linked successor is admitted");
    assert_eq!(episode.effective_followups, 2);

    let steps = fixture
        .store
        .list_recovery_steps(fixture.project, episode_id)
        .expect("the steps read");
    let dispatched: Vec<_> = steps.iter().filter_map(|s| s.agent_run_id).collect();
    assert_eq!(dispatched, vec![successor, second]);
}

#[test]
fn a_successor_whose_lineage_does_not_reach_the_parked_run_is_refused() {
    let fixture = fixture();
    let (parked_run, episode_id, revision) = ready_for_followup(&fixture);

    // A run with no parent at all: a fresh start, not a recovery of this
    // episode. Accepting it would make "linked successor" a word rather than a
    // fact.
    let orphan = fixture.agent_run();
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor: orphan,
                },
            ),
        )
        .expect_err("a parentless run is not a successor of anything");

    // A run parented on somebody else's work, so its chain never reaches the
    // parked run.
    let stranger = fixture.agent_run();
    let unrelated = fixture.successor_of(stranger);
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor: unrelated,
                },
            ),
        )
        .expect_err("a successor's lineage must lead back to the parked run");

    // A run that does not exist here at all.
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor: AgentRunId::generate(),
                },
            ),
        )
        .expect_err("a successor must exist in this project");

    // None of the three refusals moved or recorded anything.
    let after = fixture
        .store
        .get_recovery_episode(fixture.project, episode_id)
        .expect("the read succeeds")
        .expect("the episode exists");
    assert_eq!(after.effective_followups, 0);
    assert_eq!(after.successor_agent_run_id, None);
    assert_eq!(after.revision, revision);
    assert_eq!(
        fixture
            .store
            .list_recovery_steps(fixture.project, episode_id)
            .expect("the steps read")
            .len(),
        1
    );

    // The properly chained successor is admitted.
    fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor: fixture.successor_of(parked_run),
                },
            ),
        )
        .expect("a successor descending from the parked run is admitted");
}

#[test]
fn a_refused_dispatch_records_no_successor_on_its_step() {
    let fixture = fixture();
    let (parked_run, episode_id, revision) = ready_for_followup(&fixture);

    let successor = fixture.successor_of(parked_run);
    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                revision,
                RecoveryAction::Followup {
                    dispatched: true,
                    successor,
                },
            ),
        )
        .expect("the first follow-up dispatches");

    // A preflight that refused. The step is recorded — an audit needs to see it
    // was tried — but it names no run, because it ran none. Carrying the
    // episode's existing successor onto it would both misreport the attempt and
    // collide with the uniqueness the schema enforces.
    let episode = fixture
        .store
        .apply_recovery_transition(
            fixture.project,
            episode_id,
            &step(
                episode.revision,
                RecoveryAction::Followup {
                    dispatched: false,
                    successor: fixture.successor_of(successor),
                },
            ),
        )
        .expect("a refused dispatch is recorded and charged nothing");
    assert_eq!(episode.effective_followups, 1);
    assert_eq!(episode.successor_agent_run_id, Some(successor));

    let steps = fixture
        .store
        .list_recovery_steps(fixture.project, episode_id)
        .expect("the steps read");
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[2].agent_run_id, None);
}
