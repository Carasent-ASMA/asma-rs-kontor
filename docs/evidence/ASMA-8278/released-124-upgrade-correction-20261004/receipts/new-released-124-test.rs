    /// The released master generations `0123`/`0124` are exactly the tail of
    /// `MIGRATIONS[..124]`, so applying that slice builds the real release, not
    /// a downgraded copy of the current schema.
    #[test]
    fn released_124_upgrade_preserves_realm_bindings_and_memory_content() {
        use kontor_core::id::ProjectId;

        const PROJECT: &str = "01930000-0000-7000-8000-000000000124";
        const REVISION: &str = "01930000-0000-7000-8000-000000000125";
        const RUN: &str = "01930000-0000-7000-8000-000000000126";
        const DOCUMENT: &str = r#"{"schema_version":1,"text":"released 124 memory"}"#;
        const RECALL_KEY: &str = "released-124-recall";
        const REBUILD_KEY: &str = "released-124-rebuild";
        const SEEDED_TABLES: [&str; 14] = [
            "memory_items",
            "memory_revisions",
            "memory_approvals",
            "memory_receipts",
            "memory_context_bindings",
            "memory_fts",
            "memory_experience_eligibility",
            "memory_experience_proposals",
            "memory_projection_snapshots",
            "memory_projection_active",
            "memory_recall_keys",
            "memory_recall_metadata",
            "memory_projection_rebuild_keys",
            "memory_projection_rebuild_results",
        ];

        let directory = tempfile::tempdir().expect("fixture directory");
        let path = directory.path().join("kontor.db");
        let connection = Connection::open(&path).expect("released fixture");
        for migration in &MIGRATIONS[..124] {
            connection
                .execute_batch(migration)
                .expect("released migration");
        }
        assert_eq!(
            read_user_version(&connection).expect("released version"),
            124,
            "the fixture must stop at the released master generation"
        );

        let realm = RealmMetadata::create(RealmId::generate(), Timestamp::now());
        connection
            .execute(
                "INSERT INTO realm_metadata (singleton, realm_id, schema_version, created_at, display_label)
                 VALUES (1, ?1, ?2, ?3, NULL)",
                rusqlite::params![
                    realm.realm_id.to_string(),
                    i64::from(realm.schema_version.get()),
                    realm.created_at.to_string()
                ],
            )
            .expect("released realm identity");
        connection
            .execute(
                "INSERT INTO realm_idempotency_bindings (idempotency_key, operation, fingerprint, bound_at)
                 VALUES ('released-124-profile-pack', 'register_profile_pack', ?1, '2026-10-01T00:00:00Z')",
                [ContentHash::of(b"released-124-binding").as_str()],
            )
            .expect("released binding");

        let document_hash = ContentHash::of(DOCUMENT.as_bytes());
        let provenance = r#"{"source":"synthetic-124","source_id":null,"legacy_last_write_wins":false,"history_unavailable":false}"#;
        connection
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, 'Released 124 fixture', '/tmp/released-124-fixture', 1, '2026-10-01T00:00:00Z')",
                [PROJECT],
            )
            .expect("released project");
        connection
            .execute(
                "INSERT INTO memory_items (project_id, id, aggregate_revision, current_revision_id)
                 VALUES (?1, 'release-note', 1, ?2)",
                rusqlite::params![PROJECT, REVISION],
            )
            .expect("released memory item");
        connection
            .execute(
                "INSERT INTO memory_revisions
                     (project_id, item_id, id, revision, document, content_hash, provenance,
                      proposed_by, proposed_at, supersedes_id, history_unavailable)
                 VALUES (?1, 'release-note', ?2, 1, ?3, ?4, ?5, 'fixture-author',
                         '2026-10-01T00:00:00Z', NULL, 0)",
                rusqlite::params![
                    PROJECT,
                    REVISION,
                    DOCUMENT,
                    document_hash.as_str(),
                    provenance
                ],
            )
            .expect("released memory revision");
        connection
            .execute(
                "INSERT INTO memory_approvals (project_id, revision_id, approved_by, approved_at)
                 VALUES (?1, ?2, 'fixture-reviewer', '2026-10-01T00:01:00Z')",
                rusqlite::params![PROJECT, REVISION],
            )
            .expect("released memory approval");
        connection
            .execute(
                "INSERT INTO memory_receipts
                     (id, project_id, operation, item_id, revision_id, aggregate_revision,
                      result_hash, recorded_at)
                 VALUES ('released-124-receipt', ?1, 'propose_memory_revision', 'release-note',
                         ?2, 1, ?3, '2026-10-01T00:01:00Z')",
                rusqlite::params![
                    PROJECT,
                    REVISION,
                    ContentHash::of(b"released-124-receipt").as_str()
                ],
            )
            .expect("released memory receipt");
        connection
            .execute(
                "INSERT INTO memory_fts (project_id, item_id, revision_id, document)
                 VALUES (?1, 'release-note', ?2, ?3)",
                rusqlite::params![PROJECT, REVISION, DOCUMENT],
            )
            .expect("released memory fts row");
        let ordered = format!(
            "[{{\"revision_id\":\"{REVISION}\",\"content_hash\":\"{}\"}}]",
            document_hash.as_str()
        );
        connection
            .execute(
                "INSERT INTO memory_context_bindings
                     (project_id, run_id, selection_cursor, selection_spec, ordered_revisions,
                      result_hash, bound_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, '2026-10-01T00:02:00Z')",
                rusqlite::params![
                    PROJECT,
                    RUN,
                    DOCUMENT,
                    ordered,
                    ContentHash::of(b"released-124-context").as_str()
                ],
            )
            .expect("released context binding");
        connection
            .execute(
                "INSERT INTO memory_recall_keys (project_id, idempotency_key, run_id, task_id)
                 VALUES (?1, ?2, ?3, 'released-124-task')",
                rusqlite::params![PROJECT, RECALL_KEY, RUN],
            )
            .expect("released recall key");
        connection
            .execute(
                "INSERT INTO memory_recall_metadata (project_id, run_id, metadata)
                 VALUES (?1, ?2, '{\"selected\":true,\"purged\":false}')",
                rusqlite::params![PROJECT, RUN],
            )
            .expect("released recall metadata");
        connection
            .execute(
                "INSERT INTO memory_experience_eligibility
                     (project_id, revision_id, confidence, projection_policy)
                 VALUES (?1, ?2, 'observed', 'provider_eligible')",
                rusqlite::params![PROJECT, REVISION],
            )
            .expect("released experience eligibility");
        connection
            .execute(
                "INSERT INTO memory_experience_proposals
                     (project_id, idempotency_key, request_hash, revision_id, receipt)
                 VALUES (?1, 'released-124-proposal', ?2, ?3, '{\"approved\":true}')",
                rusqlite::params![
                    PROJECT,
                    ContentHash::of(b"released-124-proposal").as_str(),
                    REVISION
                ],
            )
            .expect("released experience proposal");
        let identities = format!(
            "[{{\"project_id\":\"{PROJECT}\",\"item_id\":\"release-note\",\"revision_id\":\"{REVISION}\",\"content_hash\":\"{}\"}}]",
            document_hash.as_str()
        );
        connection
            .execute(
                "INSERT INTO memory_projection_snapshots
                     (project_id, memory_cursor, dataset, digest, identities, created_at)
                 VALUES (?1, 1, 'kontor', ?2, ?3, '2026-10-01T00:03:00Z')",
                rusqlite::params![
                    PROJECT,
                    ContentHash::of(b"released-124-projection").as_str(),
                    identities
                ],
            )
            .expect("released projection snapshot");
        connection
            .execute(
                "INSERT INTO memory_projection_active (project_id, memory_cursor, generation)
                 VALUES (?1, 1, 1)",
                [PROJECT],
            )
            .expect("released active projection");
        connection
            .execute(
                "INSERT INTO memory_projection_rebuild_keys
                     (idempotency_key, project_id, request, request_hash, recorded_at)
                 VALUES (?1, ?2, '{\"expected_generation\":1}', ?3, '2026-10-01T00:04:00Z')",
                rusqlite::params![
                    REBUILD_KEY,
                    PROJECT,
                    ContentHash::of(b"released-124-rebuild-request").as_str()
                ],
            )
            .expect("released rebuild key");
        connection
            .execute(
                "INSERT INTO memory_projection_rebuild_results
                     (idempotency_key, result, result_hash, recorded_at)
                 VALUES (?1, '{\"generation\":2}', ?2, '2026-10-01T00:04:01Z')",
                rusqlite::params![
                    REBUILD_KEY,
                    ContentHash::of(b"released-124-rebuild-result").as_str()
                ],
            )
            .expect("released rebuild result");

        // The seed is real: every representative 123/124 row exists, and the
        // v28 rebuild's missing permanence guards are still missing at 124.
        assert_eq!(table_count(&connection, "memory_revisions"), 1);
        assert_eq!(table_count(&connection, "memory_approvals"), 1);
        assert_eq!(table_count(&connection, "memory_projection_snapshots"), 1);
        assert_eq!(table_count(&connection, "memory_recall_metadata"), 1);
        assert_eq!(
            table_count(&connection, "memory_projection_rebuild_results"),
            1
        );
        assert_eq!(table_count(&connection, "realm_idempotency_bindings"), 1);
        assert_eq!(
            binding_guard_count(&connection).expect("released guard readback"),
            0,
            "released 124 predates the permanence guards this upgrade restores"
        );

        let before: Vec<(&str, Vec<Vec<String>>)> = SEEDED_TABLES
            .iter()
            .map(|table| (*table, table_rows(&connection, table)))
            .collect();
        drop(connection);

        let store = crate::SqliteStore::open(&path).expect("released 124 upgrades");
        assert_eq!(store.schema_version().expect("upgraded version"), 130);
        assert_eq!(store.realm_metadata().realm_id, realm.realm_id);
        let binding = store
            .memory_binding(ProjectId::parse(PROJECT).expect("project id"), RUN)
            .expect("binding read")
            .expect("the seeded 124 binding is readable");
        assert_eq!(
            binding.result_hash.as_str(),
            ContentHash::of(b"released-124-context").as_str()
        );
        drop(store);

        let connection = Connection::open(&path).expect("upgraded readback");
        assert_eq!(
            read_user_version(&connection).expect("upgraded version"),
            130
        );
        let fingerprint: String = connection
            .query_row(
                "SELECT fingerprint FROM realm_idempotency_bindings
                  WHERE idempotency_key = 'released-124-profile-pack'",
                [],
                |row| row.get(0),
            )
            .expect("the released binding survives both rebuilds");
        assert_eq!(
            fingerprint,
            ContentHash::of(b"released-124-binding").as_str()
        );
        assert_eq!(
            binding_guard_count(&connection).expect("restored guard readback"),
            2,
            "the upgrade restores both permanence guards"
        );
        assert!(refused(
            &connection,
            "UPDATE realm_idempotency_bindings SET fingerprint = '00'
              WHERE idempotency_key = 'released-124-profile-pack'"
        ));
        assert!(refused(
            &connection,
            "DELETE FROM realm_idempotency_bindings WHERE idempotency_key = 'released-124-profile-pack'"
        ));
        for (table, expected) in &before {
            assert_eq!(
                &table_rows(&connection, table),
                expected,
                "the upgrade changed seeded {table} rows"
            );
        }
        for statement in [
            "UPDATE memory_projection_snapshots SET digest = '00'",
            "DELETE FROM memory_projection_snapshots",
            "UPDATE memory_recall_metadata SET metadata = '{}'",
            "DELETE FROM memory_recall_metadata",
            "UPDATE memory_recall_keys SET task_id = 'other'",
            "DELETE FROM memory_recall_keys",
            "UPDATE memory_experience_proposals SET request_hash = '00'",
            "DELETE FROM memory_experience_proposals",
            "UPDATE memory_projection_rebuild_keys SET request = '{}'",
            "DELETE FROM memory_projection_rebuild_keys",
            "UPDATE memory_projection_rebuild_results SET result = '{}'",
            "DELETE FROM memory_projection_rebuild_results",
            "UPDATE memory_revisions SET proposed_by = 'changed'",
        ] {
            assert!(
                refused(&connection, statement),
                "{statement} must stay refused after the upgrade"
            );
        }
        assert!(
            !connection
                .prepare("PRAGMA foreign_key_check")
                .expect("foreign key check")
                .exists([])
                .expect("foreign key result")
        );
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .expect("integrity check");
        assert_eq!(integrity, "ok");
    }

    /// Render one table's rows so a pre/post comparison covers every value.
    fn table_rows(connection: &Connection, table: &str) -> Vec<Vec<String>> {
        use rusqlite::types::ValueRef;
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .expect("fixture read");
        let width = statement.column_count();
        statement
            .query_map([], |row| {
                (0..width)
                    .map(|index| {
                        Ok(match row.get_ref(index)? {
                            ValueRef::Null => "null".to_owned(),
                            ValueRef::Integer(value) => format!("integer:{value}"),
                            ValueRef::Real(value) => format!("real:{value}"),
                            ValueRef::Text(value) => {
                                format!("text:{}", String::from_utf8_lossy(value))
                            }
                            ValueRef::Blob(value) => format!("blob:{value:?}"),
                        })
                    })
                    .collect()
            })
            .expect("fixture rows")
            .collect::<Result<_, _>>()
            .expect("fixture row values")
    }

    fn table_count(connection: &Connection, table: &str) -> i64 {
        connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("fixture count")
    }

    fn binding_guard_count(connection: &Connection) -> rusqlite::Result<i64> {
        connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger'
              AND name IN ('realm_idempotency_bindings_are_immutable',
                           'realm_idempotency_bindings_are_permanent')",
            [],
            |row| row.get(0),
        )
    }

    fn refused(connection: &Connection, sql: &str) -> bool {
        connection.execute(sql, []).is_err()
    }
}
