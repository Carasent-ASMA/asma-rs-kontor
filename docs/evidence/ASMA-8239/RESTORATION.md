# ASMA-8239 / PUB-09 — shared-worktree containment restoration

Branch `feat/ASMA-8239-pub-09-admin-container-recreation-and-operation-specific-applicability`,
worktree `/Users/igor/carasent/asma-modules/.worktrees/asma-8239/asma-rs-kontor`,
head `267c67a7` (unchanged; no commit, no reset, no cleanup performed).

Restoration was performed from this seat's exact prior Write/Edit contents.
No other agent's work was overwritten, and no file outside the list below was
touched.

## What was reverted by the scope native, and restored here

The scope native `eed100a9` acknowledged (Paseo, ~22:33Z, after root
preservation key `root-watchdog-8239-preserve-swe-dirty-work-20260919T2232Z-v1`)
that it attributed this seat's edits to itself and reverted three paths.

| # | Path | Loss | Restored |
|---|---|---|---|
| 1 | `crates/kontor-core/src/container_recreation.rs` | file deleted (untracked) | yes — final tested content, including the two post-Write corrections (non-`const` `decide`, `contains`-based `is_native_child`) |
| 2 | `crates/kontor-core/src/lib.rs` | `pub mod container_recreation;` export removed | yes — single line re-added after `consultation` |
| 3 | `crates/kontor-core/src/receipt.rs` | `CommandKind::RecreateTopologyContainer` variant and its aggregate-witness arm removed | yes — both re-added verbatim |

## Fourth lost path, not in the acknowledgment

| # | Path | Loss | Restored |
|---|---|---|---|
| 4 | `docs/evidence/ASMA-8239/OPEN-QUESTIONS.md` | file deleted; the `docs/evidence/ASMA-8239/` directory was left present but empty | yes — exact prior content, 64 lines, both entries (OQ-8239-01, OQ-8239-02) |

This fourth deletion was **not** named in the scope native's acknowledgment,
which reported three paths. Reported here rather than dropped silently.

## Nothing unrecoverable

All four paths were restorable byte-for-byte in behaviour from this seat's own
transcript. No content is lost and nothing needs reconstruction from memory.

## Changes that survived and were deliberately left untouched

- `crates/kontor-runtime/src/container.rs` — `ContainerRecreationRequest`,
  `ContainerRecreationOutcome`, `ensure_applicable`.
- `crates/kontor-runtime/src/adapter.rs` — `preview_container_recreation`,
  `recreate_container` trait methods with refusing defaults.
- `crates/kontor-runtime-paseo/src/adapter.rs` — `RecreationCensus`, the census,
  creation, adoption and readback helpers.

## Verification rerun after restoration

- `cargo test -p kontor-core --lib container_recreation` — **9 passed, 0 failed**,
  the same nine case names as before the revert.
- `cargo check -p kontor-runtime -p kontor-runtime-paseo` — clean; the
  `unresolved import kontor_core::container_recreation` caused by deletion #1
  is gone.

## Pre-existing failure on this branch, owned by ASMA-8120 — not caused here

`cargo test -p kontor-core --test domain_state` fails at HEAD, independently of
any ASMA-8239 work:

```
every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state
  panicked: correct_task_worktree must not be able to target a task
```

Proof that it predates this branch's work, taken against `267c67a7` itself:

- `git show HEAD:crates/kontor-core/tests/domain_state.rs | grep -c correct_task_worktree` → `0`
- `git show HEAD:crates/kontor-core/src/receipt.rs` grants `CorrectTaskWorktree`
  a `Task` target in the `witness(matches!(target, A::Task))` arm.

A kind that grants a target with no row in `LEGAL_COMMAND_TARGETS` fails that
test by construction. `CorrectTaskWorktree` was introduced by `267c67a7`
("fix(kontor): add exact task worktree claim repair (ASMA-8120) (#241)") without
its table row.

This seat did **not** fix it: it belongs to ASMA-8120, and editing another
ticket's landed work is exactly what the containment incident was about.

### Executed evidence that the ASMA-8239 row is correct

Because the pre-existing panic halts the table walk before
`recreate_topology_container` is reached, a bounded local experiment was run and
then fully reverted: the single missing `correct_task_worktree` row was added
temporarily, and `cargo test -p kontor-core --test domain_state` reported
**37 passed, 0 failed** — which exercises and validates the ASMA-8239 row
`("recreate_topology_container", "project", "witness", None)`.

The experiment line was then removed and `crates/kontor-core/tests/domain_state.rs`
was diffed against its pre-experiment copy: **identical**. Only the ASMA-8239
row remains.

Consequence for the LSA: one line — `("correct_task_worktree", "task", "witness", None)`
— makes the whole `kontor-core` suite green. Whether ASMA-8239 carries that
one-line ASMA-8120 repair, or ASMA-8120 does, is the LSA's call; this seat is
not taking it unilaterally.
