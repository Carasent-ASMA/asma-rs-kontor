//! The AO 0.12.1 adapter, judged against recorded fixtures and the shared
//! capability-aware contract.
//!
//! Two kinds of test live here, and the split is deliberate. The shared contracts
//! from `kontor_tests_contract` prove the AO adapter is the *same kind of thing*
//! as every other adapter — identity preserved, undeclared operations refused
//! before dispatch, no acknowledgement mistaken for a completion. The AO-specific
//! cases prove the things only this runtime can get wrong.
//!
//! The mutants this suite exists to kill:
//!
//! * mapping AO's `idle`, `exited`, `no_signal` or a merged pull request onto a
//!   terminal outcome — any of them would close a run that is still going;
//! * treating a kill acknowledgement as evidence the session stopped;
//! * retrying a launch or a follow-up whose acknowledgement was lost, so one run
//!   acquires two agents or one instruction is executed twice;
//! * accepting a sequence jump, or persisting one AO change twice;
//! * launching Codex under a permission mode AO resolves to an
//!   approvals-and-sandbox bypass, including through `restore`;
//! * adopting a foreign AO session, or inferring a parent AO never recorded;
//! * answering an undeclared `history`, `live_events` or `permission_response`
//!   with an empty success instead of a typed refusal.

use std::collections::BTreeSet;
use std::sync::Arc;

use kontor_core::id::{
    AgentRunId, BoundedText, ExternalId, ExternalName, RuntimeBindingId, RuntimeKindKey, TaskId,
    TeamRunId,
};
use kontor_core::repository::RuntimeBinding;
use kontor_core::state::{
    DerivedRunState, NativeRuntimeIdentity, ObservedRunState, RuntimeContact, TerminalOutcome,
};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError};
use kontor_runtime::capability::{RuntimeBindingSnapshot, RuntimeCapability, TrustGrade};
use kontor_runtime::observation::{
    CorrelationEvidence, ObservationSource, ReconciliationAction, ReconciliationFinding,
};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, HistoryRequest, InspectRequest, LaunchRequest,
    LiveSubscribeRequest, MessageId, PermissionDecision, PermissionResponseRequest, ResumeRequest,
    SendMessageRequest,
};
use kontor_runtime::timeline::{TimelineBreak, TimelinePosition};
use kontor_runtime::workspace::{
    WorkspaceBinding, WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspaceCorrelationEvidence,
    WorkspaceLabel, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_tests_contract::{
    SESSION_KINDS, adapter_contract, assert_native_id_is_not_a_kontor_id, assert_unsupported, at,
    closes, reconciliation_contract, session_content_contract, text,
};

use kontor_runtime_ao::adapter::{
    AO_VERSION, AoAdapter, AoAttention, AoCheckpoint, AoDelivery, AoLane, UNSUPPORTED,
    normalize_lifecycle,
};
use kontor_runtime_ao::client::AoCall;
use kontor_runtime_ao::fixture::RecordedAo;
use kontor_runtime_ao::wire::{
    AoActivityState, AoHarness, AoListAgentsResponse, AoListSessionsResponse, AoPermissionMode,
    AoSessionKind, AoSessionStatus, AoSessionView, mux, parse_sse_events,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../tests/fixtures/ao-0.12.1/", $name))
    };
}

const MANIFEST: &str = fixture!("manifest.json");
const AGENTS: &str = fixture!("agents.json");
const PROJECT_SAFE: &str = fixture!("project-safe.json");
const PROJECT_UNSAFE_EMPTY: &str = fixture!("project-unsafe-empty.json");
const PROJECT_UNSAFE_DEFAULT: &str = fixture!("project-unsafe-default.json");
const PROJECT_UNSAFE_BYPASS_OVERRIDE: &str = fixture!("project-unsafe-bypass-override.json");
const PROJECT_UNSAFE_UNKNOWN: &str = fixture!("project-unsafe-unknown.json");
const PROJECT_DEGRADED: &str = fixture!("project-degraded.json");
const PROJECT_OTHER_PATH: &str = fixture!("project-other-path.json");
const PROJECT_OTHER_ID: &str = fixture!("project-other-id.json");

const INVENTORY: &str = fixture!("sessions-inventory.json");
const INVENTORY_WITHOUT_CLAUDE: &str = fixture!("sessions-inventory-without-claude.json");
const INVENTORY_DIVERGED: &str = fixture!("sessions-inventory-diverged.json");

const SESSION_LIVE: &str = fixture!("session-claude-live.json");
const SESSION_IDLE: &str = fixture!("session-claude-idle.json");
const SESSION_BLOCKED: &str = fixture!("session-claude-blocked.json");
const SESSION_WAITING: &str = fixture!("session-claude-waiting.json");
const SESSION_NEEDS_INPUT: &str = fixture!("session-claude-needs-input-unclassified.json");
const SESSION_NO_SIGNAL: &str = fixture!("session-claude-no-signal.json");
const SESSION_EXITED: &str = fixture!("session-claude-exited.json");
const SESSION_TERMINATED: &str = fixture!("session-claude-terminated.json");
const SESSION_MERGED: &str = fixture!("session-claude-merged.json");
const SESSION_APPROVED: &str = fixture!("session-claude-approved.json");
const SESSION_LIVE_FOREIGN_PROJECT: &str = fixture!("session-claude-live-foreign-project.json");

const SPAWN_CLAUDE: &str = fixture!("spawn-claude.json");
const SPAWN_CODEX: &str = fixture!("spawn-codex.json");
const SPAWN_CURSOR: &str = fixture!("spawn-cursor.json");
const SPAWN_OPENCODE: &str = fixture!("spawn-opencode.json");
const SPAWN_WRONG_CORRELATION: &str = fixture!("spawn-wrong-correlation.json");
const SPAWN_NATIVE_ID_AS_BRANCH: &str = fixture!("spawn-native-id-as-branch.json");
const SPAWN_NO_BRANCH: &str = fixture!("spawn-no-branch.json");
const SPAWN_FOREIGN_PROJECT: &str = fixture!("spawn-foreign-project.json");
const SPAWN_WRONG_HARNESS: &str = fixture!("spawn-wrong-harness.json");
const SPAWN_WRONG_KIND: &str = fixture!("spawn-wrong-kind.json");
const SPAWN_NO_HARNESS: &str = fixture!("spawn-no-harness.json");

const SEND_OK: &str = fixture!("send-ok.json");
const SEND_NOT_OK: &str = fixture!("send-not-ok.json");
const SEND_OTHER_SESSION: &str = fixture!("send-other-session.json");
const KILL_OK: &str = fixture!("kill-ok.json");
const RESTORE_OK: &str = fixture!("restore-ok.json");
const RESTORE_FOREIGN_PROJECT: &str = fixture!("restore-foreign-project.json");
const RESUME_AGENT_OK: &str = fixture!("resume-agent-ok.json");
const HEALTHZ: &str = fixture!("healthz.json");
const API_ERROR_NOT_FOUND: &str = fixture!("api-error-not-found.json");

const EVENTS_NORMAL: &str = fixture!("events-normal.sse");
const EVENTS_DUPLICATE: &str = fixture!("events-duplicate.sse");
const EVENTS_GAP: &str = fixture!("events-gap.sse");
const EVENTS_CONFLICTING: &str = fixture!("events-conflicting.sse");
const EVENTS_RESET: &str = fixture!("events-reset.sse");
const EVENTS_RESET_AT_BOUNDARY: &str = fixture!("events-reset-at-boundary.sse");
const EVENTS_CLEAN_RESTART: &str = fixture!("events-clean-restart.sse");
const EVENTS_NOT_AO: &str = fixture!("events-not-ao.sse");

const MUX_FRAMES: &str = fixture!("mux-frames.json");
const CODEX_ARGV: &str = fixture!("codex-argv.json");

/// The AO project every lane in this suite is configured for.
const PROJECT_ID: &str = "prj_kontor_ao_dev";
const PROJECT_PATH: &str = "/w/ao-project";

/// The pinned runs the fixtures carry, so fixtures stay static data instead of
/// templates a test has to interpolate.
const RUN_CLAUDE: &str = "01890000-0000-7000-8000-000000000001";
const RUN_CODEX: &str = "01890000-0000-7000-8000-000000000002";
const RUN_CURSOR: &str = "01890000-0000-7000-8000-000000000003";
const RUN_OPENCODE: &str = "01890000-0000-7000-8000-000000000004";
const RUN_ADOPTABLE: &str = "01890000-0000-7000-8000-000000000005";

fn run(text: &str) -> AgentRunId {
    AgentRunId::parse(text).expect("the fixture pins a canonical AgentRunId")
}

fn view(json: &str) -> AoSessionView {
    serde_json::from_str(json).expect("the fixture is an AO 0.12.1 session view")
}

fn lane(harness: AoHarness) -> AoLane {
    AoLane {
        runtime_kind: RuntimeKindKey::parse(&format!("ao.{harness}")).expect("valid runtime kind"),
        host: ExternalName::parse("ao-loopback").expect("valid host"),
        project_id: PROJECT_ID.to_owned(),
        project_path: WorkspaceRoot::parse(PROJECT_PATH).expect("absolute project path"),
        kind: AoSessionKind::Worker,
        harness,
        max_concurrent_sessions: 8,
    }
}

/// A daemon wired for the happy path of one lane.
fn daemon(spawn: &str, session: &str) -> RecordedAo {
    let native = native_of(session);
    RecordedAo::new()
        .answering(&AoCall::healthz(), HEALTHZ)
        .answering(&AoCall::agents(), AGENTS)
        .answering(&AoCall::project(PROJECT_ID), PROJECT_SAFE)
        .answering(&AoCall::sessions(), INVENTORY)
        .answering(&AoCall::spawn(String::new()), spawn)
        .answering(&AoCall::session(native), session)
        .echoing_follow_up(&AoCall::send(native, String::new()))
        .answering(&AoCall::kill(native), KILL_OK)
        .answering(&AoCall::restore(native), RESTORE_OK)
        .answering(&AoCall::resume_agent(native), RESUME_AGENT_OK)
}

/// A recorded Claude session view, re-addressed to the Codex lane.
///
/// Both fields have to move together. The id is what the route asks for and the
/// harness is what makes the answer this lane's, so a view carrying one lane's id
/// and another's harness is a foreign session by construction.
fn codex_session(json: &str) -> String {
    json.replace("ses_claude_1", "ses_codex_1")
        .replace("\"claude-code\"", "\"codex\"")
}

/// The native id a session or spawn fixture is about.
fn native_of(json: &str) -> &'static str {
    // The four lane sessions are the only ones a fixture spawns or inspects, so
    // this stays a fixed lookup rather than a parse: a test that needs another id
    // is describing a different scenario and says so explicitly.
    if json.contains("\"ses_codex_1\"") {
        "ses_codex_1"
    } else if json.contains("\"ses_cursor_1\"") {
        "ses_cursor_1"
    } else if json.contains("\"ses_opencode_1\"") {
        "ses_opencode_1"
    } else {
        "ses_claude_1"
    }
}

/// One adapter and a second handle on the daemon it talks to.
///
/// The adapter owns its transport, so the ledger has to be reachable through a
/// shared handle rather than a clone: a cloned daemon would keep its own ledger
/// and every "this produced no call" assertion would pass vacuously.
struct Fixture {
    ao: AoAdapter,
    daemon: Arc<RecordedAo>,
}

impl Fixture {
    fn new(harness: AoHarness, daemon: RecordedAo) -> Self {
        let daemon = Arc::new(daemon);
        Self {
            ao: AoAdapter::new(
                lane(harness),
                Box::new(Arc::clone(&daemon)),
                AoCheckpoint::fresh(1),
            ),
            daemon,
        }
    }

    /// Rebuild the adapter from a checkpoint against a fresh daemon, as a Kontor
    /// restart does.
    fn restarted(harness: AoHarness, daemon: RecordedAo, checkpoint: AoCheckpoint) -> Self {
        let daemon = Arc::new(daemon);
        Self {
            ao: AoAdapter::new(lane(harness), Box::new(Arc::clone(&daemon)), checkpoint),
            daemon,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.daemon.calls()
    }

    fn take_calls(&self) -> Vec<String> {
        self.daemon.take_calls()
    }

    fn count(&self, call: &AoCall) -> usize {
        self.daemon.count(call)
    }

    fn mutations(&self) -> Vec<String> {
        self.daemon.mutations()
    }
}

impl std::ops::Deref for Fixture {
    type Target = AoAdapter;

    fn deref(&self) -> &Self::Target {
        &self.ao
    }
}

/// A launch of `agent_run_id` into the lane's own AO project, with no Kontor
/// workspace binding — which is the only shape AO can accept.
fn launch_request(agent_run_id: AgentRunId) -> LaunchRequest {
    LaunchRequest {
        agent_run_id,
        team_run_id: TeamRunId::generate(),
        task_id: TaskId::generate(),
        binding_id: RuntimeBindingId::generate(),
        workspace: None,
        cwd: WorkspaceRoot::parse(PROJECT_PATH).expect("absolute project path"),
        account_profile_id: None,
        prompt: text("do the work"),
        requested_at: at("2026-08-10T09:00:00Z"),
    }
}

/// Every lane, with the fixtures it launches and inspects through.
fn lanes() -> Vec<(AoHarness, &'static str, &'static str, &'static str)> {
    vec![
        (
            AoHarness::ClaudeCode,
            RUN_CLAUDE,
            SPAWN_CLAUDE,
            SESSION_LIVE,
        ),
        (
            AoHarness::Codex,
            RUN_CODEX,
            SPAWN_CODEX,
            fixture!("session-codex-live.json"),
        ),
        (
            AoHarness::Cursor,
            RUN_CURSOR,
            SPAWN_CURSOR,
            fixture!("session-cursor-live.json"),
        ),
        (
            AoHarness::OpenCode,
            RUN_OPENCODE,
            SPAWN_OPENCODE,
            fixture!("session-opencode-live.json"),
        ),
    ]
}

// ---------------------------------------------------------------------------
// The shared contracts, across all four fixture lanes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_fixture_lane_passes_the_shared_adapter_contract() {
    for (harness, pinned, spawn, session) in lanes() {
        let ao = Fixture::new(harness, daemon(spawn, session));
        let request = launch_request(run(pinned));
        let snapshot = adapter_contract(&*ao, &request)
            .await
            .unwrap_or_else(|error| panic!("{harness} lane fails the adapter contract: {error}"));

        // The native id is correlation evidence and never Kontor identity.
        assert_native_id_is_not_a_kontor_id(snapshot.identity().native_id.as_str());
        assert_eq!(snapshot.agent_run_id(), run(pinned));
        assert_eq!(
            snapshot.capabilities.trust_grade,
            TrustGrade::B,
            "AO is Grade B: control and inspect, but no trustworthy replay of content"
        );
        assert!(
            !snapshot.capabilities.account_env,
            "AO has one ambient project environment and cannot prove a per-run account"
        );
    }
}

#[tokio::test]
async fn every_fixture_lane_passes_the_shared_session_content_contract() {
    // The interesting half for AO: the contract runs the *refusal* branch for
    // history and live events, and the positive branch for follow-up.
    for (harness, pinned, spawn, session) in lanes() {
        let ao = Fixture::new(harness, daemon(spawn, session));
        let launched = ao
            .launch(&launch_request(run(pinned)))
            .await
            .expect("the lane launches");
        session_content_contract(&*ao, &launched.snapshot)
            .await
            .unwrap_or_else(|error| {
                panic!("{harness} lane fails the session-content contract: {error}")
            });
    }
}

#[tokio::test]
async fn every_fixture_lane_passes_the_shared_reconciliation_contract() {
    for (harness, pinned, spawn, session) in lanes() {
        let ao = Fixture::new(harness, daemon(spawn, session));
        let launched = ao
            .launch(&launch_request(run(pinned)))
            .await
            .expect("the lane launches");
        reconciliation_contract(&*ao, std::slice::from_ref(&launched.snapshot))
            .await
            .unwrap_or_else(|error| {
                panic!("{harness} lane fails the reconciliation contract: {error}")
            });
    }
}

#[tokio::test]
async fn four_lanes_share_one_inventory_without_stealing_each_others_sessions() {
    // One recorded inventory, four lanes. Each must see exactly its own session,
    // which is what makes a per-harness runtime-kind key meaningful rather than
    // decorative.
    for (harness, _, spawn, session) in lanes() {
        let ao = Fixture::new(harness, daemon(spawn, session));
        let found = ao.discover_sessions().await.expect("discovery succeeds");
        let ids: Vec<&str> = found
            .iter()
            .map(|it| it.identity.native_id.as_str())
            .collect();
        assert!(
            ids.contains(&native_of(session)),
            "{harness} must see its own session, saw {ids:?}"
        );
        for other in ["ses_other_project_1", "ses_aider_1", "ses_orchestrator_1"] {
            assert!(
                !ids.contains(&other),
                "{harness} must not claim {other}; a lane is one project, harness and kind"
            );
        }
        assert!(
            found
                .iter()
                .all(|it| it.identity.runtime_kind.as_str() == format!("ao.{harness}")),
            "every discovered session carries this lane's runtime kind"
        );
    }
}

// ---------------------------------------------------------------------------
// Typed refusals, issued before anything is dispatched
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_unsupported_operation_is_refused_before_the_daemon_is_called() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let binding = launched.snapshot;

    // The ledger is snapshotted here and compared afterwards: every probe below
    // must fail without adding a single call to it.
    let baseline = ao.calls();
    assert!(
        !baseline.is_empty(),
        "the launch really did talk to AO, so an unchanged ledger means something"
    );

    assert_unsupported(
        RuntimeCapability::PrepareWorkspace,
        ao.prepare_workspace(&WorkspacePrepareRequest {
            team_run_id: TeamRunId::generate(),
            task_id: TaskId::generate(),
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: WorkspaceRoot::parse("/w/ao-project/task-1").expect("absolute path"),
            requested_at: at("2026-08-10T08:59:00Z"),
        })
        .await,
    );
    assert_unsupported(
        RuntimeCapability::Adopt,
        ao.adopt(&AdoptRequest {
            agent_run_id: run(RUN_ADOPTABLE),
            binding_id: RuntimeBindingId::generate(),
            native: binding.identity().clone(),
            adopted_at: at("2026-08-10T09:10:00Z"),
        })
        .await,
    );
    assert_unsupported(
        RuntimeCapability::History,
        ao.history(&HistoryRequest {
            binding: binding.clone(),
            cursor: None,
            page_size: 10,
        })
        .await,
    );
    assert_unsupported(
        RuntimeCapability::LiveEvents,
        ao.subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: TimelinePosition::start_of(1),
        })
        .await
        .map(|_| "a subscription"),
    );
    assert_unsupported(
        RuntimeCapability::PermissionResponse,
        ao.respond_permission(&PermissionResponseRequest {
            binding: binding.clone(),
            permission_id: ExternalId::parse("perm-1").expect("valid id"),
            response_id: MessageId::generate(),
            decision: PermissionDecision::Allow,
            responded_at: at("2026-08-10T09:11:00Z"),
        })
        .await,
    );

    assert_eq!(
        ao.calls(),
        baseline,
        "an unsupported operation must reach neither REST, SSE nor the mux"
    );
}

#[test]
fn the_unsupported_table_and_the_declared_capabilities_cannot_disagree() {
    let declared = lane(AoHarness::ClaudeCode).capabilities();
    for (capability, reason) in UNSUPPORTED {
        assert!(
            !declared.supports(*capability),
            "{capability} is listed unsupported but declared supported"
        );
        assert!(
            !reason.is_empty(),
            "{capability} must say why it is refused, so the gap is reported and not hidden"
        );
    }
    // And nothing is silently missing from either side.
    let unsupported: BTreeSet<RuntimeCapability> = UNSUPPORTED.iter().map(|(it, _)| *it).collect();
    for capability in RuntimeCapability::ALL {
        assert_eq!(
            declared.supports(*capability),
            !unsupported.contains(capability),
            "{capability} must be either declared or explained"
        );
    }
}

#[tokio::test]
async fn an_account_pinned_launch_is_refused_before_dispatch() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let mut request = launch_request(run(RUN_CLAUDE));
    request.account_profile_id = Some(kontor_core::id::AccountProfileId::generate());
    let error = ao
        .launch(&request)
        .await
        .expect_err("AO cannot prove a per-run account environment");
    assert_eq!(error, RuntimeError::AccountEnvironmentUnavailable);
    assert!(
        ao.calls().is_empty(),
        "the account refusal must cost no request"
    );
}

#[tokio::test]
async fn a_launch_presenting_a_kontor_workspace_binding_is_refused_before_dispatch() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let mut request = launch_request(run(RUN_CLAUDE));
    let team_run_id = request.team_run_id;
    let task_id = request.task_id;
    let root = WorkspaceRoot::parse("/w/ao-project/task-1").expect("absolute path");
    let identity = NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("ao.claude-code").expect("valid kind"),
        host: ExternalName::parse("ao-loopback").expect("valid host"),
        generation: 1,
        native_id: ExternalId::parse("wks_invented").expect("valid id"),
    };
    request.workspace = Some(WorkspaceBindingSnapshot {
        binding: WorkspaceBinding {
            id: WorkspaceBindingId::generate(),
            team_run_id,
            task_id,
            root: root.clone(),
            identity: identity.clone(),
            bound_at: at("2026-08-10T08:59:00Z"),
        },
        capabilities: lane(AoHarness::ClaudeCode).capabilities(),
        correlation: WorkspaceCorrelationEvidence {
            label: WorkspaceLabel::for_team_run(team_run_id),
            native: identity,
            established_at: at("2026-08-10T08:59:00Z"),
        },
    });
    request.cwd = root;

    let error = ao
        .launch(&request)
        .await
        .expect_err("AO never publishes the worktree path such a binding would claim");
    assert!(matches!(error, RuntimeError::WorkspaceMismatch { .. }));
    assert!(ao.calls().is_empty(), "no session may be created");
}

#[tokio::test]
async fn a_launch_claiming_another_directory_is_refused_before_dispatch() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let mut request = launch_request(run(RUN_CLAUDE));
    request.cwd = WorkspaceRoot::parse("/w/somewhere-else").expect("absolute path");
    let error = ao
        .launch(&request)
        .await
        .expect_err("the cwd is not the lane");
    assert!(matches!(error, RuntimeError::WorkspaceMismatch { .. }));
    assert!(ao.calls().is_empty());
}

#[tokio::test]
async fn a_degraded_or_relocated_project_cannot_authorize_a_launch() {
    for (fixture, why) in [
        (PROJECT_DEGRADED, "a project AO could not resolve"),
        (PROJECT_OTHER_PATH, "a project at another path"),
        (PROJECT_OTHER_ID, "an envelope about another project id"),
    ] {
        let ao = Fixture::new(
            AoHarness::ClaudeCode,
            daemon(SPAWN_CLAUDE, SESSION_LIVE).answering(&AoCall::project(PROJECT_ID), fixture),
        );
        assert!(
            ao.launch(&launch_request(run(RUN_CLAUDE))).await.is_err(),
            "{why} must not authorize a launch"
        );
        assert!(
            ao.mutations().is_empty(),
            "{why} must be refused before any mutating request"
        );
    }
}

// ---------------------------------------------------------------------------
// Unsafe Codex default refusal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unsafe_codex_permission_mode_is_refused_before_any_mutating_call() {
    for (fixture, label) in [
        (PROJECT_UNSAFE_EMPTY, "an empty permission mode"),
        (PROJECT_UNSAFE_DEFAULT, "an explicit `default`"),
        (PROJECT_UNSAFE_UNKNOWN, "a value AO does not recognize"),
        (
            PROJECT_UNSAFE_BYPASS_OVERRIDE,
            "a worker override to `bypass-permissions`",
        ),
    ] {
        let ao = Fixture::new(
            AoHarness::Codex,
            daemon(SPAWN_CODEX, fixture!("session-codex-live.json"))
                .answering(&AoCall::project(PROJECT_ID), fixture),
        );
        let error = ao
            .launch(&launch_request(run(RUN_CODEX)))
            .await
            .err()
            .unwrap_or_else(|| {
                panic!("{label} resolves to an approvals bypass and must be refused")
            });
        assert!(
            matches!(error, RuntimeError::Domain(_)),
            "{label} must fail as a typed policy refusal, got {error:?}"
        );
        assert!(
            ao.mutations().is_empty(),
            "{label} must be refused before any mutating request"
        );
    }
}

#[tokio::test]
async fn codex_launch_restore_and_resume_agent_all_run_the_same_guard() {
    // Guarding spawn alone would leave restore as an unguarded route to the same
    // unsandboxed process — and restore is what recovery reaches for.
    //
    // The recorded Claude views are re-addressed to the Codex lane, harness
    // included: a fresh read is only this lane's session if it names this lane's
    // client, so swapping the id alone would now be refused as a foreign session
    // before the permission guard was ever reached — and this test is about the
    // guard.
    let terminated = codex_session(SESSION_TERMINATED);
    let exited = codex_session(SESSION_EXITED);

    for (session_fixture, route) in [
        (terminated.as_str(), AoCall::restore("ses_codex_1")),
        (exited.as_str(), AoCall::resume_agent("ses_codex_1")),
    ] {
        // First bind under a safe config, then let the project turn unsafe.
        let ao = Fixture::new(
            AoHarness::Codex,
            daemon(SPAWN_CODEX, fixture!("session-codex-live.json"))
                .then_answering(&AoCall::project(PROJECT_ID), PROJECT_SAFE)
                .then_answering(&AoCall::project(PROJECT_ID), PROJECT_SAFE)
                .answering(&AoCall::project(PROJECT_ID), PROJECT_UNSAFE_DEFAULT)
                .then_answering(&AoCall::session("ses_codex_1"), session_fixture),
        );
        let launched = ao
            .launch(&launch_request(run(RUN_CODEX)))
            .await
            .expect("the safe config launches");
        ao.take_calls();

        let error = ao
            .resume(&ResumeRequest {
                binding: launched.snapshot,
                requested_at: at("2026-08-10T09:20:00Z"),
            })
            .await
            .expect_err("an unsafe config must not relaunch a Codex client");
        assert!(matches!(error, RuntimeError::Domain(_)), "{error:?}");
        assert!(
            !ao.calls().contains(&route.route()),
            "{} must not be reached under an unsafe permission mode",
            route.route()
        );
    }
}

#[tokio::test]
async fn a_project_envelope_about_another_project_cannot_clear_the_codex_guard() {
    // Why the project id is checked and not only the path. This envelope is the
    // dangerous shape: it reports *this* lane's path — so the relocation check
    // passes — under another project's id, and it carries an approval-gated config.
    // The permission mode read out of it therefore belongs to someone else's
    // project, while the spawn that would follow names this lane's project, whose
    // real mode may be an approvals-and-sandbox bypass. A safe-looking config is
    // only evidence about the project it describes.
    let ao = Fixture::new(
        AoHarness::Codex,
        daemon(SPAWN_CODEX, fixture!("session-codex-live.json"))
            .answering(&AoCall::project(PROJECT_ID), PROJECT_OTHER_ID),
    );
    assert!(
        PROJECT_OTHER_ID.contains("accept-edits") && PROJECT_OTHER_ID.contains(PROJECT_PATH),
        "the fixture only tests anything if it would otherwise pass both the guard \
         and the path check"
    );
    let error = ao
        .launch(&launch_request(run(RUN_CODEX)))
        .await
        .expect_err("an envelope about another project authorizes nothing");
    assert!(
        matches!(error, RuntimeError::WorkspaceMismatch { .. }),
        "the launch must fail as the wrong project, not as a permission verdict \
         derived from it, got {error:?}"
    );
    assert!(
        ao.mutations().is_empty(),
        "the refusal must precede the spawn"
    );
}

#[tokio::test]
async fn an_approval_gated_codex_mode_proceeds() {
    for fixture in [PROJECT_SAFE, fixture!("project-auto.json")] {
        let ao = Fixture::new(
            AoHarness::Codex,
            daemon(SPAWN_CODEX, fixture!("session-codex-live.json"))
                .answering(&AoCall::project(PROJECT_ID), fixture),
        );
        ao.launch(&launch_request(run(RUN_CODEX)))
            .await
            .expect("accept-edits and auto both keep an approval gate");
    }
}

#[test]
fn the_recorded_argv_proves_which_modes_bypass_approvals() {
    // The policy and the argv AO would actually build are asserted against each
    // other, so the refusal cannot drift away from its reason.
    let recorded: serde_json::Value =
        serde_json::from_str(CODEX_ARGV).expect("the argv fixture parses");
    let modes = recorded["modes"].as_array().expect("modes is an array");
    assert!(!modes.is_empty());
    for entry in modes {
        let permissions = entry["permissions"].as_str().expect("a permissions value");
        let verdict = entry["kontor_verdict"].as_str().expect("a verdict");
        let argv: Vec<&str> = entry["approval_argv"]
            .as_array()
            .expect("an argv array")
            .iter()
            .map(|it| it.as_str().expect("argv entries are strings"))
            .collect();

        let mode = AoPermissionMode::normalize(permissions);
        assert_eq!(
            mode.codex_approval_argv(),
            argv.as_slice(),
            "the adapter's argv model disagrees with the recorded AO argv for {permissions:?}"
        );
        let bypasses = argv.contains(&"--dangerously-bypass-approvals-and-sandbox");
        match verdict {
            "refused" => {
                assert!(
                    bypasses,
                    "{permissions:?} is refused, so it must be the bypass"
                );
                assert!(!mode.is_approval_gated());
            }
            "allowed" => {
                assert!(
                    !bypasses,
                    "{permissions:?} is allowed, so its argv must not bypass approvals"
                );
                assert!(argv.contains(&"--ask-for-approval"));
                assert!(mode.is_approval_gated());
            }
            other => panic!("unknown verdict {other}"),
        }
    }

    // The residual AO always injects is recorded, and is deliberately not treated
    // as something an allowed mode removes.
    let unconditional = recorded["unconditional_flags"]
        .as_array()
        .expect("the unconditional flags are recorded");
    assert!(
        unconditional
            .iter()
            .any(|it| it == "--dangerously-bypass-hook-trust"),
        "AO's unconditional hook-trust bypass must stay recorded as a residual fact"
    );
    assert!(
        MANIFEST.contains("codex_residual_hook_trust_bypass"),
        "the manifest must carry the residual trust fact in provenance"
    );
}

// ---------------------------------------------------------------------------
// Lifecycle normalization: uncertainty is never completion
// ---------------------------------------------------------------------------

#[test]
fn ao_can_never_report_a_success_or_a_failure() {
    // The mutant this kills is the tempting one: mapping `idle` or `exited` onto
    // Succeeded. Every reachable combination is walked, so no corner is left for
    // a verdict to hide in.
    let statuses = [
        AoSessionStatus::Working,
        AoSessionStatus::NeedsInput,
        AoSessionStatus::Exited,
        AoSessionStatus::Idle,
        AoSessionStatus::NoSignal,
        AoSessionStatus::Terminated,
        AoSessionStatus::PrOpen,
        AoSessionStatus::Draft,
        AoSessionStatus::CiFailed,
        AoSessionStatus::ReviewPending,
        AoSessionStatus::ChangesRequested,
        AoSessionStatus::Approved,
        AoSessionStatus::Mergeable,
        AoSessionStatus::Merged,
    ];
    let activities = [
        AoActivityState::Active,
        AoActivityState::Idle,
        AoActivityState::WaitingInput,
        AoActivityState::Blocked,
        AoActivityState::Exited,
    ];
    let mut base = view(SESSION_LIVE);
    let mut walked = 0;
    for activity in activities {
        for status in statuses {
            for terminated in [false, true] {
                base.activity.state = activity;
                base.status = status;
                base.is_terminated = terminated;
                let lifecycle = normalize_lifecycle(&base);
                assert!(
                    !matches!(
                        lifecycle.state,
                        ObservedRunState::Succeeded | ObservedRunState::Failed
                    ),
                    "AO has no trustworthy verdict, yet {activity:?}/{status:?}/\
                     terminated={terminated} produced {:?}",
                    lifecycle.state
                );
                if !terminated {
                    assert_ne!(
                        lifecycle.state,
                        ObservedRunState::Cancelled,
                        "only an explicit isTerminated may read as cancelled"
                    );
                }
                walked += 1;
            }
        }
    }
    assert_eq!(walked, 5 * 14 * 2, "every combination is walked");
}

#[test]
fn each_recorded_state_maps_exactly_as_the_plan_says() {
    let cases = [
        (
            SESSION_LIVE,
            ObservedRunState::Running,
            RuntimeContact::Reachable,
            AoAttention::None,
        ),
        (
            SESSION_WAITING,
            ObservedRunState::WaitingInput,
            RuntimeContact::Reachable,
            AoAttention::AwaitingInstruction,
        ),
        (
            SESSION_BLOCKED,
            ObservedRunState::Blocked,
            RuntimeContact::Reachable,
            AoAttention::AwaitingDecision,
        ),
        (
            SESSION_IDLE,
            ObservedRunState::Running,
            RuntimeContact::Reachable,
            AoAttention::Idle,
        ),
        (
            SESSION_NEEDS_INPUT,
            ObservedRunState::WaitingInput,
            RuntimeContact::Reachable,
            AoAttention::NeedsInputUnclassified,
        ),
        (
            SESSION_NO_SIGNAL,
            ObservedRunState::Unknown,
            RuntimeContact::Reachable,
            AoAttention::Unknown,
        ),
        (
            SESSION_EXITED,
            ObservedRunState::Unknown,
            RuntimeContact::ProcessMissing,
            AoAttention::Unknown,
        ),
        (
            SESSION_TERMINATED,
            ObservedRunState::Cancelled,
            RuntimeContact::Reachable,
            AoAttention::None,
        ),
        // Product workflow leaves the run state alone.
        (
            SESSION_MERGED,
            ObservedRunState::Running,
            RuntimeContact::Reachable,
            AoAttention::Idle,
        ),
        (
            SESSION_APPROVED,
            ObservedRunState::Running,
            RuntimeContact::Reachable,
            AoAttention::None,
        ),
    ];
    for (fixture, state, contact, attention) in cases {
        let lifecycle = normalize_lifecycle(&view(fixture));
        assert_eq!(lifecycle.state, state, "run state for {fixture}");
        assert_eq!(lifecycle.contact, contact, "contact for {fixture}");
        assert_eq!(lifecycle.attention, attention, "attention for {fixture}");
    }
}

#[test]
fn a_blocked_session_never_accepts_an_automated_message() {
    assert!(!AoAttention::AwaitingDecision.accepts_automated_message());
    for attention in [
        AoAttention::None,
        AoAttention::Idle,
        AoAttention::AwaitingInstruction,
        AoAttention::NeedsInputUnclassified,
        AoAttention::Unknown,
    ] {
        assert!(attention.accepts_automated_message());
    }
    // And `needs_input` with only a weak raw state behind it is never upgraded
    // into a decision, because AO cannot say which of the two it is.
    assert_eq!(
        normalize_lifecycle(&view(SESSION_NEEDS_INPUT)).attention,
        AoAttention::NeedsInputUnclassified
    );
}

#[tokio::test]
async fn no_uncertain_observation_can_close_a_run() {
    for fixture in [
        SESSION_IDLE,
        SESSION_NO_SIGNAL,
        SESSION_EXITED,
        SESSION_MERGED,
        SESSION_APPROVED,
        SESSION_BLOCKED,
        SESSION_WAITING,
    ] {
        let ao = Fixture::new(
            AoHarness::ClaudeCode,
            daemon(SPAWN_CLAUDE, SESSION_LIVE)
                .then_answering(&AoCall::session("ses_claude_1"), fixture),
        );
        let launched = ao
            .launch(&launch_request(run(RUN_CLAUDE)))
            .await
            .expect("the lane launches");
        let observed = ao
            .inspect(&InspectRequest {
                binding: launched.snapshot.clone(),
                requested_at: at("2026-08-10T09:30:00Z"),
            })
            .await
            .expect("a fresh inspect succeeds");
        assert_eq!(
            closes(&*ao, &observed, &launched.snapshot).await,
            None,
            "{fixture} must not close a run"
        );
    }
}

#[tokio::test]
async fn only_a_fresh_inspect_of_an_explicit_termination_confirms_cancellation() {
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering(&AoCall::session("ses_claude_1"), SESSION_TERMINATED),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");

    // The kill acknowledgement is an acknowledgement.
    let acknowledged = ao
        .cancel(&CancelRequest {
            binding: launched.snapshot.clone(),
            requested_at: at("2026-08-10T09:31:00Z"),
        })
        .await
        .expect("AO accepts the request");
    assert_eq!(acknowledged.source, ObservationSource::CommandAck);
    assert_eq!(
        closes(&*ao, &acknowledged, &launched.snapshot).await,
        None,
        "a kill acknowledgement is not evidence the session stopped"
    );

    // The fresh inspect is the evidence.
    let confirmed = ao
        .inspect(&InspectRequest {
            binding: launched.snapshot.clone(),
            requested_at: at("2026-08-10T09:32:00Z"),
        })
        .await
        .expect("a fresh inspect succeeds");
    assert_eq!(confirmed.source, ObservationSource::Inspect);
    assert_eq!(
        closes(&*ao, &confirmed, &launched.snapshot).await,
        Some(TerminalOutcome::Cancelled),
        "Grade B closes on a fresh inspect of an explicit termination"
    );
}

#[tokio::test]
async fn a_forged_or_foreign_binding_closes_nothing_through_the_registry() {
    // The provenance fence, checked end to end rather than at the constructor:
    // terminal evidence is judged only against a snapshot this adapter's own
    // registry hands back, so a caller's edited copy cannot close a run even
    // though every field of it is public and self-consistent.
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering(&AoCall::session("ses_claude_1"), SESSION_TERMINATED),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let genuine = launched.snapshot;
    let terminated = ao
        .inspect(&InspectRequest {
            binding: genuine.clone(),
            requested_at: at("2026-08-10T09:32:00Z"),
        })
        .await
        .expect("a fresh inspect succeeds");

    // The genuine binding closes, which is what makes every refusal below a real
    // difference rather than a vacuous None.
    assert_eq!(
        closes(&*ao, &terminated, &genuine).await,
        Some(TerminalOutcome::Cancelled)
    );

    // A clone with the trust grade promoted. Grade is the field that decides
    // closure, and a snapshot is a plain value, so this is the cheapest forgery
    // available to any call site.
    let mut promoted = genuine.clone();
    promoted.capabilities.trust_grade = TrustGrade::A;
    assert!(
        ao.issued_binding(&promoted).await.is_err(),
        "a promoted clone is not the binding the runtime issued"
    );
    assert_eq!(
        closes(&*ao, &terminated, &promoted).await,
        None,
        "an edited snapshot must close nothing"
    );

    // A binding this adapter never issued at all.
    let foreign = bound_snapshot(&ao, run(RUN_ADOPTABLE), "ses_adoptable_1", RUN_ADOPTABLE);
    assert!(ao.issued_binding(&foreign).await.is_err());
    assert_eq!(closes(&*ao, &terminated, &foreign).await, None);

    // And what the runtime vouches for is its own copy, at the grade it really
    // issued — never the grade a caller presented.
    let vouched = ao
        .issued_binding(&genuine)
        .await
        .expect("the runtime vouches for its own binding");
    assert_eq!(vouched.snapshot().capabilities.trust_grade, TrustGrade::B);
}

#[tokio::test]
async fn a_forged_binding_is_refused_before_any_effect_on_every_bound_operation() {
    // The provenance fence on the operations that *drive* a session, not only on
    // the one that judges terminal evidence. `preflight` cannot close this: it
    // checks a snapshot against itself, and a self-consistent forgery is free to
    // build. Only the registry knows what this runtime issued.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let genuine = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches")
        .snapshot;

    // Two forgeries. The first names a session AO really owns but that this
    // adapter never bound; the second is the real binding with its evidence
    // quality talked up. Both are internally consistent, which is exactly the
    // point.
    let mut promoted = genuine.clone();
    promoted.capabilities.trust_grade = TrustGrade::A;
    promoted.capabilities.limits.max_message_bytes = u64::MAX;
    let forgeries = [
        (
            "a session this adapter never bound",
            bound_snapshot(&ao, run(RUN_ADOPTABLE), "ses_adoptable_1", RUN_ADOPTABLE),
        ),
        ("the real binding, promoted", promoted),
    ];

    for (label, forged) in forgeries {
        for operation in ["send", "resume", "cancel", "inspect"] {
            ao.take_calls();
            let refused = match operation {
                "send" => ao
                    .send(&SendMessageRequest {
                        binding: forged.clone(),
                        message_id: MessageId::generate(),
                        body: text("do something on my behalf"),
                        sent_at: at("2026-08-10T09:40:00Z"),
                    })
                    .await
                    .err(),
                "resume" => ao
                    .resume(&ResumeRequest {
                        binding: forged.clone(),
                        requested_at: at("2026-08-10T09:41:00Z"),
                    })
                    .await
                    .err(),
                "cancel" => ao
                    .cancel(&CancelRequest {
                        binding: forged.clone(),
                        requested_at: at("2026-08-10T09:42:00Z"),
                    })
                    .await
                    .err(),
                _ => ao
                    .inspect(&InspectRequest {
                        binding: forged.clone(),
                        requested_at: at("2026-08-10T09:43:00Z"),
                    })
                    .await
                    .err(),
            };
            let error =
                refused.unwrap_or_else(|| panic!("{operation} accepted {label} and acted on it"));
            assert!(
                matches!(error, RuntimeError::StaleBinding { .. }),
                "{operation} must refuse {label} as an unissued binding, got {error:?}"
            );
            assert!(
                ao.calls().is_empty(),
                "{operation} reached AO with {label}: the refusal must precede every effect"
            );
        }
    }

    // Nothing was delivered, and the genuine binding still works — so the
    // refusals above are a real difference rather than a broken adapter.
    assert!(
        ao.checkpoint().deliveries.is_empty(),
        "a forged binding must leave no trace in the message ledger"
    );
    ao.inspect(&InspectRequest {
        binding: genuine.clone(),
        requested_at: at("2026-08-10T09:44:00Z"),
    })
    .await
    .expect("the binding the runtime issued still works");
    ao.send(&SendMessageRequest {
        binding: genuine,
        message_id: MessageId::generate(),
        body: text("please continue"),
        sent_at: at("2026-08-10T09:45:00Z"),
    })
    .await
    .expect("and can still be sent into");
}

#[tokio::test]
async fn a_forged_binding_cannot_grant_itself_a_larger_message_limit() {
    // A limit is a value on the snapshot, and `preflight` reads it from whatever
    // snapshot it is handed. Without attestation a forgery that raises its own
    // `max_message_bytes` buys headroom AO does not have: the limit check passes on
    // the caller's own number and the oversized body goes out on the wire.
    //
    // The *ordering* of attestation and preflight is not what closes this — a
    // mutation that swaps them survives, because preflight produces no effect.
    // Attestation happening at all is.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let genuine = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches")
        .snapshot;
    assert_eq!(genuine.capabilities.limits.max_message_bytes, 4096);

    let mut inflated = genuine.clone();
    inflated.capabilities.limits.max_message_bytes = u64::MAX;
    ao.take_calls();

    let error = ao
        .send(&SendMessageRequest {
            binding: inflated,
            message_id: MessageId::generate(),
            body: BoundedText::parse(&"x".repeat(5000)).expect("bounded text"),
            sent_at: at("2026-08-10T09:46:00Z"),
        })
        .await
        .expect_err("a binding cannot raise its own ceiling");
    assert!(
        matches!(error, RuntimeError::StaleBinding { .. }),
        "the forgery must be refused on provenance, before the limit is even \
         consulted, got {error:?}"
    );
    assert!(ao.calls().is_empty());

    // The genuine binding still refuses the same body, on the real limit.
    assert_eq!(
        ao.send(&SendMessageRequest {
            binding: genuine,
            message_id: MessageId::generate(),
            body: BoundedText::parse(&"x".repeat(5000)).expect("bounded text"),
            sent_at: at("2026-08-10T09:47:00Z"),
        })
        .await
        .expect_err("AO caps a follow-up at 4096 bytes"),
        RuntimeError::LimitExceeded {
            subject: "message body",
            limit: 4096
        }
    );
}

#[tokio::test]
async fn a_launch_response_from_another_lane_is_refused_and_binds_nothing() {
    // AO echoes back whatever branch it was asked for, so a correct correlation
    // label proves nothing about *where* the session was created. A response naming
    // another project would point this run at a session in a repository Kontor
    // never verified; another harness would mean a client whose safety guard was
    // never run; another kind would mean an orchestrator standing in for a worker.
    for (fixture, label) in [
        (SPAWN_FOREIGN_PROJECT, "another project"),
        (SPAWN_WRONG_HARNESS, "another harness"),
        (SPAWN_WRONG_KIND, "another session kind"),
        (SPAWN_NO_HARNESS, "no harness at all"),
    ] {
        let ao = Fixture::new(
            AoHarness::ClaudeCode,
            daemon(SPAWN_CLAUDE, SESSION_LIVE).answering(&AoCall::spawn(String::new()), fixture),
        );
        let error = ao
            .launch(&launch_request(run(RUN_CLAUDE)))
            .await
            .err()
            .unwrap_or_else(|| panic!("a spawn naming {label} must not produce a LaunchOutcome"));
        assert_eq!(
            error,
            RuntimeError::CorrelationFailed,
            "a spawn naming {label} must fail as unproven correlation"
        );
        assert!(
            ao.checkpoint().bindings.is_empty(),
            "a spawn naming {label} must not be bound"
        );
    }

    // The unmutated response still binds, so the refusals above are the check
    // working rather than the launch path being broken.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    ao.launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("this lane's own response binds");
    assert_eq!(ao.checkpoint().bindings.len(), 1);
}

#[tokio::test]
async fn a_restore_or_resume_response_from_another_lane_is_refused() {
    // The same hole on the recovery path: `restore` and `resume-agent` each carry a
    // full session view back, and a foreign one must not become this run's
    // observation.
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering(&AoCall::session("ses_claude_1"), SESSION_TERMINATED)
            .answering(&AoCall::restore("ses_claude_1"), RESTORE_FOREIGN_PROJECT),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    assert_eq!(
        ao.resume(&ResumeRequest {
            binding: launched.snapshot,
            requested_at: at("2026-08-10T09:50:00Z"),
        })
        .await
        .expect_err("a restore naming another project is not this run's session"),
        RuntimeError::CorrelationFailed
    );
}

#[tokio::test]
async fn a_live_session_from_another_lane_is_refused_on_every_route_that_reads_one() {
    // The third shape of the same hole, and the quietest: the session read comes
    // back *live*, so resume relaunches nothing and returns it as the answer. No
    // client is started and no mutation happens, which is exactly why an unchecked
    // one would go unnoticed — the run would simply carry an observation about
    // another project's session as its own running state.
    //
    // Both routes that read a single session are covered, because the check
    // belongs to the read rather than to either caller.
    for route in ["resume", "inspect"] {
        let ao = Fixture::new(
            AoHarness::ClaudeCode,
            daemon(SPAWN_CLAUDE, SESSION_LIVE).then_answering(
                &AoCall::session("ses_claude_1"),
                SESSION_LIVE_FOREIGN_PROJECT,
            ),
        );
        let launched = ao
            .launch(&launch_request(run(RUN_CLAUDE)))
            .await
            .expect("the lane launches");
        ao.take_calls();

        let refused = match route {
            "resume" => {
                ao.resume(&ResumeRequest {
                    binding: launched.snapshot.clone(),
                    requested_at: at("2026-08-10T09:52:00Z"),
                })
                .await
            }
            _ => {
                ao.inspect(&InspectRequest {
                    binding: launched.snapshot.clone(),
                    requested_at: at("2026-08-10T09:52:00Z"),
                })
                .await
            }
        };
        assert_eq!(
            refused.expect_err("a live foreign session is not this run's answer"),
            RuntimeError::CorrelationFailed,
            "{route} must refuse a session view naming another project"
        );
        assert!(
            ao.mutations().is_empty(),
            "{route} must not have started or driven anything"
        );
    }

    // The same route on this lane's own live session still answers, so the
    // refusals above are the membership check working rather than resume and
    // inspect being broken.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    ao.take_calls();
    let observed = ao
        .resume(&ResumeRequest {
            binding: launched.snapshot,
            requested_at: at("2026-08-10T09:53:00Z"),
        })
        .await
        .expect("a live session in this lane is reported as it is");
    assert_eq!(observed.source, ObservationSource::Inspect);
    assert!(
        ao.mutations().is_empty(),
        "a live session is never relaunched"
    );
}

#[tokio::test]
async fn a_forged_binding_is_never_matched_by_reconciliation() {
    // Reconciliation is the one operation whose *output* is the authority: a
    // `Matched` finding carries the action `Keep`. A snapshot is a plain public
    // value, so without attestation a fabricated one naming a session AO really
    // has would come back endorsed as the binding to keep — and it would be
    // endorsed by the very sweep that exists to catch it.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let genuine = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches")
        .snapshot;

    // Three forgeries, all naming sessions the inventory really contains, so
    // nothing here fails for want of a session to match.
    let never_issued = bound_snapshot(&ao, run(RUN_ADOPTABLE), "ses_adoptable_1", RUN_ADOPTABLE);
    let mut promoted = genuine.clone();
    promoted.capabilities.trust_grade = TrustGrade::A;
    let mut same_id_edited = genuine.clone();
    same_id_edited.capabilities.limits.max_message_bytes = u64::MAX;

    for (label, forged) in [
        ("a binding this adapter never issued", &never_issued),
        ("the real binding with its grade promoted", &promoted),
        ("the real binding id with edited limits", &same_id_edited),
    ] {
        let report = ao
            .reconcile(std::slice::from_ref(forged))
            .await
            .expect("reconciliation reads and classifies; it never refuses wholesale");
        let matched = report
            .findings
            .iter()
            .any(|finding| matches!(finding, ReconciliationFinding::Matched { .. }));
        assert!(!matched, "{label} must never be reported as Matched");

        // Reported, not dropped: a binding no finding mentions is a binding
        // nothing ever reviews.
        let reported: Vec<&ReconciliationFinding> = report
            .findings
            .iter()
            .filter(|finding| match finding {
                ReconciliationFinding::Unattested { binding_id, .. } => {
                    *binding_id == forged.binding_id()
                }
                _ => false,
            })
            .collect();
        assert_eq!(
            reported.len(),
            1,
            "{label} must be reported as unattested, once"
        );
        // Asserted on the finding itself rather than through
        // `ReconciliationReport::needs_review`, which the inventory's own orphans
        // would satisfy on their own and so cannot say anything about this one.
        assert_eq!(
            reported[0].action(),
            ReconciliationAction::ProposeOrphanReview,
            "{label} must be sent to review rather than kept"
        );
        assert!(
            report.findings.iter().all(|finding| !matches!(
                finding.proposed_state(),
                Some(DerivedRunState::Terminal { .. })
            )),
            "{label} must not conclude anything about the work"
        );
    }

    // The binding the runtime really issued still matches, on its own values.
    let report = ao
        .reconcile(std::slice::from_ref(&genuine))
        .await
        .expect("reconciliation succeeds");
    assert!(
        report.findings.iter().any(|finding| matches!(
            finding,
            ReconciliationFinding::Matched { binding_id, .. } if *binding_id == genuine.binding_id()
        )),
        "the issued binding is kept, so the refusals above are a real difference"
    );
}

// ---------------------------------------------------------------------------
// Correlation, lost acknowledgements and the ledger
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_lost_launch_acknowledgement_recovers_by_correlation_without_a_second_post() {
    let spawn = AoCall::spawn(String::new());
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE).losing_acknowledgement(&spawn),
    );
    let request = launch_request(run(RUN_CLAUDE));
    let launched = ao
        .launch(&request)
        .await
        .expect("the session AO already created is found by its correlation branch");

    assert_eq!(
        launched.snapshot.identity().native_id.as_str(),
        "ses_claude_1"
    );
    assert_eq!(launched.snapshot.agent_run_id(), run(RUN_CLAUDE));
    assert_eq!(
        ao.count(&spawn),
        1,
        "recovery must search, never repeat the spawn"
    );
}

#[tokio::test]
async fn a_lost_launch_with_no_matching_branch_stays_unknown_rather_than_relaunching() {
    let spawn = AoCall::spawn(String::new());
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .losing_acknowledgement(&spawn)
            .answering(&AoCall::sessions(), INVENTORY_WITHOUT_CLAUDE),
    );
    let error = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect_err("nothing proves AO created a session");
    assert!(matches!(error, RuntimeError::Transport { .. }));
    assert_eq!(
        ao.count(&spawn),
        1,
        "a blind relaunch is how one run acquires two agents"
    );
}

#[tokio::test]
async fn two_sessions_on_one_correlation_branch_bind_nothing() {
    let spawn = AoCall::spawn(String::new());
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .losing_acknowledgement(&spawn)
            .answering(&AoCall::sessions(), INVENTORY_DIVERGED),
    );
    assert_eq!(
        ao.launch(&launch_request(run(RUN_CLAUDE)))
            .await
            .expect_err("a diverged lane binds nothing"),
        RuntimeError::CorrelationFailed
    );
    assert_eq!(ao.count(&spawn), 1);
}

#[tokio::test]
async fn a_branch_that_is_not_this_runs_label_is_refused() {
    for (fixture, why) in [
        (SPAWN_WRONG_CORRELATION, "another run's label"),
        (SPAWN_NATIVE_ID_AS_BRANCH, "the native session id"),
        (SPAWN_NO_BRANCH, "no label at all"),
    ] {
        let ao = Fixture::new(
            AoHarness::ClaudeCode,
            daemon(fixture, SESSION_LIVE).answering(&AoCall::spawn(String::new()), fixture),
        );
        assert_eq!(
            ao.launch(&launch_request(run(RUN_CLAUDE)))
                .await
                .expect_err("a branch reporting {why} is not correlation evidence"),
            RuntimeError::CorrelationFailed,
            "{why} must not bind"
        );
    }
}

#[tokio::test]
async fn a_retried_follow_up_replays_its_acknowledgement_and_posts_once() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let send = SendMessageRequest {
        binding: launched.snapshot.clone(),
        message_id: MessageId::generate(),
        body: text("please continue"),
        sent_at: at("2026-08-10T09:40:00Z"),
    };
    let first = ao.send(&send).await.expect("AO accepts the message");
    let replay = ao.send(&send).await.expect("the retry replays");
    assert_eq!(first, replay);
    assert_eq!(
        ao.count(&AoCall::send("ses_claude_1", String::new())),
        1,
        "a retry must be answered from the ledger, not delivered again"
    );

    // The same identifier with different content is a caller bug, not a retry.
    let contradicting = SendMessageRequest {
        body: text("do something else"),
        ..send.clone()
    };
    assert!(matches!(
        ao.send(&contradicting)
            .await
            .expect_err("one identifier cannot mean two messages"),
        RuntimeError::DuplicateMessage { .. }
    ));
    assert_eq!(ao.count(&AoCall::send("ses_claude_1", String::new())), 1);
}

#[tokio::test]
async fn a_lost_follow_up_acknowledgement_is_never_sent_again() {
    let route = AoCall::send("ses_claude_1", String::new());
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE).losing_acknowledgement(&route),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let send = SendMessageRequest {
        binding: launched.snapshot.clone(),
        message_id: MessageId::generate(),
        body: text("please continue"),
        sent_at: at("2026-08-10T09:40:00Z"),
    };
    assert!(matches!(
        ao.send(&send)
            .await
            .expect_err("the acknowledgement was lost"),
        RuntimeError::Transport { .. }
    ));
    assert_eq!(ao.count(&route), 1);

    // The retry is answered from the ledger with a typed unknown, and posts
    // nothing: a duplicated instruction is an action taken twice in someone's
    // repository, while a missing one is a stall an operator can see.
    assert!(matches!(
        ao.send(&send)
            .await
            .expect_err("delivery is unconfirmed and stays that way"),
        RuntimeError::Transport { .. }
    ));
    assert_eq!(
        ao.count(&route),
        1,
        "a confirmation-unknown message must never be POSTed again"
    );
    assert!(matches!(
        ao.checkpoint().deliveries.as_slice(),
        [(_, _, AoDelivery::ConfirmationUnknown)]
    ));
}

#[tokio::test]
async fn an_acknowledgement_for_another_message_is_not_this_messages_receipt() {
    for fixture in [SEND_OTHER_SESSION, fixture!("send-other-message.json")] {
        let ao = Fixture::new(
            AoHarness::ClaudeCode,
            daemon(SPAWN_CLAUDE, SESSION_LIVE)
                .answering(&AoCall::send("ses_claude_1", String::new()), fixture),
        );
        let launched = ao
            .launch(&launch_request(run(RUN_CLAUDE)))
            .await
            .expect("the lane launches");
        assert!(
            ao.send(&SendMessageRequest {
                binding: launched.snapshot,
                message_id: MessageId::generate(),
                body: text("please continue"),
                sent_at: at("2026-08-10T09:40:00Z"),
            })
            .await
            .is_err(),
            "AO must echo this exact session and body back"
        );
    }
}

#[tokio::test]
async fn a_refused_follow_up_leaves_the_identifier_usable() {
    let route = AoCall::send("ses_claude_1", String::new());
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering_with_status(&route, 400, API_ERROR_NOT_FOUND)
            .answering(&route, SEND_OK),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let send = SendMessageRequest {
        binding: launched.snapshot,
        message_id: MessageId::generate(),
        body: text("please continue"),
        sent_at: at("2026-08-10T09:40:00Z"),
    };
    assert!(ao.send(&send).await.is_err(), "AO answered no");
    // A 4xx is AO saying it did not accept the message, so the identifier is not
    // burned. Treating every failure as confirmation-unknown would strand a
    // follow-up AO explicitly rejected.
    ao.send(&send)
        .await
        .expect("a refused message may be sent again");
}

#[tokio::test]
async fn an_oversized_follow_up_is_refused_before_dispatch() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    ao.take_calls();
    let error = ao
        .send(&SendMessageRequest {
            binding: launched.snapshot,
            message_id: MessageId::generate(),
            body: BoundedText::parse(&"x".repeat(4097)).expect("bounded text"),
            sent_at: at("2026-08-10T09:40:00Z"),
        })
        .await
        .expect_err("AO caps a follow-up at 4096 bytes");
    assert_eq!(
        error,
        RuntimeError::LimitExceeded {
            subject: "message body",
            limit: 4096
        }
    );
    assert!(ao.calls().is_empty());
}

#[tokio::test]
async fn a_send_not_ok_is_refused_without_burning_the_identifier() {
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering(&AoCall::send("ses_claude_1", String::new()), SEND_NOT_OK)
            .answering(&AoCall::send("ses_claude_1", String::new()), SEND_OK),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let send = SendMessageRequest {
        binding: launched.snapshot,
        message_id: MessageId::generate(),
        body: text("please continue"),
        sent_at: at("2026-08-10T09:40:00Z"),
    };
    assert!(ao.send(&send).await.is_err(), "ok=false is a refusal");
    ao.send(&send).await.expect("and may be retried");
}

// ---------------------------------------------------------------------------
// Resume does the least the session needs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resume_restores_only_a_terminated_session_and_never_relaunches_a_live_one() {
    let restore = AoCall::restore("ses_claude_1");
    let resume_agent = AoCall::resume_agent("ses_claude_1");

    // A live session: inspect only.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    let observed = ao
        .resume(&ResumeRequest {
            binding: launched.snapshot.clone(),
            requested_at: at("2026-08-10T09:50:00Z"),
        })
        .await
        .expect("resume succeeds");
    assert_eq!(observed.source, ObservationSource::Inspect);
    assert_eq!(ao.count(&restore), 0);
    assert_eq!(
        ao.count(&resume_agent),
        0,
        "relaunching a working client would discard live work"
    );

    // A terminated session: restore.
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering(&AoCall::session("ses_claude_1"), SESSION_TERMINATED),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    ao.resume(&ResumeRequest {
        binding: launched.snapshot.clone(),
        requested_at: at("2026-08-10T09:51:00Z"),
    })
    .await
    .expect("a terminated session is restored");
    assert_eq!(ao.count(&restore), 1);
    assert_eq!(ao.count(&resume_agent), 0);

    // An exited client inside a live session: resume-agent.
    let ao = Fixture::new(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE)
            .then_answering(&AoCall::session("ses_claude_1"), SESSION_EXITED),
    );
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    ao.resume(&ResumeRequest {
        binding: launched.snapshot,
        requested_at: at("2026-08-10T09:52:00Z"),
    })
    .await
    .expect("an exited client is resumed");
    assert_eq!(ao.count(&resume_agent), 1);
    assert_eq!(ao.count(&restore), 0);
}

// ---------------------------------------------------------------------------
// Global replay continuity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_ao_change_is_validated_before_any_is_filtered_by_session() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let accepted = ao
        .observe_events(EVENTS_NORMAL)
        .expect("a contiguous recording is accepted");
    assert_eq!(
        accepted.iter().map(|it| it.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "seq 2 belongs to another session and must still be validated, or it \
         would read as this session's gap"
    );
    // The pull-request change is carried but is not lifecycle truth.
    assert!(
        accepted
            .iter()
            .any(|it| !it.event_type.is_session_lifecycle())
    );
}

#[tokio::test]
async fn an_exact_replay_is_dropped_and_a_contradiction_or_gap_is_reported() {
    let fresh = || Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));

    // A reconnect redelivers 1-4 and adds 5. Only 5 is new.
    let ao = fresh();
    ao.observe_events(EVENTS_NORMAL).expect("the first pass");
    let second = ao
        .observe_events(EVENTS_DUPLICATE)
        .expect("an exact replay is benign");
    assert_eq!(
        second.iter().map(|it| it.seq).collect::<Vec<_>>(),
        vec![5],
        "a redelivered change must not be persisted twice"
    );

    let ao = fresh();
    assert_eq!(
        ao.observe_events(EVENTS_GAP)
            .expect_err("seq 3 never arrived"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );

    let ao = fresh();
    assert_eq!(
        ao.observe_events(EVENTS_CONFLICTING)
            .expect_err("the same sequence carried different content"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::ConflictingDuplicate
        }
    );

    let ao = fresh();
    assert!(
        ao.observe_events(EVENTS_NOT_AO).is_err(),
        "a recording that is not AO 0.12.1 fails typed rather than being skipped"
    );

    // A sequence the live cursor itself accepted, redelivered with other content,
    // is AO contradicting itself rather than a new log.
    let ao = fresh();
    ao.observe_events(EVENTS_NORMAL).expect("the first pass");
    assert_eq!(
        ao.observe_events(EVENTS_RESET)
            .expect_err("seq 1 already carried something else"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::ConflictingDuplicate
        }
    );
}

#[tokio::test]
async fn every_replay_break_blocks_scheduling_by_invalidating_the_bindings() {
    // A break is not only reported. Until reconciliation reclassifies them, the
    // bindings Kontor holds are no longer evidence, so a gap cannot be shrugged
    // off and worked through.
    for (recording, reason) in [
        (EVENTS_GAP, TimelineBreak::SequenceGap),
        (EVENTS_CONFLICTING, TimelineBreak::ConflictingDuplicate),
    ] {
        let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
        let launched = ao
            .launch(&launch_request(run(RUN_CLAUDE)))
            .await
            .expect("the lane launches");
        assert_eq!(
            ao.observe_events(recording).expect_err("the log broke"),
            RuntimeError::TimelineRefetchRequired { reason }
        );
        assert_eq!(ao.generation(), 2);
        assert!(ao.checkpoint().bindings.is_empty());
        assert!(
            matches!(
                ao.inspect(&InspectRequest {
                    binding: launched.snapshot,
                    requested_at: at("2026-08-10T10:00:00Z"),
                })
                .await
                .expect_err("the binding is no longer evidence"),
                RuntimeError::StaleBinding { .. }
            ),
            "a {reason:?} must block work on every binding until reconciliation runs"
        );
    }
}

#[tokio::test]
async fn a_reset_change_log_starts_a_new_generation_and_invalidates_every_binding() {
    // The reset that matters in practice: Kontor restarts, reads its persisted
    // cursor, reconnects — and AO's database was wiped, so the sequence Kontor
    // durably accepted now carries a different change. The persisted digest at the
    // rehydration boundary is the only thing that can tell this from a benign
    // replay, which is why the checkpoint carries one.
    let first = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = first
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    first.observe_events(EVENTS_NORMAL).expect("the first pass");
    let checkpoint = first.checkpoint();
    assert_eq!(checkpoint.last_event_seq, 4);
    assert!(
        checkpoint.last_event_digest.is_some(),
        "the cursor persists the digest that makes a reset detectable"
    );

    let ao = Fixture::restarted(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE),
        checkpoint,
    );
    assert_eq!(ao.generation(), 1);
    assert_eq!(
        ao.observe_events(EVENTS_RESET_AT_BOUNDARY)
            .expect_err("this is a different change log"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::EpochChanged
        }
    );
    assert_eq!(ao.generation(), 2, "a reset mints a new generation");
    assert!(
        ao.checkpoint().bindings.is_empty(),
        "a repeated native id in a new generation is a different session, so the \
         old bindings are invalidated rather than re-pointed"
    );

    // And the stale binding can no longer drive anything.
    assert!(matches!(
        ao.inspect(&InspectRequest {
            binding: launched.snapshot,
            requested_at: at("2026-08-10T10:00:00Z"),
        })
        .await
        .expect_err("the generation moved"),
        RuntimeError::StaleBinding { .. }
    ));
}

#[tokio::test]
async fn an_ao_restart_duplicates_no_session_event_or_message() {
    // Round one: launch, follow up, consume the log.
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let request = launch_request(run(RUN_CLAUDE));
    let launched = ao.launch(&request).await.expect("the lane launches");
    let message_id = MessageId::generate();
    let acknowledged = ao
        .send(&SendMessageRequest {
            binding: launched.snapshot.clone(),
            message_id,
            body: text("please continue"),
            sent_at: at("2026-08-10T09:40:00Z"),
        })
        .await
        .expect("AO accepts the message");
    ao.observe_events(EVENTS_NORMAL).expect("the log is read");
    let checkpoint = ao.checkpoint();

    // Round two: Kontor restarts and rebuilds from the checkpoint alone.
    let restarted = Fixture::restarted(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE),
        checkpoint.clone(),
    );
    assert_eq!(
        restarted.generation(),
        1,
        "a clean restart keeps the generation"
    );

    // AO replays what Kontor already accepted and continues.
    let replayed = restarted
        .observe_events(EVENTS_CLEAN_RESTART)
        .expect("a clean replay is benign");
    assert!(
        replayed.is_empty(),
        "every replayed change was already persisted, so none is persisted twice"
    );

    // The binding survives, unchanged.
    let reloaded = restarted.checkpoint();
    assert_eq!(reloaded.bindings, checkpoint.bindings);
    assert_eq!(
        reloaded
            .bindings
            .first()
            .map(RuntimeBindingSnapshot::binding_id),
        Some(request.binding_id)
    );

    // And the message ledger still answers the same identifier from evidence
    // rather than delivering it a second time.
    let route = AoCall::send("ses_claude_1", String::new());
    let after = restarted
        .send(&SendMessageRequest {
            binding: launched.snapshot,
            message_id,
            body: text("please continue"),
            sent_at: at("2026-08-10T10:05:00Z"),
        })
        .await
        .expect("the retry replays from the rehydrated ledger");
    assert_eq!(after, acknowledged);
    assert_eq!(
        restarted.count(&route),
        0,
        "a rehydrated ledger must answer without touching AO"
    );
    assert_eq!(reloaded.deliveries.len(), 1);
}

#[tokio::test]
async fn a_persisted_change_is_keyed_by_generation_so_a_reset_cannot_collide() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let first = ao.observe_events(EVENTS_NORMAL).expect("the first log");
    let lane = lane(AoHarness::ClaudeCode);
    let keys: BTreeSet<String> = first.iter().map(|it| it.dedup_key(&lane)).collect();
    assert_eq!(keys.len(), first.len(), "one key per change");
    assert!(keys.iter().all(|key| key.contains("|1|")));

    // After a reset, seq 1 exists again — in another generation. Dropping the
    // generation from the key would make the new change vanish as a duplicate.
    ao.observe_events(EVENTS_RESET)
        .expect_err("a contradiction");
    let second = Fixture::restarted(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE),
        AoCheckpoint::fresh(ao.generation()),
    );
    let after = second.observe_events(EVENTS_RESET).expect("a fresh log");
    let reset_key = after
        .first()
        .expect("the fresh log has a first change")
        .dedup_key(&lane);
    assert!(
        !keys.contains(&reset_key),
        "seq 1 in generation 2 must not collide with seq 1 in generation 1"
    );
}

// ---------------------------------------------------------------------------
// Adoption inbox and parent links
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_foreign_session_enters_the_inbox_and_a_labelled_one_is_only_adoptable() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let launched = ao
        .launch(&launch_request(run(RUN_CLAUDE)))
        .await
        .expect("the lane launches");
    ao.take_calls();

    let report = ao
        .reconcile(std::slice::from_ref(&launched.snapshot))
        .await
        .expect("reconciliation succeeds");

    let orphans: Vec<&NativeRuntimeIdentity> = report
        .findings
        .iter()
        .filter_map(|finding| match finding {
            ReconciliationFinding::Orphan { identity } => Some(identity),
            _ => None,
        })
        .collect();
    let orphan_ids: Vec<&str> = orphans.iter().map(|it| it.native_id.as_str()).collect();
    assert!(
        orphan_ids.contains(&"ses_foreign_1"),
        "an unlabelled session is an inbox entry, not a Kontor session: {orphan_ids:?}"
    );
    assert!(
        orphan_ids.contains(&"ses_ao_child_1"),
        "an AO-internal orchestrator child has no durable parent field in 0.12.1, so \
         it is a foreign discovery result rather than a linked child"
    );

    let adoptable: Vec<AgentRunId> = report
        .findings
        .iter()
        .filter_map(|finding| match finding {
            ReconciliationFinding::Adoptable { agent_run_id, .. } => Some(*agent_run_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        adoptable,
        vec![run(RUN_ADOPTABLE)],
        "a labelled unbound session is offered, never taken"
    );

    // Nothing was mutated to reach any of those conclusions.
    assert!(
        ao.mutations().is_empty(),
        "reconciliation proposes; it never rebinds or adopts"
    );
    for finding in &report.findings {
        match finding.action() {
            ReconciliationAction::Keep
            | ReconciliationAction::ProposeAdoption
            | ReconciliationAction::ProposeInboxEntry
            | ReconciliationAction::ProposeLostContactReview
            | ReconciliationAction::ProposeOrphanReview => {}
        }
    }

    // And the mutation itself stays unsupported, so an offer cannot be accepted
    // through this adapter.
    assert_unsupported(
        RuntimeCapability::Adopt,
        ao.adopt(&AdoptRequest {
            agent_run_id: run(RUN_ADOPTABLE),
            binding_id: RuntimeBindingId::generate(),
            native: launched.snapshot.identity().clone(),
            adopted_at: at("2026-08-10T09:10:00Z"),
        })
        .await,
    );
}

#[tokio::test]
async fn a_flat_ao_inventory_neither_erases_nor_invents_a_parent_link() {
    // Kontor owns the parent relationship. The adapter's whole input surface for
    // a launch is an `AgentRunId`: there is no field through which a parent could
    // be read, written or inferred, and reconciliation returns only observed and
    // binding proposals. This test pins that a parent run and its child run
    // reconcile as two independent bindings even though AO lists both flat, with
    // no finding that merges, re-points or invents a relationship between them.
    //
    // The storage half of the acceptance criterion — reload the child row and
    // assert `parent_agent_run_id` is unchanged — is proven against the real
    // schema in `kontor-store`'s `repository_roundtrip` suite, which this adapter
    // never writes to.
    let parent_run = run(RUN_CLAUDE);
    let child_run = run(RUN_ADOPTABLE);

    let first = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    let parent = first
        .launch(&launch_request(parent_run))
        .await
        .expect("the parent run launches")
        .snapshot;

    // The child is bound to the other labelled session in the same flat lane. It
    // arrives the way a second lane's binding really reaches this adapter — through
    // the checkpoint the KON-MVP-03 tables hand back — because reconciliation now
    // classifies only bindings the runtime vouches for, and a snapshot assembled in
    // a test is not one of those.
    let child = bound_snapshot(&first, child_run, "ses_adoptable_1", RUN_ADOPTABLE);
    let mut checkpoint = first.checkpoint();
    checkpoint.bindings.push(child.clone());
    let ao = Fixture::restarted(
        AoHarness::ClaudeCode,
        daemon(SPAWN_CLAUDE, SESSION_LIVE),
        checkpoint,
    );

    let report = ao
        .reconcile(&[parent.clone(), child.clone()])
        .await
        .expect("reconciliation succeeds");

    let matched: Vec<(AgentRunId, &str)> = report
        .findings
        .iter()
        .filter_map(|finding| match finding {
            ReconciliationFinding::Matched {
                agent_run_id,
                identity,
                ..
            } => Some((*agent_run_id, identity.native_id.as_str())),
            _ => None,
        })
        .collect();
    assert!(matched.contains(&(parent_run, "ses_claude_1")));
    assert!(matched.contains(&(child_run, "ses_adoptable_1")));
    assert_eq!(
        matched.len(),
        2,
        "two runs, two bindings, and nothing that merges them"
    );
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.proposed_state().is_none()),
        "a flat inventory of two live sessions proposes no state change at all"
    );
}

/// A binding for a session the fixtures already contain, as the runtime would
/// have issued it.
fn bound_snapshot(
    ao: &AoAdapter,
    agent_run_id: AgentRunId,
    native_id: &str,
    pinned: &str,
) -> RuntimeBindingSnapshot {
    let identity = NativeRuntimeIdentity {
        runtime_kind: ao.lane().runtime_kind.clone(),
        host: ao.lane().host.clone(),
        generation: ao.generation(),
        native_id: ExternalId::parse(native_id).expect("valid native id"),
    };
    let bound_at = at("2026-08-10T09:00:00Z");
    RuntimeBindingSnapshot {
        binding: RuntimeBinding {
            id: RuntimeBindingId::generate(),
            agent_run_id,
            identity: identity.clone(),
            bound_at,
        },
        capabilities: ao.lane().capabilities(),
        correlation: CorrelationEvidence::establish(
            agent_run_id,
            &format!("kontor-run-{pinned}"),
            identity,
            bound_at,
        )
        .expect("the fixture branch is this run's label"),
    }
}

// ---------------------------------------------------------------------------
// PTY bytes are not session content
// ---------------------------------------------------------------------------

#[test]
fn recorded_mux_frames_parse_and_stay_out_of_the_semantic_contract() {
    let frames = mux::parse_frames(MUX_FRAMES).expect("the recording is AO 0.12.1 mux");
    assert!(
        frames.len() >= 10,
        "the recording covers the whole protocol"
    );

    // Every client and server frame type the plan names is present.
    let types: BTreeSet<&str> = frames.iter().map(|it| it.frame_type.as_str()).collect();
    for expected in [
        "open",
        "data",
        "resize",
        "opened",
        "exited",
        "error",
        "subscribe",
        "snapshot",
        "ping",
        "pong",
    ] {
        assert!(
            types.contains(expected),
            "{expected} is not in the recording"
        );
    }

    // The payload frames really do carry bytes rather than text, which is the
    // whole reason they cannot be semantic content.
    let payloads: Vec<&mux::AoMuxFrame> = frames.iter().filter(|it| it.is_pty_payload()).collect();
    assert!(!payloads.is_empty());
    for frame in &payloads {
        let len = frame.pty_payload_len().expect("a base64 payload");
        assert!(len > 0);
    }
    // One recorded payload is deliberately not valid UTF-8, so reading it as a
    // message body is not merely wrong policy but impossible.
    assert!(
        payloads
            .iter()
            .any(|frame| frame.data.as_deref() == Some("//79")),
        "the recording keeps a non-UTF-8 payload"
    );

    // The session channel carries a refetch notification and no change payload:
    // the protocol itself says the mux is a notification channel.
    let snapshot = frames
        .iter()
        .find_map(|it| it.session.as_ref())
        .expect("a sessions/snapshot frame");
    assert_eq!(snapshot.seq, 3);
    assert_eq!(snapshot.event_type, "session_updated");
}

#[tokio::test]
async fn no_mux_frame_becomes_a_runtime_event_or_a_message() {
    let ao = Fixture::new(AoHarness::ClaudeCode, daemon(SPAWN_CLAUDE, SESSION_LIVE));
    // The mux recording carries no AO change events, and the adapter has no path
    // that would turn a PTY frame into one: `observe_events` reads only `data:`
    // frames of the CDC stream, so a mux log yields nothing at all rather than a
    // stream of invented lifecycle facts.
    assert!(
        ao.observe_events(MUX_FRAMES)
            .expect("a mux log is simply not a change stream")
            .is_empty(),
        "no PTY frame may become a runtime event"
    );
    // The two recordings are not interchangeable in either direction.
    assert!(
        mux::parse_frames(EVENTS_NORMAL).is_err(),
        "an SSE recording is not a mux frame log"
    );
    let accepted = ao.observe_events(EVENTS_NORMAL).expect("real changes");
    for event in &accepted {
        let json = event.evidence.json();
        assert!(
            !json.contains("G1syShtbMzJt") && !json.contains("//79"),
            "no base64 PTY payload may reach persisted evidence"
        );
    }
    assert!(ao.checkpoint().deliveries.is_empty());
}

// ---------------------------------------------------------------------------
// Unsupported product areas stay out
// ---------------------------------------------------------------------------

#[test]
fn no_mobile_plan_state_or_account_routing_model_exists_in_this_adapter() {
    // An API audit rather than a comment: the fixtures are the recorded AO
    // surface this adapter reads, and none of them carries a mobile, plan-state
    // or account-routing model.
    for (name, fixture) in [
        ("agents", AGENTS),
        ("project", PROJECT_SAFE),
        ("inventory", INVENTORY),
        ("spawn", SPAWN_CLAUDE),
        ("restore", RESTORE_OK),
        ("resume-agent", RESUME_AGENT_OK),
    ] {
        let lowered = fixture.to_lowercase();
        for forbidden in [
            "mobile",
            "planstate",
            "plan_state",
            "accountid",
            "account_id",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "{name} fixture must not carry a {forbidden} model"
            );
        }
    }
    // And the declared capability set says the same thing structurally.
    let declared = lane(AoHarness::ClaudeCode).capabilities();
    assert!(!declared.account_env);
    assert_eq!(declared.limits.max_history_page, 0);
}

#[test]
fn the_agent_catalog_is_discovery_evidence_and_never_account_identity() {
    let catalog: AoListAgentsResponse = serde_json::from_str(AGENTS).expect("the catalog parses");
    // AO documents `authorized` as an advisory, stale-prone local probe with
    // spawn as the authoritative check, so it can say a binary resolved and can
    // never say which account it runs as.
    for entry in catalog.authorized.iter().chain(&catalog.installed) {
        assert!(!entry.id.is_empty() && !entry.label.is_empty());
    }
    for harness in AoHarness::ALL {
        assert!(
            catalog.installed.iter().any(|it| it.id == harness.as_str()),
            "{harness} is a lane in this suite and must be installed in the fixture"
        );
    }
    // An unverified harness is present in AO's own catalog and is simply not
    // drivable from here.
    assert!(catalog.supported.iter().any(|it| it.id == "aider"));
    assert!(AoHarness::parse("aider").is_err());
}

#[test]
fn the_fixture_manifest_pins_its_provenance_and_carries_no_ambient_path() {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).expect("the manifest parses");
    assert_eq!(manifest["ao_version"], AO_VERSION);
    assert_eq!(
        manifest["ao_git_sha"], "1df40e93772c2c48e916870d9c3ddf8f29a69f84",
        "fixtures are pinned to one AO commit"
    );
    assert!(
        manifest["derived_from"]
            .as_array()
            .is_some_and(|it| it.len() >= 8),
        "every recorded surface names the AO source it came from"
    );
    // Sanitization: no developer home, no real machine path, no credential.
    for fixture in [
        MANIFEST,
        AGENTS,
        PROJECT_SAFE,
        INVENTORY,
        SPAWN_CLAUDE,
        SEND_OK,
        MUX_FRAMES,
        CODEX_ARGV,
    ] {
        let lowered = fixture.to_lowercase();
        for forbidden in ["/users/", "/home/", "authorization", "bearer ", "password"] {
            assert!(
                !lowered.contains(forbidden),
                "a fixture must carry no {forbidden}"
            );
        }
    }
    // Every SSE recording is a real AO frame log.
    for recording in [
        EVENTS_NORMAL,
        EVENTS_DUPLICATE,
        EVENTS_GAP,
        EVENTS_CONFLICTING,
        EVENTS_RESET,
        EVENTS_CLEAN_RESTART,
    ] {
        assert!(
            recording.contains("event: "),
            "an SSE frame carries its type"
        );
        assert!(!parse_sse_events(recording).expect("parses").is_empty());
    }
}

#[test]
fn the_recorded_inventory_is_one_ao_envelope_including_harnesses_kontor_declines() {
    let inventory: AoListSessionsResponse =
        serde_json::from_str(INVENTORY).expect("the inventory parses");
    assert!(inventory.sessions.len() >= 10);
    // A harness this adapter has not verified must not break the inventory: one
    // foreign session would otherwise cost Kontor the reconciliation of every
    // other one.
    assert!(
        inventory
            .sessions
            .iter()
            .any(|it| it.harness.as_deref() == Some("aider"))
    );
    assert!(
        inventory
            .sessions
            .iter()
            .any(|it| it.kind == AoSessionKind::Orchestrator)
    );
}
