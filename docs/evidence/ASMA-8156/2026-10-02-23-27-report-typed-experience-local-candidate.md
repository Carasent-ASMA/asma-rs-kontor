# ASMA-8156 typed experience local candidate

> **Date:** 2026-10-02 23:27 Europe/Oslo
> **Status:** 🔴 Draft — local source candidate; independent review pending
> **Author:** ASMA-8156 persistent writer (Codex)
> **Category:** report
> **Scope:** Carasent-ASMA/asma-rs-kontor; MEM-01 of ASMA-8155, Paseo-direct
> **Summary:** Records the additive typed experience/recall contract, bounded decisions, source/fake verification and three behavioral mutation receipts. This is delivery evidence for the owning TPM, not epic acceptance or a deployed-service claim.

## When to load

Load for review of ASMA-8156, the ASMA-8157 example contract, or the ASMA-8158/8159 adapter and integration handoff. The synthetic document is [experience-v1.json](../../../crates/kontor-core/tests/fixtures/experience-v1.json); runtime validation remains authoritative.

## Authority and source

The execution contract is `/private/tmp/ASMA-8155-full-lsa-readiness-original-20261002.txt`, SHA-256 `7ee35150e2e90cdc4fab83bac999ecd2cef6286d236df128b0ad5b8c211d828b`. Sections 2–4 and 7 govern this source-only/fake candidate. The canonical plan is `asma-modules/_docs/ai-orchestration/plans/2026-09-12-10-10-plan-kontor-experience-memory.md`; its Goal and requirements were not changed.

Branch: `feat/ASMA-8156-typed-experience-contract-and-recall-core`. Base: `8acdbd17e83d9d7ec545221c4b957e0325916997`, schema 119. The frozen commit and Git tree are reported with the writer's final handoff; [source-manifest.json](receipts/source-manifest.json) pins every changed source, fixture and generated artifact independently of this report. No root/catalog or other owner's source was changed.

## Delivered behavior

- Core exports strict `ExperienceMemoryV1`, closed enums, typed evidence and recall/projection identities. Generic canonical documents retain their existing 1-MiB bound and ledger support. The shared sensitive-material scanner now also rejects credential-shaped keys and every URL userinfo authority, including username-only and later URLs in a string.
- Store proposal delegates to the existing canonical proposal/approval ledger. Proposal creates no approval. Typed idempotency records preserve the original receipt. Eligible reads exclude generic documents. Semantic candidates contribute identity/hash/finite score only; exact current approved project/item/revision/hash rehydration and freeze use one immediate transaction.
- Recall selects at most eight experiences within 32768 bytes of the exact UTF-8 canonical JSON array, including escaping, commas and brackets. Deduplication precedes budgeting; an oversized candidate is skipped so later fitting records can be retained. Eligible FTS joins its indexed revision to the current approved revision before LIMIT 64. There is no full-ledger fallback in typed recall.
- Frozen metadata records derivation hash, retrieval mode/reason, cursors, identities, exclusions and canonical block hash/bytes. Replay loads the binding first, verifies historical bytes and returns them across restart, task edits, rebuild and tombstone. Explicit purge refuses missing frozen payload without selecting replacements.
- Projection preview describes the complete provider-eligible dataset. Persistence stores immutable identities/digest, not another payload copy. Activation requires add/cognify/canary qualification, freshness and generation CAS. Fake phase failures retain the previous active pointer. Public rebuild returns typed unavailable while the adapter is absent.
- Eight HTTP/registry operations provide proposal, classification, recall preview/freeze/readback and projection preview/readback/rebuild. Concrete OpenAPI schemas describe documents and nested readbacks; CLI commands derive mechanically from the registry. Existing route/schema contracts are byte-equivalent as JSON values. Generated console types are updated.

## Bounded decisions

| Decision | Final contract and evidence |
| --- | --- |
| Discriminator | `document_type: "experience_memory"` and `schema_version: 1`; unknown fields and all unlisted kind/confidence/policy/outcome values refuse. Kind is `experience`, `lesson` or `mental_model`; confidence is `inferred` or `observed`. Core strictness tests include every field and operational-evidence negatives. |
| Bounds | Situation/intent/lesson/outcome summary: nonblank, ≤4096 UTF-8 bytes. Lists: ≤16 entries of nonblank ≤2048 bytes; actions/future_cues/domains require ≥1. Evidence: 1–16 references. Item/receipt/revision text: ≤128 bytes; receipt/revision UUID syntax. Artifact locator: ≤1024 bytes, relative slash-separated path without empty, dot, parent, colon or backslash components. Typed canonical document: ≤65536 bytes. Generic canonical maximum remains 1 MiB. Core/store boundary tests cover these constraints. |
| Evidence resolution | Receipt ids resolve in the owning project against memory receipt result hashes or command receipt intent hashes. Memory revision references resolve the exact historical project/item/revision/hash, including project attribution. Artifact references are digest-bearing immutable citations, never network fetches or filesystem reads. Resolution happens before typed proposal persistence and during typed migration backfill. Evidence tests cover valid receipt/revision citations, wrong hashes and foreign references. |
| Refusals | `invalid_experience`, `unresolved_evidence`, `frozen_payload_purged`, `frozen_payload_mismatch`, `memory_binding_conflict`, `memory_candidate_limit`, `projection_conflict`, `projection_unavailable`. Domain variants retain the corresponding closed `MemoryRefusal` names; the HTTP binding/candidate codes use the `memory_` prefix. Errors contain static advice/structural paths. HTTP tests capture tracing and check secret canaries are absent. |
| Ordering and candidate bound | Score descending, confidence descending (`observed` before `inferred`), item id ascending. Deduplicate before byte/item budgets. Upstream candidate vectors over 64 degrade as malformed; foreign-only/empty results use eligible lexical degradation. Adapter byte/deadline enforcement belongs to MEM-02. |
| Cursors and purge replay | `memory_cursor` is the existing realm-wide memory receipt rowid watermark; the primitive binding's approval selection cursor remains separate. Projection dataset is `kontor_<project_uuid>_<memory_cursor>` with complete snapshot digest and CAS generation. A changed receipt watermark conservatively makes an active snapshot stale. Replay uses frozen historical hashes and original metadata rather than today's eligibility or query. Purge preserves receipt/identity metadata and returns HTTP 410 `frozen_payload_purged`; no silent reselection or immutable duplicate payload. |
| Query derivation | Public recall accepts task id and, for freeze, an existing run id plus idempotency key. The domain service derives title/module, declared epic scope short title and selected workflow phase from authoritative rows. Hidden caller query text is refused. Task/run association and idempotent replay are checked before a new query. |

## Migration allocation and reconciliation

Local migration **0120** follows 0119 in the existing ordered transactional chain; `SCHEMA_VERSION` is 120. It creates the typed eligibility cache, immutable projection snapshots, active pointer, recall metadata and proposal/recall idempotency records. Strict typed eligibility backfill runs in that same migration transaction. The 119→120 fake upgrade preserves generic ledger documents/bindings, backfills valid typed rows, verifies immutable triggers and checks foreign keys. No live corpus or database was opened or migrated.

Unmerged 0120–0122 on the succession candidate are not reservations. This local 0120 requires additive numbering/chain reconciliation by the serialized integration owner after the actual reviewed heads are selected. No gap to 0123 is introduced or represented as installed migrations.

## Verification

[verification.json](receipts/verification.json) records exact commands, results and log hashes; [source-manifest.json](receipts/source-manifest.json) records source/artifact hashes. The suites use synthetic documents, disposable stores and the existing fake/loopback harness.

| Check | Result |
| --- | --- |
| Core library + typed document integration | 43 + 4 passed |
| Store generic/typed memory tests | 20 passed; race, byte boundaries, stale FTS, malicious tuples, ordering, replay/purge, projection failures and idempotency |
| Full store schema suite | 65 passed, including contiguous upgrade/backfill and preserved generic behavior |
| Backup/imported-memory restore regression | 1 passed; canonical ledger/binding restored with derived FTS rebuilt |
| API/MCP library and integration suites | 110 passed, including four OpenAPI artifact/schema tests |
| Daemon HTTP and CLI/MCP/HTTP memory parity | 2 + 2 passed |
| Registry cardinality, wrapper mutants and parity | 11 + 11 + 12 passed |
| Console | 305 tests passed and full console typecheck passed before the final schema refinement; final regenerated declaration passed standalone strict TypeScript checking and byte comparison against regeneration |
| Source checks | Clippy on all targets of six touched crates with warnings denied; rustfmt; diff whitespace check |
| Dependency policy | Offline license, dependency-ban and source checks passed; no manifest/lock/dependency change |

The console suites' earlier full run is distinguished from final generated-declaration verification. No live benchmark, provider, Cognee, deployment or release-wide qualification is claimed.

## Mutation receipts

[mutants.json](receipts/mutants.json) and the individual JSON/log files preserve original, seeded and restored SHA-256 hashes, exact commands and exit codes. All three seeded builds completed compilation and failed at runtime; baseline and restored runs passed. No mutant survived. [mutants.py](receipts/mutants.py) reproduces the sequential snapshot/seed/test/restore procedure.

| Mutant | Seed and witness | Result |
| --- | --- | --- |
| MUT-001 | Admit `operational_gap` in core eligibility; `strict_experience_roundtrip_and_non_memory_eligibility` fails its non-memory rejection assertion. | KILLED; exits 0 / 101 / 0 |
| MUT-002 | Omit project predicates from exact candidate SQL; the existing malicious-tuple test fails because a foreign tuple reaches the project's freeze primitive instead of being discarded (typed recall returns `NotFound`). The valid baseline returns exactly the sole valid candidate with ten invalid exclusions. | KILLED; exits 0 / 101 / 0 |
| MUT-003 | Permit one extra byte in selection; `exact_budget_32768_32769_unicode_escaping_and_skip_oversized_top` observes a 32769-byte result where the original guard returns `[]`. | KILLED; exits 0 / 101 / 0 |

The malicious-tuple witness also pins removed approval/current/tombstone/hash/provider-policy guards. The restart/task-edit/tombstone/purge witness pins replay reuse rather than recomputation. MUT-004–007 remain with their existing owning tasks.

## Handoff and remaining ownership

ASMA-8157 can use the core fixture and served OpenAPI schemas; registry spellings are `kontor_experience_propose`, `kontor_experience_classify`, `kontor_memory_recall_preview`, `kontor_memory_recall_freeze`, `kontor_memory_recall_get`, `kontor_memory_projection_preview`, `kontor_memory_projection_get`, `kontor_memory_projection_rebuild`. Generic approval remains Admin-only; proposal never authorizes approval.

ASMA-8158 owns the network adapter and actual root launch prompt/downstream reuse/lost-ack launch integration. The seams are `ApplicationOperations::recall_memory` / `rebuild_memory_projection`, `SemanticRecall`, and store projection preview/stage/qualification/activate. Store validation/freezing remains authoritative; network work must remain outside its mutex. MEM-01's daemon deliberately supplies typed `absent` degradation and refuses rebuild while unavailable. The ≤2-second total adapter/lexical budget and upstream response-byte bound remain MEM-02 obligations.

ASMA-8159 owns joining independently reviewed candidates, numbering reconciliation, combined artifact freeze, 24-case retrieval benchmarks, migration-scale latency, copied-corpus rehearsal, live migration and deployment. No reserved decision was taken, no replacement issue or new seat was created, and no independent review/acceptance is claimed. There is no source-only blocker for this local candidate.

## Execution incident

A `pnpm exec ... --version` command unexpectedly auto-installed cached workspace dependencies. This violated the no-install constraint and was disclosed during execution. The three generated dependency directories that were absent initially (`node_modules`, `apps/console/node_modules`, `apps/desktop/node_modules`) were removed after artifact generation and console verification; they remain absent. Manifests/lockfiles were unchanged. Final generation used an already-existing cached `openapi-typescript` 7.13.0 executable directly, with no further installation. No live provider/credentials, publication, service, approval or Jira mutation was performed.
