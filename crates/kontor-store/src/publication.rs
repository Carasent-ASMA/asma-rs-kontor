//! Durable publication attestations (ASMA-8101).
//!
//! One row per judged publication. Accepted and refused decisions are both
//! kept: a refusal is the evidence that the rail held. Writes are idempotent per
//! caller key; a different publication under a used key is a conflict.

use kontor_core::id::{
    ExternalName, IdempotencyKey, MiniProjectId, ProjectId, PublicationAttestationId, TaskId,
    Timestamp,
};
use kontor_core::publication::{CommitSha, PublicationDecision};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

use crate::SqliteStore;
use crate::repository::{backend, conflict, read_timestamp, text};

/// One publication decision to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPublicationAttestation {
    /// The attestation id to use if this call records.
    pub id: PublicationAttestationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic the branch key resolved to, when it resolved.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task the branch key resolved to, when the branch names a task.
    pub task_id: Option<TaskId>,
    /// The forge repository, as `owner/name`.
    pub repository: ExternalName,
    /// The branch the publication targets.
    pub base_branch: ExternalName,
    /// The branch being published, as observed.
    pub head_branch: ExternalName,
    /// The exact commit at its head.
    pub head_sha: CommitSha,
    /// The pull request, when one exists.
    pub pull_request: Option<u64>,
    /// The pull-request title, when one exists.
    pub title: Option<ExternalName>,
    /// The typed answer.
    pub decision: PublicationDecision,
    /// The caller's stable key.
    pub idempotency_key: IdempotencyKey,
    /// When the decision was made.
    pub recorded_at: Timestamp,
}

/// One recorded publication decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationAttestation {
    /// The attestation id.
    pub id: PublicationAttestationId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The epic the branch key resolved to, when it resolved.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task the branch key resolved to, when the branch names a task.
    pub task_id: Option<TaskId>,
    /// The forge repository, as `owner/name`.
    pub repository: ExternalName,
    /// The branch the publication targets.
    pub base_branch: ExternalName,
    /// The branch being published, as observed.
    pub head_branch: ExternalName,
    /// The exact commit at its head.
    pub head_sha: CommitSha,
    /// The pull request, when one exists.
    pub pull_request: Option<u64>,
    /// The pull-request title, when one exists.
    pub title: Option<ExternalName>,
    /// The typed answer.
    pub decision: PublicationDecision,
    /// When the decision was made.
    pub recorded_at: Timestamp,
}

/// Whether a record call wrote or replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationRecord {
    /// This call persisted the decision.
    Recorded(PublicationAttestation),
    /// The same idempotency key already owns this exact publication.
    Replayed(PublicationAttestation),
}

impl AttestationRecord {
    /// The attestation either way.
    #[must_use]
    pub fn attestation(&self) -> &PublicationAttestation {
        match self {
            Self::Recorded(attestation) | Self::Replayed(attestation) => attestation,
        }
    }

    /// Whether this call replayed an earlier decision.
    #[must_use]
    pub const fn is_replay(&self) -> bool {
        matches!(self, Self::Replayed(_))
    }
}

const COLUMNS: &str = "id, project_id, mini_project_id, task_id, repository, base_branch, head_branch, \
                       head_sha, pull_request, title, accepted, reasons, policy_revision, recorded_at";

impl SqliteStore {
    /// Record one publication decision under the caller's idempotency key.
    ///
    /// # Errors
    /// A used key naming a different repository, branch, commit, base, pull
    /// request or title is a conflict; the original decision is never rewritten.
    pub fn record_publication_attestation(
        &self,
        new: &NewPublicationAttestation,
    ) -> RepositoryResult<AttestationRecord> {
        if let Some(existing) =
            self.publication_attestation_by_key(new.project_id, &new.idempotency_key)?
        {
            let same = existing.repository == new.repository
                && existing.base_branch == new.base_branch
                && existing.head_branch == new.head_branch
                && existing.head_sha == new.head_sha
                && existing.pull_request == new.pull_request
                && existing.title == new.title;
            if !same {
                return Err(conflict(
                    "publication attestation",
                    "the idempotency key already names another publication",
                ));
            }
            return Ok(AttestationRecord::Replayed(existing));
        }
        let reasons =
            serde_json::to_string(&new.decision.reasons).map_err(|_| RepositoryError::Backend {
                detail: "publication reasons could not be serialized".to_owned(),
            })?;
        self.connection
            .execute(
                "INSERT INTO publication_attestations
                 (id, project_id, mini_project_id, task_id, repository, base_branch, head_branch,
                  head_sha, pull_request, title, accepted, reasons, policy_revision,
                  idempotency_key, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    new.id.to_string(),
                    new.project_id.to_string(),
                    new.mini_project_id.map(|id| id.to_string()),
                    new.task_id.map(|id| id.to_string()),
                    new.repository.as_str(),
                    new.base_branch.as_str(),
                    new.head_branch.as_str(),
                    new.head_sha.as_str(),
                    new.pull_request
                        .map(|number| i64::try_from(number).unwrap_or(i64::MAX)),
                    new.title.as_ref().map(ExternalName::as_str),
                    i64::from(new.decision.accepted),
                    reasons,
                    i64::from(new.decision.policy_revision),
                    new.idempotency_key.as_str(),
                    text(new.recorded_at),
                ],
            )
            .map_err(backend)?;
        Ok(AttestationRecord::Recorded(PublicationAttestation {
            id: new.id,
            project_id: new.project_id,
            mini_project_id: new.mini_project_id,
            task_id: new.task_id,
            repository: new.repository.clone(),
            base_branch: new.base_branch.clone(),
            head_branch: new.head_branch.clone(),
            head_sha: new.head_sha.clone(),
            pull_request: new.pull_request,
            title: new.title.clone(),
            decision: new.decision.clone(),
            recorded_at: new.recorded_at,
        }))
    }

    /// One recorded decision by id, within its project.
    pub fn get_publication_attestation(
        &self,
        project_id: ProjectId,
        id: PublicationAttestationId,
    ) -> RepositoryResult<Option<PublicationAttestation>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM publication_attestations WHERE project_id = ?1 AND id = ?2"
                ),
                params![project_id.to_string(), id.to_string()],
                read_attestation,
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn publication_attestation_by_key(
        &self,
        project_id: ProjectId,
        key: &IdempotencyKey,
    ) -> RepositoryResult<Option<PublicationAttestation>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM publication_attestations
                     WHERE project_id = ?1 AND idempotency_key = ?2"
                ),
                params![project_id.to_string(), key.as_str()],
                read_attestation,
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }
}

type Row = (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    i64,
    String,
    i64,
    String,
);

fn read_attestation(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RepositoryResult<PublicationAttestation>> {
    let columns: Row = (
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
        row.get(13)?,
    );
    Ok(build_attestation(columns))
}

fn build_attestation(columns: Row) -> RepositoryResult<PublicationAttestation> {
    let (
        id,
        project_id,
        mini_project_id,
        task_id,
        repository,
        base_branch,
        head_branch,
        head_sha,
        pull_request,
        title,
        accepted,
        reasons,
        policy_revision,
        recorded_at,
    ) = columns;
    let reasons: Vec<String> =
        serde_json::from_str(&reasons).map_err(|_| RepositoryError::Backend {
            detail: "publication reasons are not a JSON array".to_owned(),
        })?;
    Ok(PublicationAttestation {
        id: PublicationAttestationId::parse(&id)?,
        project_id: ProjectId::parse(&project_id)?,
        mini_project_id: mini_project_id
            .as_deref()
            .map(MiniProjectId::parse)
            .transpose()?,
        task_id: task_id.as_deref().map(TaskId::parse).transpose()?,
        repository: ExternalName::parse(&repository)?,
        base_branch: ExternalName::parse(&base_branch)?,
        head_branch: ExternalName::parse(&head_branch)?,
        head_sha: CommitSha::parse(&head_sha)?,
        pull_request: pull_request.map(|number| u64::try_from(number).unwrap_or(0)),
        title: title.as_deref().map(ExternalName::parse).transpose()?,
        decision: PublicationDecision {
            accepted: accepted == 1,
            reasons,
            policy_revision: u32::try_from(policy_revision).unwrap_or(0),
        },
        recorded_at: read_timestamp(&recorded_at)?,
    })
}
