//! Every bound a bounded auto-arm claims, refused one at a time.
//!
//! The rule under test is `TriggerSpec::authorize_auto_arm`, which is the single
//! copy: this suite proves each refusal in isolation, and the store suite proves
//! the same function is what a transaction re-runs before it creates work.
//!
//! The mutants this suite exists to kill:
//!
//! * an auto-arm that fires under an `approval_required` policy;
//! * an auto-arm exercised by an account the capability was not granted to;
//! * an authorization accepted outside its own window, or over work it does not
//!   cover — including a graph where only *some* tasks are covered;
//! * a concurrency ceiling read from the widest of the three bounds instead of
//!   the narrowest;
//! * a budget that exceeds the grant being treated as "bounded" because it is
//!   non-zero.

mod fixture;

use fixture::{at, budget, trigger};
use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::id::{
    AccountProfileId, CommandReceiptId, ExecutionAuthorizationId, ExternalName, MiniProjectId,
    ProjectId, TaskId, Timestamp,
};
use kontor_core::repository::{IntakeWorkPlan, NewTask};
use kontor_core::spec::{
    AutoArmPolicy, AutoArmRefusal, BudgetBounds, ExecutionCapability, TriggerSpec,
};
use kontor_core::state::TaskState;
use kontor_intake::authorize_auto_arm;

const DECIDED_AT: &str = "2026-08-12T09:00:00Z";

struct Bench {
    project: ProjectId,
    caller: AccountProfileId,
    authorization_id: ExecutionAuthorizationId,
    trigger: TriggerSpec,
    tasks: Vec<TaskId>,
}

fn bench() -> Bench {
    let caller = AccountProfileId::generate();
    let authorization_id = ExecutionAuthorizationId::generate();
    let mut spec = trigger(
        "trigger.armed",
        "monitoring",
        "conn.alpha",
        &[("/attributes/kind", "work.requested")],
    );
    spec.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: caller,
            execution_authorization: authorization_id,
        },
        max_concurrency: 2,
        budget: budget(),
    };
    Bench {
        project: ProjectId::generate(),
        caller,
        authorization_id,
        trigger: spec,
        tasks: vec![TaskId::generate()],
    }
}

fn authorization(bench: &Bench, scope: WorkScope, window: (&str, &str)) -> ExecutionAuthorization {
    ExecutionAuthorization {
        id: bench.authorization_id,
        project_id: bench.project,
        scope,
        selected_tasks: Vec::new(),
        allowed_start: TimeRange {
            start: at(window.0),
            end: at(window.1),
        },
        max_concurrency: 4,
        budget: budget(),
        created_by: bench.caller,
        capability_receipt: CommandReceiptId::generate(),
        created_at: at(DECIDED_AT),
    }
}

fn open(bench: &Bench) -> ExecutionAuthorization {
    authorization(
        bench,
        WorkScope::Project,
        ("2026-08-12T08:00:00Z", "2026-08-12T18:00:00Z"),
    )
}

fn work(bench: &Bench, goal: Option<MiniProjectId>) -> IntakeWorkPlan {
    IntakeWorkPlan {
        mini_project: None,
        tasks: bench
            .tasks
            .iter()
            .map(|id| NewTask {
                id: *id,
                project_id: bench.project,
                mini_project_id: goal,
                title: ExternalName::parse("Armed work").expect("a legal title"),
                module: None,
                state: TaskState::Ready,
                created_at: at(DECIDED_AT),
            })
            .collect(),
    }
}

fn arm(
    bench: &Bench,
    authorization: &ExecutionAuthorization,
    caller: AccountProfileId,
    at_instant: Timestamp,
) -> Result<ExecutionCapability, AutoArmRefusal> {
    authorize_auto_arm(
        &bench.trigger,
        &work(bench, None),
        caller,
        authorization,
        at_instant,
    )
}

#[test]
fn a_fully_bounded_auto_arm_is_authorized() {
    let bench = bench();
    let capability =
        arm(&bench, &open(&bench), bench.caller, at(DECIDED_AT)).expect("every bound is met");
    assert_eq!(capability.granted_to, bench.caller);
    assert_eq!(
        capability.execution_authorization, bench.authorization_id,
        "the capability names the authorization the work is armed under"
    );
}

#[test]
fn an_approval_required_policy_never_auto_arms() {
    let mut bench = bench();
    bench.trigger.approval = AutoArmPolicy::ApprovalRequired;
    assert_eq!(
        arm(&bench, &open(&bench), bench.caller, at(DECIDED_AT)),
        Err(AutoArmRefusal::PolicyRequiresApproval)
    );
}

#[test]
fn only_the_account_the_capability_names_may_exercise_it() {
    let bench = bench();
    assert_eq!(
        arm(
            &bench,
            &open(&bench),
            AccountProfileId::generate(),
            at(DECIDED_AT)
        ),
        Err(AutoArmRefusal::CallerNotGranted)
    );
}

#[test]
fn another_authorization_is_not_this_policys_authorization() {
    let bench = bench();
    let mut other = open(&bench);
    other.id = ExecutionAuthorizationId::generate();
    assert_eq!(
        arm(&bench, &other, bench.caller, at(DECIDED_AT)),
        Err(AutoArmRefusal::AuthorizationMismatched)
    );
}

#[test]
fn an_expired_window_arms_nothing() {
    let bench = bench();
    let expired = authorization(
        &bench,
        WorkScope::Project,
        ("2026-08-11T08:00:00Z", "2026-08-11T18:00:00Z"),
    );
    assert_eq!(
        arm(&bench, &expired, bench.caller, at(DECIDED_AT)),
        Err(AutoArmRefusal::AuthorizationOutOfWindow)
    );
}

#[test]
fn every_created_task_has_to_be_covered_not_merely_the_first() {
    let mut bench = bench();
    bench.tasks = vec![TaskId::generate(), TaskId::generate()];
    let mut narrow = open(&bench);
    // Selected tasks name only one half of the graph being created.
    narrow.selected_tasks = vec![bench.tasks[0]];
    assert_eq!(
        arm(&bench, &narrow, bench.caller, at(DECIDED_AT)),
        Err(AutoArmRefusal::AuthorizationScopeMismatched)
    );

    // A goal-scoped grant does not cover work filed under another goal either.
    let elsewhere = authorization(
        &bench,
        WorkScope::MiniProject {
            mini_project_id: MiniProjectId::generate(),
        },
        ("2026-08-12T08:00:00Z", "2026-08-12T18:00:00Z"),
    );
    assert_eq!(
        authorize_auto_arm(
            &bench.trigger,
            &work(&bench, Some(MiniProjectId::generate())),
            bench.caller,
            &elsewhere,
            at(DECIDED_AT)
        ),
        Err(AutoArmRefusal::AuthorizationScopeMismatched)
    );
}

#[test]
fn a_proposal_that_creates_nothing_arms_nothing() {
    let bench = bench();
    let empty = IntakeWorkPlan {
        mini_project: None,
        tasks: Vec::new(),
    };
    assert_eq!(
        authorize_auto_arm(
            &bench.trigger,
            &empty,
            bench.caller,
            &open(&bench),
            at(DECIDED_AT)
        ),
        Err(AutoArmRefusal::NoWorkProposed)
    );
}

#[test]
fn the_narrowest_of_the_three_concurrency_bounds_wins() {
    // Three tasks, and each of the three ceilings lowered to two in turn. The
    // policy, the trigger's own limits and the authorization each have to be
    // able to refuse alone.
    let mut bench = bench();
    bench.tasks = vec![TaskId::generate(), TaskId::generate(), TaskId::generate()];
    bench.trigger.limits.max_concurrency = 3;
    bench.trigger.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: bench.caller,
            execution_authorization: bench.authorization_id,
        },
        max_concurrency: 3,
        budget: budget(),
    };
    assert!(
        arm(&bench, &open(&bench), bench.caller, at(DECIDED_AT)).is_ok(),
        "three tasks under three ceilings of three is allowed"
    );

    let mut policy_bound = bench.trigger.clone();
    policy_bound.approval = AutoArmPolicy::BoundedAutoArm {
        capability: ExecutionCapability {
            granted_to: bench.caller,
            execution_authorization: bench.authorization_id,
        },
        max_concurrency: 2,
        budget: budget(),
    };
    let mut limit_bound = bench.trigger.clone();
    limit_bound.limits.max_concurrency = 2;
    let mut authorization_bound = open(&bench);
    authorization_bound.max_concurrency = 2;

    for (label, spec, authorization) in [
        ("policy", policy_bound, open(&bench)),
        ("trigger limits", limit_bound, open(&bench)),
        ("authorization", bench.trigger.clone(), authorization_bound),
    ] {
        let narrowed = Bench {
            project: bench.project,
            caller: bench.caller,
            authorization_id: bench.authorization_id,
            trigger: spec,
            tasks: bench.tasks.clone(),
        };
        assert_eq!(
            arm(&narrowed, &authorization, bench.caller, at(DECIDED_AT)),
            Err(AutoArmRefusal::ConcurrencyExceeded),
            "{label} alone must be able to refuse"
        );
    }
}

#[test]
fn a_declared_budget_may_never_exceed_the_grant() {
    let bench = bench();
    let mut lean = open(&bench);
    lean.budget = BudgetBounds {
        max_tokens: 10,
        ..budget()
    };
    assert_eq!(
        arm(&bench, &lean, bench.caller, at(DECIDED_AT)),
        Err(AutoArmRefusal::BudgetExceeded),
        "a policy cannot widen its own authorization by naming a larger number"
    );

    // A cost ceiling in another currency is not a smaller one.
    let mut foreign = open(&bench);
    foreign.budget = BudgetBounds {
        max_cost: kontor_core::id::Money {
            minor_units: 1_000_000,
            currency: kontor_core::id::CurrencyCode::parse("EUR").expect("a legal currency"),
        },
        ..budget()
    };
    assert_eq!(
        arm(&bench, &foreign, bench.caller, at(DECIDED_AT)),
        Err(AutoArmRefusal::BudgetExceeded)
    );
}
