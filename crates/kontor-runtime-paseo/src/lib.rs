//! `kontor-runtime-paseo` — the Paseo execution plane, behind
//! [`kontor_runtime::RuntimeAdapter`].
//!
//! Kontor stays the authority for intent, admission, gates, receipts and
//! verdicts. Paseo stays the authority for native session identity, transcript
//! and live runtime state. This crate is the seam, and it is one adapter with
//! one test seam rather than a second orchestration layer.
//!
//! ```text
//! Kontor mini-project / Jira epic  ->  one Paseo project
//!   task worktree                  ->  one workspace in that project
//!     (team_run, role_slot)        ->  one persistent agent in that workspace
//! ```
//!
//! Everything else follows from those three lines: an idle agent is that seat
//! waiting rather than a finished run, a drifted display name is reported and
//! not repaired, and every id is read back from the daemon protocol before it is
//! believed — because the CLI's JSON omits exactly the fields the placement
//! rules are about.
//!
//! # Layout
//!
//! * [`wire`] — the Paseo 0.2.5 CLI and protocol model, plus the one place a
//!   native timeline entry becomes a `SessionEvent`. Pure.
//! * [`client`] — the transport seam, the argv builder, the request correlation,
//!   and the [`SecretString`](secrecy::SecretString) that never leaves it.
//! * [`fixture`] — a recorded daemon with a call ledger, so a claim about the
//!   wire is a count rather than an inference.
//! * [`adapter`] — hierarchy, admission, continuity and session content.

pub mod adapter;
pub mod client;
pub mod fixture;
pub mod wire;

pub use adapter::{
    PaseoAdapter, PaseoAdoptionIntent, PaseoCheckpoint, PaseoCompaction, PaseoConfig,
    PaseoDelivery, PaseoExecutionScope, PaseoProjectBinding, PaseoProjectOutcome, PaseoSeatRecord,
    PaseoSlotPlan,
};
pub use client::{PaseoCommand, PaseoLiveTransport, PaseoRpc, PaseoTransport};
pub use fixture::RecordedPaseo;
pub use wire::{PASEO_VERSION, PaseoFeature, PaseoProjection};
