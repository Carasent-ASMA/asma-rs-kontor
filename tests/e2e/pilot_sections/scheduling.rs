//! Section 2/4 — module collision, worktree isolation and calendar admission.
//!
//! Everything here runs against `kontor_scheduler::ready::plan`, which reads no
//! clock, no database and no environment: the whole decision arrives in the
//! snapshot. That is what makes the client-clock criterion provable at all — a
//! surface has nowhere to put a time the scheduler would believe.

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil;
use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, CalendarResolution, EffectiveCalendarState,
    ExceptionKind, ExceptionProvenance, HolidayMergePolicy, IanaTimeZone, OverrideExpiry,
    ScheduleOverride, Weekday, WeeklyWindow, WorkCalendarAssignment, WorkScope,
    resolve_effective_state,
};
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CalendarExceptionId, CalendarProfileId, CommandReceiptId,
    CurrencyCode, ExternalName, ModuleKey, Money, ProjectId, SCHEMA_VERSION, ScheduleOverrideId,
    SpecVersion, TaskId, TaskWorkflowId, Timestamp, WorkCalendarId,
};
use kontor_core::spec::BudgetBounds;
use kontor_core::state::TaskState;
use kontor_policy::ModuleClaim;
use kontor_runtime::capability::{RuntimeCapabilities, RuntimeCapability, TrustGrade};
use kontor_scheduler::model::{
    AccountAdmissionEvidence, AdaptiveWindow, AdaptiveWindowConfig, AuthorizationEvidence,
    CalendarAdmission, CalendarPolicyEvidence, Candidate, CapacityConfig, CapacityUsage,
    ExternalWorkEvidence, ReconciliationEvidence, ReconciliationScope, RejectionCode,
    RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin, WorktreeClaim,
    WorktreeVerification,
};
use kontor_scheduler::ready::{explain, minimum_launch_capabilities, plan};
use kontor_tests_e2e::Bundle;
use serde_json::json;

use super::fixture::PilotProject;
use crate::{PROJECT_FIXTURE, at};

/// The pilot's fixed decision instant. Inside the pilot calendar's Wednesday
/// 09:00–17:00 Oslo window, so "open" is a fact about the fixture rather than
/// about when the suite happens to run.
const DECIDED_AT: &str = "2026-08-12T09:00:00Z";

/// Run every scheduling and calendar criterion.
pub(crate) fn run(bundle: &mut Bundle) {
    let fixture = PilotProject::parse(PROJECT_FIXTURE);
    worktrees_and_collision(bundle, &fixture);
    calendar_unrestricted_admits_unarmed(bundle);
    calendar_configured(bundle);
    calendar_ignores_client_clocks(bundle);
}

// ---------------------------------------------------------------------------
// Worktrees and the collision contender
// ---------------------------------------------------------------------------

/// The five safe tasks overlap in time on distinct verified trees; the sixth
/// shares a module without one and is refused.
fn worktrees_and_collision(bundle: &mut Bundle, fixture: &PilotProject) {
    let project = ProjectId::generate();
    let taken_at = at(DECIDED_AT);

    // Every isolated task, armed and ready, in one pass. They contend for five
    // different modules on five different verified trees, so nothing about
    // running them together is a collision.
    let mut candidates = Vec::new();
    let mut keys = Vec::new();
    for task in fixture.isolated() {
        let worktree = task
            .worktree
            .as_ref()
            .expect("an isolated pilot task declares its verified worktree");
        candidates.push(candidate(
            project,
            &task.module,
            Some(WorktreeClaim {
                worktree: name(worktree),
                verification: WorktreeVerification::Verified,
            }),
            taken_at,
        ));
        keys.push(json!({
            "key": task.key,
            "title": task.title,
            "module": task.module,
            "worktree": worktree,
        }));
    }

    let safe =
        plan(&snapshot(project, candidates.clone(), &[], taken_at)).expect("the ready pass runs");
    let admitted: Vec<String> = safe
        .batch()
        .map(|candidate| candidate.task_id.to_string())
        .collect();
    let distinct_trees: BTreeSet<String> = candidates
        .iter()
        .filter_map(|candidate| candidate.worktree.as_ref())
        .map(|claim| claim.worktree.to_string())
        .collect();

    let overlap = bundle
        .artifact(
            "snapshots/scheduling-safe-batch.json",
            &json!({
                "tasks": keys,
                "distinct_verified_worktrees": distinct_trees.len(),
                "admitted": admitted.len(),
                "decisions": safe
                    .decisions
                    .iter()
                    .map(|decision| json!({
                        "task_id": decision.task_id().to_string(),
                        "rejection": decision.rejection_code().map(|code| code.to_string()),
                    }))
                    .collect::<Vec<_>>(),
            }),
        )
        .expect("the safe batch is written");

    if admitted.len() == 5 && distinct_trees.len() == 5 {
        bundle.pass(
            "project.worktrees",
            "five pilot tasks on five distinct verified worktrees were admitted in one ready \
             batch: overlapping in time is not contention when the trees differ",
            std::slice::from_ref(&overlap),
        );
    } else {
        bundle.fail(
            "project.worktrees",
            format!(
                "expected five admitted on five distinct trees, got {} admitted across {} trees",
                admitted.len(),
                distinct_trees.len()
            ),
        );
    }

    // Now the contender. It shares `pilot.code` with the admitted pilot-code
    // task and declares no tree at all, so no worktree can isolate it.
    let contender_seed = fixture.contender();
    let holder = TaskId::generate();
    let held = ModuleClaim {
        module: module(&contender_seed.module),
        task_id: holder,
        worktree: Some(name("pilot-code-tree")),
        in_flight: true,
    };
    let contender = candidate(project, &contender_seed.module, None, taken_at);
    let contender_task = contender.task_id;

    let refused = plan(&snapshot(
        project,
        vec![contender.clone()],
        std::slice::from_ref(&held),
        taken_at,
    ))
    .expect("the ready pass runs");
    let reasons = explain(
        &snapshot(
            project,
            vec![contender.clone()],
            std::slice::from_ref(&held),
            taken_at,
        ),
        &contender,
    )
    .expect("the full explanation runs");

    let code = refused
        .decisions
        .first()
        .and_then(kontor_scheduler::model::CandidateDecision::rejection_code);
    let ledger = bundle
        .artifact(
            "runtime/collision-refusal.json",
            &json!({
                "contender_task": contender_task.to_string(),
                "shares_module_with": holder.to_string(),
                "module": contender_seed.module,
                "contender_worktree": contender_seed.worktree,
                "holder_worktree": "pilot-code-tree",
                "admitted_count": refused.admitted_count(),
                "first_blocker": code.map(|code| code.to_string()),
                "every_blocker": reasons
                    .iter()
                    .map(|refusal| json!({
                        "blocker": refusal.blocker.to_string(),
                        "code": refusal.code.to_string(),
                    }))
                    .collect::<Vec<_>>(),
            }),
        )
        .expect("the refusal ledger is written");

    if refused.admitted_count() == 0 && code == Some(RejectionCode::ModuleInFlight) {
        bundle.pass(
            "project.collision-contender",
            format!(
                "the contender shares `{}` with an in-flight task and claims no tree, so the pass \
                 admitted nothing and refused it as `{}`",
                contender_seed.module,
                RejectionCode::ModuleInFlight
            ),
            std::slice::from_ref(&ledger),
        );
        bundle.pass(
            "negative.collision",
            format!(
                "two armed tasks over one module without distinct verified isolation refuse with \
                 `{}`; the holder's claim is untouched and no second candidate is admitted",
                RejectionCode::ModuleInFlight
            ),
            &[ledger, overlap],
        );
    } else {
        let detail = format!(
            "expected zero admitted and `module_in_flight`, got {} admitted and {code:?}",
            refused.admitted_count()
        );
        bundle.fail("project.collision-contender", detail.clone());
        bundle.fail("negative.collision", detail);
    }
}

// ---------------------------------------------------------------------------
// Calendar
// ---------------------------------------------------------------------------

/// An unconfigured project is unrestricted — and default-allow admits unarmed work.
fn calendar_unrestricted_admits_unarmed(bundle: &mut Bundle) {
    let project = ProjectId::generate();
    let taken_at = at(DECIDED_AT);

    let unconfigured = CalendarResolution {
        assignment: None,
        profile: None,
        exceptions: &[],
        schedule_override: None,
        mini_project: None,
        task: None,
        now: taken_at,
    };
    let state = resolve_effective_state(&unconfigured).expect("an absent calendar resolves");

    // Same candidate twice: armed, and with its authorization removed. Both
    // must admit: absence of a calendar is not a close, and absence of a grant
    // is not a stop.
    let armed = candidate(project, "pilot.code", None, taken_at);
    let mut unarmed = armed.clone();
    unarmed.authorization = None;

    let armed_plan = plan(&snapshot(project, vec![armed], &[], taken_at)).expect("the pass runs");
    let unarmed_candidate = unarmed.clone();
    let unarmed_plan =
        plan(&snapshot(project, vec![unarmed], &[], taken_at)).expect("the pass runs");
    let unarmed_code = unarmed_plan
        .decisions
        .first()
        .and_then(kontor_scheduler::model::CandidateDecision::rejection_code);
    let unarmed_reasons = explain(
        &snapshot(project, vec![unarmed_candidate.clone()], &[], taken_at),
        &unarmed_candidate,
    )
    .expect("the full explanation runs");

    let artifact = bundle
        .artifact(
            "calendar/unrestricted.json",
            &json!({
                "effective_state": state.to_string(),
                "admits_new_work": CalendarAdmission::unrestricted().admits_new_work(),
                "policy_evidence": Option::<String>::None,
                "armed_admitted": armed_plan.admitted_count(),
                "unarmed_admitted": unarmed_plan.admitted_count(),
                "unarmed_first_blocker": unarmed_code.map(|code| code.to_string()),
                "unarmed_every_blocker": unarmed_reasons
                    .iter()
                    .map(|refusal| refusal.code.to_string())
                    .collect::<Vec<_>>(),
            }),
        )
        .expect("the unrestricted evidence is written");

    let unrestricted = state == EffectiveCalendarState::Unrestricted;
    let unarmed_admitted = unarmed_plan.admitted_count() == 1 && unarmed_code.is_none();
    if unrestricted && armed_plan.admitted_count() == 1 && unarmed_admitted {
        bundle.pass(
            "domain.calendar-unrestricted",
            "a project with no assignment resolves `unrestricted`, needs no timezone and admits \
             both armed and unarmed work at this instant — never `calendar_closed` and never \
             `authorization_missing`",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.calendar-unrestricted",
            format!(
                "state={state}, armed_admitted={}, unarmed_code={unarmed_code:?}",
                armed_plan.admitted_count()
            ),
        );
    }
}

/// A configured calendar closes, drains, observes a holiday and expires an
/// override — and the scheduler consumes each answer without re-deciding it.
fn calendar_configured(bundle: &mut Bundle) {
    let project = ProjectId::generate();
    let profile_id = CalendarProfileId::generate();
    let calendar_id = WorkCalendarId::generate();

    // Oslo, Wednesday 09:00–17:00, draining for the last 30 minutes.
    let profile = CalendarProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id,
        version: SpecVersion::FIRST,
        name: name("Pilot office hours"),
        windows: vec![WeeklyWindow {
            weekday: Weekday::Wednesday,
            start: civil::time(9, 0, 0, 0),
            end: civil::time(17, 0, 0, 0),
        }],
        holiday_merge: HolidayMergePolicy::TreatAsClosed,
        drain_lead_minutes: 30,
    };
    let assignment = WorkCalendarAssignment {
        id: calendar_id,
        project_id: project,
        profile_id,
        profile_version: SpecVersion::FIRST,
        timezone: IanaTimeZone::parse("Europe/Oslo").expect("a bundled tzdb zone"),
        window_override: None,
        active: true,
        created_at: at("2026-08-01T00:00:00Z"),
        retired_at: None,
    };

    // 2026-08-12 is a Wednesday. Oslo is UTC+2 in August.
    let resolve =
        |now: &str, exceptions: &[CalendarExceptionRevision], over: Option<&ScheduleOverride>| {
            resolve_effective_state(&CalendarResolution {
                assignment: Some(&assignment),
                profile: Some(&profile),
                exceptions,
                schedule_override: over,
                mini_project: None,
                task: None,
                now: at(now),
            })
            .expect("a configured calendar resolves")
        };

    let holiday = CalendarExceptionRevision {
        id: CalendarExceptionId::generate(),
        project_id: project,
        work_calendar_id: calendar_id,
        start_date: civil::date(2026, 8, 12),
        end_date: civil::date(2026, 8, 12),
        kind: ExceptionKind::Closed,
        label: name("Pilot public holiday"),
        provenance: ExceptionProvenance::HolidaySource {
            source_id: kontor_core::id::HolidaySourceId::generate(),
        },
        supersedes: None,
        created_at: at("2026-08-01T00:00:00Z"),
    };

    let approved_override = ScheduleOverride {
        id: ScheduleOverrideId::generate(),
        project_id: project,
        scope: WorkScope::Project,
        reason: name("Pilot urgent override"),
        start: at("2026-08-12T20:00:00Z"),
        expiry: OverrideExpiry::FixedAt {
            at: at("2026-08-12T21:00:00Z"),
        },
        hard_ceiling: at("2026-08-12T22:00:00Z"),
        max_concurrency: 1,
        budget: budget(),
        approved_by: AccountProfileId::generate(),
        approval_receipt: CommandReceiptId::generate(),
        revocations: Vec::new(),
    };

    // 09:00Z = 11:00 Oslo → open. 14:40Z = 16:40 Oslo → inside the drain lead.
    // 20:00Z = 22:00 Oslo → closed. The override reopens exactly its own hour.
    let observed = [
        ("open", resolve("2026-08-12T09:00:00Z", &[], None)),
        ("draining", resolve("2026-08-12T14:40:00Z", &[], None)),
        ("closed", resolve("2026-08-12T20:00:00Z", &[], None)),
        (
            "holiday",
            resolve("2026-08-12T09:00:00Z", std::slice::from_ref(&holiday), None),
        ),
        (
            "override_open",
            resolve("2026-08-12T20:30:00Z", &[], Some(&approved_override)),
        ),
        (
            "override_expired",
            resolve("2026-08-12T21:30:00Z", &[], Some(&approved_override)),
        ),
    ];

    let expected = [
        ("open", EffectiveCalendarState::Open),
        ("draining", EffectiveCalendarState::Draining),
        ("closed", EffectiveCalendarState::Closed),
        ("holiday", EffectiveCalendarState::Closed),
        ("override_open", EffectiveCalendarState::OverrideOpen),
        ("override_expired", EffectiveCalendarState::Closed),
    ];

    // Each resolved state is handed to the scheduler as a *consumed* answer.
    let taken_at = at(DECIDED_AT);
    let mut admissions = Vec::new();
    for (label, state) in observed {
        let admission = CalendarAdmission {
            state,
            policy: (state != EffectiveCalendarState::Unrestricted).then(|| {
                CalendarPolicyEvidence {
                    profile_id,
                    policy_revision: SpecVersion::FIRST,
                    timezone: assignment.timezone.clone(),
                    matched_window: Some(name("wednesday-09-17")),
                }
            }),
            override_id: (state == EffectiveCalendarState::OverrideOpen)
                .then_some(approved_override.id),
            next_opening: (state == EffectiveCalendarState::Closed)
                .then(|| at("2026-08-19T07:00:00Z")),
        };
        let mut scoped = candidate(project, "pilot.code", None, taken_at);
        scoped.calendar = admission.clone();
        let outcome = plan(&snapshot(project, vec![scoped], &[], taken_at)).expect("the pass runs");
        admissions.push(json!({
            "phase": label,
            "state": state.to_string(),
            "admits_new_work": admission.admits_new_work(),
            "admitted": outcome.admitted_count(),
            "rejection": outcome
                .decisions
                .first()
                .and_then(kontor_scheduler::model::CandidateDecision::rejection_code)
                .map(|code| code.to_string()),
        }));
    }

    let artifact = bundle
        .artifact(
            "calendar/configured.json",
            &json!({
                "timezone": assignment.timezone.as_str(),
                "pinned_profile_revision": assignment.profile_version.get(),
                "drain_lead_minutes": profile.drain_lead_minutes,
                "holiday_merge": "treat_as_closed",
                "override": {
                    "scope": "project",
                    "effective_end": approved_override.effective_end().to_string(),
                    "hard_ceiling": approved_override.hard_ceiling.to_string(),
                },
                "phases": admissions,
            }),
        )
        .expect("the configured-calendar evidence is written");

    let mismatched: Vec<String> = expected
        .iter()
        .zip(observed.iter())
        .filter(|((label, want), (_, got))| {
            let _ = label;
            want != got
        })
        .map(|((label, want), (_, got))| format!("{label}: wanted {want}, got {got}"))
        .collect();

    // Draining is the drain proof: new envelopes are refused while nothing
    // cancels the work already inside the window.
    let draining_refused = admissions
        .iter()
        .find(|entry| entry["phase"] == "draining")
        .is_some_and(|entry| entry["admitted"] == 0 && entry["rejection"] == "calendar_draining");

    if mismatched.is_empty() && draining_refused {
        bundle.pass(
            "domain.calendar-configured",
            "a pinned Europe/Oslo calendar resolved open, draining, closed, holiday-closed, \
             override-open and override-expired at six instants; the scheduler refused new work \
             while draining (`calendar_draining`) and while closed (`calendar_closed`) without \
             touching work already admitted",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.calendar-configured",
            format!(
                "state mismatches: [{}]; drain refused new work: {draining_refused}",
                mismatched.join(", ")
            ),
        );
    }
}

/// No surface can hand the scheduler a time it will believe.
fn calendar_ignores_client_clocks(bundle: &mut Bundle) {
    let project = ProjectId::generate();
    let taken_at = at(DECIDED_AT);
    let subject = candidate(project, "pilot.code", None, taken_at);

    // The same snapshot decided twice, with real wall-clock time passing in
    // between and the process's own clock never consulted by the pass.
    let first =
        plan(&snapshot(project, vec![subject.clone()], &[], taken_at)).expect("the pass runs");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second =
        plan(&snapshot(project, vec![subject.clone()], &[], taken_at)).expect("the pass runs");

    // And the same snapshot decided at a *different* declared instant, which is
    // the only way an instant can enter the decision at all.
    let later = at("2026-08-19T09:00:00Z");
    let mut moved = subject;
    moved.created_at = later;
    let third = plan(&snapshot(project, vec![moved], &[], later)).expect("the pass runs");

    let canonical = |value: &kontor_scheduler::model::Plan| {
        serde_json::to_string(value).expect("a plan serializes")
    };
    let stable = canonical(&first) == canonical(&second);
    let instant_is_declared = canonical(&first) != canonical(&third);

    let artifact = bundle
        .artifact(
            "calendar/client-clock.json",
            &json!({
                "same_snapshot_twice_identical": stable,
                "declared_instant_changes_the_answer": instant_is_declared,
                "snapshot_taken_at": taken_at.to_string(),
                "second_taken_at": later.to_string(),
                "rule": "SchedulingSnapshot::taken_at is the only instant `plan` reads; the pass \
                         opens no clock, database, filesystem or environment variable, so there is \
                         no field a client could put its own time into",
            }),
        )
        .expect("the client-clock evidence is written");

    if stable && instant_is_declared {
        bundle.pass(
            "domain.calendar-client-clock",
            "one snapshot decided twice across real elapsed time produced byte-identical plans, \
             and only changing the snapshot's declared instant changed the answer: admission reads \
             the instant the control plane recorded, never a caller's clock",
            &[artifact],
        );
    } else {
        bundle.fail(
            "domain.calendar-client-clock",
            format!("stable={stable}, declared_instant_changes_the_answer={instant_is_declared}"),
        );
    }
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// A bounded, non-zero budget.
fn budget() -> BudgetBounds {
    BudgetBounds {
        max_tokens: 80_000,
        max_commands: 100,
        max_duration_seconds: 3_600,
        max_cost: Money {
            minor_units: 8_000,
            currency: CurrencyCode::parse("NOK").expect("a legal currency"),
        },
    }
}

/// A bounded external name.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a legal external name")
}

/// A module key.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn module(text: &str) -> ModuleKey {
    ModuleKey::parse(text).expect("a legal module key")
}

/// Every capability the fake declares, so runtime admission is never the blocker
/// under test.
fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env: true,
        limits: kontor_runtime::capability::RuntimeLimits {
            max_message_bytes: 4_096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

/// One admissible candidate: armed, healthy, reconciled and unrestricted.
///
/// Every blocker except the one a case is about is deliberately satisfied, so a
/// refusal names the thing under test rather than the setup.
fn candidate(
    project_id: ProjectId,
    module_key: &str,
    worktree: Option<WorktreeClaim>,
    taken_at: Timestamp,
) -> Candidate {
    let runtime_kind =
        kontor_core::id::RuntimeKindKey::parse("fake.runtime").expect("a legal runtime key");
    Candidate {
        project_id,
        task_id: TaskId::generate(),
        mini_project_id: None,
        workflow_id: TaskWorkflowId::generate(),
        state: TaskState::Ready,
        revision: AggregateRevision::INITIAL,
        created_at: taken_at,
        priority: 500,
        module: Some(module(module_key)),
        changed_modules: BTreeSet::new(),
        worktree,
        depends_on: BTreeSet::new(),
        serializes_with: BTreeSet::new(),
        origin: TaskOrigin::Manual,
        authorization: Some(AuthorizationEvidence {
            id: kontor_core::id::ExecutionAuthorizationId::generate(),
            project_id,
            scope: WorkScope::Project,
            selected_tasks: BTreeSet::new(),
            allowed_start: at("2026-08-12T00:00:00Z"),
            allowed_end: at("2026-08-20T00:00:00Z"),
            max_concurrency: 8,
        }),
        calendar: CalendarAdmission::unrestricted(),
        runtime: RuntimeAdmissionEvidence {
            runtime_kind: runtime_kind.clone(),
            host: name("fake-host"),
            generation: 1,
            capabilities: capabilities(),
            required: minimum_launch_capabilities(),
            health: RuntimeHealth::Healthy,
            reconciliation: ReconciliationEvidence {
                epoch_completed: true,
                scope: ReconciliationScope {
                    project_id,
                    runtime_kind,
                    host: name("fake-host"),
                    generation: 1,
                },
                open_replay_gap: false,
                divergence: false,
                orphan_ambiguity: false,
                stale_lost_contact: false,
            },
            last_confirmed_at: Some(taken_at),
        },
        account: AccountAdmissionEvidence {
            pin: None,
            required_capabilities: BTreeSet::new(),
        },
        external: ExternalWorkEvidence::default(),
        blocked_by: None,
    }
}

/// A snapshot with capacity high enough that only the blocker under test can
/// refuse.
fn snapshot(
    project_id: ProjectId,
    candidates: Vec<Candidate>,
    module_leases: &[ModuleClaim],
    taken_at: Timestamp,
) -> SchedulingSnapshot {
    let _ = project_id;
    SchedulingSnapshot {
        schema_version: SCHEMA_VERSION,
        taken_at,
        candidates,
        in_flight_tasks: BTreeSet::new(),
        completed_tasks: BTreeSet::new(),
        module_leases: module_leases.to_vec(),
        worktree_leases: BTreeSet::new(),
        usage: CapacityUsage {
            global_in_flight: 0,
            project_in_flight: BTreeMap::new(),
            mission_in_flight: BTreeMap::new(),
            account_in_flight: BTreeMap::new(),
            provider_in_flight: BTreeMap::new(),
            runtime_in_flight: BTreeMap::new(),
        },
        capacity: CapacityConfig {
            global_max_in_flight: 16,
            project_max_in_flight: 16,
            mission_max_in_flight: 16,
            account_max_in_flight: 16,
            provider_max_in_flight: 16,
            runtime_max_in_flight: 16,
            adaptive: adaptive(),

            // No headroom policy: these fixtures judge the in-flight ceilings.
            headroom: None,
        },
        adaptive_window: AdaptiveWindow::start(adaptive()),
        freshness: jiff::SignedDuration::from_secs(120),
    }
}

/// A window wide enough not to be the thing under test.
const fn adaptive() -> AdaptiveWindowConfig {
    AdaptiveWindowConfig {
        initial: 16,
        floor: 2,
        ceiling: 16,
        growth_step: 1,
    }
}
