Artifact: `high-audit-report`

# ASMA-8203 high-audit report — release 118 at `44663e10`

Date: 2026-09-21
Task: Jira `ASMA-8203` / Kontor `01a0ac9d-a96d-75b2-9803-169d4f2e531f`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Role: independent high-audit (bounded direct-Paseo replacement; fallback native is execution evidence only)
Decision: **PASS**
Audited source: `44663e10e063ad3d11c0c84ca6a87d680d75e835`
Qualified source: `1b524140e83fc9ce48bdf9cab46d8553ecba5c05`
Tree (both): `c5ea3516f75d2144e2c33224d016d877f602207c`
Verification evidence cited: commit `b1b6207dada7b93c57d9d356e74dea932c830ea8`, path `docs/evidence/ASMA-8203/RELEASE-118-INDEPENDENT-REVIEW.md`, SHA-256 `0594efc0b41a654c30235c7ac5a12a5af6360b950055e57fb7e3dd9a640282fa` (re-hashed this turn; match)

This seat did not merge, deploy, settle, record a gate, or transition Jira. Mutation sampling ran in a disposable detached worktree at the exact audited commit and was restored byte-identical before this report was written.

## Decision

Zero unresolved P0/P1. The deployed tree at `44663e10` meets the current ASMA-8203 acceptance: durable issuance before send, epoch durability before effect, bounded suffix proof, schema-118 resumable legacy proof that never authorizes dispatch, fail-closed unknown confirmation (`do not resend`), exact current-turn settlement, identity preservation, and independently executed tests whose causal mutant reproduces the previously rejected P1.

The cited review at `b1b6207` is accepted as a prior independent execution receipt. This audit re-ran the 457-pass loopback, the P1 pair, identity/negative suffix cases, and the causal mutant, and additionally inspected settlement, observation, issuance, and schema rather than tests alone.

## Subject integrity

| Check | Result |
| --- | --- |
| `git diff 1b524140 44663e10` | empty; identical tree `c5ea3516…` |
| Deployment bundle | `deployments/ASMA-8188-20260921T100129Z-44663e10/deployment.json` records `source_commit=44663e10`, `schema_version=118`, `live=true`, reconciliation open, scheduling open, `quick_check=ok`, `foreign_key_violations=0`, `identities_preserved=true`, `signatures_valid=true` |
| Installed hashes, re-read this turn | `kontor-daemon` `d1d670f79486eda9b01e1be9fdb5e36708012dba6dd6730c0b3b1a727d741fce`; `kontor-mcp` `7d7210c84c472fbc835d8ab2bf5650c61e1836213a73e825ff963d4f1ee28316`; `kontor` `cef7ad90db3b30af4e10021cfe8141a977e6fb584ac6dbde0a6f507ef5b1fbe7` — byte-identical to the bundle |
| Live task identity | `confirmed` Jira `ASMA-8203`, revision 2, readback_hash `94f9809d5475f5a86174f3d174aecc743762d5b010e39f37f94366a2a7e62208`, `confirmed_at` 2026-09-19T02:29:21Z — unchanged from the scope-era binding |

Historical audit `artifact-asma-8203-high-audit-report-24290153` (`P1-POSTDELIVERY-FAILURE-RESTART-DUPLICATES`) remains valid as a finding against the then-deployed source. It is corrected here, not withdrawn.

## Specification, not only tests

Original high-scope (2026-09-17) asked for regression/mutation hardening of an already-present proof path and forbade a new schema unless a mutant proved a production defect. Later authorized gaps (OG-058, spanning-window settlement, post-delivery restart duplicates, epoch persist-before-ack) did prove production defects. The deployed contract at `44663e10` is that expanded specification. Auditing only the original test-only boundary would ignore what is actually shipped.

### Durable message-proof semantics

1. **Issuance before effect.** `POST /v1/sessions/{id}/messages` captures `canonical_tail_boundary` through `tail_window_recovering_epoch_once` (durable epoch barrier), then `record_message_issuance`, then `note_replayed_issuance`, then `send_issued_message`. The issuance row is written before the runtime is asked to accept the message.
2. **First-send floor is ungated.** `note_replayed_issuance` registers `note_issuance_boundary` for every send that has a stored floor, including the first. A first attempt is not declared unconfirmed (that would force a useless canonical read); a replay is.
3. **Suffix is strictly after the floor.** Paseo reconciliation skips `sequence <= floor.sequence`, requires page-join and intra-page contiguity, and treats a floor from another epoch as `TimelineRefetchRequired`. Incomplete scans return `DeliveryConfirmationUnknown` with `do not resend`.
4. **Schema 118.** `0118_runtime_message_delivery_proofs.sql` adds CAS-paged proof tables. Header and triggers: no native dispatch; issuance hash, generation, upper bound, epoch and anchor immutable; forward-only updates; no deletes. `SCHEMA_VERSION = 118`. Store advance refuses changed issuance/binding, non-monotonic revision, and `occurrences > 2`. Confirmed replay in `send_issued_message` requires exact binding, generation, native id, body digest and candidate position; anything else refuses without sending.
5. **Settlement still proves one current turn.** `prove_current_turn` requires fresh `waiting_input`, exact message-id at the claimed position, a later terminal response in the same epoch, no later turn event, and `newer_messages_inside == 0`. A spanning tuple is a distinct 409. Page-budget exhaustion is `ProofScanIncomplete`, not a forgery accusation.
6. **Observation.** No-cursor reads use a bounded tail window and pin the reported message to `issuance.delivered_at`. Cursor resumes still run `prefix_occurrences` and fail closed when uniqueness is unproven. Window-local duplicates are refused even though the issuance PK already says Kontor minted the id once.

### Identity preservation

Kontor message identity is `clientMessageId`, never the provider `messageId`. Issuance is keyed by `message_id` with conflict-on-different-session. The ASMA-8203 Jira binding is confirmed and its readback hash is unchanged by this release. Legacy Sep-20 acknowledgements remain `UNKNOWN` under fail-closed proof; this audit replayed none.

### Negative cases independently executed

| Case | Result |
| --- | --- |
| Forged current-window matrix (`settling_a_bounded_turn_refuses_a_forged_current_window`) | ok |
| Direct/derived first-send boundary registration | ok |
| Direct/derived post-delivery epoch-commit failure then restart does not resend | ok |
| Suffix duplicate refuses without a send | ok |
| Suffix changed-epoch refuses even when the message is visible | ok |
| Suffix partial absence never authorizes a send | ok |
| Pre-issuance occurrences in the boundary page are excluded | ok |
| Only echoed client id positions the Kontor message | ok |
| `native_child_archive_requires_retirement_and_recovers_a_lost_acknowledgement` | ok (6.05s; previously reported timeout does not reproduce at this head) |

### Deployment / schema assumptions

Live binaries still match the 118 bundle. Realm health in that bundle is schema 118, live, reconciliation open. Schema 118 is a one-way migration; a schema-older daemon cannot open this database (already demonstrated on this task's earlier stale-base deploy). This audit did not re-apply migrations and did not restart the shared realm.

Inherited from the bundle, **not** treated as P0/P1: `launch_anomaly` (`observed_launch_delta=2` vs `operator_restart_count=1`). Process still `pid 39728`, hashes unchanged, identities preserved, watchdog remains stopped. Extra launchd starts around `kickstart -k` are bookkeeping, not a proof-path defect. Root-cause review of that counter remains open as P2.

## Independent tests

Executed against a clean detached worktree at `44663e10`, `CARGO_TARGET_DIR=/private/tmp/kontor-8203-release118-review-20260921/target`.

| Suite | Result | Log SHA-256 |
| --- | --- | --- |
| Focused daemon (5 tests above) | 5 passed | `cdd1e0f4f453a3efdd4ea2aa11e95d21cbc00de42d90b80f6313d8842b15ee8a` |
| Focused Paseo contract (5 tests above) | 5 passed | `ac20a29714a8186b262df915e812e14253fd3d8401bf8b735c65b447cc30a4e8` |
| `kontor-daemon --test loopback_api` | **457 passed, 0 failed, 1 ignored** in 79.74s | `e6471dd490d34907ea68a4d7d5630cbd8fee31e2001f732f9fbb1cc70c4022fb` |
| `native_child_archive…` | ok | `cf5c81c1a983e4c5312ad0f6e7ebabffdaf4f5041199768738055cb1b0703982` |

The 457 figure matches both root's `final-daemon.log` and the cited `b1b6207` review. This seat produced its own log.

Not independently re-executed here: qualification `final-schema118` (64), `final-store-recovery` (34), `final-mcp` (23), `final-cli` (22), `final-openapi-check` (3). Those remain root's logs. Schema 118 itself was inspected in source (`migrations.rs`, `0118_…sql`, store guards) and in the live deployment receipt.

## Mutation sampling

One mutant at a time in the disposable `44663e10` worktree; restored with `git checkout -- <file>` before the next. Tree clean after the last restore. Restart pair re-verified green.

| Mutant | Defect | Killer | Result |
| --- | --- | --- | --- |
| **M-P1** | `note_replayed_issuance` returns `Ok` on replay without `note_unconfirmed_delivery` | `direct_` / `derived_delivery_epoch_commit_failure_then_restart_does_not_resend` | **KILLED.** Direct `left: 2 right: 1`; derived `left: 3 right: 1` — "the replay reconciled the existing delivery instead of instructing the seat twice". Log `3407700d259601d62debfffbfef7f304efd7870d7f4a8900951fe54b294c775a` |
| **M-SPAN** | drop `newer_messages_inside != 0` refusal | `settling_a_bounded_turn_refuses_a_forged_current_window` | **KILLED.** Spanning case `left: 200 right: 409`; body `applied: created`, `turn_ordinal: 1`, follow-up dispatched to `builder`. Log `d3cbd5df531b5fc947596d10f580f72c591069590f68904cdfc2c155d0de7f7e` |
| **M-FLOOR** | count occurrences at or below the issuance floor | `issuance_suffix_excludes_preissuance_occurrences_in_the_boundary_page` | **KILLED.** `DuplicateMessage { rule: "appears more than once in this session's canonical content" }`. Log `f682193820d096e86ab078fd2ab6cf4232827a9b274c68519fdf880a56a3023a` |
| Restore | — | restart pair after M-P1 restore | 2 passed. Log `fe607cfec250f579ff0c870a3b769d1d40d1ca4642cc62c8713ecd98694de6e5` |

M-P1 is the causal mutant the cited review named. The numbers match. The other two show the settlement spanning guard and the suffix floor filter are load-bearing, not vacuous.

## Kontor evidence/settlement from this fallback native

Attempted, truthful, not fabricated:

| Call | Refusal |
| --- | --- |
| `kontor_turn_settle` (`agent_run_id=01a0acf9-6636-…`, historical TeamRun id) | **404** `not_found` — `no such agent run exists in this project` |
| `kontor_artifact_record` | **400** `invalid_request` — `a value did not satisfy the invariant of its type`, subject `RoleTurnId` |
| `kontor_gate_record` | tool refused — `evaluator_account` is not a canonical `AccountProfileId` |
| `kontor_lifecycle_transition` complete_task (UUID) | **400** `invalid_request` — `the operation requires evidence that has not been recorded`, subject `task closure` |

This native is unbound. Those refusals are the settlement/evidence outcome of this turn. Root reconciliation must attach this report through a bound seat's `kontor_artifact_record` / `kontor_gate_record`. Do not treat this document as a recorded Kontor artifact or a passed gate.

`kontor_context_resolve` for ASMA-8203 returned `agent_run_id: null`. Task snapshot: phase `high-implementation`, `high-verification-gate=passed`, `high-audit-gate=rejected` (prior), revision 2.

## Findings

### P0

None.

### P1

None unresolved. Historical `P1-POSTDELIVERY-FAILURE-RESTART-DUPLICATES` is corrected at `44663e10` and independently mutation-killed.

### P2 / limitations (do not block PASS)

1. Deploy bundle `launch_anomaly` still has no root-cause write-up. Health and identity readbacks do not show loss.
2. Qualification suites other than loopback/Paseo-focused were not re-run by this seat.
3. Legacy unknown acknowledgements remain unknown by design; not replayed.
4. This fallback native cannot record Kontor artifacts or pass `high-audit-gate`.

## Verdict

**PASS.** Zero unresolved P0/P1. Acceptance evidence for the audited checkpoint is complete in Git/test/runtime form above. Kontor gate recording is blocked on an unbound native and is handed to a bound reconciler with this report as the payload.
