Artifact: gate-repair-report-round-9

# ASMA-8116 — repair of F-8116-V18

Rejected candidate: `429b358`
Verifier report: `HIGH-VERIFICATION-ROUND-9.md`, commit `67aa816`, preserved verbatim

## What this round actually repairs

The tasking described the F-8116-V17 write-boundary identity gap. The round-9
report **closes** V17: the proved immutable id is carried separately from
`observation_hash`, the write-boundary read compares it before any effect, and
M17a/M17b/M17c were independently seeded, killed and restored.

The release blocker is **F-8116-V18**, which the V17 repair introduced. Deferring
the epic binding advance until after the transition dry run was correct for the
write-bearing path and wrong for every path that writes nothing: both non-writing
policy outcomes return before that point.

- `ReconciliationOutcome::NoOp` returns `Converged` before committing.
- `ReconciliationOutcome::Conflict` records the status conflict and returns
  `Blocked` before committing.

So a rename that was *proved* never advanced the durable key or the rename
occurrence, and the next resident pass began again from the obsolete alias — for
ever. This is a durable reconciliation defect, not an effect-safety failure.

## The repair

`advance_proved_rename` is now the single place a deferred, already-proved
rename is committed, and it is called on every terminal outcome:

| Outcome | When the binding advances |
| --- | --- |
| `Transition` | after the connector dry run has re-proved the immutable id at the canonical key — unchanged from V17 |
| `NoOp` → `Converged` | at the exit; the pass writes nothing, so the alias proof is the whole proof and there is no later boundary to wait for |
| `Conflict` → `Blocked` | at the exit; likewise writes nothing, and the conflict itself was recorded against the current key |

No identity read is added: the decision already carries the proved issue id and
the key Jira reported. The V17 ordering is preserved exactly where it matters —
a transition-bearing path still waits for the write-boundary proof, so a rebind
can never move the ledger behind a refused write.

## Regressions retained

`a_converged_epic_pass_still_carries_a_proved_rename_forward` and
`a_policy_blocked_epic_pass_still_carries_a_proved_rename_forward` drive the two
non-writing exits through a shared fixture. Each asserts, in order: the write log
is empty (this is the non-writing exit under test), `renamed == 1`, the durable
key reads `MOVED-9`, and exactly one rename occurrence is spent; then a replay
adds no further rename, spends no further occurrence, and still writes nothing.
Both supply an explicit legacy backlog code so a stalled placement (OQ-004)
cannot be mistaken for the behaviour under test.

## Mutations — seeded, killed, restored

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M18a | Drop the advance on the converged exit | `a_converged_epic_pass_still_carries_a_proved_rename_forward` | **killed** |
| M18b | Drop the advance on the policy-conflict exit | `a_policy_blocked_epic_pass_still_carries_a_proved_rename_forward` | **killed** |
| M17a | Remove the write-boundary immutable-ID comparison (re-checked) | `a_write_boundary_rebind_is_refused_before_any_effect_or_binding_advance` | **killed** |

M17a was re-seeded deliberately: this round changes when a rename commits, and
the V17 guarantee had to be shown still to bite afterwards.

## Gates — isolated `CARGO_TARGET_DIR` throughout

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` (4 crates, `-D warnings`) | clean |
| `cargo test -p kontor-store --test jira_materialization` | ok, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | ok, 58/58 |
| `cargo test -p kontor-jira` | ok, 9 + 19 |
| `cargo test -p kontor-api` | ok, 23 + 5 + OpenAPI 3 |
| `cargo test -p kontor-daemon --lib` | ok, 81/81 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 (1 predeclared ignored) |
| reconciler / rename / rebind / non-writing targets (9) | ok |

## Remaining inherited failure

`an_epic_placeholder_body_is_typed_reported_and_repairable` still fails with
`503 unavailable — the configured native Jira connector could not answer`. It is
the inherited **ASMA-8123** placeholder-body failure and has failed identically
at `429b358`, `3ee6e76`, `b8e9549` and `560db2c`, including with the connector
changes of this and prior rounds reverted in the same clean target. Outside this
repair's scope; recorded, not fixed.

## Not claimed

This report does not claim gate completion. It records one bounded repair and
its evidence for independent verification.

## Preserved

Every earlier repair, all existing Jira/native identities and policies, legacy
`JiraItemCode`/token/hash behaviour, ASMA-8117 ownership, OQ-002/OQ-004/OQ-005
and migration `0095` ordering. No merge, deploy, Jira or topology action.
