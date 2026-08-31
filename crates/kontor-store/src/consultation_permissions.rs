//! Durable Committee consultation permission-response effects.

#![allow(missing_docs)]

use kontor_core::id::{
    CommitteeRunId, ExternalId, ProjectId, SeatBindingId, Timestamp, format_utc_timestamp,
    parse_utc_timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, params};

use crate::SqliteStore;
use crate::repository::backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsultationPermissionResponseStatus {
    Planned,
    Dispatching,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsultationPermissionDecision {
    Allow,
    Deny,
}

impl ConsultationPermissionResponseStatus {
    fn parse(value: &str) -> RepositoryResult<Self> {
        match value {
            "planned" => Ok(Self::Planned),
            "dispatching" => Ok(Self::Dispatching),
            "confirmed" => Ok(Self::Confirmed),
            _ => Err(RepositoryError::Conflict {
                subject: "Committee permission response",
                rule: "the durable response has an unknown status",
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConsultationPermissionResponse {
    pub project_id: ProjectId,
    pub committee_run_id: CommitteeRunId,
    pub seat_binding_id: SeatBindingId,
    pub occupancy_generation: u64,
    pub native_id: ExternalId,
    pub permission_id: ExternalId,
    pub response_id: ExternalId,
    pub decision: ConsultationPermissionDecision,
    pub status: ConsultationPermissionResponseStatus,
    pub planned_at: Timestamp,
    pub accepted_at: Option<Timestamp>,
}

fn decision_text(decision: ConsultationPermissionDecision) -> &'static str {
    match decision {
        ConsultationPermissionDecision::Allow => "allow",
        ConsultationPermissionDecision::Deny => "deny",
    }
}

fn read_response(
    row: &rusqlite::Row<'_>,
) -> Result<StoredConsultationPermissionResponse, rusqlite::Error> {
    let occupancy_generation = row.get::<_, i64>(3)?;
    let accepted_at = row.get::<_, Option<String>>(10)?;
    Ok(StoredConsultationPermissionResponse {
        project_id: ProjectId::parse(&row.get::<_, String>(0)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        committee_run_id: CommitteeRunId::parse(&row.get::<_, String>(1)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        seat_binding_id: SeatBindingId::parse(&row.get::<_, String>(2)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        occupancy_generation: u64::try_from(occupancy_generation).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        native_id: ExternalId::parse(&row.get::<_, String>(4)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        permission_id: ExternalId::parse(&row.get::<_, String>(5)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        response_id: ExternalId::parse(&row.get::<_, String>(6)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        decision: match row.get::<_, String>(7)?.as_str() {
            "allow" => ConsultationPermissionDecision::Allow,
            "deny" => ConsultationPermissionDecision::Deny,
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    7,
                    "decision".to_owned(),
                    rusqlite::types::Type::Text,
                ));
            }
        },
        status: ConsultationPermissionResponseStatus::parse(&row.get::<_, String>(8)?).map_err(
            |error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            },
        )?,
        planned_at: parse_utc_timestamp(&row.get::<_, String>(9)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        accepted_at: accepted_at
            .as_deref()
            .map(parse_utc_timestamp)
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
    })
}

const SELECT: &str = "SELECT project_id, committee_run_id, seat_binding_id, occupancy_generation,
            native_id, permission_id, response_id, decision, status, planned_at, accepted_at
     FROM consultation_permission_responses";

impl SqliteStore {
    pub fn get_consultation_permission_response(
        &self,
        response_id: &ExternalId,
    ) -> RepositoryResult<Option<StoredConsultationPermissionResponse>> {
        self.connection
            .query_row(
                &format!("{SELECT} WHERE response_id = ?1"),
                [response_id.to_string()],
                read_response,
            )
            .optional()
            .map_err(backend)
    }

    pub fn plan_consultation_permission_response(
        &self,
        response: &StoredConsultationPermissionResponse,
    ) -> RepositoryResult<StoredConsultationPermissionResponse> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO consultation_permission_responses
                    (project_id, committee_run_id, seat_binding_id, occupancy_generation,
                     native_id, permission_id, response_id, decision, status, planned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'planned', ?9)",
                params![
                    response.project_id.to_string(),
                    response.committee_run_id.to_string(),
                    response.seat_binding_id.to_string(),
                    i64::try_from(response.occupancy_generation).unwrap_or(i64::MAX),
                    response.native_id.as_str(),
                    response.permission_id.as_str(),
                    response.response_id.to_string(),
                    decision_text(response.decision),
                    format_utc_timestamp(response.planned_at),
                ],
            )
            .map_err(backend)?;
        let stored = transaction
            .query_row(
                &format!("{SELECT} WHERE response_id = ?1"),
                [response.response_id.to_string()],
                read_response,
            )
            .map_err(backend)?;
        if stored.project_id != response.project_id
            || stored.committee_run_id != response.committee_run_id
            || stored.seat_binding_id != response.seat_binding_id
            || stored.occupancy_generation != response.occupancy_generation
            || stored.native_id != response.native_id
            || stored.permission_id != response.permission_id
            || stored.decision != response.decision
        {
            return Err(RepositoryError::Conflict {
                subject: "Committee permission response",
                rule: "the response id already names another exact permission answer",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    pub fn claim_consultation_permission_response(
        &self,
        response_id: &ExternalId,
    ) -> RepositoryResult<StoredConsultationPermissionResponse> {
        let transaction = self.begin()?;
        let changed = transaction
            .execute(
                "UPDATE consultation_permission_responses SET status = 'dispatching'
                 WHERE response_id = ?1 AND status = 'planned'",
                [response_id.to_string()],
            )
            .map_err(backend)?;
        let stored = transaction
            .query_row(
                &format!("{SELECT} WHERE response_id = ?1"),
                [response_id.to_string()],
                read_response,
            )
            .optional()
            .map_err(backend)?
            .ok_or(RepositoryError::NotFound {
                subject: "Committee permission response",
            })?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "Committee permission response",
                rule: "the response is already dispatching or confirmed",
            });
        }
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }

    pub fn confirm_consultation_permission_response(
        &self,
        response_id: &ExternalId,
        accepted_at: Timestamp,
    ) -> RepositoryResult<StoredConsultationPermissionResponse> {
        let transaction = self.begin()?;
        let changed = transaction
            .execute(
                "UPDATE consultation_permission_responses
                 SET status = 'confirmed', accepted_at = ?1
                 WHERE response_id = ?2 AND status = 'dispatching'",
                params![format_utc_timestamp(accepted_at), response_id.to_string()],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(RepositoryError::Conflict {
                subject: "Committee permission response",
                rule: "only the claimed exact response may be confirmed",
            });
        }
        let stored = transaction
            .query_row(
                &format!("{SELECT} WHERE response_id = ?1"),
                [response_id.to_string()],
                read_response,
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(stored)
    }
}
