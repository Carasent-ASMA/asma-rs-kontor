Artifact: `high-verification-report`

# ASMA-8239 / PUB-09 independent high-verification report

Date: 2026-09-21
Task: Jira `ASMA-8239` / Kontor `01a0bbbc-6e93-7b22-980e-3c023109b1fb`
Phase: `high-verification`
TeamRun: `01a0bbc2-fa0f-7932-a6c6-887210308972`
Predecessor Verifier AgentRun: `01a0c055-7bb6-75b3-aaa8-e47b4668f3b4` (closed / cancelled)
This native: authorized bounded Paseo fallback; Kontor `context_resolve` returned `agent_run_id: null`
Exact tested commit: `24290153c42c21743d606bcf5e1648681af4a69a`
Exact tested tree: `186dcd0a9ee8c3527197b3d347862cbd20f80a40`
Report path: `docs/evidence/ASMA-8239/HIGH-VERIFICATION-REPORT.md`
Prior report commit (rehashed, not trusted): `d0db8d1290afd9f6461d9e5adec231a968fd5016`
Prior report SHA-256: `6233b784a81c2c159477e6eca27a96f3feb0f485dc2591a5438d82213104afbd`
Status: **PASS for the frozen ASMA-8239 contract, with the adjacent ASMA-8115 workspace-kind limitation preserved below**

## Verdict

Independent re-verification at exact merge commit `24290153` confirms the frozen
ASMA-8239 contract. The prior PASS at `d0db8d12` was rehashed and then ignored:
acceptance criteria were re-read from the frozen high-scope record, product
source at `24290153` was inspected, focused and relevant broad suites were
re-run, and one causal mutant of the still-live recreation guard was killed.

Vacant exact-parent/path recreation creates once and reads back parent, path
and title. Lost-ack adoption binds the one exact candidate with zero further
creates. Topology node and logical binding identities survive. Same-key replay
after daemon restart returns the original `recreate_absent` disposition, receipt
and replacement native identity with no runtime registered. Gate-rejection and
evaluator recovery create zero natives in a world that already has a
recreatable NativeChild. Existing adoption/refusal regressions, the PUB-07
successor replay, schema 114, formatting and strict clippy remain green.

This is a qualified release result. Recreation census and readback still do not
check `workspaceKind`. Ordinary exact-ID inspection does. A task-scoped
`local_checkout` at the exact placement could therefore be projected as a
`NativeChild` and adopted even though a task container must be a Git worktree.
That does not make any frozen ASMA-8239 test red. Its owner remains ASMA-8115.

No production source was changed. The mutant was reverted before this report
was written. No live native, topology, task, seat, run, deployment, gate,
watchdog or lifecycle state was created or mutated by this turn.

## Candidate and handoff boundary

- Frozen scope record at `24290153`:
  `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`, SHA-256
  `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33`.
- Frozen high-change record at `24290153`:
  `docs/evidence/ASMA-8239/HIGH-CHANGE-RECORD.md`, SHA-256
  `cc8a5231c3b6c42000a9391f3d713b849ac4970006ccf4eb41e6474ee41eb6a8`.
- Integrated ASMA-8239 commits on `24290153`: `27ce37ff`, `380cc710`,
  `ce3360ae`, `6c2199ca`.
- Tests ran in a clean detached worktree at `/tmp/kontor-8239-reverify-24290153`.
  `HEAD` remained `24290153c42c21743d606bcf5e1648681af4a69a`, the tree remained
  `186dcd0a9ee8c3527197b3d347862cbd20f80a40`, and `git status --porcelain` was
  empty after revert. Build outputs used
  `CARGO_TARGET_DIR=/tmp/kontor-8239-reverify-24290153-target` and are not
  candidate source.
- `24290153` is not an ancestor of prior report commit `d0db8d12`. The prior
  report lives on the feature branch; this turn tested the named merge commit
  itself.

## Independent source inspection at `24290153`

- Admin recovery remains one operation. `prepare_container_recovery` tries
  adoption first; only a positively vacant recreation census authorizes
  `recreate_absent`. `apply_container_recovery` is the sole caller of
  `recreate_prepared_container`.
- `RecreateTopologyContainer` is absent from this tree. Recreation is an
  internal second disposition of the existing Admin preview/apply surface, not
  a reusable operation. That matches the frozen scope's conforming reachability
  test: gate-rejection and evaluator recovery never call it.
- `PaseoAdapter::recreation_census` selects by exact parent and canonical path,
  then title; it refuses a still-live persisted native before the path census.
  It does not read `workspace.workspace_kind`.
- `PaseoAdapter::recreation_readback` rechecks parent, path, title and
  absent-native inequality. It does not read `workspace.workspace_kind`.
- `ContainerRecreationRequest` carries topology node, logical binding id,
  absent identity, parent native id, canonical cwd, rendered title and
  timestamp. It has no task/epic scope or required workspace kind.
- Ordinary exact-ID inspection in the same adapter maps
  `workspace.workspace_kind` through `ContainerWorkspaceKind::is_applicable_to`.
  `is_applicable_to(true)` accepts only `Worktree`.
- Store schema is `114`. Migration `0114_container_recovery_disposition.sql` is
  included. `an_empty_database_migrates_to_the_current_schema_version` asserts
  `SCHEMA_VERSION == 114`.

## Acceptance results

| Acceptance | Independent evidence at `24290153` | Result |
| --- | --- | --- |
| exact old native absent; exact parent/path vacant; exact rendered title | runtime recreation suite creates exactly once and reads back parent/path/title; daemon preview is write-free and apply creates once | **pass** |
| exact-parent/path/title refusals | live persisted native, duplicate candidates, wrong title, parked-elsewhere live native, and post-create wrong-parent readback all refuse with no second create | **pass** |
| lost-create acknowledgement | runtime retry adopts the one exact candidate with zero creates; daemon adoption preserves the logical binding | **pass** |
| logical identity and history | daemon retains topology node and container binding IDs; store CAS regression preserves identity and append-only recovery history | **pass** |
| in-process idempotent replay | same key returns the original receipt and replacement identity, reports `unchanged`, and leaves create count at one | **pass** |
| restart/replay equality | reopened daemon returns the original `recreate_absent` disposition, receipt ID and replacement native ID with no runtime registered | **pass** |
| persisted recovery disposition | migration 0114 is present, current schema is 114, and restart replay preserves `recreate_absent` | **pass** |
| operation-specific applicability | gate-rejection and evaluator recovery create zero natives; only container recovery reaches one create in the same world shape | **pass** |
| pre-existing adoption/refusal behavior | complete Paseo contract suite, including all three stale-container-recovery regressions | **pass** |
| PUB-07 continuation regression | exact admin successor replacement/retry test returns the same successor on replay/restart | **pass** |

## Independent command record

Every result below was executed during this verifier turn against the exact
detached target `24290153`. None is copied from the prior report.

All Cargo commands used
`CARGO_TARGET_DIR=/tmp/kontor-8239-reverify-24290153-target`.

| Command | Result |
| --- | --- |
| `cargo test --locked -p kontor-runtime-paseo --test contract container_recreation -- --nocapture` | **6 passed, 0 failed** |
| `cargo test --locked -p kontor-daemon --test container_recreation -- --nocapture` | **12 passed, 0 failed** |
| `cargo test --locked -p kontor-runtime-paseo --test contract` | **264 passed, 0 failed** |
| `cargo test --locked -p kontor-store --test legacy_naming_recovery` | **2 passed, 0 failed** |
| `cargo test --locked -p kontor-store --test schema_v1 an_empty_database_migrates_to_the_current_schema_version -- --nocapture` | **1 passed, 0 failed**; `SCHEMA_VERSION == 114` |
| `cargo test --locked -p kontor-api -- --nocapture` | **34 passed, 0 failed** (library 26, error contract 5, OpenAPI 3) |
| `cargo test --locked -p kontor-daemon --test loopback_api an_admin_replaces_one_runtime_cancelled_seat_inside_the_existing_team -- --exact --nocapture` | **1 passed, 0 failed** |
| `cargo test --locked -p kontor-core --test domain_state every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state -- --exact --nocapture` | **1 passed, 0 failed** |
| `cargo test --locked -p kontor-runtime-paseo --test contract a_bound_tsw_container_refuses_every_non_worktree_shape -- --exact --nocapture` | **1 passed, 0 failed**; intended task-container invariant, not invoked by recreation |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --locked -p kontor-runtime -p kontor-runtime-paseo -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | passed, zero warnings |
| `git diff --check` and final detached-worktree status | passed; clean tree `186dcd0a` |

## Causal mutant

One mutant, one suite, then immediate revert.

| Mutant | File | Suite | Result |
| --- | --- | --- | --- |
| M3 delete still-live native guard in `PaseoAdapter::recreation_census` | `crates/kontor-runtime-paseo/src/adapter.rs` | `container_recreation_refuses_a_live_persisted_native_parked_at_another_path` | **KILLED** |

The unmutated test asserts a `StaleBinding` refusal and zero creates. After the
guard was removed the same test failed with:

```text
expected StaleBinding; observed WorkspaceMismatch {
  rule: "the recreated container is not rooted at the preserved canonical path"
}
```

That is the claimed behavior regressing: a live persisted native parked off the
canonical path no longer stops recreation before a native effect. The file was
restored with `git checkout --`; the parked-native test passed again; `HEAD` and
the tree hash were unchanged.

The prior report at `d0db8d12` recorded no independent mutation. This turn
closes that gap. It does not claim a mutation score for the whole matrix.

## Adjacent limitation — ASMA-8115, not reassigned

Independently confirmed at exact target `24290153`:

- `ContainerWorkspaceKind::is_applicable_to(true)` accepts only `Worktree` in
  `crates/kontor-runtime/src/container.rs`.
- ordinary exact-ID container inspection maps and validates
  `workspace.workspace_kind` before rehydrating a task container in
  `crates/kontor-runtime-paseo/src/adapter.rs`.
- recreation census/readback do not.

The frozen ASMA-8239 matrix stays green. Re-run at least the runtime recreation
suite, daemon container-recreation suite, store recovery suite, restart replay
and strict clippy after the ASMA-8115 fix is integrated.

## Kontor evidence / settlement

Attempted on this unbound fallback native. No gate, lifecycle or Jira
transition was fabricated.

| Call | Result |
| --- | --- |
| `kontor_task_get` `ASMA-8239` | 200; confirmed Jira `ASMA-8239`; phase `high-verification`; revision 1; state `ready` |
| `kontor_context_resolve` idempotency `asma-8239-reverify-20260921-context` | 200; `agent_run_id: null`; `context_pack_id: null` |
| `kontor_run_get` `01a0c055-7bb6-75b3-aaa8-e47b4668f3b4` | 200; `lifecycle: cancelled`; `closed_at: 2026-09-21T13:30:05.500756Z`; `attached: false` |
| `kontor_turn_settle` on that predecessor run | **409** `revision_conflict`; rule: `this run is closed, so there is no bounded turn left to settle` |

Root reconciliation therefore has Git/test/runtime evidence in this file and
the exact Kontor refusals above. It does not have a bound verifier turn or a
recorded gate verdict from this native.

## Limitations and non-claims

- No full workspace/archive gate was run.
- Runtime tests use the recorded Paseo protocol harness and daemon fake, not a
  live Paseo daemon or production Realm.
- No deployment, publication, merge, gate, attestation, Jira/Kontor lifecycle
  transition or `Done` claim is made by this report.
- ASMA-8188 item 6 remains correctly named; this report does not claim separate
  ASMA-8189 remediation negatives.

## Open questions

None for the frozen ASMA-8239 contract. ASMA-8115 remains the known external
dependency named above.

## Handoff

ASMA-8239's frozen contract passes at `24290153`. Preserve the exact report
commit and file digest returned with this artifact. Before an unqualified
release claim, integrate the separately owned ASMA-8115 workspace-kind repair
and re-run at least the runtime recreation suite, daemon container-recreation
suite, store recovery suite, restart replay and strict clippy checks on the new
exact candidate.
