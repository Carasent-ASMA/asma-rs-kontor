# ASMA-8195 high-change record: held work reports as blocked

Date: 2026-09-17
Artifact: `high-change`
Task: Jira `ASMA-8195` / Kontor `01a0abdd-b855-7862-89e4-19013c3b250e`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Phase: `high-implementation`
TeamRun: `01a0b046-b5b4-7900-9455-53436bafc39c`
Seat: `implement`

Implements the contract in [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md),
frozen by the scope seat at `723f3385`. This record is implementation and
mutation evidence; independent verification is the next phase's artifact.

## Baseline

Scope inspected clean `2faeb477` and observed `origin/master` at
`e19ccd3d`. That upstream head was unchanged at the time of this turn, so the
integration step acted on exactly the state scope named. The worktree, branch
and TeamRun are preserved:
`asma-modules/.worktrees/feat/ASMA-8190-hold-visibility/_tools/asma-rs-kontor`
on `feat/ASMA-8195-hold-visibility`.

## 1 — Upstream integration (required step 1)

`origin/master` (`e19ccd3d`, seven commits) merged into the branch. Three
conflicts, all reconciled without dropping the provisional shared resolver or
its tests.

### `crates/kontor-store/src/migrations.rs` — a migration-number collision

Both sides added a migration numbered `0099`. Master's
`0099_turn_correlation_challenges.sql` is released and keeps its number; this
branch's `0099_hold_lift_conditions.sql` was renumbered to
`0100_hold_lift_conditions.sql` and `SCHEMA_VERSION` moved to `100`.

The migration runner dispatches on `PRAGMA user_version`, and each script ends
with its own. The renamed script still recorded `PRAGMA user_version = 99`, so
applying it after master's `0099` left the version unmoved and every realm
failed to open with `Pragma { pragma: "user_version" }`. The pragma now records
`100`. A sweep confirms all 100 migrations record the version their filename
claims.

This is a renumbering of a migration the branch already owned. No new schema
object was introduced by this task.

### `crates/kontor-daemon/src/applications.rs` — two additive methods, one shared match arm

The first conflict was purely additive on both sides — this branch's
`hold_lift_satisfied` and self-lift routine against master's
`epic_control_plane`. Both kept.

The second was a genuine semantic conflict in the `CandidateDecision::Reject`
arm. This branch resolved a per-task hold and passed it to `blocked_task`;
master routed the same arm through `unconfirmed.get(task_id)` to
`unconfirmed_admission_block`. Taking either alone loses the other, and master's
constructor no longer compiled against the `hold` field that `BlockedTaskDto`
had gained.

Combined so the hold is resolved once and handed to **both** paths.
`unconfirmed_admission_block` now takes the resolved hold rather than omitting
it. This follows the rationale the branch had already written into that arm —
that a task held for a second reason as well is still held, and hiding the hold
whenever another reason exists sends an operator chasing the wrong one. An
unconfirmed admission is exactly such a second reason. No second query and no
second resolver were added.

### `crates/kontor-daemon/tests/loopback_api.rs` — interleaved test additions

Both sides appended new tests whose opening boilerplate is identical, so the
textual merge interleaved two unrelated test bodies. This branch's change to the
file is a single pure insertion of 441 lines against the merge base, with no
edits elsewhere, so the file was reconstructed rather than hand-spliced: master's
blob taken whole, this branch's insertion replayed at its original anchor. All
five of this branch's functions and all thirty-one of master's are present.

## 2 — The root-cause fix (required step 2)

`crates/kontor-api/src/control.rs::task_dto`:

```rust
let state = if hold.is_some() && inspection.task.state == TaskState::Ready {
    TaskState::Blocked
} else {
    inspection.task.state
};
```

Exactly the projection DEC-8195-01 specifies, at the one seam it names. `Ready`
plus a resolved hold reads `blocked`; every other durable state is returned
unchanged, so in-progress and terminal work are never rewritten by historical
revocation evidence. The durable aggregate is untouched, which is what lets
ASMA-8194 lift a conditional hold without a receipt-backed resume and without
stranding work behind a projection that outlived its cause.

Per DEC-8195-03 this adds no type, no helper and no store query.
`SqliteStore::held_work` remains the single resolver behind both surfaces.

## 3 — Planner contract (required step 3)

Left structural, as specified. No redundant planner state field was added; the
held task is present in `SchedulerPlanDto.blocked`, absent from `ready`, and
carries the resolved hold — now asserted rather than assumed (§4).

## 4 — Test strengthening (required step 4)

`held_work_names_its_lift_condition_and_owner_on_both_surfaces` previously
compared the two `hold` objects and never asserted task state, which is why the
acceptance gap survived it. Four assertions added:

1. the planner places the task in `blocked` **and not in `ready`** — blocked
   alone is half the contract, since work that is both is still offered to an
   admitting scheduler;
2. `task-get.value.state == "blocked"` while the hold is unlifted;
3. the hold names its `authorization_id`, beside the existing whole-object
   equality that pins lift condition, owner and reason across both surfaces;
4. after the hold lifts, `task-get.value.state == "ready"` and `hold` is null —
   the projection is not one-way.

`a_reheld_epic_is_held_on_its_newest_terms_not_its_first` was preserved
unchanged (required step 5), including live-cover suppression and per-task
scope.

## 5 — Two latent defects the integration surfaced

Both belong to the provisional implementation and both fail only in the full
archive gate, so they are recorded rather than left for it:

- `crates/kontor-store/tests/schema_v1.rs` still pinned
  `assert_eq!(SCHEMA_VERSION, 98)` — the branch had added a migration without
  moving the pin. Now `100`.
- The same file's frozen expected table-set omitted `execution_hold_conditions`,
  the table the branch's own migration creates. Now listed.

The regenerated OpenAPI contract and console types are the other two pins:
`KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract`
followed by `pnpm --filter kontor-console generate:api`. The merge had dropped
`HeldWorkDto`, the `hold` members on `TaskDto` and `BlockedTaskDto`, and
`lift_condition` on `initial_hold` from the committed document; all are restored
and the contract test is green.

## 6 — Verification receipts

All run on the final tree, after `cargo fmt --all`.

| Check | Result |
| --- | --- |
| `cargo test -p kontor-daemon --test loopback_api held_work_names_its_lift_condition_and_owner_on_both_surfaces -- --exact` | ok — 1 passed |
| `cargo test -p kontor-daemon --test loopback_api a_reheld_epic_is_held_on_its_newest_terms_not_its_first -- --exact` | ok — 1 passed |
| `cargo test -p kontor-daemon --test loopback_api a_hold_lifts_itself_only_when_its_recorded_condition_comes_true -- --exact` | ok — 1 passed |
| `cargo test -p kontor-api` | ok — 24 + 5 + 3 + 0 passed, 0 failed |
| `cargo test -p kontor-store` | ok — 531 passed across 32 targets, 0 failed |
| `cargo check -p kontor-store -p kontor-api -p kontor-daemon` | clean |
| `cargo clippy -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

The full `scripts/verify-tree.py --mode archive` gate has **not** been run in
this phase; it is the verification seat's to run.

## 7 — Mutation receipts (PAT-001)

One mutant at a time, production tree restored and verified between each. Every
mutant compiled, so none was killed by the type checker. `git status --porcelain`
after the pass is byte-identical to the pre-pass baseline, and the three target
tests were re-run green afterwards.

| # | Mutant | Result | Killed by |
| --- | --- | --- | --- |
| 1 | `task_dto` returns `inspection.task.state` instead of the projection | **KILLED** | `held_work_…both_surfaces`, `held work reports as blocked, not ready` — observed payload `"state":"ready"` beside a non-null `hold`, the original defect exactly |
| 2 | `held_work` ignores the live-cover result, so any revocation projects blocked | **KILLED** | `held_work_…both_surfaces`, `the hold lifted, so nothing is holding this task` — observed `"state":"blocked"` persisting after the lift |
| 3 | `held_work` selects the oldest revoked cover (`max_by_key` → `min_by_key`) | **KILLED** | `a_reheld_epic_is_held_on_its_newest_terms_not_its_first` — left `"Kickoff hold until the graph is bound"`, right `"Re-held pending QA sign-off"` |
| 4 | `task_dto` drops the task-snapshot `hold` assignment (`hold: None`) | **KILLED** | `held_work_…both_surfaces`, `scheduler-plan and task-get must agree about the same hold` |

No mutant survived. No mutant remains in the branch.

## 8 — Scope compliance

Within the scope record's boundary: the provisional change was amended, not
replaced; the fix is the one projected-state branch at the existing `task_dto`
seam; no new API route, no durable lifecycle transition, no runtime mutation and
no live-realm write. The only schema movement is the renumbering of a migration
this branch already owned, forced by an upstream collision.

## 9 — Open-question ledger

No unresolved ambiguity. Two decisions were taken inside the scope record's
stated boundary rather than escalated, both recorded above with their reasoning:
routing the resolved hold through `unconfirmed_admission_block` (§1), and
renumbering the branch's own migration to `0100` (§1). Both follow directly from
rationale the scope record or the existing code had already fixed; neither
selects a behavior the record left open.
