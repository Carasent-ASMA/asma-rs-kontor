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

use std::sync::Arc;

use axum::extract::{Query, State};
use serde::Deserialize;
use tracing::warn;
use utoipa::IntoParams;

use crate::body::Json;
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;
use crate::{Caller, CallerCapability};

/// Run a blocking catalog read off the async worker.
async fn read_blocking<T, F>(
    catalog: Arc<kontor_catalog::Catalog>,
    read: F,
) -> Result<T, kontor_catalog::CatalogError>
where
    T: Send + 'static,
    F: FnOnce(&kontor_catalog::Catalog) -> Result<T, kontor_catalog::CatalogError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || read(&catalog))
        .await
        .map_err(|error| kontor_catalog::CatalogError::Unavailable {
            detail: format!("the catalog read task failed ({error})"),
        })?
}

/// Filters accepted by the entries read.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct EntriesQuery {
    /// `plan` or `execution_unit`.
    #[param(required = false)]
    pub kind: Option<String>,
    /// A repository id, worktree root, path prefix, logical plan id or workstream key.
    #[param(required = false)]
    pub scope: Option<String>,
    /// Include tombstoned entries. Defaults to false.
    #[serde(default)]
    #[param(required = false, default = false)]
    pub include_removed: bool,
    /// A bounded row limit.
    #[param(required = false)]
    pub limit: Option<usize>,
    /// An opaque continuation from the prior page. Bound to its generation and filter.
    #[param(required = false)]
    pub cursor: Option<String>,
}

/// Filters accepted by the native inventory read.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct NativeInventoryQuery {
    /// Include native observations removed at the active generation.
    #[serde(default)]
    #[param(required = false, default = false)]
    pub include_removed: bool,
    /// Only this stable runtime instance.
    #[param(required = false)]
    pub instance: Option<String>,
    /// A bounded row limit per collection.
    #[param(required = false)]
    pub limit: Option<usize>,
    /// An opaque continuation from the prior page. Bound to its generation and filter.
    #[param(required = false)]
    pub cursor: Option<String>,
}

/// The native census's freshness, coverage and counts.
#[utoipa::path(
    get, path = "/v1/work-catalog/native/status", tag = "work-catalog",
    responses(
        (status = 200, body = kontor_catalog::native::NativeCatalogStatus),
        (status = 401), (status = 403), (status = 503)
    )
)]
pub async fn native_status(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<kontor_catalog::native::NativeCatalogStatus>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog = state.catalog().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unavailable,
            "the work catalog is not composed in this realm",
        )
    })?;
    read_blocking(Arc::clone(catalog), |catalog| catalog.native_status())
        .await
        .map(Json)
        .map_err(|error| {
            warn!(detail = %error, "native work census status read failed");
            state.refuse(
                ApiErrorCode::Unavailable,
                "the native census is unavailable",
            )
        })
}

/// One generation-consistent native inventory.
#[utoipa::path(
    get, path = "/v1/work-catalog/native/inventory", tag = "work-catalog",
    params(NativeInventoryQuery),
    responses(
        (status = 200, body = kontor_catalog::native::NativeInventoryView),
        (status = 400), (status = 401), (status = 403), (status = 410), (status = 503)
    )
)]
pub async fn native_inventory(
    State(state): State<ApiState>,
    caller: Caller,
    Query(query): Query<NativeInventoryQuery>,
) -> Result<Json<kontor_catalog::native::NativeInventoryView>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog = state.catalog().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unavailable,
            "the work catalog is not composed in this realm",
        )
    })?;
    if query.limit == Some(0) {
        return Err(state.refuse(ApiErrorCode::InvalidRequest, "the limit must be at least 1"));
    }
    let filter = kontor_catalog::native::NativeInventoryFilter {
        include_removed: query.include_removed,
        runtime_instance: query.instance,
        limit: query.limit.unwrap_or(10_000),
        cursor: query.cursor,
    };
    read_blocking(Arc::clone(catalog), move |catalog| {
        catalog.native_inventory(&filter)
    })
    .await
    .map(Json)
    .map_err(|error| match error {
        kontor_catalog::CatalogError::StaleCursor { .. } => state.refuse(
            ApiErrorCode::StaleCursor,
            "the cursor belongs to a native generation that is no longer active; read again without a cursor",
        ),
        kontor_catalog::CatalogError::InvalidCursor { .. } => state.refuse(
            ApiErrorCode::InvalidRequest,
            "the cursor is not a valid continuation for this native read",
        ),
        error => {
            warn!(detail = %error, "native work census inventory read failed");
            state.refuse(
                ApiErrorCode::Unavailable,
                "the native census is unavailable",
            )
        }
    })
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
    read_blocking(Arc::clone(catalog), |catalog| catalog.status())
        .await
        .map(Json)
        .map_err(|error| {
            warn!(detail = %error, "work catalog status read failed");
            state.refuse(ApiErrorCode::Unavailable, "the work catalog is unavailable")
        })
}

/// Per-source freshness, coverage, errors, retry deadlines and conflicts.
#[utoipa::path(
    get, path = "/v1/work-catalog/sources", tag = "work-catalog",
    responses(
        (status = 200, body = kontor_catalog::contract::CatalogSourcesView),
        (status = 401), (status = 403), (status = 503)
    )
)]
pub async fn sources(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<kontor_catalog::contract::CatalogSourcesView>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog = state.catalog().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Unavailable,
            "the work catalog is not composed in this realm",
        )
    })?;
    read_blocking(Arc::clone(catalog), |catalog| catalog.sources())
        .await
        .map(Json)
        .map_err(|error| {
            warn!(detail = %error, "work catalog source status read failed");
            state.refuse(ApiErrorCode::Unavailable, "the work catalog is unavailable")
        })
}

/// One generation-consistent entries answer.
#[utoipa::path(
    get, path = "/v1/work-catalog/entries", tag = "work-catalog",
    params(EntriesQuery),
    responses(
        (status = 200, body = kontor_catalog::contract::EntriesView),
        (status = 400), (status = 401), (status = 403), (status = 410), (status = 503)
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
    if query.limit == Some(0) {
        return Err(state.refuse(ApiErrorCode::InvalidRequest, "the limit must be at least 1"));
    }
    let filter = kontor_catalog::EntryFilter {
        kind,
        scope: query.scope,
        include_removed: query.include_removed,
        limit: query.limit.unwrap_or(1000),
        cursor: query.cursor,
    };
    read_blocking(Arc::clone(catalog), move |catalog| catalog.entries(&filter))
        .await
        .map(Json)
        .map_err(|error| match error {
        kontor_catalog::CatalogError::StaleCursor { .. } => state.refuse(
            ApiErrorCode::StaleCursor,
            "the cursor belongs to a catalog generation that is no longer active; read again without a cursor",
        ),
        kontor_catalog::CatalogError::InvalidCursor { .. } => state.refuse(
            ApiErrorCode::InvalidRequest,
            "the cursor is not a valid continuation for this read",
        ),
        error => {
            warn!(detail = %error, "work catalog entries read failed");
            state.refuse(ApiErrorCode::Unavailable, "the work catalog is unavailable")
        }
    })
}
