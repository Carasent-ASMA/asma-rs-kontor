//! Durable desk seats over `/v1` (ASMA-8451).
//!
//! A durable desk (ASMA-8450) may hold long-lived seats. Each one is declared
//! once, as a dated decision that says who may restart it. Reading the desk's
//! seats reports what the runtime holds for each; reconciling restarts only
//! the seats an operator selected that are daemon-owned and proved missing.
//! Nothing here adopts, restarts or even asks the runtime about a seat that
//! Paseo-direct operation recovers.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kontor_core::durable_seat::{
    DurableSeatDecision, DurableSeatReason, DurableSeatRecoveryOwner, DurableSeatStatus,
};
use kontor_core::id::{AggregateRevision, DeskKey, DurableSeatKey, ExternalName, RoleCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Caller;
use crate::applications::{MutationReceiptDto, RuntimeModelRouteRequest};
use crate::auth::CallerCapability;
use crate::body::Json;
use crate::control::{idempotency_key, parse_id};
use crate::error::ApiError;
use crate::state::ApiState;

/// Declare one long-lived seat a desk holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclareDurableSeatRequest {
    /// The project revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The seat's stable key inside its desk.
    #[schema(value_type = String)]
    pub seat_key: DurableSeatKey,
    /// The catalog role the seat holds; the first segment of its title.
    #[schema(value_type = String)]
    pub role_code: RoleCode,
    /// The seat's label; the rest of its title.
    #[schema(value_type = String)]
    pub seat_label: ExternalName,
    /// The exact route a daemon-owned seat is launched on. Required for one,
    /// optional evidence for a seat recovered outside Kontor. Named chains
    /// are not accepted: a declaration freezes one route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_route: Option<RuntimeModelRouteRequest>,
    /// The repository-relative procedure the seat follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub procedure: Option<ExternalName>,
    /// Who may restart the seat. `daemon` lets Kontor restart it when it is
    /// proved missing; the other owner means the seat is run and recovered
    /// outside Kontor, which never touches it.
    #[schema(value_type = String)]
    pub recovery_owner: DurableSeatRecoveryOwner,
    /// Who made the decision.
    #[schema(value_type = String)]
    pub decided_by: ExternalName,
    /// The day the decision was made, `YYYY-MM-DD`.
    #[schema(value_type = String)]
    pub decided_on: jiff::civil::Date,
}

/// Reconcile selected seats of one desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReconcileDurableSeatsRequest {
    /// The project revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The declared seats to reconcile. At least one; nothing unselected is
    /// touched.
    #[schema(value_type = Vec<String>)]
    pub seat_keys: Vec<DurableSeatKey>,
    /// Whether a selected, daemon-owned, proved-missing seat is restarted.
    /// `false` reports and records the reconciliation without any effect.
    pub restart_missing: bool,
}

/// One declared seat, what reconciliation observed about it, and what it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DurableSeatDto {
    /// The seat's key inside its desk.
    pub seat_key: String,
    /// Its exact title.
    pub title: String,
    /// Its catalog role.
    pub role_code: String,
    /// Its label.
    pub seat_label: String,
    /// Who may restart it.
    #[schema(value_type = String)]
    pub recovery_owner: DurableSeatRecoveryOwner,
    /// The exact route it is launched on, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_route: Option<RuntimeModelRouteRequest>,
    /// The procedure it follows, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedure: Option<String>,
    /// Who decided it.
    pub decided_by: String,
    /// When they decided it.
    pub decided_on: String,
    /// When Kontor recorded the decision.
    pub declared_at: String,
    /// The SeatBinding a daemon-owned seat is correlated by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat_binding_id: Option<String>,
    /// The native Kontor records as the current occupant, read after any
    /// effect this call had.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    /// The current occupancy generation, read after any effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occupancy_generation: Option<u64>,
    /// What was observed, before any effect. Absent from a declaration's
    /// answer, which observes nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub status: Option<DurableSeatStatus>,
    /// Why.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub reason: Option<DurableSeatReason>,
    /// What reconciliation decided: `none`, `launch` or `recover`. Always
    /// `none` on a read and on a replayed reconciliation.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub decision: Option<DurableSeatDecision>,
}

/// One desk's declared seats, as observed now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DurableSeatReportDto {
    /// The desk.
    pub desk_key: String,
    /// Every declared seat, by key.
    pub seats: Vec<DurableSeatDto>,
}

/// The answer to a declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DurableSeatDeclarationOutcomeDto {
    /// The declared seat, exactly as stored.
    pub seat: DurableSeatDto,
    /// The receipt the declaration was recorded under.
    pub receipt: MutationReceiptDto,
}

/// The answer to a reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DurableSeatReconcileOutcomeDto {
    /// The desk.
    pub desk_key: String,
    /// Each selected seat, by key.
    pub seats: Vec<DurableSeatDto>,
    /// The receipt the reconciliation was recorded under.
    pub receipt: MutationReceiptDto,
}

fn parse_desk(state: &ApiState, desk_key: &str) -> Result<DeskKey, ApiError> {
    parse_id(state, DeskKey::parse(desk_key))
}

/// Report one desk's declared seats.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/desks/{desk_key}/seats", tag = "applications",
    params(("project_id" = String, Path), ("desk_key" = String, Path)),
    responses((status = 200, body = DurableSeatReportDto), (status = 403), (status = 404))
)]
pub async fn list_durable_seats(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, desk_key)): Path<(String, String)>,
) -> Result<Json<DurableSeatReportDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, kontor_core::id::ProjectId::parse(&project_id))?;
    let desk_key = parse_desk(&state, &desk_key)?;
    Ok(Json(
        state
            .applications()
            .durable_seats(project_id, &desk_key)
            .await?,
    ))
}

/// Declare one durable seat.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/desks/{desk_key}/seats:declare", tag = "applications",
    params(("project_id" = String, Path), ("desk_key" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = DeclareDurableSeatRequest,
    responses((status = 200, body = DurableSeatDeclarationOutcomeDto), (status = 400), (status = 403), (status = 409))
)]
pub async fn declare_durable_seat(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, desk_key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<DeclareDurableSeatRequest>,
) -> Result<Json<DurableSeatDeclarationOutcomeDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, kontor_core::id::ProjectId::parse(&project_id))?;
    let desk_key = parse_desk(&state, &desk_key)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(state.applications().declare_durable_seat(
        &key, project_id, &desk_key, &request,
    )?))
}

/// Reconcile selected durable seats.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/desks/{desk_key}/seats:reconcile", tag = "applications",
    params(("project_id" = String, Path), ("desk_key" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = ReconcileDurableSeatsRequest,
    responses((status = 200, body = DurableSeatReconcileOutcomeDto), (status = 400), (status = 403), (status = 409))
)]
pub async fn reconcile_durable_seats(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, desk_key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ReconcileDurableSeatsRequest>,
) -> Result<Json<DurableSeatReconcileOutcomeDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, kontor_core::id::ProjectId::parse(&project_id))?;
    let desk_key = parse_desk(&state, &desk_key)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .reconcile_durable_seats(&key, project_id, &desk_key, &request)
            .await?,
    ))
}
