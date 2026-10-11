//! PR reviews on a PR desk over `/v1` (ASMA-8453).
//!
//! A Team Definition may declare where a desk reviews pull requests: one Adam
//! tab per pull request in the desk's workspace, and one review room per pull
//! request below the desk. Opening a review places both for exactly one
//! repository and number under one confirmed Jira key, and with routes
//! launches the Adam and every room seat. Closing it releases that review's
//! seats and nothing else: the desk, its durable seats and every other pull
//! request's review keep running.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kontor_core::id::{AggregateRevision, DeskKey, ExternalId};
use kontor_core::pull_request::ForgeRepository;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Caller;
use crate::applications::{MutationReceiptDto, RuntimeModelRouteRequest, TeamDefinitionRefDto};
use crate::auth::CallerCapability;
use crate::body::Json;
use crate::control::{idempotency_key, parse_id};
use crate::error::ApiError;
use crate::state::ApiState;

/// One pull request: its forge repository and its number. Both are its
/// identity; nothing about it is read from a title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PullRequestRequest {
    /// The forge repository, as exact `owner/name`.
    #[schema(value_type = String)]
    pub repository: ForgeRepository,
    /// The pull request's number in that repository; positive.
    pub number: u64,
}

/// Open one pull request's review on a PR desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenPrReviewRequest {
    /// The project revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The exact confirmed external tracker key the review is opened under,
    /// e.g. `ASMA-8449`: an epic's, or one of its tasks'. It must authorize
    /// the pull request's repository.
    #[schema(value_type = String)]
    pub external_key: ExternalId,
    /// The pull request.
    pub pull_request: PullRequestRequest,
    /// The exact route each seat launches on, keyed by slot id: the Adam's
    /// slot and every room slot, each exactly once. Absent, the review is
    /// placed and seated with nothing launched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_routes: Option<BTreeMap<String, RuntimeModelRouteRequest>>,
}

/// Close one pull request's review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClosePrReviewRequest {
    /// The project revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The pull request whose review closes.
    pub pull_request: PullRequestRequest,
}

/// One seat of a review, as Kontor records it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PrReviewSeatDto {
    /// The seat's slot in its container.
    pub slot_id: String,
    /// The catalog role it holds.
    pub role_code: String,
    /// Its exact title.
    pub title: String,
    /// The SeatBinding it is correlated by.
    pub seat_binding_id: String,
    /// The native Kontor records as its current occupant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    /// The current occupancy generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occupancy_generation: Option<u64>,
}

/// The native container a review node's sessions run in, as last read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PrReviewContainerDto {
    /// The node that owns the container: the desk workspace for the Adam, the
    /// room itself for the room.
    pub topology_node_id: String,
    /// The native container id.
    pub native_id: String,
    /// What the runtime said it is: `project` or `workspace`.
    pub observed_kind: String,
    /// Its exact runtime-visible title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Its canonical working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Its native parent's id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_native_id: Option<String>,
}

/// One node of a review: the Adam or the room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PrReviewNodeDto {
    /// The node.
    pub topology_node_id: String,
    /// Its kind, from the definition that placed the review.
    pub kind: String,
    /// Its parent: the desk workspace for the Adam, the desk for the room.
    pub parent_id: String,
    /// Where its sessions run, once materialized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<PrReviewContainerDto>,
    /// Its current seats; empty once the review is closed.
    pub seats: Vec<PrReviewSeatDto>,
}

/// One pull request's review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PrReviewDto {
    /// The desk it is reviewed on.
    pub desk_key: String,
    /// The forge repository, as exact `owner/name`.
    pub repository: String,
    /// The pull request's number in that repository.
    pub number: u64,
    /// The confirmed external tracker key it was opened under.
    pub external_key: String,
    /// The epic that key belongs to.
    pub epic_id: String,
    /// The task, when the key is a task's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// The Team Definition revision that placed it.
    pub team_definition: TeamDefinitionRefDto,
    /// Whether any of its seats is current.
    pub open: bool,
    /// The Adam tab, in the desk's workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adam: Option<PrReviewNodeDto>,
    /// The review room, below the desk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<PrReviewNodeDto>,
    /// When the review was first opened.
    pub created_at: String,
}

/// Every review one desk holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PrReviewReportDto {
    /// The desk.
    pub desk_key: String,
    /// Each pull request's review, oldest first.
    pub reviews: Vec<PrReviewDto>,
}

/// The answer to an open or a close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PrReviewOutcomeDto {
    /// The review, read after any effect this call had.
    pub review: PrReviewDto,
    /// The receipt the command was recorded under.
    pub receipt: MutationReceiptDto,
}

fn parse_desk(state: &ApiState, desk_key: &str) -> Result<DeskKey, ApiError> {
    parse_id(state, DeskKey::parse(desk_key))
}

/// Report every review one desk holds.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/desks/{desk_key}/pr-reviews", tag = "applications",
    params(("project_id" = String, Path), ("desk_key" = String, Path)),
    responses((status = 200, body = PrReviewReportDto), (status = 403), (status = 404))
)]
pub async fn list_pr_reviews(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, desk_key)): Path<(String, String)>,
) -> Result<Json<PrReviewReportDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, kontor_core::id::ProjectId::parse(&project_id))?;
    let desk_key = parse_desk(&state, &desk_key)?;
    Ok(Json(
        state.applications().pr_reviews(project_id, &desk_key)?,
    ))
}

/// Open one pull request's review.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/desks/{desk_key}/pr-reviews:open", tag = "applications",
    params(("project_id" = String, Path), ("desk_key" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = OpenPrReviewRequest,
    responses((status = 200, body = PrReviewOutcomeDto), (status = 400), (status = 403), (status = 404), (status = 409))
)]
pub async fn open_pr_review(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, desk_key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<OpenPrReviewRequest>,
) -> Result<Json<PrReviewOutcomeDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, kontor_core::id::ProjectId::parse(&project_id))?;
    let desk_key = parse_desk(&state, &desk_key)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .open_pr_review(&key, project_id, &desk_key, &request)
            .await?,
    ))
}

/// Close one pull request's review.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/desks/{desk_key}/pr-reviews:close", tag = "applications",
    params(("project_id" = String, Path), ("desk_key" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = ClosePrReviewRequest,
    responses((status = 200, body = PrReviewOutcomeDto), (status = 400), (status = 403), (status = 404), (status = 409))
)]
pub async fn close_pr_review(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, desk_key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ClosePrReviewRequest>,
) -> Result<Json<PrReviewOutcomeDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, kontor_core::id::ProjectId::parse(&project_id))?;
    let desk_key = parse_desk(&state, &desk_key)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .close_pr_review(&key, project_id, &desk_key, &request)
            .await?,
    ))
}
