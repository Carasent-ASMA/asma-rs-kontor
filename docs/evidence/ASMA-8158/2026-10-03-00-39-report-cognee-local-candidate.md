# ASMA-8158 Cognee projection and launch local candidate

> **Date:** 2026-10-03 00:39 Europe/Oslo
> **Status:** 🔴 Draft — committed local candidate; independent review pending
> **Author:** ASMA-8158 persistent writer (Codex)
> **Category:** report
> **Scope:** Carasent-ASMA/asma-rs-kontor; MEM-02 of ASMA-8155, Paseo-direct
> **Summary:** Fixture transport, authoritative recall integration, actual launch prompt witnesses and MUT-004/005/006 receipts. No live projection, semantic-quality, independent acceptance or deployment claim.

## Authority and frozen source

Execution contract: `/private/tmp/ASMA-8155-full-lsa-readiness-original-20261002.txt`, SHA-256 `7ee35150e2e90cdc4fab83bac999ecd2cef6286d236df128b0ad5b8c211d828b`; sections 3, 4 and 7 govern this candidate. Canonical plan: `asma-modules/_docs/ai-orchestration/plans/2026-09-12-10-10-plan-kontor-experience-memory.md`, TASK-002. Neither authority was edited.

Branch: `feat/ASMA-8158-cognee-projection-and-task-launch-injection`. Accepted base: `655e07ead8b87bebf3a5d43ddccc15c860114e05`, schema 120. No migration was added. The final commit and tree hashes accompany the writer handoff, avoiding a self-referential commit hash in this document.

[source-hashes.json](receipts/source-hashes.json) pins the eleven changed feature files, including configuration and tests. Its aggregate SHA-256 is `528cbcd02cc5ccf553923c01751390bea1cf8f1441f2157e4e6a3c920ecb2b21`: SHA-256 of Python's default `json.dumps(files, sort_keys=True).encode()`, where each value is the file's SHA-256. Mutation baseline, seeded and restored manifests are recorded separately. The two mutated runtime files restored to:

- Adapter: `f461effcda17a460a0939ef2bb077f74de1bd0b26ee5b6c44b997fcd3ec84c40`.
- Applications: `0a15e9f04c77edfd34b56ef6a0be1f70b5c142b4cdc1d795bc0e5d2a7292617a`.

## Delivered behavior

- New `kontor-memory-cognee` crate reuses pinned reqwest `=0.13.4`; `multipart` is enabled at that crate only. Multipart add sends only provider-eligible preview entries. Blocking cognify must report completed runs; canary CHUNKS search qualifies identity retrieval. Qualification returns evidence and never changes the active pointer. Failure fixtures retain the previous pointer.
- Search sends canonical structured task title/module/scope/phase intent, a single immutable dataset filter and `top_k: 64`. CHUNKS text contributes identity/hash only; distance becomes descending rank. Store validation rehydrates the exact approved project/item/revision/hash and supplies every byte of canonical prompt content. Malicious upstream lesson text cannot reach the launch prompt.
- Response streaming is capped at 256 KiB before decoding. Semantic work has one 1–1500 ms deadline; at least 500 ms remains within the 2-second recall target for local selection. Redirects, automatic retries and inherited proxies are disabled. Refusals expose static reasons without upstream bodies. Fixture deadline checks are not a migration-scale latency qualification.
- Applications now use bounded recall for Context Pack memory sources. Absent, stale, failed, malformed, empty or foreign-only results degrade to eligible FTS. No `list_memory` fallback remains. Selection preserves the accepted eight-item/32768-byte canonical-array bounds and current-revision eligibility. Generic, pending, stale approved, tombstoned and upstream local-only candidates are rejected.
- `seat_with_address` and slot refill resolve or reuse the original root's frozen binding before `RuntimeAdapter::launch`. Root prompts append its exact canonical block and binding hashes; downstream roles cite the same original binding. Existing persona, duties, placement, launch intent, model, context and autonomy handling remain in their existing paths. No network await holds the store mutex.
- Fixtures inspect `FakeRuntime::launched_prompt`, rather than only an intermediate pack. Replay after a lost caller response, tombstone, new approval, projection rebuild, store reopen and reconciliation preserves prompt/model/binding and performs no second root launch or search. This is the lost **caller acknowledgement** witness; it does not claim a newly qualified provider transport acknowledgement mechanism. The existing runtime recovery implementation was retained.

## Configuration and bounded decisions

[CONFIGURATION.md](../../CONFIGURATION.md) documents optional `<state-root>/memory-cognee.json`, strict and at most 8 KiB:

| Key | Default / decision |
| --- | --- |
| `enabled` | `false`; semantic transport is disabled by default. Document policy remains `local_only`. |
| `endpoint` | `null`; HTTP loopback or HTTPS, without userinfo, query or fragment. |
| `credential_alias` | `null`; non-secret alias. The implementation never resolves it through Keychain or provider credentials. |
| `dataset_prefix` | `kontor`; v1 requires this prefix to retain accepted `kontor_<project_uuid>_<memory_cursor>` identity. |
| `timeout_ms` | `1500`; allowed range 1–1500, applied to the entire semantic operation. |

Fixture/embedding composition explicitly supplies a synthetic secret to `Client::new` and an optional client to `DaemonConfig::with_memory_cognee`. Ordinary startup refuses an enabled configuration without an explicit transport with static `cognee_unavailable`; it does not resolve credentials or initiate live projection. The public rebuild operation remains typed unavailable. Live alias resolution, ingestion/chunk identity qualification, projection activation and MUT-007 belong to ASMA-8159.

Context selection metadata retains its existing response shape, advances `selector_version` to 2, sets `ceiling_bytes: 32768` and `narrowed: true`, and leaves `omitted` empty. Enumerating omitted ids would require an unbounded corpus read; frozen metadata already records exclusion counts. API registry, OpenAPI artifacts and generated clients were not changed.

The inspected upstream reference is commit `b32d8afc59e1064d9291b9828a8a147be9cc8bab`, particularly [CLI add/cognify payloads](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/cli/api_client.py), [search route](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/api/v1/search/routers/get_search_router.py), [CHUNKS retriever](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/modules/retrieval/chunks_retriever.py) and [pipeline status model](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/modules/pipelines/models/PipelineRunInfo.py). This is a pinned inspected shape, not release or live-endpoint qualification. Fragmented/malformed identity text degrades safely; real chunk retention must be qualified by ASMA-8159.

## Verification

[verification.json](receipts/verification.json) records results and log hashes. All ten new tests use synthetic documents, disposable stores and loopback fixture servers or FakeRuntime. No semantic-quality claim is made.

| Check | Result |
| --- | --- |
| Final Cognee fixture target | 5 passed, including nine malformed/empty/foreign/timeout/HTTP/oversize/nonfinite/candidate-overflow cases, minimal projection, local exclusion and phase failure retention. |
| Final launch fixture target | 5 passed; root canonical bytes, downstream reference, timeout/concurrent store access, stale/no-match selection, replay and explicit startup capability gate. |
| Final full loopback integration target within interrupted broad run | 488 passed, 0 failed, 1 existing ignored. Includes both revised Context Pack selection regressions. |
| Accepted typed-memory HTTP regressions within final broad run | 2 passed; frozen replay and explicit purge refusal. |
| OpenAPI contract target | 4 passed; served/committed contract equivalence retained. |
| CLI/HTTP/MCP memory parity target | 2 passed. |
| Strict scoped Clippy; rustfmt; diff whitespace | Passed. |
| License, dependency-ban and source policy | Passed. No registry package or existing package version was added/changed. |
| Cargo audit, candidate and accepted base lock | Exit 0 with the same 10 allowed warnings. |
| Full cargo-deny gate | Failed on inherited yanked `yoke-derive 0.8.3`; bans/licenses/sources passed. Base lock contains the same version. |

The earlier broad daemon/adapter run completed with 673 passed, 0 failed and 1 existing ignored, before the final configuration witness and stronger guard assertions. The final broad rerun was interrupted with exit 130 after the Keychain incident below; completed subtargets are recorded, but that command is **not** reported as a passing final full suite. Remaining verification used explicit synthetic targets. The source manifest matched all frozen feature files after mutation restoration and before staging.

## Mutation receipts

[mutations.json](receipts/mutations.json) records exact commands, patch/source/log hashes, compilation, failed assertions and restoration. [run-mutations.py](run-mutations.py) reproduces the sequential fixture-only seed/test/restore procedure. Each seeded build compiled successfully, then failed a behavior assertion; each baseline and restored witness passed. Exits are 0 / 101 / 0 for all three. No invalid compile was counted as a kill.

| Mutant | Seed and witness | Result |
| --- | --- | --- |
| MUT-004 | Substitute returned Cognee text for the canonical block after freezing. `authoritative_rehydration_rejects_leakage_and_never_trusts_cognee_text` fails exact authoritative bytes. | KILLED |
| MUT-005 | Use `store.list_memory` to build the degraded block. `degraded_launch_never_lists_the_corpus_and_timeout_keeps_store_unlocked` detects the irrelevant corpus canary in the actual launched prompt. | KILLED |
| MUT-006 | Omit the root canonical block. `root_launch_contains_exact_frozen_canonical_bytes_and_downstream_cites_same_binding` fails its `FakeRuntime::launched_prompt` assertion. | KILLED |

No mutants survived. MUT-007 was not run or claimed. Patches in this evidence directory are deliberate receipts; seeded runtime source was restored before the final build and commit.

## Execution deviation and remaining blockers

The broad inherited daemon unit suite includes `usage::tests::a_home_with_no_credential_yields_no_token_for_any_vendor`. On macOS it invokes `/usr/bin/security find-generic-password` using temporary fixture-home aliases. Both broad invocations reached this test before the behavior was discovered. Its passing assertions require `NoCredential`; no provider credential was retrieved. This nevertheless violated the explicit **no Keychain access** constraint. The final broad run was stopped and the incident disclosed during execution. No further usage-unit or full-daemon unit run followed discovery. This report does not claim that execution satisfied the no-Keychain boundary.

The full dependency policy gate remains blocked by the unchanged base's yanked package. It was left in place because this task is limited to the new crate's required dependency changes; [dependency-delta.json](receipts/dependency-delta.json) records that no registry package/version changed. No source compilation or targeted fixture blocker remains.

The candidate is for the TPM to admit independent QA and Spec Audit on the final committed SHA. This writer performed implementation verification and mutation witnesses, not independent review. No Jira writes/transitions, push/PR/merge, install/deploy, live Cognee, live corpus/database mutation, experience approval outside disposable fixtures, new ADR/Goal or changes to another checkout were performed. Adapter links/instructions remain untouched and uncommitted. Stop after the single writer handoff; do not poll reviewers.
