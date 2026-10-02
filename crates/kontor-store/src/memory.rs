//! Native, project-isolated memory ledger and its rebuildable FTS projection.
#![allow(missing_docs)]

use kontor_core::authority::AuthoritySubject;
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, ContentHash, ProjectId, Timestamp, parse_utc_timestamp,
};
use kontor_core::memory::*;
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
    #[error("experience memory refused: {0}")]
    Refused(MemoryRefusal),
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
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let result = propose_memory_revision_in(
            &tx,
            project_id,
            item_id,
            expected_revision,
            document,
            provenance,
            proposed_by,
        )?;
        tx.commit()?;
        Ok(result)
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
        self.read_memory_revisions(project_id, item_id, false)
    }

    fn read_memory_revisions(
        &self,
        project_id: ProjectId,
        item_id: &str,
        current_only: bool,
    ) -> Result<Vec<MemoryRevision>, MemoryError> {
        // Current retrieval must not decode and revalidate an item's entire
        // append-only history while the API's shared store mutex is held.
        // The pointer selects the approved head, not the newest pending draft.
        let current_filter = if current_only {
            " AND r.id=i.current_revision_id"
        } else {
            ""
        };
        let query = format!(
            "SELECT r.id,r.revision,r.document,r.content_hash,r.provenance,
                    r.proposed_by,r.proposed_at,r.supersedes_id,
                    a.revision_id IS NOT NULL,i.current_revision_id IS r.id,
                    t.item_id IS NOT NULL
             FROM memory_revisions r
             JOIN memory_items i ON i.project_id=r.project_id AND i.id=r.item_id
             LEFT JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id
             LEFT JOIN memory_tombstones t ON t.project_id=r.project_id AND t.item_id=r.item_id
             WHERE r.project_id=?1 AND r.item_id=?2{current_filter}
             ORDER BY r.revision"
        );
        let mut statement = self.connection.prepare(&query)?;
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
                .read_memory_revisions(project_id, &id, true)?
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
                self.read_memory_revisions(project_id, &id, true)?
                    .into_iter()
                    .find(|r| r.current && r.approved)
                    .ok_or(MemoryError::NotFound)
            })
            .collect()
    }

    pub fn rebuild_memory_fts(&self) -> Result<usize, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        rebuild_experience_eligibility_in(&tx)?;
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
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
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
    use kontor_core::authority::SubjectOrigin;

    use super::*;

    fn document(text: &str) -> CanonicalDocument {
        CanonicalDocument::from_value(&serde_json::json!({"schema_version":1,"text":text})).unwrap()
    }
    /// Two projects whose memory was created in Kontor: writable immediately,
    /// with no cutover to wait for.
    pub(super) fn fixture() -> (tempfile::TempDir, SqliteStore, ProjectId, ProjectId) {
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
    pub(super) fn provenance() -> MemoryProvenance {
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
    fn current_retrieval_does_not_decode_superseded_history_or_unapproved_drafts() {
        let (_dir, store, project, other_project) = fixture();
        let (old, _) = store
            .propose_memory_revision(
                project,
                "checkpoint",
                0,
                &document("superseded checkpoint"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, "checkpoint", &old.revision_id, 1, "reviewer")
            .unwrap();
        let (current, _) = store
            .propose_memory_revision(
                project,
                "checkpoint",
                2,
                &document("current searchable checkpoint"),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(project, "checkpoint", &current.revision_id, 3, "reviewer")
            .unwrap();
        let (draft, _) = store
            .propose_memory_revision(
                project,
                "checkpoint",
                4,
                &document("unapproved draft"),
                &provenance(),
                "author",
            )
            .unwrap();

        // A deterministic hydration guard: touching either non-current document
        // fails its hash validation. This avoids a timing-based performance test.
        // Only this disposable test database bypasses the append-only trigger.
        store
            .connection
            .execute_batch("DROP TRIGGER memory_revisions_no_update")
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE memory_revisions SET document=?1 WHERE id IN (?2,?3)",
                params![
                    document("unreadable history sentinel").json(),
                    old.revision_id,
                    draft.revision_id
                ],
            )
            .unwrap();
        assert!(
            store.memory_history(project, "checkpoint").is_err(),
            "the explicit history surface still validates historical bytes"
        );

        let listed = store
            .list_memory(project)
            .expect("list decodes only the approved head");
        let searched = store
            .search_memory(project, "checkpoint", 10)
            .expect("search decodes only the approved head");
        for result in [&listed, &searched] {
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].revision_id, current.revision_id);
            assert_eq!(result[0].document, current.document);
            assert!(result[0].current && result[0].approved && !result[0].tombstoned);
        }
        assert!(
            store
                .search_memory(project, "checkpoint", 0)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .search_memory(project, "unapproved", 10)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .search_memory(other_project, "checkpoint", 10)
                .unwrap()
                .is_empty()
        );
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
    fn first_proposal_history_is_readable_before_approval_and_after_restart() {
        let (dir, store, project, _) = fixture();
        let (proposal, _) = store
            .propose_memory_revision(
                project,
                "pending-review",
                0,
                &document("review these immutable bytes"),
                &provenance(),
                "author",
            )
            .unwrap();
        drop(store);
        let store = SqliteStore::open(&dir.path().join("realm.db")).unwrap();
        let pending = store
            .memory_history(project, "pending-review")
            .expect("a pending first proposal is inspectable");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].revision_id, proposal.revision_id);
        assert_eq!(pending[0].document, proposal.document);
        assert!(!pending[0].approved);
        assert!(!pending[0].current);
        assert!(
            store.list_memory(project).unwrap().is_empty(),
            "unapproved evidence is not approved retrieval"
        );
        store
            .approve_memory_revision(
                project,
                "pending-review",
                &proposal.revision_id,
                1,
                "reviewer",
            )
            .unwrap();
        let approved = store.memory_history(project, "pending-review").unwrap();
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].revision_id, proposal.revision_id);
        assert_eq!(approved[0].document, proposal.document);
        assert!(approved[0].approved);
        assert!(approved[0].current);
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

// Typed experience operations share the generic ledger and its binding primitive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallMetadata {
    pub schema_version: u32,
    pub intent_hash: ContentHash,
    pub mode: RetrievalMode,
    pub reason: Option<DegradedReason>,
    /// Receipt watermark, separate from the existing approval selection cursor.
    pub memory_cursor: i64,
    pub projection_cursor: Option<i64>,
    pub projection_digest: Option<ContentHash>,
    pub identities: Vec<MemoryIdentity>,
    pub exclusions: RecallExclusions,
    pub block_hash: ContentHash,
    pub block_bytes: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct RecalledMemory {
    pub binding: ContextMemoryBinding,
    pub metadata: RecallMetadata,
    /// Exact canonical JSON array, including container overhead.
    pub canonical_block: String,
    pub replayed: bool,
}
#[derive(Debug, Clone)]
pub enum SemanticRecall {
    Degraded(DegradedReason),
    Candidates {
        projection_digest: ContentHash,
        candidates: Vec<MemoryCandidate>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionSnapshot {
    pub project_id: ProjectId,
    pub memory_cursor: i64,
    pub dataset: String,
    pub digest: ContentHash,
    pub identities: Vec<MemoryIdentity>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProjectionPreview {
    pub snapshot: ProjectionSnapshot,
    pub entries: Vec<ProjectionEntry>,
    pub active_generation: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProjectionReadback {
    pub active: Option<ProjectionSnapshot>,
    pub generation: u64,
    pub stale: bool,
    pub adapter_available: bool,
}
/// Produced by the adapter only after all three phases finish for this digest.
#[derive(Debug, Clone)]
pub struct ProjectionQualification {
    pub digest: ContentHash,
    pub added: bool,
    pub cognified: bool,
    pub canary_passed: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct ExperienceClassification {
    pub identity: MemoryIdentity,
    pub recall_eligible: bool,
    pub projection_policy: Option<ProjectionPolicy>,
}

impl SqliteStore {
    pub fn propose_experience(
        &self,
        project_id: ProjectId,
        item_id: &str,
        expected_revision: u64,
        document: &CanonicalDocument,
        provenance: &MemoryProvenance,
        proposed_by: &str,
    ) -> Result<(MemoryRevision, MemoryReceipt), MemoryError> {
        let experience = ExperienceMemoryV1::from_document(document)
            .map_err(|_| MemoryError::Refused(MemoryRefusal::InvalidExperience))?;
        Self::resolve_experience_evidence(&self.connection, project_id, &experience)?;
        self.propose_memory_revision(
            project_id,
            item_id,
            expected_revision,
            document,
            provenance,
            proposed_by,
        )
    }

    fn resolve_experience_evidence(
        connection: &rusqlite::Connection,
        project_id: ProjectId,
        experience: &ExperienceMemoryV1,
    ) -> Result<(), MemoryError> {
        for reference in &experience.evidence_refs {
            let found = match reference {
                EvidenceRef::Artifact { .. } => true, // A digest-bearing citation, never filesystem/network access.
                EvidenceRef::Receipt { receipt_id, content_hash } => connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memory_receipts WHERE project_id=?1 AND id=?2 AND result_hash=?3 UNION ALL SELECT 1 FROM command_receipts WHERE project_id=?1 AND id=?2 AND intent_hash=?3)",
                    params![project_id.to_string(), receipt_id, content_hash.as_str()], |row| row.get::<_, bool>(0))?,
                EvidenceRef::MemoryRevision { project_id: owner, item_id, revision_id, content_hash } => *owner == project_id && connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memory_revisions WHERE project_id=?1 AND item_id=?2 AND id=?3 AND content_hash=?4)",
                    params![project_id.to_string(), item_id, revision_id, content_hash.as_str()], |row| row.get::<_, bool>(0))?,
            };
            if !found {
                return Err(MemoryError::Refused(MemoryRefusal::UnresolvedEvidence));
            }
        }
        Ok(())
    }

    /// Current typed approved heads only, useful for classification/projection.
    /// This explicit administrative read is never the recall fallback.
    pub fn classify_experiences(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ExperienceClassification>, MemoryError> {
        let rows = current_documents(&self.connection, project_id)?;
        Ok(rows
            .into_iter()
            .map(|(identity, document)| {
                let typed = ExperienceMemoryV1::from_document(&document).ok();
                ExperienceClassification {
                    identity,
                    recall_eligible: typed.is_some(),
                    projection_policy: typed.map(|value| value.projection_policy),
                }
            })
            .collect())
    }

    /// Replays first. Selection, authoritative rehydration, budgeting and binding
    /// all run under the same BEGIN IMMEDIATE transaction; no validation gap.
    pub fn recall_experiences(
        &self,
        project_id: ProjectId,
        run_id: &str,
        intent: &RecallIntent,
        semantic: &SemanticRecall,
    ) -> Result<RecalledMemory, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let result = recall_experiences_in(&tx, project_id, run_id, intent, semantic)?;
        tx.commit()?;
        Ok(result)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn recall_experiences_idempotent(
        &self,
        project_id: ProjectId,
        run_id: &str,
        task_id: &str,
        key: &kontor_core::id::IdempotencyKey,
        intent: &RecallIntent,
        semantic: &SemanticRecall,
    ) -> Result<RecalledMemory, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let existing = tx.query_row("SELECT run_id,task_id FROM memory_recall_keys WHERE project_id=?1 AND idempotency_key=?2",params![project_id.to_string(),key.as_str()],|row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).optional()?;
        if existing.is_some_and(|(run, task)| run != run_id || task != task_id) {
            return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
        }
        let result = recall_experiences_in(&tx, project_id, run_id, intent, semantic)?;
        tx.execute("INSERT INTO memory_recall_keys(project_id,idempotency_key,run_id,task_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![project_id.to_string(),key.as_str(),run_id,task_id])?;
        tx.commit()?;
        Ok(result)
    }
    pub fn replay_experiences_idempotent(
        &self,
        project_id: ProjectId,
        run_id: &str,
        task_id: &str,
        key: &kontor_core::id::IdempotencyKey,
    ) -> Result<Option<RecalledMemory>, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let existing = tx.query_row("SELECT run_id,task_id FROM memory_recall_keys WHERE project_id=?1 AND idempotency_key=?2",params![project_id.to_string(),key.as_str()],|row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).optional()?;
        if existing.is_some_and(|(run, task)| run != run_id || task != task_id) {
            return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
        }
        let result = read_recall_in(&tx, project_id, run_id)?;
        if result.is_some() {
            tx.execute("INSERT INTO memory_recall_keys(project_id,idempotency_key,run_id,task_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![project_id.to_string(),key.as_str(),run_id,task_id])?;
            tx.commit()?;
        }
        Ok(result)
    }
    pub fn preview_recall(
        &self,
        project_id: ProjectId,
        intent: &RecallIntent,
        semantic: &SemanticRecall,
    ) -> Result<RecalledMemory, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        // Compute through the same selector; the preview transaction is rolled back.
        recall_experiences_in(
            &tx,
            project_id,
            &format!("preview-{}", Uuid::now_v7()),
            intent,
            semantic,
        )
    }

    pub fn recalled_memory(
        &self,
        project_id: ProjectId,
        run_id: &str,
    ) -> Result<Option<RecalledMemory>, MemoryError> {
        // One read transaction prevents purge halfway through materialization.
        let tx = self.connection.unchecked_transaction()?;
        read_recall_in(&tx, project_id, run_id)
    }

    pub fn projection_preview(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectionPreview, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        projection_preview_in(&tx, project_id)
    }
    pub fn projection_readback(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectionReadback, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        projection_readback_in(&tx, project_id)
    }
    /// Persist identities/digest only; payload is rehydrated while preparing a
    /// rebuild and never retained as a second canonical copy after purge.
    pub fn stage_projection(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectionPreview, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let preview = projection_preview_in(&tx, project_id)?;
        let snapshot = &preview.snapshot;
        tx.execute("INSERT INTO memory_projection_snapshots(project_id,memory_cursor,dataset,digest,identities,created_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT DO NOTHING", params![project_id.to_string(), snapshot.memory_cursor, snapshot.dataset, snapshot.digest.as_str(), serde_json::to_string(&snapshot.identities)?, Timestamp::now().to_string()])?;
        let stored =
            read_snapshot(&tx, project_id, snapshot.memory_cursor)?.ok_or(MemoryError::NotFound)?;
        if stored.digest != snapshot.digest || stored.identities != snapshot.identities {
            return Err(MemoryError::Refused(MemoryRefusal::ProjectionConflict));
        }
        tx.commit()?;
        Ok(preview)
    }
    /// Network work finishes before this authoritative freshness/CAS transaction.
    pub fn activate_projection(
        &self,
        project_id: ProjectId,
        cursor: i64,
        expected_generation: u64,
        qualification: &ProjectionQualification,
    ) -> Result<ProjectionReadback, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        if !qualification.added || !qualification.cognified || !qualification.canary_passed {
            return Err(MemoryError::Refused(MemoryRefusal::ProjectionUnavailable));
        }
        let snapshot = read_snapshot(&tx, project_id, cursor)?.ok_or(MemoryError::NotFound)?;
        let preview = projection_preview_in(&tx, project_id)?;
        if preview.active_generation != expected_generation
            || snapshot.digest != qualification.digest
            || snapshot.digest != preview.snapshot.digest
            || cursor != preview.snapshot.memory_cursor
        {
            return Err(MemoryError::Refused(MemoryRefusal::ProjectionConflict));
        }
        tx.execute("INSERT INTO memory_projection_active(project_id,memory_cursor,generation) VALUES (?1,?2,?3) ON CONFLICT(project_id) DO UPDATE SET memory_cursor=excluded.memory_cursor,generation=excluded.generation", params![project_id.to_string(), cursor, sql_u64(expected_generation + 1)?])?;
        let result = projection_readback_in(&tx, project_id)?;
        tx.commit()?;
        Ok(result)
    }
}

struct RankedExperience {
    identity: MemoryIdentity,
    document: CanonicalDocument,
    confidence: EvidenceConfidence,
    score: f64,
}

/// The exact candidate tuple is resolved in SQL. Upstream text is never input.
fn hydrate_candidate_in(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    candidate: &MemoryCandidate,
    provider_only: bool,
) -> Result<Option<RankedExperience>, MemoryError> {
    if candidate.validate().is_err() {
        return Ok(None);
    }
    let row = connection.query_row(
        "SELECT r.project_id,r.item_id,r.id,r.document,r.content_hash FROM memory_revisions r
         JOIN memory_items i ON i.project_id=r.project_id AND i.id=r.item_id AND i.current_revision_id=r.id
         JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id
         WHERE r.project_id=?1 AND r.project_id=?2 AND r.item_id=?3 AND r.id=?4 AND r.content_hash=?5
         AND NOT EXISTS(SELECT 1 FROM memory_tombstones t WHERE t.project_id=r.project_id AND t.item_id=r.item_id)",
        params![project_id.to_string(), candidate.project_id.to_string(), candidate.item_id, candidate.revision_id, candidate.content_hash.as_str()],
        |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?, row.get::<_,String>(4)?)),
    ).optional()?;
    let Some((owner, item, revision, json, hash)) = row else {
        return Ok(None);
    };
    let hash = ContentHash::parse(&hash)?;
    let Ok(document) = CanonicalDocument::from_stored(&json, &hash) else {
        return Ok(None);
    };
    if !is_recall_eligible(&document) {
        return Ok(None);
    }
    let Ok(experience) = ExperienceMemoryV1::from_document(&document) else {
        return Ok(None);
    };
    if provider_only && experience.projection_policy != ProjectionPolicy::ProviderEligible {
        return Ok(None);
    }
    Ok(Some(RankedExperience {
        identity: MemoryIdentity {
            project_id: ProjectId::parse(&owner)?,
            item_id: item,
            revision_id: revision,
            content_hash: hash,
        },
        document,
        confidence: experience.confidence,
        score: candidate.score,
    }))
}

fn lexical_candidates_in(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    query: &str,
) -> Result<Vec<RankedExperience>, MemoryError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // Join the indexed revision to the current approved typed head BEFORE LIMIT.
    let mut statement = connection.prepare(
        "SELECT r.item_id,r.id,r.content_hash,bm25(memory_fts) FROM memory_fts
         JOIN memory_revisions r ON r.project_id=memory_fts.project_id AND r.item_id=memory_fts.item_id AND r.id=memory_fts.revision_id
         JOIN memory_items i ON i.project_id=r.project_id AND i.id=r.item_id AND i.current_revision_id=r.id
         JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id
         JOIN memory_experience_eligibility e ON e.project_id=r.project_id AND e.revision_id=r.id
         WHERE r.project_id=?1 AND memory_fts MATCH ?2
         AND NOT EXISTS(SELECT 1 FROM memory_tombstones t WHERE t.project_id=r.project_id AND t.item_id=r.item_id)
         ORDER BY bm25(memory_fts),e.confidence DESC,r.item_id LIMIT ?3")?;
    let candidates = statement
        .query_map(
            params![project_id.to_string(), query, MAX_CANDIDATES as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    candidates
        .into_iter()
        .map(|(item, revision, hash, score)| {
            hydrate_candidate_in(
                connection,
                project_id,
                &MemoryCandidate {
                    project_id,
                    item_id: item,
                    revision_id: revision,
                    content_hash: ContentHash::parse(&hash)?,
                    score: -score,
                },
                false,
            )
        })
        .filter_map(|result| result.transpose())
        .collect()
}

fn receipt_cursor(connection: &rusqlite::Connection) -> Result<i64, MemoryError> {
    Ok(connection.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM memory_receipts",
        [],
        |row| row.get(0),
    )?)
}
fn current_documents(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
) -> Result<Vec<(MemoryIdentity, CanonicalDocument)>, MemoryError> {
    let mut statement = connection.prepare("SELECT r.item_id,r.id,r.document,r.content_hash FROM memory_revisions r JOIN memory_items i ON i.project_id=r.project_id AND i.id=r.item_id AND i.current_revision_id=r.id JOIN memory_approvals a ON a.project_id=r.project_id AND a.revision_id=r.id WHERE r.project_id=?1 AND NOT EXISTS(SELECT 1 FROM memory_tombstones t WHERE t.project_id=r.project_id AND t.item_id=r.item_id) ORDER BY r.item_id")?;
    let rows = statement
        .query_map([project_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(item, revision, json, hash)| {
            let hash = ContentHash::parse(&hash)?;
            Ok((
                MemoryIdentity {
                    project_id,
                    item_id: item,
                    revision_id: revision,
                    content_hash: hash.clone(),
                },
                CanonicalDocument::from_stored(&json, &hash)?,
            ))
        })
        .collect()
}
fn projection_preview_in(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
) -> Result<ProjectionPreview, MemoryError> {
    let cursor = receipt_cursor(connection)?;
    let entries: Vec<_> = current_documents(connection, project_id)?
        .into_iter()
        .filter_map(|(identity, document)| {
            let experience = ExperienceMemoryV1::from_document(&document).ok()?;
            (experience.projection_policy == ProjectionPolicy::ProviderEligible).then_some(
                ProjectionEntry {
                    identity,
                    cues: experience.future_cues,
                    lesson: experience.lesson,
                },
            )
        })
        .collect();
    let dataset = format!("kontor_{project_id}_{cursor}");
    let digest =
        ContentHash::of(serde_json::to_vec(&(project_id, cursor, &dataset, &entries))?.as_slice());
    let generation = active_generation(connection, project_id)?;
    Ok(ProjectionPreview {
        snapshot: ProjectionSnapshot {
            project_id,
            memory_cursor: cursor,
            dataset,
            digest,
            identities: entries.iter().map(|entry| entry.identity.clone()).collect(),
        },
        entries,
        active_generation: generation,
    })
}
fn active_generation(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
) -> Result<u64, MemoryError> {
    let generation = connection
        .query_row(
            "SELECT generation FROM memory_projection_active WHERE project_id=?1",
            [project_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    u64::try_from(generation)
        .map_err(|_| MemoryError::Rule("stored projection generation is negative"))
}
fn read_snapshot(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    cursor: i64,
) -> Result<Option<ProjectionSnapshot>, MemoryError> {
    let row = connection.query_row("SELECT dataset,digest,identities FROM memory_projection_snapshots WHERE project_id=?1 AND memory_cursor=?2", params![project_id.to_string(),cursor], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?))).optional()?;
    row.map(|(dataset, digest, identities)| {
        Ok(ProjectionSnapshot {
            project_id,
            memory_cursor: cursor,
            dataset,
            digest: ContentHash::parse(&digest)?,
            identities: serde_json::from_str(&identities)?,
        })
    })
    .transpose()
}
fn projection_readback_in(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
) -> Result<ProjectionReadback, MemoryError> {
    let cursor = connection
        .query_row(
            "SELECT memory_cursor FROM memory_projection_active WHERE project_id=?1",
            [project_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(ProjectionReadback {
        active: cursor
            .map(|value| read_snapshot(connection, project_id, value)?.ok_or(MemoryError::NotFound))
            .transpose()?,
        generation: active_generation(connection, project_id)?,
        stale: cursor.is_none_or(|value| {
            receipt_cursor(connection).map_or(true, |current| value != current)
        }),
        adapter_available: false,
    })
}
fn read_recall_in(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    run_id: &str,
) -> Result<Option<RecalledMemory>, MemoryError> {
    let json = connection
        .query_row(
            "SELECT metadata FROM memory_recall_metadata WHERE project_id=?1 AND run_id=?2",
            params![project_id.to_string(), run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(json) = json else {
        return Ok(None);
    };
    let metadata: RecallMetadata = serde_json::from_str(&json)?;
    let mismatch = || MemoryError::Refused(MemoryRefusal::FrozenPayloadMismatch);
    let binding = read_binding(connection, project_id, run_id)?.ok_or_else(mismatch)?;
    let frozen: Vec<_> = metadata
        .identities
        .iter()
        .map(|identity| FrozenRevision {
            revision_id: identity.revision_id.clone(),
            content_hash: identity.content_hash.clone(),
        })
        .collect();
    let expected_hash = ContentHash::of(
        serde_json::to_string(&(
            binding.selection_cursor,
            binding.selection_spec.hash(),
            &frozen,
        ))?
        .as_bytes(),
    );
    let spec: serde_json::Value = serde_json::from_str(binding.selection_spec.json())?;
    if binding.ordered_revisions != frozen
        || binding.result_hash != expected_hash
        || spec["metadata"] != serde_json::to_value(&metadata)?
    {
        return Err(mismatch());
    }
    let mut block = String::from("[");
    for (index, identity) in metadata.identities.iter().enumerate() {
        if identity.project_id != project_id {
            return Err(mismatch());
        }
        // Historical bytes, deliberately without current/approval/tombstone guards.
        let json = connection.query_row("SELECT document FROM memory_revisions WHERE project_id=?1 AND item_id=?2 AND id=?3 AND content_hash=?4", params![project_id.to_string(),identity.item_id,identity.revision_id,identity.content_hash.as_str()], |row| row.get::<_,String>(0)).optional()?.ok_or(MemoryError::Refused(MemoryRefusal::FrozenPayloadPurged))?;
        let document = CanonicalDocument::from_stored(&json, &identity.content_hash)
            .map_err(|_| mismatch())?;
        if index > 0 {
            block.push(',');
        }
        block.push_str(document.json());
    }
    block.push(']');
    if block.len() != metadata.block_bytes
        || block.len() > MAX_RECALL_BYTES
        || metadata.identities.len() > MAX_RECALL_ITEMS
        || ContentHash::of(block.as_bytes()) != metadata.block_hash
    {
        return Err(mismatch());
    }
    Ok(Some(RecalledMemory {
        binding,
        metadata,
        canonical_block: block,
        replayed: true,
    }))
}

fn propose_memory_revision_in(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    item_id: &str,
    expected_revision: u64,
    document: &CanonicalDocument,
    provenance: &MemoryProvenance,
    proposed_by: &str,
) -> Result<(MemoryRevision, MemoryReceipt), MemoryError> {
    kontor_core::id::reject_sensitive_text("memory.item_id", item_id)?;
    kontor_core::id::reject_sensitive_text("memory.proposed_by", proposed_by)?;
    kontor_core::id::reject_sensitive_material(&serde_json::to_value(provenance)?)?;
    require_subject_authority(tx, project_id, AuthoritySubject::Memory)?;
    let current = aggregate_revision(tx, project_id, item_id)?.unwrap_or(0);
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
    if let Ok(experience) = ExperienceMemoryV1::from_document(document) {
        SqliteStore::resolve_experience_evidence(tx, project_id, &experience)?;
        tx.execute("INSERT INTO memory_experience_eligibility(project_id,revision_id,confidence,projection_policy) VALUES (?1,?2,?3,?4)", params![project_id.to_string(), revision_id, experience.confidence.as_str(), experience.projection_policy.as_str()])?;
    }
    tx.execute(
            "UPDATE memory_items SET aggregate_revision=?3 WHERE project_id=?1 AND id=?2 AND aggregate_revision=?4",
            params![project_id.to_string(), item_id, sql_u64(revision)?, sql_u64(current)?],
        )?;
    let receipt = receipt(
        tx,
        project_id,
        "propose",
        Some(item_id),
        Some(&revision_id),
        Some(revision),
        document.hash(),
    )?;
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

impl SqliteStore {
    #[allow(clippy::too_many_arguments)]
    pub fn propose_experience_idempotent(
        &self,
        project_id: ProjectId,
        key: &kontor_core::id::IdempotencyKey,
        item_id: &str,
        expected_revision: u64,
        document: &CanonicalDocument,
        provenance: &MemoryProvenance,
        proposed_by: &str,
    ) -> Result<(MemoryRevision, MemoryReceipt), MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let request = CanonicalDocument::from_value(
            &serde_json::json!({"schema_version":1,"project_id":project_id,"item_id":item_id,"expected_revision":expected_revision,"document":document,"provenance":provenance,"proposed_by":proposed_by}),
        )?;
        let existing = tx.query_row("SELECT request_hash,revision_id,receipt FROM memory_experience_proposals WHERE project_id=?1 AND idempotency_key=?2", params![project_id.to_string(),key.as_str()], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?))).optional()?;
        if let Some((hash, revision_id, receipt)) = existing {
            if hash != request.hash().as_str() {
                return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
            }
            let revision = self
                .read_memory_revisions(project_id, item_id, false)?
                .into_iter()
                .find(|revision| revision.revision_id == revision_id)
                .ok_or(MemoryError::Refused(MemoryRefusal::FrozenPayloadPurged))?;
            return Ok((revision, serde_json::from_str(&receipt)?));
        }
        let experience = ExperienceMemoryV1::from_document(document)
            .map_err(|_| MemoryError::Refused(MemoryRefusal::InvalidExperience))?;
        Self::resolve_experience_evidence(&tx, project_id, &experience)?;
        let result = propose_memory_revision_in(
            &tx,
            project_id,
            item_id,
            expected_revision,
            document,
            provenance,
            proposed_by,
        )?;
        tx.execute("INSERT INTO memory_experience_proposals(project_id,idempotency_key,request_hash,revision_id,receipt) VALUES (?1,?2,?3,?4,?5)",params![project_id.to_string(),key.as_str(),request.hash().as_str(),result.0.revision_id,serde_json::to_string(&result.1)?])?;
        tx.commit()?;
        Ok(result)
    }
}

fn recall_experiences_in(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    run_id: &str,
    intent: &RecallIntent,
    semantic: &SemanticRecall,
) -> Result<RecalledMemory, MemoryError> {
    if let Some(replayed) = read_recall_in(tx, project_id, run_id)? {
        return Ok(replayed);
    }
    kontor_core::id::reject_sensitive_text("recall.run_id", run_id)?;
    if run_id.is_empty() || run_id.len() > 128 {
        return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
    }
    if read_binding(tx, project_id, run_id)?.is_some() {
        return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
    }
    let intent_doc = intent.canonical()?;
    let memory_cursor = receipt_cursor(tx)?;
    let active = projection_readback_in(tx, project_id)?;
    let mut exclusions = RecallExclusions::default();
    let mut ranked = Vec::new();
    let mut reason = None;
    let mut projection_cursor = None;
    let mut projection_digest = None;
    match semantic {
        SemanticRecall::Degraded(why) => reason = Some(*why),
        SemanticRecall::Candidates {
            projection_digest: digest,
            candidates,
        } => {
            if candidates.len() > MAX_CANDIDATES {
                reason = Some(DegradedReason::Malformed);
            } else if active.stale
                || active
                    .active
                    .as_ref()
                    .is_none_or(|snapshot| &snapshot.digest != digest)
            {
                reason = Some(DegradedReason::Stale);
            } else {
                projection_cursor = active
                    .active
                    .as_ref()
                    .map(|snapshot| snapshot.memory_cursor);
                projection_digest = Some(digest.clone());
                for candidate in candidates {
                    match hydrate_candidate_in(tx, project_id, candidate, true)? {
                        Some(row) => ranked.push(row),
                        None => exclusions.invalid += 1,
                    }
                }
                if ranked.is_empty() {
                    reason = Some(if candidates.is_empty() {
                        DegradedReason::Empty
                    } else {
                        DegradedReason::NoEligibleCandidates
                    });
                }
            }
        }
    }
    let mut mode = RetrievalMode::Semantic;
    if reason.is_some() {
        mode = RetrievalMode::LexicalDegraded;
        ranked = lexical_candidates_in(tx, project_id, &intent.lexical_query()?)?;
    }
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.identity.item_id.cmp(&b.identity.item_id))
    });
    let mut seen = std::collections::BTreeSet::new();
    let mut selected = Vec::new();
    let mut block = String::from("[");
    for row in ranked {
        if !seen.insert(row.identity.item_id.clone()) {
            exclusions.duplicate += 1;
            continue;
        }
        if selected.len() == MAX_RECALL_ITEMS {
            exclusions.item_limit += 1;
            continue;
        }
        let next_bytes =
            block.len() + usize::from(!selected.is_empty()) + row.document.json().len() + 1;
        if next_bytes > MAX_RECALL_BYTES {
            exclusions.budget += 1;
            continue;
        }
        if !selected.is_empty() {
            block.push(',');
        }
        block.push_str(row.document.json());
        selected.push(row.identity);
    }
    block.push(']');
    if selected.is_empty() {
        mode = RetrievalMode::None;
    }
    let metadata = RecallMetadata {
        schema_version: 1,
        intent_hash: intent_doc.hash().clone(),
        mode,
        reason,
        memory_cursor,
        projection_cursor,
        projection_digest,
        identities: selected.clone(),
        exclusions,
        block_hash: ContentHash::of(block.as_bytes()),
        block_bytes: block.len(),
    };
    let spec = CanonicalDocument::from_value(
        &serde_json::json!({"schema_version":1,"selection_version":"experience_recall_v1","intent":intent,"metadata":metadata,"max_items":MAX_RECALL_ITEMS,"max_bytes":MAX_RECALL_BYTES}),
    )?;
    let binding = freeze_memory_binding_in(
        tx,
        project_id,
        run_id,
        &spec,
        &selected
            .iter()
            .map(|identity| identity.revision_id.clone())
            .collect::<Vec<_>>(),
    )?;
    tx.execute(
        "INSERT INTO memory_recall_metadata(project_id,run_id,metadata) VALUES (?1,?2,?3)",
        params![
            project_id.to_string(),
            run_id,
            serde_json::to_string(&metadata)?
        ],
    )?;
    Ok(RecalledMemory {
        binding,
        metadata,
        canonical_block: block,
        replayed: false,
    })
}

#[cfg(test)]
mod experience_tests {
    use super::tests::{fixture, provenance};
    use super::*;
    use serde_json::{Value, json};

    fn experience(provider: bool) -> CanonicalDocument {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../kontor-core/tests/fixtures/experience-v1.json"
        ))
        .unwrap();
        if provider {
            value["projection_policy"] = json!("provider_eligible");
        }
        CanonicalDocument::from_value(&value).unwrap()
    }
    fn intent() -> RecallIntent {
        RecallIntent {
            schema_version: 1,
            task_id: "synthetic-task".into(),
            task_title: "publication deployment".into(),
            module: Some("delivery".into()),
            declared_scope: vec!["artifact readback".into()],
            phase: "verification".into(),
        }
    }
    fn approve(
        store: &SqliteStore,
        project: ProjectId,
        item: &str,
        doc: &CanonicalDocument,
    ) -> MemoryCandidate {
        let (revision, _) = store
            .propose_experience(project, item, 0, doc, &provenance(), "author")
            .unwrap();
        store
            .approve_memory_revision(project, item, &revision.revision_id, 1, "reviewer")
            .unwrap();
        MemoryCandidate {
            project_id: project,
            item_id: item.into(),
            revision_id: revision.revision_id,
            content_hash: doc.hash().clone(),
            score: 1.0,
        }
    }
    fn activate(store: &SqliteStore, project: ProjectId) -> ProjectionSnapshot {
        let preview = store.stage_projection(project).unwrap();
        store
            .activate_projection(
                project,
                preview.snapshot.memory_cursor,
                preview.active_generation,
                &ProjectionQualification {
                    digest: preview.snapshot.digest.clone(),
                    added: true,
                    cognified: true,
                    canary_passed: true,
                },
            )
            .unwrap();
        preview.snapshot
    }
    fn semantic(snapshot: &ProjectionSnapshot, candidates: Vec<MemoryCandidate>) -> SemanticRecall {
        SemanticRecall::Candidates {
            projection_digest: snapshot.digest.clone(),
            candidates,
        }
    }
    fn sized(bytes: usize) -> CanonicalDocument {
        let mut value: Value = serde_json::from_str(experience(true).json()).unwrap();
        value["actions"] = json!(vec!["x"; 16]);
        let base = CanonicalDocument::from_value(&value).unwrap().json().len();
        let mut extra = bytes - base;
        let mut actions = vec!["x".to_owned(); 16];
        for action in &mut actions {
            let add = extra.min(2047);
            action.push_str(&"x".repeat(add));
            extra -= add;
        }
        assert_eq!(extra, 0);
        value["actions"] = json!(actions);
        let document = CanonicalDocument::from_value(&value).unwrap();
        assert_eq!(document.json().len(), bytes);
        ExperienceMemoryV1::from_document(&document).unwrap();
        document
    }

    #[test]
    fn exact_budget_32768_32769_unicode_escaping_and_skip_oversized_top() {
        let (_dir, store, a, _) = fixture();
        let exact = approve(&store, a, "exact", &sized(32766));
        let too_big = approve(&store, a, "oversized", &sized(32767));
        let small = approve(&store, a, "small", &experience(true));
        let snapshot = activate(&store, a);
        let output = store
            .recall_experiences(a, "exact", &intent(), &semantic(&snapshot, vec![exact]))
            .unwrap();
        assert_eq!(output.canonical_block.len(), 32768);
        assert_eq!(output.metadata.identities.len(), 1);
        let rejected = store
            .recall_experiences(
                a,
                "oversized",
                &intent(),
                &semantic(&snapshot, vec![too_big.clone()]),
            )
            .unwrap();
        assert_eq!(rejected.canonical_block, "[]");
        assert_eq!(rejected.metadata.exclusions.budget, 1);
        let mut top = too_big;
        top.score = 2.0;
        let skipped = store
            .recall_experiences(a, "skip", &intent(), &semantic(&snapshot, vec![top, small]))
            .unwrap();
        assert_eq!(skipped.metadata.identities[0].item_id, "small");
        assert_eq!(skipped.metadata.exclusions.budget, 1);
        let mut value: Value = serde_json::from_str(experience(true).json()).unwrap();
        value["lesson"] = json!("ø\n\"雪\\");
        let doc = CanonicalDocument::from_value(&value).unwrap();
        let c = approve(&store, a, "unicode", &doc);
        let snapshot = activate(&store, a);
        let unicode = store
            .recall_experiences(a, "unicode", &intent(), &semantic(&snapshot, vec![c]))
            .unwrap();
        assert_eq!(unicode.canonical_block, format!("[{}]", doc.json()));
        assert_eq!(unicode.metadata.block_bytes, doc.json().len() + 2);
    }

    #[test]
    fn deterministic_score_confidence_ties_duplicates_and_ninth_item() {
        let (_dir, store, a, _) = fixture();
        let mut candidates = Vec::new();
        for index in (0..9).rev() {
            candidates.push(approve(
                &store,
                a,
                &format!("item-{index}"),
                &experience(true),
            ));
        }
        let mut inferred: Value = serde_json::from_str(experience(true).json()).unwrap();
        inferred["confidence"] = json!("inferred");
        let inferred = approve(
            &store,
            a,
            "aaa-inferred",
            &CanonicalDocument::from_value(&inferred).unwrap(),
        );
        candidates.push(inferred);
        candidates.push(candidates[0].clone());
        let snapshot = activate(&store, a);
        let output = store
            .recall_experiences(a, "ties", &intent(), &semantic(&snapshot, candidates))
            .unwrap();
        assert_eq!(
            output
                .metadata
                .identities
                .iter()
                .map(|i| i.item_id.as_str())
                .collect::<Vec<_>>(),
            (0..8).map(|i| format!("item-{i}")).collect::<Vec<_>>()
        );
        assert_eq!(output.metadata.exclusions.duplicate, 1);
        assert_eq!(output.metadata.exclusions.item_limit, 2);
    }

    #[test]
    fn malicious_tuple_project_current_approval_tombstone_hash_policy_and_kind() {
        let (_dir, store, a, b) = fixture();
        let valid = approve(&store, a, "valid", &experience(true));
        let foreign = approve(&store, b, "foreign", &experience(true));
        let local = approve(&store, a, "local", &experience(false));
        let stale = approve(&store, a, "stale", &experience(true));
        let (current, _) = store
            .propose_experience(a, "stale", 2, &experience(true), &provenance(), "author")
            .unwrap();
        store
            .approve_memory_revision(a, "stale", &current.revision_id, 3, "reviewer")
            .unwrap();
        let tomb = approve(&store, a, "tomb", &experience(true));
        store
            .tombstone_memory(a, "tomb", 2, "reviewer", "obsolete")
            .unwrap();
        let (draft, _) = store
            .propose_experience(a, "pending", 0, &experience(true), &provenance(), "author")
            .unwrap();
        let (generic, _) = store
            .propose_memory_revision(
                a,
                "gap",
                0,
                &CanonicalDocument::from_value(
                    &json!({"schema_version":1,"kind":"operational_gap","text":"publication"}),
                )
                .unwrap(),
                &provenance(),
                "author",
            )
            .unwrap();
        store
            .approve_memory_revision(a, "gap", &generic.revision_id, 1, "reviewer")
            .unwrap();
        let mut forged = foreign.clone();
        forged.project_id = a;
        let mut wrong_item = valid.clone();
        wrong_item.item_id = "other".into();
        let mut wrong_hash = valid.clone();
        wrong_hash.content_hash = ContentHash::of(b"wrong");
        let mut invalid_score = valid.clone();
        invalid_score.score = f64::NAN;
        let snapshot = activate(&store, a);
        let mut candidates = vec![
            foreign,
            forged,
            local,
            stale,
            tomb,
            wrong_item,
            wrong_hash,
            invalid_score,
            MemoryCandidate {
                project_id: a,
                item_id: "pending".into(),
                revision_id: draft.revision_id,
                content_hash: draft.document.hash().clone(),
                score: 99.0,
            },
            MemoryCandidate {
                project_id: a,
                item_id: "gap".into(),
                revision_id: generic.revision_id,
                content_hash: generic.document.hash().clone(),
                score: 99.0,
            },
        ];
        candidates.push(valid);
        let output = store
            .recall_experiences(a, "malicious", &intent(), &semantic(&snapshot, candidates))
            .unwrap();
        assert_eq!(output.metadata.identities.len(), 1);
        assert_eq!(output.metadata.identities[0].item_id, "valid");
        assert_eq!(output.metadata.exclusions.invalid, 10);
        assert_eq!(output.metadata.mode, RetrievalMode::Semantic);
        assert!(!output.canonical_block.contains("operational_gap"));
    }

    #[test]
    fn lexical_limit_joins_exact_current_eligible_revision_and_never_lists() {
        let (_dir, store, a, b) = fixture();
        let mut old = experience(false);
        let first = approve(&store, a, "stale", &old);
        let mut value: Value = serde_json::from_str(old.json()).unwrap();
        for (key, replacement) in [
            ("situation", json!("unrelated")),
            ("intent", json!("unrelated")),
            ("lesson", json!("unrelated")),
            ("future_cues", json!(["unrelated"])),
            ("actions", json!(["unrelated"])),
            ("outcome", json!({"kind":"success","summary":"unrelated"})),
            ("went_well", json!([])),
            ("went_wrong", json!([])),
            ("avoid", json!([])),
            ("domains", json!(["unrelated"])),
        ] {
            value[key] = replacement;
        }
        value["evidence_refs"][0]["locator"] = json!("synthetic/unrelated.json");
        old = CanonicalDocument::from_value(&value).unwrap();
        let (new, _) = store
            .propose_experience(a, "stale", 2, &old, &provenance(), "author")
            .unwrap();
        store
            .approve_memory_revision(a, "stale", &new.revision_id, 3, "reviewer")
            .unwrap();
        // A stale derived entry must be eliminated before the bounded LIMIT.
        store.connection.execute("INSERT INTO memory_fts(project_id,item_id,revision_id,document) VALUES (?1,'stale',?2,?3)",params![a.to_string(),first.revision_id,experience(false).json()]).unwrap();
        for i in 0..70 {
            let item = format!("gap-{i}");
            let (r,_)=store.propose_memory_revision(a,&item,0,&CanonicalDocument::from_value(&json!({"schema_version":1,"kind":"operational_gap","text":"publication deployment publication deployment"})).unwrap(),&provenance(),"author").unwrap();
            store
                .approve_memory_revision(a, &item, &r.revision_id, 1, "reviewer")
                .unwrap();
        }
        approve(&store, b, "foreign", &experience(false));
        let expected = approve(&store, a, "eligible", &experience(false));
        let mut lexical_intent = intent();
        lexical_intent.module = None;
        lexical_intent.declared_scope.clear();
        lexical_intent.phase = "zzzzphase".into();
        let output = store
            .recall_experiences(
                a,
                "fts",
                &lexical_intent,
                &SemanticRecall::Degraded(DegradedReason::Unavailable),
            )
            .unwrap();
        assert_eq!(output.metadata.identities.len(), 1);
        assert_eq!(
            output.metadata.identities[0].revision_id,
            expected.revision_id
        );
        assert_eq!(output.metadata.mode, RetrievalMode::LexicalDegraded);
        let mut unmatched = intent();
        unmatched.task_title = "zzzznevermatches".into();
        unmatched.module = None;
        unmatched.declared_scope.clear();
        unmatched.phase = "zzzzphase".into();
        let empty = store
            .recall_experiences(
                a,
                "none",
                &unmatched,
                &SemanticRecall::Degraded(DegradedReason::Absent),
            )
            .unwrap();
        assert_eq!(empty.metadata.mode, RetrievalMode::None);
        assert_eq!(empty.canonical_block, "[]");
    }

    #[test]
    fn replay_after_restart_task_edit_rebuild_tombstone_and_purge_preserves_receipt() {
        let (dir, store, a, _) = fixture();
        let selected = approve(&store, a, "selected", &experience(true));
        let snapshot = activate(&store, a);
        let first = store
            .recall_experiences(a, "run", &intent(), &semantic(&snapshot, vec![selected]))
            .unwrap();
        store
            .tombstone_memory(a, "selected", 2, "reviewer", "obsolete")
            .unwrap();
        approve(&store, a, "replacement", &experience(true));
        activate(&store, a);
        drop(store);
        let store = SqliteStore::open(&dir.path().join("realm.db")).unwrap();
        let mut changed = intent();
        changed.task_title = "edited".into();
        let replay = store
            .recall_experiences(
                a,
                "run",
                &changed,
                &SemanticRecall::Degraded(DegradedReason::Timeout),
            )
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.binding.result_hash, first.binding.result_hash);
        assert_eq!(replay.canonical_block, first.canonical_block);
        assert_eq!(replay.metadata.intent_hash, first.metadata.intent_hash);
        store.purge_memory(a, "selected", "admin").unwrap();
        assert!(matches!(
            store.recall_experiences(
                a,
                "run",
                &changed,
                &SemanticRecall::Degraded(DegradedReason::Absent)
            ),
            Err(MemoryError::Refused(MemoryRefusal::FrozenPayloadPurged))
        ));
        assert_eq!(
            store.memory_binding(a, "run").unwrap().unwrap().result_hash,
            first.binding.result_hash
        );
        let stored: String = store
            .connection
            .query_row(
                "SELECT metadata FROM memory_recall_metadata WHERE run_id='run'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stored.contains("situation"));
    }

    #[test]
    fn projection_minimal_payload_failures_freshness_and_compare_and_swap() {
        let (_dir, store, a, _) = fixture();
        approve(&store, a, "provider", &experience(true));
        approve(&store, a, "local", &experience(false));
        let snapshot = activate(&store, a);
        let active = store.projection_readback(a).unwrap();
        assert_eq!(active.active.unwrap().digest, snapshot.digest);
        assert!(!active.stale);
        let preview = store.stage_projection(a).unwrap();
        assert_eq!(preview.entries.len(), 1);
        let payload = serde_json::to_string(&preview.entries).unwrap();
        for forbidden in [
            "evidence_refs",
            "situation",
            "approved",
            "transcript",
            "went_wrong",
        ] {
            assert!(!payload.contains(forbidden));
        }
        for (added, cognified, canary) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
        ] {
            assert!(matches!(
                store.activate_projection(
                    a,
                    snapshot.memory_cursor,
                    1,
                    &ProjectionQualification {
                        digest: snapshot.digest.clone(),
                        added,
                        cognified,
                        canary_passed: canary
                    }
                ),
                Err(MemoryError::Refused(MemoryRefusal::ProjectionUnavailable))
            ));
            assert_eq!(store.projection_readback(a).unwrap().generation, 1);
        }
        assert!(
            store
                .activate_projection(
                    a,
                    snapshot.memory_cursor,
                    0,
                    &ProjectionQualification {
                        digest: snapshot.digest.clone(),
                        added: true,
                        cognified: true,
                        canary_passed: true
                    }
                )
                .is_err()
        );
        approve(&store, a, "new", &experience(true));
        assert!(store.projection_readback(a).unwrap().stale);
        assert!(
            store
                .activate_projection(
                    a,
                    snapshot.memory_cursor,
                    1,
                    &ProjectionQualification {
                        digest: snapshot.digest.clone(),
                        added: true,
                        cognified: true,
                        canary_passed: true
                    }
                )
                .is_err()
        );
        assert_eq!(
            store.projection_readback(a).unwrap().active.unwrap().digest,
            snapshot.digest
        );
    }

    #[test]
    fn every_degraded_reason_empty_foreign_only_candidate_limit_and_preview_rollback() {
        let (_dir, store, a, b) = fixture();
        let local = approve(&store, a, "local", &experience(false));
        let foreign = approve(&store, b, "foreign", &experience(true));
        let snapshot = activate(&store, a);
        for reason in DegradedReason::ALL {
            let result = store
                .recall_experiences(
                    a,
                    reason.as_str(),
                    &intent(),
                    &SemanticRecall::Degraded(*reason),
                )
                .unwrap();
            assert_eq!(result.metadata.reason, Some(*reason));
            assert_eq!(result.metadata.identities[0].revision_id, local.revision_id);
        }
        let empty = store
            .recall_experiences(a, "empty-upstream", &intent(), &semantic(&snapshot, vec![]))
            .unwrap();
        assert_eq!(empty.metadata.reason, Some(DegradedReason::Empty));
        let foreign_only = store
            .recall_experiences(
                a,
                "foreign-only",
                &intent(),
                &semantic(&snapshot, vec![foreign]),
            )
            .unwrap();
        assert_eq!(
            foreign_only.metadata.reason,
            Some(DegradedReason::NoEligibleCandidates)
        );
        assert_eq!(
            foreign_only.metadata.identities[0].revision_id,
            local.revision_id
        );
        let over_bound = store
            .recall_experiences(
                a,
                "candidate-limit",
                &intent(),
                &semantic(&snapshot, vec![local; 65]),
            )
            .unwrap();
        assert_eq!(over_bound.metadata.reason, Some(DegradedReason::Malformed));
        assert_eq!(over_bound.metadata.mode, RetrievalMode::LexicalDegraded);
        let before: i64 = store
            .connection
            .query_row("SELECT count(*) FROM memory_context_bindings", [], |row| {
                row.get(0)
            })
            .unwrap();
        store
            .preview_recall(
                a,
                &intent(),
                &SemanticRecall::Degraded(DegradedReason::Absent),
            )
            .unwrap();
        let after: i64 = store
            .connection
            .query_row("SELECT count(*) FROM memory_context_bindings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn validation_and_freeze_transaction_excludes_concurrent_tombstone() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (dir, store, a, _) = fixture();
        let candidate = approve(&store, a, "race", &experience(true));
        let snapshot = activate(&store, a);
        let competing = SqliteStore::open(&dir.path().join("realm.db")).unwrap();
        let tx =
            Transaction::new_unchecked(&store.connection, TransactionBehavior::Immediate).unwrap();
        assert!(
            hydrate_candidate_in(&tx, a, &candidate, true)
                .unwrap()
                .is_some()
        );
        let (begun, ready) = mpsc::channel();
        let (attempt, attempted) = mpsc::channel();
        let (resume, resumed) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            begun.send(()).unwrap();
            let first = competing.tombstone_memory(a, "race", 2, "reviewer", "concurrent");
            attempt.send(first.is_ok()).unwrap();
            resumed.recv().unwrap();
            let success = first.is_ok()
                || competing
                    .tombstone_memory(a, "race", 2, "reviewer", "retry after freeze")
                    .is_ok();
            done.send(success).unwrap();
        });
        ready.recv().unwrap();
        let before_commit = attempted.recv_timeout(Duration::from_millis(100)).ok();
        assert_ne!(
            before_commit,
            Some(true),
            "writer cannot succeed between validation and freeze"
        );
        let bound = recall_experiences_in(
            &tx,
            a,
            "race-run",
            &intent(),
            &semantic(&snapshot, vec![candidate]),
        )
        .unwrap();
        tx.commit().unwrap();
        resume.send(()).unwrap();
        assert!(result.recv_timeout(Duration::from_secs(5)).unwrap());
        writer.join().unwrap();
        assert_eq!(bound.metadata.identities.len(), 1);
        assert_eq!(
            store
                .recalled_memory(a, "race-run")
                .unwrap()
                .unwrap()
                .canonical_block,
            bound.canonical_block
        );
        let fresh = store
            .recall_experiences(
                a,
                "after-race",
                &intent(),
                &SemanticRecall::Degraded(DegradedReason::Absent),
            )
            .unwrap();
        assert_eq!(fresh.canonical_block, "[]");
    }

    #[test]
    fn typed_proposal_evidence_idempotency_secret_refusal_and_generic_history() {
        let (_dir, store, a, b) = fixture();
        let key = kontor_core::id::IdempotencyKey::parse("experience-proposal").unwrap();
        let doc = experience(false);
        let first = store
            .propose_experience_idempotent(a, &key, "proposal", 0, &doc, &provenance(), "author")
            .unwrap();
        let replay = store
            .propose_experience_idempotent(a, &key, "proposal", 0, &doc, &provenance(), "author")
            .unwrap();
        assert_eq!(first.0.revision_id, replay.0.revision_id);
        assert!(!replay.0.approved);
        assert!(
            store
                .propose_experience_idempotent(a, &key, "other", 0, &doc, &provenance(), "author")
                .is_err()
        );
        let mut evidence: Value = serde_json::from_str(doc.json()).unwrap();
        evidence["evidence_refs"] = json!([{"type":"receipt","receipt_id":first.1.receipt_id,"content_hash":first.1.result_hash}]);
        let doc = CanonicalDocument::from_value(&evidence).unwrap();
        assert!(
            store
                .propose_experience(a, "receipt-backed", 0, &doc, &provenance(), "author")
                .is_ok()
        );
        assert!(matches!(
            store.propose_experience(b, "foreign-receipt", 0, &doc, &provenance(), "author"),
            Err(MemoryError::Refused(MemoryRefusal::UnresolvedEvidence))
        ));
        let mut revision_evidence: Value = serde_json::from_str(experience(false).json()).unwrap();
        revision_evidence["evidence_refs"] = json!([{"type":"memory_revision","project_id":a,"item_id":"proposal","revision_id":first.0.revision_id,"content_hash":first.0.document.hash()}]);
        let revision_doc = CanonicalDocument::from_value(&revision_evidence).unwrap();
        assert!(
            store
                .propose_experience(
                    a,
                    "revision-backed",
                    0,
                    &revision_doc,
                    &provenance(),
                    "author"
                )
                .is_ok()
        );
        assert!(matches!(
            store.propose_experience(
                b,
                "foreign-revision",
                0,
                &revision_doc,
                &provenance(),
                "author"
            ),
            Err(MemoryError::Refused(MemoryRefusal::UnresolvedEvidence))
        ));
        revision_evidence["evidence_refs"][0]["content_hash"] =
            json!(ContentHash::of(b"wrong evidence digest"));
        assert!(
            store
                .propose_experience(
                    a,
                    "wrong-evidence",
                    0,
                    &CanonicalDocument::from_value(&revision_evidence).unwrap(),
                    &provenance(),
                    "author"
                )
                .is_err()
        );
        let mut malicious = provenance();
        malicious.source_id = Some("password=SOURCE_CANARY".into());
        let error = store
            .propose_experience(a, "secret", 0, &experience(false), &malicious, "author")
            .unwrap_err();
        assert!(!format!("{error:?}").contains("SOURCE_CANARY"));
        assert_eq!(store.memory_history(a, "proposal").unwrap().len(), 1);
    }
}

/// Derived eligibility is rebuilt through the same strict parser during upgrade
/// and explicit index repair. Generic ledger history is never rejected globally.
pub(crate) fn rebuild_experience_eligibility_in(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<usize> {
    let mut statement=connection.prepare("SELECT project_id,id,document,content_hash FROM memory_revisions WHERE json_extract(document,'$.document_type')='experience_memory'")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    connection.execute("DELETE FROM memory_experience_eligibility", [])?;
    let mut count = 0;
    for (project, revision, json, hash) in rows {
        let Ok(owner) = ProjectId::parse(&project) else {
            continue;
        };
        let Ok(hash) = ContentHash::parse(&hash) else {
            continue;
        };
        let Ok(document) = CanonicalDocument::from_stored(&json, &hash) else {
            continue;
        };
        let Ok(experience) = ExperienceMemoryV1::from_document(&document) else {
            continue;
        };
        if SqliteStore::resolve_experience_evidence(connection, owner, &experience).is_err() {
            continue;
        }
        count+=connection.execute("INSERT INTO memory_experience_eligibility(project_id,revision_id,confidence,projection_policy) VALUES (?1,?2,?3,?4)",params![project,revision,experience.confidence.as_str(),experience.projection_policy.as_str()])?;
    }
    Ok(count)
}
