# KON-OP-07 follow-up 1 — the accepted proof gap, closed

Date: 2026-08-19
Against: `2026-08-19-10-38-report-kontor-op07-release-notes.md`, "Deliberate
Limitations and Follow-Up", third bullet
Released slice: `d3ecbec` (unchanged by this follow-up's runtime behaviour)

## What the release accepted as non-blocking

> The lifecycle-transition backlog guard is present, but no regression currently
> calls that seam under withheld authority. Add it before a public surface can
> create a pending backlog. The same future change must audit all other
> graph/intake/policy write seams.

The first sentence is now closed. The audit is recorded below as a checklist for
the change that gives `backlog_origin: legacy_pending` a public exit, rather than
left as a sentence someone has to rediscover.

## The regression

`a_task_under_a_legacy_owned_backlog_refuses_its_lifecycle_transition`
(`crates/kontor-store/tests/repository_roundtrip.rs`) creates a project whose
backlog is `legacy_pending`, gives it a goal and a task through the repository
primitives, and asserts `transition_task` refuses with
`RepositoryError::AuthorityWithheld { subject: "backlog" }` and that the task's
state and revision are untouched.

The task is built with `create_mini_project`/`create_task` rather than
`apply_epic` for two reasons: `apply_epic` now refuses such a project outright, so
it cannot produce the fixture; and those primitives are the layer *below* the
policy that refuses it — the same place `create_project` records native origins.

Mutation-checked: removing `require_backlog_authority` from `transition_task`
turns it red. No other test in the suite notices that removal, which is exactly
what the release note observed.

## Write-seam audit for the backlog-import change

Guarded today (both call `require_backlog_authority` inside their own
transaction):

| Seam | Where |
| --- | --- |
| `apply_epic` — the mini-project/task/dependency graph | `graph.rs` |
| `transition_task` — task lifecycle | `repository.rs` |

Unguarded and **in scope for the change that lets a public surface create a
pending backlog**. None is reachable with a withheld backlog today, because
`projects:ensure` refuses that origin and the seeded rows are all native — so
these are not live defects, they are the checklist that stops becoming true:

| Seam | Where | Why it belongs to the backlog subject |
| --- | --- | --- |
| `create_mini_project`, `create_task`, `set_task_dependencies` | `repository.rs` | The graph itself, one row at a time. Deliberately below the policy layer, which is why the regression above can use them; a public surface that reaches them needs its own guard. |
| `replace_task_workflow`, `set_task_account_selection`, `set_task_worktree` | `graph.rs` | Task-scoped state a caller selects. Arguably placement rather than backlog; decide explicitly rather than by omission. |
| `park_task`, `apply_recovery_transition`, `write_transition` | `policy.rs` | Move task lifecycle through the recovery path, so they reach the same state `transition_task` guards. |
| intake acceptance creating tasks | `intake.rs` (`insert_lineage` and its callers) | An accepted proposal creates backlog rows. |
| `insert_team_run`, `insert_agent_run`, admission | `scheduler.rs` | Runs are not backlog facts, but admission reads task state and writes run rows against it. Most likely correct to leave unguarded — record the decision. |

Recommendation for that change: rather than adding a call at each site, decide
whether the guard belongs at the one transaction boundary they all pass through,
and make the unguarded set empty by construction instead of by review.

## Gates

`cargo fmt --all -- --check` pass; `cargo clippy --workspace --all-targets -- -D
warnings` pass; `cargo test --workspace --no-fail-fast` pass — 110 binaries, 1402
tests. This follow-up adds one test and changes no runtime source, so the console
contract artefacts are untouched.

`cargo deny check` **failed on advisories at the released lock**, and not because
of this change. A `Cargo.lock` change then appeared in the worktree during the
gate run — not authored by this seat, and left uncommitted — which moves `h2` to
the patched version and makes advisories pass. Both states are recorded because
the tree a reader finds may be either:

- `RUSTSEC-2026-0258` — `h2 v0.4.15` accepts and queues empty DATA frames without
  limit (low severity; patched in 0.4.16). It reaches this workspace transitively
  as `axum 0.8.9 → hyper 1.11.0 → h2`.
- It is newly published: the same command passed at `d3ecbec` on 2026-08-18, and
  nothing since has touched `Cargo.lock` or `Cargo.toml` — the last commits to
  either belong to other tickets. It therefore fails identically at the released
  commit and is not attributable to the released slice.
- The in-worktree lock change bumps `h2` 0.4.15 → 0.4.16 and also *downgrades*
  three `windows-sys` selections (0.61.2 → 0.59.0/0.52.0). With it,
  `cargo deny check` reports `advisories ok`. No cargo process was running when it
  was noticed, and no seat claimed it; the likeliest author is a `cargo metadata`
  re-resolution triggered by `cargo deny` itself. It is left in place rather than
  reverted, because discarding an unattributed change is worse than reporting it,
  and it is **not committed** — a lock bump is not this ticket's to land.
- The 1402-test run above executed against the *released* lock, before that change
  appeared. The bumped lock is therefore unverified by this seat.
- It is **not fixed here on purpose.** `Cargo.toml` states that the exact pins are
  authoritative and that the dependency list is owned by KON-MVP-02, with later
  tickets changing it only through re-planning. A workspace-wide `cargo update -p
  h2` inside an OP-07 follow-up would smuggle a dependency change into an
  unrelated ticket, and it blocks CI for every ticket rather than only this one.
  It needs its own decision by that owner.
