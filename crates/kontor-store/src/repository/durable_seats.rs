//! Durable desk seats: dated declarations of the long-lived seats a desk holds.
//!
//! One immutable, permanent row per declared seat (ASMA-8451). The schema's
//! triggers refuse both an update and a delete, so a decision keeps who made
//! it, when, and who may restart the seat for its whole life. Declaring the
//! identical seat again is unchanged; declaring something different under the
//! same key or title is a conflict.

use kontor_core::durable_seat::{DurableSeatDeclaration, DurableSeatRecoveryOwner};
use kontor_core::id::{DeskKey, DurableSeatKey, ExternalName, ProjectId, RoleCode, SeatBindingId};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

use super::{SqliteStore, backend, from_json, read_timestamp, text, to_json};
use crate::graph::Applied;

const DECLARATION_COLUMNS: &str = "project_id, desk_key, seat_key, title, role_code, seat_label,
     model_rung, procedure, recovery_owner, decided_by, decided_on, seat_binding_id,
     declared_at";

type DeclarationRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    String,
);

impl SqliteStore {
    /// Record one durable seat declaration, or prove the stored one is the
    /// same decision.
    ///
    /// Returns the stored row, which for an existing seat keeps its original
    /// binding and instant: a retry whose answer was lost minted fresh ones,
    /// and the seat is still the one first recorded.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the key already declares a
    /// different seat, another seat in the desk already has this title, or the
    /// desk does not exist.
    pub fn declare_durable_seat(
        &self,
        declaration: &DurableSeatDeclaration,
    ) -> RepositoryResult<(DurableSeatDeclaration, Applied)> {
        let transaction = self.begin()?;
        let existing = transaction
            .query_row(
                &format!(
                    "SELECT {DECLARATION_COLUMNS} FROM durable_seat_declarations
                     WHERE project_id = ?1 AND desk_key = ?2 AND seat_key = ?3"
                ),
                params![
                    declaration.project_id.to_string(),
                    declaration.desk_key.as_str(),
                    declaration.seat_key.as_str(),
                ],
                read_row,
            )
            .optional()
            .map_err(backend)?
            .map(decode)
            .transpose()?;
        if let Some(existing) = existing {
            transaction.rollback().map_err(backend)?;
            if existing.declares_same_seat(declaration) {
                return Ok((existing, Applied::Unchanged));
            }
            return Err(RepositoryError::Conflict {
                subject: "durable seat declaration",
                rule: "this key already declares a different seat; a decision is immutable",
            });
        }
        let model_rung = declaration.model_rung.as_ref().map(to_json).transpose()?;
        transaction
            .execute(
                "INSERT INTO durable_seat_declarations
                     (project_id, desk_key, seat_key, title, role_code, seat_label,
                      model_rung, procedure, recovery_owner, decided_by, decided_on,
                      seat_binding_id, declared_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    declaration.project_id.to_string(),
                    declaration.desk_key.as_str(),
                    declaration.seat_key.as_str(),
                    declaration.title.as_str(),
                    declaration.role_code.as_str(),
                    declaration.seat_label.as_str(),
                    model_rung,
                    declaration.procedure.as_ref().map(ExternalName::as_str),
                    declaration.recovery_owner.as_str(),
                    declaration.decided_by.as_str(),
                    declaration.decided_on.to_string(),
                    declaration.seat_binding_id.map(|id| id.to_string()),
                    text(declaration.declared_at),
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    RepositoryError::Conflict {
                        subject: "durable seat declaration",
                        rule: "the desk must exist and one title names one seat in it",
                    }
                }
                other => backend(other),
            })?;
        transaction.commit().map_err(backend)?;
        Ok((declaration.clone(), Applied::Created))
    }

    /// One declared seat, by its key inside its desk.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn get_durable_seat_declaration(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
        seat_key: &DurableSeatKey,
    ) -> RepositoryResult<Option<DurableSeatDeclaration>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {DECLARATION_COLUMNS} FROM durable_seat_declarations
                     WHERE project_id = ?1 AND desk_key = ?2 AND seat_key = ?3"
                ),
                params![project_id.to_string(), desk_key.as_str(), seat_key.as_str()],
                read_row,
            )
            .optional()
            .map_err(backend)?
            .map(decode)
            .transpose()
    }

    /// Every seat declared in one desk, by key.
    ///
    /// # Errors
    /// Returns a backend or decoding error.
    pub fn list_durable_seat_declarations(
        &self,
        project_id: ProjectId,
        desk_key: &DeskKey,
    ) -> RepositoryResult<Vec<DurableSeatDeclaration>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {DECLARATION_COLUMNS} FROM durable_seat_declarations
                 WHERE project_id = ?1 AND desk_key = ?2 ORDER BY seat_key"
            ))
            .map_err(backend)?;
        let rows = statement
            .query_map(params![project_id.to_string(), desk_key.as_str()], read_row)
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        rows.into_iter().map(decode).collect()
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeclarationRow> {
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
}

fn decode(row: DeclarationRow) -> RepositoryResult<DurableSeatDeclaration> {
    Ok(DurableSeatDeclaration {
        project_id: ProjectId::parse(&row.0)?,
        desk_key: DeskKey::parse(&row.1)?,
        seat_key: DurableSeatKey::parse(&row.2)?,
        title: ExternalName::parse(&row.3)?,
        role_code: RoleCode::parse(&row.4)?,
        seat_label: ExternalName::parse(&row.5)?,
        model_rung: row.6.as_deref().map(from_json).transpose()?,
        procedure: row.7.as_deref().map(ExternalName::parse).transpose()?,
        recovery_owner: DurableSeatRecoveryOwner::parse(&row.8)?,
        decided_by: ExternalName::parse(&row.9)?,
        decided_on: row.10.parse().map_err(|_| RepositoryError::Backend {
            detail: "a durable seat decision date could not be decoded".to_owned(),
        })?,
        seat_binding_id: row.11.as_deref().map(SeatBindingId::parse).transpose()?,
        declared_at: read_timestamp(&row.12)?,
    })
}
