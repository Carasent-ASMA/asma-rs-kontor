//! `kontor-teams` — Versioned team templates and run snapshots
//!
//! This crate turns the generic KON-MVP-03 team envelope
//! ([`kontor_core::spec::TeamTemplateRevision`],
//! [`kontor_core::spec::TeamRunSnapshot`]) into a typed document with concrete
//! role slots, and turns those slots into the one supported path for running a
//! team against a real runtime.
//!
//! Two things it deliberately is not: it is not a second persistence layer —
//! `kontor-store` still owns `(id, version)` immutability, canonical bytes and
//! compare-and-swap — and it is not a second outcome policy —
//! [`kontor_core::state::reduce_team_outcome`] stays authoritative.
//!
//! ## The invariant this crate exists to hold
//!
//! **At most one non-terminal native session per `(TeamRunId, RoleSlotId)`.**
//!
//! It holds by composition of types that already exist:
//!
//! ```text
//! (TeamRunId, RoleSlotId)
//!   -> exactly one non-terminal AgentRunId leaf   (this crate)
//!   -> at most one RuntimeBinding                 (kontor-store)
//!   -> one RuntimeBindingSnapshot while occupied  (kontor-runtime)
//! ```
//!
//! The **authoritative** rule is the last one in this list, and it is the only
//! one that holds against a caller who ignores this crate entirely. It is keyed
//! on the seat, which is the thing a second session would contend for — not on
//! the run or the binding, which a caller can mint fresh at will:
//!
//! * [`kontor_runtime::adapter::RuntimeAdapter::admit_launch`] — checks and
//!   claims `(team run, role slot)` in one atomic step, and is the only producer
//!   of a [`kontor_runtime::admission::LaunchAuthority`];
//! * [`kontor_runtime::adapter::RuntimeAdapter::launch`] — consumes that
//!   reservation before its first native effect, so a replayed request, a
//!   concurrent caller, a freshly minted run and binding, or a restart race all
//!   end in a typed refusal with nothing started.
//!
//! A [`kontor_runtime::request::LaunchRequest`] has no other origin: no struct
//! literal, no `Clone`, no `Deserialize`, no feature that unlocks one.
//!
//! The rest are **Kontor's own bookkeeping** — they keep this crate's records
//! coherent, and they are not what stops a native session from existing twice:
//!
//! * [`run::TeamRunLease`] — one team run has one live manager, so two rosters
//!   cannot each believe a seat is free;
//! * [`run::LaunchPermit`] — issued only for a vacant seat in *this* roster, and
//!   spent by the single request it helps assemble;
//! * [`run::TeamRunSlots::bind`] — consumes that spent permit, so the roster
//!   records exactly one session per seat.
//!
//! An earlier design tried to make the permit itself the proof, and could not:
//! Rust has no friend-crate visibility, so an entry point this crate can call is
//! callable by anyone who can reach it, and Cargo unifies features per build.
//! Runtime-owned admission replaced it precisely because a caller-side token
//! cannot be made unforgeable.
//!
//! Closing a seat is symmetric: the run presented must carry the very session
//! the seat is retiring, so a foreign or absent binding cannot quietly retire a
//! seat whose session is still alive.
//!
//! The raw ports remain low-level. Calling
//! [`kontor_core::repository::RunRepository::create_agent_run`] directly for a
//! slot this crate already owns is outside the supported team path and this
//! crate cannot defend against it; scheduling code consumes the slot API
//! instead.
//!
//! Every rule here is structural. No function reads a slot id, a role name, a
//! gate name or a template name.

pub mod operational;
pub mod run;
pub mod spec;

pub use operational::{
    CoreTeamRevision, CoreTeamSeat, CoreTeamSeatSelection, EpicPresence, OperationalEffects,
    OperationalKinds, OperationalWorkflow, PinnedConfiguration, ProjectSessionBaseBinding,
    PromotionNode, PromotionOutcome, PromotionPlan, PromotionPreview, PromotionSeat,
    PromotionTarget, QuickSession, QuickSessionRequest, QuickSourceEvidence, RosterUpgradeOutcome,
    RosterUpgradePlan, RosterUpgradePreview, SourceDisposition,
};

pub use run::{
    ClosedSlot, LaunchPermit, OccupiedSlot, PreparedLaunch, ReplacementPending, RoleSlotWaiver,
    SlotLaunch, TeamClosureCertificate, TeamRunLease, TeamRunSlots,
};
pub use spec::{
    MAX_HANDOFF_DEPTH, MAX_ROLE_SLOTS, MAX_SUCCESSOR_DEPTH, RoleHandoff, RoleRequirement,
    RoleSlotId, RoleSlotSpec, RoleSlotWaiverPolicy, TeamPackSpec, TeamTemplateSpec, bundled_teams,
    parse_team_pack, revise_team_template,
};
