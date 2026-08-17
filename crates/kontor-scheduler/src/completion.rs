//! Configurable epic-completion compilation and execution.
//!
//! A completion profile is compiled into ordinary task/team/gate/receipt
//! nodes. [`advance`] is a pure, revision-checked state machine over observed
//! evidence; the daemon/store layer persists the returned state and executes
//! its commands. No runtime, Committee implementation, clock, filesystem, or
//! external command is called here.

use std::collections::BTreeSet;

use kontor_core::id::{AggregateRevision, ContentHash, ExternalName, SeatBindingId, SpecVersion};
use kontor_core::{DomainError, DomainResult};
use kontor_policy::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, NeedsHumanPayload, TicketEvidence,
    TicketGateBlocker, TicketRequirement, closeout_blockers, ticket_gate_blockers,
};
use serde::{Deserialize, Serialize};

/// A bounded polling fallback, used only when callbacks are unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollingFallback {
    /// Maximum consecutive polls while one phase makes no progress.
    pub max_attempts: u8,
}

/// One immutable Completion Profile definition.
///
/// This is the strict wire spec `completion-profiles:preview` and
/// `completion-profiles:apply` decode a caller's `definition` into. Unknown
/// fields are refused *before* the definition is hashed, so a caller cannot
/// smuggle an unmodelled key past validation and then have it counted in the
/// preview hash the apply is compared against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionProfile {
    /// Stable logical profile id.
    pub id: ExternalName,
    /// Immutable revision.
    pub version: SpecVersion,
    /// Human label.
    pub name: ExternalName,
    /// Integration Team template reference.
    pub integration_team: ExternalName,
    /// Committee template reference.
    pub verdict_committee: ExternalName,
    /// Number of remediation rounds allowed after a failed verdict.
    pub max_remediation_rounds: u8,
    /// Optional polling fallback for runtimes without callbacks.
    pub polling_fallback: Option<PollingFallback>,
}

impl CompletionProfile {
    fn validate(&self) -> DomainResult<()> {
        if self
            .polling_fallback
            .is_some_and(|fallback| fallback.max_attempts == 0)
        {
            return Err(DomainError::invalid(
                "completion profile polling fallback",
                "max_attempts must be positive when polling is declared",
            ));
        }
        Ok(())
    }
}

/// The Operational MVP's seeded profile.
///
/// # Errors
/// Only if a compile-time seed stops satisfying the domain's validated string
/// rules.
pub fn operational_default() -> DomainResult<CompletionProfile> {
    Ok(CompletionProfile {
        id: ExternalName::parse("operational_default")?,
        version: SpecVersion::FIRST,
        name: ExternalName::parse("Operational default")?,
        integration_team: ExternalName::parse("Team C")?,
        verdict_committee: ExternalName::parse("independent_review@1")?,
        max_remediation_rounds: 1,
        polling_fallback: None,
    })
}

/// Stable identity of one compiled node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CompletionNodeKey {
    /// Verify every declared ticket goal/evidence item.
    Tickets,
    /// Run polyrepo integration.
    Integration,
    /// Obtain the Committee verdict for this round (one-based).
    Verdict(u8),
    /// Run the authorized remediation round (one-based).
    Remediation(u8),
    /// Record one closeout prerequisite.
    Closeout(CloseoutRequirement),
    /// Successful terminal node.
    Done,
    /// Human decision terminal node.
    NeedsHuman,
}

/// The existing execution port a compiled node targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionNodeKind {
    /// Existing task/evidence gate.
    TaskGate,
    /// Existing TeamRun port.
    TeamRun {
        /// Pinned team template.
        team: ExternalName,
    },
    /// Existing Committee/gate port.
    CommitteeGate {
        /// Pinned Committee template.
        committee: ExternalName,
        /// One-based verdict round.
        round: u8,
    },
    /// LSA proposal plus TPM-routed remediation TeamRun.
    Remediation {
        /// One-based remediation round.
        round: u8,
    },
    /// Existing typed operator/native-connector receipt port.
    Receipt(CloseoutRequirement),
    /// Successful terminal projection.
    Done,
    /// Human-attention terminal projection.
    NeedsHuman,
}

/// Condition carried by one compiled dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionEdgeCondition {
    /// The source completed successfully.
    Success,
    /// The source Committee passed.
    Pass,
    /// The source Committee failed.
    Fail,
}

/// One node in a compiled Completion DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionNode {
    /// Stable node key.
    pub key: CompletionNodeKey,
    /// Existing execution port to use.
    pub kind: CompletionNodeKind,
}

/// One conditional dependency in a compiled Completion DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionEdge {
    /// Source node.
    pub from: CompletionNodeKey,
    /// Target node.
    pub to: CompletionNodeKey,
    /// Condition that activates the edge.
    pub condition: CompletionEdgeCondition,
}

/// An immutable compiled Completion Profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledCompletion {
    /// Source definition.
    pub profile: CompletionProfile,
    /// Digest of the canonical source definition.
    pub definition_hash: ContentHash,
    /// Nodes in stable execution order.
    pub nodes: Vec<CompletionNode>,
    /// Conditional dependencies in stable order.
    pub edges: Vec<CompletionEdge>,
}

/// Compile a Completion Profile into ordinary execution ports.
///
/// # Errors
/// Refuses an invalid polling declaration or an impossible round count.
pub fn compile(profile: CompletionProfile) -> DomainResult<CompiledCompletion> {
    profile.validate()?;
    let definition = serde_json::to_vec(&profile).map_err(|_| {
        DomainError::invalid(
            "completion profile",
            "the validated definition must serialize canonically",
        )
    })?;
    let mut nodes = vec![
        CompletionNode {
            key: CompletionNodeKey::Tickets,
            kind: CompletionNodeKind::TaskGate,
        },
        CompletionNode {
            key: CompletionNodeKey::Integration,
            kind: CompletionNodeKind::TeamRun {
                team: profile.integration_team.clone(),
            },
        },
        CompletionNode {
            key: CompletionNodeKey::Verdict(1),
            kind: CompletionNodeKind::CommitteeGate {
                committee: profile.verdict_committee.clone(),
                round: 1,
            },
        },
    ];
    let mut edges = vec![
        CompletionEdge {
            from: CompletionNodeKey::Tickets,
            to: CompletionNodeKey::Integration,
            condition: CompletionEdgeCondition::Success,
        },
        CompletionEdge {
            from: CompletionNodeKey::Integration,
            to: CompletionNodeKey::Verdict(1),
            condition: CompletionEdgeCondition::Success,
        },
    ];

    for remediation_round in 1..=profile.max_remediation_rounds {
        let verdict_round = remediation_round.checked_add(1).ok_or_else(|| {
            DomainError::invalid("completion profile", "the verdict round overflowed")
        })?;
        nodes.push(CompletionNode {
            key: CompletionNodeKey::Remediation(remediation_round),
            kind: CompletionNodeKind::Remediation {
                round: remediation_round,
            },
        });
        nodes.push(CompletionNode {
            key: CompletionNodeKey::Verdict(verdict_round),
            kind: CompletionNodeKind::CommitteeGate {
                committee: profile.verdict_committee.clone(),
                round: verdict_round,
            },
        });
        edges.push(CompletionEdge {
            from: CompletionNodeKey::Verdict(remediation_round),
            to: CompletionNodeKey::Remediation(remediation_round),
            condition: CompletionEdgeCondition::Fail,
        });
        edges.push(CompletionEdge {
            from: CompletionNodeKey::Remediation(remediation_round),
            to: CompletionNodeKey::Verdict(verdict_round),
            condition: CompletionEdgeCondition::Success,
        });
    }

    for verdict_round in 1..=profile
        .max_remediation_rounds
        .checked_add(1)
        .ok_or_else(|| DomainError::invalid("completion profile", "the verdict round overflowed"))?
    {
        edges.push(CompletionEdge {
            from: CompletionNodeKey::Verdict(verdict_round),
            to: CompletionNodeKey::Closeout(CloseoutRequirement::Merge),
            condition: CompletionEdgeCondition::Pass,
        });
    }
    let final_verdict = profile
        .max_remediation_rounds
        .checked_add(1)
        .ok_or_else(|| {
            DomainError::invalid("completion profile", "the verdict round overflowed")
        })?;
    edges.push(CompletionEdge {
        from: CompletionNodeKey::Verdict(final_verdict),
        to: CompletionNodeKey::NeedsHuman,
        condition: CompletionEdgeCondition::Fail,
    });

    for requirement in CloseoutRequirement::ALL {
        nodes.push(CompletionNode {
            key: CompletionNodeKey::Closeout(requirement),
            kind: CompletionNodeKind::Receipt(requirement),
        });
    }
    for pair in CloseoutRequirement::ALL.windows(2) {
        edges.push(CompletionEdge {
            from: CompletionNodeKey::Closeout(pair[0]),
            to: CompletionNodeKey::Closeout(pair[1]),
            condition: CompletionEdgeCondition::Success,
        });
    }
    nodes.extend([
        CompletionNode {
            key: CompletionNodeKey::Done,
            kind: CompletionNodeKind::Done,
        },
        CompletionNode {
            key: CompletionNodeKey::NeedsHuman,
            kind: CompletionNodeKind::NeedsHuman,
        },
    ]);
    edges.push(CompletionEdge {
        from: CompletionNodeKey::Closeout(CloseoutRequirement::Archive),
        to: CompletionNodeKey::Done,
        condition: CompletionEdgeCondition::Success,
    });

    Ok(CompiledCompletion {
        profile,
        definition_hash: ContentHash::of(&definition),
        nodes,
        edges,
    })
}

/// The frozen profile identity attached to one completion run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionProfileRef {
    /// Logical id.
    pub id: ExternalName,
    /// Pinned version.
    pub version: SpecVersion,
    /// Frozen human label.
    pub name: ExternalName,
    /// Canonical definition digest.
    pub definition_hash: ContentHash,
}

/// Current state of the completion machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionPhase {
    /// Waiting for every ticket goal/evidence item.
    Tickets,
    /// Waiting for Team C integration evidence.
    Integration,
    /// Waiting for one Committee verdict.
    Verdict(u8),
    /// Waiting for LSA proposal plus TPM routing.
    AwaitRemediation(u8),
    /// The approved remediation TeamRun is in flight.
    Remediating(u8),
    /// Waiting for closeout receipts.
    Closeout,
    /// Successfully complete.
    Done,
    /// Human input is required.
    NeedsHuman,
}

impl CompletionPhase {
    /// Whether the state is terminal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::NeedsHuman)
    }
}

/// One repository's integration outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryOutcome {
    /// Repository/module name.
    pub repository: ExternalName,
    /// Pull-request or equivalent integration reference.
    pub pull_request: ExternalName,
    /// Delivered module revision.
    pub module_revision: ExternalName,
    /// Root-pointer revision when this module has one.
    pub root_pointer_revision: Option<ExternalName>,
}

/// One durable integration result (initial or remediation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationRecord {
    /// Receipt for the integration TeamRun/result.
    pub receipt: ContentHash,
    /// Per-repository results; no single branch is assumed.
    pub repositories: Vec<RepositoryOutcome>,
}

/// Final Committee result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitteeVerdict {
    /// Approved.
    Pass,
    /// Rejected.
    Fail,
}

/// One immutable Committee round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRound {
    /// One-based round.
    pub round: u8,
    /// Typed verdict.
    pub verdict: CommitteeVerdict,
    /// Durable finding/evidence digest.
    pub evidence: ContentHash,
    /// Roles and consultation path used by the round.
    pub deliberation: Vec<DeliberationStep>,
}

/// The two-authority remediation approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationAuthorization {
    /// LSA proposal receipt.
    pub lsa_proposal: ContentHash,
    /// TPM next-round routing receipt.
    pub tpm_routing: ContentHash,
}

/// One completed remediation round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationRecord {
    /// One-based round.
    pub round: u8,
    /// Authority that allowed launch.
    pub authorization: RemediationAuthorization,
    /// Polyrepo result.
    pub integration: IntegrationRecord,
}

/// Durable completion-run state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionState {
    /// Pinned profile.
    pub profile: CompletionProfileRef,
    /// The existing epic TPM seat to wake/reuse.
    pub tpm_seat_id: SeatBindingId,
    /// Current phase.
    pub phase: CompletionPhase,
    /// Ticket contract frozen at start.
    pub ticket_requirements: Vec<TicketRequirement>,
    /// Current ticket evidence.
    pub ticket_evidence: Vec<TicketEvidence>,
    /// Initial and remediation integration results.
    pub integrations: Vec<IntegrationRecord>,
    /// Immutable Committee rounds.
    pub rounds: Vec<CompletionRound>,
    /// Completed remediation rounds.
    pub remediations: Vec<RemediationRecord>,
    /// Approved remediation waiting for its integration result.
    pub pending_remediation: Option<RemediationAuthorization>,
    /// Closeout evidence accumulated so far.
    pub closeout: CloseoutEvidence,
    /// Mandatory human-attention payload, only in that terminal state.
    pub needs_human: Option<NeedsHumanPayload>,
    /// Signal ids already applied; a replay produces no second wake/effect.
    pub handled_signals: BTreeSet<ContentHash>,
    /// Consecutive polling attempts in the current phase.
    pub polling_attempts: u8,
    /// Optimistic-concurrency revision.
    pub revision: AggregateRevision,
}

/// Start one completion run against an immutable compiler result.
///
/// # Errors
/// Refuses duplicate ticket declarations.
pub fn start(
    compiled: &CompiledCompletion,
    tpm_seat_id: SeatBindingId,
    ticket_requirements: Vec<TicketRequirement>,
) -> DomainResult<CompletionState> {
    let _ = ticket_gate_blockers(&ticket_requirements, &[])?;
    Ok(CompletionState {
        profile: CompletionProfileRef {
            id: compiled.profile.id.clone(),
            version: compiled.profile.version,
            name: compiled.profile.name.clone(),
            definition_hash: compiled.definition_hash.clone(),
        },
        tpm_seat_id,
        phase: CompletionPhase::Tickets,
        ticket_requirements,
        ticket_evidence: Vec::new(),
        integrations: Vec::new(),
        rounds: Vec::new(),
        remediations: Vec::new(),
        pending_remediation: None,
        closeout: CloseoutEvidence::default(),
        needs_human: None,
        handled_signals: BTreeSet::new(),
        polling_attempts: 0,
        revision: AggregateRevision::INITIAL,
    })
}

/// How a completion observation reached the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalDelivery {
    /// Runtime callback/event notification.
    Callback,
    /// Declared fallback for a runtime without callbacks.
    Polling,
}

/// One observed completion fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionObservation {
    /// Current durable evidence for every ticket.
    TicketsClosed(Vec<TicketEvidence>),
    /// Initial integration TeamRun completed.
    IntegrationCompleted(IntegrationRecord),
    /// Committee verdict and independent-finding path.
    VerdictRecorded {
        /// One-based round.
        round: u8,
        /// Typed verdict.
        verdict: CommitteeVerdict,
        /// Finding/evidence digest delivered to the LSA on failure.
        evidence: ContentHash,
        /// Roles/consultations that produced the verdict.
        deliberation: Vec<DeliberationStep>,
    },
    /// LSA proposed and TPM routed the next remediation round.
    RemediationApproved(RemediationApproval),
    /// The approved remediation TeamRun completed.
    RemediationCompleted(IntegrationRecord),
    /// New merge/release/version/summary/notify/archive evidence.
    CloseoutRecorded(CloseoutEvidence),
    /// Wake/reconcile attention without changing workflow evidence.
    Attention,
    /// Any stalling path may explicitly enter human attention.
    Stalled(NeedsHumanPayload),
}

/// Remediation approval tied to its one-based round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationApproval {
    /// One-based remediation round.
    pub round: u8,
    /// Required LSA/TPM authority receipts.
    pub authorization: RemediationAuthorization,
}

/// One idempotent, revision-checked completion signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionSignal {
    /// Stable event/observation digest.
    pub id: ContentHash,
    /// Revision the caller observed.
    pub expected_revision: AggregateRevision,
    /// Callback or declared polling fallback.
    pub delivery: SignalDelivery,
    /// Observed fact.
    pub observation: CompletionObservation,
}

/// Effect requested from existing service ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionCommand {
    /// Start the pinned integration TeamRun.
    StartIntegration {
        /// Pinned team template.
        team: ExternalName,
    },
    /// Invoke the pinned Committee template.
    InvokeCommittee {
        /// Pinned Committee template.
        committee: ExternalName,
        /// One-based round.
        round: u8,
    },
    /// Deliver failed Committee evidence to the LSA.
    DeliverFailureToLsa {
        /// Failed round.
        round: u8,
        /// Finding/evidence digest.
        evidence: ContentHash,
    },
    /// Launch one authorized remediation TeamRun.
    LaunchRemediation {
        /// One-based round.
        round: u8,
        /// LSA + TPM authorization.
        authorization: RemediationAuthorization,
    },
    /// Wake/reuse the existing epic TPM seat.
    WakeTpm {
        /// Existing seat binding; never a newly-created seat.
        seat_binding_id: SeatBindingId,
    },
    /// Schedule the next bounded fallback poll.
    SchedulePoll {
        /// Attempt number in this phase.
        attempt: u8,
        /// Declared phase-local bound.
        max_attempts: u8,
    },
    /// Mark the completion aggregate done.
    MarkDone,
}

/// Result of applying one signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionTransition {
    /// New durable state (or the identical state on replay).
    pub state: CompletionState,
    /// Commands to execute through existing ports.
    pub commands: Vec<CompletionCommand>,
    /// Whether the signal had already been applied.
    pub replayed: bool,
}

/// Apply one completion observation.
///
/// # Errors
/// Refuses stale revisions, wrong-phase observations, incomplete ticket gates,
/// malformed integration/Committee evidence, unapproved remediation, evidence
/// conflicts, and changes to terminal state.
pub fn advance(
    compiled: &CompiledCompletion,
    current: &CompletionState,
    signal: &CompletionSignal,
) -> DomainResult<CompletionTransition> {
    if current.handled_signals.contains(&signal.id) {
        return Ok(CompletionTransition {
            state: current.clone(),
            commands: Vec::new(),
            replayed: true,
        });
    }
    if current.phase.is_terminal() {
        return Err(DomainError::Terminal {
            subject: "completion",
        });
    }
    current
        .revision
        .expect("completion", signal.expected_revision)?;
    if current.profile.definition_hash != compiled.definition_hash {
        return Err(DomainError::invalid(
            "completion",
            "the compiled profile does not match the pinned definition",
        ));
    }

    let previous_phase = current.phase;
    let mut next = current.clone();
    let mut commands = Vec::new();
    match (&signal.observation, current.phase) {
        (CompletionObservation::TicketsClosed(evidence), CompletionPhase::Tickets) => {
            let blockers = ticket_gate_blockers(&current.ticket_requirements, evidence)?;
            if !blockers.is_empty() {
                return Err(DomainError::MissingEvidence {
                    subject: "completion ticket gate",
                    rule: "every declared ticket goal and evidence item must be complete",
                });
            }
            next.ticket_evidence.clone_from(evidence);
            next.phase = CompletionPhase::Integration;
            commands.push(CompletionCommand::StartIntegration {
                team: compiled.profile.integration_team.clone(),
            });
        }
        (
            CompletionObservation::IntegrationCompleted(integration),
            CompletionPhase::Integration,
        ) => {
            validate_integration(integration)?;
            next.integrations.push(integration.clone());
            next.phase = CompletionPhase::Verdict(1);
            commands.push(CompletionCommand::InvokeCommittee {
                committee: compiled.profile.verdict_committee.clone(),
                round: 1,
            });
        }
        (
            CompletionObservation::VerdictRecorded {
                round,
                verdict,
                evidence,
                deliberation,
            },
            CompletionPhase::Verdict(expected_round),
        ) if *round == expected_round => {
            if deliberation.is_empty() {
                return Err(DomainError::MissingEvidence {
                    subject: "completion verdict",
                    rule: "the independent finding path must name its roles and consultation",
                });
            }
            next.rounds.push(CompletionRound {
                round: *round,
                verdict: *verdict,
                evidence: evidence.clone(),
                deliberation: deliberation.clone(),
            });
            match verdict {
                CommitteeVerdict::Pass => next.phase = CompletionPhase::Closeout,
                CommitteeVerdict::Fail => {
                    commands.push(CompletionCommand::DeliverFailureToLsa {
                        round: *round,
                        evidence: evidence.clone(),
                    });
                    if *round <= compiled.profile.max_remediation_rounds {
                        next.phase = CompletionPhase::AwaitRemediation(*round);
                    } else {
                        enter_failed_verdict_needs_human(&mut next)?;
                    }
                }
            }
        }
        (
            CompletionObservation::RemediationApproved(approval),
            CompletionPhase::AwaitRemediation(expected_round),
        ) if approval.round == expected_round => {
            next.pending_remediation = Some(approval.authorization.clone());
            next.phase = CompletionPhase::Remediating(expected_round);
            commands.push(CompletionCommand::LaunchRemediation {
                round: expected_round,
                authorization: approval.authorization.clone(),
            });
        }
        (
            CompletionObservation::RemediationCompleted(integration),
            CompletionPhase::Remediating(round),
        ) => {
            validate_integration(integration)?;
            let authorization =
                next.pending_remediation
                    .take()
                    .ok_or(DomainError::MissingAuthority {
                        subject: "completion remediation",
                        rule: "LSA proposal and TPM routing receipts are required before launch",
                    })?;
            next.integrations.push(integration.clone());
            next.remediations.push(RemediationRecord {
                round,
                authorization,
                integration: integration.clone(),
            });
            let verdict_round = round.checked_add(1).ok_or_else(|| {
                DomainError::invalid("completion remediation", "the verdict round overflowed")
            })?;
            next.phase = CompletionPhase::Verdict(verdict_round);
            commands.push(CompletionCommand::InvokeCommittee {
                committee: compiled.profile.verdict_committee.clone(),
                round: verdict_round,
            });
        }
        (CompletionObservation::CloseoutRecorded(evidence), CompletionPhase::Closeout) => {
            merge_closeout(&mut next.closeout, evidence)?;
            if closeout_blockers(&next.closeout).is_empty() {
                next.phase = CompletionPhase::Done;
                commands.push(CompletionCommand::MarkDone);
            }
        }
        (CompletionObservation::Attention, _) => {}
        (CompletionObservation::Stalled(payload), _) => {
            next.phase = CompletionPhase::NeedsHuman;
            next.needs_human = Some(payload.clone());
        }
        _ => {
            return Err(DomainError::IllegalTransition {
                subject: "completion",
                from: phase_name(current.phase),
                to: observation_name(&signal.observation),
            });
        }
    }

    next.handled_signals.insert(signal.id.clone());
    next.revision = next.revision.next()?;
    apply_delivery(
        compiled,
        &mut next,
        previous_phase,
        signal.delivery,
        &mut commands,
    )?;
    Ok(CompletionTransition {
        state: next,
        commands,
        replayed: false,
    })
}

/// One typed reason completion cannot leave the phase it stands in.
///
/// The read projection reports these rather than rendered strings: a caller
/// deciding *which* missing thing to go and produce needs the task, round or
/// receipt kind as data, and a phrase it has to parse is not data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionBlocker {
    /// A declared ticket goal, artifact or gate is absent or failing.
    Ticket(TicketGateBlocker),
    /// The pinned integration TeamRun has not reported its outcome.
    ///
    /// Which template is pinned is already readable from the profile revision
    /// this completion froze, so it is not restated per blocker.
    IntegrationTeamRun,
    /// One Committee round has not settled a typed aggregate verdict.
    CommitteeVerdict {
        /// One-based round.
        round: u8,
    },
    /// The LSA proposal and TPM route are not both durable yet.
    RemediationAuthorization {
        /// One-based remediation round.
        round: u8,
    },
    /// The authorized remediation TeamRun has not reported its outcome.
    RemediationResult {
        /// One-based remediation round.
        round: u8,
    },
    /// One fixed closeout receipt is still missing.
    Closeout(CloseoutRequirement),
}

/// Stable typed blockers for the read projection.
///
/// # Errors
/// Only if persisted ticket declarations/evidence violate their uniqueness
/// contract.
pub fn blockers(state: &CompletionState) -> DomainResult<Vec<CompletionBlocker>> {
    match state.phase {
        CompletionPhase::Tickets => Ok(ticket_gate_blockers(
            &state.ticket_requirements,
            &state.ticket_evidence,
        )?
        .into_iter()
        .map(CompletionBlocker::Ticket)
        .collect()),
        CompletionPhase::Integration => Ok(vec![CompletionBlocker::IntegrationTeamRun]),
        CompletionPhase::Verdict(round) => Ok(vec![CompletionBlocker::CommitteeVerdict { round }]),
        CompletionPhase::AwaitRemediation(round) => {
            Ok(vec![CompletionBlocker::RemediationAuthorization { round }])
        }
        CompletionPhase::Remediating(round) => {
            Ok(vec![CompletionBlocker::RemediationResult { round }])
        }
        CompletionPhase::Closeout => Ok(closeout_blockers(&state.closeout)
            .into_iter()
            .map(CompletionBlocker::Closeout)
            .collect()),
        CompletionPhase::Done | CompletionPhase::NeedsHuman => Ok(Vec::new()),
    }
}

/// Stable outstanding items, rendered from [`blockers`].
///
/// One projection computes what is missing and this one only renders it, so a
/// phase can never report a blocker in one form and not the other.
///
/// # Errors
/// Only if persisted ticket declarations/evidence violate their uniqueness
/// contract.
pub fn outstanding(state: &CompletionState) -> DomainResult<Vec<String>> {
    Ok(blockers(state)?
        .into_iter()
        .map(|blocker| match blocker {
            CompletionBlocker::Ticket(ticket) => ticket_blocker_text(ticket),
            CompletionBlocker::IntegrationTeamRun => "integration_team_run".to_owned(),
            CompletionBlocker::CommitteeVerdict { round } => {
                format!("committee_verdict_round_{round}")
            }
            CompletionBlocker::RemediationAuthorization { round } => {
                format!("remediation_authorization_round_{round}")
            }
            CompletionBlocker::RemediationResult { round } => {
                format!("remediation_result_round_{round}")
            }
            CompletionBlocker::Closeout(requirement) => requirement.as_str().to_owned(),
        })
        .collect())
}

fn validate_integration(integration: &IntegrationRecord) -> DomainResult<()> {
    if integration.repositories.is_empty() {
        return Err(DomainError::MissingEvidence {
            subject: "completion integration",
            rule: "at least one repository outcome is required",
        });
    }
    let unique: BTreeSet<_> = integration
        .repositories
        .iter()
        .map(|outcome| &outcome.repository)
        .collect();
    if unique.len() != integration.repositories.len() {
        return Err(DomainError::invalid(
            "completion integration",
            "each repository may have only one outcome per round",
        ));
    }
    Ok(())
}

fn enter_failed_verdict_needs_human(state: &mut CompletionState) -> DomainResult<()> {
    let path = state
        .rounds
        .iter()
        .flat_map(|round| round.deliberation.iter().cloned())
        .collect();
    state.needs_human = Some(NeedsHumanPayload::new(
        ExternalName::parse(
            "Review the unresolved Committee evidence with the LSA and TPM before continuing",
        )?,
        path,
    )?);
    state.phase = CompletionPhase::NeedsHuman;
    Ok(())
}

fn apply_delivery(
    compiled: &CompiledCompletion,
    state: &mut CompletionState,
    previous_phase: CompletionPhase,
    delivery: SignalDelivery,
    commands: &mut Vec<CompletionCommand>,
) -> DomainResult<()> {
    if delivery == SignalDelivery::Callback {
        commands.push(CompletionCommand::WakeTpm {
            seat_binding_id: state.tpm_seat_id,
        });
        return Ok(());
    }
    if state.phase.is_terminal() {
        return Ok(());
    }
    if state.phase != previous_phase {
        state.polling_attempts = 0;
    }
    let Some(fallback) = compiled.profile.polling_fallback else {
        return enter_polling_needs_human(state, "no bounded polling fallback was declared");
    };
    if state.polling_attempts >= fallback.max_attempts {
        return enter_polling_needs_human(state, "the bounded polling fallback was exhausted");
    }
    state.polling_attempts = state.polling_attempts.checked_add(1).ok_or_else(|| {
        DomainError::invalid(
            "completion polling",
            "the polling attempt counter overflowed",
        )
    })?;
    commands.push(CompletionCommand::SchedulePoll {
        attempt: state.polling_attempts,
        max_attempts: fallback.max_attempts,
    });
    Ok(())
}

fn enter_polling_needs_human(state: &mut CompletionState, outcome: &str) -> DomainResult<()> {
    let round = match state.phase {
        CompletionPhase::Verdict(round)
        | CompletionPhase::AwaitRemediation(round)
        | CompletionPhase::Remediating(round) => round,
        _ => 0,
    };
    state.needs_human = Some(NeedsHumanPayload::new(
        ExternalName::parse(
            "Restore callback delivery or explicitly authorize a new bounded observation path",
        )?,
        vec![DeliberationStep {
            role: ExternalName::parse("TPM")?,
            consultation: ExternalName::parse("bounded polling fallback")?,
            round,
            outcome: ExternalName::parse(outcome)?,
        }],
    )?);
    state.phase = CompletionPhase::NeedsHuman;
    Ok(())
}

fn merge_closeout(target: &mut CloseoutEvidence, incoming: &CloseoutEvidence) -> DomainResult<()> {
    merge_receipt(&mut target.merge_receipt, &incoming.merge_receipt)?;
    merge_receipt(&mut target.release_receipt, &incoming.release_receipt)?;
    merge_receipt(&mut target.summary_receipt, &incoming.summary_receipt)?;
    merge_receipt(
        &mut target.notification_receipt,
        &incoming.notification_receipt,
    )?;
    merge_receipt(&mut target.archive_receipt, &incoming.archive_receipt)?;
    for (service, version) in &incoming.delivered_versions {
        match target.delivered_versions.get(service) {
            Some(stored) if stored != version => {
                return Err(DomainError::invalid(
                    "completion version inventory",
                    "a recorded service version is immutable",
                ));
            }
            Some(_) => {}
            None => {
                target
                    .delivered_versions
                    .insert(service.clone(), version.clone());
            }
        }
    }
    Ok(())
}

fn merge_receipt(
    target: &mut Option<ContentHash>,
    incoming: &Option<ContentHash>,
) -> DomainResult<()> {
    match (&*target, incoming) {
        (Some(stored), Some(candidate)) if stored != candidate => Err(DomainError::invalid(
            "completion closeout receipt",
            "a recorded receipt is immutable",
        )),
        (None, Some(candidate)) => {
            *target = Some(candidate.clone());
            Ok(())
        }
        _ => Ok(()),
    }
}

const fn phase_name(phase: CompletionPhase) -> &'static str {
    match phase {
        CompletionPhase::Tickets => "tickets",
        CompletionPhase::Integration => "integration",
        CompletionPhase::Verdict(_) => "verdict",
        CompletionPhase::AwaitRemediation(_) => "await_remediation",
        CompletionPhase::Remediating(_) => "remediating",
        CompletionPhase::Closeout => "closeout",
        CompletionPhase::Done => "done",
        CompletionPhase::NeedsHuman => "needs_human",
    }
}

const fn observation_name(observation: &CompletionObservation) -> &'static str {
    match observation {
        CompletionObservation::TicketsClosed(_) => "tickets_closed",
        CompletionObservation::IntegrationCompleted(_) => "integration_completed",
        CompletionObservation::VerdictRecorded { .. } => "verdict_recorded",
        CompletionObservation::RemediationApproved(_) => "remediation_approved",
        CompletionObservation::RemediationCompleted(_) => "remediation_completed",
        CompletionObservation::CloseoutRecorded(_) => "closeout_recorded",
        CompletionObservation::Attention => "attention",
        CompletionObservation::Stalled(_) => "needs_human",
    }
}

fn ticket_blocker_text(blocker: TicketGateBlocker) -> String {
    match blocker {
        TicketGateBlocker::MissingTicket(task_id) => format!("ticket:{task_id}"),
        TicketGateBlocker::MissingGoal { task_id, goal } => {
            format!("ticket:{task_id}:goal:{goal}")
        }
        TicketGateBlocker::MissingEvidence { task_id, evidence } => {
            format!("ticket:{task_id}:evidence:{evidence}")
        }
    }
}
