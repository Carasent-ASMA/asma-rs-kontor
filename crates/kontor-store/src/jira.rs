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

impl SqliteStore {
    /// Persist a complete plan before its first Jira effect.
    pub fn plan_jira_materialization(
        &self,
        batch: &NewJiraMaterializationBatch,
        items: &[NewJiraMaterializationItem],
    ) -> RepositoryResult<()> {
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
        let stored: (String, String, String) = transaction
            .query_row(
                "SELECT id, epic_id, preview_hash FROM jira_materialization_batches
                 WHERE idempotency_key = ?1",
                [&batch.idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(backend)?;
        if stored
            != (
                batch.id.as_str().to_owned(),
                batch.epic_id.to_string(),
                batch.preview_hash.as_str().to_owned(),
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

    /// Atomically confirm one exact Jira readback and its durable binding.
    pub fn confirm_jira_materialization_item(
        &self,
        item: &StoredJiraMaterializationItem,
        key: &ExternalId,
        readback_hash: &ContentHash,
        confirmed_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        match item.item_kind {
            JiraItemKind::Epic => {
                transaction
                    .execute(
                        "INSERT INTO jira_epic_bindings
                         (project_id, epic_id, external_issue_key, readback_hash, confirmed_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(project_id, epic_id) DO UPDATE SET
                           external_issue_key = excluded.external_issue_key,
                           readback_hash = excluded.readback_hash,
                           confirmed_at = excluded.confirmed_at",
                        params![
                            item.project_id.to_string(),
                            item.epic_id.to_string(),
                            key.as_str(),
                            readback_hash.as_str(),
                            format_utc_timestamp(confirmed_at),
                        ],
                    )
                    .map_err(backend)?;
            }
            JiraItemKind::Task => {
                let task_id = item.task_id.ok_or_else(|| {
                    kontor_core::DomainError::invalid("Jira task item", "stored without a task")
                })?;
                let link_id = item.link_id.ok_or_else(|| {
                    kontor_core::DomainError::invalid(
                        "Jira task item",
                        "stored without a link identity",
                    )
                })?;
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO jira_links
                         (id, project_id, task_id, connector, external_issue_key, revision, created_at)
                         VALUES (?1, ?2, ?3, 'connector.jira', ?4, 1, ?5)",
                        params![
                            link_id.to_string(),
                            item.project_id.to_string(),
                            task_id.to_string(),
                            key.as_str(),
                            format_utc_timestamp(confirmed_at),
                        ],
                    )
                    .map_err(backend)?;
                let linked: Option<String> = transaction
                    .query_row(
                        "SELECT external_issue_key FROM jira_links
                         WHERE project_id = ?1 AND id = ?2 AND task_id = ?3",
                        params![
                            item.project_id.to_string(),
                            link_id.to_string(),
                            task_id.to_string(),
                        ],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?;
                if linked.as_deref() != Some(key.as_str()) {
                    return Err(RepositoryError::Conflict {
                        subject: "Jira task binding",
                        rule: "the confirmed Jira task binding names another issue",
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
                            link_id.to_string(),
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
