# ASMA-8110 high-scope record: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-scope-record`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-scope`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Status: implementation contract amended after rejected high verification

## Decision and frozen baseline

Implement one bounded recovery command for an already-recorded rejected gate
verdict. Do not reimplement the normal verdict path.

The normal path is the merged candidate from
[PR #186](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/186): merge
`b2b5cad6e920fca7e5282873faef45da61e7328c`, source
`0ce4fc6960816b07a16321a4bf30cc1e2de35335`. It already makes the rejection
evaluation append and workflow routing one SQLite transaction and avoids
immediate forward advancement from old artifacts. The implementation must
extend that transaction with durable route/fresh-authoring evidence; it must
not duplicate or rewrite the gate-recording flow.

Scope was authored from the isolated `Carasent-ASMA/asma-rs-kontor` worktree on
`feat/ASMA-8110-gate-verdict-recovery` at
`c4b92f434f56eb511bb40c607525f6d302cfb048`, tracking `origin/master`. PR #186
is an ancestor. The later `origin/master` observation `9894060` contains only
unrelated ASMA-8090 naming recovery, so this scope turn does not rebase or edit
source. Kontor topology-drift receipt
`01a0739d-dca9-79c3-b2d3-6a8ac667773a` preserves the same TSW, native
workspace, `cwd`, and four logical seats.

## Remediation authority and operational gap

This amendment follows the rejected
[`HIGH-VERIFICATION-REPORT.md`](HIGH-VERIFICATION-REPORT.md) at
`3800cb27654c16dc075b4edf8955fd6b34b404d1`; that report rejects candidate
`d59e4eb3103c7e21947a288e726f7015c47f6955`. The remediation handoff records
that the recovered high-verification seat completed, gate sequence 1 is
durably rejected at workflow revision 3, and ASMA-8110 is now in
`high-implementation` revision 4. The candidate remains evidence, not an
accepted implementation.

`operational_gap`: Kontor session-message delivery returned `503 unavailable`
and explicitly changed nothing. This direct Paseo delivery is the bounded
fallback for the remediation handoff. No Kontor receipt exists for the failed
delivery, so none may be recorded or inferred. Every existing task, TeamRun,
AgentRun, seat, native-session, workspace, and `cwd` identity remains fixed.

## Required behavior

### 1. Future rejections

Keep the PR #186 behavior in
[`crates/kontor-store/src/repository.rs`](../../../crates/kontor-store/src/repository.rs)
and
[`crates/kontor-daemon/src/applications.rs`](../../../crates/kontor-daemon/src/applications.rs):

1. Validate the active workflow, pinned gate, evaluator authority, workflow
   revision, task revision, and pinned `rejection_target` before writing.
2. In one transaction, append the next gate-evaluation sequence, route the
   workflow to the gate's pinned rejection target, increment the workflow
   revision once, append the exact command result/receipt, and append the
   rejection-route record defined below, including the exact TeamRun selected
   for this task at route time.
3. A same-key/same-intent retry returns the original receipt and exact gate
   sequence. It does not append another evaluation/route or increment the
   workflow revision.
4. Restart preserves the evaluation, route, receipt, workflow id, routed phase,
   and revision. Reconciliation performs no second route.
5. A rejected verdict never calls ordinary evidence-based forward advancement
   in the recording request.

### 2. Historical recovery surface

Add one admin-only supported operation:

```text
HTTP POST /v1/projects/{project_id}/tasks/{task_id}/gates/{gate_id}/rejections:recover
MCP  kontor_gate_rejection_recover
CLI  kontor gate-rejection-recover
```

The HTTP request body and generated MCP/CLI arguments are exactly:

```json
{
  "rejection_receipt_id": "<command-receipt-id>",
  "sequence": 1,
  "expected_task_revision": 2,
  "expected_workflow_revision": 2,
  "expected_current_phase": "technical-review",
  "expected_rejection_target": "authoring"
}
```

`Idempotency-Key` is mandatory. `project_id`, `task_id`, and `gate_id` are path
arguments. The CLI spelling is:

```text
kontor gate-rejection-recover \
  --project-id <project-id> \
  --task-id <task-id> \
  --gate-id technical-review-gate \
  --rejection-receipt-id 01a07373-0b66-7b93-905f-c2a21bee494f \
  --sequence 1 \
  --expected-task-revision 2 \
  --expected-workflow-revision 2 \
  --expected-current-phase technical-review \
  --expected-rejection-target authoring \
  --idempotency-key asma-8100-technical-review-rejection-1-recovery
```

This spelling remains the frozen protocol example, but it is no longer an
authorized live ASMA-8100 invocation: that task has advanced to terminal
`done@5`. Generic qualification substitutes a non-terminal historical fixture's
exact values for every example value. No caller may infer an identity from a
Jira number.

Success is HTTP 200 with:

```json
{
  "realm_id": "<same-realm-id>",
  "task_id": "<same-task-id>",
  "workflow_id": "<same-active-workflow-id>",
  "gate": "technical-review-gate",
  "sequence": 1,
  "rejection_receipt_id": "01a07373-0b66-7b93-905f-c2a21bee494f",
  "prior_phase": "technical-review",
  "current_phase": "authoring",
  "prior_revision": 2,
  "current_revision": 3,
  "applied": "created",
  "receipt_id": "<recovery-command-receipt-id>"
}
```

A same-key/same-canonical-request retry, including after daemon restart, returns
the original result identity and state fields with `applied: "unchanged"` and
the same recovery receipt. It is judged before mutable-state preconditions. A
reused key with any changed argument returns `409 idempotency_conflict`. A fresh
key for a rejection already consumed returns `409 revision_conflict`, names the
original recovery receipt in its diagnostic, and changes nothing.

### 3. State preconditions and refusals

Before any write, the recovery transaction must prove all of the following from
durable state:

- the project and task exist, and the active workflow belongs to that exact
  task;
- the task revision and workflow revision equal both caller expectations;
- the workflow is active, the task is non-terminal, and its current phase is
  exactly `expected_current_phase`;
- the pinned workflow declares `gate_id`, its phase is the current phase, and
  its `rejection_target` exactly equals `expected_rejection_target`;
- the target is the pinned strict ancestor already validated by the frozen
  profile, never a caller-selected phase;
- `sequence` identifies an existing append-only evaluation for this exact
  project/workflow/gate, and its verdict is `rejected`;
- `rejection_receipt_id` is a durable `RecordGateVerdict` command receipt for
  this project and task whose canonical intent says the same gate and rejected
  verdict;
- every immutable evaluation field represented by the source intent matches
  sequence 1. For a legacy receipt without an exact result binding, the supplied
  `(gate, sequence)` is accepted only after this full comparison; the original
  receipt is never rewritten;
- a receipt payload that contains an exact result binding must parse and verify
  completely, including its stored payload hash, complete result object,
  workflow id, gate, and sequence. A malformed, partial, hash-invalid, or
  mismatched exact binding is an integrity/invalid-binding refusal; it must not
  be downgraded to the legacy no-binding path;
- the exact task has a TeamRun at route time. Resolve the last TeamRun in the
  repository's canonical `(created_at, id)` order inside the route transaction
  and persist that id on the route. Lifecycle is not a precondition;
- no rejection-route row already consumes the source receipt or the
  `(workflow, gate, sequence)` identity.

Wrong project/task, gate, sequence, receipt kind/target/intent, verdict, phase,
task revision, workflow revision, rejection target, terminal/inactive workflow,
or already-consumed source must fail before any row or revision changes.
Stale state returns `409 revision_conflict` with the current relevant revision;
invalid cross-identity requests return the existing typed not-found/invalid
request response. No refusal may change task lifecycle, Jira, runtime, TeamRun,
AgentRun, seat binding, native session, or workflow history.

### 4. Append-only route and fresh-authoring fence

Add migration `crates/kontor-store/migrations/0089_gate_rejection_routes.sql`
with one append-only `task_gate_rejection_routes` row per rejected evaluation.
Its immutable fields are:

```text
project_id, task_id, workflow_id, gate_key, gate_sequence,
rejection_receipt_id, route_receipt_id, route_origin,
team_run_id, from_phase, rejection_target, from_revision, to_revision, routed_at
```

Use a primary/unique key on `(project_id, workflow_id, gate_key,
gate_sequence)`, plus unique `rejection_receipt_id` and unique
`route_receipt_id`. `route_origin` is `recorded` for the ordinary PR #186 path
and `recovered` for the historical command. No update/delete API is added.

`team_run_id` is a route-time snapshot, not a lookup hint. Both the ordinary and
historical paths resolve it once in the same transaction that writes the route.
A later TeamRun for the same task cannot replace it, and fence evaluation must
never call `list_team_runs_for_task(...).last()` or otherwise recompute a
"current" run. The route references the existing TeamRun row and remains
append-only with the rest of the route identity.

The route row is also the freshness fence. While the active workflow remains at
that row's `rejection_target`, `advance_workflow_from_evidence` must return the
current workflow unless durable `role_turns` contains a later settled turn that:

- occurred strictly after `routed_at`;
- belongs to the same project/task and exactly the route's immutable
  `team_run_id`, regardless of that run's lifecycle;
- is from the authoring role named by the pinned edge leading **into** the
  rejection target, when such an inbound edge names a role; and
- carries every required artifact produced by the rejection-target phase.

This resolves OQ-8110-01 and OQ-8110-02. The direction is inbound: an outbound
edge names the reviewer/next consumer and must never qualify the rework that
releases its own rejection fence. An entry-phase rejection target has no
inbound edge, so no role is inferred from the outbound edge; for that shape the
immutable route-time TeamRun, fresh settlement time, and complete target-phase
artifact set identify the authoring turn. "Preserved active TeamRun" means the
route-time TeamRun identity, not a requirement that its lifecycle remain active.

Old `artifact_evidence` and earlier `role_turns` remain readable and unchanged,
but do not satisfy this fence. The qualifying fresh authoring settlement is
already append-only evidence; do not add a mutable "consumed" flag. Once that
turn exists, ordinary evidence advancement may proceed. A reviewer/auditor
turn, an empty turn, a turn for another task, a later same-task TeamRun, or an
authoring turn that does not carry the target phase's required artifacts must
not release the fence.

The fence and source-unique route record apply to both future rejections and
historical recovery. This closes the gap left by merely skipping immediate
advancement inside the gate-record request.

## Exact remediation implementation ownership

The rejected candidate already supplies the recovery surface. The remediation
seat must not rewrite it. It owns only this corrective delta:

- [`crates/kontor-core/src/repository.rs`](../../../crates/kontor-core/src/repository.rs):
  add immutable `team_run_id` to the typed route record.
- `crates/kontor-store/migrations/0089_gate_rejection_routes.sql`: add the
  route-time TeamRun column and referential binding before v89 is deployed.
- [`crates/kontor-store/src/repository.rs`](../../../crates/kontor-store/src/repository.rs):
  capture/read the route-time TeamRun, distinguish absent legacy bindings from
  invalid exact bindings, and preserve the conflicting route receipt.
- [`crates/kontor-daemon/src/applications.rs`](../../../crates/kontor-daemon/src/applications.rs):
  evaluate the fence against `route.team_run_id` and return the original route
  receipt for a concurrent fresh-key conflict.
- `Cargo.lock`: commit the registry-resolved lockfile required by the
  authoritative archive gate; no dependency or manifest change is authorized.

Tests may change only
`crates/kontor-daemon/tests/loopback_api.rs`,
`crates/kontor-store/tests/repository_roundtrip.rs`,
`crates/kontor-store/tests/backup_snapshot.rs`, and
`crates/kontor-store/tests/schema_v1.rs`. Existing API, MCP, CLI, profile,
topology, runtime, Jira, task-lifecycle, and normal gate-verdict behavior is
frozen. A dependency outside these files returns to scope with evidence before
editing.

## Verification contract

### Required tests

Extend the existing PR #186 regression
`a_phase_advancing_gate_replays_after_revision_change_and_restart` in
[`crates/kontor-daemon/tests/loopback_api.rs`](../../../crates/kontor-daemon/tests/loopback_api.rs)
to assert one evaluation, one route, one revision increment, exact receipt
replay, and unchanged routed state after restart.

Retain the existing recovery tests and add or extend these remediation cases:

- `a_historical_gate_rejection_recovery_routes_once_and_replays_after_restart`
  in `crates/kontor-daemon/tests/loopback_api.rs`.
- `gate_rejection_recovery_refuses_wrong_task_gate_sequence_receipt_phase_target_and_revisions_without_writes`
  in the same file; table-drive wrong existing project/task binding, undeclared
  gate, missing/wrong sequence, missing/wrong receipt, wrong receipt target or
  canonical intent fields, non-rejected evaluation, phase, target, stale task
  revision, stale workflow revision, terminal task, and inactive workflow.
  Every row compares a before/after database and workflow census.
- `gate_rejection_recovery_refuses_same_key_with_changed_intent_without_writes`
  in the same file; require `409 idempotency_conflict` and the original receipt.
- `gate_rejection_recovery_refuses_invalid_exact_bindings_but_accepts_an_absent_legacy_binding`
  in the same file or the store round-trip suite; independently corrupt the
  stored payload hash, remove part of the exact result object, and mismatch the
  exact workflow/sequence binding. Each invalid exact binding must refuse with
  no writes. Only a payload that contains no result binding at all may use the
  legacy intent/evaluation comparison.
- `concurrent_fresh_recovery_keys_name_the_single_original_route_receipt` in
  the loopback file; synchronize two different fresh keys before either route
  commits, require one 200 and one 409, require the refusal's `diagnostic.at`
  to be `command-receipts/{winning-route-receipt-id}`, and prove one route, one
  revision increment, and no lost diagnostic.
- `legacy_artifacts_do_not_advance_a_recovered_rejection_until_a_fresh_authoring_turn_settles`
  in the same file; seed old authoring/review artifacts, recover, prove repeated
  reconcile and unrelated settlements leave the workflow in `authoring`, then
  settle the route-time TeamRun's authoring seat with fresh required artifacts
  and prove advancement is permitted.
- `a_later_same_task_team_run_cannot_release_an_earlier_rejection_fence` in the
  same file; route using TeamRun A, create TeamRun B for the same task, settle
  B with the otherwise qualifying role and artifacts, and prove the workflow
  stays fenced. Settle the equivalent fresh turn on A and prove it releases.
  Read the route before and after and require `team_run_id == A` throughout.
- `a_gate_rejection_route_and_exact_receipt_commit_or_roll_back_together` in
  [`crates/kontor-store/tests/repository_roundtrip.rs`](../../../crates/kontor-store/tests/repository_roundtrip.rs).
- `a_historical_rejection_recovery_is_source_unique_append_only_and_survives_reopen`
  in the same store test.
- command-kind target/revision coverage in
  [`crates/kontor-core/tests/domain_state.rs`](../../../crates/kontor-core/tests/domain_state.rs).
- admin tier, required input schema, URL, and generated CLI spelling in
  `crates/kontor-mcp/src/registry.rs`,
  [`tests/contract/mcp_parity.rs`](../../../tests/contract/mcp_parity.rs), and
  [`crates/kontor-cli/src/commands.rs`](../../../crates/kontor-cli/src/commands.rs).
- migration/backup round-trip coverage in
  [`crates/kontor-store/tests/backup_snapshot.rs`](../../../crates/kontor-store/tests/backup_snapshot.rs)
  and `crates/kontor-store/tests/schema_v1.rs`, including the immutable TeamRun
  foreign key and archive/reopen readback.

Run focused tests first, then the repository gate:

```text
cargo test --locked -p kontor-store --test repository_roundtrip gate_rejection
cargo test --locked -p kontor-daemon --test loopback_api gate_rejection
cargo test --locked -p kontor-core --test domain_state command_kind
cargo test --locked -p kontor-mcp
cargo test --locked -p kontor-cli
python3 scripts/verify-tree.py --mode archive
```

The final command is authoritative: it runs format, clippy, locked workspace
tests, audit/deny, frozen pnpm install, typecheck, Vitest, and production
dependency audit from an exported committed tree. The remediation commit must
include the regenerated `Cargo.lock`; an online lock regeneration in the
archive verifier must byte-compare to that committed file. Retain the complete
successful archive output and exact candidate SHA. An offline-only match does
not qualify a candidate whose online archive regeneration differs.

### Mutation cases

Each mutation must make at least one named test fail, then the unmodified test
must pass again:

| ID | Deliberate defect | Required killer |
| --- | --- | --- |
| MUT-8110-01 | Move route update outside the verdict/receipt transaction | atomic store rollback test |
| MUT-8110-02 | Route to current/first phase instead of pinned target | future-route loopback test |
| MUT-8110-03 | Remove unique source receipt/evaluation constraint | source-unique store test |
| MUT-8110-04 | Increment workflow revision on same-key retry/restart | retry/restart loopback test |
| MUT-8110-05 | Substitute latest gate evaluation for requested sequence | mismatch and exact-receipt tests |
| MUT-8110-06 | Ignore task or workflow expected revision | table-driven refusal census |
| MUT-8110-07 | Ignore task/gate/sequence/phase/target comparison | table-driven refusal census |
| MUT-8110-08 | Let pre-route artifacts satisfy authoring freshness | legacy-artifact suppression test |
| MUT-8110-09 | Let wrong-role/empty/unrelated turn release the fence | legacy-artifact suppression test |
| MUT-8110-10 | Make route/fence state disappear on reopen | reopen tests |
| MUT-8110-11 | Recompute the latest TeamRun instead of using the route-time id | two-TeamRun same-task fence test |
| MUT-8110-12 | Treat an invalid exact result binding as legacy/no binding | invalid exact-binding test |
| MUT-8110-13 | Drop the winning receipt from the concurrent loser diagnostic | concurrent fresh-key test |
| MUT-8110-14 | Permit terminal/inactive recovery or omit an exact mismatch check | expanded refusal census |
| MUT-8110-15 | Restore the stale committed lockfile | authoritative online archive gate |

## Qualification, same-realm deployment, and protected ASMA-8100 readback

The verifier's read-only census supersedes the former live recovery plan.
ASMA-8100 task `01a06e13-878c-7080-9801-d984bfe0eb04` is terminal `done@5`;
its active `docs@1` workflow `01a06e1d-b3a4-78d1-91b9-87d8d6f7d03d` is at
`final-review@5`, and later gate history includes a passed technical-review
sequence 4. Receipt `01a07373-0b66-7b93-905f-c2a21bee494f` still binds exactly
to technical-review sequence 1, but its frozen revision-2 recovery is now stale
and terminal. The safe disposition is **retired live recovery**: do not invoke
it, reopen/rewind the task, fabricate an authoring turn, or otherwise mutate
ASMA-8100.

Generic historical recovery remains release-blocking. The delivery owner—not
the scope or implementation seat—performs this sequence:

1. Freeze the reviewed remediation commit, including `Cargo.lock`. Run
   `python3 scripts/verify-tree.py --mode archive` with registry access and
   retain its complete successful output and exact SHA.
2. In that exported committed tree, require the non-terminal historical fixture
   to prove created/replayed recovery across restart, route-time TeamRun
   fencing, source uniqueness, invalid-binding refusals, stale/mismatch/terminal
   refusals, and concurrent winning-receipt diagnostics. This is the generic
   historical-recovery qualification; ASMA-8100's terminal state does not waive
   it.
3. Build that exact commit with
   `cargo build --release --locked -p kontor-cli -p kontor-daemon -p kontor-mcp`.
   Record commit and SHA-256 for `kontor`, `kontor-daemon`, and `kontor-mcp`.
4. Before stopping the current daemon, read and retain the realm id and
   ASMA-8100's exact task/workflow state, gate history and source receipt, plus
   every task, TeamRun, AgentRun, seat-binding, native-session, generation,
   workspace, and `cwd` identity in the existing topology.
5. Stop only this realm's daemon, take a recoverable copy of its state root, and
   promote the exact qualified binaries through the existing same-realm path.
   Do not create/import a realm, materialize topology, replace a seat, launch a
   TeamRun, reopen a task, or issue a recovery command. Restart against the same
   state root.
6. Run SQLite `PRAGMA integrity_check;` and `PRAGMA foreign_key_check;`. Require
   `ok`, zero foreign-key rows, schema version 89, and migration 0089 exactly
   once.
7. Read the realm, topology, ASMA-8100, its workflow, source receipt, gate
   history, and route ledger back. Require exact identity equality with step 4,
   `done@5`, `final-review@5`, the later passed evaluation unchanged, and no
   route row consuming receipt `01a07373-0b66-7b93-905f-c2a21bee494f`.
8. Do not use the live task to demonstrate refusal: even a refused command can
   create command-attempt evidence. The terminal and stale recovery refusals
   are proved by the exact automated no-write census from step 2. Restart once
   more, repeat both PRAGMAs and the readback, and require no identity or task
   state change.

Rollback is binary rollback plus restoration of the pre-deploy state-root copy
only if migration/startup fails. No successful live recovery is authorized in
this disposition; preserve all append-only history and roll forward after any
post-start defect.

## Acceptance criteria

- PR #186 remains the sole implementation of ordinary rejected-verdict routing.
- Future rejected verdict, pinned target route, source-specific route record,
  immutable route-time TeamRun, workflow revision, and exact receipt commit or
  roll back together.
- A later TeamRun for the same task cannot release an earlier route's fence;
  only a qualifying fresh turn from the route's stored TeamRun can release it.
- The authoring-role rule uses the inbound edge. It never treats the outbound
  reviewer role as the author; entry targets with no inbound edge infer no role.
- Same-key retry/restart is stable; changed intent, stale state, wrong identity,
  and second-key consumption are refused with zero effects.
- Invalid exact receipt bindings fail closed; only a truly absent result binding
  may use legacy comparison. Concurrent fresh-key refusal names the winning
  route receipt.
- Gate/evaluation/artifact history stays append-only.
- Old durable artifacts cannot move the recovered workflow out of `authoring`
  before a fresh qualifying authoring settlement.
- Generic historical recovery and stale/terminal refusal pass from the exported
  committed tree; full archive verification and all 15 mutations are
  green/killed against the committed lockfile.
- Schema migration and both database integrity checks pass in the same realm.
- Task, workflow, TeamRun, AgentRun, logical seat, native session, workspace,
  generation, and `cwd` identities are preserved.
- ASMA-8100 is never recovered or otherwise mutated and reads back terminal
  `done@5` with its workflow at `final-review@5`; its sequence-1 receipt remains
  historical and unconsumed.

## Risks and controls

| Risk | Control |
| --- | --- |
| A recovery command becomes a second loose gate-recording path | Admin-only surface; source receipt plus complete evaluation/intent comparison; no new verdict row |
| A retry or second key routes twice | Replay-first idempotency plus unique source/evaluation route constraints |
| A concurrent loser omits the winning receipt | Transaction returns the existing route, or the service re-reads that exact conflict before responding |
| A corrupt exact binding enters legacy recovery | Distinguish absent binding from parse/integrity failure; fail closed on the latter |
| A stale request rewinds newer work | Task/workflow CAS, exact current phase, pinned target, active/non-terminal checks |
| A later same-task TeamRun releases an old fence | Store `team_run_id` on the route and compare only that immutable identity |
| Old evidence immediately undoes the route | Append-only route-time fence released only by a fresh inbound-authoring settlement with target artifacts |
| Qualification rewinds terminal ASMA-8100 | No live invocation; automated stale/terminal refusal plus read-only same-realm readback |
| A migration damages live state | Atomic migration, pre/post integrity and FK checks, state-root backup, same-realm readback |
| Deployment silently replaces runtime identities | Before/after identity census; no topology or scheduler mutations |
| Registry drift makes the archive irreproducible | Commit the regenerated lock and require online archive byte equality from the exact SHA |
| Later unrelated master changes widen the patch | Implement from this contract and PR #186 ancestry; rebase/integration is a separate owner action |

## Open questions

None. OQ-8110-01 is resolved to the inbound authoring edge; OQ-8110-02 is
resolved to immutable route-time TeamRun identity without a liveness condition;
and the verifier's OQ-8110-03 is resolved by retiring the live ASMA-8100
recovery while retaining generic qualification. A future request to recover a
terminal or later-advanced task requires new scope and must not reinterpret this
contract.
