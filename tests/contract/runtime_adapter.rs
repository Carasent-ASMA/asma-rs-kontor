//! The shared runtime-adapter contract, run against the scripted fake.
//!
//! Every adapter Kontor ever gains must pass this suite. It is stated in terms
//! of [`RuntimeAdapter`] rather than the fake, so a Paseo, AO or Codex adapter
//! can be dropped in without the assertions changing. The three cross-adapter
//! contracts themselves live in this crate's library
//! ([`kontor_tests_contract`]) so a real adapter crate runs the same assertions
//! instead of copying them; the fake-specific scenarios stay here.
//!
//! The mutants this suite exists to kill:
//!
//! * launching without a prepared task workspace binding, or into a working
//!   directory other than the verified one;
//! * inflating a discovered trust grade, so an advisory runtime gets driven;
//! * skipping the capability preflight, so an unsupported operation still
//!   produces an effect;
//! * substituting a native id for a Kontor run, binding or message id;
//! * continuing a stream after an epoch change or a sequence gap;
//! * re-emitting a retried message instead of replaying its acknowledgement;
//! * claiming a terminal outcome from a command acknowledgement, a closed
//!   stream or an advisory report rather than from inspect/event evidence.

use std::collections::BTreeSet;

use kontor_core::id::{
    AgentRunId, EventCursor, ExternalId, RoleSlotId, RuntimeBindingId, TaskId, TeamRunId,
};
use kontor_core::repository::RuntimeBinding;
use kontor_core::state::{
    DerivedRunState, DesiredRunState, Freshness, ObservedRunState, RunDerivation, RuntimeContact,
    TerminalOutcome, derive_run_state,
};
use kontor_runtime::adapter::{LaunchOutcome, RuntimeAdapter, RuntimeError, RuntimeResult};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{
    RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{RequestKey, RuntimeScript, ScriptStep, ScriptedFakeRuntime};
use kontor_runtime::observation::{
    ControlPlaneObservation, CorrelationEvidence, ObservationSource, ReconciliationAction,
    ReconciliationFinding,
};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, CorrelationLabel, HistoryRequest, InspectRequest, LaunchParts,
    LaunchRequest, LiveSubscribeRequest, MessageId, PermissionDecision, PermissionResponseRequest,
    ResumeRequest, SendMessageRequest,
};
use kontor_runtime::timeline::{
    EventSubject, SessionEventKind, TimelineBreak, TimelinePosition, pending_permissions,
};
// The three cross-adapter contracts, and the helpers they are stated with, come
// from this crate's library so the fake and every real adapter are judged by one
// copy of each rule.
use kontor_tests_contract::{
    EVIDENCE_WINDOW_SECONDS, SESSION_KINDS, adapter_contract, at, closes, drain_history,
    reconciliation_contract, sequences, session_content_contract, text,
};

use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};

const HISTORY_LIVE: &str = include_str!("fixtures/runtime_adapter/history_live.json");
const GAP: &str = include_str!("fixtures/runtime_adapter/gap.json");
const EPOCH_CHANGE: &str = include_str!("fixtures/runtime_adapter/epoch_change.json");
const OUT_OF_ORDER: &str = include_str!("fixtures/runtime_adapter/out_of_order.json");
const LOST_ACK: &str = include_str!("fixtures/runtime_adapter/lost_ack.json");
const PERMISSION_WAIT: &str = include_str!("fixtures/runtime_adapter/permission_wait.json");
const ORPHAN: &str = include_str!("fixtures/runtime_adapter/orphan.json");
const RESTART: &str = include_str!("fixtures/runtime_adapter/restart.json");
const LIMITS: &str = include_str!("fixtures/runtime_adapter/limits.json");
const CANCEL: &str = include_str!("fixtures/runtime_adapter/cancel.json");
const WORKSPACE: &str = include_str!("fixtures/runtime_adapter/workspace.json");
const DUPLICATE: &str = include_str!("fixtures/runtime_adapter/duplicate.json");
const TRANSPORT_FAILURE: &str = include_str!("fixtures/runtime_adapter/transport_failure.json");

// ---------------------------------------------------------------------------
// Harness — fake-specific only; the shared helpers come from the library.
// ---------------------------------------------------------------------------

fn script(json: &str) -> RuntimeScript {
    serde_json::from_str(json).expect("fixture describes a runtime script")
}

fn every_capability() -> BTreeSet<RuntimeCapability> {
    RuntimeCapability::ALL.iter().copied().collect()
}

fn capabilities(trust_grade: TrustGrade) -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade,
        supported: every_capability(),
        account_env: true,
        limits: RuntimeLimits {
            max_message_bytes: 4096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
        },
    }
}

fn root(path: &str) -> WorkspaceRoot {
    WorkspaceRoot::parse(path).expect("absolute workspace path")
}

/// Ask the runtime to admit a launch, and spend what it issues on `parts`.
///
/// There is no other way to obtain a `LaunchRequest`, which is the point: this
/// suite has every public API of `kontor-runtime` available to it and still
/// cannot assemble one without a runtime saying yes.
///
/// # Panics
/// Panics when the seat is not admitted. Tests that are *about* refused
/// admission call [`RuntimeAdapter::admit_launch`] themselves.
async fn admitted(adapter: &dyn RuntimeAdapter, parts: LaunchParts) -> LaunchRequest {
    adapter
        .admit_launch(&AdmissionRequest {
            slot: RoleSlotKey::new(parts.team_run_id, parts.role_slot_id.clone()),
            agent_run_id: parts.agent_run_id,
            binding_id: parts.binding_id,
            replaces: None,
            requested_at: parts.requested_at,
        })
        .await
        .expect("the seat admits this launch")
        .into_authority()
        .expect("admission issues authority rather than a resume")
        .into_request(parts)
}

/// The seat one run of this suite launches into.
///
/// Distinct runs get distinct seats, because admission is keyed on the seat and
/// most of this suite is about something else. Tests that mean "two attempts at
/// *the same* seat" name the slot themselves.
fn slot_of(agent_run_id: AgentRunId) -> RoleSlotId {
    RoleSlotId::parse(&format!("slot-{agent_run_id}")).expect("a run id is a legal open key")
}
/// One team run working on one task in one verified place.
struct Team {
    fake: ScriptedFakeRuntime,
    team_run_id: TeamRunId,
    task_id: TaskId,
    workspace: WorkspaceBindingSnapshot,
    /// One binding id per run, remembered.
    ///
    /// A caller retrying a launch that failed asks the *same* question again —
    /// same seat, same run, same binding — which is what makes the retry a
    /// retry rather than a second attempt at an already-reserved seat.
    bindings: std::sync::Mutex<std::collections::BTreeMap<AgentRunId, RuntimeBindingId>>,
}

impl Team {
    /// Build a runtime at `trust_grade` and prepare its task workspace.
    async fn new(trust_grade: TrustGrade) -> Self {
        Self::with_capabilities(capabilities(trust_grade)).await
    }

    async fn with_capabilities(capabilities: RuntimeCapabilities) -> Self {
        let fake = ScriptedFakeRuntime::new(capabilities);
        let team_run_id = TeamRunId::generate();
        let task_id = TaskId::generate();
        let workspace = prepare(&fake, team_run_id, task_id, &root("/w/task-1"))
            .await
            .expect("the runtime prepares a task workspace");
        Self {
            fake,
            team_run_id,
            task_id,
            workspace,
            bindings: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }

    /// The binding id this suite uses for one run, minted once.
    fn binding_for(&self, agent_run_id: AgentRunId) -> RuntimeBindingId {
        *self
            .bindings
            .lock()
            .expect("the fixture lock is intact")
            .entry(agent_run_id)
            .or_insert_with(RuntimeBindingId::generate)
    }

    fn load(&self, json: &str, correlations: &[CorrelationLabel]) {
        self.fake
            .load_script(&script(json), correlations)
            .expect("the fixture loads");
    }

    /// What a launch for one role of this team run names, in the verified place.
    fn launch_parts(&self, agent_run_id: AgentRunId) -> LaunchParts {
        LaunchParts {
            agent_run_id,
            team_run_id: self.team_run_id,
            role_slot_id: slot_of(agent_run_id),
            task_id: self.task_id,
            binding_id: self.binding_for(agent_run_id),
            workspace: Some(self.workspace.clone()),
            cwd: self.workspace.root().clone(),
            account_profile_id: None,
            prompt: text("do the work"),
            requested_at: at("2026-08-10T09:00:00Z"),
        }
    }

    /// An admitted launch request for one role, in the verified place.
    async fn launch_request(&self, agent_run_id: AgentRunId) -> LaunchRequest {
        admitted(&self.fake, self.launch_parts(agent_run_id)).await
    }

    async fn launch(&self, agent_run_id: AgentRunId) -> RuntimeResult<LaunchOutcome> {
        let request = self.launch_request(agent_run_id).await;
        self.fake.launch(&request).await
    }

    /// Launch one role and keep the binding.
    async fn role(&self, agent_run_id: AgentRunId) -> RuntimeBindingSnapshot {
        self.launch(agent_run_id)
            .await
            .expect("the role launches")
            .snapshot
    }
}

async fn prepare(
    adapter: &dyn RuntimeAdapter,
    team_run_id: TeamRunId,
    task_id: TaskId,
    at_root: &WorkspaceRoot,
) -> RuntimeResult<WorkspaceBindingSnapshot> {
    adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: at_root.clone(),
            requested_at: at("2026-08-10T08:59:00Z"),
        })
        .await
        .map(|outcome| outcome.snapshot)
}

// ---------------------------------------------------------------------------
// Capability, trust and the frozen snapshot
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unsupported_launch_fails_before_adapter_call() {
    let mut declared = capabilities(TrustGrade::A);
    declared.supported.remove(&RuntimeCapability::Launch);
    let team = Team::with_capabilities(declared).await;
    team.fake.take_calls();

    let error = team
        .launch(AgentRunId::generate())
        .await
        .expect_err("an undeclared launch must be refused");

    assert_eq!(
        error,
        RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::Launch
        }
    );
    assert!(
        team.fake.calls().is_empty(),
        "the refusal must happen before the runtime is called"
    );
}

#[tokio::test]
async fn unsupported_session_operation_fails_before_adapter_call() {
    let mut declared = capabilities(TrustGrade::A);
    declared.supported.remove(&RuntimeCapability::History);
    let team = Team::with_capabilities(declared).await;
    let binding = team.role(AgentRunId::generate()).await;
    team.fake.take_calls();

    let error = team
        .fake
        .history(&HistoryRequest {
            binding,
            cursor: None,
            page_size: 10,
        })
        .await
        .expect_err("an undeclared history read must be refused");

    assert_eq!(
        error,
        RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::History
        }
    );
    assert!(
        team.fake.calls().is_empty(),
        "the refusal must happen before the runtime is called"
    );
}

#[tokio::test]
async fn grade_c_cannot_autonomously_dispatch() {
    // The advisory runtime declares every capability. Only the grade refuses.
    let fake = ScriptedFakeRuntime::new(capabilities(TrustGrade::C));
    let team_run_id = TeamRunId::generate();
    let task_id = TaskId::generate();

    let refused = prepare(&fake, team_run_id, task_id, &root("/w/task-1"))
        .await
        .expect_err("an advisory runtime may not be driven");
    assert_eq!(
        refused,
        RuntimeError::InsufficientTrust {
            found: TrustGrade::C,
            operation: RuntimeCapability::PrepareWorkspace,
            rule: "an advisory-grade runtime may be observed but not driven",
        }
    );

    let advisory_run = AgentRunId::generate();
    let request = admitted(
        &fake,
        LaunchParts {
            agent_run_id: advisory_run,
            team_run_id,
            role_slot_id: slot_of(advisory_run),
            task_id,
            binding_id: RuntimeBindingId::generate(),
            workspace: None,
            cwd: root("/w/task-1"),
            account_profile_id: None,
            prompt: text("do the work"),
            requested_at: at("2026-08-10T09:00:00Z"),
        },
    )
    .await;
    let launch = fake
        .launch(&request)
        .await
        .expect_err("an advisory runtime may not be launched into");
    assert!(matches!(launch, RuntimeError::InsufficientTrust { .. }));

    // Observation stays available: an advisory runtime is an inbox, not a wall.
    fake.discover_sessions()
        .await
        .expect("discovery is read-only and stays available at Grade C");
}

#[tokio::test]
async fn grade_c_terminal_report_is_not_terminal_evidence() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    team.fake.push_step(ScriptStep::CancelObservedTerminal);
    team.fake
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("the cancellation is observed");

    let observed = team
        .fake
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:06:00Z"),
        })
        .await
        .expect("inspect answers");

    assert_eq!(observed.state, ObservedRunState::Cancelled);
    assert_eq!(
        closes(&team.fake, &observed, &binding).await,
        Some(TerminalOutcome::Cancelled),
        "a fresh inspect at Grade A closes the run"
    );

    // The identical report, judged against a snapshot the caller edited, proves
    // nothing — and it is refused as a forgery rather than merely re-graded.
    // The grade is the runtime's; no call site gets to supply its own.
    let advisory = RuntimeBindingSnapshot {
        capabilities: capabilities(TrustGrade::C),
        ..binding
    };
    assert_eq!(
        team.fake
            .issued_binding(&advisory)
            .await
            .expect_err("this is not the binding the runtime issued"),
        RuntimeError::StaleBinding {
            rule: "this is not the binding the runtime issued",
        }
    );
    assert_eq!(
        closes(&team.fake, &observed, &advisory).await,
        None,
        "an edited snapshot closes nothing, downgraded or promoted"
    );
}

#[tokio::test]
async fn grade_b_terminal_requires_fresh_inspect() {
    let team = Team::new(TrustGrade::B).await;
    let binding = team.role(AgentRunId::generate()).await;

    team.fake.push_step(ScriptStep::CancelObservedTerminal);
    let event = team
        .fake
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("the runtime emits a cancellation event");
    assert_eq!(event.source, ObservationSource::AuthoritativeEvent);
    assert_eq!(
        closes(&team.fake, &event, &binding).await,
        None,
        "Grade B replay is incomplete, so an event is not proof"
    );

    let inspected = team
        .fake
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:06:00Z"),
        })
        .await
        .expect("inspect answers");
    assert_eq!(
        closes(&team.fake, &inspected, &binding).await,
        Some(TerminalOutcome::Cancelled),
        "a fresh inspect is what Grade B closes on"
    );
    let issued = team
        .fake
        .issued_binding(&binding)
        .await
        .expect("the runtime vouches for the binding it issued");
    assert_eq!(
        inspected.terminal_evidence(&issued, at("2026-08-10T09:10:00Z"), EVIDENCE_WINDOW_SECONDS),
        None,
        "the same inspect four minutes later is a description, not proof"
    );
}

#[tokio::test]
async fn binding_keeps_capability_snapshot_across_rediscovery() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    assert_eq!(binding.capabilities.trust_grade, TrustGrade::A);

    // The runtime is downgraded after the binding was made.
    team.fake.set_capabilities(capabilities(TrustGrade::C));
    assert_eq!(
        team.fake
            .discover_capabilities()
            .await
            .expect("discovery answers")
            .trust_grade,
        TrustGrade::C
    );

    assert_eq!(
        binding.capabilities.trust_grade,
        TrustGrade::A,
        "an earlier run keeps the evidence quality it was created under"
    );
    team.fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id: MessageId::generate(),
            body: text("keep going"),
            sent_at: at("2026-08-10T09:07:00Z"),
        })
        .await
        .expect("the frozen snapshot still governs this binding");

    let refused = team
        .launch(AgentRunId::generate())
        .await
        .expect_err("a new launch is judged against what the runtime proves now");
    assert!(matches!(refused, RuntimeError::InsufficientTrust { .. }));
}

// ---------------------------------------------------------------------------
// Task workspace
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workspace_prepare_is_idempotent_for_one_team_run() {
    let fake = ScriptedFakeRuntime::new(capabilities(TrustGrade::A));
    let team_run_id = TeamRunId::generate();
    let task_id = TaskId::generate();
    let place = root("/w/task-1");

    let first = fake
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: place.clone(),
            requested_at: at("2026-08-10T08:59:00Z"),
        })
        .await
        .expect("the workspace is prepared");
    assert!(first.created);

    // A retry after a lost answer mints a new Kontor id, and must still get the
    // original workspace rather than a second one.
    let retry = fake
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: place.clone(),
            requested_at: at("2026-08-10T08:59:30Z"),
        })
        .await
        .expect("the repeated preparation answers");

    assert!(!retry.created, "a retry must not create a second workspace");
    assert_eq!(retry.snapshot.binding, first.snapshot.binding);
    assert_eq!(fake.workspace_count(), 1);

    let elsewhere = fake
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: root("/w/somewhere-else"),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect_err("a second root for the same team run is a contradiction");
    assert_eq!(
        elsewhere,
        RuntimeError::WorkspaceMismatch {
            rule: "this team run already has a task workspace at another root",
        }
    );
    assert_eq!(fake.workspace_count(), 1);
}

#[tokio::test]
async fn workspace_prepare_retry_returns_the_frozen_capability_snapshot() {
    let fake = ScriptedFakeRuntime::new(capabilities(TrustGrade::A));
    let team_run_id = TeamRunId::generate();
    let task_id = TaskId::generate();
    let place = root("/w/task-1");
    let prepare_request = |workspace_binding_id| WorkspacePrepareRequest {
        team_run_id,
        task_id,
        workspace_binding_id,
        root: place.clone(),
        requested_at: at("2026-08-10T08:59:00Z"),
    };

    let first = fake
        .prepare_workspace(&prepare_request(WorkspaceBindingId::generate()))
        .await
        .expect("the workspace is prepared");
    assert!(first.created);
    assert!(
        first
            .snapshot
            .capabilities
            .supports(RuntimeCapability::PrepareWorkspace)
    );

    // The runtime is rediscovered and no longer advertises the very capability
    // this workspace was prepared under.
    let mut downgraded = capabilities(TrustGrade::A);
    downgraded
        .supported
        .remove(&RuntimeCapability::PrepareWorkspace);
    downgraded.limits.max_history_page = 1;
    fake.set_capabilities(downgraded.clone());
    assert!(
        !fake
            .discover_capabilities()
            .await
            .expect("discovery answers")
            .supports(RuntimeCapability::PrepareWorkspace),
        "the runtime really did stop advertising it"
    );

    // An idempotent retry is the same preparation, so it is still honored and
    // still answers with the capabilities that were frozen into the binding.
    let retry = fake
        .prepare_workspace(&prepare_request(WorkspaceBindingId::generate()))
        .await
        .expect("a retry is honored from the frozen snapshot, not from discovery");

    assert!(!retry.created);
    assert_eq!(
        retry.snapshot.capabilities, first.snapshot.capabilities,
        "an idempotent retry must not re-grade the workspace against current discovery"
    );
    assert_ne!(
        retry.snapshot.capabilities, downgraded,
        "the frozen snapshot is not the runtime's current capability set"
    );
    assert_eq!(retry.snapshot.binding, first.snapshot.binding);
    assert_eq!(retry.snapshot.correlation, first.snapshot.correlation);
    assert_eq!(fake.workspace_count(), 1);

    // The freeze is scoped to its own binding: a team run with no workspace yet
    // is judged against what the runtime advertises now, and refused.
    assert_eq!(
        fake.prepare_workspace(&WorkspacePrepareRequest {
            team_run_id: TeamRunId::generate(),
            task_id: TaskId::generate(),
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: root("/w/task-2"),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect_err("a new workspace cannot be prepared by a runtime that cannot prepare"),
        RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::PrepareWorkspace
        }
    );
    assert_eq!(fake.workspace_count(), 1);
}

#[tokio::test]
async fn team_run_roles_share_one_verified_workspace_binding() {
    let team = Team::new(TrustGrade::A).await;

    let builder = team.role(AgentRunId::generate()).await;
    let reviewer = team.role(AgentRunId::generate()).await;

    assert_ne!(
        builder.binding_id(),
        reviewer.binding_id(),
        "the roles are different sessions"
    );
    assert_eq!(
        team.fake.workspace_count(),
        1,
        "one team run works in exactly one place"
    );
    assert_eq!(team.workspace.root(), &root("/w/task-1"));

    // Another team run gets its own workspace, and cannot borrow this one.
    let other_team = TeamRunId::generate();
    let other_run = AgentRunId::generate();
    let request = admitted(
        &team.fake,
        LaunchParts {
            agent_run_id: other_run,
            team_run_id: other_team,
            role_slot_id: slot_of(other_run),
            task_id: team.task_id,
            binding_id: RuntimeBindingId::generate(),
            workspace: Some(team.workspace.clone()),
            cwd: team.workspace.root().clone(),
            account_profile_id: None,
            prompt: text("do the work"),
            requested_at: at("2026-08-10T09:00:00Z"),
        },
    )
    .await;
    let stolen = team
        .fake
        .launch(&request)
        .await
        .expect_err("another team run may not launch into this workspace");
    assert_eq!(
        stolen,
        RuntimeError::WorkspaceMismatch {
            rule: "the workspace was prepared for another team run",
        }
    );
}

#[tokio::test]
async fn launch_without_a_workspace_binding_is_refused_before_any_effect() {
    let team = Team::new(TrustGrade::A).await;
    team.fake.take_calls();

    let mut parts = team.launch_parts(AgentRunId::generate());
    parts.workspace = None;
    let request = admitted(&team.fake, parts).await;

    let error = team
        .fake
        .launch(&request)
        .await
        .expect_err("a launch that skipped preparation must be refused");

    assert_eq!(error, RuntimeError::WorkspaceBindingRequired);
    assert!(
        team.fake.calls().is_empty(),
        "nothing may happen before the workspace is verified"
    );
}

#[tokio::test]
async fn launch_with_a_mismatched_workspace_root_is_refused_before_any_effect() {
    let team = Team::new(TrustGrade::A).await;
    team.fake.take_calls();

    let mut parts = team.launch_parts(AgentRunId::generate());
    parts.cwd = root("/w/some-other-tree");
    let request = admitted(&team.fake, parts).await;

    let error = team
        .fake
        .launch(&request)
        .await
        .expect_err("a role may not work outside the verified workspace");

    assert_eq!(
        error,
        RuntimeError::WorkspaceMismatch {
            rule: "the claimed working directory is not the verified task workspace",
        }
    );
    assert!(
        team.fake.calls().is_empty(),
        "the refusal must precede any edit"
    );

    // The same binding presented for another task is refused too.
    let mut foreign_parts = team.launch_parts(AgentRunId::generate());
    foreign_parts.task_id = TaskId::generate();
    let foreign = admitted(&team.fake, foreign_parts).await;
    assert_eq!(
        team.fake
            .launch(&foreign)
            .await
            .expect_err("another task may not reuse this workspace"),
        RuntimeError::WorkspaceMismatch {
            rule: "the workspace was prepared for another task",
        }
    );
}

#[tokio::test]
async fn task_workspace_must_not_be_the_runtime_root() {
    let fake = ScriptedFakeRuntime::new(capabilities(TrustGrade::A));
    fake.load_script(&script(WORKSPACE), &[])
        .expect("the fixture loads");
    let runtime_root = fake.runtime_root();
    assert_eq!(runtime_root, root("/kontor-fake-root"));

    let error = fake
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id: TeamRunId::generate(),
            task_id: TaskId::generate(),
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: runtime_root,
            requested_at: at("2026-08-10T08:59:00Z"),
        })
        .await
        .expect_err("the shared root is not a task workspace");

    assert_eq!(
        error,
        RuntimeError::WorkspaceMismatch {
            rule: "a task workspace must not be the runtime root",
        }
    );
    assert_eq!(fake.workspace_count(), 0);
}

#[tokio::test]
async fn a_workspace_root_alias_cannot_dodge_the_guard_rails() {
    // The one spelling difference that is not a difference.
    assert_eq!(root("/w/task-1/"), root("/w/task-1"));

    // Every spelling that reads as one place and lands in another is refused
    // outright, so no guard rail ever has to compare its way out of one.
    for alias in [
        "/w/task-1/..",
        "/w/task-1/.",
        "/w/../task-1",
        "//w/task-1",
        "/w//task-1",
        // A repeated separator is an empty component wherever it sits: shedding
        // the trailing ones would make these normalize instead of be refused.
        "/w/task-1//",
        "/w/task-1///",
        "//",
    ] {
        assert!(
            WorkspaceRoot::parse(alias).is_err(),
            "{alias} must not become a workspace root"
        );
    }

    let fake = ScriptedFakeRuntime::new(capabilities(TrustGrade::A));
    fake.load_script(&script(WORKSPACE), &[])
        .expect("the fixture loads");
    assert_eq!(fake.runtime_root(), root("/kontor-fake-root"));

    // The runtime root under the one alias it can still be written in is
    // recognised as the runtime root, not as a task workspace beneath it.
    assert_eq!(
        prepare(
            &fake,
            TeamRunId::generate(),
            TaskId::generate(),
            &root("/kontor-fake-root/"),
        )
        .await
        .expect_err("an aliased runtime root is still the runtime root"),
        RuntimeError::WorkspaceMismatch {
            rule: "a task workspace must not be the runtime root",
        }
    );
    assert_eq!(fake.workspace_count(), 0);

    // And a role claiming the same place in the other spelling still lands in
    // the verified one rather than being refused as a stranger.
    let team = Team::new(TrustGrade::A).await;
    let mut aliased_parts = team.launch_parts(AgentRunId::generate());
    aliased_parts.cwd = root("/w/task-1/");
    let aliased = admitted(&team.fake, aliased_parts).await;
    team.fake
        .launch(&aliased)
        .await
        .expect("a trailing separator names the workspace the team run verified");
}

#[tokio::test]
async fn a_fabricated_workspace_binding_is_refused_before_any_effect() {
    let team = Team::new(TrustGrade::A).await;
    let other_team = TeamRunId::generate();
    team.fake.take_calls();

    // A snapshot whose correlation was established for another team run is not
    // evidence for the one it claims, however well-formed the binding looks.
    let mut forged = team.workspace.clone();
    forged.binding.team_run_id = other_team;
    let mut parts = team.launch_parts(AgentRunId::generate());
    parts.team_run_id = other_team;
    parts.workspace = Some(forged);
    let request = admitted(&team.fake, parts).await;
    assert_eq!(
        team.fake
            .launch(&request)
            .await
            .expect_err("a binding whose correlation is not its own proves nothing"),
        RuntimeError::CorrelationFailed
    );

    // A wholly self-consistent snapshot for a workspace this runtime never
    // prepared is refused too: consistency is not existence.
    let elsewhere = ScriptedFakeRuntime::new(capabilities(TrustGrade::A));
    let borrowed_team = TeamRunId::generate();
    let borrowed_task = TaskId::generate();
    let borrowed = prepare(&elsewhere, borrowed_team, borrowed_task, &root("/w/task-1"))
        .await
        .expect("another runtime prepares its own workspace");
    borrowed
        .ensure_correlated()
        .expect("the other runtime's snapshot is internally consistent");

    let mut imported_parts = team.launch_parts(AgentRunId::generate());
    imported_parts.team_run_id = borrowed_team;
    imported_parts.task_id = borrowed_task;
    imported_parts.workspace = Some(borrowed);
    let imported = admitted(&team.fake, imported_parts).await;
    assert_eq!(
        team.fake
            .launch(&imported)
            .await
            .expect_err("this runtime never prepared that workspace"),
        RuntimeError::WorkspaceMismatch {
            rule: "this runtime never prepared a task workspace for this team run",
        }
    );

    assert!(
        team.fake.calls().is_empty(),
        "an unverifiable workspace produces no effect at all"
    );
}

#[tokio::test]
async fn workspace_binding_keeps_its_capability_snapshot() {
    let team = Team::new(TrustGrade::A).await;
    assert_eq!(team.workspace.capabilities.trust_grade, TrustGrade::A);

    team.fake.set_capabilities(capabilities(TrustGrade::C));

    assert_eq!(
        team.workspace.capabilities.trust_grade,
        TrustGrade::A,
        "the workspace binding freezes evidence quality exactly as a session binding does"
    );
    assert_eq!(
        team.workspace.correlation.label.team_run_id(),
        team.team_run_id
    );
}

// ---------------------------------------------------------------------------
// Typed operations, identity and correlation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_typed_operations_preserve_kontor_identity() {
    let team = Team::new(TrustGrade::A).await;
    team.load(PERMISSION_WAIT, &[]);
    let agent_run_id = AgentRunId::generate();
    let request = team.launch_request(agent_run_id).await;
    let binding_id = request.binding_id();

    let launched = team.fake.launch(&request).await.expect("the role launches");
    assert_eq!(launched.snapshot.agent_run_id(), agent_run_id);
    assert_eq!(launched.snapshot.binding_id(), binding_id);
    assert_eq!(launched.observation.agent_run_id, agent_run_id);
    let binding = launched.snapshot;

    let resumed = team
        .fake
        .resume(&ResumeRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:01:00Z"),
        })
        .await
        .expect("resume answers");
    assert_eq!(resumed.agent_run_id, agent_run_id);

    let message_id = MessageId::generate();
    let sent = team
        .fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id,
            body: text("carry on"),
            sent_at: at("2026-08-10T09:02:00Z"),
        })
        .await
        .expect("send answers");
    assert_eq!(sent.message_id, message_id);
    assert_eq!(sent.binding_id, binding_id);

    let inspected = team
        .fake
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:03:00Z"),
        })
        .await
        .expect("inspect answers");
    assert_eq!(inspected.agent_run_id, agent_run_id);

    let permission_id = ExternalId::parse("perm-write-migration").expect("valid permission id");
    let response_id = MessageId::generate();
    let answered = team
        .fake
        .respond_permission(&PermissionResponseRequest {
            binding: binding.clone(),
            permission_id: permission_id.clone(),
            response_id,
            decision: PermissionDecision::Allow,
            responded_at: at("2026-08-10T09:04:00Z"),
        })
        .await
        .expect("the permission is answered");
    assert_eq!(answered.response_id, response_id);
    assert_eq!(answered.permission_id, permission_id);
    assert_eq!(answered.binding_id, binding_id);

    let cancelled = team
        .fake
        .cancel(&CancelRequest {
            binding,
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("cancel answers");
    assert_eq!(cancelled.agent_run_id, agent_run_id);
}

#[tokio::test]
async fn launch_binds_native_generation_with_correlation() {
    let team = Team::new(TrustGrade::A).await;
    let agent_run_id = AgentRunId::generate();

    let launched = team.launch(agent_run_id).await.expect("the role launches");
    let identity = launched.snapshot.identity();

    assert_eq!(identity.generation, team.fake.generation());
    assert_eq!(
        launched.snapshot.correlation.label.agent_run_id(),
        agent_run_id,
        "the binding carries proof that this native session serves this run"
    );
    assert_eq!(&launched.snapshot.correlation.native, identity);
    assert_eq!(
        closes(&team.fake, &launched.observation, &launched.snapshot).await,
        None,
        "a launch acknowledgement is not an outcome"
    );
}

#[tokio::test]
async fn native_ids_never_substitute_for_kontor_ids() {
    let team = Team::new(TrustGrade::A).await;
    let agent_run_id = AgentRunId::generate();

    // A runtime echoing its own native id instead of the Kontor label.
    team.fake.push_step(ScriptStep::EchoCorrelation {
        text: "native-session-1".to_owned(),
    });
    assert_eq!(
        team.launch(agent_run_id)
            .await
            .expect_err("a native id is not a correlation label"),
        RuntimeError::CorrelationFailed
    );

    // A runtime echoing a well-formed label for a *different* run.
    team.fake.push_step(ScriptStep::EchoCorrelation {
        text: CorrelationLabel::for_run(AgentRunId::generate()).to_string(),
    });
    assert_eq!(
        team.launch(agent_run_id)
            .await
            .expect_err("another run's label does not correlate this run"),
        RuntimeError::CorrelationFailed
    );

    let binding = team.role(agent_run_id).await;
    let native = binding.identity().native_id.as_str().to_owned();
    assert!(AgentRunId::parse(&native).is_err());
    assert!(MessageId::parse(&native).is_err());
    assert!(RuntimeBindingId::parse(&native).is_err());
    assert!(WorkspaceBindingId::parse(&native).is_err());
    assert!(CorrelationLabel::parse(&native).is_err());
    assert_eq!(binding.agent_run_id(), agent_run_id);
}

#[tokio::test]
async fn a_fabricated_session_binding_cannot_drive_a_live_session() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    let victim = team.role(AgentRunId::generate()).await;
    team.fake.take_calls();

    let send = |binding: RuntimeBindingSnapshot| SendMessageRequest {
        binding,
        message_id: MessageId::generate(),
        body: text("do as I say"),
        sent_at: at("2026-08-10T09:02:00Z"),
    };

    // A snapshot in the current generation, naming a native session that really
    // is live, under a run and binding this runtime never issued for it. Only
    // the correlation cannot be forged, so only the correlation is asked.
    let stolen = RuntimeBindingSnapshot {
        binding: RuntimeBinding {
            id: RuntimeBindingId::generate(),
            agent_run_id: AgentRunId::generate(),
            identity: victim.identity().clone(),
            bound_at: at("2026-08-10T09:00:00Z"),
        },
        ..binding.clone()
    };
    assert_eq!(
        team.fake
            .send(&send(stolen))
            .await
            .expect_err("a binding whose correlation is not its own proves nothing"),
        RuntimeError::CorrelationFailed
    );

    // Consistent correlation for a session this runtime bound to someone else
    // is still not this run's session: a native id addresses a session, it does
    // not authorize one.
    let impostor_run = AgentRunId::generate();
    let impostor = RuntimeBindingSnapshot {
        binding: RuntimeBinding {
            id: RuntimeBindingId::generate(),
            agent_run_id: impostor_run,
            identity: victim.identity().clone(),
            bound_at: at("2026-08-10T09:00:00Z"),
        },
        capabilities: binding.capabilities.clone(),
        correlation: CorrelationEvidence::establish(
            impostor_run,
            &CorrelationLabel::for_run(impostor_run).to_string(),
            victim.identity().clone(),
            at("2026-08-10T09:00:00Z"),
        )
        .expect("a self-consistent forgery"),
    };
    impostor
        .ensure_correlated()
        .expect("the forgery really is internally consistent");
    assert_eq!(
        team.fake
            .send(&send(impostor))
            .await
            .expect_err("the runtime issued another binding for that session"),
        RuntimeError::StaleBinding {
            rule: "the runtime issued a different binding for this native session",
        }
    );

    assert_eq!(
        team.fake.committed_messages(&victim),
        0,
        "nothing was delivered into the session that was addressed"
    );
    // The rightful holder is unaffected.
    team.fake
        .send(&send(victim.clone()))
        .await
        .expect("the binding the runtime issued still works");
    assert_eq!(team.fake.committed_messages(&victim), 1);
}

#[tokio::test]
async fn account_pinned_launch_requires_account_env() {
    let mut declared = capabilities(TrustGrade::A);
    declared.account_env = false;
    let team = Team::with_capabilities(declared).await;
    team.fake.take_calls();

    let mut parts = team.launch_parts(AgentRunId::generate());
    parts.account_profile_id = Some(kontor_core::id::AccountProfileId::generate());
    let request = admitted(&team.fake, parts).await;

    assert_eq!(
        team.fake
            .launch(&request)
            .await
            .expect_err("an account-pinned run needs a provable account environment"),
        RuntimeError::AccountEnvironmentUnavailable
    );
    assert!(team.fake.calls().is_empty());

    // The same runtime still launches a run that is not pinned to an account.
    team.launch(AgentRunId::generate())
        .await
        .expect("an unpinned run is unaffected");
}

#[tokio::test]
async fn observation_separates_contact_from_run_state() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    team.fake
        .resume(&ResumeRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:01:00Z"),
        })
        .await
        .expect("resume answers");

    let reachable = team
        .fake
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:02:00Z"),
        })
        .await
        .expect("inspect answers");
    assert_eq!(reachable.state, ObservedRunState::Running);
    assert_eq!(reachable.contact, RuntimeContact::Reachable);

    // The channel breaks. What the runtime last said about the work does not
    // change, and nothing about the work may be concluded from the break.
    let unreachable = ControlPlaneObservation {
        contact: RuntimeContact::Unavailable,
        ..reachable.clone()
    };
    assert_eq!(unreachable.state, ObservedRunState::Running);
    assert_eq!(closes(&team.fake, &unreachable, &binding).await, None);

    let core = reachable.to_core_observation(EventCursor::parse(1).expect("positive cursor"));
    assert_eq!(core.state, reachable.state);
    assert_eq!(&core.identity, &reachable.identity);
    assert_eq!(&core.evidence_hash, reachable.evidence_hash());

    let derived = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: Some(&core),
        binding: Some(&reachable.identity),
        freshness: Freshness::Fresh,
        contact: RuntimeContact::Unavailable,
        terminal: None,
    })
    .expect("a derivation without closure evidence succeeds");
    assert_eq!(derived, DerivedRunState::RuntimeUnavailable);
}

// ---------------------------------------------------------------------------
// Discovery, reconciliation, adoption and restart
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reconcile_classifies_missing_orphan_and_adoptable_sessions() {
    let team = Team::new(TrustGrade::A).await;
    let unbound_run = AgentRunId::generate();
    team.load(ORPHAN, &[CorrelationLabel::for_run(unbound_run)]);

    let bound_run = AgentRunId::generate();
    let bound = team.role(bound_run).await;

    // A binding whose native session the runtime no longer owns at all. It is a
    // binding the runtime really did issue once, so its correlation names the
    // session that vanished — not some other one.
    let ghost_run = AgentRunId::generate();
    let vanished = kontor_core::state::NativeRuntimeIdentity {
        native_id: ExternalId::parse("native-vanished").expect("valid native id"),
        ..bound.identity().clone()
    };
    let ghost = RuntimeBindingSnapshot {
        binding: RuntimeBinding {
            id: RuntimeBindingId::generate(),
            agent_run_id: ghost_run,
            identity: vanished.clone(),
            bound_at: at("2026-08-10T08:00:00Z"),
        },
        capabilities: bound.capabilities.clone(),
        correlation: CorrelationEvidence::establish(
            ghost_run,
            &CorrelationLabel::for_run(ghost_run).to_string(),
            vanished,
            at("2026-08-10T08:00:00Z"),
        )
        .expect("correlation for the ghost binding"),
    };

    let report = team
        .fake
        .reconcile(&[bound.clone(), ghost.clone()])
        .await
        .expect("reconciliation answers");
    assert_eq!(report.generation, team.fake.generation());
    assert!(report.needs_review());

    let matched = report
        .findings
        .iter()
        .filter(|finding| {
            matches!(finding, ReconciliationFinding::Matched { agent_run_id, .. } if *agent_run_id == bound_run)
        })
        .count();
    assert_eq!(matched, 1);

    let missing = report
        .findings
        .iter()
        .find(|finding| matches!(finding, ReconciliationFinding::MissingSession { .. }))
        .expect("the vanished session is classified");
    assert_eq!(
        missing.proposed_state(),
        Some(DerivedRunState::LostContact),
        "a session that is not there is lost contact, never completion"
    );
    assert_eq!(
        missing.action(),
        ReconciliationAction::ProposeLostContactReview
    );

    let adoptable: Vec<&ReconciliationFinding> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding, ReconciliationFinding::Adoptable { .. }))
        .collect();
    assert_eq!(adoptable.len(), 1);
    assert!(matches!(
        adoptable[0],
        ReconciliationFinding::Adoptable { agent_run_id, .. } if *agent_run_id == unbound_run
    ));
    assert_eq!(adoptable[0].action(), ReconciliationAction::ProposeAdoption);

    // The uncorrelated session and the one reporting its own native id as a
    // label are both orphans: neither can be claimed.
    let orphans = report
        .findings
        .iter()
        .filter(|finding| matches!(finding, ReconciliationFinding::Orphan { .. }))
        .count();
    assert_eq!(orphans, 2);
}

#[tokio::test]
async fn adopt_requires_explicit_correlation() {
    let team = Team::new(TrustGrade::A).await;
    let adoptable_run = AgentRunId::generate();
    team.load(ORPHAN, &[CorrelationLabel::for_run(adoptable_run)]);

    let identity_of = |native: &str| kontor_core::state::NativeRuntimeIdentity {
        runtime_kind: kontor_core::id::RuntimeKindKey::parse("fake.runtime")
            .expect("valid runtime kind"),
        host: kontor_core::id::ExternalName::parse("fake-host").expect("valid host"),
        generation: team.fake.generation(),
        native_id: ExternalId::parse(native).expect("valid native id"),
    };

    assert_eq!(
        team.fake
            .adopt(&AdoptRequest {
                agent_run_id: AgentRunId::generate(),
                binding_id: RuntimeBindingId::generate(),
                native: identity_of("native-unclaimed"),
                adopted_at: at("2026-08-10T09:10:00Z"),
            })
            .await
            .expect_err("an uncorrelated session may not be claimed"),
        RuntimeError::CorrelationFailed
    );

    assert_eq!(
        team.fake
            .adopt(&AdoptRequest {
                agent_run_id: AgentRunId::generate(),
                binding_id: RuntimeBindingId::generate(),
                native: identity_of("native-adoptable"),
                adopted_at: at("2026-08-10T09:10:01Z"),
            })
            .await
            .expect_err("a session correlated with another run may not be claimed"),
        RuntimeError::CorrelationFailed
    );

    let adopted = team
        .fake
        .adopt(&AdoptRequest {
            agent_run_id: adoptable_run,
            binding_id: RuntimeBindingId::generate(),
            native: identity_of("native-adoptable"),
            adopted_at: at("2026-08-10T09:10:02Z"),
        })
        .await
        .expect("the run named by the session's own label may claim it");
    assert_eq!(adopted.snapshot.agent_run_id(), adoptable_run);
    assert_eq!(
        adopted.snapshot.identity().generation,
        team.fake.generation()
    );
}

#[tokio::test]
async fn adopt_refuses_a_second_session_for_one_run() {
    let team = Team::new(TrustGrade::A).await;
    let run = AgentRunId::generate();
    team.load(ORPHAN, &[CorrelationLabel::for_run(run)]);

    // The run launches its own session, and the orphan carrying its label is
    // sitting there adoptable. Both are legitimately this run's to claim; the
    // point is that it may not hold both.
    let bound = team.role(run).await;
    let orphan = kontor_core::state::NativeRuntimeIdentity {
        native_id: ExternalId::parse("native-adoptable").expect("valid native id"),
        ..bound.identity().clone()
    };

    team.fake.take_calls();
    assert_eq!(
        team.fake
            .adopt(&AdoptRequest {
                agent_run_id: run,
                binding_id: RuntimeBindingId::generate(),
                native: orphan,
                adopted_at: at("2026-08-10T09:15:00Z"),
            })
            .await
            .expect_err("a run already holding a session may not be bound to a second"),
        RuntimeError::SessionAlreadyBound {
            rule: "a run holding a session is re-adopted into that one, never a second",
        }
    );
    assert!(
        team.fake.calls().is_empty(),
        "the refusal must happen before the runtime is called"
    );
    assert_eq!(
        team.fake.sessions_for(run),
        1,
        "a refused adoption leaves the run holding exactly what it already held"
    );

    // The other side of the same rule: re-adopting the session this run already
    // holds is that one binding being re-issued, not a second one — which is
    // what recovery after a restart does, and what the refusal must not cost.
    let readopted = team
        .fake
        .adopt(&AdoptRequest {
            agent_run_id: run,
            binding_id: RuntimeBindingId::generate(),
            native: bound.identity().clone(),
            adopted_at: at("2026-08-10T09:15:01Z"),
        })
        .await
        .expect("a run is re-adopted into the session it already holds");
    assert_eq!(team.fake.sessions_for(run), 1);
    assert_eq!(
        team.fake
            .resume(&ResumeRequest {
                binding: bound,
                requested_at: at("2026-08-10T09:15:02Z"),
            })
            .await
            .expect_err("the superseded binding no longer drives the session"),
        RuntimeError::StaleBinding {
            rule: "the runtime issued a different binding for this native session",
        }
    );
    team.fake
        .resume(&ResumeRequest {
            binding: readopted.snapshot,
            requested_at: at("2026-08-10T09:15:03Z"),
        })
        .await
        .expect("the binding adoption issued is the one live binding");
}

#[tokio::test]
async fn restart_generation_invalidates_stale_binding() {
    let team = Team::new(TrustGrade::A).await;
    team.load(RESTART, &[]);
    let agent_run_id = AgentRunId::generate();
    let binding = team.role(agent_run_id).await;
    let before = team.fake.generation();

    team.fake.restart();
    assert_eq!(team.fake.generation(), before + 1);

    assert_eq!(
        team.fake
            .resume(&ResumeRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:20:00Z"),
            })
            .await
            .expect_err("a binding from the previous generation is stale"),
        RuntimeError::StaleBinding {
            rule: "the runtime generation changed since this session was bound",
        }
    );

    // The workspace prepared in the old generation is stale in the same way.
    assert_eq!(
        team.launch(AgentRunId::generate())
            .await
            .expect_err("a workspace from the previous generation is stale"),
        RuntimeError::StaleBinding {
            rule: "the runtime generation changed since this workspace was prepared",
        }
    );

    let report = team
        .fake
        .reconcile(std::slice::from_ref(&binding))
        .await
        .expect("reconciliation answers");
    let finding = report
        .findings
        .first()
        .expect("the stale binding is classified");
    assert!(matches!(
        finding,
        ReconciliationFinding::GenerationChanged { agent_run_id: run, .. } if *run == agent_run_id
    ));
    assert_eq!(
        finding.proposed_state(),
        Some(DerivedRunState::Orphaned),
        "a repeated native id in a new generation is an orphan, not the same session"
    );
    assert_eq!(finding.action(), ReconciliationAction::ProposeOrphanReview);
}

#[tokio::test]
async fn fake_restart_preserves_effects_and_changes_generation() {
    let team = Team::new(TrustGrade::A).await;
    team.load(RESTART, &[]);
    let agent_run_id = AgentRunId::generate();
    let binding = team.role(agent_run_id).await;

    let message_id = MessageId::generate();
    team.fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id,
            body: text("work that must survive"),
            sent_at: at("2026-08-10T09:02:00Z"),
        })
        .await
        .expect("the message is delivered");

    let before = team.fake.generation();
    team.fake.restart();
    assert_eq!(team.fake.generation(), before + 1);

    let readopted = team
        .fake
        .adopt(&AdoptRequest {
            agent_run_id,
            binding_id: RuntimeBindingId::generate(),
            native: kontor_core::state::NativeRuntimeIdentity {
                generation: team.fake.generation(),
                ..binding.identity().clone()
            },
            adopted_at: at("2026-08-10T09:30:00Z"),
        })
        .await
        .expect("the surviving session is re-adopted in the new generation");

    let (items, _) = drain_history(&team.fake, &readopted.snapshot, 10)
        .await
        .expect("history is still readable");
    let delivered = items
        .iter()
        .filter(|event| event.subject == EventSubject::Message(message_id))
        .count();
    assert_eq!(delivered, 1, "the committed effect survived the restart");
    assert_eq!(sequences(&items), vec![1, 2]);
}

// ---------------------------------------------------------------------------
// Session content: history, live, gaps, messages and permissions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn history_then_strict_after_live_is_exactly_once() {
    let team = Team::new(TrustGrade::A).await;
    team.load(HISTORY_LIVE, &[]);
    let binding = team.role(AgentRunId::generate()).await;

    let (history, anchor) = drain_history(&team.fake, &binding, 2)
        .await
        .expect("history pages validate");
    assert_eq!(sequences(&history), vec![1, 2, 3, 4]);
    assert_eq!(
        anchor,
        TimelinePosition {
            epoch: 1,
            sequence: 4
        }
    );

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("the stream stays continuous"));
    }

    assert_eq!(sequences(&delivered), vec![5, 6, 7]);
    let mut whole: Vec<u64> = sequences(&history);
    whole.extend(sequences(&delivered));
    assert_eq!(
        whole,
        vec![1, 2, 3, 4, 5, 6, 7],
        "history and live cover the session once each, with no gap and no repeat"
    );
}

#[tokio::test]
async fn selective_live_subscription_still_validates_every_position() {
    let team = Team::new(TrustGrade::A).await;
    team.load(HISTORY_LIVE, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let (_, anchor) = drain_history(&team.fake, &binding, 4)
        .await
        .expect("history pages validate");

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: BTreeSet::from([SessionEventKind::Message]),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("the stream stays continuous"));
    }

    assert_eq!(
        sequences(&delivered),
        vec![5, 7],
        "the caller sees only the kinds it selected"
    );
    assert_eq!(
        live.position(),
        TimelinePosition {
            epoch: 1,
            sequence: 7
        },
        "continuity is still checked over the events the caller filtered out"
    );
}

#[tokio::test]
async fn epoch_change_requires_timeline_refetch() {
    let team = Team::new(TrustGrade::A).await;
    team.load(EPOCH_CHANGE, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let (_, anchor) = drain_history(&team.fake, &binding, 10)
        .await
        .expect("history pages validate");

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    assert_eq!(
        live.next_event()
            .expect("an event arrives")
            .expect_err("a renumbered event breaks the timeline"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
    assert_eq!(
        live.next_event()
            .expect("the queue is not empty")
            .expect_err("a broken stream is not continued"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
}

#[tokio::test]
async fn sequence_gap_requires_timeline_refetch() {
    let team = Team::new(TrustGrade::A).await;
    team.load(GAP, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let (history, anchor) = drain_history(&team.fake, &binding, 10)
        .await
        .expect("history pages validate");
    assert_eq!(sequences(&history), vec![1, 2]);

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    assert_eq!(
        live.next_event()
            .expect("an event arrives")
            .expect_err("a jump past sequence 3 and 4 breaks the timeline"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );
    assert_eq!(
        live.next_event()
            .expect("the late event is still queued")
            .expect_err("a late arrival does not repair a suspect stream"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );
    assert_eq!(
        live.position(),
        TimelinePosition {
            epoch: 1,
            sequence: 2
        }
    );
}

#[tokio::test]
async fn out_of_order_event_does_not_advance_timeline() {
    let team = Team::new(TrustGrade::A).await;
    team.load(OUT_OF_ORDER, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let (_, anchor) = drain_history(&team.fake, &binding, 10)
        .await
        .expect("history pages validate");

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("a late redelivery is benign, not a break"));
    }

    assert_eq!(
        sequences(&delivered),
        vec![3, 4],
        "the redelivered event 2 is dropped rather than delivered again"
    );
    assert_eq!(
        live.position(),
        TimelinePosition {
            epoch: 1,
            sequence: 4
        },
        "an out-of-order event never moves trusted state backwards"
    );
}

#[tokio::test]
async fn duplicate_content_is_dropped_and_a_contradiction_is_refused() {
    let team = Team::new(TrustGrade::A).await;
    team.load(DUPLICATE, &[]);
    let binding = team.role(AgentRunId::generate()).await;

    let (history, anchor) = drain_history(&team.fake, &binding, 10)
        .await
        .expect("history pages validate");
    assert_eq!(
        sequences(&history),
        vec![1, 2],
        "a redelivered history item is removed from the page, not merely uncounted"
    );
    assert_eq!(
        anchor,
        TimelinePosition {
            epoch: 1,
            sequence: 2
        }
    );

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    let mut delivered = Vec::new();
    let broken = loop {
        match live.next_event() {
            None => panic!("the contradiction must surface before the queue drains"),
            Some(Ok(event)) => delivered.push(event),
            Some(Err(error)) => break error,
        }
    };
    assert_eq!(
        sequences(&delivered),
        vec![3, 4],
        "an exact redelivery is dropped rather than delivered a second time"
    );
    assert_eq!(
        broken,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::ConflictingDuplicate
        },
        "rewriting a position already delivered is a contradiction, not a replay"
    );
    assert_eq!(
        live.next_event()
            .expect("a later event is still queued")
            .expect_err("a contradicted stream is not continued"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::ConflictingDuplicate
        }
    );
    assert_eq!(
        live.position(),
        TimelinePosition {
            epoch: 1,
            sequence: 4
        },
        "a contradiction never moves trusted state"
    );
}

#[tokio::test]
async fn lost_ack_retry_returns_original_message_once() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    // The script is loaded after the launch: a step is matched strictly against
    // the operation it belongs to.
    team.load(LOST_ACK, &[]);

    let message_id = MessageId::generate();
    let request = SendMessageRequest {
        binding: binding.clone(),
        message_id,
        body: text("apply the migration"),
        sent_at: at("2026-08-10T09:02:00Z"),
    };

    let lost = team
        .fake
        .send(&request)
        .await
        .expect_err("the acknowledgement is lost");
    assert_eq!(
        lost,
        RuntimeError::Transport {
            rule: "acknowledgement was lost after the message was committed",
        }
    );

    let retried = team
        .fake
        .send(&request)
        .await
        .expect("the retry is answered from the ledger");
    assert_eq!(retried.message_id, message_id);

    let content = team.fake.content(&binding);
    let delivered = content
        .iter()
        .filter(|event| event.subject == EventSubject::Message(message_id))
        .count();
    assert_eq!(delivered, 1, "the retry must not deliver a second message");
    assert_eq!(team.fake.committed_messages(&binding), 1);
    assert_eq!(
        retried.position,
        TimelinePosition {
            epoch: 1,
            sequence: 1
        }
    );
}

#[tokio::test]
async fn same_message_id_with_different_body_is_rejected() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    let message_id = MessageId::generate();

    team.fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id,
            body: text("first instruction"),
            sent_at: at("2026-08-10T09:02:00Z"),
        })
        .await
        .expect("the first message is delivered");

    assert_eq!(
        team.fake
            .send(&SendMessageRequest {
                binding: binding.clone(),
                message_id,
                body: text("a different instruction"),
                sent_at: at("2026-08-10T09:03:00Z"),
            })
            .await
            .expect_err("reusing an identifier for other content is a caller bug"),
        RuntimeError::DuplicateMessage {
            rule: "was already used for different content",
        }
    );
    assert_eq!(team.fake.committed_messages(&binding), 1);
}

#[tokio::test]
async fn permission_wait_survives_history_and_live() {
    let team = Team::new(TrustGrade::A).await;
    team.load(PERMISSION_WAIT, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let permission_id = ExternalId::parse("perm-write-migration").expect("valid permission id");

    let (history, anchor) = drain_history(&team.fake, &binding, 10)
        .await
        .expect("history pages validate");
    assert_eq!(
        pending_permissions(&history),
        BTreeSet::from([permission_id.clone()]),
        "a permission raised in history is still waiting"
    );
    assert_eq!(
        team.fake.pending_permissions(),
        BTreeSet::from([permission_id.clone()])
    );

    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");
    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("the stream stays continuous"));
    }
    assert_eq!(sequences(&delivered), vec![3]);

    let mut whole = history.clone();
    whole.extend(delivered);
    assert_eq!(
        pending_permissions(&whole),
        BTreeSet::from([permission_id.clone()]),
        "following the session live does not resolve the wait"
    );

    let answered = team
        .fake
        .respond_permission(&PermissionResponseRequest {
            binding: binding.clone(),
            permission_id: permission_id.clone(),
            response_id: MessageId::generate(),
            decision: PermissionDecision::Allow,
            responded_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("the permission is answered");
    assert_eq!(answered.decision, PermissionDecision::Allow);

    assert!(team.fake.pending_permissions().is_empty());
    assert_eq!(
        pending_permissions(&team.fake.content(&binding)),
        BTreeSet::new(),
        "the resolution is recorded in the session's own content"
    );
}

#[tokio::test]
async fn permission_response_is_idempotent_and_session_bound() {
    let team = Team::new(TrustGrade::A).await;
    team.load(PERMISSION_WAIT, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let other_role = team.role(AgentRunId::generate()).await;
    let permission_id = ExternalId::parse("perm-write-migration").expect("valid permission id");
    let response_id = MessageId::generate();

    let answer = |binding: RuntimeBindingSnapshot,
                  permission_id: ExternalId,
                  response_id: MessageId,
                  decision: PermissionDecision| PermissionResponseRequest {
        binding,
        permission_id,
        response_id,
        decision,
        responded_at: at("2026-08-10T09:05:00Z"),
    };

    let unknown = team
        .fake
        .respond_permission(&answer(
            binding.clone(),
            ExternalId::parse("perm-never-raised").expect("valid permission id"),
            MessageId::generate(),
            PermissionDecision::Allow,
        ))
        .await
        .expect_err("an unraised request cannot be answered");
    assert_eq!(
        unknown,
        RuntimeError::PermissionConflict {
            rule: "is unknown to this runtime",
        }
    );

    let foreign = team
        .fake
        .respond_permission(&answer(
            other_role,
            permission_id.clone(),
            MessageId::generate(),
            PermissionDecision::Allow,
        ))
        .await
        .expect_err("another session's request cannot be answered");
    assert_eq!(
        foreign,
        RuntimeError::PermissionConflict {
            rule: "belongs to another session",
        }
    );

    let first = team
        .fake
        .respond_permission(&answer(
            binding.clone(),
            permission_id.clone(),
            response_id,
            PermissionDecision::Allow,
        ))
        .await
        .expect("the permission is answered");

    let replay = team
        .fake
        .respond_permission(&answer(
            binding.clone(),
            permission_id.clone(),
            response_id,
            PermissionDecision::Allow,
        ))
        .await
        .expect("the retry replays the original answer");
    assert_eq!(replay, first);

    let contradiction = team
        .fake
        .respond_permission(&answer(
            binding.clone(),
            permission_id.clone(),
            response_id,
            PermissionDecision::Deny,
        ))
        .await
        .expect_err("the same response id may not carry a different answer");
    assert_eq!(
        contradiction,
        RuntimeError::PermissionConflict {
            rule: "was already resolved with a different answer",
        }
    );

    let resolved = team
        .fake
        .content(&binding)
        .iter()
        .filter(|event| event.kind == SessionEventKind::PermissionResolved)
        .count();
    assert_eq!(resolved, 1, "one decision, one recorded resolution");
}

// ---------------------------------------------------------------------------
// Limits, cancellation and stream loss
// ---------------------------------------------------------------------------

#[tokio::test]
async fn limits_fail_before_effect() {
    let team = Team::new(TrustGrade::A).await;
    team.load(LIMITS, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    team.fake.take_calls();

    let oversized = team
        .fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id: MessageId::generate(),
            body: text("this body is far longer than the runtime accepts"),
            sent_at: at("2026-08-10T09:02:00Z"),
        })
        .await
        .expect_err("an oversized message is refused");
    assert_eq!(
        oversized,
        RuntimeError::LimitExceeded {
            subject: "message body",
            limit: 16
        }
    );

    let too_many = team
        .fake
        .history(&HistoryRequest {
            binding: binding.clone(),
            cursor: None,
            page_size: 3,
        })
        .await
        .expect_err("an oversized page is refused");
    assert_eq!(
        too_many,
        RuntimeError::LimitExceeded {
            subject: "history page",
            limit: 2
        }
    );

    assert!(
        team.fake.calls().is_empty(),
        "a limit is checked before the runtime is called"
    );
    assert_eq!(team.fake.committed_messages(&binding), 0);
    assert_eq!(sequences(&team.fake.content(&binding)), vec![1, 2, 3]);
}

#[tokio::test]
async fn cancel_ack_is_not_terminal_until_observed() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;

    let acknowledged = team
        .fake
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("the cancellation is accepted");
    assert_eq!(acknowledged.state, ObservedRunState::Cancelled);
    assert_eq!(acknowledged.source, ObservationSource::CommandAck);
    assert_eq!(
        closes(&team.fake, &acknowledged, &binding).await,
        None,
        "accepting a cancellation is not carrying it out"
    );

    let still_running = team
        .fake
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:06:00Z"),
        })
        .await
        .expect("inspect answers");
    assert_eq!(closes(&team.fake, &still_running, &binding).await, None);

    // Now the runtime actually reports the cancellation.
    team.load(CANCEL, &[]);
    let observed = team
        .fake
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:07:00Z"),
        })
        .await
        .expect("the cancellation is observed");
    assert_eq!(observed.source, ObservationSource::AuthoritativeEvent);
    assert_eq!(
        closes(&team.fake, &observed, &binding).await,
        Some(TerminalOutcome::Cancelled)
    );
}

#[tokio::test]
async fn stream_close_without_terminal_remains_unconfirmed() {
    let team = Team::new(TrustGrade::A).await;
    team.load(HISTORY_LIVE, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    let (_, anchor) = drain_history(&team.fake, &binding, 10)
        .await
        .expect("history pages validate");

    team.fake.push_step(ScriptStep::CloseStreamWithoutTerminal);
    let mut live = team
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");
    while let Some(event) = live.next_event() {
        event.expect("the stream stays continuous while it lasts");
    }
    assert!(live.closed_without_terminal());

    let inspected = team
        .fake
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:09:00Z"),
        })
        .await
        .expect("inspect answers");
    let lost = ControlPlaneObservation {
        contact: RuntimeContact::StreamClosed,
        ..inspected
    };
    assert_eq!(
        closes(&team.fake, &lost, &binding).await,
        None,
        "a closed stream is a fact about the channel, not about the work"
    );

    let core = lost.to_core_observation(EventCursor::parse(1).expect("positive cursor"));
    let derived = derive_run_state(&RunDerivation {
        desired: DesiredRunState::RunRequested,
        observation: Some(&core),
        binding: Some(&lost.identity),
        freshness: Freshness::Fresh,
        contact: RuntimeContact::StreamClosed,
        terminal: None,
    })
    .expect("a derivation without closure evidence succeeds");
    assert_eq!(derived, DerivedRunState::LostContact);
}

// ---------------------------------------------------------------------------
// The script is strict
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_scripted_transport_failure_refuses_every_operation() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    // Loaded after the launch: a step is matched strictly against the operation
    // it belongs to.
    team.load(TRANSPORT_FAILURE, &[]);
    team.fake.take_calls();

    let channel_failed = RuntimeError::Transport {
        rule: "channel failed before the runtime answered",
    };
    let page = || HistoryRequest {
        binding: binding.clone(),
        cursor: None,
        page_size: 10,
    };

    assert_eq!(
        team.fake
            .history(&page())
            .await
            .expect_err("the fixture fails the channel under a read"),
        channel_failed
    );
    assert!(
        team.fake.calls().is_empty(),
        "a channel failure is not a call the runtime answered"
    );
    // Consumed exactly once: the retry reaches the runtime.
    team.fake
        .history(&page())
        .await
        .expect("the channel is fine again");

    // Every operation refuses it, not only the ones that happen to look for it.
    team.fake.push_step(ScriptStep::TransportFailure {
        operation: RuntimeCapability::Resume,
    });
    assert_eq!(
        team.fake
            .resume(&ResumeRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:01:00Z"),
            })
            .await
            .expect_err("resume fails at the channel"),
        channel_failed
    );

    team.fake.push_step(ScriptStep::TransportFailure {
        operation: RuntimeCapability::Cancel,
    });
    assert_eq!(
        team.fake
            .cancel(&CancelRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:05:00Z"),
            })
            .await
            .expect_err("cancel fails at the channel"),
        channel_failed
    );

    team.fake.push_step(ScriptStep::TransportFailure {
        operation: RuntimeCapability::Inspect,
    });
    assert_eq!(
        team.fake
            .inspect(&InspectRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:06:00Z"),
            })
            .await
            .expect_err("inspect fails at the channel"),
        channel_failed
    );

    team.fake.push_step(ScriptStep::TransportFailure {
        operation: RuntimeCapability::PermissionResponse,
    });
    assert_eq!(
        team.fake
            .respond_permission(&PermissionResponseRequest {
                binding: binding.clone(),
                permission_id: ExternalId::parse("perm-anything").expect("valid permission id"),
                response_id: MessageId::generate(),
                decision: PermissionDecision::Allow,
                responded_at: at("2026-08-10T09:07:00Z"),
            })
            .await
            .expect_err("a permission answer fails at the channel"),
        channel_failed
    );

    // Discovery is a call over the same channel. It has no binding, no request
    // and no preflight of its own, which is exactly why it was the operation
    // that quietly kept answering while the channel was scripted down.
    team.fake.push_step_for(
        ScriptStep::TransportFailure {
            operation: RuntimeCapability::Discovery,
        },
        RequestKey::Sessions,
    );
    assert_eq!(
        team.fake
            .discover_sessions()
            .await
            .expect_err("session discovery fails at the channel"),
        channel_failed
    );

    team.fake.push_step_for(
        ScriptStep::TransportFailure {
            operation: RuntimeCapability::Discovery,
        },
        RequestKey::Capabilities,
    );
    assert_eq!(
        team.fake
            .discover_capabilities()
            .await
            .expect_err("the capability read fails at the channel"),
        channel_failed
    );

    // Both discovery calls declare the same capability, so the step pinned to
    // one must not be spent by the other.
    team.fake.push_step_for(
        ScriptStep::TransportFailure {
            operation: RuntimeCapability::Discovery,
        },
        RequestKey::Capabilities,
    );
    assert_eq!(
        team.fake
            .discover_sessions()
            .await
            .expect_err("the queued failure belongs to the capability read"),
        RuntimeError::ScriptRequestMismatch {
            subject: "discovery",
        }
    );
    team.fake
        .discover_capabilities()
        .await
        .expect_err("it is still queued for the call it names");

    // A channel that never answered cannot have changed anything.
    assert_eq!(team.fake.committed_messages(&binding), 0);
    assert!(team.fake.pending_permissions().is_empty());
}

#[tokio::test]
async fn a_binding_the_runtime_never_issued_closes_nothing() {
    let team = Team::new(TrustGrade::A).await;
    let binding = team.role(AgentRunId::generate()).await;
    team.fake.push_step(ScriptStep::CancelObservedTerminal);
    let observed = team
        .fake
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("the cancellation is observed");
    assert_eq!(
        closes(&team.fake, &observed, &binding).await,
        Some(TerminalOutcome::Cancelled),
        "the binding this runtime issued closes the run it observed"
    );

    // A runtime can only vouch for what it issued. The same binding presented
    // to a different runtime is a stranger's, whatever it says about itself.
    let stranger = Team::new(TrustGrade::A).await;
    assert_eq!(
        stranger
            .fake
            .issued_binding(&binding)
            .await
            .expect_err("another runtime never issued this binding"),
        RuntimeError::StaleBinding {
            rule: "this runtime never issued this binding",
        }
    );
    assert_eq!(
        closes(&stranger.fake, &observed, &binding).await,
        None,
        "a foreign binding is not evidence, at any grade"
    );

    // Nor can one be minted. A snapshot assembled from nothing but public
    // fields — consistent correlation, plausible identity, Grade A — is
    // refused before it can promote anything, because the runtime has no
    // record of issuing it.
    let unregistered = RuntimeBindingSnapshot {
        binding: RuntimeBinding {
            id: RuntimeBindingId::generate(),
            ..binding.binding.clone()
        },
        ..binding.clone()
    };
    unregistered
        .ensure_correlated()
        .expect("the forgery is internally consistent, which is the point");
    assert_eq!(
        team.fake
            .issued_binding(&unregistered)
            .await
            .expect_err("this runtime never issued it either"),
        RuntimeError::StaleBinding {
            rule: "this runtime never issued this binding",
        }
    );
    assert_eq!(
        closes(&team.fake, &observed, &unregistered).await,
        None,
        "an unregistered binding closes nothing"
    );
}

#[tokio::test]
async fn a_step_pinned_to_one_request_is_not_spent_on_another() {
    let team = Team::new(TrustGrade::A).await;
    let mine = team.role(AgentRunId::generate()).await;
    let theirs = team.role(AgentRunId::generate()).await;

    // Two roles of one team run reach `cancel` identically. Naming the
    // operation alone would let whichever arrives first spend the other's step.
    team.fake.push_step_for(
        ScriptStep::CancelObservedTerminal,
        RequestKey::Binding(mine.binding_id()),
    );

    assert_eq!(
        team.fake
            .cancel(&CancelRequest {
                binding: theirs.clone(),
                requested_at: at("2026-08-10T09:05:00Z"),
            })
            .await
            .expect_err("the queued step belongs to another binding"),
        RuntimeError::ScriptRequestMismatch { subject: "binding" }
    );

    // It is still queued, and still refuses the wrong operation.
    assert_eq!(
        team.fake
            .inspect(&InspectRequest {
                binding: theirs,
                requested_at: at("2026-08-10T09:06:00Z"),
            })
            .await
            .expect_err("the queued step belongs to another operation"),
        RuntimeError::ScriptMismatch {
            expected: "cancel",
            called: "inspect",
        }
    );

    // The call it was queued for is the one that gets it.
    let observed = team
        .fake
        .cancel(&CancelRequest {
            binding: mine.clone(),
            requested_at: at("2026-08-10T09:07:00Z"),
        })
        .await
        .expect("the cancellation is observed");
    assert_eq!(observed.source, ObservationSource::AuthoritativeEvent);
    assert_eq!(
        closes(&team.fake, &observed, &mine).await,
        Some(TerminalOutcome::Cancelled)
    );
}

// ---------------------------------------------------------------------------
// The shared contracts, stated against the trait
// ---------------------------------------------------------------------------

// The three contracts below live in `kontor_tests_contract`; these tests are
// the fake's entry into them.

#[tokio::test]
async fn scripted_fake_passes_adapter_contract() {
    let team = Team::new(TrustGrade::A).await;
    let request = team.launch_request(AgentRunId::generate()).await;
    adapter_contract(&team.fake, &request)
        .await
        .expect("the scripted fake satisfies the adapter contract");
}

#[tokio::test]
async fn scripted_fake_passes_session_content_contract() {
    let team = Team::new(TrustGrade::A).await;
    team.load(HISTORY_LIVE, &[]);
    let binding = team.role(AgentRunId::generate()).await;
    session_content_contract(&team.fake, &binding)
        .await
        .expect("the scripted fake satisfies the session-content contract");
}

#[tokio::test]
async fn scripted_fake_passes_reconciliation_contract() {
    let team = Team::new(TrustGrade::A).await;
    team.load(ORPHAN, &[CorrelationLabel::for_run(AgentRunId::generate())]);
    let binding = team.role(AgentRunId::generate()).await;
    reconciliation_contract(&team.fake, std::slice::from_ref(&binding))
        .await
        .expect("the scripted fake satisfies the reconciliation contract");
}
