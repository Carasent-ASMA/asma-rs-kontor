//! `kontor-context` — Context Pack resolution, canonical hashing and portable handoffs
//!
//! Kontor preserves three layers of context and only one of them is portable:
//!
//! 1. provider-native context, addressed by runtime and native session id;
//! 2. raw captured history, stored as immutable runtime events;
//! 3. the **Context Pack** and the **portable handoff** — this crate.
//!
//! A Context Pack is resolved once from a fixed, closed order of sources
//! (`global_profile` → `project_profile` → `scope` → `team_role_profile` →
//! `task_additions` → `run_override`), canonicalized and hashed by
//! [`kontor_core`], and frozen against the run that started with it. Preview and
//! start run the same pipeline, so what an operator reviewed is byte-for-byte
//! what the run receives.
//!
//! Three properties hold by construction:
//!
//! * **The same inputs give the same digest.** Layer ranks are fixed, sources are
//!   ordered by `(rank, source key)`, and the digest is the core canonical
//!   document digest — never a locally rolled hash.
//! * **A started pack is immutable.** [`ContextPackSnapshot`] owns its canonical
//!   bytes, provenance and redaction report and keeps no loader or source handle.
//! * **Redacted values never appear.** Redaction removes the whole subtree and
//!   its provenance *before* canonicalization; the report keeps only path, source
//!   and reason code, and [`kontor_core::id::reject_sensitive_material`] is the
//!   fail-closed backstop behind it.
//!
//! Nothing here persists, schedules, launches or resumes anything: those are the
//! store, scheduler and runtime tickets. Errors are [`kontor_core::DomainError`]
//! values that name a subject, a rule and — where useful — a structural path, and
//! never the value that was rejected.

pub mod handoff;
pub mod model;
pub mod resolve;

pub use kontor_core::{DomainError, DomainResult};

pub use handoff::{
    ContinuationMode, HandoffAcknowledgement, HandoffCapsule, SameEngineContinuation, TestAttempt,
    TestResult, acknowledge,
};
pub use model::{
    ContextLayer, ContextPackSnapshot, ContextSource, ProvenanceEntry, RedactionReason,
    RedactionRecord, RedactionRule, ReferenceInputs, ResolvedContextPack, ResolvedReference,
    RestrictedReference, RunBinding, WorkspaceRef,
};
pub use resolve::{ResolutionRequest, preview, start_run};
