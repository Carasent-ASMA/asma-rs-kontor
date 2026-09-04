//! Durable evidence and the forward-only state machine for quota-driven seat succession.
//!
//! This module deliberately stops at the control-plane seam. It freezes the
//! decision, the predecessor and quota evidence, the handoff, the successor
//! observation and the final receipt. Runtime retirement and launch remain
//! effects performed above the repository port.

use serde::{Deserialize, Serialize};

use crate::id::{
    AccountProfileId, AgentRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash,
    EventCursor, IdempotencyKey, ProjectId, QuotaObservationProvenanceId, RoleKey,
    RuntimeBindingId, SuccessionAttemptId, SuccessionReceiptId, TaskId, TeamRunId, Timestamp,
};
use crate::spec::ModelRung;
use crate::state::NativeRuntimeIdentity;
use crate::{DomainError, DomainResult};

crate::closed_enum! {
    /// Durable position in one seat-succession attempt.
    SuccessionAttemptState, "SuccessionAttemptState" {
        /// The replacement route is frozen and may run immediately.
        Planned => "planned",
        /// Placement is waiting without a fabricated successor route.
        Deferred => "deferred",
        /// The exact predecessor has been retired after a handoff was recorded.
        PredecessorRetired => "predecessor_retired",
        /// The exact successor binding has been observed from the runtime.
        SuccessorObserved => "successor_observed",
        /// A refetch confirmed the successor and an immutable receipt exists.
        Confirmed => "confirmed",
        /// A typed terminal refusal stopped the attempt.
        Refused => "refused",
    }
}

crate::closed_enum! {
    /// Which mandatory redaction pass one handoff receipt proves.
    SuccessionRedactionPass, "SuccessionRedactionPass" {
        /// Runtime material was redacted before it reached the summarizer.
        Input => "input",
        /// The derived summary was redacted before persistence.
        Output => "output",
    }
}

/// Modeled evidence for one redaction pass, with no source transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuccessionRedactionReceipt {
    /// Receipt document schema generation.
    pub schema_version: u32,
    /// Input or output pass.
    pub pass: SuccessionRedactionPass,
    /// Digest of the bounded material before this pass.
    pub source_hash: ContentHash,
    /// Digest of the bounded material after this pass.
    pub redacted_hash: ContentHash,
    /// Digest of the exact redaction policy revision.
    pub policy_hash: ContentHash,
    /// When this pass completed.
    pub redacted_at: Timestamp,
}

impl SuccessionRedactionReceipt {
    /// Validate the receipt schema and the pass expected at its position.
    pub fn validate_for(&self, expected: SuccessionRedactionPass) -> DomainResult<()> {
        if self.schema_version != 1 || self.pass != expected {
            return Err(DomainError::invalid(
                "SuccessionRedactionReceipt",
                "must be schema version 1 and name the expected pass",
            ));
        }
        Ok(())
    }
}

impl SuccessionAttemptState {
    /// Whether no later state may be written.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Refused)
    }

    /// Whether this attempt occupies its `(team, role)` slot.
    #[must_use]
    pub const fn is_nonterminal(self) -> bool {
        !self.is_terminal()
    }

    /// Whether one forward transition is legal.
    #[must_use]
    pub const fn can_advance_to(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Planned, Self::PredecessorRetired | Self::Refused)
                | (Self::Deferred, Self::Planned | Self::Refused)
                | (
                    Self::PredecessorRetired,
                    Self::SuccessorObserved | Self::Refused
                )
                | (Self::SuccessorObserved, Self::Confirmed | Self::Refused)
        )
    }

    /// Validate one requested transition.
    pub fn ensure_advance_to(self, to: Self) -> DomainResult<()> {
        if self.can_advance_to(to) {
            Ok(())
        } else {
            Err(DomainError::IllegalTransition {
                subject: "succession attempt",
                from: self.as_str(),
                to: to.as_str(),
            })
        }
    }
}

crate::closed_enum! {
    /// Stable terminal reasons that may refuse a succession attempt.
    ///
    /// Handoff unavailability is intentionally absent. A predecessor is never
    /// released until either a summary or a typed degraded handoff is durable.
    SuccessionRefusalReason, "SuccessionRefusalReason" {
        /// An expected aggregate revision or observation cursor moved.
        EvidenceStale => "evidence_stale",
        /// The authorizing quota state no longer blocks the predecessor route.
        QuotaNoLongerBlocking => "quota_no_longer_blocking",
        /// The runtime would not confirm retirement of the exact predecessor.
        RetirementRefused => "retirement_refused",
        /// The governed successor route could not be launched or adopted.
        LaunchRefused => "launch_refused",
        /// A refetch contradicted the successor identity being confirmed.
        ConfirmationRefused => "confirmation_refused",
    }
}

crate::closed_enum! {
    /// Why a handoff contains no generated summary.
    SuccessionHandoffDegradedReason, "SuccessionHandoffDegradedReason" {
        /// No governed summarizer route could be admitted.
        SummarizerUnplaceable => "summarizer_unplaceable",
        /// The runtime could not provide a stable timeline range.
        TimelineUnavailable => "timeline_unavailable",
        /// A generated summary failed bounded validation.
        SummaryRejected => "summary_rejected",
    }
}

/// Exact native timeline span considered by a handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessionTimelineRange {
    /// Runtime-owned timeline epoch.
    pub epoch: u64,
    /// First sequence included.
    pub start_sequence: u64,
    /// Last sequence included.
    pub end_sequence: u64,
}

impl SuccessionTimelineRange {
    /// Validate an ordered, non-empty span.
    pub fn validate(self) -> DomainResult<()> {
        if self.start_sequence == 0 || self.end_sequence < self.start_sequence {
            return Err(DomainError::invalid(
                "SuccessionTimelineRange",
                "must name a positive ordered sequence span",
            ));
        }
        Ok(())
    }
}

/// Either the digest of a bounded summary or an explicit degraded outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuccessionHandoffOutcome {
    /// A governed summarizer covered this exact range.
    Summary {
        /// Timeline range summarized.
        timeline: SuccessionTimelineRange,
        /// Exact provider/model/effort route used for the summary.
        summarizer_model_rung: ModelRung,
        /// Bounded, post-redaction derived summary. Runtime transcript material
        /// has no durable field in this type.
        summary: BoundedText,
        /// Digest of `summary`.
        summary_hash: ContentHash,
        /// Receipt proving redaction before summarization.
        input_redaction: SuccessionRedactionReceipt,
        /// Receipt proving redaction before persistence.
        output_redaction: SuccessionRedactionReceipt,
    },
    /// Succession continued with an explicit loss-of-context reason.
    Degraded {
        /// Timeline span, when the runtime could provide one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeline: Option<SuccessionTimelineRange>,
        /// Stable reason no summary was produced.
        reason: SuccessionHandoffDegradedReason,
        /// Receipt proving the input was redacted before the governed summary
        /// attempt was made.
        input_redaction: SuccessionRedactionReceipt,
        /// Receipt proving no unredacted derived output entered persistence.
        output_redaction: SuccessionRedactionReceipt,
    },
}

impl SuccessionHandoffOutcome {
    fn validate(&self) -> DomainResult<()> {
        match self {
            Self::Summary {
                timeline,
                summarizer_model_rung,
                summary,
                summary_hash,
                input_redaction,
                output_redaction,
            } => {
                timeline.validate()?;
                summarizer_model_rung.validate()?;
                if ContentHash::of(summary.as_str().as_bytes()) != *summary_hash {
                    return Err(DomainError::invalid(
                        "SuccessionHandoff",
                        "summary_hash must digest the stored post-redaction summary",
                    ));
                }
                input_redaction.validate_for(SuccessionRedactionPass::Input)?;
                output_redaction.validate_for(SuccessionRedactionPass::Output)?;
                if output_redaction.redacted_hash != *summary_hash {
                    return Err(DomainError::invalid(
                        "SuccessionHandoff",
                        "the output redaction receipt must produce the stored summary",
                    ));
                }
                Ok(())
            }
            Self::Degraded {
                timeline,
                reason,
                input_redaction,
                output_redaction,
            } => {
                if let Some(timeline) = timeline {
                    timeline.validate()?;
                }
                if *reason == SuccessionHandoffDegradedReason::TimelineUnavailable
                    && timeline.is_some()
                {
                    return Err(DomainError::invalid(
                        "SuccessionHandoff",
                        "timeline_unavailable may not carry a timeline range",
                    ));
                }
                input_redaction.validate_for(SuccessionRedactionPass::Input)?;
                output_redaction.validate_for(SuccessionRedactionPass::Output)?;
                Ok(())
            }
        }
    }

    /// Summary digest when this is a summarized handoff.
    #[must_use]
    pub const fn summary_hash(&self) -> Option<&ContentHash> {
        match self {
            Self::Summary { summary_hash, .. } => Some(summary_hash),
            Self::Degraded { .. } => None,
        }
    }
}

/// Immutable handoff frozen before the predecessor is retired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuccessionHandoff {
    /// Document schema generation.
    pub schema_version: u32,
    /// Attempt this evidence belongs to.
    pub attempt_id: SuccessionAttemptId,
    /// Exact predecessor logical run.
    pub predecessor_agent_run_id: AgentRunId,
    /// Exact predecessor binding.
    pub predecessor_runtime_binding_id: RuntimeBindingId,
    /// Exact predecessor native identity and generation.
    pub predecessor_native_identity: NativeRuntimeIdentity,
    /// Summary or explicit degraded reason.
    pub outcome: SuccessionHandoffOutcome,
    /// When the evidence was produced.
    pub produced_at: Timestamp,
}

impl SuccessionHandoff {
    /// Validate identity-independent handoff invariants.
    pub fn validate(&self) -> DomainResult<()> {
        if self.schema_version != 1 {
            return Err(DomainError::invalid(
                "SuccessionHandoff",
                "schema_version must be 1",
            ));
        }
        self.outcome.validate()
    }

    /// Canonical bytes and digest used by an attempt and its receipt.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    /// Canonical handoff digest.
    pub fn hash(&self) -> DomainResult<ContentHash> {
        self.canonicalize().map(|document| document.hash().clone())
    }

    /// Summary digest when this handoff has one.
    #[must_use]
    pub const fn summary_hash(&self) -> Option<&ContentHash> {
        self.outcome.summary_hash()
    }
}

/// Decision persisted before effects, with refreshable authority only while deferred.
///
/// Identity, task/team/slot, predecessor binding, task/team revisions,
/// idempotency key, initial intent hash and creation instant never change. A
/// due deferral may replace its predecessor/quota observation fields through
/// [`SuccessionDeferredRefresh`] before any handoff or runtime effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSuccessionAttempt {
    /// Attempt identity.
    pub id: SuccessionAttemptId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Exact task whose seat is changing.
    pub task_id: TaskId,
    /// Exact team and role slot.
    pub team_run_id: TeamRunId,
    /// Role key within the frozen team.
    pub role: RoleKey,
    /// Exact predecessor run, binding and native generation.
    pub predecessor_agent_run_id: AgentRunId,
    /// Immutable runtime binding held by the predecessor.
    pub predecessor_runtime_binding_id: RuntimeBindingId,
    /// Native predecessor session including runtime generation.
    pub predecessor_native_identity: NativeRuntimeIdentity,
    /// Aggregate revisions observed while planning.
    pub expected_task_revision: AggregateRevision,
    /// Team revision observed while planning.
    pub expected_team_revision: AggregateRevision,
    /// Predecessor revision observed while planning.
    pub expected_predecessor_revision: AggregateRevision,
    /// Exact reduced runtime observation that showed the blocked predecessor.
    pub runtime_observation_cursor: EventCursor,
    /// Exact quota projection and immutable provenance authorizing succession.
    pub quota_provenance_id: QuotaObservationProvenanceId,
    /// Quota projection revision observed while planning.
    pub quota_state_revision: AggregateRevision,
    /// Digest stored by both quota projection and provenance.
    pub quota_evidence_hash: ContentHash,
    /// Provider route on which the predecessor was blocked.
    pub quota_provider: String,
    /// Frozen successor route, absent while a placement wait is deferred.
    pub successor_model_rung: Option<ModelRung>,
    /// Exact successor account, absent while a placement wait is deferred.
    pub successor_account_profile_id: Option<AccountProfileId>,
    /// Stable replay identity and digest of the complete planning intent.
    pub idempotency_key: IdempotencyKey,
    /// Digest of the initial placement intent.
    pub intent_hash: ContentHash,
    /// Earliest retry instant for a `Placement::Wait` decision.
    pub deferred_until: Option<Timestamp>,
    /// Planning instant.
    pub created_at: Timestamp,
}

impl NewSuccessionAttempt {
    /// Validate the frozen decision and compute its initial durable state.
    pub fn initial_state(&self) -> DomainResult<SuccessionAttemptState> {
        if self.quota_provider.trim().is_empty() || self.quota_provider.len() > 128 {
            return Err(DomainError::invalid(
                "NewSuccessionAttempt",
                "quota provider must be a bounded non-empty route",
            ));
        }
        match (
            self.successor_model_rung.as_ref(),
            self.successor_account_profile_id,
            self.deferred_until,
        ) {
            (Some(model_rung), Some(_), None) => {
                model_rung.validate()?;
                Ok(SuccessionAttemptState::Planned)
            }
            (None, None, Some(until)) if until > self.created_at => {
                Ok(SuccessionAttemptState::Deferred)
            }
            (None, None, Some(_)) => Err(DomainError::invalid(
                "NewSuccessionAttempt",
                "deferred_until must be later than created_at",
            )),
            _ => Err(DomainError::invalid(
                "NewSuccessionAttempt",
                "planned attempts require one exact route/account pair and deferred attempts require only deferred_until",
            )),
        }
    }
}

/// Deferred-only CAS that refreshes authority and the next placement decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessionDeferredRefresh {
    /// Owning project.
    pub project_id: ProjectId,
    /// Existing durable attempt; its identity and idempotency key are retained.
    pub attempt_id: SuccessionAttemptId,
    /// Attempt revision observed by the caller.
    pub expected_revision: AggregateRevision,
    /// Current predecessor revision after the fresh blocked observation.
    pub expected_predecessor_revision: AggregateRevision,
    /// Current reduced reachable-blocked runtime observation.
    pub runtime_observation_cursor: EventCursor,
    /// Current immutable quota provenance joined to that observation.
    pub quota_provenance_id: QuotaObservationProvenanceId,
    /// Current quota projection revision.
    pub quota_state_revision: AggregateRevision,
    /// Digest shared by the current quota row and provenance.
    pub quota_evidence_hash: ContentHash,
    /// Provider route proved blocked by the refreshed evidence.
    pub quota_provider: String,
    /// Real successor route selected from fresh headroom, or absent for Wait.
    pub successor_model_rung: Option<ModelRung>,
    /// Real successor account selected from fresh headroom, or absent for Wait.
    pub successor_account_profile_id: Option<AccountProfileId>,
    /// Next exact Wait instant, absent when a successor is admitted.
    pub deferred_until: Option<Timestamp>,
    /// Instant at which the due attempt was re-observed and replanned.
    pub refreshed_at: Timestamp,
}

impl SuccessionDeferredRefresh {
    /// Validate the refreshed authority shape and return its resulting state.
    pub fn resulting_state(&self) -> DomainResult<SuccessionAttemptState> {
        if self.quota_provider.trim().is_empty() || self.quota_provider.len() > 128 {
            return Err(DomainError::invalid(
                "SuccessionDeferredRefresh",
                "quota provider must be a bounded non-empty route",
            ));
        }
        match (
            self.successor_model_rung.as_ref(),
            self.successor_account_profile_id,
            self.deferred_until,
        ) {
            (Some(model_rung), Some(_), None) => {
                model_rung.validate()?;
                Ok(SuccessionAttemptState::Planned)
            }
            (None, None, Some(until)) if until > self.refreshed_at => {
                Ok(SuccessionAttemptState::Deferred)
            }
            (None, None, Some(_)) => Err(DomainError::invalid(
                "SuccessionDeferredRefresh",
                "a renewed deferred_until must be later than refreshed_at",
            )),
            _ => Err(DomainError::invalid(
                "SuccessionDeferredRefresh",
                "admission requires one route/account pair and Wait requires only deferred_until",
            )),
        }
    }
}

/// Exact successor readback captured before confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessionSuccessorObservation {
    /// Successor run created under the predecessor lineage.
    pub agent_run_id: AgentRunId,
    /// Successor runtime binding.
    pub runtime_binding_id: RuntimeBindingId,
    /// Exact refetched native identity.
    pub native_identity: NativeRuntimeIdentity,
    /// Exact runtime observation reduced into the successor projection.
    pub runtime_observation_cursor: EventCursor,
    /// Observation instant.
    pub observed_at: Timestamp,
}

/// Complete durable state of one succession saga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessionAttempt {
    /// Planning decision; only deferred observation/placement fields may refresh.
    pub request: NewSuccessionAttempt,
    /// Forward-only state.
    pub state: SuccessionAttemptState,
    /// Handoff written before retirement.
    pub handoff: Option<SuccessionHandoff>,
    /// Canonical digest of `handoff`.
    pub handoff_hash: Option<ContentHash>,
    /// Exact successor runtime readback.
    pub successor: Option<SuccessionSuccessorObservation>,
    /// Typed terminal refusal.
    pub refusal_reason: Option<SuccessionRefusalReason>,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
    /// Most recent durable change.
    pub updated_at: Timestamp,
    /// Instant the real successor route/account pair was frozen.
    pub successor_planned_at: Option<Timestamp>,
    /// Predecessor retirement instant.
    pub predecessor_retired_at: Option<Timestamp>,
    /// Confirmation instant.
    pub confirmed_at: Option<Timestamp>,
    /// Refusal instant.
    pub refused_at: Option<Timestamp>,
}

impl SuccessionAttempt {
    /// Whether a deferred attempt may be acted on at `now`.
    #[must_use]
    pub fn is_due(&self, now: Timestamp) -> bool {
        match self.state {
            SuccessionAttemptState::Deferred => self
                .request
                .deferred_until
                .is_some_and(|deferred_until| deferred_until <= now),
            state => state.is_nonterminal(),
        }
    }
}

/// Compare-and-swap fields shared by simple state advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccessionAttemptAdvance {
    /// Owning project.
    pub project_id: ProjectId,
    /// Attempt to advance.
    pub attempt_id: SuccessionAttemptId,
    /// Revision the caller observed.
    pub expected_revision: AggregateRevision,
    /// When the runtime effect was confirmed.
    pub occurred_at: Timestamp,
}

/// Write-once handoff attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessionHandoffRecord {
    /// Owning project.
    pub project_id: ProjectId,
    /// Attempt receiving the handoff.
    pub attempt_id: SuccessionAttemptId,
    /// Revision the caller observed.
    pub expected_revision: AggregateRevision,
    /// Immutable summary-or-degraded handoff.
    pub handoff: SuccessionHandoff,
    /// Persistence instant.
    pub recorded_at: Timestamp,
}

/// Compare-and-swap successor readback attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessionSuccessorRecord {
    /// Owning project.
    pub project_id: ProjectId,
    /// Attempt receiving the readback.
    pub attempt_id: SuccessionAttemptId,
    /// Revision the caller observed.
    pub expected_revision: AggregateRevision,
    /// Exact successor runtime observation.
    pub observation: SuccessionSuccessorObservation,
}

/// Terminal typed refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccessionRefusal {
    /// Owning project.
    pub project_id: ProjectId,
    /// Attempt being refused.
    pub attempt_id: SuccessionAttemptId,
    /// Revision the caller observed.
    pub expected_revision: AggregateRevision,
    /// Stable refusal reason.
    pub reason: SuccessionRefusalReason,
    /// Refusal instant.
    pub refused_at: Timestamp,
}

/// Immutable readback proving one succession completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuccessionReceipt {
    /// Receipt document schema generation.
    pub schema_version: u32,
    /// Receipt identity.
    pub id: SuccessionReceiptId,
    /// Attempt this receipt confirms.
    pub attempt_id: SuccessionAttemptId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Task whose exact slot changed.
    pub task_id: TaskId,
    /// Team whose exact slot changed.
    pub team_run_id: TeamRunId,
    /// Role whose seat changed.
    pub role: RoleKey,
    /// Retired predecessor logical run.
    pub predecessor_agent_run_id: AgentRunId,
    /// Retired predecessor binding.
    pub predecessor_runtime_binding_id: RuntimeBindingId,
    /// Retired predecessor native identity.
    pub predecessor_native_identity: NativeRuntimeIdentity,
    /// Installed successor logical run.
    pub successor_agent_run_id: AgentRunId,
    /// Installed successor binding.
    pub successor_runtime_binding_id: RuntimeBindingId,
    /// Installed successor native identity.
    pub successor_native_identity: NativeRuntimeIdentity,
    /// Exact refetched successor observation.
    pub successor_runtime_observation_cursor: EventCursor,
    /// Exact blocked observation that authorized planning.
    pub authorizing_runtime_observation_cursor: EventCursor,
    /// Immutable quota provenance that authorized planning.
    pub quota_provenance_id: QuotaObservationProvenanceId,
    /// Quota revision that authorized planning.
    pub quota_state_revision: AggregateRevision,
    /// Quota evidence digest that authorized planning.
    pub quota_evidence_hash: ContentHash,
    /// Canonical predecessor handoff digest.
    pub handoff_hash: ContentHash,
    /// Summary digest, absent only for an explicit degraded handoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_hash: Option<ContentHash>,
    /// Frozen succession intent digest.
    pub intent_hash: ContentHash,
    /// Confirmation instant.
    pub confirmed_at: Timestamp,
}

impl SuccessionReceipt {
    /// Validate schema and canonicalize the immutable receipt.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        if self.schema_version != 1 {
            return Err(DomainError::invalid(
                "SuccessionReceipt",
                "schema_version must be 1",
            ));
        }
        CanonicalDocument::from_serializable(self)
    }

    /// Prove every receipt link against the observed attempt.
    pub fn validate_against(&self, attempt: &SuccessionAttempt) -> DomainResult<()> {
        let request = &attempt.request;
        let handoff = attempt
            .handoff
            .as_ref()
            .ok_or(DomainError::MissingEvidence {
                subject: "succession confirmation",
                rule: "the predecessor has no durable handoff",
            })?;
        let successor = attempt
            .successor
            .as_ref()
            .ok_or(DomainError::MissingEvidence {
                subject: "succession confirmation",
                rule: "the successor has no confirmed runtime readback",
            })?;
        let handoff_hash = handoff.hash()?;
        let linked = self.attempt_id == request.id
            && self.project_id == request.project_id
            && self.task_id == request.task_id
            && self.team_run_id == request.team_run_id
            && self.role == request.role
            && self.predecessor_agent_run_id == request.predecessor_agent_run_id
            && self.predecessor_runtime_binding_id == request.predecessor_runtime_binding_id
            && self.predecessor_native_identity == request.predecessor_native_identity
            && self.successor_agent_run_id == successor.agent_run_id
            && self.successor_runtime_binding_id == successor.runtime_binding_id
            && self.successor_native_identity == successor.native_identity
            && self.successor_runtime_observation_cursor == successor.runtime_observation_cursor
            && self.authorizing_runtime_observation_cursor == request.runtime_observation_cursor
            && self.quota_provenance_id == request.quota_provenance_id
            && self.quota_state_revision == request.quota_state_revision
            && self.quota_evidence_hash == request.quota_evidence_hash
            && self.handoff_hash == handoff_hash
            && self.summary_hash.as_ref() == handoff.summary_hash()
            && self.intent_hash == request.intent_hash;
        if !linked {
            return Err(DomainError::invalid(
                "SuccessionReceipt",
                "does not exactly link the frozen attempt, handoff and successor readback",
            ));
        }
        self.canonicalize().map(|_| ())
    }
}

/// Confirmation command carrying the immutable receipt to insert atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessionConfirmation {
    /// Attempt revision observed after successor readback.
    pub expected_revision: AggregateRevision,
    /// Immutable receipt to insert atomically with confirmation.
    pub receipt: SuccessionReceipt,
}
