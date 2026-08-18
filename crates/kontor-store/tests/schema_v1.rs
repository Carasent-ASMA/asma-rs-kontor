//! Migration, connection and schema-shape verification.
//!
//! These tests use a file-backed database throughout. WAL is a file property but
//! `foreign_keys` and `busy_timeout` are not: they are per-connection, so an
//! `:memory:` database would prove nothing about a reopened file.
//!
//! The mutants this suite exists to kill:
//!
//! * applying the migration outside a transaction, so a failure leaves half a
//!   schema behind;
//! * opening a database written by a newer binary;
//! * forgetting to re-apply the connection pragmas on reopen;
//! * turning an append-only table into an updatable one, or letting direct SQL
//!   reopen a terminal run.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use kontor_core::id::{AccountProfileId, ProjectId};
use kontor_core::repository::{ProjectRepository, RepositoryError};
use kontor_store::{SCHEMA_VERSION, SqliteStore, StoreError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use tempfile::TempDir;

/// Every table the current schema owns, across all generations. The list is
/// spelled out so that adding or removing one is a deliberate, reviewed change.
const EXPECTED_TABLES: &[&str] = &[
    "account_profiles",
    "adaptive_admission_state",
    "agent_runs",
    "approval_receipts",
    "artifact_evidence",
    "availability_overrides",
    "calendar_exceptions",
    "calendar_profiles",
    "capacity_configuration",
    "capacity_observations",
    "child_calendar_windows",
    "command_outbox",
    "command_receipt_transitions",
    "command_receipts",
    "command_targets",
    "compaction_receipts",
    "context_packs",
    "core_team_revisions",
    "epic_rosters",
    "execution_authorization_revocations",
    "execution_authorization_tasks",
    "execution_authorizations",
    "external_comments",
    "external_ticket_observations",
    "external_workflow_specs",
    "gate_waivers",
    "guardrail_evaluations",
    "handoffs",
    // Schema v7 (KON-MVP-21): which importer produced a holiday source revision,
    // what the request asked for, and the chain that makes one import current.
    "holiday_import_batches",
    "holiday_sources",
    // Schema v5 (KON-MVP-19): the destination half of a redacted import.
    "import_receipts",
    "imported_records",
    // Schema v6 (KON-MVP-22): the terminal half of intake and its work lineage.
    "intake_created_work",
    "intake_decisions",
    "intake_receipts",
    "jira_links",
    "lease_events",
    "memory_approvals",
    "memory_authority",
    "memory_context_bindings",
    "memory_fts",
    "memory_fts_config",
    "memory_fts_content",
    "memory_fts_data",
    "memory_fts_docsize",
    "memory_fts_idx",
    "memory_import_manifests",
    "memory_items",
    "memory_purges",
    "memory_receipts",
    "memory_revisions",
    "memory_tombstones",
    "mini_projects",
    "mini_project_topology_snapshots",
    "persona_scenarios",
    "policy_evaluations",
    "projects",
    "project_topology_defaults",
    "quick_session_promotions",
    "quick_sessions",
    "realm_idempotency_bindings",
    "realm_metadata",
    "recovery_episodes",
    "recovery_steps",
    "registered_profile_packs",
    "resource_leases",
    "role_slot_waivers",
    "role_catalog_revisions",
    "role_turns",
    "run_context_policies",
    "run_park_closures",
    "runtime_binding_snapshots",
    "runtime_bindings",
    "runtime_content_gaps",
    "runtime_control_gaps",
    "runtime_events",
    "runtime_reconciliation_epochs",
    "runtime_reconciliation_members",
    "runtime_reconciliation_results",
    "runtime_replay_consumers",
    "schedule_overrides",
    "scheduler_admission_events",
    "source_events",
    "status_conflicts",
    "status_transition_receipts",
    "seat_bindings",
    "task_account_selections",
    "task_dependencies",
    "task_gate_evaluations",
    "task_persona_snapshots",
    "task_workflows",
    "task_worktrees",
    "tasks",
    "team_command_replays",
    "team_drafts",
    "team_revisions",
    "team_runs",
    "team_templates",
    "teams_projection",
    "ticket_field_specs",
    "ticket_sync_projections",
    "trigger_specs",
    "topology_node_containers",
    "topology_nodes",
    "topology_specs",
    "turn_dispatches",
    "work_calendars",
    "work_profiles",
];

/// The frozen v1 script, so the upgrade test can build a genuine v1 file.
const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");

/// Every migration up to schema v9, so a test can build a genuine pre-v10 file
/// rather than degrading a current one.
const MIGRATIONS_THROUGH_V9: &[&str] = &[
    MIGRATION_0001,
    include_str!("../migrations/0002_account_profiles_expanded.sql"),
    include_str!("../migrations/0003_guardrails_and_recovery.sql"),
    include_str!("../migrations/0004_scheduler_admission.sql"),
    include_str!("../migrations/0005_redacted_import.sql"),
    include_str!("../migrations/0006_intake_decisions.sql"),
    include_str!("../migrations/0007_calendar_imports.sql"),
    include_str!("../migrations/0008_child_calendar_windows.sql"),
    include_str!("../migrations/0009_authorization_revocation.sql"),
];

/// Every migration up to schema v24, so a test can build a genuine
/// pre-shareability file rather than degrading a current one.
const MIGRATIONS_THROUGH_V24: &[&str] = &[
    MIGRATION_0001,
    include_str!("../migrations/0002_account_profiles_expanded.sql"),
    include_str!("../migrations/0003_guardrails_and_recovery.sql"),
    include_str!("../migrations/0004_scheduler_admission.sql"),
    include_str!("../migrations/0005_redacted_import.sql"),
    include_str!("../migrations/0006_intake_decisions.sql"),
    include_str!("../migrations/0007_calendar_imports.sql"),
    include_str!("../migrations/0008_child_calendar_windows.sql"),
    include_str!("../migrations/0009_authorization_revocation.sql"),
    include_str!("../migrations/0010_bootstrap_command_kinds.sql"),
    include_str!("../migrations/0011_task_account_selection.sql"),
    include_str!("../migrations/0012_surface_command_kinds.sql"),
    include_str!("../migrations/0013_context_policy_and_compaction.sql"),
    include_str!("../migrations/0014_registered_profile_packs.sql"),
    include_str!("../migrations/0015_realm_idempotency_bindings.sql"),
    include_str!("../migrations/0016_task_worktrees.sql"),
    include_str!("../migrations/0017_runtime_binding_snapshots.sql"),
    include_str!("../migrations/0018_role_turns.sql"),
    include_str!("../migrations/0019_team_closure_on_settled_turns.sql"),
    include_str!("../migrations/0020_role_slot_waivers.sql"),
    include_str!("../migrations/0021_native_memory.sql"),
    include_str!("../migrations/0022_teams_editor.sql"),
    include_str!("../migrations/0023_operational_topology.sql"),
    include_str!("../migrations/0024_replace_seat_command.sql"),
];

/// The suffix deployed by OP-03 before the OP-04 migrations were integrated.
///
/// Both delivery branches called their second migration `v31`. This exact
/// historical chain builds the live shape the converging v32/v33 upgrade must
/// recognize without relabeling or rebuilding the database out of band.
const OP03_MIGRATIONS_V25_THROUGH_V31: &[&str] = &[
    include_str!("../migrations/0025_document_shareability.sql"),
    include_str!("../migrations/0026_operational_liveness.sql"),
    include_str!("../migrations/0027_task_topology_node.sql"),
    include_str!("../migrations/0028_native_capacity.sql"),
    include_str!("../migrations/0029_topology_publication.sql"),
    include_str!("../migrations/0030_retitle_container_command.sql"),
    include_str!("../migrations/0031_bounded_task_reopen.sql"),
];

/// A minimal project → task → workflow → team run → agent run chain, inserted
/// with direct SQL so the schema's own constraints are what is under test.
const RUN_FIXTURE: &str = "\
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
INSERT INTO work_profiles (project_id, profile_key, version, definition, definition_hash, \
        created_at) \
VALUES ('0193f000-0000-7000-8000-000000000001', 'q7.delivery', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
        '2026-08-09T10:00:00Z'); \
INSERT INTO task_workflows (id, project_id, task_id, profile_key, profile_version, snapshot, \
        snapshot_hash, current_phase, active, revision, created_at) \
VALUES ('0193f000-0000-7000-8000-000000000030', '0193f000-0000-7000-8000-000000000001', \
        '0193f000-0000-7000-8000-000000000010', 'q7.delivery', 1, '{}', \
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'q7.capture', 1, \
        1, '2026-08-09T10:00:00Z'); \
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
        'running', 'confirmed', 1, '2026-08-09T10:00:00Z');";

fn temp() -> TempDir {
    TempDir::new().expect("a temporary directory")
}

fn open(directory: &TempDir) -> SqliteStore {
    SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens and migrates")
}

fn raw(directory: &TempDir) -> Connection {
    let connection =
        Connection::open(directory.path().join("kontor.db")).expect("a raw connection opens");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys can be enabled");
    connection
}

fn table_names(connection: &Connection) -> BTreeSet<String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("the catalogue is readable");
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("the catalogue is readable");
    names.map(|name| name.expect("a table name")).collect()
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

#[test]
fn an_empty_database_migrates_to_the_current_schema_version() {
    let directory = temp();
    let store = open(&directory);
    assert_eq!(
        store.schema_version().expect("the version is readable"),
        SCHEMA_VERSION
    );
    assert_eq!(SCHEMA_VERSION, 33);
}

/// The two Wave-3 branches independently occupied schema numbers 30 and 31.
///
/// The live OP-03 lineage therefore has `retitle_container` receipts and the
/// bounded task-reopen trigger, but no Core Team or Quick-session tables. A
/// daemon built from OP-04 used to see the matching number, skip migration, and
/// then fail its startup inventory because its enum could not decode that
/// durable receipt. The forward-only convergence preserves the receipt, keeps
/// the Realm identity and adds the missing OP-04 schema.
#[test]
fn the_deployed_op03_v31_lineage_converges_without_losing_its_receipts() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    const REALM: &str = "0193f000-0000-7000-8000-0000000000e1";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000e2";
    const RECEIPT: &str = "0193f000-0000-7000-8000-0000000000e3";
    const HASH: &str = "a9d5f6d002d956b8af5787a05e0ca000d45c03977ffa54ee8fbed719fed5fd23";

    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        for migration in MIGRATIONS_THROUGH_V24
            .iter()
            .chain(OP03_MIGRATIONS_V25_THROUGH_V31)
        {
            connection
                .execute_batch(migration)
                .expect("the deployed OP-03 migration chain runs");
        }
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-17T06:00:00Z', NULL)",
                [REALM],
            )
            .expect("the deployed Realm identity is written");
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, 'P', '/tmp/op03-v31', 1, '2026-08-17T06:00:00Z')",
                [PROJECT],
            )
            .expect("the deployed project is written");
        connection
            .execute(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision,
                      intent, intent_hash, state, attempts, created_at, updated_at)
                 VALUES (?1, ?2, 'op03-retitle', 'retitle_container',
                         json_object('kind', 'project', 'project_id', ?2), 1,
                         json_object('schema_version', 1), ?3, 'intent_persisted', 0,
                         '2026-08-17T06:00:00Z', '2026-08-17T06:00:00Z')",
                rusqlite::params![RECEIPT, PROJECT, HASH],
            )
            .expect("the historical retitle receipt is written");
    }

    let store = SqliteStore::open(&path).expect("the deployed v31 lineage converges");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(store.realm_id().to_string(), REALM);
    store
        .foreign_key_check()
        .expect("the receipt rebuild keeps every reference sound");
    store
        .integrity_check()
        .expect("the converged file is sound");

    let project = ProjectId::parse(PROJECT).expect("a project id");
    let receipt = kontor_core::id::CommandReceiptId::parse(RECEIPT).expect("a receipt id");
    let kept = store
        .get_receipt(project, receipt)
        .expect("the historical receipt decodes")
        .expect("the historical receipt survives");
    assert_eq!(
        kept.kind,
        kontor_core::receipt::CommandKind::RetitleContainer
    );

    let connection = raw(&directory);
    for table in [
        "core_team_revisions",
        "quick_sessions",
        "quick_session_promotions",
        "epic_rosters",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("the catalogue is readable");
        assert_eq!(exists, 1, "the OP-04 table `{table}` was not added");
    }
}

/// A database left at schema v1 is brought forward on open, keeping the Realm it
/// already has. This is the upgrade path an existing file actually takes, and it
/// is the one place a second Realm could be minted by mistake.
#[test]
fn a_schema_v1_database_is_upgraded_in_place_and_keeps_its_realm() {
    let directory = temp();
    let path = directory.path().join("kontor.db");

    // Build a genuine v1 file from the frozen v1 script, rather than degrading a
    // v2 one: this is byte-for-byte what a v1 binary would have left behind.
    const REALM_BEFORE: &str = "0193f000-0000-7000-8000-0000000000c1";
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .execute_batch(MIGRATION_0001)
            .expect("the v1 migration runs");
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-09T10:00:00Z', NULL)",
                [REALM_BEFORE],
            )
            .expect("the v1 realm row is written");
    }
    let realm_before = REALM_BEFORE.to_owned();

    let store = SqliteStore::open(&path).expect("a v1 database is upgraded, not refused");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(
        store.realm_id().to_string(),
        realm_before,
        "an upgrade must not mint a second Realm identity"
    );

    let connection = raw(&directory);
    let realms: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(realms, 1);
    let triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
             WHERE type = 'trigger' AND name LIKE 'account_profiles%'",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(triggers, 3, "the v2 triggers are re-created by the upgrade");
}

/// Migration 0002 adds no default to *any* column, so a row it did not write
/// keeps saying so.
///
/// This is the whole point of the migration: `enabled = 1` would be a launch
/// policy decision and `revision = 1` a concurrency claim, and a schema change
/// is not entitled to make either on a row's behalf. The mutant this kills is
/// re-adding `NOT NULL DEFAULT 1` to those two columns.
#[test]
fn a_migrated_v1_account_profile_carries_no_invented_state() {
    let directory = temp();
    let path = directory.path().join("kontor.db");

    // A genuine v1 file with a genuine v1 account profile: the five columns
    // schema v1 had, and nothing else.
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .execute_batch(MIGRATION_0001)
            .expect("the v1 migration runs");
        connection
            .execute_batch(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, '0193f000-0000-7000-8000-0000000000c2', 1,
                         '2026-08-09T10:00:00Z', NULL);
                 INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                         '2026-08-09T10:00:00Z');
                 INSERT INTO account_profiles
                     (id, project_id, label, external_account_id, created_at)
                 VALUES ('0193f000-0000-7000-8000-0000000000a1',
                         '0193f000-0000-7000-8000-000000000001', 'Legacy', NULL,
                         '2026-08-09T10:00:00Z');",
            )
            .expect("the v1 rows are written");
    }

    let store = SqliteStore::open(&path).expect("the v1 database is upgraded");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);

    // Every column migration 0002 added is NULL on that row — including the two
    // it would have been most tempting to guess.
    let connection = raw(&directory);
    for column in [
        "harness",
        "credential_ref_kind",
        "credential_ref_alias",
        "environment_refs",
        "environment_refs_hash",
        "routing",
        "routing_hash",
        "capability",
        "capability_hash",
        "provider_identity",
        "enabled",
        "revision",
        "updated_at",
    ] {
        let nulls: i64 = connection
            .query_row(
                &format!("SELECT count(*) FROM account_profiles WHERE {column} IS NULL"),
                [],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(
            nulls, 1,
            "migration 0002 must not invent a value for `{column}`"
        );
    }

    // The schema itself declares no default, so even a bare insert of the v1
    // columns would produce NULLs rather than picking them up implicitly.
    let defaults: i64 = connection
        .query_row(
            "SELECT count(*) FROM pragma_table_info('account_profiles')
             WHERE name IN ('enabled', 'revision') AND dflt_value IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(
        defaults, 0,
        "`enabled` and `revision` must carry no column default"
    );
    let not_null: i64 = connection
        .query_row(
            "SELECT count(*) FROM pragma_table_info('account_profiles')
             WHERE name IN ('enabled', 'revision') AND \"notnull\" = 1",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(
        not_null, 0,
        "`enabled` and `revision` must be nullable, so an unwritten row stays unwritten"
    );

    // And the incomplete row is inert rather than half-usable: the repository
    // refuses to load it instead of returning a profile with guessed state.
    let error = store
        .get_account_profile(
            ProjectId::parse("0193f000-0000-7000-8000-000000000001").expect("a canonical id"),
            AccountProfileId::parse("0193f000-0000-7000-8000-0000000000a1")
                .expect("a canonical id"),
        )
        .expect_err("an incomplete profile must not load");
    assert!(
        matches!(error, RepositoryError::Conflict { .. }),
        "expected an incomplete-profile conflict, got {error:?}"
    );
}

/// The other half of the no-default contract: because nothing is defaulted, a
/// new row has to supply every column explicitly, and the insert trigger is what
/// makes that non-optional.
#[test]
fn an_account_profile_insert_without_explicit_state_is_refused() {
    let directory = temp();
    let store = open(&directory);
    drop(store);
    let connection = raw(&directory);
    connection
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z')",
            [],
        )
        .expect("the project is created");

    // Everything the trigger needs except the two columns under test.
    const COMPLETE: &str = "INSERT INTO account_profiles
        (id, project_id, label, created_at, harness, credential_ref_kind,
         credential_ref_alias, environment_refs, environment_refs_hash, routing,
         routing_hash, capability, capability_hash, enabled, revision, updated_at)
     VALUES ('0193f000-0000-7000-8000-0000000000a2',
             '0193f000-0000-7000-8000-000000000001', 'New', '2026-08-09T10:00:00Z',
             'zz.codex', 'config_home', 'zz-alpha', '{}',
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '{}',
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '{}',
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
             ENABLED, REVISION, '2026-08-09T10:00:00Z')";

    // Omitting either one is refused rather than defaulted.
    for (label, enabled, revision) in [
        ("enabled", "NULL", "1"),
        ("revision", "1", "NULL"),
        ("both", "NULL", "NULL"),
    ] {
        let statement = COMPLETE
            .replace("ENABLED", enabled)
            .replace("REVISION", revision);
        connection.execute(&statement, []).unwrap_err_with(label);
    }

    // Supplying both explicitly is accepted, so the refusals above are about the
    // missing values and not about the rest of the statement.
    let statement = COMPLETE.replace("ENABLED", "1").replace("REVISION", "1");
    connection
        .execute(&statement, [])
        .expect("a fully explicit insert is accepted");
}

/// The reader refuses a missing `enabled`/`revision` on their own account, not
/// merely as a side effect of some other column also being absent.
///
/// The migrated-v1 test above cannot show this: that row is missing its harness
/// too, so the load fails before it ever looks at these two. Here the row is
/// complete apart from them — which takes dropping the insert trigger, because
/// the schema will not otherwise let such a row exist. The mutant this kills is
/// a reader that answers `unwrap_or(1)`: a defaulted `enabled = true` would arm
/// a profile for launch that no writer ever enabled.
#[test]
fn a_profile_missing_only_its_enabled_or_revision_still_refuses_to_load() {
    const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
    const PROFILE: &str = "0193f000-0000-7000-8000-0000000000a3";

    for (label, enabled, revision) in [("enabled", "NULL", "1"), ("revision", "1", "NULL")] {
        let directory = temp();
        let path = directory.path().join("kontor.db");
        drop(open(&directory));
        let connection = raw(&directory);
        connection
            .execute_batch(&format!(
                "DROP TRIGGER account_profiles_identity_required;
                 INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES ('{PROJECT}', 'P', '/tmp/p', 1, '2026-08-09T10:00:00Z');
                 INSERT INTO account_profiles
                     (id, project_id, label, created_at, harness, credential_ref_kind,
                      credential_ref_alias, environment_refs, environment_refs_hash,
                      routing, routing_hash, capability, capability_hash,
                      enabled, revision, updated_at)
                 VALUES ('{PROFILE}', '{PROJECT}', 'Half', '2026-08-09T10:00:00Z',
                         'zz.codex', 'config_home', 'zz-alpha',
                         '{{\"schema_version\":1}}',
                         '{HASH}', '{{\"schema_version\":1}}', '{HASH}',
                         '{{\"schema_version\":1}}', '{HASH}',
                         {enabled}, {revision}, '2026-08-09T10:00:00Z');",
                HASH = content_hash_of(r#"{"schema_version":1}"#),
            ))
            .expect("the half-written row is inserted");
        drop(connection);

        let store = SqliteStore::open(&path).expect("the store reopens");
        let error = store
            .get_account_profile(
                ProjectId::parse(PROJECT).expect("a canonical id"),
                AccountProfileId::parse(PROFILE).expect("a canonical id"),
            )
            .unwrap_err();
        assert!(
            matches!(error, RepositoryError::Conflict { .. }),
            "a profile with no `{label}` must refuse to load, got {error:?}"
        );
    }
}

/// The digest the store stores alongside a canonical document.
fn content_hash_of(json: &str) -> String {
    kontor_core::id::ContentHash::of(json.as_bytes())
        .as_str()
        .to_owned()
}

/// `Result::unwrap_err` with a label, so a loop reports *which* case passed when
/// it should have failed.
trait UnwrapErrWith {
    fn unwrap_err_with(self, label: &str);
}

impl<T> UnwrapErrWith for Result<T, rusqlite::Error> {
    fn unwrap_err_with(self, label: &str) {
        assert!(
            self.is_err(),
            "an insert omitting `{label}` must be refused, not defaulted"
        );
    }
}

/// A failure in the *second* migration rolls the first one back with it. Both
/// run in one transaction precisely so a two-step upgrade cannot stop half way.
#[test]
fn a_failure_in_the_second_migration_rolls_the_first_one_back() {
    let directory = temp();
    let path = directory.path().join("kontor.db");

    // A trigger that migration 0002 also creates: the batch fails on the
    // duplicate, after 0001 has already created every table.
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .execute_batch(
                "CREATE TABLE decoy (id TEXT);
                 CREATE TRIGGER account_profiles_identity_required
                 BEFORE INSERT ON decoy BEGIN SELECT 1; END;",
            )
            .expect("the conflicting trigger is created");
    }

    SqliteStore::open(&path).expect_err("the second migration must fail");

    let connection = Connection::open(&path).expect("a raw connection opens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(
        version, 0,
        "a failure in 0002 must roll 0001 back to version zero"
    );
    let tables = table_names(&connection);
    assert!(
        !tables.contains("account_profiles"),
        "0001's tables must have been rolled back with 0002, found {tables:?}"
    );
}

#[test]
fn every_connection_reports_wal_foreign_keys_and_a_bounded_busy_timeout() {
    let directory = temp();
    let store = open(&directory);
    assert_eq!(
        store.journal_mode().expect("readable").to_lowercase(),
        "wal"
    );
    assert!(store.foreign_keys_enabled().expect("readable"));
    assert_eq!(store.busy_timeout_ms().expect("readable"), 15_000);

    // Reopening must re-apply the per-connection pragmas, not inherit them.
    drop(store);
    let reopened = open(&directory);
    assert!(
        reopened.foreign_keys_enabled().expect("readable"),
        "foreign keys must be re-enabled on every connection"
    );
    assert_eq!(reopened.busy_timeout_ms().expect("readable"), 15_000);
    assert_eq!(
        reopened.journal_mode().expect("readable").to_lowercase(),
        "wal"
    );
}

#[test]
fn opening_an_already_migrated_database_is_idempotent() {
    let directory = temp();
    let first = open(&directory);
    let before = table_names(&raw(&directory));
    drop(first);

    let second = open(&directory);
    assert_eq!(second.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(table_names(&raw(&directory)), before);
}

#[test]
fn a_failing_migration_leaves_version_zero_and_no_partial_schema() {
    let directory = temp();
    let path = directory.path().join("kontor.db");

    // Seed a conflicting object so `CREATE TABLE projects` inside the migration
    // fails part-way through the batch.
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .execute_batch("CREATE TABLE projects (unrelated TEXT);")
            .expect("the conflicting table is created");
    }

    let error = SqliteStore::open(&path).expect_err("the migration must fail");
    assert!(
        matches!(error, StoreError::Sqlite(_)),
        "expected a SQLite failure, got {error:?}"
    );

    let connection = Connection::open(&path).expect("a raw connection opens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version is readable");
    assert_eq!(version, 0, "a failed migration must not bump the version");

    let tables = table_names(&connection);
    assert_eq!(
        tables.len(),
        1,
        "only the pre-existing table may remain, found {tables:?}"
    );
    for table in EXPECTED_TABLES {
        if *table == "projects" {
            continue;
        }
        assert!(
            !tables.contains(*table),
            "`{table}` must have been rolled back"
        );
    }
    let triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger'",
            [],
            |row| row.get(0),
        )
        .expect("the catalogue is readable");
    assert_eq!(
        triggers, 0,
        "no trigger may survive a rolled-back migration"
    );
}

#[test]
fn a_newer_schema_is_refused_rather_than_downgraded() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    let too_new = SCHEMA_VERSION + 1;
    {
        let store = SqliteStore::open(&path).expect("the store migrates");
        assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    }
    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        connection
            .pragma_update(None, "user_version", too_new)
            .expect("the version can be forced forward");
    }

    let error = SqliteStore::open(&path).expect_err("a newer schema must be refused");
    match error {
        StoreError::DatabaseTooNew { found, expected } => {
            assert_eq!(found, too_new);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        other => panic!("expected DatabaseTooNew, got {other:?}"),
    }

    // Nothing was truncated on the way out.
    let connection = Connection::open(&path).expect("a raw connection opens");
    assert!(table_names(&connection).contains("projects"));
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(
        version, too_new,
        "a refused open must not rewrite the version"
    );
}

// ---------------------------------------------------------------------------
// Realm identity
// ---------------------------------------------------------------------------

#[test]
fn an_empty_database_creates_exactly_one_immutable_realm() {
    let directory = temp();
    let store = open(&directory);

    let realm = store.realm_metadata();
    assert_eq!(realm.schema_version.get(), 1);
    assert!(
        realm.display_label.is_none(),
        "a freshly initialized realm carries no label"
    );
    assert_eq!(realm.realm_id.as_uuid().get_version_num(), 7);
    // The identity is stable for the lifetime of the store.
    assert_eq!(store.realm_metadata(), realm);
    assert_eq!(store.realm_id(), realm.realm_id);

    let connection = raw(&directory);
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(rows, 1, "exactly one realm row");
    let (singleton, stored_id): (i64, String) = connection
        .query_row(
            "SELECT singleton, realm_id FROM realm_metadata",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("readable");
    assert_eq!(singleton, 1);
    assert_eq!(stored_id, realm.realm_id.to_string());

    // Two separate databases are two separate Realms.
    let other = temp();
    let other_store = open(&other);
    assert_ne!(
        other_store.realm_id(),
        store.realm_id(),
        "each database file is its own realm"
    );
}

#[test]
fn realm_identity_survives_reopen_and_cannot_be_replaced() {
    let directory = temp();
    let original = {
        let store = open(&directory);
        store.realm_metadata().clone()
    };

    for _ in 0..3 {
        let reopened = open(&directory);
        assert_eq!(
            reopened.realm_metadata(),
            &original,
            "reopening must load the same realm byte-for-byte, never regenerate it"
        );
    }

    // Even after the schema is already at v1, nothing re-runs initialization.
    let connection = raw(&directory);
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("readable");
    assert_eq!(rows, 1);
}

#[test]
fn realm_metadata_rejects_update_delete_and_duplicate() {
    let directory = temp();
    let store = open(&directory);
    let original = store.realm_metadata().clone();
    drop(store);
    let connection = raw(&directory);

    for statement in [
        "UPDATE realm_metadata SET realm_id = '0193f000-0000-7000-8000-0000000000ff'",
        "UPDATE realm_metadata SET display_label = 'renamed'",
        "UPDATE realm_metadata SET created_at = '2020-01-01T00:00:00Z'",
        "UPDATE realm_metadata SET schema_version = 1",
        "DELETE FROM realm_metadata",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "realm identity must refuse: {statement}"
        );
    }

    // A second row is impossible: the singleton primary key already holds 1, and
    // any other value fails its check.
    for singleton in ["1", "2"] {
        assert!(
            connection
                .execute(
                    &format!(
                        "INSERT INTO realm_metadata
                             (singleton, realm_id, schema_version, created_at, display_label)
                         VALUES ({singleton}, '0193f000-0000-7000-8000-0000000000fe', 1,
                                 '2026-08-09T10:00:00Z', NULL)"
                    ),
                    [],
                )
                .is_err(),
            "a second realm row must be impossible (singleton {singleton})"
        );
    }
    // An upsert is not a loophole either.
    assert!(
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, '0193f000-0000-7000-8000-0000000000fd', 1, '2026-08-09T10:00:00Z', NULL)
                 ON CONFLICT(singleton) DO UPDATE SET realm_id = excluded.realm_id",
                [],
            )
            .is_err(),
        "an upsert must not replace realm identity"
    );

    // Nothing above changed anything.
    let reopened = open(&directory);
    assert_eq!(reopened.realm_metadata(), &original);
}

// ---------------------------------------------------------------------------
// Contention
// ---------------------------------------------------------------------------

#[test]
fn a_busy_writer_waits_then_times_out_without_partial_state() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    let _store = SqliteStore::open(&path).expect("the store opens");

    let insert = |connection: &Connection, id: &str, root: &str| {
        connection.execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, 'P', ?2, 1, '2026-08-09T10:00:00Z')",
            rusqlite::params![id, root],
        )
    };

    // A writer that releases well inside the timeout is simply waited for.
    {
        let writer = Connection::open(&path).expect("a raw connection opens");
        writer
            .busy_timeout(Duration::from_millis(5_000))
            .expect("timeout applies");

        // The holder owns its whole connection inside the thread: a rusqlite
        // transaction is not `Send`.
        let (locked, is_locked) = std::sync::mpsc::channel();
        let holder_path = path.clone();
        let released = std::thread::spawn(move || {
            let mut holder = Connection::open(&holder_path).expect("a raw connection opens");
            holder
                .busy_timeout(Duration::from_millis(5_000))
                .expect("timeout applies");
            let transaction = holder
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .expect("the lock is taken");
            locked.send(()).expect("the parent is listening");
            std::thread::sleep(Duration::from_millis(250));
            transaction.commit().expect("the holder releases");
        });
        is_locked.recv().expect("the holder takes the lock first");

        insert(&writer, "0193f000-0000-7000-8000-0000000000a1", "/tmp/a1")
            .expect("a released writer is eventually followed");
        released.join().expect("the holder thread finishes");
    }

    // A writer that holds the lock past the deadline yields a typed busy
    // failure, and the blocked write leaves nothing behind.
    let mut holder = Connection::open(&path).expect("a raw connection opens");
    let writer = Connection::open(&path).expect("a raw connection opens");
    writer
        .busy_timeout(Duration::from_millis(5_000))
        .expect("timeout applies");
    let transaction = holder
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("the lock is taken");
    transaction
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-0000000000b1', 'Holder', '/tmp/b1', 1,
                     '2026-08-09T10:00:00Z')",
            [],
        )
        .expect("the holder writes inside its own transaction");

    let started = Instant::now();
    let blocked = insert(&writer, "0193f000-0000-7000-8000-0000000000a2", "/tmp/a2");
    let waited = started.elapsed();

    let error = blocked.expect_err("a writer held past the deadline must fail");
    let busy = matches!(
        &error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::DatabaseBusy
                || failure.code == rusqlite::ErrorCode::DatabaseLocked
    );
    assert!(busy, "expected a typed busy failure, got {error:?}");
    // Deliberately a conservative lower bound rather than an exact duration.
    assert!(
        waited >= Duration::from_millis(4_000),
        "the writer must actually wait for the timeout, waited {waited:?}"
    );

    // Roll the holder back; neither the blocked write nor the holder's own
    // uncommitted row may survive.
    drop(transaction);
    let reader = Connection::open(&path).expect("a raw connection opens");
    let count: i64 = reader
        .query_row(
            "SELECT count(*) FROM projects WHERE id IN
                 ('0193f000-0000-7000-8000-0000000000a2', '0193f000-0000-7000-8000-0000000000b1')",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(count, 0, "a busy failure must leave no partial state");
}

// ---------------------------------------------------------------------------
// Schema shape
// ---------------------------------------------------------------------------

#[test]
fn the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    let found = table_names(&connection);
    let expected: BTreeSet<String> = EXPECTED_TABLES.iter().map(|t| (*t).to_owned()).collect();
    // `sqlite_sequence` is created by AUTOINCREMENT and is filtered out above.
    assert_eq!(found, expected);

    let mut statement = connection
        .prepare(
            "SELECT name FROM pragma_table_list
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
               AND name NOT GLOB 'memory_fts*' AND strict = 0",
        )
        .expect("pragma_table_list is available");
    let lax: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("readable")
        .map(|name| name.expect("a name"))
        .collect();
    assert!(lax.is_empty(), "every table must be STRICT, found {lax:?}");
}

#[test]
fn the_schema_has_no_outbound_comment_representation() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // No table or column anywhere may express an outbound comment. Adding one
    // would have to be a numbered migration, which is exactly the point.
    let mut statement = connection
        .prepare("SELECT name, COALESCE(sql, '') FROM sqlite_master")
        .expect("the catalogue is readable");
    let objects: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("readable")
        .map(|row| row.expect("a catalogue row"))
        .collect();
    for (name, sql) in &objects {
        // Strip comments before scanning: the prose explains the rule, the
        // executable schema must not contain the concept.
        let executable: String = sql
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        assert!(
            !executable.contains("outbound"),
            "`{name}` mentions an outbound comment representation"
        );
    }

    // The only comment policy the projection accepts is the inbound one.
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z');
             INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000010',
                     '0193f000-0000-7000-8000-000000000001', 'T', 'draft', 1,
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z');
             INSERT INTO jira_links
                 (id, project_id, task_id, connector, external_issue_key, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000060',
                     '0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000010', 'connector.alpha', 'ABC-1', 1,
                     '2026-08-09T10:00:00Z');",
        )
        .expect("a ticket link inserts");

    for policy in ["outbound", "bidirectional", "inbound_only "] {
        assert!(
            connection
                .execute(
                    "INSERT INTO ticket_sync_projections
                         (id, project_id, link_id, link_revision, connector, external_issue_key,
                          fields, comment_policy, projection_hash, computed_at)
                     VALUES ('0193f000-0000-7000-8000-000000000061',
                             '0193f000-0000-7000-8000-000000000001',
                             '0193f000-0000-7000-8000-000000000060', 1, 'connector.alpha',
                             'ABC-1', '[]', ?1,
                             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                             '2026-08-09T10:00:00Z')",
                    rusqlite::params![policy],
                )
                .is_err(),
            "`{policy}` must not be a storable comment policy"
        );
    }
}

#[test]
fn the_integrity_and_foreign_key_checks_pass_on_a_fresh_database() {
    let directory = temp();
    let store = open(&directory);
    store.integrity_check().expect("integrity_check passes");
    store.quick_check().expect("quick_check passes");
    store.foreign_key_check().expect("foreign_key_check passes");
}

#[test]
fn strict_typing_and_check_constraints_reject_impossible_rows() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // A STRICT table refuses a value of the wrong storage class.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 'not-a-number', ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "P",
                    "/tmp/p",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err(),
        "STRICT must refuse a text revision"
    );

    // A revision below one is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "P",
                    "/tmp/p",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err()
    );

    // A non-canonical timestamp is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, '2026-08-09 10:00:00')",
                rusqlite::params!["0193f000-0000-7000-8000-000000000001", "P", "/tmp/p"],
            )
            .is_err()
    );

    connection
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000001",
                "P",
                "/tmp/p",
                "2026-08-09T10:00:00Z"
            ],
        )
        .expect("a well-formed project inserts");

    // A duplicate root path is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000002",
                    "Q",
                    "/tmp/p",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err()
    );

    // An unknown task state is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
                 VALUES (?1, ?2, 'T', 'almost_done', 1, ?3, ?3)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000010",
                    "0193f000-0000-7000-8000-000000000001",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err()
    );

    // A dangling parent is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
                 VALUES (?1, ?2, 'T', 'draft', 1, ?3, ?3)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000011",
                    "0193f000-0000-7000-8000-0000000000ff",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err(),
        "foreign keys must be enforced"
    );
}

#[test]
fn a_task_may_not_depend_on_itself() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z');
             INSERT INTO tasks (id, project_id, title, state, revision, created_at, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000010',
                     '0193f000-0000-7000-8000-000000000001', 'T', 'draft', 1,
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z');",
        )
        .expect("the fixture rows insert");

    assert!(
        connection
            .execute(
                "INSERT INTO task_dependencies
                     (project_id, task_id, depends_on_task_id, created_at)
                 VALUES (?1, ?2, ?2, ?3)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "0193f000-0000-7000-8000-000000000010",
                    "2026-08-09T10:00:00Z"
                ],
            )
            .is_err(),
        "a self dependency must be impossible in SQL as well as in Rust"
    );
}

#[test]
fn append_only_tables_reject_update_and_delete_from_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'P', '/tmp/p', 1,
                     '2026-08-09T10:00:00Z');
             INSERT INTO work_profiles
                 (project_id, profile_key, version, definition, definition_hash, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'q7.delivery', 1, '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     '2026-08-09T10:00:00Z');",
        )
        .expect("a specification revision inserts");

    assert!(
        connection
            .execute(
                "UPDATE work_profiles SET definition = '{\"a\":1}' WHERE profile_key = 'q7.delivery'",
                [],
            )
            .is_err(),
        "an immutable revision must refuse UPDATE"
    );
    assert!(
        connection
            .execute(
                "DELETE FROM work_profiles WHERE profile_key = 'q7.delivery'",
                []
            )
            .is_err(),
        "an immutable revision must refuse DELETE"
    );

    // A duplicate (id, version) is impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO work_profiles
                     (project_id, profile_key, version, definition, definition_hash, created_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'q7.delivery', 1, '{}',
                         'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                         '2026-08-09T10:00:00Z')",
                [],
            )
            .is_err()
    );
}

#[test]
fn the_runtime_event_cursor_is_monotonic_and_never_reused() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    let insert = |native: &str| -> i64 {
        connection
            .execute(
                "INSERT INTO runtime_events
                     (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                      native_id, native_event_id, native_sequence, payload, payload_hash,
                      observed_at, recorded_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                         '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                         'session-abc', ?1, 1, '{}',
                         'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                         '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
                rusqlite::params![native],
            )
            .expect("an event appends");
        connection.last_insert_rowid()
    };

    let first = insert("n-1");
    let second = insert("n-2");
    assert!(second > first);

    // The same native event id inside the same generation is a duplicate.
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_events
                     (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                      native_id, native_event_id, native_sequence, payload, payload_hash,
                      observed_at, recorded_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                         '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                         'session-abc', 'n-1', 2, '{}',
                         'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
                         '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
                [],
            )
            .is_err()
    );

    assert!(
        connection
            .execute(
                "DELETE FROM runtime_events WHERE cursor = ?1",
                rusqlite::params![first]
            )
            .is_err(),
        "the event log is append-only"
    );
    assert!(
        connection
            .execute(
                "UPDATE runtime_events SET payload = '{}' WHERE cursor = ?1",
                rusqlite::params![first]
            )
            .is_err()
    );

    // After a generation change the same native event id is a different event.
    let third = connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                  native_id, native_event_id, native_sequence, payload, payload_hash,
                  observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 2,
                     'session-abc', 'n-1', 3, '{}',
                     'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            [],
        )
        .map(|_| connection.last_insert_rowid())
        .expect("a new generation is a new event");
    assert!(third > second);

    // And a native event id is the runtime's numbering *inside one session*, so
    // two sessions of one generation may both call their first event `n-1`. A key
    // without the session would collapse them and lose one run's evidence.
    let fourth = connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                  native_id, native_event_id, native_sequence, payload, payload_hash,
                  observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                     'session-def', 'n-1', 1, '{}',
                     'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            [],
        )
        .map(|_| connection.last_insert_rowid())
        .expect("another session's `n-1` is another event");
    assert!(fourth > third);
}

/// One normalized control-plane observation, as direct SQL.
///
/// `native_event_id` is optional because plenty of runtimes number nothing: those
/// observations are identified by their native sequence alone, and the schema has
/// to recognize them by it.
fn control_observation(
    connection: &Connection,
    native_event_id: Option<&str>,
    sequence: i64,
    hash: &str,
    normalized: bool,
) -> rusqlite::Result<i64> {
    connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                  native_id, native_event_id, native_sequence, observed_state, contact, freshness,
                  audit_ref, payload, payload_hash, observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                     'session-abc', ?1, ?2, 'running', ?3, ?4, ?5, '{}', ?6,
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            rusqlite::params![
                native_event_id,
                sequence,
                normalized.then_some("reachable"),
                normalized.then_some("fresh"),
                normalized.then_some("audit-1"),
                hash
            ],
        )
        .map(|_| connection.last_insert_rowid())
}

#[test]
fn a_normalized_observation_cannot_be_stored_in_pieces_or_claim_a_used_sequence() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    let hash = |c: char| c.to_string().repeat(64);
    control_observation(&connection, Some("n-1"), 1, &hash('a'), true)
        .expect("a complete normalized observation stores");

    // The continuity identity: one native sequence per session per generation.
    // A second row claiming it — even under a different native event id — is a
    // conflict, not a second truth.
    assert!(
        control_observation(&connection, Some("n-1b"), 1, &hash('b'), true).is_err(),
        "two normalized observations must not claim one native sequence"
    );

    // Half a normalized observation is not storable: an effect derived from
    // contact or freshness would otherwise have no evidence to cite.
    for (contact, freshness, audit) in [
        (Some("reachable"), None::<&str>, Some("audit-1")),
        (None, Some("fresh"), Some("audit-1")),
        (Some("reachable"), Some("fresh"), None),
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO runtime_events
                         (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                          native_id, native_event_id, native_sequence, observed_state, contact,
                          freshness, audit_ref, payload, payload_hash, observed_at, recorded_at)
                     VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                             '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1',
                             1, 'session-abc', 'n-partial', 9, 'running', ?1, ?2, ?3, '{}', ?4,
                             '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
                    rusqlite::params![contact, freshness, audit, hash('c')],
                )
                .is_err(),
            "a normalized observation is all of it or none of it"
        );
    }

    // A normalized row without the state it normalizes is equally impossible.
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_events
                     (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                      native_id, native_event_id, native_sequence, contact, freshness, audit_ref,
                      payload, payload_hash, observed_at, recorded_at)
                 VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                         '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                         'session-abc', 'n-stateless', 11, 'reachable', 'fresh', 'audit-1', '{}',
                         ?1, '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
                rusqlite::params![hash('d')],
            )
            .is_err(),
        "a normalized observation must carry the state a reduction reads"
    );

    // The same native sequence on another host is a different session, and
    // deduplication must not swallow it.
    connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation,
                  native_id, native_event_id, native_sequence, observed_state, contact, freshness,
                  audit_ref, payload, payload_hash, observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-2', 1,
                     'session-abc', 'n-1', 1, 'running', 'reachable', 'fresh', 'audit-2', '{}',
                     ?1, '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            rusqlite::params![hash('e')],
        )
        .expect("another host is another session");
}

#[test]
fn an_observation_with_no_native_id_is_identified_by_its_sequence_not_its_payload() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    let same = "a".repeat(64);
    control_observation(&connection, None, 5, &same, true)
        .expect("an observation with no id of its own stores");

    // Two contacts with the runtime that happened to report the same thing. The
    // payload digest is identical and the observations are not: deduplicating on
    // the digest would throw the second one away, and with it the evidence that
    // the runtime was still answering at sequence 6.
    control_observation(&connection, None, 6, &same, true)
        .expect("an identical payload at a later sequence is a distinct observation");

    // What *is* identity: the native sequence in this session and generation. A
    // second row claiming sequence 6 is refused whether its payload matches or
    // not, so one moment never carries two stories.
    for payload in [&same, &"b".repeat(64)] {
        assert!(
            control_observation(&connection, None, 6, payload, true).is_err(),
            "one native sequence carries one observation"
        );
    }

    // The same holds for a bare, un-normalized observation: with no id of its own
    // it is still recognized by its sequence rather than by its bytes.
    control_observation(&connection, None, 7, &same, false)
        .expect("a bare observation with no id stores");
    assert!(
        control_observation(&connection, None, 7, &"c".repeat(64), false).is_err(),
        "one native sequence carries one observation, normalized or not"
    );

    let stored: i64 = connection
        .query_row(
            "SELECT count(*) FROM runtime_events WHERE native_event_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(stored, 3, "three distinct sequences, three rows");
}

/// What a reader may rely on when an appender allocates a cursor and then dies.
///
/// Note what is deliberately *not* claimed. SQLite keeps the AUTOINCREMENT
/// counter in `sqlite_sequence`, an ordinary table that is rolled back with the
/// transaction that moved it — so the integer a doomed append was handed can
/// legally be issued again, and this test captures it to prove that rather than
/// looking away. The guarantee readers depend on is narrower and does hold: a
/// *committed* cursor is never reissued, and a rolled-back one was never
/// committed and so never delivered to anybody, which is why reusing it costs no
/// subscriber a delivery and duplicates none.
#[test]
fn rolled_back_cursor_is_never_reused_after_reopen() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    let hash = |c: char| c.to_string().repeat(64);

    let committed = control_observation(&connection, Some("n-1"), 1, &hash('a'), true)
        .expect("the first observation commits");

    // A second appender allocates, then dies. Capture the cursor it was handed: a
    // test that never looks at it cannot prove anything about reuse.
    let doomed = {
        let transaction = connection
            .unchecked_transaction()
            .expect("a transaction opens");
        let allocated = control_observation(&connection, Some("n-2"), 2, &hash('b'), true)
            .expect("the doomed observation allocates a cursor");
        transaction.rollback().expect("the transaction rolls back");
        allocated
    };
    assert!(
        doomed > committed,
        "the doomed append allocated ahead of the committed one"
    );
    drop(connection);

    // Reopen the file, exactly as a restarted daemon would.
    let _reopened = open(&directory);
    let connection = raw(&directory);
    let next = control_observation(&connection, Some("n-3"), 3, &hash('c'), true)
        .expect("a later observation commits");

    assert!(
        next > committed,
        "a committed cursor is never handed out twice"
    );
    let rolled_back: i64 = connection
        .query_row(
            "SELECT count(*) FROM runtime_events WHERE native_event_id = 'n-2'",
            [],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(rolled_back, 0, "a rolled-back append leaves no row behind");

    // The rolled-back allocation left no claim on its cursor either. If that
    // cursor comes round again — and under SQLite's rollback of `sqlite_sequence`
    // it does — it belongs to the committed event and to nothing else.
    let occupant: Option<String> = connection
        .query_row(
            "SELECT native_event_id FROM runtime_events WHERE cursor = ?1",
            rusqlite::params![doomed],
            |row| row.get(0),
        )
        .optional()
        .expect("readable");
    assert_ne!(
        occupant.as_deref(),
        Some("n-2"),
        "a rolled-back append never owns a cursor"
    );
    assert_eq!(
        occupant.is_some(),
        next == doomed,
        "the reissued cursor, when it is reissued, is the committed event's"
    );

    // The counter itself never sits below the newest committed cursor, so the next
    // allocation cannot land on one a subscriber has already been shown.
    let sequence: i64 = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'runtime_events'",
            [],
            |row| row.get(0),
        )
        .expect("the sequence is readable");
    assert_eq!(
        sequence, next,
        "the cursor sequence never regresses below what is committed"
    );

    // Every committed cursor is distinct and ascending, so a subscriber that
    // resumes after `committed` is delivered the later event exactly once.
    let mut statement = connection
        .prepare("SELECT cursor FROM runtime_events ORDER BY cursor")
        .expect("readable");
    let cursors: Vec<i64> = statement
        .query_map([], |row| row.get(0))
        .expect("readable")
        .map(|cursor| cursor.expect("a cursor"))
        .collect();
    let distinct: BTreeSet<i64> = cursors.iter().copied().collect();
    assert_eq!(distinct.len(), cursors.len(), "cursors are unique");
    assert!(cursors.windows(2).all(|pair| pair[0] < pair[1]));
    let after: Vec<i64> = cursors
        .iter()
        .copied()
        .filter(|cursor| *cursor > committed)
        .collect();
    assert_eq!(
        after,
        vec![next],
        "resuming after a committed cursor delivers each later event once"
    );
}

#[test]
fn a_replay_checkpoint_and_a_finished_census_never_move_backwards() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    connection
        .execute(
            "INSERT INTO runtime_replay_consumers
                 (project_id, consumer_key, last_cursor, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'projector', 5,
                     '2026-08-09T10:00:00Z')",
            [],
        )
        .expect("a checkpoint stores");
    assert!(
        connection
            .execute(
                "UPDATE runtime_replay_consumers SET last_cursor = 4 WHERE consumer_key = 'projector'",
                [],
            )
            .is_err(),
        "a checkpoint must not rewind"
    );
    connection
        .execute(
            "UPDATE runtime_replay_consumers SET last_cursor = 9 WHERE consumer_key = 'projector'",
            [],
        )
        .expect("a checkpoint may advance");

    // A census that has settled keeps the moment it was taken at.
    connection
        .execute(
            "INSERT INTO runtime_reconciliation_epochs
                 (epoch_id, project_id, runtime_kind, host, generation, reconciliation_key,
                  census_start_cursor, started_at, status)
             VALUES ('0193f000-0000-7000-8000-0000000000e1',
                     '0193f000-0000-7000-8000-000000000001', 'generic.runtime', 'host-1', 1,
                     'sweep-1', 3, '2026-08-09T10:00:00Z', 'in_progress')",
            [],
        )
        .expect("an epoch begins");
    assert!(
        connection
            .execute(
                "UPDATE runtime_reconciliation_epochs SET census_start_cursor = 2
                 WHERE epoch_id = '0193f000-0000-7000-8000-0000000000e1'",
                [],
            )
            .is_err(),
        "a census start position is fixed when the census begins"
    );
    // A completed census must record where it completed; a failed one must not
    // be able to claim it was authoritative.
    assert!(
        connection
            .execute(
                "UPDATE runtime_reconciliation_epochs
                 SET status = 'completed', completed_at = '2026-08-09T11:00:00Z'
                 WHERE epoch_id = '0193f000-0000-7000-8000-0000000000e1'",
                [],
            )
            .is_err(),
        "a completed census must record its completion cursor"
    );
    connection
        .execute(
            "UPDATE runtime_reconciliation_epochs
             SET status = 'completed', completed_at = '2026-08-09T11:00:00Z',
                 completion_cursor = 7
             WHERE epoch_id = '0193f000-0000-7000-8000-0000000000e1'",
            [],
        )
        .expect("a census completes");
    assert!(
        connection
            .execute(
                "UPDATE runtime_reconciliation_epochs SET status = 'failed'
                 WHERE epoch_id = '0193f000-0000-7000-8000-0000000000e1'",
                [],
            )
            .is_err(),
        "a settled census is immutable"
    );
}

#[test]
fn a_content_gap_has_nowhere_to_put_session_content() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // The table has no column for a transcript, a message, a token count or a
    // delta, and none of the three referenced columns is one either.
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info('runtime_content_gaps')")
        .expect("readable");
    let columns: BTreeSet<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("readable")
        .map(|name| name.expect("a column name"))
        .collect();
    for forbidden in [
        "payload",
        "content",
        "transcript",
        "message",
        "messages",
        "body",
        "text",
        "tokens",
        "token_delta",
        "usage",
    ] {
        assert!(
            !columns.contains(forbidden),
            "`runtime_content_gaps` must not be able to store `{forbidden}`"
        );
    }

    // A content gap never points at closure evidence: it has no foreign key to
    // a receipt or to an event, so it cannot be cited as a reason a run ended.
    let mut statement = connection
        .prepare("SELECT \"table\" FROM pragma_foreign_key_list('runtime_content_gaps')")
        .expect("readable");
    let targets: BTreeSet<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("readable")
        .map(|name| name.expect("a table name"))
        .collect();
    assert!(
        !targets.contains("command_receipts") && !targets.contains("runtime_events"),
        "a content gap must not reference closure evidence"
    );

    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    // The one text column is an opaque reference. Prose cannot be smuggled
    // through it.
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_content_gaps
                     (id, project_id, agent_run_id, content_epoch, expected_content_sequence,
                      received_content_sequence, detected_cursor, audit_ref, detected_at)
                 VALUES ('0193f000-0000-7000-8000-0000000000f1',
                         '0193f000-0000-7000-8000-000000000001',
                         '0193f000-0000-7000-8000-000000000040', 1, 4, 9, 2,
                         'the user asked me to delete the production database',
                         '2026-08-09T10:00:00Z')",
                [],
            )
            .is_err(),
        "an audit reference is an opaque token, not a place for content"
    );
}

#[test]
fn a_terminal_run_cannot_be_reopened_or_edited_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    // A terminal lifecycle without evidence is impossible.
    assert!(
        connection
            .execute(
                "UPDATE agent_runs SET lifecycle = 'succeeded'
                 WHERE id = '0193f000-0000-7000-8000-000000000040'",
                [],
            )
            .is_err(),
        "closing a run without evidence must be impossible"
    );

    connection
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, agent_run_id, runtime_kind, host, generation, native_id,
                  native_event_id, native_sequence, observed_state, payload, payload_hash,
                  observed_at, recorded_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'runtime_observation',
                     '0193f000-0000-7000-8000-000000000040', 'generic.runtime', 'host-1', 1,
                     'session-abc', 'n-close', 9, 'succeeded', '{}',
                     'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                     '2026-08-09T10:00:00Z', '2026-08-09T10:00:01Z')",
            [],
        )
        .expect("the terminal event inserts");
    let cursor = connection.last_insert_rowid();
    connection
        .execute(
            "UPDATE agent_runs
             SET lifecycle = 'succeeded', derived_state = 'terminal',
                 terminal_outcome = 'succeeded', terminal_source_kind = 'runtime_observation',
                 terminal_event_cursor = ?1,
                 terminal_evidence_hash =
                     'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                 closed_at = '2026-08-09T11:00:00Z'
             WHERE id = '0193f000-0000-7000-8000-000000000040'",
            rusqlite::params![cursor],
        )
        .expect("an evidenced closure succeeds");

    for statement in [
        "UPDATE agent_runs SET lifecycle = 'running' WHERE id = '0193f000-0000-7000-8000-000000000040'",
        "UPDATE agent_runs SET terminal_evidence_hash = 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff' WHERE id = '0193f000-0000-7000-8000-000000000040'",
        "UPDATE agent_runs SET closed_at = NULL WHERE id = '0193f000-0000-7000-8000-000000000040'",
        "DELETE FROM agent_runs WHERE id = '0193f000-0000-7000-8000-000000000040'",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "a closed run must refuse: {statement}"
        );
    }
}

#[test]
fn a_terminal_team_run_cannot_be_reopened_or_edited_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    const TEAM_RUN: &str = "0193f000-0000-7000-8000-000000000035";

    // A terminal lifecycle without a closure time and evidence is impossible.
    assert!(
        connection
            .execute(
                "UPDATE team_runs SET lifecycle = 'succeeded' WHERE id = ?1",
                rusqlite::params![TEAM_RUN],
            )
            .is_err(),
        "closing a team run without evidence must be impossible"
    );
    assert!(
        connection
            .execute(
                "UPDATE team_runs SET lifecycle = 'succeeded', closed_at = '2026-08-09T11:00:00Z'
                 WHERE id = ?1",
                rusqlite::params![TEAM_RUN],
            )
            .is_err(),
        "a closure time alone is not evidence"
    );

    connection
        .execute(
            "UPDATE team_runs
             SET lifecycle = 'succeeded', terminal_outcome = 'succeeded',
                 terminal_source_kind = 'child_evidence',
                 terminal_evidence_hash =
                     'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                 closed_at = '2026-08-09T11:00:00Z'
             WHERE id = ?1",
            rusqlite::params![TEAM_RUN],
        )
        .expect("an evidenced closure succeeds");

    for statement in [
        "UPDATE team_runs SET lifecycle = 'running' WHERE id = ?1",
        "UPDATE team_runs SET lifecycle = 'queued' WHERE id = ?1",
        "UPDATE team_runs SET terminal_evidence_hash = 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff' WHERE id = ?1",
        "UPDATE team_runs SET terminal_outcome = 'failed' WHERE id = ?1",
        "UPDATE team_runs SET closed_at = NULL WHERE id = ?1",
        "UPDATE team_runs SET revision = revision + 1 WHERE id = ?1",
        "DELETE FROM team_runs WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TEAM_RUN])
                .is_err(),
            "a closed team run must refuse: {statement}"
        );
    }
}

#[test]
fn a_pinned_snapshot_cannot_be_rewritten_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    const WORKFLOW: &str = "0193f000-0000-7000-8000-000000000030";
    const TEAM_RUN: &str = "0193f000-0000-7000-8000-000000000035";

    // The work-profile snapshot a task is running is frozen.
    for statement in [
        "UPDATE task_workflows SET snapshot = '{\"a\":1}' WHERE id = ?1",
        "UPDATE task_workflows SET snapshot_hash =
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' WHERE id = ?1",
        "UPDATE task_workflows SET profile_key = 'other.profile' WHERE id = ?1",
        "UPDATE task_workflows SET profile_version = 2 WHERE id = ?1",
        "DELETE FROM task_workflows WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![WORKFLOW])
                .is_err(),
            "a pinned work profile snapshot must refuse: {statement}"
        );
    }

    // Advancing the phase and the revision is exactly what a live workflow is
    // allowed to do, so the trigger is not simply blocking every update.
    connection
        .execute(
            "UPDATE task_workflows SET current_phase = 'q7.shape', revision = revision + 1
             WHERE id = ?1",
            rusqlite::params![WORKFLOW],
        )
        .expect("a live workflow may advance");

    // The team definition a run started with is frozen the same way, and it is
    // frozen while the run is still open — not only once it has closed.
    for statement in [
        "UPDATE team_runs SET snapshot = '{\"a\":1}' WHERE id = ?1",
        "UPDATE team_runs SET snapshot_hash =
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' WHERE id = ?1",
        "UPDATE team_runs SET template_version = 2 WHERE id = ?1",
        "UPDATE team_runs SET task_id = '0193f000-0000-7000-8000-0000000000ee' WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TEAM_RUN])
                .is_err(),
            "a pinned team snapshot must refuse: {statement}"
        );
    }

    // A still-open team run may still move its lifecycle forward.
    connection
        .execute(
            "UPDATE team_runs SET lifecycle = 'waiting_input' WHERE id = ?1",
            rusqlite::params![TEAM_RUN],
        )
        .expect("an open team run may change lifecycle");

    // The persona snapshot table is immutable outright.
    connection
        .execute_batch(
            "INSERT INTO persona_scenarios
                 (project_id, scenario_id, version, persona_key, gate_key, definition,
                  definition_hash, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000050', 1, 'persona.x', 'zz.gate', '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     '2026-08-09T10:00:00Z');
             INSERT INTO task_persona_snapshots
                 (project_id, task_id, scenario_id, version, workflow_id, gate_key, snapshot,
                  snapshot_hash, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000010',
                     '0193f000-0000-7000-8000-000000000050', 1,
                     '0193f000-0000-7000-8000-000000000030', 'zz.gate', '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     '2026-08-09T10:00:00Z');",
        )
        .expect("a persona snapshot inserts");
    assert!(
        connection
            .execute(
                "UPDATE task_persona_snapshots SET snapshot = '{\"a\":1}'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM task_persona_snapshots", [])
            .is_err()
    );
}

#[test]
fn a_terminal_task_cannot_be_changed_by_direct_sql() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    const TASK: &str = "0193f000-0000-7000-8000-000000000010";

    // An open task moves freely.
    connection
        .execute(
            "UPDATE tasks SET state = 'blocked', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("an open task may change state");

    connection
        .execute(
            "UPDATE tasks SET state = 'done', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("a task may close");

    for statement in [
        "UPDATE tasks SET state = 'in_progress' WHERE id = ?1",
        "UPDATE tasks SET state = 'cancelled' WHERE id = ?1",
        "UPDATE tasks SET title = 'renamed' WHERE id = ?1",
        "UPDATE tasks SET revision = revision + 1 WHERE id = ?1",
        "DELETE FROM tasks WHERE id = ?1",
        // The reopen exception is `done -> ready` and *only* the state: an update
        // that also renamed the task or moved it to another epic is a rewrite.
        "UPDATE tasks SET state = 'ready', title = 'renamed' WHERE id = ?1",
        "UPDATE tasks SET state = 'ready', created_at = '2020-01-01T00:00:00Z' WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TASK])
                .is_err(),
            "a terminal task must refuse: {statement}"
        );
    }

    // The one exception the schema allows, because the domain has a rule for it:
    // a completed task returns to `ready`.
    connection
        .execute(
            "UPDATE tasks SET state = 'ready', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("a completed task may be reopened to ready");

    // The exception is not one-shot: a reopened task that completes again reopens
    // again, because the rule is about the pair of states and not about a count.
    connection
        .execute(
            "UPDATE tasks SET state = 'done', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("a reopened task may complete again");
    connection
        .execute(
            "UPDATE tasks SET state = 'ready', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("and be reopened again");

    // A failed task has a successor, not a second life. `cancelled` is the same
    // clause of the same trigger, and `TaskState::is_reopenable` is asserted over
    // both in the domain oracle — this row can only be closed one way, because
    // once it is closed as `failed` nothing moves it again, which is the point.
    connection
        .execute(
            "UPDATE tasks SET state = 'failed', revision = revision + 1 WHERE id = ?1",
            rusqlite::params![TASK],
        )
        .expect("an open task may fail");
    for statement in [
        "UPDATE tasks SET state = 'ready', revision = revision + 1 WHERE id = ?1",
        "UPDATE tasks SET state = 'done', revision = revision + 1 WHERE id = ?1",
    ] {
        assert!(
            connection
                .execute(statement, rusqlite::params![TASK])
                .is_err(),
            "a failed task must refuse: {statement}"
        );
    }
}

#[test]
fn a_derived_state_may_only_be_terminal_together_with_an_outcome() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    // Uncertainty is representable and never terminal.
    for uncertain in [
        "pending_confirmation",
        "stale",
        "diverged",
        "runtime_unavailable",
        "orphaned",
        "lost_contact",
    ] {
        connection
            .execute(
                "UPDATE agent_runs SET derived_state = ?1
                 WHERE id = '0193f000-0000-7000-8000-000000000040'",
                [uncertain],
            )
            .unwrap_or_else(|_| panic!("`{uncertain}` must be storable"));
    }

    // `terminal` without an outcome is impossible.
    assert!(
        connection
            .execute(
                "UPDATE agent_runs SET derived_state = 'terminal'
                 WHERE id = '0193f000-0000-7000-8000-000000000040'",
                [],
            )
            .is_err(),
        "a derived terminal state requires an outcome"
    );
}

#[test]
fn only_one_active_workflow_and_one_active_calendar_may_exist() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");

    assert!(
        connection
            .execute(
                "INSERT INTO task_workflows
                     (id, project_id, task_id, profile_key, profile_version, snapshot,
                      snapshot_hash, current_phase, active, revision, created_at)
                 VALUES ('0193f000-0000-7000-8000-000000000031',
                         '0193f000-0000-7000-8000-000000000001',
                         '0193f000-0000-7000-8000-000000000010', 'q7.delivery', 1, '{}',
                         'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                         'q7.capture', 1, 1, '2026-08-09T10:00:00Z')",
                [],
            )
            .is_err(),
        "a task may have only one active workflow"
    );
}

#[test]
fn all_logical_relationships_are_project_scoped_and_fk_backed() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);

    // Every logical relationship the plan names, spelled out: source table,
    // source columns, target table, target columns.
    //
    // This is an enumeration on purpose. A count ("at least N composite keys")
    // passes just as happily when the wrong N keys are present, so removing one
    // required relationship and adding an unrelated one would go unnoticed. Here
    // a missing key names itself.
    //
    // Order within a key matters and is asserted, because
    // `(project_id, task_id) -> tasks(project_id, id)` and a key that happens to
    // mention the same two columns in another order are not the same constraint.
    type Relationship = (
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static [&'static str],
    );
    const REQUIRED: &[Relationship] = &[
        // --- structure -----------------------------------------------------
        ("mini_projects", &["project_id"], "projects", &["id"]),
        (
            "tasks",
            &["project_id", "mini_project_id"],
            "mini_projects",
            &["project_id", "id"],
        ),
        (
            "task_dependencies",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_dependencies",
            &["project_id", "depends_on_task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        // --- pinned specification revisions --------------------------------
        (
            "task_workflows",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_workflows",
            &["project_id", "profile_key", "profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "task_gate_evaluations",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        (
            "task_gate_evaluations",
            &["project_id", "evaluator_account"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "task_persona_snapshots",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_persona_snapshots",
            &["project_id", "scenario_id", "version"],
            "persona_scenarios",
            &["project_id", "scenario_id", "version"],
        ),
        (
            "task_persona_snapshots",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        // --- trigger pins ---------------------------------------------------
        (
            "trigger_specs",
            &["project_id", "work_profile_key", "work_profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "trigger_specs",
            &["project_id", "team_template_id", "team_template_version"],
            "team_templates",
            &["project_id", "template_id", "version"],
        ),
        (
            "trigger_specs",
            &["calendar_profile_id", "calendar_version"],
            "calendar_profiles",
            &["profile_id", "version"],
        ),
        // --- intake ---------------------------------------------------------
        ("source_events", &["project_id"], "projects", &["id"]),
        (
            "intake_receipts",
            &["project_id", "source_event_id"],
            "source_events",
            &["project_id", "id"],
        ),
        (
            "intake_receipts",
            &["project_id", "trigger_key", "trigger_version"],
            "trigger_specs",
            &["project_id", "trigger_key", "version"],
        ),
        (
            "intake_receipts",
            &["project_id", "predecessor_receipt_id"],
            "intake_receipts",
            &["project_id", "id"],
        ),
        // --- calendar and authorization -------------------------------------
        (
            "work_calendars",
            &["profile_id", "profile_version"],
            "calendar_profiles",
            &["profile_id", "version"],
        ),
        (
            "calendar_exceptions",
            &["project_id", "work_calendar_id"],
            "work_calendars",
            &["project_id", "id"],
        ),
        (
            "execution_authorizations",
            &["project_id", "scope_task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "execution_authorizations",
            &["project_id", "created_by"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "execution_authorizations",
            &["project_id", "capability_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "execution_authorization_tasks",
            &["project_id", "authorization_id"],
            "execution_authorizations",
            &["project_id", "id"],
        ),
        (
            "execution_authorization_tasks",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "approved_by"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "approval_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "revoked_by"],
            "account_profiles",
            &["project_id", "id"],
        ),
        (
            "schedule_overrides",
            &["project_id", "revocation_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        // --- runs, bindings and events --------------------------------------
        (
            "team_runs",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "team_runs",
            &["project_id", "template_id", "template_version"],
            "team_templates",
            &["project_id", "template_id", "version"],
        ),
        (
            "agent_runs",
            &["project_id", "team_run_id"],
            "team_runs",
            &["project_id", "id"],
        ),
        (
            "agent_runs",
            &["project_id", "parent_agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "agent_runs",
            &["project_id", "terminal_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "runtime_bindings",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_events",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_events",
            &["project_id", "command_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "guardrail_evaluations",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "resource_leases",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "resource_leases",
            &["project_id", "release_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "handoffs",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        (
            "handoffs",
            &["project_id", "context_pack_id"],
            "context_packs",
            &["project_id", "id"],
        ),
        (
            "context_packs",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        // --- external tickets -----------------------------------------------
        (
            "jira_links",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "external_workflow_specs",
            &["project_id", "work_profile_key", "work_profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "ticket_sync_projections",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "ticket_sync_projections",
            &[
                "project_id",
                "connector",
                "field_spec_project",
                "field_spec_issue_type",
                "field_spec_version",
            ],
            "ticket_field_specs",
            &[
                "project_id",
                "connector",
                "external_project",
                "issue_type",
                "version",
            ],
        ),
        (
            "external_comments",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "external_ticket_observations",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "status_transition_receipts",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "status_transition_receipts",
            &["project_id", "prior_observation_id"],
            "external_ticket_observations",
            &["project_id", "id"],
        ),
        (
            "status_conflicts",
            &["project_id", "link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "status_conflicts",
            &["project_id", "observation_id"],
            "external_ticket_observations",
            &["project_id", "id"],
        ),
        (
            "status_conflicts",
            &["project_id", "resolution_receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        // --- commands --------------------------------------------------------
        (
            "command_outbox",
            &["project_id", "receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_team_run_id"],
            "team_runs",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_ticket_link_id"],
            "jira_links",
            &["project_id", "id"],
        ),
        (
            "command_targets",
            &["project_id", "target_work_calendar_id"],
            "work_calendars",
            &["project_id", "id"],
        ),
        (
            "command_receipt_transitions",
            &["project_id", "receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        // --- runtime consistency ---------------------------------------------
        (
            "runtime_control_gaps",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_control_gaps",
            &["project_id", "detected_cursor"],
            "runtime_events",
            &["project_id", "cursor"],
        ),
        (
            "runtime_content_gaps",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_reconciliation_members",
            &["project_id", "epoch_id"],
            "runtime_reconciliation_epochs",
            &["project_id", "epoch_id"],
        ),
        (
            "runtime_reconciliation_members",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
        (
            "runtime_reconciliation_members",
            &["project_id", "observation_cursor"],
            "runtime_events",
            &["project_id", "cursor"],
        ),
        (
            "runtime_reconciliation_results",
            &["project_id", "epoch_id"],
            "runtime_reconciliation_epochs",
            &["project_id", "epoch_id"],
        ),
        (
            "runtime_reconciliation_results",
            &["project_id", "agent_run_id"],
            "agent_runs",
            &["project_id", "id"],
        ),
    ];

    /// Every foreign key on `table`, as (target table, from-columns, to-columns).
    fn foreign_keys(
        connection: &Connection,
        table: &str,
    ) -> Vec<(String, Vec<String>, Vec<String>)> {
        let mut statement = connection
            .prepare(
                "SELECT id, seq, \"table\", \"from\", \"to\"
                 FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
            )
            .expect("the catalogue is readable");
        let rows: Vec<(i64, String, String, Option<String>)> = statement
            .query_map(rusqlite::params![table], |row| {
                Ok((row.get(0)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .expect("the catalogue is readable")
            .map(|row| row.expect("a foreign key row"))
            .collect();
        let mut grouped: std::collections::BTreeMap<i64, (String, Vec<String>, Vec<String>)> =
            std::collections::BTreeMap::new();
        for (id, target, from, to) in rows {
            let entry = grouped
                .entry(id)
                .or_insert_with(|| (target, Vec::new(), Vec::new()));
            entry.1.push(from);
            // A NULL `to` means the key targets the primary key of the target.
            entry.2.push(to.unwrap_or_default());
        }
        grouped.into_values().collect()
    }

    let mut missing = Vec::new();
    for (table, from, target, to) in REQUIRED {
        let keys = foreign_keys(&connection, table);
        let found = keys.iter().any(|(actual_target, actual_from, actual_to)| {
            actual_target == target
                && actual_from.as_slice() == *from
                && actual_to.as_slice() == *to
        });
        if !found {
            missing.push(format!(
                "{table} ({}) -> {target} ({})",
                from.join(", "),
                to.join(", ")
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "these required relationships are not foreign-key backed:\n  {}",
        missing.join("\n  ")
    );

    // Every relationship above is also *project-scoped*, except the two that
    // genuinely are not: calendar profiles are workspace-level, so their pins
    // carry no `project_id`. Naming the exceptions explicitly means a third one
    // cannot appear by accident.
    const WORKSPACE_LEVEL: &[(&str, &str)] = &[
        ("trigger_specs", "calendar_profiles"),
        ("work_calendars", "calendar_profiles"),
    ];
    for (table, from, target, _to) in REQUIRED {
        // A reference to `projects` *is* the scope; there is nothing to
        // compose it with.
        if WORKSPACE_LEVEL.contains(&(table, target)) || *target == "projects" {
            continue;
        }
        assert_eq!(
            from.first().copied(),
            Some("project_id"),
            "{table} -> {target} must lead with project_id"
        );
        assert!(
            from.len() > 1,
            "{table} -> {target} must be composite: a single-column key would let a \
             globally valid UUID from another project resolve"
        );
    }

    // The two normalization tables the audit required exist and are keyed the
    // way the plan specifies.
    for (table, key) in [
        ("command_targets", "receipt_id"),
        ("execution_authorization_tasks", "authorization_id"),
    ] {
        let present: i64 = connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
                rusqlite::params![table, key],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(present, 1, "`{table}` must key on `{key}`");
    }

    // A command target names exactly one typed id.
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the run fixture inserts");
    connection
        .execute_batch(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at)
             VALUES ('0193f000-0000-7000-8000-000000000070',
                     '0193f000-0000-7000-8000-000000000001', 'k-1', 'resume_task', '{}', 1, '{}',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     'intent_persisted', 0, '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z');",
        )
        .expect("a receipt inserts");

    // Zero typed ids, and two typed ids, are both impossible.
    for tail in [
        "'task', NULL, NULL, NULL, NULL, NULL, NULL, NULL",
        "'task', NULL, NULL, '0193f000-0000-7000-8000-000000000010', \
         '0193f000-0000-7000-8000-000000000035', NULL, NULL, NULL",
    ] {
        assert!(
            connection
                .execute(
                    &format!(
                        "INSERT INTO command_targets
                             (project_id, receipt_id, target_kind, target_project_id,
                              target_mini_project_id, target_task_id, target_team_run_id,
                              target_agent_run_id, target_ticket_link_id, target_work_calendar_id)
                         VALUES ('0193f000-0000-7000-8000-000000000001',
                                 '0193f000-0000-7000-8000-000000000070', {tail})"
                    ),
                    [],
                )
                .is_err(),
            "a command target must name exactly one typed id"
        );
    }
}

#[test]
fn a_concurrent_first_open_initializes_exactly_one_realm() {
    // Two processes opening the same brand-new file at once. Only one may run
    // `0001`; the other must wait, notice that the schema now exists, and adopt
    // the Realm the winner created.
    //
    // The mutant this kills is reading `user_version` *before* taking the
    // IMMEDIATE lock and not re-reading after the wait: the loser then replays
    // `0001` against an already-created schema and fails on the first duplicate
    // object, turning a concurrent open into a hard error.
    let directory = temp();
    let path = directory.path().join("kontor.db");

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let openers: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                SqliteStore::open(&path).map(|store| store.realm_id())
            })
        })
        .collect();

    let realms: Vec<_> = openers
        .into_iter()
        .map(|opener| {
            opener
                .join()
                .expect("the opener thread does not panic")
                .expect("every concurrent first open succeeds")
        })
        .collect();

    // All four agree, and the identity is a real one rather than four races
    // each inventing their own.
    let distinct: BTreeSet<_> = realms.iter().collect();
    assert_eq!(
        distinct.len(),
        1,
        "a concurrent first open must not create more than one realm"
    );

    // Exactly one row exists on disk, and reopening still reports the same id.
    let connection = Connection::open(&path).expect("a raw connection opens");
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM realm_metadata", [], |row| row.get(0))
        .expect("the realm table is readable");
    assert_eq!(rows, 1);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version is readable");
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(
        SqliteStore::open(&path)
            .expect("reopening succeeds")
            .realm_id(),
        realms[0],
        "the realm survives the race and the reopen"
    );
}

/// Migration 0010 rebuilds `command_receipts` to widen its closed `kind` list,
/// and a rebuild is the one migration shape that can silently lose rows or
/// strand the six tables that reference it.
///
/// So this builds a genuine v9 file holding a receipt *and* a child row in every
/// referencing table, upgrades it, and proves all of them are still there and
/// still joined. The mutants it kills: a rebuild that copies no rows, one that
/// copies them but drops the referencing tables' rows with the old table, and
/// one that leaves the new `kind` values unaccepted.
#[test]
fn migration_0010_rebuilds_command_receipts_without_losing_a_row_or_a_reference() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    const REALM: &str = "0193f000-0000-7000-8000-0000000000c6";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000d1";
    const RECEIPT: &str = "0193f000-0000-7000-8000-0000000000d2";
    let digest = "a".repeat(64);

    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        for migration in MIGRATIONS_THROUGH_V9 {
            connection
                .execute_batch(migration)
                .expect("every pre-v10 migration runs");
        }
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-09T10:00:00Z', NULL)",
                [REALM],
            )
            .expect("the realm row is written");
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, 'P', '/tmp/kontor-v6', 1, '2026-08-09T10:00:00Z')",
                [PROJECT],
            )
            .expect("a project is written");
        connection
            .execute(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision,
                      intent, intent_hash, state, attempts, created_at, updated_at)
                 VALUES (?1, ?2, 'v9-key', 'authorize_execution',
                         json_object('kind', 'project', 'project_id', ?2), 1,
                         '{}', ?3, 'intent_persisted', 0,
                         '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z')",
                rusqlite::params![RECEIPT, PROJECT, digest],
            )
            .expect("a v9 receipt is written");
        connection
            .execute(
                "INSERT INTO command_receipt_transitions
                     (project_id, receipt_id, sequence, state, recorded_at)
                 VALUES (?1, ?2, 1, 'intent_persisted', '2026-08-09T10:00:00Z')",
                rusqlite::params![PROJECT, RECEIPT],
            )
            .expect("its transition is written");
        connection
            .execute(
                "INSERT INTO command_targets
                     (project_id, receipt_id, target_kind, target_project_id)
                 VALUES (?1, ?2, 'project', ?1)",
                rusqlite::params![PROJECT, RECEIPT],
            )
            .expect("its target is written");
        connection
            .execute(
                "INSERT INTO command_outbox
                     (receipt_id, project_id, payload, payload_hash, not_before, attempts)
                 VALUES (?1, ?2, '{}', ?3, '2026-08-09T10:00:00Z', 0)",
                rusqlite::params![RECEIPT, PROJECT, digest],
            )
            .expect("its outbox entry is written");
    }

    let store = SqliteStore::open(&path).expect("a v9 database is upgraded, not refused");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    store
        .foreign_key_check()
        .expect("the rebuild must not strand a single reference");
    store.integrity_check().expect("the file is still sound");

    let connection = raw(&directory);
    let kept: String = connection
        .query_row(
            "SELECT kind FROM command_receipts WHERE id = ?1",
            [RECEIPT],
            |row| row.get(0),
        )
        .expect("the v5 receipt survived the rebuild");
    assert_eq!(kept, "authorize_execution");
    for (table, column) in [
        ("command_receipt_transitions", "receipt_id"),
        ("command_targets", "receipt_id"),
        ("command_outbox", "receipt_id"),
    ] {
        let rows: i64 = connection
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column} = ?1"),
                [RECEIPT],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(rows, 1, "{table} lost its row to the rebuild");
    }
    let queued: i64 = connection
        .query_row(
            "SELECT count(*) FROM command_outbox WHERE receipt_id = ?1",
            [RECEIPT],
            |row| row.get(0),
        )
        .expect("readable");
    assert_eq!(queued, 1, "the outbox lost the entry the receipt owns");

    // And the three kinds the rebuild exists for are now storable.
    for kind in [
        "revoke_execution_authorization",
        "ensure_project",
        "ensure_account_profile",
    ] {
        connection
            .execute(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision,
                      intent, intent_hash, state, attempts, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4,
                         json_object('kind', 'project', 'project_id', ?2), 1,
                         '{}', ?5, 'intent_persisted', 0,
                         '2026-08-09T10:00:00Z', '2026-08-09T10:00:00Z')",
                rusqlite::params![uuid_like(kind), PROJECT, format!("v6-{kind}"), kind, digest],
            )
            .unwrap_or_else(|error| panic!("`{kind}` must be storable after v6: {error}"));
    }
}

/// A stable, canonical-looking id derived from a short label.
fn uuid_like(label: &str) -> String {
    let mut digest = 0u32;
    for byte in label.bytes() {
        digest = digest.wrapping_mul(31).wrapping_add(u32::from(byte));
    }
    format!("0193f000-0000-7000-8000-{digest:012x}")
}

/// Migration 0025 stamps a classification onto documents published before the
/// classification existed.
///
/// A realm that predates OP-REQ-037 has topology specifications and role
/// catalogs already in it. Opening that file must not fail, must not ask a
/// human anything, and must not invent an override: every pre-existing tier-B
/// document reads back as `project_shared` by the type-default rule. The
/// mutants this kills are dropping the column defaults — which would refuse to
/// open an existing realm — and backfilling `human_override`, which would
/// attribute a decision to a human who never made one.
#[test]
fn documents_published_before_the_classification_existed_adopt_the_tier_default() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    const REALM: &str = "0193f000-0000-7000-8000-0000000000c2";
    const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        for migration in MIGRATIONS_THROUGH_V24 {
            connection
                .execute_batch(migration)
                .expect("a frozen migration runs");
        }
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-16T01:00:00Z', NULL)",
                [REALM],
            )
            .expect("the v24 realm row is written");
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, 'P', '/tmp/p', 1, '2026-08-16T01:00:00Z')",
                [PROJECT],
            )
            .expect("the project is written");
        connection
            .execute(
                "INSERT INTO topology_specs
                     (project_id, spec_id, version, name, root_kind, definition,
                      definition_hash, published_at)
                 VALUES (?1, 'spec', 1, 'Legacy topology', 'PSW', '{}', ?2,
                         '2026-08-16T01:00:00Z')",
                [PROJECT, HASH],
            )
            .expect("a pre-classification topology specification is written");
        connection
            .execute(
                "INSERT INTO role_catalog_revisions
                     (catalog_id, version, name, definition, definition_hash, published_at)
                 VALUES ('catalog', 1, 'Legacy catalog', '{}', ?1, '2026-08-16T01:00:00Z')",
                [HASH],
            )
            .expect("a pre-classification role catalog is written");
    }

    let store = SqliteStore::open(&path).expect("a v24 database opens rather than being refused");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(
        store.realm_id().to_string(),
        REALM,
        "an upgrade must not mint a second Realm identity"
    );

    let connection = raw(&directory);
    for table in ["topology_specs", "role_catalog_revisions"] {
        let (class, classifier, provenance): (String, Option<String>, String) = connection
            .query_row(
                &format!(
                    "SELECT shareability_class, shareability_classifier,
                            shareability_provenance
                     FROM {table}"
                ),
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the backfilled stamp is readable");
        assert_eq!(class, "project_shared", "{table} defaults to shareable");
        assert_eq!(
            provenance, "type_default",
            "{table} was not decided by anyone"
        );
        assert_eq!(classifier, None, "{table} names no human");
    }
}

/// The tier-A tables added by 0023 are never given somewhere to put a
/// classification.
///
/// Refusing to classify operational state means the column does not exist, not
/// that it exists and holds a null. The mutant this kills is a later migration
/// "harmonizing" these tables with the classified ones.
#[test]
fn tier_a_operational_tables_have_nowhere_to_store_a_classification() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    for table in [
        "topology_nodes",
        "seat_bindings",
        "adaptive_admission_state",
        "topology_node_containers",
    ] {
        let columns: i64 = connection
            .query_row(
                &format!(
                    "SELECT count(*) FROM pragma_table_info('{table}')
                     WHERE name LIKE 'shareability%'"
                ),
                [],
                |row| row.get(0),
            )
            .expect("readable");
        assert_eq!(columns, 0, "{table} is tier A and refuses classification");
    }
}
