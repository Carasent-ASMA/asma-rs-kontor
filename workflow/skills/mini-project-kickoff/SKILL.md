---
name: mini-project-kickoff
description: "Start a mini-project or epic whose outcome is a confirmed, executable backlog and handoff. Handles Paseo-direct or explicitly Kontor-governed setup; does not drive delivery to closure (epic-orchestration)."
---

# Mini-project kickoff

Kickoff turns an agreed initiative into one durable backlog that an LSA and
TPM can execute without another scoping pass.

## Orchestration surface (read first)

Resolve the surface before anything else. Paseo-direct is the default; an epic
is Kontor-governed only when it is explicitly marked. The
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule)
owns the full rule. Kickoff records the plan's `orchestration` and `workflow`
values ([plan field](../../templates/plan.md),
[workflow field](../../../docs/WORKFLOW.md#1-words-before-rules)).

<a id="4a-persist-and-verify-the-kickoff-hold"></a>
Kontor-governed epic: load [references/kontor-governed.md](references/kontor-governed.md).
It holds the `kontord` model, output contract, preconditions, steps 3–8 (including
the [kickoff hold](references/kontor-governed.md#4a-persist-and-verify-the-kickoff-hold)),
failure handling and verification.

## Paseo-direct kickoff (default)

1. Investigate and write the plan: steps 1 and 2 of the
   [procedure](#procedure-both-surfaces), with
   [create-implementation-plan](../create-implementation-plan/SKILL.md).
2. Review task sizing and grouping before any issue exists
   ([task sizing](../create-implementation-plan/SKILL.md#task-sizing-and-grouping)).
3. Get one user approval of the plan. It covers the whole tracker batch.
4. Create the tracker epic through the project CLI's epic operation, using the
   plan's `workstream:` key and applying the project's workstream label
   (`ws:<workstream-key>` by default; use the project's label grammar if it defines one). Keep the
   [epic content contract](../tracker-linked-creation/SKILL.md#epic-content-contract) intact.
5. Create one child issue per meaningful outcome through the project CLI's task
   operation, parented to the epic. Classify each with the
   [triage tiers](../ticket-triage-tiers/SKILL.md#tiers), and apply the
   [task content contract](../tracker-linked-creation/SKILL.md#task-content-contract)
   to every description.
6. Write the confirmed tracker keys into the plan's binding tree before the
   handoff, so plan and tracker reference each other both ways.
7. With `behaviour_scenarios: bdd`, bind each scenario at the same moment:
   write `<TASK-KEY>-SCN-<nnn>` into §1.1 and keep the draft number. Plan
   approval is the scenario approval; no separate step or role exists
   ([Behaviour scenarios](../create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md)).
8. Place the epic in Paseo with the
   [native placement reference](../epic-orchestration/references/paseo-direct-native-placement.md):
   the ESW project and one TSW per child. A new epic gets no `ECP` workspace;
   its epic lead is created in the `LSA` workspace of the durable `DESK • ADAM`
   project. Read back each native id, kind and `cwd`.
9. Hand off. Name the epic key, task keys, dependency frontier, blockers,
   selected surface and verified native names. With `bdd`, also name the
   scenario-owning tasks.

The placement reference owns hierarchy, naming, readback, seats,
consultations, committees, supervision,
[capacity](../epic-orchestration/references/paseo-direct-native-placement.md#capacity)
and recovery. This skill owns kickoff planning, issue creation and the handoff.

There is no `kontord` graph, hold, selection, receipt or memory step on this
surface. A `kontord` defect needs no containment or repair: log one row in the
project gap ledger, as the transition rule describes.

New work: at plan approval, promote the lead conditionally. If the confirmed
epic has no canonical epic LSA, promote the source ADHOC LSA in place and keep
its native origin; otherwise hand the approved plan and durable summary to that
exact existing epic LSA and require its exact-revision acknowledgement. Do not
create a fresh replacement. Follow the [conditional promotion rule](../epic-orchestration/references/paseo-direct-native-placement.md#desks-and-lead-workspaces-2026-10-24)
and the [brain-change handoff](../seat-handoff/SKILL.md#brain-change-handoff)
only when the change is actually a brain/provider change.

## Configuration and skill handoff (both surfaces)

Record in the kickoff handoff the selected workflow/Team Definition revisions,
active fleet provenance, canonical Adam/LSA and TPM identities, and required
skill paths, resolved for the receiving checkout.

- Project YAML is the shared authoring target; use it only through the
  qualified activation/publication path.
- Existing runs keep their pins. New Adam placement and planning-pair protocols
  require supported definitions and readback before selection.
- Do not silently invent bindings.
- Preserve the plan's milestone/BDD selections and completion scope.

## Planning questions (both surfaces)

Before issue creation and at handoff, apply the
[ambiguity and uncertainty rule](../create-implementation-plan/SKILL.md#ambiguity-and-uncertainty).
The plan records consequential questions, owners, resolution evidence and exact
affected tasks. Clarify acceptance-changing meaning before treating dependent
delivery as executable. A bounded investigation can be an executable research
task while delivery depends on its result; routine implementation discretion
needs no extra question or approval. Include unresolved questions and their
readiness boundaries in the LSA/TPM handoff by reference to the plan. Existing
authorization, kickoff holds and surface-specific admission rules still apply.

## Public identity and naming

Use the full confirmed tracker key as the public work-item identifier. The
[public identity and naming rule](../tracker-linked-creation/SKILL.md#tracker-public-identity-and-naming)
owns the full rule.

## Autonomous TPM supervision planning

Bind the plan's **Autonomous TPM operation** section with the
[plan binding contract](../epic-orchestration/references/autonomous-tpm-operation.md#plan-binding).
Kickoff installs no heartbeat and needs no new supervisory fleet; the handoff
names who installs supervision and its first activation condition.

## Preconditions (both surfaces)

- One user approval of the plan covers its in-scope tracker batch. Ask again
  only if a later preview expands or changes scope.
- Tracker scope is project `{{ project.tracker.project_key }}` only unless the
  user explicitly names another project for this request.
- Read the project's decision-record index
  (`{{ project.decision_authorities.decision_records_root }}`) and the
  applicable plan, spec and analysis documents. Use the codebase-memory graph
  before code grep, when installed.
- Kickoff writes no production code and starts no scheduler work.

## Procedure (both surfaces)

### 1. Investigate the current state

Verify what exists in code, documentation, the tracker and, when governed,
`kontord`. Treat memory as a lead, not proof. If the unknown itself is the
deliverable, use the project's technical-spike procedure and create a
research-shaped task rather than a delivery-team pipeline.

### 2. Write the plan and closure condition

Use [`create-implementation-plan`](../create-implementation-plan/SKILL.md) for
the durable plan's executable content and the project's documentation-placement
procedure for its filename, owning documentation root, category, metadata and
router entry. When the selected router declares bounded-context domains or
tiers, apply its domain rules too.

The plan's frontmatter `goal:` is one testable sentence describing what is true
when the initiative is done. Reuse it verbatim in the tracker epic Goal
projection; do not create paraphrased copies.

Prepare the epic's concise Summary / purpose, Scope and sub-goals, Done means,
and Detailed plan link alongside that Goal, following the
[epic content contract](../tracker-linked-creation/SKILL.md#epic-content-contract).
Prepare its repository URL and publish the agreed plan before kickoff handoff.
If publication requires the new tracker binding, use the contract's explicit
publication-pending sequence; do not create a publication/binding deadlock.

Before review/handoff, execute the planning skill's
[validation loop and gates](../create-implementation-plan/references/plan-graph-contract.md#mandatory-model-validation-loop-and-gates).
Run the project's plan sync and validate operations on the saved plan. After
confirmed bindings and **before declaring ready for launch**, run its
launch-ready validation. Read diagnostics, fix every error and rerun until exit
0/PASS; unresolved findings keep the plan draft/not ready. Do not invent
bindings or defer validation/repairs to the next model. Record the actual
command, exit code, PASS and plan SHA-256/path/revision in the external
handoff. Any plan edit requires a fresh run. The receiving coordinator repeats
the launch-ready validation against the authoritative revision before execution
admission. A drawn graph is not validation. Current unfinished plans use the
canonical format; completed/history records remain unchanged. Apply authorized
migrations in isolated copies, preserve owner pins and never use a legacy graph
extractor.

Define sub-goals, done evidence, work-item codes, exact owning modules, typed
`depends_on` edges, real serialization constraints, worktree requirements,
work-profile categories and gates, affected paths, mutation evidence for
runtime changes, and external fences `kontord` cannot express as sibling
dependencies.

Apply the task-sizing and milestone-adoption rules in
[create-implementation-plan](../create-implementation-plan/SKILL.md#task-sizing-and-grouping)
before materializing issues. Each task owns one accepted outcome and one primary
owner; its implementation/test/documentation steps do not automatically become
children. Review tiny and oversized tasks and record meaningful exceptions.

For a plan selecting milestone delivery, add the plan's milestone maps (a §2.6
Delivery maps section when the selected template does not carry one): map every
task to its milestone, every repository PR to its intended target, and every
writer to a collision-safe checkout arrangement. Verify the installed operations and record
fallbacks before admitting work. No milestone tracker issue or new native
workspace is created merely to mirror this plan grouping. Preserve the existing
ECP/TSW and consultation topology. Include the selected delivery strategy,
milestone frontier and capability limits in the kickoff handoff.

Worktrees isolate checkouts; they do not remove semantic or gitlink merge
conflicts. Later regrouping is a recorded plan amendment through the owning
workflow, not a fresh set of duplicate issues.

## Memory

On Paseo-direct, a durable lesson is a closeout disposition with optional,
separately authorized memory operations, not a kickoff step.

## Verification

- Plan goal, tracker epic goal, and closure evidence agree.
- The plan binds autonomous TPM cadence, callbacks, checkpoint, standing
  authority, context policy and shutdown; the handoff distinguishes planned
  installation from verified runtime setup.
- Consequential ambiguities and uncertainties have the dispositions required
  above; the handoff distinguishes ready investigation, ready delivery and
  affected work still awaiting evidence or a decision.
- Task grouping was reviewed before issue creation; milestone plans carry
  task/PR/checkout mappings, capability limits and a usable stopping point per
  milestone.
- A scenario-bearing plan has bound ids and approved hashes in §1.1 before
  handoff, and no closure condition carries a scenario id.
- The observed epic description passes the linked epic content contract;
  its plan URL and confirmed child links are present, and the handoff records
  the checked plan revision, tracker observation, and result.
- Paseo-direct placement matches the agreed native names and every
  `projectId`/`workspaceId`/kind/`cwd` readback is recorded in the handoff.
- No second backlog, tracker ticket, or `jira:` tag was introduced.

See also: [epic-orchestration](../epic-orchestration/SKILL.md), [tracker-linked-creation](../tracker-linked-creation/SKILL.md),
[ticket-triage-tiers](../ticket-triage-tiers/SKILL.md), [long-task-commit-checkpoints](../long-task-commit-checkpoints/SKILL.md).
