//! A deterministic, scripted runtime that implements the whole contract.
//!
//! It exists so the control plane's hard cases can be proved without Paseo, AO,
//! Codex or any installed provider: there is no clock, no randomness, no
//! network and no child process here. Tests supply generations, native ids,
//! sequences, timestamps and faults; the same inputs always produce the same
//! outputs.
//!
//! What it can be told to do covers the failures that actually break control
//! planes: a lost acknowledgement after the effect committed, duplicate and
//! out-of-order events, epoch and sequence gaps, permission waits, orphan and
//! adoptable sessions, declared limits, a cancellation that is requested but
//! never observed, a stream that closes without a verdict, and a restart into a
//! new generation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use kontor_core::compaction::{CompactionReceipt, CompactionStatus, CompactionTelemetry};
use kontor_core::id::{
    AgentRunId, CanonicalDocument, CompactionReceiptId, ContentHash, ExternalId, ExternalName,
    RuntimeBindingId, RuntimeKindKey, SeatBindingId, TaskId, TeamRunId, Timestamp, TopologyNodeId,
    parse_utc_timestamp,
};
use kontor_core::repository::RuntimeBinding;
use kontor_core::state::{NativeRuntimeIdentity, ObservedRunState, RuntimeContact};
use serde::Deserialize;

use crate::adapter::{
    ConsultationLaunchOutcome, ConsultationLaunchRequest, LaunchOutcome, MessageAck, PermissionAck,
    RuntimeAdapter, RuntimeError, RuntimeResult,
};
use crate::admission::{
    AdmissionLedger, AdmissionOutcome, AdmissionRequest, RoleSlotKey, SeatFacts,
};
use crate::capability::{
    IssuedBinding, LimitDemand, OperationContext, RuntimeBindingSnapshot, RuntimeCapabilities,
    RuntimeCapability, RuntimeLimits, preflight,
};
use crate::container::{
    ContainerBinding, ContainerBindingSnapshot, ContainerCorrelationEvidence, ContainerOutcome,
    ContainerProjection, ContainerRequest, RetitleContainerOutcome, RetitleContainerRequest,
};
use crate::observation::{
    ControlPlaneObservation, CorrelationEvidence, NativeSession, ObservationSource,
    ReconciliationReport, reconcile,
};
use crate::request::{
    AdoptRequest, CancelRequest, CompactRequest, CorrelationLabel, HistoryRequest, InspectRequest,
    LaunchRequest, LiveSubscribeRequest, MessageId, PermissionResponseRequest, ResumeRequest,
    SendMessageRequest, capability_document,
};
use crate::timeline::{
    Admission, EventSubject, HistoryCursor, HistoryPage, LiveSubscription, MessageLedger,
    PermissionLedger, SessionEvent, SessionEventKind, TimelineBreak, TimelinePosition,
};
use crate::workspace::{
    WorkspaceBinding, WorkspaceBindingSnapshot, WorkspaceCorrelationEvidence, WorkspaceOutcome,
    WorkspacePrepareRequest, WorkspaceRoot,
};

/// One deliberate deviation from the happy path.
///
/// The queue is matched strictly: the head must belong to the operation being
/// called, otherwise the call is refused with a structural mismatch that names
/// only the two operations and never the request content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum ScriptStep {
    /// The next launch reports this raw correlation text instead of the label
    /// Kontor planted. Use it to prove a native id cannot stand in for a run id.
    EchoCorrelation {
        /// The raw text the runtime claims.
        text: String,
    },
    /// The next send commits its effect and then loses the acknowledgement.
    LoseSendAck,
    /// The next cancel returns an authoritatively observed cancellation instead
    /// of a bare acknowledgement.
    CancelObservedTerminal,
    /// The next inspect finds the bound session's native process missing while
    /// making no claim that the run itself finished.
    InspectProcessMissing,
    /// The next live subscription ends without the session reaching a terminal
    /// state.
    CloseStreamWithoutTerminal,
    /// The next call of `operation` fails at the transport.
    TransportFailure {
        /// The operation that must be called next.
        operation: RuntimeCapability,
    },
    /// The next compaction is accepted but not attested. Reuse stays blocked.
    CompactPending,
    /// The next compaction fails outright.
    CompactFailed,
    /// The next compaction comes back naming a *different* native session.
    ///
    /// This is the drift a confirmation must never paper over: the runtime
    /// replaced the session instead of compacting it.
    CompactIdentityDrift {
        /// The generation the runtime reports afterwards.
        generation: u64,
    },
    /// The next compaction reports these counters. Fields left `None` stay
    /// unknown, which is how a runtime that measures nothing is recorded.
    CompactTelemetry {
        /// Active context tokens before.
        tokens_before: Option<u64>,
        /// Active context tokens after.
        tokens_after: Option<u64>,
    },
}

impl ScriptStep {
    /// The operation this step belongs to.
    const fn operation(&self) -> RuntimeCapability {
        match self {
            Self::EchoCorrelation { .. } => RuntimeCapability::Launch,
            Self::LoseSendAck => RuntimeCapability::SendMessage,
            Self::CancelObservedTerminal => RuntimeCapability::Cancel,
            Self::InspectProcessMissing => RuntimeCapability::Inspect,
            Self::CloseStreamWithoutTerminal => RuntimeCapability::LiveEvents,
            Self::CompactPending
            | Self::CompactFailed
            | Self::CompactIdentityDrift { .. }
            | Self::CompactTelemetry { .. } => RuntimeCapability::Compact,
            Self::TransportFailure { operation } => *operation,
        }
    }
}

/// Which call of an operation a scripted step answers.
///
/// Naming the operation is not enough to make a script strict: two roles of one
/// team run call `launch` the same way, and a step queued for the first would
/// otherwise be spent on whichever reached the runtime first. A key pins the
/// step to one request.
///
/// It is only ever a *Kontor* identifier, or — for the two runtime-wide reads
/// that address nothing — the name of the read itself. A step can say which
/// call it belongs to without a script, a match or a refusal ever touching a
/// prompt, a message body or anything else the runtime carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKey {
    /// Any call of the step's operation.
    Any,
    /// Exactly the capability read. Both discovery calls declare
    /// [`RuntimeCapability::Discovery`], so naming the operation alone would
    /// let a step queued for one be spent on the other.
    Capabilities,
    /// Exactly the session enumeration.
    Sessions,
    /// Exactly this run — launch and adopt.
    Run(AgentRunId),
    /// Exactly this team run — workspace preparation.
    TeamRun(TeamRunId),
    /// Exactly this topology node — container preparation.
    Node(TopologyNodeId),
    /// Exactly this binding — resume, cancel, inspect, history and live events.
    Binding(RuntimeBindingId),
    /// Exactly this message or response — send and permission response.
    Message(MessageId),
}

impl RequestKey {
    /// What disagreed, for a refusal that carries no value.
    const fn subject(self) -> &'static str {
        match self {
            Self::Any => "request",
            Self::Capabilities | Self::Sessions => "discovery",
            Self::Run(_) => "run",
            Self::TeamRun(_) => "team run",
            Self::Node(_) => "topology node",
            Self::Binding(_) => "binding",
            Self::Message(_) => "message",
        }
    }

    /// Whether a step pinned to `self` may answer the call keyed `actual`.
    fn admits(self, actual: Self) -> bool {
        self == Self::Any || self == actual
    }
}

/// One queued deviation, and the call it is allowed to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedStep {
    step: ScriptStep,
    expected: RequestKey,
}

/// One scripted event of session content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventScript {
    /// What kind of thing happened.
    pub kind: SessionEventKind,
    /// Its position inside the epoch.
    pub sequence: u64,
    /// An epoch other than the session's, to inject a renumbering.
    #[serde(default)]
    pub epoch: Option<u64>,
    /// A permission request id this event is about.
    #[serde(default)]
    pub permission_id: Option<String>,
    /// The runtime's own event id.
    #[serde(default)]
    pub native_event_id: Option<String>,
    /// When the runtime emitted it, in canonical UTC.
    pub emitted_at: String,
    /// The payload body.
    #[serde(default)]
    pub body: String,
}

/// One scripted native session that discovery reports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionScript {
    /// The runtime's own session id.
    pub native_id: String,
    /// Offset from the runtime's current generation. `-1` makes the session
    /// belong to a generation Kontor no longer talks to.
    #[serde(default)]
    pub generation_delta: i64,
    /// Index into the correlation labels supplied when the script is loaded.
    #[serde(default)]
    pub correlation_slot: Option<usize>,
    /// Raw correlation text, for a session that reports something that is not a
    /// Kontor label at all.
    #[serde(default)]
    pub correlation_text: Option<String>,
    /// What the runtime says the session is doing.
    pub state: ObservedRunState,
    /// When it was discovered, in canonical UTC.
    pub observed_at: String,
}

/// A declarative description of everything a scenario feeds the fake.
///
/// Fixtures describe *runtime input only*: content, sessions, limits and
/// deviations. Every assertion stays in Rust.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeScript {
    /// The content epoch sessions start in.
    #[serde(default)]
    pub epoch: Option<u64>,
    /// The runtime's shared root, which is never a valid task workspace.
    #[serde(default)]
    pub runtime_root: Option<String>,
    /// Limits to declare instead of the ones the fake was built with.
    #[serde(default)]
    pub limits: Option<RuntimeLimits>,
    /// Content a launched or adopted session already has.
    #[serde(default)]
    pub history: Vec<EventScript>,
    /// Content a live subscription will deliver.
    #[serde(default)]
    pub live: Vec<EventScript>,
    /// Native sessions discovery reports beyond the ones Kontor launched.
    #[serde(default)]
    pub sessions: Vec<SessionScript>,
    /// Deviations from the happy path, in the order they must be consumed.
    #[serde(default)]
    pub steps: Vec<ScriptStep>,
}

/// What the fake was asked to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterCall {
    /// Capabilities were read.
    DiscoverCapabilities,
    /// A team run's task workspace was prepared.
    PrepareWorkspace(TeamRunId),
    /// A topology node's native container was prepared.
    PrepareContainer(TopologyNodeId),
    /// A container's visible title was corrected.
    RetitleContainer(TopologyNodeId),
    /// A container's title correction was previewed, and nothing was written.
    PreviewRetitleContainer(TopologyNodeId),
    /// A run was launched.
    Launch(AgentRunId),
    /// A read-only consultation seat was launched or recovered.
    LaunchConsultation(SeatBindingId),
    /// A binding was resumed.
    Resume(RuntimeBindingId),
    /// A message was delivered.
    Send(RuntimeBindingId, MessageId),
    /// A cancellation was requested.
    Cancel(RuntimeBindingId),
    /// A session was permanently retired for replacement.
    Retire(RuntimeBindingId),
    /// A session was inspected.
    Inspect(RuntimeBindingId),
    /// A native session was adopted.
    Adopt(AgentRunId),
    /// Native sessions were enumerated.
    DiscoverSessions,
    /// A history page was read.
    History(RuntimeBindingId),
    /// A live subscription was opened.
    SubscribeLive(RuntimeBindingId),
    /// A permission request was answered.
    RespondPermission(RuntimeBindingId),
    /// A session was asked to compact its context in place.
    Compact(RuntimeBindingId),
    /// The runtime's plane-level container was prepared.
    PreparePlane,
}

impl FakeState {
    /// Refuse any operation addressed inside a plane that was never prepared.
    ///
    /// The refusal is deliberately the *same* one a real Paseo adapter raises,
    /// so a caller cannot tell the two apart and a test written against this
    /// fake proves something about the real one.
    fn require_plane(&self) -> RuntimeResult<()> {
        if self.plane == PlaneRequirement::Unprepared {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the plane has not been prepared on this runtime host",
            });
        }
        Ok(())
    }
}

/// Whether this fake behaves like a runtime with a plane-level container.
///
/// It exists to reproduce, deterministically and in-process, the one shape a
/// real Paseo plane has and the in-memory fakes did not: *every* operation is
/// addressed inside a project that has to be created first, so a runtime whose
/// plane was never prepared refuses a census and a workspace with a refusal that
/// is indistinguishable — to the caller — from being unreachable.
///
/// The default is [`PlaneRequirement::NotRequired`], so every existing fake keeps
/// behaving exactly as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaneRequirement {
    /// This runtime has no plane-level container. Nothing to prepare.
    NotRequired,
    /// This runtime has one, and it has not been prepared yet.
    Unprepared,
    /// This runtime has one and it is ready.
    Prepared,
}

/// One native session the fake owns.
#[derive(Debug, Clone)]
struct FakeSession {
    agent_run_id: AgentRunId,
    binding_id: RuntimeBindingId,
    correlation_text: Option<String>,
    state: ObservedRunState,
    epoch: u64,
    /// Every recorded event, in delivery order.
    content: Vec<SessionEvent>,
    /// How much of `content` history already returns.
    history_len: usize,
    messages: MessageLedger<MessageAck>,
}

impl FakeSession {
    fn next_sequence(&self) -> u64 {
        self.content
            .iter()
            .map(|event| event.position.sequence)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Append a recorded event and make it immediately part of history.
    ///
    /// Appending closes the gap between history and any content that is still
    /// queued for live delivery, so a later history read stays contiguous.
    fn append(
        &mut self,
        kind: SessionEventKind,
        subject: EventSubject,
        body: &str,
        emitted_at: Timestamp,
    ) -> RuntimeResult<TimelinePosition> {
        let position = TimelinePosition {
            epoch: self.epoch,
            sequence: self.next_sequence(),
        };
        self.content.push(SessionEvent {
            kind,
            position,
            subject,
            native_event_id: None,
            emitted_at,
            payload: payload(kind, position.sequence, body)?,
        });
        self.history_len = self.content.len();
        Ok(position)
    }

    /// Every permission request this session's content raises.
    fn raised_permissions(&self) -> Vec<ExternalId> {
        self.content
            .iter()
            .filter(|event| event.kind == SessionEventKind::PermissionRequest)
            .filter_map(|event| match &event.subject {
                EventSubject::Permission(id) => Some(id.clone()),
                _ => None,
            })
            .collect()
    }
}

/// A native session discovery reports but Kontor never launched.
#[derive(Debug, Clone)]
struct ScriptedSession {
    native_id: ExternalId,
    generation: u64,
    correlation_text: Option<String>,
    state: ObservedRunState,
    observed_at: Timestamp,
}

#[derive(Debug)]
struct FakeState {
    /// Whether this runtime holds a plane-level container, and whether it has
    /// been prepared yet.
    plane: PlaneRequirement,
    /// The one root this runtime will work in, when it verifies placement.
    canonical_root: Option<WorkspaceRoot>,
    /// Role slots this runtime will not launch, by slot id.
    unlaunchable: BTreeSet<String>,
    /// Every seat whose *placement* this runtime can currently prove.
    ///
    /// Separate from `bindings` because the two are lost and recovered
    /// separately on a real plane: a restart can leave a runtime able to say
    /// *which session this is* and unable to say *where it is working*, and a
    /// driving operation needs both. Modelling only the binding is what let a
    /// read-recovers-write-does-not split go unnoticed in-process.
    placements: BTreeSet<RuntimeBindingId>,
    runtime_kind: RuntimeKindKey,
    host: ExternalName,
    generation: u64,
    epoch: u64,
    capabilities: RuntimeCapabilities,
    sessions: BTreeMap<ExternalId, FakeSession>,
    scripted_sessions: Vec<ScriptedSession>,
    staged_history: Vec<SessionEvent>,
    staged_live: Vec<SessionEvent>,
    steps: VecDeque<QueuedStep>,
    calls: Vec<AdapterCall>,
    minted: u64,
    runtime_root: WorkspaceRoot,
    /// One task workspace per team run, held as the *frozen snapshot* rather
    /// than the bare binding. This is what makes preparation idempotent, what
    /// makes a second root for the same team a conflict, and what keeps a
    /// retry from re-grading a workspace against whatever the runtime happens
    /// to advertise later.
    workspaces: BTreeMap<TeamRunId, WorkspaceBindingSnapshot>,
    /// One native container per topology node, held as the frozen snapshot for
    /// the same reasons as `workspaces` above.
    ///
    /// Deliberately *not* cleared by
    /// [`ScriptedFakeRuntime::rebuild_adapter_state`]: a native project outlives
    /// the daemon that asked for it, so a restart has to be modelled as Kontor
    /// losing its ledger while the container is still there. A fake that forgot
    /// the container too would make "re-find it by its stored native id"
    /// untestable, and that path is the whole of the restart contract.
    containers: BTreeMap<TopologyNodeId, ContainerBindingSnapshot>,
    /// Consultation seats keyed by their durable SeatBinding identity.
    consultations: BTreeMap<SeatBindingId, ConsultationLaunchOutcome>,
    /// The visible title each container currently carries.
    ///
    /// Held apart from the binding because it is the one thing about a
    /// container that may change without the binding changing — which is the
    /// whole point of a retitle, and the reason it can be read back.
    container_titles: BTreeMap<TopologyNodeId, String>,
    /// The ticket scope this plane can render a title from, per task.
    ///
    /// Stands in for what a real plane reads out of its own configuration. A task
    /// absent from this map is a task the plane has no scope for, which is a
    /// refusal rather than a title missing half its content.
    task_title_scopes: BTreeMap<TaskId, String>,
    /// Every binding this runtime has issued, held as the frozen snapshot it
    /// handed back. It is the only copy nobody outside can edit, which is what
    /// makes it — and not a caller's clone — the thing terminal evidence is
    /// judged against.
    bindings: BTreeMap<RuntimeBindingId, RuntimeBindingSnapshot>,
    /// Permission requests are tracked runtime-wide, not per session, so
    /// answering another session's request is refused as exactly that rather
    /// than looking like an unknown request.
    permissions: PermissionLedger,
    /// One entry per compaction attempt, keyed by receipt id and holding the
    /// request digest beside the receipt. The digest is what makes a replay of
    /// the same attempt return the original rather than compact a second time,
    /// and a reused id with different content a conflict.
    compactions: BTreeMap<CompactionReceiptId, (ContentHash, CompactionReceipt)>,
    /// One entry per seat, and the reason AC-4 holds. Every read and write of
    /// it happens under the single state lock, so "check the seat, then claim
    /// it" has no interleaving for a second caller to slip into.
    ///
    /// The policy is the shared [`AdmissionLedger`], not a copy of it living
    /// here: this fake and every real adapter have to enforce the *same* seat
    /// rule, and a second implementation of it would be a second thing to keep
    /// in agreement.
    admissions: AdmissionLedger,
}

/// This fake's answers to the two questions the shared ledger cannot answer.
///
/// Borrowed rather than copied out of [`FakeState`], so the facts are read in the
/// same critical section that claims the seat: a replacement decided on a session
/// state that has since changed is a replacement decided on nothing.
struct FakeSeatFacts<'a> {
    sessions: &'a BTreeMap<ExternalId, FakeSession>,
    bindings: &'a BTreeMap<RuntimeBindingId, RuntimeBindingSnapshot>,
    generation: u64,
}

impl SeatFacts for FakeSeatFacts<'_> {
    fn issued_binding(&self, binding_id: RuntimeBindingId) -> Option<RuntimeBindingSnapshot> {
        self.bindings.get(&binding_id).cloned()
    }

    fn holder_is_finished_or_retired(
        &self,
        binding_id: RuntimeBindingId,
        native_id: &ExternalId,
    ) -> bool {
        // A binding from an older generation is retired rather than finished, and
        // both are equally unable to keep the seat. A session or binding this
        // runtime no longer has is gone, which is the strongest form of either.
        let finished = match self.sessions.get(native_id) {
            None => true,
            Some(session) => session.state.observed_terminal_outcome().is_some(),
        };
        let retired = match self.bindings.get(&binding_id) {
            None => true,
            Some(snapshot) => snapshot.identity().generation != self.generation,
        };
        finished || retired
    }
}

impl FakeState {
    fn identity(&self, native_id: ExternalId) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: self.runtime_kind.clone(),
            host: self.host.clone(),
            generation: self.generation,
            native_id,
        }
    }

    /// Take the scripted deviation for this call, if the script has one.
    ///
    /// Every operation routes its script handling through here, which is what
    /// makes a scripted transport failure a fact about *the channel* for all of
    /// them rather than for the three that happened to check: a failure the
    /// script queued must never be consumed and then quietly ignored, because a
    /// silently-swallowed fault is a test that proves the opposite of what it
    /// claims.
    ///
    /// # Errors
    /// * [`RuntimeError::ScriptMismatch`] — the head of the queue belongs to a
    ///   different operation.
    /// * [`RuntimeError::ScriptRequestMismatch`] — it belongs to this operation
    ///   but to a different request.
    /// * [`RuntimeError::Transport`] — the step is a scripted transport
    ///   failure. It is consumed, and the call produces no effect.
    fn take_step(
        &mut self,
        operation: RuntimeCapability,
        actual: RequestKey,
    ) -> RuntimeResult<Option<ScriptStep>> {
        let Some(head) = self.steps.front() else {
            return Ok(None);
        };
        if head.step.operation() != operation {
            return Err(RuntimeError::ScriptMismatch {
                expected: head.step.operation().as_str(),
                called: operation.as_str(),
            });
        }
        if !head.expected.admits(actual) {
            return Err(RuntimeError::ScriptRequestMismatch {
                subject: head.expected.subject(),
            });
        }
        let queued = self.steps.pop_front().expect("the head was just inspected");
        if matches!(queued.step, ScriptStep::TransportFailure { .. }) {
            return Err(RuntimeError::Transport {
                rule: "channel failed before the runtime answered",
            });
        }
        Ok(Some(queued.step))
    }

    /// Everything a retitle decides before it writes anything.
    ///
    /// Shared by the preview and the apply so the preview cannot answer about a
    /// different container, a different title or a different refusal than the
    /// apply would: one derivation, called twice.
    ///
    /// The title is *derived*, not taken from the request — the request has no
    /// finished title to take. The rule is this fake's own and deliberately
    /// simple, but it has the shape the contract requires: the structural name
    /// the control plane rendered, plus the scope only the plane holds, and a
    /// task this plane has no scope for is refused rather than titled after the
    /// structure alone.
    ///
    /// # Errors
    /// * [`RuntimeError::StaleBinding`] — the node holds no container, or the
    ///   addressed native id and generation are not the ones bound to it.
    /// * [`RuntimeError::LaunchNotAdmitted`] — the request names a task this
    ///   fake was given no scope for.
    fn retitle_facts(
        &mut self,
        request: &RetitleContainerRequest,
        call: AdapterCall,
    ) -> RuntimeResult<(ContainerBindingSnapshot, ExternalName, String)> {
        self.require_plane()?;
        // Judged against the capabilities this container was *bound* under, the
        // same rule every other operation on it follows: a later upgrade cannot
        // retroactively license work on an older placement.
        let governing = self
            .containers
            .get(&request.topology_node_id)
            .map_or_else(|| self.capabilities.clone(), |it| it.capabilities.clone());
        preflight(
            &governing,
            &OperationContext {
                operation: RuntimeCapability::RetitleContainer,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: None,
                context_policy: None,
            },
        )?;
        self.take_step(
            RuntimeCapability::RetitleContainer,
            RequestKey::Node(request.topology_node_id),
        )?;
        self.calls.push(call);

        let snapshot = self
            .containers
            .get(&request.topology_node_id)
            .cloned()
            .ok_or(RuntimeError::StaleBinding {
                rule: "this topology node holds no native container to retitle",
            })?;
        // Addressed by the exact native id inside the exact generation. An id
        // that matched in another generation names whatever replaced the
        // container after a restart, which is not the thing Kontor bound.
        if snapshot.binding.identity.native_id != request.bound_native_id
            || snapshot.binding.identity.generation != request.generation
        {
            return Err(RuntimeError::StaleBinding {
                rule: "the addressed native container is not the one bound to this node",
            });
        }

        let desired = match request.task_id {
            Some(task_id) => {
                let scope = self.task_title_scopes.get(&task_id).ok_or(
                    RuntimeError::LaunchNotAdmitted {
                        rule: "this runtime holds no ticket scope for the task the node names",
                    },
                )?;
                ExternalName::parse(&format!("{} · {scope}", request.structural_name.as_str()))
                    .map_err(RuntimeError::Domain)?
            }
            None => request.structural_name.clone(),
        };
        let current = self
            .container_titles
            .get(&request.topology_node_id)
            .cloned()
            .unwrap_or_default();
        Ok((snapshot, desired, current))
    }

    /// The session a binding addresses, if the binding is one the runtime
    /// issued for it.
    ///
    /// A native id *addresses* a session; it does not authorize one. Looking up
    /// by native id alone would let any snapshot naming a live native id drive
    /// that session, which is precisely the check a forged binding is built to
    /// skip — so the registered binding and run are compared before the session
    /// is handed out.
    fn session(&mut self, snapshot: &RuntimeBindingSnapshot) -> RuntimeResult<&mut FakeSession> {
        let binding_id = snapshot.binding_id();
        let agent_run_id = snapshot.agent_run_id();
        let session = self
            .sessions
            .get_mut(&snapshot.identity().native_id)
            .ok_or(RuntimeError::StaleBinding {
                rule: "the runtime no longer owns this native session",
            })?;
        if session.binding_id != binding_id || session.agent_run_id != agent_run_id {
            return Err(RuntimeError::StaleBinding {
                rule: "the runtime issued a different binding for this native session",
            });
        }
        Ok(session)
    }

    /// Whether this run already holds a native session other than `excluding`.
    ///
    /// One live session per [`AgentRunId`], asked of the runtime's own session
    /// table rather than of anything the caller presents — a caller minting a
    /// fresh binding id is exactly the move it has to catch.
    ///
    /// Adoption asks it because an adopt request names no seat. Launches use the
    /// shared admission ledger's run-keyed rule instead. Re-adopting the session
    /// a run already holds re-issues that one binding — [`Self::session`] stops
    /// the superseded snapshot driving anything — so it is not a second.
    fn run_holds_other_session(
        &self,
        agent_run_id: AgentRunId,
        excluding: Option<&ExternalId>,
    ) -> bool {
        self.sessions.iter().any(|(native_id, session)| {
            session.agent_run_id == agent_run_id && Some(native_id) != excluding
        })
    }

    /// Refuse a workspace snapshot this runtime never issued.
    ///
    /// [`crate::capability::preflight`] proves the claim is internally
    /// consistent and in scope; only the runtime knows whether the workspace
    /// exists here at all. Without this, a well-formed snapshot for a workspace
    /// nobody prepared would launch work into an unverified tree.
    fn ensure_registered_workspace(
        &self,
        claimed: Option<&WorkspaceBindingSnapshot>,
    ) -> RuntimeResult<()> {
        let Some(snapshot) = claimed else {
            return Ok(());
        };
        match self.workspaces.get(&snapshot.binding.team_run_id) {
            None => Err(RuntimeError::WorkspaceMismatch {
                rule: "this runtime never prepared a task workspace for this team run",
            }),
            Some(registered) if registered.binding != snapshot.binding => {
                Err(RuntimeError::WorkspaceMismatch {
                    rule: "this is not the workspace binding the runtime issued",
                })
            }
            Some(_) => Ok(()),
        }
    }

    /// Decide one admission request and, when it is granted, claim the seat.
    ///
    /// The caller holds the state lock for the whole of this, which is what
    /// makes "look at the seat, then take it" one step. The rule itself is the
    /// shared ledger's; what this adds is the pair of facts only a runtime can
    /// answer, read off this state under that same lock.
    fn admit(&mut self, request: &AdmissionRequest) -> RuntimeResult<AdmissionOutcome> {
        let facts = FakeSeatFacts {
            sessions: &self.sessions,
            bindings: &self.bindings,
            generation: self.generation,
        };
        self.admissions.admit(request, &facts)
    }

    /// Everything a launch does once its seat has agreed to it.
    ///
    /// Separate from [`crate::RuntimeAdapter::launch`] so that one place decides what
    /// a failure costs. Each `?` below is a refusal that happens after
    /// admission, and all of them are answered by the single
    /// [`Self::release`] at the call site; spelling the release out on
    /// each path is how one of them ends up forgotten.
    fn launch_admitted(&mut self, request: &LaunchRequest) -> RuntimeResult<LaunchOutcome> {
        // The run-keyed half of AC-4 is not restated here. Every session below is
        // created in the same step that marks its seat occupied, so a session this
        // runtime holds is always a seat the shared ledger can see — and the ledger
        // has already refused the run a second seat. A copy of that rule kept here
        // would be a second place for it to be subtly different.
        let declared = self.capabilities.clone();
        let generation = self.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: request.account_profile_id().is_some(),
                binding: None,
                placement: Some(request.placement_claim()),
                current_generation: Some(generation),
                demand: Some(LimitDemand::ConcurrentSessions(
                    self.sessions.len() as u32 + 1,
                )),
                context_policy: Some(request.context_policy()),
            },
        )?;
        // The preflight proves the claim is consistent and in scope; this
        // proves the workspace is one this runtime actually made.
        self.ensure_registered_workspace(request.workspace())?;
        let step = self.take_step(
            RuntimeCapability::Launch,
            RequestKey::Run(request.agent_run_id()),
        )?;
        self.calls.push(AdapterCall::Launch(request.agent_run_id()));

        self.minted += 1;
        let native_id = ExternalId::parse(&format!("native-session-{}", self.minted))?;
        let identity = self.identity(native_id.clone());
        let reported = match step {
            Some(ScriptStep::EchoCorrelation { text }) => text,
            _ => request.correlation().to_string(),
        };
        let snapshot = ScriptedFakeRuntime::bind(
            self,
            request.agent_run_id(),
            request.binding_id(),
            identity,
            &reported,
            request.requested_at(),
        )?;

        let epoch = self.epoch;
        let mut content = self.staged_history.clone();
        let history_len = content.len();
        content.extend(self.staged_live.clone());
        let session = FakeSession {
            agent_run_id: request.agent_run_id(),
            binding_id: request.binding_id(),
            correlation_text: Some(reported),
            state: ObservedRunState::Launching,
            epoch,
            content,
            history_len,
            messages: MessageLedger::new(),
        };
        let raised = session.raised_permissions();
        // The claim becomes the session in the same critical section that creates
        // it, under the lock this method has held throughout. There is therefore
        // no instant at which a session exists and its seat is still reservable,
        // and no refusal above can leave a seat spent for a launch that never
        // happened.
        self.admissions.occupy(request, native_id.clone())?;
        self.sessions.insert(native_id, session);
        // The runtime keeps its own copy of what it just issued. Everything a
        // caller later presents is checked against this one.
        self.placements.insert(request.binding_id());
        self.bindings.insert(request.binding_id(), snapshot.clone());
        for permission_id in raised {
            self.permissions.open(request.binding_id(), permission_id);
        }

        let observation = ScriptedFakeRuntime::observation(
            &snapshot,
            RuntimeContact::Reachable,
            ObservedRunState::Launching,
            ObservationSource::CommandAck,
            0,
            request.requested_at(),
        )?;
        Ok(LaunchOutcome {
            snapshot,
            observation,
        })
    }
}

/// A runtime whose whole behavior comes from a script.
#[derive(Debug)]
pub struct ScriptedFakeRuntime {
    state: Mutex<FakeState>,
}

impl ScriptedFakeRuntime {
    /// A fake declaring `capabilities`, in generation 1 and content epoch 1.
    ///
    /// # Panics
    /// Panics if the built-in runtime kind or host name is not a valid domain
    /// value, which would be a bug in this crate rather than in a caller.
    #[must_use]
    pub fn new(capabilities: RuntimeCapabilities) -> Self {
        Self {
            state: Mutex::new(FakeState {
                plane: PlaneRequirement::NotRequired,
                canonical_root: None,
                unlaunchable: BTreeSet::new(),
                placements: BTreeSet::new(),
                runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("valid runtime kind"),
                host: ExternalName::parse("fake-host").expect("valid host name"),
                generation: 1,
                epoch: 1,
                capabilities,
                sessions: BTreeMap::new(),
                scripted_sessions: Vec::new(),
                staged_history: Vec::new(),
                staged_live: Vec::new(),
                steps: VecDeque::new(),
                calls: Vec::new(),
                minted: 0,
                runtime_root: WorkspaceRoot::parse("/fake-runtime-root")
                    .expect("valid runtime root"),
                workspaces: BTreeMap::new(),
                containers: BTreeMap::new(),
                consultations: BTreeMap::new(),
                container_titles: BTreeMap::new(),
                task_title_scopes: BTreeMap::new(),
                bindings: BTreeMap::new(),
                permissions: PermissionLedger::new(),
                compactions: BTreeMap::new(),
                admissions: AdmissionLedger::new(),
            }),
        }
    }

    /// The same fake, but holding a plane-level container that nothing has
    /// prepared yet.
    ///
    /// A runtime built this way refuses [`RuntimeAdapter::discover_sessions`]
    /// and [`RuntimeAdapter::prepare_workspace`] until
    /// [`RuntimeAdapter::prepare_plane`] has succeeded — which is exactly the
    /// shape of a real Paseo plane, and exactly the shape no in-process fake had
    /// while a whole runtime family was unreachable in the shipped composition.
    #[must_use]
    pub fn requiring_a_plane(capabilities: RuntimeCapabilities) -> Self {
        let fake = Self::new(capabilities);
        fake.lock().plane = PlaneRequirement::Unprepared;
        fake
    }

    /// Make this runtime verify *where* it is asked to work.
    ///
    /// A real plane serves one canonical worktree and refuses any other root.
    /// The default fake accepts anything, which is exactly why a control plane
    /// could synthesize a placeholder path and no in-process test noticed.
    pub fn verifying_placement_at(&self, root: WorkspaceRoot) {
        self.lock().canonical_root = Some(root);
    }

    /// Refuse to launch one declared role slot, as a real runtime with no
    /// capacity for it would.
    ///
    /// Opt-in, like [`ScriptedFakeRuntime::verifying_placement_at`]. It exists
    /// because "declared but never bound" is otherwise unreachable through the
    /// public surface — every declared slot is seated at start — and a slot that
    /// cannot occur cannot be tested. The refusal is a transport fact: no
    /// session, no binding, and the seat's reservation given back.
    pub fn refusing_launch_of(&self, slot: &kontor_core::id::RoleSlotId) {
        self.lock().unlaunchable.insert(slot.as_str().to_owned());
    }

    /// Drop everything a rebuilt adapter loses, keeping what the runtime keeps.
    ///
    /// `compose_paseo` builds every adapter from `PaseoCheckpoint::fresh`, so a
    /// daemon restart destroys the adapter's own ledgers — which bindings it
    /// issued, and where each seat is placed — while the runtime it talks to
    /// keeps running with its sessions intact. Modelling the restart *without*
    /// this leaves those ledgers populated in-process, and a test then proves
    /// only that the daemon's half recovered. That is precisely how a
    /// reads-recover-but-writes-do-not split survived a green suite.
    pub fn rebuild_adapter_state(&self) {
        let mut state = self.lock();
        state.bindings.clear();
        state.placements.clear();
        state.admissions = AdmissionLedger::new();
    }

    /// Forget that the plane was ever prepared.
    ///
    /// It is how a test isolates *which* caller prepares it: reconcile has
    /// already run, the plane is dropped underneath it, and whatever still
    /// works must be preparing the plane on its own path rather than riding
    /// startup's.
    pub fn forget_the_plane(&self) {
        let mut state = self.lock();
        if state.plane == PlaneRequirement::Prepared {
            state.plane = PlaneRequirement::Unprepared;
        }
    }

    /// Whether this fake's plane has been prepared.
    ///
    /// A fake with no plane answers `true`: there was nothing to prepare and
    /// nothing is blocked, which is the same thing a caller needs to know.
    #[must_use]
    pub fn plane_is_prepared(&self) -> bool {
        self.lock().plane != PlaneRequirement::Unprepared
    }

    /// The shared root this runtime refuses to hand out as a task workspace.
    #[must_use]
    pub fn runtime_root(&self) -> WorkspaceRoot {
        self.lock().runtime_root.clone()
    }

    /// How many distinct task workspaces the runtime has actually created.
    ///
    /// A repeated preparation must not move this number.
    #[must_use]
    pub fn workspace_count(&self) -> usize {
        self.lock().workspaces.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().expect("the fake runtime lock is intact")
    }

    /// The runtime's current generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// The capabilities the runtime currently declares.
    #[must_use]
    pub fn capabilities(&self) -> RuntimeCapabilities {
        self.lock().capabilities.clone()
    }

    /// Change what the runtime declares from now on.
    ///
    /// Bindings created earlier keep their frozen snapshot.
    pub fn set_capabilities(&self, capabilities: RuntimeCapabilities) {
        self.lock().capabilities = capabilities;
    }

    /// Restart the runtime into a new generation.
    ///
    /// Recorded effects survive — content, message ledgers and permission
    /// ledgers are all still there — but every binding made in the old
    /// generation is now stale, even though the native ids repeat.
    pub fn restart(&self) {
        let mut state = self.lock();
        state.generation += 1;
        state.steps.clear();
    }

    /// Make the runtime observe one of its sessions finish.
    ///
    /// The fixture equivalent of an authoritative terminal event arriving. It
    /// exists because "has this session finished" is the runtime's own answer,
    /// and a test about replacement has to be able to set it without pretending
    /// a cancellation happened.
    ///
    /// # Errors
    /// Returns [`RuntimeError::StaleBinding`] for a session this runtime does
    /// not own, and [`RuntimeError::Domain`] for a state that is not terminal —
    /// an observation that does not close a run cannot be used to say one
    /// closed.
    pub fn observe_terminal(
        &self,
        binding: &RuntimeBindingSnapshot,
        state: ObservedRunState,
    ) -> RuntimeResult<()> {
        if state.observed_terminal_outcome().is_none() {
            return Err(RuntimeError::Domain(kontor_core::DomainError::invalid(
                "ObservedRunState",
                "does not evidence a terminal outcome",
            )));
        }
        let mut owned = self.lock();
        let session = owned
            .sessions
            .get_mut(&binding.identity().native_id)
            .ok_or(RuntimeError::StaleBinding {
                rule: "this runtime owns no session behind that binding",
            })?;
        session.state = state;
        Ok(())
    }

    /// Queue one deviation from the happy path, for any call of its operation.
    pub fn push_step(&self, step: ScriptStep) {
        self.push_step_for(step, RequestKey::Any);
    }

    /// Queue one deviation pinned to the exact call it must answer.
    ///
    /// Use this whenever more than one request can reach the operation, so the
    /// step cannot be spent on the wrong one.
    pub fn push_step_for(&self, step: ScriptStep, expected: RequestKey) {
        self.lock().steps.push_back(QueuedStep { step, expected });
    }

    /// Load a declarative script.
    ///
    /// `correlations` resolves each scripted session's `correlation_slot`, so a
    /// fixture can describe "this session belongs to the second run" without
    /// hard-coding a generated identifier.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] when the script carries a timestamp,
    /// identifier or payload the domain refuses, and when a slot has no label.
    pub fn load_script(
        &self,
        script: &RuntimeScript,
        correlations: &[CorrelationLabel],
    ) -> RuntimeResult<()> {
        let mut state = self.lock();
        if let Some(epoch) = script.epoch {
            state.epoch = epoch;
        }
        if let Some(root) = &script.runtime_root {
            state.runtime_root = WorkspaceRoot::parse(root)?;
        }
        if let Some(limits) = script.limits {
            state.capabilities.limits = limits;
        }
        let epoch = state.epoch;
        state.staged_history = build_events(&script.history, epoch)?;
        state.staged_live = build_events(&script.live, epoch)?;
        state.scripted_sessions = script
            .sessions
            .iter()
            .map(|session| {
                let correlation_text = match session.correlation_slot {
                    Some(slot) => Some(
                        correlations
                            .get(slot)
                            .ok_or_else(|| {
                                kontor_core::DomainError::invalid(
                                    "SessionScript",
                                    "names a correlation slot the test did not supply",
                                )
                            })?
                            .to_string(),
                    ),
                    None => session.correlation_text.clone(),
                };
                Ok(ScriptedSession {
                    native_id: ExternalId::parse(&session.native_id)?,
                    generation: state
                        .generation
                        .saturating_add_signed(session.generation_delta),
                    correlation_text,
                    state: session.state,
                    observed_at: parse_utc_timestamp(&session.observed_at)?,
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        // A fixture cannot name an identifier the test generates at run time,
        // so a loaded step answers any call of its operation. Pin one with
        // [`ScriptedFakeRuntime::push_step_for`] when that matters.
        state.steps = script
            .steps
            .iter()
            .map(|step| QueuedStep {
                step: step.clone(),
                expected: RequestKey::Any,
            })
            .collect();
        Ok(())
    }

    /// Give this plane the ticket scope it renders one task's titles from.
    ///
    /// Scripted rather than derived: a fake that invented a scope for every task
    /// could not be used to prove that a plane holding none refuses.
    pub fn scope_task_title(&self, task_id: TaskId, scope: &str) {
        self.lock()
            .task_title_scopes
            .insert(task_id, scope.to_owned());
    }

    /// The title one node's container currently carries, as the runtime holds it.
    #[must_use]
    pub fn container_title(&self, topology_node_id: TopologyNodeId) -> Option<String> {
        self.lock().container_titles.get(&topology_node_id).cloned()
    }

    /// Say what one container is called now, without going through Kontor.
    ///
    /// The runtime's own state, set the way the world sets it: a container named
    /// by a rule Kontor has since corrected carries the old name, and no Kontor
    /// operation put it there. It is the only way to reproduce the state a repair
    /// exists for.
    pub fn set_container_title(&self, topology_node_id: TopologyNodeId, title: &str) {
        self.lock()
            .container_titles
            .insert(topology_node_id, title.to_owned());
    }

    /// Everything the fake has been asked to do, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<AdapterCall> {
        self.lock().calls.clone()
    }

    /// Take the recorded calls and start a fresh log.
    pub fn take_calls(&self) -> Vec<AdapterCall> {
        std::mem::take(&mut self.lock().calls)
    }

    /// Every recorded event of the session behind `binding`.
    #[must_use]
    pub fn content(&self, binding: &RuntimeBindingSnapshot) -> Vec<SessionEvent> {
        self.lock()
            .sessions
            .get(&binding.identity().native_id)
            .map(|session| session.content.clone())
            .unwrap_or_default()
    }

    /// The permission requests the runtime is still waiting on.
    #[must_use]
    pub fn pending_permissions(&self) -> BTreeSet<ExternalId> {
        self.lock().permissions.pending()
    }

    /// How many native sessions this runtime owns for one seat.
    ///
    /// The AC-4 criterion is stated on this number: whatever a caller replays,
    /// races or mints fresh identifiers for, it never rises above one.
    #[must_use]
    pub fn sessions_in(&self, slot: &RoleSlotKey) -> usize {
        let state = self.lock();
        let occupying = state.admissions.occupant(slot).cloned();
        state
            .sessions
            .keys()
            .filter(|native| Some(*native) == occupying.as_ref())
            .count()
    }

    /// Whether this runtime is holding an unspent reservation for one seat.
    #[must_use]
    pub fn is_reserved(&self, slot: &RoleSlotKey) -> bool {
        self.lock().admissions.is_reserved(slot)
    }

    /// How many native sessions this runtime owns for one agent run.
    #[must_use]
    pub fn sessions_for(&self, agent_run_id: AgentRunId) -> usize {
        self.lock()
            .sessions
            .values()
            .filter(|session| session.agent_run_id == agent_run_id)
            .count()
    }

    /// How many distinct messages the session behind `binding` committed.
    #[must_use]
    pub fn committed_messages(&self, binding: &RuntimeBindingSnapshot) -> usize {
        self.lock()
            .sessions
            .get(&binding.identity().native_id)
            .map_or(0, |session| session.messages.len())
    }

    /// Bind a native session to a run, freezing the capabilities of the moment.
    fn bind(
        state: &FakeState,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
        identity: NativeRuntimeIdentity,
        reported_correlation: &str,
        at: Timestamp,
    ) -> RuntimeResult<RuntimeBindingSnapshot> {
        let correlation = CorrelationEvidence::establish(
            agent_run_id,
            reported_correlation,
            identity.clone(),
            at,
        )?;
        Ok(RuntimeBindingSnapshot {
            binding: RuntimeBinding {
                id: binding_id,
                agent_run_id,
                identity,
                bound_at: at,
            },
            capabilities: state.capabilities.clone(),
            correlation,
        })
    }

    fn observation(
        snapshot: &RuntimeBindingSnapshot,
        contact: RuntimeContact,
        state: ObservedRunState,
        source: ObservationSource,
        native_sequence: u64,
        at: Timestamp,
    ) -> RuntimeResult<ControlPlaneObservation> {
        Ok(ControlPlaneObservation {
            agent_run_id: snapshot.agent_run_id(),
            contact,
            state,
            identity: snapshot.identity().clone(),
            native_event_id: None,
            native_sequence,
            observed_at: at,
            evidence: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "observed_state": state.as_str(),
                "contact": contact.as_str(),
                "source": source,
            }))?,
            source,
        })
    }
}

fn payload(kind: SessionEventKind, sequence: u64, body: &str) -> RuntimeResult<CanonicalDocument> {
    Ok(CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "kind": kind,
        "sequence": sequence,
        "body": body,
    }))?)
}

fn build_events(scripts: &[EventScript], epoch: u64) -> RuntimeResult<Vec<SessionEvent>> {
    scripts
        .iter()
        .map(|script| {
            let subject = match &script.permission_id {
                Some(id) => EventSubject::Permission(ExternalId::parse(id)?),
                None => EventSubject::None,
            };
            let native_event_id = script
                .native_event_id
                .as_deref()
                .map(ExternalId::parse)
                .transpose()?;
            Ok(SessionEvent {
                kind: script.kind,
                position: TimelinePosition {
                    epoch: script.epoch.unwrap_or(epoch),
                    sequence: script.sequence,
                },
                subject,
                native_event_id,
                emitted_at: parse_utc_timestamp(&script.emitted_at)?,
                payload: payload(script.kind, script.sequence, &script.body)?,
            })
        })
        .collect()
}

#[async_trait]
impl RuntimeAdapter for ScriptedFakeRuntime {
    /// The claim is compared against the registry *whole* — grade, limits,
    /// correlation and all — so a clone with a better trust grade written into
    /// it is refused rather than quietly corrected.
    async fn issued_binding(
        &self,
        claimed: &RuntimeBindingSnapshot,
    ) -> RuntimeResult<IssuedBinding> {
        let state = self.lock();
        match state.bindings.get(&claimed.binding_id()) {
            None => Err(RuntimeError::StaleBinding {
                rule: "this runtime never issued this binding",
            }),
            Some(issued) if issued != claimed => Err(RuntimeError::StaleBinding {
                rule: "this is not the binding the runtime issued",
            }),
            Some(issued) => IssuedBinding::attest(issued.clone()),
        }
    }

    async fn discover_capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        let mut state = self.lock();
        // Capability discovery is the one operation with nothing to preflight —
        // it is what a preflight is made of — but it is still a call over the
        // channel, so it goes through the same choke point as the rest.
        state.take_step(RuntimeCapability::Discovery, RequestKey::Capabilities)?;
        state.calls.push(AdapterCall::DiscoverCapabilities);
        Ok(state.capabilities.clone())
    }

    /// Re-record each snapshot whose session this fake still holds, verbatim.
    ///
    /// The same rule the real adapter keeps: the live census answers only
    /// *does this session still exist here?*, and everything else comes out of
    /// the persisted snapshot. A fake that rebuilt capabilities here would let a
    /// re-grading bug pass its own restart test.
    async fn restore_bindings(
        &self,
        snapshots: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<Vec<RuntimeBindingSnapshot>> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        let mut restored = Vec::new();
        for snapshot in snapshots {
            let identity = snapshot.identity();
            // The same four checks the real adapter makes, in the same order, so
            // a test written against this fake proves something about that one.
            if snapshot.ensure_correlated().is_err() {
                continue;
            }
            // Whole native identity: a repeated native id in a new generation is
            // a different session.
            let Some(session) = state.sessions.get(&identity.native_id) else {
                continue;
            };
            if identity.generation != generation {
                continue;
            }
            // The live session's *own* correlation. This is what refuses a
            // forged but self-consistent claim: a stored document can name any
            // run, and cannot make a running session belong to it.
            if session.agent_run_id != snapshot.agent_run_id() {
                continue;
            }
            // A claim may be weaker than the live runtime and never stronger.
            if !snapshot.within(&declared) {
                continue;
            }
            // Placement travels with the binding, as it does on the real
            // adapter: a seat restored without one reads and cannot be driven.
            state.placements.insert(snapshot.binding_id());
            state
                .bindings
                .insert(snapshot.binding_id(), snapshot.clone());
            restored.push(snapshot.clone());
        }
        Ok(restored)
    }

    async fn prepare_plane(&self) -> RuntimeResult<()> {
        let mut state = self.lock();
        state.calls.push(AdapterCall::PreparePlane);
        // Idempotent, and idempotent the way the real one is: a plane that is
        // already prepared is re-attested rather than re-created.
        if state.plane == PlaneRequirement::Unprepared {
            state.plane = PlaneRequirement::Prepared;
        }
        Ok(())
    }

    async fn prepare_workspace(
        &self,
        request: &WorkspacePrepareRequest,
    ) -> RuntimeResult<WorkspaceOutcome> {
        let mut state = self.lock();
        state.require_plane()?;
        // The same refusal a real plane raises, for the same reason: a root that
        // is not this runtime's canonical worktree is a place it will not edit.
        if let Some(canonical) = &state.canonical_root
            && &request.root != canonical
        {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the requested root is not the canonical task worktree of this plane",
            });
        }
        // A repeated preparation is governed by the snapshot it will return,
        // exactly as a bound session operation is governed by its own binding.
        // A retry after a lost answer must not start failing because the
        // runtime was downgraded in between: the work it re-answers for was
        // already verified under the frozen capabilities.
        let governing = state
            .workspaces
            .get(&request.team_run_id)
            .map_or_else(|| state.capabilities.clone(), |it| it.capabilities.clone());
        preflight(
            &governing,
            &OperationContext {
                operation: RuntimeCapability::PrepareWorkspace,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: None,
                context_policy: None,
            },
        )?;
        state.take_step(
            RuntimeCapability::PrepareWorkspace,
            RequestKey::TeamRun(request.team_run_id),
        )?;
        state
            .calls
            .push(AdapterCall::PrepareWorkspace(request.team_run_id));

        request.root.ensure_task_scoped(&state.runtime_root)?;

        // Preparation is idempotent per team run: the second call returns the
        // snapshot the first one froze — binding, capabilities and correlation
        // alike — so a retry after a lost answer cannot leave a second
        // workspace behind and cannot silently re-grade an existing one.
        // Asking for a different root for the same team run is a
        // contradiction, not a retry.
        if let Some(existing) = state.workspaces.get(&request.team_run_id) {
            if existing.binding.root != request.root {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this team run already has a task workspace at another root",
                });
            }
            if existing.binding.task_id != request.task_id {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this team run's workspace was prepared for another task",
                });
            }
            return Ok(WorkspaceOutcome {
                snapshot: existing.clone(),
                created: false,
            });
        }

        state.minted += 1;
        let native_id = ExternalId::parse(&format!("native-workspace-{}", state.minted))?;
        let identity = state.identity(native_id);
        let correlation = WorkspaceCorrelationEvidence::establish(
            request.team_run_id,
            &request.correlation().to_string(),
            identity.clone(),
            request.requested_at,
        )?;
        let binding = WorkspaceBinding {
            id: request.workspace_binding_id,
            team_run_id: request.team_run_id,
            task_id: request.task_id,
            root: request.root.clone(),
            identity,
            bound_at: request.requested_at,
        };
        // The snapshot is frozen here, once, and every later answer for this
        // team run is a clone of it.
        let snapshot = WorkspaceBindingSnapshot {
            binding,
            capabilities: state.capabilities.clone(),
            correlation,
        };
        state
            .workspaces
            .insert(request.team_run_id, snapshot.clone());
        Ok(WorkspaceOutcome {
            snapshot,
            created: true,
        })
    }

    async fn prepare_container(
        &self,
        request: &ContainerRequest,
    ) -> RuntimeResult<ContainerOutcome> {
        let mut state = self.lock();
        state.require_plane()?;
        // The shape is settled before anything is minted, and it is settled from
        // the pinned specification's capabilities. Nothing below reads the
        // node's kind.
        let projection = request.validate()?;
        let operation = match projection {
            ContainerProjection::NativeRoot => RuntimeCapability::PrepareProject,
            ContainerProjection::NativeChild => RuntimeCapability::PrepareWorkspace,
            // A logical node has no native container by definition. Answering
            // one with a binding would hand back a native id for something that
            // does not exist, which is the one answer no caller can detect as
            // wrong.
            ContainerProjection::LogicalOnly => {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "a logical_only node has no native container to prepare",
                });
            }
        };
        if let Some(canonical) = &state.canonical_root
            && let Some(requested) = &request.cwd
            && requested != canonical
        {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the requested root is not the canonical task worktree of this plane",
            });
        }
        // As for a workspace: a retry is governed by the capabilities its
        // container was frozen under, not by whatever the runtime advertises
        // today.
        let governing = state
            .containers
            .get(&request.topology_node_id)
            .map_or_else(|| state.capabilities.clone(), |it| it.capabilities.clone());
        preflight(
            &governing,
            &OperationContext {
                operation,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: None,
                context_policy: None,
            },
        )?;
        state.take_step(operation, RequestKey::Node(request.topology_node_id))?;
        state
            .calls
            .push(AdapterCall::PrepareContainer(request.topology_node_id));

        // Idempotent per *node*, and a contradiction is a contradiction rather
        // than a second container: the same node asked for at a different root,
        // or as a different shape, is not the retry it looks like.
        if let Some(existing) = state.containers.get(&request.topology_node_id) {
            if existing.binding.root != request.cwd {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this topology node already has a native container at another root",
                });
            }
            if existing.binding.projection != projection {
                return Err(RuntimeError::WorkspaceMismatch {
                    rule: "this topology node already has a native container of another shape",
                });
            }
            return Ok(ContainerOutcome {
                snapshot: existing.clone(),
                created: false,
            });
        }

        state.minted += 1;
        let native_id = ExternalId::parse(&format!("native-container-{}", state.minted))?;
        let identity = state.identity(native_id);
        let correlation = ContainerCorrelationEvidence::establish(
            request.topology_node_id,
            &request.correlation().to_string(),
            identity.clone(),
            request.requested_at,
        )?;
        let snapshot = ContainerBindingSnapshot {
            binding: ContainerBinding {
                id: request.container_binding_id,
                topology_node_id: request.topology_node_id,
                projection,
                identity,
                root: request.cwd.clone(),
                bound_at: request.requested_at,
            },
            capabilities: state.capabilities.clone(),
            correlation,
        };
        state
            .containers
            .insert(request.topology_node_id, snapshot.clone());
        state.container_titles.insert(
            request.topology_node_id,
            request.display_name.as_str().to_owned(),
        );
        Ok(ContainerOutcome {
            snapshot,
            created: true,
        })
    }

    async fn retitle_container(
        &self,
        request: &RetitleContainerRequest,
    ) -> RuntimeResult<RetitleContainerOutcome> {
        let mut state = self.lock();
        let (snapshot, desired, current) = state.retitle_facts(
            request,
            AdapterCall::RetitleContainer(request.topology_node_id),
        )?;
        state
            .container_titles
            .insert(request.topology_node_id, desired.as_str().to_owned());
        // Read back rather than echoed: a caller must be able to tell a silently
        // ignored rename from one that happened.
        let observed_title = state
            .container_titles
            .get(&request.topology_node_id)
            .cloned()
            .unwrap_or_default();
        Ok(RetitleContainerOutcome {
            snapshot,
            changed: current != desired.as_str(),
            desired_title: desired,
            observed_title,
        })
    }

    async fn preview_retitle_container(
        &self,
        request: &RetitleContainerRequest,
    ) -> RuntimeResult<RetitleContainerOutcome> {
        let mut state = self.lock();
        // Every check the apply makes, and then the write it does not make. The
        // ledger records a distinct call so a test can prove the title stayed
        // where it was.
        let (snapshot, desired, current) = state.retitle_facts(
            request,
            AdapterCall::PreviewRetitleContainer(request.topology_node_id),
        )?;
        Ok(RetitleContainerOutcome {
            snapshot,
            changed: current != desired.as_str(),
            desired_title: desired,
            observed_title: current,
        })
    }

    /// Admission is bookkeeping about seats, not an operation on a session: it
    /// starts nothing, reaches no native surface, and is deliberately not
    /// recorded in [`ScriptedFakeRuntime::calls`], so "the runtime was never
    /// called" keeps meaning what it says.
    async fn admit_launch(&self, request: &AdmissionRequest) -> RuntimeResult<AdmissionOutcome> {
        self.lock().admit(request)
    }

    async fn launch(&self, request: &LaunchRequest) -> RuntimeResult<LaunchOutcome> {
        let mut state = self.lock();

        // THE AC-4 GUARANTEE, and the first thing this method does.
        //
        // A `LaunchRequest` is a value: it can be held and presented twice. What
        // it cannot do is restate a fact about *this runtime*, and whether this
        // seat is still holding the reservation this authority was issued from is
        // exactly such a fact. A replay finds it spent; a caller racing another
        // finds it claimed; an authority for another seat finds the wrong one;
        // freshly minted run and binding ids do not help, because the seat is the
        // key. Taking the reservation is part of the same call, so no second
        // launch can pass this line on the strength of a reservation the first is
        // already spending.
        if state.unlaunchable.contains(request.role_slot_id().as_str()) {
            return Err(RuntimeError::Transport {
                rule: "this runtime will not launch that role slot",
            });
        }
        state.admissions.claim(request)?;

        // From here the reservation is claimed, so every remaining refusal has to
        // give the seat back. A refused launch leaves no session and no native
        // effect either way; what it must not also leave is a seat holding a
        // claim nobody can ever spend or replace.
        let outcome = state.launch_admitted(request);
        if outcome.is_err() {
            state.admissions.release(request);
        }
        outcome
    }

    async fn launch_consultation(
        &self,
        request: &ConsultationLaunchRequest,
    ) -> RuntimeResult<ConsultationLaunchOutcome> {
        let mut state = self.lock();
        state.require_plane()?;
        request
            .container
            .ensure_node(request.container.binding.topology_node_id)?;
        request.container.ensure_correlated()?;
        if request.container.binding.root.as_ref() != Some(&request.cwd) {
            return Err(RuntimeError::WorkspaceMismatch {
                rule: "the consultation cwd is not the prepared container root",
            });
        }
        preflight(
            &state.capabilities,
            &OperationContext {
                operation: RuntimeCapability::Launch,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: Some(LimitDemand::MessageBytes(
                    u64::try_from(request.prompt.as_str().len()).unwrap_or(u64::MAX),
                )),
                context_policy: Some(&request.context_policy),
            },
        )?;
        state
            .calls
            .push(AdapterCall::LaunchConsultation(request.seat_binding_id));
        if let Some(existing) = state.consultations.get(&request.seat_binding_id) {
            return Ok(existing.clone());
        }
        state.minted = state.minted.saturating_add(1);
        let identity = state.identity(ExternalId::parse(&format!(
            "native-consultation-{}",
            state.minted
        ))?);
        let outcome = ConsultationLaunchOutcome {
            identity,
            provider_session_id: Some(ExternalId::parse(&format!(
                "provider-consultation-{}",
                state.minted
            ))?),
            observed_at: request.requested_at,
            created: true,
        };
        state
            .consultations
            .insert(request.seat_binding_id, outcome.clone());
        Ok(outcome)
    }

    async fn resume(&self, request: &ResumeRequest) -> RuntimeResult<ControlPlaneObservation> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Resume,
                autonomous: true,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        // Resume is now the normal prelude to delivering a later turn. Existing
        // scripts describe the operation whose behavior they vary (usually the
        // following send), so an ordinary resume must not consume or contradict
        // that queued deviation. A script explicitly targeting resume remains
        // strict and is consumed here.
        if state
            .steps
            .front()
            .is_some_and(|queued| queued.step.operation() == RuntimeCapability::Resume)
        {
            state.take_step(
                RuntimeCapability::Resume,
                RequestKey::Binding(request.binding.binding_id()),
            )?;
        }
        state
            .calls
            .push(AdapterCall::Resume(request.binding.binding_id()));
        let session = state.session(&request.binding)?;
        session.state = ObservedRunState::Running;
        Self::observation(
            &request.binding,
            RuntimeContact::Reachable,
            ObservedRunState::Running,
            ObservationSource::CommandAck,
            0,
            request.requested_at,
        )
    }

    async fn send(&self, request: &SendMessageRequest) -> RuntimeResult<MessageAck> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::SendMessage,
                autonomous: true,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::MessageBytes(request.body_bytes())),
                context_policy: None,
            },
        )?;
        // The binding is judged before its placement, and the order is the
        // point: a fabricated binding is a *forgery*, not a seat with a missing
        // workspace, and reporting it as the latter would tell an operator to
        // reconcile when they should investigate.
        state.session(&request.binding)?;
        // Driving a seat then needs its placement, not only its binding: a
        // message is delivered *into a workspace*. Reads do not, which is
        // exactly the asymmetry a restart exposes.
        if !state.placements.contains(&request.binding.binding_id()) {
            return Err(RuntimeError::WorkspaceBindingRequired);
        }
        let step = state.take_step(
            RuntimeCapability::SendMessage,
            RequestKey::Message(request.message_id),
        )?;
        state.calls.push(AdapterCall::Send(
            request.binding.binding_id(),
            request.message_id,
        ));

        let binding_id = request.binding.binding_id();
        let body_hash = request.body_hash();
        let session = state.session(&request.binding)?;
        if let Admission::Replay(original) =
            session.messages.admit(&request.message_id, &body_hash)?
        {
            return Ok(original);
        }
        let position = session.append(
            SessionEventKind::Message,
            EventSubject::Message(request.message_id),
            request.body.as_str(),
            request.sent_at,
        )?;
        let acknowledgement = MessageAck {
            message_id: request.message_id,
            binding_id,
            position,
            accepted_at: request.sent_at,
        };
        // The ledger is written before the acknowledgement leaves, so a lost
        // acknowledgement is answered from the ledger instead of by sending the
        // message a second time.
        session
            .messages
            .record(request.message_id, body_hash, acknowledgement.clone());
        if matches!(step, Some(ScriptStep::LoseSendAck)) {
            return Err(RuntimeError::Transport {
                rule: "acknowledgement was lost after the message was committed",
            });
        }
        Ok(acknowledgement)
    }

    async fn cancel(&self, request: &CancelRequest) -> RuntimeResult<ControlPlaneObservation> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Cancel,
                autonomous: true,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let step = state.take_step(
            RuntimeCapability::Cancel,
            RequestKey::Binding(request.binding.binding_id()),
        )?;
        state
            .calls
            .push(AdapterCall::Cancel(request.binding.binding_id()));
        let observed = matches!(step, Some(ScriptStep::CancelObservedTerminal));
        let session = state.session(&request.binding)?;
        if observed {
            session.state = ObservedRunState::Cancelled;
        }
        Self::observation(
            &request.binding,
            RuntimeContact::Reachable,
            ObservedRunState::Cancelled,
            if observed {
                ObservationSource::AuthoritativeEvent
            } else {
                ObservationSource::CommandAck
            },
            0,
            request.requested_at,
        )
    }

    async fn retire(
        &self,
        binding: &RuntimeBindingSnapshot,
        at: Timestamp,
    ) -> RuntimeResult<ControlPlaneObservation> {
        let mut state = self.lock();
        state.session(binding)?.state = ObservedRunState::Cancelled;
        state.calls.push(AdapterCall::Retire(binding.binding_id()));
        Self::observation(
            binding,
            RuntimeContact::Reachable,
            ObservedRunState::Cancelled,
            ObservationSource::Inspect,
            0,
            at,
        )
    }

    async fn inspect(&self, request: &InspectRequest) -> RuntimeResult<ControlPlaneObservation> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Inspect,
                autonomous: false,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let step = state.take_step(
            RuntimeCapability::Inspect,
            RequestKey::Binding(request.binding.binding_id()),
        )?;
        state
            .calls
            .push(AdapterCall::Inspect(request.binding.binding_id()));
        let observed = state.session(&request.binding)?.state;
        let process_missing = matches!(step, Some(ScriptStep::InspectProcessMissing));
        Self::observation(
            &request.binding,
            if process_missing {
                RuntimeContact::ProcessMissing
            } else {
                RuntimeContact::Reachable
            },
            if process_missing {
                ObservedRunState::Unknown
            } else {
                observed
            },
            ObservationSource::Inspect,
            0,
            request.requested_at,
        )
    }

    async fn adopt(&self, request: &AdoptRequest) -> RuntimeResult<LaunchOutcome> {
        let mut state = self.lock();

        // Adoption is the other door into a binding, and the one no seat
        // reservation answers for: an `AdoptRequest` names no seat to admit
        // against. It does name the run, so the run-keyed half of the
        // cardinality rule is enforced here — first, and out of the runtime's
        // own ledger. Before the preflight on purpose: the answer is already in
        // this table, so no refusal can arrive after an effect.
        if state.run_holds_other_session(request.agent_run_id, Some(&request.native.native_id)) {
            return Err(RuntimeError::SessionAlreadyBound {
                rule: "a run holding a session is re-adopted into that one, never a second",
            });
        }

        let declared = state.capabilities.clone();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Adopt,
                autonomous: true,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: None,
                context_policy: None,
            },
        )?;
        state.take_step(
            RuntimeCapability::Adopt,
            RequestKey::Run(request.agent_run_id),
        )?;
        state.calls.push(AdapterCall::Adopt(request.agent_run_id));

        if request.native.generation != state.generation {
            return Err(RuntimeError::StaleBinding {
                rule: "the named session belongs to another runtime generation",
            });
        }
        let native_id = request.native.native_id.clone();
        let reported = match state.sessions.get(&native_id) {
            Some(session) => session.correlation_text.clone(),
            None => state
                .scripted_sessions
                .iter()
                .find(|session| session.native_id == native_id)
                .ok_or(RuntimeError::StaleBinding {
                    rule: "the runtime does not own this native session",
                })?
                .correlation_text
                .clone(),
        };
        let identity = state.identity(native_id.clone());
        let snapshot = Self::bind(
            &state,
            request.agent_run_id,
            request.binding_id,
            identity,
            reported.as_deref().unwrap_or_default(),
            request.adopted_at,
        )?;

        let epoch = state.epoch;
        let staged_history = state.staged_history.clone();
        let staged_live = state.staged_live.clone();
        let session = state.sessions.entry(native_id).or_insert_with(|| {
            let history_len = staged_history.len();
            let mut content = staged_history;
            content.extend(staged_live);
            FakeSession {
                agent_run_id: request.agent_run_id,
                binding_id: request.binding_id,
                correlation_text: reported,
                state: ObservedRunState::Running,
                epoch,
                content,
                history_len,
                messages: MessageLedger::new(),
            }
        });
        // Adoption re-points an existing session at the adopting run without
        // discarding anything it already recorded.
        session.agent_run_id = request.agent_run_id;
        session.binding_id = request.binding_id;
        let raised = session.raised_permissions();
        let observed = session.state;
        state.placements.insert(request.binding_id);
        state.bindings.insert(request.binding_id, snapshot.clone());
        for permission_id in raised {
            state.permissions.open(request.binding_id, permission_id);
        }

        let observation = Self::observation(
            &snapshot,
            RuntimeContact::Reachable,
            observed,
            ObservationSource::Inspect,
            0,
            request.adopted_at,
        )?;
        Ok(LaunchOutcome {
            snapshot,
            observation,
        })
    }

    async fn discover_sessions(&self) -> RuntimeResult<Vec<NativeSession>> {
        let mut state = self.lock();
        state.require_plane()?;
        let declared = state.capabilities.clone();
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Discovery,
                autonomous: false,
                account_pinned: false,
                binding: None,
                placement: None,
                current_generation: None,
                demand: None,
                context_policy: None,
            },
        )?;
        state.take_step(RuntimeCapability::Discovery, RequestKey::Sessions)?;
        state.calls.push(AdapterCall::DiscoverSessions);

        let mut found: Vec<NativeSession> = state
            .sessions
            .iter()
            .map(|(native_id, session)| NativeSession {
                identity: state.identity(native_id.clone()),
                correlation: session
                    .correlation_text
                    .as_deref()
                    .and_then(|text| CorrelationLabel::parse(text).ok()),
                state: session.state,
                observed_at: session
                    .content
                    .last()
                    .map_or(Timestamp::UNIX_EPOCH, |event| event.emitted_at),
            })
            .collect();
        found.extend(state.scripted_sessions.iter().map(|session| {
            NativeSession {
                identity: NativeRuntimeIdentity {
                    runtime_kind: state.runtime_kind.clone(),
                    host: state.host.clone(),
                    generation: session.generation,
                    native_id: session.native_id.clone(),
                },
                correlation: session
                    .correlation_text
                    .as_deref()
                    .and_then(|text| CorrelationLabel::parse(text).ok()),
                state: session.state,
                observed_at: session.observed_at,
            }
        }));
        Ok(found)
    }

    async fn reconcile(
        &self,
        bindings: &[RuntimeBindingSnapshot],
    ) -> RuntimeResult<ReconciliationReport> {
        let sessions = self.discover_sessions().await?;
        Ok(reconcile(bindings, &sessions, self.generation()))
    }

    async fn history(&self, request: &HistoryRequest) -> RuntimeResult<HistoryPage> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::History,
                autonomous: false,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: Some(LimitDemand::HistoryPage(request.page_size)),
                context_policy: None,
            },
        )?;
        state.take_step(
            RuntimeCapability::History,
            RequestKey::Binding(request.binding.binding_id()),
        )?;
        state
            .calls
            .push(AdapterCall::History(request.binding.binding_id()));

        let binding_id = request.binding.binding_id();
        let session = state.session(&request.binding)?;
        let epoch = session.epoch;
        let start = match &request.cursor {
            Some(cursor) => cursor.resolve(binding_id)?,
            None => TimelinePosition::start_of(epoch),
        };
        if start.epoch != epoch {
            return Err(RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::EpochChanged,
            });
        }
        let recorded = &session.content[..session.history_len];
        let items: Vec<SessionEvent> = recorded
            .iter()
            .filter(|event| event.position.sequence > start.sequence)
            .take(request.page_size as usize)
            .cloned()
            .collect();
        let end = items.last().map_or(start, |event| event.position);
        let more = recorded
            .iter()
            .any(|event| event.position.sequence > end.sequence);
        Ok(HistoryPage {
            epoch,
            items,
            next: more.then(|| HistoryCursor::issue(binding_id, end)),
            end,
        })
    }

    async fn subscribe_live(
        &self,
        request: &LiveSubscribeRequest,
    ) -> RuntimeResult<LiveSubscription> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::LiveEvents,
                autonomous: false,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        let step = state.take_step(
            RuntimeCapability::LiveEvents,
            RequestKey::Binding(request.binding.binding_id()),
        )?;
        state
            .calls
            .push(AdapterCall::SubscribeLive(request.binding.binding_id()));

        let session = state.session(&request.binding)?;
        let queued: Vec<SessionEvent> = session
            .content
            .iter()
            .filter(|event| event.position.sequence > request.strict_after.sequence)
            .cloned()
            .collect();
        Ok(LiveSubscription::new(
            request.kinds.clone(),
            request.strict_after,
            queued,
            matches!(step, Some(ScriptStep::CloseStreamWithoutTerminal)),
        ))
    }

    async fn respond_permission(
        &self,
        request: &PermissionResponseRequest,
    ) -> RuntimeResult<PermissionAck> {
        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;
        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::PermissionResponse,
                autonomous: true,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;
        state.take_step(
            RuntimeCapability::PermissionResponse,
            RequestKey::Message(request.response_id),
        )?;
        state
            .calls
            .push(AdapterCall::RespondPermission(request.binding.binding_id()));

        let binding_id = request.binding.binding_id();
        if let Admission::Replay(original) = state.permissions.classify(
            binding_id,
            &request.permission_id,
            request.response_id,
            request.decision,
        )? {
            return Ok(original);
        }
        let position = state.session(&request.binding)?.append(
            SessionEventKind::PermissionResolved,
            EventSubject::Permission(request.permission_id.clone()),
            request.decision_body(),
            request.responded_at,
        )?;
        let acknowledgement = PermissionAck {
            permission_id: request.permission_id.clone(),
            response_id: request.response_id,
            binding_id,
            decision: request.decision,
            position,
            accepted_at: request.responded_at,
        };
        state
            .permissions
            .record(request.permission_id.clone(), acknowledgement.clone());
        Ok(acknowledgement)
    }

    async fn compact(&self, request: &CompactRequest) -> RuntimeResult<CompactionReceipt> {
        // The handoff guard runs first and touches nothing: a compaction that
        // would drop unrecorded work state must not reach the runtime, and must
        // not leave a receipt claiming it was considered.
        request.validate()?;

        let mut state = self.lock();
        let declared = state.capabilities.clone();
        let generation = state.generation;

        // Capability is answered *before* the shared preflight, because "this
        // runtime cannot compact" is a fact to report rather than an error to
        // raise — and reporting it must still cost zero native effect.
        if !declared.supports(RuntimeCapability::Compact) {
            return Ok(request.unsupported_receipt(&declared, request.requested_at)?);
        }

        preflight(
            &declared,
            &OperationContext {
                operation: RuntimeCapability::Compact,
                autonomous: true,
                account_pinned: false,
                binding: Some(&request.binding),
                placement: None,
                current_generation: Some(generation),
                demand: None,
                context_policy: None,
            },
        )?;

        // Idempotency by receipt id, before the effect: a replayed request
        // returns the original receipt rather than compacting a second time.
        if let Some(original) = state.compactions.get(&request.receipt_id) {
            if original.0 != request.content_hash()? {
                return Err(RuntimeError::DuplicateCompaction {
                    rule: "was reused for a different attempt",
                });
            }
            return Ok(original.1.clone());
        }

        let step = state.take_step(
            RuntimeCapability::Compact,
            RequestKey::Binding(request.binding.binding_id()),
        )?;
        state
            .calls
            .push(AdapterCall::Compact(request.binding.binding_id()));

        let before = request.binding.identity().clone();
        let telemetry = match step {
            Some(ScriptStep::CompactTelemetry {
                tokens_before,
                tokens_after,
            }) => CompactionTelemetry {
                tokens_before,
                tokens_after,
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
            // A runtime that reports nothing stays unknown. Zero would be a
            // measurement nobody took.
            _ => CompactionTelemetry::unknown(),
        };
        let (status, after, evidence) = match step {
            Some(ScriptStep::CompactPending) => (CompactionStatus::Pending, None, None),
            Some(ScriptStep::CompactFailed) => (CompactionStatus::Failed, None, None),
            Some(ScriptStep::CompactIdentityDrift { generation }) => {
                let drifted = NativeRuntimeIdentity {
                    generation,
                    ..before.clone()
                };
                // Re-read, identity moved: the session was replaced rather than
                // compacted, and that is a failure however the runtime words it.
                (CompactionStatus::Failed, Some(drifted), None)
            }
            _ => (
                CompactionStatus::Confirmed,
                Some(before.clone()),
                Some(ExternalId::parse("fake-compaction-evidence").map_err(RuntimeError::Domain)?),
            ),
        };

        let receipt = CompactionReceipt {
            schema_version: request.policy.schema_version,
            id: request.receipt_id,
            agent_run_id: request.binding.agent_run_id(),
            binding_id: request.binding.binding_id(),
            native_before: before,
            native_after: after,
            requested: request.policy.requested,
            effective: request.policy.effective,
            trigger: request.trigger,
            capabilities: capability_document(&declared).map_err(RuntimeError::Domain)?,
            status,
            telemetry,
            context_pack_hash: request.context_pack_hash.clone(),
            handoff_hash: request.handoff_hash.clone(),
            evidence,
            recorded_at: request.requested_at,
        };
        receipt.validate().map_err(RuntimeError::Domain)?;
        state.compactions.insert(
            request.receipt_id,
            (request.content_hash()?, receipt.clone()),
        );
        Ok(receipt)
    }
}
