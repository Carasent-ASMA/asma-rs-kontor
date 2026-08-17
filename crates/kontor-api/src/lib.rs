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

pub mod applications;
pub mod auth;
pub mod control;
pub mod dto;
pub mod error;
pub mod memory;
pub mod openapi;
pub mod sessions;
pub mod state;

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
        .route("/v1/projects/{project_id}/memory", get(memory::list))
        .route(
            "/v1/projects/{project_id}/memory/{item_id}/history",
            get(memory::history),
        )
        .route(
            "/v1/projects/{project_id}/memory/revisions:propose",
            post(memory::propose),
        )
        .route(
            "/v1/projects/{project_id}/memory/revisions/{revision_id}/approval",
            post(memory::approve),
        )
        .route(
            "/v1/projects/{project_id}/memory/{item_id}/tombstone",
            post(memory::tombstone),
        )
        .route(
            "/v1/projects/{project_id}/memory/{item_id}/purge",
            post(memory::purge),
        )
        .route(
            "/v1/projects/{project_id}/memory/import:preview",
            post(memory::import_preview),
        )
        .route(
            "/v1/projects/{project_id}/memory/import:apply",
            post(memory::import_apply),
        )
        .route("/v1/memory/cutover:freeze", post(memory::freeze))
        .route(
            "/v1/projects/{project_id}/memory/cutover:switch",
            post(memory::switch),
        )
        .route("/v1/openapi.json", get(openapi_document))
        .route("/v1/runs/{agent_run_id}", get(control::run_snapshot))
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}",
            get(control::task_snapshot),
        )
        .route("/v1/events", get(control::events))
        // The declarative application operations. Every one of them answers with
        // the durable projection its service produced, not with an intent.
        .route("/v1/projects:ensure", post(applications::ensure_project))
        .route(
            "/v1/catalog/work-profiles",
            get(applications::work_profiles),
        )
        .route(
            "/v1/catalog/team-templates",
            get(applications::team_templates),
        )
        .route("/v1/catalog", get(applications::model_catalog))
        .route("/v1/teams", get(applications::teams))
        .route("/v1/teams/drafts:save", post(applications::save_team_draft))
        .route(
            "/v1/teams/{team_id}/publish",
            post(applications::publish_team),
        )
        .route(
            "/v1/runtime-capabilities",
            get(applications::runtime_capabilities),
        )
        // The topology vocabulary: what kinds may exist, what roles may be
        // selected, and what every controlled code means. Draft and validate are
        // POSTs that persist nothing, so neither takes an idempotency key.
        .route(
            "/v1/projects/{project_id}/topology-specs:draft",
            post(applications::draft_topology_spec),
        )
        .route(
            "/v1/projects/{project_id}/topology-specs:validate",
            post(applications::validate_topology_spec),
        )
        .route(
            "/v1/projects/{project_id}/topology-specs:publish",
            post(applications::publish_topology_spec),
        )
        .route(
            "/v1/projects/{project_id}/topology-specs/{spec_id}/{version}",
            get(applications::topology_spec),
        )
        .route(
            "/v1/catalog/role-catalogs/{catalog_id}/{version}",
            get(applications::role_catalog),
        )
        .route(
            "/v1/catalog/role-catalogs/{catalog_id}/{version}/roles/{role_code}",
            get(applications::role),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}/code-help",
            get(applications::code_help),
        )
        .route(
            "/v1/projects/{project_id}/provider-account-profiles",
            get(applications::account_profiles),
        )
        .route(
            "/v1/projects/{project_id}/provider-account-profiles:ensure",
            post(applications::ensure_account_profile),
        )
        .route(
            "/v1/projects/{project_id}/epics:apply",
            post(applications::apply_epic),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}",
            get(applications::read_epic),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}/execution:arm",
            post(applications::arm),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}/execution:disarm",
            post(applications::disarm),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}/scheduler:plan",
            post(applications::plan),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}/scheduler:start",
            post(applications::start),
        )
        .route(
            "/v1/projects/{project_id}/epics/{epic_id}/lifecycle",
            post(applications::lifecycle),
        )
        // The Lead-required control and evidence operations. Every one of them is
        // task-scoped, because that is the grain at which a profile is pinned, a
        // gate is judged and a ticket is linked.
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/context:resolve",
            post(applications::resolve_context),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/gates/{gate_id}/record",
            post(applications::record_gate),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/profile-selection",
            post(applications::select_profile),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/team-selection",
            post(applications::select_team),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/account-selection",
            post(applications::select_account),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-plan",
            post(applications::ticket_reconcile_plan),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-apply",
            post(applications::ticket_reconcile_apply),
        )
        // Settling a run is addressed by the run, not by the task: a run is what
        // a runtime holds a session for, and it is the thing being asked about.
        .route(
            "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:settle",
            post(applications::settle_runtime),
        )
        .route(
            "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:abandon",
            post(applications::abandon_run),
        )
        // A *turn* is smaller than a run: settling one closes Kontor's bounded
        // piece of work and leaves the seat's native session live.
        .route(
            "/v1/projects/{project_id}/agent-runs/{agent_run_id}/turns:settle",
            post(applications::settle_turn),
        )
        .route(
            "/v1/projects/{project_id}/agent-runs/{agent_run_id}/handoffs:attest-late",
            post(applications::attest_late_handoff),
        )
        .route(
            "/v1/projects/{project_id}/agent-runs/{agent_run_id}/successors:replace",
            post(applications::replace_seat),
        )
        // A declared slot that never got a seat is accounted for by an explicit,
        // authorized waiver — and by nothing else.
        .route(
            "/v1/projects/{project_id}/team-runs/{team_run_id}/role-slots/{role_slot_id}/waivers",
            post(applications::waive_role_slot),
        )
        // Profile detail and validation. Workspace-level, like the catalog they
        // extend: a category resolves to the same bundle in every Realm running
        // this build, so there is no project in the address.
        .route("/v1/catalog/packs", get(applications::profile_packs))
        .route(
            "/v1/catalog/packs:register",
            post(applications::register_pack),
        )
        .route(
            "/v1/catalog/work-profiles/{category}",
            get(applications::work_profile),
        )
        .route(
            "/v1/catalog/work-profiles/{category}/validate",
            post(applications::validate_work_profile),
        )
        // Triggers and intake.
        .route(
            "/v1/projects/{project_id}/triggers/{trigger}/{version}",
            get(applications::trigger),
        )
        .route(
            "/v1/projects/{project_id}/intake:submit",
            post(applications::submit_intake),
        )
        .route(
            "/v1/projects/{project_id}/intake/{receipt_id}",
            get(applications::intake_receipt),
        )
        // Connector specifications, addressed by the connector they map.
        .route(
            "/v1/projects/{project_id}/connectors/{connector}/field-specs",
            get(applications::connector_field_specs),
        )
        .route(
            "/v1/projects/{project_id}/connectors/{connector}/workflow-specs",
            get(applications::connector_workflow_specs),
        )
        // Conflicts, inbound comments and ownership, all task-scoped: a ticket is
        // linked to a task, and every one of these is a fact about that link.
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:conflicts",
            get(applications::ticket_conflicts),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:resolve-conflict",
            post(applications::resolve_ticket_conflict),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:pull-comments",
            post(applications::pull_ticket_comments),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:comments",
            get(applications::ticket_comments),
        )
        .route(
            "/v1/projects/{project_id}/tasks/{task_id}/ticket:claim",
            post(applications::claim_ticket),
        )
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
        .route(
            "/v1/sessions/{agent_run_id}/compact",
            post(sessions::compact),
        )
        .route(
            "/v1/context-policy/preview",
            post(control::context_policy_preview),
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
