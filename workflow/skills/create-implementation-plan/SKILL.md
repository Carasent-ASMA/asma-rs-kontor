---
name: create-implementation-plan
description: "Write a durable, executable plan for scoped feature, refactor, design or infrastructure work. Use the project's documentation-placement procedure for placement; use mini-project-kickoff when the requested outcome includes an executable tracker/Paseo backlog."
---

# Create Implementation Plan

Write one plan file that an agent or a human can execute. State outcomes,
constraints, acceptance evidence and decision owners. Link the governing
contracts instead of copying them, and mark unverified assumptions. Leave
routine implementation choices to the executor.

## Rules owned elsewhere

- **Public identity and naming:** use the full confirmed tracker key as the
  public work-item identifier; plan codes and labels are local. The
  [public identity and naming rule](../tracker-linked-creation/SKILL.md#tracker-public-identity-and-naming)
  owns the full rule.
- **Orchestration and workflow fields:** every new or explicitly amended plan
  records `orchestration` and `workflow` per the
  [plan field and workflow field](../../../docs/WORKFLOW.md#the-kontor-workflow)
  and the
  [transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule);
  Paseo-direct is the default.
- **Autonomous TPM operation:** every epic plan carries this section after the
  scheduling facts, filled per the
  [plan binding contract](../epic-orchestration/references/autonomous-tpm-operation.md#plan-binding).
- **Branch and commit grammar:** delivery branches and checkpoint commits follow
  the [mandated lifecycle](../long-task-commit-checkpoints/SKILL.md#the-mandated-lifecycle);
  do not restate the grammar in a plan.

<a id="ambiguity-and-uncertainty"></a>
## Ambiguity and uncertainty

Use the canonical [planning terms](../../../docs/GLOSSARY.md#work-and-planning).
This rule makes the existing outcome, assumption and decision-ownership guidance
actionable during preparation, execution and re-planning, on both orchestration
surfaces. It is agent guidance; it does not install runtime admission enforcement.

- **Clarify meaning.** For ambiguity, record the materially different
  interpretations; for vagueness or missing detail, name the missing acceptance
  boundary. Check governing contracts, the glossary, examples and prior decisions
  first. If these settle the question, cite them and proceed. Otherwise route it
  to the authorized decision owner before dependent implementation or acceptance
  tests encode one interpretation. An assumption cannot silently choose scope.
- **Investigate uncertainty.** Record the defined question, known evidence,
  investigation owner, bounded method/timebox, evidence needed and escalation
  condition. Use a task step for a small check or the project's technical-spike
  procedure when research is itself the deliverable. A clear, bounded
  investigation may be ready while dependent delivery waits for its result;
  elapsed time alone does not resolve the question.
- **Preserve discretion.** Routine reversible choices within agreed outcomes,
  constraints and authority belong to the executor. Multiple valid approaches
  alone do not justify an open question, a spike or a human interruption.
- **Bound the affected work.** Resolve meaning that could change scope,
  acceptance, ownership or risk before dependent implementation. Remaining
  uncertainty may be investigated in-task or carried as an explicit bounded
  assumption when the authorized scope permits it; record validation, consequence
  if false and the point beyond which work must wait. Name affected tasks and
  dependencies; continue independent work under its existing authorization.
- **Escalate the actual decision.** Ask a human directly when their intent,
  preference or reserved authority is missing. Otherwise use the existing
  authorized investigation/consultation route and its limits. If it cannot
  establish a safe authorized path, provide evidence, attempted paths, options,
  a recommendation, the exact decision/owner and consequence of no action.
  Consultation cannot supply missing authority. Neither label alone is a reason
  to ask for permission, broaden scope or waive an existing gate.

Keep consequential open questions in the plan's §7, or the owning issue/handoff
for work without a plan; link from other artifacts instead of duplicating a
ledger. Record resolution evidence or the authorized decision, update affected
requirements/dependencies, and revalidate affected acceptance evidence when the
answer changes. Completion still requires the agreed evidence: accepting a
bounded assumption is not proof of an unmet criterion.

## Plan Structure Requirements

Organize phases around outcomes and declare dependencies. A task is a meaningful
unit of ownership and acceptance, potentially containing several implementation
steps and commits. A phase, task, commit, PR and worktree need not have the same
boundary.

<a id="task-sizing-and-grouping"></a>
### Task sizing and grouping

This general guidance applies independently of the optional milestone trial.
Before creating issues, group the proposed work by outcome and review context.
Keep implementation, tests, documentation and required wiring for one outcome
inside its task unless independent ownership, risk or acceptance warrants a split.

| Decision | Judgment to apply |
| --- | --- |
| Combine | The changes serve one coherent outcome, share substantial context and validation, and have compatible ownership and risk. |
| Split | Outcomes can be accepted separately, a different owner or risk boundary needs independent review, or the combined diff/acceptance scope is too large to review confidently. |
| Keep a tiny task | It is an urgent independent fix, external unblocker, or independently owned/high-risk change. State the reason briefly. |
| Keep a checklist step | It has no useful acceptance boundary outside its parent's outcome. Do not create a tracker issue, team or worktree just for the step. |

Avoid both miscellaneous catch-all tickets and tickets per file, function,
technical layer or validation step.
Sharing a repository alone is not a reason to combine unrelated fixes. A bounded
consistency correction can be one task only when a shared invariant and
acceptance check make its purpose coherent. Otherwise preserve separate issues
without manufacturing an epic. An unrelated discovery goes to its appropriate
backlog context, not into the current milestone merely to avoid a small issue.
An independent high-stakes fix remains a separate task with its own gates; never
absorb it into ordinary UI/maintenance acceptance. A cohesive high-stakes feature
can still include its own lower-risk implementation, test and documentation steps.
One primary owner remains clear. A task may span repositories when that owner can
accept one bounded outcome; split for real ownership, risk or acceptance seams,
not repository count alone. List every repository and collision constraint.

Use effort estimates as signals, not minimum/maximum time or line-count quotas.
Check both extremes: does this tiny ticket deserve independent acceptance, and
can a reviewer still confidently verify this large task's one stated goal?
Record a short rationale for consequential combine/split decisions or exceptions;
ordinary tasks need no scoring rubric. Revisit sizing when actual scope changes.

### Delivery milestones and adoption

Load the [delivery milestones and adoption reference](references/delivery-milestones-and-adoption.md)
when selecting or assessing this opt-in strategy. The heading remains here as
the stable inbound anchor.

### Behaviour scenarios (opt-in trial)

Load the [behaviour scenarios opt-in trial reference](references/behaviour-scenarios-opt-in-trial.md)
when selecting or reviewing this trial. The heading remains here as the stable
inbound anchor.

### The goal is a closure condition, not a heading

The frontmatter `goal:` is mandatory and is the plan's single closure condition:
one unambiguous sentence naming what is true when the plan is done. Phase goals
do not substitute for it. A noun phrase that repeats the title is not a testable
goal; state an outcome that can pass or fail.

Where one plan equals one epic, [mini-project-kickoff](../mini-project-kickoff/SKILL.md) sends that sentence
unchanged into the tracker epic: through the project CLI on the default
Paseo-direct surface, or through `kontord`'s tracker materialization for a
Kontor-governed epic, recording the confirmed tracker key (and the `kontord`
epic id where governed). The plan remains the detailed source. Do not create a
second backlog copy or a separately paraphrased goal.

A plan too large for one epic states which epic owns which phases, and each epic
gets its own derived closure condition.

For an epic, prepare the concise tracker-facing description from this plan using
the [epic content contract](../tracker-linked-creation/SKILL.md#epic-content-contract):
Summary / purpose, Goal, Scope and sub-goals, Done means, and Detailed plan.
For each child issue, prepare the
[task content contract](../tracker-linked-creation/SKILL.md#task-content-contract)
from the task row.
Keep Goal verbatim and provide the published repository document URL for tracker
readers; the detailed phases and waves remain in the plan.

## Phase Architecture

- Each phase must have measurable completion criteria
- Identify genuinely independent tasks and declare dependencies and serialization constraints
- Include known affected paths, contracts and acceptance checks; mark unverified implementation assumptions rather than inventing exact functions or line numbers
- Assign consequential scope/risk decisions to an owner and allow routine implementation judgment within those boundaries
- Every plan must carry a **Dependency Graph and Waves** section (template §2.5). Phases say *what each group delivers*; **Waves** say *what launches at the same time*. The two are distinct and both are required — see the glossary's [work and planning terms](../../../docs/GLOSSARY.md#work-and-planning). A plan with phases but no waves cannot be scheduled, because nothing states which phases may run concurrently or what the collision rule is.
- The waves section uses the [canonical plan-graph v1 contract](references/plan-graph-contract.md): exactly one strict JSON `plan-graph` fence, its generated wave/binding view, and scheduling facts. Author only sibling `depends_on` edges in the JSON; waves and identifier diagrams are derived. The generated binding view joins local task codes, confirmed tracker keys and governed Kontor IDs. Migrate all current, unfinished implementation plans; preserve completed/history records. Consumers read only the canonical graph, with no legacy ASCII/Mermaid/prose extractor or guessed fallback.
- Plan **useful concurrency** after grouping tasks by outcome. Account for integration/review capacity, collisions and runtime limits. Do not split tasks to manufacture roots or add dependency edges for presentation. Name each cross-stream join and its actual dependency or safety reason.
- An **externally fenced** item — one blocked by another plan's work item — appears `ready` in the graph, because `depends_on` names only siblings. The plan must say so explicitly, or someone will arm it off the back of that state.
- Where two phases touch a module another in-flight plan also edits, the waves section must state the collision control (worktree, new-file bias, single call site) rather than asserting that the phases are parallel.
- A plan that changes runtime behavior ends with a mutation-validation phase; see [Mutation validation](#mutation-validation-a-plan-is-not-tested-until-the-tests-are-tested).

## Document placement and naming

This skill owns the plan's executable **content**, not its filesystem policy.
Before choosing a path or filename:

1. Apply the project's documentation-navigation and documentation-organization
   procedures to the candidate documentation root.
2. Use the project's documentation roots
   (`{{ project.documentation_roots }}`) and decision-record root
   (`{{ project.decision_authorities.decision_records_root }}`) as the
   authoritative routing source.
3. When the selected router declares bounded-context domains or tiers, apply
   its nested-path rules.

Do not infer a global location merely because a plan crosses repositories, and
do not derive a path from examples elsewhere in this skill. Validate the final
path, filename, metadata, and nearest-router update against the authoritative
documentation procedures before considering the plan complete.

## Plan template

Copy the pack's [plan template](../../templates/plan.md) and add the optional
sections below when the plan selects them. Every base numbered section is
required and holds specific, actionable content; an optional part extends the
copy and never replaces a base section:

| Optional part | Include it when |
| --- | --- |
| Frontmatter `version`, `last_updated`, `owner`, `tags` | it is used |
| Frontmatter `delivery_strategy`, `behaviour_scenarios` | the plan selects that trial |
| §1.1 Behaviour Scenarios and the §6 scenario table | `behaviour_scenarios: bdd` |
| A §2.6 Delivery maps section | `delivery_strategy: milestone`; otherwise record a compact milestone note where the plan lists its tasks |
| Autonomous TPM operation | the plan is an epic plan |
| The §6 `MUT-` table | the plan changes runtime behavior |
| The §7 open-questions table | consequential questions exist; otherwise state that none are identified |

### Template validation rules

The author/model must execute the
[mandatory validation loop and gates](references/plan-graph-contract.md#mandatory-model-validation-loop-and-gates):

```sh
{{ project.cli.command }} plan sync <plan.md>
{{ project.cli.command }} plan validate <plan.md>
# After confirmed tracker/Kontor bindings, before declaring ready for launch:
{{ project.cli.command }} plan validate --for-launch <plan.md>
```

If the project CLI does not expose plan verbs, use the project's supported plan
validator and dependency-graph validator; record their exact command names. Run,
read every diagnostic, fix all reported errors, then rerun until exit 0/PASS.
Never leave validation or repairs as a suggestion for the next person. Resolve
missing facts through the owning workflow; do not invent bindings, suppress
failures or declare a failing plan ready. The normal check gates review/handoff;
`--for-launch` gates the launch-ready claim and is repeated by the receiving
coordinator before execution admission. Any later edit invalidates the receipt.
Record command, exit code, actual PASS and exact plan SHA-256/path/revision in
an external handoff/checkpoint, not inside the file whose hash it records.
Also record the working directory and selected validator path/SHA-256. The CLI
uses the caller's checkout; an installed CLI can still run an older worktree's
validator. For cross-checkout migration, use the agreed validator revision and
absolute candidate paths as described in the contract.

`sync` only regenerates views, never invents dependencies. The CLI, hook and CI
use one validator. Apply the contract's active-plan migration and ownership rules.
This graph check does not validate remote tracker existence, scheduler readiness
or the rest of the template; repair findings from those checks as well.
Continue the following content checks:

- Required frontmatter fields and the documentation metadata block are present
  and well formed; optional fields appear only when used.
- `workstream` names exactly one key from the project's workstream
  configuration. A workstream groups the epics that develop the same work, and
  reporting groups spend per workstream. If no key fits, ask the user. A new key
  is a decision added to that configuration; do not invent one in the plan.
- `orchestration` is exactly `paseo` or `kontor` for new or explicitly amended
  plans; the displayed value matches it and has the shared italic explanation.
  Daemon-governed plans keep the paired governance marker; historical values
  follow the compatibility rule and are not bulk-rewritten.
- All numbered section headers match exactly (case-sensitive). The H1 is a
  descriptive plan title, never the literal `Introduction`.
- All identifier prefixes follow the template format.
- Tables include all required columns.
- No placeholder text remains in the final output.
- With `behaviour_scenarios: bdd`: §1.1 and the §6 scenario table are present,
  every scenario has exactly one owning task, one home and one layer, and no
  closure condition contains a scenario id.

### Status

The frontmatter `status` shows the plan's current state, and the introduction
shows it as a badge. Values (badge colour): `Completed` (bright green),
`In progress` (yellow), `Planned` (blue), `Deprecated` (red), `On Hold`
(orange).

A plan whose status is `On Hold` carries a frontmatter `hold:` block with four
non-empty fields: `reason`, `owner`, `lifts_when` and `review_by`, in the shape
of the [plan template](../../templates/plan.md#8-holds). `reason` is one of the
template's closed list. `review_by` is an ISO date no earlier than today and at
most 30 days ahead; a hold past its review date is expired, not quietly still
valid, and a date more than 30 days ahead is refused. The pack's `check`
command enforces the block; run it before treating an `On Hold` plan as
current. The block records the same four fields the Paseo-direct TPM pause
rule requires, so a pause and a plan hold cannot disagree.

<a id="mutation-validation-a-plan-is-not-tested-until-the-tests-are-tested"></a>
## Mutation validation (a plan is not "tested" until the tests are tested)

Green tests prove nothing until something has tried to break them. Every phase
that changed runtime behavior ends with a mutation pass over that phase's
changed files, and the plan's **final phase** is a mutation-validation phase
over the whole change set, per the
[mutation-testing procedure](../mutation-testing/SKILL.md): seed a deliberate
defect, rerun the suite, and treat a still-green suite as a broken
test. Record each mutant as a `MUT-` entry in §6 Testing (mutant, `file:line`,
suite, KILLED/SURVIVED/EQUIVALENT). **A plan must not reach `status: Completed`
while any mutant is still SURVIVED and unaddressed** — an unaddressed survivor
means the phase's acceptance evidence is a test that does not check what it
claims. Each phase's pass belongs to its owning task and needs no separate
tracker issue.

## Close-out (a plan is not "done" until it is compacted)

A plan is a **point-in-time** document. Once it reaches `status: Completed` **with
no residual/open tasks** — or is `Deprecated`/abandoned — it has served its
purpose and must not be left verbose in the tree inflating future context.
**Compact it into a succinct historical summary and delete the verbose original**,
per the project's document-lifecycle procedure: keep only what shipped, the
locked decisions/deviations, where the code now lives, and the lessons; drop the
task/phase tables and resolved risks; consolidate sibling plans of one initiative
into a single `history/` doc; redirect inbound links. A plan that is only "core
complete — residual open" is **not** yet eligible; it stays a plan.
