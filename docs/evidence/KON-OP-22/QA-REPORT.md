# KON-OP-22 QA report

Date: 2026-09-03

Status: complete local, committed-tree and live release verification passed.

## Passed gates

- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- Complete Rust workspace coverage as a composite run:
  - daemon loopback: 251 passed, 1 intentional superseded test ignored;
  - MCP journey: 2 passed after canonical connector fixture correction;
  - all library, migration, store, runtime, cross-crate contract and e2e pilot
    suites passed;
  - schema v83: 57 passed.
- `pnpm --filter kontor-console verify:api`
- `pnpm -r typecheck`
- `pnpm -r test`: 16 files and 296 tests passed.
- All new Jira JSON fixtures pass `jq empty`.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo audit`: no vulnerabilities; 19 repository-allowed warnings.
- `cargo deny check`: advisories, bans, licenses and sources passed.

The complete in-place verifier passed again after the epic-route correction:
`python3 scripts/verify-tree.py --mode inplace`. The first full run exposed one
stale daemon failure-path fixture that still offered the superseded direct epic
transition. Correcting that fixture to offer the first configured route hop made
the focused regression and the complete verifier pass. No production behavior
was weakened to accommodate the test.

The complete committed-tree archive verifier passed against code commit
`1ea52d9` plus reproducible-lock commit `1793144`. It exported `HEAD` without a
`.git` directory, regenerated and byte-compared `Cargo.lock`, then independently
ran formatting, workspace Clippy, all locked Rust tests, `cargo audit`,
`cargo deny check`, frozen pnpm installation, type checking, all console tests
and the production dependency audit.

The correction's clean-archive verifier independently passed the same complete
gate set against merge commit
`7c27f4d7a8e2aa37c1b1ddc576fe60387e95cf47`. It regenerated `Cargo.lock` in the
export and byte-compared it as identical before running the locked suite.

## High-risk regressions exercised

- exact task/high-stakes/epic workflow pin separation;
- entity-neutral observe/apply/refetch authority;
- canonical Jira identity migration and alias deduplication;
- resident task and epic convergence;
- no immediate retry loop on durable conflict replay or failed apply;
- startup and periodic reopen after completion reached `Done`;
- immutable cross-generation remediation evidence;
- atomic completion state/profile/wakes/receipt rollback;
- exact project-level reuse of one derived completion profile by two epics;
- mixed Jira and unrelated ticket links converge Jira without changing the
  fail-closed non-Jira-only contract;
- foreign TPM, foreign epic wake and mismatched replay refusal;
- API/OpenAPI/MCP route, schema and authority parity.
- exact validation of configured multi-hop routes, including duplicate-source,
  cycle, wrong-target and non-terminating-route refusal;
- exact full-selector route choice and per-hop intent/readback authority;
- legacy epic workflow revision-1 deserialization and canonical-hash stability;
- resident progression through every ASMA epic hop, followed by idempotent
  replay with no duplicate effect or conflict.
- identical mixed Link/Create materialization recovery, plus exact
  non-overlapping adoption of legacy pending-batch fragments with original
  batch ownership retained and effect-free replay.
- project-configured Jira create fields are included only for their declared
  issue kind, cannot override Kontor-owned structural fields, and preserve
  safe diagnostics by exposing field identifiers rather than Jira prose.

## Independent release audit

The final independent audit returned `APPROVE` with no remaining P0/P1 release
blocker. It found two blockers during review; both were corrected before the
approval:

- persisted completion-profile JSON is compared with the exact serialization
  used on insert, with a two-epic reuse regression;
- Jira reconciliation selects the canonical Jira subset when unrelated links
  coexist, while a non-Jira-only task retains its typed
  `unsupported_capability` refusal.

A follow-up independent audit of the multi-hop epic correction also returned
`APPROVE` with no P0/P1 blocker. It verified exact selector matching, unique
sources, cycle-free target termination, actual per-hop intent authority,
revision-1 compatibility and revision-2 bundle selection. Repeated warning
evidence from an unresolved historical revision-1 conflict is a non-blocking
P2 observability improvement; it does not create duplicate effects or wakes.

## Live verification

- Release hashes:
  - `kontor`: `6d84ff729bd88d981eb011cca5647f7512f156812e76a776e546c2ba9c678ab4`;
  - `kontor-daemon`: `248e7a385a5cafcaa990087b9d7f1e42fb914348a5506b34d35caf0f180b8744`;
  - `kontor-mcp`: `021fc79fa02a58fceabd8da6882bcb4ccbacc08c582bf30b6491d90e530716a0`.
- Database: schema 83, integrity `ok`, no foreign-key violations.
- Epic route evidence: four revision-2 intents, four confirmed, four distinct
  destinations, target `In Development (10214)`, zero open conflicts.
- Replay evidence: after the resident backstop, counts remained four intents,
  four confirmations and zero open conflicts.
- Task evidence: supported reconcile plans for `ASMA-8050` and `ASMA-8062`
  returned `converged: true` and empty diffs.
- Final materialization evidence: PR #162 merged as
  `d224b153bd6430f1df37e6c9a96a59cec9ab17b0`; the exact merge runs as PID
  `43055`. Original receipt `01a0683f-7579-7d82-90b7-00781902f8b3` and
  activation `01a06994-ff6e-7501-82d5-1199259eea08` created `ASMA-8088`,
  `ASMA-8089` and `ASMA-8090`; identical replay returned the same identities.
- Whole-epic Jira audit: all 21 task plans returned `converged: true` with
  empty diffs. The three new tasks and Epic `ASMA-7869` read back at
  `In Development`; the epic conflict list is empty.
- GitHub Actions were intentionally not run; Kontor's documented local release
  gates are authoritative.
