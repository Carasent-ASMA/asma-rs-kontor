Artifact: `high-scope-record`

# ASMA-8195 high-scope record

Date: 2026-09-17
Task: Jira `ASMA-8195` / Kontor `01a0abdd-b855-7862-89e4-19013c3b250e`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-scope`
TeamRun: `01a0b046-b5b4-7900-9455-53436bafc39c`

## Outcome

Project an unlifted execution hold as `blocked` on the task read surface while
retaining the hold's exact authorization, lift condition, owner and reason.
`scheduler-plan` and `task-get` must derive those terms through the same store
resolver; a held task must not read `ready`, and a lifted hold must not leave a
stale blocked projection.

This record is the implementation contract and handoff, not implementation or
verification evidence. Its authorities are Jira ASMA-8195, REQ-004 and TEST-006
in
`asma-modules/_docs/ai-orchestration/plans/2026-09-16-22-09-plan-kontor-autonomous-epic-delivery.md`,
and the Kontor task above.

## Frozen baseline and reproduced gap

The canonical module worktree is
`asma-modules/.worktrees/feat/ASMA-8190-hold-visibility/_tools/asma-rs-kontor`
on branch `feat/ASMA-8195-hold-visibility`. Scope inspected clean HEAD
`2faeb47775370e0b85ecd3e8685460b07464db8a`; observed `origin/master` was
`e19ccd3df68ecb75dd9b4f99a432602f4394bb9f`, seven commits ahead and three
commits behind this branch. Implementation must integrate that upstream state
before amending source and must preserve this worktree and TeamRun.

Commit `a6eac3e176fefea8a0802636b1a8689d71f66d71` already contains a provisional
ASMA-8195 implementation:

- `SqliteStore::held_work` is the shared resolver for both public surfaces;
- `BlockedTaskDto.hold` exposes the hold on `scheduler-plan` rejections;
- `TaskDto.hold` exposes the same hold on `task-get`;
- loopback coverage checks the shared terms, lift disappearance and newest
  revoked cover.

The acceptance gap is in `crates/kontor-api/src/control.rs::task_dto`:

```rust
state: inspection.task.state,
```

The task aggregate remains `ready` under an unlifted hold, so `task-get`
currently returns `state: "ready"` beside a non-null `hold`. The existing
loopback test compares the two `hold` objects but never asserts the task state.
That does not satisfy the explicit requirement that held work report as
`blocked` or `needs_human`, nor TEST-006's same-state contract.

## Decision record

### DEC-8195-01 — project the hold; do not mutate task lifecycle

The durable task aggregate remains unchanged. `TaskState::Blocked` requires a
receipt-backed resume, while ASMA-8194 makes a conditional hold lift
automatically. Persisting a lifecycle transition would add a second release
mechanism and could leave work blocked after its hold has lifted.

`task-get` therefore projects `TaskState::Blocked` only when both facts are
true:

1. the durable task state is `ready`; and
2. `SqliteStore::held_work` returns a hold at the read time.

Every other durable state is preserved. In particular, the projection must not
turn in-progress or terminal work into blocked work merely because historical
revocation evidence still exists.

### DEC-8195-02 — use `blocked`, not inferred `needs_human`

The stored owner is an `AccountProfileId`; neither that type nor a `manual`
lift condition proves a human owns the decision. Inferring `needs_human` would
violate capability honesty. REQ-004 explicitly permits `blocked`, so this task
uses that truthful projection and continues to expose the exact owner for
operator routing.

### DEC-8195-03 — retain one hold resolver

Do not add another authorization query or a second projection helper. Keep
`SqliteStore::held_work` as the one resolver used by both surfaces. The
smallest remaining source change is the state selection in `task_dto` plus the
existing loopback assertions that pin it.

No new ADR is required. The governing plan already settles the product
behavior, and this decision preserves the existing authority boundary: task
lifecycle remains Kontor state; held status remains a derived read projection
over execution authorization evidence.

## Required implementation

1. Integrate the observed upstream head through the supported ASMA Git
   workflow before source edits; reconcile conflicts without dropping the
   provisional shared resolver or its tests.
2. In `task_dto`, return `blocked` when a `ready` task has a resolved hold;
   otherwise return the durable task state. Add no new type, helper or store
   query.
3. Keep the planner contract structural: the held task is present in
   `SchedulerPlanDto.blocked`, absent from `ready`, and carries the resolved
   `hold`. No redundant planner state field is required.
4. Strengthen
   `held_work_names_its_lift_condition_and_owner_on_both_surfaces` to assert:
   - the planner places the task in `blocked` and not `ready`;
   - `task-get.value.state == "blocked"` while the hold is unlifted;
   - both surfaces expose the same lift condition, owner, reason and
     authorization id;
   - after ASMA-8194 lifts the hold, `task-get.value.state == "ready"` and the
     `hold` field is absent/null.
5. Preserve the re-hold test: the newest revoked covering authorization owns
   the projected terms. Keep live-cover suppression and per-task scope intact.
6. Record the implementation and mutation receipts in the required
   `high-change` artifact. Do not claim the pre-existing commit message as a
   durable mutation receipt.

## Verification and mutation contract

Run the narrowest focused loopback checks first, then the affected crate checks
required by the final diff. At minimum:

```text
cargo test -p kontor-daemon --test loopback_api held_work_names_its_lift_condition_and_owner_on_both_surfaces -- --exact
cargo test -p kontor-daemon --test loopback_api a_reheld_epic_is_held_on_its_newest_terms_not_its_first -- --exact
cargo test -p kontor-api
cargo check -p kontor-store -p kontor-api -p kontor-daemon
```

Run one mutant at a time and restore the production tree after each:

| Mutant | Expected killer |
| --- | --- |
| Replace the projected task state with `inspection.task.state` | the held-state assertion must fail because `task-get` reads `ready` |
| Project `blocked` whenever a revocation exists, ignoring the live-cover result | the post-lift assertion must fail because `task-get` stays blocked |
| Select the oldest revoked cover instead of the newest | the re-held-epic assertion must fail on reason/lift condition |
| Drop the planner or task-snapshot `hold` assignment | the cross-surface equality assertion must fail |

PAT-001 is satisfied only by recorded red/green receipts. A compile failure is
not a killed mutant, and no mutant may remain in the branch.

## Open-question ledger

No unresolved ambiguity remains. The accepted `blocked` alternative, derived
projection boundary, source resolver and verification behavior are all
evidenced by the Jira task, governing plan and inspected code. No unevidenced
assumption was used to proceed.

## Scope handoff

The implementation seat should amend the provisional change rather than
replace it. The root-cause fix is one projected-state branch in the existing
`task_dto` seam, with the current end-to-end test strengthened to fail if held
work ever reads `ready` again. No schema, migration, dependency, new API route,
durable lifecycle transition, runtime mutation or live-realm write belongs in
ASMA-8195.
