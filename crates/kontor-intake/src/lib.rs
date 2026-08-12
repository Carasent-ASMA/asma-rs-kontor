//! `kontor-intake` — canonical source events, deterministic matching and the
//! decisions that follow from them.
//!
//! This crate turns *whatever arrived* into one shape and then decides about
//! that shape. It holds no connection, opens no socket, reads no row and writes
//! none: everything here is a pure function of its arguments, which is what
//! makes an intake decision re-checkable years later against the same envelope
//! and the same pinned trigger revision.
//!
//! ## One envelope, and no source-kind vocabulary anywhere behind it
//!
//! An adapter authenticates a connection and normalizes what that connection
//! said into [`InboundEvent`]. A manual request, a pull request, a CI result, a
//! monitoring alert and a bug report all become the same
//! [`CanonicalSourceEvent`]: a redacted, canonically serialized envelope with a
//! declared event schema and a bounded set of attributes. Source-specific
//! parsing *stops at the adapter*.
//!
//! Everything after that is data-driven. [`match_triggers`] compares keys and
//! evaluates JSON pointers; it has no `match` on a source kind, and neither has
//! anything downstream — `kontor-scheduler`'s `tests/no_seed_branching.rs`
//! asserts that against its own source, and `tests/no_source_branching.rs`
//! asserts the same about this crate: an adapter may name its own vocabulary,
//! the matcher may not.
//!
//! ## What a decision is, and what it is not
//!
//! [`evaluate`] produces exactly one [`IntakeReceipt`] per event: `proposed`
//! when a trigger matched, `ignored` when none did. It never produces
//! `approved`. Approving is a separate, receipt-backed act
//! ([`kontor_core::repository::IntakeAuthority`]), and bounded auto-arming is
//! the trigger's own policy exercising a capability — checked here by
//! [`authorize_auto_arm`] and checked *again* inside the transaction that
//! creates the work, through the same rule in `kontor-core`.
//!
//! Nothing in this crate creates work, and nothing in it launches a runtime.

#![forbid(unsafe_code)]

pub mod adapter;
pub mod decide;
pub mod matching;

pub use adapter::{InboundEvent, canonicalize};
pub use decide::{Intake, MatchedTrigger, WorkPins, authorize_auto_arm, evaluate, idempotency_key};
pub use matching::{ENVELOPE_SCHEMA_VERSION, match_triggers};
