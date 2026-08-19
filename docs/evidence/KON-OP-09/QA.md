# KON-OP-09 QA gate evidence

Date: 2026-08-18
Task: `KON-OP-09` / Jira `ASMA-7878`
QA gate verdict: **passed** at `ffeffc3` (round 2)

| Round | Head | Verdict |
| --- | --- | --- |
| 1 | `47948b6` / `7cf08a4` | rejected — two acceptance proofs missing |
| 2 | `ffeffc3` / `4f3242b` | **passed** |
| 3 | `a247587` / PR 44 current-master integration | **passed** |

## Round 3 — current-master integration passed

QA independently verified the current-master integration production source at
`a24758714244762170da17e7604718086aac4a8b`, after code-review receipt
`01a019bd-f9b8-71e0-9542-080700d325e9` (sequence `4`). The integration keeps
the console as a generated-contract client and correctly accepts the merged
consultation and completion DTO shapes.

| Check | Result |
| --- | --- |
| Generated API drift | `pnpm verify:api` — pass |
| Console type check | `pnpm typecheck` — pass |
| Console component/contract tests | 16 files, **295 passed** |
| Browser QA | `pnpm test:e2e` — 4 passed: Project Operations and Teams, desktop and phone |
| Rust formatting and lint | `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` — pass |
| Workspace verification | `cargo test --workspace --quiet` — pass, including the 174-test loopback API suite |

The focused consultation invariant is now behavior-pinned: an Advisor result
without a receipt renders its typed run id but no `Confirmed receipt` status.
The deliberate mutant that rendered `Confirmed receipt pending` for an absent
receipt made that test fail, then was reverted. This test-only QA addition does
not alter the reviewed production source at `a247587`.

The inspector's disclosed retired-seat, stale-read, same-typed-parameter and
incomplete-completion-panel observations remain non-blocking follow-ups; this
QA run neither reopens accepted findings nor performs a release action.

## Round 2 — passed

The `ffeffc3` remediation closes both findings from round 1, and inspector
round 3 (`4f3242b`) independently reviewed the changes and their focused
mutations. This QA rerun independently verified the accepted head:

| Check | Result |
| --- | --- |
| Generated API drift | pass |
| Console type check | pass |
| Console component/contract tests | 16 files, **290 passed** |
| Browser QA | 4 passed: Project Operations and Delivery Teams at desktop and phone widths |
| Rust format and clippy | pass |
| Workspace verification | pass, including `tests/e2e/pilot.rs` and `pilot_live.rs` |

The corrected UI now holds one idempotency key for an unchanged request intent,
releasing it only after a confirmed receipt. It also renders the Core Team and
Completion server projections independently from their catalog siblings, so a
catalog refusal no longer erases valid evidence. The new behavior-level tests
cover replay/key rotation and the three independent-panel cases.

The prior rejected round remains below as the historical record that prompted
this remediation. The remaining `release()`-coverage gap and the unavailable
materialize/settle controls are non-blocking observations, as recorded in the
independent round-3 review; they do not invalidate the OP-09 acceptance proof.

## Scope and immutable evidence

The QA checkpoint reviewed the Operational diagnostic UI at the accepted
code-review chain `47948b6` (remediation) and `7cf08a4` (round-2 review).
No production state, topology, Jira record, or other task scope was changed.

Artifact keys:

| Key | Artifact |
| --- | --- |
| `qa.architecture` | `docs/evidence/KON-OP-09/ARCHITECTURE.md` |
| `qa.code-review.round-2` | `docs/evidence/KON-OP-09/REVIEW.md` at `7cf08a4` |
| `qa.console.desktop-phone` | `evidence/ASMA-7878-PROJECT-{DESKTOP,PHONE}.png` |
| `qa.gate.record` | this file |

## Successful checks

| Check | Result |
| --- | --- |
| Generated API drift | `pnpm --dir apps/console verify:api` — pass |
| Console type check | `pnpm --dir apps/console typecheck` — pass |
| Console component/contract tests | `pnpm --dir apps/console test` — 16 files, 285 tests passed |
| Browser QA | `pnpm --dir apps/console test:e2e` — 4 passed: Project Operations and Delivery Teams at desktop and phone widths |
| Rust formatting | `cargo fmt --all -- --check` — pass |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` — pass |
| Workspace verification | `cargo test --workspace` — pass, including `tests/e2e/pilot.rs` and `pilot_live.rs` |

The browser check needed the local loopback listener normally used by Vite; it
was rerun under the approved local-only test permission and passed. The
workspace test run likewise needed loopback for its HTTP/CLI parity fixture and
passed under that same bounded condition.

The source/test evidence confirms the accepted UI boundaries: a generated
`/v1` client, no console source naming a direct runtime, server-backed code
help with keyboard/click disclosure and an unknown-code state, and responsive
desktop/phone Project Operations coverage.

## Blocking acceptance failures

The build and existing tests are green, but two required proofs in
`qa.architecture` are not met at the reviewed head. They are implementation
defects, not test-environment failures.

1. **Idempotency replay is not preserved.** The architecture requires a retry
   of one uncertain intent to reuse its original idempotency key. The Core Team
   apply, Quick Session ensure, promotion apply, consultation invoke, and
   completion commands mint `crypto.randomUUID()` at activation in
   `apps/console/src/views/ProjectView.tsx`. After `act()` clears `busy` on an
   uncertain response, retrying the unchanged intent produces a new key and
   can therefore create a second durable command. This violates the required
   replay proof and mutation rule 2.

2. **A failed sibling projection can hide valid evidence.** `ProjectView.tsx`
   renders the Core Team panel only if both `coreTeam.value` and `roles.value`
   exist, and renders Completion only if both `completion.value` and
   `completionProfiles.value` exist. If either catalog request fails, the
   successful server projection is replaced by an unavailable banner. This
   violates the independent-projection-loading requirement and its required
   proof that a failed panel does not erase a successful sibling projection.

`REVIEW.md` records both as deliberately deferred to OP-10 and non-blocking
for the **code-review** gate. They are nevertheless explicit OP-09 acceptance
requirements, so this separate QA gate cannot pass them as complete.

## Required disposition

Return `KON-OP-09` to implementation for the two defects above. Re-run this QA
gate after adding behavior-level coverage for same-intent idempotency-key replay
and for preserving a successful Core Team/Completion projection when its
catalog sibling is unavailable.
