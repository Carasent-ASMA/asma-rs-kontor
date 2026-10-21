---
name: tracker-linked-creation
description: "Create, link or reconcile tracker epics and issues under the applicable Paseo-direct or Kontor-governed authority and content contracts. Not a project kickoff or delivery orchestration procedure."
---

# Tracker-linked creation

For comments and status messages, follow the project's communication contract.
It governs wording, not write authority; the content contracts below still
govern issue descriptions.

For a Kontor-governed epic, create the `kontord` graph first, then ask `kontord`
to materialize and confirm its tracker bindings. Never create a parallel
backlog, a tag hierarchy, or a direct tracker-sync workflow.

## Orchestration surface (read first)

Resolve the surface per the
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule).
On the default **Paseo-direct** surface create and link tracker issues directly:
through the project CLI's tracker operation for the epic and each task, then
use its describe/update operation to set or repair a description from a
reviewed file. Apply the [epic content contract](#epic-content-contract) to
every epic description and the [task content contract](#task-content-contract)
to every non-epic issue created or substantively revised. Move status through
the supported tracker writer or the publication command's transition flag. No
`kontord` preview/apply, receipt, binding or `operational_gap` record exists for
such an epic. Everything else in this skill applies to a **Kontor-governed**
epic.

<a id="tracker-public-identity-and-naming"></a>
## Public identity and naming

Use the full confirmed tracker key as the public work-item identifier; retain
the Kontor UUID in structured bindings and evidence. The key must match
`{{ project.tracker.key_pattern }}`. Pre-binding plan references are local
planning labels, not a second durable backlog namespace. Drafts may await
binding, but delivery and native materialization require confirmation. Legacy
codes are read-only lookups through recorded mappings, never write identifiers
or inferred tracker keys. New-policy work requires no code allocation.

New native definitions use `EPIC_JIRA_KEY`, `TASK_JIRA_KEY` and
`SCOPE_JIRA_KEY`, with scope taken from the subject. These Jira-named tokens
are the supported public contract spellings: they name the tracker-key position
even when the configured tracker is not Jira. Do not add or request kernel
support for a second spelling in this pack. Seat labels remain local role/slot
labels. Preserve old token meanings, immutable pins, archived names
and evidence; deploy support and use Kontor upgrade preview/apply before
changing existing pins. Read back exact native identities and desired names.
A skill change does not install runtime enforcement. Record an unsupported
surface or blocked migration through the existing operational-gap procedure.

<a id="tracker-convergence"></a>
## Tracker convergence

Resident task-and-epic tracker convergence supplies the normal path. Normal
orchestration reads its receipts, conflicts and current binding state; it does
not run manual preview/apply on every turn. Enablement requires the project
connector and matching immutable task/epic workflow specifications to be
installed for the selected work and completion profiles. A missing matching
workflow blocks convergence; install the appropriate supported revision, without
changing a frozen task profile to evade the mismatch. Manual
`ticket-reconcile-plan` and apply remain useful for diagnosis, explicit recovery
and initial materialization.

The project's pinned connector specification (`{{ project.tracker.spec }}` in
the selected config folder) owns field/workflow mapping and its release
evidence.

## Authority boundary

- **Kontor owns** project/mini-project/task identity, goal references,
  dependencies, module resources, worktree policy, work-profile/team/account
  pins, lifecycle, gates, evidence, and desired tracker projection.
- **The tracker owns** minted issue keys and observed external fields/workflow
  state.
- **Kontor receipts own the join** between those identities. A tracker key in
  prose or a tag is not a binding.
- **Legacy backlogs are read/import only.** Do not write a legacy backlog or a
  legacy memory mirror.
- A pending legacy ticket and `mod:`/`jira:` tags belong only to work explicitly
  still governed by that legacy system; they are not Kontor authority,
  hierarchy, or a second progress ledger. Check Kontor subject authority before
  reading legacy backlog or memory; after cutover use the approved Kontor
  revision rather than the legacy mirror.
- Tracker scope is `{{ project.tracker.project_key }}` only unless the user
  explicitly names another project for the current request.

## Preconditions

1. The user has approved the goal or batch.
2. Resolve the exact Kontor realm, project id, mini-project/epic id, task ids,
   current revision, and subject-authority state through Kontor MCP or the
   supported `kontor` CLI over `/v1`.
3. For a new initiative, apply the Kontor project/epic/task graph before tracker
   materialization. Use [mini-project-kickoff](../mini-project-kickoff/SKILL.md) and [ticket-triage-tiers](../ticket-triage-tiers/SKILL.md);
   do not make the tracker the place where the task graph is invented.
4. Inspect the live Kontor tool schemas and the project's pinned tracker
   connector field/workflow specifications. Do not copy remembered custom-field
   ids, issue-type ids, transition ids, status names, or credentials into a
   skill or caller payload.

<a id="epic-content-contract"></a>
## Epic content contract

Every epic created, linked for delivery, or revised by an agent must have
a concise description that makes sense without opening the plan. Use these
sections in this order; let the pinned connector encode them for the tracker:

| Section | Required content |
| --- | --- |
| **Summary / purpose** | A short paragraph explaining the current problem, who it affects, and why the intended improvement matters. A repeated title is insufficient. |
| **Goal** | One testable sentence stating what is true when the epic is complete, copied verbatim from the authoritative plan's `goal:`. For a plan spanning several epics, use that epic's explicitly assigned closure condition. |
| **Scope and sub-goals** | Main outcomes and scope boundaries, with child work-item references grouped by outcome. Use confirmed child links once materialized; before keys exist, use the exact planned task references and never invent keys. |
| **Done means** | Independently verifiable completion conditions and the evidence needed to prove the Goal. "All children closed" alone is insufficient. |
| **Detailed plan** | A clickable repository document URL, with a descriptive label and repository-relative path, accessible to tracker readers by kickoff handoff. A local absolute path alone is insufficient. |

Prepare this description from the agreed plan before materialization. Keep
implementation detail, dependency waves, and phase-by-phase instructions in
the linked plan. The epic describes purpose, outcome, scope, and completion.
Empty headings, `TODO`/`TBD` placeholders, a title-only body, and a link-only
body do not meet this contract. Do not invent missing scope or closure criteria.

If repository publication requires the new confirmed tracker binding, include
the intended plan URL/path with an explicit **publication pending** note in the
initial projection. Then publish through the authorized repository workflow,
verify the link, and reconcile away that note before kickoff handoff or
admission. Do not require publication before the binding needed to publish,
and never describe an unpublished link as accessible. This bounded sequencing
case permits no missing Goal, purpose, scope, or completion conditions.

Check at three points:

1. **Before apply:** inspect the actual tracker-facing projection. Require all
   five populated sections, Goal equality with the plan, consistent scope and
   completion conditions, and the plan link (or the explicit publication-pending
   case above). Check child references against the exact Kontor graph. Correct
   missing content before writing; preview success alone does not establish this
   content check.
2. **After materialization or update:** read the observed tracker description
   through Kontor and verify that encoding/rendering preserved the sections,
   Goal, links, and child references. Complete child links through supported
   reconciliation after keys are confirmed. A binding receipt or HTTP success
   without description readback is insufficient.
3. **Before kickoff handoff, admission, and closeout:** require the same
   content check against the current agreed plan and tracker observation, with
   the published plan link verified and no publication-pending note. Keep
   the kickoff hold until it passes. If later human edits conflict, preserve
   them and use the supported conflict-resolution flow; never overwrite them
   to satisfy a checklist or silently change the Goal.

Record the plan path/revision, exact epic tracker key, Kontor revision,
projection or reconciliation receipt, observed-description reference, and check
result in the owning workflow's evidence/handoff. Recheck after plan or
epic-content changes. Missing readback is unverified, never a pass. If Kontor
cannot expose or preserve the required content, use the operational-gap
procedure below; do not silently omit sections or invent unsupported payload
fields.

This is a required agent/workflow check. A skill edit does not install a
server-side validator. Claim automatic enforcement only with evidence that
the deployed connector rejects incomplete content before writes and detects
subsequent drift. Checking section presence cannot prove the prose is useful;
the creating/reviewing role must assess its meaning against the plan.

<a id="task-content-contract"></a>
## Task content contract

Every **non-epic** issue created or substantively revised by an agent — Task,
Tech task, User Story, Bug — carries this structure in its description, in this
order. Apply it at creation and at a substantive description or scope revision,
not on a comment, label or status transition. Epics use the epic contract
above; a plan document keeps the implementation detail.

| Section | Required content |
| --- | --- |
| **Summary / purpose** | A short paragraph: what problem, who it affects, why this issue. A repeated title is insufficient. |
| **Goal** | One testable closure sentence. Copy the task's own closure condition from the plan's task row verbatim; copy the plan's frontmatter `goal:` only when this task owns that entire outcome; for a standalone task, its description owns the Goal. Never "not applicable". |
| **Scope and sub-goals** | Outcomes and boundaries of this issue alone. |
| **Delivery context** | Files and modules touched, direct `depends_on` blockers (or "none — single work item"), external fences, and risks/assumptions. The full dependency graph and waves stay in the plan's §2.5 and are linked, never duplicated. Dependencies constrain readiness; scheduling and dispatch follow the owning workflow — the description grants no dispatch authority, and nothing is auto-dispatched on the Paseo-direct surface. |
| **Verification and evidence** | Planned checks separated from observed results; reference the mutation policy for runtime behavior; docs-only work still needs link and mirror checks. |
| **Detailed plan** | The exact task-entry link and plan revision when a plan exists; "No plan; this description is the record" when it does not. |
| **Done means** | Independently verifiable acceptance conditions. Never "not applicable". |

For milestone delivery, **Delivery context** names the plan-local milestone and
links its task/PR mapping; **Done means** states the acceptance boundary (for
example integrated, or merged and released) without inventing new tracker
statuses. A milestone grouping does not create another tracker issue type or
parent level. Perform the
[task-sizing review](../create-implementation-plan/SKILL.md#task-sizing-and-grouping)
before materialization. Implementation checklist steps remain within their
owning task; the task's goal and evidence remain independently attributable.

For a task owning behaviour scenarios (plan
[`behaviour_scenarios: bdd`](../create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md)),
**Verification and evidence** lists as planned the bound scenario ids, layer,
home path and exact command, and records as observed, per run: repository
HEAD SHA, exact command, `retries: 0`, the hash check against the approved
baseline, the per-scenario result lines copied from the runner output, and for
new behaviour the first behavioural RED. Results are text in the issue and in
the PR description, not links to CI artifacts, because pilot runs are manual
and CI retains no passing report. **Done means** may name scenarios observed
passing at zero retries and never consists of "all scenarios pass" alone.
**Goal** carries no scenario id and is unchanged by the trial. Scenario text
never enters the tracker: the plan's §1.1 and the home are its only copies. At
plan compaction, repoint the task's Detailed plan link through this whole-body
replacement flow.

No silent omissions: a section is filled, or explicitly says
`Not applicable — <reason>`, `Unknown — <owner and how it will be resolved>`,
or `Not yet performed — <planned check>`. Unknown dependencies are not "none";
unperformed evidence is not "passed"; Goal and Done means are never not
applicable. Populated headings prove structure only: the creating or reviewing
role still checks the content against the plan and the current observed state.

Replacement is whole-body, never merged. Review the current description first
(the project CLI's dry-run or preview operation prints the current and new body
with their hashes), keep human content you intend to preserve, and apply with
the reviewed `--expect-hash`; a changed baseline refuses. After a write, require
the read-back confirmation; a timeout or mismatch is `unverified` and must be
reconciled before retrying.

## Create or link an epic and its tasks

An ordinary explicit **Link** preserves the tracker issue's observed summary and
body and hashes that observed content into the receipt. It still must validate
the exact tracker project, issue type, parent and immutable external issue
identity. It must never replace approved human content with generated
placeholders merely to make a projection compare equal. Strict generated
summary/description equality applies to **Create**, recovery, and any path
whose marker policy explicitly requires it. This content distinction does not
weaken identity or hierarchy checks and does not permit title similarity,
numeric suffixes, or other display data to establish a binding.

1. **Read before preview.** Read the Kontor project and epic, including every
   task intended for materialization. Confirm each task has one owning module,
   typed dependencies, worktree policy, and enough approved tracker-facing scope
   to produce its issue type's required fields.
   Prepare and validate the epic description against the
   [epic content contract](#epic-content-contract).

2. **Preview the whole tracker graph.** Call the live Kontor
   `jira-materialization-preview` surface with the project id, epic id, one
   epic create-or-link intent, and one create-or-link intent per task id. Let
   the pinned connector specifications derive tracker field ids, issue types,
   canonical body encoding, parent links, workflow policy, and allowed field
   ownership.

3. **Review the preview as a transaction.** Require all of the following:

   - project is exactly `{{ project.tracker.project_key }}`;
   - one tracker epic binding is planned for the Kontor mini-project;
   - the actual epic projection passes the epic content contract, including
     its verbatim Goal and plan link with truthful publication state;
   - every delivery task gets exactly one tracker issue binding;
   - every child is parented to the previewed or already-confirmed epic;
   - existing matching keys are linked, not recreated, and an ordinary Link
     preserves and hashes their observed summary and body;
   - conflicting key, parent, issue type, or governed field blocks the batch;
   - optional absent fields remain absent and private execution material is not
     projected;
   - no outbound tracker comment is representable;
   - the preview reports its hash and the exact Kontor revision it was built
     from.

   A duplicate candidate or ambiguous existing issue is a conflict. Do not
   choose by title similarity or proceed with the rest of the batch.

4. **Apply the exact preview.** Use `jira-materialization-apply` with the
   returned preview hash, the exact previewed materialization payload, the
   expected revision, and a stable idempotency key. Use the caller tier required
   by the live tool contract. Reusing the same key must return the original
   receipt rather than create another tracker issue.

5. **Read back both sides.** Require a confirmed receipt for the tracker epic
   and every child, including external key, type, parent, governed fields,
   status, assignee observation, and resulting Kontor binding. Refetch the
   Kontor epic and tasks. A success response without confirmed external readback
   is not completion.
   Read the observed epic description and record the content-contract result;
   complete confirmed child links before kickoff handoff.
   For an ordinary Link, compare this readback with the observed content hashed
   by the receipt; do not substitute a Create/recovery placeholder projection.

6. **Keep lifecycle reconciliation ticket-scoped.** For later status or
   assignee convergence, call `ticket-reconcile-plan` for the exact Kontor
   task. Review the projection and apply it with `ticket-reconcile-apply` only
   when the current turn authorizes the write, using the returned projection
   hash and a stable idempotency key. Refetch and require convergence.

   - Never hard-code a transition id or ask a model to choose a route.
   - Preserve an existing human assignee. A coding seat, model, provider, team,
     or account profile is not a tracker assignee.
   - Use the Kontor ticket-claim flow for an authorized self-claim; identity is
     resolved by the connector, never from email or display name.
   - Treat external status conflicts through `ticket-conflicts-list` and the
     typed resolution surface. Do not overwrite an incompatible human move.
   - Tracker comments are inbound-only through `ticket-comments-pull`; Kontor
     has no outbound-comment command.

## Tracker MCP fallback

A tracker MCP surface is a fallback for one exact tracker operation only when
the corresponding Kontor capability is absent, unavailable, or demonstrably
broken. It is not a second normal path and never becomes the continuing
authority.

Before using it:

1. Capture the failed intended Kontor tool, error class, realm/project/epic/
   task identities, last confirmed revision/receipt, and resume checkpoint.
2. Record typed `operational_gap` evidence on the owning Kontor task or epic.
   If Kontor cannot accept evidence, create the dated repository gap report
   required by the project's workflow and attach it after recovery.
3. Constrain every tracker MCP search/read/write to
   `{{ project.tracker.project_key }}`. Discover live issue-type metadata and
   transitions; do not reuse historical hard-coded field/transition tables.
4. Perform only the blocked operation, using an idempotent search-before-create
   check and exact readback. Apply the same epic content contract before the
   write and to the observed description afterward. Do not continue the wider
   workflow through the tracker MCP surface, tracker UI, Paseo, a legacy
   backlog, or a direct tracker sync.
5. Import/link the confirmed tracker identity into the same Kontor graph at the
   recorded checkpoint, repair the Kontor gap, and resume there. Never restart
   the initiative or create a replacement task to hide the fallback.

If evidence cannot be recorded and a direct write would create dual authority,
stop and report the blocker instead of mutating the tracker.

## Verification

- The previewed and observed epic descriptions pass the epic content contract;
  the exact plan Goal, plan link, child references, and verification evidence
  survive readback. Missing content or observation blocks kickoff completion.
- The Kontor epic and every delivery task have exactly one confirmed tracker
  binding and no unresolved ticket conflict.
- Replaying materialization with the same idempotency key creates nothing.
- A fresh `ticket-reconcile-plan` is converged or explains a typed conflict.
- Tracker comments can only move inbound.
- No legacy backlog write, `jira:` tag convention, direct tracker credential
  access, or direct tracker-sync command appears in the workflow.
- Any tracker MCP fallback has durable `operational_gap` evidence and a
  recorded Kontor resume checkpoint.

See also: [mini-project-kickoff](../mini-project-kickoff/SKILL.md), [epic-orchestration](../epic-orchestration/SKILL.md),
[ticket-triage-tiers](../ticket-triage-tiers/SKILL.md), [long-task-commit-checkpoints](../long-task-commit-checkpoints/SKILL.md).
