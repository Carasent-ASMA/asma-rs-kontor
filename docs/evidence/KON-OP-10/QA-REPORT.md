# KON-OP-10 / ASMA-7879 — QA report

Verdict: **passed**

The touched slice is `kontor-scheduler` (the admission pass). Run against the
workspace at branch `feat/ASMA-7879-seven-run-ceiling-qnr-v2`.

## Commands

```
cargo test -p kontor-scheduler --test ready_batch
cargo test -p kontor-scheduler
cargo test -p kontor-accounts admission
```

## Results

| Suite | Result |
| --- | --- |
| `kontor-scheduler` `ready_batch` integration | 33 passed, 0 failed |
| `kontor-scheduler` unit + doc | passed |
| `kontor-accounts` `admission` (two-clean-observation fold) | 7 passed, 0 failed |

The new test
`the_operational_seven_run_ceiling_admits_four_through_seven_and_refuses_the_eighth`
passed. It asserts:

- a fresh window admits exactly four;
- one clean fold widens the window by one step, 4 → 5 → 6 → 7;
- exactly one newly eligible candidate is admitted at each of 5, 6 and 7;
- the eighth candidate is refused `capacity_exhausted`;
- the refusal evidence names `Capacity { limit: Mission, remaining: 0 }`.

The referenced `kontor-accounts` fold suite confirms the two-distinct-clean
gate (`the_second_distinct_clean_observation_grows_by_exactly_one_step`,
`replaying_one_observation_changes_nothing_at_all`) and pressure contraction
(`pressure_narrows_to_the_floor_and_forgets_the_trend`), which the new test's
per-pass `observe(Clean)` stands in for.
