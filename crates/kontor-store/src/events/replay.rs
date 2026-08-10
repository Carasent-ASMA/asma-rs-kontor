//! Reading the control-plane log back, statelessly or from a persisted
//! checkpoint.
//!
//! Both routes obey the same rule: a reader resumes **strictly after** a cursor
//! it was given, and never synthesizes one. Cursors are allocated by the writing
//! transaction and by nothing else, so two appenders racing cannot be handed the
//! same position and a reader cannot invent one that was never committed.

use kontor_core::id::{AgentRunId, EventCursor, ExternalId, ProjectId, Timestamp};
use kontor_core::repository::{RepositoryError, RepositoryResult, RuntimeEvent};
use kontor_core::state::NativeRuntimeIdentity;
use rusqlite::{OptionalExtension, Row, params, params_from_iter, types::Value};

use crate::SqliteStore;
use crate::events::append::stored_payload;
use crate::events::types::{ConsumerPage, page_limit};
use crate::repository::{backend, read_timestamp, text};

/// The columns every reconstructed [`RuntimeEvent`] needs, in row order.
const EVENT_COLUMNS: &str = "cursor, project_id, agent_run_id, runtime_kind, host, generation, \
     native_id, native_event_id, native_sequence, payload, payload_hash, observed_at, recorded_at";

fn read_event(row: &Row<'_>) -> RepositoryResult<RuntimeEvent> {
    let native_event_id: Option<String> = row.get(7).map_err(backend)?;
    let payload: String = row.get(9).map_err(backend)?;
    let payload_hash: String = row.get(10).map_err(backend)?;
    let generation: i64 = row.get(5).map_err(backend)?;
    Ok(RuntimeEvent {
        cursor: EventCursor::parse(row.get(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        agent_run_id: AgentRunId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        identity: NativeRuntimeIdentity {
            runtime_kind: kontor_core::id::RuntimeKindKey::parse(
                &row.get::<_, String>(3).map_err(backend)?,
            )?,
            host: kontor_core::id::ExternalName::parse(&row.get::<_, String>(4).map_err(backend)?)?,
            generation: u64::try_from(generation).unwrap_or_default(),
            native_id: ExternalId::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        },
        native_event_id: native_event_id
            .as_deref()
            .map(ExternalId::parse)
            .transpose()?,
        native_sequence: u64::try_from(row.get::<_, i64>(8).map_err(backend)?).unwrap_or_default(),
        payload: stored_payload(&payload, &payload_hash)?,
        observed_at: read_timestamp(&row.get::<_, String>(11).map_err(backend)?)?,
        recorded_at: read_timestamp(&row.get::<_, String>(12).map_err(backend)?)?,
    })
}

/// Select observations strictly after `after`, ascending, optionally for one run
/// and optionally bounded by `limit`.
///
/// `after` of `None` starts at the origin, which is control-plane cursor 1 and
/// names no row — so the first real event is never skipped.
fn select_events(
    connection: &rusqlite::Connection,
    project_id: ProjectId,
    agent_run_id: Option<AgentRunId>,
    after: Option<EventCursor>,
    limit: Option<i64>,
) -> RepositoryResult<Vec<RuntimeEvent>> {
    let mut sql = format!(
        "SELECT {EVENT_COLUMNS} FROM runtime_events
         WHERE event_kind = 'runtime_observation' AND project_id = ?1 AND cursor > ?2"
    );
    let mut arguments: Vec<Value> = vec![
        Value::Text(project_id.to_string()),
        Value::Integer(after.map_or(0, EventCursor::get)),
    ];
    if let Some(run) = agent_run_id {
        arguments.push(Value::Text(run.to_string()));
        sql.push_str(&format!(" AND agent_run_id = ?{}", arguments.len()));
    }
    sql.push_str(" ORDER BY cursor");
    if let Some(limit) = limit {
        arguments.push(Value::Integer(limit));
        sql.push_str(&format!(" LIMIT ?{}", arguments.len()));
    }

    let mut statement = connection.prepare(&sql).map_err(backend)?;
    let mut rows = statement
        .query(params_from_iter(arguments))
        .map_err(backend)?;
    let mut events = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        events.push(read_event(row)?);
    }
    Ok(events)
}

/// Every observation of one run after `after`. The KON-MVP-03 read path.
pub(crate) fn read_runtime_events(
    store: &SqliteStore,
    project_id: ProjectId,
    agent_run_id: AgentRunId,
    after: Option<EventCursor>,
) -> RepositoryResult<Vec<RuntimeEvent>> {
    select_events(
        &store.connection,
        project_id,
        Some(agent_run_id),
        after,
        None,
    )
}

impl SqliteStore {
    /// Read control-plane observations strictly after a cursor.
    ///
    /// Stateless: the caller owns its position, nothing is written, and the
    /// result is always ascending and always `> after`. Passing the newest
    /// cursor returns nothing; passing one beyond the newest returns nothing
    /// either, rather than wrapping or failing.
    ///
    /// # Errors
    /// * [`RepositoryError::Domain`] when `limit` is zero.
    /// * [`RepositoryError::Backend`] on backend failure.
    pub fn read_control_events_after(
        &self,
        project_id: ProjectId,
        agent_run_id: Option<AgentRunId>,
        after: EventCursor,
        limit: u32,
    ) -> RepositoryResult<Vec<RuntimeEvent>> {
        let limit = page_limit(limit)?;
        select_events(
            &self.connection,
            project_id,
            agent_run_id,
            Some(after),
            Some(limit),
        )
    }

    /// Deliver the next page to a persisted consumer and advance its checkpoint.
    ///
    /// The read and the advance happen in one transaction, so two callers cannot
    /// be handed the same page and a crash between them cannot lose one. An
    /// empty page leaves the checkpoint exactly where it was: there is nothing
    /// to acknowledge, and moving it would skip whatever commits next.
    ///
    /// A consumer that has never been seen starts at the origin (control-plane
    /// cursor 1, which names no row), so its first page begins at the very first
    /// event rather than after it.
    ///
    /// # Errors
    /// * [`RepositoryError::Domain`] when `limit` is zero.
    /// * [`RepositoryError::Conflict`] when the checkpoint moved during the
    ///   write.
    pub fn page_consumer(
        &self,
        project_id: ProjectId,
        consumer_key: &ExternalId,
        limit: u32,
        now: Timestamp,
    ) -> RepositoryResult<ConsumerPage> {
        let limit = page_limit(limit)?;
        let transaction = self.begin()?;
        let stored: Option<i64> = transaction
            .query_row(
                "SELECT last_cursor FROM runtime_replay_consumers
                 WHERE project_id = ?1 AND consumer_key = ?2",
                params![project_id.to_string(), consumer_key.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        // The origin is the reserved cursor 1: it names no row, so a new
        // consumer resumes *after* nothing and is delivered everything.
        let last_cursor = EventCursor::parse(stored.unwrap_or(1))?;

        let events = select_events(
            &transaction,
            project_id,
            None,
            Some(last_cursor),
            Some(limit),
        )?;
        let Some(newest) = events.last().map(|event| event.cursor) else {
            transaction.commit().map_err(backend)?;
            return Ok(ConsumerPage {
                events,
                last_cursor,
            });
        };

        let changed = transaction
            .execute(
                "INSERT INTO runtime_replay_consumers
                     (project_id, consumer_key, last_cursor, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (project_id, consumer_key) DO UPDATE
                     SET last_cursor = excluded.last_cursor, updated_at = excluded.updated_at
                     WHERE runtime_replay_consumers.last_cursor = ?5",
                params![
                    project_id.to_string(),
                    consumer_key.as_str(),
                    newest.get(),
                    text(now),
                    last_cursor.get()
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "replay consumer",
                rule: "the checkpoint moved during the write",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(ConsumerPage {
            events,
            last_cursor: newest,
        })
    }

    /// The checkpoint a persisted consumer currently stands at, if it has one.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Backend`] on backend failure.
    pub fn consumer_cursor(
        &self,
        project_id: ProjectId,
        consumer_key: &ExternalId,
    ) -> RepositoryResult<Option<EventCursor>> {
        let stored: Option<i64> = self
            .connection
            .query_row(
                "SELECT last_cursor FROM runtime_replay_consumers
                 WHERE project_id = ?1 AND consumer_key = ?2",
                params![project_id.to_string(), consumer_key.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        stored
            .map(EventCursor::parse)
            .transpose()
            .map_err(Into::into)
    }
}
