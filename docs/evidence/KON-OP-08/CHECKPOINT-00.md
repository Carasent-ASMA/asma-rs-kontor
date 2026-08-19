# KON-OP-08 checkpoint 0 — integrated baseline and frozen inventories

Date: 2026-08-19
Status: implemented; gates recorded below
Against: `docs/evidence/KON-OP-08/2026-08-19-11-39-architecture-kontor-operational-control-surfaces.md`
(checkpoint 0)

## Why this checkpoint existed

The ticket worktree was checked out at OP-03 (`33e07a2`). OP-08 composes OP-04
through OP-07, so nothing could be built until that baseline was real and green
in one tree.

## What the baseline actually was

`origin/master` already carried OP-03, OP-04, OP-05, OP-06 and the accepted
correctives (`fix/ASMA-7869-compose-consultation-completion-services`,
three `fix/ASMA-7874-*`, `fix/ASMA-7876-compose-jira-reconciliation-and-operational-gaps`,
`fix/ASMA-7869-replay-undelivered-handoff`).

**OP-07's authority slice was not in master.** It sits on
`feat/ASMA-7876-kontor-jira-policy-cutover`, released 🟢 at `d3ecbec`, cut from
`3d2dfca` — a mid-OP-04 base, before OP-05/OP-06 existed. Integrating it was
therefore a real merge, not a fast-forward, and the numbering collision the
architecture handoff anticipated was concrete:

| Concern | Resolution |
| --- | --- |
| OP-07 shipped `0032_project_subject_authority.sql`; master already had `0032_core_team_revisions.sql` | renumbered to `0041_project_subject_authority.sql`, its terminal `PRAGMA user_version` moved 32 → 41 |
| `SCHEMA_VERSION` 40 (master) vs 32 (OP-07) | 41 |
| OP-07's `MIGRATIONS_THROUGH_V31` fixture chain described a lineage that no longer exists (its `0030`/`0031` now name other migrations) | dropped; added `migrate_through_v40`, so "pre-authority" means the real pre-authority state |
| OP-07's two migration tests were named for `0032` and a v31 realm | renamed to `migration_0041_*`, fixtures rebuilt on the v40 chain |
| `crates/kontor-api/src/memory.rs` — master moved to the crate's own `Json` extractor | kept `crate::body::Json`, added OP-07's `AuthoritySubject` import |
| `mcp_parity` operation counts (master 120/121, OP-07 117/118) | superseded by the inventory below |

Six conflicts in total; the other two were an import superset
(`crates/kontor-daemon/tests/loopback_api.rs`) and the generated console types.

`crates/kontor-api/contract/openapi.json` auto-merged to exactly what the router
serves — `the_committed_contract_document_is_the_one_this_crate_serves` passes
untouched — so only `apps/console/src/api/schema.d.ts` needed regenerating.

The merged surface retains every slice's tools: `kontor_subject_authority_get`
and `_attest` (OP-07), `kontor_advisor_run_invoke` / `kontor_committee_run_settle`
(OP-05), `kontor_completion_advance` (OP-06), `kontor_core_team_apply` /
`kontor_quick_session_ensure` (OP-04).

## One real defect the renumber exposed

`apply_pending` has a special convergence branch for the operational-hardening
lineage (a durable v34/v35 file with the escalation brief but no consultation
tables). It applied a **hardcoded index list** ending at `MIGRATIONS[39]`, i.e.
`0040_advisor_advice.sql`.

That was exactly right while `SCHEMA_VERSION` was 40 and silently wrong the
moment a migration was appended: the branch left `user_version` at 40 while the
binary claimed 41, and `verify_applied` refused the open with
`StoreError::Pragma { pragma: "user_version" }`.
`the_operational_hardening_v35_lineage_converges_without_losing_its_receipt`
caught it.

The fix is not "add `MIGRATIONS[40]` to the list" — that reintroduces the same
trap for the next migration. The branch now iterates every generation from the
first one this lineage is missing, skipping the single generation it already has:

```rust
for (index, migration) in MIGRATIONS.iter().enumerate().skip(CONSULTATION_GENERATION) {
    if index == ESCALATION_GENERATION {
        continue;
    }
    transaction.execute_batch(migration)?;
}
```

Both indices are named next to the array they index. Neither needs its own
assertion: a wrong `ESCALATION_GENERATION` re-applies the escalation script and
fails on a duplicate object, and a wrong `CONSULTATION_GENERATION` leaves the
consultation tables missing — the lineage test fails either way.

Mutation status: observed naturally. The test was red under the original
hardcoded list and green after the change, which is the proof this branch needed.

## The two frozen inventories

Both are generated and pinned. Neither duplicates a production registry: each is
rendered from the live source of truth and compared, so drift is a reviewable
diff rather than a surprise.

- **`tests/contract/fixtures/v1-operation-inventory.txt`** — every public `/v1`
  operation joined to the tool that reaches it, with tier and kind, or to the
  allowlist entry that excuses it. Rendered from
  `kontor_api::openapi::document()` and `kontor_mcp::REGISTRY` — the same two
  sides the parity oracle compares. Regenerate with `KONTOR_UPDATE_CONTRACT=1`.

  This replaces `the_snapshot_canary_holds_at_this_base`, whose two
  hand-maintained counts said only that *something* moved and had to be
  re-guessed on every change. The allowlist-is-still-the-reviewed-pair assertion
  is kept as its own test, because that rule is about omissions being reviewed
  rather than about a count.

- **`_tools/asma-cli/tests/unit/fixtures/asma-invocation-modes.txt`** — 62 leaf
  invocation modes walked from the live Click tree. Regenerate with
  `ASMA_UPDATE_MODE_INVENTORY=1`. This is the list checkpoint 4 maps dispositions
  against.

## One finding against the accepted disposition table

The handoff's bounded-ASMA-compatibility table names
**`asma jira transition-list`** as "deprecated read-only forwarder over native
Jira observation". **That mode does not exist.** The `jira` group has exactly
two leaves, `import` and `sync`, neither hidden; `src/asma_cli/jira_transitions.py`
is an internal helper, not a command.

It is not being created: the same handoff forbids new ASMA command families, so
inventing a mode in order to deprecate it would be the wrong direction. Its
checkpoint-4 disposition is *absent — nothing to forward*, and the transition
read stays reachable through `asma doctor jira`.

Everything else in that table reconciles with the inventory. Verified present:
`asma jira sync --request-json`, `asma jira import`,
`asma prompt --write-ai-interpretation`, and `--transition` on `asma git commit`,
`asma acp`, `asma git checkout` and `asma git branch create`. Verified *absent*,
as the handoff requires: `asma fleet probe`.

## Deliberately not in this checkpoint

The handoff asks checkpoint 0 to end on a red parity-oracle test naming the
missing OP-08 routes. It also requires the workspace green before moving on, and
every checkpoint in this repository lands green with `#[ignore]` reserved for
live-daemon tests. Committing a knowingly-red test would break bisect for every
later checkpoint.

The red demonstration is therefore recorded as evidence and the assertion lands
with the checkpoint-1 work that turns it green. The gap it will name is already
frozen, mechanically, in the operation inventory: no epic Jira preview/apply, no
backlog import preview/apply/readback, no subject-scoped cutover preview/apply,
and no integration or closeout receipt operations.

## Gates

Run on the integrated tree at the state this checkpoint commits.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test -p kontor-daemon --all-targets` | pass — 208 tests (`loopback_api` 175 in 194s) |
| `cargo test -p kontor-store --all-targets` | pass — 306 tests |
| `cargo test --workspace --all-targets` minus daemon/store/e2e | pass |
| `cargo test -p kontor-tests-e2e --all-targets` | pass |
| `cargo deny check` | pass — advisories, bans, licenses, sources ok |
| `pnpm --filter kontor-console verify:api` | pass — committed types match the contract |
| `pnpm -r typecheck` | pass |
| `pnpm -r test` | pass — 278 tests |
| ASMA CLI, scoped to the command tree | pass — mode inventory + entrypoints |

The Rust suite is run in per-package chunks rather than one invocation. That is a
harness limit, not a scope reduction: ~643s of pure test time across 86 binaries
(`loopback_api` alone is 246s of it) exceeds a single call's ceiling, and a
backgrounded run was terminated mid-binary three times without a usable exit
code. The chunks are exhaustive over the workspace with `--all-targets`, and each
reports its own exit status.

The **complete** ASMA CLI unit suite (~1900 serial tests, no `pytest-xdist`) is
not a checkpoint-0 gate. This checkpoint adds one test file and one fixture to
that package; the full suite is the gate for checkpoint 4, where the forwarders
actually change `asma-cli` behavior.
