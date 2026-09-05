# KON-OP-22 mutation proof

Date: 2026-09-03

## Killed mutation

The Jira entity-neutral apply boundary was deliberately changed to accept an
`applied` response without refetch confirmation.

The exact regression
`entity_neutral_delegation_binds_apply_to_observation_route_and_readback`
failed because the malformed response was accepted. The confirmation guard was
restored and the same test passed. No mutation remains in the tree.

## Test-strengthening result

A preliminary mutation that emitted an append signal after every epic-conflict
attempt survived the original request-count-only assertion because the
existing open conflict short-circuits subsequent durable insertion. Production
was restored immediately. The regression now additionally observes the append
signal directly and proves a deduplicated conflict publishes no new wake. This
exercise changed the test only; it did not weaken or remove the production
`inserted` guard.

## 2026-09-05 — gate-evaluator identity mutations

The evidence-integrity follow-up was mutation-tested at the two evaluator-seat
boundaries added by merge `082b63ad2e15beddac3b745bdf55c794f35d0b88`.

### Killed mutation: a different live role may stand in for the evaluator

`live_evaluator_seat` was temporarily changed to accept the first live seat
without checking its role. The focused regression
`an_ordinary_gate_verdict_requires_a_live_evaluator_seat` failed: the malformed
implementation returned HTTP 200 and appended a rejected gate evaluation where
the test required HTTP 400. The exact role filter was restored and the test
passed.

### Killed mutation: any real account may attribute the evaluator verdict

The ordinary gate path's pinned-account comparison was temporarily inverted,
allowing a different enabled account through. The focused regression
`an_ordinary_gate_verdict_requires_the_evaluator_seats_pinned_account` failed:
the malformed implementation returned HTTP 200 and appended a rejected gate
evaluation where the test required HTTP 400. The inequality check was restored
and the test passed.

Both restored regressions pass together. `cargo fmt --all -- --check` and
`git diff --check` also pass. No mutation remains in the tree.
