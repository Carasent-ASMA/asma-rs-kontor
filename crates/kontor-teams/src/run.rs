//! The sealed role-slot lifecycle: one non-terminal native session per slot,
//! evidenced replacement, successor lineage and team closure certification.
//!
//! [`TeamRunSlots`] owns the whole state of a team run's seats and keeps it
//! private. Callers never hold a slot's state; they hold a *permit* or a
//! *handle*, and each of those exposes exactly one legal next operation:
//!
//! ```text
//! TeamRunLease --open / hydrate------> TeamRunSlots  (one writer per team run)
//! vacant slot --reserve--------------> LaunchPermit
//! LaunchPermit --admission_request---> AdmissionRequest -> the runtime
//! runtime --admit_launch-------------> LaunchAuthority   (or a refusal)
//! LaunchPermit --launch_request------> PreparedLaunch    (both are spent)
//! PreparedLaunch --bind(snapshot)----> OccupiedSlot
//! OccupiedSlot --resume / send--------> the same RuntimeBindingSnapshot
//! OccupiedSlot --begin_replacement----> ReplacementPending   (no launch here)
//! OccupiedSlot --close_completed------> ClosedSlot
//! ReplacementPending --close_replaced-> ClosedSlot
//! ClosedSlot --reserve_successor------> LaunchPermit (parent = the closed run,
//!                                       citing the binding it retired)
//! ```
//!
//! ## Where the AC-4 guarantee actually lives
//!
//! **At the runtime, not here.**
//! [`kontor_runtime::adapter::RuntimeAdapter::admit_launch`] owns a table keyed
//! by `(team run, role slot)` and decides, atomically, whether a seat may be
//! filled; [`kontor_runtime::adapter::RuntimeAdapter::launch`] consumes that
//! decision before its first native effect. A [`LaunchPermit`] cannot produce a
//! launch on its own — there is no way to build a
//! [`kontor_runtime::request::LaunchRequest`] except from a
//! [`LaunchAuthority`], and no way to get one of those except by asking a
//! runtime.
//!
//! That is deliberate, and it is the second design here. The first tried to make
//! the permit itself the proof, and could not: Rust has no friend-crate
//! visibility, so any entry point `kontor-teams` can call is callable by anyone
//! who can reach it, and Cargo unifies features per build. A caller-side token
//! cannot be made unforgeable, so the decision moved to the only party whose
//! answer a caller cannot restate.
//!
//! What *this* module still contributes is real, and is about **Kontor's own
//! records** rather than about native sessions: one writer per team run
//! ([`TeamRunLease`]), one live attempt per seat in the roster, lineage that
//! forces every replacement to name its parent, and the citation
//! ([`ClosedSlot::retired_binding`]) that lets the runtime agree a seat is free.
//! A manager that lost its answer, or a second manager that cannot see the
//! first, is refused by the runtime rather than by hoping these records agree.
//!
//! Two sessions in parallel still require two declared slots, which is AC-5.
//!
//! Nothing here reads a slot id, a role name or a gate name. The rules are
//! structural, so `researcher-a`/`researcher-b` and `q7`/`q8` behave identically.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{LazyLock, Mutex};

use kontor_core::id::{
    AccountProfileId, AgentRunId, ArtifactKey, BoundedText, CanonicalDocument, ContentHash,
    ProjectId, RoleKey, RuntimeBindingId, SchemaVersion, SpecVersion, TaskId, TeamRunId,
    TeamTemplateId, Timestamp,
};
use kontor_core::repository::{AgentRun, NewAgentRun};
use kontor_core::spec::{
    ContextPolicySnapshot, ContextWindowPolicy, ModelRung, ResolvedContextPolicy,
    TeamContextPolicySeed, TeamRunSnapshot,
};
use kontor_core::state::{
    RunLifecycle, SlotDisposition, TaskTeamClosure, TeamChildEvidence, TeamEvidenceSource,
    TeamTerminalEvidence, TerminalOutcome, reduce_team_outcome, team_child_evidence_digest,
};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::admission::{AdmissionRequest, LaunchAuthority, ReplacedBinding, RoleSlotKey};
use kontor_runtime::capability::RuntimeBindingSnapshot;
use kontor_runtime::request::{
    LaunchParts, LaunchPlacement, LaunchRequest, MessageId, ResumeRequest, SendMessageRequest,
};
use kontor_runtime::workspace::WorkspaceRoot;
use serde::Serialize;

use crate::spec::{RoleSlotId, TeamTemplateSpec};

// ---------------------------------------------------------------------------
// Private slot state
// ---------------------------------------------------------------------------

/// One finished attempt at a slot, with the evidence that closed it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClosedAttempt {
    agent_run_id: AgentRunId,
    parent: Option<AgentRunId>,
    lifecycle: RunLifecycle,
    evidence_hash: ContentHash,
    /// The session this attempt held, if it ever held one. Kept because a
    /// successor has to cite the exact binding it is replacing, and the runtime
    /// checks that citation against the session it still owns.
    binding_id: Option<RuntimeBindingId>,
}

/// What is happening at a slot right now.
#[derive(Debug)]
enum SlotHead {
    /// No live attempt. Every attempt so far is closed.
    Vacant,
    /// An attempt has been reserved but not yet bound to a native session.
    Reserved {
        agent_run_id: AgentRunId,
        parent: Option<AgentRunId>,
    },
    /// One native session is bound to this slot.
    Occupied {
        agent_run_id: AgentRunId,
        parent: Option<AgentRunId>,
        binding: Box<RuntimeBindingSnapshot>,
    },
    /// The occupying session must reach an evidenced terminal before the slot
    /// may be filled again. No launch is reachable from here.
    Replacing {
        agent_run_id: AgentRunId,
        parent: Option<AgentRunId>,
        binding: Box<RuntimeBindingSnapshot>,
    },
}

impl SlotHead {
    const fn live_run(&self) -> Option<AgentRunId> {
        match self {
            Self::Vacant => None,
            Self::Reserved { agent_run_id, .. }
            | Self::Occupied { agent_run_id, .. }
            | Self::Replacing { agent_run_id, .. } => Some(*agent_run_id),
        }
    }

    const fn parent(&self) -> Option<AgentRunId> {
        match self {
            Self::Vacant => None,
            Self::Reserved { parent, .. }
            | Self::Occupied { parent, .. }
            | Self::Replacing { parent, .. } => *parent,
        }
    }

    /// The session the slot is currently holding, if it has one.
    fn binding(&self) -> Option<&RuntimeBindingSnapshot> {
        match self {
            Self::Vacant | Self::Reserved { .. } => None,
            Self::Occupied { binding, .. } | Self::Replacing { binding, .. } => Some(binding),
        }
    }
}

// ---------------------------------------------------------------------------
// Exclusive ownership of one team run's seats
// ---------------------------------------------------------------------------

/// The team runs whose seats are currently owned by a live [`TeamRunSlots`].
static LEASED_TEAM_RUNS: LazyLock<Mutex<BTreeSet<TeamRunId>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));

fn leased() -> std::sync::MutexGuard<'static, BTreeSet<TeamRunId>> {
    // A panic while holding this lock leaves the set intact — it only ever
    // contains ids — so recovering from poisoning is strictly better than
    // refusing every later lease because one unrelated test panicked.
    LEASED_TEAM_RUNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Exclusive authority to fill one team run's seats.
///
/// [`TeamRunSlots`] holds the whole state of a team run in memory, so two of
/// them built from the same snapshot would each believe a seat was vacant and
/// each hand out a permit for it — AC-4 broken not by a missing check but by two
/// managers that cannot see each other. The lease makes that unrepresentable:
/// it is minted once per [`TeamRunId`], is not [`Clone`], and is released only
/// when the manager holding it is dropped.
///
/// The scope is deliberately this process. Two *processes* are kept apart by
/// the store's compare-and-swap revisions, which is where cross-process
/// exclusivity belongs; a lease cannot and does not claim to provide it.
#[derive(Debug)]
pub struct TeamRunLease {
    team_run_id: TeamRunId,
}

impl TeamRunLease {
    /// Take exclusive ownership of one team run's seats.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when another live manager already owns
    /// this team run. That is a caller bug — two writers for one team run — and
    /// failing here is what stops it from becoming a double launch later.
    pub fn acquire(team_run_id: TeamRunId) -> DomainResult<Self> {
        if !leased().insert(team_run_id) {
            return Err(DomainError::invalid(
                "TeamRunLease",
                "another live manager already owns this team run's seats",
            ));
        }
        Ok(Self { team_run_id })
    }

    /// The team run this lease owns.
    #[must_use]
    pub const fn team_run_id(&self) -> TeamRunId {
        self.team_run_id
    }
}

impl Drop for TeamRunLease {
    fn drop(&mut self) {
        leased().remove(&self.team_run_id);
    }
}

#[derive(Debug)]
struct SlotState {
    lineage: Vec<ClosedAttempt>,
    head: SlotHead,
}

impl SlotState {
    const fn vacant() -> Self {
        Self {
            lineage: Vec::new(),
            head: SlotHead::Vacant,
        }
    }
}

// ---------------------------------------------------------------------------
// Permits and handles
// ---------------------------------------------------------------------------

/// This manager's intent to fill one slot, and the bookkeeping that goes with
/// it.
///
/// It is deliberately not [`Clone`], and it is consumed by
/// [`TeamRunSlots::bind`], so the local roster cannot record two sessions for
/// one seat. It is **not** the authority to launch: that comes from the runtime,
/// through [`LaunchPermit::admission_request`], and only the runtime can issue
/// it.
#[derive(Debug)]
pub struct LaunchPermit {
    team_run_id: TeamRunId,
    slot: RoleSlotId,
    agent_run_id: AgentRunId,
    parent: Option<AgentRunId>,
    /// The finished predecessor this attempt replaces, when it replaces one.
    replaces: Option<ReplacedBinding>,
}

impl LaunchPermit {
    /// The slot this permit fills.
    #[must_use]
    pub const fn slot(&self) -> &RoleSlotId {
        &self.slot
    }

    /// The seat this permit fills, as the runtime addresses it.
    #[must_use]
    pub fn slot_key(&self) -> RoleSlotKey {
        RoleSlotKey::new(self.team_run_id, self.slot.clone())
    }

    /// The run this permit launches.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.agent_run_id
    }

    /// The attempt this one succeeds, if it is a replacement.
    #[must_use]
    pub const fn parent_agent_run_id(&self) -> Option<AgentRunId> {
        self.parent
    }

    /// The storage row for this attempt.
    ///
    /// The slot id becomes `role` and the closed predecessor becomes
    /// `parent_agent_run_id`; this is the one place team code maps a slot into
    /// storage, so neither can be dropped at a call site.
    ///
    /// `binding` is the session the launch produced. KON-MVP-03 writes a run and
    /// its binding in one insert and offers no later bind, so a row that will
    /// carry a session is written once the runtime has answered — passing the
    /// snapshot here is what keeps the two agreeing. `None` records an attempt
    /// that has not been launched yet.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the binding belongs to another run, or is
    /// not correlated with the run it names.
    pub fn new_agent_run(
        &self,
        project_id: ProjectId,
        account_profile_id: Option<AccountProfileId>,
        binding: Option<&RuntimeBindingSnapshot>,
        created_at: Timestamp,
    ) -> DomainResult<NewAgentRun> {
        let bound = match binding {
            Some(snapshot) => {
                if snapshot.agent_run_id() != self.agent_run_id {
                    return Err(DomainError::invalid(
                        "TeamRunSlots",
                        "the binding belongs to a different run than the permit",
                    ));
                }
                snapshot.ensure_correlated().map_err(|_| {
                    DomainError::invalid(
                        "TeamRunSlots",
                        "the binding is not correlated with its own run",
                    )
                })?;
                Some(snapshot.binding.clone())
            }
            None => None,
        };
        Ok(NewAgentRun {
            id: self.agent_run_id,
            project_id,
            team_run_id: self.team_run_id,
            parent_agent_run_id: self.parent,
            role: self.slot.as_role_key().clone(),
            account_profile_id,
            binding: bound,
            created_at,
        })
    }

    /// Ask the runtime to admit this attempt into this seat.
    ///
    /// The seat, the run and the replacement citation all come from the permit,
    /// never from the caller, so a request built here cannot be aimed at a seat
    /// the manager did not reserve or claim a predecessor it did not close. What
    /// comes back is the runtime's decision, and only it can produce a
    /// [`LaunchAuthority`].
    #[must_use]
    pub fn admission_request(&self, launch: &SlotLaunch) -> AdmissionRequest {
        AdmissionRequest {
            slot: self.slot_key(),
            agent_run_id: self.agent_run_id,
            binding_id: launch.binding_id,
            replaces: self.replaces.clone(),
            requested_at: launch.requested_at,
        }
    }

    /// Spend the runtime's authority on exactly one launch request.
    ///
    /// Taking both `self` and `authority` by value is the point: one reservation
    /// yields one request, and the permit cannot be asked for a second one.
    ///
    /// Nothing is validated here. The runtime compares what this request says
    /// against the reservation it is holding, and that is the only comparison
    /// that means anything — a check written in this crate would be a check the
    /// runtime is trusting a caller to have made.
    #[must_use]
    pub fn launch_request(self, authority: LaunchAuthority, launch: SlotLaunch) -> PreparedLaunch {
        let request = authority.into_request(LaunchParts {
            agent_run_id: self.agent_run_id,
            team_run_id: self.team_run_id,
            role_slot_id: self.slot.clone(),
            task_id: launch.task_id,
            binding_id: launch.binding_id,
            placement: launch.placement,
            cwd: launch.cwd,
            account_profile_id: launch.account_profile_id,
            prompt: launch.prompt,
            model_rung: launch.model_rung,
            context_policy: launch.context_policy,
            requested_at: launch.requested_at,
        });
        PreparedLaunch {
            permit: self,
            request,
        }
    }
}

/// One spent permit, one spent authority, and the single request they produced.
///
/// The request is lent out, never handed over, and the permit inside is what
/// [`TeamRunSlots::bind`] consumes — so this roster's path from "this seat is
/// vacant" to "this seat holds a session" passes through exactly one request.
///
/// Handing the same `&LaunchRequest` to an adapter twice is still physically
/// possible, and harmless:
/// [`kontor_runtime::adapter::RuntimeAdapter::launch`] consumed the reservation
/// the first time, so the second call finds nothing to spend.
#[derive(Debug)]
pub struct PreparedLaunch {
    permit: LaunchPermit,
    request: LaunchRequest,
}

impl PreparedLaunch {
    /// The request to hand to the runtime.
    #[must_use]
    pub const fn request(&self) -> &LaunchRequest {
        &self.request
    }

    /// The slot this launch fills.
    #[must_use]
    pub const fn slot(&self) -> &RoleSlotId {
        self.permit.slot()
    }

    /// The run being launched.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.permit.agent_run_id()
    }

    /// The attempt this one succeeds, if it is a replacement.
    #[must_use]
    pub const fn parent_agent_run_id(&self) -> Option<AgentRunId> {
        self.permit.parent_agent_run_id()
    }

    /// The storage row for this attempt, once the runtime has answered.
    ///
    /// # Errors
    /// As [`LaunchPermit::new_agent_run`].
    pub fn new_agent_run(
        &self,
        project_id: ProjectId,
        account_profile_id: Option<AccountProfileId>,
        binding: Option<&RuntimeBindingSnapshot>,
        created_at: Timestamp,
    ) -> DomainResult<NewAgentRun> {
        self.permit
            .new_agent_run(project_id, account_profile_id, binding, created_at)
    }
}

/// Everything a launch needs that the slot does not already know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLaunch {
    /// The task the team serves.
    pub task_id: TaskId,
    /// The binding id Kontor has minted for the session to come.
    pub binding_id: RuntimeBindingId,
    /// The verified place every role of this team run works in.
    pub placement: Option<LaunchPlacement>,
    /// Where this role says it will work.
    pub cwd: WorkspaceRoot,
    /// The coding account this attempt is pinned to, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// What the session starts with.
    pub prompt: BoundedText,
    /// The exact provider/model/effort rung selected from the frozen template.
    pub model_rung: ModelRung,
    /// The frozen requested/effective context-window policy for this seat.
    ///
    /// Resolved from [`TeamRunSlots::requested_context_window`] and the
    /// runtime's declared bounds *before* the session exists, so the record of
    /// what was asked for cannot be written after the fact.
    pub context_policy: ContextPolicySnapshot,
    /// When the launch was requested.
    pub requested_at: Timestamp,
}

/// A slot with exactly one live native session.
///
/// There is no `launch` here, and the type is not [`Clone`]. Continuing the work
/// means reusing the binding this handle already holds.
#[derive(Debug)]
pub struct OccupiedSlot {
    team_run_id: TeamRunId,
    slot: RoleSlotId,
    agent_run_id: AgentRunId,
    binding: Box<RuntimeBindingSnapshot>,
}

impl OccupiedSlot {
    /// The slot this session occupies.
    #[must_use]
    pub const fn slot(&self) -> &RoleSlotId {
        &self.slot
    }

    /// The run this session serves.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.agent_run_id
    }

    /// The one binding this slot currently owns.
    #[must_use]
    pub fn binding(&self) -> &RuntimeBindingSnapshot {
        &self.binding
    }

    /// Continue the *same* native session in place.
    #[must_use]
    pub fn resume_request(&self, requested_at: Timestamp) -> ResumeRequest {
        ResumeRequest {
            binding: self.binding.as_ref().clone(),
            requested_at,
        }
    }

    /// Deliver one message into the *same* native session.
    #[must_use]
    pub fn message_request(
        &self,
        message_id: MessageId,
        body: BoundedText,
        sent_at: Timestamp,
    ) -> SendMessageRequest {
        SendMessageRequest {
            binding: self.binding.as_ref().clone(),
            message_id,
            body,
            sent_at,
        }
    }
}

/// A slot whose session must close with evidence before it can be filled again.
///
/// Replacement is two-stage on purpose: this state retains the old binding and
/// offers no way to launch, so the next session cannot start before the old one
/// has actually finished.
#[derive(Debug)]
pub struct ReplacementPending {
    team_run_id: TeamRunId,
    slot: RoleSlotId,
    agent_run_id: AgentRunId,
    binding: Box<RuntimeBindingSnapshot>,
}

impl ReplacementPending {
    /// The slot awaiting replacement.
    #[must_use]
    pub const fn slot(&self) -> &RoleSlotId {
        &self.slot
    }

    /// The run that must close first.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.agent_run_id
    }

    /// The binding being retired. It is never rebound or reopened.
    #[must_use]
    pub fn binding(&self) -> &RuntimeBindingSnapshot {
        &self.binding
    }
}

/// A slot whose latest attempt closed with evidence.
///
/// The only thing it can mint is the next attempt, and that attempt carries this
/// run as its parent.
#[derive(Debug)]
pub struct ClosedSlot {
    team_run_id: TeamRunId,
    slot: RoleSlotId,
    agent_run_id: AgentRunId,
    retired_binding: Option<RuntimeBindingId>,
}

impl ClosedSlot {
    /// The slot that closed.
    #[must_use]
    pub const fn slot(&self) -> &RoleSlotId {
        &self.slot
    }

    /// The attempt that closed.
    #[must_use]
    pub const fn agent_run_id(&self) -> AgentRunId {
        self.agent_run_id
    }

    /// The session this attempt was holding when it closed, if it held one.
    #[must_use]
    pub const fn retired_binding(&self) -> Option<RuntimeBindingId> {
        self.retired_binding
    }
}

// ---------------------------------------------------------------------------
// Waivers and the closure certificate
// ---------------------------------------------------------------------------

/// An authorized excuse for one declared slot that never produced a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleSlotWaiver {
    /// The slot being excused.
    pub slot: RoleSlotId,
    /// The role that excused it. Checked against the slot's snapshotted policy.
    pub authorized_by: RoleKey,
    /// The evidence the waiver cites.
    pub evidence: Vec<ArtifactKey>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// Proof that every slot the pinned template declares is accounted for.
///
/// The only way to obtain one is [`TeamRunSlots::certify_team_closure`], so a
/// caller cannot assert that a team is finished. Its digest covers the template
/// revision, every declared slot, every lineage edge with its terminal evidence
/// and every waiver: changing or omitting any of them changes the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamClosureCertificate {
    team_run_id: TeamRunId,
    policy_digest: ContentHash,
    children: Vec<TeamChildEvidence>,
    outcome: TerminalOutcome,
    basis: TeamClosureBasis,
}

/// What a team's closure was proved from.
///
/// Separately typed on purpose. The two are not interchangeable readings of one
/// fact: one says the team's child *runs* ended, the other says Kontor's work in
/// every declared slot is finished while the seats holding it are expected to
/// still be live. A call site that could confuse them could close a team on
/// evidence about the wrong thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamClosureBasis {
    /// Every declared slot produced a terminal run.
    TerminalRuns,
    /// Every declared slot settled its final bounded Kontor turn. The native
    /// sessions may still be live, and normally are.
    SettledTurns,
    /// Every declared slot carries exactly one disposition: a settled turn, or
    /// an authorized waiver of a slot that was never bound.
    ///
    /// The basis a team needs when a declared slot never got a seat at all.
    /// [`Self::SettledTurns`] cannot speak for such a slot, because there is no
    /// turn to speak with.
    RoleSlotDispositions,
}

impl TeamClosureCertificate {
    /// The team this certificate closes.
    #[must_use]
    pub const fn team_run_id(&self) -> TeamRunId {
        self.team_run_id
    }

    /// What this certificate was proved from.
    #[must_use]
    pub const fn basis(&self) -> TeamClosureBasis {
        self.basis
    }

    /// The team closure envelope for a settled-turn certificate.
    ///
    /// Separate from [`TeamClosureCertificate::into_terminal_evidence`], which
    /// builds a child-evidence envelope and would be a lie here: there is no
    /// child terminal evidence, deliberately.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the certificate was not proved from
    /// settled turns. The two envelopes are not interchangeable.
    pub fn into_settled_turn_evidence(
        self,
        closed_at: Timestamp,
    ) -> DomainResult<TeamTerminalEvidence> {
        if self.basis != TeamClosureBasis::SettledTurns {
            return Err(DomainError::invalid(
                "team closure",
                "this certificate was not proved from settled turns",
            ));
        }
        Ok(TeamTerminalEvidence {
            outcome: self.outcome,
            source: TeamEvidenceSource::SettledTurns {
                team_run_id: self.team_run_id,
            },
            // The declared-slot policy digest *is* the evidence here: it covers
            // the template, every declared slot and the turn digest that
            // accounted for it, so the store can re-derive what was proved.
            evidence_hash: self.policy_digest,
            closed_at,
        })
    }

    /// Turn this certificate into the immutable evidence that closes the team,
    /// for a closure proved from role-slot dispositions.
    ///
    /// # Errors
    /// [`DomainError::Invalid`] when the certificate was proved some other way.
    /// The two are not interchangeable: they cite different rows and the store
    /// re-proves them differently.
    pub fn into_disposition_evidence(
        self,
        closed_at: Timestamp,
    ) -> DomainResult<TeamTerminalEvidence> {
        if self.basis != TeamClosureBasis::RoleSlotDispositions {
            return Err(DomainError::invalid(
                "team closure",
                "this certificate was not proved from role slot dispositions",
            ));
        }
        Ok(TeamTerminalEvidence {
            outcome: self.outcome,
            source: TeamEvidenceSource::RoleSlotDispositions {
                team_run_id: self.team_run_id,
            },
            evidence_hash: self.policy_digest,
            closed_at,
        })
    }

    /// The digest of the declared-slot policy this certificate proves.
    #[must_use]
    pub const fn policy_digest(&self) -> &ContentHash {
        &self.policy_digest
    }

    /// The exact child evidence the core outcome policy was reduced from.
    #[must_use]
    pub fn children(&self) -> &[TeamChildEvidence] {
        &self.children
    }

    /// The outcome the children compute, under the existing core policy.
    #[must_use]
    pub const fn outcome(&self) -> TerminalOutcome {
        self.outcome
    }

    /// The citation a terminal task transition presents for this team.
    ///
    /// The store re-proves the substance from its own rows; what this carries is
    /// *which* team run was certified and *what policy* was proved about it. The
    /// part only this crate can prove — that every slot the template declares is
    /// accounted for, including one that never produced a run — is what obtaining
    /// the certificate in the first place required.
    #[must_use]
    pub fn task_team_closure(&self) -> TaskTeamClosure {
        TaskTeamClosure::Certified {
            team_run_id: self.team_run_id,
            policy_digest: self.policy_digest.clone(),
        }
    }

    /// Convert into the KON-MVP-03 closure envelope.
    ///
    /// The digest is the existing core child-evidence digest, so the store
    /// recomputes it unchanged at commit; the declared-slot proof this crate
    /// adds is what the generic envelope cannot express on its own.
    ///
    /// # Errors
    /// Returns [`DomainError`] only when the children cannot be canonicalized.
    pub fn into_terminal_evidence(
        &self,
        closed_at: Timestamp,
    ) -> DomainResult<TeamTerminalEvidence> {
        Ok(TeamTerminalEvidence {
            outcome: self.outcome,
            source: TeamEvidenceSource::ChildEvidence {
                team_run_id: self.team_run_id,
            },
            evidence_hash: team_child_evidence_digest(&self.children)?,
            closed_at,
        })
    }
}

/// The exact shape the policy digest is computed over.
#[derive(Debug, Serialize)]
struct PolicyDigestInput<'a> {
    schema_version: SchemaVersion,
    team_run_id: TeamRunId,
    template_id: TeamTemplateId,
    template_version: SpecVersion,
    template_hash: &'a ContentHash,
    slots: Vec<SlotDigest<'a>>,
}

#[derive(Debug, Serialize)]
struct SettledPolicyDigestInput<'a> {
    schema_version: SchemaVersion,
    team_run_id: TeamRunId,
    template_id: TeamTemplateId,
    template_version: SpecVersion,
    template_hash: &'a ContentHash,
    slots: Vec<SettledSlotDigest<'a>>,
}

#[derive(Debug, Serialize)]
struct SettledSlotDigest<'a> {
    slot: &'a RoleSlotId,
    turn_evidence: Option<&'a ContentHash>,
    waiver: Option<&'a RoleSlotWaiver>,
}

#[derive(Debug, Serialize)]
struct SlotDigest<'a> {
    slot: &'a RoleSlotId,
    lineage: Vec<AttemptDigest<'a>>,
    waiver: Option<&'a RoleSlotWaiver>,
}

#[derive(Debug, Serialize)]
struct AttemptDigest<'a> {
    agent_run_id: AgentRunId,
    parent_agent_run_id: Option<AgentRunId>,
    lifecycle: RunLifecycle,
    evidence_hash: &'a ContentHash,
}

// ---------------------------------------------------------------------------
// The slot map
// ---------------------------------------------------------------------------

/// Every declared seat of one team run, and the only path that fills them.
#[derive(Debug)]
pub struct TeamRunSlots {
    /// Exclusive ownership of this team run. Held for the manager's whole life,
    /// so a second manager for the same team run cannot exist to disagree with
    /// this one about which seats are free.
    lease: TeamRunLease,
    template: TeamTemplateSpec,
    template_hash: ContentHash,
    /// The run's frozen context-window resolution inputs, copied from the team
    /// run snapshot so every seat resolves against the same data.
    context_policy: TeamContextPolicySeed,
    slots: BTreeMap<RoleSlotId, SlotState>,
}

impl TeamRunSlots {
    /// Open the seats of a team run that has not launched anything yet.
    ///
    /// # Errors
    /// As [`TeamTemplateSpec::from_snapshot`].
    pub fn open(lease: TeamRunLease, snapshot: &TeamRunSnapshot) -> DomainResult<Self> {
        Self::hydrate(lease, snapshot, &[], &[])
    }

    /// Rebuild the seats from what storage and the runtime actually hold.
    ///
    /// This is the restart backstop. It proves the same invariants the live
    /// transitions do — one root, one parent, one successor, one non-terminal
    /// leaf, one current binding, bounded depth — and fails closed rather than
    /// producing a roster it cannot vouch for.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] for a run of another team, a run naming an
    ///   undeclared slot, more than one non-terminal run or binding in a slot,
    ///   more than one root, a parent outside the slot, a branching parent, a
    ///   broken chain and a chain deeper than the template allows.
    /// * [`DomainError::MissingEvidence`] for a closed attempt with no evidence.
    pub fn hydrate(
        lease: TeamRunLease,
        snapshot: &TeamRunSnapshot,
        runs: &[AgentRun],
        bindings: &[RuntimeBindingSnapshot],
    ) -> DomainResult<Self> {
        let team_run_id = lease.team_run_id();
        let template = TeamTemplateSpec::from_snapshot(snapshot)?;
        let template_hash = snapshot.definition.hash().clone();
        let mut slots: BTreeMap<RoleSlotId, SlotState> = template
            .slots
            .iter()
            .map(|slot| (slot.id.clone(), SlotState::vacant()))
            .collect();

        let mut grouped: BTreeMap<RoleSlotId, Vec<&AgentRun>> = BTreeMap::new();
        for run in runs {
            if run.team_run_id != team_run_id {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "a run belongs to a different team run",
                ));
            }
            let slot = RoleSlotId::new(run.role.clone());
            if !slots.contains_key(&slot) {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "a run names a role slot the template does not declare",
                ));
            }
            grouped.entry(slot).or_default().push(run);
        }

        for (slot, attempts) in grouped {
            let state = Self::hydrate_slot(&attempts, bindings, template.max_successor_depth)?;
            slots.insert(slot, state);
        }

        Ok(Self {
            lease,
            template,
            template_hash,
            context_policy: snapshot.context_policy.clone(),
            slots,
        })
    }

    /// Resolve one seat's requested context-window policy.
    ///
    /// The role slot's own declaration and the run's frozen work-profile default
    /// and seed table come from this roster; only an explicit authorized
    /// override is supplied by the caller, because nothing in the team document
    /// can carry one.
    ///
    /// Resolution is a pure function of frozen inputs, so calling it again for
    /// the same seat always produces the same source and policy.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] for a slot the template does not declare.
    /// * As [`kontor_core::spec::resolve_context_window`].
    pub fn requested_context_window(
        &self,
        slot: &RoleSlotId,
        run_override: Option<&ContextWindowPolicy>,
    ) -> DomainResult<ResolvedContextPolicy> {
        let declared = self.template.slot(slot).ok_or(DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "the template does not declare this role slot",
        })?;
        self.context_policy_seed().resolve(
            &declared.role.role,
            declared.context_window.as_ref(),
            run_override,
        )
    }

    /// The run's frozen context-window resolution inputs.
    #[must_use]
    pub const fn context_policy_seed(&self) -> &TeamContextPolicySeed {
        &self.context_policy
    }

    fn hydrate_slot(
        attempts: &[&AgentRun],
        bindings: &[RuntimeBindingSnapshot],
        max_successor_depth: u32,
    ) -> DomainResult<SlotState> {
        // The AC-4 conflict, decided before anything else: two live sessions in
        // one seat is exactly what a restart after a lost acknowledgement looks
        // like, and it must never yield a launch permit.
        let live = attempts
            .iter()
            .filter(|run| !run.projection.lifecycle.is_terminal())
            .count();
        if live > 1 {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "a role slot has more than one non-terminal run",
            ));
        }

        let ids: BTreeSet<AgentRunId> = attempts.iter().map(|run| run.id).collect();
        if ids.len() != attempts.len() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "a role slot records the same run twice",
            ));
        }

        let mut roots = attempts
            .iter()
            .filter(|run| run.parent_agent_run_id.is_none());
        let root = roots.next().ok_or(DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "a role slot with attempts has no root attempt",
        })?;
        if roots.next().is_some() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "a role slot has more than one root attempt",
            ));
        }

        let mut successor: BTreeMap<AgentRunId, &AgentRun> = BTreeMap::new();
        for run in attempts {
            let Some(parent) = run.parent_agent_run_id else {
                continue;
            };
            if !ids.contains(&parent) {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "a successor names a parent outside its own role slot",
                ));
            }
            if successor.insert(parent, run).is_some() {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "an attempt has more than one successor",
                ));
            }
        }

        let mut chain: Vec<&AgentRun> = vec![root];
        while let Some(next) = successor.get(&chain[chain.len() - 1].id) {
            chain.push(next);
            if chain.len() > attempts.len() {
                break;
            }
        }
        if chain.len() != attempts.len() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the successor chain does not cover every attempt of the slot",
            ));
        }
        if u32::try_from(chain.len() - 1).unwrap_or(u32::MAX) > max_successor_depth {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the successor chain is deeper than the template allows",
            ));
        }

        let (leaf, closed) = chain.split_last().expect("the chain has at least the root");
        let mut lineage = Vec::with_capacity(closed.len());
        for run in closed {
            lineage.push(Self::closed_attempt(run)?);
        }

        let head = if leaf.projection.lifecycle.is_terminal() {
            lineage.push(Self::closed_attempt(leaf)?);
            SlotHead::Vacant
        } else {
            let mut current = bindings
                .iter()
                .filter(|snapshot| snapshot.agent_run_id() == leaf.id);
            match (current.next(), current.next()) {
                (Some(_), Some(_)) => {
                    return Err(DomainError::invalid(
                        "TeamRunSlots",
                        "a role slot has more than one current binding",
                    ));
                }
                (Some(snapshot), None) => {
                    snapshot.ensure_correlated().map_err(|_| {
                        DomainError::invalid(
                            "TeamRunSlots",
                            "a current binding is not correlated with its own run",
                        )
                    })?;
                    if let Some(stored) = &leaf.binding
                        && stored.id != snapshot.binding_id()
                    {
                        return Err(DomainError::invalid(
                            "TeamRunSlots",
                            "the stored binding and the runtime binding disagree",
                        ));
                    }
                    SlotHead::Occupied {
                        agent_run_id: leaf.id,
                        parent: leaf.parent_agent_run_id,
                        binding: Box::new(snapshot.clone()),
                    }
                }
                (None, _) => SlotHead::Reserved {
                    agent_run_id: leaf.id,
                    parent: leaf.parent_agent_run_id,
                },
            }
        };

        Ok(SlotState { lineage, head })
    }

    fn closed_attempt(run: &AgentRun) -> DomainResult<ClosedAttempt> {
        if !run.projection.lifecycle.is_terminal() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "an attempt other than the leaf is still open",
            ));
        }
        let evidence = run.terminal.as_ref().ok_or(DomainError::MissingEvidence {
            subject: "role slot lineage",
            rule: "a closed attempt must carry its closure evidence",
        })?;
        if run.closed_at.is_none() {
            return Err(DomainError::MissingEvidence {
                subject: "role slot lineage",
                rule: "a closed attempt must record when it closed",
            });
        }
        Ok(ClosedAttempt {
            agent_run_id: run.id,
            parent: run.parent_agent_run_id,
            lifecycle: run.projection.lifecycle,
            evidence_hash: evidence.evidence_hash.clone(),
            binding_id: run.binding.as_ref().map(|binding| binding.id),
        })
    }

    /// The pinned template these seats come from.
    #[must_use]
    pub const fn template(&self) -> &TeamTemplateSpec {
        &self.template
    }

    /// The team run these seats belong to.
    #[must_use]
    pub const fn team_run_id(&self) -> TeamRunId {
        self.lease.team_run_id()
    }

    /// The run currently live at a slot, if any.
    #[must_use]
    pub fn live_run(&self, slot: &RoleSlotId) -> Option<AgentRunId> {
        self.slots.get(slot).and_then(|state| state.head.live_run())
    }

    /// The binding a slot currently holds, whether it is occupied or waiting
    /// for its session to close.
    ///
    /// A slot being replaced keeps answering with the *old* binding: the new
    /// session does not exist until the old one closed, so there is never a
    /// moment where a slot reports two.
    #[must_use]
    pub fn current_binding(&self, slot: &RoleSlotId) -> Option<&RuntimeBindingSnapshot> {
        match &self.slots.get(slot)?.head {
            SlotHead::Occupied { binding, .. } | SlotHead::Replacing { binding, .. } => {
                Some(binding)
            }
            SlotHead::Vacant | SlotHead::Reserved { .. } => None,
        }
    }

    /// How many attempts a slot has recorded, closed and live together.
    #[must_use]
    pub fn attempt_count(&self, slot: &RoleSlotId) -> usize {
        self.slots.get(slot).map_or(0, |state| {
            state.lineage.len() + usize::from(state.head.live_run().is_some())
        })
    }

    /// Recover the token for a slot whose latest durable attempt is closed.
    ///
    /// Hydration has already proved the lineage is a single evidenced chain.
    /// Reconstructing this token is therefore the restart-safe equivalent of
    /// [`TeamRunSlots::close_completed`], and is the only supported way an
    /// operator reconciliation may reserve a successor after process loss.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the slot is undeclared, still live, or has
    /// never recorded a closed attempt.
    pub fn latest_closed(&self, slot: &RoleSlotId) -> DomainResult<ClosedSlot> {
        let state = self.state(slot)?;
        if state.head.live_run().is_some() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the role slot still has a live attempt",
            ));
        }
        let latest = state.lineage.last().ok_or(DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "the role slot has no closed attempt to succeed",
        })?;
        Ok(ClosedSlot {
            team_run_id: self.team_run_id(),
            slot: slot.clone(),
            agent_run_id: latest.agent_run_id,
            retired_binding: latest.binding_id,
        })
    }

    /// Take the handle for a slot that a hydration found already occupied.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the slot is not declared or is not occupied.
    pub fn occupied(&self, slot: &RoleSlotId) -> DomainResult<OccupiedSlot> {
        match self.state(slot)?.head {
            SlotHead::Occupied {
                agent_run_id,
                ref binding,
                ..
            } => Ok(OccupiedSlot {
                team_run_id: self.team_run_id(),
                slot: slot.clone(),
                agent_run_id,
                binding: binding.clone(),
            }),
            _ => Err(DomainError::invalid(
                "TeamRunSlots",
                "the role slot has no live native session",
            )),
        }
    }

    fn state(&self, slot: &RoleSlotId) -> DomainResult<&SlotState> {
        self.slots.get(slot).ok_or(DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "the template does not declare this role slot",
        })
    }

    fn state_mut(&mut self, slot: &RoleSlotId) -> DomainResult<&mut SlotState> {
        self.slots.get_mut(slot).ok_or(DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "the template does not declare this role slot",
        })
    }

    /// Reserve the *first* attempt at a declared slot.
    ///
    /// Only a slot that has never run can be reserved this way; a slot that has
    /// already closed an attempt is refilled through
    /// [`TeamRunSlots::reserve_successor`], which is what forces the parent link
    /// onto every replacement.
    ///
    /// # Errors
    /// Returns [`DomainError`] for an undeclared slot, a slot that already has a
    /// live attempt, and a slot with closed attempts.
    pub fn reserve(
        &mut self,
        slot: &RoleSlotId,
        agent_run_id: AgentRunId,
    ) -> DomainResult<LaunchPermit> {
        let team_run_id = self.team_run_id();
        let state = self.state_mut(slot)?;
        if state.head.live_run().is_some() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the role slot already has a live attempt",
            ));
        }
        if !state.lineage.is_empty() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "a slot that has already run is refilled only as a successor",
            ));
        }
        state.head = SlotHead::Reserved {
            agent_run_id,
            parent: None,
        };
        Ok(LaunchPermit {
            team_run_id,
            slot: slot.clone(),
            agent_run_id,
            parent: None,
            replaces: None,
        })
    }

    /// Reserve the next attempt at a slot whose previous attempt closed.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the token does not match the slot's recorded
    /// state, when the slot is live again, or when the chain would exceed the
    /// template's declared successor depth.
    pub fn reserve_successor(
        &mut self,
        closed: ClosedSlot,
        agent_run_id: AgentRunId,
    ) -> DomainResult<LaunchPermit> {
        let team_run_id = self.team_run_id();
        let max_depth = self.template.max_successor_depth;
        let state = self.state_mut(&closed.slot)?;
        if closed.team_run_id != team_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the token belongs to a different team run",
            ));
        }
        if state.head.live_run().is_some() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the role slot already has a live attempt",
            ));
        }
        let last = state.lineage.last().ok_or(DomainError::Invalid {
            subject: "TeamRunSlots",
            rule: "the role slot has no closed attempt to succeed",
        })?;
        if last.agent_run_id != closed.agent_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the token does not name the slot's latest closed attempt",
            ));
        }
        if u32::try_from(state.lineage.len()).unwrap_or(u32::MAX) > max_depth {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the successor chain is deeper than the template allows",
            ));
        }
        let parent = Some(closed.agent_run_id);
        // The citation the runtime will check its own seat against: the exact
        // binding that closed, the run that held it, and this attempt as its
        // recorded successor. An attempt that never held a session has nothing
        // to cite, and asks for a seat that must be genuinely free.
        let replaces = closed.retired_binding.map(|binding_id| ReplacedBinding {
            binding_id,
            agent_run_id: closed.agent_run_id,
            successor_agent_run_id: agent_run_id,
        });
        state.head = SlotHead::Reserved {
            agent_run_id,
            parent,
        };
        Ok(LaunchPermit {
            team_run_id,
            slot: closed.slot,
            agent_run_id,
            parent,
            replaces,
        })
    }

    /// Record the native session a prepared launch produced.
    ///
    /// This consumes the spent permit, so the round trip vacant -> permit ->
    /// request -> session admits exactly one session per seat.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the slot is not reserved for this permit,
    /// when the binding belongs to another run, or when it is not the session
    /// the prepared request asked for.
    pub fn bind(
        &mut self,
        prepared: PreparedLaunch,
        binding: &RuntimeBindingSnapshot,
    ) -> DomainResult<OccupiedSlot> {
        if binding.binding_id() != prepared.request.binding_id() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the binding is not the one this launch request asked for",
            ));
        }
        let permit = prepared.permit;
        let team_run_id = self.team_run_id();
        if permit.team_run_id != team_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the token belongs to a different team run",
            ));
        }
        if binding.agent_run_id() != permit.agent_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the binding belongs to a different run than the permit",
            ));
        }
        binding.ensure_correlated().map_err(|_| {
            DomainError::invalid(
                "TeamRunSlots",
                "the binding is not correlated with its own run",
            )
        })?;
        let state = self.state_mut(&permit.slot)?;
        match state.head {
            SlotHead::Reserved { agent_run_id, .. } if agent_run_id == permit.agent_run_id => {}
            _ => {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "the role slot is not reserved for this permit",
                ));
            }
        }
        let parent = state.head.parent();
        state.head = SlotHead::Occupied {
            agent_run_id: permit.agent_run_id,
            parent,
            binding: Box::new(binding.clone()),
        };
        Ok(OccupiedSlot {
            team_run_id,
            slot: permit.slot,
            agent_run_id: permit.agent_run_id,
            binding: Box::new(binding.clone()),
        })
    }

    /// Begin replacing the session at an occupied slot.
    ///
    /// The old binding is retained and no launch is reachable until the old run
    /// closes with evidence.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the token does not match the slot's state.
    pub fn begin_replacement(
        &mut self,
        occupied: OccupiedSlot,
    ) -> DomainResult<ReplacementPending> {
        let team_run_id = self.team_run_id();
        if occupied.team_run_id != team_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the token belongs to a different team run",
            ));
        }
        let state = self.state_mut(&occupied.slot)?;
        match state.head {
            SlotHead::Occupied { agent_run_id, .. } if agent_run_id == occupied.agent_run_id => {}
            _ => {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "the role slot is not occupied by this session",
                ));
            }
        }
        let parent = state.head.parent();
        state.head = SlotHead::Replacing {
            agent_run_id: occupied.agent_run_id,
            parent,
            binding: occupied.binding.clone(),
        };
        Ok(ReplacementPending {
            team_run_id,
            slot: occupied.slot,
            agent_run_id: occupied.agent_run_id,
            binding: occupied.binding,
        })
    }

    /// Close the session a replacement is waiting on.
    ///
    /// # Errors
    /// As [`TeamRunSlots::close_completed`].
    pub fn close_replaced(
        &mut self,
        pending: ReplacementPending,
        run: &AgentRun,
    ) -> DomainResult<ClosedSlot> {
        self.record_close(pending.team_run_id, pending.slot, pending.agent_run_id, run)
    }

    /// Close an occupied slot that finished its work.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the token belongs to another team, when the
    /// closing run is not this slot's live attempt, when it names another slot
    /// or parent, or when it is not terminal with evidence.
    pub fn close_completed(
        &mut self,
        occupied: OccupiedSlot,
        run: &AgentRun,
    ) -> DomainResult<ClosedSlot> {
        self.record_close(
            occupied.team_run_id,
            occupied.slot,
            occupied.agent_run_id,
            run,
        )
    }

    fn record_close(
        &mut self,
        token_team_run_id: TeamRunId,
        slot: RoleSlotId,
        agent_run_id: AgentRunId,
        run: &AgentRun,
    ) -> DomainResult<ClosedSlot> {
        let team_run_id = self.team_run_id();
        if token_team_run_id != team_run_id || run.team_run_id != team_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the closing run belongs to a different team run",
            ));
        }
        if run.id != agent_run_id {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the closing run is not the slot's live attempt",
            ));
        }
        if RoleSlotId::new(run.role.clone()) != slot {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the closing run names a different role slot",
            ));
        }
        let attempt = Self::closed_attempt(run)?;
        let state = self.state_mut(&slot)?;
        if state.head.live_run() != Some(agent_run_id) {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the role slot is not live under this token",
            ));
        }
        if attempt.parent != state.head.parent() {
            return Err(DomainError::invalid(
                "TeamRunSlots",
                "the closing run has a different parent than the slot recorded",
            ));
        }
        // The run must be closing the session this seat is actually holding.
        // Without this, a row carrying some *other* run's binding — or none at
        // all — would retire the seat while the session it was retiring stayed
        // live, which is precisely the "one live session per seat" guarantee
        // failing quietly instead of loudly.
        match (state.head.binding(), run.binding.as_ref()) {
            (None, None) => {}
            (Some(held), Some(closing))
                if closing.id == held.binding_id()
                    && &closing.identity == held.identity()
                    && closing.agent_run_id == run.id => {}
            _ => {
                return Err(DomainError::invalid(
                    "TeamRunSlots",
                    "the closing run does not carry the session the slot is retiring",
                ));
            }
        }
        let retired_binding = attempt.binding_id;
        state.lineage.push(attempt);
        state.head = SlotHead::Vacant;
        Ok(ClosedSlot {
            team_run_id,
            slot,
            agent_run_id,
            retired_binding,
        })
    }

    /// Validate a waiver set against the template, and index it by slot.
    ///
    /// Shared by both certifiers on purpose: the rules about *who* may waive a
    /// slot and *what* they must cite are properties of the template, and do not
    /// change with what the closure is otherwise proved from.
    fn validated_waivers<'w>(
        &self,
        waivers: &'w [RoleSlotWaiver],
    ) -> DomainResult<BTreeMap<&'w RoleSlotId, &'w RoleSlotWaiver>> {
        let mut by_slot: BTreeMap<&RoleSlotId, &RoleSlotWaiver> = BTreeMap::new();
        for waiver in waivers {
            let declared = self
                .template
                .slot(&waiver.slot)
                .ok_or(DomainError::Invalid {
                    subject: "team closure",
                    rule: "a waiver names a role slot the template does not declare",
                })?;
            if by_slot.insert(&waiver.slot, waiver).is_some() {
                return Err(DomainError::invalid(
                    "team closure",
                    "a role slot is waived more than once",
                ));
            }
            let policy = declared
                .waiver_policy
                .as_ref()
                .ok_or(DomainError::MissingAuthority {
                    subject: "team closure",
                    rule: "the template does not allow this role slot to be waived",
                })?;
            if !policy.authorized_roles.contains(&waiver.authorized_by) {
                return Err(DomainError::MissingAuthority {
                    subject: "team closure",
                    rule: "the waiving role is not authorized for this role slot",
                });
            }
            let cited: BTreeSet<&ArtifactKey> = waiver.evidence.iter().collect();
            if !policy
                .required_evidence
                .iter()
                .all(|required| cited.contains(required))
            {
                return Err(DomainError::MissingEvidence {
                    subject: "team closure",
                    rule: "a waiver must cite every evidence reference the slot requires",
                });
            }
        }
        Ok(by_slot)
    }

    /// Certify that every slot the pinned template declares is accounted for.
    ///
    /// Every declared slot must either have a lineage whose leaf closed with
    /// evidence, or an authorized, evidence-bearing waiver. The declared-slot
    /// and waiver proof runs *before* the outcome is reduced, and the reduction
    /// itself stays the existing core policy — this crate does not invent a
    /// second one.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] for a slot with no attempts and no
    ///   waiver, and for a slot still live.
    /// * [`DomainError::MissingAuthority`] for a waiver on a slot the template
    ///   does not allow waiving, or by a role it does not authorize.
    /// * [`DomainError::Invalid`] for a waiver naming an undeclared or repeated
    ///   slot.
    /// * Whatever [`reduce_team_outcome`] returns for the collected children.
    pub fn certify_team_closure(
        &self,
        waivers: &[RoleSlotWaiver],
    ) -> DomainResult<TeamClosureCertificate> {
        let by_slot = self.validated_waivers(waivers)?;

        let mut children: Vec<TeamChildEvidence> = Vec::new();
        let mut digest_slots: Vec<SlotDigest<'_>> = Vec::new();
        // Walking the *template's* declared slots — not the runs that happen to
        // exist — is what makes an omitted seat fail instead of pass silently.
        for declared in &self.template.slots {
            let state = self.state(&declared.id)?;
            if state.head.live_run().is_some() {
                return Err(DomainError::MissingEvidence {
                    subject: "team closure",
                    rule: "a role slot still has a live attempt",
                });
            }
            let waiver = by_slot.get(&declared.id).copied();
            if state.lineage.is_empty() && waiver.is_none() {
                return Err(DomainError::MissingEvidence {
                    subject: "team closure",
                    rule: "a declared role slot produced no terminal run and was not waived",
                });
            }
            for attempt in &state.lineage {
                children.push(TeamChildEvidence {
                    agent_run_id: attempt.agent_run_id,
                    lifecycle: attempt.lifecycle,
                    evidence_hash: Some(attempt.evidence_hash.clone()),
                });
            }
            digest_slots.push(SlotDigest {
                slot: &declared.id,
                lineage: state
                    .lineage
                    .iter()
                    .map(|attempt| AttemptDigest {
                        agent_run_id: attempt.agent_run_id,
                        parent_agent_run_id: attempt.parent,
                        lifecycle: attempt.lifecycle,
                        evidence_hash: &attempt.evidence_hash,
                    })
                    .collect(),
                waiver,
            });
        }

        let lifecycles: Vec<RunLifecycle> = children.iter().map(|child| child.lifecycle).collect();
        let outcome = reduce_team_outcome(&lifecycles)?;

        let policy_digest = CanonicalDocument::from_serializable(&PolicyDigestInput {
            schema_version: kontor_core::id::SCHEMA_VERSION,
            team_run_id: self.team_run_id(),
            template_id: self.template.template_id,
            template_version: self.template.version,
            template_hash: &self.template_hash,
            slots: digest_slots,
        })?
        .hash()
        .clone();

        Ok(TeamClosureCertificate {
            team_run_id: self.team_run_id(),
            policy_digest,
            children,
            outcome,
            basis: TeamClosureBasis::TerminalRuns,
        })
    }

    /// Certify closure because every declared slot settled its final turn.
    ///
    /// The same walk over the template's *declared* slots as
    /// [`TeamRunSlots::certify_team_closure`], and deliberately not the same
    /// admissibility rule. A slot is accounted for by `accounted` — the caller's
    /// read of this team's immutable role-turn rows — rather than by a terminal
    /// run, and **a live run is not disqualifying**: a persistent seat outliving
    /// the work taken in it is the normal case this exists for.
    ///
    /// What is unchanged: an *unaccounted* declared slot still fails, and it
    /// fails whether or not a run exists for it, because the template is what
    /// says which seats must be answered for.
    ///
    /// The outcome is not reduced through
    /// [`kontor_core::state::reduce_team_outcome`], which requires every child
    /// terminal and would refuse every team this path exists to close. It is
    /// `Succeeded` because every declared slot finished its turn — which is the
    /// only thing this certificate claims, and is a statement about Kontor's
    /// work rather than about any runtime's verdict.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] when a declared slot is neither
    ///   accounted for by a settled turn nor waived.
    /// * As [`TeamRunSlots::certify_team_closure`] for waiver validation.
    pub fn certify_from_settled_turns(
        &self,
        accounted: &BTreeMap<RoleSlotId, ContentHash>,
        waivers: &[RoleSlotWaiver],
    ) -> DomainResult<TeamClosureCertificate> {
        let by_slot = self.validated_waivers(waivers)?;
        let mut digest_slots: Vec<SettledSlotDigest<'_>> = Vec::new();
        for declared in &self.template.slots {
            let settled = accounted.get(&declared.id);
            let waiver = by_slot.get(&declared.id).copied();
            if settled.is_none() && waiver.is_none() {
                return Err(DomainError::MissingEvidence {
                    subject: "team closure",
                    rule: "a declared role slot settled no final turn and was not waived",
                });
            }
            digest_slots.push(SettledSlotDigest {
                slot: &declared.id,
                turn_evidence: settled,
                waiver,
            });
        }
        let policy_digest = CanonicalDocument::from_serializable(&SettledPolicyDigestInput {
            schema_version: kontor_core::id::SCHEMA_VERSION,
            team_run_id: self.team_run_id(),
            template_id: self.template.template_id,
            template_version: self.template.version,
            template_hash: &self.template_hash,
            slots: digest_slots,
        })?
        .hash()
        .clone();
        Ok(TeamClosureCertificate {
            team_run_id: self.team_run_id(),
            policy_digest,
            // No child evidence is cited: the children are expected to be live,
            // and citing a live run as closure evidence is exactly the confusion
            // the separate basis exists to prevent.
            children: Vec::new(),
            outcome: TerminalOutcome::Succeeded,
            basis: TeamClosureBasis::SettledTurns,
        })
    }

    /// Certify a team whose declared slots are each accounted for by exactly one
    /// disposition — a settled turn, or an authorized waiver of a slot that was
    /// never bound.
    ///
    /// The rule this enforces, and that nothing else can, is *exactly one*. A
    /// slot with both a settled turn and a waiver is a contradiction: the seat
    /// did work and was simultaneously excused for never existing. A slot with
    /// neither is the gap the whole design exists to name. Both are refused
    /// here, before a certificate exists to be acted on.
    ///
    /// A `WaivedUnbound` disposition is only honoured when a *validated* waiver
    /// backs it, so an unauthorized or absent waiver never reaches certificate
    /// construction — the caller cannot hand in a disposition that excuses a slot
    /// the template never allowed excusing.
    ///
    /// The outcome is `Succeeded`: an authorized waiver is the template's own
    /// statement that the slot's obligation is discharged, so a team all of whose
    /// slots are disposed of has nothing outstanding. As with
    /// [`TeamRunSlots::certify_from_settled_turns`], no child evidence is cited
    /// and the children are expected to still be live.
    ///
    /// # Errors
    /// * [`DomainError::MissingEvidence`] when a declared slot has no
    ///   disposition, or a `WaivedUnbound` disposition has no validated waiver.
    /// * [`DomainError::Invalid`] when a slot carries both sources, when a
    ///   disposition names an undeclared slot, or when a waiver is recorded for
    ///   a slot whose disposition is a settled turn.
    /// * As [`TeamRunSlots::certify_team_closure`] for waiver validation.
    pub fn certify_from_dispositions(
        &self,
        dispositions: &BTreeMap<RoleSlotId, SlotDisposition>,
        waivers: &[RoleSlotWaiver],
    ) -> DomainResult<TeamClosureCertificate> {
        let by_slot = self.validated_waivers(waivers)?;
        for slot in dispositions.keys() {
            if self.template.slot(slot).is_none() {
                return Err(DomainError::invalid(
                    "team closure",
                    "a disposition names a role slot the template does not declare",
                ));
            }
        }
        let mut digest_slots: Vec<(RoleSlotId, SlotDisposition)> = Vec::new();
        for declared in &self.template.slots {
            let waiver = by_slot.get(&declared.id).copied();
            let disposition =
                dispositions
                    .get(&declared.id)
                    .ok_or(DomainError::MissingEvidence {
                        subject: "team closure",
                        rule: "a declared role slot is neither settled nor waived",
                    })?;
            match disposition {
                SlotDisposition::SettledTurn { .. } => {
                    if waiver.is_some() {
                        return Err(DomainError::invalid(
                            "team closure",
                            "a role slot that settled a turn is also waived",
                        ));
                    }
                }
                SlotDisposition::WaivedUnbound { .. } => {
                    // The disposition alone proves nothing. Only a waiver that
                    // passed the template's own authority and evidence rules can
                    // support one, which is what keeps an unauthorized excuse
                    // out of a certificate.
                    if waiver.is_none() {
                        return Err(DomainError::MissingEvidence {
                            subject: "team closure",
                            rule: "a waived role slot has no authorized waiver",
                        });
                    }
                }
            }
            digest_slots.push((declared.id.clone(), disposition.clone()));
        }
        // One canonical form, computed by `kontor-core` for both the certificate
        // and the store's re-proof. Two implementations would be the defect.
        let policy_digest = kontor_core::state::role_slot_disposition_digest(
            kontor_core::id::SCHEMA_VERSION,
            self.team_run_id(),
            self.template.template_id,
            self.template.version,
            &self.template_hash,
            &digest_slots,
        )?;
        Ok(TeamClosureCertificate {
            team_run_id: self.team_run_id(),
            policy_digest,
            children: Vec::new(),
            outcome: TerminalOutcome::Succeeded,
            basis: TeamClosureBasis::RoleSlotDispositions,
        })
    }
}
