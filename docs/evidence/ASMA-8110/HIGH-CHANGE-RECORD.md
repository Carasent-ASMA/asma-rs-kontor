# ASMA-8110 high-change record: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-change`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-implementation`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Status: second remediation, after the re-verification rejection of `5743d7a`

## This turn: repair of the re-verification rejection

The first remediation merged to master as
[`bf71bc2`](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/199) (PR #199).
Independent re-verification then rejected candidate `5743d7a` in
[`HIGH-VERIFICATION-REPORT.md`](HIGH-VERIFICATION-REPORT.md) (local `66b34fe`,
carried onto this branch). This section records the repair of those findings;
everything below it describes the first remediation and is retained unchanged.

The repair branches from newest `origin/master` (`bf71bc2`), which already
contains the merged ASMA-8110 code, and preserves the task, TeamRun, AgentRun,
seat, native-session, workspace and `cwd` identities untouched.

### F-8110-R2 — a present `null` binding no longer falls back to legacy

`bound_gate_record_result` returned `None` when `result` was absent **or null**.
A present null is a malformed claim about what the recording transaction wrote,
not the absence of a claim, so it reached the legacy comparison and let the
caller's own `(gate, sequence)` stand in for the receipt's. Only an absent
member now returns `None`; every present `result` is parsed strictly.

Regression: the `NullResult` case in
`gate_rejection_recovery_refuses_invalid_exact_bindings_but_accepts_an_absent_legacy_binding`,
beside the hash, partial-object, mismatched-binding and absent-member cases.
Verified non-vacuous by reinstating the old predicate: the case fails with the
endpoint returning 200/`created`, exactly as the verifier observed.

### F-8110-R3 — an invalid source receipt keeps its own refusal

The post-transaction decoration ran on **every** store error, so a receipt
belonging to another task was rewritten as `409 already routed` and disclosed a
route receipt the caller had no right to. Decoration is now gated on the
repository's own route-uniqueness conflict
(`RepositoryError::Conflict { subject: "gate rejection route", .. }`); every
other refusal is returned exactly as the store produced it, which means source
validation runs to completion before any existing route can be named.

Regression:
`a_post_route_invalid_source_keeps_its_refusal_and_discloses_no_route_receipt`
drives three post-route probes — a receipt targeting another task, one naming
another gate, one that recorded no verdict — and asserts each is refused, names
no route resource in `diagnostic.at`, does not contain the winning receipt
anywhere in the body, and writes nothing.

### F-8110-R4 — the concurrency proof is deterministic, and corrects the model

`concurrent_recovery_keys_both_observe_no_route_before_either_commits`
(store-level, which the report explicitly permits) performs the service's route
pre-check for **both** keys and asserts both observe no route *before* either
transaction opens. That is the barrier, established by ordering rather than by
hoping a race lands.

It also corrects the report's model of the race. A same-source loser does **not**
reach the route-uniqueness check: both requests were built against workflow
revision 1, the winner moves it to 2, and the loser's compare-and-swap fails
first with a revision conflict naming the revision to re-read. That is a
stronger stop, not a gap. The test pins that refusal exactly, then separately
drives a caller whose expectations are current to the route-uniqueness conflict
— the only error the service is allowed to decorate — and pins that too.

### F-8110-R5 — the scope now names the deployed generation

`HIGH-SCOPE-RECORD.md` still required migration 0089 and schema 89 in three
places, including the protected delivery readback, while the candidate and the
deployed realm are at 90. All three now say 0090/90, and a new
"Schema generation" section records why the number moved so a later reader does
not correct it back.

### F-8110-R1 — the lockfile

Regenerated against current registry resolution (`libflate`, `ureq`,
`ureq-proto` and their dependents) and verified idempotent: a second
`cargo generate-lockfile` is byte-identical to the committed file.

**Residual risk, unchanged in kind:** this only holds while upstream resolution
does not move again. The gate compares a *freshly resolved* lock to a committed
one, so any registry publication between commit and verification re-opens
F-8110-R1 for reasons no candidate controls. That is a property of the gate, not
of this work, and it is the second time it has rejected a candidate.

### New: a released rejection stays in verification until a fresh verdict

The live database showed only rejection sequence 1, yet the workflow was
observed back at the rejection target after a fresh implementation turn had
settled. `a_released_rejection_stays_in_verification_until_a_fresh_gate_verdict`
locks the intended behaviour: after the rework releases the fence the workflow
lands in the gate's own phase, and neither reconciliation, nor a reviewer's
turn, nor anything short of a **new** gate verdict moves it. It also asserts the
rejection stays consumed exactly once and that no verdict is invented while the
workflow waits.

Current code passes it, so the live observation is explained by the recovery
legitimately routing at revision 5→6 rather than by a re-consumption defect.
Verified non-vacuous: making a rejected gate count as satisfying its phase
advances the workflow straight past verification and the test fails.

### Out of scope by instruction

The inherited publication MCP vocabulary baseline
(`no_tool_names_a_store_a_database_or_a_migration` flagging three ASMA-8101
tools whose argument is named `repository`) is **ASMA-8115's**. It is untouched
here and still present on master.

## What this remediation is

The rejected candidate supplied the recovery surface. This turn does not
rewrite it. It applies the corrective delta the amended
[`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md) authorizes, against the findings
in [`HIGH-VERIFICATION-REPORT.md`](HIGH-VERIFICATION-REPORT.md).

Those three commits were rewritten by the rebase onto current `origin/master`,
so each is recorded under both identities. The pre-rebase SHAs are the ones the
handoff named; the post-rebase SHAs are the ones reachable from this branch.

| Document | Pre-rebase (handoff) | On this branch |
|---|---|---|
| Rejected candidate's high-change record | `d59e4eb3103c7e21947a288e726f7015c47f6955` | `4575d0c` |
| High verification report (rejection) | `3800cb27654c16dc075b4edf8955fd6b34b404d1` | `577918f` |
| Amended high scope record | `aa5f17a41ca64e9b4e8b3b3eb5a8a5fbad24e20d` | `a79eca1` |

Their content is identical under both; only the commit identity moved.

Every existing task, TeamRun, AgentRun, seat, native-session, workspace and
`cwd` identity is untouched. The branch and worktree are the ones the handoff
named. No deployment, no merge, no Jira mutation, no recovery invocation.

## Integration with current master

Master moved from `9894060` to `e4bb5fbd59a9194e06f22c52f84f95d283c7f5a5`
during the rejection cycle. The branch was rebased onto it. Three collisions
mattered:

- **Master claimed migration slot 0089** for ASMA-8101 publication
  attestations. This work renumbered to
  `0090_gate_rejection_routes.sql` and `SCHEMA_VERSION` is now **90**. Master's
  0089 does not touch `command_receipts`, and master added no command kind, so
  the widened kind vocabulary in 0090 is still exactly the v84 list plus
  `recover_gate_rejection`.
- **`mcp_parity` counts** moved by four on master; this work adds one tool, so
  the canaries are 171/170/172.
- **`schema_v1`** pins both the version and the exact table set; both now carry
  v90 and `task_gate_rejection_routes` beside master's
  `publication_attestations`.

`crates/kontor-api/contract/openapi.json` and the console's generated types
merged cleanly and re-verify against what the crate serves.

The ESW/ECP/TSW scheduler admission and naming defect is **not** implemented
here. It is routed separately through ASMA-8101/PUB-01 to avoid file and runtime
overlap, per the handoff.

## Corrections, finding by finding

### F-8110-01 — the fence's TeamRun is now route-time identity

`team_run_id` is an immutable column on `task_gate_rejection_routes`, with a
composite foreign key to `team_runs (project_id, id)`. Both write paths resolve
it once — inside the transaction that writes the route — using the repository's
canonical `(created_at, id)` order, and freeze it on the row. A task with no
TeamRun refuses rather than writing a fence nothing could release.

`rejection_fence_holds` reads `route.team_run_id` and no longer calls
`list_team_runs_for_task(...).last()` or anything like it. Lifecycle is
deliberately not a precondition: a team whose seats have all settled closes as
`succeeded` while its seats stay reusable, which is the state a recovered
rejection is found in.

This is the scope's disposition of **OQ-8110-02**. The authoring role is the one
named by the edge leading **into** the rejection target, which is the scope's
disposition of **OQ-8110-01** and matches what the rejected candidate already
implemented.

### F-8110-02 — the lockfile is the registry-resolved one

`cargo generate-lockfile` with registry access moved `ipnet` 2.12.1 → 2.12.2 and
changed nothing else. That resolution is committed. No manifest or dependency
change was made or authorized.

### F-8110-03 — invalid exact bindings fail closed

`bound_gate_record_result` now returns `None` for one thing only: there is no
exact binding to check, meaning no stored payload or a payload with no `result`
object. That is the genuine legacy shape and still takes the intent/evaluation
comparison path.

Anything else is an error. The stored digest is verified *before* the content is
read, so a payload that fails its own hash is never asked whether it has a
result; and a `result` that is present but partial, intent-mismatched or
unreadable propagates rather than silently becoming "legacy". A corrupted
binding can no longer authorize a caller-selected sequence.

### F-8110-04 — the concurrent loser still names the winner

A route is unique under two identities: the source receipt it consumed and the
evaluation it belongs to. The service's pre-check only knows the first. On any
transactional refusal the service now re-reads **both** and, if a durable route
exists, returns the refusal that names the winning receipt in
`diagnostic.at` as `command-receipts/{id}`. One construction of that refusal is
shared by the pre-check and the post-transaction path, so a concurrent loser
cannot get a thinner answer than a sequential one.

### F-8110-05 — the negative matrix is complete

The named refusal test is table-driven over: a nonexistent task, **an existing
wrong task in the same project**, an undeclared gate, a missing/wrong sequence,
a receipt that recorded no verdict, a receipt that exists nowhere, **a receipt
targeting another task**, **a receipt whose canonical intent names another
gate**, **a non-rejected evaluation**, a wrong phase, a wrong target, a stale
task revision and a stale workflow revision — each with a before/after database
and workflow census. **A terminal task** and **an inactive workflow** are
handled as separate state-shaped cases, each set up, attempted and restored
around its own census.

Added tests: `gate_rejection_recovery_refuses_same_key_with_changed_intent_without_writes`,
`gate_rejection_recovery_refuses_invalid_exact_bindings_but_accepts_an_absent_legacy_binding`,
`concurrent_fresh_recovery_keys_name_the_single_original_route_receipt`, and
`a_later_same_task_team_run_cannot_release_an_earlier_rejection_fence`. The
fence test additionally covers a qualifying turn on **another task** and asserts
the releasing turn belonged to the route's own TeamRun.

## ASMA-8100 is untouched, and its request is proved refused

Per the amended scope's **retired live recovery** disposition, ASMA-8100 was not
invoked, reopened, rewound or mutated in any way. Nothing in this turn read or
wrote that task.

Its two live shapes — terminal, and long past the workflow revision its frozen
recovery was written against — are proved refused by the automated no-write
census rather than by touching it: the `a terminal task` and `a stale workflow
revision` / `a stale task revision` rows of
`gate_rejection_recovery_refuses_wrong_task_gate_sequence_receipt_phase_target_and_revisions_without_writes`,
each with an unchanged before/after census. That is the scope's step 8: even a
refused command against the live task could create command-attempt evidence, so
the refusal is demonstrated in the exported tree instead.

**OQ-8110-03 is resolved by the amended scope** as option 1 — the live recovery
objective is superseded; generic historical recovery remains release-blocking
and is what these tests qualify.

## A defect this remediation introduced and then removed

Resolving the route-time TeamRun inside the *shared* evaluation append gave the
receiptless `append_gate_evaluation` contract method a precondition it has no
use for. That path deliberately records no route — a route names the command
receipt that caused it, and that entry point has none — so a task with no
TeamRun could no longer record a rejection through it at all.

Two long-standing store tests failed on it in the authoritative gate:
`no_gate_verdict_can_turn_its_citations_into_producer_evidence` and
`the_gate_state_map_reduces_the_whole_append_only_history`. The correction is to
resolve the TeamRun in the transaction that actually writes the route, which is
still "inside the route transaction" as the scope requires, and leaves the
receiptless path exactly as it was. Fixing the two fixtures instead would have
hidden a real compatibility regression behind test setup.

## Remediation commits

| Commit | What |
|---|---|
| `df3992e` | route-time TeamRun on the route and the fence; invalid bindings fail closed; concurrent conflict decorated; migration renumbered to v90 |
| `f660eaf` | the complete negative matrix and the two-TeamRun regression |
| `f86a3f1` | the registry-resolved `Cargo.lock` |
| `d92c622` | name the winner for either of the route's two unique identities |
| `5743d7a` | require a TeamRun only where a route is written |

Everything before them on the branch is the rebased rejected candidate plus the
scope and verification documents.

## Mutation results

All fifteen cases killed: each injected, its named killer required to fail, the
defect reverted, and the killer required to pass again. MUT-8110-04 is recorded
as two cases because the service answers a replay before the store is reached,
so one injection cannot exercise both paths.

| ID | Defect | Killer | Result |
|---|---|---|---|
| MUT-8110-01 | verdict and route commit before the receipt | store atomicity test | killed |
| MUT-8110-02 | route to the current phase, not the pinned target | PR #186 regression | killed |
| MUT-8110-03 | drop source-receipt uniqueness and its pre-check | source-unique store test | killed |
| MUT-8110-04a | replay increments the workflow revision (store) | source-unique store test | killed |
| MUT-8110-04b | replay re-routes instead of answering (service) | historical recovery loopback | killed |
| MUT-8110-05 | substitute the gate's latest evaluation | source-unique store test | killed |
| MUT-8110-06 | ignore both expected revisions | refusal census | killed |
| MUT-8110-07 | ignore the phase and target comparisons | refusal census | killed |
| MUT-8110-08 | pre-route artifacts satisfy freshness | fence test | killed |
| MUT-8110-09 | any turn releases the fence | fence test | killed |
| MUT-8110-10 | route state does not survive a reopen | reopen test | killed |
| MUT-8110-11 | recompute the latest TeamRun | two-TeamRun fence test | killed |
| MUT-8110-12 | invalid exact binding treated as legacy | invalid-binding test | killed |
| MUT-8110-13 | drop the winning receipt from the loser's diagnostic | concurrent test | killed |
| MUT-8110-14 | permit terminal recovery | refusal census | killed |
| MUT-8110-15 | restore the stale committed lockfile | online lock regeneration | killed |

### What mutation testing caught that review did not

MUT-8110-13 survived its first injection. The reason is worth recording: the
store takes one process-wide lock, so two HTTP recoveries serialize and the
loser is answered by the service's pre-check, never reaching the transactional
conflict the finding was about. The test proved the observable contract and not
the fix.

The case was rebuilt around the path that *is* reachable in-process — a second
source receipt citing an already-routed evaluation, which the pre-check cannot
see — and that immediately exposed a genuine gap: the decoration looked the
route up only by source receipt, while the collision is on the evaluation
identity. Both identities are now re-read.

That correction was then silently reverted by a `git checkout --` while
reverting a later mutation, and the same test caught it again on the next full
run. Both facts are recorded because they are the argument for the test, not
incidental churn.

## Verification

### Authoritative archive gate

`python3 scripts/verify-tree.py --mode archive` on `5743d7a`:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | **`Cargo.lock byte-compare: identical`** — F-8110-02 closed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2257 passed**, 1 failed — see the baseline classification below |
| audit / deny / pnpm / typecheck / Vitest / prod audit | not reached |

Every ASMA-8110 test passed inside that exported tree:

```text
a_phase_advancing_gate_replays_after_revision_change_and_restart ......................... ok
a_historical_gate_rejection_recovery_routes_once_and_replays_after_restart ............... ok
gate_rejection_recovery_refuses_wrong_task_gate_sequence_receipt_phase_target_and_revisions_without_writes ... ok
gate_rejection_recovery_refuses_same_key_with_changed_intent_without_writes .............. ok
gate_rejection_recovery_refuses_invalid_exact_bindings_but_accepts_an_absent_legacy_binding ... ok
concurrent_fresh_recovery_keys_name_the_single_original_route_receipt ................... ok
legacy_artifacts_do_not_advance_a_recovered_rejection_until_a_fresh_authoring_turn_settles ... ok
a_later_same_task_team_run_cannot_release_an_earlier_rejection_fence .................... ok
a_gate_rejection_route_and_exact_receipt_commit_or_roll_back_together ................... ok
a_historical_rejection_recovery_is_source_unique_append_only_and_survives_reopen ........ ok
the_gate_rejection_route_schema_migrates_and_survives_a_snapshot ........................ ok
```

### The one failure is a pre-existing master baseline defect, not this work

**Failing test:** `no_tool_names_a_store_a_database_or_a_migration`
(`tests/contract/mcp_mutants.rs:63`).

```text
direct persistence: the tool vocabulary names something it must not reach: [
    "kontor_publication_preview.repository contains `repository`",
    "kontor_publication_attest.repository contains `repository`",
    "kontor_publication_merge.repository contains `repository`",
]
```

**Classification: inherited from merged ASMA-8101, reproduced identically on
clean master.** The evidence, in order of strength:

1. A clean `git archive origin/master` (`57fd689232e33f8be21e47fe84e85d507ab25efb`)
   extracted to a scratch tree and run with
   `cargo test --locked -p kontor-tests-contract --test mcp_mutants no_tool_names_a_store`
   fails with **byte-identical diagnostics** and `BASELINE_EXIT=101`. The defect
   exists on master with none of this branch's commits present.
2. All three flagged tools — `kontor_publication_preview`,
   `kontor_publication_attest`, `kontor_publication_merge` — were introduced by
   ASMA-8101, already merged. None is an ASMA-8110 surface.
3. This branch adds **zero** occurrences of `repository` to
   `crates/kontor-mcp/src/registry.rs`. Its only registry change is the single
   `kontor_gate_rejection_recover` tool, and that tool is not flagged.
4. This branch does not modify `tests/contract/mcp_mutants.rs` at all.

The rule this trips is a vocabulary guard: an MCP tool argument may not name
storage. ASMA-8101's publication tools take a *source-control* repository, which
is a different sense of the word than the store/database/migration the guard is
protecting against — so the fix is a decision about that guard's wording or an
exemption for the publication sense, and it belongs to ASMA-8101, not here.
Changing either the guard or those three tools from this seat would edit a
surface the amended scope freezes.

**This is deliberately not fixed here** and is the residual issue reported to
root. It blocks a fully green archive run for any branch off current master,
including master itself.

## Residual issues

| Issue | State |
|---|---|
| `no_tool_names_a_store_a_database_or_a_migration` fails on master | Inherited, reproduced on clean master, owned by ASMA-8101. Blocks a fully green archive gate for every branch until fixed |
| The archive gate's later stages | Not reached, because the workspace test gate exits first on the inherited failure. Audit/deny/pnpm/typecheck/Vitest/prod-audit therefore carry no result for this candidate |
| ESW/ECP/TSW scheduler admission and naming defect | Out of scope by instruction; routed through ASMA-8101/PUB-01 |
| Receiptless `append_gate_evaluation` records no route | Unchanged and intended. A rejection through that path leaves the fence unarmed, which degrades to pre-ASMA-8110 behaviour. Production never reaches it for a verdict |

## Handoff

Freeze `5743d7a`. It is the remediation candidate; the evidence commit on top of
it changes only this document.

The ASMA-8110 delta is complete against the amended scope: F-8110-01 through
F-8110-05 are corrected, all fifteen mutations are killed, and the lockfile
byte-compares identical online. The candidate cannot show a fully green archive
gate until ASMA-8101's `repository` vocabulary finding is resolved on master,
because that failure precedes the gate's remaining stages and is reproducible
without any of this branch's commits.

No deployment, no merge, no push to master, no Jira mutation, no recovery
invocation, and no live-state change. ASMA-8100 remains untouched.

## Second-remediation integration handoff

The preceding `5743d7a` handoff is retained as the historical first-remediation
record. It is not the current candidate.

The second-remediation implementation and regression tests were integrated with
current master `508a514` and frozen at `438f9760eb9faa1b5a830b751a476c3c7a24e44d`.
That exact tree passed formatting, a locked all-target workspace check, and the
focused null-binding, invalid-source non-disclosure, fresh-verdict fencing, and
deterministic concurrent-recovery regressions. Its full locked workspace test
run had one failure: the known parallel-load
`a_concurrent_first_open_initializes_exactly_one_realm` transient, which passed
three consecutive isolated reruns; every ASMA-8110 test passed.

Independent verification then identified that the integrated master carries
schema migrations 0091/0092 after this work's immutable 0090 migration. The
scope's protected delivery readback was corrected to schema 92 in the
documentation-only follow-up `4fd5b18`. Therefore the final candidate for PR
#201 is **`4fd5b18`**: code tree `438f976` plus only that factual scope
correction.

The implementation seat reached its provider spend limit after completing the
tests but before it could commit the already-authored change record or push.
Root performed only that bounded integration remainder and recorded it as
`operational_gap`; all task, TeamRun, AgentRun, seat, native-session,
workspace, and `cwd` identities were preserved.

## Third-remediation verification-environment correction

Gate rejection sequence 2 was recorded after independent verification of
`438f976` found one load-sensitive concurrent-first-open failure and the frozen
handoff did not yet include the schema-92 documentation correction. The same
first-open test then passed eight consecutive exact reruns against `d49b151`;
the production migration path and its bounded timeout are unchanged.

A fresh authoritative archive run against `d49b151` passed lockfile
regeneration, formatting and clippy. The workspace test gate then exposed a
separate deterministic harness error in
`crates/kontor-cli/tests/memory_parity.rs`: a verification process launched by
an identity-bound Kontor seat legitimately carries `KONTOR_AUTH`, and the child
CLI deliberately prefers that seat-scoped operator credential. The fixture
intends to exercise the explicit disk credential belonging to its temporary
Realm, so inheriting the parent seat identity makes that Realm correctly answer
`unauthenticated`.

The fixture now removes `KONTOR_AUTH` from only the child CLI process. Production
credential precedence is not changed, the credential is neither read nor
logged, and the focused parity test passes while the parent verification seat
remains identity-bound.

The resumed archive then found the same unstated assumption in
`client::tests::a_credential_file_yields_exactly_the_tier_that_was_asked_for`.
`Credential::read` now delegates its unchanged on-disk branch to a private
`read_file` helper, and that disk-format unit calls the helper directly. The
identity-bound operator branch remains first in the public production method.
Both the exact unit and `memory_parity` pass with the parent seat credential
still present.

During the rejected verification turn, the registered native workspace and
worktree disappeared while the exact branch, Kontor task, TeamRun, AgentRuns,
SeatBindings and native sessions remained. Supported topology materialization
continued to refuse the stale binding after runtime settlement. Root therefore
restored the exact registered path from the existing branch with a bounded
`git worktree add`; no logical or runtime identity was replaced. Both the lost
workspace and the unsupported restorative remainder are `operational_gap`
evidence for closeout.

## Fourth-remediation delivery: bounded direct fallback (`operational_gap`)

Gate rejection sequence 2 (receipt `01a07741-cf6c-7330-9c89-0f8bb5979766`) put
ASMA-8110 at `high-implementation` revision 7. The correction lineage for that
rejection begins at local verification commit `66b34fe`, which is carried onto
this branch as `25265c8`.

Delivering that handoff to the preserved implementation seat through the
supported Kontor path failed closed. The exact fact pattern, recorded so the
closeout can judge the gap rather than infer it:

- after the provider quota reset, `kontor_session_timeline_get`,
  `kontor_runtime_settle`, `kontor_topology_materialize(ticket)`, and one fresh
  `kontor_session_message_send` **each returned `409 stale_binding` without
  delivery**;
- `kontor_seat_attention` succeeded and preserved the exact seat;
- a daemon restart attempted startup reconciliation, but the runtime refused the
  persisted session generation;
- root then used a bounded direct Paseo fallback to the **same** native agent.

Nothing was replaced or re-created. The preserved identities are native agent
`b600c003-b029-47fc-b5cc-ed120be11a1e`, AgentRun
`01a07398-d34a-7b31-a3a6-1547a37e71a6`, SeatBinding
`01a07398-b936-75b1-ae2c-121c255a7f75`, TeamRun
`01a07398-b8d2-7363-8dcc-e92c061deffa`, task
`01a07391-328e-74a3-a808-e7b5775c8438`, branch
`fix/ASMA-8110-gate-recovery-binding-integrity`, and the exact registered
worktree.

No Kontor receipt exists for the four refused deliveries, so none may be
recorded or inferred for them. No credential or authorization value was read,
logged, or disclosed in establishing any of the above.

Root additionally corrected two **test-boundary** credential assumptions so an
inherited identity-bound seat credential is not mistaken for a Realm or on-disk
fixture credential. Production credential precedence is unchanged; both
corrections are confined to test setup and a private read helper.

### Independent re-validation of the exact head

Performed by this seat on `069d321` after the fallback delivery, to confirm the
candidate rather than accept it on report:

| Check | Result |
|---|---|
| `cargo generate-lockfile` byte-compare | **identical** |
| `loopback_api rejection` (7 tests) | passed |
| `loopback_api team_run` (6 tests) | passed |
| `repository_roundtrip rejection` (2 tests) | passed |
| `repository_roundtrip concurrent_recovery` (1 test) | passed |
| `kontor-cli memory_parity` | passed |
| `a_credential_file_yields_exactly_the_tier_that_was_asked_for` | passed |

Every F-8110-R1…R5 correction and both new regressions are present in this tree
and were re-confirmed by inspection: the strict absent-member binding check, the
route-uniqueness-gated decoration, the deterministic pre-check barrier, the
post-route invalid-source regression, and the released-rejection regression. The
scope's protected readback now names schema 92 with migrations 0090, 0091 and
0092, matching the integrated tree.

This seat did not approve or advance the gate.
