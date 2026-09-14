# ASMA-8116 high-verification report — round 2

Date: 2026-09-12
Artifact: `high-verification-report-round-2`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `0913786643574a4952920b135a9cdde9d2a8da3d`
Candidate tree: `219aa1a0a7eee40b4b17e126b7629d7ab47c337e`
Round 1: `HIGH-VERIFICATION-REPORT.md`, commit `0a400ec`
Repair account: `GATE-1-REPAIR-REPORT.md`
Status: **rejected — further implementation changes required**

## Verdict

The remediation closes round 1 findings V1, V4, V5 and V6 on the evidence
reviewed. It does not close V3 for the task ledger, and its epic anti-rebind
branch records a block but then continues into the effect path. An independent
adversarial probe made that path attempt a Jira mutation against the different
immutable issue. The V2 replacement guard also retains every destination
authorization permanently; after a later legitimate rename, raw SQL can replay
an older authorization and roll the canonical task key back without fresh Jira
readback.

The gate remains rejected on three high findings below. No production source
was changed during verification. Each adversarial test was added temporarily,
run once against the exact candidate, and removed. At settlement the only
working-tree content retained from this turn is this report.

## Candidate boundary

- The handed-off branch was clean at
  `0913786643574a4952920b135a9cdde9d2a8da3d`; the repair consists of
  `a9aef057851684742248cc712818f6d54bee2555` and
  `0913786643574a4952920b135a9cdde9d2a8da3d` on top of the round-1 report.
- Review covered all 11 files, 1,405 insertions and 24 deletions in
  `0a400ec..0913786`, including the store migration and repository, Jira
  connector, resident daemon integration, focused tests, repair account and
  mutation account.
- `GATE-1-REPAIR-REPORT.md` and
  `RESOLVER-COVERAGE-MUTANTS.md` remain point-in-time implementation claims;
  this report records the independent round-2 disposition.

## Round-1 finding disposition

| Finding | Round-2 disposition |
| --- | --- |
| F-8116-V1 — legacy cross-ledger exact replay | **closed**; the cross-ledger check now precedes legacy-id establishment and the bidirectional regression passes |
| F-8116-V2 — direct SQL forges a task rename | **reopened** by F-8116-V9; a never-authorized destination is blocked, but a stale authorized destination is replayable |
| F-8116-V3 — production rename reconciliation | **reopened** by F-8116-V7 and V8; only the epic path is wired, and its refusal is non-terminal |
| F-8116-V4 — connector suite/readback coverage | **closed**; all 9 unit and 19 native connector tests pass, including exact/missing/malformed immutable-id cases |
| F-8116-V5 — real v94-to-v95 migration gate | **closed**; the focused migration case ran and passed in the 58-test schema target |
| F-8116-V6 — post-commit result race | **closed**; the result is constructed inside the transaction and both retained regressions pass |

## Findings

### F-8116-V7 — high: production rename reconciliation still excludes task bindings

The admitted contract requires same-issue rename and different-ID anti-rebind
behavior for both `jira_epic_bindings` and
`jira_task_binding_confirmations`. The repair's only production call to
`reconcile_confirmed_jira_key` is `follow_same_issue_rename`, and its only call
site is inside `reconcile_jira_epic`
(`crates/kontor-daemon/src/applications.rs:3125-3172,3222-3228`). The lookup at
`:3215-3220` explicitly selects only `JiraBindingSubject::Epic`.

The resident task loop instead calls `prepare_ticket_plan`
(`applications.rs:30014-30066`). That method does receive the connector's
identity-bearing observation at `:3575-3578`, but it immediately runs the
status policy and never compares `observed.response.observed_identity` with the
task confirmation ledger. A repository search finds no other non-test call to
the store reconciliation method.

Consequently a confirmed task cannot follow a same-ID Jira rename through any
production controller path. The two new end-to-end tests seed only an epic, so
they do not discriminate this omission. This is the exact epic-only reduction
rejected by resolved OQ-003 and high-scope clauses 2-4.

Required correction: apply one fail-closed identity decision to every observed
confirmed task before task policy/conflict/effect processing, and retain
end-to-end same-ID rename and different-ID no-effect tests for both ledgers.

### F-8116-V8 — high: an epic anti-rebind refusal does not stop Jira effects

`follow_same_issue_rename` returns `()` in every case. On a different immutable
ID it increments `report.blocked` and returns only from the helper
(`applications.rs:3133-3140`). Its caller ignores whether the helper reconciled,
refused or failed, and continues at `:3229` into issue classification, policy,
intent persistence and possible connector apply. A legacy binding with no
stored ID and an identity lookup failure likewise return from the helper
without making the caller fail closed.

A temporary adversarial variant of
`the_resident_reconciler_refuses_a_key_that_now_answers_for_another_issue`
kept the fixture's different ID (`999`) but returned `DRAFT` plus a live
transition to `TO BE GROOMED`, and counted every non-GET. The required
no-effect assertion failed:

```text
assertion `left == right` failed: a different immutable issue must receive no Jira effect
  left: 1
 right: 0
```

The checked-in negative test cannot detect this: its fixture supplies no live
transitions and its assertions check only the report and unchanged binding
(`crates/kontor-daemon/tests/loopback_api.rs:43955-43992`). Thus the branch can
correctly avoid rebinding the ledger while still trying to transition the Jira
issue the immutable-ID check proved was different.

Required correction: make the identity decision a returned control-flow result
and stop the subject before any policy, intent or external effect on every
refusal/error. After a successful rename, either restart on or explicitly carry
the observed current key so later intents and effects do not retain the
superseded request key. The regression must offer a valid transition and assert
zero Jira writes, not merely an unchanged Kontor binding.

### F-8116-V9 — high: permanent rename authority can be replayed after it is stale

Migration 0095 stores rename authority permanently under
`PRIMARY KEY (project_id, link_id, external_issue_key)` and forbids update or
delete (`0095_immutable_jira_issue_identity.sql:76-99`). The canonical-key
trigger admits a destination whenever *any* matching historical authority row
exists for the link and immutable ID (`:101-123`). It does not bind that row to
the current predecessor key, the current confirmation evidence, a sequence, or
one consumption.

A temporary store regression performed two supported same-issue task renames,
`ASMA-2 -> HISTORIC-2 -> CURRENT-2`. It then used one raw transaction to update
`jira_links` and `canonical_jira_task_links` back to `HISTORIC-2`. The assertion
that the canonical update be refused failed: both updates succeeded by reusing
the first rename's permanent authority.

The retained test at
`crates/kontor-store/tests/jira_materialization.rs:2006-2069` covers only a
never-authorized `FORGED-9`, so it passes while stale proof remains reusable.
This violates the resolved OQ-003 rule that a task key changes only after fresh
connector readback proves the same immutable issue.

Required correction: make authority exact to one current-state transition and
non-replayable after the binding advances (for example through a predecessor
evidence/sequence and consumed/current authority invariant enforced at the
database boundary). Retain a three-state regression that authorizes A-to-B and
B-to-C, then proves direct SQL cannot reuse the A-to-B authority.

## Known open/integration blockers preserved

- **OQ-002:** the full `schema_v1` target reproduced the ledgered intermittent
  concurrent-first-open `DatabaseBusy` failure: 57 passed, 1 failed. The new
  v95 migration test itself passed. This single run does not settle attribution.
- **OQ-004:** the exact `mcp_journey` bootstrap still fails with
  `placement_blocked: the epic has no active immutable backlog code`, as its
  ledger predicts.
- **Migration sequencing:** ASMA-8116 and ASMA-8117 still both claim migration
  0095; final numbering remains assigned to integration.
- The console's unchanged `DeepSeek V4 Flash` expectation still disagrees with
  `DeepSeek V4.1 Flash`; the 299/300 result is inherited from files outside the
  ASMA-8116 diff.

No new open question was required: the three blocking states above were
directly evidenced rather than assumed.

## Independent test record

| Command or probe | Result |
| --- | --- |
| `git diff --check` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy -p kontor-jira -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | passed |
| `cargo test -p kontor-store --test jira_materialization` | passed, 24/24 |
| temporary stale-authorization replay probe | **failed protection:** the raw canonical rollback succeeded |
| `cargo test -p kontor-jira` | passed, unit 9/9 and native connector 19/19 |
| `cargo test -p kontor-api` | passed, 23/23 + 5/5 + OpenAPI 3/3 |
| `cargo test -p kontor-store --test schema_v1` | **failed:** 57/58; OQ-002 `DatabaseBusy`; v95 migration test passed |
| `cargo test -p kontor-daemon --test loopback_api jira` | passed, 9; 1 predeclared ignored |
| checked-in same-ID rename and different-ID refusal tests, exact | passed, 1/1 each |
| temporary different-ID/no-effect daemon probe | **failed protection:** one Jira mutation was attempted |
| `cargo test -p kontor-daemon --test mcp_journey an_empty_realm_is_bootstrapped_through_mcp_tools_alone -- --exact --nocapture` | **failed as OQ-004 records:** placement blocked, no seat started |
| `pnpm --filter kontor-console verify:api` | passed |
| `pnpm --filter kontor-console typecheck` | passed |
| `pnpm --filter kontor-console test` | **failed:** 299/300 on inherited model-display expectation |

The first sandboxed adversarial daemon run was denied a Wiremock port; its
approved localhost rerun produced the finding above. That sandbox denial is not
candidate evidence.

## Exit criteria for round 3

1. Extend the resident identity gate to task bindings and prove same-ID rename
   plus different-ID no-effect behavior end to end for both subject kinds.
2. Make every epic identity refusal terminal for that subject before intent or
   Jira effect; ensure the current observed key, not the superseded request key,
   governs any continuation after a successful rename.
3. Make task rename authority current-transition-specific and non-replayable,
   and retain the stale-authority raw-SQL regression.
4. Re-run the focused store, migration, connector, daemon, API/OpenAPI, MCP and
   console gates on one exact new candidate while preserving OQ-002, OQ-004 and
   migration sequencing in their existing ledgers.
