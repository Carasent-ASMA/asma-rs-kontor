# KON-OP-03 — the bounded task reopen

Answers the operational gap the ASMA-7869 LSA recorded in
`_docs/ai-orchestration/reports/2026-08-17-13-47-report-kontor-reopen-task-terminal-gap.md`:
`reopen_task` was advertised, mapped to `ready`, given a resume receipt — and
refused with `409 revision_conflict`, "the aggregate is terminal and immutable".

## Why it could not work

Three layers agreed that a terminal task never moves, and one of them was asked
first.

| Layer | What it did |
| --- | --- |
| `apply_task_transition` | refused **every** terminal source before looking at the requested transition |
| `TaskState::can_transition_to` | had no `done -> ready` pair |
| `tasks_terminal_immutable` (SQL, v1) | aborted any update to a row in `done`, `failed` or `cancelled` |

So the action existed on the surface and had no rule behind it. The daemon was
already doing its part: `ReopenTask` records a `resume_task` receipt and passes it
to the store.

## The rule

A terminal task is still immutable, with one bounded exception:

> a `done` task presenting a `TaskReopenAuthority` may return to `ready`.

Four constraints, each one deliberate:

1. **One source state.** Only `done` is reopenable. A completed task carries a
   *claim* — its declared work is finished — and new evidence can falsify that
   claim without contradicting anything else it recorded. A `failed` or
   `cancelled` task carries an *outcome*: reopening one would mean deciding that a
   run which closed failed did not, or that withdrawn work is live again. Neither
   is a claim later evidence settles, and both have a successor task as the honest
   answer. They stay refused, and the refusal names the *transition* rather than
   the terminality, because terminality is not what stopped it.
2. **One target state.** `ready`, and nothing else. A reopen puts work back in the
   queue; it does not resume a session or declare progress.
3. **A durable authority.** `TaskReopenAuthority` can only be built from a
   `CommandReceiptId` and carries it, so the intent to reopen cannot be expressed
   without the recorded command that authorizes it. The store refuses to assemble
   one from a reopen with no receipt.
4. **Not a resume.** A resume and a reopen carry the same *kind* of receipt, so the
   store is told which one this is rather than inferring it. An ordinary `resume`
   against a completed task is still `Terminal`.

## What is preserved

Reopening changes one column. It closes no run and opens none, touches no gate
evaluation, and claims no seat — the runtime is not called at all. The history the
completion was granted on stays exactly as recorded, which is what makes the
reopen auditable instead of a rewrite. The receipt is in `command_receipts` under
`resume_task` with its canonical intent, like every other decision here.

## Schema v31

The v1 trigger is replaced by a narrower pair, the same shape v29 used for the
epic pin:

- `tasks_terminal_immutable` now aborts every terminal update **except**
  `done -> ready`;
- `tasks_reopen_changes_only_the_state` aborts a reopen that also changed the
  project, the epic, the title, the module key or the creation instant — a reopen
  that renamed a task or moved it to another epic would be a rewrite wearing a
  smaller word.

`0028`, `0029` and `0030` are untouched.

## Tests

| Where | Proves |
| --- | --- |
| `crates/kontor-core/tests/domain_state.rs` — `a_completed_task_reopens_only_under_an_explicit_authority` | an authorized `done -> ready` is allowed; `failed` and `cancelled` are refused as `task reopen` transitions; a reopen to anything but `ready` is refused; a reopen of a non-terminal task is refused |
| the same file — `a_terminal_task_is_immutable` | extended: a resume receipt alone still cannot reopen a completed task |
| the same file — `task_transitions_follow_the_declared_table` | the table's only terminal exit is `done -> ready`, and only `done` is reopenable |
| `crates/kontor-daemon/tests/loopback_api.rs` — `a_completed_task_reopens_without_rewriting_what_it_recorded` | a genuinely completed task (team settled, every gate discharged, artifacts cited) is reopened over `/v1`; the revision advances; every gate verdict and every team run is byte-identical afterwards; the runtime sees nothing; a plain `resume` from `done` is still `409` naming terminality; a second reopen is refused as `task reopen`; and the reopened task completes again on fresh evidence |

**Mutation checks.** Making every terminal state reopenable: caught by the domain
oracle. Granting a plain `resume` the reopen authority: caught by the loopback
regression.

## One test renamed, nothing weakened

`a_teamless_task_completes_reopens_and_lets_its_epic_close_and_reopen` never
completed or reopened anything — it held a task, resumed it, and checked that the
epic would not close. It is now
`a_teamless_task_is_held_and_resumed_and_its_epic_stays_open`, which is what it
asserts. The completion and reopen it claimed are covered by the new regression
above.

The settle-every-seat and discharge-the-profile sequences are now helpers, so the
reopen regression drives a real completion through the same public routes the
settlement test does rather than a second copy of them.

## Out of scope, and observed

A lifecycle replay under the *original* revision answers `409`: the caller-facing
revision check runs before the idempotency lookup, so once a transition has
advanced the task, the same key with the old revision is refused. That is
pre-existing for `block`, `resume` and `complete_task` alike and is not something
this change introduced or fixed — the regression asserts the bounded rule instead
of claiming replay-idempotency that is not there.
