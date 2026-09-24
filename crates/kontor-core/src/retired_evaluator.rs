//! Evidence-only proof for an exact retired evaluator seat.
//!
//! A gate whose evaluator seat was retired by rejection settlement has no live
//! seat to challenge, and the forward-only live-seat challenge must not be
//! widened to reach it: that surface exists to put a *new* question to a seat
//! that can still answer. This is the other half — a read-only judgement that
//! an exact retired seat already rendered its verdict, on evidence that is
//! already durable.
//!
//! It is a pure function on purpose. It holds no store, no adapter and no
//! clock, so it cannot write, cannot dispatch to a native, and cannot reach a
//! runtime. Every fact it compares is one the caller must name *and* the server
//! must have independently derived; the only thing it produces is a digest of
//! the facts that matched.

use crate::id::{
    AgentRunId, AggregateRevision, ArtifactKey, ContentHash, ExternalId, GateKey, ProjectId,
    RoleKey, RoleSlotId, SeatBindingId, TaskId, TeamRunId,
};

/// What the caller asserts about the retired evaluator it wants attested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationClaim {
    /// The realm project the whole claim is scoped to.
    pub project_id: ProjectId,
    /// The task whose gate is being proved.
    pub task_id: TaskId,
    /// The workflow revision the caller read before claiming.
    pub expected_workflow_revision: AggregateRevision,
    /// The exact gate.
    pub gate: GateKey,
    /// The TeamRun the evaluator was admitted on.
    pub team_run_id: TeamRunId,
    /// The catalog role the gate declares as its evaluator.
    pub evaluator_role: RoleKey,
    /// The evaluator's own closed run.
    pub agent_run_id: AgentRunId,
    /// The runtime binding that run held.
    pub runtime_binding_id: ExternalId,
    /// That binding's generation.
    pub runtime_generation: u64,
    /// The native the binding named.
    pub native_id: ExternalId,
    /// The retired topology seat.
    pub seat_binding_id: SeatBindingId,
    /// The seat revision the caller read before claiming.
    pub expected_seat_revision: AggregateRevision,
    /// The artifact the settled turn produced.
    pub artifact: ArtifactKey,
    /// That artifact's checksum.
    pub artifact_checksum: ContentHash,
    /// The digest of the settled turn's immutable evidence.
    pub settled_evidence_digest: ContentHash,
}

/// The retired topology seat, as the server reads it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedSeat {
    /// The exact retired seat.
    pub id: SeatBindingId,
    /// Its revision now.
    pub revision: AggregateRevision,
    /// Retired, as opposed to active or archived. Only retired is attestable:
    /// an active seat can still answer and must be challenged, not attested.
    pub retired: bool,
    /// The slot it was admitted on.
    pub role_slot_id: RoleSlotId,
    /// The catalog role this slot holds, resolved from the frozen definition.
    pub catalog_role: RoleKey,
    /// The task it serves, when it serves one.
    pub task_id: Option<TaskId>,
    /// The TeamRun it serves, when it serves one.
    pub team_run_id: Option<TeamRunId>,
}

/// The runtime binding the evaluator run held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedBinding {
    /// The runtime binding's own id.
    pub id: ExternalId,
    /// The generation it was issued in.
    pub generation: u64,
    /// The native it named.
    pub native_id: ExternalId,
}

/// The evaluator's own run, as the server reads it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRun {
    /// The evaluator's run.
    pub id: AgentRunId,
    /// The slot it was admitted on.
    pub role_slot_id: RoleSlotId,
    /// The TeamRun the task's evaluator sits on.
    pub team_run_id: TeamRunId,
    /// Closed or settled. An open run is not attestable: it can still record
    /// its own verdict, and attesting one would pre-empt it.
    pub closed: bool,
    /// The runtime binding it held, when it held one.
    pub binding: Option<ObservedBinding>,
}

/// The immutable settled-turn evidence the claim rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedSettlement {
    /// The artifact the settled turn produced.
    pub artifact: ArtifactKey,
    /// That artifact's checksum.
    pub artifact_checksum: ContentHash,
    /// The digest of the turn's immutable evidence.
    pub evidence_digest: ContentHash,
}

/// Everything the server derived for itself, never taken from the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedEvaluator {
    /// The realm project this reading is scoped to.
    pub project_id: ProjectId,
    /// The task read.
    pub task_id: TaskId,
    /// Its active workflow's revision now.
    pub workflow_revision: AggregateRevision,
    /// Whether the task's frozen workflow declares the claimed gate at all.
    pub declares_gate: bool,
    /// The catalog roles that gate declares as evaluators.
    pub gate_evaluator_roles: Vec<RoleKey>,
    /// The TeamRun the task's evaluator sits on.
    pub team_run_id: TeamRunId,
    /// The retired seat.
    pub seat: ObservedSeat,
    /// The evaluator's closed run.
    pub run: ObservedRun,
    /// `None` when the turn left no durable settled evidence.
    pub settled: Option<ObservedSettlement>,
}

/// Exactly why a claim was refused. One variant per fence, so a caller is told
/// which fact disagreed and a regression can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationRefusal {
    /// The claim names another realm project.
    ForeignProject,
    /// The claim names another task.
    TaskMismatch,
    /// The workflow moved since the caller read it.
    WorkflowMoved,
    /// The frozen workflow declares no such gate.
    UnknownGate,
    /// The claim names another TeamRun.
    TeamRunMismatch,
    /// The claimed role does not evaluate this gate.
    RoleNotEvaluatorOfGate,
    /// The seat's slot does not hold the claimed evaluator role.
    SlotDoesNotHoldRole,
    /// The claim names another seat.
    SeatMismatch,
    /// The seat moved since the caller read it.
    SeatMoved,
    /// The seat is not retired. An active seat must be challenged instead.
    SeatNotRetired,
    /// The seat does not serve this task and TeamRun.
    SeatNotThisTasksEvaluator,
    /// The claim names another run.
    RunMismatch,
    /// The run was not admitted on this TeamRun.
    RunNotOnThisTeamRun,
    /// The run does not hold the seat's slot.
    RunNotOnThisSlot,
    /// The run is still open.
    RunNotClosed,
    /// The run holds no runtime binding to name.
    BindingMissing,
    /// The binding, its generation or its native disagrees.
    BindingIdentityMismatch,
    /// The turn left no durable settled evidence.
    NoSettledEvidence,
    /// The settled turn produced another artifact.
    ArtifactMismatch,
    /// The artifact's checksum disagrees.
    ChecksumMismatch,
    /// The settled evidence is not the evidence claimed.
    EvidenceChanged,
}

impl AttestationRefusal {
    /// The closed refusal vocabulary, for a typed API answer.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ForeignProject => "foreign_project",
            Self::TaskMismatch => "task_mismatch",
            Self::WorkflowMoved => "workflow_moved",
            Self::UnknownGate => "unknown_gate",
            Self::TeamRunMismatch => "team_run_mismatch",
            Self::RoleNotEvaluatorOfGate => "role_not_evaluator_of_gate",
            Self::SlotDoesNotHoldRole => "slot_does_not_hold_role",
            Self::SeatMismatch => "seat_mismatch",
            Self::SeatMoved => "seat_moved",
            Self::SeatNotRetired => "seat_not_retired",
            Self::SeatNotThisTasksEvaluator => "seat_not_this_tasks_evaluator",
            Self::RunMismatch => "run_mismatch",
            Self::RunNotOnThisTeamRun => "run_not_on_this_team_run",
            Self::RunNotOnThisSlot => "run_not_on_this_slot",
            Self::RunNotClosed => "run_not_closed",
            Self::BindingMissing => "binding_missing",
            Self::BindingIdentityMismatch => "binding_identity_mismatch",
            Self::NoSettledEvidence => "no_settled_evidence",
            Self::ArtifactMismatch => "artifact_mismatch",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::EvidenceChanged => "evidence_changed",
        }
    }
}

/// The proof a matched claim yields.
///
/// [`Self::proof_digest`] is a deterministic digest of every fenced fact. Two
/// calls naming the same facts produce the same digest, which is what lets a
/// receipt be replayed idempotently; a call that changed any fact produces a
/// different one, which is how duplicate-intent drift is caught rather than
/// silently overwriting an earlier proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationProof {
    /// The retired seat this proof is about.
    pub seat_binding_id: SeatBindingId,
    /// The evaluator run it proves.
    pub agent_run_id: AgentRunId,
    /// The gate it proves.
    pub gate: GateKey,
    /// The artifact the settled turn produced.
    pub artifact: ArtifactKey,
    /// The digest of the turn's immutable evidence.
    pub evidence_digest: ContentHash,
    /// The deterministic digest of every fenced fact.
    pub proof_digest: ContentHash,
}

/// Judge one claim against what the server derived.
///
/// # Errors
/// One [`AttestationRefusal`] per disagreeing fact; the first is returned and
/// nothing is produced. A refusal is a no-write answer by construction: this
/// function has nothing to write with.
pub fn judge(
    claim: &AttestationClaim,
    observed: &ObservedEvaluator,
) -> Result<AttestationProof, AttestationRefusal> {
    if claim.project_id != observed.project_id {
        return Err(AttestationRefusal::ForeignProject);
    }
    if claim.task_id != observed.task_id {
        return Err(AttestationRefusal::TaskMismatch);
    }
    if claim.expected_workflow_revision != observed.workflow_revision {
        return Err(AttestationRefusal::WorkflowMoved);
    }
    if !observed.declares_gate {
        return Err(AttestationRefusal::UnknownGate);
    }
    if claim.team_run_id != observed.team_run_id {
        return Err(AttestationRefusal::TeamRunMismatch);
    }
    if !observed
        .gate_evaluator_roles
        .iter()
        .any(|role| role == &claim.evaluator_role)
    {
        return Err(AttestationRefusal::RoleNotEvaluatorOfGate);
    }
    // The seat's slot must *hold* the evaluator role. Comparing the slot id to
    // the role name is the mistake this surface must not repeat.
    if observed.seat.catalog_role != claim.evaluator_role {
        return Err(AttestationRefusal::SlotDoesNotHoldRole);
    }
    if claim.seat_binding_id != observed.seat.id {
        return Err(AttestationRefusal::SeatMismatch);
    }
    if claim.expected_seat_revision != observed.seat.revision {
        return Err(AttestationRefusal::SeatMoved);
    }
    if !observed.seat.retired {
        return Err(AttestationRefusal::SeatNotRetired);
    }
    if observed.seat.task_id != Some(observed.task_id)
        || observed.seat.team_run_id != Some(observed.team_run_id)
    {
        return Err(AttestationRefusal::SeatNotThisTasksEvaluator);
    }
    if claim.agent_run_id != observed.run.id {
        return Err(AttestationRefusal::RunMismatch);
    }
    if observed.run.team_run_id != observed.team_run_id {
        return Err(AttestationRefusal::RunNotOnThisTeamRun);
    }
    if observed.run.role_slot_id != observed.seat.role_slot_id {
        return Err(AttestationRefusal::RunNotOnThisSlot);
    }
    if !observed.run.closed {
        return Err(AttestationRefusal::RunNotClosed);
    }
    let binding = observed
        .run
        .binding
        .as_ref()
        .ok_or(AttestationRefusal::BindingMissing)?;
    if binding.id != claim.runtime_binding_id
        || binding.generation != claim.runtime_generation
        || binding.native_id != claim.native_id
    {
        return Err(AttestationRefusal::BindingIdentityMismatch);
    }
    let settled = observed
        .settled
        .as_ref()
        .ok_or(AttestationRefusal::NoSettledEvidence)?;
    if settled.artifact != claim.artifact {
        return Err(AttestationRefusal::ArtifactMismatch);
    }
    if settled.artifact_checksum != claim.artifact_checksum {
        return Err(AttestationRefusal::ChecksumMismatch);
    }
    if settled.evidence_digest != claim.settled_evidence_digest {
        return Err(AttestationRefusal::EvidenceChanged);
    }
    Ok(AttestationProof {
        seat_binding_id: observed.seat.id,
        agent_run_id: observed.run.id,
        gate: claim.gate.clone(),
        artifact: claim.artifact.clone(),
        evidence_digest: settled.evidence_digest.clone(),
        proof_digest: proof_digest(claim),
    })
}

/// A deterministic digest of every fenced fact, in a fixed order.
fn proof_digest(claim: &AttestationClaim) -> ContentHash {
    let mut material = String::new();
    for field in [
        claim.project_id.to_string(),
        claim.task_id.to_string(),
        claim.expected_workflow_revision.get().to_string(),
        claim.gate.as_str().to_owned(),
        claim.team_run_id.to_string(),
        claim.evaluator_role.as_str().to_owned(),
        claim.agent_run_id.to_string(),
        claim.runtime_binding_id.as_str().to_owned(),
        claim.runtime_generation.to_string(),
        claim.native_id.as_str().to_owned(),
        claim.seat_binding_id.to_string(),
        claim.expected_seat_revision.get().to_string(),
        claim.artifact.as_str().to_owned(),
        claim.artifact_checksum.as_str().to_owned(),
        claim.settled_evidence_digest.as_str().to_owned(),
    ] {
        material.push_str(&field);
        // A separator no id may contain, so two different field splits cannot
        // digest to the same material.
        material.push('\u{1f}');
    }
    ContentHash::of(material.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact ASMA-8119 live shape: the retired audit seat on the settled
    /// candidate, and the authentic re-audit digest that seat rendered.
    const PROJECT: &str = "01a0064a-e056-7603-9968-ef64fdaacb75";
    const TASK: &str = "01a07722-c3ea-7300-9215-6f55309848f2";
    const TEAM_RUN: &str = "01a09f49-0bbd-7402-a0c8-4882dbdfedc3";
    const AUDIT_RUN: &str = "01a0b619-6aea-78d1-9018-ef1f8b166eda";
    const RETIRED_SEAT: &str = "01a09f49-595e-7e50-962a-a2ee14ae76ad";
    const REAUDIT_DIGEST: &str = "227f487700996eea037fcfa25d137e3a9c7bb81bc80254378dda983a0d907af3";

    fn hash(seed: &str) -> ContentHash {
        ContentHash::of(seed.as_bytes())
    }

    fn live_pair() -> (AttestationClaim, ObservedEvaluator) {
        let project_id = ProjectId::parse(PROJECT).expect("a project id");
        let task_id = TaskId::parse(TASK).expect("a task id");
        let team_run_id = TeamRunId::parse(TEAM_RUN).expect("a team run id");
        let agent_run_id = AgentRunId::parse(AUDIT_RUN).expect("a run id");
        let seat_binding_id = SeatBindingId::parse(RETIRED_SEAT).expect("a seat id");
        let evaluator_role = RoleKey::parse("fleet-spec-auditor").expect("a role");
        let role_slot_id = RoleSlotId::parse("audit").expect("a slot");
        let gate = GateKey::parse("high-audit-gate").expect("a gate");
        let artifact = ArtifactKey::parse("high-audit-report").expect("an artifact");
        let evidence = ContentHash::parse(REAUDIT_DIGEST).expect("the authentic digest");
        let checksum = hash("high-audit-report contents");
        let revision = AggregateRevision::parse(4).expect("a revision");
        let seat_revision = AggregateRevision::parse(7).expect("a revision");
        let binding_id = ExternalId::parse("01a09f49-595e-7e50-962a-a2ee14ae76ae").expect("id");
        let native_id = ExternalId::parse("150d6ff3-1474-4200-9600-c39796efc1f7").expect("id");

        let claim = AttestationClaim {
            project_id,
            task_id,
            expected_workflow_revision: revision,
            gate: gate.clone(),
            team_run_id,
            evaluator_role: evaluator_role.clone(),
            agent_run_id,
            runtime_binding_id: binding_id.clone(),
            runtime_generation: 1,
            native_id: native_id.clone(),
            seat_binding_id,
            expected_seat_revision: seat_revision,
            artifact: artifact.clone(),
            artifact_checksum: checksum.clone(),
            settled_evidence_digest: evidence.clone(),
        };
        let observed = ObservedEvaluator {
            project_id,
            task_id,
            workflow_revision: revision,
            declares_gate: true,
            gate_evaluator_roles: vec![evaluator_role.clone()],
            team_run_id,
            seat: ObservedSeat {
                id: seat_binding_id,
                revision: seat_revision,
                retired: true,
                role_slot_id: role_slot_id.clone(),
                catalog_role: evaluator_role,
                task_id: Some(task_id),
                team_run_id: Some(team_run_id),
            },
            run: ObservedRun {
                id: agent_run_id,
                role_slot_id,
                team_run_id,
                closed: true,
                binding: Some(ObservedBinding {
                    id: binding_id,
                    generation: 1,
                    native_id,
                }),
            },
            settled: Some(ObservedSettlement {
                artifact,
                artifact_checksum: checksum,
                evidence_digest: evidence,
            }),
        };
        (claim, observed)
    }

    #[test]
    fn the_exact_live_retired_audit_seat_is_attestable() {
        let (claim, observed) = live_pair();
        let proof = judge(&claim, &observed).expect("the live shape attests");
        assert_eq!(proof.agent_run_id.to_string(), AUDIT_RUN);
        assert_eq!(proof.seat_binding_id.to_string(), RETIRED_SEAT);
        assert_eq!(proof.evidence_digest.as_str(), REAUDIT_DIGEST);
        assert_eq!(proof.artifact.as_str(), "high-audit-report");
    }

    #[test]
    fn the_same_claim_proves_the_same_digest_and_a_changed_one_does_not() {
        let (claim, observed) = live_pair();
        let first = judge(&claim, &observed).expect("attests");
        let replay = judge(&claim, &observed).expect("attests again");
        assert_eq!(
            first.proof_digest, replay.proof_digest,
            "an identical claim replays to one proof, which is what makes a \
             receipt idempotent and a lost acknowledgement safe"
        );

        let mut drifted = claim.clone();
        drifted.artifact_checksum = hash("a different artifact");
        let mut moved = observed.clone();
        moved.settled.as_mut().expect("settled").artifact_checksum =
            drifted.artifact_checksum.clone();
        let drifted_proof = judge(&drifted, &moved).expect("attests on its own facts");
        assert_ne!(
            first.proof_digest, drifted_proof.proof_digest,
            "a claim that changed any fenced fact is a different proof, never a \
             silent overwrite of the first"
        );
    }

    /// Every fence, each proved by perturbing exactly one fact.
    #[test]
    fn each_fence_refuses_on_its_own() {
        type Perturb = fn(&mut AttestationClaim, &mut ObservedEvaluator);
        let cases: Vec<(&str, AttestationRefusal, Perturb)> = vec![
            (
                "foreign project",
                AttestationRefusal::ForeignProject,
                |_, o| {
                    o.project_id = ProjectId::parse("01a0064a-e056-7603-9968-ef64fdaacb76")
                        .expect("another project");
                },
            ),
            ("another task", AttestationRefusal::TaskMismatch, |_, o| {
                o.task_id = TaskId::parse("01a07722-c3ea-7300-9215-6f55309848f3").expect("other");
            }),
            (
                "workflow moved",
                AttestationRefusal::WorkflowMoved,
                |_, o| {
                    o.workflow_revision = AggregateRevision::parse(5).expect("a revision");
                },
            ),
            ("unknown gate", AttestationRefusal::UnknownGate, |_, o| {
                o.declares_gate = false;
            }),
            (
                "another team run",
                AttestationRefusal::TeamRunMismatch,
                |_, o| {
                    o.team_run_id =
                        TeamRunId::parse("01a09f49-0bbd-7402-a0c8-4882dbdfedc4").expect("other");
                },
            ),
            (
                "role evaluates no gate",
                AttestationRefusal::RoleNotEvaluatorOfGate,
                |_, o| {
                    o.gate_evaluator_roles =
                        vec![RoleKey::parse("fleet-verifier").expect("another role")];
                },
            ),
            (
                "slot holds another role",
                AttestationRefusal::SlotDoesNotHoldRole,
                |_, o| {
                    o.seat.catalog_role =
                        RoleKey::parse("fleet-implementer").expect("another role");
                },
            ),
            ("another seat", AttestationRefusal::SeatMismatch, |_, o| {
                o.seat.id =
                    SeatBindingId::parse("01a09f49-595e-7e50-962a-a2ee14ae76af").expect("other");
            }),
            ("seat moved", AttestationRefusal::SeatMoved, |_, o| {
                o.seat.revision = AggregateRevision::parse(8).expect("a revision");
            }),
            (
                "seat still active",
                AttestationRefusal::SeatNotRetired,
                |_, o| {
                    o.seat.retired = false;
                },
            ),
            (
                "seat serves another task",
                AttestationRefusal::SeatNotThisTasksEvaluator,
                |_, o| o.seat.task_id = None,
            ),
            ("another run", AttestationRefusal::RunMismatch, |_, o| {
                o.run.id =
                    AgentRunId::parse("01a0b619-6aea-78d1-9018-ef1f8b166edb").expect("other");
            }),
            (
                "run on another team run",
                AttestationRefusal::RunNotOnThisTeamRun,
                |_, o| {
                    o.run.team_run_id =
                        TeamRunId::parse("01a09f49-0bbd-7402-a0c8-4882dbdfedc5").expect("other");
                },
            ),
            (
                "run on another slot",
                AttestationRefusal::RunNotOnThisSlot,
                |_, o| o.run.role_slot_id = RoleSlotId::parse("verify").expect("another slot"),
            ),
            (
                "run still open",
                AttestationRefusal::RunNotClosed,
                |_, o| {
                    o.run.closed = false;
                },
            ),
            ("no binding", AttestationRefusal::BindingMissing, |_, o| {
                o.run.binding = None;
            }),
            (
                "another generation",
                AttestationRefusal::BindingIdentityMismatch,
                |_, o| o.run.binding.as_mut().expect("bound").generation = 2,
            ),
            (
                "no settled evidence",
                AttestationRefusal::NoSettledEvidence,
                |_, o| o.settled = None,
            ),
            (
                "another artifact",
                AttestationRefusal::ArtifactMismatch,
                |_, o| {
                    o.settled.as_mut().expect("settled").artifact =
                        ArtifactKey::parse("high-change").expect("another artifact");
                },
            ),
            (
                "another checksum",
                AttestationRefusal::ChecksumMismatch,
                |_, o| {
                    o.settled.as_mut().expect("settled").artifact_checksum =
                        ContentHash::of(b"rewritten artifact");
                },
            ),
            (
                "evidence changed",
                AttestationRefusal::EvidenceChanged,
                |_, o| {
                    o.settled.as_mut().expect("settled").evidence_digest =
                        ContentHash::of(b"a different verdict");
                },
            ),
        ];
        for (why, expected, perturb) in cases {
            let (mut claim, mut observed) = live_pair();
            perturb(&mut claim, &mut observed);
            assert_eq!(
                judge(&claim, &observed),
                Err(expected),
                "{why} must refuse with its own reason"
            );
        }
    }
}
