//! Policy inputs, verdicts and evidence records.
//!
//! Everything here is data. The types name *what a rule is allowed to look at*
//! and *what it may say*, and nothing in this module — or in the two that use it
//! — ever branches on a profile id, a phase name, a gate name, a role name or a
//! persona name. Those are deployment data: a rule reads them out of the pinned
//! snapshot it was handed and compares them with each other, which is why an
//! arbitrary custom profile behaves exactly like a bundled one.
//!
//! The identifiers a rule *does* branch on are Kontor's own closed vocabularies
//! — [`GuardrailRuleKey`], [`ActionEffect`], [`AuthoritySource`] — declared with
//! the same [`kontor_core::closed_enum`] macro as the rest of the domain, so an
//! unknown spelling arriving from SQL or JSON is refused rather than defaulted.

use std::fmt;

use kontor_core::closed_enum;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CanonicalDocument, ContentHash,
    EventCursor, ExternalId, ExternalName, GateKey, GuardrailEvaluationId, ModuleKey, PersonaKey,
    PhaseKey, ProjectId, RoleKey, SchemaVersion, SpecVersion, TaskId, TaskWorkflowId, TeamRunId,
    Timestamp,
};
use kontor_core::repository::GateEvaluation;
use kontor_core::spec::ResolvedWorkProfileSnapshot;
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Declare a UUIDv7 identifier owned by this crate.
///
/// The domain's own entity ids live in [`kontor_core::id`]; these are the ones
/// KON-MVP-10 introduces, and they follow the same rules — version 7, canonical
/// lowercase text, parsed rather than trusted.
macro_rules! policy_ids {
    ($( $(#[$meta:meta])* $name:ident ),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub struct $name(Uuid);

            impl $name {
                /// Mint a new time-ordered identifier.
                #[must_use]
                pub fn generate() -> Self {
                    Self(kontor_core::id::generate_uuid_v7())
                }

                /// Parse a stored identifier.
                ///
                /// # Errors
                /// Rejects anything that is not a canonical version 7 UUID.
                pub fn parse(text: &str) -> DomainResult<Self> {
                    let parsed = Uuid::parse_str(text)
                        .map_err(|_| DomainError::invalid(stringify!($name), "is not a UUID"))?;
                    if parsed.get_version_num() != 7 {
                        return Err(DomainError::invalid(
                            stringify!($name),
                            "is not a version 7 UUID",
                        ));
                    }
                    if parsed.hyphenated().to_string() != text {
                        return Err(DomainError::invalid(
                            stringify!($name),
                            "is not in canonical lowercase hyphenated form",
                        ));
                    }
                    Ok(Self(parsed))
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "{}", self.0.hyphenated())
                }
            }

            impl Serialize for $name {
                fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    s.serialize_str(&self.to_string())
                }
            }

            impl<'de> Deserialize<'de> for $name {
                fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    use ::serde::de::Error as _;
                    let text = <String as Deserialize>::deserialize(d)?;
                    Self::parse(&text).map_err(D::Error::custom)
                }
            }
        )+
    };
}

policy_ids! {
    /// Identifies one stored artifact-evidence record.
    ArtifactEvidenceId,
    /// Identifies one explicit gate-waiver authority receipt.
    GateWaiverId,
    /// Identifies one approval bound to one exact destructive action.
    ApprovalReceiptId,
    /// Identifies one bounded recovery episode for one parked run.
    RecoveryEpisodeId,
}

closed_enum! {
    /// The seven architecture rules this crate evaluates.
    ///
    /// The set is closed because each key names a distinct evaluator with its
    /// own inputs and its own reason codes; a deployment adds work profiles, not
    /// guardrail rules.
    GuardrailRuleKey, "GuardrailRuleKey" {
        /// A run stays in the worktree it was first recorded in.
        WorktreeSticky => "worktree_sticky",
        /// Two tasks do not hold the same module without worktree isolation.
        ModuleCollision => "module_collision",
        /// The second rejection by one reviewer on one gate parks the work.
        SecondRejectionParks => "second_rejection_parks",
        /// Degraded evidence cannot write a gate verdict.
        DegradedVerdictDenied => "degraded_verdict_denied",
        /// A destructive action needs an approval bound to that exact action.
        DestructiveRequiresApproval => "destructive_requires_approval",
        /// A run acts as the account it was pinned to, or not at all.
        AccountPinRequired => "account_pin_required",
        /// Closing a run requires the terminal evidence of that run.
        TerminalEvidenceRequired => "terminal_evidence_required",
    }
}

closed_enum! {
    /// What a guardrail evaluation concluded.
    ///
    /// The two refusals are not interchangeable. [`PolicyVerdict::Block`] means
    /// *this action* may not happen and the work continues; [`PolicyVerdict::Park`]
    /// means the work itself stops and recovery takes over. Nothing infers one
    /// from the other.
    PolicyVerdict, "PolicyVerdict" {
        /// Admitted.
        Pass => "pass",
        /// Admitted, with a recorded concern.
        Warn => "warn",
        /// Refused. Nothing is dispatched.
        Block => "block",
        /// Refused, and the work is parked for bounded recovery.
        Park => "park",
        /// Refused, and only a human can decide.
        NeedsHuman => "needs_human",
    }
}

closed_enum! {
    /// Why a guardrail evaluation reached its verdict.
    ///
    /// One code per distinguishable outcome, so an audit reads the reason
    /// without re-running the rule, and a test asserts the *reason* rather than
    /// only the verdict — which is what stops a rule from being right by
    /// accident.
    ReasonCode, "ReasonCode" {
        /// The rule does not apply to this request.
        NotApplicable => "not_applicable",

        // worktree_sticky
        /// No worktree was pinned yet and exactly one candidate was offered.
        WorktreeFirstClaim => "worktree_first_claim",
        /// The claimed worktree is the one already recorded.
        WorktreeMatchesPin => "worktree_matches_pin",
        /// The claimed worktree is not the one already recorded.
        WorktreeMoved => "worktree_moved",
        /// More than one worktree could be meant, or none could.
        WorktreeAmbiguous => "worktree_ambiguous",
        /// The run claims no worktree at all.
        WorktreeUnclaimed => "worktree_unclaimed",

        // module_collision
        /// No other task holds this module.
        ModuleFree => "module_free",
        /// Another task holds it, in a different worktree.
        ModuleIsolatedByWorktree => "module_isolated_by_worktree",
        /// Another task holds it, in the same worktree.
        ModuleInFlight => "module_in_flight",

        // second_rejection_parks
        /// This reviewer has no open rejection on this gate.
        NoRejectionRecorded => "no_rejection_recorded",
        /// This reviewer's first rejection on this gate.
        FirstRejection => "first_rejection",
        /// This reviewer's second rejection on this gate.
        SecondRejectionParks => "second_rejection_parks",

        // degraded_verdict_denied
        /// The actor may record this verdict.
        VerdictAuthorityHeld => "verdict_authority_held",
        /// The actor's evidence rung is below the verdict threshold.
        VerdictRungDegraded => "verdict_rung_degraded",
        /// The pinned profile does not authorize this role over this gate.
        RoleNotAuthorized => "role_not_authorized",
        /// A simulated persona cannot record a gate verdict.
        PersonaCannotEvaluate => "persona_cannot_evaluate",
        /// A simulated persona cannot decide the gate it is under test for.
        PersonaSelfApproval => "persona_self_approval",

        // destructive_requires_approval
        /// The action changes nothing that needs approving.
        ActionNonDestructive => "action_non_destructive",
        /// The action was requested as a dry run.
        ActionDryRun => "action_dry_run",
        /// The approval matches the action, the scope and the actor.
        ApprovalBound => "approval_bound",
        /// No approval was presented.
        ApprovalMissing => "approval_missing",
        /// The approval had expired.
        ApprovalExpired => "approval_expired",
        /// The approval had already been consumed.
        ApprovalConsumed => "approval_consumed",
        /// The approval names a different action.
        ApprovalActionMismatch => "approval_action_mismatch",
        /// The approval names a different project or task.
        ApprovalScopeMismatch => "approval_scope_mismatch",
        /// The approval was issued by recovery advice, which cannot approve.
        ApprovalFromRecoveryAdvice => "approval_from_recovery_advice",

        // account_pin_required
        /// The actor is the account the run is pinned to.
        AccountPinMatches => "account_pin_matches",
        /// The run records no pinned account.
        AccountPinMissing => "account_pin_missing",
        /// The actor is not the account the run is pinned to.
        AccountPinMismatch => "account_pin_mismatch",

        // terminal_evidence_required
        /// The cited terminal observation belongs to this run.
        TerminalEvidencePresent => "terminal_evidence_present",
        /// No terminal observation was cited.
        TerminalEvidenceMissing => "terminal_evidence_missing",
        /// The cited terminal observation belongs to another run.
        TerminalEvidenceForeign => "terminal_evidence_foreign",
        /// Every artifact the phase declares has been produced.
        ArtifactEvidenceComplete => "artifact_evidence_complete",
        /// The phase declares an artifact that has not been produced.
        ArtifactEvidenceIncomplete => "artifact_evidence_incomplete",
    }
}

closed_enum! {
    /// What a guardrail evaluation is *about*.
    SubjectKind, "SubjectKind" {
        /// A task.
        Task => "task",
        /// A task's resolved workflow.
        TaskWorkflow => "task_workflow",
        /// One run of a team.
        TeamRun => "team_run",
        /// One run of a single agent.
        AgentRun => "agent_run",
        /// One gate of the pinned profile.
        Gate => "gate",
        /// One requested action.
        Action => "action",
    }
}

closed_enum! {
    /// Which surface a requested action touches.
    ActionDomain, "ActionDomain" {
        /// The working tree on disk.
        Filesystem => "filesystem",
        /// A runtime session.
        Runtime => "runtime",
        /// An external ticket system.
        ExternalTicket => "external_ticket",
        /// Kontor's own records.
        ControlPlane => "control_plane",
    }
}

closed_enum! {
    /// What a requested action is trying to achieve.
    ///
    /// Deliberately coarse: these are the intents guardrails distinguish, not a
    /// catalogue of operations. The operation itself stays opaque in
    /// [`RequestedAction::operation`].
    ActionIntent, "ActionIntent" {
        /// Read without changing anything.
        Inspect => "inspect",
        /// Produce or update artifact evidence.
        ProduceArtifact => "produce_artifact",
        /// Record a phase as complete.
        CompletePhase => "complete_phase",
        /// Record a passing, waiving or starting gate verdict.
        RecordGateVerdict => "record_gate_verdict",
        /// Record a rejecting gate verdict.
        RecordGateRejection => "record_gate_rejection",
        /// Close a run with terminal evidence.
        CloseRun => "close_run",
        /// Change something outside Kontor's records.
        Mutate => "mutate",
    }
}

impl ActionIntent {
    /// Whether this intent writes a gate verdict of any kind.
    #[must_use]
    pub const fn writes_gate_verdict(self) -> bool {
        matches!(self, Self::RecordGateVerdict | Self::RecordGateRejection)
    }
}

closed_enum! {
    /// What a requested action does to its target.
    ///
    /// Declared by the caller that classified the operation, never guessed from
    /// its name: an unclassified action is not admitted as harmless.
    ActionEffect, "ActionEffect" {
        /// Observes only.
        Read => "read",
        /// Changes state that can be changed back.
        Mutate => "mutate",
        /// Destroys state. Needs an approval bound to this exact action.
        Destroy => "destroy",
    }
}

closed_enum! {
    /// Where an approval's authority came from.
    ///
    /// [`AuthoritySource::RecoveryAdvice`] exists only so it can be refused:
    /// an advisor or a committee produces recommendations, and a recommendation
    /// is never an approval. The store refuses to persist one at all.
    AuthoritySource, "AuthoritySource" {
        /// A human operator decision.
        Operator => "operator",
        /// A bounded execution authorization already granted over the scope.
        ExecutionAuthorization => "execution_authorization",
        /// Advice produced during recovery. Never sufficient.
        RecoveryAdvice => "recovery_advice",
    }
}

closed_enum! {
    /// What an approval covers.
    ApprovalScopeKind, "ApprovalScopeKind" {
        /// The whole project.
        Project => "project",
        /// One task.
        Task => "task",
    }
}

/// How much trust the actor's evidence carries.
///
/// Rung 1 is degraded: the actor is running on advisory evidence and may
/// produce work, not verdicts. [`VerdictRung::VERDICT_THRESHOLD`] is the rung a
/// gate verdict requires, and it is a property of the *actor*, not of the gate —
/// which is what keeps the rule free of gate names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct VerdictRung(u32);

impl VerdictRung {
    /// The lowest rung that may write a gate verdict.
    pub const VERDICT_THRESHOLD: Self = Self(2);

    /// Parse a stored rung.
    ///
    /// # Errors
    /// Rejects `0`: a rung is a positive claim about evidence quality, and the
    /// absence of one is not rung zero.
    pub fn parse(value: u32) -> DomainResult<Self> {
        if value == 0 {
            return Err(DomainError::invalid("VerdictRung", "must be at least 1"));
        }
        Ok(Self(value))
    }

    /// The numeric rung.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Whether this rung may write a gate verdict.
    #[must_use]
    pub const fn may_write_verdict(self) -> bool {
        self.0 >= Self::VERDICT_THRESHOLD.0
    }
}

impl TryFrom<u32> for VerdictRung {
    type Error = DomainError;

    fn try_from(value: u32) -> DomainResult<Self> {
        Self::parse(value)
    }
}

impl From<VerdictRung> for u32 {
    fn from(rung: VerdictRung) -> Self {
        rung.0
    }
}

/// The rule revision being evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardrailRule {
    /// Which rule.
    pub key: GuardrailRuleKey,
    /// Which revision of it. Recorded with every evaluation so a later rule
    /// change is visible as a change rather than as a silent re-interpretation.
    pub version: SpecVersion,
}

/// The simulated identity an actor is running as, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaActor {
    /// The simulated persona.
    pub persona: PersonaKey,
    /// The gate the scenario exercises, taken from the pinned snapshot.
    pub gate_under_test: GateKey,
    /// The role the persona acts as.
    pub actor_role: RoleKey,
}

/// Who is acting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorContext {
    /// The account profile the actor authenticates through.
    pub account: AccountProfileId,
    /// The stable authenticated principal behind that account.
    ///
    /// This — not the agent run, not the display name — is what a rejection
    /// counter is keyed on. A relaunch mints a new run id and changes nothing
    /// about who reviewed.
    pub principal: ExternalId,
    /// The role the actor holds.
    pub role: RoleKey,
    /// How much the actor's evidence is worth.
    pub verdict_rung: VerdictRung,
    /// The pinned persona scenario identity, when the actor is simulated.
    pub persona: Option<PersonaActor>,
}

/// Which run, task and rule revision the request belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContext {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task.
    pub task_id: TaskId,
    /// The task's workflow.
    pub workflow_id: TaskWorkflowId,
    /// The module the task contends for, when it declares one.
    pub module: Option<ModuleKey>,
    /// The team run, when there is one.
    pub team_run_id: Option<TeamRunId>,
    /// The agent run, when there is one.
    pub agent_run_id: Option<AgentRunId>,
    /// The run this one succeeds, for a recovery successor.
    pub parent_agent_run_id: Option<AgentRunId>,
    /// The account this run is pinned to.
    pub pinned_account: Option<AccountProfileId>,
    /// The worktree this run was first recorded in.
    pub recorded_worktree: Option<ExternalName>,
    /// What is about to happen.
    pub requested_action: RequestedAction,
    /// The rule-set revision in force.
    pub rule_set_revision: SpecVersion,
}

/// The exact action a guardrail is being asked about.
///
/// [`RequestedAction::digest`] is the whole binding between an approval and an
/// action: it is the canonical digest of the concrete command, so an approval
/// for one deletion cannot be replayed against another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedAction {
    /// Which surface it touches.
    pub domain: ActionDomain,
    /// What it is trying to achieve.
    pub intent: ActionIntent,
    /// What it does to its target.
    pub effect: ActionEffect,
    /// The operation, as the adapter names it. Opaque; never a branch here.
    pub operation: ExternalName,
    /// The target, as the adapter addresses it. Opaque.
    pub target: ExternalId,
    /// Canonical digest of the exact action, arguments included.
    pub digest: ContentHash,
    /// Whether the adapter can run this operation without effects.
    pub dry_run_supported: bool,
    /// Whether *this* request is the effect-free one.
    pub dry_run: bool,
}

/// What the workspace layer can prove about where work would land.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceEvidence {
    /// The worktree this request claims to be acting in.
    pub claimed_worktree: Option<ExternalName>,
    /// Every worktree that could plausibly be meant.
    ///
    /// More than one is ambiguity, and ambiguity parks. It is never resolved by
    /// preferring the first, the newest or the shortest.
    pub candidate_worktrees: Vec<ExternalName>,
    /// Which modules are currently held, and by whom.
    pub module_claims: Vec<ModuleClaim>,
}

/// One module held by one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleClaim {
    /// The module.
    pub module: ModuleKey,
    /// The task holding it.
    pub task_id: TaskId,
    /// The worktree it is held in, when it is isolated by one.
    pub worktree: Option<ExternalName>,
    /// Whether the claim is still live.
    pub in_flight: bool,
}

/// One produced artifact, addressed by reference rather than copied.
///
/// The locator is a canonical document naming *where* the artifact is; it is
/// never the artifact. Transcripts, diffs and credentials do not belong in this
/// record, and [`CanonicalDocument`] refuses the last of those outright.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEvidence {
    /// The evidence record.
    pub id: ArtifactEvidenceId,
    /// The artifact contract this satisfies, as the pinned profile declares it.
    pub key: ArtifactKey,
    /// Where the artifact is.
    pub locator: CanonicalDocument,
    /// Which role produced it.
    pub producer_role: RoleKey,
    /// Which account produced it.
    pub producer_account: AccountProfileId,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// An approval bound to one exact destructive action in one exact scope.
///
/// There is deliberately no reusable, standing approval: every field below
/// narrows the receipt to a single action, and the store enforces one receipt
/// per action digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceipt {
    /// The receipt.
    pub id: ApprovalReceiptId,
    /// Whether it covers a project or one task.
    pub scope_kind: ApprovalScopeKind,
    /// The project it was issued in.
    pub project_id: ProjectId,
    /// The task, when the scope is one task.
    pub task_id: Option<TaskId>,
    /// The domain of the approved action.
    pub action_domain: ActionDomain,
    /// The intent of the approved action.
    pub action_intent: ActionIntent,
    /// The effect of the approved action.
    pub action_effect: ActionEffect,
    /// The canonical digest of the approved action.
    pub action_digest: ContentHash,
    /// The principal that approved.
    pub approver_principal: ExternalId,
    /// The role it approved as.
    pub approver_role: RoleKey,
    /// The account it approved through.
    pub approver_account: AccountProfileId,
    /// Where the authority came from.
    pub authority_source: AuthoritySource,
    /// The evidence bundle behind the decision.
    pub evidence: CanonicalDocument,
    /// When it was issued.
    pub issued_at: Timestamp,
    /// When it stops being valid.
    pub expires_at: Timestamp,
    /// When it was spent, if it has been.
    pub consumed_at: Option<Timestamp>,
}

impl ApprovalReceipt {
    /// Whether this receipt authorizes `action` in this scope at `now`.
    ///
    /// Returns the reason it does not, so the evaluator records *which* binding
    /// failed rather than a bare refusal.
    #[must_use]
    pub fn refusal_for(
        &self,
        action: &RequestedAction,
        project_id: ProjectId,
        task_id: TaskId,
        now: Timestamp,
    ) -> Option<ReasonCode> {
        if self.authority_source == AuthoritySource::RecoveryAdvice {
            return Some(ReasonCode::ApprovalFromRecoveryAdvice);
        }
        if self.action_digest != action.digest
            || self.action_domain != action.domain
            || self.action_intent != action.intent
            || self.action_effect != action.effect
        {
            return Some(ReasonCode::ApprovalActionMismatch);
        }
        if self.project_id != project_id {
            return Some(ReasonCode::ApprovalScopeMismatch);
        }
        if self.scope_kind == ApprovalScopeKind::Task && self.task_id != Some(task_id) {
            return Some(ReasonCode::ApprovalScopeMismatch);
        }
        if self.consumed_at.is_some() {
            return Some(ReasonCode::ApprovalConsumed);
        }
        if now >= self.expires_at {
            return Some(ReasonCode::ApprovalExpired);
        }
        None
    }
}

/// A pointer to the runtime observation that evidences a run's closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeObservationRef {
    /// The run the observation is about.
    pub agent_run_id: AgentRunId,
    /// The stored event's control-plane cursor.
    pub cursor: EventCursor,
    /// Digest of the stored canonical payload.
    pub evidence_hash: ContentHash,
}

/// Where the proof behind a verdict is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceRef {
    /// A produced artifact.
    Artifact {
        /// The artifact contract.
        key: ArtifactKey,
        /// The evidence record.
        id: ArtifactEvidenceId,
    },
    /// An approval receipt.
    Approval {
        /// The receipt.
        id: ApprovalReceiptId,
        /// The action it is bound to.
        action_digest: ContentHash,
    },
    /// One prior gate verdict.
    GateVerdict {
        /// The gate.
        gate: GateKey,
        /// Its position in that gate's append-only history.
        sequence: u32,
    },
    /// A terminal runtime observation.
    RuntimeObservation {
        /// The run it is about.
        agent_run_id: AgentRunId,
        /// Its control-plane cursor.
        cursor: EventCursor,
    },
    /// A worktree the workspace layer offered.
    Worktree {
        /// The worktree.
        worktree: ExternalName,
    },
    /// A module claim held by another task.
    ModuleClaim {
        /// The module.
        module: ModuleKey,
        /// The task holding it.
        task_id: TaskId,
    },
}

/// What a guardrail evaluation is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationSubject {
    /// The kind of thing.
    pub kind: SubjectKind,
    /// Its identifier, as canonical text.
    pub id: ExternalId,
}

/// Everything a rule is allowed to look at.
///
/// The whole request is canonicalized into the stored evaluation, so the inputs
/// a verdict was reached on are recoverable and comparable byte-for-byte. That
/// is what makes "the same inputs always produce the same verdict" a checkable
/// claim rather than an aspiration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationRequest {
    /// Schema generation of this request document.
    pub schema_version: SchemaVersion,
    /// The rule to apply.
    pub rule: GuardrailRule,
    /// The task's pinned work-profile snapshot.
    pub workflow: ResolvedWorkProfileSnapshot,
    /// The phase the workflow is in.
    pub current_phase: PhaseKey,
    /// The gate under evaluation, when the request is about one.
    pub gate: Option<GateKey>,
    /// Who is acting.
    pub actor: ActorContext,
    /// Which run and action.
    pub run: RunContext,
    /// What the workspace layer can prove.
    pub workspace: WorkspaceEvidence,
    /// Artifacts produced so far.
    pub artifacts: Vec<ArtifactEvidence>,
    /// The approval presented, if any.
    pub approval: Option<ApprovalReceipt>,
    /// This workflow's append-only gate history.
    pub prior_gate_evaluations: Vec<GateEvaluation>,
    /// The terminal observation cited, if any.
    pub terminal_observation: Option<RuntimeObservationRef>,
    /// The instant the evaluation is made against.
    ///
    /// An input, not a call to the clock: an expiry check that read `now()`
    /// inside the rule would make the same request decide differently on replay.
    pub evaluated_at: Timestamp,
}

/// One immutable guardrail evaluation.
///
/// Every evaluation is a new value. Nothing here is ever updated: a later
/// evaluation of the same rule against the same subject is another record, and
/// the store's triggers refuse any other outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardrailEvaluation {
    /// The evaluation.
    pub id: GuardrailEvaluationId,
    /// Which rule produced it.
    pub rule_key: GuardrailRuleKey,
    /// Which revision of that rule.
    pub rule_version: SpecVersion,
    /// What it is about.
    pub subject: EvaluationSubject,
    /// The canonical inputs it was decided on.
    pub inputs: CanonicalDocument,
    /// Digest of those inputs.
    pub inputs_hash: ContentHash,
    /// The verdict.
    pub verdict: PolicyVerdict,
    /// Why.
    pub reason_code: ReasonCode,
    /// Where the proof is.
    pub evidence_refs: Vec<EvidenceRef>,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// The part of an evaluation a rule actually decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The verdict.
    pub verdict: PolicyVerdict,
    /// Why.
    pub reason_code: ReasonCode,
    /// Where the proof is.
    pub evidence_refs: Vec<EvidenceRef>,
}

impl Decision {
    /// A decision carrying no evidence pointers.
    #[must_use]
    pub const fn bare(verdict: PolicyVerdict, reason_code: ReasonCode) -> Self {
        Self {
            verdict,
            reason_code,
            evidence_refs: Vec::new(),
        }
    }

    /// A decision with evidence pointers.
    #[must_use]
    pub const fn with_evidence(
        verdict: PolicyVerdict,
        reason_code: ReasonCode,
        evidence_refs: Vec<EvidenceRef>,
    ) -> Self {
        Self {
            verdict,
            reason_code,
            evidence_refs,
        }
    }

    /// Whether the decision admits the request.
    #[must_use]
    pub const fn admits(&self) -> bool {
        matches!(self.verdict, PolicyVerdict::Pass | PolicyVerdict::Warn)
    }
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

closed_enum! {
    /// Where a recovery episode is.
    RecoveryStatus, "RecoveryStatus" {
        /// Opened by the park; nothing attempted yet.
        Open => "open",
        /// Deterministic inspection and repair has run.
        DeterministicRepair => "deterministic_repair",
        /// An advisor has been consulted.
        Advisor => "advisor",
        /// A committee has been convened.
        Committee => "committee",
        /// At least one follow-up has been dispatched.
        Followup => "followup",
        /// Terminal: the work recovered.
        Recovered => "recovered",
        /// Terminal: only a human can decide.
        NeedsHuman => "needs_human",
    }
}

impl RecoveryStatus {
    /// Whether the episode is closed.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Recovered | Self::NeedsHuman)
    }
}

closed_enum! {
    /// One appended step of a recovery episode.
    RecoveryStepKind, "RecoveryStepKind" {
        /// Deterministic inspection and repair.
        DeterministicRepair => "deterministic_repair",
        /// A read-only advisor consultation.
        Advisor => "advisor",
        /// A read-only committee.
        Committee => "committee",
        /// A follow-up dispatched to a linked successor run.
        FollowupExecution => "followup_execution",
        /// An escalation to a human.
        Escalation => "escalation",
    }
}

impl RecoveryStepKind {
    /// Whether this step may only append recommendations and evidence.
    ///
    /// A read-only step never passes, rejects or waives a gate, never approves
    /// destructive work, never resets a counter and never launches a team. The
    /// type says so, and [`crate::recovery::plan`] enforces it by refusing to
    /// let such a step carry a successor run.
    #[must_use]
    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::Advisor | Self::Committee)
    }
}

closed_enum! {
    /// The only five reasons an episode reaches a human.
    ///
    /// The set is closed and complete: there is no `Other`, and no code path
    /// reaches [`RecoveryStatus::NeedsHuman`] without naming one of these.
    EscalationCause, "EscalationCause" {
        /// The workspace or runtime state is not safe to act on.
        UnsafeState => "unsafe_state",
        /// The authority required does not exist.
        MissingAuthority => "missing_authority",
        /// A committee did not converge.
        CommitteeDisagreement => "committee_disagreement",
        /// Required evidence is incomplete.
        IncompleteEvidence => "incomplete_evidence",
        /// The episode's bounded budget is spent.
        BudgetExhausted => "budget_exhausted",
    }
}

/// One bounded recovery episode for one parked run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryEpisode {
    /// The episode.
    pub id: RecoveryEpisodeId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task that parked.
    pub task_id: TaskId,
    /// Its workflow.
    pub workflow_id: TaskWorkflowId,
    /// The run that was parked. Never resumed in place.
    pub parked_agent_run_id: AgentRunId,
    /// Where the episode is.
    pub status: RecoveryStatus,
    /// The guardrail evaluation that caused the park.
    pub cause_evaluation_id: GuardrailEvaluationId,
    /// Whether the one advisor consultation has been spent.
    pub advisor_used: bool,
    /// Whether the one committee has been spent.
    pub committee_used: bool,
    /// How many follow-ups were actually dispatched.
    pub effective_followups: u32,
    /// The linked successor run, once one exists.
    pub successor_agent_run_id: Option<AgentRunId>,
    /// Why it escalated, when it did.
    pub escalation_cause: Option<EscalationCause>,
    /// Optimistic concurrency.
    pub revision: AggregateRevision,
    /// When it opened.
    pub created_at: Timestamp,
    /// When it closed.
    pub closed_at: Option<Timestamp>,
}
