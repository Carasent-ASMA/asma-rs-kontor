//! Thin `/v1` interface over the native memory ledger.
#![allow(missing_docs)]

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

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
pub struct BacklogSwitch {
    pub source: String,
    #[schema(value_type = String)]
    pub final_import_hash: ContentHash,
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

#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/backlog/cutover:switch",
    tag = "memory",
    params(
        ("project_id" = String, Path),
        ("Idempotency-Key" = String, Header)
    ),
    request_body = BacklogSwitch,
    responses((status = 200), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn switch_backlog(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    headers: HeaderMap,
    Json(body): Json<BacklogSwitch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let _key = crate::control::idempotency_key(&state, &headers)?;
    let project = parse_project(&state, &project)?;
    let readback = state
        .with_store(|store| store.backlog_readback_hash(project))
        .map_err(|error| map_authority(&state, error))?;
    let (row, receipt) = state
        .with_store(|store| {
            store.switch_subject_authority(
                project,
                AuthoritySubject::Backlog,
                &body.source,
                &body.final_import_hash,
                &readback,
                body.expected_revision,
            )
        })
        .map_err(|error| map_authority(&state, error))?;
    Ok(Json(serde_json::json!({
        "realm_id": state.realm_id(), "project_id": project, "subject": row.subject,
        "authority": row.authority, "revision": row.revision.get(), "receipt": receipt,
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
pub fn map(state: &ApiState, error: MemoryError) -> ApiError {
    match error {
        MemoryError::RevisionConflict { current, .. } => ApiError::new(
            state.realm_id(),
            ApiErrorCode::RevisionConflict,
            "the memory aggregate moved since the caller read it",
        )
        .with_revision(AggregateRevision::parse(current).ok()),
        MemoryError::Refused(code) => {
            use kontor_core::memory::MemoryRefusal;
            let api_code = match code {
                MemoryRefusal::InvalidExperience => ApiErrorCode::InvalidExperience,
                MemoryRefusal::UnresolvedEvidence => ApiErrorCode::UnresolvedEvidence,
                MemoryRefusal::FrozenPayloadPurged => ApiErrorCode::FrozenPayloadPurged,
                MemoryRefusal::FrozenPayloadMismatch => ApiErrorCode::FrozenPayloadMismatch,
                MemoryRefusal::BindingConflict => ApiErrorCode::MemoryBindingConflict,
                MemoryRefusal::CandidateLimit => ApiErrorCode::MemoryCandidateLimit,
                MemoryRefusal::ProjectionConflict => ApiErrorCode::ProjectionConflict,
                MemoryRefusal::ProjectionUnavailable => ApiErrorCode::ProjectionUnavailable,
            };
            state.refuse(api_code, code.as_str())
        }
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

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallRequest {
    #[schema(value_type = String)]
    pub task_id: kontor_core::id::TaskId,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallFreezeRequest {
    #[schema(value_type = String)]
    pub task_id: kontor_core::id::TaskId,
    #[schema(value_type = String)]
    pub agent_run_id: kontor_core::id::AgentRunId,
}
#[derive(serde::Serialize, ToSchema)]
pub struct RecallResponse {
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    #[schema(schema_with = crate::memory_schema::recall)]
    pub recall: kontor_store::memory::RecalledMemory,
}
#[derive(serde::Serialize, ToSchema)]
pub struct ProjectionResponse {
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    #[schema(schema_with = crate::memory_schema::projection)]
    pub projection: kontor_store::memory::ProjectionReadback,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExperienceProposal {
    pub item_id: String,
    pub expected_revision: u64,
    #[schema(schema_with = crate::memory_schema::experience)]
    pub document: CanonicalDocument,
    #[schema(schema_with = crate::memory_schema::provenance)]
    pub provenance: MemoryProvenance,
    pub proposed_by: String,
}

#[derive(serde::Serialize, ToSchema)]
pub struct ExperienceProposalResponse {
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    #[schema(schema_with = crate::memory_schema::revision)]
    pub revision: kontor_store::memory::MemoryRevision,
    #[schema(schema_with = crate::memory_schema::receipt)]
    pub receipt: kontor_store::memory::MemoryReceipt,
}
#[derive(serde::Serialize, ToSchema)]
pub struct ProjectionPreviewResponse {
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    #[schema(schema_with = crate::memory_schema::preview)]
    pub preview: kontor_store::memory::ProjectionPreview,
}
#[derive(serde::Serialize, ToSchema)]
pub struct ExperienceClassificationResponse {
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    #[schema(schema_with = crate::memory_schema::classifications)]
    pub classifications: Vec<kontor_store::memory::ExperienceClassification>,
}

#[utoipa::path(post, path = "/v1/projects/{project_id}/memory/experiences:propose", tag = "memory", params(("project_id" = String, Path), ("Idempotency-Key" = String, Header)), request_body = ExperienceProposal, responses((status = 200, body = ExperienceProposalResponse), (status = 400), (status = 409)))]
pub async fn propose_experience(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ExperienceProposal>,
) -> Result<Json<ExperienceProposalResponse>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project = parse_project(&state, &project)?;
    let key = crate::control::idempotency_key(&state, &headers)?;
    let (revision, receipt) = state
        .with_store(|store| {
            store.propose_experience_idempotent(
                project,
                &key,
                &body.item_id,
                body.expected_revision,
                &body.document,
                &body.provenance,
                &body.proposed_by,
            )
        })
        .map_err(|error| map(&state, error))?;
    Ok(Json(ExperienceProposalResponse {
        realm_id: state.realm_id(),
        revision,
        receipt,
    }))
}
#[utoipa::path(post, path = "/v1/projects/{project_id}/memory/recall:preview", tag = "memory", params(("project_id" = String, Path)), request_body = RecallRequest, responses((status = 200, body = RecallResponse), (status = 400), (status = 404)))]
pub async fn recall_preview(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    Json(body): Json<RecallRequest>,
) -> Result<Json<RecallResponse>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let recall = state
        .applications()
        .recall_memory(project, body.task_id, None, None)?;
    Ok(Json(RecallResponse {
        realm_id: state.realm_id(),
        recall,
    }))
}
#[utoipa::path(post, path = "/v1/projects/{project_id}/memory/recall:freeze", tag = "memory", params(("project_id" = String, Path), ("Idempotency-Key" = String, Header)), request_body = RecallFreezeRequest, responses((status = 200, body = RecallResponse), (status = 400), (status = 409), (status = 410)))]
pub async fn recall_freeze(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    headers: HeaderMap,
    Json(body): Json<RecallFreezeRequest>,
) -> Result<Json<RecallResponse>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let key = crate::control::idempotency_key(&state, &headers)?;
    let project = parse_project(&state, &project)?;
    let recall = state.applications().recall_memory(
        project,
        body.task_id,
        Some(body.agent_run_id),
        Some(&key),
    )?;
    Ok(Json(RecallResponse {
        realm_id: state.realm_id(),
        recall,
    }))
}
#[utoipa::path(get, path = "/v1/projects/{project_id}/memory/recall/{agent_run_id}", tag = "memory", params(("project_id" = String, Path), ("agent_run_id" = String, Path)), responses((status = 200, body = RecallResponse), (status = 404), (status = 410)))]
pub async fn recall_readback(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, run)): Path<(String, String)>,
) -> Result<Json<RecallResponse>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let run = crate::control::parse_id(&state, kontor_core::id::AgentRunId::parse(&run))?;
    let recall = state
        .with_store(|store| store.recalled_memory(project, &run.to_string()))
        .map_err(|error| map(&state, error))?
        .ok_or_else(|| {
            state.refuse(
                ApiErrorCode::NotFound,
                "no frozen experience recall exists for this run",
            )
        })?;
    Ok(Json(RecallResponse {
        realm_id: state.realm_id(),
        recall,
    }))
}
#[utoipa::path(get, path = "/v1/projects/{project_id}/memory/projection:preview", tag = "memory", params(("project_id" = String, Path)), responses((status = 200, body = ProjectionPreviewResponse)))]
pub async fn projection_preview(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
) -> Result<Json<ProjectionPreviewResponse>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let preview = state
        .with_store(|store| store.projection_preview(project))
        .map_err(|error| map(&state, error))?;
    Ok(Json(ProjectionPreviewResponse {
        realm_id: state.realm_id(),
        preview,
    }))
}
#[utoipa::path(get, path = "/v1/projects/{project_id}/memory/projection", tag = "memory", params(("project_id" = String, Path)), responses((status = 200, body = ProjectionResponse)))]
pub async fn projection_readback(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
) -> Result<Json<ProjectionResponse>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let projection = state
        .with_store(|store| store.projection_readback(project))
        .map_err(|error| map(&state, error))?;
    Ok(Json(ProjectionResponse {
        realm_id: state.realm_id(),
        projection,
    }))
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRebuildRequest {
    pub expected_generation: u64,
    pub expected_memory_cursor: i64,
    #[schema(value_type = String)]
    pub preview_digest: ContentHash,
}
#[utoipa::path(post, path = "/v1/projects/{project_id}/memory/projection:rebuild", tag = "memory", params(("project_id" = String, Path), ("Idempotency-Key" = String, Header)), request_body = ProjectionRebuildRequest, responses((status = 200, body = ProjectionResponse), (status = 409), (status = 503)))]
pub async fn projection_rebuild(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ProjectionRebuildRequest>,
) -> Result<Json<ProjectionResponse>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project = parse_project(&state, &project)?;
    let key = crate::control::idempotency_key(&state, &headers)?;
    let projection = state
        .applications()
        .rebuild_memory_projection(project, &key, &body)
        .await?;
    Ok(Json(ProjectionResponse {
        realm_id: state.realm_id(),
        projection,
    }))
}
#[utoipa::path(get, path = "/v1/projects/{project_id}/memory/experiences:classify", tag = "memory", params(("project_id" = String, Path)), responses((status = 200, body = ExperienceClassificationResponse)))]
pub async fn classify(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project): Path<String>,
) -> Result<Json<ExperienceClassificationResponse>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project = parse_project(&state, &project)?;
    let classifications = state
        .with_store(|store| store.classify_experiences(project))
        .map_err(|error| map(&state, error))?;
    Ok(Json(ExperienceClassificationResponse {
        realm_id: state.realm_id(),
        classifications,
    }))
}
