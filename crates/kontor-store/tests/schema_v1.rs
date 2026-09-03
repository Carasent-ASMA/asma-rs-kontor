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

use kontor_core::authority::{AuthoritySubject, SubjectOrigin};
use kontor_core::id::{AccountProfileId, CanonicalDocument, ProjectId};
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
    "canonical_jira_task_links",
    "capacity_configuration",
    "capacity_observations",
    "child_calendar_windows",
    "command_outbox",
    "command_receipt_transitions",
    "command_receipts",
    "command_targets",
    "compaction_receipts",
    "consultation_profile_revisions",
    // Schema v36 (KON-OP-05): frozen consultation execution, exact native
    // seat bindings, and immutable Committee findings.
    "consultation_runs",
    "consultation_seats",
    "consultation_seat_recoveries",
    "consultation_seat_recovery_attempts",
    "consultation_seat_materialization_reroutes",
    "consultation_topic_migration_provenance",
    // Schema v75 (ASMA-8050): immutable, exact-seat Committee permission
    // responses with durable dispatch and confirmation state.
    "consultation_permission_responses",
    "committee_findings",
    "committee_remediations",
    "committee_re_review_claims",
    "advisor_advice_artifacts",
    "context_packs",
    "core_team_revisions",
    // Schema v32 (KON-OP-06): published Completion Profile revisions, one durable
    // completion run per epic, and the TPM wake outbox.
    "completion_profile_revisions",
    "epic_completion",
    "epic_completion_remediation_command_claims",
    "epic_completion_remediation_proposals",
    "epic_completion_wakes",
    "epic_completion_wake_deliveries",
    "epic_backlog_codes",
    "epic_native_name_tokens",
    "epic_execution_scopes",
    "epic_jira_transition_intents",
    "epic_rosters",
    "epic_status_conflicts",
    "execution_authorization_revocations",
    "execution_authorization_tasks",
    "execution_authorizations",
    "external_comments",
    "external_ticket_observations",
    "external_workflow_specs",
    "gate_waivers",
    "guardrail_evaluations",
    "handoffs",
    // Schema v44 (KON-OP-17): exact native identities for persistent Core Team
    // topology seats.
    "hosted_topology_seats",
    "hosted_topology_seat_history",
    // Schema v7 (KON-MVP-21): which importer produced a holiday source revision,
    // what the request asked for, and the chain that makes one import current.
    "holiday_import_batches",
    "holiday_sources",
    // Schema v5 (KON-MVP-19): the destination half of a redacted import.
    "import_receipts",
    "imported_profile_selection_outcomes",
    "imported_records",
    // Schema v6 (KON-MVP-22): the terminal half of intake and its work lineage.
    "intake_created_work",
    "intake_decisions",
    "intake_receipts",
    "asma_epic_activations",
    "jira_epic_bindings",
    "jira_links",
    "jira_materialization_batches",
    "jira_materialization_items",
    // Schema v74 (ASMA-8050): exact immutable recovery receipts for an
    // interrupted Jira materialization batch.
    "jira_materialization_recoveries",
    "jira_task_binding_confirmations",
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
    "mini_project_team_definition_snapshots",
    "mini_project_topology_snapshots",
    "open_questions",
    "open_question_dispositions",
    "open_question_rounds",
    "open_question_trigger_firings",
    "persona_scenarios",
    "policy_evaluations",
    "projects",
    "project_subject_authority",
    "project_team_definition_defaults",
    "project_topology_defaults",
    "provider_quota_states",
    "provider_quota_windows",
    "provider_usage_observations",
    "profile_selection_outcomes",
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
    "subject_authority_receipts",
    "subject_import_manifests",
    "seat_bindings",
    "task_account_selections",
    "task_ai_short_names",
    "task_dependencies",
    "task_gate_evaluations",
    "task_modules",
    "task_persona_snapshots",
    "task_workflows",
    "task_short_codes",
    "task_worktrees",
    "tasks",
    "team_command_replays",
    "team_definitions",
    "team_definition_migration_command_intents",
    "team_definition_migration_intents",
    "team_definition_migration_receipts",
    "team_definition_migration_targets",
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
    "topology_spec_canonicalization_receipts",
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
        .pragma_update(None, "foreign_keys", false)
        .expect("foreign keys can be disabled");
    let fk: i64 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("readable");
    eprintln!("FOREIGN_KEYS = {fk}");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys can be enabled");
    connection
}

fn migrate_through_v33(connection: &Connection) {
    for migration in MIGRATIONS_THROUGH_V24
        .iter()
        .chain(OP03_MIGRATIONS_V25_THROUGH_V31)
        .chain(&[
            include_str!("../migrations/0032_core_team_revisions.sql"),
            include_str!("../migrations/0033_quick_sessions_and_promotion.sql"),
        ])
    {
        connection
            .execute_batch(migration)
            .expect("the canonical migrations through v33 run");
    }
}

fn migrate_through_v46(connection: &Connection) {
    migrate_through_v33(connection);
    for migration in [
        include_str!("../migrations/0034_consultation_profiles.sql"),
        include_str!("../migrations/0035_epic_completion.sql"),
        include_str!("../migrations/0036_consultation_runs.sql"),
        include_str!("../migrations/0037_escalation_brief.sql"),
        include_str!("../migrations/0038_publish_trigger_command.sql"),
        include_str!("../migrations/0039_committee_remediation.sql"),
        include_str!("../migrations/0040_advisor_advice.sql"),
        include_str!("../migrations/0041_open_questions.sql"),
        include_str!("../migrations/0042_imported_task_lifecycle.sql"),
        include_str!("../migrations/0043_epic_execution_scopes.sql"),
        include_str!("../migrations/0044_hosted_topology_seats.sql"),
        include_str!("../migrations/0045_admin_workflow_install_and_withdrawal.sql"),
        include_str!("../migrations/0046_task_short_codes_and_hosted_route_history.sql"),
    ] {
        connection
            .execute_batch(migration)
            .expect("the canonical migrations through v46 run");
    }
}

fn legacy_operational_topology() -> CanonicalDocument {
    let domain: serde_json::Value = serde_json::from_str(include_str!(
        "../../kontor-profiles/fixtures/operational-domain.json"
    ))
    .expect("the bundled domain JSON parses");
    let mut spec = domain["topology_specs"][0].clone();
    let object = spec.as_object_mut().expect("the topology is an object");
    object.remove("name_separator");
    for node in object["node_kinds"]
        .as_array_mut()
        .expect("the node kinds are an array")
    {
        let kind = node["kind"].as_str().expect("a kind");
        let legacy = match kind {
            "PSW" => "Project Session Workspace",
            "QSW" => "Quick Session Workspace",
            "ESW" => "Epic Session Workspace",
            "ECP" => "ECP · <Jira epic> · <short title>",
            "TSW" => "Ticket Session Workspace",
            "ASW" => "Advisor Session Workspace",
            "CSW" => "Committee Session Workspace",
            other => panic!("unexpected bundled kind {other}"),
        };
        let node = node.as_object_mut().expect("a node is an object");
        node.insert("name_template".to_owned(), serde_json::json!(legacy));
        node.remove("seat_name_template");
    }
    CanonicalDocument::from_value(&spec).expect("the known legacy spec canonicalizes")
}

fn seed_v46_operational_topology(connection: &Connection, definition: &CanonicalDocument) {
    const REALM: &str = "0193f000-0000-7000-8000-0000000000a1";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000a2";
    const EPIC: &str = "0193f000-0000-7000-8000-0000000000a3";
    const NODE: &str = "0193f000-0000-7000-8000-0000000000a4";
    const SPEC: &str = "01936f5a-1000-7000-8000-000000000001";
    connection
        .execute(
            "INSERT INTO realm_metadata
                 (singleton, realm_id, schema_version, created_at, display_label)
             VALUES (1, ?1, 1, '2026-08-20T18:00:00Z', NULL)",
            [REALM],
        )
        .expect("the v46 Realm is seeded");
    connection
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, 'P', '/tmp/v46-naming', 1, '2026-08-20T18:00:00Z')",
            [PROJECT],
        )
        .expect("the project is seeded");
    connection
        .execute(
            "INSERT INTO mini_projects (id, project_id, name, revision, created_at)
             VALUES (?1, ?2, 'ASMA-7675 · QNR v2 Nonprod Delivery', 1,
                     '2026-08-20T18:00:00Z')",
            rusqlite::params![EPIC, PROJECT],
        )
        .expect("the epic is seeded");
    connection
        .execute(
            "INSERT INTO topology_specs
                 (project_id, spec_id, version, name, root_kind, definition,
                  definition_hash, published_at, shareability_class,
                  shareability_classifier, shareability_provenance)
             VALUES (?1, ?2, 1, 'Operational project session topology', 'PSW',
                     ?3, ?4, '2026-08-20T18:00:00Z', 'project_shared', NULL, 'type_default')",
            rusqlite::params![PROJECT, SPEC, definition.json(), definition.hash().as_str()],
        )
        .expect("the old topology is seeded");
    connection
        .execute(
            "INSERT INTO project_topology_defaults
                 (project_id, spec_id, version, canonical_hash, selected_at)
             VALUES (?1, ?2, 1, ?3, '2026-08-20T18:00:00Z')",
            rusqlite::params![PROJECT, SPEC, definition.hash().as_str()],
        )
        .expect("the project pin is seeded");
    connection
        .execute(
            "INSERT INTO mini_project_topology_snapshots
                 (mini_project_id, project_id, spec_id, version, canonical_hash, pinned_at)
             VALUES (?1, ?2, ?3, 1, ?4, '2026-08-20T18:00:00Z')",
            rusqlite::params![EPIC, PROJECT, SPEC, definition.hash().as_str()],
        )
        .expect("the epic pin is seeded");
    connection
        .execute(
            "INSERT INTO topology_nodes
                 (id, project_id, mini_project_id, spec_id, spec_version, spec_hash,
                  kind, parent_id, lifecycle, placement, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, 'ESW', NULL, 'active', 'bound', 1,
                     '2026-08-20T18:00:00Z', '2026-08-20T18:00:00Z')",
            rusqlite::params![NODE, PROJECT, EPIC, SPEC, definition.hash().as_str()],
        )
        .expect("the pinned node is seeded");
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
    // Pinned deliberately: appending a migration must be a decision, not a
    // side effect. v68 adds immutable native-less materialization reroute;
    // v69 widens global Committee recovery rounds; v70 reconciles intermediate
    // v69 projections; and v71 adds exact-occupancy durable Completion-wake
    // delivery evidence; v72 adds durable project-scoped epic namespaces; and
    // v73 permits safe link recovery after an unconfirmed create attempt;
    // v74 adds the exact Jira materialization recovery ledger; v75 adds
    // durable, exact-seat Committee permission responses; v76 permits the
    // exact mixed link/create batch interrupted by a Jira connector outage;
    // v77 adds the immutable Team Definition that owns native hierarchy and
    // naming, its project selection and epic pin, and the durable resumable
    // intent an identity-preserving retitle applies under; and v78 keys Advisor
    // advice by the seat that gave it, so one ASW can hold several
    // independently reporting advisor seats.
    // v79 records the command receipt a confirmed migration was commanded
    // under, closing the crash window between the pin commit and the receipt.
    // v80 records the canonical command intent a migration was issued under,
    // so crash-window recovery can prove the retry is the same command. v81
    // adds the canonical Jira task-link and unique-open-conflict ledgers. v82
    // adds first-class epic Jira conflict and transition-intent ledgers. v83
    // attributes remediation evidence and replay claims to a completion era.
    assert_eq!(SCHEMA_VERSION, 83);
}

#[test]
fn v80_backfills_only_provable_v79_command_intents_and_fences_the_rest() {
    let connection = Connection::open_in_memory().expect("the v79 fixture opens");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys enable");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE team_definition_migration_intents (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 fingerprint TEXT NOT NULL,
                 state TEXT NOT NULL,
                 recorded_at TEXT NOT NULL
             ) STRICT;
             CREATE TABLE command_receipts (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 kind TEXT NOT NULL,
                 intent_hash TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 UNIQUE (project_id, id)
             ) STRICT;
             CREATE TABLE team_definition_migration_receipts (
                 intent_id TEXT PRIMARY KEY
                     REFERENCES team_definition_migration_intents(id) ON DELETE RESTRICT,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 receipt_id TEXT NOT NULL,
                 bound_at TEXT NOT NULL,
                 FOREIGN KEY (project_id, receipt_id)
                     REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT
             ) STRICT;
             INSERT INTO projects (id) VALUES ('project');",
        )
        .expect("the v79 parent schema installs");
    let fingerprint = "f".repeat(64);
    for (id, state) in [
        ("recorded", "recorded"),
        ("applying", "applying"),
        ("confirmed-unreceipted", "confirmed"),
        ("confirmed-receipted", "confirmed"),
    ] {
        connection
            .execute(
                "INSERT INTO team_definition_migration_intents
                     (id, project_id, fingerprint, state, recorded_at)
                 VALUES (?1, 'project', ?2, ?3, '2026-09-01T12:00:00Z')",
                rusqlite::params![id, fingerprint, state],
            )
            .expect("the v79 migration intent is seeded");
    }
    let exact_intent_hash = "a".repeat(64);
    connection
        .execute(
            "INSERT INTO command_receipts
                 (id, project_id, kind, intent_hash, created_at)
             VALUES ('receipt', 'project', 'upgrade_team_definition', ?1,
                     '2026-09-01T12:01:00Z')",
            rusqlite::params![exact_intent_hash],
        )
        .expect("the exact command receipt is seeded");
    connection
        .execute(
            "INSERT INTO team_definition_migration_receipts
                 (intent_id, project_id, receipt_id, bound_at)
             VALUES ('confirmed-receipted', 'project', 'receipt',
                     '2026-09-01T12:02:00Z')",
            [],
        )
        .expect("the confirmed migration is bound to its exact receipt");

    connection
        .execute_batch(include_str!(
            "../migrations/0080_team_definition_migration_command_intents.sql"
        ))
        .expect("v80 migrates data-bearing v79 state");

    let migrated = |id: &str| {
        connection
            .query_row(
                "SELECT intent_hash, source
                 FROM team_definition_migration_command_intents
                 WHERE intent_id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("every v79 migration is explicitly classified")
    };
    for id in ["recorded", "applying", "confirmed-unreceipted"] {
        assert_eq!(
            migrated(id),
            (None, "legacy_unrecoverable".to_owned()),
            "an unreceipted {id} migration is fenced rather than guessed"
        );
    }
    assert_eq!(
        migrated("confirmed-receipted"),
        (Some(exact_intent_hash), "legacy_receipt".to_owned()),
        "a receipt is the only v79 source that proves the original command intent"
    );
}

#[test]
fn v64_preserves_published_committee_remediation_and_its_immutability() {
    let connection = Connection::open_in_memory().expect("the migration fixture opens");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys enable");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE consultation_runs (
                 run_id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 UNIQUE (project_id, run_id)
             ) STRICT;
             INSERT INTO projects (id) VALUES
                 ('0193f000-0000-7000-8000-000000000001');
             INSERT INTO consultation_runs (run_id, project_id) VALUES
                 ('0193f000-0000-7000-8000-000000000002',
                  '0193f000-0000-7000-8000-000000000001'),
                 ('0193f000-0000-7000-8000-000000000003',
                  '0193f000-0000-7000-8000-000000000001');",
        )
        .expect("the parent identities seed");
    connection
        .execute_batch(include_str!("../migrations/0039_committee_remediation.sql"))
        .expect("the published v39 shape installs");
    connection
        .execute(
            "INSERT INTO committee_remediations
                 (committee_run_id, project_id, from_round, recommendation,
                  tried_path, document, document_hash, recorded_at)
             VALUES (?1, ?2, 1, 'preserve this recommendation',
                     'preserve this tried path', ?3, ?4, '2026-08-25T20:00:00Z')",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000002",
                "0193f000-0000-7000-8000-000000000001",
                r#"{"from_round":1,"immutable":true}"#,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ],
        )
        .expect("the historical immutable row publishes");

    connection
        .execute_batch(include_str!(
            "../migrations/0064_committee_remediation_rounds.sql"
        ))
        .expect("the supported v64 migration reconciles the schema");

    let preserved: (i64, String, String, String) = connection
        .query_row(
            "SELECT from_round, recommendation, document, document_hash
             FROM committee_remediations WHERE committee_run_id = ?1",
            ["0193f000-0000-7000-8000-000000000002"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the historical row reads after migration");
    assert_eq!(preserved.0, 1);
    assert_eq!(preserved.1, "preserve this recommendation");
    assert_eq!(preserved.2, r#"{"from_round":1,"immutable":true}"#);
    assert_eq!(
        preserved.3,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    connection
        .execute(
            "INSERT INTO committee_remediations
                 (committee_run_id, project_id, from_round, recommendation,
                  tried_path, document, document_hash, recorded_at)
             VALUES (?1, ?2, 2, 'round two', 'bounded path', '{}', ?3,
                     '2026-08-25T20:01:00Z')",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000003",
                "0193f000-0000-7000-8000-000000000001",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ],
        )
        .expect("the widened source-round constraint admits round two");
    assert!(
        connection
            .execute(
                "UPDATE committee_remediations SET recommendation = 'rewritten'
                 WHERE committee_run_id = ?1",
                ["0193f000-0000-7000-8000-000000000002"],
            )
            .is_err(),
        "migration must recreate the immutable-update trigger"
    );
    assert!(
        connection
            .execute(
                "DELETE FROM committee_remediations WHERE committee_run_id = ?1",
                ["0193f000-0000-7000-8000-000000000002"],
            )
            .is_err(),
        "migration must recreate the permanent-row trigger"
    );
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the migrated generation reads");
    assert_eq!(version, 64);
}

#[test]
fn v65_backfills_published_proposals_and_fences_new_generations() {
    let connection = Connection::open_in_memory().expect("the migration fixture opens");
    connection
        .execute_batch(
            "CREATE TABLE epic_completion_remediation_proposals (
                 project_id TEXT NOT NULL,
                 mini_project_id TEXT NOT NULL,
                 round INTEGER NOT NULL CHECK (round >= 1),
                 failed_round_evidence TEXT NOT NULL,
                 proposal TEXT NOT NULL,
                 lsa_seat_binding_id TEXT NOT NULL,
                 proposed_at TEXT NOT NULL,
                 PRIMARY KEY (project_id, mini_project_id, round)
             ) STRICT;
             INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, round, failed_round_evidence,
                  proposal, lsa_seat_binding_id, proposed_at)
             VALUES ('project', 'epic', 1, 'evidence', 'proposal', 'lsa',
                     '2026-08-25T20:00:00Z');",
        )
        .expect("the published proposal shape seeds");

    connection
        .execute_batch(include_str!(
            "../migrations/0065_remediation_proposal_seat_generation.sql"
        ))
        .expect("the supported v65 migration installs");

    let generation: i64 = connection
        .query_row(
            "SELECT lsa_occupancy_generation
             FROM epic_completion_remediation_proposals
             WHERE project_id = 'project' AND mini_project_id = 'epic' AND round = 1",
            [],
            |row| row.get(0),
        )
        .expect("the published proposal survives");
    assert_eq!(generation, 1, "legacy proposals belong to generation one");
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, round, failed_round_evidence,
                  proposal, lsa_seat_binding_id, proposed_at,
                  lsa_occupancy_generation)
             VALUES ('project', 'epic', 2, 'evidence-2', 'proposal-2', 'lsa',
                     '2026-08-25T20:01:00Z', 2)",
            [],
        )
        .expect("a successor occupancy can author a proposal");
    let invalid = connection
        .execute(
            "INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, round, failed_round_evidence,
                  proposal, lsa_seat_binding_id, proposed_at,
                  lsa_occupancy_generation)
             VALUES ('project', 'epic', 3, 'evidence-3', 'proposal-3', 'lsa',
                     '2026-08-25T20:02:00Z', 0)",
            [],
        )
        .expect_err("generation zero is not a seat occupancy");
    assert!(
        invalid
            .to_string()
            .contains("lsa_occupancy_generation >= 1"),
        "the generation constraint refused for the wrong reason: {invalid}"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("the schema version reads"),
        65
    );
}

#[test]
fn v83_preserves_era_one_remediation_and_separates_reopened_rounds() {
    let connection = Connection::open_in_memory().expect("fixture opens");
    let hash_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let hash_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let project = "0193f000-0000-7000-8000-000000000001";
    let epic = "0193f000-0000-7000-8000-000000000002";
    let seat = "0193f000-0000-7000-8000-000000000003";
    connection
        .execute_batch(&format!(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE mini_projects (id TEXT NOT NULL, project_id TEXT NOT NULL,
                 PRIMARY KEY (project_id, id)) STRICT;
             CREATE TABLE epic_completion_remediation_proposals (
                 project_id TEXT NOT NULL, mini_project_id TEXT NOT NULL,
                 round INTEGER NOT NULL, failed_round_evidence TEXT NOT NULL,
                 proposal TEXT NOT NULL, lsa_seat_binding_id TEXT NOT NULL,
                 proposed_at TEXT NOT NULL, lsa_occupancy_generation INTEGER NOT NULL,
                 PRIMARY KEY (project_id, mini_project_id, round)) STRICT;
             CREATE TABLE epic_completion_remediation_command_claims (
                 project_id TEXT NOT NULL, mini_project_id TEXT NOT NULL,
                 round INTEGER NOT NULL, action TEXT NOT NULL,
                 idempotency_key TEXT NOT NULL UNIQUE, intent_hash TEXT NOT NULL,
                 effect_revision INTEGER NULL, claimed_at TEXT NOT NULL,
                 PRIMARY KEY (project_id, mini_project_id, round, action)) STRICT;
             CREATE TRIGGER epic_completion_remediation_command_claims_are_immutable
             BEFORE UPDATE ON epic_completion_remediation_command_claims
             BEGIN SELECT RAISE(ABORT, 'old immutable claim'); END;
             CREATE TRIGGER epic_completion_remediation_command_claims_are_permanent
             BEFORE DELETE ON epic_completion_remediation_command_claims
             BEGIN SELECT RAISE(ABORT, 'old permanent claim'); END;
             INSERT INTO projects VALUES ('{project}');
             INSERT INTO mini_projects VALUES ('{epic}', '{project}');
             INSERT INTO epic_completion_remediation_proposals VALUES
                 ('{project}', '{epic}', 1, '{hash_a}', '{hash_b}', '{seat}',
                  '2026-09-03T09:00:00Z', 1);
             INSERT INTO epic_completion_remediation_command_claims VALUES
                 ('{project}', '{epic}', 1, 'lsa_proposal', 'era-one', '{hash_a}',
                  2, '2026-09-03T09:00:00Z');"
        ))
        .expect("era-one rows seed");
    connection
        .execute_batch(include_str!(
            "../migrations/0083_completion_remediation_generations.sql"
        ))
        .expect("v83 applies");
    let preserved: (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT completion_generation FROM epic_completion_remediation_proposals),
                (SELECT completion_generation FROM epic_completion_remediation_command_claims)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("backfill reads");
    assert_eq!(preserved, (1, 1));
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_proposals
                 (project_id, mini_project_id, completion_generation, round,
                  failed_round_evidence, proposal, lsa_seat_binding_id, proposed_at,
                  lsa_occupancy_generation)
             VALUES (?1, ?2, 2, 1, ?3, ?4, ?5, ?6, 1)",
            rusqlite::params![project, epic, hash_b, hash_a, seat, "2026-09-03T10:00:00Z"],
        )
        .expect("era two may own round one independently");
    connection
        .execute(
            "INSERT INTO epic_completion_remediation_command_claims
                 (project_id, mini_project_id, completion_generation, round, action,
                  idempotency_key, intent_hash, effect_revision, claimed_at)
             VALUES (?1, ?2, 2, 1, 'lsa_proposal', 'era-two', ?3, 3, ?4)",
            rusqlite::params![project, epic, hash_b, "2026-09-03T10:00:00Z"],
        )
        .expect("era two may own its replay claim independently");
    assert!(
        connection
            .execute(
                "UPDATE epic_completion_remediation_command_claims SET intent_hash = ?1
                 WHERE completion_generation = 2",
                [hash_a],
            )
            .is_err(),
        "the rebuilt claim remains immutable"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version reads"),
        83
    );
}

#[test]
fn v66_claims_remediation_commands_and_re_review_provenance_immutably() {
    let connection = Connection::open_in_memory().expect("the migration fixture opens");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys enable");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE mini_projects (
                 id TEXT NOT NULL,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 PRIMARY KEY (project_id, id)
             ) STRICT;
             CREATE TABLE consultation_runs (
                 run_id TEXT NOT NULL PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 UNIQUE (project_id, run_id)
             ) STRICT;
             INSERT INTO projects VALUES ('0193f000-0000-7000-8000-000000000001');
             INSERT INTO mini_projects VALUES (
                 '0193f000-0000-7000-8000-000000000002',
                 '0193f000-0000-7000-8000-000000000001'
             );
             INSERT INTO consultation_runs VALUES
                 ('0193f000-0000-7000-8000-000000000003',
                  '0193f000-0000-7000-8000-000000000001'),
                 ('0193f000-0000-7000-8000-000000000004',
                  '0193f000-0000-7000-8000-000000000001');",
        )
        .expect("the parent identities seed");
    connection
        .execute_batch(include_str!(
            "../migrations/0066_remediation_command_claims.sql"
        ))
        .expect("the supported v66 migration installs");

    connection
        .execute(
            "INSERT INTO epic_completion_remediation_command_claims
                 (project_id, mini_project_id, round, action, idempotency_key,
                  intent_hash, effect_revision, claimed_at)
             VALUES (?1, ?2, 1, 'lsa_proposal', 'proposal-key', ?3, 3, ?4)",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000001",
                "0193f000-0000-7000-8000-000000000002",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "2026-08-26T10:00:00Z",
            ],
        )
        .expect("the remediation command claim records");
    assert!(
        connection
            .execute(
                "INSERT INTO epic_completion_remediation_command_claims
                     (project_id, mini_project_id, round, action, idempotency_key,
                      intent_hash, effect_revision, claimed_at)
                 VALUES (?1, ?2, 1, 'lsa_proposal', 'different-key', ?3, 3, ?4)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "0193f000-0000-7000-8000-000000000002",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "2026-08-26T10:00:01Z",
                ],
            )
            .is_err(),
        "one remediation action cannot acquire a second replay authority"
    );
    assert!(
        connection
            .execute(
                "UPDATE epic_completion_remediation_command_claims
                    SET intent_hash = ?1",
                ["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
            )
            .is_err(),
        "the command claim is immutable"
    );

    let provenance = r#"{"completion_revision":7,"completion_round":1}"#;
    let provenance_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    connection
        .execute(
            "INSERT INTO committee_re_review_claims
                 (project_id, mini_project_id, provenance, provenance_hash,
                  committee_run_id, claimed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000001",
                "0193f000-0000-7000-8000-000000000002",
                provenance,
                provenance_hash,
                "0193f000-0000-7000-8000-000000000003",
                "2026-08-26T10:01:00Z",
            ],
        )
        .expect("the first re-review claims its provenance");
    assert!(
        connection
            .execute(
                "INSERT INTO committee_re_review_claims
                     (project_id, mini_project_id, provenance, provenance_hash,
                      committee_run_id, claimed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "0193f000-0000-7000-8000-000000000001",
                    "0193f000-0000-7000-8000-000000000002",
                    provenance,
                    provenance_hash,
                    "0193f000-0000-7000-8000-000000000004",
                    "2026-08-26T10:01:01Z",
                ],
            )
            .is_err(),
        "distinct invoke keys cannot freeze the same normalized provenance twice"
    );
    assert!(
        connection
            .execute("DELETE FROM committee_re_review_claims", [])
            .is_err(),
        "a provenance claim cannot be withdrawn"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("the schema version reads"),
        66
    );
}

#[test]
fn v67_keeps_every_provider_usage_heartbeat_immutable_and_permanent() {
    let connection = Connection::open_in_memory().expect("the migration fixture opens");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys enable");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE account_profiles (
                 id TEXT NOT NULL,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
                 PRIMARY KEY (project_id, id)
             ) STRICT;
             INSERT INTO projects VALUES ('0193f000-0000-7000-8000-000000000001');
             INSERT INTO account_profiles VALUES (
                 '0193f000-0000-7000-8000-000000000002',
                 '0193f000-0000-7000-8000-000000000001'
             );",
        )
        .expect("the parent identities seed");
    connection
        .execute_batch(include_str!(
            "../migrations/0067_provider_usage_observations.sql"
        ))
        .expect("the supported v67 migration installs");
    connection
        .execute(
            "INSERT INTO provider_usage_observations
                 (id, project_id, account_profile_id, provider, evidence_hash, state,
                  resets_at, windows, observed_at, idempotency_key, intent_hash)
             VALUES (?1, ?2, ?3, 'claude-work', ?4, 'available', NULL, '[]', ?5,
                     'probe-key', ?6)",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000003",
                "0193f000-0000-7000-8000-000000000001",
                "0193f000-0000-7000-8000-000000000002",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "2026-08-27T17:01:00Z",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ],
        )
        .expect("the heartbeat records");
    assert!(
        connection
            .execute(
                "UPDATE provider_usage_observations SET observed_at = ?1",
                ["2026-08-27T17:02:00Z"],
            )
            .is_err(),
        "a success heartbeat is immutable"
    );
    assert!(
        connection
            .execute("DELETE FROM provider_usage_observations", [])
            .is_err(),
        "a success heartbeat cannot be withdrawn"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("the schema version reads"),
        67
    );
}

#[test]
fn v68_materialization_reroute_lineage_is_immutable_and_has_distinct_receipt_authority() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .pragma_update(None, "foreign_keys", false)
        .expect("the isolated lineage fixture disables parent checks");
    connection
        .execute(
            "INSERT INTO consultation_seat_materialization_reroutes
         (project_id,run_id,role_slot_id,seat_binding_id,
          predecessor_generation,successor_generation,
          predecessor_model_rung,successor_model_rung,reason,
          recovery_profile,recovery_profile_hash,request_intent_hash,
          idempotency_key,headroom_account_profile_id,headroom_observation_id,
          headroom_evidence_hash,predecessor_revision,successor_revision,rerouted_at)
         VALUES (?1,?2,'reviewer-a',?3,1,2,?4,?5,
                 'permission_mode_unsupported',?6,?7,?8,'reroute-v1',?9,?10,?11,1,2,?12)",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000001",
                "0193f000-0000-7000-8000-000000000002",
                "0193f000-0000-7000-8000-000000000003",
                r#"{"provider":"opencode","model":"deepseek/deepseek-v4-flash","effort":"max"}"#,
                r#"{"provider":"claude-work","model":"claude-opus-5","effort":"xhigh"}"#,
                r#"{"schema_version":1,"ordered_routes":[]}"#,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "0193f000-0000-7000-8000-000000000004",
                "0193f000-0000-7000-8000-000000000005",
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "2026-08-27T17:01:00Z",
            ],
        )
        .expect("one exact lineage row records");
    assert!(
        connection
            .execute(
                "UPDATE consultation_seat_materialization_reroutes SET reason = reason",
                [],
            )
            .is_err(),
        "lineage refuses even a no-op UPDATE"
    );
    assert!(
        connection
            .execute("DELETE FROM consultation_seat_materialization_reroutes", [],)
            .is_err(),
        "lineage cannot be withdrawn"
    );

    connection
        .execute(
            "INSERT INTO command_receipts
         (id,project_id,idempotency_key,kind,target,target_revision,intent,intent_hash,
          state,attempts,created_at,updated_at,execution_mode)
         VALUES (?1,?2,'reroute-v1','reroute_unmaterialized_consultation_seat',
                 '{}',2,'{}',?3,'confirmed',0,?4,?4,'local')",
            rusqlite::params![
                "0193f000-0000-7000-8000-000000000006",
                "0193f000-0000-7000-8000-000000000001",
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                "2026-08-27T17:01:01Z",
            ],
        )
        .expect("the distinct reroute command kind is accepted");
}

#[test]
fn v69_preserves_global_round_rows_and_opens_the_positive_u8_domain() {
    const PROJECT: &str = "0193f000-0000-7000-8000-000000000001";
    const EPIC: &str = "0193f000-0000-7000-8000-000000000002";
    const RUN_TWO: &str = "0193f000-0000-7000-8000-000000000003";
    const RUN_THREE: &str = "0193f000-0000-7000-8000-000000000004";
    const CALLER: &str = "0193f000-0000-7000-8000-000000000005";
    const NODE_TWO: &str = "0193f000-0000-7000-8000-000000000006";
    const NODE_THREE: &str = "0193f000-0000-7000-8000-000000000007";
    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let connection = Connection::open_in_memory().expect("the migration fixture opens");
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE mini_projects (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id),
                 UNIQUE (project_id, id)
             ) STRICT;
             CREATE TABLE consultation_profile_revisions (
                 project_id TEXT NOT NULL, family TEXT NOT NULL,
                 profile_id TEXT NOT NULL, version INTEGER NOT NULL,
                 PRIMARY KEY (project_id, family, profile_id, version)
             ) STRICT;
             CREATE TABLE topology_nodes (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE seat_bindings (id TEXT PRIMARY KEY) STRICT;
             CREATE TABLE consultation_runs (
                 run_id TEXT PRIMARY KEY, project_id TEXT NOT NULL,
                 mini_project_id TEXT NOT NULL, family TEXT NOT NULL,
                 profile_id TEXT NOT NULL, profile_version INTEGER NOT NULL,
                 definition_hash TEXT NOT NULL, question TEXT NOT NULL,
                 question_hash TEXT NOT NULL, context TEXT NOT NULL,
                 context_hash TEXT NOT NULL, caller_seat_binding_id TEXT NOT NULL,
                 topology_node_id TEXT NOT NULL UNIQUE,
                 invoke_key TEXT NOT NULL UNIQUE, invoke_intent_hash TEXT NOT NULL,
                 state TEXT NOT NULL, round INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
                 result TEXT NULL, result_hash TEXT NULL, revision INTEGER NOT NULL,
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL, settled_at TEXT NULL,
                 UNIQUE (project_id, run_id)
             ) STRICT;
             CREATE INDEX ix_consultation_runs_epic
                 ON consultation_runs(project_id, mini_project_id, family, created_at, run_id);
             CREATE TRIGGER consultation_run_inputs_are_frozen
             BEFORE UPDATE ON consultation_runs
             WHEN OLD.context <> NEW.context
             BEGIN SELECT RAISE(ABORT, 'old frozen run'); END;
             CREATE TABLE consultation_seats (
                 run_id TEXT NOT NULL, role_slot_id TEXT NOT NULL,
                 seat_binding_id TEXT NOT NULL, native_id TEXT NULL,
                 PRIMARY KEY (run_id, role_slot_id),
                 FOREIGN KEY (run_id) REFERENCES consultation_runs(run_id)
             ) STRICT;
             CREATE TABLE advisor_advice_artifacts (
                 advisor_run_id TEXT NOT NULL, project_id TEXT NOT NULL,
                 seat_binding_id TEXT NOT NULL
             ) STRICT;
             CREATE TRIGGER advisor_advice_belongs_to_its_attested_seat
             BEFORE INSERT ON advisor_advice_artifacts
             WHEN NOT EXISTS (
                 SELECT 1 FROM consultation_runs AS run
                 JOIN consultation_seats AS seat ON seat.run_id = run.run_id
                 WHERE run.project_id = NEW.project_id
                   AND run.run_id = NEW.advisor_run_id
                   AND run.family = 'advisor'
                   AND seat.seat_binding_id = NEW.seat_binding_id
                   AND seat.native_id IS NOT NULL
             )
             BEGIN SELECT RAISE(ABORT, 'old Advisor attestation guard'); END;
             CREATE TABLE committee_findings (
                 committee_run_id TEXT NOT NULL, project_id TEXT NOT NULL,
                 round INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
                 role_slot_id TEXT NOT NULL, role TEXT NOT NULL, verdict TEXT NOT NULL,
                 evidence_complete INTEGER NOT NULL, document TEXT NOT NULL,
                 document_hash TEXT NOT NULL, recorded_at TEXT NOT NULL,
                 PRIMARY KEY (committee_run_id, round, role_slot_id)
             ) STRICT;
             CREATE TRIGGER committee_findings_are_immutable
             BEFORE UPDATE ON committee_findings
             BEGIN SELECT RAISE(ABORT, 'old immutable finding'); END;
             CREATE TRIGGER committee_findings_are_permanent
             BEFORE DELETE ON committee_findings
             BEGIN SELECT RAISE(ABORT, 'old permanent finding'); END;
             CREATE TABLE epic_completion_remediation_command_claims (
                 project_id TEXT NOT NULL, mini_project_id TEXT NOT NULL,
                 round INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
                 action TEXT NOT NULL, idempotency_key TEXT NOT NULL UNIQUE,
                 intent_hash TEXT NOT NULL, effect_revision INTEGER NULL,
                 claimed_at TEXT NOT NULL,
                 PRIMARY KEY (project_id, mini_project_id, round, action)
             ) STRICT;
             CREATE TRIGGER epic_completion_remediation_command_claims_are_immutable
             BEFORE UPDATE ON epic_completion_remediation_command_claims
             BEGIN SELECT RAISE(ABORT, 'old immutable claim'); END;
             CREATE TRIGGER epic_completion_remediation_command_claims_are_permanent
             BEFORE DELETE ON epic_completion_remediation_command_claims
             BEGIN SELECT RAISE(ABORT, 'old permanent claim'); END;
             CREATE TABLE run_children (
                 project_id TEXT NOT NULL, run_id TEXT NOT NULL,
                 FOREIGN KEY (project_id, run_id)
                     REFERENCES consultation_runs(project_id, run_id)
             ) STRICT;",
        )
        .expect("the v68 round-bearing schema is reproduced");
    connection
        .execute_batch(&format!(
            "INSERT INTO projects VALUES ('{PROJECT}');
             INSERT INTO mini_projects VALUES ('{EPIC}', '{PROJECT}');
             INSERT INTO consultation_profile_revisions
                 VALUES ('{PROJECT}', 'committee', 'independent-review', 1);
             INSERT INTO topology_nodes VALUES ('{NODE_TWO}'), ('{NODE_THREE}');
             INSERT INTO seat_bindings VALUES ('{CALLER}');
             INSERT INTO consultation_runs
                 (run_id, project_id, mini_project_id, family, profile_id,
                  profile_version, definition_hash, question, question_hash,
                  context, context_hash, caller_seat_binding_id, topology_node_id,
                  invoke_key, invoke_intent_hash, state, round, result, result_hash,
                  revision, created_at, updated_at, settled_at)
             VALUES ('{RUN_TWO}', '{PROJECT}', '{EPIC}', 'committee',
                     'independent-review', 1, '{HASH_A}', 'round two', '{HASH_A}',
                     '{{}}', '{HASH_A}', '{CALLER}', '{NODE_TWO}', 'invoke-two',
                     '{HASH_A}', 'running', 2, NULL, NULL, 1,
                     '2026-08-29T20:00:00Z', '2026-08-29T20:00:00Z', NULL);
             INSERT INTO consultation_seats
                 (run_id, role_slot_id, seat_binding_id, native_id)
                 VALUES ('{RUN_TWO}', 'reviewer-a', '{CALLER}', 'native-two');
             INSERT INTO committee_findings
                 VALUES ('{RUN_TWO}', '{PROJECT}', 2, 'reviewer-a', 'reviewer',
                         'non_compliant', 1, '{{}}', '{HASH_A}', '2026-08-29T20:01:00Z');
             INSERT INTO epic_completion_remediation_command_claims
                 VALUES ('{PROJECT}', '{EPIC}', 2, 'tpm_route', 'route-two',
                         '{HASH_A}', 7, '2026-08-29T20:02:00Z');
             INSERT INTO run_children VALUES ('{PROJECT}', '{RUN_TWO}');"
        ))
        .expect("the published round-two rows are seeded");

    connection
        .pragma_update(None, "foreign_keys", false)
        .expect("foreign keys disable around the table rebuilds");
    connection
        .execute_batch(include_str!(
            "../migrations/0069_global_committee_recovery_rounds.sql"
        ))
        .expect("the v69 round-domain migration applies");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys re-enable after the isolated rebuild");

    let foreign_key_violations: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("the foreign-key graph is readable");
    assert_eq!(foreign_key_violations, 0);
    let retained: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM consultation_runs WHERE round = 2),
                 (SELECT count(*) FROM committee_findings WHERE round = 2),
                 (SELECT count(*) FROM epic_completion_remediation_command_claims WHERE round = 2),
                 (SELECT count(*) FROM run_children WHERE run_id = ?1)",
            [RUN_TWO],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the migrated rows are readable");
    assert_eq!(retained, (1, 1, 1, 1));

    for table in [
        "consultation_runs",
        "committee_findings",
        "epic_completion_remediation_command_claims",
    ] {
        let definition: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("the rebuilt definition reads");
        assert!(
            definition.contains("round BETWEEN 1 AND 255"),
            "{table} retained a narrower global round domain: {definition}"
        );
    }
    let run_child_parent: String = connection
        .query_row(
            "SELECT \"table\" FROM pragma_foreign_key_list('run_children') LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("the child foreign key reads");
    assert_eq!(run_child_parent, "consultation_runs");
    let run_index: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'ix_consultation_runs_epic'",
            [],
            |row| row.get(0),
        )
        .expect("the run index catalogue reads");
    assert_eq!(run_index, 1);

    connection
        .execute_batch(&format!(
            "INSERT INTO consultation_runs
                 (run_id, project_id, mini_project_id, family, profile_id,
                  profile_version, definition_hash, question, question_hash,
                  context, context_hash, caller_seat_binding_id, topology_node_id,
                  invoke_key, invoke_intent_hash, state, round, result, result_hash,
                  revision, created_at, updated_at, settled_at)
             VALUES ('{RUN_THREE}', '{PROJECT}', '{EPIC}', 'committee',
                     'independent-review', 1, '{HASH_B}', 'round three', '{HASH_B}',
                     '{{}}', '{HASH_B}', '{CALLER}', '{NODE_THREE}', 'invoke-three',
                     '{HASH_B}', 'running', 3, NULL, NULL, 1,
                     '2026-08-29T21:00:00Z', '2026-08-29T21:00:00Z', NULL);
             INSERT INTO consultation_seats
                 (run_id, role_slot_id, seat_binding_id, native_id)
                 VALUES ('{RUN_THREE}', 'reviewer-a', '{CALLER}', 'native-three');
             INSERT INTO committee_findings
                 VALUES ('{RUN_THREE}', '{PROJECT}', 3, 'reviewer-a', 'reviewer',
                         'compliant', 1, '{{}}', '{HASH_B}', '2026-08-29T21:01:00Z');
             INSERT INTO epic_completion_remediation_command_claims
                 VALUES ('{PROJECT}', '{EPIC}', 3, 'lsa_proposal', 'proposal-three',
                         '{HASH_B}', NULL, '2026-08-29T21:02:00Z');"
        ))
        .expect("a repeated needs-human recovery round persists globally");

    assert!(
        connection
            .execute(
                "UPDATE consultation_runs SET context = '{\"changed\":true}'",
                []
            )
            .is_err(),
        "the run input-freeze trigger must survive the rebuild"
    );
    assert!(
        connection
            .execute("DELETE FROM committee_findings WHERE round = 3", [])
            .is_err(),
        "the finding permanence trigger must survive the rebuild"
    );
    assert!(
        connection
            .execute(
                "UPDATE epic_completion_remediation_command_claims
                 SET intent_hash = intent_hash WHERE round = 3",
                [],
            )
            .is_err(),
        "the command-claim immutability trigger must survive the rebuild"
    );
    assert!(
        connection
            .execute(
                "INSERT INTO advisor_advice_artifacts VALUES ('missing', ?1, ?2)",
                [PROJECT, CALLER],
            )
            .is_err(),
        "the cross-table Advisor attestation trigger must survive the rebuild"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("the schema version reads"),
        69
    );

    connection
        .pragma_update(None, "foreign_keys", false)
        .expect("foreign keys disable around the reconciliation rebuilds");
    connection
        .execute_batch(include_str!(
            "../migrations/0070_reconcile_global_committee_rounds.sql"
        ))
        .expect("the v70 round-domain reconciliation applies over v69");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("foreign keys re-enable after the reconciliation rebuilds");

    let reconciled: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM consultation_runs WHERE round IN (2, 3)),
                 (SELECT count(*) FROM committee_findings WHERE round IN (2, 3)),
                 (SELECT count(*) FROM epic_completion_remediation_command_claims
                    WHERE round IN (2, 3)),
                 (SELECT count(*) FROM run_children WHERE run_id = ?1)",
            [RUN_TWO],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("all round-bearing rows survive the reconciliation");
    assert_eq!(reconciled, (2, 2, 2, 1));
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("the reconciled foreign-key graph is readable"),
        0
    );
    for table in [
        "consultation_runs",
        "committee_findings",
        "epic_completion_remediation_command_claims",
    ] {
        let definition: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("the reconciled table definition reads");
        assert!(
            definition.contains("round BETWEEN 1 AND 255"),
            "{table} retained a partial v69 round domain: {definition}"
        );
    }
    assert!(
        connection
            .execute(
                "UPDATE consultation_runs SET context = '{\"changed-again\":true}'",
                []
            )
            .is_err(),
        "the run input-freeze trigger must survive reconciliation"
    );
    assert!(
        connection
            .execute("DELETE FROM committee_findings WHERE round = 3", [])
            .is_err(),
        "the finding permanence trigger must survive reconciliation"
    );
    assert!(
        connection
            .execute(
                "UPDATE epic_completion_remediation_command_claims
                 SET intent_hash = intent_hash WHERE round = 3",
                [],
            )
            .is_err(),
        "the command-claim immutability trigger must survive reconciliation"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("the reconciled schema version reads"),
        70
    );

    connection
        .execute_batch(include_str!(
            "../migrations/0071_completion_wake_deliveries.sql"
        ))
        .expect("the v71 wake-delivery migration applies over deployed v70");
    let delivery_table: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'table' AND name = 'epic_completion_wake_deliveries'",
            [],
            |row| row.get(0),
        )
        .expect("the wake-delivery table is installed");
    assert!(delivery_table.contains("occupancy_generation"));
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("the wake-delivery schema version reads"),
        71
    );
}

#[test]
fn v46_to_v47_canonicalizes_only_the_known_builtin_hash_and_every_reference() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    let legacy = legacy_operational_topology();
    assert_eq!(
        legacy.hash().as_str(),
        "36551ae60f0d354cfe5093b48f482f42227ce99d7cc704a02a3afa92e302dbf1"
    );
    {
        let connection = Connection::open(&path).expect("the v46 database opens");
        migrate_through_v46(&connection);
        seed_v46_operational_topology(&connection, &legacy);
    }

    let store = SqliteStore::open(&path).expect("the known v46 topology upgrades");
    // Opening migrates all the way forward, so this is the current version
    // rather than 47; what this test is about is the v47 canonicalization below.
    assert_eq!(
        store.schema_version().expect("the version reads"),
        SCHEMA_VERSION
    );
    drop(store);
    let connection = raw(&directory);
    const CANONICAL: &str = "c112faff3f0ad0d8893bd41a1a53215816e0bd93cd9d65ed359ba74d0822254b";
    for (table, column) in [
        ("topology_specs", "definition_hash"),
        ("project_topology_defaults", "canonical_hash"),
        ("mini_project_topology_snapshots", "canonical_hash"),
        ("topology_nodes", "spec_hash"),
    ] {
        let query = format!("SELECT {column} FROM {table} LIMIT 1");
        let hash: String = connection
            .query_row(&query, [], |row| row.get(0))
            .unwrap_or_else(|error| panic!("{table}.{column} reads: {error}"));
        assert_eq!(hash, CANONICAL, "{table}.{column} moved atomically");
    }
    let receipt: (String, String) = connection
        .query_row(
            "SELECT prior_hash, canonical_hash
             FROM topology_spec_canonicalization_receipts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the canonicalization receipt reads");
    assert_eq!(receipt.0, legacy.hash().as_str());
    assert_eq!(receipt.1, CANONICAL);
}

#[test]
fn v47_refuses_the_builtin_identity_when_its_prior_hash_is_unknown() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    let mut unknown: serde_json::Value =
        serde_json::from_str(legacy_operational_topology().json()).expect("the old spec parses");
    unknown["name"] = serde_json::json!("Unexpected legacy variant");
    let unknown = CanonicalDocument::from_value(&unknown).expect("the variant canonicalizes");
    {
        let connection = Connection::open(&path).expect("the v46 database opens");
        migrate_through_v46(&connection);
        seed_v46_operational_topology(&connection, &unknown);
    }

    let error = SqliteStore::open(&path).expect_err("an unknown prior hash must fail closed");
    assert!(error.to_string().contains("unknown prior hash"), "{error}");
    let connection = Connection::open(path).expect("the rolled-back v46 database reopens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the rolled-back version reads");
    assert_eq!(version, 46, "the failed migration wrote nothing");
}

#[test]
fn v47_refuses_an_unknown_reference_hash_before_rewriting_the_builtin() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    let legacy = legacy_operational_topology();
    {
        let connection = Connection::open(&path).expect("the v46 database opens");
        migrate_through_v46(&connection);
        seed_v46_operational_topology(&connection, &legacy);
        connection
            .execute(
                "UPDATE project_topology_defaults
                 SET canonical_hash = ?1",
                ["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
            )
            .expect("the inconsistent v46 reference is seeded");
    }

    let error = SqliteStore::open(&path).expect_err("an unknown reference hash must fail closed");
    assert!(
        error
            .to_string()
            .contains("unknown built-in topology reference hash"),
        "{error}"
    );
    let connection = Connection::open(path).expect("the rolled-back v46 database reopens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the rolled-back version reads");
    assert_eq!(version, 46, "the failed migration wrote nothing");
    let definition_hash: String = connection
        .query_row("SELECT definition_hash FROM topology_specs", [], |row| {
            row.get(0)
        })
        .expect("the built-in definition remains readable");
    assert_eq!(definition_hash, legacy.hash().as_str());
    let receipt_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'topology_spec_canonicalization_receipts'",
            [],
            |row| row.get(0),
        )
        .expect("the schema catalogue reads");
    assert_eq!(receipt_count, 0, "schema 47 rolled back as one transaction");
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

/// Master and the operational-recovery branch both shipped schema v35 with
/// different objects. This constructs master's durable shape -- escalation
/// brief plus a `publish_trigger` receipt, but no consultation tables -- and
/// proves the merge recognizes shape rather than trusting the colliding number.
#[test]
fn the_operational_hardening_v35_lineage_converges_without_losing_its_receipt() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    const REALM: &str = "0193f000-0000-7000-8000-0000000000f1";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000f2";
    const RECEIPT: &str = "0193f000-0000-7000-8000-0000000000f3";
    const HASH: &str = "a9d5f6d002d956b8af5787a05e0ca000d45c03977ffa54ee8fbed719fed5fd23";

    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        migrate_through_v33(&connection);
        connection
            .execute_batch(include_str!("../migrations/0037_escalation_brief.sql"))
            .expect("the historical escalation migration runs");
        connection
            .execute_batch(include_str!(
                "../migrations/0038_publish_trigger_command.sql"
            ))
            .expect("the historical publish-trigger receipt shape is built");
        connection
            .execute_batch("PRAGMA user_version = 35;")
            .expect("the historical lineage carries its shipped version");
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-18T06:00:00Z', NULL)",
                [REALM],
            )
            .expect("the Realm identity is written");
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, 'P', '/tmp/hardening-v35', 1, '2026-08-18T06:00:00Z')",
                [PROJECT],
            )
            .expect("the project is written");
        connection
            .execute(
                "INSERT INTO command_receipts
                     (id, project_id, idempotency_key, kind, target, target_revision,
                      intent, intent_hash, state, attempts, created_at, updated_at)
                 VALUES (?1, ?2, 'historical-publish-trigger', 'publish_trigger',
                         json_object('kind', 'project', 'project_id', ?2), 1,
                         json_object('schema_version', 1), ?3, 'intent_persisted', 0,
                         '2026-08-18T06:00:00Z', '2026-08-18T06:00:00Z')",
                rusqlite::params![RECEIPT, PROJECT, HASH],
            )
            .expect("the historical publish-trigger receipt is written");
    }

    let store = SqliteStore::open(&path).expect("the operational-hardening lineage converges");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(store.realm_id().to_string(), REALM);
    store
        .foreign_key_check()
        .expect("the receipt survives with sound references");

    let project = ProjectId::parse(PROJECT).expect("a project id");
    let receipt = kontor_core::id::CommandReceiptId::parse(RECEIPT).expect("a receipt id");
    let kept = store
        .get_receipt(project, receipt)
        .expect("the historical receipt decodes")
        .expect("the historical receipt survives");
    assert_eq!(kept.kind, kontor_core::receipt::CommandKind::PublishTrigger);

    let connection = raw(&directory);
    for table in [
        "consultation_profile_revisions",
        "completion_profile_revisions",
        "consultation_runs",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("the catalogue is readable");
        assert_eq!(exists, 1, "the converged table `{table}` is missing");
    }
    let receipt_triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema
             WHERE type = 'trigger' AND name LIKE 'command_receipts_%'",
            [],
            |row| row.get(0),
        )
        .expect("the trigger catalogue is readable");
    assert_eq!(
        receipt_triggers, 2,
        "the final rebuild restores both invariants"
    );
    let scopes: i64 = connection
        .query_row("SELECT count(*) FROM epic_execution_scopes", [], |row| {
            row.get(0)
        })
        .expect("the execution-scope table is readable");
    assert_eq!(
        scopes, 0,
        "the convergence must not invent runtime identity for historical epics"
    );
}

/// The currently deployed recovery daemon is schema v36. Its normal append-only
/// path must add the operational-hardening objects and keep its Realm identity.
#[test]
fn the_deployed_consultation_v36_lineage_upgrades_forward() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    const REALM: &str = "0193f000-0000-7000-8000-0000000000f4";

    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        migrate_through_v33(&connection);
        for migration in [
            include_str!("../migrations/0034_consultation_profiles.sql"),
            include_str!("../migrations/0035_epic_completion.sql"),
            include_str!("../migrations/0036_consultation_runs.sql"),
        ] {
            connection
                .execute_batch(migration)
                .expect("the deployed consultation lineage runs");
        }
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-18T06:00:00Z', NULL)",
                [REALM],
            )
            .expect("the deployed Realm identity is written");
    }

    let store = SqliteStore::open(&path).expect("the deployed v36 lineage upgrades");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(store.realm_id().to_string(), REALM);
    let connection = raw(&directory);
    let escalation_columns: i64 = connection
        .query_row(
            "SELECT count(*) FROM pragma_table_info('recovery_episodes')
             WHERE name IN ('escalation_recommendation',
                            'escalation_recommended_by', 'deliberation_path_json')",
            [],
            |row| row.get(0),
        )
        .expect("the recovery shape is readable");
    assert_eq!(escalation_columns, 3);
}

/// OP-12 owns schema 41, OP-14 appends imported lifecycle as schema 42, and
/// OP-17 appends per-epic runtime identity as schema 43. The whole merged chain
/// must upgrade without renumbering, replaying, or inventing identity for rows
/// created before the new declaration existed.
#[test]
fn the_merged_op12_v41_lineage_upgrades_through_epic_execution_scopes_v43() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    const REALM: &str = "0193f000-0000-7000-8000-0000000000f5";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000f6";
    const TASK: &str = "0193f000-0000-7000-8000-0000000000f7";

    {
        let connection = Connection::open(&path).expect("a raw connection opens");
        migrate_through_v33(&connection);
        for migration in [
            include_str!("../migrations/0034_consultation_profiles.sql"),
            include_str!("../migrations/0035_epic_completion.sql"),
            include_str!("../migrations/0036_consultation_runs.sql"),
            include_str!("../migrations/0037_escalation_brief.sql"),
            include_str!("../migrations/0038_publish_trigger_command.sql"),
            include_str!("../migrations/0039_committee_remediation.sql"),
            include_str!("../migrations/0040_advisor_advice.sql"),
            include_str!("../migrations/0041_open_questions.sql"),
        ] {
            connection
                .execute_batch(migration)
                .expect("the merged schema-41 lineage runs");
        }
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("schema 41 is readable");
        assert_eq!(version, 41);
        connection
            .execute(
                "INSERT INTO realm_metadata
                     (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, 1, '2026-08-19T12:00:00Z', NULL)",
                [REALM],
            )
            .expect("the deployed Realm identity is written");
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, 'P', '/tmp/op12-v41', 1, '2026-08-19T12:00:00Z')",
                [PROJECT],
            )
            .expect("the deployed project is written");
        connection
            .execute(
                "INSERT INTO tasks
                     (id, project_id, mini_project_id, title, module_key, state,
                      revision, created_at, updated_at)
                 VALUES (?1, ?2, NULL, 'Existing ready task', NULL, 'ready', 1,
                         '2026-08-19T12:00:00Z', '2026-08-19T12:00:00Z')",
                rusqlite::params![TASK, PROJECT],
            )
            .expect("the deployed task is written without future provenance");
    }

    let store = SqliteStore::open(&path).expect("the merged v41 lineage upgrades once");
    assert_eq!(store.schema_version().expect("readable"), SCHEMA_VERSION);
    assert_eq!(store.realm_id().to_string(), REALM);
    let connection = raw(&directory);
    let (state, imported_state): (String, Option<String>) = connection
        .query_row(
            "SELECT state, imported_state FROM tasks WHERE id = ?1",
            [TASK],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the upgraded task reads");
    assert_eq!(state, "ready");
    assert_eq!(
        imported_state, None,
        "migration 0042 must not invent historical provenance for an existing task"
    );
    let scopes: i64 = connection
        .query_row("SELECT count(*) FROM epic_execution_scopes", [], |row| {
            row.get(0)
        })
        .expect("the schema-43 execution-scope table reads");
    assert_eq!(
        scopes, 0,
        "migration 0043 must not invent runtime identity for an existing epic"
    );
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
    assert_eq!(store.busy_timeout_ms().expect("readable"), 30_000);

    // Reopening must re-apply the per-connection pragmas, not inherit them.
    drop(store);
    let reopened = open(&directory);
    assert!(
        reopened.foreign_keys_enabled().expect("readable"),
        "foreign keys must be re-enabled on every connection"
    );
    assert_eq!(reopened.busy_timeout_ms().expect("readable"), 30_000);
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

/// OP-REQ-036, enforced by the database and not only by the type.
///
/// `RecoveryAction::Escalate` already makes a briefless escalation
/// unrepresentable in Rust, which is the guarantee that matters for the code
/// that exists today. This is the guard for the code that does not: a repair
/// script, an import, or a future writer that reaches the table by another
/// route. An operator asked to decide something with no recommendation and no
/// account of what was tried is exactly the outcome the requirement exists to
/// prevent, whichever writer produced it.
#[test]
fn a_needs_human_row_cannot_be_written_without_its_brief() {
    let directory = temp();
    let _store = open(&directory);
    // Foreign keys are off for this one test. An episode references a project, a
    // task, a workflow and an agent run, and building all four would be four
    // fixtures' worth of setup for a question about none of them — while a
    // reference failure would also make the "accepted" half of the test pass for
    // the wrong reason. Triggers fire either way, which is what is under test.
    let connection =
        Connection::open(directory.path().join("kontor.db")).expect("a raw connection opens");
    connection
        .pragma_update(None, "foreign_keys", false)
        .expect("foreign keys can be disabled");

    let insert = |status: &str, brief: bool| {
        let (recommendation, author, path) = if brief {
            (
                Some("Cancel the seat and re-plan the ticket."),
                Some("lsa"),
                Some(
                    r#"{"deterministic_repair":true,"advisor":true,"committee":true,"followups":2}"#,
                ),
            )
        } else {
            (None, None, None)
        };
        connection.execute(
            "INSERT INTO recovery_episodes
                 (id, project_id, task_id, workflow_id, parked_agent_run_id, status,
                  cause_evaluation_id, advisor_used, committee_used, effective_followups,
                  successor_agent_run_id, escalation_cause, revision, created_at, closed_at,
                  escalation_recommendation, escalation_recommended_by, deliberation_path_json)
             VALUES (?1, '0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000002',
                     '0193f000-0000-7000-8000-000000000003',
                     '0193f000-0000-7000-8000-000000000004', ?2,
                     '0193f000-0000-7000-8000-000000000005', 1, 1, 2, NULL,
                     'budget_exhausted', 1, '2026-08-16T10:00:00Z',
                     '2026-08-16T11:00:00Z', ?3, ?4, ?5)",
            rusqlite::params![
                format!(
                    "0193f000-0000-7000-8000-00000000{:04}",
                    u32::from(brief) + 10
                ),
                status,
                recommendation,
                author,
                path
            ],
        )
    };

    assert!(
        insert("needs_human", false).is_err(),
        "a `needs_human` row with no recommendation, author or path must be refused"
    );
    insert("needs_human", true).expect("a complete escalation is accepted");

    // A non-terminal episode has nothing to recommend yet, so the rule applies
    // to the escalation and not to every row.
    connection
        .execute(
            "INSERT INTO recovery_episodes
                 (id, project_id, task_id, workflow_id, parked_agent_run_id, status,
                  cause_evaluation_id, advisor_used, committee_used, effective_followups,
                  successor_agent_run_id, escalation_cause, revision, created_at, closed_at)
             VALUES ('0193f000-0000-7000-8000-000000000020',
                     '0193f000-0000-7000-8000-000000000001',
                     '0193f000-0000-7000-8000-000000000002',
                     '0193f000-0000-7000-8000-000000000003',
                     '0193f000-0000-7000-8000-000000000004', 'open',
                     '0193f000-0000-7000-8000-000000000005', 0, 0, 0, NULL, NULL, 1,
                     '2026-08-16T10:00:00Z', NULL)",
            [],
        )
        .expect("an open episode needs no brief");

    // …and the update path is the one every real escalation actually takes.
    assert!(
        connection
            .execute(
                "UPDATE recovery_episodes
                 SET status = 'needs_human', escalation_cause = 'budget_exhausted',
                     closed_at = '2026-08-16T11:00:00Z'
                 WHERE id = '0193f000-0000-7000-8000-000000000020'",
                [],
            )
            .is_err(),
        "escalating by UPDATE must carry the brief too, or the trigger guards only the rarer path"
    );
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
fn a_profile_selection_outcome_is_an_immutable_receipt_to_policy_binding() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(RUN_FIXTURE)
        .expect("the workflow fixture inserts");
    connection
        .execute_batch(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision,
                  intent, intent_hash, state, attempts, created_at, updated_at,
                  execution_mode)
             VALUES
                 ('0193f000-0000-7000-8000-000000000050',
                  '0193f000-0000-7000-8000-000000000001', 'selection-outcome',
                  'select_task_profile', '{}', 1, '{}',
                  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                  'intent_persisted', 0, '2026-08-09T10:00:00Z',
                  '2026-08-09T10:00:00Z', 'local');
             INSERT INTO profile_selection_outcomes
                 (project_id, receipt_id, task_id, workflow_id, profile_key,
                  profile_version, profile_hash, team_template_id,
                  team_template_version, team_template_hash, applied, recorded_at)
             VALUES
                 ('0193f000-0000-7000-8000-000000000001',
                  '0193f000-0000-7000-8000-000000000050',
                  '0193f000-0000-7000-8000-000000000010',
                  '0193f000-0000-7000-8000-000000000030', 'q7.delivery', 1,
                  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                  '0193f000-0000-7000-8000-000000000020', 1,
                  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                  'created', '2026-08-09T10:00:00Z');",
        )
        .expect("the receipt and its exact outcome insert");

    assert!(
        connection
            .execute(
                "UPDATE profile_selection_outcomes SET applied = 'unchanged'",
                [],
            )
            .is_err(),
        "the historical result cannot follow a later active workflow"
    );
    assert!(
        connection
            .execute("DELETE FROM profile_selection_outcomes", [])
            .is_err(),
        "the receipt-to-policy binding cannot be withdrawn"
    );
}

#[test]
fn imported_profile_selection_outcome_lineage_is_immutable_and_non_authoritative() {
    let directory = temp();
    let _store = open(&directory);
    let connection = raw(&directory);
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('0193f000-0000-7000-8000-000000000001', 'destination', '/tmp/destination',
                     1, '2026-08-09T10:00:00Z');
             INSERT INTO import_receipts
                 (id, project_id, source_realm_id, export_schema_version,
                  source_schema_version, records_hash, exported_at, imported_at,
                  record_count, materialized_count)
             VALUES
                 ('0193f000-0000-7000-8000-000000000060',
                  '0193f000-0000-7000-8000-000000000001',
                  '0193f000-0000-7000-8000-000000000099', 3, 62,
                  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                  '2026-08-09T09:00:00Z', '2026-08-09T10:00:00Z', 1, 0);
             INSERT INTO imported_profile_selection_outcomes
                 (project_id, import_id, source_project_id, source_receipt_id, source_task_id,
                  source_workflow_id, profile_key, profile_version, profile_hash,
                  team_template_id, team_template_version, team_template_hash, applied,
                  source_recorded_at, source_record_hash)
             VALUES
                 ('0193f000-0000-7000-8000-000000000001',
                  '0193f000-0000-7000-8000-000000000060',
                  '0193f000-0000-7000-8000-000000000002',
                  '0193f000-0000-7000-8000-000000000003',
                  '0193f000-0000-7000-8000-000000000004',
                  '0193f000-0000-7000-8000-000000000005', 'q7.delivery', 1,
                  'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                  NULL, NULL, NULL, 'created', '2026-08-09T09:00:00Z',
                  'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc');",
        )
        .expect("the destination-owned lineage inserts without live source rows");

    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM command_receipts", [], |row| row
                .get::<_, i64>(0))
            .expect("the live authority table is readable"),
        0,
        "source receipt identity remains a reference, never live authority"
    );
    assert!(
        connection
            .execute(
                "UPDATE imported_profile_selection_outcomes SET applied = 'unchanged'",
                [],
            )
            .is_err(),
        "imported exact lineage cannot be rewritten"
    );
    assert!(
        connection
            .execute("DELETE FROM imported_profile_selection_outcomes", [])
            .is_err(),
        "imported exact lineage cannot be withdrawn"
    );
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
        (
            "imported_profile_selection_outcomes",
            &["project_id", "import_id"],
            "import_receipts",
            &["project_id", "id"],
        ),
        // --- structure -----------------------------------------------------
        ("mini_projects", &["project_id"], "projects", &["id"]),
        (
            "tasks",
            &["project_id", "mini_project_id"],
            "mini_projects",
            &["project_id", "id"],
        ),
        (
            "epic_completion_remediation_command_claims",
            &["project_id", "mini_project_id"],
            "mini_projects",
            &["project_id", "id"],
        ),
        (
            "committee_re_review_claims",
            &["project_id", "mini_project_id"],
            "mini_projects",
            &["project_id", "id"],
        ),
        (
            "committee_re_review_claims",
            &["project_id", "committee_run_id"],
            "consultation_runs",
            &["project_id", "run_id"],
        ),
        (
            "task_dependencies",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "task_modules",
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
            "profile_selection_outcomes",
            &["project_id", "receipt_id"],
            "command_receipts",
            &["project_id", "id"],
        ),
        (
            "profile_selection_outcomes",
            &["project_id", "task_id"],
            "tasks",
            &["project_id", "id"],
        ),
        (
            "profile_selection_outcomes",
            &["project_id", "workflow_id"],
            "task_workflows",
            &["project_id", "id"],
        ),
        (
            "profile_selection_outcomes",
            &["project_id", "profile_key", "profile_version"],
            "work_profiles",
            &["project_id", "profile_key", "version"],
        ),
        (
            "profile_selection_outcomes",
            &["project_id", "team_template_id", "team_template_version"],
            "team_templates",
            &["project_id", "template_id", "version"],
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

/// Every canonical migration up to and including v48, so a test can seed the
/// state a real realm was in before v49 ran.
fn migrate_through_v48(connection: &Connection) {
    migrate_through_v46(connection);
    for migration in [
        include_str!("../migrations/0047_configurable_native_names.sql"),
        include_str!("../migrations/0048_provider_quota_states.sql"),
    ] {
        connection
            .execute_batch(migration)
            .expect("the canonical migrations through v48 run");
    }
}

fn migrate_through_v53(connection: &Connection) {
    migrate_through_v48(connection);
    for migration in [
        include_str!("../migrations/0049_command_execution_mode.sql"),
        include_str!("../migrations/0050_provider_report_quota_source.sql"),
        include_str!("../migrations/0051_provider_quota_headroom.sql"),
        include_str!("../migrations/0052_task_modules_and_module_identity.sql"),
        include_str!("../migrations/0053_default_allow_admission.sql"),
    ] {
        connection
            .execute_batch(migration)
            .expect("the canonical migrations through v53 run");
    }
}

#[test]
fn v54_seeds_each_existing_project_without_claiming_pending_memory() {
    const REALM: &str = "0193f000-0000-7000-8000-0000000000c1";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000c2";
    let directory = temp();
    let path = directory.path().join("kontor.db");
    {
        let connection = Connection::open(&path).expect("the v53 database opens");
        migrate_through_v53(&connection);
        connection
            .execute_batch(&format!(
                "INSERT INTO realm_metadata
                 (singleton, realm_id, schema_version, created_at, display_label)
             VALUES (1, '{REALM}', 1, '2026-08-23T09:00:00Z', NULL);
             INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES ('{PROJECT}', 'P', '/tmp/v54-authority', 1, '2026-08-23T09:00:00Z');"
            ))
            .expect("the v53 realm is seeded");
    }

    let store = SqliteStore::open(&path).expect("the v53 realm upgrades");
    let project = ProjectId::parse(PROJECT).expect("a canonical project id");
    let memory = store
        .subject_authority(project, AuthoritySubject::Memory)
        .expect("memory authority is seeded");
    let backlog = store
        .subject_authority(project, AuthoritySubject::Backlog)
        .expect("backlog authority is seeded");
    assert_eq!(memory.origin, SubjectOrigin::LegacyPending);
    assert!(!memory.writable_by_kontor());
    assert_eq!(backlog.origin, SubjectOrigin::KontorNative);
    assert!(backlog.writable_by_kontor());
}

fn seed_v48_confirmed_abandonment(connection: &Connection) {
    const REALM: &str = "0193f000-0000-7000-8000-0000000000b1";
    const PROJECT: &str = "0193f000-0000-7000-8000-0000000000b2";
    const RECEIPT: &str = "0193f000-0000-7000-8000-0000000000b3";
    connection
        .execute(
            "INSERT INTO realm_metadata
                 (singleton, realm_id, schema_version, created_at, display_label)
             VALUES (1, ?1, 1, '2026-08-21T18:00:00Z', NULL)",
            [REALM],
        )
        .expect("the v48 Realm is seeded");
    connection
        .execute(
            "INSERT INTO projects (id, name, root_path, revision, created_at)
             VALUES (?1, 'P', '/tmp/v48-abandon', 1, '2026-08-21T18:00:00Z')",
            [PROJECT],
        )
        .expect("the project is seeded");
    connection
        .execute(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision,
                  intent, intent_hash, state, attempts, created_at, updated_at)
             VALUES (?1, ?2, 'abandon-1', 'abandon_run', '{}', 1, '{}',
                     '0000000000000000000000000000000000000000000000000000000000000000',
                     'confirmed', 0, '2026-08-21T18:00:00Z', '2026-08-21T18:00:00Z')",
            [RECEIPT, PROJECT],
        )
        .expect("a confirmed operator abandonment is seeded");
}

/// v49 backfills `execution_mode` on receipts that are already `confirmed`, and
/// `command_receipts_identity_immutable` (v47) aborts *any* update whose OLD row
/// is confirmed -- not merely one that moves an identity column. So the
/// migration is fine on a fresh database and cannot run on any realm that has
/// ever abandoned a run, which is what took the live realm down on 2026-08-22.
#[test]
fn v49_backfills_a_confirmed_receipt_without_tripping_the_immutability_trigger() {
    let directory = temp();
    let path = directory.path().join("kontor.db");
    {
        let connection = Connection::open(&path).expect("the v48 database opens");
        migrate_through_v48(&connection);
        seed_v48_confirmed_abandonment(&connection);
    }

    let store = SqliteStore::open(&path).expect("a realm with a confirmed abandonment migrates");
    assert_eq!(
        store.schema_version().expect("the version reads"),
        SCHEMA_VERSION
    );
    drop(store);

    let connection = raw(&directory);
    let mode: String = connection
        .query_row(
            "SELECT execution_mode FROM command_receipts WHERE kind = 'abandon_run'",
            [],
            |row| row.get(0),
        )
        .expect("the receipt reads back");
    assert_eq!(mode, "local", "the backfill must actually have run");

    // And the trigger is back: suspending it is scoped to the migration's own
    // transaction, not a permanent relaxation.
    let error = connection
        .execute(
            "UPDATE command_receipts SET attempts = attempts + 1 WHERE kind = 'abandon_run'",
            [],
        )
        .expect_err("a confirmed receipt is immutable again");
    assert!(
        error.to_string().contains("identity is immutable"),
        "the v47 trigger must be reinstated: {error}"
    );
}
