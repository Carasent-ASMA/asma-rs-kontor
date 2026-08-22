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

use std::collections::BTreeSet;

use jiff::civil;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::id::{
    AccountProfileId, CalendarExceptionId, CalendarProfileId, CanonicalDocument, CommandReceiptId,
    ContentHash, ExecutionAuthorizationId, ExternalName, HolidaySourceId, IdempotencyKey,
    MiniProjectId, ProjectId, ScheduleOverrideId, SchemaVersion, SpecVersion, TaskId, Timestamp,
    WorkCalendarId, parse_utc_timestamp,
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

    /// Whole local minutes left before this window closes.
    ///
    /// Wall-clock minutes, which is exactly how the window and the drain lead are
    /// both expressed, and seconds are truncated so the two ends of a comparison
    /// cannot disagree by a partial minute. Negative once the window has closed.
    ///
    /// It lives here rather than at each call site because the drain boundary is
    /// decided in two places — this crate's reducer and `kontor-calendar`'s
    /// resolution — and two spellings of one formula is exactly how those two
    /// answers would come to differ.
    #[must_use]
    pub fn minutes_remaining(&self, time: civil::Time) -> i64 {
        i64::from(self.end.hour() - time.hour()) * 60 + i64::from(self.end.minute() - time.minute())
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

/// One immutable revision of working windows for a child scope.
///
/// Mini-projects and tasks inherit their project's calendar. This value can
/// narrow that inherited calendar; widening is admitted only while a scoped
/// approved override is active. Revisions are append-only and the newest leaf
/// in the supersession chain is the effective one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildCalendarWindows {
    /// Project owning the scope and calendar.
    pub project_id: ProjectId,
    /// Calendar assignment these windows narrow.
    pub work_calendar_id: WorkCalendarId,
    /// Mini-project or task being narrowed. Project scope is invalid here.
    pub scope: WorkScope,
    /// Revision number within this scope.
    pub version: SpecVersion,
    /// Local-time windows for the child.
    pub windows: Vec<WeeklyWindow>,
    /// Previous revision, when replacing one.
    pub supersedes: Option<SpecVersion>,
    /// When this revision was recorded.
    pub created_at: Timestamp,
}

impl ChildCalendarWindows {
    /// Validate the revision's shape.
    ///
    /// # Errors
    /// Rejects project scope, invalid windows, and inconsistent revision lineage.
    pub fn validate(&self) -> DomainResult<()> {
        if self.scope == WorkScope::Project {
            return Err(DomainError::invalid(
                "ChildCalendarWindows",
                "must name a mini-project or task scope",
            ));
        }
        validate_windows(&self.windows)?;
        match self.supersedes {
            None if self.version != SpecVersion::FIRST => Err(DomainError::invalid(
                "ChildCalendarWindows",
                "the first revision must have version one",
            )),
            Some(previous) if previous.next()? != self.version => Err(DomainError::invalid(
                "ChildCalendarWindows",
                "a revision must immediately follow the one it supersedes",
            )),
            _ => Ok(()),
        }
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
    /// Which importer produced a holiday source revision.
    ///
    /// This is the *precise* provenance, and it is deliberately not the same
    /// vocabulary as [`HolidayProviderKind`]. That column was written in schema
    /// v1, before any importer existed, and SQLite cannot widen a v1 `CHECK`;
    /// schema v6 therefore records the exact importer beside it rather than
    /// pretending the coarse value can carry a distinction it was never given.
    HolidayImportKind, "HolidayImportKind" {
        /// The Nager holiday API's JSON.
        NagerV4 => "nager_v4",
        /// The GOV.UK bank-holidays JSON.
        GovUkJson => "gov_uk_json",
        /// An iCalendar document, from a file or a URL.
        Ical => "ical",
    }
}

closed_enum! {
    /// What kind of day an imported entry is.
    ///
    /// Public and bank holidays are what a work calendar normally means by
    /// "closed". The rest are real days in the sources and are imported only when
    /// a caller names them, because silently closing a workspace on every school
    /// holiday would be a surprise nobody asked for.
    HolidayCategory, "HolidayCategory" {
        /// A public holiday.
        Public => "public",
        /// A bank holiday.
        Bank => "bank",
        /// A holiday for public authorities only.
        Authorities => "authorities",
        /// An optional holiday.
        Optional => "optional",
        /// A school holiday.
        School => "school",
        /// An observance, which is not normally a day off.
        Observance => "observance",
    }
}

impl HolidayCategory {
    /// The categories imported when a caller names none.
    pub const DEFAULT_SELECTION: &'static [Self] = &[Self::Public, Self::Bank];
}

closed_enum! {
    /// Why an importer refused or dropped one entry.
    ///
    /// A stable code, never prose and never the offending value: an import
    /// warning is stored, exported and shown to operators, and a source document
    /// is not this crate's to echo.
    ImportWarningCode, "ImportWarningCode" {
        /// The entry had a time of day, so it is not an all-day closure.
        TimedEvent => "timed_event",
        /// The entry recurred, and recurrence expansion is not supported.
        RecurringEvent => "recurring_event",
        /// The entry was missing a field or could not be read.
        MalformedEntry => "malformed_entry",
        /// The entry used a feature this importer does not support.
        UnsupportedEntry => "unsupported_entry",
        /// The entry fell outside the requested date range.
        OutOfRange => "out_of_range",
        /// The entry's category was not selected.
        FilteredCategory => "filtered_category",
        /// The entry applied to a subdivision that was not requested.
        FilteredSubdivision => "filtered_subdivision",
        /// A second entry claimed an identity an earlier one already used.
        DuplicateIdentity => "duplicate_identity",
    }
}

/// One refused or dropped entry, by position in the source document.
///
/// The position is the whole payload. It is enough for an operator to find the
/// entry in the document they supplied, and it cannot leak what the entry said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImportWarning {
    /// Why the entry was refused.
    pub code: ImportWarningCode,
    /// Its zero-based position in the source document.
    pub entry: u32,
}

/// The most warnings one import records before it is refused outright.
///
/// A document that produces more than this is not a document with a few bad
/// rows; it is the wrong document, and importing the remainder of it would be a
/// guess.
pub const MAX_IMPORT_WARNINGS: usize = 512;

/// The longest span one import may cover.
///
/// Bounded because an import is a *retrieval*, and an unbounded retrieval is how
/// a preview turns into a five-thousand-row apply nobody reviewed.
pub const MAX_IMPORT_DAYS: i64 = 366 * 5;

/// The import that produced one holiday source revision, and what it did.
///
/// One of these exists for exactly one [`HolidaySourceRevision`] — it is that
/// revision's provenance, not a second copy of it. It carries what the *request*
/// asked for, so a later reader can tell an empty result from a filtered one,
/// and the revision it replaced, so an import history is a chain rather than a
/// pile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidayImportBatch {
    /// The source revision this provenance belongs to.
    pub source_id: HolidaySourceId,
    /// The project whose calendar it was applied to.
    pub project_id: ProjectId,
    /// The calendar assignment it was applied to.
    pub work_calendar_id: WorkCalendarId,
    /// Which importer produced it.
    pub kind: HolidayImportKind,
    /// First date the request asked for.
    pub requested_start: civil::Date,
    /// Last date the request asked for.
    pub requested_end: civil::Date,
    /// The categories the request selected.
    pub categories: BTreeSet<HolidayCategory>,
    /// What the importer refused or dropped.
    pub warnings: Vec<ImportWarning>,
    /// How many exception revisions the apply wrote.
    pub applied_exceptions: u32,
    /// The source revision this one replaces, if it replaces one.
    pub supersedes: Option<HolidaySourceId>,
    /// The caller's replay key. A repeat of it returns the original apply.
    pub idempotency_key: IdempotencyKey,
    /// When it was applied.
    pub applied_at: Timestamp,
}

impl HolidayImportBatch {
    /// Validate the provenance record.
    ///
    /// # Errors
    /// Rejects an inverted or unbounded requested range, an empty category
    /// selection and an implausible warning count.
    pub fn validate(&self) -> DomainResult<()> {
        if self.requested_start > self.requested_end {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "covers an inverted date range",
            ));
        }
        let days = self
            .requested_start
            .until(self.requested_end)
            .map_err(|_| DomainError::invalid("HolidayImportBatch", "covers an unmeasurable span"))?
            .get_days();
        if i64::from(days) > MAX_IMPORT_DAYS {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "covers more than the bounded import span",
            ));
        }
        if self.categories.is_empty() {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "selects no holiday category",
            ));
        }
        if self.warnings.len() > MAX_IMPORT_WARNINGS {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "records more warnings than one import may produce",
            ));
        }
        if self.supersedes == Some(self.source_id) {
            return Err(DomainError::invalid(
                "HolidayImportBatch",
                "cannot supersede its own source revision",
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
    /// The window used when a caller does not narrow one.
    ///
    /// Same instants the existing tests already treat as "effectively open":
    /// 2020-01-01Z through 2099-01-01Z. There is no infinite range type, and
    /// inventing one would be a second way to say the same thing.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            start: parse_utc_timestamp("2020-01-01T00:00:00Z")
                .expect("the default-allow window opens"),
            end: parse_utc_timestamp("2099-01-01T00:00:00Z")
                .expect("the default-allow window closes far enough"),
        }
    }

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

/// An explicit authorization that *narrows* ready work.
///
/// Unarmed ready work is eligible (default-allow). A grant is optional
/// narrowing: a window, a concurrency ceiling, a task whitelist. Disarming is
/// an explicit stop, not a return to unarmed.
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

/// The exception revision that governs one local date, if any does.
///
/// Three rules decide it, in this order:
///
/// * a revision that a later revision supersedes is dead, and a dead revision is
///   never consulted — that is how a refreshed import drops a holiday its source
///   no longer lists without rewriting the row that recorded it;
/// * a **manual** exception beats an imported one covering the same date. A human
///   who closed or opened a day did it knowing the import existed, so recording
///   order is not allowed to decide against them;
/// * otherwise the most recently recorded revision wins.
///
/// An imported exception is skipped entirely under
/// [`HolidayMergePolicy::Ignore`]: the profile has said holidays do not affect
/// this calendar, and a skipped import must not shadow a manual revision either.
#[must_use]
pub fn governing_exception(
    exceptions: &[CalendarExceptionRevision],
    date: civil::Date,
    holiday_merge: HolidayMergePolicy,
) -> Option<&CalendarExceptionRevision> {
    let superseded: BTreeSet<CalendarExceptionId> = exceptions
        .iter()
        .filter_map(|exception| exception.supersedes)
        .collect();
    exceptions
        .iter()
        .enumerate()
        .filter(|(_, exception)| {
            !superseded.contains(&exception.id)
                && exception.covers(date)
                && match exception.provenance {
                    ExceptionProvenance::Manual { .. } => true,
                    ExceptionProvenance::HolidaySource { .. } => {
                        holiday_merge != HolidayMergePolicy::Ignore
                    }
                }
        })
        .max_by_key(|(index, exception)| {
            (
                matches!(exception.provenance, ExceptionProvenance::Manual { .. }),
                exception.created_at,
                *index,
            )
        })
        .map(|(_, exception)| exception)
}

/// Whether a governing exception closes the day.
///
/// A manual revision says so itself. An imported one does not: the *profile*
/// decides what a holiday means, which is why one workspace can treat a public
/// holiday as a closed day and another as an open one from the same import.
#[must_use]
pub fn exception_closes(
    exception: &CalendarExceptionRevision,
    holiday_merge: HolidayMergePolicy,
) -> bool {
    match exception.provenance {
        ExceptionProvenance::Manual { .. } => exception.kind == ExceptionKind::Closed,
        ExceptionProvenance::HolidaySource { .. } => {
            holiday_merge == HolidayMergePolicy::TreatAsClosed
        }
    }
}

/// Resolve the effective calendar state.
///
/// The order of the steps is itself a rule, and it is the reason this function
/// reads top to bottom rather than as a set of independent checks:
///
/// 1. **No active assignment is `unrestricted`, and nothing overrides it.** An
///    override on an unconfigured project is redundant, not authoritative: there
///    is no closed window for it to open, and reporting `override_open` there
///    would make an unconfigured project look governed by a calendar it does not
///    have.
/// 2. The pinned profile revision and the configured zone are validated.
/// 3. The instant is converted to local time, never the other way round.
/// 4. Exceptions, then windows, then the drain lead decide the state.
/// 5. **An override is consulted last**, and only when the calendar would
///    otherwise refuse new work. It opens what policy closed; it never
///    manufactures a policy.
///
/// # Errors
/// * [`DomainError::Invalid`] when an assignment is present without its pinned
///   profile revision, or with a *different* revision — an applied revision is
///   never silently upgraded.
/// * [`DomainError::Invalid`] when the configured time zone is unknown.
pub fn resolve_effective_state(
    input: &CalendarResolution<'_>,
) -> DomainResult<EffectiveCalendarState> {
    // No configured calendar means no restriction. This is the single most
    // important line in the module: absence must never read as "closed" — and,
    // just as importantly, never as "overridden".
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

    let windows = assignment
        .window_override
        .as_deref()
        .unwrap_or(&profile.windows);
    let state = match governing_exception(input.exceptions, local_date, profile.holiday_merge) {
        Some(exception) if exception_closes(exception, profile.holiday_merge) => {
            EffectiveCalendarState::Closed
        }
        Some(_) => EffectiveCalendarState::Open,
        None => match windows.iter().find(|w| w.contains(weekday, local_time)) {
            None => EffectiveCalendarState::Closed,
            Some(window)
                if window.minutes_remaining(local_time)
                    <= i64::from(profile.drain_lead_minutes) =>
            {
                EffectiveCalendarState::Draining
            }
            Some(_) => EffectiveCalendarState::Open,
        },
    };

    // Last, and only over a refusal. `draining` counts as one: it is the state
    // that admits no *new* top-level work, which is precisely what an urgent
    // override exists to obtain.
    let refuses_new_work = matches!(
        state,
        EffectiveCalendarState::Closed | EffectiveCalendarState::Draining
    );
    if refuses_new_work
        && let Some(over) = input.schedule_override
        && over.is_active(input.now, input.mini_project, input.task)
    {
        return Ok(EffectiveCalendarState::OverrideOpen);
    }
    Ok(state)
}
