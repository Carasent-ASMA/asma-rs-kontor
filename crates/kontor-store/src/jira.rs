//! Durable native Jira materialization and activation facts.

#![allow(missing_docs)]

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId, MiniProjectId,
    ProjectId, TaskId, TicketLinkId, Timestamp, format_utc_timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{NewLocalCommand, RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

use crate::SqliteStore;
use crate::repository::{backend, conflict, ensure_receipt_authorizes, text};

/// The honest result of an atomic Jira conflict close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictClose {
    /// This call persisted the command and closed the conflict together.
    Closed(CommandReceiptId),
    /// The same idempotency key already owns the durable close.
    Replayed(CommandReceiptId),
}

impl ConflictClose {
    /// Whether this call committed the close instead of replaying it.
    #[must_use]
    pub const fn is_fresh(self) -> bool {
        matches!(self, Self::Closed(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JiraItemKind {
    Epic,
    Task,
}

impl JiraItemKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Epic => "epic",
            Self::Task => "task",
        }
    }

    fn parse(value: &str) -> RepositoryResult<Self> {
        match value {
            "epic" => Ok(Self::Epic),
            "task" => Ok(Self::Task),
            _ => Err(kontor_core::DomainError::invalid(
                "Jira materialization item kind",
                "stored an unknown value",
            )
            .into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JiraIntentKind {
    Create,
    Link,
}

impl JiraIntentKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Link => "link",
        }
    }

    fn parse(value: &str) -> RepositoryResult<Self> {
        match value {
            "create" => Ok(Self::Create),
            "link" => Ok(Self::Link),
            _ => Err(kontor_core::DomainError::invalid(
                "Jira materialization intent kind",
                "stored an unknown value",
            )
            .into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJiraMaterializationBatch {
    pub id: ExternalId,
    pub project_id: ProjectId,
    pub epic_id: MiniProjectId,
    pub idempotency_key: String,
    pub preview_hash: ContentHash,
    pub expected_revision: AggregateRevision,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJiraMaterializationItem {
    pub id: ExternalId,
    pub batch_id: ExternalId,
    pub project_id: ProjectId,
    pub epic_id: MiniProjectId,
    pub task_id: Option<TaskId>,
    pub link_id: Option<TicketLinkId>,
    pub ordinal: u32,
    pub item_kind: JiraItemKind,
    pub intent_kind: JiraIntentKind,
    pub requested_key: Option<ExternalId>,
    pub marker: ExternalId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJiraMaterializationItem {
    pub id: ExternalId,
    pub batch_id: ExternalId,
    pub project_id: ProjectId,
    pub epic_id: MiniProjectId,
    pub task_id: Option<TaskId>,
    pub link_id: Option<TicketLinkId>,
    pub ordinal: u32,
    pub item_kind: JiraItemKind,
    pub intent_kind: JiraIntentKind,
    pub requested_key: Option<ExternalId>,
    pub marker: ExternalId,
    pub confirmed_key: Option<ExternalId>,
    pub readback_hash: Option<ContentHash>,
    pub confirmed_at: Option<Timestamp>,
}

/// One exact existing Jira issue a pending create item may adopt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraMaterializationRecoveryItem {
    pub ordinal: u32,
    pub item_kind: JiraItemKind,
    pub task_id: Option<TaskId>,
    pub requested_key: ExternalId,
    pub marker: ExternalId,
}

/// The exact pending batch set selected by a durable recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredJiraMaterialization {
    /// The oldest selected batch, retained as the stable response identity.
    pub batch_id: ExternalId,
    /// Every original batch in deterministic creation order.
    pub batch_ids: Vec<ExternalId>,
    pub items: Vec<StoredJiraMaterializationItem>,
}

impl SqliteStore {
    /// Record authority and close one task Jira conflict in one transaction.
    pub fn resolve_task_jira_conflict_atomically(
        &self,
        project_id: ProjectId,
        conflict_id: kontor_core::id::StatusConflictId,
        command: &NewLocalCommand,
        resolved_at: Timestamp,
    ) -> RepositoryResult<ConflictClose> {
        let transaction = self.begin()?;
        let (receipt_id, recorded_earlier) =
            match crate::commands::intent::insert_local_command(&transaction, command)? {
                Some(existing) => (existing.id, true),
                None => (command.receipt_id, false),
            };
        let row: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT link_id, resolution_receipt_id FROM status_conflicts
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), conflict_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((link_id, resolution)) = row else {
            return Err(conflict("status conflict", "no such conflict is recorded"));
        };
        if let Some(resolution) = resolution {
            let owner = CommandReceiptId::parse(&resolution)?;
            if recorded_earlier && owner == receipt_id {
                transaction.commit().map_err(backend)?;
                return Ok(ConflictClose::Replayed(owner));
            }
            return Err(conflict(
                "status conflict",
                "the conflict is already resolved and its resolution is final",
            ));
        }
        ensure_receipt_authorizes(
            &transaction,
            "StatusConflict",
            project_id,
            receipt_id,
            CommandKind::ResolveStatusConflict,
            AggregateRef::TicketLink {
                link_id: TicketLinkId::parse(&link_id)?,
            },
        )?;
        let changed = transaction
            .execute(
                "UPDATE status_conflicts SET resolved_at = ?1, resolution_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4 AND resolved_at IS NULL",
                params![
                    text(resolved_at),
                    receipt_id.to_string(),
                    project_id.to_string(),
                    conflict_id.to_string(),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "status conflict",
                "the conflict changed before its resolution could commit",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(ConflictClose::Closed(receipt_id))
    }

    /// Record authority and close one epic Jira conflict in one transaction.
    pub fn resolve_epic_jira_conflict_atomically(
        &self,
        project_id: ProjectId,
        conflict_id: kontor_core::id::StatusConflictId,
        command: &NewLocalCommand,
        resolved_at: Timestamp,
    ) -> RepositoryResult<ConflictClose> {
        let transaction = self.begin()?;
        let (receipt_id, recorded_earlier) =
            match crate::commands::intent::insert_local_command(&transaction, command)? {
                Some(existing) => (existing.id, true),
                None => (command.receipt_id, false),
            };
        let row: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT epic_id, resolution_receipt_id FROM epic_status_conflicts
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), conflict_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((epic_id, resolution)) = row else {
            return Err(conflict(
                "epic Jira conflict",
                "no such conflict is recorded",
            ));
        };
        if let Some(resolution) = resolution {
            let owner = CommandReceiptId::parse(&resolution)?;
            if recorded_earlier && owner == receipt_id {
                transaction.commit().map_err(backend)?;
                return Ok(ConflictClose::Replayed(owner));
            }
            return Err(conflict(
                "epic Jira conflict",
                "the conflict is already resolved and its resolution is final",
            ));
        }
        ensure_receipt_authorizes(
            &transaction,
            "EpicStatusConflict",
            project_id,
            receipt_id,
            CommandKind::ResolveStatusConflict,
            AggregateRef::MiniProject {
                mini_project_id: MiniProjectId::parse(&epic_id)?,
            },
        )?;
        let changed = transaction
            .execute(
                "UPDATE epic_status_conflicts SET resolved_at = ?1, resolution_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4 AND resolved_at IS NULL",
                params![
                    text(resolved_at),
                    receipt_id.to_string(),
                    project_id.to_string(),
                    conflict_id.to_string(),
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "epic Jira conflict",
                "the conflict changed before its resolution could commit",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(ConflictClose::Closed(receipt_id))
    }

    /// Persist a complete plan before its first Jira effect.
    pub fn plan_jira_materialization(
        &self,
        batch: &NewJiraMaterializationBatch,
        items: &[NewJiraMaterializationItem],
    ) -> RepositoryResult<()> {
        if items.is_empty() {
            return Err(kontor_core::DomainError::invalid(
                "Jira materialization plan",
                "must contain at least one item",
            )
            .into());
        }
        for (expected_ordinal, item) in items.iter().enumerate() {
            if item.batch_id != batch.id
                || item.project_id != batch.project_id
                || item.epic_id != batch.epic_id
                || usize::try_from(item.ordinal).ok() != Some(expected_ordinal)
            {
                return Err(kontor_core::DomainError::invalid(
                    "Jira materialization plan",
                    "items must exactly match the batch scope in contiguous ordinal order",
                )
                .into());
            }
        }
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO jira_materialization_batches
                 (id, project_id, epic_id, idempotency_key, preview_hash,
                  expected_revision, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'planned', ?7)",
                params![
                    batch.id.as_str(),
                    batch.project_id.to_string(),
                    batch.epic_id.to_string(),
                    batch.idempotency_key,
                    batch.preview_hash.as_str(),
                    i64::try_from(batch.expected_revision.get()).unwrap_or(i64::MAX),
                    format_utc_timestamp(batch.created_at),
                ],
            )
            .map_err(backend)?;
        let stored: (String, String, String, String, i64) = transaction
            .query_row(
                "SELECT id, project_id, epic_id, preview_hash, expected_revision
                 FROM jira_materialization_batches
                 WHERE idempotency_key = ?1",
                [&batch.idempotency_key],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(backend)?;
        if stored
            != (
                batch.id.as_str().to_owned(),
                batch.project_id.to_string(),
                batch.epic_id.to_string(),
                batch.preview_hash.as_str().to_owned(),
                i64::try_from(batch.expected_revision.get()).unwrap_or(i64::MAX),
            )
        {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization",
                rule: "the Jira materialization idempotency key names another plan",
            });
        }
        for item in items {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO jira_materialization_items
                     (id, batch_id, project_id, epic_id, task_id, link_id, ordinal,
                      item_kind, intent_kind, requested_key, marker, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'planned')",
                    params![
                        item.id.as_str(),
                        item.batch_id.as_str(),
                        item.project_id.to_string(),
                        item.epic_id.to_string(),
                        item.task_id.map(|id| id.to_string()),
                        item.link_id.map(|id| id.to_string()),
                        i64::from(item.ordinal),
                        item.item_kind.as_str(),
                        item.intent_kind.as_str(),
                        item.requested_key.as_ref().map(ExternalId::as_str),
                        item.marker.as_str(),
                    ],
                )
                .map_err(backend)?;
        }
        let mut statement = transaction
            .prepare(
                "SELECT id, batch_id, project_id, epic_id, task_id, link_id,
                        ordinal, item_kind, intent_kind, requested_key, marker
                 FROM jira_materialization_items
                 WHERE project_id = ?1 AND batch_id = ?2
                 ORDER BY ordinal",
            )
            .map_err(backend)?;
        let stored_items = statement
            .query_map(
                params![batch.project_id.to_string(), batch.id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        drop(statement);
        let expected_items = items
            .iter()
            .map(|item| {
                (
                    item.id.as_str().to_owned(),
                    item.batch_id.as_str().to_owned(),
                    item.project_id.to_string(),
                    item.epic_id.to_string(),
                    item.task_id.map(|id| id.to_string()),
                    item.link_id.map(|id| id.to_string()),
                    i64::from(item.ordinal),
                    item.item_kind.as_str().to_owned(),
                    item.intent_kind.as_str().to_owned(),
                    item.requested_key
                        .as_ref()
                        .map(|key| key.as_str().to_owned()),
                    item.marker.as_str().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        if stored_items != expected_items {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization plan",
                rule: "the durable batch item set differs from the exact requested plan",
            });
        }
        transaction.commit().map_err(backend)
    }

    /// The stable item set of one planned batch.
    pub fn jira_materialization_items(
        &self,
        project_id: ProjectId,
        batch_id: &ExternalId,
    ) -> RepositoryResult<Vec<StoredJiraMaterializationItem>> {
        let transaction = self.begin()?;
        let mut statement = transaction
            .prepare(
                "SELECT id, batch_id, project_id, epic_id, task_id, link_id, ordinal,
                        item_kind, intent_kind, requested_key, marker, confirmed_key,
                        readback_hash, confirmed_at
                 FROM jira_materialization_items
                 WHERE project_id = ?1 AND batch_id = ?2 ORDER BY ordinal",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), batch_id.as_str()])
            .map_err(backend)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let task: Option<String> = row.get(4).map_err(backend)?;
            let link: Option<String> = row.get(5).map_err(backend)?;
            let requested: Option<String> = row.get(9).map_err(backend)?;
            let confirmed: Option<String> = row.get(11).map_err(backend)?;
            let hash: Option<String> = row.get(12).map_err(backend)?;
            let at: Option<String> = row.get(13).map_err(backend)?;
            items.push(StoredJiraMaterializationItem {
                id: ExternalId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                batch_id: ExternalId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                project_id: ProjectId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                epic_id: MiniProjectId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                task_id: task.as_deref().map(TaskId::parse).transpose()?,
                link_id: link.as_deref().map(TicketLinkId::parse).transpose()?,
                ordinal: u32::try_from(row.get::<_, i64>(6).map_err(backend)?).map_err(|_| {
                    kontor_core::DomainError::invalid(
                        "Jira item ordinal",
                        "stored a value outside u32",
                    )
                })?,
                item_kind: JiraItemKind::parse(&row.get::<_, String>(7).map_err(backend)?)?,
                intent_kind: JiraIntentKind::parse(&row.get::<_, String>(8).map_err(backend)?)?,
                requested_key: requested.as_deref().map(ExternalId::parse).transpose()?,
                marker: ExternalId::parse(&row.get::<_, String>(10).map_err(backend)?)?,
                confirmed_key: confirmed.as_deref().map(ExternalId::parse).transpose()?,
                readback_hash: hash.as_deref().map(ContentHash::parse).transpose()?,
                confirmed_at: at.as_deref().map(parse_utc_timestamp).transpose()?,
            });
        }
        drop(rows);
        drop(statement);
        transaction.commit().map_err(backend)?;
        Ok(items)
    }

    /// Adopt one exact pending create plan for a recovery.
    ///
    /// The plan may be one batch or an exact, non-overlapping union of legacy
    /// batch fragments. Every original item must still have the same ordinal,
    /// kind, task scope and marker. Requested Jira keys are appended to the
    /// immutable recovery ledger before the connector is contacted. Original
    /// batch and item ownership is never rewritten.
    pub fn recover_pending_jira_materialization(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        recovery_receipt_id: CommandReceiptId,
        preview_hash: &ContentHash,
        recovery: &[JiraMaterializationRecoveryItem],
        recovered_at: Timestamp,
    ) -> RepositoryResult<Option<RecoveredJiraMaterialization>> {
        if recovery.is_empty()
            || recovery.iter().enumerate().any(|(ordinal, item)| {
                usize::try_from(item.ordinal).ok() != Some(ordinal)
                    || matches!(item.item_kind, JiraItemKind::Epic) != item.task_id.is_none()
            })
        {
            return Err(kontor_core::DomainError::invalid(
                "Jira materialization recovery",
                "must name an exact non-empty item set in contiguous ordinal order",
            )
            .into());
        }
        let transaction = self.begin()?;
        let project_text = project_id.to_string();
        let epic_text = epic_id.to_string();
        let expected_intent = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "operation": "jira_materialization_apply",
            "project_id": project_text.as_str(),
            "epic_id": epic_text.as_str(),
            "preview_hash": preview_hash.as_str(),
        }))?;
        let receipt_scope: Option<(String, String, String, Option<String>, String)> = transaction
            .query_row(
                "SELECT receipt.project_id, receipt.kind, target.target_kind,
                        target.target_mini_project_id, receipt.intent_hash
                 FROM command_receipts AS receipt
                 JOIN command_targets AS target
                   ON target.project_id = receipt.project_id
                  AND target.receipt_id = receipt.id
                 WHERE receipt.id = ?1",
                [recovery_receipt_id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if receipt_scope
            .as_ref()
            .map(|(project, kind, target_kind, target_epic, intent_hash)| {
                (
                    project.as_str(),
                    kind.as_str(),
                    target_kind.as_str(),
                    target_epic.as_deref(),
                    intent_hash.as_str(),
                )
            })
            != Some((
                project_text.as_str(),
                "materialize_jira",
                "mini_project",
                Some(epic_text.as_str()),
                expected_intent.hash().as_str(),
            ))
        {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization recovery",
                rule: "the recovery receipt does not authorize this project and operation",
            });
        }

        let mut prior_statement = transaction
            .prepare(
                "SELECT batch_id, item_id, ordinal, requested_key, marker, preview_hash
                 FROM jira_materialization_recoveries
                 WHERE project_id = ?1 AND recovery_receipt_id = ?2
                 ORDER BY ordinal",
            )
            .map_err(backend)?;
        let prior = prior_statement
            .query_map(
                params![project_text, recovery_receipt_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        drop(prior_statement);
        if !prior.is_empty() {
            let exact = prior.len() == recovery.len()
                && prior.iter().zip(recovery).all(
                    |((_, _, ordinal, requested_key, marker, stored_preview), requested)| {
                        u32::try_from(*ordinal).ok() == Some(requested.ordinal)
                            && requested_key == requested.requested_key.as_str()
                            && marker == requested.marker.as_str()
                            && stored_preview == preview_hash.as_str()
                    },
                );
            if !exact {
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization recovery",
                    rule: "the recovery receipt already names another exact item set",
                });
            }
            let mut batch_ids = Vec::new();
            for (original_batch_id, item_id, ..) in &prior {
                let current_batch_id: Option<String> = transaction
                    .query_row(
                        "SELECT batch_id FROM jira_materialization_items
                         WHERE project_id = ?1 AND id = ?2",
                        params![project_text, item_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?;
                if current_batch_id.as_deref() != Some(original_batch_id.as_str()) {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira materialization recovery",
                        rule: "a recovered item no longer belongs to its original batch",
                    });
                }
                if !batch_ids.contains(original_batch_id) {
                    batch_ids.push(original_batch_id.clone());
                }
            }
            let batch_ids = batch_ids
                .iter()
                .map(|batch_id| ExternalId::parse(batch_id).map_err(RepositoryError::from))
                .collect::<RepositoryResult<Vec<_>>>()?;
            let batch_id = batch_ids
                .first()
                .cloned()
                .ok_or(RepositoryError::Conflict {
                    subject: "Jira materialization recovery",
                    rule: "the recovered item set has no batch",
                })?;
            transaction.commit().map_err(backend)?;
            let mut items = Vec::with_capacity(recovery.len());
            for recovered_batch_id in &batch_ids {
                items.extend(self.jira_materialization_items(project_id, recovered_batch_id)?);
            }
            items.sort_by_key(|item| item.ordinal);
            let scope_matches = items.len() == recovery.len()
                && items.iter().zip(recovery).all(|(stored, requested)| {
                    stored.ordinal == requested.ordinal
                        && stored.item_kind == requested.item_kind
                        && stored.task_id == requested.task_id
                        && stored.marker == requested.marker
                });
            if !scope_matches {
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization recovery",
                    rule: "the recovered batch no longer matches its immutable recovery ledger",
                });
            }
            return Ok(Some(RecoveredJiraMaterialization {
                batch_id,
                batch_ids,
                items,
            }));
        }

        let mut batches = transaction
            .prepare(
                "SELECT id FROM jira_materialization_batches
                 WHERE project_id = ?1 AND epic_id = ?2 AND status = 'planned'
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let batch_ids = batches
            .query_map(
                params![project_id.to_string(), epic_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        drop(batches);

        type CandidateItem = (
            String,
            i64,
            String,
            Option<String>,
            String,
            Option<String>,
            String,
            Option<String>,
        );
        let mut matched = Vec::<(String, Vec<CandidateItem>)>::new();
        for batch_id in &batch_ids {
            let mut statement = transaction
                .prepare(
                    "SELECT id, ordinal, item_kind, task_id, intent_kind,
                            requested_key, marker, confirmed_key
                     FROM jira_materialization_items
                     WHERE project_id = ?1 AND batch_id = ?2 ORDER BY ordinal",
                )
                .map_err(backend)?;
            let stored = statement
                .query_map(params![project_id.to_string(), batch_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                })
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            drop(statement);
            let exact_fragment = !stored.is_empty()
                && stored.iter().all(
                    |(
                        _,
                        ordinal,
                        kind,
                        task_id,
                        intent,
                        stored_requested_key,
                        marker,
                        confirmed_key,
                    )| {
                        let Some(requested) = u32::try_from(*ordinal)
                            .ok()
                            .and_then(|ordinal| recovery.get(ordinal as usize))
                        else {
                            return false;
                        };
                        let intent_matches = match JiraIntentKind::parse(intent).ok() {
                            Some(JiraIntentKind::Create) => stored_requested_key.is_none(),
                            Some(JiraIntentKind::Link) => {
                                stored_requested_key.as_deref()
                                    == Some(requested.requested_key.as_str())
                            }
                            None => false,
                        };
                        u32::try_from(*ordinal).ok() == Some(requested.ordinal)
                            && JiraItemKind::parse(kind).ok() == Some(requested.item_kind)
                            && task_id.as_deref().map(TaskId::parse).transpose().ok()
                                == Some(requested.task_id)
                            && intent_matches
                            && marker == requested.marker.as_str()
                            && confirmed_key
                                .as_deref()
                                .is_none_or(|key| key == requested.requested_key.as_str())
                    },
                );
            if exact_fragment {
                matched.push((batch_id.clone(), stored));
            }
        }
        let batch_id = match matched.as_slice() {
            [] if batch_ids.is_empty() => {
                transaction.commit().map_err(backend)?;
                return Ok(None);
            }
            [] => {
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization recovery",
                    rule: "the pending batch set does not exactly match the recovery scope and markers",
                });
            }
            [(batch_id, _)] => ExternalId::parse(batch_id)?,
            [(batch_id, _), ..] => ExternalId::parse(batch_id)?,
        };
        let batch_ids = matched
            .iter()
            .map(|(batch_id, _)| ExternalId::parse(batch_id).map_err(RepositoryError::from))
            .collect::<RepositoryResult<Vec<_>>>()?;

        let mut ordinal_coverage = vec![0_u8; recovery.len()];
        for (_, items) in &matched {
            for (_, ordinal, ..) in items {
                let Some(count) = u32::try_from(*ordinal)
                    .ok()
                    .and_then(|ordinal| ordinal_coverage.get_mut(ordinal as usize))
                else {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira materialization recovery",
                        rule: "a pending fragment carries an ordinal outside the recovery scope",
                    });
                };
                *count = count.saturating_add(1);
            }
        }
        if ordinal_coverage.iter().any(|count| *count != 1) {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization recovery",
                rule: "pending fragments do not form one exact non-overlapping recovery scope",
            });
        }

        for (original_batch_id, items) in &matched {
            for (item_id, ordinal, ..) in items {
                let requested = &recovery[usize::try_from(*ordinal).map_err(|_| {
                    kontor_core::DomainError::invalid(
                        "Jira materialization recovery",
                        "stored a negative item ordinal",
                    )
                })?];
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO jira_materialization_recoveries
                             (project_id, batch_id, item_id, recovery_receipt_id,
                              preview_hash, ordinal, requested_key, marker, recovered_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            project_id.to_string(),
                            original_batch_id,
                            item_id,
                            recovery_receipt_id.to_string(),
                            preview_hash.as_str(),
                            ordinal,
                            requested.requested_key.as_str(),
                            requested.marker.as_str(),
                            format_utc_timestamp(recovered_at),
                        ],
                    )
                    .map_err(backend)?;
                let stored: (String, String, i64, String, String) = transaction
                    .query_row(
                        "SELECT recovery_receipt_id, preview_hash, ordinal, requested_key, marker
                         FROM jira_materialization_recoveries
                         WHERE project_id = ?1 AND batch_id = ?2 AND item_id = ?3",
                        params![project_id.to_string(), original_batch_id, item_id],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                            ))
                        },
                    )
                    .map_err(backend)?;
                if stored
                    != (
                        recovery_receipt_id.to_string(),
                        preview_hash.as_str().to_owned(),
                        i64::from(requested.ordinal),
                        requested.requested_key.as_str().to_owned(),
                        requested.marker.as_str().to_owned(),
                    )
                {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira materialization recovery",
                        rule: "the pending create item already names another recovery",
                    });
                }
            }
        }

        transaction.commit().map_err(backend)?;
        let mut items = Vec::with_capacity(recovery.len());
        for recovered_batch_id in &batch_ids {
            items.extend(self.jira_materialization_items(project_id, recovered_batch_id)?);
        }
        items.sort_by_key(|item| item.ordinal);
        let scope_matches = items.len() == recovery.len()
            && items.iter().zip(recovery).all(|(stored, requested)| {
                stored.ordinal == requested.ordinal
                    && stored.item_kind == requested.item_kind
                    && stored.task_id == requested.task_id
                    && stored.marker == requested.marker
            });
        if !scope_matches {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization recovery",
                rule: "the recovered batch set does not match its immutable recovery ledger",
            });
        }
        Ok(Some(RecoveredJiraMaterialization {
            batch_id,
            batch_ids,
            items,
        }))
    }

    /// Return the externally read-back Jira epic key, never an imported or
    /// merely requested execution-scope value.
    pub fn confirmed_jira_epic_key(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> RepositoryResult<Option<ExternalId>> {
        let key: Option<String> = self
            .connection
            .query_row(
                "SELECT external_issue_key FROM jira_epic_bindings
                 WHERE project_id = ?1 AND epic_id = ?2",
                params![project_id.to_string(), epic_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(key.as_deref().map(ExternalId::parse).transpose()?)
    }

    /// Return the one externally read-back Jira task key.
    ///
    /// Multiple confirmed Jira links for the same task are an ambiguous
    /// identity and are refused instead of selecting one by row order.
    pub fn confirmed_jira_task_key(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<ExternalId>> {
        let (key, count): (Option<String>, i64) = self
            .connection
            .query_row(
                "SELECT MIN(ledger.external_issue_key), COUNT(*)
                 FROM canonical_jira_task_links AS ledger
                 JOIN jira_task_binding_confirmations AS confirmation
                   ON confirmation.project_id = ledger.project_id
                  AND confirmation.link_id = ledger.link_id
                 WHERE ledger.project_id = ?1 AND ledger.task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(backend)?;
        match count {
            0 => Ok(None),
            1 => Ok(Some(ExternalId::parse(key.as_deref().ok_or(
                RepositoryError::Conflict {
                    subject: "confirmed Jira task binding",
                    rule: "the unique confirmed binding has no issue key",
                },
            )?)?)),
            _ => Err(RepositoryError::Conflict {
                subject: "confirmed Jira task binding",
                rule: "more than one confirmed Jira issue is bound to the task",
            }),
        }
    }

    /// Atomically confirm one exact Jira readback and its durable binding.
    pub fn confirm_jira_materialization_item(
        &self,
        item: &StoredJiraMaterializationItem,
        key: &ExternalId,
        readback_hash: &ContentHash,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let stored: Option<(String, Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT status, confirmed_key, readback_hash
                 FROM jira_materialization_items
                 WHERE project_id = ?1 AND id = ?2",
                params![item.project_id.to_string(), item.id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        match stored {
            Some((status, stored_key, stored_hash)) if status == "confirmed" => {
                if stored_key.as_deref() == Some(key.as_str())
                    && stored_hash.as_deref() == Some(readback_hash.as_str())
                {
                    transaction.commit().map_err(backend)?;
                    return Ok(());
                }
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization item",
                    rule: "the item is already confirmed with another Jira readback",
                });
            }
            Some((status, _, _)) if status == "planned" => {}
            Some(_) => {
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization item",
                    rule: "the item is not confirmable in its current state",
                });
            }
            None => {
                return Err(RepositoryError::NotFound {
                    subject: "Jira materialization item",
                });
            }
        }
        match item.item_kind {
            JiraItemKind::Epic => {
                let existing: Option<String> = transaction
                    .query_row(
                        "SELECT external_issue_key FROM jira_epic_bindings
                         WHERE project_id = ?1 AND epic_id = ?2",
                        params![item.project_id.to_string(), item.epic_id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?;
                if existing
                    .as_deref()
                    .is_some_and(|stored| stored != key.as_str())
                {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira epic binding",
                        rule: "the epic already has another confirmed Jira binding",
                    });
                }
                let changed = transaction
                    .execute(
                        "INSERT INTO jira_epic_bindings
                         (project_id, epic_id, external_issue_key, readback_hash, confirmed_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(project_id, epic_id) DO UPDATE SET
                           readback_hash = excluded.readback_hash,
                           confirmed_at = excluded.confirmed_at
                         WHERE jira_epic_bindings.external_issue_key = excluded.external_issue_key",
                        params![
                            item.project_id.to_string(),
                            item.epic_id.to_string(),
                            key.as_str(),
                            readback_hash.as_str(),
                            format_utc_timestamp(confirmed_at),
                        ],
                    )
                    .map_err(backend)?;
                if changed != 1 {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira epic binding",
                        rule: "the epic binding changed during confirmation",
                    });
                }
            }
            JiraItemKind::Task => {
                let task_id = item.task_id.ok_or_else(|| {
                    kontor_core::DomainError::invalid("Jira task item", "stored without a task")
                })?;
                let planned_link_id = item.link_id.ok_or_else(|| {
                    kontor_core::DomainError::invalid(
                        "Jira task item",
                        "stored without a link identity",
                    )
                })?;
                let existing_for_key: Option<(String, String)> = transaction
                    .query_row(
                        "SELECT id, task_id FROM jira_links
                         WHERE project_id = ?1 AND connector = 'connector.jira'
                           AND external_issue_key = ?2",
                        params![item.project_id.to_string(), key.as_str()],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(backend)?;
                let jira_links_for_task: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*) FROM jira_links
                         WHERE project_id = ?1 AND task_id = ?2
                           AND connector = 'connector.jira'",
                        params![item.project_id.to_string(), task_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(backend)?;
                let effective_link_id = match existing_for_key {
                    Some((link_id, linked_task_id))
                        if linked_task_id == task_id.to_string() && jira_links_for_task == 1 =>
                    {
                        link_id
                    }
                    Some(_) => {
                        return Err(RepositoryError::Conflict {
                            subject: "Jira task binding",
                            rule: "the confirmed Jira issue is bound ambiguously or to another task",
                        });
                    }
                    None if jira_links_for_task > 0 => {
                        return Err(RepositoryError::Conflict {
                            subject: "Jira task binding",
                            rule: "the task already has another Jira binding",
                        });
                    }
                    None => {
                        transaction
                            .execute(
                                "INSERT INTO jira_links
                                 (id, project_id, task_id, connector, external_issue_key, revision, created_at)
                                 VALUES (?1, ?2, ?3, 'connector.jira', ?4, 1, ?5)",
                                params![
                                    planned_link_id.to_string(),
                                    item.project_id.to_string(),
                                    task_id.to_string(),
                                    key.as_str(),
                                    format_utc_timestamp(confirmed_at),
                                ],
                            )
                            .map_err(backend)?;
                        planned_link_id.to_string()
                    }
                };
                if effective_link_id != planned_link_id.to_string() {
                    transaction
                        .execute(
                            "UPDATE jira_materialization_items
                             SET link_id = ?1
                             WHERE project_id = ?2 AND id = ?3 AND status = 'planned'
                               AND link_id = ?4",
                            params![
                                effective_link_id,
                                item.project_id.to_string(),
                                item.id.as_str(),
                                planned_link_id.to_string(),
                            ],
                        )
                        .map_err(backend)?;
                }
                transaction
                    .execute(
                        "INSERT INTO canonical_jira_task_links
                             (project_id, task_id, external_issue_key, link_id)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(project_id, task_id) DO NOTHING",
                        params![
                            item.project_id.to_string(),
                            task_id.to_string(),
                            key.as_str(),
                            effective_link_id,
                        ],
                    )
                    .map_err(backend)?;
                let canonical: Option<(String, String)> = transaction
                    .query_row(
                        "SELECT external_issue_key, link_id
                         FROM canonical_jira_task_links
                         WHERE project_id = ?1 AND task_id = ?2",
                        params![item.project_id.to_string(), task_id.to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(backend)?;
                if canonical
                    .as_ref()
                    .map(|(issue, link)| (issue.as_str(), link.as_str()))
                    != Some((key.as_str(), effective_link_id.as_str()))
                {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira task binding",
                        rule: "the confirmed Jira issue contradicts the canonical task-link ledger",
                    });
                }
                let stored_link_id: Option<String> = transaction
                    .query_row(
                        "SELECT link_id FROM jira_materialization_items
                         WHERE project_id = ?1 AND id = ?2",
                        params![item.project_id.to_string(), item.id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?;
                if stored_link_id.as_deref() != Some(effective_link_id.as_str()) {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira task binding",
                        rule: "the durable materialization item names another Jira link",
                    });
                }
                transaction
                    .execute(
                        "INSERT INTO jira_task_binding_confirmations
                         (project_id, link_id, readback_hash, confirmed_at)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(project_id, link_id) DO UPDATE SET
                           readback_hash = excluded.readback_hash,
                           confirmed_at = excluded.confirmed_at",
                        params![
                            item.project_id.to_string(),
                            effective_link_id,
                            readback_hash.as_str(),
                            format_utc_timestamp(confirmed_at),
                        ],
                    )
                    .map_err(backend)?;
            }
        }
        transaction
            .execute(
                "UPDATE jira_materialization_items
                 SET status = 'confirmed', confirmed_key = ?1, readback_hash = ?2, confirmed_at = ?3
                 WHERE project_id = ?4 AND id = ?5
                   AND (status = 'planned' OR (confirmed_key = ?1 AND readback_hash = ?2))",
                params![
                    key.as_str(),
                    readback_hash.as_str(),
                    format_utc_timestamp(confirmed_at),
                    item.project_id.to_string(),
                    item.id.as_str(),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)
    }

    /// Confirm the batch only when every planned item is confirmed.
    pub fn confirm_jira_materialization_batch(
        &self,
        project_id: ProjectId,
        batch_id: &ExternalId,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let remaining: i64 = transaction
            .query_row(
                "SELECT count(*) FROM jira_materialization_items
                 WHERE project_id = ?1 AND batch_id = ?2 AND status <> 'confirmed'",
                params![project_id.to_string(), batch_id.as_str()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if remaining != 0 {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization batch",
                rule: "a Jira materialization batch still has unconfirmed items",
            });
        }
        transaction
            .execute(
                "UPDATE jira_materialization_batches SET status = 'confirmed', confirmed_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                params![
                    format_utc_timestamp(confirmed_at),
                    project_id.to_string(),
                    batch_id.as_str(),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)
    }

    /// Record ASMA activation after the service has proved every binding.
    pub fn activate_asma_epic(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        receipt_id: CommandReceiptId,
        activated_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let task_count: i64 = transaction
            .query_row(
                "SELECT count(*) FROM tasks WHERE project_id = ?1 AND mini_project_id = ?2",
                params![project_id.to_string(), epic_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let confirmed: i64 = transaction
            .query_row(
                "SELECT count(DISTINCT l.task_id)
                 FROM jira_links l JOIN jira_task_binding_confirmations c
                   ON c.project_id = l.project_id AND c.link_id = l.id
                 JOIN tasks t ON t.project_id = l.project_id AND t.id = l.task_id
                 WHERE t.project_id = ?1 AND t.mini_project_id = ?2",
                params![project_id.to_string(), epic_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let epic_bound: i64 = transaction
            .query_row(
                "SELECT count(*) FROM jira_epic_bindings WHERE project_id = ?1 AND epic_id = ?2",
                params![project_id.to_string(), epic_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if epic_bound != 1 || confirmed != task_count {
            return Err(RepositoryError::Conflict {
                subject: "ASMA epic activation",
                rule: "ASMA activation requires one confirmed Jira binding per epic and task",
            });
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO asma_epic_activations
                 (project_id, epic_id, receipt_id, activated_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    project_id.to_string(),
                    epic_id.to_string(),
                    receipt_id.to_string(),
                    format_utc_timestamp(activated_at),
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)
    }

    pub fn asma_epic_is_active(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> RepositoryResult<bool> {
        let transaction = self.begin()?;
        let active = transaction
            .query_row(
                "SELECT 1 FROM asma_epic_activations WHERE project_id = ?1 AND epic_id = ?2",
                params![project_id.to_string(), epic_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(backend)?
            .is_some();
        transaction.commit().map_err(backend)?;
        Ok(active)
    }
}
