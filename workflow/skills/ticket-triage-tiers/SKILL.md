---
name: ticket-triage-tiers
description: "Classify risk and work profile when creating or triaging tasks for a Kontor-governed epic; on Paseo-direct work, use the skill's tracker tier table. Not a general task-sizing or delivery orchestration guide."
---

# Ticket triage tiers

Applies to a Kontor-governed epic (see the
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule)).
On the default Paseo-direct surface there is no Kontor task to triage: pick the
tier from the same table for the tracker issue, note it in the plan, and staff
the ticket's `TSW • <task key>` workspace accordingly.

Triage decides how a Kontor task is executed. It does not assign a tracker
human owner and does not write the tracker directly.

## Scope before tier

Apply the [task-sizing judgment](../create-implementation-plan/SKILL.md#task-sizing-and-grouping)
before selecting a team or materializing an issue. A checklist step does not
need its own team/worktree. Size and risk are independent: a tiny tenant-isolation
fix is high-stakes. An independent high-stakes fix stays a separate task from
ordinary UI/maintenance work, however small. A cohesive high-stakes feature may
include its own lower-risk steps, but inherits the highest applicable risk and
all required gates; split for distinct approval or ownership.
For milestone plans, retain the task's primary module and identify every affected
repository and collision constraint. Dedicated task checkouts remain the default.
Cross-task reuse requires separately qualified
[handoff controls](../epic-orchestration/SKILL.md#checkout-ownership-and-reuse);
this trial does not enable it.

## Tiers

| Tier | Trigger predicates | `kontord` selection outcome |
|---|---|---|
| Chore | Documentation, rename, configuration/pointer bump, lint/style, deterministic scaffold, or bounded research with no production behavior risk | Select the smallest suitable work profile and a teamless or single-seat template when the live catalog permits it. Keep ordinary gates required by that profile. |
| Standard | Ordinary feature, bug, refactor, test, design, or research work with one clear owning module and no high-stakes boundary | Select the matching versioned work profile and the project's ordinary Delivery Team template. |
| High-stakes | Authentication/RBAC/permissions, tenant isolation, database migration, billing/money, security, patient/clinical safety, destructive operations, cross-service contract, or a decision-record-governed architectural decision | Select a high-stakes profile/team with independent review and all required gates. When the task's scope or evidence leaves a concrete Standard/High-stakes classification unresolved, classify as High-stakes pending resolution. |

The names above are policy categories, not hard-coded `kontord` profile or team
ids. Resolve the current catalog every time.

Risk classification does not settle intended scope. On both surfaces, apply the
[ambiguity and uncertainty rule](../create-implementation-plan/SKILL.md#ambiguity-and-uncertainty)
to consequential questions before dependent work starts. A research task's tier
follows the risk of its actual activities; calling work a spike does not remove
its required gates, and choosing a higher tier does not resolve ambiguity.

## Required inputs

Before selecting:

- read the exact Kontor task, project, and epic;
- inspect the task's goal/scope, primary module, dependencies, and worktree
  requirement;
- read the project's decision-record index
  (`{{ project.decision_authorities.decision_records_root }}`) for governed
  areas;
- list live `work-profiles-list`, `team-templates-list`,
  `model-catalog-get`, and `account-profiles-list`;
- read current provider quota/capacity state when account routing matters.

Do not read legacy teams, saved agents, fallback overrides, or machine-local
legacy configuration. Those are not execution authority.

## Selection procedure

1. Classify the tier from the task's actual risk, not only its tracker issue
   type or title.
2. Select a work profile whose phase DAG, artifacts, handoffs, and gates match
   the work. `code`, `research`, `docs`, and UX/UI profiles are examples,
   not a closed vocabulary.
3. Select a compatible Delivery Team template when the profile requires one.
   Do not invent free-form roles or bypass the template's stable role slots.
4. Set the task's structured primary module and worktree policy. Use a worktree
   for parallel or collision-prone delivery; a tag is not a lease.
5. Select an account profile only when policy or availability requires an
   explicit pin. Resolve account before model rung: prefer an eligible account
   on the current quality rung before descending to a weaker rung. Keep the
   current verdict cutoff and account rules until a separately qualified,
   explicitly adopted successor changes them.
6. Apply through `kontord` `profile-select`, `team-select`, and
   `account-select` as appropriate, with the current revision and supported
   authority.
7. Read the task back. Confirm stored profile/team/account revisions, worktree,
   module, and effective eligibility before scheduler admission.

Never type a remembered provider/model id into durable task state without
checking the live catalog. Quota headroom and credit reserve are account facts;
do not encode a static "cheap model" table in this skill.

## Tracker boundary

- The `kontord` triage outcome may inform tracker issue intent, but the pinned
  tracker connector field spec owns issue-type and field mapping.
- Use [tracker-linked-creation](../tracker-linked-creation/SKILL.md) for create/link/reconcile.
- A tracker assignee is a verified human account. Never derive it from a
  `kontord` role, TeamRun, AgentRun, provider account, or model.
- A tracker workflow status is external observed state. It is not the
  work-profile phase or Kontor task lifecycle.
- Do not use a tracker MCP surface, tracker UI, or a direct tracker sync command
  as the normal triage path.

## Escalation and changes

A later risk discovery may move a task upward before execution or at a safe
checkpoint. Preview and apply the changed profile/team selection through
`kontord`, preserve prior receipts, and rerun admission checks. Do not silently
remove gates from an in-flight task.

If no live profile/team fits, record the mismatch and route it to the LSA or
authorized catalog owner. Do not invent a third local execution system or a
one-off legacy team.

## Verification

- Task readback names the intended immutable profile/team revisions.
- Module and worktree state match the planned collision boundary.
- Account/model route is valid in the live catalog and capacity policy.
- Required gates remain present.
- Tracker intent remains separate from execution assignment.
- No legacy configuration or direct tracker write was used.

See also: [mini-project-kickoff](../mini-project-kickoff/SKILL.md), [epic-orchestration](../epic-orchestration/SKILL.md),
[tracker-linked-creation](../tracker-linked-creation/SKILL.md).
