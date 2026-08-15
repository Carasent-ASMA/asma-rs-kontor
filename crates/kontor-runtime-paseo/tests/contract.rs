//! The Paseo 0.3.1 adapter, judged against sanitized recordings of the live
//! daemon and the shared capability-aware contract.
//!
//! Two kinds of test live here, and the split is deliberate. The shared
//! contracts from `kontor_tests_contract` prove this adapter is the *same kind
//! of thing* as every other one — identity preserved, undeclared operations
//! refused before dispatch, no acknowledgement mistaken for a completion. The
//! Paseo-specific cases prove the things only this runtime can get wrong.
//!
//! The mutants this suite exists to kill:
//!
//! * believing a CLI answer that omits the project, the workspace, the labels
//!   and the parent — every placement rule is decided from a session readback,
//!   and skipping it is how a role edits somebody else's tree;
//! * taking an answer by correlation id alone, so an `rpc_error` or another
//!   question's answer decides a placement rule;
//! * treating an unsolicited `agent_stream` frame as the answer to whatever
//!   request happens to be pending, or draining another agent's frames into
//!   this session's timeline;
//! * treating an idle or `finished` agent as a finished run, which replaces a
//!   seat that was merely waiting and doubles the hierarchy every turn, or
//!   missing that retirement is an `archivedAt` stamp rather than a status;
//! * launching into a seat a previous process already filled, or into two seats
//!   for one role name;
//! * retrying a launch, a message or a permission answer whose acknowledgement
//!   was lost, instead of reconciling first;
//! * paging a `projected` timeline, whose collapsed source ranges are holes a
//!   canonical cursor cannot see, or paging past a `gap`/`reset`/`staleCursor`
//!   the page itself declared;
//! * allocating a fresh epoch for a raw one a restore already knew, which makes
//!   every persisted cursor point into a numbering that no longer exists;
//! * writing Paseo's internal state to fix a display name, or letting the host
//!   target reach a ledger, a checkpoint, an error or a fixture.

use std::collections::BTreeMap;
use std::sync::Arc;

use kontor_core::id::{
    AgentRunId, ExternalId, ExternalName, RoleSlotId, RuntimeBindingId, RuntimeKindKey, TaskId,
    TeamRunId,
};
use kontor_core::spec::{EffortLevel, ModelRef, ModelRung, ProviderRef};
use kontor_core::state::{ObservedRunState, RuntimeContact, TerminalOutcome};
use kontor_runtime::adapter::{LaunchOutcome, RuntimeAdapter, RuntimeError, RuntimeResult};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{RuntimeBindingSnapshot, RuntimeCapability, TrustGrade};
use kontor_runtime::observation::{ReconciliationAction, ReconciliationFinding};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, HistoryRequest, InspectRequest, LaunchParts, LaunchRequest,
    LiveSubscribeRequest, MessageId, PermissionDecision, PermissionResponseRequest, ResumeRequest,
    SendMessageRequest,
};
use kontor_runtime::timeline::{HistoryCursor, TimelineBreak, TimelinePosition};
use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_tests_contract::{
    SESSION_KINDS, adapter_contract, assert_native_id_is_not_a_kontor_id, at, closes,
    drain_history, reconciliation_contract, session_content_contract, text,
};

use kontor_runtime_paseo::adapter::{
    PaseoAdapter, PaseoAdoptionIntent, PaseoCheckpoint, PaseoCompaction, PaseoConfig,
    PaseoDelivery, PaseoExecutionScope, PaseoProjectOutcome, PaseoSlotPlan,
};
use kontor_runtime_paseo::client::{PaseoCommand, PaseoTransport};
use kontor_runtime_paseo::fixture::RecordedPaseo;
use kontor_runtime_paseo::wire::{MAX_FRAME_BYTES, label};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("fixtures/paseo-0.3.1/", $name))
    };
}

const VERSION: &str = fixture!("cli/version.txt");
const CLI_WORKSPACE_CREATED: &str = fixture!("cli/workspace-created.json");
const CLI_AGENT_STARTED: &str = fixture!("cli/agent-started.json");
const CLI_AGENT_UPDATED: &str = fixture!("cli/agent-updated.json");
const CLI_AGENT_UPDATED_NEW_ID: &str = fixture!("cli/agent-updated-new-id.json");
const CLI_AGENT_ARCHIVED: &str = fixture!("cli/agent-archived.json");
const CLI_AGENT_STOPPED: &str = fixture!("cli/agent-stopped.json");
const CLI_AGENT_RELOADED: &str = fixture!("cli/agent-reloaded.json");
const SUBSCRIPTION_ACK: &str = fixture!("protocol/subscription-ack.json");

const SERVER_INFO: &str = fixture!("protocol/server-info.json");
const SERVER_INFO_DEGRADED: &str = fixture!("protocol/server-info-degraded.json");
const PROJECT_LIST: &str = fixture!("protocol/project-list.json");
const PROJECT_LIST_EMPTY: &str = fixture!("protocol/project-list-empty.json");
const PROJECT_LIST_RENAMED: &str = fixture!("protocol/project-list-renamed.json");
const PROJECT_LIST_DUPLICATE: &str = fixture!("protocol/project-list-duplicate-name.json");
const PROJECT_ADDED: &str = fixture!("protocol/project-added.json");
const WORKSPACE_LIST_EMPTY: &str = fixture!("protocol/workspace-list-empty.json");
const WORKSPACE_LIST_ONE: &str = fixture!("protocol/workspace-list-one.json");
const WORKSPACE_LIST_TWO: &str = fixture!("protocol/workspace-list-two.json");
const WORKSPACE_OTHER_PROJECT: &str = fixture!("protocol/workspace-other-project.json");
const WORKSPACE_ROOT_LOCAL: &str = fixture!("protocol/workspace-root-local.json");
const WORKSPACE_OTHER_CWD: &str = fixture!("protocol/workspace-other-cwd.json");
const WORKSPACE_PASEO_OWNED: &str = fixture!("protocol/workspace-paseo-owned.json");
const WORKSPACE_NO_ID: &str = fixture!("protocol/workspace-no-id.json");
const AGENT: &str = fixture!("protocol/agent.json");
const AGENT_IDLE_FINISHED: &str = fixture!("protocol/agent-idle-finished.json");
const AGENT_STOPPED: &str = fixture!("protocol/agent-closed.json");
const AGENT_ARCHIVED: &str = fixture!("protocol/agent-archived.json");
const AGENT_WRONG_PARENT_LABEL: &str = fixture!("protocol/agent-wrong-parent-label.json");
const AGENT_ADOPTED_PROVIDER_ROTATED: &str =
    fixture!("protocol/agent-adopted-provider-rotated.json");
const AGENT_OTHER_WORKSPACE: &str = fixture!("protocol/agent-other-workspace.json");
const AGENT_OTHER_CWD: &str = fixture!("protocol/agent-other-cwd.json");
const AGENT_FOREIGN: &str = fixture!("protocol/agent-foreign.json");
const AGENT_ADOPTED: &str = fixture!("protocol/agent-adopted.json");
const AGENT_LIST_EMPTY: &str = fixture!("protocol/agent-list-empty.json");
const AGENT_LIST_IMPLEMENT: &str = fixture!("protocol/agent-list-implement.json");
const AGENT_LIST_WITH_FOREIGN: &str = fixture!("protocol/agent-list-with-foreign.json");
const AGENT_LIST_DUPLICATE_SLOT: &str = fixture!("protocol/agent-list-duplicate-slot.json");
const AGENT_LIST_ARCHIVED_ONLY: &str = fixture!("protocol/agent-list-archived-only.json");
const TIMELINE_GAP: &str = fixture!("protocol/timeline-gap.json");
const TIMELINE_COLLAPSED: &str = fixture!("protocol/timeline-projected-collapsed.json");
const TIMELINE_MESSAGE_TWICE: &str = fixture!("protocol/timeline-message-twice.json");
const TIMELINE_MESSAGE_LANDED: &str = fixture!("protocol/timeline-message-landed.json");
const AGENT_PERMISSION_OPEN: &str = fixture!("protocol/agent-permission-open.json");
const PERMISSION_RESOLVED: &str = fixture!("protocol/permission-resolved.json");
const PERMISSION_RESOLVED_OTHER_AGENT: &str =
    fixture!("protocol/permission-resolved-other-agent.json");
const TIMELINE_MESSAGE_LANDED_NEW_EPOCH: &str =
    fixture!("protocol/timeline-message-landed-new-epoch.json");
const TIMELINE_PAGE_ONE_OF_TWO: &str = fixture!("protocol/timeline-page-one-of-two.json");
const TIMELINE_PAGE_TWO_RENUMBERED: &str = fixture!("protocol/timeline-page-two-renumbered.json");
const AGENT_LIST_SLOT_MOVED: &str = fixture!("protocol/agent-list-slot-moved.json");
const SERVER_INFO_OTHER_VERSION: &str = fixture!("protocol/unsupported-app-version.json");
const TIMELINE_RESET: &str = fixture!("protocol/timeline-reset.json");
const TIMELINE_STALE_CURSOR: &str = fixture!("protocol/timeline-stale-cursor.json");
const CLI_STOPPED_NONE: &str = fixture!("cli/agent-stopped-none.json");
const CLI_STOPPED_OTHER_ID: &str = fixture!("cli/agent-stopped-other-id.json");

// The pinned canonical identifiers the fixtures and the tests share. Generating
// them would make a fixture unable to name them.
const RUN_IMPLEMENT: &str = "01890000-0000-7000-8000-000000000001";
const RUN_QA: &str = "01890000-0000-7000-8000-000000000002";
const TEAM_RUN: &str = "01890000-0000-7000-8000-0000000000a1";
const TASK: &str = "01890000-0000-7000-8000-0000000000b1";
const MESSAGE: &str = "01890000-0000-7000-8000-000000000011";
const MESSAGE_ALT: &str = "01890000-0000-7000-8000-000000000012";

const HOST_KEY: &str = "paseo-dev";
const RUNTIME_KIND: &str = "paseo.agent";
const MINI_PROJECT: &str = "kon-mini-1";
const PROJECT_ID: &str = "prj_epic";
const WORKSPACE_ID: &str = "wks_task11";
const AGENT_ID: &str = "agt_implement";
const ORCHESTRATOR: &str = "agt_orchestrator";
const CWD: &str = "/w/epic/task-11";
const EPOCH_RAW: &str = "8f2b1c34-0000-4000-8000-000000000001";

fn v(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).expect("a fixture is valid JSON")
}

fn external(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a valid external id")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

fn slot(text: &str) -> RoleSlotId {
    RoleSlotId::parse(text).expect("a valid role slot id")
}

fn run(text: &str) -> AgentRunId {
    AgentRunId::parse(text).expect("the fixture pins a canonical AgentRunId")
}

fn team_run() -> TeamRunId {
    TeamRunId::parse(TEAM_RUN).expect("the fixture pins a canonical TeamRunId")
}

fn task() -> TaskId {
    TaskId::parse(TASK).expect("the fixture pins a canonical TaskId")
}

fn root() -> WorkspaceRoot {
    WorkspaceRoot::parse(CWD).expect("an absolute canonical path")
}

/// The standard-fallback context policy a seat launches under when the test is
/// about something else.
///
/// The current Paseo daemon exposes no per-seat context configuration, so the
/// effective half is `not_enforced` — which is exactly what this adapter must
/// keep reporting.
fn standard_context_policy() -> kontor_core::spec::ContextPolicySnapshot {
    kontor_core::spec::ContextPolicySnapshot::standard(
        &kontor_core::spec::ContextWindowBounds::unknown(),
        false,
        kontor_core::id::SCHEMA_VERSION,
        at("2026-08-10T09:00:00Z"),
    )
    .expect("the standard fallback freezes")
}

// ---------------------------------------------------------------------------
// 0.3.1 content builders
// ---------------------------------------------------------------------------

/// One canonical entry, in the shape `fetch_agent_timeline_response` carries.
fn entry(seq: u64, item: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "item": item,
        "timestamp": "2026-08-10T09:30:00.000Z",
        "seqStart": seq,
        "seqEnd": seq,
        "sourceSeqRanges": [{ "startSeq": seq, "endSeq": seq }],
        "collapsed": [],
    })
}

/// A user message carrying the caller's own id as Paseo echoes it back.
fn user_entry(seq: u64, client_message_id: &str) -> serde_json::Value {
    entry(
        seq,
        serde_json::json!({
            "type": "user_message",
            "text": "synthetic text",
            "clientMessageId": client_message_id,
        }),
    )
}

fn assistant_entry(seq: u64) -> serde_json::Value {
    entry(
        seq,
        serde_json::json!({ "type": "assistant_message", "text": "synthetic text" }),
    )
}

fn tool_entry(seq: u64, call_id: &str) -> serde_json::Value {
    entry(
        seq,
        serde_json::json!({
            "type": "tool_call",
            "callId": call_id,
            "name": "synthetic name",
            "status": "completed",
            "error": serde_json::Value::Null,
            "detail": { "type": "plain_text", "text": "synthetic text" },
        }),
    )
}

/// One unsolicited `agent_stream` frame carrying timeline content.
fn stream_entry(agent_id: &str, seq: u64, epoch: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "agent_stream",
        "payload": {
            "agentId": agent_id,
            "event": {
                "type": "timeline",
                "provider": "claude",
                "item": { "type": "assistant_message", "text": "synthetic text" },
            },
            "timestamp": "2026-08-10T09:30:00.000Z",
            "seq": seq,
            "epoch": epoch,
        },
    })
}

// ---------------------------------------------------------------------------
// Plane construction
// ---------------------------------------------------------------------------

fn scope() -> PaseoExecutionScope {
    PaseoExecutionScope {
        jira_epic_key: external("ASMA-7744"),
        mini_project_short_title: name("Kontor MVP"),
        plan_item_key: external("KON-MVP-11"),
        jira_issue_key: external("ASMA-7755"),
        ticket_short_code: external("KON-11"),
        seat_display_roles: [
            (slot("implement-a"), (name("Implement"), Some(name("A")))),
            (slot("implement-b"), (name("Implement"), Some(name("B")))),
            (slot("qa-a"), (name("QA"), None)),
        ]
        .into_iter()
        .collect(),
        // The epic's repository root, which is *not* the task worktree: a
        // project registered from the worktree would be one project per task.
        project_root_cwd: WorkspaceRoot::parse("/w/epic").expect("absolute"),
        canonical_worktree_cwd: root(),
        orchestrator_agent_id: external(ORCHESTRATOR),
    }
}

fn config() -> PaseoConfig {
    PaseoConfig {
        runtime_kind: RuntimeKindKey::parse(RUNTIME_KIND).expect("a valid runtime key"),
        host_key: name(HOST_KEY),
        mini_project_id: external(MINI_PROJECT),
        scope: scope(),
        max_concurrent_sessions: 8,
    }
}

fn model_rung() -> ModelRung {
    ModelRung {
        provider: ProviderRef("claude".to_owned()),
        model: ModelRef("claude-opus-5".to_owned()),
        effort: None,
    }
}

/// Route-equivalent commands. A ledger key is subcommand and addressed id only,
/// so the arguments here are irrelevant to which answers they route.
fn any_workspace_create() -> PaseoCommand {
    PaseoCommand::workspace_create(CWD, PROJECT_ID, "t")
}

fn any_agent_run() -> PaseoCommand {
    PaseoCommand::agent_run(
        WORKSPACE_ID,
        CWD,
        &model_rung(),
        "t",
        &BTreeMap::new(),
        ORCHESTRATOR,
        "p",
    )
}

/// A daemon scripted for the whole happy path: create the workspace, launch one
/// agent, read everything back.
fn daemon() -> RecordedPaseo {
    RecordedPaseo::new()
        .answering(&PaseoCommand::version(), VERSION)
        .answering(&any_workspace_create(), CLI_WORKSPACE_CREATED)
        .answering(&any_agent_run(), CLI_AGENT_STARTED)
        .answering(&PaseoCommand::agent_stop(AGENT_ID), CLI_AGENT_STOPPED)
        .answering(&PaseoCommand::agent_archive(AGENT_ID), CLI_AGENT_ARCHIVED)
        .answering(&PaseoCommand::agent_reload(AGENT_ID), CLI_AGENT_RELOADED)
        .announcing(&v(SERVER_INFO))
        .answering_rpc("project.list.request", v(PROJECT_LIST))
        // 0.3.1 has one workspace request, so the census before a create and
        // the readback after it are the same route answering twice — first an
        // empty project, then the workspace the create put in it. A single
        // standing answer would make "was one created?" unaskable.
        .then_answering_rpc("fetch_workspaces_request", v(WORKSPACE_LIST_EMPTY))
        .answering_rpc("fetch_workspaces_request", v(WORKSPACE_LIST_ONE))
        .answering_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY))
        .answering_rpc("fetch_agent_request", v(AGENT))
        .answering_rpc(
            "agent.timeline.set_subscription.request",
            v(SUBSCRIPTION_ACK),
        )
        .journaling(AGENT_ID, EPOCH_RAW, Vec::new())
}

struct Plane {
    daemon: Arc<RecordedPaseo>,
    adapter: PaseoAdapter,
}

impl Plane {
    fn build(recorded: RecordedPaseo, checkpoint: PaseoCheckpoint) -> Self {
        let daemon = Arc::new(recorded);
        let adapter = PaseoAdapter::new(config(), Box::new(Arc::clone(&daemon)), checkpoint)
            .expect("a consistent checkpoint restores");
        Self { daemon, adapter }
    }

    fn fresh(recorded: RecordedPaseo) -> Self {
        Self::build(recorded, PaseoCheckpoint::fresh(1, name(HOST_KEY)))
    }

    /// A plane with the epic project and the task workspace already prepared.
    async fn prepared(recorded: RecordedPaseo) -> (Self, WorkspaceBindingSnapshot) {
        let plane = Self::fresh(recorded);
        plane
            .adapter
            .prepare_project("cmd-prepare-1")
            .await
            .expect("the epic project is prepared");
        let snapshot = plane.prepare_workspace().await.expect("a task workspace");
        (plane, snapshot)
    }

    async fn prepare_workspace(&self) -> RuntimeResult<WorkspaceBindingSnapshot> {
        self.adapter
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id: team_run(),
                task_id: task(),
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: root(),
                requested_at: at("2026-08-10T09:00:00Z"),
            })
            .await
            .map(|outcome| outcome.snapshot)
    }

    /// Admit and build one launch for `slot_id` under `agent_run_id`.
    async fn launch_request(
        &self,
        agent_run_id: AgentRunId,
        slot_id: &RoleSlotId,
        workspace: &WorkspaceBindingSnapshot,
    ) -> RuntimeResult<LaunchRequest> {
        self.launch_request_for(agent_run_id, slot_id, workspace, model_rung())
            .await
    }

    async fn launch_request_for(
        &self,
        agent_run_id: AgentRunId,
        slot_id: &RoleSlotId,
        workspace: &WorkspaceBindingSnapshot,
        model_rung: ModelRung,
    ) -> RuntimeResult<LaunchRequest> {
        let binding_id = RuntimeBindingId::generate();
        let authority = self
            .adapter
            .admit_launch(&AdmissionRequest {
                slot: RoleSlotKey::new(team_run(), slot_id.clone()),
                agent_run_id,
                binding_id,
                replaces: None,
                requested_at: at("2026-08-10T09:00:00Z"),
            })
            .await?
            .into_authority()?;
        Ok(authority.into_request(LaunchParts {
            agent_run_id,
            team_run_id: team_run(),
            role_slot_id: slot_id.clone(),
            task_id: task(),
            binding_id,
            workspace: Some(workspace.clone()),
            cwd: root(),
            account_profile_id: None,
            prompt: text("bootstrap the role"),
            model_rung,
            context_policy: standard_context_policy(),
            requested_at: at("2026-08-10T09:00:00Z"),
        }))
    }

    async fn launch(
        &self,
        agent_run_id: AgentRunId,
        slot_id: &RoleSlotId,
        workspace: &WorkspaceBindingSnapshot,
    ) -> RuntimeResult<LaunchOutcome> {
        let request = self
            .launch_request(agent_run_id, slot_id, workspace)
            .await?;
        self.adapter.launch(&request).await
    }
}

/// The whole happy path: prepared plane, one launched Implement seat.
async fn launched() -> (Plane, RuntimeBindingSnapshot) {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    let outcome = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the Implement seat launches");
    (plane, outcome.snapshot)
}

// ---------------------------------------------------------------------------
// Shared contracts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shared_adapter_contract_holds() {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    let request = plane
        .launch_request(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("admission");
    adapter_contract(&plane.adapter, &request)
        .await
        .expect("the shared adapter contract holds");
}

#[tokio::test]
async fn shared_session_content_contract_holds() {
    let (plane, binding) = launched().await;
    session_content_contract(&plane.adapter, &binding)
        .await
        .expect("the shared session-content contract holds");
}

#[tokio::test]
async fn shared_reconciliation_contract_holds() {
    let (plane, binding) = launched().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_IMPLEMENT));
    reconciliation_contract(&plane.adapter, &[binding])
        .await
        .expect("the shared reconciliation contract holds");
}

#[tokio::test]
async fn shared_native_ids_are_not_kontor_ids() {
    for native in [PROJECT_ID, WORKSPACE_ID, AGENT_ID, ORCHESTRATOR, EPOCH_RAW] {
        assert_native_id_is_not_a_kontor_id(native);
    }
}

// ---------------------------------------------------------------------------
// hierarchy_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn hierarchy_is_one_project_one_workspace_one_agent_per_slot() {
    let (plane, binding) = launched().await;
    let record = plane
        .adapter
        .seat_record(binding.binding_id())
        .expect("the seat records its whole correlation chain");

    assert_eq!(record.project_id.as_str(), PROJECT_ID);
    assert_eq!(record.workspace_id.as_str(), WORKSPACE_ID);
    assert_eq!(record.agent_id.as_str(), AGENT_ID);
    assert_eq!(record.parent_agent_id.as_str(), ORCHESTRATOR);
    assert_eq!(record.canonical_worktree_cwd, root());
    assert_eq!(record.host_key.as_str(), HOST_KEY);
    assert_eq!(record.jira_epic_key.as_str(), "ASMA-7744");
    assert_eq!(record.plan_item_key.as_str(), "KON-MVP-11");
    assert_eq!(record.team_run_id, team_run());
    assert_eq!(record.role_slot_id, slot("implement-a"));
    assert_eq!(
        record.provider_session_id.as_ref().map(ExternalId::as_str),
        Some("prov_sess_1")
    );

    // One project created, one workspace created, one agent run. The whole
    // acceptance criterion is that these counts stay at one.
    assert_eq!(plane.daemon.count("rpc project.add.request"), 0);
    assert_eq!(plane.daemon.count("workspace create"), 1);
    assert_eq!(plane.daemon.count("agent run"), 1);
}

#[tokio::test]
async fn hierarchy_names_are_compact_and_derived_from_validated_fields() {
    let scope = scope();
    assert_eq!(
        scope.project_display_name(),
        "Epic · ASMA-7744 · Kontor MVP"
    );
    assert_eq!(scope.workspace_display_name(), "TSW · ASMA-7755 · KON-11");
    assert_eq!(
        scope
            .agent_display_name(&slot("implement-a"))
            .expect("the slot has a canonical display role"),
        "Implement · KON-11 · A"
    );
    assert_eq!(
        scope
            .agent_display_name(&slot("implement-b"))
            .expect("the slot has a canonical display role"),
        "Implement · KON-11 · B"
    );
    assert_eq!(
        scope
            .agent_display_name(&slot("qa-a"))
            .expect("the slot has a canonical display role"),
        "QA · KON-11"
    );
}

#[tokio::test]
async fn hierarchy_refuses_a_slot_without_a_canonical_display_role() {
    let error = scope()
        .agent_display_name(&slot("undeclared"))
        .expect_err("an unknown slot must not get an invented title");
    assert!(matches!(error, RuntimeError::LaunchNotAdmitted { .. }));
}

#[tokio::test]
async fn hierarchy_refuses_duplicate_visible_seat_names() {
    let mut scope = scope();
    scope
        .seat_display_roles
        .insert(slot("implement-a"), (name("Implement"), None));
    scope
        .seat_display_roles
        .insert(slot("implement-b"), (name("Implement"), None));
    let error = scope
        .agent_display_name(&slot("implement-a"))
        .expect_err("two tabs must not receive the same visible title");
    assert!(matches!(error, RuntimeError::LaunchNotAdmitted { .. }));
}

#[tokio::test]
async fn hierarchy_reuses_one_epic_project_across_two_task_worktrees() {
    let first = Plane::fresh(daemon());
    let outcome = first
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("the epic project is prepared");
    assert_eq!(first.daemon.count("rpc project.add.request"), 0);

    // A second ticket in the same epic is a second plane restored from the same
    // project binding. It creates no project of its own.
    let mut checkpoint = PaseoCheckpoint::fresh(1, name(HOST_KEY));
    checkpoint.project = Some(outcome.binding().clone());
    let second = Plane::build(daemon(), checkpoint);
    let again = second
        .adapter
        .prepare_project("cmd-2")
        .await
        .expect("the persisted binding is authoritative");
    assert_eq!(again.binding().project_id, outcome.binding().project_id);
    assert_eq!(second.daemon.count("rpc project.add.request"), 0);
}

#[tokio::test]
async fn hierarchy_role_slot_reconciliation_reuses_an_idle_seat_and_materializes_a_missing_one() {
    let (plane, _) = Plane::prepared(daemon()).await;
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_IMPLEMENT));

    let plans = plane
        .adapter
        .reconcile_role_slots(team_run(), &[slot("implement-a"), slot("qa-a")])
        .await
        .expect("the declared roster reconciles");

    assert!(
        matches!(&plans[0], PaseoSlotPlan::Reuse { agent_id, needs_reload, .. }
            if agent_id.as_str() == AGENT_ID && !needs_reload),
        "an existing seat is reused rather than reported vacant, got {:?}",
        plans[0]
    );
    assert!(
        matches!(&plans[1], PaseoSlotPlan::Materialize { .. }),
        "a seat with no agent is materialized exactly once, got {:?}",
        plans[1]
    );
}

#[tokio::test]
async fn hierarchy_two_live_agents_in_one_slot_block_rather_than_pick_one() {
    let (plane, _) = Plane::prepared(daemon()).await;
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_DUPLICATE_SLOT));

    let plans = plane
        .adapter
        .reconcile_role_slots(team_run(), &[slot("implement-a")])
        .await
        .expect("the census is taken");
    assert!(
        matches!(&plans[0], PaseoSlotPlan::Blocked { .. }),
        "two live agents for one seat is divergence, not a choice, got {:?}",
        plans[0]
    );
}

#[tokio::test]
async fn hierarchy_an_archived_agent_leaves_its_slot_vacant() {
    let (plane, _) = Plane::prepared(daemon()).await;
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_ARCHIVED_ONLY));
    let plans = plane
        .adapter
        .reconcile_role_slots(team_run(), &[slot("implement-a")])
        .await
        .expect("the census is taken");
    assert!(matches!(&plans[0], PaseoSlotPlan::Materialize { .. }));
}

// ---------------------------------------------------------------------------
// preparation_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn preparation_is_idempotent_and_creates_nothing_on_replay() {
    let (plane, first) = Plane::prepared(daemon()).await;
    assert_eq!(plane.daemon.count("workspace create"), 1);

    plane.daemon.take_calls();
    let again = plane
        .prepare_workspace()
        .await
        .expect("a repeated preparation returns the original binding");
    assert_eq!(again, first);
    // The capability probe still runs — an operation the runtime no longer
    // declares must be refused rather than answered from a cache — but nothing
    // that touches a workspace does.
    assert!(
        plane.daemon.mutations().is_empty(),
        "a replayed preparation creates nothing, got {:?}",
        plane.daemon.mutations()
    );
    assert!(
        !plane
            .daemon
            .calls()
            .iter()
            .any(|call| call.contains("workspace")),
        "a replayed preparation is answered from state, got {:?}",
        plane.daemon.calls()
    );
}

#[tokio::test]
async fn preparation_reuses_an_existing_workspace_without_creating_one() {
    let recorded = daemon();
    recorded.forget_queued_rpc("fetch_workspaces_request");
    let (plane, snapshot) = Plane::prepared(recorded).await;

    assert_eq!(
        snapshot.binding.identity.native_id.as_str(),
        WORKSPACE_ID,
        "the existing workspace is bound"
    );
    assert_eq!(
        plane.daemon.count("workspace create"),
        0,
        "an exact (project, canonical cwd) match is reused, never duplicated"
    );
}

#[tokio::test]
async fn preparation_reports_rename_pending_and_writes_nothing() {
    // Drift is observed on the *bound* project. Without a binding the display
    // name is the only correlation Paseo's `project.add` leaves behind, so
    // "the name does not match" and "this epic has no project yet" are the same
    // observation; with one, the id is authority and the name is just data.
    let recorded = daemon();
    recorded.set_answer_rpc("project.list.request", v(PROJECT_LIST_RENAMED));
    let mut checkpoint = PaseoCheckpoint::fresh(1, name(HOST_KEY));
    checkpoint.project = Some(kontor_runtime_paseo::adapter::PaseoProjectBinding {
        mini_project_id: external(MINI_PROJECT),
        host_key: name(HOST_KEY),
        project_id: external(PROJECT_ID),
        observed_name: "Epic · ASMA-7744 · Kontor MVP".to_owned(),
    });
    let plane = Plane::build(recorded, checkpoint);

    let outcome = plane
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("a drifted name is still a usable project");
    match outcome {
        PaseoProjectOutcome::ReadyWithRenamePending {
            binding,
            desired_name,
            observed_name,
        } => {
            assert_eq!(binding.project_id.as_str(), PROJECT_ID);
            assert_eq!(desired_name, "Epic · ASMA-7744 · Kontor MVP");
            assert_eq!(observed_name, "kontor mvp (old name)");
        }
        other => panic!("display drift must be reported, got {other:?}"),
    }
    // Nothing was written: no rename, and emphatically no better-named twin.
    assert!(
        plane.daemon.mutations().is_empty(),
        "a display name is not worth writing Paseo's internal state for, got {:?}",
        plane.daemon.mutations()
    );
    assert_eq!(plane.daemon.count("rpc project.add.request"), 0);
    // …and the drift does not block work: the binding is usable exactly as it
    // is, because an id is authority and a display string is not.
    assert_eq!(outcome_binding_id(&plane).await, PROJECT_ID);
}

async fn outcome_binding_id(plane: &Plane) -> String {
    plane
        .adapter
        .project_binding()
        .expect("preparation persisted a binding")
        .project_id
        .as_str()
        .to_owned()
}

#[tokio::test]
async fn preparation_adds_one_project_and_reads_it_back_by_exact_id() {
    let recorded = daemon();
    // Empty before, present after: the add is real and the binding is made from
    // the readback rather than from the acknowledgement.
    recorded.queue_answer_rpc("project.list.request", v(PROJECT_LIST_EMPTY));
    recorded.set_answer_rpc("project.add.request", v(PROJECT_ADDED));
    let plane = Plane::fresh(recorded);

    let outcome = plane
        .adapter
        .prepare_project("cmd-durable-1")
        .await
        .expect("a project is created");
    assert!(!outcome.rename_pending());
    assert_eq!(plane.daemon.count("rpc project.add.request"), 1);
}

#[tokio::test]
async fn preparation_refuses_two_projects_carrying_one_epic_name() {
    let recorded = daemon();
    recorded.set_answer_rpc("project.list.request", v(PROJECT_LIST_DUPLICATE));
    let plane = Plane::fresh(recorded);

    assert!(
        plane.adapter.prepare_project("cmd-1").await.is_err(),
        "an ambiguous prior effect must not become a second project"
    );
    assert!(plane.daemon.mutations().is_empty());
}

#[tokio::test]
async fn preparation_refuses_two_workspaces_at_one_canonical_worktree() {
    let recorded = daemon();
    recorded.forget_queued_rpc("fetch_workspaces_request");
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_LIST_TWO));
    let plane = Plane::fresh(recorded);
    plane
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("the project is prepared");

    let refused = plane
        .prepare_workspace()
        .await
        .expect_err("two workspaces at one path is a diverged hierarchy");
    assert!(matches!(refused, RuntimeError::WorkspaceMismatch { .. }));
    assert_eq!(
        plane.daemon.count("workspace create"),
        0,
        "divergence is reported before anything is created"
    );
}

#[tokio::test]
async fn preparation_refuses_a_workspace_readback_from_another_project() {
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_OTHER_PROJECT));
    let plane = Plane::fresh(recorded);
    plane
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("project");

    let refused = plane
        .prepare_workspace()
        .await
        .expect_err("a workspace in another epic project is not this task's");
    assert!(matches!(refused, RuntimeError::WorkspaceMismatch { .. }));
}

#[tokio::test]
async fn preparation_refuses_a_second_root_for_one_team_run() {
    let (plane, _) = Plane::prepared(daemon()).await;
    let elsewhere = plane
        .adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id: team_run(),
            task_id: task(),
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: WorkspaceRoot::parse("/w/epic/task-99").expect("absolute"),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect_err("one team run works in one place");
    assert!(matches!(elsewhere, RuntimeError::WorkspaceMismatch { .. }));
}

#[tokio::test]
async fn preparation_survives_a_lost_create_ack_without_a_second_create() {
    let recorded = daemon();
    recorded.lose_next(&any_workspace_create());
    // Paseo committed the effect before the channel died, so the census that
    // must run before any retry has something true to find: the pre-create page
    // `daemon()` queues is empty, and every page after it holds the workspace.
    let plane = Plane::fresh(recorded);
    plane
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("project");

    assert!(
        plane.prepare_workspace().await.is_err(),
        "a lost acknowledgement is not a success"
    );
    let recovered = plane
        .prepare_workspace()
        .await
        .expect("the retry finds the effect the lost ack hid");
    assert_eq!(recovered.binding.identity.native_id.as_str(), WORKSPACE_ID);
    assert_eq!(
        plane.daemon.count("workspace create"),
        1,
        "the create count stays at one across a lost acknowledgement"
    );
}

#[tokio::test]
async fn preparation_replays_the_original_ids_after_a_restart_with_zero_creates() {
    let (plane, first) = Plane::prepared(daemon()).await;
    let checkpoint = plane.adapter.checkpoint();
    drop(plane);

    let restarted = Plane::build(daemon(), checkpoint);
    let again = restarted
        .prepare_workspace()
        .await
        .expect("a restored plane replays its own preparation");
    assert_eq!(again, first);
    assert!(
        restarted.daemon.mutations().is_empty(),
        "a restored preparation creates nothing, got {:?}",
        restarted.daemon.mutations()
    );
    assert_eq!(restarted.daemon.count("workspace create"), 0);
}

// ---------------------------------------------------------------------------
// prelaunch_
// ---------------------------------------------------------------------------

/// Every prelaunch refusal asserts the same two things: the typed error, and a
/// call ledger with no mutation in it.
async fn assert_prelaunch_refusal(recorded: RecordedPaseo) -> RuntimeError {
    let plane = Plane::fresh(recorded);
    plane
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("project");
    let workspace = plane.prepare_workspace().await;
    let error = match workspace {
        Err(error) => error,
        Ok(workspace) => plane
            .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
            .await
            .expect_err("this placement must be refused"),
    };
    assert_eq!(
        plane.daemon.count("agent run"),
        0,
        "a refused placement starts no agent"
    );
    error
}

#[tokio::test]
async fn prelaunch_refuses_a_root_or_plain_local_workspace() {
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_ROOT_LOCAL));
    let error = assert_prelaunch_refusal(recorded).await;
    assert!(matches!(error, RuntimeError::WorkspaceMismatch { .. }));
}

#[tokio::test]
async fn prelaunch_refuses_a_canonical_cwd_mismatch() {
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_OTHER_CWD));
    let error = assert_prelaunch_refusal(recorded).await;
    assert!(matches!(error, RuntimeError::WorkspaceMismatch { .. }));
}

#[tokio::test]
async fn prelaunch_refuses_a_paseo_provisioned_worktree() {
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_PASEO_OWNED));
    let error = assert_prelaunch_refusal(recorded).await;
    assert!(matches!(error, RuntimeError::WorkspaceMismatch { .. }));
}

#[tokio::test]
async fn prelaunch_refuses_a_workspace_with_no_id() {
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_NO_ID));
    let error = assert_prelaunch_refusal(recorded).await;
    assert!(matches!(
        error,
        RuntimeError::WorkspaceMismatch { .. } | RuntimeError::CorrelationFailed
    ));
}

#[tokio::test]
async fn prelaunch_refuses_an_agent_the_readback_places_elsewhere() {
    for (fixture, why) in [
        (AGENT_OTHER_WORKSPACE, "another workspace"),
        (AGENT_OTHER_CWD, "another working directory"),
    ] {
        let recorded = daemon();
        recorded.set_answer_rpc("fetch_agent_request", v(fixture));
        let (plane, workspace) = Plane::prepared(recorded).await;
        let error = plane
            .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
            .await
            .expect_err("an agent in {why} is not this seat");
        assert!(
            matches!(error, RuntimeError::WorkspaceMismatch { .. }),
            "placing an agent in {why} must be refused, got {error:?}"
        );
    }
}

#[tokio::test]
async fn prelaunch_refuses_a_duplicate_active_role_slot() {
    let recorded = daemon();
    // A previous process already filled this seat. The admission ledger cannot
    // know that; only a native census can.
    recorded.set_answer_rpc("fetch_agents_request", v(AGENT_LIST_IMPLEMENT));
    let (plane, workspace) = Plane::prepared(recorded).await;

    let error = plane
        .launch(run(RUN_QA), &slot("implement-a"), &workspace)
        .await
        .expect_err("a seat a live agent already holds is not free");
    assert!(matches!(error, RuntimeError::SlotAlreadyAdmitted { .. }));
    assert_eq!(plane.daemon.count("agent run"), 0);
}

#[tokio::test]
async fn prelaunch_refuses_a_workspace_binding_this_runtime_did_not_prepare() {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    let (other, _) = Plane::prepared(daemon()).await;
    // Self-consistent, correctly correlated, and not the one this runtime made.
    let forged = other.prepare_workspace().await.expect("a second binding");
    assert_ne!(forged, workspace);

    let error = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &forged)
        .await
        .expect_err("a workspace binding is only evidence from its own runtime");
    assert!(matches!(error, RuntimeError::WorkspaceMismatch { .. }));
    assert_eq!(plane.daemon.count("agent run"), 0);
}

#[tokio::test]
async fn prelaunch_trusts_no_cli_answer_without_a_protocol_readback() {
    // The CLI says the agent started, and says nothing about where. The
    // protocol readback says it is in another project's workspace. Believing
    // the CLI is what this refusal exists to prevent.
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_agent_request", v(AGENT_OTHER_WORKSPACE));
    let (plane, workspace) = Plane::prepared(recorded).await;

    assert!(
        plane
            .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
            .await
            .is_err(),
        "an id is not a placement"
    );
    assert_eq!(
        plane.daemon.count("rpc fetch_agent_request"),
        1,
        "the readback happened, and it is what decided"
    );
}

#[tokio::test]
async fn prelaunch_refuses_a_route_the_readback_did_not_apply() {
    for (field, value) in [("provider", "codex"), ("model", "claude-fable-5")] {
        let recorded = daemon();
        let mut wrong = v(AGENT);
        wrong["agent"][field] = serde_json::json!(value);
        recorded.set_answer_rpc("fetch_agent_request", wrong);
        let (plane, workspace) = Plane::prepared(recorded).await;

        let error = plane
            .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
            .await
            .expect_err("a runtime default cannot replace the selected route");
        assert_eq!(error, RuntimeError::CorrelationFailed, "field: {field}");
    }
}

#[tokio::test]
async fn prelaunch_refuses_an_effort_the_readback_did_not_apply() {
    let recorded = daemon();
    let mut wrong = v(AGENT);
    wrong["agent"]["thinkingOptionId"] = serde_json::json!("high");
    wrong["agent"]["effectiveThinkingOptionId"] = serde_json::json!("high");
    recorded.set_answer_rpc("fetch_agent_request", wrong);
    let (plane, workspace) = Plane::prepared(recorded).await;
    let mut requested = model_rung();
    requested.effort = Some(EffortLevel::Xhigh);
    let request = plane
        .launch_request_for(
            run(RUN_IMPLEMENT),
            &slot("implement-a"),
            &workspace,
            requested,
        )
        .await
        .expect("the seat is admitted");

    let error = plane
        .adapter
        .launch(&request)
        .await
        .expect_err("the effective effort must match the launch");
    assert_eq!(error, RuntimeError::CorrelationFailed);
}

// ---------------------------------------------------------------------------
// role_slot_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn role_slot_same_role_needs_distinct_slots_to_run_in_parallel() {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the first seat launches");

    // Same role name, second slot: legal, and it is the only spelling that is.
    plane
        .daemon
        .set_answer(&any_agent_run(), fixture!("cli/agent-started-qa.json"));
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(fixture!("protocol/agent-qa.json")));
    plane
        .launch(run(RUN_QA), &slot("qa-a"), &workspace)
        .await
        .expect("a distinct slot is a distinct seat");
    assert_eq!(plane.daemon.count("agent run"), 2);
}

#[tokio::test]
async fn role_slot_a_same_slot_race_yields_one_permit_and_one_agent() {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    let seat = RoleSlotKey::new(team_run(), slot("implement-a"));

    let first = plane
        .adapter
        .admit_launch(&AdmissionRequest {
            slot: seat.clone(),
            agent_run_id: run(RUN_IMPLEMENT),
            binding_id: RuntimeBindingId::generate(),
            replaces: None,
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("the first caller is admitted");
    assert!(first.resumed().is_none());

    // A second caller for the same seat, with freshly minted run and binding
    // ids, finds the seat spoken for. Minting new ids does not help, because
    // the seat is the key.
    let second = plane
        .adapter
        .admit_launch(&AdmissionRequest {
            slot: seat,
            agent_run_id: run(RUN_QA),
            binding_id: RuntimeBindingId::generate(),
            replaces: None,
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect_err("one seat admits one launch");
    assert!(matches!(second, RuntimeError::SlotAlreadyAdmitted { .. }));

    let request = first
        .into_authority()
        .expect("the first caller holds the authority")
        .into_request(LaunchParts {
            agent_run_id: run(RUN_IMPLEMENT),
            team_run_id: team_run(),
            role_slot_id: slot("implement-a"),
            task_id: task(),
            binding_id: RuntimeBindingId::generate(),
            workspace: Some(workspace),
            cwd: root(),
            account_profile_id: None,
            prompt: text("bootstrap"),
            model_rung: model_rung(),
            context_policy: standard_context_policy(),
            requested_at: at("2026-08-10T09:00:00Z"),
        });
    // The parts name a different binding than the reservation does, so even the
    // admitted caller is refused rather than trusted.
    assert!(matches!(
        plane
            .adapter
            .launch(&request)
            .await
            .expect_err("mismatched"),
        RuntimeError::LaunchNotAdmitted { .. }
    ));
    assert_eq!(plane.daemon.count("agent run"), 0);
}

// ---------------------------------------------------------------------------
// lost_launch_ack_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lost_launch_ack_binds_the_one_correlated_agent_without_a_second_run() {
    let recorded = daemon();
    recorded.lose_next(&any_agent_run());
    // The census before the launch is empty; the recovery census, taken after
    // Paseo committed the effect, finds exactly one agent carrying this
    // launch's full label set.
    recorded.queue_answer_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY));
    recorded.queue_answer_rpc("fetch_agents_request", v(AGENT_LIST_IMPLEMENT));
    let (plane, workspace) = Plane::prepared(recorded).await;

    let outcome = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the agent Paseo already created is found by its labels");
    assert_eq!(outcome.snapshot.identity().native_id.as_str(), AGENT_ID);
    assert_eq!(
        plane.daemon.count("agent run"),
        1,
        "recovery is a census, never a second launch"
    );
}

#[tokio::test]
async fn lost_launch_ack_with_no_match_stays_unknown_rather_than_relaunching() {
    let recorded = daemon();
    recorded.lose_next(&any_agent_run());
    let (plane, workspace) = Plane::prepared(recorded).await;

    let error = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect_err("whether Paseo created an agent is not known");
    assert!(matches!(error, RuntimeError::Transport { .. }));
    assert_eq!(
        plane.daemon.count("agent run"),
        1,
        "a blind relaunch is how one seat acquires two agents"
    );
}

#[tokio::test]
async fn lost_launch_ack_with_two_matches_diverges() {
    let recorded = daemon();
    recorded.lose_next(&any_agent_run());
    recorded.queue_answer_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY));
    recorded.queue_answer_rpc("fetch_agents_request", v(AGENT_LIST_DUPLICATE_SLOT));
    let (plane, workspace) = Plane::prepared(recorded).await;

    let error = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect_err("picking one of two would bind a run to somebody else's agent");
    assert_eq!(error, RuntimeError::CorrelationFailed);
    assert_eq!(plane.daemon.count("agent run"), 1);
}

// ---------------------------------------------------------------------------
// adoption_
// ---------------------------------------------------------------------------

async fn adoptable_plane() -> (Plane, WorkspaceBindingSnapshot) {
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_agents_request", v(AGENT_LIST_WITH_FOREIGN));
    recorded.set_answer(
        &PaseoCommand::agent_update_labels("agt_foreign", &BTreeMap::new()),
        CLI_AGENT_UPDATED,
    );
    Plane::prepared(recorded).await
}

fn adopt_request() -> AdoptRequest {
    AdoptRequest {
        agent_run_id: run(RUN_IMPLEMENT),
        binding_id: RuntimeBindingId::generate(),
        native: kontor_core::state::NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(RUNTIME_KIND).expect("valid"),
            host: name(HOST_KEY),
            generation: 1,
            native_id: external("agt_foreign"),
        },
        adopted_at: at("2026-08-10T09:10:00Z"),
    }
}

#[tokio::test]
async fn adoption_preserves_the_native_identity() {
    let (plane, _) = adoptable_plane().await;
    plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", v(AGENT_FOREIGN));
    plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", v(AGENT_ADOPTED));
    plane.adapter.authorize_adoption(PaseoAdoptionIntent {
        native_agent_id: external("agt_foreign"),
        team_run_id: team_run(),
        role_slot_id: slot("implement-a"),
        task_id: task(),
    });

    let outcome = plane
        .adapter
        .adopt(&adopt_request())
        .await
        .expect("an authorized adoption binds the session that is already there");
    assert_eq!(
        outcome.snapshot.identity().native_id.as_str(),
        "agt_foreign",
        "adoption binds the existing agent and does not create one"
    );
    assert_eq!(
        plane.daemon.count("agent run"),
        0,
        "adoption never launches"
    );
    assert_eq!(plane.daemon.count("agent update agt_foreign"), 1);
    let record = plane
        .adapter
        .seat_record(outcome.snapshot.binding_id())
        .expect("an adopted seat records its chain");
    assert_eq!(
        record.provider_session_id.as_ref().map(ExternalId::as_str),
        Some("prov_sess_foreign"),
        "the provider session the operator's work lives in is preserved"
    );
}

#[tokio::test]
async fn adoption_without_an_authorized_intent_touches_nothing() {
    let (plane, _) = adoptable_plane().await;
    plane.daemon.take_calls();

    let error = plane
        .adapter
        .adopt(&adopt_request())
        .await
        .expect_err("discovery is read-only; adoption needs explicit authorization");
    assert!(matches!(error, RuntimeError::LaunchNotAdmitted { .. }));
    assert!(
        plane.daemon.mutations().is_empty(),
        "an unauthorized adoption writes no label at all, got {:?}",
        plane.daemon.mutations()
    );
}

#[tokio::test]
async fn adoption_refuses_a_session_that_already_belongs_to_a_run() {
    let (plane, _) = adoptable_plane().await;
    // The session already carries a Kontor run label. Re-labelling it would move
    // it out from under the run that owns it.
    plane.daemon.set_answer_rpc("fetch_agent_request", v(AGENT));
    plane.adapter.authorize_adoption(PaseoAdoptionIntent {
        native_agent_id: external("agt_foreign"),
        team_run_id: team_run(),
        role_slot_id: slot("implement-a"),
        task_id: task(),
    });
    plane.daemon.take_calls();

    assert_eq!(
        plane
            .adapter
            .adopt(&adopt_request())
            .await
            .expect_err("a session that belongs to a run is not an orphan"),
        RuntimeError::CorrelationFailed
    );
    assert!(plane.daemon.mutations().is_empty());
}

#[tokio::test]
async fn adoption_refuses_a_readback_whose_identity_changed() {
    // Two distinct identity changes, because they are caught by two different
    // things and a single fixture that trips both would let either be deleted
    // silently: the CLI acknowledging another agent id is refused before the
    // readback, and the *same* id with a rotated provider session — a fresh
    // conversation wearing the old name — is refused by the adoption check
    // against the readback.
    for (readback, cli, why) in [
        (AGENT_ADOPTED, CLI_AGENT_UPDATED_NEW_ID, "another agent id"),
        (
            AGENT_ADOPTED_PROVIDER_ROTATED,
            CLI_AGENT_UPDATED,
            "a rotated provider session",
        ),
    ] {
        let (plane, _) = adoptable_plane().await;
        plane
            .daemon
            .queue_answer_rpc("fetch_agent_request", v(AGENT_FOREIGN));
        plane
            .daemon
            .queue_answer_rpc("fetch_agent_request", v(readback));
        plane.daemon.set_answer(
            &PaseoCommand::agent_update_labels("agt_foreign", &BTreeMap::new()),
            cli,
        );
        plane.adapter.authorize_adoption(PaseoAdoptionIntent {
            native_agent_id: external("agt_foreign"),
            team_run_id: team_run(),
            role_slot_id: slot("implement-a"),
            task_id: task(),
        });

        assert_eq!(
            plane
                .adapter
                .adopt(&adopt_request())
                .await
                .expect_err("adoption that changes the identity is not adoption"),
            RuntimeError::CorrelationFailed,
            "adopting into {why} is a create wearing an update's name"
        );
    }
}

// ---------------------------------------------------------------------------
// freshness_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn freshness_a_missing_session_is_lost_contact_and_never_terminal() {
    let (plane, binding) = launched().await;
    // The agent is gone from the census entirely.
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY));

    let report = plane
        .adapter
        .reconcile(std::slice::from_ref(&binding))
        .await
        .expect("reconciliation runs");
    let finding = report
        .findings
        .iter()
        .find(|finding| matches!(finding, ReconciliationFinding::MissingSession { .. }))
        .expect("a bound session that is not there is missing");
    assert_eq!(
        finding.action(),
        ReconciliationAction::ProposeLostContactReview
    );
    assert!(
        report.findings.iter().all(|finding| !matches!(
            finding.proposed_state(),
            Some(kontor_core::state::DerivedRunState::Terminal { .. })
        )),
        "a session that disappeared did not finish"
    );
}

#[tokio::test]
async fn freshness_idle_and_finished_agents_stay_resumable_seats() {
    let (plane, binding) = launched().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_IDLE_FINISHED));

    let observed = plane
        .adapter
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:20:00Z"),
        })
        .await
        .expect("an idle agent is inspectable");
    assert_eq!(observed.state, ObservedRunState::WaitingInput);
    assert_eq!(observed.contact, RuntimeContact::Reachable);
    assert_eq!(
        closes(&plane.adapter, &observed, &binding).await,
        None,
        "`attentionReason=finished` on an idle seat is the turn ending, not the run"
    );

    // …and the next turn is the same agent id, with nothing reloaded.
    plane.daemon.take_calls();
    let resumed = plane
        .adapter
        .resume(&ResumeRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:21:00Z"),
        })
        .await
        .expect("an idle seat takes the next turn");
    assert_eq!(resumed.identity.native_id.as_str(), AGENT_ID);
    assert_eq!(
        plane.daemon.count(&format!("agent reload {AGENT_ID}")),
        0,
        "reloading to simulate a new turn would discard live work"
    );
}

#[tokio::test]
async fn freshness_a_stopped_agent_is_reloaded_and_keeps_its_identity() {
    let (plane, binding) = launched().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_STOPPED));

    let observed = plane
        .adapter
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:20:00Z"),
        })
        .await
        .expect("a stopped agent is inspectable");
    assert_eq!(observed.state, ObservedRunState::Unknown);
    assert_eq!(observed.contact, RuntimeContact::ProcessMissing);
    assert_eq!(
        closes(&plane.adapter, &observed, &binding).await,
        None,
        "a stopped process is lost contact, not a verdict"
    );

    // Resume reloads exactly this case, and the readback must be the same seat.
    plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", v(AGENT_STOPPED));
    plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", v(AGENT));
    plane
        .adapter
        .resume(&ResumeRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:22:00Z"),
        })
        .await
        .expect("an explicitly stopped seat may be reloaded");
    assert_eq!(plane.daemon.count(&format!("agent reload {AGENT_ID}")), 1);
}

#[tokio::test]
async fn freshness_only_a_fresh_archived_readback_is_terminal_evidence() {
    let (plane, binding) = launched().await;

    // A stop acknowledgement is not evidence, however cheerful.
    plane
        .daemon
        .queue_answer_rpc("fetch_agent_request", v(AGENT));
    let cancelled = plane
        .adapter
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:30:00Z"),
        })
        .await
        .expect("the stop is accepted");
    assert_eq!(
        closes(&plane.adapter, &cancelled, &binding).await,
        None,
        "an acknowledgement is not a completion"
    );

    // An explicit archive, read back fresh, is.
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_ARCHIVED));
    let retired = plane
        .adapter
        .retire(&binding, at("2026-08-10T09:31:00Z"))
        .await
        .expect("an archived readback follows an explicit archive intent");
    assert_eq!(
        closes(&plane.adapter, &retired, &binding).await,
        Some(TerminalOutcome::Cancelled)
    );
    assert_eq!(plane.daemon.count(&format!("agent archive {AGENT_ID}")), 1);
}

#[tokio::test]
async fn freshness_an_archive_that_is_not_observed_evidences_nothing() {
    let (plane, binding) = launched().await;
    // Paseo acknowledged the archive and then kept reporting the agent running.
    let refused = plane
        .adapter
        .retire(&binding, at("2026-08-10T09:31:00Z"))
        .await
        .expect_err("an acknowledgement is not an archived state");
    assert_eq!(refused, RuntimeError::CorrelationFailed);
}

#[tokio::test]
async fn freshness_a_refused_or_misaddressed_stop_is_not_a_stop() {
    // Two ways an acknowledgement can be present and mean nothing: Paseo said
    // no, and Paseo said yes about somebody else's agent. Reading either as a
    // stop would hand the control plane a cancellation that never happened.
    for (ack, why) in [
        (CLI_STOPPED_NONE, "the runtime refused"),
        (
            CLI_STOPPED_OTHER_ID,
            "the runtime answered about another agent",
        ),
    ] {
        let (plane, binding) = launched().await;
        plane
            .daemon
            .set_answer(&PaseoCommand::agent_stop(AGENT_ID), ack);
        assert_eq!(
            plane
                .adapter
                .cancel(&CancelRequest {
                    binding,
                    requested_at: at("2026-08-10T09:30:00Z"),
                })
                .await
                .expect_err("an acknowledgement that is not about this stop is not a stop"),
            RuntimeError::CorrelationFailed,
            "{why}"
        );
    }
}

#[tokio::test]
async fn freshness_a_retired_session_cannot_be_resumed() {
    let (plane, binding) = launched().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_ARCHIVED));
    let refused = plane
        .adapter
        .resume(&ResumeRequest {
            binding,
            requested_at: at("2026-08-10T09:40:00Z"),
        })
        .await
        .expect_err("a retired seat is finished, not waiting");
    assert!(matches!(refused, RuntimeError::StaleBinding { .. }));
}

#[tokio::test]
async fn freshness_a_degraded_daemon_is_observed_but_never_driven() {
    let recorded = daemon();
    recorded.set_identity(&v(SERVER_INFO_DEGRADED));
    let plane = Plane::fresh(recorded);

    let declared = plane
        .adapter
        .discover_capabilities()
        .await
        .expect("a degraded daemon still answers");
    assert_eq!(declared.trust_grade, TrustGrade::C);
    assert!(declared.supports(RuntimeCapability::Inspect));
    assert!(!declared.supports(RuntimeCapability::Launch));
    assert!(!declared.supports(RuntimeCapability::PrepareWorkspace));

    // And the missing capability is refused as exactly that, before dispatch.
    plane.daemon.take_calls();
    let refused = plane
        .prepare_workspace()
        .await
        .expect_err("a daemon that did not advertise its features is not driven");
    assert_eq!(
        refused,
        RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::PrepareWorkspace
        }
    );
    assert!(plane.daemon.mutations().is_empty());
}

// ---------------------------------------------------------------------------
// continuity_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn continuity_a_wrong_parent_refuses_the_launch() {
    // 0.3.1 records parentage in exactly one place — the
    // `paseo.parent-agent-id` label — so there is one half to check here, not
    // the two the 0.2.5 wire allowed. The 0.2.5 adapter compared the label
    // against an independent `parentAgentId` field; that field does not exist on
    // this snapshot, and a second check reading the same label under another
    // name would be a check that cannot fail.
    let recorded = daemon();
    recorded.set_answer_rpc("fetch_agent_request", v(AGENT_WRONG_PARENT_LABEL));
    let (plane, workspace) = Plane::prepared(recorded).await;

    assert_eq!(
        plane
            .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
            .await
            .expect_err("an agent under another orchestrator is not this seat"),
        RuntimeError::CorrelationFailed
    );
}

#[tokio::test]
async fn continuity_parent_and_predecessor_survive_a_restart() {
    let (plane, binding) = launched().await;
    plane
        .adapter
        .link_predecessor(binding.binding_id(), external("agt_predecessor"))
        .expect("the successor links what it replaced");

    let checkpoint = plane.adapter.checkpoint();
    drop(plane);
    let restarted = Plane::build(daemon(), checkpoint);

    let record = restarted
        .adapter
        .seat_record(binding.binding_id())
        .expect("the chain survives a restart");
    assert_eq!(record.parent_agent_id.as_str(), ORCHESTRATOR);
    assert_eq!(
        record.previous_agent_id.as_ref().map(ExternalId::as_str),
        Some("agt_predecessor")
    );
    // …and the binding it names is still vouched for, which is what makes it
    // usable rather than merely present.
    restarted
        .adapter
        .issued_binding(&binding)
        .await
        .expect("a restored binding is still the runtime's own");
}

#[tokio::test]
async fn continuity_a_restored_seat_refuses_a_second_launch() {
    let (plane, workspace) = Plane::prepared(daemon()).await;
    plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the seat is filled");
    let checkpoint = plane.adapter.checkpoint();
    drop(plane);

    let restarted = Plane::build(daemon(), checkpoint);
    let refused = restarted
        .adapter
        .admit_launch(&AdmissionRequest {
            slot: RoleSlotKey::new(team_run(), slot("implement-a")),
            agent_run_id: run(RUN_QA),
            binding_id: RuntimeBindingId::generate(),
            replaces: None,
            requested_at: at("2026-08-10T10:00:00Z"),
        })
        .await
        .expect_err("a restored occupied seat is still occupied");
    assert!(matches!(refused, RuntimeError::SlotAlreadyAdmitted { .. }));
    assert!(restarted.daemon.mutations().is_empty());
}

#[tokio::test]
async fn continuity_a_checkpoint_from_another_host_or_generation_is_refused() {
    let (plane, _) = launched().await;
    let good = plane.adapter.checkpoint();

    let mut foreign_host = good.clone();
    foreign_host.host_key = name("paseo-somewhere-else");
    assert!(
        PaseoAdapter::new(config(), Box::new(Arc::new(daemon())), foreign_host).is_err(),
        "a checkpoint taken against another host describes another runtime"
    );

    let mut wrong_generation = good.clone();
    wrong_generation.generation = 2;
    assert!(
        PaseoAdapter::new(config(), Box::new(Arc::new(daemon())), wrong_generation).is_err(),
        "a repeated native id in a new generation is a different session"
    );

    let mut colliding_epochs = good;
    colliding_epochs.epochs = vec![("epoch-a".to_owned(), 1), ("epoch-b".to_owned(), 1)];
    assert!(
        PaseoAdapter::new(config(), Box::new(Arc::new(daemon())), colliding_epochs).is_err(),
        "two raw epochs mapping to one Kontor epoch splices two numberings into one cursor"
    );
}

// ---------------------------------------------------------------------------
// timeline_
// ---------------------------------------------------------------------------

/// A plane whose seat already has four canonical entries.
async fn with_history() -> (Plane, RuntimeBindingSnapshot) {
    let recorded = daemon().journaling(
        AGENT_ID,
        EPOCH_RAW,
        vec![
            user_entry(1, "msg_someone_else"),
            assistant_entry(2),
            tool_entry(3, "call_1"),
            tool_entry(4, "call_1"),
        ],
    );
    let (plane, workspace) = Plane::prepared(recorded).await;
    let outcome = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the seat launches");
    (plane, outcome.snapshot)
}

#[tokio::test]
async fn timeline_history_then_live_has_no_gap_and_no_overlap() {
    let (plane, binding) = with_history().await;
    let (history, anchor) = drain_history(&plane.adapter, &binding, 2)
        .await
        .expect("history pages");
    assert_eq!(
        history
            .iter()
            .map(|event| event.position.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "one Kontor event per native sequence"
    );
    assert_eq!(anchor.sequence, 4);

    // Live delivers strictly after the anchor, and the buffered frame and the
    // catch-up fetch of the same entry are one event.
    plane
        .daemon
        .push_stream(AGENT_ID, vec![stream_entry(AGENT_ID, 5, EPOCH_RAW)]);
    let mut live = plane
        .adapter
        .subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("a live subscription");
    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("no break").position.sequence);
    }
    assert_eq!(
        delivered,
        vec![5],
        "nothing before the anchor is redelivered"
    );
    assert!(
        live.closed_without_terminal(),
        "a stream that ends is a channel fact, never a completion"
    );
}

#[tokio::test]
async fn timeline_a_collapsed_projection_is_refused_rather_than_paged() {
    let (plane, binding) = with_history().await;
    // What a `projected` read looks like: one entry covering a range.
    plane
        .daemon
        .set_answer_rpc("fetch_agent_timeline_request", v(TIMELINE_COLLAPSED));

    let refused = plane
        .adapter
        .history(&HistoryRequest {
            binding,
            cursor: None,
            page_size: 10,
        })
        .await
        .expect_err("a collapsed range is a hole a canonical cursor cannot page over");
    assert_eq!(
        refused,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );
}

#[tokio::test]
async fn timeline_a_native_gap_breaks_the_page() {
    let (plane, binding) = with_history().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_timeline_request", v(TIMELINE_GAP));

    // 0.3.1 declares the hole on the response itself, so the refusal happens
    // one step earlier than it did on the 0.2.5 wire: the page never becomes
    // events at all, because paging over a gap the daemon just admitted to is
    // the thing the flag exists to prevent.
    let refused = plane
        .adapter
        .history(&HistoryRequest {
            binding: binding.clone(),
            cursor: None,
            page_size: 10,
        })
        .await
        .expect_err("a page that declares a gap is not a page");
    assert_eq!(
        refused,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );
}

#[tokio::test]
async fn timeline_a_declared_break_forces_a_canonical_refetch() {
    // 0.3.1 puts `gap`, `reset` and `staleCursor` on the timeline *response*
    // rather than on the stream, so this is where a hole is admitted to and
    // where delivery has to stop. Each one ends the read and demands a
    // canonical refetch; none of them says anything about the run.
    for (page, reason) in [
        (TIMELINE_GAP, TimelineBreak::SequenceGap),
        (TIMELINE_RESET, TimelineBreak::EpochChanged),
        (TIMELINE_STALE_CURSOR, TimelineBreak::EpochChanged),
    ] {
        let (plane, binding) = with_history().await;
        let (_, anchor) = drain_history(&plane.adapter, &binding, 10)
            .await
            .expect("history");
        plane
            .daemon
            .set_answer_rpc("fetch_agent_timeline_request", v(page));

        let refused = plane
            .adapter
            .subscribe_live(&LiveSubscribeRequest {
                binding: binding.clone(),
                kinds: SESSION_KINDS.iter().copied().collect(),
                strict_after: anchor,
            })
            .await
            .expect_err("a suppressed gap is a hole the control plane cannot see");
        assert_eq!(
            refused,
            RuntimeError::TimelineRefetchRequired { reason },
            "a declared break must demand a refetch"
        );
        // And it changed no lifecycle state.
        let checkpoint = plane.adapter.checkpoint();
        assert_eq!(checkpoint.bindings.len(), 1);
    }
}

#[tokio::test]
async fn timeline_restart_keeps_the_raw_epoch_mapping() {
    let (plane, binding) = with_history().await;
    let (_, anchor) = drain_history(&plane.adapter, &binding, 10)
        .await
        .expect("history");
    let checkpoint = plane.adapter.checkpoint();
    assert_eq!(
        checkpoint.epochs,
        vec![(EPOCH_RAW.to_owned(), anchor.epoch)],
        "the raw epoch is persisted as itself, never hashed"
    );
    drop(plane);

    let restarted = Plane::build(
        daemon().journaling(AGENT_ID, EPOCH_RAW, vec![user_entry(1, "msg_someone_else")]),
        checkpoint,
    );
    // Continuing from the persisted cursor must resolve the *same* Kontor epoch.
    // Allocating a fresh one would make the cursor point into a numbering that
    // no longer exists, and the read would silently start over.
    let page = restarted
        .adapter
        .history(&HistoryRequest {
            binding,
            cursor: Some(HistoryCursor::issue(
                RuntimeBindingSnapshot::binding_id(&restarted.adapter.checkpoint().bindings[0]),
                anchor,
            )),
            page_size: 10,
        })
        .await
        .expect("a restored cursor still resolves");
    assert_eq!(page.epoch, anchor.epoch);
}

#[tokio::test]
async fn timeline_a_renumbered_epoch_breaks_a_restored_cursor() {
    // The other half of the epoch rule, and the dangerous one. Keeping the
    // mapping is only useful if a raw epoch the mapping does *not* know is
    // refused: silently allocating a fresh Kontor epoch for it would splice a
    // renumbered transcript onto a cursor from the old numbering, and every
    // sequence after that would be a lie the continuity guard cannot see.
    let (plane, binding) = with_history().await;
    let (_, anchor) = drain_history(&plane.adapter, &binding, 10)
        .await
        .expect("history");
    let checkpoint = plane.adapter.checkpoint();
    drop(plane);

    let restarted = Plane::build(
        daemon().journaling(
            AGENT_ID,
            "8f2b1c34-0000-4000-8000-00000000000f",
            vec![user_entry(1, "msg_someone_else")],
        ),
        checkpoint,
    );
    let refused = restarted
        .adapter
        .history(&HistoryRequest {
            binding,
            cursor: Some(HistoryCursor::issue(
                RuntimeBindingSnapshot::binding_id(&restarted.adapter.checkpoint().bindings[0]),
                anchor,
            )),
            page_size: 10,
        })
        .await
        .expect_err("a raw epoch the registry does not know is a renumbering");
    assert_eq!(
        refused,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
}

#[tokio::test]
async fn timeline_a_cursor_from_another_session_is_refused() {
    let (plane, binding) = with_history().await;
    let refused = plane
        .adapter
        .history(&HistoryRequest {
            binding,
            cursor: Some(HistoryCursor::issue(
                RuntimeBindingId::generate(),
                TimelinePosition::start_of(1),
            )),
            page_size: 10,
        })
        .await
        .expect_err("a foreign cursor is refused rather than reset");
    assert!(matches!(refused, RuntimeError::InvalidCursor { .. }));
}

#[tokio::test]
async fn timeline_a_permission_observed_on_a_readback_stays_pending() {
    // 0.3.1's canonical timeline has no permission items, so a raised request
    // is a row in the agent snapshot. Observing it is what makes answering it
    // possible at all: a permission the adapter never saw raised cannot be
    // answered.
    let (plane, binding) = with_permission().await;
    let checkpoint = plane.adapter.checkpoint();
    assert_eq!(
        checkpoint.pending_permissions,
        vec![(binding.binding_id(), external("perm_1"))],
        "a request Paseo reports pending is pending"
    );

    resolve_permission(&plane);
    plane
        .adapter
        .respond_permission(&PermissionResponseRequest {
            binding,
            permission_id: external("perm_1"),
            response_id: MessageId::parse(MESSAGE).expect("pinned"),
            decision: PermissionDecision::Allow,
            responded_at: at("2026-08-10T09:50:00Z"),
        })
        .await
        .expect("a pending permission can be answered");
}

// ---------------------------------------------------------------------------
// message_
// ---------------------------------------------------------------------------

fn message(binding: &RuntimeBindingSnapshot, body: &str) -> SendMessageRequest {
    SendMessageRequest {
        binding: binding.clone(),
        message_id: MessageId::parse(MESSAGE).expect("pinned"),
        body: text(body),
        sent_at: at("2026-08-10T09:40:00Z"),
    }
}

#[tokio::test]
async fn message_same_id_yields_one_native_message_and_one_ack() {
    let (plane, binding) = launched().await;
    let request = message(&binding, "the next turn");

    let first = plane
        .adapter
        .send(&request)
        .await
        .expect("the message lands");
    let replay = plane.adapter.send(&request).await.expect("a retry replays");
    assert_eq!(first, replay, "a retried message replays its own result");
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        1,
        "the second call never reached the wire"
    );
    assert_eq!(
        plane.daemon.journal_len(AGENT_ID),
        1,
        "exactly one native user message exists for this id"
    );
    assert_eq!(
        first.position.sequence, 1,
        "the position comes from the timeline, not from a counter this adapter kept"
    );
}

#[tokio::test]
async fn message_a_changed_body_under_one_id_is_rejected() {
    let (plane, binding) = launched().await;
    plane
        .adapter
        .send(&message(&binding, "the next turn"))
        .await
        .expect("the first body lands");

    let refused = plane
        .adapter
        .send(&message(&binding, "a different instruction"))
        .await
        .expect_err("one identifier is one effect");
    assert!(matches!(refused, RuntimeError::DuplicateMessage { .. }));
    assert_eq!(plane.daemon.journal_len(AGENT_ID), 1);
}

#[tokio::test]
async fn message_a_lost_ack_is_reconciled_rather_than_resent() {
    let (plane, binding) = launched().await;
    plane.daemon.lose_next_rpc("send_agent_message_request");
    // Paseo committed the effect before the channel died, and canonical history
    // is where that is visible.
    plane
        .daemon
        .set_answer_rpc("fetch_agent_timeline_request", v(TIMELINE_MESSAGE_LANDED));

    let acknowledged = plane
        .adapter
        .send(&message(&binding, "the next turn"))
        .await
        .expect("the timeline settles what the acknowledgement could not");
    assert_eq!(acknowledged.position.sequence, 1);
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        1,
        "the message was never sent a second time"
    );
}

#[tokio::test]
async fn message_a_lost_ack_under_a_renumbered_epoch_is_a_break_not_a_fresh_epoch() {
    let (plane, binding) = with_history().await;
    // Reading the content is what makes an epoch this session's epoch.
    drain_history(&plane.adapter, &binding, 10)
        .await
        .expect("history");
    plane.daemon.lose_next_rpc("send_agent_message_request");
    // The reconciliation's answer comes back under a raw epoch this session has
    // never been read in. "Did my message land?" cannot be answered out of a
    // numbering the question was not asked in — and allocating a fresh epoch
    // here would turn that unanswerable into a confident `no`, which authorizes
    // exactly the resend the reconciliation exists to prevent.
    plane.daemon.set_answer_rpc(
        "fetch_agent_timeline_request",
        v(TIMELINE_MESSAGE_LANDED_NEW_EPOCH),
    );

    let refused = plane
        .adapter
        .send(&message(&binding, "the next turn"))
        .await
        .expect_err("an unknown raw epoch during reconciliation is a break");
    assert_eq!(
        refused,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        1,
        "the message was never sent a second time"
    );
}

#[tokio::test]
async fn message_a_scan_pins_the_epoch_for_every_later_reconciliation() {
    // No `history` call anywhere in this test. A plane that is driven rather
    // than read still has to have an epoch, and the reconciliation's own scan is
    // what gives it one — otherwise the expectation is never set, and every raw
    // epoch that turns up is allocated as if it were the first.
    let (plane, binding) = launched().await;
    plane.daemon.lose_next_rpc("send_agent_message_request");
    plane
        .daemon
        .set_answer_rpc("fetch_agent_timeline_request", v(TIMELINE_MESSAGE_LANDED));
    plane
        .adapter
        .send(&message(&binding, "the first turn"))
        .await
        .expect("the timeline settles what the acknowledgement could not");

    // A later reconciliation, answered under a raw epoch this session has never
    // been read in. Allocating a fresh one here answers "did it land?" out of a
    // numbering the question was never asked in, and a `no` from it authorizes
    // the resend the whole reconciliation exists to prevent.
    plane.daemon.lose_next_rpc("send_agent_message_request");
    plane.daemon.set_answer_rpc(
        "fetch_agent_timeline_request",
        v(TIMELINE_MESSAGE_LANDED_NEW_EPOCH),
    );
    let refused = plane
        .adapter
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id: MessageId::parse(MESSAGE_ALT).expect("pinned"),
            body: text("the next turn"),
            sent_at: at("2026-08-10T09:41:00Z"),
        })
        .await
        .expect_err("a renumbered epoch during a later reconciliation is a break");
    assert_eq!(
        refused,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        2,
        "one attempt each, and neither was sent a second time"
    );
}

#[tokio::test]
async fn message_a_renumbering_between_two_pages_of_one_scan_is_a_break() {
    // No cursor exists yet — this scan is the first read of the session, so
    // there is nothing persisted for its pages to be judged against. The
    // continuity that has to hold is *within* the scan: page one arrives under
    // one epoch, and page two under another, which is Paseo renumbering the
    // transcript while it was being read.
    let (plane, binding) = launched().await;
    plane.daemon.lose_next_rpc("send_agent_message_request");
    plane
        .daemon
        .queue_answer_rpc("fetch_agent_timeline_request", v(TIMELINE_PAGE_ONE_OF_TWO));
    plane.daemon.set_answer_rpc(
        "fetch_agent_timeline_request",
        v(TIMELINE_PAGE_TWO_RENUMBERED),
    );

    let refused = plane
        .adapter
        .send(&message(&binding, "the first turn"))
        .await
        .expect_err("page two belongs to a transcript page one was not part of");
    assert_eq!(
        refused,
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
    assert_eq!(
        plane.daemon.count("rpc fetch_agent_timeline_request"),
        2,
        "the scan did page on: the break is between two pages, not on the first"
    );

    // Page two carries the landed message and an open permission request, and
    // neither may leave a mark: content read out of a numbering this scan was
    // not asking in is not evidence about anything.
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        1,
        "one attempt, and no resend authorized by page two"
    );
    let checkpoint = plane.adapter.checkpoint();
    assert!(
        matches!(
            checkpoint.deliveries.as_slice(),
            [(_, _, PaseoDelivery::ConfirmationUnknown)]
        ),
        "the delivery stays unconfirmed rather than acknowledged from page two"
    );
    assert!(
        checkpoint.pending_permissions.is_empty(),
        "page two's permission request was never opened"
    );
    assert!(
        checkpoint.cursors.is_empty(),
        "a scan that broke mid-way has no single epoch to claim"
    );
}

#[tokio::test]
async fn message_two_native_entries_for_one_id_are_divergence() {
    let (plane, binding) = launched().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_timeline_request", v(TIMELINE_MESSAGE_TWICE));

    let refused = plane
        .adapter
        .send(&message(&binding, "the next turn"))
        .await
        .expect_err("one id delivered twice is not a luckier answer");
    assert!(matches!(refused, RuntimeError::DuplicateMessage { .. }));
}

#[tokio::test]
async fn message_restart_replays_the_original_ack_without_a_second_send() {
    let (plane, binding) = launched().await;
    let request = message(&binding, "the next turn");
    let first = plane
        .adapter
        .send(&request)
        .await
        .expect("the message lands");

    let checkpoint = plane.adapter.checkpoint();
    assert_eq!(checkpoint.deliveries.len(), 1);
    let journal = plane.daemon.journal_len(AGENT_ID);
    drop(plane);

    // A new adapter over a new daemon that has never seen this message. If the
    // ledger did not survive, this would send again — and the count says whether
    // it did.
    let restarted = Plane::build(daemon(), checkpoint);
    let replay = restarted
        .adapter
        .send(&message(&binding, "the next turn"))
        .await
        .expect("the restored ledger answers");
    assert_eq!(replay, first, "the acknowledgement is the same one");
    assert_eq!(
        restarted.daemon.count("rpc send_agent_message_request"),
        0,
        "a restart must not duplicate a delivery"
    );
    assert_eq!(journal, 1);
}

// ---------------------------------------------------------------------------
// permission_
// ---------------------------------------------------------------------------

/// A plane whose seat has one permission request open.
///
/// 0.3.1's canonical timeline carries no permission items at all, so an open
/// request is a row in the agent snapshot's `pendingPermissions` and reading
/// *the agent* — not the transcript — is what makes it known. The seat also
/// starts with two entries of content, because the acknowledgement a resolution
/// produces is stamped at the session's last read position and a session nobody
/// has read has none.
async fn with_permission() -> (Plane, RuntimeBindingSnapshot) {
    let recorded = daemon().journaling(
        AGENT_ID,
        EPOCH_RAW,
        vec![user_entry(1, "msg_someone_else"), assistant_entry(2)],
    );
    recorded.set_answer_rpc("fetch_agent_request", v(AGENT_PERMISSION_OPEN));
    recorded.set_answer_rpc("agent_permission_response", v(PERMISSION_RESOLVED));
    let (plane, workspace) = Plane::prepared(recorded).await;
    let outcome = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the seat launches");
    drain_history(&plane.adapter, &outcome.snapshot, 10)
        .await
        .expect("history");
    // An inspect is a fresh agent readback, and that is where the pending
    // request is observed.
    plane
        .adapter
        .inspect(&InspectRequest {
            binding: outcome.snapshot.clone(),
            requested_at: at("2026-08-10T09:45:00Z"),
        })
        .await
        .expect("an inspect reads the pending permission");
    (plane, outcome.snapshot)
}

/// Answer as Paseo would once the request has left `pendingPermissions`.
fn resolve_permission(plane: &Plane) {
    plane.daemon.set_answer_rpc("fetch_agent_request", v(AGENT));
}

fn permission(
    binding: &RuntimeBindingSnapshot,
    decision: PermissionDecision,
) -> PermissionResponseRequest {
    PermissionResponseRequest {
        binding: binding.clone(),
        permission_id: external("perm_1"),
        response_id: MessageId::parse(MESSAGE).expect("pinned"),
        decision,
        responded_at: at("2026-08-10T09:50:00Z"),
    }
}

#[tokio::test]
async fn permission_response_is_session_bound_and_idempotent() {
    let (plane, binding) = with_permission().await;
    let request = permission(&binding, PermissionDecision::Allow);
    resolve_permission(&plane);

    let first = plane
        .adapter
        .respond_permission(&request)
        .await
        .expect("the answer is applied");
    assert_eq!(first.decision, PermissionDecision::Allow);
    assert_eq!(
        first.position.sequence, 2,
        "0.3.1 records no resolution in the transcript, so the acknowledgement \
         is stamped at the session's last read position rather than at an \
         invented one"
    );

    let replay = plane
        .adapter
        .respond_permission(&request)
        .await
        .expect("the same answer replays");
    assert_eq!(first, replay);
    assert_eq!(
        plane.daemon.count("rpc agent_permission_response"),
        1,
        "the replay never reached the wire"
    );

    // A contradicting answer is refused rather than applied.
    let refused = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Deny))
        .await
        .expect_err("an answered request is not answerable again differently");
    assert!(matches!(refused, RuntimeError::PermissionConflict { .. }));
}

#[tokio::test]
async fn permission_an_unknown_request_is_refused_before_dispatch() {
    let (plane, binding) = launched().await;
    plane.daemon.take_calls();
    let refused = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect_err("a permission this runtime never saw raised cannot be answered");
    assert!(matches!(refused, RuntimeError::PermissionConflict { .. }));
    assert!(
        plane.daemon.mutations().is_empty(),
        "nothing was dispatched, got {:?}",
        plane.daemon.mutations()
    );
}

#[tokio::test]
async fn permission_an_unknown_delivery_is_reconciled_not_resent() {
    let (plane, binding) = with_permission().await;
    plane.daemon.lose_next_rpc("agent_permission_response");
    // Paseo applied it before the channel died, so the readback no longer
    // reports the request pending.
    resolve_permission(&plane);

    let acknowledged = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect("the timeline settles what the acknowledgement could not");
    assert_eq!(acknowledged.position.sequence, 2);
    assert_eq!(
        plane.daemon.count("rpc agent_permission_response"),
        1,
        "an unknown outcome is never blindly resent"
    );
}

#[tokio::test]
async fn permission_survives_a_restart_as_the_same_acknowledgement() {
    let (plane, binding) = with_permission().await;
    resolve_permission(&plane);
    let first = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect("the answer is applied");
    let checkpoint = plane.adapter.checkpoint();
    drop(plane);

    let restarted = Plane::build(daemon(), checkpoint);
    let replay = restarted
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect("the restored ledger answers");
    assert_eq!(first, replay);
    assert_eq!(
        restarted.daemon.count("rpc agent_permission_response"),
        0,
        "a restart must not duplicate a permission response"
    );
}

#[tokio::test]
async fn permission_an_open_request_survives_a_restart_and_is_answered_once() {
    let (plane, binding) = with_permission().await;

    // The request is open and unanswered. A checkpoint that forgot it would
    // make a perfectly valid answer arrive at an adapter that has never heard
    // of the request — refused for being unknown, which is not what happened.
    let checkpoint = plane.adapter.checkpoint();
    assert_eq!(
        checkpoint.pending_permissions,
        vec![(binding.binding_id(), external("perm_1"))],
        "an unanswered request survives naming the session that raised it"
    );
    drop(plane);

    let recorded = daemon().journaling(
        AGENT_ID,
        EPOCH_RAW,
        vec![user_entry(1, "msg_someone_else"), assistant_entry(2)],
    );
    recorded.set_answer_rpc("agent_permission_response", v(PERMISSION_RESOLVED));
    let restarted = Plane::build(recorded, checkpoint);
    let acknowledged = restarted
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect("a request raised before the restart is still answerable after it");
    assert_eq!(
        restarted.daemon.count("rpc agent_permission_response"),
        1,
        "the answer reached the runtime exactly once"
    );

    // And it is answered once: the restored ledger now owns the answer, so the
    // same request replays instead of dispatching a second time.
    let replay = restarted
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect("the same answer replays");
    assert_eq!(acknowledged, replay);
    assert_eq!(
        restarted.daemon.count("rpc agent_permission_response"),
        1,
        "a resolved request is never dispatched twice"
    );
}

#[tokio::test]
async fn permission_a_resolution_paseo_reports_is_never_answered_a_second_time() {
    // The operator answered it in Paseo's own UI. There is no acknowledgement of
    // Kontor's for it — no response id, no decision — only the request having
    // left `pendingPermissions`, which on this wire is the whole of the
    // evidence that somebody answered.
    let (plane, binding) = with_permission().await;
    resolve_permission(&plane);
    plane
        .adapter
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:46:00Z"),
        })
        .await
        .expect("a readback shows the request resolved");
    plane.daemon.take_calls();

    let refused = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect_err("a request Paseo shows answered is not answerable again");
    assert!(matches!(refused, RuntimeError::PermissionConflict { .. }));
    assert!(
        plane.daemon.mutations().is_empty(),
        "nothing was dispatched, got {:?}",
        plane.daemon.mutations()
    );

    let checkpoint = plane.adapter.checkpoint();
    assert!(
        checkpoint.pending_permissions.is_empty(),
        "an answered request is not pending"
    );
    assert_eq!(
        checkpoint.resolved_in_history,
        vec![external("perm_1")],
        "the resolution is what has to survive, because no acknowledgement can"
    );
    drop(plane);

    // After the restart the daemon reports the request pending again — a
    // provider that re-raises it, or simply a readback taken before Paseo
    // settled. Without the persisted resolution it looks open, and the
    // duplicate answer the operator never asked for goes out.
    let restarted = Plane::build(daemon(), checkpoint);
    restarted
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_PERMISSION_OPEN));
    restarted
        .adapter
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:47:00Z"),
        })
        .await
        .expect("a readback");
    restarted.daemon.take_calls();

    let refused = restarted
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect_err("re-reading the request does not reopen an answered one");
    assert!(matches!(refused, RuntimeError::PermissionConflict { .. }));
    assert!(
        restarted.daemon.mutations().is_empty(),
        "nothing was dispatched after the restart, got {:?}",
        restarted.daemon.mutations()
    );
    // …and the re-read did not put it back into the pending set either. A
    // checkpoint that lists an answered request as pending is an untrue fact
    // about the plane, whether or not the refusal above happens to mask it.
    assert!(
        restarted
            .adapter
            .checkpoint()
            .pending_permissions
            .is_empty(),
        "an answered request never becomes pending again"
    );
}

// ---------------------------------------------------------------------------
// placement_ — a seat is re-proved where it is on every turn, not once
// ---------------------------------------------------------------------------

/// A launched seat whose next readback puts it somewhere else.
async fn moved(agent: &str) -> (Plane, RuntimeBindingSnapshot) {
    let (plane, binding) = launched().await;
    plane.daemon.set_answer_rpc("fetch_agent_request", v(agent));
    plane.daemon.take_calls();
    (plane, binding)
}

#[tokio::test]
async fn placement_a_moved_session_is_driven_no_further() {
    // Labels travel with an agent, so every one of these still answers its
    // census wearing the right name. Only the placement readback disagrees.
    for agent in [AGENT_OTHER_WORKSPACE, AGENT_OTHER_CWD] {
        let (plane, binding) = moved(agent).await;
        let refused = plane
            .adapter
            .resume(&ResumeRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:20:00Z"),
            })
            .await
            .expect_err("a moved session is not resumed");
        assert!(
            matches!(refused, RuntimeError::WorkspaceMismatch { .. }),
            "got {refused:?}"
        );

        let (plane, binding) = moved(agent).await;
        let refused = plane
            .adapter
            .send(&message(&binding, "the next turn"))
            .await
            .expect_err("a moved session is not messaged");
        assert!(
            matches!(refused, RuntimeError::WorkspaceMismatch { .. }),
            "got {refused:?}"
        );
        assert!(
            plane.daemon.mutations().is_empty(),
            "the turn never reached the wire, got {:?}",
            plane.daemon.mutations()
        );
        assert_eq!(plane.daemon.count("rpc send_agent_message_request"), 0);
    }
}

#[tokio::test]
async fn placement_a_workspace_that_stopped_being_the_task_worktree_stops_the_turn() {
    // The agent has not moved. The *workspace* has been re-registered under it —
    // as the project root, as a plain local directory, or replaced by one Paseo
    // provisioned for itself. None of that is visible from the agent readback.
    for workspace in [
        WORKSPACE_ROOT_LOCAL,
        WORKSPACE_PASEO_OWNED,
        WORKSPACE_OTHER_PROJECT,
    ] {
        let (plane, binding) = launched().await;
        plane
            .daemon
            .set_answer_rpc("fetch_workspaces_request", v(workspace));
        plane.daemon.take_calls();

        let refused = plane
            .adapter
            .send(&message(&binding, "the next turn"))
            .await
            .expect_err("a workspace that is no longer the task worktree stops the turn");
        assert!(
            matches!(refused, RuntimeError::WorkspaceMismatch { .. }),
            "got {refused:?}"
        );
        assert_eq!(
            plane.daemon.count("rpc send_agent_message_request"),
            0,
            "the refusal came before the wire"
        );
    }
}

#[tokio::test]
async fn placement_adoption_reproves_the_workspace_before_it_writes_a_label() {
    let (plane, _) = adoptable_plane().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_FOREIGN));
    plane.adapter.authorize_adoption(PaseoAdoptionIntent {
        native_agent_id: external("agt_foreign"),
        team_run_id: team_run(),
        role_slot_id: slot("implement-a"),
        task_id: task(),
    });
    // The session is where it should be; the workspace under it is not.
    plane
        .daemon
        .set_answer_rpc("fetch_workspaces_request", v(WORKSPACE_PASEO_OWNED));
    plane.daemon.take_calls();

    let refused = plane
        .adapter
        .adopt(&adopt_request())
        .await
        .expect_err("a session is not adopted into a worktree Paseo owns");
    assert!(
        matches!(refused, RuntimeError::WorkspaceMismatch { .. }),
        "got {refused:?}"
    );
    // Adoption's whole risk is the label write onto somebody else's session.
    assert!(
        plane.daemon.mutations().is_empty(),
        "no label was written, got {:?}",
        plane.daemon.mutations()
    );
}

#[tokio::test]
async fn placement_a_reregistered_workspace_is_not_reused_under_its_old_id() {
    // The id survives the re-registration, so every id-based check still agrees:
    // the census finds the seat, the agent reports the right workspace id, and
    // the labels are untouched. Only the workspace readback disagrees.
    for workspace in [
        WORKSPACE_ROOT_LOCAL,
        WORKSPACE_PASEO_OWNED,
        WORKSPACE_OTHER_PROJECT,
    ] {
        let (plane, _) = Plane::prepared(daemon()).await;
        plane
            .daemon
            .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_IMPLEMENT));
        plane
            .daemon
            .set_answer_rpc("fetch_workspaces_request", v(workspace));

        let plans = plane
            .adapter
            .reconcile_role_slots(team_run(), &[slot("implement-a")])
            .await
            .expect("the census is taken");
        // Not `Reuse`: that plans the seat's next turn there.
        assert!(
            matches!(plans.as_slice(), [PaseoSlotPlan::Blocked { .. }]),
            "a {workspace:?} workspace produced {plans:?}"
        );

        // …and not `Materialize` either, which is the other half and the one an
        // agent-side check can never reach: with no seat there yet, there is no
        // misplaced agent to notice, and the plan would *create* one in a
        // workspace that stopped being the task worktree.
        plane
            .daemon
            .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_EMPTY));
        let plans = plane
            .adapter
            .reconcile_role_slots(team_run(), &[slot("implement-a")])
            .await
            .expect("the census is taken");
        assert!(
            matches!(plans.as_slice(), [PaseoSlotPlan::Blocked { .. }]),
            "an empty census in a {workspace:?} workspace produced {plans:?}"
        );
    }
}

#[tokio::test]
async fn placement_a_labelled_agent_outside_the_workspace_blocks_the_slot() {
    let (plane, _) = Plane::prepared(daemon()).await;
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_SLOT_MOVED));

    let plans = plane
        .adapter
        .reconcile_role_slots(team_run(), &[slot("implement-a")])
        .await
        .expect("the census is taken");
    // Neither reuse nor materialize: reusing drives the seat in the wrong tree,
    // and materializing leaves two live agents answering for one slot.
    assert!(
        matches!(plans.as_slice(), [PaseoSlotPlan::Blocked { .. }]),
        "got {plans:?}"
    );
}

// ---------------------------------------------------------------------------
// security_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn security_a_daemon_off_the_pinned_baseline_is_observed_not_driven() {
    // Every DTO, argv and label spelling here was recorded against 0.3.1. A
    // full feature list from an unrecognized build does not re-establish that,
    // and Grade A is exactly the claim that it did. The degraded fixture is the
    // other half: a build that *is* 0.3.1 but withholds a required feature is
    // just as undriveable, and for a different reason.
    for (info, why) in [
        (
            SERVER_INFO_OTHER_VERSION,
            "an unrecognized application version",
        ),
        (SERVER_INFO_DEGRADED, "a missing required feature"),
    ] {
        let recorded = daemon();
        recorded.set_identity(&v(info));
        let plane = Plane::fresh(recorded);

        let declared = plane
            .adapter
            .discover_capabilities()
            .await
            .expect("the runtime is still reachable, which is a separate fact");
        assert_eq!(
            declared.trust_grade,
            TrustGrade::C,
            "{why} makes a Paseo build advisory"
        );
        assert!(declared.supports(RuntimeCapability::Inspect));
        assert!(
            !declared.supports(RuntimeCapability::Launch),
            "nothing is driven on a build this adapter was not recorded against"
        );
    }
}

#[tokio::test]
async fn security_an_oversized_frame_is_refused_at_every_acceptance_point() {
    let oversized = serde_json::json!({ "filler": "x".repeat(MAX_FRAME_BYTES) });
    let bound = RuntimeError::Transport {
        rule: "frame exceeded the bounded frame size",
    };

    // The request/response half: an answer is refused before it is parsed into
    // anything the adapter would then act on.
    let (plane, binding) = with_history().await;
    plane
        .daemon
        .set_answer_rpc("fetch_agent_timeline_request", oversized.clone());
    let refused = plane
        .adapter
        .history(&HistoryRequest {
            binding: binding.clone(),
            cursor: None,
            page_size: 10,
        })
        .await
        .expect_err("an oversized answer is refused");
    assert_eq!(refused, bound);

    // The pushed half: a subscription frame is bounded too, and refused before
    // the epoch registry or the timeline guard sees it.
    let (plane, binding) = with_history().await;
    let (_, anchor) = drain_history(&plane.adapter, &binding, 10)
        .await
        .expect("history");
    plane.daemon.push_stream(AGENT_ID, vec![oversized]);
    let refused = plane
        .adapter
        .subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect_err("an oversized stream frame is refused");
    assert_eq!(refused, bound);
    // And it changed no lifecycle state.
    plane
        .adapter
        .issued_binding(&binding)
        .await
        .expect("the binding is untouched by a refused frame");
}

#[tokio::test]
async fn security_no_endpoint_or_credential_reaches_a_ledger_checkpoint_or_error() {
    let (plane, binding) = launched().await;
    plane
        .adapter
        .send(&message(&binding, "the next turn"))
        .await
        .expect("a message");

    let ledger = format!("{:?}", plane.daemon.calls());
    let checkpoint = format!("{:?}", plane.adapter.checkpoint());
    let surfaces = [ledger, checkpoint, format!("{:?}", plane.adapter.config())];
    for surface in &surfaces {
        for forbidden in [
            "--host",
            "http://",
            "https://",
            "password",
            "@",
            "the next turn",
        ] {
            assert!(
                !surface.contains(forbidden),
                "a client-visible surface must not carry {forbidden}: {surface}"
            );
        }
    }
    // What it *does* carry is the non-secret host key, which is the whole point
    // of storing a key rather than a target.
    assert!(surfaces[1].contains(HOST_KEY));
}

#[tokio::test]
async fn security_every_lifecycle_command_is_argv_json_and_hostless() {
    let (plane, _) = launched().await;
    for call in plane.daemon.calls() {
        assert!(
            !call.contains("--host") && !call.contains(CWD),
            "a ledger entry must be a subcommand and an id, got {call}"
        );
    }
    // The commands themselves ask for JSON and never carry the host, which the
    // transport owns.
    for command in [
        PaseoCommand::version(),
        any_workspace_create(),
        any_agent_run(),
        PaseoCommand::agent_stop(AGENT_ID),
        PaseoCommand::agent_archive(AGENT_ID),
        PaseoCommand::agent_reload(AGENT_ID),
        PaseoCommand::workspace_archive(WORKSPACE_ID),
        PaseoCommand::agent_update_labels(AGENT_ID, &BTreeMap::new()),
    ] {
        assert!(command.argv().iter().any(|argument| argument == "--json"));
        assert!(!command.argv().iter().any(|argument| argument == "--host"));
    }
}

#[tokio::test]
async fn security_the_full_label_set_is_planted_and_verified() {
    let (plane, binding) = launched().await;
    // Every label key the plan names travels, and the readback is what proves
    // it: a launch whose agent lacks one is refused, which the placement tests
    // above exercise from the other side.
    let record = plane
        .adapter
        .seat_record(binding.binding_id())
        .expect("a seat record");
    assert_eq!(record.agent_id.as_str(), AGENT_ID);
    let answer: serde_json::Value = v(AGENT);
    for key in label::ALL {
        assert!(
            answer["agent"]["labels"].get(*key).is_some(),
            "{key} must be planted on the agent"
        );
    }
}

// ---------------------------------------------------------------------------
// compaction_
// ---------------------------------------------------------------------------

#[tokio::test]
async fn compaction_is_reported_unsupported_and_never_simulated() {
    let (plane, _) = launched().await;
    assert_eq!(
        plane.adapter.compaction_status(false),
        PaseoCompaction::Unsupported
    );
    assert_eq!(
        plane.adapter.compaction_status(true),
        PaseoCompaction::Pending,
        "a policy that demands a compacted seat blocks rather than proceeds"
    );
    // And nothing was reloaded, replaced or archived to pretend otherwise.
    assert!(
        !plane
            .daemon
            .calls()
            .iter()
            .any(|call| call.starts_with("agent reload") || call.starts_with("agent archive")),
        "compaction is never simulated with a reload or a replacement"
    );
}

// ---------------------------------------------------------------------------
// transport_ — the 0.3.1 socket's own fail-closed rules
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transport_an_unrecognized_app_version_refuses_every_driving_operation() {
    // The pushed identity is the gate, and it is checked before anything is
    // driven rather than after something has already been started. A daemon on
    // an unrecognized build stays observable — that is what keeps an unknown
    // Paseo visible in the adoption inbox — and every operation that would
    // change it is refused as exactly the capability it is.
    let recorded = daemon();
    recorded.set_identity(&v(SERVER_INFO_OTHER_VERSION));
    let plane = Plane::fresh(recorded);
    plane
        .adapter
        .prepare_project("cmd-1")
        .await
        .expect("reading the project list is not driving anything");
    plane.daemon.take_calls();

    let refused = plane
        .prepare_workspace()
        .await
        .expect_err("an undeclared capability produces no effect");
    assert_eq!(
        refused,
        RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::PrepareWorkspace
        }
    );
    assert!(
        plane.daemon.mutations().is_empty(),
        "the refusal happened before the wire, got {:?}",
        plane.daemon.mutations()
    );
}

#[tokio::test]
async fn transport_a_wrong_request_correlation_fails_closed() {
    // Two shapes of the same defect, and the second is the one an id-only check
    // lets through: an answer stamped with somebody else's correlation id, and
    // an answer stamped with *this* id that is a different kind of answer
    // entirely. Both would decide a placement rule from a frame about something
    // else, so both refuse.
    for (inject, why) in [
        (
            "misroute" as &str,
            "an answer carrying another request's id",
        ),
        ("wrong-type", "a same-id answer of another response type"),
    ] {
        let (plane, binding) = launched().await;
        plane.daemon.take_calls();
        if inject == "misroute" {
            plane.daemon.misroute_next_rpc("fetch_agent_request");
        } else {
            plane
                .daemon
                .wrong_response_type_next_rpc("fetch_agent_request");
        }

        let refused = plane
            .adapter
            .inspect(&InspectRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T10:00:00Z"),
            })
            .await
            .expect_err("{why} is not this request's answer");
        assert!(
            matches!(refused, RuntimeError::Transport { .. }),
            "{why} must fail closed, got {refused:?}"
        );
        assert!(
            plane.daemon.mutations().is_empty(),
            "a misrouted answer changes nothing, got {:?}",
            plane.daemon.mutations()
        );
    }
}

#[tokio::test]
async fn transport_an_unsolicited_frame_for_another_agent_never_drains_for_this_one() {
    // The socket is multiplexed, so agent B's pushed frames arrive on the same
    // connection as agent A's answers. Routing them by arrival — or by the
    // subscription that happens to be open — would splice another session's
    // content into this one's timeline, at sequence numbers that look perfectly
    // ordinary.
    let (plane, binding) = with_history().await;
    let (_, anchor) = drain_history(&plane.adapter, &binding, 10)
        .await
        .expect("history");
    plane.daemon.push_stream(
        AGENT_ID,
        vec![
            stream_entry("agt_qa", 5, EPOCH_RAW),
            stream_entry(AGENT_ID, 5, EPOCH_RAW),
        ],
    );

    let mut live = plane
        .adapter
        .subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("a live subscription");
    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("no break").position.sequence);
    }
    assert_eq!(
        delivered,
        vec![5],
        "exactly this agent's frame is delivered, once"
    );

    // …and the other agent's frame is still where it belongs, unread by this
    // session's drain.
    let theirs = plane
        .daemon
        .drain_stream("agt_qa")
        .await
        .expect("the other agent's queue");
    assert_eq!(
        theirs.len(),
        1,
        "agent B's frame was buffered for agent B, not consumed by agent A"
    );
}

#[tokio::test]
async fn permission_an_acknowledgement_for_another_agent_is_refused() {
    // The resolution frame is correlated by the permission request id, and that
    // id is all it shares with the answer. So the agent it names is checked too:
    // a resolution about somebody else's session, carrying the id this request
    // was sent under, would otherwise close a permission on evidence from
    // another conversation.
    let (plane, binding) = with_permission().await;
    resolve_permission(&plane);
    plane.daemon.set_answer_rpc(
        "agent_permission_response",
        v(PERMISSION_RESOLVED_OTHER_AGENT),
    );

    let refused = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect_err("a resolution about another agent is not this answer");
    assert_eq!(
        refused,
        RuntimeError::Transport {
            rule: "runtime acknowledged something other than this permission answer"
        }
    );
}

// ---------------------------------------------------------------------------
// Context policy and compaction
// ---------------------------------------------------------------------------

fn compaction_policy(
    enforcement: kontor_core::spec::ContextEnforcement,
) -> kontor_core::spec::ContextPolicySnapshot {
    let declared = kontor_core::spec::ContextWindowPolicy {
        enforcement,
        ..kontor_core::spec::ContextWindowPolicy::standard()
    };
    let resolved =
        kontor_core::spec::resolve_context_window(&kontor_core::spec::ContextPolicyInputs {
            role_slot: Some(&declared),
            ..kontor_core::spec::ContextPolicyInputs::default()
        })
        .expect("the slot declaration resolves");
    let requested =
        kontor_core::spec::RequestedContextPolicy::of(&resolved, kontor_core::id::SCHEMA_VERSION);
    // Paseo declares no context configuration, so the effective half is derived
    // against an unsupported runtime — which is what `not_enforced` means.
    let effective = kontor_core::spec::EffectiveContextPolicy::derive(
        &requested,
        &kontor_core::spec::ContextWindowBounds::unknown(),
        false,
    )
    .expect("best effort derives against an incapable runtime");
    kontor_core::spec::ContextPolicySnapshot::freeze(
        requested,
        effective,
        at("2026-08-10T09:00:00Z"),
    )
    .expect("both halves freeze")
}

/// The current daemon surface exposes neither capability, and says so.
#[tokio::test]
async fn the_current_daemon_advertises_no_context_or_compaction_capability() {
    let (plane, _) = launched().await;
    let declared = plane
        .adapter
        .discover_capabilities()
        .await
        .expect("capabilities are discoverable");

    assert!(
        !declared.supports(RuntimeCapability::ContextPolicy),
        "Paseo 0.3.1 exposes no per-seat context configuration"
    );
    assert!(
        !declared.supports(RuntimeCapability::Compact),
        "Paseo 0.3.1 exposes no compaction operation"
    );
    // Unknown bounds stay unknown: no number is invented on the daemon's behalf.
    assert_eq!(declared.limits.context_window.safe_ceiling_tokens, None);
    assert_eq!(declared.limits.context_window.minimum_trigger_tokens, None);
}

/// MUT-CTX-05. Reporting an unsupported Paseo compaction as `Confirmed` makes
/// this fail — and so does substituting any daemon mutation for it.
#[tokio::test]
async fn compaction_is_reported_unsupported_and_mutates_nothing() {
    let (plane, binding) = launched().await;
    let before = binding.identity().clone();
    plane.daemon.take_calls();

    let receipt = plane
        .adapter
        .compact(&kontor_runtime::request::CompactRequest {
            binding: binding.clone(),
            receipt_id: kontor_core::id::CompactionReceiptId::generate(),
            trigger: kontor_core::compaction::CompactionTrigger::Threshold,
            policy: compaction_policy(kontor_core::spec::ContextEnforcement::BestEffort),
            context_pack_hash: kontor_core::id::ContentHash::of(b"context-pack"),
            handoff_hash: None,
            requested_at: at("2026-08-10T09:30:00Z"),
        })
        .await
        .expect("an incapable daemon reports rather than fails");

    assert_eq!(
        receipt.status,
        kontor_core::compaction::CompactionStatus::Unsupported
    );
    assert_ne!(
        receipt.status,
        kontor_core::compaction::CompactionStatus::Confirmed
    );
    // Nothing was re-read, because nothing was done.
    assert_eq!(receipt.native_after, None);
    assert!(receipt.telemetry.is_unknown());

    // The substitutions that would look like compaction from outside, and are
    // all a different session: none of them happened.
    assert!(
        plane.daemon.mutations().is_empty(),
        "an unsupported compaction must emit no daemon mutation, got {:?}",
        plane.daemon.mutations()
    );
    for forbidden in [
        "reload", "archive", "close", "cancel", "create", "spawn", "replace",
    ] {
        assert!(
            !plane
                .daemon
                .calls()
                .iter()
                .any(|call| call.contains(forbidden)),
            "`{forbidden}` is not a compaction substitute, got {:?}",
            plane.daemon.calls()
        );
    }
    // The seat is still the same native session it was.
    assert_eq!(binding.identity(), &before);
}

/// A `required` policy never reaches a Paseo seat at all.
///
/// The approved policy allows either branch — block reuse with `pending`, or
/// reject the launch before any effect. On this daemon it is the second, and it
/// happens at the earliest possible moment: the effective half cannot even be
/// derived, so no launch is ever assembled, let alone dispatched.
///
/// That is stronger than a `pending` receipt would be, and it is why a required
/// seat cannot silently run unenforced here.
#[tokio::test]
async fn a_required_policy_cannot_be_frozen_for_a_seat_this_daemon_runs() {
    let declared = kontor_core::spec::ContextWindowPolicy {
        enforcement: kontor_core::spec::ContextEnforcement::Required,
        ..kontor_core::spec::ContextWindowPolicy::standard()
    };
    let resolved =
        kontor_core::spec::resolve_context_window(&kontor_core::spec::ContextPolicyInputs {
            role_slot: Some(&declared),
            ..kontor_core::spec::ContextPolicyInputs::default()
        })
        .expect("the slot declaration resolves");
    let requested =
        kontor_core::spec::RequestedContextPolicy::of(&resolved, kontor_core::id::SCHEMA_VERSION);

    let refused = kontor_core::spec::EffectiveContextPolicy::derive(
        &requested,
        &kontor_core::spec::ContextWindowBounds::unknown(),
        // Paseo declares no `ContextPolicy` capability.
        false,
    )
    .expect_err("a required policy cannot be honoured by this daemon");
    assert!(matches!(
        refused,
        kontor_core::DomainError::MissingEvidence { .. }
    ));
}
