//! The deterministic ready-batch pass.
//!
//! One function, one snapshot, one ordered answer. [`plan`] reads nothing but its
//! argument: no clock, no database, no environment, no randomness, no hash-map
//! iteration order. Identical snapshots therefore produce byte-identical
//! decisions on this machine, on another one, and after a restart — which is what
//! makes a persisted admission re-checkable rather than merely re-readable.
//!
//! ## The pass
//!
//! 1. validate the snapshot; refuse the whole pass rather than half of it;
//! 2. sort every candidate by priority descending, then age, then task id;
//! 3. evaluate every candidate against [`BLOCKER_ORDER`], keeping the first
//!    blocker that refuses and every refusal;
//! 4. walk the eligible candidates once, in that same order, admitting each one
//!    that still fits its ceilings and collides with nothing already selected;
//! 5. stop admitting when the ceilings are spent — the remaining eligible
//!    candidates are still *decided*, as [`RejectionCode::CapacityExhausted`].
//!
//! Step 3 and step 4 are separate for a reason. Capacity is the only blocker
//! whose answer depends on what the pass has already done, so it is the only one
//! evaluated during the walk; every other blocker is a property of the snapshot
//! alone and is therefore answered identically no matter where in the order the
//! candidate sits.
//!
//! ## What the pass may not do
//!
//! It never calls a runtime, never writes a row and never cancels anything.
//! Admission is a *decision*; making it durable is one store transaction
//! (`kontor_store::SqliteStore::admit_candidate`), and dispatching it happens
//! after that transaction commits, out of the outbox.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{ExternalName, SCHEMA_VERSION, TaskId};
use kontor_core::state::TaskState;
use kontor_core::{DomainResult, closed_enum};
use kontor_policy::{ModuleClaim, module_isolated_by_worktree};
use kontor_runtime::RuntimeCapability;

use crate::model::{
    AccountPin, AdmittedCandidate, Candidate, CandidateDecision, CapacityLimitKind,
    CapacitySnapshot, CapacityUsage, Plan, PreflightOutcome, RejectionCode, RejectionEvidence,
    RosterGovernance, RuntimeHealth, SchedulingSnapshot, TaskOrigin, WorktreeClaim,
    WorktreeVerification,
};

closed_enum! {
    /// One blocker, in the order it is evaluated.
    ///
    /// The order is fixed and is part of the contract: a candidate's reported
    /// refusal is the *first* blocker that refuses it, so two passes over the
    /// same snapshot cannot report the same problem differently. It runs from the
    /// most fundamental facts to the most situational — a task that is not ready
    /// is never reported as a capacity problem — and capacity is deliberately
    /// last, because it is the only blocker whose answer depends on what the pass
    /// has already admitted.
    Blocker, "Blocker" {
        /// The task is not in `ready`.
        Readiness => "readiness",
        /// Whether the owning epic has the leadership its roster mandates.
        Governance => "governance",
        /// Where the work came from, and whether that authorizes it.
        Origin => "origin",
        /// Its dependencies.
        Dependencies => "dependencies",
        /// Its execution authorization.
        Authorization => "authorization",
        /// The already-resolved calendar answer.
        Calendar => "calendar",
        /// Unresolved external conflicts and ticket ownership.
        ExternalWork => "external_work",
        /// The pinned runtime's capabilities, trust, health and reconciliation.
        Runtime => "runtime",
        /// The pinned account's state, compatibility and preflight.
        Account => "account",
        /// Whether the worktree it claims was verified.
        Worktree => "worktree",
        /// Serialization peers and module contention against work already in
        /// flight.
        Contention => "contention",
    }
}

/// The blockers, in evaluation order.
///
/// `Blocker::ALL` is generated in declaration order by the macro, so this is that
/// order named for what it is rather than a second list to keep in step.
pub const BLOCKER_ORDER: &[Blocker] = Blocker::ALL;

/// Decide one pass.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the snapshot itself is not
/// admissible — a duplicate candidate, an out-of-range priority, a
/// self-dependency, an inconsistent calendar answer, a zero ceiling. Nothing is
/// decided in that case: a pass over an invalid snapshot has no partial answer.
pub fn plan(snapshot: &SchedulingSnapshot) -> DomainResult<Plan> {
    snapshot.validate()?;

    let sorted = snapshot.sorted_candidates();

    // Step 3. Every candidate, in order, against every snapshot-only blocker.
    // Rejections are retained rather than filtered away: the refusals are the
    // larger half of the record, and an operator asking "why is nothing running"
    // is asking about exactly them.
    let mut decisions: Vec<CandidateDecision> = Vec::with_capacity(sorted.len());
    let mut eligible: Vec<&Candidate> = Vec::with_capacity(sorted.len());
    for candidate in &sorted {
        match refuse(snapshot, candidate) {
            Some((code, evidence)) => decisions.push(CandidateDecision::Reject {
                task_id: candidate.task_id,
                project_id: candidate.project_id,
                code,
                evidence,
            }),
            None => eligible.push(candidate),
        }
    }

    // Steps 4 and 5. One walk, in the same order, over a working set that grows
    // as candidates are selected.
    let mut selection = Selection::new(snapshot);
    for candidate in eligible {
        match selection.take(snapshot, candidate) {
            Ok(admitted) => decisions.push(CandidateDecision::Admit(Box::new(admitted))),
            Err((code, evidence)) => decisions.push(CandidateDecision::Reject {
                task_id: candidate.task_id,
                project_id: candidate.project_id,
                code,
                evidence,
            }),
        }
    }

    // The decisions are re-sorted into the pass's own total order so that the
    // record reads in the order the candidates were considered, rather than in
    // two blocks — refused first, then walked. The order is the sort key, so it
    // is the same order for the same snapshot however the two loops interleaved.
    let position: BTreeMap<TaskId, usize> = sorted
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.task_id, index))
        .collect();
    decisions.sort_by_key(|decision| position.get(&decision.task_id()).copied().unwrap_or(0));

    Ok(Plan {
        schema_version: SCHEMA_VERSION,
        taken_at: snapshot.taken_at,
        decisions,
    })
}

/// One blocker's verdict on one candidate.
fn verdict(snapshot: &SchedulingSnapshot, candidate: &Candidate, blocker: Blocker) -> Refusal {
    match blocker {
        Blocker::Readiness => readiness(snapshot, candidate),
        Blocker::Governance => governance(candidate),
        Blocker::Origin => origin(candidate),
        Blocker::Dependencies => dependencies(snapshot, candidate),
        Blocker::Authorization => authorization(snapshot, candidate),
        Blocker::Calendar => calendar(candidate),
        Blocker::ExternalWork => external_work(candidate),
        Blocker::Runtime => runtime(snapshot, candidate),
        Blocker::Account => account(snapshot, candidate),
        Blocker::Worktree => worktree(candidate),
        Blocker::Contention => contention(snapshot, candidate),
    }
}

/// The first blocker that refuses `candidate`, with its evidence.
///
/// Capacity is not evaluated here: it is the one blocker whose answer depends on
/// the rest of the pass, and it is applied during the walk.
fn refuse(
    snapshot: &SchedulingSnapshot,
    candidate: &Candidate,
) -> Option<(RejectionCode, Vec<RejectionEvidence>)> {
    for blocker in BLOCKER_ORDER {
        let refusal = verdict(snapshot, candidate, *blocker);
        if refusal.is_some() {
            return refusal;
        }
    }
    None
}

/// One blocker's refusal, named.
///
/// [`Plan`] carries one code per candidate because a *decision* has one reason.
/// This carries the blocker as well, because an *explanation* has to say which of
/// the ten answered.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Refused {
    /// Which blocker refused.
    pub blocker: Blocker,
    /// The code it refused with.
    pub code: RejectionCode,
    /// What it refused on.
    pub evidence: Vec<RejectionEvidence>,
}

/// Every blocker that refuses `candidate`, in evaluation order.
///
/// # Why this exists alongside [`plan`]
///
/// [`plan`] reports the **first** blocker, and that is deliberate: a decision has
/// one reason, and reporting one keeps two passes over the same snapshot from
/// describing the same problem differently. That contract is unchanged and this
/// function does not touch it.
///
/// An *explanation* has the opposite need. An operator asking "why is nothing
/// running" who is told only about the first blocker fixes it, runs again, and is
/// told about the second — round a loop as long as the blocker list. So this
/// evaluates all ten and returns every one that refuses, leaving the caller to
/// present them together.
///
/// The two can never disagree, because they ask the same functions in the same
/// order: the first element of this list is exactly the code [`plan`] would
/// report.
///
/// # What is deliberately absent
///
/// Capacity. It is the one blocker whose answer depends on what the pass has
/// already admitted, so it has no meaning for a candidate considered on its own —
/// and inventing one would be reporting a ceiling nobody was measured against.
/// A capacity refusal appears in [`plan`]'s own decision for that candidate.
///
/// # Errors
/// Returns [`kontor_core::DomainError`] when the snapshot is not admissible, for
/// the same reasons [`plan`] does. An explanation of an invalid snapshot would be
/// an explanation of nothing.
pub fn explain(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> DomainResult<Vec<Refused>> {
    snapshot.validate()?;
    candidate.validate()?;
    Ok(BLOCKER_ORDER
        .iter()
        .filter_map(|blocker| {
            verdict(snapshot, candidate, *blocker).map(|(code, evidence)| Refused {
                blocker: *blocker,
                code,
                evidence,
            })
        })
        .collect())
}

type Refusal = Option<(RejectionCode, Vec<RejectionEvidence>)>;

fn bare(code: RejectionCode) -> Refusal {
    Some((code, Vec::new()))
}

fn with(code: RejectionCode, evidence: Vec<RejectionEvidence>) -> Refusal {
    Some((code, evidence))
}

// ---------------------------------------------------------------------------
// Blockers
// ---------------------------------------------------------------------------

/// Only `ready` work with nothing already running is admissible.
///
/// `todo` is deliberately not admissible: the domain defines it as "accepted, but
/// dependencies or inputs are not resolved yet", so admitting one would
/// contradict the state it is in. Work reaches `ready` either from `todo` or from
/// a receipt-backed resume, and both arrive here identically — the scheduler
/// cannot tell them apart and has no reason to.
///
/// The second half is the one a lease cannot cover. A task never contends with
/// itself for its own module, so a task that already has an envelope running is
/// invisible to every contention check; and because a task's lifecycle and its
/// runs are orthogonal dimensions, "still reads as `ready`" is not evidence that
/// nothing is running. A task is therefore implicitly serialized against itself.
fn readiness(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> Refusal {
    if candidate.state != TaskState::Ready {
        return bare(RejectionCode::TaskNotReady);
    }
    if snapshot.in_flight_tasks.contains(&candidate.task_id) {
        return bare(RejectionCode::TaskAlreadyInFlight);
    }
    None
}

/// The owning epic must have the governed leadership its roster mandates.
///
/// Declaring `LSA` and `TPM` mandatory only binds a roster that was actually
/// resolved. An epic that never froze one has no governed leadership at all, and
/// admitting its work is how eighteen tasks reach `done` with no architecture
/// lead. Refusing here is the check that makes the declaration mean something.
fn governance(candidate: &Candidate) -> Refusal {
    match candidate.governance {
        RosterGovernance::Seated => None,
        RosterGovernance::RosterUnfrozen => with(RejectionCode::EpicRosterUnfrozen, Vec::new()),
        RosterGovernance::LeadershipSeatUnbound => {
            with(RejectionCode::LeadershipSeatUnbound, Vec::new())
        }
    }
}

/// Manual work needs no receipt; event-origin work needs its lineage.
fn origin(candidate: &Candidate) -> Refusal {
    match candidate.origin.admits(candidate.task_id) {
        Ok(()) => None,
        Err(code) => {
            let evidence = match &candidate.origin {
                TaskOrigin::Event {
                    lineage: Some(lineage),
                } => vec![RejectionEvidence::IntakeReceipt {
                    id: lineage.receipt_id,
                }],
                TaskOrigin::Event { lineage: None } | TaskOrigin::Manual => Vec::new(),
            };
            with(code, evidence)
        }
    }
}

/// Every declared dependency must have reached `done`.
///
/// A dependency that is merely *not failed* is not finished, and a dependency
/// this snapshot knows nothing about is not finished either: absence is refused
/// rather than read as completion.
fn dependencies(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> Refusal {
    let unfinished: Vec<RejectionEvidence> = candidate
        .depends_on
        .iter()
        .filter(|dependency| !snapshot.completed_tasks.contains(*dependency))
        .map(|dependency| RejectionEvidence::Dependency {
            task_id: *dependency,
        })
        .collect();
    if unfinished.is_empty() {
        None
    } else {
        with(RejectionCode::DependencyIncomplete, unfinished)
    }
}

/// Ready work is default-allow. A grant only narrows; a disarm stops it.
fn authorization(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> Refusal {
    if let Some(id) = candidate.blocked_by {
        return with(
            RejectionCode::AuthorizationBlocked,
            vec![RejectionEvidence::Authorization { id }],
        );
    }
    let Some(authorization) = candidate.authorization.as_ref() else {
        return None;
    };
    let evidence = vec![RejectionEvidence::Authorization {
        id: authorization.id,
    }];
    if authorization.project_id != candidate.project_id
        || !authorization
            .scope
            .covers(candidate.mini_project_id, Some(candidate.task_id))
        || !(authorization.selected_tasks.is_empty()
            || authorization.selected_tasks.contains(&candidate.task_id))
    {
        return with(RejectionCode::AuthorizationScopeMismatch, evidence);
    }
    if snapshot.taken_at < authorization.allowed_start
        || snapshot.taken_at > authorization.allowed_end
    {
        return with(RejectionCode::AuthorizationExpired, evidence);
    }
    None
}

/// A closed or draining calendar admits no new top-level run.
///
/// An *unrestricted* answer does not invent an authorization requirement —
/// default-allow is the same idea as an unconfigured calendar.
fn calendar(candidate: &Candidate) -> Refusal {
    if candidate.calendar.admits_new_work() {
        return None;
    }
    let evidence = vec![RejectionEvidence::Calendar {
        state: candidate.calendar.state,
        next_opening: candidate.calendar.next_opening,
    }];
    let code =
        if candidate.calendar.state == kontor_core::calendar::EffectiveCalendarState::Draining {
            RejectionCode::CalendarDraining
        } else {
            RejectionCode::CalendarClosed
        };
    with(code, evidence)
}

/// External status is a gate, never a source of ownership.
fn external_work(candidate: &Candidate) -> Refusal {
    if let Some(conflict) = candidate.external.blocking_conflicts.first() {
        return with(
            RejectionCode::ExternalConflictUnresolved,
            vec![RejectionEvidence::Conflict { id: *conflict }],
        );
    }
    // Work with no external link has no ownership gate, so there is nothing here
    // to refuse.
    let ownership = candidate.external.ownership.as_ref()?;
    // Reported, never resolved by taking over. Kontor's own task state and
    // ownership are untouched by this refusal.
    if ownership.owned_by_another() {
        return bare(RejectionCode::ExternalOwnershipConflict);
    }
    if !ownership.milestone_confirmed {
        return bare(RejectionCode::OwnershipMilestoneUnconfirmed);
    }
    None
}

/// The runtime must be able to do the work, be allowed to be driven, be
/// answering, have been reconciled, and have said so recently.
fn runtime(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> Refusal {
    let evidence = || {
        vec![RejectionEvidence::Runtime {
            runtime_kind: candidate.runtime.runtime_kind.clone(),
            generation: candidate.runtime.generation,
        }]
    };

    // An undeclared capability produces no effect, so it is refused before
    // anything else is considered about the runtime.
    if let Some(missing) = candidate
        .runtime
        .required
        .iter()
        .find(|capability| !candidate.runtime.capabilities.supports(**capability))
    {
        return with(
            RejectionCode::RuntimeCapabilityMissing,
            vec![RejectionEvidence::MissingRuntimeCapability {
                capability: *missing,
            }],
        );
    }

    // Grade C is advisory: it may be discovered, inspected and read, and the
    // scheduler never drives it. The predicate is `kontor-runtime`'s, so the
    // grades cannot be re-ranked here by accident.
    if !candidate
        .runtime
        .capabilities
        .trust_grade
        .may_dispatch_autonomously()
    {
        return with(RejectionCode::RuntimeTrustInsufficient, evidence());
    }

    if candidate.runtime.health != RuntimeHealth::Healthy {
        return with(RejectionCode::RuntimeUnhealthy, evidence());
    }

    let reconciliation = &candidate.runtime.reconciliation;
    let scope_matches = reconciliation.scope.project_id == candidate.project_id
        && reconciliation.scope.runtime_kind == candidate.runtime.runtime_kind
        && reconciliation.scope.host == candidate.runtime.host
        && reconciliation.scope.generation == candidate.runtime.generation;
    if !reconciliation.epoch_completed
        || !scope_matches
        || reconciliation.open_replay_gap
        || reconciliation.divergence
        || reconciliation.orphan_ambiguity
        || reconciliation.stale_lost_contact
    {
        return with(RejectionCode::RuntimeReconciliationIncomplete, evidence());
    }

    if !fresh(snapshot, candidate.runtime.last_confirmed_at) {
        return with(RejectionCode::RuntimeEvidenceStale, evidence());
    }
    None
}

/// Whether an instant is inside the snapshot's freshness window.
///
/// `None` is never fresh. Nothing has been confirmed, and "we have never heard
/// from it" is not evidence that it is there.
fn fresh(snapshot: &SchedulingSnapshot, at: Option<kontor_core::id::Timestamp>) -> bool {
    at.is_some_and(|at| {
        at <= snapshot.taken_at && snapshot.taken_at.duration_since(at) <= snapshot.freshness
    })
}

/// The pinned account must still be the account that was pinned.
fn account(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> Refusal {
    let Some(pin) = candidate.account.pin.as_ref() else {
        // Unpinned work requires no account evidence. It is not "any account
        // will do": there is no account, so there is nothing to prove about one.
        return None;
    };
    let evidence = || {
        vec![RejectionEvidence::Account {
            id: pin.account_profile_id,
        }]
    };

    if pin.pinned_revision != pin.current_revision {
        return with(RejectionCode::AccountPinStale, evidence());
    }
    if !pin.enabled {
        return with(RejectionCode::AccountDisabled, evidence());
    }
    if pin
        .cooldown_until
        .is_some_and(|until| snapshot.taken_at < until)
    {
        return with(RejectionCode::AccountCoolingDown, evidence());
    }
    if pin.harness != candidate.runtime.runtime_kind {
        return with(RejectionCode::AccountRuntimeIncompatible, evidence());
    }
    if let Some(missing) = candidate
        .account
        .required_capabilities
        .iter()
        .find(|capability| !pin.declared_capabilities.contains(*capability))
    {
        return with(
            RejectionCode::AccountCapabilityMissing,
            vec![RejectionEvidence::MissingAccountCapability {
                capability: missing.clone(),
            }],
        );
    }
    // An account-pinned run through a runtime that cannot prove which account it
    // executed as makes the pin unverifiable, which is the same as not having
    // one. `kontor_runtime::preflight` refuses this at dispatch; refusing it here
    // means nothing is queued that dispatch would have to throw away.
    if !candidate.runtime.capabilities.account_env {
        return with(RejectionCode::AccountEnvironmentUnavailable, evidence());
    }
    if !preflight_valid(snapshot, pin) {
        return with(RejectionCode::FleetPreflightFailed, evidence());
    }
    None
}

/// Whether a preflight both passed and is recent enough to be evidence.
fn preflight_valid(snapshot: &SchedulingSnapshot, pin: &AccountPin) -> bool {
    pin.preflight.outcome == PreflightOutcome::Passed
        && fresh(snapshot, Some(pin.preflight.observed_at))
}

/// A claimed tree that nothing verified buys no isolation and is refused as
/// such.
///
/// Refusing rather than downgrading it to "no tree" is the point: a fabricated
/// path would otherwise be indistinguishable from unisolated work, and unisolated
/// work is admissible when nothing else holds the module.
fn worktree(candidate: &Candidate) -> Refusal {
    match candidate.worktree.as_ref() {
        Some(claim) if claim.verification == WorktreeVerification::Unverified => with(
            RejectionCode::WorktreeUnverified,
            vec![RejectionEvidence::Worktree {
                worktree: claim.worktree.clone(),
            }],
        ),
        Some(_) | None => None,
    }
}

/// Nothing may run beside work it serializes against, and nothing may hold a
/// module another task already holds without a distinct verified tree.
fn contention(snapshot: &SchedulingSnapshot, candidate: &Candidate) -> Refusal {
    if let Some(peer) = candidate
        .serializes_with
        .iter()
        .find(|peer| snapshot.in_flight_tasks.contains(*peer))
    {
        return with(
            RejectionCode::SerializationPeerInFlight,
            vec![RejectionEvidence::SerializationPeer { task_id: *peer }],
        );
    }
    module_conflict(candidate, &snapshot.module_leases)
}

/// Whether `candidate` may hold its module given the claims already held.
///
/// The isolation rule is `kontor_policy::module_isolated_by_worktree` — the same
/// function the `module_collision` guardrail uses, so the answer a scheduler
/// gives and the answer a guardrail gives cannot drift apart. What differs is the
/// input: the guardrail is asked about one run's action, and this is asked about
/// every module lease in the Realm.
fn module_conflict(candidate: &Candidate, held: &[ModuleClaim]) -> Refusal {
    // Work that contends for no module cannot collide over one.
    let mut evidence = Vec::new();
    let mine = candidate
        .worktree
        .as_ref()
        .and_then(WorktreeClaim::verified);
    for module in candidate.integration_modules() {
        let contenders: Vec<&ModuleClaim> = held
            .iter()
            .filter(|claim| {
                claim.in_flight
                    && claim.module.contends_with(&module)
                    && claim.task_id != candidate.task_id
            })
            .collect();
        if contenders.is_empty() {
            continue;
        }
        if module_isolated_by_worktree(mine, contenders.iter().copied()) {
            continue;
        }
        evidence.extend(
            contenders
                .iter()
                .map(|claim| RejectionEvidence::ModuleHeld {
                    module: claim.module.clone(),
                    task_id: claim.task_id,
                }),
        );
    }
    if evidence.is_empty() {
        return None;
    }
    with(RejectionCode::ModuleInFlight, evidence)
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// The pass's growing working set.
///
/// Capacity and contention are both cumulative: admitting a candidate spends
/// headroom under six ceilings and claims a module and a tree, and the next
/// candidate must see all of it. So the walk carries a mutable copy of the usage
/// and of the claim lists rather than re-reading the snapshot, which is also why
/// a second scheduler instance cannot be kept out by this type — that is the
/// admission transaction's job, and the leases are what do it.
struct Selection {
    usage: CapacityUsage,
    module_claims: Vec<ModuleClaim>,
    worktrees: BTreeSet<ExternalName>,
    selected_tasks: BTreeSet<TaskId>,
    window_remaining: u32,
}

impl Selection {
    fn new(snapshot: &SchedulingSnapshot) -> Self {
        Self {
            usage: snapshot.usage.clone(),
            module_claims: snapshot.module_leases.clone(),
            worktrees: snapshot.worktree_leases.clone(),
            selected_tasks: BTreeSet::new(),
            window_remaining: snapshot.adaptive_window.current(),
        }
    }

    /// Admit `candidate` if it still fits, and record what that spent.
    fn take(
        &mut self,
        snapshot: &SchedulingSnapshot,
        candidate: &Candidate,
    ) -> Result<AdmittedCandidate, (RejectionCode, Vec<RejectionEvidence>)> {
        // A peer selected earlier in this same pass is exactly as much of a
        // conflict as one already running: it is about to be running.
        if let Some(peer) = candidate
            .serializes_with
            .iter()
            .find(|peer| self.selected_tasks.contains(*peer))
        {
            return Err((
                RejectionCode::SerializationPeerInFlight,
                vec![RejectionEvidence::SerializationPeer { task_id: *peer }],
            ));
        }
        if let Some(refusal) = module_conflict(candidate, &self.module_claims) {
            return Err(refusal);
        }
        // Two candidates cannot both claim one tree. Whichever the order put
        // first keeps it; the second is not isolated from the first by a tree
        // they share.
        let verified = candidate
            .worktree
            .as_ref()
            .and_then(WorktreeClaim::verified)
            .cloned();
        if let Some(worktree) = verified.as_ref()
            && self.worktrees.contains(worktree)
        {
            return Err((
                RejectionCode::WorktreeDuplicate,
                vec![RejectionEvidence::Worktree {
                    worktree: worktree.clone(),
                }],
            ));
        }

        let capacity = self.headroom(snapshot, candidate);
        if capacity.effective == 0 {
            return Err((
                RejectionCode::CapacityExhausted,
                vec![RejectionEvidence::Capacity {
                    limit: capacity.binding,
                    remaining: 0,
                }],
            ));
        }

        // A grant is optional: default-allow admits with no authorization_id.
        let authorization_id = candidate
            .authorization
            .as_ref()
            .map(|authorization| authorization.id);

        self.spend(candidate);
        for claim in candidate.module_claims() {
            self.module_claims.push(claim);
        }
        if let Some(worktree) = verified.clone() {
            self.worktrees.insert(worktree);
        }
        self.selected_tasks.insert(candidate.task_id);

        Ok(AdmittedCandidate {
            project_id: candidate.project_id,
            task_id: candidate.task_id,
            revision: candidate.revision,
            workflow_id: candidate.workflow_id,
            ordering: candidate.ordering(),
            capacity,
            module: candidate.module.clone(),
            changed_modules: candidate.changed_modules.clone(),
            worktree: verified,
            authorization_id,
            calendar: candidate.calendar.clone(),
            account_profile_id: candidate
                .account
                .pin
                .as_ref()
                .map(|pin| pin.account_profile_id),
            runtime_kind: candidate.runtime.runtime_kind.clone(),
            runtime_generation: candidate.runtime.generation,
            intake_receipt_id: match &candidate.origin {
                TaskOrigin::Event {
                    lineage: Some(lineage),
                } => Some(lineage.receipt_id),
                TaskOrigin::Event { lineage: None } | TaskOrigin::Manual => None,
            },
        })
    }

    /// The remaining headroom under every ceiling that applies to `candidate`.
    ///
    /// The effective capacity is the *minimum*, which is the only reading that
    /// cannot over-admit: a batch bounded by the sum, the maximum or the global
    /// ceiling alone would exceed whichever of the others happened to be
    /// smallest.
    ///
    /// A ceiling that does not apply — mission for a task with no goal, account
    /// and provider for unpinned work — is absent from the map rather than
    /// recorded as unlimited, so the record says which ceilings were consulted.
    fn headroom(&self, snapshot: &SchedulingSnapshot, candidate: &Candidate) -> CapacitySnapshot {
        let config = &snapshot.capacity;
        let mut remaining: BTreeMap<CapacityLimitKind, u32> = BTreeMap::new();

        let spare = |limit: u32, used: u32| limit.saturating_sub(used);

        remaining.insert(
            CapacityLimitKind::Global,
            spare(config.global_max_in_flight, self.usage.global_in_flight),
        );
        remaining.insert(
            CapacityLimitKind::Project,
            spare(
                config.project_max_in_flight,
                counted(&self.usage.project_in_flight, &candidate.project_id),
            ),
        );
        if let Some(mission) = candidate.mini_project_id {
            remaining.insert(
                CapacityLimitKind::Mission,
                spare(
                    config.mission_max_in_flight,
                    counted(&self.usage.mission_in_flight, &mission),
                ),
            );
        }
        if let Some(pin) = candidate.account.pin.as_ref() {
            remaining.insert(
                CapacityLimitKind::Account,
                spare(
                    config.account_max_in_flight,
                    counted(&self.usage.account_in_flight, &pin.account_profile_id),
                ),
            );
        }
        if let Some(provider) = candidate.provider() {
            remaining.insert(
                CapacityLimitKind::Provider,
                spare(
                    config.provider_max_in_flight,
                    counted(&self.usage.provider_in_flight, provider),
                ),
            );
        }
        let runtime_used = counted(
            &self.usage.runtime_in_flight,
            &candidate.runtime.runtime_kind,
        );
        remaining.insert(
            CapacityLimitKind::Runtime,
            spare(config.runtime_max_in_flight, runtime_used),
        );
        // The runtime's own declared bound. It is not configuration: the runtime
        // said how many simultaneous sessions it will accept, and exceeding it
        // would be refused at dispatch after the work was already queued.
        remaining.insert(
            CapacityLimitKind::RuntimeSessions,
            spare(
                candidate
                    .runtime
                    .capabilities
                    .limits
                    .max_concurrent_sessions,
                runtime_used,
            ),
        );
        if let Some(authorization) = candidate.authorization.as_ref() {
            remaining.insert(
                CapacityLimitKind::Authorization,
                spare(
                    authorization.max_concurrency,
                    counted(&self.usage.project_in_flight, &candidate.project_id),
                ),
            );
        }
        remaining.insert(CapacityLimitKind::AdaptiveWindow, self.window_remaining);

        // `min_by_key` over the sorted map is deterministic, and the tie goes to
        // the earliest-declared ceiling — so the same exhausted snapshot always
        // names the same limit.
        let (binding, effective) = remaining
            .iter()
            .min_by_key(|(limit, spare)| (**spare, **limit))
            .map_or((CapacityLimitKind::Global, 0), |(limit, spare)| {
                (*limit, *spare)
            });
        CapacitySnapshot {
            remaining,
            effective,
            binding,
        }
    }

    /// Record what admitting `candidate` spent.
    fn spend(&mut self, candidate: &Candidate) {
        self.usage.global_in_flight = self.usage.global_in_flight.saturating_add(1);
        bump(&mut self.usage.project_in_flight, candidate.project_id);
        if let Some(mission) = candidate.mini_project_id {
            bump(&mut self.usage.mission_in_flight, mission);
        }
        if let Some(pin) = candidate.account.pin.as_ref() {
            bump(&mut self.usage.account_in_flight, pin.account_profile_id);
        }
        if let Some(provider) = candidate.provider().cloned() {
            bump(&mut self.usage.provider_in_flight, provider);
        }
        bump(
            &mut self.usage.runtime_in_flight,
            candidate.runtime.runtime_kind.clone(),
        );
        self.window_remaining = self.window_remaining.saturating_sub(1);
    }
}

/// How much of a keyed ceiling is spent. An absent key is zero, never unlimited.
fn counted<K: Ord>(counts: &BTreeMap<K, u32>, key: &K) -> u32 {
    counts.get(key).copied().unwrap_or(0)
}

fn bump<K: Ord>(counts: &mut BTreeMap<K, u32>, key: K) {
    let entry = counts.entry(key).or_insert(0);
    *entry = entry.saturating_add(1);
}

/// A runtime capability set every launch needs, as a convenience for callers
/// assembling a snapshot.
///
/// Launching a session and reading it back is the minimum a driven runtime has to
/// be able to do; a runtime that cannot report what it started cannot be
/// supervised, and supervising is not optional.
#[must_use]
pub fn minimum_launch_capabilities() -> BTreeSet<RuntimeCapability> {
    [
        RuntimeCapability::Launch,
        RuntimeCapability::Inspect,
        RuntimeCapability::LiveEvents,
    ]
    .into_iter()
    .collect()
}
