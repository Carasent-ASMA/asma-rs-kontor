//! Native, project-isolated memory ledger and its rebuildable FTS projection.
#![allow(missing_docs)]

use kontor_core::authority::AuthoritySubject;
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, ContentHash, ContextPackId, ProjectId, TaskId, Timestamp,
    parse_utc_timestamp,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SqliteStore;
use crate::authority::{
    AuthorityError, SubjectAuthorityReceipt, SubjectImportRecord, record_subject_import_in,
    require_subject_authority, subject_authority_in,
};

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

/// Memory speaks the ledger's refusals in its own error type, so every existing
/// caller and every `/v1` mapping keeps working unchanged after authority moved
/// from the Realm to `(project, memory)`.
impl From<AuthorityError> for MemoryError {
    fn from(error: AuthorityError) -> Self {
        match error {
            AuthorityError::Denied { current, .. } => Self::Authority {
                current: current.to_string(),
                required: "kontor",
            },
            // A project with no declared origin is not writable. It is reported as
            // an authority refusal rather than a missing record because that is
            // what it means to the caller: nothing has granted Kontor this
            // project's memory.
            AuthorityError::NotFound => Self::Authority {
                current: "undeclared".to_owned(),
                required: "kontor",
            },
            AuthorityError::RevisionConflict { expected, current } => {
                Self::RevisionConflict { expected, current }
            }
            AuthorityError::Rule(reason) => Self::Rule(reason),
            AuthorityError::Domain(error) => Self::Domain(error),
            AuthorityError::Sqlite(error) => Self::Sqlite(error),
            AuthorityError::Json(error) => Self::Json(error),
            AuthorityError::Repository(_) => {
                Self::Rule("a backlog graph refusal reached the memory authority path")
            }
        }
    }
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
pub struct StoredContextPack {
    pub id: ContextPackId,
    pub project_id: ProjectId,
    pub task_id: TaskId,
    pub content: CanonicalDocument,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenRunContext {
    pub context_pack: StoredContextPack,
    pub memory_binding: ContextMemoryBinding,
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
    pub fn memory_cursor(&self) -> Result<i64, MemoryError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM memory_receipts",
            [],
            |row| row.get(0),
        )?)
    }

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
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        require_subject_authority(&tx, project_id, AuthoritySubject::Memory)?;
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
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        require_subject_authority(&tx, project_id, AuthoritySubject::Memory)?;
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
        let mut revisions = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(revision) = self
                .memory_history(project_id, &id)?
                .into_iter()
                .find(|r| r.current && r.approved && !r.tombstoned)
            {
                revisions.push(revision);
            }
        }
        Ok(revisions)
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
        let binding =
            freeze_memory_binding_in(&tx, project_id, run_id, selection_spec, revision_ids)?;
        tx.commit()?;
        Ok(binding)
    }

    pub fn memory_binding(
        &self,
        project_id: ProjectId,
        run_id: &str,
    ) -> Result<Option<ContextMemoryBinding>, MemoryError> {
        read_binding(&self.connection, project_id, run_id)
    }

    /// Freeze the canonical pack and the approved memory revisions behind it in
    /// one transaction. A retry returns the first freeze unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn freeze_run_context(
        &self,
        project_id: ProjectId,
        run_id: &str,
        context_pack_id: ContextPackId,
        task_id: TaskId,
        content: &CanonicalDocument,
        selection_spec: &CanonicalDocument,
        revision_ids: &[String],
    ) -> Result<FrozenRunContext, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        if let Some(existing) = read_frozen_run_context(&tx, project_id, run_id, context_pack_id)? {
            return Ok(existing);
        }

        let created_at = Timestamp::now();
        tx.execute(
            "INSERT INTO context_packs(id,project_id,task_id,content,content_hash,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                context_pack_id.to_string(),
                project_id.to_string(),
                task_id.to_string(),
                content.json(),
                content.hash().as_str(),
                created_at.to_string()
            ],
        )?;
        let memory_binding =
            freeze_memory_binding_in(&tx, project_id, run_id, selection_spec, revision_ids)?;
        let context_pack = StoredContextPack {
            id: context_pack_id,
            project_id,
            task_id,
            content: content.clone(),
            created_at,
        };
        tx.commit()?;
        Ok(FrozenRunContext {
            context_pack,
            memory_binding,
        })
    }

    /// Read a complete run freeze. A half-written pair is refused as corrupt
    /// evidence instead of being silently repaired from newer memory.
    pub fn frozen_run_context(
        &self,
        project_id: ProjectId,
        run_id: &str,
        context_pack_id: ContextPackId,
    ) -> Result<Option<FrozenRunContext>, MemoryError> {
        read_frozen_run_context(&self.connection, project_id, run_id, context_pack_id)
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
        require_subject_authority(&tx, project_id, AuthoritySubject::Memory)?;
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
        require_subject_authority(&tx, project_id, AuthoritySubject::Memory)?;
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

    /// The canonical digest of what this project's memory *actually holds*.
    ///
    /// Computed from stored rows — the current, approved, untombstoned revision of
    /// every item — and never from the bytes an import submitted. It is what the
    /// switch compares against the hash the import recorded, so an import that
    /// claimed more than it persisted cannot be switched afterwards.
    ///
    /// # Errors
    /// Propagates SQLite and parse failures.
    pub fn memory_readback_hash(&self, project_id: ProjectId) -> Result<ContentHash, MemoryError> {
        memory_readback_hash(&self.connection, project_id)
    }

    pub fn preview_agentsroom_import(
        &self,
        export: &AgentsRoomExport,
    ) -> Result<ImportPreview, MemoryError> {
        verify_export(export)?;
        // Both manifest tables are consulted. v21's table is no longer written,
        // but a database that imported under it must not be told the same export
        // is still pending and import it a second time.
        let imported = self
            .connection
            .query_row(
                "SELECT 1 FROM subject_import_manifests
                 WHERE project_id=?1 AND subject='memory' AND source=?2 AND import_hash=?3
                 UNION ALL
                 SELECT 1 FROM memory_import_manifests
                 WHERE project_id=?1 AND source=?2 AND export_hash=?3",
                params![
                    export.project_id.to_string(),
                    export.source,
                    export.export_hash.as_str()
                ],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        Ok(ImportPreview {
            source: export.source.clone(),
            export_hash: export.export_hash.clone(),
            entries: export.entries.len(),
            already_imported: imported,
            history_unavailable: true,
        })
    }

    /// Import one project's legacy memory export and record its manifest.
    ///
    /// The import no longer waits for a realm-wide freeze. Freezing is an operator
    /// attestation about *this project's* source and is recorded afterwards, by
    /// [`SqliteStore::attest_subject_source_frozen`]; the switch refuses without
    /// it. Requiring it here as well would mean a project could not be imported
    /// until its source was already frozen, which is the ordering that made the
    /// old global ceremony necessary.
    pub fn apply_agentsroom_import(
        &self,
        export: &AgentsRoomExport,
    ) -> Result<ImportPreview, MemoryError> {
        let preview = self.preview_agentsroom_import(export)?;
        if preview.already_imported {
            return Ok(preview);
        }
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        // Read inside the transaction that acts on the answer. A check taken from
        // another transaction is a check of what was true before this one began.
        let authority = subject_authority_in(&tx, export.project_id, AuthoritySubject::Memory)?;
        if !authority.origin.permits_cutover() {
            return Err(MemoryError::Rule(
                "this project's memory was created in Kontor and has nothing to import",
            ));
        }
        if authority.writable_by_kontor() {
            return Err(MemoryError::Rule(
                "this project's memory has already been switched to Kontor",
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
        // The manifest and its readback join the same transaction as the items.
        // `already_imported` is derived from the manifest, so a commit between the
        // two would let a retry re-run the item loop against rows that are already
        // there: it dies on their primary key and the subject can never be
        // switched. One transaction means a failure anywhere leaves nothing, and
        // the retry is a first attempt again.
        let readback = memory_readback_hash(&tx, export.project_id)?;
        record_subject_import_in(
            &tx,
            &SubjectImportRecord {
                project_id: export.project_id,
                subject: AuthoritySubject::Memory,
                source: &export.source,
                import_hash: &export.export_hash,
                canonical_manifest: &serde_json::to_string(export)?,
                imported_count: u64::try_from(export.entries.len())
                    .map_err(|_| MemoryError::Rule("too many import entries"))?,
                readback_hash: &readback,
            },
        )?;
        tx.commit()?;
        // Derived and rebuildable, so it is outside the transaction on purpose: a
        // failure here costs an index rebuild, not the import.
        self.rebuild_memory_fts()?;
        Ok(preview)
    }

    /// Move one project's memory authority to Kontor.
    ///
    /// The readback is recomputed here, from this project's stored rows, and the
    /// ledger refuses the switch unless it equals what the import recorded. Only
    /// `(project_id, memory)` changes: another project, and this project's
    /// backlog, are untouched.
    ///
    /// # Errors
    /// [`MemoryError::Rule`] when the project's memory is native, already
    /// switched, unattested, or has no manifest for the named export;
    /// [`MemoryError::RevisionConflict`] on a stale revision.
    pub fn switch_project_memory_authority(
        &self,
        project_id: ProjectId,
        source: &str,
        export_hash: &ContentHash,
        expected_revision: AggregateRevision,
    ) -> Result<SubjectAuthorityReceipt, MemoryError> {
        let readback = self.memory_readback_hash(project_id)?;
        let (_, receipt) = self.switch_subject_authority(
            project_id,
            AuthoritySubject::Memory,
            source,
            export_hash,
            &readback,
            expected_revision,
        )?;
        self.rebuild_memory_fts()?;
        Ok(receipt)
    }
}

/// The canonical digest of one project's stored memory, over any connection.
///
/// Taken over a `&Connection` rather than `&self` so an import can compute it
/// inside the transaction that wrote the rows. Computing it after a separate
/// commit would describe a state that is no longer only this import's work.
fn memory_readback_hash(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
) -> Result<ContentHash, MemoryError> {
    let mut statement = connection.prepare(
        "SELECT i.id, r.content_hash
         FROM memory_items i
         JOIN memory_revisions r
           ON r.project_id = i.project_id AND r.id = i.current_revision_id
         JOIN memory_approvals a
           ON a.project_id = r.project_id AND a.revision_id = r.id
         LEFT JOIN memory_tombstones t
           ON t.project_id = i.project_id AND t.item_id = i.id
         WHERE i.project_id = ?1 AND t.item_id IS NULL
         ORDER BY i.id",
    )?;
    let items = statement
        .query_map([project_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ContentHash::of(
        serde_json::to_string(&serde_json::json!({
            "project_id": project_id,
            "subject": "memory",
            "items": items,
        }))?
        .as_bytes(),
    ))
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

fn read_context_pack(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    context_pack_id: ContextPackId,
) -> Result<Option<StoredContextPack>, MemoryError> {
    let row = connection
        .query_row(
            "SELECT task_id,content,content_hash,created_at
             FROM context_packs WHERE project_id=?1 AND id=?2",
            params![project_id.to_string(), context_pack_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    row.map(|(task_id, content, hash, created_at)| {
        let hash = ContentHash::parse(&hash)?;
        Ok(StoredContextPack {
            id: context_pack_id,
            project_id,
            task_id: TaskId::parse(&task_id)?,
            content: CanonicalDocument::from_stored(&content, &hash)?,
            created_at: parse_utc_timestamp(&created_at)?,
        })
    })
    .transpose()
}

fn read_frozen_run_context(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    run_id: &str,
    context_pack_id: ContextPackId,
) -> Result<Option<FrozenRunContext>, MemoryError> {
    let pack = read_context_pack(connection, project_id, context_pack_id)?;
    let binding = read_binding(connection, project_id, run_id)?;
    match (pack, binding) {
        (None, None) => Ok(None),
        (Some(context_pack), Some(memory_binding)) => Ok(Some(FrozenRunContext {
            context_pack,
            memory_binding,
        })),
        _ => Err(MemoryError::Rule("run context freeze is incomplete")),
    }
}

fn freeze_memory_binding_in(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    run_id: &str,
    selection_spec: &CanonicalDocument,
    revision_ids: &[String],
) -> Result<ContextMemoryBinding, MemoryError> {
    if let Some(existing) = read_binding(tx, project_id, run_id)? {
        return Ok(existing);
    }
    let cursor: i64 = tx.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM memory_approvals",
        [],
        |row| row.get(0),
    )?;
    let mut ordered = Vec::with_capacity(revision_ids.len());
    for id in revision_ids {
        let hash: String = tx
            .query_row(
                "SELECT r.content_hash
                 FROM memory_revisions r
                 JOIN memory_items i
                   ON i.project_id=r.project_id AND i.current_revision_id=r.id
                 JOIN memory_approvals a
                   ON a.project_id=r.project_id AND a.revision_id=r.id
                 LEFT JOIN memory_tombstones t
                   ON t.project_id=r.project_id AND t.item_id=r.item_id
                 WHERE r.project_id=?1 AND r.id=?2 AND t.item_id IS NULL",
                params![project_id.to_string(), id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(MemoryError::NotFound)?;
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
    tx.execute(
        "INSERT INTO memory_context_bindings
         (project_id,run_id,selection_cursor,selection_spec,ordered_revisions,result_hash,bound_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            project_id.to_string(),
            run_id,
            cursor,
            selection_spec.json(),
            ordered_json,
            result_hash.as_str(),
            bound_at.to_string()
        ],
    )?;
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
    use kontor_core::authority::SubjectOrigin;

    use super::*;

    fn document(text: &str) -> CanonicalDocument {
        CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"text":text})).unwrap()
    }
    /// Two projects whose memory was created in Kontor: writable immediately,
    /// with no cutover to wait for.
    fn fixture() -> (tempfile::TempDir, SqliteStore, ProjectId, ProjectId) {
        project_fixture(SubjectOrigin::KontorNative)
    }

    /// Two projects whose memory is still AgentsRoom's until it is imported and
    /// switched.
    fn legacy_fixture() -> (tempfile::TempDir, SqliteStore, ProjectId, ProjectId) {
        project_fixture(SubjectOrigin::LegacyPending)
    }

    fn project_fixture(
        memory: SubjectOrigin,
    ) -> (tempfile::TempDir, SqliteStore, ProjectId, ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("realm.db")).unwrap();
        let a = ProjectId::generate();
        let b = ProjectId::generate();
        for (id, name) in [(a, "a"), (b, "b")] {
            store.connection.execute("INSERT INTO projects(id,name,root_path,revision,created_at) VALUES (?1,?2,?3,1,?4)",params![id.to_string(),name,format!("/{name}"),Timestamp::now().to_string()]).unwrap();
            // The backlog of a project created here is always native; only the
            // memory origin is what these tests vary.
            for (subject, origin) in [
                (AuthoritySubject::Memory, memory),
                (AuthoritySubject::Backlog, SubjectOrigin::KontorNative),
            ] {
                store
                    .connection
                    .execute(
                        "INSERT INTO project_subject_authority
                             (project_id,subject,origin,authority,revision)
                         VALUES (?1,?2,?3,?4,1)",
                        params![
                            id.to_string(),
                            subject.as_str(),
                            origin.as_str(),
                            origin.initial_authority().as_str()
                        ],
                    )
                    .unwrap();
            }
        }
        (dir, store, a, b)
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
        store.connection.execute(
            "INSERT INTO memory_fts(project_id,item_id,revision_id,document) VALUES (?1,'policy',?2,?3)",
            params![a.to_string(), proposal.revision_id, document("approved searchable alpha").json()],
        ).unwrap();
        assert!(store.list_memory(a).unwrap().is_empty());
        assert!(
            store.search_memory(a, "alpha", 10).unwrap().is_empty(),
            "a tombstone remains excluded even if its derived index is stale"
        );
    }

    #[test]
    fn run_context_freezes_pack_and_memory_atomically_and_reuses_them() {
        let (_dir, store, project, _) = fixture();
        let task_id = TaskId::generate();
        let now = Timestamp::now();
        store
            .connection
            .execute(
                "INSERT INTO tasks(id,project_id,title,state,revision,created_at,updated_at)
                 VALUES (?1,?2,'Memory task','ready',1,?3,?3)",
                params![task_id.to_string(), project.to_string(), now.to_string()],
            )
            .unwrap();
        let (proposal, _) = store
            .propose_memory_revision(
                project,
                "launch-policy",
                0,
                &document("use the approved launch policy"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(
                project,
                "launch-policy",
                &proposal.revision_id,
                1,
                "reviewer",
            )
            .unwrap();

        let pack_id = ContextPackId::generate();
        let pack = document("first canonical pack");
        let selection = document("all approved project memory");
        let first = store
            .freeze_run_context(
                project,
                "run-context-1",
                pack_id,
                task_id,
                &pack,
                &selection,
                std::slice::from_ref(&proposal.revision_id),
            )
            .unwrap();
        let retry = store
            .freeze_run_context(
                project,
                "run-context-1",
                pack_id,
                task_id,
                &document("newer bytes must not replace the freeze"),
                &document("newer selection must not replace the freeze"),
                &[],
            )
            .unwrap();
        assert_eq!(first.context_pack.content, retry.context_pack.content);
        assert_eq!(
            first.memory_binding.result_hash,
            retry.memory_binding.result_hash
        );
        assert_eq!(retry.memory_binding.ordered_revisions.len(), 1);

        let failed_pack_id = ContextPackId::generate();
        let failed = store.freeze_run_context(
            project,
            "run-context-bad",
            failed_pack_id,
            task_id,
            &pack,
            &selection,
            &["missing-revision".to_owned()],
        );
        assert!(matches!(failed, Err(MemoryError::NotFound)));
        let persisted: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM context_packs WHERE project_id=?1 AND id=?2",
                params![project.to_string(), failed_pack_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, 0, "the pack rolls back with a refused binding");
    }

    #[test]
    fn reproposal_never_resets_the_aggregate_revision() {
        let (_dir, store, project, _) = fixture();
        store
            .propose_memory_revision(
                project,
                "integrity",
                0,
                &document("revision one"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .propose_memory_revision(
                project,
                "integrity",
                1,
                &document("revision two"),
                &provenance(),
                "author",
            )
            .unwrap();
        let (aggregate, maximum): (i64, i64) = store
            .connection
            .query_row(
                "SELECT i.aggregate_revision, MAX(r.revision)
                 FROM memory_items i JOIN memory_revisions r
                   ON r.project_id=i.project_id AND r.item_id=i.id
                 WHERE i.project_id=?1 AND i.id='integrity'",
                [project.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((aggregate, maximum), (2, 2));
    }

    #[test]
    fn two_approvals_leave_exactly_one_current_revision() {
        let (_dir, store, project, _) = fixture();
        let (first, _) = store
            .propose_memory_revision(
                project,
                "single-current",
                0,
                &document("first"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, "single-current", &first.revision_id, 1, "reviewer")
            .unwrap();
        let (second, _) = store
            .propose_memory_revision(
                project,
                "single-current",
                2,
                &document("second"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(
                project,
                "single-current",
                &second.revision_id,
                3,
                "reviewer",
            )
            .unwrap();

        let history = store.memory_history(project, "single-current").unwrap();
        assert_eq!(
            history.iter().filter(|revision| revision.approved).count(),
            2
        );
        assert_eq!(
            history.iter().filter(|revision| revision.current).count(),
            1
        );
        assert!(
            history
                .iter()
                .any(|revision| { revision.revision_id == second.revision_id && revision.current })
        );
    }

    #[test]
    fn frozen_revision_hash_is_the_approved_stored_hash() {
        let (_dir, store, project, _) = fixture();
        let (proposal, _) = store
            .propose_memory_revision(
                project,
                "frozen-hash",
                0,
                &document("freeze exact bytes"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, "frozen-hash", &proposal.revision_id, 1, "reviewer")
            .unwrap();
        let binding = store
            .freeze_memory_binding(
                project,
                "hash-run",
                &document("selection"),
                std::slice::from_ref(&proposal.revision_id),
            )
            .unwrap();
        let stored = store.memory_history(project, "frozen-hash").unwrap();
        assert_eq!(binding.ordered_revisions.len(), 1);
        assert_eq!(
            binding.ordered_revisions[0].content_hash,
            *stored[0].document.hash()
        );
        assert_eq!(
            binding.ordered_revisions[0].content_hash,
            *proposal.document.hash()
        );
    }

    #[test]
    fn proposal_never_enters_fts_before_approval() {
        let (_dir, store, project, _) = fixture();
        store
            .propose_memory_revision(
                project,
                "draft-index",
                0,
                &document("unapproved draft phrase"),
                &provenance(),
                "author",
            )
            .unwrap();
        let unapproved: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_fts f
                 LEFT JOIN memory_approvals a
                   ON a.project_id=f.project_id AND a.revision_id=f.revision_id
                 WHERE f.project_id=?1 AND f.item_id='draft-index'
                   AND a.revision_id IS NULL",
                [project.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            unapproved, 0,
            "a proposal is scanned and stored, never indexed"
        );
    }

    #[test]
    fn concurrent_approvers_get_one_commit_and_one_typed_conflict() {
        let (dir, store, project, _) = fixture();
        let (proposal, _) = store
            .propose_memory_revision(
                project,
                "race",
                0,
                &document("one winner"),
                &provenance(),
                "author",
            )
            .unwrap();
        drop(store);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for reviewer in ["reviewer-a", "reviewer-b"] {
            let database = dir.path().join("realm.db");
            let barrier = std::sync::Arc::clone(&barrier);
            let revision_id = proposal.revision_id.clone();
            threads.push(std::thread::spawn(move || {
                let store = SqliteStore::open(&database).unwrap();
                barrier.wait();
                store.approve_memory_revision(project, "race", &revision_id, 1, reviewer)
            }));
        }
        barrier.wait();
        let outcomes: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(
                    outcome,
                    Err(MemoryError::RevisionConflict {
                        expected: 1,
                        current: 2
                    })
                ))
                .count(),
            1,
            "the stale concurrent writer receives the typed current revision"
        );
    }

    #[test]
    fn purge_removes_payload_approval_and_index_but_keeps_receipts() {
        let (_dir, store, project, _) = fixture();
        let (proposal, propose_receipt) = store
            .propose_memory_revision(
                project,
                "purged",
                0,
                &document("erase this phrase"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, "purged", &proposal.revision_id, 1, "reviewer")
            .unwrap();
        let purge_receipt = store
            .purge_memory(project, "purged", "privacy-admin")
            .unwrap();

        assert!(store.memory_history(project, "purged").unwrap().is_empty());
        assert!(
            store
                .search_memory(project, "erase", 10)
                .unwrap()
                .is_empty()
        );
        for table in ["memory_revisions", "memory_approvals", "memory_fts"] {
            let count: i64 = store
                .connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE project_id=?1"),
                    [project.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "purge leaves no row in {table}");
        }
        let receipt_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_receipts WHERE id IN (?1,?2)",
                params![propose_receipt.receipt_id, purge_receipt.receipt_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(receipt_count, 2, "hashed operation receipts survive purge");
    }

    /// An import that fails *after* its items, while writing the manifest, leaves
    /// nothing — and the retry is a first attempt again.
    ///
    /// This is the failure the old two-transaction shape could not survive.
    /// `already_imported` is derived from the manifest, so items committed without
    /// one are invisible to the retry: it re-ran the item loop, died on the
    /// existing primary key, and the subject could never reach its switch. The
    /// existing cutover test injects its failure *inside* the item loop, which is
    /// why it passed either way. The mutants this kills are committing the items
    /// before the manifest, and reading the readback hash on the store's
    /// connection after that commit.
    #[test]
    fn an_import_that_fails_while_recording_its_manifest_leaves_nothing_and_resumes() {
        let (_dir, store, project, _) = legacy_fixture();
        let mut export = AgentsRoomExport {
            schema_version: 1,
            source: "agentsroom".into(),
            project_id: project,
            entries: vec![
                LegacyMemoryEntry {
                    item_id: "first".into(),
                    document: document("first legacy value"),
                    source_id: Some("old-1".into()),
                },
                LegacyMemoryEntry {
                    item_id: "second".into(),
                    document: document("second legacy value"),
                    source_id: Some("old-2".into()),
                },
            ],
            export_hash: ContentHash::of(b"placeholder"),
        };
        export.export_hash = export.calculate_hash().unwrap();

        // Fail the manifest insert only — every item has already been written by
        // the time this fires.
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_manifest BEFORE INSERT ON subject_import_manifests
                 BEGIN SELECT RAISE(ABORT, 'injected manifest failure'); END;",
            )
            .unwrap();
        assert!(matches!(
            store.apply_agentsroom_import(&export),
            Err(MemoryError::Sqlite(_))
        ));
        for table in [
            "memory_items",
            "memory_revisions",
            "memory_approvals",
            "subject_import_manifests",
        ] {
            let count: i64 = store
                .connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE project_id=?1"),
                    [project.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "the failed manifest rolls back {table} with it");
        }
        assert!(
            !store
                .preview_agentsroom_import(&export)
                .unwrap()
                .already_imported,
            "a rolled-back import is still pending, not half done"
        );

        store
            .connection
            .execute("DROP TRIGGER fail_manifest", [])
            .unwrap();
        store.apply_agentsroom_import(&export).unwrap();
        assert_eq!(
            store.list_memory(project).unwrap().len(),
            2,
            "the retry imports every item exactly once"
        );

        // And the subject can still reach its switch, which is what the stuck state
        // took away.
        let (attested, _) = store
            .attest_subject_source_frozen(
                project,
                AuthoritySubject::Memory,
                AggregateRevision::INITIAL,
                "agentsroom-cursor-1",
                &ContentHash::of(b"frozen source"),
            )
            .unwrap();
        store
            .switch_project_memory_authority(
                project,
                "agentsroom",
                &export.export_hash,
                attested.revision,
            )
            .unwrap();
        assert!(
            store
                .subject_authority(project, AuthoritySubject::Memory)
                .unwrap()
                .writable_by_kontor()
        );
    }

    #[test]
    fn cutover_is_attested_hashed_transactional_and_idempotent() {
        let (_dir, store, project, other) = legacy_fixture();
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
        assert!(
            matches!(
                store.propose_memory_revision(
                    project,
                    "native-too-early",
                    0,
                    &document("not yet"),
                    &provenance(),
                    "author"
                ),
                Err(MemoryError::Authority { .. })
            ),
            "a pending subject refuses native writes before its switch"
        );
        assert!(
            !store
                .preview_agentsroom_import(&export)
                .unwrap()
                .already_imported
        );
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_memory_import BEFORE INSERT ON memory_items
             WHEN NEW.id = 'legacy' BEGIN SELECT RAISE(ABORT, 'injected import failure'); END;",
            )
            .unwrap();
        assert!(matches!(
            store.apply_agentsroom_import(&export),
            Err(MemoryError::Sqlite(_))
        ));
        for table in [
            "subject_import_manifests",
            "memory_items",
            "memory_revisions",
        ] {
            let count: i64 = store
                .connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE project_id=?1"),
                    [project.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "the injected failure rolls back {table}");
        }
        store
            .connection
            .execute("DROP TRIGGER fail_memory_import", [])
            .unwrap();
        store.apply_agentsroom_import(&export).unwrap();
        assert!(
            store
                .apply_agentsroom_import(&export)
                .unwrap()
                .already_imported
        );
        // The switch is refused until the operator has attested that *this*
        // project's legacy source is frozen.
        assert!(matches!(
            store.switch_project_memory_authority(
                project,
                "agentsroom",
                &export.export_hash,
                AggregateRevision::INITIAL
            ),
            Err(MemoryError::Rule(_))
        ));
        let (attested, attest_receipt) = store
            .attest_subject_source_frozen(
                project,
                AuthoritySubject::Memory,
                AggregateRevision::INITIAL,
                "agentsroom-cursor-9",
                &ContentHash::of(b"frozen source"),
            )
            .unwrap();
        assert_eq!(attest_receipt.operation, "attest");
        assert!(
            !attested.writable_by_kontor(),
            "attesting a frozen source does not itself move authority"
        );
        // A readback that does not describe stored state cannot be switched
        // against, even with a manifest and an attestation in place.
        assert!(matches!(
            store.switch_subject_authority(
                project,
                AuthoritySubject::Memory,
                "agentsroom",
                &export.export_hash,
                &ContentHash::of(b"not what is stored"),
                attested.revision,
            ),
            Err(crate::authority::AuthorityError::Rule(_))
        ));
        let switch_receipt = store
            .switch_project_memory_authority(
                project,
                "agentsroom",
                &export.export_hash,
                attested.revision,
            )
            .unwrap();
        let replayed_switch = store
            .switch_project_memory_authority(
                project,
                "agentsroom",
                &export.export_hash,
                attested.revision.next().unwrap(),
            )
            .unwrap();
        assert_eq!(
            replayed_switch.receipt_id, switch_receipt.receipt_id,
            "an identical switch replays its receipt rather than moving authority twice"
        );
        assert!(
            matches!(
                store.propose_memory_revision(
                    other,
                    "sibling",
                    0,
                    &document("still legacy"),
                    &provenance(),
                    "author"
                ),
                Err(MemoryError::Authority { .. })
            ),
            "switching one project does not switch another in the same realm"
        );
        assert!(
            store
                .subject_authority(project, AuthoritySubject::Backlog)
                .unwrap()
                .writable_by_kontor(),
            "the memory switch left this project's backlog exactly as it was"
        );
        let (native_revision, _) = store
            .propose_memory_revision(
                project,
                "first-native",
                0,
                &document("after cutover"),
                &provenance(),
                "author",
            )
            .unwrap();
        assert_eq!(
            native_revision.revision, 1,
            "the first native write begins only after switch"
        );
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
