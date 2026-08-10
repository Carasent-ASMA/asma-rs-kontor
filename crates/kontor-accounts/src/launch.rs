//! Launch admission, the audit receipt, and explicit successor-run failover.
//!
//! # The admission order is the security property
//!
//! [`admit_pinned_launch`] evaluates in one fixed order, and each step must be
//! able to refuse without the later ones having run:
//!
//! 1. the run exists and its stored pin *equals* the requested profile;
//! 2. the profile exists in the same project and is enabled;
//! 3. a fresh availability observation says the account is usable;
//! 4. the runtime's own preflight proves it can supply an account environment;
//! 5. only now is anything resolved;
//! 6. the profile is re-read and must be unchanged;
//! 7. a receipt is produced.
//!
//! Steps 1–4 are what makes "refused before any effect" true: a disabled,
//! cooling, mismatched or account-blind launch never reaches a keychain, a
//! filesystem or an adapter. Step 6 is what makes it true under concurrency —
//! a keychain lookup can block for a long time, and a disable that lands during
//! one must still stop the launch it was racing.
//!
//! # Why failover is a new run
//!
//! A terminal run is immutable. Rotating an account is therefore not an edit of
//! the run that failed but a *successor*: a new [`AgentRunId`] with the same
//! project, team, task and role, `parent_agent_run_id` pointing at the
//! predecessor, the new pin, and no binding. The predecessor keeps its account,
//! its binding and its terminal evidence, so the audit trail of what actually
//! ran under the old account survives the rotation intact.
//!
//! Convergence comes from the existing command-receipt ledger rather than from a
//! second store: the successor's id lives in the receipt's canonical intent, so
//! a retry under the same idempotency key finds the original receipt and
//! finishes creating *that* successor instead of minting a second one.

use kontor_core::DomainError;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, ContentHash,
    EnvironmentVariableName, ExternalId, IdempotencyKey, ProjectId, RealmId, RuntimeKindKey,
    SchemaVersion, Timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind, CommandReceipt};
use kontor_core::repository::{
    AgentRun, CommandRepository, NewAgentRun, NewCommandIntent, ProjectRepository, RepositoryError,
    RunRepository,
};
use kontor_runtime::adapter::RuntimeError;
use kontor_runtime::capability::{OperationContext, RuntimeCapabilities, RuntimeCapability};
use serde::{Deserialize, Serialize};

use crate::resolver::{AccountResolver, ResolutionError, ResolvedAccountEnvironment};

/// How old an availability observation may be and still be acted on.
///
/// Fleet cooldown state changes on the order of minutes, so a minute-old
/// observation is evidence and a ten-minute-old one is a memory. Anything older
/// fails closed rather than being treated as "probably still fine".
pub const MAX_OBSERVATION_AGE_SECONDS: i64 = 60;

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

/// What the fleet boundary last said about one account's usability.
///
/// This is deliberately *not* folded into
/// [`kontor_runtime::observation::ControlPlaneObservation`], which is evidence
/// about one native session's contact and work state. Account availability is a
/// different fact, about a different subject, owned by a different authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccountAvailability {
    /// The account may be used now.
    Available,
    /// The account is in cooldown until the given instant.
    Cooling {
        /// When the cooldown lifts.
        blocked_until: Timestamp,
    },
    /// Nothing is known. Never treated as available.
    Unknown,
}

/// One typed availability fact, supplied by the fleet integration.
///
/// Kontor never reads `~/.asma/fleet/` to obtain one: `asma fleet` owns cooldown
/// mechanics, and this is the value it hands over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityObservation {
    /// The account it concerns.
    pub profile_id: AccountProfileId,
    /// What was observed.
    pub availability: AccountAvailability,
    /// When it was observed.
    pub observed_at: Timestamp,
    /// The fleet record this came from, for the receipt.
    pub evidence: ExternalId,
}

impl AvailabilityObservation {
    /// Whether this observation still counts as evidence at `now`.
    ///
    /// An observation from the future is refused as well as an old one: clock
    /// skew is a reason to fail closed, not a reason to extend freshness.
    #[must_use]
    pub fn is_fresh(&self, now: Timestamp) -> bool {
        let age = now.as_second() - self.observed_at.as_second();
        (0..=MAX_OBSERVATION_AGE_SECONDS).contains(&age)
    }
}

// ---------------------------------------------------------------------------
// Admission
// ---------------------------------------------------------------------------

/// Why a pinned launch was refused.
///
/// Every variant is a fact about identity, policy or freshness. None of them
/// carries a resolved value, so a refusal is safe to log verbatim.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LaunchRefusal {
    /// The run does not exist in this project.
    #[error("agent run is unknown in this project")]
    RunNotFound,
    /// The profile does not exist in this project.
    #[error("account profile is unknown in this project")]
    ProfileNotFound,
    /// The request named a different account than the run is pinned to, or the
    /// run carries no pin at all.
    ///
    /// A request can only ever *name* the pin the run already has. It cannot
    /// set, change or supply one — that is what makes the pin the single source
    /// of truth about which account a run executes as.
    #[error("the requested account is not the account this run is pinned to")]
    PinMismatch,
    /// The profile is disabled.
    #[error("account profile is disabled")]
    ProfileDisabled,
    /// The observation concerns another account.
    #[error("the availability observation concerns another account")]
    ObservationMismatch,
    /// The account is in cooldown.
    #[error("account is cooling until {blocked_until}")]
    Cooling {
        /// When the cooldown lifts.
        blocked_until: Timestamp,
    },
    /// Availability is unknown, which is never treated as available.
    #[error("account availability is unknown")]
    AvailabilityUnknown,
    /// The observation is too old, or dated in the future, to be acted on.
    #[error("account availability evidence is not fresh")]
    AvailabilityStale,
    /// The runtime refused the operation before any effect.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// Resolution was refused.
    #[error(transparent)]
    Resolution(#[from] ResolutionError),
    /// The profile changed between the policy check and the authorization.
    #[error("account profile changed during resolution")]
    ProfileMovedDuringResolution,
    /// The repository refused.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// Everything an admission decision needs.
#[derive(Debug, Clone, Copy)]
pub struct LaunchAdmissionRequest<'a> {
    /// The Realm the receipt will be qualified with.
    pub realm_id: RealmId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The run being launched.
    pub agent_run_id: AgentRunId,
    /// The account the caller believes the run is pinned to.
    pub account_profile_id: AccountProfileId,
    /// Fresh evidence about that account, from the fleet boundary.
    pub observation: &'a AvailabilityObservation,
    /// What the runtime currently declares.
    pub capabilities: &'a RuntimeCapabilities,
    /// The decision instant.
    pub now: Timestamp,
}

/// The non-secret, serializable record of one admission.
///
/// It names the profile and records the evidence the decision rested on. It
/// carries no secret, no resolved value, no path, no keychain target, no auth
/// file content and no digest of any of those — `policy_evidence` is a digest of
/// the approved *names*, and a digest of a secret would still be a fact about
/// that secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountLaunchReceipt {
    /// The envelope contract this receipt was written under.
    pub schema_version: SchemaVersion,
    /// The Realm that produced it.
    pub realm_id: RealmId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The run that was admitted.
    pub agent_run_id: AgentRunId,
    /// The account it is pinned to.
    pub account_profile_id: AccountProfileId,
    /// The profile revision the decision was taken against.
    pub account_profile_revision: AggregateRevision,
    /// The immutable harness the profile authenticates through.
    pub harness: RuntimeKindKey,
    /// The non-secret provider identity hint, if the profile records one.
    pub provider_identity: Option<ExternalId>,
    /// The environment variable *names* the launch will fill.
    pub environment_names: Vec<EnvironmentVariableName>,
    /// A digest of the resolver policy's approved names.
    pub policy_evidence: ContentHash,
    /// The fleet record the availability decision rested on.
    pub availability_evidence: ExternalId,
    /// When that availability was observed.
    pub availability_observed_at: Timestamp,
    /// A digest of the capabilities the runtime declared.
    pub capability_evidence: ContentHash,
    /// When the decision was taken.
    pub decided_at: Timestamp,
}

impl AccountLaunchReceipt {
    /// Canonicalize the receipt so a launch command can embed it in its intent.
    ///
    /// There is deliberately no second receipt store: this document travels
    /// inside the existing command receipt's canonical safe intent.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the receipt does not canonicalize, which
    /// includes [`DomainError::SensitiveMaterial`] if any field ever came to
    /// hold credential-looking text.
    pub fn to_document(&self) -> Result<CanonicalDocument, DomainError> {
        CanonicalDocument::from_serializable(self)
    }
}

/// An admitted launch: the audit record, and the material to hand the child.
pub struct AdmittedLaunch {
    /// The non-secret record of the decision.
    pub receipt: AccountLaunchReceipt,
    /// The short-lived environment. Dropped — and zeroized — with this value.
    pub environment: ResolvedAccountEnvironment,
}

impl std::fmt::Debug for AdmittedLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmittedLaunch")
            .field("receipt", &self.receipt)
            .field("environment", &self.environment)
            .finish()
    }
}

/// Judge one account-pinned launch, resolving only once everything else agrees.
///
/// # Errors
/// Returns the first [`LaunchRefusal`] in the documented order. Refusals at
/// steps 1–4 reach neither the resolver nor the runtime adapter.
pub fn admit_pinned_launch<S>(
    store: &S,
    resolver: &AccountResolver<'_>,
    request: &LaunchAdmissionRequest<'_>,
) -> Result<AdmittedLaunch, LaunchRefusal>
where
    S: ProjectRepository + RunRepository,
{
    // 1. The pin is the run's, not the request's. A request that names a
    //    different account — or any account for an unpinned run — is refused
    //    rather than treated as setting one.
    let run = store
        .get_agent_run(request.project_id, request.agent_run_id)?
        .ok_or(LaunchRefusal::RunNotFound)?;
    if run.account_profile_id != Some(request.account_profile_id) {
        return Err(LaunchRefusal::PinMismatch);
    }

    // 2. The profile, in the same project, enabled.
    let profile = store
        .get_account_profile(request.project_id, request.account_profile_id)?
        .ok_or(LaunchRefusal::ProfileNotFound)?;
    if !profile.enabled {
        return Err(LaunchRefusal::ProfileDisabled);
    }

    // 3. Fresh availability. `Unknown` and stale both fail closed.
    check_availability(request.observation, profile.id, request.now)?;

    // 4. The runtime's own gate, with the pin declared. A runtime that cannot
    //    prove a per-run account environment refuses here — before resolution,
    //    and before any adapter call.
    let mut context = OperationContext::new(RuntimeCapability::Launch);
    context.account_pinned = true;
    kontor_runtime::capability::preflight(request.capabilities, &context)?;

    // 5. Only now does anything look at a keychain or a filesystem.
    let environment = resolver.resolve(&profile)?;

    // 6. Re-read. A keychain lookup can block for seconds; a disable or a
    //    rename that landed during one must invalidate this authorization
    //    rather than be overtaken by it.
    let current = store
        .get_account_profile(request.project_id, request.account_profile_id)?
        .ok_or(LaunchRefusal::ProfileMovedDuringResolution)?;
    if current.revision != profile.revision || !current.enabled {
        return Err(LaunchRefusal::ProfileMovedDuringResolution);
    }

    let receipt = AccountLaunchReceipt {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        realm_id: request.realm_id,
        project_id: request.project_id,
        agent_run_id: request.agent_run_id,
        account_profile_id: profile.id,
        account_profile_revision: profile.revision,
        harness: profile.harness.clone(),
        provider_identity: profile.provider_identity.clone(),
        environment_names: environment.names(),
        policy_evidence: resolver.policy().evidence(),
        availability_evidence: request.observation.evidence.clone(),
        availability_observed_at: request.observation.observed_at,
        capability_evidence: capability_evidence(request.capabilities),
        decided_at: request.now,
    };
    Ok(AdmittedLaunch {
        receipt,
        environment,
    })
}

/// The availability half of admission, shared by launch and failover.
fn check_availability(
    observation: &AvailabilityObservation,
    profile_id: AccountProfileId,
    now: Timestamp,
) -> Result<(), LaunchRefusal> {
    if observation.profile_id != profile_id {
        return Err(LaunchRefusal::ObservationMismatch);
    }
    if !observation.is_fresh(now) {
        return Err(LaunchRefusal::AvailabilityStale);
    }
    match observation.availability {
        AccountAvailability::Available => Ok(()),
        AccountAvailability::Cooling { blocked_until } => {
            Err(LaunchRefusal::Cooling { blocked_until })
        }
        AccountAvailability::Unknown => Err(LaunchRefusal::AvailabilityUnknown),
    }
}

/// A digest of what the runtime declared, so a receipt records the evidence
/// quality the decision rested on without copying the whole declaration.
fn capability_evidence(capabilities: &RuntimeCapabilities) -> ContentHash {
    let rendered = serde_json::to_string(capabilities)
        .unwrap_or_else(|_| "unserializable-capabilities".to_owned());
    ContentHash::of(rendered.as_bytes())
}

// ---------------------------------------------------------------------------
// Failover
// ---------------------------------------------------------------------------

/// Why an account was rotated.
///
/// A closed set, and the *whole* reason: there is deliberately no accompanying
/// free-text note. This is the one field of a failover an operator chooses, and
/// a free-text sibling would be a caller-controlled string that lands in the
/// persisted intent, the command receipt, every projection built from them, and
/// any export — which is precisely where a pasted credential path or keychain
/// target ends up in practice.
///
/// Validating such a note was considered and rejected. `BoundedText` already
/// screens for credential *markers* (`sk-…`, `ghp_…`, `password=`) and it is
/// exactly that denylist which fails to see `/Users/someone/.codex` or a
/// keychain service name. A second denylist would have the same shape and the
/// same eventual gap; a caller-supplied string with no unsafe content is not a
/// thing this crate can recognise. So the reason is a code, the code carries its
/// own human-readable text, and no caller bytes reach persisted state at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailoverReason {
    /// The predecessor's account hit a provider quota or cooldown.
    AccountExhausted,
    /// The predecessor's credentials stopped working.
    CredentialRejected,
    /// An operator moved the work deliberately.
    OperatorDirected,
}

impl FailoverReason {
    /// The stable spelling used in JSON, receipts and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountExhausted => "account_exhausted",
            Self::CredentialRejected => "credential_rejected",
            Self::OperatorDirected => "operator_directed",
        }
    }

    /// The human-readable half of the reason.
    ///
    /// This is what a note would have been for. Because it is derived from the
    /// code rather than supplied with it, an audit record can be read by a human
    /// without any caller-controlled text having been persisted to produce it.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::AccountExhausted => "the account hit a provider quota or cooldown",
            Self::CredentialRejected => "the account's credentials were rejected",
            Self::OperatorDirected => "an operator moved the work deliberately",
        }
    }
}

impl std::fmt::Display for FailoverReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a failover was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FailoverRefusal {
    /// The predecessor does not exist in this project.
    #[error("the predecessor run is unknown in this project")]
    PredecessorNotFound,
    /// The predecessor moved since the caller read it.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// The predecessor is still active.
    ///
    /// Cooling or a credential change never rotates an account inside a live
    /// binding: the session running under the old account has to have stopped
    /// before another one starts under the new one.
    #[error("the predecessor run is not terminal, and an account never rotates under a live run")]
    PredecessorActive,
    /// The predecessor closed without evidence.
    #[error("the predecessor run has no terminal evidence")]
    PredecessorEvidenceMissing,
    /// The successor is the account the predecessor already used.
    #[error("the successor account is the one the predecessor already ran under")]
    SameAccount,
    /// The team run the predecessor belongs to is missing.
    #[error("the predecessor's team run is unknown in this project")]
    TeamRunNotFound,
    /// The successor account was refused by the same checks a launch applies.
    #[error(transparent)]
    Successor(#[from] LaunchRefusal),
    /// The recorded intent could not be read back.
    #[error("the recorded failover intent is not readable")]
    IntentUnreadable,
    /// The repository refused.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// One explicit request to move work to another account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailoverRequest {
    /// Owning project.
    pub project_id: ProjectId,
    /// The run that closed.
    pub predecessor: AgentRunId,
    /// The revision the caller believes the predecessor is at.
    pub expected_predecessor_revision: AggregateRevision,
    /// The account the successor will be pinned to.
    pub successor_account: AccountProfileId,
    /// Why. A closed code, and the whole of the caller-chosen justification —
    /// see [`FailoverReason`] for why there is no free-text sibling.
    pub reason: FailoverReason,
    /// The caller's idempotency key.
    pub idempotency_key: IdempotencyKey,
}

/// What a failover produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailoverOutcome {
    /// The new run. Pinned to the successor account, parented to the
    /// predecessor, and deliberately unbound: it has not launched yet.
    pub successor: AgentRun,
    /// The ledger entry that makes the operation idempotent.
    pub receipt: CommandReceipt,
}

/// The canonical safe intent a failover is recorded under.
///
/// The successor's id is *in* the intent. That is what makes a retry converge:
/// the second call finds the receipt, reads the id the first call minted, and
/// finishes creating that run rather than minting a second one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FailoverIntent {
    schema_version: SchemaVersion,
    operation: String,
    predecessor_agent_run_id: AgentRunId,
    successor_agent_run_id: AgentRunId,
    predecessor_account_profile_id: Option<AccountProfileId>,
    successor_account_profile_id: AccountProfileId,
    reason: FailoverReason,
}

/// The stable spelling of the operation this intent records.
const FAILOVER_OPERATION: &str = "account_failover";

/// Move work to another account by creating one linked successor run.
///
/// The predecessor is never touched: not its account, not its binding, not its
/// terminal evidence, not its revision.
///
/// # Errors
/// Returns [`FailoverRefusal`] for an unknown, active, evidence-free or moved
/// predecessor, a same-account request, and a successor account that fails the
/// enabled/availability/policy/capability checks. Every one of those refuses
/// before a receipt or a run exists.
pub fn fail_over_to_new_run<S>(
    store: &S,
    resolver: &AccountResolver<'_>,
    request: &FailoverRequest,
    observation: &AvailabilityObservation,
    capabilities: &RuntimeCapabilities,
    now: Timestamp,
) -> Result<FailoverOutcome, FailoverRefusal>
where
    S: ProjectRepository + RunRepository + CommandRepository,
{
    let predecessor = store
        .get_agent_run(request.project_id, request.predecessor)?
        .ok_or(FailoverRefusal::PredecessorNotFound)?;
    predecessor
        .revision
        .expect("agent run", request.expected_predecessor_revision)?;
    if !predecessor.projection.lifecycle.is_terminal() {
        return Err(FailoverRefusal::PredecessorActive);
    }
    let Some(terminal) = predecessor.terminal.as_ref() else {
        return Err(FailoverRefusal::PredecessorEvidenceMissing);
    };
    if predecessor.account_profile_id == Some(request.successor_account) {
        return Err(FailoverRefusal::SameAccount);
    }

    // The successor account passes the same gates a launch applies, minus the
    // resolution itself: a failover that hands work to a disabled, cooling or
    // unapproved account is not a recovery.
    let successor_profile = store
        .get_account_profile(request.project_id, request.successor_account)?
        .ok_or(LaunchRefusal::ProfileNotFound)?;
    if !successor_profile.enabled {
        return Err(LaunchRefusal::ProfileDisabled.into());
    }
    check_availability(observation, successor_profile.id, now)?;
    resolver
        .validate(&successor_profile)
        .map_err(LaunchRefusal::Resolution)?;
    let mut context = OperationContext::new(RuntimeCapability::Launch);
    context.account_pinned = true;
    kontor_runtime::capability::preflight(capabilities, &context)
        .map_err(LaunchRefusal::Runtime)?;

    // The intent is recorded against the team run as a *witness*: it cites the
    // revision it was computed against without moving it, so recording a
    // failover does not mutate any run — least of all the predecessor.
    let team_run = store
        .get_team_run(request.project_id, predecessor.team_run_id)?
        .ok_or(FailoverRefusal::TeamRunNotFound)?;

    // A retry reuses the successor id and the witnessed revision from the
    // original receipt, and rebuilds every other field from the *current*
    // request — so an unchanged retry replays, and a retry that changed the
    // account or the reason conflicts instead of quietly succeeding.
    let existing = store.get_receipt_by_key(&request.idempotency_key)?;
    let (successor_id, target_revision) = match existing.as_ref() {
        Some(receipt) => {
            let recorded: FailoverIntent = receipt
                .intent
                .deserialize()
                .map_err(|_| FailoverRefusal::IntentUnreadable)?;
            (recorded.successor_agent_run_id, receipt.target_revision)
        }
        None => (AgentRunId::generate(), team_run.revision),
    };

    let intent = CanonicalDocument::from_serializable(&FailoverIntent {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        operation: FAILOVER_OPERATION.to_owned(),
        predecessor_agent_run_id: predecessor.id,
        successor_agent_run_id: successor_id,
        predecessor_account_profile_id: predecessor.account_profile_id,
        successor_account_profile_id: successor_profile.id,
        reason: request.reason,
    })
    .map_err(FailoverRefusal::Domain)?;

    let receipt = store.record_intent(&NewCommandIntent {
        project_id: request.project_id,
        receipt_id: kontor_core::id::CommandReceiptId::generate(),
        idempotency_key: request.idempotency_key.clone(),
        kind: CommandKind::LaunchRun,
        target: AggregateRef::TeamRun {
            team_run_id: predecessor.team_run_id,
        },
        target_revision,
        intent: intent.clone(),
        payload: intent,
        desired: None,
        // A stable instant, so a retry does not look like a different command
        // just because it happened later. The predecessor is terminal, so its
        // closure time no longer moves.
        not_before: terminal.closed_at,
        created_at: now,
    })?;

    // The receipt exists, so the successor's identity is durable. Creating the
    // run is the effect that follows it, and a retry after a crash in between
    // completes it rather than starting again.
    let successor = match store.get_agent_run(request.project_id, successor_id)? {
        Some(existing) => existing,
        None => store.create_agent_run(&NewAgentRun {
            id: successor_id,
            project_id: predecessor.project_id,
            team_run_id: predecessor.team_run_id,
            parent_agent_run_id: Some(predecessor.id),
            role: predecessor.role.clone(),
            account_profile_id: Some(successor_profile.id),
            // Unbound on purpose: the successor has not launched, and the
            // predecessor's binding is never moved, replaced or reused.
            binding: None,
            created_at: now,
        })?,
    };

    Ok(FailoverOutcome { successor, receipt })
}
