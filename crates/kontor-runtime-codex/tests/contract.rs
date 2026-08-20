//! The Codex adapter, judged against recorded processes, real approved config
//! homes, and the shared capability-aware contract.
//!
//! Two kinds of test live here, and the split is deliberate. The shared contracts
//! from `kontor_tests_contract` prove this adapter is the *same kind of thing* as
//! every other one — identity preserved, undeclared operations refused before
//! dispatch, no acknowledgement mistaken for a completion. The Codex-specific
//! cases prove the things only this runtime can get wrong.
//!
//! The account homes are **real directories** under `tests/fixtures/`, resolved
//! through a real `ResolverPolicy` and a real `AccountResolver`. Stubbing the
//! resolved environment would have made the central claim of this ticket
//! untestable: what is under test is that the value the child receives is the
//! approved home for the account the run is pinned to, and a stub would be
//! whatever the test said it was.
//!
//! The mutants this suite exists to kill:
//!
//! * letting an ambient `CODEX_HOME` — Kontor's own, or a leftover — reach a run
//!   pinned to another account;
//! * accepting a resolved home without checking whose it is, so two accounts
//!   share one identity because one policy mapped two aliases to one directory;
//! * reading an exit status, an EOF, a signal, a deadline or a kill as a verdict
//!   on the work, which closes runs that never finished;
//! * answering an undeclared operation with an empty page, a synthetic permission
//!   state or a fabricated acknowledgement instead of a typed refusal;
//! * starting a second process for a replayed launch, or two for one seat;
//! * renumbering stdout over a frame the transport had to drop, so the caller
//!   receives content with an invisible hole in it;
//! * putting a config home, an auth file's contents or a prompt into a receipt, a
//!   checkpoint, a ledger, a refusal or a `Debug` rendering.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use kontor_accounts::{
    AccountEnvironmentMap, AccountLaunchReceipt, AccountResolver, AdmittedLaunch, KeychainBackend,
    KeychainFailure, KeychainTarget, LaunchRefusal, ResolverPolicy,
};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash,
    CredentialAlias, EnvironmentVariableName, ExternalId, ExternalName, MiniProjectId, ProjectId,
    RealmId, RoleSlotId, RuntimeBindingId, RuntimeKindKey, SCHEMA_VERSION, TaskId, TeamRunId,
};
use kontor_core::repository::{
    AccountProfile, CredentialReference, CredentialReferenceKind, RuntimeBinding,
};
use kontor_core::state::{ObservedRunState, RuntimeContact};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{
    OperationContext, RuntimeBindingSnapshot, RuntimeCapabilities, RuntimeCapability, TrustGrade,
    preflight,
};
use kontor_runtime::observation::{ObservationSource, ReconciliationAction, ReconciliationFinding};
use kontor_runtime::request::{
    AdoptRequest, CancelRequest, InspectRequest, LaunchParts, LaunchPlacement, LaunchRequest,
    LiveSubscribeRequest, MessageId, PermissionDecision, PermissionResponseRequest, ResumeRequest,
    SendMessageRequest,
};
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::timeline::{SessionEventKind, TimelineBreak, TimelinePosition};
use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_runtime_codex::adapter::{
    CodexAccountAdmission, CodexAccountAuthority, CodexAccountRequest, CodexAdapter,
    CodexCheckpoint, CodexConfig, UNSUPPORTED,
};
use kontor_runtime_codex::client::{
    CodexCommand, CodexDrained, CodexLiveTransport, CodexStarted, EXEC_ROUTE, PreparedCommand,
};
use kontor_runtime_codex::fixture::{CodexDispatch, CodexScript, RecordedCodex};
use kontor_runtime_codex::wire::{CODEX_HOME, CodexEnding, KONTOR_RUN_ENV};
use kontor_tests_contract::{
    SESSION_KINDS, adapter_contract, assert_native_id_is_not_a_kontor_id, at, closes,
    drain_history, session_content_contract, text,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// The account-profile ids the committed markers name. Fixed rather than
/// generated: the marker is a file on disk, and the whole point of the check is
/// that the two agree.
const PROFILE_A: &str = "01936f4a-0000-7000-8000-00000000000a";
const PROFILE_B: &str = "01936f4a-0000-7000-8000-00000000000b";
const IDENTITY_A: &str = "codex-account-a@example.test";
const IDENTITY_B: &str = "codex-account-b@example.test";
const ALIAS_A: &str = "codex-home-account-a";
const ALIAS_B: &str = "codex-home-account-b";

/// The contents of each fixture home's stand-in credential file. Kontor never
/// opens either, so neither string may exist anywhere this suite can reach.
const AUTH_CANARY_A: &str = "kontor-canary-codex-alpha-auth-7f19d2";
const AUTH_CANARY_B: &str = "kontor-canary-codex-beta-auth-3c58e1";

/// A prompt no artefact may quote.
const PROMPT: &str = "implement the thing, and mention kontor-canary-prompt-4b21f8";
const PROMPT_CANARY: &str = "kontor-canary-prompt-4b21f8";

const ACK: &str =
    "{\"id\":\"0\",\"msg\":{\"type\":\"session_configured\",\"session_id\":\"cdx-1\"}}";
const ACK_TWO: &str =
    "{\"id\":\"0\",\"msg\":{\"type\":\"session_configured\",\"session_id\":\"cdx-2\"}}";
const MESSAGE: &str = "{\"id\":\"1\",\"msg\":{\"type\":\"agent_message\",\"message\":\"hello\"}}";
const TOOL_CALL: &str =
    "{\"id\":\"2\",\"msg\":{\"type\":\"exec_command_begin\",\"command\":[\"ls\"]}}";
const DIAGNOSTIC: &str = "{\"id\":\"3\",\"msg\":{\"type\":\"token_count\",\"total\":42}}";
const COMPLETE: &str = "{\"id\":\"4\",\"msg\":{\"type\":\"task_complete\"}}";

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The canonical path of one approved home, as the resolver policy resolves it.
fn home_path(name: &str) -> String {
    std::fs::canonicalize(fixture_dir(name))
        .expect("the fixture home exists")
        .to_string_lossy()
        .into_owned()
}

fn worktree() -> WorkspaceRoot {
    let canonical = std::fs::canonicalize(fixture_dir("worktree"))
        .expect("the fixture worktree exists")
        .to_string_lossy()
        .into_owned();
    WorkspaceRoot::parse(&canonical).expect("the fixture worktree is an absolute path")
}

fn execution_scope(task_id: TaskId, root: WorkspaceRoot) -> ExecutionScope {
    ExecutionScope::for_task(
        EpicScope {
            mini_project_id: MiniProjectId::generate(),
            external_epic_key: ExternalId::parse("ASMA-TEST").expect("epic key"),
            short_title: ExternalName::parse("Adapter contract").expect("epic title"),
        },
        TaskScope {
            task_id,
            external_issue_key: ExternalId::parse("ASMA-TEST-1").expect("issue key"),
            short_code: ExternalId::parse("TEST-1").expect("short code"),
            worktree: root,
        },
    )
}

fn alias(text: &str) -> CredentialAlias {
    CredentialAlias::parse(text).expect("a valid alias")
}

fn profile_id(text: &str) -> AccountProfileId {
    AccountProfileId::parse(text).expect("a valid account profile id")
}

fn harness() -> RuntimeKindKey {
    RuntimeKindKey::parse("codex.exec").expect("a valid runtime key")
}

/// A keychain that refuses everything and counts being asked.
///
/// This adapter's isolation model is a config home, so a keychain lookup would
/// mean a profile shaped in a way it cannot prove. Refusing rather than
/// answering is what turns that into a visible failure.
#[derive(Debug, Default)]
struct NoKeychain;

impl KeychainBackend for NoKeychain {
    fn secret(&self, _target: &KeychainTarget) -> Result<secrecy::SecretString, KeychainFailure> {
        Err(KeychainFailure::NotFound)
    }
}

fn policy() -> ResolverPolicy {
    ResolverPolicy::builder()
        .harness(harness())
        .config_home(alias(ALIAS_A), &fixture_dir("account-a"))
        .expect("account-a is an approved home")
        .config_home(alias(ALIAS_B), &fixture_dir("account-b"))
        .expect("account-b is an approved home")
        .environment(EnvironmentVariableName::parse(CODEX_HOME).expect("a valid variable name"))
        .build()
}

fn empty_document() -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 1 }))
        .expect("a canonical document")
}

/// One account profile, pointed at an approved home by alias.
///
/// `home_alias` is a parameter rather than a derived value so a test can point a
/// profile at *another account's* home, which is the confusion the marker check
/// exists to catch.
fn profile(id: &str, identity: &str, home_alias: &str) -> AccountProfile {
    let environment = AccountEnvironmentMap::new().with(
        EnvironmentVariableName::parse(CODEX_HOME).expect("a valid variable name"),
        CredentialReference {
            kind: CredentialReferenceKind::ConfigHome,
            alias: alias(home_alias),
        },
    );
    AccountProfile {
        id: profile_id(id),
        project_id: ProjectId::generate(),
        label: ExternalName::parse("codex fixture account").expect("a valid label"),
        external_account_id: None,
        harness: harness(),
        credential_ref: CredentialReference {
            kind: CredentialReferenceKind::ConfigHome,
            alias: alias(home_alias),
        },
        environment: environment
            .to_document()
            .expect("a canonical environment map"),
        routing: empty_document(),
        capability: empty_document(),
        provider_identity: Some(ExternalId::parse(identity).expect("a valid provider identity")),
        enabled: true,
        revision: AggregateRevision::INITIAL,
        created_at: at("2026-08-10T08:00:00Z"),
        updated_at: at("2026-08-10T08:00:00Z"),
    }
}

/// A profile that delivers nothing through the environment.
///
/// The shape an "inherited `CODEX_HOME`" deployment actually has: nobody sets the
/// variable, so whatever the parent process carries would be inherited. The
/// adapter must refuse rather than run under it.
fn ambient_profile(id: &str, identity: &str) -> AccountProfile {
    AccountProfile {
        environment: AccountEnvironmentMap::new()
            .to_document()
            .expect("an empty environment map"),
        ..profile(id, identity, ALIAS_A)
    }
}

// ---------------------------------------------------------------------------
// The account authority
// ---------------------------------------------------------------------------

/// A recorded account authority that resolves for real.
///
/// It stands in for [`kontor_runtime_codex::adapter::CodexPinnedAccounts`], whose
/// own ordering rules — the pin is the run's, the profile must be enabled,
/// availability must be fresh, nothing is resolved until the rest agrees, and the
/// profile is re-read afterwards — live in `admit_pinned_launch` and are pinned by
/// KON-MVP-07's suite against a real store. What this suite owns is everything
/// *after* that decision, so the resolution here is real and the refusals are
/// scripted.
struct RecordedAccounts {
    policy: ResolverPolicy,
    keychain: NoKeychain,
    profiles: BTreeMap<AccountProfileId, AccountProfile>,
    refusals: Mutex<VecDeque<LaunchRefusal>>,
    calls: AtomicUsize,
}

impl RecordedAccounts {
    fn new(profiles: Vec<AccountProfile>) -> Self {
        Self {
            policy: policy(),
            keychain: NoKeychain,
            profiles: profiles
                .into_iter()
                .map(|profile| (profile.id, profile))
                .collect(),
            refusals: Mutex::new(VecDeque::new()),
            calls: AtomicUsize::new(0),
        }
    }

    /// Refuse the next admission the way KON-MVP-07 would.
    fn refusing(self, refusal: LaunchRefusal) -> Self {
        self.refusals
            .lock()
            .expect("the fixture lock is intact")
            .push_back(refusal);
        self
    }
}

impl CodexAccountAuthority for RecordedAccounts {
    fn admit(
        &self,
        request: &CodexAccountRequest<'_>,
    ) -> Result<CodexAccountAdmission, LaunchRefusal> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if let Some(refusal) = self
            .refusals
            .lock()
            .expect("the fixture lock is intact")
            .pop_front()
        {
            return Err(refusal);
        }
        let profile = self
            .profiles
            .get(&request.account_profile_id)
            .ok_or(LaunchRefusal::ProfileNotFound)?;
        if !profile.enabled {
            return Err(LaunchRefusal::ProfileDisabled);
        }
        // The runtime's own gate, with the pin declared — exactly as
        // `admit_pinned_launch` applies it, and never a second copy of the rule.
        let mut context = OperationContext::new(RuntimeCapability::Launch);
        context.account_pinned = true;
        preflight(request.capabilities, &context).map_err(LaunchRefusal::Runtime)?;

        let resolver = AccountResolver::new(&self.policy, &self.keychain);
        let environment = resolver
            .resolve(profile)
            .map_err(LaunchRefusal::Resolution)?;
        let receipt = AccountLaunchReceipt {
            schema_version: SCHEMA_VERSION,
            realm_id: RealmId::generate(),
            project_id: profile.project_id,
            agent_run_id: request.agent_run_id,
            account_profile_id: profile.id,
            account_profile_revision: profile.revision,
            harness: profile.harness.clone(),
            provider_identity: profile.provider_identity.clone(),
            environment_names: environment.names(),
            policy_evidence: self.policy.evidence(),
            availability_evidence: ExternalId::parse("fleet-observation-1").expect("a valid id"),
            availability_observed_at: request.now,
            capability_evidence: ContentHash::of(
                serde_json::to_string(request.capabilities)
                    .expect("capabilities serialize")
                    .as_bytes(),
            ),
            decided_at: request.now,
        };
        Ok(CodexAccountAdmission {
            admitted: AdmittedLaunch {
                receipt,
                environment,
            },
            credential_alias: profile.credential_ref.alias.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn config() -> CodexConfig {
    CodexConfig {
        runtime_kind: harness(),
        host_key: ExternalName::parse("codex-fixture-host").expect("a valid host key"),
        executable: "codex".to_owned(),
        task_worktree: worktree(),
        max_concurrent_sessions: 4,
    }
}

struct Plane {
    adapter: CodexAdapter<'static>,
    codex: std::sync::Arc<RecordedCodex>,
}

fn build_plane(codex: RecordedCodex, accounts: RecordedAccounts) -> Plane {
    let codex = std::sync::Arc::new(codex);
    Plane {
        adapter: CodexAdapter::new(
            config(),
            Box::new(std::sync::Arc::clone(&codex)),
            Box::new(accounts),
            CodexCheckpoint::fresh(1),
        ),
        codex,
    }
}

/// One plane running account A, with both auth canaries and account B's home
/// watched for.
fn plane_a(codex: RecordedCodex) -> Plane {
    build_plane(
        codex
            .watching_for(AUTH_CANARY_A)
            .watching_for(AUTH_CANARY_B)
            .watching_for(&home_path("account-b")),
        RecordedAccounts::new(vec![profile(PROFILE_A, IDENTITY_A, ALIAS_A)]),
    )
}

fn one_run_script() -> RecordedCodex {
    RecordedCodex::new().running(CodexScript::acknowledging(ACK))
}

async fn prepared_workspace(
    adapter: &CodexAdapter<'_>,
    team_run_id: TeamRunId,
    task_id: TaskId,
) -> WorkspaceBindingSnapshot {
    adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            scope: execution_scope(task_id, worktree()),
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            display_name: ExternalName::parse("TSW • ASMA-1 • TEST-1").expect("a native name"),
            root: worktree(),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("the task worktree verifies")
        .snapshot
}

struct Seat {
    slot: RoleSlotKey,
    task_id: TaskId,
    workspace: WorkspaceBindingSnapshot,
}

async fn open_seat(adapter: &CodexAdapter<'_>, slot_id: &str) -> Seat {
    let team_run_id = TeamRunId::generate();
    let task_id = TaskId::generate();
    let workspace = prepared_workspace(adapter, team_run_id, task_id).await;
    Seat {
        slot: RoleSlotKey::new(
            team_run_id,
            RoleSlotId::parse(slot_id).expect("a valid slot id"),
        ),
        task_id,
        workspace,
    }
}

/// The parts of a launch, with every knob a test might turn.
struct Parts {
    agent_run_id: AgentRunId,
    binding_id: RuntimeBindingId,
    account: Option<AccountProfileId>,
    workspace: Option<WorkspaceBindingSnapshot>,
    cwd: WorkspaceRoot,
    prompt: String,
}

fn parts(seat: &Seat, account: &str) -> Parts {
    Parts {
        agent_run_id: AgentRunId::generate(),
        binding_id: RuntimeBindingId::generate(),
        account: Some(profile_id(account)),
        workspace: Some(seat.workspace.clone()),
        cwd: worktree(),
        prompt: PROMPT.to_owned(),
    }
}

/// The standard-fallback context policy a seat launches under when the test is
/// about something else. Codex declares context configuration, so the effective
/// half is `configured`.
fn standard_context_policy() -> kontor_core::spec::ContextPolicySnapshot {
    kontor_core::spec::ContextPolicySnapshot::standard(
        &kontor_core::spec::ContextWindowBounds::unknown(),
        true,
        kontor_core::id::SCHEMA_VERSION,
        at("2026-08-10T09:00:00Z"),
    )
    .expect("the standard fallback freezes")
}

async fn admitted(adapter: &CodexAdapter<'_>, seat: &Seat, parts: &Parts) -> LaunchRequest {
    adapter
        .admit_launch(&AdmissionRequest {
            slot: seat.slot.clone(),
            agent_run_id: parts.agent_run_id,
            binding_id: parts.binding_id,
            replaces: None,
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("a vacant seat admits one launch")
        .into_authority()
        .expect("the seat was free")
        .into_request(LaunchParts {
            scope: execution_scope(seat.task_id, parts.cwd.clone()),
            display_name: ExternalName::parse("Implement • KON-19").expect("display name"),
            agent_run_id: parts.agent_run_id,
            team_run_id: seat.slot.team_run_id,
            role_slot_id: seat.slot.role_slot_id.clone(),
            task_id: seat.task_id,
            binding_id: parts.binding_id,
            placement: parts.workspace.clone().map(LaunchPlacement::Workspace),
            cwd: parts.cwd.clone(),
            account_profile_id: parts.account,
            prompt: BoundedText::parse(&parts.prompt).expect("bounded text"),
            model_rung: kontor_core::spec::ModelRung {
                provider: kontor_core::spec::ProviderRef("codex".to_owned()),
                model: kontor_core::spec::ModelRef("gpt-5.6-sol".to_owned()),
                effort: None,
            },
            context_policy: standard_context_policy(),
            autonomy: kontor_core::spec::SeatAutonomy::standard(),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
}

/// Admit and launch one run in one seat, in the ordinary way.
async fn launch(
    adapter: &CodexAdapter<'_>,
    seat: &Seat,
    account: &str,
) -> (LaunchRequest, RuntimeBindingSnapshot) {
    let parts = parts(seat, account);
    let request = admitted(adapter, seat, &parts).await;
    let outcome = adapter
        .launch(&request)
        .await
        .expect("the launch is admitted");
    (request, outcome.snapshot)
}

fn subscribe(binding: &RuntimeBindingSnapshot, after: u64) -> LiveSubscribeRequest {
    LiveSubscribeRequest {
        binding: binding.clone(),
        kinds: SESSION_KINDS.iter().copied().collect(),
        strict_after: TimelinePosition {
            epoch: 1,
            sequence: after,
        },
    }
}

/// A binding this adapter never issued, self-consistent in every field.
fn forged(
    runtime_kind: &RuntimeKindKey,
    capabilities: RuntimeCapabilities,
) -> RuntimeBindingSnapshot {
    let agent_run_id = AgentRunId::generate();
    let identity = kontor_core::state::NativeRuntimeIdentity {
        runtime_kind: runtime_kind.clone(),
        host: ExternalName::parse("codex-fixture-host").expect("a valid host key"),
        generation: 1,
        native_id: ExternalId::parse("cdx-forged").expect("a valid native id"),
    };
    RuntimeBindingSnapshot {
        binding: RuntimeBinding {
            id: RuntimeBindingId::generate(),
            agent_run_id,
            identity: identity.clone(),
            bound_at: at("2026-08-10T09:00:00Z"),
        },
        capabilities,
        correlation: kontor_runtime::observation::CorrelationEvidence {
            label: kontor_runtime::request::CorrelationLabel::for_run(agent_run_id),
            native: identity,
            established_at: at("2026-08-10T09:00:00Z"),
        },
    }
}

// ---------------------------------------------------------------------------
// The shared contracts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_shared_adapter_and_session_content_contracts_hold() {
    let plane = plane_a(one_run_script().running(CodexScript::acknowledging(ACK_TWO)));
    let seat = open_seat(&plane.adapter, "implement").await;
    let parts = parts(&seat, PROFILE_A);
    let request = admitted(&plane.adapter, &seat, &parts).await;

    let binding = adapter_contract(&plane.adapter, &request)
        .await
        .expect("the shared adapter contract holds");
    session_content_contract(&plane.adapter, &binding)
        .await
        .expect("the shared session-content contract holds");

    // Discovery is the one shared contract this adapter cannot be judged by,
    // because it declares no inventory to discover. It owes the typed refusal
    // instead, and it owes it before anything is dispatched.
    let before = plane.codex.calls().len();
    assert_eq!(
        plane
            .adapter
            .discover_sessions()
            .await
            .expect_err("a runtime with no inventory enumerates nothing"),
        RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::Discovery
        }
    );
    assert_eq!(plane.codex.calls().len(), before);

    assert_native_id_is_not_a_kontor_id(binding.identity().native_id.as_str());
    assert!(
        plane.codex.leaked_canaries().is_empty(),
        "no auth file content and no other account's home reached a dispatch"
    );
}

// ---------------------------------------------------------------------------
// Account isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_launch_proves_its_account_home_before_it_starts_a_process() {
    let plane = plane_a(one_run_script());
    let seat = open_seat(&plane.adapter, "implement").await;
    let (request, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;

    let receipt = plane
        .adapter
        .account_receipt(binding.binding_id())
        .expect("an admitted launch records which account it ran as");
    assert_eq!(receipt.account_profile_id, profile_id(PROFILE_A));
    assert_eq!(receipt.provider_identity.as_str(), IDENTITY_A);
    assert_eq!(receipt.credential_alias.as_str(), ALIAS_A);
    assert_eq!(receipt.marker_schema_version, 1);
    assert_eq!(receipt.account_profile_revision, AggregateRevision::INITIAL);

    // Exactly one process, given exactly one environment variable, in the
    // verified worktree.
    let dispatches = plane.codex.dispatches();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].route, EXEC_ROUTE);
    assert_eq!(dispatches[0].cwd, worktree().as_str());
    assert_eq!(
        dispatches[0].env_names,
        [CODEX_HOME.to_owned(), KONTOR_RUN_ENV.to_owned()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert!(plane.codex.leaked_canaries().is_empty());

    // The binding is this run's, and the launch acknowledgement closes nothing.
    assert_eq!(binding.agent_run_id(), request.agent_run_id());
    let observation = plane
        .adapter
        .inspect(&InspectRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:01:00Z"),
        })
        .await
        .expect("a live process inspects");
    assert_eq!(observation.state, ObservedRunState::Running);
    assert_eq!(observation.contact, RuntimeContact::Reachable);
    assert_eq!(closes(&plane.adapter, &observation, &binding).await, None);
}

#[tokio::test]
async fn two_accounts_run_concurrently_under_their_own_homes() {
    let codex = RecordedCodex::new()
        .running(CodexScript::acknowledging(ACK))
        .running(CodexScript::acknowledging(ACK_TWO))
        .watching_for(AUTH_CANARY_A)
        .watching_for(AUTH_CANARY_B);
    let plane = build_plane(
        codex,
        RecordedAccounts::new(vec![
            profile(PROFILE_A, IDENTITY_A, ALIAS_A),
            profile(PROFILE_B, IDENTITY_B, ALIAS_B),
        ]),
    );

    let first = open_seat(&plane.adapter, "implement").await;
    let second = open_seat(&plane.adapter, "review").await;
    let (_, binding_a) = launch(&plane.adapter, &first, PROFILE_A).await;
    let (_, binding_b) = launch(&plane.adapter, &second, PROFILE_B).await;

    // Two processes, two seats, two accounts — and each receipt names its own.
    let receipt_a = plane
        .adapter
        .account_receipt(binding_a.binding_id())
        .expect("account A's receipt");
    let receipt_b = plane
        .adapter
        .account_receipt(binding_b.binding_id())
        .expect("account B's receipt");
    assert_eq!(receipt_a.account_profile_id, profile_id(PROFILE_A));
    assert_eq!(receipt_b.account_profile_id, profile_id(PROFILE_B));
    assert_eq!(receipt_a.provider_identity.as_str(), IDENTITY_A);
    assert_eq!(receipt_b.provider_identity.as_str(), IDENTITY_B);
    assert_ne!(receipt_a.credential_alias, receipt_b.credential_alias);
    assert_ne!(receipt_a.marker_digest, receipt_b.marker_digest);

    let dispatches = plane.codex.dispatches();
    assert_eq!(dispatches.len(), 2);
    assert_ne!(dispatches[0].process_id, dispatches[1].process_id);
    assert_ne!(binding_a.identity(), binding_b.identity());
    assert!(
        plane.codex.leaked_canaries().is_empty(),
        "neither account's credential file reached either process"
    );
}

#[tokio::test]
async fn an_unpinned_or_ambient_config_home_never_starts_a_process() {
    // A run with no account pin at all. This adapter exists to prove which
    // account executed the work, so it has nothing to offer an unpinned run.
    let plane = plane_a(one_run_script());
    let seat = open_seat(&plane.adapter, "implement").await;
    let unpinned = Parts {
        account: None,
        ..parts(&seat, PROFILE_A)
    };
    let request = admitted(&plane.adapter, &seat, &unpinned).await;
    assert!(matches!(
        plane
            .adapter
            .launch(&request)
            .await
            .expect_err("an unpinned run has no account to prove"),
        RuntimeError::Domain(_)
    ));
    assert!(plane.codex.calls().is_empty(), "and nothing was started");

    // A profile that delivers nothing through the environment. Whatever
    // `CODEX_HOME` this process happens to carry would be inherited, and an
    // inherited home is precisely the failure this adapter exists to prevent.
    let ambient = build_plane(
        one_run_script(),
        RecordedAccounts::new(vec![ambient_profile(PROFILE_A, IDENTITY_A)]),
    );
    let seat = open_seat(&ambient.adapter, "implement").await;
    let parts = parts(&seat, PROFILE_A);
    let request = admitted(&ambient.adapter, &seat, &parts).await;
    assert!(matches!(
        ambient
            .adapter
            .launch(&request)
            .await
            .expect_err("an ambient config home is not a proven one"),
        RuntimeError::Domain(_)
    ));
    assert!(ambient.codex.calls().is_empty());
    assert!(ambient.adapter.checkpoint().bindings.is_empty());
}

#[tokio::test]
async fn a_home_whose_marker_names_another_account_never_starts_a_process() {
    // The confusion the marker exists to catch: a policy that points account B's
    // profile at account A's approved home. Every earlier check passes — the pin
    // matches, the alias is approved, the environment resolves — and only the
    // marker inside the directory disagrees.
    let plane = build_plane(
        one_run_script(),
        RecordedAccounts::new(vec![profile(PROFILE_B, IDENTITY_B, ALIAS_A)]),
    );
    let seat = open_seat(&plane.adapter, "implement").await;
    let parts = parts(&seat, PROFILE_B);
    let request = admitted(&plane.adapter, &seat, &parts).await;

    assert!(matches!(
        plane
            .adapter
            .launch(&request)
            .await
            .expect_err("the home belongs to another account"),
        RuntimeError::Domain(_)
    ));
    assert!(plane.codex.calls().is_empty(), "before any process existed");

    // And the same home with the *right* profile still works, so the refusal is
    // about the mismatch rather than about the directory.
    let honest = plane_a(one_run_script());
    let seat = open_seat(&honest.adapter, "implement").await;
    launch(&honest.adapter, &seat, PROFILE_A).await;
    assert_eq!(honest.codex.started(), 1);
}

#[tokio::test]
async fn an_account_refusal_stops_the_launch_before_any_process() {
    // Every refusal KON-MVP-07 can produce arrives before this adapter has done
    // anything, and each one leaves the seat spendable rather than wedged.
    for refusal in [
        LaunchRefusal::PinMismatch,
        LaunchRefusal::ProfileDisabled,
        LaunchRefusal::ProfileMovedDuringResolution,
        LaunchRefusal::AvailabilityUnknown,
    ] {
        let plane = build_plane(
            one_run_script(),
            RecordedAccounts::new(vec![profile(PROFILE_A, IDENTITY_A, ALIAS_A)]).refusing(refusal),
        );
        let seat = open_seat(&plane.adapter, "implement").await;
        let parts = parts(&seat, PROFILE_A);
        let request = admitted(&plane.adapter, &seat, &parts).await;
        assert!(matches!(
            plane
                .adapter
                .launch(&request)
                .await
                .expect_err("a refused account is a refused launch"),
            RuntimeError::Domain(_)
        ));
        assert!(plane.codex.calls().is_empty());

        // The seat was never claimed, so a second attempt under the same
        // authority is not blocked by the first.
        assert!(
            plane
                .adapter
                .admit_launch(&AdmissionRequest {
                    slot: seat.slot.clone(),
                    agent_run_id: parts.agent_run_id,
                    binding_id: parts.binding_id,
                    replaces: None,
                    requested_at: at("2026-08-10T09:02:00Z"),
                })
                .await
                .is_ok()
        );
    }
}

// ---------------------------------------------------------------------------
// Seats, workspaces and one process per admitted run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn preparing_a_workspace_verifies_the_worktree_and_creates_nothing() {
    let plane = plane_a(one_run_script());
    let team_run_id = TeamRunId::generate();
    let task_id = TaskId::generate();
    let first = plane
        .adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            scope: execution_scope(task_id, worktree()),
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            display_name: ExternalName::parse("TSW • ASMA-1 • TEST-1").expect("a native name"),
            root: worktree(),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("the worktree verifies");
    assert!(
        !first.created,
        "Codex has no workspace to create, and saying it created one would be cosmetic"
    );

    // Idempotent per team run: a retry returns the original binding.
    let again = plane
        .adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            scope: execution_scope(task_id, worktree()),
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            display_name: ExternalName::parse("TSW • ASMA-1 • TEST-1").expect("a native name"),
            root: worktree(),
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect("a retry verifies the same place");
    assert_eq!(again.snapshot, first.snapshot);

    // A directory that is not there is refused rather than discovered by the
    // child after it has already been handed an account.
    let missing = WorkspaceRoot::parse("/definitely/not/a/task/worktree").expect("absolute");
    let missing_task_id = TaskId::generate();
    assert!(matches!(
        plane
            .adapter
            .prepare_workspace(&WorkspacePrepareRequest {
                scope: execution_scope(missing_task_id, missing.clone()),
                team_run_id: TeamRunId::generate(),
                task_id: missing_task_id,
                workspace_binding_id: WorkspaceBindingId::generate(),
                display_name: ExternalName::parse("TSW • ASMA-1 • TEST-1").expect("a native name"),
                root: missing,
                requested_at: at("2026-08-10T09:06:00Z"),
            })
            .await
            .expect_err("a worktree that is not a directory is not a worktree"),
        RuntimeError::WorkspaceMismatch { .. }
    ));
}

#[tokio::test]
async fn a_launch_outside_the_verified_worktree_never_starts_a_process() {
    let plane = plane_a(one_run_script());

    // A workspace binding this adapter never prepared, for the same team run and
    // task, self-consistent in every field. It is what a fabricated one looks
    // like, and the shared claim cannot catch it — a forgery agrees with itself.
    let elsewhere_plane = plane_a(one_run_script());

    // Each case takes its own seat: a launch that refuses *before* it claims
    // leaves the seat's reservation standing, exactly as the shared ledger says
    // an unspent reservation behaves, so reusing one seat would be testing the
    // ledger rather than the workspace rules.
    let wrong_cwd = open_seat(&plane.adapter, "implement-a").await;
    let no_binding = open_seat(&plane.adapter, "implement-b").await;
    let foreign_binding = open_seat(&plane.adapter, "implement-c").await;

    // A working directory that is not the bound root.
    let elsewhere = Parts {
        cwd: WorkspaceRoot::parse("/tmp").expect("absolute"),
        ..parts(&wrong_cwd, PROFILE_A)
    };
    let request = admitted(&plane.adapter, &wrong_cwd, &elsewhere).await;
    assert!(matches!(
        plane
            .adapter
            .launch(&request)
            .await
            .expect_err("a role never works outside the verified worktree"),
        RuntimeError::WorkspaceMismatch { .. }
    ));

    // No workspace binding at all.
    let bare = Parts {
        workspace: None,
        ..parts(&no_binding, PROFILE_A)
    };
    let request = admitted(&plane.adapter, &no_binding, &bare).await;
    assert_eq!(
        plane
            .adapter
            .launch(&request)
            .await
            .expect_err("a launch that skipped preparation has no verified place"),
        RuntimeError::WorkspaceBindingRequired
    );

    // The forgery: prepared somewhere else, presented here.
    let foreign = Parts {
        workspace: Some(
            prepared_workspace(
                &elsewhere_plane.adapter,
                foreign_binding.slot.team_run_id,
                foreign_binding.task_id,
            )
            .await,
        ),
        ..parts(&foreign_binding, PROFILE_A)
    };
    let request = admitted(&plane.adapter, &foreign_binding, &foreign).await;
    assert!(matches!(
        plane
            .adapter
            .launch(&request)
            .await
            .expect_err("a workspace binding from elsewhere verifies nothing here"),
        RuntimeError::WorkspaceMismatch { .. }
    ));

    assert!(
        plane.codex.calls().is_empty(),
        "every workspace refusal happened before a process existed"
    );
}

#[tokio::test]
async fn one_admitted_run_starts_one_process_however_often_it_is_replayed() {
    let plane = plane_a(one_run_script().running(CodexScript::acknowledging(ACK_TWO)));
    let seat = open_seat(&plane.adapter, "implement").await;
    let parts = parts(&seat, PROFILE_A);
    let request = admitted(&plane.adapter, &seat, &parts).await;

    plane
        .adapter
        .launch(&request)
        .await
        .expect("the first launch spends the reservation");
    let replayed = plane
        .adapter
        .launch(&request)
        .await
        .expect_err("a replayed request finds its reservation spent");
    assert!(matches!(
        replayed,
        RuntimeError::LaunchNotAdmitted { .. } | RuntimeError::SessionAlreadyBound { .. }
    ));
    assert_eq!(
        plane.codex.started(),
        1,
        "the replay started no second Codex in the worktree"
    );

    // And the seat refuses a second admission while it holds a live process.
    let second = plane
        .adapter
        .admit_launch(&AdmissionRequest {
            slot: seat.slot.clone(),
            agent_run_id: AgentRunId::generate(),
            binding_id: RuntimeBindingId::generate(),
            replaces: None,
            requested_at: at("2026-08-10T09:10:00Z"),
        })
        .await
        .expect_err("the seat is filled");
    assert!(matches!(second, RuntimeError::SlotAlreadyAdmitted { .. }));
    assert_eq!(plane.codex.started(), 1);
}

// ---------------------------------------------------------------------------
// Session content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn only_stdout_json_frames_become_session_content() {
    let plane = plane_a(RecordedCodex::new().running(
        CodexScript::acknowledging(ACK).then_printing(&[MESSAGE, TOOL_CALL, DIAGNOSTIC, COMPLETE]),
    ));
    let seat = open_seat(&plane.adapter, "implement").await;
    let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;

    let mut live = plane
        .adapter
        .subscribe_live(&subscribe(&binding, 0))
        .await
        .expect("the process's stdout is the session content");
    let mut delivered = Vec::new();
    while let Some(event) = live.next_event() {
        delivered.push(event.expect("contiguous content"));
    }

    assert_eq!(
        delivered.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            SessionEventKind::Message,
            SessionEventKind::ToolCall,
            SessionEventKind::Log,
            SessionEventKind::StateChange,
        ]
    );
    assert_eq!(
        delivered
            .iter()
            .map(|event| event.position.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "the numbering is contiguous inside one generation"
    );
    // Every accepted frame keeps its raw JSON as canonical evidence, before any
    // mapping this adapter did.
    assert!(delivered[0].payload.json().contains("agent_message"));
    assert!(delivered[0].payload.json().contains("raw_digest"));

    // The launch acknowledgement is evidence, not content: it never appears twice.
    assert!(
        !delivered
            .iter()
            .any(|event| event.payload.json().contains("session_configured"))
    );
}

#[tokio::test]
async fn a_malformed_or_skipped_frame_breaks_the_stream_rather_than_passing_quietly() {
    // Malformed: a line that is not the pinned envelope is a typed failure, never
    // a frame that is quietly skipped.
    let plane = plane_a(
        RecordedCodex::new()
            .running(CodexScript::acknowledging(ACK).then_printing(&[MESSAGE, "not json at all"])),
    );
    let seat = open_seat(&plane.adapter, "implement").await;
    let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;
    assert!(matches!(
        plane
            .adapter
            .subscribe_live(&subscribe(&binding, 0))
            .await
            .expect_err("a malformed frame is refused"),
        RuntimeError::Domain(_)
    ));

    // Skipped: the transport could not keep a line, so the numbering has a hole
    // in it. It is reported as the gap it is, and the break sticks — a later
    // drain must not resume as though nothing happened.
    let dropping = plane_a(
        RecordedCodex::new().running(
            CodexScript::acknowledging(ACK)
                .dropping(2)
                .then_printing(&[MESSAGE])
                .then_printing(&[TOOL_CALL]),
        ),
    );
    let seat = open_seat(&dropping.adapter, "implement").await;
    let (_, binding) = launch(&dropping.adapter, &seat, PROFILE_A).await;
    assert_eq!(
        dropping
            .adapter
            .subscribe_live(&subscribe(&binding, 0))
            .await
            .expect_err("frames the transport could not keep are a gap"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );
    assert_eq!(
        dropping
            .adapter
            .subscribe_live(&subscribe(&binding, 0))
            .await
            .expect_err("and a broken stream stays broken"),
        RuntimeError::TimelineRefetchRequired {
            reason: TimelineBreak::SequenceGap
        }
    );
}

// ---------------------------------------------------------------------------
// Endings, cancellation and the one conclusion nothing walks back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_process_ending_becomes_terminal_evidence() {
    // The sweep that keeps this adapter honest. Every way a Codex process can
    // stop is read through both surfaces that could close a run — a fresh
    // inspect, which is the source a Grade B runtime *is* allowed to close on,
    // and a cancellation acknowledgement — and neither may close anything.
    for ending in CodexEnding::ALL.iter().copied() {
        let plane = plane_a(
            RecordedCodex::new().running(CodexScript::acknowledging(ACK).ending_with(ending)),
        );
        let seat = open_seat(&plane.adapter, "implement").await;
        let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;

        let inspected = plane
            .adapter
            .inspect(&InspectRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:05:00Z"),
            })
            .await
            .expect("a process that ended still inspects");
        assert_eq!(
            inspected.state,
            ObservedRunState::Unknown,
            "{ending:?} is a fact about a process, not a verdict on the work"
        );
        assert_eq!(inspected.contact, RuntimeContact::ProcessMissing);
        assert_eq!(inspected.source, ObservationSource::AdvisoryReport);
        assert_eq!(
            closes(&plane.adapter, &inspected, &binding).await,
            None,
            "{ending:?} closed a run"
        );

        let cancelled = plane
            .adapter
            .cancel(&CancelRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:06:00Z"),
            })
            .await
            .expect("cancelling an ended process is answered, not refused");
        assert_eq!(cancelled.state, ObservedRunState::Unknown);
        assert_eq!(cancelled.source, ObservationSource::CommandAck);
        assert_eq!(closes(&plane.adapter, &cancelled, &binding).await, None);

        // The evidence records what happened without concluding from it.
        assert!(cancelled.evidence.json().contains(ending.as_str()));
    }
}

#[tokio::test]
async fn cancellation_addresses_the_bound_process_and_never_evidences_closure() {
    let plane = plane_a(
        RecordedCodex::new()
            .running(CodexScript::acknowledging(ACK).then_printing(&[MESSAGE, TOOL_CALL])),
    );
    let seat = open_seat(&plane.adapter, "implement").await;

    // Before the process exists there is no binding to cancel, and a fabricated
    // one is not this adapter's.
    let fabricated = forged(
        &config().runtime_kind,
        plane
            .adapter
            .discover_capabilities()
            .await
            .expect("declared capabilities"),
    );
    assert!(matches!(
        plane
            .adapter
            .cancel(&CancelRequest {
                binding: fabricated,
                requested_at: at("2026-08-10T09:00:30Z"),
            })
            .await
            .expect_err("a binding this adapter never issued addresses nothing"),
        RuntimeError::StaleBinding { .. }
    ));
    assert!(plane.codex.calls().is_empty());

    // During output: the process is still writing when the cancellation arrives.
    let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;
    let acknowledged = plane
        .adapter
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:01:00Z"),
        })
        .await
        .expect("a live process is stopped");
    assert_eq!(acknowledged.state, ObservedRunState::Unknown);
    assert_eq!(closes(&plane.adapter, &acknowledged, &binding).await, None);
    assert_eq!(plane.codex.count("codex stop"), 1);

    // After disappearance: a second cancellation is answered from what already
    // happened rather than renaming it, and still closes nothing.
    let again = plane
        .adapter
        .cancel(&CancelRequest {
            binding: binding.clone(),
            requested_at: at("2026-08-10T09:02:00Z"),
        })
        .await
        .expect("cancelling twice is answered");
    assert_eq!(again.state, ObservedRunState::Unknown);
    assert_eq!(closes(&plane.adapter, &again, &binding).await, None);

    // A process that is gone is lost contact, never completion.
    let report = plane
        .adapter
        .reconcile(std::slice::from_ref(&binding))
        .await
        .expect("bindings this adapter issued classify");
    assert_eq!(report.findings.len(), 1);
    assert!(matches!(
        report.findings[0],
        ReconciliationFinding::MissingSession { .. }
    ));
    assert_eq!(
        report.findings[0].action(),
        ReconciliationAction::ProposeLostContactReview
    );
    assert!(report.findings.iter().all(|finding| !matches!(
        finding.proposed_state(),
        Some(kontor_core::state::DerivedRunState::Terminal { .. })
    )));
}

// ---------------------------------------------------------------------------
// Typed refusals and the frozen snapshot
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_unsupported_operation_fails_typed_with_zero_dispatches() {
    let plane = plane_a(one_run_script());
    let seat = open_seat(&plane.adapter, "implement").await;
    let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;
    let declared = plane
        .adapter
        .discover_capabilities()
        .await
        .expect("declared capabilities");
    plane.codex.take_calls();

    let refusals: Vec<(RuntimeCapability, RuntimeError)> = vec![
        (
            RuntimeCapability::Resume,
            plane
                .adapter
                .resume(&ResumeRequest {
                    binding: binding.clone(),
                    requested_at: at("2026-08-10T09:02:00Z"),
                })
                .await
                .expect_err("codex exec is one shot"),
        ),
        (
            RuntimeCapability::SendMessage,
            plane
                .adapter
                .send(&SendMessageRequest {
                    binding: binding.clone(),
                    message_id: MessageId::generate(),
                    body: text("a follow-up nothing can deliver"),
                    sent_at: at("2026-08-10T09:02:00Z"),
                })
                .await
                .expect_err("there is no channel to deliver on"),
        ),
        (
            RuntimeCapability::History,
            drain_history(&plane.adapter, &binding, 2)
                .await
                .expect_err("there is nothing to replay")
                .clone(),
        ),
        (
            RuntimeCapability::PermissionResponse,
            plane
                .adapter
                .respond_permission(&PermissionResponseRequest {
                    binding: binding.clone(),
                    permission_id: ExternalId::parse("codex-permission-1").expect("a valid id"),
                    response_id: MessageId::generate(),
                    decision: PermissionDecision::Allow,
                    responded_at: at("2026-08-10T09:02:00Z"),
                })
                .await
                .expect_err("there is no permission surface to answer"),
        ),
        (
            RuntimeCapability::Adopt,
            plane
                .adapter
                .adopt(&AdoptRequest {
                    agent_run_id: AgentRunId::generate(),
                    binding_id: RuntimeBindingId::generate(),
                    native: binding.identity().clone(),
                    adopted_at: at("2026-08-10T09:02:00Z"),
                })
                .await
                .expect_err("a foreign process carries no Kontor label"),
        ),
        (
            RuntimeCapability::Discovery,
            plane
                .adapter
                .discover_sessions()
                .await
                .expect_err("there is no inventory to enumerate"),
        ),
    ];

    for (capability, error) in refusals {
        assert_eq!(
            error,
            RuntimeError::UnsupportedCapability { capability },
            "an undeclared {capability} must fail as exactly that capability"
        );
        assert!(!declared.supports(capability));
    }
    assert!(
        plane.codex.calls().is_empty(),
        "not one of those reached a process"
    );

    // The refusal table and the declaration agree, in both directions.
    for (capability, _) in UNSUPPORTED {
        assert!(
            !declared.supports(*capability),
            "{capability} is documented unsupported but declared supported"
        );
    }
}

#[tokio::test]
async fn the_capability_snapshot_is_frozen_and_states_what_it_can_prove() {
    let plane = plane_a(one_run_script());
    let declared = plane
        .adapter
        .discover_capabilities()
        .await
        .expect("declared capabilities");
    assert_eq!(declared.trust_grade, TrustGrade::B);
    assert!(
        declared.account_env,
        "the whole reason this adapter exists next to its siblings"
    );
    for capability in [
        RuntimeCapability::PrepareWorkspace,
        RuntimeCapability::Launch,
        RuntimeCapability::Cancel,
        RuntimeCapability::Inspect,
        RuntimeCapability::LiveEvents,
    ] {
        assert!(declared.supports(capability), "{capability}");
    }
    // Discovery of capabilities costs no process: the set is an audited statement
    // about the CLI contract, and probing it would mean spawning something.
    assert!(plane.codex.calls().is_empty());

    let seat = open_seat(&plane.adapter, "implement").await;
    let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;
    assert_eq!(binding.capabilities, declared);

    // A clone with a promoted grade is not the binding this adapter issued, so it
    // vouches for nothing and closes nothing.
    let mut promoted = binding.clone();
    promoted.capabilities.trust_grade = TrustGrade::A;
    assert!(matches!(
        plane
            .adapter
            .issued_binding(&promoted)
            .await
            .expect_err("a promoted clone is not what was issued"),
        RuntimeError::StaleBinding { .. }
    ));
    let report = plane
        .adapter
        .reconcile(std::slice::from_ref(&promoted))
        .await
        .expect("a presented binding is always classified");
    assert!(matches!(
        report.findings[0],
        ReconciliationFinding::Unattested { .. }
    ));
}

#[tokio::test]
async fn a_cross_engine_handoff_prompt_launches_a_fresh_pinned_codex_run() {
    // The fallback this adapter is for: work that started on another engine is
    // handed over as a prompt, and Codex takes it as a *new* run under a proven
    // account. There is no resume and no adopt to reach for — both are refused —
    // so the handoff is a launch or it is nothing.
    let plane = plane_a(one_run_script());
    let seat = open_seat(&plane.adapter, "implement").await;
    let handoff = Parts {
        prompt: "Handoff from the Paseo Implement seat: the migration is written, the tests \
                 are not. Continue from the working tree."
            .to_owned(),
        ..parts(&seat, PROFILE_A)
    };
    let request = admitted(&plane.adapter, &seat, &handoff).await;
    let outcome = plane
        .adapter
        .launch(&request)
        .await
        .expect("a handoff prompt launches a fresh pinned run");

    assert_eq!(outcome.snapshot.agent_run_id(), handoff.agent_run_id);
    assert_eq!(
        outcome.observation.source,
        ObservationSource::CommandAck,
        "a launch acknowledgement is an acknowledgement"
    );
    assert_eq!(
        closes(&plane.adapter, &outcome.observation, &outcome.snapshot).await,
        None
    );
    assert_eq!(
        plane
            .adapter
            .account_receipt(outcome.snapshot.binding_id())
            .expect("the handoff run records its account")
            .account_profile_id,
        profile_id(PROFILE_A)
    );
    assert_eq!(plane.codex.started(), 1);
}

// ---------------------------------------------------------------------------
// The isolation claim, checked against every artefact
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_home_path_prompt_or_auth_canary_reaches_any_artefact() {
    let plane = plane_a(
        RecordedCodex::new()
            .running(CodexScript::acknowledging(ACK).then_printing(&[MESSAGE]))
            .watching_for(AUTH_CANARY_A)
            .watching_for(AUTH_CANARY_B),
    );
    let seat = open_seat(&plane.adapter, "implement").await;
    let (_, binding) = launch(&plane.adapter, &seat, PROFILE_A).await;
    let receipt = plane
        .adapter
        .account_receipt(binding.binding_id())
        .expect("a receipt");

    let home_a = home_path("account-a");
    let home_b = home_path("account-b");
    let worktree = worktree();
    let forbidden = [
        home_a.as_str(),
        home_b.as_str(),
        // The working directory too: it names a place on an operator's machine,
        // and a rendering that carries it is the same class of leak as one that
        // carries the config home.
        worktree.as_str(),
        AUTH_CANARY_A,
        AUTH_CANARY_B,
        PROMPT_CANARY,
    ];

    // Every durable, loggable or printable artefact this adapter produces.
    let artefacts = vec![
        (
            "the account receipt",
            receipt
                .to_document()
                .expect("the receipt canonicalizes")
                .json()
                .to_owned(),
        ),
        (
            "the checkpoint",
            format!("{:?}", plane.adapter.checkpoint()),
        ),
        (
            "the adapter's own rendering",
            format!("{:?}", plane.adapter),
        ),
        (
            "the dispatch ledger",
            format!("{:?}", plane.codex.dispatches()),
        ),
        ("the call ledger", format!("{:?}", plane.codex.calls())),
        ("the binding", format!("{binding:?}")),
        (
            "a refusal",
            format!(
                "{:?}",
                plane
                    .adapter
                    .send(&SendMessageRequest {
                        binding: binding.clone(),
                        message_id: MessageId::generate(),
                        body: text("anything"),
                        sent_at: at("2026-08-10T09:02:00Z"),
                    })
                    .await
                    .expect_err("send is unsupported")
            ),
        ),
    ];
    for (what, rendered) in artefacts {
        for needle in forbidden {
            assert!(
                !rendered.contains(needle),
                "{what} carries something it must not"
            );
        }
    }

    // The one place the home legitimately exists is the environment block of the
    // process that was about to be spawned, and even there only as a name from
    // the outside.
    let dispatches = plane.codex.dispatches();
    assert!(dispatches[0].env_names.contains(&CODEX_HOME.to_owned()));
    assert!(
        plane.codex.leaked_canaries().is_empty(),
        "no credential file content reached the process"
    );

    // And the marker digest is a digest of a file that provably holds nothing
    // but names — which is why it is admissible where a digest of `auth.json`
    // would not be.
    assert_ne!(receipt.marker_digest, ContentHash::of(home_a.as_bytes()));
}

#[tokio::test]
async fn no_debug_rendering_exposes_a_prompt_a_path_or_an_environment_value() {
    // `Debug` is the leak that does not go through the ledger. A derived one
    // renders every field at every `{:?}`, every `tracing` field and every
    // `expect` message, so a type that holds a prompt, a directory or an
    // environment *value* must write its own — and this sweeps every type in the
    // crate that holds one, with a distinct canary per hiding place so a failure
    // names which.
    const PROMPT_TEXT: &str = "kontor-canary-debug-prompt-1a2b3c";
    const CWD_TEXT: &str = "/private/kontor-canary-debug-cwd-4d5e6f";
    const PROGRAM_TEXT: &str = "/opt/kontor-canary-debug-program-7g8h9i";
    const HOME_TEXT: &str = "/private/kontor-canary-debug-home-0j1k2l";
    const STDOUT_TEXT: &str = "kontor-canary-debug-stdout-3m4n5o";

    let command = CodexCommand::exec(
        PROGRAM_TEXT,
        CWD_TEXT,
        PROMPT_TEXT,
        vec![CODEX_HOME.to_owned()],
    );
    let prepared = {
        let mut process = std::process::Command::new(PROGRAM_TEXT);
        process.args(command.argv());
        process.current_dir(CWD_TEXT);
        process.env_remove("KONTOR_CANARY_CLEARED");
        process.env(CODEX_HOME, HOME_TEXT);
        PreparedCommand::new(process)
    };
    let started = CodexStarted {
        exec_id: ExternalId::parse("codex-exec-debug").expect("a valid id"),
        process_id: 4242,
        launch_ack: format!("{{\"raw\":\"{STDOUT_TEXT}\"}}"),
    };
    let drained = CodexDrained {
        lines: vec![STDOUT_TEXT.to_owned(), STDOUT_TEXT.to_owned()],
        dropped: 1,
        ending: Some(CodexEnding::Eof),
    };
    let dispatch = CodexDispatch {
        route: EXEC_ROUTE,
        cwd: CWD_TEXT.to_owned(),
        env_names: vec![CODEX_HOME.to_owned()],
        cleared_names: vec![CODEX_HOME.to_owned()],
        exec_id: ExternalId::parse("codex-exec-debug").expect("a valid id"),
        process_id: 4242,
    };

    // A whole plane, so the composed renderings are covered too: an adapter that
    // redacted its own fields while printing a config, a checkpoint or a fixture
    // that did not would still leak.
    let plane = plane_a(
        RecordedCodex::new()
            .running(CodexScript::acknowledging(ACK).then_printing(&[MESSAGE]))
            .watching_for(AUTH_CANARY_A),
    );
    let seat = open_seat(&plane.adapter, "implement").await;
    launch(&plane.adapter, &seat, PROFILE_A).await;

    let renderings = [
        ("CodexCommand", format!("{command:?}")),
        ("PreparedCommand", format!("{prepared:?}")),
        ("CodexStarted", format!("{started:?}")),
        ("CodexDrained", format!("{drained:?}")),
        ("CodexDispatch", format!("{dispatch:?}")),
        (
            "CodexScript",
            format!("{:?}", CodexScript::acknowledging(ACK)),
        ),
        ("RecordedCodex", format!("{:?}", plane.codex)),
        ("CodexConfig", format!("{:?}", plane.adapter.config())),
        ("CodexAdapter", format!("{:?}", plane.adapter)),
        (
            "CodexCheckpoint",
            format!("{:?}", plane.adapter.checkpoint()),
        ),
        (
            "CodexLiveTransport",
            format!(
                "{:?}",
                CodexLiveTransport::new(30, 900).expect("a configured transport")
            ),
        ),
    ];

    let approved_home = home_path("account-a");
    let verified_worktree = worktree();
    let forbidden = [
        PROMPT_TEXT,
        CWD_TEXT,
        PROGRAM_TEXT,
        HOME_TEXT,
        STDOUT_TEXT,
        PROMPT_CANARY,
        AUTH_CANARY_A,
        approved_home.as_str(),
        verified_worktree.as_str(),
        ACK,
        MESSAGE,
    ];
    for (what, rendered) in &renderings {
        for needle in forbidden {
            assert!(
                !rendered.contains(needle),
                "{what}'s Debug rendering carries something it must not: {rendered}"
            );
        }
    }

    // …and the redaction is a redaction rather than an empty rendering. A `Debug`
    // that printed nothing would pass every assertion above while making the type
    // useless to diagnose with, so the safe half has to still be there.
    assert!(renderings[0].1.contains(EXEC_ROUTE));
    assert!(renderings[0].1.contains("arguments: 3"));
    assert!(renderings[0].1.contains(CODEX_HOME));
    assert!(renderings[1].1.contains(CODEX_HOME));
    assert!(renderings[2].1.contains("4242"));
    assert!(renderings[3].1.contains("dropped: 1"));
    assert!(renderings[4].1.contains(EXEC_ROUTE));
    assert!(renderings[7].1.contains("codex.exec"));
    for (what, rendered) in &renderings {
        assert!(
            rendered.starts_with(what) || rendered.starts_with(&format!("{what} {{")),
            "{what}'s Debug should still name its own type, got {rendered}"
        );
    }

    // The accessors are untouched: a field a test reads deliberately is a
    // different thing from a field a log line prints by accident.
    assert_eq!(command.cwd(), CWD_TEXT);
    assert_eq!(command.program(), PROGRAM_TEXT);
    assert!(command.argv().iter().any(|arg| arg == PROMPT_TEXT));
    assert_eq!(dispatch.cwd, CWD_TEXT);
    // And the values really were in there: the canary probe finds the home and
    // the prompt inside the very command whose `Debug` printed neither, so the
    // redaction is a redaction and not an artefact of an empty fixture.
    assert!(prepared.contains(HOME_TEXT));
    assert!(prepared.contains(PROMPT_TEXT));
    assert!(prepared.contains(CWD_TEXT));
}

// ---------------------------------------------------------------------------
// Context policy and the hermetic app-server compaction lane (TASK-026)
// ---------------------------------------------------------------------------

fn context_policy(
    class: kontor_core::spec::ContextWindowClass,
    scope: kontor_core::spec::ContextTriggerScope,
) -> kontor_core::spec::ContextPolicySnapshot {
    let declared = kontor_core::spec::ContextWindowPolicy {
        class,
        trigger_scope: scope,
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
    let effective = kontor_core::spec::EffectiveContextPolicy::derive(
        &requested,
        &kontor_core::spec::ContextWindowBounds::unknown(),
        true,
    )
    .expect("the effective half derives");
    kontor_core::spec::ContextPolicySnapshot::freeze(
        requested,
        effective,
        at("2026-08-10T09:00:00Z"),
    )
    .expect("both halves freeze")
}

/// The exact `-c key=value` argv a seat's policy produces, asserted as bytes.
#[tokio::test]
async fn a_launch_configures_codex_with_the_exact_auto_compaction_values() {
    let plane = plane_a(RecordedCodex::new());
    let seat = open_seat(&plane.adapter, "compact-a").await;
    let mut parts = parts(&seat, PROFILE_A);
    parts.prompt = PROMPT.to_owned();
    let request = admitted(&plane.adapter, &seat, &parts).await;

    // The frozen policy the seat launches under.
    let policy = context_policy(
        kontor_core::spec::ContextWindowClass::Deep,
        kontor_core::spec::ContextTriggerScope::GrowthAfterPrefix,
    );
    let config = kontor_runtime_codex::wire::auto_compact_config(&policy.effective);
    assert_eq!(
        config,
        vec![
            (
                "model_auto_compact_token_limit".to_owned(),
                "512000".to_owned()
            ),
            (
                "model_auto_compact_token_limit_scope".to_owned(),
                "body_after_prefix".to_owned()
            ),
        ]
    );

    // And the command actually carries them, as separate argv entries before the
    // prompt, so a value can never be read as another option.
    let command = kontor_runtime_codex::client::CodexCommand::exec_with_config(
        "codex",
        request.cwd().as_str(),
        request.prompt().as_str(),
        Vec::new(),
        &config,
    );
    let argv = command.argv();
    assert_eq!(argv[0], "exec");
    assert_eq!(argv[1], "--json");
    assert_eq!(argv[2], "-c");
    assert_eq!(argv[3], "model_auto_compact_token_limit=512000");
    assert_eq!(argv[4], "-c");
    assert_eq!(
        argv[5],
        "model_auto_compact_token_limit_scope=body_after_prefix"
    );
    assert_eq!(argv[6], PROMPT, "the prompt stays last");
}

/// Production advertises no compaction, because production has no app-server.
#[tokio::test]
async fn a_production_adapter_advertises_context_policy_but_not_compact() {
    let plane = plane_a(RecordedCodex::new());
    let declared = plane
        .adapter
        .discover_capabilities()
        .await
        .expect("capabilities are discoverable");
    assert!(declared.supports(RuntimeCapability::ContextPolicy));
    assert!(
        !declared.supports(RuntimeCapability::Compact),
        "the production lane is `codex exec --json` and cannot compact a live thread"
    );
}

/// MUT-CTX-06's Codex half: `Confirmed` only on an unchanged thread id and
/// generation, proved by a fresh re-read rather than by the lifecycle's word.
#[tokio::test]
async fn the_hermetic_app_server_confirms_only_an_unchanged_thread() {
    let app_server = std::sync::Arc::new(kontor_runtime_codex::fixture::FakeAppServer::new(1));
    let plane = plane_a(one_run_script());
    let adapter = plane
        .adapter
        .with_app_server(std::sync::Arc::clone(&app_server)
            as std::sync::Arc<dyn kontor_runtime_codex::adapter::CodexAppServer>);

    let declared = adapter
        .discover_capabilities()
        .await
        .expect("capabilities are discoverable");
    assert!(
        declared.supports(RuntimeCapability::Compact),
        "the hermetic lane is what makes compaction attestable"
    );

    let seat = open_seat(&adapter, "compact-b").await;
    let (_, binding) = launch(&adapter, &seat, PROFILE_A).await;
    let thread = binding.identity().native_id.as_str().to_owned();

    let receipt = adapter
        .compact(&kontor_runtime::request::CompactRequest {
            binding: binding.clone(),
            receipt_id: kontor_core::id::CompactionReceiptId::generate(),
            trigger: kontor_core::compaction::CompactionTrigger::ScopeBoundary,
            policy: context_policy(
                kontor_core::spec::ContextWindowClass::Standard,
                kontor_core::spec::ContextTriggerScope::GrowthAfterPrefix,
            ),
            context_pack_hash: kontor_core::id::ContentHash::of(b"context-pack"),
            handoff_hash: Some(kontor_core::id::ContentHash::of(b"sealed-handoff")),
            requested_at: at("2026-08-10T09:30:00Z"),
        })
        .await
        .expect("the hermetic app-server compacts");

    assert_eq!(
        receipt.status,
        kontor_core::compaction::CompactionStatus::Confirmed
    );
    assert!(receipt.preserves_native_identity());
    receipt.validate().expect("a confirmed receipt validates");
    // Only what the runtime reported. Nothing invented a "before" count.
    assert_eq!(receipt.telemetry.tokens_after, Some(42_000));
    assert_eq!(receipt.telemetry.tokens_before, None);
    assert_eq!(receipt.telemetry.cache_read_tokens, None);

    // The exact wire messages: the approved method, the exact body, and a fresh
    // re-read afterwards.
    let calls = app_server.calls();
    assert_eq!(calls[0].0, "thread/compact/start");
    assert_eq!(calls[0].1, format!("{{\"threadId\":\"{thread}\"}}"));
    assert_eq!(calls[1].0, "thread/get");
}

/// A thread that re-reads in another generation was replaced, not compacted.
#[tokio::test]
async fn a_drifted_thread_is_failed_and_never_adopted_as_a_successor() {
    let app_server = std::sync::Arc::new(kontor_runtime_codex::fixture::FakeAppServer::new(1));
    let plane = plane_a(one_run_script());
    let adapter = plane
        .adapter
        .with_app_server(std::sync::Arc::clone(&app_server)
            as std::sync::Arc<dyn kontor_runtime_codex::adapter::CodexAppServer>);

    let seat = open_seat(&adapter, "compact-c").await;
    let (_, binding) = launch(&adapter, &seat, PROFILE_A).await;
    let thread = binding.identity().native_id.as_str().to_owned();
    app_server.drift_to(kontor_runtime_codex::wire::ThreadIdentity {
        thread_id: thread,
        // The lifecycle still says "completed"; the generation says otherwise.
        generation: 7,
    });

    let receipt = adapter
        .compact(&kontor_runtime::request::CompactRequest {
            binding: binding.clone(),
            receipt_id: kontor_core::id::CompactionReceiptId::generate(),
            trigger: kontor_core::compaction::CompactionTrigger::Operator,
            policy: context_policy(
                kontor_core::spec::ContextWindowClass::Standard,
                kontor_core::spec::ContextTriggerScope::GrowthAfterPrefix,
            ),
            context_pack_hash: kontor_core::id::ContentHash::of(b"context-pack"),
            handoff_hash: Some(kontor_core::id::ContentHash::of(b"sealed-handoff")),
            requested_at: at("2026-08-10T09:30:00Z"),
        })
        .await
        .expect("the attempt still returns a receipt");

    assert_eq!(
        receipt.status,
        kontor_core::compaction::CompactionStatus::Failed
    );
    assert!(!receipt.preserves_native_identity());
    assert!(
        receipt.evidence.is_none(),
        "a failed attempt cites no confirmation evidence"
    );
}
