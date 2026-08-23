//! The typed team template and the sealed role-slot lifecycle.
//!
//! The mutants this suite exists to kill:
//!
//! * accepting a duplicate slot id, or counting cardinality from live sessions
//!   rather than from declared slots;
//! * ignoring a dangling, self, duplicate or cyclic handoff, or a handoff path
//!   past the declared depth;
//! * permitting a second launch while a slot is occupied;
//! * letting a replacement launch before the old session closed with evidence;
//! * dropping `parent_agent_run_id`, or moving a successor to another slot;
//! * treating two sessions of one logical role as parallel without two slots;
//! * skipping a declared slot at team closure, or accepting an unauthorized or
//!   evidence-free waiver;
//! * rewriting a published revision instead of publishing the next one.

use std::collections::BTreeSet;

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, AggregateRevision, ArtifactKey, BoundedText, ContentHash, EventCursor, ExternalId,
    ExternalName, MiniProjectId, ProjectId, RoleKey, RuntimeBindingId, SCHEMA_VERSION, SpecVersion,
    TaskId, TeamRunId, Timestamp, parse_utc_timestamp,
};
use kontor_core::repository::AgentRun;
use kontor_core::spec::{
    ContextPolicySource, ContextWindowClass, ContextWindowPolicy, RoleContextSeed,
    TeamContextPolicySeed, TeamRunSnapshot, TeamTemplateRevision,
};
use kontor_core::state::{
    DerivedRunState, DesiredRunState, ObservedRunState, RunLifecycle, RunProjection,
    TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError, RuntimeResult};
use kontor_runtime::admission::{AdmissionOutcome, AdmissionRequest, ReplacedBinding, RoleSlotKey};
use kontor_runtime::capability::{
    RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{AdapterCall, RequestKey, ScriptStep, ScriptedFakeRuntime};
use kontor_runtime::request::{CancelRequest, LaunchParts, LaunchPlacement};
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_teams::run::{
    LaunchPermit, PreparedLaunch, RoleSlotWaiver, SlotLaunch, TeamRunLease, TeamRunSlots,
};
use kontor_teams::spec::{
    RoleSlotId, TeamTemplateSpec, bundled_teams, parse_team_pack, revise_team_template,
};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn now() -> Timestamp {
    at("2026-08-10T09:00:00Z")
}

/// The standard-fallback context policy a seat launches under when the test is
/// about something else.
fn standard_context_policy() -> kontor_core::spec::ContextPolicySnapshot {
    kontor_core::spec::ContextPolicySnapshot::standard(
        &kontor_core::spec::ContextWindowBounds::unknown(),
        true,
        kontor_core::id::SCHEMA_VERSION,
        now(),
    )
    .expect("the standard fallback freezes")
}

fn slot(text: &str) -> RoleSlotId {
    RoleSlotId::parse(text).expect("a valid slot id")
}

fn role(text: &str) -> RoleKey {
    RoleKey::parse(text).expect("a valid role key")
}

fn artifact(text: &str) -> ArtifactKey {
    ArtifactKey::parse(text).expect("a valid artifact key")
}

/// One bundled template, by name-independent position in the pack.
///
/// The tests index the pack rather than looking a seed id up, so the suite
/// itself never encodes a behavioral dependency on a seed name.
fn seed(index: usize) -> TeamTemplateSpec {
    let pack = bundled_teams().expect("the bundled team pack loads");
    pack.teams
        .get(index)
        .cloned()
        .expect("the bundled pack declares this template")
}

/// The bundled template that declares a logical role twice.
fn parallel_seed() -> TeamTemplateSpec {
    bundled_teams()
        .expect("the bundled team pack loads")
        .teams
        .into_iter()
        .find(|team| {
            team.roles
                .iter()
                .any(|requirement| requirement.min_slots > 1)
        })
        .expect("one bundled template declares a role more than once")
}

/// Exclusive ownership of one team run, for a test that owns it outright.
fn lease(team_run_id: TeamRunId) -> TeamRunLease {
    TeamRunLease::acquire(team_run_id).expect("the test is the only writer")
}

fn snapshot_of(template: &TeamTemplateSpec) -> TeamRunSnapshot {
    let revision = template.to_revision().expect("the template canonicalizes");
    TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION)
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

fn execution_scope(task_id: TaskId, root: WorkspaceRoot) -> ExecutionScope {
    ExecutionScope::for_task(
        EpicScope {
            mini_project_id: MiniProjectId::generate(),
            external_epic_key: ExternalId::parse("ASMA-TEAM").expect("epic key"),
            short_title: ExternalName::parse("Team contract").expect("epic title"),
        },
        TaskScope {
            task_id,
            external_issue_key: ExternalId::parse("ASMA-TEAM-1").expect("issue key"),
            short_code: ExternalId::parse("TEAM-1").expect("short code"),
            worktree: root,
        },
    )
}

/// One prepared workspace shared by every role of one team run.
struct Runtime {
    fake: ScriptedFakeRuntime,
    team_run_id: TeamRunId,
    task_id: TaskId,
    workspace: WorkspaceBindingSnapshot,
}

impl Runtime {
    async fn prepare(team_run_id: TeamRunId) -> Self {
        let fake = ScriptedFakeRuntime::new(capabilities());
        let task_id = TaskId::generate();
        let workspace = fake
            .prepare_workspace(&WorkspacePrepareRequest {
                scope: execution_scope(
                    task_id,
                    WorkspaceRoot::parse("/w/task-1").expect("an absolute path"),
                ),
                team_run_id,
                task_id,
                workspace_binding_id: WorkspaceBindingId::generate(),
                display_name: kontor_core::id::ExternalName::parse("TSW • ASMA-1 • TEST-1")
                    .expect("a native name"),
                root: WorkspaceRoot::parse("/w/task-1").expect("an absolute path"),
                requested_at: at("2026-08-10T08:59:00Z"),
            })
            .await
            .expect("the runtime prepares a task workspace")
            .snapshot;
        Self {
            fake,
            team_run_id,
            task_id,
            workspace,
        }
    }

    /// What a launch names, with no authorization attached.
    fn launch_parts(&self, slot: &RoleSlotId, agent_run_id: AgentRunId) -> LaunchParts {
        LaunchParts {
            scope: execution_scope(self.task_id, self.workspace.root().clone()),
            display_name: kontor_core::id::ExternalName::parse("Implement • KON-19")
                .expect("display name"),
            agent_run_id,
            team_run_id: self.team_run_id,
            role_slot_id: slot.clone(),
            task_id: self.task_id,
            binding_id: RuntimeBindingId::generate(),
            placement: Some(LaunchPlacement::Workspace(self.workspace.clone())),
            cwd: self.workspace.root().clone(),
            account_profile_id: None,
            prompt: BoundedText::parse("do the work").expect("bounded text"),
            model_rung: kontor_core::spec::ModelRung {
                provider: kontor_core::spec::ProviderRef("test".to_owned()),
                model: kontor_core::spec::ModelRef("test".to_owned()),
                effort: None,
            },
            context_policy: standard_context_policy(),
            autonomy: kontor_core::spec::SeatAutonomy::standard(),
            requested_at: now(),
        }
    }

    /// The seat, as the runtime addresses it.
    fn slot_key(&self, slot: &RoleSlotId) -> RoleSlotKey {
        RoleSlotKey::new(self.team_run_id, slot.clone())
    }

    /// Ask the runtime to admit a launch that cites nothing and replaces
    /// nothing — the plain "this seat should be free" question.
    async fn admit(&self, parts: &LaunchParts) -> RuntimeResult<AdmissionOutcome> {
        self.fake
            .admit_launch(&AdmissionRequest {
                slot: self.slot_key(&parts.role_slot_id),
                agent_run_id: parts.agent_run_id,
                binding_id: parts.binding_id,
                replaces: None,
                requested_at: parts.requested_at,
            })
            .await
    }

    fn launch_input(&self) -> SlotLaunch {
        SlotLaunch {
            scope: execution_scope(self.task_id, self.workspace.root().clone()),
            display_name: kontor_core::id::ExternalName::parse("Implement • KON-19")
                .expect("display name"),
            task_id: self.task_id,
            binding_id: RuntimeBindingId::generate(),
            placement: Some(LaunchPlacement::Workspace(self.workspace.clone())),
            cwd: self.workspace.root().clone(),
            account_profile_id: None,
            prompt: BoundedText::parse("do the work").expect("bounded text"),
            model_rung: kontor_core::spec::ModelRung {
                provider: kontor_core::spec::ProviderRef("test".to_owned()),
                model: kontor_core::spec::ModelRef("test".to_owned()),
                effort: None,
            },
            context_policy: standard_context_policy(),
            autonomy: kontor_core::spec::SeatAutonomy::standard(),
            requested_at: now(),
        }
    }

    /// Cancel a seat's session and have the runtime *observe* it end.
    ///
    /// A scripted cancellation that the runtime confirms is the only thing here
    /// that makes a session terminal in the runtime's own view — which is the
    /// view replacement is judged against.
    async fn observe_terminal(&self, binding: &RuntimeBindingSnapshot) {
        self.fake.push_step_for(
            ScriptStep::CancelObservedTerminal,
            RequestKey::Binding(binding.binding_id()),
        );
        self.fake
            .cancel(&CancelRequest {
                binding: binding.clone(),
                requested_at: now(),
            })
            .await
            .expect("the runtime confirms the cancellation");
    }

    /// How many native sessions the runtime was actually asked to start.
    ///
    /// Counting launches — not every adapter call — is what makes "the refusal
    /// happened before the runtime was touched" a real assertion.
    fn launch_count(&self) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::Launch(_)))
            .count()
    }
}

/// Take a permit through runtime admission to the one request it can spend.
///
/// # Panics
/// Panics when the runtime refuses the seat. Tests that are *about* a refused
/// admission call [`RuntimeAdapter::admit_launch`] themselves.
async fn admitted(runtime: &Runtime, permit: LaunchPermit) -> PreparedLaunch {
    let launch = runtime.launch_input();
    let authority = runtime
        .fake
        .admit_launch(&permit.admission_request(&launch))
        .await
        .expect("the runtime admits this seat")
        .into_authority()
        .expect("a free seat is admitted rather than resumed");
    permit.launch_request(authority, launch)
}

/// Reserve, be admitted, launch through the real adapter and bind — the whole
/// supported path.
async fn occupy(
    slots: &mut TeamRunSlots,
    runtime: &Runtime,
    id: &RoleSlotId,
    agent_run_id: AgentRunId,
) -> RuntimeBindingSnapshot {
    let permit = slots
        .reserve(id, agent_run_id)
        .expect("a vacant declared slot reserves");
    let prepared = admitted(runtime, permit).await;
    let outcome = runtime
        .fake
        .launch(prepared.request())
        .await
        .expect("the role launches");
    slots
        .bind(prepared, &outcome.snapshot)
        .expect("the slot binds its own session");
    outcome.snapshot
}

/// A stored run row for one attempt at one slot.
fn run_row(
    team_run_id: TeamRunId,
    id: &RoleSlotId,
    agent_run_id: AgentRunId,
    parent: Option<AgentRunId>,
    lifecycle: RunLifecycle,
) -> AgentRun {
    let terminal = lifecycle
        .terminal_outcome()
        .map(|outcome| TerminalEvidence {
            outcome,
            source: TerminalEvidenceSource::RuntimeObservation {
                cursor: EventCursor::parse(7).expect("a positive cursor"),
            },
            evidence_hash: ContentHash::of(agent_run_id.to_string().as_bytes()),
            closed_at: now(),
        });
    AgentRun {
        id: agent_run_id,
        project_id: ProjectId::generate(),
        team_run_id,
        parent_agent_run_id: parent,
        role: id.as_role_key().clone(),
        account_profile_id: None,
        binding: None,
        projection: RunProjection {
            lifecycle,
            desired: DesiredRunState::RunRequested,
            observed: ObservedRunState::Running,
            derived: DerivedRunState::Confirmed,
            last_confirmed_at: Some(now()),
            last_cursor: None,
        },
        terminal: terminal.clone(),
        revision: AggregateRevision::INITIAL,
        created_at: now(),
        closed_at: terminal.as_ref().map(|_| now()),
    }
}

/// A stored run row that carries the session the slot is holding.
fn closing_row(
    team_run_id: TeamRunId,
    id: &RoleSlotId,
    binding: &RuntimeBindingSnapshot,
    parent: Option<AgentRunId>,
    lifecycle: RunLifecycle,
) -> AgentRun {
    let mut row = run_row(team_run_id, id, binding.agent_run_id(), parent, lifecycle);
    row.binding = Some(binding.binding.clone());
    row
}

/// Close every declared slot of `template` with one succeeded attempt.
fn all_slots_closed(template: &TeamTemplateSpec, team_run_id: TeamRunId) -> Vec<AgentRun> {
    template
        .slots
        .iter()
        .map(|declared| {
            run_row(
                team_run_id,
                &declared.id,
                AgentRunId::generate(),
                None,
                RunLifecycle::Succeeded,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Template documents and revisions
// ---------------------------------------------------------------------------

#[test]
fn every_seed_template_round_trips_byte_and_hash_identically() {
    let pack = bundled_teams().expect("the bundled team pack loads");
    assert!(
        pack.teams.len() >= 2,
        "the bundled pack ships more than one template"
    );
    for template in &pack.teams {
        let revision = template.to_revision().expect("the template canonicalizes");
        let read = TeamTemplateSpec::from_revision(&revision).expect("the envelope round trips");
        assert_eq!(&read, template, "the definition survives the envelope");
        assert_eq!(
            read.to_revision().expect("recanonicalizes").definition,
            revision.definition,
            "canonical bytes are stable across a round trip"
        );
        assert_eq!(revision.role_authority, template.role_authority());

        let snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION);
        let frozen = TeamTemplateSpec::from_snapshot(&snapshot).expect("the run snapshot reads");
        assert_eq!(&frozen, template);
    }
}

#[test]
fn bundled_claude_routes_try_both_accounts_before_the_next_model() {
    let pack = bundled_teams().expect("the bundled team pack loads");
    let expected = [
        ("claude-work", "claude-opus-5"),
        ("claude-personal", "claude-opus-5"),
        ("codex-work", "gpt-5.6-sol"),
        ("codex-personal", "gpt-5.6-sol"),
    ];
    let mut checked = 0;
    for slot in pack.teams.iter().flat_map(|team| &team.slots) {
        let chain = slot.model_chain.as_ref().expect("a bundled route");
        if chain
            .rungs
            .first()
            .is_some_and(|rung| rung.model.0 == "claude-opus-5")
        {
            let actual: Vec<(&str, &str)> = chain
                .rungs
                .iter()
                .map(|rung| (rung.provider.0.as_str(), rung.model.0.as_str()))
                .collect();
            assert_eq!(actual, expected, "{}", slot.id);
            checked += 1;
        }
    }
    assert!(checked > 0, "the bundled pack exercises Claude routing");
}

#[test]
fn a_logical_role_may_be_declared_twice_only_through_distinct_slots() {
    let template = parallel_seed();
    let repeated = template
        .roles
        .iter()
        .find(|requirement| requirement.min_slots > 1)
        .expect("the template repeats a role");

    let slots = template.slots_of(&repeated.role.role);
    assert_eq!(
        slots.len(),
        usize::try_from(repeated.min_slots).expect("a small cardinality"),
        "cardinality is counted from declared slots"
    );
    let distinct: BTreeSet<&RoleSlotId> = slots.iter().map(|declared| &declared.id).collect();
    assert_eq!(distinct.len(), slots.len(), "the slot ids are distinct");
    assert_eq!(
        template.cardinality_of(&repeated.role.role),
        slots.len(),
        "cardinality never reads a session"
    );
}

#[test]
fn a_revision_publishes_the_next_version_and_leaves_the_previous_untouched() {
    let first = seed(0);
    let before = first.to_revision().expect("the template canonicalizes");

    let second = revise_team_template(&first, |template| {
        template.max_handoff_depth = template.max_handoff_depth.saturating_sub(1).max(1);
    })
    .expect("a structural edit revises");

    assert_eq!(second.template_id, first.template_id, "the id is preserved");
    assert_eq!(second.version, SpecVersion::parse(2).expect("v2"));
    assert_eq!(first.version, SpecVersion::FIRST, "v1 is not mutated");

    let after = first.to_revision().expect("v1 still canonicalizes");
    assert_eq!(
        after.definition.json(),
        before.definition.json(),
        "publishing v2 does not rewrite v1's bytes"
    );
    assert_eq!(after.definition.hash(), before.definition.hash());
    assert_ne!(
        second.to_revision().expect("v2 canonicalizes").definition,
        before.definition,
        "v2 is a different document"
    );
}

#[test]
fn a_revision_may_not_rename_or_renumber_the_template() {
    let first = seed(0);
    let renamed = revise_team_template(&first, |template| {
        template.template_id = kontor_core::id::TeamTemplateId::generate();
    });
    assert!(renamed.is_err(), "a revision may not change the logical id");

    let renumbered = revise_team_template(&first, |template| {
        template.version = SpecVersion::parse(9).expect("v9");
    });
    assert!(
        renumbered.is_err(),
        "a revision may not choose its own version"
    );
}

#[test]
fn an_envelope_that_disagrees_with_its_definition_is_refused() {
    let template = seed(0);
    let honest = template.to_revision().expect("the template canonicalizes");

    let renamed = TeamTemplateRevision {
        name: kontor_core::id::ExternalName::parse("Another name").expect("a name"),
        ..honest.clone()
    };
    assert!(
        TeamTemplateSpec::from_revision(&renamed).is_err(),
        "the envelope name must match the definition"
    );

    let reauthorized = TeamTemplateRevision {
        role_authority: Vec::new(),
        ..honest
    };
    assert!(
        TeamTemplateSpec::from_revision(&reauthorized).is_err(),
        "the envelope may not carry authority the definition does not derive"
    );
}

// ---------------------------------------------------------------------------
// Structural validation (AC-3)
// ---------------------------------------------------------------------------

/// Each case mutates one structural property of a valid seed and must be
/// refused. Nothing here names a seed id: every case is expressed as an edit.
#[test]
fn structurally_invalid_templates_are_refused() {
    type Case = (&'static str, fn(&mut TeamTemplateSpec));
    let cases: &[Case] = &[
        ("duplicate slot id", |template| {
            let first = template.slots[0].clone();
            template.slots[1].id = first.id;
            template.slots[1].role = first.role;
        }),
        ("slot of an unrequired role", |template| {
            template.slots[0].role.role = role("zz.unrequired");
        }),
        ("slot pinning another role revision", |template| {
            template.slots[0].role.version = SpecVersion::parse(9).expect("v9");
        }),
        ("duplicate role requirement", |template| {
            let first = template.roles[0].clone();
            template.roles.push(first);
        }),
        ("cardinality with min above max", |template| {
            template.roles[0].min_slots = 3;
            template.roles[0].max_slots = 1;
        }),
        ("cardinality with a zero minimum", |template| {
            template.roles[0].min_slots = 0;
        }),
        ("a role with too few slots", |template| {
            template.roles[0].min_slots = 4;
            template.roles[0].max_slots = 4;
        }),
        ("a role with too many slots", |template| {
            let role_key = template.slots[0].role.role.clone();
            let declared = template.cardinality_of(&role_key);
            for requirement in &mut template.roles {
                if requirement.role.role == role_key {
                    requirement.max_slots =
                        u32::try_from(declared).expect("a small cardinality") - 1;
                    requirement.min_slots = 1;
                }
            }
        }),
        ("a duplicate skill pin", |template| {
            let first = template.slots[0].skills[0].clone();
            template.slots[0].skills.push(first);
        }),
        ("evaluator and waiver authority overlapping", |template| {
            let gate = kontor_core::id::GateKey::parse("zz.gate").expect("a gate key");
            template.slots[0].may_evaluate.push(gate.clone());
            template.slots[0].may_waive.push(gate);
        }),
        ("a duplicate evaluated gate", |template| {
            let gate = kontor_core::id::GateKey::parse("zz.gate").expect("a gate key");
            template.slots[0].may_evaluate.push(gate.clone());
            template.slots[0].may_evaluate.push(gate);
        }),
        ("a self handoff", |template| {
            template.handoffs[0].to_slot = template.handoffs[0].from_slot.clone();
        }),
        ("a duplicate handoff", |template| {
            let first = template.handoffs[0].clone();
            template.handoffs.push(first);
        }),
        ("a dangling handoff", |template| {
            template.handoffs[0].to_slot = slot("zz.nowhere");
        }),
        ("a handoff cycle", |template| {
            let first = template.handoffs[0].clone();
            template.handoffs.push(kontor_teams::spec::RoleHandoff {
                from_slot: first.to_slot,
                to_slot: first.from_slot,
                after_phase: None,
                required_artifacts: Vec::new(),
            });
        }),
        ("a handoff path past the declared depth", |template| {
            template.max_handoff_depth = 1;
        }),
        ("a successor depth past the global bound", |template| {
            template.max_successor_depth = kontor_teams::spec::MAX_SUCCESSOR_DEPTH + 1;
        }),
        ("no slots at all", |template| {
            template.slots.clear();
        }),
        ("a waiver policy with no authority", |template| {
            for declared in &mut template.slots {
                if let Some(policy) = declared.waiver_policy.as_mut() {
                    policy.authorized_roles.clear();
                }
            }
        }),
        ("a waiver policy with no evidence", |template| {
            for declared in &mut template.slots {
                if let Some(policy) = declared.waiver_policy.as_mut() {
                    policy.required_evidence.clear();
                }
            }
        }),
        ("a slot excused by its own role", |template| {
            for declared in &mut template.slots {
                if let Some(policy) = declared.waiver_policy.as_mut() {
                    policy.authorized_roles = vec![declared.role.role.clone()];
                }
            }
        }),
    ];

    for (label, mutate) in cases {
        let mut broken = parallel_seed();
        mutate(&mut broken);
        let refused = broken.validate();
        assert!(refused.is_err(), "{label} must be refused, but validated");
        assert!(
            broken.to_revision().is_err(),
            "{label} must not reach a stored revision"
        );
    }
    parallel_seed()
        .validate()
        .expect("the untouched seed still validates");
}

/// A duplicate slot id is refused *as a duplicate slot id*.
///
/// The case in the table above also breaks cardinality, and a duplicate id
/// happens to disturb the handoff bookkeeping too, so "it was refused" does not
/// say which rule refused. Here the cardinality bound is widened to admit the
/// extra seat and the error itself is asserted, which is what pins the rule
/// rather than one of its neighbours.
#[test]
fn a_duplicate_slot_id_is_refused_by_the_rule_that_names_it() {
    let mut template = parallel_seed();
    let first = template.slots[0].clone();
    for requirement in &mut template.roles {
        if requirement.role.role == first.role.role {
            requirement.max_slots += 1;
        }
    }
    template.slots.push(first);

    assert_eq!(
        template
            .validate()
            .expect_err("a duplicate slot id is refused"),
        DomainError::Invalid {
            subject: "TeamTemplateSpec",
            rule: "declares a duplicate role slot id",
        },
        "the duplicate id must be refused by its own rule, not by a side effect"
    );
}

#[test]
fn a_pack_with_a_duplicate_template_revision_is_refused() {
    let template = seed(0);
    let revision = serde_json::to_value(&template).expect("the template serializes");
    let doubled = serde_json::json!({
        "schema_version": 1,
        "teams": [revision, revision],
    });
    let refused = parse_team_pack(&doubled.to_string());
    assert!(
        refused.is_err(),
        "a pack may not declare one revision twice"
    );
}

// ---------------------------------------------------------------------------
// The sealed slot lifecycle (AC-4, AC-5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_occupied_slot_offers_no_second_launch_permit() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();

    let bound = occupy(&mut slots, &runtime, &id, AgentRunId::generate()).await;
    let launches = runtime.launch_count();

    let refused = slots.reserve(&id, AgentRunId::generate());
    assert!(
        refused.is_err(),
        "an occupied slot must not mint a second permit"
    );
    assert_eq!(
        runtime.launch_count(),
        launches,
        "the refusal happens before the runtime is called"
    );
    assert_eq!(
        slots.current_binding(&id),
        Some(&bound),
        "the slot still holds exactly its first binding"
    );
}

#[tokio::test]
async fn two_distinct_slots_of_one_role_hold_independent_sessions() {
    let template = parallel_seed();
    let repeated = template
        .roles
        .iter()
        .find(|requirement| requirement.min_slots > 1)
        .expect("the template repeats a role")
        .role
        .role
        .clone();
    let pair: Vec<RoleSlotId> = template
        .slots_of(&repeated)
        .iter()
        .map(|declared| declared.id.clone())
        .collect();
    assert_eq!(pair.len(), 2, "the role is declared by two slots");

    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");

    let first = occupy(&mut slots, &runtime, &pair[0], AgentRunId::generate()).await;
    let second = occupy(&mut slots, &runtime, &pair[1], AgentRunId::generate()).await;

    assert_ne!(
        first.binding_id(),
        second.binding_id(),
        "two slots hold two bindings"
    );
    assert_ne!(
        first.identity(),
        second.identity(),
        "two slots hold two native sessions"
    );
    assert_ne!(first.agent_run_id(), second.agent_run_id());
    assert_eq!(runtime.launch_count(), 2);
}

#[tokio::test]
async fn a_replacement_closes_the_old_session_before_the_successor_exists() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();

    let first_run = AgentRunId::generate();
    let old = occupy(&mut slots, &runtime, &id, first_run).await;
    let occupied = slots.occupied(&id).expect("the slot is occupied");
    let pending = slots
        .begin_replacement(occupied)
        .expect("replacement begins");

    assert_eq!(pending.binding(), &old, "the old binding is retained");
    assert!(
        slots.reserve(&id, AgentRunId::generate()).is_err(),
        "no launch is reachable while the old session is open"
    );
    assert_eq!(
        runtime.launch_count(),
        1,
        "nothing was launched during the pending replacement"
    );

    let closed_row = closing_row(team_run_id, &id, &old, None, RunLifecycle::Succeeded);
    let closed = slots
        .close_replaced(pending, &closed_row)
        .expect("an evidenced terminal closes the slot");

    let successor_run = AgentRunId::generate();
    let permit = slots
        .reserve_successor(closed, successor_run)
        .expect("a closed slot mints the successor permit");
    assert_eq!(
        permit.parent_agent_run_id(),
        Some(first_run),
        "the successor carries its parent"
    );

    let row = permit
        .new_agent_run(ProjectId::generate(), None, None, now())
        .expect("an unlaunched row builds");
    assert_eq!(row.parent_agent_run_id, Some(first_run));
    assert_eq!(
        &row.role,
        id.as_role_key(),
        "the successor stays in its slot"
    );
    assert_eq!(row.team_run_id, team_run_id);
    assert!(row.binding.is_none(), "the successor starts unbound");
    assert!(
        permit
            .new_agent_run(ProjectId::generate(), None, Some(&old), now())
            .is_err(),
        "the retired session may not be filed against the successor's row"
    );

    // The runtime frees the seat on its own observation, not on Kontor's row.
    runtime.observe_terminal(&old).await;
    let prepared = admitted(&runtime, permit).await;
    let outcome = runtime
        .fake
        .launch(prepared.request())
        .await
        .expect("the successor launches");
    let new_binding = slots
        .bind(prepared, &outcome.snapshot)
        .expect("the successor binds");

    let fresh = new_binding.binding();
    assert_ne!(fresh.binding_id(), old.binding_id(), "a fresh binding");
    assert_ne!(fresh.identity(), old.identity(), "a fresh session");
    assert_eq!(
        old.agent_run_id(),
        first_run,
        "the retired binding still names the run it was issued for"
    );
    assert_eq!(
        fresh.agent_run_id(),
        successor_run,
        "the new binding names the successor run"
    );
}

#[tokio::test]
async fn a_pending_replacement_refuses_a_run_that_has_not_closed() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let first_run = AgentRunId::generate();

    occupy(&mut slots, &runtime, &id, first_run).await;
    let occupied = slots.occupied(&id).expect("the slot is occupied");
    let pending = slots
        .begin_replacement(occupied)
        .expect("replacement begins");

    let still_running = run_row(team_run_id, &id, first_run, None, RunLifecycle::Running);
    assert!(
        slots.close_replaced(pending, &still_running).is_err(),
        "a non-terminal run must not close a pending replacement"
    );
    assert!(
        slots.reserve(&id, AgentRunId::generate()).is_err(),
        "the slot stays unavailable after a refused close"
    );
    assert_eq!(
        runtime.launch_count(),
        1,
        "no successor was launched behind the refusal"
    );
}

/// Uncertainty is not completion, whatever the row asserts about itself.
///
/// The row here carries closure evidence *and* a non-terminal lifecycle — the
/// shape a caller produces when it decides a session is finished before the
/// runtime said so. Only the lifecycle check can refuse it, which is what makes
/// this case distinguish that rule from the missing-evidence one.
#[tokio::test]
async fn a_run_that_claims_evidence_while_still_open_closes_nothing() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let first_run = AgentRunId::generate();

    let binding = occupy(&mut slots, &runtime, &id, first_run).await;

    // Everything about this row is right except the one thing under test: it
    // holds the seat's own session and carries closure evidence, and is still
    // running. Only the lifecycle rule can refuse it.
    let mut claiming = closing_row(team_run_id, &id, &binding, None, RunLifecycle::Succeeded);
    assert!(
        claiming.terminal.is_some(),
        "the row does carry closure evidence"
    );
    assert!(
        claiming.binding.is_some(),
        "and does hold the session the seat is retiring"
    );
    claiming.projection.lifecycle = RunLifecycle::Running;

    let occupied = slots.occupied(&id).expect("the slot is occupied");
    assert!(
        slots.close_completed(occupied, &claiming).is_err(),
        "an open run must not close its slot even with evidence attached"
    );

    let occupied = slots.occupied(&id).expect("the slot is still occupied");
    let pending = slots
        .begin_replacement(occupied)
        .expect("replacement begins");
    assert!(
        slots.close_replaced(pending, &claiming).is_err(),
        "and it must not satisfy a pending replacement either"
    );
    assert!(
        slots.reserve(&id, AgentRunId::generate()).is_err(),
        "so the seat stays unavailable"
    );
    assert_eq!(runtime.launch_count(), 1);
}

/// Replaying an admitted request opens no second session.
///
/// The reservation is consumed by the launch that spends it, so the second
/// presentation of the *same* request finds nothing to spend. Stated on the
/// runtime's own state: the seat holds one native session before and after.
#[tokio::test]
async fn one_admission_buys_exactly_one_native_session() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();

    let permit = slots
        .reserve(&id, AgentRunId::generate())
        .expect("a vacant seat reserves");
    let launch = runtime.launch_input();
    let authority = runtime
        .fake
        .admit_launch(&permit.admission_request(&launch))
        .await
        .expect("the runtime admits a vacant seat")
        .into_authority()
        .expect("a vacant seat is admitted rather than resumed");
    assert!(
        runtime.fake.is_reserved(&runtime.slot_key(&id)),
        "the runtime is holding the reservation it just issued"
    );
    let prepared = permit.launch_request(authority, launch);

    let outcome = runtime
        .fake
        .launch(prepared.request())
        .await
        .expect("the first launch is the one the admission paid for");
    assert_eq!(runtime.launch_count(), 1);
    assert!(
        !runtime.fake.is_reserved(&runtime.slot_key(&id)),
        "and the launch spent it"
    );

    // The very same request, handed over again.
    let replayed = runtime.fake.launch(prepared.request()).await;
    assert_eq!(
        replayed.expect_err("a replayed request must not start a second session"),
        RuntimeError::LaunchNotAdmitted {
            rule: "this seat is holding no reservation to spend",
        }
    );
    assert_eq!(
        runtime.launch_count(),
        1,
        "the replay is refused before it reaches the runtime's session table"
    );
    assert_eq!(
        runtime.fake.sessions_in(&runtime.slot_key(&id)),
        1,
        "and the seat still holds exactly one native session"
    );

    slots
        .bind(prepared, &outcome.snapshot)
        .expect("the seat binds the one session it started");
}

/// Fresh identifiers do not buy a seat that is taken.
///
/// This is the attack the previous, permit-shaped design could not answer: mint
/// a brand-new [`AgentRunId`] and [`RuntimeBindingId`], and every rule keyed on
/// *those* has nothing to object to. Admission is keyed on the seat, which the
/// attacker cannot mint, so the refusal happens before a request can even be
/// assembled — there is no route from here to a `LaunchRequest` at all.
#[tokio::test]
async fn freshly_minted_identifiers_cannot_take_an_occupied_seat() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let honest_run = AgentRunId::generate();

    occupy(&mut slots, &runtime, &id, honest_run).await;
    runtime.fake.take_calls();

    // Nothing about this is malformed. It is a different run, a different
    // binding, correctly named, aimed at a seat that is already full.
    let impostor_run = AgentRunId::generate();
    let impostor = runtime.launch_parts(&id, impostor_run);
    assert_eq!(
        runtime
            .admit(&impostor)
            .await
            .expect_err("an occupied seat admits nobody else"),
        RuntimeError::SlotAlreadyAdmitted {
            rule: "this seat already holds a live native session",
        }
    );

    // Claiming to be a replacement does not help either: the runtime checks the
    // citation against the session it is actually holding.
    let forged = runtime
        .fake
        .admit_launch(&AdmissionRequest {
            slot: runtime.slot_key(&id),
            agent_run_id: impostor_run,
            binding_id: impostor.binding_id,
            replaces: Some(ReplacedBinding {
                binding_id: RuntimeBindingId::generate(),
                agent_run_id: honest_run,
                successor_agent_run_id: impostor_run,
            }),
            requested_at: now(),
        })
        .await
        .expect_err("a citation naming a binding this seat never held is refused");
    assert_eq!(
        forged,
        RuntimeError::ReplacementNotEvidenced {
            rule: "the cited binding is not the one this seat holds",
        }
    );

    assert!(
        runtime.fake.calls().is_empty(),
        "not one of the refusals reached the runtime"
    );
    assert_eq!(
        runtime.fake.sessions_in(&runtime.slot_key(&id)),
        1,
        "the seat still holds exactly one native session"
    );
    assert_eq!(
        runtime.fake.sessions_for(impostor_run),
        0,
        "and the impostor owns none"
    );
}

/// An outstanding reservation holds the seat against everyone but itself.
///
/// The two halves are one rule seen from both sides. A different attempt is
/// refused while the reservation stands — otherwise a seat could accumulate
/// reservations and hand out several launches. The *same* attempt asking again
/// is a caller that lost the answer, and gets the reservation it already has
/// rather than a second one.
#[tokio::test]
async fn a_reserved_seat_admits_nobody_else() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let id = template.slots[0].id.clone();

    let mine = runtime.launch_parts(&id, AgentRunId::generate());
    runtime
        .admit(&mine)
        .await
        .expect("a vacant seat admits the first attempt")
        .into_authority()
        .expect("a vacant seat is admitted");

    // A different attempt at the same seat, while the reservation stands.
    let rival = runtime.launch_parts(&id, AgentRunId::generate());
    assert_eq!(
        runtime
            .admit(&rival)
            .await
            .expect_err("a reserved seat is not free"),
        RuntimeError::SlotAlreadyAdmitted {
            rule: "another launch is already reserved for this seat",
        }
    );

    // The same attempt, with a different binding, is also a different attempt.
    let mut rebound = mine.clone();
    rebound.binding_id = RuntimeBindingId::generate();
    assert_eq!(
        runtime
            .admit(&rebound)
            .await
            .expect_err("a new binding is a new attempt, not a retry"),
        RuntimeError::SlotAlreadyAdmitted {
            rule: "another launch is already reserved for this seat",
        }
    );

    // The identical question, asked again: one reservation, handed back.
    let retried = runtime
        .admit(&mine)
        .await
        .expect("a lost answer is recoverable")
        .into_authority()
        .expect("the retry is admitted, not resumed");
    assert_eq!(retried.slot(), &runtime.slot_key(&id));
    assert_eq!(retried.agent_run_id(), mine.agent_run_id);
    assert_eq!(retried.binding_id(), mine.binding_id);

    // And it is the *same* reservation: spending it leaves nothing behind.
    runtime
        .fake
        .launch(&retried.into_request(mine))
        .await
        .expect("the retried authority is the live one");
    assert!(
        !runtime.fake.is_reserved(&runtime.slot_key(&id)),
        "one reservation existed, and one launch spent it"
    );
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&id)), 1);
}

/// A replacement must be the successor Kontor recorded, not merely *a* run.
///
/// Everything else about this citation is right — the seat's own binding, the
/// seat's own predecessor, a session the runtime has watched end. Only the
/// linkage is wrong, and without checking it any run at all could inherit a
/// seat on the strength of somebody else's closure.
#[tokio::test]
async fn a_replacement_must_be_the_successor_kontor_recorded() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let first_run = AgentRunId::generate();

    let old = occupy(&mut slots, &runtime, &id, first_run).await;
    runtime.observe_terminal(&old).await;

    let interloper = AgentRunId::generate();
    let refused = runtime
        .fake
        .admit_launch(&AdmissionRequest {
            slot: runtime.slot_key(&id),
            agent_run_id: interloper,
            binding_id: RuntimeBindingId::generate(),
            replaces: Some(ReplacedBinding {
                binding_id: old.binding_id(),
                agent_run_id: first_run,
                // Recorded against somebody else entirely.
                successor_agent_run_id: AgentRunId::generate(),
            }),
            requested_at: now(),
        })
        .await
        .expect_err("a closure that names another successor is not this run's inheritance");
    assert_eq!(
        refused,
        RuntimeError::ReplacementNotEvidenced {
            rule: "the recorded successor is not the run asking to be admitted",
        }
    );
    assert_eq!(runtime.launch_count(), 1, "and nothing was launched");
    assert_eq!(runtime.fake.sessions_for(interloper), 0);
}

/// A seat's reservation may be spent only by the authority that *is* it.
///
/// The sharp case: two seats reserved for the same run and binding, so
/// comparing the launch's run and binding against the reservation agrees on
/// both. Only the ticket — the runtime's private name for *this* reservation —
/// tells the two apart. Without it, one intent would open two sessions: one
/// seat's authority spends the other's reservation, and its own is still there
/// to spend afterwards.
#[tokio::test]
async fn one_seats_authority_cannot_spend_another_seats_reservation() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();

    let run = AgentRunId::generate();
    let here = runtime.launch_parts(&first, run);
    let mut there = here.clone();
    there.role_slot_id = second.clone();

    let mine = runtime
        .admit(&here)
        .await
        .expect("the first seat admits this launch")
        .into_authority()
        .expect("a vacant seat is admitted");
    runtime
        .admit(&there)
        .await
        .expect("the second seat admits the same run and binding too")
        .into_authority()
        .expect("a vacant seat is admitted");

    assert_eq!(
        runtime
            .fake
            .launch(&mine.into_request(there))
            .await
            .expect_err("a reservation is spent only by its own authority"),
        RuntimeError::LaunchNotAdmitted {
            rule: "this authority is not the reservation this seat is holding",
        }
    );
    assert_eq!(runtime.launch_count(), 0);
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&second)), 0);
    assert!(
        runtime.fake.is_reserved(&runtime.slot_key(&first))
            && runtime.fake.is_reserved(&runtime.slot_key(&second)),
        "both reservations survive the refusal, unspent"
    );
}

/// One run admitted into two seats still owns only one native session.
///
/// Seat-keyed admission has nothing to object to here — the two seats are
/// genuinely different, and each is genuinely free. The run-keyed refusal is
/// the only thing standing between this and one run driving two sessions.
#[tokio::test]
async fn one_run_cannot_hold_two_seats_at_once() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();
    let run = AgentRunId::generate();

    let here = runtime.launch_parts(&first, run);
    let authority = runtime
        .admit(&here)
        .await
        .expect("the first seat admits it")
        .into_authority()
        .expect("a vacant seat is admitted");
    runtime
        .fake
        .launch(&authority.into_request(here))
        .await
        .expect("the first seat starts the session");

    // A different seat, a different binding, the same run.
    let there = runtime.launch_parts(&second, run);
    let authority = runtime
        .admit(&there)
        .await
        .expect("the second seat is free, so admission has no objection")
        .into_authority()
        .expect("a vacant seat is admitted");

    assert_eq!(
        runtime
            .fake
            .launch(&authority.into_request(there))
            .await
            .expect_err("a run owns at most one native session"),
        RuntimeError::SessionAlreadyBound {
            rule: "recovery launches a successor run, never the same run twice",
        }
    );
    assert_eq!(
        runtime.fake.sessions_for(run),
        1,
        "the run still owns exactly one"
    );
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&second)), 0);
}

/// A refusal that happens *after* admission gives the seat back.
///
/// The run-keyed refusal above fires past the seat's yes, so the second seat's
/// reservation has already been spent by the time the launch is turned away. A
/// reservation has no other way out: nothing but a launch that succeeds removes
/// one. Left in place it would hold the seat against every future attempt —
/// neither a live binding nor a spendable reservation, and no replacement
/// possible either, because there is no session to observe terminal. A seat
/// that can never be filled again fails AC-4 as squarely as one filled twice.
#[tokio::test]
async fn a_run_refused_in_a_second_seat_releases_that_seat() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();
    let run = AgentRunId::generate();

    let here = runtime.launch_parts(&first, run);
    let authority = runtime
        .admit(&here)
        .await
        .expect("the first seat admits it")
        .into_authority()
        .expect("a vacant seat is admitted");
    runtime
        .fake
        .launch(&authority.into_request(here))
        .await
        .expect("the first seat starts the session");

    let there = runtime.launch_parts(&second, run);
    let authority = runtime
        .admit(&there)
        .await
        .expect("the second seat is free, so admission has no objection")
        .into_authority()
        .expect("a vacant seat is admitted");
    assert_eq!(
        runtime
            .fake
            .launch(&authority.into_request(there))
            .await
            .expect_err("a run owns at most one native session"),
        RuntimeError::SessionAlreadyBound {
            rule: "recovery launches a successor run, never the same run twice",
        }
    );

    assert!(
        !runtime.fake.is_reserved(&runtime.slot_key(&second)),
        "the refused seat is free again rather than spent"
    );
    // Free has to mean usable, not merely un-reserved: the seat still takes the
    // run it was always meant for.
    let parts = runtime.launch_parts(&second, AgentRunId::generate());
    let authority = runtime
        .admit(&parts)
        .await
        .expect("the released seat admits the next attempt")
        .into_authority()
        .expect("a vacant seat is admitted");
    runtime
        .fake
        .launch(&authority.into_request(parts))
        .await
        .expect("and the launch it was released for goes through");
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&second)), 1);
}

/// A launch the runtime never answered releases its seat too.
///
/// Nothing about the release belongs to the run-keyed rule in particular: every
/// refusal between admission and the first native effect leaves the same spent
/// reservation behind. A channel failure is the plainest of them — the seat
/// said yes, the call never arrived — and it has to end with the seat free, or
/// one flaky moment would retire a role for the rest of the team run.
#[tokio::test]
async fn a_launch_that_fails_at_the_channel_releases_its_seat() {
    let template = parallel_seed();
    let runtime = Runtime::prepare(TeamRunId::generate()).await;
    let id = template.slots[0].id.clone();

    runtime.fake.push_step(ScriptStep::TransportFailure {
        operation: RuntimeCapability::Launch,
    });
    let parts = runtime.launch_parts(&id, AgentRunId::generate());
    let authority = runtime
        .admit(&parts)
        .await
        .expect("the seat admits this launch")
        .into_authority()
        .expect("a vacant seat is admitted");
    assert_eq!(
        runtime
            .fake
            .launch(&authority.into_request(parts))
            .await
            .expect_err("the channel fails before the runtime answers"),
        RuntimeError::Transport {
            rule: "channel failed before the runtime answered",
        }
    );

    assert!(
        !runtime.fake.is_reserved(&runtime.slot_key(&id)),
        "a failed call does not retire the seat"
    );
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&id)), 0);
    // The retry is a new attempt, and the seat has room for it.
    let retry = runtime.launch_parts(&id, AgentRunId::generate());
    let authority = runtime
        .admit(&retry)
        .await
        .expect("the released seat admits the retry")
        .into_authority()
        .expect("a vacant seat is admitted");
    runtime
        .fake
        .launch(&authority.into_request(retry))
        .await
        .expect("and the retry launches");
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&id)), 1);
}

/// Authority issued for one seat launches nothing in another.
#[tokio::test]
async fn authority_for_one_seat_cannot_launch_another() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();

    let run = AgentRunId::generate();
    let parts = runtime.launch_parts(&first, run);
    let authority = runtime
        .admit(&parts)
        .await
        .expect("the first seat admits this launch")
        .into_authority()
        .expect("a vacant seat is admitted");

    // The same run and binding, the same authority — aimed at the seat next
    // door, which nobody reserved.
    let mut elsewhere = parts.clone();
    elsewhere.role_slot_id = second.clone();
    let misaimed = authority.into_request(elsewhere);

    assert_eq!(
        runtime
            .fake
            .launch(&misaimed)
            .await
            .expect_err("a launch may only spend the reservation of the seat it names"),
        RuntimeError::LaunchNotAdmitted {
            rule: "this seat is holding no reservation to spend",
        }
    );
    assert_eq!(runtime.launch_count(), 0);
    assert_eq!(
        runtime.fake.sessions_in(&runtime.slot_key(&second)),
        0,
        "the seat next door gained nothing"
    );
    assert!(
        runtime.fake.is_reserved(&runtime.slot_key(&first)),
        "and the reservation it was issued for is untouched"
    );
}

/// A launch whose parts disagree with the reservation spends nothing.
///
/// The seat is right, so the reservation is found; the run is not the one it
/// was issued for. Without this comparison a caller could be admitted for one
/// attempt and launch a different one into the same seat.
#[tokio::test]
async fn a_launch_that_renames_its_run_is_not_admitted() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let id = template.slots[0].id.clone();

    let parts = runtime.launch_parts(&id, AgentRunId::generate());
    let authority = runtime
        .admit(&parts)
        .await
        .expect("the seat admits this launch")
        .into_authority()
        .expect("a vacant seat is admitted");

    let mut renamed = parts.clone();
    renamed.agent_run_id = AgentRunId::generate();

    assert_eq!(
        runtime
            .fake
            .launch(&authority.into_request(renamed))
            .await
            .expect_err("the reservation names the attempt it was issued for"),
        RuntimeError::LaunchNotAdmitted {
            rule: "the launch names a different run or binding than the reservation",
        }
    );
    assert_eq!(runtime.launch_count(), 0);
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&id)), 0);
}

/// Compatible work resumes the session the seat already has.
///
/// Asking again with the same seat, run and binding is not an attack and is not
/// an error — it is what a caller that lost its answer does. The runtime hands
/// back its own binding instead of starting anything.
#[tokio::test]
async fn compatible_work_resumes_the_seats_existing_binding() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let run = AgentRunId::generate();

    let binding = occupy(&mut slots, &runtime, &id, run).await;
    let launches = runtime.launch_count();

    let outcome = runtime
        .fake
        .admit_launch(&AdmissionRequest {
            slot: runtime.slot_key(&id),
            agent_run_id: run,
            binding_id: binding.binding_id(),
            replaces: None,
            requested_at: now(),
        })
        .await
        .expect("the same work is admitted as a resume");
    assert_eq!(
        outcome
            .resumed()
            .expect("compatible work answers with the live binding")
            .binding_id(),
        binding.binding_id(),
        "and it is the runtime's own binding, not a new one"
    );
    assert!(
        outcome.into_authority().is_err(),
        "a resume issues no authority to launch"
    );
    assert_eq!(
        runtime.launch_count(),
        launches,
        "nothing was launched to answer it"
    );
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&id)), 1);
}

/// A restart does not reopen a seat the runtime is still holding.
///
/// This is the hydration race: Kontor comes back up, rebuilds a roster from
/// storage that never recorded the launch, and believes the seat is free. The
/// runtime remembers, so the launch is refused with nothing started.
#[tokio::test]
async fn a_restart_does_not_reopen_an_occupied_seat() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let snapshot = snapshot_of(&template);
    let id = template.slots[0].id.clone();

    {
        let mut slots = TeamRunSlots::open(lease(team_run_id), &snapshot).expect("slots open");
        occupy(&mut slots, &runtime, &id, AgentRunId::generate()).await;
    }
    runtime.fake.restart();

    // A fresh roster with no memory of the launch: the seat looks vacant here.
    let mut rebuilt = TeamRunSlots::open(lease(team_run_id), &snapshot).expect("the roster opens");
    let recovered = AgentRunId::generate();
    let permit = rebuilt
        .reserve(&id, recovered)
        .expect("the rebuilt roster believes the seat is free");
    let launch = runtime.launch_input();

    assert_eq!(
        runtime
            .fake
            .admit_launch(&permit.admission_request(&launch))
            .await
            .expect_err("the runtime remembers what the roster forgot"),
        RuntimeError::SlotAlreadyAdmitted {
            rule: "this seat already holds a live native session",
        }
    );
    assert_eq!(
        runtime.launch_count(),
        1,
        "no second launch survived the restart"
    );
    assert_eq!(runtime.fake.sessions_in(&runtime.slot_key(&id)), 1);
}

/// Two managers racing for one seat: exactly one of them gets a session.
///
/// The rivals are genuine threads with genuinely distinct run and binding ids,
/// so nothing but the seat key stands between them. Check-and-claim happens in
/// one step, so there is no window in which two of them both see a free seat.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_admissions_for_one_seat_start_exactly_one_session() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = std::sync::Arc::new(Runtime::prepare(team_run_id).await);
    let id = template.slots[0].id.clone();

    let rivals = 8;
    let mut racing = tokio::task::JoinSet::new();
    for _ in 0..rivals {
        let runtime = std::sync::Arc::clone(&runtime);
        let id = id.clone();
        racing.spawn(async move {
            let parts = runtime.launch_parts(&id, AgentRunId::generate());
            match runtime.admit(&parts).await {
                Err(_) => false,
                Ok(outcome) => {
                    let authority = outcome
                        .into_authority()
                        .expect("a contested seat is admitted or refused, never resumed");
                    runtime
                        .fake
                        .launch(&authority.into_request(parts))
                        .await
                        .is_ok()
                }
            }
        });
    }

    let mut winners = 0;
    while let Some(finished) = racing.join_next().await {
        if finished.expect("no rival panicked") {
            winners += 1;
        }
    }

    assert_eq!(winners, 1, "exactly one rival may start a session");
    assert_eq!(
        runtime.fake.sessions_in(&runtime.slot_key(&id)),
        1,
        "and the seat holds exactly one"
    );
    assert_eq!(runtime.launch_count(), 1, "one launch reached the runtime");
}

/// A replacement is admitted only once the runtime has seen the old session end.
///
/// Kontor closing its own row is not enough — that is Kontor agreeing with
/// itself. The seat opens when the runtime, which owns the session, has observed
/// it finish.
#[tokio::test]
async fn a_replacement_waits_for_the_runtime_to_observe_the_end() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let first_run = AgentRunId::generate();

    let old = occupy(&mut slots, &runtime, &id, first_run).await;
    let occupied = slots.occupied(&id).expect("the seat is occupied");
    let pending = slots
        .begin_replacement(occupied)
        .expect("replacement begins");
    let closed_row = closing_row(team_run_id, &id, &old, None, RunLifecycle::Succeeded);
    let closed = slots
        .close_replaced(pending, &closed_row)
        .expect("an evidenced terminal closes the seat in Kontor's records");

    // Kontor's records say finished. The runtime has observed nothing of the
    // kind, and it is the one that owns the session.
    let successor_run = AgentRunId::generate();
    let permit = slots
        .reserve_successor(closed, successor_run)
        .expect("a closed seat mints the successor permit");
    let launch = runtime.launch_input();
    assert_eq!(
        runtime
            .fake
            .admit_launch(&permit.admission_request(&launch))
            .await
            .expect_err("Kontor agreeing with itself does not free the seat"),
        RuntimeError::ReplacementNotEvidenced {
            rule: "the session this seat holds has not been observed finished",
        }
    );
    assert_eq!(runtime.launch_count(), 1, "and nothing was launched");

    // Now the runtime sees it end, and the same citation is accepted.
    runtime.observe_terminal(&old).await;
    let authority = runtime
        .fake
        .admit_launch(&permit.admission_request(&launch))
        .await
        .expect("an observed terminal frees the seat")
        .into_authority()
        .expect("the successor is admitted rather than resumed");
    let prepared = permit.launch_request(authority, launch);
    let outcome = runtime
        .fake
        .launch(prepared.request())
        .await
        .expect("the successor launches");
    slots
        .bind(prepared, &outcome.snapshot)
        .expect("the seat binds the successor");

    assert_eq!(runtime.launch_count(), 2);
    assert_eq!(
        runtime.fake.sessions_in(&runtime.slot_key(&id)),
        1,
        "the seat holds the successor, and only the successor"
    );
    assert_ne!(
        outcome.snapshot.binding_id(),
        old.binding_id(),
        "which is a fresh session, not the retired one"
    );
}

#[tokio::test]
async fn hydration_recovers_the_latest_closed_slot_for_operator_replacement() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let slot = template.slots[0].id.clone();
    let first_run = AgentRunId::generate();
    let binding = occupy(&mut slots, &runtime, &slot, first_run).await;
    let occupied = slots.occupied(&slot).expect("the seat is occupied");
    let closed_row = closing_row(team_run_id, &slot, &binding, None, RunLifecycle::Cancelled);
    slots
        .close_completed(occupied, &closed_row)
        .expect("the evidenced terminal closes the slot");
    drop(slots);

    let mut recovered = TeamRunSlots::hydrate(
        lease(team_run_id),
        &snapshot_of(&template),
        &[closed_row],
        &[],
    )
    .expect("the durable lineage hydrates");
    let closed = recovered
        .latest_closed(&slot)
        .expect("the latest closed attempt is recoverable");
    assert_eq!(closed.agent_run_id(), first_run);
    assert_eq!(closed.retired_binding(), Some(binding.binding_id()));

    let successor = AgentRunId::generate();
    let permit = recovered
        .reserve_successor(closed, successor)
        .expect("the recovered token reserves exactly one successor");
    assert_eq!(permit.parent_agent_run_id(), Some(first_run));
    assert!(recovered.latest_closed(&slot).is_err());
}

/// Finding 2 regression: the closing run must carry the session being retired.
#[tokio::test]
async fn a_closing_run_carrying_another_session_is_refused() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();

    let mine = occupy(&mut slots, &runtime, &first, AgentRunId::generate()).await;
    let foreign = occupy(&mut slots, &runtime, &second, AgentRunId::generate()).await;

    // A row for the right run and slot, but carrying somebody else's session.
    let mut stolen = closing_row(team_run_id, &first, &mine, None, RunLifecycle::Succeeded);
    stolen.binding = Some(foreign.binding.clone());
    let occupied = slots.occupied(&first).expect("the seat is occupied");
    assert_eq!(
        slots
            .close_completed(occupied, &stolen)
            .expect_err("a foreign session must not retire this seat"),
        DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "the closing run does not carry the session the slot is retiring",
        }
    );

    // A row that claims the seat held nothing at all.
    let mut unbound = closing_row(team_run_id, &first, &mine, None, RunLifecycle::Succeeded);
    unbound.binding = None;
    let occupied = slots.occupied(&first).expect("the seat is still occupied");
    assert!(
        slots.close_completed(occupied, &unbound).is_err(),
        "an occupied seat may not be retired by a run that held no session"
    );

    // The honest row closes it.
    let honest = closing_row(team_run_id, &first, &mine, None, RunLifecycle::Succeeded);
    let occupied = slots.occupied(&first).expect("the seat is still occupied");
    slots
        .close_completed(occupied, &honest)
        .expect("the run that held the session retires the seat");
}

/// Finding 3 regression: one team run has one writer.
#[test]
fn two_managers_cannot_own_one_team_run() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let snapshot = snapshot_of(&template);

    let first = TeamRunSlots::open(lease(team_run_id), &snapshot).expect("the first manager opens");
    assert_eq!(
        TeamRunLease::acquire(team_run_id).expect_err("a second writer must be refused"),
        DomainError::Invalid {
            subject: "TeamRunLease",
            rule: "another live manager already owns this team run's seats",
        }
    );

    // A different team run is unaffected.
    let other = TeamRunId::generate();
    let elsewhere = TeamRunLease::acquire(other).expect("another team run is free");
    assert_eq!(elsewhere.team_run_id(), other);
    drop(elsewhere);

    // Ownership is released with the manager that held it.
    drop(first);
    let again = TeamRunSlots::open(lease(team_run_id), &snapshot)
        .expect("the seats are available once the first manager is gone");
    assert_eq!(again.team_run_id(), team_run_id);
}

/// Finding 3 regression: a rival hydration cannot reserve a seat the live
/// manager already holds.
///
/// The race is made deterministic rather than probabilistic: the owner holds
/// its manager for the whole scope, so *every* rival must be refused. A lease
/// that did not exclude would let each rival hydrate a fresh, empty roster,
/// see the seat as vacant and reserve it — which is the double reservation this
/// pins down.
#[test]
fn racing_hydrations_cannot_double_reserve_a_slot() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let snapshot = snapshot_of(&template);
    let id = template.slots[0].id.clone();

    let mut owner = TeamRunSlots::open(lease(team_run_id), &snapshot).expect("the owner opens");
    owner
        .reserve(&id, AgentRunId::generate())
        .expect("the owner reserves the seat");

    let rivals = 8;
    let refused = AtomicUsize::new(0);
    let double_reserved = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..rivals {
            scope.spawn(|| match TeamRunLease::acquire(team_run_id) {
                Err(_) => {
                    refused.fetch_add(1, Ordering::SeqCst);
                }
                Ok(stolen) => {
                    let mut rival =
                        TeamRunSlots::open(stolen, &snapshot).expect("a rival roster opens");
                    if rival.reserve(&id, AgentRunId::generate()).is_ok() {
                        double_reserved.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
        }
    });

    assert_eq!(
        refused.load(Ordering::SeqCst),
        rivals,
        "every rival must be refused while the owner is alive"
    );
    assert_eq!(
        double_reserved.load(Ordering::SeqCst),
        0,
        "no rival may reserve a seat the live manager already holds"
    );
    assert!(owner.live_run(&id).is_some(), "the owner still holds it");
}

#[tokio::test]
async fn a_closing_run_from_another_slot_is_refused() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();
    let agent_run_id = AgentRunId::generate();

    occupy(&mut slots, &runtime, &first, agent_run_id).await;
    let occupied = slots.occupied(&first).expect("the slot is occupied");

    // Same run id, same team, but the row says it acted in another seat.
    let elsewhere = run_row(
        team_run_id,
        &second,
        agent_run_id,
        None,
        RunLifecycle::Succeeded,
    );
    assert!(
        slots.close_completed(occupied, &elsewhere).is_err(),
        "a run naming another slot must not close this one"
    );

    let occupied = slots.occupied(&first).expect("the slot is still occupied");
    let foreign_team = run_row(
        TeamRunId::generate(),
        &first,
        agent_run_id,
        None,
        RunLifecycle::Succeeded,
    );
    assert!(
        slots.close_completed(occupied, &foreign_team).is_err(),
        "a run of another team must not close this slot"
    );
}

#[tokio::test]
async fn a_permit_is_consumed_by_the_binding_it_produced() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let runtime = Runtime::prepare(team_run_id).await;
    let mut slots =
        TeamRunSlots::open(lease(team_run_id), &snapshot_of(&template)).expect("slots open");
    let id = template.slots[0].id.clone();
    let agent_run_id = AgentRunId::generate();

    let permit = slots.reserve(&id, agent_run_id).expect("the slot reserves");
    let prepared = admitted(&runtime, permit).await;
    let outcome = runtime
        .fake
        .launch(prepared.request())
        .await
        .expect("the role launches");
    slots.bind(prepared, &outcome.snapshot).expect("it binds");

    // A binding for another run cannot be laundered into another seat's permit.
    let other = AgentRunId::generate();
    let second = template.slots[1].id.clone();
    let stray_permit = slots.reserve(&second, other).expect("the slot reserves");
    let stray = admitted(&runtime, stray_permit).await;
    assert!(
        slots.bind(stray, &outcome.snapshot).is_err(),
        "a binding for another run must not fill this slot"
    );
}

#[test]
fn the_successor_chain_stops_at_the_declared_depth() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let id = template.slots[0].id.clone();
    let depth = template.max_successor_depth;

    let mut rows = Vec::new();
    let mut parent = None;
    for _ in 0..=depth {
        let this = AgentRunId::generate();
        rows.push(run_row(
            team_run_id,
            &id,
            this,
            parent,
            RunLifecycle::Succeeded,
        ));
        parent = Some(this);
    }
    let within = TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[])
        .expect("at bound");
    assert_eq!(
        within.attempt_count(&id),
        usize::try_from(depth).expect("small") + 1
    );
    drop(within);

    let this = AgentRunId::generate();
    rows.push(run_row(
        team_run_id,
        &id,
        this,
        parent,
        RunLifecycle::Succeeded,
    ));
    assert!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[]).is_err(),
        "one attempt past the declared depth is refused"
    );
}

// ---------------------------------------------------------------------------
// Hydration fails closed (AC-3, AC-4)
// ---------------------------------------------------------------------------

#[test]
fn malformed_hydrated_state_yields_no_roster() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let first = template.slots[0].id.clone();
    let second = template.slots[1].id.clone();
    let snapshot = snapshot_of(&template);

    let cases: Vec<(&str, Vec<AgentRun>)> = vec![
        (
            "two live leaves in one slot",
            vec![
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    None,
                    RunLifecycle::Running,
                ),
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    None,
                    RunLifecycle::Running,
                ),
            ],
        ),
        ("two roots in one slot", {
            vec![
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    None,
                    RunLifecycle::Succeeded,
                ),
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    None,
                    RunLifecycle::Succeeded,
                ),
            ]
        }),
        ("a parent in another slot", {
            let parent = AgentRunId::generate();
            vec![
                run_row(team_run_id, &second, parent, None, RunLifecycle::Succeeded),
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    Some(parent),
                    RunLifecycle::Succeeded,
                ),
            ]
        }),
        ("a branching parent", {
            let root = AgentRunId::generate();
            vec![
                run_row(team_run_id, &first, root, None, RunLifecycle::Succeeded),
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    Some(root),
                    RunLifecycle::Succeeded,
                ),
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    Some(root),
                    RunLifecycle::Succeeded,
                ),
            ]
        }),
        ("an open attempt below the leaf", {
            let root = AgentRunId::generate();
            vec![
                run_row(team_run_id, &first, root, None, RunLifecycle::Running),
                run_row(
                    team_run_id,
                    &first,
                    AgentRunId::generate(),
                    Some(root),
                    RunLifecycle::Succeeded,
                ),
            ]
        }),
        (
            "a run naming an undeclared slot",
            vec![run_row(
                team_run_id,
                &slot("zz.nowhere"),
                AgentRunId::generate(),
                None,
                RunLifecycle::Succeeded,
            )],
        ),
        (
            "a run of another team",
            vec![run_row(
                TeamRunId::generate(),
                &first,
                AgentRunId::generate(),
                None,
                RunLifecycle::Succeeded,
            )],
        ),
    ];

    for (label, rows) in cases {
        let hydrated = TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &rows, &[]);
        assert!(
            hydrated.is_err(),
            "{label} must fail closed, but produced a roster"
        );
    }
}

/// Two live runs in one seat is refused *as an AC-4 conflict*, by name.
///
/// The table above proves the roster fails closed; this proves *which* rule
/// closed it. That matters because the structural rules would refuse this
/// roster anyway — a second non-terminal run is necessarily either a second
/// root or a non-terminal non-leaf — so without naming the refusal the AC-4
/// check could be deleted and every test would stay green.
#[test]
fn two_live_runs_in_one_seat_are_refused_as_an_ac4_conflict() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let id = template.slots[0].id.clone();
    let rows = vec![
        run_row(
            team_run_id,
            &id,
            AgentRunId::generate(),
            None,
            RunLifecycle::Running,
        ),
        run_row(
            team_run_id,
            &id,
            AgentRunId::generate(),
            None,
            RunLifecycle::Running,
        ),
    ];

    assert_eq!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[])
            .expect_err("two live runs in one seat produce no roster"),
        DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "a role slot has more than one non-terminal run",
        }
    );
}

#[test]
fn a_closed_attempt_without_evidence_is_refused() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let id = template.slots[0].id.clone();
    let mut row = run_row(
        team_run_id,
        &id,
        AgentRunId::generate(),
        None,
        RunLifecycle::Succeeded,
    );
    row.terminal = None;

    assert!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &[row], &[]).is_err(),
        "a terminal attempt must carry its closure evidence"
    );
}

// ---------------------------------------------------------------------------
// Team closure over declared slots (AC-6)
// ---------------------------------------------------------------------------

#[test]
fn closure_requires_every_declared_slot() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let snapshot = snapshot_of(&template);
    let complete = all_slots_closed(&template, team_run_id);

    let certified = TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &complete, &[])
        .expect("a complete roster hydrates")
        .certify_team_closure(&[])
        .expect("every declared slot closed");
    assert_eq!(certified.outcome(), TerminalOutcome::Succeeded);
    assert_eq!(certified.children().len(), template.slots.len());

    // Omitting each declared slot in turn must fail.
    for index in 0..complete.len() {
        let mut partial = complete.clone();
        partial.remove(index);
        let refused = TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &partial, &[])
            .expect("a partial roster still hydrates")
            .certify_team_closure(&[]);
        assert!(
            refused.is_err(),
            "omitting one declared slot must refuse closure"
        );
    }
}

#[test]
fn closure_refuses_a_slot_whose_leaf_is_not_terminal() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let mut rows = all_slots_closed(&template, team_run_id);
    rows[0] = run_row(
        team_run_id,
        &template.slots[0].id,
        AgentRunId::generate(),
        None,
        RunLifecycle::Running,
    );

    let refused = TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[])
        .expect("an open leaf still hydrates")
        .certify_team_closure(&[]);
    assert!(refused.is_err(), "an open leaf must refuse closure");
}

#[test]
fn closure_accepts_an_authorized_evidence_bearing_waiver() {
    let template = parallel_seed();
    let waivable = template
        .slots
        .iter()
        .find(|declared| declared.waiver_policy.is_some())
        .expect("one seed slot is waivable")
        .clone();
    let policy = waivable.waiver_policy.clone().expect("a waiver policy");
    let team_run_id = TeamRunId::generate();
    let snapshot = snapshot_of(&template);

    let rows: Vec<AgentRun> = all_slots_closed(&template, team_run_id)
        .into_iter()
        .filter(|row| row.role != *waivable.id.as_role_key())
        .collect();

    let honest = RoleSlotWaiver {
        slot: waivable.id.clone(),
        authorized_by: policy.authorized_roles[0].clone(),
        evidence: policy.required_evidence.clone(),
        recorded_at: now(),
    };
    let certificate = TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &rows, &[])
        .expect("the roster hydrates")
        .certify_team_closure(std::slice::from_ref(&honest))
        .expect("an authorized, evidenced waiver excuses the slot");
    assert_eq!(certificate.children().len(), rows.len());

    let unauthorized = RoleSlotWaiver {
        authorized_by: role("zz.stranger"),
        ..honest.clone()
    };
    assert!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &rows, &[])
            .expect("the roster hydrates")
            .certify_team_closure(&[unauthorized])
            .is_err(),
        "an unauthorized waiver must not excuse a slot"
    );

    let evidence_free = RoleSlotWaiver {
        evidence: Vec::new(),
        ..honest.clone()
    };
    assert!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &rows, &[])
            .expect("the roster hydrates")
            .certify_team_closure(&[evidence_free])
            .is_err(),
        "an evidence-free waiver must not excuse a slot"
    );

    let wrong_evidence = RoleSlotWaiver {
        evidence: vec![artifact("zz.unrelated")],
        ..honest.clone()
    };
    assert!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &rows, &[])
            .expect("the roster hydrates")
            .certify_team_closure(&[wrong_evidence])
            .is_err(),
        "a waiver must cite the evidence the slot requires"
    );

    let unwaivable = template
        .slots
        .iter()
        .find(|declared| declared.waiver_policy.is_none())
        .expect("one seed slot is not waivable");
    let forbidden = RoleSlotWaiver {
        slot: unwaivable.id.clone(),
        ..honest
    };
    assert!(
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot, &rows, &[])
            .expect("the roster hydrates")
            .certify_team_closure(&[forbidden])
            .is_err(),
        "a slot the template does not allow waiving must not be waived"
    );
}

#[test]
fn the_certificate_binds_the_template_and_every_slot() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let rows = all_slots_closed(&template, team_run_id);

    let first = TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[])
        .expect("hydrates")
        .certify_team_closure(&[])
        .expect("certifies");
    let again = TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[])
        .expect("hydrates")
        .certify_team_closure(&[])
        .expect("certifies");
    assert_eq!(
        first.policy_digest(),
        again.policy_digest(),
        "the digest is deterministic"
    );

    let evidence = first
        .into_terminal_evidence(now())
        .expect("the certificate converts");
    evidence
        .verify_children(team_run_id, first.children())
        .expect("the core store recomputes the same child digest");

    // The digest is bound to the pinned template revision: the same runs under a
    // different revision of the same template certify to a different policy.
    let revised = revise_team_template(&template, |next| {
        next.max_handoff_depth = next.max_handoff_depth.saturating_sub(1).max(1);
    })
    .expect("a structural edit revises");
    let under_v2 = TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&revised), &rows, &[])
        .expect("hydrates")
        .certify_team_closure(&[])
        .expect("certifies");
    assert_ne!(
        first.policy_digest(),
        under_v2.policy_digest(),
        "the certificate names the template revision it proves"
    );
}

#[test]
fn a_failed_attempt_still_counts_as_child_evidence() {
    let template = parallel_seed();
    let team_run_id = TeamRunId::generate();
    let mut rows = all_slots_closed(&template, team_run_id);
    rows[0] = run_row(
        team_run_id,
        &template.slots[0].id,
        AgentRunId::generate(),
        None,
        RunLifecycle::Failed,
    );

    let certificate =
        TeamRunSlots::hydrate(lease(team_run_id), &snapshot_of(&template), &rows, &[])
            .expect("hydrates")
            .certify_team_closure(&[])
            .expect("certifies");
    assert_eq!(
        certificate.outcome(),
        TerminalOutcome::Failed,
        "the core reducer stays authoritative"
    );
}

// ---------------------------------------------------------------------------
// Context-window resolution through the roster
// ---------------------------------------------------------------------------

fn context_policy(class: ContextWindowClass) -> ContextWindowPolicy {
    ContextWindowPolicy {
        class,
        ..ContextWindowPolicy::standard()
    }
}

/// The roster resolves a seat against the slot's own declaration first and the
/// run's frozen seed table second — and records which one it used.
///
/// MUT-CTX-01 also lands here: swapping the resolver's role-slot and
/// work-profile arms changes the recorded source for the declaring slot.
#[test]
fn a_seat_resolves_its_own_declaration_over_the_frozen_seed() {
    let mut template = seed(0);
    let declaring = template.slots[0].id.clone();
    let seeded = template.slots[1].id.clone();
    let seeded_role = template.slots[1].role.role.clone();
    template.slots[0].context_window = Some(context_policy(ContextWindowClass::Extended));

    let snapshot = snapshot_of(&template)
        .with_context_policy(TeamContextPolicySeed {
            work_profile: Some(context_policy(ContextWindowClass::Standard)),
            role_seeds: vec![RoleContextSeed {
                role: seeded_role,
                context_window: context_policy(ContextWindowClass::Lean),
            }],
        })
        .expect("the seed table validates");

    let team_run_id = TeamRunId::generate();
    let slots = TeamRunSlots::open(lease(team_run_id), &snapshot).expect("the roster opens");

    // The slot declared `extended` explicitly, which is the only way to reach it.
    let declared = slots
        .requested_context_window(&declaring, None)
        .expect("the declaring seat resolves");
    assert_eq!(declared.source, ContextPolicySource::RoleSlot);
    assert_eq!(declared.policy.class, ContextWindowClass::Extended);

    // The other seat declared nothing, so the work-profile default outranks its
    // seed — precedence, not "the most specific thing that exists".
    let inherited = slots
        .requested_context_window(&seeded, None)
        .expect("the seeded seat resolves");
    assert_eq!(inherited.source, ContextPolicySource::WorkProfile);
    assert_eq!(inherited.policy.class, ContextWindowClass::Standard);

    // An authorized override outranks even an explicit slot declaration.
    let override_policy = context_policy(ContextWindowClass::Lean);
    let overridden = slots
        .requested_context_window(&declaring, Some(&override_policy))
        .expect("the override resolves");
    assert_eq!(
        overridden.source,
        ContextPolicySource::AuthorizedRunOverride
    );
    assert_eq!(overridden.policy.class, ContextWindowClass::Lean);
}

/// Editing the template after the run was created cannot reach backwards into
/// what the run resolves: the roster reads the frozen copy, not the source.
#[test]
fn a_later_template_edit_does_not_change_a_live_runs_policy() {
    let mut template = seed(0);
    let seat = template.slots[0].id.clone();
    template.slots[0].context_window = Some(context_policy(ContextWindowClass::Deep));
    let snapshot = snapshot_of(&template);

    let team_run_id = TeamRunId::generate();
    let slots = TeamRunSlots::open(lease(team_run_id), &snapshot).expect("the roster opens");
    let before = slots
        .requested_context_window(&seat, None)
        .expect("the seat resolves");
    assert_eq!(before.policy.class, ContextWindowClass::Deep);

    // The deployment changes its mind. The live run does not.
    template.slots[0].context_window = Some(context_policy(ContextWindowClass::Lean));
    let after = slots
        .requested_context_window(&seat, None)
        .expect("the seat still resolves");
    assert_eq!(after.policy.class, ContextWindowClass::Deep);
    assert_eq!(before, after);
}
