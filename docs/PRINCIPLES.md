# Kontor governing principles

These principles govern every Kontor product, plan, decision record, workflow,
team, guardrail and configuration decision. The procedure that applies them is
[`WORKFLOW.md`](WORKFLOW.md); the terms are in [`GLOSSARY.md`](GLOSSARY.md).

## The two values

### Autonomy

Kontor runs the work without the operator driving each step, and runs it the way
that operator would. Both halves are required:

- **Deterministic rails.** Phases, gates, roles, teams, topology, completion and
  consultation are versioned data the kernel executes, not behaviour a model
  improvises per session. The same inputs produce the same admission decision,
  the same gate authority and the same refusal, on every restart and from every
  client.
- **Captured personality.** What the operator decided, prefers and keeps
  correcting becomes durable, approved, versioned memory and configuration,
  injected into every seat's context deterministically. It is not re-explained
  each session and not inferred from a transcript.

Autonomy is never "the model decides". It is "the operator's decisions, made
once, become the system's behaviour, always".

### Delivery quality

The unit of value is a completed epic or ticket, and completion is defined,
checked and independently verified rather than asserted.

Delivered means production-ready: implemented, tested, reviewed by an authority
that did not do the work, integrated, released and evidenced. Delivered also
includes what happens after the merge: the workflow, teams and guardrails that
own support, operations and post-delivery correction are in scope.

The sharpest consequence: **a green task graph is not a delivered epic.** Only a
compliant independent verdict plus complete closeout evidence completes one.

## The principles

| # | Principle | What it requires |
| --- | --- | --- |
| **P1** | **One writer per fact** | Every fact has exactly one authority. Reuse a mature system before duplicating its capability. Kontor owns the cross-system join; the runtime owns native sessions, tools and provider authentication; the tracker owns external issue keys and workflow; the forge owns repositories, changes and releases. Never edit another system's internal store. Never add a second scheduler for the same task graph. A narrow capability may move into Kontor only through an explicit cutover that ends with one writer. |
| **P2** | **Uncertainty is never completion** | Desired, observed and derived state are three separate facts. `stale`, `diverged`, `runtime_unavailable`, `orphaned` and `lost_contact` are visible non-terminal states. A missing process, a closed stream, a timeout or a gap never becomes success, failure or cancellation by itself. |
| **P3** | **Intent precedes effect** | A runtime-changing command persists its intent, desired state and receipt in one transaction before dispatch, then confirms the effect from correlated evidence. After a crash, search by correlation before retrying; never blindly repeat a launch, resume, send or cancel. |
| **P4** | **The workflow is data; the kernel is generic** | Phases, gates, roles, teams, topology, naming, consultation and completion are versioned, hashed, immutable specifications. No core code branches on a profile id, role name, topology kind or work type. Adding a work type is publishing a spec, not editing the scheduler. |
| **P5** | **Proposal and authority are separate** | A model may recommend anything and submit evidence. Only a deterministic evaluator reading a pinned snapshot writes scheduler truth, gate verdicts, authority or receipts. A seat's authority comes from its credential tier and its snapshotted role slot, never from what it claims in chat. |
| **P6** | **Autonomy is bounded by narrowing, not by permission** | Ready work runs. An operator narrows it — window, concurrency, selected tasks, capacity, budget — and may stop it explicitly. Arming is a constraint, not an authorization; a disarm is a stop, not a return to an unarmed default. |
| **P7** | **Memory is proposed, approved, versioned and injected** | Any agent or human may propose an immutable memory revision. Only an authorized approval selects the current one. Retrieval excludes proposed, superseded, tombstoned and foreign-project revisions. A frozen context pack records the exact revision ids and hashes it selected, so a later approval never rewrites a running seat's brief. Search indexes are rebuildable projections and can never create or approve a memory. |
| **P8** | **One semantic surface, one authority model** | There is one semantic API boundary and one tool registry. The CLI is generated from the registry so the two cannot drift. A caller's authority is its credential tier; a serve profile may narrow what a seat is shown and may call, and can never widen it. Chat advances no lifecycle until a typed command succeeds. |
| **P9** | **A green graph is not a delivered epic** | Task completion and gate passes are necessary and not sufficient. An epic completes only through its pinned completion profile: declared goals and evidence, integration, an independent verdict, bounded remediation, and every closeout prerequisite — merge, release, version inventory, summary, notification and archive. |
| **P10** | **Final review is independent, adversarial and un-waivable** | The completion verdict is issued by a read-only committee whose members did not do the work, record findings independently before seeing each other's, preserve dissent and aggregate by a declared deterministic rule. Provider diversity is a template constraint, not a coincidence. No advisor, committee or agent may grant missing authority, approve destructive work, waive a gate or reset a rejection counter. |
| **P11** | **Evidence is frozen, and drift invalidates it** | A completion bundle names exact commits and hashes. Product change after the freeze invalidates the part of the bundle it touches; the response is re-freeze and re-review, not a waiver. Mutation evidence is re-run against the tree that is actually being accepted. |
| **P12** | **Failure is bounded and ends with a human, not a loop** | A second rejection by the same gate parks the work. Recovery is a bounded episode: deterministic inspection and safe automatic recovery first; then at most one advisor; then at most one committee for genuinely cross-cutting ambiguity; then at most two evidence-backed follow-up executions. Never a blind full-team relaunch. If no safe authorized path exists, or the budget is exhausted, the result is `NEEDS_HUMAN` with one prepared decision brief. Decisions inherently reserved to human authority go directly there. |
| **P13** | **Capability honesty** | A runtime, adapter or seat declares only what it can prove. Unsupported operations refuse typed and without side effects rather than degrading silently. Documentation says "unsupported" or "unconfigured" for an unfinished capability instead of implying it exists. |
| **P14** | **Delivery does not end at merge** | Support, operations, incident response, drift and regression checking, dependency maintenance, compliance and retirement are work types the workflow is expected to describe and run — not a separate product and not out of scope. Delivered value that nobody can operate is not delivered. |

## Applying the principles

- A proposal that improves speed while weakening P2, P5, P9, P10 or P11 is
  rejected regardless of its benefit. Those five are the delivery-quality core.
- A proposal that adds a hard-coded work type, role, topology kind or status
  name violates P4 and should be expressed as a specification.
- A proposal that requires per-task human authorization to proceed violates P6
  and should be expressed as a narrowing.
- A proposal to reproduce a mature tracker, forge or runtime capability must
  first prove that integration or embedding leaves a material need unmet. A
  narrow absorption must reduce lifetime complexity and end with one writer.
- Thin authority does not remove the operator view. The operator must be able to
  see dependency order, what is running or waiting, what needs attention, what
  can run next and whether the work is actually delivered.
- Do not ask a human to solve an ordinary recoverable failure before
  deterministic handling, an eligible policy path and bounded consultation have
  been tried. A decision inherently reserved to human authority is the explicit
  exception.
- Never make an unproven capability look available, in code or in prose.
- "This is only for one kind of work" is not a valid scope argument. "This
  capability is not built yet" is valid, and must be said out loud.
