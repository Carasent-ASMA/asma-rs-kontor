# Runnable synthetic census and isolated rehearsal

> Date: 2026-10-03 10:43 Europe/Oslo
> Status: Synthetic execution passed; copied-live execution remains gated
> Category: plan
> Scope: ASMA-8159 local preparation; same-Realm synthetic copy only
> Summary: A 55-identity artificial cohort plus two current additions exercises coherent census, verified backup/restore, optimistic mutations and immutable replay. No real historical identity is asserted.

## When to load

Load for rerunning the synthetic proof or preparing a later precisely authorized same-Realm copied-data rehearsal. This extends the existing corpus scaffold; it does not replace the owning plan or its Goal.

## Runnable local proof

From this checkout, with the installed toolchain and existing offline dependencies:

```sh
python3 docs/evidence/ASMA-8159/production-prep/run.py run synthetic-census-rehearsal-repeat cargo test -p kontor-daemon --test synthetic_census_rehearsal --locked --offline -- --nocapture
```

Choose a fresh receipt name for each run. The test calls supported `propose_memory_revision`, `approve_memory_revision`, `tombstone_memory`, `classify_experiences`, `projection_preview`, `recall_experiences_idempotent`, `recovery::snapshot` and `recovery::restore`. It performs no purge or direct mutation of canonical ledger tables. Administrative census SQL is read-only in one transaction. The deliberately injected migration failure is a separate disposable F4 fixture.

Recorded result: [synthetic proof](receipts/synthetic-rehearsal-proof.json); complete command/log/source identities in [run receipt](receipts/synthetic-census-rehearsal.json). The test prints `SYNTHETIC_REHEARSAL_RESULT` with all 55 artificial project/item/revision/hash identities, classification rows, snapshot manifest, actual temporary destinations, prior-history digest and teardown proof. The [real 55-slot worksheet](../historical-55-census.csv) remains unchanged and pending.

The synthetic current corpus contains 57 items: 55 in the explicit artificial cohort and two additional items. Current approved, untombstoned heads: 47, comprising 41 typed recall-eligible heads and six generic heads. Of the typed heads, 31 are provider eligible and ten local only. Five pending and five tombstoned items account for the remaining ten. The projection contains exactly 31 provider-eligible entries. SQL `document_type` is a candidate label; the Rust validator supplies final typed eligibility. No SQL shape heuristic grants provider eligibility.

## Exact boundary, provenance and retention

All rows originate in `crates/kontor-daemon/tests/synthetic_census_rehearsal.rs` using `ASMA-8159-explicitly-synthetic` provenance and synthetic fixture authors/reviewers. The seeded fake-runtime World uses a new TempDir, empty provider-home approvals and no composed Cognee client. Snapshot copies the entire synthetic SQLite database with its manifest, preserving its Realm identity. It is a same-Realm backup, not a redacted portable export. No credential files, `memory-cognee.json`, provider homes or real project checkout are copied.

The manifest’s byte length and SHA-256 are verified before restore. A separate isolated TempDir receives the restore through the product’s exclusive state-root lock; restore leaves reconciliation required. This test opens only the store in that copy and dispatches no daemon/runtime/provider work. Origin, backup and copy roots are retained for the test lifetime, then deleted and checked absent. Only explicitly synthetic identities, hashes, paths and proof receipts remain in Git evidence. No credential is needed for this executed proof.

A later live copy would require authorization to read the exact source Realm/database/cohort artifact and write the selected backup/copy destinations, plus an approved retention deadline and teardown owner. Operator API readback may require that Realm’s operator capability; offline backup access requires filesystem access, not a guessed Cognee/Jira/provider credential. Cognee access is a separate gate and is unnecessary for local-only census/ledger rehearsal. Possession of a database backup grants no provider, approval or live mutation authority.

## Product command shapes for later approved rehearsal (documentation only)

The paths below are placeholders that must be bound to the approved source, copied destination and reviewed binary. They are not executed against live data here.

```sh
<reviewed-kontor-daemon> --state-root <authorized-source-root> snapshot --into <approved-isolated-backup-directory>
<reviewed-kontor-daemon> --state-root <approved-stopped-copy-root> restore --snapshot <verified-snapshot-path>
```

Record the product snapshot manifest and independently verify hash/length; preserve the original snapshot. Confirm the destination is stopped, isolated from production sockets/processes/worktrees and has no provider credential/config homes. Product restore must report same Realm, exact schema and reconciliation required. Do not start normal serving until approved recovery/isolation prerequisites are met. Do not substitute a portable export/import for a same-Realm database rehearsal.

The exact read-only census is [census.sql](census.sql). Execute it under `BEGIN` on the isolated authorized copy, together with coherent counts of items, revisions, approvals, tombstones, receipts, bindings and actual current approved heads. Export ordered identity/hash/provenance/classification rows to a reviewed redacted artifact. Call supported typed classification for final eligibility and report generic/pending/tombstoned/local/provider counts separately. Hash ordered rows, including immutable prior revisions, approvals, tombstones, receipts, context bindings, recall keys and metadata; record current mutable aggregate revisions/head pointers separately.

Compare the immutable owning historical 55-cohort artifact by its exact identity tuples against coherent current N. Report retained, superseded, tombstoned, missing and additional identities explicitly. Never infer the first 55 current rows, replace the historical artifact with synthetic identities, or classify a generic row as an experience automatically. Real cohort and current N remain unavailable in this checkpoint.

In the synthetic execution, copy history and mutable item rows exactly match the source before edits. A supported tombstone appends one receipt and rejects a stale repeated aggregate revision. A supported new revision plus approval appends two further receipts and changes only the intended approved head/aggregate. Every prior immutable row remains byte-equivalent, source history/item rows remain unchanged, zero purges occur, and a frozen recall binding replays exact bytes/hash after reopening the copy. Updated aggregate readback must change; preserved-history hashes must remain equal over the original-row set. Tear down only the explicitly owned copy/backup roots after retaining approved redacted evidence, and verify absence. Live retention, timing, teardown and rollback execution are still later obligations.
