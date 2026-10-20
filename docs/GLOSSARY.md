# Kontor glossary

This is the one dictionary for the Kontor workflow. A procedure belongs in
[`WORKFLOW.md`](WORKFLOW.md); a principle belongs in
[`PRINCIPLES.md`](PRINCIPLES.md). Terms are defined here once, and other
documents link to them instead of restating them.

## Workflow and surfaces

| Term | Definition | Avoid |
| --- | --- | --- |
| **Kontor** | The workflow: roles, seats, skills, naming, handoffs, reviews, committees, gates and completion rules. It runs with or without `kontord`. | the daemon; a product host |
| **Kontor workflow** | The full name of **Kontor** when the context could confuse it with the software. | daemon-only workflow |
| **`kontord`** | The optional deterministic Rust control plane: daemon, store, `/v1` API, MCP server, CLI and console. | Kontor (bare); the workflow |
| **Orchestration surface** | How an epic runs the workflow: **Paseo-direct** or **Kontor-governed**. Resolved before kickoff. | workflow definition; runtime; mode |
| **Paseo-direct** (`orchestration: paseo`) | The workflow run by agents through Paseo, the plan, the tracker and the project CLI, without a daemon. The default. | a lesser workflow; legacy mode |
| **Kontor-governed** (`orchestration: kontor`) | The workflow run through `kontord`, which owns the task graph, admission, evidence and tracker projection deterministically. Requires the plan marker and the tracker governance marker. | the workflow itself; Kontor (bare) |
| **`workflow: kontor \| free`** | Whether the epic follows the Kontor workflow at all. `free` keeps the safety floor and any named workflow piece. | the surface |
| **Safety floor** | The rules that hold in every workflow and are never switched off: the tracker stays in the project's configured tracker project; never overwrite another person's work, one writer per branch; no force-push and no ruleset bypass; every check the repository defines must pass; merge only on the host owner's command or the pull request author's merge order in the project's configured merge rule; no secrets in files; every pull request is reviewed. Secrets stay references and the public tree carries only public material. | optional guardrails |
| **Workflow piece** | A named procedure used on its own — a review, a committee, a takeover, a handoff, a plan hardening, a decision record. It brings its own rules only while it runs. | workflow stage; mandatory step |

### The `kontor` value

`workflow: kontor` and `orchestration: kontor` use the same word for two
different things. The values are kept for parser and plan compatibility. In
prose, write **Kontor** for the method, **Kontor-governed** for the surface and
**`kontord`** for the daemon. A bare "Kontor" never selects daemon governance.

## Work and planning

| Term | Definition | Avoid |
| --- | --- | --- |
| **MiniProject** | The tracker-neutral Kontor aggregate that coordinates one goal and its tasks. | tracker epic; Paseo project |
| **Epic** | A policy-qualified MiniProject with one confirmed tracker epic binding. | kernel aggregate; workspace |
| **Task** | The operational work item that owns goal, dependencies, lifecycle, evidence and its external projection. | ticket; issue |
| **Delivery Task** | One meaningful unit of ownership and acceptance, bound to a confirmed tracker issue and, when governed, a Kontor task. It may span several commits. | file edit; commit; pull request |
| **Implementation Step** | A checklist action inside a Delivery Task; it ordinarily has no separate issue, team or checkout. | Delivery Task |
| **Implementation Plan** | The durable plan that records scope, dependencies, waves, verification and closeout for one change. | brainstorm; notes |
| **Phase** | A goal-bounded group of work inside one plan, with its own completion criteria. Answers *what this delivers*. | wave; sprint |
| **Wave** | A set of work items launched together, bounded by collision and capacity rules. Answers *what starts at the same time*. A wave is never a unit of deliverable scope. | phase; release |
| **Frontier** | The lowest wave in the plan's dependency graph (see [Wave](#work-and-planning)) that still contains a task that is neither done nor cancelled. A statement about the plan, not about capacity. | ready set; launch set |
| **Acceptance criteria** | The behavioural outcomes that define when a task and its bound issue may be completed. | task notes; guesses |
| **Milestone** | A plan-defined, independently useful increment that is safely deployable and verifiable, delivered through a pull request or a coordinated set. | phase; release |
| **Hold** | A recorded pause with a reason, an owner, a checkable lift condition and a review date at most 30 days ahead. The procedure is in [`WORKFLOW.md`](WORKFLOW.md#5-holds). | indefinite wait; blocker note |
| **Gap ledger** | The project's one list of every gap report and typed operational gap, each with a type flag, owner and closure condition. The procedure is in [`WORKFLOW.md`](WORKFLOW.md#6-the-gap-ledger). | scattered bug lists |

## Roles, seats and personas

Role codes are canonical. A persona is a label on a role slot, never a separate
source of authority.

| Term | Definition | Avoid |
| --- | --- | --- |
| **LSA** | Lead Software Architect. Owns architecture, scope coherence, cross-task decisions and remediation design. | solution architect; epic lead |
| **TPM** | Technical Program Manager. Owns routing, admission, dependencies, evidence, closeout and the plan revision. | orchestrator; project manager |
| **Adam** | The coordinator persona used for new work. It fills the coordinator slot and is the single human-facing tab for one epic. | a role code; the daemon |
| **Coordinator Adam** | The Adam tab that runs one epic. On existing work the TPM seat performs the same coordination. | standing TPM; second coordinator |
| **Brainstorm Adam** | The premium coordinator that turns an idea into a plan and hands the approved plan to the coordinator. | planner; LSA |
| **LSA advisor** | The on-demand premium advisor started on a fixed hard-question trigger. Not a standing second coordinator. | TPM; co-coordinator |
| **SWE** | The delivery seat. Implements one scoped task and produces its evidence. | builder; engineer |
| **QA** | Independent task-level verification. A rejection returns rework to the fix owner. | tester; reviewer |
| **AUD** | Independent audit of a change or delivery evidence beyond ordinary tests. | QA; reviewer |
| **REV A**, **REV B** | The two read-only reviewers in a review room. They use contrasting vendors. | AUD; author |
| **JUDGE** | The read-only seat that checks each finding against the exact code and aggregates after both reviewers are complete. | REV A; approver |
| **REV SEC**, **REV DB** | Specialist read-only reviewers for security or database risk. | mandatory seats |
| **PR Adam** | The coordinator that runs one pull-request review room. It never merges. | JUDGE; merger |
| **Seat** | A persistent role occupant bound to one role slot. An idle seat remains reusable. | process; tab; TeamRun |
| **TeamRun** | Execution state attached to one ticket workspace. It is not a topology node and not a container. | seat; workspace |

## Codes and workspaces

| Code | Name | Definition |
| --- | --- | --- |
| **PSW** | Project Session Workspace | The logical root that groups a project's quick and epic workspaces. |
| **QSW** | Quick Session Workspace | An ad-hoc one-role workspace under the PSW that may later hand off to an epic. |
| **ESW** | Epic Session Workspace | The logical boundary for one epic and the parent of its control, ticket, advisor and committee workspaces. |
| **ECP** | Epic Control Plane | The ESW child that hosts the epic's persistent leadership and coordination seats. |
| **TSW** | Ticket Session Workspace | The delivery-capable ESW child for one ticket; it owns that ticket's persistent delivery seats. |
| **ASW** | Advisor Session Workspace | A read-only ESW child for one advised subject or topic. It does not aggregate a verdict. |
| **CSW** | Committee Session Workspace | A read-only ESW child for one committee run; it owns the seats its pinned template declares. |
| **DESK** | Desk project | A durable top-level workspace named `DESK • <name>`, such as `DESK • ADAM` or `DESK • PR REVIEW`. |
| **RSW** | Review work room | A review room named `RSW • <tracker key> • <repository>#<number>` that holds the review seats for one pull request. |

An ESW is the complete epic boundary; an ECP is its leadership and coordination
host. They are not synonyms. Paseo cannot nest projects, so a PSW-to-ESW
relation is logical: the PSW may bind to one base project while each ESW binds
to its own project.

## Evidence and records

| Term | Definition | Avoid |
| --- | --- | --- |
| **Evidence** | A durable record that a claim is true at an exact revision: a command and result, a test, a review finding, a receipt, a readback, a snapshot hash. | a status; an opinion |
| **Evidence ladder** | The six rungs below. Every claim names its rung. | done; green; shipped |
| **Status** | The state of a document or work item in the project's status vocabulary. A green status is valid only at the rung its evidence supports. | evidence |
| **Gap** | A verified difference between what the method or product claims and what it does, with a type flag and an owner. | TODO; wish |
| **Decision record** | A reviewed record of one decision, its authority, context, consequences and alternatives. Kontor decisions live in [`decisions/`](decisions/README.md). | chat agreement; plan note |
| **Experience memory** | A proposed, approved and versioned reusable lesson, injected into a future seat's context. A proposal is not approval. | transcript; status report |
| **Context Pack** | The exact, frozen selection of memory, evidence and instructions a seat receives, with the revision ids and hashes it selected. | prompt dump |

### The evidence ladder

| Rung | Required evidence |
| --- | --- |
| **Implemented** | The change exists in a working tree, at an exact revision. |
| **Merged** | The change is on the project's default branch, with the exact commit and the reviewed change. |
| **Qualified** | The merged revision passed the declared gates on that exact tree: tests, review and mutation evidence where applicable. |
| **Released** | A versioned artifact contains the qualified revision, with a release record and artifact identity. |
| **Deployed with readback** | The released artifact is installed or running in the named environment, and a readback proves its identity and behaviour. |
| **Enabled** | The deployed capability is switched on for the named project, realm or user, with a readback of that enablement. |

`Merged` is not `qualified`; `qualified` is not `released`; `released` is not
`deployed`; `deployed` is not `enabled`.

### Status vocabulary

| Status | Meaning |
| --- | --- |
| **Planned** | Scoped and approved, not started |
| **In progress** | Work has started and is not complete |
| **On Hold** | Paused under a valid four-field hold |
| **Completed** | The claimed outcome is complete at the named evidence rung, with no open task |
| **Deprecated** | Replaced or withdrawn, with the superseding record named |

A status is never upgraded by a calendar date, a merge alone, or a green graph.
