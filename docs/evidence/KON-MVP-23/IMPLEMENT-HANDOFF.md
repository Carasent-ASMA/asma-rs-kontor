# KON-MVP-23 / ASMA-7821 — IMPLEMENT handoff

Status: **bounded IMPLEMENT parity correction complete; ready for the remaining QA/audit proofs, not yet QA sign-off**.

## Tree identity

- TSW: `/Users/igor/.paseo/worktrees/0vl4ss0m/kon-mvp-23`
- Submodule: `_tools/asma-rs-kontor`
- Base commit: `3cf8221efb0b6497b1069b526b6960d5072f1127`
- Checkout: local branch `feat/ASMA-7821-kon-mvp-23`, created from the base without Jira mutation because `JIRA_BASE_URL` remains absent.
- Commit: the bounded parity correction changeset containing this handoff.
- Staging: explicit owned paths only.
- `Cargo.lock`: restored byte-for-byte to `HEAD`; it is not part of this change.

Owned paths:

- `crates/kontor-store/migrations/0021_native_memory.sql`
- `crates/kontor-store/src/memory.rs`
- `crates/kontor-store/src/migrations.rs`
- `crates/kontor-store/src/lib.rs`
- `crates/kontor-store/src/backup/restore.rs`
- `crates/kontor-store/tests/schema_v1.rs`
- `crates/kontor-api/src/memory.rs`
- `crates/kontor-api/src/lib.rs`
- `crates/kontor-api/src/openapi.rs`
- `crates/kontor-mcp/src/registry.rs`
- this handoff

## Implemented evidence by acceptance criterion

1. **Typed stale-write conflict:** `MemoryError::RevisionConflict` and expected/current compare-and-swap are in `crates/kontor-store/src/memory.rs:12`, `:123`, `:192`; exercised at `:680`.
2. **At most one current approved revision:** immutable approval facts plus the single `memory_items.current_revision_id` pointer are created in `crates/kontor-store/migrations/0021_native_memory.sql:4` and `:30`; approval updates the pointer and FTS in one transaction at `crates/kontor-store/src/memory.rs:192`.
3. **Default exclusion:** list/search join only the current approved pointer and exclude tombstones/project siblings at `crates/kontor-store/src/memory.rs:300` and `:323`; exercised at `:680`.
4. **Secret scan before persistence/index/export/future embedding:** memory documents are `CanonicalDocument`, whose existing constructor is the repository-wide pre-persistence scanner; actor/item inputs additionally call `reject_sensitive_text` in `crates/kontor-store/src/memory.rs:123` and `:192`. The canary check is at `:747`. No embedding path was added.
5. **Rebuildable FTS5:** the derived virtual table is declared at `crates/kontor-store/migrations/0021_native_memory.sql:104`; full delete/rebuild from approved current native revisions is `crates/kontor-store/src/memory.rs:338`; restore rebuilds it at `crates/kontor-store/src/backup/restore.rs:112`.
6. **Deterministic Context Pack memory binding:** cursor, canonical selection spec, ordered revision IDs/hashes, result hash and bound time are stored by `crates/kontor-store/src/memory.rs:346`; an existing run binding is returned without a new query. The no-requery check is at `:680`.
7. **Backup/restore:** all ledger, pointer, receipt, provenance and import-manifest tables live in the realm SQLite file (`0021_native_memory.sql:4-102`), so the existing SQLite snapshot retains them; restore explicitly discards/rebuilds FTS at `crates/kontor-store/src/backup/restore.rs:112`.
8. **Idempotent legacy import:** manifest uniqueness is `(project_id, source, export_hash)` at `0021_native_memory.sql:83`; preview/apply and `history_unavailable=true` plus `legacy_last_write_wins=true` provenance are at `crates/kontor-store/src/memory.rs:478` and `:492`; exercised at `:747`.
9. **No dual writer:** initial authority is `agentsroom`; native propose/approve/tombstone/purge require `kontor`; import refuses until the durable freeze timestamp exists; switch requires the final imported hash. Schema is `0021_native_memory.sql:94`; flow is `crates/kontor-store/src/memory.rs:474-560`.
10. **No transcript/tool/token auto-memory:** the implementation has no transcript ingestion path. Every new native value enters through explicit `propose_memory_revision` and becomes retrievable only through `approve_memory_revision` (`crates/kontor-store/src/memory.rs:123`, `:192`). Existing session-content rejection remained green in the full store run (`transcript_and_token_deltas_are_rejected`).

Public surfaces implemented and parity-green:

- `/v1` handlers: `crates/kontor-api/src/memory.rs:62-260`; routes: `crates/kontor-api/src/lib.rs:193-230`.
- MCP capability registry (also the stable-JSON CLI command source): starts at `crates/kontor-mcp/src/registry.rs:1491`. Observer: search/history; operator: propose; admin: approve/tombstone/purge/import/cutover.

## Verification ledger

Green:

- `cargo check -p kontor-store`
- `cargo check -p kontor-api -p kontor-mcp -p kontor-cli`
- `cargo test -p kontor-store memory::tests -- --nocapture` — 2 passed
- Full `cargo test -p kontor-store` passed every suite except the schema inventory before that inventory was updated; the failing inventory was then fixed and rerun directly:
  `cargo test -p kontor-store --test schema_v1 the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict` — passed
- `cargo test -p kontor-mcp --lib` — 35 passed after route-template correction
- `cargo fmt --all` applied

Bounded correction gates now green:

- `cargo test -p kontor-tests-contract --test mcp_parity` — 11 passed
- `cargo test -p kontor-store memory::tests` — 2 passed
- `cargo test -p kontor-store --test schema_v1 the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict` — 1 passed
- `cargo test -p kontor-mcp --lib` — 35 passed
- `cargo fmt --all` — completed
- `Cargo.lock` was rewritten by Cargo and restored byte-for-byte to `HEAD`.

Still not run in this bounded correction:
- Full `cargo test -p kontor-store` was not rerun after the schema-inventory-only fix.
- Workspace-wide tests, clippy, daemon loopback API tests, CLI process tests, backup/restore memory-content proof, purge proof, concurrent-thread race proof, and import rollback fault injection were not run/added in this bounded turn.
- No commit, push or PR exists because the TSW is detached and ASMA Jira branch creation is unavailable.

## Exact continuation (same seat)

1. Add focused backup/restore, purge, true concurrent stale-writer, transactional import rollback and HTTP/CLI checks.
2. Run full store/API/MCP/CLI/contract gates and clippy. Inspect `Cargo.lock` immediately afterward and restore it if Cargo rewrites it without a manifest change.

Do not treat this handoff as acceptance evidence EVD-027. It is an honest IMPLEMENT checkpoint with known red gates.
