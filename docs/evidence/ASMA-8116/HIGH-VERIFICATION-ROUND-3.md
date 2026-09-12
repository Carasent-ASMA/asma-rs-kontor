# ASMA-8116 high-verification report — round 3

Date: 2026-09-12
Artifact: `high-verification-report-round-3`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `f183ec30c7330713f42825cf7c99f0107dd3c237`
Candidate tree: `90fb76c48a7bf21d6997d0031123fecce1d29822`
Round 2: `HIGH-VERIFICATION-ROUND-2.md`, commit `a01eee2`
Repair account: `GATE-2-REPAIR-REPORT.md`
Gate receipt: `01a0959b-6637-7c80-b788-64a238a164f6`
Status: **rejected — further implementation changes required**

## Verdict

The remediation makes an identity refusal terminal, and independent tests with
valid workflow selectors confirmed that neither the epic nor task path reaches
a Jira effect after a different immutable ID is observed. Round-2 finding V8 is
therefore closed.

The same-ID task path does not, however, carry the `current_key` returned by the
identity decision into the plan or apply delegation. A verifier probe rotated
the connector from the old key at preview to the same immutable issue under a
new key at apply; the durable binding followed the rename, but the transition
was posted to the superseded key. V7 is reopened by F-8116-V10.

The predecessor-key addition also does not make authority non-replayable. The
authority rows are permanent edges keyed by `(from, to)`. After supported fresh
observations take one binding through A→B→C→A, the old A→B edge becomes eligible
again and a raw canonical update succeeds without a new A→B observation. V9 is
reopened by F-8116-V11. The task anti-rebind route also collapses a permanent
identity contradiction and operational failures into the same HTTP 503, and
the retained no-effect fixtures do not in fact offer the valid transition they
claim to offer.

No production source was changed during verification. Each adversarial or
corrected-fixture test was added temporarily, run against the exact candidate,
and removed. At settlement the only working-tree content retained from this
turn is this report.

## Candidate boundary

- The handed-off branch and its upstream were both clean at
  `f183ec30c7330713f42825cf7c99f0107dd3c237` before verification.
- Review covered all 8 files, 811 insertions and 85 deletions in
  `a01eee2..f183ec3`: the daemon identity gate and tests, store migration and
  repository, schema coverage, Gate-2 repair account and mutation account.
- `GATE-2-REPAIR-REPORT.md` and `RESOLVER-COVERAGE-MUTANTS.md` are preserved as
  point-in-time implementation claims. This report is the independent round-3
  disposition.

## Round-2 finding disposition

| Finding | Round-3 disposition |
| --- | --- |
| F-8116-V7 — task bindings excluded from production identity gate | **reopened by F-8116-V10**; tasks now enter the gate, but discard its current key and can apply through the superseded key |
| F-8116-V8 — anti-rebind refusal does not stop effects | **closed**; both subject paths are terminal, including when the verifier corrects the fixture so a valid transition is genuinely available |
| F-8116-V9 — stale rename authority is replayable | **reopened by F-8116-V11**; predecessor matching blocks A→B reuse while the binding is at C, but not after the key later returns to A |

## Findings

### F-8116-V10 — high: a same-ID task rename applies through the superseded key

`IdentityDecision::Proceed` explicitly carries Jira's current key
(`crates/kontor-daemon/src/applications.rs:507-514`). The epic caller consumes
that field. The task caller does not: it matches
`IdentityDecision::Proceed { renamed, .. }` at `applications.rs:3629-3636`.

The task projection and `TicketDelegation` were already built from the old
`link.external_issue_key` before observation and identity reconciliation
(`applications.rs:3596-3618`). After the store follows the same-ID rename, the
same delegation runs policy at `:3645` and its projection is retained in the
prepared ticket. `ticket_reconcile_apply` re-derives that plan at `:27052` and
then applies each prepared ticket through `ticket.projection`
(`:27124-27144`). Nothing replaces the projection's old key with the
`current_key` the decision returned.

A temporary stateful daemon regression performed this sequence:

1. Preview observed immutable issue `902` at the confirmed key `ASMA-8202`.
2. Apply re-derived the plan after Jira reported the same issue `902` at
   `MOVED-8202`, with a valid transition available.
3. The store accepted the same-issue rename and the API apply succeeded.
4. The captured Jira write was
   `/rest/api/3/issue/ASMA-8202/transitions`, not
   `/rest/api/3/issue/MOVED-8202/transitions`.

The required assertion failed with:

```text
a task rename must continue under Jira's current key:
["/rest/api/3/issue/ASMA-8202/transitions"]
```

This directly contradicts the Gate-2 claim that a successful decision
continues under the key Jira reports now. It also fails the round-2 exit
criterion that later intents and effects never retain the superseded request
key. The old key may be a Jira redirect today, but that is neither the admitted
identity contract nor authority to persist and emit a known-stale identifier.

Required correction: decide identity before constructing the task projection
and delegation, or reconstruct all key-bearing plan state from `current_key`
after the decision. Retain a preview/apply regression that changes the key
between the two reads and asserts the exact Jira mutation path, the stored
binding, and the returned projection all name the new key.

### F-8116-V11 — high: historical edge authority reactivates after a key cycle

Migration 0095 makes task authority append-only and permanent, with primary key
`(project_id, link_id, from_external_issue_key, external_issue_key)`
(`0095_immutable_jira_issue_identity.sql:76-106`). Its guard admits a canonical
task-key update whenever any historical row matches the row's current and new
key (`:113-135`). The epic table and guard use the same permanent-edge shape
(`:141-178`).

The repository reinforces that lifetime: both authority inserts use
`ON CONFLICT ... DO NOTHING` and never consume or supersede a row
(`crates/kontor-store/src/jira.rs:1655-1686,1706-1735`). A predecessor string
therefore describes an edge, not one occurrence of a transition in the
binding's history.

A temporary task-store regression used only supported reconciliation to prove
the same immutable issue at four successive current keys:

```text
ASMA-2 -> HISTORIC-2 -> CURRENT-2 -> ASMA-2
```

It then used raw SQL to update `jira_links` and the canonical task ledger from
`ASMA-2` to `HISTORIC-2`. The canonical update should have required fresh
readback for this new occurrence of A→B, but it succeeded because the original
permanent A→B authority matched again.

The retained task regression at
`crates/kontor-store/tests/jira_materialization.rs:2773-2850` tests only
A→B→C followed by an attempted C→B rollback. That proves predecessor matching,
not non-replayability. The retained epic case at `:2852-2900` has the same
blind spot. No schema invariant makes recurrence of a key impossible.

Required correction: bind authority to a unique binding generation or current
confirmation occurrence and advance or consume that authority atomically. A
historical A→B record must never authorize a later A→B occurrence merely
because the key string A became current again. Retain A→B→C→A cycle regressions
for both task and epic ledgers, followed by unauthorized raw A→B updates.

### F-8116-V12 — medium: task anti-rebind is reported as an availability outage

`IdentityDecision::Stop` erases why identity processing stopped: different
immutable issue, legacy unproven identity, missing observed identity and store
reconciliation failure all become the same variant
(`applications.rs:507-517,3131-3211`). The task route maps every one of those
states to `ApiErrorCode::Unavailable` and HTTP 503
(`applications.rs:3638-3642`; retained assertion at
`crates/kontor-daemon/tests/loopback_api.rs:44261-44283`).

A different immutable ID is a permanent anti-rebind contradiction, not an
availability failure. High-scope clauses 4 and 6 require a stable typed
anti-rebind refusal, while operational connector/store failures need to remain
distinguishable and retryable. Returning 503 invites retries and prevents a
caller or operator from identifying the safety refusal without parsing prose.

Required correction: preserve a typed stop reason through the identity
decision. Map `DifferentIssue` to the established domain conflict/refusal
semantics, keep genuinely transient connector or repository failure as
`Unavailable`, and give legacy/missing proof their own stable fail-closed
classification. Assert the response code and structured error code, not only
the rule text.

### F-8116-V13 — medium: retained no-effect fixtures do not offer the live transition they claim

`RenamedIssueJira` reports `DRAFT` with status ID `10200` and offers
`TO BE GROOMED` with status ID `10213`
(`crates/kontor-daemon/tests/loopback_api.rs:43762-43786`). The bundled v2
workflow identifies those states as `10237 / DRAFT` and
`10236 / TO BE GROOMED`
(`crates/kontor-jira/fixtures/external-workflow-asma-epic-v2.json:33-40,60-65`).

The comments and Gate-2 repair account say the fixture presents a live
DRAFT→TO BE GROOMED transition so continuation would genuinely write. It does
not: the selectors do not describe either state. This means the checked-in
tests can kill a non-terminal mutant incidentally through later unknown-status
classification or a different refusal, rather than by observing the promised
write boundary.

The verifier temporarily corrected the two IDs to `10237` and `10236` and ran
the exact epic and task anti-rebind tests. Both passed, including zero
mutations. That independent result closes V8 for the current production code,
but it does not make the inaccurate retained regression or M8/M9 mutation
account durable proof.

Required correction: retain the valid selector IDs, re-run M8 and M9, and
record a kill only when the non-terminal mutation reaches the counted Jira
write while the restored code records zero writes.

## Hygiene finding

`git diff --check a01eee2..f183ec3` fails at
`docs/evidence/ASMA-8116/RESOLVER-COVERAGE-MUTANTS.md:70` because the candidate
adds a blank line at end of file. This is not a gate blocker by itself, but the
candidate does not satisfy its ordinary whitespace check.

## Known open/integration blockers preserved

- **OQ-002:** the full `schema_v1` target again reproduced the ledgered
  intermittent concurrent-first-open `DatabaseBusy` failure: 57 passed and 1
  failed. The v94-to-v95 migration regression itself passed. This run does not
  settle attribution.
- **OQ-004:** the exact MCP bootstrap journey still fails with
  `placement_blocked: the epic has no active immutable backlog code`, before a
  seat starts, exactly as its ledger records.
- **Migration sequencing:** ASMA-8116 and ASMA-8117 still both claim migration
  0095; final numbering remains assigned to integration.
- The console's unchanged `DeepSeek V4 Flash` expectation still disagrees with
  `DeepSeek V4.1 Flash`; 299/300 tests pass and the mismatch is outside the
  ASMA-8116 candidate diff.

No new open question was required. The blocking states in this report were
directly evidenced rather than assumed.

## Independent test record

| Command or probe | Result |
| --- | --- |
| `git diff --check a01eee2..f183ec3` | **failed:** one added blank line at EOF in the mutation account |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy -p kontor-jira -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | passed |
| `cargo test -p kontor-store --test jira_materialization` | passed, 26/26 |
| temporary A→B→C→A authority-cycle probe | **failed protection:** historical raw A→B canonical update succeeded |
| `cargo test -p kontor-store --test schema_v1` | **failed:** 57/58; OQ-002 `DatabaseBusy`; v95 migration test passed |
| `cargo test -p kontor-jira` | passed, unit 9/9 and native connector 19/19 |
| `cargo test -p kontor-api` | passed, 23/23 + 5/5 + OpenAPI 3/3 |
| checked-in epic/task same-ID rename and different-ID refusal tests, exact | passed, 1/1 each |
| corrected-selector epic and task anti-rebind tests, exact | passed, 1/1 each; zero Jira mutations |
| temporary task preview/apply key-rotation probe | **failed protection:** apply posted to `ASMA-8202`, not `MOVED-8202` |
| `cargo test -p kontor-daemon --test loopback_api jira` | passed with localhost permission, 9; 1 predeclared ignored |
| `cargo test -p kontor-daemon --test mcp_journey an_empty_realm_is_bootstrapped_through_mcp_tools_alone -- --exact --nocapture` | **failed as OQ-004 records:** placement blocked, no seat started |
| `pnpm --filter kontor-console verify:api` | passed |
| `pnpm --filter kontor-console typecheck` | passed |
| `pnpm --filter kontor-console test` | **failed:** 299/300 on inherited model-display expectation |

The first sandboxed broad daemon run was denied Wiremock ports. Its approved
localhost rerun passed and is the candidate result recorded above; the sandbox
denial is not candidate evidence.

## Exit criteria for round 4

1. Carry the observed current key through every task plan, intent and Jira
   effect after a same-ID rename, and retain a two-read preview/apply regression
   that asserts the exact outbound key.
2. Make task and epic rename authority occurrence-specific and non-replayable
   even if a key string later recurs; retain A→B→C→A database-boundary tests for
   both ledgers.
3. Preserve and expose the identity stop reason so a different-ID anti-rebind
   is a stable typed domain refusal rather than HTTP 503, without collapsing
   operational failures into the same response.
4. Correct the workflow selectors in the retained no-effect fixtures and
   re-kill M8/M9 at the actual Jira write boundary; remove the trailing blank
   line from the mutation account.
5. Re-run the focused store, migration, connector, daemon, API/OpenAPI, MCP and
   console gates on one exact new candidate while preserving OQ-002, OQ-004 and
   migration sequencing in their existing ledgers.
