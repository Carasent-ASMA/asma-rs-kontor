//! PR review subjects: which pull request one Adam tab and one review room on
//! a PR desk serve.
//!
//! One row per (desk, repository, number), written before either node and
//! carrying the ids they will use. The row is immutable and permanent (the
//! schema's triggers refuse an update and a delete), and each node serves one
//! pull request. Closing a review releases its seats, never this row.

use kontor_core::id::{
    ContentHash, DeskKey, ExternalId, MiniProjectId, ProjectId, TaskId, TeamDefinitionId,
    TopologyNodeId,
};
use kontor_core::pull_request::{ForgeRepository, PullRequestRef};
use kontor_core::repository::{RepositoryError, RepositoryResult, StoredPrReviewSubject};
use kontor_core::spec::TeamDefinitionSnapshot;
use rusqlite::{OptionalExtension, params};

use super::{SqliteStore, backend, read_timestamp, read_version, text, version_column};

const SUBJECT_COLUMNS: &str = "desk_key, repository, pull_request_number, jira_key,
     mini_project_id, task_id, adam_node_id, room_node_id, team_definition_id,
     team_definition_version, team_definition_hash, created_at";

type SubjectRow = (
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    i64,
    String,
    String,
);

impl SqliteStore {
    /// Record one PR review subject and the node ids its placement will use.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the pull request already has
    /// a subject on that desk, when either node already serves one, when the
    /// task is not the epic's, or when the desk, epic, task or pinned Team
    /// Definition revision does not exist.
    pub fn create_pr_review_subject(
        &self,
        subject: &StoredPrReviewSubject,
    ) -> RepositoryResult<()> {
        let number = i64::try_from(subject.pull_request.number()).map_err(|_| {
            RepositoryError::Conflict {
                subject: "PR review subject",
                rule: "a pull-request number fits the stored integer",
            }
        })?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO pr_review_subjects
                     (project_id, desk_key, repository, pull_request_number, jira_key,
                      mini_project_id, task_id, adam_node_id, room_node_id,
                      team_definition_id, team_definition_version, team_definition_hash,
                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    subject.project_id.to_string(),
                    subject.desk_key.as_str(),
                    subject.pull_request.repository().as_str(),
                    number,
                    subject.jira_key.as_str(),
                    subject.mini_project_id.to_string(),
                    subject.task_id.map(|id| id.to_string()),
                    subject.adam_node_id.to_string(),
                    subject.room_node_id.to_string(),
                    subject.team_definition.definition_id.to_string(),
                    version_column(subject.team_definition.version),
                    subject.team_definition.canonical_hash.as_str(),
                    text(subject.created_at),
                ],
            )
            .map_err(|error| match error {
                // Two opens of one pull request racing, or a node id already
                // serving another. The row is first, so the loser has written
                // nothing else and reads the winner.
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "PR review subject",
                        rule: "one pull request per desk names one subject, each node serves \
                               one pull request, and a task key is its epic's",
                    }
                }
                other => backend(other),
            })?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// The subject one pull request names on one desk.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_pr_review_subject(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
        pull_request: &PullRequestRef,
    ) -> RepositoryResult<Option<StoredPrReviewSubject>> {
        let Ok(number) = i64::try_from(pull_request.number()) else {
            return Ok(None);
        };
        self.connection
            .query_row(
                &format!(
                    "SELECT {SUBJECT_COLUMNS} FROM pr_review_subjects
                     WHERE project_id = ?1 AND desk_key = ?2
                       AND repository = ?3 AND pull_request_number = ?4"
                ),
                params![
                    project_id.to_string(),
                    desk_key.as_str(),
                    pull_request.repository().as_str(),
                    number,
                ],
                read_row,
            )
            .optional()
            .map_err(backend)?
            .map(|row| decode(project_id, row))
            .transpose()
    }

    /// The subject whose Adam or room is this node, if any.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_pr_review_subject_by_node(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<StoredPrReviewSubject>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {SUBJECT_COLUMNS} FROM pr_review_subjects
                     WHERE project_id = ?1 AND (adam_node_id = ?2 OR room_node_id = ?2)"
                ),
                params![project_id.to_string(), topology_node_id.to_string()],
                read_row,
            )
            .optional()
            .map_err(backend)?
            .map(|row| decode(project_id, row))
            .transpose()
    }

    /// Every subject on one desk, oldest first.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_pr_review_subjects(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> RepositoryResult<Vec<StoredPrReviewSubject>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {SUBJECT_COLUMNS} FROM pr_review_subjects
                 WHERE project_id = ?1 AND desk_key = ?2
                 ORDER BY created_at, repository, pull_request_number"
            ))
            .map_err(backend)?;
        let rows = statement
            .query_map(params![project_id.to_string(), desk_key.as_str()], read_row)
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| decode(project_id, row))
            .collect()
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubjectRow> {
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
    ))
}

fn decode(project_id: ProjectId, row: SubjectRow) -> RepositoryResult<StoredPrReviewSubject> {
    let number = u64::try_from(row.2).map_err(|_| RepositoryError::Conflict {
        subject: "PR review subject",
        rule: "a stored pull-request number is positive",
    })?;
    Ok(StoredPrReviewSubject {
        project_id,
        desk_key: DeskKey::parse(&row.0)?,
        pull_request: PullRequestRef::new(ForgeRepository::parse(&row.1)?, number)?,
        jira_key: ExternalId::parse(&row.3)?,
        mini_project_id: MiniProjectId::parse(&row.4)?,
        task_id: row.5.as_deref().map(TaskId::parse).transpose()?,
        adam_node_id: TopologyNodeId::parse(&row.6)?,
        room_node_id: TopologyNodeId::parse(&row.7)?,
        team_definition: TeamDefinitionSnapshot {
            definition_id: TeamDefinitionId::parse(&row.8)?,
            version: read_version(row.9)?,
            canonical_hash: ContentHash::parse(&row.10)?,
        },
        created_at: read_timestamp(&row.11)?,
    })
}
