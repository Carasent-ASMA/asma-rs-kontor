//! The one deterministic calendar resolution.
//!
//! Everything the resolver may read is an argument. It has no clock, no store
//! and no network: the instant is the coordinator's, the profile revision is the
//! pinned one, and the exceptions are the ones already applied locally. Two
//! callers with the same inputs get the same answer, which is what makes a
//! recorded admission re-checkable rather than merely re-readable.

use jiff::civil;
use jiff::tz::TimeZone;
use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, EffectiveCalendarState, HolidayMergePolicy,
    ScheduleOverride, Weekday, WeeklyWindow, WorkCalendarAssignment, exception_closes,
    governing_exception, validate_windows,
};
use kontor_core::id::{ExternalName, MiniProjectId, TaskId, Timestamp};
use kontor_core::{DomainError, DomainResult};
use kontor_scheduler::model::{CalendarAdmission, CalendarPolicyEvidence};

use crate::{CalendarError, CalendarResult};

/// How far ahead a closed calendar looks for its next opening.
///
/// A year and a day. Beyond that a weekly window that never matches is not a
/// calendar that opens later, it is a calendar that never opens, and reporting
/// `None` says so honestly instead of scanning forever to find out.
pub const NEXT_OPENING_HORIZON_DAYS: i16 = 366;

/// Everything resolution is allowed to read.
///
/// Note what is *not* here: a client timestamp, a store handle, a URL. The
/// instant is supplied by the coordinator, and a caller that wanted to resolve
/// "as the client sees it" would have to pass the coordinator's clock anyway.
#[derive(Debug, Clone)]
pub struct ResolutionRequest<'a> {
    /// The coordinator's instant. Never a client's.
    pub now: Timestamp,
    /// The project's active assignment, if it has one. `None` is `unrestricted`.
    pub assignment: Option<&'a WorkCalendarAssignment>,
    /// The exact profile revision the assignment pins.
    pub profile: Option<&'a CalendarProfileSpec>,
    /// The applied exceptions for that calendar: every manual revision, plus the
    /// imported ones belonging to the import that is currently applied.
    pub exceptions: &'a [CalendarExceptionRevision],
    /// A child scope's own windows. They may only narrow what they inherit.
    pub child_windows: Option<&'a [WeeklyWindow]>,
    /// Override candidates. Scope, revocation, expiry and ceiling are re-checked
    /// here; being in the slice grants nothing.
    pub overrides: &'a [ScheduleOverride],
    /// Goals that have reached a terminal state, so a goal-bound override that
    /// follows one of them has ended even though its ceiling has not arrived.
    pub terminal_goals: &'a [MiniProjectId],
    /// The goal the work belongs to, for scope checks.
    pub mini_project: Option<MiniProjectId>,
    /// The task, for scope checks.
    pub task: Option<TaskId>,
}

/// Resolve one instant into the answer the scheduler consumes.
///
/// The order of these steps is the contract, not an implementation detail:
///
/// 1. **No active assignment is `unrestricted`**, and an override on such a
///    project is ignored rather than honoured. There is no closed window for it
///    to open, and answering `override_open` would describe an unconfigured
///    project as governed by a calendar it does not have.
/// 2. The pinned profile's identity and revision are proved, and the configured
///    zone is resolved against the bundled tzdb. A newer revision of the same
///    profile is refused, never silently adopted.
/// 3. The instant is converted to project-local time. Always in that direction:
///    an instant has exactly one local time, while a local time can be missing
///    or repeated at a DST boundary.
/// 4. Effective windows — the profile's, or the assignment's replacement — then
///    child narrowing, then the governing exception, then the window match and
///    the drain lead.
/// 5. **An override is consulted last**, and only over a refusal.
///
/// # Errors
/// * [`CalendarError::Domain`] when an assignment arrives without its pinned
///   profile revision, with a different one, with an unknown zone, or with a
///   window set that is not a valid window set.
/// * [`CalendarError::WidenedWithoutApproval`] when a child scope declares hours
///   its parent does not cover and no approved override covers the work.
pub fn resolve(request: &ResolutionRequest<'_>) -> CalendarResult<CalendarAdmission> {
    let scopes: Vec<&[WeeklyWindow]> = request.child_windows.into_iter().collect();
    resolve_scoped(request, &scopes)
}

/// Resolve with every inherited child level, from broadest to narrowest.
///
/// The production path supplies the mini-project revision followed by the task
/// revision. Each level must narrow the result it inherited; an approved
/// override covering this work is the only widening route.
pub fn resolve_scoped(
    request: &ResolutionRequest<'_>,
    child_scopes: &[&[WeeklyWindow]],
) -> CalendarResult<CalendarAdmission> {
    // 1. Absence is not closure — and not an override either.
    let Some(assignment) = request.assignment.filter(|a| a.active) else {
        return Ok(CalendarAdmission::unrestricted());
    };

    // 2. The pinned revision, and only it.
    let profile = request.profile.ok_or_else(|| {
        DomainError::invalid(
            "CalendarResolution",
            "an assignment requires its pinned profile revision",
        )
    })?;
    if profile.profile_id != assignment.profile_id || profile.version != assignment.profile_version
    {
        return Err(DomainError::invalid(
            "CalendarResolution",
            "the supplied profile revision is not the pinned one",
        )
        .into());
    }
    let zone = assignment.timezone.to_time_zone()?;

    // 3. Instant to local time, never the other way round.
    let local = request.now.to_zoned(zone.clone());
    let local_date = local.date();
    let local_time = local.time();
    let weekday = Weekday::from_civil(local.weekday());

    let policy = |matched: Option<ExternalName>| CalendarPolicyEvidence {
        profile_id: profile.profile_id,
        policy_revision: profile.version,
        timezone: assignment.timezone.clone(),
        matched_window: matched,
    };
    let in_force = override_in_force(request);

    // 4. Inherited windows, then the child's narrowing of them.
    let inherited = assignment
        .window_override
        .as_deref()
        .unwrap_or(&profile.windows);
    validate_windows(inherited)?;
    let mut windows = inherited;
    for child in child_scopes {
        validate_windows(child)?;
        if !narrows(windows, child) {
            // A widening child is refused outright — unless an approved
            // override already covers this work, in which case the calendar
            // is being bypassed anyway and refusing would only hide that.
            let Some(over) = in_force else {
                return Err(CalendarError::WidenedWithoutApproval);
            };
            return admission(CalendarAdmission {
                state: EffectiveCalendarState::OverrideOpen,
                policy: Some(policy(None)),
                override_id: Some(over.id),
                next_opening: None,
            });
        }
        windows = child;
    }

    let (state, matched) =
        match governing_exception(request.exceptions, local_date, profile.holiday_merge) {
            Some(exception) if exception_closes(exception, profile.holiday_merge) => (
                EffectiveCalendarState::Closed,
                Some(exception.label.clone()),
            ),
            Some(exception) => (EffectiveCalendarState::Open, Some(exception.label.clone())),
            None => match windows.iter().find(|w| w.contains(weekday, local_time)) {
                None => (EffectiveCalendarState::Closed, None),
                Some(window) => {
                    let state = if window.minutes_remaining(local_time)
                        <= i64::from(profile.drain_lead_minutes)
                    {
                        EffectiveCalendarState::Draining
                    } else {
                        EffectiveCalendarState::Open
                    };
                    (state, window_label(window))
                }
            },
        };

    // 5. The override, last, and only over a refusal. `draining` is one: it is
    // the state that admits no new top-level work, which is exactly what an
    // urgent override is asked for.
    let refuses_new_work = matches!(
        state,
        EffectiveCalendarState::Closed | EffectiveCalendarState::Draining
    );
    if refuses_new_work && let Some(over) = in_force {
        return admission(CalendarAdmission {
            state: EffectiveCalendarState::OverrideOpen,
            policy: Some(policy(matched)),
            override_id: Some(over.id),
            next_opening: None,
        });
    }

    let next_opening = (state == EffectiveCalendarState::Closed)
        .then(|| {
            next_opening(
                &zone,
                local_date,
                local_time,
                windows,
                request.exceptions,
                profile.holiday_merge,
            )
        })
        .flatten();
    admission(CalendarAdmission {
        state,
        policy: Some(policy(matched)),
        override_id: None,
        next_opening,
    })
}

/// Prove the answer's parts agree before anyone records it.
///
/// [`CalendarAdmission::validate`] is the scheduler's own contract, so running
/// it here means a malformed answer cannot leave this crate — which is a better
/// place to find one than in the admission evidence of a dispatched run.
fn admission(resolved: CalendarAdmission) -> CalendarResult<CalendarAdmission> {
    resolved.validate()?;
    Ok(resolved)
}

/// The override in force for this work, if one is.
///
/// Four independent bounds, all re-checked here: it must have started and not
/// yet reached its expiry or its hard ceiling, it must cover this work's scope,
/// it must not be revoked, and a goal-bound one must not have outlived the goal
/// it follows. The earliest-starting candidate wins, so a project with two
/// valid overrides resolves the same way every time.
fn override_in_force<'a>(request: &ResolutionRequest<'a>) -> Option<&'a ScheduleOverride> {
    request
        .overrides
        .iter()
        .filter(|over| over.is_active(request.now, request.mini_project, request.task))
        .filter(|over| !goal_completed(over, request.terminal_goals))
        .min_by_key(|over| (over.start, over.id))
}

/// Whether a goal-bound override has been ended by its goal completing.
///
/// A fixed-expiry override ignores goals entirely: it ends when it said it
/// would.
fn goal_completed(over: &ScheduleOverride, terminal_goals: &[MiniProjectId]) -> bool {
    match over.expiry {
        kontor_core::calendar::OverrideExpiry::GoalBound { mini_project_id } => {
            terminal_goals.contains(&mini_project_id)
        }
        kontor_core::calendar::OverrideExpiry::FixedAt { .. } => false,
    }
}

/// Whether every child window lies inside a window the child inherits.
///
/// Narrowing is per-window containment rather than a total-minutes comparison: a
/// child that drops Friday and extends Monday by the same number of hours is
/// widening on Monday, and "the same number of hours overall" is not the
/// property a calendar promises.
fn narrows(inherited: &[WeeklyWindow], child: &[WeeklyWindow]) -> bool {
    child.iter().all(|narrow| {
        inherited.iter().any(|wide| {
            wide.weekday == narrow.weekday && narrow.start >= wide.start && narrow.end <= wide.end
        })
    })
}

/// A stable, parseable label for one window: `monday-08:00-16:00`.
///
/// Stable because it is derived only from the window, so the same window
/// produces the same label in every client, in every recorded admission and in
/// every export. Opaque to the scheduler, which records it and never reads it.
fn window_label(window: &WeeklyWindow) -> Option<ExternalName> {
    let text = format!(
        "{}-{:02}:{:02}-{:02}:{:02}",
        window.weekday.as_str(),
        window.start.hour(),
        window.start.minute(),
        window.end.hour(),
        window.end.minute()
    );
    // The format cannot produce an invalid display name — no control character,
    // no surrounding space, well inside the length bound — and `matched_window`
    // is optional anyway, so a label is reported when there is one rather than
    // failing a resolution that is otherwise correct.
    ExternalName::parse(&text).ok()
}

/// The next instant this calendar opens, if it opens within the horizon.
///
/// Walks forward one local day at a time, which is the only unit in which the
/// question is well posed: windows are weekly and local, exceptions are dated
/// and local, and the UTC offset can change between two of those days.
fn next_opening(
    zone: &TimeZone,
    from_date: civil::Date,
    from_time: civil::Time,
    windows: &[WeeklyWindow],
    exceptions: &[CalendarExceptionRevision],
    holiday_merge: HolidayMergePolicy,
) -> Option<Timestamp> {
    for offset in 0..=NEXT_OPENING_HORIZON_DAYS {
        let date = from_date.checked_add(jiff::Span::new().days(offset)).ok()?;
        let today = offset == 0;
        let opens_at = match governing_exception(exceptions, date, holiday_merge) {
            Some(exception) if exception_closes(exception, holiday_merge) => continue,
            // An open exception opens the whole local day, so the day itself is
            // the opening. Today cannot land here: an open day is not closed.
            Some(_) if !today => Some(civil::Time::midnight()),
            Some(_) => None,
            None => {
                let weekday = Weekday::from_civil(date.weekday());
                windows
                    .iter()
                    .filter(|window| window.weekday == weekday)
                    .map(|window| window.start)
                    .filter(|start| !today || *start > from_time)
                    .min()
            }
        };
        let Some(time) = opens_at else { continue };
        // A local opening time that a DST transition skipped is shifted forward
        // by the length of the gap, so the reported instant is one that exists.
        // A repeated local time takes its first pass. Both are the tzdb's own
        // compatible disambiguation, so the answer is the same on every host.
        if let Ok(zoned) = zone.to_zoned(date.to_datetime(time)) {
            return Some(zoned.timestamp());
        }
    }
    None
}

/// Resolve the state alone, for callers that want the domain value rather than
/// the admission evidence around it.
///
/// # Errors
/// As [`resolve`].
pub fn resolve_state(request: &ResolutionRequest<'_>) -> CalendarResult<EffectiveCalendarState> {
    Ok(resolve(request)?.state)
}

/// The domain's own reducer, applied to the same request.
///
/// Used by this crate's tests to prove the two answers agree; exposed because a
/// caller holding a [`ResolutionRequest`] should not have to rebuild a
/// [`kontor_core::calendar::CalendarResolution`] by hand to ask the domain the
/// same question.
///
/// # Errors
/// As [`kontor_core::calendar::resolve_effective_state`].
pub fn core_state(request: &ResolutionRequest<'_>) -> DomainResult<EffectiveCalendarState> {
    kontor_core::calendar::resolve_effective_state(&kontor_core::calendar::CalendarResolution {
        assignment: request.assignment,
        profile: request.profile,
        exceptions: request.exceptions,
        schedule_override: override_in_force(request),
        mini_project: request.mini_project,
        task: request.task,
        now: request.now,
    })
}
