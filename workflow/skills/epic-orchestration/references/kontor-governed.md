# Kontor-governed epic orchestration

Load this file only for an epic marked **Kontor-governed**: the tracker label
`kontor-governed` plus the plan's `Orchestration: kontor-governed` (see the
[transition rule](orchestration-surface-transition.md#orchestration-surface-transition-rule)).
The `kontord` steps, receipts, gates, scheduler, `operational_gap` records and
recovery below apply to no other epic. The shared rules in
[SKILL.md](../SKILL.md) still apply.

On this surface the `kontord` epic graph is the baton.

## Contents

- [Resident tracker convergence](#resident-tracker-convergence)
- [Entry gate additions](#entry-gate-additions)
- [Native identity migration](#native-identity-migration)
- [Authority rules](#authority-rules)
- [Dispatch loop](#dispatch-loop)
- [Planning questions and milestones](#planning-questions-and-milestones)
- [Capacity and recovery](#capacity-and-recovery)
- [Completion](#completion)
- [Verification](#verification)

## Resident tracker convergence

Resident task-and-epic tracker convergence supplies the normal path. Normal
orchestration reads its receipts, conflicts and current binding state; it does
not run manual preview/apply on every turn.

- Enablement needs the project connector and matching immutable task/epic
  workflow specifications installed for the selected work and completion
  profiles.
- A missing matching workflow blocks convergence. Install the right supported
  revision; never change a frozen task profile to evade the mismatch.
- Manual `ticket-reconcile-plan` and apply stay useful for diagnosis, explicit
  recovery and initial materialization.
- An instruction below to reconcile means: verify the resident result first.
  Run manual mutations only for those explicit cases.

The project's pinned connector specification (`{{ project.tracker.spec }}` in
the selected config folder) is the field/workflow authority.

## Entry gate additions

Add these to the shared [entry gate](../SKILL.md#entry-gate):

1. For materialization, read the pinned naming reference before repair.
2. Resolve and read the exact Kontor realm, project, epic, tasks, current
   revision/cursor, selected topology, Core Team, work profiles, teams,
   completion profile, capacity configuration and subject authority.
3. Reconcile the Kontor graph with the plan, tracker bindings and source-control
   evidence.
4. Run `topology-inspect` and `topology-drift`. Identity, kind, project,
   workspace, checkout or `cwd` drift blocks admission.
5. Read ticket conflicts and a ticket-scoped reconciliation plan for each task
   on the current frontier. An unresolved conflict blocks only the affected
   task unless it invalidates the epic goal.

Do not launch from an external tracker status, chat message or runtime tab.
Only a supported Kontor command and receipt advances orchestration state.

## Native identity migration

- New native definitions use `EPIC_TRACKER_KEY`, `TASK_TRACKER_KEY` and
  `SCOPE_TRACKER_KEY`, with scope taken from the subject. Seat labels stay local
  role/slot labels.
- Preserve old token meanings, immutable pins, archived names and evidence.
  Deploy support and use Kontor upgrade preview/apply before changing existing
  pins. Read back exact native identities and desired names.
- A skill change does not install runtime enforcement. Record an unsupported
  surface or blocked migration through the existing operational-gap procedure.

## Authority rules

- All project/epic/task creation, selection, topology, placement, scheduling,
  lifecycle, gate, memory, tracker, consultation, settlement and completion
  mutations use Kontor MCP or the supported Kontor CLI over `/v1`.
- Use direct Paseo only for the one bounded containment operation the
  [transition rule](orchestration-surface-transition.md#orchestration-surface-transition-rule)
  allows when Kontor lacks or cannot perform the required surface.
- Use the tracker's MCP surface only under
  [tracker-linked-creation](../../tracker-linked-creation/SKILL.md)'s bounded fallback.
- Never edit runtime or Kontor internal state files.
- Chat is conversation, not workflow state. A seat message cannot replace a
  lifecycle, gate, evidence or completion receipt.
- Do not turn Paseo, a tracker MCP surface, a tracker UI or project CLI
  compatibility commands into a second control plane.

## Dispatch loop

### 1. Plan from current state

Call `scheduler-plan` and inspect its exact ready set, blockers, admission
window, module resources, worktree leases, profile gates, account availability
and runtime freshness. Do not recompute a private scheduler in prose.

Ready work needs no broad authorization. Use `execution-arm` only to narrow
scope, time, concurrency, budget or task set; use `execution-disarm` to stop
new admissions without cancelling live work.

Event-originated work enters the same launch loop only after a durable intake
receipt records source identity, deduplication key, trigger revision and either
operator approval or a policy-authorized auto-create decision. Any accompanying
legacy-named `auto-arm` is a separate admission narrowing, never the permission
that makes work runnable. The scheduler does not parse pull-request, CI,
monitoring or bug-report payloads. Replayed source events return the original
receipt and cannot create another child.

### 2. Enforce collision and checkout safety

Apply the shared [collision and worktree rules](../SKILL.md#collision-and-worktree-safety).

### 3. Materialize and reuse topology through Kontor

Use `topology-ensure` and `topology-materialize`; verify with
`topology-inspect` and `topology-drift`.

Under the selected pinned topology, one ESW contains:

- one ECP with distinct persistent LSA and TPM seats;
- one TSW per delivery task with one persistent seat per declared role slot;
- zero or more consultation-scoped ASWs, each named from its advised subject
  tracker key and topic;
- zero or more CSWs for Committees.

Hierarchy and native display names come from the immutable pinned Team
Definition JSON; topology validates structural legality. Native project/workspace
ids, kinds and paths come from Kontor readback. Do not compose a name in a
prompt, client or adapter, and do not create or repair Paseo projects,
workspaces or seats directly.

- An idle seat is reusable
  ([idle seat reuse](paseo-direct-native-placement.md#idle-seat-reuse)): send the
  next bounded role turn to the same binding.
- Replace a seat only through `seat-replace` after it is proven unusable and its
  predecessor evidence is preserved.
- ASW naming and reuse follow [Advisor sessions](paseo-direct-native-placement.md#advisor-sessions);
  the pinned Team Definition supplies the template, role codes and slot labels.

### 4. Start or resume the exact plan

Consume the accepted kickoff message in the existing LSA seat, including the
hold owner, condition, authorization ids and exact next checkpoint. The LSA
confirms technical readiness; the separate TPM routes the scheduler frontier.
Neither role needs a routine human "continue" within existing authorization.

For a `kickoff_ready` hold, verify the confirmed epic/child tracker bindings and
durable child worktree declarations, then read the self-lift authorization and
`hold-lift-<authorization-id>` receipt produced by final tracker activation.
Native placement and admission verification remain later fences, not
preconditions that make the hold wait on its own effects. If satisfied facts
have no lift receipt, repair the failed activation/readback and resume its exact
checkpoint. Do not create a second grant merely because a handoff arrived.

Historical holds and explicit `manual` holds remain manual. Read their exact
owner, scope and release decision; an existing authorization to resume permits
lifting through `execution-arm` without asking again. If a human decision is
actually missing, keep the hold and expose that decision and owner as
`needs_human`. Re-plan after any lift; task and scheduler projections must agree.

Use `scheduler-start` for the named ready batch and `scheduler-resume` for
queued admissions. Read every resulting TeamRun, AgentRun, binding, account,
model route, worktree and runtime identity back before accepting work or
verdicts.

Do not create undeclared role slots, duplicate persistent seats, or a new run
to shrink context. After each bounded role turn:

1. persist the handoff/verdict/evidence;
2. settle it through `turn-settle` or the appropriate gate surface;
3. apply the [context policy](autonomous-tpm-operation.md#context-policy);
4. confirm agent-run and native session identities are unchanged;
5. leave the seat reusable.

Wait for seat finished and end the TPM turn. The stalled-turn watchdog follows
[supervision](paseo-direct-native-placement.md#autonomous-coordination) and the
configured stale-evidence predicates.

### 5. Apply gates and lifecycle

Use the task's pinned work-profile phase/gate graph. Record verdicts with
`gate-record`; transition task lifecycle through `lifecycle-transition`.
A runtime's self-report is evidence, not authority.

On rejection, route the evidence to the same responsible seat. Keep rejection
counters per task, gate and reviewer across runs. Repeated rejection,
workspace mismatch, degraded verdict, no-output timeout or unsafe dirt enters
the typed Kontor recovery/attention path. Do not invent a second private state
machine.

Use Advisors and Committees only through their Kontor invocation and settlement
surfaces. They are read-only. A Committee result is pending until every
template-required independent finding is durable; dissent and missing evidence
remain visible.

### 6. Verify resident tracker convergence

For affected tasks and their owning epic, inspect confirmed bindings, current
projections, resident reconciliation receipts and `ticket-conflicts-list`.
Require the matching installed workflow specs for the frozen profiles and check
that observed tracker state converges. Inbound comments are pulled only when
authorized. Do not run a manual reconciliation mutation on every role turn.
After plan or epic-description changes, recheck the epic content contract
against the observed tracker body and record the result. Status convergence
alone does not prove description completeness or Goal equality.

For diagnosis or explicit recovery, use `ticket-reconcile-plan`, review the
external observation and deterministic action, then apply only with its returned
projection hash, stable idempotency key and current authority. Refetch and require
convergence. Initial tracker materialization follows the linked-creation skill.

- Never hard-code status or transition ids.
- Never derive a tracker assignee from a seat, role, team, provider, model or
  account. Preserve an existing human assignee; use the Kontor ticket-claim
  flow for an authorized self-claim.
- External human movement that conflicts with the pinned workflow becomes a
  typed conflict, not an overwrite.
- Do not use a direct tracker sync command, direct credentials or a tracker MCP
  surface on the normal path.

## Planning questions and milestones

- Apply the existing supported admission controls and readback to a planning
  question. A prose question entry alone is not proof that the scheduler is
  blocked.
- Kontor-governed milestones remain intent until supported bindings, gates and
  completion semantics are qualified through the owning workflow.

## Capacity and recovery

Capacity follows the [placement reference](paseo-direct-native-placement.md#capacity),
counted as active TeamRuns.

A missing Kontor capability is an `operational_gap`, not permission to move
authority:

1. record the intended tool, failure class, exact realm/project/epic/task/run/
   seat identities, checkpoint, fallback action, owner and status;
2. use only the bounded fallback operation the transition rule permits;
3. repair the Kontor gap with the smallest regression test and correction,
   choosing the correction by the preference in the transition rule: remove the
   faulty path, then a type or schema, then a guard, then a regression test
   built from the real failing case; prose last. The preference supplements,
   never replaces, the repair steps: the regression test, redeploy and readback
   remain mandatory;
4. redeploy the same realm/runtime fleet and verify identity/state readback;
5. notify the blocked LSA/TPM and resume from the recorded checkpoint.

If Kontor cannot accept the evidence, create the dated repository gap report
the transition rule requires and attach it after recovery.

## Completion

Drive epic completion through `completion-get`, `completion-advance` and
`completion-remediate`:

1. every declared task goal is complete or explicitly deferred with owner,
   trigger and preserved artifact;
2. integration, review, Committee, remediation, merge/release, version,
   summary, notification and archive evidence required by the pinned
   completion profile is present;
3. every tracker binding is conflict-free and ticket-scoped reconciliation
   converges;
4. the observed tracker epic still passes the linked epic content contract, its
   Goal matches the current agreed plan, and its Done means checks actually pass;
5. the shared memory disposition is recorded; any required experience proposal,
   authorized approval and projection/recall evidence meet the owning Goal and
   selected completion profile, without treating proposal as approval;
6. final Kontor and external readback confirms the epic's terminal state.

Do not close the tracker epic directly. The behaviour-scenario evidence review
in [SKILL.md](../SKILL.md#completion-and-closeout) runs inside the pinned gates.

## Verification

Before declaring completion, read the Kontor epic at the final revision, run
its closure evidence, inspect topology and capacity for leaked or duplicate
state, and require converged tracker plans. The shared
[verification](../SKILL.md#verification) still applies.
