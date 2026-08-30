//! Account profile CRUD, credential isolation, launch admission and failover.
//!
//! Every test here runs against the *real* store and a real database file. An
//! in-memory stub would prove nothing about the one claim this crate makes —
//! that no credential material reaches persisted bytes — because the bytes are
//! the thing under test.
//!
//! The mutants this suite exists to kill:
//!
//! * dropping the revision comparison from a profile update or delete;
//! * falling back to a default credential home when an alias is unapproved;
//! * resolving a disabled, cooling or unconfirmed profile;
//! * letting a request name an account the run is not pinned to;
//! * serializing, logging or `Debug`-printing resolved material;
//! * putting a token or a resolved path in a child's argv;
//! * mutating the predecessor's binding or account during a failover;
//! * creating a second successor when a failover is retried.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

#[cfg(target_os = "macos")]
use kontor_accounts::SystemKeychain;
use kontor_accounts::{
    AccountAvailability, AccountEnvironmentMap, AccountProfileDraft, AccountResolver,
    AccountService, AvailabilityObservation, FailoverReason, FailoverRefusal, FailoverRequest,
    KeychainBackend, KeychainFailure, KeychainTarget, LaunchAdmissionRequest, LaunchRefusal,
    PolicyError, ResolutionReason, ResolverPolicy, ResolverPolicyBuilder, admit_pinned_launch,
    fail_over_to_new_run,
};
use kontor_core::DomainError;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, CredentialAlias,
    EnvironmentVariableName, ExternalId, ExternalName, IdempotencyKey, ProjectId, RealmId,
    RuntimeKindKey, SCHEMA_VERSION, SpecVersion, TaskId, TeamRunId, TeamTemplateId, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::repository::{
    AccountProfile, AccountProfileUpdate, CredentialReference, CredentialReferenceKind,
    NewAgentRun, NewProject, NewTask, NewTeamRun, ProjectRepository, RealmRepository,
    RepositoryError, RunRepository, RuntimeBinding, SpecRepository,
};
use kontor_core::spec::{TeamRunSnapshot, TeamTemplateRevision};
use kontor_core::state::{
    Freshness, NativeRuntimeIdentity, ObservedRunState, RuntimeContact, TaskState,
    TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
};
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_store::SqliteStore;
use secrecy::SecretString;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Canaries
// ---------------------------------------------------------------------------

/// The contents of the alpha fixture home's fake auth file. Kontor never opens
/// that file, so this string must not exist anywhere in this process.
const ALPHA_CONFIG_CANARY: &str = "kontor-canary-alpha-confighome-9f3a1c";
/// The beta fixture home's independent canary.
const BETA_CONFIG_CANARY: &str = "kontor-canary-beta-confighome-2d7e40";
/// What the fake keychain hands back for alpha. It reaches exactly one child
/// process environment and nothing else.
const ALPHA_KEYCHAIN_CANARY: &str = "kontor-canary-alpha-keychain-51bd88";
/// Beta's independent keychain canary.
const BETA_KEYCHAIN_CANARY: &str = "kontor-canary-beta-keychain-e07c12";
/// The keychain service the policy approves. Identifying, so it must not appear
/// in any artefact either.
const KEYCHAIN_SERVICE: &str = "kontor-canary-service-a41f";
/// The keychain account name. Same rule.
const KEYCHAIN_ACCOUNT_ALPHA: &str = "kontor-canary-account-alpha-77c2";
const KEYCHAIN_ACCOUNT_BETA: &str = "kontor-canary-account-beta-31e9";

/// Every string that must never appear in a persisted, serialized, logged or
/// argv artefact.
fn canaries() -> Vec<&'static str> {
    vec![
        ALPHA_CONFIG_CANARY,
        BETA_CONFIG_CANARY,
        ALPHA_KEYCHAIN_CANARY,
        BETA_KEYCHAIN_CANARY,
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT_ALPHA,
        KEYCHAIN_ACCOUNT_BETA,
    ]
}

// ---------------------------------------------------------------------------
// Fake keychain
// ---------------------------------------------------------------------------

/// A keychain that records every lookup, so a test can prove a refusal happened
/// *before* any backend access rather than after one that simply failed.
///
/// It never touches the developer's real keychain.
struct FakeKeychain {
    entries: Vec<(KeychainTarget, SecretString)>,
    lookups: AtomicUsize,
    /// Forces two concurrent resolutions to overlap inside the backend.
    barrier: Option<Arc<Barrier>>,
    /// Runs once, inside the lookup, so a test can change the world mid-resolution.
    interceptor: Option<Box<dyn Fn() + Send + Sync>>,
    failure: Option<KeychainFailure>,
}

impl FakeKeychain {
    fn new() -> Self {
        Self {
            entries: vec![
                (
                    KeychainTarget::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT_ALPHA),
                    SecretString::from(ALPHA_KEYCHAIN_CANARY.to_owned()),
                ),
                (
                    KeychainTarget::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT_BETA),
                    SecretString::from(BETA_KEYCHAIN_CANARY.to_owned()),
                ),
            ],
            lookups: AtomicUsize::new(0),
            barrier: None,
            interceptor: None,
            failure: None,
        }
    }

    fn failing(failure: KeychainFailure) -> Self {
        Self {
            failure: Some(failure),
            ..Self::new()
        }
    }

    fn with_barrier(barrier: Arc<Barrier>) -> Self {
        Self {
            barrier: Some(barrier),
            ..Self::new()
        }
    }

    fn intercepting(interceptor: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            interceptor: Some(Box::new(interceptor)),
            ..Self::new()
        }
    }

    fn lookups(&self) -> usize {
        self.lookups.load(Ordering::SeqCst)
    }
}

impl KeychainBackend for FakeKeychain {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.barrier {
            // Neither resolution can finish until both are inside the backend,
            // so any shared mutable state between them would be observable.
            barrier.wait();
        }
        if let Some(interceptor) = &self.interceptor {
            interceptor();
        }
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        self.entries
            .iter()
            .find(|(candidate, _)| {
                candidate.service() == target.service() && candidate.account() == target.account()
            })
            .map(|(_, secret)| secret.clone())
            .ok_or(KeychainFailure::NotFound)
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical timestamp")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn alias(text: &str) -> CredentialAlias {
    CredentialAlias::parse(text).expect("a valid alias")
}

fn env_name(text: &str) -> EnvironmentVariableName {
    EnvironmentVariableName::parse(text).expect("a valid environment variable name")
}

fn harness() -> RuntimeKindKey {
    RuntimeKindKey::parse("zz.codex").expect("a valid runtime key")
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

fn fixture_home(which: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config-homes")
        .join(which)
}

/// Everything a runtime that *can* prove an account environment declares.
fn capabilities(account_env: bool) -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env,
        limits: RuntimeLimits {
            max_message_bytes: 4_096,
            max_history_page: 50,
            max_concurrent_sessions: 4,
            context_window: kontor_core::spec::ContextWindowBounds::unknown(),
        },
    }
}

/// A fully populated builder, stopped one call short of `build()`.
///
/// The half-built policy is scanned alongside the finished one: it holds the
/// same approved paths and keychain targets, so it is exactly as capable of
/// leaking them, and it is the value a composition-time log line is most likely
/// to be holding.
fn policy_builder() -> ResolverPolicyBuilder {
    ResolverPolicy::builder()
        .harness(harness())
        .config_home(alias("alpha-home"), &fixture_home("alpha"))
        .expect("the alpha fixture home is approved")
        .config_home(alias("beta-home"), &fixture_home("beta"))
        .expect("the beta fixture home is approved")
        .keychain(
            alias("alpha-token"),
            KeychainTarget::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT_ALPHA),
        )
        .expect("the alpha keychain entry is approved")
        .keychain(
            alias("beta-token"),
            KeychainTarget::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT_BETA),
        )
        .expect("the beta keychain entry is approved")
        .environment(env_name("ZZ_CODEX_HOME"))
        .environment(env_name("ZZ_PROVIDER_CREDENTIAL"))
}

fn policy() -> ResolverPolicy {
    policy_builder().build()
}

/// Every rendering of the resolver's own types, in one string.
///
/// Both the builder and the finished policy are here. A redaction that is
/// correct on one and forgotten on the other is the defect this exists to
/// catch — the finished policy is the type everyone thinks to check, and the
/// builder is the one that actually got shipped leaking.
fn resolver_renderings(policy: &ResolverPolicy, builder: &ResolverPolicyBuilder) -> String {
    let empty = ResolverPolicy::builder();
    format!("{policy:?} {builder:?} {empty:?} {:?}", policy.evidence())
}

/// A profile that delivers both kinds of reference, so every resolution touches
/// the config-home branch *and* the keychain backend exactly once.
fn draft(project_id: ProjectId, label: &str, side: &str) -> AccountProfileDraft {
    AccountProfileDraft {
        project_id,
        label: name(label),
        harness: harness(),
        credential_ref: CredentialReference {
            kind: CredentialReferenceKind::ConfigHome,
            alias: alias(&format!("{side}-home")),
        },
        environment: AccountEnvironmentMap::new()
            .with(
                env_name("ZZ_CODEX_HOME"),
                CredentialReference {
                    kind: CredentialReferenceKind::ConfigHome,
                    alias: alias(&format!("{side}-home")),
                },
            )
            .with(
                env_name("ZZ_PROVIDER_CREDENTIAL"),
                CredentialReference {
                    kind: CredentialReferenceKind::Keychain,
                    alias: alias(&format!("{side}-token")),
                },
            ),
        routing: document("routing"),
        capability: document("capability"),
        external_account_id: Some(
            ExternalId::parse(&format!("acct-{side}")).expect("a valid external id"),
        ),
        provider_identity: Some(
            ExternalId::parse(&format!("provider-{side}")).expect("a valid external id"),
        ),
    }
}

struct Fixture {
    _directory: TempDir,
    path: PathBuf,
    store: SqliteStore,
    project: ProjectId,
    other_project: ProjectId,
    team_run: TeamRunId,
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");

    let project = ProjectId::generate();
    let other_project = ProjectId::generate();
    for (id, suffix) in [(project, "a"), (other_project, "b")] {
        store
            .create_project(&NewProject {
                id,
                name: name(&format!("Project {suffix}")),
                root_path: name(&format!("/tmp/project-{suffix}")),
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("the project is created");
    }

    let task = TaskId::generate();
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: None,
            title: name("A task"),
            module: None,
            state: TaskState::Ready,
            created_at: at("2026-08-10T09:00:00Z"),
        })
        .expect("the task is created");

    let template = TeamTemplateId::generate();
    store
        .insert_team_template(
            project,
            &TeamTemplateRevision {
                template_id: template,
                version: SpecVersion::FIRST,
                name: name("Team"),
                definition: document("team"),
                role_authority: Vec::new(),
            },
        )
        .expect("the team revision is stored");
    let revision = store
        .get_team_template(project, template, SpecVersion::FIRST)
        .expect("the read succeeds")
        .expect("the revision exists");
    let team_run = TeamRunId::generate();
    store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: project,
            task_id: task,
            snapshot: TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION),
            created_at: at("2026-08-10T09:00:00Z"),
        })
        .expect("the team run is created");

    Fixture {
        _directory: directory,
        path,
        store,
        project,
        other_project,
        team_run,
    }
}

impl Fixture {
    fn service(&self) -> AccountService<'_, SqliteStore> {
        AccountService::new(&self.store)
    }

    fn profile(&self, label: &str, side: &str) -> AccountProfile {
        self.service()
            .create(
                AccountProfileId::generate(),
                &draft(self.project, label, side),
                at("2026-08-10T09:00:00Z"),
            )
            .expect("the profile is created")
    }

    fn identity(&self, native: &str) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: harness(),
            host: name("host-1"),
            generation: 1,
            native_id: ExternalId::parse(native).expect("a valid native id"),
        }
    }

    fn run(&self, account: Option<AccountProfileId>, bound: Option<&str>) -> AgentRunId {
        let id = AgentRunId::generate();
        self.store
            .create_agent_run(&NewAgentRun {
                id,
                project_id: self.project,
                team_run_id: self.team_run,
                parent_agent_run_id: None,
                role: kontor_core::id::RoleKey::parse("zz.maker").expect("a valid role"),
                account_profile_id: account,
                binding: bound.map(|native| RuntimeBinding {
                    id: kontor_core::id::RuntimeBindingId::generate(),
                    agent_run_id: id,
                    identity: self.identity(native),
                    bound_at: at("2026-08-10T09:00:00Z"),
                }),
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("the run is created");
        id
    }

    /// Close a run with real runtime-observation evidence, which is the only
    /// route to a terminal run that a failover will accept.
    fn close(&self, run: AgentRunId, native: &str) {
        let current = self
            .store
            .get_agent_run(self.project, run)
            .expect("the read succeeds")
            .expect("the run exists");
        self.store
            .record_observation(&kontor_core::repository::NewObservation {
                event: kontor_core::repository::NewRuntimeEvent {
                    project_id: self.project,
                    agent_run_id: run,
                    identity: self.identity(native),
                    native_event_id: Some(
                        ExternalId::parse(&format!("{native}-terminal")).expect("a valid id"),
                    ),
                    native_sequence: 1,
                    payload: document("terminal"),
                    observed_at: at("2026-08-10T09:30:00Z"),
                },
                observed: ObservedRunState::Failed,
                contact: RuntimeContact::Reachable,
                freshness: Freshness::Fresh,
                expected_revision: current.revision,
            })
            .expect("the terminal observation is recorded");
        let stored = self
            .store
            .read_runtime_events(self.project, run, None)
            .expect("the read succeeds")
            .into_iter()
            .next_back()
            .expect("the event exists");
        let current = self
            .store
            .get_agent_run(self.project, run)
            .expect("the read succeeds")
            .expect("the run exists");
        self.store
            .close_agent_run(&kontor_core::repository::RunClosure {
                project_id: self.project,
                agent_run_id: run,
                expected_revision: current.revision,
                evidence: TerminalEvidence {
                    outcome: TerminalOutcome::Failed,
                    source: TerminalEvidenceSource::RuntimeObservation {
                        cursor: stored.cursor,
                    },
                    evidence_hash: stored.payload.hash().clone(),
                    closed_at: at("2026-08-10T09:31:00Z"),
                },
            })
            .expect("the run closes");
    }
}

fn available(profile: &AccountProfile) -> AvailabilityObservation {
    AvailabilityObservation {
        profile_id: profile.id,
        availability: AccountAvailability::Available,
        observed_at: at("2026-08-10T10:00:00Z"),
        evidence: ExternalId::parse("fleet-observation-1").expect("a valid id"),
    }
}

const NOW: &str = "2026-08-10T10:00:30Z";

fn admission<'a>(
    fixture: &Fixture,
    run: AgentRunId,
    profile: &AccountProfile,
    observation: &'a AvailabilityObservation,
    capabilities: &'a RuntimeCapabilities,
) -> LaunchAdmissionRequest<'a> {
    LaunchAdmissionRequest {
        realm_id: fixture.store.realm_id(),
        project_id: fixture.project,
        agent_run_id: run,
        account_profile_id: profile.id,
        observation,
        capabilities,
        now: at(NOW),
    }
}

// ---------------------------------------------------------------------------
// Phase 1 — profile CRUD
// ---------------------------------------------------------------------------

#[test]
fn profile_crud_round_trips_without_credential_material() {
    let fixture = fixture();
    let created = fixture.profile("Alpha", "alpha");
    assert_eq!(created.revision, AggregateRevision::INITIAL);
    assert!(created.enabled);

    // The stored reference is an alias and a kind. Nothing about it says where
    // the credential is.
    assert_eq!(created.credential_ref.alias.as_str(), "alpha-home");
    assert_eq!(
        created.credential_ref.kind,
        CredentialReferenceKind::ConfigHome
    );

    // Reopening proves the fields are on disk. The temporary directory stays
    // alive; only the connection is dropped.
    let Fixture {
        _directory,
        path,
        store: first,
        project,
        ..
    } = fixture;
    drop(first);
    let store = SqliteStore::open(&path).expect("the store reopens");
    let service = AccountService::new(&store);
    let loaded = service
        .get(project, created.id)
        .expect("the read succeeds")
        .expect("the profile survives a reopen");
    assert_eq!(loaded, created);

    // The whole persisted record, serialized, contains ids and aliases only.
    let json = serde_json::to_string(&loaded).expect("the projection serializes");
    assert!(json.contains(&created.id.to_string()));
    assert!(json.contains("alpha-home"));
    for canary in canaries() {
        assert!(
            !json.contains(canary),
            "the public projection must not contain a canary"
        );
    }
    assert!(
        !json.contains(&fixture_home("alpha").to_string_lossy().into_owned()),
        "the public projection must not contain a resolved home path"
    );
}

#[test]
fn profile_updates_use_compare_and_swap() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let service = fixture.service();

    let renamed = service
        .rename(&profile, name("Alpha renamed"), at(NOW))
        .expect("the first update succeeds");
    assert_eq!(renamed.revision.get(), 2);
    assert_eq!(renamed.label.as_str(), "Alpha renamed");

    // The caller still holding revision 1 loses, and writes nothing.
    let error = service
        .rename(&profile, name("Alpha again"), at(NOW))
        .expect_err("a stale revision must be refused");
    assert!(
        matches!(
            error,
            kontor_accounts::AccountError::Repository(RepositoryError::Domain(
                DomainError::RevisionConflict {
                    expected: 1,
                    found: 2,
                    ..
                }
            ))
        ),
        "expected a revision conflict, got {error:?}"
    );
    assert_eq!(
        service
            .get(fixture.project, profile.id)
            .expect("the read succeeds")
            .expect("the profile exists"),
        renamed,
        "a refused update leaves every column as it was"
    );
}

#[test]
fn referenced_profile_must_be_disabled_not_deleted() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let service = fixture.service();

    // Unreferenced, it deletes.
    let spare = fixture.profile("Spare", "beta");
    service
        .delete(fixture.project, spare.id, spare.revision)
        .expect("an unreferenced profile is deleted");

    // Referenced by a run, it does not.
    fixture.run(Some(profile.id), None);
    let error = service
        .delete(fixture.project, profile.id, profile.revision)
        .expect_err("a referenced profile must not be deleted");
    assert!(
        matches!(
            error,
            kontor_accounts::AccountError::Repository(RepositoryError::Conflict { .. })
        ),
        "expected a conflict, got {error:?}"
    );
    assert!(
        service
            .get(fixture.project, profile.id)
            .expect("the read succeeds")
            .is_some()
    );

    // Disabling is the supported retirement path.
    let disabled = service
        .set_enabled(&profile, false, at(NOW))
        .expect("a referenced profile may be disabled");
    assert!(!disabled.enabled);
}

#[test]
fn credential_identity_change_requires_a_new_profile_id() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let run = fixture.run(Some(profile.id), None);
    let service = fixture.service();

    // There is no API that changes a credential-affecting field. Rotation
    // produces a second id and retires the first.
    let successor_id = AccountProfileId::generate();
    let (successor, retired) = service
        .rotate(
            &profile,
            successor_id,
            &draft(fixture.project, "Alpha rotated", "beta"),
            at(NOW),
        )
        .expect("the rotation succeeds");
    assert_ne!(successor.id, profile.id);
    assert_eq!(successor.credential_ref.alias.as_str(), "beta-home");
    assert!(!retired.enabled);

    // The predecessor keeps its identity, and the run keeps pointing at it.
    let predecessor = service
        .get(fixture.project, profile.id)
        .expect("the read succeeds")
        .expect("the predecessor is retained");
    assert_eq!(predecessor.credential_ref, profile.credential_ref);
    assert_eq!(predecessor.harness, profile.harness);
    assert_eq!(predecessor.environment, profile.environment);
    assert_eq!(
        fixture
            .store
            .get_agent_run(fixture.project, run)
            .expect("the read succeeds")
            .expect("the run exists")
            .account_profile_id,
        Some(profile.id),
        "a rotation must not move an existing run's pin"
    );
}

#[test]
fn foreign_realm_or_project_profile_is_refused_atomically() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let service = fixture.service();

    // Another project's read never resolves it.
    assert!(
        service
            .get(fixture.other_project, profile.id)
            .expect("the read succeeds")
            .is_none()
    );
    assert!(
        service
            .list(fixture.other_project)
            .expect("the list succeeds")
            .is_empty()
    );

    // A foreign Realm envelope is refused before any write.
    let foreign = ReceiptEnvelope::new(
        RealmId::generate(),
        AccountProfileUpdate {
            project_id: fixture.project,
            id: profile.id,
            expected_revision: profile.revision,
            label: name("Foreign"),
            enabled: false,
            updated_at: at(NOW),
        },
    );
    let error = fixture
        .store
        .update_account_profile_in_realm(&foreign)
        .expect_err("a foreign realm must be refused");
    assert!(matches!(
        error,
        RepositoryError::Domain(DomainError::RealmMismatch { .. })
    ));
    let unchanged = service
        .get(fixture.project, profile.id)
        .expect("the read succeeds")
        .expect("the profile exists");
    assert_eq!(unchanged, profile, "a refused envelope writes nothing");
}

// ---------------------------------------------------------------------------
// Phase 2 — resolution
// ---------------------------------------------------------------------------

#[test]
fn resolver_rejects_unapproved_reference_before_backend_access() {
    let fixture = fixture();
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);

    // An alias nobody approved. The profile is perfectly well-formed; it simply
    // names something the policy has never heard of.
    let mut unapproved = draft(fixture.project, "Rogue", "alpha");
    unapproved.credential_ref = CredentialReference {
        kind: CredentialReferenceKind::ConfigHome,
        alias: alias("not-approved"),
    };
    let profile = fixture
        .service()
        .create(
            AccountProfileId::generate(),
            &unapproved,
            at("2026-08-10T09:00:00Z"),
        )
        .expect("the profile is created");

    let error = resolver
        .resolve(&profile)
        .expect_err("an unapproved reference must be refused");
    assert_eq!(error.reason, ResolutionReason::ReferenceNotApproved);
    assert_eq!(error.profile_id, profile.id);
    assert_eq!(
        keychain.lookups(),
        0,
        "an unapproved reference must be refused before any backend access"
    );

    // A harness nobody approved is refused too, and equally early.
    let mut other_harness = draft(fixture.project, "Other", "alpha");
    other_harness.harness = RuntimeKindKey::parse("zz.unknown").expect("a valid runtime key");
    let profile = fixture
        .service()
        .create(
            AccountProfileId::generate(),
            &other_harness,
            at("2026-08-10T09:00:00Z"),
        )
        .expect("the profile is created");
    assert_eq!(
        resolver
            .resolve(&profile)
            .expect_err("an unapproved harness must be refused")
            .reason,
        ResolutionReason::HarnessNotApproved
    );
    assert_eq!(keychain.lookups(), 0);

    // An environment variable name outside the policy is refused as well, so a
    // profile cannot decide which variables a child process gets.
    let mut rogue_variable = draft(fixture.project, "Rogue variable", "alpha");
    rogue_variable.environment = AccountEnvironmentMap::new().with(
        env_name("ZZ_NOT_APPROVED"),
        CredentialReference {
            kind: CredentialReferenceKind::Keychain,
            alias: alias("alpha-token"),
        },
    );
    let profile = fixture
        .service()
        .create(
            AccountProfileId::generate(),
            &rogue_variable,
            at("2026-08-10T09:00:00Z"),
        )
        .expect("the profile is created");
    assert_eq!(
        resolver
            .resolve(&profile)
            .expect_err("an unapproved variable must be refused")
            .reason,
        ResolutionReason::EnvironmentNameNotApproved
    );
    assert_eq!(keychain.lookups(), 0);
}

#[test]
fn an_unapproved_config_home_is_not_silently_defaulted() {
    // The policy builder refuses a directory that does not exist rather than
    // substituting a discovered default, so there is no path by which an
    // unapproved alias acquires a home.
    let error = ResolverPolicy::builder()
        .config_home(alias("ghost"), Path::new("/nonexistent/kontor/fixture"))
        .expect_err("a missing directory must not be approved");
    assert_eq!(
        error,
        PolicyError::UnusableConfigHome {
            alias: alias("ghost")
        }
    );

    // And approving the same alias twice is refused, so an alias cannot be
    // quietly repointed at a second home.
    let duplicate = ResolverPolicy::builder()
        .config_home(alias("alpha-home"), &fixture_home("alpha"))
        .expect("the first approval succeeds")
        .config_home(alias("alpha-home"), &fixture_home("beta"))
        .expect_err("a repeated alias must be refused");
    assert_eq!(
        duplicate,
        PolicyError::DuplicateAlias {
            alias: alias("alpha-home")
        }
    );

    // Both refusals were produced *from* a path, so both are a place a path
    // could escape. They carry the alias and nothing else.
    let rendered = format!("{error:?} {error} {duplicate:?} {duplicate}");
    for home in ["alpha", "beta"] {
        assert!(
            !rendered.contains(&fixture_home(home).to_string_lossy().into_owned()),
            "a policy error must not echo the {home} path it was given"
        );
    }
    assert!(!rendered.contains("/nonexistent/kontor/fixture"));
    assert!(rendered.contains("alpha-home"), "the alias is safe to name");
}

#[test]
fn two_profiles_resolve_concurrently_without_cross_talk() {
    let fixture = fixture();
    let alpha = fixture.profile("Alpha", "alpha");
    let beta = fixture.profile("Beta", "beta");
    let policy = policy();
    // Two parties, so neither lookup can return until both are inside the
    // backend: any shared "current profile" would be observable here.
    let keychain = FakeKeychain::with_barrier(Arc::new(Barrier::new(2)));
    let resolver = AccountResolver::new(&policy, &keychain);

    let (alpha_env, beta_env) = std::thread::scope(|scope| {
        let alpha_handle = scope.spawn(|| resolver.resolve(&alpha).expect("alpha resolves"));
        let beta_handle = scope.spawn(|| resolver.resolve(&beta).expect("beta resolves"));
        (
            alpha_handle.join().expect("alpha finishes"),
            beta_handle.join().expect("beta finishes"),
        )
    });

    assert_eq!(alpha_env.profile_id(), alpha.id);
    assert_eq!(beta_env.profile_id(), beta.id);
    assert_eq!(
        keychain.lookups(),
        2,
        "both resolutions overlapped inside the backend"
    );

    // Each environment carries only its own side's material. The values are
    // compared through the child environment they are applied to, and are never
    // printed.
    let alpha_applied = applied(&alpha_env);
    let beta_applied = applied(&beta_env);
    assert!(alpha_applied.contains(&(
        "ZZ_PROVIDER_CREDENTIAL".to_owned(),
        ALPHA_KEYCHAIN_CANARY.to_owned()
    )));
    assert!(beta_applied.contains(&(
        "ZZ_PROVIDER_CREDENTIAL".to_owned(),
        BETA_KEYCHAIN_CANARY.to_owned()
    )));
    assert!(
        !alpha_applied
            .iter()
            .any(|(_, value)| value.contains(BETA_KEYCHAIN_CANARY) || value.contains("beta")),
        "alpha's child must receive nothing of beta's"
    );
    assert!(
        !beta_applied
            .iter()
            .any(|(_, value)| value.contains(ALPHA_KEYCHAIN_CANARY) || value.contains("alpha")),
        "beta's child must receive nothing of alpha's"
    );
}

/// Apply an environment to a throwaway command and read the block back.
///
/// The command is never spawned. `Command::get_envs` is the only way this suite
/// inspects resolved values, and it never prints them.
fn applied(environment: &kontor_accounts::ResolvedAccountEnvironment) -> Vec<(String, String)> {
    let mut command = std::process::Command::new("/nonexistent/kontor-fake-launcher");
    environment.apply(&mut command);
    command
        .get_envs()
        .filter_map(|(key, value)| {
            Some((
                key.to_string_lossy().into_owned(),
                value?.to_string_lossy().into_owned(),
            ))
        })
        .collect()
}

#[test]
fn resolution_never_mutates_process_environment() {
    let fixture = fixture();
    let alpha = fixture.profile("Alpha", "alpha");
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);

    let before: BTreeSet<(String, String)> = std::env::vars().collect();
    let environment = resolver.resolve(&alpha).expect("alpha resolves");
    let after: BTreeSet<(String, String)> = std::env::vars().collect();
    assert_eq!(
        before, after,
        "resolution must not touch this process's environment"
    );

    // The material exists — it just went nowhere global.
    assert_eq!(environment.len(), 2);
    assert!(
        !std::env::vars().any(|(_, value)| value.contains(ALPHA_KEYCHAIN_CANARY)),
        "no resolved value may reach the parent environment"
    );
}

#[test]
fn keychain_canary_is_absent_from_debug_errors_and_serialization() {
    let fixture = fixture();
    let alpha = fixture.profile("Alpha", "alpha");
    let policy = policy();

    // 1. A successful resolution's Debug and Display, together with every
    //    rendering of the resolver's own types — the *builder* included, since a
    //    half-built policy holds the same approved paths a finished one does.
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let environment = resolver.resolve(&alpha).expect("alpha resolves");
    let rendered = format!(
        "{environment:?} {environment} {resolver:?} {}",
        resolver_renderings(&policy, &policy_builder())
    );
    for canary in canaries() {
        assert!(
            !rendered.contains(canary),
            "a rendered environment, resolver, policy or builder must not contain a canary"
        );
    }
    for home in ["alpha", "beta"] {
        assert!(
            !rendered.contains(&fixture_home(home).to_string_lossy().into_owned()),
            "a rendered value must not contain the {home} config home path"
        );
    }
    // The non-secret half is still there, which is what makes the rendering
    // useful rather than merely empty — and proves the assertions above are not
    // passing simply because nothing was printed.
    assert!(rendered.contains("ZZ_PROVIDER_CREDENTIAL"));
    assert!(rendered.contains(&alpha.id.to_string()));
    assert!(rendered.contains("alpha-home"), "aliases stay visible");
    assert!(rendered.contains("ResolverPolicyBuilder"));

    // 2. A backend failure that carries a canary internally. The reason code is
    //    all that comes out.
    let failing = FakeKeychain::failing(KeychainFailure::Denied);
    let resolver = AccountResolver::new(&policy, &failing);
    let error = resolver
        .resolve(&alpha)
        .expect_err("the backend refuses the lookup");
    let rendered = format!("{error:?} {error}");
    for canary in canaries() {
        assert!(
            !rendered.contains(canary),
            "a resolution error must not contain a canary"
        );
    }
    assert_eq!(
        error.reason,
        ResolutionReason::Keychain(KeychainFailure::Denied)
    );

    // 3. The keychain target itself.
    let target = KeychainTarget::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT_ALPHA);
    let rendered = format!("{target:?}");
    assert!(!rendered.contains(KEYCHAIN_SERVICE));
    assert!(!rendered.contains(KEYCHAIN_ACCOUNT_ALPHA));
}

// ---------------------------------------------------------------------------
// Phase 3 — launch admission
// ---------------------------------------------------------------------------

#[test]
fn disabled_profile_refuses_before_resolution_or_runtime_call() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let run = fixture.run(Some(profile.id), None);
    let disabled = fixture
        .service()
        .set_enabled(&profile, false, at(NOW))
        .expect("the profile is disabled");

    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let observation = available(&disabled);
    let declared = capabilities(true);

    let error = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &disabled, &observation, &declared),
    )
    .expect_err("a disabled profile must not launch");
    assert!(matches!(error, LaunchRefusal::ProfileDisabled));
    assert_eq!(
        keychain.lookups(),
        0,
        "a disabled profile must be refused before resolution"
    );
}

#[test]
fn cooling_or_unconfirmed_profile_refuses_before_resolution_or_runtime_call() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let run = fixture.run(Some(profile.id), None);
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let declared = capabilities(true);

    let blocked_until = at("2026-08-10T11:00:00Z");
    let cases: Vec<(AvailabilityObservation, &str)> = vec![
        (
            AvailabilityObservation {
                availability: AccountAvailability::Cooling { blocked_until },
                ..available(&profile)
            },
            "cooling",
        ),
        (
            AvailabilityObservation {
                availability: AccountAvailability::Unknown,
                ..available(&profile)
            },
            "unknown",
        ),
        (
            // Stale: observed well outside the freshness window.
            AvailabilityObservation {
                observed_at: at("2026-08-10T09:00:00Z"),
                ..available(&profile)
            },
            "stale",
        ),
        (
            // Dated in the future. Skew fails closed rather than extending the
            // window.
            AvailabilityObservation {
                observed_at: at("2026-08-10T12:00:00Z"),
                ..available(&profile)
            },
            "future",
        ),
        (
            // Evidence about a different account is not evidence about this one.
            AvailabilityObservation {
                profile_id: AccountProfileId::generate(),
                ..available(&profile)
            },
            "another account",
        ),
    ];

    for (observation, label) in cases {
        let refusal = admit_pinned_launch(
            &fixture.store,
            &resolver,
            &admission(&fixture, run, &profile, &observation, &declared),
        );
        assert!(
            refusal.is_err(),
            "{label} availability must be refused, not admitted"
        );
        let error = refusal.expect_err("already asserted");
        assert!(
            matches!(
                error,
                LaunchRefusal::Cooling { .. }
                    | LaunchRefusal::AvailabilityUnknown
                    | LaunchRefusal::AvailabilityStale
                    | LaunchRefusal::ObservationMismatch
            ),
            "{label} must be refused on availability grounds, got {error:?}"
        );
    }

    assert_eq!(
        keychain.lookups(),
        0,
        "no availability refusal may reach the resolver"
    );

    // The same run and profile with fresh, available evidence does launch, so
    // the refusals above are about the evidence and not about the fixture.
    admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &profile, &available(&profile), &declared),
    )
    .expect("fresh, available evidence admits the launch");
    assert_eq!(keychain.lookups(), 1);
}

#[test]
fn account_pin_bypass_is_refused_before_resolution() {
    let fixture = fixture();
    let pinned = fixture.profile("Alpha", "alpha");
    let other = fixture.profile("Beta", "beta");
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let declared = capabilities(true);

    // 1. A run pinned to alpha, a request naming beta.
    let run = fixture.run(Some(pinned.id), None);
    let observation = available(&other);
    let error = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &other, &observation, &declared),
    )
    .expect_err("a request must not replace the run's pin");
    assert!(matches!(error, LaunchRefusal::PinMismatch));

    // 2. An unpinned run plus a supplied profile is equally refused: a request
    //    cannot *set* a pin either.
    let unpinned = fixture.run(None, None);
    let observation = available(&pinned);
    let error = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, unpinned, &pinned, &observation, &declared),
    )
    .expect_err("a request must not supply a pin the run does not have");
    assert!(matches!(error, LaunchRefusal::PinMismatch));

    assert_eq!(
        keychain.lookups(),
        0,
        "a pin mismatch must be refused before resolution"
    );
}

#[test]
fn a_runtime_that_cannot_prove_an_account_refuses_before_resolution() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let run = fixture.run(Some(profile.id), None);
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let observation = available(&profile);

    // The refusal is the runtime contract's own, not a second copy of the rule.
    let blind = capabilities(false);
    let error = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &profile, &observation, &blind),
    )
    .expect_err("a runtime that cannot prove the account must refuse");
    assert!(
        matches!(
            error,
            LaunchRefusal::Runtime(
                kontor_runtime::adapter::RuntimeError::AccountEnvironmentUnavailable
            )
        ),
        "expected the runtime's own refusal, got {error:?}"
    );
    assert_eq!(
        keychain.lookups(),
        0,
        "an account-blind runtime must be refused before resolution"
    );

    // The same runtime declaration with account_env on admits it.
    let declared = capabilities(true);
    admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &profile, &observation, &declared),
    )
    .expect("a runtime that can prove the account admits the launch");
    assert_eq!(keychain.lookups(), 1);
}

#[test]
fn concurrent_disable_invalidates_pending_authorization() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let run = fixture.run(Some(profile.id), None);
    let path = fixture.path.clone();
    let profile_for_disable = profile.clone();

    // The disable lands *inside* the keychain lookup, which is exactly the
    // window a check-then-resolve implementation would miss.
    let disabled = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&disabled);
    let keychain = FakeKeychain::intercepting(move || {
        let mut done = flag.lock().expect("the flag is not poisoned");
        if *done {
            return;
        }
        *done = true;
        // A second connection to the same file: WAL lets this commit while the
        // admitting connection is mid-resolution.
        let other = SqliteStore::open(&path).expect("a second connection opens");
        AccountService::new(&other)
            .set_enabled(&profile_for_disable, false, at(NOW))
            .expect("the profile is disabled mid-resolution");
    });

    let policy = policy();
    let resolver = AccountResolver::new(&policy, &keychain);
    let observation = available(&profile);
    let declared = capabilities(true);

    let error = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &profile, &observation, &declared),
    )
    .expect_err("a disable during resolution must invalidate the authorization");
    assert!(
        matches!(error, LaunchRefusal::ProfileMovedDuringResolution),
        "expected the post-resolution recheck to refuse, got {error:?}"
    );
    assert_eq!(keychain.lookups(), 1, "the resolution really did happen");
}

#[test]
fn launch_receipt_contains_identity_but_no_resolution_material() {
    let fixture = fixture();
    let profile = fixture.profile("Alpha", "alpha");
    let run = fixture.run(Some(profile.id), None);
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let observation = available(&profile);
    let declared = capabilities(true);

    let admitted = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(&fixture, run, &profile, &observation, &declared),
    )
    .expect("the launch is admitted");

    let receipt = &admitted.receipt;
    assert_eq!(receipt.agent_run_id, run);
    assert_eq!(receipt.account_profile_id, profile.id);
    assert_eq!(receipt.account_profile_revision, profile.revision);
    assert_eq!(receipt.realm_id, fixture.store.realm_id());
    assert_eq!(receipt.harness, harness());
    assert_eq!(
        receipt.environment_names,
        vec![
            env_name("ZZ_CODEX_HOME"),
            env_name("ZZ_PROVIDER_CREDENTIAL")
        ],
        "the receipt records variable names, which are not secret"
    );
    assert_eq!(receipt.policy_evidence, policy.evidence());
    assert_eq!(receipt.availability_evidence, observation.evidence);

    // The receipt canonicalizes, which means it also passed the domain's own
    // sensitive-material scan on the way in.
    let document = receipt.to_document().expect("the receipt canonicalizes");
    let rendered = format!(
        "{}{}",
        document.json(),
        serde_json::to_string(receipt).expect("the receipt serializes")
    );
    for canary in canaries() {
        assert!(
            !rendered.contains(canary),
            "the launch receipt must not contain a canary"
        );
    }
    assert!(
        !rendered.contains(&fixture_home("alpha").to_string_lossy().into_owned()),
        "the launch receipt must not contain a resolved home path"
    );
    assert!(rendered.contains(&profile.id.to_string()));
    assert!(rendered.contains(&run.to_string()));
}

// ---------------------------------------------------------------------------
// Phase 4 — failover
// ---------------------------------------------------------------------------

struct FailoverFixture {
    fixture: Fixture,
    predecessor: AgentRunId,
    alpha: AccountProfile,
    beta: AccountProfile,
}

fn failover_fixture() -> FailoverFixture {
    let fixture = fixture();
    let alpha = fixture.profile("Alpha", "alpha");
    let beta = fixture.profile("Beta", "beta");
    let predecessor = fixture.run(Some(alpha.id), Some("session-alpha"));
    fixture.close(predecessor, "session-alpha");
    FailoverFixture {
        fixture,
        predecessor,
        alpha,
        beta,
    }
}

fn failover_request(
    fixture: &FailoverFixture,
    key: &str,
    successor_account: AccountProfileId,
) -> FailoverRequest {
    let predecessor = fixture
        .fixture
        .store
        .get_agent_run(fixture.fixture.project, fixture.predecessor)
        .expect("the read succeeds")
        .expect("the predecessor exists");
    FailoverRequest {
        project_id: fixture.fixture.project,
        predecessor: fixture.predecessor,
        expected_predecessor_revision: predecessor.revision,
        successor_account,
        reason: FailoverReason::AccountExhausted,
        idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
    }
}

#[test]
fn failover_creates_a_new_linked_run_and_preserves_old_binding() {
    let scenario = failover_fixture();
    let before = scenario
        .fixture
        .store
        .get_agent_run(scenario.fixture.project, scenario.predecessor)
        .expect("the read succeeds")
        .expect("the predecessor exists");

    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let observation = available(&scenario.beta);

    let outcome = fail_over_to_new_run(
        &scenario.fixture.store,
        &resolver,
        &failover_request(&scenario, "failover-1", scenario.beta.id),
        &observation,
        &capabilities(true),
        at(NOW),
    )
    .expect("the failover succeeds");

    let successor = &outcome.successor;
    assert_ne!(successor.id, scenario.predecessor);
    assert_eq!(successor.parent_agent_run_id, Some(scenario.predecessor));
    assert_eq!(successor.account_profile_id, Some(scenario.beta.id));
    assert_eq!(successor.team_run_id, before.team_run_id);
    assert_eq!(successor.role, before.role);
    assert!(
        successor.binding.is_none(),
        "the successor has not launched, so it has no binding"
    );

    // The predecessor is byte-for-byte what it was: same account, same binding,
    // same terminal evidence, same revision.
    let after = scenario
        .fixture
        .store
        .get_agent_run(scenario.fixture.project, scenario.predecessor)
        .expect("the read succeeds")
        .expect("the predecessor exists");
    assert_eq!(after, before, "a failover must not touch the predecessor");
    assert_eq!(after.account_profile_id, Some(scenario.alpha.id));
    assert!(after.binding.is_some());
    assert!(after.terminal.is_some());

    // Resolution is not part of recording a failover: the successor is checked
    // against the policy, but nothing is unlocked until it actually launches.
    assert_eq!(keychain.lookups(), 0);
}

#[test]
fn failover_retry_returns_the_same_successor() {
    let scenario = failover_fixture();
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let observation = available(&scenario.beta);
    let request = failover_request(&scenario, "failover-retry", scenario.beta.id);

    let first = fail_over_to_new_run(
        &scenario.fixture.store,
        &resolver,
        &request,
        &observation,
        &capabilities(true),
        at(NOW),
    )
    .expect("the first failover succeeds");

    let second = fail_over_to_new_run(
        &scenario.fixture.store,
        &resolver,
        &request,
        &observation,
        &capabilities(true),
        at("2026-08-10T10:00:45Z"),
    )
    .expect("a retry converges rather than creating a second run");

    assert_eq!(first.successor.id, second.successor.id);
    assert_eq!(first.receipt.id, second.receipt.id);
    assert_eq!(
        children_of(&scenario, scenario.predecessor),
        1,
        "a retry must not leave a second successor behind"
    );

    // Changing the request under the same key is a different command, and it
    // conflicts without creating anything.
    let mut changed = request.clone();
    changed.reason = FailoverReason::OperatorDirected;
    let error = fail_over_to_new_run(
        &scenario.fixture.store,
        &resolver,
        &changed,
        &observation,
        &capabilities(true),
        at(NOW),
    )
    .expect_err("a changed retry must conflict");
    assert!(
        matches!(error, FailoverRefusal::Repository(_)),
        "expected the receipt ledger to refuse, got {error:?}"
    );
    assert_eq!(children_of(&scenario, scenario.predecessor), 1);
}

/// A failover records ids and a closed reason code — and nothing a caller wrote.
///
/// The mutant this exists to kill is a free-text field on the persisted intent.
/// The domain's own screening does not stop one being dangerous: every bounded
/// string type in `kontor-core` goes through the same *marker* denylist, which
/// catches `sk-…`, `ghp_…` and `password=` but reads a bare credential-home path
/// or a keychain service name as perfectly ordinary text. The first assertion
/// below states that explicitly, so the reason this suite cannot rely on
/// validation is written down rather than assumed.
#[test]
fn a_failover_intent_records_only_ids_and_a_closed_reason_code() {
    let scenario = failover_fixture();
    let fixture = &scenario.fixture;

    // Exactly the payload a pasted "why did we rotate?" note would carry: a real
    // approved config home, plus a real keychain service and account.
    let hostile = format!(
        "rotate off {} keychain {} {}",
        fixture_home("alpha").display(),
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT_ALPHA
    );
    ExternalName::parse(&hostile)
        .expect("a path and a keychain target pass the domain's marker denylist — the finding");

    // Hand that text to the one caller-controlled field a profile still has, so
    // anything that copies caller text into the intent has something
    // incriminating to copy.
    let successor = AccountService::new(&fixture.store)
        .create(
            AccountProfileId::generate(),
            &AccountProfileDraft {
                label: ExternalName::parse(&hostile).expect("the hostile label parses"),
                ..draft(fixture.project, "unused", "beta")
            },
            at("2026-08-10T09:00:00Z"),
        )
        .expect("the profile is created");

    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let outcome = fail_over_to_new_run(
        &fixture.store,
        &resolver,
        &failover_request(&scenario, "failover-hostile", successor.id),
        &available(&successor),
        &capabilities(true),
        at(NOW),
    )
    .expect("the failover succeeds");

    // 1. The intent's shape is closed. A new free-text field would show up here
    //    before it ever showed up in a canary scan.
    let intent: serde_json::Value =
        serde_json::from_str(outcome.receipt.intent.json()).expect("the intent is JSON");
    let keys: BTreeSet<&str> = intent
        .as_object()
        .expect("the intent is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "operation",
            "predecessor_account_profile_id",
            "predecessor_agent_run_id",
            "reason",
            "schema_version",
            "successor_account_profile_id",
            "successor_agent_run_id",
        ]),
        "the failover intent carries ids and a reason code, and nothing else"
    );
    assert_eq!(intent["reason"], "account_exhausted");

    // 2. The reason is a closed code: arbitrary text cannot become one, so the
    //    field cannot be repurposed as a note.
    assert!(serde_json::from_value::<FailoverReason>(serde_json::json!(hostile)).is_err());
    assert_eq!(
        FailoverReason::AccountExhausted.description(),
        "the account hit a provider quota or cooldown",
        "the human-readable half is derived from the code, not supplied with it"
    );

    // 3. Nothing the caller wrote reached the intent, the receipt, or any
    //    rendering of the outcome.
    let rendered = format!(
        "{} {outcome:?} {:?} {:?}",
        outcome.receipt.intent.json(),
        outcome.receipt,
        outcome.successor
    );

    // 4. …nor the persisted command ledger. This is scoped to the ledger on
    //    purpose: a profile *label* legitimately lives in `account_profiles`,
    //    and the claim being made here is that caller text does not spread from
    //    there into the command record of a security event.
    let connection = rusqlite::Connection::open(&fixture.path).expect("a raw connection opens");
    let mut ledger = String::new();
    for sql in [
        "SELECT intent FROM command_receipts",
        "SELECT payload FROM command_outbox",
        "SELECT payload FROM runtime_events",
    ] {
        let mut statement = connection.prepare(sql).expect("the query prepares");
        let mut rows = statement.query([]).expect("the query runs");
        while let Some(row) = rows.next().expect("a row reads") {
            ledger.push_str(&row.get::<_, String>(0).expect("a text column"));
            ledger.push('\n');
        }
    }
    assert!(
        ledger.contains(&outcome.successor.id.to_string()),
        "the ledger scan must be reading the real rows"
    );

    for (label, haystack) in [("the outcome", &rendered), ("the command ledger", &ledger)] {
        for needle in [
            fixture_home("alpha").to_string_lossy().into_owned(),
            KEYCHAIN_SERVICE.to_owned(),
            KEYCHAIN_ACCOUNT_ALPHA.to_owned(),
            hostile.clone(),
        ] {
            assert!(
                !haystack.contains(&needle),
                "{label} must not carry caller-supplied text: `{needle}`"
            );
        }
    }
}

/// How many runs name `parent` as their parent.
fn children_of(scenario: &FailoverFixture, parent: AgentRunId) -> i64 {
    let connection =
        rusqlite::Connection::open(&scenario.fixture.path).expect("a raw connection opens");
    connection
        .query_row(
            "SELECT count(*) FROM agent_runs WHERE parent_agent_run_id = ?1",
            [parent.to_string()],
            |row| row.get(0),
        )
        .expect("the count is readable")
}

#[test]
fn invalid_failover_has_no_partial_run_or_receipt() {
    let scenario = failover_fixture();
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);
    let declared = capabilities(true);

    let runs_before = run_count(&scenario);
    let receipts_before = receipt_count(&scenario);

    // 1. The same account is not a failover.
    let same = failover_request(&scenario, "failover-same", scenario.alpha.id);
    assert!(matches!(
        fail_over_to_new_run(
            &scenario.fixture.store,
            &resolver,
            &same,
            &available(&scenario.alpha),
            &declared,
            at(NOW),
        )
        .expect_err("the same account must be refused"),
        FailoverRefusal::SameAccount
    ));

    // 2. A stale predecessor revision.
    let mut stale = failover_request(&scenario, "failover-stale", scenario.beta.id);
    stale.expected_predecessor_revision =
        AggregateRevision::parse(99).expect("a positive revision");
    assert!(matches!(
        fail_over_to_new_run(
            &scenario.fixture.store,
            &resolver,
            &stale,
            &available(&scenario.beta),
            &declared,
            at(NOW),
        )
        .expect_err("a stale revision must be refused"),
        FailoverRefusal::Domain(DomainError::RevisionConflict { .. })
    ));

    // 3. A disabled successor.
    let disabled = AccountService::new(&scenario.fixture.store)
        .set_enabled(&scenario.beta, false, at(NOW))
        .expect("the successor is disabled");
    assert!(matches!(
        fail_over_to_new_run(
            &scenario.fixture.store,
            &resolver,
            &failover_request(&scenario, "failover-disabled", disabled.id),
            &available(&disabled),
            &declared,
            at(NOW),
        )
        .expect_err("a disabled successor must be refused"),
        FailoverRefusal::Successor(LaunchRefusal::ProfileDisabled)
    ));
    AccountService::new(&scenario.fixture.store)
        .set_enabled(&disabled, true, at(NOW))
        .expect("the successor is re-enabled");

    // 4. A cooling successor.
    assert!(matches!(
        fail_over_to_new_run(
            &scenario.fixture.store,
            &resolver,
            &failover_request(&scenario, "failover-cooling", scenario.beta.id),
            &AvailabilityObservation {
                availability: AccountAvailability::Cooling {
                    blocked_until: at("2026-08-10T11:00:00Z")
                },
                ..available(&scenario.beta)
            },
            &declared,
            at(NOW),
        )
        .expect_err("a cooling successor must be refused"),
        FailoverRefusal::Successor(LaunchRefusal::Cooling { .. })
    ));

    // 5. An active predecessor: an account never rotates under a live run.
    let active = scenario.fixture.run(Some(scenario.alpha.id), None);
    let active_run = scenario
        .fixture
        .store
        .get_agent_run(scenario.fixture.project, active)
        .expect("the read succeeds")
        .expect("the run exists");
    assert!(matches!(
        fail_over_to_new_run(
            &scenario.fixture.store,
            &resolver,
            &FailoverRequest {
                predecessor: active,
                expected_predecessor_revision: active_run.revision,
                ..failover_request(&scenario, "failover-active", scenario.beta.id)
            },
            &available(&scenario.beta),
            &declared,
            at(NOW),
        )
        .expect_err("an active predecessor must be refused"),
        FailoverRefusal::PredecessorActive
    ));

    // 6. A predecessor in another project simply does not resolve.
    assert!(matches!(
        fail_over_to_new_run(
            &scenario.fixture.store,
            &resolver,
            &FailoverRequest {
                project_id: scenario.fixture.other_project,
                ..failover_request(&scenario, "failover-foreign", scenario.beta.id)
            },
            &available(&scenario.beta),
            &declared,
            at(NOW),
        )
        .expect_err("a cross-project predecessor must be refused"),
        FailoverRefusal::PredecessorNotFound
    ));

    // Nothing above created a run or a receipt. `active` is the one run this
    // test added deliberately.
    assert_eq!(run_count(&scenario), runs_before + 1);
    assert_eq!(receipt_count(&scenario), receipts_before);
}

fn run_count(scenario: &FailoverFixture) -> i64 {
    scalar(scenario, "SELECT count(*) FROM agent_runs")
}

fn receipt_count(scenario: &FailoverFixture) -> i64 {
    scalar(scenario, "SELECT count(*) FROM command_receipts")
}

fn scalar(scenario: &FailoverFixture, sql: &str) -> i64 {
    let connection =
        rusqlite::Connection::open(&scenario.fixture.path).expect("a raw connection opens");
    connection
        .query_row(sql, [], |row| row.get(0))
        .expect("the count is readable")
}

// ---------------------------------------------------------------------------
// Phase 5 — the full canary boundary
// ---------------------------------------------------------------------------

#[test]
fn database_logs_export_and_argv_contain_ids_not_secrets() {
    let scenario = failover_fixture();
    let fixture = &scenario.fixture;
    let policy = policy();
    let keychain = FakeKeychain::new();
    let resolver = AccountResolver::new(&policy, &keychain);

    // Produce every artefact a real launch would: a failover successor, an
    // admission, a receipt, a child command, and the projections an API or
    // export would serve.
    let outcome = fail_over_to_new_run(
        &fixture.store,
        &resolver,
        &failover_request(&scenario, "failover-canary", scenario.beta.id),
        &available(&scenario.beta),
        &capabilities(true),
        at(NOW),
    )
    .expect("the failover succeeds");

    let admitted = admit_pinned_launch(
        &fixture.store,
        &resolver,
        &admission(
            fixture,
            outcome.successor.id,
            &scenario.beta,
            &available(&scenario.beta),
            &capabilities(true),
        ),
    )
    .expect("the successor's launch is admitted");

    // The child process command. Material may exist only in its environment
    // block; argv must be free of it.
    let mut command = std::process::Command::new("/nonexistent/kontor-fake-launcher");
    command.arg("--profile").arg(scenario.beta.id.to_string());
    admitted.environment.apply(&mut command);
    let argv: String = command
        .get_args()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let environment_keys: Vec<String> = command
        .get_envs()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect();

    // Everything a reader could ever be handed, in one string.
    let mut artefacts = String::new();
    artefacts.push_str(&argv);
    artefacts.push('\n');
    artefacts.push_str(&environment_keys.join(" "));
    artefacts.push('\n');
    artefacts.push_str(
        &serde_json::to_string(&admitted.receipt).expect("the launch receipt serializes"),
    );
    artefacts.push('\n');
    artefacts.push_str(
        admitted
            .receipt
            .to_document()
            .expect("the receipt canonicalizes")
            .json(),
    );
    artefacts.push('\n');
    artefacts.push_str(outcome.receipt.intent.json());
    artefacts.push('\n');
    artefacts.push_str(
        &serde_json::to_string(
            &AccountService::new(&fixture.store)
                .list(fixture.project)
                .expect("the list succeeds"),
        )
        .expect("the list projection serializes"),
    );
    artefacts.push('\n');
    artefacts.push_str(
        &serde_json::to_string(
            &fixture
                .store
                .snapshot_account_profile(fixture.project, scenario.beta.id)
                .expect("the snapshot succeeds"),
        )
        .expect("the snapshot envelope serializes"),
    );
    artefacts.push('\n');
    // What a `Debug` log line would carry.
    artefacts.push_str(&format!(
        "{admitted:?} {resolver:?} {} {:?}",
        resolver_renderings(&policy, &policy_builder()),
        outcome.successor
    ));

    // The database's own bytes, main file plus WAL and SHM.
    drop(admitted);
    let mut database = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let candidate = PathBuf::from(format!("{}{suffix}", fixture.path.display()));
        if candidate.exists() {
            database.extend(std::fs::read(&candidate).expect("the database file is readable"));
        }
    }
    let database = String::from_utf8_lossy(&database).into_owned();

    // The scan actually looks at live bytes: the ids are there.
    assert!(
        database.contains(&scenario.beta.id.to_string()),
        "the scan must be reading the real database"
    );
    assert!(artefacts.contains(&scenario.beta.id.to_string()));
    assert!(artefacts.contains(&outcome.successor.id.to_string()));
    assert!(artefacts.contains(&scenario.alpha.id.to_string()));

    // And none of them carries anything resolvable.
    let alpha_home = fixture_home("alpha").to_string_lossy().into_owned();
    let beta_home = fixture_home("beta").to_string_lossy().into_owned();
    for (label, haystack) in [("artefacts", &artefacts), ("database", &database)] {
        for canary in canaries() {
            assert!(
                !haystack.contains(canary),
                "{label} must not contain the canary `{canary}`"
            );
        }
        assert!(
            !haystack.contains(&alpha_home),
            "{label} must not contain a resolved config home path"
        );
        assert!(
            !haystack.contains(&beta_home),
            "{label} must not contain a resolved config home path"
        );
    }

    // The environment *keys* are recorded — that is the non-secret half, and
    // recording it is what makes the receipt auditable.
    assert!(environment_keys.iter().any(|key| key == "ZZ_CODEX_HOME"));
    assert!(
        environment_keys
            .iter()
            .any(|key| key == "ZZ_PROVIDER_CREDENTIAL")
    );
}

/// The macOS backend reads exactly one of `security`'s exit statuses — the one
/// meaning "no such item" — and reports everything else as unavailable. A
/// lookup for a service that cannot exist proves that mapping against the real
/// `/usr/bin/security` without needing a keychain entry, a grant, or an
/// authorization dialog: a miss is answered from the search alone.
#[cfg(target_os = "macos")]
#[test]
fn system_keychain_reports_a_missing_entry_as_not_found() {
    let target = KeychainTarget::new(
        "kontor-account-security-absent-service",
        "kontor-account-security-absent-account",
    );

    assert!(matches!(
        SystemKeychain.secret(&target),
        Err(KeychainFailure::NotFound)
    ));
}
