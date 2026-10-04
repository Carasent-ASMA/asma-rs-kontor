# Composed production preparation — ASMA-8159

> Date: 2026-10-03 10:55 Europe/Oslo
> Status: Local candidate; independent review and production frontiers pending
> Category: report
> Scope: MEM-04 / TASK-004 of ASMA-8155; Paseo-direct
> Summary: Async supplied-client rebuild, durable original-result replay, composed mutation proof and synthetic migration/corpus rehearsal. Exact command/results and remaining qualification blockers accompany this frozen additive candidate.

## When to load

Load for TPM admission of existing independent review on the writer’s frozen SHA, routing inherited qualification findings, coordinating serialized external joins, or presenting the real credential proposal to Igor. This is writer verification and preparation, not an independent review or release acceptance.

## Authority and immutable pins

Checkout `/Users/igor/.paseo/worktrees/0n8yzbno/feat-asma-8155-kontor-experience-memory-and`; branch `feat/ASMA-8155-kontor-experience-memory-and-analogue-recall`; repository `Carasent-ASMA/asma-rs-kontor`; TSW `wks_b22a010864eaa006`.

Starting checkpoint `29c8134646b610234abcb740738979dea84c85fe`, tree `c8b03ad2a0694a0b48500d2c0e397c83bc6a0a03`. Operational authorization `44d5d916-d778-46b8-b00d-2cca01cc6553/8155-production-prep`. The full LSA disposition was read and verified: 30,440 bytes, final LF, SHA-256 `2b2c89f3b417237dadaebbb1d3f8698eb94b60c926bc01dc4b975224d7a8c55c`, path `/private/tmp/ASMA-8155-ASMA-8159-lsa-production-prep-disposition-20261003.json`. Its partial acceptance permits the existing supplied-client source/fake seam and continues reserving real credential resolution and account child-environment extraction.

| Pin | Exact SHA | Frozen status |
| --- | --- | --- |
| Accepted 8156 | `655e07ead8b87bebf3a5d43ddccc15c860114e05` | QA+Audit PASS |
| Accepted 8158 | `831dd7c1dc95a57258c75f5896bff5b76f3830fa` | QA+Audit PASS; auditPassed=true |
| Accepted external docs | `7d19280857ae3c60a870e017da3f733f9ff2625b` | Corrective docs independent review PASS, TPM pin |
| Prior package | `fe5373f2d2a2c147a6e2b6398c07b5091eaa1ca0` | Accepted local package; historical receipts preserved |
| Resumption stop | `29c8134646b610234abcb740738979dea84c85fe` | Real-resolution stop partially upheld; independent fake work admitted |

Original contract SHA-256 `7ee35150e2e90cdc4fab83bac999ecd2cef6286d236df128b0ad5b8c211d828b` and the existing plan’s Goal remain unchanged. No Jira, push/PR, external owner dispatch, deployment, install, live corpus, real credential/provider/Cognee/Keychain action or production activation occurred. Only local fake-runtime, loopback provider fixtures and disposable synthetic stores were used.

## Implemented source and owned paths

`ApplicationOperations::rebuild_memory_projection` now uses the existing async port; the same Operator HTTP handler awaits it. The daemon’s new `applications/memory_projection.rs` uses the already composed `Client`, staging coherent optimistic inputs before add/cognify/canary and releasing the store lock across network awaits. Missing client remains typed `projection_unavailable`. The narrow synthetic adverse qualifier carries only preview/qualification/redacted error and its production implementation delegates to `Client::qualify`.

The store extracts shared staging/activation transaction helpers without weakening the accepted final canary/digest/cursor/CAS guard. Durable key binding and original result receipts use canonical envelope hashes and existing typed memory receipt conventions. Same-key/same-bytes success replays its original authoritative readback after later ledger changes and restart, even without a client; changed bytes/project conflict. The activation and its result receipt commit together, and injected receipt-write failure rolls back the pointer. Concurrent same-key qualification may repeat provider effects, but only one activation/result wins; distinct keys require late CAS. No provider text enters canonical authority.

Necessary additive storage is `0121_memory_projection_rebuild_receipts.sql`: two append-only key/result tables, registered at schema 121. Existing lifecycle command result storage has a closed kind trigger and cannot validly hold this operation. The memory proposal/recall receipt pattern is reused. This is task-local tentative numbering, not a reserved slot; 0120 SQL stays byte-exact. No dependency or account-resolver change is made.

Owned source paths: API `src/applications.rs`, `src/memory.rs`; daemon `src/applications.rs`, new `src/applications/memory_projection.rs`, `tests/harness/mod.rs`, new `tests/projection_rebuild.rs` and `tests/synthetic_census_rehearsal.rs`; store `src/memory.rs`, new `src/memory/projection_rebuild.rs`, `src/migrations.rs`, new `migrations/0121_memory_projection_rebuild_receipts.sql`, `tests/schema_v1.rs` necessary new-schema/rewind expectations, and new `tests/memory_migration_rollback.rs`. Evidence changes are confined to this directory and its parent README. All adapters and other owner paths remain unstaged.

## Proof and exact commands

Each [command receipt](receipts/) contains argv, exact cwd, UTC start/end, exit, test summaries, full gzip output and raw/archive SHA-256, recorder identity and pre-command source hashes. `run.py` captures the commands without altering historical receipts. Initial own-development failures are retained and labelled; they are not mutation kills. Full command/results tables are generated below after all runs finish.

The composed fixture target covers actual loopback qualification success, provider-only egress, bearer/response redaction, failed add/cognify/canary/malformed/timeout preserving the prior active pointer, stale preview before egress, in-flight ledger change with an unlocked store, competing same/distinct keys, completed replay after ledger change/restart, immutable receipt failure/retry, absent/disabled/no-supplier refusal and operator/observer parity. It performs no real alias lookup.

MUT-007 first uses a real Client canary failure fixture. Its mutation witness then injects a deliberately adverse qualification through the same composed application helper with valid digest, added=true, cognified=true, canary_passed=false. Removing only the shared authoritative canary guard compiles and wrongly advances generation 1 to 2, failing the unchanged-pointer assertion. Exact restoration passes. The historical store-only kill is not used as current composed proof. All seven initial mutants were killed/restored before F4, then the entire set was repeated with the final rehearsal test source present; final baseline/seeded/restored hashes are in `final-mutants.json` and `final-mutant-source.json`.

F4 forces a duplicate trigger after four CREATE TABLE statements inside unmodified migration 0120 on a disposable installed v119 fixture. Failure leaves user_version 119, the entire schema and ordered ledger/history/FTS rows byte-equal. Removing only the synthetic trigger enables a clean 119→121 retry, one valid typed eligibility backfill, preserved frozen-binding hash, FTS rebuild/search, FK and immutable-trigger checks, forward-version refusal and reopen. Fresh final schema and separate generic/typed predecessor-upgrade fixtures also pass. This is an SQL-phase failure, not a claimed forced Rust-backfill failure; successful backfill is observed after retry. Backup/recovery is exercised separately through supported synthetic snapshot/restore. After numbering reconciliation, retest the selected real predecessor and final chain.

Synthetic census/rehearsal detail, exact product command shapes and copied-data prerequisites: [runnable preparation](2026-10-03-10-43-plan-synthetic-census-rehearsal.md). Explicit artificial cohort 55, current N=57, current approved untombstoned 47, typed 41, provider 31, local-only 10, generic current six, pending five, tombstoned five. Verified same-Realm snapshot copied only a synthetic database plus manifest into an isolated temporary root. Prior immutable rows/source rows preserved, three appended receipts, stale conflict, approved-head change, frozen replay after restart, zero purges and all roots deleted are recorded. Real historical/current corpus inputs remain unavailable; the 55 pending slots are untouched.

## Remote/default divergence and migration reconciliation

Actual `git ls-remote --symref origin HEAD` pins in `integration-pins.json`:

| Repo | Actual remote master | Starting-candidate divergence |
| --- | --- | --- |
| Module | `4d365dd5de649c68aaabe8c2b67351ae7dd5dbb0` | `29c81346`: 1 behind / 4 ahead; merge base `8acdbd17e83d9d7ec545221c4b957e0325916997` |
| Root | `f6ffaa60f789f196b420fa2e79ff51cdf456683f` | Docs `7d192808`: 12 behind / 3 ahead; merge base `f77c3b31bd73148a93cb3370d0ae34bf807fe2ff` |

Module default advance is accepted ASMA-8113 PR #283, changing only Cargo.lock to refresh yoke-derive 0.8.3→0.8.4. This branch preserves the inherited lock and consumes the exact correction pin/status for integration routing, without duplicating remediation. A supported local replay/rebase/cherry-pick/branch-merge operation remains missing in the installed ASMA CLI. `pull` cannot safely reconcile this diverged integration branch under the current no-raw-operation constraints; `merge-pr` is an attested squash publication operation, not the required local join. No fetch/rebase/merge or lock rewrite was performed. That accepted correction must join before final delivery qualification; advisories are not claimed passed here.

The local chain is contiguous 0001–0121. Existing 0120 remains byte-exact; new 0121 is unshipped task-local receipt storage. External 8187’s unmerged 0120–0122 remain candidate numbers. Serialize independently accepted joins, inspect the resulting actual maximum/predecessor, reconcile unshipped numbers and registration/schema/rewind/export/client fixtures through the supported workflow, then rerun fresh/upgrade/forced rollback, migration integrity, all affected mutations and combined gates. No number or second migration writer is claimed. The existing migration reconciliation scaffold remains applicable, with this extra tentative receipt migration included.

## Frozen docs join finding

The external docs worktree remains exact at accepted `7d192808`. Skill source and mirrored adapter SHA-256 both equal `0e1b1948818058c6131988a063ebcffbf5e253836f29d6bbe5833c0dcb1dc039`; runbook SHA-256 `168bc2b664644903a7d80968f6e032b4e46c65ce91f131b13c54f8aa3acf1eb6`. Public operation/config names match: `kontor_memory_projection_rebuild`, `memory-projection-rebuild`, `credential_alias`, `memory-cognee.json`, `projection_conflict`, `projection_unavailable`. Internal Rust trait operation name is not a public-doc omission.

New behavioral drift for TPM/8157 owner: both `.github/skills/experience-memory/SKILL.md` and its `.agents` mirror lines 160–161 say fresh inputs return projection_unavailable and perform no rebuild. Runbook `_docs/ai-orchestration/operations/2026-09-12-10-10-runbook-kontor-experience-memory.md` lines 206–208 carries the equivalent caveat. The composed application now performs stage→qualify→activate and replays completed success; unavailable remains correct when composition is absent/disabled or qualification fails. Update those exact caveats and idempotency/CAS behavior while retaining the real supplier/runtime activation gates. No docs branch modification occurred.

## Cross-lane ownership and order

Consumed from the existing TPM coordination record; statuses are observations, not new acceptance. No external branch is merged or modified and no new owner is appointed.

| Lane | Existing writer/contact | Sole integration authority or unresolved designation | External candidate/status |
| --- | --- | --- | --- |
| 8187 | `19b7107d-ce1c-46e5-89e4-0d2413a1c50d` | 8186 TPM; no shared join claim by writer | `99b237201e71bbeacea726df09f32325428b58a5`, unaccepted P2, isolated replay gap |
| 8196 | `ba4ded2e-4943-45d3-b267-7eca5ee5440f` | No integrator designated; 8190 TPM `74250c1e-7540-440a-9809-25952915fd2a` coordinates | `940138c5578b67c1590587a0d29c10390a58847a`; QA evidence `f77c32501c92aecad1ee10180edbfd3158cd06fb` PASS, AUD pending in consumed record |
| 8114 | `27bd6bb0-95ba-4bbb-bf74-bc554293ed54` | 8113 TPM/LSA, writer only | `04a8d8c09d291a7445af7d96c4d6b3a776181135`, frozen unaccepted |
| 8340 | `1da997b5-a5ab-49a7-ab4e-38ac0bbdfe15` | 7869/8340 coordination, no separate integrator designation | `d3fa8068`, supplied abbreviated unaccepted candidate |
| 8341 | `cf1b2891-3a4b-4acb-abc7-a4bfe960afa9` | Same seat expressly designated sole integrator of its lane | `4a3eb2438fa375063db2ab0399c9afa4cacdf7a6`, unaccepted security hold |
| 8155 / 8159 | This persistent writer | Sole epic integration writer | Accepted memory/launch pins plus this local candidate; independent review pending |

Proposed serialized order: baseline PR #278/#280 → accepted 8187 → accepted 8196 → accepted 8114 → accepted 8340 → accepted memory 8156 → accepted launch 8158/composition. Join accepted 8341 Cargo/bootstrap before final lock/artifact freeze, and accepted 8113 PR #283 correction before final dependency qualification; docs join by its independent accepted pin. TPMs must establish acceptance, one integration owner and a supported operation before shared joins. Missing designations are unresolved, not a claim by this seat.

## Remaining reserved and operational frontiers

[Igor packet](2026-10-03-10-32-analysis-igor-cognee-credential-binding.md) supplies the exact proposed interface/startup diff, redacted alias/consumer/Realm/root/endpoint/target binding, alternatives, account child-only implications, egress policy, synthetic proof, immutable primary-source/licence hashes and later prerequisite list. Real supplier and target registration/secret use remain reserved for Igor/account-security ownership. Memory code has no Keychain/env lookup or child-environment extraction; ordinary startup stays disabled by default.

Release still needs independent review/acceptance of this combined change and any subsequent join, migration/default/dependency reconciliation, approved real credential/provider/egress/image/licence scope, accepted real historical/current corpus provenance/retention/teardown, the 24-case analogue benchmark, backup/rollback/isolation on the reviewed artifact, precise runtime activation authorization, deployment/restart/readback, exact default/opt-out/frozen prompt proof and the original [LSA section-6 evidence inventory](../2026-10-03-01-11-plan-release-readback-and-evidence.md). Synthetic qualification does not satisfy live quality/latency or release gates. No review polling or self-review is performed.

## Final outcomes and command inventory

The exact scoped source manifest is SHA-256 `e0e1f5107ba42b2c4b748fcdb8bad6d75e554edcc378c535c86aa8f84c0015d5` (761 files). [Candidate source hashes](receipts/candidate-source.json) enumerate every changed/new source file and protected file identities. The final mutant, loopback, store-library, schema, composed/recovery, runtime-contract, Jira-fixture, fmt and whitespace receipts carry that same manifest. Initial F4/synthetic runs predate the added rehearsal target or census-manifest scope; their tested production code bytes are unchanged and exact pre-command manifests are retained.

F1 seven composed fixtures pass; all seven final mutants compile, fail behaviorally and restore exactly. F4 passes forced rollback, fresh schema and predecessor upgrade plus separate backup/recovery proof. F3 executed breadth totals **1,551 passing tests, one inherited ignored case and one inherited failing publication assertion** across the final listed targets; this excludes duplicate baselines/mutation/restoration runs. Build, clippy, fmt, whitespace, OpenAPI/console schema parity and selective deny policy pass. **F3 is blocked, not PASS.** Full native lane/full console/full heavy-store completion is not claimed.

| Receipt | Exact command | Exit / result |
| --- | --- | --- |
| [f1-composed-baseline](receipts/f1-composed-baseline.json) | `cargo test -p kontor-daemon --test projection_rebuild --locked --offline` | 101 — FAILED. 0 passed |
| [f1-composed-envelope-corrected](receipts/f1-composed-envelope-corrected.json) | `cargo test -p kontor-daemon --test projection_rebuild --locked --offline` | 0 — ok. 6 passed |
| [f1-composed-final-baseline](receipts/f1-composed-final-baseline.json) | `cargo test -p kontor-daemon --test projection_rebuild --locked --offline` | 101 — FAILED. 6 passed |
| [f1-composed-atomicity-corrected](receipts/f1-composed-atomicity-corrected.json) | `cargo test -p kontor-daemon --test projection_rebuild --locked --offline` | 0 — ok. 7 passed |
| [primary-upstream-inspection](receipts/primary-upstream-inspection.json) | `python3 docs/evidence/ASMA-8159/production-prep/inspect-upstream.py` | 0 — PASS |
| [f4-migration-0120-forced-rollback](receipts/f4-migration-0120-forced-rollback.json) | `cargo test -p kontor-store --test memory_migration_rollback --locked --offline -- --nocapture` | 0 — ok. 1 passed |
| [f4-v119-upgrade](receipts/f4-v119-upgrade.json) | `cargo test -p kontor-store --test schema_v1 v120_upgrade_preserves_generic_ledger_and_enforces_immutable_memory_receipts --locked --offline -- --exact` | 0 — ok. 1 passed |
| [f4-fresh-schema](receipts/f4-fresh-schema.json) | `cargo test -p kontor-store --test schema_v1 an_empty_database_migrates_to_the_current_schema_version --locked --offline -- --exact` | 0 — ok. 1 passed |
| [synthetic-census-rehearsal](receipts/synthetic-census-rehearsal.json) | `cargo test -p kontor-daemon --test synthetic_census_rehearsal --locked --offline -- --nocapture` | 0 — ok. 1 passed |
| [f3-clippy-workspace-contract](receipts/f3-clippy-workspace-contract.json) | `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 — PASS |
| [f3-build-workspace](receipts/f3-build-workspace.json) | `cargo build --workspace --locked --offline` | 0 — PASS |
| [f3-fast-lane-plan](receipts/f3-fast-lane-plan.json) | `env CARGO_NET_OFFLINE=true python3 scripts/test-lanes.py --lane fast --base 29c81346 --dry-run` | 0 — PASS |
| [f3-light-and-contract-breadth](receipts/f3-light-and-contract-breadth.json) | `cargo test -p kontor-core -p kontor-api -p kontor-cli -p kontor-mcp -p kontor-memory-cognee -p kontor-tests-contract --locked --offline` | 0 — 569 passed across 41 result lines; full target counts in receipt |
| [f3-heavy-lane-plan](receipts/f3-heavy-lane-plan.json) | `env CARGO_NET_OFFLINE=true python3 scripts/test-lanes.py --lane heavy --base 29c81346 --dry-run` | 0 — PASS |
| [f3-cargo-deny-policy](receipts/f3-cargo-deny-policy.json) | `cargo deny --offline check licenses bans sources` | 0 — PASS |
| [actual-default-heads-and-docs](receipts/actual-default-heads-and-docs.json) | `python3 docs/evidence/ASMA-8159/production-prep/inspect-integration.py` | 0 — PASS |
| [supported-git-operations](receipts/supported-git-operations.json) | `asma git --help` | 0 — PASS |
| [f3-generated-console-contract-parity](receipts/f3-generated-console-contract-parity.json) | `python3 docs/evidence/ASMA-8159/production-prep/check-contract-parity.py` | 0 — PASS |
| [f3-safe-isolation-inventory](receipts/f3-safe-isolation-inventory.json) | `python3 docs/evidence/ASMA-8159/production-prep/safety-inventory.py` | 0 — PASS |
| [f3-full-safe-loopback](receipts/f3-full-safe-loopback.json) | `cargo test -p kontor-daemon --test loopback_api --locked --offline -- --test-threads=4` | 0 — ok. 488 passed |
| [f3-store-library](receipts/f3-store-library.json) | `cargo test -p kontor-store --lib --locked --offline` | 0 — ok. 25 passed |
| [f3-fmt](receipts/f3-fmt.json) | `cargo fmt --all -- --check` | 0 — PASS |
| [f3-composed-memory-and-recovery](receipts/f3-composed-memory-and-recovery.json) | `cargo test -p kontor-daemon --test projection_rebuild --test experience_memory --test recovery_security --locked --offline -- --test-threads=4` | 0 — ok. 2 passed; ok. 7 passed; ok. 6 passed |
| [f3-whitespace](receipts/f3-whitespace.json) | `git diff --check` | 0 — PASS |
| [f3-full-schema-v1](receipts/f3-full-schema-v1.json) | `cargo test -p kontor-store --test schema_v1 --locked --offline -- --test-threads=4` | 0 — ok. 65 passed |
| [f3-runtime-synthetic-contracts](receipts/f3-runtime-synthetic-contracts.json) | `cargo test -p kontor-runtime-paseo -p kontor-runtime-codex -p kontor-runtime-ao --test contract --locked --offline` | 0 — ok. 60 passed; ok. 22 passed; ok. 285 passed |
| [f3-jira-fake-boundary](receipts/f3-jira-fake-boundary.json) | `cargo test -p kontor-jira --test credential_scope --test native_connector --locked --offline` | 0 — ok. 2 passed; ok. 20 passed |
| [f3-publication-immutable-boundary](receipts/f3-publication-immutable-boundary.json) | `cargo test -p kontor-store --test publication_attestations_immutable --locked --offline` | 101 — FAILED. 0 passed |

All commands above run from the exact task checkout. Native fast/heavy rows are dry-run plans only; they earn no executed-lane credit. The full lane was not invoked, including with `--dry-run`, because its implementation delegates to `verify-tree.py` before honoring dry-run and executes workspace tests, audits and installation. Selective policy command checks licenses/bans/sources only; it does not claim advisories. Upstream primary inspection performs bounded public immutable source GETs only.

### Final mutation witnesses

Run command: `python3 docs/evidence/ASMA-8159/production-prep/run.py mutants final-`. Initial ordered set used `.../run.py mutants` before F4. Each witness below ran baseline → seeded → exact restored with the same argv. Baseline and restoration each exit 0 with one passing executed witness; seeded exit 101 after compilation, with the named behavioral failure. Every restored file hash equals its baseline hash. [Full final patch/source/result identities](receipts/final-mutants.json); [baseline source](receipts/final-mutant-source.json). The initial set remains in `mutants.json`.

| Mutant | Exact witness argv | Seeded failure / result |
| --- | --- | --- |
| MUT-007 | `cargo test -p kontor-daemon --test projection_rebuild --locked --offline -- composed_adverse_qualification_cannot_activate_before_canary --exact` | KILLED; `composed_adverse_qualification_cannot_activate_before_canary`; [baseline](receipts/final-mut-007-baseline.json), [seeded](receipts/final-mut-007-seeded.json), [restored](receipts/final-mut-007-restored.json) |
| MUT-001 | `cargo test -p kontor-core --test experience_memory --locked --offline -- strict_experience_roundtrip_and_non_memory_eligibility --exact` | KILLED; `strict_experience_roundtrip_and_non_memory_eligibility`; [baseline](receipts/final-mut-001-baseline.json), [seeded](receipts/final-mut-001-seeded.json), [restored](receipts/final-mut-001-restored.json) |
| MUT-002 | `cargo test -p kontor-store --lib --locked --offline -- memory::experience_tests::malicious_tuple_project_current_approval_tombstone_hash_policy_and_kind --exact` | KILLED; `memory::experience_tests::malicious_tuple_project_current_approval_tombstone_hash_policy_and_kind`; [baseline](receipts/final-mut-002-baseline.json), [seeded](receipts/final-mut-002-seeded.json), [restored](receipts/final-mut-002-restored.json) |
| MUT-003 | `cargo test -p kontor-store --lib --locked --offline -- memory::experience_tests::exact_budget_32768_32769_unicode_escaping_and_skip_oversized_top --exact` | KILLED; `memory::experience_tests::exact_budget_32768_32769_unicode_escaping_and_skip_oversized_top`; [baseline](receipts/final-mut-003-baseline.json), [seeded](receipts/final-mut-003-seeded.json), [restored](receipts/final-mut-003-restored.json) |
| MUT-004 | `cargo test -p kontor-memory-cognee --test fixtures authoritative_rehydration_rejects_leakage_and_never_trusts_cognee_text --locked --offline -- --exact` | KILLED; `authoritative_rehydration_rejects_leakage_and_never_trusts_cognee_text`; [baseline](receipts/final-mut-004-baseline.json), [seeded](receipts/final-mut-004-seeded.json), [restored](receipts/final-mut-004-restored.json) |
| MUT-005 | `cargo test -p kontor-daemon --test loopback_api experience_launch::degraded_launch_never_lists_the_corpus_and_timeout_keeps_store_unlocked --locked --offline -- --exact` | KILLED; `experience_launch::degraded_launch_never_lists_the_corpus_and_timeout_keeps_store_unlocked`; [baseline](receipts/final-mut-005-baseline.json), [seeded](receipts/final-mut-005-seeded.json), [restored](receipts/final-mut-005-restored.json) |
| MUT-006 | `cargo test -p kontor-daemon --test loopback_api experience_launch::root_launch_contains_exact_frozen_canonical_bytes_and_downstream_cites_same_binding --locked --offline -- --exact` | KILLED; `experience_launch::root_launch_contains_exact_frozen_canonical_bytes_and_downstream_cites_same_binding`; [baseline](receipts/final-mut-006-baseline.json), [seeded](receipts/final-mut-006-seeded.json), [restored](receipts/final-mut-006-restored.json) |

### Accepted-source defect and mandatory stop

Exact command: `cargo test -p kontor-store --test publication_attestations_immutable --locked --offline`. It compiled, then exited 101: `recorded_publication_attestations_are_immutable_at_the_database_boundary` fails at `crates/kontor-store/tests/publication_attestations_immutable.rs:18`, actual 121, expected hard-coded 119. That file is byte-identical to accepted `831dd7c1`, SHA-256 recorded in [source attribution](receipts/inherited-publication-source.json); accepted source already declared schema 120. The stale assertion therefore predates this new 121 receipt migration. The actual immutability body is never reached, so this target earns no immutability proof. Full schema tests separately execute existing publication trigger checks.

Per the user’s accepted-source-defect rule, source changes and further qualification stopped at this finding. The accepted test is not modified. TPM must route its owner’s correction, including checking other predecessor-rewind fixtures, then admit renewed affected combined breadth on the resulting frozen tree. Remaining full store/heavy/full-lane checks are unperformed, not inferred passed. This candidate is committed for review/routing with that blocker; it does not certify complete combined qualification.

### Exact excluded boundaries and other blockers

- Entire `kontor-daemon --lib`, especially all `usage.rs` tests, remains excluded on macOS: the inherited provider-home Claude path invokes `/usr/bin/security find-generic-password`. [Isolation source inventory](receipts/safety-inventory.json) records TempDir harness, empty provider-home approval set and scripted exact-provider reporters used by full loopback.
- Full loopback’s inherited ignored case is `a_configured_jira_boundary_distinguishes_historical_from_native_completion`, marked superseded by kontor-jira native connector contract tests. It is not executed or credited; selected native connector/credential fixtures pass separately.
- `kontor-runtime-ao`, `kontor-runtime-codex`, `kontor-runtime-paseo` live targets are unexecuted; only their explicit synthetic `contract` targets ran. Unisolated blanket accounts/e2e tests, including `tests/e2e/pilot_sections/runtime.rs` SystemKeychain use, are not promoted to executed coverage.
- Console app typecheck/vitest/production audit remain unexecuted because `apps/console/node_modules` is absent and installation is not authorized. Existing installed generator and standalone schema typecheck pass; these do not substitute for full app gates.
- Accepted remote ASMA-8113 PR #283 lock correction awaits a supported join; inherited yanked 0.8.3 remains in this candidate. No duplicate dependency correction or advisory waiver is made.
- Tentative 0120/0121 versus external 8187 0120–0122, supported local replay/join capability, final lane designations/order and docs caveat update remain integration frontiers.
- Real credential supplier/alias/consumer/target binding, production corpus/55-cohort/current-N provenance, image/provider/egress/licence qualification, 24-case live benchmark and backup/rollback/isolation/activation/deployment/readback remain explicitly gated.

The inherited store readback `adapter_available=false` is preserved and is not used as a live readiness certificate; composed availability is demonstrated by the actual synthetic transport call and canary witness. Any release-facing availability semantics adjustment needs owning-source disposition and renewed parity proof.

Initial own-development compile/envelope/fault-status expectation failures are retained in their development receipts and were corrected before mutation qualification. They are not inherited defects, seeded mutants or executed-gate passes.

Only additive owned changes are staged; adapters, Cargo.lock, accepted 0120 SQL, account resolver, external docs checkout and historical receipts remain unchanged. The commit SHA/tree are reported in the writer’s final handoff to avoid a self-referential commit hash. Independent review follows through TPM; this writer stops after the checkpoint.


Initial checkpoint whitespace command: `git diff --cached --check`, exit 2 on an extra blank line at EOF in this new report; [initial receipt](receipts/checkpoint-staged-whitespace.json). The report whitespace was corrected; the passing rerun is [checkpoint-staged-whitespace-corrected](receipts/checkpoint-staged-whitespace-corrected.json). Staging is restricted to the 13 enumerated owned source paths plus this new evidence subtree and parent README.
