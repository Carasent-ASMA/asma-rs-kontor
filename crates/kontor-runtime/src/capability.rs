//! Capability discovery, trust grades, frozen binding snapshots and the one
//! preflight every adapter operation passes before it reaches a runtime.
//!
//! Two rules shape this module:
//!
//! 1. **Evidence, not optimism.** A runtime declares what it can prove. An
//!    operation the declaration does not cover is refused *before* dispatch, so
//!    an unsupported capability can never produce a side effect.
//! 2. **The snapshot is frozen.** Every binding carries the exact capabilities,
//!    trust grade and limits discovered when it was created. A later adapter
//!    upgrade or downgrade cannot rewrite the evidence quality of an earlier
//!    run, in either direction.

use std::collections::BTreeSet;
use std::fmt;

use kontor_core::id::{AgentRunId, RuntimeBindingId};
use kontor_core::repository::RuntimeBinding;
use kontor_core::state::NativeRuntimeIdentity;
use serde::{Deserialize, Serialize};

use crate::adapter::{RuntimeError, RuntimeResult};
use crate::observation::CorrelationEvidence;
use crate::workspace::WorkspaceClaim;

/// One operation a runtime may or may not be able to perform.
///
/// The set is closed: an adapter cannot invent a capability, and the control
/// plane never branches on an unknown value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapability {
    /// Enumerate native sessions the runtime currently owns.
    Discovery,
    /// Make a team run's task workspace exist and be usable.
    PrepareWorkspace,
    /// Start a new native session for an agent run.
    Launch,
    /// Continue an existing native session in place.
    Resume,
    /// Deliver a message into an existing native session.
    SendMessage,
    /// Ask an existing native session to stop.
    Cancel,
    /// Read the current authoritative state of one native session.
    Inspect,
    /// Bind an already-running native session to an agent run.
    Adopt,
    /// Page through a session's recorded content.
    History,
    /// Follow a session's content as it is produced.
    LiveEvents,
    /// Answer a permission request raised inside a session.
    PermissionResponse,
}

impl RuntimeCapability {
    /// Every capability, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Discovery,
        Self::PrepareWorkspace,
        Self::Launch,
        Self::Resume,
        Self::SendMessage,
        Self::Cancel,
        Self::Inspect,
        Self::Adopt,
        Self::History,
        Self::LiveEvents,
        Self::PermissionResponse,
    ];

    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::PrepareWorkspace => "prepare_workspace",
            Self::Launch => "launch",
            Self::Resume => "resume",
            Self::SendMessage => "send_message",
            Self::Cancel => "cancel",
            Self::Inspect => "inspect",
            Self::Adopt => "adopt",
            Self::History => "history",
            Self::LiveEvents => "live_events",
            Self::PermissionResponse => "permission_response",
        }
    }

    /// Whether performing this operation changes the runtime rather than only
    /// reading it.
    ///
    /// Read-only operations stay available at every trust grade: a Grade C
    /// runtime may still be observed, it just may not be driven.
    #[must_use]
    pub const fn changes_runtime(self) -> bool {
        matches!(
            self,
            Self::PrepareWorkspace
                | Self::Launch
                | Self::Resume
                | Self::SendMessage
                | Self::Cancel
                | Self::Adopt
                | Self::PermissionResponse
        )
    }
}

impl fmt::Display for RuntimeCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How much of what a runtime reports Kontor is allowed to act on.
///
/// The ordering between grades is **policy**, not the declaration order of this
/// enum: [`TrustGrade::rank`] is the only comparison, so reordering the variants
/// cannot silently promote a runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustGrade {
    /// Stable native id, correlation, replay cursor, inspect and control.
    A,
    /// Stable native id, correlation and inspect; replay is incomplete.
    B,
    /// Discovery or liveness only. Observation and adoption inbox, nothing else.
    C,
}

impl TrustGrade {
    /// The stable spelling used in JSON, errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::C => "c",
        }
    }

    /// The explicit policy rank. Higher is more trusted.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::A => 2,
            Self::B => 1,
            Self::C => 0,
        }
    }

    /// Whether this grade is at least `minimum`.
    #[must_use]
    pub const fn at_least(self, minimum: Self) -> bool {
        self.rank() >= minimum.rank()
    }

    /// Whether Kontor may dispatch a runtime-changing command on its own
    /// authority through a runtime at this grade.
    ///
    /// Grade C is advisory: it may be discovered, inspected and read, but the
    /// scheduler may not drive it.
    #[must_use]
    pub const fn may_dispatch_autonomously(self) -> bool {
        matches!(self, Self::A | Self::B)
    }
}

impl fmt::Display for TrustGrade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The bounds a runtime declares for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLimits {
    /// Largest message body the runtime accepts, in bytes.
    pub max_message_bytes: u64,
    /// Largest history page the runtime will return.
    pub max_history_page: u32,
    /// Largest number of simultaneous native sessions.
    pub max_concurrent_sessions: u32,
}

/// One request's demand against [`RuntimeLimits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitDemand {
    /// A message body of this many bytes.
    MessageBytes(u64),
    /// A history page of this many items.
    HistoryPage(u32),
    /// This many simultaneous sessions.
    ConcurrentSessions(u32),
}

impl RuntimeLimits {
    /// Check one demand against the declared bounds.
    ///
    /// # Errors
    /// Returns [`RuntimeError::LimitExceeded`] naming the subject and the bound,
    /// never the request body.
    pub const fn check(&self, demand: LimitDemand) -> RuntimeResult<()> {
        let (subject, requested, limit) = match demand {
            LimitDemand::MessageBytes(bytes) => ("message body", bytes, self.max_message_bytes),
            LimitDemand::HistoryPage(items) => {
                ("history page", items as u64, self.max_history_page as u64)
            }
            LimitDemand::ConcurrentSessions(count) => (
                "concurrent sessions",
                count as u64,
                self.max_concurrent_sessions as u64,
            ),
        };
        if requested > limit {
            return Err(RuntimeError::LimitExceeded { subject, limit });
        }
        Ok(())
    }
}

/// Everything a runtime declares about itself at one point in time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    /// How much of what this runtime reports may be acted on.
    pub trust_grade: TrustGrade,
    /// The operations it can actually perform.
    pub supported: BTreeSet<RuntimeCapability>,
    /// Whether it can prove which coding account a run executes as.
    pub account_env: bool,
    /// The bounds it declares for one request.
    pub limits: RuntimeLimits,
}

impl RuntimeCapabilities {
    /// Whether `capability` is declared supported.
    #[must_use]
    pub fn supports(&self, capability: RuntimeCapability) -> bool {
        self.supported.contains(&capability)
    }
}

/// A runtime binding together with the evidence quality it was created under.
///
/// The `capabilities` field is a *frozen copy*: the binding keeps answering with
/// what the runtime could prove when the session was bound, whatever a later
/// discovery reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBindingSnapshot {
    /// The KON-MVP-03 binding between an agent run and a native session.
    pub binding: RuntimeBinding,
    /// The capabilities discovered when the binding was created.
    pub capabilities: RuntimeCapabilities,
    /// Proof that this native session belongs to this agent run.
    pub correlation: CorrelationEvidence,
}

impl RuntimeBindingSnapshot {
    /// The Kontor run this binding serves.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.binding.agent_run_id
    }

    /// The Kontor binding id.
    #[must_use]
    pub const fn binding_id(&self) -> RuntimeBindingId {
        self.binding.id
    }

    /// The native session this binding names.
    #[must_use]
    pub const fn identity(&self) -> &NativeRuntimeIdentity {
        &self.binding.identity
    }

    /// Prove the snapshot's own parts agree with each other.
    ///
    /// A binding is only evidence if the correlation travelling inside it is
    /// evidence *for that binding*. A snapshot whose label names another run,
    /// or whose correlation was established against another native session,
    /// proves nothing about the session it addresses — and that is exactly what
    /// a fabricated snapshot looks like: minting a plausible binding for a
    /// native id costs nothing, while re-deriving matching correlation is the
    /// part only the runtime can do.
    ///
    /// # Errors
    /// Returns [`RuntimeError::CorrelationFailed`] when the correlation does
    /// not belong to the binding it travels with.
    pub fn ensure_correlated(&self) -> RuntimeResult<()> {
        if self.correlation.label.agent_run_id() != self.binding.agent_run_id
            || self.correlation.native != self.binding.identity
        {
            return Err(RuntimeError::CorrelationFailed);
        }
        Ok(())
    }

    /// Prove the binding still names a session in the runtime's current
    /// generation.
    ///
    /// # Errors
    /// Returns [`RuntimeError::StaleBinding`] when the runtime has restarted
    /// since the binding was created. A repeated native id in a new generation
    /// is a different session, never the same one.
    pub const fn ensure_generation(&self, current: u64) -> RuntimeResult<()> {
        if self.binding.identity.generation == current {
            return Ok(());
        }
        Err(RuntimeError::StaleBinding {
            rule: "the runtime generation changed since this session was bound",
        })
    }
}

/// A binding snapshot the runtime that issued it has vouched for.
///
/// [`RuntimeBindingSnapshot`] is a plain value with public fields, so anyone
/// holding one can clone it and write [`TrustGrade::A`] into the copy. For a
/// *request* that is harmless — the runtime re-checks its own registry before
/// it acts. For a *closure* it is not: closing a run is the one conclusion
/// nothing walks back, and it is decided by the trust grade. So terminal
/// evidence is judged only against a snapshot that came back out of the issuing
/// runtime's registry, and this type is the proof it did: it can only be minted
/// inside this crate, by the adapter that holds the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedBinding(RuntimeBindingSnapshot);

impl IssuedBinding {
    /// Vouch for a snapshot the runtime found in its own registry.
    ///
    /// # Errors
    /// Returns [`RuntimeError::CorrelationFailed`] when the registered snapshot
    /// is not internally consistent, which no snapshot a runtime issued ever
    /// is.
    pub(crate) fn attest(snapshot: RuntimeBindingSnapshot) -> RuntimeResult<Self> {
        snapshot.ensure_correlated()?;
        Ok(Self(snapshot))
    }

    /// Read the vouched-for snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &RuntimeBindingSnapshot {
        &self.0
    }
}

/// Everything the shared preflight needs to decide one operation.
#[derive(Debug, Clone, Copy)]
pub struct OperationContext<'a> {
    /// The operation about to be attempted.
    pub operation: RuntimeCapability,
    /// Whether Kontor is acting on its own authority rather than relaying an
    /// explicit operator decision.
    pub autonomous: bool,
    /// Whether the run is pinned to a specific coding account.
    pub account_pinned: bool,
    /// The binding the operation addresses, for everything but launch.
    pub binding: Option<&'a RuntimeBindingSnapshot>,
    /// What the operation claims about the task workspace it will work in.
    /// Present for launch, absent for operations that address an already-bound
    /// session.
    pub workspace: Option<WorkspaceClaim<'a>>,
    /// The runtime's current generation, when it is known.
    pub current_generation: Option<u64>,
    /// The bound this request consumes, if any.
    pub demand: Option<LimitDemand>,
}

impl<'a> OperationContext<'a> {
    /// A minimal context for `operation`.
    #[must_use]
    pub const fn new(operation: RuntimeCapability) -> Self {
        Self {
            operation,
            autonomous: true,
            account_pinned: false,
            binding: None,
            workspace: None,
            current_generation: None,
            demand: None,
        }
    }

    /// The capabilities this operation is judged against.
    ///
    /// A bound operation is judged against the binding's frozen snapshot; only
    /// an unbound operation (launch, adopt, discovery) uses freshly discovered
    /// capabilities. Routing through one accessor is what makes the freeze rule
    /// impossible to bypass at a call site.
    #[must_use]
    pub fn effective(&self, discovered: &'a RuntimeCapabilities) -> &'a RuntimeCapabilities {
        match self.binding {
            Some(snapshot) => &snapshot.capabilities,
            None => discovered,
        }
    }
}

/// The single gate every adapter operation passes before it touches a runtime.
///
/// The order matters and is deliberate: capability, then trust, then account
/// policy, then the task workspace, then binding identity, then limits. Each
/// check must be able to refuse without the later ones having run, and none of
/// them may produce a side effect.
///
/// # Errors
/// * [`RuntimeError::UnsupportedCapability`] — the runtime never declared it.
/// * [`RuntimeError::InsufficientTrust`] — the grade may not drive this runtime.
/// * [`RuntimeError::AccountEnvironmentUnavailable`] — an account-pinned run
///   through a runtime that cannot prove the account environment.
/// * [`RuntimeError::WorkspaceBindingRequired`] — a launch that skipped
///   workspace preparation on a runtime that prepares workspaces.
/// * [`RuntimeError::WorkspaceMismatch`] — another team run's or task's
///   workspace, or a working directory that is not the verified root.
/// * [`RuntimeError::CorrelationFailed`] — a session or workspace binding whose
///   correlation evidence does not belong to the binding it travels with.
/// * [`RuntimeError::StaleBinding`] — the runtime restarted since binding.
/// * [`RuntimeError::LimitExceeded`] — the request is larger than declared.
pub fn preflight(
    discovered: &RuntimeCapabilities,
    context: &OperationContext<'_>,
) -> RuntimeResult<()> {
    let capabilities = context.effective(discovered);

    if !capabilities.supports(context.operation) {
        return Err(RuntimeError::UnsupportedCapability {
            capability: context.operation,
        });
    }

    if context.autonomous
        && context.operation.changes_runtime()
        && !capabilities.trust_grade.may_dispatch_autonomously()
    {
        return Err(RuntimeError::InsufficientTrust {
            found: capabilities.trust_grade,
            operation: context.operation,
            rule: "an advisory-grade runtime may be observed but not driven",
        });
    }

    if context.account_pinned && !capabilities.account_env {
        return Err(RuntimeError::AccountEnvironmentUnavailable);
    }

    // A runtime that prepares task workspaces only ever works inside one. This
    // runs before any effect precisely because a wrong-tree edit cannot be
    // taken back once it has happened.
    if let Some(claim) = context.workspace
        && capabilities.supports(RuntimeCapability::PrepareWorkspace)
    {
        claim.verify(context.current_generation)?;
    }

    // Identity before generation: a snapshot that is not internally consistent
    // is not a binding at all, so asking which generation it belongs to is
    // already asking the wrong question.
    if let Some(snapshot) = context.binding {
        snapshot.ensure_correlated()?;
        if let Some(generation) = context.current_generation {
            snapshot.ensure_generation(generation)?;
        }
    }

    if let Some(demand) = context.demand {
        capabilities.limits.check(demand)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities(trust_grade: TrustGrade) -> RuntimeCapabilities {
        RuntimeCapabilities {
            trust_grade,
            supported: RuntimeCapability::ALL.iter().copied().collect(),
            account_env: true,
            limits: RuntimeLimits {
                max_message_bytes: 64,
                max_history_page: 10,
                max_concurrent_sessions: 2,
            },
        }
    }

    #[test]
    fn trust_rank_is_policy_not_declaration_order() {
        assert!(TrustGrade::A.at_least(TrustGrade::C));
        assert!(!TrustGrade::C.at_least(TrustGrade::B));
        assert!(!TrustGrade::C.may_dispatch_autonomously());
        assert!(TrustGrade::B.may_dispatch_autonomously());
    }

    #[test]
    fn unsupported_capability_is_refused_before_trust_is_considered() {
        let mut declared = capabilities(TrustGrade::A);
        declared.supported.remove(&RuntimeCapability::Launch);
        let error = preflight(&declared, &OperationContext::new(RuntimeCapability::Launch))
            .expect_err("an undeclared capability must be refused");
        assert_eq!(
            error,
            RuntimeError::UnsupportedCapability {
                capability: RuntimeCapability::Launch
            }
        );
    }

    #[test]
    fn advisory_grade_may_read_but_not_drive() {
        let declared = capabilities(TrustGrade::C);
        preflight(
            &declared,
            &OperationContext::new(RuntimeCapability::Inspect),
        )
        .expect("observation stays available at every grade");
        preflight(&declared, &OperationContext::new(RuntimeCapability::Launch))
            .expect_err("an advisory runtime may not be driven");
    }

    #[test]
    fn limits_refuse_an_oversized_request() {
        let declared = capabilities(TrustGrade::A);
        let mut context = OperationContext::new(RuntimeCapability::SendMessage);
        context.demand = Some(LimitDemand::MessageBytes(65));
        assert_eq!(
            preflight(&declared, &context).expect_err("65 bytes exceeds 64"),
            RuntimeError::LimitExceeded {
                subject: "message body",
                limit: 64
            }
        );
    }
}
