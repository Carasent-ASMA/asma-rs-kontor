# Historical corpus census and copied-database rehearsal

> **Date:** 2026-10-03 01:11 Europe/Oslo
> **Status:** 🔴 Draft — prepared; no corpus or database accessed
> **Category:** plan
> **Scope:** ASMA-8159 / TASK-004 release worksheet under the existing ASMA-8155 plan
> **Summary:** Reconcile the exact historical 55-item cohort with the current corpus, preserve history, and rehearse supported tombstones and evidence-backed seeds on a copy before any authorized live change.

## When to load

Load at the authorized corpus frontier, after serialized source integration and
independent review. This is a rehearsal specification, not execution evidence.

## Inputs and identity

The authoritative plan is
/Users/igor/carasent/asma-modules/_docs/ai-orchestration/plans/2026-09-12-10-10-plan-kontor-experience-memory.md,
TASK-004 / REQ-016–017 / TEST-010. The preserved LSA contract has SHA-256
7ee35150e2e90cdc4fab83bac999ecd2cef6286d236df128b0ad5b8c211d828b.
Its sections 4 and 6 supersede the historical plan's migration-0095 wording.

The historical realm/project identities in that plan are
01a00649-9ee6-73e0-ba1b-6a6c35cfd065 /
01a0064a-e056-7603-9968-ef64fdaacb75. They are intended targets, not a fresh
readback. Obtain supported realm/project identity readback before later use.

Neither the plan nor the supplied frozen inputs contains the 55 actual item
identities. historical-55-census.csv contains exactly 55 numbered **pending
slots**. Empty item/revision/hash fields are deliberately unfilled. It is not a
census result. Obtain the immutable historical approved-cohort export/readback
and its hash from the owning evidence before filling any slot. Do not choose
the first 55 current records or infer item ids from slot numbers.

## Exact census procedure

1. Pin the historical cohort artifact, capture time, realm/project, corpus
   cursor and SHA-256. Require exactly 55 distinct project/item identities.
   Fill each slot with its historical revision id and canonical content hash.
   Preserve the artifact separately from this index; do not embed corpus bodies
   or secrets in repository evidence.
2. At a coherent read boundary, capture the complete current approved corpus
   through the supported read surfaces. Record cursor, item/revision/hash,
   aggregate revision, approval identity/time, tombstone state and strict typed
   eligibility. The classification operation returns identity,
   recall_eligible and optional projection_policy; approval, aggregate/history
   and tombstone details require their existing ledger readbacks too.
3. Join the historical 55 to current rows by project/item, retaining both
   historical and current revision/hash. Classify each as unchanged,
   superseded, tombstoned, purged, missing or unresolved. Verify historical
   hashes through immutable revision/history evidence where available.
   Missing/purged/unresolved records are findings, never synthetic conversions.
4. Maintain a separate current-corpus manifest for additions since the original
   cohort. Report historical total 55, current total N, overlap, additions and
   each disposition count. All counts must reconcile; N need not equal 55.
5. Validate strict ExperienceMemoryV1, three-kind eligibility, resolved typed
   evidence and sensitive-material checks. Retain current eligible revisions;
   plan supported tombstones for every current approved legacy non-experience
   document. Flag typed/evidence anomalies for owning review. Preserve exact
   current aggregate revision/conflict inputs and reason per planned command.
6. Hash a deterministic manifest sorted by project/item/revision; store the
   exact serialization and algorithm with the digest. Each mutation plan binds
   its project, item, aggregate revision, revision/hash, reason, caller tier and
   stable idempotency key to the frozen census. A changed cursor/current
   revision requires a fresh preview and comparison before applying.

## Supported operations (commands below are templates, not executed)

The combined registry at 831dd7c1 provides Observer classification:

    kontor experience-classify --project-id <confirmed-project>

This generates GET /v1/projects/{project_id}/memory/experiences:classify and
MCP kontor_experience_classify. Use its actual deployed CLI help/registry for
state-root, endpoint and tier selection; this document supplies no credential.
Tombstones use the existing Admin kontor_memory_tombstone operation, preserving
optimistic inputs and immutable receipts. Seed proposals use Operator
kontor_experience_propose; approvals use existing Admin kontor_memory_approve.
Check those operations against the final deployed registry before use.
No purge, raw SQL writes or bulk synthetic conversion is part of this migration.

## Copied-database rehearsal

1. Under later explicit release authorization, obtain a supported coherent
   same-Realm snapshot and its manifest/database hashes, size, realm identity
   and schema version. Keep an untouched verified original. Do not copy a
   running SQLite main file without its supported consistency mechanism.
2. Restore a separate copy through the supported stopped-realm recovery
   procedure. Keep its state root, ports, runtime roots, corpus and manifests
   separate from live services. Disable external transports and use explicit
   fake runtime/fixture credentials for rehearsal. Verify identity and recovery
   barriers before starting a disposable test daemon. No live credential or
   provider access follows from possessing a database copy.
3. Apply the final reviewed contiguous migration chain using normal store
   startup/recovery, never direct migration SQL. Record installed starting
   version, target version, SQL hashes, outcomes and rollback-failure evidence.
   Local 119→120 synthetic coverage is distinct from this copied-live rehearsal.
4. Before mutations, export deterministic rows/counts for memory_items,
   memory_revisions, memory_approvals, memory_tombstones, memory_purges,
   memory_receipts and memory_context_bindings; also import-lineage/cutover
   records and the new typed/projection/recall tables. Hash canonical document
   bytes and the sorted old rowsets. Preserve raw canonical text as text:
   reparsing/reformatting JSON must not alter historical content hashes.
5. Run classification on the copy and compare exactly with the frozen census.
   Apply only planned supported tombstones, then separately propose/review the
   three seeds. Compare exact receipts, current pointers and aggregate revision
   increments with the command plan. Reusing each key must reuse its receipt.
6. Verify every pre-existing immutable revision, approval, receipt and binding
   row still has the same hash. Report appended rows separately. Whole-table
   and database hashes may change; prove preservation by the original rowset,
   not by claiming unchanged whole-file hashes. No new memory_purges row is
   permitted. Expected current-pointer/aggregate changes must match the plan.
7. Verify foreign keys, immutable triggers, typed backfill and derived FTS
   rebuild, including indexed-revision/current-approved join semantics.
   Reopen the copied store and verify frozen history/replay. A purge refusal
   is tested with synthetic fixtures only, never the copied historical cohort.
8. Run the prepared benchmark at the exact 55-item cohort and current N-item
   scale, preserving separate semantic/degraded metrics and prompt evidence.
   Return a rehearsal bundle with before/after manifests, row hashes, receipts,
   errors, schema readbacks and limitations for independent review.
9. Live mutation remains a separate authorized frontier. Re-census live state,
   compare all expected revisions/hashes and snapshot again immediately before
   the change; do not replay stale rehearsal writes against changed live rows.

## Three seeds — content and evidence to obtain

| Seed intent | Required settled evidence | Later disposition |
| --- | --- | --- |
| Default-allow admission without a revoked hold | Exact before/after admission state, hold/revocation receipts, selected configuration and observed corrected behavior | Small experience with concrete future cues; propose only after reference resolution |
| Source/merge claims without deployed readback | Exact source/merge/artifact identities, restart/readback mismatch and corrected runtime canary | Separate source identity from deployed identity |
| Operational evidence misfiled as long-term memory | Exact historical item/revision/hash, classification and tombstone receipt, evidence-backed replacement if warranted | Keep operational evidence in its owner; a separate settled lesson may cite it |

No evidence locator, timestamp, canonical seed hash, approval or provider grant
is invented here. Default each seed to local_only. Choose provider_eligible only
for an explicitly authorized exact revision. Proposal, approval, projection and
recall observations remain separate.

## Backup and portability boundary

Same-Realm snapshots preserve native ledger/bindings. Current generation-12
cross-Realm export omits them; run bindings and active projection selection
remain realm-local. Derived FTS and Cognee are rebuilt, not copied as authority.
ASMA-8187's candidate generation-13 changes require serialized owner
coordination; this worksheet does not claim portable recall state.
