//! Recording an intent, and claiming it for dispatch.
//!
//! Both operations are single transactions that leave the database in a state a
//! restart can read unambiguously. Recording an intent writes six things at once
//! — receipt, normalized target, outbox entry, desired state, the first
//! transition and the control-plane intent event — so there is no window in
//! which a command exists halfway. Claiming writes a durable token *before*
//! anything is sent, so a crash immediately afterwards still leaves the key a
//! native lookup needs.

use kontor_core::id::{CanonicalDocument, CommandReceiptId, ExternalId, ProjectId, Timestamp};
use kontor_core::receipt::{
    AggregateRef, CommandReceipt, CommandReceiptState, RevisionRule, TargetRule,
};
use kontor_core::repository::{
    CommandRepository, NewCommandIntent, RepositoryError, RepositoryResult,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::SqliteStore;
use crate::commands::receipts::{append_transition, last_transition, read_receipt_row};
use crate::events::append::stored_payload;
use crate::repository::{
    RECEIPT_COLUMNS, backend, conflict, read_timestamp, revision_column, target_columns,
    target_project, text, to_json,
};

/// One claimed dispatch: the payload to send, and the correlation to send it
/// under.
///
/// The correlation is the outbox row's durable claim token. It is written before
/// the caller is told about the entry at all, so however the dispatch ends —
/// success, timeout, or a process that never comes back — the key to ask the
/// runtime "did this already happen?" is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchClaim {
    /// The receipt this dispatch belongs to.
    pub receipt_id: CommandReceiptId,
    /// The durable correlation every attempt of this command shares.
    pub correlation: ExternalId,
    /// The canonical dispatch payload.
    pub payload: CanonicalDocument,
    /// How many claims this entry has had, including this one.
    pub attempts: u32,
    /// When this claim was taken.
    pub claimed_at: Timestamp,
}

/// Prove a reused idempotency key names the *same* durable command.
///
/// The key is the caller's promise that two requests are one command, and the
/// receipt alone cannot check that promise: the dispatch payload and the earliest
/// instant it may be sent live on the outbox row, and a key replayed with either
/// of them changed would be handed a receipt for a command nobody asked for —
/// with the *original* payload still queued. So the whole durable identity is
/// compared, receipt and outbox together, before the original is returned.
///
/// Two fields are deliberately not compared. `desired` needs no comparison of
/// its own: [`kontor_core::receipt::CommandKind::ensure_compatible`] pins it to
/// the kind and target pair, and both of those are compared here, so an equal
/// pair cannot carry a different desired state. `created_at` is not part of the
/// command either — a retry legitimately happens later than the request it
/// repeats.
fn ensure_same_command(
    transaction: &rusqlite::Transaction<'_>,
    existing: &CommandReceipt,
    request: &NewCommandIntent,
) -> RepositoryResult<()> {
    // The key is unique across the whole database, not per project, so a foreign
    // receipt is a possible answer to this lookup and never an acceptable one.
    if existing.project_id != request.project_id {
        return Err(RepositoryError::CrossProject {
            subject: "command receipt",
        });
    }
    existing.ensure_replay(&request.target, &request.intent)?;
    let reused = |rule: &'static str| {
        RepositoryError::Domain(kontor_core::DomainError::invalid("CommandReceipt", rule))
    };
    if existing.kind != request.kind {
        return Err(reused(
            "idempotency key reused with a different command kind",
        ));
    }
    if existing.target_revision != request.target_revision {
        return Err(reused(
            "idempotency key reused against a different target revision",
        ));
    }
    let entry: Option<(String, String)> = transaction
        .query_row(
            "SELECT payload_hash, not_before FROM command_outbox
             WHERE project_id = ?1 AND receipt_id = ?2",
            params![existing.project_id.to_string(), existing.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let (payload_hash, not_before) = entry.ok_or(RepositoryError::NotFound {
        subject: "command outbox entry",
    })?;
    if payload_hash != request.payload.hash().as_str() {
        return Err(reused(
            "idempotency key reused with a different dispatch payload",
        ));
    }
    if not_before != text(request.not_before) {
        return Err(reused(
            "idempotency key reused with a different earliest dispatch instant",
        ));
    }
    Ok(())
}

/// Record an intent, its outbox entry, its desired state and its first
/// transition atomically.
pub(crate) fn record_intent(
    store: &SqliteStore,
    request: &NewCommandIntent,
) -> RepositoryResult<CommandReceipt> {
    // The kind and the target are not two independently supplied facts that
    // happen to be stored next to each other: one constrains the other, and the
    // pair decides both the revision rule and the desired-state change. Refused
    // here, before `begin`, so no effect of the six can survive.
    let rule: TargetRule = request
        .kind
        .ensure_compatible(&request.target, request.desired)?;
    if let Some(project) = target_project(&request.target)
        && project != request.project_id
    {
        return Err(RepositoryError::CrossProject {
            subject: "command target",
        });
    }
    let transaction = store.begin()?;
    let existing: Option<RepositoryResult<CommandReceipt>> = transaction
        .query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM command_receipts WHERE idempotency_key = ?1"),
            params![request.idempotency_key.as_str()],
            |row| Ok(read_receipt_row(row)),
        )
        .optional()
        .map_err(backend)?;
    if let Some(existing) = existing {
        let existing = existing?;
        // A replay of the same durable command returns the original receipt;
        // anything else fails atomically.
        ensure_same_command(&transaction, &existing, request)?;
        return Ok(existing);
    }

    let target = to_json(&request.target)?;
    transaction
        .execute(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'intent_persisted', 0, ?9, ?9)",
            params![
                request.receipt_id.to_string(),
                request.project_id.to_string(),
                request.idempotency_key.as_str(),
                request.kind.as_str(),
                target,
                revision_column(request.target_revision)?,
                request.intent.json(),
                request.intent.hash().as_str(),
                text(request.created_at)
            ],
        )
        .map_err(backend)?;
    // The normalized target row is what makes the canonical target JSON
    // trustworthy: the same reference, expressed relationally, with a composite
    // foreign key that cannot point outside this project.
    let (kind, columns) = target_columns(&request.target);
    transaction
        .execute(
            "INSERT INTO command_targets
                 (project_id, receipt_id, target_kind, target_project_id,
                  target_mini_project_id, target_task_id, target_team_run_id,
                  target_agent_run_id, target_ticket_link_id, target_work_calendar_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                request.project_id.to_string(),
                request.receipt_id.to_string(),
                kind,
                columns[0],
                columns[1],
                columns[2],
                columns[3],
                columns[4],
                columns[5],
                columns[6]
            ],
        )
        .map_err(backend)?;
    transaction
        .execute(
            "INSERT INTO command_outbox
                 (receipt_id, project_id, payload, payload_hash, not_before, attempts)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![
                request.receipt_id.to_string(),
                request.project_id.to_string(),
                request.payload.json(),
                request.payload.hash().as_str(),
                text(request.not_before)
            ],
        )
        .map_err(backend)?;

    // The durable history starts here, in the same transaction: a receipt that
    // exists always has a first transition, so recovery never has to guess what
    // the earliest durable promise was.
    append_transition(
        &transaction,
        request.project_id,
        request.receipt_id,
        1,
        CommandReceiptState::IntentPersisted,
        None,
        None,
        None,
        request.created_at,
    )?;

    // Desired state moves in the same transaction as the intent and the outbox
    // entry. Either all of them exist or none do. Which commands get here is the
    // matrix's decision, not this code's: the rule says compare-and-swap exactly
    // when the target records a desired state and this command moves it.
    if let (RevisionRule::CompareAndSwap, Some(desired), AggregateRef::AgentRun { agent_run_id }) =
        (rule.revision, request.desired, request.target)
    {
        let changed = transaction
            .execute(
                "UPDATE agent_runs SET desired_state = ?1, revision = revision + 1
                 WHERE project_id = ?2 AND id = ?3 AND revision = ?4",
                params![
                    desired.as_str(),
                    request.project_id.to_string(),
                    agent_run_id.to_string(),
                    revision_column(request.target_revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            // Either the run does not exist in this project, or it moved since
            // the intent was computed. Both refuse the whole transaction:
            // intent, outbox entry, transition and desired state are one unit of
            // work.
            return Err(conflict(
                "command target",
                "the target run is unknown in this project or its revision moved",
            ));
        }
    }
    // The intent event is the last effect, and it commits with the others or not
    // at all.
    transaction
        .execute(
            "INSERT INTO runtime_events
                 (project_id, event_kind, command_receipt_id, payload, payload_hash,
                  observed_at, recorded_at)
             VALUES (?1, 'command_intent', ?2, ?3, ?4, ?5, ?6)",
            params![
                request.project_id.to_string(),
                request.receipt_id.to_string(),
                request.intent.json(),
                request.intent.hash().as_str(),
                text(request.created_at),
                text(Timestamp::now())
            ],
        )
        .map_err(backend)?;
    transaction.commit().map_err(backend)?;

    store
        .get_receipt_by_key(&request.idempotency_key)?
        .ok_or(RepositoryError::NotFound {
            subject: "command receipt",
        })
}

/// Read the outbox entries as they stand, without claiming anything.
pub(crate) fn read_outbox(
    store: &SqliteStore,
    project_id: ProjectId,
    now: Timestamp,
    limit: u32,
) -> RepositoryResult<Vec<kontor_core::receipt::CommandOutboxEntry>> {
    let mut statement = store
        .connection
        .prepare(
            "SELECT receipt_id, payload, payload_hash, not_before, dispatched_at, attempts
             FROM command_outbox
             WHERE project_id = ?1 AND dispatched_at IS NULL AND not_before <= ?2
             ORDER BY not_before, receipt_id LIMIT ?3",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), text(now), i64::from(limit)])
        .map_err(backend)?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let payload: String = row.get(1).map_err(backend)?;
        let payload_hash: String = row.get(2).map_err(backend)?;
        let dispatched: Option<String> = row.get(4).map_err(backend)?;
        let attempts: i64 = row.get(5).map_err(backend)?;
        entries.push(kontor_core::receipt::CommandOutboxEntry {
            receipt_id: CommandReceiptId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
            payload: stored_payload(&payload, &payload_hash)?,
            not_before: read_timestamp(&row.get::<_, String>(3).map_err(backend)?)?,
            dispatched_at: dispatched.as_deref().map(read_timestamp).transpose()?,
            attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        });
    }
    Ok(entries)
}

impl SqliteStore {
    /// Claim due outbox entries for dispatch.
    ///
    /// Claiming is a **write**, not a read: it mints and persists the
    /// correlation, advances the receipt from `intent_persisted` to
    /// `dispatch_pending` and appends that transition, all in one transaction.
    /// Two dispatchers racing therefore cannot both come away holding the same
    /// work — the loser finds the receipt has already left `intent_persisted`
    /// and claims nothing.
    ///
    /// A receipt that is already `dispatch_pending` or beyond is deliberately
    /// *not* re-claimable here. Whether it may be sent again is a recovery
    /// question, and recovery answers it with evidence
    /// ([`SqliteStore::classify_command_recovery`]), never with a lease that
    /// happened to expire.
    ///
    /// # Errors
    /// * [`RepositoryError::Conflict`] when an entry moved during the claim.
    /// * [`RepositoryError::Backend`] on backend failure.
    pub fn claim_due(
        &self,
        project_id: ProjectId,
        now: Timestamp,
        limit: u32,
    ) -> RepositoryResult<Vec<DispatchClaim>> {
        let transaction = self.begin()?;
        let due: Vec<(String, String, String, i64)> = {
            let mut statement = transaction
                .prepare(
                    "SELECT outbox.receipt_id, outbox.payload, outbox.payload_hash, outbox.attempts
                     FROM command_outbox AS outbox
                     JOIN command_receipts AS receipt
                       ON receipt.id = outbox.receipt_id AND receipt.project_id = outbox.project_id
                     WHERE outbox.project_id = ?1 AND outbox.dispatched_at IS NULL
                       AND outbox.not_before <= ?2 AND receipt.state = 'intent_persisted'
                     ORDER BY outbox.not_before, outbox.receipt_id LIMIT ?3",
                )
                .map_err(backend)?;
            let mut rows = statement
                .query(params![project_id.to_string(), text(now), i64::from(limit)])
                .map_err(backend)?;
            let mut due = Vec::new();
            while let Some(row) = rows.next().map_err(backend)? {
                due.push((
                    row.get(0).map_err(backend)?,
                    row.get(1).map_err(backend)?,
                    row.get(2).map_err(backend)?,
                    row.get(3).map_err(backend)?,
                ));
            }
            due
        };

        let mut claims = Vec::with_capacity(due.len());
        for (receipt_id, payload, payload_hash, attempts) in due {
            let receipt_id = CommandReceiptId::parse(&receipt_id)?;
            // The correlation is minted here and persisted before the caller is
            // told anything, so it exists on disk before any native call can.
            let correlation = ExternalId::parse(&Uuid::now_v7().to_string())?;
            let claimed = transaction
                .execute(
                    "UPDATE command_outbox
                     SET claim_token = ?1, claimed_at = ?2, attempts = attempts + 1
                     WHERE project_id = ?3 AND receipt_id = ?4
                       AND claim_token IS NULL AND dispatched_at IS NULL",
                    params![
                        correlation.as_str(),
                        text(now),
                        project_id.to_string(),
                        receipt_id.to_string()
                    ],
                )
                .map_err(backend)?;
            if claimed != 1 {
                return Err(conflict(
                    "command outbox",
                    "the entry was claimed by another dispatcher",
                ));
            }
            let advanced = transaction
                .execute(
                    "UPDATE command_receipts
                     SET state = 'dispatch_pending', correlation = ?1, updated_at = ?2
                     WHERE project_id = ?3 AND id = ?4 AND state = 'intent_persisted'",
                    params![
                        correlation.as_str(),
                        text(now),
                        project_id.to_string(),
                        receipt_id.to_string()
                    ],
                )
                .map_err(backend)?;
            if advanced != 1 {
                return Err(conflict(
                    "command receipt",
                    "the receipt left intent_persisted during the claim",
                ));
            }
            let sequence = last_transition(&transaction, project_id, receipt_id)?
                .map_or(1, |(sequence, _)| sequence + 1);
            append_transition(
                &transaction,
                project_id,
                receipt_id,
                sequence,
                CommandReceiptState::DispatchPending,
                Some(&correlation),
                None,
                None,
                now,
            )?;
            claims.push(DispatchClaim {
                receipt_id,
                correlation,
                payload: stored_payload(&payload, &payload_hash)?,
                attempts: u32::try_from(attempts + 1).unwrap_or(u32::MAX),
                claimed_at: now,
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(claims)
    }
}
