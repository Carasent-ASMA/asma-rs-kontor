//! A recorded Paseo daemon: fixture answers in, a call ledger out.
//!
//! The same choice `kontor-runtime` makes with its scripted fake and
//! `kontor-runtime-ao` makes with `RecordedAo`, for the same reason: the hard
//! cases in this adapter are *orderings*, not payloads. "The acknowledgement was
//! lost after Paseo already created the agent", "the refusal happened before
//! anything was dispatched", "the retry did not run `agent run` again", "the
//! restart replayed the same message id and sent nothing" are all claims about
//! what crossed the wire and when, and a recorded ledger is the only thing that
//! can settle them.
//!
//! Three properties matter and are why this is a transport rather than a process
//! mock:
//!
//! * a fault fires **after** the fixture's own effect is recorded, so
//!   confirmation-unknown can be tested in the one ordering that is dangerous;
//! * the ledger keys on subcommand and request type only, so an assertion about
//!   the wire can never accidentally quote a prompt, a path or a title;
//! * every recorded answer is delivered as a real [`PaseoFrame`] carrying the
//!   response type the request declared and the correlation id it was sent
//!   under, so replaying a fixture exercises the same correlation and routing
//!   rules the live socket does. A misroute is a frame with the wrong id, a
//!   wrong-kind answer is a frame with the wrong type, and both are refused by
//!   the adapter's own code rather than by a special case here.
//!
//! It is public because the contract suite lives in another crate. Nothing here
//! has an opinion about Paseo's behavior — every answer comes from a fixture.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

use crate::client::{PaseoCommand, PaseoFrame, PaseoOutput, PaseoRpc, PaseoTransport};
use crate::mcp::PaseoMcp;
use crate::wire::{PASEO_APP_VERSION, PaseoServerInfo, REQUIRED_FEATURES};

/// One queued answer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Queued<T> {
    /// Answer with this payload.
    Answer(T),
    /// Refuse, the way Paseo refuses: nothing happened.
    Refuse,
    /// Fail at the channel, *after* Paseo would already have committed the
    /// effect.
    ///
    /// This is the lost-acknowledgement shape. An adapter that treats it as "did
    /// not happen" and retries is exactly the defect the fixtures exist to
    /// catch, and the fixture's own inventory keeps reporting the effect so the
    /// census that must run before the retry has something true to find.
    LoseAcknowledgement,
    /// Answer, but correlated to another request.
    Misroute,
    /// Answer with the right correlation id and the wrong response type.
    ///
    /// The frame a same-id check accepts and a type check does not: on a
    /// multiplexed socket the daemon really can stamp an `rpc_error`, or the
    /// answer to a different question, with the id that is pending.
    WrongResponseType,
}

/// One agent's canonical content, as a daemon that actually keeps one would.
///
/// A static body cannot model this surface, for the same reason AO's follow-up
/// endpoint could not: Paseo's send *contract* is that the caller's `messageId`
/// comes back as the `clientMessageId` of the resulting user message, and that
/// is precisely what the adapter derives an acknowledgement from. A daemon
/// answering a fixed page would make exactly-once untestable in the honest
/// direction — every send would look unconfirmed, and a test could only pass by
/// weakening the assertion it exists to make.
#[derive(Debug, Clone)]
struct Journal {
    epoch: String,
    entries: Vec<serde_json::Value>,
}

impl Journal {
    fn append(&mut self, item: serde_json::Value) -> u64 {
        let seq = self.entries.len() as u64 + 1;
        self.entries.push(serde_json::json!({
            "item": item,
            "timestamp": "2026-08-10T09:30:00.000Z",
            "seqStart": seq,
            "seqEnd": seq,
            "sourceSeqRanges": [{ "startSeq": seq, "endSeq": seq }],
            "collapsed": [],
        }));
        seq
    }

    /// One page in `direction` from `cursor`, at most `limit` entries.
    fn page(
        &self,
        agent_id: &str,
        cursor: Option<u64>,
        limit: usize,
        projection: &str,
        direction: &str,
    ) -> serde_json::Value {
        // The projection is honored rather than ignored, and that is the point
        // of modelling it: `projected` folds a tool lifecycle into one entry
        // spanning both native sequences, exactly as the live daemon does. A
        // fixture that returned canonical entries whatever was asked for would
        // make "always read canonical" an untestable claim — the adapter could
        // ask for either and no assertion could tell.
        let entries = if projection == "projected" {
            collapse_tool_lifecycles(&self.entries)
        } else {
            self.entries.clone()
        };
        let start = cursor.unwrap_or(0) as usize;
        let remaining = entries.get(start..).unwrap_or_default();
        let taken = &remaining[..remaining.len().min(limit.max(1))];
        let has_newer = taken.len() < remaining.len();
        let cursor_at = |entry: Option<&serde_json::Value>, field: &str| {
            entry.map(|entry| {
                serde_json::json!({
                    "epoch": self.epoch,
                    "seq": entry[field].as_u64().unwrap_or_default(),
                })
            })
        };
        serde_json::json!({
            "agentId": agent_id,
            "agent": serde_json::Value::Null,
            "direction": direction,
            "projection": projection,
            "epoch": self.epoch,
            "reset": false,
            "staleCursor": false,
            "gap": false,
            "window": {
                "minSeq": taken.first().map_or(0, |entry| entry["seqStart"].as_u64().unwrap_or_default()),
                "maxSeq": taken.last().map_or(0, |entry| entry["seqEnd"].as_u64().unwrap_or_default()),
                "nextSeq": taken.last().map_or(1, |entry| entry["seqEnd"].as_u64().unwrap_or_default() + 1),
            },
            "startCursor": cursor_at(taken.first(), "seqStart"),
            "endCursor": cursor_at(taken.last(), "seqEnd"),
            "hasOlder": start > 0,
            "hasNewer": has_newer,
            "entries": taken,
            "error": serde_json::Value::Null,
        })
    }
}

/// Fold each `tool_call` immediately followed by a second `tool_call` for the
/// same call id into one entry covering both native sequences.
fn collapse_tool_lifecycles(entries: &[serde_json::Value]) -> Vec<serde_json::Value> {
    fn call_id(entry: &serde_json::Value) -> Option<&str> {
        (entry["item"]["type"].as_str()? == "tool_call")
            .then(|| entry["item"]["callId"].as_str())
            .flatten()
    }
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(entries.len());
    let mut index = 0;
    while index < entries.len() {
        let same_call = call_id(&entries[index]).is_some()
            && entries
                .get(index + 1)
                .and_then(call_id)
                .is_some_and(|next| Some(next) == call_id(&entries[index]));
        if same_call {
            let mut folded = entries[index].clone();
            let start = folded["seqStart"].as_u64().unwrap_or_default();
            let end = entries[index + 1]["seqEnd"].as_u64().unwrap_or_default();
            folded["seqEnd"] = serde_json::json!(end);
            folded["sourceSeqRanges"] = serde_json::json!([{ "startSeq": start, "endSeq": end }]);
            folded["collapsed"] = serde_json::json!(["tool_lifecycle"]);
            out.push(folded);
            index += 2;
        } else {
            out.push(entries[index].clone());
            index += 1;
        }
    }
    out
}

/// A recorded Paseo daemon and CLI.
#[derive(Debug)]
pub struct RecordedPaseo {
    identity: Mutex<PaseoServerInfo>,
    cli: Mutex<BTreeMap<String, VecDeque<Queued<PaseoOutput>>>>,
    cli_sticky: Mutex<BTreeMap<String, PaseoOutput>>,
    rpc: Mutex<BTreeMap<String, VecDeque<Queued<serde_json::Value>>>>,
    rpc_sticky: Mutex<BTreeMap<String, serde_json::Value>>,
    stream: Mutex<BTreeMap<String, VecDeque<serde_json::Value>>>,
    journals: Mutex<BTreeMap<String, Journal>>,
    calls: Mutex<Vec<String>>,
    mutating_routes: Mutex<BTreeSet<String>>,
    titles: Mutex<Vec<(String, String)>>,
    sent: Mutex<Vec<(String, serde_json::Value)>>,
}

impl Default for RecordedPaseo {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordedPaseo {
    /// A daemon on the pinned baseline that answers nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            identity: Mutex::new(Self::baseline_identity()),
            sent: Mutex::new(Vec::new()),
            cli: Mutex::new(BTreeMap::new()),
            cli_sticky: Mutex::new(BTreeMap::new()),
            rpc: Mutex::new(BTreeMap::new()),
            rpc_sticky: Mutex::new(BTreeMap::new()),
            stream: Mutex::new(BTreeMap::new()),
            journals: Mutex::new(BTreeMap::new()),
            calls: Mutex::new(Vec::new()),
            mutating_routes: Mutex::new(BTreeSet::new()),
            titles: Mutex::new(Vec::new()),
        }
    }

    /// The identity a pinned 0.3.1 daemon pushes after the hello.
    #[must_use]
    pub fn baseline_identity() -> PaseoServerInfo {
        PaseoServerInfo {
            server_id: "srv_kontor_fixture".to_owned(),
            version: Some(PASEO_APP_VERSION.to_owned()),
            hostname: Some("kontor-fixture-host".to_owned()),
            permissions: None,
            features: REQUIRED_FEATURES
                .iter()
                .map(|feature| (feature.as_str().to_owned(), true))
                .collect(),
        }
    }

    /// Push this identity instead, from a recorded `status/server_info`.
    #[must_use]
    pub fn announcing(self, payload: &serde_json::Value) -> Self {
        self.set_identity(payload);
        self
    }

    /// Replace the pushed identity after construction.
    ///
    /// # Panics
    /// Panics when `payload` is not a `status/server_info` payload, which in a
    /// fixture is a test bug rather than a daemon behaviour.
    pub fn set_identity(&self, payload: &serde_json::Value) {
        let parsed: PaseoServerInfo = serde_json::from_value(payload.clone())
            .expect("a recorded server_info payload is the pinned shape");
        *self.identity.lock().expect("the fixture lock is intact") = parsed;
    }

    /// Give `agent_id` a canonical journal starting empty in `epoch`.
    ///
    /// From here the daemon behaves the way Paseo's own contract says it does:
    /// an accepted `send_agent_message_request` appends a `user_message`
    /// carrying the caller's own id as `clientMessageId`, and a canonical fetch
    /// pages over the result. A queued or standing answer still wins, so a test
    /// describing a gap, a collapsed projection or an epoch change routes one
    /// and the journal stays out of its way.
    #[must_use]
    pub fn journaling(self, agent_id: &str, epoch: &str, entries: Vec<serde_json::Value>) -> Self {
        self.journals
            .lock()
            .expect("the fixture lock is intact")
            .insert(
                agent_id.to_owned(),
                Journal {
                    epoch: epoch.to_owned(),
                    entries,
                },
            );
        self
    }

    /// How many entries `agent_id`'s journal holds.
    ///
    /// "Exactly one native user message exists for this id" is this function.
    #[must_use]
    pub fn journal_len(&self, agent_id: &str) -> usize {
        self.journals
            .lock()
            .expect("the fixture lock is intact")
            .get(agent_id)
            .map_or(0, |journal| journal.entries.len())
    }

    // -- CLI ---------------------------------------------------------------

    /// Answer every invocation of `command`'s route with `stdout` and exit 0.
    #[must_use]
    pub fn answering(self, command: &PaseoCommand, stdout: &str) -> Self {
        self.set_answer(command, stdout);
        self
    }

    /// Answer the *next* invocation of `command`'s route with `stdout`.
    #[must_use]
    pub fn then_answering(self, command: &PaseoCommand, stdout: &str) -> Self {
        self.queue_answer(command, stdout);
        self
    }

    /// Lose the acknowledgement of the next invocation of `command`'s route,
    /// after the effect is considered committed on Paseo's side.
    #[must_use]
    pub fn losing_acknowledgement(self, command: &PaseoCommand) -> Self {
        self.lose_next(command);
        self
    }

    /// Set a standing CLI answer after construction.
    pub fn set_answer(&self, command: &PaseoCommand, stdout: &str) {
        self.cli_sticky
            .lock()
            .expect("the fixture lock is intact")
            .insert(
                command.route().to_owned(),
                PaseoOutput::new(0, stdout.to_owned()),
            );
    }

    /// Queue one CLI answer after construction.
    pub fn queue_answer(&self, command: &PaseoCommand, stdout: &str) {
        self.queue_cli(
            command,
            Queued::Answer(PaseoOutput::new(0, stdout.to_owned())),
        );
    }

    /// Refuse the next invocation of `command`'s route with a non-zero exit.
    pub fn refuse_next(&self, command: &PaseoCommand) {
        self.queue_cli(command, Queued::Refuse);
    }

    /// Lose the next acknowledgement on `command`'s route, after the effect.
    pub fn lose_next(&self, command: &PaseoCommand) {
        self.queue_cli(command, Queued::LoseAcknowledgement);
    }

    // -- Session protocol ---------------------------------------------------

    /// Answer every request of `request_type` with `payload`.
    #[must_use]
    pub fn answering_rpc(self, request_type: &str, payload: serde_json::Value) -> Self {
        self.set_answer_rpc(request_type, payload);
        self
    }

    /// Answer the *next* request of `request_type` with `payload`.
    ///
    /// This is how a hierarchy that changes between two reads is described: the
    /// census before a create sees nothing, the readback after it sees one.
    #[must_use]
    pub fn then_answering_rpc(self, request_type: &str, payload: serde_json::Value) -> Self {
        self.queue_answer_rpc(request_type, payload);
        self
    }

    /// Set a standing session answer after construction.
    pub fn set_answer_rpc(&self, request_type: &str, payload: serde_json::Value) {
        self.rpc_sticky
            .lock()
            .expect("the fixture lock is intact")
            .insert(request_type.to_owned(), payload);
    }

    /// Queue one session answer after construction.
    pub fn queue_answer_rpc(&self, request_type: &str, payload: serde_json::Value) {
        self.queue_rpc(request_type, Queued::Answer(payload));
    }

    /// Drop every queued answer for `request_type`, keeping the standing one.
    ///
    /// A daemon is often scripted "empty, then one" for a route a single
    /// operation calls twice. A test describing a daemon that *already* holds
    /// the row says so by forgetting the first half rather than by rebuilding
    /// the whole script.
    pub fn forget_queued_rpc(&self, request_type: &str) {
        self.rpc
            .lock()
            .expect("the fixture lock is intact")
            .remove(request_type);
    }

    /// Refuse the next request of `request_type`.
    pub fn refuse_next_rpc(&self, request_type: &str) {
        self.queue_rpc(request_type, Queued::Refuse);
    }

    /// Lose the acknowledgement of the next request of `request_type`.
    pub fn lose_next_rpc(&self, request_type: &str) {
        self.queue_rpc(request_type, Queued::LoseAcknowledgement);
    }

    /// Answer the next request of `request_type` under another request's
    /// correlation id.
    pub fn misroute_next_rpc(&self, request_type: &str) {
        self.queue_rpc(request_type, Queued::Misroute);
    }

    /// Answer the next request of `request_type` with the right correlation id
    /// and the wrong response type.
    pub fn wrong_response_type_next_rpc(&self, request_type: &str) {
        self.queue_rpc(request_type, Queued::WrongResponseType);
    }

    /// Queue unsolicited frames for one agent's selective subscription.
    ///
    /// The frames are whole `agent_stream` envelopes, exactly as the live reader
    /// buffers them, so a routing mistake in the adapter is a routing mistake
    /// here too.
    #[must_use]
    pub fn streaming(self, agent_id: &str, frames: Vec<serde_json::Value>) -> Self {
        self.push_stream(agent_id, frames);
        self
    }

    /// Queue unsolicited frames after construction.
    ///
    /// Routing is by the frame's own `payload.agentId`, not by the argument: a
    /// test that queues agent B's frame under agent A is describing the daemon
    /// misrouting, and the frame still lands where the *frame* says it belongs.
    pub fn push_stream(&self, agent_id: &str, frames: Vec<serde_json::Value>) {
        let mut streams = self.stream.lock().expect("the fixture lock is intact");
        for frame in frames {
            let routed = frame
                .get("payload")
                .and_then(|payload| payload.get("agentId"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(agent_id)
                .to_owned();
            streams.entry(routed).or_default().push_back(frame);
        }
    }

    // -- Ledger ------------------------------------------------------------

    /// Every call made so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.lock_calls().clone()
    }

    /// How many calls addressed `route`.
    ///
    /// The acceptance rule "the `agent run` count remains one" is this function.
    #[must_use]
    pub fn count(&self, route: &str) -> usize {
        self.lock_calls()
            .iter()
            .filter(|made| *made == route)
            .count()
    }

    /// The message every request of `request_type` carried, in order.
    ///
    /// Kept apart from the call ledger: the ledger answers "did this happen",
    /// this answers "what did it say". A contract that can only count calls
    /// cannot state that a create carried its permission block.
    #[must_use]
    pub fn sent_messages(&self, request_type: &str) -> Vec<serde_json::Value> {
        self.sent
            .lock()
            .expect("the fixture lock is intact")
            .iter()
            .filter(|(kind, _)| kind == request_type)
            .map(|(_, message)| message.clone())
            .collect()
    }

    /// Every title `route` was asked to give something, in order.
    ///
    /// Kept apart from the call ledger because a title is not a ledger key —
    /// but it *is* Kontor's decision, so a contract has to be able to state
    /// what a container was actually going to be called.
    #[must_use]
    pub fn titles(&self, route: &str) -> Vec<String> {
        self.titles
            .lock()
            .expect("the fixture lock is intact")
            .iter()
            .filter(|(made, _)| made == route)
            .map(|(_, title)| title.clone())
            .collect()
    }

    /// Every call that could have changed Paseo.
    ///
    /// A refusal that must happen before any effect is proved by this being
    /// empty.
    #[must_use]
    pub fn mutations(&self) -> Vec<String> {
        let mutating = self
            .mutating_routes
            .lock()
            .expect("the fixture lock is intact");
        self.lock_calls()
            .iter()
            .filter(|made| mutating.contains(*made))
            .cloned()
            .collect()
    }

    /// Forget every recorded call, keeping the queued answers.
    ///
    /// Used to state "and from *here* on, nothing was dispatched", which is the
    /// shape most restart and recovery assertions take.
    pub fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut self.lock_calls())
    }

    fn queue_cli(&self, command: &PaseoCommand, queued: Queued<PaseoOutput>) {
        self.cli
            .lock()
            .expect("the fixture lock is intact")
            .entry(command.route().to_owned())
            .or_default()
            .push_back(queued);
    }

    fn queue_rpc(&self, request_type: &str, queued: Queued<serde_json::Value>) {
        self.rpc
            .lock()
            .expect("the fixture lock is intact")
            .entry(request_type.to_owned())
            .or_default()
            .push_back(queued);
    }

    fn lock_calls(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.calls.lock().expect("the fixture lock is intact")
    }

    /// The answer a journalled agent's own contract produces, if any.
    ///
    /// A message is appended **once per distinct id**: Paseo's `messageId` is an
    /// idempotency key, so a daemon that appended a second entry for a repeated
    /// id would be modelling a runtime that does not exist — and would make the
    /// adapter's retry rule look broken when it is the fixture that is wrong.
    fn journal_answer(&self, request: &PaseoRpc) -> Option<serde_json::Value> {
        let message = &request.message;
        let agent_id = message.get("agentId")?.as_str()?.to_owned();
        let mut journals = self.journals.lock().expect("the fixture lock is intact");
        let journal = journals.get_mut(&agent_id)?;
        match request.request_type {
            "fetch_agent_timeline_request" => {
                let cursor = message
                    .get("cursor")
                    .and_then(|cursor| cursor.get("seq"))
                    .and_then(serde_json::Value::as_u64);
                let limit = message
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::from(u32::MAX)) as usize;
                let projection = message
                    .get("projection")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("canonical");
                let direction = message
                    .get("direction")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("after");
                Some(journal.page(&agent_id, cursor, limit, projection, direction))
            }
            "send_agent_message_request" => {
                let message_id = message.get("messageId")?.as_str()?.to_owned();
                let already = journal.entries.iter().any(|entry| {
                    entry["item"]["clientMessageId"].as_str() == Some(message_id.as_str())
                });
                if !already {
                    journal.append(serde_json::json!({
                        "type": "user_message",
                        "text": "synthetic text",
                        "clientMessageId": message_id,
                    }));
                }
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "accepted": true,
                    "error": serde_json::Value::Null,
                }))
            }
            _ => None,
        }
    }

    fn record(&self, route: String, mutates: bool) {
        if mutates {
            self.mutating_routes
                .lock()
                .expect("the fixture lock is intact")
                .insert(route.clone());
        }
        self.lock_calls().push(route);
    }

    /// Deliver one recorded payload as the frame the live socket would build.
    ///
    /// The recorded payload's own `requestId` is replaced by the id this request
    /// was actually sent under. A fixture cannot know the correlation id the
    /// adapter minted, and leaving a placeholder in would make every replayed
    /// answer look misrouted — which would prove the correlation rule by making
    /// it impossible to pass.
    fn deliver(request: &PaseoRpc, mut payload: serde_json::Value) -> PaseoFrame {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "requestId".to_owned(),
                serde_json::json!(request.request_id),
            );
        }
        PaseoFrame::ok(request.response_type, request.request_id.clone(), payload)
    }
}

/// Share one recorded daemon between the adapter and the test that inspects it.
///
/// `PaseoAdapter` takes ownership of its transport, so a test that also wants to
/// read the call ledger needs a second handle to the same daemon rather than a
/// copy of it — a copy would have its own ledger and would prove nothing.
#[async_trait]
impl PaseoTransport for std::sync::Arc<RecordedPaseo> {
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo> {
        std::sync::Arc::as_ref(self).server_identity().await
    }

    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        std::sync::Arc::as_ref(self).run(command).await
    }

    async fn request(&self, request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        std::sync::Arc::as_ref(self).request(request).await
    }

    async fn drain_stream(&self, agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        std::sync::Arc::as_ref(self).drain_stream(agent_id).await
    }
}

#[async_trait]
impl PaseoTransport for RecordedPaseo {
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo> {
        Ok(self
            .identity
            .lock()
            .expect("the fixture lock is intact")
            .clone())
    }

    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        // A command the live transport would refuse to dispatch is refused here
        // too, so a hostile id fails the same way against both.
        command.ensure_dispatchable()?;
        let route = command.route().to_owned();
        // The call is recorded *before* the answer is decided, so a lost
        // acknowledgement still counts as a call that reached Paseo. Recording
        // only successful calls would make the retry rule untestable: the very
        // case that must not repeat would leave no trace of having happened
        // once.
        if let Some(title) = command.title() {
            self.titles
                .lock()
                .expect("the fixture lock is intact")
                .push((route.clone(), title.to_owned()));
        }
        self.record(route.clone(), command.mutates());

        let queued = self
            .cli
            .lock()
            .expect("the fixture lock is intact")
            .get_mut(&route)
            .and_then(VecDeque::pop_front);
        match queued {
            Some(Queued::LoseAcknowledgement) => Err(RuntimeError::Transport {
                rule: "acknowledgement was lost after the runtime may have accepted it",
            }),
            Some(Queued::Refuse) => Ok(PaseoOutput::new(1, String::new())),
            Some(Queued::Answer(output)) => Ok(output),
            Some(Queued::Misroute | Queued::WrongResponseType) => Err(RuntimeError::Transport {
                rule: "answer carried another request's correlation id",
            }),
            None => match self
                .cli_sticky
                .lock()
                .expect("the fixture lock is intact")
                .get(&route)
                .cloned()
            {
                Some(output) => Ok(output),
                // An unrouted call is a test bug, and it must not look like a
                // Paseo answer of any kind.
                None => Err(RuntimeError::Transport {
                    rule: "channel failed before the runtime answered",
                }),
            },
        }
    }

    async fn request(&self, request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        let route = request.route();
        self.record(route, request.mutates);
        // The message as sent, so a contract can state what a request actually
        // carried rather than only that it was made. Recorded before the answer
        // is decided, like the call itself.
        self.sent
            .lock()
            .expect("the fixture lock is intact")
            .push((request.request_type.to_owned(), request.message.clone()));

        let queued = self
            .rpc
            .lock()
            .expect("the fixture lock is intact")
            .get_mut(request.request_type)
            .and_then(VecDeque::pop_front);
        match queued {
            Some(Queued::LoseAcknowledgement) => Err(RuntimeError::Transport {
                rule: "acknowledgement was lost after the runtime may have accepted it",
            }),
            Some(Queued::Refuse) => Ok(PaseoFrame::failed(
                request.request_id.clone(),
                "refused".to_owned(),
            )),
            Some(Queued::Answer(payload)) => Ok(RecordedPaseo::deliver(request, payload)),
            Some(Queued::Misroute) => Ok(PaseoFrame::ok(
                request.response_type,
                format!("{}-not-yours", request.request_id),
                serde_json::json!({}),
            )),
            Some(Queued::WrongResponseType) => Ok(PaseoFrame::ok(
                "fetch_workspaces_response",
                request.request_id.clone(),
                serde_json::json!({ "requestId": request.request_id, "entries": [] }),
            )),
            None => {
                let standing = self
                    .rpc_sticky
                    .lock()
                    .expect("the fixture lock is intact")
                    .get(request.request_type)
                    .cloned();
                match standing {
                    Some(payload) => Ok(RecordedPaseo::deliver(request, payload)),
                    None => match self.journal_answer(request) {
                        Some(payload) => Ok(RecordedPaseo::deliver(request, payload)),
                        None => Err(RuntimeError::Transport {
                            rule: "channel failed before the runtime answered",
                        }),
                    },
                }
            }
        }
    }

    async fn drain_stream(&self, agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        self.record("stream agent_stream".to_owned(), false);
        Ok(self
            .stream
            .lock()
            .expect("the fixture lock is intact")
            .get_mut(agent_id)
            .map(|frames| frames.drain(..).collect())
            .unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// The MCP facade
// ---------------------------------------------------------------------------

/// A recorded MCP facade: one scripted answer per tool, a call ledger out.
///
/// Deliberately as narrow as the surface it stands in for. It records the exact
/// arguments each call carried, because the whole safety argument for renaming
/// through this facade is *what is not sent*: a request carrying a directory, a
/// parent or a placement would be a re-placement, and the only way a contract can
/// state that is to read back what crossed the wire.
#[derive(Debug, Default)]
pub struct RecordedMcp {
    answers: Mutex<BTreeMap<String, serde_json::Value>>,
    failures: Mutex<BTreeSet<String>>,
    calls: Mutex<Vec<(String, serde_json::Value)>>,
}

impl RecordedMcp {
    /// A facade that answers nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer `tool` with `payload`, as the facade's result object.
    #[must_use]
    pub fn answering(self, tool: &str, payload: serde_json::Value) -> Self {
        self.answers
            .lock()
            .expect("the fixture lock is intact")
            .insert(tool.to_owned(), payload);
        self
    }

    /// Fail every call to `tool`, as an unreachable facade does.
    #[must_use]
    pub fn failing(self, tool: &str) -> Self {
        self.failures
            .lock()
            .expect("the fixture lock is intact")
            .insert(tool.to_owned());
        self
    }

    /// Every call made, in order, with the arguments it carried.
    #[must_use]
    pub fn calls(&self) -> Vec<(String, serde_json::Value)> {
        self.calls
            .lock()
            .expect("the fixture lock is intact")
            .clone()
    }

    /// The arguments every call to `tool` carried, in order.
    #[must_use]
    pub fn arguments(&self, tool: &str) -> Vec<serde_json::Value> {
        self.calls()
            .into_iter()
            .filter(|(made, _)| made == tool)
            .map(|(_, arguments)| arguments)
            .collect()
    }
}

#[async_trait]
impl PaseoMcp for RecordedMcp {
    async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> RuntimeResult<serde_json::Value> {
        // Recorded before the fault fires, for the same reason the CLI fixture
        // does it: the dangerous ordering is the one where the effect happened
        // and the answer was lost.
        self.calls
            .lock()
            .expect("the fixture lock is intact")
            .push((tool.to_owned(), arguments));
        if self
            .failures
            .lock()
            .expect("the fixture lock is intact")
            .contains(tool)
        {
            return Err(RuntimeError::Transport {
                rule: "channel failed before the runtime answered",
            });
        }
        self.answers
            .lock()
            .expect("the fixture lock is intact")
            .get(tool)
            .cloned()
            .ok_or(RuntimeError::Transport {
                rule: "the MCP facade serves no such tool",
            })
    }
}

#[async_trait]
impl PaseoMcp for std::sync::Arc<RecordedMcp> {
    async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> RuntimeResult<serde_json::Value> {
        self.as_ref().call(tool, arguments).await
    }
}
