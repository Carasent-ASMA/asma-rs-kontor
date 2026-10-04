# ASMA-8280 high-verification report — bounded slice two

Date: 2026-09-27
Artifact: high-verification-report-asma-8280-slice-two-20260927
Task: Jira `ASMA-8280`
Phase: bounded slice-two review (not TASK-002 Goal)
Verify seat: Paseo agent `2399c746-32aa-4d10-a39e-ccdbc8c5aed1`
Native session/handle: `4447678e-9d39-4eea-a910-3b0356b2a614`
Parent TPM: `3c52abda-4fd0-4af6-b086-476bc51eb305`
Workspace: `wks_a284b5d96a0ea32b`
Checkout: `/Users/igor/carasent/asma-modules/.worktrees/asma-8280/asma-rs-kontor`
Candidate HEAD: `ec68ec5a8c5029d11728cc6f6c45c77b5b4255bf`
Candidate tree: `51db91c62531dea43f4332105d29e1c54d9ae82b`
Parent: `2a96a65086384f3d12680b3157a10bc8cd26364e`
Required ancestors: `1168c17a996283cedc65f1926e99d2fb8e98c74d`, `c52165040ef6ba80e916a6d7e5240cc904d0641e`, `5d0921b34e36f059b44212a2d54a911fcc1898d1`
Branch: `feat/ASMA-8280-share-activated-yaml-policy-across-orchestration-modes` (local, not pushed)
Status: **PASS**

## Verdict

Bounded slice two passes independent verification of candidate `ec68ec5a`.

The three named Core Team call sites resolve LSA and TPM through `kontor-fleet`. A recorded leadership decision matches, field for field, what that library returns from the activated artifact. Native SeatBinding identity is preserved; successor occupancy is an explicit generation. Off-chain routes and tampered activations refuse with no native effect. Publication without activation and an unactivated `fleet.yml` edit do not change routing. Committee seat recovery and native-less consultation reroute fail closed on a broken activation. The ASMA-8255 operator-accepted consultation path and the compiled Cursor `gpt-5.6-sol` exception are unchanged. Publication and activation are registered application operations on the existing route table.

This report does not close TASK-002 or ASMA-8280.

## Invariants at review start

- cwd, workspace, parent, agent id and native handle matched the handoff.
- HEAD, parent and required ancestors matched. The worktree was clean.
- Working context: `ENGINEER` (defaulted; workspace-root `WORKING_ROLE` unset).

## Claims

### 1. The three call sites resolve LSA and TPM through `kontor-fleet`

`Applications::leadership_route` builds `LeadershipKey::for_pinned_seat` from the pinned Core Team revision and the frozen seat, then calls `FleetSnapshot::resolve_leadership`. `core_team_route_plan`, `materialize_core_team` and `supersede_core_team_launch_intent` all consult that helper before any native effect. Materialization binds both LSA and TPM; route correction and supersession use the same helper for whichever frozen seat they address.

Locked by `leadership_materialization_is_resolved_through_the_activated_policy`, `a_leadership_route_correction_is_held_to_the_activated_chain`, `a_leadership_launch_intent_supersession_is_held_to_the_activated_chain`.

### 2. Recorded decision matches the library result from the activated artifact

`record_leadership_decision` writes policy hash, schema version, binding key, chain, roster hash, slot, step, sub-step, provider, model, effort and vendor. The loopback helper `direct_resolution` re-parses `fleet-history/<hash>.yml` with `kontor_fleet::FleetSnapshot::parse_policy` and `assert_decision_is_the_shared_resolution` compares those fields. Daemon result and library result agree.

There is no asma-cli direct-mode policy consumer in this slice (`_tools/asma-cli` `fleet_events.py` is local telemetry, not a resolver). That absence is a non-claim.

### 3. Native seat identity is preserved; successor occupancy is explicit

Route correction keeps the same `SeatBindingId`, launches a new native, and records occupancy `hosted_topology_seat_occupancy_generation + 1` (observed `2`). First materialization records `FIRST_HOSTED_OCCUPANCY` (`1`). Supersession of a never-bound intent keeps occupancy generation `1` and records that generation on the decision.

### 4. Binding key carries roster revision hash and slot

`LeadershipKey` text is `leadership/<core-team-revision-hash>/<role-slot-id>`. The hash is the canonical content hash of the complete pinned revision. There is no `FromStr` / parser; only `for_pinned_seat`. Loopback decisions equal `key.as_str()`.

### 5. Off-chain route and tampered activation refuse with no native effect

Tampering the published artifact refuses with `a published fleet policy does not hash to its content address` and zero hosted launches. An LSA route off the bound chain refuses `placement_blocked` before any logical seat is written. The same off-chain refusal holds for route preview and launch-intent supersession, with the wedged OpenCode intent left unswapped.

### 6. Publication without activation, and an unactivated `fleet.yml` edit, do not change routing

After activation, an additional published policy and a `fleet.yml` rewrite still leave LSA on the activated xhigh-only chain. Between route preview and apply, the same unactivated sources do not expire the preview; a later *activation* does, with no retire or launch.

### 7. Committee recovery and native-less reroute fail closed on a broken activation

Both paths call `fleet_policy()` (verify activation; no fallback) before native effect. Tamper refuses with the content-address check. Recovery leaves adapter calls unchanged; native-less reroute leaves the consultation-run revision unchanged.

Locked by `a_committee_seat_recovery_fails_closed_on_an_unverifiable_activation` and `a_native_less_consultation_reroute_fails_closed_on_an_unverifiable_activation`.

### 8. ASMA-8255 operator-accepted path unchanged; Cursor exception not extended

`crates/kontor-runtime-paseo` has no delta on this branch versus `5d0921b3`. Cursor consultation still admits only the exact operator-accepted recovery route `cursor` / `gpt-5.6-sol` @ `xhigh`. The compiled catalog arm `("cursor", "gpt-5.6-sol")` is the historical 2026-09-23 exception and is not widened; named Cursor accounts still miss it. Schema v2 YAML that tries to author `operator_exceptions`, `operator_accepted`, or similar extra fields is a `PolicyDocument` error.

Locked by `applications::tests::the_historical_cursor_exception_gains_nothing_from_an_activated_policy` and `kontor-fleet` `the_successor_adds_no_section_or_field`.

### 9. Publication and activation are registered application operations

`/v1/fleet/policy`, `:preview`, `:publish` and `:activate` sit on the existing `kontor-api` applications route table. MCP registry rows `kontor_fleet_policy_{get,preview,publish,activate}` name those same paths. `tests/contract/mcp_parity.rs` lists them. Parser/validator/resolver live in `kontor-fleet`; the daemon re-exports `FleetSnapshot` and owns only state-root I/O, activation and receipts. No out-of-registry `kontor` subcommand was added.

Locked by `the_fleet_policy_is_published_and_activated_through_registered_operations`.

## Tests re-run

Only tests that lock a claim not fully proved by reading:

- `kontor-fleet` (15 unit tests)
- `applications::tests::the_historical_cursor_exception_gains_nothing_from_an_activated_policy`
- `the_fleet_policy_is_published_and_activated_through_registered_operations`
- `leadership_materialization_is_resolved_through_the_activated_policy`
- `a_leadership_route_correction_is_held_to_the_activated_chain`
- `a_leadership_launch_intent_supersession_is_held_to_the_activated_chain`
- `a_committee_seat_recovery_fails_closed_on_an_unverifiable_activation`
- `a_native_less_consultation_reroute_fails_closed_on_an_unverifiable_activation`

All passed. The plan's broader scenario set was not started.

## Explicitly unmet (not defects of this slice)

- A real Paseo adapter launch
- A policy hash carried on the launch receipt
- Policy-chosen routes or quota walks
- Hosted-seat paths other than `core_team_route_plan`, `materialize_core_team` and `supersede_core_team_launch_intent`
- The full Goal that both modes resolve every leadership, delivery and consultation slot

Migration `0120_fleet_policy_operations.sql` is a rebase hazard, not by itself a defect.

## What this seat did not do

No implementation edits. No push. No Jira movement. TASK-002 and ASMA-8280 are not marked complete. The ECP checkout was not written. ASMA-8279 was not touched.
