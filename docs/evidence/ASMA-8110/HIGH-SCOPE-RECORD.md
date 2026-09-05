# ASMA-8110 high-scope record: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-scope-record`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-scope`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Status: implementation contract ready

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
   rejection-route record defined below.
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

The operator must replace only `<project-id>` and `<task-id>` with the fresh
confirmed ASMA-8100 binding readback. If either differs from the receipt's
durable target, the command refuses; it never infers an identity from the Jira
number.

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
from_phase, rejection_target, from_revision, to_revision, routed_at
```

Use a primary/unique key on `(project_id, workflow_id, gate_key,
gate_sequence)`, plus unique `rejection_receipt_id` and unique
`route_receipt_id`. `route_origin` is `recorded` for the ordinary PR #186 path
and `recovered` for the historical command. No update/delete API is added.

The route row is also the freshness fence. While the active workflow remains at
that row's `rejection_target`, `advance_workflow_from_evidence` must return the
current workflow unless durable `role_turns` contains a later settled turn that:

- occurred strictly after `routed_at`;
- belongs to the same project/task and the preserved active TeamRun;
- is from the pinned handoff role that owns the edge out of the rejection
  target; and
- carries every required artifact produced by the rejection-target phase.

Old `artifact_evidence` and earlier `role_turns` remain readable and unchanged,
but do not satisfy this fence. The qualifying fresh authoring settlement is
already append-only evidence; do not add a mutable "consumed" flag. Once that
turn exists, ordinary evidence advancement may proceed. A reviewer/auditor
turn, an empty turn, a turn for another task/run, or an authoring turn that does
not carry the target phase's required artifacts must not release the fence.

The fence and source-unique route record apply to both future rejections and
historical recovery. This closes the gap left by merely skipping immediate
advancement inside the gate-record request.

## Exact implementation ownership

The implementation seat owns only these production surfaces:

- [`crates/kontor-core/src/receipt.rs`](../../../crates/kontor-core/src/receipt.rs):
  add the distinct `RecoverGateRejection` task-target command kind and its
  witnessed revision rule.
- [`crates/kontor-core/src/repository.rs`](../../../crates/kontor-core/src/repository.rs):
  typed recovery/route records and repository contract.
- `crates/kontor-store/migrations/0089_gate_rejection_routes.sql`: append-only
  route/fence schema.
- [`crates/kontor-store/src/repository.rs`](../../../crates/kontor-store/src/repository.rs):
  atomic future route extension, historical recovery transaction, source
  validation, route readback, and fresh-turn query.
- [`crates/kontor-api/src/applications.rs`](../../../crates/kontor-api/src/applications.rs):
  request/result DTO, admin route, and operation trait.
- `crates/kontor-daemon/src/applications.rs`: recovery service and freshness
  guard in the existing evidence-advance path.
- [`crates/kontor-mcp/src/registry.rs`](../../../crates/kontor-mcp/src/registry.rs):
  the single tool definition from which the CLI command is generated.

Tests may touch only the matching existing contract files named below. No UI,
Jira workflow, profile, team topology, runtime adapter, task lifecycle, or
normal gate-recovery citation change belongs to ASMA-8110. If implementation
requires a different production file, return to scope with the concrete
dependency before editing it.

## Verification contract

### Required tests

Extend the existing PR #186 regression
`a_phase_advancing_gate_replays_after_revision_change_and_restart` in
[`crates/kontor-daemon/tests/loopback_api.rs`](../../../crates/kontor-daemon/tests/loopback_api.rs)
to assert one evaluation, one route, one revision increment, exact receipt
replay, and unchanged routed state after restart.

Add the following behavior tests:

- `a_historical_gate_rejection_recovery_routes_once_and_replays_after_restart`
  in `crates/kontor-daemon/tests/loopback_api.rs`.
- `gate_rejection_recovery_refuses_wrong_task_gate_sequence_receipt_phase_target_and_revisions_without_writes`
  in the same file; table-drive every mismatch and compare a before/after
  census.
- `legacy_artifacts_do_not_advance_a_recovered_rejection_until_a_fresh_authoring_turn_settles`
  in the same file; seed old authoring/review artifacts, recover, prove repeated
  reconcile and unrelated settlements leave the workflow in `authoring`, then
  settle the preserved authoring seat with fresh required artifacts and prove
  advancement is permitted.
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
  [`crates/kontor-store/tests/backup_snapshot.rs`](../../../crates/kontor-store/tests/backup_snapshot.rs).

Run focused tests first, then the repository gate:

```text
cargo test -p kontor-store --test repository_roundtrip gate_rejection
cargo test -p kontor-daemon --test loopback_api gate_rejection
cargo test -p kontor-core --test domain_state command_kind
cargo test -p kontor-mcp
cargo test -p kontor-cli
python3 scripts/verify-tree.py --mode archive
```

The final command is authoritative: it runs format, clippy, locked workspace
tests, audit/deny, frozen pnpm install, typecheck, Vitest, and production
dependency audit from an exported committed tree.

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

## Qualification, same-realm deployment, and ASMA-8100 recovery

The delivery owner—not the scope or implementation seat—performs this sequence:

1. Freeze the reviewed implementation commit. Run
   `python3 scripts/verify-tree.py --mode archive`; retain its complete output.
2. Build that exact commit with
   `cargo build --release --locked -p kontor-cli -p kontor-daemon -p kontor-mcp`.
   Record commit and SHA-256 for `kontor`, `kontor-daemon`, and `kontor-mcp`.
3. Before stopping the current daemon, read and retain ASMA-8100's confirmed
   project/task binding, active workflow id/revision/phase, gate sequence 1 and
   receipt, task revision/state, TeamRun ids, AgentRun ids, seat-binding ids,
   native session ids, generations, workspace id, and `cwd`.
4. Stop only this realm's daemon, take a recoverable copy of its state root, and
   promote the three exact qualified binaries through the existing same-realm
   installation path. Do not create a realm, import state, materialize topology,
   replace a seat, or launch another TeamRun. Restart the daemon against the
   same state root.
5. Before recovery, run SQLite `PRAGMA integrity_check;` and
   `PRAGMA foreign_key_check;`. Require `ok` and zero foreign-key rows. Read the
   schema as version 89 and prove migration 0089 exists once.
6. Read the realm, ASMA-8100 task, workflow, route ledger, and topology back.
   Byte-compare the identifiers captured in step 3; only the installed binary
   hashes and schema version may differ.
7. Invoke the exact CLI recovery command above once using the fresh confirmed
   project/task ids. Require sequence 1, source receipt
   `01a07373-0b66-7b93-905f-c2a21bee494f`, prior `technical-review@2`, and
   result `authoring@3` (`workflow_revision > 2`).
8. Repeat with the same key before and after one daemon restart. Require the
   original recovery receipt, `applied: unchanged`, one evaluation row, one
   route row, workflow revision 3, and no second effect. A fresh key for the
   same source must refuse without changing the census.
9. Re-run both PRAGMAs. Re-read task/run/seat/native identities and require exact
   equality with step 3. Require ASMA-8100 to remain in `authoring` while only
   legacy artifacts exist and across reconciliation/restart.
10. Hand the preserved authoring seat a new bounded turn. Only its newly settled
    required artifact may release the fence. Record that settlement and the
    resulting workflow readback as separate verification evidence; do not
    fabricate it during deployment.

Rollback is binary rollback plus restoration of the pre-deploy state-root copy
only if migration/startup fails before the recovery command. After a successful
recovery command, do not restore an older database over the append-only receipt;
roll forward with a corrected binary and preserve the recovery history.

## Acceptance criteria

- PR #186 remains the sole implementation of ordinary rejected-verdict routing.
- Future rejected verdict, pinned target route, source-specific route record,
  workflow revision, and exact receipt commit or roll back together.
- Receipt `01a07373-0b66-7b93-905f-c2a21bee494f`, ASMA-8100,
  `technical-review-gate`, and sequence 1 are consumed exactly once.
- Same-key retry/restart is stable; changed intent, stale state, wrong identity,
  and second-key consumption are refused with zero effects.
- Gate/evaluation/artifact history stays append-only.
- Old durable artifacts cannot move the recovered workflow out of `authoring`
  before a fresh qualifying authoring settlement.
- Full exported-tree verification and all ten mutation cases are green/killed.
- Schema migration and both database integrity checks pass in the same realm.
- Task, workflow, TeamRun, AgentRun, logical seat, native session, workspace,
  generation, and `cwd` identities are preserved.
- ASMA-8100 reads back at `authoring` with workflow revision greater than 2.

## Risks and controls

| Risk | Control |
| --- | --- |
| A recovery command becomes a second loose gate-recording path | Admin-only surface; source receipt plus complete evaluation/intent comparison; no new verdict row |
| A retry or second key routes twice | Replay-first idempotency plus unique source/evaluation route constraints |
| A stale request rewinds newer work | Task/workflow CAS, exact current phase, pinned target, active/non-terminal checks |
| Old evidence immediately undoes the route | Append-only route-time fence released only by a fresh owning-role settlement with target artifacts |
| A migration damages live state | Atomic migration, pre/post integrity and FK checks, state-root backup, same-realm readback |
| Deployment silently replaces runtime identities | Before/after identity census; no topology or scheduler mutations |
| Later unrelated master changes widen the patch | Implement from this contract and PR #186 ancestry; rebase/integration is a separate owner action |

## Open questions

None. Any newly discovered mismatch in ASMA-8100's fresh task/workflow revision,
phase, source receipt, sequence, pinned target, or confirmed binding blocks the
recovery invocation and must be attached to ASMA-8110 before an alternative is
chosen.
