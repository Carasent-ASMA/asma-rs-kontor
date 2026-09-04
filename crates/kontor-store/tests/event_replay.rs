//! The control-plane log: evidence before consequence, deduplication, the two
//! gap models, and replay.
//!
//! The mutants this suite exists to kill:
//!
//! * reducing a projection without inserting the evidence it came from, or
//!   inserting the evidence after the effect;
//! * treating a duplicate, an older sequence or an equal sequence as progress;
//! * accepting two different observations for one native sequence;
//! * letting a reader synthesize a cursor, or a consumer skip or repeat one;
//! * turning a missing control fact — or a hole in a transcript — into a
//!   lifecycle change, an outcome or a terminal run;
//! * persisting transcript, message or token data in the durable log.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{
    AgentRunId, AggregateRevision, CanonicalDocument, EventCursor, ExternalId, ExternalName,
    ProjectId, RuntimeKindKey, Timestamp, parse_utc_timestamp,
};
use kontor_core::repository::{NewObservation, NewRuntimeEvent, RunRepository};
use kontor_core::state::{
    DerivedRunState, Freshness, NativeRuntimeIdentity, ObservedRunState, RunLifecycle,
    RuntimeContact,
};
use kontor_store::{ContentDiscontinuity, ContentGapOutcome, ControlObservation, SqliteStore};
use rusqlite::Connection;
use tempfile::TempDir;

/// One arrival order, with what each observation is allowed to do.
const OBSERVATIONS: &str = include_str!("fixtures/runtime/control_observations.json");

const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
const RUN: &str = "0193f000-0000-7000-8000-000000000040";
const RUN_B: &str = "0193f000-0000-7000-8000-000000000041";

/// Two bound runs in one team, so a concurrency test has two independent
/// appenders and a paging test has more than one stream.
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
        '2026-08-09T10:00:00Z'); \
INSERT INTO projects (id, name, root_path, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-0000000000b1', 'B', '/tmp/b', 1, '2026-08-09T10:00:00Z'); \
INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at) \
VALUES ('0193f000-0000-7000-8000-0000000000b2', '0193f000-0000-7000-8000-0000000000b1', \
        'TB', 'in_progress', 1, '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z'); \
INSERT INTO team_templates (project_id, template_id, version, name, definition, \
        definition_hash, role_authority, created_at) \
VALUES ('0193f000-0000-7000-8000-0000000000b1', '0193f000-0000-7000-8000-0000000000b3', 1, \
        'TeamB', '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '[]', \
        '2026-08-09T10:00:00Z'); \
INSERT INTO team_runs (id, project_id, task_id, template_id, template_version, snapshot, \
        snapshot_hash, lifecycle, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-0000000000b4', '0193f000-0000-7000-8000-0000000000b1', \
        '0193f000-0000-7000-8000-0000000000b2', '0193f000-0000-7000-8000-0000000000b3', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'running', 1, \
        '2026-08-09T10:00:00Z'); \
INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle, desired_state, \
        observed_state, derived_state, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-0000000000b5', '0193f000-0000-7000-8000-0000000000b1', \
        '0193f000-0000-7000-8000-0000000000b4', 'maker.primary', 'running', 'run_requested', \
        'unknown', 'pending_confirmation', 1, '2026-08-09T10:00:00Z'); \
INSERT INTO runtime_bindings (id, project_id, agent_run_id, runtime_kind, host, generation, \
        native_id, bound_at) \
VALUES ('0193f000-0000-7000-8000-0000000000b6', '0193f000-0000-7000-8000-0000000000b1', \
        '0193f000-0000-7000-8000-0000000000b5', 'generic.runtime', 'host-2', 1, 'session-b', \
        '2026-08-09T10:00:00Z');";

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
    run: AgentRunId,
    binding: kontor_core::id::RuntimeBindingId,
}

impl Fixture {
    fn restart(self) -> Self {
        let Self {
            _directory,
            path,
            store,
            project,
            run,
            binding,
        } = self;
        drop(store);
        let store = SqliteStore::open(&path).expect("the store reopens");
        Self {
            _directory,
            path,
            store,
            project,
            run,
            binding,
        }
    }

    fn revision(&self, run: AgentRunId) -> AggregateRevision {
        self.store
            .get_agent_run(self.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .revision
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
        run: AgentRunId::parse(RUN).expect("a canonical id"),
        binding: kontor_core::id::RuntimeBindingId::parse("0193f000-0000-7000-8000-000000000050")
            .expect("a canonical id"),
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

fn identity(native: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("generic.runtime").expect("a valid runtime key"),
        host: ExternalName::parse("host-1").expect("a valid host"),
        generation: 1,
        native_id: external(native),
    }
}

fn observation(fixture: &Fixture, sequence: u64, marker: &str) -> ControlObservation {
    ControlObservation {
        project_id: fixture.project,
        agent_run_id: fixture.run,
        identity: identity("session-1"),
        native_event_id: Some(external(marker)),
        native_sequence: sequence,
        expected_sequence: Some(sequence),
        observed: ObservedRunState::Running,
        contact: RuntimeContact::Reachable,
        freshness: Freshness::Fresh,
        raw: document(marker),
        audit_ref: external(&format!("audit-{marker}")),
        observed_at: now(),
        expected_revision: fixture.revision(fixture.run),
    }
}

fn census(fixture: &Fixture) -> BTreeMap<&'static str, i64> {
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    [
        "runtime_events",
        "runtime_control_gaps",
        "runtime_content_gaps",
        "runtime_bindings",
        "command_receipts",
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

/// The full projection of a run, for before/after comparison.
fn projection(
    fixture: &Fixture,
    run: AgentRunId,
) -> (
    RunLifecycle,
    ObservedRunState,
    DerivedRunState,
    AggregateRevision,
    Option<EventCursor>,
) {
    let stored = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists");
    (
        stored.projection.lifecycle,
        stored.projection.observed,
        stored.projection.derived,
        stored.revision,
        stored.projection.last_cursor,
    )
}

// ---------------------------------------------------------------------------
// Evidence before consequence
// ---------------------------------------------------------------------------

#[test]
fn raw_and_normalized_observation_exist_before_projection_effect() {
    let fixture = fixture();
    let outcome = fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the observation is recorded");
    assert!(outcome.appended && outcome.reduced);

    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let (payload_hash, observed, contact, freshness, audit, sequence): (
        String,
        String,
        String,
        String,
        String,
        i64,
    ) = connection
        .query_row(
            "SELECT payload_hash, observed_state, contact, freshness, audit_ref, native_sequence
             FROM runtime_events WHERE cursor = ?1",
            rusqlite::params![outcome.cursor.get()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("the evidence row exists");

    // Both halves are in the same row: the immutable raw payload digest, and
    // every normalized field the reduction actually read.
    assert_eq!(payload_hash, document("n-2").hash().as_str());
    assert_eq!(observed, "running");
    assert_eq!(contact, "reachable");
    assert_eq!(freshness, "fresh");
    assert_eq!(audit, "audit-n-2");
    assert_eq!(sequence, 2);
    assert_eq!(outcome.projection.observed, ObservedRunState::Running);
    assert_eq!(outcome.projection.derived, DerivedRunState::Confirmed);
}

#[test]
fn projection_effect_always_cites_persisted_event() {
    let fixture = fixture();
    for (sequence, marker) in [(2, "n-2"), (3, "n-3"), (4, "n-4")] {
        fixture
            .store
            .append_control_observation(&observation(&fixture, sequence, marker))
            .expect("the observation is recorded");
    }

    let fixture = fixture.restart();
    let (_, observed, _, _, last_cursor) = projection(&fixture, fixture.run);
    let cursor = last_cursor.expect("a reduced run cites a cursor");

    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let (stored_run, stored_observed, contact): (String, String, String) = connection
        .query_row(
            "SELECT agent_run_id, observed_state, contact FROM runtime_events WHERE cursor = ?1",
            rusqlite::params![cursor.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the cited event is a persisted row");
    assert_eq!(stored_run, RUN, "the cited event belongs to this run");
    assert_eq!(
        stored_observed,
        observed.as_str(),
        "the projection says exactly what its evidence says"
    );
    assert_eq!(contact, "reachable");

    // And the cursor a run cites is always one that exists: a projection can
    // never point past the log.
    let beyond: i64 = connection
        .query_row(
            "SELECT count(*) FROM agent_runs
             WHERE last_cursor IS NOT NULL
               AND last_cursor NOT IN (SELECT cursor FROM runtime_events)",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(beyond, 0, "no projection may cite an absent event");
}

#[test]
fn normalization_or_cas_failure_leaves_no_event_or_effect() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the first observation is recorded");
    let settled = census(&fixture);
    let before = projection(&fixture, fixture.run);

    // A conflicting observation for a sequence already stored: the row is
    // attempted and the whole transaction is thrown away.
    let conflicting = ControlObservation {
        native_event_id: Some(external("n-2-conflicting")),
        raw: document("a-different-story"),
        ..observation(&fixture, 2, "n-2")
    };
    assert!(
        fixture
            .store
            .append_control_observation(&conflicting)
            .is_err()
    );

    // A stale revision, a run that does not exist here, and a payload carrying
    // session content are all refused the same way.
    let stale = ControlObservation {
        expected_revision: AggregateRevision::parse(99).expect("a valid revision"),
        ..observation(&fixture, 3, "n-3")
    };
    assert!(fixture.store.append_control_observation(&stale).is_err());
    let foreign = ControlObservation {
        agent_run_id: AgentRunId::generate(),
        ..observation(&fixture, 4, "n-4")
    };
    assert!(fixture.store.append_control_observation(&foreign).is_err());

    assert_eq!(
        census(&fixture),
        settled,
        "a refused observation leaves no event behind"
    );
    assert_eq!(
        projection(&fixture, fixture.run),
        before,
        "and no consequence either"
    );

    let fixture = fixture.restart();
    assert_eq!(census(&fixture), settled);
    assert_eq!(projection(&fixture, fixture.run), before);
}

// ---------------------------------------------------------------------------
// Deduplication and ordering
// ---------------------------------------------------------------------------

#[test]
fn duplicate_native_id_returns_original_cursor() {
    let fixture = fixture();
    let first = fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the observation is recorded");
    let settled = census(&fixture);
    let after_first = projection(&fixture, fixture.run);

    let replay = fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("a replay is not an error");
    assert_eq!(
        replay.cursor, first.cursor,
        "a duplicate maps to its original"
    );
    assert!(!replay.appended && !replay.reduced);
    assert_eq!(census(&fixture), settled);
    assert_eq!(projection(&fixture, fixture.run), after_first);
}

#[test]
fn same_sequence_conflicting_payload_is_rejected() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the observation is recorded");
    let settled = census(&fixture);
    let before = projection(&fixture, fixture.run);

    // Same session, same native sequence, different story. Neither version may
    // be trusted enough to reduce, so nothing is stored and nothing changes.
    for candidate in [
        ControlObservation {
            native_event_id: Some(external("n-2-other")),
            raw: document("other"),
            ..observation(&fixture, 2, "n-2")
        },
        ControlObservation {
            raw: document("other"),
            observed: ObservedRunState::Succeeded,
            ..observation(&fixture, 2, "n-2")
        },
    ] {
        assert!(
            fixture
                .store
                .append_control_observation(&candidate)
                .is_err(),
            "one native sequence carries one observation"
        );
    }
    assert_eq!(census(&fixture), settled);
    assert_eq!(projection(&fixture, fixture.run), before);
}

#[test]
fn identical_payloads_without_native_ids_stay_distinct_observations() {
    let fixture = fixture();

    // A runtime that gives its events no ids of their own, reporting the same
    // thing twice — "still running" at sequence 2, then again at sequence 3. The
    // payloads are byte-for-byte identical, and the observations are not: they are
    // two separate contacts with the runtime, and the second is evidence the first
    // cannot supply.
    let anonymous = |sequence: u64| ControlObservation {
        native_event_id: None,
        raw: document("still-running"),
        audit_ref: external(&format!("audit-{sequence}")),
        ..observation(&fixture, sequence, "unused")
    };

    let first = fixture
        .store
        .append_control_observation(&anonymous(2))
        .expect("the first observation is recorded");
    assert!(first.appended && first.reduced);

    let second = fixture
        .store
        .append_control_observation(&ControlObservation {
            expected_revision: fixture.revision(fixture.run),
            ..anonymous(3)
        })
        .expect("the second observation is recorded");
    assert!(
        second.appended,
        "a repeated payload is not a repeated observation"
    );
    assert!(
        second.cursor > first.cursor,
        "the second observation gets its own control-plane cursor"
    );
    assert_eq!(
        census(&fixture)["runtime_events"],
        2,
        "identity is the native sequence, never the payload digest"
    );

    // Identity is still identity: the *same* native sequence replayed maps back to
    // the row it already has, and a different story about that one moment is a
    // conflict rather than a second truth.
    let replay = fixture
        .store
        .append_control_observation(&ControlObservation {
            expected_revision: fixture.revision(fixture.run),
            ..anonymous(3)
        })
        .expect("a replay is not an error");
    assert_eq!(replay.cursor, second.cursor);
    assert!(!replay.appended && !replay.reduced);
    assert!(
        fixture
            .store
            .append_control_observation(&ControlObservation {
                raw: document("a-different-story"),
                expected_revision: fixture.revision(fixture.run),
                ..anonymous(3)
            })
            .is_err(),
        "one native sequence carries one observation, id or no id"
    );
    assert_eq!(census(&fixture)["runtime_events"], 2);
}

#[test]
fn older_distinct_observation_appends_but_does_not_reduce() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 5, "n-5"))
        .expect("the newer observation reduces");
    let after_newer = projection(&fixture, fixture.run);
    let events_before = census(&fixture)["runtime_events"];

    let older = ControlObservation {
        observed: ObservedRunState::WaitingInput,
        ..observation(&fixture, 3, "n-3")
    };
    let outcome = fixture
        .store
        .append_control_observation(&older)
        .expect("an older observation is not an error");
    assert!(outcome.appended, "genuinely new evidence is still kept");
    assert!(!outcome.reduced, "but it does not move the projection");
    assert_eq!(
        census(&fixture)["runtime_events"],
        events_before + 1,
        "the older observation is stored as audit evidence"
    );
    assert_eq!(
        projection(&fixture, fixture.run),
        after_newer,
        "observed, derived, revision and the reduced cursor all stand still"
    );
}

#[test]
fn duplicate_and_out_of_order_never_regress_effects() {
    let fixture = fixture();
    let script: serde_json::Value =
        serde_json::from_str(OBSERVATIONS).expect("the observation script parses");
    let steps = script["observations"]
        .as_array()
        .expect("the script lists observations");

    let mut highest_cursor = EventCursor::parse(1).expect("the origin");
    for step in steps {
        let name = step["name"].as_str().expect("a name");
        let expect = &step["expect"];
        let candidate = ControlObservation {
            native_event_id: Some(external(step["native_event_id"].as_str().expect("an id"))),
            native_sequence: step["native_sequence"].as_u64().expect("a sequence"),
            expected_sequence: step["expected_sequence"].as_u64(),
            observed: ObservedRunState::parse(step["observed"].as_str().expect("a state"))
                .expect("a known observed state"),
            contact: RuntimeContact::parse(step["contact"].as_str().expect("a contact"))
                .expect("a known contact"),
            freshness: Freshness::parse(step["freshness"].as_str().expect("a freshness"))
                .expect("a known freshness"),
            raw: document(step["marker"].as_str().expect("a marker")),
            ..observation(&fixture, 0, "unused")
        };
        let before = projection(&fixture, fixture.run);
        let outcome = fixture
            .store
            .append_control_observation(&candidate)
            .unwrap_or_else(|error| panic!("`{name}` is recorded: {error}"));

        assert_eq!(
            outcome.appended,
            expect["appended"].as_bool().expect("a verdict"),
            "`{name}` appended the wrong way"
        );
        assert_eq!(
            outcome.reduced,
            expect["reduced"].as_bool().expect("a verdict"),
            "`{name}` reduced the wrong way"
        );
        assert_eq!(
            outcome.control_gap.is_some(),
            expect["gap"].as_bool().expect("a verdict"),
            "`{name}` recorded the wrong gap verdict"
        );

        let (lifecycle, observed, derived, revision, cursor) = projection(&fixture, fixture.run);
        assert_eq!(
            observed.as_str(),
            expect["observed"].as_str().expect("an observed state"),
            "`{name}` left the wrong observed state"
        );
        assert_eq!(
            derived.as_str(),
            expect["derived"].as_str().expect("a derived state"),
            "`{name}` left the wrong conclusion"
        );
        assert_eq!(
            lifecycle,
            RunLifecycle::Running,
            "`{name}` must not touch the lifecycle dimension"
        );
        assert!(!derived.is_terminal(), "`{name}` must not close the run");

        if outcome.reduced {
            assert!(
                revision.get() > before.3.get(),
                "`{name}` must advance the revision"
            );
            let cursor = cursor.expect("a reduced run cites a cursor");
            assert!(
                cursor > highest_cursor,
                "`{name}` must move the reduced cursor forward"
            );
            highest_cursor = cursor;
        } else {
            assert_eq!(
                (revision, cursor),
                (before.3, before.4),
                "`{name}` must leave the revision and the reduced cursor alone"
            );
        }
    }

    // Every stored event is still there, and the run never closed.
    let distinct = steps
        .iter()
        .filter(|step| step["expect"]["appended"].as_bool() == Some(true))
        .count();
    let stored = fixture
        .store
        .read_runtime_events(fixture.project, fixture.run, None)
        .expect("the read succeeds");
    assert_eq!(
        stored.len(),
        distinct,
        "every distinct observation is retained, and no duplicate is stored twice"
    );
    let run = fixture
        .store
        .get_agent_run(fixture.project, fixture.run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(run.terminal.is_none());
    assert!(!run.projection.lifecycle.is_terminal());
}

#[test]
fn concurrent_appenders_allocate_unique_monotonic_control_cursors() {
    let fixture = fixture();
    let path = fixture.path.clone();
    let project = fixture.project;
    drop(fixture.store);

    // Two independent connections to one file, appending at the same time.
    let appenders: Vec<_> = [(RUN, "session-1"), (RUN_B, "session-2")]
        .into_iter()
        .map(|(run, native)| {
            let path = path.clone();
            std::thread::spawn(move || {
                let store = SqliteStore::open(&path).expect("the store opens");
                let run = AgentRunId::parse(run).expect("a canonical id");
                let mut cursors = Vec::new();
                for sequence in 2..12u64 {
                    let revision = store
                        .get_agent_run(project, run)
                        .expect("the read succeeds")
                        .expect("the run exists")
                        .revision;
                    let marker = format!("{native}-{sequence}");
                    let outcome = store
                        .append_control_observation(&ControlObservation {
                            project_id: project,
                            agent_run_id: run,
                            identity: identity(native),
                            native_event_id: Some(external(&marker)),
                            native_sequence: sequence,
                            expected_sequence: Some(sequence),
                            observed: ObservedRunState::Running,
                            contact: RuntimeContact::Reachable,
                            freshness: Freshness::Fresh,
                            raw: document(&marker),
                            audit_ref: external(&format!("audit-{marker}")),
                            observed_at: now(),
                            expected_revision: revision,
                        })
                        .expect("the observation is recorded");
                    cursors.push(outcome.cursor.get());
                }
                cursors
            })
        })
        .collect();

    let mut all = Vec::new();
    for appender in appenders {
        let cursors = appender.join().expect("the appender does not panic");
        assert!(
            cursors.windows(2).all(|pair| pair[0] < pair[1]),
            "one appender's own cursors are strictly increasing"
        );
        all.extend(cursors);
    }

    let distinct: BTreeSet<i64> = all.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        all.len(),
        "two appenders must never be handed the same control cursor"
    );
    assert_eq!(all.len(), 20);

    // Every cursor names a distinct committed row.
    let connection = Connection::open(&path).expect("a raw connection opens");
    let stored: i64 = connection
        .query_row(
            "SELECT count(*) FROM runtime_events WHERE contact IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(stored, 20);
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

#[test]
fn replay_after_n_returns_only_greater_cursors_in_order() {
    let fixture = fixture();
    let mut cursors = Vec::new();
    for (sequence, marker) in [(2, "n-2"), (3, "n-3"), (4, "n-4")] {
        cursors.push(
            fixture
                .store
                .append_control_observation(&observation(&fixture, sequence, marker))
                .expect("the observation is recorded")
                .cursor,
        );
    }

    for (index, cursor) in cursors.iter().enumerate() {
        let page = fixture
            .store
            .read_control_events_after(fixture.project, None, *cursor, 100)
            .expect("the read succeeds");
        assert!(
            page.iter().all(|event| event.cursor > *cursor),
            "replay is strictly after the cursor it was given"
        );
        assert!(
            page.windows(2).all(|pair| pair[0].cursor < pair[1].cursor),
            "replay is ascending"
        );
        assert_eq!(page.len(), cursors.len() - index - 1);
    }

    // The newest cursor returns nothing, and so does one beyond the end.
    let newest = *cursors.last().expect("a cursor");
    assert!(
        fixture
            .store
            .read_control_events_after(fixture.project, None, newest, 100)
            .expect("the read succeeds")
            .is_empty()
    );
    let beyond = EventCursor::parse(newest.get() + 1_000).expect("a cursor");
    assert!(
        fixture
            .store
            .read_control_events_after(fixture.project, None, beyond, 100)
            .expect("the read succeeds")
            .is_empty(),
        "a cursor past the end is empty, not wrapped"
    );

    // A page size of zero can never make progress, so it is refused rather than
    // quietly looking like a caught-up consumer.
    assert!(
        fixture
            .store
            .read_control_events_after(fixture.project, None, newest, 0)
            .is_err()
    );
}

#[test]
fn empty_origin_snapshot_does_not_skip_first_event() {
    let fixture = fixture();
    let consumer = external("projector");

    // Against an empty ledger the consumer has no checkpoint at all.
    let empty = fixture
        .store
        .page_consumer(fixture.project, &consumer, 10, now())
        .expect("the page reads");
    assert!(empty.events.is_empty());
    assert_eq!(
        empty.last_cursor,
        EventCursor::parse(1).expect("the origin"),
        "an empty page leaves the checkpoint at the reserved origin"
    );
    assert!(
        fixture
            .store
            .consumer_cursor(fixture.project, &consumer)
            .expect("the read succeeds")
            .is_none(),
        "an empty page persists no checkpoint to skip past"
    );

    // The very first event must then be delivered, not skipped over.
    let first = fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the observation is recorded");
    let page = fixture
        .store
        .page_consumer(fixture.project, &consumer, 10, now())
        .expect("the page reads");
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].cursor, first.cursor);
    assert!(
        first.cursor.get() > 1,
        "the reserved origin never names a row"
    );
}

#[test]
fn persisted_consumer_pages_without_gap_or_overlap() {
    let fixture = fixture();
    let consumer = external("projector");
    let mut appended = Vec::new();
    for sequence in 2..8u64 {
        let marker = format!("n-{sequence}");
        appended.push(
            fixture
                .store
                .append_control_observation(&observation(&fixture, sequence, &marker))
                .expect("the observation is recorded")
                .cursor,
        );
    }

    let mut delivered = Vec::new();
    loop {
        let page = fixture
            .store
            .page_consumer(fixture.project, &consumer, 2, now())
            .expect("the page reads");
        if page.events.is_empty() {
            break;
        }
        assert!(page.events.len() <= 2, "the page honours its limit");
        delivered.extend(page.events.iter().map(|event| event.cursor));
    }

    assert_eq!(delivered, appended, "every event exactly once, in order");
    let distinct: BTreeSet<EventCursor> = delivered.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        delivered.len(),
        "no cursor is delivered twice"
    );

    // A caught-up consumer stays caught up rather than rewinding.
    let checkpoint = fixture
        .store
        .consumer_cursor(fixture.project, &consumer)
        .expect("the read succeeds")
        .expect("the consumer has a checkpoint");
    assert_eq!(checkpoint, *appended.last().expect("a cursor"));
    let empty = fixture
        .store
        .page_consumer(fixture.project, &consumer, 2, now())
        .expect("the page reads");
    assert!(empty.events.is_empty());
    assert_eq!(empty.last_cursor, checkpoint);
}

#[test]
fn consumer_cursor_survives_restart() {
    let fixture = fixture();
    let consumer = external("projector");
    for sequence in 2..6u64 {
        let marker = format!("n-{sequence}");
        fixture
            .store
            .append_control_observation(&observation(&fixture, sequence, &marker))
            .expect("the observation is recorded");
    }
    let first_page = fixture
        .store
        .page_consumer(fixture.project, &consumer, 2, now())
        .expect("the page reads");
    assert_eq!(first_page.events.len(), 2);

    // The consumer process dies and comes back with no memory of its position.
    let fixture = fixture.restart();
    assert_eq!(
        fixture
            .store
            .consumer_cursor(fixture.project, &consumer)
            .expect("the read succeeds"),
        Some(first_page.last_cursor),
        "the checkpoint is on disk, not in the process"
    );
    let resumed = fixture
        .store
        .page_consumer(fixture.project, &consumer, 10, now())
        .expect("the page reads");
    assert_eq!(resumed.events.len(), 2, "it resumes without repeating");
    assert!(
        resumed
            .events
            .iter()
            .all(|event| event.cursor > first_page.last_cursor)
    );

    // A different consumer keeps its own position, and starts from the origin.
    let other = external("archiver");
    let fresh = fixture
        .store
        .page_consumer(fixture.project, &other, 10, now())
        .expect("the page reads");
    assert_eq!(fresh.events.len(), 4, "a new consumer sees everything");
}

// ---------------------------------------------------------------------------
// Control-plane gaps
// ---------------------------------------------------------------------------

#[test]
fn control_sequence_jump_records_typed_control_gap() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the first observation is recorded");

    let jumped = ControlObservation {
        expected_sequence: Some(3),
        ..observation(&fixture, 7, "n-7")
    };
    let outcome = fixture
        .store
        .append_control_observation(&jumped)
        .expect("the later observation is recorded");
    let gap = outcome.control_gap.expect("the jump is recorded as a gap");
    assert_eq!(gap.expected_sequence, 3);
    assert_eq!(gap.received_sequence, 7);
    assert_eq!(gap.detected_cursor, outcome.cursor);

    // The later, trustworthy observation still reduces — but the missing facts
    // keep the conclusion conservative rather than confirmed.
    assert!(outcome.reduced);
    assert_eq!(outcome.projection.observed, ObservedRunState::Running);
    assert_eq!(outcome.projection.derived, DerivedRunState::Stale);

    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let (run, expected, received, cursor): (String, i64, i64, i64) = connection
        .query_row(
            "SELECT agent_run_id, expected_sequence, received_sequence, detected_cursor
             FROM runtime_control_gaps",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the gap row exists");
    assert_eq!(run, RUN);
    assert_eq!((expected, received), (3, 7));
    assert_eq!(
        cursor,
        outcome.cursor.get(),
        "the gap cites the evidence that revealed it"
    );

    // A control-plane gap is not a content gap, and never becomes one.
    assert_eq!(census(&fixture)["runtime_content_gaps"], 0);
}

#[test]
fn control_gap_is_idempotent_on_replay() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the first observation is recorded");
    let jumped = ControlObservation {
        expected_sequence: Some(3),
        ..observation(&fixture, 7, "n-7")
    };
    fixture
        .store
        .append_control_observation(&jumped)
        .expect("the jump is recorded");
    let settled = census(&fixture);
    let before = projection(&fixture, fixture.run);

    // Replaying the same observation across a restart records the same single
    // gap, not a second one.
    let fixture = fixture.restart();
    let replay = fixture
        .store
        .append_control_observation(&ControlObservation {
            expected_sequence: Some(3),
            ..observation(&fixture, 7, "n-7")
        })
        .expect("a replay is not an error");
    assert!(!replay.appended && !replay.reduced);
    assert_eq!(census(&fixture), settled);
    assert_eq!(projection(&fixture, fixture.run), before);
    assert_eq!(census(&fixture)["runtime_control_gaps"], 1);
}

#[test]
fn control_gap_never_infers_terminal_state() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the first observation is recorded");

    // A jump, then a closed stream, then an unreachable runtime: three ways of
    // losing facts, none of which is a verdict about the work.
    for (sequence, marker, contact) in [
        (7u64, "n-7", RuntimeContact::Reachable),
        (8, "n-8", RuntimeContact::StreamClosed),
        (9, "n-9", RuntimeContact::Unavailable),
    ] {
        fixture
            .store
            .append_control_observation(&ControlObservation {
                expected_sequence: Some(sequence - 2),
                contact,
                ..observation(&fixture, sequence, marker)
            })
            .expect("the observation is recorded");
        let run = fixture
            .store
            .get_agent_run(fixture.project, fixture.run)
            .expect("the read succeeds")
            .expect("the run exists");
        assert!(
            !run.projection.derived.is_terminal(),
            "a gap must not conclude a terminal state"
        );
        assert!(
            !run.projection.lifecycle.is_terminal(),
            "a gap must not close a run"
        );
        assert!(run.terminal.is_none(), "a gap is not closure evidence");
        assert!(run.closed_at.is_none());
    }

    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let terminal: i64 = connection
        .query_row(
            "SELECT count(*) FROM agent_runs WHERE terminal_outcome IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(terminal, 0, "no gap may write a terminal outcome");
    assert_eq!(census(&fixture)["runtime_control_gaps"], 3);
}

// ---------------------------------------------------------------------------
// Session-content gaps
// ---------------------------------------------------------------------------

fn discontinuity(
    fixture: &Fixture,
    epoch: u64,
    expected: u64,
    received: u64,
) -> ContentDiscontinuity {
    ContentDiscontinuity {
        project_id: fixture.project,
        agent_run_id: fixture.run,
        content_epoch: epoch,
        expected_sequence: expected,
        received_sequence: received,
        audit_ref: external("timeline-ref-1"),
        detected_at: now(),
    }
}

#[test]
fn content_epoch_gap_returns_timeline_refetch_required() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the observation is recorded");

    let outcome = fixture
        .store
        .record_content_discontinuity(&discontinuity(&fixture, 4, 11, 19))
        .expect("the discontinuity is recorded");
    let ContentGapOutcome::TimelineRefetchRequired {
        run,
        content_epoch,
        expected_sequence,
        received_sequence,
        audit_ref,
    } = outcome
    else {
        panic!("a content discontinuity is a refetch obligation and nothing else")
    };
    assert_eq!(run, fixture.run);
    assert_eq!(content_epoch, 4);
    assert_eq!(expected_sequence, 11);
    assert_eq!(received_sequence, 19);
    assert_eq!(audit_ref, external("timeline-ref-1"));

    // A content epoch rolling over is not a control-plane gap, and the two
    // ledgers never borrow each other's rows.
    assert_eq!(census(&fixture)["runtime_content_gaps"], 1);
    assert_eq!(census(&fixture)["runtime_control_gaps"], 0);
}

#[test]
fn content_sequence_gap_changes_no_lifecycle_or_projection() {
    let fixture = fixture();
    fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "n-2"))
        .expect("the observation is recorded");
    let before = projection(&fixture, fixture.run);
    let settled = census(&fixture);

    fixture
        .store
        .record_content_discontinuity(&discontinuity(&fixture, 1, 4, 9))
        .expect("the discontinuity is recorded");

    assert_eq!(
        projection(&fixture, fixture.run),
        before,
        "lifecycle, observed, derived, revision and the reduced cursor all stand still"
    );
    assert_eq!(
        census(&fixture)["runtime_events"],
        settled["runtime_events"],
        "a content gap appends no control-plane event"
    );

    let fixture = fixture.restart();
    assert_eq!(projection(&fixture, fixture.run), before);
    let run = fixture
        .store
        .get_agent_run(fixture.project, fixture.run)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(run.terminal.is_none());
    assert!(!run.projection.lifecycle.is_terminal());

    // Repeating the same discontinuity records the same single fact.
    fixture
        .store
        .record_content_discontinuity(&discontinuity(&fixture, 1, 4, 9))
        .expect("a replay is not an error");
    assert_eq!(census(&fixture)["runtime_content_gaps"], 1);
}

#[test]
fn content_gap_persists_only_binding_continuity_and_audit_ref() {
    let fixture = fixture();
    fixture
        .store
        .record_content_discontinuity(&discontinuity(&fixture, 2, 4, 9))
        .expect("the discontinuity is recorded");

    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    let mut statement = connection
        .prepare("SELECT * FROM runtime_content_gaps")
        .expect("readable");
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        columns,
        vec![
            "id",
            "project_id",
            "agent_run_id",
            "content_epoch",
            "expected_content_sequence",
            "received_content_sequence",
            "detected_cursor",
            "audit_ref",
            "detected_at",
        ],
        "a content gap stores the binding, the continuity and an audit reference — nothing else"
    );

    // Every stored string is an id, a timestamp or an opaque token.
    let stored: Vec<String> = statement
        .query_map([], |row| {
            Ok(vec![
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ])
        })
        .expect("readable")
        .flat_map(|row| row.expect("a row"))
        .collect();
    for value in &stored {
        assert!(
            !value.contains(' '),
            "no free text may reach a content gap row"
        );
    }
    assert!(stored.contains(&"timeline-ref-1".to_owned()));
    assert!(stored.contains(&RUN.to_owned()));
}

#[test]
fn transcript_and_token_deltas_are_rejected() {
    let fixture = fixture();
    let before = census(&fixture);

    // Every one of these is the runtime's to keep. None of them may be copied
    // into the durable control-plane log.
    for payload in [
        serde_json::json!({"schema_version": 1, "transcript": "the model said hello"}),
        serde_json::json!({"schema_version": 1, "messages": [{"role": "user"}]}),
        serde_json::json!({"schema_version": 1, "message": {"role": "assistant"}}),
        serde_json::json!({"schema_version": 1, "reasoning": "step by step"}),
        serde_json::json!({"schema_version": 1, "tool_calls": []}),
        serde_json::json!({"schema_version": 1, "token_delta": 12}),
        serde_json::json!({"schema_version": 1, "tokens": 4096}),
        serde_json::json!({"schema_version": 1, "usage": {"in": 1}}),
        serde_json::json!({"schema_version": 1, "state": {"output": "..."}}),
        serde_json::json!({"schema_version": 1, "nested": [{"content": "..."}]}),
    ] {
        let candidate = ControlObservation {
            raw: CanonicalDocument::from_value(&payload).expect("a canonical document"),
            ..observation(&fixture, 2, "n-2")
        };
        assert!(
            fixture
                .store
                .append_control_observation(&candidate)
                .is_err(),
            "session content must not reach the durable log: {payload}"
        );
    }
    assert_eq!(
        census(&fixture),
        before,
        "a rejected payload never reaches SQL"
    );

    // A control payload made of control facts is fine.
    fixture
        .store
        .append_control_observation(&ControlObservation {
            raw: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "native_sequence": 2,
                "session_state": "running",
                "observed_at": "2026-08-09T10:00:00Z"
            }))
            .expect("a canonical document"),
            ..observation(&fixture, 2, "n-2")
        })
        .expect("control metadata is welcome");

    // And a transcript cannot be smuggled in as an audit reference either.
    assert!(
        ExternalId::parse("the model said hello and then it said goodbye").is_err(),
        "an audit reference is an opaque token, not prose"
    );
}

#[test]
fn the_session_content_boundary_holds_on_every_append_path() {
    let fixture = fixture();
    let before = census(&fixture);
    let projection_before = projection(&fixture, fixture.run);

    // The boundary is a property of the log, so the narrow legacy paths are held
    // to it exactly as the evidence-complete one is. `record_observation` reduces
    // and `append_runtime_event` does not; neither may persist a transcript.
    let event = NewRuntimeEvent {
        project_id: fixture.project,
        agent_run_id: fixture.run,
        identity: identity("session-1"),
        native_event_id: Some(external("n-9")),
        native_sequence: 2,
        payload: CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "transcript": "the model said hello"
        }))
        .expect("a canonical document"),
        observed_at: now(),
    };
    assert!(
        fixture.store.append_runtime_event(&event).is_err(),
        "a raw append is not a way around the content boundary"
    );
    assert!(
        fixture
            .store
            .record_observation(&NewObservation {
                event: event.clone(),
                observed: ObservedRunState::Running,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: fixture.revision(fixture.run),
                quota_state: None,
            })
            .is_err(),
        "a reducing append is not a way around it either"
    );

    // Token accounting is the runtime's, whichever door it knocks on.
    assert!(
        fixture
            .store
            .append_runtime_event(&NewRuntimeEvent {
                payload: CanonicalDocument::from_value(&serde_json::json!({
                    "schema_version": 1,
                    "nested": [{"token_delta": 12}]
                }))
                .expect("a canonical document"),
                ..event.clone()
            })
            .is_err()
    );

    assert_eq!(
        census(&fixture),
        before,
        "a rejected payload never reaches SQL, by any route"
    );
    assert_eq!(
        projection(&fixture, fixture.run),
        projection_before,
        "and it moves no projection on its way out"
    );

    // Control metadata is welcome through the same doors.
    fixture
        .store
        .append_runtime_event(&NewRuntimeEvent {
            payload: document("control-only"),
            ..event
        })
        .expect("control metadata appends");
}

#[test]
fn two_sessions_may_share_one_native_event_id() {
    let fixture = fixture();
    let run_b = AgentRunId::parse(RUN_B).expect("a canonical id");

    // A native event id is the runtime's numbering *inside one session*. Two
    // sessions of one generation both calling their first event `e-1` are two
    // observations, and collapsing them would silently discard one run's evidence
    // and leave its projection reduced from the other's.
    let first = fixture
        .store
        .append_control_observation(&observation(&fixture, 2, "e-1"))
        .expect("the first session's event appends");
    let second = fixture
        .store
        .append_control_observation(&ControlObservation {
            agent_run_id: run_b,
            identity: identity("session-2"),
            expected_revision: fixture.revision(run_b),
            ..observation(&fixture, 2, "e-1")
        })
        .expect("the second session's event is not a replay of the first");

    assert!(
        second.appended,
        "the second session's event is a new row, not a mapped duplicate"
    );
    assert_ne!(
        first.cursor, second.cursor,
        "two sessions' events hold two cursors"
    );
    assert!(
        first.reduced && second.reduced,
        "each session reduced its own run"
    );
    assert_eq!(
        census(&fixture).get("runtime_events"),
        Some(&2),
        "both observations are stored"
    );

    // The same holds on the raw path, where the runtime's event id is the only
    // identity there is — and where a replay is resolved by *looking the event
    // up*, so the lookup has to be keyed the way the index is.
    let raw = |run, native, sequence| NewRuntimeEvent {
        project_id: fixture.project,
        agent_run_id: run,
        identity: identity(native),
        native_event_id: Some(external("e-2")),
        native_sequence: sequence,
        payload: document("raw-e-2"),
        observed_at: now(),
    };
    let raw_first = fixture
        .store
        .append_runtime_event(&raw(fixture.run, "session-1", 3))
        .expect("the first session's raw event appends");
    let raw_second = fixture
        .store
        .append_runtime_event(&raw(run_b, "session-2", 4))
        .expect("the second session's raw event is not a replay");
    assert_ne!(
        raw_first, raw_second,
        "a shared event id across sessions is two events, not one"
    );

    // A genuine replay — same session, same event id — maps back to *that
    // session's* row. Resolving it without the session in the key would hand back
    // the other run's cursor, and a caller told "you already have this" would file
    // its evidence against a stranger.
    assert_eq!(
        fixture
            .store
            .append_runtime_event(&raw(run_b, "session-2", 5))
            .expect("a replay is not an error"),
        raw_second,
        "the replay resolves to the session that actually holds the event id"
    );
    assert_eq!(
        fixture
            .store
            .append_runtime_event(&raw(fixture.run, "session-1", 6))
            .expect("a replay is not an error"),
        raw_first,
        "and the other session still resolves to its own"
    );
    assert_eq!(
        census(&fixture).get("runtime_events"),
        Some(&4),
        "two control observations and two raw events, and no replay added a fifth"
    );
}

#[test]
fn one_native_event_id_may_not_carry_two_payloads() {
    let fixture = fixture();
    let event = NewRuntimeEvent {
        project_id: fixture.project,
        agent_run_id: fixture.run,
        identity: identity("session-1"),
        native_event_id: Some(external("e-1")),
        native_sequence: 2,
        payload: document("raw-e-1"),
        observed_at: now(),
    };
    let cursor = fixture
        .store
        .append_runtime_event(&event)
        .expect("the first append lands");
    let after = census(&fixture);

    // A raw append is identified by the runtime's own event id — it carries no
    // normalized fields, so the continuity identity does not apply to it. The same
    // id replayed byte-for-byte is the same moment, and maps back to the row that
    // already holds it.
    assert_eq!(
        fixture
            .store
            .append_runtime_event(&event)
            .expect("a replay maps onto the original"),
        cursor,
        "a replay returns the cursor the original already has"
    );
    assert_eq!(census(&fixture), after, "and appends nothing");

    // The same id carrying *different* bytes is not a replay: the runtime has told
    // us two different things about one moment. Answering it with the original
    // cursor would report "already stored" for an observation that was never
    // stored — the second statement vanishes, and every consumer paging the log
    // sees only the first.
    let contradiction = NewRuntimeEvent {
        payload: document("raw-e-1-rewritten"),
        ..event.clone()
    };
    let refused = fixture
        .store
        .append_runtime_event(&contradiction)
        .expect_err("one event id may not name two payloads");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict { .. }
        ),
        "the contradiction is a typed conflict, not a duplicate: {refused:?}"
    );
    assert_eq!(census(&fixture), after, "and nothing lands");

    // The stored row is still the first statement, unrewritten.
    let stored: String = Connection::open(&fixture.path)
        .expect("a raw connection opens")
        .query_row(
            "SELECT payload_hash FROM runtime_events WHERE native_event_id = 'e-1'",
            [],
            |row| row.get(0),
        )
        .expect("the row is readable");
    assert_eq!(
        stored,
        document("raw-e-1").hash().as_str(),
        "the refused append left the original evidence exactly as it was"
    );
}

#[test]
fn an_unlisted_alias_does_not_walk_session_content_past_the_boundary() {
    let fixture = fixture();
    let before = census(&fixture);
    let projection_before = projection(&fixture, fixture.run);

    // Not one of these keys is a word the runtime's vocabulary was ever taught to
    // refuse, which is exactly how an enumerable denylist lets them through: it can
    // only block the aliases someone thought of. Every value here is a single
    // whitespace-free token, so nothing but the *field itself* can be what refuses
    // them — the shape admits control metadata it recognizes and nothing else.
    for payload in [
        serde_json::json!({"schema_version": 1, "assistant_response": "sure-heres-the-code"}),
        serde_json::json!({"schema_version": 1, "agent_reply": "done"}),
        serde_json::json!({"schema_version": 1, "model_output": "hello"}),
        serde_json::json!({"schema_version": 1, "answer": "42"}),
        serde_json::json!({"schema_version": 1, "summary": "refactored-the-store"}),
        serde_json::json!({"schema_version": 1, "notes": "looks-fine"}),
        serde_json::json!({"schema_version": 1, "final_message_text": "bye"}),
        serde_json::json!({"schema_version": 1, "blob": "aGVsbG8="}),
        // And a recognized field is not a lid to hide a subtree under: control
        // metadata is flat scalars, because a list of turns or a nested result is
        // the shape session content arrives in.
        serde_json::json!({"schema_version": 1, "marker": {"role": "assistant"}}),
        serde_json::json!({"schema_version": 1, "marker": ["running-2", "running-3"]}),
        // Nor a place to put prose, the same way the `audit_ref` column refuses it.
        serde_json::json!({"schema_version": 1, "marker": "the model said hello"}),
    ] {
        let candidate = ControlObservation {
            raw: CanonicalDocument::from_value(&payload).expect("a canonical document"),
            ..observation(&fixture, 2, "n-2")
        };
        assert!(
            fixture
                .store
                .append_control_observation(&candidate)
                .is_err(),
            "an unrecognized field is not control metadata: {payload}"
        );
        // The boundary belongs to the log, so the raw path answers the same way.
        assert!(
            fixture
                .store
                .append_runtime_event(&NewRuntimeEvent {
                    project_id: fixture.project,
                    agent_run_id: fixture.run,
                    identity: identity("session-1"),
                    native_event_id: Some(external("e-alias")),
                    native_sequence: 2,
                    payload: CanonicalDocument::from_value(&payload).expect("a canonical document"),
                    observed_at: now(),
                })
                .is_err(),
            "and a raw append is not a way around it: {payload}"
        );
    }
    assert_eq!(
        census(&fixture),
        before,
        "a rejected payload never reaches SQL, whatever it calls itself"
    );
    assert_eq!(
        projection(&fixture, fixture.run),
        projection_before,
        "and it moves no projection on its way out"
    );

    // The control facts themselves are still welcome, spelled any way an adapter
    // spells them: the vocabulary is normalized, not case- and separator-fussy.
    fixture
        .store
        .append_control_observation(&ControlObservation {
            raw: CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "nativeSequence": 2,
                "session-state": "running",
                "contact": "reachable",
                "exit_code": null,
                "observed_at": "2026-08-09T10:00:00Z"
            }))
            .expect("a canonical document"),
            ..observation(&fixture, 2, "n-2")
        })
        .expect("control metadata is welcome however it is spelled");
}

// ---------------------------------------------------------------------------
// Observation and quota land together, and only when the observation is the one
// that actually reduces.
// ---------------------------------------------------------------------------

fn account(fixture: &Fixture) -> kontor_core::id::AccountProfileId {
    use kontor_core::id::CredentialAlias;
    use kontor_core::repository::{
        CredentialReference, CredentialReferenceKind, NewAccountProfile, ProjectRepository,
    };
    let id = kontor_core::id::AccountProfileId::generate();
    fixture
        .store
        .create_account_profile(&NewAccountProfile {
            id,
            project_id: fixture.project,
            label: ExternalName::parse("codex-work").expect("a label"),
            external_account_id: None,
            harness: RuntimeKindKey::parse("generic.runtime").expect("a runtime key"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::ConfigHome,
                alias: CredentialAlias::parse("codex-work").expect("an alias"),
            },
            environment: document("environment"),
            routing: document("routing"),
            capability: document("capability"),
            provider_identity: None,
            enabled: true,
            created_at: now(),
        })
        .expect("the account profile is created");
    id
}

/// Provenance for a runtime-observed decision, matching the row it authorizes.
#[allow(clippy::too_many_arguments)]
fn provenance_for(
    fixture: &Fixture,
    id: kontor_core::id::AccountProfileId,
    state: kontor_core::spec::ProviderQuotaKind,
    resets_at: Option<Timestamp>,
    evidence: kontor_core::id::ContentHash,
    observed_at: Timestamp,
) -> kontor_core::repository::NewQuotaObservationProvenance {
    kontor_core::repository::NewQuotaObservationProvenance {
        id: kontor_core::id::QuotaObservationProvenanceId::generate(),
        project_id: fixture.project,
        account_profile_id: id,
        provider: "codex-work".to_owned(),
        signal_id: "codex-usage-limit-work".to_owned(),
        signal_version: kontor_core::id::SpecVersion::FIRST,
        signal_definition_hash: document("signal").hash().clone(),
        agent_run_id: fixture.run,
        runtime_binding_id: fixture.binding,
        native_id: external("session-1"),
        binding_generation: 1,
        item_epoch: 1,
        item_seq_start: 2,
        item_seq_end: 2,
        source_sequences: vec![(2, 2)],
        item_kind: "assistant_message".to_owned(),
        item_observed_at: observed_at,
        decision_basis: kontor_core::spec::QuotaDecisionBasis::RuntimeRefusal,
        decided_state: state,
        parsed_resets_at: resets_at,
        reset_zone: Some("Europe/Chisinau".to_owned()),
        evidence_digest: evidence,
        recorded_at: observed_at,
    }
}

#[allow(clippy::too_many_arguments)]
fn quota_at(
    fixture: &Fixture,
    id: kontor_core::id::AccountProfileId,
    state: kontor_core::spec::ProviderQuotaKind,
    resets_at: Option<Timestamp>,
    marker: &str,
    expected_revision: AggregateRevision,
    source: kontor_core::spec::ProviderQuotaSource,
    observed_at: Timestamp,
) -> kontor_core::repository::NewProviderQuotaState {
    kontor_core::repository::NewProviderQuotaState {
        project_id: fixture.project,
        account_profile_id: id,
        provider: "codex-work".to_owned(),
        state,
        resets_at,
        windows: Vec::new(),
        credit: None,
        evidence_hash: document(marker).hash().clone(),
        provenance: (source == kontor_core::spec::ProviderQuotaSource::RuntimeObservation).then(
            || {
                provenance_for(
                    fixture,
                    id,
                    state,
                    resets_at,
                    document(marker).hash().clone(),
                    observed_at,
                )
            },
        ),
        source,
        observed_at,
        expected_revision,
        updated_at: observed_at,
    }
}

fn quota(
    fixture: &Fixture,
    id: kontor_core::id::AccountProfileId,
    state: kontor_core::spec::ProviderQuotaKind,
    resets_at: Option<Timestamp>,
    marker: &str,
    expected_revision: AggregateRevision,
) -> kontor_core::repository::NewProviderQuotaState {
    kontor_core::repository::NewProviderQuotaState {
        project_id: fixture.project,
        account_profile_id: id,
        provider: "codex-work".to_owned(),
        state,
        resets_at,
        windows: Vec::new(),
        credit: None,
        evidence_hash: document(marker).hash().clone(),
        provenance: Some(provenance_for(
            fixture,
            id,
            state,
            resets_at,
            document(marker).hash().clone(),
            now(),
        )),
        source: kontor_core::spec::ProviderQuotaSource::RuntimeObservation,
        observed_at: now(),
        expected_revision,
        updated_at: now(),
    }
}

fn current_quota(
    fixture: &Fixture,
    id: kontor_core::id::AccountProfileId,
) -> Option<kontor_core::repository::ProviderQuotaState> {
    use kontor_core::repository::CapacityRepository;
    fixture
        .store
        .list_provider_quota_states(fixture.project)
        .expect("quota states")
        .into_iter()
        .find(|row| row.account_profile_id == id)
}

fn observe_with_quota(
    fixture: &Fixture,
    sequence: u64,
    marker: &str,
    quota_state: Option<kontor_core::repository::NewProviderQuotaState>,
) -> Result<(), kontor_core::repository::RepositoryError> {
    fixture
        .store
        .record_observation(&NewObservation {
            event: NewRuntimeEvent {
                project_id: fixture.project,
                agent_run_id: fixture.run,
                identity: identity("session-1"),
                native_event_id: Some(external(marker)),
                native_sequence: sequence,
                payload: document(marker),
                observed_at: now(),
            },
            observed: ObservedRunState::Running,
            contact: RuntimeContact::Reachable,
            freshness: Freshness::Fresh,
            expected_revision: fixture.revision(fixture.run),
            quota_state,
        })
        .map(|_| ())
}

#[test]
fn a_reducible_observation_writes_its_event_projection_and_quota_together() {
    let fixture = fixture();
    let id = account(&fixture);
    let before = fixture.revision(fixture.run);

    observe_with_quota(
        &fixture,
        10,
        "refusal",
        Some(quota(
            &fixture,
            id,
            kontor_core::spec::ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "refusal",
            AggregateRevision::INITIAL,
        )),
    )
    .expect("the observation reduces");

    assert!(
        fixture.revision(fixture.run) > before,
        "the projection advanced",
    );
    let row = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(row.state, kontor_core::spec::ProviderQuotaKind::Exhausted);
}

#[test]
fn an_older_refusal_cannot_regress_a_newer_availability_row() {
    let fixture = fixture();
    let id = account(&fixture);

    // A newer observation already concluded the account is fine.
    observe_with_quota(
        &fixture,
        20,
        "available",
        Some(quota(
            &fixture,
            id,
            kontor_core::spec::ProviderQuotaKind::Available,
            None,
            "available",
            AggregateRevision::INITIAL,
        )),
    )
    .expect("the newer observation reduces");
    let newer = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(newer.state, kontor_core::spec::ProviderQuotaKind::Available);

    // Now an *older* refusal arrives out of order. Its raw evidence may append;
    // it must not move current quota, because it is not the authoritative
    // observation any more.
    let _ = observe_with_quota(
        &fixture,
        10,
        "stale-refusal",
        Some(quota(
            &fixture,
            id,
            kontor_core::spec::ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "stale-refusal",
            newer.revision,
        )),
    );

    let after = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(
        after.state,
        kontor_core::spec::ProviderQuotaKind::Available,
        "an out-of-order refusal must not regress current quota",
    );
    assert_eq!(after.revision, newer.revision, "the row is untouched");
}

#[test]
fn a_duplicate_replay_cannot_mutate_quota() {
    let fixture = fixture();
    let id = account(&fixture);

    observe_with_quota(
        &fixture,
        30,
        "refusal",
        Some(quota(
            &fixture,
            id,
            kontor_core::spec::ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "refusal",
            AggregateRevision::INITIAL,
        )),
    )
    .expect("the first delivery reduces");
    let first = current_quota(&fixture, id).expect("a quota row");

    // The identical event, delivered twice.
    let _ = observe_with_quota(
        &fixture,
        30,
        "refusal",
        Some(quota(
            &fixture,
            id,
            kontor_core::spec::ProviderQuotaKind::Unknown,
            None,
            "replay",
            first.revision,
        )),
    );

    let after = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(
        after.revision, first.revision,
        "a replay changes no projection, so it changes no quota either",
    );
    assert_eq!(after.state, kontor_core::spec::ProviderQuotaKind::Exhausted);
}

#[test]
fn a_refused_quota_write_rolls_back_the_event_and_the_projection() {
    let fixture = fixture();
    let id = account(&fixture);
    let before = fixture.revision(fixture.run);
    let events_before = census(&fixture);

    // A stale expected revision makes the quota half refuse; the whole
    // transaction must roll back with it.
    let refused = observe_with_quota(
        &fixture,
        40,
        "refusal",
        Some(quota(
            &fixture,
            id,
            kontor_core::spec::ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "refusal",
            AggregateRevision::INITIAL
                .next()
                .expect("a next revision")
                .next()
                .expect("a next revision"),
        )),
    );
    assert!(refused.is_err(), "a stale quota revision refuses the write");

    assert_eq!(
        fixture.revision(fixture.run),
        before,
        "the projection did not advance",
    );
    assert_eq!(
        census(&fixture),
        events_before,
        "the event did not survive its own transaction",
    );
    assert!(current_quota(&fixture, id).is_none(), "no quota row landed");
}

/// Per-run native sequence orders one run's events. It is not an authority
/// order for an `(account, provider)` pair, which the poller, an operator and
/// any run holding the account all write.
///
/// So a refusal can be the newest reducible event *for its run* and still
/// describe a moment older than a `ProviderReport` that has already restored
/// availability. It must not overwrite it.
#[test]
fn a_late_runtime_refusal_cannot_overwrite_a_newer_provider_report() {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);

    // The poller answered at 10:05: this account is fine.
    fixture
        .store
        .set_provider_quota_state(&quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Available,
            None,
            "poller",
            AggregateRevision::INITIAL,
            ProviderQuotaSource::ProviderReport,
            at("2026-08-09T10:05:00Z"),
        ))
        .expect("the poller's report is stored");
    let newer = current_quota(&fixture, id).expect("a quota row");

    // A refusal observed at 10:00 arrives afterwards, and is the newest
    // reducible sequence for its own run.
    observe_with_quota(
        &fixture,
        50,
        "late-refusal",
        Some(quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "late-refusal",
            newer.revision,
            ProviderQuotaSource::RuntimeObservation,
            at("2026-08-09T10:00:00Z"),
        )),
    )
    .expect("the observation itself still reduces");

    let after = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(
        after.state,
        ProviderQuotaKind::Available,
        "an older runtime refusal must not regress a newer provider report",
    );
    assert_eq!(after.revision, newer.revision, "the row is untouched");
    assert_eq!(after.source, ProviderQuotaSource::ProviderReport);
}

/// On an identical instant the structured answer wins: a `ProviderReport` is
/// the only source that can restore availability without a human, while a
/// runtime observation only ever learns after something was already refused.
#[test]
fn on_an_equal_instant_the_provider_report_outranks_a_runtime_refusal() {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);
    let instant = at("2026-08-09T10:00:00Z");

    fixture
        .store
        .set_provider_quota_state(&quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Available,
            None,
            "poller",
            AggregateRevision::INITIAL,
            ProviderQuotaSource::ProviderReport,
            instant,
        ))
        .expect("the poller's report is stored");
    let newer = current_quota(&fixture, id).expect("a quota row");

    observe_with_quota(
        &fixture,
        60,
        "equal-instant",
        Some(quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "equal-instant",
            newer.revision,
            ProviderQuotaSource::RuntimeObservation,
            instant,
        )),
    )
    .expect("the observation reduces");

    let after = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(after.state, ProviderQuotaKind::Available);
    assert_eq!(after.revision, newer.revision);
}

/// A newer runtime refusal is still allowed to block, so the guard fences
/// staleness rather than the source.
#[test]
fn a_newer_runtime_refusal_still_blocks_over_an_older_report() {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);

    fixture
        .store
        .set_provider_quota_state(&quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Available,
            None,
            "poller",
            AggregateRevision::INITIAL,
            ProviderQuotaSource::ProviderReport,
            at("2026-08-09T09:00:00Z"),
        ))
        .expect("the poller's report is stored");
    let older = current_quota(&fixture, id).expect("a quota row");

    observe_with_quota(
        &fixture,
        70,
        "fresh-refusal",
        Some(quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Exhausted,
            Some(at("2099-01-01T00:00:00Z")),
            "fresh-refusal",
            older.revision,
            ProviderQuotaSource::RuntimeObservation,
            at("2026-08-09T10:00:00Z"),
        )),
    )
    .expect("the observation reduces");

    let after = current_quota(&fixture, id).expect("a quota row");
    assert_eq!(
        after.state,
        ProviderQuotaKind::Exhausted,
        "a refusal newer than the report is exactly what must be recorded",
    );
    assert!(after.revision > older.revision);
}

// ---------------------------------------------------------------------------
// Provenance: the record that says which item authorized a quota decision.
// ---------------------------------------------------------------------------

use kontor_core::repository::NewQuotaObservationProvenance;

fn write_runtime_quota(
    fixture: &Fixture,
    id: kontor_core::id::AccountProfileId,
    revision: AggregateRevision,
    mutate: impl FnOnce(&mut NewQuotaObservationProvenance),
) -> Result<(), kontor_core::repository::RepositoryError> {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let evidence = document("refusal").hash().clone();
    let mut record = provenance_for(
        fixture,
        id,
        ProviderQuotaKind::Exhausted,
        Some(at("2099-01-01T00:00:00Z")),
        evidence.clone(),
        now(),
    );
    mutate(&mut record);
    fixture
        .store
        .set_provider_quota_state(&kontor_core::repository::NewProviderQuotaState {
            project_id: fixture.project,
            account_profile_id: id,
            provider: "codex-work".to_owned(),
            state: ProviderQuotaKind::Exhausted,
            resets_at: Some(at("2099-01-01T00:00:00Z")),
            windows: Vec::new(),
            credit: None,
            evidence_hash: evidence,
            provenance: Some(record),
            source: ProviderQuotaSource::RuntimeObservation,
            observed_at: now(),
            expected_revision: revision,
            updated_at: now(),
        })
        .map(|_| ())
}

/// The readback an operator's "why is this blocked?" resolves to. This is also
/// the only test that reaches the provenance SELECT, which is how a column
/// renamed in the INSERT but not the SELECT would otherwise ship green.
#[test]
fn a_recorded_decision_reads_back_exactly_including_its_range_set() {
    use kontor_core::repository::CapacityRepository;

    let fixture = fixture();
    let id = account(&fixture);
    write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |record| {
        record.item_seq_start = 4;
        record.item_seq_end = 9;
        // Disjoint ranges: an envelope alone would claim 6, 7 and 8.
        record.source_sequences = vec![(4, 5), (9, 9)];
        record.item_seq_end = 9;
    })
    .expect("the decision is recorded");

    let row = current_quota(&fixture, id).expect("a quota row");
    let provenance_id = row.provenance_id.expect("the row cites its provenance");
    let stored = fixture
        .store
        .get_quota_observation_provenance(fixture.project, provenance_id)
        .expect("the record reads")
        .expect("the record exists");
    assert_eq!(stored.record.signal_id, "codex-usage-limit-work");
    assert_eq!(stored.record.item_kind, "assistant_message");
    assert_eq!(
        stored.record.source_sequences,
        vec![(4, 5), (9, 9)],
        "the exact set round-trips, not an envelope",
    );
    assert_eq!(stored.record.evidence_digest, row.evidence_hash);
}

#[test]
fn a_runtime_observation_without_provenance_is_refused_even_when_a_row_exists() {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);
    write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |_| {})
        .expect("the first decision is recorded");
    let existing = current_quota(&fixture, id).expect("a row");

    // A *newer* runtime observation carrying no record would clear the
    // reference and leave a new decision nobody can explain.
    let refused =
        fixture
            .store
            .set_provider_quota_state(&kontor_core::repository::NewProviderQuotaState {
                project_id: fixture.project,
                account_profile_id: id,
                provider: "codex-work".to_owned(),
                state: ProviderQuotaKind::Unknown,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: document("other").hash().clone(),
                provenance: None,
                source: ProviderQuotaSource::RuntimeObservation,
                observed_at: at("2026-08-09T12:00:00Z"),
                expected_revision: existing.revision,
                updated_at: at("2026-08-09T12:00:00Z"),
            });
    assert!(
        refused.is_err(),
        "an unexplained runtime decision is refused"
    );
    let after = current_quota(&fixture, id).expect("a row");
    assert_eq!(after.revision, existing.revision, "nothing moved");
    assert!(after.provenance_id.is_some(), "the reference survives");
}

/// Every field the record shares with the row it authorizes is compared, and a
/// disagreement in any one of them refuses the whole write.
#[test]
fn a_record_that_disagrees_with_its_row_is_refused_per_column() {
    use kontor_core::spec::ProviderQuotaKind;

    let fixture = fixture();
    let other_project = ProjectId::generate();
    for (name, mutate) in [
        (
            "project_id",
            Box::new(move |r: &mut NewQuotaObservationProvenance| r.project_id = other_project)
                as Box<dyn FnOnce(&mut NewQuotaObservationProvenance)>,
        ),
        (
            "account_profile_id",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.account_profile_id = kontor_core::id::AccountProfileId::generate();
            }),
        ),
        (
            "provider",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.provider = "codex-personal".to_owned();
            }),
        ),
        (
            "decided_state",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.decided_state = ProviderQuotaKind::Unknown;
            }),
        ),
        (
            "parsed_resets_at",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.parsed_resets_at = Some(at("2098-01-01T00:00:00Z"));
            }),
        ),
        (
            "evidence_digest",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.evidence_digest = document("something else").hash().clone();
            }),
        ),
    ] {
        let id = account(&fixture);
        let refused = write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, mutate);
        assert!(refused.is_err(), "{name} disagreement must refuse");
        assert!(
            current_quota(&fixture, id).is_none(),
            "{name}: neither the row nor the record may land",
        );
    }
}

/// The composite binding FK, exercised rather than read. Each tuple element is
/// substituted in turn with a value the store does not hold.
#[test]
fn a_record_citing_a_binding_the_store_does_not_hold_is_refused() {
    let fixture = fixture();
    for (name, mutate) in [
        (
            "runtime_binding_id",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.runtime_binding_id = kontor_core::id::RuntimeBindingId::generate();
            }) as Box<dyn FnOnce(&mut NewQuotaObservationProvenance)>,
        ),
        (
            "agent_run_id",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.agent_run_id = AgentRunId::generate();
            }),
        ),
        (
            "binding_generation",
            Box::new(|r: &mut NewQuotaObservationProvenance| r.binding_generation = 2),
        ),
        (
            "native_id",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.native_id = external("another-session");
            }),
        ),
    ] {
        let id = account(&fixture);
        let refused = write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, mutate);
        assert!(
            refused.is_err(),
            "{name}: a record must cite the exact immutable binding",
        );
        assert!(
            current_quota(&fixture, id).is_none(),
            "{name}: nothing lands"
        );
    }
}

/// The child set has to describe the same item the envelope claims.
#[test]
fn an_inconsistent_or_empty_range_set_is_refused() {
    let fixture = fixture();
    for (name, mutate) in [
        (
            "empty",
            Box::new(|r: &mut NewQuotaObservationProvenance| r.source_sequences.clear())
                as Box<dyn FnOnce(&mut NewQuotaObservationProvenance)>,
        ),
        (
            "reversed range",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.source_sequences = vec![(9, 4)];
            }),
        ),
        (
            "unordered",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.item_seq_start = 4;
                r.item_seq_end = 9;
                r.source_sequences = vec![(9, 9), (4, 5)];
            }),
        ),
        (
            "envelope disagrees",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.item_seq_start = 1;
                r.item_seq_end = 99;
                r.source_sequences = vec![(2, 2)];
            }),
        ),
    ] {
        let id = account(&fixture);
        let refused = write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, mutate);
        assert!(refused.is_err(), "{name}: the set must match the envelope");
        assert!(
            current_quota(&fixture, id).is_none(),
            "{name}: nothing lands"
        );
    }
}

/// A sequence larger than the database stores exactly is refused rather than
/// clamped: a clamped number is one nobody observed, written as though it were.
#[test]
fn a_sequence_too_large_to_store_exactly_is_refused_rather_than_clamped() {
    let fixture = fixture();
    let id = account(&fixture);
    let refused = write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |record| {
        record.item_seq_start = u64::MAX;
        record.item_seq_end = u64::MAX;
        record.source_sequences = vec![(u64::MAX, u64::MAX)];
    });
    assert!(refused.is_err(), "an unstorable sequence is a refusal");
    assert!(current_quota(&fixture, id).is_none());
}

/// No marker, sentence or fragment of a refusal may reach the database.
#[test]
fn no_refusal_text_reaches_the_database() {
    let fixture = fixture();
    let id = account(&fixture);
    write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |_| {})
        .expect("the decision is recorded");
    let bytes = std::fs::read(&fixture.path).expect("the database is readable");
    let haystack = String::from_utf8_lossy(&bytes);
    for fragment in [
        "individual spend limit",
        "usage limit",
        "cc_cli_limit_message",
        "You've hit",
    ] {
        assert!(
            !haystack.contains(fragment),
            "the database must not contain {fragment:?}",
        );
    }
}

/// The same-project trigger, reached directly and against a record that really
/// exists in another project.
///
/// Pointing at a nonexistent id proves nothing: an id-only trigger would refuse
/// that too. A *real* project-B record is the only thing that separates
/// `(id, project)` from `id`.
#[test]
fn a_quota_row_cannot_cite_another_projects_provenance() {
    use kontor_core::repository::CapacityRepository;

    let fixture = fixture();
    let id = account(&fixture);
    write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |_| {})
        .expect("this project's decision is recorded");
    let own = current_quota(&fixture, id).expect("a row");
    let own_provenance = own.provenance_id.expect("its own provenance");

    // A record that genuinely belongs to project B, written through the store
    // so it satisfies every invariant a real record does.
    let project_b = ProjectId::parse("0193f000-0000-7000-8000-0000000000b1").expect("project B");
    let account_b = kontor_core::id::AccountProfileId::generate();
    {
        use kontor_core::repository::{
            CredentialReference, CredentialReferenceKind, NewAccountProfile, ProjectRepository,
        };
        fixture
            .store
            .create_account_profile(&NewAccountProfile {
                id: account_b,
                project_id: project_b,
                label: ExternalName::parse("codex-work-b").expect("a label"),
                external_account_id: None,
                harness: RuntimeKindKey::parse("generic.runtime").expect("a runtime key"),
                credential_ref: CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: kontor_core::id::CredentialAlias::parse("b-alias").expect("an alias"),
                },
                environment: document("environment"),
                routing: document("routing"),
                capability: document("capability"),
                provider_identity: None,
                enabled: true,
                created_at: now(),
            })
            .expect("project B has an account");
    }
    let evidence_b = document("b-refusal").hash().clone();
    let mut record_b = provenance_for(
        &fixture,
        account_b,
        kontor_core::spec::ProviderQuotaKind::Unknown,
        None,
        evidence_b.clone(),
        now(),
    );
    record_b.project_id = project_b;
    record_b.agent_run_id =
        AgentRunId::parse("0193f000-0000-7000-8000-0000000000b5").expect("B's run");
    record_b.runtime_binding_id =
        kontor_core::id::RuntimeBindingId::parse("0193f000-0000-7000-8000-0000000000b6")
            .expect("B's binding");
    record_b.native_id = external("session-b");
    let foreign_id = record_b.id.to_string();
    fixture
        .store
        .set_provider_quota_state(&kontor_core::repository::NewProviderQuotaState {
            project_id: project_b,
            account_profile_id: account_b,
            provider: "codex-work".to_owned(),
            state: kontor_core::spec::ProviderQuotaKind::Unknown,
            resets_at: None,
            windows: Vec::new(),
            credit: None,
            evidence_hash: evidence_b,
            provenance: Some(record_b),
            source: kontor_core::spec::ProviderQuotaSource::RuntimeObservation,
            observed_at: now(),
            expected_revision: AggregateRevision::INITIAL,
            updated_at: now(),
        })
        .expect("project B records its own decision");

    let connection = Connection::open(&fixture.path).expect("a raw connection");
    connection
        .execute("PRAGMA foreign_keys = ON", [])
        .expect("foreign keys on");

    // UPDATE: this project's row may not repoint at it.
    let updated = connection.execute(
        "UPDATE provider_quota_states SET provenance_id = ?1
          WHERE project_id = ?2 AND account_profile_id = ?3",
        rusqlite::params![
            foreign_id.as_str(),
            fixture.project.to_string(),
            id.to_string()
        ],
    );
    assert!(
        updated.is_err(),
        "the update path must refuse a record another project owns",
    );

    // INSERT: nor may a new row in this project cite it.
    let inserted = connection.execute(
        "INSERT INTO provider_quota_states
             (project_id, account_profile_id, provider, state, resets_at, evidence_hash,
              source, observed_at, revision, updated_at, provenance_id)
         VALUES (?1, ?2, 'codex-personal', 'unknown', NULL,
                 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                 'runtime_observation', '2026-08-09T10:00:00Z', 1, '2026-08-09T10:00:00Z', ?3)",
        rusqlite::params![
            fixture.project.to_string(),
            id.to_string(),
            foreign_id.as_str()
        ],
    );
    assert!(
        inserted.is_err(),
        "the insert path must refuse it too, not only the update path",
    );

    let after = current_quota(&fixture, id).expect("a row");
    assert_eq!(
        after.provenance_id,
        Some(own_provenance),
        "the row still cites its own project's record",
    );
}

fn provenance_count(fixture: &Fixture) -> i64 {
    Connection::open(&fixture.path)
        .expect("a raw connection")
        .query_row(
            "SELECT COUNT(*) FROM provider_quota_observation_provenance",
            [],
            |row| row.get(0),
        )
        .expect("a count")
}

fn range_count(fixture: &Fixture) -> i64 {
    Connection::open(&fixture.path)
        .expect("a raw connection")
        .query_row(
            "SELECT COUNT(*) FROM provider_quota_observation_source_ranges",
            [],
            |row| row.get(0),
        )
        .expect("a count")
}

/// Cardinality, stated exactly: one accepted item is one record and one moved
/// pointer. A replay, a stale observation and a refused write add none and move
/// nothing.
#[test]
fn provenance_cardinality_matches_what_was_actually_accepted() {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);
    assert_eq!(provenance_count(&fixture), 0);

    // Accepted: exactly one record, one pointer.
    write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |_| {})
        .expect("the decision is recorded");
    assert_eq!(provenance_count(&fixture), 1, "one item, one record");
    assert_eq!(range_count(&fixture), 1, "and its exact range set");
    let accepted = current_quota(&fixture, id).expect("a row");
    let pointer = accepted.provenance_id.expect("the pointer moved");

    // Stale: an older runtime observation is refused by the recency rule before
    // anything is written.
    let evidence = document("older").hash().clone();
    let stale =
        fixture
            .store
            .set_provider_quota_state(&kontor_core::repository::NewProviderQuotaState {
                project_id: fixture.project,
                account_profile_id: id,
                provider: "codex-work".to_owned(),
                state: ProviderQuotaKind::Unknown,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: evidence.clone(),
                provenance: Some(provenance_for(
                    &fixture,
                    id,
                    ProviderQuotaKind::Unknown,
                    None,
                    evidence,
                    at("2026-08-09T08:00:00Z"),
                )),
                source: ProviderQuotaSource::RuntimeObservation,
                observed_at: at("2026-08-09T08:00:00Z"),
                expected_revision: accepted.revision,
                updated_at: at("2026-08-09T08:00:00Z"),
            });
    assert!(
        stale.is_ok(),
        "a stale observation is a no-op, not an error"
    );
    assert_eq!(provenance_count(&fixture), 1, "stale adds no record");
    assert_eq!(
        current_quota(&fixture, id).expect("a row").provenance_id,
        Some(pointer),
        "stale cannot move the pointer",
    );

    // Refused: a record that disagrees with its row adds nothing either.
    let refused = write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, |record| {
        record.provider = "codex-personal".to_owned();
    });
    assert!(refused.is_err());
    assert_eq!(provenance_count(&fixture), 1, "a refusal adds no record");
    assert_eq!(range_count(&fixture), 1);
    assert_eq!(
        current_quota(&fixture, id).expect("a row").provenance_id,
        Some(pointer),
        "a refusal cannot move the pointer",
    );
}

/// The failpoint the whole transaction exists for: the record is inserted, then
/// the quota half refuses. Neither may survive, and neither may the child rows.
#[test]
fn a_failure_after_the_record_but_before_the_row_leaves_neither() {
    let fixture = fixture();
    let id = account(&fixture);
    assert_eq!(provenance_count(&fixture), 0);

    // A stale expected revision fails the quota half *after* the record and its
    // ranges have been inserted in the same transaction.
    let refused = write_runtime_quota(
        &fixture,
        id,
        AggregateRevision::INITIAL
            .next()
            .expect("a next revision")
            .next()
            .expect("a next revision"),
        |_| {},
    );
    assert!(refused.is_err(), "the quota half refuses");
    assert_eq!(
        provenance_count(&fixture),
        0,
        "the record did not survive its own transaction",
    );
    assert_eq!(range_count(&fixture), 0, "nor did its ranges");
    assert!(current_quota(&fixture, id).is_none(), "nor did the row");
}

/// Each independently stored integer can regress on its own, so each is proven
/// on its own. A clamp writes a number nobody observed.
#[test]
fn every_stored_integer_refuses_rather_than_clamps() {
    let fixture = fixture();
    for (name, mutate) in [
        (
            "binding_generation",
            Box::new(|r: &mut NewQuotaObservationProvenance| r.binding_generation = u64::MAX)
                as Box<dyn FnOnce(&mut NewQuotaObservationProvenance)>,
        ),
        (
            "item_epoch",
            Box::new(|r: &mut NewQuotaObservationProvenance| r.item_epoch = u64::MAX),
        ),
        (
            "envelope",
            Box::new(|r: &mut NewQuotaObservationProvenance| {
                r.item_seq_start = u64::MAX;
                r.item_seq_end = u64::MAX;
                r.source_sequences = vec![(u64::MAX, u64::MAX)];
            }),
        ),
    ] {
        let id = account(&fixture);
        let refused = write_runtime_quota(&fixture, id, AggregateRevision::INITIAL, mutate);
        assert!(refused.is_err(), "{name} must refuse rather than clamp");
        assert!(
            current_quota(&fixture, id).is_none(),
            "{name}: nothing lands"
        );
        assert_eq!(provenance_count(&fixture), 0, "{name}: no record either");
    }
}

// ---------------------------------------------------------------------------
// The quota chain on the export surface.
//
// The export carries its own digest, so any tamper that rehashes produces a
// document that verifies on digest alone. What keeps a forged lineage out is the
// link checking in `KontorExportV1::verify`, and every assertion below tampers
// *and rehashes* so that it is the links being tested and not the hash.
// ---------------------------------------------------------------------------

/// Recompute the records digest, exactly as the export writes it.
fn rehash(document: &mut serde_json::Value) {
    let mut records = serde_json::to_vec(
        document
            .get("records")
            .expect("the export document carries records"),
    )
    .expect("the records serialize");
    records.push(b'\n');
    document["records_hash"] =
        serde_json::json!(kontor_core::id::ContentHash::of(&records).to_string());
}

/// One realm holding a coherent quota chain: a runtime-observed row with
/// provenance, its source ranges and a window, plus a credit-bearing row from a
/// provider report.
fn chain() -> (Fixture, serde_json::Value) {
    use kontor_core::quota::{CreditBalance, QuotaWindow, QuotaWindowKind};
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);

    let mut runtime = quota_at(
        &fixture,
        id,
        ProviderQuotaKind::Exhausted,
        Some(at("2026-08-09T22:40:00Z")),
        "refusal",
        AggregateRevision::INITIAL,
        ProviderQuotaSource::RuntimeObservation,
        at("2026-08-09T10:05:00Z"),
    );
    runtime.windows = vec![QuotaWindow {
        kind: QuotaWindowKind::Session,
        resets_at: at("2026-08-09T22:40:00Z"),
        used_percent: 100,
    }];
    fixture
        .store
        .set_provider_quota_state(&runtime)
        .expect("the runtime observation is stored with its provenance");

    let credit = kontor_core::repository::NewProviderQuotaState {
        provider: "opencode".to_owned(),
        state: ProviderQuotaKind::Drained,
        resets_at: None,
        windows: Vec::new(),
        credit: Some(CreditBalance {
            remaining: kontor_core::id::Money {
                minor_units: 0,
                currency: kontor_core::id::CurrencyCode::parse("USD").expect("a valid currency"),
            },
            reserve: kontor_core::id::Money {
                minor_units: 500,
                currency: kontor_core::id::CurrencyCode::parse("USD").expect("a valid currency"),
            },
        }),
        provenance: None,
        source: ProviderQuotaSource::ProviderReport,
        ..quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Drained,
            None,
            "balance",
            AggregateRevision::INITIAL,
            ProviderQuotaSource::ProviderReport,
            at("2026-08-09T10:06:00Z"),
        )
    };
    fixture
        .store
        .set_provider_quota_state(&credit)
        .expect("the credit row is stored");

    let export = kontor_store::backup::export_realm(&fixture.store, at("2026-08-10T10:00:00Z"))
        .expect("the export");
    let document = serde_json::to_value(&export).expect("the export serializes");
    (fixture, document)
}

/// Re-read a tampered document through the production entry point.
fn reparse(
    document: &serde_json::Value,
) -> Result<kontor_store::backup::KontorExportV1, kontor_store::backup::BackupError> {
    kontor_store::backup::KontorExportV1::parse(
        &serde_json::to_vec(document).expect("the document serializes"),
    )
}

/// The chain leaves the realm whole: exact ids, ranges, digests and balances,
/// and not one fragment of what a provider actually said.
#[test]
fn the_exported_quota_chain_is_exact_and_carries_no_refusal_text() {
    let (fixture, document) = chain();
    let export = reparse(&document).expect("a coherent chain verifies");

    let provenance = &export.records.provider_quota_observation_provenance;
    assert_eq!(provenance.len(), 1, "one runtime observation, one record");
    let record = &provenance[0];
    assert_eq!(record.project_id, fixture.project.to_string());
    assert_eq!(record.signal_id, "codex-usage-limit-work");
    assert_eq!(record.decision_basis, "runtime_refusal");
    assert_eq!(record.decided_state, "exhausted");

    let ranges = &export.records.provider_quota_observation_source_ranges;
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].provenance_id, record.id);
    assert_eq!(ranges[0].ordinal, 0);
    assert_eq!(
        (ranges[0].seq_start, ranges[0].seq_end),
        (record.item_seq_start, record.item_seq_end),
        "the exported ranges span exactly the envelope the record claims",
    );

    let runtime = export
        .records
        .provider_quota_states
        .iter()
        .find(|row| row.provider == "codex-work")
        .expect("the runtime-observed row");
    assert_eq!(runtime.provenance_id.as_deref(), Some(record.id.as_str()));
    assert_eq!(runtime.evidence_hash, record.evidence_digest);
    assert_eq!(runtime.source, "runtime_observation");

    // The balance columns, which decide launch eligibility.
    let balance = export
        .records
        .provider_quota_states
        .iter()
        .find(|row| row.provider == "opencode")
        .expect("the credit row");
    assert_eq!(balance.credit_minor_units, Some(0));
    assert_eq!(balance.credit_reserve_minor_units, Some(500));
    assert_eq!(balance.credit_currency.as_deref(), Some("USD"));

    assert_eq!(export.records.provider_quota_windows.len(), 1);
    let window = &export.records.provider_quota_windows[0];
    assert_eq!(window.provider, "codex-work");
    assert_eq!(window.used_percent, 100);

    // Through the import, where the chain must arrive as lineage and nothing
    // else. A destination realm learns what the source held and can re-judge it;
    // it does not inherit live quota authority over accounts it does not own.
    let lineage = export.records.lineage().expect("the lineage");
    let destination_home = TempDir::new().expect("a temporary directory");
    let destination_database = destination_home.path().join("kontor.db");
    let destination = SqliteStore::open(&destination_database).expect("the destination migrates");
    use kontor_core::repository::ProjectRepository;
    let into = ProjectId::generate();
    destination
        .create_project(&kontor_core::repository::NewProject {
            id: into,
            name: kontor_core::id::ExternalName::parse("Quota lineage destination")
                .expect("a name"),
            root_path: kontor_core::id::ExternalName::parse("/tmp/kontor-quota-destination")
                .expect("a path"),
            created_at: at("2026-08-11T09:00:00Z"),
        })
        .expect("the destination project exists");
    let report = kontor_store::backup::import_export(
        &destination,
        &export,
        &kontor_store::backup::ImportPlan::redacted_import_into(into),
        at("2026-08-11T10:00:00Z"),
    )
    .expect("the chain imports");

    // Read back through the supported surface, not raw SQL: this is what an
    // operator or a later import actually sees.
    let recorded = destination
        .imported_records(&report.import_id.to_string())
        .expect("the lineage reads back");
    let inspect = Connection::open(&destination_database).expect("a raw connection");
    for kind in [
        "provider_quota_observation_provenance",
        "provider_quota_observation_source_ranges",
        "provider_quota_states",
        "provider_quota_windows",
    ] {
        let mut expected: Vec<(String, String)> = lineage
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| (entry.identity.clone(), entry.hash.to_string()))
            .collect();
        assert!(!expected.is_empty(), "{kind} must appear in the lineage");
        let mut found: Vec<(String, String)> = recorded
            .iter()
            .filter(|row| row.record_kind == kind)
            .map(|row| (row.source_identity.clone(), row.source_hash.clone()))
            .collect();
        expected.sort();
        found.sort();
        assert_eq!(found, expected, "{kind} lineage must be exact");

        // Lineage, never authority. Raw SQL is right here: the claim is that
        // these tables hold nothing at all in a destination.
        let live: i64 = inspect
            .query_row(&format!("SELECT COUNT(*) FROM {kind}"), [], |row| {
                row.get(0)
            })
            .expect("a count");
        assert_eq!(
            live, 0,
            "{kind} must not be materialized into a destination"
        );
    }

    let serialized = serde_json::to_string(&export).expect("the export serializes");
    // The readback type an operator holds, in both of its renderings.
    let readback = format!(
        "{recorded:?}{}",
        serde_json::to_string(
            &recorded
                .iter()
                .map(|row| (
                    row.record_kind.clone(),
                    row.source_identity.clone(),
                    row.source_hash.clone(),
                    row.disposition.clone(),
                    row.reason_code.clone(),
                ))
                .collect::<Vec<_>>()
        )
        .expect("the readback serializes"),
    );
    for fragment in [
        "usage limit",
        "individual spend limit",
        "session limit resets",
        "try again at",
        "system error",
        "insufficient credits",
    ] {
        for (surface, text) in [("export", &serialized), ("import readback", &readback)] {
            assert!(
                !text.to_lowercase().contains(fragment),
                "the {surface} must carry no refusal fragment, found {fragment:?}",
            );
        }
    }
}

/// One forgery attempt: what it is called, what it does to the document, and the
/// exact rule the verifier must answer with.
type TamperCase = (
    &'static str,
    Box<dyn Fn(&mut serde_json::Value)>,
    &'static str,
);

/// Index of the quota state for one provider in a document.
fn state_at(document: &serde_json::Value, provider: &str) -> usize {
    document["records"]["provider_quota_states"]
        .as_array()
        .expect("states are an array")
        .iter()
        .position(|row| row["provider"] == provider)
        .unwrap_or_else(|| panic!("a {provider} state"))
}

/// Tamper, rehash, and return the exact rule the verifier refused with.
///
/// Rehashing is the point: without it every case would be killed by the digest
/// and prove nothing about the links.
fn refused_detail(tamper: impl FnOnce(&mut serde_json::Value)) -> String {
    let (_fixture, mut document) = chain();
    tamper(&mut document);
    rehash(&mut document);
    match reparse(&document) {
        Err(kontor_store::backup::BackupError::Verification { detail }) => detail.to_owned(),
        Err(other) => panic!("expected a verification refusal, got {other:?}"),
        Ok(_) => panic!("a tampered chain verified"),
    }
}

/// Every broken link in the four-table chain, each tampered *and rehashed*.
///
/// The exact rule is asserted rather than "some refusal", because a tamper that
/// changes a vector's length also invalidates the continuity summary: matching
/// on the error kind alone would let a stale-count refusal stand in for a link
/// check that never ran.
#[test]
fn a_rehashed_document_cannot_forge_the_quota_chain() {
    let codex = |document: &serde_json::Value| state_at(document, "codex-work");

    let cases: Vec<TamperCase> = vec![
        (
            "the cited provenance is dropped",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_provenance"] =
                    serde_json::json!([]);
                document["records"]["provider_quota_observation_source_ranges"] =
                    serde_json::json!([]);
            }),
            "a provider_quota_states row cites provenance the export does not carry",
        ),
        (
            "the pointer is dropped while its record remains",
            Box::new(move |document: &mut serde_json::Value| {
                let at = codex(document);
                document["records"]["provider_quota_states"][at]["provenance_id"] =
                    serde_json::Value::Null;
            }),
            "a provider_quota_states row does not cite the provenance that decided it",
        ),
        (
            "the record's digest no longer matches the row citing it",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_provenance"][0]["evidence_digest"] = serde_json::json!(
                    "blake3:0000000000000000000000000000000000000000000000000000000000000000"
                );
            }),
            "a provider_quota_states row cites provenance whose evidence digest is not the row's",
        ),
        (
            "the pointer is moved to a neighbouring record",
            Box::new(move |document: &mut serde_json::Value| {
                let mut neighbour =
                    document["records"]["provider_quota_observation_provenance"][0].clone();
                neighbour["id"] = serde_json::json!("0193f000-0000-7000-8000-0000000009ff");
                neighbour["evidence_digest"] = serde_json::json!(
                    "blake3:1111111111111111111111111111111111111111111111111111111111111111"
                );
                let mut range =
                    document["records"]["provider_quota_observation_source_ranges"][0].clone();
                range["provenance_id"] = neighbour["id"].clone();
                let at = codex(document);
                document["records"]["provider_quota_states"][at]["provenance_id"] =
                    neighbour["id"].clone();
                document["records"]["provider_quota_observation_provenance"]
                    .as_array_mut()
                    .expect("an array")
                    .push(neighbour);
                document["records"]["provider_quota_observation_source_ranges"]
                    .as_array_mut()
                    .expect("an array")
                    .push(range);
            }),
            // The reverse invariant reaches this first, and says the more
            // useful thing: the record that actually decided this row is still
            // sitting there uncited.
            "a provider_quota_states row does not cite the provenance that decided it",
        ),
        (
            "an operator row claims runtime authority",
            Box::new(move |document: &mut serde_json::Value| {
                let at = codex(document);
                document["records"]["provider_quota_states"][at]["source"] =
                    serde_json::json!("provider_report");
            }),
            "a provider_quota_states row that is not a runtime observation cites quota provenance",
        ),
        (
            "two records share one identity across projects",
            Box::new(|document: &mut serde_json::Value| {
                let mut twin =
                    document["records"]["provider_quota_observation_provenance"][0].clone();
                twin["project_id"] = serde_json::json!("0193f000-0000-7000-8000-0000000000ff");
                document["records"]["provider_quota_observation_provenance"]
                    .as_array_mut()
                    .expect("an array")
                    .push(twin);
            }),
            "the export carries two quota provenance records with one identity",
        ),
        (
            "a record carries no source range",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_source_ranges"] =
                    serde_json::json!([]);
            }),
            "a quota provenance record carries no source range",
        ),
        (
            "two source ranges share one identity",
            Box::new(|document: &mut serde_json::Value| {
                let twin =
                    document["records"]["provider_quota_observation_source_ranges"][0].clone();
                document["records"]["provider_quota_observation_source_ranges"]
                    .as_array_mut()
                    .expect("an array")
                    .push(twin);
            }),
            "the export carries two quota source ranges with one identity",
        ),
        (
            "a source range names a parent that is not exported",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_source_ranges"][0]["provenance_id"] =
                    serde_json::json!("0193f000-0000-7000-8000-0000000009aa");
            }),
            "a quota source range names a parent the export does not carry",
        ),
        (
            "a source range reaches outside its parent's envelope",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_source_ranges"][0]["seq_end"] =
                    serde_json::json!(99);
            }),
            "a quota source range falls outside its parent's envelope",
        ),
        (
            "the ranges do not span what the record claims to have read",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_provenance"][0]["item_seq_end"] =
                    serde_json::json!(9);
            }),
            "a quota provenance record's source ranges do not span its envelope",
        ),
        (
            "the ranges overlap",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_provenance"][0]["item_seq_end"] =
                    serde_json::json!(5);
                let parent = document["records"]["provider_quota_observation_source_ranges"][0]
                    ["provenance_id"]
                    .clone();
                let project = document["records"]["provider_quota_observation_source_ranges"][0]
                    ["project_id"]
                    .clone();
                document["records"]["provider_quota_observation_source_ranges"] = serde_json::json!([
                    {"provenance_id": parent, "project_id": project, "ordinal": 0, "seq_start": 2, "seq_end": 4},
                    {"provenance_id": parent, "project_id": project, "ordinal": 1, "seq_start": 3, "seq_end": 5},
                ]);
            }),
            "a quota provenance record's source ranges overlap or are misordered",
        ),
        (
            "the ordinals are not a list from zero",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_observation_provenance"][0]["item_seq_end"] =
                    serde_json::json!(5);
                let parent = document["records"]["provider_quota_observation_source_ranges"][0]
                    ["provenance_id"]
                    .clone();
                let project = document["records"]["provider_quota_observation_source_ranges"][0]
                    ["project_id"]
                    .clone();
                document["records"]["provider_quota_observation_source_ranges"] = serde_json::json!([
                    {"provenance_id": parent, "project_id": project, "ordinal": 0, "seq_start": 2, "seq_end": 3},
                    {"provenance_id": parent, "project_id": project, "ordinal": 2, "seq_start": 4, "seq_end": 5},
                ]);
            }),
            "a quota provenance record's source ranges are not a contiguous ordinal list from zero",
        ),
        (
            "two quota states share one identity",
            Box::new(move |document: &mut serde_json::Value| {
                let at = codex(document);
                let twin = document["records"]["provider_quota_states"][at].clone();
                document["records"]["provider_quota_states"]
                    .as_array_mut()
                    .expect("an array")
                    .push(twin);
            }),
            "the export carries two provider_quota_states rows with one identity",
        ),
        (
            "two windows share one identity",
            Box::new(|document: &mut serde_json::Value| {
                let twin = document["records"]["provider_quota_windows"][0].clone();
                document["records"]["provider_quota_windows"]
                    .as_array_mut()
                    .expect("an array")
                    .push(twin);
            }),
            "the export carries two provider_quota_windows rows with one identity",
        ),
        (
            "a window hangs off no exported state",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_windows"][0]["provider"] =
                    serde_json::json!("no-such-provider");
            }),
            "a provider_quota_windows row names no exported quota state in its project",
        ),
        (
            "a window belongs to another project",
            Box::new(|document: &mut serde_json::Value| {
                document["records"]["provider_quota_windows"][0]["project_id"] =
                    serde_json::json!("0193f000-0000-7000-8000-0000000000ff");
            }),
            "a provider_quota_windows row names no exported quota state in its project",
        ),
    ];

    for (name, tamper, expected) in cases {
        assert_eq!(refused_detail(tamper), expected, "case: {name}");
    }
}

/// A row that is not a runtime observation may agree with a provenance record in
/// every modelled value and still owe it no citation.
#[test]
fn a_non_runtime_row_that_happens_to_agree_is_not_forced_to_cite() {
    let (_fixture, mut document) = chain();
    let record = document["records"]["provider_quota_observation_provenance"][0].clone();
    let at = state_at(&document, "opencode");
    let row = &mut document["records"]["provider_quota_states"][at];
    row["state"] = record["decided_state"].clone();
    row["resets_at"] = record["parsed_resets_at"].clone();
    row["evidence_hash"] = record["evidence_digest"].clone();
    row["credit_minor_units"] = serde_json::Value::Null;
    row["credit_reserve_minor_units"] = serde_json::Value::Null;
    row["credit_currency"] = serde_json::Value::Null;
    rehash(&mut document);

    reparse(&document).expect("a provider report that merely agrees is not a forgery");
}

/// Strip the quota chain from a document, as a generation that never defined it
/// would have written it.
fn without_quota(document: &mut serde_json::Value) {
    for key in [
        "provider_quota_observation_provenance",
        "provider_quota_observation_source_ranges",
        "provider_quota_states",
        "provider_quota_windows",
    ] {
        document["records"]
            .as_object_mut()
            .expect("records are an object")
            .remove(key);
        document["continuity_summary"]["record_counts"]
            .as_object_mut()
            .expect("record counts are an object")
            .remove(key);
    }
}

/// Set the stated database generation and re-read.
fn at_database(
    document: &serde_json::Value,
    generation: i64,
) -> Result<kontor_store::backup::KontorExportV1, kontor_store::backup::BackupError> {
    let mut document = document.clone();
    document["database_schema_version"] = serde_json::json!(generation);
    rehash(&mut document);
    reparse(&document)
}

/// Which generation a document may claim, limb by limb.
///
/// Each limb of the quota chain arrived in a different migration, so one cutoff
/// cannot stand for the others: keying the legacy fence to the provenance
/// generation would let a document silently drop every quota row written
/// between 0048 and 0073.
#[test]
fn a_document_may_only_claim_the_quota_chain_its_generation_had() {
    let (_fixture, current) = chain();

    // A legacy generation that defined no quota field at all. It cannot prove it
    // omitted nothing once the source database had somewhere to omit it from,
    // and `provider_quota_states` is that somewhere from 0048 -- not 0074.
    let mut legacy = current.clone();
    legacy["schema_version"] = serde_json::json!(7);
    without_quota(&mut legacy);
    for refused in [48, 51, 62, 73] {
        assert!(
            matches!(
                at_database(&legacy, refused),
                Err(kontor_store::backup::BackupError::Verification {
                    detail: "the legacy export generation cannot prove quota chain completeness",
                }),
            ),
            "a generation-seven document over database {refused} must not read as complete",
        );
    }
    at_database(&legacy, 47).expect("a genuinely pre-quota generation remains readable");

    // And the converses: a document cannot carry a limb its stated database had
    // no table for.
    let mut no_windows = current.clone();
    no_windows["records"]["provider_quota_windows"] = serde_json::json!([]);
    no_windows["continuity_summary"]["record_counts"]["provider_quota_windows"] =
        serde_json::json!(0);

    for (generation, document, expected) in [
        (
            47,
            &current,
            "the export carries provider_quota_states rows its database generation could not hold",
        ),
        (
            50,
            &current,
            "the export carries provider_quota_windows rows its database generation could not hold",
        ),
        (
            50,
            &no_windows,
            "the export carries provider_quota_states credit balances its database generation could not hold",
        ),
        (
            84,
            &current,
            "the export carries quota provenance its database generation could not hold",
        ),
    ] {
        match at_database(document, generation) {
            Err(kontor_store::backup::BackupError::Verification { detail }) => {
                assert_eq!(detail, expected, "at database generation {generation}");
            }
            other => panic!("database {generation} must be refused, got {other:?}"),
        }
    }

    at_database(&current, 85).expect("the generation that holds the whole chain verifies");
}

/// The child range collection is sealed when its record is written.
///
/// Immutability and the no-delete trigger were not enough on their own: the
/// primary key stops a duplicate ordinal, but a later INSERT at a *fresh*
/// ordinal would have changed the exact source set behind a record whose
/// envelope and evidence digest are both final -- and behind a quota row still
/// pointing at it. The declared count bounds the ordinal, so every slot is
/// filled in the creating transaction and there is no slot left to append into.
#[test]
fn a_source_range_cannot_be_appended_after_its_record_is_written() {
    use kontor_core::repository::CapacityRepository;
    use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

    let fixture = fixture();
    let id = account(&fixture);
    fixture
        .store
        .set_provider_quota_state(&quota_at(
            &fixture,
            id,
            ProviderQuotaKind::Exhausted,
            Some(at("2026-08-09T22:40:00Z")),
            "refusal",
            AggregateRevision::INITIAL,
            ProviderQuotaSource::RuntimeObservation,
            at("2026-08-09T10:05:00Z"),
        ))
        .expect("the runtime observation is stored with its provenance");

    let connection = Connection::open(&fixture.path).expect("a raw connection");
    let (provenance, declared): (String, i64) = connection
        .query_row(
            "SELECT id, source_range_count FROM provider_quota_observation_provenance",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the record exists");
    assert_eq!(declared, 1, "the fixture sealed itself to one range");

    // The append this seal exists to refuse: a brand new ordinal, past the end
    // of the declared set, on a record nothing may change.
    let appended = connection.execute(
        "INSERT INTO provider_quota_observation_source_ranges
             (provenance_id, project_id, ordinal, seq_start, seq_end)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            provenance,
            fixture.project.to_string(),
            declared,
            9_i64,
            9_i64
        ],
    );
    assert!(
        appended
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("declared set")),
        "a later append must be refused, got {appended:?}",
    );

    // And the two doors that were already shut stay shut.
    assert!(
        connection
            .execute(
                "INSERT INTO provider_quota_observation_source_ranges
                     (provenance_id, project_id, ordinal, seq_start, seq_end)
                 VALUES (?1, ?2, 0, 9, 9)",
                rusqlite::params![provenance, fixture.project.to_string()],
            )
            .is_err(),
        "a taken ordinal is refused by the primary key",
    );
    assert!(
        connection
            .execute(
                "DELETE FROM provider_quota_observation_source_ranges WHERE provenance_id = ?1",
                rusqlite::params![provenance],
            )
            .is_err(),
        "a range cannot be withdrawn",
    );

    // The readback still presents exactly the sealed set.
    let record = fixture
        .store
        .get_quota_observation_provenance(
            fixture.project,
            kontor_core::id::QuotaObservationProvenanceId::parse(&provenance).expect("an id"),
        )
        .expect("the readback succeeds")
        .expect("the record is there");
    assert_eq!(record.record.source_sequences, vec![(2, 2)]);
}
