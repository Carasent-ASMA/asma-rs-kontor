//! Crash recovery for commands: intent, claim, dispatch, acknowledgement and
//! confirmation.
//!
//! Every test here reopens the database between protocol steps, because the
//! question the suite exists to answer is not "does the state machine work" but
//! "what does a process that lost its memory do next".
//!
//! The mutants this suite exists to kill:
//!
//! * treating `dispatch_pending`, an expired lease or `confirmation_unknown` as
//!   launchable, so a restart fires a second native session;
//! * claiming the outbox with a read instead of a write, so two dispatchers hold
//!   the same work;
//! * making a native call before the correlation is durable, so the lookup that
//!   recovery depends on has no key;
//! * recording a confirmation without evidence, or treating an acknowledgement
//!   as one;
//! * letting a duplicate or out-of-order transition move a receipt backwards.

use std::collections::BTreeMap;

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId,
    IdempotencyKey, ProjectId, Timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind, CommandReceiptState, NoEffectEvidence};
use kontor_core::repository::{
    CommandRepository, NewCommandIntent, NewLocalCommand, RunRepository,
};
use kontor_core::state::{DesiredRunState, NativeRuntimeIdentity};
use kontor_store::{CommandRecovery, CommandTransition, SqliteStore};
use rusqlite::Connection;
use tempfile::TempDir;

/// The crash matrix, as data. Each row is a durable boundary a dispatcher can
/// die at, and what a restart is allowed to do about it.
const CRASH_POINTS: &str = include_str!("fixtures/runtime/command_crash_points.json");

const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
const RUN: &str = "0193f000-0000-7000-8000-000000000040";

/// A project → task → team run → agent run chain with one native binding,
/// inserted with direct SQL so the protocol under test is the only thing the
/// suite exercises.
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
        '0193f000-0000-7000-8000-000000000035', 'maker.primary', 'queued', 'no_intent', \
        'unknown', 'pending_confirmation', 1, '2026-08-09T10:00:00Z'); \
INSERT INTO runtime_bindings (id, project_id, agent_run_id, runtime_kind, host, generation, \
        native_id, bound_at) \
VALUES ('0193f000-0000-7000-8000-000000000050', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1, 'session-1', \
        '2026-08-09T10:00:00Z');";

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project: ProjectId,
}

impl Fixture {
    /// Reopen the file through a brand-new connection, exactly as a restarted
    /// daemon does. Nothing survives except what was committed.
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
    let connection = Connection::open(&path).expect("a raw connection opens");
    connection
        .execute_batch(FIXTURE_SQL)
        .expect("the fixture inserts");
    Fixture {
        _directory: directory,
        path,
        store,
        project: ProjectId::parse(PROJECT).expect("a canonical id"),
    }
}

fn unbound_fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");
    let connection = Connection::open(&path).expect("a raw connection opens");
    let without_binding = FIXTURE_SQL
        .split_once("INSERT INTO runtime_bindings")
        .expect("the fixture has a binding suffix")
        .0;
    connection
        .execute_batch(without_binding)
        .expect("the unbound fixture inserts");
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

fn identity() -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: kontor_core::id::RuntimeKindKey::parse("generic.runtime")
            .expect("a valid runtime key"),
        host: kontor_core::id::ExternalName::parse("host-1").expect("a valid host"),
        generation: 1,
        native_id: external("session-1"),
    }
}

#[test]
fn a_local_command_is_never_dispatchable_and_completes_only_after_success() {
    let fixture = fixture();
    let key = IdempotencyKey::parse("local-command-1").expect("a valid key");
    let receipt = fixture
        .store
        .record_local_command(&NewLocalCommand {
            project_id: fixture.project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: key.clone(),
            kind: CommandKind::EnsureProject,
            target: AggregateRef::Project {
                project_id: fixture.project,
            },
            target_revision: AggregateRevision::INITIAL,
            intent: document("local-command"),
            created_at: now(),
        })
        .expect("the local intent is recorded");
    assert_eq!(receipt.state, CommandReceiptState::IntentPersisted);
    assert_eq!(census(&fixture)["command_outbox"], 0);
    assert!(
        fixture
            .store
            .unsettled_receipts()
            .expect("recovery inventory is readable")
            .is_empty(),
        "a local operation is not work for the dispatcher"
    );

    let fixture = fixture.restart();
    let completed = fixture
        .store
        .complete_local_command(&key, now())
        .expect("successful application completes the receipt")
        .expect("the key names a receipt");
    assert_eq!(completed.state, CommandReceiptState::Confirmed);
    assert_eq!(
        completed.result_ref,
        Some(external(completed.intent.hash().as_str()))
    );
    assert_eq!(
        fixture
            .store
            .receipt_history(fixture.project, completed.id)
            .expect("history is readable")
            .iter()
            .map(|step| step.state)
            .collect::<Vec<_>>(),
        vec![
            CommandReceiptState::IntentPersisted,
            CommandReceiptState::Confirmed
        ]
    );
    assert_eq!(census(&fixture)["command_outbox"], 0);
}

#[test]
fn the_local_completion_boundary_ignores_a_dispatch_receipt() {
    let fixture = fixture();
    let (_, intent) = launch_intent(&fixture, "dispatch-is-not-local", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the dispatch intent is recorded");

    let completed = fixture
        .store
        .complete_local_command(&intent.idempotency_key, now())
        .expect("a dispatch receipt is outside the local boundary");
    assert!(completed.is_none());
    assert_eq!(
        fixture
            .store
            .get_receipt_by_key(&intent.idempotency_key)
            .expect("the receipt remains readable")
            .expect("the receipt exists")
            .state,
        CommandReceiptState::IntentPersisted
    );
}

#[test]
fn startup_confirms_a_legacy_launch_only_from_its_durable_binding() {
    let fixture = fixture();
    let (_, intent) = launch_intent(&fixture, "legacy-bound-launch", "launch");
    let receipt = fixture
        .store
        .record_intent(&intent)
        .expect("the old launch intent is recorded");
    assert_eq!(receipt.state, CommandReceiptState::IntentPersisted);

    let fixture = fixture.restart();
    let report = fixture
        .store
        .reconcile_legacy_launch_receipts(now())
        .expect("the binding is valid launch evidence");
    assert_eq!(report.confirmed, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.pending, 0);
    let confirmed = fixture
        .store
        .get_receipt_by_key(&intent.idempotency_key)
        .expect("the receipt is readable")
        .expect("the receipt exists");
    assert_eq!(confirmed.state, CommandReceiptState::Confirmed);
    assert_eq!(confirmed.native_identity, Some(identity()));
    assert!(
        confirmed.result_ref.is_some(),
        "the binding id is cited as evidence"
    );
    assert!(
        fixture
            .store
            .unsettled_receipts()
            .expect("the inventory is readable")
            .is_empty()
    );
}

#[test]
fn startup_fails_a_legacy_launch_only_from_terminal_run_evidence() {
    let fixture = unbound_fixture();
    let (_, intent) = launch_intent(&fixture, "legacy-terminal-launch", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the old launch intent is recorded");

    // Model a historical launch whose runtime produced a terminal observation
    // but never left a binding behind. The receipt may be failed because the
    // immutable run evidence proves that this launch can no longer take effect.
    let evidence = document("terminal-runtime-observation");
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                  native_id, native_event_id, native_sequence, observed_state, contact,
                  freshness, audit_ref, payload, payload_hash, observed_at, recorded_at)
             VALUES (?1, 'runtime_observation', ?2, 'generic.runtime', 'host-1', 1,
                     'session-1', 'terminal-event', 2, 'failed', 'reachable', 'fresh',
                     'runtime://terminal-event', ?3, ?4, ?5, ?5)",
            rusqlite::params![
                PROJECT,
                RUN,
                evidence.json(),
                evidence.hash().as_str(),
                now().to_string()
            ],
        )
        .expect("terminal evidence is durable");
    let cursor = connection.last_insert_rowid();
    connection
        .execute(
            "UPDATE agent_runs
             SET lifecycle = 'failed', observed_state = 'failed', derived_state = 'terminal',
                 terminal_outcome = 'failed', terminal_source_kind = 'runtime_observation',
                 terminal_event_cursor = ?1, terminal_evidence_hash = ?2, closed_at = ?3,
                 revision = revision + 1
             WHERE project_id = ?4 AND id = ?5",
            rusqlite::params![
                cursor,
                evidence.hash().as_str(),
                now().to_string(),
                PROJECT,
                RUN
            ],
        )
        .expect("the run closes from that evidence");
    drop(connection);

    let fixture = fixture.restart();
    let report = fixture
        .store
        .reconcile_legacy_launch_receipts(now())
        .expect("terminal run evidence is valid failure evidence");
    assert_eq!(report.confirmed, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.pending, 0);
    let failed = fixture
        .store
        .get_receipt_by_key(&intent.idempotency_key)
        .expect("the receipt is readable")
        .expect("the receipt exists");
    assert_eq!(failed.state, CommandReceiptState::Failed);
    assert_eq!(failed.result_ref, Some(external(evidence.hash().as_str())));
}

/// Count every row a command touches, through an independent connection.
fn census(fixture: &Fixture) -> BTreeMap<&'static str, i64> {
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    [
        "command_receipts",
        "command_targets",
        "command_outbox",
        "command_receipt_transitions",
        "runtime_events",
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

fn launch_intent(
    fixture: &Fixture,
    key: &str,
    marker: &str,
) -> (CommandReceiptId, NewCommandIntent) {
    let receipt_id = CommandReceiptId::generate();
    let run = kontor_core::id::AgentRunId::parse(RUN).expect("a canonical id");
    let revision = fixture
        .store
        .get_agent_run(fixture.project, run)
        .expect("the read succeeds")
        .expect("the run exists")
        .revision;
    (
        receipt_id,
        NewCommandIntent {
            project_id: fixture.project,
            receipt_id,
            idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
            kind: CommandKind::LaunchRun,
            target: AggregateRef::AgentRun { agent_run_id: run },
            target_revision: revision,
            intent: document(marker),
            payload: document(&format!("{marker}-payload")),
            desired: Some(DesiredRunState::RunRequested),
            not_before: now(),
            created_at: now(),
        },
    )
}

fn step(
    fixture: &Fixture,
    receipt: CommandReceiptId,
    to: CommandReceiptState,
) -> CommandTransition {
    CommandTransition {
        project_id: fixture.project,
        receipt_id: receipt,
        to,
        correlation: None,
        native_identity: None,
        evidence_ref: None,
        no_effect: None,
        occurred_at: now(),
    }
}

/// Drive one receipt to a state, through the protocol, one durable step at a
/// time.
fn drive_to(fixture: &Fixture, receipt: CommandReceiptId, state: CommandReceiptState) {
    use CommandReceiptState as S;
    if state == S::IntentPersisted {
        return;
    }
    let claims = fixture
        .store
        .claim_due(fixture.project, now(), 10)
        .expect("the outbox is claimable");
    assert!(
        claims.iter().any(|claim| claim.receipt_id == receipt),
        "the due entry is claimed"
    );
    if state == S::DispatchPending {
        return;
    }
    fixture
        .store
        .apply_command_transition(&CommandTransition {
            native_identity: Some(identity()),
            ..step(fixture, receipt, S::Dispatched)
        })
        .expect("the dispatch is recorded");
    match state {
        S::Dispatched => {}
        S::ConfirmationUnknown => {
            fixture
                .store
                .apply_command_transition(&step(fixture, receipt, S::ConfirmationUnknown))
                .expect("the result is unknown");
        }
        S::Acknowledged => {
            fixture
                .store
                .apply_command_transition(&step(fixture, receipt, S::Acknowledged))
                .expect("the target acknowledges");
        }
        S::Confirmed => {
            fixture
                .store
                .apply_command_transition(&step(fixture, receipt, S::Acknowledged))
                .expect("the target acknowledges");
            fixture
                .store
                .apply_command_transition(&CommandTransition {
                    evidence_ref: Some(external("native-confirmation-1")),
                    ..step(fixture, receipt, S::Confirmed)
                })
                .expect("the effect is confirmed");
        }
        other => panic!("{other:?} is not a crash point this suite drives to"),
    }
}

// ---------------------------------------------------------------------------
// Intent
// ---------------------------------------------------------------------------

#[test]
fn intent_outbox_desired_and_event_commit_atomically() {
    let fixture = fixture();
    let before = census(&fixture);
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    let receipt = fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    assert_eq!(receipt.state, CommandReceiptState::IntentPersisted);

    let after = census(&fixture);
    for table in [
        "command_receipts",
        "command_targets",
        "command_outbox",
        "command_receipt_transitions",
        "runtime_events",
    ] {
        assert_eq!(
            after[table],
            before[table] + 1,
            "`{table}` must gain exactly one row from one intent"
        );
    }

    // The durable history starts with the intent itself, so a restart never has
    // to guess what the earliest promise was.
    let history = fixture
        .store
        .receipt_history(fixture.project, receipt_id)
        .expect("the history is readable");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].sequence, 1);
    assert_eq!(history[0].state, CommandReceiptState::IntentPersisted);
    assert!(
        history[0].correlation.is_none(),
        "nothing has been dispatched yet, so there is no correlation to claim"
    );

    // Desired state moved in the same transaction.
    let run = kontor_core::id::AgentRunId::parse(RUN).expect("a canonical id");
    assert_eq!(
        fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .projection
            .desired,
        DesiredRunState::RunRequested
    );

    // And all of it survives a restart.
    let fixture = fixture.restart();
    assert_eq!(
        fixture
            .store
            .get_receipt(fixture.project, receipt_id)
            .expect("the read succeeds")
            .expect("the receipt exists")
            .state,
        CommandReceiptState::IntentPersisted
    );
    assert_eq!(census(&fixture), after);
}

#[test]
fn intent_failure_rolls_back_every_effect() {
    let fixture = fixture();
    let before = census(&fixture);

    // A revision the target does not have: the compare-and-swap fails, and with
    // it the receipt, the target row, the outbox entry, the first transition and
    // the intent event.
    let (_, intent) = launch_intent(&fixture, "launch-1", "launch");
    let stale = NewCommandIntent {
        target_revision: AggregateRevision::parse(99).expect("a valid revision"),
        ..intent
    };
    assert!(fixture.store.record_intent(&stale).is_err());
    assert_eq!(
        census(&fixture),
        before,
        "a refused intent leaves zero partial rows"
    );

    let run = kontor_core::id::AgentRunId::parse(RUN).expect("a canonical id");
    assert_eq!(
        fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .projection
            .desired,
        DesiredRunState::NoIntent,
        "a refused intent never moves desired state"
    );

    let fixture = fixture.restart();
    assert_eq!(census(&fixture), before, "and nothing appears on reopen");
}

#[test]
fn idempotency_replay_returns_original_receipt_and_cursor() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    let first = fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    let after = census(&fixture);

    let intent_cursor = |fixture: &Fixture| -> i64 {
        let connection = Connection::open(&fixture.path).expect("a raw connection opens");
        connection
            .query_row(
                "SELECT cursor FROM runtime_events WHERE event_kind = 'command_intent'
                   AND command_receipt_id = ?1",
                rusqlite::params![receipt_id.to_string()],
                |row| row.get(0),
            )
            .expect("the intent event exists")
    };
    let cursor = intent_cursor(&fixture);

    // A byte-identical replay across a restart returns the original receipt and
    // the original control-plane cursor, and enqueues nothing.
    let fixture = fixture.restart();
    let replay = fixture
        .store
        .record_intent(&intent)
        .expect("a replay returns the original");
    assert_eq!(replay.id, first.id);
    assert_eq!(replay.state, first.state);
    assert_eq!(
        intent_cursor(&fixture),
        cursor,
        "the cursor is the original"
    );
    assert_eq!(census(&fixture), after, "a replay writes nothing");

    // The same key with a different intent is a different command wearing a used
    // key, and fails without touching anything.
    let (_, other) = launch_intent(&fixture, "launch-1", "cancel");
    assert!(fixture.store.record_intent(&other).is_err());
    assert_eq!(census(&fixture), after);
}

// ---------------------------------------------------------------------------
// Crash recovery
// ---------------------------------------------------------------------------

#[test]
fn every_command_crash_point_recovers_without_second_native_launch() {
    let matrix: serde_json::Value =
        serde_json::from_str(CRASH_POINTS).expect("the crash matrix parses");
    let points = matrix["crash_points"]
        .as_array()
        .expect("the matrix lists crash points");
    assert_eq!(points.len(), 9, "every durable boundary is covered");

    for point in points {
        let name = point["name"].as_str().expect("a name");
        let reached =
            CommandReceiptState::parse(point["reached_state"].as_str().expect("a reached state"))
                .expect("a known receipt state");
        let native_launched = point["native_effect"].as_str() == Some("launched");
        let may_launch = point["authorizes_launch"].as_bool().expect("a verdict");

        let fixture = fixture();
        let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
        fixture
            .store
            .record_intent(&intent)
            .expect("the intent is recorded");
        drive_to(&fixture, receipt_id, reached);

        // The process dies here and comes back with no memory at all.
        let fixture = fixture.restart();
        let recovery = fixture
            .store
            .classify_command_recovery(fixture.project, receipt_id)
            .expect("recovery is classifiable");

        assert_eq!(
            recovery.authorizes_launch(),
            may_launch,
            "crash point `{name}` reached the wrong recovery verdict"
        );

        // A dispatcher that obeys the verdict launches at most once in total,
        // however far the original attempt got.
        let launches = usize::from(native_launched) + usize::from(recovery.authorizes_launch());
        assert!(
            launches <= 1,
            "crash point `{name}` would produce {launches} native launches"
        );

        if !may_launch && !reached.is_terminal() {
            assert!(
                recovery.correlation().is_some(),
                "crash point `{name}` must leave a correlation to look the command up by"
            );
        }
        assert_eq!(
            fixture
                .store
                .get_receipt(fixture.project, receipt_id)
                .expect("the read succeeds")
                .expect("the receipt exists")
                .state,
            reached,
            "crash point `{name}` must reopen in the state it committed"
        );
    }
}

#[test]
fn ambiguous_dispatch_never_becomes_fresh_launch() {
    for state in [
        CommandReceiptState::DispatchPending,
        CommandReceiptState::Dispatched,
        CommandReceiptState::Acknowledged,
        CommandReceiptState::ConfirmationUnknown,
    ] {
        let fixture = fixture();
        let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
        fixture
            .store
            .record_intent(&intent)
            .expect("the intent is recorded");
        drive_to(&fixture, receipt_id, state);

        // Restart twice: neither a fresh process nor a long-idle one earns the
        // right to send the command again.
        let fixture = fixture.restart().restart();
        let recovery = fixture
            .store
            .classify_command_recovery(fixture.project, receipt_id)
            .expect("recovery is classifiable");
        assert!(
            !recovery.authorizes_launch(),
            "{state:?} must never authorize a fresh launch"
        );
        assert!(matches!(
            recovery,
            CommandRecovery::AmbiguousOrLaunched { .. }
        ));
        assert!(
            recovery.correlation().is_some(),
            "the correlation a lookup needs was persisted before the native call"
        );

        // And the receipt itself refuses to go back to the start.
        let attempt = fixture.store.apply_command_transition(&step(
            &fixture,
            receipt_id,
            CommandReceiptState::DispatchPending,
        ));
        if state == CommandReceiptState::DispatchPending {
            let repeated = attempt.expect("a repeat of the current state is idempotent");
            assert!(!repeated.appended, "a repeat appends no second transition");
        } else {
            assert!(
                attempt.is_err(),
                "{state:?} must not be re-dispatched without evidence"
            );
        }
    }
}

#[test]
fn lost_ack_is_resolved_by_persisted_correlation() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    drive_to(&fixture, receipt_id, CommandReceiptState::Dispatched);
    let correlation = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists")
        .correlation
        .expect("the dispatch persisted a correlation");

    // The acknowledgement never arrives and the process dies. On restart the
    // command is ambiguous, and the correlation is the key to resolving it.
    let fixture = fixture.restart();
    let recovery = fixture
        .store
        .classify_command_recovery(fixture.project, receipt_id)
        .expect("recovery is classifiable");
    assert!(!recovery.authorizes_launch());
    assert_eq!(recovery.correlation(), Some(&correlation));

    // The lookup finds the session, so the *original* receipt is confirmed and
    // bound — no replacement command is ever minted.
    let confirmed = fixture
        .store
        .apply_command_transition(&CommandTransition {
            native_identity: Some(identity()),
            evidence_ref: Some(external("native-confirmation-1")),
            ..step(&fixture, receipt_id, CommandReceiptState::Confirmed)
        })
        .expect("the lookup confirms the original receipt");
    assert_eq!(confirmed.receipt.id, receipt_id);
    assert_eq!(confirmed.receipt.correlation, Some(correlation));
    assert_eq!(confirmed.receipt.native_identity, Some(identity()));
    assert_eq!(
        census(&fixture)["command_receipts"],
        1,
        "recovery never mints a replacement command"
    );
}

#[test]
fn confirmed_receipt_is_restart_terminal() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    drive_to(&fixture, receipt_id, CommandReceiptState::Confirmed);

    let fixture = fixture.restart();
    let recovery = fixture
        .store
        .classify_command_recovery(fixture.project, receipt_id)
        .expect("recovery is classifiable");
    assert!(!recovery.authorizes_launch());
    assert!(matches!(
        recovery,
        CommandRecovery::Settled {
            state: CommandReceiptState::Confirmed,
            ..
        }
    ));

    // A settled receipt never moves again, in Rust or in SQL.
    for state in [
        CommandReceiptState::DispatchPending,
        CommandReceiptState::Dispatched,
        CommandReceiptState::Failed,
    ] {
        assert!(
            fixture
                .store
                .apply_command_transition(&CommandTransition {
                    evidence_ref: Some(external("evidence-1")),
                    ..step(&fixture, receipt_id, state)
                })
                .is_err(),
            "a confirmed receipt must not move to {state:?}"
        );
    }
    let repeated = fixture
        .store
        .apply_command_transition(&CommandTransition {
            evidence_ref: Some(external("native-confirmation-1")),
            ..step(&fixture, receipt_id, CommandReceiptState::Confirmed)
        })
        .expect("repeating the settled state is idempotent");
    assert!(!repeated.appended);
}

// ---------------------------------------------------------------------------
// Acknowledgement, confirmation and history
// ---------------------------------------------------------------------------

#[test]
fn acknowledgement_is_nonterminal_and_confirmation_needs_evidence() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    drive_to(&fixture, receipt_id, CommandReceiptState::Dispatched);

    let acknowledged = fixture
        .store
        .apply_command_transition(&step(
            &fixture,
            receipt_id,
            CommandReceiptState::Acknowledged,
        ))
        .expect("the target acknowledges");
    assert!(
        !acknowledged.receipt.state.is_terminal(),
        "an acknowledgement settles nothing"
    );
    assert!(
        acknowledged.receipt.result_ref.is_none(),
        "an acknowledgement cites no result"
    );

    // An acknowledgement that claims evidence is a confirmation in disguise.
    assert!(
        fixture
            .store
            .apply_command_transition(&CommandTransition {
                evidence_ref: Some(external("native-confirmation-1")),
                ..step(&fixture, receipt_id, CommandReceiptState::Acknowledged)
            })
            .is_err(),
        "an acknowledgement carries no evidence"
    );

    // Confirmation without a reference to the proof is refused, and changes
    // nothing.
    let before = census(&fixture);
    assert!(
        fixture
            .store
            .apply_command_transition(&step(&fixture, receipt_id, CommandReceiptState::Confirmed))
            .is_err(),
        "confirmation requires evidence"
    );
    assert!(
        fixture
            .store
            .apply_command_transition(&step(&fixture, receipt_id, CommandReceiptState::Failed))
            .is_err(),
        "failure requires evidence"
    );
    assert_eq!(census(&fixture), before);
    assert_eq!(
        fixture
            .store
            .get_receipt(fixture.project, receipt_id)
            .expect("the read succeeds")
            .expect("the receipt exists")
            .state,
        CommandReceiptState::Acknowledged,
        "a refused confirmation leaves the receipt where it was"
    );

    let confirmed = fixture
        .store
        .apply_command_transition(&CommandTransition {
            evidence_ref: Some(external("native-confirmation-1")),
            ..step(&fixture, receipt_id, CommandReceiptState::Confirmed)
        })
        .expect("evidence confirms");
    assert!(confirmed.receipt.state.is_terminal());
    assert_eq!(
        confirmed.receipt.result_ref,
        Some(external("native-confirmation-1"))
    );

    // Acknowledgement and confirmation are two distinct durable rows.
    let history = fixture
        .store
        .receipt_history(fixture.project, receipt_id)
        .expect("the history is readable");
    let acknowledgements: Vec<_> = history
        .iter()
        .filter(|entry| entry.state == CommandReceiptState::Acknowledged)
        .collect();
    let confirmations: Vec<_> = history
        .iter()
        .filter(|entry| entry.state == CommandReceiptState::Confirmed)
        .collect();
    assert_eq!(acknowledgements.len(), 1);
    assert_eq!(confirmations.len(), 1);
    assert!(acknowledgements[0].evidence_ref.is_none());
    assert!(confirmations[0].evidence_ref.is_some());
    assert!(acknowledgements[0].sequence < confirmations[0].sequence);
}

#[test]
fn receipt_transition_history_survives_reopen() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");

    // One step per reopen: the history must be built from what is on disk, not
    // from anything a single process happened to remember.
    let mut fixture = fixture;
    for state in [
        CommandReceiptState::DispatchPending,
        CommandReceiptState::Dispatched,
        CommandReceiptState::Acknowledged,
    ] {
        fixture = fixture.restart();
        drive_one(&fixture, receipt_id, state);
    }
    let fixture = fixture.restart();
    fixture
        .store
        .apply_command_transition(&CommandTransition {
            evidence_ref: Some(external("native-confirmation-1")),
            ..step(&fixture, receipt_id, CommandReceiptState::Confirmed)
        })
        .expect("the effect is confirmed");

    let fixture = fixture.restart();
    let history = fixture
        .store
        .receipt_history(fixture.project, receipt_id)
        .expect("the history is readable");
    let states: Vec<CommandReceiptState> = history.iter().map(|entry| entry.state).collect();
    assert_eq!(
        states,
        vec![
            CommandReceiptState::IntentPersisted,
            CommandReceiptState::DispatchPending,
            CommandReceiptState::Dispatched,
            CommandReceiptState::Acknowledged,
            CommandReceiptState::Confirmed,
        ]
    );
    let sequences: Vec<u32> = history.iter().map(|entry| entry.sequence).collect();
    assert_eq!(sequences, vec![1, 2, 3, 4, 5]);

    // The correlation is on disk from the claim onwards, which is what makes
    // every later restart able to ask the runtime about this exact command.
    assert!(history[0].correlation.is_none());
    let correlation = history[1]
        .correlation
        .clone()
        .expect("the claim persisted a correlation");
    assert!(
        history[1..]
            .iter()
            .all(|entry| entry.correlation.as_ref() == Some(&correlation)),
        "every later step reuses the original correlation"
    );

    // History is append-only against direct SQL too.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    assert!(
        connection
            .execute(
                "UPDATE command_receipt_transitions SET state = 'failed' WHERE sequence = 1",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "DELETE FROM command_receipt_transitions WHERE sequence = 1",
                []
            )
            .is_err()
    );
}

/// Apply exactly one protocol step.
fn drive_one(fixture: &Fixture, receipt: CommandReceiptId, state: CommandReceiptState) {
    if state == CommandReceiptState::DispatchPending {
        let claims = fixture
            .store
            .claim_due(fixture.project, now(), 10)
            .expect("the outbox is claimable");
        assert!(claims.iter().any(|claim| claim.receipt_id == receipt));
        return;
    }
    fixture
        .store
        .apply_command_transition(&step(fixture, receipt, state))
        .unwrap_or_else(|error| panic!("the {state:?} step applies: {error}"));
}

#[test]
fn duplicate_or_out_of_order_receipt_transition_does_not_regress() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    drive_to(&fixture, receipt_id, CommandReceiptState::Acknowledged);
    let settled = census(&fixture);
    let receipt = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists");

    // A duplicate of the current state is the caller resuming, and the durable
    // answer is the one already recorded.
    let duplicate = fixture
        .store
        .apply_command_transition(&step(
            &fixture,
            receipt_id,
            CommandReceiptState::Acknowledged,
        ))
        .expect("a duplicate is not an error");
    assert!(!duplicate.appended);
    assert_eq!(duplicate.receipt.state, receipt.state);
    assert_eq!(duplicate.receipt.attempts, receipt.attempts);
    assert_eq!(census(&fixture), settled);

    // Out-of-order steps are refused rather than applied backwards.
    for state in [
        CommandReceiptState::IntentPersisted,
        CommandReceiptState::DispatchPending,
        CommandReceiptState::Dispatched,
    ] {
        assert!(
            fixture
                .store
                .apply_command_transition(&step(&fixture, receipt_id, state))
                .is_err(),
            "an acknowledged receipt must not move back to {state:?}"
        );
    }
    assert_eq!(census(&fixture), settled);
    let after = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists");
    assert_eq!(after.state, receipt.state);
    assert_eq!(after.attempts, receipt.attempts);
    assert_eq!(after.correlation, receipt.correlation);
}

#[test]
fn two_claimers_never_receive_the_same_work() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");

    // A second, independent connection to the same file — a second dispatcher.
    let second = SqliteStore::open(&fixture.path).expect("a second store opens");
    let first_claims = fixture
        .store
        .claim_due(fixture.project, now(), 10)
        .expect("the first claim succeeds");
    let second_claims = second
        .claim_due(fixture.project, now(), 10)
        .expect("the second claim succeeds");

    assert_eq!(first_claims.len(), 1);
    assert!(
        second_claims.is_empty(),
        "claiming is a write: the same work cannot be handed out twice"
    );
    assert_eq!(first_claims[0].receipt_id, receipt_id);

    // The claim advanced the receipt and persisted the correlation, both of them
    // durably, before any native call could have happened.
    let receipt = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists");
    assert_eq!(receipt.state, CommandReceiptState::DispatchPending);
    assert_eq!(
        receipt.correlation,
        Some(first_claims[0].correlation.clone())
    );

    // The token is immutable: a later attempt is recognizably the same command.
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    assert!(
        connection
            .execute(
                "UPDATE command_outbox SET claim_token = 'other' WHERE receipt_id = ?1",
                rusqlite::params![receipt_id.to_string()],
            )
            .is_err(),
        "a claim token is minted once and reused"
    );
}

#[test]
fn a_retry_needs_proof_of_no_effect_and_reuses_the_original_command() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    drive_to(
        &fixture,
        receipt_id,
        CommandReceiptState::ConfirmationUnknown,
    );
    let correlation = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists")
        .correlation
        .expect("the dispatch persisted a correlation");

    let fixture = fixture.restart();
    // A blind retry, and a retry proved against somebody else's correlation, are
    // both refused.
    assert!(
        fixture
            .store
            .apply_command_transition(&step(
                &fixture,
                receipt_id,
                CommandReceiptState::DispatchPending
            ))
            .is_err()
    );
    assert!(
        fixture
            .store
            .apply_command_transition(&CommandTransition {
                no_effect: Some(NoEffectEvidence {
                    correlation: external("some-other-correlation"),
                    searched_identity: Some(identity()),
                    reconciled_at: now(),
                    evidence_hash: ContentHash::of(b"lookup"),
                }),
                ..step(&fixture, receipt_id, CommandReceiptState::DispatchPending)
            })
            .is_err()
    );

    let retried = fixture
        .store
        .apply_command_transition(&CommandTransition {
            no_effect: Some(NoEffectEvidence {
                correlation: correlation.clone(),
                searched_identity: Some(identity()),
                reconciled_at: now(),
                evidence_hash: ContentHash::of(b"lookup"),
            }),
            ..step(&fixture, receipt_id, CommandReceiptState::DispatchPending)
        })
        .expect("proof of no effect authorizes one retry");
    assert_eq!(
        retried.receipt.correlation,
        Some(correlation),
        "a retry reuses the original command identity, it does not mint a new one"
    );
    assert_eq!(
        census(&fixture)["command_receipts"],
        1,
        "a retry is the same command, not a replacement"
    );

    let redispatched = fixture
        .store
        .apply_command_transition(&step(&fixture, receipt_id, CommandReceiptState::Dispatched))
        .expect("the retry dispatches");
    assert_eq!(
        redispatched.receipt.attempts, 2,
        "attempts are counted durably"
    );
}

#[test]
fn the_correlation_is_the_claim_token_and_nothing_else() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");

    // Before a claim there is no token, so there is no correlation a caller may
    // assert either: a dispatch-bearing state has to be reached through the claim
    // that mints one.
    assert!(
        fixture
            .store
            .apply_command_transition(&CommandTransition {
                correlation: Some(external("correlation-nobody-claimed")),
                ..step(&fixture, receipt_id, CommandReceiptState::DispatchPending)
            })
            .is_err(),
        "a correlation with no outbox claim behind it is not persistable"
    );

    let correlation = fixture
        .store
        .claim_due(fixture.project, now(), 10)
        .expect("the outbox is claimable")
        .into_iter()
        .find(|claim| claim.receipt_id == receipt_id)
        .expect("the due entry is claimed")
        .correlation;

    // The outbox claim token is immutable, so the correlation recorded against it
    // must be too. A caller that "corrects" it would leave recovery asking the
    // runtime about a command that was never sent under that name — and answering
    // "no such command" for one that already ran.
    let fixture = fixture.restart();
    assert!(
        fixture
            .store
            .apply_command_transition(&CommandTransition {
                correlation: Some(external("correlation-a-restart-invented")),
                ..step(&fixture, receipt_id, CommandReceiptState::Dispatched)
            })
            .is_err(),
        "the persisted correlation is never replaced"
    );
    let receipt = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists");
    assert_eq!(
        receipt.correlation,
        Some(correlation.clone()),
        "the refused write left the durable correlation exactly as it was"
    );
    assert_eq!(
        receipt.state,
        CommandReceiptState::DispatchPending,
        "and moved the receipt nowhere"
    );

    // Repeating the token it already holds is not a change, and is allowed.
    fixture
        .store
        .apply_command_transition(&CommandTransition {
            correlation: Some(correlation.clone()),
            native_identity: Some(identity()),
            ..step(&fixture, receipt_id, CommandReceiptState::Dispatched)
        })
        .expect("the dispatch is recorded under the token it was claimed with");

    // Which is what a restart reads: receipt, history and claim are three records
    // of one correlation, and recovery uses them interchangeably.
    let fixture = fixture.restart();
    let history = fixture
        .store
        .receipt_history(fixture.project, receipt_id)
        .expect("the history reads");
    assert!(
        history
            .iter()
            .skip(1)
            .all(|entry| entry.correlation.as_ref() == Some(&correlation)),
        "every dispatch-bearing history row names the claim token"
    );
    let claimed: String = Connection::open(&fixture.path)
        .expect("a raw connection opens")
        .query_row(
            "SELECT claim_token FROM command_outbox WHERE receipt_id = ?1",
            rusqlite::params![receipt_id.to_string()],
            |row| row.get(0),
        )
        .expect("the claim token is readable");
    assert_eq!(
        claimed,
        correlation.as_str(),
        "the outbox token and the receipt correlation are the same fact"
    );
    assert_eq!(
        fixture
            .store
            .classify_command_recovery(fixture.project, receipt_id)
            .expect("recovery classifies")
            .correlation(),
        Some(&correlation),
        "so recovery asks the runtime about the command that was actually sent"
    );
}

#[test]
fn a_repeat_of_the_current_state_still_has_to_prove_its_correlation() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");

    let unchanged = |label: &str| {
        let receipt = fixture
            .store
            .get_receipt(fixture.project, receipt_id)
            .expect("the read succeeds")
            .expect("the receipt exists");
        let history = fixture
            .store
            .receipt_history(fixture.project, receipt_id)
            .expect("the history reads");
        (label.to_owned(), census(&fixture), receipt, history)
    };

    // Asking for the state the receipt is already in is the resuming caller's
    // question, and it is *not* exempt from the correlation rule: a repeat still
    // asserts which token this command was sent under. Answered with the original
    // receipt, an invented one would be read back by recovery as agreement — and
    // a lookup under a name the runtime never heard answers "no such command" for
    // one that may already have run.
    let before = unchanged("before any claim");
    let refused = fixture
        .store
        .apply_command_transition(&CommandTransition {
            correlation: Some(external("correlation-nobody-claimed")),
            ..step(&fixture, receipt_id, CommandReceiptState::IntentPersisted)
        })
        .expect_err("a repeat carrying an unclaimed correlation is refused");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict { .. }
        ),
        "the disagreement is a conflict, not a quiet success: {refused:?}"
    );
    assert_eq!(
        unchanged("after the refusal"),
        (
            "after the refusal".to_owned(),
            before.1.clone(),
            before.2.clone(),
            before.3.clone()
        ),
        "a refused repeat writes no row, appends no transition and moves no state"
    );

    let correlation = fixture
        .store
        .claim_due(fixture.project, now(), 10)
        .expect("the outbox is claimable")
        .into_iter()
        .find(|claim| claim.receipt_id == receipt_id)
        .expect("the due entry is claimed")
        .correlation;

    // The same rule once a token exists: `dispatch_pending` → `dispatch_pending`
    // carrying a correlation the outbox never minted is refused, where before it
    // was handed the original receipt as though the two had agreed.
    let fixture = fixture.restart();
    let claimed = census(&fixture);
    let receipt_before = fixture
        .store
        .get_receipt(fixture.project, receipt_id)
        .expect("the read succeeds")
        .expect("the receipt exists");
    let history_before = fixture
        .store
        .receipt_history(fixture.project, receipt_id)
        .expect("the history reads");
    let refused = fixture
        .store
        .apply_command_transition(&CommandTransition {
            correlation: Some(external("correlation-a-replay-invented")),
            ..step(&fixture, receipt_id, CommandReceiptState::DispatchPending)
        })
        .expect_err("a same-state replay may not invent a correlation either");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict { .. }
        ),
        "the disagreement is a conflict: {refused:?}"
    );
    assert_eq!(census(&fixture), claimed, "and it writes nothing");
    assert_eq!(
        fixture
            .store
            .get_receipt(fixture.project, receipt_id)
            .expect("the read succeeds")
            .expect("the receipt exists"),
        receipt_before,
        "the durable correlation is exactly as it was"
    );
    assert_eq!(
        fixture
            .store
            .receipt_history(fixture.project, receipt_id)
            .expect("the history reads"),
        history_before,
        "and no history row was appended"
    );

    // The genuine repeat — the claim token it already holds, and the caller that
    // supplies nothing at all — is still idempotent, so the rule refuses
    // disagreement rather than resumption.
    for repeat in [Some(correlation.clone()), None] {
        let repeated = fixture
            .store
            .apply_command_transition(&CommandTransition {
                correlation: repeat,
                ..step(&fixture, receipt_id, CommandReceiptState::DispatchPending)
            })
            .expect("a repeat under the claimed token is idempotent");
        assert!(!repeated.appended, "and appends no second transition");
        assert_eq!(repeated.receipt.correlation, Some(correlation.clone()));
    }
    assert_eq!(
        census(&fixture),
        claimed,
        "an idempotent repeat writes nothing"
    );
}

#[test]
fn a_reused_idempotency_key_must_name_the_same_durable_command() {
    let fixture = fixture();
    let (receipt_id, intent) = launch_intent(&fixture, "launch-1", "launch");
    let original = fixture
        .store
        .record_intent(&intent)
        .expect("the intent is recorded");
    let after = census(&fixture);

    let queued_payload = |fixture: &Fixture| -> String {
        Connection::open(&fixture.path)
            .expect("a raw connection opens")
            .query_row(
                "SELECT payload_hash FROM command_outbox WHERE receipt_id = ?1",
                rusqlite::params![receipt_id.to_string()],
                |row| row.get(0),
            )
            .expect("the queued payload is readable")
    };
    let queued = queued_payload(&fixture);

    // Every one of these keeps the target and the intent digest the *receipt*
    // stores, and changes something else the command is made of. Comparing only
    // those two returned the original receipt for all three — while the original
    // payload stayed queued, so the caller is told "recorded" about a dispatch
    // that will send something else, at a different time, against a revision it
    // never checked.
    let reused = [
        (
            "a different dispatch payload",
            NewCommandIntent {
                receipt_id: CommandReceiptId::generate(),
                payload: document("launch-payload-rewritten"),
                ..intent.clone()
            },
        ),
        (
            "a different target revision",
            NewCommandIntent {
                receipt_id: CommandReceiptId::generate(),
                target_revision: AggregateRevision::parse(99).expect("a valid revision"),
                ..intent.clone()
            },
        ),
        (
            "a different earliest dispatch instant",
            NewCommandIntent {
                receipt_id: CommandReceiptId::generate(),
                not_before: at("2026-08-09T18:00:00Z"),
                ..intent.clone()
            },
        ),
    ];
    for (label, request) in reused {
        assert!(
            fixture.store.record_intent(&request).is_err(),
            "a key reused with {label} is a different command wearing a used key"
        );
        assert_eq!(
            census(&fixture),
            after,
            "and refusing it writes nothing ({label})"
        );
        assert_eq!(
            queued_payload(&fixture),
            queued,
            "the original dispatch is still the one queued ({label})"
        );
    }

    // The byte-identical replay is untouched by the stricter comparison: it is
    // still the same command, and still answered with the original receipt.
    let fixture = fixture.restart();
    let replay = fixture
        .store
        .record_intent(&intent)
        .expect("a replay of the same command returns the original");
    assert_eq!(replay, original);
    assert_eq!(census(&fixture), after, "and enqueues nothing");
}

#[test]
fn a_receipt_that_queued_nothing_is_settled_rather_than_a_missing_outbox_entry() {
    // The incident: abandoning one run left the realm serving with scheduling
    // shut. A closure receipt records a decision already carried out in its own
    // transaction, so it deliberately writes no outbox entry — and the recovery
    // scan raised `NotFound` on the row it was defined never to have, which
    // failed the whole startup inventory rather than that one receipt.
    let fixture = fixture();
    let receipt_id = CommandReceiptId::generate();
    let intent = document("abandon");
    fixture
        .store
        .record_abandon_receipt(&kontor_core::repository::NewAbandonReceipt {
            project_id: fixture.project,
            receipt_id,
            idempotency_key: IdempotencyKey::parse("abandon-1").expect("a key"),
            target: AggregateRef::AgentRun {
                agent_run_id: kontor_core::id::AgentRunId::parse(RUN).expect("a canonical id"),
            },
            target_revision: AggregateRevision::parse(1).expect("a valid revision"),
            intent,
            recorded_at: now(),
        })
        .expect("the abandon receipt is recorded");
    assert_eq!(
        census(&fixture)["command_outbox"],
        0,
        "a closure receipt queues nothing, which is the whole point"
    );

    let fixture = fixture.restart();
    let recovery = fixture
        .store
        .classify_command_recovery(fixture.project, receipt_id)
        .expect("a receipt with nothing queued is still classifiable");
    assert!(
        !recovery.authorizes_launch(),
        "a decision already carried out must never authorize a launch"
    );
    assert!(
        matches!(
            recovery,
            CommandRecovery::Settled {
                state: CommandReceiptState::Confirmed,
                ..
            }
        ),
        "a closure receipt is born confirmed: it is not queued and never will be"
    );

    // The reader has to hold on its own, because the rows an older binary
    // already wrote are still `intent_persisted` with nothing queued behind
    // them, and no migration reaches a realm that has already started. One such
    // row was enough to fail the whole inventory.
    let legacy = CommandReceiptId::generate();
    let connection = Connection::open(&fixture.path).expect("a raw connection opens");
    connection
        .execute(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at)
             SELECT ?1, project_id, ?2, kind, target, target_revision, intent, intent_hash,
                    'intent_persisted', attempts, created_at, updated_at
               FROM command_receipts WHERE id = ?3",
            rusqlite::params![legacy.to_string(), "abandon-legacy", receipt_id.to_string()],
        )
        .expect("a receipt as an older binary wrote it");
    let recovery = fixture
        .store
        .classify_command_recovery(fixture.project, legacy)
        .expect("an already-written closure receipt does not fail the inventory");
    assert!(
        !recovery.authorizes_launch(),
        "nothing was ever queued, so nothing may be sent"
    );
    assert!(
        matches!(recovery, CommandRecovery::Settled { .. }),
        "a receipt with no outbox entry is settled, not a missing row"
    );
}
