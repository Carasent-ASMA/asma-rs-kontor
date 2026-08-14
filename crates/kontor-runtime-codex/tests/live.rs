//! An opt-in smoke test against two real, authenticated Codex accounts.
//!
//! Ignored by default, and it skips with a precise reason when its environment is
//! absent — because the alternative is worse than no coverage: a live test that
//! silently passes when it could not run tells you the integration works when
//! nothing was checked.
//!
//! # What it proves that a recording cannot
//!
//! Exactly one thing, and it is the thing this whole adapter exists for: that two
//! concurrent runs pinned to two different accounts really do execute under two
//! different `CODEX_HOME` directories, against a real `codex` binary and a real
//! operating-system process. Everything else — the refusals, the endings, the
//! continuity, the redaction — is proved against recordings in `contract.rs`,
//! where every ordering can be scripted.
//!
//! # Only ever against disposable identities and a disposable tree
//!
//! The prompt is bounded, read-only and instantly answerable, the working tree is
//! one the operator nominates, and both processes are killed by positive process
//! identity when the test finishes. Nothing here is left running.
//!
//! ```bash
//! KONTOR_CODEX_LIVE=1 \
//! KONTOR_CODEX_HOME_A=/path/to/approved/home-a \
//! KONTOR_CODEX_HOME_B=/path/to/approved/home-b \
//! KONTOR_CODEX_WORKTREE=/path/to/disposable/worktree \
//! KONTOR_CODEX_EXECUTABLE=codex \
//! cargo test -p kontor-runtime-codex --test live -- --ignored --nocapture
//! ```
//!
//! Each home must be authenticated for Codex *and* carry the operator's
//! `kontor-profile.json` marker, exactly as production requires. The test reads
//! the two markers to learn which accounts it is talking about; it never writes
//! one, and it never opens any other file inside either home.

use std::path::{Path, PathBuf};

use kontor_accounts::{
    AccountEnvironmentMap, AccountLaunchReceipt, AccountResolver, AdmittedLaunch, KeychainBackend,
    KeychainFailure, KeychainTarget, LaunchRefusal, ResolverPolicy,
};
use kontor_core::id::{
    AgentRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash, CredentialAlias,
    EnvironmentVariableName, ExternalId, ExternalName, ProjectId, RealmId, RoleSlotId,
    RuntimeBindingId, RuntimeKindKey, SCHEMA_VERSION, TaskId, TeamRunId,
};
use kontor_core::repository::{AccountProfile, CredentialReference, CredentialReferenceKind};
use kontor_core::state::ObservedRunState;
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{OperationContext, RuntimeCapability, preflight};
use kontor_runtime::request::{CancelRequest, InspectRequest, LaunchParts};
use kontor_runtime::workspace::{WorkspaceBindingId, WorkspacePrepareRequest, WorkspaceRoot};
use kontor_runtime_codex::adapter::{
    CodexAccountAdmission, CodexAccountAuthority, CodexAccountRequest, CodexAdapter,
    CodexCheckpoint, CodexConfig,
};
use kontor_runtime_codex::client::CodexLiveTransport;
use kontor_runtime_codex::wire::{CODEX_HOME, CodexHomeMarker, MARKER_FILE_NAME};
use kontor_tests_contract::{at, closes};

/// A bounded, read-only instruction that any authenticated Codex answers at once.
const PROMPT: &str = "Reply with the single word OK and stop. Do not read or modify any files.";

/// Why a live run could not happen, stated precisely rather than as a silent
/// pass.
struct Skip(&'static str);

struct Live {
    executable: String,
    homes: [PathBuf; 2],
    worktree: WorkspaceRoot,
}

fn gate() -> Result<Live, Skip> {
    if std::env::var("KONTOR_CODEX_LIVE").as_deref() != Ok("1") {
        return Err(Skip("KONTOR_CODEX_LIVE is not 1"));
    }
    let home_a =
        std::env::var("KONTOR_CODEX_HOME_A").map_err(|_| Skip("KONTOR_CODEX_HOME_A is unset"))?;
    let home_b =
        std::env::var("KONTOR_CODEX_HOME_B").map_err(|_| Skip("KONTOR_CODEX_HOME_B is unset"))?;
    if home_a == home_b {
        // Two runs under one home would pass every assertion below while proving
        // the opposite of what this test is for.
        return Err(Skip("the two approved homes are the same directory"));
    }
    let worktree = std::env::var("KONTOR_CODEX_WORKTREE")
        .map_err(|_| Skip("KONTOR_CODEX_WORKTREE is unset"))?;
    let worktree = std::fs::canonicalize(&worktree)
        .map_err(|_| Skip("KONTOR_CODEX_WORKTREE is not a directory"))?;
    let worktree = WorkspaceRoot::parse(&worktree.to_string_lossy())
        .map_err(|_| Skip("KONTOR_CODEX_WORKTREE is not a normalized absolute path"))?;
    for home in [&home_a, &home_b] {
        if !Path::new(home).join(MARKER_FILE_NAME).is_file() {
            return Err(Skip(
                "an approved home carries no kontor-profile.json marker",
            ));
        }
    }
    Ok(Live {
        executable: std::env::var("KONTOR_CODEX_EXECUTABLE").unwrap_or_else(|_| "codex".to_owned()),
        homes: [PathBuf::from(home_a), PathBuf::from(home_b)],
        worktree,
    })
}

/// A keychain that refuses everything: this adapter's isolation model is a config
/// home, and a keychain lookup would mean a profile it cannot prove.
struct NoKeychain;

impl KeychainBackend for NoKeychain {
    fn secret(&self, _target: &KeychainTarget) -> Result<secrecy::SecretString, KeychainFailure> {
        Err(KeychainFailure::NotFound)
    }
}

fn harness() -> RuntimeKindKey {
    RuntimeKindKey::parse("codex.exec").expect("a valid runtime key")
}

fn empty_document() -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 1 }))
        .expect("a canonical document")
}

/// Read one approved home's marker, and build the account profile it describes.
///
/// The profile is derived from the operator's own marker rather than invented, so
/// the two agree by construction and the adapter's check is about the *home*
/// rather than about this test's bookkeeping.
fn profile_from_marker(home: &Path, alias: &CredentialAlias) -> (AccountProfile, CodexHomeMarker) {
    let raw = std::fs::read_to_string(home.join(MARKER_FILE_NAME)).expect("the marker is readable");
    let marker = CodexHomeMarker::parse(&raw).expect("the marker is the non-secret Kontor marker");
    let environment = AccountEnvironmentMap::new().with(
        EnvironmentVariableName::parse(CODEX_HOME).expect("a valid variable name"),
        CredentialReference {
            kind: CredentialReferenceKind::ConfigHome,
            alias: alias.clone(),
        },
    );
    let profile = AccountProfile {
        id: marker.account_profile_id,
        project_id: ProjectId::generate(),
        label: ExternalName::parse("codex live account").expect("a valid label"),
        external_account_id: None,
        harness: harness(),
        credential_ref: CredentialReference {
            kind: CredentialReferenceKind::ConfigHome,
            alias: alias.clone(),
        },
        environment: environment
            .to_document()
            .expect("a canonical environment map"),
        routing: empty_document(),
        capability: empty_document(),
        provider_identity: Some(marker.provider_identity.clone()),
        enabled: true,
        revision: AggregateRevision::INITIAL,
        created_at: at("2026-08-10T08:00:00Z"),
        updated_at: at("2026-08-10T08:00:00Z"),
    };
    (profile, marker)
}

/// The live authority: a real policy, a real resolver, and one profile.
///
/// It stands in for the store half of
/// [`kontor_runtime_codex::adapter::CodexPinnedAccounts`], which needs a Kontor
/// database this smoke test has no business creating. What is real here is the
/// part the live run is about: the policy approves the home, and the resolver
/// resolves it.
struct LiveAccounts {
    policy: ResolverPolicy,
    keychain: NoKeychain,
    profile: AccountProfile,
}

impl CodexAccountAuthority for LiveAccounts {
    fn admit(
        &self,
        request: &CodexAccountRequest<'_>,
    ) -> Result<CodexAccountAdmission, LaunchRefusal> {
        if request.account_profile_id != self.profile.id {
            return Err(LaunchRefusal::PinMismatch);
        }
        let mut context = OperationContext::new(RuntimeCapability::Launch);
        context.account_pinned = true;
        preflight(request.capabilities, &context).map_err(LaunchRefusal::Runtime)?;
        let resolver = AccountResolver::new(&self.policy, &self.keychain);
        let environment = resolver
            .resolve(&self.profile)
            .map_err(LaunchRefusal::Resolution)?;
        let receipt = AccountLaunchReceipt {
            schema_version: SCHEMA_VERSION,
            realm_id: RealmId::generate(),
            project_id: self.profile.project_id,
            agent_run_id: request.agent_run_id,
            account_profile_id: self.profile.id,
            account_profile_revision: self.profile.revision,
            harness: self.profile.harness.clone(),
            provider_identity: self.profile.provider_identity.clone(),
            environment_names: environment.names(),
            policy_evidence: self.policy.evidence(),
            availability_evidence: ExternalId::parse("live-smoke").expect("a valid id"),
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
            credential_alias: self.profile.credential_ref.alias.clone(),
        })
    }
}

fn adapter(live: &Live, home: &Path, slot: &str) -> (CodexAdapter<'static>, AccountProfile) {
    let alias = CredentialAlias::parse(&format!("codex-live-{slot}")).expect("a valid alias");
    let (profile, _) = profile_from_marker(home, &alias);
    let policy = ResolverPolicy::builder()
        .harness(harness())
        .config_home(alias, home)
        .expect("the approved home is a readable directory")
        .environment(EnvironmentVariableName::parse(CODEX_HOME).expect("a valid variable name"))
        .build();
    let adapter = CodexAdapter::new(
        CodexConfig {
            runtime_kind: harness(),
            host_key: ExternalName::parse("codex-live").expect("a valid host key"),
            executable: live.executable.clone(),
            task_worktree: live.worktree.clone(),
            max_concurrent_sessions: 2,
        },
        Box::new(CodexLiveTransport::new(60, 120).expect("the live transport is configured")),
        Box::new(LiveAccounts {
            policy,
            keychain: NoKeychain,
            profile: profile.clone(),
        }),
        CodexCheckpoint::fresh(1),
    );
    (adapter, profile)
}

/// Launch one bounded run and hand back everything needed to judge and stop it.
async fn run_one(
    adapter: &CodexAdapter<'_>,
    profile: &AccountProfile,
    slot: &str,
) -> kontor_runtime::capability::RuntimeBindingSnapshot {
    let team_run_id = TeamRunId::generate();
    let task_id = TaskId::generate();
    let workspace = adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id,
            task_id,
            workspace_binding_id: WorkspaceBindingId::generate(),
            root: adapter.config().task_worktree.clone(),
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("the disposable worktree verifies")
        .snapshot;
    let agent_run_id = AgentRunId::generate();
    let binding_id = RuntimeBindingId::generate();
    let role_slot_id = RoleSlotId::parse(slot).expect("a valid slot id");
    let request = adapter
        .admit_launch(&AdmissionRequest {
            slot: RoleSlotKey::new(team_run_id, role_slot_id.clone()),
            agent_run_id,
            binding_id,
            replaces: None,
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("a vacant seat admits one launch")
        .into_authority()
        .expect("the seat was free")
        .into_request(LaunchParts {
            agent_run_id,
            team_run_id,
            role_slot_id,
            task_id,
            binding_id,
            workspace: Some(workspace),
            cwd: adapter.config().task_worktree.clone(),
            account_profile_id: Some(profile.id),
            prompt: BoundedText::parse(PROMPT).expect("bounded text"),
            model_rung: kontor_core::spec::ModelRung {
                provider: kontor_core::spec::ProviderRef("codex".to_owned()),
                model: kontor_core::spec::ModelRef("gpt-5.6-sol".to_owned()),
                effort: None,
            },
            context_policy: kontor_core::spec::ContextPolicySnapshot::standard(
                &kontor_core::spec::ContextWindowBounds::unknown(),
                true,
                kontor_core::id::SCHEMA_VERSION,
                at("2026-08-10T09:00:00Z"),
            )
            .expect("the standard fallback freezes"),
            requested_at: at("2026-08-10T09:00:00Z"),
        });
    adapter
        .launch(&request)
        .await
        .expect("a real codex acknowledges its launch")
        .snapshot
}

#[tokio::test]
#[ignore = "requires two authenticated Codex accounts and a disposable worktree"]
async fn two_pinned_accounts_run_concurrently_and_neither_ending_closes_a_run() {
    let live = match gate() {
        Ok(live) => live,
        Err(Skip(reason)) => {
            eprintln!("skipping the Codex live smoke: {reason}");
            return;
        }
    };

    let (adapter_a, profile_a) = adapter(&live, &live.homes[0], "account-a");
    let (adapter_b, profile_b) = adapter(&live, &live.homes[1], "account-b");

    // Concurrently, because "one at a time" would not exercise the thing that
    // actually goes wrong: two processes resolving two homes at once.
    let (binding_a, binding_b) = tokio::join!(
        run_one(&adapter_a, &profile_a, "account-a"),
        run_one(&adapter_b, &profile_b, "account-b"),
    );

    // Attribution: each run recorded the account it actually executed as, and the
    // two are different accounts under different homes.
    let receipt_a = adapter_a
        .account_receipt(binding_a.binding_id())
        .expect("account A recorded its receipt");
    let receipt_b = adapter_b
        .account_receipt(binding_b.binding_id())
        .expect("account B recorded its receipt");
    assert_eq!(receipt_a.account_profile_id, profile_a.id);
    assert_eq!(receipt_b.account_profile_id, profile_b.id);
    assert_ne!(
        receipt_a.account_profile_id, receipt_b.account_profile_id,
        "two live runs under one account prove nothing this test is for"
    );
    assert_ne!(receipt_a.marker_digest, receipt_b.marker_digest);
    assert_ne!(receipt_a.provider_identity, receipt_b.provider_identity);

    // Cancel by positive process identity — the handle this adapter issued for
    // the process it started — and clean both up, whatever they were doing.
    for (adapter, binding) in [(&adapter_a, &binding_a), (&adapter_b, &binding_b)] {
        let cancelled = adapter
            .cancel(&CancelRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:01:00Z"),
            })
            .await
            .expect("the process this binding names is stopped");
        assert_eq!(
            cancelled.state,
            ObservedRunState::Unknown,
            "a real process stopping is still not a verdict on the work"
        );
        assert_eq!(closes(adapter, &cancelled, binding).await, None);

        let inspected = adapter
            .inspect(&InspectRequest {
                binding: binding.clone(),
                requested_at: at("2026-08-10T09:02:00Z"),
            })
            .await
            .expect("a stopped process still inspects");
        assert_eq!(inspected.state, ObservedRunState::Unknown);
        assert_eq!(
            closes(adapter, &inspected, binding).await,
            None,
            "no real ending closes a run either"
        );
    }
}
