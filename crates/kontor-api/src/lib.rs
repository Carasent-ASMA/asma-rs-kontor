//! `kontor-api` — Versioned loopback HTTP JSON and SSE contract
//!
//! This crate is the whole HTTP surface of a Kontor Realm, and deliberately
//! nothing else. It does not open a database, take a filesystem lock, mint a
//! credential, read a runtime endpoint or bind a socket: it is handed an
//! [`state::ApiState`] the composition root assembled, and it turns requests into
//! calls on the repositories and runtime adapters inside it. `kontor-daemon` is
//! where those decisions live, which is what keeps them in one place.
//!
//! # Two cursor spaces, never mixed
//!
//! | Space | Owner | Where it appears |
//! | --- | --- | --- |
//! | control-plane cursor | this Realm's SQLite log | `/v1/events`, every `snapshot_cursor` |
//! | session content position | the runtime | `/v1/sessions/…/timeline`, `/v1/sessions/…/stream` |
//!
//! A control-plane position is an integer allocated by a writing transaction. A
//! content position is an `epoch:sequence` pair the runtime owns. They are
//! spelled differently, carried on different routes, and neither is accepted
//! where the other belongs — because a hole in one is a paging question and a hole
//! in the other is a refetch obligation, and treating either as the other is how a
//! control plane starts inventing certainty it does not have.
//!
//! # What every response carries
//!
//! Every successful body and every refusal names the Realm it came from. Receipts,
//! events and snapshots use `kontor-core`'s own envelopes, so a value that leaves
//! this process is always read as `(realm_id, …)` rather than as a bare id that
//! means something else in another Realm.
//!
//! # What no response carries
//!
//! There is no field anywhere in [`dto`] or [`error`] for a bearer token, a
//! runtime endpoint, a credential value, a config home or an adapter's client
//! configuration. Those exist only in daemon state, and a DTO cannot leak what it
//! has nowhere to put.

pub mod auth;
pub mod control;
pub mod dto;
pub mod error;
pub mod openapi;
pub mod query;
pub mod sessions;
pub mod state;
pub mod wired;

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router, extract::Request};
use kontor_core::id::Timestamp;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{CallerCapability, IngressRefusal};
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;

/// The instant a handler acts at.
///
/// One function, so every freshness judgement, receipt timestamp and message
/// instant in this crate reads the same clock — and a future test clock replaces
/// all of them at once rather than most of them.
#[must_use]
pub fn now() -> Timestamp {
    Timestamp::now()
}

/// The authority the authenticated caller carries.
///
/// It is produced by [`authenticate`] and put in the request extensions, so a
/// handler cannot be reached without one. Extracting it is infallible for that
/// reason; the fallible part already happened, before any handler ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller(pub CallerCapability);

impl Caller {
    /// Refuse a caller whose tier does not reach `required`.
    ///
    /// Every handler calls this on its first line. That is the whole authorization
    /// model: the requirement is stated where the work happens, in one call that
    /// is impossible to satisfy accidentally.
    ///
    /// # Errors
    /// Returns [`ApiErrorCode::Forbidden`] when the caller's tier is lower than
    /// `required`.
    pub fn require(self, state: &ApiState, required: CallerCapability) -> Result<(), ApiError> {
        if self.0.at_least(required) {
            return Ok(());
        }
        Err(state.refuse(
            ApiErrorCode::Forbidden,
            "this route requires a higher realm authority than the presented credential",
        ))
    }
}

impl FromRequestParts<ApiState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> Result<Self, Self::Rejection> {
        // The extension is only ever put here by [`authenticate`]. Its absence
        // means the request did not pass authentication — a router assembled
        // without the layer — and the answer to that is a refusal. Defaulting to
        // the lowest tier would look careful and would in fact serve every read
        // route to an unauthenticated caller.
        parts.extensions.get::<Self>().copied().ok_or_else(|| {
            state.refuse(
                ApiErrorCode::Unauthenticated,
                "the request did not pass realm authentication",
            )
        })
    }
}

/// Refuse a request before any handler sees it.
///
/// The order is where, then who, then what. Each check refuses on its own, and
/// none of them has looked at a route, a body or a path parameter yet — so a
/// request from a disallowed origin cannot reach a handler by naming a route that
/// forgot to check.
async fn authenticate(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let (mut parts, body) = request.into_parts();
    state
        .ingress()
        .admit(&parts.headers)
        .map_err(|refusal| match refusal {
            IngressRefusal::Host => state.refuse(
                ApiErrorCode::Forbidden,
                "this realm answers only to a loopback host",
            ),
            IngressRefusal::Origin => state.refuse(
                ApiErrorCode::Forbidden,
                "this realm does not answer to the presented origin",
            ),
            IngressRefusal::Credential => state.refuse(
                ApiErrorCode::Unauthenticated,
                "no realm credential was presented",
            ),
        })?;
    let presented = crate::auth::bearer(&parts.headers).ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unauthenticated,
            "this realm requires an Authorization: Bearer credential",
        )
    })?;
    let authority = state.credentials().authority(presented).ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unauthenticated,
            "the presented credential is not one of this realm's",
        )
    })?;
    parts.extensions.insert(Caller(authority));
    Ok(next.run(Request::from_parts(parts, body)).await)
}

/// The versioned HTTP and SSE surface of one Realm.
///
/// Every route is `/v1/…`, every route is authenticated, and the CORS policy is
/// exactly the configured origins — never a wildcard, and never with credentials
/// allowed, because the credential this API takes is a bearer header the browser
/// must be given deliberately rather than a cookie it would attach on its own.
pub fn router(state: ApiState) -> Router {
    let origins: Vec<_> = state
        .ingress()
        .allowed_origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            crate::control::IDEMPOTENCY_KEY,
            axum::http::header::HeaderName::from_static("last-event-id"),
        ]);

    Router::new()
        .route("/v1/health", get(control::health))
        .route("/v1/realm", get(control::realm))
        .route("/v1/openapi.json", get(openapi_document))
        .route("/v1/runs/{agent_run_id}", get(control::run_snapshot))
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}",
            get(control::task_snapshot),
        )
        .route("/v1/commands/{kind}", post(control::command))
        .route("/v1/events", get(control::events))
        // The KON-MVP-16 contract amendment. Every one of these is a thin read
        // over a repository method that already existed; see `query`'s own docs
        // for what is deliberately absent and which ticket owns it.
        // The KON-MVP-16 second amendment: the surfaces whose owning seams are
        // merged. See `wired` for what each one reads and what it refuses.
        .route("/v1/projects", get(wired::projects))
        .route("/v1/projects/{project_id}/team-runs", get(wired::missions))
        .route("/v1/projects/{project_id}/runs", get(wired::runs))
        .route(
            "/v1/projects/{project_id}/scheduler/plan",
            get(wired::scheduler_plan),
        )
        .route("/v1/projects/{project_id}/tickets", get(wired::tickets))
        .route(
            "/v1/projects/{project_id}/tickets/{link_id}",
            get(wired::ticket),
        )
        .route(
            "/v1/projects/{project_id}/tickets/{link_id}/comments",
            get(wired::ticket_comments),
        )
        .route(
            "/v1/projects/{project_id}/tickets/{link_id}/transitions",
            get(wired::ticket_transitions),
        )
        .route(
            "/v1/runtimes/{runtime_kind}/sessions",
            get(wired::runtime_sessions),
        )
        .route("/v1/projects/{project_id}", get(query::project))
        .route("/v1/projects/{project_id}/tasks", get(query::tasks))
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/gates",
            get(query::gates),
        )
        .route(
            "/v1/projects/{project_id}/team-runs/{team_run_id}",
            get(query::mission),
        )
        .route(
            "/v1/projects/{project_id}/profiles/{profile_key}/{version}",
            get(query::profile),
        )
        .route(
            "/v1/projects/{project_id}/receipts/{receipt_id}",
            get(query::receipt),
        )
        .route("/v1/projects/{project_id}/accounts", get(query::accounts))
        .route(
            "/v1/projects/{project_id}/accounts/{account_profile_id}",
            get(query::account),
        )
        .route("/v1/runtimes", get(query::runtimes))
        .route("/v1/scheduler/contention", get(query::scheduler_contention))
        .route(
            "/v1/sessions/{agent_run_id}/timeline",
            get(sessions::timeline),
        )
        .route("/v1/sessions/{agent_run_id}/stream", get(sessions::stream))
        .route(
            "/v1/sessions/{agent_run_id}/messages",
            post(sessions::send_message),
        )
        .route(
            "/v1/sessions/{agent_run_id}/permissions/{request_id}",
            post(sessions::respond_permission),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// The generated contract document.
///
/// It is behind the same authentication as every other route: the document names
/// every route and every shape, and a Realm does not owe that to an unauthenticated
/// caller.
async fn openapi_document(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<utoipa::openapi::OpenApi>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(openapi::document()))
}
