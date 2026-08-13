//! Normalized control-plane observations, correlation evidence and
//! reconciliation classification.
//!
//! An observation keeps two facts apart that a runtime happily conflates:
//!
//! * [`RuntimeContact`] — what happened to *the channel*.
//! * [`ObservedRunState`] — what the runtime said about *the work*.
//!
//! An unreachable runtime, a missing process or a closed stream is a fact about
//! the channel and says nothing about the work. That is why
//! [`ControlPlaneObservation::terminal_evidence`] refuses to close a run on
//! anything but a matching authoritative event or a fresh inspect result at a
//! grade that is allowed to evidence it.

use kontor_core::id::{AgentRunId, CanonicalDocument, ContentHash, EventCursor, Timestamp};
use kontor_core::state::{
    DerivedRunState, NativeRuntimeIdentity, ObservedRunState, RuntimeContact, RuntimeObservation,
    TerminalOutcome,
};
use kontor_core::{
    id::{ExternalId, RuntimeBindingId},
    state::TerminalEvidenceSource,
};
use serde::{Deserialize, Serialize};

use crate::adapter::{RuntimeError, RuntimeResult};
use crate::capability::{IssuedBinding, RuntimeBindingSnapshot, TrustGrade};
use crate::request::CorrelationLabel;

/// Where one observation came from.
///
/// This is the evidence class, not the transport. A command acknowledgement and
/// an advisory report are both real answers from a runtime; neither is proof
/// that the work reached the state they mention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    /// A trusted event the runtime emitted about the session itself.
    AuthoritativeEvent,
    /// A fresh, direct read of the session's current state.
    Inspect,
    /// The runtime accepted a command. It has not said the command took effect.
    CommandAck,
    /// A report from a runtime that is only trusted to describe, not to prove.
    AdvisoryReport,
}

/// Whether a runtime at `trust` may close a run on evidence from `source`.
///
/// * Grade A proves state through its own events and through inspect.
/// * Grade B has incomplete replay, so an event stream is not proof; only a
///   fresh inspect is.
/// * Grade C proves nothing. Its reports populate the adoption inbox.
#[must_use]
pub const fn may_evidence_terminal(trust: TrustGrade, source: ObservationSource) -> bool {
    matches!(
        (trust, source),
        (
            TrustGrade::A,
            ObservationSource::AuthoritativeEvent | ObservationSource::Inspect
        ) | (TrustGrade::B, ObservationSource::Inspect)
    )
}

/// One normalized control-plane fact about a native session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneObservation {
    /// The Kontor run this observation is about.
    pub agent_run_id: AgentRunId,
    /// What happened to the channel.
    pub contact: RuntimeContact,
    /// What the runtime said about the work.
    pub state: ObservedRunState,
    /// Which native session reported it.
    pub identity: NativeRuntimeIdentity,
    /// The runtime's own event id, when it provides one.
    pub native_event_id: Option<ExternalId>,
    /// The runtime's own monotonic ordering for this fact.
    pub native_sequence: u64,
    /// When the runtime reported it.
    pub observed_at: Timestamp,
    /// The canonical raw payload this observation reduces.
    pub evidence: CanonicalDocument,
    /// The evidence class this fact belongs to.
    pub source: ObservationSource,
}

impl ControlPlaneObservation {
    /// Digest of the canonical payload, as KON-MVP-03 stores it.
    #[must_use]
    pub const fn evidence_hash(&self) -> &ContentHash {
        self.evidence.hash()
    }

    /// The KON-MVP-03 observation this normalizes to, once the store has
    /// allocated the local cursor for the raw event.
    #[must_use]
    pub fn to_core_observation(&self, cursor: EventCursor) -> RuntimeObservation {
        RuntimeObservation {
            agent_run_id: self.agent_run_id,
            state: self.state,
            identity: self.identity.clone(),
            cursor,
            observed_at: self.observed_at,
            evidence_hash: self.evidence_hash().clone(),
        }
    }

    /// Where a closure citing this observation would point.
    #[must_use]
    pub const fn terminal_evidence_source(cursor: EventCursor) -> TerminalEvidenceSource {
        TerminalEvidenceSource::RuntimeObservation { cursor }
    }

    /// The terminal outcome this observation may actually close `binding` on.
    ///
    /// Two things about the grade are deliberate. It is taken from the
    /// binding's **frozen** snapshot rather than from the caller, because were
    /// it an argument a Grade C report would close a run the moment any call
    /// site passed [`TrustGrade::A`] — by mistake or otherwise. And the
    /// snapshot has to be an [`IssuedBinding`], because a snapshot is a value
    /// with public fields: a caller who could hand over its own copy would just
    /// write the better grade into it and be believed. Only the runtime that
    /// issued a binding can say what it was issued at.
    ///
    /// Returns `None` for every uncertain input: a broken channel, an
    /// observation about another run or another native session, an evidence
    /// class that only acknowledges or describes, a grade that may not evidence
    /// closure, an observation older than `max_age_seconds`, and any
    /// non-terminal reported state.
    #[must_use]
    pub fn terminal_evidence(
        &self,
        binding: &IssuedBinding,
        now: Timestamp,
        max_age_seconds: i64,
    ) -> Option<TerminalOutcome> {
        let binding = binding.snapshot();
        if !matches!(self.contact, RuntimeContact::Reachable) {
            return None;
        }
        // The observation has to be about this binding's work, in the session
        // this binding names. A repeated native id in a new generation is a
        // different session, and `same_session` is what says so.
        if self.agent_run_id != binding.agent_run_id()
            || !self.identity.same_session(binding.identity())
        {
            return None;
        }
        if !may_evidence_terminal(binding.capabilities.trust_grade, self.source) {
            return None;
        }
        // A closure is a claim about *now*. An observation left to age is a
        // description of the past, whatever it says.
        if now.duration_since(self.observed_at).as_secs() > max_age_seconds {
            return None;
        }
        self.state.observed_terminal_outcome()
    }
}

/// Proof that a native session belongs to the run that asked for it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CorrelationEvidence {
    /// The Kontor label the runtime reported back.
    pub label: CorrelationLabel,
    /// The native session that reported it.
    pub native: NativeRuntimeIdentity,
    /// When the correlation was established.
    pub established_at: Timestamp,
}

impl CorrelationEvidence {
    /// Establish correlation from what a runtime reported.
    ///
    /// `reported` is raw runtime text. It must be exactly the label Kontor
    /// planted for `agent_run_id`: a native session id, a label for another run,
    /// or a missing label are all refusals, never a silent bind.
    ///
    /// # Errors
    /// Returns [`RuntimeError::CorrelationFailed`] when the runtime did not
    /// report this run's label.
    pub fn establish(
        agent_run_id: AgentRunId,
        reported: &str,
        native: NativeRuntimeIdentity,
        established_at: Timestamp,
    ) -> RuntimeResult<Self> {
        let label = CorrelationLabel::for_run(agent_run_id);
        if reported != label.to_string() {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(Self {
            label,
            native,
            established_at,
        })
    }
}

/// A native session as discovery found it, before Kontor assigns any identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSession {
    /// The native session.
    pub identity: NativeRuntimeIdentity,
    /// The Kontor label it carries, when it carries a parseable one.
    pub correlation: Option<CorrelationLabel>,
    /// What the runtime says the session is doing.
    pub state: ObservedRunState,
    /// When it was discovered.
    pub observed_at: Timestamp,
}

/// One classification produced by reconciling bindings against discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationFinding {
    /// The bound session exists in the current generation.
    Matched {
        /// The run.
        agent_run_id: AgentRunId,
        /// The binding.
        binding_id: RuntimeBindingId,
        /// The native session, unchanged.
        identity: NativeRuntimeIdentity,
    },
    /// A session with the bound native id exists, but in another generation. A
    /// repeated native id after a restart is a different session.
    GenerationChanged {
        /// The run.
        agent_run_id: AgentRunId,
        /// The binding.
        binding_id: RuntimeBindingId,
        /// What Kontor bound.
        bound: NativeRuntimeIdentity,
        /// What discovery found.
        found: NativeRuntimeIdentity,
    },
    /// The bound session is not there at all. This is lost contact, never
    /// completion.
    MissingSession {
        /// The run.
        agent_run_id: AgentRunId,
        /// The binding.
        binding_id: RuntimeBindingId,
        /// What Kontor bound.
        bound: NativeRuntimeIdentity,
    },
    /// The runtime does not vouch for this binding at all.
    ///
    /// A [`RuntimeBindingSnapshot`] is a plain value with public fields, so one
    /// can be fabricated to name any native session. An adapter that keeps an
    /// [`IssuedBindingRegistry`](crate::capability::IssuedBindingRegistry)
    /// resolves every presented snapshot through it first, and reports this
    /// rather than [`Self::Matched`] — whose action is `Keep` — for anything the
    /// registry did not issue, or issued with different values. It is the
    /// reconciliation counterpart of [`RuntimeError::StaleBinding`].
    ///
    /// Distinct from [`Self::GenerationChanged`], which is the narrower claim
    /// that a session with this native id exists in *another* generation. This
    /// one makes no claim about the session at all — only that the binding is not
    /// the runtime's.
    Unattested {
        /// The run the binding claims.
        agent_run_id: AgentRunId,
        /// The binding.
        binding_id: RuntimeBindingId,
        /// The identity the snapshot presented. The runtime never issued it, so
        /// it is what was claimed rather than what was bound.
        presented: NativeRuntimeIdentity,
    },
    /// An unbound session carrying a Kontor label for a run with no binding.
    Adoptable {
        /// The run the session claims.
        agent_run_id: AgentRunId,
        /// The native session.
        identity: NativeRuntimeIdentity,
    },
    /// An unbound session Kontor cannot claim.
    Orphan {
        /// The native session.
        identity: NativeRuntimeIdentity,
    },
}

/// What reconciliation *proposes*. It never rebinds on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReconciliationAction {
    /// Nothing to do; the binding still holds.
    Keep,
    /// Review the binding as orphaned.
    ProposeOrphanReview,
    /// Review the binding as having lost contact.
    ProposeLostContactReview,
    /// Offer the session in the adoption inbox.
    ProposeAdoption,
    /// Record the session in the adoption inbox without a run to offer it to.
    ProposeInboxEntry,
}

impl ReconciliationFinding {
    /// The derived state this finding supports, for the bindings it concerns.
    ///
    /// Every non-matching outcome is an uncertainty variant: reconciliation can
    /// never conclude that work finished.
    #[must_use]
    pub const fn proposed_state(&self) -> Option<DerivedRunState> {
        match self {
            Self::Matched { .. } | Self::Adoptable { .. } | Self::Orphan { .. } => None,
            Self::GenerationChanged { .. } | Self::Unattested { .. } => {
                Some(DerivedRunState::Orphaned)
            }
            Self::MissingSession { .. } => Some(DerivedRunState::LostContact),
        }
    }

    /// What Kontor should do about it.
    #[must_use]
    pub const fn action(&self) -> ReconciliationAction {
        match self {
            Self::Matched { .. } => ReconciliationAction::Keep,
            Self::GenerationChanged { .. } | Self::Unattested { .. } => {
                ReconciliationAction::ProposeOrphanReview
            }
            Self::MissingSession { .. } => ReconciliationAction::ProposeLostContactReview,
            Self::Adoptable { .. } => ReconciliationAction::ProposeAdoption,
            Self::Orphan { .. } => ReconciliationAction::ProposeInboxEntry,
        }
    }
}

/// The result of one reconciliation epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationReport {
    /// The runtime generation this epoch observed.
    pub generation: u64,
    /// Every binding and every discovered session, classified.
    pub findings: Vec<ReconciliationFinding>,
}

impl ReconciliationReport {
    /// Whether any finding needs an operator or scheduler decision.
    #[must_use]
    pub fn needs_review(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.action() != ReconciliationAction::Keep)
    }
}

/// Classify every binding and every discovered session.
///
/// The output is a proposal. Nothing here rebinds a run, adopts a session or
/// closes anything: a changed generation yields an orphan classification rather
/// than a quiet re-point onto the new session.
#[must_use]
pub fn reconcile(
    bindings: &[RuntimeBindingSnapshot],
    sessions: &[NativeSession],
    generation: u64,
) -> ReconciliationReport {
    let mut findings = Vec::with_capacity(bindings.len() + sessions.len());
    let mut claimed: Vec<&NativeRuntimeIdentity> = Vec::with_capacity(bindings.len());

    for snapshot in bindings {
        let bound = snapshot.identity();
        let found = sessions
            .iter()
            .find(|session| same_native_id(&session.identity, bound));
        let finding = match found {
            None => ReconciliationFinding::MissingSession {
                agent_run_id: snapshot.agent_run_id(),
                binding_id: snapshot.binding_id(),
                bound: bound.clone(),
            },
            Some(session) => {
                claimed.push(&session.identity);
                if session.identity.same_session(bound) {
                    ReconciliationFinding::Matched {
                        agent_run_id: snapshot.agent_run_id(),
                        binding_id: snapshot.binding_id(),
                        identity: session.identity.clone(),
                    }
                } else {
                    ReconciliationFinding::GenerationChanged {
                        agent_run_id: snapshot.agent_run_id(),
                        binding_id: snapshot.binding_id(),
                        bound: bound.clone(),
                        found: session.identity.clone(),
                    }
                }
            }
        };
        findings.push(finding);
    }

    for session in sessions {
        if claimed
            .iter()
            .any(|identity| same_native_id(identity, &session.identity))
        {
            continue;
        }
        let adoptable = session.correlation.filter(|label| {
            !bindings
                .iter()
                .any(|snapshot| snapshot.agent_run_id() == label.agent_run_id())
        });
        findings.push(match adoptable {
            Some(label) => ReconciliationFinding::Adoptable {
                agent_run_id: label.agent_run_id(),
                identity: session.identity.clone(),
            },
            None => ReconciliationFinding::Orphan {
                identity: session.identity.clone(),
            },
        });
    }

    ReconciliationReport {
        generation,
        findings,
    }
}

/// Whether two identities name the same native session, ignoring generation.
fn same_native_id(left: &NativeRuntimeIdentity, right: &NativeRuntimeIdentity) -> bool {
    left.runtime_kind == right.runtime_kind
        && left.host == right.host
        && left.native_id == right.native_id
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use kontor_core::id::{AgentRunId, ExternalName, RuntimeKindKey, parse_utc_timestamp};
    use kontor_core::repository::RuntimeBinding;

    use super::*;
    use crate::capability::{RuntimeCapabilities, RuntimeCapability, RuntimeLimits};
    use crate::request::CorrelationLabel;

    const NOW: &str = "2026-08-10T09:00:30Z";
    const WINDOW: i64 = 60;

    fn identity(generation: u64) -> NativeRuntimeIdentity {
        NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("valid runtime key"),
            host: ExternalName::parse("fake-host").expect("valid host"),
            generation,
            native_id: kontor_core::id::ExternalId::parse("session-1").expect("valid native id"),
        }
    }

    /// A consistent binding for `agent_run_id` at `trust`.
    fn binding(agent_run_id: AgentRunId, trust: TrustGrade) -> RuntimeBindingSnapshot {
        let at = parse_utc_timestamp("2026-08-10T08:59:00Z").expect("canonical UTC");
        RuntimeBindingSnapshot {
            binding: RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id,
                identity: identity(1),
                bound_at: at,
            },
            capabilities: RuntimeCapabilities {
                trust_grade: trust,
                supported: RuntimeCapability::ALL
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>(),
                account_env: true,
                limits: RuntimeLimits {
                    max_message_bytes: 64,
                    max_history_page: 10,
                    max_concurrent_sessions: 2,
                    context_window: kontor_core::spec::ContextWindowBounds::unknown(),
                },
            },
            correlation: CorrelationEvidence {
                label: CorrelationLabel::for_run(agent_run_id),
                native: identity(1),
                established_at: at,
            },
        }
    }

    /// The runtime's own copy of that binding, as it would hand it back.
    fn issued(agent_run_id: AgentRunId, trust: TrustGrade) -> IssuedBinding {
        IssuedBinding::attest(binding(agent_run_id, trust))
            .expect("a binding the runtime issued is consistent")
    }

    fn observation(
        agent_run_id: AgentRunId,
        contact: RuntimeContact,
        state: ObservedRunState,
        source: ObservationSource,
    ) -> ControlPlaneObservation {
        ControlPlaneObservation {
            agent_run_id,
            contact,
            state,
            identity: identity(1),
            native_event_id: None,
            native_sequence: 1,
            observed_at: parse_utc_timestamp("2026-08-10T09:00:00Z").expect("canonical UTC"),
            evidence: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "state": state.as_str(),
            }))
            .expect("canonical payload"),
            source,
        }
    }

    fn now() -> Timestamp {
        parse_utc_timestamp(NOW).expect("canonical UTC")
    }

    #[test]
    fn a_command_acknowledgement_never_closes_a_run() {
        let run = AgentRunId::generate();
        let observed = observation(
            run,
            RuntimeContact::Reachable,
            ObservedRunState::Cancelled,
            ObservationSource::CommandAck,
        );
        assert_eq!(
            observed.terminal_evidence(&issued(run, TrustGrade::A), now(), WINDOW),
            None
        );
    }

    #[test]
    fn a_broken_channel_never_closes_a_run() {
        let run = AgentRunId::generate();
        let observed = observation(
            run,
            RuntimeContact::StreamClosed,
            ObservedRunState::Succeeded,
            ObservationSource::AuthoritativeEvent,
        );
        assert_eq!(
            observed.terminal_evidence(&issued(run, TrustGrade::A), now(), WINDOW),
            None
        );
    }

    #[test]
    fn only_an_allowed_grade_and_source_closes_a_run() {
        let run = AgentRunId::generate();
        let event = observation(
            run,
            RuntimeContact::Reachable,
            ObservedRunState::Succeeded,
            ObservationSource::AuthoritativeEvent,
        );
        let inspect = observation(
            run,
            RuntimeContact::Reachable,
            ObservedRunState::Succeeded,
            ObservationSource::Inspect,
        );
        assert_eq!(
            event.terminal_evidence(&issued(run, TrustGrade::A), now(), WINDOW),
            Some(TerminalOutcome::Succeeded)
        );
        assert_eq!(
            event.terminal_evidence(&issued(run, TrustGrade::B), now(), WINDOW),
            None
        );
        assert_eq!(
            inspect.terminal_evidence(&issued(run, TrustGrade::B), now(), WINDOW),
            Some(TerminalOutcome::Succeeded)
        );
        assert_eq!(
            inspect.terminal_evidence(&issued(run, TrustGrade::C), now(), WINDOW),
            None
        );
    }

    #[test]
    fn the_grade_comes_from_the_binding_not_from_the_caller() {
        // The very mutant this signature exists to kill: a Grade C observation
        // promoted by whatever the call site felt like passing.
        let run = AgentRunId::generate();
        let inspect = observation(
            run,
            RuntimeContact::Reachable,
            ObservedRunState::Succeeded,
            ObservationSource::Inspect,
        );
        assert!(may_evidence_terminal(
            TrustGrade::A,
            ObservationSource::Inspect
        ));
        assert_eq!(
            inspect.terminal_evidence(&issued(run, TrustGrade::C), now(), WINDOW),
            None,
            "an advisory binding closes nothing, whatever the caller believes"
        );
    }

    #[test]
    fn an_observation_about_another_run_or_session_closes_nothing() {
        let run = AgentRunId::generate();
        let inspect = observation(
            run,
            RuntimeContact::Reachable,
            ObservedRunState::Succeeded,
            ObservationSource::Inspect,
        );

        let other_run = issued(AgentRunId::generate(), TrustGrade::A);
        assert_eq!(
            inspect.terminal_evidence(&other_run, now(), WINDOW),
            None,
            "another run's binding is not closed by this run's observation"
        );

        // A repeated native id in a new generation is a different session.
        let mut restarted = binding(run, TrustGrade::A);
        restarted.binding.identity = identity(2);
        restarted.correlation.native = identity(2);
        let restarted = IssuedBinding::attest(restarted).expect("still a consistent binding");
        assert_eq!(
            inspect.terminal_evidence(&restarted, now(), WINDOW),
            None,
            "a session from another generation is not this session"
        );
    }

    #[test]
    fn an_aged_observation_closes_nothing() {
        let run = AgentRunId::generate();
        let inspect = observation(
            run,
            RuntimeContact::Reachable,
            ObservedRunState::Succeeded,
            ObservationSource::Inspect,
        );
        let bound = issued(run, TrustGrade::A);
        // Observed at 09:00:00, judged 30 seconds later.
        assert_eq!(
            inspect.terminal_evidence(&bound, now(), 30),
            Some(TerminalOutcome::Succeeded)
        );
        assert_eq!(
            inspect.terminal_evidence(&bound, now(), 29),
            None,
            "a closure is a claim about now, not about half a minute ago"
        );
    }

    #[test]
    fn a_forged_binding_is_not_evidence() {
        let run = AgentRunId::generate();
        let mut forged = binding(run, TrustGrade::A);
        forged.correlation.label = CorrelationLabel::for_run(AgentRunId::generate());
        assert_eq!(
            forged
                .ensure_correlated()
                .expect_err("the label names another run"),
            RuntimeError::CorrelationFailed
        );

        let mut mismatched = binding(run, TrustGrade::A);
        mismatched.correlation.native = identity(9);
        assert_eq!(
            mismatched
                .ensure_correlated()
                .expect_err("the correlation was established against another session"),
            RuntimeError::CorrelationFailed
        );

        // Neither one can be vouched for, so neither one reaches a closure at
        // all: the refusal is in the way the evidence is asked for, not in a
        // check a call site has to remember.
        assert_eq!(
            IssuedBinding::attest(forged).expect_err("a forgery is not vouched for"),
            RuntimeError::CorrelationFailed
        );
        assert_eq!(
            IssuedBinding::attest(mismatched).expect_err("neither is this one"),
            RuntimeError::CorrelationFailed
        );

        binding(run, TrustGrade::A)
            .ensure_correlated()
            .expect("a binding the runtime actually issued is consistent");
    }
}
