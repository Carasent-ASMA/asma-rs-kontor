//! Thin `/v1` interface over the native memory ledger.
#![allow(missing_docs)]

use axum::extract::{Path, Query, State};

use crate::body::Json;
use kontor_core::authority::AuthoritySubject;
use kontor_core::id::{AggregateRevision, CanonicalDocument, ContentHash, ProjectId};
use kontor_store::authority::AuthorityError;
use kontor_store::memory::{AgentsRoomExport, LegacyMemoryEntry, MemoryError, MemoryProvenance};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::{
    Caller,
    auth::CallerCapability,
    error::{ApiError, ApiErrorCode},
    state::ApiState,
};

#[derive(Deserialize)]
pub struct Search {
    pub q: Option<String>,
    pub limit: Option<u32>,
}
#[derive(Deserialize, ToSchema)]
pub struct Propose {
    pub item_id: String,
    pub expected_revision: u64,
    #[schema(value_type = Object)]
    pub document: CanonicalDocument,
    #[schema(value_type = Object)]
    pub provenance: MemoryProvenance,
    pub proposed_by: String,
}
#[derive(Deserialize, ToSchema)]
pub struct Approve {
    pub item_id: String,
    pub expected_revision: u64,
    pub approved_by: String,
}
#[derive(Deserialize, ToSchema)]
pub struct Tombstone {
    pub expected_revision: u64,
    pub by: String,
    pub reason: String,
}
#[derive(Deserialize, ToSchema)]
pub struct Purge {
    pub by: String,
}
#[derive(Deserialize, ToSchema)]
pub struct Switch {
    pub source: String,
    #[schema(value_type = String)]
    pub snapshot_hash: ContentHash,
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}
#[derive(Deserialize, ToSchema)]
pub struct Attest {
    #[schema(value_type = String)]
    pub subject: AuthoritySubject,
    pub source_cursor: String,
    #[schema(value_type = String)]
    pub source_hash: ContentHash,
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}
#[derive(Deserialize, ToSchema)]
pub struct ImportBody {
    pub schema_version: u32,
    pub source: String,
    #[schema(value_type = Vec<Object>)]
    pub entries: Vec<LegacyMemoryEntry>,
    #[schema(value_type = String)]
    pub snapshot_hash: ContentHash,
}

#[utoipa::path(
    get,
    path = "/v1/projects/{project_id}/memory",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("q" = Option<String>, Query),
        ("limit" = Option<u32>, Query)
    ),
    responses((status = 400, description = "always: replaced by per-project attestation"))
)]
pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Query(search): Query<Search>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let (rows, cursor) = state
        .with_store(|s| {
            let rows = match search.q {
                Some(ref q) => s.search_memory(project, q, search.limit.unwrap_or(20)),
                None => s.list_memory(project),
            }?;
            Ok((rows, s.memory_cursor()?))
        })
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"project_id":project,"cursor":cursor,"revisions":rows}),
    ))
}
#[utoipa::path(
    get,
    path = "/v1/projects/{project_id}/memory/{item_id}/history",
    tag = "memory",
    params(("project_id" = String, Path), ("item_id" = String, Path)),
    responses((status = 200))
)]
pub async fn history(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, item)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let (rows, cursor) = state
        .with_store(|s| Ok((s.memory_history(project, &item)?, s.memory_cursor()?)))
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"project_id":project,"item_id":item,"cursor":cursor,"revisions":rows}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/revisions:propose",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = Propose,
    responses((status = 200))
)]
pub async fn propose(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Json(body): Json<Propose>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project = parse_project(&state, &project)?;
    let (revision, receipt) = state
        .with_store(|s| {
            s.propose_memory_revision(
                project,
                &body.item_id,
                body.expected_revision,
                &body.document,
                &body.provenance,
                &body.proposed_by,
            )
        })
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"revision":revision,"receipt":receipt}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/revisions/{revision_id}/approval",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("revision_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = Approve,
    responses((status = 200))
)]
pub async fn approve(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, revision)): Path<(String, String)>,
    Json(body): Json<Approve>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let receipt = state
        .with_store(|s| {
            s.approve_memory_revision(
                project,
                &body.item_id,
                &revision,
                body.expected_revision,
                &body.approved_by,
            )
        })
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"receipt":receipt}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/{item_id}/tombstone",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("item_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = Tombstone,
    responses((status = 200))
)]
pub async fn tombstone(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, item)): Path<(String, String)>,
    Json(body): Json<Tombstone>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let receipt = state
        .with_store(|s| {
            s.tombstone_memory(
                project,
                &item,
                body.expected_revision,
                &body.by,
                &body.reason,
            )
        })
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"receipt":receipt}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/{item_id}/purge",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("item_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = Purge,
    responses((status = 200))
)]
pub async fn purge(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, item)): Path<(String, String)>,
    Json(body): Json<Purge>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let receipt = state
        .with_store(|s| s.purge_memory(project, &item, &body.by))
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"receipt":receipt}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/import:preview",
    tag = "memory",
    params(("project_id" = String, Path)),
    request_body = ImportBody,
    responses((status = 200))
)]
pub async fn import_preview(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Json(body): Json<ImportBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let export = AgentsRoomExport {
        schema_version: body.schema_version,
        source: body.source,
        project_id: project,
        entries: body.entries,
        export_hash: body.snapshot_hash,
    };
    let preview = state
        .with_store(|s| s.preview_agentsroom_import(&export))
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"preview":preview}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/import:apply",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = ImportBody,
    responses((status = 200))
)]
pub async fn import_apply(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Json(body): Json<ImportBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let export = AgentsRoomExport {
        schema_version: body.schema_version,
        source: body.source,
        project_id: project,
        entries: body.entries,
        export_hash: body.snapshot_hash,
    };
    let preview = state
        .with_store(|s| s.apply_agentsroom_import(&export))
        .map_err(|e| map(&state, e))?;
    Ok(Json(
        serde_json::json!({"realm_id":state.realm_id(),"import":preview}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/memory/cutover:freeze",
    tag = "memory",
    params(("Idempotency-Key" = String, Header)),
    responses((status = 200))
)]
pub async fn freeze(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    Err(state.refuse(
        ApiErrorCode::InvalidRequest,
        "realm-wide memory freeze was replaced by per-project attestation: POST /v1/projects/{project_id}/subjects/authority:attest",
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/subjects/authority:attest",
    tag = "memory",
    params(("project_id" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = Attest,
    responses((status = 200))
)]
pub async fn attest_authority(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Json(body): Json<Attest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let (row, receipt) = state
        .with_store(|s| {
            s.attest_subject_source_frozen(
                project,
                body.subject,
                body.expected_revision,
                &body.source_cursor,
                &body.source_hash,
            )
        })
        .map_err(|e| map_authority(&state, e))?;
    Ok(Json(serde_json::json!({
        "realm_id": state.realm_id(), "subject": row.subject,
        "authority": row.authority, "revision": row.revision.get(), "receipt": receipt,
    })))
}
#[utoipa::path(
    get,
    path = "/v1/projects/{project_id}/subjects/authority",
    tag = "memory",
    params(("project_id" = String, Path)),
    responses((status = 200))
)]
pub async fn authority(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let rows = state
        .with_store(|s| s.subject_authorities(project))
        .map_err(|e| map_authority(&state, e))?;
    let subjects: Vec<_> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "subject": row.subject, "origin": row.origin, "authority": row.authority,
                "revision": row.revision.get(), "source_frozen_at": row.source_frozen_at,
                "final_import_hash": row.final_import_hash, "readback_hash": row.readback_hash,
                "switched_at": row.switched_at,
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({"realm_id": state.realm_id(), "subjects": subjects}),
    ))
}
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/memory/cutover:switch",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = Switch,
    responses((status = 200))
)]
pub async fn switch(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Json(body): Json<Switch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project = parse_project(&state, &project)?;
    let receipt = state
        .with_store(|s| {
            s.switch_project_memory_authority(
                project,
                &body.source,
                &body.snapshot_hash,
                body.expected_revision,
            )
        })
        .map_err(|e| map(&state, e))?;
    Ok(Json(serde_json::json!({
        "realm_id": state.realm_id(), "project_id": project, "subject": "memory",
        "memory_authority": "kontor", "receipt": receipt,
    })))
}

fn parse_project(state: &ApiState, text: &str) -> Result<ProjectId, ApiError> {
    ProjectId::parse(text).map_err(|e| ApiError::from_domain(state.realm_id(), &e))
}
fn map_authority(state: &ApiState, error: AuthorityError) -> ApiError {
    match error {
        AuthorityError::RevisionConflict { current, .. } => ApiError::new(
            state.realm_id(),
            ApiErrorCode::RevisionConflict,
            "the subject authority moved since the caller read it",
        )
        .with_revision(AggregateRevision::parse(current).ok()),
        AuthorityError::NotFound => state.refuse(
            ApiErrorCode::NotFound,
            "this project has no declared authority for that subject",
        ),
        AuthorityError::Denied { .. } => state.refuse(
            ApiErrorCode::Forbidden,
            "the legacy system still owns this project's subject",
        ),
        AuthorityError::Domain(e) => ApiError::from_domain(state.realm_id(), &e),
        _ => state.refuse(
            ApiErrorCode::InvalidRequest,
            "the subject authority operation was refused",
        ),
    }
}
fn map(state: &ApiState, error: MemoryError) -> ApiError {
    match error {
        MemoryError::RevisionConflict { current, .. } => ApiError::new(
            state.realm_id(),
            ApiErrorCode::RevisionConflict,
            "the memory aggregate moved since the caller read it",
        )
        .with_revision(AggregateRevision::parse(current).ok()),
        MemoryError::NotFound => state.refuse(
            ApiErrorCode::NotFound,
            "no such memory record exists in this project",
        ),
        MemoryError::Authority { .. } => state.refuse(
            ApiErrorCode::Forbidden,
            "native memory writes are unavailable until the one-way authority switch",
        ),
        MemoryError::Domain(e) => ApiError::from_domain(state.realm_id(), &e),
        _ => state.refuse(
            ApiErrorCode::InvalidRequest,
            "the memory operation was refused",
        ),
    }
}
