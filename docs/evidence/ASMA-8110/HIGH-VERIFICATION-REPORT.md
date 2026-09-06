# ASMA-8110 high-verification report: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-verification-report`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-verification`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Candidate: `d59e4eb3103c7e21947a288e726f7015c47f6955`
Status: **rejected — implementation changes required**

## Verdict

Candidate `d59e4eb3103c7e21947a288e726f7015c47f6955` does not satisfy the
high-verification gate. The atomic route/recovery core and its focused happy
paths work, but one safety-fence identity is evaluated against mutable later
state, invalid exact receipt bindings are silently treated as legacy, the
fresh-key concurrency path can lose its required receipt diagnostic, and the
authoritative exported-tree gate is red. The frozen refusal and fence test
matrix is also incomplete.

No production source was changed during verification. No daemon was started,
no schema was migrated, no recovery was invoked, and no Jira/runtime/topology
state was mutated.

## Candidate and handoff

- Frozen handoff artifact: `high-change` at
  `d59e4eb3103c7e21947a288e726f7015c47f6955`.
- Reviewed implementation commit: `a7217ef`; `d59e4eb` changes only this task's
  high-change document after it.
- Merge base declared by the scope and reproduced by Git:
  `98940604d9a554aa24e0d87474bf0807232499e5`.
- The worktree was clean at handoff. The only verification-seat change is this
  report.
- The task's pinned profile is
  `asma-high-stakes-primary-20260829@1`; its durable workflow was read at
  `high-verification@3`, and implement turn ordinal 2 was settled with the
  canonical `high-change` artifact.

## Findings

### F-8110-01 — high: a later TeamRun can become the fence's “preserved” run

The scope requires the releasing turn to belong to the TeamRun preserved when
the rejection was routed, and expressly says a turn from another run must not
release the fence (`HIGH-SCOPE-RECORD.md:178-193`). The route row stores no
TeamRun identity. At every advancement attempt,
`rejection_fence_holds` calls `list_team_runs_for_task(...).last()`
(`crates/kontor-daemon/src/applications.rs:1159-1163`). That repository query
orders all runs oldest-first (`crates/kontor-store/src/graph.rs:1431-1445`), so
any TeamRun created later for the same task replaces the route-time run in the
predicate. A qualifying artifact turn on that later run then passes
`turn.team_run_id == preserved_run` and releases the fence.

This is not an append-only identity check; it is a mutable latest-run lookup.
The fence test settles only roles in the original `runs` vector
(`loopback_api.rs:10341-10399`) and never creates or settles another same-task
TeamRun, so the required negative case is absent.

Required correction: bind the fence to the route-time TeamRun identity, or
obtain a scope ruling for another immutable derivation. Adding `team_run_id` to
the route is the direct design, but the scope's exact field list omits it and
therefore needs an explicit scope amendment. Add a regression with two
same-task TeamRuns proving the later run cannot release the earlier route.

### F-8110-02 — high: the authoritative archive gate is red

`python3 scripts/verify-tree.py --mode archive` failed at its first gate on the
frozen candidate. Online `cargo generate-lockfile` changed only:

```diff
-name = "ipnet"
-version = "2.12.1"
-checksum = "6a756c3fac73139e83f14c2d742155dd2b78d3ee56597b419a0579b7bdd6dd78"
+name = "ipnet"
+version = "2.12.2"
+checksum = "791930b43c0d5973160d90a8f3894509f2b273430f5c5c73b668636d0287c5c0"
```

The script reported `Cargo.lock regeneration differs byte-for-byte from the
committed lockfile` and exited 1 before format, clippy, tests, audits or pnpm
gates. An offline regeneration remains byte-identical, which isolates the
failure to current registry resolution rather than a local edit, but the scope
declares this online exported-tree command authoritative. Update/freeze the
lock as project policy requires and rerun the complete archive gate on the new
candidate.

### F-8110-03 — medium: an invalid exact result binding is downgraded to legacy

`bound_gate_record_result` reads any stored outbox payload and then calls
`parse_gate_record_result(...).ok()`
(`crates/kontor-store/src/repository.rs:11673-11689`). Consequently a payload
whose hash is wrong, whose `result` object is partial, or whose binding is
otherwise invalid becomes `None`, the same value used for a genuinely old
receipt with no result binding. Recovery then falls back to matching intent and
evaluation fields. With repeated otherwise-identical rejections, that can
consume a caller-selected sequence even though the source receipt's corrupted
binding did not authorize it.

The no-binding legacy case must remain supported. The invalid-binding case must
propagate the parse/integrity error. Add negative tests for a bad payload hash,
a partial result object and an invalid exact workflow/sequence binding; only a
payload with no result binding at all may take the legacy comparison path.

### F-8110-04 — medium: a concurrent second key can lose the original-receipt diagnostic

The service first reads `gate_rejection_route_by_rejection` in one
`with_store` lock window and adds `located_at(command-receipts/{id})` if it
finds a route (`crates/kontor-daemon/src/applications.rs:24746-24766`). It then
releases that lock and later opens the recovery transaction. Two different
fresh keys can both observe no route. The first transaction commits; the
second transaction detects the route at
`crates/kontor-store/src/repository.rs:12152-12170` and returns only the generic
`already_routed()` conflict. Mapping that error back to the API has no route
receipt with which to populate `diagnostic.at`.

The state effect is still prevented, but the response no longer meets the
scope's requirement that every fresh-key already-consumed refusal name the
original recovery receipt (`HIGH-SCOPE-RECORD.md:120-125`). Return the existing
route from the transactional conflict path, or re-read and decorate that exact
conflict after the failed transaction. Add a concurrent two-key test.

### F-8110-05 — medium: the required negative matrix is incomplete

The named refusal test covers a nonexistent task, undeclared gate, missing
sequence, wrong/missing receipt id, phase, target and two stale revisions
(`loopback_api.rs:10119-10225`). It does not cover the scope's full list at
`HIGH-SCOPE-RECORD.md:153-155`: wrong existing project/task binding, wrong
receipt target or canonical intent fields, a non-rejected evaluation, a
terminal task, or an inactive workflow. It also does not test same-key changed
intent. The fence test covers a reviewer turn and an empty turn, but not another
task/run despite the explicit requirement and MUT-8110-09.

Add those cases with the same before/after census. The implementation report's
mutation narrative is useful, but the final tree does not independently prove
the omitted killers.

## Verified behavior

Static review and the passing focused tests support the following parts of the
candidate:

- migration 0089 creates the append-only route table, source/evaluation and
  route-receipt uniqueness, and update/delete triggers;
- a future rejection appends its evaluation, exact receipt result, route and
  single workflow revision in one SQLite transaction;
- the historical command is distinct, admin-only, chooses the frozen profile's
  target, appends no evaluation, and handles ordinary replay before mutable
  preconditions;
- the focused recovery, route atomicity, reopen, contract, CLI/MCP and migration
  tests pass under the committed lock;
- `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets --locked -- -D warnings` pass.

## Test record

| Command | Result |
|---|---|
| `python3 scripts/verify-tree.py --mode archive` | **failed**: online lock regeneration changed `ipnet` 2.12.1 → 2.12.2 |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --locked -p kontor-store --test repository_roundtrip rejection` | 2 passed |
| `cargo test --locked -p kontor-daemon --test loopback_api rejection` | 3 passed |
| `cargo test --locked -p kontor-core --test domain_state command_kind` | 1 passed |
| `cargo test --locked -p kontor-mcp` | 63 passed |
| `cargo test --locked -p kontor-cli` | 22 passed after rerunning outside the socket-restricted sandbox |
| `cargo test --locked -p kontor-tests-contract --test mcp_parity` | 12 passed |
| `cargo test --locked -p kontor-store --test backup_snapshot gate_rejection` | 1 passed |
| `cargo test --locked -p kontor-store --test schema_v1` | 56 passed, 1 transient `database is locked` failure |
| exact rerun of `a_concurrent_first_open_initializes_exactly_one_realm` | passed |
| `cargo test --workspace --locked` | passed (exit 0) |

The initial archive attempt inside the filesystem/network sandbox failed only
because crates.io DNS was unavailable. The recorded authoritative result above
is the rerun with registry access. The first CLI run likewise hit a sandbox
loopback-bind denial; the authorized rerun passed.

## Inherited unresolved questions

OQ-8110-01 and OQ-8110-02 remain open in
[`HIGH-CHANGE-RECORD.md`](HIGH-CHANGE-RECORD.md). In particular, the candidate
implements the authoring role named by the edge *into* the rejection target,
while the literal scope says the edge *out of* it. The scope's examples and
mutation requirement contradict that literal wording, so verification does not
invent a resolution. F-8110-01 is independent of that role-direction question:
whichever role is selected, its turn must still belong to the preserved
route-time TeamRun.

## Open questions

### OQ-8110-03 — the live ASMA-8100 state no longer satisfies the frozen recovery invocation

Subject: whether and how the ASMA-8100 same-realm qualification remains required
after that task advanced beyond the rejection the command was scoped to consume.

Attaches to: `ASMA-8110`,
[`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md) section "Qualification,
same-realm deployment, and ASMA-8100 recovery", and ASMA-8100 task
`01a06e13-878c-7080-9801-d984bfe0eb04`.

Why ambiguous: the frozen procedure requires ASMA-8100 to be a non-terminal
task at task revision 2 with its active `docs@1` workflow at
`technical-review@2`, then expects the recovery to route it to `authoring@3`.
A fresh read-only census of the same realm on 2026-09-06 instead finds the task
`done@5` and its active workflow at `final-review@5`. The gate history now also
contains later technical-review evaluations through sequence 4 (passed) and
passed final-review evaluations. Source receipt
`01a07373-0b66-7b93-905f-c2a21bee494f` still binds exactly to workflow
`01a06e1d-b3a4-78d1-91b9-87d8d6f7d03d`, gate sequence 1, but the recovery's
terminal-task, task-revision, workflow-revision and current-phase preconditions
must now refuse it. The scope expressly says such a fresh mismatch blocks the
invocation and must be attached before an alternative is chosen.

Options seen:

1. Treat the live recovery objective as superseded because ASMA-8100 completed,
   and qualify ASMA-8110's generic recovery behavior without mutating ASMA-8100.
2. Qualify against a recoverable historical copy that still has the frozen
   `technical-review@2` state, accepting that this is not the current same-realm
   state named by the scope.
3. Rescope a new non-terminal fixture/task for same-realm qualification and
   explicitly waive the ASMA-8100-specific acceptance criterion.
4. Require a new scoped recovery design for completed/later-advanced workflows;
   the current command correctly refuses that expansion.

No option is selected in this verification turn. Verification therefore covers
the candidate code and tests only; it performs no deployment or recovery
invocation and does not treat the ASMA-8100-specific acceptance criterion as
satisfied.

## Same-realm read-only census

The observer daemon was unavailable, so verification did not infer live state
from a failed API call. A read-only SQLite census found:

- schema version 88; migration 0089 and its route table are not deployed;
- `PRAGMA integrity_check` is `ok` and `PRAGMA foreign_key_check` returns no
  rows;
- ASMA-8100 task `01a06e13-878c-7080-9801-d984bfe0eb04` is `done@5`;
- active `docs@1` workflow `01a06e1d-b3a4-78d1-91b9-87d8d6f7d03d` is at
  `final-review@5`;
- the technical-review gate now has four evaluations, with sequence 4 passed;
- source receipt `01a07373-0b66-7b93-905f-c2a21bee494f` has an exact result
  binding to that workflow and technical-review sequence 1. It is not the
  no-result legacy shape described in the implementation handoff.

These facts are observations only. OQ-8110-03 records the decision they require.

## Exit criteria for re-verification

1. Correct F-8110-01, F-8110-03 and F-8110-04, and add the missing tests in
   F-8110-05.
2. Obtain and record scope dispositions for OQ-8110-01, OQ-8110-02 and
   OQ-8110-03 before relying on any affected behavior.
3. Produce a new frozen commit whose authoritative archive gate passes in full.
4. Keep same-realm deployment and any ASMA-8100 mutation with the delivery
   owner after OQ-8110-03 is resolved.
