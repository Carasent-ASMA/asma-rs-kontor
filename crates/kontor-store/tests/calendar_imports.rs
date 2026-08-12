//! Applying a holiday import, and what the store refuses to let one do.
//!
//! The mutants this suite exists to kill:
//!
//! * applying a source revision without its exceptions, or the other way round;
//! * a replayed import writing a second copy of itself;
//! * two imports both claiming to be the one a calendar has applied;
//! * a refreshed import that keeps closing a day its source dropped;
//! * a refresh that discards a human's exception;
//! * an imported exception whose source revision does not exist;
//! * rewriting or deleting an applied import through direct SQL;
//! * an offline resolution that disagrees with the state just applied.

use jiff::civil;
use kontor_calendar::import::{ImportRequest, ImportTarget, plan, preview};
use kontor_calendar::resolve::{ResolutionRequest, resolve};
use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, CountryCode, EffectiveCalendarState,
    ExceptionKind, ExceptionProvenance, HolidayCategory, HolidayImportKind, HolidayMergePolicy,
    IanaTimeZone, Weekday, WeeklyWindow, WorkCalendarAssignment,
};
use kontor_core::id::{
    AccountProfileId, CalendarExceptionId, CalendarProfileId, ExternalName, HolidaySourceId,
    IdempotencyKey, ProjectId, SCHEMA_VERSION, SpecVersion, Timestamp, WorkCalendarId,
    parse_utc_timestamp,
};
use kontor_core::repository::{
    CalendarRepository, NewProject, ProjectRepository, RepositoryError, SpecRepository,
};
use kontor_store::SqliteStore;
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::path::PathBuf;
use tempfile::TempDir;

/// Two national days in 2026, in the Nager document shape.
const FIRST_IMPORT: &str = r#"[
  {"date":"2026-05-17","localName":"Grunnlovsdag","name":"Constitution Day","countryCode":"NO","fixed":true,"global":true,"counties":null,"launchYear":null,"types":["Public"]},
  {"date":"2026-08-10","localName":"Fridag","name":"A day off","countryCode":"NO","fixed":true,"global":true,"counties":null,"launchYear":null,"types":["Public"]}
]"#;

/// The same year, with the August day withdrawn and the May day renamed.
const REFRESHED_IMPORT: &str = r#"[
  {"date":"2026-05-17","localName":"Grunnlovsdagen","name":"Norwegian Constitution Day","countryCode":"NO","fixed":true,"global":true,"counties":null,"launchYear":null,"types":["Public"]}
]"#;

struct Fixture {
    _directory: TempDir,
    path: PathBuf,
    store: SqliteStore,
    project: ProjectId,
    account: AccountProfileId,
    profile: CalendarProfileSpec,
    assignment: WorkCalendarAssignment,
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid display name")
}

fn date(text: &str) -> civil::Date {
    text.parse().expect("a civil date")
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");

    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Calendar project"),
            root_path: name("/tmp/calendar-project"),
            created_at: at("2026-01-01T00:00:00Z"),
        })
        .expect("a project is created");

    let profile = CalendarProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id: CalendarProfileId::generate(),
        version: SpecVersion::FIRST,
        name: name("Office hours"),
        windows: vec![
            WeeklyWindow {
                weekday: Weekday::Monday,
                start: "08:00:00".parse().expect("a civil time"),
                end: "16:00:00".parse().expect("a civil time"),
            },
            WeeklyWindow {
                weekday: Weekday::Sunday,
                start: "08:00:00".parse().expect("a civil time"),
                end: "16:00:00".parse().expect("a civil time"),
            },
        ],
        holiday_merge: HolidayMergePolicy::TreatAsClosed,
        drain_lead_minutes: 30,
    };
    store
        .insert_calendar_profile(&profile)
        .expect("the profile revision is stored");

    let assignment = WorkCalendarAssignment {
        id: WorkCalendarId::generate(),
        project_id: project,
        profile_id: profile.profile_id,
        profile_version: profile.version,
        timezone: IanaTimeZone::parse("Europe/Oslo").expect("a bundled tzdb zone"),
        window_override: None,
        active: true,
        created_at: at("2026-01-01T00:00:00Z"),
        retired_at: None,
    };
    store
        .assign_calendar(&assignment)
        .expect("the assignment is stored");

    Fixture {
        _directory: directory,
        path,
        store,
        project,
        account: AccountProfileId::generate(),
        profile,
        assignment,
    }
}

fn request() -> ImportRequest {
    ImportRequest {
        kind: HolidayImportKind::NagerV4,
        country: CountryCode::parse("NO").expect("a country code"),
        subdivision: None,
        range_start: date("2026-01-01"),
        range_end: date("2026-12-31"),
        categories: HolidayCategory::DEFAULT_SELECTION.iter().copied().collect(),
        reference: name("https://example.test/holidays/NO/2026"),
    }
}

/// Apply one document to the fixture's calendar, superseding whatever is
/// currently applied.
fn apply(
    fixture: &Fixture,
    raw: &str,
    key: &str,
    applied_at: &str,
) -> Result<usize, RepositoryError> {
    let preview = preview(&request(), raw).expect("the document parses");
    let applied = fixture
        .store
        .applied_exceptions(fixture.project, fixture.assignment.id)
        .expect("the read succeeds");
    let current = fixture
        .store
        .applied_import(fixture.project, fixture.assignment.id)
        .expect("the read succeeds")
        .map(|batch| batch.source_id);
    let application = plan(
        &preview,
        &ImportTarget {
            project_id: fixture.project,
            work_calendar_id: fixture.assignment.id,
            profile_id: fixture.profile.profile_id,
            profile_version: fixture.profile.version,
            applied: &applied,
            supersedes: current,
            idempotency_key: IdempotencyKey::parse(key).expect("a key"),
            retrieved_at: at(applied_at),
            applied_at: at(applied_at),
        },
    )
    .expect("the plan is valid");
    fixture
        .store
        .apply_holiday_import(
            &application.batch,
            &application.revision,
            &application.exceptions,
        )
        .map(|batch| batch.applied_exceptions as usize)
}

/// The state the store's own rows resolve to at one instant.
fn state_from_store(fixture: &Fixture, now: &str) -> EffectiveCalendarState {
    let assignment = fixture
        .store
        .active_assignment(fixture.project)
        .expect("the read succeeds")
        .expect("an assignment exists");
    let profile = fixture
        .store
        .get_calendar_profile(assignment.profile_id, assignment.profile_version)
        .expect("the read succeeds")
        .expect("the pinned revision exists");
    let exceptions = fixture
        .store
        .applied_exceptions(fixture.project, assignment.id)
        .expect("the read succeeds");

    resolve(&ResolutionRequest {
        now: at(now),
        assignment: Some(&assignment),
        profile: Some(&profile),
        exceptions: &exceptions,
        child_windows: None,
        overrides: &[],
        terminal_goals: &[],
        mini_project: None,
        task: None,
    })
    .expect("the stored calendar resolves")
    .state
}

fn rows(fixture: &Fixture, sql: &str) -> i64 {
    let connection = Connection::open(&fixture.path).expect("the database opens");
    connection
        .query_row(sql, [], |row| row.get(0))
        .expect("the count is readable")
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

#[test]
fn an_import_applies_its_revision_provenance_and_exceptions_together() {
    let fixture = fixture();
    assert_eq!(
        apply(
            &fixture,
            FIRST_IMPORT,
            "import-no-2026",
            "2026-01-02T09:00:00Z"
        )
        .expect("the import applies"),
        2
    );

    let batch = fixture
        .store
        .applied_import(fixture.project, fixture.assignment.id)
        .expect("the read succeeds")
        .expect("an import is applied");
    assert_eq!(batch.kind, HolidayImportKind::NagerV4);
    assert_eq!(batch.applied_exceptions, 2);
    assert_eq!(batch.requested_start, date("2026-01-01"));
    assert_eq!(batch.supersedes, None);
    assert!(batch.warnings.is_empty());

    let exceptions = fixture
        .store
        .applied_exceptions(fixture.project, fixture.assignment.id)
        .expect("the read succeeds");
    assert_eq!(exceptions.len(), 2);
    assert!(exceptions.iter().all(|exception| {
        exception.kind == ExceptionKind::Closed
            && exception.provenance
                == ExceptionProvenance::HolidaySource {
                    source_id: batch.source_id,
                }
    }));

    // 2026-05-17 is a Sunday the profile would otherwise open; the import closes
    // it, and only after it was applied.
    assert_eq!(
        state_from_store(&fixture, "2026-05-17T08:00:00Z"),
        EffectiveCalendarState::Closed
    );
    assert_eq!(
        state_from_store(&fixture, "2026-05-24T08:00:00Z"),
        EffectiveCalendarState::Open,
        "the following Sunday is an ordinary open day"
    );
}

#[test]
fn a_preview_alone_changes_no_row() {
    let fixture = fixture();
    let preview = preview(&request(), FIRST_IMPORT).expect("the document parses");
    assert_eq!(preview.holidays.len(), 2);

    assert_eq!(rows(&fixture, "SELECT count(*) FROM holiday_sources"), 0);
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM calendar_exceptions"),
        0
    );
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM holiday_import_batches"),
        0
    );
    assert_eq!(
        state_from_store(&fixture, "2026-05-17T08:00:00Z"),
        EffectiveCalendarState::Open,
        "an unapplied preview closes nothing"
    );
}

#[test]
fn a_replayed_idempotency_key_returns_the_original_apply() {
    let fixture = fixture();
    apply(
        &fixture,
        FIRST_IMPORT,
        "import-no-2026",
        "2026-01-02T09:00:00Z",
    )
    .expect("the import applies");
    let original = fixture
        .store
        .applied_import(fixture.project, fixture.assignment.id)
        .expect("the read succeeds")
        .expect("an import is applied");

    // The same key again, with a different document behind it. The store returns
    // what it wrote the first time and writes nothing.
    apply(
        &fixture,
        REFRESHED_IMPORT,
        "import-no-2026",
        "2026-02-02T09:00:00Z",
    )
    .expect("the replay is not an error");

    let current = fixture
        .store
        .applied_import(fixture.project, fixture.assignment.id)
        .expect("the read succeeds")
        .expect("an import is applied");
    assert_eq!(current, original);
    assert_eq!(rows(&fixture, "SELECT count(*) FROM holiday_sources"), 1);
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM calendar_exceptions"),
        2
    );
}

#[test]
fn a_second_import_must_supersede_the_one_already_applied() {
    let fixture = fixture();
    apply(
        &fixture,
        FIRST_IMPORT,
        "import-no-2026",
        "2026-01-02T09:00:00Z",
    )
    .expect("the import applies");

    // A plan built as though nothing were applied yet.
    let preview = preview(&request(), REFRESHED_IMPORT).expect("the document parses");
    let application = plan(
        &preview,
        &ImportTarget {
            project_id: fixture.project,
            work_calendar_id: fixture.assignment.id,
            profile_id: fixture.profile.profile_id,
            profile_version: fixture.profile.version,
            applied: &[],
            supersedes: None,
            idempotency_key: IdempotencyKey::parse("import-no-2026-second").expect("a key"),
            retrieved_at: at("2026-02-02T09:00:00Z"),
            applied_at: at("2026-02-02T09:00:00Z"),
        },
    )
    .expect("the plan is valid");

    let refused = fixture.store.apply_holiday_import(
        &application.batch,
        &application.revision,
        &application.exceptions,
    );
    assert!(
        matches!(refused, Err(RepositoryError::Conflict { .. })),
        "two imports cannot both be the one this calendar applied"
    );
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM holiday_import_batches"),
        1,
        "the refused apply left nothing behind"
    );
    assert_eq!(rows(&fixture, "SELECT count(*) FROM holiday_sources"), 1);
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM calendar_exceptions"),
        2
    );
}

#[test]
fn a_refresh_stops_a_withdrawn_holiday_without_deleting_its_history() {
    let fixture = fixture();
    apply(
        &fixture,
        FIRST_IMPORT,
        "import-no-2026",
        "2026-01-02T09:00:00Z",
    )
    .expect("the first import applies");
    assert_eq!(
        state_from_store(&fixture, "2026-08-10T08:00:00Z"),
        EffectiveCalendarState::Closed
    );

    apply(
        &fixture,
        REFRESHED_IMPORT,
        "import-no-2026-refresh",
        "2026-02-02T09:00:00Z",
    )
    .expect("the refresh applies");

    // The August day is no longer listed, so it no longer closes.
    assert_eq!(
        state_from_store(&fixture, "2026-08-10T08:00:00Z"),
        EffectiveCalendarState::Open
    );
    // The May day still closes, under the new revision.
    assert_eq!(
        state_from_store(&fixture, "2026-05-17T08:00:00Z"),
        EffectiveCalendarState::Closed
    );

    let applied = fixture
        .store
        .applied_exceptions(fixture.project, fixture.assignment.id)
        .expect("the read succeeds");
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].label, name("Norwegian Constitution Day"));
    assert!(
        applied[0].supersedes.is_some(),
        "the re-imported day names the row it replaced"
    );

    // Nothing was deleted: both revisions are still there to audit.
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM calendar_exceptions"),
        3
    );
    assert_eq!(rows(&fixture, "SELECT count(*) FROM holiday_sources"), 2);
}

#[test]
fn a_manual_exception_survives_a_refresh_that_drops_every_import() {
    let fixture = fixture();
    apply(
        &fixture,
        FIRST_IMPORT,
        "import-no-2026",
        "2026-01-02T09:00:00Z",
    )
    .expect("the first import applies");

    let manual = CalendarExceptionRevision {
        id: CalendarExceptionId::generate(),
        project_id: fixture.project,
        work_calendar_id: fixture.assignment.id,
        start_date: date("2026-06-07"),
        end_date: date("2026-06-07"),
        kind: ExceptionKind::Closed,
        label: name("Company offsite"),
        provenance: ExceptionProvenance::Manual {
            by: fixture.account,
        },
        supersedes: None,
        created_at: at("2026-03-01T00:00:00Z"),
    };
    fixture
        .store
        .append_exception(&manual)
        .expect("the manual exception is appended");

    apply(
        &fixture,
        REFRESHED_IMPORT,
        "import-no-2026-refresh",
        "2026-04-02T09:00:00Z",
    )
    .expect("the refresh applies");

    let applied = fixture
        .store
        .applied_exceptions(fixture.project, fixture.assignment.id)
        .expect("the read succeeds");
    assert!(
        applied.iter().any(|exception| exception.id == manual.id),
        "an import refresh does not withdraw a decision a human made"
    );
    // 2026-06-07 is a Sunday the profile opens and the human closed.
    assert_eq!(
        state_from_store(&fixture, "2026-06-07T08:00:00Z"),
        EffectiveCalendarState::Closed
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[test]
fn an_imported_exception_must_name_a_holiday_source_that_exists() {
    let fixture = fixture();
    let orphan = CalendarExceptionRevision {
        id: CalendarExceptionId::generate(),
        project_id: fixture.project,
        work_calendar_id: fixture.assignment.id,
        start_date: date("2026-05-17"),
        end_date: date("2026-05-17"),
        kind: ExceptionKind::Closed,
        label: name("A holiday from nowhere"),
        provenance: ExceptionProvenance::HolidaySource {
            source_id: HolidaySourceId::generate(),
        },
        supersedes: None,
        created_at: at("2026-01-02T09:00:00Z"),
    };

    assert!(
        fixture.store.append_exception(&orphan).is_err(),
        "a closure must be explainable by a source revision that exists"
    );
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM calendar_exceptions"),
        0
    );
}

#[test]
fn an_import_whose_exceptions_belong_elsewhere_is_refused_before_any_write() {
    let fixture = fixture();
    let preview = preview(&request(), FIRST_IMPORT).expect("the document parses");
    let mut application = plan(
        &preview,
        &ImportTarget {
            project_id: fixture.project,
            work_calendar_id: fixture.assignment.id,
            profile_id: fixture.profile.profile_id,
            profile_version: fixture.profile.version,
            applied: &[],
            supersedes: None,
            idempotency_key: IdempotencyKey::parse("import-no-2026").expect("a key"),
            retrieved_at: at("2026-01-02T09:00:00Z"),
            applied_at: at("2026-01-02T09:00:00Z"),
        },
    )
    .expect("the plan is valid");
    application.exceptions[1].work_calendar_id = WorkCalendarId::generate();

    assert!(
        fixture
            .store
            .apply_holiday_import(
                &application.batch,
                &application.revision,
                &application.exceptions,
            )
            .is_err()
    );
    assert_eq!(rows(&fixture, "SELECT count(*) FROM holiday_sources"), 0);
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM calendar_exceptions"),
        0
    );
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM holiday_import_batches"),
        0
    );
}

#[test]
fn an_applied_import_cannot_be_rewritten_or_deleted_through_direct_sql() {
    let fixture = fixture();
    apply(
        &fixture,
        FIRST_IMPORT,
        "import-no-2026",
        "2026-01-02T09:00:00Z",
    )
    .expect("the import applies");

    let connection = Connection::open(&fixture.path).expect("the database opens");
    assert!(
        connection
            .execute(
                "UPDATE holiday_import_batches SET applied_exceptions = 0",
                []
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM holiday_import_batches", [])
            .is_err()
    );
    assert_eq!(
        rows(&fixture, "SELECT count(*) FROM holiday_import_batches"),
        1
    );
}

#[test]
fn the_categories_a_request_selected_are_recorded_with_the_apply() {
    let fixture = fixture();
    apply(
        &fixture,
        FIRST_IMPORT,
        "import-no-2026",
        "2026-01-02T09:00:00Z",
    )
    .expect("the import applies");

    let batch = fixture
        .store
        .applied_import(fixture.project, fixture.assignment.id)
        .expect("the read succeeds")
        .expect("an import is applied");
    assert_eq!(
        batch.categories,
        BTreeSet::from([HolidayCategory::Public, HolidayCategory::Bank]),
        "an empty result and a filtered one are different facts, and the filter is stored"
    );
}
