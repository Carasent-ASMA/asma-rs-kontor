//! Restart-safe native session release after a consultation's durable settlement.

use crate::{SqliteStore, repository::backend};
use kontor_core::consultation::ConsultationRunId;
use kontor_core::id::{
    ProjectId, SeatBindingId, Timestamp, format_utc_timestamp, parse_utc_timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use kontor_core::state::NativeRuntimeIdentity;
use rusqlite::params;

/// A persisted release, correlated to one immutable settled consultation seat.
#[derive(Debug, Clone)]
pub struct ConsultationSessionRelease {
    /// Owning project.
    pub project_id: ProjectId,
    /// Settled consultation.
    pub run_id: ConsultationRunId,
    /// Logical seat.
    pub seat_binding_id: SeatBindingId,
    /// Exact native occupant, retained as historical binding evidence.
    pub identity: NativeRuntimeIdentity,
    /// Stable intent timestamp.
    pub requested_at: Timestamp,
}

impl SqliteStore {
    /// Persist a bounded batch of settlement-derived release intents, then
    /// return the least recently attempted pending effects. A permanently
    /// unavailable runtime cannot starve later releases.
    pub fn plan_consultation_releases(
        &self,
        limit: u32,
        now: Timestamp,
    ) -> RepositoryResult<Vec<ConsultationSessionRelease>> {
        let stamp = format_utc_timestamp(now);
        let transaction = self.begin()?;
        transaction.execute(
            "INSERT INTO consultation_session_releases
                (project_id, run_id, seat_binding_id, native_id, native_identity, requested_at)
             SELECT s.project_id, s.run_id, s.seat_binding_id, s.native_id,
                    json_object('runtime_kind', s.runtime_kind, 'host', s.host,
                                'generation', s.generation, 'native_id', s.native_id), ?1
               FROM consultation_seats s JOIN consultation_runs r ON r.run_id = s.run_id AND r.project_id = s.project_id
              WHERE r.state = 'settled' AND s.native_id IS NOT NULL
                AND NOT EXISTS (SELECT 1 FROM consultation_session_releases q
                    WHERE q.project_id = s.project_id AND q.run_id = s.run_id
                      AND q.seat_binding_id = s.seat_binding_id AND q.native_id = s.native_id)
              ORDER BY r.settled_at, s.seat_binding_id LIMIT ?2", params![stamp, limit]).map_err(backend)?;
        let rows = {
            let mut query = transaction.prepare(
                "SELECT q.project_id, q.run_id, r.family, q.seat_binding_id, q.native_id, q.requested_at, q.native_identity
                   FROM consultation_session_releases q JOIN consultation_runs r ON r.run_id = q.run_id AND r.project_id = q.project_id
                  WHERE q.archived_at IS NULL AND r.state = 'settled'
                  ORDER BY q.attempted_at, q.requested_at, q.seat_binding_id LIMIT ?1").map_err(backend)?;
            query
                .query_map([limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                })
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?
        };
        for (project, run, _, seat, native, _, _) in &rows {
            transaction.execute("UPDATE consultation_session_releases SET attempted_at = ?1 WHERE project_id = ?2 AND run_id = ?3 AND seat_binding_id = ?4 AND native_id = ?5",
                params![stamp, project, run, seat, native]).map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        rows.into_iter()
            .map(
                |(project, run, family, seat, native, requested, frozen_identity)| {
                    let project_id = ProjectId::parse(&project)?;
                    let seat_binding_id = SeatBindingId::parse(&seat)?;
                    let run_id = match family.as_str() {
                        "committee" => ConsultationRunId::Committee(
                            kontor_core::id::CommitteeRunId::parse(&run)?,
                        ),
                        "advisor" => {
                            ConsultationRunId::Advisor(kontor_core::id::AdvisorRunId::parse(&run)?)
                        }
                        _ => {
                            return Err(RepositoryError::Conflict {
                                subject: "consultation release",
                                rule: "unknown consultation family",
                            });
                        }
                    };
                    let stored = self
                        .get_consultation_seat_by_binding(project_id, seat_binding_id)?
                        .ok_or(RepositoryError::NotFound {
                            subject: "consultation release seat",
                        })?;
                    let frozen_identity: NativeRuntimeIdentity =
                        serde_json::from_str(&frozen_identity).map_err(|_| {
                            RepositoryError::Conflict {
                                subject: "consultation release",
                                rule: "invalid frozen native identity",
                            }
                        })?;
                    let identity = stored
                        .native_identity
                        .filter(|identity| {
                            identity.native_id.as_str() == native && identity == &frozen_identity
                        })
                        .ok_or(RepositoryError::Conflict {
                            subject: "consultation release",
                            rule: "native occupant changed",
                        })?;
                    Ok(ConsultationSessionRelease {
                        project_id,
                        run_id,
                        seat_binding_id,
                        identity,
                        requested_at: parse_utc_timestamp(&requested)?,
                    })
                },
            )
            .collect()
    }

    /// Confirm only the exact persisted native release after runtime readback.
    pub fn confirm_consultation_release(
        &self,
        release: &ConsultationSessionRelease,
        archived_at: Timestamp,
    ) -> RepositoryResult<()> {
        self.connection.execute(
            "UPDATE consultation_session_releases SET archived_at = ?1
             WHERE project_id = ?2 AND run_id = ?3 AND seat_binding_id = ?4 AND native_id = ?5 AND archived_at IS NULL",
            params![format_utc_timestamp(archived_at),release.project_id.to_string(),release.run_id.as_text(),release.seat_binding_id.to_string(),release.identity.native_id.as_str()]).map_err(backend)?;
        Ok(())
    }
}
