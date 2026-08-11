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
//! Two properties matter and are why this is a transport rather than a process
//! mock:
//!
//! * a fault fires **after** the fixture's own effect is recorded, so
//!   confirmation-unknown can be tested in the one ordering that is dangerous;
//! * the ledger keys on subcommand and protocol method only, so an assertion
//!   about the wire can never accidentally quote a prompt, a path or a title.
//!
//! It is public because the contract suite lives in another crate. Nothing here
//! has an opinion about Paseo's behavior — every answer comes from a fixture.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

use crate::client::{PaseoCommand, PaseoFrame, PaseoOutput, PaseoRpc, PaseoTransport};

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
}

/// One agent's canonical content, as a daemon that actually keeps one would.
///
/// A static body cannot model this surface, for the same reason AO's follow-up
/// endpoint could not: Paseo's send *contract* is that the caller's `messageId`
/// comes back on the resulting user message, and that is precisely what the
/// adapter derives an acknowledgement from. A daemon answering a fixed page
/// would make exactly-once untestable in the honest direction — every send would
/// look unconfirmed, and a test could only pass by weakening the assertion it
/// exists to make.
#[derive(Debug, Clone)]
struct Journal {
    epoch: String,
    entries: Vec<serde_json::Value>,
}

impl Journal {
    fn append(&mut self, entry_type: &str, subject: (&str, &str)) -> u64 {
        let seq = self.entries.len() as u64 + 1;
        let (key, value) = subject;
        self.entries.push(serde_json::json!({
            "seq": seq,
            "type": entry_type,
            "at": "2026-08-10T09:30:00Z",
            "span": 1,
            "entryId": format!("ent_{seq}"),
            key: value,
        }));
        seq
    }

    /// One page after `after`, at most `limit` entries.
    fn page(
        &self,
        agent_id: &str,
        after: Option<u64>,
        limit: usize,
        projection: &str,
    ) -> serde_json::Value {
        // The projection is honored rather than ignored, and that is the point
        // of modelling it: `projected` folds a tool lifecycle into one entry
        // spanning both native sequences, exactly as the live probe recorded. A
        // fixture that returned canonical entries whatever was asked for would
        // make "always read canonical" an untestable claim — the adapter could
        // ask for either and no assertion could tell.
        let entries = if projection == "projected" {
            collapse_tool_lifecycles(&self.entries)
        } else {
            self.entries.clone()
        };
        let start = after.unwrap_or(0) as usize;
        let remaining = entries.get(start..).unwrap_or_default();
        let taken = &remaining[..remaining.len().min(limit.max(1))];
        let next_after = (taken.len() < remaining.len()).then(|| start as u64 + taken.len() as u64);
        let mut page = serde_json::json!({
            "agentId": agent_id,
            "epoch": self.epoch,
            "entries": taken,
        });
        if let Some(next) = next_after {
            page["nextAfter"] = serde_json::json!(next);
        }
        page
    }
}

/// Fold each `tool_call` immediately followed by a `tool_result` into one entry
/// covering both native sequences.
fn collapse_tool_lifecycles(entries: &[serde_json::Value]) -> Vec<serde_json::Value> {
    fn kind(entry: &serde_json::Value) -> &str {
        entry
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(entries.len());
    let mut index = 0;
    while index < entries.len() {
        let followed_by_result = entries
            .get(index + 1)
            .is_some_and(|entry| kind(entry) == "tool_result");
        if kind(&entries[index]) == "tool_call" && followed_by_result {
            let mut folded = entries[index].clone();
            folded["span"] = serde_json::json!(2);
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
    cli: Mutex<BTreeMap<String, VecDeque<Queued<PaseoOutput>>>>,
    cli_sticky: Mutex<BTreeMap<String, PaseoOutput>>,
    rpc: Mutex<BTreeMap<String, VecDeque<Queued<serde_json::Value>>>>,
    rpc_sticky: Mutex<BTreeMap<String, serde_json::Value>>,
    stream: Mutex<BTreeMap<String, VecDeque<serde_json::Value>>>,
    journals: Mutex<BTreeMap<String, Journal>>,
    calls: Mutex<Vec<String>>,
    mutating_routes: Mutex<BTreeSet<String>>,
}

impl Default for RecordedPaseo {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordedPaseo {
    /// A daemon that answers nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cli: Mutex::new(BTreeMap::new()),
            cli_sticky: Mutex::new(BTreeMap::new()),
            rpc: Mutex::new(BTreeMap::new()),
            rpc_sticky: Mutex::new(BTreeMap::new()),
            stream: Mutex::new(BTreeMap::new()),
            journals: Mutex::new(BTreeMap::new()),
            calls: Mutex::new(Vec::new()),
            mutating_routes: Mutex::new(BTreeSet::new()),
        }
    }

    /// Give `agent_id` a canonical journal starting from `entries` in `epoch`.
    ///
    /// From here the daemon behaves the way Paseo's own contract says it does:
    /// an accepted `send_agent_message_request` appends a `user_message`
    /// carrying the caller's own id, an accepted permission response appends a
    /// `permission_resolved`, and a canonical fetch pages over the result.
    /// A queued or standing answer still wins, so a test describing a gap, a
    /// collapsed projection or an epoch change routes one and the journal stays
    /// out of its way.
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

    /// Answer every invocation of `command`'s route with `json` and exit 0.
    #[must_use]
    pub fn answering(self, command: &PaseoCommand, json: &str) -> Self {
        self.set_answer(command, json);
        self
    }

    /// Answer the *next* invocation of `command`'s route with `json`.
    #[must_use]
    pub fn then_answering(self, command: &PaseoCommand, json: &str) -> Self {
        self.queue_answer(command, json);
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
    pub fn set_answer(&self, command: &PaseoCommand, json: &str) {
        self.cli_sticky
            .lock()
            .expect("the fixture lock is intact")
            .insert(
                command.route().to_owned(),
                PaseoOutput::new(0, json.to_owned()),
            );
    }

    /// Queue one CLI answer after construction.
    pub fn queue_answer(&self, command: &PaseoCommand, json: &str) {
        self.queue_cli(
            command,
            Queued::Answer(PaseoOutput::new(0, json.to_owned())),
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

    // -- Protocol ----------------------------------------------------------

    /// Answer every request of `method` with `result`.
    #[must_use]
    pub fn answering_rpc(self, method: &str, result: serde_json::Value) -> Self {
        self.set_answer_rpc(method, result);
        self
    }

    /// Answer the *next* request of `method` with `result`.
    ///
    /// This is how a hierarchy that changes between two reads is described: the
    /// census before a create sees nothing, the readback after it sees one.
    #[must_use]
    pub fn then_answering_rpc(self, method: &str, result: serde_json::Value) -> Self {
        self.queue_answer_rpc(method, result);
        self
    }

    /// Set a standing protocol answer after construction.
    pub fn set_answer_rpc(&self, method: &str, result: serde_json::Value) {
        self.rpc_sticky
            .lock()
            .expect("the fixture lock is intact")
            .insert(method.to_owned(), result);
    }

    /// Queue one protocol answer after construction.
    pub fn queue_answer_rpc(&self, method: &str, result: serde_json::Value) {
        self.queue_rpc(method, Queued::Answer(result));
    }

    /// Refuse the next request of `method`.
    pub fn refuse_next_rpc(&self, method: &str) {
        self.queue_rpc(method, Queued::Refuse);
    }

    /// Lose the acknowledgement of the next request of `method`.
    pub fn lose_next_rpc(&self, method: &str) {
        self.queue_rpc(method, Queued::LoseAcknowledgement);
    }

    /// Answer the next request of `method` under another request's correlation
    /// id.
    pub fn misroute_next_rpc(&self, method: &str) {
        self.queue_rpc(method, Queued::Misroute);
    }

    /// Queue live frames for one agent's selective subscription.
    #[must_use]
    pub fn streaming(self, agent_id: &str, frames: Vec<serde_json::Value>) -> Self {
        self.push_stream(agent_id, frames);
        self
    }

    /// Queue live frames after construction.
    pub fn push_stream(&self, agent_id: &str, frames: Vec<serde_json::Value>) {
        self.stream
            .lock()
            .expect("the fixture lock is intact")
            .entry(agent_id.to_owned())
            .or_default()
            .extend(frames);
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

    fn queue_rpc(&self, method: &str, queued: Queued<serde_json::Value>) {
        self.rpc
            .lock()
            .expect("the fixture lock is intact")
            .entry(method.to_owned())
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
        let agent_id = request.params.get("agentId")?.as_str()?.to_owned();
        let mut journals = self.journals.lock().expect("the fixture lock is intact");
        let journal = journals.get_mut(&agent_id)?;
        match request.method {
            "fetch_agent_timeline_request" => {
                let after = request
                    .params
                    .get("after")
                    .and_then(serde_json::Value::as_u64);
                let limit = request
                    .params
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::from(u32::MAX)) as usize;
                let projection = request
                    .params
                    .get("projection")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("canonical");
                Some(journal.page(&agent_id, after, limit, projection))
            }
            "setAgentTimelineSubscription" => Some(serde_json::json!({ "ok": true })),
            "send_agent_message_request" => {
                let message_id = request.params.get("messageId")?.as_str()?.to_owned();
                let already = journal.entries.iter().any(|entry| {
                    entry.get("messageId").and_then(serde_json::Value::as_str)
                        == Some(message_id.as_str())
                });
                if !already {
                    journal.append("user_message", ("messageId", &message_id));
                }
                Some(serde_json::json!({ "agentId": agent_id, "messageId": message_id }))
            }
            "agent_permission_response" => {
                let permission_id = request.params.get("permissionId")?.as_str()?.to_owned();
                let decision = request.params.get("decision")?.as_str()?.to_owned();
                let already = journal.entries.iter().any(|entry| {
                    entry.get("type").and_then(serde_json::Value::as_str)
                        == Some("permission_resolved")
                        && entry
                            .get("permissionId")
                            .and_then(serde_json::Value::as_str)
                            == Some(permission_id.as_str())
                });
                if !already {
                    journal.append("permission_resolved", ("permissionId", &permission_id));
                }
                Some(serde_json::json!({
                    "agentId": agent_id,
                    "permissionId": permission_id,
                    "decision": decision,
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
}

/// Share one recorded daemon between the adapter and the test that inspects it.
///
/// `PaseoAdapter` takes ownership of its transport, so a test that also wants to
/// read the call ledger needs a second handle to the same daemon rather than a
/// copy of it — a copy would have its own ledger and would prove nothing.
#[async_trait]
impl PaseoTransport for std::sync::Arc<RecordedPaseo> {
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
            Some(Queued::Misroute) => Err(RuntimeError::Transport {
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

        let queued = self
            .rpc
            .lock()
            .expect("the fixture lock is intact")
            .get_mut(request.method)
            .and_then(VecDeque::pop_front);
        match queued {
            Some(Queued::LoseAcknowledgement) => Err(RuntimeError::Transport {
                rule: "acknowledgement was lost after the runtime may have accepted it",
            }),
            Some(Queued::Refuse) => Ok(PaseoFrame::failed(
                request.request_id.clone(),
                "refused".to_owned(),
            )),
            Some(Queued::Answer(result)) => Ok(PaseoFrame::ok(request.request_id.clone(), result)),
            Some(Queued::Misroute) => Ok(PaseoFrame::ok(
                format!("{}-not-yours", request.request_id),
                serde_json::json!({}),
            )),
            None => {
                let standing = self
                    .rpc_sticky
                    .lock()
                    .expect("the fixture lock is intact")
                    .get(request.method)
                    .cloned();
                match standing {
                    Some(result) => Ok(PaseoFrame::ok(request.request_id.clone(), result)),
                    None => match self.journal_answer(request) {
                        Some(result) => Ok(PaseoFrame::ok(request.request_id.clone(), result)),
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
