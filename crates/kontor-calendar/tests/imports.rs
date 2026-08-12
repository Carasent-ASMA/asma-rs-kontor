//! Holiday imports: parse, normalize, diff and plan, on real document shapes.
//!
//! The mutants this suite exists to kill:
//!
//! * importing a regional day as a national closure;
//! * importing school, optional or observance days nobody selected;
//! * importing a timed or recurring event as an all-day closure;
//! * reading an exclusive `DTEND` as an inclusive one;
//! * a normalization whose digest depends on the order the source listed;
//! * a refresh that hides an addition, a removal or a change;
//! * a preview that changes something;
//! * a planned import that does not supersede the one already applied.

use std::collections::BTreeSet;

use jiff::civil;
use kontor_calendar::import::{ImportRequest, ImportTarget, diff, plan, preview};
use kontor_core::calendar::{
    CalendarExceptionRevision, CountryCode, ExceptionKind, ExceptionProvenance, HolidayCategory,
    HolidayImportKind, HolidayProviderKind, ImportWarningCode,
};
use kontor_core::id::{
    AccountProfileId, CalendarExceptionId, CalendarProfileId, ExternalName, HolidaySourceId,
    IdempotencyKey, ProjectId, SpecVersion, Timestamp, WorkCalendarId, parse_utc_timestamp,
};

const US: &str = include_str!("fixtures/nager_us_2026.json");
const MD: &str = include_str!("fixtures/nager_md_2026.json");
const NO: &str = include_str!("fixtures/nager_no_2026.json");
const GB: &str = include_str!("fixtures/gov_uk_bank_holidays.json");
const ICS: &str = include_str!("fixtures/office_closures.ics");

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid display name")
}

fn date(text: &str) -> civil::Date {
    text.parse().expect("a civil date")
}

fn categories(selection: &[HolidayCategory]) -> BTreeSet<HolidayCategory> {
    selection.iter().copied().collect()
}

fn request(kind: HolidayImportKind, country: &str, subdivision: Option<&str>) -> ImportRequest {
    ImportRequest {
        kind,
        country: CountryCode::parse(country).expect("a country code"),
        subdivision: subdivision.map(name),
        range_start: date("2026-01-01"),
        range_end: date("2026-12-31"),
        categories: categories(HolidayCategory::DEFAULT_SELECTION),
        reference: name("fixture"),
    }
}

/// Every date one preview closed, in order.
fn dates(preview: &kontor_calendar::ImportPreview) -> Vec<String> {
    preview
        .holidays
        .iter()
        .map(|holiday| holiday.start.to_string())
        .collect()
}

fn warning_codes(preview: &kontor_calendar::ImportPreview) -> Vec<ImportWarningCode> {
    preview
        .warnings
        .iter()
        .map(|warning| warning.code)
        .collect()
}

// ---------------------------------------------------------------------------
// Nager
// ---------------------------------------------------------------------------

#[test]
fn the_us_federal_fixture_closes_the_expected_national_dates() {
    let preview = preview(&request(HolidayImportKind::NagerV4, "US", None), US)
        .expect("the fixture is a Nager document");

    assert_eq!(
        dates(&preview),
        vec![
            "2026-01-01",
            "2026-01-19",
            "2026-02-16",
            "2026-05-25",
            "2026-06-19",
            "2026-07-04",
            "2026-09-07",
            "2026-10-12",
            "2026-11-11",
            "2026-11-26",
            "2026-12-25",
        ]
    );
    // Texas Independence Day is a Texas day, and Valentine's Day is an
    // observance. Neither closes a national calendar, and both say why.
    assert_eq!(
        warning_codes(&preview),
        vec![
            ImportWarningCode::FilteredCategory,
            ImportWarningCode::FilteredSubdivision,
        ]
    );
}

#[test]
fn a_named_subdivision_admits_its_own_regional_day() {
    let mut texan = request(HolidayImportKind::NagerV4, "US", Some("US-TX"));
    texan.subdivision = Some(name("US-TX"));
    let preview = preview(&texan, US).expect("the fixture is a Nager document");

    assert!(
        dates(&preview).contains(&"2026-03-02".to_owned()),
        "a request that named Texas gets the Texas day"
    );
}

#[test]
fn the_moldova_and_norway_fixtures_close_their_own_national_dates() {
    let moldova = preview(&request(HolidayImportKind::NagerV4, "MD", None), MD)
        .expect("the fixture is a Nager document");
    assert_eq!(moldova.holidays.len(), 11);
    assert!(
        dates(&moldova).contains(&"2026-08-27".to_owned()),
        "Independence Day"
    );
    assert!(
        dates(&moldova).contains(&"2026-01-07".to_owned()),
        "Orthodox Christmas"
    );

    // Norway's fixture is two years, concatenated the way a multi-year retrieval
    // arrives. The second year is outside the requested range and says so.
    let norway = preview(&request(HolidayImportKind::NagerV4, "NO", None), NO)
        .expect("the fixture is a Nager document");
    assert_eq!(
        dates(&norway).first().map(String::as_str),
        Some("2026-01-01")
    );
    assert!(
        dates(&norway).contains(&"2026-05-17".to_owned()),
        "Constitution Day"
    );
    assert!(
        !dates(&norway).iter().any(|day| day.starts_with("2027")),
        "2027 was not asked for"
    );
    assert_eq!(
        warning_codes(&norway),
        vec![
            ImportWarningCode::OutOfRange,
            ImportWarningCode::OutOfRange,
            ImportWarningCode::OutOfRange,
        ]
    );

    // A wider request takes both years from the same document.
    let mut two_years = request(HolidayImportKind::NagerV4, "NO", None);
    two_years.range_end = date("2027-12-31");
    let both = preview(&two_years, NO).expect("the fixture is a Nager document");
    assert_eq!(both.holidays.len(), 15);
}

#[test]
fn selecting_a_category_by_name_is_the_only_way_to_import_it() {
    let mut with_observances = request(HolidayImportKind::NagerV4, "US", None);
    with_observances.categories =
        categories(&[HolidayCategory::Public, HolidayCategory::Observance]);
    let preview = preview(&with_observances, US).expect("the fixture is a Nager document");

    assert!(
        dates(&preview).contains(&"2026-02-14".to_owned()),
        "an observance arrives only when it was asked for by name"
    );
}

// ---------------------------------------------------------------------------
// GOV.UK
// ---------------------------------------------------------------------------

#[test]
fn each_uk_division_closes_its_own_dates() {
    let england = preview(
        &request(HolidayImportKind::GovUkJson, "GB", Some("GB-ENG")),
        GB,
    )
    .expect("the fixture is a GOV.UK document");
    let wales = preview(
        &request(HolidayImportKind::GovUkJson, "GB", Some("GB-WLS")),
        GB,
    )
    .expect("the fixture is a GOV.UK document");
    let scotland = preview(
        &request(HolidayImportKind::GovUkJson, "GB", Some("GB-SCT")),
        GB,
    )
    .expect("the fixture is a GOV.UK document");
    let ulster = preview(
        &request(HolidayImportKind::GovUkJson, "GB", Some("GB-NIR")),
        GB,
    )
    .expect("the fixture is a GOV.UK document");

    assert_eq!(
        dates(&england),
        dates(&wales),
        "England and Wales share one list"
    );
    assert!(
        dates(&scotland).contains(&"2026-01-02".to_owned()),
        "2nd January"
    );
    assert!(
        dates(&scotland).contains(&"2026-11-30".to_owned()),
        "St Andrew's Day"
    );
    assert!(
        !dates(&england).contains(&"2026-11-30".to_owned()),
        "England does not close for St Andrew's Day"
    );
    assert!(
        dates(&ulster).contains(&"2026-03-17".to_owned()),
        "St Patrick's Day"
    );
    assert!(
        dates(&ulster).contains(&"2026-07-13".to_owned()),
        "the Twelfth, substituted"
    );
    assert!(
        !dates(&scotland).contains(&"2026-07-13".to_owned()),
        "Scotland does not close for the Twelfth"
    );
    // Scotland's summer bank holiday is in August but not the same August day.
    assert!(dates(&scotland).contains(&"2026-08-03".to_owned()));
    assert!(dates(&england).contains(&"2026-08-31".to_owned()));
}

#[test]
fn a_uk_import_must_name_a_division_it_knows() {
    assert!(
        preview(&request(HolidayImportKind::GovUkJson, "GB", None), GB).is_err(),
        "the document holds several divisions and 'the UK's bank holidays' is not one of them"
    );
    assert!(
        preview(
            &request(HolidayImportKind::GovUkJson, "GB", Some("GB-XYZ")),
            GB
        )
        .is_err()
    );
}

#[test]
fn bank_holidays_are_bank_holidays_and_a_public_only_request_takes_none_of_them() {
    let mut public_only = request(HolidayImportKind::GovUkJson, "GB", Some("GB-ENG"));
    public_only.categories = categories(&[HolidayCategory::Public]);
    let preview = preview(&public_only, GB).expect("the fixture is a GOV.UK document");

    assert!(preview.holidays.is_empty());
    assert!(
        preview
            .warnings
            .iter()
            .all(|warning| warning.code == ImportWarningCode::FilteredCategory)
    );
}

// ---------------------------------------------------------------------------
// iCalendar
// ---------------------------------------------------------------------------

#[test]
fn an_ics_feed_yields_all_day_closures_and_refuses_everything_else() {
    let preview = preview(&request(HolidayImportKind::Ical, "NO", None), ICS)
        .expect("the fixture is an iCalendar document");

    assert_eq!(dates(&preview), vec!["2026-01-01", "2026-07-06"]);
    // The exclusive DTEND of 2026-07-11 is the inclusive 2026-07-10.
    assert_eq!(preview.holidays[1].end, date("2026-07-10"));

    let codes = warning_codes(&preview);
    assert!(
        codes.contains(&ImportWarningCode::TimedEvent),
        "the standup"
    );
    assert!(
        codes.contains(&ImportWarningCode::RecurringEvent),
        "the payday rule"
    );
    assert!(
        codes
            .iter()
            .filter(|code| **code == ImportWarningCode::MalformedEntry)
            .count()
            >= 2,
        "one event has no summary and one has no UID"
    );
    assert!(
        codes.contains(&ImportWarningCode::OutOfRange),
        "next year's day"
    );
    assert!(
        codes.contains(&ImportWarningCode::FilteredCategory),
        "the school break"
    );
}

#[test]
fn an_ics_school_break_arrives_only_when_school_days_were_selected() {
    let mut with_school = request(HolidayImportKind::Ical, "NO", None);
    with_school.categories = categories(&[
        HolidayCategory::Public,
        HolidayCategory::Bank,
        HolidayCategory::School,
    ]);
    let preview = preview(&with_school, ICS).expect("the fixture is an iCalendar document");

    let autumn = preview
        .holidays
        .iter()
        .find(|holiday| holiday.start == date("2026-10-05"))
        .expect("the half term is imported");
    assert_eq!(autumn.category, HolidayCategory::School);
    assert_eq!(
        autumn.end,
        date("2026-10-09"),
        "an exclusive DTEND, made inclusive"
    );
}

#[test]
fn every_ics_recurrence_exclusion_is_refused_while_plain_all_day_still_imports() {
    let request = request(HolidayImportKind::Ical, "NO", None);
    for (start, property) in [
        ("", "EXDATE:20260102"),
        ("", "EXRULE:FREQ=YEARLY"),
        ("DTSTART;VALUE=DATE:20260101\r\n", "EXDATE:20260102"),
    ] {
        let document = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:recurring\r\n{start}{property}\r\nSUMMARY:Recurring closure\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let rejected = preview(&request, &document).expect("the calendar document parses");
        assert!(rejected.holidays.is_empty());
        assert_eq!(rejected.warnings[0].code, ImportWarningCode::RecurringEvent);
    }

    let plain = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:plain\r\nDTSTART;VALUE=DATE:20260101\r\nSUMMARY:Plain closure\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let imported = preview(&request, plain).expect("the plain all-day event parses");
    assert_eq!(imported.holidays.len(), 1);
    assert!(imported.warnings.is_empty());
}

#[test]
fn ics_categories_are_explicit_and_unknown_values_are_never_public() {
    let document = |category: Option<&str>| {
        format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:category\r\nDTSTART;VALUE=DATE:20260101\r\n{}SUMMARY:Closure\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            category
                .map(|value| format!("CATEGORIES:{value}\r\n"))
                .unwrap_or_default()
        )
    };
    let default = request(HolidayImportKind::Ical, "NO", None);

    let unknown = preview(&default, &document(Some("unsupported-type")))
        .expect("an unsupported event is reported, not imported");
    assert!(unknown.holidays.is_empty());
    assert_eq!(
        unknown.warnings[0].code,
        ImportWarningCode::UnsupportedEntry
    );
    let ambiguous = preview(&default, &document(Some("public,optional")))
        .expect("mixed category semantics are not guessed");
    assert!(ambiguous.holidays.is_empty());
    assert_eq!(
        ambiguous.warnings[0].code,
        ImportWarningCode::UnsupportedEntry
    );

    let optional = preview(&default, &document(Some("optional")))
        .expect("an unselected known category is filtered");
    assert!(optional.holidays.is_empty());
    assert_eq!(
        optional.warnings[0].code,
        ImportWarningCode::FilteredCategory
    );

    let mut with_optional = default.clone();
    with_optional.categories.insert(HolidayCategory::Optional);
    let selected = preview(&with_optional, &document(Some("optional")))
        .expect("an explicitly selected optional closure imports");
    assert_eq!(selected.holidays.len(), 1);
    assert_eq!(selected.holidays[0].category, HolidayCategory::Optional);

    for category in [None, Some("public")] {
        let imported = preview(&default, &document(category)).expect("public closure imports");
        assert_eq!(imported.holidays.len(), 1);
        assert_eq!(imported.holidays[0].category, HolidayCategory::Public);
    }
}

#[test]
fn a_document_of_the_wrong_shape_is_refused_whole() {
    for (kind, raw) in [
        (HolidayImportKind::NagerV4, "{\"not\":\"an array\"}"),
        (HolidayImportKind::NagerV4, "not json at all"),
        (HolidayImportKind::GovUkJson, "[]"),
        (HolidayImportKind::Ical, "BEGIN:VCARD\nEND:VCARD\n"),
    ] {
        let mut probe = request(kind, "GB", Some("GB-ENG"));
        probe.kind = kind;
        assert!(
            preview(&probe, raw).is_err(),
            "{kind} must refuse a document it cannot read"
        );
    }
}

#[test]
fn provider_scope_and_nager_payload_country_are_bound_to_the_request() {
    let mut unsupported = request(HolidayImportKind::NagerV4, "GB", Some("GB-ENG"));
    assert!(preview(&unsupported, "[]").is_err());

    unsupported = request(HolidayImportKind::GovUkJson, "NO", Some("GB-ENG"));
    assert!(
        preview(
            &unsupported,
            include_str!("fixtures/gov_uk_bank_holidays.json")
        )
        .is_err()
    );

    let norway = request(HolidayImportKind::NagerV4, "NO", None);
    let mismatched = preview(
        &norway,
        r#"[{"date":"2026-01-01","name":"Wrong country","countryCode":"US","global":true,"types":["Public"]}]"#,
    )
    .expect("an entry mismatch is reported without importing it");
    assert!(mismatched.holidays.is_empty());
    assert_eq!(
        mismatched.warnings[0].code,
        ImportWarningCode::MalformedEntry
    );
}

#[test]
fn an_unbounded_or_inverted_request_is_refused_before_a_document_is_read() {
    let mut inverted = request(HolidayImportKind::NagerV4, "US", None);
    inverted.range_end = date("2025-01-01");
    assert!(preview(&inverted, US).is_err());

    let mut unbounded = request(HolidayImportKind::NagerV4, "US", None);
    unbounded.range_end = date("2050-12-31");
    assert!(preview(&unbounded, US).is_err());

    let mut nothing_selected = request(HolidayImportKind::NagerV4, "US", None);
    nothing_selected.categories = BTreeSet::new();
    assert!(preview(&nothing_selected, US).is_err());
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn normalization_is_deterministic_and_independent_of_source_order() {
    let request = request(HolidayImportKind::NagerV4, "US", None);
    let once = preview(&request, US).expect("the fixture parses");
    let twice = preview(&request, US).expect("the fixture parses");
    assert_eq!(once, twice, "the same document gives the same preview");

    // The same holidays, listed backwards, are the same holidays.
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_str(US).expect("the fixture is an array");
    entries.reverse();
    let reversed = serde_json::to_string(&entries).expect("the array re-renders");
    let shuffled = preview(&request, &reversed).expect("the reordered fixture parses");

    assert_eq!(
        once.normalized_hash, shuffled.normalized_hash,
        "the normalized digest is a property of the holidays, not of their order"
    );
    assert_ne!(
        once.raw_hash, shuffled.raw_hash,
        "the raw digest is a property of the bytes, and the bytes did change"
    );
}

// ---------------------------------------------------------------------------
// Diff and plan
// ---------------------------------------------------------------------------

/// The exception rows an earlier import would have left behind.
fn applied_from(preview: &kontor_calendar::ImportPreview) -> Vec<CalendarExceptionRevision> {
    let source_id = HolidaySourceId::generate();
    preview
        .holidays
        .iter()
        .map(|holiday| CalendarExceptionRevision {
            id: CalendarExceptionId::generate(),
            project_id: ProjectId::generate(),
            work_calendar_id: WorkCalendarId::generate(),
            start_date: holiday.start,
            end_date: holiday.end,
            kind: ExceptionKind::Closed,
            label: holiday.label.clone(),
            provenance: ExceptionProvenance::HolidaySource { source_id },
            supersedes: None,
            created_at: at("2026-01-01T00:00:00Z"),
        })
        .collect()
}

#[test]
fn a_refresh_reports_additions_removals_and_changes_before_anything_is_applied() {
    let request = request(HolidayImportKind::NagerV4, "US", None);
    let original = preview(&request, US).expect("the fixture parses");
    let applied = applied_from(&original);

    // Nothing moved: every day is unchanged and the refresh is a no-op.
    let unchanged = diff(&original, &applied);
    assert!(unchanged.is_empty(), "an identical refresh changes nothing");
    assert_eq!(unchanged.unchanged, 11);

    // A year in which one day moved, one was renamed and one was withdrawn.
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_str(US).expect("the fixture is an array");
    entries[0]["date"] = serde_json::json!("2026-01-02");
    entries[1]["name"] = serde_json::json!("Martin Luther King Jr. Day");
    entries.remove(10);
    let amended = serde_json::to_string(&entries).expect("the array re-renders");
    let refreshed = preview(&request, &amended).expect("the amended fixture parses");

    let difference = diff(&refreshed, &applied);
    assert_eq!(
        difference
            .added
            .iter()
            .map(|day| day.start.to_string())
            .collect::<Vec<_>>(),
        vec!["2026-01-02"]
    );
    assert_eq!(
        difference
            .removed
            .iter()
            .map(|day| day.start_date.to_string())
            .collect::<Vec<_>>(),
        vec!["2026-01-01", "2026-12-25"]
    );
    assert_eq!(difference.changed.len(), 1, "the renamed day");
    assert_eq!(
        difference.changed[0].incoming.start.to_string(),
        "2026-01-19"
    );
    assert!(!difference.is_empty());
}

#[test]
fn a_diff_never_touches_a_manual_exception() {
    let request = request(HolidayImportKind::NagerV4, "US", None);
    let preview = preview(&request, US).expect("the fixture parses");
    let manual = CalendarExceptionRevision {
        id: CalendarExceptionId::generate(),
        project_id: ProjectId::generate(),
        work_calendar_id: WorkCalendarId::generate(),
        start_date: date("2026-06-05"),
        end_date: date("2026-06-05"),
        kind: ExceptionKind::Closed,
        label: name("Company offsite"),
        provenance: ExceptionProvenance::Manual {
            by: AccountProfileId::generate(),
        },
        supersedes: None,
        created_at: at("2026-01-01T00:00:00Z"),
    };
    let mut applied = applied_from(&preview);
    applied.push(manual);

    let difference = diff(&preview, &applied);
    assert!(
        difference.removed.is_empty(),
        "an import does not remove a decision a human made"
    );
    assert!(difference.added.is_empty());
    assert!(difference.changed.is_empty());
}

#[test]
fn a_planned_import_carries_provenance_lineage_and_the_revision_it_replaces() {
    let request = request(HolidayImportKind::NagerV4, "US", None);
    let first = preview(&request, US).expect("the fixture parses");
    let project_id = ProjectId::generate();
    let work_calendar_id = WorkCalendarId::generate();
    let profile_id = CalendarProfileId::generate();

    let target = ImportTarget {
        project_id,
        work_calendar_id,
        profile_id,
        profile_version: SpecVersion::FIRST,
        applied: &[],
        supersedes: None,
        idempotency_key: IdempotencyKey::parse("import-us-2026").expect("a key"),
        retrieved_at: at("2026-01-01T09:00:00Z"),
        applied_at: at("2026-01-01T09:00:05Z"),
    };
    let application = plan(&first, &target).expect("the plan is valid");

    assert_eq!(application.exceptions.len(), first.holidays.len());
    assert_eq!(application.batch.applied_exceptions, 11);
    assert_eq!(application.batch.kind, HolidayImportKind::NagerV4);
    assert_eq!(application.batch.requested_start, date("2026-01-01"));
    assert_eq!(application.batch.requested_end, date("2026-12-31"));
    assert_eq!(application.batch.supersedes, None);
    assert_eq!(application.revision.provider, HolidayProviderKind::Ical);
    assert_eq!(application.revision.range_start, date("2026-01-01"));
    assert_eq!(application.revision.range_end, date("2026-12-25"));
    assert_eq!(application.revision.raw_hash, first.raw_hash);
    assert!(
        application.exceptions.iter().all(|exception| {
            exception.kind == ExceptionKind::Closed
                && exception.project_id == project_id
                && exception.work_calendar_id == work_calendar_id
                && exception.provenance
                    == ExceptionProvenance::HolidaySource {
                        source_id: application.revision.id,
                    }
                && exception.supersedes.is_none()
        }),
        "a first import cites its own revision and supersedes nothing"
    );

    // The second import replaces the first, and each day that survived says
    // which row it replaces.
    let applied: Vec<CalendarExceptionRevision> = application
        .exceptions
        .iter()
        .cloned()
        .map(|mut exception| {
            exception.project_id = project_id;
            exception.work_calendar_id = work_calendar_id;
            exception
        })
        .collect();
    let second_target = ImportTarget {
        applied: &applied,
        supersedes: Some(application.revision.id),
        idempotency_key: IdempotencyKey::parse("import-us-2026-refresh").expect("a key"),
        ..target.clone()
    };
    let refresh = plan(&first, &second_target).expect("the plan is valid");

    assert_eq!(refresh.batch.supersedes, Some(application.revision.id));
    assert!(
        refresh
            .exceptions
            .iter()
            .all(|exception| exception.supersedes.is_some()),
        "every re-imported day names the row it replaces"
    );
    assert!(refresh.diff.is_empty(), "the same document changes nothing");
}

#[test]
fn a_preview_writes_nothing_and_a_plan_is_still_only_a_value() {
    let request = request(HolidayImportKind::GovUkJson, "GB", Some("GB-SCT"));
    let preview = preview(&request, GB).expect("the fixture parses");
    let target = ImportTarget {
        project_id: ProjectId::generate(),
        work_calendar_id: WorkCalendarId::generate(),
        profile_id: CalendarProfileId::generate(),
        profile_version: SpecVersion::FIRST,
        applied: &[],
        supersedes: None,
        idempotency_key: IdempotencyKey::parse("import-gb-sct-2026").expect("a key"),
        retrieved_at: at("2026-01-01T09:00:00Z"),
        applied_at: at("2026-01-01T09:00:05Z"),
    };

    // Planning twice from one preview produces the same content, differing only
    // in the identifiers each apply would mint.
    let first = plan(&preview, &target).expect("the plan is valid");
    let second = plan(&preview, &target).expect("the plan is valid");
    assert_eq!(
        first.batch.applied_exceptions,
        second.batch.applied_exceptions
    );
    assert_eq!(
        first.revision.normalized_hash,
        second.revision.normalized_hash
    );
    assert_ne!(
        first.revision.id, second.revision.id,
        "each apply is its own revision until one of them is committed"
    );
}
