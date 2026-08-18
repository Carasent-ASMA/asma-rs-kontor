//! Command intent, outbox and confirmation.
//!
//! Kontor never treats an acknowledgement as completion. A command moves
//! through [`CommandReceiptState`], and the one state that matters most is
//! [`CommandReceiptState::ConfirmationUnknown`]: after a dispatch whose result
//! is unknown, **retrying is forbidden** until a correlation lookup proves the
//! command had no effect. [`CommandReceipt::authorize_retry`] is the only way to
//! obtain that permission, and it demands evidence.
//!
//! Intent, desired state, the outbox entry and the intent event are written in
//! one transaction by the store; nothing here can be persisted on its own.

use serde::{Deserialize, Serialize};

use crate::id::{
    AgentRunId, AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId,
    IdempotencyKey, MiniProjectId, ProjectId, TaskId, TeamRunId, TicketLinkId, Timestamp,
    WorkCalendarId,
};
use crate::state::{DesiredRunState, NativeRuntimeIdentity};
use crate::{DomainError, DomainResult};

closed_enum! {
    /// What a command asks for.
    ///
    /// The set is generic control-plane vocabulary; no value names a vendor, a
    /// deployment or a seed profile.
    CommandKind, "CommandKind" {
        /// Launch a run.
        LaunchRun => "launch_run",
        /// Cancel a run.
        CancelRun => "cancel_run",
        /// Park a run.
        ParkRun => "park_run",
        /// Abandon a run without a runtime verdict.
        AbandonRun => "abandon_run",
        /// Return a blocked, parked or human-held task to `ready`.
        ResumeTask => "resume_task",
        /// Record a gate verdict.
        RecordGateVerdict => "record_gate_verdict",
        /// Approve an intake proposal.
        ApproveIntake => "approve_intake",
        /// Write a projection to an external ticket.
        SyncTicket => "sync_ticket",
        /// Converge an external ticket's assignee.
        AssignTicket => "assign_ticket",
        /// Converge an external ticket's status.
        TransitionTicket => "transition_ticket",
        /// Grant a bounded execution authorization over a scope.
        AuthorizeExecution => "authorize_execution",
        /// Approve a calendar override over a scope.
        ApproveScheduleOverride => "approve_schedule_override",
        /// Revoke a calendar override over a scope.
        ///
        /// Deliberately distinct from
        /// [`CommandKind::ApproveScheduleOverride`]: if one kind covered both,
        /// an approval receipt could be replayed as its own revocation.
        RevokeScheduleOverride => "revoke_schedule_override",
        /// Resolve a detected status conflict on a ticket link.
        ResolveStatusConflict => "resolve_status_conflict",
        /// Assign a calendar to a project.
        AssignWorkCalendar => "assign_work_calendar",
        /// Revoke a bounded execution authorization over a scope.
        ///
        /// Deliberately distinct from
        /// [`CommandKind::RevokeScheduleOverride`], for the same reason that one
        /// is distinct from its own approval: a calendar override and an
        /// execution authorization are different grants over the same scope, and
        /// one kind covering both would let a receipt that revoked a calendar
        /// window be replayed as the authority that disarmed the work.
        RevokeExecutionAuthorization => "revoke_execution_authorization",
        /// Bring a project into existence, or prove the one at that root is the
        /// one the caller meant.
        ///
        /// It is the first command a Realm can record: it targets the project it
        /// is about, so the receipt is attributable in the ordinary way, and it
        /// carries no desired state because a project has none.
        EnsureProject => "ensure_project",
        /// Apply a declarative epic's whole work graph.
        ApplyEpicGraph => "apply_epic_graph",
        /// Move an epic through a lifecycle transition. The action it carries is
        /// in the intent; the kind says only that epic lifecycle authority was
        /// exercised, which is what must not be confused with applying one.
        TransitionEpic => "transition_epic",
        /// Start an already-planned batch through admission.
        StartScheduledWork => "start_scheduled_work",
        /// Move a task through a lifecycle transition other than a resume.
        ///
        /// Distinct from [`CommandKind::ResumeTask`] because a resume receipt is
        /// consumed as the *authority* to leave a held state: one kind covering
        /// both would let the receipt that blocked a task be replayed as the
        /// authority that released it.
        TransitionTask => "transition_task",
        /// Resolve and freeze a task's context pack.
        ResolveContext => "resolve_context",
        /// Correct the work profile a task is pinned to, before a run froze it.
        SelectTaskProfile => "select_task_profile",
        /// Confirm the team revision a task's pinned profile prescribes.
        SelectTaskTeam => "select_task_team",
        /// Correct the provider account a task will run under.
        SelectTaskAccount => "select_task_account",
        /// Converge a task's external tickets towards its own milestone.
        ReconcileTicket => "reconcile_ticket",
        /// Settle a run against what its runtime currently reports.
        ///
        /// It carries no desired state and no outcome: settling is the act of
        /// *asking*, and what comes back is the runtime's answer. A kind that
        /// carried an outcome would be an operator declaring one.
        SettleRuntime => "settle_runtime",
        /// Replace one runtime-terminal persistent seat with its linked successor.
        ReplaceSeat => "replace_seat",
        /// Run the native capacity collectors and fold what they report.
        ///
        /// It targets the *project*, for the same reason as
        /// [`CommandKind::EnsureAccountProfile`]: what is being recorded is
        /// authority over the project's fleet, not over one account in it.
        RefreshCapacity => "refresh_capacity",
        /// Stand an operator judgement beside one account's raw evidence.
        ///
        /// Never over it. The raw observation is immutable, so this kind can
        /// only ever have added a second record — which is what makes the
        /// disagreement auditable.
        OverrideAvailability => "override_availability",
        /// Observe one exact bound seat and record what came back.
        ObserveSeat => "observe_seat",
        /// Retire and release one exact bound seat.
        RetireSeat => "retire_seat",
        /// Publish one immutable project topology specification revision.
        ///
        /// It targets the *project*, because deciding which node kinds may ever
        /// exist is authority over the project and not over any node in it —
        /// and the revision it publishes is not an aggregate a command may name.
        PublishTopologySpec => "publish_topology_spec",
        /// Move one epic's pinned topology revision to another published one.
        ///
        /// The epic is the aggregate: the pin is the epic's, and the revision it
        /// moves to is immutable and shared.
        UpgradeTopology => "upgrade_topology",
        /// Correct the visible title of one bound native container.
        ///
        /// It carries no title, because the title is not the caller's: the
        /// operation derives it from the node's pinned topology and the plane's
        /// typed scope. What is being recorded is the authority to repair a
        /// display that Kontor itself rendered wrongly, and the container it
        /// repairs is addressed by its durable binding rather than by its name.
        RetitleContainer => "retitle_container",
        /// Publish the next immutable Project Core Team revision.
        ///
        /// The project is the aggregate. This changes project configuration and
        /// nothing else: it seats no epic, because a Core Team is the roster an
        /// epic is staffed *from*, and a running epic holds the revision it
        /// froze at promotion.
        ApplyCoreTeam => "apply_core_team",
        /// Open one ad-hoc Quick session under the project's session base.
        ///
        /// The project is the aggregate. A Quick session creates no MiniProject
        /// and no TeamRun, so there is no narrower one to name.
        EnsureQuickSession => "ensure_quick_session",
        /// Turn one Quick session into an epic.
        ///
        /// The epic is the aggregate, because the epic is what the command
        /// brings into existence and what every later command about this work
        /// will address.
        PromoteQuickSession => "promote_quick_session",
        /// Materialize one epic's frozen roster into seats.
        MaterializeCoreTeam => "materialize_core_team",
        /// Move one epic's roster pin to another published revision.
        UpgradeEpicRoster => "upgrade_epic_roster",
        /// Bring a provider-account profile into existence, or prove the one
        /// with that label matches.
        ///
        /// It targets the *project*, because a profile is not an aggregate a
        /// command may name — which is the point: the authority being recorded is
        /// authority over the project's fleet, not over one row in it.
        EnsureAccountProfile => "ensure_account_profile",
        /// Decide one inbound source event under a pinned trigger revision.
        ///
        /// Distinct from [`CommandKind::ApproveIntake`], which is the *human
        /// approval* an approved intake requires. A receipt that merely recorded
        /// a decision must never be citable as the approval that armed it.
        SubmitIntake => "submit_intake",
        /// Mirror one task's inbound external comment revisions.
        ///
        /// It reads the external system and writes only the mirror.
        /// [`CommandKind::SyncTicket`] is the kind that writes *to* a ticket, and
        /// a pull receipt must not be replayable as authority for a push.
        PullTicketComments => "pull_ticket_comments",
        /// Take ownership of a task's external tickets for the principal Kontor
        /// authenticates as.
        ///
        /// Distinct from [`CommandKind::AssignTicket`], which can name any
        /// assignee the connector accepts. A claim can name only the principal,
        /// so a claim receipt must not be citable as an arbitrary assignment.
        ClaimTicket => "claim_ticket",
    }
}

/// Which aggregate a command targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AggregateRef {
    /// A project.
    Project {
        /// The project.
        project_id: ProjectId,
    },
    /// A goal.
    MiniProject {
        /// The goal.
        mini_project_id: MiniProjectId,
    },
    /// A task.
    Task {
        /// The task.
        task_id: TaskId,
    },
    /// A team run.
    TeamRun {
        /// The team run.
        team_run_id: TeamRunId,
    },
    /// An agent run.
    AgentRun {
        /// The agent run.
        agent_run_id: AgentRunId,
    },
    /// An external ticket link.
    TicketLink {
        /// The link.
        link_id: TicketLinkId,
    },
    /// A project calendar assignment.
    WorkCalendar {
        /// The assignment.
        work_calendar_id: WorkCalendarId,
    },
}

closed_enum! {
    /// Which aggregate a command names, without naming *which* one.
    ///
    /// The spelling matches `command_targets.target_kind`.
    AggregateKind, "AggregateKind" {
        /// A project.
        Project => "project",
        /// A goal.
        MiniProject => "mini_project",
        /// A task.
        Task => "task",
        /// A team run.
        TeamRun => "team_run",
        /// An agent run.
        AgentRun => "agent_run",
        /// An external ticket link.
        TicketLink => "ticket_link",
        /// A project calendar assignment.
        WorkCalendar => "work_calendar",
    }
}

impl AggregateRef {
    /// Which aggregate this reference names.
    #[must_use]
    pub const fn kind(&self) -> AggregateKind {
        match self {
            Self::Project { .. } => AggregateKind::Project,
            Self::MiniProject { .. } => AggregateKind::MiniProject,
            Self::Task { .. } => AggregateKind::Task,
            Self::TeamRun { .. } => AggregateKind::TeamRun,
            Self::AgentRun { .. } => AggregateKind::AgentRun,
            Self::TicketLink { .. } => AggregateKind::TicketLink,
            Self::WorkCalendar { .. } => AggregateKind::WorkCalendar,
        }
    }
}

/// How a command's cited target revision relates to the target aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionRule {
    /// Recording the intent advances the target's own revision, so the store
    /// compares and swaps it in the same transaction as the receipt.
    CompareAndSwap,
    /// The intent cites the revision it was computed against as a witness; the
    /// target row itself is not changed by recording the intent.
    Witness,
}

/// What desired-state change a command carries against a given target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredStateRule {
    /// The command must carry exactly this desired run state.
    Requires(DesiredRunState),
    /// The command must carry no desired-state change. Only an agent run
    /// records a desired state at all: a team run's lifecycle is derived from
    /// its children, and no other aggregate has one.
    Forbidden,
}

/// The rule one command kind obeys against one aggregate kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetRule {
    /// How the cited revision is treated.
    pub revision: RevisionRule,
    /// What desired-state change is carried.
    pub desired: DesiredStateRule,
}

/// The rule for a run command: launching, cancelling, parking and abandoning
/// are the only commands that move a run's desired state, and only an agent run
/// records one. Against a team run the same command is a witness — a team's
/// state is derived from its children, never asked for directly.
const fn run_intent(target: AggregateKind, desired: DesiredRunState) -> Option<TargetRule> {
    match target {
        AggregateKind::AgentRun => Some(TargetRule {
            revision: RevisionRule::CompareAndSwap,
            desired: DesiredStateRule::Requires(desired),
        }),
        AggregateKind::TeamRun => Some(TargetRule {
            revision: RevisionRule::Witness,
            desired: DesiredStateRule::Forbidden,
        }),
        _ => None,
    }
}

/// The rule for a command that only witnesses its target's revision, legal
/// against `legal` targets and nothing else.
const fn witness(legal: bool) -> Option<TargetRule> {
    if legal {
        Some(TargetRule {
            revision: RevisionRule::Witness,
            desired: DesiredStateRule::Forbidden,
        })
    } else {
        None
    }
}

impl CommandKind {
    /// The rule this command obeys against `target`, or `None` when this
    /// command may not target that aggregate at all.
    ///
    /// This is the whole compatibility matrix: legal targets, the revision rule
    /// for each, and the desired-state change carried. A command kind and an
    /// aggregate kind are not independently supplied facts that happen to be
    /// stored side by side — one constrains the other.
    #[must_use]
    pub const fn rule_for(self, target: AggregateKind) -> Option<TargetRule> {
        use AggregateKind as A;
        match self {
            Self::LaunchRun => run_intent(target, DesiredRunState::RunRequested),
            Self::CancelRun => run_intent(target, DesiredRunState::CancelRequested),
            Self::ParkRun => run_intent(target, DesiredRunState::ParkRequested),
            Self::AbandonRun => run_intent(target, DesiredRunState::AbandonRequested),
            Self::ResumeTask | Self::RecordGateVerdict => witness(matches!(target, A::Task)),
            // A project is a legal target because an intake proposal is decided
            // *before* the work it proposes exists: at that moment there is no
            // goal and no task to name, and a receipt cannot target a row that
            // no transaction has written yet. Approving an already-created graph
            // still targets that graph.
            Self::ApproveIntake => witness(matches!(target, A::Project | A::MiniProject | A::Task)),
            Self::SyncTicket
            | Self::AssignTicket
            | Self::TransitionTicket
            | Self::ResolveStatusConflict => witness(matches!(target, A::TicketLink)),
            // A capability or an override is granted over a work scope, and a
            // work scope is exactly one of these three aggregates.
            Self::AuthorizeExecution
            | Self::ApproveScheduleOverride
            | Self::RevokeScheduleOverride
            | Self::RevokeExecutionAuthorization => {
                witness(matches!(target, A::Project | A::MiniProject | A::Task))
            }
            Self::AssignWorkCalendar => witness(matches!(target, A::WorkCalendar)),
            // Bootstrap authority is authority over the project, and over
            // nothing narrower: neither command names a row inside it.
            // Bootstrap and intake authority is authority over the project. An
            // intake decision that creates no work graph has no narrower
            // aggregate to name, and naming one it did not create would be a
            // claim about work that does not exist.
            Self::EnsureProject | Self::EnsureAccountProfile | Self::SubmitIntake => {
                witness(matches!(target, A::Project))
            }
            Self::ApplyEpicGraph | Self::TransitionEpic | Self::StartScheduledWork => {
                witness(matches!(target, A::MiniProject))
            }
            Self::TransitionTask
            | Self::ResolveContext
            | Self::SelectTaskProfile
            | Self::SelectTaskTeam
            | Self::SelectTaskAccount
            | Self::ReconcileTicket
            // Pulling comments and claiming ownership both cover *every* link a
            // task holds, so the task is the aggregate the authority is over. A
            // receipt naming one link would understate what it authorized.
            | Self::PullTicketComments
            | Self::ClaimTicket => witness(matches!(target, A::Task)),
            // Settlement witnesses the run it is about. It is deliberately not a
            // `run_intent`: those carry a desired state, and asking a runtime what
            // is already true desires nothing.
            Self::SettleRuntime => witness(matches!(target, A::AgentRun)),
            Self::ReplaceSeat => witness(matches!(target, A::TeamRun)),
            // Capacity is a fact about the project's fleet. Neither command
            // names an account, because an account profile is not an aggregate
            // a command may target — and a refresh covers several of them at
            // once, so naming one would understate what it authorized.
            Self::RefreshCapacity | Self::OverrideAvailability => {
                witness(matches!(target, A::Project))
            }
            // A seat is not an aggregate a command may target, and a persistent
            // control-plane seat has no TeamRun to stand in for one — that is
            // precisely what makes it persistent. The project is what the
            // authority is over, and it is the one aggregate every seat has.
            Self::ObserveSeat | Self::RetireSeat => witness(matches!(target, A::Project)),
            Self::PublishTopologySpec => witness(matches!(target, A::Project)),
            Self::UpgradeTopology => witness(matches!(target, A::MiniProject)),
            // Neither a native container nor the topology node holding it is an
            // aggregate a command may name, and the epic is too wide: a retitle
            // touches one node's container. The project is the one aggregate it
            // certainly has, exactly as for `ObserveSeat`.
            Self::RetitleContainer => witness(matches!(target, A::Project)),
            // The project, and only the project. A Core Team is project
            // configuration: allowing an epic here would let a receipt claim
            // that publishing a roster changed one running epic, which is the
            // one thing publishing a roster deliberately does not do.
            Self::ApplyCoreTeam | Self::EnsureQuickSession => witness(matches!(target, A::Project)),
            // The epic each of these is about. Promotion names the epic it
            // creates rather than the project it creates it in: the receipt has
            // to be findable from the thing that now exists.
            Self::PromoteQuickSession
            | Self::MaterializeCoreTeam
            | Self::UpgradeEpicRoster => witness(matches!(target, A::MiniProject)),
        }
    }

    /// Prove this command may target `target` carrying `desired`, and return
    /// the rule the pair obeys.
    ///
    /// The rule is returned rather than looked up a second time by the caller:
    /// one gate, one answer, so a store cannot accidentally admit a pair the
    /// domain refused.
    ///
    /// # Errors
    /// * [`DomainError::Invalid`] when the aggregate is not a legal target for
    ///   this command, when a desired-state change is missing or wrong for a
    ///   command that carries one, and when a desired-state change rides on a
    ///   command or an aggregate that records none.
    ///
    /// Neither the candidate target id nor the candidate state is echoed.
    pub fn ensure_compatible(
        self,
        target: &AggregateRef,
        desired: Option<DesiredRunState>,
    ) -> DomainResult<TargetRule> {
        let Some(rule) = self.rule_for(target.kind()) else {
            return Err(DomainError::invalid(
                "CommandIntent",
                "this command may not target that kind of aggregate",
            ));
        };
        match (rule.desired, desired) {
            (DesiredStateRule::Requires(required), Some(carried)) if carried == required => {
                Ok(rule)
            }
            (DesiredStateRule::Requires(_), _) => Err(DomainError::invalid(
                "CommandIntent",
                "this command must carry its own desired run state",
            )),
            (DesiredStateRule::Forbidden, None) => Ok(rule),
            (DesiredStateRule::Forbidden, Some(_)) => Err(DomainError::invalid(
                "CommandIntent",
                "this command records no desired-state change against that aggregate",
            )),
        }
    }
}

/// What a stored command receipt says, read back inside the transaction that
/// wants to consume it as authority.
///
/// A foreign key proves the receipt exists in this project. It does not prove
/// the receipt *authorizes anything*: a receipt for one command against one
/// aggregate is not permission to do a different thing somewhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptAuthority {
    /// The project the stored receipt belongs to.
    pub project_id: ProjectId,
    /// The command the stored receipt records.
    pub kind: CommandKind,
    /// The aggregate the stored receipt targets.
    pub target: AggregateRef,
}

impl ReceiptAuthority {
    /// Prove the loaded receipt authorizes `kind` against `target` in
    /// `project_id`.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a receipt recorded in another
    /// project, a receipt recording a different command, and a receipt aimed at
    /// a different aggregate. No candidate value is echoed.
    pub fn authorizes(
        &self,
        subject: &'static str,
        project_id: ProjectId,
        kind: CommandKind,
        target: AggregateRef,
    ) -> DomainResult<()> {
        if self.project_id != project_id {
            return Err(DomainError::invalid(
                subject,
                "the cited receipt belongs to another project",
            ));
        }
        if self.kind != kind {
            return Err(DomainError::invalid(
                subject,
                "the cited receipt records a different command",
            ));
        }
        if self.target != target {
            return Err(DomainError::invalid(
                subject,
                "the cited receipt targets a different aggregate",
            ));
        }
        Ok(())
    }
}

closed_enum! {
    /// How far a command has got.
    CommandReceiptState, "CommandReceiptState" {
        /// The intent is durably recorded; nothing has been sent.
        IntentPersisted => "intent_persisted",
        /// The outbox entry is claimed and about to be sent.
        DispatchPending => "dispatch_pending",
        /// It was sent.
        Dispatched => "dispatched",
        /// The target acknowledged receipt. This is *not* completion.
        Acknowledged => "acknowledged",
        /// The dispatch result is unknown; retry is forbidden until a
        /// correlation lookup proves the command had no effect.
        ConfirmationUnknown => "confirmation_unknown",
        /// The effect was independently confirmed.
        Confirmed => "confirmed",
        /// It failed, with evidence.
        Failed => "failed",
    }
}

impl CommandReceiptState {
    /// Whether the state is final.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Failed)
    }

    /// Whether `next` is a legal successor.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::IntentPersisted, Self::DispatchPending | Self::Failed)
                | (
                    Self::DispatchPending,
                    Self::Dispatched | Self::ConfirmationUnknown | Self::Failed
                )
                | (
                    Self::Dispatched,
                    Self::Acknowledged | Self::ConfirmationUnknown | Self::Confirmed | Self::Failed
                )
                | (
                    Self::Acknowledged,
                    Self::Confirmed | Self::ConfirmationUnknown | Self::Failed
                )
                | (
                    Self::ConfirmationUnknown,
                    Self::Confirmed | Self::Failed | Self::DispatchPending
                )
        )
    }
}

/// Proof that a command with an unknown result had no effect.
///
/// It cites the correlation the dispatcher recorded and the native identity that
/// was searched for, so "we did not find it" is evidence rather than an
/// assumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoEffectEvidence {
    /// The correlation recorded with the original dispatch.
    pub correlation: ExternalId,
    /// The native identity that was searched for, if the runtime has one.
    pub searched_identity: Option<NativeRuntimeIdentity>,
    /// When the lookup ran.
    pub reconciled_at: Timestamp,
    /// Digest of the lookup evidence.
    pub evidence_hash: ContentHash,
}

/// The durable record of one command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReceipt {
    /// This receipt's id.
    pub id: CommandReceiptId,
    /// The project that owns it.
    pub project_id: ProjectId,
    /// The caller's idempotency key. Unique across the database.
    pub idempotency_key: IdempotencyKey,
    /// What was asked for.
    pub kind: CommandKind,
    /// Which aggregate it targets.
    pub target: AggregateRef,
    /// The aggregate revision the intent was computed against.
    pub target_revision: AggregateRevision,
    /// The canonical intent document, stored byte-for-byte with its digest.
    pub intent: CanonicalDocument,
    /// How far it has got.
    pub state: CommandReceiptState,
    /// The dispatcher's correlation token.
    pub correlation: Option<ExternalId>,
    /// The native identity the command created or addressed.
    pub native_identity: Option<NativeRuntimeIdentity>,
    /// Reference to the recorded result or failure evidence.
    pub result_ref: Option<ExternalId>,
    /// How many dispatch attempts have been made.
    pub attempts: u32,
    /// When the intent was recorded.
    pub created_at: Timestamp,
    /// When the receipt last changed.
    pub updated_at: Timestamp,
}

impl CommandReceipt {
    /// Whether replaying `idempotency_key` with this intent is the *same*
    /// command.
    ///
    /// Byte-identical intent against the same target is a replay and must return
    /// the original receipt. Anything else is a different command wearing a used
    /// key, and must fail.
    #[must_use]
    pub fn is_replay_of(&self, target: &AggregateRef, intent: &CanonicalDocument) -> bool {
        &self.target == target && self.intent.hash() == intent.hash()
    }

    /// Guard a replay of this receipt's idempotency key.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] when the key is reused with a different
    /// target or a different intent.
    pub fn ensure_replay(
        &self,
        target: &AggregateRef,
        intent: &CanonicalDocument,
    ) -> DomainResult<()> {
        if self.is_replay_of(target, intent) {
            Ok(())
        } else {
            Err(DomainError::invalid(
                "CommandReceipt",
                "idempotency key reused with a different target or intent",
            ))
        }
    }

    /// Move the receipt to `next`.
    ///
    /// # Errors
    /// * [`DomainError::Terminal`] when the receipt is already final.
    /// * [`DomainError::IllegalTransition`] for an illegal pair.
    /// * [`DomainError::MissingEvidence`] when leaving
    ///   [`CommandReceiptState::ConfirmationUnknown`] towards another dispatch
    ///   without proof of no effect — that path exists only via
    ///   [`CommandReceipt::authorize_retry`].
    pub fn transition(&self, next: CommandReceiptState) -> DomainResult<CommandReceiptState> {
        if self.state.is_terminal() {
            return Err(DomainError::Terminal {
                subject: "command receipt",
            });
        }
        if self.state == CommandReceiptState::ConfirmationUnknown
            && next == CommandReceiptState::DispatchPending
        {
            return Err(DomainError::MissingEvidence {
                subject: "command retry",
                rule: "an unknown dispatch result must be reconciled before retrying",
            });
        }
        if !self.state.can_transition_to(next) {
            return Err(DomainError::IllegalTransition {
                subject: "command receipt",
                from: self.state.as_str(),
                to: next.as_str(),
            });
        }
        Ok(next)
    }

    /// Authorize one more dispatch after an unknown result.
    ///
    /// # Errors
    /// * [`DomainError::IllegalTransition`] when the receipt is not in
    ///   [`CommandReceiptState::ConfirmationUnknown`].
    /// * [`DomainError::MissingEvidence`] when the evidence does not cite the
    ///   correlation this receipt actually dispatched with.
    pub fn authorize_retry(
        &self,
        evidence: &NoEffectEvidence,
    ) -> DomainResult<CommandReceiptState> {
        if self.state != CommandReceiptState::ConfirmationUnknown {
            return Err(DomainError::IllegalTransition {
                subject: "command receipt",
                from: self.state.as_str(),
                to: CommandReceiptState::DispatchPending.as_str(),
            });
        }
        let correlation = self
            .correlation
            .as_ref()
            .ok_or(DomainError::MissingEvidence {
                subject: "command retry",
                rule: "the original dispatch recorded no correlation to reconcile against",
            })?;
        if correlation != &evidence.correlation {
            return Err(DomainError::MissingEvidence {
                subject: "command retry",
                rule: "the reconciliation evidence cites a different correlation",
            });
        }
        Ok(CommandReceiptState::DispatchPending)
    }
}

/// The outbox row that carries a command to its target.
///
/// Exactly one entry exists per receipt, and it is inserted in the same
/// transaction as the desired state and the intent event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutboxEntry {
    /// The receipt this entry belongs to.
    pub receipt_id: CommandReceiptId,
    /// The canonical dispatch payload, stored byte-for-byte with its digest.
    pub payload: CanonicalDocument,
    /// The earliest instant it may be dispatched.
    pub not_before: Timestamp,
    /// When it was dispatched, if it has been.
    pub dispatched_at: Option<Timestamp>,
    /// How many attempts have been made.
    pub attempts: u32,
}
