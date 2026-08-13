//! The deterministic ready-batch pass, judged as a pure function.
//!
//! Every test here injects its clock as [`SchedulingSnapshot::taken_at`] and its
//! ceilings as [`CapacityConfig`], so nothing in this suite depends on when it
//! runs, on which machine, or on how many times it has run before.
//!
//! The mutants this suite exists to kill:
//!
//! * an ordering that depends on insertion order, on a hash map, or on a
//!   tie-break that two tasks can share;
//! * a blocker skipped or reordered, so a candidate is reported as the wrong kind
//!   of problem — or admitted because the check that would have refused it ran
//!   after the one that let it through;
//! * a batch bounded by the global ceiling, the sum of the ceilings or the
//!   authorization alone rather than by the smallest of them;
//! * an adaptive window that cancels admitted work when it narrows;
//! * a dependency, a serialization peer, an unverified tree or a duplicate tree
//!   treated as harmless;
//! * event-origin work admitted on a receipt that proposed, rejected, ignored or
//!   duplicated it, or manual work refused for having no receipt at all.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::calendar::{EffectiveCalendarState, IanaTimeZone, WorkScope};
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CalendarProfileId, CanonicalDocument, ContentHash,
    ExecutionAuthorizationId, ExternalId, ExternalName, IntakeReceiptId, MiniProjectId, ModuleKey,
    ProjectId, RuntimeKindKey, SCHEMA_VERSION, SpecVersion, TaskId, TaskWorkflowId, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::spec::IntakeResult;
use kontor_core::state::TaskState;
use kontor_policy::ModuleClaim;
use kontor_runtime::{RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade};
use kontor_scheduler::{
    AccountAdmissionEvidence, AccountCapabilityKey, AccountPin, AdaptiveWindow,
    AdaptiveWindowConfig, AuthorizationEvidence, BLOCKER_ORDER, Blocker, CalendarAdmission,
    CalendarPolicyEvidence, Candidate, CandidateDecision, CapacityConfig, CapacityLimitKind,
    CapacityObservation, CapacityUsage, ExternalOwnership, ExternalWorkEvidence, FleetPreflight,
    IntakeLineage, MAX_PRIORITY, Plan, PreflightOutcome, ReconciliationEvidence,
    ReconciliationScope, RejectionCode, RuntimeAdmissionEvidence, RuntimeHealth,
    SchedulingSnapshot, TaskOrigin, WorktreeClaim, WorktreeVerification,
    minimum_launch_capabilities, plan,
};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// One labelled mutation of a candidate's reconciliation evidence.
type ReconciliationCase = (&'static str, fn(&mut ReconciliationEvidence));

/// One expected refusal and the single thing that causes it.
type CandidateCase = (RejectionCode, Box<dyn Fn(&mut Candidate)>);

/// One ceiling and the narrowing that makes it the binding one.
type CeilingCase = (CapacityLimitKind, Box<dyn Fn(&mut SchedulingSnapshot)>);

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC fixture timestamp")
}

fn now() -> Timestamp {
    at("2026-08-12T09:00:00Z")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn runtime_kind() -> RuntimeKindKey {
    RuntimeKindKey::parse("rb.runtime").expect("a valid runtime key")
}

fn module(text: &str) -> ModuleKey {
    ModuleKey::parse(text).expect("a valid module key")
}

fn digest() -> ContentHash {
    ContentHash::of(b"ready-batch fixture evidence")
}

fn capabilities(trust_grade: TrustGrade, sessions: u32) -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4_096,
            max_history_page: 64,
            max_concurrent_sessions: sessions,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

/// Ceilings wide enough that nothing is refused for capacity unless a test
/// narrows one deliberately.
fn wide_capacity() -> CapacityConfig {
    CapacityConfig {
        global_max_in_flight: 100,
        project_max_in_flight: 100,
        mission_max_in_flight: 100,
        account_max_in_flight: 100,
        provider_max_in_flight: 100,
        runtime_max_in_flight: 100,
        adaptive: AdaptiveWindowConfig {
            initial: 100,
            floor: 1,
            ceiling: 100,
            growth_step: 1,
        },
    }
}

/// A candidate with every blocker in its admitted position.
///
/// Each test moves exactly one thing, so a refusal names the thing the test
/// moved rather than something the fixture happened to leave wrong.
fn candidate(project: ProjectId, task: TaskId) -> Candidate {
    Candidate {
        project_id: project,
        task_id: task,
        mini_project_id: None,
        workflow_id: TaskWorkflowId::generate(),
        state: TaskState::Ready,
        revision: AggregateRevision::INITIAL,
        created_at: at("2026-08-12T08:00:00Z"),
        priority: 500,
        module: None,
        worktree: None,
        depends_on: BTreeSet::new(),
        serializes_with: BTreeSet::new(),
        origin: TaskOrigin::Manual,
        authorization: Some(AuthorizationEvidence {
            id: ExecutionAuthorizationId::generate(),
            project_id: project,
            scope: WorkScope::Project,
            selected_tasks: BTreeSet::new(),
            allowed_start: at("2026-08-12T00:00:00Z"),
            allowed_end: at("2026-08-13T00:00:00Z"),
            max_concurrency: 100,
        }),
        calendar: CalendarAdmission::unrestricted(),
        runtime: RuntimeAdmissionEvidence {
            runtime_kind: runtime_kind(),
            host: name("rb-host"),
            generation: 7,
            capabilities: capabilities(TrustGrade::A, 100),
            required: minimum_launch_capabilities(),
            health: RuntimeHealth::Healthy,
            reconciliation: ReconciliationEvidence {
                epoch_completed: true,
                scope: ReconciliationScope {
                    project_id: project,
                    runtime_kind: runtime_kind(),
                    host: name("rb-host"),
                    generation: 7,
                },
                open_replay_gap: false,
                divergence: false,
                orphan_ambiguity: false,
                stale_lost_contact: false,
            },
            last_confirmed_at: Some(at("2026-08-12T08:59:00Z")),
        },
        account: AccountAdmissionEvidence {
            pin: None,
            required_capabilities: BTreeSet::new(),
        },
        external: ExternalWorkEvidence::default(),
    }
}

fn pin(account: AccountProfileId) -> AccountPin {
    AccountPin {
        account_profile_id: account,
        pinned_revision: AggregateRevision::INITIAL,
        current_revision: AggregateRevision::INITIAL,
        enabled: true,
        cooldown_until: None,
        harness: runtime_kind(),
        declared_capabilities: BTreeSet::new(),
        provider_identity: Some(ExternalId::parse("provider-alpha").expect("a valid provider")),
        preflight: FleetPreflight {
            outcome: PreflightOutcome::Passed,
            evidence_hash: digest(),
            observed_at: at("2026-08-12T08:59:30Z"),
        },
    }
}

fn snapshot(candidates: Vec<Candidate>) -> SchedulingSnapshot {
    SchedulingSnapshot {
        schema_version: SCHEMA_VERSION,
        taken_at: now(),
        candidates,
        in_flight_tasks: BTreeSet::new(),
        completed_tasks: BTreeSet::new(),
        module_leases: Vec::new(),
        worktree_leases: BTreeSet::new(),
        usage: CapacityUsage::default(),
        capacity: wide_capacity(),
        adaptive_window: AdaptiveWindow::start(wide_capacity().adaptive),
        freshness: jiff::SignedDuration::from_secs(120),
    }
}

/// The pass's answer for one task.
fn decision_for(plan: &Plan, task: TaskId) -> &CandidateDecision {
    plan.decisions
        .iter()
        .find(|decision| decision.task_id() == task)
        .expect("every candidate is decided")
}

/// Assert the pass refuses `task` for exactly `code`.
fn assert_refused(plan: &Plan, task: TaskId, code: RejectionCode) {
    let decision = decision_for(plan, task);
    assert_eq!(
        decision.rejection_code(),
        Some(code),
        "expected {code} for this candidate, got {decision:?}"
    );
}

fn assert_admitted(plan: &Plan, task: TaskId) {
    let decision = decision_for(plan, task);
    assert!(
        matches!(decision, CandidateDecision::Admit(_)),
        "expected an admission, got {decision:?}"
    );
}

/// The canonical bytes of a plan, which is what "byte-identical" means.
fn bytes(plan: &Plan) -> String {
    CanonicalDocument::from_serializable(plan)
        .expect("a plan canonicalizes")
        .json()
        .to_owned()
}

// ---------------------------------------------------------------------------
// Determinism and ordering
// ---------------------------------------------------------------------------

#[test]
fn identical_snapshots_produce_byte_identical_decisions() {
    let project = ProjectId::generate();
    let candidates: Vec<Candidate> = (0..8)
        .map(|index| {
            let mut candidate = candidate(project, TaskId::generate());
            candidate.priority = 100 * (index % 3);
            candidate
        })
        .collect();
    let snapshot = snapshot(candidates);

    let first = plan(&snapshot).expect("the pass runs");
    let second = plan(&snapshot).expect("the pass runs again");
    assert_eq!(
        bytes(&first),
        bytes(&second),
        "the same snapshot must produce the same bytes"
    );
}

#[test]
fn ordering_is_priority_then_age_then_id_whatever_the_insertion_order() {
    let project = ProjectId::generate();
    // Two candidates share a priority *and* a creation instant, so only the id
    // can order them. That is the tie-break a restart must reproduce.
    let mut low = candidate(project, TaskId::generate());
    low.priority = 10;
    let mut high = candidate(project, TaskId::generate());
    high.priority = 900;
    let mut old = candidate(project, TaskId::generate());
    old.priority = 500;
    old.created_at = at("2026-08-10T00:00:00Z");
    let mut tie_a = candidate(project, TaskId::generate());
    tie_a.priority = 500;
    tie_a.created_at = at("2026-08-11T00:00:00Z");
    let mut tie_b = candidate(project, TaskId::generate());
    tie_b.priority = 500;
    tie_b.created_at = tie_a.created_at;

    let (first_tie, second_tie) = if tie_a.task_id < tie_b.task_id {
        (tie_a.task_id, tie_b.task_id)
    } else {
        (tie_b.task_id, tie_a.task_id)
    };
    let expected = vec![
        high.task_id,
        old.task_id,
        first_tie,
        second_tie,
        low.task_id,
    ];

    // Every permutation of the input must give the same output order. Two
    // deliberately different insertion orders are enough to kill a pass that
    // preserves the caller's.
    for candidates in [
        vec![
            low.clone(),
            high.clone(),
            old.clone(),
            tie_a.clone(),
            tie_b.clone(),
        ],
        vec![tie_b, tie_a, old, high, low],
    ] {
        let decided = plan(&snapshot(candidates)).expect("the pass runs");
        let order: Vec<TaskId> = decided
            .decisions
            .iter()
            .map(CandidateDecision::task_id)
            .collect();
        assert_eq!(order, expected);
    }
}

#[test]
fn the_blocker_order_is_the_declared_order_and_covers_every_blocker() {
    // The order is the contract: a candidate's reported refusal is the first
    // blocker that refuses it, so re-ordering this list changes what a passing
    // test means.
    assert_eq!(
        BLOCKER_ORDER,
        &[
            Blocker::Readiness,
            Blocker::Origin,
            Blocker::Dependencies,
            Blocker::Authorization,
            Blocker::Calendar,
            Blocker::ExternalWork,
            Blocker::Runtime,
            Blocker::Account,
            Blocker::Worktree,
            Blocker::Contention,
        ]
    );
}

/// A task is implicitly serialized against itself.
///
/// This is the double-admission no contention check catches: a task does not
/// contend with itself for its own module, and a task's lifecycle may still read
/// `ready` while an envelope of it is running, because those are orthogonal
/// dimensions.
#[test]
fn a_task_that_already_has_work_in_flight_is_refused_for_that_reason() {
    let project = ProjectId::generate();
    let mut running = candidate(project, TaskId::generate());
    // It even holds the module it would claim, which is precisely the case a
    // module lease cannot refuse.
    running.module = Some(module("directory.app"));
    let task = running.task_id;

    let mut in_flight = snapshot(vec![running.clone()]);
    in_flight.in_flight_tasks.insert(task);
    in_flight.module_leases.push(ModuleClaim {
        module: module("directory.app"),
        task_id: task,
        worktree: None,
        in_flight: true,
    });
    assert_refused(
        &plan(&in_flight).expect("the pass runs"),
        task,
        RejectionCode::TaskAlreadyInFlight,
    );

    // Its own claim is not a contention problem, so with nothing in flight the
    // same candidate is admitted.
    let mut idle = snapshot(vec![running]);
    idle.module_leases.push(ModuleClaim {
        module: module("directory.app"),
        task_id: task,
        worktree: None,
        in_flight: true,
    });
    assert_admitted(&plan(&idle).expect("the pass runs"), task);
}

#[test]
fn a_candidate_failing_two_blockers_reports_the_earlier_one() {
    let project = ProjectId::generate();
    let mut broken = candidate(project, TaskId::generate());
    // Not ready *and* unauthorized. Readiness is first in the order, so that is
    // the reported reason — a pass that reported the other would be answering a
    // different question.
    broken.state = TaskState::Todo;
    broken.authorization = None;
    let task = broken.task_id;

    let decided = plan(&snapshot(vec![broken])).expect("the pass runs");
    assert_refused(&decided, task, RejectionCode::TaskNotReady);
}

#[test]
fn an_invalid_snapshot_decides_nothing_at_all() {
    let project = ProjectId::generate();
    let task = TaskId::generate();
    let mut over_priority = candidate(project, task);
    over_priority.priority = MAX_PRIORITY + 1;
    assert!(
        plan(&snapshot(vec![over_priority])).is_err(),
        "an out-of-range priority refuses the pass rather than one candidate"
    );

    let duplicate = candidate(project, task);
    let twice = candidate(project, task);
    assert!(
        plan(&snapshot(vec![duplicate, twice])).is_err(),
        "one task cannot be a candidate twice"
    );

    let mut self_dependent = candidate(project, task);
    self_dependent.depends_on.insert(task);
    assert!(plan(&snapshot(vec![self_dependent])).is_err());

    let mut zero_ceiling = snapshot(vec![candidate(project, TaskId::generate())]);
    zero_ceiling.capacity.global_max_in_flight = 0;
    assert!(
        plan(&zero_ceiling).is_err(),
        "a zero ceiling is a configuration error, not `no work allowed`"
    );
}

// ---------------------------------------------------------------------------
// Dependencies, serialization and contention
// ---------------------------------------------------------------------------

#[test]
fn a_dependency_that_has_not_finished_blocks_and_a_finished_one_does_not() {
    let project = ProjectId::generate();
    let dependency = TaskId::generate();
    let mut dependent = candidate(project, TaskId::generate());
    dependent.depends_on.insert(dependency);
    let task = dependent.task_id;

    let mut blocked = snapshot(vec![dependent.clone()]);
    assert_refused(
        &plan(&blocked).expect("the pass runs"),
        task,
        RejectionCode::DependencyIncomplete,
    );

    blocked.completed_tasks.insert(dependency);
    assert_admitted(&plan(&blocked).expect("the pass runs"), task);
}

#[test]
fn a_serialization_peer_blocks_whether_it_is_running_or_selected_in_this_pass() {
    let project = ProjectId::generate();
    let first = candidate(project, TaskId::generate());
    let mut second = candidate(project, TaskId::generate());
    second.serializes_with.insert(first.task_id);
    // Priority makes the order explicit rather than incidental: the peer is
    // selected first, so the second candidate meets it as a selected peer.
    let mut first = first;
    first.priority = 900;
    second.priority = 100;
    let (first_task, second_task) = (first.task_id, second.task_id);

    let selected = plan(&snapshot(vec![first, second.clone()])).expect("the pass runs");
    assert_admitted(&selected, first_task);
    assert_refused(
        &selected,
        second_task,
        RejectionCode::SerializationPeerInFlight,
    );

    // The same refusal when the peer is already running rather than selected.
    let mut running = snapshot(vec![second]);
    running.in_flight_tasks.insert(first_task);
    assert_refused(
        &plan(&running).expect("the pass runs"),
        second_task,
        RejectionCode::SerializationPeerInFlight,
    );
}

#[test]
fn one_module_is_held_once_unless_distinct_verified_trees_keep_the_work_apart() {
    let project = ProjectId::generate();
    let shared = module("directory.app");

    // Held by other work with no tree at all: nothing is isolated from it.
    let mut contender = candidate(project, TaskId::generate());
    contender.module = Some(shared.clone());
    let task = contender.task_id;
    let mut held = snapshot(vec![contender.clone()]);
    held.module_leases.push(ModuleClaim {
        module: shared.clone(),
        task_id: TaskId::generate(),
        worktree: None,
        in_flight: true,
    });
    assert_refused(
        &plan(&held).expect("the pass runs"),
        task,
        RejectionCode::ModuleInFlight,
    );

    // Both sides in distinct verified trees: admitted.
    let mut isolated = contender.clone();
    isolated.worktree = Some(WorktreeClaim {
        worktree: name("/trees/mine"),
        verification: WorktreeVerification::Verified,
    });
    let mut apart = snapshot(vec![isolated]);
    apart.module_leases.push(ModuleClaim {
        module: shared.clone(),
        task_id: TaskId::generate(),
        worktree: Some(name("/trees/theirs")),
        in_flight: true,
    });
    assert_admitted(&plan(&apart).expect("the pass runs"), task);

    // The same tree on both sides is not isolation.
    let mut same_tree = snapshot(vec![{
        let mut candidate = contender.clone();
        candidate.worktree = Some(WorktreeClaim {
            worktree: name("/trees/shared"),
            verification: WorktreeVerification::Verified,
        });
        candidate
    }]);
    same_tree.module_leases.push(ModuleClaim {
        module: shared,
        task_id: TaskId::generate(),
        worktree: Some(name("/trees/shared")),
        in_flight: true,
    });
    assert_refused(
        &plan(&same_tree).expect("the pass runs"),
        task,
        RejectionCode::ModuleInFlight,
    );
}

#[test]
fn an_unverified_tree_is_refused_rather_than_read_as_no_tree() {
    let project = ProjectId::generate();
    let mut unverified = candidate(project, TaskId::generate());
    unverified.module = Some(module("directory.app"));
    unverified.worktree = Some(WorktreeClaim {
        worktree: name("/trees/claimed"),
        verification: WorktreeVerification::Unverified,
    });
    let task = unverified.task_id;

    // Nothing else holds the module, so treating the claim as "no tree" would
    // have admitted it. The refusal is what makes a fabricated path worthless.
    assert_refused(
        &plan(&snapshot(vec![unverified])).expect("the pass runs"),
        task,
        RejectionCode::WorktreeUnverified,
    );
}

#[test]
fn two_candidates_cannot_claim_one_tree_and_a_held_tree_is_not_reclaimed() {
    let project = ProjectId::generate();
    let tree = name("/trees/only-one");
    let claim = || {
        Some(WorktreeClaim {
            worktree: tree.clone(),
            verification: WorktreeVerification::Verified,
        })
    };
    let mut first = candidate(project, TaskId::generate());
    first.priority = 900;
    first.worktree = claim();
    let mut second = candidate(project, TaskId::generate());
    second.priority = 100;
    second.worktree = claim();
    let (first_task, second_task) = (first.task_id, second.task_id);

    let decided = plan(&snapshot(vec![first, second.clone()])).expect("the pass runs");
    assert_admitted(&decided, first_task);
    assert_refused(&decided, second_task, RejectionCode::WorktreeDuplicate);

    // A tree an existing lease already holds is refused the same way.
    let mut held = snapshot(vec![second]);
    held.worktree_leases.insert(tree);
    assert_refused(
        &plan(&held).expect("the pass runs"),
        second_task,
        RejectionCode::WorktreeDuplicate,
    );
}

// ---------------------------------------------------------------------------
// Authorization and calendar
// ---------------------------------------------------------------------------

#[test]
fn an_unrestricted_calendar_still_needs_an_authorization() {
    let project = ProjectId::generate();
    let mut unarmed = candidate(project, TaskId::generate());
    unarmed.calendar = CalendarAdmission::unrestricted();
    unarmed.authorization = None;
    let task = unarmed.task_id;

    assert_refused(
        &plan(&snapshot(vec![unarmed])).expect("the pass runs"),
        task,
        RejectionCode::AuthorizationMissing,
    );
}

#[test]
fn an_authorization_must_cover_this_task_and_this_instant() {
    let project = ProjectId::generate();

    let mut elsewhere = candidate(project, TaskId::generate());
    if let Some(authorization) = elsewhere.authorization.as_mut() {
        authorization.scope = WorkScope::Task {
            task_id: TaskId::generate(),
        };
    }
    let scoped_out = elsewhere.task_id;
    assert_refused(
        &plan(&snapshot(vec![elsewhere])).expect("the pass runs"),
        scoped_out,
        RejectionCode::AuthorizationScopeMismatch,
    );

    let mut unselected = candidate(project, TaskId::generate());
    if let Some(authorization) = unselected.authorization.as_mut() {
        authorization.selected_tasks.insert(TaskId::generate());
    }
    let not_selected = unselected.task_id;
    assert_refused(
        &plan(&snapshot(vec![unselected])).expect("the pass runs"),
        not_selected,
        RejectionCode::AuthorizationScopeMismatch,
    );

    let mut lapsed = candidate(project, TaskId::generate());
    if let Some(authorization) = lapsed.authorization.as_mut() {
        authorization.allowed_end = at("2026-08-12T08:00:00Z");
    }
    let expired = lapsed.task_id;
    assert_refused(
        &plan(&snapshot(vec![lapsed])).expect("the pass runs"),
        expired,
        RejectionCode::AuthorizationExpired,
    );
}

#[test]
fn a_closed_or_draining_calendar_admits_no_new_run_and_an_override_does() {
    let project = ProjectId::generate();
    let policy = || {
        Some(CalendarPolicyEvidence {
            profile_id: CalendarProfileId::generate(),
            policy_revision: SpecVersion::FIRST,
            timezone: IanaTimeZone::parse("Europe/Oslo").expect("a known zone"),
            matched_window: Some(name("weekday-daytime")),
        })
    };

    let mut closed = candidate(project, TaskId::generate());
    closed.calendar = CalendarAdmission {
        state: EffectiveCalendarState::Closed,
        policy: policy(),
        override_id: None,
        next_opening: Some(at("2026-08-13T07:00:00Z")),
    };
    let closed_task = closed.task_id;
    assert_refused(
        &plan(&snapshot(vec![closed])).expect("the pass runs"),
        closed_task,
        RejectionCode::CalendarClosed,
    );

    let mut draining = candidate(project, TaskId::generate());
    draining.calendar = CalendarAdmission {
        state: EffectiveCalendarState::Draining,
        policy: policy(),
        override_id: None,
        next_opening: None,
    };
    let draining_task = draining.task_id;
    assert_refused(
        &plan(&snapshot(vec![draining])).expect("the pass runs"),
        draining_task,
        RejectionCode::CalendarDraining,
    );

    let mut overridden = candidate(project, TaskId::generate());
    overridden.calendar = CalendarAdmission {
        state: EffectiveCalendarState::OverrideOpen,
        policy: policy(),
        override_id: Some(kontor_core::id::ScheduleOverrideId::generate()),
        next_opening: None,
    };
    let overridden_task = overridden.task_id;
    assert_admitted(
        &plan(&snapshot(vec![overridden])).expect("the pass runs"),
        overridden_task,
    );
}

#[test]
fn an_inconsistent_calendar_answer_refuses_the_pass() {
    let project = ProjectId::generate();
    let mut lying = candidate(project, TaskId::generate());
    // `override_open` with no override is an answer nobody can act on.
    lying.calendar = CalendarAdmission {
        state: EffectiveCalendarState::OverrideOpen,
        policy: Some(CalendarPolicyEvidence {
            profile_id: CalendarProfileId::generate(),
            policy_revision: SpecVersion::FIRST,
            timezone: IanaTimeZone::parse("Europe/Oslo").expect("a known zone"),
            matched_window: None,
        }),
        override_id: None,
        next_opening: None,
    };
    assert!(plan(&snapshot(vec![lying])).is_err());
}

// ---------------------------------------------------------------------------
// Runtime and account trust
// ---------------------------------------------------------------------------

#[test]
fn an_advisory_grade_runtime_is_never_eligible() {
    let project = ProjectId::generate();
    for grade in [TrustGrade::A, TrustGrade::B, TrustGrade::C] {
        let mut routed = candidate(project, TaskId::generate());
        routed.runtime.capabilities = capabilities(grade, 100);
        let task = routed.task_id;
        let decided = plan(&snapshot(vec![routed])).expect("the pass runs");
        if grade == TrustGrade::C {
            assert_refused(&decided, task, RejectionCode::RuntimeTrustInsufficient);
        } else {
            assert_admitted(&decided, task);
        }
    }
}

#[test]
fn an_undeclared_capability_is_refused_before_trust_is_considered() {
    let project = ProjectId::generate();
    let mut incapable = candidate(project, TaskId::generate());
    // Grade C *and* missing a capability. The capability check runs first, so a
    // pass that reported insufficient trust would have run them out of order.
    incapable.runtime.capabilities = capabilities(TrustGrade::C, 100);
    incapable
        .runtime
        .capabilities
        .supported
        .remove(&RuntimeCapability::Launch);
    let task = incapable.task_id;
    assert_refused(
        &plan(&snapshot(vec![incapable])).expect("the pass runs"),
        task,
        RejectionCode::RuntimeCapabilityMissing,
    );
}

#[test]
fn a_runtime_that_is_unhealthy_unreconciled_or_stale_blocks() {
    let project = ProjectId::generate();

    let mut degraded = candidate(project, TaskId::generate());
    degraded.runtime.health = RuntimeHealth::Degraded;
    let degraded_task = degraded.task_id;
    assert_refused(
        &plan(&snapshot(vec![degraded])).expect("the pass runs"),
        degraded_task,
        RejectionCode::RuntimeUnhealthy,
    );

    // Every way reconciliation can be incomplete, including a census of the same
    // host in an earlier generation — which proves nothing about the generation
    // now answering.
    let mutations: Vec<ReconciliationCase> = vec![
        ("no census at all", |evidence| {
            evidence.epoch_completed = false;
        }),
        ("a census of another generation", |evidence| {
            evidence.scope.generation = 6;
        }),
        ("a census of another host", |evidence| {
            evidence.scope.host = ExternalName::parse("other-host").expect("a valid host");
        }),
        ("a census of another project", |evidence| {
            evidence.scope.project_id = ProjectId::generate();
        }),
        ("an open replay gap", |evidence| {
            evidence.open_replay_gap = true;
        }),
        ("an unresolved divergence", |evidence| {
            evidence.divergence = true;
        }),
        ("an ambiguous orphan", |evidence| {
            evidence.orphan_ambiguity = true;
        }),
        ("lost contact", |evidence| {
            evidence.stale_lost_contact = true;
        }),
    ];
    for (label, mutate) in mutations {
        let mut unreconciled = candidate(project, TaskId::generate());
        mutate(&mut unreconciled.runtime.reconciliation);
        let task = unreconciled.task_id;
        let decided = plan(&snapshot(vec![unreconciled])).expect("the pass runs");
        assert_eq!(
            decision_for(&decided, task).rejection_code(),
            Some(RejectionCode::RuntimeReconciliationIncomplete),
            "{label} must block admission"
        );
    }

    for last_confirmed in [None, Some(at("2026-08-12T08:00:00Z"))] {
        let mut stale = candidate(project, TaskId::generate());
        stale.runtime.last_confirmed_at = last_confirmed;
        let task = stale.task_id;
        assert_refused(
            &plan(&snapshot(vec![stale])).expect("the pass runs"),
            task,
            RejectionCode::RuntimeEvidenceStale,
        );
    }
}

#[test]
fn a_pinned_account_must_be_current_enabled_warm_compatible_and_preflighted() {
    let project = ProjectId::generate();
    let account = AccountProfileId::generate();

    let cases: Vec<CandidateCase> = vec![
        (
            RejectionCode::AccountPinStale,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    pin.current_revision = AggregateRevision::parse(2).expect("a revision");
                }
            }),
        ),
        (
            RejectionCode::AccountDisabled,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    pin.enabled = false;
                }
            }),
        ),
        (
            RejectionCode::AccountCoolingDown,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    pin.cooldown_until = Some(at("2026-08-12T10:00:00Z"));
                }
            }),
        ),
        (
            RejectionCode::AccountRuntimeIncompatible,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    pin.harness = RuntimeKindKey::parse("rb.other").expect("a valid runtime key");
                }
            }),
        ),
        (
            RejectionCode::AccountCapabilityMissing,
            Box::new(|candidate: &mut Candidate| {
                candidate
                    .account
                    .required_capabilities
                    .insert(AccountCapabilityKey::parse("rb.long-context").expect("a valid key"));
            }),
        ),
        (
            RejectionCode::AccountEnvironmentUnavailable,
            Box::new(|candidate: &mut Candidate| {
                candidate.runtime.capabilities.account_env = false;
            }),
        ),
        (
            RejectionCode::FleetPreflightFailed,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    pin.preflight.outcome = PreflightOutcome::Failed;
                }
            }),
        ),
        (
            RejectionCode::FleetPreflightFailed,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    pin.preflight.outcome = PreflightOutcome::Absent;
                }
            }),
        ),
        (
            RejectionCode::FleetPreflightFailed,
            Box::new(|candidate: &mut Candidate| {
                if let Some(pin) = candidate.account.pin.as_mut() {
                    // Passed, but a week ago. A stale probe is not evidence
                    // about now.
                    pin.preflight.observed_at = at("2026-08-05T09:00:00Z");
                }
            }),
        ),
    ];

    // The baseline admits, so each refusal below is caused by the mutation.
    let mut baseline = candidate(project, TaskId::generate());
    baseline.account.pin = Some(pin(account));
    let baseline_task = baseline.task_id;
    assert_admitted(
        &plan(&snapshot(vec![baseline.clone()])).expect("the pass runs"),
        baseline_task,
    );

    for (code, mutate) in cases {
        let mut pinned = candidate(project, TaskId::generate());
        pinned.account.pin = Some(pin(account));
        mutate(&mut pinned);
        let task = pinned.task_id;
        let decided = plan(&snapshot(vec![pinned])).expect("the pass runs");
        assert_eq!(
            decision_for(&decided, task).rejection_code(),
            Some(code),
            "expected {code}"
        );
    }
}

// ---------------------------------------------------------------------------
// External ownership
// ---------------------------------------------------------------------------

#[test]
fn an_unresolved_conflict_or_a_foreign_owner_blocks_without_taking_anything_over() {
    let project = ProjectId::generate();
    let acting = ExternalId::parse("kontor-bot").expect("a valid principal");

    let mut conflicted = candidate(project, TaskId::generate());
    conflicted
        .external
        .blocking_conflicts
        .insert(kontor_core::id::StatusConflictId::generate());
    let conflicted_task = conflicted.task_id;
    let decided = plan(&snapshot(vec![conflicted.clone()])).expect("the pass runs");
    assert_refused(
        &decided,
        conflicted_task,
        RejectionCode::ExternalConflictUnresolved,
    );
    // The refusal changes nothing about the task itself: the pass returns
    // decisions and holds no writeable state at all.
    assert_eq!(conflicted.state, TaskState::Ready);

    let ownership = |owner: Option<&str>, confirmed: bool| ExternalOwnership {
        connector: kontor_core::id::ConnectorKey::parse("rb.connector").expect("a valid connector"),
        spec_version: SpecVersion::FIRST,
        ownership_milestone: kontor_core::id::SemanticMilestoneKey::parse("rb.in-progress")
            .expect("a valid milestone"),
        milestone_confirmed: confirmed,
        owning_principal: owner.map(|text| ExternalId::parse(text).expect("a valid principal")),
        acting_principal: acting.clone(),
    };

    let mut foreign = candidate(project, TaskId::generate());
    foreign.external.ownership = Some(ownership(Some("someone-else"), true));
    let foreign_task = foreign.task_id;
    assert_refused(
        &plan(&snapshot(vec![foreign])).expect("the pass runs"),
        foreign_task,
        RejectionCode::ExternalOwnershipConflict,
    );

    let mut unconfirmed = candidate(project, TaskId::generate());
    unconfirmed.external.ownership = Some(ownership(Some("kontor-bot"), false));
    let unconfirmed_task = unconfirmed.task_id;
    assert_refused(
        &plan(&snapshot(vec![unconfirmed])).expect("the pass runs"),
        unconfirmed_task,
        RejectionCode::OwnershipMilestoneUnconfirmed,
    );

    let mut ours = candidate(project, TaskId::generate());
    ours.external.ownership = Some(ownership(Some("kontor-bot"), true));
    let ours_task = ours.task_id;
    assert_admitted(
        &plan(&snapshot(vec![ours])).expect("the pass runs"),
        ours_task,
    );
}

// ---------------------------------------------------------------------------
// Origin
// ---------------------------------------------------------------------------

#[test]
fn event_origin_work_needs_its_receipt_and_manual_work_needs_none() {
    let project = ProjectId::generate();

    let manual = candidate(project, TaskId::generate());
    let manual_task = manual.task_id;
    assert_admitted(
        &plan(&snapshot(vec![manual])).expect("the pass runs"),
        manual_task,
    );

    let mut absent = candidate(project, TaskId::generate());
    absent.origin = TaskOrigin::Event { lineage: None };
    let absent_task = absent.task_id;
    assert_refused(
        &plan(&snapshot(vec![absent])).expect("the pass runs"),
        absent_task,
        RejectionCode::IntakeReceiptMissing,
    );

    let lineage = |task_id: TaskId, result: IntakeResult, auto_arm: bool| TaskOrigin::Event {
        lineage: Some(IntakeLineage {
            receipt_id: IntakeReceiptId::generate(),
            result,
            armed_task_id: task_id,
            auto_arm_authorization: auto_arm.then(ExecutionAuthorizationId::generate),
        }),
    };

    // Approved arms; every other bare result does not; an explicitly authorized
    // auto-arm arms a proposal and nothing else.
    let cases = [
        (IntakeResult::Approved, false, None),
        (
            IntakeResult::Proposed,
            false,
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
        (IntakeResult::Proposed, true, None),
        (
            IntakeResult::Rejected,
            true,
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
        (
            IntakeResult::Ignored,
            true,
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
        (
            IntakeResult::Duplicate,
            true,
            Some(RejectionCode::IntakeReceiptNotApproved),
        ),
    ];
    for (result, auto_arm, expected) in cases {
        let mut triggered = candidate(project, TaskId::generate());
        triggered.origin = lineage(triggered.task_id, result, auto_arm);
        let task = triggered.task_id;
        let decided = plan(&snapshot(vec![triggered])).expect("the pass runs");
        assert_eq!(
            decision_for(&decided, task).rejection_code(),
            expected,
            "{result} with auto_arm={auto_arm}"
        );
    }

    // A receipt that armed *other* work is not this task's authority, however
    // approved it was.
    let mut mismatched = candidate(project, TaskId::generate());
    mismatched.origin = lineage(TaskId::generate(), IntakeResult::Approved, false);
    let mismatched_task = mismatched.task_id;
    assert_refused(
        &plan(&snapshot(vec![mismatched])).expect("the pass runs"),
        mismatched_task,
        RejectionCode::IntakeReceiptMismatched,
    );
}

// ---------------------------------------------------------------------------
// Capacity
// ---------------------------------------------------------------------------

/// Every ceiling, one at a time: the batch is bounded by the smallest.
#[test]
fn the_batch_never_exceeds_the_smallest_ceiling_that_applies() {
    let project = ProjectId::generate();
    let mission = MiniProjectId::generate();
    let account = AccountProfileId::generate();

    let build = |count: usize| -> Vec<Candidate> {
        (0..count)
            .map(|index| {
                let mut candidate = candidate(project, TaskId::generate());
                // Descending priority, so the order is fixed and the cut is
                // where the ceiling says rather than wherever the ids fell.
                candidate.priority = u32::try_from(900 - index).expect("in range");
                candidate.mini_project_id = Some(mission);
                candidate.account.pin = Some(pin(account));
                candidate
            })
            .collect()
    };

    let narrow: Vec<CeilingCase> = vec![
        (
            CapacityLimitKind::Global,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                snapshot.capacity.global_max_in_flight = 2;
            }),
        ),
        (
            CapacityLimitKind::Project,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                snapshot.capacity.project_max_in_flight = 2;
            }),
        ),
        (
            CapacityLimitKind::Mission,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                snapshot.capacity.mission_max_in_flight = 2;
            }),
        ),
        (
            CapacityLimitKind::Account,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                snapshot.capacity.account_max_in_flight = 2;
            }),
        ),
        (
            CapacityLimitKind::Provider,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                snapshot.capacity.provider_max_in_flight = 2;
            }),
        ),
        (
            CapacityLimitKind::Runtime,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                snapshot.capacity.runtime_max_in_flight = 2;
            }),
        ),
        (
            CapacityLimitKind::RuntimeSessions,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                for candidate in &mut snapshot.candidates {
                    candidate
                        .runtime
                        .capabilities
                        .limits
                        .max_concurrent_sessions = 2;
                }
            }),
        ),
        (
            CapacityLimitKind::Authorization,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                for candidate in &mut snapshot.candidates {
                    if let Some(authorization) = candidate.authorization.as_mut() {
                        authorization.max_concurrency = 2;
                    }
                }
            }),
        ),
        (
            CapacityLimitKind::AdaptiveWindow,
            Box::new(|snapshot: &mut SchedulingSnapshot| {
                let config = AdaptiveWindowConfig {
                    initial: 2,
                    floor: 1,
                    ceiling: 8,
                    growth_step: 1,
                };
                snapshot.capacity.adaptive = config;
                snapshot.adaptive_window = AdaptiveWindow::start(config);
            }),
        ),
    ];

    for (limit, narrow) in narrow {
        let mut restricted = snapshot(build(5));
        narrow(&mut restricted);
        let decided = plan(&restricted).expect("the pass runs");
        assert_eq!(
            decided.admitted_count(),
            2,
            "{limit} caps the batch at two admissions"
        );
        let refused: Vec<RejectionCode> = decided
            .decisions
            .iter()
            .filter_map(CandidateDecision::rejection_code)
            .collect();
        assert_eq!(
            refused,
            vec![RejectionCode::CapacityExhausted; 3],
            "{limit} refuses the rest as a capacity problem and still decides them"
        );
        // The record names which ceiling bound the pass, so an operator does not
        // have to re-derive it.
        let binding: BTreeSet<CapacityLimitKind> = decided
            .batch()
            .map(|admitted| admitted.capacity.binding)
            .collect();
        assert!(
            binding.contains(&limit),
            "{limit} must be the ceiling recorded as binding, got {binding:?}"
        );
    }
}

#[test]
fn headroom_already_spent_counts_against_the_ceiling() {
    let project = ProjectId::generate();
    let mut spent = snapshot(vec![candidate(project, TaskId::generate())]);
    spent.capacity.global_max_in_flight = 3;
    spent.usage.global_in_flight = 3;
    let task = spent.candidates[0].task_id;
    assert_refused(
        &plan(&spent).expect("the pass runs"),
        task,
        RejectionCode::CapacityExhausted,
    );
}

#[test]
fn a_ceiling_that_does_not_apply_is_not_consulted() {
    let project = ProjectId::generate();
    // No goal and no account: mission, account and provider must be absent from
    // the record rather than recorded as unlimited.
    let mut unpinned = snapshot(vec![candidate(project, TaskId::generate())]);
    unpinned.capacity.mission_max_in_flight = 1;
    let decided = plan(&unpinned).expect("the pass runs");
    let admitted = decided.batch().next().expect("one admission");
    let consulted: BTreeSet<CapacityLimitKind> =
        admitted.capacity.remaining.keys().copied().collect();
    assert!(!consulted.contains(&CapacityLimitKind::Mission));
    assert!(!consulted.contains(&CapacityLimitKind::Account));
    assert!(!consulted.contains(&CapacityLimitKind::Provider));
    assert!(consulted.contains(&CapacityLimitKind::Global));
    assert!(consulted.contains(&CapacityLimitKind::Project));
}

#[test]
fn the_adaptive_window_grows_on_clean_observations_and_falls_to_the_floor_under_pressure() {
    let config = AdaptiveWindowConfig {
        initial: 4,
        floor: 2,
        ceiling: 7,
        growth_step: 1,
    };
    let mut window = AdaptiveWindow::start(config);
    assert_eq!(window.current(), 4);
    for expected in [5, 6, 7, 7, 7] {
        window = window.observe(config, CapacityObservation::Clean);
        assert_eq!(window.current(), expected, "growth stops at the ceiling");
    }
    window = window.observe(config, CapacityObservation::Pressure);
    assert_eq!(
        window.current(),
        config.floor,
        "pressure goes to the floor rather than stepping down"
    );
    window = window.observe(config, CapacityObservation::Pressure);
    assert_eq!(window.current(), config.floor, "and stays there");

    // A persisted width outside the band is clamped, so a configuration change
    // takes effect on the next pass instead of failing it.
    assert_eq!(AdaptiveWindow::restore(config, 99).current(), 7);
    assert_eq!(AdaptiveWindow::restore(config, 0).current(), 2);
}

#[test]
fn narrowing_the_window_below_the_work_in_flight_cancels_nothing() {
    let project = ProjectId::generate();
    let config = AdaptiveWindowConfig {
        initial: 4,
        floor: 2,
        ceiling: 7,
        growth_step: 1,
    };
    let mut narrowed = snapshot(vec![candidate(project, TaskId::generate())]);
    narrowed.capacity.adaptive = config;
    // Five runs are already in flight and the window has fallen to two. The pass
    // admits nothing new; what it must not do is produce anything that stops the
    // five — and it structurally cannot, because a `Plan` has no cancellation.
    narrowed.adaptive_window =
        AdaptiveWindow::start(config).observe(config, CapacityObservation::Pressure);
    narrowed.usage.global_in_flight = 5;
    narrowed.capacity.global_max_in_flight = 5;

    let decided = plan(&narrowed).expect("the pass runs");
    assert_eq!(decided.admitted_count(), 0);
    assert_eq!(
        decided.decisions.len(),
        1,
        "a pass under pressure decides its candidates and nothing else"
    );
    assert!(
        decided
            .decisions
            .iter()
            .all(|decision| decision.rejection_code() == Some(RejectionCode::CapacityExhausted))
    );
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

#[test]
fn an_admission_records_the_inputs_it_was_ordered_and_sized_on() {
    let project = ProjectId::generate();
    let account = AccountProfileId::generate();
    let mut admitted = candidate(project, TaskId::generate());
    admitted.priority = 750;
    admitted.module = Some(module("directory.app"));
    admitted.worktree = Some(WorktreeClaim {
        worktree: name("/trees/one"),
        verification: WorktreeVerification::Verified,
    });
    admitted.account.pin = Some(pin(account));
    let expected_authorization = admitted
        .authorization
        .as_ref()
        .expect("the fixture arms the task")
        .id;
    let task = admitted.task_id;

    let decided = plan(&snapshot(vec![admitted])).expect("the pass runs");
    let record = decided.batch().next().expect("one admission");
    assert_eq!(record.task_id, task);
    assert_eq!(record.ordering.priority, 750);
    assert_eq!(record.ordering.task_id, task);
    assert_eq!(record.authorization_id, expected_authorization);
    assert_eq!(record.account_profile_id, Some(account));
    assert_eq!(record.module, Some(module("directory.app")));
    assert_eq!(record.worktree, Some(name("/trees/one")));
    assert_eq!(record.runtime_generation, 7);
    assert!(record.capacity.effective > 0);

    // The batch is a projection of the decisions, not a second list: every
    // admitted task appears exactly once in both.
    let admitted_tasks: Vec<TaskId> = decided.batch().map(|record| record.task_id).collect();
    let from_decisions: Vec<TaskId> = decided
        .decisions
        .iter()
        .filter(|decision| decision.rejection_code().is_none())
        .map(CandidateDecision::task_id)
        .collect();
    assert_eq!(admitted_tasks, from_decisions);
}

#[test]
fn an_unverified_tree_never_reaches_the_record_as_isolation() {
    let project = ProjectId::generate();
    let mut unverified = candidate(project, TaskId::generate());
    unverified.worktree = Some(WorktreeClaim {
        worktree: name("/trees/claimed"),
        verification: WorktreeVerification::Unverified,
    });
    // It is refused, so nothing reaches the record at all — and the claim it
    // would have made carries no tree.
    assert!(unverified.module_claim().is_none());
    let mut with_module = unverified.clone();
    with_module.module = Some(module("directory.app"));
    assert_eq!(
        with_module.module_claim().expect("a module claim").worktree,
        None,
        "an unverified claim isolates nothing"
    );
}

/// A decision that cannot be canonicalized cannot be stored, so every shape the
/// pass can produce has to survive [`CanonicalDocument`].
///
/// This is not a formality. The document rejects any key a credential could hide
/// behind — `token`, `credential`, `authorization` — so a field innocently named
/// `authorization` would make every admission unstorable, and it would fail at
/// the store rather than here. Both an admission and a refusal are checked.
#[test]
fn every_decision_shape_canonicalizes_for_storage() {
    let project = ProjectId::generate();
    let account = AccountProfileId::generate();

    let mut admitted = candidate(project, TaskId::generate());
    admitted.priority = 900;
    admitted.module = Some(module("directory.app"));
    admitted.worktree = Some(WorktreeClaim {
        worktree: name("/trees/one"),
        verification: WorktreeVerification::Verified,
    });
    let capability =
        AccountCapabilityKey::parse("rb.long-context").expect("a valid capability key");
    let mut declared = pin(account);
    declared.declared_capabilities.insert(capability.clone());
    admitted.account.pin = Some(declared);
    admitted.account.required_capabilities.insert(capability);

    // One of each: an admission, and refusals carrying every evidence variant a
    // blocker can attach.
    let mut refused_dependency = candidate(project, TaskId::generate());
    refused_dependency.depends_on.insert(TaskId::generate());
    let mut refused_runtime = candidate(project, TaskId::generate());
    refused_runtime.runtime.health = RuntimeHealth::Degraded;
    let mut refused_calendar = candidate(project, TaskId::generate());
    refused_calendar.calendar = CalendarAdmission {
        state: EffectiveCalendarState::Closed,
        policy: Some(CalendarPolicyEvidence {
            profile_id: CalendarProfileId::generate(),
            policy_revision: SpecVersion::FIRST,
            timezone: IanaTimeZone::parse("Europe/Oslo").expect("a known zone"),
            matched_window: Some(name("weekday-daytime")),
        }),
        override_id: None,
        next_opening: Some(at("2026-08-13T07:00:00Z")),
    };
    let mut refused_authorization = candidate(project, TaskId::generate());
    refused_authorization.authorization = None;
    let mut refused_intake = candidate(project, TaskId::generate());
    refused_intake.origin = TaskOrigin::Event {
        lineage: Some(IntakeLineage {
            receipt_id: IntakeReceiptId::generate(),
            result: IntakeResult::Rejected,
            armed_task_id: refused_intake.task_id,
            auto_arm_authorization: None,
        }),
    };
    let mut refused_conflict = candidate(project, TaskId::generate());
    refused_conflict
        .external
        .blocking_conflicts
        .insert(kontor_core::id::StatusConflictId::generate());

    let decided = plan(&snapshot(vec![
        admitted,
        refused_dependency,
        refused_runtime,
        refused_calendar,
        refused_authorization,
        refused_intake,
        refused_conflict,
    ]))
    .expect("the pass runs");
    assert_eq!(decided.admitted_count(), 1);
    // Once for the whole plan, and once per decision — the store persists the
    // per-decision evidence, so each one has to canonicalize on its own too.
    let _ = bytes(&decided);
    for decision in &decided.decisions {
        CanonicalDocument::from_serializable(&serde_json::json!({
            "schema_version": 1,
            "decision": decision,
        }))
        .expect("every decision canonicalizes on its own");
    }
}

#[test]
fn every_candidate_is_decided_exactly_once() {
    let project = ProjectId::generate();
    let mut candidates = Vec::new();
    for index in 0..6 {
        let mut candidate = candidate(project, TaskId::generate());
        if index % 2 == 0 {
            candidate.state = TaskState::Todo;
        }
        candidates.push(candidate);
    }
    let expected: BTreeSet<TaskId> = candidates
        .iter()
        .map(|candidate| candidate.task_id)
        .collect();

    let decided = plan(&snapshot(candidates)).expect("the pass runs");
    assert_eq!(decided.decisions.len(), expected.len());
    let seen: BTreeMap<TaskId, usize> =
        decided
            .decisions
            .iter()
            .fold(BTreeMap::new(), |mut counts, decision| {
                *counts.entry(decision.task_id()).or_insert(0) += 1;
                counts
            });
    assert!(seen.values().all(|count| *count == 1));
    assert_eq!(seen.keys().copied().collect::<BTreeSet<TaskId>>(), expected);
}
