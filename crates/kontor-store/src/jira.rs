//! Durable native Jira materialization and activation facts.

#![allow(missing_docs)]

use kontor_core::id::{
    AggregateRevision, CommandReceiptId, ContentHash, ExternalId, MiniProjectId, ProjectId, TaskId,
    TicketLinkId, Timestamp, format_utc_timestamp, parse_utc_timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

use crate::SqliteStore;
use crate::repository::backend;

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

/// The original pending batch selected by a durable non-creating recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredJiraMaterialization {
    pub batch_id: ExternalId,
    pub items: Vec<StoredJiraMaterializationItem>,
}

impl SqliteStore {
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

    /// Adopt one exact pending create batch for a link-only recovery.
    ///
    /// Every original item must still have the same ordinal, kind, task scope
    /// and marker. The requested Jira keys are appended to an immutable ledger
    /// before the connector is contacted; the create plan itself is unchanged.
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
        let receipt_scope: Option<(String, String)> = transaction
            .query_row(
                "SELECT project_id, kind FROM command_receipts WHERE id = ?1",
                [recovery_receipt_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let project_text = project_id.to_string();
        if receipt_scope
            .as_ref()
            .map(|(project, kind)| (project.as_str(), kind.as_str()))
            != Some((project_text.as_str(), "materialize_jira"))
        {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization recovery",
                rule: "the recovery receipt does not authorize this project and operation",
            });
        }

        let mut prior_statement = transaction
            .prepare(
                "SELECT batch_id, ordinal, requested_key, marker
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
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        drop(prior_statement);
        if !prior.is_empty() {
            let batch_id = ExternalId::parse(&prior[0].0)?;
            let exact = prior.len() == recovery.len()
                && prior.iter().zip(recovery).all(
                    |((stored_batch, ordinal, requested_key, marker), requested)| {
                        stored_batch == batch_id.as_str()
                            && u32::try_from(*ordinal).ok() == Some(requested.ordinal)
                            && requested_key == requested.requested_key.as_str()
                            && marker == requested.marker.as_str()
                    },
                );
            if !exact {
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization recovery",
                    rule: "the recovery receipt already names another exact item set",
                });
            }
            transaction.commit().map_err(backend)?;
            let items = self.jira_materialization_items(project_id, &batch_id)?;
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
            return Ok(Some(RecoveredJiraMaterialization { batch_id, items }));
        }

        let mut batches = transaction
            .prepare(
                "SELECT id FROM jira_materialization_batches
                 WHERE project_id = ?1 AND epic_id = ?2 AND status = 'planned'
                   AND EXISTS (
                       SELECT 1 FROM jira_materialization_items AS item
                       WHERE item.batch_id = jira_materialization_batches.id
                         AND item.intent_kind = 'create'
                   )
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

        let mut matched = Vec::<String>::new();
        for batch_id in &batch_ids {
            let mut statement = transaction
                .prepare(
                    "SELECT ordinal, item_kind, task_id, intent_kind, requested_key,
                            marker, confirmed_key
                     FROM jira_materialization_items
                     WHERE project_id = ?1 AND batch_id = ?2 ORDER BY ordinal",
                )
                .map_err(backend)?;
            let stored = statement
                .query_map(params![project_id.to_string(), batch_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                })
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            drop(statement);
            let exact = stored.len() == recovery.len()
                && stored.iter().zip(recovery).all(
                    |(
                        (
                            ordinal,
                            kind,
                            task_id,
                            intent,
                            stored_requested_key,
                            marker,
                            confirmed_key,
                        ),
                        requested,
                    )| {
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
            if exact {
                matched.push(batch_id.clone());
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
                    rule: "the pending create batch does not exactly match the recovery scope and markers",
                });
            }
            [batch_id] => ExternalId::parse(batch_id)?,
            _ => {
                return Err(RepositoryError::Conflict {
                    subject: "Jira materialization recovery",
                    rule: "more than one pending create batch matches the recovery scope",
                });
            }
        };

        let mut item_ids = transaction
            .prepare(
                "SELECT id, ordinal FROM jira_materialization_items
                 WHERE project_id = ?1 AND batch_id = ?2 ORDER BY ordinal",
            )
            .map_err(backend)?;
        let stored_ids = item_ids
            .query_map(params![project_id.to_string(), batch_id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        drop(item_ids);
        if stored_ids.len() != recovery.len() {
            return Err(RepositoryError::Conflict {
                subject: "Jira materialization recovery",
                rule: "the selected batch item set changed before recovery was recorded",
            });
        }
        for ((item_id, ordinal), requested) in stored_ids.iter().zip(recovery) {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO jira_materialization_recoveries
                         (project_id, batch_id, item_id, recovery_receipt_id,
                          preview_hash, ordinal, requested_key, marker, recovered_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        project_id.to_string(),
                        batch_id.as_str(),
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
                    params![project_id.to_string(), batch_id.as_str(), item_id],
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
        transaction.commit().map_err(backend)?;
        let items = self.jira_materialization_items(project_id, &batch_id)?;
        Ok(Some(RecoveredJiraMaterialization { batch_id, items }))
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
                "SELECT MIN(link.external_issue_key), COUNT(*)
                 FROM jira_links AS link
                 JOIN jira_task_binding_confirmations AS confirmation
                   ON confirmation.project_id = link.project_id
                  AND confirmation.link_id = link.id
                 WHERE link.project_id = ?1 AND link.task_id = ?2
                   AND link.connector = 'connector.jira'",
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
