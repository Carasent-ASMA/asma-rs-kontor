//! Guardrail evidence and parked recovery, persisted.
//!
//! `kontor-policy` decides; this module records what was decided and applies the
//! consequences. The split matters: every function here writes, and none of them
//! re-decides. A verdict arrives already made, and the store's job is to make it
//! and its consequences durable together or not at all.
//!
//! ## The one transaction that has to be one transaction
//!
//! [`SqliteStore::record_gate_rejection`] appends a rejection and, when that
//! rejection is the reviewer's second on that gate, parks the work — appending
//! the guardrail evaluation, closing the run with receipt-backed evidence,
//! moving the task to `parked` and opening the recovery episode inside the same
//! `BEGIN IMMEDIATE`. There is deliberately no window between those steps for a
//! responsible role to be launched into work that is already parked, and a crash
//! at any point leaves either all of it or none of it.
//!
//! The count is derived here, from `task_gate_evaluations`, and never taken from
//! the caller. A caller that believes the count is one when the history says two
//! parks anyway.
//!
//! ## How a parked run is closed
//!
//! Schema v1 froze which evidence may close a run: a trusted runtime observation
//! or an explicit receipt-backed closure. No runtime ever reports "parked", so
//! `TerminalOutcome::Parked` has no admissible evidence and never will — and
//! SQLite cannot widen a `CHECK` through `ALTER TABLE`, so v3 could not add one
//! without rebuilding `agent_runs` and every table that references it.
//!
//! It does not need to. The domain already separates the two dimensions:
//! [`TerminalOutcome::Abandoned`] means *closed without a runtime verdict*, and
//! [`kontor_core::state::TerminalOutcome::lifecycle`] maps it to
//! [`kontor_core::state::RunLifecycle::Parked`]. So a guardrail park closes the
//! run through the route v1 admits — a receipt-backed closure whose receipt
//! records the park decision — and the run's lifecycle *is* `parked`, with
//! `abandoned` recording that no runtime pronounced on it.
//!
//! What that route cannot say on its own is *who* abandoned it, and a guardrail
//! park is not a human decision. `run_park_closures` is what says so: it names
//! the evaluation that caused the park, the episode that owns the recovery and
//! the receipt, all by composite foreign key. Nothing anywhere fabricates a
//! runtime observation.

use kontor_core::DomainError;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CanonicalDocument,
    CommandReceiptId, ContentHash, ExternalId, GateKey, GuardrailEvaluationId, IdempotencyKey,
    ProjectId, RoleKey, SpecVersion, TaskId, TaskWorkflowId, TeamRunId, Timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind, CommandReceiptState};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use kontor_core::state::{
    GateVerdict, TaskState, TaskTransition, TerminalEvidence, TerminalEvidenceSource,
    TerminalOutcome, apply_task_transition,
};
use kontor_policy::model::{
    ApprovalReceipt, ApprovalReceiptId, ArtifactEvidenceId, AuthoritySource, EscalationCause,
    GateWaiverId, GuardrailEvaluation, GuardrailRuleKey, PolicyVerdict, ReasonCode,
    RecoveryEpisode, RecoveryEpisodeId, RecoveryStatus, RecoveryStepKind,
};
use kontor_policy::recovery::{RecoveryRequest, RecoveryTransition};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::SqliteStore;
use crate::commands::receipts::append_transition;
use crate::repository::{
    backend, conflict, read_timestamp, revision_column, revision_of, text, to_json,
};

/// The reason code a run's park closure is filed under.
const PARKED_AUTO_TRIAGE: &str = "parked_auto_triage";

// ---------------------------------------------------------------------------
// Request and result types
// ---------------------------------------------------------------------------

/// Which task, workflow and run an evaluation was made about.
///
/// The evaluation itself carries its subject; this is the ownership that binds
/// it, by composite foreign key, into one project's records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationBinding {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// Its workflow.
    pub workflow_id: TaskWorkflowId,
    /// The team run, when there was one.
    pub team_run_id: Option<TeamRunId>,
    /// The agent run, when there was one.
    pub agent_run_id: Option<AgentRunId>,
}

/// One artifact-evidence record to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArtifactEvidence {
    /// The record id.
    pub id: ArtifactEvidenceId,
    /// Where it belongs.
    pub binding: EvaluationBinding,
    /// The artifact contract it satisfies.
    pub key: ArtifactKey,
    /// Where the artifact is. A reference, never the content.
    pub locator: CanonicalDocument,
    /// The role that produced it.
    pub producer_role: RoleKey,
    /// The account that produced it.
    pub producer_account: AccountProfileId,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// The explicit authority receipt behind one waived gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGateWaiver {
    /// The receipt id.
    pub id: GateWaiverId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The workflow.
    pub workflow_id: TaskWorkflowId,
    /// The gate.
    pub gate: GateKey,
    /// The waiving evaluation's position in that gate's history.
    pub sequence: u32,
    /// The role that waived.
    pub authorizing_role: RoleKey,
    /// The account it waived through.
    pub authorizing_account: AccountProfileId,
    /// Why. Non-secret prose.
    pub reason: String,
    /// The evidence bundle behind the decision.
    pub evidence: CanonicalDocument,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// One rejection to append, with the park it may turn out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRejection {
    /// Owning project.
    pub project_id: ProjectId,
    /// The workflow.
    pub workflow_id: TaskWorkflowId,
    /// The gate.
    pub gate: GateKey,
    /// The role rejecting. Checked against the pinned profile.
    pub evaluator_role: RoleKey,
    /// The account it rejects through.
    pub evaluator_account: AccountProfileId,
    /// The stable principal behind it. This is what the counter is keyed on.
    pub reviewer_principal: ExternalId,
    /// The run the reviewer was acting inside, when there was one.
    pub agent_run_id: Option<AgentRunId>,
    /// Artifacts cited.
    pub evidence: Vec<ArtifactKey>,
    /// When it happened.
    pub recorded_at: Timestamp,
    /// The identities to use if this rejection turns out to be the park.
    ///
    /// Supplied up front and unused when it is not: the caller cannot know
    /// whether the park will happen, because the count is derived here, so it
    /// hands over the identities either way rather than being asked a second
    /// time inside a transaction that has already started.
    pub park: ParkPlan,
}

/// The identities a park needs, prepared before the transaction opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParkPlan {
    /// The guardrail evaluation to record. Must be a `second_rejection_parks`
    /// park verdict; anything else is refused.
    pub evaluation: GuardrailEvaluation,
    /// The recovery episode to open.
    pub episode_id: RecoveryEpisodeId,
    /// The receipt id recording the park decision.
    pub closure_receipt_id: CommandReceiptId,
    /// Its idempotency key.
    pub closure_key: IdempotencyKey,
    /// The canonical park decision document. Its digest is the closure evidence.
    pub closure_intent: CanonicalDocument,
}

/// What a park actually wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParkedRecovery {
    /// The evaluation that caused it.
    pub evaluation_id: GuardrailEvaluationId,
    /// The episode now recovering it.
    pub episode_id: RecoveryEpisodeId,
    /// The run that was closed.
    pub parked_agent_run_id: AgentRunId,
    /// The receipt recording the decision.
    pub closure_receipt_id: CommandReceiptId,
}

/// The outcome of appending a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejectionOutcome {
    /// The rejection's position in the gate's append-only history.
    pub sequence: u32,
    /// How many rejections this reviewer now has on this gate since their last
    /// pass, this one included.
    pub rejections_since_pass: u32,
    /// What the park wrote, when the count reached the threshold.
    pub parked: Option<ParkedRecovery>,
}

/// One appended recovery step, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecoveryStep {
    /// Position in the episode's append-only history, from 1.
    pub sequence: u32,
    /// What kind of step.
    pub kind: RecoveryStepKind,
    /// Digest of what it was given.
    pub input_hash: ContentHash,
    /// Digest of what it produced.
    pub output_hash: Option<ContentHash>,
    /// The successor run it dispatched, when it dispatched one.
    pub agent_run_id: Option<AgentRunId>,
    /// When it happened.
    pub recorded_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Column helpers
// ---------------------------------------------------------------------------

fn version_column(version: SpecVersion) -> i64 {
    i64::from(version.get())
}

fn flag(value: bool) -> i64 {
    i64::from(value)
}

fn count_column(value: u32) -> i64 {
    i64::from(value)
}

fn read_count(value: i64) -> RepositoryResult<u32> {
    u32::try_from(value).map_err(|_| RepositoryError::Backend {
        detail: "a stored counter is out of range".to_owned(),
    })
}

impl SqliteStore {
    // -----------------------------------------------------------------------
    // Evaluations and evidence
    // -----------------------------------------------------------------------

    /// Append one immutable guardrail evaluation.
    ///
    /// # Errors
    /// Returns [`RepositoryError`] when the binding names rows outside this
    /// project, or when the backend refuses the write.
    pub fn append_policy_evaluation(
        &self,
        binding: &EvaluationBinding,
        evaluation: &GuardrailEvaluation,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        insert_policy_evaluation(&transaction, binding, evaluation)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Append one artifact-evidence record.
    ///
    /// # Errors
    /// Returns [`RepositoryError`] when the backend refuses the write.
    pub fn record_artifact_evidence(
        &self,
        record: &NewArtifactEvidence,
    ) -> RepositoryResult<ArtifactEvidenceId> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO artifact_evidence
                     (id, project_id, task_id, workflow_id, agent_run_id, artifact_key,
                      locator, locator_hash, producer_role, producer_account, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    record.id.to_string(),
                    record.binding.project_id.to_string(),
                    record.binding.task_id.to_string(),
                    record.binding.workflow_id.to_string(),
                    record.binding.agent_run_id.map(|run| run.to_string()),
                    record.key.as_str(),
                    record.locator.json(),
                    record.locator.hash().as_str(),
                    record.producer_role.as_str(),
                    record.producer_account.to_string(),
                    text(record.recorded_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(record.id)
    }

    /// Record the explicit authority receipt behind a waived gate.
    ///
    /// The waiver verdict itself already lives in `task_gate_evaluations`; the
    /// composite foreign key means this receipt cannot be filed against a gate
    /// evaluation that does not exist, and the unique key means one waiver
    /// cannot collect two authorities.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when the cited evaluation does not exist.
    /// * [`DomainError::Invalid`] when the cited evaluation is not a waiver.
    pub fn record_gate_waiver(&self, record: &NewGateWaiver) -> RepositoryResult<GateWaiverId> {
        let transaction = self.begin()?;
        let verdict: Option<String> = transaction
            .query_row(
                "SELECT verdict FROM task_gate_evaluations
                 WHERE project_id = ?1 AND workflow_id = ?2 AND gate_key = ?3 AND sequence = ?4",
                params![
                    record.project_id.to_string(),
                    record.workflow_id.to_string(),
                    record.gate.as_str(),
                    i64::from(record.sequence)
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(verdict) = verdict else {
            return Err(RepositoryError::NotFound {
                subject: "gate evaluation",
            });
        };
        if GateVerdict::parse(&verdict)? != GateVerdict::Waived {
            return Err(DomainError::invalid(
                "gate waiver",
                "the cited gate evaluation is not a waiver",
            )
            .into());
        }
        transaction
            .execute(
                "INSERT INTO gate_waivers
                     (id, project_id, workflow_id, gate_key, sequence, authorizing_role,
                      authorizing_account, reason, evidence, evidence_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    record.id.to_string(),
                    record.project_id.to_string(),
                    record.workflow_id.to_string(),
                    record.gate.as_str(),
                    i64::from(record.sequence),
                    record.authorizing_role.as_str(),
                    record.authorizing_account.to_string(),
                    record.reason.as_str(),
                    record.evidence.json(),
                    record.evidence.hash().as_str(),
                    text(record.recorded_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(record.id)
    }

    // -----------------------------------------------------------------------
    // Approvals
    // -----------------------------------------------------------------------

    /// Store one approval bound to one exact destructive action.
    ///
    /// # Errors
    /// * [`DomainError::MissingAuthority`] when the receipt claims recovery
    ///   advice as its authority. An advisor's recommendation is not an
    ///   approval, and the schema refuses to hold one either.
    /// * [`RepositoryError::Conflict`] when this project already has a receipt
    ///   for this action digest.
    pub fn issue_approval_receipt(&self, receipt: &ApprovalReceipt) -> RepositoryResult<()> {
        if receipt.authority_source == AuthoritySource::RecoveryAdvice {
            return Err(DomainError::MissingAuthority {
                subject: "approval receipt",
                rule: "recovery advice can never approve a destructive action",
            }
            .into());
        }
        if receipt.consumed_at.is_some() {
            return Err(DomainError::invalid(
                "approval receipt",
                "is issued unspent; consumption is a separate, one-way step",
            )
            .into());
        }
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO approval_receipts
                     (id, project_id, scope_kind, task_id, action_domain, action_intent,
                      action_effect, action_digest, approver_principal, approver_role,
                      approver_account, authority_source, evidence, evidence_hash,
                      issued_at, expires_at, consumed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, NULL)",
                params![
                    receipt.id.to_string(),
                    receipt.project_id.to_string(),
                    receipt.scope_kind.as_str(),
                    receipt.task_id.map(|task| task.to_string()),
                    receipt.action_domain.as_str(),
                    receipt.action_intent.as_str(),
                    receipt.action_effect.as_str(),
                    receipt.action_digest.as_str(),
                    receipt.approver_principal.as_str(),
                    receipt.approver_role.as_str(),
                    receipt.approver_account.to_string(),
                    receipt.authority_source.as_str(),
                    receipt.evidence.json(),
                    receipt.evidence.hash().as_str(),
                    text(receipt.issued_at),
                    text(receipt.expires_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Spend an approval against the exact action it was issued for.
    ///
    /// The digest is compared inside the transaction that consumes the receipt,
    /// so an approval for one command cannot be spent on another even by a
    /// caller holding a valid receipt id.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when no such receipt exists here.
    /// * [`DomainError::MissingAuthority`] when the digest does not match.
    /// * [`RepositoryError::Conflict`] when it was already spent or had expired.
    pub fn consume_approval_receipt(
        &self,
        project_id: ProjectId,
        id: ApprovalReceiptId,
        action_digest: &ContentHash,
        at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let found: Option<(String, String, Option<String>)> = transaction
            .query_row(
                "SELECT action_digest, expires_at, consumed_at FROM approval_receipts
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((stored_digest, expires_at, consumed_at)) = found else {
            return Err(RepositoryError::NotFound {
                subject: "approval receipt",
            });
        };
        if ContentHash::parse(&stored_digest)? != *action_digest {
            return Err(DomainError::MissingAuthority {
                subject: "approval receipt",
                rule: "the receipt was issued for a different action",
            }
            .into());
        }
        if consumed_at.is_some() {
            return Err(conflict("approval receipt", "has already been consumed"));
        }
        if at >= read_timestamp(&expires_at)? {
            return Err(conflict("approval receipt", "has expired"));
        }
        let changed = transaction
            .execute(
                "UPDATE approval_receipts SET consumed_at = ?1
                 WHERE project_id = ?2 AND id = ?3 AND consumed_at IS NULL",
                params![text(at), project_id.to_string(), id.to_string()],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "approval receipt",
                "was consumed during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Prove an approval receipt is persisted, unexpired, unconsumed, and
    /// bound to exactly this action — read-only, without spending it.
    ///
    /// This is the persistence half of `destructive_requires_approval`: an
    /// in-memory receipt object alone never admits a destructive action; the
    /// caller must first prove the receipt exists in `approval_receipts` in
    /// the state the evaluator trusts it. The digest is compared against the
    /// stored row, so a fabricated receipt carrying a valid-looking digest is
    /// refused when no such row exists.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when no such receipt exists here.
    /// * [`DomainError::MissingAuthority`] when the stored digest does not
    ///   match the action the caller is trying to authorize.
    /// * [`RepositoryError::Conflict`] when it was already spent or had
    ///   expired.
    pub fn verify_approval_receipt(
        &self,
        project_id: ProjectId,
        id: ApprovalReceiptId,
        action_digest: &ContentHash,
        at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let found: Option<(String, String, Option<String>)> = transaction
            .query_row(
                "SELECT action_digest, expires_at, consumed_at FROM approval_receipts
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((stored_digest, expires_at, consumed_at)) = found else {
            return Err(RepositoryError::NotFound {
                subject: "approval receipt",
            });
        };
        if ContentHash::parse(&stored_digest)? != *action_digest {
            return Err(DomainError::MissingAuthority {
                subject: "approval receipt",
                rule: "the receipt was issued for a different action",
            }
            .into());
        }
        if consumed_at.is_some() {
            return Err(conflict("approval receipt", "has already been consumed"));
        }
        if at >= read_timestamp(&expires_at)? {
            return Err(conflict("approval receipt", "has expired"));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rejection counting and parking
    // -----------------------------------------------------------------------

    /// How many times one principal has rejected one gate since their last pass.
    ///
    /// Derived from the append-only history. There is no counter row, so there
    /// is nothing to drift out of step with the evaluations it summarizes.
    ///
    /// # Errors
    /// Returns [`RepositoryError`] on backend failure or unreadable stored data.
    pub fn rejections_since_pass(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
        gate: &GateKey,
        principal: &ExternalId,
    ) -> RepositoryResult<u32> {
        let transaction = self.begin()?;
        rejections_since_pass_in(&transaction, project_id, workflow_id, gate, principal)
    }

    /// Append a rejection and, if it is the reviewer's second on this gate, park
    /// the work — all in one transaction.
    ///
    /// What the park writes, in this order and with no gap between any two:
    ///
    /// 1. the `second_rejection_parks` guardrail evaluation;
    /// 2. the rejection itself, carrying the reviewer principal it is counted
    ///    under *and* the evaluation that parked it;
    /// 3. the receipt recording the park decision;
    /// 4. the run's terminal closure, citing that receipt;
    /// 5. the recovery episode, open and empty;
    /// 6. the `run_park_closures` row saying a guardrail parked it;
    /// 7. the task's move to `parked`.
    ///
    /// The count is therefore taken *before* the rejection is written, with the
    /// pending rejection added to it. That ordering is forced rather than
    /// stylistic: `task_gate_evaluations` is append-only, so the link back to
    /// the evaluation has to be part of the row's one and only INSERT — there is
    /// no later UPDATE available to add it, and a park whose cause could not be
    /// recorded on its own rejection would be an audit trail with a hole in it.
    ///
    /// # Errors
    /// * [`DomainError::MissingAuthority`] when the pinned profile does not
    ///   authorize this role to reject this gate.
    /// * [`DomainError::Invalid`] when the supplied park plan is not a
    ///   `second_rejection_parks` park verdict, or when the park is due and the
    ///   request names no run to close.
    /// * [`RepositoryError`] on revision conflicts and backend failures. The
    ///   whole unit rolls back together.
    pub fn record_gate_rejection(
        &self,
        request: &GateRejection,
    ) -> RepositoryResult<RejectionOutcome> {
        let transaction = self.begin()?;
        let (workflow, _) = crate::repository::load_workflow(
            &transaction,
            request.project_id,
            request.workflow_id,
        )?;
        let gate = workflow
            .snapshot
            .definition
            .gate(&request.gate)
            .ok_or(RepositoryError::NotFound { subject: "gate" })?;
        // The same authority the ordinary gate path enforces. A rejection is a
        // verdict, and a role the profile does not authorize cannot record one
        // here either — least of all one that parks the task.
        if !gate.evaluator_roles.contains(&request.evaluator_role) {
            return Err(DomainError::MissingAuthority {
                subject: "gate rejection",
                rule: "the acting role is not an evaluator for this gate",
            }
            .into());
        }

        // The stream as it stands, plus the rejection about to join it. Derived
        // from the stored history, never from anything the caller believes.
        let count = rejections_since_pass_in(
            &transaction,
            request.project_id,
            request.workflow_id,
            &request.gate,
            &request.reviewer_principal,
        )?
        .saturating_add(1);
        let parks = count >= kontor_policy::REJECTIONS_BEFORE_PARK;

        // The evaluation first, so the rejection row can name it. Everything the
        // park needs to refuse — an unusable plan, a run that is already closed
        // — is refused in here, before a single row is written.
        let prepared = if parks {
            Some(prepare_park(
                &transaction,
                request,
                workflow.task_id,
                request.recorded_at,
            )?)
        } else {
            None
        };

        let previous: Option<i64> = transaction
            .query_row(
                "SELECT MAX(sequence) FROM task_gate_evaluations
                 WHERE project_id = ?1 AND workflow_id = ?2 AND gate_key = ?3",
                params![
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    request.gate.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?
            .flatten();
        let sequence = previous.unwrap_or(0) + 1;
        transaction
            .execute(
                "INSERT INTO task_gate_evaluations
                     (project_id, workflow_id, gate_key, sequence, verdict, evaluator_role,
                      evaluator_account, evidence, recorded_at, agent_run_id,
                      reviewer_principal, policy_evaluation_id)
                 VALUES (?1, ?2, ?3, ?4, 'rejected', ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    request.gate.as_str(),
                    sequence,
                    request.evaluator_role.as_str(),
                    request.evaluator_account.to_string(),
                    to_json(&request.evidence)?,
                    text(request.recorded_at),
                    request.agent_run_id.map(|run| run.to_string()),
                    request.reviewer_principal.as_str(),
                    prepared.as_ref().map(|park| park.evaluation_id.to_string())
                ],
            )
            .map_err(backend)?;

        let sequence = read_count(sequence)?;
        if let Some(parked) = prepared {
            finish_park(&transaction, request, workflow.task_id, &parked)?;
            transaction.commit().map_err(backend)?;
            return Ok(RejectionOutcome {
                sequence,
                rejections_since_pass: count,
                parked: Some(parked),
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(RejectionOutcome {
            sequence,
            rejections_since_pass: count,
            parked: None,
        })
    }

    // -----------------------------------------------------------------------
    // Recovery
    // -----------------------------------------------------------------------

    /// Load one recovery episode.
    ///
    /// # Errors
    /// Returns [`RepositoryError`] on backend failure or unreadable stored data.
    pub fn get_recovery_episode(
        &self,
        project_id: ProjectId,
        id: RecoveryEpisodeId,
    ) -> RepositoryResult<Option<RecoveryEpisode>> {
        let transaction = self.begin()?;
        read_episode(&transaction, project_id, id)
    }

    /// Apply one planned recovery transition and append its step.
    ///
    /// The transition is computed by [`kontor_policy::recovery::plan`] against
    /// the episode loaded *inside* this transaction, so a caller that planned
    /// against a stale episode is refused by the revision check rather than
    /// having its stale plan applied. The step and the state it produced are
    /// written together.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] when the episode does not exist here.
    /// * [`DomainError::Terminal`], [`DomainError::RevisionConflict`],
    ///   [`DomainError::IllegalTransition`] or [`DomainError::Invalid`] as
    ///   [`kontor_policy::recovery::plan`] refuses.
    pub fn apply_recovery_transition(
        &self,
        project_id: ProjectId,
        episode_id: RecoveryEpisodeId,
        request: &RecoveryRequest,
    ) -> RepositoryResult<RecoveryEpisode> {
        let transaction = self.begin()?;
        let episode = read_episode(&transaction, project_id, episode_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "recovery episode",
            },
        )?;
        let transition = kontor_policy::recovery::plan(&episode, request)?;
        let next = write_transition(&transaction, &episode, request, &transition)?;
        transaction.commit().map_err(backend)?;
        Ok(next)
    }

    /// Every step one episode has taken, in order.
    ///
    /// # Errors
    /// Returns [`RepositoryError`] on backend failure or unreadable stored data.
    pub fn list_recovery_steps(
        &self,
        project_id: ProjectId,
        episode_id: RecoveryEpisodeId,
    ) -> RepositoryResult<Vec<StoredRecoveryStep>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT sequence, kind, input_hash, output_hash, agent_run_id, recorded_at
                 FROM recovery_steps
                 WHERE project_id = ?1 AND episode_id = ?2
                 ORDER BY sequence",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), episode_id.to_string()])
            .map_err(backend)?;
        let mut steps = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let output: Option<String> = row.get(3).map_err(backend)?;
            let run: Option<String> = row.get(4).map_err(backend)?;
            steps.push(StoredRecoveryStep {
                sequence: read_count(row.get(0).map_err(backend)?)?,
                kind: RecoveryStepKind::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                input_hash: ContentHash::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                output_hash: output.as_deref().map(ContentHash::parse).transpose()?,
                agent_run_id: run.as_deref().map(AgentRunId::parse).transpose()?,
                recorded_at: read_timestamp(&row.get::<_, String>(5).map_err(backend)?)?,
            });
        }
        Ok(steps)
    }
}

// ---------------------------------------------------------------------------
// Transaction-scoped work
// ---------------------------------------------------------------------------

fn insert_policy_evaluation(
    transaction: &Transaction<'_>,
    binding: &EvaluationBinding,
    evaluation: &GuardrailEvaluation,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO policy_evaluations
                 (id, project_id, task_id, workflow_id, team_run_id, agent_run_id, rule_key,
                  rule_version, subject_kind, subject_id, inputs, inputs_hash, verdict,
                  reason_code, evidence_refs, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                evaluation.id.to_string(),
                binding.project_id.to_string(),
                binding.task_id.to_string(),
                binding.workflow_id.to_string(),
                binding.team_run_id.map(|run| run.to_string()),
                binding.agent_run_id.map(|run| run.to_string()),
                evaluation.rule_key.as_str(),
                version_column(evaluation.rule_version),
                evaluation.subject.kind.as_str(),
                evaluation.subject.id.as_str(),
                evaluation.inputs.json(),
                evaluation.inputs_hash.as_str(),
                evaluation.verdict.as_str(),
                evaluation.reason_code.as_str(),
                to_json(&evaluation.evidence_refs)?,
                text(evaluation.recorded_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

/// Count one principal's rejections on one gate since their last pass.
///
/// Rows with no recorded principal are attributable to nobody and are skipped by
/// the predicate rather than folded into somebody's stream.
fn rejections_since_pass_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    workflow_id: TaskWorkflowId,
    gate: &GateKey,
    principal: &ExternalId,
) -> RepositoryResult<u32> {
    let mut statement = transaction
        .prepare(
            "SELECT verdict FROM task_gate_evaluations
             WHERE project_id = ?1 AND workflow_id = ?2 AND gate_key = ?3
               AND reviewer_principal = ?4
             ORDER BY sequence",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![
            project_id.to_string(),
            workflow_id.to_string(),
            gate.as_str(),
            principal.as_str()
        ])
        .map_err(backend)?;
    let mut count: u32 = 0;
    while let Some(row) = rows.next().map_err(backend)? {
        match GateVerdict::parse(&row.get::<_, String>(0).map_err(backend)?)? {
            GateVerdict::Rejected => count = count.saturating_add(1),
            GateVerdict::Passed => count = 0,
            // Neither an acceptance nor a refusal of the work, so the stream is
            // left exactly where it was.
            GateVerdict::Started | GateVerdict::Waived | GateVerdict::Parked => {}
        }
    }
    Ok(count)
}

/// The half of a park that must happen before the rejection row exists.
///
/// Refusals live here, together: an unusable plan, a rejection that names no
/// run, a run that is already closed. Every one of them is raised before the
/// first row is written, so a park that cannot complete takes the rejection down
/// with it rather than leaving a rejection whose consequences never happened.
fn prepare_park(
    transaction: &Transaction<'_>,
    request: &GateRejection,
    task_id: TaskId,
    recorded_at: Timestamp,
) -> RepositoryResult<ParkedRecovery> {
    let plan = &request.park;
    if plan.evaluation.rule_key != GuardrailRuleKey::SecondRejectionParks
        || plan.evaluation.verdict != PolicyVerdict::Park
        || plan.evaluation.reason_code != ReasonCode::SecondRejectionParks
    {
        return Err(DomainError::invalid(
            "gate rejection",
            "the supplied park plan is not a second-rejection park verdict",
        )
        .into());
    }
    if plan.evaluation.recorded_at != recorded_at {
        return Err(DomainError::invalid(
            "gate rejection",
            "the park evaluation must be recorded at the instant of the rejection that caused it",
        )
        .into());
    }
    let Some(agent_run_id) = request.agent_run_id else {
        return Err(DomainError::MissingEvidence {
            subject: "gate rejection",
            rule: "parking closes the run that was rejected, so the rejection must name one",
        }
        .into());
    };

    let run = crate::repository::read_agent_run(transaction, request.project_id, agent_run_id)?
        .ok_or(RepositoryError::NotFound {
            subject: "agent run",
        })?;
    run.projection.ensure_open("agent run")?;

    let binding = EvaluationBinding {
        project_id: request.project_id,
        task_id,
        workflow_id: request.workflow_id,
        team_run_id: Some(run.team_run_id),
        agent_run_id: Some(agent_run_id),
    };
    insert_policy_evaluation(transaction, &binding, &plan.evaluation)?;

    Ok(ParkedRecovery {
        evaluation_id: plan.evaluation.id,
        episode_id: plan.episode_id,
        parked_agent_run_id: agent_run_id,
        closure_receipt_id: plan.closure_receipt_id,
    })
}

/// The half of a park that happens once the rejection is on record.
fn finish_park(
    transaction: &Transaction<'_>,
    request: &GateRejection,
    task_id: TaskId,
    parked: &ParkedRecovery,
) -> RepositoryResult<()> {
    let plan = &request.park;
    let agent_run_id = parked.parked_agent_run_id;
    let run = crate::repository::read_agent_run(transaction, request.project_id, agent_run_id)?
        .ok_or(RepositoryError::NotFound {
            subject: "agent run",
        })?;

    let target = AggregateRef::AgentRun { agent_run_id };
    insert_closure_receipt(transaction, request, &target, run.revision)?;

    // The closure is verified against the receipt just written, using the same
    // domain rule the ordinary closure path uses. Writing the row and then
    // proving it authorizes the write is not circular: the proof is what stops a
    // future edit here from quietly producing a closure nothing authorizes.
    let evidence = TerminalEvidence {
        outcome: TerminalOutcome::Abandoned,
        source: TerminalEvidenceSource::OperatorAbandon {
            receipt_id: plan.closure_receipt_id,
        },
        evidence_hash: plan.closure_intent.hash().clone(),
        closed_at: request.recorded_at,
    };
    let facts = crate::repository::read_abandon_receipt(
        transaction,
        request.project_id,
        plan.closure_receipt_id,
        &target,
    )?;
    evidence.verify_abandon(run.revision, &facts)?;

    let closed_revision = run.revision.next()?;
    let changed = transaction
        .execute(
            "UPDATE agent_runs
             SET lifecycle = ?1, derived_state = 'terminal', terminal_outcome = ?2,
                 terminal_source_kind = 'operator_abandon', terminal_receipt_id = ?3,
                 terminal_evidence_hash = ?4, closed_at = ?5, revision = ?6
             WHERE project_id = ?7 AND id = ?8 AND revision = ?9",
            params![
                evidence.outcome.lifecycle().as_str(),
                evidence.outcome.as_str(),
                plan.closure_receipt_id.to_string(),
                evidence.evidence_hash.as_str(),
                text(evidence.closed_at),
                revision_column(closed_revision)?,
                request.project_id.to_string(),
                agent_run_id.to_string(),
                revision_column(run.revision)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "agent run",
            "the run revision moved during the park",
        ));
    }

    // The episode has to exist before the park closure can point at it.
    transaction
        .execute(
            "INSERT INTO recovery_episodes
                 (id, project_id, task_id, workflow_id, parked_agent_run_id, status,
                  cause_evaluation_id, advisor_used, committee_used, effective_followups,
                  successor_agent_run_id, escalation_cause, revision, created_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, 0, 0, 0, NULL, NULL, 1, ?7, NULL)",
            params![
                plan.episode_id.to_string(),
                request.project_id.to_string(),
                task_id.to_string(),
                request.workflow_id.to_string(),
                agent_run_id.to_string(),
                plan.evaluation.id.to_string(),
                text(request.recorded_at)
            ],
        )
        .map_err(backend)?;

    transaction
        .execute(
            "INSERT INTO run_park_closures
                 (project_id, agent_run_id, team_run_id, policy_evaluation_id,
                  recovery_episode_id, closure_receipt_id, reason_code, evidence_hash, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                request.project_id.to_string(),
                agent_run_id.to_string(),
                run.team_run_id.to_string(),
                plan.evaluation.id.to_string(),
                plan.episode_id.to_string(),
                plan.closure_receipt_id.to_string(),
                PARKED_AUTO_TRIAGE,
                plan.closure_intent.hash().as_str(),
                text(request.recorded_at)
            ],
        )
        .map_err(backend)?;

    park_task(
        transaction,
        request.project_id,
        task_id,
        request.recorded_at,
    )?;
    Ok(())
}

/// Write the receipt that records the park decision.
///
/// A receipt, not a dispatch: there is no outbox entry, because nothing is being
/// sent anywhere. The decision is Kontor's own and it is already carried out by
/// the time this transaction commits.
fn insert_closure_receipt(
    transaction: &Transaction<'_>,
    request: &GateRejection,
    target: &AggregateRef,
    target_revision: AggregateRevision,
) -> RepositoryResult<()> {
    let plan = &request.park;
    transaction
        .execute(
            "INSERT INTO command_receipts
                 (id, project_id, idempotency_key, kind, target, target_revision, intent,
                  intent_hash, state, attempts, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'intent_persisted', 0, ?9, ?9)",
            params![
                plan.closure_receipt_id.to_string(),
                request.project_id.to_string(),
                plan.closure_key.as_str(),
                CommandKind::AbandonRun.as_str(),
                to_json(target)?,
                revision_column(target_revision)?,
                plan.closure_intent.json(),
                plan.closure_intent.hash().as_str(),
                text(request.recorded_at)
            ],
        )
        .map_err(backend)?;
    let (kind, columns) = crate::repository::target_columns(target);
    transaction
        .execute(
            "INSERT INTO command_targets
                 (project_id, receipt_id, target_kind, target_project_id,
                  target_mini_project_id, target_task_id, target_team_run_id,
                  target_agent_run_id, target_ticket_link_id, target_work_calendar_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                request.project_id.to_string(),
                plan.closure_receipt_id.to_string(),
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
    append_transition(
        transaction,
        request.project_id,
        plan.closure_receipt_id,
        1,
        CommandReceiptState::IntentPersisted,
        None,
        None,
        None,
        request.recorded_at,
    )
}

/// Move the task to `parked`, through the domain transition table.
fn park_task(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    at: Timestamp,
) -> RepositoryResult<()> {
    let found: Option<(String, i64)> = transaction
        .query_row(
            "SELECT state, revision FROM tasks WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), task_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((state, revision)) = found else {
        return Err(RepositoryError::NotFound { subject: "task" });
    };
    let current = TaskState::parse(&state)?;
    // A task already parked is left alone: a second guardrail park of the same
    // task is not an error, and re-parking would advance a revision for no
    // change.
    if current == TaskState::Parked {
        return Ok(());
    }
    let next = apply_task_transition(current, &TaskTransition::to(TaskState::Parked))?;
    let revision = revision_of(revision)?;
    let changed = transaction
        .execute(
            "UPDATE tasks SET state = ?1, revision = ?2, updated_at = ?3
             WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
            params![
                next.as_str(),
                revision_column(revision.next()?)?,
                text(at),
                project_id.to_string(),
                task_id.to_string(),
                revision_column(revision)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict("task", "the task revision moved during the park"));
    }
    Ok(())
}

fn read_episode(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: RecoveryEpisodeId,
) -> RepositoryResult<Option<RecoveryEpisode>> {
    type Row = (
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        i64,
        Option<String>,
        Option<String>,
        i64,
        String,
        Option<String>,
    );
    let found: Option<Row> = transaction
        .query_row(
            "SELECT task_id, workflow_id, parked_agent_run_id, status, cause_evaluation_id,
                    advisor_used, committee_used, effective_followups, successor_agent_run_id,
                    escalation_cause, revision, created_at, closed_at
             FROM recovery_episodes WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), id.to_string()],
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
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((
        task_id,
        workflow_id,
        parked,
        status,
        cause,
        advisor,
        committee,
        followups,
        successor,
        escalation,
        revision,
        created_at,
        closed_at,
    )) = found
    else {
        return Ok(None);
    };
    Ok(Some(RecoveryEpisode {
        id,
        project_id,
        task_id: TaskId::parse(&task_id)?,
        workflow_id: TaskWorkflowId::parse(&workflow_id)?,
        parked_agent_run_id: AgentRunId::parse(&parked)?,
        status: RecoveryStatus::parse(&status)?,
        cause_evaluation_id: GuardrailEvaluationId::parse(&cause)?,
        advisor_used: advisor == 1,
        committee_used: committee == 1,
        effective_followups: read_count(followups)?,
        successor_agent_run_id: successor.as_deref().map(AgentRunId::parse).transpose()?,
        escalation_cause: escalation
            .as_deref()
            .map(EscalationCause::parse)
            .transpose()?,
        revision: revision_of(revision)?,
        created_at: read_timestamp(&created_at)?,
        closed_at: closed_at.as_deref().map(read_timestamp).transpose()?,
    }))
}

/// Prove a dispatched follow-up runs as a distinct successor of this episode.
///
/// Three things have to hold, and none of them can be decided without storage:
///
/// * the successor exists, in this project — a foreign key would catch a
///   dangling id, but only after the step had already been written;
/// * it descends from the episode. Its parent is either the parked run or the
///   successor a previous follow-up dispatched, so the chain leads back to the
///   run that was parked. A parentless run is a fresh start, not a recovery of
///   this episode, and recording one as a successor would make the lineage a
///   claim rather than a fact;
/// * no step of this episode already used it. The unique index enforces this
///   too; checking here turns a constraint violation into a typed refusal that
///   names what was wrong.
fn ensure_linked_successor(
    transaction: &Transaction<'_>,
    episode: &RecoveryEpisode,
    successor: AgentRunId,
) -> RepositoryResult<()> {
    let parent: Option<Option<String>> = transaction
        .query_row(
            "SELECT parent_agent_run_id FROM agent_runs WHERE project_id = ?1 AND id = ?2",
            params![episode.project_id.to_string(), successor.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    let Some(parent) = parent else {
        return Err(RepositoryError::NotFound {
            subject: "recovery successor run",
        });
    };
    let parent = parent
        .as_deref()
        .map(AgentRunId::parse)
        .transpose()?
        .ok_or(DomainError::MissingEvidence {
            subject: "recovery follow-up",
            rule: "a successor must descend from the run it is recovering",
        })?;
    if parent != episode.parked_agent_run_id && Some(parent) != episode.successor_agent_run_id {
        return Err(DomainError::MissingEvidence {
            subject: "recovery follow-up",
            rule: "a successor's lineage must lead back to the parked run",
        }
        .into());
    }

    let reused: i64 = transaction
        .query_row(
            "SELECT count(*) FROM recovery_steps
             WHERE project_id = ?1 AND episode_id = ?2 AND agent_run_id = ?3",
            params![
                episode.project_id.to_string(),
                episode.id.to_string(),
                successor.to_string()
            ],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if reused > 0 {
        return Err(conflict(
            "recovery follow-up",
            "this successor has already been dispatched by an earlier step",
        ));
    }
    Ok(())
}

/// Apply a planned transition and append the step that produced it.
fn write_transition(
    transaction: &Transaction<'_>,
    episode: &RecoveryEpisode,
    request: &RecoveryRequest,
    transition: &RecoveryTransition,
) -> RepositoryResult<RecoveryEpisode> {
    let next_revision = episode.revision.next()?;

    // A dispatched follow-up has to be a real, distinct, linked successor before
    // anything is written. `plan` already refused the parked run and the
    // episode's current successor; this proves the rest against storage, which
    // is the only place that can answer it.
    if let Some(successor) = transition.dispatched_successor {
        ensure_linked_successor(transaction, episode, successor)?;
    }

    // The step is appended *first*, and the episode moves second. That order is
    // required, not stylistic: `recovery_episodes_require_step` refuses an
    // update whose revision does not match the number of steps on record, so an
    // advance is only reachable once the step accounting for it exists. It is
    // also the order every other consequence in this schema is written in —
    // evidence, then what was derived from it.
    let previous: Option<i64> = transaction
        .query_row(
            "SELECT MAX(sequence) FROM recovery_steps WHERE project_id = ?1 AND episode_id = ?2",
            params![episode.project_id.to_string(), episode.id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?
        .flatten();
    let sequence = previous.unwrap_or(0) + 1;
    // Only the run *this* step dispatched. A read-only consultation names none,
    // and neither does a refused dispatch: recording the previous attempt's run
    // on a step that ran nothing would both misreport it and collide with the
    // successor uniqueness index.
    let step_run = transition.dispatched_successor.map(|run| run.to_string());
    transaction
        .execute(
            "INSERT INTO recovery_steps
                 (project_id, episode_id, sequence, kind, input_hash, output_hash,
                  agent_run_id, policy_evaluation_id, artifact_evidence_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8)",
            params![
                episode.project_id.to_string(),
                episode.id.to_string(),
                sequence,
                transition.step.as_str(),
                request.input_hash.as_str(),
                request.output_hash.as_ref().map(ContentHash::as_str),
                step_run,
                text(request.occurred_at)
            ],
        )
        .map_err(backend)?;

    let changed = transaction
        .execute(
            "UPDATE recovery_episodes
             SET status = ?1, advisor_used = ?2, committee_used = ?3, effective_followups = ?4,
                 successor_agent_run_id = ?5, escalation_cause = ?6, closed_at = ?7, revision = ?8
             WHERE project_id = ?9 AND id = ?10 AND revision = ?11",
            params![
                transition.status.as_str(),
                flag(transition.advisor_used),
                flag(transition.committee_used),
                count_column(transition.effective_followups),
                transition.successor_agent_run_id.map(|run| run.to_string()),
                transition.escalation_cause.map(|cause| cause.as_str()),
                transition.closed_at.map(text),
                revision_column(next_revision)?,
                episode.project_id.to_string(),
                episode.id.to_string(),
                revision_column(episode.revision)?
            ],
        )
        .map_err(backend)?;
    if changed != 1 {
        return Err(conflict(
            "recovery episode",
            "the episode revision moved during the write",
        ));
    }

    Ok(RecoveryEpisode {
        status: transition.status,
        advisor_used: transition.advisor_used,
        committee_used: transition.committee_used,
        effective_followups: transition.effective_followups,
        successor_agent_run_id: transition.successor_agent_run_id,
        escalation_cause: transition.escalation_cause,
        revision: next_revision,
        closed_at: transition.closed_at,
        ..episode.clone()
    })
}
