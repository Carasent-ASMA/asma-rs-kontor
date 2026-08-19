//! `kontor-policy` — Guardrail evaluation and evidence bundles
//!
//! Seven architecture rules, evaluated as pure functions of their inputs, plus
//! the bounded state machine that recovers work they parked. This crate decides;
//! `kontor-store` records what it decided and applies the consequences in one
//! transaction.
//!
//! ## The rules
//!
//! | Rule | Refuses |
//! |---|---|
//! | `worktree_sticky` | acting in a tree that is not the one the run was pinned to, or in an ambiguous one |
//! | `module_collision` | two tasks holding one module without worktree isolation |
//! | `second_rejection_parks` | a second rejection by the same reviewer on the same gate |
//! | `degraded_verdict_denied` | a gate verdict from degraded evidence, an unauthorized role, or a simulated persona |
//! | `destructive_requires_approval` | a destructive action without an approval bound to that exact action |
//! | `account_pin_required` | a run acting as an account it was not pinned to |
//! | `terminal_evidence_required` | completing a phase or closing a run without the evidence it declared |
//!
//! ## Three properties the whole crate rests on
//!
//! **Deterministic.** [`evaluator::decide`] reads nothing but its argument. The
//! instant an expiry is judged against is an input; so is the gate history a
//! counter is derived from. Identical canonical inputs give an identical
//! verdict, reason and evidence set, which is what makes a stored evaluation
//! re-checkable rather than merely re-readable.
//!
//! **Name-free.** No function branches on a profile id, phase, gate, role or
//! persona name. Rules read the *shape* of the pinned snapshot — the artifacts a
//! phase declares, the roles a gate authorizes, whether a waiver is allowed at
//! all — so a deployment's own profile is governed exactly like a bundled one.
//!
//! **Append-only.** Every evaluation is a new immutable value, and every
//! recovery step is appended. Nothing here has an update path, and the store's
//! triggers make that true against direct SQL as well.
//!
//! ## What this crate deliberately does not do
//!
//! It does not own gate authority. The pinned profile and
//! `WorkflowRepository::append_gate_evaluation` decide who may pass, reject or
//! waive a gate, and no part of that moved here — the guardrail layer sits in
//! front of it so a refused action is never attempted, not behind it as a second
//! opinion. It also grants nothing: an advisor and a committee append
//! recommendations, and [`model::AuthoritySource::RecoveryAdvice`] is refused
//! wherever an approval is required.

pub mod completion;
pub mod evaluator;
pub mod model;
pub mod recovery;

pub use completion::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, NeedsHumanPayload, TicketEvidence,
    TicketGateBlocker, TicketRequirement, closeout_blockers, ticket_gate_blockers,
};
pub use evaluator::{
    REJECTIONS_BEFORE_PARK, decide, evaluate, module_isolated_by_worktree, rejections_since_pass,
};
pub use model::{
    ActionDomain, ActionEffect, ActionIntent, ActorContext, ApprovalReceipt, ApprovalReceiptId,
    ApprovalScopeKind, ArtifactEvidence, ArtifactEvidenceId, AuthoritySource, Decision,
    EscalationCause, EvaluationRequest, EvaluationSubject, EvidenceRef, GateWaiverId,
    GuardrailEvaluation, GuardrailRule, GuardrailRuleKey, ModuleClaim, PersonaActor, PolicyVerdict,
    ReasonCode, RecoveryEpisode, RecoveryEpisodeId, RecoveryStatus, RecoveryStepKind,
    RequestedAction, RunContext, RuntimeObservationRef, SubjectKind, VerdictRung,
    WorkspaceEvidence,
};
pub use recovery::{
    MAX_ADVISOR_CONSULTATIONS, MAX_COMMITTEE_CONSULTATIONS, MAX_EFFECTIVE_FOLLOWUPS,
    RecoveryAction, RecoveryRequest, RecoveryTransition, plan,
};
