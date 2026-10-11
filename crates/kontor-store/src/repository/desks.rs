//! Durable desks: project-level native projects that no epic owns.
//!
//! One row per declared desk in a project, written before either of its nodes
//! and carrying the ids they will use. The row is immutable and permanent (the
//! schema's triggers refuse both an update and a delete), so a desk keeps its
//! nodes and the exact Team Definition revision that names it for its whole
//! life, whatever the project selects later.

use kontor_core::id::{ContentHash, DeskKey, ProjectId, TeamDefinitionId, TopologyNodeId};
use kontor_core::repository::{
    RepositoryError, RepositoryResult, StoredDesk, TeamDefinitionRepository,
};
use kontor_core::spec::TeamDefinitionSnapshot;
use kontor_core::state::SessionTopologyNode;
use rusqlite::{OptionalExtension, params};

use super::{SqliteStore, backend, read_timestamp, read_version, text, version_column};

const DESK_COLUMNS: &str = "desk_key, topology_node_id, workspace_node_id, team_definition_id,
     team_definition_version, team_definition_hash, created_at";

type DeskRow = (String, String, String, String, i64, String, String);

impl SqliteStore {
    /// Record one desk and the node ids its placement will use.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the project already has a
    /// desk under this key, either planned node already belongs to a desk, or
    /// the pinned Team Definition revision is not published in the project.
    pub fn create_desk(&self, desk: &StoredDesk) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        create_desk_in(&transaction, desk)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Record a fresh desk and both logical nodes as one atomic write.
    ///
    /// # Errors
    /// Returns the existing desk/node validation errors. A refused node rolls
    /// back the desk reservation and every node inserted by this transaction.
    pub fn create_desk_topology(
        &self,
        desk: &StoredDesk,
        root: &SessionTopologyNode,
    ) -> RepositoryResult<SessionTopologyNode> {
        let transaction = self.begin()?;
        create_desk_in(&transaction, desk)?;
        let definition = self
            .get_team_definition(
                desk.project_id,
                desk.team_definition.definition_id,
                desk.team_definition.version,
            )?
            .ok_or(RepositoryError::NotFound {
                subject: "desk Team Definition",
            })?;
        let declaration = definition
            .desk(&desk.desk_key)
            .ok_or(RepositoryError::NotFound {
                subject: "desk declaration",
            })?;
        let node = |id, kind, parent_id| kontor_core::repository::NewSessionTopologyNode {
            id,
            project_id: desk.project_id,
            mini_project_id: None,
            topology: definition.topology.clone(),
            kind,
            parent_id: Some(parent_id),
            task_id: None,
            created_at: desk.created_at,
        };
        super::create_topology_node_in(
            &transaction,
            &node(desk.topology_node_id, declaration.kind.clone(), root.id),
        )?;
        let workspace = super::create_topology_node_in(
            &transaction,
            &node(
                desk.workspace_node_id,
                declaration.workspace_kind.clone(),
                desk.topology_node_id,
            ),
        )?;
        transaction.commit().map_err(backend)?;
        Ok(workspace)
    }

    /// One desk, by its declared key.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_desk(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> RepositoryResult<Option<StoredDesk>> {
        self.desk_where("desk_key = ?2", project_id, desk_key.as_str())
    }

    /// The desk one node realizes, whether it is the desk or its workspace.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_desk_by_node(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
    ) -> RepositoryResult<Option<StoredDesk>> {
        self.desk_where(
            "(topology_node_id = ?2 OR workspace_node_id = ?2)",
            project_id,
            &topology_node_id.to_string(),
        )
    }

    /// Every desk in one project, by key.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_desks(&self, project_id: ProjectId) -> RepositoryResult<Vec<StoredDesk>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {DESK_COLUMNS} FROM desks WHERE project_id = ?1 ORDER BY desk_key"
            ))
            .map_err(backend)?;
        let rows = statement
            .query_map(params![project_id.to_string()], read_row)
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| decode(project_id, row))
            .collect()
    }

    fn desk_where(
        &self,
        predicate: &str,
        project_id: ProjectId,
        value: &str,
    ) -> RepositoryResult<Option<StoredDesk>> {
        self.connection
            .query_row(
                &format!("SELECT {DESK_COLUMNS} FROM desks WHERE project_id = ?1 AND {predicate}"),
                params![project_id.to_string(), value],
                read_row,
            )
            .optional()
            .map_err(backend)?
            .map(|row| decode(project_id, row))
            .transpose()
    }
}

fn create_desk_in(
    transaction: &rusqlite::Transaction<'_>,
    desk: &StoredDesk,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO desks
                     (project_id, desk_key, topology_node_id, workspace_node_id,
                      team_definition_id, team_definition_version, team_definition_hash,
                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                desk.project_id.to_string(),
                desk.desk_key.as_str(),
                desk.topology_node_id.to_string(),
                desk.workspace_node_id.to_string(),
                desk.team_definition.definition_id.to_string(),
                version_column(desk.team_definition.version),
                desk.team_definition.canonical_hash.as_str(),
                text(desk.created_at),
            ],
        )
        .map_err(|error| match error {
            // Two ensures of one desk racing. The loser has written nothing
            // else yet — the row is deliberately first — so it reads the
            // winner's desk and reconciles that one.
            rusqlite::Error::SqliteFailure(failure, _)
                if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                RepositoryError::Conflict {
                    subject: "desk",
                    rule: "one key names one desk in a project",
                }
            }
            other => backend(other),
        })?;
    Ok(())
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeskRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn decode(project_id: ProjectId, row: DeskRow) -> RepositoryResult<StoredDesk> {
    Ok(StoredDesk {
        project_id,
        desk_key: DeskKey::parse(&row.0)?,
        topology_node_id: TopologyNodeId::parse(&row.1)?,
        workspace_node_id: TopologyNodeId::parse(&row.2)?,
        team_definition: TeamDefinitionSnapshot {
            definition_id: TeamDefinitionId::parse(&row.3)?,
            version: read_version(row.4)?,
            canonical_hash: ContentHash::parse(&row.5)?,
        },
        created_at: read_timestamp(&row.6)?,
    })
}
