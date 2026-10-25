//! Authenticated observer reads of the host-local work catalog.
//!
//! The namespace is `/v1/work-catalog`, deliberately not `/v1/catalog`: the
//! latter already names the model/work-profile catalog this Realm serves from
//! governed data. Two different facts must not share one path.
//!
//! Both handlers are observer reads. They never write, never open a database
//! themselves, and never reach a runtime, Jira or Paseo. The catalog handle is
//! composed by the daemon; absence of that handle is reported as unavailable
//! rather than as an empty catalog, because "nothing configured" and "nothing
//! composed" are different facts.

use axum::extract::{Query, State};
use serde::Deserialize;
use tracing::warn;
use utoipa::IntoParams;

use crate::body::Json;
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;
use crate::{Caller, CallerCapability};

/// Filters accepted by the entries read.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct EntriesQuery {
    /// `plan` or `execution_unit`.
    pub kind: Option<String>,
    /// A repository id, worktree root, path prefix, logical plan id or workstream key.
    pub scope: Option<String>,
    /// Include tombstoned entries. Defaults to false.
    #[serde(default)]
    pub include_removed: bool,
    /// A bounded row limit. TASK-004 owns cursors; this is a hard bound only.
    pub limit: Option<usize>,
}

/// The catalog's freshness, coverage, ownership and conflicts.
#[utoipa::path(
    get, path = "/v1/work-catalog/status", tag = "work-catalog",
    responses(
        (status = 200, body = kontor_catalog::contract::CatalogStatus),
        (status = 401), (status = 403), (status = 503)
    )
)]
pub async fn status(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<kontor_catalog::contract::CatalogStatus>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog = state.catalog().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unavailable,
            "the work catalog is not composed in this realm",
        )
    })?;
    catalog.status().map(Json).map_err(|error| {
        warn!(detail = %error, "work catalog status read failed");
        state.refuse(ApiErrorCode::Unavailable, "the work catalog is unavailable")
    })
}

/// One generation-consistent entries answer.
#[utoipa::path(
    get, path = "/v1/work-catalog/entries", tag = "work-catalog",
    params(EntriesQuery),
    responses(
        (status = 200, body = kontor_catalog::contract::EntriesView),
        (status = 400), (status = 401), (status = 403), (status = 503)
    )
)]
pub async fn entries(
    State(state): State<ApiState>,
    caller: Caller,
    Query(query): Query<EntriesQuery>,
) -> Result<Json<kontor_catalog::contract::EntriesView>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog = state.catalog().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unavailable,
            "the work catalog is not composed in this realm",
        )
    })?;
    let kind = match query.kind.as_deref() {
        None => None,
        Some(text) => Some(kontor_catalog::EntryKind::parse(text).ok_or_else(|| {
            state.refuse(
                ApiErrorCode::InvalidRequest,
                "the kind filter must be `plan` or `execution_unit`",
            )
        })?),
    };
    let filter = kontor_catalog::EntryFilter {
        kind,
        scope: query.scope,
        include_removed: query.include_removed,
        limit: query.limit.unwrap_or(1000),
    };
    catalog.entries(&filter).map(Json).map_err(|error| {
        warn!(detail = %error, "work catalog entries read failed");
        state.refuse(ApiErrorCode::Unavailable, "the work catalog is unavailable")
    })
}
