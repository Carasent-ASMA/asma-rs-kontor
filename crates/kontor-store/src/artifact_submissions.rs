//! Atomic, idempotent operator recovery of addressable producer claims.
use crate::policy::insert_artifact_evidence;
use crate::repository::backend;
use crate::{NewArtifactEvidence, SqliteStore};
use kontor_core::DomainError;
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, ContentHash, IdempotencyKey, ProjectId, RoleTurnId,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

/// An operator-verified locator and the immutable augmentation receipt.
pub struct NewArtifactSubmission {
    /// Existing artifact registry record, with derived producer identity.
    pub evidence: NewArtifactEvidence,
    /// Exact settled turn that claimed the key.
    pub role_turn_id: RoleTurnId,
    /// Current task revision, rechecked atomically.
    pub task_revision: AggregateRevision,
    /// Stable retry identity.
    pub idempotency_key: IdempotencyKey,
    /// Digest of the complete public request and authenticated authority.
    pub request_hash: ContentHash,
    /// Authenticated realm authority, never a body field.
    pub authority_tier: String,
    /// Truthful distinction between native proof and historical/attested settlement.
    pub turn_proof_class: String,
    /// Original public answer for restart-safe replay.
    pub response: CanonicalDocument,
}
impl SqliteStore {
    /// Replay a previous submission without touching a native session or checkout.
    pub fn artifact_submission_replay(
        &self,
        project_id: ProjectId,
        key: &IdempotencyKey,
        hash: &ContentHash,
    ) -> RepositoryResult<Option<String>> {
        let row: Option<(String, String, String)> = self.connection.query_row(
            "SELECT project_id, request_hash, response FROM artifact_producer_submissions WHERE idempotency_key = ?1",
            params![key.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(backend)?;
        match row {
            None => Ok(None),
            Some((project, prior, response))
                if project == project_id.to_string() && prior == hash.as_str() =>
            {
                Ok(Some(response))
            }
            Some(_) => Err(RepositoryError::Conflict {
                subject: "artifact submission",
                rule: "idempotency key already records a different request",
            }),
        }
    }

    /// Append both producer registry record and augmentation receipt in one transaction.
    pub fn record_artifact_submission(
        &self,
        request: &NewArtifactSubmission,
    ) -> RepositoryResult<String> {
        let transaction = self.begin()?;
        if let Some(response) = self.artifact_submission_replay(
            request.evidence.binding.project_id,
            &request.idempotency_key,
            &request.request_hash,
        )? {
            transaction.commit().map_err(backend)?;
            return Ok(response);
        }
        let e = &request.evidence;
        // Re-prove all immutable attribution and the mutable workflow/revision fence
        // inside the same transaction as the two inserts.
        let valid: bool = transaction.query_row(
            "SELECT EXISTS (
                SELECT 1 FROM role_turns turn
                JOIN tasks task ON task.project_id = turn.project_id AND task.id = turn.task_id
                JOIN team_runs team ON team.project_id = turn.project_id AND team.id = turn.team_run_id AND team.task_id = turn.task_id
                JOIN agent_runs run ON run.project_id = turn.project_id AND run.id = turn.agent_run_id AND run.team_run_id = turn.team_run_id
                JOIN task_workflows workflow ON workflow.project_id = task.project_id AND workflow.task_id = task.id AND workflow.active = 1
                WHERE turn.id = ?1 AND turn.project_id = ?2 AND turn.task_id = ?3
                  AND task.revision = ?4 AND workflow.id = ?5 AND run.id = ?6
                  AND turn.settled_at >= workflow.created_at
                  AND run.role_key = ?7 AND run.account_profile_id IS ?8
                  AND (turn.account_profile IS NULL OR turn.account_profile IS ?8)
                  AND EXISTS (SELECT 1 FROM json_each(turn.artifacts) a WHERE a.type = 'text' AND a.value = ?9)
            )",
            params![request.role_turn_id.to_string(), e.binding.project_id.to_string(), e.binding.task_id.to_string(),
                i64::try_from(request.task_revision.get()).map_err(|_| DomainError::invalid("artifact submission", "task revision exceeds storage range"))?, e.binding.workflow_id.to_string(), e.binding.agent_run_id.map(|id| id.to_string()),
                e.producer_role.as_str(), e.producer_account.map(|account| account.to_string()), e.key.as_str()], |row| row.get(0)).map_err(backend)?;
        if !valid {
            return Err(RepositoryError::Domain(DomainError::invalid(
                "artifact submission",
                "the task, workflow, settled claim, or producer attribution moved",
            )));
        }
        insert_artifact_evidence(&transaction, e)?;
        transaction.execute(
            "INSERT INTO artifact_producer_submissions (evidence_id, project_id, task_id, role_turn_id, idempotency_key, request_hash, authority_tier, provenance, turn_proof_class, response)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'operator_recovered_git_blob', ?8, ?9)",
            params![e.id.to_string(), e.binding.project_id.to_string(), e.binding.task_id.to_string(), request.role_turn_id.to_string(),
                request.idempotency_key.as_str(), request.request_hash.as_str(), request.authority_tier, request.turn_proof_class, request.response.json()]).map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(request.response.json().to_owned())
    }
}
