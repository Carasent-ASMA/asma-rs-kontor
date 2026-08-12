//! A scripted, recording transport: the mock daemon this crate's own claims are
//! proved against.
//!
//! It is public and not `#[cfg(test)]`, for the same reason
//! `kontor_runtime::fake::ScriptedFakeRuntime` is: the CLI has to run the same
//! assertions against the same fake, and a fake behind a `cfg` is a fake each
//! crate rewrites.
//!
//! # What it exists to prove
//!
//! One thing above all: that a refusal happened **before dispatch**. Every request
//! that reaches this transport is recorded, so a test can assert an observer's
//! mutation attempt left *no* record — which is a claim a real server could not
//! support, because a real server only sees the requests that were sent. Binding a
//! socket to find out would also break TST-001; nothing in this workspace does.
//!
//! It answers `/v1/realm` on its own so a test does not have to script the identity
//! read every call path performs, and it answers anything unscripted with an empty
//! realm-qualified document — enough to satisfy the envelope rules without
//! pretending to be a store.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::client::{
    CallerTier, Frame, FrameBudget, Method, Reply, Request, Transport, TransportFailure,
};

/// A fixed Realm identity for fixtures, so two fakes are two Realms only when a
/// test says so.
pub const FIXTURE_REALM: &str = "0192f0c0-0000-7000-8000-00000000fa11";

/// One answer a fake was told to give.
#[derive(Debug, Clone)]
pub struct Scripted {
    /// The status to answer with.
    pub status: u16,
    /// The body to answer with.
    pub body: serde_json::Value,
}

/// A transport that records what it was asked and answers from a script.
pub struct FakeTransport {
    tier: CallerTier,
    realm_id: String,
    scripted: Mutex<VecDeque<Scripted>>,
    recorded: Mutex<Vec<Request>>,
    unreachable: Mutex<bool>,
}

impl std::fmt::Debug for FakeTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FakeTransport")
            .field("tier", &self.tier)
            .field("dispatched", &self.dispatched())
            .finish_non_exhaustive()
    }
}

impl FakeTransport {
    /// A fake that answers for [`FIXTURE_REALM`] at `tier`.
    #[must_use]
    pub fn new(tier: CallerTier) -> Self {
        Self::in_realm(tier, FIXTURE_REALM)
    }

    /// A fake that answers for a named Realm.
    #[must_use]
    pub fn in_realm(tier: CallerTier, realm_id: &str) -> Self {
        Self {
            tier,
            realm_id: realm_id.to_owned(),
            scripted: Mutex::new(VecDeque::new()),
            recorded: Mutex::new(Vec::new()),
            unreachable: Mutex::new(false),
        }
    }

    /// Queue one answer. Answers are given in the order they were queued.
    pub fn push(&self, status: u16, body: serde_json::Value) {
        self.locked(&self.scripted)
            .push_back(Scripted { status, body });
    }

    /// Queue one successful answer, stamped with this fake's Realm.
    ///
    /// The stamp is what most tests want: every answer of this contract names its
    /// Realm, and a fixture that forgot to would be testing a body the daemon
    /// cannot produce.
    pub fn push_ok(&self, body: serde_json::Value) {
        let mut stamped = body;
        if let Some(object) = stamped.as_object_mut() {
            object
                .entry("realm_id")
                .or_insert_with(|| serde_json::Value::String(self.realm_id.clone()));
        }
        self.push(200, stamped);
    }

    /// Queue one refusal in the daemon's own envelope shape.
    pub fn push_refusal(&self, status: u16, code: &str, rule: &str) {
        self.push(
            status,
            serde_json::json!({
                "realm_id": self.realm_id,
                "code": code,
                "rule": rule,
                "current_revision": serde_json::Value::Null,
                "oldest_retained_cursor": serde_json::Value::Null,
                "newest_cursor": serde_json::Value::Null,
            }),
        );
    }

    /// Make every later call fail as if nothing were listening.
    pub fn go_unreachable(&self) {
        *self.locked(&self.unreachable) = true;
    }

    /// Every request this transport was asked to make, in order.
    #[must_use]
    pub fn recorded(&self) -> Vec<Request> {
        self.locked(&self.recorded).clone()
    }

    /// How many requests reached this transport.
    #[must_use]
    pub fn dispatched(&self) -> usize {
        self.locked(&self.recorded).len()
    }

    /// How many *writes* reached this transport.
    ///
    /// The number a mutation test asserts is zero.
    #[must_use]
    pub fn writes(&self) -> usize {
        self.locked(&self.recorded)
            .iter()
            .filter(|request| request.method == Method::Post)
            .count()
    }

    /// The Realm this fake answers for.
    #[must_use]
    pub fn realm_id(&self) -> &str {
        &self.realm_id
    }

    /// Lock one field, recovering a poisoned lock rather than propagating it.
    fn locked<'a, T>(&self, field: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
        field
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Record one request and produce the answer it is owed.
    fn answer(&self, request: &Request, streaming: bool) -> Result<Reply, TransportFailure> {
        self.locked(&self.recorded).push(request.clone());
        if *self.locked(&self.unreachable) {
            return Err(TransportFailure::Unreachable {
                base_url: self.base_url(),
            });
        }
        if let Some(scripted) = self.locked(&self.scripted).pop_front() {
            return Ok(Reply {
                status: scripted.status,
                body: scripted.body,
            });
        }
        // Unscripted: a realm-qualified, empty answer. Enough for the envelope
        // rules, and obviously not a store.
        let mut body = serde_json::json!({ "realm_id": self.realm_id });
        if streaming {
            body["frames"] = serde_json::Value::Array(Vec::new());
        }
        Ok(Reply { status: 200, body })
    }
}

#[async_trait::async_trait]
impl Transport for FakeTransport {
    fn tier(&self) -> CallerTier {
        self.tier
    }

    fn base_url(&self) -> String {
        "http://127.0.0.1:7717".to_owned()
    }

    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure> {
        self.answer(request, false)
    }

    async fn frames(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        let reply = self.answer(request, true)?;
        // A scripted stream may name more frames than the caller's budget allows.
        // Truncating here is what the real transport does when it stops reading, so
        // a budget test exercises the same rule against either one.
        let Some(frames) = reply.body.get("frames").and_then(|value| value.as_array()) else {
            return Ok(reply);
        };
        let kept: Vec<serde_json::Value> = frames.iter().take(budget.max_frames).cloned().collect();
        let mut body = reply.body.clone();
        body["frames"] = serde_json::Value::Array(kept);
        Ok(Reply {
            status: reply.status,
            body,
        })
    }
}

/// A shared fake is itself a transport.
///
/// `RealmClient` takes ownership of its transport, and a test needs to inspect the
/// fake *afterwards* — what it was asked, and how much. Implementing the trait for
/// the `Arc` is what lets a test hold one half and hand the other over, without
/// every test writing the same forwarding wrapper.
#[async_trait::async_trait]
impl Transport for std::sync::Arc<FakeTransport> {
    fn tier(&self) -> CallerTier {
        FakeTransport::tier(self)
    }

    fn base_url(&self) -> String {
        FakeTransport::base_url(self)
    }

    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure> {
        FakeTransport::call(self, request).await
    }

    async fn frames(
        &self,
        request: &Request,
        budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        FakeTransport::frames(self, request, budget).await
    }
}

/// One scripted event frame, in the shape the real transport produces.
#[must_use]
pub fn frame(event: &str, id: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::to_value(Frame {
        event: event.to_owned(),
        id: id.to_owned(),
        data,
    })
    .unwrap_or(serde_json::Value::Null)
}
