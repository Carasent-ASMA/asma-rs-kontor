//! ASMA-8159: explicit synthetic cohort, coherent census and same-Realm copy.
#[allow(dead_code)]
mod harness;
use harness::World;
use kontor_core::id::{CanonicalDocument, ContentHash, IdempotencyKey, Timestamp};
use kontor_core::memory::{DegradedReason, RecallIntent};
use kontor_daemon::{DATABASE_FILE, recovery};
use kontor_store::SqliteStore;
use kontor_store::memory::{MemoryError, MemoryProvenance, SemanticRecall};
use rusqlite::{Connection, types::ValueRef};
use serde_json::{Value, json};

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
fn history(connection: &Connection) -> Value {
    let mut value = json!({});
    for table in [
        "memory_revisions",
        "memory_approvals",
        "memory_tombstones",
        "memory_receipts",
        "memory_context_bindings",
        "memory_recall_keys",
        "memory_recall_metadata",
    ] {
        value[table] = json!(rows(
            connection,
            &format!("SELECT * FROM {table} ORDER BY 1,2,3")
        ));
    }
    value
}
fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn document(index: usize) -> CanonicalDocument {
    if (40..45).contains(&index) || index == 56 {
        CanonicalDocument::from_value(
            &json!({"schema_version":1,"text":format!("synthetic generic {index}")}),
        )
        .unwrap()
    } else {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../kontor-core/tests/fixtures/experience-v1.json"
        ))
        .unwrap();
        value["projection_policy"] = json!(if (30..40).contains(&index) {
            "local_only"
        } else {
            "provider_eligible"
        });
        value["lesson"] = json!(format!(
            "Synthetic cohort {index}: verify publication readback before reporting completion."
        ));
        CanonicalDocument::from_value(&value).unwrap()
    }
}
fn intent() -> RecallIntent {
    RecallIntent {
        schema_version: 1,
        task_id: "synthetic-rehearsal".into(),
        task_title: "publication readback".into(),
        module: None,
        declared_scope: vec![],
        phase: "verification".into(),
    }
}

#[tokio::test]
async fn synthetic_55_cohort_current_57_snapshot_restore_conflict_history_and_replay() {
    let world = World::open().await;
    let origin = world.directory.path().to_owned();
    let backup = tempfile::tempdir().unwrap();
    let copied = tempfile::tempdir().unwrap();
    let backup_path = backup.path().to_owned();
    let copy_path = copied.path().to_owned();
    let project = world.project;
    let provenance = MemoryProvenance {
        source: "ASMA-8159-explicitly-synthetic".into(),
        source_id: Some("artificial-55-cohort-plus-2".into()),
        legacy_last_write_wins: false,
        history_unavailable: false,
    };
    let mut cohort = Vec::new();
    for index in 0..57 {
        world.daemon.state().with_store(|store|{
            let item=format!("synthetic-55-{index:03}");
            let document=document(index);
            let (revision,_)=store.propose_memory_revision(project,&item,0,&document,&provenance,"synthetic-author").unwrap();
            if !(45..50).contains(&index) {
                store.approve_memory_revision(project,&item,&revision.revision_id,1,"synthetic-reviewer").unwrap();
            }
            if (50..55).contains(&index) {
                store.tombstone_memory(project,&item,2,"synthetic-reviewer","synthetic obsolete fixture").unwrap();
            }
            if index<55 {cohort.push(json!({"project_id":project,"item_id":item,"revision_id":revision.revision_id,"content_hash":document.hash(),"provenance":provenance.source}));}
        });
    }
    assert_eq!(cohort.len(), 55);
    let key = IdempotencyKey::parse("synthetic-frozen-rehearsal").unwrap();
    let frozen = world
        .daemon
        .state()
        .with_store(|store| {
            store.recall_experiences_idempotent(
                project,
                "synthetic-frozen-run",
                "synthetic-rehearsal",
                &key,
                &intent(),
                &SemanticRecall::Degraded(DegradedReason::Absent),
            )
        })
        .unwrap();
    let original = Connection::open(origin.join(DATABASE_FILE)).unwrap();
    let tx = original.unchecked_transaction().unwrap();
    let before = history(&tx);
    let before_items = rows(&tx, "SELECT * FROM memory_items ORDER BY project_id,id");
    let census = rows(
        &tx,
        include_str!("../../../docs/evidence/ASMA-8159/production-prep/census.sql"),
    );
    assert_eq!(census.len(), 57);
    let before_hash = ContentHash::of(&serde_json::to_vec(&before).unwrap());
    tx.commit().unwrap();
    let classified = world
        .daemon
        .state()
        .with_store(|s| s.classify_experiences(project))
        .unwrap();
    assert_eq!(classified.len(), 47);
    assert_eq!(classified.iter().filter(|v| v.recall_eligible).count(), 41);
    let staged = world
        .daemon
        .state()
        .with_store(|s| s.projection_preview(project))
        .unwrap();
    assert_eq!(staged.entries.len(), 31);
    let (snapshot, pruned) =
        recovery::snapshot(&origin, Some(&backup_path), Timestamp::now()).unwrap();
    assert!(pruned.is_empty());
    snapshot.manifest.verify_file(&snapshot.snapshot).unwrap();
    let restored = recovery::restore(&copy_path, &snapshot.snapshot, Timestamp::now()).unwrap();
    assert!(restored.reconciliation_required);
    assert_eq!(restored.realm_id, snapshot.manifest.realm_id);
    // Only snapshot database+manifest are copied. No tokens/config/provider homes.
    assert!(!copy_path.join("provider-homes").exists());
    assert!(!copy_path.join("memory-cognee.json").exists());
    let store = SqliteStore::open(&copy_path.join(DATABASE_FILE)).unwrap();
    let copy = Connection::open(copy_path.join(DATABASE_FILE)).unwrap();
    assert_eq!(history(&copy), before);
    assert_eq!(
        rows(&copy, "SELECT * FROM memory_items ORDER BY project_id,id"),
        before_items
    );
    let receipts_before = count(&copy, "memory_receipts");
    let aggregate_before = store.memory_readback_hash(project).unwrap();
    let pointer_before: Option<String> = copy
        .query_row(
            "SELECT current_revision_id FROM memory_items WHERE id='synthetic-55-001'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    store
        .tombstone_memory(
            project,
            "synthetic-55-000",
            2,
            "synthetic-reviewer",
            "isolated rehearsal tombstone",
        )
        .unwrap();
    assert!(matches!(
        store.tombstone_memory(
            project,
            "synthetic-55-000",
            2,
            "synthetic-reviewer",
            "stale rehearsal"
        ),
        Err(MemoryError::RevisionConflict {
            expected: 2,
            current: 3
        })
    ));
    let (next, _) = store
        .propose_memory_revision(
            project,
            "synthetic-55-001",
            2,
            &document(101),
            &provenance,
            "synthetic-author",
        )
        .unwrap();
    store
        .approve_memory_revision(
            project,
            "synthetic-55-001",
            &next.revision_id,
            3,
            "synthetic-reviewer",
        )
        .unwrap();
    let pointer_after: Option<String> = copy
        .query_row(
            "SELECT current_revision_id FROM memory_items WHERE id='synthetic-55-001'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(pointer_before, pointer_after);
    assert_eq!(pointer_after, Some(next.revision_id));
    assert_eq!(count(&copy, "memory_receipts"), receipts_before + 3);
    let after = history(&copy);
    for (table, prior) in before.as_object().unwrap() {
        let current = after[table].as_array().unwrap();
        assert!(
            prior
                .as_array()
                .unwrap()
                .iter()
                .all(|row| current.contains(row)),
            "immutable prior rows changed in {table}"
        );
    }
    assert_ne!(
        aggregate_before,
        store.memory_readback_hash(project).unwrap()
    );
    assert_eq!(count(&copy, "memory_purges"), 0);
    assert_eq!(
        copy.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    drop(store);
    let store = SqliteStore::open(&copy_path.join(DATABASE_FILE)).unwrap();
    let replay = store
        .recall_experiences_idempotent(
            project,
            "synthetic-frozen-run",
            "synthetic-rehearsal",
            &key,
            &intent(),
            &SemanticRecall::Degraded(DegradedReason::Timeout),
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.canonical_block, frozen.canonical_block);
    assert_eq!(replay.binding.result_hash, frozen.binding.result_hash);
    assert_eq!(
        history(&original),
        before,
        "source history must be unchanged by copy rehearsal"
    );
    assert_eq!(
        rows(
            &original,
            "SELECT * FROM memory_items ORDER BY project_id,id"
        ),
        before_items
    );
    let result = json!({"boundary":"synthetic same-Realm snapshot, no live/copy-live corpus","origin":origin,"backup_directory":backup_path,"restore_root":copy_path,"manifest":snapshot.manifest,"synthetic_cohort_count":55,"synthetic_current_n":57,"additional_current_items":2,"current_approved_not_tombstoned":47,"typed_recall_eligible":41,"projection_provider_eligible":31,"local_only":10,"generic_current":6,"pending":5,"tombstoned":5,"cohort":cohort,"census":census,"prior_history_sha256":before_hash,"source_rows_unchanged":true,"prior_immutable_rows_preserved":true,"appended_memory_receipts":3,"optimistic_conflict":true,"approved_head_pointer_changed":true,"replay_after_restart_equal":true,"purges":0,"retention":"test lifetime; all temporary roots deleted; only synthetic hash/identity evidence retained"});
    drop(copy);
    drop(original);
    drop(store);
    drop(world);
    drop(backup);
    drop(copied);
    assert!(!origin.exists() && !backup_path.exists() && !copy_path.exists());
    println!(
        "SYNTHETIC_REHEARSAL_RESULT {}",
        json!({"proof":result,"teardown_roots_absent":true})
    );
}
