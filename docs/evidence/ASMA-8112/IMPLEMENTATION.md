# ASMA-8112 implementation

Date: 2026-09-06

Status: implemented on `fix/ASMA-8112-targetless-handoff`; full `kontor-daemon`
suite, `fmt` and `clippy` green.

## The defect

A downstream delivery seat is materialized with its `TeamRun` and carries no
admission event of its own. When its first native launch is refused, the only
durable authority that can activate the slot is the undelivered `turn_dispatch`
the upstream handoff decided. `Services::replace_seat` therefore gated its
never-bound recovery on finding one:

```rust
dispatch.target_agent_run == Some(agent_run_id)
```

A dispatch records its target at derivation, in `derive_follow_ups`, from
`seat_for_slot` — which resolves only the slot's **non-terminal** member. An
operator-abandoned run is terminal. The recovery itself requires the exact run
to be abandoned first (`is_operator_abandoned_unbound`), so whenever the
abandonment precedes the upstream settlement, the handoff is persisted with
`target_agent_run` NULL and no row can ever name the seat. Nothing re-targets it
later: `mark_turn_dispatched` writes a target only when delivery succeeds.

The result was a seat with no admission event to resume and no dispatch that
would admit to being about it — the exact state the recovery exists to repair,
refused with `revision_conflict` / "no pending handoff dispatch or recorded
successor authorizes this never-bound seat".

## The repair

`crates/kontor-daemon/src/applications.rs`, never-bound arm of
`Services::replace_seat`.

The undelivered dispatches for the predecessor's `TeamRun` and role slot are now
read once into `undelivered`. Exact-target dispatch remains the primary
authority and is unchanged. Additionally authorized: the case where
`undelivered` holds **exactly one** row and that row's `target_agent_run` is
NULL.

```rust
let targetless_dispatch = matches!(
    undelivered.as_slice(),
    [only] if only.target_agent_run.is_none()
);
```

Refused, unchanged, as `revision_conflict`:

- zero undelivered rows for the slot;
- more than one undelivered row for the slot, targetless or not — two decisions,
  neither of which says which one is being recovered;
- a single undelivered row naming a different run.

Delivery needed no change. `retry_undelivered_dispatches` re-resolves
`seat_for_slot` at retry time and ignores the recorded target, so the successor
receives the same message id and `mark_turn_dispatched` then records the seat it
reached.

### Interpretation recorded

The authorizing count is taken over the whole candidate set — undelivered, same
`TeamRun`, same role slot — and that single row must be targetless. The narrower
alternative (count only the targetless rows, and ignore a coexisting row naming
another run) would additionally authorize the mixed case
`[targeted-at-another-run, targetless]`. That reading was rejected as a widening:
the handoff specifies the minimal repair, and refusing is side-effect-free and
correctable under a new key, while wrongly authorizing creates a successor and
launches it.

## Fences preserved

Untouched, and each still exercised by the existing suite: binding generation
zero for a never-bound predecessor; the required explicit Admin model route;
mutual exclusion of provider-outage and quota evidence on this arm; predecessor,
task and team revisions; role equality against the requested slot; the terminal
team and team-definition-migration fences; the `OperatorAbandon`/`Abandoned`
terminal evidence check; idempotency and the recorded-successor replay path; and
successor lineage via `parent_agent_run_id` and
`reserve_after_unbound_abandonment`.

No API, MCP, schema or migration change. `turn_dispatches.target_agent_run` was
already nullable (`0018_role_turns.sql`).

## Tests

`crates/kontor-daemon/tests/loopback_api.rs`. All four seed
`omega_with_one_unbound_slot(_, "omega-u-cat")` and abandon the refused
`omega-k3` launch **before** settling the upstream `omega-k1` turn, which is what
derives the targetless handoff. Each negative allows the `omega-k3` launch on
purpose, so a gate that wrongly authorizes fails on a created successor rather
than being rescued by a refusing runtime.

- `an_admin_reroutes_a_never_bound_seat_whose_handoff_recorded_no_target` —
  asserts the precondition from the store (one undelivered row, no target), then
  a successful replacement: successor created and bound, account pinned from the
  exact recovery alias, `parent_agent_run_id` naming the predecessor, the
  abandoned predecessor retained as lineage, the same handoff delivered and now
  recorded against the successor, exact-key replay returning `unchanged` with no
  third seat and no relaunch, and the scheduler's exact admission replay
  hydrating the recovered lineage with nothing blocked.
- `two_targetless_handoffs_refuse_a_never_bound_replacement` — two upstream
  turns each decide their own targetless handoff; the replacement is refused
  `409 revision_conflict`, no seat is created and neither handoff is delivered.
- `a_handoff_naming_another_run_refuses_a_never_bound_replacement` — the one
  pending handoff is re-pointed at a live sibling, leaving it undelivered for the
  same `TeamRun` and slot; the replacement is refused and no seat is created.
- `a_mixed_targetless_and_mistargeted_pair_refuses_a_never_bound_replacement` —
  the case that pins which set the "exactly one" is counted over. Two upstream
  turns each decide a handoff; one is then re-pointed at a live sibling by its
  own settling turn, leaving `[names another run, names nothing]` undelivered for
  the same `TeamRun` and slot. Counting only the targetless rows would find its
  single row and authorize; counting the whole candidate set finds two decisions
  and refuses. Asserts `409 revision_conflict`, no seat created, and that the two
  rows are unchanged — nothing delivered, re-targeted or derived. It also pins
  the absence of a recorded successor immediately before the call, so the
  separate `already_replaced` authority in the same gate cannot be what the
  refusal is passing on.

The positive test was confirmed to fail against the unrepaired gate with exactly
the production refusal (`revision_conflict`, "no pending handoff dispatch or
recorded successor authorizes this never-bound seat") before the repair was
restored.

## Validation

`cargo test -p kontor-daemon` at the production commit — **390 passed, 0 failed,
1 ignored**; 0 doc-tests.

The one ignored test is pre-existing and unrelated to this change:
`a_configured_jira_boundary_distinguishes_historical_from_native_completion` in
`tests/loopback_api.rs`, carrying
`#[ignore = "superseded by kontor-jira native connector contract tests"]`.

| Target | Tests enumerated |
| --- | --- |
| `lib` (unit) | 71 |
| `tests/account_pinning.rs` | 5 |
| `tests/loopback_api.rs` | 283 (282 run, 1 ignored) |
| `tests/mcp_journey.rs` | 2 |
| `tests/quota_observation.rs` | 21 |
| `tests/recovery_security.rs` | 6 |
| `tests/succession_handoff.rs` | 3 |

`cargo fmt -p kontor-daemon -- --check` — clean.
`cargo clippy -p kontor-daemon --all-targets` — clean, no warnings.

The mixed-candidate-set test was added afterwards, in review remediation, and
takes `tests/loopback_api.rs` to 284 enumerated. Production code is unchanged by
that commit; the four ASMA-8112 tests were rerun individually and pass.

Only `kontor-daemon` is touched, so no other crate's suite was rerun.
