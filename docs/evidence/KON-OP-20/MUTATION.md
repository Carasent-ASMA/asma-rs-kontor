# KON-OP-20 — permission-posture mutation proof

Date: 2026-08-30
Task: `01a02a7f-8e47-7682-be52-1b9f2a632ac4` (permission posture). **Not ASMA-7968.**
Branch: `feat/KON-OP-20-permission-posture-at-spawn`, baseline `origin/master` `e814661`.

## What this proves

The plan required each gate to be seeded with a deliberate defect and shown to
fail. Every mutation below was applied to the corrected source, the named focused
test was run against that broken build, and the mutation was immediately restored
with `git checkout --`. A mutant that produced a *compile* error rather than a
test failure would have been counted as survived; none did. The tree was verified
clean after the pass.

Run in an isolated `CARGO_TARGET_DIR` because another process in this worktree
held the shared build lock — the results are this branch's own.

| # | Deliberate mutation | Killer test | Gate it defends |
| --- | --- | --- | --- |
| M1 | `READABLE_SCHEMAS` drops generation 4 | `a_generation_four_document_is_read_and_grants_nothing` | v4→v5 back-compatible read |
| M2 | Resolution never consults the plane default | `seat_autonomy_resolves_slot_then_plane_then_supervised` | the family default actually reaches a slot |
| M3 | The plane default overrules the role slot | `seat_autonomy_resolves_slot_then_plane_then_supervised` | slot stays authoritative over the plane |
| M4 | The destructive floor renders `ask` instead of `deny` | `posture::` (floor tests) | `deny ≠ ask` — the whole outage fix |
| M5 | `PermissionAllowance::parse` accepts a wildcard | `an_allowance_cannot_be_a_wildcard_or_blank` | an exception cannot be allow-all |
| M5b | …and the same defect at fleet composition | `a_wildcard_task_exception_is_refused_at_composition` | the refusal holds at the config boundary too |
| M6 | `ask` posture also spells `read: ask` | `ask_gains_the_floor_and_no_new_asks` | composition never makes a seat stall *more* |
| M7 | A task exception leaks into the launch mode | `allowances_never_move_the_mode_or_the_feature` | launch/readback agreement under overrides |
| M8 | Cursor's `ask` renders as `agent` | `each_provider_spells_each_posture_natively` | native mode tables, per provider |

**9 of 9 mutants killed.**

## What this does not prove

The tests pin what Kontor *renders and writes*. They do not exercise opencode's
own permission evaluator — in particular that a specific `deny` pattern beats
`"*": "allow"` inside one `bash` map. That belongs to opencode, is the same shape
the machine-local stopgap has been running since 2026-08-22, and is recorded as an
assumption rather than claimed as verified. A live autonomous seat on a clean host
is the evidence that would settle it, and has not been run here.
