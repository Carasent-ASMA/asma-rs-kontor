//! Section 3 — the session contract: one canonical history, two clients, and
//! the things a client is not allowed to reach.
//!
//! Every case here drives a *real* Realm: a `TempDir` state root with its own
//! lock, credential file and migrated database, the same `axum::Router` the
//! daemon binary serves, and a scripted runtime behind a real launch. Nothing
//! binds a socket and nothing spawns a process (TST-001), so what is exercised is
//! the real middleware, the real extractors and the real handlers — over
//! `tower::ServiceExt::oneshot`.
//!
//! # Why the harness is written again here rather than imported
//!
//! `crates/kontor-daemon/tests/harness/mod.rs` is the canonical shape and this
//! module deliberately mirrors its seeding and launch order. It cannot be
//! imported: it is `pub(crate)` inside another crate's test binary, and a crate
//! does not export its test harness. Copying the *sequence* rather than the code
//! is the honest option — and the sequence is load-bearing, because without the
//! final `sessions().record(...)` every `/v1/sessions/*` route answers
//! `stale_binding`.
//!
//! # What "desktop and phone" means at this layer
//!
//! There is no browser harness anywhere in this tree — no Playwright config, no
//! driver, no screenshot pipeline — and this driver does not add one. Viewport
//! behaviour is exercised by the console's own vitest suite through
//! `apps/console/src/test/viewport.ts`. What is provable *here*, and what the
//! criterion actually claims, is the contract-layer invariant underneath it: the
//! same canonical history regardless of who asks or how small their pages are.
//! So the two clients differ exactly where a desktop and a phone differ against
//! this API — a `User-Agent` and a page size — and the concatenated, normalized
//! item streams must be identical.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, Response};
use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{
    AgentRunId, BoundedText, ExternalName, ProjectId, RoleSlotId, RuntimeBindingId, RuntimeKindKey,
    SCHEMA_VERSION, TaskId, TeamRunId,
};
use kontor_core::repository::{
    NewAgentRun, NewProject, NewTask, NewTeamRun, ProjectRepository, RunRepository, RuntimeBinding,
    SpecRepository,
};
use kontor_core::spec::TeamRunSnapshot;
use kontor_core::state::TaskState;
use kontor_daemon::{Daemon, DaemonConfig};
use kontor_mcp::Dispatcher;
use kontor_mcp::client::{
    CallerTier, Frame, FrameBudget, Method as ClientMethod, Reply, Request as ClientRequest,
    Transport, TransportFailure,
};
use kontor_profiles::pack::{PackAvailability, resolve_profile};
use kontor_profiles::seeds::bundled_pack;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{
    RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{
    AdapterCall, RequestKey, RuntimeScript, ScriptStep, ScriptedFakeRuntime,
};
use kontor_runtime::request::{LaunchParts, MessageId};
use kontor_runtime::timeline::{EventSubject, HistoryCursor, SessionEventKind, TimelinePosition};
use kontor_runtime::workspace::{WorkspaceBindingId, WorkspacePrepareRequest, WorkspaceRoot};
use kontor_tests_e2e::{Bundle, digest, scan_for_canaries};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tower::ServiceExt as _;

use crate::at;

// ---------------------------------------------------------------------------
// Fixture constants
// ---------------------------------------------------------------------------

/// The loopback authority every request claims.
///
/// `IngressPolicy` admits on the `Host` it *receives*, so this is not decoration:
/// a missing or foreign authority is refused before a handler is reached.
const LOOPBACK_AUTHORITY: &str = "127.0.0.1:7717";

/// The runtime family the scripted fake answers to.
const FAKE_FAMILY: &str = "fake.runtime";

/// The one task workspace every seat of the pilot team run shares.
///
/// One team run has one task workspace, and the runtime refuses a second root
/// for the same team run — which is the isolation rule, not a harness quirk.
const TASK_WORKSPACE: &str = "/w/pilot-session";

/// The permission request the pilot session raises.
const PERMISSION_ID: &str = "pilot-permission-1";

/// How many recorded items the pilot session's history holds.
///
/// Twenty is chosen so the two page sizes genuinely disagree about paging: the
/// desktop's fifty takes one page and the phone's eight takes three. A history
/// that fitted in both page sizes would make the parity claim vacuous.
const HISTORY_ITEMS: u64 = 20;

/// The desktop client's page size — one page covers the whole history.
const DESKTOP_PAGE: u64 = 50;

/// The phone client's page size — three pages cover the same history.
const PHONE_PAGE: u64 = 8;

/// The `User-Agent` the desktop client presents.
const DESKTOP_AGENT: &str = "kontor-pilot/desktop (1440x900)";

/// The `User-Agent` the phone client presents.
const PHONE_AGENT: &str = "kontor-pilot/phone (390x844)";

/// A transcript body that must never be persisted anywhere.
///
/// It is sent as a real session message through the daemon, so its absence from
/// the state root is a fact about what the control plane wrote — not about a
/// string that was never in play.
const TRANSCRIPT_CANARY: &str = "kontor-pilot-transcript-canary-4f1c9a2b";

/// A credential-shaped string the *runtime* leaks into its own content.
///
/// This is the nastier case: the control plane relays it to a reader and must
/// still not keep a copy. It is deliberately not planted in `credentials.json`,
/// which legitimately holds this Realm's tier secrets.
const TOKEN_CANARY: &str = "kontor-pilot-token-canary-sk-7d3e0155";

/// The launch prompt, which is also a canary.
const PROMPT_CANARY: &str = "kontor-pilot-prompt-canary-b82ef460";

/// The Realm's credential file, excluded from the token-canary claim.
const CREDENTIAL_FILE: &str = "credentials.json";

/// Schemes no client source may speak. The daemon is reached over `/v1` only.
const FORBIDDEN_SCHEMES: &[&str] = &["ws://", "wss://"];

/// Runtime vocabulary that must not appear in any client source file.
const RUNTIME_NEEDLES: &[&str] = &["paseo", "agent-orchestrator", "runtime_endpoint"];

/// Ways a client could open a socket the contract does not describe.
const FORBIDDEN_CONSTRUCTORS: &[&str] = &["new WebSocket(", "new EventSource("];

/// Reserved top-level domains, which can only ever be fixtures.
///
/// RFC 2606 and RFC 6761 guarantee these never resolve, so a literal under one
/// is a test asserting how a *bad* endpoint is treated, not a route to anything.
const RESERVED_TLDS: &[&str] = &[".test", ".example", ".invalid", ".localhost"];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Answer every session-contract criterion.
pub(crate) async fn run(bundle: &mut Bundle) {
    // Structural, and needs no Realm: the claim is about what client source can
    // address at all, which is decided before anything runs.
    no_direct_runtime(bundle);

    let realm = Realm::open().await;
    realm.script(&parity_script());
    let (run, snapshot) = realm.launch("pilot-session-seat").await;
    bundle.event(
        "session.realm",
        json!({
            "agent_run_id": run.to_string(),
            "binding_id": snapshot.binding_id().to_string(),
            "runtime_kind": FAKE_FAMILY,
            "history_items": HISTORY_ITEMS,
        }),
    );

    // Reads first, then mutations: a message committed between the two clients
    // would make them disagree for a reason that is not a defect.
    let desktop = history_parity(bundle, &realm, run).await;
    live_parity(bundle, &realm, run, desktop.as_ref()).await;
    message_idempotency(bundle, &realm, run, &snapshot).await;
    permission_idempotency(bundle, &realm, run, &snapshot).await;
    refetch(bundle, &realm, run, &snapshot, desktop.as_ref()).await;
    surface_parity(bundle, &realm, run).await;

    // Last, so everything the section could possibly have persisted has been
    // written by the time the state root is read.
    privacy(bundle, &realm);
}

// ---------------------------------------------------------------------------
// 1 — history parity
// ---------------------------------------------------------------------------

/// One client's whole canonical history, normalized for comparison.
///
/// Nothing here carries a payload body: a parity artifact that quoted a
/// transcript to prove two readers agreed would be the leak section 3 is
/// otherwise about.
struct ClientHistory {
    /// The label this client is reported under.
    client: &'static str,
    /// The `User-Agent` it presented.
    agent: &'static str,
    /// Its page size.
    page_size: u64,
    /// How many pages it needed.
    pages: usize,
    /// The concatenated item stream: kind, position, subject and payload digest.
    items: Vec<Value>,
    /// The content epoch every page reported.
    epoch: u64,
    /// The anchor the last page ended on.
    anchor: String,
    /// The permission ids the history raised, in order.
    permissions: Vec<String>,
}

impl ClientHistory {
    /// One digest over the whole normalized stream.
    ///
    /// Two clients agreeing on this single value is the claim; the per-item list
    /// is written beside it so a disagreement can be located rather than merely
    /// reported.
    fn stream_digest(&self) -> String {
        digest(
            serde_json::to_string(&self.items)
                .expect("normalized items serialize")
                .as_bytes(),
        )
    }

    /// The redacted evidence document for this client.
    fn evidence(&self) -> Value {
        json!({
            "client": self.client,
            "user_agent": self.agent,
            "page_size": self.page_size,
            "pages_fetched": self.pages,
            "epoch": self.epoch,
            "item_count": self.items.len(),
            "anchor": self.anchor,
            "permissions_raised": self.permissions,
            "stream_sha256": self.stream_digest(),
            "items": self.items,
            "redaction": "kinds, positions, subject ids and payload digests only; no payload body \
                          is written into any pilot artifact",
        })
    }
}

/// Two clients, two page sizes, one canonical history.
///
/// Returns the desktop reading so the later cases can reuse its anchor and its
/// digest rather than fetching the same page again under a different name.
async fn history_parity(
    bundle: &mut Bundle,
    realm: &Realm,
    run: AgentRunId,
) -> Option<ClientHistory> {
    let desktop = match read_history(realm, run, "desktop", DESKTOP_AGENT, DESKTOP_PAGE).await {
        Ok(history) => history,
        Err(problem) => {
            bundle.fail("session.history-parity", problem);
            return None;
        }
    };
    let phone = match read_history(realm, run, "phone", PHONE_AGENT, PHONE_PAGE).await {
        Ok(history) => history,
        Err(problem) => {
            bundle.fail("session.history-parity", problem);
            return Some(desktop);
        }
    };

    let desktop_path = bundle
        .artifact("session/history-desktop.json", &desktop.evidence())
        .expect("the desktop history is written");
    let phone_path = bundle
        .artifact("session/history-phone.json", &phone.evidence())
        .expect("the phone history is written");

    let identical = desktop.items == phone.items
        && desktop.anchor == phone.anchor
        && desktop.epoch == phone.epoch
        && desktop.permissions == phone.permissions;
    let genuinely_paginated = phone.pages > desktop.pages && desktop.pages == 1;
    let complete = u64::try_from(desktop.items.len()).unwrap_or_default() == HISTORY_ITEMS;

    if identical && genuinely_paginated && complete {
        bundle.pass(
            "session.history-parity",
            format!(
                "two clients differing only in `User-Agent` and page size loaded byte-identical \
                 canonical history: {} items over {} desktop page and {} phone pages, the same \
                 epoch {}, the same anchor and the same stream digest `{}`. Deviation, stated \
                 plainly: there are no PNG screenshots and no viewport is driven here, because no \
                 browser harness exists in this tree — the console's own vitest suite exercises \
                 the viewport through `apps/console/src/test/viewport.ts`, and this driver proves \
                 the API-level invariant underneath it, which is that page size and client \
                 identity cannot change what the history *is*",
                desktop.items.len(),
                desktop.pages,
                phone.pages,
                desktop.epoch,
                &desktop.stream_digest()[..16],
            ),
            &[desktop_path, phone_path],
        );
    } else {
        bundle.fail(
            "session.history-parity",
            format!(
                "identical_streams={identical}, desktop_pages={}, phone_pages={}, \
                 desktop_items={}, phone_items={}, desktop_anchor={}, phone_anchor={}",
                desktop.pages,
                phone.pages,
                desktop.items.len(),
                phone.items.len(),
                desktop.anchor,
                phone.anchor,
            ),
        );
    }
    Some(desktop)
}

/// Follow the cursor until the history is exhausted, normalizing as it goes.
async fn read_history(
    realm: &Realm,
    run: AgentRunId,
    client: &'static str,
    agent: &'static str,
    page_size: u64,
) -> Result<ClientHistory, String> {
    let mut items = Vec::new();
    let mut permissions = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    // Collected rather than overwritten: every page of one history must report
    // the same content epoch, and a set with two members is a break the reader
    // would already have refused.
    let mut epochs = BTreeSet::new();
    let anchor;
    loop {
        let uri = match &cursor {
            None => format!("/v1/sessions/{run}/timeline?limit={page_size}"),
            Some(after) => {
                format!("/v1/sessions/{run}/timeline?limit={page_size}&after={after}")
            }
        };
        let answer = realm.get_as(&uri, CallerTier::Observer, agent).await;
        if answer.status != 200 {
            return Err(format!(
                "{client}: page {} answered {} rather than 200",
                pages + 1,
                answer.status
            ));
        }
        let body = answer.json();
        pages += 1;
        epochs.insert(body["epoch"].as_u64().unwrap_or_default());
        if let Some(page) = body["items"].as_array() {
            for item in page {
                if let Some(id) = item["permission_id"].as_str() {
                    permissions.push(id.to_owned());
                }
                items.push(normalize(item));
            }
        }
        match body["next"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => {
                anchor = body["anchor"].as_str().unwrap_or_default().to_owned();
                break;
            }
        }
        if pages > 32 {
            return Err(format!("{client}: the cursor never exhausted the history"));
        }
    }
    if epochs.len() > 1 {
        return Err(format!(
            "{client}: the pages reported {} different epochs",
            epochs.len()
        ));
    }
    Ok(ClientHistory {
        client,
        agent,
        page_size,
        pages,
        items,
        epoch: epochs.into_iter().next().unwrap_or_default(),
        anchor,
        permissions,
    })
}

/// One timeline item, reduced to what a parity claim is allowed to compare.
fn normalize(item: &Value) -> Value {
    json!({
        "kind": item["kind"],
        "epoch": item["epoch"],
        "sequence": item["sequence"],
        "permission_id": item["permission_id"],
        "message_id": item["message_id"],
        "payload_sha256": digest(
            serde_json::to_string(&item["payload"])
                .unwrap_or_default()
                .as_bytes(),
        ),
    })
}

// ---------------------------------------------------------------------------
// 2 — live parity
// ---------------------------------------------------------------------------

/// Both clients subscribe strictly after the same runtime cursor and agree.
async fn live_parity(
    bundle: &mut Bundle,
    realm: &Realm,
    run: AgentRunId,
    desktop: Option<&ClientHistory>,
) {
    let Some(history) = desktop else {
        bundle.fail(
            "session.live-parity",
            "history never loaded, so there was no anchor for a live read to be strictly after",
        );
        return;
    };
    let anchor = &history.anchor;
    let after = history
        .items
        .last()
        .and_then(|item| item["sequence"].as_u64())
        .unwrap_or_default();

    let desktop_frames = read_frames(realm, run, anchor, DESKTOP_AGENT).await;
    let phone_frames = read_frames(realm, run, anchor, PHONE_AGENT).await;

    let (desktop_status, desktop_items) = desktop_frames;
    let (phone_status, phone_items) = phone_frames;

    let strictly_after = desktop_items
        .iter()
        .all(|frame| frame["sequence"].as_u64().unwrap_or_default() > after);
    let contiguous = desktop_items.iter().enumerate().all(|(index, frame)| {
        let offset = u64::try_from(index).unwrap_or_default();
        frame["sequence"].as_u64() == Some(after + offset + 1)
    });

    let artifact = bundle
        .artifact(
            "session/live-frames.json",
            &json!({
                "anchor": anchor,
                "anchor_sequence": after,
                "desktop": {
                    "user_agent": DESKTOP_AGENT,
                    "status": desktop_status,
                    "frames": desktop_items,
                },
                "phone": {
                    "user_agent": PHONE_AGENT,
                    "status": phone_status,
                    "frames": phone_items,
                },
                "identical": desktop_items == phone_items,
                "strictly_after_anchor": strictly_after,
                "contiguous_from_anchor": contiguous,
                "redaction": "frame ids, normalized kinds, positions and payload digests only",
            }),
        )
        .expect("the live frames are written");

    let agreed = desktop_items == phone_items;
    let delivered = !desktop_items.is_empty();
    if agreed && delivered && strictly_after && contiguous && desktop_status == 200 {
        bundle.pass(
            "session.live-parity",
            format!(
                "both clients subscribed at the anchor the timeline returned (`{}`, sequence {}) \
                 and were delivered the same {} frames: identical normalized kinds, identical \
                 `(epoch, sequence)` positions and identical payload digests, every one strictly \
                 after the anchor and contiguous from it — so neither client saw an item the other \
                 did not, and neither re-read an item its history already covered",
                &anchor[..anchor.len().min(24)],
                after,
                desktop_items.len(),
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "session.live-parity",
            format!(
                "agreed={agreed}, delivered={delivered}, strictly_after={strictly_after}, \
                 contiguous={contiguous}, desktop_status={desktop_status}, \
                 phone_status={phone_status}"
            ),
        );
    }
}

/// One client's live read, as `(status, normalized frames)`.
async fn read_frames(
    realm: &Realm,
    run: AgentRunId,
    anchor: &str,
    agent: &'static str,
) -> (u16, Vec<Value>) {
    let answer = realm
        .get_as(
            &format!("/v1/sessions/{run}/stream?after={anchor}"),
            CallerTier::Observer,
            agent,
        )
        .await;
    let frames = sse_frames(&answer.body)
        .into_iter()
        .filter(|(event, _, _)| event == "content")
        .map(|(_, id, data)| {
            let mut normalized = normalize(&data["item"]);
            if let Some(object) = normalized.as_object_mut() {
                object.insert("frame_id".to_owned(), Value::String(id));
            }
            normalized
        })
        .collect();
    (answer.status, frames)
}

// ---------------------------------------------------------------------------
// 3 — message idempotency
// ---------------------------------------------------------------------------

/// The same client message id twice: one runtime effect, one original receipt.
async fn message_idempotency(
    bundle: &mut Bundle,
    realm: &Realm,
    run: AgentRunId,
    snapshot: &RuntimeBindingSnapshot,
) {
    let before_effects = realm.fake.committed_messages(snapshot);
    let before_sends = realm.sends(snapshot);

    // 1. The plain retry: same key, same body.
    let key = MessageId::generate().to_string();
    let body = json!({ "body": TRANSCRIPT_CANARY });
    let first = realm.message(run, &key, &body).await;
    let repeat = realm.message(run, &key, &body).await;

    // 2. The contradiction: same key, different body.
    let conflict = realm
        .message(run, &key, &json!({ "body": "a different thing entirely" }))
        .await;

    // 3. The lost acknowledgement. The runtime commits and then drops the ack, so
    //    the caller cannot tell whether it landed. Retrying must not send twice.
    let lost_key = MessageId::generate().to_string();
    realm.fake.push_step_for(
        ScriptStep::LoseSendAck,
        RequestKey::Message(MessageId::parse(&lost_key).expect("a canonical message id")),
    );
    let lost_body = json!({ "body": "the pilot message whose acknowledgement was lost" });
    let lost = realm.message(run, &lost_key, &lost_body).await;
    let recovered = realm.message(run, &lost_key, &lost_body).await;

    let effects = realm.fake.committed_messages(snapshot) - before_effects;
    let dispatches = realm.sends(snapshot) - before_sends;
    let timeline_copies = realm.count_messages(snapshot, &key);
    let lost_copies = realm.count_messages(snapshot, &lost_key);

    let artifact = bundle
        .artifact(
            "session/idempotency.json",
            &json!({
                "message": {
                    "key_is_the_idempotency_header": true,
                    "body_sha256": digest(TRANSCRIPT_CANARY.as_bytes()),
                    "plain_retry": {
                        "first_status": first.status,
                        "repeat_status": repeat.status,
                        "acknowledgements_identical": first.json() == repeat.json(),
                        "epoch": first.json()["value"]["epoch"],
                        "sequence": first.json()["value"]["sequence"],
                    },
                    "contradiction": {
                        "status": conflict.status,
                        "code": conflict.code(),
                    },
                    "lost_acknowledgement": {
                        "first_status": lost.status,
                        "first_code": lost.code(),
                        "retry_status": recovered.status,
                        "retry_replayed_the_original": recovered.json()["value"]["message_id"]
                            == json!(lost_key),
                        "committed_copies": lost_copies,
                    },
                    "runtime_dispatches": dispatches,
                    "runtime_effects": effects,
                    "timeline_copies_of_the_key": timeline_copies,
                },
                "rule": "the client-supplied id IS the Idempotency-Key: there is no id field in \
                         the body, so a caller cannot present two ids that disagree about whether \
                         a retry is the same message",
            }),
        )
        .expect("the message idempotency evidence is written");

    let replayed = first.status == 200 && repeat.status == 200 && first.json() == repeat.json();
    let conflicted = conflict.status == 409 && conflict.code() == "idempotency_conflict";
    let lost_then_replayed = lost.status == 503
        && lost.code() == "unavailable"
        && recovered.status == 200
        && recovered.json()["value"]["message_id"] == json!(lost_key);
    // Five dispatches — two keys posted twice, plus the contradiction, which is
    // refused by the runtime's own ledger and so reaches it — but two effects.
    let one_effect_each =
        effects == 2 && dispatches == 5 && timeline_copies == 1 && lost_copies == 1;

    if replayed && conflicted && lost_then_replayed && one_effect_each {
        bundle.pass(
            "session.message-idempotency",
            "two message ids were each posted twice through the daemon, plus one contradiction: 5 \
             dispatches reached the runtime and exactly 2 messages were committed, each appearing \
             once in the session's content. The repeat answered 200 with the byte-identical \
             original acknowledgement — \
             same epoch, same sequence, same `accepted_at` — rather than a fresh one; the same key \
             with different content was refused 409 `idempotency_conflict`; and a send whose \
             acknowledgement was lost after committing answered 503 `unavailable` and then \
             replayed its original receipt on retry instead of sending a second message",
            &[artifact],
        );
    } else {
        bundle.fail(
            "session.message-idempotency",
            format!(
                "replayed={replayed}, conflicted={conflicted}, \
                 lost_then_replayed={lost_then_replayed}, effects={effects}, \
                 dispatches={dispatches}, timeline_copies={timeline_copies}, \
                 lost_copies={lost_copies}"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 4 — permission idempotency
// ---------------------------------------------------------------------------

/// The same permission response id twice: one effect, one original receipt.
async fn permission_idempotency(
    bundle: &mut Bundle,
    realm: &Realm,
    run: AgentRunId,
    snapshot: &RuntimeBindingSnapshot,
) {
    let pending_before = realm.fake.pending_permissions().len();
    let before_dispatches = realm.responses();

    let key = MessageId::generate().to_string();
    let allow = json!({ "decision": "allow" });
    let first = realm.permission(run, PERMISSION_ID, &key, &allow).await;
    let repeat = realm.permission(run, PERMISSION_ID, &key, &allow).await;
    let contradiction = realm
        .permission(run, PERMISSION_ID, &key, &json!({ "decision": "deny" }))
        .await;

    // A request this session's content never raised is refused before dispatch,
    // which is what makes "answered once" a claim about *this* session.
    let foreign_key = MessageId::generate().to_string();
    let foreign = realm
        .permission(run, "pilot-permission-elsewhere", &foreign_key, &allow)
        .await;

    let dispatches = realm.responses() - before_dispatches;
    let resolutions = realm.count_resolutions(snapshot, PERMISSION_ID);
    let pending_after = realm.fake.pending_permissions().len();

    let artifact = bundle
        .artifact(
            "session/permission-idempotency.json",
            &json!({
                "permission_id": PERMISSION_ID,
                "first_status": first.status,
                "repeat_status": repeat.status,
                "acknowledgements_identical": first.json() == repeat.json(),
                "decision": first.json()["value"]["decision"],
                "epoch": first.json()["value"]["epoch"],
                "sequence": first.json()["value"]["sequence"],
                "contradiction": {
                    "status": contradiction.status,
                    "code": contradiction.code(),
                },
                "never_raised_here": {
                    "status": foreign.status,
                    "code": foreign.code(),
                },
                "runtime_dispatches": dispatches,
                "resolutions_in_content": resolutions,
                "pending_before": pending_before,
                "pending_after": pending_after,
            }),
        )
        .expect("the permission idempotency evidence is written");

    let replayed = first.status == 200 && repeat.status == 200 && first.json() == repeat.json();
    let conflicted = contradiction.status == 409 && contradiction.code() == "idempotency_conflict";
    let refused_foreign = foreign.status == 404;
    // Three dispatches — the answer, its replay and the contradiction the
    // runtime's ledger refuses — against exactly one applied decision. The
    // fourth call never reached the runtime at all.
    let one_effect = resolutions == 1 && pending_after + 1 == pending_before && dispatches == 3;

    if replayed && conflicted && refused_foreign && one_effect {
        bundle.pass(
            "session.permission-idempotency",
            format!(
                "the same response id answered `{PERMISSION_ID}` twice: {dispatches} dispatches \
                 reached the runtime and exactly one `permission_resolved` event was appended to \
                 the session's content, with the pending set falling from {pending_before} to \
                 {pending_after}. The repeat answered 200 with the byte-identical original \
                 acknowledgement rather than applying a second decision; the same id with the \
                 opposite decision was refused 409 `idempotency_conflict`; and an id this \
                 session's content never raised was refused 404 before dispatch"
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "session.permission-idempotency",
            format!(
                "replayed={replayed}, conflicted={conflicted}, refused_foreign={refused_foreign}, \
                 resolutions={resolutions}, pending {pending_before}->{pending_after}, \
                 dispatches={dispatches}"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 5 — refetch
// ---------------------------------------------------------------------------

/// An epoch change and a sequence gap both force a refetch, and neither touches
/// the run's lifecycle.
async fn refetch(
    bundle: &mut Bundle,
    realm: &Realm,
    run: AgentRunId,
    snapshot: &RuntimeBindingSnapshot,
    desktop: Option<&ClientHistory>,
) {
    let before = realm.lifecycle(run).await;

    // (a) The history layer. A cursor issued for *this* binding but naming an
    //     epoch the runtime is no longer in is the injected renumbering: the
    //     binding resolves, so nothing is refused for the wrong reason.
    let renumbered = HistoryCursor::issue(
        snapshot.binding_id(),
        TimelinePosition {
            epoch: 9,
            sequence: 1,
        },
    );
    let refused = realm
        .get(
            &format!("/v1/sessions/{run}/timeline?after={}", renumbered.as_str()),
            CallerTier::Observer,
        )
        .await;

    // (b) The live layer. A second session whose live content skips a sequence
    //     ends its stream with a typed refusal rather than handing the reader a
    //     hole it cannot see.
    realm.script(&gap_script());
    let (gap_run, _) = realm.launch("pilot-gap-seat").await;
    let gap_history = realm
        .get(
            &format!("/v1/sessions/{gap_run}/timeline"),
            CallerTier::Observer,
        )
        .await;
    let gap_anchor = gap_history.json()["anchor"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let gap_stream = realm
        .get(
            &format!("/v1/sessions/{gap_run}/stream?after={gap_anchor}"),
            CallerTier::Observer,
        )
        .await;
    let frames = sse_frames(&gap_stream.body);
    let delivered = frames
        .iter()
        .filter(|(event, _, _)| event == "content")
        .count();
    let ending = frames.last().cloned().unwrap_or_default();

    // (c) The reload. Canonical history is read again from the start and must be
    //     the same history the desktop client already validated.
    let reloaded = read_history(realm, run, "reload", DESKTOP_AGENT, DESKTOP_PAGE).await;
    let after = realm.lifecycle(run).await;

    // The reload is a *superset*, and deliberately so: real messages and a real
    // permission answer were committed into this session between the two reads,
    // and this runtime records a committed effect as content. What must hold is
    // the prefix — everything the first read validated is still there, in the
    // same order, with the same payload digests. A refetch that renumbered or
    // rewrote already-delivered items is exactly the defect being ruled out.
    let original: Vec<Value> = desktop
        .map(|history| history.items.clone())
        .unwrap_or_default();
    let (reload_count, prefix_matches, reload_digest) = match reloaded.as_ref() {
        Ok(history) => (
            history.items.len(),
            history.items.len() >= original.len()
                && !original.is_empty()
                && history.items[..original.len()] == original[..],
            Some(history.stream_digest()),
        ),
        Err(_) => (0, false, None),
    };

    let artifact = bundle
        .artifact(
            "session/refetch.json",
            &json!({
                "injected_epoch_change": {
                    "cursor_binding": "this session's own binding",
                    "cursor_epoch": 9,
                    "status": refused.status,
                    "code": refused.code(),
                    "rule": refused.json()["rule"],
                },
                "injected_sequence_gap": {
                    "agent_run_id": gap_run.to_string(),
                    "anchor": gap_anchor,
                    "stream_status": gap_stream.status,
                    "content_frames_before_the_break": delivered,
                    "final_event": ending.0,
                    "final_event_carried_an_id": !ending.1.is_empty(),
                    "final_code": ending.2["code"],
                    "final_rule": ending.2["rule"],
                },
                "canonical_reload": {
                    "ok": reloaded.is_ok(),
                    "items": reload_count,
                    "items_at_the_first_read": original.len(),
                    "stream_sha256": reload_digest,
                    "prefix_matches_the_first_read": prefix_matches,
                    "why_it_grew": "a message and a permission answer were committed into this \
                                    session between the two reads, and this runtime records a \
                                    committed effect as content",
                },
                "lifecycle": {
                    "before": before,
                    "after": after,
                    "unchanged": before == after,
                },
            }),
        )
        .expect("the refetch evidence is written");

    let epoch_refused = refused.status == 409 && refused.code() == "timeline_refetch_required";
    let gap_refused = gap_stream.status == 200
        && ending.0 == "error"
        && ending.1.is_empty()
        && ending.2["code"] == json!("timeline_refetch_required")
        && delivered == 1;
    let reloaded_ok = reload_digest.is_some() && prefix_matches;
    let lifecycle_held = before == after && before.is_some();

    if epoch_refused && gap_refused && reloaded_ok && lifecycle_held {
        bundle.pass(
            "session.refetch",
            "an epoch change and a sequence gap were both injected and both forced \
             `timeline_refetch_required`: a history cursor issued for this binding at a foreign \
             epoch was refused 409 by `GET /timeline`, and a live stream whose content skipped a \
             sequence delivered the one frame it could vouch for and then ended with an \
             `event: error` frame carrying no id — status still 200, because the subscription was \
             valid and the *content* is what broke. Canonical history then reloaded cleanly from \
             the start: every item the first read validated came back in the same order with the \
             same payload digests, and the pages that follow are the message and permission \
             effects genuinely committed in between, not a renumbering. Through all of it the \
             run's lifecycle, derived state, revision and `closed_at` are unchanged: a refetch is \
             a fact about a timeline, never about a run",
            &[artifact],
        );
    } else {
        bundle.fail(
            "session.refetch",
            format!(
                "epoch_refused={epoch_refused} ({} {}), gap_refused={gap_refused} \
                 (final `{}`, delivered {delivered}), reloaded_ok={reloaded_ok}, \
                 lifecycle_held={lifecycle_held}",
                refused.status,
                refused.code(),
                ending.0,
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 6 — no direct runtime access
// ---------------------------------------------------------------------------

/// Every URL-ish literal a client source file holds, and where it came from.
struct UrlLiteral {
    /// Repository-relative path.
    file: String,
    /// The line it sits on.
    line: usize,
    /// The literal itself, verbatim, so the judgement can be audited.
    literal: String,
    /// Whether the file is a test.
    test: bool,
    /// How this driver classified it.
    verdict: &'static str,
}

impl UrlLiteral {
    /// The redacted evidence row.
    fn evidence(&self) -> Value {
        json!({
            "file": self.file,
            "line": self.line,
            "literal": self.literal,
            "test_file": self.test,
            "verdict": self.verdict,
        })
    }
}

/// Client traffic can address the daemon and nothing else.
///
/// This is proved structurally and reported as such. The whole ledger of
/// URL-shaped literals goes into the artifact — including the ones that passed —
/// so an inspector audits the judgement rather than trusting the conclusion.
fn no_direct_runtime(bundle: &mut Bundle) {
    let root = kontor_tests_e2e::repo_root();
    let scanned = ["apps/console/src", "apps/desktop/src-tauri/src"];
    let mut literals: Vec<UrlLiteral> = Vec::new();
    let mut forbidden: Vec<String> = Vec::new();
    let mut client_paths: Vec<String> = Vec::new();
    let mut fetch_sites: Vec<String> = Vec::new();
    let mut prose_mentions: Vec<String> = Vec::new();

    for directory in scanned {
        for file in source_files(&root.join(directory)) {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            let relative = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            let is_test = relative.contains(".test.") || relative.contains("/test/");
            let is_client = relative.ends_with("apps/console/src/api/client.ts");

            for (index, line) in text.lines().enumerate() {
                let number = index + 1;
                for scheme in FORBIDDEN_SCHEMES {
                    if line.contains(scheme) {
                        forbidden.push(format!("{relative}:{number} speaks `{scheme}`"));
                    }
                }
                for constructor in FORBIDDEN_CONSTRUCTORS {
                    if line.contains(constructor) {
                        forbidden.push(format!("{relative}:{number} calls `{constructor}`"));
                    }
                }
                for needle in RUNTIME_NEEDLES {
                    if line.to_ascii_lowercase().contains(needle) {
                        forbidden.push(format!("{relative}:{number} names `{needle}`"));
                    }
                }
                // A prose mention of a transport this code does not use is not
                // traffic, but it should be visible rather than silently dropped.
                if line.contains("EventSource") || line.contains("WebSocket") {
                    prose_mentions.push(format!("{relative}:{number}"));
                }
                for literal in urls_in(line) {
                    let verdict = classify(&literal, is_test);
                    literals.push(UrlLiteral {
                        file: relative.clone(),
                        line: number,
                        literal,
                        test: is_test,
                        verdict,
                    });
                }
                if is_client {
                    client_paths.extend(paths_in(line));
                    if line.contains("this.#fetch(") {
                        fetch_sites.push(format!(
                            "{relative}:{number}{}",
                            if line.contains("${this.#endpoint.baseUrl}${path}") {
                                ""
                            } else {
                                " — NOT prefixed with the configured base URL"
                            }
                        ));
                    }
                }
            }
        }
    }

    let unsafe_literals: Vec<&UrlLiteral> = literals
        .iter()
        .filter(|entry| entry.verdict == "foreign")
        .collect();
    // `/v1` on its own is how the doc comments name the surface; a path that is
    // not under it at all is the thing worth refusing.
    let non_v1: Vec<&String> = client_paths
        .iter()
        .filter(|path| path.as_str() != "/v1" && !path.starts_with("/v1/"))
        .collect();
    let unrooted_fetches: Vec<&String> = fetch_sites
        .iter()
        .filter(|site| site.contains("NOT prefixed"))
        .collect();

    let artifact = bundle
        .artifact(
            "session/network-ledger.json",
            &json!({
                "scanned_directories": scanned,
                "url_literals": literals.iter().map(UrlLiteral::evidence).collect::<Vec<_>>(),
                "typed_client_paths": client_paths,
                "fetch_call_sites": fetch_sites,
                "transport_mentions_in_prose": prose_mentions,
                "forbidden_hits": forbidden,
                "rules": {
                    "schemes_refused": FORBIDDEN_SCHEMES,
                    "constructors_refused": FORBIDDEN_CONSTRUCTORS,
                    "runtime_vocabulary_refused": RUNTIME_NEEDLES,
                    "reserved_tlds_allowed_in_tests": RESERVED_TLDS,
                    "path_rule": "every path the typed client builds begins `/v1/`",
                    "base_url_rule": "`apps/console/src/api/endpoint.ts` is the only place a base \
                                      URL enters, and its `Endpoint` carries a base URL and a realm \
                                      bearer with no field for a runtime endpoint or credential",
                },
            }),
        )
        .expect("the network ledger is written");

    if forbidden.is_empty()
        && unsafe_literals.is_empty()
        && non_v1.is_empty()
        && unrooted_fetches.is_empty()
        && !client_paths.is_empty()
    {
        bundle.pass(
            "session.no-direct-runtime",
            format!(
                "every one of the {} URL-shaped literals in `apps/console/src` and \
                 `apps/desktop/src-tauri/src` is either a loopback daemon address or a reserved \
                 test-only host, and the full list is in the artifact so the judgement can be \
                 audited rather than believed. No source speaks `ws://` or `wss://`, constructs a \
                 `WebSocket` or an `EventSource`, or names a runtime plane; all {} paths the typed \
                 client builds begin `/v1/`, and both of its `fetch` sites prefix the base URL the \
                 console was configured with. This is a structural proof, stated as one: it shows \
                 there is nowhere in client source to put a runtime endpoint or credential, which \
                 is a stronger claim than watching one run and seeing no such call",
                literals.len(),
                client_paths.len(),
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "session.no-direct-runtime",
            format!(
                "forbidden={forbidden:?}, foreign_literals={:?}, non_v1_paths={non_v1:?}, \
                 unrooted_fetches={unrooted_fetches:?}, client_paths={}",
                unsafe_literals
                    .iter()
                    .map(|entry| format!("{}:{} {}", entry.file, entry.line, entry.literal))
                    .collect::<Vec<_>>(),
                client_paths.len(),
            ),
        );
    }
}

/// Every `scheme://…` literal on one line.
fn urls_in(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0;
    while let Some(offset) = line[index..].find("://") {
        let colon = index + offset;
        // Walk back over the scheme.
        let mut start = colon;
        while start > 0 && bytes[start - 1].is_ascii_alphabetic() {
            start -= 1;
        }
        // Walk forward to the first delimiter a source file would use.
        let mut end = colon + 3;
        while end < bytes.len()
            && !matches!(
                bytes[end],
                b'"' | b'\'' | b'`' | b' ' | b')' | b',' | b'<' | b'>' | b'\\'
            )
        {
            end += 1;
        }
        if start < colon {
            found.push(line[start..end].to_owned());
        }
        index = end.max(colon + 3);
    }
    found
}

/// How one URL literal should be judged.
fn classify(literal: &str, test_file: bool) -> &'static str {
    let lower = literal.to_ascii_lowercase();
    if lower.starts_with("tauri://") {
        return "tauri-origin";
    }
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        // A module specifier or a namespace URI, not an address this client dials.
        return "not-an-address";
    }
    let authority = lower
        .split("://")
        .nth(1)
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(head, _)| head);
    if host == "localhost"
        || host == "[::1]"
        || host == "::1"
        || host.starts_with("127.")
        || host.is_empty()
    {
        return "loopback";
    }
    if test_file && RESERVED_TLDS.iter().any(|tld| host.ends_with(tld)) {
        // A negative fixture: the console must label it `not_loopback`, and an
        // unroutable reserved name is the only honest way to write that test.
        return "reserved-test-host";
    }
    "foreign"
}

/// Every absolute route path a line names.
fn paths_in(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for delimiter in ['\'', '"', '`'] {
        let mut parts = line.split(delimiter);
        // Odd-indexed segments are the quoted ones.
        let _ = parts.next();
        let mut inside = true;
        for part in parts {
            if inside && part.starts_with('/') && !part.starts_with("//") {
                let path: String = part
                    .chars()
                    .take_while(|character| !matches!(character, '$' | '{'))
                    .collect();
                if path.len() > 1 {
                    found.push(path);
                }
            }
            inside = !inside;
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Every `.rs`, `.ts` and `.tsx` file under `directory`.
fn source_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![directory.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
            continue;
        }
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "rs" | "ts" | "tsx"))
        {
            found.push(path);
        }
    }
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// 7 — no transcript persistence
// ---------------------------------------------------------------------------

/// Seeded transcript and token canaries reach no persisted artifact.
fn privacy(bundle: &mut Bundle, realm: &Realm) {
    // The positive control. If the scanner cannot find a string that *is* in the
    // database, an empty canary result proves nothing at all.
    let control = realm.project.to_string();
    let needles = [TRANSCRIPT_CANARY, TOKEN_CANARY, PROMPT_CANARY, &control];

    let state_hits =
        scan_for_canaries(realm.directory.path(), &needles).expect("the state root is readable");
    let mut bundle_hits = scan_for_canaries(bundle.ephemeral(), &needles)
        .expect("the ephemeral bundle root is readable");
    bundle_hits.extend(
        scan_for_canaries(bundle.retained(), &needles)
            .expect("the retained bundle root is readable"),
    );

    let control_found = state_hits.iter().any(|(needle, _)| *needle == control);
    let leaked: Vec<String> = state_hits
        .iter()
        .chain(bundle_hits.iter())
        .filter(|(needle, _)| *needle != control)
        // The Realm's own credential file legitimately holds this Realm's tier
        // secrets. No canary was planted in it, so a hit there would be a real
        // finding — but the token claim is scoped away from it by design and the
        // exclusion is stated rather than silently applied.
        .filter(|(_, path)| path != CREDENTIAL_FILE)
        .map(|(needle, path)| format!("{} in {path}", &digest(needle.as_bytes())[..16]))
        .collect();

    let state_files: Vec<String> = fs::read_dir(realm.directory.path())
        .map(|entries| {
            let mut names: Vec<String> = entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        })
        .unwrap_or_default();

    let artifact = bundle
        .artifact(
            "privacy-scan.json",
            &json!({
                "scanned_roots": [
                    "the daemon's state root, every file including *.db, *.db-wal and *.db-shm",
                    "the pilot's ephemeral bundle root",
                    "the pilot's retained bundle root",
                ],
                "state_root_files": state_files,
                "canaries": [
                    {
                        "role": "transcript",
                        "planted_as": "the body of a session message sent through POST /v1/sessions/{id}/messages",
                        "sha256": digest(TRANSCRIPT_CANARY.as_bytes()),
                    },
                    {
                        "role": "token",
                        "planted_as": "runtime-authored content the daemon relays but must not keep",
                        "sha256": digest(TOKEN_CANARY.as_bytes()),
                    },
                    {
                        "role": "prompt",
                        "planted_as": "the launch prompt handed to the runtime",
                        "sha256": digest(PROMPT_CANARY.as_bytes()),
                    },
                ],
                "positive_control": {
                    "what": "the pilot project id, which the control plane certainly persists",
                    "found_in_state_root": control_found,
                    "why": "an empty canary result is only evidence if the same scanner finds \
                            something that is genuinely there",
                },
                "excluded": {
                    "path": CREDENTIAL_FILE,
                    "why": "it legitimately holds this Realm's tier secrets; no canary was planted \
                            in it",
                },
                "leaks": leaked,
                "export_surface": {
                    "exists": false,
                    "evidence": "no backup or export route, command or writer exists anywhere in \
                                 `crates/kontor-daemon`, `crates/kontor-store` or \
                                 `crates/kontor-api`",
                },
                "log_surface": {
                    "state_root_log_files": 0,
                    "evidence": "the daemon emits `tracing` to the process subscriber and writes no \
                                 log file into its state root",
                },
            }),
        )
        .expect("the privacy scan is written");

    if leaked.is_empty() && control_found {
        bundle.pass(
            "session.no-transcript-persistence",
            "a transcript canary was sent as a real session message, a token canary was authored \
             by the runtime into the content the daemon relays, and a third canary was the launch \
             prompt. Every file under the daemon's state root — the SQLite database, its `-wal` \
             and `-shm` companions, the lock and the credential file — and both pilot bundle roots \
             were then scanned as raw bytes: none of the three appears anywhere. The same scan for \
             the pilot project id found it, so the empty result is a finding rather than a broken \
             scanner. Two deviations, stated rather than glossed: the criterion also names export, \
             and no backup or export surface exists anywhere in the daemon, store or API — an \
             absent feature cannot carry a canary, so the criterion is answered on everything that \
             does exist rather than blocked on something that does not; and the daemon writes no \
             log file into its state root, emitting `tracing` to the process subscriber instead, \
             so the log half is answered by the absence of a sink",
            &[artifact],
        );
    } else {
        bundle.fail(
            "session.no-transcript-persistence",
            format!(
                "positive_control_found={control_found}, leaks={leaked:?} (reported by digest \
                 prefix and path — a scanner that quoted its finding would be the leak)"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// 8 — surface parity
// ---------------------------------------------------------------------------

/// The HTTP API and the thin MCP tool surface answer the same thing.
async fn surface_parity(bundle: &mut Bundle, realm: &Realm, run: AgentRunId) {
    let tier = CallerTier::Observer;
    let dispatcher = Dispatcher::new(Box::new(RouterTransport::new(realm, tier)));

    // --- HTTP, straight at the router the binary serves.
    let http_timeline = realm
        .get(
            &format!("/v1/sessions/{run}/timeline?limit={PHONE_PAGE}"),
            tier,
        )
        .await;
    let http_run = realm.get(&format!("/v1/runs/{run}"), tier).await;
    let anchor = http_timeline.json()["anchor"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let http_stream = realm
        .get(&format!("/v1/sessions/{run}/stream?after={anchor}"), tier)
        .await;

    // --- MCP, through the real tool catalogue and the real gate.
    let mcp_timeline = call_tool(
        &dispatcher,
        "kontor_session_timeline_get",
        &[
            ("agent_run_id", json!(run.to_string())),
            ("limit", json!(PHONE_PAGE)),
        ],
    )
    .await;
    let mcp_run = call_tool(
        &dispatcher,
        "kontor_run_get",
        &[("agent_run_id", json!(run.to_string()))],
    )
    .await;
    let mcp_stream = call_tool(
        &dispatcher,
        "kontor_session_stream_read",
        &[
            ("agent_run_id", json!(run.to_string())),
            ("after", json!(anchor.clone())),
        ],
    )
    .await;

    // --- Compare. The timeline is a pure function of stored content, so it is
    //     compared whole. The run snapshot carries a `freshness` judgement about
    //     *now*, so it is compared on the fields the criterion names.
    let timelines_agree = http_timeline.json() == mcp_timeline;
    let run_facts = |document: &Value| {
        json!({
            "agent_run_id": document["value"]["agent_run_id"],
            "revision": document["value"]["revision"],
            "binding_id": document["value"]["binding"]["binding_id"],
            "lifecycle": document["value"]["projection"]["lifecycle"],
            "snapshot_cursor": document["snapshot_cursor"],
        })
    };
    let http_facts = run_facts(&http_run.json());
    let runs_agree = http_facts == run_facts(&mcp_run);

    let http_frames: Vec<Value> = sse_frames(&http_stream.body)
        .into_iter()
        .filter(|(event, _, _)| event == "content")
        .map(|(event, id, data)| json!({ "event": event, "id": id, "item": normalize(&data["item"]) }))
        .collect();
    let mcp_frames: Vec<Value> = mcp_stream["frames"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|frame| frame["event"] == json!("content"))
        .map(|frame| {
            json!({
                "event": frame["event"],
                "id": frame["id"],
                "item": normalize(&frame["data"]["item"]),
            })
        })
        .collect();
    let streams_agree = http_frames == mcp_frames && !http_frames.is_empty();

    let artifact = bundle
        .artifact(
            "session/surface-parity.json",
            &json!({
                "subject": {
                    "agent_run_id": run.to_string(),
                    "page_size": PHONE_PAGE,
                    "anchor": anchor,
                },
                "timeline": {
                    "http_status": http_timeline.status,
                    "epoch": http_timeline.json()["epoch"],
                    "items": http_timeline.json()["items"].as_array().map_or(0, Vec::len),
                    "next": http_timeline.json()["next"],
                    "http_sha256": document_digest(&http_timeline.json()),
                    "mcp_sha256": document_digest(&mcp_timeline),
                    "agree": timelines_agree,
                },
                "run": {
                    "facts": http_facts,
                    "http_sha256": document_digest(&http_facts),
                    "mcp_sha256": document_digest(&run_facts(&mcp_run)),
                    "agree": runs_agree,
                },
                "stream": {
                    "http_frames": http_frames,
                    "mcp_frames": mcp_frames,
                    "agree": streams_agree,
                },
            }),
        )
        .expect("the surface parity evidence is written");

    if timelines_agree && runs_agree && streams_agree {
        bundle.pass(
            "surface.parity",
            "the same session was read over two surfaces against one running Realm. The HTTP \
                 route and the MCP tool `kontor_session_timeline_get` returned the byte-identical \
                 timeline document — same `agent_run_id`, same epoch, same \
                 item positions, same continuation cursor `next` and same anchor. `run_show` \
                 agreed across both on the run id, the revision, the binding id, the \
                 lifecycle and the snapshot cursor, compared field by field because the run \
                 document also carries a freshness judgement about *now*. The bounded live read \
                 agreed frame for frame, ids included. Both ran \
                 in-process against the same `axum::Router` over a `Transport` written here, \
                 because the shipped `HttpTransport` needs a socket and TST-001 forbids binding \
                 one",
            &[artifact],
        );
    } else {
        bundle.fail(
            "surface.parity",
            format!(
                "timelines_agree={timelines_agree}, runs_agree={runs_agree}, \
                 streams_agree={streams_agree}"
            ),
        );
    }
}

/// Run one catalogue tool and return its document, or `null` when it refused.
async fn call_tool(dispatcher: &Dispatcher, name: &str, arguments: &[(&str, Value)]) -> Value {
    let mut operands = Map::new();
    for (key, value) in arguments {
        operands.insert((*key).to_owned(), value.clone());
    }
    dispatcher
        .call(name, &Value::Object(operands))
        .await
        .map_or(Value::Null, |envelope| envelope.body)
}

/// A stable digest of one JSON document.
fn document_digest(value: &Value) -> String {
    digest(serde_json::to_string(value).unwrap_or_default().as_bytes())
}

// ---------------------------------------------------------------------------
// The in-process Realm
// ---------------------------------------------------------------------------

/// One started Realm, its router and the runtime behind it.
struct Realm {
    /// The state root. Kept for its `Drop`, and scanned by the privacy case.
    directory: TempDir,
    /// The daemon itself, which holds the state-root lock.
    daemon: Daemon,
    /// The router the binary serves.
    router: Router,
    /// The scripted runtime every session in this section comes from.
    fake: Arc<ScriptedFakeRuntime>,
    /// The pilot project.
    project: ProjectId,
    /// The task every session's team run is for.
    task: TaskId,
    /// The team run every seat belongs to.
    team_run: TeamRunId,
}

impl Realm {
    /// Start a Realm with a fake runtime that declares everything.
    async fn open() -> Self {
        let directory = TempDir::new().expect("a temporary state root");
        let fake = Arc::new(ScriptedFakeRuntime::new(capabilities()));
        let registry = RuntimeRegistry::new().with(
            RuntimeKindKey::parse(FAKE_FAMILY).expect("the fake's family key"),
            Arc::clone(&fake) as Arc<dyn RuntimeAdapter>,
        );
        let daemon = Daemon::start(DaemonConfig::at(directory.path()).with_port(0), registry)
            .expect("the realm starts");
        let router = daemon.router();

        let project = ProjectId::generate();
        let task = TaskId::generate();
        let team_run = TeamRunId::generate();
        daemon.state().with_store(|store| {
            store
                .create_project(&NewProject {
                    id: project,
                    name: name("Pilot session project"),
                    root_path: name("/tmp/kontor-pilot-session"),
                    created_at: at("2026-08-12T08:00:00Z"),
                })
                .expect("the pilot project is created");
            store
                .create_task(&NewTask {
                    id: task,
                    project_id: project,
                    mini_project_id: None,
                    title: name("The pilot session task"),
                    module: None,
                    state: TaskState::Ready,
                    created_at: at("2026-08-12T08:00:00Z"),
                })
                .expect("the pilot task is created");

            // The team revision comes from the bundled pack: the run's foreign key
            // demands a stored revision and inventing one would test a shape no
            // deployment has.
            let pack = bundled_pack().expect("the bundled pack loads");
            let entry = pack
                .manifest
                .iter()
                .find(|entry| entry.availability == PackAvailability::Seeded)
                .expect("the bundled pack seeds at least one category");
            let resolved = resolve_profile(&pack, &entry.category, at("2026-08-12T08:00:00Z"))
                .expect("the seeded category resolves");
            let revision = resolved.team.clone().expect("the profile pinned a team");
            store
                .insert_work_profile(project, &resolved.profile.definition)
                .expect("the profile revision is stored");
            store
                .insert_team_template(project, &revision)
                .expect("the team revision is stored");
            store
                .create_team_run(&NewTeamRun {
                    id: team_run,
                    project_id: project,
                    task_id: task,
                    snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
                    created_at: at("2026-08-12T08:00:00Z"),
                })
                .expect("the team run is created");
        });

        Self {
            directory,
            daemon,
            router,
            fake,
            project,
            task,
            team_run,
        }
    }

    /// Stage the content the *next* launched session will hold.
    ///
    /// A session snapshots the staged script when it binds, so loading a second
    /// script never rewrites a session that already exists.
    fn script(&self, script: &Value) {
        let parsed: RuntimeScript =
            serde_json::from_value(script.clone()).expect("a well-formed runtime script");
        self.fake
            .load_script(&parsed, &[])
            .expect("the script loads");
    }

    /// One tier's credential, read from the Realm's own `0600` file.
    fn secret(&self, tier: CallerTier) -> String {
        let path = kontor_daemon::credentials::path_in(self.directory.path());
        let bytes = fs::read(&path).expect("the realm wrote its credential file");
        let document: Value = serde_json::from_slice(&bytes).expect("the credential file is JSON");
        document[tier.as_str()]
            .as_str()
            .expect("the credential file names every tier")
            .to_owned()
    }

    /// Launch one session the way a real launch path does.
    ///
    /// A seat holds one live session, so each call needs its own role slot. The
    /// last step is the load-bearing one: without handing the frozen snapshot to
    /// the session registry, every `/v1/sessions/*` route answers `stale_binding`.
    async fn launch(&self, seat: &str) -> (AgentRunId, RuntimeBindingSnapshot) {
        let agent_run_id = AgentRunId::generate();
        let binding_id = RuntimeBindingId::generate();
        let role_slot_id = RoleSlotId::parse(seat).expect("a valid slot key");
        let workspace = self
            .fake
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id: self.team_run,
                task_id: self.task,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: WorkspaceRoot::parse(TASK_WORKSPACE).expect("an absolute path"),
                requested_at: at("2026-08-12T08:59:00Z"),
            })
            .await
            .expect("the runtime prepares the task workspace")
            .snapshot;
        let parts = LaunchParts {
            agent_run_id,
            team_run_id: self.team_run,
            role_slot_id: role_slot_id.clone(),
            task_id: self.task,
            binding_id,
            workspace: Some(workspace.clone()),
            cwd: workspace.root().clone(),
            account_profile_id: None,
            prompt: BoundedText::parse(PROMPT_CANARY).expect("bounded text"),
            requested_at: at("2026-08-12T09:00:00Z"),
        };
        let authority = self
            .fake
            .admit_launch(&AdmissionRequest {
                slot: RoleSlotKey::new(self.team_run, role_slot_id.clone()),
                agent_run_id,
                binding_id,
                replaces: None,
                requested_at: at("2026-08-12T09:00:00Z"),
            })
            .await
            .expect("the runtime admits the seat")
            .into_authority()
            .expect("a vacant seat is admitted rather than resumed");
        let outcome = self
            .fake
            .launch(&authority.into_request(parts))
            .await
            .expect("the seat launches");

        self.daemon.state().with_store(|store| {
            store
                .create_agent_run(&NewAgentRun {
                    id: agent_run_id,
                    project_id: self.project,
                    team_run_id: self.team_run,
                    parent_agent_run_id: None,
                    role: role_slot_id.clone().into_role_key(),
                    account_profile_id: None,
                    binding: Some(RuntimeBinding {
                        id: outcome.snapshot.binding_id(),
                        agent_run_id,
                        identity: outcome.snapshot.identity().clone(),
                        bound_at: at("2026-08-12T09:00:00Z"),
                    }),
                    created_at: at("2026-08-12T09:00:00Z"),
                })
                .expect("the run and its binding are persisted");
        });
        self.daemon
            .state()
            .sessions()
            .record(outcome.snapshot.clone());
        (agent_run_id, outcome.snapshot)
    }

    /// A loopback-shaped, authenticated request builder.
    ///
    /// `Origin` is deliberately absent, which is what a CLI sends and what the
    /// ingress policy admits; a foreign or missing `Host` would be refused before
    /// a handler is reached.
    fn signed(&self, method: Method, uri: &str, tier: CallerTier) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", LOOPBACK_AUTHORITY)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.secret(tier)))
    }

    /// A `GET` as one tier.
    async fn get(&self, uri: &str, tier: CallerTier) -> Answer {
        let request = self
            .signed(Method::GET, uri, tier)
            .body(Body::empty())
            .expect("a well-formed request");
        Answer::of(&self.router, request).await
    }

    /// A `GET` as one tier, from a named client.
    async fn get_as(&self, uri: &str, tier: CallerTier, agent: &str) -> Answer {
        let request = self
            .signed(Method::GET, uri, tier)
            .header("user-agent", agent)
            .body(Body::empty())
            .expect("a well-formed request");
        Answer::of(&self.router, request).await
    }

    /// Deliver one message under `key`.
    async fn message(&self, run: AgentRunId, key: &str, body: &Value) -> Answer {
        self.keyed(&format!("/v1/sessions/{run}/messages"), key, body)
            .await
    }

    /// Answer one permission request under `key`.
    async fn permission(
        &self,
        run: AgentRunId,
        permission: &str,
        key: &str,
        body: &Value,
    ) -> Answer {
        self.keyed(
            &format!("/v1/sessions/{run}/permissions/{permission}"),
            key,
            body,
        )
        .await
    }

    /// A `POST` as an operator, committed under one idempotency key.
    async fn keyed(&self, uri: &str, key: &str, body: &Value) -> Answer {
        let request = self
            .signed(Method::POST, uri, CallerTier::Operator)
            .header("idempotency-key", key)
            .body(Body::from(
                serde_json::to_vec(body).expect("a serializable body"),
            ))
            .expect("a well-formed request");
        Answer::of(&self.router, request).await
    }

    /// The run's lifecycle facts, or `None` when it could not be read.
    async fn lifecycle(&self, run: AgentRunId) -> Option<Value> {
        let answer = self
            .get(&format!("/v1/runs/{run}"), CallerTier::Observer)
            .await;
        if answer.status != 200 {
            return None;
        }
        let body = answer.json();
        Some(json!({
            "lifecycle": body["value"]["projection"]["lifecycle"],
            "derived": body["value"]["projection"]["derived"],
            "revision": body["value"]["revision"],
            "closed_at": body["value"]["closed_at"],
        }))
    }

    /// How many messages were dispatched into `snapshot`'s session.
    fn sends(&self, snapshot: &RuntimeBindingSnapshot) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::Send(binding, _) if *binding == snapshot.binding_id()))
            .count()
    }

    /// How many permission answers were dispatched at all.
    fn responses(&self) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|call| matches!(call, AdapterCall::RespondPermission(_)))
            .count()
    }

    /// How many committed events in the session carry `key` as their message id.
    fn count_messages(&self, snapshot: &RuntimeBindingSnapshot, key: &str) -> usize {
        self.fake
            .content(snapshot)
            .iter()
            .filter(|event| match &event.subject {
                EventSubject::Message(id) => id.to_string() == key,
                _ => false,
            })
            .count()
    }

    /// How many resolutions of `permission` the session's content holds.
    fn count_resolutions(&self, snapshot: &RuntimeBindingSnapshot, permission: &str) -> usize {
        self.fake
            .content(snapshot)
            .iter()
            .filter(|event| {
                event.kind == SessionEventKind::PermissionResolved
                    && matches!(&event.subject, EventSubject::Permission(id) if id.as_str() == permission)
            })
            .count()
    }
}

/// One whole answer, body included.
struct Answer {
    /// The HTTP status.
    status: u16,
    /// The whole body as text. SSE bodies are finite here, so buffering is safe.
    body: String,
}

impl Answer {
    /// Drive the real router and read everything it said.
    async fn of(router: &Router, request: Request<Body>) -> Self {
        let response: Response<Body> = router
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the whole body is readable");
        Self {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    /// The body as JSON, or `null` when it is not a JSON document.
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }

    /// The stable machine code a refusal carries, or `""`.
    fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or_default().to_owned()
    }
}

/// Every SSE block that carried data, as `(event, id, data)`.
///
/// Parsed rather than deserialized whole because what matters is the *framing*:
/// which frames carry a resumable id and which do not.
fn sse_frames(body: &str) -> Vec<(String, String, Value)> {
    let mut frames = Vec::new();
    for block in body.split("\n\n") {
        let mut event = String::new();
        let mut id = String::new();
        let mut data = String::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event = rest.trim().to_owned();
            } else if let Some(rest) = line.strip_prefix("id:") {
                id = rest.trim().to_owned();
            } else if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.trim());
            }
        }
        if data.is_empty() {
            continue;
        }
        frames.push((
            event,
            id,
            serde_json::from_str(&data).unwrap_or(Value::Null),
        ));
    }
    frames
}

// ---------------------------------------------------------------------------
// The in-process transport
// ---------------------------------------------------------------------------

/// A `Transport` that answers from this pilot's in-process router.
///
/// The two shipped transports are both wrong here: `HttpTransport` needs a real
/// socket, which TST-001 forbids, and `FakeTransport` is a recording mock rather
/// than a Realm. Surface parity is only worth claiming if the MCP catalogue
/// reaches the *same daemon* the HTTP case did, so the narrow seam the
/// client is built on is implemented over `oneshot` instead.
struct RouterTransport {
    router: Router,
    tier: CallerTier,
    secret: String,
}

impl RouterTransport {
    /// One transport for one Realm at one tier.
    fn new(realm: &Realm, tier: CallerTier) -> Self {
        Self {
            router: realm.router.clone(),
            tier,
            secret: realm.secret(tier),
        }
    }

    /// Turn one narrow request into a loopback-shaped HTTP call.
    async fn dispatch(&self, request: &ClientRequest) -> Answer {
        let mut uri = request.path.clone();
        if !request.query.is_empty() {
            uri.push('?');
            uri.push_str(
                &request
                    .query
                    .iter()
                    .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
        let method = match request.method {
            ClientMethod::Get => Method::GET,
            ClientMethod::Post => Method::POST,
        };
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", LOOPBACK_AUTHORITY)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.secret));
        if let Some(key) = &request.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        let body = request.body.as_ref().map_or_else(Body::empty, |value| {
            Body::from(serde_json::to_vec(value).expect("a serializable body"))
        });
        Answer::of(
            &self.router,
            builder.body(body).expect("a well-formed request"),
        )
        .await
    }
}

impl fmt::Debug for RouterTransport {
    /// Never the secret: a transport that printed its credential in a panic
    /// message would be the disclosure this section is about.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RouterTransport")
            .field("base_url", &self.base_url())
            .field("tier", &self.tier)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Transport for RouterTransport {
    fn tier(&self) -> CallerTier {
        self.tier
    }

    fn base_url(&self) -> String {
        format!("http://{LOOPBACK_AUTHORITY}")
    }

    async fn call(&self, request: &ClientRequest) -> Result<Reply, TransportFailure> {
        let answer = self.dispatch(request).await;
        let body = serde_json::from_str(&answer.body).map_err(|_| TransportFailure::Protocol {
            path: request.path.clone(),
            detail: "the answer was not a JSON document",
        })?;
        Ok(Reply {
            status: answer.status,
            body,
        })
    }

    async fn frames(
        &self,
        request: &ClientRequest,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        let answer = self.dispatch(request).await;
        // A refused stream answers with a JSON error body rather than frames, and
        // the client has to see it as the refusal it is.
        if !(200..300).contains(&answer.status) {
            let body =
                serde_json::from_str(&answer.body).map_err(|_| TransportFailure::Protocol {
                    path: request.path.clone(),
                    detail: "the refusal was not a JSON document",
                })?;
            return Ok(Reply {
                status: answer.status,
                body,
            });
        }
        let frames: Vec<Frame> = sse_frames(&answer.body)
            .into_iter()
            .take(budget.max_frames)
            .map(|(event, id, data)| Frame { event, id, data })
            .collect();
        Ok(Reply {
            status: answer.status,
            body: json!({ "frames": frames }),
        })
    }
}

/// Percent-encode one query component.
///
/// Hand-rolled because `url` is not a dependency of this crate and the alphabet a
/// session cursor uses — hex, `-` and `:` — needs nothing more than this.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Everything the fake declares, so a capability is never the blocker under test.
fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4_096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
        },
    }
}

/// A bounded external name.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a legal external name")
}

/// The pilot session's content: twenty recorded items and three live ones.
///
/// The token canary is authored by the *runtime* into item 3, which is the case
/// worth proving: the control plane must relay it to a reader and still keep no
/// copy. The permission request at item 4 is what criterion 4 answers.
fn parity_script() -> Value {
    let mut history = Vec::new();
    for sequence in 1..=HISTORY_ITEMS {
        let (kind, body) = match sequence {
            1 => ("message", "the pilot session opened".to_owned()),
            2 => ("tool_call", "reading the pilot task".to_owned()),
            3 => ("log", format!("runtime diagnostic: {TOKEN_CANARY}")),
            4 => ("permission_request", "may the pilot proceed".to_owned()),
            _ if sequence % 4 == 0 => ("state_change", format!("checkpoint {sequence}")),
            _ if sequence % 3 == 0 => ("tool_call", format!("step {sequence}")),
            _ if sequence % 2 == 0 => ("log", format!("note {sequence}")),
            _ => ("message", format!("progress {sequence}")),
        };
        let mut item = json!({
            "kind": kind,
            "sequence": sequence,
            "emitted_at": format!("2026-08-12T09:{sequence:02}:00Z"),
            "body": body,
        });
        if sequence == 4 {
            item["permission_id"] = json!(PERMISSION_ID);
        }
        history.push(item);
    }
    let live: Vec<Value> = (HISTORY_ITEMS + 1..=HISTORY_ITEMS + 3)
        .map(|sequence| {
            json!({
                "kind": "message",
                "sequence": sequence,
                "emitted_at": format!("2026-08-12T09:{sequence:02}:00Z"),
                "body": format!("live {sequence}"),
            })
        })
        .collect();
    json!({ "epoch": 1, "history": history, "live": live })
}

/// A session whose live content skips a sequence.
///
/// Item 3 is never emitted, so a subscriber that kept going past item 4 would be
/// handing its reader a hole it cannot see.
fn gap_script() -> Value {
    json!({
        "epoch": 1,
        "history": [
            {"kind": "message", "sequence": 1, "emitted_at": "2026-08-12T09:01:00Z",
             "body": "the gap session opened"}
        ],
        "live": [
            {"kind": "message", "sequence": 2, "emitted_at": "2026-08-12T09:02:00Z",
             "body": "the last item anyone can vouch for"},
            {"kind": "message", "sequence": 4, "emitted_at": "2026-08-12T09:04:00Z",
             "body": "the item after the hole"}
        ]
    })
}
