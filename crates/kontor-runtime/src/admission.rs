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
//! * [`crate::adapter::RuntimeAdapter::launch`] *claims* the reservation before
//!   its first native effect — checking it and taking it are one step — so an
//!   authority that is spent, superseded, aimed at another seat, or being spent
//!   right now by another caller buys nothing. The same step decides the
//!   run-keyed half of AC-4, because a run committed to one seat is a fact about
//!   the whole table rather than about the seat being asked for.
//!
//! Fabricating a fresh [`kontor_core::id::AgentRunId`] and
//! [`kontor_core::id::RuntimeBindingId`] does not help, because the key admission
//! is decided on contains neither.
//!
//! ## Where the table lives
//!
//! [`AdmissionLedger`] is that table, and it is the only minter of an
//! [`AdmissionTicket`] and a [`LaunchAuthority`] — both constructors are
//! crate-private, so an adapter in another crate can *own* a ledger without
//! being able to mint the authority it hands out. Every adapter, in-crate fake
//! or out-of-crate real one, holds one and gets the same policy from it rather
//! than restating it.
//!
//! The ledger owns *who may fill a seat*. It does not own *what a runtime's
//! sessions are doing*, and replacement needs both, so the two facts only a
//! runtime can answer arrive through [`SeatFacts`].

use std::collections::BTreeMap;

use kontor_core::id::{AgentRunId, ExternalId, RoleSlotId, RuntimeBindingId, TeamRunId, Timestamp};
use uuid::Uuid;

use crate::adapter::{RuntimeError, RuntimeResult};
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
        Self(kontor_core::id::generate_uuid_v7())
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

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// What one seat is currently holding.
///
/// These variants are the whole of AC-4: a seat has an unspent reservation, one
/// launch spending it, or a native binding — never two of anything, and never two
/// at once.
#[derive(Debug, Clone)]
enum SlotAdmission {
    /// Authority has been issued for this seat and not yet spent.
    Reserved {
        ticket: AdmissionTicket,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
    },
    /// One launch has taken the reservation and is in flight with it.
    ///
    /// The state between "admitted" and "a session exists", and the reason a
    /// check-and-take has to be one step: a launch parked in a native call has
    /// released the runtime's lock, and without this the seat would still read as
    /// reservable to whoever asked next.
    Launching {
        ticket: AdmissionTicket,
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
    },
    /// A native session was launched into this seat.
    Occupied {
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
        native_id: ExternalId,
    },
    /// A launch was in flight for this seat when the runtime's state was last
    /// persisted, and whether it created a session is unknown.
    ///
    /// Restored from [`ClaimedSeat`] and never entered any other way. It refuses
    /// everything, which is the only safe reading: the launch may have reached the
    /// runtime, and the alternative — restoring the seat as vacant — is the one
    /// that produces a second live session.
    Unresolved {
        agent_run_id: AgentRunId,
        binding_id: RuntimeBindingId,
    },
}

/// A launch that had taken a seat and had not yet finished taking it.
///
/// The persistable form of an in-flight claim, and deliberately not a
/// reservation: it carries no [`AdmissionTicket`], so nothing restored from one
/// can be spent as authority. It comes back as a seat that refuses every
/// question until the launch it names is resolved from evidence — see
/// [`AdmissionLedger::restore_claimed`] for where that evidence has to come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedSeat {
    /// The seat.
    pub slot: RoleSlotKey,
    /// The run whose launch was in flight.
    pub agent_run_id: AgentRunId,
    /// The binding that launch would have been recorded under.
    pub binding_id: RuntimeBindingId,
}

/// The native session a seat holds, for a runtime counting its own sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccupiedSeat {
    /// The seat.
    pub slot: RoleSlotKey,
    /// The run that holds it.
    pub agent_run_id: AgentRunId,
    /// The binding the session was recorded under.
    pub binding_id: RuntimeBindingId,
    /// The native session itself.
    pub native_id: ExternalId,
}

/// The two facts about a seat's current holder that only the runtime can supply.
///
/// The ledger decides who may fill a seat; a runtime knows what its sessions are
/// doing. Replacement needs both, so this is the seam — and the ledger never
/// guesses an answer it was not given, because a replacement granted on the
/// caller's own claim that the predecessor finished is a second live session.
pub trait SeatFacts {
    /// The runtime's own copy of a binding it issued, if it still holds one.
    ///
    /// Answering a duplicate launch with a resume hands this back, so it must be
    /// the runtime's copy and never one presented by a caller.
    fn issued_binding(&self, binding_id: RuntimeBindingId) -> Option<RuntimeBindingSnapshot>;

    /// Whether the seat's holder has released its claim on the seat.
    ///
    /// Two ways, and either is enough: the runtime has observed that native
    /// session reach a terminal state, or the binding belongs to an older
    /// generation and is therefore retired rather than finished. Both are
    /// statements about what this runtime owns, which is the point — Kontor's
    /// citation says *which* session is replaced, and this says whether it may
    /// be.
    fn holder_is_finished_or_retired(
        &self,
        binding_id: RuntimeBindingId,
        native_id: &ExternalId,
    ) -> bool;
}

/// One runtime's reservation table: both halves of AC-4, in one place.
///
/// One seat holds one launch, and one run owns one session. The second is a fact
/// about the whole table, so it is decided here too rather than by each adapter
/// counting its own sessions — an adapter that only knows about seats it has
/// bindings for cannot see a claim that has not become one yet.
///
/// Held by an adapter and read and written only under whatever lock that adapter
/// already holds over its own state. That is what makes "look at the seat, then
/// take it" a single step with no interleaving for a second caller to slip into;
/// the ledger cannot provide that itself, because the atomicity has to cover the
/// runtime's session table too.
///
/// It mints [`AdmissionTicket`] and [`LaunchAuthority`] through their
/// crate-private constructors, so an adapter crate can own a ledger and hand out
/// authority without being able to fabricate either.
#[derive(Debug, Default)]
pub struct AdmissionLedger {
    slots: BTreeMap<RoleSlotKey, SlotAdmission>,
}

impl AdmissionLedger {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide one admission request and, when it is granted, claim the seat.
    ///
    /// Every refusal returns before the table is touched, so a refused admission
    /// leaves the seat exactly as it was.
    ///
    /// # Errors
    /// * [`RuntimeError::SlotAlreadyAdmitted`] — the seat is holding another
    ///   launch's reservation, or a live session that is not this run's.
    /// * [`RuntimeError::ReplacementNotEvidenced`] — the citation does not match
    ///   what the seat holds, or the runtime has not seen that holder finish.
    /// * [`RuntimeError::StaleBinding`] — the seat's own binding is no longer
    ///   registered with the runtime, so there is nothing to resume.
    pub fn admit(
        &mut self,
        request: &AdmissionRequest,
        facts: &dyn SeatFacts,
    ) -> RuntimeResult<AdmissionOutcome> {
        match self.slots.get(&request.slot).cloned() {
            None => {
                // Nothing here to replace. Granting a citation that names a
                // session this seat never held would make the replacement rule
                // decorative.
                if request.replaces.is_some() {
                    return Err(RuntimeError::ReplacementNotEvidenced {
                        rule: "this seat holds no session to replace",
                    });
                }
            }
            Some(SlotAdmission::Reserved {
                ticket,
                agent_run_id,
                binding_id,
            }) => {
                // The same question asked twice is one reservation, not two: a
                // caller that lost the answer can ask again, and a caller
                // asking for anything else is refused while this one stands.
                if agent_run_id == request.agent_run_id && binding_id == request.binding_id {
                    return Ok(AdmissionOutcome::Admitted(LaunchAuthority::issue(
                        ticket,
                        request.slot.clone(),
                        agent_run_id,
                        binding_id,
                    )));
                }
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "another launch is already reserved for this seat",
                });
            }
            // A reservation that is being spent right now is not a reservation
            // anyone else can be handed, and re-issuing it to the caller already
            // spending it would hand back authority that cannot be claimed twice.
            Some(SlotAdmission::Launching { .. }) => {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "a launch into this seat is already in flight",
                });
            }
            // The seat may hold a session nobody recorded, and the citation route
            // is closed too: a replacement is evidenced against a *native* session,
            // and the native id of a launch that was in flight at the last
            // checkpoint is precisely what is unknown. So this refuses everything
            // — see `restore_claimed` for the boundary that resolves it.
            Some(SlotAdmission::Unresolved { .. }) => {
                return Err(RuntimeError::SlotAlreadyAdmitted {
                    rule: "a launch into this seat was in flight when the runtime last \
                           checkpointed and may have started a session",
                });
            }
            Some(SlotAdmission::Occupied {
                agent_run_id,
                binding_id,
                native_id,
            }) => match &request.replaces {
                None => {
                    // Compatible work: the seat already holds this very run's
                    // session, so there is nothing to launch. Handing back the
                    // runtime's own binding is what turns a duplicate launch
                    // attempt into a resume instead of a refusal.
                    if agent_run_id == request.agent_run_id && binding_id == request.binding_id {
                        let snapshot =
                            facts
                                .issued_binding(binding_id)
                                .ok_or(RuntimeError::StaleBinding {
                                    rule: "the seat's binding is no longer registered",
                                })?;
                        return Ok(AdmissionOutcome::Resumed(Box::new(snapshot)));
                    }
                    return Err(RuntimeError::SlotAlreadyAdmitted {
                        rule: "this seat already holds a live native session",
                    });
                }
                Some(cited) => Self::ensure_replaceable(
                    cited,
                    request.agent_run_id,
                    agent_run_id,
                    binding_id,
                    &native_id,
                    facts,
                )?,
            },
        }

        let ticket = AdmissionTicket::mint();
        self.slots.insert(
            request.slot.clone(),
            SlotAdmission::Reserved {
                ticket,
                agent_run_id: request.agent_run_id,
                binding_id: request.binding_id,
            },
        );
        Ok(AdmissionOutcome::Admitted(LaunchAuthority::issue(
            ticket,
            request.slot.clone(),
            request.agent_run_id,
            request.binding_id,
        )))
    }

    /// Agree, or refuse to agree, that a seat's holder is finished.
    ///
    /// Kontor's citation is checked against what the runtime owns, in both
    /// directions: the citation must name the session that is actually here, and
    /// the runtime must have seen that session finish. Either half alone would
    /// admit a replacement over a live seat.
    fn ensure_replaceable(
        cited: &ReplacedBinding,
        successor: AgentRunId,
        held_run: AgentRunId,
        held_binding: RuntimeBindingId,
        native_id: &ExternalId,
        facts: &dyn SeatFacts,
    ) -> RuntimeResult<()> {
        if cited.binding_id != held_binding {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "the cited binding is not the one this seat holds",
            });
        }
        if cited.agent_run_id != held_run {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "the cited predecessor is not the run this seat holds",
            });
        }
        if cited.successor_agent_run_id != successor {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "the recorded successor is not the run asking to be admitted",
            });
        }
        // Terminal *as the runtime observed it*, never as the caller reports it.
        if !facts.holder_is_finished_or_retired(held_binding, native_id) {
            return Err(RuntimeError::ReplacementNotEvidenced {
                rule: "the session this seat holds has not been observed finished",
            });
        }
        Ok(())
    }

    /// Take the reservation this seat is holding, for this one launch.
    ///
    /// Checking and taking are deliberately one call, because they have to be one
    /// step. A launch's first native effect happens with the runtime's lock
    /// released — it has to, the call is `async` — so a launch that merely *read*
    /// the reservation and went off to use it would leave the seat reading as
    /// reservable for as long as that call takes. Two callers presenting one
    /// authority would both pass, and each would create a session. Here the second
    /// one finds the seat already `Launching` and is refused before it dispatches
    /// anything.
    ///
    /// Looked up by the seat *the launch names*, not the one the authority claims:
    /// checking a value against itself proves nothing.
    ///
    /// Two rules are decided here, and the second is not implied by the first: a
    /// seat admits one launch, and a **run owns one session**. One run admitted
    /// into two *different* seats satisfies the seat rule twice over, and both
    /// launches are then racing to create that run a second agent — so the run is
    /// checked across every seat in the same step, for the same reason.
    ///
    /// # Errors
    /// * [`RuntimeError::LaunchNotAdmitted`] — the seat holds no reservation,
    ///   holds a different one, is already being launched into, or the launch
    ///   names a different run or binding than the reservation does.
    /// * [`RuntimeError::SessionAlreadyBound`] — another seat is already holding
    ///   this run's session, or a launch on its way to one.
    pub fn claim(&mut self, request: &LaunchRequest) -> RuntimeResult<()> {
        let slot = request.slot();
        match self.slots.get(&slot) {
            Some(SlotAdmission::Reserved {
                ticket,
                agent_run_id,
                binding_id,
            }) => {
                if *ticket != request.authority().ticket() {
                    return Err(RuntimeError::LaunchNotAdmitted {
                        rule: "this authority is not the reservation this seat is holding",
                    });
                }
                if *agent_run_id != request.agent_run_id() || *binding_id != request.binding_id() {
                    return Err(RuntimeError::LaunchNotAdmitted {
                        rule: "the launch names a different run or binding than the reservation",
                    });
                }
            }
            Some(SlotAdmission::Launching { .. }) => {
                return Err(RuntimeError::LaunchNotAdmitted {
                    rule: "another launch is already spending this seat's reservation",
                });
            }
            Some(SlotAdmission::Unresolved { .. }) => {
                return Err(RuntimeError::LaunchNotAdmitted {
                    rule: "a launch into this seat was in flight when the runtime last \
                           checkpointed and may have started a session",
                });
            }
            // A spent reservation and a seat that never had one are the same
            // answer: there is nothing here to launch with.
            Some(SlotAdmission::Occupied { .. }) | None => {
                return Err(RuntimeError::LaunchNotAdmitted {
                    rule: "this seat is holding no reservation to spend",
                });
            }
        }
        // The run-keyed half, before the seat is transitioned, because a launch
        // this run may not make must cost no native effect either.
        if self.run_is_committed_elsewhere(request.agent_run_id(), &slot) {
            // And the reservation it would have spent is abandoned rather than
            // left standing. This run can never spend it — that is what was just
            // decided — and nothing else removes one, so leaving it would hold the
            // seat against every future attempt: neither a live session nor a
            // spendable reservation, and no replacement possible either, because
            // there is no session to observe terminal. A seat that can never be
            // filled again fails AC-4 as squarely as one filled twice.
            //
            // Only the exact reservation validated above is removed, which is the
            // one this launch was issued from.
            self.slots.remove(&slot);
            return Err(RuntimeError::SessionAlreadyBound {
                rule: "recovery launches a successor run, never the same run twice",
            });
        }
        self.slots.insert(
            slot,
            SlotAdmission::Launching {
                ticket: request.authority().ticket(),
                agent_run_id: request.agent_run_id(),
                binding_id: request.binding_id(),
            },
        );
        Ok(())
    }

    /// Whether a seat other than `excluding` is already committed to this run.
    ///
    /// Committed means a native session, or a launch on its way to one — including
    /// a restored [`SlotAdmission::Unresolved`], whose whole meaning is that a
    /// session for that run may exist.
    ///
    /// A [`SlotAdmission::Reserved`] elsewhere is deliberately not a conflict. It
    /// is a question nobody has spent yet, and treating it as one would refuse
    /// *both* launches of a run that was admitted in two seats, leaving it unable
    /// to start anywhere. The first claim wins, and the reservation the loser is
    /// holding is what gets turned away when it tries to spend it.
    fn run_is_committed_elsewhere(
        &self,
        agent_run_id: AgentRunId,
        excluding: &RoleSlotKey,
    ) -> bool {
        self.slots.iter().any(|(slot, admission)| {
            slot != excluding
                && match admission {
                    SlotAdmission::Launching {
                        agent_run_id: held, ..
                    }
                    | SlotAdmission::Occupied {
                        agent_run_id: held, ..
                    }
                    | SlotAdmission::Unresolved {
                        agent_run_id: held, ..
                    } => *held == agent_run_id,
                    SlotAdmission::Reserved { .. } => false,
                }
        })
    }

    /// Turn this launch's claim into the session it created.
    ///
    /// Called in the same critical section that records the session, so there is
    /// no instant at which a session exists and its seat is still reservable.
    ///
    /// Only the claim *this* launch took is transitioned. That is not defensive
    /// bookkeeping: writing the seat unconditionally would let a launch that never
    /// held the claim — or held one that has since been released and re-reserved —
    /// overwrite whatever the seat legitimately holds.
    ///
    /// # Errors
    /// Returns [`RuntimeError::LaunchNotAdmitted`] when the seat is no longer
    /// holding this launch's claim, leaving the table untouched. The session it
    /// was called about is then a session no binding names, which is what
    /// reconciliation reports as an orphan — the safe direction, and the reason
    /// this refuses rather than writing the seat anyway.
    pub fn occupy(&mut self, request: &LaunchRequest, native_id: ExternalId) -> RuntimeResult<()> {
        let slot = request.slot();
        if !self.holds_claim(&slot, request) {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "this seat is no longer holding the claim this launch took",
            });
        }
        self.slots.insert(
            slot,
            SlotAdmission::Occupied {
                agent_run_id: request.agent_run_id(),
                binding_id: request.binding_id(),
                native_id,
            },
        );
        Ok(())
    }

    /// Whether this seat is holding the claim `request` took.
    ///
    /// The ticket is what distinguishes it: the seat, run and binding of a
    /// second attempt at the same seat all agree with the first.
    fn holds_claim(&self, slot: &RoleSlotKey, request: &LaunchRequest) -> bool {
        matches!(
            self.slots.get(slot),
            Some(SlotAdmission::Launching { ticket, .. })
                if *ticket == request.authority().ticket()
        )
    }

    /// Hand a seat back after a launch that claimed it and then failed.
    ///
    /// A claim has exactly two ends: it becomes a session, or it comes back here.
    /// Without the second, any refusal between the claim and the first native
    /// effect would wedge the seat forever — nothing else removes a `Launching`
    /// entry, so the seat would hold neither a binding nor a launchable
    /// reservation, which is precisely the state AC-4 forbids.
    ///
    /// Only the claim *this* launch took is released. An occupied seat is holding
    /// a session that has every right to it, a standing reservation was never this
    /// launch's to free, and a claim carrying another ticket belongs to another
    /// attempt that is still in flight.
    pub fn release(&mut self, request: &LaunchRequest) {
        let slot = request.slot();
        if self.holds_claim(&slot, request) {
            self.slots.remove(&slot);
        }
    }

    /// The native session a seat holds, if it holds one.
    #[must_use]
    pub fn occupant(&self, slot: &RoleSlotKey) -> Option<&ExternalId> {
        match self.slots.get(slot) {
            Some(SlotAdmission::Occupied { native_id, .. }) => Some(native_id),
            _ => None,
        }
    }

    /// Whether this table is holding an unspent reservation for one seat.
    #[must_use]
    pub fn is_reserved(&self, slot: &RoleSlotKey) -> bool {
        matches!(self.slots.get(slot), Some(SlotAdmission::Reserved { .. }))
    }

    /// Every seat that holds a native session.
    ///
    /// One half of the persistable table, for an adapter that survives a Kontor
    /// restart; [`Self::claimed_seats`] is the other. Reservations are in neither:
    /// one exists only between an admission and the launch that claims it, the
    /// [`LaunchAuthority`] it was issued with cannot be serialized either, and
    /// restoring a ticket would need a public way to mint one. A caller that loses
    /// a reservation asks again — a caller that loses a *claim* may already have
    /// started something, which is why that half is persisted.
    pub fn occupied_seats(&self) -> impl Iterator<Item = OccupiedSeat> + '_ {
        self.slots
            .iter()
            .filter_map(|(slot, admission)| match admission {
                SlotAdmission::Occupied {
                    agent_run_id,
                    binding_id,
                    native_id,
                } => Some(OccupiedSeat {
                    slot: slot.clone(),
                    agent_run_id: *agent_run_id,
                    binding_id: *binding_id,
                    native_id: native_id.clone(),
                }),
                SlotAdmission::Reserved { .. }
                | SlotAdmission::Launching { .. }
                | SlotAdmission::Unresolved { .. } => None,
            })
    }

    /// Every seat whose launch is in flight, or was when it was last persisted.
    ///
    /// A checkpoint can be taken while a launch sits between its claim and its
    /// session — that is the whole in-flight window, and it contains a native
    /// call. Dropping those seats would restore one as vacant while a session for
    /// it may already exist, so they travel; and an unresolved seat that is
    /// checkpointed again travels again.
    pub fn claimed_seats(&self) -> impl Iterator<Item = ClaimedSeat> + '_ {
        self.slots
            .iter()
            .filter_map(|(slot, admission)| match admission {
                SlotAdmission::Launching {
                    agent_run_id,
                    binding_id,
                    ..
                }
                | SlotAdmission::Unresolved {
                    agent_run_id,
                    binding_id,
                } => Some(ClaimedSeat {
                    slot: slot.clone(),
                    agent_run_id: *agent_run_id,
                    binding_id: *binding_id,
                }),
                SlotAdmission::Reserved { .. } | SlotAdmission::Occupied { .. } => None,
            })
    }

    /// Put an occupied seat back after a Kontor restart.
    ///
    /// The counterpart of [`Self::occupied_seats`]. It restores no reservation
    /// and therefore no authority: the seat comes back occupied, which refuses a
    /// second launch into it, and that is the state AC-4 needs to survive a
    /// restart.
    pub fn restore_occupied(&mut self, seat: OccupiedSeat) {
        self.slots.insert(
            seat.slot,
            SlotAdmission::Occupied {
                agent_run_id: seat.agent_run_id,
                binding_id: seat.binding_id,
                native_id: seat.native_id,
            },
        );
    }

    /// Put an in-flight claim back after a Kontor restart, shut.
    ///
    /// The counterpart of [`Self::claimed_seats`], and the fail-closed end of the
    /// restart race: the seat comes back refusing every admission and every
    /// launch. It restores no ticket, so nothing about it can be spent, and it is
    /// not reopened, because the launch it names may have reached the runtime.
    ///
    /// **The recovery boundary.** This table cannot clear such a seat and
    /// deliberately offers no way to. The one thing that would settle it is
    /// whether a native session exists for that run, which is evidence about a
    /// runtime rather than about seats. Kontor's route to it is the
    /// inventory-and-inspect reconciliation it already owes after a restart: a
    /// session carrying that run's correlation label appears there as an orphan,
    /// with no binding to its name, and ending that run is what ends this claim.
    /// Until then the seat stays shut — which costs one seat of one team run, and
    /// never a second live session.
    pub fn restore_claimed(&mut self, seat: ClaimedSeat) {
        self.slots.insert(
            seat.slot,
            SlotAdmission::Unresolved {
                agent_run_id: seat.agent_run_id,
                binding_id: seat.binding_id,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use kontor_core::id::{BoundedText, TaskId, parse_utc_timestamp};

    use super::*;
    use crate::workspace::WorkspaceRoot;

    fn seat() -> RoleSlotKey {
        RoleSlotKey::new(
            TeamRunId::generate(),
            RoleSlotId::parse("slot-a").expect("a valid slot id"),
        )
    }

    /// A launch aimed at `slot`, carrying authority issued for it.
    fn launch_request(slot: &RoleSlotKey) -> LaunchRequest {
        let agent_run_id = AgentRunId::generate();
        let binding_id = RuntimeBindingId::generate();
        let authority = LaunchAuthority::issue(
            AdmissionTicket::mint(),
            slot.clone(),
            agent_run_id,
            binding_id,
        );
        authority.into_request(LaunchParts {
            agent_run_id,
            team_run_id: slot.team_run_id,
            role_slot_id: slot.role_slot_id.clone(),
            task_id: TaskId::generate(),
            binding_id,
            workspace: None,
            cwd: WorkspaceRoot::parse("/w/task-1").expect("an absolute path"),
            account_profile_id: None,
            prompt: BoundedText::parse("do the work").expect("bounded text"),
            model_rung: kontor_core::spec::ModelRung {
                provider: kontor_core::spec::ProviderRef("test".to_owned()),
                model: kontor_core::spec::ModelRef("test".to_owned()),
                effort: None,
            },
            context_policy: kontor_core::spec::ContextPolicySnapshot::standard(
                &kontor_core::spec::ContextWindowBounds::unknown(),
                true,
                kontor_core::id::SCHEMA_VERSION,
                parse_utc_timestamp("2026-08-10T09:00:00Z").expect("a canonical time"),
            )
            .expect("the standard fallback freezes"),
            autonomy: kontor_core::spec::SeatAutonomy::standard(),
            requested_at: parse_utc_timestamp("2026-08-10T09:00:00Z").expect("a canonical time"),
        })
    }

    /// The reservation `request`'s authority was issued from.
    ///
    /// Placed rather than obtained from [`AdmissionLedger::admit`], which would
    /// mint its own ticket and answer with a different authority. Admission's own
    /// path is exercised by every adapter suite; what these tests need is the seat
    /// in a stated starting state.
    fn reserve(ledger: &mut AdmissionLedger, request: &LaunchRequest) {
        ledger.slots.insert(
            request.slot(),
            SlotAdmission::Reserved {
                ticket: request.authority().ticket(),
                agent_run_id: request.agent_run_id(),
                binding_id: request.binding_id(),
            },
        );
    }

    fn native(text: &str) -> ExternalId {
        ExternalId::parse(text).expect("a valid native id")
    }

    /// The three ways another seat can already be committed to a run: a launch in
    /// flight, the session it became, and one whose fate a restart could not
    /// settle. Every one of them means a native session for that run may exist.
    fn committed_states(agent_run_id: AgentRunId) -> [SlotAdmission; 3] {
        let binding_id = RuntimeBindingId::generate();
        [
            SlotAdmission::Launching {
                ticket: AdmissionTicket::mint(),
                agent_run_id,
                binding_id,
            },
            SlotAdmission::Occupied {
                agent_run_id,
                binding_id,
                native_id: native("native-session-1"),
            },
            SlotAdmission::Unresolved {
                agent_run_id,
                binding_id,
            },
        ]
    }

    /// One run owns one session, which the seat rule does not imply: two seats
    /// each holding their own reservation for one run pass the seat check twice.
    ///
    /// Stated here, over each state that counts as committed, because only the
    /// table can see them all — an adapter counting its own bindings cannot see a
    /// claim that has not become one yet.
    #[test]
    fn a_run_committed_in_one_seat_cannot_claim_another() {
        let here = seat();
        let request = launch_request(&here);

        for elsewhere in committed_states(request.agent_run_id()) {
            let mut ledger = AdmissionLedger::new();
            ledger.slots.insert(seat(), elsewhere.clone());
            reserve(&mut ledger, &request);

            let refused = ledger
                .claim(&request)
                .expect_err("a run already committed to a seat may not take a second one");

            assert!(
                matches!(refused, RuntimeError::SessionAlreadyBound { .. }),
                "the refusal names the run rather than the seat, got {refused:?} against \
                 {elsewhere:?}"
            );
            assert!(
                !ledger.is_reserved(&here),
                "and the reservation this run can never spend is abandoned rather than left \
                 to wedge the seat"
            );
        }
    }

    /// A reservation elsewhere is a question, not a session.
    ///
    /// Counting one as a conflict would refuse *both* launches of a run admitted
    /// in two seats, leaving it unable to start anywhere. The first claim wins.
    #[test]
    fn a_reservation_in_another_seat_does_not_block_a_claim() {
        let here = seat();
        let there = seat();
        let request = launch_request(&here);
        let mut ledger = AdmissionLedger::new();
        reserve(&mut ledger, &request);
        ledger.slots.insert(
            there.clone(),
            SlotAdmission::Reserved {
                ticket: AdmissionTicket::mint(),
                agent_run_id: request.agent_run_id(),
                binding_id: RuntimeBindingId::generate(),
            },
        );

        ledger.claim(&request).expect("the first claim wins");

        assert!(
            ledger.is_reserved(&there),
            "the seat it did not claim keeps its unspent question"
        );
    }

    /// One reservation admits one launch, even when two launches present it.
    ///
    /// The claim is what makes this true, and it is why checking and taking are
    /// one call: a second caller arriving while the first is still in its native
    /// call finds the seat taken rather than reservable. Stated here as the
    /// ledger's own rule; that a real adapter cannot interleave around it is
    /// proved against a gated transport in the AO suite.
    #[test]
    fn a_reservation_being_spent_cannot_be_claimed_a_second_time() {
        let slot = seat();
        let request = launch_request(&slot);
        let mut ledger = AdmissionLedger::new();
        reserve(&mut ledger, &request);

        ledger
            .claim(&request)
            .expect("the first launch takes the reservation");
        let second = ledger
            .claim(&request)
            .expect_err("and there is nothing left for a second to take");

        assert!(
            matches!(second, RuntimeError::LaunchNotAdmitted { .. }),
            "the second launch is unadmitted, not merely unlucky, got {second:?}"
        );
    }

    /// A session is recorded in the seat its own launch claimed, and no other.
    #[test]
    fn a_launch_that_never_claimed_its_seat_records_no_session_in_it() {
        let slot = seat();
        let request = launch_request(&slot);
        let mut ledger = AdmissionLedger::new();
        reserve(&mut ledger, &request);

        let refused = ledger
            .occupy(&request, native("native-session-1"))
            .expect_err("a reservation is not a claim");

        assert!(
            matches!(refused, RuntimeError::LaunchNotAdmitted { .. }),
            "got {refused:?}"
        );
        assert!(
            ledger.occupant(&slot).is_none(),
            "and the table is left exactly as it was"
        );
    }

    /// Releasing hands back a claim, never a seat with a session in it.
    ///
    /// [`AdmissionLedger::release`] runs on refusal paths only. Whether any
    /// refusal can reach it after the session exists is a fact about each
    /// adapter; the guarantee is the check against what the seat is actually
    /// holding, which survives someone adding a fallible step later. Stated here
    /// because no launch-level test can reach it.
    #[test]
    fn releasing_never_evicts_a_seat_that_holds_a_session() {
        let slot = seat();
        let request = launch_request(&slot);
        let mut ledger = AdmissionLedger::new();
        reserve(&mut ledger, &request);
        ledger.claim(&request).expect("the launch takes its seat");
        ledger
            .occupy(&request, native("native-session-1"))
            .expect("and the claim becomes the session it created");

        ledger.release(&request);

        assert_eq!(
            ledger.occupant(&slot).map(ExternalId::as_str),
            Some("native-session-1"),
            "a live session keeps its seat"
        );
    }

    /// And never another attempt's claim.
    ///
    /// The seat is right and the run and binding agree, so only the ticket tells
    /// this claim from the one the launch itself took.
    #[test]
    fn releasing_leaves_another_attempts_claim_alone() {
        let slot = seat();
        let request = launch_request(&slot);
        let mut ledger = AdmissionLedger::new();
        ledger.slots.insert(
            slot.clone(),
            SlotAdmission::Launching {
                ticket: AdmissionTicket::mint(),
                agent_run_id: request.agent_run_id(),
                binding_id: request.binding_id(),
            },
        );

        ledger.release(&request);

        assert_eq!(
            ledger.claimed_seats().count(),
            1,
            "a claim this launch never took is not its to spend or free"
        );
    }
}
