# KON-MVP-23 / ASMA-7821 — Audit

Date: 2026-08-14  
TSW: `wks_0fe677df8a595067` (`.worktrees/0vl4ss0m/kon-mvp-23`)  
Frozen submodule: `_tools/asma-rs-kontor`  
Frozen commit: `5d3600cc5b291587d6a4c07c7a4524b270552af1`  

## Verdict

**AUDITED_FALSE**

The native-memory implementation and acceptance evidence are sound, but the
frozen branch adds an unjustified manifest dependency:

- `crates/kontor-cli/Cargo.toml:36` adds `kontor-runtime` as a dev-dependency.
- No file under `crates/kontor-cli/**/*.rs` references `kontor-runtime` or
  `kontor_runtime`; the new parity test uses `kontor-api`, `kontor-core`,
  `kontor-daemon`, `kontor-store`, `axum`, `reqwest`, `tempfile`, and `tokio`.

Remove that dependency or provide a concrete use and rerun the integrated
dependency/license gates. This is the only KON-23 contract-scope defect found.

## Contract audit

| Area | Result | Evidence |
|---|---|---|
| Immutable revisions, provenance, supersession, approval and CAS | PASS | `crates/kontor-store/migrations/0021_native_memory.sql:4-37,106-115`; `crates/kontor-store/src/memory.rs:131-251`; concurrent test `memory.rs:764-812` |
| One current approved revision and default retrieval filtering | PASS | Pointer/approval schema above; `memory.rs:253-347`; matrix assertions `memory.rs:704-760` |
| Tombstone and purge semantics, durable receipts | PASS | `memory.rs:407-483`; `memory.rs:814-862`; stale-index tombstone assertion at `memory.rs:753-760` |
| Realm/project isolation | PASS | `crates/kontor-store/tests/repository_roundtrip.rs` test `a_valid_id_from_another_project_never_resolves`; direct cached binary: 1 passed |
| Rebuildable FTS projection | PASS | `memory.rs:349-355`; `crates/kontor-store/src/backup/restore.rs:123-145`; memory snapshot test `backup_snapshot.rs:642-747` |
| Deterministic frozen Context Pack memory binding | PASS | `memory.rs:357-397`; no-requery assertion `memory.rs:734-748`; Context Pack resolution binary: 15 passed |
| Backup/export/restore | PASS | `backup_snapshot.rs:642-747`; direct cached binary: 1 passed; generic snapshot suites are also covered by the QA record |
| Idempotent AgentsRoom import and no-dual-writer cutover | PASS | `memory.rs:485-567`; transactional rollback/idempotency/first-native-write assertions `memory.rs:865-965` |
| Secret and transcript/token boundaries | PASS | Secret rejection `memory.rs:140-141,208`; canary `memory.rs:959-963`; existing `event_replay.rs:1219-1324` rejection suite; no transcript ingestion path |
| API/CLI/MCP parity | PASS | API routes `crates/kontor-api/src/lib.rs:193-227`; handlers `crates/kontor-api/src/memory.rs:67-359`; CLI parity test `crates/kontor-cli/tests/memory_parity.rs:36-191`; MCP registry `crates/kontor-mcp/src/registry.rs:1491-1800` |
| OpenAPI and forbidden vocabulary guards | PASS | Cached frozen binaries: OpenAPI 3/3; `mcp_mutants` 11/11; `mcp_parity` 11/11; loopback contract 1/1 |
| Mutation kills | PASS | QA durable mutation ledger in `docs/evidence/KON-MVP-23/QA.md` (replacement re-run): tombstone-filter mutant KILLED and purge-deletion mutant KILLED; corresponding assertions are present at `memory.rs:753-760` and `memory.rs:835-861` |

## Independent execution

The cached test binaries built from the frozen checkout passed:

- native memory unit tests: 4/4;
- memory backup/restore: 1/1;
- OpenAPI contract: 3/3;
- MCP forbidden-vocabulary mutants: 11/11;
- MCP parity: 11/11;
- native-memory HTTP/CLI parity: 1/1;
- Context Pack resolution: 15/15;
- schema inventory: 35/35;
- project isolation: 1/1;
- loopback contract: 1/1;
- MCP journey: 2/2.

The durable evidence reviewed was `IMPLEMENT-HANDOFF.md` and the replacement
QA PASS section of `QA.md` in this directory. No acceptance mutant survived in
the supplied QA re-run.

## Lock provenance and shipment gate

`Cargo.lock` is intentionally not owned by KON-MVP-23/KON-MVP-25. KON-MVP-20
owns the one final regeneration after both branches and migration renumbering
merge. A fresh throwaway checkout of the shared pre-ticket base
`3cf8221efb0b6497b1069b526b6960d5072f1127` has the same
`cargo metadata --format-version=1 --locked --offline` stale-lock refusal.
Therefore this is a pre-existing integration/shipment prerequisite, not a
KON-23 memory-contract defect.

In this standalone checkout, `cargo test --workspace --locked` and
`cargo metadata --format-version=1 --locked --offline` stop at that expected
lock refusal before executing Cargo’s test graph. The frozen lock blob is still
verified exactly:

```text
Cargo.lock blob: 03c9e37793cf1de5b1e385f47991784f4c725b35
HEAD:Cargo.lock: 03c9e37793cf1de5b1e385f47991784f4c725b35
```

Before shipment, the integrated archive must regenerate `Cargo.lock` once and
pass the locked workspace tests, locked clippy, license gate, and the complete
acceptance/contract suite. The current false verdict is caused by the unused
dependency above; the stale lock is separately a required integration gate.

## Tree and evidence integrity

- Submodule `HEAD` is exactly `5d3600cc5b291587d6a4c07c7a4524b270552af1` on
  `feat/ASMA-7821-kon-mvp-23`.
- Tracked production files and the frozen lockfile have no worktree diff.
- The superproject gitlink remains at its pre-integration base
  `3cf8221efb0b6497b1069b526b6960d5072f1127`; no integration or push was done.
- The only submodule worktree changes are the committed/untracked ticket
  evidence and the pre-existing untracked KON-18 directories
  `run-40870492d74e3b3a/` and `run-97d55adc7ea6a9ef/`. They were not changed,
  deleted, or incorporated into this audit.
- No Jira, AgentsRoom, production source, migration, OpenAPI, MCP, or lockfile
  mutation remains after audit cleanup.

## Re-audit — 2026-08-14 — commit `498e83a`

### Verdict

**AUDITED_TRUE**

The sole prior audit blocker is resolved. The committed tree at
`498e83a72fc4bf42c18d62c79a96e68b1f9207ee` contains exactly one change from
the prior audited commit `5d3600cc5b291587d6a4c07c7a4524b270552af1`:

```text
M crates/kontor-cli/Cargo.toml
1 deletion: kontor-runtime dev-dependency
```

`crates/kontor-cli/Cargo.toml` no longer declares `kontor-runtime`; no CLI
source/test file, other dependency, lockfile, or production file changed.
The code graph reports no changed code symbols, as expected for this manifest
only correction.

### Per-area acceptance checklist

| Area | Result | Evidence re-confirmed on `498e83a` |
|---|---|---|
| 1. Immutable revisions, provenance, approval, CAS and stale-write conflict | PASS | `crates/kontor-store/migrations/0021_native_memory.sql:4-37,106-115`; `crates/kontor-store/src/memory.rs:131-251,764-812`; workspace memory tests passed |
| 2. One current approved revision and proposed/superseded/foreign exclusion | PASS | `crates/kontor-store/src/memory.rs:253-347,704-760`; workspace repository and memory tests passed |
| 3. Tombstone, purge, supersession and durable receipts | PASS | `memory.rs:407-483,814-862`; former tombstone and purge mutants remain killed per QA re-run |
| 4. Secret scan before persistence/index/export and no transcript/token auto-memory | PASS | `memory.rs:140-141,208,959-963`; `crates/kontor-store/tests/event_replay.rs:1219-1324` passed |
| 5. Rebuildable FTS5 projection | PASS | `memory.rs:349-355`; `crates/kontor-store/src/backup/restore.rs:123-145`; memory backup proof passed |
| 6. Deterministic frozen Context Pack revision IDs/hashes | PASS | `memory.rs:357-397,734-748`; `crates/kontor-context/tests/resolution.rs` 15/15 passed |
| 7. Backup/export/restore of ledger, provenance, approvals, receipts and manifest | PASS | `crates/kontor-store/tests/backup_snapshot.rs:642-747`; 10/10 backup snapshot tests and 7/7 backup export tests passed |
| 8. Idempotent AgentsRoom import, legacy provenance and transactional rollback | PASS | `memory.rs:485-567,865-965`; focused rollback/idempotency proof passed |
| 9. No-dual-writer freeze/import/hash cutover and first native write | PASS | `memory.rs:542-567,865-965`; focused cutover proof passed |
| 10. API/CLI/MCP parity, OpenAPI and forbidden-vocabulary boundaries | PASS | API routes `crates/kontor-api/src/lib.rs:193-227`; CLI parity; OpenAPI 3/3; MCP parity 11/11; MCP mutants 11/11; loopback contract 105/105 |

No hidden scope reduction was found against the ten ACs or Zone C: the only
committed delta is removal of the unjustified dev-dependency, while the
previously accepted memory proofs, API/CLI/MCP surfaces, OpenAPI contract,
security vocabulary guards, and foreign-evidence boundaries are unchanged.
Handoff citations continue to resolve to the same implementation files in the
committed tree; the updated QA record is `docs/evidence/KON-MVP-23/QA.md`,
section `Re-QA — 2026-08-14 — commit 498e83a`.

### Independent gates

The unlocked workspace rerun on `498e83a` completed successfully:

- workspace: 102 suites, 1,231 passed, 0 failed, 7 ignored;
- `kontor-cli`: 15 passed within the workspace run;
- `mcp_parity`: 11 passed;
- `mcp_mutants`: 11 passed;
- native memory unit proofs: 4 passed;
- memory backup/restore: 1 passed;
- native HTTP/CLI parity: 1 passed;
- project isolation: 1 passed;
- deterministic Context Pack resolution: 15 passed;
- `cargo clippy -p kontor-store -p kontor-api -p kontor-mcp -p kontor-cli --all-targets -- -D warnings`: passed;
- `cargo fmt --all -- --check`: passed.

The prior QA disposable mutation rerun remains durable evidence that both
former survivors were killed: removing the tombstone filter fails the stale
index assertion, and removing revision deletion fails the purge-history
assertion. The canonical checkout was not mutated for those seeds.

### Lock and integration qualification

`Cargo.lock` remains byte-identical to the frozen commit:

```text
03c9e37793cf1de5b1e385f47991784f4c725b35
```

Unlocked Cargo gates temporarily generated the known four-package resolver
delta; it was restored from `498e83a` and `cmp` returned 0. The stale locked
metadata/workspace condition remains the declared KON-20 responsibility for
the post-merge archive, not a standalone KON-23 defect. Migration `0022`
renumbering, one final integrated lock regeneration, the locked integrated
workspace/clippy/test/license rerun, and final integration are non-blocking
shipment qualifications and must complete before release.

### Final tree and evidence integrity

- `HEAD`: `498e83a72fc4bf42c18d62c79a96e68b1f9207ee`.
- Git tree: `e65ae3e5d01d2be9b44560026f7b5d436b7df64b`.
- Tracked owned paths are clean; no source, migration, contract, test, or lock
  changes remain in the worktree.
- Untracked evidence byproducts are preserved only: `QA.md`, `AUDIT.md`, and
  foreign KON-MVP-18 run directories including `run-40870492d74e3b3a`,
  `run-89a688943e1099bf`, and `run-97d55adc7ea6a9ef` (alongside the existing
  retained foreign runs). None is part of the commit delta or was mutated.
- No Jira, AgentsRoom, integration, push, or new seat action was performed.
