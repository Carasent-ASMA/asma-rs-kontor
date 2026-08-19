# KON-OP-14 / ASMA-7941 — mutation proof

Date: 2026-08-19
Branch: `feat/ASMA-7941-preserve-task-lifecycle-state-during-epic-import` (submodule `_tools/asma-rs-kontor`)
Commits: `b0d58a6` (import lifecycle) and the corrective commit that carries this file.
Requirement: OP-REQ-030 / operational gap GAP-1.

## What this proves

Every behaviour OP-14 claims has one test that fails when the behaviour is
deliberately removed. A test that passes against a broken implementation proves
nothing, so each row below names the mutation, the test that caught it, and the
receipt after restoration.

Two mutants — M7 and M8 — were run in this session, first-hand, and their exact
failure text is recorded. M1–M6 were run by the builder seat that produced
`b0d58a6`; their receipts are carried from that handoff and were **not** re-run
here. The tests those rows name were read and confirmed to assert the stated
behaviour, but the failure text is second-hand and marked as such.

| # | Mutation | Killer test | Receipt |
| --- | --- | --- | --- |
| M1 | `ImportedTaskState::task_state` maps `Completed` to `Ready`, flattening historical completion | `epic_import_preview_apply_and_replay_preserve_historical_task_lifecycle` (`crates/kontor-daemon/tests/loopback_api.rs:2280`) | killed — builder handoff, not re-run here |
| M2 | `preview_epic` commits instead of rolling back | same test's census assertion, "preview must not leave even a partial task graph behind" | killed — builder handoff, not re-run here |
| M3 | the insert drops `imported_state`, persisting only the projection | `a_mixed_import_closes_after_only_its_native_task_earns_completion` (`loopback_api.rs:2639`) | killed — builder handoff, not re-run here |
| M4 | the contradiction check is removed, so a re-import may overwrite a historical fact | `epic_import_defaults_ready_and_refuses_invalid_or_contradictory_state_atomically` (`loopback_api.rs:2409`) | killed — builder handoff, not re-run here |
| M5 | `native_done` reverts to `task.state == Done`, so imported terminality forges closure evidence | `a_configured_jira_boundary_distinguishes_historical_from_native_completion` (`loopback_api.rs:4671`) | killed — builder handoff, not re-run here |
| M6 | the export row drops the provenance column | `imported_task_lifecycle_provenance_survives_export_serialization_and_parse` (`crates/kontor-store/tests/backup_export.rs:293`) | killed — builder handoff, not re-run here |
| M7 | `ensure_task` compares the task's **current state** against the requested import state | `an_identical_manifest_reapplies_over_a_task_that_natively_progressed` (`loopback_api.rs:2515`) | **killed, verified here** — see below |
| M8 | `park_task` moves the task without clearing `imported_state` | `parking_an_imported_task_clears_its_historical_lifecycle_provenance` (`crates/kontor-store/tests/policy_evidence.rs:540`) | **killed, verified here** — see below |

## M7 — the current-state comparison (the rejected check)

The mutation is the check as first written: `if task.state != plan.imported_state.task_state()`.
It reads as a drift guard and is not one. Import provenance constrains the
historical declaration; the first native transition clears the provenance *and*
moves the state, so comparing the current state makes an identical manifest
un-replayable the moment any task starts.

Red, with the check present:

```
thread 'an_identical_manifest_reapplies_over_a_task_that_natively_progressed' panicked at
crates/kontor-daemon/tests/loopback_api.rs:2609:
assertion `left == right` failed:
  {"code":"revision_conflict","rule":"a persistence rule refused the write against the presented state", ...}
  left: 409
 right: 200
test result: FAILED. 0 passed; 1 failed; 177 filtered out
```

Green, with the check removed and only the provenance comparisons retained:

```
test an_identical_manifest_reapplies_over_a_task_that_natively_progressed ... ok
test result: ok. 1 passed; 0 failed; 177 filtered out
```

What the fix deliberately keeps, because each is about the declaration rather
than the progress:

* imported `ready` re-declared as `completed` → conflict;
* imported `completed` re-declared as `ready` → conflict;
* a provenance-free task (pre-v42, or natively transitioned since) re-declared
  as historically `completed` → conflict;
* a provenance-free task re-declared as the omitted/`ready` default → unchanged,
  and its current state is preserved.

## M8 — the guardrail park

A guardrail park is a native lifecycle transition, so it owns the state exactly
as `transition_task` does. Leaving `imported_state` behind lets a parked task go
on claiming an imported lifecycle it no longer has.

Red, with the writer unchanged:

```
thread 'parking_an_imported_task_clears_its_historical_lifecycle_provenance' panicked at
crates/kontor-store/tests/policy_evidence.rs:586:
assertion `left == right` failed: a native park owns the lifecycle and drops the imported fact
  left: Some(Ready)
 right: None
test result: FAILED. 0 passed; 1 failed; 21 filtered out
```

Green, after adding `imported_state = NULL` to the park update:

```
test result: ok. 22 passed; 0 failed; 0 ignored
```

`policy.rs` and `repository.rs` are the only two task-state *updaters*; the two
inserters (`graph.rs`, `intake.rs`) are column-explicit and correctly leave the
column NULL for a natively created task.

## Restoration

No mutant remains in the worktree. Both mutated files were restored from their
pre-mutation copies and verified by absence of the mutated expression before any
gate was run:

* `crates/kontor-store/src/graph.rs` — `task.state != requested_state` absent, both
  provenance checks present (`:1487`, `:1499`);
* `crates/kontor-store/src/policy.rs` — `imported_state = NULL` present in the park
  update (`:1150`).

## Gates behind this artifact

`cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
`cargo test --workspace`; `pnpm --filter kontor-console verify:api`; `pnpm -r typecheck`;
`pnpm -r test`.

The corrected tree was rerun from scratch after restoration on 2026-08-19:

* `cargo fmt --all -- --check` — exit 0;
* `cargo clippy --workspace --all-targets -- -D warnings` — exit 0;
* `cargo test --workspace` — exit 0;
* `pnpm --filter kontor-console verify:api` — exit 0;
* `pnpm -r typecheck` — exit 0;
* `pnpm -r test` — exit 0 (295 console tests).

The first full Rust rerun was invalidated externally when concurrent disk
cleanup removed the next compiled test binary between suites (`ENOENT`). It was
not accepted as evidence; the clean rerun above completed with exit 0.
