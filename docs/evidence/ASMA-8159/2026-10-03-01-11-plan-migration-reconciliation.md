# Migration reconciliation at the serialized integration frontier

> **Date:** 2026-10-03 01:11 Europe/Oslo
> **Status:** 🔴 Draft — external integration pending
> **Category:** plan
> **Scope:** ASMA-8159; accepted local memory 0120 versus unmerged ASMA-8187 0120–0122
> **Summary:** Preserve shipped SQL and reconcile actual reviewed external heads in one owner-controlled lane. No migration number is reserved.

## When to load

Load before joining external candidates, changing migration numbering, or
qualifying the final release schema. No migration or integration runs here.

## Pinned local facts

Remote-base source 8acdbd17e83d9d7ec545221c4b957e0325916997 has schema 119.
Accepted ASMA-8156 655e07ead8b87bebf3a5d43ddccc15c860114e05 adds
crates/kontor-store/migrations/0120_experience_memory_projection.sql, contiguous
119→120 registration and strict typed backfill in the migration transaction.
Accepted ASMA-8158 831dd7c1dc95a57258c75f5896bff5b76f3830fa adds no migration.
The exact local SQL and registration hashes are in receipts/source-manifest.json.

Local 0120 adds memory_experience_eligibility, memory_projection_snapshots,
memory_projection_active, memory_recall_metadata, memory_experience_proposals
and memory_recall_keys. Immutable snapshots/metadata/key triggers preserve
history; no duplicate canonical payload defeats purge semantics.

Plan references to 0095_experience_memory_projection.sql and applying migration
0095 are historical naming drift. Actual immutable 0095 is Jira issue identity
SQL. Never rewrite or reapply it as memory migration.

## External facts and proposed order

The LSA observed ASMA-8187 candidate
99b237201e71bbeacea726df09f32325428b58a5, schema 122 / export generation 13,
with candidates 0120–0122. That is a historical observation, not current
acceptance or a reservation. This branch has no such candidate integrated.
TPM and the succession owner must supply final accepted heads and shipped
migration facts before any shared integration.

The contract's proposed serialized lane is PR #278/#280 baseline → ASMA-8187
succession → ASMA-8196 read API → accepted ASMA-8114 topic/checkout correction
→ committed ASMA-8340 summary composition → ASMA-8156 memory → ASMA-8158
adapter/launch. Join ASMA-8341 Cargo/bootstrap before final lock/artifact/client
freeze. Re-read all heads and amend the sequence through the owning workflow
if actual reviewed dependencies differ. No external branch/worktree is changed.

## Reconciliation procedure owed later

1. Confirm final accepted source SHAs, ancestry, schema/export generations,
   installed migration inventory, overlap owners and exclusive integration lane.
2. Compare every overlapping SQL filename/number, ordered registration,
   SCHEMA_VERSION, schema tests, restore/export code and generated contracts.
   Preserve all shipped SQL bytes. Do not assume local 0120 has shipped.
3. If the selected succession chain has shipped or takes 0120–0122, relocate
   only the unshipped memory addition additively to the actual next free
   contiguous slot. The number is decided at that fresh check; 0123 is neither
   allocated nor reserved by this package. No gap or duplicated user_version.
4. Route accepted-source edits to their owning writer through TPM. Record
   filename/registration/schema-test changes and the exact conflict disposition.
   Rerun affected suites and all mutations on the resulting source. Frozen
   reviews on 655e07ea/831dd7c1 do not accept a changed tree automatically.
5. Test fresh database, installed 119 upgrade, the final selected predecessor
   version, transactional backfill rollback on failure, immutable history,
   generic documents/bindings, foreign keys and same-Realm restore/FTS rebuild.
   Test forward-version refusal. Use copied-live rehearsal only when authorized.
6. Freeze final schema number, SQL SHA-256, registration/source SHA, compiled
   artifact hashes and backup/recovery receipts. Coordinate generation-13
   export/import assertions with ASMA-8187; do not add a private memory archive.

Live credentials, native-successor evidence and imported history never acquire
experience approval through this join. ASMA-8187's extra hosted-seat bearer
markers must be preserved in its scanner; no fork is introduced.
