# Combined local integration evidence — ASMA-8159

> **Date:** 2026-10-03 01:11 Europe/Oslo
> **Status:** 🔴 Draft — local candidate; independent combined review pending
> **Category:** report
> **Scope:** MEM-04 / TASK-004 of ASMA-8155; Paseo-direct; Carasent-ASMA/asma-rs-kontor
> **Summary:** Combined affected-source verification and all seven compiled behavioral mutation kills, frozen docs join findings, and prepared production-frontier worksheets. Accepted runtime source is unchanged; no live or release acceptance is claimed.

## When to load

Load for TPM admission of independent review on the exact final local SHA, for
verification/mutation evidence, or for later authorized production-frontier work.
The [package index](README.md) routes every worksheet and receipt. The writer
stops after the local commit and handoff, without polling or reviewing this work.

## Authority, branch and frozen inputs

Task ASMA-8159, integration owner/single persistent writer; workspace
wks_b22a010864eaa006. Checkout:
/Users/igor/.paseo/worktrees/0n8yzbno/feat-asma-8155-kontor-experience-memory-and.
Branch: feat/ASMA-8155-kontor-experience-memory-and-analogue-recall.

The complete preserved execution contract was read before editing; SHA-256
7ee35150e2e90cdc4fab83bac999ecd2cef6286d236df128b0ad5b8c211d828b matches the
supplied pin. Sections 3 (8159), 4, 5, 6 and 7 govern. The existing canonical
plan/TASK-004 and unchanged Goal remain authoritative; no parallel epic/ADR,
Jira change, orchestration prerequisite or source remediation was introduced.
WORKING_ROLE is ENGINEER, defaulted. Only adapter-workspace role lines were
checked; no application environment or secret was inspected.

| Pin | Exact SHA | Review/status provenance |
| --- | --- | --- |
| Base | 8acdbd17e83d9d7ec545221c4b957e0325916997 | PR #278/#280 baseline; not installed-source proof |
| ASMA-8156 | 655e07ead8b87bebf3a5d43ddccc15c860114e05 | User-supplied frozen QA+Audit PASS |
| ASMA-8158 / initial combined HEAD | 831dd7c1dc95a57258c75f5896bff5b76f3830fa | User-supplied frozen QA+Audit PASS; auditPassed=true |
| ASMA-8157 docs | 9b5b538c269deca55058f3fdd9efc66ee8046ab9 | User-supplied independent docs review PASS |

[provenance.json](receipts/provenance.json) records exact trees, contract/plan
hashes and toolchain. [ancestry.json](receipts/ancestry.json) proves both accepted
module commits are reachable, while the named external candidates are not.
Initial HEAD tracked state was clean; only .agents/, .asma/, .cursor/, AGENTS.md
and CLAUDE.md were untracked adapters. They remain excluded from staging.

## What is integrated

The existing linear combined source includes the strict typed experience and
evidence contract, canonical security/approval/history, authoritative candidate
rehydration and bounded FTS/semantic selection, frozen recall/replay metadata,
local migration 0120/schema 120, immutable projection stage/activation primitives,
eight typed registry/HTTP/CLI/MCP surfaces and generated console contract. It
also includes the bounded Cognee fixture transport, disabled configuration and
actual root-launch/downstream/lost-caller-ack prompt witnesses from 8158.

This task adds only docs/evidence/ASMA-8159/. Runtime, migration, lock, contracts,
configuration and inherited 8156/8158 evidence remain exact. The separate docs
candidate is joined by immutable reference/name comparison, not by copying or
modifying its repository. This evidence commit's SHA/tree are supplied in the
final handoff; they cannot be embedded self-referentially in the commit.

Initial combined Git tree: ae9bcea100575e5bfa295cf57865139b08b9ab70.
Tracked non-evidence source manifest: 81c6d5ac5e55c809deb367d47e2b064c2cac6878daf314d63858099d3ca9a239
(1142 files; SHA-256 of Python json.dumps(files, sort_keys=True).encode(), each
value an exact file SHA-256). [source-manifest.json](receipts/source-manifest.json)
pins every file; final restoration/integrity checks are in
[final-validation.json](receipts/final-validation.json).

| Key source/artifact | Exact SHA-256 |
| --- | --- |
| Cargo.toml | 00e391943e2bed3edc387693798856e385f731aee740516d9334dcb43b263a5c |
| Cargo.lock | df30ba74da6a3ffd42cf5c0c3fc9ba4712e2f8cf96a6a09b4154573b819c5683 |
| crates/kontor-core/src/memory.rs | d441653116bfeb841fe322ebffb84820a843a1ed47cffff313f2bd06c2d0876f |
| crates/kontor-store/src/memory.rs | 6ef3e31eec386f70fbe3099f63555299a6ae6ef4a90a32b45b5b353cad126a88 |
| crates/kontor-store/migrations/0120_experience_memory_projection.sql | bbad30ea8f311c79a5c044b863e2998ddafa7be9ccc7322aed39a639ed532031 |
| crates/kontor-store/src/migrations.rs | 716814bbe2e1f5c87ab6e84b49a4d12c6ad2b2f19d8d1048cb918d08d753b278 |
| crates/kontor-memory-cognee/src/lib.rs | f461effcda17a460a0939ef2bb077f74de1bd0b26ee5b6c44b997fcd3ec84c40 |
| crates/kontor-daemon/src/applications.rs | 0a15e9f04c77edfd34b56ef6a0be1f70b5c142b4cdc1d795bc0e5d2a7292617a |
| crates/kontor-mcp/src/registry.rs | a3979118e6ba1b1f67f64886f0aa8dace3aa321b7618ab96565a801c97a27c69 |
| crates/kontor-api/contract/openapi.json | 43a09454b1661bab879c5cc76a9b9a9f6d22d17116d22329a9f21bee22a2ea6b |
| apps/console/src/api/schema.d.ts | f139d5ac32ddfc6d12fe7f07d4e22f9c9256c555a8db11e2ed00f8918afa5af4 |
| docs/CONFIGURATION.md | 52291614fab66dce96dba72ff8559107d2dc364874366735d32e25bb598ac373 |

## Combined commands and results

All 17 checks passed; **293 tests passed, zero failed**. Build covers all
seven affected crates and their dependencies; Clippy covers their all-targets
compilation with warnings denied. Explicit synthetic targets exclude the
inherited macOS daemon lib/usage suite that touches Keychain. No full workspace/
desktop/console runtime suite or real analogue benchmark is represented here.

Every command below runs in the exact checkout above, without source changes.
[verification.json](receipts/verification.json) records exact argv/cwd, exits,
timings, executed/failed-test summaries and full raw/gzip log hashes. No empty
or zero-test invocation is credited as a test pass.

| Check | Exact command | Result |
| --- | --- | --- |
| combined-build | `cargo build -p kontor-core -p kontor-store -p kontor-api -p kontor-daemon -p kontor-cli -p kontor-mcp -p kontor-memory-cognee --locked --offline` | exit 0 |
| core | `cargo test -p kontor-core --lib --test experience_memory --locked --offline` | exit 0; 47 passed |
| store-memory | `cargo test -p kontor-store --lib memory:: --locked --offline` | exit 0; 20 passed |
| schema | `cargo test -p kontor-store --test schema_v1 --locked --offline` | exit 0; 65 passed |
| backup | `cargo test -p kontor-store --test backup_snapshot --locked --offline memory_ledger_and_import_evidence_restore_while_fts_is_rebuilt` | exit 0; 1 passed |
| api-mcp | `cargo test -p kontor-api -p kontor-mcp --lib --tests --locked --offline` | exit 0; 110 passed |
| http-cli | `cargo test -p kontor-daemon --test experience_memory -p kontor-cli --test memory_parity --locked --offline` | exit 0; 4 passed |
| registry-parity | `cargo test -p kontor-tests-contract --test mcp_parity --test mcp_cardinality --test mcp_mutants --locked --offline` | exit 0; 34 passed |
| cognee-fixtures | `cargo test -p kontor-memory-cognee --test fixtures --locked --offline` | exit 0; 5 passed |
| launch-fixtures | `cargo test -p kontor-daemon --test loopback_api experience_launch:: --locked --offline` | exit 0; 5 passed |
| context-approved | `cargo test -p kontor-daemon --test loopback_api --locked --offline -- resolving_a_task_context_tracks_approved_memory_and_returns_no_content --exact` | exit 0; 1 passed |
| context-generic | `cargo test -p kontor-daemon --test loopback_api --locked --offline -- generic_corpus_past_the_general_ceiling_is_excluded_by_typed_recall --exact` | exit 0; 1 passed |
| combined-clippy | `cargo clippy -p kontor-core -p kontor-store -p kontor-api -p kontor-daemon -p kontor-cli -p kontor-mcp -p kontor-memory-cognee --all-targets --locked --offline -- -D warnings` | exit 0 |
| fmt | `cargo fmt --all -- --check` | exit 0 |
| accepted-whitespace | `git diff --check 8acdbd17 HEAD` | exit 0 |
| working-whitespace | `git diff --check` | exit 0 |
| dependency-policy | `cargo deny --offline check licenses bans sources` | exit 0 |

The existing cached generator and global TypeScript executable were invoked
directly; no install occurred. Regeneration is byte-identical to the accepted
console declaration, and standalone strict typechecking passed. This does not
repeat the inherited full console suite. The generated schema hash remains
f139d5ac32ddfc6d12fe7f07d4e22f9c9256c555a8db11e2ed00f8918afa5af4.
[supplemental.json](receipts/supplemental.json) contains these exact commands and
all eight generated CLI help readbacks, each exit 0:

| Check | Exact command | Result |
| --- | --- | --- |
| client-generate | `node /Users/igor/.npm/_npx/f1e70922fc87e24a/node_modules/openapi-typescript/bin/cli.js crates/kontor-api/contract/openapi.json -o target/mem04-schema.d.ts` | exit 0 |
| client-byte-parity | `cmp apps/console/src/api/schema.d.ts target/mem04-schema.d.ts` | exit 0 |
| client-typecheck | `/opt/homebrew/bin/tsc --noEmit --strict --skipLibCheck --lib ES2022 apps/console/src/api/schema.d.ts` | exit 0 |
| kontor_experience_propose-help | `target/debug/kontor experience-propose --help` | exit 0 |
| kontor_memory_recall_preview-help | `target/debug/kontor memory-recall-preview --help` | exit 0 |
| kontor_memory_recall_freeze-help | `target/debug/kontor memory-recall-freeze --help` | exit 0 |
| kontor_memory_recall_get-help | `target/debug/kontor memory-recall-get --help` | exit 0 |
| kontor_memory_projection_preview-help | `target/debug/kontor memory-projection-preview --help` | exit 0 |
| kontor_memory_projection_get-help | `target/debug/kontor memory-projection-get --help` | exit 0 |
| kontor_memory_projection_rebuild-help | `target/debug/kontor memory-projection-rebuild --help` | exit 0 |
| kontor_experience_classify-help | `target/debug/kontor experience-classify --help` | exit 0 |

The accepted 8158 evidence carries the full cargo-deny advisory failure for
inherited yanked yoke-derive 0.8.3. That full advisory gate was not rerun here;
this candidate reruns bans/licenses/sources only and preserves the inherited
finding for its owner. No source defect was found by combined verification.

## MUT-001–007 outcomes

Reproduction: python3 docs/evidence/ASMA-8159/run-verification.py --mutations.
One mutant was seeded at a time on final source. Every seeded test compiled,
then failed its behavior assertion; every baseline and restored witness passed
exactly one test. Exits are 0 / 101 / 0 for all seven. No compile failure earns a
kill. No current survivor or invalid compile occurred. Historical receipts and
reviewed outcomes remain preserved separately; they were never overwritten.

[mutations.json](receipts/mutations.json) pins exact source/patch/log hashes,
argv, failed assertions, baseline/seeded/restored source-manifest hashes and
virtual seeded Git trees. Virtual trees use normal Git SHA-1 blob/tree encoding
of the initial tracked tree with one substituted source blob; the computed
baseline equals the actual Git tree. No Git index/object writes or mutant
staging were used. New evidence is outside that inherited tracked tree.

| Mutant | Source decision and exact witness | Result |
| --- | --- | --- |
| MUT-001 | Admit operational_gap in typed eligibility; strict_experience_roundtrip_and_non_memory_eligibility | KILLED; compiled; 0/101/0; exact bytes restored |
| MUT-002 | Omit project predicates from exact candidate SQL; memory::experience_tests::malicious_tuple_project_current_approval_tombstone_hash_policy_and_kind | KILLED; compiled; 0/101/0; exact bytes restored |
| MUT-003 | Admit one extra byte past 32768; memory::experience_tests::exact_budget_32768_32769_unicode_escaping_and_skip_oversized_top | KILLED; compiled; 0/101/0; exact bytes restored |
| MUT-004 | Replace canonical frozen block with trusted Cognee text; authoritative_rehydration_rejects_leakage_and_never_trusts_cognee_text | KILLED; compiled; 0/101/0; exact bytes restored |
| MUT-005 | Use list_memory as degraded prompt content; experience_launch::degraded_launch_never_lists_the_corpus_and_timeout_keeps_store_unlocked | KILLED; compiled; 0/101/0; exact bytes restored |
| MUT-006 | Omit root frozen canonical prompt block; experience_launch::root_launch_contains_exact_frozen_canonical_bytes_and_downstream_cites_same_binding | KILLED; compiled; 0/101/0; exact bytes restored |
| MUT-007 | Remove canary-success guard from authoritative activation; memory::experience_tests::projection_minimal_payload_failures_freshness_and_compare_and_swap | KILLED; compiled; 0/101/0; exact bytes restored |

### MUT-007 — killed on the owning final source

Source: crates/kontor-store/src/memory.rs:1865,
SqliteStore::activate_projection. Remove only the canary_passed rejection while
retaining added/cognified checks. The existing
memory::experience_tests::projection_minimal_payload_failures_freshness_and_compare_and_swap
witness reaches added=true / cognified=true / canary=false, and fails the
ProjectionUnavailable refusal assertion at line 2849 because the mutant accepts
activation. This is an actual synthetic authoritative activation, not a
manufactured transport qualification or a live activation claim.

Exact witness command (baseline, seeded and restored):

    cargo test -p kontor-store --lib --locked --offline -- memory::experience_tests::projection_minimal_payload_failures_freshness_and_compare_and_swap --exact

Baseline/restored source SHA-256:
6ef3e31eec386f70fbe3099f63555299a6ae6ef4a90a32b45b5b353cad126a88.
Seeded source SHA-256:
6d1b0cdc25144daaf61ba0b063eb071b03e9ced6f8499897512583ea3976bc1d.
Seeded virtual Git tree: 211dfe434f7c8bcef3077623469827c545eef77e.
No deferral of MUT-007 is needed. Real public rebuild/credential composition/
post-canary activation remains a separate unperformed production frontier.

## Frozen docs join and TPM routing

[Frozen join report](2026-10-03-01-11-report-frozen-docs-join.md) records exact
locations, eight operation names and five configuration keys. Findings:

- DOC-01: skill/runbook still call the now-accepted typed contract/names
  provisional; all eight final operation names are absent, and wire-valid
  examples remain pending. Route final references/examples to the docs owner.
- DOC-02: memory-cognee.json and its concrete transport keys/defaults and
  ordinary-startup/public-rebuild capability limits are absent from those docs.
- DOC-03: runbook still says the Claude catalog link/inventory refresh is owed;
  frozen 9b5b538c already contains both. Correct the stale source note through
  docs/loader owners; presence is not installed-runtime proof.

No conflicting literal spelling was found. Conceptual contracts align; all four
skill pairs are byte-identical. The docs branch remained clean at its exact pin.
No doc fix or acceptance override was made by this integration writer.

## External candidates remain external

These are LSA historical observations; obtain final accepted heads/status from
the existing owners before any shared integration. Ancestry proves the four
named candidate commits below are absent from this branch. ASMA-8340's observed
base is present through the baseline, but its uncommitted summary delta is not
joined or treated as a frozen candidate. No other owner's checkout was read or
modified for this reconciliation.

| Owner | Historical observed candidate | Shared integration frontier |
| --- | --- | --- |
| ASMA-8187 succession | 99b237201e71bbeacea726df09f32325428b58a5 | 0120–0122/schema122/export13, scanner/native successor/imported-evidence semantics |
| ASMA-8196 Core Team read API | f77c32501c92aecad1ee10180edbfd3158cd06fb | API/OpenAPI/application, exact generation/persona/occupancy attribution |
| ASMA-8114 topic/checkout correction | 94286eaa6010295b1c85f998bc9c60b6543f046a plus uncommitted final delta | Strict topic mutation/legacy archival reads and checkout persistence |
| ASMA-8340 summary composition | f95e206563bca88b6871f48528623441e5e1a231 base with uncommitted delta | Shared application/prompt join; summaries are handoff evidence, not auto-approved memories |
| ASMA-8341 Cargo/bootstrap | 4a3eb2438fa375063db2ab0399c9afa4cacdf7a6 | Lock/client/artifact freeze and qualified installer/readback; fake service capability earns no live credit |

The exact proposed serialized order and additive migration procedure are in the
[migration worksheet](2026-10-03-01-11-plan-migration-reconciliation.md). Local
memory 0120 is not rewritten. No number is claimed for reconciliation, including
0123; installed/shipped migrations and final accepted external heads decide it.
Historical plan 0095 memory references are stale; actual 0095 is immutable Jira
identity SQL. The docs runbook correctly warns it is occupied.

## Prepared production-frontier package and remaining obligations

| Prepared path | Outcome now / owed frontier |
| --- | --- |
| [Census/rehearsal](2026-10-03-01-11-plan-corpus-census-and-rehearsal.md) and historical-55-census.csv | Exact 55 pending slots and cohort/current-N reconciliation procedure. No item identity/hash invented; actual historical cohort artifact/live reads, copied database, history/aggregate hashes, supported tombstones and three seeds still owed. |
| [Migration reconciliation](2026-10-03-01-11-plan-migration-reconciliation.md) | Local 0120 versus unmerged 0120–0122, serialized owner coordination, immutable shipped SQL and contiguous final tests; no allocation/external join executed. |
| [24-case benchmark](2026-10-03-01-11-plan-analogue-benchmark.md) | Exactly 24 planned cases, frozen relevance/hash requirements, quality/zero-invalid gates and warm-scale latency samples; no labels/results or real embedding quality fabricated. |
| [Release readback/evidence](2026-10-03-01-11-plan-release-readback-and-evidence.md) | Deployment/restart/readback checklist and full section-6 evidence inventory; all production boxes unperformed. |

Remaining frontiers: independent combined QA/Spec Audit by TPM; DOC-01–03
routing; final accepted external heads/serialized shared integration; inherited
advisory-policy disposition; supported live credential/transport and public
rebuild/activation composition; pinned Cognee/provider/image/licence/dependency/
chunk-identity qualification; actual historical/current census and copied-live
rehearsal; supported live tombstones/seeds and separate authorized approvals;
24-case analogue quality and migration-scale warm latency; final safe lanes,
merge/artifacts/backup/deployment/restart/projection/binding/context/prompt proof.
These limit release/epic closure, not this completed local preparation package.
FTS-only delivery cannot satisfy the unchanged analogue/live Goal.

No source-fake blocker remains. No accepted source defect was silently repaired.
No push/PR/merge, Jira, live corpus/database/Cognee/provider/Keychain access,
installation/deployment, primary-checkout write, accepted-task branch change,
adapter staging or another-owner change occurred. Only writer-owned paths are
staged for the authorized local ASMA commit; the exact resulting SHA/tree and
files accompany the final handoff. The writer does not self-review or wait for
independent review.
