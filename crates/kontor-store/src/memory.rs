//! Native, project-isolated memory ledger and its rebuildable FTS projection.
#![allow(missing_docs)]

use kontor_core::id::{CanonicalDocument, ContentHash, ProjectId, Timestamp, parse_utc_timestamp};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SqliteStore;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("memory authority is `{current}`; `{required}` is required")]
    Authority {
        current: String,
        required: &'static str,
    },
    #[error("memory record was not found")]
    NotFound,
    #[error("memory rule refused the operation: {0}")]
    Rule(&'static str),
    #[error(transparent)]
    Domain(#[from] kontor_core::DomainError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored memory JSON is invalid")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryProvenance {
    pub source: String,
    pub source_id: Option<String>,
    pub legacy_last_write_wins: bool,
    pub history_unavailable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRevision {
    pub project_id: ProjectId,
    pub item_id: String,
    pub revision_id: String,
    pub revision: u64,
    pub document: CanonicalDocument,
    pub provenance: MemoryProvenance,
    pub proposed_by: String,
    pub proposed_at: Timestamp,
    pub supersedes_id: Option<String>,
    pub approved: bool,
    pub current: bool,
    pub tombstoned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryReceipt {
    pub receipt_id: String,
    pub project_id: ProjectId,
    pub operation: String,
    pub item_id: Option<String>,
    pub revision_id: Option<String>,
    pub aggregate_revision: Option<u64>,
    pub result_hash: ContentHash,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenRevision {
    pub revision_id: String,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMemoryBinding {
    pub project_id: ProjectId,
    pub run_id: String,
    pub selection_cursor: i64,
    pub selection_spec: CanonicalDocument,
    pub ordered_revisions: Vec<FrozenRevision>,
    pub result_hash: ContentHash,
    pub bound_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyMemoryEntry {
    pub item_id: String,
    pub document: CanonicalDocument,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsRoomExport {
    pub schema_version: u32,
    pub source: String,
    pub project_id: ProjectId,
    pub entries: Vec<LegacyMemoryEntry>,
    pub export_hash: ContentHash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub source: String,
    pub export_hash: ContentHash,
    pub entries: usize,
    pub already_imported: bool,
    pub history_unavailable: bool,
}

impl AgentsRoomExport {
    pub fn calculate_hash(&self) -> Result<ContentHash, MemoryError> {
        let value = serde_json::json!({
            "schema_version": self.schema_version,
            "source": self.source,
            "project_id": self.project_id,
            "entries": self.entries,
        });
        Ok(CanonicalDocument::from_value(&value)?.hash().clone())
    }
}

impl SqliteStore {
    pub fn propose_memory_revision(
        &self,
        project_id: ProjectId,
        item_id: &str,
        expected_revision: u64,
        document: &CanonicalDocument,
        provenance: &MemoryProvenance,
        proposed_by: &str,
    ) -> Result<(MemoryRevision, MemoryReceipt), MemoryError> {
        kontor_core::id::reject_sensitive_text("memory.item_id", item_id)?;
        kontor_core::id::reject_sensitive_text("memory.proposed_by", proposed_by)?;
        let tx = self.connection.unchecked_transaction()?;
        require_authority(&tx, "kontor")?;
        let current = aggregate_revision(&tx, project_id, item_id)?.unwrap_or(0);
        if current != expected_revision {
            return Err(MemoryError::RevisionConflict {
                expected: expected_revision,
                current,
            });
        }
        tx.execute(
            "INSERT INTO memory_items(project_id,id,aggregate_revision) VALUES (?1,?2,0) ON CONFLICT DO NOTHING",
            params![project_id.to_string(), item_id],
        )?;
        let revision = current + 1;
        let revision_id = Uuid::now_v7().to_string();
        let proposed_at = Timestamp::now();
        let supersedes_id: Option<String> = tx.query_row(
            "SELECT current_revision_id FROM memory_items WHERE project_id=?1 AND id=?2",
            params![project_id.to_string(), item_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO memory_revisions(project_id,item_id,id,revision,document,content_hash,provenance,proposed_by,proposed_at,supersedes_id,history_unavailable) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![project_id.to_string(), item_id, revision_id, sql_u64(revision)?, document.json(), document.hash().as_str(), serde_json::to_string(provenance)?, proposed_by, proposed_at.to_string(), supersedes_id, provenance.history_unavailable],
        )?;
        tx.execute(
            "UPDATE memory_items SET aggregate_revision=?3 WHERE project_id=?1 AND id=?2 AND aggregate_revision=?4",
            params![project_id.to_string(), item_id, sql_u64(revision)?, sql_u64(current)?],
        )?;
        let receipt = receipt(
            &tx,
            project_id,
            "propose",
            Some(item_id),
            Some(&revision_id),
            Some(revision),
            document.hash(),
        )?;
        tx.commit()?;
        Ok((
            MemoryRevision {
                project_id,
                item_id: item_id.into(),
                revision_id,
                revision,
                document: document.clone(),
                provenance: provenance.clone(),
                proposed_by: proposed_by.into(),
                proposed_at,
                supersedes_id,
                approved: false,
                current: false,
                tombstoned: false,
            },
            receipt,
        ))
    }

    pub fn approve_memory_revision(
        &self,
        project_id: ProjectId,
        item_id: &str,
        revision_id: &str,
        expected_revision: u64,
        approved_by: &str,
    ) -> Result<MemoryReceipt, MemoryError> {
        kontor_core::id::reject_sensitive_text("memory.approved_by", approved_by)?;
        let tx = self.connection.unchecked_transaction()?;
        require_authority(&tx, "kontor")?;
        let current = aggregate_revision(&tx, project_id, item_id)?.ok_or(MemoryError::NotFound)?;
        if current != expected_revision {
            return Err(MemoryError::RevisionConflict {
                expected: expected_revision,
                current,
            });
        }
        let hash: String = tx.query_row(
            "SELECT content_hash FROM memory_revisions WHERE project_id=?1 AND item_id=?2 AND id=?3",
            params![project_id.to_string(), item_id, revision_id], |row| row.get(0),
        ).optional()?.ok_or(MemoryError::NotFound)?;
        let approved_at = Timestamp::now();
        tx.execute("INSERT INTO memory_approvals(project_id,revision_id,approved_by,approved_at) VALUES (?1,?2,?3,?4)", params![project_id.to_string(), revision_id, approved_by, approved_at.to_string()])?;
        let changed = tx.execute(
            "UPDATE memory_items SET current_revision_id=?3, aggregate_revision=aggregate_revision+1 WHERE project_id=?1 AND id=?2 AND aggregate_revision=?4",
            params![project_id.to_string(), item_id, revision_id, sql_u64(current)?],
        )?;
        if changed != 1 {
            return Err(MemoryError::RevisionConflict {
                expected: expected_revision,
                current: aggregate_revision(&tx, project_id, item_id)?.unwrap_or(current),
            });
        }
        tx.execute(
            "DELETE FROM memory_fts WHERE project_id=?1 AND item_id=?2",
            params![project_id.to_string(), item_id],
        )?;
        tx.execute("INSERT INTO memory_fts(project_id,item_id,revision_id,document) SELECT project_id,item_id,id,document FROM memory_revisions WHERE project_id=?1 AND id=?2", params![project_id.to_string(), revision_id])?;
        let parsed = ContentHash::parse(&hash)?;
        let receipt = receipt(
            &tx,
            project_id,
            "approve",
            Some(item_id),
            Some(revision_id),
            Some(current + 1),
            &parsed,
        )?;
        tx.commit()?;
        Ok(receipt)
    }

    pub fn memory_history(
        &self,
        project_id: ProjectId,
        item_id: &str,
    ) -> Result<Vec<MemoryRevision>, MemoryError> {
        let mut statement = self.connection.prepare("SELECT r.id,r.revision,r.document,r.content_hash,r.provenance,r.proposed_by,r.proposed_at,r.supersedes_id,a.revision_id IS NOT NULL,i.current_revision_id=r.id,t.item_id IS NOT NULL FROM memory_revisions r JOIN memory_items i ON i.project_id=r.project_id AND i.id=r.item_id LEFT JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id LEFT JOIN memory_tombstones t ON t.project_id=r.project_id AND t.item_id=r.item_id WHERE r.project_id=?1 AND r.item_id=?2 ORDER BY r.revision")?;
        let rows = statement.query_map(params![project_id.to_string(), item_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, bool>(8)?,
                row.get::<_, bool>(9)?,
                row.get::<_, bool>(10)?,
            ))
        })?;
        rows.map(|row| {
            let (
                revision_id,
                revision,
                json,
                hash,
                provenance,
                proposed_by,
                at,
                supersedes_id,
                approved,
                current,
                tombstoned,
            ) = row?;
            let hash = ContentHash::parse(&hash)?;
            Ok(MemoryRevision {
                project_id,
                item_id: item_id.into(),
                revision_id,
                revision: u64::try_from(revision)
                    .map_err(|_| MemoryError::Rule("stored revision is negative"))?,
                document: CanonicalDocument::from_stored(&json, &hash)?,
                provenance: serde_json::from_str(&provenance)?,
                proposed_by,
                proposed_at: parse_utc_timestamp(&at)?,
                supersedes_id,
                approved,
                current,
                tombstoned,
            })
        })
        .collect()
    }

    pub fn search_memory(
        &self,
        project_id: ProjectId,
        query: &str,
        limit: u32,
    ) -> Result<Vec<MemoryRevision>, MemoryError> {
        let mut statement = self.connection.prepare("SELECT item_id FROM memory_fts WHERE project_id=?1 AND memory_fts MATCH ?2 ORDER BY rank LIMIT ?3")?;
        let ids = statement
            .query_map(
                params![project_id.to_string(), query, limit.min(100)],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                self.memory_history(project_id, &id)?
                    .into_iter()
                    .find(|r| r.current && r.approved && !r.tombstoned)
                    .ok_or(MemoryError::NotFound)
            })
            .collect()
    }

    pub fn list_memory(&self, project_id: ProjectId) -> Result<Vec<MemoryRevision>, MemoryError> {
        let mut statement = self.connection.prepare("SELECT id FROM memory_items WHERE project_id=?1 AND current_revision_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM memory_tombstones t WHERE t.project_id=memory_items.project_id AND t.item_id=memory_items.id) ORDER BY id")?;
        let ids = statement
            .query_map([project_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                self.memory_history(project_id, &id)?
                    .into_iter()
                    .find(|r| r.current && r.approved)
                    .ok_or(MemoryError::NotFound)
            })
            .collect()
    }

    pub fn rebuild_memory_fts(&self) -> Result<usize, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("DELETE FROM memory_fts", [])?;
        let count = tx.execute("INSERT INTO memory_fts(project_id,item_id,revision_id,document) SELECT r.project_id,r.item_id,r.id,r.document FROM memory_revisions r JOIN memory_items i ON i.project_id=r.project_id AND i.id=r.item_id AND i.current_revision_id=r.id JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id LEFT JOIN memory_tombstones t ON t.project_id=r.project_id AND t.item_id=r.item_id WHERE t.item_id IS NULL", [])?;
        tx.commit()?;
        Ok(count)
    }

    pub fn freeze_memory_binding(
        &self,
        project_id: ProjectId,
        run_id: &str,
        selection_spec: &CanonicalDocument,
        revision_ids: &[String],
    ) -> Result<ContextMemoryBinding, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        if let Some(existing) = read_binding(&tx, project_id, run_id)? {
            return Ok(existing);
        }
        let cursor: i64 = tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM memory_approvals",
            [],
            |row| row.get(0),
        )?;
        let mut ordered = Vec::with_capacity(revision_ids.len());
        for id in revision_ids {
            let hash: String = tx.query_row("SELECT r.content_hash FROM memory_revisions r JOIN memory_items i ON i.project_id=r.project_id AND i.current_revision_id=r.id JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id LEFT JOIN memory_tombstones t ON t.project_id=r.project_id AND t.item_id=r.item_id WHERE r.project_id=?1 AND r.id=?2 AND t.item_id IS NULL", params![project_id.to_string(), id], |row| row.get(0)).optional()?.ok_or(MemoryError::NotFound)?;
            ordered.push(FrozenRevision {
                revision_id: id.clone(),
                content_hash: ContentHash::parse(&hash)?,
            });
        }
        let ordered_json = serde_json::to_string(&ordered)?;
        let result_hash = ContentHash::of(
            serde_json::to_string(&(cursor, selection_spec.hash(), &ordered))?.as_bytes(),
        );
        let bound_at = Timestamp::now();
        tx.execute("INSERT INTO memory_context_bindings(project_id,run_id,selection_cursor,selection_spec,ordered_revisions,result_hash,bound_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![project_id.to_string(),run_id,cursor,selection_spec.json(),ordered_json,result_hash.as_str(),bound_at.to_string()])?;
        tx.commit()?;
        Ok(ContextMemoryBinding {
            project_id,
            run_id: run_id.into(),
            selection_cursor: cursor,
            selection_spec: selection_spec.clone(),
            ordered_revisions: ordered,
            result_hash,
            bound_at,
        })
    }

    pub fn memory_binding(
        &self,
        project_id: ProjectId,
        run_id: &str,
    ) -> Result<Option<ContextMemoryBinding>, MemoryError> {
        read_binding(&self.connection, project_id, run_id)
    }

    pub fn tombstone_memory(
        &self,
        project_id: ProjectId,
        item_id: &str,
        expected_revision: u64,
        by: &str,
        reason: &str,
    ) -> Result<MemoryReceipt, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        require_authority(&tx, "kontor")?;
        let current = aggregate_revision(&tx, project_id, item_id)?.ok_or(MemoryError::NotFound)?;
        if current != expected_revision {
            return Err(MemoryError::RevisionConflict {
                expected: expected_revision,
                current,
            });
        }
        let at = Timestamp::now();
        tx.execute("INSERT INTO memory_tombstones(project_id,item_id,aggregate_revision,reason,tombstoned_by,tombstoned_at) VALUES (?1,?2,?3,?4,?5,?6)",params![project_id.to_string(),item_id,sql_u64(current+1)?,reason,by,at.to_string()])?;
        tx.execute("UPDATE memory_items SET aggregate_revision=aggregate_revision+1 WHERE project_id=?1 AND id=?2",params![project_id.to_string(),item_id])?;
        tx.execute(
            "DELETE FROM memory_fts WHERE project_id=?1 AND item_id=?2",
            params![project_id.to_string(), item_id],
        )?;
        let hash =
            ContentHash::of(format!("{project_id}:{item_id}:tombstone:{current}").as_bytes());
        let out = receipt(
            &tx,
            project_id,
            "tombstone",
            Some(item_id),
            None,
            Some(current + 1),
            &hash,
        )?;
        tx.commit()?;
        Ok(out)
    }

    pub fn purge_memory(
        &self,
        project_id: ProjectId,
        item_id: &str,
        by: &str,
    ) -> Result<MemoryReceipt, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        require_authority(&tx, "kontor")?;
        let current = aggregate_revision(&tx, project_id, item_id)?.ok_or(MemoryError::NotFound)?;
        let hashes: Vec<String> = {
            let mut s=tx.prepare("SELECT content_hash FROM memory_revisions WHERE project_id=?1 AND item_id=?2 ORDER BY revision")?;
            s.query_map(params![project_id.to_string(), item_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        let manifest_hash = ContentHash::of(serde_json::to_string(&hashes)?.as_bytes());
        tx.execute("INSERT INTO memory_purges(project_id,item_id,manifest_hash,purged_by,purged_at) VALUES (?1,?2,?3,?4,?5)",params![project_id.to_string(),item_id,manifest_hash.as_str(),by,Timestamp::now().to_string()])?;
        tx.execute(
            "DELETE FROM memory_fts WHERE project_id=?1 AND item_id=?2",
            params![project_id.to_string(), item_id],
        )?;
        tx.execute("UPDATE memory_items SET current_revision_id=NULL,aggregate_revision=aggregate_revision+1 WHERE project_id=?1 AND id=?2",params![project_id.to_string(),item_id])?;
        tx.execute("DELETE FROM memory_approvals WHERE project_id=?1 AND revision_id IN (SELECT id FROM memory_revisions WHERE project_id=?1 AND item_id=?2)",params![project_id.to_string(),item_id])?;
        tx.execute(
            "DELETE FROM memory_revisions WHERE project_id=?1 AND item_id=?2",
            params![project_id.to_string(), item_id],
        )?;
        let out = receipt(
            &tx,
            project_id,
            "purge",
            Some(item_id),
            None,
            Some(current + 1),
            &manifest_hash,
        )?;
        tx.commit()?;
        Ok(out)
    }

    pub fn freeze_agentsroom_writes(&self) -> Result<(), MemoryError> {
        self.connection.execute("UPDATE memory_authority SET agentsroom_writes_frozen_at=COALESCE(agentsroom_writes_frozen_at,?1) WHERE singleton=1 AND authority='agentsroom'",[Timestamp::now().to_string()])?;
        Ok(())
    }
    pub fn preview_agentsroom_import(
        &self,
        export: &AgentsRoomExport,
    ) -> Result<ImportPreview, MemoryError> {
        verify_export(export)?;
        let imported=self.connection.query_row("SELECT 1 FROM memory_import_manifests WHERE project_id=?1 AND source=?2 AND export_hash=?3",params![export.project_id.to_string(),export.source,export.export_hash.as_str()],|r|r.get::<_,i64>(0)).optional()?.is_some();
        Ok(ImportPreview {
            source: export.source.clone(),
            export_hash: export.export_hash.clone(),
            entries: export.entries.len(),
            already_imported: imported,
            history_unavailable: true,
        })
    }
    pub fn apply_agentsroom_import(
        &self,
        export: &AgentsRoomExport,
    ) -> Result<ImportPreview, MemoryError> {
        let preview = self.preview_agentsroom_import(export)?;
        if preview.already_imported {
            return Ok(preview);
        }
        let tx = self.connection.unchecked_transaction()?;
        let frozen:Option<String>=tx.query_row("SELECT agentsroom_writes_frozen_at FROM memory_authority WHERE singleton=1 AND authority='agentsroom'",[],|r|r.get(0)).optional()?.flatten();
        if frozen.is_none() {
            return Err(MemoryError::Rule(
                "AgentsRoom writes must be frozen before import",
            ));
        }
        for entry in &export.entries {
            tx.execute(
                "INSERT INTO memory_items(project_id,id,aggregate_revision) VALUES (?1,?2,2)",
                params![export.project_id.to_string(), entry.item_id],
            )?;
            let id = Uuid::now_v7().to_string();
            let provenance = MemoryProvenance {
                source: "agentsroom".into(),
                source_id: entry.source_id.clone(),
                legacy_last_write_wins: true,
                history_unavailable: true,
            };
            tx.execute("INSERT INTO memory_revisions(project_id,item_id,id,revision,document,content_hash,provenance,proposed_by,proposed_at,history_unavailable) VALUES (?1,?2,?3,1,?4,?5,?6,'agentsroom-cutover',?7,1)",params![export.project_id.to_string(),entry.item_id,id,entry.document.json(),entry.document.hash().as_str(),serde_json::to_string(&provenance)?,Timestamp::now().to_string()])?;
            tx.execute("INSERT INTO memory_approvals(project_id,revision_id,approved_by,approved_at) VALUES (?1,?2,'agentsroom-cutover',?3)",params![export.project_id.to_string(),id,Timestamp::now().to_string()])?;
            tx.execute(
                "UPDATE memory_items SET current_revision_id=?3 WHERE project_id=?1 AND id=?2",
                params![export.project_id.to_string(), entry.item_id, id],
            )?;
        }
        let manifest = serde_json::to_string(export)?;
        tx.execute("INSERT INTO memory_import_manifests(project_id,source,export_hash,manifest,imported_count,imported_at) VALUES (?1,?2,?3,?4,?5,?6)",params![export.project_id.to_string(),export.source,export.export_hash.as_str(),manifest,i64::try_from(export.entries.len()).map_err(|_|MemoryError::Rule("too many import entries"))?,Timestamp::now().to_string()])?;
        tx.commit()?;
        Ok(preview)
    }
    pub fn switch_memory_authority(
        &self,
        project_id: ProjectId,
        source: &str,
        export_hash: &ContentHash,
    ) -> Result<(), MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        let count:i64=tx.query_row("SELECT imported_count FROM memory_import_manifests WHERE project_id=?1 AND source=?2 AND export_hash=?3",params![project_id.to_string(),source,export_hash.as_str()],|r|r.get(0)).optional()?.ok_or(MemoryError::Rule("the final hashed export has not been imported"))?;
        let actual: i64 = tx.query_row(
            "SELECT COUNT(*) FROM memory_items WHERE project_id=?1",
            [project_id.to_string()],
            |r| r.get(0),
        )?;
        if actual < count {
            return Err(MemoryError::Rule("import verification failed"));
        }
        let changed=tx.execute("UPDATE memory_authority SET authority='kontor',final_export_hash=?1,switched_at=?2 WHERE singleton=1 AND authority='agentsroom' AND agentsroom_writes_frozen_at IS NOT NULL",params![export_hash.as_str(),Timestamp::now().to_string()])?;
        if changed != 1 {
            return Err(MemoryError::Rule(
                "memory authority was already switched or writes were not frozen",
            ));
        }
        tx.commit()?;
        self.rebuild_memory_fts()?;
        Ok(())
    }
}

fn aggregate_revision(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    item_id: &str,
) -> Result<Option<u64>, rusqlite::Error> {
    tx.query_row(
        "SELECT aggregate_revision FROM memory_items WHERE project_id=?1 AND id=?2",
        params![project_id.to_string(), item_id],
        |r| {
            r.get::<_, i64>(0).and_then(|v| {
                u64::try_from(v).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Integer,
                        Box::new(e),
                    )
                })
            })
        },
    )
    .optional()
}
fn require_authority(tx: &Transaction<'_>, required: &'static str) -> Result<(), MemoryError> {
    let current: String = tx.query_row(
        "SELECT authority FROM memory_authority WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    if current == required {
        Ok(())
    } else {
        Err(MemoryError::Authority { current, required })
    }
}
fn receipt(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    operation: &str,
    item_id: Option<&str>,
    revision_id: Option<&str>,
    aggregate_revision: Option<u64>,
    hash: &ContentHash,
) -> Result<MemoryReceipt, MemoryError> {
    let receipt_id = Uuid::now_v7().to_string();
    let at = Timestamp::now();
    let sql_revision = aggregate_revision.map(sql_u64).transpose()?;
    tx.execute("INSERT INTO memory_receipts(id,project_id,operation,item_id,revision_id,aggregate_revision,result_hash,recorded_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![receipt_id,project_id.to_string(),operation,item_id,revision_id,sql_revision,hash.as_str(),at.to_string()])?;
    Ok(MemoryReceipt {
        receipt_id,
        project_id,
        operation: operation.into(),
        item_id: item_id.map(str::to_owned),
        revision_id: revision_id.map(str::to_owned),
        aggregate_revision,
        result_hash: hash.clone(),
        recorded_at: at,
    })
}
fn read_binding(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    run_id: &str,
) -> Result<Option<ContextMemoryBinding>, MemoryError> {
    let row=connection.query_row("SELECT selection_cursor,selection_spec,ordered_revisions,result_hash,bound_at FROM memory_context_bindings WHERE project_id=?1 AND run_id=?2",params![project_id.to_string(),run_id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).optional()?;
    row.map(|(cursor, spec, ordered, hash, at)| {
        let value: serde_json::Value = serde_json::from_str(&spec)?;
        Ok(ContextMemoryBinding {
            project_id,
            run_id: run_id.into(),
            selection_cursor: cursor,
            selection_spec: CanonicalDocument::from_value(&value)?,
            ordered_revisions: serde_json::from_str(&ordered)?,
            result_hash: ContentHash::parse(&hash)?,
            bound_at: parse_utc_timestamp(&at)?,
        })
    })
    .transpose()
}
fn verify_export(export: &AgentsRoomExport) -> Result<(), MemoryError> {
    if export.schema_version != 1 {
        return Err(MemoryError::Rule("unsupported AgentsRoom export schema"));
    }
    if export.calculate_hash()? != export.export_hash {
        return Err(MemoryError::Rule("AgentsRoom export hash mismatch"));
    }
    Ok(())
}
fn sql_u64(value: u64) -> Result<i64, MemoryError> {
    i64::try_from(value).map_err(|_| MemoryError::Rule("revision exceeds SQLite integer range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(text: &str) -> CanonicalDocument {
        CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"text":text})).unwrap()
    }
    fn fixture() -> (tempfile::TempDir, SqliteStore, ProjectId, ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("realm.db")).unwrap();
        let a = ProjectId::generate();
        let b = ProjectId::generate();
        for (id, name) in [(a, "a"), (b, "b")] {
            store.connection.execute("INSERT INTO projects(id,name,root_path,revision,created_at) VALUES (?1,?2,?3,1,?4)",params![id.to_string(),name,format!("/{name}"),Timestamp::now().to_string()]).unwrap();
        }
        (dir, store, a, b)
    }
    fn native(store: &SqliteStore) {
        store.connection.execute("UPDATE memory_authority SET authority='kontor',agentsroom_writes_frozen_at=?1,final_export_hash=?2,switched_at=?1 WHERE singleton=1",params![Timestamp::now().to_string(),ContentHash::of(b"cutover").as_str()]).unwrap();
    }
    fn provenance() -> MemoryProvenance {
        MemoryProvenance {
            source: "operator".into(),
            source_id: None,
            legacy_last_write_wins: false,
            history_unavailable: false,
        }
    }

    #[test]
    fn ledger_conflicts_filters_rebuilds_and_freezes_context() {
        let (_dir, store, a, b) = fixture();
        native(&store);
        let (proposal, _) = store
            .propose_memory_revision(
                a,
                "policy",
                0,
                &document("approved searchable alpha"),
                &provenance(),
                "author",
            )
            .unwrap();
        assert!(
            store.list_memory(a).unwrap().is_empty(),
            "a proposal is not retrieval"
        );
        assert!(matches!(
            store.propose_memory_revision(
                a,
                "policy",
                0,
                &document("stale"),
                &provenance(),
                "author"
            ),
            Err(MemoryError::RevisionConflict { current: 1, .. })
        ));
        store
            .approve_memory_revision(a, "policy", &proposal.revision_id, 1, "reviewer")
            .unwrap();
        assert_eq!(store.list_memory(a).unwrap().len(), 1);
        assert!(
            store.list_memory(b).unwrap().is_empty(),
            "another project cannot retrieve it"
        );
        assert_eq!(store.search_memory(a, "alpha", 10).unwrap().len(), 1);
        store
            .connection
            .execute("DELETE FROM memory_fts", [])
            .unwrap();
        assert!(store.search_memory(a, "alpha", 10).unwrap().is_empty());
        assert_eq!(store.rebuild_memory_fts().unwrap(), 1);
        let spec = document("ordered selection");
        let first = store
            .freeze_memory_binding(
                a,
                "run-1",
                &spec,
                std::slice::from_ref(&proposal.revision_id),
            )
            .unwrap();
        let second = store
            .freeze_memory_binding(a, "run-1", &document("different"), &[])
            .unwrap();
        assert_eq!(
            first.result_hash, second.result_hash,
            "a started run returns its stored binding without re-querying"
        );
        store
            .tombstone_memory(a, "policy", 2, "reviewer", "obsolete")
            .unwrap();
        assert!(store.list_memory(a).unwrap().is_empty());
        assert!(store.search_memory(a, "alpha", 10).unwrap().is_empty());
    }

    #[test]
    fn cutover_is_frozen_hashed_transactional_and_idempotent() {
        let (_dir, store, project, _) = fixture();
        let mut export = AgentsRoomExport {
            schema_version: 1,
            source: "agentsroom".into(),
            project_id: project,
            entries: vec![LegacyMemoryEntry {
                item_id: "legacy".into(),
                document: document("legacy value"),
                source_id: Some("old-1".into()),
            }],
            export_hash: ContentHash::of(b"placeholder"),
        };
        export.export_hash = export.calculate_hash().unwrap();
        assert!(matches!(
            store.apply_agentsroom_import(&export),
            Err(MemoryError::Rule(_))
        ));
        store.freeze_agentsroom_writes().unwrap();
        assert!(
            !store
                .preview_agentsroom_import(&export)
                .unwrap()
                .already_imported
        );
        store.apply_agentsroom_import(&export).unwrap();
        assert!(
            store
                .apply_agentsroom_import(&export)
                .unwrap()
                .already_imported
        );
        store
            .switch_memory_authority(project, "agentsroom", &export.export_hash)
            .unwrap();
        let history = store.memory_history(project, "legacy").unwrap();
        assert!(history[0].provenance.history_unavailable);
        assert!(history[0].provenance.legacy_last_write_wins);
        assert!(
            CanonicalDocument::from_value(
                &serde_json::json!({"schema_version":1,"text":"token=abcdefghijklmnopqrstuvwxyz"})
            )
            .is_err(),
            "secret scanning happens before a ledger value can exist"
        );
    }
}
