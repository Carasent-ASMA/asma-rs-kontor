//! Receipt transitions, their durable history, and what a restart may conclude
//! from them.
//!
//! Acknowledgement and confirmation are different facts and are stored as
//! different rows. An acknowledgement says the target received the command; it
//! carries no evidence and never settles anything. A confirmation says the
//! effect was independently observed, and may not be recorded without a
//! reference to the proof.
//!
//! Between the two sits the state this module is really built around:
//! `confirmation_unknown`. A command in it may have taken effect. Every route
//! out of it either finds the effect (confirm) or proves its absence
//! ([`kontor_core::receipt::NoEffectEvidence`]) — and no route out of it is
//! opened by a restart, an expired lease or a quiet retry.

use kontor_core::DomainError;
use kontor_core::id::{
    CanonicalDocument, CommandReceiptId, ContentHash, ExternalId, ExternalName, ProjectId,
    RuntimeKindKey, Timestamp,
};
use kontor_core::receipt::{CommandReceipt, CommandReceiptState, NoEffectEvidence};
use kontor_core::repository::{ReceiptAdvance, RepositoryError, RepositoryResult};
use kontor_core::state::NativeRuntimeIdentity;
use rusqlite::{OptionalExtension, Row, params};

use crate::SqliteStore;
use crate::events::append::stored_payload;
use crate::repository::{
    RECEIPT_COLUMNS, backend, conflict, from_json, read_timestamp, revision_of, text, to_json,
};

/// Rebuild a command receipt from a `RECEIPT_COLUMNS` row.
pub(crate) fn read_receipt_row(row: &Row<'_>) -> RepositoryResult<CommandReceipt> {
    let intent: String = row.get(6).map_err(backend)?;
    let intent_hash: String = row.get(7).map_err(backend)?;
    let digest = ContentHash::parse(&intent_hash)?;
    let native: Option<String> = row.get(10).map_err(backend)?;
    let correlation: Option<String> = row.get(9).map_err(backend)?;
    let result_ref: Option<String> = row.get(11).map_err(backend)?;
    let attempts: i64 = row.get(12).map_err(backend)?;
    Ok(CommandReceipt {
        id: CommandReceiptId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        idempotency_key: kontor_core::id::IdempotencyKey::parse(
            &row.get::<_, String>(2).map_err(backend)?,
        )?,
        kind: kontor_core::receipt::CommandKind::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        target: from_json(&row.get::<_, String>(4).map_err(backend)?)?,
        target_revision: revision_of(row.get::<_, i64>(5).map_err(backend)?)?,
        intent: CanonicalDocument::from_stored(&intent, &digest)?,
        state: CommandReceiptState::parse(&row.get::<_, String>(8).map_err(backend)?)?,
        correlation: correlation.as_deref().map(ExternalId::parse).transpose()?,
        native_identity: native
            .map(|json| from_json::<NativeRuntimeIdentity>(&json))
            .transpose()?,
        result_ref: result_ref.as_deref().map(ExternalId::parse).transpose()?,
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        created_at: read_timestamp(&row.get::<_, String>(13).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(14).map_err(backend)?)?,
    })
}

/// The newest durable transition of one receipt, as `(sequence, state)`.
pub(crate) fn last_transition(
    transaction: &rusqlite::Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
) -> RepositoryResult<Option<(u32, CommandReceiptState)>> {
    let row: Option<(i64, String)> = transaction
        .query_row(
            "SELECT sequence, state FROM command_receipt_transitions
             WHERE project_id = ?1 AND receipt_id = ?2
             ORDER BY sequence DESC LIMIT 1",
            params![project_id.to_string(), receipt_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    row.map(|(sequence, state)| {
        Ok((
            u32::try_from(sequence).unwrap_or(u32::MAX),
            CommandReceiptState::parse(&state)?,
        ))
    })
    .transpose()
}

/// Append one transition to a receipt's immutable history.
#[allow(clippy::too_many_arguments)]
pub(crate) fn append_transition(
    transaction: &rusqlite::Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
    sequence: u32,
    state: CommandReceiptState,
    correlation: Option<&ExternalId>,
    native_identity: Option<&NativeRuntimeIdentity>,
    evidence_ref: Option<&ExternalId>,
    recorded_at: Timestamp,
) -> RepositoryResult<()> {
    let native = native_identity.map(to_json).transpose()?;
    transaction
        .execute(
            "INSERT INTO command_receipt_transitions
                 (project_id, receipt_id, sequence, state, correlation, native_identity,
                  evidence_ref, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                project_id.to_string(),
                receipt_id.to_string(),
                i64::from(sequence),
                state.as_str(),
                correlation.map(ExternalId::as_str),
                native,
                evidence_ref.map(ExternalId::as_str),
                text(recorded_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

/// One durable step in a receipt's history, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptTransition {
    /// Position in this receipt's history, from 1.
    pub sequence: u32,
    /// The state it moved to.
    pub state: CommandReceiptState,
    /// The correlation in force at that point.
    pub correlation: Option<ExternalId>,
    /// The evidence cited, for the states that require it.
    pub evidence_ref: Option<ExternalId>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// One evidence-bearing step of the command protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTransition {
    /// Owning project.
    pub project_id: ProjectId,
    /// The receipt to move.
    pub receipt_id: CommandReceiptId,
    /// The state to move to.
    pub to: CommandReceiptState,
    /// The dispatch correlation, when this step establishes or repeats one.
    pub correlation: Option<ExternalId>,
    /// The native identity the command created or addressed.
    pub native_identity: Option<NativeRuntimeIdentity>,
    /// Where the proof of confirmation or failure is recorded. Required for
    /// both, and forbidden on an acknowledgement.
    pub evidence_ref: Option<ExternalId>,
    /// Proof that a command with an unknown result had no effect. The only key
    /// that unlocks another dispatch.
    pub no_effect: Option<NoEffectEvidence>,
    /// When the step happened.
    pub occurred_at: Timestamp,
}

/// The result of one protocol step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedTransition {
    /// The receipt projection after the step.
    pub receipt: CommandReceipt,
    /// The sequence of the newest durable transition.
    pub sequence: u32,
    /// Whether this call appended one. A repeat of the state the receipt is
    /// already in returns the existing projection and appends nothing.
    pub appended: bool,
}

/// What a restart may do about one command.
///
/// Exactly one variant authorizes a launch, and it is the one that proves
/// nothing was ever sent. The others describe commands that may already have
/// taken effect; the only way past them is a native lookup keyed by the
/// persisted correlation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandRecovery {
    /// The intent is durable and nothing has left this process. Safe to
    /// dispatch.
    Undispatched {
        /// The receipt.
        receipt_id: CommandReceiptId,
        /// The payload to send.
        payload: CanonicalDocument,
    },
    /// A dispatch was, or may have been, made. Whether it took effect is a
    /// question for the runtime, not for this process.
    AmbiguousOrLaunched {
        /// The receipt.
        receipt_id: CommandReceiptId,
        /// How far it got before contact was lost.
        state: CommandReceiptState,
        /// The correlation to look the command up by.
        correlation: Option<ExternalId>,
        /// The native identity, once one is known.
        native_identity: Option<NativeRuntimeIdentity>,
        /// The original payload, so a proven-safe retry sends the same command.
        payload: CanonicalDocument,
    },
    /// The command is settled and a restart changes nothing about it.
    Settled {
        /// The receipt.
        receipt_id: CommandReceiptId,
        /// Its final state.
        state: CommandReceiptState,
    },
}

/// What durable launch evidence let startup settle without guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaunchReceiptReconciliation {
    /// Launches whose native binding proves the session exists.
    pub confirmed: usize,
    /// Launches whose unbound run closed with terminal evidence.
    pub failed: usize,
    /// Open, unbound launches that remain real dispatch obligations.
    pub pending: usize,
}

impl CommandRecovery {
    /// Whether a fresh native launch is authorized right now.
    ///
    /// It is `true` in exactly one case: the intent is durable and provably
    /// nothing was ever sent. A restarted process, an expired lease and a
    /// forgotten acknowledgement are all `false`, because none of them is
    /// evidence about the runtime.
    #[must_use]
    pub const fn authorizes_launch(&self) -> bool {
        matches!(self, Self::Undispatched { .. })
    }

    /// The correlation a native lookup must ask about, when there is one.
    #[must_use]
    pub const fn correlation(&self) -> Option<&ExternalId> {
        match self {
            Self::AmbiguousOrLaunched { correlation, .. } => correlation.as_ref(),
            Self::Undispatched { .. } | Self::Settled { .. } => None,
        }
    }
}

/// Move a receipt forward through the one protocol there is.
///
/// [`kontor_core::repository::CommandRepository::advance_receipt`] predates the
/// durable receipt history, and its request shape is the only thing left of it:
/// it is translated here and applied by
/// [`SqliteStore::apply_command_transition`], so the trait cannot be used as a
/// side door that moves a receipt without evidence or without appending the
/// history a restart reads. `result_ref` is the transition's `evidence_ref` —
/// the same field under the older name.
pub(crate) fn advance_receipt(
    store: &SqliteStore,
    request: &ReceiptAdvance,
) -> RepositoryResult<CommandReceipt> {
    store
        .apply_command_transition(&CommandTransition {
            project_id: request.project_id,
            receipt_id: request.receipt_id,
            to: request.to,
            correlation: request.correlation.clone(),
            native_identity: request.native_identity.clone(),
            evidence_ref: request.result_ref.clone(),
            no_effect: request.no_effect.clone(),
            occurred_at: request.occurred_at,
        })
        .map(|recorded| recorded.receipt)
}

/// The durable claim token of one receipt's outbox entry.
///
/// It is the correlation the command is dispatched under, written once by the
/// claim and never rewritten.
fn claim_token(
    transaction: &rusqlite::Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
) -> RepositoryResult<Option<ExternalId>> {
    let stored: Option<Option<String>> = transaction
        .query_row(
            "SELECT claim_token FROM command_outbox WHERE project_id = ?1 AND receipt_id = ?2",
            params![project_id.to_string(), receipt_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    Ok(stored
        .flatten()
        .as_deref()
        .map(ExternalId::parse)
        .transpose()?)
}

/// The correlation this step is bound to, proved against every durable record of
/// it.
///
/// The correlation is minted once, by the outbox claim, and never rewritten. A
/// caller supplying a different one is not updating a field: it is asking
/// recovery to look this command up under a token the runtime was never told
/// about, which is exactly how an already-executed command gets missed and sent
/// twice. Whatever correlation is left in force must then be the outbox's
/// immutable claim token — the receipt, its history and the claim are three
/// records of one fact, and recovery reads the first two to ask the runtime about
/// the third.
fn effective_correlation(
    transaction: &rusqlite::Transaction<'_>,
    request: &CommandTransition,
    receipt: &CommandReceipt,
) -> RepositoryResult<Option<ExternalId>> {
    if let (Some(supplied), Some(durable)) =
        (request.correlation.as_ref(), receipt.correlation.as_ref())
        && supplied != durable
    {
        return Err(conflict(
            "command receipt",
            "the dispatch correlation is persisted once and is never replaced",
        ));
    }
    let correlation = request
        .correlation
        .clone()
        .or_else(|| receipt.correlation.clone());
    if let Some(correlation) = correlation.as_ref()
        && claim_token(transaction, request.project_id, request.receipt_id)?.as_ref()
            != Some(correlation)
    {
        return Err(conflict(
            "command receipt",
            "the correlation is not the outbox entry's durable claim token",
        ));
    }
    Ok(correlation)
}

/// Load one receipt inside a transaction.
fn load_receipt(
    transaction: &rusqlite::Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
) -> RepositoryResult<CommandReceipt> {
    let existing: Option<RepositoryResult<CommandReceipt>> = transaction
        .query_row(
            &format!(
                "SELECT {RECEIPT_COLUMNS} FROM command_receipts
                 WHERE project_id = ?1 AND id = ?2"
            ),
            params![project_id.to_string(), receipt_id.to_string()],
            |row| Ok(read_receipt_row(row)),
        )
        .optional()
        .map_err(backend)?;
    existing.ok_or(RepositoryError::NotFound {
        subject: "command receipt",
    })?
}

/// Compare-and-swap the receipt projection on its current state.
#[allow(clippy::too_many_arguments)]
fn write_projection(
    transaction: &rusqlite::Transaction<'_>,
    receipt: &CommandReceipt,
    next: CommandReceiptState,
    correlation: Option<&str>,
    native_identity: Option<&str>,
    result_ref: Option<&str>,
    attempts: u32,
    occurred_at: Timestamp,
) -> RepositoryResult<()> {
    let changed = transaction
        .execute(
            "UPDATE command_receipts
             SET state = ?1, correlation = ?2, native_identity = ?3, result_ref = ?4,
                 attempts = ?5, updated_at = ?6
             WHERE project_id = ?7 AND id = ?8 AND state = ?9",
            params![
                next.as_str(),
                correlation,
                native_identity,
                result_ref,
                i64::from(attempts),
                text(occurred_at),
                receipt.project_id.to_string(),
                receipt.id.to_string(),
                receipt.state.as_str()
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "command receipt",
            "the receipt state moved during the write",
        ));
    }
    Ok(())
}

impl SqliteStore {
    /// Read one receipt by its project-scoped id.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Backend`] on backend failure.
    pub fn get_receipt(
        &self,
        project_id: ProjectId,
        receipt_id: CommandReceiptId,
    ) -> RepositoryResult<Option<CommandReceipt>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {RECEIPT_COLUMNS} FROM command_receipts
                     WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), receipt_id.to_string()],
                |row| Ok(read_receipt_row(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    /// Every durable transition of one receipt, oldest first.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Backend`] on backend failure.
    pub fn receipt_history(
        &self,
        project_id: ProjectId,
        receipt_id: CommandReceiptId,
    ) -> RepositoryResult<Vec<ReceiptTransition>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT sequence, state, correlation, evidence_ref, recorded_at
                 FROM command_receipt_transitions
                 WHERE project_id = ?1 AND receipt_id = ?2 ORDER BY sequence",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), receipt_id.to_string()])
            .map_err(backend)?;
        let mut history = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let correlation: Option<String> = row.get(2).map_err(backend)?;
            let evidence: Option<String> = row.get(3).map_err(backend)?;
            history.push(ReceiptTransition {
                sequence: u32::try_from(row.get::<_, i64>(0).map_err(backend)?).unwrap_or(u32::MAX),
                state: CommandReceiptState::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                correlation: correlation.as_deref().map(ExternalId::parse).transpose()?,
                evidence_ref: evidence.as_deref().map(ExternalId::parse).transpose()?,
                recorded_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
            });
        }
        Ok(history)
    }

    /// Apply one evidence-bearing protocol step to a receipt.
    ///
    /// The projection and the history row are written in the same transaction,
    /// so the two can never disagree about what was promised. Repeating the step
    /// a receipt is already in returns the existing projection and appends
    /// nothing — but only once the correlation it carries has been proved against
    /// the persisted one and the outbox claim, because a repeat is still a claim
    /// about which correlation the command was sent under. An out-of-order or
    /// illegal step fails and moves nothing backwards.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when the receipt is not in this project.
    /// * [`RepositoryError::Domain`] when confirmation or failure cites no
    ///   evidence, when an acknowledgement claims some, when a dispatch has no
    ///   persisted correlation, or when the transition is illegal.
    /// * [`RepositoryError::Conflict`] when the receipt moved during the write
    ///   or outside this protocol, and when the supplied correlation is neither
    ///   the persisted one nor the outbox entry's claim token.
    pub fn apply_command_transition(
        &self,
        request: &CommandTransition,
    ) -> RepositoryResult<RecordedTransition> {
        // The shape of the claim is judged before anything is looked up, so an
        // acknowledgement dressed as a confirmation is refused whether or not
        // the receipt happens to be there already.
        match request.to {
            CommandReceiptState::Confirmed | CommandReceiptState::Failed
                if request.evidence_ref.is_none() =>
            {
                return Err(DomainError::MissingEvidence {
                    subject: "command confirmation",
                    rule: "a confirmed or failed command must cite the evidence for it",
                }
                .into());
            }
            CommandReceiptState::Acknowledged if request.evidence_ref.is_some() => {
                return Err(DomainError::invalid(
                    "command acknowledgement",
                    "an acknowledgement proves receipt only and carries no evidence",
                )
                .into());
            }
            _ => {}
        }

        let transaction = self.begin()?;
        let receipt = load_receipt(&transaction, request.project_id, request.receipt_id)?;
        let (sequence, recorded) =
            last_transition(&transaction, request.project_id, request.receipt_id)?.ok_or(
                RepositoryError::Conflict {
                    subject: "command receipt",
                    rule: "the receipt has no durable transition history to continue from",
                },
            )?;
        if recorded != receipt.state {
            return Err(conflict(
                "command receipt",
                "the receipt projection and its durable history disagree",
            ));
        }

        // The correlation is proved before anything is concluded from the state,
        // including whether this step is a repeat. A same-state replay is still a
        // claim about which correlation this command was sent under, and one
        // carrying an invented token must be refused rather than handed the
        // original receipt as though the two had agreed.
        let correlation = effective_correlation(&transaction, request, &receipt)?;

        // A repeated step is not progress and not an error: the caller is
        // resuming, and the durable answer is the one already recorded.
        if request.to == receipt.state {
            transaction.commit().map_err(backend)?;
            return Ok(RecordedTransition {
                receipt,
                sequence,
                appended: false,
            });
        }

        let next = if request.to == CommandReceiptState::DispatchPending
            && receipt.state == CommandReceiptState::ConfirmationUnknown
        {
            // The only door out of an unknown result, and it needs a key: proof
            // that the original correlation had no effect.
            let evidence = request
                .no_effect
                .as_ref()
                .ok_or(DomainError::MissingEvidence {
                    subject: "command retry",
                    rule: "an unknown dispatch result must be reconciled before retrying",
                })?;
            receipt.authorize_retry(evidence)?
        } else {
            receipt.transition(request.to)?
        };

        if matches!(
            next,
            CommandReceiptState::DispatchPending | CommandReceiptState::Dispatched
        ) && correlation.is_none()
        {
            return Err(DomainError::MissingEvidence {
                subject: "command dispatch",
                rule: "the correlation must be persisted before any native call",
            }
            .into());
        }

        let attempts = if next == CommandReceiptState::Dispatched {
            receipt.attempts.saturating_add(1)
        } else {
            receipt.attempts
        };
        let native = request
            .native_identity
            .as_ref()
            .map(to_json)
            .transpose()?
            .or(receipt.native_identity.as_ref().map(to_json).transpose()?);
        let result_ref = request
            .evidence_ref
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .or_else(|| {
                receipt
                    .result_ref
                    .as_ref()
                    .map(|value| value.as_str().to_owned())
            });
        write_projection(
            &transaction,
            &receipt,
            next,
            correlation.as_ref().map(ExternalId::as_str),
            native.as_deref(),
            result_ref.as_deref(),
            attempts,
            request.occurred_at,
        )?;
        append_transition(
            &transaction,
            request.project_id,
            request.receipt_id,
            sequence + 1,
            next,
            correlation.as_ref(),
            request.native_identity.as_ref(),
            request.evidence_ref.as_ref(),
            request.occurred_at,
        )?;
        if next == CommandReceiptState::Dispatched {
            transaction
                .execute(
                    "UPDATE command_outbox SET dispatched_at = ?1
                     WHERE project_id = ?2 AND receipt_id = ?3",
                    params![
                        text(request.occurred_at),
                        request.project_id.to_string(),
                        request.receipt_id.to_string()
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;

        let receipt = self
            .get_receipt(request.project_id, request.receipt_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "command receipt",
            })?;
        Ok(RecordedTransition {
            receipt,
            sequence: sequence + 1,
            appended: true,
        })
    }

    /// Settle legacy launch receipts from evidence already durable in Kontor.
    ///
    /// A binding proves the launch took effect. A closed, unbound run with a
    /// terminal evidence hash proves the launch can no longer take effect. An
    /// open unbound run proves neither and remains pending. No clock or absence
    /// of activity is treated as evidence.
    pub fn reconcile_legacy_launch_receipts(
        &self,
        reconciled_at: Timestamp,
    ) -> RepositoryResult<LaunchReceiptReconciliation> {
        type Candidate = (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let candidates: Vec<Candidate> = {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT receipt.project_id, receipt.id, receipt.state, receipt.correlation,
                            binding.id, binding.runtime_kind, binding.host, binding.generation,
                            binding.native_id, run.closed_at, run.terminal_evidence_hash
                     FROM command_receipts AS receipt
                     JOIN command_targets AS target
                       ON target.project_id = receipt.project_id
                      AND target.receipt_id = receipt.id
                     JOIN agent_runs AS run
                       ON run.project_id = receipt.project_id
                      AND run.id = target.target_agent_run_id
                     LEFT JOIN runtime_bindings AS binding
                       ON binding.project_id = run.project_id
                      AND binding.agent_run_id = run.id
                     WHERE receipt.execution_mode = 'dispatch'
                       AND receipt.kind = 'launch_run'
                       AND receipt.state NOT IN ('confirmed', 'failed')
                     ORDER BY receipt.created_at, receipt.id",
                )
                .map_err(backend)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                })
                .map_err(backend)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(backend)?
        };

        let mut report = LaunchReceiptReconciliation::default();
        for (
            project,
            receipt,
            state,
            correlation,
            binding_id,
            runtime_kind,
            host,
            generation,
            native_id,
            closed_at,
            terminal_evidence,
        ) in candidates
        {
            let project_id = ProjectId::parse(&project)?;
            let receipt_id = CommandReceiptId::parse(&receipt)?;
            let state = CommandReceiptState::parse(&state)?;
            let correlation = correlation.as_deref().map(ExternalId::parse).transpose()?;
            let binding = match (runtime_kind, host, generation, native_id) {
                (Some(runtime_kind), Some(host), Some(generation), Some(native_id)) => {
                    Some(NativeRuntimeIdentity {
                        runtime_kind: RuntimeKindKey::parse(&runtime_kind)?,
                        host: ExternalName::parse(&host)?,
                        generation: u64::try_from(generation).map_err(|_| {
                            RepositoryError::Backend {
                                detail: "a runtime binding generation is negative".to_owned(),
                            }
                        })?,
                        native_id: ExternalId::parse(&native_id)?,
                    })
                }
                (None, None, None, None) => None,
                _ => {
                    return Err(RepositoryError::Backend {
                        detail: "a runtime binding is only partially stored".to_owned(),
                    });
                }
            };

            if let Some(binding) = binding {
                if state == CommandReceiptState::DispatchPending {
                    self.apply_command_transition(&CommandTransition {
                        project_id,
                        receipt_id,
                        to: CommandReceiptState::Dispatched,
                        correlation: correlation.clone(),
                        native_identity: Some(binding.clone()),
                        evidence_ref: None,
                        no_effect: None,
                        occurred_at: reconciled_at,
                    })?;
                }
                let evidence =
                    ExternalId::parse(binding_id.as_deref().ok_or(RepositoryError::Backend {
                        detail: "a loaded runtime binding has no binding id".to_owned(),
                    })?)?;
                self.apply_command_transition(&CommandTransition {
                    project_id,
                    receipt_id,
                    to: CommandReceiptState::Confirmed,
                    correlation,
                    native_identity: Some(binding),
                    evidence_ref: Some(evidence),
                    no_effect: None,
                    occurred_at: reconciled_at,
                })?;
                report.confirmed += 1;
                continue;
            }

            if closed_at.is_some()
                && let Some(evidence) = terminal_evidence
            {
                self.apply_command_transition(&CommandTransition {
                    project_id,
                    receipt_id,
                    to: CommandReceiptState::Failed,
                    correlation,
                    native_identity: None,
                    evidence_ref: Some(ExternalId::parse(&evidence)?),
                    no_effect: None,
                    occurred_at: reconciled_at,
                })?;
                report.failed += 1;
            } else {
                report.pending += 1;
            }
        }
        Ok(report)
    }

    /// Decide what a restart may do about one command.
    ///
    /// The answer comes from the durable receipt and nothing else — not from how
    /// long ago the process started, not from a lease, not from the absence of a
    /// reply. `intent_persisted` is the single state that authorizes a launch;
    /// every state after it requires a native lookup by the persisted
    /// correlation before anything may be sent again.
    ///
    /// A receipt with no outbox entry is settled, not missing. Not every receipt
    /// is a dispatch: a closure receipt records a decision Kontor already
    /// carried out, and deliberately queues nothing. The outbox row is written in
    /// the same transaction as the receipt it belongs to, so its absence can
    /// never be a torn write — it can only mean there was nothing to send. This
    /// used to raise, and one abandoned run was enough to fail the whole startup
    /// inventory and leave the realm serving with scheduling shut.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] when the receipt is not in this
    /// project.
    pub fn classify_command_recovery(
        &self,
        project_id: ProjectId,
        receipt_id: CommandReceiptId,
    ) -> RepositoryResult<CommandRecovery> {
        let receipt =
            self.get_receipt(project_id, receipt_id)?
                .ok_or(RepositoryError::NotFound {
                    subject: "command receipt",
                })?;
        let execution_mode: String = self
            .connection
            .query_row(
                "SELECT execution_mode FROM command_receipts
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), receipt_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if execution_mode == "local" {
            return Ok(CommandRecovery::Settled {
                receipt_id,
                state: receipt.state,
            });
        }
        if receipt.state.is_terminal() {
            return Ok(CommandRecovery::Settled {
                receipt_id,
                state: receipt.state,
            });
        }
        let entry: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT payload, payload_hash FROM command_outbox
                 WHERE project_id = ?1 AND receipt_id = ?2",
                params![project_id.to_string(), receipt_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((payload, payload_hash)) = entry else {
            return Ok(CommandRecovery::Settled {
                receipt_id,
                state: receipt.state,
            });
        };
        let payload = stored_payload(&payload, &payload_hash)?;

        match receipt.state {
            CommandReceiptState::IntentPersisted => Ok(CommandRecovery::Undispatched {
                receipt_id,
                payload,
            }),
            // Dispatch pending, dispatched, acknowledged and confirmation
            // unknown are all "we may already have launched". The process
            // restarting tells us nothing new about any of them.
            state => Ok(CommandRecovery::AmbiguousOrLaunched {
                receipt_id,
                state,
                correlation: receipt.correlation.clone(),
                native_identity: receipt.native_identity.clone(),
                payload,
            }),
        }
    }
}

impl SqliteStore {
    /// Every command receipt in this Realm that is not yet settled, oldest first.
    ///
    /// This is the *inventory* a restart recovers from, and it is deliberately
    /// only that: what may be done about each one is
    /// [`SqliteStore::classify_command_recovery`]'s answer, read from the durable
    /// receipt. Listing a receipt here is never permission to dispatch it.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Backend`] on backend failure.
    pub fn unsettled_receipts(&self) -> RepositoryResult<Vec<(ProjectId, CommandReceiptId)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT project_id, id FROM command_receipts
                 WHERE execution_mode = 'dispatch'
                   AND state NOT IN ('confirmed', 'failed')
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut receipts = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            receipts.push((
                ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                CommandReceiptId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
            ));
        }
        Ok(receipts)
    }
}
