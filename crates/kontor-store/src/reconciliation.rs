//! Reconciliation epochs: censusing a runtime generation without inventing
//! certainty.
//!
//! A census answers one question — *which native sessions does this runtime
//! generation say it has right now?* — and the answer is only ever used to move
//! contact and freshness. It never closes a run. A session that is absent from a
//! completed census becomes [`DerivedRunState::LostContact`], which is a
//! statement about Kontor's knowledge, not about the work; the run's lifecycle,
//! terminal outcome and closure evidence are left exactly as they were, and a
//! later census that finds the session again lifts the uncertainty back off.
//!
//! Two things make that safe to repeat. An epoch is addressed by a caller-stable
//! key, so a crash mid-census reopens the same epoch instead of starting a
//! second one against a different moment. And absence is only ever read from a
//! census that actually *completed*: a partial or failed sweep proves nothing
//! about what it did not reach, so it changes nothing.

use std::fmt;

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, CanonicalDocument, EventCursor, ExternalId, ExternalName, ProjectId,
    RuntimeKindKey, Timestamp,
};
use kontor_core::repository::{NewRuntimeEvent, RepositoryError, RepositoryResult};
use kontor_core::state::{
    DerivedRunState, Freshness, NativeRuntimeIdentity, ObservedRunState, RunDerivation,
    RunProjection, RuntimeContact,
};
use rusqlite::{OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::SqliteStore;
use crate::events::append::{
    NormalizedControl, append_event, last_reduced_sequence, reduce_observation, sequence_column,
};
use crate::repository::{
    backend, conflict, generation_column, read_agent_run, revision_column, text,
};

/// The identity of one reconciliation epoch.
///
/// A local id rather than a domain one: an epoch is an operational sweep of this
/// store, not a domain aggregate anything else refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReconciliationEpochId(Uuid);

impl ReconciliationEpochId {
    /// Mint a new epoch id.
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Parse a stored epoch id.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for anything that is not a UUID.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        Uuid::parse_str(text)
            .map(Self)
            .map_err(|_| DomainError::invalid("ReconciliationEpochId", "is not a UUID"))
    }
}

impl fmt::Display for ReconciliationEpochId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// How far one census has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochStatus {
    /// The census is running. Nothing may be concluded from absence yet.
    InProgress,
    /// The census finished and is authoritative about what it did not find.
    Completed,
    /// The census did not finish. It proves nothing about absence, ever.
    Failed,
}

impl EpochStatus {
    /// The stable spelling used in SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// Parse the stable spelling.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for any other text.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        match text {
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(DomainError::invalid("EpochStatus", "is not a known value")),
        }
    }
}

/// The caller-stable identity of one census.
///
/// Repeating the key reopens the same epoch: a daemon that dies mid-sweep and
/// restarts continues the census it began, against the control-plane position it
/// began it at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochKey {
    /// Owning project.
    pub project_id: ProjectId,
    /// The runtime family being censused.
    pub runtime_kind: RuntimeKindKey,
    /// The host that owns the generation.
    pub host: ExternalName,
    /// The generation being censused.
    pub generation: u64,
    /// The caller's stable key for this sweep.
    pub reconciliation_key: ExternalId,
}

/// One census, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationEpoch {
    /// The epoch.
    pub epoch_id: ReconciliationEpochId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The runtime family it censuses.
    pub runtime_kind: RuntimeKindKey,
    /// The host whose generation it censuses.
    pub host: ExternalName,
    /// The generation it censuses.
    pub generation: u64,
    /// The control-plane position the census was started against.
    pub census_start_cursor: EventCursor,
    /// The position it completed at, once it has.
    pub completion_cursor: Option<EventCursor>,
    /// How far it has got.
    pub status: EpochStatus,
    /// When it began.
    pub started_at: Timestamp,
}

/// One native session a census saw.
///
/// The caller reports what the runtime said; it does *not* say which run the
/// session belongs to. That is resolved here from the persisted binding, so a
/// census cannot attach a session to a run by assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusItem {
    /// The native session.
    pub identity: NativeRuntimeIdentity,
    /// The runtime's own event id for this report, when it has one.
    pub native_event_id: Option<ExternalId>,
    /// The runtime's own ordering for this report.
    pub native_sequence: u64,
    /// What the runtime reported.
    pub observed: ObservedRunState,
    /// The transport result of the census contact.
    pub contact: RuntimeContact,
    /// How old the newest confirmation is.
    pub freshness: Freshness,
    /// The immutable canonical control metadata, free of session content.
    pub raw: CanonicalDocument,
    /// An opaque reference to the runtime's own record.
    pub audit_ref: ExternalId,
    /// When the runtime reported it.
    pub observed_at: Timestamp,
}

/// What recording one census item did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusOutcome {
    /// The run the native session is bound to, when one exists.
    pub agent_run_id: Option<AgentRunId>,
    /// The control-plane cursor of the raw-plus-normalized observation this item
    /// was recorded from.
    ///
    /// Never optional: an orphan is evidenced exactly like a bound session, so
    /// every census fact — membership included — cites a row that was persisted
    /// before it.
    pub observation_cursor: EventCursor,
    /// Whether the projection was reduced from it.
    pub reduced: bool,
    /// Whether this session belongs to no local binding. Recorded as evidence,
    /// attached to nothing.
    pub orphaned: bool,
}

/// What finishing a census concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochSummary {
    /// The epoch.
    pub epoch_id: ReconciliationEpochId,
    /// Its final status.
    pub status: EpochStatus,
    /// The control-plane position it completed at.
    pub completion_cursor: Option<EventCursor>,
    /// Bound sessions the census found.
    pub present: u32,
    /// Bound sessions a *completed* census did not find, now lost contact.
    pub lost_contact: u32,
    /// Native sessions with no local binding.
    pub orphaned: u32,
}

/// Read one epoch row, including the full runtime identity it censuses.
///
/// The identity is not decoration: every membership row and every completion
/// comparison is keyed by `native_id` alone, and a native id is only unique
/// inside `(runtime_kind, host, generation)`. Without the epoch's own scope to
/// check an item against, a session from a different host or generation could sit
/// in this epoch's census under a colliding id.
fn read_epoch(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    epoch_id: ReconciliationEpochId,
) -> RepositoryResult<Option<ReconciliationEpoch>> {
    /// `(runtime_kind, host, generation, census_start, completion, status, started_at)`.
    type EpochRow = (String, String, i64, i64, Option<i64>, String, String);

    let row: Option<EpochRow> = transaction
        .query_row(
            "SELECT runtime_kind, host, generation, census_start_cursor, completion_cursor,
                    status, started_at
             FROM runtime_reconciliation_epochs WHERE project_id = ?1 AND epoch_id = ?2",
            params![project_id.to_string(), epoch_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((runtime_kind, host, generation, census_start, completion, status, started_at)) = row
    else {
        return Ok(None);
    };
    Ok(Some(ReconciliationEpoch {
        epoch_id,
        project_id,
        runtime_kind: RuntimeKindKey::parse(&runtime_kind)?,
        host: ExternalName::parse(&host)?,
        generation: u64::try_from(generation).unwrap_or_default(),
        census_start_cursor: EventCursor::parse(census_start)?,
        completion_cursor: completion.map(EventCursor::parse).transpose()?,
        status: EpochStatus::parse(&status)?,
        started_at: crate::repository::read_timestamp(&started_at)?,
    }))
}

/// Append the raw-plus-normalized evidence for a native session this Realm holds
/// no binding for.
///
/// The orphan case is where an evidence-before-consequence rule is easiest to
/// quietly drop — there is no run to file the observation against, so it is
/// tempting to record only the membership fact. That would leave a stored
/// consequence with nothing behind it, so the observation is appended anyway, in
/// the same cursor space and with the same normalized fields as a bound one, and
/// simply names no run.
///
/// Its continuity identity is the usual
/// `(runtime_kind, host, generation, native_id, native_sequence)`, in the census
/// space: a second sweep reporting the same moment maps onto the row that already
/// holds it rather than filing a second truth.
fn append_census_observation(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    item: &CensusItem,
) -> RepositoryResult<EventCursor> {
    let generation = generation_column(item.identity.generation)?;
    let native_sequence = sequence_column(item.native_sequence)?;
    let payload_hash = item.raw.hash().as_str().to_owned();
    let inserted = transaction.execute(
        "INSERT INTO runtime_events
             (project_id, event_kind, runtime_kind, host, generation, native_id, native_event_id,
              native_sequence, observed_state, contact, freshness, audit_ref, payload, payload_hash,
              observed_at, recorded_at)
         VALUES (?1, 'census_observation', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15)",
        params![
            project_id.to_string(),
            item.identity.runtime_kind.as_str(),
            item.identity.host.as_str(),
            generation,
            item.identity.native_id.as_str(),
            item.native_event_id.as_ref().map(ExternalId::as_str),
            native_sequence,
            item.observed.as_str(),
            item.contact.as_str(),
            item.freshness.as_str(),
            item.audit_ref.as_str(),
            item.raw.json(),
            payload_hash.as_str(),
            text(item.observed_at),
            text(Timestamp::now())
        ],
    );
    let Err(error) = inserted else {
        return Ok(EventCursor::parse(transaction.last_insert_rowid())?);
    };
    let mapped = backend(error);
    if !matches!(mapped, RepositoryError::Conflict { .. }) {
        return Err(mapped);
    }

    let existing: Option<(i64, String)> = transaction
        .query_row(
            "SELECT cursor, payload_hash FROM runtime_events
             WHERE event_kind = 'census_observation' AND runtime_kind = ?1 AND host = ?2
               AND generation = ?3 AND native_id = ?4 AND native_sequence = ?5",
            params![
                item.identity.runtime_kind.as_str(),
                item.identity.host.as_str(),
                generation,
                item.identity.native_id.as_str(),
                native_sequence
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((cursor, stored_hash)) = existing else {
        return Err(mapped);
    };
    if stored_hash != payload_hash {
        return Err(conflict(
            "census observation",
            "a different observation is already stored for this native sequence",
        ));
    }
    Ok(EventCursor::parse(cursor)?)
}

/// The newest allocated control-plane cursor, or the reserved origin.
fn head_cursor(transaction: &Transaction<'_>) -> RepositoryResult<EventCursor> {
    let head: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(cursor), 1) FROM runtime_events",
            [],
            |row| row.get(0),
        )
        .map_err(backend)?;
    Ok(EventCursor::parse(head)?)
}

impl SqliteStore {
    /// Begin — or reopen — the census named by `key`.
    ///
    /// The same key always returns the same epoch, including its original
    /// census-start cursor. That is what makes a sweep interrupted by a crash
    /// resumable: it continues against the moment it started, not against the
    /// moment it woke up.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Backend`] on backend failure, or
    /// [`RepositoryError::Conflict`] when the epoch row cannot be read back.
    pub fn begin_reconciliation_epoch(
        &self,
        key: &EpochKey,
        now: Timestamp,
    ) -> RepositoryResult<ReconciliationEpoch> {
        let transaction = self.begin()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT epoch_id FROM runtime_reconciliation_epochs
                 WHERE project_id = ?1 AND runtime_kind = ?2 AND host = ?3 AND generation = ?4
                   AND reconciliation_key = ?5",
                params![
                    key.project_id.to_string(),
                    key.runtime_kind.as_str(),
                    key.host.as_str(),
                    generation_column(key.generation)?,
                    key.reconciliation_key.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if let Some(epoch_id) = existing {
            let epoch_id = ReconciliationEpochId::parse(&epoch_id)?;
            let epoch = read_epoch(&transaction, key.project_id, epoch_id)?.ok_or(
                RepositoryError::NotFound {
                    subject: "reconciliation epoch",
                },
            )?;
            transaction.commit().map_err(backend)?;
            return Ok(epoch);
        }

        let epoch_id = ReconciliationEpochId::generate();
        let census_start = head_cursor(&transaction)?;
        transaction
            .execute(
                "INSERT INTO runtime_reconciliation_epochs
                     (epoch_id, project_id, runtime_kind, host, generation, reconciliation_key,
                      census_start_cursor, started_at, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'in_progress')",
                params![
                    epoch_id.to_string(),
                    key.project_id.to_string(),
                    key.runtime_kind.as_str(),
                    key.host.as_str(),
                    generation_column(key.generation)?,
                    key.reconciliation_key.as_str(),
                    census_start.get(),
                    text(now)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(ReconciliationEpoch {
            epoch_id,
            project_id: key.project_id,
            runtime_kind: key.runtime_kind.clone(),
            host: key.host.clone(),
            generation: key.generation,
            census_start_cursor: census_start,
            completion_cursor: None,
            status: EpochStatus::InProgress,
            started_at: now,
        })
    }

    /// Record one native session a census saw.
    ///
    /// The observation is appended as raw-plus-normalized evidence first, then
    /// the membership row, and only then — if the binding, generation and
    /// sequence rules all permit it — is anything reduced. A session with no
    /// local binding is recorded as an orphan: evidenced in exactly the same
    /// order, and attached to nothing.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when the epoch does not exist.
    /// * [`RepositoryError::Conflict`] when a *closed* census is asked to admit
    ///   a session it never saw.
    /// * [`RepositoryError::Domain`] when the raw payload carries runtime-owned
    ///   session content, or when the item's runtime kind, host or generation is
    ///   not the one this epoch censuses.
    pub fn observe_census_item(
        &self,
        epoch_id: ReconciliationEpochId,
        project_id: ProjectId,
        item: &CensusItem,
    ) -> RepositoryResult<CensusOutcome> {
        crate::events::types::ensure_no_session_content(&item.raw)?;

        let transaction = self.begin()?;
        let epoch =
            read_epoch(&transaction, project_id, epoch_id)?.ok_or(RepositoryError::NotFound {
                subject: "reconciliation epoch",
            })?;

        // A census is a sweep of *one* runtime generation on one host, and
        // everything downstream is keyed by `native_id` alone: the membership row,
        // the presence check at completion, the absence rule. A native id is only
        // unique inside `(runtime_kind, host, generation)`, so an item from another
        // scope with a colliding id would mark a bound session present it never
        // saw, or occupy the membership slot of the session that is genuinely
        // missing. Refused before the membership lookup and before any append, so
        // a foreign item cannot map onto this epoch's rows either.
        if item.identity.runtime_kind != epoch.runtime_kind
            || item.identity.host != epoch.host
            || item.identity.generation != epoch.generation
        {
            return Err(DomainError::invalid(
                "census item",
                "the session is not in the runtime kind, host and generation this epoch censuses",
            )
            .into());
        }

        // A member already recorded is returned as it stands, whatever the
        // epoch's status: re-running a census must record no second fact.
        let recorded: Option<(Option<String>, i64)> = transaction
            .query_row(
                "SELECT agent_run_id, observation_cursor FROM runtime_reconciliation_members
                 WHERE project_id = ?1 AND epoch_id = ?2 AND native_id = ?3",
                params![
                    project_id.to_string(),
                    epoch_id.to_string(),
                    item.identity.native_id.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((run, cursor)) = recorded {
            transaction.commit().map_err(backend)?;
            return Ok(CensusOutcome {
                agent_run_id: run.as_deref().map(AgentRunId::parse).transpose()?,
                observation_cursor: EventCursor::parse(cursor)?,
                reduced: false,
                orphaned: run.is_none(),
            });
        }
        if epoch.status != EpochStatus::InProgress {
            return Err(conflict(
                "reconciliation epoch",
                "a finished census cannot admit a session it never saw",
            ));
        }

        // The binding is *looked up*, never supplied: a census may not decide
        // which run a native session belongs to.
        let bound: Option<String> = transaction
            .query_row(
                "SELECT agent_run_id FROM runtime_bindings
                 WHERE project_id = ?1 AND runtime_kind = ?2 AND host = ?3 AND generation = ?4
                   AND native_id = ?5",
                params![
                    project_id.to_string(),
                    item.identity.runtime_kind.as_str(),
                    item.identity.host.as_str(),
                    generation_column(item.identity.generation)?,
                    item.identity.native_id.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;

        let Some(agent_run_id) = bound.as_deref().map(AgentRunId::parse).transpose()? else {
            // An unbound native session. It is real, it is recorded, and it is
            // not attached to anything: a control plane that guesses here is how
            // one run's evidence ends up closing another.
            //
            // Evidence still comes first. The observation is appended before the
            // membership row that cites it, in the same order and the same
            // transaction a bound session gets, so an orphan is not the one place
            // in the store where a fact exists with nothing behind it.
            let cursor = append_census_observation(&transaction, project_id, item)?;
            transaction
                .execute(
                    "INSERT INTO runtime_reconciliation_members
                         (project_id, epoch_id, native_id, observation_cursor, observed_state,
                          recorded_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        project_id.to_string(),
                        epoch_id.to_string(),
                        item.identity.native_id.as_str(),
                        cursor.get(),
                        item.observed.as_str(),
                        text(item.observed_at)
                    ],
                )
                .map_err(backend)?;
            transaction.commit().map_err(backend)?;
            return Ok(CensusOutcome {
                agent_run_id: None,
                observation_cursor: cursor,
                reduced: false,
                orphaned: true,
            });
        };

        let event = NewRuntimeEvent {
            project_id,
            agent_run_id,
            identity: item.identity.clone(),
            native_event_id: item.native_event_id.clone(),
            native_sequence: item.native_sequence,
            payload: item.raw.clone(),
            observed_at: item.observed_at,
        };
        let (cursor, appended) = append_event(
            &transaction,
            &event,
            Some(item.observed),
            Some(NormalizedControl {
                contact: item.contact,
                freshness: item.freshness,
                audit_ref: &item.audit_ref,
            }),
        )?;
        transaction
            .execute(
                "INSERT INTO runtime_reconciliation_members
                     (project_id, epoch_id, native_id, agent_run_id, observation_cursor,
                      observed_state, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    project_id.to_string(),
                    epoch_id.to_string(),
                    item.identity.native_id.as_str(),
                    agent_run_id.to_string(),
                    cursor.get(),
                    item.observed.as_str(),
                    text(item.observed_at)
                ],
            )
            .map_err(backend)?;

        let run = read_agent_run(&transaction, project_id, agent_run_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "agent run",
            },
        )?;
        let bound_session = run
            .binding
            .as_ref()
            .is_some_and(|binding| binding.identity.same_session(&item.identity));
        let last_applied = last_reduced_sequence(&transaction, project_id, agent_run_id)?;
        let reducible = appended
            && bound_session
            && !run.projection.is_closed()
            && RunProjection::may_reduce(last_applied, item.native_sequence);
        if reducible {
            reduce_observation(
                &transaction,
                &run,
                cursor,
                &item.identity,
                item.observed,
                item.observed_at,
                item.raw.hash(),
                item.contact,
                item.freshness,
                item.native_sequence,
            )?;
        }
        transaction.commit().map_err(backend)?;
        Ok(CensusOutcome {
            agent_run_id: Some(agent_run_id),
            observation_cursor: cursor,
            reduced: reducible,
            orphaned: false,
        })
    }

    /// Finish a census and apply what it is entitled to conclude.
    ///
    /// `authoritative` is the caller's statement that the sweep actually
    /// covered the generation. A `false` here — a partial list, a truncated
    /// page, a runtime that stopped answering halfway — marks the epoch failed
    /// and changes no run at all, because a census that did not finish proves
    /// nothing about what it did not reach.
    ///
    /// A completed census applies absence to exactly one dimension: a bound
    /// session it did not find becomes [`DerivedRunState::LostContact`]. The
    /// run's lifecycle, terminal outcome and closure evidence are untouched, and
    /// finishing the same completed epoch again writes nothing.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when the epoch does not exist.
    /// * [`RepositoryError::Conflict`] when a run's revision moved during the
    ///   write.
    pub fn finish_reconciliation_epoch(
        &self,
        epoch_id: ReconciliationEpochId,
        project_id: ProjectId,
        authoritative: bool,
        now: Timestamp,
    ) -> RepositoryResult<EpochSummary> {
        let transaction = self.begin()?;
        let epoch =
            read_epoch(&transaction, project_id, epoch_id)?.ok_or(RepositoryError::NotFound {
                subject: "reconciliation epoch",
            })?;
        if epoch.status != EpochStatus::InProgress {
            // Already settled. Repeating a finish is a read.
            let summary = summarize(&transaction, &epoch)?;
            transaction.commit().map_err(backend)?;
            return Ok(summary);
        }

        if !authoritative {
            transaction
                .execute(
                    "UPDATE runtime_reconciliation_epochs
                     SET status = 'failed', completed_at = ?1
                     WHERE project_id = ?2 AND epoch_id = ?3 AND status = 'in_progress'",
                    params![text(now), project_id.to_string(), epoch_id.to_string()],
                )
                .map_err(backend)?;
            let epoch = ReconciliationEpoch {
                status: EpochStatus::Failed,
                ..epoch
            };
            let summary = summarize(&transaction, &epoch)?;
            transaction.commit().map_err(backend)?;
            return Ok(summary);
        }

        // Every binding this generation owns, compared against what the census
        // actually saw.
        let bindings: Vec<(String, String)> = {
            let mut statement = transaction
                .prepare(
                    "SELECT binding.agent_run_id, binding.native_id
                     FROM runtime_bindings AS binding
                     JOIN runtime_reconciliation_epochs AS epoch
                       ON epoch.project_id = binding.project_id
                      AND epoch.runtime_kind = binding.runtime_kind
                      AND epoch.host = binding.host
                      AND epoch.generation = binding.generation
                     WHERE epoch.project_id = ?1 AND epoch.epoch_id = ?2",
                )
                .map_err(backend)?;
            let mut rows = statement
                .query(params![project_id.to_string(), epoch_id.to_string()])
                .map_err(backend)?;
            let mut bindings = Vec::new();
            while let Some(row) = rows.next().map_err(backend)? {
                bindings.push((row.get(0).map_err(backend)?, row.get(1).map_err(backend)?));
            }
            bindings
        };

        let mut present = 0u32;
        let mut lost = 0u32;
        for (agent_run_id, native_id) in bindings {
            let agent_run_id = AgentRunId::parse(&agent_run_id)?;
            let seen: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM runtime_reconciliation_members
                     WHERE project_id = ?1 AND epoch_id = ?2 AND native_id = ?3",
                    params![
                        project_id.to_string(),
                        epoch_id.to_string(),
                        native_id.as_str()
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if seen.is_some() {
                present += 1;
                record_result(
                    &transaction,
                    project_id,
                    epoch_id,
                    agent_run_id,
                    "present",
                    now,
                )?;
                continue;
            }
            if apply_absence(&transaction, project_id, epoch_id, agent_run_id, now)? {
                lost += 1;
            }
        }

        let orphaned: i64 = transaction
            .query_row(
                "SELECT count(*) FROM runtime_reconciliation_members
                 WHERE project_id = ?1 AND epoch_id = ?2 AND agent_run_id IS NULL",
                params![project_id.to_string(), epoch_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;

        let completion = head_cursor(&transaction)?;
        transaction
            .execute(
                "UPDATE runtime_reconciliation_epochs
                 SET status = 'completed', completed_at = ?1, completion_cursor = ?2
                 WHERE project_id = ?3 AND epoch_id = ?4 AND status = 'in_progress'",
                params![
                    text(now),
                    completion.get(),
                    project_id.to_string(),
                    epoch_id.to_string()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;

        Ok(EpochSummary {
            epoch_id,
            status: EpochStatus::Completed,
            completion_cursor: Some(completion),
            present,
            lost_contact: lost,
            orphaned: u32::try_from(orphaned).unwrap_or(u32::MAX),
        })
    }
}

/// Apply the one consequence absence is allowed to have.
///
/// Returns whether this call changed the run. A closed run is left alone, and a
/// run that is already out of contact is not "changed" a second time — which is
/// what keeps repeating a census free of revision churn.
fn apply_absence(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    epoch_id: ReconciliationEpochId,
    agent_run_id: AgentRunId,
    now: Timestamp,
) -> RepositoryResult<bool> {
    let Some(run) = read_agent_run(transaction, project_id, agent_run_id)? else {
        return Ok(false);
    };
    if run.projection.is_closed() {
        // A closed run has a verdict already. A census cannot revisit it, and
        // absence certainly cannot.
        return Ok(false);
    }
    // The reduction reads desired, the binding, contact and lifecycle
    // separately, and writes only the conclusion. `ProcessMissing` is the honest
    // input here: the census reached the runtime and the session was not there.
    let derived = kontor_core::state::derive_run_state(&RunDerivation {
        desired: run.projection.desired,
        observation: None,
        binding: run.binding.as_ref().map(|binding| &binding.identity),
        freshness: Freshness::Stale,
        contact: RuntimeContact::ProcessMissing,
        terminal: None,
    })?;
    debug_assert!(
        !derived.is_terminal(),
        "absence must never reduce to a terminal conclusion"
    );
    if derived == run.projection.derived {
        record_result(
            transaction,
            project_id,
            epoch_id,
            agent_run_id,
            "unchanged",
            now,
        )?;
        return Ok(false);
    }
    let next_revision = run.revision.next()?;
    let changed = transaction
        .execute(
            "UPDATE agent_runs SET derived_state = ?1, revision = ?2
             WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
            params![
                derived.as_str(),
                revision_column(next_revision)?,
                project_id.to_string(),
                agent_run_id.to_string(),
                revision_column(run.revision)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "agent run",
            "the run revision moved during reconciliation",
        ));
    }
    debug_assert_eq!(derived, DerivedRunState::LostContact);
    transaction
        .execute(
            "INSERT INTO runtime_reconciliation_results
                 (project_id, epoch_id, agent_run_id, outcome, source_revision,
                  resulting_revision, recorded_at)
             VALUES (?1, ?2, ?3, 'lost_contact', ?4, ?5, ?6)
             ON CONFLICT DO NOTHING",
            params![
                project_id.to_string(),
                epoch_id.to_string(),
                agent_run_id.to_string(),
                revision_column(run.revision)?,
                revision_column(next_revision)?,
                text(now)
            ],
        )
        .map_err(backend)?;
    Ok(true)
}

/// Record what a census concluded about one run, once.
fn record_result(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    epoch_id: ReconciliationEpochId,
    agent_run_id: AgentRunId,
    outcome: &str,
    now: Timestamp,
) -> RepositoryResult<()> {
    let Some(run) = read_agent_run(transaction, project_id, agent_run_id)? else {
        return Ok(());
    };
    let revision = revision_column(run.revision)?;
    transaction
        .execute(
            "INSERT INTO runtime_reconciliation_results
                 (project_id, epoch_id, agent_run_id, outcome, source_revision,
                  resulting_revision, source_cursor, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)
             ON CONFLICT DO NOTHING",
            params![
                project_id.to_string(),
                epoch_id.to_string(),
                agent_run_id.to_string(),
                outcome,
                revision,
                run.projection.last_cursor.map(EventCursor::get),
                text(now)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

/// Read back what a settled epoch concluded, without changing anything.
fn summarize(
    transaction: &Transaction<'_>,
    epoch: &ReconciliationEpoch,
) -> RepositoryResult<EpochSummary> {
    let count = |outcome: &str| -> RepositoryResult<u32> {
        let total: i64 = transaction
            .query_row(
                "SELECT count(*) FROM runtime_reconciliation_results
                 WHERE project_id = ?1 AND epoch_id = ?2 AND outcome = ?3",
                params![
                    epoch.project_id.to_string(),
                    epoch.epoch_id.to_string(),
                    outcome
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        Ok(u32::try_from(total).unwrap_or(u32::MAX))
    };
    let orphaned: i64 = transaction
        .query_row(
            "SELECT count(*) FROM runtime_reconciliation_members
             WHERE project_id = ?1 AND epoch_id = ?2 AND agent_run_id IS NULL",
            params![epoch.project_id.to_string(), epoch.epoch_id.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    Ok(EpochSummary {
        epoch_id: epoch.epoch_id,
        status: epoch.status,
        completion_cursor: epoch.completion_cursor,
        present: count("present")?,
        lost_contact: count("lost_contact")?,
        orphaned: u32::try_from(orphaned).unwrap_or(u32::MAX),
    })
}
