//! ASMA-8159 F4: real ordered migration rollback on a disposable v119 ledger.
use kontor_core::id::{CanonicalDocument, ContentHash, ExternalName, ProjectId, Timestamp};
use kontor_core::repository::{NewProject, ProjectRepository};
use kontor_store::memory::MemoryProvenance;
use kontor_store::{SCHEMA_VERSION, SqliteStore, StoreError};
use rusqlite::{Connection, types::ValueRef};

fn rows(connection: &Connection, sql: &str) -> Vec<Vec<String>> {
    let mut statement = connection.prepare(sql).unwrap();
    let width = statement.column_count();
    statement
        .query_map([], |row| {
            (0..width)
                .map(|i| {
                    Ok(match row.get_ref(i)? {
                        ValueRef::Null => "null".into(),
                        ValueRef::Integer(v) => format!("integer:{v}"),
                        ValueRef::Real(v) => format!("real:{v}"),
                        ValueRef::Text(v) => format!("text:{}", String::from_utf8_lossy(v)),
                        ValueRef::Blob(v) => format!("blob:{v:?}"),
                    })
                })
                .collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
fn ledger(connection: &Connection) -> Vec<Vec<Vec<String>>> {
    [
        "memory_items",
        "memory_revisions",
        "memory_approvals",
        "memory_receipts",
        "memory_context_bindings",
        "memory_fts",
    ]
    .iter()
    .map(|table| rows(connection, &format!("SELECT * FROM {table} ORDER BY rowid")))
    .collect()
}

#[test]
fn migration_0123_failure_preserves_v119_schema_history_and_clean_retry() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("kontor.db");
    let store = SqliteStore::open(&database).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: ExternalName::parse("Synthetic F4").unwrap(),
            root_path: ExternalName::parse("/tmp/synthetic-f4").unwrap(),
            created_at: Timestamp::now(),
        })
        .unwrap();
    let provenance = MemoryProvenance {
        source: "synthetic-f4".into(),
        source_id: None,
        legacy_last_write_wins: false,
        history_unavailable: false,
    };
    let typed = CanonicalDocument::from_value(
        &serde_json::from_str::<serde_json::Value>(include_str!(
            "../../kontor-core/tests/fixtures/experience-v1.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let generic = CanonicalDocument::from_value(
        &serde_json::json!({"schema_version":1,"text":"synthetic generic history"}),
    )
    .unwrap();
    let mut revisions = Vec::new();
    for (item, document) in [("typed", &typed), ("generic", &generic)] {
        let (revision, _) = store
            .propose_memory_revision(project, item, 0, document, &provenance, "synthetic-author")
            .unwrap();
        store
            .approve_memory_revision(
                project,
                item,
                &revision.revision_id,
                1,
                "synthetic-reviewer",
            )
            .unwrap();
        revisions.push(revision.revision_id);
    }
    let binding = store
        .freeze_memory_binding(project, "synthetic-before-upgrade", &generic, &revisions)
        .unwrap();
    drop(store);
    let connection = Connection::open(&database).unwrap();
    connection.execute_batch("DROP TABLE memory_projection_rebuild_results; DROP TABLE memory_projection_rebuild_keys; DROP TABLE memory_recall_keys; DROP TABLE memory_recall_metadata; DROP TABLE memory_experience_proposals; DROP TABLE memory_projection_active; DROP TABLE memory_projection_snapshots; DROP TABLE memory_experience_eligibility; DROP TABLE imported_record_evidence; DROP TABLE core_team_route_successions; PRAGMA user_version=119;").unwrap();
    // The ASMA-8278 feature generations are additive too: a fixture that replays
    // the chain from 119 must not leave their tables behind either.
    connection.execute_batch("DROP TABLE planning_pair_contributions; DROP TABLE planning_pair_record_revisions; DROP TABLE planning_pair_placements; DROP TABLE planning_pair_member_natives; DROP TABLE attestation_authority_keys; DROP TABLE attestation_authority_heads; DROP TABLE prepared_attestation_tokens; DROP TABLE attestation_token_heads; DROP TABLE desks;").unwrap();
    // A synthetic name collision fails inside unmodified 0123, after four
    // CREATE TABLE statements. Only this deliberate fixture trigger is removed.
    connection.execute_batch("CREATE TRIGGER memory_projection_snapshots_no_update BEFORE UPDATE ON memory_revisions BEGIN SELECT 1; END;").unwrap();
    let history = ledger(&connection);
    let hash = ContentHash::of(&serde_json::to_vec(&history).unwrap());
    let schema = rows(
        &connection,
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name",
    );
    drop(connection);
    let error = SqliteStore::open(&database).expect_err("forced 0123 collision must roll back");
    assert!(matches!(error, StoreError::Sqlite(_)), "{error:?}");
    let connection = Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        119
    );
    assert_eq!(
        rows(
            &connection,
            "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name"
        ),
        schema
    );
    assert_eq!(ledger(&connection), history);
    connection
        .execute_batch("DROP TRIGGER memory_projection_snapshots_no_update;")
        .unwrap();
    drop(connection);
    let upgraded = SqliteStore::open(&database).unwrap();
    assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(
        upgraded
            .memory_binding(project, "synthetic-before-upgrade")
            .unwrap()
            .unwrap()
            .result_hash,
        binding.result_hash
    );
    assert_eq!(
        upgraded
            .classify_experiences(project)
            .unwrap()
            .iter()
            .filter(|r| r.recall_eligible)
            .count(),
        1
    );
    assert_eq!(upgraded.rebuild_memory_fts().unwrap(), 2);
    assert_eq!(
        upgraded
            .search_memory(project, "synthetic", 10)
            .unwrap()
            .len(),
        2
    );
    drop(upgraded);
    let connection = Connection::open(&database).unwrap();
    assert_eq!(ledger(&connection), history);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
    assert!(
        connection
            .execute("UPDATE memory_revisions SET proposed_by='changed'", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM memory_approvals", [])
            .is_err()
    );
    let cache: i64 = connection
        .query_row(
            "SELECT count(*) FROM memory_experience_eligibility",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cache, 1,
        "clean retry must backfill canonical typed eligibility"
    );
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(connection);
    assert!(matches!(
        SqliteStore::open(&database),
        Err(StoreError::DatabaseTooNew { .. })
    ));
    let connection = Connection::open(&database).unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .unwrap();
    drop(connection);
    assert_eq!(
        SqliteStore::open(&database)
            .unwrap()
            .schema_version()
            .unwrap(),
        SCHEMA_VERSION
    );
    println!(
        "F4_RESULT {}",
        serde_json::json!({"prior_version":119,"final_version":SCHEMA_VERSION,"injected_failure":"0123 duplicate trigger after four table creations","history_sha256":hash,"rollback_schema_equal":true,"rollback_ledger_equal":true,"clean_retry_equal":true,"typed_backfill_rows":cache,"forward_version_refused":true})
    );
}
