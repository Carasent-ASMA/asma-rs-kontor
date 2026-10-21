# Workflow templates

These templates are the generic wording the workflow pack renders for a
project. Copy one, replace every `{{ ... }}` placeholder, and keep the result in
the project that owns it.

## Placeholder grammar

- `{{ project.<key> }}` is filled from the `project:` block in the selected
  config folder's `orchestration/orchestration.yml`.
- `{{ document.<key> }}`, `{{ epic.<key> }}` and `{{ task.<key> }}` are filled by
  the author of that document or tracker item.
- A value that is not a placeholder is neutral wording. A project-specific
  value belongs in a fenced example block marked with
  `<!-- example: project-specific -->`; nothing outside such a block may name a
  real project, account, person, host or tracker key.

An unfilled placeholder is an error. The renderer (`scripts/pack.py`, WP-05)
refuses it rather than shipping `{{ ... }}` into an instruction file.

## Files

| Template | Renders |
| --- | --- |
| [`AGENTS.md`](AGENTS.md) | The project entry file: Workflow part and Project part. |
| [`document-header.md`](document-header.md) | Front matter and the blockquote header for a plan, analysis or report. |
| [`index-router.md`](index-router.md) | The documentation index/router table. |
| [`plan.md`](plan.md) | A plan with requirements, steps, the `plan-graph` block and the `hold:` block. |
| [`epic.md`](epic.md) | An epic's tracker-neutral content contract. |
| [`task.md`](task.md) | A task's tracker-neutral content contract. |

## Rules

- One truth per rule: link the owning document instead of copying its text.
- A plan with `status: On Hold` carries a `hold:` block with a reason from the
  closed list, an owner, a checkable lift condition and a review date at most 30
  days ahead.
- An epic description contains Summary / purpose, Goal, Scope and sub-goals,
  Done means, and Detailed plan, in that order.
- A task description contains Summary / purpose, Goal, Scope and sub-goals,
  Delivery context, Verification and evidence, Detailed plan, and Done means, in
  that order.
- The testable closure sentence comes verbatim from the authoritative plan.
- No template links into a private repository.
