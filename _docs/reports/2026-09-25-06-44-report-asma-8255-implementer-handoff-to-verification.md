# ASMA-8255 — Implementer Handoff to Verification

> **Date:** 2026-09-25 06:44
> **Status:** 🟡 Ready for verification (implementer scope complete through TASK-035)
> **Category:** report
> **Scope:** `asma-rs-kontor` branch `feat/ASMA-8255-live-fleet-config` (draft PR #271), the plan `2026-09-24-07-30-plan-kontor-live-fleet-configuration.md`, and the live realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`
> **Summary:** LF-01…LF-07 are implemented and committed; LF-06 was investigation only (verdict: DEF-005). LF-08's mutation pass killed all 20 mutants and TASK-035's gates are green except the two known pre-existing reds. The live system is untouched: nothing is merged, deployed, installed or drilled. This document is the verifier's entry point.

---

## When to Load

**Load this document when:** performing the lead's TASK-036…039 (diff review against the plan, re-running gates, merge, deploy, live `fleet.yml` install, drill D-1…D-8) or auditing what the implementer actually delivered.

---

## Delivery facts

- **Branch:** `feat/ASMA-8255-live-fleet-config` — pushed; PR **#271** (draft, not merged). Supersedes #270, which GitHub closed when its branch was renamed to the canonical name.
- **Commits (oldest first):**
  | Commit | What |
  |---|---|
  | `4062f11e` | LF-01 — `fleet.rs`, `config/examples/fleet.yml`, 20 unit tests (predecessor) |
  | `1dc5a01a` | LF-02 — `Services.fleet`, fleet-aware catalog, `fleet_configuration` provenance |
  | `0fec567a` | LF-03 — delivery seats read the fleet; `declared_delivery_rungs`, `record_fleet_decision`, 6 loopback tests |
  | `a7fff2fd` | LF-04 — Committee/Advisor fleet routes, vendor independence, 5 loopback tests + 1 unit test |
  | `969c18f6` | LF-05 — `independent_of` filter, 3 loopback tests |
  | `19c4d83a` | LF-07 — CONFIGURATION.md section, architecture record, example aligned to Appendix A |
  | `d5c3d527` | LF-08 — calibration-scope test fix (from MUT-009) |
  | `33a5ecdf` | LF-08 — vendor-independence + advisor-provenance observability (MUT-015, MUT-018) |
- **Tree:** clean at `33a5ecdf`. **Live system:** unchanged (no merge, deploy, `fleet.yml` install or drill).

## What is proven

- **TASK-034 mutation pass: 20/20 mutants killed**, each applied as a source defect, verified red on its named test, reverted, tree clean after every row. Four rows needed the plan's "fix the test, not the code" path: MUT-006 (the size rule is three redundant guards, so the mutant is the rule-level removal), MUT-009 (test widened so a scoped rule is distinguishable from an unconditional one), MUT-015 (test extended with same-family/different-vendor reviewers), MUT-018 (test-only fake instrumentation records the launched consultation provenance). These test fixes are the LF-08 commits above.
- **TASK-035 gates** (run at `33a5ecdf`, host 2026-09-25 ~06:40):
  | Gate | Result |
  |---|---|
  | `cargo fmt --all -- --check` | pass |
  | `cargo clippy --workspace --all-targets -- -D warnings` | pass |
  | `cargo test --workspace` | 105 result groups green; **1 known pre-existing red**: `kontor-store` `v115_confirms_exact_local_mutations_with_typed_provenance_and_preserves_history` (plan §"Known pre-existing failures") |
  | `cargo audit` | exit 0, 9 allowed warnings, no vulnerabilities |
  | `cargo deny check` | advisories / bans / licenses / sources ok |
  | `pnpm install --frozen-lockfile` | done |
  | `pnpm --filter kontor-console verify:api` | clean (generated schema matches the committed one) |
  | `pnpm -r typecheck` | done |
  | `pnpm -r test` | 305 passed / 17 files |
  | `pnpm audit --prod` | no known vulnerabilities |
  | `python3 scripts/verify-tree.py --mode archive` | only the known `Cargo.lock` drift |

## What the verifier must still decide or do (the lead's TASK-036…039)

1. **TASK-036** — review the full diff against every REQ/SEC/CON of the plan, rerun the TASK-035 gates yourself on the exact commit, then merge PR #271 once satisfied (`ASMA_ALLOW_AGENT_PUSH=1` no longer needed for pushes: the branch is canonical).
2. **TASK-037** — deploy exactly as PR #267 did (state-root backup first, release binaries for the merged SHA, restart the LaunchAgent, repoint the MCP symlink, read back version/PID/health).
3. **TASK-038** — install the live `fleet.yml` from Appendix A: reconcile every `domains.*.accounts` alias with `kontor_account_profiles_list`, list any switched-off alias in `unavailable.accounts`, **keep `unavailable.domains: [openrouter, deepseek]`** until D-4 and D-8 pass (DEC-010), then confirm `fleet-status.json` has `active_hash` set and `last_error: null`.
4. **TASK-039** — run drill D-1…D-8, record results in the takeover gap report, then resume the stalled epics from their checkpoints.

## Points for scrutiny (decisions the implementer flagged, not defects found)

- **R-01/R-02 never reach the wire with their Appendix-B text.** `ApiError::from_domain` substitutes its own message for `MissingEvidence`; only `ProviderHeadroom` forwards the domain rule. Delivery seats therefore refuse an exhausted bound chain as `400 invalid_request` with `subject: "FleetConfiguration"`. If REQ-008's "fails closed with `PlacementBlocked`" is meant literally for delivery too, that is a small `refuse_domain` change — deliberately not made here.
- **Succession decisions record the predecessor's `agent_run_id`** in `recover_quota_seat`/`refresh_due_succession_attempt` (the successor run does not exist at the Admit site). `last_vendor` keys on team run + binding, so independence is unaffected.
- **`fill_slot`/`seat_with_address` fleet routing has no loopback coverage**: in `seat_fill_world` the fill path is pinned to the fixture's `test` account, and V-03 forbids a `test` provider domain, so a fleet-bound fill refuses (`400 AccountProfile.routing`). The takeover path shares the same `declared_delivery_rungs`/`record_fleet_decision` code and is covered end-to-end.
- **`recover_consultation_seat`'s reviewer-diversity retain still uses `provider_family`** (TASK-023 said not to change other uses); only the materialization block moved to `independence_key`.
- **LF-06 verdict: DEF-005** — three distinct explicit-route selection sites for Core Team seats (`materialize_core_team`, `core_team_route_plan`, `supersede_core_team_launch_intent`), no single integration point, and `core/` bindings would change request contracts. Live check: all six stalled epics have bound ECP/ESW nodes and active LSA/TPM seats, so no bridge move is pending.
- **`config/examples/fleet.yml` drift fixed in LF-07**: the example had `deepseek-flash … calibrated: true` without the DEC-017 comment; it is now byte-identical to Appendix A (8676 bytes). Anything that installs the live file must use the plan's Appendix A, which now matches.
- **Two `#[allow(clippy::too_many_arguments)]`** were added on `Services::new` and `record_fleet_decision`, both required by the plan's mandated signatures (workspace precedent).
- **Test-only instrumentation**: `crates/kontor-runtime/src/fake.rs` gained `consultation_route_provenance` (records what a launch was told); no production behaviour changed.

## Known pre-existing reds — do not chase

- `kontor-store` schema-v115 migration test (`v115_confirms_exact_local_mutations_with_typed_provenance_and_preserves_history`).
- `scripts/verify-tree.py --mode archive` reports `Cargo.lock` regeneration drift.
- (The plan also lists some `e2e_pilot` session tests as known; they did not fail in this run.)

## How to re-run the implementer's evidence

```sh
export CARGO_TARGET_DIR=/Users/igor/carasent/asma-modules/.worktrees/kontor-settled-turn-evidence/target
cargo test -p kontor-daemon --lib                       # 119 unit tests, incl. fleet + independence
cargo test -p kontor-daemon --test loopback_api          # 473 pass, 1 ignored
cargo test -p kontor-runtime-paseo                       # 126 + 285
cargo test -p kontor-api --test openapi_contract         # 3
```

Reference documents: [`docs/CONFIGURATION.md`](../../docs/CONFIGURATION.md) (operator guide),
[`_docs/architecture/2026-09-24-20-45-architecture-live-fleet-routing.md`](../architecture/2026-09-24-20-45-architecture-live-fleet-routing.md) (decision),
[plan](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/plans/2026-09-24-07-30-plan-kontor-live-fleet-configuration.md),
[ASMA-8255](https://carasent.atlassian.net/browse/ASMA-8255), PR #271.
