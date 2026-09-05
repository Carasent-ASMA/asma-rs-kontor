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

### Killed compatibility mutation: archived exact fetch treated as live

The live naming census exposed a Paseo compatibility change: exact agent fetch
can return an archived agent, including its historical workspace. The original
adapter treated that result as a live rename target. Before the correction,
`seat_retitle_classifies_an_exact_archived_native_agent_as_stale` failed because
preview returned success instead of `StaleBinding`. The adapter now rejects the
archived snapshot before provider-session or workspace correlation; the same
test passes and proves no `agent update` command was emitted. Whole-epic naming
therefore retains the logical seat as `rename_pending` without rewriting
archived native history.

### Killed migration mutation: legacy ESW root absence treated as drift

The live apply exposed an older native-root binding whose persisted
`canonical_cwd` is absent even though the exact runtime identity reads its
canonical root back. The regression reproduces that legacy `NULL` after
materialization. With the old apply comparison restored,
`a_team_definition_upgrade_preserves_native_ids_and_renders_confirmed_item_codes`
fails with the same `stale_binding` returned by the live migration. The
corrected planner retains the fresh runtime root only for a native project
container whose persisted root is absent; a stored root still has to match
exactly, and the complete preview hash still freezes the runtime readback.

After restoring the correction, the focused regression passes. The full
daemon contract run passed 266 tests before seven Jira mock-server cases were
refused only because the sandbox prohibited loopback port binding; all seven
passed when rerun with loopback permission (nine Jira-filtered passes plus the
one separately filtered resident-controller case, with one superseded test
ignored). Formatting and daemon all-target Clippy with warnings denied also
pass. No mutation remains in the tree.

## 2026-09-05 — initial-hold admission mutation

The new `ensure_initial_hold` path was temporarily changed to return the live
epic-wide grant without invoking `execution:disarm`. The focused regression
`an_epic_can_be_created_with_a_replay_safe_covering_hold` failed on the absent
revocation timestamp and printed the live authorization projection. Restoring
the disarm call made the same test pass. This kills the failure mode that would
make kickoff appear held while leaving its ready tasks admissible. No mutation
remains in the tree; the full receipt and scheduler proof is in
[ATOMIC-KICKOFF-HOLD.md](ATOMIC-KICKOFF-HOLD.md).

## 2026-09-05 — terminal legacy TeamRun naming-census mutation

The live Team Definition v2 apply safely refused with `revision_conflict`
before changing any runtime title. Read-only diagnosis found nine bound child
runs from succeeded OP-01 and OP-02 TeamRuns. Those teams closed before
SeatBindings existed, so their child lifecycle projections remained
nonterminal even though their immutable parent TeamRuns were terminal.

The mutant is the original census predicate: omit the parent TeamRun terminal
lifecycle filter and treat every seatless, bound, nonterminal child run as a
live migration subject. With that predicate,
`a_terminal_team_run_makes_legacy_seatless_bound_sessions_history` failed with
the same `Conflict` as the live apply. Restoring the predicate that excludes
terminal parent TeamRuns made that regression pass.

The counter-proof
`a_live_delivery_session_with_no_seat_at_its_slot_fails_closed` also passes, so
the correction does not weaken refusal for a genuinely live team. The complete
`team_definition_migration_completeness` suite passes all 14 tests, and
`cargo clippy -p kontor-store --all-targets -- -D warnings` passes. No mutation
remains in the tree.
