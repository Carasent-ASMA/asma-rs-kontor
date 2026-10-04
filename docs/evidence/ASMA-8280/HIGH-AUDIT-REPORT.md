# ASMA-8280 high-audit report — bounded slice two

Date: 2026-09-27
Artifact: `high-audit-report-asma-8280-slice-two-20260927`
Task: Jira `ASMA-8280`
Phase: bounded slice-two audit (not TASK-002 Goal)
Audit seat: Paseo agent `ed34590c-3623-41ca-ac27-68853c9cb4b9`
Native session/handle: `01a0e42c-474d-7891-a835-23988f03f626`
Parent TPM: `3c52abda-4fd0-4af6-b086-476bc51eb305`
Workspace: `wks_a284b5d96a0ea32b`
Checkout: `/Users/igor/carasent/asma-modules/.worktrees/asma-8280/asma-rs-kontor`
Audited verification HEAD: `0eee8f6341104877f5c54749c2d90a030a164d71`
Implementation candidate: `ec68ec5a8c5029d11728cc6f6c45c77b5b4255bf`
Implementation tree: `51db91c62531dea43f4332105d29e1c54d9ae82b`
Decision: **PASS**

## Verdict

The bounded slice-two verification passes audit. Commit `0eee8f63` is directly
on `ec68ec5a`, and its complete delta is the single new file
`docs/evidence/ASMA-8280/HIGH-VERIFICATION-REPORT.md`. The report's SHA-256 is
`cd869720e7143f1cdf7a789cb4d7230e96cd2bdf5d17b61fe6f6361cbbbd29e8`.

The verification report accurately describes the implementation tree and its
limits. No P0 or P1 finding was found within this bounded verification. This
PASS does not close TASK-002 or ASMA-8280 and does not qualify the full Goal.

## Prior verification receipt

Paseo readback identifies verify seat
`2399c746-32aa-4d10-a39e-ccdbc8c5aed1`, native handle
`4447678e-9d39-4eea-a910-3b0356b2a614`, in the required workspace and checkout,
with parent `3c52abda-4fd0-4af6-b086-476bc51eb305`. Its completed activity reports
**PASS** on candidate `ec68ec5a8c5029d11728cc6f6c45c77b5b4255bf`, names
`0eee8f6341104877f5c54749c2d90a030a164d71` as its report-only commit, and the
seat is idle.

## Independent tree confirmation

- `Services::leadership_route` is called only by Core Team route planning,
  Core Team materialization, and launch-intent supersession. It constructs a
  proved `LeadershipKey` from the complete pinned Core Team revision and exact
  frozen seat, then resolves through `kontor-fleet`.
- A bound seat admits only an exact provider/model/effort rung returned by
  `FleetResolution::route_for`; exhausted or off-chain resolutions fail closed.
- Leadership decision rows record policy hash, schema, binding key, chain,
  pinned revision hash, slot, route position, provider/model/effort, vendor,
  SeatBinding and occupancy generation. The loopback comparison reparses the
  activated content-addressed artifact with `kontor_fleet::FleetSnapshot` and
  compares those fields directly.
- Materialization preserves each SeatBinding and records first occupancy as
  generation 1. Route correction retains the SeatBinding while creating and
  recording generation 2. Exact launch-intent supersession replay records no
  duplicate decision.
- Tampered activation and off-chain route tests refuse before a native effect;
  the materialization test also proves no logical Core Team seat is written.
  Unactivated publication and `fleet.yml` edits do not influence resolution,
  while a changed activation expires a route-correction preview.
- Committee recovery and native-less consultation reroute both refuse on an
  unverifiable activation without the tested native or durable-state effect.
- `crates/kontor-runtime-paseo` is unchanged from `5d0921b3`. The compiled
  `cursor/gpt-5.6-sol` arm predates this slice, and the regression confirms an
  activated policy does not widen the historical operator exception.
- Fleet get, preview, publish, and activate are present in the API route table,
  MCP registry, and MCP parity contract under the existing applications
  surface. The candidate adds no separate CLI command path.

## Independent checks

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p kontor-fleet -- --test-threads=8` | PASS: 15 passed |
| Cursor historical-exception unit regression | PASS: 1 passed |
| registered fleet policy operations loopback | PASS: 1 passed |
| leadership materialization loopback | PASS: 1 passed |
| leadership route-correction loopback | PASS: 1 passed |
| leadership launch-intent supersession loopback | PASS: 1 passed |
| Committee recovery fail-closed loopback | PASS: 1 passed |
| native-less reroute fail-closed loopback | PASS: 1 passed |

## Explicitly unmet and not claimed

The tree still does not demonstrate:

- a real Paseo adapter launch;
- a policy hash carried on the launch receipt;
- policy-chosen routes or quota walks (the policy admits caller-named routes);
- hosted-seat paths beyond the three named Core Team paths; or
- the full both-modes Goal covering every leadership, delivery, and
  consultation slot.

The ASMA CLI contains no direct-mode policy consumer in this slice, and the
native hosted-seat launch request contains no policy-hash field. These absences
agree with the verification report's non-claims. Migration
`0120_fleet_policy_operations.sql` remains a rebase hazard, not by itself a
defect in this bounded candidate.

## Seat disposition

This seat changed no implementation. It did not write the ECP checkout, push,
move Jira, record completion, or mark TASK-002 or ASMA-8280 complete. Only this
audit report is committed on the task checkout; the audit seat then remains
idle.
