---
name: epic-orchestration
description: "Drive an existing scoped epic from its confirmed task graph through delivery, gates and closeout. Not for creating a new project or backlog (mini-project-kickoff) or isolated tracker issue edits (tracker-linked-creation)."
---

# Epic orchestration

The LSA owns architecture, scope coherence, cross-task decisions and
remediation design. The TPM owns routing, admission, dependencies, evidence and
closeout. They are separate persistent identities, even when they share one
runtime workspace.

Paseo is the execution runtime on both surfaces and the only control surface on
Paseo-direct. The tracker is reached through `kontord` on a Kontor-governed
epic and directly otherwise.

Every component follows the project's communication contract for human-facing
comments, reviews, status messages and handoffs, on both surfaces.

<a id="orchestration-surface-read-first"></a>
Resolve the surface first with the
[transition rule](references/orchestration-surface-transition.md#orchestration-surface-transition-rule).
This skill applies to `workflow: kontor` plans; a `workflow: free` plan follows
only the safety floor plus the pieces it asks for
([workflow field](../../../docs/WORKFLOW.md#1-words-before-rules)).
The short `orchestration` field
([plan field](../../../workflow/templates/plan.md)) never replaces the paired
governance marker.

## Paseo-direct procedure (default surface)

The baton is the tracker epic with its plan. The coordinator (Adam on new work,
the TPM seat on existing work):

1. Passes the [entry gate](#entry-gate).
2. Reconciles the tracker task graph against the plan's waves.
3. Places the epic per the [native placement reference](references/paseo-direct-native-placement.md):
   an `ESW • <epic key>` project and one `TSW • <task key>` workspace per
   ticket. An existing epic also gets its `ECP • <epic key>` workspace; a new
   epic gets none, and its Adam tab lives in the `DESK • ADAM` project.
4. Admits seats within its [capacity ceiling](references/paseo-direct-native-placement.md#capacity).
5. Integrates child branches into the plan-selected epic or milestone branch
   through verified project-CLI git operations under the
   [mandated lifecycle](../long-task-commit-checkpoints/SKILL.md#the-mandated-lifecycle).
6. Runs the Independent Review committee for the final gate.
7. Records acceptance evidence and moves the tracker through the current
   supported writer. When an activated PR desk is qualified and assigned the
   ticket-state writer role, the coordinator supplies acceptance evidence and
   verifies the desk's result instead of also writing that ticket's state. The
   coordinator retains epic acceptance and closure. A plan alone does not
   activate this handoff; until qualification and activation, the existing
   writer remains responsible.
8. Closes out in the plan and the PR ([completion and closeout](#completion-and-closeout)).

`kontord` unavailability never blocks this surface. The observing seat logs a
`kontord` defect as the transition rule says and does not contain or repair it.

<a id="authority-rules"></a><a id="dispatch-loop"></a><a id="1-plan-from-current-state"></a><a id="2-enforce-collision-and-checkout-safety"></a><a id="3-materialize-and-reuse-topology-through-kontor"></a><a id="4-start-or-resume-the-exact-plan"></a><a id="5-apply-gates-and-lifecycle"></a><a id="6-verify-resident-jira-convergence"></a><a id="capacity-and-recovery"></a>
**Kontor-governed epic: load [references/kontor-governed.md](references/kontor-governed.md).**

## Shared configuration and lifecycle (both surfaces)

Resolve the actual selected definition and supported capability before acting.
An unimplemented configuration successor does not replace the current run's
pins or require a `kontord` read on direct work.

- Adam/LSA can plan future work while the TPM admits ready delivery. Keep one
  plan revision writer, exact prerequisites and exclusive checkout ownership.
- Handoff reuses the next declared seat and names the exact artifact. QA
  rejection is rework; epic review rejection is remediation. Keep findings and counters.
- Resume the same occupant when usable; compact only under the
  [context policy](references/autonomous-tpm-operation.md#context-policy).
  Takeover needs verified retirement and one successor; resolve its authority
  afresh for its generation and never copy an old grant.
- Keep the current verdict rules, including the below-rung-2 prohibition and
  required independence. A proposed selected-policy resolver does not replace
  them without a separately qualified, explicit upgrade.
- Select Advisor, planning pair or Independent Review explicitly. Use only a
  qualified selected protocol; a planning pair never satisfies a formal gate.
- Add procedure triggers through the project's reviewed loading router when it
  is installed. Do not create a second loader or supervisor.

## Autonomous TPM operation (both surfaces)

Load the [autonomous TPM operation contract](references/autonomous-tpm-operation.md)
when planning supervision, starting the TPM, handling seat finished or
recovering idle coordination. Each epic plan binds cadence, standing authority,
checkpoint, context policy and shutdown rules. Seat finished is the primary
wake; a supported same-agent heartbeat is the safety wake. The TPM continues
eligible work without a routine "continue" prompt, keeps one admission and plan
writer, and reports installation or runtime limits honestly. A dated,
user-approved [execution-method exception](references/autonomous-tpm-operation.md#shutdown-and-method-exceptions)
may give the operator combined LSA/TPM/lead responsibility.

<a id="autonomy-and-escalation"></a>
## Autonomy and escalation

Applies to new work.

- Seats heal and finish their own work; no one babysits them.
- Only a real human decision that seats, advisors and committees cannot
  resolve goes to `{{ project.decision_authorities.platform }}`. No status
  questions, no "what next" questions, no heartbeat-like pings.
- When a real decision is missing: notify `{{ project.decision_authorities.platform }}`,
  start advisor children the decision owner can talk to, and stop only the
  blocked ticket and the tickets that depend on it. All other work keeps moving.
- Merge, build and release happen only as `{{ project.merge_authority.role }}`
  grants under `{{ project.merge_authority.policy }}`. Without a grant, prepare
  the change and stop at the gate.

## Planning questions during execution (both surfaces)

Apply the [ambiguity and uncertainty rule](../create-implementation-plan/SKILL.md#ambiguity-and-uncertainty)
before admitting affected work and when new evidence changes its readiness.

- Executors report the exact question, evidence and affected work. The LSA
  disposes scope/architecture within its authority; the TPM routes eligible
  work; product intent and reserved decisions go to the authorized owner.
- Keep question and answer in the owning plan/issue; link them from handoffs.
- Continue bounded investigation and independent ready work. Return unresolved
  scope to the LSA or kickoff; hold dependent work until its recorded condition
  is met. After a changed answer, update dependencies and revalidate evidence.
- A question alone never calls for a human, a new committee or an epic-wide stop.

## Plan-selected milestone delivery

Adoption, sizing, the trial rules and how orchestration runs a milestone live in
the [delivery milestones and adoption reference](../create-implementation-plan/references/delivery-milestones-and-adoption.md#orchestrating-a-milestone);
branch mechanics live in
[milestone publication](../long-task-commit-checkpoints/SKILL.md#milestone-branches-and-publication).

## Checkout ownership and reuse

Allocate isolation for concurrent writers per repository; a TSW stays the
persistent ticket/seat container. Steps inside one task normally keep their
checkout. Separate tasks share a physical checkout only when supported runtime
operations verify all of these:

- The old owner committed and preserved its work and source SHAs; the tree is
  clean and no pending review depends on mutating that checkout.
- Every old writer, resumed session, watcher or scheduled wake is prevented
  from writing after handoff. An idle/finished seat or a prose promise is not a
  write fence; do not retire reusable seats merely to enable pooling.
- The new exclusive owner and exact repository, path, branch, HEAD and native
  binding are read back before work. Resumes validate ownership again.
- Failed or partial reassignment leaves the previous evidence intact and admits
  no new writer until ownership is unambiguous.

**Dedicated task checkouts are the default; this rule does not enable
cross-task checkout reassignment.** Adopting it needs a separately qualified,
supported handoff mechanism meeting the conditions above. Do not simulate a
lease in Markdown, redirect persistent seats by editing state files, reassign a
TSW to another ticket, or delete worktrees as a side effect. While an old seat
can still resume, checkout reuse is ineligible.

## Collision and worktree safety

The collision unit is the task's structured module resource:

> At most one in-flight task per module unless every colliding task has a
> confirmed isolated worktree.

A worktree is checkout isolation, not a TSW. Before launch, read the stored
worktree and runtime `cwd` back. For a module-scoped task, create the child
worktree from the primary workspace and launch into that path:

```sh
{{ project.cli.command }} worktree add <tracker-key-or-slug> --mod <catalog-path>
```

It materializes the module when needed and creates a checkout under
`{{ project.worktree_layout }}`. If the project CLI has no worktree verb, use
`git worktree add` at the same configured path and record the exact command. Do
not launch a module-scoped task into a superproject worktree and sync the child
modules there if that creates a second child forest; a task limited to
superproject-tracked files may use a superproject worktree.

Worktrees never remove semantic conflicts; serialize tasks, integration or
merges when needed. Never stash, clean, reset or overwrite another seat's changes.

## Public identity and naming

The full confirmed tracker key is the public work-item identifier. Follow the
[public identity and naming rule](../tracker-linked-creation/SKILL.md#tracker-public-identity-and-naming)
for UUIDs, drafts, pre-binding labels and legacy codes.

### Paseo-direct native naming

Before creating or reusing Paseo topology on the default surface, follow the
[native placement reference](references/paseo-direct-native-placement.md). It
owns hierarchy, names, seats, readback, consultations, committees, supervision,
capacity and recovery; this skill owns the task graph and delivery process.
This heading stays as a stable inbound anchor.

## Entry gate

Before dispatching work:

1. Read the project entry file, the Paseo watchdog/recovery policy at
   `<state-root>/supervision.yml` (sample
   [`config/examples/paseo-supervision.yml`](../../../config/examples/paseo-supervision.yml)),
   and the TPM cadence profile (`{{ project.supervision }}` in the selected
   config folder); for Paseo-direct placement, the
   [native placement reference](references/paseo-direct-native-placement.md).
   The two supervision files stay distinct; `project.supervision` is not the
   Paseo watchdog policy.
2. For memory context, cite the existing frozen binding or use bounded eligible
   recall after verifying its reviewed registry and deployed capability. Keep
   its ids/hashes and actual retrieval mode; unavailable recall never permits a
   whole-corpus list fallback. This is not a mandatory `kontord` read on Paseo-direct.
3. Reconcile the plan, tracker bindings and source-control evidence. Child
   repositories are separate clones, so a root `git status` shows nothing about
   them: use the project CLI's status operation or `git status --short` inside
   each module in scope.
4. Confirm one tracker Epic binding for the epic and one tracker issue binding
   per delivery task. Missing bindings go through [tracker-linked-creation](../tracker-linked-creation/SKILL.md)
   before execution.
5. Before lifting the kickoff hold or admitting execution, run the project's
   plan validator with its launch-ready check on the authoritative saved plan.
   Follow the planning [validation loop and migration boundary](../create-implementation-plan/references/plan-graph-contract.md#mandatory-model-validation-loop-and-gates):
   read all diagnostics, fix each error within ownership, rerun until exit 0/PASS.
   Missing facts/bindings go through their owning workflow, never invented data
   or a waived validator. Failure blocks readiness and dependent admission.
   Retain command, exit code, PASS and exact SHA-256/path/revision in the external
   checkpoint; a changed plan needs a new receipt before admission using it.
   Do not infer remote binding validity or authorization from this graph check.
   New launches of legacy plans require owner-led migration; preserve already
   admitted work and active pins. Apply this on both orchestration surfaces.
6. Check the observed epic description against the
   [epic content contract](../tracker-linked-creation/SKILL.md#epic-content-contract)
   and every delivery issue against the
   [task content contract](../tracker-linked-creation/SKILL.md#task-content-contract);
   compare both with the agreed plan. Missing content or readback blocks lifting the kickoff
   hold and dispatching new work; repair through the same supported workflow
   and keep conflicting human edits.

## Completion and closeout

A task is not done because a seat says so. Require its profile gates, tests,
mutation evidence where applicable, committed source-control evidence in the
correct repo, settled runs and a converged tracker plan. "All children closed"
is not proof that the goal was achieved.

Record the memory disposition through the project's experience-memory procedure,
when one is installed. A reusable lesson gets evidence, a typed preview and a
proposal, which is not approval; record approval, projection and canary only
when performed, and keep pending or degraded results explicit. Otherwise record
"no durable lesson" with its reason. Progress, speculation, handoffs, open
questions, operational gaps, source, artifacts and verification reports stay
evidence, never recall-eligible memories. A handoff cites the existing frozen
recall binding. Instruction-file preferences use the project's preference
procedure. On Paseo-direct, record this in the owning plan/issue/PR; optional
memory operations add no `kontord` prerequisite, but explicit memory-delivery
Goals still need their proof.

For a plan with `behaviour_scenarios: bdd`, the Independent Review final gate
and epic closeout also check, from the plan's scenario table and the tasks'
evidence: hash equality between plan block and home at the baseline commit; no
scenario-line change in the home's history without a recorded amendment;
observed, not planned, results at zero retries; and mutation evidence for the
changed logic behind each `e2e` scenario or its recorded exemption
([Behaviour scenarios](../create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md)).
This reviews evidence; it adds no `kontord` gate, field or lifecycle value.

## Epic watchdogs

To author or audit a recurring epic supervisor, use the project's watchdog
runbook, when installed, and read only the relevant section during a pass. The
project's setup recipes and complete prompt are for installation and authoring;
do not reload a prompt the scheduler supplied.

- Prefer one watchdog for connected epics sharing repair or deployment owners;
  separate watchdogs only for explicit, non-overlapping scopes. Never
  auto-enroll every workspace epic.
- It adds idle-work, dependency and closeout supervision to the
  [stalled-turn watchdog](references/paseo-direct-native-placement.md#autonomous-coordination)
  and relaxes no stale-evidence or topology rule.
- Keep stable policy apart from the approved checkpoint, require blocker
  ownership and receipt readback, and surface real human decisions while
  independent work continues.
- An explicit deferral is recorded honestly; it is not successful delivery when
  the user's objective requires every agreed item committed, pushed, merged,
  live and tested.

## What this does not do

- It does not rescope. Return unclear scope to [mini-project-kickoff](../mini-project-kickoff/SKILL.md) or the
  epic LSA.
- It does not let the TPM implement delivery work.
- It does not silently waive gates, reduce scope, or replace missing evidence.

## Verification

Before reporting completion, confirm the ESW/ECP/TSW/ASW/CSW names and every
`projectId`/`workspaceId`/kind/`cwd` readback match the agreed convention,
confirm source-control receipts, and report anything still local or deferred.

See also: [mini-project-kickoff](../mini-project-kickoff/SKILL.md), [tracker-linked-creation](../tracker-linked-creation/SKILL.md),
[ticket-triage-tiers](../ticket-triage-tiers/SKILL.md), [long-task-commit-checkpoints](../long-task-commit-checkpoints/SKILL.md).
