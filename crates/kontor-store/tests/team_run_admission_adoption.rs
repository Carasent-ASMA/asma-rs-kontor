//! Adopting an already-created AgentRun into a declared TeamRun slot.
//!
//! The ledger this exercises is append-only and its three uniqueness rules are
//! the contract: one slot is adopted once, one run is adopted once, and one
//! command records one adoption. Every test here is about a refusal that has to
//! happen *before* the insert, or about the insert being the only thing that
//! changed.
//!
//! The fixtures are seeded through raw SQL rather than through `admit_candidate`
//! for one reason: the shape under test is a run that the scheduler never
//! admitted, and `admit_candidate` cannot produce one — it writes the admission
//! event whose absence is the precondition.

use kontor_core::id::{AgentRunId, CommandReceiptId, ExternalId, parse_utc_timestamp};
use kontor_core::id::{AggregateRevision, ProjectId, RoleSlotId, TaskId, TeamRunId};
use kontor_core::repository::{AdoptionWrite, RepositoryError, StoredTeamRunAdmissionAdoption};
use kontor_store::SqliteStore;
use rusqlite::Connection;
use tempfile::TempDir;

const NOW: &str = "2026-09-20T12:00:00Z";
const TEMPLATE: &str = "0000000a-0000-4000-8000-0000000000aa";

struct Harness {
    directory: TempDir,
    store: SqliteStore,
}

impl Harness {
    fn new() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let store =
            SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");
        Self { directory, store }
    }

    fn raw(&self) -> Connection {
        let connection = Connection::open(self.directory.path().join("kontor.db"))
            .expect("a raw connection opens");
        connection
            .pragma_update(None, "foreign_keys", true)
            .expect("foreign keys can be enabled");
        connection
    }
}

/// Every identity one adoption is about.
struct Fixture {
    project: ProjectId,
    task: TaskId,
    team_run: TeamRunId,
    agent_run: AgentRunId,
    receipt: CommandReceiptId,
    slot: RoleSlotId,
}

/// A team whose frozen snapshot declares one `verify` slot, and a run holding
/// that role which nothing has claimed: no admission, no dispatch, no binding.
fn seed(harness: &Harness, label: &str) -> Fixture {
    let project = ProjectId::generate();
    let task = TaskId::generate();
    let team_run = TeamRunId::generate();
    let agent_run = AgentRunId::generate();
    let receipt = CommandReceiptId::generate();
    let slot = RoleSlotId::parse("verify").expect("a slot id");
    let snapshot = serde_json::json!({
        "schema_version": 1,
        "template_id": TEMPLATE,
        "template_version": 1,
        "definition": {
            "schema_version": 1,
            "template_id": TEMPLATE,
            "version": 1,
            "name": label,
            "roles": [],
            "handoffs": [],
            "max_handoff_depth": 4,
            "max_successor_depth": 4,
            "slots": [{ "id": "verify", "role": "verifier" }]
        }
    })
    .to_string();

    let connection = harness.raw();
    connection
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![
                project.to_string(),
                format!("{label} project"),
                format!("/tmp/{label}"),
                NOW
            ],
        )
        .expect("a project is seeded");
    connection
        .execute(
            "INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'in_progress', 1, ?4, ?4)",
            rusqlite::params![task.to_string(), project.to_string(), label, NOW],
        )
        .expect("a task is seeded");
    connection
        .execute(
            "INSERT INTO team_templates (project_id, template_id, version, name, definition,
                                         definition_hash, role_authority, created_at)
             VALUES (?1, ?6, 1, ?2, ?3, ?4, '{}', ?5)",
            rusqlite::params![
                project.to_string(),
                format!("{label} template"),
                snapshot,
                "4".repeat(64),
                NOW,
                TEMPLATE
            ],
        )
        .expect("a team template is seeded");
    connection
        .execute(
            "INSERT INTO team_runs (id, project_id, task_id, template_id, template_version,
                                    snapshot, snapshot_hash, lifecycle, revision, created_at)
             VALUES (?1, ?2, ?3, ?7, 1, ?4, ?5, 'running', 1, ?6)",
            rusqlite::params![
                team_run.to_string(),
                project.to_string(),
                task.to_string(),
                snapshot,
                "0".repeat(64),
                NOW,
                TEMPLATE
            ],
        )
        .expect("a team run is seeded");
    connection
        .execute(
            "INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle,
                                     desired_state, observed_state, derived_state, revision,
                                     created_at)
             VALUES (?1, ?2, ?3, 'verifier', 'queued', 'run_requested', 'unknown',
                     'pending_confirmation', 3, ?4)",
            rusqlite::params![
                agent_run.to_string(),
                project.to_string(),
                team_run.to_string(),
                NOW
            ],
        )
        .expect("an unclaimed run is seeded");
    seed_receipt(&connection, project, receipt, &format!("{label}-key"));

    Fixture {
        project,
        task,
        team_run,
        agent_run,
        receipt,
        slot,
    }
}

fn seed_receipt(connection: &Connection, project: ProjectId, receipt: CommandReceiptId, key: &str) {
    connection
        .execute(
            "INSERT INTO command_receipts (id, project_id, idempotency_key, kind, target,
                                           target_revision, intent, intent_hash, state, attempts,
                                           created_at, updated_at)
             VALUES (?1, ?2, ?3, 'replace_seat', '{}', 1, '{}', ?4, 'confirmed', 0, ?5, ?5)",
            rusqlite::params![
                receipt.to_string(),
                project.to_string(),
                key,
                "1".repeat(64),
                NOW
            ],
        )
        .expect("a receipt is seeded");
}

fn claim(fixture: &Fixture) -> StoredTeamRunAdmissionAdoption {
    StoredTeamRunAdmissionAdoption {
        id: ExternalId::parse(&uuid_like(1)).expect("an adoption id"),
        project_id: fixture.project,
        task_id: fixture.task,
        team_run_id: fixture.team_run,
        role_slot_id: fixture.slot.clone(),
        agent_run_id: fixture.agent_run,
        adopted_agent_run_revision: AggregateRevision::parse(3).expect("a revision"),
        receipt_id: fixture.receipt,
        adopted_at: parse_utc_timestamp(NOW).expect("a timestamp"),
    }
}

fn uuid_like(seed: u8) -> String {
    format!("0000000{seed}-0000-4000-8000-00000000000{seed}")
}

fn rule_of(error: &RepositoryError) -> &'static str {
    match error {
        RepositoryError::Conflict { rule, .. } => rule,
        other => panic!("expected a conflict, got {other:?}"),
    }
}

fn adoption_count(harness: &Harness) -> i64 {
    harness
        .raw()
        .query_row(
            "SELECT COUNT(*) FROM team_run_admission_adoptions",
            [],
            |row| row.get(0),
        )
        .expect("the ledger is readable")
}

#[test]
fn an_unclaimed_run_is_adopted_into_its_declared_slot() {
    let harness = Harness::new();
    let fixture = seed(&harness, "happy");
    let (stored, write) = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect("the adoption records");
    assert_eq!(write, AdoptionWrite::Recorded);
    assert_eq!(stored.agent_run_id, fixture.agent_run);
    assert_eq!(stored.role_slot_id, fixture.slot);
    assert_eq!(
        stored.adopted_agent_run_revision,
        AggregateRevision::parse(3).expect("a revision")
    );
    assert_eq!(adoption_count(&harness), 1);

    // It is readable as the authority a later fill consumes.
    let by_slot = harness
        .store
        .team_run_admission_adoption(fixture.project, fixture.team_run, &fixture.slot)
        .expect("the slot reads")
        .expect("an adoption");
    assert_eq!(by_slot, stored);
}

#[test]
fn the_same_command_replays_onto_the_same_adoption() {
    let harness = Harness::new();
    let fixture = seed(&harness, "replay");
    let first = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect("the adoption records");
    let second = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect("the replay reads");
    assert_eq!(first.0, second.0, "a replay returns the original row");
    assert_eq!(second.1, AdoptionWrite::Replayed);
    assert_eq!(adoption_count(&harness), 1, "a replay writes nothing");
}

#[test]
fn the_same_receipt_carrying_a_different_claim_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "drift");
    harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect("the adoption records");

    // A second run, same receipt: the command already said something else.
    let connection = harness.raw();
    let other_run = AgentRunId::generate();
    connection
        .execute(
            "INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle,
                                     desired_state, observed_state, derived_state, revision,
                                     created_at)
             VALUES (?1, ?2, ?3, 'verifier', 'queued', 'run_requested', 'unknown',
                     'pending_confirmation', 3, ?4)",
            rusqlite::params![
                other_run.to_string(),
                fixture.project.to_string(),
                fixture.team_run.to_string(),
                NOW
            ],
        )
        .expect("a second run is seeded");
    let mut changed = claim(&fixture);
    changed.agent_run_id = other_run;
    let error = harness
        .store
        .adopt_team_run_admission(&changed)
        .expect_err("changed intent is refused");
    assert_eq!(
        rule_of(&error),
        "this receipt already recorded a different adoption"
    );
    assert_eq!(adoption_count(&harness), 1);
}

#[test]
fn a_run_that_moved_since_the_caller_read_it_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "revision");
    let mut stale = claim(&fixture);
    stale.adopted_agent_run_revision = AggregateRevision::parse(2).expect("a revision");
    let error = harness
        .store
        .adopt_team_run_admission(&stale)
        .expect_err("a stale revision is refused");
    assert_eq!(rule_of(&error), "the run moved since the caller read it");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_run_a_runtime_already_bound_is_not_adoptable() {
    let harness = Harness::new();
    let fixture = seed(&harness, "bound");
    harness
        .raw()
        .execute(
            "INSERT INTO runtime_bindings (id, project_id, agent_run_id, runtime_kind, host,
                                           generation, native_id, bound_at)
             VALUES (?1, ?2, ?3, 'sa.runtime', 'host-1', 1, 'native-1', ?4)",
            rusqlite::params![
                uuid_like(7),
                fixture.project.to_string(),
                fixture.agent_run.to_string(),
                NOW
            ],
        )
        .expect("a binding is seeded");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("a bound run is refused");
    assert_eq!(rule_of(&error), "the run already has a runtime binding");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_run_from_another_team_or_holding_another_role_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "membership");
    let other = seed(&harness, "elsewhere");

    // Same project shape, different team: membership is not implied by the id
    // existing somewhere.
    let mut foreign = claim(&fixture);
    foreign.agent_run_id = other.agent_run;
    let error = harness
        .store
        .adopt_team_run_admission(&foreign)
        .expect_err("a foreign run is refused");
    assert_eq!(rule_of(&error), "the run does not exist in this project");

    // Same team, wrong role for the slot the snapshot declares.
    let connection = harness.raw();
    let wrong_role = AgentRunId::generate();
    connection
        .execute(
            "INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle,
                                     desired_state, observed_state, derived_state, revision,
                                     created_at)
             VALUES (?1, ?2, ?3, 'implementer', 'queued', 'run_requested', 'unknown',
                     'pending_confirmation', 3, ?4)",
            rusqlite::params![
                wrong_role.to_string(),
                fixture.project.to_string(),
                fixture.team_run.to_string(),
                NOW
            ],
        )
        .expect("a wrong-role run is seeded");
    let mut mismatched = claim(&fixture);
    mismatched.agent_run_id = wrong_role;
    let error = harness
        .store
        .adopt_team_run_admission(&mismatched)
        .expect_err("a wrong role is refused");
    assert_eq!(
        rule_of(&error),
        "the run does not hold the role this slot declares"
    );
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_slot_the_frozen_snapshot_does_not_declare_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "undeclared");
    let mut invented = claim(&fixture);
    invented.role_slot_id = RoleSlotId::parse("audit").expect("a slot id");
    let error = harness
        .store
        .adopt_team_run_admission(&invented)
        .expect_err("an undeclared slot is refused");
    assert_eq!(
        rule_of(&error),
        "the frozen TeamRun snapshot does not declare this slot"
    );
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_run_the_scheduler_already_admitted_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "admitted");
    harness
        .raw()
        .execute(
            "INSERT INTO scheduler_admission_events (id, project_id, task_id, team_run_id,
                                                     agent_run_id, launch_receipt_id, decision,
                                                     evidence, evidence_hash, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?8, 'admitted', '{}', ?6, ?7)",
            rusqlite::params![
                uuid_like(8),
                fixture.project.to_string(),
                fixture.task.to_string(),
                fixture.team_run.to_string(),
                fixture.agent_run.to_string(),
                "2".repeat(64),
                NOW,
                fixture.receipt.to_string()
            ],
        )
        .expect("an admission is seeded");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("an admitted run is refused");
    assert_eq!(rule_of(&error), "the run already has a scheduler admission");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_slot_that_already_owes_a_dispatch_needs_no_adoption() {
    let harness = Harness::new();
    let fixture = seed(&harness, "owed");
    let connection = harness.raw();
    // The dispatch's own parent is not what is under test, so the isolated
    // fixture disables foreign keys exactly as the schema fixtures do.
    connection
        .pragma_update(None, "foreign_keys", false)
        .expect("the isolated fixture disables foreign keys");
    connection
        .execute(
            "INSERT INTO turn_dispatches (settled_turn_id, to_role_slot_id, project_id,
                                          team_run_id, message_id, dispatched, derived_at)
             VALUES (?1, 'verify', ?2, ?3, ?4, 0, ?5)",
            rusqlite::params![
                uuid_like(9),
                fixture.project.to_string(),
                fixture.team_run.to_string(),
                uuid_like(6),
                NOW
            ],
        )
        .expect("an owed dispatch is seeded");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("an owed slot is refused");
    assert_eq!(
        rule_of(&error),
        "the slot already has an owed dispatch and needs no adoption"
    );
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_waived_slot_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "waived");
    harness
        .raw()
        .execute(
            "INSERT INTO role_slot_waivers (id, project_id, task_id, team_run_id, role_slot_id,
                                            idempotency_key, team_run_revision, authorized_role,
                                            authority_tier, evidence, evidence_hash, recorded_at)
             VALUES (?1, ?2, ?3, ?4, 'verify', 'waive-1', 1, 'verifier', 'admin', '[1]', ?5, ?6)",
            rusqlite::params![
                uuid_like(5),
                fixture.project.to_string(),
                fixture.task.to_string(),
                fixture.team_run.to_string(),
                "3".repeat(64),
                NOW
            ],
        )
        .expect("a waiver is seeded");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("a waived slot is refused");
    assert_eq!(rule_of(&error), "the declared slot was waived");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_terminal_team_cannot_adopt_a_run() {
    let harness = Harness::new();
    let fixture = seed(&harness, "terminal-team");
    harness
        .raw()
        .execute(
            "UPDATE team_runs SET lifecycle = 'cancelled', closed_at = ?2,
                    terminal_outcome = 'cancelled', terminal_source_kind = 'child_evidence',
                    terminal_evidence_hash = ?3
              WHERE id = ?1",
            rusqlite::params![fixture.team_run.to_string(), NOW, "5".repeat(64)],
        )
        .expect("the team is closed");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("a terminal team is refused");
    assert_eq!(rule_of(&error), "a terminal TeamRun cannot adopt a run");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_closed_task_cannot_adopt_a_run() {
    // A separate fixture, because a closed team's evidence is immutable and the
    // schema will not let one be reopened to reuse it.
    let harness = Harness::new();
    let fixture = seed(&harness, "closed-task");
    harness
        .raw()
        .execute(
            "UPDATE tasks SET state = 'done' WHERE id = ?1",
            rusqlite::params![fixture.task.to_string()],
        )
        .expect("the task closes");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("a closed task is refused");
    assert_eq!(rule_of(&error), "a closed task cannot adopt a run");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_run_that_is_not_an_unstarted_admission_is_refused() {
    let harness = Harness::new();
    let fixture = seed(&harness, "started");
    harness
        .raw()
        .execute(
            "UPDATE agent_runs SET lifecycle = 'running', observed_state = 'running'
              WHERE id = ?1",
            rusqlite::params![fixture.agent_run.to_string()],
        )
        .expect("the run starts");
    let error = harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect_err("a started run is refused");
    assert_eq!(rule_of(&error), "the run is not an unstarted admission");
    assert_eq!(adoption_count(&harness), 0);
}

#[test]
fn a_refused_adoption_writes_nothing_at_all() {
    let harness = Harness::new();
    let fixture = seed(&harness, "rollback");
    let connection = harness.raw();
    let before: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_runs", [], |row| row.get(0))
        .expect("runs count");

    let mut stale = claim(&fixture);
    stale.adopted_agent_run_revision = AggregateRevision::parse(9).expect("a revision");
    harness
        .store
        .adopt_team_run_admission(&stale)
        .expect_err("the adoption is refused");

    assert_eq!(adoption_count(&harness), 0, "no adoption is left behind");
    let after: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_runs", [], |row| row.get(0))
        .expect("runs count");
    assert_eq!(before, after, "a refusal touches no other row");
    let run_revision: i64 = connection
        .query_row(
            "SELECT revision FROM agent_runs WHERE id = ?1",
            rusqlite::params![fixture.agent_run.to_string()],
            |row| row.get(0),
        )
        .expect("the run reads");
    assert_eq!(run_revision, 3, "the run itself is untouched");
}

#[test]
fn one_slot_and_one_run_are_adopted_exactly_once() {
    let harness = Harness::new();
    let fixture = seed(&harness, "unique");
    harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect("the first adoption records");

    // A different command claiming the same slot with a different run.
    let connection = harness.raw();
    let second_receipt = CommandReceiptId::generate();
    seed_receipt(&connection, fixture.project, second_receipt, "unique-key-2");
    let other_run = AgentRunId::generate();
    connection
        .execute(
            "INSERT INTO agent_runs (id, project_id, team_run_id, role_key, lifecycle,
                                     desired_state, observed_state, derived_state, revision,
                                     created_at)
             VALUES (?1, ?2, ?3, 'verifier', 'queued', 'run_requested', 'unknown',
                     'pending_confirmation', 3, ?4)",
            rusqlite::params![
                other_run.to_string(),
                fixture.project.to_string(),
                fixture.team_run.to_string(),
                NOW
            ],
        )
        .expect("a second run is seeded");
    let mut same_slot = claim(&fixture);
    same_slot.id = ExternalId::parse(&uuid_like(2)).expect("an id");
    same_slot.receipt_id = second_receipt;
    same_slot.agent_run_id = other_run;
    let error = harness
        .store
        .adopt_team_run_admission(&same_slot)
        .expect_err("the slot is already adopted");
    assert_eq!(rule_of(&error), "this slot was already adopted");
    assert_eq!(adoption_count(&harness), 1);
}

#[test]
fn an_adoption_can_be_neither_edited_nor_removed() {
    let harness = Harness::new();
    let fixture = seed(&harness, "immutable");
    harness
        .store
        .adopt_team_run_admission(&claim(&fixture))
        .expect("the adoption records");
    let connection = harness.raw();
    let updated = connection.execute(
        "UPDATE team_run_admission_adoptions SET adopted_agent_run_revision = 99",
        [],
    );
    assert!(updated.is_err(), "an adoption is immutable");
    let deleted = connection.execute("DELETE FROM team_run_admission_adoptions", []);
    assert!(deleted.is_err(), "an adoption is not deletable");
    assert_eq!(adoption_count(&harness), 1);
}
