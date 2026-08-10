//! Reconciliation epochs: what a census may conclude, and what it may not.
//!
//! The one rule everything here defends: **absence is not an outcome.** A bound
//! session that a completed census did not find has lost contact — which is a
//! statement about what Kontor knows, not about whether the work finished. The
//! run's lifecycle, its terminal outcome and its closure evidence are the same
//! after reconciliation as before it, every time.
//!
//! The mutants this suite exists to kill:
//!
//! * mapping a missing session, a changed generation or an unreachable runtime
//!   to succeeded, failed, cancelled or any terminal lifecycle;
//! * applying absence from a partial or failed census;
//! * attaching an unbound native session to a run, or rebinding a run to a new
//!   generation on its own;
//! * re-running a census and recording the facts, or the effects, twice.

use std::collections::BTreeMap;

use kontor_core::id::{
    AgentRunId, CanonicalDocument, ExternalId, ExternalName, ProjectId, RuntimeKindKey, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::repository::RunRepository;
use kontor_core::state::{
    DerivedRunState, DesiredRunState, Freshness, NativeRuntimeIdentity, ObservedRunState,
    RunLifecycle, RuntimeContact,
};
use kontor_store::{
    CensusItem, ControlObservation, EpochKey, EpochStatus, ReconciliationEpochId, SqliteStore,
};
use rusqlite::Connection;
use tempfile::TempDir;

/// Four censuses of one generation, in order.
const EPOCHS: &str = include_str!("fixtures/runtime/reconciliation_epochs.json");

const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
const RUN_A: &str = "0193f000-0000-7000-8000-000000000040";
const RUN_B: &str = "0193f000-0000-7000-8000-000000000041";

/// Two runs, each bound to one native session in generation 1.
const FIXTURE_SQL: &str = "\
INSERT INTO projects (id, name, root_path, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1, '2026-08-09T10:00:00Z'); \
INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at) \
VALUES ('0193f000-0000-7000-8000-000000000010', '0193f000-0000-7000-8000-000000000001', \
        'T', 'in_progress', 1, '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z'); \
INSERT INTO team_templates (project_id, template_id, version, name, definition, \
        definition_hash, role_authority, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000001', '0193f000-0000-7000-8000-000000000020', 1, \
        'Team', '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '[]', \
        '2026-08-09T10:00:00Z'); \
INSERT INTO team_runs (id, project_id, task_id, template_id, template_version, snapshot, \
        snapshot_hash, lifecycle, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000035', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000010', '0193f000-0000-7000-8000-000000000020', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'running', 1, \
        '2026-08-09T10:00:00Z'); \
INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle, desired_state, \
        observed_state, derived_state, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000040', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000035', 'maker.primary', 'running', 'run_requested', \
        'unknown', 'pending_confirmation', 1, '2026-08-09T10:00:00Z'), \
       ('0193f000-0000-7000-8000-000000000041', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000035', 'maker.second', 'running', 'run_requested', \
        'unknown', 'pending_confirmation', 1, '2026-08-09T10:00:00Z'); \
INSERT INTO runtime_bindings (id, project_id, agent_run_id, runtime_kind, host, generation, \
        native_id, bound_at) \
VALUES ('0193f000-0000-7000-8000-000000000050', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1, 'session-1', \
        '2026-08-09T10:00:00Z'), \
       ('0193f000-0000-7000-8000-000000000051', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000041', 'generic.runtime', 'host-1', 1, 'session-2', \
        '2026-08-09T10:00:00Z');";

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
}

impl Fixture {
    fn restart(self) -> Self {
        let Self {
            _directory,
            path,
            store,
            project,
        } = self;
        drop(store);
        let store = SqliteStore::open(&path).expect("the store reopens");
        Self {
            _directory,
            path,
            store,
            project,
        }
    }
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");
    Connection::open(&path)
        .expect("a raw connection opens")
        .execute_batch(FIXTURE_SQL)
        .expect("the fixture inserts");
    Fixture {
        _directory: directory,
        path,
        store,
        project: ProjectId::parse(PROJECT).expect("a canonical id"),
    }
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn now() -> Timestamp {
    at("2026-08-09T10:00:00Z")
}

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a valid external id")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker
    }))
    .expect("a canonical document")
}

fn identity(native: &str, generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("generic.runtime").expect("a valid runtime key"),
        host: ExternalName::parse("host-1").expect("a valid host"),
        generation,
        native_id: external(native),
    }
}

fn epoch_key(fixture: &Fixture, key: &str, generation: u64) -> EpochKey {
    EpochKey {
        project_id: fixture.project,
        runtime_kind: RuntimeKindKey::parse("generic.runtime").expect("a valid runtime key"),
        host: ExternalName::parse("host-1").expect("a valid host"),
        generation,
        reconciliation_key: external(key),
    }
}

fn census_item(native: &str, sequence: u64, observed: ObservedRunState) -> CensusItem {
    CensusItem {
        identity: identity(native, 1),
        native_event_id: Some(external(&format!("{native}-{sequence}"))),
        native_sequence: sequence,
        observed,
        contact: RuntimeContact::Reachable,
        freshness: Freshness::Fresh,
        raw: document(&format!("{native}-{sequence}")),
        audit_ref: external(&format!("audit-{native}-{sequence}")),
        observed_at: now(),
    }
}

fn rows(fixture: &Fixture) -> BTreeMap<&'static str, i64> {
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    [
        "runtime_events",
        "runtime_reconciliation_epochs",
        "runtime_reconciliation_members",
        "runtime_reconciliation_results",
        "runtime_bindings",
    ]
    .into_iter()
    .map(|table| {
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|_| panic!("`{table}` is countable"));
        (table, count)
    })
    .collect()
}

/// Every dimension of one run, so a test can prove which ones moved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunState {
    lifecycle: RunLifecycle,
    desired: DesiredRunState,
    observed: ObservedRunState,
    derived: DerivedRunState,
    revision: u64,
    terminal: bool,
    closed: bool,
}

fn run_state(fixture: &Fixture, run: &str) -> RunState {
    let stored = fixture
        .store
        .get_agent_run(
            fixture.project,
            AgentRunId::parse(run).expect("a canonical id"),
        )
        .expect("the read succeeds")
        .expect("the run exists");
    RunState {
        lifecycle: stored.projection.lifecycle,
        desired: stored.projection.desired,
        observed: stored.projection.observed,
        derived: stored.projection.derived,
        revision: stored.revision.get(),
        terminal: stored.terminal.is_some(),
        closed: stored.closed_at.is_some(),
    }
}

/// Run one whole census: begin, observe every item, finish.
fn sweep(
    fixture: &Fixture,
    key: &str,
    items: &[CensusItem],
    authoritative: bool,
) -> ReconciliationEpochId {
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(fixture, key, 1), now())
        .expect("the epoch begins");
    for item in items {
        fixture
            .store
            .observe_census_item(epoch.epoch_id, fixture.project, item)
            .expect("the census item is recorded");
    }
    fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, authoritative, now())
        .expect("the census finishes");
    epoch.epoch_id
}

// ---------------------------------------------------------------------------
// Epoch identity and persistence
// ---------------------------------------------------------------------------

#[test]
fn reconciliation_epoch_survives_restart() {
    let fixture = fixture();
    let begun = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch begins");
    assert_eq!(begun.status, EpochStatus::InProgress);
    fixture
        .store
        .observe_census_item(
            begun.epoch_id,
            fixture.project,
            &census_item("session-1", 2, ObservedRunState::Running),
        )
        .expect("the first item is recorded");

    // The process dies mid-census. Reopening with the same key continues the
    // same sweep, against the same moment — not a second one against now.
    let fixture = fixture.restart();
    let reopened = fixture
        .store
        .begin_reconciliation_epoch(
            &epoch_key(&fixture, "sweep-1", 1),
            at("2026-08-09T12:00:00Z"),
        )
        .expect("the epoch reopens");
    assert_eq!(reopened.epoch_id, begun.epoch_id);
    assert_eq!(reopened.census_start_cursor, begun.census_start_cursor);
    assert_eq!(reopened.started_at, begun.started_at);
    assert_eq!(reopened.status, EpochStatus::InProgress);
    assert_eq!(rows(&fixture)["runtime_reconciliation_epochs"], 1);

    // A different key is a different census.
    let other = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-2", 1), now())
        .expect("a second epoch begins");
    assert_ne!(other.epoch_id, begun.epoch_id);
    assert_eq!(rows(&fixture)["runtime_reconciliation_epochs"], 2);
}

#[test]
fn completed_epoch_records_membership_and_completion_cursor() {
    let fixture = fixture();
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch begins");
    for native in ["session-1", "session-2"] {
        let outcome = fixture
            .store
            .observe_census_item(
                epoch.epoch_id,
                fixture.project,
                &census_item(native, 2, ObservedRunState::Running),
            )
            .expect("the census item is recorded");
        assert!(!outcome.orphaned, "{native} is bound");
        assert!(outcome.reduced, "a present session confirms its run");
        assert!(
            outcome.observation_cursor > epoch.census_start_cursor,
            "membership cites an observation appended during this census"
        );
    }

    let summary = fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, true, now())
        .expect("the census completes");
    assert_eq!(summary.status, EpochStatus::Completed);
    assert_eq!(summary.present, 2);
    assert_eq!(summary.lost_contact, 0);
    let completion = summary
        .completion_cursor
        .expect("a completed census records where it completed");
    assert!(completion >= epoch.census_start_cursor);
    assert_eq!(rows(&fixture)["runtime_reconciliation_members"], 2);

    // Every membership row cites a real, persisted control-plane event.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let dangling: i64 = connection
        .query_row(
            "SELECT count(*) FROM runtime_reconciliation_members
             WHERE observation_cursor IS NOT NULL
               AND observation_cursor NOT IN (SELECT cursor FROM runtime_events)",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(dangling, 0);

    // And it all survives a restart.
    let fixture = fixture.restart();
    let reopened = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch reopens");
    assert_eq!(reopened.status, EpochStatus::Completed);
    assert_eq!(reopened.completion_cursor, Some(completion));
}

#[test]
fn generation_change_is_persisted_without_rebinding() {
    let fixture = fixture();
    let before = run_state(&fixture, RUN_A);

    // The runtime restarted: same native id, new generation. That is evidence,
    // and it is reconciliation input — it is not permission to move a binding.
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-gen-2", 2), now())
        .expect("the epoch begins");
    let outcome = fixture
        .store
        .observe_census_item(
            epoch.epoch_id,
            fixture.project,
            &CensusItem {
                identity: identity("session-1", 2),
                ..census_item("session-1", 2, ObservedRunState::Running)
            },
        )
        .expect("the census item is recorded");
    assert!(
        outcome.orphaned,
        "a session in another generation belongs to no local binding"
    );
    assert!(outcome.agent_run_id.is_none(), "and is attached to nothing");
    assert!(!outcome.reduced);

    // The binding still names generation 1, and the run is untouched.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let generation: i64 = connection
        .query_row(
            "SELECT generation FROM runtime_bindings WHERE agent_run_id = ?1",
            rusqlite::params![RUN_A],
            |row| row.get(0),
        )
        .expect("the binding is readable");
    assert_eq!(generation, 1, "a binding is never silently re-pointed");
    assert_eq!(run_state(&fixture, RUN_A), before);

    // Finishing the generation-2 census concludes nothing about generation 1:
    // it censused a different generation, so it saw nothing about those runs.
    let summary = fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, true, now())
        .expect("the census completes");
    assert_eq!(summary.orphaned, 1);
    assert_eq!(summary.lost_contact, 0);
    assert_eq!(run_state(&fixture, RUN_A), before);
    assert_eq!(run_state(&fixture, RUN_B), run_state(&fixture, RUN_B));

    let fixture = fixture.restart();
    assert_eq!(run_state(&fixture, RUN_A), before);
}

// ---------------------------------------------------------------------------
// Absence
// ---------------------------------------------------------------------------

#[test]
fn missing_bound_session_becomes_lost_contact_not_completed() {
    let fixture = fixture();
    // Both sessions are seen once, so both runs are confirmed to begin with.
    sweep(
        &fixture,
        "sweep-1",
        &[
            census_item("session-1", 2, ObservedRunState::Running),
            census_item("session-2", 2, ObservedRunState::Running),
        ],
        true,
    );
    assert_eq!(
        run_state(&fixture, RUN_B).derived,
        DerivedRunState::Confirmed
    );
    let before_b = run_state(&fixture, RUN_B);

    // The next census finds only one of them.
    sweep(
        &fixture,
        "sweep-2",
        &[census_item("session-1", 3, ObservedRunState::Running)],
        true,
    );

    let after = run_state(&fixture, RUN_B);
    assert_eq!(
        after.derived,
        DerivedRunState::LostContact,
        "a missing session costs its run contact"
    );
    assert!(!after.derived.is_terminal(), "and nothing else");
    assert_eq!(after.lifecycle, before_b.lifecycle);
    assert_eq!(after.observed, before_b.observed);
    assert_eq!(after.desired, before_b.desired);
    assert!(!after.terminal);
    assert!(!after.closed);
    assert_eq!(
        run_state(&fixture, RUN_A).derived,
        DerivedRunState::Confirmed,
        "the session that was found is unaffected"
    );

    // It survives a restart as uncertainty, never resolving itself into a
    // verdict while nobody is looking.
    let fixture = fixture.restart();
    assert_eq!(
        run_state(&fixture, RUN_B).derived,
        DerivedRunState::LostContact
    );
    assert!(!run_state(&fixture, RUN_B).terminal);
}

#[test]
fn absence_never_sets_terminal_outcome() {
    let fixture = fixture();
    for key in ["sweep-1", "sweep-2", "sweep-3"] {
        sweep(&fixture, key, &[], true);
    }

    // Three completed censuses that found nothing at all. Both runs are out of
    // contact, and neither has an outcome, a closure time or a terminal
    // lifecycle.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let terminal: i64 = connection
        .query_row(
            "SELECT count(*) FROM agent_runs
             WHERE terminal_outcome IS NOT NULL OR closed_at IS NOT NULL
                OR terminal_source_kind IS NOT NULL OR terminal_evidence_hash IS NOT NULL
                OR lifecycle IN ('succeeded', 'failed', 'cancelled', 'parked')
                OR derived_state = 'terminal'",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(terminal, 0, "absence must never produce a terminal run");

    for run in [RUN_A, RUN_B] {
        let state = run_state(&fixture, run);
        assert_eq!(state.derived, DerivedRunState::LostContact);
        assert_eq!(state.lifecycle, RunLifecycle::Running);
        assert!(!state.terminal && !state.closed);
    }

    // Nothing in the results ledger claims an outcome either.
    let mut statement = connection
        .prepare("SELECT DISTINCT outcome FROM runtime_reconciliation_results")
        .expect("readable");
    let outcomes: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("readable")
        .map(|value| value.expect("an outcome"))
        .collect();
    for outcome in &outcomes {
        assert!(
            ![
                "succeeded",
                "failed",
                "cancelled",
                "parked",
                "abandoned",
                "terminal"
            ]
            .contains(&outcome.as_str()),
            "a census concluded `{outcome}`, which is a verdict it cannot have"
        );
    }
}

#[test]
fn failed_census_does_not_mark_missing() {
    let fixture = fixture();
    sweep(
        &fixture,
        "sweep-1",
        &[
            census_item("session-1", 2, ObservedRunState::Running),
            census_item("session-2", 2, ObservedRunState::Running),
        ],
        true,
    );
    let before = (run_state(&fixture, RUN_A), run_state(&fixture, RUN_B));

    // A sweep that did not finish saw neither session — and proves nothing
    // about either.
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-partial", 1), now())
        .expect("the epoch begins");
    let summary = fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, false, now())
        .expect("the census is abandoned");
    assert_eq!(summary.status, EpochStatus::Failed);
    assert_eq!(summary.lost_contact, 0);
    assert!(
        summary.completion_cursor.is_none(),
        "a failed census has no authoritative completion position"
    );
    assert_eq!(
        (run_state(&fixture, RUN_A), run_state(&fixture, RUN_B)),
        before,
        "a failed census changes nothing"
    );

    // And it cannot be promoted to authoritative afterwards.
    let retried = fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, true, now())
        .expect("finishing again is a read");
    assert_eq!(retried.status, EpochStatus::Failed);
    assert_eq!(
        (run_state(&fixture, RUN_A), run_state(&fixture, RUN_B)),
        before
    );
}

#[test]
fn partial_epoch_cannot_apply_absence() {
    let fixture = fixture();
    sweep(
        &fixture,
        "sweep-1",
        &[
            census_item("session-1", 2, ObservedRunState::Running),
            census_item("session-2", 2, ObservedRunState::Running),
        ],
        true,
    );
    let before = (run_state(&fixture, RUN_A), run_state(&fixture, RUN_B));
    let settled = rows(&fixture);

    // A census that reached one session and then gave up. The session it did not
    // reach is not absent — it is simply unknown, and unknown changes nothing.
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-partial", 1), now())
        .expect("the epoch begins");
    fixture
        .store
        .observe_census_item(
            epoch.epoch_id,
            fixture.project,
            &census_item("session-1", 3, ObservedRunState::Running),
        )
        .expect("the one item it reached is recorded");
    let summary = fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, false, now())
        .expect("the census is abandoned");

    assert_eq!(summary.status, EpochStatus::Failed);
    assert_eq!(summary.lost_contact, 0);
    assert_eq!(
        run_state(&fixture, RUN_B).derived,
        before.1.derived,
        "the unreached session keeps whatever was last known about it"
    );
    assert_eq!(
        rows(&fixture)["runtime_reconciliation_results"],
        settled["runtime_reconciliation_results"],
        "a failed census records no conclusions"
    );

    // An in-progress census applies nothing either: only finishing does.
    let running = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-open", 1), now())
        .expect("the epoch begins");
    assert_eq!(running.status, EpochStatus::InProgress);
    assert_eq!(run_state(&fixture, RUN_B).derived, before.1.derived);
}

// ---------------------------------------------------------------------------
// Idempotence
// ---------------------------------------------------------------------------

#[test]
fn repeating_same_epoch_duplicates_no_facts_or_effects() {
    let fixture = fixture();
    let items = [
        census_item("session-1", 2, ObservedRunState::Running),
        census_item("session-2", 2, ObservedRunState::Running),
    ];
    let epoch = sweep(&fixture, "sweep-1", &items, true);
    let settled = rows(&fixture);
    let before = (run_state(&fixture, RUN_A), run_state(&fixture, RUN_B));

    // Re-running the same census across a restart: same key, same items, same
    // finish. Every step finds its own fact already recorded.
    let fixture = fixture.restart();
    let reopened = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch reopens");
    assert_eq!(reopened.epoch_id, epoch);
    for item in &items {
        let outcome = fixture
            .store
            .observe_census_item(epoch, fixture.project, item)
            .expect("a repeated census item is not an error");
        assert!(!outcome.reduced, "a repeated item reduces nothing");
    }
    fixture
        .store
        .finish_reconciliation_epoch(epoch, fixture.project, true, now())
        .expect("finishing again is a read");

    assert_eq!(rows(&fixture), settled, "no fact is recorded twice");
    assert_eq!(
        (run_state(&fixture, RUN_A), run_state(&fixture, RUN_B)),
        before,
        "and no effect is applied twice"
    );

    // A session the finished census never saw cannot be slipped in afterwards.
    assert!(
        fixture
            .store
            .observe_census_item(
                epoch,
                fixture.project,
                &census_item("session-late", 9, ObservedRunState::Running)
            )
            .is_err(),
        "a closed census admits nothing new"
    );
    assert_eq!(rows(&fixture), settled);
}

#[test]
fn repeating_completed_epoch_does_not_increment_revision() {
    let fixture = fixture();
    let epoch = sweep(
        &fixture,
        "sweep-1",
        &[census_item("session-1", 2, ObservedRunState::Running)],
        true,
    );
    // `session-2` was missing, so its run lost contact exactly once.
    let after_first = run_state(&fixture, RUN_B);
    assert_eq!(after_first.derived, DerivedRunState::LostContact);

    for _ in 0..3 {
        let summary = fixture
            .store
            .finish_reconciliation_epoch(epoch, fixture.project, true, now())
            .expect("finishing again is a read");
        assert_eq!(summary.status, EpochStatus::Completed);
        assert_eq!(
            run_state(&fixture, RUN_B),
            after_first,
            "a settled census applies its conclusion once, not once per call"
        );
    }

    // A *different* census that also finds it missing records its own result
    // row, but the run is already out of contact so nothing about it changes.
    sweep(&fixture, "sweep-2", &[], true);
    assert_eq!(run_state(&fixture, RUN_B).revision, after_first.revision);
    assert_eq!(
        run_state(&fixture, RUN_B).derived,
        DerivedRunState::LostContact
    );
}

#[test]
fn later_positive_epoch_can_restore_freshness() {
    let fixture = fixture();
    sweep(&fixture, "sweep-1", &[], true);
    assert_eq!(
        run_state(&fixture, RUN_A).derived,
        DerivedRunState::LostContact
    );

    // The session answers again. Positive evidence lifts the uncertainty that
    // absence created — the reverse direction absence itself may never take.
    let fixture = fixture.restart();
    sweep(
        &fixture,
        "sweep-2",
        &[census_item("session-1", 4, ObservedRunState::Running)],
        true,
    );
    let restored = run_state(&fixture, RUN_A);
    assert_eq!(restored.derived, DerivedRunState::Confirmed);
    assert_eq!(restored.observed, ObservedRunState::Running);
    assert!(!restored.terminal);
    assert_eq!(restored.lifecycle, RunLifecycle::Running);
}

#[test]
fn reconciliation_preserves_distinct_desired_observed_derived_and_lifecycle() {
    let fixture = fixture();

    // Ask for a cancel, then let the census report the session still running:
    // four dimensions, four different answers, none collapsed into another.
    Connection::open(&fixture.path)
        .expect("a raw connection opens")
        .execute(
            "UPDATE agent_runs SET desired_state = 'cancel_requested', revision = revision + 1
             WHERE id = ?1",
            rusqlite::params![RUN_A],
        )
        .expect("the desired state moves");

    sweep(
        &fixture,
        "sweep-1",
        &[census_item("session-1", 2, ObservedRunState::Running)],
        true,
    );

    let state = run_state(&fixture, RUN_A);
    assert_eq!(state.desired, DesiredRunState::CancelRequested);
    assert_eq!(state.observed, ObservedRunState::Running);
    assert_eq!(state.derived, DerivedRunState::Diverged);
    assert_eq!(state.lifecycle, RunLifecycle::Running);
    assert!(!state.terminal);

    // The same four values come back off disk.
    let fixture = fixture.restart();
    assert_eq!(run_state(&fixture, RUN_A), state);

    // And the run that was missing is out of contact without any of its other
    // dimensions moving.
    let missing = run_state(&fixture, RUN_B);
    assert_eq!(missing.derived, DerivedRunState::LostContact);
    assert_eq!(missing.desired, DesiredRunState::RunRequested);
    assert_eq!(missing.observed, ObservedRunState::Unknown);
    assert_eq!(missing.lifecycle, RunLifecycle::Running);
}

// ---------------------------------------------------------------------------
// The whole scripted sequence
// ---------------------------------------------------------------------------

#[test]
fn the_scripted_census_sequence_reaches_its_recorded_conclusions() {
    let fixture = fixture();
    let script: serde_json::Value = serde_json::from_str(EPOCHS).expect("the epoch script parses");
    let epochs = script["epochs"]
        .as_array()
        .expect("the script lists epochs");

    for entry in epochs {
        let name = entry["name"].as_str().expect("a name");
        let items: Vec<CensusItem> = entry["items"]
            .as_array()
            .expect("a census lists items")
            .iter()
            .map(|item| {
                census_item(
                    item["native_id"].as_str().expect("a native id"),
                    item["native_sequence"].as_u64().expect("a sequence"),
                    ObservedRunState::parse(item["observed"].as_str().expect("a state"))
                        .expect("a known observed state"),
                )
            })
            .collect();
        let authoritative = entry["authoritative"].as_bool().expect("a verdict");

        let epoch = fixture
            .store
            .begin_reconciliation_epoch(
                &epoch_key(
                    &fixture,
                    entry["reconciliation_key"].as_str().expect("a key"),
                    1,
                ),
                now(),
            )
            .unwrap_or_else(|error| panic!("`{name}` begins: {error}"));
        for item in &items {
            fixture
                .store
                .observe_census_item(epoch.epoch_id, fixture.project, item)
                .unwrap_or_else(|error| panic!("`{name}` records its item: {error}"));
        }
        let summary = fixture
            .store
            .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, authoritative, now())
            .unwrap_or_else(|error| panic!("`{name}` finishes: {error}"));

        let expect = &entry["expect"];
        assert_eq!(
            u64::from(summary.present),
            expect["present"].as_u64().expect("a count"),
            "`{name}` found the wrong number of bound sessions"
        );
        assert_eq!(
            u64::from(summary.lost_contact),
            expect["lost_contact"].as_u64().expect("a count"),
            "`{name}` lost contact with the wrong number of runs"
        );
        assert_eq!(
            u64::from(summary.orphaned),
            expect["orphaned"].as_u64().expect("a count"),
            "`{name}` recorded the wrong number of orphans"
        );

        // Whatever the census concluded, no run is ever closed by it.
        for run in [RUN_A, RUN_B] {
            let state = run_state(&fixture, run);
            assert!(!state.terminal, "`{name}` closed {run}");
            assert!(!state.closed, "`{name}` closed {run}");
            assert!(!state.lifecycle.is_terminal(), "`{name}` closed {run}");
            assert!(!state.derived.is_terminal(), "`{name}` closed {run}");
        }
    }

    // The last census found both sessions again, so both runs are confirmed.
    for run in [RUN_A, RUN_B] {
        assert_eq!(run_state(&fixture, run).derived, DerivedRunState::Confirmed);
    }

    // An orphan was recorded as evidence and attached to nothing.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let orphans: i64 = connection
        .query_row(
            "SELECT count(*) FROM runtime_reconciliation_members WHERE agent_run_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(orphans, 1);
    let attached: i64 = connection
        .query_row(
            "SELECT count(*) FROM runtime_bindings WHERE native_id = 'session-unknown'",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(attached, 0, "an orphan is never silently bound to a run");
}

#[test]
fn an_unbound_native_session_is_recorded_but_never_attached() {
    let fixture = fixture();
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch begins");
    let outcome = fixture
        .store
        .observe_census_item(
            epoch.epoch_id,
            fixture.project,
            &census_item("session-nobody-launched", 2, ObservedRunState::Running),
        )
        .expect("the orphan is recorded");
    assert!(outcome.orphaned);
    assert!(outcome.agent_run_id.is_none());
    assert!(!outcome.reduced);

    // Exactly one event: the orphan's own raw-plus-normalized observation, and no
    // second, spurious one anywhere.
    let counts = rows(&fixture);
    assert_eq!(counts["runtime_bindings"], 2, "no binding was invented");
    assert_eq!(counts["runtime_events"], 1);
    assert_eq!(counts["runtime_reconciliation_members"], 1);

    // The evidence exists, and it carries both halves — the immutable raw payload
    // *and* every normalized field a reduction reads — exactly like a bound
    // observation. It simply names no run, because there is none to name.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let (kind, run, native, sequence, observed, contact, freshness, audit, hash): (
        String,
        Option<String>,
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
    ) = connection
        .query_row(
            "SELECT event_kind, agent_run_id, native_id, native_sequence, observed_state, contact,
                    freshness, audit_ref, payload_hash
             FROM runtime_events WHERE cursor = ?1",
            rusqlite::params![outcome.observation_cursor.get()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .expect("an orphan's observation is persisted like any other");
    assert_eq!(kind, "census_observation");
    assert_eq!(run, None, "an orphan's evidence names no run");
    assert_eq!(native, "session-nobody-launched");
    assert_eq!(sequence, 2);
    assert_eq!(observed, "running");
    assert_eq!(contact, "reachable");
    assert_eq!(freshness, "fresh");
    assert_eq!(audit, "audit-session-nobody-launched-2");
    assert_eq!(
        hash,
        document("session-nobody-launched-2").hash().as_str(),
        "the raw payload is stored under its own digest"
    );

    // And the consequence cites it. The observation cannot have been written
    // *after* the membership row, because the foreign key refuses a membership row
    // naming a cursor that does not exist yet — the ordering is enforced by the
    // schema, not by hope.
    let cited: i64 = connection
        .query_row(
            "SELECT observation_cursor FROM runtime_reconciliation_members
             WHERE native_id = 'session-nobody-launched'",
            [],
            |row| row.get(0),
        )
        .expect("the membership row cites its evidence");
    assert_eq!(cited, outcome.observation_cursor.get());
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_reconciliation_members
                     (project_id, epoch_id, native_id, observation_cursor, observed_state,
                      recorded_at)
                 VALUES (?1, ?2, 'session-from-nowhere', 9999, 'running',
                         '2026-08-09T10:00:00Z')",
                rusqlite::params![PROJECT, epoch.epoch_id.to_string()],
            )
            .is_err(),
        "no census fact may cite an observation that was never persisted"
    );

    // A duplicate census member records no second fact, and no second event.
    fixture
        .store
        .observe_census_item(
            epoch.epoch_id,
            fixture.project,
            &census_item("session-nobody-launched", 3, ObservedRunState::Blocked),
        )
        .expect("a duplicate member is not an error");
    let after = rows(&fixture);
    assert_eq!(after["runtime_reconciliation_members"], 1);
    assert_eq!(after["runtime_events"], 1, "and no second observation");
}

#[test]
fn a_census_observation_is_evidence_before_it_is_a_conclusion() {
    let fixture = fixture();
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch begins");
    let outcome = fixture
        .store
        .observe_census_item(
            epoch.epoch_id,
            fixture.project,
            &census_item("session-1", 2, ObservedRunState::Running),
        )
        .expect("the census item is recorded");
    let cursor = outcome.observation_cursor;

    // The row carries the raw payload *and* the normalized fields, exactly like
    // any other control-plane observation.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let (observed, contact, freshness, audit): (String, String, String, String) = connection
        .query_row(
            "SELECT observed_state, contact, freshness, audit_ref FROM runtime_events
             WHERE cursor = ?1",
            rusqlite::params![cursor.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the evidence row exists");
    assert_eq!(observed, "running");
    assert_eq!(contact, "reachable");
    assert_eq!(freshness, "fresh");
    assert_eq!(audit, "audit-session-1-2");

    // A census carrying session content is refused before SQL sees it.
    assert!(
        fixture
            .store
            .observe_census_item(
                epoch.epoch_id,
                fixture.project,
                &CensusItem {
                    raw: CanonicalDocument::from_value(&serde_json::json!({
                        "schema_version": 1,
                        "transcript": "hello"
                    }))
                    .expect("a canonical document"),
                    ..census_item("session-2", 2, ObservedRunState::Running)
                },
            )
            .is_err(),
        "a census does not smuggle transcripts into the control-plane log"
    );
}

#[test]
fn an_observation_from_a_control_path_and_a_census_share_one_cursor_space() {
    let fixture = fixture();
    let run = AgentRunId::parse(RUN_A).expect("a canonical id");
    let revision = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists")
        .revision;
    let streamed = fixture
        .store
        .append_control_observation(&ControlObservation {
            project_id: fixture.project,
            agent_run_id: run,
            identity: identity("session-1", 1),
            native_event_id: Some(external("stream-2")),
            native_sequence: 2,
            expected_sequence: Some(2),
            observed: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            raw: document("stream-2"),
            audit_ref: external("audit-stream-2"),
            observed_at: now(),
            expected_revision: revision,
        })
        .expect("the streamed observation is recorded");

    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-1", 1), now())
        .expect("the epoch begins");
    assert!(
        epoch.census_start_cursor >= streamed.cursor,
        "a census starts from the position the log is actually at"
    );
    let censused = fixture
        .store
        .observe_census_item(
            epoch.epoch_id,
            fixture.project,
            &census_item("session-1", 3, ObservedRunState::WaitingInput),
        )
        .expect("the census item is recorded");
    let cursor = censused.observation_cursor;
    assert!(
        cursor > streamed.cursor,
        "one control-plane cursor space, allocated in order"
    );
    assert!(censused.reduced, "a newer census observation still reduces");
    assert_eq!(
        run_state(&fixture, RUN_A).observed,
        ObservedRunState::WaitingInput
    );
}

#[test]
fn a_census_admits_only_the_scope_it_censuses() {
    let fixture = fixture();
    let epoch = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-gen-1", 1), now())
        .expect("the epoch begins");
    let before_rows = rows(&fixture);
    let before_a = run_state(&fixture, RUN_A);

    // A native id is only unique inside `(runtime_kind, host, generation)`, and
    // every fact this census records downstream — the membership row, the presence
    // check at completion, the absence rule — is keyed by the id alone. So an item
    // from another scope carrying a colliding id is refused outright: admitting
    // one would mark a bound session present that this generation never reported.
    let foreign = [
        CensusItem {
            identity: identity("session-1", 2),
            ..census_item("session-1", 2, ObservedRunState::Running)
        },
        CensusItem {
            identity: NativeRuntimeIdentity {
                host: ExternalName::parse("host-2").expect("a valid host"),
                ..identity("session-1", 1)
            },
            ..census_item("session-1", 3, ObservedRunState::Running)
        },
        CensusItem {
            identity: NativeRuntimeIdentity {
                runtime_kind: RuntimeKindKey::parse("other.runtime").expect("a valid runtime key"),
                ..identity("session-1", 1)
            },
            ..census_item("session-1", 4, ObservedRunState::Running)
        },
    ];
    for item in &foreign {
        assert!(
            fixture
                .store
                .observe_census_item(epoch.epoch_id, fixture.project, item)
                .is_err(),
            "a session outside this epoch's scope is not this census's business: {:?}",
            item.identity
        );
    }
    assert_eq!(
        rows(&fixture),
        before_rows,
        "a refused item appends no evidence, no membership row and no result"
    );
    assert_eq!(
        run_state(&fixture, RUN_A),
        before_a,
        "and it reduces nothing"
    );

    // Which means the completed census still tells the truth about the session it
    // actually did not find: present is earned in scope, or not at all.
    let summary = fixture
        .store
        .finish_reconciliation_epoch(epoch.epoch_id, fixture.project, true, now())
        .expect("the census finishes");
    assert_eq!(summary.present, 0, "no foreign item bought a presence");
    assert_eq!(
        summary.lost_contact, 2,
        "both bound sessions are genuinely missing from this generation"
    );
    assert_eq!(
        run_state(&fixture, RUN_A).derived,
        DerivedRunState::LostContact
    );
    assert!(
        !run_state(&fixture, RUN_A).terminal && !run_state(&fixture, RUN_A).closed,
        "absence is still not an outcome"
    );

    // The in-scope session is admitted by the same call that refused the others.
    let in_scope = fixture
        .store
        .begin_reconciliation_epoch(&epoch_key(&fixture, "sweep-gen-1-again", 1), now())
        .expect("a second epoch begins");
    assert!(
        fixture
            .store
            .observe_census_item(
                in_scope.epoch_id,
                fixture.project,
                &census_item("session-1", 5, ObservedRunState::Running),
            )
            .expect("the in-scope item is recorded")
            .agent_run_id
            .is_some(),
        "the epoch's own generation still resolves to its binding"
    );
}
