//! Production-path proof for persisted child-scope calendar windows.

use kontor_core::calendar::{
    CalendarProfileSpec, ChildCalendarWindows, EffectiveCalendarState, HolidayMergePolicy,
    IanaTimeZone, Weekday, WeeklyWindow, WorkCalendarAssignment, WorkScope,
};
use kontor_core::id::{
    CalendarProfileId, ExternalName, ProjectId, SCHEMA_VERSION, SpecVersion, TaskId, Timestamp,
    WorkCalendarId, parse_utc_timestamp,
};
use kontor_core::repository::{
    CalendarRepository, NewProject, NewTask, ProjectRepository, SpecRepository,
};
use kontor_core::state::TaskState;
use kontor_store::SqliteStore;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a UTC instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a display name")
}

fn window(start: &str, end: &str) -> WeeklyWindow {
    WeeklyWindow {
        weekday: Weekday::Monday,
        start: start.parse().expect("a local time"),
        end: end.parse().expect("a local time"),
    }
}

fn fixture(child: WeeklyWindow) -> (tempfile::TempDir, SqliteStore, ProjectId, TaskId) {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");
    let project = ProjectId::generate();
    let task = TaskId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Child calendar project"),
            root_path: name("/tmp/child-calendar-project"),
            created_at: at("2026-08-10T00:00:00Z"),
        })
        .expect("the project is stored");
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: None,
            title: name("Scoped task"),
            module: None,
            state: TaskState::Ready,
            created_at: at("2026-08-10T00:00:00Z"),
        })
        .expect("the task is stored");
    let profile = CalendarProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id: CalendarProfileId::generate(),
        version: SpecVersion::FIRST,
        name: name("Office hours"),
        windows: vec![window("08:00:00", "16:00:00")],
        holiday_merge: HolidayMergePolicy::TreatAsClosed,
        drain_lead_minutes: 0,
    };
    store
        .insert_calendar_profile(&profile)
        .expect("the profile is stored");
    let calendar = WorkCalendarId::generate();
    store
        .assign_calendar(&WorkCalendarAssignment {
            id: calendar,
            project_id: project,
            profile_id: profile.profile_id,
            profile_version: profile.version,
            timezone: IanaTimeZone::parse("UTC").expect("a bundled zone"),
            window_override: None,
            active: true,
            created_at: at("2026-08-10T00:00:00Z"),
            retired_at: None,
        })
        .expect("the calendar is assigned");
    store
        .append_child_windows(&ChildCalendarWindows {
            project_id: project,
            work_calendar_id: calendar,
            scope: WorkScope::Task { task_id: task },
            version: SpecVersion::FIRST,
            windows: vec![child],
            supersedes: None,
            created_at: at("2026-08-10T00:01:00Z"),
        })
        .expect("the child revision is stored");
    (directory, store, project, task)
}

#[test]
fn a_persisted_task_narrowing_changes_production_calendar_admission() {
    let (_directory, store, project, task) = fixture(window("10:00:00", "12:00:00"));
    let inputs = store
        .calendar_inputs(project, at("2026-08-10T09:00:00Z"))
        .expect("the production inputs load");
    assert_eq!(inputs.child_windows.len(), 1);
    assert_eq!(
        inputs
            .resolve(at("2026-08-10T09:00:00Z"), None, task, &[])
            .expect("the narrowed calendar resolves")
            .state,
        EffectiveCalendarState::Closed,
        "the project is open at 09:00 but the task narrowing is not"
    );
    assert_eq!(
        inputs
            .resolve(at("2026-08-10T10:30:00Z"), None, task, &[])
            .expect("the narrowed calendar resolves")
            .state,
        EffectiveCalendarState::Open
    );
}

#[test]
fn a_persisted_task_widening_is_refused_without_a_scoped_override() {
    let (_directory, store, project, task) = fixture(window("07:00:00", "17:00:00"));
    let inputs = store
        .calendar_inputs(project, at("2026-08-10T09:00:00Z"))
        .expect("the production inputs load");
    assert!(
        inputs
            .resolve(at("2026-08-10T09:00:00Z"), None, task, &[])
            .is_err(),
        "stored child hours cannot widen the project calendar by being wired through the store"
    );
}
