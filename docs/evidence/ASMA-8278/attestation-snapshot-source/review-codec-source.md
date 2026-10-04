**ASMA-8278 SEAT B — bounded SOURCE findings: pure public snapshot codec**
Reads: module `845e36c36efa9bbf3912c61f91fb7fdb2b590ea9` tree `58b9b276…` vs parent `26b1c871…`, root spec `6862685…` §9.3, and the named evidence bundle only. No tests/writes/native probes; all prior findings, R-01 correction, O-01/O-02, B2 and frozen R3 preserved.

**Pins verified**
- Delta exactly 3 paths, +350, parent is `26b1c87`: `attestation.rs` `bb7edb67…` ✓ (one added line: `pub mod snapshot_codec;` at `:28` — nothing else), `snapshot_codec.rs` `c3b83275…` ✓, `snapshot_codec/tests.rs` `6ddbdc8a…` ✓. Tree matches.
- No production caller: outside the module only the export line exists (grep clean); no `CanonicalDocument`, keychain, store or native reference beyond the doc comment (`snapshot_codec.rs:7`).

**Requirement checks (all hold, exact lines)**
- Exact schema 1: `:73-75` (`Schema`). Positive revision, ≤64 keys, DER 1..=2048, duplicate issuer/key pairs, invalid intervals: reused parent-private `validate_snapshot` via `super::validate_snapshot` (`:125-126`; Rust child-module privacy permits this), mapped to `Snapshot` (`:34-36`).
- Identifier ≤256 UTF-8 **bytes**: `:127-131` uses `str::len()` (bytes), `IdentifierBytes` (`:37-39`); core `ExternalId` grammar untouched (max 256 chars).
- Input ≤1 MiB **pre-parse** (`:68-70`); output ≤1 MiB post-serialize (`:119-121`).
- Unknown/duplicate fields: `#[serde(deny_unknown_fields)]` on both wire structs (`:42-59`) plus serde duplicate-field refusal; malformed/invalid typed JSON → `Payload` (`:71-72`).
- Pre-clone encode validation: `validate_transport` runs at `:101` before any `.clone()` (`:102-117`).
- Untrusted public config only: no claims, no authenticity/freshness/DER-validity statement (`:1-7`, `:61-63`, `:95-96`); returns the same freely-constructible `PublicKeySnapshot` the verifier consumes; no authority conversion.

**Findings**
- **C-01 · Info · post-parse structural validation.** Only the 1 MiB ceiling is pre-parse; key count (≤64), DER size and interval checks run after `serde_json::from_slice` (`:68-72` vs `:91`). A 1 MiB input can therefore materialize up to thousands of parsed key objects before refusal. Amplification is bounded by the pre-parse ceiling (serde output roughly proportional to input), there is no caller/admission path, and spec §9.3 mandates only the byte bound. Disposition: accepted posture; no reachable defect chain.
- **C-02 · Info · encode size check is a backstop.** For any snapshot passing `validate_transport`, worst-case serialization is ≈0.6 MiB (64 × (2048 DER values as numeric JSON + two 256-byte ids)), so `SnapshotCodecRefusal::Size` on encode is unreachable through structurally valid input; `key_and_der_count_boundaries_keep_maximum_output_below_ceiling` asserts the bound (`tests.rs:179-212`). No defect.
- **C-03 · Info · evidence bundle lacks the mutation runner.** The directory contains `MANIFEST.json`, raw logs and `mutation-source/`, but no runner script; `isolated_target` (`MANIFEST.json:196`) reuses the previous run's `mutation-target` path. The four kills are nonetheless substantiated by genuine runtime panics at `tests.rs:44` (preparse size), `:63` (schema), `:122` (shared validation), `:166` (UTF-8 bytes), each exit 101, with baselines passing before/after (`baseline-before/after.log`, 7 tests) and `originals_preserved`/`isolated_source_restored` true. Mutation definitions/kill criterion cannot be inspected from this bundle alone. Disposition: evidence-hygiene note for root's provenance supplement; no source impact.
- **C-04 · Info · label preserved as instructed.** `source_head` = `26b1c87` (pre-commit parent) with owned hashes pinning `845` content. Content is fully pinned by hash; the label is intentional.

**Evidence limits (no rerun performed)**
`runtime.log` corroborates the reported totals: 141 unit + 5+7+8 integration + 2 doctests, all `ok`; `focused.log` shows the 7 codec tests; fmt/diff/clippy logs show exit 0. I did not independently execute tests, inspect `mutation-source/` working copies, or verify the cross-run target directory. The manifest `limits` correctly disclaim authenticity, freshness, DER validity and all wiring — consistent with what the source does.

**Disposition:** no substantiated blocker or reachable defect chain to this non-authorizing transport slice; C-01/C-02 are bounded posture notes, C-03/C-04 evidence hygiene. This remains untrusted public configuration only — no trust, possession, freshness, fence or admission is established — and this is a bounded source finding, not a formal committee/calibration/live/official epic PASS.
