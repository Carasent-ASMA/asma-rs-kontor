//! Desk Adam subjects: which work a leadership session on a durable desk does.
//!
//! One row per epic led from a desk and per ad-hoc topic opened there, written
//! before the subject node and carrying the id it will use. The row is
//! immutable and permanent (the schema's triggers refuse an update and a
//! delete), one epic has at most one subject, one ad-hoc subject binds to at
//! most one epic, and a subject hosts one live session at a time.

use kontor_core::id::{
    ContentHash, DeskKey, ExternalName, MiniProjectId, ProjectId, TeamDefinitionId, TopologyNodeId,
};
use kontor_core::repository::{RepositoryError, RepositoryResult, StoredDeskAdamSubject};
use kontor_core::spec::TeamDefinitionSnapshot;
use rusqlite::{OptionalExtension, params};

use super::{SqliteStore, backend, read_timestamp, read_version, text, version_column};

const SUBJECT_COLUMNS: &str = "topology_node_id, desk_key, mini_project_id, topic,
     origin_topology_node_id, team_definition_id, team_definition_version,
     team_definition_hash, created_at";

type SubjectRow = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    i64,
    String,
    String,
);

impl SqliteStore {
    /// Record one desk Adam subject and the node id its placement will use.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the node, the epic, the
    /// ad-hoc topic on that desk or the bound origin already names a subject,
    /// when the origin is not an ad-hoc subject of the same desk or still has
    /// an active seat or a native occupancy, or when the desk, epic or pinned
    /// Team Definition revision does not exist.
    pub fn create_desk_adam_subject(
        &self,
        subject: &StoredDeskAdamSubject,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO desk_adam_subjects
                     (project_id, topology_node_id, desk_key, subject_kind, mini_project_id,
                      topic, origin_topology_node_id, team_definition_id,
                      team_definition_version, team_definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    subject.project_id.to_string(),
                    subject.topology_node_id.to_string(),
                    subject.desk_key.as_str(),
                    if subject.mini_project_id.is_some() {
                        "epic"
                    } else {
                        "ad_hoc"
                    },
                    subject.mini_project_id.map(|id| id.to_string()),
                    subject.topic.as_str(),
                    subject.origin_topology_node_id.map(|id| id.to_string()),
                    subject.team_definition.definition_id.to_string(),
                    version_column(subject.team_definition.version),
                    subject.team_definition.canonical_hash.as_str(),
                    text(subject.created_at),
                ],
            )
            .map_err(|error| match error {
                // Two opens of one subject racing, or a second subject for an
                // epic, topic or origin that already has one. The row is first,
                // so the loser has written nothing else and reads the winner. A
                // binding whose ad-hoc origin is not fenced yet lands here too,
                // and its reread finds no winner.
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "desk Adam subject",
                        rule: "one node, epic, ad-hoc topic or binding names one subject, \
                               and a binding follows the ad-hoc fence",
                    }
                }
                other => backend(other),
            })?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// One subject, by its node.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_desk_adam_subject(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<StoredDeskAdamSubject>> {
        self.subject_where(
            "topology_node_id = ?2",
            project_id,
            &topology_node_id.to_string(),
        )
    }

    /// The one subject that leads an epic, if the epic is led from a desk.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_epic_desk_adam_subject(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<StoredDeskAdamSubject>> {
        self.subject_where(
            "mini_project_id = ?2",
            project_id,
            &mini_project_id.to_string(),
        )
    }

    /// The epic subject an ad-hoc subject was bound to, if it has been.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_desk_adam_subject_bound_from(
        &self,
        project_id: ProjectId,
        origin_topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<StoredDeskAdamSubject>> {
        self.subject_where(
            "origin_topology_node_id = ?2",
            project_id,
            &origin_topology_node_id.to_string(),
        )
    }

    /// The ad-hoc subject one topic names on one desk.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_ad_hoc_desk_adam_subject(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
        topic: &ExternalName,
    ) -> RepositoryResult<Option<StoredDeskAdamSubject>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {SUBJECT_COLUMNS} FROM desk_adam_subjects
                     WHERE project_id = ?1 AND desk_key = ?2 AND topic = ?3
                       AND subject_kind = 'ad_hoc'"
                ),
                params![project_id.to_string(), desk_key.as_str(), topic.as_str()],
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
    pub fn list_desk_adam_subjects(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> RepositoryResult<Vec<StoredDeskAdamSubject>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {SUBJECT_COLUMNS} FROM desk_adam_subjects
                 WHERE project_id = ?1 AND desk_key = ?2
                 ORDER BY created_at, topology_node_id"
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

    fn subject_where(
        &self,
        predicate: &str,
        project_id: ProjectId,
        value: &str,
    ) -> RepositoryResult<Option<StoredDeskAdamSubject>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {SUBJECT_COLUMNS} FROM desk_adam_subjects
                     WHERE project_id = ?1 AND {predicate}"
                ),
                params![project_id.to_string(), value],
                read_row,
            )
            .optional()
            .map_err(backend)?
            .map(|row| decode(project_id, row))
            .transpose()
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
    ))
}

fn decode(project_id: ProjectId, row: SubjectRow) -> RepositoryResult<StoredDeskAdamSubject> {
    Ok(StoredDeskAdamSubject {
        project_id,
        topology_node_id: TopologyNodeId::parse(&row.0)?,
        desk_key: DeskKey::parse(&row.1)?,
        mini_project_id: row.2.as_deref().map(MiniProjectId::parse).transpose()?,
        topic: ExternalName::parse(&row.3)?,
        origin_topology_node_id: row.4.as_deref().map(TopologyNodeId::parse).transpose()?,
        team_definition: TeamDefinitionSnapshot {
            definition_id: TeamDefinitionId::parse(&row.5)?,
            version: read_version(row.6)?,
            canonical_hash: ContentHash::parse(&row.7)?,
        },
        created_at: read_timestamp(&row.8)?,
    })
}
