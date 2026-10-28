---
name: seat-handoff
description: Use when work moves between seats — a normal phase handoff (hand to QA, SWE done and ready for review), QA or AUD rework, epic remediation after a completion rejection, or refusing premature closure. Not for a fresh role start, same-work resume or takeover, or peer clarification.
---

# Seat handoff

Moves **ownership of the next authorized phase** to an eligible receiver while
preserving evidence. Distinguishes **handoff** (job done for this phase),
**rework** (task-level rejection back to fix owner), and **remediation** (epic-level
completion rejection with bounded repair). Peer messages and chat delivery alone
do not advance gates.

Vocabulary: [Kontor glossary](../../../docs/GLOSSARY.md#roles-seats-and-personas).
Gate fence: [the workflow guide](../../../docs/WORKFLOW.md#35-review-and-gates).
Contract: [the workflow guide](../../../docs/WORKFLOW.md#3-kickoff-to-closeout).

## When to use

**Use for:** "hand to QA", "SWE done — ready for review", QA/AUD **rework** with
evidence, TPM routing after gate rejection, Adam/LSA **remediation** design with
TPM dispatch of bounded fixes, or refusing premature closure when evidence is
incomplete.

**Do not use for:** starting a fresh role from idle ([agent-dispatch](../agent-dispatch/SKILL.md)),
continuing the same interrupted occupant ([seat-takeover](../seat-takeover/SKILL.md)), or peer
clarification (no scope/gate change).

**Misclassified "handoff":** Same unfinished job → [seat-takeover](../seat-takeover/SKILL.md) or authorized
scope transfer, not downstream handoff.

## Orchestration surface

Resolve the surface with the
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule).

| Transition | Paseo-direct | Kontor-governed |
| --- | --- | --- |
| Normal handoff | Durable artifact reference + receiver ack; TPM records semantic phase advance | Supported handoff/gate/settlement operations |
| Rework | Same fix owner, preserved gate and counters; new artifact revision | Configured gate-rejection route; frozen TeamRun slot mapping |
| Remediation | Adam designs repair; TPM dispatches; prior completion verdict immutable | Typed remediation surfaces when qualified |
| Completion | Evaluate selected completion requirements; tracker Done ≠ epic done | Completion profile and immutable verdicts |

Message delivery is **pending communication** until acknowledgement; acknowledgement
without prerequisite evidence is **not** a passing gate.

## Classify first

| Class | Trigger | Receiver |
| --- | --- | --- |
| Normal handoff | Source phase complete; artifacts and verification present; target phase eligible; scope unchanged | Next owning seat in workflow DAG |
| Rework | QA/AUD rejection with criterion, reproduction, expected/actual, severity | Same eligible fix owner (SWE or declared owner) |
| Remediation | Epic completion review rejection | Bounded tasks Adam/LSA defines; TPM dispatches |

Route uncertain requirements to Adam/LSA or authorized product owner; do not
manufacture a code defect or a pass to unblock.

## Common evidence envelope

Every **normal handoff**, **rework** and **remediation** package includes the
same provenance and checkout readback: the
[common evidence envelope](references/evidence-envelope.md), shared with
[seat-takeover](../seat-takeover/SKILL.md).

**Guarantee classes:** **Procedural** — what this skill requires the actor to
record before advancing. **Observed** — what a supported runtime/API actually
persisted or read back; absence of observed proof blocks claims of durable
transition even when the procedure was followed in chat.

## Normal handoff procedure

1. Verify source phase complete and required artifacts present (commit SHA or
   other declared revision).
2. Verify target phase eligible under the **selected workflow revision** (not
   "latest" implicitly).
3. Identify exact receiver seat and occupant (reuse idle persistent seat when
   eligible).
4. Assemble the **common evidence envelope** plus: fulfilled and **unverified**
   criteria, findings, next action, instruction references by path (not full
   policy text).
5. Deliver handoff package; require receiver **acknowledgement of the exact
   artifact revision** before the receiver starts verification work.
6. Source stops claiming the target phase; remains reusable idle for its own role.
7. TPM aggregates progress for Adam; peer clarification copies TPM when readiness
   or scope timing is affected.

## Used alone (free work)

A `workflow: free` plan can use a plain session-to-session handoff: the sender
passes the bounded work, the artifact revision and the next action to a receiving
session, which acknowledges the exact revision. No phase is advanced or claimed,
and no gate or workflow state changes.

## Rework procedure

Follow the gate-rejection fence: resolve logical role through the **exact
route-time TeamRun** frozen slot mapping; require **newer evidence** from the
responsible role. Renaming slots, starting another TeamRun or replaying old
evidence cannot satisfy the rejected gate.

Carry the **common evidence envelope** plus rework-specific fields:

- Rejected criterion and reproduction steps
- Expected vs actual result
- Artifact revision rejected and required recheck
- Severity/impact and fix owner
- Preserved **retry counters and all failed findings** across sessions and takeovers

Flow:

```text
SWE artifact R1 → QA rejects → TPM routes REWORK → same SWE fixes → R2
→ QA rechecks R2 only → remaining gates
```

New commits invalidate affected acceptance evidence; change-impact rules decide
which tests/reviews repeat. Round limits come from the **selected workflow**, not
a universal number in this skill.

## Remediation procedure

When completion review rejects the epic candidate:

1. Prior verdict remains **immutable** (record dissent and findings).
2. Record the **common evidence envelope** for the remediation round, including
   the rejected completion artifact SHA and provenance of the selected completion
   profile revision (reference/hash only).
3. Adam/LSA designs a **bounded** repair set (scope, owners, evidence required).
4. TPM dispatches remediation tasks through [agent-dispatch](../agent-dispatch/SKILL.md).
5. Corrected evidence receives a **clean independent re-review**.
6. Exhausted rounds follow the owning workflow escalation, not session replacement.

## Conditional ADHOC promotion

Promotion into an epic is not a brain change by default.

- If authoritative discovery finds no canonical epic LSA, promote the same native
  ADHOC LSA in place. Preserve its ID, history, model and operator title; bind the
  epic context and project the logical LSA node while retaining and reporting the
  native `ADHOC` origin. Do not claim a workspace move and do not create a seat.
- If a distinct canonical epic LSA exists, use the normal handoff procedure with
  the durable summary, decisions, artifacts and revisions, fulfilled and unverified
  criteria, findings and next action. Require that exact receiver's exact-revision
  acknowledgement before releasing the source; create no second lead.
- Unknown, incomplete or ambiguous discovery is not evidence of absence. A legacy
  fixed persona that forbids promotion needs supported same-ID refresh; otherwise
  report the legacy gap and do not substitute a fresh session.
- Promotion grants no tracker, runtime or scheduler authority beyond the receiving
  role's existing boundaries, and does not inherit stale grants.

<a id="brain-change-handoff"></a>

## Brain-change handoff (new work)

Healing reuses the existing lifecycle skills: recovery uses
[seat-takeover](../seat-takeover/SKILL.md), [seat-handoff](SKILL.md) and the
configured Paseo watchdog/recovery policy (`<state-root>/supervision.yml`,
sampled by [`config/examples/paseo-supervision.yml`](../../../config/examples/paseo-supervision.yml)).
`{{ project.supervision }}` is the TPM cadence profile, not this file. No
special recovery rules are added.

A planned brain change follows this rule:

- **Same vendor:** change the model of the same session at a clear boundary.
  Keep the native identity; do not create a seat.
- **Different vendors:** do a phase handoff at plan approval, before any child
  exists. Start the successor through [agent-dispatch](../agent-dispatch/SKILL.md) with the full seat
  card.
- [seat-takeover](../seat-takeover/SKILL.md) handles same-work recovery or explicitly authorized
  replacement. The planned phase-boundary brain change above uses handoff.

New brainstorm work uses this handoff: the premium brainstorm Adam hands the
approved plan to the coordinator Adam and stops.

## Refuse premature closure

Under the selected completion contract, **all ticket statuses Done** with missing
release or acceptance evidence leaves the epic **incomplete**. Handoff and
completion steps must refuse closure; no status label substitutes for missing
evidence.

## Qualification reference

For authoring or reviewing this procedure, load the [qualification scenarios and
capability ownership](references/qualification.md).

## What this skill is not

- Not a gate implementation or tracker writer by itself.
- Not [agent-dispatch](../agent-dispatch/SKILL.md) for first seat launch.
- Not [seat-takeover](../seat-takeover/SKILL.md) for same-seat interruption recovery.
- Not proof that governed handoff APIs are deployed until evidenced per task.

## Related skills

- [agent-dispatch](../agent-dispatch/SKILL.md) — launch seats after handoff creates readiness
- [seat-takeover](../seat-takeover/SKILL.md) — interrupted same-phase work
- [epic-orchestration](../epic-orchestration/SKILL.md) — TPM admission and closeout context
