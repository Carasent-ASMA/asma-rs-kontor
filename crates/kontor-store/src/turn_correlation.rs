//! Durable, forward-only correlation challenges for identity-poor native history.

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, AggregateRevision, ArtifactKey, BoundedText, ContentHash, ExternalId, ExternalName,
    ProjectId, RoleSlotId, RoleTurnId, RuntimeBindingId, RuntimeKindKey, SeatBindingId, TaskId,
    TeamRunId, Timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{OptionalExtension, Row, params};

use crate::SqliteStore;
use crate::graph::Applied;
use crate::repository::{backend, conflict, read_timestamp, revision_column, revision_of, text};

/// Durable state of one correlation challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCorrelationState {
    /// The server-owned intent exists and no dispatcher has claimed it.
    Prepared,
    /// Exactly one dispatcher claimed the first native effect.
    Dispatching,
    /// The exact challenge was found once after its stored boundary.
    Acknowledged,
    /// A role turn atomically consumed the acknowledged challenge.
    Settled,
}

impl TurnCorrelationState {
    fn parse(value: &str) -> RepositoryResult<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "dispatching" => Ok(Self::Dispatching),
            "acknowledged" => Ok(Self::Acknowledged),
            "settled" => Ok(Self::Settled),
            _ => Err(DomainError::invalid(
                "turn correlation challenge",
                "has an unknown durable state",
            )
            .into()),
        }
    }
}

/// Immutable server-owned challenge intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTurnCorrelationChallenge {
    /// Server-generated message identity.
    pub message_id: ExternalId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact task described by the evidence.
    pub task_id: TaskId,
    /// Existing team run whose seat receives the challenge.
    pub team_run_id: TeamRunId,
    /// Existing agent run whose seat receives the challenge.
    pub agent_run_id: AgentRunId,
    /// Existing role slot retained by the operation.
    pub role_slot_id: RoleSlotId,
    /// Exact active topology SeatBinding retained by the operation.
    pub seat_binding_id: SeatBindingId,
    /// Exact issued runtime binding.
    pub runtime_binding_id: RuntimeBindingId,
    /// Runtime adapter kind pinned by the binding.
    pub runtime_kind: RuntimeKindKey,
    /// Runtime host pinned by the binding.
    pub runtime_host: ExternalName,
    /// Runtime adapter generation pinned by the binding.
    pub runtime_generation: u64,
    /// Native session identity pinned by the binding.
    pub native_id: ExternalId,
    /// Task revision verified before the intent was stored.
    pub task_revision: AggregateRevision,
    /// Agent-run revision verified before the intent was stored.
    pub agent_run_revision: AggregateRevision,
    /// Exact high-scope artifact the response must confirm.
    pub artifact_key: ArtifactKey,
    /// Approved immutable evidence revision.
    pub evidence_revision_id: ExternalId,
    /// Content hash of that exact evidence revision.
    pub evidence_content_hash: ContentHash,
    /// Approved operational-gap report checksum.
    pub report_checksum: ContentHash,
    /// Kontor epoch of the pre-dispatch boundary.
    pub boundary_epoch: u64,
    /// Sequence of the pre-dispatch boundary.
    pub boundary_sequence: u64,
    /// Runtime-owned spelling of the boundary epoch.
    pub native_epoch: ExternalId,
    /// Exact server-generated challenge body.
    pub body: BoundedText,
    /// Digest of the frozen challenge body.
    pub body_hash: ContentHash,
    /// Exact response text that can complete the challenge.
    pub expected_response: BoundedText,
    /// Hash of the server-owned no-write preview.
    pub preview_hash: ContentHash,
    /// Hash binding the apply arguments.
    pub request_hash: ContentHash,
    /// Caller retry key scoped by the unique database constraint.
    pub idempotency_key: String,
    /// Time at which the server froze the challenge intent.
    pub created_at: Timestamp,
}

/// One persisted correlation challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCorrelationChallenge {
    /// Server-generated message identity.
    pub message_id: ExternalId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact task described by the evidence.
    pub task_id: TaskId,
    /// Existing team run whose seat received the challenge.
    pub team_run_id: TeamRunId,
    /// Existing agent run whose seat received the challenge.
    pub agent_run_id: AgentRunId,
    /// Existing role slot retained by the operation.
    pub role_slot_id: RoleSlotId,
    /// Exact active topology SeatBinding retained by the operation.
    pub seat_binding_id: SeatBindingId,
    /// Exact issued runtime binding.
    pub runtime_binding_id: RuntimeBindingId,
    /// Runtime adapter kind pinned by the binding.
    pub runtime_kind: RuntimeKindKey,
    /// Runtime host pinned by the binding.
    pub runtime_host: ExternalName,
    /// Runtime adapter generation pinned by the binding.
    pub runtime_generation: u64,
    /// Native session identity pinned by the binding.
    pub native_id: ExternalId,
    /// Task revision verified when the intent was stored.
    pub task_revision: AggregateRevision,
    /// Agent-run revision verified when the intent was stored.
    pub agent_run_revision: AggregateRevision,
    /// Exact high-scope artifact the response must confirm.
    pub artifact_key: ArtifactKey,
    /// Approved immutable evidence revision.
    pub evidence_revision_id: ExternalId,
    /// Content hash of that exact evidence revision.
    pub evidence_content_hash: ContentHash,
    /// Approved operational-gap report checksum.
    pub report_checksum: ContentHash,
    /// Kontor epoch of the pre-dispatch boundary.
    pub boundary_epoch: u64,
    /// Sequence of the pre-dispatch boundary.
    pub boundary_sequence: u64,
    /// Runtime-owned spelling of the boundary epoch.
    pub native_epoch: ExternalId,
    /// Exact server-generated challenge body.
    pub body: BoundedText,
    /// Digest of the frozen challenge body.
    pub body_hash: ContentHash,
    /// Exact response text that can complete the challenge.
    pub expected_response: BoundedText,
    /// Hash of the server-owned no-write preview.
    pub preview_hash: ContentHash,
    /// Hash binding the apply arguments.
    pub request_hash: ContentHash,
    /// Caller retry key scoped by the unique database constraint.
    pub idempotency_key: String,
    /// Current forward-only lifecycle state.
    pub state: TurnCorrelationState,
    /// Kontor epoch of the acknowledged native user message.
    pub message_epoch: Option<u64>,
    /// Sequence of the acknowledged native user message.
    pub message_sequence: Option<u64>,
    /// Time at which exact canonical history acknowledged the message.
    pub acknowledged_at: Option<Timestamp>,
    /// Role turn that atomically consumed the challenge.
    pub settled_turn_id: Option<RoleTurnId>,
    /// Time at which the role turn consumed the challenge.
    pub settled_at: Option<Timestamp>,
    /// Time at which the server froze the challenge intent.
    pub created_at: Timestamp,
    /// Time of the latest forward-only lifecycle transition.
    pub updated_at: Timestamp,
}

const COLUMNS: &str = "message_id, project_id, task_id, team_run_id, agent_run_id, \
role_slot_id, seat_binding_id, runtime_binding_id, runtime_kind, runtime_host, runtime_generation, native_id, \
task_revision, agent_run_revision, artifact_key, evidence_revision_id, evidence_content_hash, \
report_checksum, boundary_epoch, boundary_sequence, native_epoch, body, body_hash, \
expected_response, preview_hash, request_hash, idempotency_key, state, message_epoch, \
message_sequence, acknowledged_at, settled_turn_id, settled_at, created_at, updated_at";

fn positive(row: &Row<'_>, index: usize, subject: &'static str) -> RepositoryResult<u64> {
    let value: i64 = row.get(index).map_err(backend)?;
    let value = u64::try_from(value)
        .map_err(|_| DomainError::invalid(subject, "has a negative integer"))?;
    if value == 0 {
        return Err(DomainError::invalid(subject, "must be positive").into());
    }
    Ok(value)
}

fn nonnegative(row: &Row<'_>, index: usize) -> RepositoryResult<u64> {
    u64::try_from(row.get::<_, i64>(index).map_err(backend)?)
        .map_err(|_| DomainError::invalid("turn correlation boundary", "is negative").into())
}

fn read(row: &Row<'_>) -> RepositoryResult<TurnCorrelationChallenge> {
    let message_epoch: Option<i64> = row.get(28).map_err(backend)?;
    let message_sequence: Option<i64> = row.get(29).map_err(backend)?;
    let acknowledged_at: Option<String> = row.get(30).map_err(backend)?;
    let settled_turn_id: Option<String> = row.get(31).map_err(backend)?;
    let settled_at: Option<String> = row.get(32).map_err(backend)?;
    Ok(TurnCorrelationChallenge {
        message_id: ExternalId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        task_id: TaskId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        team_run_id: TeamRunId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        agent_run_id: AgentRunId::parse(&row.get::<_, String>(4).map_err(backend)?)?,
        role_slot_id: RoleSlotId::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        seat_binding_id: SeatBindingId::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        runtime_binding_id: RuntimeBindingId::parse(&row.get::<_, String>(7).map_err(backend)?)?,
        runtime_kind: RuntimeKindKey::parse(&row.get::<_, String>(8).map_err(backend)?)?,
        runtime_host: ExternalName::parse(&row.get::<_, String>(9).map_err(backend)?)?,
        runtime_generation: nonnegative(row, 10)?,
        native_id: ExternalId::parse(&row.get::<_, String>(11).map_err(backend)?)?,
        task_revision: revision_of(row.get::<_, i64>(12).map_err(backend)?)?,
        agent_run_revision: revision_of(row.get::<_, i64>(13).map_err(backend)?)?,
        artifact_key: ArtifactKey::parse(&row.get::<_, String>(14).map_err(backend)?)?,
        evidence_revision_id: ExternalId::parse(&row.get::<_, String>(15).map_err(backend)?)?,
        evidence_content_hash: ContentHash::parse(&row.get::<_, String>(16).map_err(backend)?)?,
        report_checksum: ContentHash::parse(&row.get::<_, String>(17).map_err(backend)?)?,
        boundary_epoch: positive(row, 18, "turn correlation boundary epoch")?,
        boundary_sequence: nonnegative(row, 19)?,
        native_epoch: ExternalId::parse(&row.get::<_, String>(20).map_err(backend)?)?,
        body: BoundedText::parse(&row.get::<_, String>(21).map_err(backend)?)?,
        body_hash: ContentHash::parse(&row.get::<_, String>(22).map_err(backend)?)?,
        expected_response: BoundedText::parse(&row.get::<_, String>(23).map_err(backend)?)?,
        preview_hash: ContentHash::parse(&row.get::<_, String>(24).map_err(backend)?)?,
        request_hash: ContentHash::parse(&row.get::<_, String>(25).map_err(backend)?)?,
        idempotency_key: row.get(26).map_err(backend)?,
        state: TurnCorrelationState::parse(&row.get::<_, String>(27).map_err(backend)?)?,
        message_epoch: message_epoch
            .map(u64::try_from)
            .transpose()
            .map_err(|_| DomainError::invalid("turn correlation message epoch", "is negative"))?,
        message_sequence: message_sequence
            .map(u64::try_from)
            .transpose()
            .map_err(|_| {
                DomainError::invalid("turn correlation message sequence", "is negative")
            })?,
        acknowledged_at: acknowledged_at
            .as_deref()
            .map(kontor_core::id::parse_utc_timestamp)
            .transpose()?,
        settled_turn_id: settled_turn_id
            .as_deref()
            .map(RoleTurnId::parse)
            .transpose()?,
        settled_at: settled_at
            .as_deref()
            .map(kontor_core::id::parse_utc_timestamp)
            .transpose()?,
        created_at: read_timestamp(&row.get::<_, String>(33).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(34).map_err(backend)?)?,
    })
}

impl SqliteStore {
    /// Read one challenge by its server-generated message identity.
    pub fn turn_correlation_challenge(
        &self,
        project_id: ProjectId,
        message_id: &ExternalId,
    ) -> RepositoryResult<Option<TurnCorrelationChallenge>> {
        self.connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM turn_correlation_challenges WHERE project_id=?1 AND message_id=?2"),
                params![project_id.to_string(), message_id.as_str()],
                |row| Ok(read(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    /// Read the challenge already owned by an idempotency key.
    pub fn turn_correlation_challenge_by_key(
        &self,
        key: &str,
    ) -> RepositoryResult<Option<TurnCorrelationChallenge>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM turn_correlation_challenges WHERE idempotency_key=?1"
                ),
                params![key],
                |row| Ok(read(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    /// Persist one immutable challenge before any native dispatch.
    pub fn prepare_turn_correlation_challenge(
        &self,
        challenge: &NewTurnCorrelationChallenge,
    ) -> RepositoryResult<(TurnCorrelationChallenge, Applied)> {
        if let Some(existing) =
            self.turn_correlation_challenge_by_key(&challenge.idempotency_key)?
        {
            if existing.project_id != challenge.project_id
                || existing.request_hash != challenge.request_hash
            {
                return Err(conflict(
                    "turn correlation challenge",
                    "the idempotency key already names a different challenge",
                ));
            }
            return Ok((existing, Applied::Unchanged));
        }
        self.connection
            .execute(
                 "INSERT INTO turn_correlation_challenges
                 (message_id,project_id,task_id,team_run_id,agent_run_id,role_slot_id,
                  seat_binding_id,runtime_binding_id,runtime_kind,runtime_host,runtime_generation,native_id,
                  task_revision,agent_run_revision,artifact_key,evidence_revision_id,
                  evidence_content_hash,report_checksum,boundary_epoch,boundary_sequence,
                  native_epoch,body,body_hash,expected_response,preview_hash,request_hash,
                  idempotency_key,state,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                         ?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,'prepared',?28,?28)",
                params![
                    challenge.message_id.as_str(),
                    challenge.project_id.to_string(),
                    challenge.task_id.to_string(),
                    challenge.team_run_id.to_string(),
                    challenge.agent_run_id.to_string(),
                    challenge.role_slot_id.as_role_key().as_str(),
                    challenge.seat_binding_id.to_string(),
                    challenge.runtime_binding_id.to_string(),
                    challenge.runtime_kind.as_str(),
                    challenge.runtime_host.as_str(),
                    i64::try_from(challenge.runtime_generation).unwrap_or(i64::MAX),
                    challenge.native_id.as_str(),
                    revision_column(challenge.task_revision)?,
                    revision_column(challenge.agent_run_revision)?,
                    challenge.artifact_key.as_str(),
                    challenge.evidence_revision_id.as_str(),
                    challenge.evidence_content_hash.as_str(),
                    challenge.report_checksum.as_str(),
                    i64::try_from(challenge.boundary_epoch).unwrap_or(i64::MAX),
                    i64::try_from(challenge.boundary_sequence).unwrap_or(i64::MAX),
                    challenge.native_epoch.as_str(),
                    challenge.body.as_str(),
                    challenge.body_hash.as_str(),
                    challenge.expected_response.as_str(),
                    challenge.preview_hash.as_str(),
                    challenge.request_hash.as_str(),
                    challenge.idempotency_key,
                    text(challenge.created_at),
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    conflict(
                        "turn correlation challenge",
                        "this AgentRun already has an unsettled correlation challenge",
                    )
                }
                other => backend(other),
            })?;
        let stored = self
            .turn_correlation_challenge(challenge.project_id, &challenge.message_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "turn correlation challenge",
            })?;
        Ok((stored, Applied::Created))
    }

    /// Claim the sole first native dispatch. A replay can only reconcile.
    pub fn claim_turn_correlation_dispatch(
        &self,
        project_id: ProjectId,
        message_id: &ExternalId,
        claimed_at: Timestamp,
    ) -> RepositoryResult<bool> {
        let changed = self
            .connection
            .execute(
                "UPDATE turn_correlation_challenges SET state='dispatching', updated_at=?3
             WHERE project_id=?1 AND message_id=?2 AND state='prepared'",
                params![
                    project_id.to_string(),
                    message_id.as_str(),
                    text(claimed_at)
                ],
            )
            .map_err(backend)?;
        Ok(changed == 1)
    }

    /// Record the exact canonical challenge position selected by the adapter.
    pub fn acknowledge_turn_correlation_challenge(
        &self,
        project_id: ProjectId,
        message_id: &ExternalId,
        epoch: u64,
        sequence: u64,
        acknowledged_at: Timestamp,
    ) -> RepositoryResult<TurnCorrelationChallenge> {
        let existing = self
            .turn_correlation_challenge(project_id, message_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "turn correlation challenge",
            })?;
        if matches!(
            existing.state,
            TurnCorrelationState::Acknowledged | TurnCorrelationState::Settled
        ) {
            if existing.message_epoch == Some(epoch) && existing.message_sequence == Some(sequence)
            {
                return Ok(existing);
            }
            return Err(conflict(
                "turn correlation challenge",
                "the challenge was already acknowledged at another position",
            ));
        }
        if existing.state != TurnCorrelationState::Dispatching || epoch == 0 || sequence == 0 {
            return Err(conflict(
                "turn correlation challenge",
                "only a claimed challenge can be acknowledged at a positive position",
            ));
        }
        let changed = self
            .connection
            .execute(
                "UPDATE turn_correlation_challenges
             SET state='acknowledged', message_epoch=?3, message_sequence=?4,
                 acknowledged_at=?5, updated_at=?5
             WHERE project_id=?1 AND message_id=?2 AND state='dispatching'",
                params![
                    project_id.to_string(),
                    message_id.as_str(),
                    i64::try_from(epoch).unwrap_or(i64::MAX),
                    i64::try_from(sequence).unwrap_or(i64::MAX),
                    text(acknowledged_at)
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "turn correlation challenge",
                "the challenge moved during acknowledgement",
            ));
        }
        self.turn_correlation_challenge(project_id, message_id)?
            .ok_or(RepositoryError::NotFound {
                subject: "turn correlation challenge",
            })
    }
}
