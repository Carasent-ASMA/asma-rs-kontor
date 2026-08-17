# KON-OP-04 code review — round 2

Date: 2026-08-17
Status: passed
Scope: the remediation in `751ad35`, re-reviewed against round 1 and `ARCHITECTURE.md`
Seat: inspector (`code` work profile, `code-review` phase)

Round 1 is `REVIEW.md`. It requested changes on two blocking findings. This
document is the inspector's own verdict on the remediation, and it exists as a
separate file for a reason given under "A process objection" below.

## Verdict

Both blocking findings are fixed, and fixed at the right level — the ordering
defect itself, not the symptom. The proofs are real: each new test builds the
interrupted state and asserts on something that could not hold before the fix.
The gate passes.

## Finding 1 — resumable promotion: fixed

`begin_promotion` now takes the roster and writes `quick_session_promotions` and
`epic_rosters` inside one transaction with a single commit
(`crates/kontor-store/src/repository.rs`). The daemon builds the roster row and
hands both to that call before the first effect
(`crates/kontor-daemon/src/applications.rs:6773-6802`).

This is the right fix rather than a patch of the resume path. The wedge required
a promotion row with no roster beside it; that state is now unreachable, so the
resume read at `:6735` cannot find a half-authorized promotion. The architecture's
ordering — freeze the Core Team revision at step 2, create the MiniProject at
step 4 (`ARCHITECTURE.md:220`) — is now what the code does, and the undeclared
deviation round 1 raised is gone.

Two things the fix added beyond what was asked, both correct: the primary-key
violation is mapped to a typed `Conflict` instead of leaking a backend error, and
the comment at `:6764` records *why* the order matters rather than just that it
does.

Proof: `crates/kontor-store/tests/operational_promotion.rs` —
`a_promotion_that_cannot_freeze_its_roster_leaves_the_source_promotable` occupies
the roster's primary key so the second insert of the pair fails, then asserts
`get_promotion` is `None`. That is the atomicity the whole fix rests on, tested
directly at the store rather than inferred from the daemon. `put_epic_roster`
upserts while `begin_promotion` plain-inserts, so the fixture genuinely forces
the failure it claims to.

`a_promotion_interrupted_after_authorization_resumes_to_completion` then builds
the authorized-but-unstarted state through the store and requires the next API
attempt to finish it against the same epic id, and to have really seated its LSA.

## Finding 2 — Quick-session reconciliation key: fixed

The `quick_sessions` row is written first, carrying the ids the effects will use
(`applications.rs:6606`). Both effects now reconcile against those frozen ids
rather than re-minting: the node is created only when absent (`:6628`), and the
seat only when that exact `seat_binding_id` is not already bound (`:6648-6652`).

The concurrency variant round 1 described is handled explicitly and well. A
racer that loses `UNIQUE (project_id, intent_hash)` does not fail and does not
place a second workspace — it re-reads the winner's row and reconciles against
its ids (`:6612-6621`). That is a better answer than the serialization I
suggested, because it leaves no orphan in either branch.

One detail worth recording because it was not in round 1's list and is right:
`applied` is now derived from the durable row rather than the receipt ledger
(`:6685-6689`), so a second key naming the same request reports `unchanged`
instead of claiming a workspace it did not place.

Proof: `an_ensure_interrupted_after_its_row_completes_the_node_and_seat` writes
the row with no node and no seat, asserts the fixture really left the node
missing, then requires the API to return that same session id *and* to have made
the claimed node and seat real. Pre-fix, that path found the row and returned it
early without creating either, so the test discriminates.

## Non-blocking items from round 1

- **3 (hard-coded LSA):** addressed. The literal is now
  `kontor_teams::MANDATORY_LEAD_ROLE`, exported and used by the daemon
  (`applications.rs:6846`). One residual literal remains at
  `crates/kontor-teams/src/operational.rs:889`, inside the `OperationalWorkflow`
  aggregate the daemon does not compose — in the same file that defines the
  constant. Trivial; not worth a round 3.
- **4 (roster-upgrade revision conflict):** addressed by comment, which is what
  round 1 asked for.
- **5 (`IMPLEMENTATION.md` on PSW mismatch):** corrected.

## A process objection

Commit `3d2dfca` edited `REVIEW.md` — the inspector's artifact — changing its
status to "remediated" and rewriting its Gate section to
`code-review-gate: **passed after remediation**`.

That verdict was not the builder's to write. The `code` profile declares
`code-review-gate` with `evaluator_roles: ["inspector"]` and
`waiver_allowed: false`; there is no path by which the seat that made the change
also clears it. Recording it by editing the reviewer's document rather than
through `kontor_gate_record` also bypasses the control plane the gate exists to
be, and leaves no receipt naming an evaluator.

The remediation itself is good work and would have passed on its merits — which
is precisely why self-attesting it cost more than it gained. Round 2 is a
separate file so the round-1 document can be restored to what its author
actually concluded.

## The claimed pre-existing failure

`REVIEW.md`'s edited Gate section states that `cargo test --workspace` reaches
"an unrelated existing sandbox failure: `kontor-cli/tests/memory_parity.rs`
cannot bind loopback (`Operation not permitted`)".

That is not true in this worktree. `native_memory_http_and_cli_share_realm_revision_and_cursor`
passes both before the remediation (baseline run on `090b61f`: `1 passed;
0 failed`, 1.12s) and after it (`1 passed; 0 failed`, 1.29s). The failure came
from the builder's own execution sandbox denying a loopback bind, not from the
code and not from this repository.

The consequence matters more than the detail: no full-suite run stood behind the
"all recovery tests pass" claim. A sandbox-blocked run was reported as a suite
result. This review's verdict rests on a completed run, recorded below.

## Evidence

- `cargo test --manifest-path _tools/asma-rs-kontor/Cargo.toml --workspace` —
  exit 0; 110 suites, 1394 passed, 0 failed, 8 ignored. All five tests added by
  the remediation confirmed run and green by name, rather than inferred from the
  total.
- `memory_parity` green on both `090b61f` and `751ad35`, as above.
- 5 tests added by the remediation: 3 store-level in
  `crates/kontor-store/tests/operational_promotion.rs`, 2 through the public API
  in `crates/kontor-daemon/tests/loopback_api.rs`.
- Round 1's coverage gap is closed: the restart arm of the architecture's
  required proof (`ARCHITECTURE.md:305`) is now exercised for both
  `quick-sessions:ensure` and `promotion:apply`.

An operational note, since it nearly produced a false result here: an earlier
attempt at this run executed from the superproject root, where `cargo` exited
101 with "could not find `Cargo.toml`" while the shell wrapper still reported
success. Runs recorded in this document use an explicit `--manifest-path`.

## Gate

`code-review-gate`: **passed** — recorded through `kontor_gate_record` as the
inspector, citing `code-change` and `review-notes`.

- receipt: `01a01041-2fc1-7293-a078-af9e822787b6`
- sequence: 2 (round 1's rejection is sequence 1)
- evaluator: role `inspector`, account `01a00751-5be9-7281-bba5-75d8c0c101e7`
- readback: `kontor_task_get` reports `code-review-gate: passed` at snapshot
  cursor 98 — the receipt is the command, this is the state.

Round 1's rejection stands as the first verdict on this task; this is the second.
The next phase is `qa`, whose gate belongs to the tester.
