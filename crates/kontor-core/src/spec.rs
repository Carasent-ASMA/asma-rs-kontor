//! Versioned specifications: work profiles, teams, personas and triggers.
//!
//! Every specification in this module is an immutable revision identified by
//! `(id, version)`. A snapshot copies the whole resolved definition — not a
//! reference to it — into the task or run that uses it, so editing a template
//! later can never rewrite history.
//!
//! Nothing here interprets a particular profile, phase or gate name. A profile
//! called `ux-ui-layout` and one called `q7` must behave identically; the
//! validation below is entirely structural.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calendar::ExecutionAuthorization;
use crate::id::{
    AccountProfileId, ArtifactKey, CalendarProfileId, CanonicalDocument, ConnectorKey, ContentHash,
    EventSchemaKey, ExecutionAuthorizationId, ExternalId, ExternalIssueTypeKey, ExternalName,
    ExternalProjectKey, GateKey, IdempotencyKey, IntakeReceiptId, MiniProjectId, Money, PersonaKey,
    PersonaScenarioId, PhaseKey, ProjectId, RoleKey, RuntimeKindKey, SchemaVersion, SkillKey,
    SourceConnectionKey, SourceEventId, SourceKindKey, SpecVersion, TaskId, TeamTemplateId,
    Timestamp, TriggerKey, WorkProfileKey,
};
use crate::state::{GateState, TaskClosureCertificate};
use crate::{DomainError, DomainResult};

/// Upper bound on the number of phases in one work profile.
pub const MAX_PHASES: usize = 256;
/// Upper bound on the number of gates in one work profile.
pub const MAX_GATES: usize = 256;

// ---------------------------------------------------------------------------
// Shared references and bounds
// ---------------------------------------------------------------------------

/// A pinned reference to one revision of a role definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleRef {
    /// The role.
    pub role: RoleKey,
    /// The pinned revision of that role.
    pub version: SpecVersion,
}

/// A pinned reference to one revision of a skill definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRef {
    /// The skill.
    pub skill: SkillKey,
    /// The pinned revision of that skill.
    pub version: SpecVersion,
}

/// A pinned reference to one revision of a team template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTemplateRef {
    /// The template.
    pub template_id: TeamTemplateId,
    /// The pinned revision of that template.
    pub version: SpecVersion,
}

/// Where work of this profile is routed. Names a runtime *family*, never a
/// session and never a concrete adapter build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRoutingRef {
    /// The runtime family.
    pub runtime_kind: RuntimeKindKey,
    /// The adapter contract revision this profile was written against.
    pub version: SpecVersion,
}

/// A pinned reference to one revision of a calendar profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarPolicyRef {
    /// The calendar profile.
    pub profile_id: CalendarProfileId,
    /// The pinned revision. Applied revisions are never silently upgraded.
    pub version: SpecVersion,
}

/// A pinned reference to one revision of an external workflow specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalWorkflowRef {
    /// The connector implementation.
    pub connector: ConnectorKey,
    /// The external project.
    pub project: ExternalProjectKey,
    /// The external issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// The pinned revision.
    pub version: SpecVersion,
}

/// A pinned reference to a context-pack template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTemplateRef {
    /// The template name.
    pub template: ArtifactKey,
    /// The pinned revision.
    pub version: SpecVersion,
}

/// Explicit, non-negative resource bounds.
///
/// Every field is mandatory: there is no "unlimited" spelling anywhere in
/// Kontor, so an unbounded budget cannot be expressed by omission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetBounds {
    /// Maximum tokens across the bounded work.
    pub max_tokens: u64,
    /// Maximum runtime commands across the bounded work.
    pub max_commands: u64,
    /// Maximum wall-clock seconds across the bounded work.
    pub max_duration_seconds: u64,
    /// Maximum monetary cost, in integer minor units.
    pub max_cost: Money,
}

impl BudgetBounds {
    /// Validate that every bound is a real, positive limit.
    ///
    /// # Errors
    /// Rejects a zero bound, which would otherwise read as "no work allowed" in
    /// one place and "no limit" in another.
    pub fn validate(&self) -> DomainResult<()> {
        if self.max_tokens == 0
            || self.max_commands == 0
            || self.max_duration_seconds == 0
            || self.max_cost.minor_units == 0
        {
            return Err(DomainError::invalid(
                "BudgetBounds",
                "every bound must be positive",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Context-window policy
// ---------------------------------------------------------------------------

/// How much conversation context one persistent seat is allowed to accumulate
/// before the runtime is asked to compact it.
///
/// The set is closed and the trigger targets are a property of the class, not a
/// number a document may carry: there is no spelling for "compact at 300000
/// tokens", so a deployment cannot drift into per-seat magic numbers.
///
/// A class is a *trigger target*, never a claim about the model's physical
/// window. Kontor does not widen or narrow what the provider actually offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowClass {
    /// Orchestration, chores, focused QA and advisors.
    Lean,
    /// Ordinary implementation, testing, inspection and gates.
    Standard,
    /// Architecture, high-stakes implementation and broad debugging.
    Deep,
    /// Explicit cross-repository synthesis, large research or migration work.
    Extended,
    /// The runtime's own default. An escape hatch, never an inferred default.
    Native,
}

impl ContextWindowClass {
    /// Every class, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Lean,
        Self::Standard,
        Self::Deep,
        Self::Extended,
        Self::Native,
    ];

    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lean => "lean",
            Self::Standard => "standard",
            Self::Deep => "deep",
            Self::Extended => "extended",
            Self::Native => "native",
        }
    }

    /// The auto-compaction trigger target, in tokens.
    ///
    /// [`ContextWindowClass::Native`] has none: the runtime keeps its own
    /// default, and Kontor deliberately has no number to send.
    ///
    /// This is the *only* place a class becomes a token count. Specifications
    /// store the class, so re-tuning a target is a versioned change here rather
    /// than an edit spread across deployment data.
    #[must_use]
    pub const fn trigger_tokens(self) -> Option<u64> {
        match self {
            Self::Lean => Some(128_000),
            Self::Standard => Some(256_000),
            Self::Deep => Some(512_000),
            Self::Extended => Some(720_000),
            Self::Native => None,
        }
    }

    /// Whether this class may only be reached by an explicit declaration.
    ///
    /// A seed table or the fallback may not select one: the largest windows and
    /// the runtime default are deliberate, auditable choices, and a model may
    /// not promote itself into them by judging its own task difficult.
    #[must_use]
    pub const fn requires_explicit_selection(self) -> bool {
        matches!(self, Self::Extended | Self::Native)
    }
}

impl fmt::Display for ContextWindowClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How hard the runtime is required to honour the resolved policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEnforcement {
    /// Apply it where the runtime can; record honestly where it cannot.
    BestEffort,
    /// The runtime must be able to enforce it, or the work does not start.
    Required,
}

impl ContextEnforcement {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BestEffort => "best_effort",
            Self::Required => "required",
        }
    }
}

impl fmt::Display for ContextEnforcement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the trigger target is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTriggerScope {
    /// The whole active context.
    Total,
    /// Only what grew after the stable cached prefix.
    GrowthAfterPrefix,
}

impl ContextTriggerScope {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Total => "total",
            Self::GrowthAfterPrefix => "growth_after_prefix",
        }
    }
}

impl fmt::Display for ContextTriggerScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The versioned summarization contract a compaction must satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSummaryProfile {
    /// The portable handoff every seat already produces at a scope boundary.
    PortableHandoffV1,
}

impl ContextSummaryProfile {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PortableHandoffV1 => "portable_handoff_v1",
        }
    }
}

impl fmt::Display for ContextSummaryProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which declaration a resolved policy actually came from.
///
/// Recorded on every run, because "why does this seat have this window" must be
/// answerable from the run itself rather than by re-reading whatever the
/// templates happen to say today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicySource {
    /// An explicit, authorized override attached to this run.
    AuthorizedRunOverride,
    /// The policy the team template's role slot declares.
    RoleSlot,
    /// The default the work profile declares.
    WorkProfile,
    /// The deployment's seed table for this logical role.
    RoleSeed,
    /// Nothing declared one, so the standard class applies.
    StandardFallback,
}

impl ContextPolicySource {
    /// Every source, in strict precedence order — highest first.
    ///
    /// [`resolve_context_window`] walks exactly this slice, so precedence is
    /// this one ordered declaration rather than a chain of `if let` a reader has
    /// to reassemble.
    pub const PRECEDENCE: &'static [Self] = &[
        Self::AuthorizedRunOverride,
        Self::RoleSlot,
        Self::WorkProfile,
        Self::RoleSeed,
    ];

    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthorizedRunOverride => "authorized_run_override",
            Self::RoleSlot => "role_slot",
            Self::WorkProfile => "work_profile",
            Self::RoleSeed => "role_seed",
            Self::StandardFallback => "standard_fallback",
        }
    }

    /// Whether a policy from this source may name an explicit-only class.
    ///
    /// Only a deliberate, per-seat declaration qualifies. A seed table covers a
    /// whole role across every deployment that loads it, and the fallback covers
    /// everything that declared nothing at all; neither is a decision anybody
    /// made about *this* seat.
    #[must_use]
    pub const fn may_select_explicit_only(self) -> bool {
        matches!(
            self,
            Self::AuthorizedRunOverride | Self::RoleSlot | Self::WorkProfile
        )
    }
}

impl fmt::Display for ContextPolicySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One complete context-window policy.
///
/// Every field is mandatory. The MVP policy value is whole, so two partially
/// populated declarations are never merged field-by-field into a policy nobody
/// wrote — the highest-precedence declaration wins entire.
///
/// `deny_unknown_fields` is what refuses an arbitrary token target: a document
/// that tries to carry `trigger_tokens` fails to deserialize instead of being
/// silently ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowPolicy {
    /// The trigger target class.
    pub class: ContextWindowClass,
    /// How hard the runtime must honour it.
    pub enforcement: ContextEnforcement,
    /// What the target is measured against.
    pub trigger_scope: ContextTriggerScope,
    /// Whether a coherent scope change may request compaction below the
    /// threshold.
    pub boundary_compaction: bool,
    /// The summarization contract a compaction must satisfy.
    pub summary_profile: ContextSummaryProfile,
}

impl ContextWindowPolicy {
    /// The policy a seat gets when nothing declared one.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            class: ContextWindowClass::Standard,
            enforcement: ContextEnforcement::BestEffort,
            trigger_scope: ContextTriggerScope::GrowthAfterPrefix,
            boundary_compaction: true,
            summary_profile: ContextSummaryProfile::PortableHandoffV1,
        }
    }

    /// The trigger target this policy asks for, in tokens.
    #[must_use]
    pub const fn trigger_tokens(&self) -> Option<u64> {
        self.class.trigger_tokens()
    }

    /// Prove this policy may be reached from `source`.
    ///
    /// # Errors
    /// Returns [`DomainError::MissingAuthority`] when a seed or fallback source
    /// names an explicit-only class.
    pub const fn ensure_selectable_by(&self, source: ContextPolicySource) -> DomainResult<()> {
        if self.class.requires_explicit_selection() && !source.may_select_explicit_only() {
            return Err(DomainError::MissingAuthority {
                subject: "context window policy",
                rule: "the largest windows and the runtime default require an explicit declaration",
            });
        }
        Ok(())
    }
}

/// The declared candidates one resolution considers.
///
/// Absent is not the same as declared-and-standard: a candidate that is `None`
/// hands the decision to the next source, which is what makes precedence
/// observable in the recorded [`ContextPolicySource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContextPolicyInputs<'a> {
    /// An explicit, authorized override attached to this run.
    pub run_override: Option<&'a ContextWindowPolicy>,
    /// What the team template's role slot declares.
    pub role_slot: Option<&'a ContextWindowPolicy>,
    /// What the work profile declares as its default.
    pub work_profile: Option<&'a ContextWindowPolicy>,
    /// What the deployment's seed table declares for this logical role.
    pub role_seed: Option<&'a ContextWindowPolicy>,
}

impl<'a> ContextPolicyInputs<'a> {
    /// The candidate each source offers, in the order precedence considers them.
    fn candidate(&self, source: ContextPolicySource) -> Option<&'a ContextWindowPolicy> {
        match source {
            ContextPolicySource::AuthorizedRunOverride => self.run_override,
            ContextPolicySource::RoleSlot => self.role_slot,
            ContextPolicySource::WorkProfile => self.work_profile,
            ContextPolicySource::RoleSeed => self.role_seed,
            ContextPolicySource::StandardFallback => None,
        }
    }
}

/// One resolved policy and the declaration it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedContextPolicy {
    /// Which declaration won.
    pub source: ContextPolicySource,
    /// The whole policy that declaration carried.
    pub policy: ContextWindowPolicy,
}

/// Resolve one seat's context-window policy.
///
/// The first present candidate in [`ContextPolicySource::PRECEDENCE`] wins
/// whole; when none is present the standard fallback applies. The function is
/// pure and total, so the same inputs always produce the same source and policy
/// — which is what makes the recorded snapshot reproducible.
///
/// # Errors
/// Returns [`DomainError::MissingAuthority`] when the winning declaration is a
/// seed that names an explicit-only class.
pub fn resolve_context_window(
    inputs: &ContextPolicyInputs<'_>,
) -> DomainResult<ResolvedContextPolicy> {
    for source in ContextPolicySource::PRECEDENCE.iter().copied() {
        if let Some(policy) = inputs.candidate(source) {
            policy.ensure_selectable_by(source)?;
            return Ok(ResolvedContextPolicy {
                source,
                policy: *policy,
            });
        }
    }
    Ok(ResolvedContextPolicy {
        source: ContextPolicySource::StandardFallback,
        policy: ContextWindowPolicy::standard(),
    })
}

/// What a runtime attests about the triggers it can be configured with.
///
/// Both bounds are optional and absence means **unknown**, never zero and never
/// an invented default. An unknown ceiling leaves the request standing; an
/// unknown minimum imposes no floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowBounds {
    /// The largest trigger this runtime can safely honour, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_ceiling_tokens: Option<u64>,
    /// The smallest trigger this runtime can be configured with, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_trigger_tokens: Option<u64>,
}

impl ContextWindowBounds {
    /// Bounds a runtime declared nothing about.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            safe_ceiling_tokens: None,
            minimum_trigger_tokens: None,
        }
    }

    /// Validate that the two bounds can both be satisfied.
    ///
    /// # Errors
    /// Rejects a zero bound, which would read as "no context allowed", and a
    /// minimum above the ceiling, which no trigger could satisfy.
    pub fn validate(&self) -> DomainResult<()> {
        if self.safe_ceiling_tokens == Some(0) || self.minimum_trigger_tokens == Some(0) {
            return Err(DomainError::invalid(
                "ContextWindowBounds",
                "a declared bound must be positive",
            ));
        }
        if let (Some(ceiling), Some(minimum)) =
            (self.safe_ceiling_tokens, self.minimum_trigger_tokens)
            && minimum > ceiling
        {
            return Err(DomainError::invalid(
                "ContextWindowBounds",
                "the minimum configurable trigger is above the safe ceiling",
            ));
        }
        Ok(())
    }
}

/// Whether the runtime could actually be told about the policy.
///
/// This is a fact about the runtime, not about the work, and it is recorded
/// rather than smoothed over: an unsupported runtime records
/// [`ContextCapabilityResult::NotEnforced`] and never
/// [`ContextCapabilityResult::Configured`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCapabilityResult {
    /// The runtime accepts context configuration and was given the trigger.
    Configured,
    /// The runtime declares no context configuration; `best_effort` continues
    /// visibly unenforced.
    NotEnforced,
    /// Enforcement was required and its confirmation has not arrived. Reuse
    /// stays blocked until it does.
    Pending,
}

impl ContextCapabilityResult {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::NotEnforced => "not_enforced",
            Self::Pending => "pending",
        }
    }
}

impl fmt::Display for ContextCapabilityResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why an effective trigger differs from the requested one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextClamp {
    /// The request stands unchanged.
    None,
    /// The runtime's safe ceiling is below the request.
    ToSafeCeiling,
    /// The runtime's smallest configurable trigger is above the request.
    ToMinimumTrigger,
}

impl ContextClamp {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ToSafeCeiling => "to_safe_ceiling",
            Self::ToMinimumTrigger => "to_minimum_trigger",
        }
    }

    /// Whether the request was changed at all.
    #[must_use]
    pub const fn is_clamped(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl fmt::Display for ContextClamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one run asked for, before any runtime saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedContextPolicy {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// Which declaration won.
    pub source: ContextPolicySource,
    /// The whole policy that declaration carried.
    pub policy: ContextWindowPolicy,
    /// The trigger the class asks for. Absent for `native`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_tokens: Option<u64>,
}

impl RequestedContextPolicy {
    /// Freeze one resolution as the requested half.
    #[must_use]
    pub const fn of(resolved: &ResolvedContextPolicy, schema_version: SchemaVersion) -> Self {
        Self {
            schema_version,
            source: resolved.source,
            policy: resolved.policy,
            trigger_tokens: resolved.policy.trigger_tokens(),
        }
    }
}

/// What the runtime was actually configured with, and the evidence for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveContextPolicy {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The policy as requested. Clamping changes the trigger, never the class:
    /// the class is what was asked for and stays auditable as such.
    pub policy: ContextWindowPolicy,
    /// The trigger actually in force. Absent for `native`, and absent when the
    /// runtime cannot be configured at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_tokens: Option<u64>,
    /// The bounds the request was measured against. Absent fields stay unknown.
    pub bounds: ContextWindowBounds,
    /// Why the effective trigger differs from the requested one.
    pub clamp: ContextClamp,
    /// Whether the runtime could be told at all.
    pub capability: ContextCapabilityResult,
}

impl EffectiveContextPolicy {
    /// Derive the effective half from the requested one and what the runtime
    /// attested.
    ///
    /// `supported` is whether the runtime declares context configuration at all.
    ///
    /// The arithmetic is the whole rule: for a non-`native` class the effective
    /// trigger is the request raised to any declared minimum and lowered to any
    /// declared ceiling. An unknown bound imposes nothing and is never replaced
    /// by a number.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] when enforcement is
    ///   [`ContextEnforcement::Required`] and the runtime cannot configure the
    ///   policy at all.
    /// * [`DomainError::Invalid`] when enforcement is required and the runtime's
    ///   smallest configurable trigger is above the request, which cannot be
    ///   honoured without silently widening the seat's window.
    /// * As [`ContextWindowBounds::validate`].
    pub fn derive(
        requested: &RequestedContextPolicy,
        bounds: &ContextWindowBounds,
        supported: bool,
    ) -> DomainResult<Self> {
        bounds.validate()?;
        let required = requested.policy.enforcement == ContextEnforcement::Required;

        if !supported {
            if required {
                return Err(DomainError::MissingEvidence {
                    subject: "context window policy",
                    rule: "required enforcement needs a runtime that can configure it",
                });
            }
            return Ok(Self {
                schema_version: requested.schema_version,
                policy: requested.policy,
                trigger_tokens: None,
                bounds: *bounds,
                clamp: ContextClamp::None,
                capability: ContextCapabilityResult::NotEnforced,
            });
        }

        // `native` sends no trigger at all, so no bound can clamp it.
        let Some(target) = requested.trigger_tokens else {
            return Ok(Self {
                schema_version: requested.schema_version,
                policy: requested.policy,
                trigger_tokens: None,
                bounds: *bounds,
                clamp: ContextClamp::None,
                capability: ContextCapabilityResult::Configured,
            });
        };

        let mut effective = target;
        let mut clamp = ContextClamp::None;
        if let Some(minimum) = bounds.minimum_trigger_tokens
            && minimum > effective
        {
            if required {
                return Err(DomainError::invalid(
                    "context window policy",
                    "the runtime's smallest configurable trigger is above a required target",
                ));
            }
            effective = minimum;
            clamp = ContextClamp::ToMinimumTrigger;
        }
        if let Some(ceiling) = bounds.safe_ceiling_tokens
            && ceiling < effective
        {
            effective = ceiling;
            clamp = ContextClamp::ToSafeCeiling;
        }

        Ok(Self {
            schema_version: requested.schema_version,
            policy: requested.policy,
            trigger_tokens: Some(effective),
            bounds: *bounds,
            clamp,
            capability: ContextCapabilityResult::Configured,
        })
    }

    /// The same policy, recorded as awaiting a confirmation that has not
    /// arrived.
    ///
    /// Used where enforcement is required and the runtime has accepted the
    /// request but not yet attested it: reuse stays blocked, and nothing claims
    /// success in the meantime.
    #[must_use]
    pub fn pending(mut self) -> Self {
        self.capability = ContextCapabilityResult::Pending;
        self
    }
}

/// Both halves of one run's context policy, frozen with their digests.
///
/// This is the immutable record the whole feature is audited from: it is
/// produced once, before the session exists, and never re-resolved. A later edit
/// to a template, a profile, a seed table or a runtime's declared bounds cannot
/// reach backwards into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPolicySnapshot {
    /// Schema generation of this snapshot.
    pub schema_version: SchemaVersion,
    /// What was asked for.
    pub requested: RequestedContextPolicy,
    /// Digest of the canonical requested half.
    pub requested_hash: ContentHash,
    /// What is actually in force.
    pub effective: EffectiveContextPolicy,
    /// Digest of the canonical effective half.
    pub effective_hash: ContentHash,
    /// When the policy was resolved.
    pub resolved_at: Timestamp,
}

impl ContextPolicySnapshot {
    /// Canonicalize and hash both halves.
    ///
    /// # Errors
    /// Returns [`DomainError`] when either half cannot be canonicalized.
    pub fn freeze(
        requested: RequestedContextPolicy,
        effective: EffectiveContextPolicy,
        resolved_at: Timestamp,
    ) -> DomainResult<Self> {
        let requested_hash = CanonicalDocument::from_serializable(&requested)?
            .hash()
            .clone();
        let effective_hash = CanonicalDocument::from_serializable(&effective)?
            .hash()
            .clone();
        Ok(Self {
            schema_version: requested.schema_version,
            requested,
            requested_hash,
            effective,
            effective_hash,
            resolved_at,
        })
    }

    /// Freeze the standard fallback against what a runtime declared.
    ///
    /// This is the seat nobody declared anything for: no override, no slot
    /// policy, no profile default and no seed. It is resolved through the same
    /// resolver as every other seat, so the recorded source is
    /// [`ContextPolicySource::StandardFallback`] rather than an invented one.
    ///
    /// # Errors
    /// As [`EffectiveContextPolicy::derive`] and [`ContextPolicySnapshot::freeze`].
    pub fn standard(
        bounds: &ContextWindowBounds,
        supported: bool,
        schema_version: SchemaVersion,
        resolved_at: Timestamp,
    ) -> DomainResult<Self> {
        let resolved = resolve_context_window(&ContextPolicyInputs::default())?;
        let requested = RequestedContextPolicy::of(&resolved, schema_version);
        let effective = EffectiveContextPolicy::derive(&requested, bounds, supported)?;
        Self::freeze(requested, effective, resolved_at)
    }

    /// Verify that both halves still hash to their recorded digests.
    ///
    /// # Errors
    /// Returns [`DomainError`] when either half or either digest was altered.
    pub fn verify(&self) -> DomainResult<()> {
        let requested = CanonicalDocument::from_serializable(&self.requested)?;
        if requested.hash() != &self.requested_hash {
            return Err(DomainError::invalid(
                "ContextPolicySnapshot",
                "the requested policy no longer matches its pinned hash",
            ));
        }
        let effective = CanonicalDocument::from_serializable(&self.effective)?;
        if effective.hash() != &self.effective_hash {
            return Err(DomainError::invalid(
                "ContextPolicySnapshot",
                "the effective policy no longer matches its pinned hash",
            ));
        }
        Ok(())
    }

    /// Whether this seat may be reused without waiting for a confirmation.
    ///
    /// A required policy the runtime has not attested stays
    /// [`ContextCapabilityResult::Pending`], and pending blocks reuse rather
    /// than quietly proceeding as if it had been enforced.
    #[must_use]
    pub const fn permits_reuse(&self) -> bool {
        !matches!(self.effective.capability, ContextCapabilityResult::Pending)
    }
}

/// One logical role's seeded context-window policy, as deployment data.
///
/// The seed table is the *only* place an ASMA role name meets a context class,
/// and it lives in a data file. No Rust in Kontor compares a role id to a
/// literal to decide how much context it gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleContextSeed {
    /// The logical role.
    pub role: RoleKey,
    /// The policy that role seeds.
    pub context_window: ContextWindowPolicy,
}

/// The context-window resolution inputs a team run froze when it was created.
///
/// Freezing the *inputs* is what makes [`resolve_context_window`] reproducible
/// for the whole life of the run: re-resolving later reads this copy, not
/// whatever the profile pack says today.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamContextPolicySeed {
    /// The work profile's declared default, if it declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_profile: Option<ContextWindowPolicy>,
    /// The deployment's per-role seed table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_seeds: Vec<RoleContextSeed>,
}

impl TeamContextPolicySeed {
    /// The seeded policy for one logical role, if the table names it.
    #[must_use]
    pub fn seed_for(&self, role: &RoleKey) -> Option<&ContextWindowPolicy> {
        self.role_seeds
            .iter()
            .find(|seed| &seed.role == role)
            .map(|seed| &seed.context_window)
    }

    /// Resolve one seat's policy against these frozen inputs.
    ///
    /// `role_slot` is what the seat itself declares and `run_override` an
    /// explicit authorized override; both come from the caller because neither
    /// is part of the frozen data.
    ///
    /// # Errors
    /// As [`resolve_context_window`].
    pub fn resolve(
        &self,
        role: &RoleKey,
        role_slot: Option<&ContextWindowPolicy>,
        run_override: Option<&ContextWindowPolicy>,
    ) -> DomainResult<ResolvedContextPolicy> {
        resolve_context_window(&ContextPolicyInputs {
            run_override,
            role_slot,
            work_profile: self.work_profile.as_ref(),
            role_seed: self.seed_for(role),
        })
    }

    /// Validate the frozen inputs.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] when a role is seeded twice.
    /// * [`DomainError::MissingAuthority`] when a seed names an explicit-only
    ///   class, which no seed table may reach.
    pub fn validate(&self) -> DomainResult<()> {
        let mut seen: BTreeSet<&RoleKey> = BTreeSet::new();
        for seed in &self.role_seeds {
            if !seen.insert(&seed.role) {
                return Err(DomainError::invalid(
                    "TeamContextPolicySeed",
                    "seeds the same logical role twice",
                ));
            }
            seed.context_window
                .ensure_selectable_by(ContextPolicySource::RoleSeed)?;
        }
        if let Some(policy) = &self.work_profile {
            policy.ensure_selectable_by(ContextPolicySource::WorkProfile)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Work profiles
// ---------------------------------------------------------------------------

/// The kind of content an artifact carries. Generic on purpose: no scheduler or
/// UI behaviour may branch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactContentType {
    /// Prose or structured document.
    Document,
    /// A reference to something stored elsewhere.
    Link,
    /// A code change (diff, branch, pull request).
    CodeChange,
    /// A generated report.
    Report,
    /// An image or recording.
    Image,
    /// A machine-readable dataset.
    Dataset,
}

/// The contract an artifact must satisfy to count as produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactContractSpec {
    /// The artifact.
    pub key: ArtifactKey,
    /// Human label.
    pub label: ExternalName,
    /// The phase that produces it.
    pub producer_phase: PhaseKey,
    /// What kind of content it is.
    pub content_type: ArtifactContentType,
    /// Whether stored evidence is required, not just a declaration.
    pub evidence_required: bool,
}

/// One phase of a work profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseSpec {
    /// The phase.
    pub id: PhaseKey,
    /// Human label.
    pub label: ExternalName,
    /// Artifacts that must exist before the phase can complete.
    pub required_artifacts: Vec<ArtifactKey>,
    /// Gates evaluated at the end of this phase.
    pub gates: Vec<GateKey>,
    /// Where rejected work returns to. Must be a strict ancestor of this phase.
    pub rejection_route: Option<PhaseKey>,
}

/// One forward edge of the phase DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseEdge {
    /// Source phase.
    pub from: PhaseKey,
    /// Target phase.
    pub to: PhaseKey,
    /// Role that owns the handoff across this edge, if any.
    pub handoff_role: Option<RoleKey>,
}

/// One gate of a work profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSpec {
    /// The gate.
    pub id: GateKey,
    /// The phase that owns it.
    pub phase: PhaseKey,
    /// Roles allowed to pass or reject it. Never empty.
    pub evaluator_roles: Vec<RoleKey>,
    /// Artifacts that must be present as evidence.
    pub required_evidence: Vec<ArtifactKey>,
    /// Where a rejection sends the work. Must be a strict ancestor of `phase`.
    pub rejection_target: PhaseKey,
    /// Whether the gate may be waived at all.
    pub waiver_allowed: bool,
    /// Roles allowed to waive it. Must be disjoint from `evaluator_roles`.
    pub waiver_roles: Vec<RoleKey>,
}

/// A complete, versioned work profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkProfileSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// Open profile id. Deployment data; never interpreted here.
    pub id: WorkProfileKey,
    /// Immutable revision of that id.
    pub version: SpecVersion,
    /// Human name.
    pub name: ExternalName,
    /// Ordered phases.
    pub phases: Vec<PhaseSpec>,
    /// Forward edges forming a DAG over `phases`.
    pub edges: Vec<PhaseEdge>,
    /// The single entry phase.
    pub entry_phase: PhaseKey,
    /// The declared terminal phases.
    pub terminal_phases: Vec<PhaseKey>,
    /// Pinned role revisions the profile refers to.
    pub roles: Vec<RoleRef>,
    /// Pinned skill revisions the profile refers to.
    pub skills: Vec<SkillRef>,
    /// Pinned team template, if the profile prescribes one.
    pub team_template: Option<TeamTemplateRef>,
    /// Artifact contracts.
    pub artifacts: Vec<ArtifactContractSpec>,
    /// Gates.
    pub gates: Vec<GateSpec>,
    /// Where runs of this profile are routed.
    pub runtime_routing: RuntimeRoutingRef,
    /// Default budget bounds for runs of this profile.
    pub budget_defaults: BudgetBounds,
    /// Optional calendar policy.
    pub calendar_policy: Option<CalendarPolicyRef>,
    /// Optional external workflow mapping.
    pub external_workflow: Option<ExternalWorkflowRef>,
    /// The context-window default for seats of runs of this profile.
    ///
    /// Defaulted so a profile revision written before the policy existed still
    /// parses; omission means "declare nothing", which hands the decision to the
    /// role seed rather than silently asserting a class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<ContextWindowPolicy>,
}

impl WorkProfileSpec {
    /// Validate the whole specification.
    ///
    /// Rejects: duplicate phase/gate/artifact ids, duplicate or self edges,
    /// dangling references, cycles in the forward graph, phases unreachable from
    /// the entry phase, phases that cannot reach a terminal phase, undeclared
    /// sinks, an entry phase with incoming edges, an empty terminal set, a
    /// terminal phase with outgoing edges, rejection routes that are not strict
    /// ancestors, gates without evaluator authority, waivers whose authority is
    /// not distinct, and required artifacts with no producing contract.
    ///
    /// # Errors
    /// Returns the first [`DomainError`] found. Errors name the rule, never the
    /// offending document content.
    pub fn validate(&self) -> DomainResult<()> {
        self.validate_inventory()?;
        let graph = PhaseGraph::build(self)?;
        graph.validate_shape(self)?;
        self.validate_gates(&graph)?;
        self.validate_artifacts()?;
        self.budget_defaults.validate()?;
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`WorkProfileSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    fn validate_inventory(&self) -> DomainResult<()> {
        if self.phases.is_empty() {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "must declare at least one phase",
            ));
        }
        if self.phases.len() > MAX_PHASES {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "declares more phases than the bound allows",
            ));
        }
        if self.gates.len() > MAX_GATES {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "declares more gates than the bound allows",
            ));
        }
        let mut phase_ids = BTreeSet::new();
        for phase in &self.phases {
            if !phase_ids.insert(phase.id.clone()) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "declares a duplicate phase id",
                ));
            }
        }
        let mut gate_ids = BTreeSet::new();
        for gate in &self.gates {
            if !gate_ids.insert(gate.id.clone()) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "declares a duplicate gate id",
                ));
            }
        }
        let mut artifact_ids = BTreeSet::new();
        for artifact in &self.artifacts {
            if !artifact_ids.insert(artifact.key.clone()) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "declares a duplicate artifact contract",
                ));
            }
        }
        Ok(())
    }

    fn validate_gates(&self, graph: &PhaseGraph) -> DomainResult<()> {
        let declared_roles: BTreeSet<&RoleKey> = self.roles.iter().map(|r| &r.role).collect();
        let artifact_keys: BTreeSet<&ArtifactKey> = self.artifacts.iter().map(|a| &a.key).collect();
        let gate_ids: BTreeSet<&GateKey> = self.gates.iter().map(|g| &g.id).collect();

        for phase in &self.phases {
            for gate in &phase.gates {
                if !gate_ids.contains(gate) {
                    return Err(DomainError::invalid(
                        "WorkProfileSpec",
                        "a phase references an undeclared gate",
                    ));
                }
            }
        }

        for gate in &self.gates {
            if !graph.contains(&gate.phase) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "a gate references an undeclared phase",
                ));
            }
            if gate.evaluator_roles.is_empty() {
                return Err(DomainError::MissingAuthority {
                    subject: "gate",
                    rule: "a gate must declare at least one evaluator role",
                });
            }
            let evaluators: BTreeSet<&RoleKey> = gate.evaluator_roles.iter().collect();
            if evaluators.len() != gate.evaluator_roles.len() {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "a gate lists a duplicate evaluator role",
                ));
            }
            for role in &gate.evaluator_roles {
                if !declared_roles.contains(role) {
                    return Err(DomainError::invalid(
                        "WorkProfileSpec",
                        "a gate references an undeclared role",
                    ));
                }
            }
            if gate.waiver_allowed {
                if gate.waiver_roles.is_empty() {
                    return Err(DomainError::MissingAuthority {
                        subject: "gate waiver",
                        rule: "a waivable gate must declare waiver authority",
                    });
                }
                for role in &gate.waiver_roles {
                    if !declared_roles.contains(role) {
                        return Err(DomainError::invalid(
                            "WorkProfileSpec",
                            "a gate references an undeclared waiver role",
                        ));
                    }
                    if evaluators.contains(role) {
                        return Err(DomainError::MissingAuthority {
                            subject: "gate waiver",
                            rule: "waiver authority must be distinct from evaluator authority",
                        });
                    }
                }
            } else if !gate.waiver_roles.is_empty() {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "a non-waivable gate must not declare waiver authority",
                ));
            }
            for evidence in &gate.required_evidence {
                if !artifact_keys.contains(evidence) {
                    return Err(DomainError::invalid(
                        "WorkProfileSpec",
                        "a gate requires evidence with no artifact contract",
                    ));
                }
            }
            graph.ensure_strict_ancestor(&gate.rejection_target, &gate.phase, "gate rejection")?;
        }
        Ok(())
    }

    fn validate_artifacts(&self) -> DomainResult<()> {
        let phase_ids: BTreeSet<&PhaseKey> = self.phases.iter().map(|p| &p.id).collect();
        let contracts: BTreeMap<&ArtifactKey, &ArtifactContractSpec> =
            self.artifacts.iter().map(|a| (&a.key, a)).collect();
        for artifact in &self.artifacts {
            if !phase_ids.contains(&artifact.producer_phase) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "an artifact contract names an undeclared producer phase",
                ));
            }
        }
        for phase in &self.phases {
            for required in &phase.required_artifacts {
                let contract = contracts.get(required).ok_or(DomainError::Invalid {
                    subject: "WorkProfileSpec",
                    rule: "a required artifact has no producing contract",
                })?;
                if !contract.evidence_required {
                    return Err(DomainError::MissingEvidence {
                        subject: "artifact contract",
                        rule: "a required artifact must carry an evidence contract",
                    });
                }
            }
        }
        Ok(())
    }

    /// The gates a phase owns.
    #[must_use]
    pub fn gates_of(&self, phase: &PhaseKey) -> Vec<&GateSpec> {
        self.gates.iter().filter(|g| &g.phase == phase).collect()
    }

    /// Look up a gate by key.
    #[must_use]
    pub fn gate(&self, gate: &GateKey) -> Option<&GateSpec> {
        self.gates.iter().find(|g| &g.id == gate)
    }
}

/// Adjacency view of a validated phase DAG.
struct PhaseGraph {
    order: Vec<PhaseKey>,
    successors: BTreeMap<PhaseKey, BTreeSet<PhaseKey>>,
    predecessors: BTreeMap<PhaseKey, BTreeSet<PhaseKey>>,
}

impl PhaseGraph {
    fn build(spec: &WorkProfileSpec) -> DomainResult<Self> {
        let order: Vec<PhaseKey> = spec.phases.iter().map(|p| p.id.clone()).collect();
        let known: BTreeSet<&PhaseKey> = order.iter().collect();
        let mut successors: BTreeMap<PhaseKey, BTreeSet<PhaseKey>> =
            order.iter().map(|p| (p.clone(), BTreeSet::new())).collect();
        let mut predecessors = successors.clone();

        for edge in &spec.edges {
            if !known.contains(&edge.from) || !known.contains(&edge.to) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "an edge references an undeclared phase",
                ));
            }
            if edge.from == edge.to {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "an edge connects a phase to itself",
                ));
            }
            let inserted = successors
                .get_mut(&edge.from)
                .is_some_and(|set| set.insert(edge.to.clone()));
            if !inserted {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "declares a duplicate edge",
                ));
            }
            if let Some(set) = predecessors.get_mut(&edge.to) {
                set.insert(edge.from.clone());
            }
        }
        Ok(Self {
            order,
            successors,
            predecessors,
        })
    }

    fn contains(&self, phase: &PhaseKey) -> bool {
        self.successors.contains_key(phase)
    }

    fn validate_shape(&self, spec: &WorkProfileSpec) -> DomainResult<()> {
        if !self.contains(&spec.entry_phase) {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "the entry phase is not declared",
            ));
        }
        if self
            .predecessors
            .get(&spec.entry_phase)
            .is_some_and(|p| !p.is_empty())
        {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "the entry phase must have no incoming edge",
            ));
        }
        if spec.terminal_phases.is_empty() {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "must declare at least one terminal phase",
            ));
        }
        let terminals: BTreeSet<&PhaseKey> = spec.terminal_phases.iter().collect();
        if terminals.len() != spec.terminal_phases.len() {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "declares a duplicate terminal phase",
            ));
        }
        for terminal in &terminals {
            if !self.contains(terminal) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "a terminal phase is not declared",
                ));
            }
            if self
                .successors
                .get(*terminal)
                .is_some_and(|s| !s.is_empty())
            {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "a terminal phase must have no outgoing edge",
                ));
            }
        }
        for phase in &self.order {
            let is_sink = self.successors.get(phase).is_some_and(BTreeSet::is_empty);
            if is_sink && !terminals.contains(phase) {
                return Err(DomainError::invalid(
                    "WorkProfileSpec",
                    "a phase without outgoing edges is not declared terminal",
                ));
            }
        }

        self.reject_cycles()?;

        let reachable = self.reachable_from(&spec.entry_phase);
        if reachable.len() != self.order.len() {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "a phase is unreachable from the entry phase",
            ));
        }
        let co_reachable = self.co_reachable_from(&terminals);
        if co_reachable.len() != self.order.len() {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "a phase cannot reach a terminal phase",
            ));
        }

        for phase in &spec.phases {
            if let Some(route) = &phase.rejection_route {
                self.ensure_strict_ancestor(route, &phase.id, "phase rejection")?;
            }
        }
        Ok(())
    }

    /// Kahn's algorithm over the forward edges only. Rejection routes are
    /// validated separately and never make the forward graph cyclic.
    fn reject_cycles(&self) -> DomainResult<()> {
        let mut indegree: BTreeMap<&PhaseKey, usize> = self
            .order
            .iter()
            .map(|p| {
                (
                    p,
                    self.predecessors
                        .get(p)
                        .map_or(0, std::collections::BTreeSet::len),
                )
            })
            .collect();
        let mut queue: Vec<&PhaseKey> = indegree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(phase, _)| *phase)
            .collect();
        let mut visited = 0usize;
        while let Some(phase) = queue.pop() {
            visited += 1;
            if let Some(successors) = self.successors.get(phase) {
                for successor in successors {
                    if let Some(degree) = indegree.get_mut(successor) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(successor);
                        }
                    }
                }
            }
        }
        if visited == self.order.len() {
            Ok(())
        } else {
            Err(DomainError::invalid(
                "WorkProfileSpec",
                "the forward phase graph contains a cycle",
            ))
        }
    }

    fn reachable_from(&self, start: &PhaseKey) -> BTreeSet<PhaseKey> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start.clone()];
        while let Some(phase) = stack.pop() {
            if !seen.insert(phase.clone()) {
                continue;
            }
            if let Some(successors) = self.successors.get(&phase) {
                stack.extend(successors.iter().cloned());
            }
        }
        seen
    }

    fn co_reachable_from(&self, terminals: &BTreeSet<&PhaseKey>) -> BTreeSet<PhaseKey> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<PhaseKey> = terminals.iter().map(|p| (*p).clone()).collect();
        while let Some(phase) = stack.pop() {
            if !seen.insert(phase.clone()) {
                continue;
            }
            if let Some(predecessors) = self.predecessors.get(&phase) {
                stack.extend(predecessors.iter().cloned());
            }
        }
        seen
    }

    fn ancestors_of(&self, phase: &PhaseKey) -> BTreeSet<PhaseKey> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<PhaseKey> = self
            .predecessors
            .get(phase)
            .map(|p| p.iter().cloned().collect())
            .unwrap_or_default();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(predecessors) = self.predecessors.get(&current) {
                stack.extend(predecessors.iter().cloned());
            }
        }
        seen
    }

    fn ensure_strict_ancestor(
        &self,
        target: &PhaseKey,
        phase: &PhaseKey,
        subject: &'static str,
    ) -> DomainResult<()> {
        if !self.contains(target) {
            return Err(DomainError::invalid(
                "WorkProfileSpec",
                "a rejection route references an undeclared phase",
            ));
        }
        if target == phase {
            return Err(DomainError::Invalid {
                subject,
                rule: "a rejection route must not target its own phase",
            });
        }
        if !self.ancestors_of(phase).contains(target) {
            return Err(DomainError::Invalid {
                subject,
                rule: "a rejection route must target a strict ancestor",
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Resolved snapshots
// ---------------------------------------------------------------------------

/// The frozen copy of a work profile that a task actually runs.
///
/// The definition, its profile version and its hash are immutable for the life
/// of the workflow; only the current phase and the aggregate revision advance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedWorkProfileSnapshot {
    /// Schema generation of this snapshot.
    pub schema_version: SchemaVersion,
    /// The full resolved definition, copied — not referenced.
    pub definition: WorkProfileSpec,
    /// Digest of the canonical definition.
    pub definition_hash: ContentHash,
    /// When the definition was resolved.
    pub resolved_at: Timestamp,
}

impl ResolvedWorkProfileSnapshot {
    /// Resolve and freeze a validated profile.
    ///
    /// # Errors
    /// As [`WorkProfileSpec::validate`].
    pub fn resolve(definition: &WorkProfileSpec, resolved_at: Timestamp) -> DomainResult<Self> {
        let document = definition.canonicalize()?;
        Ok(Self {
            schema_version: definition.schema_version,
            definition: definition.clone(),
            definition_hash: document.hash().clone(),
            resolved_at,
        })
    }

    /// Verify that the snapshot still hashes to its recorded digest.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the definition or the digest was altered.
    pub fn verify(&self) -> DomainResult<()> {
        let document = self.definition.canonicalize()?;
        if document.hash() != &self.definition_hash {
            return Err(DomainError::invalid(
                "ResolvedWorkProfileSnapshot",
                "definition no longer matches its pinned hash",
            ));
        }
        Ok(())
    }

    /// Certify that this task may close.
    ///
    /// Every phase must be recorded complete, every gate the profile declares
    /// must be [`GateState::Passed`] or [`GateState::Waived`], and every required
    /// artifact must be present. Waivers are only accepted when the gate allows
    /// them, which the profile — not the caller — decides.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] when a phase, gate or artifact is
    ///   outstanding.
    /// * [`DomainError::MissingAuthority`] when a gate was waived that the
    ///   profile does not allow waiving.
    pub fn certify_closure(
        &self,
        completed_phases: &BTreeSet<PhaseKey>,
        gate_states: &BTreeMap<GateKey, GateState>,
        produced_artifacts: &BTreeSet<ArtifactKey>,
    ) -> DomainResult<TaskClosureCertificate> {
        for phase in &self.definition.phases {
            if !completed_phases.contains(&phase.id) {
                return Err(DomainError::MissingEvidence {
                    subject: "task closure",
                    rule: "a profile phase has not completed",
                });
            }
            for artifact in &phase.required_artifacts {
                if !produced_artifacts.contains(artifact) {
                    return Err(DomainError::MissingEvidence {
                        subject: "task closure",
                        rule: "a required artifact has not been produced",
                    });
                }
            }
        }
        for gate in &self.definition.gates {
            let state = gate_states
                .get(&gate.id)
                .copied()
                .unwrap_or(GateState::NotReady);
            if !state.satisfies_requirement() {
                return Err(DomainError::MissingEvidence {
                    subject: "task closure",
                    rule: "a profile gate has not passed or been waived",
                });
            }
            if state == GateState::Waived && !gate.waiver_allowed {
                return Err(DomainError::MissingAuthority {
                    subject: "task closure",
                    rule: "a gate was waived that the profile forbids waiving",
                });
            }
            for evidence in &gate.required_evidence {
                if !produced_artifacts.contains(evidence) {
                    return Err(DomainError::MissingEvidence {
                        subject: "task closure",
                        rule: "gate evidence is missing",
                    });
                }
            }
        }
        Ok(TaskClosureCertificate::issue())
    }
}

/// One immutable revision of a team template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTemplateRevision {
    /// The template id shared by every revision.
    pub template_id: TeamTemplateId,
    /// This revision.
    pub version: SpecVersion,
    /// Human name.
    pub name: ExternalName,
    /// The canonical definition, stored byte-for-byte with its digest.
    pub definition: CanonicalDocument,
    /// Role authority carried by this template revision.
    pub role_authority: Vec<RoleAuthority>,
}

/// What a role may decide inside a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAuthority {
    /// The role.
    pub role: RoleKey,
    /// Gates this role may pass or reject.
    pub may_evaluate: Vec<GateKey>,
    /// Gates this role may waive.
    pub may_waive: Vec<GateKey>,
}

/// The frozen copy of a team template that a run actually used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRunSnapshot {
    /// Schema generation of this snapshot.
    pub schema_version: SchemaVersion,
    /// The template this run came from.
    pub template_id: TeamTemplateId,
    /// The pinned template revision.
    pub template_version: SpecVersion,
    /// The full canonical definition, copied into the run.
    pub definition: CanonicalDocument,
    /// Role authority as it stood when the run started.
    pub role_authority: Vec<RoleAuthority>,
    /// The context-window resolution inputs, frozen with the rest of the run.
    ///
    /// The work-profile default and the deployment's role seed table are copied
    /// here — not referenced — so every seat this run ever launches resolves
    /// against the same inputs, whatever the pack says later.
    #[serde(default)]
    pub context_policy: TeamContextPolicySeed,
}

impl TeamRunSnapshot {
    /// The role slots the frozen template declares, by id.
    ///
    /// Read from the run's own copied definition, so it is the set this run was
    /// pinned to rather than whatever the template says now. It exists here
    /// rather than in `kontor-teams` because the store needs it — a closure
    /// re-proof has to know which seats must be accounted for — and the store is
    /// stated against this crate.
    ///
    /// Deliberately narrow: it reads slot *identity* and nothing else. Anything
    /// that needs the slots' rules parses the whole template.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the frozen definition does not
    /// carry a readable slot list, which no snapshot this crate froze ever does.
    pub fn declared_role_slots(&self) -> DomainResult<BTreeSet<crate::id::RoleSlotId>> {
        let value: serde_json::Value =
            serde_json::from_str(self.definition.json()).map_err(|_| {
                DomainError::invalid("TeamRunSnapshot", "the frozen definition is not valid JSON")
            })?;
        let slots = value
            .get("slots")
            .and_then(serde_json::Value::as_array)
            .ok_or(DomainError::Invalid {
                subject: "TeamRunSnapshot",
                rule: "the frozen definition declares no role slots",
            })?;
        slots
            .iter()
            .map(|slot| {
                slot.get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(DomainError::Invalid {
                        subject: "TeamRunSnapshot",
                        rule: "a frozen role slot carries no id",
                    })
                    .and_then(crate::id::RoleSlotId::parse)
            })
            .collect()
    }

    /// Freeze a template revision into a run.
    ///
    /// The run starts with no context-window inputs; a composer that has the
    /// profile and the deployment seed table attaches them with
    /// [`TeamRunSnapshot::with_context_policy`].
    #[must_use]
    pub fn from_revision(revision: &TeamTemplateRevision, schema_version: SchemaVersion) -> Self {
        Self {
            schema_version,
            template_id: revision.template_id,
            template_version: revision.version,
            definition: revision.definition.clone(),
            role_authority: revision.role_authority.clone(),
            context_policy: TeamContextPolicySeed::default(),
        }
    }

    /// Freeze the context-window resolution inputs into this run.
    ///
    /// # Errors
    /// As [`TeamContextPolicySeed::validate`].
    pub fn with_context_policy(mut self, seed: TeamContextPolicySeed) -> DomainResult<Self> {
        seed.validate()?;
        self.context_policy = seed;
        Ok(self)
    }

    /// Resolve one seat's policy against this run's frozen inputs.
    ///
    /// `role_slot` is what the seat itself declares and `run_override` an
    /// explicit authorized override; both come from the caller because neither
    /// is part of the team document.
    ///
    /// # Errors
    /// As [`resolve_context_window`].
    pub fn resolve_context_window(
        &self,
        role: &RoleKey,
        role_slot: Option<&ContextWindowPolicy>,
        run_override: Option<&ContextWindowPolicy>,
    ) -> DomainResult<ResolvedContextPolicy> {
        self.context_policy.resolve(role, role_slot, run_override)
    }

    /// Whether `role` may pass or reject `gate` in this run.
    #[must_use]
    pub fn may_evaluate(&self, role: &RoleKey, gate: &GateKey) -> bool {
        self.role_authority
            .iter()
            .any(|a| &a.role == role && a.may_evaluate.contains(gate))
    }

    /// Whether `role` may waive `gate` in this run.
    #[must_use]
    pub fn may_waive(&self, role: &RoleKey, gate: &GateKey) -> bool {
        self.role_authority
            .iter()
            .any(|a| &a.role == role && a.may_waive.contains(gate))
    }
}

// ---------------------------------------------------------------------------
// Persona scenarios
// ---------------------------------------------------------------------------

/// Which environment a persona scenario runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentKind {
    /// A developer machine.
    Local,
    /// A shared test environment.
    Test,
    /// An isolated sandbox.
    Sandbox,
    /// Production. Never valid for a persona scenario; representable only so it
    /// can be rejected explicitly.
    Production,
}

/// The seeded identity a persona scenario acts as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestIdentityRef {
    /// Reference to the seeded fixture identity.
    pub reference: ExternalName,
    /// Whether the identity is seeded test data. Must be true.
    pub seeded: bool,
}

/// The environment a persona scenario runs against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentRef {
    /// Environment class.
    pub kind: EnvironmentKind,
    /// Reference to the concrete environment.
    pub reference: ExternalName,
}

/// One ordered step of a persona scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioStep {
    /// 1-based position.
    pub order: u32,
    /// What the persona does.
    pub instruction: ExternalName,
    /// Artifacts this step is expected to produce.
    pub expected_evidence: Vec<ArtifactKey>,
}

/// A versioned persona scenario.
///
/// A persona is a *simulated* actor used to exercise a gate. It carries no real
/// identity, no production reference and no credential; and it can never
/// evaluate the gate it is under test for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaScenarioSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The scenario id shared by every revision.
    pub scenario_id: PersonaScenarioId,
    /// This revision.
    pub version: SpecVersion,
    /// The simulated persona.
    pub persona: PersonaKey,
    /// Characteristics and goals of the persona, as a redacted canonical
    /// document.
    pub characteristics: CanonicalDocument,
    /// Seeded test identity.
    pub identity: TestIdentityRef,
    /// Target environment.
    pub environment: EnvironmentRef,
    /// Ordered steps.
    pub steps: Vec<ScenarioStep>,
    /// Actions the persona must not take.
    pub prohibited_actions: Vec<ExternalName>,
    /// Evidence the scenario must produce.
    pub required_evidence: Vec<ArtifactKey>,
    /// The gate this scenario exercises.
    pub gate_under_test: GateKey,
    /// The role the persona acts as.
    pub actor_role: RoleKey,
    /// Independent evaluators. Never empty, never containing `actor_role`.
    pub evaluator_roles: Vec<RoleKey>,
}

impl PersonaScenarioSpec {
    /// Validate the scenario.
    ///
    /// # Errors
    /// * [`DomainError::MissingAuthority`] when the actor could evaluate or
    ///   waive its own scenario, or when no independent evaluator exists.
    /// * [`DomainError::Invalid`] for a production or non-seeded identity, empty
    ///   evidence, or malformed steps.
    pub fn validate(&self) -> DomainResult<()> {
        if self.evaluator_roles.is_empty() {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "an independent evaluator role is required",
            });
        }
        if self.evaluator_roles.contains(&self.actor_role) {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "the acting persona must not evaluate its own scenario",
            });
        }
        let unique: BTreeSet<&RoleKey> = self.evaluator_roles.iter().collect();
        if unique.len() != self.evaluator_roles.len() {
            return Err(DomainError::invalid(
                "PersonaScenarioSpec",
                "lists a duplicate evaluator role",
            ));
        }
        if self.environment.kind == EnvironmentKind::Production {
            return Err(DomainError::invalid(
                "PersonaScenarioSpec",
                "must not reference a production environment",
            ));
        }
        if !self.identity.seeded {
            return Err(DomainError::invalid(
                "PersonaScenarioSpec",
                "must reference a seeded test identity",
            ));
        }
        if self.required_evidence.is_empty() {
            return Err(DomainError::MissingEvidence {
                subject: "persona scenario",
                rule: "required evidence must not be empty",
            });
        }
        // Prohibited actions are mandatory and must each say something
        // different. `ExternalName` has already enforced trimmed, bounded and
        // sensitive-free, so uniqueness is what is left to check — using the
        // same canonical comparison as any other display text.
        if self.prohibited_actions.is_empty() {
            return Err(DomainError::invalid(
                "PersonaScenarioSpec",
                "must declare at least one prohibited action",
            ));
        }
        let distinct: BTreeSet<&ExternalName> = self.prohibited_actions.iter().collect();
        if distinct.len() != self.prohibited_actions.len() {
            return Err(DomainError::invalid(
                "PersonaScenarioSpec",
                "lists a duplicate prohibited action",
            ));
        }
        if self.steps.is_empty() {
            return Err(DomainError::invalid(
                "PersonaScenarioSpec",
                "must declare at least one step",
            ));
        }
        for (index, step) in self.steps.iter().enumerate() {
            let expected = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if step.order != expected {
                return Err(DomainError::invalid(
                    "PersonaScenarioSpec",
                    "steps must be numbered consecutively from 1",
                ));
            }
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`PersonaScenarioSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }
}

/// The frozen copy of a persona scenario attached to a task or team run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaScenarioSnapshot {
    /// Schema generation of this snapshot.
    pub schema_version: SchemaVersion,
    /// The scenario, copied.
    pub definition: PersonaScenarioSpec,
    /// Digest of the canonical scenario.
    pub definition_hash: ContentHash,
}

impl PersonaScenarioSnapshot {
    /// Freeze a validated scenario without any authority context.
    ///
    /// This is deliberately *not* enough to attach a scenario to a task: a
    /// standalone scenario cannot assert who may evaluate it, because authority
    /// belongs to the gate in the task's pinned profile. Use
    /// [`PersonaScenarioSnapshot::freeze_onto_task`] for that.
    ///
    /// # Errors
    /// As [`PersonaScenarioSpec::validate`].
    pub fn freeze(definition: &PersonaScenarioSpec) -> DomainResult<Self> {
        let document = definition.canonicalize()?;
        Ok(Self {
            schema_version: definition.schema_version,
            definition: definition.clone(),
            definition_hash: document.hash().clone(),
        })
    }

    /// Freeze a scenario onto a task, proving its authority against the gate the
    /// task's pinned work profile actually declares.
    ///
    /// The simulated persona must not be able to sign off its own scenario, in
    /// any form: it may not evaluate the gate, it may not waive it, and the
    /// independent evaluators it names must be ones the pinned gate already
    /// authorizes. Evaluator and waiver authority stay disjoint, so no single
    /// role can both fail and forgive the same gate.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] when the pinned profile declares no such gate.
    /// * [`DomainError::MissingAuthority`] when the actor holds evaluator or
    ///   waiver authority, when a declared evaluator is not authorized by the
    ///   gate, when evaluator and waiver authority overlap, or when the scenario
    ///   relies on a waiver the gate forbids.
    pub fn freeze_onto_task(
        definition: &PersonaScenarioSpec,
        profile: &ResolvedWorkProfileSnapshot,
    ) -> DomainResult<Self> {
        definition.validate()?;
        let gate =
            profile
                .definition
                .gate(&definition.gate_under_test)
                .ok_or(DomainError::Invalid {
                    subject: "persona scenario",
                    rule: "the task's pinned profile declares no such gate",
                })?;

        let evaluators: BTreeSet<&RoleKey> = gate.evaluator_roles.iter().collect();
        let waivers: BTreeSet<&RoleKey> = gate.waiver_roles.iter().collect();

        if evaluators.contains(&definition.actor_role) || waivers.contains(&definition.actor_role) {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "the simulated persona must not hold authority over its own gate",
            });
        }
        for role in &definition.evaluator_roles {
            if !evaluators.contains(role) {
                return Err(DomainError::MissingAuthority {
                    subject: "persona scenario",
                    rule: "an evaluator is not authorized by the pinned gate",
                });
            }
            if waivers.contains(role) {
                return Err(DomainError::MissingAuthority {
                    subject: "persona scenario",
                    rule: "evaluator and waiver authority must not overlap",
                });
            }
        }
        // A gate that forbids waiving has no waiver authority to borrow; the
        // profile validator already guarantees the set is empty, so relying on
        // one here is unrepresentable rather than merely refused.
        if !gate.waiver_allowed && !gate.waiver_roles.is_empty() {
            return Err(DomainError::MissingAuthority {
                subject: "persona scenario",
                rule: "the pinned gate forbids waiver but declares waiver authority",
            });
        }

        Self::freeze(definition)
    }
}

// ---------------------------------------------------------------------------
// Triggers, source events and intake
// ---------------------------------------------------------------------------

/// An RFC 6901 JSON pointer into a canonical event envelope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct JsonPointer(String);

impl JsonPointer {
    /// Parse a JSON pointer.
    ///
    /// # Errors
    /// Rejects pointers that do not start with `/`, are empty or exceed the
    /// length bound.
    pub fn parse(text: &str) -> DomainResult<Self> {
        if !text.starts_with('/') || text.len() > 256 {
            return Err(DomainError::invalid(
                "JsonPointer",
                "must start with '/' and be at most 256 characters",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    /// Borrow the pointer text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve the pointer against a value, returning its canonical rendering.
    fn resolve(&self, value: &serde_json::Value) -> Option<String> {
        value.pointer(&self.0).map(|found| match found {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        })
    }
}

impl TryFrom<String> for JsonPointer {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<JsonPointer> for String {
    fn from(value: JsonPointer) -> Self {
        value.0
    }
}

/// A deterministic deduplication expression.
///
/// It is a list of pointers, not an interpreted language: the same envelope
/// always produces the same key, and the key can be recomputed years later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupExpression {
    /// Pointers whose resolved values form the key, in order.
    pub pointers: Vec<JsonPointer>,
}

impl DedupExpression {
    /// Compute the deduplication key of an envelope.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the expression is empty or a pointer does
    /// not resolve; a partially resolvable expression must never silently
    /// produce a weaker key.
    pub fn evaluate(&self, envelope: &CanonicalDocument) -> DomainResult<ContentHash> {
        if self.pointers.is_empty() {
            return Err(DomainError::invalid(
                "DedupExpression",
                "must declare at least one pointer",
            ));
        }
        let value: serde_json::Value = serde_json::from_str(envelope.json())
            .map_err(|_| DomainError::invalid("DedupExpression", "envelope is not valid JSON"))?;
        let mut material = String::new();
        for pointer in &self.pointers {
            let resolved = pointer.resolve(&value).ok_or(DomainError::Invalid {
                subject: "DedupExpression",
                rule: "a pointer does not resolve in this envelope",
            })?;
            material.push_str(pointer.as_str());
            material.push('\u{1f}');
            material.push_str(&resolved);
            material.push('\u{1e}');
        }
        Ok(ContentHash::of(material.as_bytes()))
    }
}

/// Bounded limits every trigger must declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerLimits {
    /// Scheduling priority (higher runs first). Bounded.
    pub priority: u32,
    /// Maximum concurrent work graphs from this trigger.
    pub max_concurrency: u32,
    /// Budget bounds applied to created work.
    pub budget: BudgetBounds,
}

impl TriggerLimits {
    /// Validate that every limit is bounded and positive.
    ///
    /// # Errors
    /// Rejects zero concurrency, an out-of-range priority or an unbounded
    /// budget.
    pub fn validate(&self) -> DomainResult<()> {
        if self.max_concurrency == 0 {
            return Err(DomainError::invalid(
                "TriggerLimits",
                "concurrency must be positive",
            ));
        }
        if self.priority > 1000 {
            return Err(DomainError::invalid(
                "TriggerLimits",
                "priority is out of range",
            ));
        }
        self.budget.validate()
    }
}

/// The capability under which bounded auto-arming may act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionCapability {
    /// The account whose capability is exercised.
    pub granted_to: AccountProfileId,
    /// The execution authorization that bounds it.
    ///
    /// The field is *not* named `authorization`. Every route to a digest runs
    /// [`crate::id::reject_sensitive_material`], whose `FORBIDDEN_KEYS` contains
    /// `authorization` — the HTTP header — so a policy spelling it that way
    /// validated and then could never be canonicalized, hashed, receipted or
    /// stored. The scanner is the shared rule and stays exactly as it is; the
    /// field carries the name that says what it actually is.
    pub execution_authorization: ExecutionAuthorizationId,
}

/// Whether a trigger may arm work by itself.
///
/// There is deliberately no unbounded variant and no default: auto-arming always
/// names a capability, a concurrency bound, a budget and an authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutoArmPolicy {
    /// A human must approve before work is armed.
    ApprovalRequired,
    /// Bounded automatic arming.
    BoundedAutoArm {
        /// Capability the trigger acts under.
        capability: ExecutionCapability,
        /// Maximum concurrent auto-armed work graphs.
        max_concurrency: u32,
        /// Budget bounds for auto-armed work.
        budget: BudgetBounds,
    },
}

impl AutoArmPolicy {
    /// Validate the policy's bounds.
    ///
    /// # Errors
    /// Rejects zero concurrency or an unbounded budget on auto-arm.
    pub fn validate(&self) -> DomainResult<()> {
        match self {
            Self::ApprovalRequired => Ok(()),
            Self::BoundedAutoArm {
                max_concurrency,
                budget,
                ..
            } => {
                if *max_concurrency == 0 {
                    return Err(DomainError::invalid(
                        "AutoArmPolicy",
                        "auto-arm concurrency must be positive",
                    ));
                }
                budget.validate()
            }
        }
    }
}

closed_enum! {
    /// Why a bounded auto-arm may not arm the work it proposes.
    ///
    /// Each value is one independent reason, so a refusal says which bound was
    /// not met rather than that "something" was not. Nothing here is advisory:
    /// a bounded auto-arm arms work only when every one of them passes.
    AutoArmRefusal, "AutoArmRefusal" {
        /// The pinned policy is `approval_required`; only a human decides.
        PolicyRequiresApproval => "policy_requires_approval",
        /// The caller is not the account the capability was granted to.
        CallerNotGranted => "caller_not_granted",
        /// The authorization presented is not the one the policy pins.
        AuthorizationMismatched => "authorization_mismatched",
        /// The authorization does not cover the start instant.
        AuthorizationOutOfWindow => "authorization_out_of_window",
        /// The authorization does not cover every task being created.
        AuthorizationScopeMismatched => "authorization_scope_mismatched",
        /// The proposal creates no work, so there is nothing to arm.
        NoWorkProposed => "no_work_proposed",
        /// More work than the narrowest concurrency bound allows.
        ConcurrencyExceeded => "concurrency_exceeded",
        /// A declared budget exceeds what the authorization grants.
        BudgetExceeded => "budget_exceeded",
    }
}

/// What a bounded auto-arm is asking to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoArmRequest<'a> {
    /// The account exercising the capability.
    pub caller: AccountProfileId,
    /// The authorization the caller presents, read from storage.
    pub authorization: &'a ExecutionAuthorization,
    /// When the created work may start.
    pub at: Timestamp,
    /// The goal the created work belongs to, if any.
    pub mini_project_id: Option<MiniProjectId>,
    /// The tasks the intake decision creates.
    pub task_ids: &'a [TaskId],
}

impl BudgetBounds {
    /// Whether every bound here is within `grant`.
    ///
    /// Currencies must agree: a cost ceiling in another currency is not a
    /// smaller one, it is an incomparable one.
    #[must_use]
    pub fn within(&self, grant: &Self) -> bool {
        self.max_tokens <= grant.max_tokens
            && self.max_commands <= grant.max_commands
            && self.max_duration_seconds <= grant.max_duration_seconds
            && self.max_cost.currency == grant.max_cost.currency
            && self.max_cost.minor_units <= grant.max_cost.minor_units
    }
}

/// A versioned trigger specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// Open trigger id.
    pub id: TriggerKey,
    /// This revision.
    pub version: SpecVersion,
    /// Which source kind this trigger listens to.
    pub source_kind: SourceKindKey,
    /// Which configured connection of that kind.
    pub source_connection: SourceConnectionKey,
    /// The event schema it accepts.
    pub event_schema: EventSchemaKey,
    /// The pinned event schema revision.
    pub event_schema_version: SpecVersion,
    /// Typed filter over the canonical envelope: every pointer must equal its
    /// literal for the trigger to fire.
    pub filter: Vec<TriggerFilterClause>,
    /// Deterministic dedup expression.
    pub dedup: DedupExpression,
    /// Work profile the created graph uses.
    pub work_profile: WorkProfileKey,
    /// Pinned work-profile revision.
    pub work_profile_version: SpecVersion,
    /// Team template for the created work.
    pub team_template: TeamTemplateRef,
    /// Context-pack template for the created work.
    pub context_template: ContextTemplateRef,
    /// Approval or bounded auto-arm.
    pub approval: AutoArmPolicy,
    /// Bounded limits.
    pub limits: TriggerLimits,
    /// Optional calendar policy.
    pub calendar_policy: Option<CalendarPolicyRef>,
}

/// One clause of a trigger filter: a pointer that must equal a literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerFilterClause {
    /// Where to look in the canonical envelope.
    pub pointer: JsonPointer,
    /// The value the pointer must resolve to.
    pub equals: ExternalName,
}

impl TriggerSpec {
    /// Validate the trigger.
    ///
    /// # Errors
    /// Rejects an empty dedup expression, an unbounded limit or an auto-arm
    /// policy without capability bounds.
    pub fn validate(&self) -> DomainResult<()> {
        if self.dedup.pointers.is_empty() {
            return Err(DomainError::invalid(
                "TriggerSpec",
                "must declare a deterministic dedup expression",
            ));
        }
        self.limits.validate()?;
        self.approval.validate()?;
        Ok(())
    }

    /// Whether the filter matches an envelope.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the envelope cannot be read.
    pub fn matches(&self, envelope: &CanonicalDocument) -> DomainResult<bool> {
        let value: serde_json::Value = serde_json::from_str(envelope.json())
            .map_err(|_| DomainError::invalid("TriggerSpec", "envelope is not valid JSON"))?;
        Ok(self.filter.iter().all(|clause| {
            clause
                .pointer
                .resolve(&value)
                .is_some_and(|found| found == clause.equals.as_str())
        }))
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`TriggerSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    /// Whether this trigger's own policy arms the work a request describes.
    ///
    /// This is the *whole* bounded auto-arm rule and the only copy of it: the
    /// intake layer calls it to decide, and the store calls it again inside the
    /// transaction that creates the work, so a caller that skips the decision
    /// layer is refused by the same bounds rather than by none. Everything it
    /// reads is pinned or receipt-backed — the trigger revision, the stored
    /// authorization, the tasks actually being created — so the answer is a
    /// property of the evidence rather than of who asked.
    ///
    /// The narrowest of the three concurrency ceilings wins, and a declared
    /// budget may never exceed what the authorization grants: a trigger cannot
    /// widen its own authorization by naming a larger number.
    ///
    /// # Errors
    /// Returns the single [`AutoArmRefusal`] that applies, evaluated in the
    /// declaration order of the enum.
    pub fn authorize_auto_arm(
        &self,
        request: &AutoArmRequest<'_>,
    ) -> Result<ExecutionCapability, AutoArmRefusal> {
        let Self {
            approval:
                AutoArmPolicy::BoundedAutoArm {
                    capability,
                    max_concurrency,
                    budget,
                },
            ..
        } = self
        else {
            return Err(AutoArmRefusal::PolicyRequiresApproval);
        };
        if request.caller != capability.granted_to {
            return Err(AutoArmRefusal::CallerNotGranted);
        }
        let authorization = request.authorization;
        if authorization.id != capability.execution_authorization {
            return Err(AutoArmRefusal::AuthorizationMismatched);
        }
        if !authorization.allowed_start.contains(request.at) {
            return Err(AutoArmRefusal::AuthorizationOutOfWindow);
        }
        if request.task_ids.is_empty() {
            return Err(AutoArmRefusal::NoWorkProposed);
        }
        // Every created task, not merely the first: an authorization that covers
        // one task of a graph authorizes one task of a graph.
        if !request
            .task_ids
            .iter()
            .all(|task_id| authorization.arms(request.at, request.mini_project_id, Some(*task_id)))
        {
            return Err(AutoArmRefusal::AuthorizationScopeMismatched);
        }
        let ceiling = (*max_concurrency)
            .min(self.limits.max_concurrency)
            .min(authorization.max_concurrency);
        if u32::try_from(request.task_ids.len()).unwrap_or(u32::MAX) > ceiling {
            return Err(AutoArmRefusal::ConcurrencyExceeded);
        }
        if !budget.within(&authorization.budget)
            || !self.limits.budget.within(&authorization.budget)
        {
            return Err(AutoArmRefusal::BudgetExceeded);
        }
        Ok(*capability)
    }
}

/// The identity of one inbound event in its source system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceIdentity {
    /// Which kind of source.
    pub source_kind: SourceKindKey,
    /// Which configured connection.
    pub source_connection: SourceConnectionKey,
    /// The event id as the source system spells it.
    pub external_event_id: ExternalId,
}

closed_enum! {
    /// How far a source event has been processed.
    SourceProcessingState, "SourceProcessingState" {
        /// Committed, not yet evaluated.
        Received => "received",
        /// Evaluated against triggers.
        Evaluated => "evaluated",
        /// Evaluated and deliberately ignored.
        Ignored => "ignored",
        /// Recognized as a duplicate of an earlier event.
        Duplicate => "duplicate",
        /// Evaluation failed and must be retried or triaged.
        Failed => "failed",
    }
}

/// A canonical, redacted inbound event.
///
/// The event is committed *before* any trigger is evaluated, so a crash between
/// ingestion and evaluation loses no evidence and creates no work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalSourceEvent {
    /// Kontor's id for the event.
    pub id: SourceEventId,
    /// Its identity in the source system.
    pub identity: SourceIdentity,
    /// The redacted canonical envelope.
    pub envelope: CanonicalDocument,
    /// When the source system observed it.
    pub external_observed_at: Timestamp,
    /// When Kontor ingested it.
    pub ingested_at: Timestamp,
    /// Processing state.
    pub processing_state: SourceProcessingState,
}

closed_enum! {
    /// The deterministic outcome of evaluating one source event.
    IntakeResult, "IntakeResult" {
        /// Work was proposed and awaits approval.
        Proposed => "proposed",
        /// Work was approved and created.
        Approved => "approved",
        /// The event matched but was rejected by policy.
        Rejected => "rejected",
        /// No trigger matched.
        Ignored => "ignored",
        /// The event repeats one already recorded.
        Duplicate => "duplicate",
    }
}

/// Evidence that a human approved intake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceipt {
    /// Who approved.
    pub authority: AccountProfileId,
    /// The command receipt that recorded the approval.
    pub receipt: crate::id::CommandReceiptId,
    /// When it was approved.
    pub approved_at: Timestamp,
}

/// The work graph an intake decision proposes or created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedWorkGraph {
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning goal, if any.
    pub mini_project_id: Option<MiniProjectId>,
    /// Tasks proposed or created.
    pub task_ids: Vec<TaskId>,
}

/// The immutable record of one intake decision.
///
/// Re-evaluating the same event under a new trigger revision appends another
/// receipt; it never rewrites this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeReceipt {
    /// Kontor's id for the decision.
    pub id: IntakeReceiptId,
    /// The event that was evaluated.
    pub source_event_id: SourceEventId,
    /// Digest of that event's canonical envelope.
    pub source_event_hash: ContentHash,
    /// The trigger that decided.
    pub trigger: TriggerKey,
    /// The pinned trigger revision.
    pub trigger_version: SpecVersion,
    /// The deterministic outcome.
    pub result: IntakeResult,
    /// Approval evidence, required when `result` is `approved`.
    pub approval: Option<ApprovalReceipt>,
    /// The proposed or created work graph.
    pub proposed: Option<ProposedWorkGraph>,
    /// Idempotency key for the decision.
    pub idempotency_key: IdempotencyKey,
    /// Deterministic dedup key of the source event.
    pub dedup_key: ContentHash,
    /// The original receipt this one duplicates, when `result` is `duplicate`.
    pub duplicate_of: Option<IntakeReceiptId>,
    /// The receipt this one supersedes under a strictly newer trigger revision.
    ///
    /// Distinct from `duplicate_of`: a successor is a *new decision* about an
    /// already-stored event, not a repeat of an old one.
    pub predecessor_receipt_id: Option<IntakeReceiptId>,
    /// When the decision was recorded.
    pub decided_at: Timestamp,
}

impl IntakeReceipt {
    /// Validate the internal consistency of the decision.
    ///
    /// # Errors
    /// * `approved` without approval evidence.
    /// * `duplicate` without the original receipt, or a non-duplicate with one.
    /// * a created work graph on a result that creates no work.
    pub fn validate(&self) -> DomainResult<()> {
        match self.result {
            IntakeResult::Approved if self.approval.is_none() => {
                Err(DomainError::MissingAuthority {
                    subject: "intake approval",
                    rule: "an approved intake requires approval evidence",
                })
            }
            IntakeResult::Duplicate if self.duplicate_of.is_none() => Err(DomainError::invalid(
                "IntakeReceipt",
                "a duplicate must point at the original receipt",
            )),
            IntakeResult::Duplicate if self.proposed.is_some() => Err(DomainError::invalid(
                "IntakeReceipt",
                "a duplicate must not create a second work graph",
            )),
            _ if self.result != IntakeResult::Duplicate && self.duplicate_of.is_some() => {
                Err(DomainError::invalid(
                    "IntakeReceipt",
                    "only a duplicate may reference an original receipt",
                ))
            }
            IntakeResult::Ignored | IntakeResult::Rejected if self.proposed.is_some() => {
                Err(DomainError::invalid(
                    "IntakeReceipt",
                    "an ignored or rejected intake must not create work",
                ))
            }
            _ if self.predecessor_receipt_id == Some(self.id) => Err(DomainError::invalid(
                "IntakeReceipt",
                "a receipt cannot supersede itself",
            )),
            _ if self.duplicate_of.is_some() && self.predecessor_receipt_id.is_some() => Err(
                DomainError::invalid("IntakeReceipt", "a duplicate is not a successor"),
            ),
            _ => Ok(()),
        }
    }

    /// Whether two receipts record the *same* decision about the same event.
    ///
    /// A trigger revision is deterministic: evaluating one stored event under
    /// one pinned revision must always yield the same verdict, the same
    /// idempotency key and the same proposed graph. Only receipts that agree on
    /// all of that are replays of each other.
    ///
    /// The receipt's own id, its recording time and its lineage are deliberately
    /// excluded — those differ between a replay and the row it replays.
    #[must_use]
    pub fn decides_the_same_as(&self, other: &Self) -> bool {
        self.source_event_id == other.source_event_id
            && self.source_event_hash == other.source_event_hash
            && self.trigger == other.trigger
            && self.trigger_version == other.trigger_version
            && self.result == other.result
            && self.idempotency_key == other.idempotency_key
            && self.dedup_key == other.dedup_key
            && self.approval == other.approval
            && self.proposed == other.proposed
            && self.duplicate_of == other.duplicate_of
    }

    /// Prove this receipt actually decides the event it is stored against.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the receipt names another event or
    /// a different digest of it. Without this, a receipt could be filed against
    /// an event it never evaluated.
    pub fn ensure_decides(
        &self,
        source_event_id: SourceEventId,
        source_event_hash: &ContentHash,
    ) -> DomainResult<()> {
        if self.source_event_id != source_event_id {
            return Err(DomainError::invalid(
                "IntakeReceipt",
                "the decision names a different source event",
            ));
        }
        if &self.source_event_hash != source_event_hash {
            return Err(DomainError::invalid(
                "IntakeReceipt",
                "the decision cites a different digest of the source event",
            ));
        }
        Ok(())
    }
}
