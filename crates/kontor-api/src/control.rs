//! Liveness, identity, snapshots, control-plane commands and the durable event
//! feed.
//!
//! Everything here works in the *control-plane* cursor space and nothing here
//! touches a runtime. The two spaces are never mixed: a position from
//! `/v1/events` is meaningless to `/v1/sessions/…/timeline` and the reverse, and
//! they are spelled differently so a caller cannot pass one where the other
//! belongs.

use std::collections::VecDeque;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::HeaderName;
use axum::response::sse::{Event, KeepAlive, Sse};

use crate::body::Json;
use futures::stream::Stream;
use kontor_core::id::{AgentRunId, EventCursor, IdempotencyKey, ProjectId, TaskId};
use kontor_core::realm::RealmCursor;
use kontor_core::repository::{RealmRepository, RunInspection, TaskInspection};
use kontor_core::state::{DerivedRunState, Freshness};
use serde::Deserialize;

use crate::auth::CallerCapability;
use crate::dto::{
    BindingDto, ContextPolicyDto, EventDto, GapDto, HealthDto, ProjectionDto, RealmDto, RunDto,
    SnapshotDto, TaskDto, run_revisions, task_revisions,
};
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;
use crate::{Caller, now};

/// How many events one read of the durable feed takes at a time.
///
/// A bound rather than a tuning knob: it keeps one page's memory predictable, and
/// the feed simply reads again when a page comes back full.
const FEED_PAGE: u32 = 256;

/// The header a caller resumes the durable feed with, per the SSE specification.
const LAST_EVENT_ID: HeaderName = HeaderName::from_static("last-event-id");

/// Liveness, Realm identity and the startup barrier.
#[utoipa::path(
    get, path = "/v1/health", tag = "control",
    responses((status = 200, body = HealthDto), (status = 401), (status = 403))
)]
pub async fn health(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<HealthDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let schema_version = state
        .with_store(|store| store.schema_version())
        .map_err(|_| {
            state.refuse(
                ApiErrorCode::Unavailable,
                "the control-plane store could not report its schema version",
            )
        })?;
    let barrier = state.barrier().state();
    Ok(Json(HealthDto {
        realm_id: state.realm_id(),
        live: true,
        schema_version,
        reconciliation: barrier,
        scheduling_open: barrier.is_open(),
        runtimes: state.runtimes().families().cloned().collect(),
    }))
}

/// This Realm's immutable identity.
#[utoipa::path(
    get, path = "/v1/realm", tag = "control",
    responses((status = 200, body = RealmDto), (status = 401), (status = 403))
)]
pub async fn realm(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<RealmDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let realm = state.realm();
    Ok(Json(RealmDto {
        realm_id: realm.realm_id,
        schema_version: realm.schema_version,
        created_at: realm.created_at,
        display_label: realm.display_label.clone(),
    }))
}

/// One agent run, its projection, its pinned revisions and its recorded gaps.
#[utoipa::path(
    get, path = "/v1/runs/{agent_run_id}", tag = "control",
    params(("agent_run_id" = String, Path, description = "The Kontor agent run")),
    responses((status = 200, body = SnapshotDto<RunDto>), (status = 404))
)]
pub async fn run_snapshot(
    State(state): State<ApiState>,
    caller: Caller,
    Path(agent_run_id): Path<String>,
) -> Result<Json<SnapshotDto<RunDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let agent_run_id = parse_id(&state, AgentRunId::parse(&agent_run_id))?;
    let snapshot = state
        .with_store(|store| store.snapshot_run_inspection(agent_run_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    let cursor = snapshot.snapshot_cursor;
    let inspection = snapshot
        .open(state.realm_id())
        .map_err(|error| ApiError::from_domain(state.realm_id(), &error))?
        .ok_or_else(|| {
            state.refuse(
                ApiErrorCode::NotFound,
                "no such agent run exists in this realm",
            )
        })?;
    Ok(Json(SnapshotDto {
        realm_id: state.realm_id(),
        snapshot_cursor: cursor,
        value: run_dto(&state, &inspection),
    }))
}

/// One task, its active workflow, its gate states and its pinned revisions.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tasks/{task_id}", tag = "control",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task")
    ),
    responses((status = 200, body = SnapshotDto<TaskDto>), (status = 404))
)]
pub async fn task_snapshot(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
) -> Result<Json<SnapshotDto<TaskDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let task_id = parse_id(&state, TaskId::parse(&task_id))?;
    let snapshot = state
        .with_store(|store| store.snapshot_task_inspection(project_id, task_id))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    let cursor = snapshot.snapshot_cursor;
    let inspection = snapshot
        .open(state.realm_id())
        .map_err(|error| ApiError::from_domain(state.realm_id(), &error))?
        .ok_or_else(|| {
            state.refuse(
                ApiErrorCode::NotFound,
                "no such task exists in this project",
            )
        })?;
    Ok(Json(SnapshotDto {
        realm_id: state.realm_id(),
        snapshot_cursor: cursor,
        value: task_dto(&inspection),
    }))
}

/// Where a durable subscriber wants to resume.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct FeedQuery {
    /// The control-plane position already seen. Delivery starts strictly after it.
    pub after: Option<i64>,
}

/// The durable control-plane feed.
///
/// Three properties make it restartable. Every frame's SSE `id` is the event's
/// own persisted control-plane cursor, so `Last-Event-ID` is a position this
/// Realm actually allocated. Delivery is strictly greater than the requested
/// position, so nothing is repeated and nothing is skipped. And the events come
/// from SQLite rather than from memory, so a reconnect after a restart reads the
/// same log the previous connection was reading.
#[utoipa::path(
    get, path = "/v1/events", tag = "control",
    params(("after" = Option<i64>, Query, description = "Resume strictly after this cursor")),
    responses(
        (status = 200, description = "An SSE stream of control-plane events"),
        (status = 410, description = "The position is outside the retained history")
    )
)]
pub async fn events(
    State(state): State<ApiState>,
    caller: Caller,
    Query(query): Query<FeedQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let resume = resume_position(&state, &query, &headers)?;

    // The window and the first page are read together, so "outside the retained
    // history" is decided against the same moment the first frame comes from.
    let first = state
        .with_store(|store| store.realm_event_page(resume, FEED_PAGE))
        .map_err(|error| ApiError::from_repository(state.realm_id(), &error))?;
    if let Some(resume) = resume {
        let position = resume
            .resolve(state.realm_id())
            .map_err(|error| ApiError::from_domain(state.realm_id(), &error))?;
        // Beyond the newest allocated position, or behind the oldest retained
        // one: either way the caller is describing a log this Realm does not
        // have, and the honest answer is to snapshot again.
        if position > first.newest.cursor || position.get() < first.oldest_retained.cursor.get() - 1
        {
            return Err(ApiError::resnapshot(
                state.realm_id(),
                (first.oldest_retained, first.newest),
            ));
        }
    }

    let realm_id = state.realm_id();
    let mut cursor = resume.map(|position| position.cursor);
    let buffered: VecDeque<Event> = first
        .events
        .iter()
        .map(|envelope| frame(realm_id, envelope))
        .collect::<Result<_, _>>()?;
    if let Some(last) = first.events.last() {
        cursor = Some(last.cursor);
    }

    let stream = futures::stream::unfold(
        FeedState {
            state,
            cursor,
            buffered,
            appends: None,
            stops: None,
        },
        |mut feed| async move {
            loop {
                if let Some(event) = feed.buffered.pop_front() {
                    return Some((Ok(event), feed));
                }
                // Subscribe *before* the last read is repeated, so an append that
                // lands between the read and the wait is not lost.
                let mut appends = feed
                    .appends
                    .take()
                    .unwrap_or_else(|| feed.state.signals().appends());
                let mut stops = feed
                    .stops
                    .take()
                    .unwrap_or_else(|| feed.state.signals().stops());
                let page = feed
                    .state
                    .with_store(|store| {
                        store.realm_event_page(
                            feed.cursor
                                .map(|cursor| RealmCursor::new(feed.state.realm_id(), cursor)),
                            FEED_PAGE,
                        )
                    })
                    .ok()?;
                if !page.events.is_empty() {
                    let realm_id = feed.state.realm_id();
                    for envelope in &page.events {
                        feed.buffered.push_back(frame(realm_id, envelope).ok()?);
                    }
                    feed.cursor = page.events.last().map(|event| event.cursor);
                    feed.appends = Some(appends);
                    feed.stops = Some(stops);
                    continue;
                }
                // Caught up. A graceful stop ends the stream at this boundary
                // rather than tearing a frame in half.
                if *stops.borrow_and_update() {
                    return None;
                }
                tokio::select! {
                    changed = appends.changed() => {
                        if changed.is_err() {
                            return None;
                        }
                    }
                    changed = stops.changed() => {
                        if changed.is_err() || *stops.borrow() {
                            return None;
                        }
                    }
                }
                feed.appends = Some(appends);
                feed.stops = Some(stops);
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// What one open durable feed is holding between frames.
struct FeedState {
    state: ApiState,
    cursor: Option<EventCursor>,
    buffered: VecDeque<Event>,
    appends: Option<tokio::sync::watch::Receiver<u64>>,
    stops: Option<tokio::sync::watch::Receiver<bool>>,
}

/// One event, as an SSE frame whose `id` is its own persisted cursor.
fn frame(
    realm_id: kontor_core::id::RealmId,
    envelope: &kontor_core::realm::EventEnvelope<kontor_core::repository::RuntimeEvent>,
) -> Result<Event, ApiError> {
    let event = envelope
        .peek(realm_id)
        .map_err(|error| ApiError::from_domain(realm_id, &error))?;
    Event::default()
        .id(envelope.cursor.get().to_string())
        .event("control")
        .json_data(EventDto::of(realm_id, event))
        .map_err(|_| {
            ApiError::new(
                realm_id,
                ApiErrorCode::Unavailable,
                "a stored control-plane event could not be serialized",
            )
        })
}

/// Reconcile `?after=` with `Last-Event-ID`.
///
/// A caller may present either. Presenting both and disagreeing is refused rather
/// than resolved by precedence: one of the two is a bug in the caller, and
/// silently picking a winner would decide which of its two beliefs about its own
/// position to honour.
fn resume_position(
    state: &ApiState,
    query: &FeedQuery,
    headers: &HeaderMap,
) -> Result<Option<RealmCursor>, ApiError> {
    let header = headers
        .get(LAST_EVENT_ID)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let header = match header {
        None => None,
        Some(value) => Some(value.parse::<i64>().map_err(|_| {
            state.refuse(
                ApiErrorCode::InvalidRequest,
                "Last-Event-ID is not a control-plane cursor this realm issued",
            )
        })?),
    };
    let position = match (query.after, header) {
        (None, None) => return Ok(None),
        (Some(after), None) | (None, Some(after)) => after,
        (Some(after), Some(header)) if after == header => after,
        (Some(_), Some(_)) => {
            return Err(state.refuse(
                ApiErrorCode::InvalidRequest,
                "?after and Last-Event-ID name different positions",
            ));
        }
    };
    let cursor = EventCursor::parse(position)
        .map_err(|error| ApiError::from_domain(state.realm_id(), &error))?;
    Ok(Some(RealmCursor::new(state.realm_id(), cursor)))
}

/// The header every mutation carries its idempotency token in.
///
/// One spelling, shared by the handlers, the CORS allowlist and the contract
/// document, so none of the three can drift from the other two.
pub const IDEMPOTENCY_KEY: HeaderName = HeaderName::from_static("idempotency-key");

/// The `Idempotency-Key` a mutation must carry.
pub fn idempotency_key(state: &ApiState, headers: &HeaderMap) -> Result<IdempotencyKey, ApiError> {
    let value = headers
        .get(IDEMPOTENCY_KEY)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            state.refuse(
                ApiErrorCode::InvalidRequest,
                "every mutation must carry an Idempotency-Key header",
            )
        })?;
    IdempotencyKey::parse(value).map_err(|error| ApiError::from_domain(state.realm_id(), &error))
}

/// Parse a caller-supplied identifier.
pub fn parse_id<T>(state: &ApiState, parsed: kontor_core::DomainResult<T>) -> Result<T, ApiError> {
    parsed.map_err(|_| {
        state.refuse(
            ApiErrorCode::InvalidRequest,
            "the identifier is not in canonical form",
        )
    })
}

/// Resolve a context-window policy from explicit inputs, and change nothing.
///
/// A pure read: it touches no store, dispatches to no runtime and persists
/// nothing, so asking "what would this seat get" is free of consequence. The
/// same inputs always produce the same answer, which is what makes it worth
/// asking before a run exists.
///
/// It returns the *same* [`ContextPolicyDto`] a run carries, so a preview and
/// the thing it previewed cannot describe a policy differently.
#[utoipa::path(
    post, path = "/v1/context-policy/preview", tag = "control",
    request_body = crate::dto::ContextPolicyPreviewRequest,
    responses(
        (status = 200, body = ContextPolicyDto, description = "The policy those inputs resolve to"),
        (status = 422, description = "A value outside the closed set, or a seed reaching an explicit-only class")
    )
)]
pub async fn context_policy_preview(
    State(state): State<ApiState>,
    caller: Caller,
    Json(request): Json<crate::dto::ContextPolicyPreviewRequest>,
) -> Result<Json<ContextPolicyDto>, ApiError> {
    // A read, so reader authority is enough. Nothing here can change anything.
    caller.require(&state, crate::auth::CallerCapability::Observer)?;
    let realm_id = state.realm_id();
    let domain = |error: &kontor_core::DomainError| ApiError::from_domain(realm_id, error);

    let parse = |declared: Option<&crate::dto::ContextWindowPolicyDto>| {
        declared
            .map(crate::dto::ContextWindowPolicyDto::parse)
            .transpose()
    };
    let run_override = parse(request.run_override.as_ref()).map_err(|e| domain(&e))?;
    let role_slot = parse(request.role_slot.as_ref()).map_err(|e| domain(&e))?;
    let work_profile = parse(request.work_profile.as_ref()).map_err(|e| domain(&e))?;
    let role_seed = parse(request.role_seed.as_ref()).map_err(|e| domain(&e))?;

    // The same resolver a launch uses. A preview that reimplemented precedence
    // would be a second answer that could disagree with the real one.
    let resolved =
        kontor_core::spec::resolve_context_window(&kontor_core::spec::ContextPolicyInputs {
            run_override: run_override.as_ref(),
            role_slot: role_slot.as_ref(),
            work_profile: work_profile.as_ref(),
            role_seed: role_seed.as_ref(),
        })
        .map_err(|e| domain(&e))?;

    let requested =
        kontor_core::spec::RequestedContextPolicy::of(&resolved, kontor_core::id::SCHEMA_VERSION);
    let effective = kontor_core::spec::EffectiveContextPolicy::derive(
        &requested,
        &kontor_core::spec::ContextWindowBounds {
            safe_ceiling_tokens: request.safe_ceiling,
            minimum_trigger_tokens: request.minimum_trigger,
        },
        request.context_policy_capable,
    )
    .map_err(|e| domain(&e))?;
    let snapshot = kontor_core::spec::ContextPolicySnapshot::freeze(requested, effective, now())
        .map_err(|e| domain(&e))?;

    // No receipt, because nothing happened.
    Ok(Json(ContextPolicyDto::of(&snapshot, None)))
}

/// Build the wire view of one run inspection.
fn run_dto(state: &ApiState, inspection: &RunInspection) -> RunDto {
    let projection = &inspection.run.projection;
    let freshness = Freshness::evaluate(
        projection.last_confirmed_at,
        now(),
        jiff::SignedDuration::from_secs(state.evidence_window_seconds()),
    );
    let derived = projection.derived.at_read_time(freshness);
    let outcome = match derived {
        DerivedRunState::Terminal { outcome } => Some(outcome),
        _ => None,
    };
    RunDto {
        agent_run_id: inspection.run.id,
        project_id: inspection.project_id,
        team_run_id: inspection.run.team_run_id,
        parent_agent_run_id: inspection.run.parent_agent_run_id,
        role: inspection.run.role.clone(),
        account_profile_id: inspection.run.account_profile_id,
        context_policy: inspection
            .context_policy
            .as_ref()
            .map(|snapshot| ContextPolicyDto::of(snapshot, inspection.latest_compaction.as_ref())),
        binding: inspection.run.binding.as_ref().map(|binding| BindingDto {
            binding_id: binding.id,
            runtime_kind: binding.identity.runtime_kind.clone(),
            host: binding.identity.host.clone(),
            generation: binding.identity.generation,
            native_id: binding.identity.native_id.clone(),
            bound_at: binding.bound_at,
            attached: state.sessions().get(binding.id).is_some(),
        }),
        projection: ProjectionDto {
            lifecycle: projection.lifecycle,
            desired: projection.desired,
            observed: projection.observed,
            derived: derived.as_str().to_owned(),
            outcome,
            last_confirmed_at: projection.last_confirmed_at,
            // Freshness is a judgement about *now*, so it is computed here rather
            // than stored: the same row read a minute later is a staler answer.
            freshness,
            last_cursor: projection.last_cursor,
        },
        revision: inspection.run.revision,
        applied: run_revisions(inspection),
        gaps: inspection.gaps.iter().map(GapDto::from).collect(),
        created_at: inspection.run.created_at,
        closed_at: inspection.run.closed_at,
    }
}

/// Build the wire view of one task inspection.
fn task_dto(inspection: &TaskInspection) -> TaskDto {
    TaskDto {
        task_id: inspection.task.id,
        project_id: inspection.task.project_id,
        title: inspection.task.title.clone(),
        state: inspection.task.state,
        revision: inspection.task.revision,
        current_phase: inspection
            .workflow
            .as_ref()
            .map(|workflow| workflow.current_phase.clone()),
        gates: inspection.gates.clone(),
        applied: task_revisions(inspection),
        updated_at: inspection.task.updated_at,
    }
}
