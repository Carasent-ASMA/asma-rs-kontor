//! Appending control-plane evidence, and only then reducing from it.
//!
//! Every path in this module obeys the same order inside one transaction:
//!
//! 1. refuse runtime-owned session content, before SQL sees anything;
//! 2. insert the immutable raw payload **and** its normalized control fields in
//!    one statement, or map the row to the one that already holds that identity;
//! 3. record any continuity gap the observation revealed;
//! 4. only then compare-and-swap the projection.
//!
//! A failure at any step rolls the whole thing back, so a stored consequence can
//! always name the stored evidence it came from, and evidence never exists
//! because of a consequence.

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, CanonicalDocument, ContentHash, EventCursor, ExternalId, ProjectId, Timestamp,
};
use kontor_core::repository::{
    AgentRun, NewObservation, NewRuntimeEvent, RepositoryError, RepositoryResult,
};
use kontor_core::state::{
    DerivedRunState, Freshness, NativeRuntimeIdentity, ObservedRunState, RunDerivation,
    RunLifecycle, RunProjection, RuntimeContact, RuntimeObservation, reduce_run_lifecycle,
};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::SqliteStore;
use crate::events::types::{
    ContentDiscontinuity, ContentGapOutcome, ControlGap, ControlObservation,
    ControlObservationOutcome,
};
use crate::repository::{
    backend, conflict, generation_column, read_agent_run, revision_column, text,
};

/// The normalized half of a control-plane observation, written in the same
/// statement as the raw payload.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NormalizedControl<'a> {
    pub(crate) contact: RuntimeContact,
    pub(crate) freshness: Freshness,
    pub(crate) audit_ref: &'a ExternalId,
}

/// Map a matched identity onto the cursor it already holds, or refuse it.
///
/// Two rows claiming one identity are the same observation — unless their
/// payloads differ, in which case the runtime has told us two different things
/// about one moment and neither may be trusted enough to reduce. Every identity
/// the log recognizes is held to that rule, so no dedup path can quietly accept a
/// changed payload as a duplicate and drop the disagreement on the floor.
fn same_observation(
    stored: Option<(i64, String)>,
    payload_hash: &str,
    rule: &'static str,
) -> RepositoryResult<Option<EventCursor>> {
    let Some((cursor, stored_hash)) = stored else {
        return Ok(None);
    };
    if stored_hash != payload_hash {
        return Err(conflict("control observation", rule));
    }
    Ok(Some(EventCursor::parse(cursor)?))
}

/// Widen a native sequence into the storable range.
pub(crate) fn sequence_column(sequence: u64) -> RepositoryResult<i64> {
    i64::try_from(sequence).map_err(|_| RepositoryError::Backend {
        detail: "native sequence exceeds the storable range".to_owned(),
    })
}

/// Append one raw runtime event, mapping a replay onto the row it already has.
///
/// Returns the row's control-plane cursor and whether this call is what created
/// it. `false` means the identity was already stored: the caller must not reduce
/// anything a second time.
pub(crate) fn append_event(
    transaction: &Transaction<'_>,
    request: &NewRuntimeEvent,
    observed: Option<ObservedRunState>,
    normalized: Option<NormalizedControl<'_>>,
) -> RepositoryResult<(EventCursor, bool)> {
    // The content boundary lives here, in the one statement every append goes
    // through, rather than in each public method: an entry point that forgot to
    // check would otherwise be a hole in the boundary rather than a bug in one
    // caller. Checked before any SQL, so a rejected transcript never reaches a
    // row it could be rolled back out of.
    crate::events::types::ensure_no_session_content(&request.payload)?;

    let generation = generation_column(request.identity.generation)?;
    let payload_hash = request.payload.hash().as_str().to_owned();
    let native_sequence = sequence_column(request.native_sequence)?;
    let inserted = transaction.execute(
        "INSERT INTO runtime_events
             (project_id, event_kind, agent_run_id, runtime_kind, host, generation, native_id,
              native_event_id, native_sequence, observed_state, contact, freshness, audit_ref,
              payload, payload_hash, observed_at, recorded_at)
         VALUES (?1, 'runtime_observation', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16)",
        params![
            request.project_id.to_string(),
            request.agent_run_id.to_string(),
            request.identity.runtime_kind.as_str(),
            request.identity.host.as_str(),
            generation,
            request.identity.native_id.as_str(),
            request.native_event_id.as_ref().map(ExternalId::as_str),
            native_sequence,
            observed.map(ObservedRunState::as_str),
            normalized.map(|control| control.contact.as_str()),
            normalized.map(|control| control.freshness.as_str()),
            normalized.map(|control| control.audit_ref.as_str()),
            request.payload.json(),
            payload_hash.as_str(),
            text(request.observed_at),
            text(Timestamp::now())
        ],
    );
    let Err(error) = inserted else {
        return Ok((EventCursor::parse(transaction.last_insert_rowid())?, true));
    };

    // A replayed event is not an error: return the cursor it already has, and
    // tell the caller it is a replay so nothing reduces twice.
    let mapped = backend(error);
    if !matches!(mapped, RepositoryError::Conflict { .. }) {
        return Err(mapped);
    }

    // An observation is identified by its continuity identity —
    // `(runtime_kind, host, generation, native_id, native_sequence)` — in exactly
    // the cases the schema recognizes it by: every normalized observation, and
    // every observation the runtime gave no id of its own. Two rows claiming one
    // native sequence in one session are the same observation — unless their
    // payloads differ, in which case the runtime has told us two different things
    // about one moment and neither may be trusted enough to reduce.
    //
    // Identity is never the payload digest. Two distinct observations may say
    // byte-for-byte the same thing, and treating that as a duplicate would drop a
    // real one on the floor.
    if normalized.is_some() || request.native_event_id.is_none() {
        let existing: Option<(i64, String)> = transaction
            .query_row(
                // The same predicate the continuity index is built on, so the
                // lookup can find every row that index could have rejected.
                "SELECT cursor, payload_hash FROM runtime_events
                 WHERE event_kind = 'runtime_observation'
                   AND (contact IS NOT NULL OR native_event_id IS NULL)
                   AND runtime_kind = ?1 AND host = ?2 AND generation = ?3
                   AND native_id = ?4 AND native_sequence = ?5",
                params![
                    request.identity.runtime_kind.as_str(),
                    request.identity.host.as_str(),
                    generation,
                    request.identity.native_id.as_str(),
                    native_sequence
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some(cursor) = same_observation(
            existing,
            &payload_hash,
            "a different observation is already stored for this native sequence",
        )? {
            return Ok((cursor, false));
        }
    }

    // Otherwise the runtime's own event id is the identity, and a repeat of it
    // maps back to the cursor it already has — on the same terms as the continuity
    // identity above, payload included. An event id is a name for one moment, so
    // the same name carrying different bytes is the runtime contradicting itself,
    // not a replay, and it is refused rather than silently answered with the
    // original cursor. With no id and no continuity match above, this conflict is
    // not a duplicate at all, and the error stands.
    //
    // The native session is part of that identity, exactly as it is in the index:
    // an event id is the runtime's own numbering *within one session*, so two
    // sessions of one generation may both call their first event `e-1` without
    // either being a replay of the other.
    let existing: Option<(i64, String)> = if let Some(native) = &request.native_event_id {
        transaction
            .query_row(
                "SELECT cursor, payload_hash FROM runtime_events
                 WHERE event_kind = 'runtime_observation' AND runtime_kind = ?1 AND host = ?2
                   AND generation = ?3 AND native_id = ?4 AND native_event_id = ?5",
                params![
                    request.identity.runtime_kind.as_str(),
                    request.identity.host.as_str(),
                    generation,
                    request.identity.native_id.as_str(),
                    native.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?
    } else {
        None
    };
    match same_observation(
        existing,
        &payload_hash,
        "a different observation is already stored for this native event id",
    )? {
        Some(cursor) => Ok((cursor, false)),
        None => Err(mapped),
    }
}

/// Compare-and-swap the observed and derived halves of a run's projection from
/// one already-stored event.
///
/// The cursor is the row that was just appended, so the projection this writes
/// always cites persisted evidence.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reduce_observation(
    transaction: &Transaction<'_>,
    run: &AgentRun,
    cursor: EventCursor,
    identity: &NativeRuntimeIdentity,
    observed: ObservedRunState,
    observed_at: Timestamp,
    evidence_hash: &ContentHash,
    contact: RuntimeContact,
    freshness: Freshness,
    native_sequence: u64,
) -> RepositoryResult<RunProjection> {
    let observation = RuntimeObservation {
        agent_run_id: run.id,
        state: observed,
        identity: identity.clone(),
        cursor,
        observed_at,
        evidence_hash: evidence_hash.clone(),
    };
    let derived = kontor_core::state::derive_run_state(&RunDerivation {
        desired: run.projection.desired,
        observation: Some(&observation),
        binding: run.binding.as_ref().map(|binding| &binding.identity),
        freshness,
        contact,
        // Closure is a separate, evidence-bearing operation. Reduction never
        // reaches a terminal value, whatever the observation says.
        terminal: None,
    })?;
    let confirmed_at = if derived == DerivedRunState::Confirmed {
        Some(text(observed_at))
    } else {
        run.projection.last_confirmed_at.map(text)
    };
    let next_revision = run.revision.next()?;
    let lifecycle = reduce_run_lifecycle(run.projection.lifecycle, observed);
    let changed = transaction
        .execute(
            "UPDATE agent_runs
             SET lifecycle = ?1, observed_state = ?2, derived_state = ?3,
                 last_confirmed_at = ?4, last_cursor = ?5,
                 last_native_sequence = ?6, revision = ?7
             WHERE project_id = ?8 AND id = ?9 AND revision = ?10",
            params![
                lifecycle.as_str(),
                observed.as_str(),
                derived.as_str(),
                confirmed_at,
                cursor.get(),
                sequence_column(native_sequence)?,
                revision_column(next_revision)?,
                run.project_id.to_string(),
                run.id.to_string(),
                revision_column(run.revision)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "agent run",
            "the run revision moved during the write",
        ));
    }
    reduce_team_lifecycle(transaction, run, observed)?;
    Ok(RunProjection {
        lifecycle,
        desired: run.projection.desired,
        observed,
        derived,
        last_confirmed_at: confirmed_at
            .as_deref()
            .map(crate::repository::read_timestamp)
            .transpose()?,
        last_cursor: Some(cursor),
    })
}

/// Move the owning TeamRun from the same fresh child observation.
///
/// This lives in the observation transaction so `/v1/runs` and the epic view
/// cannot disagree merely because the process stopped between two writes. A
/// team may skip the unobserved `launching` intermediate for the same reason as
/// its child: an exact native report of `running` or `waiting_input` proves the
/// dispatch already happened.
fn reduce_team_lifecycle(
    transaction: &Transaction<'_>,
    run: &AgentRun,
    observed: ObservedRunState,
) -> RepositoryResult<()> {
    let row: Option<(String, i64)> = transaction
        .query_row(
            "SELECT lifecycle, revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
            params![run.project_id.to_string(), run.team_run_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((stored, revision)) = row else {
        return Err(RepositoryError::NotFound {
            subject: "team run",
        });
    };
    let current = RunLifecycle::parse(&stored)?;
    let lifecycle = reduce_run_lifecycle(current, observed);
    if lifecycle == current {
        return Ok(());
    }
    let next_revision = revision.checked_add(1).ok_or(RepositoryError::Backend {
        detail: "team run revision overflow".to_owned(),
    })?;
    let changed = transaction
        .execute(
            "UPDATE team_runs SET lifecycle = ?1, revision = ?2
             WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
            params![
                lifecycle.as_str(),
                next_revision,
                run.project_id.to_string(),
                run.team_run_id.to_string(),
                revision
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "team run",
            "the run revision moved during observation reduction",
        ));
    }
    Ok(())
}

/// The highest native sequence this run has actually reduced.
pub(crate) fn last_reduced_sequence(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    agent_run_id: AgentRunId,
) -> RepositoryResult<Option<u64>> {
    let applied: Option<i64> = transaction
        .query_row(
            "SELECT last_native_sequence FROM agent_runs WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), agent_run_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?
        .flatten();
    Ok(applied.map(|value| u64::try_from(value).unwrap_or_default()))
}

/// Record a forward jump in the runtime's own control sequence.
///
/// Idempotent: the same discontinuity replayed records the same single row.
fn record_control_gap(
    transaction: &Transaction<'_>,
    observation: &ControlObservation,
    expected_sequence: u64,
    detected_cursor: EventCursor,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO runtime_control_gaps
                 (id, project_id, agent_run_id, runtime_kind, host, generation, native_id,
                  expected_sequence, received_sequence, detected_cursor, audit_ref, detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT DO NOTHING",
            params![
                kontor_core::id::generate_uuid_v7().to_string(),
                observation.project_id.to_string(),
                observation.agent_run_id.to_string(),
                observation.identity.runtime_kind.as_str(),
                observation.identity.host.as_str(),
                generation_column(observation.identity.generation)?,
                observation.identity.native_id.as_str(),
                sequence_column(expected_sequence)?,
                sequence_column(observation.native_sequence)?,
                detected_cursor.get(),
                observation.audit_ref.as_str(),
                text(observation.observed_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

impl SqliteStore {
    /// Append one control-plane observation and reduce the projection from it.
    ///
    /// The raw payload and the normalized fields land together, before any
    /// consequence. A duplicate returns the original row's cursor and changes
    /// nothing. An observation that is not strictly newer than the last one this
    /// run reduced, or that was emitted by a session this run is not bound to,
    /// is kept as evidence and applied to nothing.
    ///
    /// # Errors
    /// * [`RepositoryError::Domain`] when the raw payload carries runtime-owned
    ///   session content.
    /// * [`RepositoryError::NotFound`] when the run is not in this project.
    /// * [`RepositoryError::Conflict`] when the run is terminal, its revision
    ///   moved, or a different observation is already stored for this native
    ///   sequence or native event id.
    pub fn append_control_observation(
        &self,
        observation: &ControlObservation,
    ) -> RepositoryResult<ControlObservationOutcome> {
        // The content boundary is checked first, so a rejected transcript never
        // reaches SQL at all.
        observation.ensure_no_session_content()?;

        let transaction = self.begin()?;
        let run = read_agent_run(
            &transaction,
            observation.project_id,
            observation.agent_run_id,
        )?
        .ok_or(RepositoryError::NotFound {
            subject: "agent run",
        })?;
        run.projection.ensure_open("agent run")?;
        run.revision
            .expect("agent run", observation.expected_revision)?;

        let event = NewRuntimeEvent {
            project_id: observation.project_id,
            agent_run_id: observation.agent_run_id,
            identity: observation.identity.clone(),
            native_event_id: observation.native_event_id.clone(),
            native_sequence: observation.native_sequence,
            payload: observation.raw.clone(),
            observed_at: observation.observed_at,
        };
        let (cursor, appended) = append_event(
            &transaction,
            &event,
            Some(observation.observed),
            Some(NormalizedControl {
                contact: observation.contact,
                freshness: observation.freshness,
                audit_ref: &observation.audit_ref,
            }),
        )?;

        if !appended {
            // The evidence and every consequence of it already exist. Committing
            // is correct: this call wrote nothing.
            transaction.commit().map_err(backend)?;
            return Ok(ControlObservationOutcome {
                cursor,
                appended: false,
                reduced: false,
                projection: run.projection,
                control_gap: None,
            });
        }

        // A gap is persisted *before* the observation that revealed it is
        // applied, so the record of what is missing cannot be lost to a crash
        // that keeps the consequence.
        let control_gap = match observation.expected_sequence {
            Some(expected) if observation.native_sequence > expected => {
                record_control_gap(&transaction, observation, expected, cursor)?;
                Some(ControlGap {
                    expected_sequence: expected,
                    received_sequence: observation.native_sequence,
                    detected_cursor: cursor,
                })
            }
            _ => None,
        };

        // Only the run's own bound session may move its observed state, and only
        // with a strictly newer native sequence. Anything else stays evidence.
        let bound = run
            .binding
            .as_ref()
            .is_some_and(|binding| binding.identity.same_session(&observation.identity));
        let last_applied = last_reduced_sequence(
            &transaction,
            observation.project_id,
            observation.agent_run_id,
        )?;
        if !bound || !RunProjection::may_reduce(last_applied, observation.native_sequence) {
            transaction.commit().map_err(backend)?;
            return Ok(ControlObservationOutcome {
                cursor,
                appended: true,
                reduced: false,
                projection: run.projection,
                control_gap,
            });
        }

        // Missing control-plane facts keep the conclusion conservative: an
        // observation that arrived across a hole may still move observed state,
        // but it is not treated as a fresh confirmation until reconciliation
        // supplies the continuity that is missing.
        let freshness = if control_gap.is_some() {
            Freshness::Stale
        } else {
            observation.freshness
        };
        let projection = reduce_observation(
            &transaction,
            &run,
            cursor,
            &observation.identity,
            observation.observed,
            observation.observed_at,
            observation.raw.hash(),
            observation.contact,
            freshness,
            observation.native_sequence,
        )?;
        transaction.commit().map_err(backend)?;

        Ok(ControlObservationOutcome {
            cursor,
            appended: true,
            reduced: true,
            projection,
            control_gap,
        })
    }

    /// Record a discontinuity in the runtime's own **session content**.
    ///
    /// This is a fetch obligation and nothing more. It appends no control-plane
    /// event, moves no cursor, touches no desired, observed, derived or
    /// lifecycle value, and persists no transcript, message, token or delta —
    /// only the binding, the epoch/sequence pair and an opaque reference.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when the run is not in this project.
    /// * [`RepositoryError::Domain`] when the sequences do not describe a
    ///   forward gap.
    pub fn record_content_discontinuity(
        &self,
        discontinuity: &ContentDiscontinuity,
    ) -> RepositoryResult<ContentGapOutcome> {
        if discontinuity.received_sequence <= discontinuity.expected_sequence {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "content discontinuity",
                "the received content sequence is not ahead of the expected one",
            )));
        }
        let transaction = self.begin()?;
        let known: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM agent_runs WHERE project_id = ?1 AND id = ?2",
                params![
                    discontinuity.project_id.to_string(),
                    discontinuity.agent_run_id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if known.is_none() {
            return Err(RepositoryError::NotFound {
                subject: "agent run",
            });
        }

        // The position this was noticed at, in the control-plane cursor space.
        // It is a bookmark, not a claim that a control-plane event is missing.
        let detected_cursor: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(cursor), 1) FROM runtime_events",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO runtime_content_gaps
                     (id, project_id, agent_run_id, content_epoch, expected_content_sequence,
                      received_content_sequence, detected_cursor, audit_ref, detected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT DO NOTHING",
                params![
                    kontor_core::id::generate_uuid_v7().to_string(),
                    discontinuity.project_id.to_string(),
                    discontinuity.agent_run_id.to_string(),
                    sequence_column(discontinuity.content_epoch)?,
                    sequence_column(discontinuity.expected_sequence)?,
                    sequence_column(discontinuity.received_sequence)?,
                    detected_cursor,
                    discontinuity.audit_ref.as_str(),
                    text(discontinuity.detected_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;

        Ok(ContentGapOutcome::TimelineRefetchRequired {
            run: discontinuity.agent_run_id,
            content_epoch: discontinuity.content_epoch,
            expected_sequence: discontinuity.expected_sequence,
            received_sequence: discontinuity.received_sequence,
            audit_ref: discontinuity.audit_ref.clone(),
        })
    }
}

/// Reduce one KON-MVP-03 observation: append the raw event, then apply it.
///
/// Kept as the store's original narrow path — it takes the caller's word for the
/// observed state and stores no normalized contact or freshness — while
/// [`SqliteStore::append_control_observation`] is the evidence-complete one.
pub(crate) fn record_observation(
    store: &SqliteStore,
    request: &NewObservation,
) -> RepositoryResult<RunProjection> {
    let transaction = store.begin()?;
    let project_id = request.event.project_id;
    let run_id = request.event.agent_run_id;
    let run =
        read_agent_run(&transaction, project_id, run_id)?.ok_or(RepositoryError::NotFound {
            subject: "agent run",
        })?;
    run.projection.ensure_open("agent run")?;
    run.revision
        .expect("agent run", request.expected_revision)?;

    // A reducible observation must come from the run's own immutable binding. A
    // different generation or identity is reconciliation input, never an
    // overwrite of this run.
    let binding = run.binding.as_ref().ok_or(DomainError::MissingEvidence {
        subject: "observation",
        rule: "an unbound run has nothing to reduce an observation against",
    })?;
    if !binding.identity.same_session(&request.event.identity) {
        return Err(DomainError::MissingEvidence {
            subject: "observation",
            rule: "the event was not emitted by this run's binding",
        }
        .into());
    }

    // The raw event is appended first; the reduced state is derived from it,
    // never the other way round.
    let (cursor, stored) =
        append_event(&transaction, &request.event, Some(request.observed), None)?;

    // Monotonic protection. A replay, or anything at or behind the highest
    // sequence already applied, leaves the projection *exactly* as it was: no
    // observed/derived change, no cursor move, no revision increment.
    let last_applied = last_reduced_sequence(&transaction, project_id, run_id)?;
    if !stored || !RunProjection::may_reduce(last_applied, request.event.native_sequence) {
        // Committing is correct here: a genuinely new-but-older event has still
        // been appended as evidence, and a replay wrote nothing.
        transaction.commit().map_err(backend)?;
        return Ok(run.projection);
    }

    let projection = reduce_observation(
        &transaction,
        &run,
        cursor,
        &request.event.identity,
        request.observed,
        request.event.observed_at,
        request.event.payload.hash(),
        request.contact,
        request.freshness,
        request.event.native_sequence,
    )?;
    transaction.commit().map_err(backend)?;
    Ok(projection)
}

/// Append one raw runtime event with no reduction at all.
pub(crate) fn append_runtime_event(
    store: &SqliteStore,
    request: &NewRuntimeEvent,
) -> RepositoryResult<EventCursor> {
    let transaction = store.begin()?;
    let (cursor, _) = append_event(&transaction, request, None, None)?;
    transaction.commit().map_err(backend)?;
    Ok(cursor)
}

/// Rebuild a stored runtime event's canonical payload, verifying its digest.
pub(crate) fn stored_payload(json: &str, hash: &str) -> RepositoryResult<CanonicalDocument> {
    let digest = ContentHash::parse(hash)?;
    Ok(CanonicalDocument::from_stored(json, &digest)?)
}
