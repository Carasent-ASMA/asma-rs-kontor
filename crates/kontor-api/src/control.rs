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
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Json, http::header::HeaderName};
use futures::stream::Stream;
use kontor_core::id::CommandReceiptId;
use kontor_core::id::{
    AgentRunId, CanonicalDocument, EventCursor, IdempotencyKey, ProjectId, TaskId,
};
use kontor_core::realm::{RealmCursor, ReceiptEnvelope};
use kontor_core::receipt::CommandKind;
use kontor_core::repository::{
    CommandRepository, NewCommandIntent, RealmRepository, RunInspection, TaskInspection,
};
use kontor_core::state::{DerivedRunState, Freshness};
use serde::Deserialize;

use crate::auth::CallerCapability;
use crate::dto::{
    BindingDto, EventDto, GapDto, HealthDto, ProjectionDto, RealmDto, ReceiptDto, ReceiptResponse,
    RunDto, SnapshotDto, TaskDto, run_revisions, task_revisions,
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

/// Which authority one command kind demands.
///
/// The split is by what the command *is authority over*, not by how disruptive it
/// looks: granting execution capability and approving or revoking a schedule
/// override are decisions about who may act at all, so they sit with the admin
/// tier alongside credentials. Everything else is ordinary operator work.
#[must_use]
pub const fn command_authority(kind: CommandKind) -> CallerCapability {
    match kind {
        CommandKind::AuthorizeExecution
        | CommandKind::ApproveScheduleOverride
        | CommandKind::RevokeScheduleOverride => CallerCapability::Admin,
        _ => CallerCapability::Operator,
    }
}

/// Record one control-plane command intent.
///
/// The kind, the target and the desired state are checked against the domain's own
/// compatibility matrix before anything is written, and the revision is checked
/// against what the aggregate currently stands at — so a caller working from a
/// stale read is told the current revision and nothing is mutated. The write
/// itself is `kontor-store`'s single transaction: intent, target row, outbox
/// entry, first durable transition, desired state and the intent event, or none of
/// them.
#[utoipa::path(
    post, path = "/v1/commands/{kind}", tag = "control",
    params(
        ("kind" = String, Path, description = "The command kind"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = crate::dto::CommandRequest,
    responses(
        (status = 200, body = ReceiptResponse, description = "Recorded, or replayed unchanged"),
        (status = 409, description = "Stale revision or a reused idempotency key")
    )
)]
pub async fn command(
    State(state): State<ApiState>,
    caller: Caller,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(request): Json<crate::dto::CommandRequest>,
) -> Result<Json<ReceiptResponse>, ApiError> {
    let kind = CommandKind::parse(&kind)
        .map_err(|_| state.refuse(ApiErrorCode::NotFound, "no such command kind exists"))?;
    caller.require(&state, command_authority(kind))?;
    let key = idempotency_key(&state, &headers)?;

    // The matrix decides whether this command may target that aggregate at all,
    // and whether it must carry a desired state. Refused before the revision is
    // read, so an incompatible pair never reaches a row.
    kind.ensure_compatible(&request.target, request.desired_state)
        .map_err(|error| ApiError::from_domain(state.realm_id(), &error))?;

    let intent = canonical(&state, &request.intent)?;
    let payload = canonical(&state, &request.payload)?;
    let recorded_at = now();

    let realm_id = state.realm_id();
    let outcome = state.with_store(|store| -> Result<(bool, _), ApiError> {
        // The current revision, read before anything is written. A caller working
        // from a stale read is answered with the number it needs and nothing is
        // mutated; the compare-and-swap inside the write is still what makes that
        // refusal safe under a race.
        let current = store
            .snapshot_target_revision(request.project_id, &request.target)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?
            .open(realm_id)
            .map_err(|error| ApiError::from_domain(realm_id, &error))?;
        let Some(current) = current else {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "the command target does not exist in this project",
            ));
        };
        if current != request.expected_revision {
            return Err(ApiError::new(
                realm_id,
                ApiErrorCode::RevisionConflict,
                "the target aggregate moved since the caller read it",
            )
            .with_revision(Some(current)));
        }

        // A replay is answered from the durable receipt and never re-recorded.
        //
        // Going through the write path instead would compare this call's
        // wall-clock earliest-dispatch instant against the stored one and refuse
        // an honest retry for differing by the seconds it took to arrive. The
        // durable record already says whether this is the same command, so that is
        // what decides — `ensure_replay` for the target and the intent, and the
        // kind and revision the store would otherwise have compared.
        if let Some(existing) = store
            .get_receipt_by_key(&key)
            .map_err(|error| ApiError::from_repository(realm_id, &error))?
        {
            let reused = ApiError::new(
                realm_id,
                ApiErrorCode::IdempotencyConflict,
                "the idempotency key was already used for a different command",
            );
            existing
                .ensure_replay(&request.target, &intent)
                .map_err(|_| reused.clone())?;
            if existing.kind != kind
                || existing.project_id != request.project_id
                || existing.target_revision != request.expected_revision
            {
                return Err(reused);
            }
            return Ok((true, existing));
        }
        let envelope = ReceiptEnvelope::new(
            realm_id,
            NewCommandIntent {
                project_id: request.project_id,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key.clone(),
                kind,
                target: request.target,
                target_revision: request.expected_revision,
                intent,
                payload,
                desired: request.desired_state,
                not_before: recorded_at,
                created_at: recorded_at,
            },
        );
        let receipt = store
            .record_intent_in_realm(&envelope)
            .map_err(|error| intent_refusal(realm_id, store, &request, &error))?;
        Ok((false, receipt))
    })?;

    // The intent event committed with the receipt, so the control-plane log moved
    // and every durable subscriber is owed a wake-up.
    state.signals().appended();
    let (replayed, receipt) = outcome;
    Ok(Json(ReceiptResponse {
        envelope: ReceiptEnvelope::new(state.realm_id(), ReceiptDto::from(&receipt)),
        replayed,
    }))
}

/// Turn a refused intent into the refusal the caller is owed.
///
/// The store merges "unknown target" and "its revision moved" into one conflict,
/// because from inside the write they are the same failed compare-and-swap. From
/// out here they are different answers, so the target is re-read to say which —
/// and a revision conflict carries the number the caller needs.
fn intent_refusal(
    realm_id: kontor_core::id::RealmId,
    store: &kontor_store::SqliteStore,
    request: &crate::dto::CommandRequest,
    error: &kontor_core::repository::RepositoryError,
) -> ApiError {
    use kontor_core::repository::RepositoryError;
    match error {
        RepositoryError::Domain(kontor_core::DomainError::Invalid { subject, .. })
            if *subject == "CommandReceipt" =>
        {
            ApiError::new(
                realm_id,
                ApiErrorCode::IdempotencyConflict,
                "the idempotency key was already used for a different command",
            )
        }
        RepositoryError::Conflict { .. } => {
            let current = store
                .snapshot_target_revision(request.project_id, &request.target)
                .ok()
                .and_then(|snapshot| snapshot.open(realm_id).ok())
                .flatten();
            match current {
                None => ApiError::new(
                    realm_id,
                    ApiErrorCode::NotFound,
                    "the command target does not exist in this project",
                ),
                Some(current) => ApiError::new(
                    realm_id,
                    ApiErrorCode::RevisionConflict,
                    "the target aggregate moved while the intent was being recorded",
                )
                .with_revision(Some(current)),
            }
        }
        other => ApiError::from_repository(realm_id, other),
    }
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

/// Canonicalize one caller-supplied document.
///
/// The document must carry its own `schema_version`, because that is what
/// [`CanonicalDocument`] is: a byte-frozen document with a declared generation and
/// a digest, not arbitrary JSON.
fn canonical(state: &ApiState, value: &serde_json::Value) -> Result<CanonicalDocument, ApiError> {
    CanonicalDocument::from_value(value)
        .map_err(|error| ApiError::from_domain(state.realm_id(), &error))
}

/// Build the wire view of one run inspection.
fn run_dto(state: &ApiState, inspection: &RunInspection) -> RunDto {
    let projection = &inspection.run.projection;
    let outcome = match projection.derived {
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
            derived: projection.derived.as_str().to_owned(),
            outcome,
            last_confirmed_at: projection.last_confirmed_at,
            // Freshness is a judgement about *now*, so it is computed here rather
            // than stored: the same row read a minute later is a staler answer.
            freshness: Freshness::evaluate(
                projection.last_confirmed_at,
                now(),
                jiff::SignedDuration::from_secs(state.evidence_window_seconds()),
            ),
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
