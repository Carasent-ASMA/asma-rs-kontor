# The Kontor workflow

This guide is the method. It applies to work delivered in this repository and
to a project that adopts the Kontor workflow pack. Terms are defined once in
[`GLOSSARY.md`](GLOSSARY.md); principles are in [`PRINCIPLES.md`](PRINCIPLES.md).
Kontor decisions are recorded in [`decisions/`](decisions/README.md).

The workflow is one method with two orchestration surfaces. The surface decides
who coordinates; it does not change the roles, phases, gates, evidence or
closeout rules.

| Surface | Plan field | Who coordinates | What it guarantees |
| --- | --- | --- | --- |
| **Paseo-direct** (default) | `orchestration: paseo` | A coordinator seat through the plan, the tracker, the project CLI and the runtime | The full method, checked by procedure and evidence |
| **Kontor-governed** (opt-in) | `orchestration: kontor` | `kontord`, with the same coordinator roles around it | The full method, enforced deterministically by the daemon |

`workflow: kontor | free` is independent of the surface. `workflow: free` keeps
the safety floor and any named workflow piece, and drops the remaining
obligations. `workflow: free` with `orchestration: kontor` is not a valid
combination.

## 1. Words before rules

- **Kontor** is the workflow: roles, seats, skills, naming, handoffs, reviews,
  gates and completion rules.
- **`kontord`** is the optional deterministic control plane: daemon, store,
  `/v1` API, MCP server, CLI and console.
- **Paseo-direct** runs the workflow without a daemon. `kontord` unavailability
  never blocks it.
- **Kontor-governed** runs the workflow through `kontord`. An epic is governed
  only when the plan states the governed surface and the tracker epic carries
  the matching governance marker.

In prose, write **Kontor** for the method, **Kontor-governed** for the surface,
and **`kontord`** for the daemon. The shared value `kontor` appears in two plan
fields; it is not a synonym for the daemon. See the glossary note on the
[`kontor` value](GLOSSARY.md#the-kontor-value).

## 2. Roles and personas

Role codes are canonical. A persona is a label on a role slot, never a
different source of authority.

| Role | Owns |
| --- | --- |
| **LSA** — Lead Software Architect | Architecture, scope coherence, cross-task decisions and remediation design |
| **TPM** — Technical Program Manager | Routing, admission, dependencies, evidence, closeout and the plan revision |
| **Adam** | The coordinator persona used for new work; the single human-facing tab for one epic |
| **SWE** — delivery seat | Implements one scoped task in its own checkout and produces the task evidence |
| **QA** | Independent task-level verification; a rejection returns rework to the fix owner |
| **AUD** | Independent audit of a change or delivery evidence beyond ordinary tests |
| **REV A**, **REV B**, **JUDGE** | Read-only reviewers and the finding checker in a review room; reviewers use contrasting vendors and the judge is fresh |
| **REV SEC**, **REV DB** | Specialist reviewers for security or database risk |

Two personas deserve their own line:

- **Coordinator Adam** runs one epic and is the only tab the operator talks to.
  On existing work the TPM seat performs the same coordination.
- **LSA advisor** is started on a fixed hard-question trigger: architecture or a
  decision record, a scope change, a blocking reviewer disagreement, a second
  rejection, security, tenant data, money, or the coordinator being unsure.
  It is not a standing second coordinator.

The coordinator and the advisor are separate identities even when they share a
workspace. Under Kontor-governed orchestration, a seat's authority comes from
its credential tier and its snapshotted role slot. On Paseo-direct, it comes
from the selected plan, the role the coordinator assigned and the native session
readback. In both cases, a self-description in chat changes neither.

## 3. Kickoff to closeout

### 3.1 Readiness

Before work is admitted:

1. Read the repository entry file, the owning plan and the project
   configuration.
2. Reconcile the plan, the tracker bindings and source control. In a polyrepo,
   inspect each repository separately.
3. Confirm one tracker epic binding and one issue binding per delivery task.
4. Validate the plan and its dependency graph with the project's supported
   validator. A failed validation blocks readiness; never invent a binding or
   waive the check.
5. Check that the epic description states the agreed scope and its defined
   outcome. Missing content blocks the kickoff.

A hold at kickoff carries the same four fields as every other hold (§5).

### 3.2 Plan and graph

The plan is the durable authority for scope, dependencies, waves, gates and
closeout. Keep one plan-revision writer. Tasks are sized by outcome, not by
file edit or commit. A dependency edge names a real input; a wave is a launch
set, never a unit of deliverable scope.

The plan records at least:

- the surface and workflow value;
- the epic and task bindings;
- the dependency graph and any external fence;
- the verification for each task;
- the holds, if any, with reason, owner, lift condition and review date;
- the final acceptance gate and closeout evidence.

### 3.3 Admission and placement

The coordinator admits only ready work, within the declared concurrency and
capacity limits, and with an exclusive checkout per concurrent writer. A
worktree is checkout isolation, not a seat. Colliding tasks either serialize or
prove isolated paths before launch.

On Paseo-direct, the coordinator places an epic workspace and one ticket
workspace per task, according to the project's placement procedure. On a
Kontor-governed epic, `kontord` renders placement from the pinned definition;
do not place or rename by hand.

### 3.4 Delivery

A delivery seat:

- works in its own checkout, on the project's branch grammar;
- stages explicit paths and keeps one issue or ticket to one reviewable diff;
- records the exact revision and the verification it ran;
- hands off with the artifact revision, the fulfilled and unverified criteria,
  the findings, and the next action;
- never claims a phase or gate that the evidence does not support.

Message delivery is pending communication until acknowledged. Acknowledgement
without prerequisite evidence is not a passing gate.

### 3.5 Review and gates

Every task passes the gates its work profile requires. A review has three
properties:

1. **Independence.** A reviewer never reviews its own work. Reviewers use
   contrasting vendors where the project requires it.
2. **Read-only findings.** Review seats inspect and report; they do not edit the
   change or write scheduler truth.
3. **Sealed findings.** Reviewers record findings before seeing each other's,
   and the judge aggregates by a declared rule after both are complete.

A QA rejection is **rework** against the same fix owner and the same rejected
criterion. An epic-level completion rejection is **remediation**: the LSA
designs a bounded repair and the TPM dispatches it. A rejection counter is not
reset by renaming a seat, starting another run or replaying old evidence.

### 3.6 Completion

Task completion and gate passes are necessary, not sufficient. An epic completes
only through its selected completion profile:

1. every declared task goal and evidence is present;
2. integration is verified across the affected repositories;
3. an independent, read-only verdict is issued on the frozen revision;
4. remediation, if needed, is bounded and re-reviewed;
5. the closeout evidence is complete: merge, release or version record,
   inventory, summary, notification and archive, as the profile requires.

A non-compliant verdict is not waived. A committee cannot grant itself missing
authority. A second rejection by the same gate parks the work; recovery is
bounded and ends with a prepared human decision when no safe authorized path
exists.

### 3.7 Closeout

Closeout records what was delivered, at which evidence rung, with which exact
revisions. It also records the memory disposition: either an approved reusable
lesson, or "no durable lesson" with its reason. Progress, speculation, handoffs,
open questions and verification reports stay evidence; they are not memories.

## 4. Planning questions and ambiguity

A consequential ambiguity is resolved before dependent work starts. The
executor reports the exact question, the evidence and the affected work. The
LSA disposes scope and architecture within its authority; the TPM routes
eligible work; product intent and reserved decisions go to the authorized owner.

Keep the question and answer in the owning plan or issue and link them from the
handoff. Continue independent ready work. A question alone never stops an epic
or calls for a committee. A scope cut never applies by itself: it becomes an
open gap row.

## 5. Holds

No hold is indefinite: every hold has a review date and an escalation path.
Every hold records four fields:

| Field | Requirement |
| --- | --- |
| **Reason** | One of the closed values: waiting on a named dependency, on capacity, on a named decision, on an external party, or deprioritised by the operator |
| **Owner** | The one identity that acts when the hold ends |
| **Lift condition** | A checkable condition; when it is met, the hold lifts by itself |
| **Review date** | At most 30 days ahead |

A hold stays in force until its recorded lift condition is met or the operator
lifts it. Elapsed time and silence never lift or extend a hold. At the review
date the owner resumes the work only if the lift condition is met, closes it
with a reason, or renews the hold once with new evidence. A second renewal goes
to the authorized owner as one alert with a recommendation. If the review date
passes without a met condition or an operator decision, the hold remains in
force; the owner records the overdue review and escalates once. A scope cut
stays an open gap row.

A plan with status `On Hold`, a paused epic, a held issue, an authorization
revoked or withheld to park work, and a deferred decision all carry the same
four fields. The fields record the pause; they do not create authority to resume it.

## 6. The gap ledger

There is **one gap ledger** per project. Every gap report and every typed
operational gap gets a row. A row carries:

- a stable id and a **type flag**: `kontord` gap, workflow gap, or tool advisory;
- severity, evidence and the exact revision it was observed against;
- an owner and a checkable closure condition;
- a status that follows the evidence ladder (§7).

A `kontord` defect observed on a Paseo-direct epic is recorded and carried on;
the observing seat does not contain or repair it. A workflow gap is fixed in the
owning workflow document or skill. A tool advisory closes by a re-test or
retires with a cited reason; it is never excluded from the count.

A row closes only when the cause is removed, the smallest regression test exists,
the correction is present, and — where the claim is runtime — the release and
readback are recorded. "Green" is not a closure reason.

## 7. Evidence ladder and status

Every claim names its rung: implemented, merged, qualified, released, deployed
with readback, or enabled. The rungs and the evidence each one requires are
defined once in the [evidence ladder](GLOSSARY.md#the-evidence-ladder). A status
may be green only at the rung its evidence supports; otherwise it stays in
progress, on hold or planned.

## 8. Decisions

A decision lives in a reviewed decision record, never as an assertion in chat
or as a live fact in prose. Kontor decisions that govern this repository and
the workflow pack are recorded in [`decisions/`](decisions/README.md). A
consuming project's product and architecture decisions stay in that project's
own decision records.

A decision record states its status, authority and scope, the context, the
decision, the consequences and the alternatives considered. A deviation from an
accepted decision requires an explicitly superseding record. Only public
decisions and public evidence belong in this repository.

## 9. Configuration and documentation

Values that change by project, user or machine — tracker, repositories, base
branch, roles, teams, fleet routes, host paths and credentials — belong in the
project's selected configuration folder, not in code, skills or prose.
Credentials stay references; the values stay in the existing secret store.

Every fact has one home:

- a **decision** lives in a reviewed decision record;
- a **live fact** is read live, or written "as of" a time and a command;
- **history** is a dated report.

New and changed documents are routed once from the owning index. Do not add a
second router or duplicate a rule that already has an owner.

## 10. The two surfaces in practice

| Step | Paseo-direct | Kontor-governed |
| --- | --- | --- |
| Placement | Coordinator places the epic and ticket workspaces with the project CLI | `kontord` renders placement from the pinned definition |
| Admission | Coordinator checks readiness, capacity and isolation | `kontord` evaluates admission deterministically |
| Tracker | Coordinator writes through the supported tracker command | `kontord` projects the tracker state from evidence |
| Evidence | Recorded in the plan, the pull request and the delivery evidence | Recorded as typed evidence and receipts |
| Completion | Coordinator runs the independent review and records the verdict | The pinned completion profile runs the verdict and closeout |
| Failure | Coordinator repairs or escalates with the same bounds | Typed recovery, parked work, `NEEDS_HUMAN` with a prepared brief |

A procedure that needs a daemon call belongs to the Kontor-governed column and
says so. A Paseo-direct rule never depends on, blocks on, or requires `kontord`.
