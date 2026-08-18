# KON-OP-06 review remediation — checkpoint 4

Date: 2026-08-18
Ticket: ASMA-7875
Answers: `docs/evidence/KON-OP-06/REVIEW.md` (verdict REJECTED, review commit `5554aa4`)
Remediates: `e58336d`

## R1 — durable state on a refused advance — FIXED

`advance_completion` started the run before the `expected_revision` guard, so a
first advance presenting anything but the initial revision created the epic's
completion run, returned `409`, and recorded no receipt for the write it had just
performed. The refusal also claimed the run had *moved since the caller read it*,
which it could not have — the read route answers `404` until that call creates the
row.

Fixed by the reviewer's first suggested shape. `advance_completion` now:

1. reads the existing run without creating one;
2. builds the canonical intent and judges the idempotency key;
3. guards the revision — against the standing revision when a run exists, and
   against `AggregateRevision::INITIAL` when none does, with an honest rule
   (*"this epic has no completion run yet, so a first advance must present the
   initial revision"*) and that revision in `current_revision`;
4. only then calls `start_completion`.

No durable completion state is created on a refused advance. A start that is
followed by a *refusing transition* does leave the run standing; that is
deterministic initialization of the epic's own declared contract, is re-derived
identically on the next call, and the command receipt still covers only the
transition that committed. The code says so at the call site.

## R2 — no API-level test for the two state-machine operations — FIXED, and it caught two more defects

Three tests added to `crates/kontor-daemon/tests/loopback_api.rs`:

- **`a_refused_first_advance_creates_no_completion_run_and_no_receipt`** — pins R1
  directly: wrong revision → `409 revision_conflict` naming the initial revision
  and the honest rule; the read is still `404`; and reusing the *same* key with a
  corrected revision is not an `idempotency_conflict`, which is only true if the
  refused call wrote nothing to the ledger. Then the corrected call passes the
  guard and fails on its real missing dependency, still leaving no run.
- **`advance_and_remediate_judge_the_key_before_the_revision`** — drives both
  writes over a real run on a promoted epic with materialized ECP seats, covering
  the replay → stale-`expected_revision` sequence the review asked for on each:
  advance (200, phase `integration`, one wake naming the epic's TPM seat) → replay
  (`unchanged`, no second transition) → different key with the stale revision
  (`409`, `current_revision` 2); then route-before-proposal refused, proposal
  against the wrong round's evidence refused, proposal accepted without moving the
  run, proposal replayed, route accepted and the run moved, route replayed, and a
  stale route refused.
- **`remediate_on_an_unstarted_completion_run_refuses_without_a_receipt`** — `404`
  and no receipt.

The run in the second test is seeded with no declared tickets so the ticket gate
is vacuously satisfied: what is under test is the handler composition — replay,
revision, authority, phase — not OP-01 evidence plumbing.

### Two further defects these tests found, both in this change

**`epic_control_seat` addressed the wrong topology node.** It took
`scope_nodes(...).first()`, but `scope_nodes` filters by epic only and ignores the
scope's kind. An epic owns at least its own ESW as well as its ECP, so the first
node was the delivery workspace — which then truthfully reported holding none of
the control-plane seats. Every completion path that resolves the LSA or TPM seat
was affected: starting a run, waking the TPM, and both remediation authorities.
Now matched on `scope.kind`.

**`remediate_completion` judged the phase before the key.** The revision guard was
correctly ordered (the review verified that), but the `AwaitRemediation` phase
check was not: a successful route leaves the run in `remediation`, so replaying
that route hit the phase guard and was refused — the same class of mistake as R1,
one guard further along. `remediate_completion` now builds its intent from the
caller's action alone, judges the key, and applies the revision *and* phase guards
only when nothing was replayed. Dropping the server-looked-up proposal digest from
the route intent is what lets the retry rebuild it without first reading state the
original call has moved.

## Verification

| Check | Result |
| --- | --- |
| `cargo test --workspace` | exit 0 — **1409 passed, 0 failed, 8 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, clean |
| `cargo fmt --all --check` | clean |

The 8 ignored are the same environment-gated cases the review identified; none is
a completion proof.

## Not done here

Review item 3 — re-running the KON-MVP-18 pilot harness against the fixed HEAD and
committing the bundle — is out of scope for this remediation and remains open
before that bundle may be cited as checkpoint-4 evidence. The review records it as
required-before-citation rather than part of the gate.

Of the review's non-blocking observations, the stale console types were closed
separately on this branch by `0f08a0c` (`apps/console/src/api/schema.d.ts`), which
this remediation is rebased onto. The rest stand unchanged: the
`command_receipts` append-only triggers dropped by the table-rebuild pattern are
pre-existing from `user_version = 23` and not introduced here.
