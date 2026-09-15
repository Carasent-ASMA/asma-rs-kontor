# ASMA-8110 high-change record: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-change`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-implementation`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Status: Gate 12 archive-qualified remediation candidate after live-deployment finding R11; awaiting independent verification

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

## Verification (historical — first remediation, candidate `5743d7a`)

> Retained as the record of that turn. It is **not** this document's handoff;
> the current candidate and its archive result are in the final section.

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

## Handoff (historical — first remediation, candidate `5743d7a`)

> Superseded. `5743d7a` was rejected; see the final section for the candidate
> this document actually hands over.

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

## Fifth remediation (superseded): the archive gate reaches exit 0

This section is the handoff. Everything above it is the record of earlier turns
and is retained unchanged.

### Frozen candidate

| | |
|---|---|
| **Candidate SHA** | `47c8482e297474d58b4d1bf8ae635e7a5737e545` |
| **Tree SHA** | `7d1974593ecab31d7435c5f3482e26e14b96a518` |
| **Integrated master** | `f78d041e80042417e0d9a059449eb85737571797` (schema **96**) |
| **Archive exit** | **0** |
| **Archive log digest** | `b59ac10eec24a55b10e7dfdc539bd29f1280a231b18a8248baacad594824d3f8` |

The candidate is code-only. This document and `HIGH-SCOPE-RECORD.md` are
committed separately, on top of it, and change no source.

### What the authoritative gate established

`python3 scripts/verify-tree.py --mode archive`, run from a `git archive` export
of the candidate, with registry access:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | `Cargo.lock byte-compare: identical` |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2477 passed, 0 failed** |
| `cargo audit` | passed |
| `cargo deny check` | passed |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm -r typecheck` | passed |
| `pnpm -r test` | **300 passed** (16 files) |
| `pnpm audit --prod` | passed |

### Everything repaired here was inherited, and each was proved so

No ASMA-8110 behaviour was changed in this turn. The route/fence corrections
(F-8110-R2, R3) and the deterministic interleaving test are already in master.
What blocked the gate was breakage that arrived with master, and each item was
reproduced on a clean export of master before being touched:

| Blocker | Proof it was inherited | Repair |
|---|---|---|
| `JiraResponse` missing `observed_identity` — `cargo clippy --all-targets` fails E0063, stopping the gate before any test | clean `origin/master` fails identically (`MASTER_EXIT=101`) | the two e2e initializers get `observed_identity: None`, the value and comment ASMA-8116 used on its sibling fixture |
| Six Jira `503 unavailable` — "the configured native Jira connector could not answer" | all four sampled reproduce byte-identically on clean master | `DescriptionJira` reports the id `jira_issue_id` derives from its key — what a confirmed binding records — and answers by key or id |
| `reconcile_plan_refuses_to_call_a_placeholder_body_converged` — `placement_blocked`, then `stale_binding` | same | its declaratively applied link is now confirmed the way a materialization would; the placeholder-body assertion is untouched |
| `replaying_a_partial_admission_delivers_its_durable_follow_up` — `revision_conflict` | same | hardcoded `expected_task_revision: 1` now read from the store; the race guard is unchanged, the settlement still presents the revision it read |
| `an_empty_realm_is_bootstrapped_through_mcp_tools_alone` — "the epic has no active immutable backlog code" | clean master `f78d041` fails identically (`MCHK2_EXIT=101`) | fixture sends the supported top-level `epic_backlog_code` beside the runtime-facing execution scope. Production seating and the Jira-confirmed naming contract are unchanged, and no external materialization is forced |
| Console Vitest — `expected ['DeepSeek V4.1 Flash'] to deeply equal ['DeepSeek V4 Flash']` | the daemon catalog and the console state both already say V4.1 on master; this branch changes no frontend or catalog source | the one stale assertion updated. Still an exact `toEqual` over the full option list, so it still proves exactly one route is offered and names it |

The `an_empty_realm_…` repair was referred to the scope owner rather than
guessed: the journey declared only `execution_scope.kontor_backlog_code`, which
places an epic in a runtime, while seating derives its display item code from
the separate top-level `epic_backlog_code` that persists the active immutable
namespace. The alternative — relaxing the seating guard or forcing Jira
materialization into the bootstrap — would have changed a frozen surface.

### Residual risk: the lockfile gate is a moving target

The gate byte-compares a *freshly resolved* lock against the committed one, so
any crates.io publication between commit and verification re-opens it for
reasons no candidate controls. This rejected a candidate twice in this work:
`ipnet` 2.12.1→2.12.2 earlier, and `clap_lex` 1.1.0→1.1.1 **during** gate run
six. Exit 0 above is therefore a statement about this candidate at this instant,
not a property that survives an arbitrary delay before delivery. A candidate
that sits unverified long enough will fail this gate again with no code change.

### Not done here, deliberately

- This Gate 7 candidate was not pushed, published, merged, or deployed.
- No topology created, no Kontor or Jira state modified.
- The ASMA-8115 publication MCP vocabulary baseline is untouched; ASMA-8115 owns it.
- The gate was not waived, and no failure was reclassified to avoid fixing it.

## Sixth remediation (superseded): preserve cross-slot recovery lineage

The canonical replacement of the settled implementation slot exposed a defect
that the earlier archive could not observe. The existing TeamRun contains an
operator-abandoned, unbound verify parent whose terminal recovered successor is
still authoritative. While replacing the unrelated implementation slot,
`slot_members` discarded the abandoned parent before `TeamRunSlots::hydrate`
could attach its successor. Hydration therefore received a parentless successor
and rejected the supported request with `400 invalid TeamRunSlots`. The refusal
was side-effect free; the predecessor implementation run had already been
settled terminal under receipt
`01a0a1a1-f385-7ef3-a1e3-fea657bff5dd`.

`slot_members` now passes the complete lineage to `TeamRunSlots::hydrate`,
excluding only rows already named as a recorded successor. Hydration remains
the single authority that keeps referenced abandoned parents and drops
unreferenced ones. This preserves the exact old TeamRun, AgentRun, seat, native
session and recovery chain while allowing an unrelated terminal slot to be
replaced.

The loopback regression
`replacing_one_slot_preserves_an_abandoned_parent_in_another_slot` constructs
that exact cross-slot shape, recovers the verify successor, replaces a separate
terminal slot, and proves the successor still names its abandoned parent. It
failed before the production correction and passes afterward.

### Current frozen candidate

| | |
|---|---|
| **Candidate SHA** | `dc6cb3718671d1ef421797be88301c9dda64b7b1` |
| **Tree SHA** | `2a6902787555cc3b42e6a456c6e6d8f71bcbc0ff` |
| **Integrated master** | `f78d041e80042417e0d9a059449eb85737571797` (schema **96**) |
| **Archive exit** | **0** |
| **Archive log digest** | `522aded4a78d63bf0aff770df89520dcdbad2bc17abb6f2848d8e31edbf426b7` |

This candidate is code-only. The evidence documents are committed separately
above it. The fifth-remediation Gate 7 candidate and digest remain in this
append-only record as superseded evidence; they are not the verification or
publication target.

### Gate 8 result

`python3 scripts/verify-tree.py --mode archive` ran from a `git archive` export
of the exact candidate with registry access:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | `Cargo.lock byte-compare: identical` |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2478 passed, 0 failed** (9 ignored) |
| `loopback_api` | **329 passed, 0 failed** (1 ignored) |
| `cargo audit` | passed |
| `cargo deny check` | passed |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm -r typecheck` | passed |
| `pnpm -r test` | **300 passed** (16 files) |
| `pnpm audit --prod` | passed |

No gate was waived. This Gate 8 candidate was not pushed, published, or
merged. Operational promotion of this exact candidate is permitted only to
restore the supported seat-replacement route in the same realm; final delivery
still requires independent verification, audit, publication, merged-SHA
deployment, and identity-preserving readback.

## Seventh remediation (superseded): preserve lineage through quota succession

Independent inspection of Gate 8 found the same premature pre-filter in
`launch_succession_successor`. That path hydrates the whole TeamRun before a
quota successor is launched. On a TeamRun containing a recovered verify
successor and its referenced abandoned-unbound parent, the filter again removed
the root and made quota succession in any other slot fail as `400
invalid_request`, subject `TeamRunSlots`.

The same bounded correction now applies at both hydration call sites:
`slot_members` excludes only a row already named as the recorded successor and
passes the complete recovery lineage to `TeamRunSlots::hydrate`. Hydration's
existing rule remains authoritative: it retains a referenced abandoned parent
and removes an unreferenced abandoned attempt. The never-bound reroute path is
unchanged because its abandoned attempt is not yet referenced when it is
hydrated.

The new
`succeeding_one_slot_preserves_an_abandoned_parent_in_another_slot` regression
freezes the durable `SuccessionAttempt` directly in the store, pins the sibling
seat's account, and drives the supported `successors:recover` saga through
retirement, launch, whole-TeamRun hydration, and readback. It deliberately does
not retest `recover_quota_seat`'s earlier fresh-observation, provenance-matching,
or headroom-walk planning; the defect is in the later launch step's roster. With
only the production hunk reverted it fails at that succession route with `400` and
`subject: "TeamRunSlots"`; with the fix restored it passes. It also proves that
the successor was created in the quota-exhausted slot while the recovered
successor in the other slot still points to its abandoned parent. The ordinary
replacement regression and quota-succession regression pass together.

### Current frozen candidate

| | |
|---|---|
| **Candidate SHA** | `18923bd0387f8bc88c1d5ed3bb878c478b3e189f` |
| **Tree SHA** | `e6f7b3713b998fa08bacd83c9eb1847313c19612` |
| **Parent candidate** | `dc6cb3718671d1ef421797be88301c9dda64b7b1` |
| **Integrated master** | `f78d041e80042417e0d9a059449eb85737571797` (schema **96**) |
| **Archive exit** | **0** |
| **Archive log digest** | `5eb052058249b07362877140784c742e2886f70dbe9455341cf3ed2257b48c46` |

The candidate is code/test only. It changes
`crates/kontor-daemon/src/applications.rs` and
`crates/kontor-daemon/tests/loopback_api.rs`; these evidence documents are
committed separately above it. Gate 8 remains immutable superseded evidence.

### Gate 9 result

`python3 scripts/verify-tree.py --mode archive` ran from a `git archive` export
of the exact candidate with registry access:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | `Cargo.lock byte-compare: identical` |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2479 passed, 0 failed** (9 ignored) |
| `loopback_api` | **330 passed, 0 failed** (1 ignored) |
| both MCP journeys | passed |
| `cargo audit` | passed |
| `cargo deny check` | passed |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm -r typecheck` | passed |
| `pnpm -r test` | **300 passed** (16 files) |
| `pnpm audit --prod` | passed |

No gate was waived or skipped. The nine ignored Rust tests predate this
candidate; neither new regression is ignored. The current candidate has not
been pushed, published, or merged. Independent verification and audit must
evaluate this SHA and the evidence-only commit above it.

The generic archive script logs that it exported `HEAD` but does not print the
commit or tree SHA. The Gate 9 binding is therefore established by the invoking
seat's recorded HEAD, the new test identities present in the log, the exact test
count delta, and this separately committed evidence; improving the generic log
format is deferred as OQ-B and is not part of this candidate.

## Eighth remediation (superseded): resolve the fence's role through the frozen template

Independent verification rejected candidate `18923bd` at gate sequence 4 under
receipt `01a0a243-c170-7970-9d17-2bf7675f97eb`. Gate sequence 3 had passed and
the fresh high-change turn met freshness, the route-time TeamRun and the
artifact conditions, yet the workflow stayed pinned at `high-implementation@7`.

### F-8110-R9: two identities compared as one string

`rejection_fence_holds` reads the logical role off the workflow edge leading
*into* the rejection target — for this task, `fleet-implementer` — and compared
it directly to the settled turn's `role_slot_id.as_role_key()`, whose durable
value is the concrete slot `implement`. A team template maps a slot onto a
logical role; the two are deliberately separate identities, and one role may be
carried by several slots. Comparing the strings meant no qualifying rework could
ever match, so the fence could not be released by the only turn entitled to
release it. The gate that would have cleared the phase had already passed.

The correction resolves each candidate turn's concrete slot through the
immutable team template pinned by `route.team_run_id`, then compares that
slot's logical role to the edge's `handoff_role`.

- **Route-time authority is unchanged.** The template comes from the route row's
  own TeamRun, never the task's latest run and never the mutable catalog, so a
  definition published after a rejection cannot decide who may answer it.
- **Resolution fails closed at every step.** A route TeamRun that cannot be read
  and a snapshot whose frozen template does not verify both return the fence
  held; a slot the template never declared cannot match. An identity nobody can
  check does not release a fence.
- **Everything else is untouched.** Freshness, required artifacts, task and the
  exact route-time TeamRun checks are byte-identical, and an entry phase still
  names no role, consults no template, and rests on the other conditions exactly
  as before.

### Why the existing suite could not have caught it

Every pre-existing fence case routes to the bundled profile's **entry** phase,
which has no inbound edge. `handoff_role` is therefore `None` on all of them and
the role comparison is never evaluated. The new regressions run on a purpose-built
pack fixture, `crates/kontor-profiles/tests/fixtures/custom-pack-f.json`, whose
rejection target `high-implementation` is *not* the entry phase, so the edge into
it genuinely names a role — and whose team separates the two identities on
purpose: slot `implement` carries role `fleet-implementer`, while a decoy slot
*spelled* `fleet-implementer` carries `fleet-reviewer` instead.

`a_fresh_high_change_turn_releases_a_non_entry_fence_through_its_logical_role`
walks the recorded incident in order: the verification gate rejects and routes
to `high-implementation`; pre-rejection evidence is re-read three times without
advancing; a fresh `high-change` is settled in slot `implement` on the route's
own preserved run; the verification gate then passes; and reconciliation keeps
the workflow past the rejection target while writing no second route row and no
turn of its own.

> **Correction (ninth remediation).** The claim above is narrower than it reads.
> That test's workflow advanced **at the moment the rework turn settled**,
> because a settlement is itself a caller-driven reprojection. The later
> `reconcile` calls only confirmed the workflow stayed where the settlement had
> already put it. It therefore proved the corrected predicate, and did **not**
> prove that a realm already holding durable qualifying evidence recovers when a
> corrected binary starts against it. That gap is F-8110-R10, recorded below.

`a_slot_spelled_like_the_required_role_cannot_release_the_fence` settles the
decoy seat with the required artifact, on the route's own run, strictly after
the route — every condition satisfied except the one that matters — and proves
the fence stays closed, then that the seat actually carrying the role opens it.

### Red then green

| Regression | Against the pre-fix comparison | With the correction |
|---|---|---|
| positive release | `left: "high-implementation", right: "high-verification"` — the entitled rework cannot release the fence | passes |
| decoy slot | `left: ("high-verification", 5), right: ("high-implementation", 4)` — the wrong seat releases it | passes |

Both directions of the defect are reproduced, not just the reported one. The
retained stale-evidence, empty-turn, reviewer, other-task and later-TeamRun
cases are unchanged and pass alongside them.

### Current frozen candidate

| | |
|---|---|
| **Candidate SHA** | `705571de51f9e152ba3029a1f36961807a333e8e` |
| **Tree SHA** | `1cc762b0fa3f537217d36aa9a9a16959344e6ef3` |
| **Parent candidate** | `36ddec5ad519c300d77a57e0b7e8afa8e0180966` (Gate 9 evidence head) |
| **Integrated master** | `f78d041e80042417e0d9a059449eb85737571797` (schema **96**) |
| **Archive exit** | **0** |
| **Archive log digest** | `8b4b116cd0da7ca62eb124d4a29cd9250c273ad398d764504b844a5c6daf2b89` |

The candidate is code/test only. It changes
`crates/kontor-daemon/src/applications.rs` (+39/-3),
`crates/kontor-daemon/tests/loopback_api.rs` (+464/-0) and adds
`crates/kontor-profiles/tests/fixtures/custom-pack-f.json` (+180). These evidence
documents are committed separately above it, and the candidate changes no file
under `docs/`. Gates 7, 8 and 9 and their digests remain in this append-only
record as superseded evidence.

### Gate 10 result

`python3 scripts/verify-tree.py --mode archive` ran from a `git archive` export
of the exact candidate with registry access:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | `Cargo.lock byte-compare: identical` |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2481 passed, 0 failed** (9 ignored) |
| `loopback_api` | **332 passed, 0 failed** (1 ignored) |
| both MCP journeys | passed |
| `cargo audit` | passed |
| `cargo deny check` | passed |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm -r typecheck` | passed |
| `pnpm -r test` | **300 passed** (16 files) |
| `pnpm audit --prod` | passed |

No gate was waived or skipped. All ten gate invocations are in the log and the
script raises on any non-zero, so exit 0 entails every gate passed. The nine
ignored Rust tests are byte-identical to Gate 9's set; neither new regression is
ignored, and the test-file change is additive, so no existing assertion was
removed or weakened.

The archive-log identity limitation recorded for Gate 9 applies unchanged here:
the script prints that it exported `HEAD` without naming the commit or tree. The
Gate 10 binding rests on the invoking seat's recorded HEAD, the two new test
identities present in the log, the exact 2479 → 2481 count delta, and this
separately committed evidence. OQ-B remains deferred and is not part of this
candidate.

### Not done here, deliberately

- No topology, TeamRun, Jira change, publication, or deploy.
- The candidate was not pushed, published, merged, or deployed.
- No Kontor state was mutated and the protected ASMA-8100 receipt is untouched.
- Route-time TeamRun authority was not widened, and no gate was waived.

## Ninth remediation (superseded): converge realms the earlier predicate already stranded

Independent verification rejected candidate `705571d` at gate sequence 5 under
receipt `01a0a289-db11-7323-ab7b-cfb195087266`. The logical-role mapping fix is
correct and is preserved unchanged; the finding is about the realms it was wrong
about.

### F-8110-R10: a corrected predicate nobody asks

`advance_workflow_from_evidence` runs only when a caller drives it — a settled
turn, or a gate record that did not reject. That is sufficient while the
predicate deciding a fence is right, and insufficient the moment it is
corrected. A realm whose qualifying turn *and* passing gate verdict were already
durable when the binary was upgraded has no settlement left to make: nothing
ever asks the question again, so the workflow sits on its rejection target
indefinitely with every release condition already satisfied on disk. The Gate 10
regression could not have caught this, because its advance happened at
settlement time; the correction to that statement is recorded in place above.

### The mechanism

- `SqliteStore::list_fenced_task_workflows` enumerates exactly the stuck
  population: active workflows sitting on a phase a route returned them to. That
  is the same join `active_gate_rejection_fence` applies to a single workflow, so
  it cannot report a workflow that reads as unfenced, and a realm that never
  rejected a gate is not touched at all.
- `Services::catch_up_fenced_workflows` reprojects each one through the existing
  evidence projection and reports how many moved.
- `Daemon::reconcile` calls it once per supported startup, on the same
  post-barrier seam that already owns "what did this realm leave unfinished?".

It is a projection, not a repair. It reads durable evidence and writes at most
the phase advance that evidence already justifies: no turn replayed, no gate
evaluation re-recorded, and the rejection route neither rewritten nor removed —
it stays as history exactly as before. A converged realm gives it nothing to do,
which is what makes a restart loop safe.

It fails closed in both directions. A workflow whose state cannot be read is
skipped rather than advanced. And the new `PhaseRoute::Unambiguous` mode stops
the catch-up wherever a phase leads to more than one successor: choosing a branch
with no turn behind the choice is not a projection of evidence. Both
caller-driven call sites keep `PhaseRoute::Declared`, so settlement behaviour is
byte-for-byte what it was.

### The regression, and what makes it honest

`a_restart_converges_a_workflow_left_fenced_by_the_earlier_predicate` builds the
stuck state out of rows the ordinary public path actually wrote — the rejection
route, the qualifying `implement` turn, the passing verification verdict — and
then pins the workflow's one mutable column back onto `high-implementation`.
That reproduces the database an earlier build would have left rather than
simulating one. Recovery is then driven through the supported restart:
`Daemon::start` over the same state root, followed by `reconcile`.

It asserts the workflow converges, then reconciles three more times and asserts
the phase and revision do not move again, and that the route, role-turn and
gate-evaluation rows are identical in identity and content to the census taken
before the restart. The route is additionally compared field by field.

### Red then green

| Condition | Result |
|---|---|
| startup catch-up hunk reverted, logical-role fix intact | **fails**: `left: "high-implementation", right: "high-implementation"` — the restarted daemon leaves the workflow stranded |
| both logical-role regressions, same reverted build | **pass** — which is what isolates this finding to the catch-up rather than to the predicate |
| catch-up restored | all five regressions pass together |

### Current frozen candidate

| | |
|---|---|
| **Candidate SHA** | `5d9f9799f6a335c090e23f98bc11985e2ae4e8ed` |
| **Tree SHA** | `8c7c38f2bc221d1c0af34f13b889829cc3c78cc3` |
| **Parent candidate** | `e493d47c4ef2b53d9ca8582854e79c6e1b7befe5` (Gate 10 evidence head) |
| **Integrated master** | `f78d041e80042417e0d9a059449eb85737571797` (schema **96**) |
| **Archive exit** | **0** |
| **Archive log digest** | `3bc0ad3e64bdb85d4fe5f697e02ad57a87c598b41e58a43e35115b520931c68c` |

Code/test only, four files, nothing under `docs/`:

| File | Change |
|---|---|
| `crates/kontor-daemon/src/applications.rs` | +104 / -6 |
| `crates/kontor-daemon/src/lib.rs` | +21 / -0 |
| `crates/kontor-daemon/tests/loopback_api.rs` | +241 / -0 |
| `crates/kontor-store/src/repository.rs` | +39 / -0 |

Gates 7 through 10 and their digests remain in this append-only record as
superseded evidence.

### Gate 11 result

`python3 scripts/verify-tree.py --mode archive` ran exactly once, from a
`git archive` export of the exact candidate with registry access:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | `Cargo.lock byte-compare: identical` |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2482 passed, 0 failed** (9 ignored) |
| `loopback_api` | **333 passed, 0 failed** (1 ignored) |
| both MCP journeys | passed |
| `cargo audit` | passed |
| `cargo deny check` | passed |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm -r typecheck` | passed |
| `pnpm -r test` | **300 passed** (16 files) |
| `pnpm audit --prod` | passed |

No gate was waived or skipped; all ten invocations are in the log and the script
raises on any non-zero. The nine ignored Rust tests are byte-identical to Gate
10's set, no new regression is ignored, and the test-file change is additive. The
retained fence family — stale evidence, empty turn, reviewer, other task, later
TeamRun — and both cross-slot recovery regressions pass unchanged.

The archive-log identity limitation recorded for Gates 9 and 10 applies here too:
the script prints that it exported `HEAD` without naming the commit or tree. The
Gate 11 binding rests on the invoking seat's recorded HEAD, the new test identity
present in the log, the exact 2481 → 2482 count delta, and this separately
committed evidence. OQ-B remains deferred.

### Not done here, deliberately

- No topology, TeamRun, Jira change, publication, or deploy.
- The candidate was not pushed, published, merged, or deployed.
- No Kontor state was mutated and the protected ASMA-8100 receipt is untouched.
- Route-time TeamRun authority was not widened, and no gate was waived.

## Tenth remediation: the catch-up must not queue behind a runtime

The first finding in this task's history that came from a *live deployment*
rather than from reading the diff. The LSA installed the exact Gate-11 candidate
`5d9f979` (binary SHA-256
`50b685dc20b009c5a20885b7522cfed01f12f3a779cc7aea0b8346c42ca6befb`) at
02:25:00Z. The listener was healthy and the scheduling barrier opened at
02:25:10.630832Z.

### The observed live sequence, start to finish

| Time (2026-09-15) | Observed |
|---|---|
| 02:25:00Z | exact Gate-11 candidate installed |
| 02:25:10.630832Z | scheduling barrier opened |
| ~02:32Z | workflow still `high-implementation` at revision 9, gate rejected, no "fenced workflows converged" line and no "startup reconciliation finished" marker |
| 02:39:13.028833Z | **"fenced workflows converged" logged** |
| 02:39:14.257255Z | **startup reconciliation finished** |

The R10 catch-up therefore *did* run and *did* do its job. Kontor and a
read-only read of the realm database now show workflow
`01a07391-328e-74a3-a808-e7cbffc4b828` at **`high-verification`, revision 10**,
with 15 turns, 5 gate evaluations, 4 immutable rejection routes, and the 61
dispatches still undelivered. The mid-flight observation at ~02:32Z was taken
while startup was still inside the retry; it was accurate at that moment and is
not the final state.

### F-8110-R11: a local durable repair queued behind a network wait

`Daemon::reconcile` awaited `retry_undelivered_dispatches` before
`catch_up_fenced_workflows`. That retry hands each undelivered follow-up to a
runtime and waits for the answer. The live realm holds **61** historical
undelivered `turn_dispatches`, including targets last contacted on 22 August
whose seats are long gone. Working through them took the retry from 02:25:10Z to
02:39:13Z, so the catch-up behind it was delayed by roughly **14 minutes** on a
realm where the evidence releasing the fence had been durable the whole time.

Two things are wrong with that, and the second is the serious one.

- **The delay is unnecessary.** The catch-up reads and writes only this realm's
  own database and owes nothing to any native session. Fourteen minutes of a
  workflow sitting fenced is fourteen minutes of an operator reading a state that
  the realm's own evidence had already disproved.
- **The bound is not ours to set.** The retry's duration is a property of how
  many stale targets a realm accumulated and how each runtime answers. Nothing
  guarantees it terminates: one target that accepts a connection and never
  replies would hold startup open indefinitely, and the local durable repair
  behind it would never run at all. This deployment was slow; the next one is
  not required to be merely slow.

Nothing was wrong with the catch-up itself. It was placed behind a wait it has
no business being behind. Ordering was the entire defect, and ordering is the
entire fix. The catch-up now runs first, immediately after the barrier opens and
before anything that awaits a runtime — so the repair completes in the same
moment the barrier opens, whatever the retry behind it goes on to do.

Everything else is preserved exactly. The retry still runs, unmodified, in the
same position relative to `reopen_completed_epics` and `retry_completion_wakes`;
no retry is dropped, skipped or reordered among themselves; and the barrier is
settled where it always was, so barrier semantics are untouched.

### Why the Gate-11 suite could not have caught this

Every earlier restart regression ran against a realm with no undelivered
dispatch to retry, so the awaited call returned immediately and the catch-up's
position behind it never mattered. No test could observe either the delay or the
unbounded case. The new regression removes that accident deliberately, and holds
the send open rather than merely slowing it, because the failure worth excluding
is the one where the runtime never answers.

`FakeAdapter::pause_next_send` holds a message send immediately before its
native effect, mirroring the pause the fake already offers for hosted
retirement. It lets a test occupy the stall deterministically instead of
depending on a timeout, which is what makes the red side of this provable rather
than merely slow.

`a_stalled_follow_up_delivery_does_not_delay_the_fenced_catch_up` rebuilds the
stuck state, re-opens an already-delivered dispatch so the restart genuinely owes
a delivery, and then holds startup inside the awaited send — the exact position
the live realm spent fourteen minutes in, held open here rather than merely
slowed, because the case worth excluding is the one that never returns. It
asserts the workflow has *already* converged at
that instant, then releases the runtime and asserts the retry completed and
delivered, the barrier still opened, three further reconciliations move nothing,
and the route, role-turn and gate-evaluation rows are unchanged.

### Red then green

| Condition | Result |
|---|---|
| catch-up returned to its Gate-11 position | **fails**: `left: "high-implementation", right: "high-implementation"` — startup is inside the unanswered send and the workflow is still fenced |
| R9 and R10 regressions, same reverted build | **pass** — which isolates this finding to the ordering, not to the predicate or the catch-up itself |
| catch-up first | all six regressions pass together |

### Current frozen candidate

| | |
|---|---|
| **Candidate SHA** | `741443f1b6b8df122c8425471c6ae7cdadf8d64f` |
| **Tree SHA** | `405006ebe8ddf892550bd5a941459029cb6143b1` |
| **Parent candidate** | `11bb3e4e79c0c06e1867b1f6cc50f416746c92bb` (Gate 11 evidence head) |
| **Integrated master** | `f78d041e80042417e0d9a059449eb85737571797` (schema **96**) |
| **Archive exit** | **0** |
| **Archive log digest** | `036965a07453e814ff6cd17183ce8c88d7b36a134d0a8066ef948d6e9f6400a0` |

Code/test only, three files, nothing under `docs/`:

| File | Change |
|---|---|
| `crates/kontor-daemon/src/lib.rs` | +29 / -21 |
| `crates/kontor-daemon/tests/loopback_api.rs` | +178 / -0 |
| `crates/kontor-runtime/src/fake.rs` | +19 / -0 |

Gates 7 through 11 and their digests remain in this append-only record as
superseded evidence.

### Gate 12 result

`python3 scripts/verify-tree.py --mode archive` ran exactly once, from a
`git archive` export of the exact candidate with registry access:

| Gate | Result |
|---|---|
| `cargo generate-lockfile` + byte-compare | `Cargo.lock byte-compare: identical` |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | **2483 passed, 0 failed** (9 ignored) |
| `loopback_api` | **334 passed, 0 failed** (1 ignored) |
| both MCP journeys | passed |
| `cargo audit` | passed |
| `cargo deny check` | passed |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm -r typecheck` | passed |
| `pnpm -r test` | **300 passed** (16 files) |
| `pnpm audit --prod` | passed |

No gate was waived or skipped; all ten invocations are in the log and the script
raises on any non-zero. The nine ignored Rust tests are byte-identical to Gate
11's set. All six ASMA-8110 regressions pass — the two cross-slot recovery cases,
the logical-role and decoy cases, the restart-convergence case and the new
stalled-delivery case — as do all four retained fence cases: stale evidence,
later TeamRun, released-rejection-stays-in-verification, and historical recovery
across a restart.

The archive-log identity limitation recorded since Gate 9 applies unchanged.
OQ-B remains deferred.

### Not done here, deliberately

- No topology, TeamRun, Jira change, publication, or deploy.
- The candidate was not pushed, published, merged, or deployed.
- The preserved AgentRun, SeatBinding and TeamRun identities are untouched.
- No gate was waived, and no retry behaviour was weakened to achieve the fix.
