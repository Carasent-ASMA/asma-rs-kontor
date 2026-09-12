# ASMA-8116 high-verification report

Date: 2026-09-12
Artifact: `high-verification-report`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `6796fdaaf7fb66f8ba3acc865dfb9106b8c43278`
Implementation code head: `bc19803b1c4262ff52ebfdf571fe710f66081a66`
Handoff: `docs/evidence/ASMA-8116/IMPLEMENT-TO-VERIFY-HANDOFF.md`
Status: **rejected — further implementation changes required**

## Verdict

The candidate is not ready for integration. Its existing focused store and API
tests pass, but independent adversarial checks found two supported/direct-write
paths that defeat the immutable Jira identity rules. The same-issue rename
implementation is not called by production code, five native connector tests
are red after the new mandatory Jira `id` readback, and the required real
v94-to-v95 preservation test is absent.

The already-recorded OQ-004 placement gap also remains observable: the exact MCP
bootstrap journey seats nobody because the new-policy epic has no legacy backlog
code. This report does not reinterpret that open question or the ASMA-8117
migration-number collision; both remain attached to their existing ledger
records.

No production source was changed during verification. Two temporary adversarial
tests were added one at a time, run, and removed. The untracked `.pnpm-store/`
created by console verification was removed. At settlement the only working-tree
content retained from this turn is this report.

## Candidate boundary

- The clean handed-off branch was at `6796fda`; its tree is
  `1fbf0ec2beb50093ea8dfedc428414c60b4c7846`.
- `6796fda` changes only the handoff document. The implementation and its final
  added tests are frozen at parent `bc19803`.
- The merge base and contained `origin/master` are
  `e40b5f42759c06c15ea8d666a18b843d950db51d`.
- Review covered all 25 changed files in `origin/master...6796fda`, including
  connector, migration, store, daemon, API/OpenAPI, console schema and tests.

## Findings

### F-8116-V1 — high: a legacy exact replay can claim an issue already bound in the other ledger

`confirm_jira_materialization_item` returns early for an exact replay after
calling `establish_immutable_issue_id`
(`crates/kontor-store/src/jira.rs:1185-1198`). The cross-subject guard is below
that return (`:1218-1247`) and is therefore never evaluated. The helper's
uniqueness protection is only the per-table index
(`:1844-1915`; migration lines 25-29), so it cannot see the other ledger.

A temporary regression created a normal confirmed epic and task, reduced only
the epic binding to the migrated pre-v95 `external_issue_id = NULL` shape, then
replayed its exact key and hash while presenting the immutable issue id already
held by the task. The expected typed conflict failed:

```text
a legacy replay must not claim an immutable issue already bound to a task: Ok(())
```

The supported call committed one immutable Jira issue id onto two subjects in
the same project, contrary to scope clause 7. The existing cross-subject mutant
does not cover this branch because it starts from a planned item rather than an
exact replay.

Required correction: perform the cross-ledger issue-id check before every
legacy-id establishment, retain the database/transaction boundary under
concurrency, and add both epic-to-task and task-to-epic exact-replay cases.

### F-8116-V2 — high: two direct SQL updates can forge the trigger's “proof” of a task rename

The new task-key trigger admits a canonical key update when the confirmation row
has any non-null immutable id and the mutable `jira_links` row already carries
the requested new key
(`0095_immutable_jira_issue_identity.sql:66-82`). It does not bind that change to
fresh connector evidence or an authorized reconciliation intent.

A temporary regression updated `jira_links.external_issue_key` first and then
updated `canonical_jira_task_links.external_issue_key` in the same raw SQLite
transaction. Both writes succeeded; the assertion requiring the second write to
be refused failed:

```text
direct SQL must not forge the tail of a proven same-issue rename
```

The checked-in direct-SQL test changes only the canonical row, so it does not
exercise the exact two-step condition the trigger treats as proof. This leaves
OQ-003's required unauthorized-direct-SQL invariant unproved and false.

Required correction: make the database guard depend on durable, exact rename
authority that a caller cannot fabricate merely by changing the other mutable
key row, and retain this two-statement test beside the single-row refusals.

### F-8116-V3 — high: same-issue key reconciliation is unreachable from production

The only non-test definition/reference of `reconcile_confirmed_jira_key` is the
public store method itself (`crates/kontor-store/src/jira.rs:1521`). A repository
search finds no daemon, API, MCP, connector-controller or other production call.
The materialization loop explicitly skips every already-confirmed epic and task
(`crates/kontor-daemon/src/applications.rs:18089,18119`), so the connector cannot
provide a fresh renamed readback through that path either.

Consequently the branch can reconcile a rename only when a Rust test directly
supplies an `issue_id`, key, hash and timestamp to the store. The delivered
system has no path that observes a confirmed issue again and invokes the method,
so scope clauses 1-4 are not implemented end to end.

Required correction: wire the existing resident Jira/readback boundary to
detect and reconcile a current key by immutable issue id, preserving typed
anti-rebind refusals. Add an end-to-end daemon test in which the connector
returns a changed key for the same id, plus a different-id refusal.

### F-8116-V4 — high: the native Jira connector suite is red and the new accepted readback is untested

With loopback port permission, `cargo test -p kontor-jira` passed 9 unit tests
but failed 5 of 17 `native_connector` tests. Each formerly accepted
materialization now returns:

```text
Unavailable { operation: "transport", reason: MalformedResponse,
              detail: "Jira returned an incomplete response" }
```

The failures are
`create_is_marker_idempotent_and_credentials_are_resolved_per_request`,
`explicit_link_confirms_level_zero_without_claiming_type_or_content`,
`recovery_preserves_a_body_authored_since_creation`,
`materialization_identifies_each_mismatch_without_mutating_jira`, and
`task_create_includes_project_configured_required_fields`.

Their successful readback fixtures were not updated with the now-mandatory
top-level `id`. There is also no connector test asserting that an accepted
readback returns the exact immutable id, and no focused negative for a missing or
malformed id.

Required correction: update every accepted native connector fixture, assert the
exact returned `issue_id`, add missing/malformed-id refusals, and restore the full
package to green.

### F-8116-V5 — high: the required v94-to-v95 preservation gate is absent

The handoff itself records that a focused v94-to-v95 upgrade-preserving test was
not delivered. Inspection confirms it: `schema_v1.rs` changes only the current
version narrative/assertion, while the “legacy” Jira cases open the current v95
schema and manually drop a trigger/set an id to NULL. They do not execute the
new migration over populated v94 epic and task confirmation ledgers.

That does not prove the migration preserves old rows, leaves both ids NULL,
installs the narrowed v81 trigger correctly, stays reopenable, and survives the
declared export/backup/recovery route. These are explicit scope-item-5 gates.

Required correction: add a populated v94 fixture that runs the actual v95
migration and verifies row preservation, fail-closed NULL identities, indexes,
triggers, integrity, reopen and backup/restore behavior.

### F-8116-V6 — medium: a successful rename can return failure after it commits

`reconcile_confirmed_jira_key` commits at `jira.rs:1655` and only then resolves
the requested key on a new statement at `:1656`. Another writer can acquire the
write lock after that commit and rename the same immutable issue again before
the first caller's resolve. The first call has then changed durable state but can
return `NotFound` for its requested key or observe a later binding.

The checked-in race covers two different issues competing for one key. It does
not cover two new keys competing for the same immutable issue, so it cannot
detect this post-commit read window.

Required correction: derive/read the return value inside the committing
transaction and add a same-issue competing-rename case that proves no caller
reports a failed write after committing it.

## Known open/integration blockers preserved

- **OQ-002:** the concurrent first-open test passed this verification run, but a
  single pass cannot settle the recorded intermittent rate or its attribution.
- **OQ-004:** the exact MCP bootstrap journey still fails with
  `placement_blocked: the epic has no active immutable backlog code`. The handoff
  assigns the Jira-key tokens and seeded naming integration to ASMA-8117/8119;
  this report makes no competing ownership decision.
- **Migration sequencing:** this branch and ASMA-8117 both claim schema 0095.
  The handoff assigns final numbering to ASMA-8119 integration. The candidate is
  internally numbered consistently but is not directly composable as-is.

## Independent test record

| Command or probe | Result |
| --- | --- |
| `git diff --check` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy -p kontor-jira -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | passed |
| `cargo test -p kontor-store --test jira_materialization` | passed, 20/20 |
| temporary legacy exact-replay cross-ledger probe | **failed protection:** returned `Ok(())` |
| temporary two-row direct-SQL rename probe | **failed protection:** both updates succeeded |
| `cargo test -p kontor-jira` with loopback permission | **failed:** unit 9/9; native connector 12/17, 5 failures |
| `cargo test -p kontor-api` | passed, 23/23 + 5/5 + OpenAPI 3/3 |
| `cargo test -p kontor-store --test schema_v1` | passed, 57/57; does not settle OQ-002 |
| `cargo test -p kontor-daemon --test loopback_api jira` | passed, 9; 1 predeclared ignored |
| exact task/Jira public-projection loopback test | passed, 1/1 |
| exact empty-realm MCP bootstrap journey | **failed:** placement blocked, no seat started |
| `pnpm --filter kontor-console verify:api` | passed |
| `pnpm --filter kontor-console typecheck` | passed |
| `pnpm --filter kontor-console test` | **failed:** 299/300; inherited `DeepSeek V4 Flash` vs `V4.1 Flash` expectation in files unchanged by ASMA-8116 |

The first sandboxed connector run failed only because Wiremock could not bind a
port and is not used as candidate evidence; the unrestricted rerun above is.
The first console dependency attempt was interrupted after sandboxed DNS
failures; the permitted rerun installed the locked dependencies and produced
the recorded results.

## Exit criteria for another verification turn

1. Close F-8116-V1 and V2 with retained adversarial regressions on both ledgers
   and the complete raw-SQL sequence.
2. Close F-8116-V3 with a production connector/controller path and end-to-end
   same-id rename/different-id refusal coverage.
3. Restore `kontor-jira` to green and pin exact issue-id extraction/refusal.
4. Add the real populated v94-to-v95 preservation/recovery gate.
5. Close the post-commit return race or prove it impossible with a retained
   same-issue concurrency test.
6. Re-run format, clippy, store, connector, API/OpenAPI, focused daemon, schema,
   MCP journey and console gates on one exact new candidate. Preserve OQ-002,
   OQ-004 and the ASMA-8117 migration collision in their ledgers until their
   assigned integration owner settles them.
