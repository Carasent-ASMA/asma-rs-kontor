//! Calendars, execution authorization and bounded overrides.
//!
//! The rule that shapes this module: **absence of a calendar is not a closed
//! calendar.** A project with no active assignment resolves to
//! [`EffectiveCalendarState::Unrestricted`], needs no timezone and needs no
//! holiday source. Only a *configured* calendar can ever close anything.
//!
//! Two further rules:
//!
//! * A calendar assignment pins a profile *revision*. Resolution refuses to run
//!   against a different revision rather than silently upgrading.
//! * Instants are converted to local time, never the other way round, so a
//!   missing or repeated local hour at a DST boundary cannot make the resolver
//!   ambiguous.

use jiff::civil;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::id::{
    AccountProfileId, CalendarExceptionId, CalendarProfileId, CanonicalDocument, CommandReceiptId,
    ContentHash, ExecutionAuthorizationId, ExternalName, HolidaySourceId, MiniProjectId, ProjectId,
    ScheduleOverrideId, SchemaVersion, SpecVersion, TaskId, Timestamp, WorkCalendarId,
};
use crate::receipt::AggregateRef;
use crate::spec::BudgetBounds;
use crate::{DomainError, DomainResult};

closed_enum! {
    /// A day of the week, with a stable persisted spelling.
    Weekday, "Weekday" {
        /// Monday.
        Monday => "monday",
        /// Tuesday.
        Tuesday => "tuesday",
        /// Wednesday.
        Wednesday => "wednesday",
        /// Thursday.
        Thursday => "thursday",
        /// Friday.
        Friday => "friday",
        /// Saturday.
        Saturday => "saturday",
        /// Sunday.
        Sunday => "sunday",
    }
}

impl Weekday {
    /// Convert from the calendar library's weekday.
    #[must_use]
    pub const fn from_civil(weekday: civil::Weekday) -> Self {
        match weekday {
            civil::Weekday::Monday => Self::Monday,
            civil::Weekday::Tuesday => Self::Tuesday,
            civil::Weekday::Wednesday => Self::Wednesday,
            civil::Weekday::Thursday => Self::Thursday,
            civil::Weekday::Friday => Self::Friday,
            civil::Weekday::Saturday => Self::Saturday,
            civil::Weekday::Sunday => Self::Sunday,
        }
    }
}

/// An IANA time zone name, validated against the bundled tzdb.
///
/// The tzdb is bundled on every platform, so DST behaviour does not depend on
/// the host's `/usr/share/zoneinfo`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IanaTimeZone(String);

impl IanaTimeZone {
    /// Parse and validate a time zone name.
    ///
    /// # Errors
    /// Rejects any name the bundled tzdb does not know.
    pub fn parse(text: &str) -> DomainResult<Self> {
        TimeZone::get(text)
            .map_err(|_| DomainError::invalid("IanaTimeZone", "is not a known IANA time zone"))?;
        Ok(Self(text.to_owned()))
    }

    /// Borrow the zone name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve to the time zone itself.
    ///
    /// # Errors
    /// Returns [`DomainError`] if the bundled tzdb no longer knows the name.
    pub fn to_time_zone(&self) -> DomainResult<TimeZone> {
        TimeZone::get(&self.0)
            .map_err(|_| DomainError::invalid("IanaTimeZone", "is not a known IANA time zone"))
    }
}

impl TryFrom<String> for IanaTimeZone {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<IanaTimeZone> for String {
    fn from(value: IanaTimeZone) -> Self {
        value.0
    }
}

/// One recurring open window in local time.
///
/// A window never spans midnight: an overnight shift must be declared as two
/// explicit windows, so no consumer has to guess whether `22:00–06:00` means
/// eight hours or sixteen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyWindow {
    /// The local weekday.
    pub weekday: Weekday,
    /// Local start time, inclusive.
    pub start: civil::Time,
    /// Local end time, exclusive.
    pub end: civil::Time,
}

impl WeeklyWindow {
    /// Validate one window.
    ///
    /// # Errors
    /// Rejects an empty window and one that would wrap past midnight.
    pub fn validate(&self) -> DomainResult<()> {
        if self.start >= self.end {
            return Err(DomainError::invalid(
                "WeeklyWindow",
                "must start before it ends and must not span midnight",
            ));
        }
        Ok(())
    }

    /// Whether a local time falls inside this window.
    #[must_use]
    pub fn contains(&self, weekday: Weekday, time: civil::Time) -> bool {
        self.weekday == weekday && time >= self.start && time < self.end
    }
}

/// Validate a whole window set: each window individually, and no overlap within
/// a weekday.
///
/// # Errors
/// Rejects an invalid or overlapping window set. Overlap is rejected rather than
/// merged so the stored definition and the resolved behaviour never diverge.
pub fn validate_windows(windows: &[WeeklyWindow]) -> DomainResult<()> {
    for window in windows {
        window.validate()?;
    }
    let mut sorted: Vec<&WeeklyWindow> = windows.iter().collect();
    sorted.sort_by_key(|window| (window.weekday, window.start));
    for pair in sorted.windows(2) {
        let (previous, next) = (pair[0], pair[1]);
        if previous.weekday == next.weekday && next.start < previous.end {
            return Err(DomainError::invalid(
                "WeeklyWindow",
                "windows on the same weekday must not overlap",
            ));
        }
    }
    Ok(())
}

closed_enum! {
    /// How holiday-derived exceptions combine with the weekly windows.
    HolidayMergePolicy, "HolidayMergePolicy" {
        /// Holidays do not affect the calendar.
        Ignore => "ignore",
        /// A holiday closes the day.
        TreatAsClosed => "treat_as_closed",
        /// A holiday opens the day.
        TreatAsOpen => "treat_as_open",
    }
}

/// One immutable revision of a workspace-level calendar profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarProfileSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The profile id shared by every revision.
    pub profile_id: CalendarProfileId,
    /// This revision.
    pub version: SpecVersion,
    /// Human name.
    pub name: ExternalName,
    /// Recurring weekly windows in local time.
    pub windows: Vec<WeeklyWindow>,
    /// How holidays combine with those windows.
    pub holiday_merge: HolidayMergePolicy,
    /// How long before a window closes work is considered draining.
    pub drain_lead_minutes: u32,
}

impl CalendarProfileSpec {
    /// Validate the profile.
    ///
    /// # Errors
    /// Rejects invalid or overlapping windows and an out-of-range drain lead.
    pub fn validate(&self) -> DomainResult<()> {
        validate_windows(&self.windows)?;
        if self.drain_lead_minutes > 24 * 60 {
            return Err(DomainError::invalid(
                "CalendarProfileSpec",
                "drain lead must be at most one day",
            ));
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`CalendarProfileSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }
}

/// A project's pinned calendar assignment.
///
/// At most one assignment per project is active at a time; zero active
/// assignments means the project is unrestricted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCalendarAssignment {
    /// This assignment's id.
    pub id: WorkCalendarId,
    /// The project it applies to.
    pub project_id: ProjectId,
    /// The pinned profile.
    pub profile_id: CalendarProfileId,
    /// The pinned profile revision. Never upgraded in place.
    pub profile_version: SpecVersion,
    /// The time zone the windows are interpreted in. Required once configured.
    pub timezone: IanaTimeZone,
    /// A project-specific replacement for the profile's windows.
    pub window_override: Option<Vec<WeeklyWindow>>,
    /// Whether this assignment is the active one.
    pub active: bool,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it was retired, if it was.
    pub retired_at: Option<Timestamp>,
}

impl WorkCalendarAssignment {
    /// Validate the assignment.
    ///
    /// # Errors
    /// Rejects an invalid window override and an active assignment that also
    /// records a retirement.
    pub fn validate(&self) -> DomainResult<()> {
        if let Some(windows) = &self.window_override {
            validate_windows(windows)?;
        }
        if self.active && self.retired_at.is_some() {
            return Err(DomainError::invalid(
                "WorkCalendarAssignment",
                "an active assignment cannot be retired",
            ));
        }
        Ok(())
    }
}

closed_enum! {
    /// Where a holiday set came from.
    HolidayProviderKind, "HolidayProviderKind" {
        /// An iCalendar feed.
        Ical => "ical",
        /// Entered by a human.
        Manual => "manual",
        /// Shipped with Kontor.
        Bundled => "bundled",
    }
}

/// A two-letter ISO-3166-1 country code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CountryCode(String);

impl CountryCode {
    /// Parse a country code.
    ///
    /// # Errors
    /// Rejects anything that is not two uppercase ASCII letters.
    pub fn parse(text: &str) -> DomainResult<Self> {
        if text.len() != 2 || !text.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(DomainError::invalid(
                "CountryCode",
                "must be two uppercase ASCII letters",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    /// Borrow the code text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CountryCode {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<CountryCode> for String {
    fn from(value: CountryCode) -> Self {
        value.0
    }
}

/// One retrieved, immutable holiday source revision.
///
/// It records provenance — where the data came from, when and what it hashed to
/// — and never a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidaySourceRevision {
    /// This revision's id.
    pub id: HolidaySourceId,
    /// The calendar profile it belongs to.
    pub profile_id: CalendarProfileId,
    /// The profile revision it belongs to.
    pub profile_version: SpecVersion,
    /// Where the data came from.
    pub provider: HolidayProviderKind,
    /// Country.
    pub country: CountryCode,
    /// Subdivision, if the source is regional.
    pub subdivision: Option<ExternalName>,
    /// A non-secret reference to the source.
    pub reference: ExternalName,
    /// First date covered.
    pub range_start: civil::Date,
    /// Last date covered.
    pub range_end: civil::Date,
    /// When it was retrieved.
    pub retrieved_at: Timestamp,
    /// Digest of the raw payload.
    pub raw_hash: ContentHash,
    /// Digest of the normalized payload.
    pub normalized_hash: ContentHash,
}

impl HolidaySourceRevision {
    /// Validate the revision.
    ///
    /// # Errors
    /// Rejects an inverted date range.
    pub fn validate(&self) -> DomainResult<()> {
        if self.range_start > self.range_end {
            return Err(DomainError::invalid(
                "HolidaySourceRevision",
                "covers an inverted date range",
            ));
        }
        Ok(())
    }
}

closed_enum! {
    /// What a calendar exception does to a day.
    ExceptionKind, "ExceptionKind" {
        /// The day is closed.
        Closed => "closed",
        /// The day is open.
        Open => "open",
    }
}

/// Where a calendar exception came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExceptionProvenance {
    /// A human entered it.
    Manual {
        /// Who entered it.
        by: AccountProfileId,
    },
    /// It came from a retrieved holiday source.
    HolidaySource {
        /// Which source revision.
        source_id: HolidaySourceId,
    },
}

/// One append-only calendar exception revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarExceptionRevision {
    /// This revision's id.
    pub id: CalendarExceptionId,
    /// The project it applies to.
    pub project_id: ProjectId,
    /// The calendar assignment it applies to.
    pub work_calendar_id: WorkCalendarId,
    /// First local date covered.
    pub start_date: civil::Date,
    /// Last local date covered.
    pub end_date: civil::Date,
    /// What it does.
    pub kind: ExceptionKind,
    /// Human label.
    pub label: ExternalName,
    /// Where it came from.
    pub provenance: ExceptionProvenance,
    /// The revision it supersedes, if any.
    pub supersedes: Option<CalendarExceptionId>,
    /// When it was recorded.
    pub created_at: Timestamp,
}

impl CalendarExceptionRevision {
    /// Validate the revision.
    ///
    /// # Errors
    /// Rejects an inverted date range.
    pub fn validate(&self) -> DomainResult<()> {
        if self.start_date > self.end_date {
            return Err(DomainError::invalid(
                "CalendarExceptionRevision",
                "covers an inverted date range",
            ));
        }
        Ok(())
    }

    /// Whether this exception covers a local date.
    #[must_use]
    pub fn covers(&self, date: civil::Date) -> bool {
        date >= self.start_date && date <= self.end_date
    }
}

closed_enum! {
    /// What the calendar says right now.
    EffectiveCalendarState, "EffectiveCalendarState" {
        /// No calendar is configured; nothing restricts execution.
        Unrestricted => "unrestricted",
        /// A calendar is configured and currently closed.
        Closed => "closed",
        /// A calendar is configured and currently open.
        Open => "open",
        /// Open, but the window closes within the drain lead.
        Draining => "draining",
        /// Closed, but an approved override is in force.
        OverrideOpen => "override_open",
    }
}

/// The scope a bounded authorization or override applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkScope {
    /// The whole project.
    Project,
    /// One goal inside the project.
    MiniProject {
        /// The goal.
        mini_project_id: MiniProjectId,
    },
    /// One task.
    Task {
        /// The task.
        task_id: TaskId,
    },
}

impl WorkScope {
    /// The aggregate a receipt granting authority over this scope must target.
    ///
    /// A capability or an override is granted over a scope, so "the receipt
    /// matches the scope" is a concrete equality rather than a judgement call.
    #[must_use]
    pub const fn aggregate(&self, project_id: ProjectId) -> AggregateRef {
        match self {
            Self::Project => AggregateRef::Project { project_id },
            Self::MiniProject { mini_project_id } => AggregateRef::MiniProject {
                mini_project_id: *mini_project_id,
            },
            Self::Task { task_id } => AggregateRef::Task { task_id: *task_id },
        }
    }

    /// Whether this scope covers a given goal and task.
    #[must_use]
    pub fn covers(&self, mini_project: Option<MiniProjectId>, task: Option<TaskId>) -> bool {
        match self {
            Self::Project => true,
            Self::MiniProject { mini_project_id } => mini_project == Some(*mini_project_id),
            Self::Task { task_id } => task == Some(*task_id),
        }
    }
}

/// An inclusive instant range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    /// First instant covered.
    pub start: Timestamp,
    /// Last instant covered.
    pub end: Timestamp,
}

impl TimeRange {
    /// Validate the range.
    ///
    /// # Errors
    /// Rejects an inverted range.
    pub fn validate(&self) -> DomainResult<()> {
        if self.start > self.end {
            return Err(DomainError::invalid("TimeRange", "is inverted"));
        }
        Ok(())
    }

    /// Whether an instant falls inside the range.
    #[must_use]
    pub fn contains(&self, instant: Timestamp) -> bool {
        instant >= self.start && instant <= self.end
    }
}

/// An explicit authorization that arms ready work.
///
/// Work that is merely `ready` is never eligible: something must arm it, and
/// that something is always a receipt-backed authorization with bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionAuthorization {
    /// This authorization's id.
    pub id: ExecutionAuthorizationId,
    /// The project it belongs to.
    pub project_id: ProjectId,
    /// What it covers.
    pub scope: WorkScope,
    /// The specific tasks it arms, if it is not scope-wide.
    pub selected_tasks: Vec<TaskId>,
    /// When work may start.
    pub allowed_start: TimeRange,
    /// Maximum concurrent runs it authorizes.
    pub max_concurrency: u32,
    /// Budget bounds it authorizes.
    pub budget: BudgetBounds,
    /// Who created it.
    pub created_by: AccountProfileId,
    /// The command receipt that recorded the capability.
    pub capability_receipt: CommandReceiptId,
    /// When it was created.
    pub created_at: Timestamp,
}

impl ExecutionAuthorization {
    /// Validate the authorization.
    ///
    /// # Errors
    /// Rejects an inverted start range, zero concurrency or an unbounded budget.
    pub fn validate(&self) -> DomainResult<()> {
        self.allowed_start.validate()?;
        if self.max_concurrency == 0 {
            return Err(DomainError::invalid(
                "ExecutionAuthorization",
                "concurrency must be positive",
            ));
        }
        self.budget.validate()
    }

    /// Whether this authorization arms a given task at a given instant.
    #[must_use]
    pub fn arms(
        &self,
        now: Timestamp,
        mini_project: Option<MiniProjectId>,
        task: Option<TaskId>,
    ) -> bool {
        if !self.allowed_start.contains(now) || !self.scope.covers(mini_project, task) {
            return false;
        }
        match (&self.selected_tasks.is_empty(), task) {
            (true, _) => true,
            (false, Some(task_id)) => self.selected_tasks.contains(&task_id),
            (false, None) => false,
        }
    }
}

/// When a schedule override stops applying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverrideExpiry {
    /// It ends at a fixed instant.
    FixedAt {
        /// The instant.
        at: Timestamp,
    },
    /// It ends when a goal completes — but never later than the hard ceiling.
    GoalBound {
        /// The goal it follows.
        mini_project_id: MiniProjectId,
    },
}

/// One append-only revocation of an override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideRevocation {
    /// When it was revoked.
    pub revoked_at: Timestamp,
    /// Who revoked it.
    pub revoked_by: AccountProfileId,
    /// The command receipt that recorded the revocation.
    pub receipt: CommandReceiptId,
}

/// A bounded, approved override of calendar admission.
///
/// An override bypasses the *calendar* and nothing else: dependencies,
/// authorization, leases, guardrails and budgets still apply. Its hard ceiling
/// is mandatory, so a goal-bound override cannot outlive its goal indefinitely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleOverride {
    /// This override's id.
    pub id: ScheduleOverrideId,
    /// The project it belongs to.
    pub project_id: ProjectId,
    /// What it covers.
    pub scope: WorkScope,
    /// Why it exists.
    pub reason: ExternalName,
    /// When it starts.
    pub start: Timestamp,
    /// When it is scheduled to end.
    pub expiry: OverrideExpiry,
    /// The instant it can never outlive. Mandatory.
    pub hard_ceiling: Timestamp,
    /// Maximum concurrent runs it allows.
    pub max_concurrency: u32,
    /// Budget bounds it allows.
    pub budget: BudgetBounds,
    /// Who approved it.
    pub approved_by: AccountProfileId,
    /// The command receipt that recorded the approval.
    pub approval_receipt: CommandReceiptId,
    /// Append-only revocation history.
    pub revocations: Vec<OverrideRevocation>,
}

impl ScheduleOverride {
    /// Validate the override.
    ///
    /// # Errors
    /// Rejects a ceiling at or before the start, a fixed expiry beyond the
    /// ceiling, zero concurrency and an unbounded budget.
    pub fn validate(&self) -> DomainResult<()> {
        if self.hard_ceiling <= self.start {
            return Err(DomainError::invalid(
                "ScheduleOverride",
                "the hard ceiling must be after the start",
            ));
        }
        if let OverrideExpiry::FixedAt { at } = self.expiry
            && at > self.hard_ceiling
        {
            return Err(DomainError::invalid(
                "ScheduleOverride",
                "a fixed expiry must not exceed the hard ceiling",
            ));
        }
        if self.max_concurrency == 0 {
            return Err(DomainError::invalid(
                "ScheduleOverride",
                "concurrency must be positive",
            ));
        }
        self.budget.validate()
    }

    /// The instant this override can never be active beyond.
    #[must_use]
    pub fn effective_end(&self) -> Timestamp {
        match self.expiry {
            OverrideExpiry::FixedAt { at } => at.min(self.hard_ceiling),
            OverrideExpiry::GoalBound { .. } => self.hard_ceiling,
        }
    }

    /// Whether the override is in force for a given scope at a given instant.
    #[must_use]
    pub fn is_active(
        &self,
        now: Timestamp,
        mini_project: Option<MiniProjectId>,
        task: Option<TaskId>,
    ) -> bool {
        if !self.revocations.is_empty() {
            return false;
        }
        now >= self.start && now <= self.effective_end() && self.scope.covers(mini_project, task)
    }
}

/// Everything the calendar resolver is allowed to read.
#[derive(Debug, Clone)]
pub struct CalendarResolution<'a> {
    /// The project's active assignment, if it has one.
    pub assignment: Option<&'a WorkCalendarAssignment>,
    /// The profile revision the assignment pins. Required when `assignment` is
    /// present, and must be exactly the pinned revision.
    pub profile: Option<&'a CalendarProfileSpec>,
    /// Exceptions recorded for that assignment.
    pub exceptions: &'a [CalendarExceptionRevision],
    /// An override, if one exists.
    pub schedule_override: Option<&'a ScheduleOverride>,
    /// The goal the work belongs to, for scope checks.
    pub mini_project: Option<MiniProjectId>,
    /// The task, for scope checks.
    pub task: Option<TaskId>,
    /// The instant being resolved.
    pub now: Timestamp,
}

/// Resolve the effective calendar state.
///
/// # Errors
/// * [`DomainError::Invalid`] when an assignment is present without its pinned
///   profile revision, or with a *different* revision — an applied revision is
///   never silently upgraded.
/// * [`DomainError::Invalid`] when the configured time zone is unknown.
pub fn resolve_effective_state(
    input: &CalendarResolution<'_>,
) -> DomainResult<EffectiveCalendarState> {
    if let Some(over) = input.schedule_override
        && over.is_active(input.now, input.mini_project, input.task)
    {
        return Ok(EffectiveCalendarState::OverrideOpen);
    }

    // No configured calendar means no restriction. This is the single most
    // important line in the module: absence must never read as "closed".
    let Some(assignment) = input.assignment else {
        return Ok(EffectiveCalendarState::Unrestricted);
    };
    if !assignment.active {
        return Ok(EffectiveCalendarState::Unrestricted);
    }

    let profile = input.profile.ok_or(DomainError::Invalid {
        subject: "CalendarResolution",
        rule: "an assignment requires its pinned profile revision",
    })?;
    if profile.profile_id != assignment.profile_id || profile.version != assignment.profile_version
    {
        return Err(DomainError::invalid(
            "CalendarResolution",
            "the supplied profile revision is not the pinned one",
        ));
    }

    let zone = assignment.timezone.to_time_zone()?;
    // Instant to local time is always unambiguous, so a missing or repeated
    // local hour at a DST boundary cannot make this resolution ambiguous.
    let local = input.now.to_zoned(zone);
    let local_date = local.date();
    let local_time = local.time();
    let weekday = Weekday::from_civil(local.weekday());

    let covering = input.exceptions.iter().rfind(|exception| {
        exception.covers(local_date)
            && match exception.provenance {
                ExceptionProvenance::Manual { .. } => true,
                ExceptionProvenance::HolidaySource { .. } => {
                    profile.holiday_merge != HolidayMergePolicy::Ignore
                }
            }
    });
    if let Some(exception) = covering {
        let closed = match exception.provenance {
            ExceptionProvenance::Manual { .. } => exception.kind == ExceptionKind::Closed,
            ExceptionProvenance::HolidaySource { .. } => {
                profile.holiday_merge == HolidayMergePolicy::TreatAsClosed
            }
        };
        if closed {
            return Ok(EffectiveCalendarState::Closed);
        }
        return Ok(EffectiveCalendarState::Open);
    }

    let windows = assignment
        .window_override
        .as_deref()
        .unwrap_or(&profile.windows);
    let Some(window) = windows.iter().find(|w| w.contains(weekday, local_time)) else {
        return Ok(EffectiveCalendarState::Closed);
    };

    // Drain is measured in local wall-clock minutes, which is exactly how the
    // window itself is expressed.
    let remaining_minutes = i64::from(window.end.hour() - local_time.hour()) * 60
        + i64::from(window.end.minute() - local_time.minute());
    if remaining_minutes <= i64::from(profile.drain_lead_minutes) {
        return Ok(EffectiveCalendarState::Draining);
    }
    Ok(EffectiveCalendarState::Open)
}
