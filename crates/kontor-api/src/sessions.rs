//! Session content: the runtime's own transcript, live stream, messages and
//! permission answers.
//!
//! Everything here works in the *runtime's* cursor space — a content epoch and a
//! sequence inside it — and never in the control-plane one. A `/v1/events`
//! position is not accepted here and an anchor from here is not accepted there.
//!
//! # The order every route resolves in
//!
//! 1. authenticate the Realm (the middleware, before any handler);
//! 2. authorize the caller's tier;
//! 3. load the Kontor run *in this Realm*, which also resolves its project;
//! 4. resolve the persisted binding — no binding is not "no content", it is a run
//!    that was never launched;
//! 5. select the adapter for the binding's runtime family;
//! 6. ask that adapter to vouch for the binding;
//! 7. run the binding's **frozen** capability preflight;
//! 8. and only then dispatch.
//!
//! Steps 6 and 7 are what make "an unsupported operation has zero runtime effect"
//! true here and not only inside the adapter: the refusal happens in this process,
//! before a request is built.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::body::Json;
use crate::dto::{CompactRequestBody, CompactionReceiptDto};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use kontor_core::compaction::CompactionTrigger;
use kontor_core::id::ContentHash;
use kontor_core::id::{AgentRunId, BoundedText, ExternalId, RealmId};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::repository::RealmRepository;
use kontor_core::repository::RunRepository as _;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::capability::{
    LimitDemand, OperationContext, RuntimeBindingSnapshot, RuntimeCapability, preflight,
};
use kontor_runtime::request::CompactRequest;
use kontor_runtime::request::{
    HistoryRequest, LiveSubscribeRequest, MessageId, PermissionResponseRequest, SendMessageRequest,
};
use kontor_runtime::timeline::{EventSubject, HistoryCursor, HistoryReader, SessionEventKind};
use serde::Deserialize;

use crate::auth::CallerCapability;
use crate::control::{idempotency_key, parse_id};
use crate::dto::{
    MessageAckDto, MessageRequest, PermissionAckDto, PermissionRequestBody, StreamFrameDto,
    StreamRefusalDto, TimelineDto,
};
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;
use crate::{Caller, now};

/// Every kind of session content, so a subscription cannot look complete by
/// filtering away what it failed to deliver.
const EVERY_KIND: &[SessionEventKind] = &[
    SessionEventKind::Message,
    SessionEventKind::ToolCall,
    SessionEventKind::PermissionRequest,
    SessionEventKind::PermissionResolved,
    SessionEventKind::StateChange,
    SessionEventKind::Log,
];

/// The default timeline page when a caller names none.
const DEFAULT_PAGE: u32 = 50;

/// The SSE event name a broken timeline is reported under.
const REFUSAL_EVENT: &str = "error";

/// One resolved, vouched-for session this process may address.
struct Session {
    agent_run_id: AgentRunId,
    /// The project the run resolved into, so a later write is scoped to the
    /// same place the read came from rather than to a caller-supplied id.
    project_id: kontor_core::id::ProjectId,
    snapshot: RuntimeBindingSnapshot,
    adapter: Arc<dyn RuntimeAdapter>,
    /// The immutable context window this seat was launched under, when one was
    /// frozen. Read here so a compaction need not re-query for it.
    context_policy: Option<kontor_core::spec::ContextPolicySnapshot>,
}

impl Session {
    /// Run the binding's frozen capability preflight for one operation.
    ///
    /// The frozen snapshot is passed as the discovered set as well, because
    /// `OperationContext::effective` reads a bound operation's capabilities off the
    /// binding either way — writing it out makes it impossible for a later edit to
    /// slip fresh discovery in here.
    ///
    /// `autonomous` is false: every route in this module relays an explicit
    /// operator decision, which is precisely the case the trust rule exempts.
    fn preflight(
        &self,
        realm_id: RealmId,
        operation: RuntimeCapability,
        demand: Option<LimitDemand>,
    ) -> Result<(), ApiError> {
        let mut context = OperationContext::new(operation);
        context.autonomous = false;
        context.binding = Some(&self.snapshot);
        context.demand = demand;
        preflight(&self.snapshot.capabilities, &context)
            .map_err(|error| ApiError::from_runtime(realm_id, &error))
    }
}

/// Resolve `{id}` into a session this process may act on.
async fn resolve(
    state: &ApiState,
    agent_run_id: &str,
    required: CallerCapability,
    caller: Caller,
) -> Result<Session, ApiError> {
    caller.require(state, required)?;
    let agent_run_id = parse_id(state, AgentRunId::parse(agent_run_id))?;
    let realm_id = state.realm_id();

    let snapshot = state
        .with_store(|store| store.snapshot_run_inspection(agent_run_id))
        .map_err(|error| ApiError::from_repository(realm_id, &error))?;
    let inspection = snapshot
        .open(realm_id)
        .map_err(|error| ApiError::from_domain(realm_id, &error))?
        .ok_or_else(|| {
            ApiError::new(
                realm_id,
                ApiErrorCode::NotFound,
                "no such agent run exists in this realm",
            )
        })?;
    // A run with no binding has no session. That is not an empty transcript.
    let binding = inspection.run.binding.as_ref().ok_or_else(|| {
        ApiError::new(
            realm_id,
            ApiErrorCode::NotFound,
            "this run has never been bound to a native session",
        )
    })?;
    let adapter = state
        .runtimes()
        .get(&binding.identity.runtime_kind)
        .ok_or_else(|| {
            ApiError::new(
                realm_id,
                ApiErrorCode::Unavailable,
                "this daemon is not configured with the runtime that owns the session",
            )
        })?;
    // The frozen snapshot lives in this process, not in SQLite. Its absence means
    // this process cannot address the session at the evidence quality it was bound
    // at, and rebuilding one from fresh discovery would be exactly the re-grading
    // the freeze rule forbids.
    let held = state.sessions().get(binding.id).ok_or_else(|| {
        ApiError::new(
            realm_id,
            ApiErrorCode::StaleBinding,
            "this process holds no frozen capability snapshot for the session",
        )
    })?;
    // The runtime's own copy is the one that counts. A snapshot it never issued —
    // or one that differs in any field from what it issued — vouches for nothing.
    let issued = adapter
        .issued_binding(&held)
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    Ok(Session {
        agent_run_id,
        project_id: inspection.project_id,
        snapshot: issued.snapshot().clone(),
        adapter,
        context_policy: inspection.context_policy.clone(),
    })
}

/// Where a caller wants to continue reading a session's content.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TimelineQuery {
    /// The runtime continuation cursor a previous page returned.
    pub after: Option<String>,
    /// How many items to return at most.
    pub limit: Option<u32>,
}

/// One page of a session's recorded content, read from the runtime.
///
/// The page is validated through `HistoryReader` before it is returned, so what a
/// caller receives is exactly-once by construction: a redelivered item is dropped
/// from the page rather than merely uncounted, and a page that changes epoch,
/// skips a sequence or rewrites a position it already delivered is refused.
#[utoipa::path(
    get, path = "/v1/sessions/{agent_run_id}/timeline", tag = "sessions",
    params(
        ("agent_run_id" = String, Path, description = "The Kontor agent run"),
        ("after" = Option<String>, Query, description = "A runtime continuation cursor"),
        ("limit" = Option<u32>, Query, description = "Maximum items")
    ),
    responses(
        (status = 200, body = TimelineDto),
        (status = 409, description = "The timeline must be refetched from the start"),
        (status = 422, description = "This runtime cannot replay content")
    )
)]
pub async fn timeline(
    State(state): State<ApiState>,
    caller: Caller,
    Path(agent_run_id): Path<String>,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<TimelineDto>, ApiError> {
    let session = resolve(&state, &agent_run_id, CallerCapability::Observer, caller).await?;
    let realm_id = state.realm_id();
    let page_size = query.limit.unwrap_or(DEFAULT_PAGE);
    session.preflight(
        realm_id,
        RuntimeCapability::History,
        Some(LimitDemand::HistoryPage(page_size)),
    )?;

    let cursor = query.after.as_deref().map(HistoryCursor::from_text);
    // The cursor is resolved against this binding *before* dispatch, so a cursor
    // issued for another session is refused without the runtime being asked. It
    // doubles as the position the page must be validated from — read from the
    // caller's own claim rather than from the page, because deriving it from the
    // page's first item is exactly what would make a missing start invisible.
    let resume = cursor
        .as_ref()
        .map(|cursor| cursor.resolve(session.snapshot.binding_id()))
        .transpose()
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    let mut page = session
        .adapter
        .history(&HistoryRequest {
            binding: session.snapshot.clone(),
            cursor,
            page_size,
        })
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    let mut reader = match resume {
        None => HistoryReader::start(session.snapshot.binding_id(), page.epoch),
        Some(position) => HistoryReader::resuming(session.snapshot.binding_id(), position),
    };
    reader
        .accept_page(&mut page)
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    Ok(Json(TimelineDto::of(
        realm_id,
        session.agent_run_id,
        session.snapshot.binding_id(),
        &page,
        reader.anchor(),
    )))
}

/// Where a live subscription must start.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamQuery {
    /// The anchor a timeline read ended at. Delivery starts strictly after it.
    pub after: Option<String>,
}

/// Follow a session's content strictly after a validated timeline anchor.
///
/// The anchor is required, and that is the transport spelling of "history anchors
/// live": without a position a previous read validated, there is nothing for
/// delivery to be strictly after, and a stream that guessed would be unable to
/// tell a runtime dropping events from its own missing start.
#[utoipa::path(
    get, path = "/v1/sessions/{agent_run_id}/stream", tag = "sessions",
    params(
        ("agent_run_id" = String, Path, description = "The Kontor agent run"),
        ("after" = String, Query, description = "The anchor a timeline read returned")
    ),
    responses(
        (status = 200, description = "An SSE stream of session content"),
        (status = 400, description = "No anchor was presented"),
        (status = 422, description = "This runtime cannot stream content")
    )
)]
pub async fn stream(
    State(state): State<ApiState>,
    caller: Caller,
    Path(agent_run_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let session = resolve(&state, &agent_run_id, CallerCapability::Observer, caller).await?;
    let realm_id = state.realm_id();
    session.preflight(realm_id, RuntimeCapability::LiveEvents, None)?;

    let anchor = query.after.as_deref().ok_or_else(|| {
        ApiError::new(
            realm_id,
            ApiErrorCode::InvalidRequest,
            "a live subscription starts strictly after the anchor a timeline read returned",
        )
    })?;
    let strict_after = HistoryCursor::from_text(anchor)
        .resolve(session.snapshot.binding_id())
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    let subscription = session
        .adapter
        .subscribe_live(&LiveSubscribeRequest {
            binding: session.snapshot.clone(),
            kinds: EVERY_KIND.iter().copied().collect::<BTreeSet<_>>(),
            strict_after,
        })
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    let stream = futures::stream::unfold(
        Some((subscription, realm_id, session.agent_run_id)),
        |held| async move {
            let (mut subscription, realm_id, agent_run_id) = held?;
            match subscription.next_event() {
                None => None,
                Some(Ok(event)) => {
                    // The frame id is the *runtime's* position, spelled
                    // `epoch:sequence` so it can never be mistaken for — or
                    // replayed as — a control-plane cursor.
                    let frame = Event::default()
                        .id(event.position.to_string())
                        .event("content")
                        .json_data(StreamFrameDto {
                            realm_id,
                            agent_run_id,
                            item: crate::dto::TimelineItemDto::from(&event),
                        })
                        .ok()?;
                    Some((Ok(frame), Some((subscription, realm_id, agent_run_id))))
                }
                // A broken timeline ends the stream with a typed frame. Continuing
                // would hand the caller a hole it cannot see.
                Some(Err(_)) => {
                    let refusal = Event::default()
                        .event(REFUSAL_EVENT)
                        .json_data(StreamRefusalDto {
                            realm_id,
                            code: "timeline_refetch_required",
                            rule: "the runtime renumbered or skipped this session's content",
                        })
                        .ok()?;
                    Some((Ok(refusal), None))
                }
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Deliver one message into a session.
///
/// The `Idempotency-Key` *is* the stable client message id: it must parse as one,
/// and it is what the runtime's own ledger keys the effect on. Requiring two
/// separate tokens would create two things that can disagree about whether a
/// retry is the same message, and only one of them would be the one the runtime
/// actually checks.
#[utoipa::path(
    post, path = "/v1/sessions/{agent_run_id}/messages", tag = "sessions",
    params(
        ("agent_run_id" = String, Path, description = "The Kontor agent run"),
        ("Idempotency-Key" = String, Header, description = "The stable client message id")
    ),
    request_body = MessageRequest,
    responses(
        (status = 200, body = MessageAckDto, description = "Delivered, or the original ack replayed"),
        (status = 409, description = "The id already committed different content"),
        (status = 422, description = "This runtime cannot take messages")
    )
)]
pub async fn send_message(
    State(state): State<ApiState>,
    caller: Caller,
    Path(agent_run_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<MessageRequest>,
) -> Result<Json<ReceiptEnvelope<MessageAckDto>>, ApiError> {
    let session = resolve(&state, &agent_run_id, CallerCapability::Operator, caller).await?;
    let realm_id = state.realm_id();
    let message_id = message_identifier(&state, &headers)?;
    let body = BoundedText::parse(&request.body)
        .map_err(|error| ApiError::from_domain(realm_id, &error))?;
    let demand = LimitDemand::MessageBytes(body.as_str().len() as u64);
    session.preflight(realm_id, RuntimeCapability::SendMessage, Some(demand))?;

    let acknowledged = session
        .adapter
        .send(&SendMessageRequest {
            binding: session.snapshot.clone(),
            message_id,
            body,
            sent_at: now(),
        })
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    Ok(Json(ReceiptEnvelope::new(
        realm_id,
        MessageAckDto::from(&acknowledged),
    )))
}

/// Answer one permission request raised inside a session.
#[utoipa::path(
    post, path = "/v1/sessions/{agent_run_id}/permissions/{request_id}", tag = "sessions",
    params(
        ("agent_run_id" = String, Path, description = "The Kontor agent run"),
        ("request_id" = String, Path, description = "The runtime's own permission request id"),
        ("Idempotency-Key" = String, Header, description = "The stable client response id")
    ),
    request_body = PermissionRequestBody,
    responses(
        (status = 200, body = PermissionAckDto, description = "Applied, or the original ack replayed"),
        (status = 409, description = "The request was already answered differently"),
        (status = 422, description = "This runtime cannot take permission answers")
    )
)]
pub async fn respond_permission(
    State(state): State<ApiState>,
    caller: Caller,
    Path((agent_run_id, request_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<PermissionRequestBody>,
) -> Result<Json<ReceiptEnvelope<PermissionAckDto>>, ApiError> {
    let session = resolve(&state, &agent_run_id, CallerCapability::Operator, caller).await?;
    let realm_id = state.realm_id();
    let response_id = message_identifier(&state, &headers)?;
    let permission_id =
        ExternalId::parse(&request_id).map_err(|error| ApiError::from_domain(realm_id, &error))?;
    session.preflight(realm_id, RuntimeCapability::PermissionResponse, None)?;

    // Prove the request was raised by *this* session before answering it. The
    // runtime's own ledger refuses a foreign request too, but a permission answer
    // is an authorization decision at a trust boundary, and this process does not
    // outsource those. When the runtime cannot replay content there is nothing
    // here to read, and the runtime's refusal is the only available check.
    if session
        .snapshot
        .capabilities
        .supports(RuntimeCapability::History)
    {
        ensure_raised_here(&state, &session, &permission_id).await?;
    }

    let acknowledged = session
        .adapter
        .respond_permission(&PermissionResponseRequest {
            binding: session.snapshot.clone(),
            permission_id,
            response_id,
            decision: request.decision,
            responded_at: now(),
        })
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    Ok(Json(ReceiptEnvelope::new(
        realm_id,
        PermissionAckDto::from(&acknowledged),
    )))
}

/// Refuse a permission id this session's own content never raised.
///
/// The question is *raised here*, not *still open*. An already-answered request
/// has to reach the runtime: an identical retry is owed its original
/// acknowledgement and a contradictory one is owed a typed conflict, and a check
/// that refused everything already resolved would turn both of those into "no
/// such request".
async fn ensure_raised_here(
    state: &ApiState,
    session: &Session,
    permission_id: &ExternalId,
) -> Result<(), ApiError> {
    let realm_id = state.realm_id();
    let mut cursor: Option<HistoryCursor> = None;
    let mut raised = BTreeSet::new();
    loop {
        let page = session
            .adapter
            .history(&HistoryRequest {
                binding: session.snapshot.clone(),
                cursor: cursor.clone(),
                page_size: DEFAULT_PAGE,
            })
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        raised.extend(
            page.items
                .iter()
                .filter_map(|item| match (&item.kind, &item.subject) {
                    (SessionEventKind::PermissionRequest, EventSubject::Permission(id)) => {
                        Some(id.clone())
                    }
                    _ => None,
                }),
        );
        cursor = page.next.clone();
        if cursor.is_none() {
            break;
        }
    }
    if raised.contains(permission_id) {
        return Ok(());
    }
    Err(ApiError::new(
        realm_id,
        ApiErrorCode::NotFound,
        "this session's content raises no such permission request",
    ))
}

/// The `Idempotency-Key`, read as the Kontor message identifier it has to be.
fn message_identifier(state: &ApiState, headers: &HeaderMap) -> Result<MessageId, ApiError> {
    let key = idempotency_key(state, headers)?;
    MessageId::parse(key.as_str()).map_err(|_| {
        state.refuse(
            ApiErrorCode::InvalidRequest,
            "a session Idempotency-Key is the client's stable message id: a canonical UUID v7",
        )
    })
}

/// The `Idempotency-Key` a compaction is keyed on, which *is* the receipt id.
fn compaction_identifier(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<kontor_core::id::CompactionReceiptId, ApiError> {
    let key = idempotency_key(state, headers)?;
    kontor_core::id::CompactionReceiptId::parse(key.as_str()).map_err(|_| {
        state.refuse(
            ApiErrorCode::InvalidRequest,
            "a compaction Idempotency-Key is the receipt id: a canonical UUID v7",
        )
    })
}

/// Ask one seat to compact its context, in place.
///
/// Every guard the approved policy names runs **before** the adapter is
/// reached, and each of them refuses without an effect:
///
/// * realm authorization and a frozen binding, from [`resolve`];
/// * the runtime's frozen [`RuntimeCapability::Compact`] capability, so a
///   `required` policy on a runtime that cannot compact refuses here rather
///   than being reported as done;
/// * a *deterministic* trigger — threshold, durable scope boundary or an
///   authorized operator request. A finished role turn is not one, and there is
///   no spelling for it in [`CompactionTrigger`];
/// * no active tool action and no unresolved permission, because compacting
///   mid-decision discards the decision;
/// * a sealed durable handoff for a boundary or operator compaction, plus the
///   Context Pack hash the run was frozen against.
///
/// The `Idempotency-Key` *is* the compaction receipt id, for the same reason it
/// is the message id on `send_message`: two tokens could disagree about whether
/// a retry is the same attempt, and only one of them is what the ledger keys on.
#[utoipa::path(
    post, path = "/v1/sessions/{agent_run_id}/compact", tag = "sessions",
    params(
        ("agent_run_id" = String, Path, description = "The Kontor agent run"),
        ("Idempotency-Key" = String, Header, description = "The stable compaction receipt id")
    ),
    request_body = CompactRequestBody,
    responses(
        (status = 200, body = CompactionReceiptDto, description = "The outcome, or the original receipt replayed"),
        (status = 409, description = "The id already recorded a different attempt"),
        (status = 422, description = "An unsafe trigger or a missing handoff")
    )
)]
pub async fn compact(
    State(state): State<ApiState>,
    caller: Caller,
    Path(agent_run_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CompactRequestBody>,
) -> Result<Json<ReceiptEnvelope<CompactionReceiptDto>>, ApiError> {
    let session = resolve(&state, &agent_run_id, CallerCapability::Operator, caller).await?;
    let realm_id = state.realm_id();
    let domain = |error: &kontor_core::DomainError| ApiError::from_domain(realm_id, error);

    let trigger: CompactionTrigger = serde_json::from_value(serde_json::Value::String(
        request.trigger.clone(),
    ))
    .map_err(|_| {
        state.refuse(
            ApiErrorCode::InvalidRequest,
            "a compaction names threshold, scope_boundary or operator; a finished turn is not a trigger",
        )
    })?;

    // A tool action or a permission nobody answered is work in flight. Throwing
    // away the context around it is how a seat forgets what it was doing.
    if request.active_tool || request.unresolved_permission {
        return Err(state.refuse(
            ApiErrorCode::UnsupportedCapability,
            "a session with an active tool action or an unresolved permission is not at a safe point",
        ));
    }

    // The sealed-handoff guard, at the command surface and before anything is
    // looked up. It needs only the trigger and the hash, so it refuses the
    // cheapest and most fundamental case first — and long before the adapter.
    if trigger.requires_durable_handoff() && request.handoff_hash.is_none() {
        return Err(state.refuse(
            ApiErrorCode::UnsupportedCapability,
            "a boundary or operator compaction requires a sealed durable handoff",
        ));
    }

    let receipt_id = compaction_identifier(&state, &headers)?;
    let context_pack_hash =
        ContentHash::parse(&request.context_pack_hash).map_err(|e| domain(&e))?;
    let handoff_hash = request
        .handoff_hash
        .as_deref()
        .map(ContentHash::parse)
        .transpose()
        .map_err(|e| domain(&e))?;

    let policy = session.context_policy.clone().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::NotFound,
            "this run has no frozen context policy to compact under",
        )
    })?;

    let compaction = CompactRequest {
        binding: session.snapshot.clone(),
        receipt_id,
        trigger,
        policy,
        context_pack_hash,
        handoff_hash,
        requested_at: now(),
    };
    // The sealed-handoff guard, at the command surface. It refuses before the
    // adapter is called at all, so a boundary compaction that would drop
    // unrecorded work state never reaches a runtime.
    compaction
        .validate()
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    let receipt = session
        .adapter
        .compact(&compaction)
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

    // Persisted through the same immutable, id-keyed path everything else uses:
    // an identical replay returns the original, and a reused id carrying a
    // different attempt is a conflict rather than an overwrite.
    let stored = state
        .with_store(|store| store.record_compaction_receipt(session.project_id, &receipt))
        .map_err(|error| ApiError::from_repository(realm_id, &error))?;

    Ok(Json(ReceiptEnvelope::new(
        realm_id,
        CompactionReceiptDto::of(&stored),
    )))
}
