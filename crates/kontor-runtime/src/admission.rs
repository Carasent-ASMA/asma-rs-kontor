//! Launch admission: the runtime decides who may fill a seat, and says so once.
//!
//! ## Why this lives in the runtime
//!
//! A seat in a team run — `(team_run_id, role_slot_id)` — may hold **at most one
//! non-terminal native binding, or one outstanding launch reservation, and never
//! both**. That is AC-4, and it is a statement about native sessions, so the only
//! party that can enforce it is the party that owns them.
//!
//! Every earlier attempt to enforce it in the caller failed for the same reason:
//! a caller-side token is a value, and a value can be replayed, rebuilt or
//! written by a second caller that cannot see the first. Rust offers no
//! friend-crate visibility and Cargo unifies features per build, so no
//! arrangement of types in `kontor-teams` can make itself the exclusive minter of
//! a launchable request.
//!
//! Admission sidesteps the whole problem instead of trying to win it. The
//! runtime keeps a table keyed by [`RoleSlotKey`], and:
//!
//! * [`crate::adapter::RuntimeAdapter::admit_launch`] checks and claims that key
//!   in one atomic step, and is the **only** producer of a [`LaunchAuthority`];
//! * [`LaunchAuthority`] has no public constructor, no `Clone`, no `Deserialize`
//!   and no feature-gated back door, and is consumed by
//!   [`LaunchAuthority::into_request`] — so a [`crate::request::LaunchRequest`]
//!   cannot exist without a runtime having admitted it;
//! * [`crate::adapter::RuntimeAdapter::launch`] re-reads the table and consumes
//!   the reservation before its first native effect, so an authority that is
//!   spent, superseded or aimed at another seat buys nothing.
//!
//! Fabricating a fresh [`kontor_core::id::AgentRunId`] and
//! [`kontor_core::id::RuntimeBindingId`] does not help, because the key admission
//! is decided on contains neither.

use kontor_core::id::{AgentRunId, RoleSlotId, RuntimeBindingId, TeamRunId, Timestamp};
use uuid::Uuid;

use crate::capability::RuntimeBindingSnapshot;
use crate::request::{LaunchParts, LaunchRequest};

/// The address launch admission is decided on: one seat of one team run.
///
/// Not the run and not the binding. Both of those are minted fresh for every
/// attempt, so a rule keyed on them is a rule a caller can step around by
/// generating new ones. The seat is the thing a second session would actually
/// contend for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoleSlotKey {
    /// The team run the seat belongs to.
    pub team_run_id: TeamRunId,
    /// The seat inside that team run.
    pub role_slot_id: RoleSlotId,
}

impl RoleSlotKey {
    /// Address one seat.
    #[must_use]
    pub const fn new(team_run_id: TeamRunId, role_slot_id: RoleSlotId) -> Self {
        Self {
            team_run_id,
            role_slot_id,
        }
    }
}

impl std::fmt::Display for RoleSlotKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.team_run_id, self.role_slot_id)
    }
}

/// Kontor's durable record that a seat's previous holder is finished.
///
/// A replacement is not "the old one looks done"; it is a citation of the exact
/// binding being replaced, the run that held it, and the successor Kontor has
/// already linked to it. The runtime checks all three against what it owns, so a
/// citation that names the wrong binding, the wrong predecessor or a successor
/// other than the run now asking cannot admit anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedBinding {
    /// The exact binding being replaced.
    pub binding_id: RuntimeBindingId,
    /// The run that held it.
    pub agent_run_id: AgentRunId,
    /// The successor run Kontor has durably recorded against it.
    pub successor_agent_run_id: AgentRunId,
}

/// Ask a runtime to admit one launch into one seat.
///
/// Building one is deliberately harmless: it is a question, not an answer. The
/// answer is a [`LaunchAuthority`], and only the runtime can produce one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionRequest {
    /// The seat being filled.
    pub slot: RoleSlotKey,
    /// The run that would own the session.
    pub agent_run_id: AgentRunId,
    /// The binding id Kontor has minted for the session to come.
    pub binding_id: RuntimeBindingId,
    /// The finished predecessor this launch replaces, when it replaces one.
    /// `None` asks for a seat that must be genuinely vacant.
    pub replaces: Option<ReplacedBinding>,
    /// When admission was requested.
    pub requested_at: Timestamp,
}

/// The runtime's own name for one reservation it is holding.
///
/// Opaque on purpose. Its only meaning is "the runtime that issued this is still
/// holding it", which is a fact about that runtime's table — so a ticket a
/// caller assembles, copies from a spent authority or carries to a different
/// runtime resolves to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdmissionTicket(Uuid);

impl AdmissionTicket {
    /// Mint a fresh, time-ordered ticket.
    ///
    /// Crate-private, alongside the adapters that own admission tables — the
    /// same rule [`crate::capability::IssuedBinding`] already follows, and for
    /// the same reason: a value only the runtime can produce is worth checking,
    /// and one anybody can produce is not.
    pub(crate) fn mint() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for AdmissionTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0.as_hyphenated(), f)
    }
}

/// Runtime-issued authority to start exactly one native session in one seat.
///
/// The only producer is [`crate::adapter::RuntimeAdapter::admit_launch`]. There
/// is no public constructor, no `Clone`, no `Deserialize` and no feature that
/// unlocks one — and, unlike every caller-side token this replaced, forging the
/// struct would not help anyway: [`crate::adapter::RuntimeAdapter::launch`]
/// resolves the ticket against the issuing runtime's own reservation table, and
/// a ticket that is not the one outstanding for that seat admits nothing.
///
/// It is consumed by [`LaunchAuthority::into_request`], so one admission yields
/// one request.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchAuthority {
    ticket: AdmissionTicket,
    slot: RoleSlotKey,
    agent_run_id: AgentRunId,
    binding_id: RuntimeBindingId,
}

impl LaunchAuthority {
    /// Issue authority for a reservation the runtime has just claimed.
    pub(crate) const fn issue(
        ticket: AdmissionTicket,
        slot: RoleSlotKey,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
    ) -> Self {
        Self {
            ticket,
            slot,
            agent_run_id,
            binding_id,
        }
    }

    /// The reservation this authority names.
    pub(crate) const fn ticket(&self) -> AdmissionTicket {
        self.ticket
    }

    /// The seat this authority fills.
    #[must_use]
    pub const fn slot(&self) -> &RoleSlotKey {
        &self.slot
    }

    /// The run this authority admits.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.agent_run_id
    }

    /// The binding the admitted session will be recorded under.
    #[must_use]
    pub const fn binding_id(&self) -> RuntimeBindingId {
        self.binding_id
    }

    /// Spend this authority on the one launch it admits.
    ///
    /// Taking `self` is the point: an authority that could be asked for a
    /// request twice would be an authority for two launches.
    ///
    /// It deliberately does *not* check `parts` against itself. A launch whose
    /// parts name another seat, run or binding is refused by the runtime, which
    /// compares both against the reservation it is actually holding — checking
    /// here as well would move the decision to a place that cannot see the
    /// table, and would let an adapter that skipped the table look correct.
    #[must_use]
    pub fn into_request(self, parts: LaunchParts) -> LaunchRequest {
        LaunchRequest::admitted(self, parts)
    }
}

/// What the runtime decided about one admission request.
#[derive(Debug)]
pub enum AdmissionOutcome {
    /// The seat was free — or the cited predecessor was genuinely finished — and
    /// the runtime is now holding a reservation for this caller.
    Admitted(LaunchAuthority),
    /// Compatible work: this seat already holds a live session for this very
    /// run. There is nothing to launch, and the existing binding is the thing to
    /// continue with.
    Resumed(Box<RuntimeBindingSnapshot>),
}

impl AdmissionOutcome {
    /// The authority, when the runtime admitted a fresh launch.
    ///
    /// # Errors
    /// Returns [`crate::adapter::RuntimeError::SlotAlreadyAdmitted`] when the
    /// seat answered with a live session instead, which is a resume and not a
    /// launch.
    pub fn into_authority(self) -> crate::adapter::RuntimeResult<LaunchAuthority> {
        match self {
            Self::Admitted(authority) => Ok(authority),
            Self::Resumed(_) => Err(crate::adapter::RuntimeError::SlotAlreadyAdmitted {
                rule: "this seat already holds a live session for this run; resume it",
            }),
        }
    }

    /// The live binding, when the seat answered with compatible work.
    #[must_use]
    pub const fn resumed(&self) -> Option<&RuntimeBindingSnapshot> {
        match self {
            Self::Admitted(_) => None,
            Self::Resumed(binding) => Some(binding),
        }
    }
}
