//! What the calendar says, and what it must never say.
//!
//! The mutants this suite exists to kill:
//!
//! * treating an absent calendar as closed, or as overridden;
//! * letting an override open a calendar that was not refusing;
//! * silently adopting a newer profile revision than the one pinned;
//! * losing the drain lead, the midnight edge or a DST boundary;
//! * letting a child scope widen the hours it inherits;
//! * letting an import shadow a human's exception;
//! * an override that outlives its scope, its ceiling, its goal or its
//!   revocation;
//! * evidence that names no policy revision, no zone or the wrong window.

use std::collections::BTreeSet;

use jiff::civil;
use kontor_calendar::resolve::{ResolutionRequest, core_state, resolve};
use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, EffectiveCalendarState, ExceptionKind,
    ExceptionProvenance, HolidayMergePolicy, IanaTimeZone, OverrideExpiry, OverrideRevocation,
    ScheduleOverride, Weekday, WeeklyWindow, WorkCalendarAssignment, WorkScope,
};
use kontor_core::id::{
    AccountProfileId, CalendarExceptionId, CalendarProfileId, CommandReceiptId, CurrencyCode,
    ExternalName, HolidaySourceId, MiniProjectId, Money, ProjectId, SCHEMA_VERSION,
    ScheduleOverrideId, SpecVersion, TaskId, Timestamp, WorkCalendarId, parse_utc_timestamp,
};
use kontor_core::spec::BudgetBounds;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid display name")
}

fn time(text: &str) -> civil::Time {
    text.parse().expect("a civil time")
}

fn date(text: &str) -> civil::Date {
    text.parse().expect("a civil date")
}

fn oslo() -> IanaTimeZone {
    IanaTimeZone::parse("Europe/Oslo").expect("a bundled tzdb zone")
}

fn window(weekday: Weekday, start: &str, end: &str) -> WeeklyWindow {
    WeeklyWindow {
        weekday,
        start: time(start),
        end: time(end),
    }
}

/// Monday 08:00–16:00 in Oslo, draining for the last thirty minutes.
fn office_hours() -> CalendarProfileSpec {
    profile(
        vec![window(Weekday::Monday, "08:00:00", "16:00:00")],
        HolidayMergePolicy::TreatAsClosed,
        30,
    )
}

fn profile(
    windows: Vec<WeeklyWindow>,
    holiday_merge: HolidayMergePolicy,
    drain_lead_minutes: u32,
) -> CalendarProfileSpec {
    CalendarProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id: CalendarProfileId::generate(),
        version: SpecVersion::FIRST,
        name: name("Office hours"),
        windows,
        holiday_merge,
        drain_lead_minutes,
    }
}

fn assignment(profile: &CalendarProfileSpec) -> WorkCalendarAssignment {
    WorkCalendarAssignment {
        id: WorkCalendarId::generate(),
        project_id: ProjectId::generate(),
        profile_id: profile.profile_id,
        profile_version: profile.version,
        timezone: oslo(),
        window_override: None,
        active: true,
        created_at: at("2026-01-01T00:00:00Z"),
        retired_at: None,
    }
}

fn budget() -> BudgetBounds {
    BudgetBounds {
        max_tokens: 1_000,
        max_commands: 10,
        max_duration_seconds: 600,
        max_cost: Money {
            minor_units: 100,
            currency: CurrencyCode::parse("NOK").expect("a currency"),
        },
    }
}

fn exception(
    calendar: &WorkCalendarAssignment,
    day: &str,
    kind: ExceptionKind,
    label: &str,
    provenance: ExceptionProvenance,
    created_at: &str,
) -> CalendarExceptionRevision {
    CalendarExceptionRevision {
        id: CalendarExceptionId::generate(),
        project_id: calendar.project_id,
        work_calendar_id: calendar.id,
        start_date: date(day),
        end_date: date(day),
        kind,
        label: name(label),
        provenance,
        supersedes: None,
        created_at: at(created_at),
    }
}

fn imported(
    calendar: &WorkCalendarAssignment,
    day: &str,
    label: &str,
    created_at: &str,
) -> CalendarExceptionRevision {
    exception(
        calendar,
        day,
        ExceptionKind::Closed,
        label,
        ExceptionProvenance::HolidaySource {
            source_id: HolidaySourceId::generate(),
        },
        created_at,
    )
}

fn manual(
    calendar: &WorkCalendarAssignment,
    day: &str,
    kind: ExceptionKind,
    label: &str,
    created_at: &str,
) -> CalendarExceptionRevision {
    exception(
        calendar,
        day,
        kind,
        label,
        ExceptionProvenance::Manual {
            by: AccountProfileId::generate(),
        },
        created_at,
    )
}

fn schedule_override(
    project_id: ProjectId,
    scope: WorkScope,
    start: &str,
    expiry: OverrideExpiry,
    ceiling: &str,
) -> ScheduleOverride {
    ScheduleOverride {
        id: ScheduleOverrideId::generate(),
        project_id,
        scope,
        reason: name("Incident"),
        start: at(start),
        expiry,
        hard_ceiling: at(ceiling),
        max_concurrency: 1,
        budget: budget(),
        approved_by: AccountProfileId::generate(),
        approval_receipt: CommandReceiptId::generate(),
        revocations: Vec::new(),
    }
}

/// A request with every optional input empty, for a configured calendar.
fn request<'a>(
    assignment: &'a WorkCalendarAssignment,
    profile: &'a CalendarProfileSpec,
    now: &str,
) -> ResolutionRequest<'a> {
    ResolutionRequest {
        now: at(now),
        assignment: Some(assignment),
        profile: Some(profile),
        exceptions: &[],
        child_windows: None,
        overrides: &[],
        terminal_goals: &[],
        mini_project: None,
        task: None,
    }
}

fn state_at(
    assignment: &WorkCalendarAssignment,
    profile: &CalendarProfileSpec,
    now: &str,
) -> EffectiveCalendarState {
    resolve(&request(assignment, profile, now))
        .expect("a configured calendar resolves")
        .state
}

// ---------------------------------------------------------------------------
// Absence
// ---------------------------------------------------------------------------

#[test]
fn a_project_with_no_assignment_is_unrestricted_at_every_instant() {
    for instant in [
        "2026-01-01T00:00:00Z",
        "2026-03-29T02:30:00Z",
        "2026-08-09T23:59:59Z",
        "2026-12-25T12:00:00Z",
    ] {
        let resolved = resolve(&ResolutionRequest {
            now: at(instant),
            assignment: None,
            profile: None,
            exceptions: &[],
            child_windows: None,
            overrides: &[],
            terminal_goals: &[],
            mini_project: None,
            task: None,
        })
        .expect("an unconfigured project resolves");

        assert_eq!(resolved.state, EffectiveCalendarState::Unrestricted);
        assert!(resolved.admits_new_work(), "{instant} must admit new work");
        assert!(
            resolved.policy.is_none(),
            "an unconfigured project names no policy"
        );
        assert!(resolved.next_opening.is_none());
    }
}

/// The ordering bug this ticket fixes: an override was consulted *before* the
/// absence of an assignment, so an unconfigured project with a stray override
/// reported `override_open` — a calendar answer for a project that has no
/// calendar.
#[test]
fn an_override_on_an_unconfigured_project_is_ignored_not_honoured() {
    let over = schedule_override(
        ProjectId::generate(),
        WorkScope::Project,
        "2026-08-09T00:00:00Z",
        OverrideExpiry::FixedAt {
            at: at("2026-08-11T00:00:00Z"),
        },
        "2026-08-12T00:00:00Z",
    );
    let overrides = [over];

    let resolved = resolve(&ResolutionRequest {
        now: at("2026-08-10T10:00:00Z"),
        assignment: None,
        profile: None,
        exceptions: &[],
        child_windows: None,
        overrides: &overrides,
        terminal_goals: &[],
        mini_project: None,
        task: None,
    })
    .expect("an unconfigured project resolves");

    assert_eq!(
        resolved.state,
        EffectiveCalendarState::Unrestricted,
        "absence of a calendar is not something an override can open"
    );
    assert!(resolved.override_id.is_none());
    assert!(resolved.policy.is_none());
}

#[test]
fn a_retired_assignment_is_unrestricted() {
    let profile = office_hours();
    let mut retired = assignment(&profile);
    retired.active = false;
    retired.retired_at = Some(at("2026-08-01T00:00:00Z"));

    // 2026-08-09 is a Sunday: a configured calendar would be closed here.
    assert_eq!(
        state_at(&retired, &profile, "2026-08-09T10:00:00Z"),
        EffectiveCalendarState::Unrestricted
    );
}

// ---------------------------------------------------------------------------
// Windows, drain, boundaries
// ---------------------------------------------------------------------------

/// 2026-08-10 is a Monday. Oslo is UTC+2 in August, so 06:00Z is 08:00 local.
#[test]
fn the_open_drain_and_close_boundaries_are_exact() {
    let profile = office_hours();
    let calendar = assignment(&profile);

    for (instant, expected, why) in [
        (
            "2026-08-10T05:59:59Z",
            EffectiveCalendarState::Closed,
            "07:59:59 local, before the window",
        ),
        (
            "2026-08-10T06:00:00Z",
            EffectiveCalendarState::Open,
            "08:00 local, the first instant inside",
        ),
        (
            "2026-08-10T13:29:59Z",
            EffectiveCalendarState::Open,
            "15:29:59 local, one second before the drain lead",
        ),
        (
            "2026-08-10T13:30:00Z",
            EffectiveCalendarState::Draining,
            "15:30 local, the drain lead begins",
        ),
        (
            "2026-08-10T13:59:59Z",
            EffectiveCalendarState::Draining,
            "15:59:59 local, still draining",
        ),
        (
            "2026-08-10T14:00:00Z",
            EffectiveCalendarState::Closed,
            "16:00 local, the window is exclusive at its end",
        ),
    ] {
        assert_eq!(state_at(&calendar, &profile, instant), expected, "{why}");
    }
}

#[test]
fn draining_admits_no_new_work_while_open_does() {
    let profile = office_hours();
    let calendar = assignment(&profile);

    let open = resolve(&request(&calendar, &profile, "2026-08-10T08:00:00Z")).expect("resolves");
    let draining =
        resolve(&request(&calendar, &profile, "2026-08-10T13:45:00Z")).expect("resolves");
    let closed = resolve(&request(&calendar, &profile, "2026-08-10T20:00:00Z")).expect("resolves");

    assert!(open.admits_new_work());
    assert!(
        !draining.admits_new_work(),
        "draining exists precisely so that new top-level work stops while bounded work finishes"
    );
    assert!(!closed.admits_new_work());
}

#[test]
fn a_window_that_ends_at_midnight_closes_at_midnight_and_not_before() {
    let profile = profile(
        vec![window(Weekday::Monday, "22:00:00", "23:59:59")],
        HolidayMergePolicy::Ignore,
        0,
    );
    let calendar = assignment(&profile);

    // 21:00Z is 23:00 Monday local; 22:00Z is 00:00 Tuesday local.
    assert_eq!(
        state_at(&calendar, &profile, "2026-08-10T21:00:00Z"),
        EffectiveCalendarState::Open
    );
    assert_eq!(
        state_at(&calendar, &profile, "2026-08-10T22:00:00Z"),
        EffectiveCalendarState::Closed,
        "Tuesday 00:00 belongs to Tuesday, which has no window"
    );
}

#[test]
fn a_daylight_saving_gap_never_opens_a_local_hour_that_does_not_exist() {
    // Oslo springs forward on 2026-03-29: local 02:00–02:59 does not happen.
    let profile = profile(
        vec![window(Weekday::Sunday, "02:00:00", "03:00:00")],
        HolidayMergePolicy::Ignore,
        0,
    );
    let calendar = assignment(&profile);

    for instant in [
        "2026-03-29T00:30:00Z",
        "2026-03-29T00:59:59Z",
        "2026-03-29T01:00:00Z",
        "2026-03-29T01:30:00Z",
    ] {
        assert_eq!(
            state_at(&calendar, &profile, instant),
            EffectiveCalendarState::Closed,
            "{instant} falls in or beside a local hour that never happened"
        );
    }
}

#[test]
fn a_repeated_local_hour_opens_for_both_passes() {
    // Oslo falls back on 2026-10-25: local 02:00–02:59 happens twice, once at
    // UTC+2 and once at UTC+1. Both are inside the window, and the resolver
    // converts instant to local rather than the other way round, so neither
    // pass is ambiguous.
    let profile = profile(
        vec![window(Weekday::Sunday, "02:00:00", "03:00:00")],
        HolidayMergePolicy::Ignore,
        0,
    );
    let calendar = assignment(&profile);

    for instant in ["2026-10-25T00:30:00Z", "2026-10-25T01:30:00Z"] {
        assert_eq!(
            state_at(&calendar, &profile, instant),
            EffectiveCalendarState::Open,
            "{instant} is 02:30 local on one of the two passes"
        );
    }
    assert_eq!(
        state_at(&calendar, &profile, "2026-10-25T02:00:00Z"),
        EffectiveCalendarState::Closed,
        "03:00 local, after the second pass"
    );
}

// ---------------------------------------------------------------------------
// Pinning and replacement hours
// ---------------------------------------------------------------------------

#[test]
fn a_pinned_revision_is_refused_not_upgraded_when_a_newer_one_arrives() {
    let profile = office_hours();
    let calendar = assignment(&profile);

    let mut newer = profile.clone();
    newer.version = profile.version.next().expect("a next revision");
    newer.windows[0].end = time("20:00:00");

    assert!(
        resolve(&request(&calendar, &newer, "2026-08-10T16:00:00Z")).is_err(),
        "a newer revision of the pinned profile is a different policy, not an upgrade"
    );
    // 18:00 local is inside the newer window and outside the pinned one.
    assert_eq!(
        state_at(&calendar, &profile, "2026-08-10T16:00:00Z"),
        EffectiveCalendarState::Closed
    );
}

#[test]
fn an_assignment_without_its_pinned_profile_is_refused() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let mut orphaned = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    orphaned.profile = None;

    assert!(resolve(&orphaned).is_err());
}

#[test]
fn project_replacement_hours_replace_the_profile_hours() {
    let profile = office_hours();
    let mut calendar = assignment(&profile);
    calendar.window_override = Some(vec![window(Weekday::Monday, "10:00:00", "12:00:00")]);

    // 08:00 local is inside the profile's window and outside the project's.
    assert_eq!(
        state_at(&calendar, &profile, "2026-08-10T06:00:00Z"),
        EffectiveCalendarState::Closed
    );
    assert_eq!(
        state_at(&calendar, &profile, "2026-08-10T09:00:00Z"),
        EffectiveCalendarState::Open,
        "11:00 local is inside the project's replacement hours"
    );
}

// ---------------------------------------------------------------------------
// Child narrowing
// ---------------------------------------------------------------------------

#[test]
fn a_child_scope_may_narrow_the_hours_it_inherits() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let narrower = [window(Weekday::Monday, "09:00:00", "12:00:00")];
    let mut narrowed = request(&calendar, &profile, "2026-08-10T06:00:00Z");
    narrowed.child_windows = Some(&narrower);

    assert_eq!(
        resolve(&narrowed)
            .expect("a narrowing child resolves")
            .state,
        EffectiveCalendarState::Closed,
        "08:00 local is open for the parent and outside the child's own hours"
    );

    let mut inside = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    inside.child_windows = Some(&narrower);
    assert_eq!(
        resolve(&inside).expect("a narrowing child resolves").state,
        EffectiveCalendarState::Open
    );
}

#[test]
fn a_child_scope_cannot_widen_without_an_approved_override() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let wider = [window(Weekday::Monday, "08:00:00", "20:00:00")];
    let mut widened = request(&calendar, &profile, "2026-08-10T16:00:00Z");
    widened.child_windows = Some(&wider);

    assert!(
        matches!(
            resolve(&widened),
            Err(kontor_calendar::CalendarError::WidenedWithoutApproval)
        ),
        "inheritance narrows; widening is a policy change and needs an approval"
    );

    // A different weekday is widening too: the child claims hours on a day the
    // parent never opens.
    let other_day = [window(Weekday::Sunday, "09:00:00", "10:00:00")];
    let mut sunday = request(&calendar, &profile, "2026-08-09T08:00:00Z");
    sunday.child_windows = Some(&other_day);
    assert!(matches!(
        resolve(&sunday),
        Err(kontor_calendar::CalendarError::WidenedWithoutApproval)
    ));
}

#[test]
fn a_widening_child_resolves_only_under_an_approved_scoped_override() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let wider = [window(Weekday::Monday, "08:00:00", "20:00:00")];
    let approved = [schedule_override(
        calendar.project_id,
        WorkScope::Project,
        "2026-08-10T15:00:00Z",
        OverrideExpiry::FixedAt {
            at: at("2026-08-10T18:00:00Z"),
        },
        "2026-08-10T19:00:00Z",
    )];
    let mut widened = request(&calendar, &profile, "2026-08-10T16:00:00Z");
    widened.child_windows = Some(&wider);
    widened.overrides = &approved;

    let resolved = resolve(&widened).expect("an approved override covers the widening");
    assert_eq!(resolved.state, EffectiveCalendarState::OverrideOpen);
    assert_eq!(resolved.override_id, Some(approved[0].id));
}

// ---------------------------------------------------------------------------
// Exceptions
// ---------------------------------------------------------------------------

#[test]
fn an_imported_holiday_closes_the_day_it_covers() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let holidays = [imported(
        &calendar,
        "2026-08-10",
        "Public holiday",
        "2026-08-01T00:00:00Z",
    )];

    let mut with_holiday = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    with_holiday.exceptions = &holidays;

    assert_eq!(
        state_at(&calendar, &profile, "2026-08-10T08:00:00Z"),
        EffectiveCalendarState::Open,
        "without the import applied, the Monday is an ordinary working day"
    );
    let closed = resolve(&with_holiday).expect("resolves");
    assert_eq!(closed.state, EffectiveCalendarState::Closed);
    assert_eq!(
        closed
            .policy
            .as_ref()
            .and_then(|policy| policy.matched_window.clone()),
        Some(name("Public holiday")),
        "the evidence names the exception that closed the day"
    );
}

#[test]
fn a_profile_that_ignores_holidays_is_not_closed_by_an_import() {
    let profile = profile(
        vec![window(Weekday::Monday, "08:00:00", "16:00:00")],
        HolidayMergePolicy::Ignore,
        0,
    );
    let calendar = assignment(&profile);
    let holidays = [imported(
        &calendar,
        "2026-08-10",
        "Public holiday",
        "2026-08-01T00:00:00Z",
    )];
    let mut ignored = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    ignored.exceptions = &holidays;

    assert_eq!(
        resolve(&ignored).expect("resolves").state,
        EffectiveCalendarState::Open
    );
}

#[test]
fn a_manual_exception_beats_an_import_whatever_order_they_were_recorded_in() {
    let profile = office_hours();
    let calendar = assignment(&profile);

    // The human opened the day *before* the import closed it. Recording order
    // must not decide against them.
    let both = [
        manual(
            &calendar,
            "2026-08-10",
            ExceptionKind::Open,
            "We work this one",
            "2026-07-01T00:00:00Z",
        ),
        imported(
            &calendar,
            "2026-08-10",
            "Public holiday",
            "2026-08-01T00:00:00Z",
        ),
    ];
    let mut mixed = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    mixed.exceptions = &both;

    assert_eq!(
        resolve(&mixed).expect("resolves").state,
        EffectiveCalendarState::Open,
        "a human's decision is not overwritten by a refreshed feed"
    );
}

#[test]
fn a_superseded_exception_no_longer_decides_anything() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let original = imported(
        &calendar,
        "2026-08-10",
        "Withdrawn holiday",
        "2026-07-01T00:00:00Z",
    );
    let mut replacement = imported(
        &calendar,
        "2026-08-17",
        "Moved holiday",
        "2026-08-01T00:00:00Z",
    );
    replacement.supersedes = Some(original.id);
    let history = [original, replacement];

    let mut refreshed = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    refreshed.exceptions = &history;

    assert_eq!(
        resolve(&refreshed).expect("resolves").state,
        EffectiveCalendarState::Open,
        "the superseded closure stays in history and stops being policy"
    );
}

// ---------------------------------------------------------------------------
// Overrides
// ---------------------------------------------------------------------------

#[test]
fn an_override_opens_a_refusal_and_nothing_else() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let overrides = [schedule_override(
        calendar.project_id,
        WorkScope::Project,
        "2026-08-10T00:00:00Z",
        OverrideExpiry::FixedAt {
            at: at("2026-08-11T00:00:00Z"),
        },
        "2026-08-11T00:00:00Z",
    )];

    // Closed becomes override_open.
    let mut closed = request(&calendar, &profile, "2026-08-10T20:00:00Z");
    closed.overrides = &overrides;
    let opened = resolve(&closed).expect("resolves");
    assert_eq!(opened.state, EffectiveCalendarState::OverrideOpen);
    assert_eq!(opened.override_id, Some(overrides[0].id));
    assert!(
        opened.next_opening.is_none(),
        "an open calendar has no next opening"
    );
    assert!(
        opened.policy.is_some(),
        "the pinned policy is still evidence"
    );

    // Draining becomes override_open too: it is the state that refuses new work.
    let mut draining = request(&calendar, &profile, "2026-08-10T13:45:00Z");
    draining.overrides = &overrides;
    assert_eq!(
        resolve(&draining).expect("resolves").state,
        EffectiveCalendarState::OverrideOpen
    );

    // An already-open calendar is not relabelled.
    let mut open = request(&calendar, &profile, "2026-08-10T08:00:00Z");
    open.overrides = &overrides;
    let ordinary = resolve(&open).expect("resolves");
    assert_eq!(ordinary.state, EffectiveCalendarState::Open);
    assert!(ordinary.override_id.is_none());
}

#[test]
fn an_override_cannot_escape_its_scope_ceiling_goal_or_revocation() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let goal = MiniProjectId::generate();
    let task = TaskId::generate();
    let other_task = TaskId::generate();
    let closed_instant = "2026-08-10T20:00:00Z";

    let scoped = |scope| {
        schedule_override(
            calendar.project_id,
            scope,
            "2026-08-10T00:00:00Z",
            OverrideExpiry::FixedAt {
                at: at("2026-08-11T00:00:00Z"),
            },
            "2026-08-11T00:00:00Z",
        )
    };

    // Out of scope: an override for another task opens nothing here.
    let elsewhere = [scoped(WorkScope::Task {
        task_id: other_task,
    })];
    let mut wrong_scope = request(&calendar, &profile, closed_instant);
    wrong_scope.overrides = &elsewhere;
    wrong_scope.task = Some(task);
    assert_eq!(
        resolve(&wrong_scope).expect("resolves").state,
        EffectiveCalendarState::Closed
    );

    // In scope.
    let mine = [scoped(WorkScope::Task { task_id: task })];
    let mut right_scope = request(&calendar, &profile, closed_instant);
    right_scope.overrides = &mine;
    right_scope.task = Some(task);
    assert_eq!(
        resolve(&right_scope).expect("resolves").state,
        EffectiveCalendarState::OverrideOpen
    );

    // Revoked.
    let mut revoked = mine[0].clone();
    revoked.revocations.push(OverrideRevocation {
        revoked_at: at("2026-08-10T18:00:00Z"),
        revoked_by: AccountProfileId::generate(),
        receipt: CommandReceiptId::generate(),
    });
    let revoked = [revoked];
    let mut after_revocation = request(&calendar, &profile, closed_instant);
    after_revocation.overrides = &revoked;
    after_revocation.task = Some(task);
    assert_eq!(
        resolve(&after_revocation).expect("resolves").state,
        EffectiveCalendarState::Closed
    );

    // Past its fixed expiry, and past its hard ceiling.
    let mut expired = request(&calendar, &profile, "2026-08-11T20:00:00Z");
    expired.overrides = &mine;
    expired.task = Some(task);
    assert_eq!(
        resolve(&expired).expect("resolves").state,
        EffectiveCalendarState::Closed
    );

    // Goal-bound, and the goal has completed.
    let goal_bound = [schedule_override(
        calendar.project_id,
        WorkScope::MiniProject {
            mini_project_id: goal,
        },
        "2026-08-10T00:00:00Z",
        OverrideExpiry::GoalBound {
            mini_project_id: goal,
        },
        "2026-08-30T00:00:00Z",
    )];
    let terminal = [goal];
    let mut after_goal = request(&calendar, &profile, closed_instant);
    after_goal.overrides = &goal_bound;
    after_goal.mini_project = Some(goal);
    after_goal.terminal_goals = &terminal;
    assert_eq!(
        resolve(&after_goal).expect("resolves").state,
        EffectiveCalendarState::Closed,
        "a goal-bound override ends when its goal does, long before its ceiling"
    );

    // The same override, while the goal is still running.
    let mut during_goal = request(&calendar, &profile, closed_instant);
    during_goal.overrides = &goal_bound;
    during_goal.mini_project = Some(goal);
    assert_eq!(
        resolve(&during_goal).expect("resolves").state,
        EffectiveCalendarState::OverrideOpen
    );
}

#[test]
fn a_goal_bound_override_still_stops_at_its_hard_ceiling() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let goal = MiniProjectId::generate();
    let overrides = [schedule_override(
        calendar.project_id,
        WorkScope::MiniProject {
            mini_project_id: goal,
        },
        "2026-08-10T00:00:00Z",
        OverrideExpiry::GoalBound {
            mini_project_id: goal,
        },
        "2026-08-10T22:00:00Z",
    )];

    let mut before = request(&calendar, &profile, "2026-08-10T21:00:00Z");
    before.overrides = &overrides;
    before.mini_project = Some(goal);
    assert_eq!(
        resolve(&before).expect("resolves").state,
        EffectiveCalendarState::OverrideOpen
    );

    let mut after = request(&calendar, &profile, "2026-08-10T22:00:01Z");
    after.overrides = &overrides;
    after.mini_project = Some(goal);
    assert_eq!(
        resolve(&after).expect("resolves").state,
        EffectiveCalendarState::Closed,
        "the ceiling is mandatory precisely so an unfinished goal cannot hold a calendar open"
    );
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

#[test]
fn the_evidence_names_the_pinned_revision_the_zone_and_the_matched_window() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let resolved =
        resolve(&request(&calendar, &profile, "2026-08-10T08:00:00Z")).expect("resolves");

    let policy = resolved
        .policy
        .expect("a configured calendar names its policy");
    assert_eq!(policy.profile_id, profile.profile_id);
    assert_eq!(policy.policy_revision, profile.version);
    assert_eq!(policy.timezone, oslo());
    assert_eq!(
        policy.matched_window,
        Some(name("monday-08:00-16:00")),
        "the label is derived from the window, so every client renders the same one"
    );
}

#[test]
fn a_closed_calendar_reports_a_deterministic_next_opening() {
    let profile = office_hours();
    let calendar = assignment(&profile);

    // Sunday 2026-08-09: the next opening is Monday 08:00 Oslo = 06:00Z.
    let sunday = resolve(&request(&calendar, &profile, "2026-08-09T10:00:00Z")).expect("resolves");
    assert_eq!(sunday.state, EffectiveCalendarState::Closed);
    assert_eq!(sunday.next_opening, Some(at("2026-08-10T06:00:00Z")));

    // Monday after the window closed: the following Monday.
    let evening = resolve(&request(&calendar, &profile, "2026-08-10T20:00:00Z")).expect("resolves");
    assert_eq!(evening.next_opening, Some(at("2026-08-17T06:00:00Z")));

    // The same question twice gives the same answer.
    let again = resolve(&request(&calendar, &profile, "2026-08-10T20:00:00Z")).expect("resolves");
    assert_eq!(evening.next_opening, again.next_opening);
}

#[test]
fn the_next_opening_skips_a_closed_holiday_and_survives_a_dst_change() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let holidays = [imported(
        &calendar,
        "2026-10-26",
        "Autumn holiday",
        "2026-08-01T00:00:00Z",
    )];

    // 2026-10-25 is the Sunday Oslo falls back to UTC+1; 2026-10-26 is a Monday
    // and a holiday, so the next opening is Monday 2026-11-02 at 08:00 = 07:00Z.
    let mut closed = request(&calendar, &profile, "2026-10-25T10:00:00Z");
    closed.exceptions = &holidays;
    let resolved = resolve(&closed).expect("resolves");
    assert_eq!(resolved.state, EffectiveCalendarState::Closed);
    assert_eq!(
        resolved.next_opening,
        Some(at("2026-11-02T07:00:00Z")),
        "08:00 local is 07:00Z once the offset has changed"
    );
}

#[test]
fn a_calendar_with_no_window_at_all_reports_no_next_opening() {
    let profile = profile(Vec::new(), HolidayMergePolicy::Ignore, 0);
    let calendar = assignment(&profile);
    let resolved =
        resolve(&request(&calendar, &profile, "2026-08-10T08:00:00Z")).expect("resolves");

    assert_eq!(resolved.state, EffectiveCalendarState::Closed);
    assert!(
        resolved.next_opening.is_none(),
        "a calendar that never opens has no next opening, and saying so is honest"
    );
}

#[test]
fn a_dst_gap_opening_resolves_forward_past_the_hour_that_never_happens() {
    // The window starts at 02:30 local on 2026-03-29, inside the hour Oslo
    // skips. The opening is shifted forward by the length of the gap — 03:30
    // local, 01:30Z — rather than reported at a local time that never occurs.
    let profile = profile(
        vec![window(Weekday::Sunday, "02:30:00", "04:00:00")],
        HolidayMergePolicy::Ignore,
        0,
    );
    let calendar = assignment(&profile);
    let resolved =
        resolve(&request(&calendar, &profile, "2026-03-28T10:00:00Z")).expect("resolves");

    assert_eq!(resolved.state, EffectiveCalendarState::Closed);
    assert_eq!(resolved.next_opening, Some(at("2026-03-29T01:30:00Z")));
}

// ---------------------------------------------------------------------------
// Agreement with the domain reducer
// ---------------------------------------------------------------------------

/// The domain has its own small reducer and this crate has the full contract.
/// Two answers to one question is exactly the shape a divergence hides in, so
/// they are held to each other over the whole interesting matrix.
#[test]
fn the_resolution_agrees_with_the_domain_reducer_everywhere() {
    let profile = office_hours();
    let calendar = assignment(&profile);
    let holidays = [imported(
        &calendar,
        "2026-08-17",
        "Public holiday",
        "2026-08-01T00:00:00Z",
    )];
    let overrides = [schedule_override(
        calendar.project_id,
        WorkScope::Project,
        "2026-08-10T18:00:00Z",
        OverrideExpiry::FixedAt {
            at: at("2026-08-10T22:00:00Z"),
        },
        "2026-08-10T23:00:00Z",
    )];

    let instants = [
        "2026-08-09T10:00:00Z",
        "2026-08-10T05:59:59Z",
        "2026-08-10T06:00:00Z",
        "2026-08-10T13:29:59Z",
        "2026-08-10T13:30:00Z",
        "2026-08-10T14:00:00Z",
        "2026-08-10T19:00:00Z",
        "2026-08-10T23:30:00Z",
        "2026-08-17T08:00:00Z",
        "2026-03-29T00:30:00Z",
        "2026-10-25T00:30:00Z",
        "2026-10-25T01:30:00Z",
    ];
    for instant in instants {
        for (with_exceptions, with_overrides) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut probe = request(&calendar, &profile, instant);
            if with_exceptions {
                probe.exceptions = &holidays;
            }
            if with_overrides {
                probe.overrides = &overrides;
            }
            let resolved = resolve(&probe).expect("resolves").state;
            let reduced = core_state(&probe).expect("the domain reducer resolves");
            assert_eq!(
                resolved, reduced,
                "{instant} (exceptions={with_exceptions}, overrides={with_overrides})"
            );
        }
    }
}

/// Categories, `BTreeSet` ordering and the unused-import lint all conspire to
/// make this file's imports easy to get wrong; asserting the default selection
/// keeps the vocabulary honest in the same suite that consumes it.
#[test]
fn the_default_category_selection_is_public_and_bank_only() {
    let default: BTreeSet<_> = kontor_core::calendar::HolidayCategory::DEFAULT_SELECTION
        .iter()
        .copied()
        .collect();
    assert_eq!(
        default,
        BTreeSet::from([
            kontor_core::calendar::HolidayCategory::Public,
            kontor_core::calendar::HolidayCategory::Bank,
        ])
    );
}
