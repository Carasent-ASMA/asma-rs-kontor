//! Public, seat-authored operations over the existing open-question ledger.
#![allow(missing_docs)]

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kontor_core::id::{BoundedText, OpenQuestionId, SeatBindingId, TriggerKey};
use kontor_core::open_question::{DispositionOutcome, OpenQuestionAttachment, QuestionScope};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Caller;
use crate::auth::CallerCapability;
use crate::body::Json;
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionAction {
    Raise {
        #[schema(value_type = String)]
        subject: BoundedText,
        #[schema(value_type = String)]
        scope: QuestionScope,
        #[schema(value_type = Object)]
        attachment: OpenQuestionAttachment,
        #[schema(value_type = String)]
        why_ambiguous: BoundedText,
        #[schema(value_type = Vec<String>)]
        options: Vec<BoundedText>,
    },
    Correct {
        #[schema(value_type = String)]
        why_ambiguous: BoundedText,
        #[schema(value_type = Vec<String>)]
        options: Vec<BoundedText>,
        supersedes: Option<u32>,
    },
    Dispose {
        #[schema(value_type = Object)]
        outcome: DispositionOutcome,
        supersedes: Option<u32>,
    },
    FireTrigger {
        #[schema(value_type = String)]
        trigger: TriggerKey,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordQuestionRequest {
    #[schema(value_type = String)]
    pub question_id: OpenQuestionId,
    /// Zero when raising; otherwise the exact question revision just read.
    pub expected_revision: u64,
    /// Ordinary Operator reporting names the active seat on whose behalf it
    /// reports. Closing always requires that leadership seat's scoped bearer.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub author_seat_binding_id: Option<SeatBindingId>,
    pub action: QuestionAction,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct QuestionActor {
    pub seat_binding_id: SeatBindingId,
    pub occupancy_generation: Option<u64>,
}

#[utoipa::path(
    get, path = "/v1/projects/{project_id}/epics/{epic_id}/open-questions", tag = "applications",
    params(("project_id" = String, Path), ("epic_id" = String, Path)),
    responses((status = 200, body = Object), (status = 403), (status = 404))
)]
pub async fn list_open_questions(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, epic)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = caller.seat().map(|seat_binding_id| QuestionActor {
        seat_binding_id,
        occupancy_generation: caller.occupancy_generation(),
    });
    if actor.is_none() {
        caller.require(&state, CallerCapability::Observer)?;
    }
    let project = crate::control::parse_id(&state, kontor_core::id::ProjectId::parse(&project))?;
    let epic = crate::applications::resolve_epic_selector(&state, project, &epic)?;
    Ok(Json(
        state.applications().open_questions(project, epic, actor)?,
    ))
}

#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/open-questions:record", tag = "applications",
    params(("project_id" = String, Path), ("epic_id" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = RecordQuestionRequest,
    responses((status = 200, body = Object), (status = 400), (status = 403), (status = 409))
)]
pub async fn record_open_question(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, epic)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordQuestionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = if let Some(seat_binding_id) = caller.seat() {
        if request
            .author_seat_binding_id
            .is_some_and(|id| id != seat_binding_id)
        {
            return Err(state.refuse(
                ApiErrorCode::Forbidden,
                "a scoped seat cannot name another author",
            ));
        }
        QuestionActor {
            seat_binding_id,
            occupancy_generation: caller.occupancy_generation(),
        }
    } else {
        caller.require(&state, CallerCapability::Operator)?;
        if matches!(request.action, QuestionAction::Dispose { .. }) {
            return Err(state.refuse(
                ApiErrorCode::Forbidden,
                "closing a question requires its configured leadership seat's scoped credential",
            ));
        }
        QuestionActor {
            seat_binding_id: request.author_seat_binding_id.ok_or_else(|| {
                state.refuse(
                    ApiErrorCode::InvalidRequest,
                    "operator reporting must name the active author seat",
                )
            })?,
            occupancy_generation: None,
        }
    };
    let project = crate::control::parse_id(&state, kontor_core::id::ProjectId::parse(&project))?;
    let epic = crate::applications::resolve_epic_selector(&state, project, &epic)?;
    let key = crate::control::idempotency_key(&state, &headers)?;
    Ok(Json(state.applications().record_open_question(
        &key, project, epic, actor, &request,
    )?))
}
