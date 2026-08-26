//! The redacted export document and the redacted import that reads it.
//!
//! The fixture is one seeded Realm carrying a row of every shape the export has
//! an opinion about: a versioned specification, an account profile with a
//! credential reference, an inbound external comment with prose in it, a
//! command receipt and a control-plane observation.
//!
//! The mutants this suite exists to kill:
//!
//! * exporting a credential reference, an environment mapping or an external
//!   comment body;
//! * publishing a document a canary matched, or scanning only its top level;
//! * a document whose bytes or digest change when nothing did;
//! * reading a future export generation as if it were this one;
//! * importing into the wrong Realm, into no project, or without a destination
//!   receipt;
//! * writing a source command receipt into the destination's own receipt
//!   tables;
//! * losing the lineage of a record that was deliberately not materialized.
//! * omitting a task's imported historical lifecycle provenance.

use std::path::{Path, PathBuf};

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, BoundedText, CanonicalDocument,
    CommandReceiptId, ConnectorKey, ContentHash, ContextPackId, CredentialAlias, ExternalId,
    ExternalName, IdempotencyKey, ProjectId, RoleSlotId, RuntimeKindKey, SCHEMA_VERSION,
    SpecVersion, TaskId, TaskWorkflowId, TeamRunId, TicketLinkId, Timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CredentialReference, CredentialReferenceKind, NewAccountProfile, NewAgentRun, NewLocalCommand,
    NewProject, NewRuntimeEvent, NewTask, NewTaskWorkflow, NewTeamRun, NewTicketLink,
    ProjectRepository, RunRepository, SpecRepository, TicketRepository,
};
use kontor_core::spec::{ResolvedWorkProfileSnapshot, TeamRunSnapshot};
use kontor_core::state::TaskState;
use kontor_core::ticket::ExternalCommentRevision;
use kontor_store::backup::{
    BackupError, EXPORT_SCHEMA_VERSION, ImportPlan, KontorExportV1, export_realm, import_export,
};
use kontor_store::{ProfileSelection, SqliteStore, TeamTemplateSource};
use tempfile::TempDir;

/// The prose an inbound Jira comment can carry, and the reason bodies are not
/// exported: this is the shape Zone C material arrives in.
const COMMENT_BODY: &str = "Ola Nordmann rang about the appointment on Tuesday.";

/// An opaque credential alias. It resolves to nothing without the resolver
/// policy, and it still must not leave the Realm.
const CREDENTIAL_ALIAS: &str = "kontor-prod-openai";

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn document(value: &serde_json::Value) -> CanonicalDocument {
    CanonicalDocument::from_value(value).expect("a canonical document")
}

fn rehash_records(document: &mut serde_json::Value) {
    let mut records = serde_json::to_vec(
        document
            .get("records")
            .expect("the export document carries records"),
    )
    .expect("the records serialize");
    records.push(b'\n');
    document["records_hash"] = serde_json::json!(ContentHash::of(&records).to_string());
}

/// One Realm with a row of every exported shape in it.
struct Seeded {
    /// Kept for its `Drop`.
    _home: TempDir,
    store: SqliteStore,
    database: PathBuf,
    project: ProjectId,
    task: TaskId,
    profile_hash: ContentHash,
}

/// Count a table through a second connection.
///
/// The store deliberately has no raw-SQL escape hatch, and these assertions are
/// about tables the public API cannot reach — which is the point: an import must
/// not have written them by any route.
fn count(database: &Path, table: &str) -> i64 {
    let connection =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("the database opens");
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("the table is readable")
}

/// Plant a row the domain's own boundary would have refused.
fn plant(database: &Path, statement: &str) {
    let connection = rusqlite::Connection::open(database).expect("the database opens");
    connection
        .execute_batch(statement)
        .expect("the row is planted");
}

fn seed() -> Seeded {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the database migrates");
    let project = ProjectId::generate();
    let task = TaskId::generate();
    let team_run = TeamRunId::generate();
    let agent_run = AgentRunId::generate();
    let created = at("2026-08-10T09:00:00Z");

    store
        .create_project(&NewProject {
            id: project,
            name: name("Exported project"),
            root_path: name("/tmp/kontor-export"),
            created_at: created,
        })
        .expect("a project is created");
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: None,
            title: name("An exported task"),
            module: None,
            state: TaskState::Ready,
            created_at: created,
        })
        .expect("a task is created");

    // Real specifications from the bundled pack: a hand-rolled document would
    // test a shape no deployment has, and the import path re-validates these
    // through the same domain types.
    let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
        .expect("the pack seeds at least one category");
    let bundle = kontor_profiles::pack::resolve_profile(&pack, &entry.category, created)
        .expect("the seeded category resolves");
    let profile_hash = store
        .insert_work_profile(project, &bundle.profile.definition)
        .expect("the profile revision is stored");
    let team = bundle.team.clone().expect("the profile pinned a team");
    store
        .insert_team_template(project, &team)
        .expect("the team revision is stored");
    store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: project,
            task_id: task,
            snapshot: TeamRunSnapshot::from_revision(&team, SCHEMA_VERSION),
            created_at: created,
        })
        .expect("a team run is created");
    store
        .create_agent_run(&NewAgentRun {
            id: agent_run,
            project_id: project,
            team_run_id: team_run,
            parent_agent_run_id: None,
            role: RoleSlotId::parse("exported-seat")
                .expect("a valid slot key")
                .into_role_key(),
            account_profile_id: None,
            binding: None,
            created_at: created,
        })
        .expect("an agent run is created");

    // An account profile with a credential *reference*: the alias and the
    // environment mapping are exactly what must not be exported.
    store
        .create_account_profile(&NewAccountProfile {
            id: AccountProfileId::generate(),
            project_id: project,
            label: name("Production account"),
            external_account_id: None,
            harness: RuntimeKindKey::parse("paseo").expect("a valid family key"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::Keychain,
                alias: CredentialAlias::parse(CREDENTIAL_ALIAS).expect("a valid alias"),
            },
            environment: document(&serde_json::json!({
                "schema_version": 1,
                "OPENAI_API_KEY": CREDENTIAL_ALIAS,
            })),
            routing: document(&serde_json::json!({"schema_version": 1, "provider": "openai"})),
            capability: document(&serde_json::json!({"schema_version": 1, "streaming": true})),
            provider_identity: Some(ExternalId::parse("acct-7742").expect("a valid external id")),
            enabled: true,
            created_at: created,
        })
        .expect("an account profile is created");

    // An inbound external comment: identity and digest are evidence, the body is
    // prose from a system Kontor does not own.
    let link = TicketLinkId::generate();
    store
        .create_ticket_link(&NewTicketLink {
            id: link,
            project_id: project,
            task_id: task,
            connector: ConnectorKey::parse("jira").expect("a valid connector key"),
            external_issue_key: ExternalId::parse("ASMA-7763").expect("a valid issue key"),
            created_at: created,
        })
        .expect("a ticket link is created");
    store
        .append_comment(
            project,
            &ExternalCommentRevision {
                link_id: link,
                external_comment_id: ExternalId::parse("comment-1").expect("a valid comment id"),
                author_account_id: ExternalId::parse("author-1").expect("a valid account id"),
                author_display: Some(name("A Reporter")),
                external_created_at: created,
                external_updated_at: created,
                body: BoundedText::parse(COMMENT_BODY).expect("a valid body"),
                body_hash: ContentHash::of(COMMENT_BODY.as_bytes()),
                observed_at: created,
                supersedes: None,
            },
        )
        .expect("the inbound comment is stored");

    // One control-plane observation: control metadata only, which the export
    // re-checks before it publishes anything.
    store
        .append_runtime_event(&NewRuntimeEvent {
            project_id: project,
            agent_run_id: agent_run,
            identity: kontor_core::state::NativeRuntimeIdentity {
                runtime_kind: RuntimeKindKey::parse("paseo").expect("a valid family key"),
                host: name("localhost"),
                generation: 1,
                native_id: ExternalId::parse("native-1").expect("a valid native id"),
            },
            native_event_id: Some(ExternalId::parse("event-1").expect("a valid event id")),
            native_sequence: 1,
            payload: document(&serde_json::json!({
                "schema_version": 1,
                "lifecycle": "running",
                "observed_at": "2026-08-10T09:00:00Z",
            })),
            observed_at: created,
        })
        .expect("the control observation is stored");

    Seeded {
        _home: home,
        store,
        database,
        project,
        task,
        profile_hash,
    }
}

/// Record two selection commands against consecutive immutable policy
/// revisions, returning their exact historical bindings.
fn seed_profile_selection_outcomes(
    seeded: &Seeded,
) -> [kontor_store::StoredProfileSelectionOutcome; 2] {
    let first_at = at("2026-08-10T09:01:00Z");
    let second_at = at("2026-08-10T09:02:00Z");
    let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
        .expect("the pack seeds at least one category");
    let first_bundle = kontor_profiles::pack::resolve_profile(&pack, &entry.category, first_at)
        .expect("the first profile resolves");
    let first_team = first_bundle.team.clone().expect("the profile pins a team");
    let first_command = NewLocalCommand {
        project_id: seeded.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("backup-profile-selection-k").expect("a valid key"),
        kind: CommandKind::SelectTaskProfile,
        target: AggregateRef::Task {
            task_id: seeded.task,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: document(&serde_json::json!({
            "schema_version": 1,
            "marker": "profile-selection-p1",
        })),
        created_at: first_at,
    };
    let first_workflow = NewTaskWorkflow {
        id: TaskWorkflowId::generate(),
        project_id: seeded.project,
        task_id: seeded.task,
        snapshot: first_bundle.profile.clone(),
        current_phase: first_bundle.profile.definition.entry_phase.clone(),
        created_at: first_at,
    };
    let first = seeded
        .store
        .apply_profile_selection(&ProfileSelection {
            command: &first_command,
            workflow: &first_workflow,
            definition: &first_bundle.profile.definition,
            team: Some(&first_team),
            team_source: TeamTemplateSource::Bundled,
        })
        .expect("K selects P1");

    let mut second_definition = first_bundle.profile.definition.clone();
    second_definition.version =
        SpecVersion::parse(second_definition.version.get() + 1).expect("the next profile version");
    let mut second_team = first_team.clone();
    second_team.version =
        SpecVersion::parse(second_team.version.get() + 1).expect("the next team version");
    second_definition
        .team_template
        .as_mut()
        .expect("the profile pins a team")
        .version = second_team.version;
    let second_snapshot = ResolvedWorkProfileSnapshot::resolve(&second_definition, second_at)
        .expect("the second profile resolves");
    let second_command = NewLocalCommand {
        project_id: seeded.project,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("backup-profile-selection-k2").expect("a valid key"),
        kind: CommandKind::SelectTaskProfile,
        target: AggregateRef::Task {
            task_id: seeded.task,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: document(&serde_json::json!({
            "schema_version": 1,
            "marker": "profile-selection-p2",
        })),
        created_at: second_at,
    };
    let second_workflow = NewTaskWorkflow {
        id: TaskWorkflowId::generate(),
        project_id: seeded.project,
        task_id: seeded.task,
        snapshot: second_snapshot,
        current_phase: second_definition.entry_phase.clone(),
        created_at: second_at,
    };
    let second = seeded
        .store
        .apply_profile_selection(&ProfileSelection {
            command: &second_command,
            workflow: &second_workflow,
            definition: &second_definition,
            team: Some(&second_team),
            team_source: TeamTemplateSource::Registered,
        })
        .expect("K2 selects P2");
    [first, second]
}

#[test]
fn an_export_of_unchanged_state_is_byte_identical_and_hashes_the_same() {
    let seeded = seed();
    let first = export_realm(&seeded.store, at("2026-08-10T10:00:00Z")).expect("the first export");
    let second =
        export_realm(&seeded.store, at("2026-08-10T18:30:00Z")).expect("the second export");

    assert_eq!(
        first.canonical_records_bytes().expect("canonical records"),
        second.canonical_records_bytes().expect("canonical records"),
        "two exports of one unchanged realm must produce identical record bytes"
    );
    assert_eq!(first.records_hash, second.records_hash);
    assert_ne!(
        first.exported_at, second.exported_at,
        "the instant is the one value that is allowed to differ"
    );
    assert_ne!(
        first.canonical_bytes().expect("canonical bytes"),
        second.canonical_bytes().expect("canonical bytes"),
        "and it is carried outside the hashed records, in the document"
    );

    // The digest is over the records the document carries, and it is checked
    // rather than trusted.
    first.verify().expect("the first export verifies");
    let mut tampered = first.clone();
    tampered.records.projects.clear();
    assert!(
        matches!(tampered.verify(), Err(BackupError::Verification { .. })),
        "a document whose records were edited must not verify"
    );
}

#[test]
fn imported_task_lifecycle_provenance_survives_export_serialization_and_parse() {
    let seeded = seed();
    plant(
        &seeded.database,
        "UPDATE tasks
         SET state = 'done', imported_state = 'completed'
         WHERE id = (SELECT id FROM tasks ORDER BY id LIMIT 1)",
    );

    let export = export_realm(&seeded.store, at("2026-08-10T10:00:00Z")).expect("the export");
    let bytes = export.canonical_bytes().expect("canonical bytes");
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("the export is JSON");
    assert_eq!(
        document.pointer("/records/tasks/0/imported_state"),
        Some(&serde_json::json!("completed")),
        "the typed task record must carry historical lifecycle provenance"
    );

    let parsed = KontorExportV1::parse(&bytes).expect("the provenance-bearing export parses");
    assert_eq!(parsed.records_hash, export.records_hash);
    let reparsed = serde_json::to_value(parsed).expect("the parsed export serializes");
    assert_eq!(
        reparsed.pointer("/records/tasks/0/imported_state"),
        Some(&serde_json::json!("completed")),
        "parse and serialization must preserve the source lifecycle fact"
    );

    let mut legacy = document;
    legacy
        .pointer_mut("/records/tasks/0")
        .and_then(serde_json::Value::as_object_mut)
        .expect("the legacy task row is an object")
        .remove("imported_state");
    let mut legacy_records = serde_json::to_vec(
        legacy
            .get("records")
            .expect("the legacy document carries records"),
    )
    .expect("the legacy records serialize");
    legacy_records.push(b'\n');
    legacy["records_hash"] = serde_json::json!(ContentHash::of(&legacy_records).to_string());
    let mut legacy_bytes = serde_json::to_vec(&legacy).expect("the legacy export serializes");
    legacy_bytes.push(b'\n');

    let legacy = KontorExportV1::parse(&legacy_bytes)
        .expect("a valid v1 export from before the optional provenance field still parses");
    assert_eq!(legacy.schema_version, EXPORT_SCHEMA_VERSION);
    assert_eq!(legacy.records.tasks[0].imported_state, None);
}

#[test]
fn an_export_carries_no_credential_reference_no_comment_body_and_no_secret() {
    let seeded = seed();
    let export = export_realm(&seeded.store, at("2026-08-10T10:00:00Z")).expect("the export");
    let bytes = export.canonical_bytes().expect("canonical bytes");
    let text = String::from_utf8(bytes).expect("the document is UTF-8");

    // Values, not column names: the redaction summary names the withheld
    // columns on purpose, and asserting on those names would be asserting that
    // the document does not explain itself.
    for canary in [
        CREDENTIAL_ALIAS,
        COMMENT_BODY,
        "Ola Nordmann",
        "OPENAI_API_KEY",
    ] {
        assert!(
            !text.contains(canary),
            "the export must not carry `{canary}`"
        );
    }
    assert!(
        !text.contains("\"claim_token\""),
        "a live dispatch claim is never a field of an export"
    );

    // What *is* carried: the opaque provider identity, the comment's identity
    // and digest, and the cursor evidence.
    assert!(
        text.contains("acct-7742"),
        "the opaque profile identity is provenance"
    );
    assert_eq!(export.records.external_comments.len(), 1);
    assert_eq!(
        export.records.external_comments[0].body_hash,
        ContentHash::of(COMMENT_BODY.as_bytes()).to_string(),
        "the body's digest is exported so an importer can prove continuity"
    );

    // And the document says what it withheld rather than leaving it to be
    // inferred from an absence.
    assert!(export.redaction_summary.canary_scanned);
    assert!(
        export
            .redaction_summary
            .excluded_columns
            .contains_key("external_comments.body")
    );
    assert!(
        export
            .redaction_summary
            .excluded_columns
            .contains_key("account_profiles.credential_ref_alias")
    );
    assert!(
        export
            .redaction_summary
            .excluded_tables
            .contains_key("resource_leases")
    );
    assert!(
        !export
            .redaction_summary
            .excluded_tables
            .contains_key("profile_selection_outcomes"),
        "the non-secret exact selection result is exported, not redacted"
    );
    for destination_local in [
        "import_receipts",
        "imported_records",
        "imported_profile_selection_outcomes",
    ] {
        assert!(
            export
                .redaction_summary
                .excluded_tables
                .contains_key(destination_local),
            "the destination-local lineage exclusion must be disclosed"
        );
    }

    // The continuity summary is derived from the records, so it cannot claim a
    // completeness they do not support.
    assert_eq!(export.continuity_summary.control_gaps, 0);
    assert_eq!(export.continuity_summary.content_gaps, 0);
    assert_eq!(
        export.continuity_summary.highest_control_cursor,
        export
            .records
            .runtime_events
            .iter()
            .map(|event| event.cursor)
            .max()
            .expect("the realm recorded at least one control event"),
        "the reported cursor is the one the records actually carry"
    );
    assert_eq!(
        export.continuity_summary.record_counts.get("projects"),
        Some(&1)
    );
}

#[test]
fn a_canary_inside_a_stored_document_aborts_the_export() {
    let seeded = seed();
    // A persisted document that carries credential material — which the domain's
    // own boundary refuses on the way in, so it is planted here through raw SQL
    // in a column that stores a canonical document. The point is that the export
    // scanner does not trust that boundary to have held: it opens every stored
    // document and scans it as structure, not as an opaque string.
    let planted = r#"{"schema_version":1,"api_key":"sk-0123456789abcdef0123456789"}"#;
    plant(
        &seeded.database,
        &format!(
            "INSERT INTO context_packs (id, project_id, task_id, content, content_hash, created_at)
             SELECT '{}', project_id, id, '{planted}',
                    '0000000000000000000000000000000000000000000000000000000000000000',
                    '2026-08-10T09:00:00Z'
             FROM tasks LIMIT 1",
            ContextPackId::generate()
        ),
    );

    let refused = export_realm(&seeded.store, at("2026-08-10T10:00:00Z"))
        .expect_err("a document carrying credential material is never published");
    match refused {
        BackupError::Redaction { path } => assert!(
            !path.contains("sk-0123456789"),
            "a refusal names the path, never the value it found"
        ),
        other => panic!("the export must refuse as a redaction failure: {other:?}"),
    }
}

#[test]
fn a_future_export_generation_is_refused_and_this_one_parses() {
    let seeded = seed();
    let export = export_realm(&seeded.store, at("2026-08-10T10:00:00Z")).expect("the export");
    let bytes = export.canonical_bytes().expect("canonical bytes");

    let parsed = KontorExportV1::parse(&bytes).expect("this generation parses");
    assert_eq!(parsed.schema_version, EXPORT_SCHEMA_VERSION);
    assert_eq!(parsed.records_hash, export.records_hash);

    let mut future: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the document is JSON");
    future["schema_version"] = serde_json::json!(EXPORT_SCHEMA_VERSION + 1);
    let refused = KontorExportV1::parse(&serde_json::to_vec(&future).expect("bytes"))
        .expect_err("a later generation is refused");
    match refused {
        BackupError::UnsupportedExportVersion { found, expected } => {
            assert_eq!(found, EXPORT_SCHEMA_VERSION + 1);
            assert_eq!(expected, EXPORT_SCHEMA_VERSION);
        }
        other => panic!("the refusal must be typed: {other:?}"),
    }
}

#[test]
fn generation_two_without_profile_selection_outcomes_remains_importable() {
    let source = seed();
    let current =
        export_realm(&source.store, at("2026-08-10T10:00:00Z")).expect("the current export");
    assert!(current.records.profile_selection_outcomes.is_empty());
    let mut legacy = serde_json::to_value(&current).expect("the export serializes");
    legacy["schema_version"] = serde_json::json!(2);
    legacy
        .pointer_mut("/records")
        .and_then(serde_json::Value::as_object_mut)
        .expect("records are an object")
        .remove("profile_selection_outcomes");
    legacy
        .pointer_mut("/continuity_summary/record_counts")
        .and_then(serde_json::Value::as_object_mut)
        .expect("record counts are an object")
        .remove("profile_selection_outcomes");
    rehash_records(&mut legacy);
    legacy["database_schema_version"] = serde_json::json!(62);
    assert!(
        matches!(
            KontorExportV1::parse(&serde_json::to_vec(&legacy).expect("ambiguous bytes")),
            Err(BackupError::Verification { .. })
        ),
        "a v2 document made after outcomes existed cannot prove that it omitted none"
    );
    legacy["database_schema_version"] = serde_json::json!(61);
    let legacy = KontorExportV1::parse(&serde_json::to_vec(&legacy).expect("legacy bytes"))
        .expect("generation two from before outcome persistence remains readable");
    assert_eq!(legacy.schema_version, 2);
    assert!(legacy.records.profile_selection_outcomes.is_empty());

    let home = TempDir::new().expect("a temporary directory");
    let destination_database = home.path().join("kontor.db");
    let destination = SqliteStore::open(&destination_database).expect("the destination migrates");
    let into = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: into,
            name: name("Legacy destination"),
            root_path: name("/tmp/kontor-legacy-destination"),
            created_at: at("2026-08-11T09:00:00Z"),
        })
        .expect("the destination project exists");
    let report = import_export(
        &destination,
        &legacy,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-11T10:00:00Z"),
    )
    .expect("a generation-two export imports");
    assert_eq!(
        report.record_count,
        legacy.records.lineage().unwrap().len() as u64
    );
    assert_eq!(
        count(&destination_database, "profile_selection_outcomes"),
        0
    );
    assert_eq!(
        count(&destination_database, "imported_profile_selection_outcomes"),
        0
    );
}

#[test]
fn profile_selection_outcomes_round_trip_as_exact_non_executable_lineage() {
    let source = seed();
    let [first, second] = seed_profile_selection_outcomes(&source);
    let export = export_realm(&source.store, at("2026-08-10T10:00:00Z"))
        .expect("the outcome-bearing export");
    assert_eq!(export.schema_version, 3);
    assert_eq!(export.records.profile_selection_outcomes.len(), 2);
    assert_eq!(
        export
            .continuity_summary
            .record_counts
            .get("profile_selection_outcomes"),
        Some(&2),
        "the new immutable evidence is disclosed in continuity counts"
    );
    assert_ne!(first.profile.2, second.profile.2);
    assert_ne!(first.workflow_id, second.workflow_id);
    let bytes = export.canonical_bytes().expect("canonical bytes");
    let parsed = KontorExportV1::parse(&bytes).expect("the v3 export parses");
    assert_eq!(parsed.records_hash, export.records_hash);
    assert_eq!(
        parsed.records.profile_selection_outcomes,
        export.records.profile_selection_outcomes
    );

    let home = TempDir::new().expect("a temporary directory");
    let destination_database = home.path().join("kontor.db");
    let destination = SqliteStore::open(&destination_database).expect("the destination migrates");
    let into = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: into,
            name: name("Outcome lineage destination"),
            root_path: name("/tmp/kontor-outcome-lineage"),
            created_at: at("2026-08-11T09:00:00Z"),
        })
        .expect("the destination project exists");
    let report = import_export(
        &destination,
        &parsed,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-11T10:00:00Z"),
    )
    .expect("the v3 export imports");
    assert_eq!(count(&destination_database, "command_receipts"), 0);
    assert_eq!(count(&destination_database, "task_workflows"), 0);
    assert_eq!(
        count(&destination_database, "profile_selection_outcomes"),
        0
    );

    let import_id = report.import_id.as_hyphenated().to_string();
    let imported = destination
        .imported_profile_selection_outcomes(&import_id)
        .expect("the exact lineage reads back");
    assert_eq!(imported.len(), 2);
    for outcome in [&first, &second] {
        let row = imported
            .iter()
            .find(|row| row.source_receipt_id == outcome.receipt_id.to_string())
            .expect("each exact receipt binding survives");
        assert_eq!(row.source_project_id, source.project.to_string());
        assert_eq!(row.source_task_id, source.task.to_string());
        assert_eq!(row.source_workflow_id, outcome.workflow_id.to_string());
        assert_eq!(row.profile_key, outcome.profile.0.as_str());
        assert_eq!(row.profile_version, i64::from(outcome.profile.1.get()));
        assert_eq!(row.profile_hash, outcome.profile.2.as_str());
        assert_eq!(
            row.team_template_id.as_deref(),
            outcome
                .team
                .as_ref()
                .map(|team| team.0.to_string())
                .as_deref()
        );
        assert_eq!(row.source_record_hash.len(), 64);
        let generic = destination
            .imported_records(&import_id)
            .expect("generic lineage reads back")
            .into_iter()
            .find(|generic| {
                generic.record_kind == "profile_selection_outcomes"
                    && generic.source_identity
                        == format!("{}/{}", source.project, outcome.receipt_id)
            })
            .expect("the exact row also has generic lineage");
        assert_eq!(generic.disposition, "recorded");
        assert_eq!(generic.source_hash, row.source_record_hash);
    }
}

#[test]
fn an_import_refuses_an_outcome_that_does_not_match_its_source_policy() {
    let source = seed();
    seed_profile_selection_outcomes(&source);
    let export = export_realm(&source.store, at("2026-08-10T10:00:00Z")).expect("the export");
    let mut document = serde_json::to_value(export).expect("the export serializes");
    document["records"]["profile_selection_outcomes"][0]["profile_hash"] =
        serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    rehash_records(&mut document);
    let tampered = KontorExportV1::parse(&serde_json::to_vec(&document).expect("bytes"))
        .expect("the internally hashed document parses");

    let home = TempDir::new().expect("a temporary directory");
    let destination_database = home.path().join("kontor.db");
    let destination = SqliteStore::open(&destination_database).expect("the destination migrates");
    let into = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: into,
            name: name("Refusing destination"),
            root_path: name("/tmp/kontor-refusing-destination"),
            created_at: at("2026-08-11T09:00:00Z"),
        })
        .expect("the destination project exists");
    let refused = import_export(
        &destination,
        &tampered,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-11T10:00:00Z"),
    )
    .expect_err("mismatched outcome lineage is refused before any write");
    assert!(matches!(refused, BackupError::Verification { .. }));
    assert!(destination.import_receipts().expect("readable").is_empty());
}

#[test]
fn an_import_mints_a_destination_receipt_and_replays_no_source_receipt() {
    let source = seed();
    let export = export_realm(&source.store, at("2026-08-10T10:00:00Z")).expect("the export");

    // A separately initialized destination realm, with its own project.
    let home = TempDir::new().expect("a temporary directory");
    let destination_database = home.path().join("kontor.db");
    let destination = SqliteStore::open(&destination_database).expect("the destination migrates");
    assert_ne!(destination.realm_id(), source.store.realm_id());
    let into = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: into,
            name: name("Destination project"),
            root_path: name("/tmp/kontor-destination"),
            created_at: at("2026-08-11T09:00:00Z"),
        })
        .expect("the destination project exists");

    let report = import_export(
        &destination,
        &export,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-11T10:00:00Z"),
    )
    .expect("the export is imported");

    assert_eq!(report.source_realm_id, source.store.realm_id());
    assert_eq!(report.destination_project, into);
    assert!(
        report.materialized >= 2,
        "the specifications are materialized"
    );
    assert!(report.recorded > 0, "everything else is kept as lineage");
    assert!(report.reconciliation_required);

    // The destination minted its own receipt, and it names the source.
    let receipts = destination
        .import_receipts()
        .expect("the receipts are readable");
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].source_realm_id,
        export.source_realm_id.to_string()
    );
    assert_eq!(receipts[0].records_hash, export.records_hash.to_string());
    assert_eq!(
        receipts[0].export_schema_version,
        i64::from(EXPORT_SCHEMA_VERSION)
    );
    assert_eq!(receipts[0].imported_at, "2026-08-11T10:00:00Z");
    assert_eq!(receipts[0].exported_at, "2026-08-10T10:00:00Z");

    // No source receipt, observation, comment or account profile became
    // destination state. They are lineage, and only lineage.
    for table in [
        "command_receipts",
        "runtime_events",
        "external_comments",
        "account_profiles",
        "agent_runs",
        "team_runs",
        "tasks",
    ] {
        assert_eq!(
            count(&destination_database, table),
            0,
            "`{table}` must not be written by an import"
        );
    }

    let lineage = destination
        .imported_records(&receipts[0].id)
        .expect("the lineage is readable");
    let dispositions: Vec<(&str, &str)> = lineage
        .iter()
        .map(|record| (record.record_kind.as_str(), record.disposition.as_str()))
        .collect();
    assert!(
        dispositions.contains(&("work_profiles", "materialized")),
        "a versioned specification is re-validated and materialized"
    );
    for kind in ["runtime_events", "external_comments", "account_profiles"] {
        assert!(
            dispositions.contains(&(kind, "recorded")),
            "`{kind}` must survive as source-referenced lineage"
        );
    }
    // The lineage carries the source's digest, so a later reader can prove which
    // bytes it came from.
    let profile = lineage
        .iter()
        .find(|record| record.record_kind == "work_profiles")
        .expect("the profile's lineage was recorded");
    assert_eq!(profile.source_hash.len(), 64);

    // And the materialized specification really is in the destination, with the
    // source's own digest.
    assert!(count(&destination_database, "work_profiles") >= 1);
    assert_eq!(
        source.profile_hash.to_string().len(),
        64,
        "the source's digest is what the destination reproduced"
    );

    // Re-importing the same document is refused rather than duplicated.
    let refused = import_export(
        &destination,
        &export,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-11T11:00:00Z"),
    )
    .expect_err("the same export is not imported twice");
    assert!(matches!(refused, BackupError::Repository(_)));
    assert_eq!(
        destination
            .import_receipts()
            .expect("the receipts are readable")
            .len(),
        1
    );
}

#[test]
fn a_realm_never_imports_its_own_export_and_never_imports_into_no_project() {
    let seeded = seed();
    let export = export_realm(&seeded.store, at("2026-08-10T10:00:00Z")).expect("the export");

    let refused = import_export(
        &seeded.store,
        &export,
        &ImportPlan::redacted_import_into(seeded.project),
        at("2026-08-11T10:00:00Z"),
    )
    .expect_err("a realm restores its own export, it does not import it");
    assert!(matches!(refused, BackupError::SameRealmImport { .. }));

    let home = TempDir::new().expect("a temporary directory");
    let destination =
        SqliteStore::open(&home.path().join("kontor.db")).expect("the destination migrates");
    let refused = import_export(
        &destination,
        &export,
        &ImportPlan::redacted_import_into(ProjectId::generate()),
        at("2026-08-11T10:00:00Z"),
    )
    .expect_err("an import needs a destination project that exists");
    assert!(matches!(refused, BackupError::Repository(_)));
    assert_eq!(
        destination
            .import_receipts()
            .expect("the receipts are readable")
            .len(),
        0,
        "a refused import writes no receipt"
    );
}

#[test]
fn a_round_trip_preserves_the_versioned_specifications_and_the_source_evidence() {
    let source = seed();
    let first = export_realm(&source.store, at("2026-08-10T10:00:00Z")).expect("the first export");

    let home = TempDir::new().expect("a temporary directory");
    let destination =
        SqliteStore::open(&home.path().join("kontor.db")).expect("the destination migrates");
    let into = ProjectId::generate();
    destination
        .create_project(&NewProject {
            id: into,
            name: name("Destination project"),
            root_path: name("/tmp/kontor-destination"),
            created_at: at("2026-08-11T09:00:00Z"),
        })
        .expect("the destination project exists");
    let report = import_export(
        &destination,
        &first,
        &ImportPlan::redacted_import_into(into),
        at("2026-08-11T10:00:00Z"),
    )
    .expect("the export is imported");

    let second = export_realm(&destination, at("2026-08-11T11:00:00Z")).expect("the second export");

    // Every specification the source pinned survives the round trip with the
    // digest it had there.
    let source_profiles: Vec<&String> = first
        .records
        .work_profiles
        .iter()
        .map(|row| &row.definition_hash)
        .collect();
    assert!(!source_profiles.is_empty());
    for hash in &source_profiles {
        assert!(
            second
                .records
                .work_profiles
                .iter()
                .any(|row| &&row.definition_hash == hash),
            "a versioned specification lost its identity in the round trip"
        );
    }
    let source_teams: Vec<&String> = first
        .records
        .team_templates
        .iter()
        .map(|row| &row.definition_hash)
        .collect();
    for hash in &source_teams {
        assert!(
            second
                .records
                .team_templates
                .iter()
                .any(|row| &&row.definition_hash == hash)
        );
    }

    // The evidence that is deliberately not executable in the destination is
    // still *there*, as lineage under the destination's own receipt, with the
    // source realm and the source digests intact.
    let receipt = &destination.import_receipts().expect("readable")[0];
    assert_eq!(receipt.source_realm_id, source.store.realm_id().to_string());
    let lineage = destination.imported_records(&receipt.id).expect("readable");
    assert_eq!(lineage.len() as u64, report.record_count);
    for row in &first.records.runtime_events {
        assert!(
            lineage
                .iter()
                .any(|record| record.record_kind == "runtime_events"
                    && record.source_identity == row.cursor.to_string()),
            "a control observation must keep its source identity"
        );
    }
    // The destination's own realm identity is untouched by the import.
    assert_eq!(second.source_realm_id, destination.realm_id());
    assert_ne!(second.source_realm_id, first.source_realm_id);
}
