Artifact: `high-scope-record`

# ASMA-8239 / PUB-09 high-scope record: Admin NativeChild container recreation

Date: 2026-09-20
Task: Jira `ASMA-8239` / Kontor `01a0bbbc-6e93-7b22-980e-3c023109b1fb`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-scope`
TeamRun: `01a0bbc2-fa0f-7932-a6c6-887210308972`
Scope AgentRun: `01a0bbc2-fa0f-7932-a6c6-888c9b799700`
Implement AgentRun: `01a0bbc3-1184-7ce0-9204-452107d52371`
Standing authority revision: `01a0b9a6-6a23-7042-a780-1430cd021032`
Source baseline: `267c67a7`

## Frozen outcome

Extend only the existing Admin
`kontor_container_recovery_preview` / `kontor_container_recovery_apply`
operation. For an already-persisted `NativeChild` whose exact persisted native
is absent and whose fresh exact-parent/canonical-path census returns zero
candidates, preview may authorize apply to create exactly one replacement
native and bind it to the same logical topology node.

This is a narrow second disposition beside the operation's existing
single-candidate adoption behavior. It is not a new recovery operation, a
generic recreation facility, a new topology, a new workspace/worktree, or a
replacement task/TeamRun.

The preserved identity and placement tuple is:

```text
topology node id
logical container binding id and kind = NativeChild
canonical cwd
native parent id
current rendered visible title
```

The caller does not supply or override that tuple. The server derives it from
the current persisted binding and the same `prepare_container` / NativeChild
placement rules used by ordinary creation.

## Existing seam and baseline evidence

The implementation must reuse these existing surfaces and semantics:

- `kontor_container_recovery_preview` and
  `kontor_container_recovery_apply`, including Admin authorization and
  `prepare_container_recovery`;
- `prepare_container` and Paseo NativeChild placement, which already derive
  exact parent, canonical path, and rendered title, create on zero candidates,
  and adopt one exact candidate after a lost acknowledgement;
- the current exact-census regressions
  `stale_container_recovery_requires_one_exact_parent_path_and_title_candidate`,
  `stale_container_recovery_refuses_a_still_live_old_identity`, and
  `stale_container_recovery_refuses_zero_multiple_and_wrong_title_candidates`;
- store CAS/history through `recover_topology_container_with_intent`, including
  full binding comparison, append-only `topology_container_recoveries`, and
  unchanged replay behavior proven by
  `a_stale_container_recovery_cas_preserves_logical_identity_and_history`.

The zero-candidate baseline refusal changes only for the applicable case below.
All other existing adoption and refusal behavior stays intact.

## Preview contract

Preview is write-free. It freshly reads the project and exact logical binding,
attests that the subject is a persisted `NativeChild`, derives the canonical
placement tuple, and performs an exact native census under the exact persisted
parent.

Preview authorizes recreation only when all of these are simultaneously true:

1. the exact persisted native identity is absent;
2. the exact persisted parent is live and unchanged;
3. the derived canonical cwd, parent, and current rendered visible title equal
   the persisted/current authoritative values;
4. the exact-parent/canonical-path census contains zero candidates; and
5. the project revision and complete logical binding snapshot are captured for
   apply CAS.

The preview result must distinguish `recreate_absent` from the existing
single-candidate adoption disposition and bind its preview hash to the project,
topology node, complete before-binding, canonical placement tuple, zero-result
census, and intended operation. A caller-proposed native id, path, parent, or
title is never authoritative.

## Apply, CAS, idempotency, and lost acknowledgement

Apply accepts the existing operation's preview proof and mandatory idempotency
key. It proceeds in this order:

1. Resolve a durable same-key result before mutable-state preconditions. The
   same key and same canonical intent returns the original receipt and exact
   before/after evidence with `applied: unchanged` and performs no runtime
   effect. The same key with changed intent is an idempotency conflict.
2. Revalidate Admin authority, project revision, the entire logical binding,
   subject kind, canonical cwd, exact parent, rendered visible title, old-native
   absence, and current exact census against the preview.
3. When the fresh census is still zero, issue one native create using the exact
   derived parent/path/title. No loop, fallback path, alternate parent, title
   rewrite, or second create is permitted.
4. Freshly read back the exact parent/path census. It must contain exactly one
   candidate with the exact title. That native is the sole replacement eligible
   for binding.
5. Commit through the existing project/binding CAS and append durable recovery
   history plus the command result/receipt. Preserve the topology node and
   logical binding identity; change only its native identity and binding
   revision/evidence required by the existing recovery model.

If creation succeeds but its acknowledgement is lost before the durable CAS,
retry must perform the fresh census first, find exactly one exact
parent/path/title candidate, adopt that candidate, and complete the same CAS.
It must not issue another create. If the durable command result was committed
but its response was lost, the idempotency replay in step 1 returns the stored
result without a runtime call. Together, those two paths prove at-most-one
replacement creation across lost acknowledgement and replay.

Durable proof must retain the idempotency key, canonical intent hash, preview
binding, exact before/after binding evidence, selected replacement native,
project/binding revisions, append-only recovery history identity, and stable
receipt identity. Restarted replay must return that same evidence.

## Fail-closed refusals

Before create, bind, or durable state change, refuse:

- **stale native present:** the exact persisted native is live;
- **candidate ambiguity:** more than zero candidates at recreation preview, or
  more than one at lost-ack/readback adoption;
- **path drift:** any candidate or recomputed placement differs from the exact
  canonical cwd/path;
- **title drift:** a candidate at the path has any title other than the current
  rendered visible title, or the authoritative title changes after preview;
- **parent drift:** the expected parent is absent, changed, or a candidate is
  found under another parent;
- **binding/project drift:** any project revision or full logical binding field
  differs from the preview/CAS expectation;
- **subject mismatch:** every subject kind other than a persisted
  `NativeChild`, including `NativeRoot` and `Virtual`;
- **preview mismatch or reuse:** missing, stale, altered, cross-project,
  cross-node, or wrong-operation preview evidence; and
- **post-create ambiguity:** readback is zero, multiple, wrong-title,
  wrong-path, or wrong-parent. This records no binding change and never emits a
  second create in the same attempt.

A refusal cannot alter topology, binding, runtime/native state, task, TeamRun,
AgentRun, seat, workflow, or history. If a runtime create has already succeeded
but storage cannot safely bind it, the next retry is constrained to exact
lost-ack adoption; it cannot create again.

## PUB-07 continuation

Successful container recreation repairs only the container prerequisite for
the existing PUB-07 flow. The existing `kontor_seat_replace` route must then
bind the already-preserved PUB-07 successor AgentRun
`01a0ba00-4f76-7781-8f30-87df14681521` to the repaired container.

No new successor may be created. The replacement call must reuse the existing
successor/idempotent succession record; container recovery itself must not call
seat creation, seat replacement, or succession logic.

## ASMA-8188 operation-specific applicability

ASMA-8188 is the confirmed Kontor epic
`01a0aaa3-64a2-7de1-ab2d-8f40bad733ad`, “Kontor gate-rejection role-slot fence
recovery”; its landed child ASMA-8189 is commit `47024af8`.

Applicability is enforced directly at the existing API/runtime/daemon
container-recovery surface. Do not add a generic kontor-core applicability
predicate, operation registry, or future call-site abstraction.

| Existing operation | Subject/class and state | Recreation applicability | Required direct proof |
| --- | --- | --- | --- |
| Admin `kontor_container_recovery_preview/apply` | Persisted `NativeChild`; exact old native absent; exact parent/path census zero; placement unchanged | Applicable | API authorization plus daemon/runtime test: preview writes nothing; apply emits exactly one create, preserves the identity tuple, and records CAS/history/receipt |
| Admin `kontor_container_recovery_preview/apply` | Persisted `NativeChild`; one exact candidate | Inapplicable to creation; existing adoption applies | Runtime test: adopts the one exact candidate and create counter remains zero |
| Admin `kontor_container_recovery_preview/apply` | `NativeRoot`, `Virtual`, or any non-`NativeChild` subject | Inapplicable and refused | Direct API/daemon/runtime negatives: stable refusal before create or store write |
| `kontor_gate_rejection_recover` | Gate-rejection workflow/task subject | Inapplicable | Direct API/daemon regression: recovery retains its own authorization, CAS, idempotency, and replay semantics while the native/container prerequisite is absent; recreation path is never called and create counter remains zero |
| `kontor_gate_record` retired-evaluator/evaluator recovery | Gate evaluation with the existing recovery AgentRun/session evidence | Inapplicable | Direct API/daemon regression: evaluator recovery succeeds or returns only its own typed refusal without evaluating a NativeChild precondition; recreation path is never called and create counter remains zero |

The last two negatives are symmetric: those operations must not acquire a
generic native recreation capability, and they must not be blocked merely
because the container-recreation NativeChild precondition is inapplicable to
their subjects.

## Required implementation evidence

The implement turn must add the smallest focused extensions to the current
tests and record exact commands/results:

1. Preview/apply success from exact-native-absent plus zero candidates, with
   one create and exact topology/binding/cwd/parent/title preservation.
2. Refusals for stale-native-present, zero-to-multiple race, wrong path, wrong
   title, changed/missing parent, stale project revision, full-binding drift,
   and every non-NativeChild class, each proving no forbidden write/create.
3. Lost-create-ack adoption proving a retry binds the single exact candidate
   and create count remains one.
4. Same-key replay before and after restart proving the same receipt/evidence
   and no second native call; changed-intent key reuse must fail.
5. Store proof that project/full-binding CAS preserves logical identity and
   append-only recovery history.
6. The direct ASMA-8188 applicable/inapplicable matrix above, exercised through
   the existing API/daemon/runtime surfaces rather than a standalone generic
   predicate test.
7. Existing single-candidate recovery, stale-native refusal, ambiguous/wrong
   title refusals, gate-rejection recovery, evaluator recovery, and
   `kontor_seat_replace` replay regressions remain green.

## Explicit non-scope

- No live native, workspace, worktree, topology node, task, TeamRun, AgentRun,
  seat, or successor creation occurs in this scope turn.
- No new container-recovery endpoint or generic recreation entry point.
- No caller-selected path, parent, title, logical identity, or replacement
  native identity.
- No relaxation of Admin authorization, preview proof, project/full-binding
  CAS, exact census, idempotency, or append-only history.
- No generic applicability predicate for gate/evaluator operations.
- No PUB-07 successor other than
  `01a0ba00-4f76-7781-8f30-87df14681521`.

## Technical-design disagreement for release LSA disposition

The shared implement work was already active when this scope record was
written. Its observed work-in-progress includes changes named
`container_recreation`, a `RecreateTopologyContainer` command kind, and runtime
recreation methods. The checkpoint requires direct restriction at the existing
Admin container-recovery surfaces and forbids a generic applicability
abstraction. Those facts create a review question, not a scope-seat approval or
rejection of the implementation.

The release LSA must disposition the work by inspecting its actual reachability:

- conforming: any new internal type or command evidence is private to the
  existing Admin preview/apply flow, cannot be invoked by gate-rejection or
  evaluator recovery, and passes the direct matrix above; or
- nonconforming: it classifies arbitrary operation/subject pairs, exposes a
  reusable recreation route, or makes unrelated recovery paths depend on the
  NativeChild precondition. In that case the implementer must narrow it before
  acceptance.

The scope seat does not alter the shared production work to resolve this review
question.

## Shared-work preservation incident

The scope seat initially misattributed shared uncommitted SWE work to itself and
performed a narrow revert before the watchdog correction:

- removed the then-added `pub mod container_recreation;` line from
  `crates/kontor-core/src/lib.rs`;
- removed the then-added `RecreateTopologyContainer` receipt variant and its
  project-target match arm from `crates/kontor-core/src/receipt.rs`; and
- deleted the then-untracked
  `crates/kontor-core/src/container_recreation.rs`.

No reset or checkout was run. No runtime file was reverted. After the
correction, the active SWE implementer restored/reintroduced those core changes
and continued its other shared edits; the scope seat has not touched any
production file since. This incident and the design question above require LSA
awareness but do not decide the implementation's acceptance.

## Open questions

None. OQ-8239-01 is resolved by the confirmed ASMA-8188 epic/ASMA-8189 commit
and the explicit direct applicability matrix. OQ-8239-02 is resolved because
this checkpoint is the accepted contract under standing authority revision
`01a0b9a6-6a23-7042-a780-1430cd021032`; no separate PUB-09 design artifact is
required. The obsolete `OPEN-QUESTIONS.md` is removed.

## Handoff

The existing implement AgentRun
`01a0bbc3-1184-7ce0-9204-452107d52371` continues in the existing TeamRun and
worktree. It must treat this record as the frozen scope, preserve the current
shared dirty work for LSA review, implement and prove only the bounded contract
above, and hand the result to the existing release LSA. It must not create a
replacement topology, workspace, worktree, native outside the apply contract,
TeamRun, or PUB-07 successor.
