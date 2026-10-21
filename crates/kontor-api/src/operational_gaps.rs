//! The typed operational-gap write surface (ASMA-8514 / KR-09).
//!
//! A seat records a missing Kontor capability here before it takes a bounded
//! fallback. The route is task-scoped because the transition rule attaches the
//! evidence to the work that was blocked, and it requires Operator authority
//! because observing a gap and deciding to record one are different powers.
//!
//! The record is append-only and idempotent: the caller names the gap once and
//! the idempotency key once, and a retry returns the original record rather
//! than a second observation of one event.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kontor_core::id::{
    AgentRunId, BoundedText, ContentHash, GapFailureClass, MiniProjectId, OperationalGapId,
    ProjectId, SeatBindingId, TaskId, Timestamp,
};
use kontor_core::operational_gap::{OperationalGap, OperationalGapStatus};
use kontor_core::repository::ProjectRepository;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Caller;
use crate::auth::CallerCapability;
use crate::body::Json;
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;

/// One typed gap record a seat asks Kontor to keep.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordOperationalGapRequest {
    /// The stable gap identity the caller reuses on every retry.
    #[schema(value_type = String)]
    pub gap_id: OperationalGapId,
    /// The operating class of the failure, as an open key such as
    /// `capability_missing`.
    #[schema(value_type = String)]
    pub failure_class: GapFailureClass,
    /// The Kontor operation the seat intended to perform.
    #[schema(value_type = String)]
    pub intended_operation: BoundedText,
    /// The checkpoint the seat resumed from after containing the gap.
    #[schema(value_type = String)]
    pub checkpoint: BoundedText,
    /// The bounded fallback action actually taken.
    #[schema(value_type = String)]
    pub fallback_action: BoundedText,
    /// Who owns the repair.
    #[schema(value_type = String)]
    pub owner: BoundedText,
    /// The exact agent run the gap was observed around, when there was one.
    #[schema(value_type = Option<String>)]
    pub agent_run_id: Option<AgentRunId>,
    /// The exact persistent seat the gap was observed around, when there was
    /// one.
    #[schema(value_type = Option<String>)]
    pub seat_binding_id: Option<SeatBindingId>,
    /// Digest of the evidence document this record cites, when one exists.
    #[schema(value_type = Option<String>)]
    pub evidence: Option<ContentHash>,
}

/// One durable operational-gap record, as it was written.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OperationalGapDto {
    /// The Realm the record belongs to.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The gap identity.
    #[schema(value_type = String)]
    pub gap_id: OperationalGapId,
    /// The owning project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The epic the owning task belonged to.
    #[schema(value_type = Option<String>)]
    pub epic_id: Option<MiniProjectId>,
    /// The owning task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The exact run the gap was observed around.
    #[schema(value_type = Option<String>)]
    pub agent_run_id: Option<AgentRunId>,
    /// The exact seat the gap was observed around.
    #[schema(value_type = Option<String>)]
    pub seat_binding_id: Option<SeatBindingId>,
    /// The operating class of the failure.
    #[schema(value_type = String)]
    pub failure_class: GapFailureClass,
    /// The intended Kontor operation.
    #[schema(value_type = String)]
    pub intended_operation: BoundedText,
    /// The checkpoint the seat resumed from.
    #[schema(value_type = String)]
    pub checkpoint: BoundedText,
    /// The bounded fallback action.
    #[schema(value_type = String)]
    pub fallback_action: BoundedText,
    /// The repair owner.
    #[schema(value_type = String)]
    pub owner: BoundedText,
    /// The cited evidence digest, when one was recorded.
    #[schema(value_type = Option<String>)]
    pub evidence: Option<ContentHash>,
    /// The state the record was written in.
    #[schema(value_type = String)]
    pub status: OperationalGapStatus,
    /// When it was written.
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: Timestamp,
    /// Whether this call returned a record an earlier call under the same key
    /// already wrote.
    pub replayed: bool,
}

impl OperationalGapDto {
    fn of(realm_id: kontor_core::id::RealmId, gap: &OperationalGap, replayed: bool) -> Self {
        Self {
            realm_id,
            gap_id: gap.gap_id,
            project_id: gap.project_id,
            epic_id: gap.mini_project_id,
            task_id: gap.task_id,
            agent_run_id: gap.agent_run_id,
            seat_binding_id: gap.seat_binding_id,
            failure_class: gap.failure_class.clone(),
            intended_operation: gap.intended_operation.clone(),
            checkpoint: gap.checkpoint.clone(),
            fallback_action: gap.fallback_action.clone(),
            owner: gap.owner.clone(),
            evidence: gap.evidence.clone(),
            status: gap.status,
            recorded_at: gap.recorded_at,
            replayed,
        }
    }
}

/// Record one typed operational gap against a task.
///
/// Operator work, and idempotent under the caller's key.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/operational-gaps:record",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task, by UUID or confirmed Jira key"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = RecordOperationalGapRequest,
    responses(
        (status = 200, body = OperationalGapDto),
        (status = 400), (status = 401), (status = 403), (status = 404), (status = 409)
    )
)]
pub async fn record_operational_gap(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordOperationalGapRequest>,
) -> Result<Json<OperationalGapDto>, ApiError> {
    // Operator is the floor, and a seat-scoped bearer is refused by the shared
    // guard. An observer can read the gap's effects elsewhere; recording one is
    // authority to state what Kontor failed to do.
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = crate::control::parse_id(&state, ProjectId::parse(&project_id))?;
    let task_id = crate::applications::resolve_task_selector(&state, project_id, &task_id)?;
    let task = state
        .with_store(|store| store.get_task(project_id, task_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?
        .ok_or_else(|| {
            state.refuse(
                ApiErrorCode::NotFound,
                "no such task exists in this project",
            )
        })?;
    let key = crate::control::idempotency_key(&state, &headers)?;
    let gap = OperationalGap {
        gap_id: request.gap_id,
        project_id,
        mini_project_id: task.mini_project_id,
        task_id,
        agent_run_id: request.agent_run_id,
        seat_binding_id: request.seat_binding_id,
        failure_class: request.failure_class,
        intended_operation: request.intended_operation,
        checkpoint: request.checkpoint,
        fallback_action: request.fallback_action,
        owner: request.owner,
        evidence: request.evidence,
        status: OperationalGapStatus::Open,
        recorded_at: crate::now(),
    };
    let (stored, replayed) = state
        .with_store(|store| store.record_operational_gap(&key, &gap))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    Ok(Json(OperationalGapDto::of(
        state.realm_id(),
        &stored,
        replayed,
    )))
}
