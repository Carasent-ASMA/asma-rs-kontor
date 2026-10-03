# ASMA-8196 roster-GET re-verification

Date: 2026-10-02
Task: Jira `ASMA-8196` under epic `ASMA-8190` (Paseo-direct)
Branch: `feat/ASMA-8196-roster-occupancy-read-port`
Checkout: `/Users/igor/carasent/asma-modules/.worktrees/asma-8196/asma-rs-kontor`
Source head: `940138c5578b67c1590587a0d29c10390a58847a`
Parent: `c56b80eba5879ee7f6a45afc43a1c2e3ed4d6cd5`
Base: `f95e206563bca88b6871f48528623441e5e1a231`

This record is additive. It does not replace `2026-09-20-21-53-report-independent-source-reverification.md`. It is not a gate, a merge, or a deployment claim.

## Verdict

`PASS` for the roster-GET finding on source head `940138c5`.

The rework reads the current occupancy generation from the seat and loads that generation's persona. A claim that supersedes a launched occupancy reports null on the roster, the occupancy chain agrees, and the retired occupancy keeps its frozen persona. The read-port functions verified at `c56b80eb` are byte-identical. Reintroducing `latest_hosted_seat_role_persona` makes the new regression fail. Source was restored; tracked diff after restoration was empty.

## Claims

| Claim | Result |
|---|---|
| Claim over a launched occupancy reports null on the roster, not the predecessor `prompt_hash` | **Pass** |
| Roster and occupancy chain agree for that current generation | **Pass** |
| Retired occupancy keeps its own frozen persona | **Pass** |
| `epic_hosted_seat_occupancies`, `hosted_seat_occupancy_dto`, and `epic_control_seat_binding` are unchanged from `c56b80eb` | **Pass** |
| The four `kontor-api` files have zero diff against `c56b80eb` | **Pass** |
| `latest_hosted_seat_role_persona` has zero callers | **Pass** |
| Predecessor-persona fallback is killed by the new regression | **Pass** |

`latest_hosted_seat_role_persona` remains defined in `crates/kontor-store/src/repository.rs`. The only other hit is a comment in `epic_core_team_dto`. The roster path calls `hosted_topology_seat_occupancy_generation` (`1 + COUNT(history)` on the current seat row) and then `get_hosted_seat_role_persona` for that generation. No current row yields null.

## The defect, re-tested

`a_claim_superseding_a_launched_occupancy_reports_a_null_roster_persona` launches an LSA, checks the roster hash equals the generation-1 persona, claims a hand-started native, and then requires:

- current occupancy generation greater than 1 and no persona row for it;
- roster `role_persona` null while `native_seat.native_id` is the claimant;
- that roster value unequal to the launched `prompt_hash`;
- the chain's current occupancy null, with the same persona value as the roster;
- `occupancies[0].role_persona.prompt_hash` still equal to the launched persona.

The test passed on `940138c5` before the mutant.

## Unchanged surface

Function bodies were extracted from the signature line through the matching closing brace and hashed with SHA-256. The same extraction at `c56b80eb` and `940138c5` matched.

| Function | SHA-256 |
|---|---|
| `epic_hosted_seat_occupancies` | `6da4f88183af0f155556f215ab52eb1abb7b5d9093b366a202abdd6467e4ba17` |
| `hosted_seat_occupancy_dto` | `f16babd0b78485fd62e776a170adbe19402bab561a6111fc10d73f43b667d56a` |
| `epic_control_seat_binding` | `450bdc1ac1336e00041df05eb30d88818f4fbdea8f80343f7c4215f63f2ee2d0` |

Git blob ids, identical at both commits:

| File | Blob |
|---|---|
| `crates/kontor-api/contract/openapi.json` | `067cf292b889ffca293ca2f909bc018725b41506` |
| `crates/kontor-api/src/applications.rs` | `d05678665252adb518a7c3ead4ed4b54538e9fd2` |
| `crates/kontor-api/src/lib.rs` | `322362bfb43a53192f107e22137e5ade87a442b6` |
| `crates/kontor-api/src/openapi.rs` | `508095dfd0ccb12a324e9557fc7ab89e6e58bce9` |

`git diff c56b80eb 940138c5` is only `crates/kontor-daemon/src/applications.rs` and `crates/kontor-daemon/tests/loopback_api.rs`, +264/−12.

## Mutant

In `epic_core_team_dto`, the current-generation lookup was replaced with `latest_hosted_seat_role_persona`, which is the defect from `c56b80eb`.

Command:

```
cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_claim_superseding_a_launched_occupancy_reports_a_null_roster_persona
```

**Killed.** `loopback_api.rs:65060`. The roster returned the generation-1 persona (`occupancy_generation` 1, `prompt_hash` `be05a2220f6800089f7d51ecb44763ad830747df4635c2f3a40e8b34960117e7`) on the claimant. Exit 101. 0 passed, 1 failed, 484 filtered out, 4.18s.

Restored with `git checkout -- crates/kontor-daemon/src/applications.rs`. `git diff --stat` was empty.

| File | SHA-256 before the mutant and after restore |
|---|---|
| `crates/kontor-daemon/src/applications.rs` | `3ccba358352695bc520b1f78383cb3c9c55ead483fdc6b0293784f7eac2c8fb1` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `cc6507565e3b8b3842c4125a2aeb02e85db5056acb6fe12de905031bbac2efd5` |

## Commands

`cargo test -p kontor-api --test openapi_contract`

3 passed, 0 failed, 0 ignored, 0.05s. Exit 0.

`cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_claim_superseding_a_launched_occupancy_reports_a_null_roster_persona`

1 passed, 0 failed, 484 filtered out, 1.98s. Exit 0. This run was before the mutant.

`cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_promotion_creates_one_epic_and_hands_the_work_to_its_lsa`

1 passed, 0 failed, 484 filtered out, 4.48s. Exit 0. This test still holds the occupancy-chain reads from `c56b80eb`.

`cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact materializing_a_replaced_seat_reuses_its_current_occupancy`

1 passed, 0 failed, 484 filtered out, 3.75s. Exit 0.

`cargo test -p kontor-daemon --test loopback_api`

484 passed, 0 failed, 1 ignored, 110.10s. Exit 0. The ignored test is `a_configured_jira_boundary_distinguishes_historical_from_native_completion`. `legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery` passed in this run. The earlier parallel 409 was not reproduced and was not changed.
