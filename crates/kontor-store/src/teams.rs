//! Durable Teams editor documents and immutable published revisions.

use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::SqliteStore;
use crate::repository::{backend, conflict};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One mutable Teams draft stored as canonical JSON slots.
pub struct StoredTeamDraft {
    /// Logical template id.
    pub id: String,
    /// Current draft name.
    pub name: String,
    /// Slot document.
    pub slots_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One immutable published Teams revision.
pub struct StoredTeamRevision {
    /// Logical template id.
    pub id: String,
    /// Monotonic version.
    pub version: u32,
    /// Frozen name.
    pub name: String,
    /// Frozen slot document.
    pub slots_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// The complete durable Teams read projection.
pub struct StoredTeamsProjection {
    /// Position advanced by each successful Teams write.
    pub cursor: i64,
    /// Current drafts.
    pub drafts: Vec<StoredTeamDraft>,
    /// Published revisions.
    pub revisions: Vec<StoredTeamRevision>,
}

impl SqliteStore {
    /// Read the complete Teams projection at one cursor.
    pub fn teams_projection(&self) -> RepositoryResult<StoredTeamsProjection> {
        read_projection(&self.connection)
    }

    /// Create or replace a draft under a durable idempotency binding.
    pub fn save_team_draft(
        &self,
        key: &str,
        fingerprint: &str,
        draft: &StoredTeamDraft,
    ) -> RepositoryResult<StoredTeamsProjection> {
        let transaction = self.connection.unchecked_transaction().map_err(backend)?;
        if let Some(answer) = replay(&transaction, key, fingerprint)? {
            transaction.commit().map_err(backend)?;
            return Ok(answer);
        }
        transaction.execute(
            "INSERT INTO team_drafts(team_id, name, slots_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(team_id) DO UPDATE SET name = excluded.name, slots_json = excluded.slots_json",
            params![draft.id, draft.name, draft.slots_json],
        ).map_err(backend)?;
        transaction
            .execute(
                "UPDATE teams_projection SET cursor = cursor + 1 WHERE singleton = 1",
                [],
            )
            .map_err(backend)?;
        let answer = read_projection(&transaction)?;
        record_replay(&transaction, key, fingerprint, &answer)?;
        transaction.commit().map_err(backend)?;
        Ok(answer)
    }

    /// Publish the next immutable revision under a durable idempotency binding.
    pub fn publish_team(
        &self,
        key: &str,
        fingerprint: &str,
        team_id: &str,
    ) -> RepositoryResult<Option<StoredTeamsProjection>> {
        let transaction = self.connection.unchecked_transaction().map_err(backend)?;
        if let Some(answer) = replay(&transaction, key, fingerprint)? {
            transaction.commit().map_err(backend)?;
            return Ok(Some(answer));
        }
        let draft = transaction
            .query_row(
                "SELECT team_id, name, slots_json FROM team_drafts WHERE team_id = ?1",
                [team_id],
                |row| {
                    Ok(StoredTeamDraft {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        slots_json: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(backend)?;
        let Some(draft) = draft else {
            transaction.commit().map_err(backend)?;
            return Ok(None);
        };
        let next: u32 = transaction
            .query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM team_revisions WHERE team_id = ?1",
                [team_id],
                |row| row.get(0),
            )
            .map_err(backend)?;
        transaction.execute(
            "INSERT INTO team_revisions(team_id, version, name, slots_json) VALUES (?1, ?2, ?3, ?4)",
            params![draft.id, next, draft.name, draft.slots_json],
        ).map_err(backend)?;
        transaction
            .execute(
                "UPDATE teams_projection SET cursor = cursor + 1 WHERE singleton = 1",
                [],
            )
            .map_err(backend)?;
        let answer = read_projection(&transaction)?;
        record_replay(&transaction, key, fingerprint, &answer)?;
        transaction.commit().map_err(backend)?;
        Ok(Some(answer))
    }
}

fn read_projection(connection: &Connection) -> RepositoryResult<StoredTeamsProjection> {
    let cursor = connection
        .query_row(
            "SELECT cursor FROM teams_projection WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(backend)?;
    let mut drafts_statement = connection
        .prepare("SELECT team_id, name, slots_json FROM team_drafts ORDER BY team_id")
        .map_err(backend)?;
    let drafts = drafts_statement
        .query_map([], |row| {
            Ok(StoredTeamDraft {
                id: row.get(0)?,
                name: row.get(1)?,
                slots_json: row.get(2)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    let mut revisions_statement = connection.prepare(
        "SELECT team_id, version, name, slots_json FROM team_revisions ORDER BY team_id, version",
    ).map_err(backend)?;
    let revisions = revisions_statement
        .query_map([], |row| {
            Ok(StoredTeamRevision {
                id: row.get(0)?,
                version: row.get(1)?,
                name: row.get(2)?,
                slots_json: row.get(3)?,
            })
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    Ok(StoredTeamsProjection {
        cursor,
        drafts,
        revisions,
    })
}

fn replay(
    connection: &Connection,
    key: &str,
    fingerprint: &str,
) -> RepositoryResult<Option<StoredTeamsProjection>> {
    let row: Option<(String, String)> = connection.query_row(
        "SELECT fingerprint, response_json FROM team_command_replays WHERE idempotency_key = ?1",
        [key], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(backend)?;
    let Some((bound, response)) = row else {
        return Ok(None);
    };
    if bound != fingerprint {
        return Err(conflict(
            "Teams command",
            "the idempotency key names different content",
        ));
    }
    serde_json::from_str(&response)
        .map(Some)
        .map_err(json_error)
}

fn record_replay(
    connection: &Connection,
    key: &str,
    fingerprint: &str,
    answer: &StoredTeamsProjection,
) -> RepositoryResult<()> {
    let response = serde_json::to_string(answer).map_err(json_error)?;
    connection.execute(
        "INSERT INTO team_command_replays(idempotency_key, fingerprint, response_json) VALUES (?1, ?2, ?3)",
        params![key, fingerprint, response],
    ).map_err(backend)?;
    Ok(())
}

fn json_error(error: serde_json::Error) -> RepositoryError {
    RepositoryError::Backend {
        detail: format!("stored Teams JSON is invalid: {error}"),
    }
}
