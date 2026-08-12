//! The KON-MVP-16 second amendment: the surfaces whose owning seams are merged.
//!
//! The first amendment ([`crate::query`]) added the thin reads a CLI cannot work
//! without. This one adds the four surfaces that needed a merged seam behind them,
//! and it is where the honest boundary of this build now sits.
//!
//! | Surface | Seam it needed | Where the work is |
//! | --- | --- | --- |
//! | scheduling explanation | `kontor-scheduler` (KON-MVP-09) | snapshot assembled here from stored rows + adapter evidence |
//! | external-ticket evidence | schema v1 ticket tables (KON-MVP-14) | read through `kontor_store::query` |
//! | project / mission / run lists | schema v1 | read through `kontor_store::query` |
//! | live session discovery | the runtime adapters (KON-MVP-11/12/13) | asked of the adapter, never of the store |
//!
//! # The scheduling explanation is assembled, and says so
//!
//! `kontor_scheduler::plan` is a pure function over a `SchedulingSnapshot`. Nothing
//! stored *is* that snapshot: it is assembled here, from tasks, workflows,
//! dependencies, authorizations and leases that are read, plus runtime evidence the
//! adapter reports. A handful of its fields have no source in schema v1 —
//! `kontor_store::query::scheduling_candidates` documents each one — and every one of
//! them is either neutral or fails closed. In particular an absent execution
//! authorization becomes `authorization_missing`, which is a real refusal and the
//! right one.
//!
//! A project with a work calendar assigned is answered rather than refused: the
//! assignment, its pinned profile revision, the applied exceptions and the live
//! overrides are read here and resolved by `kontor-calendar` (KON-MVP-21) against
//! *this* coordinator's clock, and the resolved answer travels with each candidate.
//! Nothing in this crate reads a window, a zone or a holiday.
//!
//! What this route will not do is admit anything: this is `plan`'s decision
//! *reported*, not committed. Committing an admission is
//! `kontor_store::admit_candidate`, behind the scheduler service, and no read route
//! reaches it.
//!
//! # Every blocker, not just the first
//!
//! `plan` reports one code per candidate on purpose: a decision has one reason. An
//! explanation has the opposite need, so each refused candidate here also carries
//! `kontor_scheduler::explain`'s full list. The two cannot disagree — `explain` asks
//! the same blockers in the same order — so the first entry of the list is always the
//! code the decision reports.
//!
//! # Ticket reads are evidence, never a live call
//!
//! Everything under `/v1/projects/{id}/tickets` is a row this Realm recorded: its own
//! projection, its own observations, the comments it mirrored inbound, the conflicts it
//! detected, the convergence attempts it made. No route here runs `asma`, and no route
//! here reaches an external system — the *writes* do that, through
//! `POST /v1/commands/{kind}`, behind the daemon's own dispatcher.
//!
//! There is no outbound comment anywhere: schema v1 has no table and no column for
//! one, so there is nothing to read and nothing to write.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use jiff::SignedDuration;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ExternalId, ExternalName, ProjectId, RoleKey,
    RuntimeKindKey, SpecVersion, TaskId, TeamRunId, TeamTemplateId, TicketLinkId, Timestamp,
};
use kontor_core::state::{DesiredRunState, ObservedRunState, RunLifecycle};
use kontor_runtime::capability::RuntimeCapabilities;
use kontor_scheduler::model::{
    AdaptiveWindow, AdaptiveWindowConfig, CandidateDecision, CapacityConfig, CapacityUsage,
    ReconciliationEvidence, ReconciliationScope, RuntimeAdmissionEvidence, RuntimeHealth,
    SchedulingSnapshot,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::CallerCapability;
use crate::control::parse_id;
use crate::error::{ApiError, ApiErrorCode};
use crate::query::ViewDto;
use crate::state::{ApiState, BarrierState};
use crate::{Caller, now};

/// How many rows a ticket history read returns when the caller names no bound.
const DEFAULT_TICKET_PAGE: u32 = 50;

/// The largest ticket history page a caller may ask for.
const MAX_TICKET_PAGE: u32 = 500;

/// How old a runtime confirmation may be and still count, for a planning pass.
///
/// The snapshot's `freshness` is a *validation* bound — it must be positive — and the
/// scheduler compares it against `last_confirmed_at`. The daemon's own evidence
/// window is the same judgement made in the same units, so it is what is used.
fn freshness(state: &ApiState) -> SignedDuration {
    SignedDuration::from_secs(state.evidence_window_seconds().max(1))
}

/// Refuse anything the addressed Realm does not hold.
fn found<T>(state: &ApiState, value: Option<T>, rule: &'static str) -> Result<T, ApiError> {
    value.ok_or_else(|| state.refuse(ApiErrorCode::NotFound, rule))
}

/// Read a caller's page bound, held to something a Realm will actually serve.
fn page(limit: Option<u32>) -> u32 {
    limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TICKET_PAGE)
        .min(MAX_TICKET_PAGE)
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

/// One project, as a list entry.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProjectEntryDto {
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

/// Every project in this Realm.
#[utoipa::path(
    get, path = "/v1/projects", tag = "query",
    responses((status = 200, body = ViewDto<Vec<ProjectEntryDto>>), (status = 403))
)]
pub async fn projects(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<ViewDto<Vec<ProjectEntryDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let listed = state
        .with_store(kontor_store::SqliteStore::list_projects)
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    Ok(ViewDto::of(
        &state,
        listed
            .into_iter()
            .map(|project| ProjectEntryDto {
                project_id: project.project_id,
                name: project.name,
                root_path: project.root_path,
                revision: project.revision,
                created_at: project.created_at,
            })
            .collect(),
    ))
}

/// One mission, as a list entry.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MissionEntryDto {
    /// The team run.
    #[schema(value_type = String)]
    pub team_run_id: TeamRunId,
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

/// Every mission in one project.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/team-runs", tag = "query",
    params(("project_id" = String, Path, description = "The owning project")),
    responses((status = 200, body = ViewDto<Vec<MissionEntryDto>>), (status = 404))
)]
pub async fn missions(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ViewDto<Vec<MissionEntryDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let listed = state
        .with_store(|store| store.list_team_runs(project_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    Ok(ViewDto::of(
        &state,
        listed
            .into_iter()
            .map(|run| MissionEntryDto {
                team_run_id: run.team_run_id,
                task_id: run.task_id,
                team_template: run.team_template,
                team_template_version: run.team_template_version,
                lifecycle: run.lifecycle,
                revision: run.revision,
                created_at: run.created_at,
                closed_at: run.closed_at,
            })
            .collect(),
    ))
}

/// Which runs a caller wants.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RunFilter {
    /// Only the runs of this team run.
    pub team_run: Option<String>,
}

/// One agent run, as a list entry.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunEntryDto {
    /// The agent run.
    #[schema(value_type = String)]
    pub agent_run_id: AgentRunId,
    /// The team run it serves.
    #[schema(value_type = String)]
    pub team_run_id: TeamRunId,
    /// The role slot it fills.
    #[schema(value_type = String)]
    pub role: RoleKey,
    /// The coding account it is pinned to, if any.
    #[schema(value_type = Option<String>)]
    pub account_profile_id: Option<AccountProfileId>,
    /// Its own lifecycle.
    #[schema(value_type = String)]
    pub lifecycle: RunLifecycle,
    /// What Kontor asked for.
    #[schema(value_type = String)]
    pub desired: DesiredRunState,
    /// What the runtime last reported.
    #[schema(value_type = String)]
    pub observed: ObservedRunState,
    /// What Kontor concluded.
    pub derived: String,
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

/// Every agent run in one project, optionally one mission's.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/runs", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("team_run" = Option<String>, Query, description = "Only this mission's runs")
    ),
    responses((status = 200, body = ViewDto<Vec<RunEntryDto>>), (status = 404))
)]
pub async fn runs(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    Query(filter): Query<RunFilter>,
) -> Result<Json<ViewDto<Vec<RunEntryDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let team_run = filter
        .team_run
        .as_deref()
        .map(|value| parse_id(&state, TeamRunId::parse(value)))
        .transpose()?;
    let listed = state
        .with_store(|store| store.list_agent_runs(project_id, team_run))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    Ok(ViewDto::of(
        &state,
        listed
            .into_iter()
            .map(|run| RunEntryDto {
                agent_run_id: run.agent_run_id,
                team_run_id: run.team_run_id,
                role: run.role,
                account_profile_id: run.account_profile_id,
                lifecycle: run.lifecycle,
                desired: run.desired,
                observed: run.observed,
                derived: run.derived,
                revision: run.revision,
                created_at: run.created_at,
                closed_at: run.closed_at,
            })
            .collect(),
    ))
}

// ---------------------------------------------------------------------------
// The scheduling explanation
// ---------------------------------------------------------------------------

/// One blocker's refusal of one candidate.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BlockerDto {
    /// Which blocker refused.
    pub blocker: String,
    /// The code it refused with.
    pub code: String,
    /// What it refused on, in the scheduler's own evidence shape.
    #[schema(value_type = Vec<Object>)]
    pub evidence: Vec<serde_json::Value>,
}

/// What the pass decided about one candidate.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DecisionDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// Whether the pass would admit it.
    pub admitted: bool,
    /// The one code the decision reports, for a refusal.
    pub code: Option<String>,
    /// Every blocker that refuses it, in evaluation order.
    ///
    /// The first entry is always the code above: `explain` asks the same blockers
    /// in the same order the decision does. Later entries are what an operator
    /// would hit next after fixing the first, which is why they are here.
    pub blockers: Vec<BlockerDto>,
    /// The authorization the pass would admit it under.
    #[schema(value_type = Option<String>)]
    pub authorization_id: Option<String>,
    /// The runtime family it would launch through.
    #[schema(value_type = Option<String>)]
    pub runtime_kind: Option<RuntimeKindKey>,
}

/// One planning pass, reported and not committed.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlanDto {
    /// The project the pass was taken for.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The instant every judgement was made against.
    #[schema(value_type = String, format = DateTime)]
    pub taken_at: Timestamp,
    /// How many tasks were looked at.
    pub considered: usize,
    /// How many the pass would admit.
    pub admitted: usize,
    /// Tasks with no active workflow, which are therefore not candidates.
    #[schema(value_type = Vec<String>)]
    pub without_workflow: Vec<TaskId>,
    /// Every decision, in the order the pass considered them.
    pub decisions: Vec<DecisionDto>,
    /// Facts about this pass a reader must not mistake for stored state.
    ///
    /// Names each field of the snapshot that had no source in schema v1 and the
    /// value it was assembled with, so an answer is never read as evidence of
    /// something nobody recorded.
    pub assembled_defaults: Vec<&'static str>,
}

/// Explain what a scheduling pass over one project would decide.
///
/// Nothing is admitted, queued, launched or leased. This is `kontor_scheduler::plan`
/// run over a snapshot assembled from stored rows, reported with every blocker.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/scheduler/plan", tag = "query",
    params(("project_id" = String, Path, description = "The project to plan")),
    responses(
        (status = 200, body = ViewDto<PlanDto>),
        (status = 404)
    )
)]
pub async fn scheduler_plan(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ViewDto<PlanDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let realm_id = state.realm_id();
    let taken_at = now();

    // The runtime half of the snapshot comes from the adapter, because a runtime's
    // capabilities and health are the adapter's to report and this crate does not
    // reach one directly.
    let runtime = runtime_evidence(&state, project_id).await?;

    let assembly = state.with_store(|store| -> Result<_, ApiError> {
        use kontor_core::repository::ProjectRepository;
        if store
            .get_project(project_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?
            .is_none()
        {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "no such project exists in this realm",
            ));
        }
        // `taken_at` is the coordinator's clock, and it is the only clock any
        // calendar in this answer is judged against. A caller's instant is never
        // accepted here, because a client that could choose the instant could
        // choose the window.
        store
            .scheduling_candidates_at(project_id, &runtime, taken_at)
            .map_err(|error| ApiError::from_repository(realm_id, &error))
    })?;

    let (in_flight, load, completed) = state.with_store(|store| -> Result<_, ApiError> {
        let in_flight = store
            .tasks_with_open_runs()
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let load = store
            .open_run_load()
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let completed = store
            .completed_task_ids(project_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        Ok((in_flight, load, completed))
    })?;
    let (claims, worktrees) = state.with_store(|store| -> Result<_, ApiError> {
        let claims = store
            .active_module_claims(taken_at)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let worktrees = store
            .active_worktree_leases(taken_at)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        Ok((claims, worktrees))
    })?;

    let capacity = CapacityConfig {
        global_max_in_flight: u32::MAX,
        project_max_in_flight: u32::MAX,
        mission_max_in_flight: u32::MAX,
        account_max_in_flight: u32::MAX,
        provider_max_in_flight: u32::MAX,
        runtime_max_in_flight: u32::MAX,
        adaptive: AdaptiveWindowConfig {
            initial: u32::MAX,
            floor: 1,
            ceiling: u32::MAX,
            growth_step: 1,
        },
    };
    let adaptive = AdaptiveWindowConfig {
        initial: u32::MAX,
        floor: 1,
        ceiling: u32::MAX,
        growth_step: 1,
    };
    let snapshot = SchedulingSnapshot {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        taken_at,
        candidates: assembly.candidates.clone(),
        in_flight_tasks: in_flight,
        completed_tasks: completed,
        module_leases: claims,
        worktree_leases: worktrees,
        usage: usage(&load),
        capacity,
        adaptive_window: AdaptiveWindow::start(adaptive),
        freshness: freshness(&state),
    };

    let plan = kontor_scheduler::plan(&snapshot)
        .map_err(|error| ApiError::from_domain(realm_id, &error))?;

    let mut decisions = Vec::with_capacity(plan.decisions.len());
    for decision in &plan.decisions {
        let candidate = assembly
            .candidates
            .iter()
            .find(|candidate| candidate.task_id == decision.task_id());
        // Every blocker, not only the one the decision names. `explain` asks the
        // same functions in the same order, so this cannot contradict the decision.
        let blockers = match candidate {
            None => Vec::new(),
            Some(candidate) => kontor_scheduler::explain(&snapshot, candidate)
                .map_err(|error| ApiError::from_domain(realm_id, &error))?
                .iter()
                .map(|refused| BlockerDto {
                    blocker: refused.blocker.as_str().to_owned(),
                    code: refused.code.as_str().to_owned(),
                    evidence: refused
                        .evidence
                        .iter()
                        .map(|item| serde_json::to_value(item).unwrap_or(serde_json::Value::Null))
                        .collect(),
                })
                .collect(),
        };
        decisions.push(match decision {
            CandidateDecision::Admit(admitted) => DecisionDto {
                task_id: admitted.task_id,
                admitted: true,
                code: None,
                blockers,
                authorization_id: Some(admitted.authorization_id.to_string()),
                runtime_kind: Some(admitted.runtime_kind.clone()),
            },
            CandidateDecision::Reject { task_id, code, .. } => DecisionDto {
                task_id: *task_id,
                admitted: false,
                code: Some(code.as_str().to_owned()),
                blockers,
                authorization_id: None,
                runtime_kind: None,
            },
        });
    }

    let admitted = decisions
        .iter()
        .filter(|decision| decision.admitted)
        .count();
    Ok(ViewDto::of(
        &state,
        PlanDto {
            project_id,
            taken_at,
            considered: assembly.considered,
            admitted,
            without_workflow: assembly.without_workflow,
            decisions,
            assembled_defaults: ASSEMBLED_DEFAULTS.to_vec(),
        },
    ))
}

/// Every snapshot field this build assembles rather than reads, and its value.
///
/// Served with the answer so a caller is never left to assume a default was
/// evidence. Each one is documented at
/// `kontor_store::query::SqliteStore::scheduling_candidates`.
pub const ASSEMBLED_DEFAULTS: &[&str] = &[
    "priority = 0 for every candidate (schema v1 has no tasks.priority column), so the order is \
     creation instant then task id",
    "serialization peers = none (schema v1 has no task_serializes_with table); module contention \
     is still read from live leases",
    "origin = manual (intake lineage is KON-MVP-22)",
    "account pin = none (a task has no pinned account until a run exists for it)",
    "external work evidence = none (live ticket ownership is read at convergence, not here)",
    "worktree claim = none (a candidate claims one at admission)",
    "capacity ceilings = unbounded, so no candidate is refused for capacity by this route",
];

/// Sum the open runs into the keyed counts a ceiling is stated against.
fn usage(load: &[kontor_store::query::OpenRunLoad]) -> CapacityUsage {
    let mut usage = CapacityUsage::default();
    for run in load {
        usage.global_in_flight = usage.global_in_flight.saturating_add(1);
        bump(&mut usage.project_in_flight, run.project_id);
        if let Some(account) = run.account_profile_id {
            bump(&mut usage.account_in_flight, account);
        }
    }
    usage
}

/// Add one to a keyed count.
fn bump<K: Ord>(counts: &mut std::collections::BTreeMap<K, u32>, key: K) {
    let entry = counts.entry(key).or_insert(0);
    *entry = entry.saturating_add(1);
}

/// The runtime evidence a planning pass is judged against.
///
/// One family answers for the pass: the first configured family that reports its
/// capabilities. A Realm with no configured fleet cannot be planned at all, because
/// every candidate's runtime evidence would be invented — so that is refused rather
/// than answered with a fabricated runtime.
async fn runtime_evidence(
    state: &ApiState,
    project_id: ProjectId,
) -> Result<RuntimeAdmissionEvidence, ApiError> {
    let families: Vec<RuntimeKindKey> = state.runtimes().families().cloned().collect();
    for family in families {
        let Some(adapter) = state.runtimes().get(&family) else {
            continue;
        };
        let Ok(capabilities) = adapter.discover_capabilities().await else {
            continue;
        };
        return Ok(evidence(state, project_id, family, capabilities));
    }
    Err(state.refuse(
        ApiErrorCode::Unavailable,
        "no configured runtime answered, so every candidate's runtime evidence would be invented",
    ))
}

/// Build the runtime half of a candidate from what the adapter reported.
fn evidence(
    state: &ApiState,
    project_id: ProjectId,
    family: RuntimeKindKey,
    capabilities: RuntimeCapabilities,
) -> RuntimeAdmissionEvidence {
    let barrier = state.barrier().state();
    let host = ExternalName::parse(family.as_str())
        .unwrap_or_else(|_| ExternalName::parse("runtime").expect("a constant name is valid"));
    RuntimeAdmissionEvidence {
        runtime_kind: family.clone(),
        // The family key doubles as the host label: this crate is not told an
        // endpoint, and a plan does not need one.
        host: host.clone(),
        generation: 1,
        capabilities,
        required: kontor_scheduler::minimum_launch_capabilities(),
        // Health is read from the barrier rather than guessed: a Realm whose
        // reconciliation did not finish has not proved its runtime is usable.
        health: match barrier {
            BarrierState::Open => RuntimeHealth::Healthy,
            BarrierState::Pending => RuntimeHealth::Degraded,
            BarrierState::Failed => RuntimeHealth::Unavailable,
        },
        reconciliation: ReconciliationEvidence {
            epoch_completed: barrier.is_open(),
            scope: ReconciliationScope {
                project_id,
                runtime_kind: family,
                host,
                generation: 1,
            },
            // Not "there is no gap": these say what the barrier proves, and a
            // barrier that opened is a census that completed and found none. A
            // barrier that did not open is reported as degraded above, which is
            // what refuses the candidate.
            open_replay_gap: !barrier.is_open(),
            divergence: false,
            orphan_ambiguity: false,
            stale_lost_contact: false,
        },
        // Never confirmed by this route: a planning read takes no observation, and
        // claiming a confirmation instant would be claiming an observation.
        last_confirmed_at: None,
    }
}

// ---------------------------------------------------------------------------
// External tickets
// ---------------------------------------------------------------------------

/// One external-ticket link, as a list entry.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TicketLinkDto {
    /// The link.
    #[schema(value_type = String)]
    pub link_id: TicketLinkId,
    /// The task it links.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The connector implementation.
    #[schema(value_type = String)]
    pub connector: ExternalName,
    /// The external issue key.
    #[schema(value_type = String)]
    pub external_issue_key: ExternalId,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// When the link was made.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
}

/// Every external-ticket link in one project.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tickets", tag = "query",
    params(("project_id" = String, Path, description = "The owning project")),
    responses((status = 200, body = ViewDto<Vec<TicketLinkDto>>), (status = 404))
)]
pub async fn tickets(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ViewDto<Vec<TicketLinkDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let listed = state
        .with_store(|store| store.list_ticket_links(project_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    Ok(ViewDto::of(
        &state,
        listed.into_iter().map(link_dto).collect(),
    ))
}

/// Build the wire view of one link.
fn link_dto(link: kontor_store::query::TicketLinkSummary) -> TicketLinkDto {
    TicketLinkDto {
        link_id: link.link_id,
        task_id: link.task_id,
        connector: link.connector,
        external_issue_key: link.external_issue_key,
        revision: link.revision,
        created_at: link.created_at,
    }
}

/// The projection this Realm computed for one ticket.
///
/// Published as `TicketProjectionDto` rather than under its Rust name. `utoipa`
/// keys the contract document by bare type name, and [`crate::dto::ProjectionDto`]
/// — the orthogonal state of a *run* — already holds `ProjectionDto` there. Two
/// registrations under one name do not collide loudly: the second silently
/// replaces the first, so `RunDto.projection` would resolve to this shape and
/// every generated client would be wrong about it while the wire stayed right.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[schema(as = TicketProjectionDto)]
pub struct ProjectionDto {
    /// The projection.
    #[schema(value_type = String)]
    pub projection_id: String,
    /// The link revision it was computed against.
    #[schema(value_type = u64)]
    pub link_revision: AggregateRevision,
    /// The pinned field specification's external project.
    #[schema(value_type = String)]
    pub field_spec_project: String,
    /// Its issue type.
    #[schema(value_type = String)]
    pub field_spec_issue_type: ExternalName,
    /// Its pinned revision.
    #[schema(value_type = u32)]
    pub field_spec_version: SpecVersion,
    /// The fields this Realm would write.
    #[schema(value_type = Object)]
    pub fields: serde_json::Value,
    /// The comment policy in force. Always `inbound_only`: schema v1 has no
    /// outbound comment table or column.
    pub comment_policy: String,
    /// How far inbound comments have been mirrored.
    #[schema(value_type = Option<String>)]
    pub external_comment_cursor: Option<ExternalId>,
    /// The digest of the projection.
    pub projection_hash: String,
    /// When it was computed.
    #[schema(value_type = String, format = DateTime)]
    pub computed_at: Timestamp,
}

/// One observation of the external ticket's own state.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ObservationDto {
    /// The observation.
    #[schema(value_type = String)]
    pub observation_id: ExternalId,
    /// The external status identifier.
    #[schema(value_type = String)]
    pub status_id: ExternalId,
    /// Its human name, as the external system spells it.
    #[schema(value_type = String)]
    pub status_name: ExternalName,
    /// Its category, as the external system spells it.
    #[schema(value_type = String)]
    pub status_category: ExternalName,
    /// The external issue type.
    #[schema(value_type = String)]
    pub issue_type: ExternalName,
    /// The assignee's external account, when the ticket has one.
    #[schema(value_type = Option<String>)]
    pub assignee_account_id: Option<ExternalId>,
    /// The assignee's display name, when the external system provided one.
    #[schema(value_type = Option<String>)]
    pub assignee_display: Option<ExternalName>,
    /// The external version token, when the external system issues one.
    #[schema(value_type = Option<String>)]
    pub external_version: Option<ExternalId>,
    /// When the observation was taken.
    #[schema(value_type = String, format = DateTime)]
    pub observed_at: Timestamp,
}

/// One detected conflict.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConflictDto {
    /// The conflict.
    #[schema(value_type = String)]
    pub conflict_id: String,
    /// What kind it is.
    pub kind: String,
    /// The observation it was detected against.
    #[schema(value_type = String)]
    pub observation_id: ExternalId,
    /// The internal milestone involved, when there is one.
    #[schema(value_type = Option<String>)]
    pub milestone: Option<ExternalName>,
    /// When it was detected.
    #[schema(value_type = String, format = DateTime)]
    pub detected_at: Timestamp,
    /// When it was resolved.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub resolved_at: Option<Timestamp>,
    /// The receipt that authorized the resolution.
    #[schema(value_type = Option<String>)]
    pub resolution_receipt_id: Option<String>,
}

/// One ticket, with the evidence this Realm holds about it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TicketDto {
    /// The link itself.
    pub link: TicketLinkDto,
    /// The newest projection, or `null` when none has been computed.
    pub projection: Option<ProjectionDto>,
    /// The newest observation, or `null` when the ticket has never been observed.
    ///
    /// This is where the *live* assignee and status appear, as last seen. It is an
    /// observation and never a claim about now.
    pub observed: Option<ObservationDto>,
    /// Every conflict, newest first.
    pub conflicts: Vec<ConflictDto>,
    /// How many of those are still unresolved.
    pub unresolved_conflicts: usize,
}

/// One ticket's stored evidence: the projection, the last observation, the
/// conflicts.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tickets/{link_id}", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("link_id" = String, Path, description = "The ticket link")
    ),
    responses((status = 200, body = ViewDto<TicketDto>), (status = 404))
)]
pub async fn ticket(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, link_id)): Path<(String, String)>,
) -> Result<Json<ViewDto<TicketDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let link_id = parse_id(&state, TicketLinkId::parse(&link_id))?;
    let realm_id = state.realm_id();

    let view = state.with_store(|store| -> Result<_, ApiError> {
        let link = store
            .get_ticket_link(project_id, link_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let Some(link) = link else {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "no such ticket link exists in this project",
            ));
        };
        let projection = store
            .latest_ticket_projection(project_id, link_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let observations = store
            .list_ticket_observations(project_id, link_id, 1)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let conflicts = store
            .list_ticket_conflicts(project_id, link_id)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?;
        let unresolved = conflicts
            .iter()
            .filter(|conflict| conflict.resolved_at.is_none())
            .count();
        Ok(TicketDto {
            link: link_dto(link),
            projection: projection.map(|projection| ProjectionDto {
                projection_id: projection.projection_id.to_string(),
                link_revision: projection.link_revision,
                field_spec_project: projection.field_spec_project.to_string(),
                field_spec_issue_type: projection.field_spec_issue_type,
                field_spec_version: projection.field_spec_version,
                // Served as the document it is rather than as a string, so a
                // caller does not have to parse a field out of a field.
                fields: serde_json::from_str(&projection.fields).unwrap_or(serde_json::Value::Null),
                comment_policy: projection.comment_policy,
                external_comment_cursor: projection.external_comment_cursor,
                projection_hash: projection.projection_hash,
                computed_at: projection.computed_at,
            }),
            observed: observations.into_iter().next().map(observation_dto),
            conflicts: conflicts
                .into_iter()
                .map(|conflict| ConflictDto {
                    conflict_id: conflict.conflict_id.to_string(),
                    kind: conflict.kind,
                    observation_id: conflict.observation_id,
                    milestone: conflict.milestone,
                    detected_at: conflict.detected_at,
                    resolved_at: conflict.resolved_at,
                    resolution_receipt_id: conflict
                        .resolution_receipt_id
                        .map(|receipt| receipt.to_string()),
                })
                .collect(),
            unresolved_conflicts: unresolved,
        })
    })?;
    Ok(ViewDto::of(&state, view))
}

/// Build the wire view of one observation.
fn observation_dto(observation: kontor_store::query::TicketObservation) -> ObservationDto {
    ObservationDto {
        observation_id: observation.observation_id,
        status_id: observation.status_id,
        status_name: observation.status_name,
        status_category: observation.status_category,
        issue_type: observation.issue_type,
        assignee_account_id: observation.assignee_account_id,
        assignee_display: observation.assignee_display,
        external_version: observation.external_version,
        observed_at: observation.observed_at,
    }
}

/// How much of a ticket's history a caller wants.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HistoryQuery {
    /// How many rows at most.
    pub limit: Option<u32>,
}

/// One inbound comment.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CommentDto {
    /// The external system's own comment id.
    #[schema(value_type = String)]
    pub external_comment_id: ExternalId,
    /// The digest of the body, which is half this revision's identity.
    pub body_hash: String,
    /// The author's external account.
    #[schema(value_type = String)]
    pub author_account_id: ExternalId,
    /// The author's display name, when the external system provided one.
    #[schema(value_type = Option<String>)]
    pub author_display: Option<ExternalName>,
    /// When it was created externally.
    #[schema(value_type = String, format = DateTime)]
    pub external_created_at: Timestamp,
    /// When it was last edited externally.
    #[schema(value_type = String, format = DateTime)]
    pub external_updated_at: Timestamp,
    /// When this Realm mirrored it.
    #[schema(value_type = String, format = DateTime)]
    pub observed_at: Timestamp,
    /// The revision this one replaces, for an edit.
    pub supersedes_hash: Option<String>,
    /// The comment text, as mirrored.
    pub body: String,
}

/// One ticket's inbound comments, newest first.
///
/// Inbound only, and structurally so: there is no outbound comment table in this
/// schema and no route that writes one.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tickets/{link_id}/comments", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("link_id" = String, Path, description = "The ticket link"),
        ("limit" = Option<u32>, Query, description = "How many rows at most")
    ),
    responses((status = 200, body = ViewDto<Vec<CommentDto>>), (status = 404))
)]
pub async fn ticket_comments(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, link_id)): Path<(String, String)>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ViewDto<Vec<CommentDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let link_id = parse_id(&state, TicketLinkId::parse(&link_id))?;
    let realm_id = state.realm_id();
    let listed = state.with_store(|store| -> Result<_, ApiError> {
        ensure_link(store, realm_id, project_id, link_id)?;
        store
            .list_inbound_comments(project_id, link_id, page(query.limit))
            .map_err(|error| ApiError::from_repository(realm_id, &error))
    })?;
    Ok(ViewDto::of(
        &state,
        listed
            .into_iter()
            .map(|comment| CommentDto {
                external_comment_id: comment.external_comment_id,
                body_hash: comment.body_hash,
                author_account_id: comment.author_account_id,
                author_display: comment.author_display,
                external_created_at: comment.external_created_at,
                external_updated_at: comment.external_updated_at,
                observed_at: comment.observed_at,
                supersedes_hash: comment.supersedes_hash,
                body: comment.body,
            })
            .collect(),
    ))
}

/// One convergence attempt.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TransitionDto {
    /// The receipt.
    #[schema(value_type = String)]
    pub receipt_id: String,
    /// The task whose state was being projected.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The internal milestone being converged to.
    #[schema(value_type = String)]
    pub milestone: ExternalName,
    /// The external status it aimed at.
    #[schema(value_type = String)]
    pub target_status_id: ExternalId,
    /// The external transition used. `null` for an assignee-only convergence.
    #[schema(value_type = Option<String>)]
    pub transition_id: Option<ExternalId>,
    /// The external principal it acted as.
    #[schema(value_type = String)]
    pub principal_account_id: ExternalId,
    /// Whether an assignment had to happen first.
    pub assignment_prerequisite: bool,
    /// When it was dispatched.
    #[schema(value_type = String, format = DateTime)]
    pub dispatched_at: Timestamp,
    /// When the external system acknowledged it.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub acknowledged_at: Option<Timestamp>,
    /// When a *refetched* observation confirmed it.
    ///
    /// `null` means unconfirmed, which is never the same as failed: an
    /// acknowledgement is not a confirmation, and this column is only written when
    /// the external system was read again.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub confirmed_at: Option<Timestamp>,
    /// The observation that confirmed it.
    #[schema(value_type = Option<String>)]
    pub refetched_observation_id: Option<ExternalId>,
}

/// One ticket's convergence attempts, newest first.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tickets/{link_id}/transitions", tag = "query",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("link_id" = String, Path, description = "The ticket link"),
        ("limit" = Option<u32>, Query, description = "How many rows at most")
    ),
    responses((status = 200, body = ViewDto<Vec<TransitionDto>>), (status = 404))
)]
pub async fn ticket_transitions(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, link_id)): Path<(String, String)>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ViewDto<Vec<TransitionDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let link_id = parse_id(&state, TicketLinkId::parse(&link_id))?;
    let realm_id = state.realm_id();
    let listed = state.with_store(|store| -> Result<_, ApiError> {
        ensure_link(store, realm_id, project_id, link_id)?;
        store
            .list_ticket_transitions(project_id, link_id, page(query.limit))
            .map_err(|error| ApiError::from_repository(realm_id, &error))
    })?;
    Ok(ViewDto::of(
        &state,
        listed
            .into_iter()
            .map(|attempt| TransitionDto {
                receipt_id: attempt.receipt_id.to_string(),
                task_id: attempt.task_id,
                milestone: attempt.milestone,
                target_status_id: attempt.target_status_id,
                transition_id: attempt.transition_id,
                principal_account_id: attempt.principal_account_id,
                assignment_prerequisite: attempt.assignment_prerequisite,
                dispatched_at: attempt.dispatched_at,
                acknowledged_at: attempt.acknowledged_at,
                confirmed_at: attempt.confirmed_at,
                refetched_observation_id: attempt.refetched_observation_id,
            })
            .collect(),
    ))
}

/// Refuse a history read for a link this project does not hold.
///
/// Without it an unknown link would answer with an empty history, which reads as
/// "this ticket has never been touched" — a different and wrong statement.
fn ensure_link(
    store: &kontor_store::SqliteStore,
    realm_id: kontor_core::id::RealmId,
    project_id: ProjectId,
    link_id: TicketLinkId,
) -> Result<(), ApiError> {
    let link = store
        .get_ticket_link(project_id, link_id)
        .map_err(|error| ApiError::from_repository(realm_id, &error))?;
    if link.is_none() {
        return Err(ApiError::new(
            realm_id,
            ApiErrorCode::NotFound,
            "no such ticket link exists in this project",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Live session discovery
// ---------------------------------------------------------------------------

/// One native session a runtime currently owns.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NativeSessionDto {
    /// The runtime family that owns it.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The runtime's own session id. Correlation evidence, never an identity.
    #[schema(value_type = String)]
    pub native_id: ExternalId,
    /// The host label the runtime reports for itself.
    #[schema(value_type = String)]
    pub host: ExternalName,
    /// The runtime generation it belongs to.
    pub generation: u64,
    /// Whether this Realm already holds a binding for it.
    ///
    /// The point of a discovery read: a session the Realm does not know about is an
    /// adoption candidate, and one it does know about is not.
    pub bound: bool,
}

/// The sessions one runtime family currently owns.
///
/// A read of the runtime, not of this Realm's log — so it is the one place an
/// operator can see a session that exists natively and has no binding here.
///
/// Adoption itself is deliberately absent: binding a native session to an agent run
/// creates a run, a binding and a frozen capability snapshot in one transaction, and
/// `CommandKind` has no variant that records that intent. Adding one is a
/// `kontor-core` change with its own compatibility matrix entry, which is a ticket
/// and not a route. Until then a discovered session is reported and nothing here
/// claims it.
#[utoipa::path(
    get, path = "/v1/runtimes/{runtime_kind}/sessions", tag = "query",
    params(("runtime_kind" = String, Path, description = "The runtime family to ask")),
    responses(
        (status = 200, body = ViewDto<Vec<NativeSessionDto>>),
        (status = 404, description = "This daemon is not configured with that runtime"),
        (status = 422, description = "That runtime never declared discovery")
    )
)]
pub async fn runtime_sessions(
    State(state): State<ApiState>,
    caller: Caller,
    Path(runtime_kind): Path<String>,
) -> Result<Json<ViewDto<Vec<NativeSessionDto>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let family = parse_id(&state, RuntimeKindKey::parse(&runtime_kind))?;
    let realm_id = state.realm_id();
    let adapter = found(
        &state,
        state.runtimes().get(&family),
        "this daemon is not configured with that runtime family",
    )?;

    // Discovery is a declared capability, and a runtime that never declared it is
    // answered with the contract's own code rather than with an empty list. An
    // empty list would say "this runtime owns no sessions", which is not known.
    let declared = adapter
        .discover_capabilities()
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    if !declared.supports(kontor_runtime::capability::RuntimeCapability::Discovery) {
        return Err(state.refuse(
            ApiErrorCode::UnsupportedCapability,
            "this runtime never declared session discovery",
        ));
    }

    let sessions = adapter
        .discover_sessions()
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    let bound: BTreeSet<ExternalId> = state
        .with_store(kontor_store::SqliteStore::open_bindings)
        .map_err(|error| ApiError::from_repository(realm_id, &error))?
        .into_iter()
        .map(|open| open.binding.identity.native_id)
        .collect();

    Ok(ViewDto::of(
        &state,
        sessions
            .into_iter()
            .map(|session| NativeSessionDto {
                bound: bound.contains(&session.identity.native_id),
                runtime_kind: session.identity.runtime_kind.clone(),
                native_id: session.identity.native_id.clone(),
                host: session.identity.host.clone(),
                generation: session.identity.generation,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_bound_is_defaulted_and_capped() {
        assert_eq!(page(None), DEFAULT_TICKET_PAGE);
        assert_eq!(page(Some(0)), DEFAULT_TICKET_PAGE, "zero is not a page");
        assert_eq!(page(Some(10)), 10);
        assert_eq!(
            page(Some(100_000)),
            MAX_TICKET_PAGE,
            "a caller cannot ask a realm to render its whole history at once"
        );
    }

    #[test]
    fn every_assembled_default_is_named_for_a_reader() {
        // The list is served with the answer, so an operator is never left to guess
        // whether a value was read or supplied.
        assert!(
            ASSEMBLED_DEFAULTS.len() >= 7,
            "each snapshot field with no schema v1 source must be named"
        );
        for note in ASSEMBLED_DEFAULTS {
            assert!(
                note.contains('=') || note.contains("none"),
                "a default must state the value it was assembled with: {note}"
            );
        }
    }
}
