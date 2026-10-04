# ASMA-8278 module master-conflict repair (2026-10-04)

Author: DeepSeek implementation child `e9d2abec-b9b7-4a13-9742-092ce2032eaa`
(OpenCode, `deepseek/deepseek-flash`, max, Build). Root: `d70d8a8f`. Paseo-direct
surface; the Kontor publication attestation refusal is a logged advisory, not a
control-plane prerequisite.

## What this is

The module release-integration candidate had passed review against the then-current
master `1d7a71dd` (ASMA-8187). Master then advanced to
`abe990acf754f66be66c3f5564077bb5d3333288` (ASMA-8155 experience memory /
analogue recall, PR #291) while PR #292 sat CONFLICTING. This is the resolution of
that integration in the module worktree, executed by the child that owns module
source writes for this assignment.

- Branch: `feat/ASMA-8278-shared-orchestration-release-integration`
- PR: <https://github.com/Carasent-ASMA/asma-rs-kontor/pull/292>
- Before: `c76351f6` (tree `ee94dc30`)
- Master integrated: `abe990ac` (tree `47615df2`)
- Merge base: `1d7a71dd`
- Integration commit: `ced6db99` (merge; parents `c76351f6` + `abe990ac`)

## The integration

The exact released master was integrated with a reversible no-commit merge
(`git merge --no-commit --no-ff abe990ac`), then every collision was resolved
semantically and the merge was concluded through `asma git commit --push`:

```
asma git commit --repo external-repo::<worktree> \
  -m "feat(orchestration): Integrate released master abe990ac for ASMA-8278" --push
```

Ten paths conflicted; the full prediction is in
`receipts/preflight-01-merge-tree-prediction.txt` and the per-path resolution is
in `receipts/conflict-inventory.json`. The load-bearing rules:

- Both bodies of behaviour are retained: the released ASMA-8155 memory /
  experience source and all ASMA-8278 planning-pair, fleet-policy/activation and
  attestation behaviour. No side was replaced wholesale.
- Released migrations `0123_experience_memory_projection.sql` and
  `0124_memory_projection_rebuild_receipts.sql` are byte-identical to `abe990`
  and were not edited.
- The six unmerged ASMA-8278 draft migrations moved `0123-0128` → `0125-0130`,
  with rebuild table names, `PRAGMA user_version`, registration comments, test
  names and historical fixtures following. `SCHEMA_VERSION` is now `130`, and
  the attestation-ledger cutoffs in `backup/export.rs` moved `127/128` →
  `129/130`.
- Generated artefacts (OpenAPI contract, console TypeScript types) were
  regenerated from the merged source rather than hand-merged.
- Master's eight new MCP memory tools were preserved under the merged
  `ToolSpec.execution: Execution::Http { method, path }` shape; no route or tool
  was dropped.
- Two master fixtures that reconstruct a v119 database by dropping a fixed
  table list were corrected to also drop the additive tables from `0127-0130`
  so the real migration chain can replay; no assertion was weakened.

## Verification evidence (attribution corrected — see Errata)

All commands ran with `CARGO_BUILD_JOBS=2`, the shared reusable target
`/Users/igor/.local/state/asma/epics/ASMA-8278/prepared-diagnostic-20261004-0029/cargo-target`,
`--locked`, and `--offline` for the combined run.

| Gate | Result | Raw log |
| --- | --- | --- |
| `cargo fmt --all --check` | clean; no raw receipt captured at the time (E1) | — |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | rc=0 at the time; receipt carries no commit pin (E1) | `receipts/clippy.log` |
| Focused `kontor-store` (all targets) | **HISTORICAL RED before the fixture fix**: exit 101, two failed v119-replay targets (`receipts/focused-store.meta`); superseded by the pinned full suite | `receipts/focused-store.log` |
| Focused `kontor-mcp` + `kontor-api` + `kontor-tests-contract` | green, no commit pin (E2) | `receipts/focused-mcp-api-contract.log` |
| Focused daemon memory/projection/census | green, no commit pin (E2) | `receipts/focused-daemon-memory.log` |
| Focused `loopback_api` | 570 passed, 1 ignored, no commit pin (E2) | `receipts/focused-daemon-loopback.log` |
| Console `verify:api` and `typecheck` | clean; no raw receipt captured at the time (E1) | — |
| Combined suite: prior 13 packages + `kontor-memory-cognee` | exit 0, pinned to `ced6db99` | `receipts/full-combined.log` |

Combined suite at `ced6db99` (tree `ced6db99^{tree}`), command:

```
cargo test --locked --offline --no-fail-fast \
  -p kontor-core -p kontor-store -p kontor-runtime -p kontor-daemon \
  -p kontor-fleet -p kontor-fleet-activation -p kontor-cli -p kontor-api \
  -p kontor-runtime-paseo -p kontor-mcp -p kontor-runtime-ao \
  -p kontor-runtime-codex -p kontor-tests-contract -p kontor-memory-cognee
```

Honest counts: **122 test-result suites, 108 target binaries, 2943 passed,
0 failed, 9 ignored**. The nine ignored are the same named live-environment
tests as before (disposable AO daemon, authenticated Codex accounts, live Paseo
daemon) — see the full log; nothing was hidden and no ignored test was counted
as a pass. This is new-candidate evidence; the old review PASS does not certify
it.

## Errata (2026-10-04, correction after independent review)

These corrections were made under the ASMA-8278 correction dispatch; the new
packet `docs/evidence/ASMA-8278/released-124-upgrade-correction-20261004/` binds
them with fresh raw receipts. Every other artifact in this packet is unchanged.

- **M2-a (mislabeled gate).** This file previously listed the focused
  `kontor-store` run as "green". The raw receipt records `exit=101`: two targets
  failed at the time it ran ("table planning_pair_placements already exists" in
  the v119-replay fixtures, before the fixture fix). The table above now labels
  that run historical red; store coverage comes from the pinned full suite at
  `ced6db99`. The raw log and `.meta` are untouched.
- **M2-b (over-claimed pin).** The former heading "Verification at the
  committed head" was wrong for the focused and clippy runs: they predate the
  `ced6db99` commit and carry no head pin. Only the combined suite is pinned to
  a commit; the rest are labeled unpinned (E1/E2).
- **M2-c (citation).** `receipts/conflict-inventory.json` credited
  "schema_v1 v123 upgrade test green" and "69/69 green" as the migrations
  verification. The honest attribution is the focused schema_v1 run and the
  pinned full log, and neither is a released-124 qualification.
- **M1 (missing released-124 check).** No real released-124 upgrade fixture
  existed. The correction adds
  `released_124_upgrade_preserves_realm_bindings_and_memory_content`, which
  applies the exact `MIGRATIONS[..124]` chain, seeds a Realm, a permanent
  binding and representative 0123/0124 memory/experience/projection/recall/
  rebuild rows, then opens through `SqliteStore` to 130 and asserts identity,
  content, permanence and guard preservation. It passed immediately: it is a
  required regression net, not evidence of a production bug. Details and raw
  logs are in the correction packet; migrations 0115-0124 are verified
  byte-identical to `abe990acf754f66be66c3f5564077bb5d3333288`.
- **Self-reference note.** `manifest.json` in this packet describes the bytes
  at `d0d27332`, so the hashes it lists for this `README.md` and
  `receipts/conflict-inventory.json` no longer match those two intentionally
  corrected files. The original manifest is preserved unchanged and is not
  rewritten; the correction packet carries the old/new hash inventory.

## Preservation

- The six machine-local adapter groups (`.agents/`, `.asma/`, `.cursor/`,
  `.dsh/`, `AGENTS.md`, `CLAUDE.md`) remain untracked and are not part of any
  commit; `--include-untracked` was never used.
- Prior evidence artifacts were copied, not rewritten; the only intentional
  byte changes are this `README.md` and `receipts/conflict-inventory.json`,
  recorded in the Errata above.
- Mutation targets `crates/kontor-fleet/src/allocation.rs`,
  `crates/kontor-runtime/.../commands/contribution.rs` and
  `.../commands/recovery.rs` remain byte-equal to the prior reviewed manifest
  (`receipts/mutation-targets.json`); the root `_tools/ai-orchestration/tpm_supervision.py`
  target is outside this module checkout and untouched.

## Boundaries

Child is implement-only: this evidence does not self-certify PASS, does not move
Jira, does not merge PR #292, does not adopt the release, and does not activate
live capabilities. Root owns revised independent review, merge gates, released
adoption and worktree synchronization.
