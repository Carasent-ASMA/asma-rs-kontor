# ASMA-8454 durable desk qualification candidate

> **Date:** 2026-10-11 16:00
> **Status:** 🟡 Frozen source candidate — Independent Review and release/readback pending
> **Author:** Codex — ASMA-8454 independent qualification owner
> **Category:** report
> **Scope:** ASMA-8449 TASK-005; Paseo-direct; disposable realms and isolated scripted fixtures
> **Summary:** Integrated acceptance, two authorized qualification repairs, frozen source provenance, backup/restore/replay, old-pin and declaration comparisons, selection rollback, whole-change mutations and the proposed coordinator release/readback. Independent Review and released-build acceptance remain separate gates.

---

## When to Load

Load when reviewing the ASMA-8454 candidate, reproducing its evidence or routing the coordinator's Independent Review and release gateway. Use the [operator runbook](../operations/2026-10-11-15-28-runbook-durable-desk-qualification.md) for commands and release sequence.

## Candidate and authority

The qualification checkout is `[task-workspace]`, branch `feat/ASMA-8454-qualify-and-release-durable-desk-capability-safely`. Its merged implementation base is `e376f4059e47de8fb40956d4dbfbf6920dabc4ec`; all four delivered tasks are present there. Source candidate: `bdd4838ee08c4a387ebea7b473b522a733df39d4` (tree `81db7c6d9d8561618909c57ae2044e724985df30`). Its non-Markdown verification inputs are frozen in the evidence manifest. The evidence/report carrier preserves those bytes. Its exact final commit and a further complete archive-gate receipt are recorded in the PR/handoff after the carrier is committed; the coordinator reviews that exact revision. The committed logs in this bundle identify their actual tested source commits and are never relabelled as carrier runs.

The approved plan TASK-005, REQ-009/010, CON-002/007/008, §6/TEST-007 and RISK-004 govern this work. Exact plan/advisor/skill/alignment hashes are in [authority provenance](../../docs/evidence/ASMA-8454/authority-provenance.json). Igor's 2026-10-10 batch permission covers build/merge/release; the coordinator owns Independent Review and the gateway. This report supplies no verdict or release authorization of its own. No Jira, plan, real durable seat, live epic, watchdog, fleet policy or existing production pin was written.

Codex independently qualified the four Claude implementations. Codex also owns the two explicitly approved repairs below; their own-author checks require the coordinator's independent reviewers and JUDGE before release acceptance.

## Proven source behavior

| Scope | Evidence | Result |
| --- | --- | --- |
| Desks | Both project-level desk projects/workspaces, epic independence, replay, restart and undeclared-key refusal | 5 acceptance tests pass |
| Durable seats | Exact declarations, Paseo-direct zero runtime effects, busy/unchecked/duplicate/archive fences, daemon fixture single launch/replay, generic retirement refusal | 9 acceptance tests pass |
| Desk Adams | Two epic subjects in one host plus a durable same-role seat, subject credentials/evidence isolation, no-ECP completion, cross-vendor scripted handoff, lost-answer refusals, several-carrier refusal | 21 acceptance tests pass |
| PR reviews and integration | Repository/number identity, same-key distinct subjects, close isolation/convergent retry, integrated restore/replay/selection rollback, later-revision total-zero-effect refusal | 11 acceptance tests pass |
| HTTP/MCP/CLI | Successful desk ensure/replay and undeclared refusal have identical bodies; CLI `placement_blocked` exits 3 | Parity fixture passes |

These are executable source/recorded-contract checks. Their runtime is the in-process scripted adapter. They do not establish live Paseo lost-reply timing or acceptance by an installed released executable.

The integrated test starts an old epic on immutable Team Definition v2, hash `093b0d852d2be02fd08e442446e860b43fa6506bd973e2b627afec357fcd4dc4`. It publishes and selects only in the qualification project a combined v8 definition, hash `79def4231c04072de862998f9743eee5e02e79ff4297609f0bfed98faf72f074`, with topology v5 hash `5c8d6d480fbf997a1217dec51574b842e4eb2d9a53cbbb00e56367252a81d54b`. These fixture revisions are reproduced from the test, not recommendations to select a live default.

Both desks, the two dated permanent-seat declarations, one unrelated daemon fixture seat, two epic Adams, an ad-hoc Adam and two PR subjects coexist. Snapshot creation and supported stopped-realm restore preserve the exact redacted export records hash before startup. Reconciliation opens the barrier; replay mints no additional native. The evidence contains the snapshot manifest, schema 134/export generation 17, exact export hash, reports, node/seat/binding/native census and all canonical definition/topology documents. This batch uses no migration 0135 or export generation 18.

Rollback explicitly selects desk-less v1, then re-drives both existing desk keys with fresh command keys. Their own pins, native ids/kinds/titles/cwd/parentage and subject records remain identical. Reselecting the successor preserves the same census. Closing PR 101 preserves PR 102, both desks, permanent-seat declarations and the old epic pin. Restore/rollback evidence is disposable-realm evidence; no real pin or seat is migrated.

## Exact declaration comparison

The Q-002 source is the coordinator's private local declaration file (path withheld), SHA-256 `2297aba99eb830c35d9a5f386635162a008056dcdcd1bc1d3c019db33ea9cb05`, an **uncommitted dated decision**, branch `feat/ASMA-8419-pr-draft-status-checker`, coordinator-reported HEAD `c4f9957c`, owner Igor / 2026-10-08. It has no commit or PR and is absent from master. The coordinator's 2026-10-11T13:16Z disposition accepts these exact bytes as the comparison source.

The read-only comparator verifies both titles (`LSA • PR DRAFTS STATUS CHECKER`, `LSA • GENERAL`), role, skill, owner and date against supported fixture readback. Both remain `paseo_direct`, `unchecked`, with no native, seat binding or model route. `premium` remains the source chain declaration and is not resolved, adopted or launched. A future hash/commit/PR needs Igor's explicit newer decision; this qualification does not chase it. See [declaration comparison](../../docs/evidence/ASMA-8454/asma-8419-comparison.json).

## Qualification repairs

1. **CLI classification**, checkpoint `c5d426cf`. At `crates/kontor-cli/src/output.rs:81`, `placement_blocked` fell through to exit 1 while HTTP/MCP/CLI bodies already matched. The additive repair maps `placement_blocked`, `ensure_mismatch`, `handoff_unsettled` → Refused **3**; `delivery_unconfirmed` → Conflict **4**; `proof_scan_incomplete`, `provider_usage_throttled` → Unavailable **5**; `rename_pending` → Absent **6**. Every previously mapped class is unchanged, including `stale_binding`/`idempotency_conflict`/`revision_conflict` → **4**, and `unsupported_capability` → **6**. The source audit enumerates all direct desk/seat/Adam/PR codes and shared-path additions. The table test at `output.rs:266` pins exact numbers for all new codes; the parity fixture pins process exit **3** and identical bodies. Three existing local-resolution tests were updated from the former placement-refusal exit **1** to **3**. Before/after receipts and `cli-code-map-audit.json` preserve the finding and affected-gate results.
2. **Atomic fresh-desk topology**, checkpoint `5f0e2e72`. Previously `ensure_desk` inserted the permanent successor reservation before its nodes; the later-revision bridge correctly refused `revision_conflict` but left that desk row. The fresh path at `crates/kontor-daemon/src/applications.rs:10789` now calls `crates/kontor-store/src/repository/desks.rs:43` (`create_desk_topology`) to insert the reservation and both nodes under one `BEGIN IMMEDIATE`. Existing-desk paths are unchanged. The old insert bodies are extracted at `repository.rs:10629` and `repository/desks.rs:158`; original single-write callers retain their own `BEGIN IMMEDIATE` and commit. Non-comment token comparison preserves SQL/error literal bytes and all bridge predicates, allowing only the transaction-reference parameterization. No caller requiring a behavior change was found. The regression at `tests/loopback/qualification.rs:107` now proves the same refusal, unchanged full export records hash/census/runtime-call history, no new reservation/node/native/receipt and unchanged existing bindings/pins. Selecting the prior supported revision and reusing the refused command key then succeeds and replays unchanged. The total-zero-effect observation covers this existing-root bridge refusal; project-root preparation keeps its original separate write path.

The first finding was observed as CLI exit **1**, then pinned to **3**. The second was a failing census comparison containing the extra `StoredDesk` row, then passed the strengthened durable-record and runtime checks. Both were reported before source edits and explicitly approved by the coordinator/user. See the retained [repair evidence](../../docs/evidence/ASMA-8454/repairs/) and [extraction check](../../docs/evidence/ASMA-8454/extraction-check.json).

## Final whole-change mutation phase

The final complete pass at `bdd4838ee08c4a387ebea7b473b522a733df39d4` killed **12 / 12 non-equivalent mutants**, with **0 survivors, 0 invalid/blocked and 0 equivalents**. Each seed compiled and failed an actual test assertion after its exact target command passed a fresh baseline. Only one seed was present at a time; every source file's original bytes and Git status were restored before the next seed. The restored four-suite baseline passed all 46 tests. All 883 frozen verification inputs match their original SHA-256 values and aggregate `4cd85071334b518480520dbd7fedffef41f64eb23f008d4189fff83581129511`.

| Seed | File:line | Fault and observed check | Final result |
| --- | --- | --- | --- |
| MUT-001 | `crates/kontor-daemon/src/applications.rs:10755` | Require an epic before fresh desk ensure; epic-independent desk assertions fail | KILLED |
| MUT-002-busy | `crates/kontor-core/src/durable_seat.rs:492` | Allow restart of busy seat; missing-only fence fails | KILLED |
| MUT-002-unchecked | `crates/kontor-core/src/durable_seat.rs:492` | Allow restart of unchecked seat; proof fence fails | KILLED |
| MUT-002-replay | `crates/kontor-daemon/src/applications/durable_seats.rs:246` | Remove successful launch replay fence; launch-once assertion fails | KILLED |
| MUT-003-host | `crates/kontor-daemon/src/applications.rs:10736` | Admit all epic subjects in one desk host; persisted completion-owner assertion fails | KILLED |
| MUT-003-authority | `crates/kontor-daemon/src/applications.rs:18775` | Admit every active desk seat as completion control; completed-delivery gate refuses multiple carriers | KILLED |
| MUT-003-ecp | `crates/kontor-daemon/src/applications/desk_adams.rs:78` | Force legacy ECP resolution; desk-led planning/delivery without ECP fails | KILLED |
| MUT-004-repository | `crates/kontor-store/src/repository/pr_reviews.rs:119` | Drop repository filter; same PR number across repositories collides | KILLED |
| MUT-004-close | `crates/kontor-daemon/src/applications/pr_reviews.rs:1040` | Retire all project nodes on one PR close; sibling/integrated isolation fails | KILLED |
| MUT-005 | `crates/kontor-store/src/repository.rs:23903` | Read mutable project default instead of old epic pin; rollback/replay identity fails | KILLED |
| MUT-Q-CLI | `crates/kontor-cli/src/output.rs:81` | Remove placement refusal classification; actual process exits 1 instead of 3 | KILLED |
| MUT-Q-ATOMIC | `crates/kontor-store/src/repository/desks.rs:48` | Commit desk reservation before node transaction; zero-effect census fails | KILLED |

The first complete pass at `9d18c9190dc55cec7135f263d78170e92f66b692` killed 10 seeds; `MUT-003-host` and `MUT-003-authority` survived. Completion attempts in the earlier two-epic fixture stopped at the open-ticket gate, so they never proved persisted completion authority. That finding blocked qualification. After finishing the whole pass and restoring source, the existing completed-delivery/close test was strengthened to deliver both subjects, assert both actual persisted TPM owners, and exercise bidirectional foreign-TPM/durable-seat rejection plus an own-TPM positive control. The fixture's scripted capacity was raised to 32 for these simultaneous subjects; no production capacity or fleet policy changed. The earlier test now describes its ticket-gate limit honestly. No production repair was needed for these two survivors.

The final host fault changes the stored owner assertion; the final any-active-seat fault causes a `placement_blocked` multiple-live-seat refusal where supported completed delivery must succeed. The evidence keeps both complete passes, exact commands/lines, original and seeded hashes, failed tests and failure excerpts. See [final results](../../docs/evidence/ASMA-8454/final-mutations/mutation-results.json) and [first pass](../../docs/evidence/ASMA-8454/first-mutation-pass/mutation-results.json). The restored CLI suite and parity fixture were rebuilt and rerun after the mutation phase, avoiding a stale seeded CLI executable.


## Accepted pre-existing message-proof defect

The first archive run stopped at `message_delivery_proofs::legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery`, `message_delivery_proofs.rs:305`: HTTP **409** instead of **200**. Its failed log SHA-256 is `c0b596e9a7a31c2ac9057cefe88e7c3d10b6fa4369973962a7533359d5bdf7d2`. The original assertion did not print the response body; attributing that first failure to the root cause below is an inference.

Five focused repeats passed on both clean `e376f4059e47de8fb40956d4dbfbf6920dabc4ec` and candidate `bdd4838ee08c4a387ebea7b473b522a733df39d4`. An unmodified full loopback run passed on each (base 612 / candidate 616 passes, one existing ignored test each). The base was exported by read-only `git archive`, its entire file manifest was checked afterward, and it used a separate Cargo target. Scratch-only observability captured the response and read-only issuance/anchor fields while preserving the original operation and status assertion. Standalone 100-call probes initially passed on both sources. The approved bounded window then ran two full scratch targets; its clean-base probe recurred at round 62, while the candidate observer target passed.

The captured **409 `timeline_refetch_required`** says “this binding has no earlier durable message or turn epoch anchor”. Readback shows an earlier same-binding issuance at `2026-10-11T15:23:05.724Z`, already delivered at `(epoch 1, sequence 1)`, before the new issuance at `2026-10-11T15:23:05.724434Z`; the anchor query returned `None`. The unchanged query at `crates/kontor-store/src/message_delivery_proofs.rs:138` filters with `issued_at <= ?2` as **TEXT**. An in-memory read-only SQLite comparison returns false for `.724Z <= .724434Z`, while chronological comparison is true. This explains the clean-base recurrence. The failing test, issuance helpers, sessions reconcile handler and message-proof store are byte-identical between base and candidate; the batch never modified them.

The coordinator/user explicitly accepted this as a **pre-existing, unrelated defect outside this epic** and directed that it remain unrepaired here. Only this exact signature is accounted in the gate disposition; any other failure retains the candidate hold. The initial red receipt and concrete recurrence remain evidence, even if the archive retry passes. The coordinator owns separate follow-up routing.

The original finding is `.qualification/reproduction/concrete-finding.json`, SHA-256 `8339f1f52b8287dfd6026acc9dbd3b1db7f978ff03be6ceba5615a6445a5858d`; its [committed copy](../../docs/evidence/ASMA-8454/archive-failure/concrete-finding.json), [accepted disposition](../../docs/evidence/ASMA-8454/archive-failure/archive-failure-disposition.json), diagnostic diffs and complete comparison receipts are in the [archive-failure evidence](../../docs/evidence/ASMA-8454/archive-failure/). The clean-base recurrence log SHA-256 is `f2477f465d26eb0a7c6ddf481c37298b1dba3e61a095eac4db2732330d143b2a`.

Reproduce from a disposable checkout with the recorder's build/fixture environment. Export the exact base to a separate directory and target, then execute this same command from both source roots:

```sh
cargo test --offline --locked -p kontor-daemon --test loopback_api \
  message_delivery_proofs::legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery \
  -- --nocapture --test-threads=2
cargo test --offline --locked -p kontor-daemon --test loopback_api -- --nocapture --test-threads=2
```

The retained reproduction scripts show exact export, target isolation, scratch-only readouts, commands and bounds. Do not repair message-proof semantics under ASMA-8454.

The first retry also exposed an environment issue: reusing the Cargo target across temporary archive roots retained an `account_security` test binary whose embedded `CARGO_MANIFEST_DIR` pointed to the deleted first export. Sixteen tests then refused the absent `alpha-home` fixture. This was diagnosed from the embedded path and the resolver's directory canonicalization, not waived. Complete source and final-carrier archive gates use separate empty target directories. The failed retry and environment receipt remain in the archive-failure evidence; no account code or test changed.

## Required local gates and reproduction

The complete isolated archive gate passed at qualified source `bdd4838ee08c4a387ebea7b473b522a733df39d4`: **3564 Rust tests passed**, **9 existing tests ignored**, across **180 test targets**. Formatting, workspace/all-target clippy with warnings denied, locked workspace tests, Cargo audit/deny, frozen pnpm install, frontend types/tests and production audit all exited zero. Cargo audit retains its existing allowlisted warnings; this is not a claim of no advisories. The separate OpenAPI verification also passed; API/schema inputs are unchanged. Exact commands, timestamps, tested commits, output digests and failure assertions are in the [gate index](../../docs/evidence/ASMA-8454/gate-index.json) and [archive summary](../../docs/evidence/ASMA-8454/archive-summary.json). No gate was silently skipped.

| Focused evidence | Result |
| --- | --- |
| Restored four-structure acceptance | 46 passed / 0 failed |
| Restored CLI, including parity and exact exits | 37 passed / 0 failed |
| Store ensure/materialize/replay and bridge coverage | 727 passed / 0 failed |
| Recorded Paseo protocol contracts | 306 passed / 0 failed |
| Whole-change final mutations | 12 killed / 0 survivors / 0 invalid |

The final evidence-only carrier receives its own fresh-target complete archive gate and API verification after it is committed; that exact receipt is recorded in the PR/handoff before routing Independent Review. The committed source-run receipt is not relabelled as a carrier run. Installing/releasing and exact released-process readback remain coordinator gates.

The recorder uses pinned/offline Cargo where applicable, `RUSTC_WRAPPER=''`, worktree `CARGO_TARGET_DIR`/`TMPDIR`, `GIT_CEILING_DIRECTORIES` and removes inherited `KONTOR_AUTH`. The cached Swagger v5.17.14 ZIP has SHA-256 `481244d0812097b11fbaeef79f71d942b171617f9c9f9514e63acbe13e71ccdc`; its read-only origin was `$HOME/.cache/op08-qa-20260921/target/debug/build/utoipa-swagger-ui-9f37ed19c045eada/out/v5.17.14.zip`. Rust/Cargo 1.97.1, Node v24.13.1 and pnpm 11.6.0 were used. Lockfile package versions/checksums are unchanged; the only Cargo.lock delta adds the existing `kontor-runtime` as a CLI test dependency. Temporary socket binding requires this environment's sandbox escalation; endpoints are ephemeral loopback fixtures.

```sh
python3 evidence/ASMA-8454-qualify-durable-desks.py baseline
python3 evidence/ASMA-8454-qualify-durable-desks.py declarations --declaration-source "$DECLARATION_SOURCE"
python3 evidence/ASMA-8454-qualify-durable-desks.py mutations
cargo test --offline --locked -p kontor-cli
cargo test --offline --locked -p kontor-store
cargo test --offline --locked -p kontor-runtime-paseo --test contract
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/verify-tree.py --mode archive
pnpm --filter kontor-console verify:api
```

`DECLARATION_SOURCE` is the operator-supplied private declaration path; the path is
deliberately not recorded.

The archive gate also runs workspace tests, Cargo audit/deny, frozen pnpm install, typecheck, frontend tests and production dependency audit against the committed candidate. Store and daemon bridge/ensure/materialize/replay coverage must pass. Environmental blocks require exact evidence and coordinator disposition.

## Open criteria and limits

- **Same-vendor LSA→TPM and `keep_native` binding remain refused pending Igor's seat-credential decision.** No same-vendor acceptance is claimed. The advisor's withdrawal of a feasible credential injection does not supply Igor's decision about authority continuity or an amendment of REQ-005.
- Live Paseo lost-reply timing remains unproven. Scripted loss/carrier tests and recorded protocol contracts are bounded evidence.
- The message-proof timestamp TEXT-order defect above is accepted as pre-existing/unrelated. Its exact signature is accounted; no fix is included, and the coordinator surfaces it separately.
- Same-name/same-number/other-owner PR title collision is a documented refusal. The generic action wording remains a candidate: a refusal during non-atomic PR close can follow prior successful retirement. Supported readback and same-key convergent retry are required.
- The no-proved-container close branch is disclosed/source-verified and is not directly exercised here.
- Independent reviewers from different vendors plus a separate JUDGE have not yet reviewed this frozen bundle. Qualification repairs require independent scrutiny too.
- No merged/released/installed artifact, released-process four-structure readback or released-fixture cleanup receipt exists yet. TEST-007 and the Reports row remain open until the coordinator supplies them.

## Proposed coordinator gateway

Review the final carrier commit and its input manifest, then route the required Independent Review/JUDGE. With the judged decision, merge through ASMA, record the merged source and build/install hashes, and retain existing selections. Select the exact qualification successor only in a separately allocated disposable released-build realm with isolated Paseo fixtures and a named cleanup owner. Start and hash the actual released executable; capture supported topology, seat/PR reports, immutable old pins and native identity/title/kind/cwd/parentage; repeat restore/replay and selection rollback with that executable. Preserve commands/outputs and cleanup proof. Resolve the Reports row only after these readbacks prove the goal; keep Igor's unresolved seat-credential decision explicit.
