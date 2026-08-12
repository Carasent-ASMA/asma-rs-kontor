//! The KON-MVP-16 contract amendment: the authenticated read routes a
//! command-line and an MCP caller cannot work without.
//!
//! # Why this module exists
//!
//! KON-MVP-15 merged a router that answers liveness, identity, two aggregate
//! snapshots, the command intake and the session routes. That is enough for a
//! console that already knows every identifier it wants. It is not enough for a
//! CLI or an MCP tool, which have to *find* the task before they can name it, and
//! have to be able to show an operator why a decision came out the way it did.
//!
//! So this module amends the contract, and the amendment is deliberately narrow:
//! **every route here is a thin read over a repository method that already
//! exists.** Not one of them adds SQL, and not one of them reaches past the
//! daemon into a store, a scheduler, a connector or a runtime the composition
//! root did not hand over. Where the owning seam has not merged — ticket reads,
//! calendar administration, intake proposals, the scheduling plan — there is no
//! route here at all, because a route that guessed would be worse than a missing
//! one. `crate::query::STAGED` names each of those and the ticket that owns it.
//!
//! # What is *not* here, and why that is the honest answer
//!
//! | Absent surface | Why |
//! | --- | --- |
//! | assigning a calendar, previewing or applying a holiday import | no command surface for it yet; the *resolved* policy travels with every candidate in [`crate::wired::scheduler_plan`] |
//! | intake proposals, source lineage, replay | `kontor-intake` is a scaffold (KON-MVP-22) |
//! | external-workflow mapping specifications | written by an onboarding path that has not merged |
//! | session adoption | no `CommandKind` records the intent |
//!
//! Lists, external-ticket evidence, live session discovery and the scheduling plan
//! itself are served by [`crate::wired`], which is the second amendment: each one
//! needed a merged seam behind it, and each one has it. [`STAGED`] is the current
//! list of what is still refused and why.
//!
//! [`scheduler_contention`] stays here and stays separate from the plan: it is the
//! raw contention evidence — which modules and worktrees are held, which tasks have
//! open runs — and it answers for the whole Realm rather than for one project, which
//! is a different and cheaper question.
//!
//! # Realm qualification without a borrowed cursor
//!
//! Every body here is wrapped in a [`ViewDto`], which carries the Realm and
//! nothing else. It deliberately has no `snapshot_cursor`: these reads are not
//! taken in the same transaction as a control-plane position, so quoting one
//! would invite a subscriber to resume from a cursor this answer was never
//! consistent with. A caller that needs a resumable position uses a snapshot
//! route — `/v1/runs/{id}` or `/v1/projects/{id}/tasks/{id}` — which has one.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::{Json, http::StatusCode};
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CommandReceiptId, ExternalId, ExternalName, GateKey,
    ModuleKey, PhaseKey, ProjectId, RealmId, RoleKey, RuntimeKindKey, SpecVersion, TaskId,
    TeamRunId, TeamTemplateId, Timestamp, WorkProfileKey,
};
use kontor_core::repository::{
    ProjectRepository, RealmRepository, RunRepository, SpecRepository, WorkflowRepository,
};
use kontor_core::state::{GateState, RunLifecycle, TaskState};
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth::CallerCapability;
use crate::control::parse_id;
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;
use crate::{Caller, now};

/// Every surface KON-MVP-16 was asked for that this build deliberately does not
/// serve, and the ticket that has to merge first.
///
/// It is a constant rather than a comment so the CLI and the MCP server can be
/// tested against it: a name on this list must not appear as a command or a tool,
/// because advertising a surface whose seam is absent is how an operator — or a
/// language model — is taught to trust an answer nobody computed.
pub const STAGED: &[(&str, &str)] = &[
    (
        "source-event intake over HTTP: delivering an event, and approving or rejecting a \
         proposal through this API",
        "KON-MVP-22 merged the domain, not the transport. `kontor-intake` normalizes and \
         decides, the store commits the identity before the decision and records approval, \
         rejection and bounded auto-arm append-only, and `scheduling_candidates` reads the \
         created-work lineage — an intake-created task is assembled as `TaskOrigin::Event` \
         and admitted through its receipt. What has no route here is the *ingress*: no \
         endpoint accepts a delivery from an authenticated connection and no endpoint runs \
         an `ApproveIntake` command against a proposal, so nothing in this API can create \
         intake work.",
    ),
    (
        "calendar administration: assigning a profile, previewing and applying a holiday import",
        "KON-MVP-21 resolves a calendar and `wired::scheduler_plan` reports the resolved \
         answer with every candidate, so the effective policy *is* observable. What is still \
         absent is the administrative half: no route assigns a profile revision, previews a \
         holiday import, applies one or approves an override, because a command surface for \
         those is KON-MVP-15's to add and a read route that showed a preview nobody could \
         apply would be advertising half a workflow.",
    ),
    (
        "external-workflow mapping: milestone-to-status specifications and their selection",
        "KON-MVP-21/22. `external_workflow_specs` and `ticket_field_specs` are written by an \
         onboarding path that has not merged, and `kontor_integrations_asma::jira::SpecCatalog` \
         needs an `asma` executable this daemon is not configured with. The ticket *evidence* \
         this realm recorded is served; the mapping that produced it is not.",
    ),
    (
        "session adoption",
        "`CommandKind` has no variant that records the intent to bind an existing native \
         session to an agent run, and binding one creates a run, a binding and a frozen \
         capability snapshot in a single transaction. Adding the variant is a `kontor-core` \
         change with its own compatibility-matrix entry. Discovery *is* wired: \
         `GET /v1/runtimes/{runtime_kind}/sessions` reports which native sessions exist and \
         which of them this realm already holds a binding for.",
    ),
    (
        "live external-ticket reads and applies through the asma connector",
        "The daemon is not configured with an `AsmaExecutable`, so \
         `kontor_integrations_asma::jira::TicketDelegation` cannot be constructed. What is \
         wired is the stored evidence — projection, observation, assignee, conflicts, \
         convergence attempts, inbound comments — and the receipt-backed commands that ask \
         the daemon's own dispatcher to converge. Nothing in this crate spawns a process.",
    ),
];

/// A Realm-qualified view.
///
/// The Realm is named on every answer for the same reason it is named on a
/// snapshot: a value that leaves this process is read as `(realm_id, …)` or not
/// at all.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ViewDto<T> {
    /// The Realm this view came from.
    #[schema(value_type = String)]
    pub realm_id: RealmId,
    /// The value.
    pub value: T,
}

impl<T> ViewDto<T> {
    /// Wrap one value in the Realm it was read in.
    ///
    /// Public so the second amendment in `crate::wired` uses the same wrapper: two
    /// envelope helpers would be two chances to forget the realm.
    pub fn of(state: &ApiState, value: T) -> Json<Self> {
        Json(Self {
            realm_id: state.realm_id(),
            value,
        })
    }
}

/// Refuse anything the addressed Realm does not hold.
fn found<T>(state: &ApiState, value: Option<T>, rule: &'static str) -> Result<T, ApiError> {
    value.ok_or_else(|| state.refuse(ApiErrorCode::NotFound, rule))
}

/// Read one project's identity and revision.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProjectDto {
    /// The project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// Its human name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// Its root path on disk.
    #[schema(value_type = String)]
    pub root_path: ExternalName,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// When it was created.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
}

/// One project.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}", tag = "query",
    params(("project_id" = String, Path, description = "The project")),
    responses((status = 200, body = ViewDto<ProjectDto>), (status = 404))
)]
pub async fn project(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ViewDto<ProjectDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let project = state
        .with_store(|store| store.get_project(project_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    let project = found(&state, project, "no such project exists in this realm")?;
    Ok(ViewDto::of(
        &state,
        ProjectDto {
            project_id: project.id,
            name: project.name,
            root_path: project.root_path,
            revision: project.revision,
            created_at: project.created_at,
        },
    ))
}

/// One task, as a list entry.
///
/// It is deliberately smaller than `TaskDto`: a list is for finding the task you
/// want, and the gates, the pinned revisions and the workflow are one read away
/// on the snapshot route.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskSummaryDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// Its title.
    #[schema(value_type = String)]
    pub title: ExternalName,
    /// Its lifecycle state.
    #[schema(value_type = String)]
    pub state: TaskState,
    /// The module it contends for, if any.
    #[schema(value_type = Option<String>)]
    pub module: Option<ModuleKey>,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// When it was created.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    /// When it last changed.
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: Timestamp,
}

/// Every task in one project, oldest first.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tasks", tag = "query",
    params(("project_id" = String, Path, description = "The project")),
    responses((status = 200, body = ViewDto<Vec<TaskSummaryDto>>), (status = 404))
)]
pub async fn tasks(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ViewDto<Vec<TaskSummaryDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    // The project is read first so an unknown id is a refusal rather than an
    // empty list. "No such project" and "a project with no tasks" are different
    // answers, and a caller paging through them must not have to guess which.
    let listed = state.with_store(|store| -> Result<_, ApiError> {
        let realm_id = state.realm_id();
        let project = store
            .get_project(project_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        if project.is_none() {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "no such project exists in this realm",
            ));
        }
        store
            .list_tasks(project_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))
    })?;
    Ok(ViewDto::of(
        &state,
        listed
            .into_iter()
            .map(|task| TaskSummaryDto {
                task_id: task.id,
                title: task.title,
                state: task.state,
                module: task.module,
                revision: task.revision,
                created_at: task.created_at,
                updated_at: task.updated_at,
            })
            .collect(),
    ))
}

/// One team run — a mission — and the template revision it froze.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MissionDto {
    /// The team run.
    #[schema(value_type = String)]
    pub team_run_id: TeamRunId,
    /// The project it belongs to.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The task it serves.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The team template it froze.
    #[schema(value_type = String)]
    pub team_template: TeamTemplateId,
    /// That template's pinned revision.
    #[schema(value_type = u32)]
    pub team_template_version: SpecVersion,
    /// Its lifecycle.
    #[schema(value_type = String)]
    pub lifecycle: RunLifecycle,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// When it was created.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    /// When it closed.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub closed_at: Option<Timestamp>,
}

/// One mission.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/team-runs/{team_run_id}", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("team_run_id" = String, Path, description = "The team run")
    ),
    responses((status = 200, body = ViewDto<MissionDto>), (status = 404))
)]
pub async fn mission(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, team_run_id)): Path<(String, String)>,
) -> Result<Json<ViewDto<MissionDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let team_run_id = parse_id(&state, TeamRunId::parse(&team_run_id))?;
    let run = state
        .with_store(|store| store.get_team_run(project_id, team_run_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    let run = found(&state, run, "no such team run exists in this project")?;
    Ok(ViewDto::of(
        &state,
        MissionDto {
            team_run_id: run.id,
            project_id: run.project_id,
            task_id: run.task_id,
            team_template: run.snapshot.template_id,
            team_template_version: run.snapshot.template_version,
            lifecycle: run.lifecycle,
            revision: run.revision,
            created_at: run.created_at,
            closed_at: run.closed_at,
        },
    ))
}

/// One phase of a work profile, with the gates evaluated at its end.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PhaseDto {
    /// The phase.
    #[schema(value_type = String)]
    pub phase: PhaseKey,
    /// Its human label.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// The gates evaluated at the end of it.
    #[schema(value_type = Vec<String>)]
    pub gates: Vec<GateKey>,
    /// Where a rejection returns the work to.
    #[schema(value_type = Option<String>)]
    pub rejection_route: Option<PhaseKey>,
}

/// One resolved work-profile revision, as its phase and gate structure.
///
/// The whole [`kontor_core::spec::WorkProfileSpec`] is deliberately not served:
/// it carries a runtime-routing reference, and a read route is not the place to
/// start naming where runs go. What a caller needs to reason about a task is the
/// phase DAG and the gates, which is what this is.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProfileDto {
    /// The profile key. An open, deployment-defined key.
    #[schema(value_type = String)]
    pub profile: WorkProfileKey,
    /// The immutable revision of that key.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
    /// Its human name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// The single entry phase.
    #[schema(value_type = String)]
    pub entry_phase: PhaseKey,
    /// The declared terminal phases.
    #[schema(value_type = Vec<String>)]
    pub terminal_phases: Vec<PhaseKey>,
    /// Every phase, in declaration order.
    pub phases: Vec<PhaseDto>,
}

impl ProfileDto {
    /// Build the view of one validated profile revision.
    fn of(definition: &kontor_core::spec::WorkProfileSpec) -> Self {
        Self {
            profile: definition.id.clone(),
            version: definition.version,
            name: definition.name.clone(),
            entry_phase: definition.entry_phase.clone(),
            terminal_phases: definition.terminal_phases.clone(),
            phases: definition
                .phases
                .iter()
                .map(|phase| PhaseDto {
                    phase: phase.id.clone(),
                    label: phase.label.clone(),
                    gates: phase.gates.clone(),
                    rejection_route: phase.rejection_route.clone(),
                })
                .collect(),
        }
    }
}

/// One stored work-profile revision, addressed by its open key and version.
///
/// The key is whatever the deployment called it. Nothing in this route — or in
/// the CLI and MCP surfaces above it — enumerates the legal values, because a
/// seeded profile pack is deployment data and a control plane that hard-coded its
/// names would refuse every profile a deployment added.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/profiles/{profile_key}/{version}", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("profile_key" = String, Path, description = "The open work-profile key"),
        ("version" = u32, Path, description = "The pinned revision")
    ),
    responses((status = 200, body = ViewDto<ProfileDto>), (status = 404))
)]
pub async fn profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, profile_key, version)): Path<(String, String, u32)>,
) -> Result<Json<ViewDto<ProfileDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let profile_key = parse_id(&state, WorkProfileKey::parse(&profile_key))?;
    let version = parse_id(&state, SpecVersion::parse(version))?;
    let stored = state
        .with_store(|store| store.get_work_profile(project_id, &profile_key, version))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    let stored = found(
        &state,
        stored,
        "no such work-profile revision exists in this project",
    )?;
    Ok(ViewDto::of(&state, ProfileDto::of(&stored)))
}

/// One recorded gate verdict, as evidence.
///
/// The evaluator's account and the reviewer principal are named because a gate
/// verdict is an accountability record; the guardrail evaluation is named by id
/// only, because its inputs are a stored document and not this route's to serve.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GateEvaluationDto {
    /// The gate.
    #[schema(value_type = String)]
    pub gate: GateKey,
    /// Position in that gate's append-only history, starting at 1.
    pub sequence: u32,
    /// The verdict recorded.
    #[schema(value_type = String)]
    pub verdict: String,
    /// The role that recorded it.
    #[schema(value_type = String)]
    pub evaluator_role: RoleKey,
    /// The account that recorded it.
    #[schema(value_type = String)]
    pub evaluator_account: AccountProfileId,
    /// The artifacts cited as evidence.
    #[schema(value_type = Vec<String>)]
    pub evidence: Vec<String>,
    /// The stable authenticated principal, when the row records one.
    #[schema(value_type = Option<String>)]
    pub reviewer_principal: Option<ExternalId>,
}

/// A task's active workflow, the gates reduced from its evaluations, and the
/// append-only evidence behind them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GateInspectionDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The active workflow, or `null` when the task has none.
    #[schema(value_type = Option<String>)]
    pub workflow_id: Option<String>,
    /// The phase the active workflow is in.
    #[schema(value_type = Option<String>)]
    pub current_phase: Option<PhaseKey>,
    /// The profile revision the workflow froze, as its phase and gate structure.
    pub profile: Option<ProfileDto>,
    /// The reduced gate states, keyed by gate.
    #[schema(value_type = Object)]
    pub gates: BTreeMap<GateKey, GateState>,
    /// Every recorded verdict, oldest first.
    pub evaluations: Vec<GateEvaluationDto>,
}

/// One task's gate states and the evidence they were reduced from.
///
/// A task with no active workflow answers with an empty inspection rather than a
/// refusal: the task exists, and "this task has no workflow yet" is a fact about
/// it rather than a missing row.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tasks/{task_id}/gates", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task")
    ),
    responses((status = 200, body = ViewDto<GateInspectionDto>), (status = 404))
)]
pub async fn gates(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
) -> Result<Json<ViewDto<GateInspectionDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let task_id = parse_id(&state, TaskId::parse(&task_id))?;
    let realm_id = state.realm_id();

    let inspection = state.with_store(|store| -> Result<_, ApiError> {
        let task = store
            .get_task(project_id, task_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        if task.is_none() {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "no such task exists in this project",
            ));
        }
        let workflow = store
            .get_active_task_workflow(project_id, task_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let Some(workflow) = workflow else {
            return Ok(GateInspectionDto {
                task_id,
                workflow_id: None,
                current_phase: None,
                profile: None,
                gates: BTreeMap::new(),
                evaluations: Vec::new(),
            });
        };
        let gates = store
            .gate_states(project_id, workflow.id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let evaluations = store
            .list_gate_evaluations(project_id, workflow.id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        Ok(GateInspectionDto {
            task_id,
            workflow_id: Some(workflow.id.to_string()),
            current_phase: Some(workflow.current_phase.clone()),
            profile: Some(ProfileDto::of(&workflow.snapshot.definition)),
            gates,
            evaluations: evaluations
                .iter()
                .map(|evaluation| GateEvaluationDto {
                    gate: evaluation.gate.clone(),
                    sequence: evaluation.sequence,
                    verdict: evaluation.verdict.to_string(),
                    evaluator_role: evaluation.evaluator_role.clone(),
                    evaluator_account: evaluation.evaluator_account,
                    evidence: evaluation
                        .evidence
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    reviewer_principal: evaluation.reviewer_principal.clone(),
                })
                .collect(),
        })
    })?;
    Ok(ViewDto::of(&state, inspection))
}

/// One step of a receipt's durable history.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReceiptTransitionDto {
    /// Position in the receipt's history, starting at 1.
    pub sequence: u32,
    /// The state the receipt moved to.
    #[schema(value_type = String)]
    pub state: String,
    /// When it moved.
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: Timestamp,
}

/// One command receipt and every state it has been through.
///
/// This is what makes an idempotent replay checkable rather than merely claimed:
/// a caller that retried a command can read the receipt its key recorded and see
/// that the history did not grow.
///
/// The stored correlation and native identity are deliberately absent, exactly as
/// they are from `ReceiptDto`: a correlation is the dispatcher's private handle on
/// a foreign system.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReceiptInspectionDto {
    /// The receipt.
    #[schema(value_type = String)]
    pub receipt_id: String,
    /// The project that owns it.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The caller's idempotency key.
    pub idempotency_key: String,
    /// What was asked for.
    #[schema(value_type = String)]
    pub kind: String,
    /// Which aggregate it targets.
    #[schema(value_type = Object)]
    pub target: kontor_core::receipt::AggregateRef,
    /// The revision the intent was computed against.
    #[schema(value_type = u64)]
    pub target_revision: AggregateRevision,
    /// How far it has got.
    #[schema(value_type = String)]
    pub state: String,
    /// How many dispatch attempts have been made.
    pub attempts: u32,
    /// Every state it has been through, oldest first.
    pub history: Vec<ReceiptTransitionDto>,
    /// When the intent was recorded.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    /// When it last changed.
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: Timestamp,
}

/// One command receipt, with its transition history.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/receipts/{receipt_id}", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("receipt_id" = String, Path, description = "The command receipt")
    ),
    responses((status = 200, body = ViewDto<ReceiptInspectionDto>), (status = 404))
)]
pub async fn receipt(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, receipt_id)): Path<(String, String)>,
) -> Result<Json<ViewDto<ReceiptInspectionDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let receipt_id = parse_id(&state, CommandReceiptId::parse(&receipt_id))?;
    let realm_id = state.realm_id();

    let inspection = state.with_store(|store| -> Result<_, ApiError> {
        let receipt = store
            .get_receipt(project_id, receipt_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let Some(receipt) = receipt else {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "no such command receipt exists in this project",
            ));
        };
        let history = store
            .receipt_history(project_id, receipt_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        Ok(ReceiptInspectionDto {
            receipt_id: receipt.id.to_string(),
            project_id: receipt.project_id,
            idempotency_key: receipt.idempotency_key.as_str().to_owned(),
            kind: receipt.kind.to_string(),
            target: receipt.target,
            target_revision: receipt.target_revision,
            state: receipt.state.to_string(),
            attempts: receipt.attempts,
            history: history
                .iter()
                .map(|transition| ReceiptTransitionDto {
                    sequence: transition.sequence,
                    state: transition.state.to_string(),
                    recorded_at: transition.recorded_at,
                })
                .collect(),
            created_at: receipt.created_at,
            updated_at: receipt.updated_at,
        })
    })?;
    Ok(ViewDto::of(&state, inspection))
}

/// One coding-account profile, as a policy reader sees it.
///
/// Three of the stored fields — the environment map, the routing metadata and the
/// declared capability document — are deliberately absent. Every one of them is
/// non-secret by construction, and every one of them is also the natural place a
/// deployment would eventually write an endpoint. A view that never carries them
/// cannot start carrying one by accident.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AccountDto {
    /// The profile.
    #[schema(value_type = String)]
    pub account_profile_id: AccountProfileId,
    /// The project that owns it.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// Its human label.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// The runtime family it authenticates against.
    #[schema(value_type = String)]
    pub harness: RuntimeKindKey,
    /// Which approved family the credential alias belongs to.
    #[schema(value_type = String)]
    pub credential_kind: String,
    /// The opaque approved alias. Never a credential, and never where one
    /// resolves to.
    #[schema(value_type = String)]
    pub credential_alias: String,
    /// The non-secret provider identity hint, when the deployment records one.
    #[schema(value_type = Option<String>)]
    pub provider_identity: Option<ExternalId>,
    /// The external account id it authenticates as, when it records one.
    #[schema(value_type = Option<String>)]
    pub external_account_id: Option<ExternalId>,
    /// Whether launches may select it.
    pub enabled: bool,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
}

impl AccountDto {
    /// Build the view of one stored profile.
    fn of(profile: &kontor_core::repository::AccountProfile) -> Self {
        Self {
            account_profile_id: profile.id,
            project_id: profile.project_id,
            label: profile.label.clone(),
            harness: profile.harness.clone(),
            credential_kind: profile.credential_ref.kind.as_str().to_owned(),
            credential_alias: profile.credential_ref.alias.as_str().to_owned(),
            provider_identity: profile.provider_identity.clone(),
            external_account_id: profile.external_account_id.clone(),
            enabled: profile.enabled,
            revision: profile.revision,
        }
    }
}

/// Every coding-account profile in one project.
///
/// Admin, not observer. The tier model puts credential and account routes with
/// the admin secret, and an account profile names the alias a resolver looks a
/// credential up under — so even though the alias is not itself a capability,
/// enumerating them is an account-authority read.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/accounts", tag = "query",
    params(("project_id" = String, Path, description = "The owning project")),
    responses((status = 200, body = ViewDto<Vec<AccountDto>>), (status = 403))
)]
pub async fn accounts(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ViewDto<Vec<AccountDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let listed = state
        .with_store(|store| store.list_account_profiles(project_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    Ok(ViewDto::of(
        &state,
        listed.iter().map(AccountDto::of).collect(),
    ))
}

/// One coding-account profile.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/accounts/{account_profile_id}", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("account_profile_id" = String, Path, description = "The account profile")
    ),
    responses((status = 200, body = crate::dto::SnapshotDto<AccountDto>), (status = 404))
)]
pub async fn account(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, account_profile_id)): Path<(String, String)>,
) -> Result<Json<crate::dto::SnapshotDto<AccountDto>>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let account_profile_id = parse_id(&state, AccountProfileId::parse(&account_profile_id))?;
    let snapshot = state
        .with_store(|store| store.snapshot_account_profile(project_id, account_profile_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    let cursor = snapshot.snapshot_cursor;
    // The store hands this one back inside a snapshot envelope, so — unlike the
    // views above — the position is one the value really was consistent with, and
    // it is quoted.
    let profile = snapshot
        .open(state.realm_id())
        .map_err(|error| ApiError::from_domain(state.realm_id(), &error))?;
    let profile = found(
        &state,
        profile,
        "no such account profile exists in this project",
    )?;
    Ok(Json(crate::dto::SnapshotDto {
        realm_id: state.realm_id(),
        snapshot_cursor: cursor,
        value: AccountDto::of(&profile),
    }))
}

/// What one configured runtime family reports about itself right now.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RuntimeDto {
    /// The runtime family. Never an endpoint.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// Whether the runtime answered the capability question at all.
    ///
    /// This is the health signal, and it is a fact about the channel: a family
    /// that could not be reached reports `false` here and no capabilities, rather
    /// than the capabilities it had last time.
    pub reachable: bool,
    /// How much of what it reports may be acted on, when it answered.
    #[schema(value_type = Option<String>)]
    pub trust_grade: Option<String>,
    /// The operations it currently declares, when it answered.
    #[schema(value_type = Vec<String>)]
    pub supported: Vec<String>,
    /// Whether it can prove which coding account a run executes as.
    pub account_env: Option<bool>,
    /// The largest message it declares it will take, in bytes.
    pub max_message_bytes: Option<u64>,
    /// The largest history page it declares.
    pub max_history_page: Option<u32>,
    /// How many concurrent sessions it declares.
    pub max_concurrent_sessions: Option<u32>,
}

/// Every configured runtime family, with what it declares right now.
///
/// A *freshly discovered* declaration, not a frozen one. That distinction is the
/// whole point of the route: a binding keeps answering with the capabilities it
/// was frozen at, and an operator asking "what can this runtime do today" is
/// asking a different question — one whose answer must never be written back onto
/// a binding.
#[utoipa::path(
    get, path = "/v1/runtimes", tag = "query",
    responses((status = 200, body = ViewDto<Vec<RuntimeDto>>), (status = 403))
)]
pub async fn runtimes(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<ViewDto<Vec<RuntimeDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let families: Vec<RuntimeKindKey> = state.runtimes().families().cloned().collect();
    let mut reported = Vec::with_capacity(families.len());
    for family in families {
        let Some(adapter) = state.runtimes().get(&family) else {
            continue;
        };
        let declared = adapter.discover_capabilities().await;
        reported.push(match declared {
            Err(_) => RuntimeDto {
                runtime_kind: family,
                reachable: false,
                trust_grade: None,
                supported: Vec::new(),
                account_env: None,
                max_message_bytes: None,
                max_history_page: None,
                max_concurrent_sessions: None,
            },
            Ok(capabilities) => RuntimeDto {
                runtime_kind: family,
                reachable: true,
                trust_grade: Some(format!("{:?}", capabilities.trust_grade).to_lowercase()),
                supported: capabilities
                    .supported
                    .iter()
                    .map(|capability| capability.as_str().to_owned())
                    .collect(),
                account_env: Some(capabilities.account_env),
                max_message_bytes: Some(capabilities.limits.max_message_bytes),
                max_history_page: Some(capabilities.limits.max_history_page),
                max_concurrent_sessions: Some(capabilities.limits.max_concurrent_sessions),
            },
        });
    }
    Ok(ViewDto::of(&state, reported))
}

/// One module held by one task.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModuleClaimDto {
    /// The module being contended for.
    #[schema(value_type = String)]
    pub module: ModuleKey,
    /// The task holding it.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The worktree it is isolated by, when it is.
    #[schema(value_type = Option<String>)]
    pub worktree: Option<ExternalName>,
    /// Whether the claim is still live.
    pub in_flight: bool,
}

/// What is currently held, and therefore what a scheduling pass would contend
/// with.
///
/// This is evidence and **not a plan**. A plan is `kontor_scheduler::plan`'s
/// answer over a `SchedulingSnapshot`, and that snapshot needs authorization,
/// calendar, fleet-preflight and external-work evidence this build has no read
/// path for. Serving contention under an honest name is the difference between
/// telling an operator what is known and inventing a decision.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ContentionDto {
    /// When the contention was read.
    #[schema(value_type = String, format = DateTime)]
    pub observed_at: Timestamp,
    /// Every live module claim in the Realm.
    pub module_claims: Vec<ModuleClaimDto>,
    /// Every worktree currently leased.
    #[schema(value_type = Vec<String>)]
    pub worktree_leases: Vec<ExternalName>,
    /// Every task with an open run.
    #[schema(value_type = Vec<String>)]
    pub tasks_with_open_runs: Vec<TaskId>,
}

/// The Realm's current scheduling contention.
#[utoipa::path(
    get, path = "/v1/scheduler/contention", tag = "query",
    responses((status = 200, body = ViewDto<ContentionDto>), (status = 403))
)]
pub async fn scheduler_contention(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<ViewDto<ContentionDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let observed_at = now();
    let realm_id = state.realm_id();
    let contention = state.with_store(|store| -> Result<_, ApiError> {
        let claims = store
            .active_module_claims(observed_at)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let worktrees = store
            .active_worktree_leases(observed_at)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let tasks = store
            .tasks_with_open_runs()
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        Ok(ContentionDto {
            observed_at,
            module_claims: claims
                .iter()
                .map(|claim| ModuleClaimDto {
                    module: claim.module.clone(),
                    task_id: claim.task_id,
                    worktree: claim.worktree.clone(),
                    in_flight: claim.in_flight,
                })
                .collect(),
            worktree_leases: worktrees.into_iter().collect(),
            tasks_with_open_runs: tasks.into_iter().collect(),
        })
    })?;
    Ok(ViewDto::of(&state, contention))
}

/// The status every route in this module answers a successful read with.
///
/// Named so the contract document and the tests quote one constant rather than
/// two copies of the number.
pub const OK: StatusCode = StatusCode::OK;
