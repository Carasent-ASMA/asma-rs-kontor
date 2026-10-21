# Kontor-governed kickoff

Load only for an epic that is Kontor-governed under the
[transition rule](../../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule).
The Paseo-direct checklist in [SKILL.md](../SKILL.md) uses none of this.
Steps 1 and 2 of the [procedure](../SKILL.md#procedure-both-surfaces), the
preconditions and the verification in SKILL.md apply here too; this file adds
the `kontord` parts.

## `kontord` model

For a Kontor-governed epic the backlog is the `kontord` graph. `kontord` is the
control plane, the tracker is the linked external workflow system, and no
second progress ledger exists.

## Output contract

A complete kickoff has:

- one authoritative implementation plan with a single closure-condition goal;
- one Kontor project and mini-project/epic with stable ids;
- one Kontor task per reviewable delivery unit, with structured module,
  dependency, worktree, work-profile, team, and account-policy state;
- one confirmed tracker epic binding and one confirmed tracker issue binding
  for every delivery task, created or linked through Kontor;
- a recorded memory disposition: no durable lesson, an evidence draft/proposal
  pending authorized approval, or an exact approved revision with separate
  projection/recall observations when performed;
- a persisted epic-wide kickoff hold with its owner, machine-checkable lift
  condition and receipt; readback proves either that it still blocks or that
  its recorded condition produced a replacement authorization;
- a handoff naming the exact project id, epic id, task ids, tracker keys,
  dependency frontier, blockers, and selected orchestration surface, delivered
  to the persistent LSA seat through Kontor.

There is no duplicate tracker ticket, `jira:` tag hierarchy, or duplicated
progress checklist. The Kontor epic graph is the baton.

## Preconditions

- One user approval of the plan/batch authorizes its in-scope Kontor and tracker
  materialization. Ask again only if a later preview expands or changes scope.
- Resolve the exact Kontor realm and project. Read `subject-authority-get` for
  memory and backlog; never assume a legacy project completed cutover.
- For memory context, cite an existing frozen binding or use bounded eligible
  recall only after its reviewed registry and deployed capability are verified.
  Record unavailable recall honestly; do not load the whole corpus as fallback.

## Procedure

Steps 1 and 2 are in [SKILL.md](../SKILL.md#procedure-both-surfaces). In step 2,
Kontor selections still use supported structured fields and exact readback,
never invented payloads.

### 3. Ensure the Kontor project

Use the supported Kontor project read/ensure surfaces. Read back project id,
revision, backlog and memory authority, selected topology, and relevant
catalogs. A fresh `kontor_native` project accepts native backlog/memory writes.
A legacy import-pending subject must complete its receipt-backed
import/readback/cutover first; do not create a second writer or fabricate an
empty import.

Record origin separately from work type. Manual kickoff, scheduled work, pull
requests, CI failures, monitoring alerts and bug reports may produce the same
profile-backed task graph. Kontor event intake is available
(`kontor_trigger_publish`, `kontor_intake_submit`,
`kontor_intake_receipt_get`): retain the canonical source id, matched trigger
revision, deduplication key, intake-decision receipt (operator approval or
policy-authorized auto-create), any separate automatic-admission narrowing
receipt, and generated child ids. `auto-arm` is a legacy name for an optional
narrowing, not permission that makes otherwise-ready work runnable; never hide
source parsing or automatic launch inside the scheduler.

### 4. Preview and apply the Kontor epic graph

Use `epic-preview` before `epic-apply`. The preview must contain the exact
task set and typed dependencies from the plan. Every task records its primary
module resource and worktree policy; no tag substitutes for structured state.

Review the preview for stable work-item-code-to-task-id mapping, no dangling or
cyclic dependency, no hidden scope reduction, collision-safe module/worktree
choices, expected project/topology identity, and an immutable preview
hash/revision.

Include `initial_hold` in the matching `epic-preview` and `epic-apply` requests.
Read its previewed epic-wide scope, revoked state, `held_by` account profile and
`lift_condition` before apply. The deployed graph surface records this hold
before the new epic can become eligible; do not create eligible work and stop
it afterward. If that surface is unavailable, record the deployment gap and
repair it before graph creation.

Apply with a stable idempotency key and required expected revision. Read the
epic back and confirm graph, task ids, dependencies, and revision. Replaying
the same apply must not duplicate tasks.

### 4a. Persist and verify the kickoff hold

Ready work runs without `execution-arm`; omitting that call is not a hold.
For an approved initiative, request `initial_hold` with the exact `held_by`
account profile, a bounded `reason`, and `lift_condition: kickoff_ready`.
`kickoff_ready` means the epic and every child have confirmed tracker bindings
and every child has a durable worktree declaration. It does not claim that
native placement or scheduler worktree verification has already happened.
Requiring those later admission effects before lifting would make the hold wait
on itself.

1. After apply, retain the hold authorization and revocation receipts. Read the
   epic, tasks and scheduler plan: the covering revoked authorization must block
   admission while its condition is unmet. A prior dependency/governance blocker
   may mask `authorization_blocked`; also inspect the persisted authorization.
2. Complete profiles, teams, account policy, module/worktree declarations, plan
   publication and required content checks before final tracker materialization.
   The final confirmation/activation boundary evaluates `kickoff_ready`; do not
   leave additional kickoff checks dependent on the hold remaining in force.
3. Read back the replacement authorization and its ordinary command receipt,
   identified by `hold-lift-<authorization-id>`, plus scheduler/task state.
   The lift retains the recorded owner and concurrency ceiling. It is not an
   extra trigger-spec installation or a human permission prompt. Deliver the
   leadership handoff in step 8 and let the LSA/TPM continue authorized work.

Use `manual` only when a named decision really remains with a human. State the
owner, decision and release condition in the hold reason and handoff. Omitted
conditions and historical holds remain manual; never infer automatic consent
from their age. An already-authorized operator may lift the exact manual hold
through `execution-arm`, then re-plan. A `kickoff_ready` hold that stays revoked
after its facts are durable is a repairable control-plane gap, not a request for
the user to repeat permission.

Verify the deployed capability and receipts; editing this skill does not deploy
it.

### 5. Select profiles, teams, and accounts

Apply [ticket-triage-tiers](../../ticket-triage-tiers/SKILL.md) through Kontor's profile/team/account selection
surfaces. Resolve live catalog revisions and ids; do not hard-code a team id,
saved agent, provider, or model. Read every task back and confirm selections
before scheduler admission.

A tracker assignee is human issue ownership. It is never derived from a Kontor
seat, role, team, provider, model, or account profile.

### 6. Materialize the tracker through Kontor

Use [tracker-linked-creation](../../tracker-linked-creation/SKILL.md) for one epic-first preview/apply operation.
The tracker epic projection must pass the
[epic content contract](../../tracker-linked-creation/SKILL.md#epic-content-contract):
Summary / purpose, Goal, Scope and sub-goals, Done means, and Detailed plan,
in that order. Validate the actual projection before apply and the observed
tracker description afterward, including the verbatim plan Goal and plan link.
Complete confirmed child links and verify plan publication before handoff.

Let Kontor's pinned connector field/workflow specs derive issue types, required
fields, body encoding, parent links, status policy, and assignee actions. Do not
call the tracker directly, copy historical custom-field ids, or run a direct
tracker sync command.

Read back one tracker epic binding and one tracker issue binding per delivery
task. Ambiguous duplicates, conflicting parent/type/field observations,
incomplete refetch, or unresolved ticket conflicts block kickoff.
Record the content-check evidence with the handoff. Missing sections,
placeholders, a mismatched Goal, or missing description readback also block
kickoff completion; keep the existing admission hold in force while repairing.

### 7. Publish project memory

For a durable lesson from already settled work, use the project's
[experience-memory procedure](../../experience-memory/SKILL.md) to collect
exact evidence, preview the typed
revision and propose through the supported canonical surface. Proposal and
authorized approval are separate; read back the exact revision/hash and record
projection/recall observations only when performed. Use the accepted
`kontor_experience_propose` operation and the contract pins documented by that
procedure; its operation table supplies the HTTP/CLI/MCP names and fixtures.

An unshipped kickoff plan, progress, speculation, handoffs, questions, gaps,
source and verification reports remain evidence. If no settled reusable lesson
exists, record "no durable lesson" and its reason; create no placeholder memory.
Keep instruction-file preferences in the project's preference procedure. Never
edit a legacy memory mirror.

### 8. Hand off to the persistent LSA seat

Report the plan path and goal; Kontor realm/project/epic ids and revision;
task ids/work-item codes/tracker keys in dependency order; initial scheduler
frontier and module constraints; selected profiles/teams and any account pin;
unresolved conflicts or external fences; and the exact resume checkpoint for
[epic-orchestration](../../epic-orchestration/SKILL.md).

Materialize or recover the pinned ECP and its existing logical LSA/TPM seats
through Kontor. Read back the exact project, workspace, checkout and native
bindings. Send the bounded checkpoint with `kontor_topology_seat_message_send`
to the LSA seat and retain its accepted message id and native-id readback. The
LSA owns technical readiness and hands routing to the separate TPM seat; kickoff
does not execute child delivery or replace these seats with the human operator.

Include hold owner, condition and current hold/lift receipts. Ask the LSA to
continue within the existing authorization, persist its disposition and keep
the same seat reusable. Acknowledged delivery is the handoff checkpoint; a chat
summary to the user alone is not delivery. If the seat cannot receive it, repair
that exact binding/surface and resume here. Report the checkpoint to the user
without a routine request to type "continue".

## Kontor or tracker failure

- If Kontor is unavailable before the graph exists, kickoff is blocked. A
  tracker MCP surface cannot replace the missing control plane, so do not create
  tracker-first orphan issues.
- If tracker materialization fails after the Kontor graph is durable, follow
  [tracker-linked-creation](../../tracker-linked-creation/SKILL.md)'s bounded tracker fallback, record
  `operational_gap` evidence, and resume from the same Kontor checkpoint.
- Never move the rest of kickoff into a tracker UI, Paseo or a tracker MCP
  surface because one Kontor operation failed.

## Verification

In addition to the verification in [SKILL.md](../SKILL.md#verification):

- The handoff also records the Kontor revision checked with the epic
  description.
- Kontor contains every planned task and only those tasks.
- Every task has structured module/worktree/dependency and selection state.
- Tracker bindings are confirmed through Kontor receipts with correct parents.
- An unmet hold blocks admission; a satisfied `kickoff_ready` hold has its
  replacement authorization and receipt. A remaining manual hold names the real
  decision and owner.
- The persistent LSA received the exact handoff through Kontor; accepted
  message/native identity and the separate TPM routing seat are recorded.
- No direct tracker credential path was introduced.
