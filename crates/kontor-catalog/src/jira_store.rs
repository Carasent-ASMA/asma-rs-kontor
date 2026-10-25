//! Jira evidence storage in the catalog's own database.
//!
//! It shares the catalog connection but not the local discovery generation.
//! Pages are staged, a complete sweep promotes them, and only a complete
//! relevant census may write a tombstone. An incomplete sweep inserts a
//! previously unknown issue hidden and never updates or removes an existing
//! fact.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::contract::HOST_SCOPE;
use crate::jira_contract::{
    JiraAlias, JiraCollectionCoverage, JiraCollectionState, JiraDanglingAssociation,
    JiraIssueObservation, JiraObservationState, JiraObservationVia, JiraSweepMode,
};
use crate::store::{Database, StoreError, bounded, generation_to_sql};

type JiraCollectionRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<String>,
);

type JiraEvidenceCollectionRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The collector identity recorded in the durable evidence view.
pub const JIRA_COLLECTOR: &str = "kontor-catalog@1";
/// The adapter identity recorded in the durable evidence view.
pub const JIRA_ADAPTER: &str = "kontor-jira-read-only@1";

/// One staged issue observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedJiraIssue {
    /// Jira's immutable REST id.
    pub id: String,
    /// The current key.
    pub key: String,
    /// The actual issue type name.
    #[serde(rename = "type")]
    pub issue_type: String,
    /// The actual hierarchy level.
    pub level: i64,
    /// The parent's current key.
    pub parent: Option<String>,
    /// The parent's immutable id.
    pub parent_id: Option<String>,
    /// Every label, sorted.
    pub labels: Vec<String>,
    /// The status name.
    pub status: String,
    /// The status category key.
    pub cat: String,
    /// The source update instant, canonical UTC.
    pub updated: String,
    /// The summary.
    pub summary: String,
    /// When the observation was received.
    pub observed_at: String,
    /// How the issue entered the observation.
    pub via: JiraObservationVia,
}

impl ObservedJiraIssue {
    /// Convert a stored observation into the staged shape.
    #[must_use]
    pub fn from_observation(issue: &JiraIssueObservation) -> Self {
        Self {
            id: issue.id.clone(),
            key: issue.key.clone(),
            issue_type: issue.issue_type.clone(),
            level: issue.level,
            parent: issue.parent.clone(),
            parent_id: issue.parent_id.clone(),
            labels: issue.labels.clone(),
            status: issue.status.clone(),
            cat: issue.cat.clone(),
            updated: issue.updated.clone(),
            summary: issue.summary.clone(),
            observed_at: issue.observed_at.clone(),
            via: issue.via,
        }
    }
}

/// One in-flight Jira sweep, which is also its fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraSweepId {
    /// The attempt generation.
    pub generation: u64,
    /// The sweep mode.
    pub mode: JiraSweepMode,
    /// The monotonic projection order.
    pub order: u64,
    /// The incremental window start, when one applies.
    pub window_start: Option<String>,
}

/// A fact a complete relevant census proved is no longer in scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraRemoval {
    /// The immutable issue id.
    pub issue_id: String,
    /// The structural reason.
    pub reason: String,
}

/// The final facts one Jira sweep commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraSweepOutcome {
    /// Every request needed by this mode succeeded.
    pub complete: bool,
    /// The sweep could not start or lost credentials/permission.
    pub failed: bool,
    /// This sweep was a complete relevant census and may remove facts.
    pub census: bool,
    /// Honest coverage.
    pub coverage: JiraCollectionCoverage,
    /// Structural diagnostics.
    pub diagnostics: Vec<String>,
    /// Aliases observed by this sweep.
    pub aliases: Vec<JiraAlias>,
    /// Facts a complete relevant census proved gone.
    pub removals: Vec<JiraRemoval>,
    /// When the next full sweep is due.
    pub next_full_sweep_at: Option<String>,
    /// Issues observed.
    pub issues_seen: usize,
    /// Pages committed.
    pub pages: usize,
    /// Requests attempted.
    pub requests: usize,
    /// Retries performed.
    pub retries: usize,
    /// Whether permission loss stopped the sweep.
    pub permission_loss: bool,
    /// Whether the credential was unavailable.
    pub credential_failure: bool,
    /// The absolute retry deadline, when one applies.
    pub backoff_until: Option<String>,
    /// The failure class, when the sweep failed.
    pub failure: Option<crate::JiraReadFailure>,
    /// A structural error.
    pub error: Option<String>,
}

/// The scheduling and identity checkpoint for Jira collection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JiraCheckpoint {
    /// The last attempted generation.
    pub generation: u64,
    /// The last complete generation.
    pub active_generation: u64,
    /// The last projection order.
    pub last_order: u64,
    /// The latest state.
    pub state: JiraCollectionState,
    /// The latest mode.
    pub mode: Option<JiraSweepMode>,
    /// When the last complete sweep finished.
    pub last_complete_at: Option<String>,
    /// When the last complete full sweep finished.
    pub last_full_sweep_at: Option<String>,
    /// When the next full sweep is due.
    pub next_full_sweep_at: Option<String>,
    /// The last complete projection order.
    pub order_value: u64,
    /// Whether the full-sweep deadline has passed.
    pub overdue: bool,
    /// The retry deadline Jira named, when one applies.
    pub backoff_until: Option<String>,
}

/// The generation-consistent Jira read.
#[derive(Debug, Clone)]
pub struct JiraEvidenceRead {
    /// The active complete generation.
    pub generation: u64,
    /// The projection order of that generation.
    pub order: u64,
    /// When that generation completed.
    pub as_of: Option<String>,
    /// The active complete generation's coverage.
    pub coverage: Option<JiraCollectionCoverage>,
    /// The active complete generation's mode.
    pub mode: Option<JiraSweepMode>,
    /// The latest attempt's state, which may be later than the active generation.
    pub latest_state: JiraCollectionState,
    /// The latest attempt's structural error, when it failed.
    pub latest_error: Option<String>,
    /// The latest attempt's failure class.
    pub latest_failure: Option<crate::JiraReadFailure>,
    /// Every observation visible at the active generation.
    pub issues: Vec<JiraIssueObservation>,
    /// Every alias visible at the active generation.
    pub aliases: Vec<JiraAlias>,
    /// Latest diagnostics.
    pub diagnostics: Vec<String>,
    /// When the last complete full sweep finished.
    pub last_full_sweep_at: Option<String>,
    /// When the next full sweep is due.
    pub next_full_sweep_at: Option<String>,
    /// Whether the full-sweep deadline has passed.
    pub overdue: bool,
    /// The retry deadline Jira named, when one applies.
    pub backoff_until: Option<String>,
}

impl Database {
    /// Begin one Jira sweep, allocating the next independent generation.
    ///
    /// # Errors
    /// [`StoreError::NotOwner`] when this process no longer holds the catalog
    /// lease, or a SQL refusal.
    pub fn begin_jira_sweep(
        &self,
        owner_id: &str,
        mode: JiraSweepMode,
        window_start: Option<&Timestamp>,
        now: &Timestamp,
    ) -> Result<JiraSweepId, StoreError> {
        let now_text = now.to_string();
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current_owner, lease_until): (Option<String>, Option<String>) = transaction
            .query_row(
                "SELECT owner_id, lease_until FROM catalog_scope WHERE scope = ?1",
                [HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, None));
        if current_owner.as_deref() != Some(owner_id)
            || !lease_until.is_some_and(|until| until > now_text)
        {
            return Err(StoreError::NotOwner);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_jira_scope
                 (scope, generation, active_generation, last_order, updated_at)
             VALUES (?1, 0, 0, 0, ?2)",
            params![HOST_SCOPE, now_text],
        )?;
        let (generation, last_order): (i64, i64) = transaction.query_row(
            "SELECT generation, last_order FROM catalog_jira_scope WHERE scope = ?1",
            [HOST_SCOPE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let generation = u64::try_from(generation)
            .map_err(|_| StoreError::Corrupt("negative Jira generation".to_owned()))?
            + 1;
        let last_order = u64::try_from(last_order)
            .map_err(|_| StoreError::Corrupt("negative Jira order".to_owned()))?;
        let clock_order = u64::try_from(now.as_millisecond()).unwrap_or(0);
        let order = last_order.saturating_add(1).max(clock_order).max(1);
        transaction.execute(
            "INSERT INTO catalog_jira_sweeps
                 (scope, generation, mode, state, started_at, window_start, window_end,
                  order_value, pages_committed, issues_seen)
             VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?4, ?6, 0, 0)",
            params![
                HOST_SCOPE,
                generation_to_sql(generation),
                mode.as_str(),
                now_text,
                window_start.map(ToString::to_string),
                generation_to_sql(order)
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_jira_scope
             SET generation = ?1, last_order = ?2, updated_at = ?3
             WHERE scope = ?4",
            params![
                generation_to_sql(generation),
                generation_to_sql(order),
                now_text,
                HOST_SCOPE
            ],
        )?;
        transaction.commit()?;
        Ok(JiraSweepId {
            generation,
            mode,
            order,
            window_start: window_start.map(ToString::to_string),
        })
    }

    /// Commit one page of staged observations.
    ///
    /// # Errors
    /// [`StoreError::Fenced`] when a newer sweep has begun.
    pub fn commit_jira_page(
        &self,
        sweep: &JiraSweepId,
        page_index: u32,
        observed_at: &Timestamp,
        issues: &[ObservedJiraIssue],
    ) -> Result<(), StoreError> {
        let issues_json = serde_json::to_string(issues)?;
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<(String, i64)> = transaction
            .query_row(
                "SELECT s.state, j.generation
                 FROM catalog_jira_sweeps s
                 JOIN catalog_jira_scope j ON j.scope = s.scope
                 WHERE s.scope = ?1 AND s.generation = ?2",
                params![HOST_SCOPE, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match state {
            Some((state, current))
                if state == "running" && current == generation_to_sql(sweep.generation) => {}
            Some(_) => return Err(StoreError::Fenced),
            None => return Err(StoreError::MissingSweep),
        }
        transaction.execute(
            "INSERT INTO catalog_jira_sweep_pages
                 (scope, generation, page_index, observed_at, observations_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (scope, generation, page_index) DO UPDATE SET
                 observed_at = excluded.observed_at,
                 observations_json = excluded.observations_json",
            params![
                HOST_SCOPE,
                generation_to_sql(sweep.generation),
                i64::from(page_index),
                observed_at.to_string(),
                issues_json
            ],
        )?;
        transaction.execute(
            "UPDATE catalog_jira_sweeps
             SET pages_committed = (SELECT COUNT(*) FROM catalog_jira_sweep_pages
                                    WHERE scope = ?1 AND generation = ?2),
                 issues_seen = (SELECT COALESCE(SUM(json_array_length(observations_json)), 0)
                                FROM catalog_jira_sweep_pages
                                WHERE scope = ?1 AND generation = ?2)
             WHERE scope = ?1 AND generation = ?2",
            params![HOST_SCOPE, generation_to_sql(sweep.generation)],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Finish one Jira sweep and commit its checkpoint.
    ///
    /// A complete sweep promotes every staged page. A complete **and** census
    /// sweep additionally writes tombstones for the facts it proved gone. An
    /// incomplete or failed sweep inserts only previously unknown rows, hidden
    /// from readers, and never updates or removes an existing fact.
    ///
    /// # Errors
    /// [`StoreError::Fenced`], [`StoreError::MissingSweep`] or a SQL/JSON
    /// refusal.
    pub fn finalize_jira_sweep(
        &self,
        sweep: &JiraSweepId,
        outcome: &JiraSweepOutcome,
        now: &Timestamp,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: Option<(String, i64)> = transaction
            .query_row(
                "SELECT s.state, j.generation
                 FROM catalog_jira_sweeps s
                 JOIN catalog_jira_scope j ON j.scope = s.scope
                 WHERE s.scope = ?1 AND s.generation = ?2",
                params![HOST_SCOPE, generation_to_sql(sweep.generation)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match state {
            Some((state, current))
                if state == "running" && current == generation_to_sql(sweep.generation) => {}
            Some(_) => return Err(StoreError::Fenced),
            None => return Err(StoreError::MissingSweep),
        }
        let mut staged = Vec::new();
        {
            let mut statement = transaction.prepare(
                "SELECT observations_json FROM catalog_jira_sweep_pages
                 WHERE scope = ?1 AND generation = ?2 ORDER BY page_index",
            )?;
            let rows = statement.query_map(
                params![HOST_SCOPE, generation_to_sql(sweep.generation)],
                |row| row.get::<_, String>(0),
            )?;
            for row in rows {
                let json = row?;
                let issues: Vec<ObservedJiraIssue> = serde_json::from_str(&json)
                    .map_err(|error| StoreError::Corrupt(error.to_string()))?;
                staged.extend(issues);
            }
        }
        if outcome.complete {
            promote_jira_complete(
                &transaction,
                sweep.generation,
                now,
                &staged,
                &outcome.aliases,
                &outcome.removals,
                outcome.census,
            )?;
            transaction.execute(
                "UPDATE catalog_jira_scope
                 SET active_generation = ?1, updated_at = ?2
                 WHERE scope = ?3",
                params![
                    generation_to_sql(sweep.generation),
                    now.to_string(),
                    HOST_SCOPE
                ],
            )?;
        } else {
            stage_jira_incomplete(&transaction, sweep.generation, now, &staged)?;
        }
        let state_text = if outcome.failed {
            "failed"
        } else if outcome.complete {
            "complete"
        } else {
            "incomplete"
        };
        let coverage_json = serde_json::to_string(&outcome.coverage)?;
        let diagnostics_json = serde_json::to_string(&outcome.diagnostics)?;
        transaction.execute(
            "UPDATE catalog_jira_sweeps
             SET state = ?1, finished_at = ?2, error_kind = ?3, error = ?4,
                 coverage_json = ?5, diagnostics_json = ?6,
                 pages_committed = (SELECT COUNT(*) FROM catalog_jira_sweep_pages
                                    WHERE scope = ?7 AND generation = ?8),
                 issues_seen = ?9, requests = ?10, retries = ?11
             WHERE scope = ?7 AND generation = ?8",
            params![
                state_text,
                now.to_string(),
                outcome.failure.map(|failure| failure.as_str()),
                outcome.error.as_deref().map(bounded),
                coverage_json,
                diagnostics_json,
                HOST_SCOPE,
                generation_to_sql(sweep.generation),
                i64::try_from(outcome.issues_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.requests).unwrap_or(i64::MAX),
                i64::try_from(outcome.retries).unwrap_or(i64::MAX),
            ],
        )?;
        transaction.execute(
            "INSERT INTO catalog_jira_collection
                 (scope, generation, attempt_generation, state, mode, started_at, finished_at,
                  last_complete_at, last_full_sweep_at, next_full_sweep_at, order_value,
                  pages, issues_seen, requests, retries, permission_loss, credential_failure,
                  backoff_until, overdue, coverage_json, diagnostics_json, error_kind, error)
             VALUES (?1,
                     CASE WHEN ?2 = 1 THEN ?3 ELSE 0 END,
                     ?3, ?4, ?5, ?6, ?7,
                     CASE WHEN ?2 = 1 THEN ?7 ELSE NULL END,
                     CASE WHEN ?2 = 1 AND ?5 = 'full' THEN ?7 ELSE NULL END,
                     ?8, CASE WHEN ?2 = 1 THEN ?9 ELSE 0 END,
                     ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
             ON CONFLICT (scope) DO UPDATE SET
                 generation = CASE WHEN ?2 = 1 THEN ?3 ELSE catalog_jira_collection.generation END,
                 attempt_generation = ?3,
                 state = ?4,
                 mode = ?5,
                 started_at = ?6,
                 finished_at = ?7,
                 last_complete_at = CASE WHEN ?2 = 1 THEN ?7
                                         ELSE catalog_jira_collection.last_complete_at END,
                 last_full_sweep_at = CASE WHEN ?2 = 1 AND ?5 = 'full' THEN ?7
                                           ELSE catalog_jira_collection.last_full_sweep_at END,
                 next_full_sweep_at = COALESCE(?8, catalog_jira_collection.next_full_sweep_at),
                 order_value = CASE WHEN ?2 = 1 THEN ?9
                                    ELSE catalog_jira_collection.order_value END,
                 pages = ?10,
                 issues_seen = ?11,
                 requests = ?12,
                 retries = ?13,
                 permission_loss = ?14,
                 credential_failure = ?15,
                 backoff_until = ?16,
                 overdue = ?17,
                 coverage_json = ?18,
                 diagnostics_json = ?19,
                 error_kind = ?20,
                 error = ?21",
            params![
                HOST_SCOPE,
                if outcome.complete { 1 } else { 0 },
                generation_to_sql(sweep.generation),
                state_text,
                sweep.mode.as_str(),
                transaction
                    .query_row(
                        "SELECT started_at FROM catalog_jira_sweeps
                         WHERE scope = ?1 AND generation = ?2",
                        params![HOST_SCOPE, generation_to_sql(sweep.generation)],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .unwrap_or_else(|| now.to_string()),
                now.to_string(),
                outcome.next_full_sweep_at,
                generation_to_sql(sweep.order),
                i64::try_from(outcome.pages).unwrap_or(i64::MAX),
                i64::try_from(outcome.issues_seen).unwrap_or(i64::MAX),
                i64::try_from(outcome.requests).unwrap_or(i64::MAX),
                i64::try_from(outcome.retries).unwrap_or(i64::MAX),
                i64::from(outcome.permission_loss),
                i64::from(outcome.credential_failure),
                outcome.backoff_until,
                i64::from(outcome.coverage.overdue),
                serde_json::to_string(&outcome.coverage)?,
                serde_json::to_string(&outcome.diagnostics)?,
                outcome.failure.map(|failure| failure.as_str()),
                outcome.error.as_deref().map(bounded),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// The current Jira scheduling checkpoint.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn jira_checkpoint(&self) -> Result<JiraCheckpoint, StoreError> {
        let connection = self.lock();
        let scope: Option<(i64, i64, i64)> = connection
            .query_row(
                "SELECT generation, active_generation, last_order
                 FROM catalog_jira_scope WHERE scope = ?1",
                [HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((generation, active_generation, last_order)) = scope else {
            return Ok(JiraCheckpoint::default());
        };
        let collection: Option<JiraCollectionRow> = connection
            .query_row(
                "SELECT state, mode, last_complete_at, last_full_sweep_at,
                        next_full_sweep_at, overdue, order_value, backoff_until
                 FROM catalog_jira_collection WHERE scope = ?1",
                [HOST_SCOPE],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        let (
            state,
            mode,
            last_complete_at,
            last_full_sweep_at,
            next_full_sweep_at,
            overdue,
            order_value,
            backoff_until,
        ) = collection
            .map(|row| {
                (
                    JiraCollectionState::parse(&row.0).unwrap_or(JiraCollectionState::Incomplete),
                    match row.1.as_str() {
                        "full" => Some(JiraSweepMode::Full),
                        "incremental" => Some(JiraSweepMode::Incremental),
                        _ => None,
                    },
                    row.2,
                    row.3,
                    row.4,
                    row.5 != 0,
                    u64::try_from(row.6).unwrap_or(0),
                    row.7,
                )
            })
            .unwrap_or((
                JiraCollectionState::NeverCollected,
                None,
                None,
                None,
                None,
                false,
                0,
                None,
            ));
        Ok(JiraCheckpoint {
            generation: u64::try_from(generation).unwrap_or(0),
            active_generation: u64::try_from(active_generation).unwrap_or(0),
            last_order: u64::try_from(last_order).unwrap_or(0),
            state,
            mode,
            last_complete_at,
            last_full_sweep_at,
            next_full_sweep_at,
            order_value,
            overdue,
            backoff_until,
        })
    }

    /// Read every observation and alias visible at the active generation.
    ///
    /// # Errors
    /// SQLite refused or stored evidence is corrupt.
    pub fn read_jira_evidence(
        &self,
        include_removed: bool,
    ) -> Result<JiraEvidenceRead, StoreError> {
        let connection = self.lock();
        let scope: Option<(i64, i64)> = connection
            .query_row(
                "SELECT active_generation, last_order
                 FROM catalog_jira_scope WHERE scope = ?1",
                [HOST_SCOPE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((active, _last_order)) = scope else {
            return Ok(empty_evidence());
        };
        let active = u64::try_from(active).unwrap_or(0);
        if active == 0 {
            return Ok(empty_evidence());
        }
        let active_sweep: Option<(String, i64, String, String)> = connection
            .query_row(
                "SELECT coverage_json, order_value, finished_at, mode
                 FROM catalog_jira_sweeps
                 WHERE scope = ?1 AND generation = ?2 AND state = 'complete'",
                params![HOST_SCOPE, generation_to_sql(active)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((coverage_json, order, as_of, mode)) = active_sweep else {
            return Ok(empty_evidence());
        };
        let collection: Option<JiraEvidenceCollectionRow> = connection
            .query_row(
                "SELECT state, last_complete_at, last_full_sweep_at,
                        next_full_sweep_at, overdue, error, error_kind, backoff_until
                 FROM catalog_jira_collection WHERE scope = ?1",
                [HOST_SCOPE],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        let (
            latest_state,
            _last_complete_at,
            last_full_sweep_at,
            next_full_sweep_at,
            overdue,
            latest_error,
            latest_failure,
            backoff_until,
        ) = collection
            .map(|row| {
                (
                    JiraCollectionState::parse(&row.0).unwrap_or(JiraCollectionState::Incomplete),
                    row.1,
                    row.2,
                    row.3,
                    row.4 != 0,
                    row.5,
                    row.6.as_deref().and_then(parse_failure),
                    row.7,
                )
            })
            .unwrap_or((
                JiraCollectionState::NeverCollected,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
            ));
        let coverage = serde_json::from_str::<JiraCollectionCoverage>(&coverage_json).ok();
        let diagnostics = coverage
            .as_ref()
            .map(|coverage| coverage.diagnostics.clone())
            .unwrap_or_default();
        let mode = match mode.as_str() {
            "full" => Some(JiraSweepMode::Full),
            "incremental" => Some(JiraSweepMode::Incremental),
            _ => None,
        };
        let mut issues = Vec::new();
        {
            let mut statement = connection.prepare(
                "SELECT issue_id, current_key, issue_type, hierarchy_level, parent_key,
                        parent_id, labels_json, status, status_category, updated_at,
                        summary, observed_at, via, first_seen, last_seen, state,
                        removed_reason
                 FROM catalog_jira_observations
                 WHERE scope = ?1 AND visible_generation = ?2
                 ORDER BY current_key",
            )?;
            let rows =
                statement.query_map(params![HOST_SCOPE, generation_to_sql(active)], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                        row.get::<_, String>(14)?,
                        row.get::<_, String>(15)?,
                        row.get::<_, Option<String>>(16)?,
                    ))
                })?;
            for row in rows {
                let row = row?;
                let state = match row.15.as_str() {
                    "active" => JiraObservationState::Active,
                    "removed" => JiraObservationState::Removed,
                    _ => {
                        return Err(StoreError::Corrupt(
                            "an unknown Jira observation state was stored".to_owned(),
                        ));
                    }
                };
                if state == JiraObservationState::Removed && !include_removed {
                    continue;
                }
                let labels: Vec<String> = serde_json::from_str(&row.6)
                    .map_err(|error| StoreError::Corrupt(error.to_string()))?;
                let via = JiraObservationVia::parse(&row.12).ok_or_else(|| {
                    StoreError::Corrupt("an unknown Jira observation via was stored".to_owned())
                })?;
                issues.push(JiraIssueObservation {
                    id: row.0,
                    key: row.1,
                    issue_type: row.2,
                    level: row.3,
                    parent: row.4,
                    parent_id: row.5,
                    labels,
                    status: row.7,
                    cat: row.8,
                    updated: row.9,
                    summary: row.10,
                    observed_at: row.11,
                    via,
                    state,
                    first_seen: row.13,
                    last_seen: row.14,
                    removed_reason: row.16,
                });
            }
        }
        let mut aliases = Vec::new();
        {
            let mut statement = connection.prepare(
                "SELECT issue_id, alias_key, current_key, last_seen
                 FROM catalog_jira_aliases
                 WHERE scope = ?1 AND visible_generation = ?2
                 ORDER BY alias_key",
            )?;
            let rows =
                statement.query_map(params![HOST_SCOPE, generation_to_sql(active)], |row| {
                    Ok(JiraAlias {
                        issue_id: row.get(0)?,
                        old_key: row.get(1)?,
                        current_key: row.get(2)?,
                        observed_at: row.get(3)?,
                    })
                })?;
            for row in rows {
                aliases.push(row?);
            }
        }
        Ok(JiraEvidenceRead {
            generation: active,
            order: u64::try_from(order).unwrap_or(0),
            as_of: Some(as_of),
            coverage,
            mode,
            latest_state,
            latest_error,
            latest_failure,
            issues,
            aliases,
            diagnostics,
            last_full_sweep_at,
            next_full_sweep_at,
            overdue,
            backoff_until,
        })
    }

    /// Explicit catalog epic associations that currently name no observed epic.
    ///
    /// The association rows are read, never rewritten: a Jira outage or a
    /// tombstone leaves the operator's declaration dangling and visible.
    ///
    /// # Errors
    /// SQLite refused.
    pub fn read_jira_dangling_associations(
        &self,
        observed_epic_keys: &BTreeSet<String>,
    ) -> Result<Vec<JiraDanglingAssociation>, StoreError> {
        let connection = self.lock();
        let mut associations = Vec::new();
        {
            let mut statement = connection.prepare(
                "SELECT logical_plan_id, value, last_seen
                 FROM catalog_associations
                 WHERE relation = 'epic'
                 ORDER BY value, logical_plan_id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (logical_plan_id, epic, last_seen) = row?;
                if observed_epic_keys.contains(&epic) {
                    continue;
                }
                associations.push(JiraDanglingAssociation {
                    logical_plan_id,
                    workstream: None,
                    epic: Some(epic),
                    last_seen,
                });
            }
        }
        Ok(associations)
    }
}

fn parse_failure(text: &str) -> Option<crate::JiraReadFailure> {
    Some(match text {
        "credential" => crate::JiraReadFailure::Credential,
        "permission" => crate::JiraReadFailure::Permission,
        "not_found" => crate::JiraReadFailure::NotFound,
        "rate_limited" => crate::JiraReadFailure::RateLimited,
        "timeout" => crate::JiraReadFailure::Timeout,
        "transport" => crate::JiraReadFailure::Transport,
        "oversized" => crate::JiraReadFailure::Oversized,
        "malformed" => crate::JiraReadFailure::Malformed,
        "rejected" => crate::JiraReadFailure::Rejected,
        "bounded" => crate::JiraReadFailure::Bounded,
        "configuration" => crate::JiraReadFailure::Configuration,
        _ => return None,
    })
}

fn empty_evidence() -> JiraEvidenceRead {
    JiraEvidenceRead {
        generation: 0,
        order: 0,
        as_of: None,
        coverage: None,
        mode: None,
        latest_state: JiraCollectionState::NeverCollected,
        latest_error: None,
        latest_failure: None,
        issues: Vec::new(),
        aliases: Vec::new(),
        diagnostics: Vec::new(),
        last_full_sweep_at: None,
        next_full_sweep_at: None,
        overdue: false,
        backoff_until: None,
    }
}

struct PreviousJiraObservation {
    updated: String,
}

fn promote_jira_complete(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    staged: &[ObservedJiraIssue],
    aliases: &[JiraAlias],
    removals: &[JiraRemoval],
    census: bool,
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    let active: i64 = transaction.query_row(
        "SELECT active_generation FROM catalog_jira_scope WHERE scope = ?1",
        [HOST_SCOPE],
        |row| row.get(0),
    )?;
    let active = u64::try_from(active)
        .map_err(|_| StoreError::Corrupt("negative Jira active generation".to_owned()))?;
    let mut previous = BTreeMap::new();
    {
        let mut statement = transaction.prepare(
            "SELECT issue_id, updated_at
             FROM catalog_jira_observations
             WHERE scope = ?1",
        )?;
        let rows = statement.query_map([HOST_SCOPE], |row| {
            Ok((
                row.get::<_, String>(0)?,
                PreviousJiraObservation {
                    updated: row.get(1)?,
                },
            ))
        })?;
        for row in rows {
            let (id, observation) = row?;
            previous.insert(id, observation);
        }
    }
    let staged_ids: BTreeSet<&str> = staged.iter().map(|issue| issue.id.as_str()).collect();
    for issue in staged {
        let labels_json = serde_json::to_string(&issue.labels)?;
        if let Some(existing) = previous.get(&issue.id) {
            let stale = is_stale(&existing.updated, &issue.updated);
            if stale {
                // A late or repeated answer may prove presence, but it may not
                // overwrite newer fields with older ones.
                transaction.execute(
                    "UPDATE catalog_jira_observations
                     SET last_seen = ?1, last_seen_generation = ?2,
                         visible_generation = ?2, state = 'active',
                         removed_at = NULL, removed_reason = NULL
                     WHERE scope = ?3 AND issue_id = ?4",
                    params![
                        observed_at,
                        generation_to_sql(generation),
                        HOST_SCOPE,
                        issue.id
                    ],
                )?;
            } else {
                transaction.execute(
                    "UPDATE catalog_jira_observations
                     SET current_key = ?1, issue_type = ?2, hierarchy_level = ?3,
                         parent_key = ?4, parent_id = ?5, labels_json = ?6,
                         status = ?7, status_category = ?8, updated_at = ?9,
                         summary = ?10, observed_at = ?11, via = ?12,
                         last_seen = ?13, last_seen_generation = ?14,
                         visible_generation = ?14, state = 'active',
                         removed_at = NULL, removed_reason = NULL
                     WHERE scope = ?15 AND issue_id = ?16",
                    params![
                        issue.key,
                        issue.issue_type,
                        issue.level,
                        issue.parent,
                        issue.parent_id,
                        labels_json,
                        issue.status,
                        issue.cat,
                        issue.updated,
                        issue.summary,
                        issue.observed_at,
                        issue.via.as_str(),
                        observed_at,
                        generation_to_sql(generation),
                        HOST_SCOPE,
                        issue.id
                    ],
                )?;
            }
        } else {
            transaction.execute(
                "INSERT INTO catalog_jira_observations
                     (scope, issue_id, current_key, issue_type, hierarchy_level, parent_key,
                      parent_id, labels_json, status, status_category, updated_at, summary,
                      observed_at, via, first_seen, last_seen, first_seen_generation,
                      last_seen_generation, visible_generation, state, removed_at,
                      removed_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, ?15, ?16, ?16, ?16, 'active', NULL, NULL)",
                params![
                    HOST_SCOPE,
                    issue.id,
                    issue.key,
                    issue.issue_type,
                    issue.level,
                    issue.parent,
                    issue.parent_id,
                    labels_json,
                    issue.status,
                    issue.cat,
                    issue.updated,
                    issue.summary,
                    issue.observed_at,
                    issue.via.as_str(),
                    observed_at,
                    generation_to_sql(generation)
                ],
            )?;
        }
    }
    // Carry every previous row forward so a fact whose key Jira answered 404
    // for stays visible instead of vanishing with the old generation.
    transaction.execute(
        "UPDATE catalog_jira_observations
         SET visible_generation = ?1
         WHERE scope = ?2 AND visible_generation = ?3",
        params![
            generation_to_sql(generation),
            HOST_SCOPE,
            generation_to_sql(active)
        ],
    )?;
    if census {
        for removal in removals {
            if staged_ids.contains(removal.issue_id.as_str()) {
                continue;
            }
            transaction.execute(
                "UPDATE catalog_jira_observations
                 SET state = 'removed', removed_at = ?1, removed_reason = ?2,
                     visible_generation = ?3, last_seen_generation = ?3
                 WHERE scope = ?4 AND issue_id = ?5",
                params![
                    observed_at,
                    removal.reason,
                    generation_to_sql(generation),
                    HOST_SCOPE,
                    removal.issue_id
                ],
            )?;
        }
    }
    // Carry aliases forward, then add this sweep's moves.
    transaction.execute(
        "UPDATE catalog_jira_aliases
         SET visible_generation = ?1
         WHERE scope = ?2 AND visible_generation = ?3",
        params![
            generation_to_sql(generation),
            HOST_SCOPE,
            generation_to_sql(active)
        ],
    )?;
    for alias in aliases {
        transaction.execute(
            "INSERT INTO catalog_jira_aliases
                 (scope, alias_key, issue_id, current_key, first_seen, last_seen,
                  visible_generation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
             ON CONFLICT (scope, alias_key) DO UPDATE SET
                 issue_id = excluded.issue_id,
                 current_key = excluded.current_key,
                 last_seen = excluded.last_seen,
                 visible_generation = excluded.visible_generation",
            params![
                HOST_SCOPE,
                alias.old_key,
                alias.issue_id,
                alias.current_key,
                alias.observed_at,
                generation_to_sql(generation)
            ],
        )?;
    }
    Ok(())
}

fn stage_jira_incomplete(
    transaction: &rusqlite::Transaction<'_>,
    generation: u64,
    now: &Timestamp,
    staged: &[ObservedJiraIssue],
) -> Result<(), StoreError> {
    let observed_at = now.to_string();
    for issue in staged {
        let labels_json = serde_json::to_string(&issue.labels)?;
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_jira_observations
                 (scope, issue_id, current_key, issue_type, hierarchy_level, parent_key,
                  parent_id, labels_json, status, status_category, updated_at, summary,
                  observed_at, via, first_seen, last_seen, first_seen_generation,
                  last_seen_generation, visible_generation, state, removed_at, removed_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                     ?15, ?15, ?16, ?16, NULL, 'active', NULL, NULL)",
            params![
                HOST_SCOPE,
                issue.id,
                issue.key,
                issue.issue_type,
                issue.level,
                issue.parent,
                issue.parent_id,
                labels_json,
                issue.status,
                issue.cat,
                issue.updated,
                issue.summary,
                issue.observed_at,
                issue.via.as_str(),
                observed_at,
                generation_to_sql(generation)
            ],
        )?;
    }
    Ok(())
}

fn is_stale(existing: &str, staged: &str) -> bool {
    match (existing.parse::<Timestamp>(), staged.parse::<Timestamp>()) {
        (Ok(existing), Ok(staged)) => staged < existing,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_v1_catalog_migrates_to_the_jira_evidence_schema() {
        let directory = tempfile::TempDir::new().expect("a state root");
        let path = directory.path().join("catalog.db");
        {
            let connection = rusqlite::Connection::open(&path).expect("a v1 database");
            connection
                .execute_batch(include_str!("../migrations/0001_catalog.sql"))
                .expect("the v1 schema");
        }
        let database = Database::open(&path).expect("the catalog migrates");
        assert_eq!(database.schema_version().expect("a schema version"), 2);
        let tables: i64 = database
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table'
                   AND name IN ('catalog_jira_scope', 'catalog_jira_sweeps',
                                'catalog_jira_sweep_pages', 'catalog_jira_observations',
                                'catalog_jira_aliases', 'catalog_jira_collection')",
                [],
                |row| row.get(0),
            )
            .expect("the Jira tables exist");
        assert_eq!(tables, 6);
    }

    #[test]
    fn a_late_page_from_an_older_sweep_is_fenced() {
        let directory = tempfile::TempDir::new().expect("a state root");
        let database =
            Database::open(&directory.path().join("catalog.db")).expect("the catalog opens");
        let now = Timestamp::now();
        database
            .acquire_owner("owner", 300, &now)
            .expect("the lease");
        let first = database
            .begin_jira_sweep("owner", JiraSweepMode::Full, None, &now)
            .expect("the first sweep");
        let second = database
            .begin_jira_sweep("owner", JiraSweepMode::Incremental, None, &now)
            .expect("the newer sweep");
        assert_eq!(second.generation, first.generation + 1);
        assert!(matches!(
            database.commit_jira_page(&first, 0, &now, &[]),
            Err(StoreError::Fenced)
        ));
        database
            .commit_jira_page(&second, 0, &now, &[])
            .expect("the current sweep commits");
    }

    #[test]
    fn stale_answers_compare_as_instants_not_text() {
        assert!(is_stale("2026-10-25T10:00:00Z", "2026-10-25T09:00:00Z"));
        assert!(!is_stale(
            "2026-10-25T10:00:00Z",
            "2026-10-25T10:00:00.123Z"
        ));
        assert!(!is_stale("2026-10-25T10:00:00Z", "2026-10-25T10:00:00Z"));
    }
}
