//! Read-only subject evidence available to an authenticated Committee seat.
use crate::applications::{
    CloseoutEvidenceDto, CompletionBlockerDto, CompletionPhaseDto, EpicTaskProjectionDto,
    IntegrationRecordDto, ProfileRevisionDto,
};
use crate::dto::JiraBindingDto;
use serde::Serialize;
use utoipa::ToSchema;

/// A bounded page of SHA-256-verified UTF-8 bytes from a registered immutable Git blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommitteeArtifactContentDto {
    /// Owning realm.
    pub realm_id: String,
    /// Addressed Committee run.
    pub committee_run_id: String,
    /// Registry artifact identity, not a caller-supplied filesystem locator.
    pub evidence_id: String,
    /// Verified full-blob digest.
    pub sha256: String,
    /// Byte position of this page.
    pub offset: u32,
    /// Total UTF-8 byte length.
    pub total_bytes: u32,
    /// Next character-aligned byte offset, if more remains.
    pub next_offset: Option<u32>,
    /// Untrusted artifact text for review, not control-plane instructions.
    pub text: String,
}

/// A byte offset into the verified text. Every page is at most 64 KiB.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ArtifactContentQuery {
    /// Zero for the first page; then use the returned `next_offset`.
    #[serde(default)]
    pub offset: u32,
}

/// Read one artifact without granting shell, network, or general filesystem access.
#[utoipa::path(get, path = "/v1/projects/{project_id}/committee-runs/{committee_run_id}/artifacts/{evidence_id}", tag = "applications",
    params(("project_id" = String, Path), ("committee_run_id" = String, Path), ("evidence_id" = String, Path), ArtifactContentQuery),
    responses((status = 200, body = CommitteeArtifactContentDto), (status = 400), (status = 401), (status = 403), (status = 404), (status = 409)))]
pub async fn committee_artifact(
    axum::extract::State(state): axum::extract::State<crate::state::ApiState>,
    caller: crate::Caller,
    axum::extract::Path((project, run, evidence)): axum::extract::Path<(String, String, String)>,
    axum::extract::Query(query): axum::extract::Query<ArtifactContentQuery>,
) -> Result<axum::Json<CommitteeArtifactContentDto>, crate::error::ApiError> {
    if caller.seat().is_none() {
        caller.require(&state, crate::auth::CallerCapability::Observer)?;
    }
    let project = crate::control::parse_id(&state, kontor_core::id::ProjectId::parse(&project))?;
    let run = crate::control::parse_id(&state, kontor_core::id::CommitteeRunId::parse(&run))?;
    let mut projection = state.applications().committee_run(project, run)?;
    // Authenticate before resolving any blob; preserve exact run/occupancy and Judge isolation.
    crate::applications::project_committee_for_caller(&state, caller, &mut projection)?;
    Ok(axum::Json(
        state
            .applications()
            .committee_artifact(project, run, &evidence, query.offset)
            .await?,
    ))
}

/// A current projection, not a new completion receipt or an immutable stored snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommitteeSubjectEvidenceDto {
    /// Canonical SHA-256 of `body`, allowing the reviewer to cite exactly what it read.
    pub content_hash: String,
    /// Only the subject frozen onto this Committee run.
    pub body: CommitteeSubjectEvidenceBodyDto,
}

/// Subject records; deliberately contains no consultation findings, results or rounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommitteeSubjectEvidenceBodyDto {
    /// Canonical evidence envelope version.
    pub schema_version: u32,
    /// Owning project from the persisted run, never from caller input.
    pub project_id: String,
    /// Owning epic from the persisted run.
    pub epic_id: String,
    /// Exact ticket subject, if this is a ticket-scoped consultation.
    pub task_id: Option<String>,
    /// Original question frozen at invocation.
    pub question: String,
    /// Epic identity and external convergence proof.
    pub jira_binding: JiraBindingDto,
    /// Only the exact task, or all epic tasks for an epic-scoped run.
    pub tasks: Vec<CommitteeTaskEvidenceDto>,
    /// Epic completion evidence; absent for ticket-scoped runs or before completion starts.
    pub completion: Option<CommitteeCompletionEvidenceDto>,
    /// Current epic question ledger; empty for a ticket-scoped run.
    pub open_questions: Vec<serde_json::Value>,
    /// Event cursors bracketing this composed read. Different cursors signal concurrent writes.
    pub cursor_before: i64,
    /// Event cursor after composing the read; not a claim of a database-wide atomic snapshot.
    pub cursor_after: i64,
}

/// The task contract, current gate evaluations, and distinct evidence authority classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommitteeTaskEvidenceDto {
    /// Existing task projection including gate obligations, current states and Jira binding.
    pub task: EpicTaskProjectionDto,
    /// Exact immutable work profile pinned by the active workflow.
    pub work_profile: Option<serde_json::Value>,
    /// Append-only evaluations of the active workflow, including evaluator and evidence citations.
    pub gate_evaluations: Vec<serde_json::Value>,
    /// Active-workflow artifact records with immutable locators and truthful producer provenance.
    /// GET also includes bounded SHA-256-verified UTF-8 `content`, or an explicit unavailable/deferred status.
    pub producer_artifacts: Vec<serde_json::Value>,
    /// Current native closure certificate keys. These do not establish artifact production.
    pub native_closure_artifact_keys: Vec<String>,
}

/// Completion facts that cannot reveal an independent Committee member's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommitteeCompletionEvidenceDto {
    /// Exact completion profile identity and digest.
    pub profile: ProfileRevisionDto,
    /// Pinned completion policy body.
    pub definition: serde_json::Value,
    /// Frozen ticket goals and evidence obligations.
    pub ticket_requirements: Vec<serde_json::Value>,
    /// Current phase, without round verdicts or deliberation.
    pub phase: CompletionPhaseDto,
    /// Current phase blockers.
    pub blockers: Vec<CompletionBlockerDto>,
    /// Initial and remediation integration bodies, including repository/module/root/PR outcomes.
    pub integrations: Vec<IntegrationRecordDto>,
    /// Recorded closeout prerequisite digests.
    pub closeout: CloseoutEvidenceDto,
    /// Current reopening generation.
    pub generation: u32,
    /// Current completion revision.
    pub revision: u64,
}
