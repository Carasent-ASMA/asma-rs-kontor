# ASMA-8110 high-change record: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-change-record`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-change`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Status: implementation complete, verifier handoff below

## What was built

Three things, against the contract in
[`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md).

**1. Every rejected verdict now records *why* its workflow moved.** PR #186's
transaction already routed a rejection atomically with its verdict; it left no
durable evidence of the route. Migration `0089` adds
`task_gate_rejection_routes`, one append-only row per rejected evaluation, and
`append_gate_evaluation_with_intent` writes it inside the same transaction as
the verdict and the exact receipt. PR #186 remains the sole implementation of
ordinary rejected-verdict routing; nothing in its logic was rewritten.

**2. One bounded, admin-only command routes a rejection recorded before that
existed.** `RecoverGateRejection` is a distinct command kind. It appends no
evaluation — it consumes one that is already durable — and every argument it
takes is an expectation about state, never an instruction. The phase it routes
to is the pinned target from the frozen profile.

**3. The route row is also the freshness fence.** While the active workflow
sits at a route's `rejection_target`, `advance_workflow_from_evidence` returns
the current workflow unless a role turn settled strictly after `routed_at`, on
the task's preserved TeamRun, carrying every artifact that phase requires. The
artifacts that existed when the rejection was recorded are precisely the ones
the reviewer rejected, so without this the recovered workflow would walk itself
straight back out of the phase it was returned to.

## Production surfaces changed

Exactly the seven the scope names, and no others.

| File | Change |
|---|---|
| `crates/kontor-core/src/receipt.rs` | `RecoverGateRejection` kind; witnesses a `Task`, carries no desired state |
| `crates/kontor-core/src/repository.rs` | `GateRouteOrigin`, `GateRejectionRoute`, `GateRejectionRecovery` |
| `crates/kontor-store/migrations/0089_gate_rejection_routes.sql` | route table, its two append-only triggers, widened command-kind vocabulary |
| `crates/kontor-store/src/repository.rs` | route written inside the verdict transaction; whole recovery transaction; route/fence readbacks |
| `crates/kontor-api/src/applications.rs` | `RecoverGateRejectionRequest`, `GateRejectionRecoveryDto`, admin route, trait method |
| `crates/kontor-daemon/src/applications.rs` | recovery service; `rejection_fence_holds` in the evidence-advance path |
| `crates/kontor-mcp/src/registry.rs` | `kontor_gate_rejection_recover`, admin tier |

`crates/kontor-api/src/lib.rs` and `openapi.rs` carry the route and schema
registrations that the scope's own parity contract requires; they are
registration lines for the owned handler, not new behaviour.

The generated CLI spelling is exactly the one the scope documents:

```text
kontor gate-rejection-recover --project-id … --task-id … --gate-id … \
  --rejection-receipt-id … --sequence … --expected-task-revision … \
  --expected-workflow-revision … --expected-current-phase … \
  --expected-rejection-target … --idempotency-key …
```

## Decisions taken inside the contract

**The `from_phase <> rejection_target` check is scoped to recovered routes.**
A recovery proves the gate's phase *is* the current phase before it writes, so a
degenerate recovered route is impossible by construction. The ordinary path is
deliberately not held to it: a rejection may be recorded while the stored phase
already sits at the target — the verdict path does not require a gate's phase to
be current for a rejection — and refusing that would turn an existing permitted
recording into a hard failure. Such a route is recorded honestly instead.

**The already-consumed refusal names the receipt via the diagnostic's `at`.**
The scope requires the refusal to name the original recovery receipt.
`ApiError` has no free-text detail field, and `crates/kontor-api/src/error.rs`
is not an owned surface, so the receipt is named as the resource address
`command-receipts/{id}` in the diagnostic that exists for "something a caller
can go and look at". No unowned file was edited.

**The receiptless `append_gate_evaluation` trait method routes without
recording a route.** A route names the command receipt that caused it, and that
entry point has no receipt to name. Writing one with a fabricated id would put
an unattributable row in the one ledger whose whole value is that every row is
attributable. Production never reaches it for a verdict; it is used only by
store tests. Noted as a residual risk below.

## Open questions

### OQ-8110-01 — the fence's qualifying role is named inconsistently by scope

Subject: which role's settled turn releases the post-rejection freshness fence.
Attaches to: `ASMA-8110`, and specifically
[`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md) §4.

§4 requires the releasing turn to be "from the pinned handoff role that owns the
edge out of the rejection target". In the frozen `code@1` profile used by the
regression suite the rejection target is `implementation`, and in `docs@1` — the
ASMA-8100 shape — it is `authoring`. In both, the edge *out of* the target hands
work to the **inspector**: the reviewer whose rejection raised the fence. Read
literally, §4 would let the rejecting reviewer's own next turn release it.

That contradicts three statements in the same contract: §4's own "a
reviewer/auditor turn … must not release the fence", mutation case
MUT-8110-09, and qualification step 10, which releases the fence by handing
"the preserved **authoring** seat a new bounded turn".

The contradiction is not a reading error. `handoff_role` on `from -> to` names
the role work is handed **to** — `kontor-profiles/src/pack.rs` refuses one that
is "a role the pinned team supplies no slot for", because that role must pick
the work up. The role that *authors* a phase is therefore named by the edge
leading **into** it.

Options seen: (1) the edge **into** the target, naming the author; (2) the edge
**out of** it verbatim, naming the reviewer; (3) no role condition, fencing on
freshness, run and required artifacts alone.

Chosen, pending scope's ruling: **option 1**, degrading to option 3 where the
profile names nobody (a rejection target that is the entry phase has no inbound
edge). It is the only reading under which every acceptance criterion and all ten
mutation cases hold at once. In both seeded profiles the required artifacts do
the discriminating work regardless: an inspector turn carries `review-notes`,
not the target phase's `code-change`/`draft`.

The choice is one expression in
`crates/kontor-daemon/src/applications.rs::rejection_fence_holds` and is a
one-line change if scope rules otherwise. No stored data depends on it: the
route row records no role.

### OQ-8110-02 — "the preserved active TeamRun" cannot mean a live one

Subject: the TeamRun condition on the qualifying turn.
Attaches to: `ASMA-8110`, `HIGH-SCOPE-RECORD.md` §4.

§4 requires the releasing turn to belong to "the preserved active TeamRun".
Implemented literally as a non-terminal lifecycle, the fence never releases: a
team whose seats have all settled closes as `succeeded` while its seats stay
persistent and reusable, and that is exactly the state a recovered rejection is
found in and a new bounded turn is handed into. This was observed, not
predicted — the first run of
`legacy_artifacts_do_not_advance_a_recovered_rejection_until_a_fresh_authoring_turn_settles`
failed on it with the team run at `succeeded`.

Implemented as **identity**: the turn must belong to the task's current
(preserved) TeamRun, whatever its lifecycle. This still excludes a turn from a
different or superseded TeamRun, which is what the clause is for. If scope
intended a liveness requirement, the recovery cannot be released by any turn and
qualification step 10 is unreachable.

## Commits

Branch `feat/ASMA-8110-gate-verdict-recovery`, rebased onto `origin/master`
`98940604d9a554aa24e0d87474bf0807232499e5` before any source change. PR #186's
merge `b2b5cad6e920fca7e5282873faef45da61e7328c` is an ancestor and its logic is
untouched.

| Commit | What |
|---|---|
| `1263d88` | the scope record, preserved (rebased, unedited) |
| `1cb86df` | migration, command kind, typed records, store transactions, API/MCP/daemon surfaces, fence |
| `c0cbb09` | the behaviour tests the scope names |
| `6bc5efe` | source-uniqueness coverage |
| `8f54fcb` | the three coverage gaps mutation testing exposed |
| `c78f7d6` | fmt and clippy |
| `0397318` | regenerated OpenAPI contract and console types |

## Mutation results

All ten cases killed. Each was injected, the named killer was run and required
to fail, then the defect was reverted and the killer required to pass.

| ID | Defect | Killer | Result |
|---|---|---|---|
| MUT-8110-01 | verdict and route commit before the receipt is written | `a_gate_rejection_route_and_exact_receipt_commit_or_roll_back_together` | killed — census showed rows surviving a refused receipt |
| MUT-8110-02 | route to the current phase instead of the pinned target | `a_phase_advancing_gate_replays_after_revision_change_and_restart` | killed |
| MUT-8110-03 | drop the source-receipt uniqueness and its pre-check | `a_historical_rejection_recovery_is_source_unique_append_only_and_survives_reopen` | killed |
| MUT-8110-04a | replay increments the workflow revision (store) | same store test | killed |
| MUT-8110-04b | replay re-routes instead of answering (daemon) | `a_historical_gate_rejection_recovery_routes_once_and_replays_after_restart` | killed |
| MUT-8110-05 | substitute the gate's latest evaluation for the requested one | source-unique store test | killed |
| MUT-8110-06 | ignore both expected revisions | `gate_rejection_recovery_refuses_…_without_writes` | killed |
| MUT-8110-07 | ignore the phase and target comparisons | same refusal census | killed |
| MUT-8110-08 | pre-route artifacts satisfy freshness | `legacy_artifacts_do_not_advance_…` | killed |
| MUT-8110-09 | any turn releases the fence | same fence test | killed |
| MUT-8110-10 | route state does not survive a reopen | source-unique store test | killed |

MUT-8110-04 is recorded as two cases because the daemon answers a replay before
the store is reached, so one injection cannot exercise both paths.

### Three cases initially survived, and each exposed a real coverage gap

These are the findings of the exercise, not incidental test churn.

**MUT-8110-03 survived first.** The source-uniqueness case cited the spent
receipt under a second sequence while the workflow sat at the rejection target,
so the *gate-phase* precondition refused it before uniqueness was ever
consulted. The test proved a refusal that did not depend on the constraint the
mutation removed. Fixed by reproducing the shape that actually occurs — rework,
re-review, a second rejection, the workflow back at the gate's phase — so every
other precondition passes and only the receipt's uniqueness stands.

**MUT-8110-04 survived first.** Two gaps at once: no assertion held the workflow
revision still across a replay, and the loopback test could not reach the store's
replay path because the daemon short-circuits first. Fixed by asserting the
revision after the store replay and by injecting the defect separately in each
path.

**MUT-8110-05 survived first.** Substituting the gate's latest evaluation was
invisible because only one evaluation existed, and where a second did exist the
receipt's exact result binding caught the mismatch anyway. Fixed by seeding both
rejected evaluations before the recovery and asserting the route names the
requested sequence — the legacy-receipt path, which has no binding to fall back
on, is exactly where the substitution would do damage.

### One implementation defect was found by a test, not by review

`legacy_artifacts_do_not_advance_a_recovered_rejection_until_a_fresh_authoring_turn_settles`
failed on its first run: the fence never released. The cause is recorded above
as OQ-8110-02 — a team whose seats have all settled closes as `succeeded`, so a
liveness reading of "the preserved active TeamRun" fences the workflow forever.
Corrected to an identity comparison.

## Residual risks

| Risk | State |
|---|---|
| OQ-8110-01 — the fence's qualifying role | Open. Chosen reading is one expression; no stored data depends on it |
| OQ-8110-02 — "preserved active TeamRun" | Open. Implemented as identity; a liveness reading makes qualification step 10 unreachable |
| The receiptless `append_gate_evaluation` routes without recording a route | Accepted. Test-only entry point; a rejection through it leaves the fence unarmed, degrading to pre-ASMA-8110 behaviour rather than to anything worse |
| `reconcile()` does not itself call the evidence advance | Observed during MUT-8110-08. The fence test's reconcile assertions are therefore weaker than they look; the settlement assertions carry the proof, and those do kill both fence mutations |
| A rejection recorded while the workflow already sits at the target | Recorded honestly as a route whose `from_phase` equals its target. Only the recorded path can produce one; recovery cannot |

