---
name: agent-dispatch
description: Use when a short request must start or delegate work to a declared role seat — start a job, start a named role, delegate delivery, run Independent Review, or ask a supported advisor or formal committee. Not for phase handoff, rework, resume, takeover or epic kickoff.
---

# Agent dispatch

Turns a bounded short request into one correctly placed, briefed occupant for a
**declared logical seat**. Dispatch selects **who** may launch **which** seat under
the selected workflow; it does not resolve model chains, publish YAML, route
through the task/path router, or substitute for [epic-orchestration](../epic-orchestration/SKILL.md) admission.

Vocabulary: [Kontor glossary](../../../docs/GLOSSARY.md#roles-seats-and-personas).
Contract: [the workflow guide](../../../docs/WORKFLOW.md#3-kickoff-to-closeout).

## When to use

**Use for:** "start a job", "start QA for `<tracker-key>`", "delegate SWE on ...", "run
Independent Review on ...", "ask the advisor about ...", or an authorized
TPM/Adam dispatch after readiness checks.

**Do not use for:** phase completion ([seat-handoff](../seat-handoff/SKILL.md)), rework routing
([seat-handoff](../seat-handoff/SKILL.md)), resume or replace an existing occupant ([seat-takeover](../seat-takeover/SKILL.md)),
epic kickoff ([mini-project-kickoff](../mini-project-kickoff/SKILL.md)), or ordinary peer clarification (no
skill — message only; no scope or gate change).

**Wrong trigger:** If the user intends continuation of the **same unfinished**
seat work, classify as [seat-takeover](../seat-takeover/SKILL.md) or an explicitly authorized scope
transfer, not dispatch to a downstream phase.

## Orchestration surface

Resolve the surface per the
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule).
Record the selected workflow revision, surface and policy provenance references;
do not copy policy tables into the briefing.

| Concern | Paseo-direct | Kontor-governed |
| --- | --- | --- |
| Who may dispatch delivery seats | TPM (or operator with TPM authority); Adam on new work | TPM through supported scheduler/admission surfaces; Adam on new work |
| Who may dispatch Adam/LSA work | Authorized kickoff/operator or Adam self-start per plan | Supported Core Team / epic bindings |
| Placement | Epic `projectId`, exact `workspaceId`, kind and `cwd` readback before seats | `kontord` topology materialization; same readback discipline |
| Parentage | Native caller is parent; on existing work Adam → TPM → delivery, on new work Adam → delivery; consultations report to caller | Same intended tree; verify supported parent projection |
| Policy inputs | Activated project YAML when the selected policy qualifies; until then the selected plan + role references | Same source through materialized revisions |

The Paseo-direct surface must not require a `kontord` read for unrelated work.
A missing required configuration blocks **only** the affected launch with an
actionable reason.

## Authority split

The split differs for existing work and new work. **Existing work keeps the TPM
model; on new work Adam is the coordinator.**

- **Adam / epic LSA:** architecture, scope, remediation design, consultation
  requests, plan revisions (one publisher at a time). On existing work, Adam
  does not independently admit delivery work that the TPM has not accepted. On
  new work, Adam is the coordinator and holds delivery admission.
- **TPM:** on existing work, delivery admission, TSW/ASW/CSW placement (only
  the epic TPM creates or reuses consultation workspaces), dispatch of ready
  tasks, rework/remediation routing after classification in [seat-handoff](../seat-handoff/SKILL.md).
  On new work, Adam holds these duties.
- **Callers vs placement:** An authorized role may **request** dispatch; only
  the placement owner (the TPM on existing work; Adam on new work; or the
  supported operator path) **places** ASW/CSW. Advice never substitutes for a
  required formal gate.

## First line is the exact seat name

The first line of every first message, handoff, takeover brief or prompt is
the receiving session's exact name, alone on the line. An unregistered session
uses its exact Paseo session title in capitals. On a Kontor-governed epic, use
the exact registered role code or Team Definition slot label. On Paseo-direct
work, use the
[provisional seat naming rule](../epic-orchestration/references/paseo-direct-native-placement.md#provisional-seat-naming).

<a id="seat-start-and-communication"></a>

## Seat start and communication

The first message carries everything: the exact seat name alone on the first
line, the seat card (role, personality, hard rules, steps, output format), then
facts collected by a script. Fixed text comes first and is the same for every
seat of the same role, so the provider's prompt cache works.

Every seat follows the project's communication contract for comments, reviews,
status messages and handoffs. It owns the shared plain language, brevity and
requested-language rules; the exact first-line rule above still applies to seat
prompts.

Seats also keep the
[three homes for information](../../../docs/WORKFLOW.md#3-kickoff-to-closeout):
decisions in Git with owner and date, facts read live, history in dated reports.

<a id="adam-lsa-and-advisor-triggers"></a>

## Adam, LSA and advisor triggers (new work)

For new work, **Adam is the coordinator** (the job TPM does today) and the only
tab the operator talks to. The **LSA is an on-demand premium advisor child**:
Adam starts it for a hard question and it reports back to him. It is not a
standing second coordinator.

Start an advisor child when any fixed trigger holds:

- an architecture or decision-record question;
- a scope change;
- two reviewers disagree on a blocking finding;
- a second rejection;
- security, tenant data or money is involved;
- Adam is not sure.

A fleet-declared hard-decision chain in `{{ project.fleet }}`, when the
selected fleet defines one, is for these rare questions only. Choose it on
purpose; never make it a default route.

Existing epics keep their names, seats and placement.

## Procedure

1. **Classify the request.** Map the short text to one declared logical seat or
   supported consultation protocol (Advisor, qualified planning pair, Independent
   Review). Reject ambiguous multi-seat requests until one target is explicit.
2. **Resolve scope and workflow.** Confirm epic/task tracker key, receiving
   checkout (absolute path for the seat's real `cwd`, not the launcher's assumed
   tree), selected workflow revision and eligibility (phase, dependencies, holds,
   permissions, calendar windows).
3. **Select or reuse the seat.** Prefer an idle persistent seat for the same
   `(team_run, role_slot)`
   ([idle seats are reusable](../epic-orchestration/references/paseo-direct-native-placement.md#idle-seat-reuse));
   never create a second occupant for a duplicate request.
   Reconcile lost callbacks against existing operation/seat identity before relaunch.
4. **Resolve route without restating policy.** Consume selected slot policy through
   the shared resolver when available; record source bundle/hash and considered
   routes in evidence. Do not embed model lists, chain order or permission tables
   in the skill body or briefing.
5. **Preflight placement and parent inputs.** Fail closed if `projectId`,
   `workspaceId`, workspace kind, `cwd`, parent/supervisor mapping or checkout
   ownership fails readback.
6. **Brief.** Task goal, scope boundaries, artifact revision under review,
   forbidden effects, instruction/skill paths by reference, and absolute paths
   for the receiving checkout. Use `$HOME` or the host configuration for
   host-specific roots; never paste a personal home path into the skill.
7. **Launch** through the correct supervisor path so native parentage matches the
   intended tree: on existing work, Adam launches the TPM and the TPM launches
   delivery; on new work, Adam launches the workforce directly. Watchdog-created
   agents are not automatically Adam or TPM children.
8. **Read back** effective model/effort, permission posture, native ID, parent,
   workspace/cwd and checkout before accepting task effects or verdicts.
9. **Record** dispatch evidence: discovered / supplied / read / behaviorally
   exercised loading is classified separately; file presence is not activation.

<!-- example: project-specific -->
```text
Receiving checkout: $HOME/<project-root>/.worktrees/<key-slug>/<module>
Workspace cwd:      $HOME/<project-root>/.worktrees/<key-slug>/<module>
```
<!-- /example: project-specific -->

## Consultation dispatch

Select protocol by purpose, not by surface:

| Protocol | Dispatch rule |
| --- | --- |
| Advisor | Supported Advisor profile; read-only; caller records disposition |
| Planning pair | Only after a qualified planning-pair protocol is selected; two contrasting vendors; no Judge |
| Independent Review | Required reviewers plus Judge; conjunctive gate; never replace with planning pair |

Reuse the same ASW/CSW and seats for the same topic; a materially different
subject creates another workspace
([ASW naming and reuse](../epic-orchestration/references/paseo-direct-native-placement.md#advisor-sessions)).

## Parallel planning (dispatch boundary)

On existing work, Adam may plan independent future work while the TPM
dispatches ready delivery ([epic-orchestration](../epic-orchestration/SKILL.md)); on new work, Adam does
both. Dispatch must preserve **one plan publisher** and **one admission
writer** per surface. Ready independent work proceeds; work blocked on undecided
dependencies waits. Same-module collisions require verified isolated ownership
or serialization; integrated candidates need recheck after merge.

## Qualification reference

For authoring or reviewing this procedure, load the
[qualification scenarios and capability ownership](references/qualification.md).

## What this skill is not

- Not a router, loader, scheduler, resolver implementation or recovery loop.
- Not authority to edit the project's agent-policy routing or inventory.
- Not proof of cross-surface parity until paired runs qualify it
  ([qualification](references/qualification.md)).
- Not a model roster; fleet and eligibility live in selected policy, not here.

## Related skills

- [seat-handoff](../seat-handoff/SKILL.md) — normal handoff, rework, remediation
- [seat-takeover](../seat-takeover/SKILL.md) — resume and replacement
- [mini-project-kickoff](../mini-project-kickoff/SKILL.md) / [epic-orchestration](../epic-orchestration/SKILL.md) — durable epic procedures
  that call these lifecycle skills
- [ticket-triage-tiers](../ticket-triage-tiers/SKILL.md#tiers) — risk and
  team shape before first dispatch (Kontor-governed catalog)
