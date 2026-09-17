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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

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
    HistoryRequest, InspectRequest, LiveSubscribeRequest, MessageId, PermissionResponseRequest,
    ResumeRequest, SendMessageRequest,
};
use kontor_runtime::timeline::{
    EventSubject, HistoryCursor, HistoryReader, SessionEventKind, TimelinePosition,
};
use serde::Deserialize;

use crate::auth::CallerCapability;
use crate::control::{idempotency_key, parse_id};
use crate::dto::{
    MessageAckDto, MessageRequest, ObservedTurnDto, PermissionAckDto, PermissionRequestBody,
    StreamFrameDto, StreamRefusalDto, TimelineDto,
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

/// How often a quiet runtime subscription is reconciled for newly pushed data.
///
/// The adapter closes the history/live race and returns a bounded batch. Keeping
/// the HTTP response live therefore means taking another bounded batch after its
/// last trusted position; it never means guessing that an empty batch is a
/// terminal session.
const LIVE_RECONCILE_INTERVAL: Duration = Duration::from_millis(250);

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

/// How many canonical pages one observation will walk before giving up.
///
/// Bounded for the same reason settlement's scan is: a cursor-free read of a
/// long-lived seat can outlive its own request. A caller that hits the budget is
/// told so and hands back the anchor it reached, which is what `after` is for.
const OBSERVE_PAGE_BUDGET: usize = 64;

/// How many times `wanted` appears at or before `through`, read from the origin.
///
/// Exists only because `after` lets the main scan start late. It is the *same*
/// canonical read, over the window that scan skipped, counting one id — not a
/// second opinion about what the current turn is. Nothing here decides anything:
/// it returns a count, and the caller refuses on it.
///
/// Fails closed. A prefix that cannot be read to `through` inside the budget
/// leaves uniqueness unproven, and an unproven uniqueness must not be reported
/// as a clean turn.
async fn prefix_occurrences(
    session: &Session,
    realm_id: RealmId,
    page_size: u32,
    through: TimelinePosition,
    wanted: MessageId,
) -> Result<usize, ApiError> {
    let mut cursor = None;
    let mut reader: Option<HistoryReader> = None;
    let mut found = 0usize;
    for _ in 0..OBSERVE_PAGE_BUDGET {
        let mut page = session
            .adapter
            .history(&HistoryRequest {
                binding: session.snapshot.clone(),
                cursor,
                page_size,
            })
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        let reader = reader
            .get_or_insert_with(|| HistoryReader::start(session.snapshot.binding_id(), page.epoch));
        reader
            .accept_page(&mut page)
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        for event in &page.items {
            if event.position.sequence > through.sequence {
                break;
            }
            if let EventSubject::Message(id) = &event.subject
                && *id == wanted
            {
                found += 1;
            }
        }
        if reader.anchor().sequence >= through.sequence {
            return Ok(found);
        }
        match page.next {
            Some(next) => cursor = Some(next),
            None => return Ok(found),
        }
    }
    Err(ApiError::new(
        realm_id,
        ApiErrorCode::Unavailable,
        "the prefix before the supplied cursor could not be read, so this message id's uniqueness is unproven",
    )
    .advising("observe again without `after` so the whole canonical history is read"))
}

/// Where an observation resumes from.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ObserveQuery {
    /// A previous observation's anchor, or a timeline anchor. Absent reads from
    /// the start of the retained epoch.
    pub after: Option<String>,
    /// Maximum items per page.
    pub limit: Option<u32>,
}

/// Observe the exact current turn, without asserting anything about it.
///
/// This exists because a delivery seat cannot settle itself: it has no way to
/// name the canonical position of a response it has not returned yet. A
/// post-turn control caller can, and until now it had to hand-derive the tuple
/// from a timeline read. Hand-derivation is exactly where a wrong position comes
/// from, and a wrong position is what settlement's guard then has to catch.
///
/// Read-only by construction. It runs the same canonical history path
/// `/timeline` does — same cursor, same `HistoryReader` validation, so a gap, a
/// redelivery or an epoch change is refused here too — and it writes nothing,
/// attests nothing and settles nothing. `turns:settle` re-derives all of it and
/// remains the only validator: an observation is a convenience for the caller,
/// never evidence on its own.
///
/// The turn it reports is the *last complete* one: the final canonically
/// addressed Kontor message, and the terminal provider response that closed it.
/// A seat still working has no such pair and is reported as unfinished rather
/// than as a turn whose end has not arrived.
#[utoipa::path(
    get, path = "/v1/sessions/{agent_run_id}/turns/current", tag = "sessions",
    params(
        ("agent_run_id" = String, Path, description = "The Kontor agent run"),
        ("after" = Option<String>, Query, description = "Resume from a previous anchor"),
        ("limit" = Option<u32>, Query, description = "Maximum items per page")
    ),
    responses(
        (status = 200, body = ObservedTurnDto, description = "The exact current turn"),
        (status = 404, description = "No completed turn is visible in the scanned window"),
        (status = 409, description = "The seat is still working, or the history broke"),
        (status = 422, description = "This runtime cannot replay content")
    )
)]
pub async fn observe_current_turn(
    State(state): State<ApiState>,
    caller: Caller,
    Path(agent_run_id): Path<String>,
    Query(query): Query<ObserveQuery>,
) -> Result<Json<ObservedTurnDto>, ApiError> {
    let session = resolve(&state, &agent_run_id, CallerCapability::Observer, caller).await?;
    let realm_id = state.realm_id();
    let page_size = query.limit.unwrap_or(DEFAULT_PAGE);
    session.preflight(
        realm_id,
        RuntimeCapability::History,
        Some(LimitDemand::HistoryPage(page_size)),
    )?;
    session.preflight(realm_id, RuntimeCapability::Inspect, None)?;

    // A turn that has not ended has no terminal response, and reporting the
    // newest message with whatever follows it would be inventing one. The fresh
    // inspect is what distinguishes "finished" from "quiet for a moment", and it
    // is a read: nothing about it is persisted here.
    let observation = session
        .adapter
        .inspect(&InspectRequest {
            binding: session.snapshot.clone(),
            requested_at: now(),
        })
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    if observation.state != kontor_core::state::ObservedRunState::WaitingInput {
        return Err(ApiError::new(
            realm_id,
            ApiErrorCode::RevisionConflict,
            "this seat is not waiting after a finished turn, so it has no current turn to observe",
        )
        .advising("observe again once the seat reports waiting for input"));
    }

    let mut cursor = query.after.as_deref().map(HistoryCursor::from_text);
    let resume = cursor
        .as_ref()
        .map(|cursor| cursor.resolve(session.snapshot.binding_id()))
        .transpose()
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
    let mut reader: Option<HistoryReader> = None;

    // What the scan is looking for, carried across pages.
    let mut message: Option<(MessageId, TimelinePosition)> = None;
    let mut seen: BTreeMap<MessageId, usize> = BTreeMap::new();
    let mut response: Option<TimelinePosition> = None;
    let mut last_turn: Option<TimelinePosition> = None;
    let mut anchor = resume;
    let mut exhausted = false;

    for _ in 0..OBSERVE_PAGE_BUDGET {
        let mut page = session
            .adapter
            .history(&HistoryRequest {
                binding: session.snapshot.clone(),
                cursor: cursor.clone(),
                page_size,
            })
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

        // The same exactly-once validation `/timeline` applies. A gap, a
        // redelivered position or a changed epoch is refused rather than
        // silently producing a tuple assembled from two transcripts.
        let reader = reader.get_or_insert_with(|| match resume {
            None => HistoryReader::start(session.snapshot.binding_id(), page.epoch),
            Some(position) => HistoryReader::resuming(session.snapshot.binding_id(), position),
        });
        reader
            .accept_page(&mut page)
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

        for event in &page.items {
            if let EventSubject::Message(id) = &event.subject {
                let id = *id;
                // A new addressed message opens a new turn and retires whatever
                // response was collected for the previous one.
                *seen.entry(id).or_insert(0) += 1;
                message = Some((id, event.position));
                response = None;
            } else if event.kind == SessionEventKind::Message && message.is_some() {
                response = Some(event.position);
            }
            if !matches!(
                event.kind,
                SessionEventKind::StateChange | SessionEventKind::Log
            ) {
                last_turn = Some(event.position);
            }
        }
        anchor = Some(reader.anchor());
        match page.next {
            Some(next) => cursor = Some(next),
            None => {
                exhausted = true;
                break;
            }
        }
    }

    if !exhausted {
        return Err(ApiError::new(
            realm_id,
            ApiErrorCode::Unavailable,
            "the canonical scan reached its page budget before the end of this session",
        )
        .advising("observe again with `after` set to the anchor this read returned"));
    }

    let (message_id, message_position) = message.ok_or_else(|| {
        ApiError::new(
            realm_id,
            ApiErrorCode::NotFound,
            "no canonically addressed Kontor message appears in the scanned window",
        )
        .advising("widen the window by observing without `after`, or send the turn first")
    })?;
    // One id, one turn. The same id twice is divergence and is worth strictly
    // less than no answer: a caller cannot tell which of them it is settling.
    //
    // `seen` only covers what this scan walked, so with an `after` cursor it
    // covers only the suffix. That is not enough, and nothing downstream closes
    // the gap: settlement's own scan starts immediately before the occurrence it
    // is handed, and the store refuses an id that already *settled*, not one
    // that merely already *appeared*. A first occurrence sitting in the skipped
    // prefix would therefore be invisible to every layer. So when a cursor was
    // used, the prefix is read for this exact id before the tuple is reported.
    let occurrences = seen.get(&message_id).copied().unwrap_or_default()
        + match resume {
            None => 0,
            Some(resume_at) => {
                prefix_occurrences(&session, realm_id, page_size, resume_at, message_id).await?
            }
        };
    if occurrences > 1 {
        return Err(ApiError::new(
            realm_id,
            ApiErrorCode::RevisionConflict,
            "this message id appears more than once in the session's canonical content",
        ));
    }
    let response_position = response.ok_or_else(|| {
        ApiError::new(
            realm_id,
            ApiErrorCode::RevisionConflict,
            "the current message has no terminal provider response yet",
        )
        .advising("observe again once the seat has answered")
    })?;
    // The response has to be the tail, for the same reason settlement requires
    // it: anything after it means the turn being described is not the current
    // one. State changes and logs are not turn content and do not count.
    if last_turn != Some(response_position) {
        return Err(ApiError::new(
            realm_id,
            ApiErrorCode::RevisionConflict,
            "the newest turn's response is not the last canonical turn event",
        ));
    }

    Ok(Json(ObservedTurnDto {
        realm_id,
        agent_run_id: session.agent_run_id,
        message_id: message_id.to_string(),
        timeline_epoch: message_position.epoch,
        message_sequence: message_position.sequence,
        response_sequence: response_position.sequence,
        anchor: anchor
            .map(|position| {
                HistoryCursor::issue(session.snapshot.binding_id(), position)
                    .as_str()
                    .to_owned()
            })
            .unwrap_or_default(),
    }))
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

    let stopping = state.signals().stops();
    let binding = session.snapshot;
    let adapter = session.adapter;
    let kinds = EVERY_KIND.iter().copied().collect::<BTreeSet<_>>();
    let stream = futures::stream::unfold(
        Some((
            subscription,
            adapter,
            binding,
            kinds,
            stopping,
            realm_id,
            session.agent_run_id,
        )),
        |held| async move {
            let (mut subscription, adapter, binding, kinds, mut stopping, realm_id, agent_run_id) =
                held?;
            loop {
                match subscription.next_event() {
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
                        return Some((
                            Ok(frame),
                            Some((
                                subscription,
                                adapter,
                                binding,
                                kinds,
                                stopping,
                                realm_id,
                                agent_run_id,
                            )),
                        ));
                    }
                    // A broken timeline ends the stream with a typed frame.
                    // Continuing would hand the caller a hole it cannot see.
                    Some(Err(_)) => {
                        let refusal = Event::default()
                            .event(REFUSAL_EVENT)
                            .json_data(StreamRefusalDto {
                                realm_id,
                                code: "timeline_refetch_required",
                                rule: "the runtime renumbered or skipped this session's content",
                            })
                            .ok()?;
                        return Some((Ok(refusal), None));
                    }
                    None => {}
                }

                if *stopping.borrow() {
                    return None;
                }
                tokio::select! {
                    () = tokio::time::sleep(LIVE_RECONCILE_INTERVAL) => {}
                    changed = stopping.changed() => {
                        if changed.is_err() || *stopping.borrow() {
                            return None;
                        }
                    }
                }

                let strict_after = subscription.position();
                subscription = match adapter
                    .subscribe_live(&LiveSubscribeRequest {
                        binding: binding.clone(),
                        kinds: kinds.clone(),
                        strict_after,
                    })
                    .await
                {
                    Ok(next) => next,
                    Err(error) => {
                        tracing::warn!(
                            agent_run_id = %agent_run_id,
                            detail = %error,
                            "a live session stream lost its runtime"
                        );
                        let refusal = Event::default()
                            .event(REFUSAL_EVENT)
                            .json_data(StreamRefusalDto {
                                realm_id,
                                code: "runtime_unavailable",
                                rule: "the session's runtime could not continue the live stream",
                            })
                            .ok()?;
                        return Some((Ok(refusal), None));
                    }
                };
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Re-state a refusal that happened *after* the message was delivered.
///
/// The code is kept — whatever went wrong with the readback really did go
/// wrong, and a caller's backoff for it is still right. What is replaced is the
/// claim the caller would otherwise act on. An ordinary channel refusal advises
/// "nothing was changed"; here the most important thing in the world did
/// change, and a caller who resends under a fresh idempotency key puts a second
/// copy of one instruction into a live seat's transcript.
fn after_delivery(error: ApiError) -> ApiError {
    ApiError::new(
        error.realm_id,
        error.code,
        "the message was delivered and acknowledged, but the session readback that follows it did not answer",
    )
    .advising(
        "replay this exact idempotency key: it returns the original acknowledgement and repairs the session projection. Never resend under a new one",
    )
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
    // A message is not considered successfully projected unless the exact
    // issued binding can be inspected afterwards. Refuse before delivery when
    // that evidence surface was not frozen into this binding.
    session.preflight(realm_id, RuntimeCapability::Inspect, None)?;

    // A persistent seat may be between native processes. Resume is a no-op for
    // an already-live session and reloads a closed, resumable one in place. A
    // direct send cannot do that job: Paseo quite correctly refuses to address a
    // process that is no longer running.
    session
        .adapter
        .resume(&ResumeRequest {
            binding: session.snapshot.clone(),
            requested_at: now(),
        })
        .await
        .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
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
    // Acceptance says only that the message effect landed. The runtime's fresh
    // readback decides whether the same native seat is running, waiting or in
    // another state, and the shared reducer advances its AgentRun and TeamRun
    // together. A replay follows this same path, repairing a projection left
    // stale when an earlier acknowledgement or post-send inspect was lost.
    let reduced_at = now();
    // Past this line the message is delivered and its acknowledgement is in
    // hand. Everything that remains is *projection*, and a projection fault
    // must never be reported as a failed delivery: the readback is how Kontor
    // learns what the seat is now doing, not how the seat learns what to do.
    let observation = session
        .adapter
        .inspect(&InspectRequest {
            binding: session.snapshot.clone(),
            requested_at: reduced_at,
        })
        .await
        .map_err(|error| after_delivery(ApiError::from_runtime(realm_id, &error)))?;
    state
        .applications()
        .persist_session_observation(
            session.project_id,
            session.agent_run_id,
            &observation,
            reduced_at,
        )
        .map_err(after_delivery)?;
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
///
/// The identity has to come from the caller's key, because on these routes the
/// message id *is* the idempotency record: generating one per call would send
/// the message twice on a retry. It does not have to be spelled as a UUIDv7.
/// Requiring that made these routes the only writes in Kontor that refuse the
/// key vocabulary the contract documents, and ASMA-8191 records what that cost
/// on the one route whose refusal also named no field.
fn message_identifier(state: &ApiState, headers: &HeaderMap) -> Result<MessageId, ApiError> {
    let key = idempotency_key(state, headers)?;
    Ok(MessageId::parse(key.as_str()).unwrap_or_else(|_| MessageId::derive(key.as_str())))
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
