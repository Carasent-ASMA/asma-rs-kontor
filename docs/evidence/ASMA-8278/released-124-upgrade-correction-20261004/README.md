# ASMA-8278 released-124 upgrade correction (2026-10-04)

Correction packet for the two must-fix findings in the independent post-DeepSeek
source-release review (Claude Work SEAT A, `SOURCE_RELEASE_FAIL`):

- **M1** — the owning plan's real released-124 upgrade check was missing.
- **M2** — the earlier packet called a failed focused run "green" and
  over-claimed a commit pin.

Author: DeepSeek implementation child `e9d2abec-b9b7-4a13-9742-092ce2032eaa`
(OpenCode, `deepseek/deepseek-flash`, max, Build) under the correction dispatch.
Paseo-direct surface; implement-only: this packet does not self-certify PASS and
does not move Jira, merge PR #292 or adopt the release.

## M1 — real released-124 → 130 upgrade regression

Added to the existing test-only module in
`crates/kontor-store/src/migrations.rs`
(`#[cfg(test)] mod release_integration_tests`):

```
migrations::release_integration_tests::released_124_upgrade_preserves_realm_bindings_and_memory_content
```

The test:

1. Applies the **exact** `MIGRATIONS[..124]` chain to a disposable temp file
   (the released master `abe990ac` generations are the tail of that slice) and
   asserts `PRAGMA user_version = 124`.
2. Seeds a real released-124 database with raw SQL that honours the released
   schema, triggers and foreign keys:
   - Realm identity row; a permanent `realm_idempotency_bindings` row
     (`register_profile_pack`, valid at 124);
   - a project, `memory_items`/`memory_revisions`/`memory_approvals`/
     `memory_receipts`/`memory_fts` and a frozen `memory_context_bindings` row
     (`ordered_revisions` and `selection_spec` in the domain shapes the store
     reads back);
   - representative 0123 rows: eligibility, projection snapshot + active
     pointer, recall key + metadata, experience proposal;
   - representative 0124 rows: projection rebuild key + result.
3. Asserts the seed is real (row counts) and that the v28 rebuild's missing
   binding-permanence guards are still absent at 124.
4. Opens the database through the current `SqliteStore` (migrates to 130) and
   asserts: `SCHEMA_VERSION` 130, unchanged Realm id, the seeded binding
   readable through `memory_binding`, unchanged fingerprint, both permanence
   guards restored and now refusing UPDATE and DELETE, table-by-table equality
   of all 14 seeded tables, every immutability/one-way trigger still refusing
   (`memory_projection_snapshots`, `memory_recall_metadata`,
   `memory_recall_keys`, `memory_experience_proposals`,
   `memory_projection_rebuild_keys`, `memory_projection_rebuild_results`,
   `memory_revisions`), a clean `PRAGMA foreign_key_check` and
   `PRAGMA integrity_check = ok`.

Result: the test is **passing in the whole-store run below**. The focused
log and meta disagree (C1 erratum); an immediate pass is not established. The
negative/refusal assertions make it fail if an upgrade ever corrupts seeded
identity, content or binding permanence.

Command (source-target, shared target dir, `--locked`, `CARGO_BUILD_JOBS=2`):

```
cargo test -p kontor-store --lib released_124_upgrade_preserves_realm_bindings_and_memory_content --locked
```

Focused log: `receipts/released-124-focused.log` reports 1 passed, but its
`.meta` records exit=101 and no finish line. This inconsistent pair is
**not a qualifying successful invocation**. Failure cause and any intermediate
test edits are UNKNOWN; the retained bytes do not establish them. The bound
whole-store run below independently includes the new test passing.

Released-migration bytes: `receipts/released-migrations-byteequal.txt` verifies
`0115`–`0124` are byte-identical to `abe990acf754f66be66c3f5564077bb5d3333288`
(sha256 and git blobs on both sides; `all_identical=true`). Production source and
generated API inputs are byte-equal to `d0d27332`: the only source delta is
inside the `#[cfg(test)]` module
(`receipts/source-equivalence.txt`, single hunk `@@ -1085,4 +1085,385 @@`).

## Whole-store regression

```
cargo test -p kontor-store --locked --no-fail-fast
```

Raw log: `receipts/store-full.log` — **40 test-result suites, 697 passed,
0 failed, 0 ignored, exit 0**. This includes the new released-124 test, the
v119-replay fixtures, `schema_v1` and the memory/attestation suites.

## M2 — evidence corrections

Corrected in the original packet, which is otherwise preserved byte-for-byte:

- `docs/evidence/ASMA-8278/module-master-conflict-repair-20261004/README.md` —
  the verification table now labels the focused `kontor-store` run as
  **historical red** (`receipts/focused-store.meta` exit=101, two failed
  v119-replay targets before the fixture fix), marks the focused/clippy/fmt runs
  **unpinned**, and adds an Errata section naming every original mislabel.
- `.../receipts/conflict-inventory.json` — the "schema_v1 v123 upgrade green"
  and "69/69 green" citations are re-attributed to the pinned full suite at
  `ced6db99`, explicitly **not** a released-124 qualification; an `errata` array
  records M1, M2-a, M2-b, M2-c.

Preservation: the original packet's other 23 artifacts and its
`manifest.json` are unchanged (`receipts/prior-artifact-preservation.txt`
records their sha256 and git blobs). Because the original manifest still
describes the `d0d27332` bytes, its listed hashes for the two corrected files no
longer match; the old/new hashes are in
`receipts/corrected-files-old-and-new.txt`.

## Raw receipts for previously unreceipted gates

- `receipts/fmt-check.log` / `.meta` — `cargo fmt --all --check`, exit 0, pin
  recorded in the meta (pre-commit working tree).
- `receipts/clippy.log` / `.meta` — `cargo clippy --workspace --all-targets
  --locked -- -D warnings`, exit 0, pin recorded in the meta.

`receipts/precommit-binding.json` binds the pre-commit `HEAD`, the working-tree
source sha256/blob, each command and each raw log hash; the published commit pin
supplement is added after the correction commit. The earlier focused
MCP/API/contract, daemon-memory and loopback logs remain **unpinned** and are
superseded by the pinned full suite at `ced6db99` (E2 retained honestly).

## Limits

- E3-style static review of this packet is not performed by its author; the
  fresh independent correction review and Sol Judge belong to root.
- No mutants were rerun; all in-module mutation targets remain byte-equal and no
  historical kill count is re-claimed.

### C1 evidence erratum

The focused released-124 meta records exit=101 with no finish line while its
log reports one passed test. Neither an immediate pass nor a successful
focused invocation can be established. Cause and intermediate test edits
remain UNKNOWN. The unchanged bound whole-store log proves the new test
passes. The old focused schema_v1 run is red (68/1); only the pinned combined
log supports 69/69. See the correction packet `C1-ERRATUM.json`. Original
receipts/history remain unchanged; no source edit or rerun was performed.
