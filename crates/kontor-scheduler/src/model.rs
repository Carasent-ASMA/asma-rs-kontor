//! What the scheduler is allowed to look at, and what it may say.
//!
//! Everything here is data. The types name the inputs one admission decision
//! rests on and the vocabulary it answers in, and nothing in this module — or in
//! [`crate::ready`] — reads a clock, a database, a filesystem or an environment
//! variable. The instant a freshness window is judged against arrives as
//! [`SchedulingSnapshot::taken_at`], and everything else arrives with it.
//!
//! ## What the scheduler deliberately does not know
//!
//! No type here carries a work-profile id, a phase name, a role name, a seed
//! profile id or a source kind, and no function in this crate branches on one.
//! Routing is *pinned* before a candidate reaches the scheduler
//! ([`Candidate::runtime`], [`Candidate::account`]), so admitting work under a
//! deployment's own profile is the same code path as admitting work under a
//! bundled one. `tests/no_seed_branching.rs` asserts that against the source.
//!
//! ## Trust arrives snapshotted, and is re-proved where it can be
//!
//! [`RuntimeAdmissionEvidence`] and [`AccountPin`] are plain values, so a caller
//! holding one can clone it and write a better trust grade into the copy. That is
//! the same limitation `kontor_runtime::RuntimeBindingSnapshot` documents, and it
//! has the same answer: a pure function cannot prove provenance, so the
//! *consequences* are bound elsewhere. The scheduler's own refusals are
//! structural — an undeclared capability, a stale revision, a disabled account —
//! and the facts a fabricated snapshot would most want to move (the task's
//! revision, state, dependencies, conflicts and the leases themselves) are
//! re-read inside the admission transaction, where a caller supplies nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use kontor_core::calendar::{EffectiveCalendarState, IanaTimeZone, WorkScope};
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CalendarProfileId, ConnectorKey, ContentHash,
    ExecutionAuthorizationId, ExternalId, ExternalName, IntakeReceiptId, MiniProjectId, ModuleKey,
    ProjectId, RuntimeKindKey, ScheduleOverrideId, SchemaVersion, SemanticMilestoneKey,
    SpecVersion, StatusConflictId, TaskId, TaskWorkflowId, Timestamp, validate_open_key,
};
use kontor_core::spec::IntakeResult;
use kontor_core::state::TaskState;
use kontor_core::{DomainError, DomainResult, closed_enum};
use kontor_policy::ModuleClaim;
use kontor_runtime::{RuntimeCapabilities, RuntimeCapability};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use uuid::Uuid;

/// The largest scheduling priority a candidate may carry.
///
/// The same bound [`kontor_core::spec::TriggerLimits`] validates, because it is
/// the same number: a trigger declares the priority the work it creates is
/// ordered by, and a candidate the scheduler sorts is that work.
pub const MAX_PRIORITY: u32 = 1000;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Identifies one immutable admission decision.
///
/// It belongs to this crate rather than to [`kontor_core::id`] for the same
/// reason `kontor_policy`'s ids do: the decision is this layer's aggregate, and
/// the store records it. The rules are the domain's own — version 7, canonical
/// lowercase text, parsed rather than trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdmissionEventId(Uuid);

impl AdmissionEventId {
    /// Mint a new time-ordered identifier.
    #[must_use]
    pub fn generate() -> Self {
        Self(kontor_core::id::generate_uuid_v7())
    }

    /// Parse a stored identifier.
    ///
    /// # Errors
    /// Rejects anything that is not a canonical version 7 UUID.
    pub fn parse(text: &str) -> DomainResult<Self> {
        let parsed = Uuid::parse_str(text)
            .map_err(|_| DomainError::invalid("AdmissionEventId", "is not a UUID"))?;
        if parsed.get_version_num() != 7 {
            return Err(DomainError::invalid(
                "AdmissionEventId",
                "is not a version 7 UUID",
            ));
        }
        if parsed.hyphenated().to_string() != text {
            return Err(DomainError::invalid(
                "AdmissionEventId",
                "is not in canonical lowercase hyphenated form",
            ));
        }
        Ok(Self(parsed))
    }
}

impl fmt::Display for AdmissionEventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.as_hyphenated(), f)
    }
}

impl Serialize for AdmissionEventId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AdmissionEventId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Account capability keys
// ---------------------------------------------------------------------------

/// One capability a coding account declares, or one a launch requires.
///
/// An open, deployment-defined key: the scheduler compares the required set
/// against the declared set and never looks at an individual value, so a
/// deployment's own capability vocabulary is checked exactly like a bundled one.
///
/// It is a key rather than a closed enum because the account capability surface
/// is deployment data — a provider tier, a model family, a quota class — and a
/// closed enum here would have to be migrated every time a provider changed its
/// offering.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountCapabilityKey(String);

impl AccountCapabilityKey {
    /// Parse and validate a capability key.
    ///
    /// # Errors
    /// As [`validate_open_key`]: the shared lexical rule for internal open
    /// keys.
    pub fn parse(text: &str) -> DomainResult<Self> {
        validate_open_key("AccountCapabilityKey", text)?;
        Ok(Self(text.to_owned()))
    }

    /// Borrow the key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountCapabilityKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for AccountCapabilityKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AccountCapabilityKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Calendar
// ---------------------------------------------------------------------------

/// The pinned calendar policy an answer was resolved against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarPolicyEvidence {
    /// The workspace-level calendar profile.
    pub profile_id: CalendarProfileId,
    /// The pinned revision of it.
    pub policy_revision: SpecVersion,
    /// The zone the window was evaluated in.
    pub timezone: IanaTimeZone,
    /// The window the instant matched, when it matched one. Opaque text: the
    /// scheduler records it and never parses it.
    pub matched_window: Option<ExternalName>,
}

/// The resolved calendar answer, as the scheduler consumes it.
///
/// **The scheduler never resolves a calendar.** It parses no ICS, no holiday
/// feed, no time zone and no weekly window; KON-MVP-21 owns resolution and hands
/// the answer over already made. What this type adds to
/// [`EffectiveCalendarState`] is the evidence the admission decision persists, so
/// a reviewer can see which policy revision and which window an admission was
/// judged against rather than only that it was "open".
///
/// The five shapes the brief names are field combinations rather than a second
/// five-value enum, because a second enum would be a second vocabulary that can
/// drift from the core's. [`CalendarAdmission::validate`] is what makes the
/// combinations exact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAdmission {
    /// What the calendar says.
    pub state: EffectiveCalendarState,
    /// The policy the answer came from. `None` exactly when nothing restricts
    /// execution.
    pub policy: Option<CalendarPolicyEvidence>,
    /// The approved override in force. `Some` exactly when the state is
    /// `override_open`.
    pub override_id: Option<ScheduleOverrideId>,
    /// When the calendar next opens, when it is closed and that is known.
    pub next_opening: Option<Timestamp>,
}

impl CalendarAdmission {
    /// The answer for a project with no calendar at all.
    #[must_use]
    pub const fn unrestricted() -> Self {
        Self {
            state: EffectiveCalendarState::Unrestricted,
            policy: None,
            override_id: None,
            next_opening: None,
        }
    }

    /// Whether this answer admits a *new* top-level run.
    ///
    /// `draining` does not. It means the window is about to close, so work that
    /// is already running is allowed to finish and nothing new joins it — which
    /// is the whole reason `draining` is a state of its own rather than a shade
    /// of `open`.
    #[must_use]
    pub const fn admits_new_work(&self) -> bool {
        matches!(
            self.state,
            EffectiveCalendarState::Unrestricted
                | EffectiveCalendarState::Open
                | EffectiveCalendarState::OverrideOpen
        )
    }

    /// Prove the answer's parts agree with each other.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for an unrestricted answer that names a
    /// policy, a restricted one that names none, an override id on any state but
    /// `override_open`, and an `override_open` that names no override.
    pub fn validate(&self) -> DomainResult<()> {
        let unrestricted = self.state == EffectiveCalendarState::Unrestricted;
        if unrestricted != self.policy.is_none() {
            return Err(DomainError::invalid(
                "CalendarAdmission",
                "a policy is recorded exactly when a calendar restricts execution",
            ));
        }
        let overridden = self.state == EffectiveCalendarState::OverrideOpen;
        if overridden != self.override_id.is_some() {
            return Err(DomainError::invalid(
                "CalendarAdmission",
                "an override id is recorded exactly when an override is in force",
            ));
        }
        if self.next_opening.is_some() && self.state != EffectiveCalendarState::Closed {
            return Err(DomainError::invalid(
                "CalendarAdmission",
                "only a closed calendar has a next opening",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Authorization
// ---------------------------------------------------------------------------

/// The bounded authorization that armed a candidate, when a grant is attached.
///
/// Absence is default-allow. An attached grant only *narrows*: the scheduler
/// re-checks the window, the scope and the selection rather than re-deriving
/// the grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationEvidence {
    /// The authorization.
    pub id: ExecutionAuthorizationId,
    /// The project it was granted in.
    pub project_id: ProjectId,
    /// What it covers.
    pub scope: WorkScope,
    /// The specific tasks it arms. Empty means the whole scope.
    pub selected_tasks: BTreeSet<TaskId>,
    /// The first instant work may start under it.
    pub allowed_start: Timestamp,
    /// The last instant work may start under it.
    pub allowed_end: Timestamp,
    /// The concurrency it authorizes.
    pub max_concurrency: u32,
}

impl AuthorizationEvidence {
    /// Whether this authorization arms `task` at `now`.
    #[must_use]
    pub fn arms(
        &self,
        project_id: ProjectId,
        mini_project_id: Option<MiniProjectId>,
        task_id: TaskId,
        now: Timestamp,
    ) -> bool {
        self.project_id == project_id
            && self.scope.covers(mini_project_id, Some(task_id))
            && (self.selected_tasks.is_empty() || self.selected_tasks.contains(&task_id))
            && now >= self.allowed_start
            && now <= self.allowed_end
    }
}

/// The grant that should travel with a candidate, or the disarm that stops it.
///
/// Active grants that *scope-cover* the task are attached even when
/// `selected_tasks` excludes it — that is how a whitelist surfaces
/// [`RejectionCode::AuthorizationScopeMismatch`] instead of disappearing into
/// default-allow. When no active grant covers the scope, a revoked covering
/// grant is a stop, not a return to unarmed.
#[must_use]
pub fn covering_authority(
    active: &[AuthorizationEvidence],
    revoked: &[AuthorizationEvidence],
    mini_project_id: Option<MiniProjectId>,
    task_id: TaskId,
) -> (
    Option<AuthorizationEvidence>,
    Option<ExecutionAuthorizationId>,
) {
    let in_scope = |authorization: &&AuthorizationEvidence| {
        authorization.scope.covers(mini_project_id, Some(task_id))
    };
    let rank = |authorization: &AuthorizationEvidence| match authorization.scope {
        WorkScope::Task { .. } => 0_u8,
        WorkScope::MiniProject { .. } => 1,
        WorkScope::Project => 2,
    };
    if let Some(grant) = active
        .iter()
        .filter(in_scope)
        .min_by_key(|grant| rank(grant))
    {
        return (Some(grant.clone()), None);
    }
    (
        None,
        revoked
            .iter()
            .filter(in_scope)
            .min_by_key(|grant| rank(grant))
            .map(|authorization| authorization.id),
    )
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

closed_enum! {
    /// What the runtime module last said about a runtime's own health.
    ///
    /// This is about the *runtime*, not about any run inside it, and it is never
    /// derived from a run's state: a failing run does not make a runtime
    /// unhealthy, and a healthy runtime does not make a lost run finished.
    RuntimeHealth, "RuntimeHealth" {
        /// Answering, and answering correctly.
        Healthy => "healthy",
        /// Answering, but not well enough to be driven.
        Degraded => "degraded",
        /// Not answering.
        Unavailable => "unavailable",
    }
}

/// The exact runtime generation a reconciliation census covered.
///
/// A census of a different host, or of the same host in an earlier generation,
/// proves nothing about the runtime a launch is about to be routed to — a
/// restarted runtime reissues native ids, so a stale census is worse than none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationScope {
    /// The project the census ran for.
    pub project_id: ProjectId,
    /// The runtime family.
    pub runtime_kind: RuntimeKindKey,
    /// The host or endpoint that owns the generation.
    pub host: ExternalName,
    /// The generation the census covered.
    pub generation: u64,
}

/// What startup reconciliation concluded about the selected runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationEvidence {
    /// Whether a startup census completed at all.
    pub epoch_completed: bool,
    /// What that census covered.
    pub scope: ReconciliationScope,
    /// Whether a replay gap is still open.
    pub open_replay_gap: bool,
    /// Whether intent and observation still disagree somewhere.
    pub divergence: bool,
    /// Whether a native session's ownership is still ambiguous.
    pub orphan_ambiguity: bool,
    /// Whether contact with a session was lost and never resolved.
    ///
    /// It blocks *new* admissions and, deliberately, implies nothing about the
    /// run that lost contact: an absence is not a completion and not a failure.
    pub stale_lost_contact: bool,
}

/// Everything the scheduler reads about the runtime a launch is pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAdmissionEvidence {
    /// The runtime family the pinned routing selected.
    pub runtime_kind: RuntimeKindKey,
    /// The host or endpoint.
    pub host: ExternalName,
    /// The generation currently answering.
    pub generation: u64,
    /// What that runtime declares about itself, as `kontor-runtime` discovered
    /// it.
    pub capabilities: RuntimeCapabilities,
    /// The capabilities this launch actually needs.
    pub required: BTreeSet<RuntimeCapability>,
    /// The runtime's own health.
    pub health: RuntimeHealth,
    /// What startup reconciliation concluded.
    pub reconciliation: ReconciliationEvidence,
    /// When the runtime was last confirmed. `None` means never.
    pub last_confirmed_at: Option<Timestamp>,
}

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

closed_enum! {
    /// What a fleet or provider preflight concluded.
    ///
    /// There is no "unknown" that admits: an absent preflight is
    /// [`PreflightOutcome::Absent`] and blocks, because "we did not check" is not
    /// evidence that the provider will accept the work.
    PreflightOutcome, "PreflightOutcome" {
        /// The provider accepted a probe.
        Passed => "passed",
        /// The provider refused a probe.
        Failed => "failed",
        /// No probe was run.
        Absent => "absent",
    }
}

/// One fleet/provider preflight result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetPreflight {
    /// What it concluded.
    pub outcome: PreflightOutcome,
    /// Digest of the stored probe evidence.
    pub evidence_hash: ContentHash,
    /// When the probe ran. Judged against the same freshness window as runtime
    /// confirmation: a preflight from last week is not evidence about now.
    pub observed_at: Timestamp,
}

/// The coding account a candidate is pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPin {
    /// The profile.
    pub account_profile_id: AccountProfileId,
    /// The revision the pin was taken against.
    pub pinned_revision: AggregateRevision,
    /// The revision the profile carries now.
    ///
    /// Both are recorded rather than one compared value, so the evidence says
    /// *what moved* and not merely that something did.
    pub current_revision: AggregateRevision,
    /// Whether launches may select it.
    pub enabled: bool,
    /// The instant it stops cooling down, when it is cooling down.
    pub cooldown_until: Option<Timestamp>,
    /// The runtime family it authenticates against.
    pub harness: RuntimeKindKey,
    /// The capability keys it declares, read out of its non-secret capability
    /// document by `kontor-accounts`.
    pub declared_capabilities: BTreeSet<AccountCapabilityKey>,
    /// The non-secret provider identity it routes through, when one is recorded.
    /// Provider capacity is counted against this.
    pub provider_identity: Option<ExternalId>,
    /// The fleet/provider preflight behind it.
    pub preflight: FleetPreflight,
}

/// What the scheduler reads about the account side of a launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountAdmissionEvidence {
    /// The pinned account, when the work is account-pinned.
    ///
    /// `None` is legal and means the launch is not account-pinned; it does not
    /// mean "any account will do". An account-pinned run through a runtime that
    /// cannot prove the account environment is refused
    /// ([`RejectionCode::AccountEnvironmentUnavailable`]).
    pub pin: Option<AccountPin>,
    /// The capability keys this launch requires of its account.
    pub required_capabilities: BTreeSet<AccountCapabilityKey>,
}

// ---------------------------------------------------------------------------
// External ownership
// ---------------------------------------------------------------------------

/// What the external system says about a connector-linked task.
///
/// External status and assignee are **observations and gates**. Nothing here
/// replaces Kontor's own task state or ownership, and no admission decision
/// writes any of it: a foreign principal owning the ticket produces a reported
/// conflict, never a takeover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalOwnership {
    /// The connector the link belongs to.
    pub connector: ConnectorKey,
    /// The pinned external-workflow specification revision.
    pub spec_version: SpecVersion,
    /// The internal milestone that revision requires before dispatch.
    pub ownership_milestone: SemanticMilestoneKey,
    /// Whether the external ticket has been observed at that milestone.
    pub milestone_confirmed: bool,
    /// The principal the external system reports as owning the ticket.
    pub owning_principal: Option<ExternalId>,
    /// The principal Kontor would act as.
    pub acting_principal: ExternalId,
}

impl ExternalOwnership {
    /// Whether another principal owns the ticket.
    #[must_use]
    pub fn owned_by_another(&self) -> bool {
        self.owning_principal
            .as_ref()
            .is_some_and(|owner| owner != &self.acting_principal)
    }
}

/// The external-workflow evidence a candidate carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExternalWorkEvidence {
    /// Unresolved status conflicts whose recorded scheduling impact blocks
    /// dispatch. A conflict with no scheduling impact is not in this list.
    pub blocking_conflicts: BTreeSet<StatusConflictId>,
    /// The ownership gate, when the task is connector-linked.
    pub ownership: Option<ExternalOwnership>,
}

// ---------------------------------------------------------------------------
// Origin
// ---------------------------------------------------------------------------

/// The durable intake lineage of an event-origin task.
///
/// The scheduler receives the receipt's *identity and status* and nothing else.
/// It never reads a source envelope, never normalizes an event, never re-matches
/// a trigger filter and never branches on a source kind — which is why no field
/// here can carry one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeLineage {
    /// The receipt.
    pub receipt_id: IntakeReceiptId,
    /// What it decided.
    pub result: IntakeResult,
    /// The task the receipt actually armed.
    ///
    /// Compared with the candidate: a receipt that armed different work is a
    /// mismatched lineage, not this task's authority.
    pub armed_task_id: TaskId,
    /// The bounded auto-arm authorization the trigger acted under, when it was
    /// explicitly authorized to arm work by itself.
    pub auto_arm_authorization: Option<ExecutionAuthorizationId>,
}

/// Where a candidate came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskOrigin {
    /// An operator created it. It needs no intake receipt, and asking for one
    /// would make manual work unschedulable.
    Manual,
    /// A trigger created it. Eligible only through its durable intake lineage.
    Event {
        /// The lineage, when it resolves to a receipt at all. `None` is an
        /// absent lineage and blocks.
        lineage: Option<IntakeLineage>,
    },
}

impl TaskOrigin {
    /// Whether this origin authorizes admission for `task_id`.
    ///
    /// Manual work needs nothing. Event-origin work needs a receipt that armed
    /// *this* task and either approved it or armed it under an explicit
    /// auto-arm authorization. A rejected, ignored or duplicate receipt never
    /// arms work, whatever authorization accompanies it: a duplicate in
    /// particular is the one decision that exists to prevent a second work
    /// graph.
    pub fn admits(&self, task_id: TaskId) -> Result<(), RejectionCode> {
        let Self::Event { lineage } = self else {
            return Ok(());
        };
        let Some(lineage) = lineage else {
            return Err(RejectionCode::IntakeReceiptMissing);
        };
        if lineage.armed_task_id != task_id {
            return Err(RejectionCode::IntakeReceiptMismatched);
        }
        match lineage.result {
            IntakeResult::Approved => Ok(()),
            IntakeResult::Proposed if lineage.auto_arm_authorization.is_some() => Ok(()),
            IntakeResult::Proposed
            | IntakeResult::Rejected
            | IntakeResult::Ignored
            | IntakeResult::Duplicate => Err(RejectionCode::IntakeReceiptNotApproved),
        }
    }
}

// ---------------------------------------------------------------------------
// Worktree
// ---------------------------------------------------------------------------

closed_enum! {
    /// Whether a worktree identity has been proved to exist and to be this
    /// task's.
    WorktreeVerification, "WorktreeVerification" {
        /// The workspace layer verified this exact tree for this work.
        Verified => "verified",
        /// A tree is claimed but nothing has proved it.
        Unverified => "unverified",
    }
}

/// The worktree a candidate claims, when it claims one.
///
/// An unverified claim is *carried* rather than dropped, so it can be refused
/// with its own reason: "this claim was never verified" and "this work has no
/// tree at all" are different failures, and treating the first as the second
/// would let a fabricated path buy isolation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeClaim {
    /// The tree identity leases are keyed on.
    pub worktree: ExternalName,
    /// Whether it has been verified.
    pub verification: WorktreeVerification,
}

impl WorktreeClaim {
    /// The identity, when it may be relied on for isolation.
    #[must_use]
    pub const fn verified(&self) -> Option<&ExternalName> {
        match self.verification {
            WorktreeVerification::Verified => Some(&self.worktree),
            WorktreeVerification::Unverified => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Capacity
// ---------------------------------------------------------------------------

closed_enum! {
    /// One ceiling the scheduler obeys.
    ///
    /// Every value is a *configured* bound or a bound a runtime declared about
    /// itself. There is no compiled concurrency number anywhere in this crate,
    /// and no ceiling that exists only for one deployment's shape.
    CapacityLimitKind, "CapacityLimitKind" {
        /// Across the whole Realm.
        Global => "global",
        /// Across one project.
        Project => "project",
        /// Across one goal.
        Mission => "mission",
        /// Across one coding account.
        Account => "account",
        /// Across one provider identity.
        Provider => "provider",
        /// Across one runtime family, as configuration bounds it.
        Runtime => "runtime",
        /// Across one runtime family, as the runtime declares it.
        RuntimeSessions => "runtime_sessions",
        /// The concurrency the arming authorization granted.
        ///
        /// Spelled `arming_authorization` rather than `authorization` because this
        /// value is a *map key* in [`CapacitySnapshot::remaining`], and a persisted
        /// document may not carry a key called `authorization` — that is the name of
        /// an HTTP credential header, so
        /// [`kontor_core::id::reject_sensitive_material`] refuses it.
        Authorization => "arming_authorization",
        /// The adaptive window currently in force.
        AdaptiveWindow => "adaptive_window",
    }
}

/// Every ceiling, from configuration.
///
/// The numbers are a deployment's, never this crate's. A profile does not get its
/// own branch here and there is no compiled default: a caller that wants
/// yesterday's 7/4/2 shape configures 7, 4 and 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityConfig {
    /// Simultaneous admitted runs across the Realm.
    pub global_max_in_flight: u32,
    /// Simultaneous admitted runs in one project.
    pub project_max_in_flight: u32,
    /// Simultaneous admitted runs in one goal.
    pub mission_max_in_flight: u32,
    /// Simultaneous admitted runs on one account.
    pub account_max_in_flight: u32,
    /// Simultaneous admitted runs on one provider identity.
    pub provider_max_in_flight: u32,
    /// Simultaneous admitted runs on one runtime family.
    pub runtime_max_in_flight: u32,
    /// How the adaptive window moves.
    pub adaptive: AdaptiveWindowConfig,
    /// The provider-headroom policy, when this deployment has declared one.
    ///
    /// `None` is not a permissive default dressed up as absence — it is the
    /// honest state of a realm configured before OP-REQ-042 existed, and a
    /// stored ceilings document written then must keep parsing rather than
    /// bricking the realm on upgrade. Selection then falls back to
    /// [`crate::headroom::HeadroomConfig::state_only`], which gates on the
    /// recorded provider state exactly as it did before and adds no window
    /// threshold nobody chose.
    #[serde(default)]
    pub headroom: Option<crate::headroom::HeadroomConfig>,
}

impl CapacityConfig {
    /// Validate every ceiling.
    ///
    /// # Errors
    /// Rejects a zero ceiling, which reads as "no work allowed" in one place and
    /// "no limit" in another — the same refusal
    /// [`kontor_core::spec::BudgetBounds`] makes.
    pub fn validate(&self) -> DomainResult<()> {
        let bounds = [
            self.global_max_in_flight,
            self.project_max_in_flight,
            self.mission_max_in_flight,
            self.account_max_in_flight,
            self.provider_max_in_flight,
            self.runtime_max_in_flight,
        ];
        if bounds.contains(&0) {
            return Err(DomainError::invalid(
                "CapacityConfig",
                "every ceiling must be positive",
            ));
        }
        if let Some(headroom) = self.headroom {
            headroom.validate()?;
        }
        self.adaptive.validate()
    }
}

/// How the adaptive window grows and shrinks.
///
/// The floor, the starting width and the ceiling are all configured. A
/// deployment that wants the historical "start at 4, fall back to 2, grow to 7"
/// behaviour writes those three numbers here; nothing in this crate knows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveWindowConfig {
    /// The width a fresh window starts at.
    pub initial: u32,
    /// The narrowest the window ever gets under pressure.
    pub floor: u32,
    /// The widest it ever grows on clean observations.
    pub ceiling: u32,
    /// How much one clean observation adds.
    pub growth_step: u32,
}

impl AdaptiveWindowConfig {
    /// Validate the window's bounds.
    ///
    /// # Errors
    /// Rejects a zero floor or growth step and any floor/initial/ceiling that is
    /// not ordered.
    pub fn validate(&self) -> DomainResult<()> {
        if self.floor == 0 || self.growth_step == 0 {
            return Err(DomainError::invalid(
                "AdaptiveWindowConfig",
                "the floor and the growth step must be positive",
            ));
        }
        if !(self.floor <= self.initial && self.initial <= self.ceiling) {
            return Err(DomainError::invalid(
                "AdaptiveWindowConfig",
                "the floor, the initial width and the ceiling must be ordered",
            ));
        }
        Ok(())
    }
}

closed_enum! {
    /// What the last window of admitted work looked like from outside.
    CapacityObservation, "CapacityObservation" {
        /// Everything admitted behaved: no throttling, no refusal, no timeout.
        Clean => "clean",
        /// Something pushed back.
        Pressure => "pressure",
    }
}

/// The adaptive admission window.
///
/// It bounds how many *new* runs one pass may admit, and it is the one capacity
/// input that moves on its own. Two properties are deliberate:
///
/// * it never cancels admitted work. There is no method here that can: pressure
///   narrows the window, and a narrowed window means the next pass admits less,
///   not that something already running stops;
/// * pressure goes straight to the floor rather than stepping down. A provider
///   that is refusing work is not helped by a scheduler that keeps most of its
///   concurrency for another few passes, and the cost of over-correcting is one
///   slower pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveWindow {
    current: u32,
}

impl AdaptiveWindow {
    /// A window at its configured starting width.
    #[must_use]
    pub const fn start(config: AdaptiveWindowConfig) -> Self {
        Self {
            current: config.initial,
        }
    }

    /// Re-admit a persisted width, clamped into the configured band.
    ///
    /// Clamping rather than refusing: a configuration change that narrows the
    /// ceiling must take effect on the next pass, not fail the pass.
    #[must_use]
    pub fn restore(config: AdaptiveWindowConfig, width: u32) -> Self {
        Self {
            current: width.clamp(config.floor, config.ceiling),
        }
    }

    /// The width in force.
    #[must_use]
    pub const fn current(self) -> u32 {
        self.current
    }

    /// Fold one observation in.
    #[must_use]
    pub fn observe(self, config: AdaptiveWindowConfig, observation: CapacityObservation) -> Self {
        let current = match observation {
            CapacityObservation::Clean => self
                .current
                .saturating_add(config.growth_step)
                .min(config.ceiling),
            CapacityObservation::Pressure => config.floor,
        };
        Self {
            current: current.clamp(config.floor, config.ceiling),
        }
    }
}

/// How much of each ceiling is already spent.
///
/// Everything is a `BTreeMap` rather than a hash map so that a pass over the same
/// usage produces the same order, the same decisions and the same bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CapacityUsage {
    /// Admitted runs across the Realm.
    pub global_in_flight: u32,
    /// Admitted runs per project.
    pub project_in_flight: BTreeMap<ProjectId, u32>,
    /// Admitted runs per goal.
    pub mission_in_flight: BTreeMap<MiniProjectId, u32>,
    /// Admitted runs per account.
    pub account_in_flight: BTreeMap<AccountProfileId, u32>,
    /// Admitted runs per provider identity.
    pub provider_in_flight: BTreeMap<ExternalId, u32>,
    /// Admitted runs per runtime family.
    pub runtime_in_flight: BTreeMap<RuntimeKindKey, u32>,
}

/// The headroom under every ceiling at the moment one candidate was decided.
///
/// Persisted with the admission, so "why was only one of four admitted" is
/// answerable from the record rather than by re-deriving the whole pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacitySnapshot {
    /// Remaining headroom under each ceiling.
    pub remaining: BTreeMap<CapacityLimitKind, u32>,
    /// The smallest of them, which is the one that decided.
    pub effective: u32,
    /// Which ceiling that was.
    pub binding: CapacityLimitKind,
}

// ---------------------------------------------------------------------------
// Candidates
// ---------------------------------------------------------------------------

/// One task the scheduler is deciding about, with everything a blocker reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// Owning goal, if any. Mission capacity is counted against this.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task's active workflow, whose pinned profile routed it.
    pub workflow_id: TaskWorkflowId,
    /// Lifecycle state.
    pub state: TaskState,
    /// The revision the decision is computed against. The admission transaction
    /// re-checks it, so a task that moves between the snapshot and the commit is
    /// refused rather than admitted on stale facts.
    pub revision: AggregateRevision,
    /// When the task was created. The second sort key.
    pub created_at: Timestamp,
    /// Scheduling priority, higher first. The first sort key.
    pub priority: u32,
    /// The module the task contends for, if any.
    pub module: Option<ModuleKey>,
    /// Additional modules this task changes, besides [`Self::module`].
    ///
    /// Empty on older persisted admission evidence. Admission takes a lease for
    /// every key in the union of this set and [`Self::module`].
    #[serde(default)]
    pub changed_modules: BTreeSet<ModuleKey>,
    /// The worktree it claims, if any.
    pub worktree: Option<WorktreeClaim>,
    /// Tasks that must be `done` first.
    pub depends_on: BTreeSet<TaskId>,
    /// Tasks this one may not run beside.
    pub serializes_with: BTreeSet<TaskId>,
    /// Where the task came from.
    pub origin: TaskOrigin,
    /// The authorization that armed it, if any.
    ///
    /// Absence is default-allow: registered project resources may run. A later
    /// explicit grant only *narrows* (window, concurrency, selected tasks).
    pub authorization: Option<AuthorizationEvidence>,
    /// An explicit disarm that still covers this task.
    ///
    /// Disarm is a stop, not a return to unarmed. Unarmed admits; a revoked
    /// covering grant refuses until a new active grant replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<ExecutionAuthorizationId>,
    /// The already-resolved calendar answer.
    pub calendar: CalendarAdmission,
    /// The pinned runtime.
    pub runtime: RuntimeAdmissionEvidence,
    /// The pinned account.
    pub account: AccountAdmissionEvidence,
    /// External-workflow gates.
    pub external: ExternalWorkEvidence,
}

fn integration_modules(
    primary: Option<&ModuleKey>,
    extras: &BTreeSet<ModuleKey>,
) -> BTreeSet<ModuleKey> {
    let mut modules = BTreeSet::<ModuleKey>::new();
    if let Some(primary) = primary {
        modules.insert(primary.clone());
    }
    for extra in extras {
        if modules.iter().any(|existing| existing.contends_with(extra)) {
            continue;
        }
        modules.insert(extra.clone());
    }
    modules
}

impl Candidate {
    /// The deterministic sort key: priority descending, then age, then id.
    ///
    /// The id is the last resort rather than a tie-break of convenience: two
    /// tasks created in the same millisecond with the same priority must still
    /// have exactly one order, on every machine and after every restart, and a
    /// UUIDv7 is the only field guaranteed to differ.
    #[must_use]
    pub fn ordering(&self) -> OrderingInputs {
        OrderingInputs {
            priority: self.priority,
            created_at: self.created_at,
            task_id: self.task_id,
        }
    }

    /// The provider capacity this candidate consumes, when it is known.
    #[must_use]
    pub fn provider(&self) -> Option<&ExternalId> {
        self.account
            .pin
            .as_ref()
            .and_then(|pin| pin.provider_identity.as_ref())
    }

    /// The module claim this candidate would hold, as the guardrail rule reads
    /// one.
    ///
    /// Only a *verified* tree isolates. An unverified claim produces a claim with
    /// no tree, which contends with everything — and is refused earlier anyway,
    /// with its own reason.
    #[must_use]
    pub fn module_claim(&self) -> Option<ModuleClaim> {
        self.module.as_ref().map(|module| ModuleClaim {
            module: module.clone(),
            task_id: self.task_id,
            worktree: self
                .worktree
                .as_ref()
                .and_then(WorktreeClaim::verified)
                .cloned(),
            in_flight: true,
        })
    }

    /// Validate what can be judged about one candidate on its own.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for an out-of-range priority or a task
    /// that depends on, or serializes with, itself; and whatever
    /// [`CalendarAdmission::validate`] refuses.
    pub fn validate(&self) -> DomainResult<()> {
        if self.priority > MAX_PRIORITY {
            return Err(DomainError::invalid(
                "Candidate",
                "priority is out of range",
            ));
        }
        if self.depends_on.contains(&self.task_id) {
            return Err(DomainError::invalid(
                "Candidate",
                "a task must not depend on itself",
            ));
        }
        if self.serializes_with.contains(&self.task_id) {
            return Err(DomainError::invalid(
                "Candidate",
                "a task must not serialize against itself",
            ));
        }
        self.calendar.validate()
    }

    /// Every module this candidate must take a lease on.
    #[must_use]
    pub fn integration_modules(&self) -> BTreeSet<ModuleKey> {
        integration_modules(self.module.as_ref(), &self.changed_modules)
    }

    /// The module claims this candidate would hold, as the guardrail rule reads
    /// them.
    #[must_use]
    pub fn module_claims(&self) -> Vec<ModuleClaim> {
        let worktree = self
            .worktree
            .as_ref()
            .and_then(WorktreeClaim::verified)
            .cloned();
        self.integration_modules()
            .into_iter()
            .map(|module| ModuleClaim {
                module,
                task_id: self.task_id,
                worktree: worktree.clone(),
                in_flight: true,
            })
            .collect()
    }
}

/// The exact values a candidate was ordered on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderingInputs {
    /// Priority, higher first.
    pub priority: u32,
    /// Creation instant, older first.
    pub created_at: Timestamp,
    /// The task id, ascending.
    pub task_id: TaskId,
}

impl OrderingInputs {
    /// The total order, as a comparable tuple.
    ///
    /// Priority is negated rather than reversed at the call site so that every
    /// caller sorts the same way by construction.
    fn sort_key(&self) -> (std::cmp::Reverse<u32>, Timestamp, TaskId) {
        (
            std::cmp::Reverse(self.priority),
            self.created_at,
            self.task_id,
        )
    }
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

closed_enum! {
    /// Why a candidate was not admitted.
    ///
    /// One code per distinguishable refusal, and the spellings are stable: an
    /// operator reads the reason without re-running the pass, and a test asserts
    /// the *reason* rather than only that something was refused — which is what
    /// stops a blocker from being right by accident.
    ///
    /// The declaration order is the evaluation order
    /// ([`crate::ready::BLOCKER_ORDER`]), and it is not arbitrary: the cheapest
    /// and most fundamental facts come first, so a task that is not even ready is
    /// never reported as a capacity problem.
    RejectionCode, "RejectionCode" {
        /// The task is not in `ready`.
        TaskNotReady => "task_not_ready",
        /// The task already has work in flight.
        ///
        /// Distinct from [`RejectionCode::TaskNotReady`] on purpose. A task whose
        /// envelope is already running may still *read* as `ready` — the task's
        /// own lifecycle and its runs are orthogonal dimensions — and admitting a
        /// second envelope for it is the one double-admission no module lease
        /// catches, because a task never contends with itself for its own module.
        TaskAlreadyInFlight => "task_already_in_flight",
        /// A dependency has not finished.
        DependencyIncomplete => "dependency_incomplete",
        /// A task it may not run beside is in flight or already selected.
        SerializationPeerInFlight => "serialization_peer_in_flight",
        /// Another task holds its module without worktree isolation.
        ModuleInFlight => "module_in_flight",
        /// It claims a worktree nothing verified.
        WorktreeUnverified => "worktree_unverified",
        /// Another selected candidate claims the same worktree.
        WorktreeDuplicate => "worktree_duplicate",
        /// Nothing armed it.
        ///
        /// Kept for stored refusals. New passes do not emit this for ordinary
        /// unarmed work: absence of a grant is default-allow.
        AuthorizationMissing => "authorization_missing",
        /// An operator explicitly disarmed a grant that still covers this task.
        AuthorizationBlocked => "authorization_blocked",
        /// Its authorization does not cover this task.
        AuthorizationScopeMismatch => "authorization_scope_mismatch",
        /// Its authorization's window has passed or has not opened.
        AuthorizationExpired => "authorization_expired",
        /// The runtime does not declare a capability the launch needs.
        RuntimeCapabilityMissing => "runtime_capability_missing",
        /// The runtime's trust grade may not be driven autonomously.
        RuntimeTrustInsufficient => "runtime_trust_insufficient",
        /// The runtime is degraded or unavailable.
        RuntimeUnhealthy => "runtime_unhealthy",
        /// Startup reconciliation has not completed for this exact runtime.
        RuntimeReconciliationIncomplete => "runtime_reconciliation_incomplete",
        /// The newest runtime confirmation is older than the freshness window.
        RuntimeEvidenceStale => "runtime_evidence_stale",
        /// The pinned profile revision is not the one the profile carries now.
        AccountPinStale => "account_pin_stale",
        /// The pinned account is disabled.
        AccountDisabled => "account_disabled",
        /// The pinned account is cooling down.
        AccountCoolingDown => "account_cooling_down",
        /// The pinned account authenticates against another runtime family.
        AccountRuntimeIncompatible => "account_runtime_incompatible",
        /// The pinned account does not declare a capability the launch needs.
        AccountCapabilityMissing => "account_capability_missing",
        /// The runtime cannot prove the account environment of a pinned run.
        AccountEnvironmentUnavailable => "account_environment_unavailable",
        /// The fleet/provider preflight failed, is absent or is stale.
        FleetPreflightFailed => "fleet_preflight_failed",
        /// An unresolved external status conflict blocks scheduling.
        ExternalConflictUnresolved => "external_conflict_unresolved",
        /// Another principal owns the external ticket.
        ExternalOwnershipConflict => "external_ownership_conflict",
        /// The pinned ownership milestone has not been observed.
        OwnershipMilestoneUnconfirmed => "ownership_milestone_unconfirmed",
        /// The calendar is closed.
        CalendarClosed => "calendar_closed",
        /// The calendar window is draining, so no new run joins it.
        CalendarDraining => "calendar_draining",
        /// Event-origin work whose intake lineage resolves to nothing.
        IntakeReceiptMissing => "intake_receipt_missing",
        /// Event-origin work whose receipt neither approved nor auto-armed it.
        IntakeReceiptNotApproved => "intake_receipt_not_approved",
        /// Event-origin work whose receipt armed a different task.
        IntakeReceiptMismatched => "intake_receipt_mismatched",
        /// Every ceiling that applies is spent.
        CapacityExhausted => "capacity_exhausted",
    }
}

impl RejectionCode {
    /// The next CLI/MCP move a caller holding only this code can try.
    #[must_use]
    pub const fn next_action(self) -> &'static str {
        match self {
            Self::TaskNotReady => {
                "wait until the task is ready; kontor_execution_arm does not make it ready"
            }
            Self::TaskAlreadyInFlight => {
                "this task already has a run; inspect that seat or wait for it to settle"
            }
            Self::DependencyIncomplete => "wait for the named dependency to finish, then re-plan",
            Self::SerializationPeerInFlight => {
                "wait for the named peer to settle; these tasks may not run together"
            }
            Self::ModuleInFlight => {
                "wait for the module holder to settle, or isolate this task on its own worktree"
            }
            Self::WorktreeUnverified => "verify the worktree claim, then re-plan",
            Self::WorktreeDuplicate => {
                "wait for the other claim on this worktree to settle, then re-plan"
            }
            Self::AuthorizationMissing => {
                "work now runs without kontor_execution_arm; re-plan, or ignore a stored copy of this code"
            }
            Self::AuthorizationBlocked => {
                "the epic was disarmed; call kontor_execution_arm (omit budget, allowed_start, allowed_end, max_concurrency) to resume, or leave it stopped"
            }
            Self::AuthorizationScopeMismatch => {
                "the active grant excludes this task; arm it with kontor_execution_arm, or disarm that whitelist with kontor_execution_disarm"
            }
            Self::AuthorizationExpired => {
                "the grant's window does not cover now; re-arm with kontor_execution_arm omitting allowed_start and allowed_end"
            }
            Self::RuntimeCapabilityMissing => {
                "bind a runtime that declares the missing capability, then re-plan"
            }
            Self::RuntimeTrustInsufficient => {
                "bind a runtime whose trust grade may be driven autonomously"
            }
            Self::RuntimeUnhealthy => "wait until the runtime is healthy, then re-plan",
            Self::RuntimeReconciliationIncomplete => {
                "wait for startup reconciliation to finish, then re-plan"
            }
            Self::RuntimeEvidenceStale => "refresh the runtime confirmation, then re-plan",
            Self::AccountPinStale => {
                "re-read the account pin and retry with the revision it reports"
            }
            Self::AccountDisabled => "enable the pinned account, or pin a different one",
            Self::AccountCoolingDown => {
                "wait until the pinned account's cooldown ends, then re-plan"
            }
            Self::AccountRuntimeIncompatible => {
                "pin an account that authenticates against this runtime family"
            }
            Self::AccountCapabilityMissing => {
                "pin an account that declares the missing capability, then re-plan"
            }
            Self::AccountEnvironmentUnavailable => {
                "bind a runtime that can prove the pinned account environment"
            }
            Self::FleetPreflightFailed => "fix the fleet/provider preflight, then re-plan",
            Self::ExternalConflictUnresolved => {
                "resolve the named external-status conflict, then re-plan"
            }
            Self::ExternalOwnershipConflict => {
                "the external ticket is owned by another principal; do not start this task"
            }
            Self::OwnershipMilestoneUnconfirmed => {
                "wait until the pinned ownership milestone is observed, then re-plan"
            }
            Self::CalendarClosed => {
                "wait for the calendar to open, or set an override, then re-plan"
            }
            Self::CalendarDraining => {
                "wait for the next calendar window; draining admits no new run"
            }
            Self::IntakeReceiptMissing => {
                "event-origin work needs its intake receipt; restore the lineage or do not start it"
            }
            Self::IntakeReceiptNotApproved => {
                "the intake receipt did not approve this work; approve it or do not start it"
            }
            Self::IntakeReceiptMismatched => {
                "the intake receipt armed a different task; do not start this one under it"
            }
            Self::CapacityExhausted => {
                "retry when in-flight work finishes; nothing was refused about the request itself"
            }
        }
    }
}

/// Where the proof behind a refusal is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RejectionEvidence {
    /// A dependency that has not finished.
    Dependency {
        /// The unfinished task.
        task_id: TaskId,
    },
    /// A peer that may not run beside this work.
    SerializationPeer {
        /// The peer.
        task_id: TaskId,
    },
    /// A module held by other work.
    ModuleHeld {
        /// The module.
        module: ModuleKey,
        /// The task holding it.
        task_id: TaskId,
    },
    /// A worktree identity.
    Worktree {
        /// The tree.
        worktree: ExternalName,
    },
    /// The authorization that was examined.
    Authorization {
        /// The authorization.
        id: ExecutionAuthorizationId,
    },
    /// The runtime that was examined.
    Runtime {
        /// The family.
        runtime_kind: RuntimeKindKey,
        /// The generation.
        generation: u64,
    },
    /// A capability that was required and not declared.
    MissingRuntimeCapability {
        /// The capability.
        capability: RuntimeCapability,
    },
    /// An account capability that was required and not declared.
    MissingAccountCapability {
        /// The capability key.
        capability: AccountCapabilityKey,
    },
    /// The account that was examined.
    Account {
        /// The profile.
        id: AccountProfileId,
    },
    /// An unresolved external conflict.
    Conflict {
        /// The conflict.
        id: StatusConflictId,
    },
    /// The intake receipt that was examined.
    IntakeReceipt {
        /// The receipt.
        id: IntakeReceiptId,
    },
    /// The calendar answer that refused.
    Calendar {
        /// What it said.
        state: EffectiveCalendarState,
        /// When it next opens, when that is known.
        next_opening: Option<Timestamp>,
    },
    /// The ceiling that was spent.
    Capacity {
        /// Which ceiling.
        limit: CapacityLimitKind,
        /// How much headroom it had.
        remaining: u32,
    },
}

/// One candidate's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum CandidateDecision {
    /// Admitted, and this is what it was admitted on.
    Admit(Box<AdmittedCandidate>),
    /// Refused, and this is why.
    Reject {
        /// The task.
        task_id: TaskId,
        /// Owning project.
        project_id: ProjectId,
        /// The first blocker in [`crate::ready::BLOCKER_ORDER`] that refused.
        code: RejectionCode,
        /// Where the proof is.
        evidence: Vec<RejectionEvidence>,
    },
}

impl CandidateDecision {
    /// The task this decision is about.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        match self {
            Self::Admit(admitted) => admitted.task_id,
            Self::Reject { task_id, .. } => *task_id,
        }
    }

    /// The refusal code, when the decision is a refusal.
    #[must_use]
    pub const fn rejection_code(&self) -> Option<RejectionCode> {
        match self {
            Self::Admit(_) => None,
            Self::Reject { code, .. } => Some(*code),
        }
    }
}

/// One admitted candidate and the evidence its admission rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmittedCandidate {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// The task revision the admission was computed against.
    pub revision: AggregateRevision,
    /// Its active workflow.
    pub workflow_id: TaskWorkflowId,
    /// The values it was ordered on.
    pub ordering: OrderingInputs,
    /// The headroom it fitted into.
    pub capacity: CapacitySnapshot,
    /// The module lease the admission must acquire, if any.
    pub module: Option<ModuleKey>,
    /// Additional module leases the admission must acquire.
    ///
    /// Empty on older persisted evidence. Never contains a key that contends
    /// with [`Self::module`].
    #[serde(default)]
    pub changed_modules: BTreeSet<ModuleKey>,
    /// The verified worktree lease the admission must acquire, if any.
    pub worktree: Option<ExternalName>,
    /// The authorization that armed it, when a grant narrowed the run.
    ///
    /// `None` is default-allow: the task was admitted because nothing blocked it,
    /// not because a money ceiling was invented.
    ///
    /// The field is `authorization_id` and not `authorization` on purpose: a
    /// persisted document may not carry a key named `authorization`
    /// ([`kontor_core::id::reject_sensitive_material`] refuses it, since that is
    /// what an HTTP credential header is called), and this record *is* persisted
    /// as the admission's canonical evidence.
    pub authorization_id: Option<ExecutionAuthorizationId>,
    /// The calendar answer it was admitted under.
    pub calendar: CalendarAdmission,
    /// The account it is pinned to, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// The runtime it is routed to.
    pub runtime_kind: RuntimeKindKey,
    /// The generation of that runtime.
    pub runtime_generation: u64,
    /// The intake receipt that armed it, for event-origin work.
    pub intake_receipt_id: Option<IntakeReceiptId>,
}

impl AdmittedCandidate {
    /// Every module this admission must take a lease on.
    #[must_use]
    pub fn integration_modules(&self) -> BTreeSet<ModuleKey> {
        integration_modules(self.module.as_ref(), &self.changed_modules)
    }
}

/// The ordered outcome of one pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Schema generation of this document, so a persisted plan is readable by
    /// exactly the binaries that understand it.
    pub schema_version: SchemaVersion,
    /// The instant the snapshot was taken.
    pub taken_at: Timestamp,
    /// Every candidate's decision, in the pass's own order.
    pub decisions: Vec<CandidateDecision>,
}

impl Plan {
    /// The admitted candidates, in the order they were selected.
    ///
    /// This is the launch batch: the decisions are the whole record and the
    /// batch is the part a dispatcher acts on, so there is one list and one
    /// projection of it rather than two lists that can disagree.
    pub fn batch(&self) -> impl Iterator<Item = &AdmittedCandidate> {
        self.decisions.iter().filter_map(|decision| match decision {
            CandidateDecision::Admit(admitted) => Some(admitted.as_ref()),
            CandidateDecision::Reject { .. } => None,
        })
    }

    /// How many candidates were admitted.
    #[must_use]
    pub fn admitted_count(&self) -> usize {
        self.batch().count()
    }
}

// ---------------------------------------------------------------------------
// The snapshot
// ---------------------------------------------------------------------------

/// One transactionally consistent view of everything a pass decides on.
///
/// It is taken once and read many times. A pass that re-read the store between
/// candidates could admit two tasks against one lease's worth of headroom
/// without either read being wrong, so the whole input arrives as one value —
/// and the *store* re-proves the parts that must not have moved when it commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingSnapshot {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The instant the snapshot is judged against. Every window, expiry and
    /// freshness check reads this and never a clock.
    pub taken_at: Timestamp,
    /// The candidates, in any order. The pass sorts them itself.
    pub candidates: Vec<Candidate>,
    /// Tasks with work in flight anywhere in the Realm.
    pub in_flight_tasks: BTreeSet<TaskId>,
    /// Tasks that have reached `done`, for dependency resolution.
    pub completed_tasks: BTreeSet<TaskId>,
    /// Module claims held by durable leases across every project in the Realm.
    ///
    /// Realm-wide, not project-local: a module is a place on disk, and disk does
    /// not know about project rows.
    pub module_leases: Vec<ModuleClaim>,
    /// Worktree identities held by durable leases across the Realm.
    pub worktree_leases: BTreeSet<ExternalName>,
    /// How much of each ceiling is spent.
    pub usage: CapacityUsage,
    /// Every ceiling.
    pub capacity: CapacityConfig,
    /// The adaptive window in force.
    pub adaptive_window: AdaptiveWindow,
    /// How old runtime confirmation and preflight evidence may be.
    pub freshness: jiff::SignedDuration,
}

impl SchedulingSnapshot {
    /// Validate the snapshot before a single decision is made.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a duplicate candidate, a
    /// non-positive freshness window, an invalid ceiling and anything
    /// [`Candidate::validate`] refuses.
    pub fn validate(&self) -> DomainResult<()> {
        self.capacity.validate()?;
        if self.freshness <= jiff::SignedDuration::ZERO {
            return Err(DomainError::invalid(
                "SchedulingSnapshot",
                "the freshness window must be positive",
            ));
        }
        let mut seen = BTreeSet::new();
        for candidate in &self.candidates {
            candidate.validate()?;
            if !seen.insert(candidate.task_id) {
                return Err(DomainError::invalid(
                    "SchedulingSnapshot",
                    "a task appears twice among the candidates",
                ));
            }
        }
        Ok(())
    }

    /// The candidates in the pass's total order.
    pub(crate) fn sorted_candidates(&self) -> Vec<&Candidate> {
        let mut sorted: Vec<&Candidate> = self.candidates.iter().collect();
        sorted.sort_by_key(|candidate| candidate.ordering().sort_key());
        sorted
    }
}
