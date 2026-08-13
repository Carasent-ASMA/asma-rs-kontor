//! When a seat's context may be compacted, and the evidence that it was.
//!
//! Two rules shape this module, and both exist because compaction is lossy:
//!
//! 1. **A finished turn is not a trigger.** There are exactly three reasons to
//!    compact — a resolved token threshold, a durable coherent-scope boundary,
//!    and an authorized operator request — and [`CompactionTrigger`] has no
//!    fourth spelling. Reviewer correction, gate retry and follow-up in the same
//!    scope reuse the existing context, because throwing away the immediate
//!    correction context is exactly what makes a seat repeat work it already
//!    did.
//! 2. **`Confirmed` is a claim about identity.** A receipt may only say
//!    `confirmed` when the native session it names is provably the *same*
//!    session afterwards — same runtime kind, host, native id and generation —
//!    and the runtime attested it. Compaction never replaces a session; a
//!    changed identity is [`CompactionStatus::Failed`], never an adoption and
//!    never a successor.
//!
//! Nothing here talks to a runtime. The receipt is a provider-neutral record so
//! the store, the API, the CLI and MCP can all project it without linking a
//! runtime adapter; an adapter freezes its own capability snapshot into a
//! [`CanonicalDocument`] on the way in.

use serde::{Deserialize, Serialize};

use crate::id::{
    AgentRunId, CanonicalDocument, CompactionReceiptId, ContentHash, ExternalId, RuntimeBindingId,
    SchemaVersion, Timestamp,
};
use crate::spec::{EffectiveContextPolicy, RequestedContextPolicy};
use crate::state::NativeRuntimeIdentity;
use crate::{DomainError, DomainResult};

/// Why a compaction was requested.
///
/// The set is closed and deliberately short. Adding a "the turn ended" variant
/// is the change this type exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    /// The runtime reports the resolved token threshold has been reached.
    Threshold,
    /// A coherent scope closed, its evidence is durable, and the next
    /// assignment changes scope.
    ScopeBoundary,
    /// An authorized operator asked for it explicitly.
    Operator,
}

impl CompactionTrigger {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Threshold => "threshold",
            Self::ScopeBoundary => "scope_boundary",
            Self::Operator => "operator",
        }
    }

    /// Whether this trigger requires a durable handoff before it may dispatch.
    ///
    /// A threshold compaction is the runtime protecting itself and cannot wait
    /// for a scope to close. A boundary or operator compaction is a deliberate
    /// act at a point where the work state *is* expressible, so it must be
    /// written down first.
    #[must_use]
    pub const fn requires_durable_handoff(self) -> bool {
        matches!(self, Self::ScopeBoundary | Self::Operator)
    }
}

impl std::fmt::Display for CompactionTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a compaction attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStatus {
    /// The runtime compacted the same session and attested it.
    Confirmed,
    /// The runtime cannot enforce context policy; `best_effort` continued
    /// without it. This is never success.
    NotEnforced,
    /// The runtime declares no compaction capability at all.
    Unsupported,
    /// Requested and not yet attested. Reuse stays blocked.
    Pending,
    /// The attempt failed, including every case where session identity moved.
    Failed,
}

impl CompactionStatus {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::NotEnforced => "not_enforced",
            Self::Unsupported => "unsupported",
            Self::Pending => "pending",
            Self::Failed => "failed",
        }
    }

    /// Whether the seat may be reused without waiting for anything further.
    ///
    /// Only `pending` blocks: a required compaction nobody has attested must not
    /// quietly proceed as though it had happened.
    #[must_use]
    pub const fn permits_reuse(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

impl std::fmt::Display for CompactionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a runtime reported about the cost of one compaction.
///
/// Every field is optional and absence means **unknown**. Zero is a measurement,
/// not a placeholder: a runtime that reports nothing must not be recorded as
/// having used no tokens and read no cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionTelemetry {
    /// Active context tokens before the attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    /// Active context tokens afterwards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after: Option<u64>,
    /// Prompt-cache tokens read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Prompt-cache tokens written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

impl CompactionTelemetry {
    /// Telemetry a runtime reported nothing about.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            tokens_before: None,
            tokens_after: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
        }
    }

    /// Whether the runtime reported anything at all.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        self.tokens_before.is_none()
            && self.tokens_after.is_none()
            && self.cache_read_tokens.is_none()
            && self.cache_write_tokens.is_none()
    }
}

/// Everything Kontor knows about a seat when it decides whether to compact.
///
/// Deliberately includes `turn_finished` so the rule that it changes nothing is
/// stated in the type and provable by a test, rather than being the absence of
/// code somebody could add back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactionAssessment {
    /// Active context tokens the runtime reports, when it reports any.
    pub observed_tokens: Option<u64>,
    /// The seat just finished a role turn. On its own this is not a reason.
    pub turn_finished: bool,
    /// A coherent scope closed, its evidence is durable, and the next
    /// assignment changes scope.
    pub scope_boundary: bool,
    /// An authorized operator asked for compaction explicitly.
    pub operator_request: bool,
}

/// Decide whether compaction may be requested, and on what grounds.
///
/// Returns `None` when no deterministic trigger is true — and a finished turn on
/// its own always lands here. The order is significance, not preference: an
/// explicit operator act is recorded as such even when a threshold also happens
/// to be met, because the receipt should say who decided.
///
/// A scope boundary only triggers when the resolved policy actually allows
/// boundary compaction.
#[must_use]
pub fn compaction_trigger(
    policy: &EffectiveContextPolicy,
    assessment: &CompactionAssessment,
) -> Option<CompactionTrigger> {
    if assessment.operator_request {
        return Some(CompactionTrigger::Operator);
    }
    if let (Some(observed), Some(threshold)) = (assessment.observed_tokens, policy.trigger_tokens)
        && observed >= threshold
    {
        return Some(CompactionTrigger::Threshold);
    }
    if assessment.scope_boundary && policy.policy.boundary_compaction {
        return Some(CompactionTrigger::ScopeBoundary);
    }
    None
}

/// The durable, provider-neutral record of one compaction attempt.
///
/// This is what the store persists and what every client projects. It carries no
/// prompt, no transcript, no provider payload and no configuration path — only
/// identifiers, the policy that was in force, hashes and what the runtime
/// attested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionReceipt {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// This receipt's identity, and the idempotency key for the attempt.
    pub id: CompactionReceiptId,
    /// The run whose seat was compacted.
    pub agent_run_id: AgentRunId,
    /// The binding the attempt addressed.
    pub binding_id: RuntimeBindingId,
    /// The native session as it stood before the attempt.
    pub native_before: NativeRuntimeIdentity,
    /// The native session afterwards, when the runtime could be re-read.
    ///
    /// Absent for an attempt that never reached the runtime, and for one whose
    /// outcome is still unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_after: Option<NativeRuntimeIdentity>,
    /// What the seat's policy asked for.
    pub requested: RequestedContextPolicy,
    /// What was actually in force.
    pub effective: EffectiveContextPolicy,
    /// Why the compaction was requested.
    pub trigger: CompactionTrigger,
    /// The runtime capability snapshot, frozen by the adapter that acted.
    ///
    /// A canonical document rather than a typed capability set, so this record
    /// stays independent of any particular adapter crate.
    pub capabilities: CanonicalDocument,
    /// How the attempt ended.
    pub status: CompactionStatus,
    /// What the runtime reported about the cost. Unknown stays unknown.
    #[serde(default)]
    pub telemetry: CompactionTelemetry,
    /// The immutable Context Pack the run was frozen against.
    pub context_pack_hash: ContentHash,
    /// The sealed durable handoff this attempt was allowed to proceed on.
    ///
    /// Absent only for a threshold compaction, which the runtime forces and
    /// which cannot wait for a scope to close. A boundary or operator
    /// compaction without one is refused rather than recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_hash: Option<ContentHash>,
    /// A reference to the runtime's own evidence for the outcome.
    ///
    /// An opaque identifier, never a payload: the evidence stays where the
    /// runtime keeps it, and this record only says where to look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<ExternalId>,
    /// When the outcome was recorded.
    pub recorded_at: Timestamp,
}

impl CompactionReceipt {
    /// Whether this attempt provably kept the same native session.
    ///
    /// Every part of the identity must match, generation included: a repeated
    /// native id in a new generation is a different session, which is precisely
    /// the drift a confirmation must not paper over.
    #[must_use]
    pub fn preserves_native_identity(&self) -> bool {
        self.native_after
            .as_ref()
            .is_some_and(|after| after == &self.native_before)
    }

    /// Validate the receipt's internal claims.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] when a `confirmed` receipt carries no
    ///   runtime evidence reference.
    /// * [`DomainError::Invalid`] when a `confirmed` receipt does not prove the
    ///   native session survived unchanged, and when a receipt that never
    ///   reached a runtime nevertheless reports telemetry.
    pub fn validate(&self) -> DomainResult<()> {
        // MUT-CTX-07's structural half: a deliberate compaction that recorded no
        // sealed handoff discarded work state nothing else holds.
        if self.trigger.requires_durable_handoff() && self.handoff_hash.is_none() {
            return Err(DomainError::MissingEvidence {
                subject: "CompactionReceipt",
                rule: "a boundary or operator compaction must cite a sealed durable handoff",
            });
        }
        if self.status == CompactionStatus::Confirmed {
            if !self.preserves_native_identity() {
                return Err(DomainError::invalid(
                    "CompactionReceipt",
                    "a confirmed compaction must name the same native session before and after",
                ));
            }
            if self.evidence.is_none() {
                return Err(DomainError::MissingEvidence {
                    subject: "CompactionReceipt",
                    rule: "a confirmed compaction must reference the runtime's own evidence",
                });
            }
        }
        if matches!(
            self.status,
            CompactionStatus::Unsupported | CompactionStatus::NotEnforced
        ) && !self.telemetry.is_unknown()
        {
            return Err(DomainError::invalid(
                "CompactionReceipt",
                "an attempt that reached no runtime cannot report telemetry",
            ));
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`CompactionReceipt::validate`], plus canonicalization failures —
    /// which is where the shared redaction rule refuses credentials or other
    /// sensitive material that reached a field it should not have.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    /// Whether replaying this receipt's id carries the *same* attempt.
    ///
    /// Byte-identical content under the same id is a replay and returns the
    /// original. Anything else is a different attempt wearing a used key.
    ///
    /// # Errors
    /// As [`CompactionReceipt::canonicalize`].
    pub fn is_replay_of(&self, other: &Self) -> DomainResult<bool> {
        if self.id != other.id {
            return Ok(false);
        }
        Ok(self.canonicalize()?.hash() == other.canonicalize()?.hash())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ContextPolicyInputs, ContextWindowBounds, resolve_context_window};

    fn effective(supported: bool) -> EffectiveContextPolicy {
        let resolved = resolve_context_window(&ContextPolicyInputs::default()).expect("resolves");
        let requested = RequestedContextPolicy::of(&resolved, crate::id::SCHEMA_VERSION);
        EffectiveContextPolicy::derive(&requested, &ContextWindowBounds::unknown(), supported)
            .expect("derives")
    }

    /// MUT-CTX-10. Making a finished turn a reason to compact makes this fail.
    #[test]
    fn a_finished_turn_is_not_a_trigger() {
        let policy = effective(true);
        let just_finished = CompactionAssessment {
            turn_finished: true,
            ..CompactionAssessment::default()
        };
        assert_eq!(compaction_trigger(&policy, &just_finished), None);

        // Even well under the threshold, with a finished turn, nothing fires.
        let idle = CompactionAssessment {
            observed_tokens: Some(1_000),
            turn_finished: true,
            ..CompactionAssessment::default()
        };
        assert_eq!(compaction_trigger(&policy, &idle), None);
    }

    #[test]
    fn each_deterministic_trigger_fires_on_its_own_grounds() {
        let policy = effective(true);
        assert_eq!(policy.trigger_tokens, Some(256_000));

        assert_eq!(
            compaction_trigger(
                &policy,
                &CompactionAssessment {
                    observed_tokens: Some(256_000),
                    ..CompactionAssessment::default()
                }
            ),
            Some(CompactionTrigger::Threshold)
        );
        assert_eq!(
            compaction_trigger(
                &policy,
                &CompactionAssessment {
                    scope_boundary: true,
                    ..CompactionAssessment::default()
                }
            ),
            Some(CompactionTrigger::ScopeBoundary)
        );
        assert_eq!(
            compaction_trigger(
                &policy,
                &CompactionAssessment {
                    operator_request: true,
                    ..CompactionAssessment::default()
                }
            ),
            Some(CompactionTrigger::Operator)
        );
    }

    #[test]
    fn a_runtime_that_enforces_nothing_has_no_threshold_to_reach() {
        // `not_enforced` carries no effective trigger, so no observation can
        // manufacture a threshold for it.
        let policy = effective(false);
        assert_eq!(policy.trigger_tokens, None);
        assert_eq!(
            compaction_trigger(
                &policy,
                &CompactionAssessment {
                    observed_tokens: Some(u64::MAX),
                    ..CompactionAssessment::default()
                }
            ),
            None
        );
    }

    #[test]
    fn a_boundary_trigger_respects_the_policy_that_forbids_it() {
        let mut policy = effective(true);
        policy.policy.boundary_compaction = false;
        assert_eq!(
            compaction_trigger(
                &policy,
                &CompactionAssessment {
                    scope_boundary: true,
                    ..CompactionAssessment::default()
                }
            ),
            None
        );
    }

    #[test]
    fn only_a_boundary_or_operator_compaction_owes_a_durable_handoff() {
        assert!(CompactionTrigger::ScopeBoundary.requires_durable_handoff());
        assert!(CompactionTrigger::Operator.requires_durable_handoff());
        // A threshold compaction is the runtime protecting itself; it cannot
        // wait for a scope to close.
        assert!(!CompactionTrigger::Threshold.requires_durable_handoff());
    }
}
