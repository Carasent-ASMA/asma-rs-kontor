---
name: experience-memory
description: "Form an evidence-backed experience from a settled task, preview and propose its typed canonical revision, and verify authorized approval, projection and bounded recall. Use for experience-history closeout; use the project's instruction-authoring skill for durable agent instructions."
---

# Experience memory

Use this skill when settled work supports a small reusable experience, lesson or
mental model. `kontord` owns canonical revisions and approval; a rebuildable
projection supplies candidate identifiers, never prompt text. Keep progress,
speculation, handoffs, open questions, operational gaps, source, implementation
artifacts, documentation and verification reports in their owning task or
evidence state. A report can support a distilled lesson without becoming a
memory itself. Instruction-file preferences belong to the project's
instruction-authoring skill.

Read the project's experience-memory runbook for the relevant procedure and the
accepted requirements in the owning plan. Use the project's existing skill
discovery and router; this skill adds no loader or supervisor.

## Capability and authority boundary

The accepted source contracts are the repository's
[server-produced OpenAPI](../../../crates/kontor-api/contract/openapi.json),
[core fixture](../../../crates/kontor-core/tests/fixtures/experience-v1.json)
and [registry](../../../crates/kontor-mcp/src/registry.rs); do not maintain a
second JSON schema. These establish source names, not installed runtime
availability. Read back the deployed registry before use.

### Accepted operations

All paths below are relative to `/v1/projects/{project_id}/memory`. CLI
subcommands are generated from the same registry; the
[CLI parity fixture](../../../crates/kontor-cli/tests/memory_parity.rs)
establishes source parity rather than installed help.

| MCP operation | Generated CLI subcommand | HTTP method / suffix | Tier |
| --- | --- | --- | --- |
| `kontor_experience_propose` | `experience-propose` | POST `/experiences:propose` | Operator |
| `kontor_experience_classify` | `experience-classify` | GET `/experiences:classify` | Observer |
| `kontor_memory_recall_preview` | `memory-recall-preview` | POST `/recall:preview` | Observer |
| `kontor_memory_recall_freeze` | `memory-recall-freeze` | POST `/recall:freeze` | Operator |
| `kontor_memory_recall_get` | `memory-recall-get` | GET `/recall/{agent_run_id}` | Observer |
| `kontor_memory_projection_preview` | `memory-projection-preview` | GET `/projection:preview` | Observer |
| `kontor_memory_projection_get` | `memory-projection-get` | GET `/projection` | Observer |
| `kontor_memory_projection_rebuild` | `memory-projection-rebuild` | POST `/projection:rebuild` | Operator |

There is no public typed-document validation or projection-activation operation
in this registry. Recall preview previews selection and persists no binding; it
does not validate a proposed document. Classification is an explicit
administrative read, not a recall fallback.

The procedure describes memory product operations, not epic orchestration
authority. Paseo-direct work has no mandatory `kontord` read, approval receipt or
repair prerequisite. Invoke canonical memory operations only when in scope and
authorized; unavailable optional memory work is recorded in closeout evidence.
Do not change orchestration surface to obtain memory support.

## 1. Collect settled evidence

Read the owning task's accepted scope and observed result. Collect exact
receipts, immutable source and artifact hashes, test results and runtime
readbacks that establish what happened. Distinguish source, integration, merge
and deployed behavior. Resolve contradictions and identify what remains
uncertain before claiming a settled outcome; do not synthesize a success from
plans or progress.

Choose only `experience`, `lesson` or `mental_model`. Use `confidence=observed`
for directly established facts; use `inferred` for a bounded synthesis supported
by settled facts, with its limitations stated. Unsupported speculation is not
made eligible by calling it a mental model.

Collect all plan fields: `situation`, `intent`, `actions`, `outcome`,
`went_well`, `went_wrong`, `lesson`, `future_cues`, `avoid`, `domains`,
`occurred_at`, `confidence`, `projection_policy` and typed `evidence_refs`. The
discriminator is `schema_version: 1`, `document_type: "experience_memory"`;
`outcome` is an object with `kind: success|failure|mixed` and a nonblank
`summary`. The four narrative strings have 4096-byte bounds. Lists have at most
16 nonblank strings of at most 2048 UTF-8 bytes each; `actions`, `future_cues`
and `domains` require at least one. `went_well`, `went_wrong` and `avoid` may be
empty. The complete typed document is bounded to 65536 canonical bytes, separate
from the 32768-byte recall slice.

`evidence_refs` requires 1–16 references tagged by `type`: `receipt`
(`receipt_id`, `content_hash`), `memory_revision` (`project_id`, `item_id`,
`revision_id`, `content_hash`) or `artifact` (`locator`, `content_hash`).
Receipt and revision ids are UUIDs; hashes are 64 lowercase hexadecimal
characters. Receipt and revision evidence is resolved in the owning project.
Artifact locators are relative digest-bearing citations, not URLs or
instructions for the server to open files. Establish their supporting evidence
during collection. Use the event time for `occurred_at`, not a guessed closeout
time. Keep one actionable lesson and concrete future cues; omit transcripts and
report bodies. For a recurring mistake, the lesson states which rung of the
project's correction-preference ladder prevents the mistake, and why no higher
rung was possible. It supplements, never replaces, the owning repair steps.
Unknown fields are rejected, including nested outcome and evidence fields; use
the accepted core or OpenAPI for the remaining identifier and timestamp bounds.

Default to `projection_policy=local_only`. Choose `provider_eligible` only with
explicit authorization for the exact revision's minimal outbound content under
the configured provider policy. Approval of a lesson alone does not grant
outbound eligibility. Keep endpoints and credential aliases in deployment
configuration; reject secrets, credential-shaped text and URL userinfo in all
fields, keys, nested lists and evidence locators before persistence. Refusal
diagnostics contain static codes and structural paths, never rejected values.

If no durable lesson is supported, record "no durable lesson" and the evidence
reason in closeout. Create no placeholder experience or memory approval request.

## 2. Preview the exact typed revision

Load the pinned OpenAPI and core fixture. For an offline document preview, use
the accepted `CanonicalDocument` serialization and
`ExperienceMemoryV1::from_document` validation, including sensitive-material
rejection. Show the exact bytes to be submitted; reserialization with a default
`projection_policy` inserted can change their hash. There is no public
validation call to make before proposal. Receipt and revision resolution happens
at the authoritative proposal boundary; an offline shape check does not prove
it.

Before proposal, show the exact canonical document bytes and content hash,
target project and item identity, schema and registry provenance, evidence
resolution and projection-policy choice. Include every field and its exact
value; do not substitute a summary. Preview only after sensitive-material checks
pass. If the available surface cannot establish canonical bytes or hash, retain
the draft as evidence and stop the memory mutation. Do not label an unvalidated
draft "exact typed preview".

## 3. Propose; read authorized approval separately

Submit only the previewed document through `kontor_experience_propose`
(`experience-propose`, POST
`/v1/projects/{project_id}/memory/experiences:propose`). The body is `item_id`,
`expected_revision`, `document`, `provenance` and `proposed_by`; use
`Idempotency-Key` (MCP `idempotency_key`, CLI `--idempotency-key`). Read back
the proposal's exact project, item, revision and hash. A lost acknowledgement is
reconciled against that same operation and key, not retried as a new proposal.

Proposal requires the existing Operator authority; approval requires the
existing Admin `kontor_memory_approve` / `memory-approve` flow, POST
`/v1/projects/{project_id}/memory/revisions/{revision_id}/approval`, with
`item_id`, `expected_revision` and `approved_by`. This skill grants neither.
Present the exact revision for that flow when authorized; do not auto-approve,
infer approval from a review verdict, or approve a changed payload under an
earlier preview. Read back the approved or current revision, hash and
non-tombstoned status. Pending or rejected proposals stay out of automatic
recall. A conflict returns to evidence and a fresh preview; never overwrite a
concurrent revision. Use `kontor_memory_history` / `memory-history` for revision
readback. Accepted refusal names include `invalid_experience`,
`unresolved_evidence`, `binding_conflict`, `candidate_limit`,
`frozen_payload_purged`, `frozen_payload_mismatch`, `projection_conflict` and
`projection_unavailable`; their meanings and HTTP statuses are in the runbook.

## 4. Rebuild and read back the derived projection

After authorized approval, use `kontor_memory_projection_preview` and
`kontor_memory_projection_get` to inspect the snapshot and active readback.
Rebuild takes `expected_generation` from `preview.active_generation`,
`expected_memory_cursor` from `preview.snapshot.memory_cursor`, `preview_digest`
from `preview.snapshot.digest`, and an idempotency key. The accepted public
rebuild returns `projection_conflict` for stale inputs, otherwise
`projection_unavailable`; it does not execute a live rebuild.

The optional `<state-root>/memory-cognee.json` keys are `enabled`, `endpoint`,
`credential_alias`, `dataset_prefix` and `timeout_ms`. Transport defaults
disabled; v1 requires prefix `kontor` and a 1–1500 ms semantic timeout (default
1500). Ordinary startup refuses `enabled: true` with `cognee_unavailable`
without explicit transport composition. `DaemonConfig::with_memory_cognee` is
the accepted fixture and embedding composition gate, not an installed
credential resolver. See the runbook for exact endpoint and alias constraints.

`local_only` can use local eligible full-text search; it never enters the
external projection. For later authorized release qualification and activation,
build a complete immutable dataset `kontor_<project_uuid>_<memory_cursor>`. Only
canonical ids, project, cues, lesson and content hash enter its minimal payload;
exclude evidence bodies, transcripts, credentials and approval state.

`Client::qualify` produces add, cognify and canary evidence without activation;
`SqliteStore::activate_projection` requires those successes and freshness and
CAS checks. No public activation operation exists. Live activation, migration,
deployment and the real benchmark remain release work. Read back active dataset
identity, snapshot digest and cursor against canonical state. Failure preserves
the previous pointer. An unavailable adapter, incomplete build or stale snapshot
is reported honestly; it is not a successful rebuild or semantic qualification.

## 5. Check recall and preserve the binding

Use `kontor_memory_recall_preview` with `project_id` and `task_id` for an
authorized canary. Its HTTP body contains only `task_id`; the server derives
title, owning module, declared scope and selected work-profile phase. Do not
supply hidden caller prompt text. Check the returned exact project, item and
revision hashes, retrieval mode and reason, and exclusion counts against
canonical records. Semantic candidates must be current, approved,
non-tombstoned, strictly typed, provider-eligible and from the requested
project. Provider text never enters the prompt; only rehydrated canonical bytes
do.

The canary must distinguish `semantic`, `lexical_degraded` and `none`. Absent,
stale, unavailable, timeout, malformed or empty or foreign-only semantic results
use bounded eligible full-text search, never `list_memory` or a full-corpus
dump. Check stale, pending, tombstoned, foreign, wrong-kind, duplicate and
hash-mismatched exclusions with synthetic fixtures, not new live records.

Verify deduplication, stable score, confidence and item-id order, at most eight
items and 32768 UTF-8 bytes of exact canonical memory JSON including escaping
and container overhead. Whole non-fitting records are skipped; nothing is
truncated. Final eligibility and freeze share the authoritative transaction.
Before normal launch, freeze only through the authorized normal launch or
`kontor_memory_recall_freeze` for an existing run (`task_id`, `agent_run_id`,
idempotency key). Read back with `kontor_memory_recall_get`. The response
contains `recall.binding`, `recall.metadata`, `recall.canonical_block` and
`recall.replayed`; metadata names include `intent_hash`, `mode`, `reason`,
`memory_cursor`, `projection_cursor`, `projection_digest`, `identities`,
`exclusions`, `block_hash` and `block_bytes`. Actual prompt readback must
contain the exact frozen block for a launch-injection claim.

Replay loads the existing binding without a new projection or full-text query.
Downstream handoffs cite that binding. Historical canonical payload hashes are
verified; explicit purge that makes the payload unavailable returns
`frozen_payload_purged` (HTTP 410), never replacement memories. Do not duplicate
immutable payloads to evade purge. See the runbook for canary and closeout
evidence requirements.

## Examples and closeout disposition

Use the linked accepted
[experience-v1.json](../../../crates/kontor-core/tests/fixtures/experience-v1.json)
as the full synthetic document; its SHA-256 is
`2e61802621515979c0e745d72302caaf6d560d477bd7ae0a470a13501464337e`.
The accepted CLI parity fixture uses it for proposal and readback. Do not copy
its synthetic artifact hash into a real experience or treat fixture acceptance
as proof of a live event. These excerpts explain its shape, not another payload:

| Field | Accepted fixture value |
| --- | --- |
| schema_version / document_type | `1` / `experience_memory` |
| kind | `experience` |
| outcome | `{"kind":"success","summary":"The deployed artifact matched the intended digest."}` |
| occurred_at | `2026-10-02T12:00:00Z` |
| confidence | `observed` |
| projection_policy | Omitted in this fixture; defaults to `local_only`. |
| evidence_refs | `[{"type":"artifact","locator":"synthetic/deployed-readback.json","content_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]` |

The fixture also supplies every situation, intent, action, lesson, cue and
domain field. Artifact evidence is a digest-bearing citation; the server does
not fetch its locator. Receipt and memory-revision references require
authoritative resolution. A bounded `mental_model` may instead synthesize
settled observations as "source identity and running identity require separate
evidence", with `confidence=inferred`, exact supporting references and a stated
scope. A `lesson` may distill the observed corrective action. Neither form is
blanket approval of a general rule or a substitute for canonical validation.

| Input | Disposition |
| --- | --- |
| "Implementation is 70% complete." | Task progress; no memory proposal. |
| "The projection will probably improve recall." | Unsupported speculation; keep an investigation question. |
| A successor handoff or a pending question | Handoff or question evidence; no automatic recall. |
| An operational-gap record | Gap evidence in its owning state; no automatic recall. |
| Source code, implementation artifact, documentation or verification report | Evidence or reference material; distill only a separately supported settled lesson. |
| A credential-shaped value, unresolved evidence or an unknown kind | Refuse the typed proposal; report safe structural diagnostics. |
| Settled work with no reusable lesson | "No durable lesson" with reason; no placeholder memory. |

Closeout records the actual disposition and artifact references: no durable
lesson; draft awaiting evidence or usable preview tooling; proposed pending
approval; rejected; or approved with separate projection and recall
observations. These are prose dispositions, not new API enums. Never turn a
pending proposal, lexical canary or unavailable rebuild into an approved
semantic-delivery claim.
