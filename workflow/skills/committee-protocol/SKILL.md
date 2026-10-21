---
name: committee-protocol
description: "Run a formal committee for a final gate, root-cause analysis, competing plans, reviewer disagreement or advisor uncertainty. Owns fleet-bound seat models, read-only seats, contrasting reviewers and a fresh judge. Not for a single review, an advisory question or routine task verification."
---

# Committee protocol

A committee is a read-only consultation with independent findings and one
declared aggregation rule. It is used when a single review or advisor is not
enough: a final close-out gate, root-cause analysis, competing plans, a
reviewer disagreement or advisor uncertainty.

Vocabulary: [Kontor glossary](../../../docs/GLOSSARY.md#roles-seats-and-personas).
Gate rules: [the workflow guide](../../../docs/WORKFLOW.md#35-review-and-gates).
Workspace placement stays with the placement owner in
[`epic-orchestration`](../epic-orchestration/references/paseo-direct-native-placement.md#committee-sessions-final-gates-root-cause-hard-problems).

A committee never grants itself authority. The owning workflow decides whether
a committee is required, who may call one and what its verdict satisfies.

## Independence rules

These rules are the single owner of committee independence. A review desk, an
epic or another skill links here instead of restating them.

1. **Read-only seats.** Every committee seat inspects and reports. It never
   modifies code, tickets, tracker state, runtime state or evidence. Its tool
   profile enforces the read-only tier.
2. **Fleet-bound models.** Each seat's model and effort come from its fleet slot
   binding in `{{ project.fleet }}`; the seat card and the report record the
   resolved provider and model. Do not pin a model in a skill, a template or a
   prompt.
3. **Contrasting reviewers.** The two reviewers use different vendors. A model
   from the same vendor as the other reviewer does not satisfy the contrast
   rule. The selected fleet policy and its `independent_of` bindings decide
   eligibility before placement.
4. **Fresh judge.** The judge is a separate seat. It reads both findings only
   after both are complete, and it never reviews the work itself. A judge that
   saw one finding before the other was sealed is not fresh.
5. **No self-review.** Nobody reviews work they produced, directed or can
   change in the same run.
6. **Calibration and vision.** A verdict seat uses only a model marked
   calibrated for the protocol, and a seat that must see an image or screenshot
   uses a model marked for vision. The fleet file owns these flags.

## Procedure

1. **Select the protocol.** Use a qualified committee template from the project's
   team pack (`{{ project.team_pack }}`) or the shipped default. The Independent
   Review template has two reviewers and a judge; that shape is the shipped
   fixture, not a universal rule. A template may declare another cardinality,
   quorum, round limit or aggregation rule.
2. **Resolve the workspace.** The placement owner creates or reuses the
   committee workspace and passes its exact identity to every seat. Never rely
   on runtime project inference. Read back the workspace identity and `cwd`
   before creating seats.
3. **Resolve each seat.** Read the fleet slot binding, the resolved provider,
   model and effort, and the seat's read-only capability. A missing route, a
   calibration failure or an unreadable identity blocks the seat, not the whole
   committee.
4. **Brief every seat with the same frozen evidence.** The evidence set is
   identical for both reviewers. The first message carries the exact seat name
   alone on the first line, the seat card, and the evidence references.
5. **Collect independent findings.** Reviewers record findings before seeing
   each other's. Do not aggregate early. A reviewer that cannot establish a
   finding records a question, not a guess.
6. **Seal and judge.** After both findings are durable, the judge checks each
   finding against the evidence. The judge keeps only findings with proof and
   records the aggregation outcome and every dissent. It never fabricates or
   predicts a finding and never turns a failing conjunction into a pass.
7. **Record the verdict.** Persist all findings, evidence references, dissent
   and the typed aggregate verdict. Missing evidence counts against the
   committee. A non-compliant verdict is not waived; route rework or remediation
   through the owning workflow.
8. **Reuse for the same topic.** Reuse the same committee workspace and seats
   for corrective re-reviews under the project's context policy. Do not create
   duplicate reviewer sessions for the same topic. A materially different
   subject gets another workspace.

An aggregate verdict before every protocol-required finding is durable is
`pending`, not a guess. A judge's note is not a substitute for the typed
aggregate.

## Independent Review as the default template

The shipped default is the Independent Review committee:

- two reviewers on contrasting vendors, each read-only;
- one fresh judge that reads both findings and checks them against the
  evidence;
- a conjunctive rule: every required reviewer approves and no unresolved
  P0/P1, missing acceptance evidence, hidden scope reduction or state drift
  remains;
- dissent is recorded, not resolved away.

The default is a starting shape. A project may select another qualified
template with a declared cardinality, quorum, round limit or aggregation rule.
A planning pair is not a committee and never satisfies a formal gate.

## Used alone (free work)

A `workflow: free` caller may run a committee without an epic or a TPM. The
caller acts as the coordinator and, at the operator's explicit choice, either
reuses the chosen workspace or creates a committee workspace inside the chosen
existing project, with the existing identity and path readback. The protocol
above is otherwise unchanged.

## What this skill is not

- Not a scheduler, a loader or a workspace creator. Placement belongs to the
  owning coordinator.
- Not an approval authority. The owning workflow reads the verdict and decides
  what it satisfies.
- Not a second reviewer roster. Seat identities and labels come from the
  project's team definition and the crosswalk in
  [`seat-cards`](../seat-cards/SKILL.md#crosswalk).
