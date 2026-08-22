//! `kontor-accounts` — Non-secret account profiles, credential references and routing
//!
//! A run is pinned to exactly one [`kontor_core::id::AccountProfileId`], and
//! that pin is the only
//! thing that decides which coding account the run executes as. This crate owns
//! what that pin means: the durable non-secret profile, the approved reference
//! it resolves through, the admission decision that lets a launch use it, and
//! the explicit successor run that a rotation produces.
//!
//! # The one invariant
//!
//! **A resolved secret is never persisted, serialized, logged, exported, put in
//! process arguments, or returned through a projection.** Three separate things
//! enforce it, and none of them is "remember not to":
//!
//! 1. *Nothing resolvable is stored.* [`kontor_core::repository::AccountProfile`]
//!    holds a closed reference kind plus an opaque alias. The alias means
//!    nothing without a [`ResolverPolicy`], and the policy is built in memory at
//!    composition time from trusted local operator configuration — it is never
//!    written to SQLite and never exported. There is therefore no persisted
//!    value that could leak, only a name for one.
//!
//! 2. *Resolved material has no serialized form.* [`ResolvedAccountEnvironment`]
//!    has private fields, no `Serialize`, and redacted `Debug`/`Display`. The
//!    only way a value leaves it is
//!    [`ResolvedAccountEnvironment::apply`], which writes it into a child
//!    process environment through [`std::process::Command::env`] — never into
//!    `std::env::set_var`, a shell fragment, a command flag, a prompt or argv.
//!
//! 3. *Errors carry reason codes, not values.* A resolver, keychain or
//!    filesystem failure is mapped to a closed reason before it is returned, so
//!    no source error whose `Display` might contain a path, a keychain target or
//!    a token is ever wrapped.
//!
//! # What this crate is not
//!
//! It contains no runtime adapter, API or export engine. The runtime contract
//! stays authoritative for capability enforcement — an account-pinned launch is
//! refused by [`kontor_runtime::capability::preflight`] itself, not by a second
//! copy of that rule here.
//!
//! # What this crate now owns
//!
//! Cooldown and admission mechanics. They used to belong to `asma fleet`, which
//! meant a Realm could only know whether an account was usable by shelling out
//! to another tool — so the answer was as available as that tool was, and
//! Kontor could not run without it. The observation is now collected here,
//! persisted as raw evidence before anything is derived from it, and folded
//! into the adaptive admission window by [`fold`]. The scheduler still owns the
//! arithmetic; this crate owns which evidence moves it.

mod admission;
mod capacity;
mod launch;
mod profile;
mod quota;
mod resolver;
mod usage;

pub use admission::{AdaptivePosition, fold};
pub use capacity::{
    COOLDOWN_SECONDS, CapacityReading, DerivedAvailability, ProbeOutcome, ProbeRefusal,
    cools_until, derive,
};
pub use launch::{
    AccountAvailability, AccountLaunchReceipt, AdmittedLaunch, AvailabilityObservation,
    FailoverOutcome, FailoverReason, FailoverRefusal, FailoverRequest, LaunchAdmissionRequest,
    LaunchRefusal, MAX_OBSERVATION_AGE_SECONDS, admit_pinned_launch, fail_over_to_new_run,
};
pub use profile::{
    AccountEnvironmentMap, AccountError, AccountProfileDraft, AccountService, ENVIRONMENT_SCHEMA,
};
pub use quota::{ObservedQuota, QuotaBasis, QuotaSignal, classify};
pub use usage::{
    UsageFailure, UsageReading, UsageWindow, observe, read_chatgpt_usage,
};
pub use resolver::{
    AccountResolver, KeychainBackend, KeychainFailure, KeychainTarget, PolicyError,
    ResolutionError, ResolutionReason, ResolvedAccountEnvironment, ResolverPolicy,
    ResolverPolicyBuilder, SystemKeychain,
};
