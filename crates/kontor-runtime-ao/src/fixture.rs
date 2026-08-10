//! A recorded AO daemon: fixture answers in, a call ledger out.
//!
//! This is the same choice `kontor-runtime` makes with its scripted fake, and for
//! the same reason: the hard cases in this adapter are *orderings*, not payloads.
//! "The acknowledgement was lost after AO already created the session", "the
//! refusal happened before anything was dispatched", "the retry did not POST
//! again" are all claims about what crossed the wire and when, and a recorded
//! ledger is the only thing that can settle them.
//!
//! Two properties matter and are why this is a transport rather than an HTTP mock:
//!
//! * a fault fires **after** the fixture's own effect is recorded, so
//!   confirmation-unknown can be tested in the one ordering that is dangerous;
//! * the ledger keys on verb and path only, so an assertion about the wire can
//!   never accidentally quote a prompt or a message body.
//!
//! It is public because the contract suite lives in another crate. Nothing here
//! has an opinion about AO's behavior — every answer comes from a fixture file.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

use crate::client::{AoCall, AoMethod, AoReply, AoTransport};

/// One queued answer for a route.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Queued {
    /// Answer with this reply.
    Reply(AoReply),
    /// Fail at the channel, *after* AO would already have committed the effect.
    ///
    /// This is the lost-acknowledgement shape: the mutation happened, the answer
    /// did not arrive. An adapter that treats it as "did not happen" and retries
    /// is exactly the defect the fixtures exist to catch.
    LoseAcknowledgement,
}

/// A recorded AO daemon.
#[derive(Debug)]
pub struct RecordedAo {
    /// Queued answers per `"VERB /path"`, consumed in order. The last reply for a
    /// route is reused once the queue drains, so a test only queues the answers
    /// whose *sequence* it cares about.
    replies: Mutex<BTreeMap<String, VecDeque<Queued>>>,
    sticky: Mutex<BTreeMap<String, AoReply>>,
    echo: Mutex<std::collections::BTreeSet<String>>,
    calls: Mutex<Vec<String>>,
}

impl Default for RecordedAo {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordedAo {
    /// A daemon that answers nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            replies: Mutex::new(BTreeMap::new()),
            sticky: Mutex::new(BTreeMap::new()),
            echo: Mutex::new(std::collections::BTreeSet::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Answer every call of `call`'s route with `body` and HTTP 200.
    #[must_use]
    pub fn answering(self, call: &AoCall, body: &str) -> Self {
        self.status(call, 200, body)
    }

    /// Answer every call of `call`'s route with `status` and `body`.
    #[must_use]
    pub fn status(self, call: &AoCall, status: u16, body: &str) -> Self {
        self.lock_sticky()
            .insert(call.route(), AoReply::new(status, body.to_owned()));
        self
    }

    /// Answer the *next* call of `call`'s route with `body`, before falling back
    /// to the route's standing answer.
    ///
    /// This is how a session that changes between two reads is described: the
    /// first inspect sees one thing, the second sees another.
    #[must_use]
    pub fn then_answering(self, call: &AoCall, body: &str) -> Self {
        self.queue(call, Queued::Reply(AoReply::new(200, body.to_owned())))
    }

    /// Answer the *next* call of `call`'s route with an explicit status.
    ///
    /// A 4xx and a 5xx are different facts — one is AO refusing, the other is AO
    /// possibly having accepted and then failed — so a test has to be able to say
    /// which it means.
    #[must_use]
    pub fn then_answering_with_status(self, call: &AoCall, status: u16, body: &str) -> Self {
        self.queue(call, Queued::Reply(AoReply::new(status, body.to_owned())))
    }

    /// Lose the acknowledgement of the next call of `call`'s route, after the
    /// effect is considered committed on AO's side.
    #[must_use]
    pub fn losing_acknowledgement(self, call: &AoCall) -> Self {
        self.queue(call, Queued::LoseAcknowledgement)
    }

    /// Answer every follow-up on `call`'s route the way AO does, by echoing the
    /// session and the message back.
    ///
    /// A static body cannot model this endpoint. AO's `send` contract *is* the
    /// echo — the adapter compares it so an acknowledgement for one message is
    /// never read as the receipt for another — so a daemon that answered with a
    /// fixed body would make that comparison untestable in the honest direction:
    /// every send would look like a mismatch, and a test could only pass by
    /// weakening the assertion it exists to make.
    #[must_use]
    pub fn echoing_follow_up(self, call: &AoCall) -> Self {
        self.echo
            .lock()
            .expect("the fixture lock is intact")
            .insert(call.route());
        self
    }

    /// Every call made so far, as `"VERB /path"`, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.lock_calls().clone()
    }

    /// How many calls addressed `call`'s route.
    ///
    /// The acceptance rule "the POST count remains one" is this function.
    #[must_use]
    pub fn count(&self, call: &AoCall) -> usize {
        let route = call.route();
        self.lock_calls()
            .iter()
            .filter(|made| **made == route)
            .count()
    }

    /// How many calls could have changed AO.
    ///
    /// A refusal that must happen before dispatch is proved by this being zero —
    /// and by [`RecordedAo::calls`] being empty where the rule is stricter still.
    #[must_use]
    pub fn mutations(&self) -> Vec<String> {
        self.lock_calls()
            .iter()
            .filter(|made| made.starts_with(AoMethod::Post.as_str()))
            .cloned()
            .collect()
    }

    /// Forget every recorded call, keeping the queued answers.
    pub fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut self.lock_calls())
    }

    /// AO's own follow-up acknowledgement: `ok`, the session it addressed, and
    /// the message verbatim.
    fn echo_follow_up(call: &AoCall) -> AoReply {
        let session_id = call
            .path
            .trim_start_matches('/')
            .split('/')
            .nth(3)
            .unwrap_or_default()
            .to_owned();
        let message = call
            .body
            .as_deref()
            .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
            .and_then(|body| {
                body.get("message")
                    .and_then(|it| it.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        AoReply::new(
            200,
            serde_json::json!({ "ok": true, "sessionId": session_id, "message": message })
                .to_string(),
        )
    }

    fn queue(self, call: &AoCall, queued: Queued) -> Self {
        self.lock_replies()
            .entry(call.route())
            .or_default()
            .push_back(queued);
        self
    }

    fn lock_replies(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, VecDeque<Queued>>> {
        self.replies.lock().expect("the fixture lock is intact")
    }

    fn lock_sticky(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, AoReply>> {
        self.sticky.lock().expect("the fixture lock is intact")
    }

    fn lock_calls(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.calls.lock().expect("the fixture lock is intact")
    }
}

/// Share one recorded daemon between the adapter and the test that inspects it.
///
/// `AoAdapter` takes ownership of its transport, so a test that also wants to
/// read the call ledger needs a second handle to the same daemon rather than a
/// copy of it — a copy would have its own ledger and would prove nothing.
#[async_trait]
impl AoTransport for std::sync::Arc<RecordedAo> {
    async fn call(&self, call: &AoCall) -> RuntimeResult<AoReply> {
        std::sync::Arc::as_ref(self).call(call).await
    }
}

#[async_trait]
impl AoTransport for RecordedAo {
    async fn call(&self, call: &AoCall) -> RuntimeResult<AoReply> {
        let route = call.route();
        // The call is recorded *before* the answer is decided, so a lost
        // acknowledgement still counts as a call that reached AO. Recording only
        // successful calls would make the retry rule untestable: the very case
        // that must not repeat would leave no trace of having happened once.
        self.lock_calls().push(route.clone());

        let queued = self
            .lock_replies()
            .get_mut(&route)
            .and_then(VecDeque::pop_front);
        match queued {
            Some(Queued::LoseAcknowledgement) => Err(RuntimeError::Transport {
                rule: "acknowledgement was lost after the runtime may have accepted it",
            }),
            Some(Queued::Reply(reply)) => Ok(reply),
            // A route with an explicit standing answer keeps it: a test that
            // routes one is describing AO answering something the adapter must
            // refuse, and the echo default must not quietly make it well-behaved.
            None => match self.lock_sticky().get(&route).cloned() {
                Some(reply) => Ok(reply),
                None if self
                    .echo
                    .lock()
                    .expect("the fixture lock is intact")
                    .contains(&route) =>
                {
                    Ok(Self::echo_follow_up(call))
                }
                None => Err({
                    // An unrouted call is a test bug, and it must not look
                    // like an AO answer of any kind.
                    RuntimeError::Transport {
                        rule: "channel failed before the runtime answered",
                    }
                }),
            },
        }
    }
}
