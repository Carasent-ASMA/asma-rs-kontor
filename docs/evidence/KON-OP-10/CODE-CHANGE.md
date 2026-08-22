# KON-OP-10 / ASMA-7879 — code change

Date: 2026-08-22
Branch: `feat/ASMA-7879-seven-run-ceiling-qnr-v2` (submodule `_tools/asma-rs-kontor`)
Baseline: `origin/master` `38b9c87` (`38b9c8704d2ab70568df8e3f056efca447248ff6`, PR 86).
Requirement: OP-REQ-032 (seven-run MiniProject Concurrency Ceiling + adaptive
4→7 window), evidenced by EVD-OP-012; EVD-OP-010 (disposable Jira / empty-realm
journey) is carried by the OP-07/OP-08 suites and named below.

## What this delivers

A deterministic, isolated proof of the admission arithmetic the Operational
realm runs under, in one place: the four-way start, one newly eligible candidate
per growth pass through five, six and seven, the eighth-run refusal on the
mission ceiling, and the exact ceiling recorded at zero headroom.

The production mechanism this proves was already shipped across OP-03/OP-08 and
is exercised end-to-end by the loopback suite. The gap OP-10 closes is the
*single integrated admission narrative* at the operational numbers
(`initial=4`, `ceiling=7`, `growth_step=1`, `mission=7`): before this change,
the pieces were proven separately but no one test walked 4 → 5 → 6 → 7 and
refused the eighth.

## The change

`crates/kontor-scheduler/tests/ready_batch.rs` — one new test:

`the_operational_seven_run_ceiling_admits_four_through_seven_and_refuses_the_eighth`

| Assertion | What it proves |
| --- | --- |
| fresh window admits exactly 4 of the first batch | the seeded `initial=4` start |
| window widens by exactly one step per fold, 4 → 5 → 6 → 7 | `growth_step=1`, ceiling 7 |
| exactly one newly eligible candidate admitted per pass at 5, 6 and 7 | active totals climb one slot at a time |
| eighth candidate refused `capacity_exhausted` | seven is the ceiling |
| refusal evidence carries `Capacity { limit: Mission, remaining: 0 }` | the *mission* ceiling bound it, at zero headroom — not the window, not the account |

The window width is an input to the pass, not a derivation inside it. The
two-distinct-clean-observations gate that advances that width lives in
`kontor_accounts::fold` (see the proof matrix); the test's `observe(Clean)` per
pass stands in for one `fold`'s second reading.

## Proof matrix — EVD-OP-012

| Property | Where proven |
| --- | --- |
| isolated 4 → 5 → 6 → 7 admission | **this test** (`ready_batch.rs`); `kontor_accounts::admission::fold` streak tests; loopback `a_plan_admits_against_the_width_that_was_learned` (4→5 over the store) |
| two distinct clean observations per growth step | `kontor_accounts/src/admission.rs` — `the_second_distinct_clean_observation_grows_by_exactly_one_step`, `replaying_one_observation_changes_nothing_at_all` |
| eighth-run refusal | **this test** (`capacity_exhausted`, mission at zero) |
| idle-seat exclusion (TeamRuns counted, seats not) | loopback `the_mission_ceiling_counts_team_runs_and_not_the_seats_they_hold`; runtime-paseo `capacity_counts_active_processes_not_persistent_idle_seats` |
| pressure contraction without cancellation | `the_adaptive_window_grows_on_clean_observations_and_falls_to_the_floor_under_pressure`, `narrowing_the_window_below_the_work_in_flight_cancels_nothing` (both `ready_batch.rs`); `kontor_accounts::admission::pressure_narrows_to_the_floor_and_forgets_the_trend` |
| persisted window / streak / last observation survive restart & replay | loopback `a_plan_restores_the_persisted_adaptive_width_rather_than_starting_at_four` |
| Operational ceiling configuration (mission 7, adaptive ceiling 7) | `crates/kontor-daemon/src/lib.rs::DEFAULT_CAPACITY` + loopback `the_capacity_configuration_reports_the_operational_ceilings_and_guards_its_revision` |

## The disposable Jira / empty-realm journey — EVD-OP-010

The empty-realm bootstrap and the Jira create/link/read-back journey are owned
by OP-07/OP-08 and are proven by their suites, which OP-10 reuses rather than
re-implements:

- empty realm, no seeded state: loopback `an_empty_realm_is_bootstrapped_through_public_operations_alone`; MCP `an_empty_realm_is_bootstrapped_through_mcp_tools_alone`.
- no `asma` executable dependency: loopback `a_capacity_refresh_answers_without_any_external_executable`.
- Jira workflow install / create / link / ownership / comment mirroring: the KON-MVP-18 pilot `domain.jira-*` criteria and the `kontor-integrations-asma` connector fixtures.

## Scope note — what is deliberately not here

The full fourteen-step acceptance narrative runs against a **live** isolated
user home with a real Paseo fleet and a real Jira boundary (disposable Epic and
Issues created, closed and cleaned up with read-back receipts). That is a live
exercise requiring the runtime fleet and the native Jira connector's credential
boundary; it is not reproducible by this offline recovery seat and is left to
the run that has those surfaces. This change proves the deterministic admission
core — the 4→5→6→7 climb, the eighth refusal, idle-seat exclusion and pressure
contraction — and records exactly which live surface each remaining step needs.

No QNR production changes were made; the QNR owner has not opted in.
