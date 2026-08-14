# KON-MVP-23 / ASMA-7821 — QA verdict

Date: 2026-08-14
TSW: `wks_0fe677df8a595067` (`.worktrees/0vl4ss0m/kon-mvp-23`)
Frozen submodule: `_tools/asma-rs-kontor`
Commit: `53a2eb99ca710931b47a15a4eefdd7e972b42bec`
Branch: `feat/ASMA-7821-kon-mvp-23`

## Verdict

**NOT-READY-FOR-AUDIT**.

The frozen implementation has useful green coverage, but acceptance is blocked by:

1. `cargo test -p kontor-api` fails its committed OpenAPI snapshot check: 17 unit tests pass and 2/3 OpenAPI tests pass; `the_committed_contract_document_is_the_one_this_crate_serves` fails at `crates/kontor-api/tests/openapi_contract.rs:56` because `crates/kontor-api/contract/openapi.json` is stale.
2. Contract `mcp_mutants` fails `no_tool_can_name_a_credential_an_address_or_a_proxy`: `port` is found in `export_hash` for import-preview, import-apply and cutover-switch. 10/11 tests pass.
3. Required memory-specific backup/restore, purge, true concurrent stale-writer, transactional import rollback, and end-to-end HTTP/CLI parity proofs are absent. Existing generic suites do not prove these memory flows.
4. Mutation checks found two survivors: dropping the tombstone filter and skipping purge deletion both leave the available memory unit suite green.

## Acceptance criteria

1. **FAIL — typed stale-write conflict / never last-write-wins.**
   `MemoryError::RevisionConflict` and CAS paths exist at `crates/kontor-store/src/memory.rs:12`, `:123`, and `:192`; the sequential stale assertion is at `:680-707`. A deliberate comparison-flip mutant was killed. No true concurrent stale-writer race proof exists; the handoff's test is sequential.

2. **PASS — at most one current approved revision.**
   The single `memory_items.current_revision_id` pointer is defined at `migrations/0021_native_memory.sql:4-11`; approval facts and the atomic pointer update are at `:30-37` and `crates/kontor-store/src/memory.rs:192-242`. Historical approval facts remain immutable; retrieval exposes only one current revision.

3. **FAIL — default exclusion evidence incomplete.**
   Current/project/tombstone filters are implemented at `crates/kontor-store/src/memory.rs:300-335`, with the sequential proposal/foreign-project/tombstone assertions at `:680-744`. The tombstone-filter mutation survived, showing the available test does not independently prove search exclusion after a stale/retained index row; no dedicated proposed/superseded/tombstoned/foreign matrix proof exists.

4. **PASS — secret scan before native persistence.**
   Actor/item inputs are scanned at `crates/kontor-store/src/memory.rs:123-133` and `:192-200`; `CanonicalDocument` is the canonical document boundary. The canary is at `:747-791`. Existing export and repository secret tests are green. No embedding path exists in this commit.

5. **FAIL — rebuildable FTS5 proof incomplete.**
   FTS5 is declared at `migrations/0021_native_memory.sql:104`; rebuild-from-approved-current rows is at `crates/kontor-store/src/memory.rs:338-343`; restore invokes rebuild at `crates/kontor-store/src/backup/restore.rs:112-145`. The unit test proves manual FTS deletion/rebuild, but no memory-content backup/restore proof exists.

6. **PASS — deterministic Context Pack binding.**
   Binding fields and persistence are at `crates/kontor-store/src/memory.rs:346-385`; an existing binding returns before a new selection query at `:353-356`. The no-requery assertion is at `:723-737`.

7. **FAIL — backup/restore of memory ledger and derived rebuild.**
   Ledger, pointer, receipts, provenance and manifests are SQLite tables in `migrations/0021_native_memory.sql:4-102`; restore rebuilds FTS at `crates/kontor-store/src/backup/restore.rs:112-145`. Generic backup suites passed, but `rg` found no memory-specific backup/restore-content test. Required proof is missing.

8. **FAIL — idempotent import with legacy provenance and rollback proof.**
   Manifest uniqueness is at `migrations/0021_native_memory.sql:83-92`; preview/apply and provenance are at `crates/kontor-store/src/memory.rs:478-529`; the unit test at `:747-785` proves freeze refusal, idempotent replay, and provenance. No transactional import rollback fault-injection test exists.

9. **FAIL — no-dual-writer cutover evidence incomplete.**
   Initial authority and durable freeze/switch fields are at `migrations/0021_native_memory.sql:94-104`; freeze/import/switch flow is at `crates/kontor-store/src/memory.rs:474-556`. The unit test proves import-before-freeze refusal and hashed switch, but there is no concurrent or first-native-write cutover assertion. Required no-dual-writer proof is therefore incomplete.

10. **PASS — no transcript/tool/token auto-memory path.**
    Native values enter through explicit proposal and approval at `crates/kontor-store/src/memory.rs:123-243`; no transcript ingestion path was added. `crates/kontor-store/tests/event_replay.rs:1219-1324` passed `transcript_and_token_deltas_are_rejected`, and the schema test rejects content-shaped session columns at `crates/kontor-store/tests/schema_v1.rs:1693-1740`.

## Gate results

| Gate | Result |
|---|---:|
| `cargo test -p kontor-store` | PASS — 250 passed, 0 failed; 0 doctests |
| `cargo test -p kontor-api` | FAIL — 18 passed, 1 failed |
| `cargo test -p kontor-mcp` | PASS — 39 passed |
| `cargo test -p kontor-cli` | PASS — 14 passed, including process/version test |
| `cargo test -p kontor-tests-contract --test mcp_parity` | PASS — 11 passed |
| Contract cardinality | PASS — 11 passed |
| Contract guardrails | PASS — 5 passed |
| Contract profiles/teams | PASS — 11 passed |
| Contract runtime adapter | PASS — 52 passed |
| Contract scheduling | PASS — 2 passed |
| Contract `mcp_mutants` | FAIL — 10 passed, 1 failed |
| All individually run contract targets | FAIL — 102 passed, 1 failed |
| `cargo test -p kontor-daemon --test loopback_api` | PASS — 105 passed |
| `cargo test -p kontor-daemon --test mcp_journey` | PASS — 2 passed |
| `cargo test --workspace` | FAIL — stopped at the same API OpenAPI failure |
| `cargo clippy -p kontor-store -p kontor-api -p kontor-mcp -p kontor-cli --all-targets -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

Generic backup export/snapshot tests included in the store run passed (7 and 9 respectively), but neither contains a native-memory content round-trip proof. The MCP parity and CLI process gates are green but do not exercise the native-memory HTTP/CLI data path end to end.

## Mutation evidence

Baseline in an isolated worktree: `cargo test -p kontor-store memory::tests -- --nocapture` — 2 passed.

| Seeded defect in `crates/kontor-store/src/memory.rs` | Result |
|---|---|
| Flip stale CAS comparison (`!=` → `==`) | KILLED at the stale assertion (`:692`) |
| Skip approval FTS insertion | KILLED at search count assertion (`:716`) |
| Remove freeze-before-import guard | KILLED at freeze refusal assertion (`:761`) |
| Remove idempotent-import early return | KILLED by duplicate `memory_items` constraint (`:776`) |
| Drop `!r.tombstoned` search filter | **SURVIVED**; 2 memory tests remained green |
| Skip purge revision deletion | **SURVIVED**; 2 memory tests remained green |

No concrete KON-MVP-20 mutant-ID ledger was present in the supplied committee-memory ruling or IMPLEMENT handoff; the table records direct AC-focused seeds against the frozen commit. The isolated mutation worktree was removed and no mutant remains in the committed checkout.

## Evidence integrity and tree state

- Handoff read from `docs/evidence/KON-MVP-23/IMPLEMENT-HANDOFF.md` inside the frozen submodule.
- Governing ruling read from `/Users/igor/kon-mvp-20-scratch/evidence/committee-memory/2026-08-13-judge-ruling-BLOCK_1_0_WITH_KON_MVP_23.md`.
- Every acceptance citation in the handoff was checked against the cited file/line in commit `53a2eb99`; cited symbols/routes exist.
- Submodule HEAD: `53a2eb99ca710931b47a15a4eefdd7e972b42bec`.
- `Cargo.lock` is byte-identical to `HEAD` (`03c9e37793cf1de5b1e385f47991784f4c725b35`). Cargo rewrote it during uncapped test runs; it was restored before this verdict.
- Before writing this QA artifact, root and submodule trees were clean. After writing, the only intentional working-tree change is this uncommitted `QA.md`; no production code or lockfile was changed.

## Bounded correction handoff

Return to the same Implement seat for only these corrections: refresh the committed OpenAPI contract and resolve the MCP schema-vocabulary failure; add the memory-specific backup/restore, purge, true concurrent stale-writer, transactional import rollback and HTTP/CLI parity proofs; strengthen tests to kill the tombstone-filter and purge-deletion survivors. Do not expand scope beyond those blockers.

## Re-QA continuation guard — 2026-08-14

Status: `compaction_pending`; no re-QA verdict is issued in this bounded continuation.

The same QA seat remains active and was read back as agent `88301c86-dbe2-484f-9295-f4be67e7ab81`, workspace `wks_0fe677df8a595067`, native session `01a000e5-d7f8-76f3-b241-2da27a8da71b`. The runtime advertises session persistence and rewind, but exposes no in-place compaction operation or compaction-attestation/read-back surface. Per the context-boundary guard, the corrected-checkpoint gates were not started in this continuation and no replacement seat was created.

## Replacement QA re-run — 2026-08-14

Replacement seat: prior QA seat `88301c86-dbe2-484f-9295-f4be67e7ab81` was verified closed because its native runtime could not attest compaction. This re-run used the same canonical TSW and preserved the prior evidence and both foreign KON-18 artifact directories.

Frozen identity verified before testing:

- Submodule HEAD: `5d3600cc5b291587d6a4c07c7a4524b270552af1` (`feat/ASMA-7821-kon-mvp-23`).
- Implement handoff, design/acceptance plan, ticket body, and prior QA including its `compaction_pending` record were read.
- No production source, migration, OpenAPI, MCP, or test file was modified by QA.

### Verdict

**PASS** — all KON-MVP-23 acceptance evidence is complete, all prior blockers are cleared, and no unresolved P0/P1 remains.

### Acceptance gates rerun

| Command | Result |
|---|---|
| `cargo test --workspace --locked` | PASS, exit 0; full workspace completed with no failed tests |
| `cargo test -p kontor-api --test openapi_contract --locked` | PASS, 3/3 |
| `cargo test -p kontor-tests-contract --test mcp_mutants --locked` | PASS, 11/11; forbidden MCP port/address/proxy vocabulary guard passes |
| `cargo test -p kontor-tests-contract --test mcp_parity --locked` | PASS, 11/11 |
| `cargo test -p kontor-store memory::tests --locked -- --nocapture` | PASS, 4/4 memory tests |
| `cargo test -p kontor-store --test backup_snapshot memory_ledger_and_import_evidence_restore_while_fts_is_rebuilt --locked` | PASS, 1/1 |
| `cargo test -p kontor-cli --test memory_parity --locked` | PASS, 1/1 HTTP/CLI native-memory parity |
| `cargo test -p kontor-store --test repository_roundtrip valid_id_from_another_project_never_resolves --locked` | PASS, 1/1 project isolation |
| `cargo test -p kontor-context --test resolution --locked` | PASS, 15/15 deterministic Context Pack resolution/snapshot tests |
| `cargo test -p kontor-store memory::tests::cutover_is_frozen_hashed_transactional_and_idempotent --locked` | PASS, 1/1 no-dual-writer cutover |
| `cargo clippy -p kontor-store -p kontor-api -p kontor-mcp -p kontor-cli --all-targets --locked -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

The focused memory run explicitly passed:

- `concurrent_approvers_get_one_commit_and_one_typed_conflict` — one commit, one typed `RevisionConflict`.
- `ledger_conflicts_filters_rebuilds_and_freezes_context` — CAS, proposed/superseded/tombstone filtering, FTS rebuild and frozen Context Pack binding.
- `purge_removes_payload_approval_and_index_but_keeps_receipts` — purge deletion with durable receipts.
- `cutover_is_frozen_hashed_transactional_and_idempotent` — freeze-before-import, hashed switch, transactional import and idempotent replay.
- `memory_ledger_and_import_evidence_restore_while_fts_is_rebuilt` — memory ledger, pointers, approvals, provenance, receipts and import manifest survive restore; FTS is rebuilt.
- `native_memory_http_and_cli_share_realm_revision_and_cursor` — HTTP and stable-JSON CLI agree on realm, revision and cursor.

### Mutation-kill rerun

Both prior survivors were reseeded in a disposable copy of the frozen submodule; the canonical checkout was not modified:

| Seeded defect | Result |
|---|---|
| Remove `!r.tombstoned` from `search_memory` | KILLED — `ledger_conflicts_filters_rebuilds_and_freezes_context` failed at the stale-index tombstone assertion |
| Remove `DELETE FROM memory_revisions` from `purge_memory` | KILLED — `purge_removes_payload_approval_and_index_but_keeps_receipts` failed when purged history was read |

### Cargo.lock residue and final restoration

All Cargo commands finished before restoration. The pre-restoration `Cargo.lock` diff was inspected in full and contained only generated dependency-resolution residue: registry package additions (`data-encoding`, `sha1`, `tokio-tungstenite`, `tungstenite`) and workspace dependency-list changes. No intentional source, migration, contract, or application-content change was present.

- Before restore: working blob `9324c4148a6857c23f63d7bd5b53222779340f25`; frozen blob `03c9e37793cf1de5b1e385f47991784f4c725b35`; `git diff --exit-code -- Cargo.lock` failed as expected.
- Final repository action: `git restore --source=5d3600cc5b291587d6a4c07c7a4524b270552af1 -- Cargo.lock`.
- After restore: working blob `03c9e37793cf1de5b1e385f47991784f4c725b35`; `cmp` returned 0; `git diff --exit-code -- Cargo.lock` returned 0.
- Final submodule status shows no modified tracked file; only the pre-existing/untracked evidence paths remain: `docs/evidence/KON-MVP-18/run-40870492d74e3b3a/`, `docs/evidence/KON-MVP-18/run-97d55adc7ea6a9ef/`, and this `docs/evidence/KON-MVP-23/QA.md`.

No Jira, AgentsRoom, audit, integration, push, or foreign KON-18 artifact was mutated.

## Re-QA — 2026-08-14 — commit `498e83a`

Same replacement QA seat and canonical TSW: `wks_0fe677df8a595067`. Frozen commit verified as `498e83a72fc4bf42c18d62c79a96e68b1f9207ee` on `feat/ASMA-7821-kon-mvp-23`.

### Verdict

**READY-FOR-AUDIT** — the audit’s sole blocker is cleared and the prior PASS surface remains green. No residual blocker.

### Narrow fix verification

- `git diff 5d3600cc5b291587d6a4c07c7a4524b270552af1..498e83a72fc4bf42c18d62c79a96e68b1f9207ee --name-status` reports only `M crates/kontor-cli/Cargo.toml`.
- The complete commit diff is one deletion: `kontor-runtime = { path = "../kontor-runtime", version = "=0.1.0" }` from `[dev-dependencies]`.
- No other dependency was removed; no `crates/kontor-cli/**/*.rs` source or test file changed.
- `rg` confirms `crates/kontor-cli/Cargo.toml` no longer declares `kontor-runtime`.

### Gates on `498e83a`

| Command | Result |
|---|---|
| `cargo check -p kontor-cli` | PASS, exit 0 |
| `cargo test -p kontor-cli` | PASS, 15 passed, 0 failed |
| `cargo test -p kontor-tests-contract --test mcp_parity` | PASS, 11 passed, 0 failed |
| `cargo test --workspace` | PASS, 102 suites; 1,231 passed, 0 failed, 7 ignored |
| `cargo clippy -p kontor-cli --all-targets -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

The workspace rerun includes the previously accepted OpenAPI, MCP vocabulary/contract, native-memory backup/restore, purge, concurrent CAS, rollback, HTTP/CLI parity, project/realm isolation, deterministic Context Pack, and no-dual-writer cutover tests. The only implementation delta from the prior PASS commit is the unused CLI dev-dependency removal.

### Cargo.lock and tree integrity

The checked-in lock remained the frozen blob `03c9e37793cf1de5b1e385f47991784f4c725b35`. During the unlocked Cargo gates, Cargo generated a temporary resolver diff containing only the four registry packages `data-encoding`, `sha1`, `tokio-tungstenite`, and `tungstenite`, plus workspace dependency-list changes; no source or intentional application content was present. It was restored from the new frozen HEAD after all Cargo commands:

```text
git restore --source=498e83a72fc4bf42c18d62c79a96e68b1f9207ee -- Cargo.lock
HEAD: 498e83a72fc4bf42c18d62c79a96e68b1f9207ee
Cargo.lock HEAD blob: 03c9e37793cf1de5b1e385f47991784f4c725b35
Cargo.lock work blob: 03c9e37793cf1de5b1e385f47991784f4c725b35
cmp: 0
git diff --exit-code -- Cargo.lock: 0
```

Final owned-path status is clean for tracked files. Only untracked evidence byproducts remain and were preserved: `docs/evidence/KON-MVP-23/QA.md`, `docs/evidence/KON-MVP-23/AUDIT.md`, and the foreign `docs/evidence/KON-MVP-18/run-40870492d74e3b3a/`, `run-89a688943e1099bf/`, and `run-97d55adc7ea6a9ef/` directories. No code was changed or staged; no new seat was created.
