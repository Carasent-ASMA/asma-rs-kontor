//! `kontor-scheduler` — Ready-batch planning, durable-lease inputs, budgets and explanations
//!
//! This crate decides *which ready work starts next, and why not the rest*.
//! `kontor-store` makes one of those decisions durable in a single transaction;
//! nothing here writes a row, calls a runtime, or cancels anything that is
//! already running.
//!
//! ## The five properties the whole crate rests on
//!
//! **Deterministic.** [`plan`] reads nothing but its argument. The instant every
//! window is judged against is [`SchedulingSnapshot::taken_at`]; the capacity in
//! force, the leases held, the calendar answer and the runtime and account
//! evidence all arrive with it. Identical snapshots give byte-identical decisions,
//! which is what makes a persisted admission re-checkable rather than merely
//! re-readable.
//!
//! **Ordered by a total order.** Priority descending, then creation instant, then
//! task id. The last key is a UUIDv7, so two tasks created in the same
//! millisecond at the same priority still have exactly one order — on every
//! machine and after every restart.
//!
//! **Refusals are first-class.** Every candidate gets a decision, and a refusal
//! carries a closed [`RejectionCode`] plus the evidence behind it. The blockers
//! are evaluated in one fixed order ([`ready::BLOCKER_ORDER`]), so the reported
//! reason for a candidate is a property of the snapshot rather than of which
//! check happened to run first.
//!
//! **Every ceiling is configured.** [`CapacityConfig`] carries the numbers and
//! this crate carries none. There is no compiled concurrency, no default
//! fan-out, and no branch anywhere on a work-profile id, a seed profile id or a
//! source kind — `tests/no_seed_branching.rs` asserts that against the source.
//!
//! **Uncertainty is not capacity, and never an outcome.** A runtime that has not
//! been reconciled, an account whose preflight is absent, a lease whose holder
//! lost contact: each one refuses *new* work and says nothing at all about the
//! work already running. Nothing in this crate can conclude that a run finished.
//!
//! ## What the scheduler does not own
//!
//! * **Calendar resolution** (KON-MVP-21). [`CalendarAdmission`] is a resolved
//!   answer the scheduler consumes and persists. This crate parses no ICS, no
//!   holiday feed, no time zone and no weekly window.
//! * **Intake** (KON-MVP-22). Event-origin work is admitted through the identity
//!   and status of its durable intake receipt ([`IntakeLineage`]) and nothing
//!   else. This crate never reads a source envelope, normalizes an event or
//!   re-matches a trigger filter.
//! * **Guardrails** (KON-MVP-10). The module-isolation rule is
//!   [`kontor_policy::module_isolated_by_worktree`], called from here rather than
//!   restated, so a scheduler's answer and a guardrail's answer cannot drift.
//! * **Team seats** (KON-MVP-08). Admission starts one task's top-level envelope.
//!   Filling further seats of that team is `kontor-teams`' slot API.
//! * **Exclusion between processes.** Two scheduler instances are kept apart by
//!   the durable leases the admission transaction acquires, never by a value in
//!   this crate.

pub mod completion;
pub mod model;
pub mod ready;

pub use completion::{
    CommitteeVerdict, CompiledCompletion, CompletionBlocker, CompletionCommand, CompletionEdge,
    CompletionEdgeCondition, CompletionNode, CompletionNodeKey, CompletionNodeKind,
    CompletionObservation, CompletionPhase, CompletionProfile, CompletionProfileRef,
    CompletionRound, CompletionSignal, CompletionState, CompletionTransition, IntegrationRecord,
    PollingFallback, RemediationApproval, RemediationAuthorization, RemediationRecord,
    RepositoryOutcome, SignalDelivery, advance, blockers, compile, operational_default,
    outstanding, start,
};
pub use model::{
    AccountAdmissionEvidence, AccountCapabilityKey, AccountPin, AdaptiveWindow,
    AdaptiveWindowConfig, AdmissionEventId, AdmittedCandidate, AuthorizationEvidence,
    CalendarAdmission, CalendarPolicyEvidence, Candidate, CandidateDecision, CapacityConfig,
    CapacityLimitKind, CapacityObservation, CapacitySnapshot, CapacityUsage, ExternalOwnership,
    ExternalWorkEvidence, FleetPreflight, IntakeLineage, MAX_PRIORITY, OrderingInputs, Plan,
    PreflightOutcome, ReconciliationEvidence, ReconciliationScope, RejectionCode,
    RejectionEvidence, RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin,
    WorktreeClaim, WorktreeVerification,
};
pub use ready::{BLOCKER_ORDER, Blocker, Refused, explain, minimum_launch_capabilities, plan};
