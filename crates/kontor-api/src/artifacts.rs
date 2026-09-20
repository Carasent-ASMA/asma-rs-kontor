//! Operator recovery of an addressable artifact claimed by a settled turn.
use crate::{
    Caller,
    auth::CallerCapability,
    body::Json,
    control::{idempotency_key, parse_id},
    error::ApiError,
    state::ApiState,
};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kontor_core::id::{AggregateRevision, ProjectId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Attaches verified Git bytes to an existing claim; does not assert agent authorship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordArtifactRequest {
    /// Exact existing settled-turn receipt.
    pub role_turn_id: String,
    /// Declared artifact key already claimed by that turn.
    pub artifact_key: String,
    /// Current task revision.
    #[schema(value_type = u64)]
    pub expected_task_revision: AggregateRevision,
    /// Registered repository root: `task` worktree or `project` checkout.
    pub repository: String,
    /// Full immutable Git commit object id (40 or 64 lower-case hexadecimal digits).
    pub commit: String,
    /// Repository-relative blob path, without parent traversal.
    pub path: String,
    /// Expected SHA-256 of the blob contents, verified by the daemon.
    pub sha256: String,
}

/// Immutable augmentation receipt. Attribution identifies the old claim, not byte authorship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ArtifactSubmissionDto {
    /// Envelope schema version.
    pub schema_version: u32,
    /// Owning Realm.
    pub realm_id: String,
    /// Artifact registry identity.
    pub evidence_id: String,
    /// Exact task and workflow.
    pub task_id: String,
    /// Pinned workflow whose declared artifact this satisfies.
    pub workflow_id: String,
    /// Source settled-turn receipt.
    pub role_turn_id: String,
    /// Source persistent agent run.
    pub agent_run_id: String,
    /// Source role slot, derived from the settled turn.
    pub producer_role: String,
    /// Provider account derived from the source run; not the calling operator's identity.
    pub producer_account: String,
    /// Declared artifact key.
    pub artifact_key: String,
    /// Pinned producing phase.
    pub producer_phase: String,
    /// Always `operator_recovered_git_blob`, never inferred agent authorship.
    pub provenance: String,
    /// Native-proof versus historical/attested source turn.
    pub turn_proof_class: String,
    /// Credential tier that authorized the augmentation.
    pub recorded_by: String,
    /// Persistent common Git directory, independent of a disposable worktree.
    pub git_dir: String,
    /// Exact commit locator.
    pub commit: String,
    /// Exact blob path.
    pub path: String,
    /// Verified content digest.
    pub sha256: String,
    /// Canonical locator digest.
    pub locator_hash: String,
    /// Verification/recording instant.
    pub recorded_at: String,
}

/// Verify and durably record one settled-turn artifact augmentation.
#[utoipa::path(post, path = "/v1/projects/{project_id}/tasks/{task_id}/artifacts:record", tag = "applications",
    params(("project_id" = String, Path), ("task_id" = String, Path), ("Idempotency-Key" = String, Header)),
    request_body = RecordArtifactRequest,
    responses((status = 200, body = ArtifactSubmissionDto), (status = 401), (status = 403), (status = 404), (status = 409)))]
pub async fn record_artifact(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project, task)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordArtifactRequest>,
) -> Result<Json<ArtifactSubmissionDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project = parse_id(&state, ProjectId::parse(&project))?;
    let task = crate::applications::resolve_task_selector(&state, project, &task)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .record_artifact(&key, caller.0, project, task, &request)
            .await?,
    ))
}
