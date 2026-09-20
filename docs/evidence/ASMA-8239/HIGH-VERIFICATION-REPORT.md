Artifact: `high-verification-report`

# ASMA-8239 / PUB-09 high-verification report

Date: 2026-09-20
Task: Jira `ASMA-8239` / Kontor `01a0bbbc-6e93-7b22-980e-3c023109b1fb`
Phase: `high-verification`
TeamRun: `01a0bbc2-fa0f-7932-a6c6-887210308972`
Verifier AgentRun: `01a0c055-7bb6-75b3-aaa8-e47b4668f3b4`
Exact tested commit: `24290153c42c21743d606bcf5e1648681af4a69a`
Exact tested tree: `186dcd0a9ee8c3527197b3d347862cbd20f80a40`
Report path: `docs/evidence/ASMA-8239/HIGH-VERIFICATION-REPORT.md`
Status: **PASS for the frozen ASMA-8239 contract, with the adjacent ASMA-8115 workspace-kind limitation preserved below**

## Verdict

The integrated ASMA-8239 behavior at merge commit `24290153` passes its frozen
acceptance. Independent focused execution proved the vacant exact-parent/path
case creates one native; both lost-ack paths adopt rather than create again;
the topology node and logical binding identities survive; durable recovery
history remains append-only; and same-key replay after a daemon restart returns
the original disposition, receipt and replacement native identity without a
runtime being registered.

The existing adoption/refusal regressions, direct ASMA-8188 applicability
matrix, seat-replacement replay, API/OpenAPI contract, schema-114 migration,
formatting and strict clippy checks are green on the same exact tree. The
historical `correct_task_worktree` command-target failure recorded by the
source-branch high-change artifact is closed in this integrated target; its
exact regression passes.

This is a qualified release result, not a claim that every adjacent container
invariant is closed. The disclosed ASMA-8115 defect is reachable at the
ASMA-8239 recreation boundary: the Paseo recreation census/readback validates
native ID, exact parent, canonical path and title, but not `workspaceKind`.
Consequently a task-scoped `local_checkout` at the exact placement could be
projected as a `NativeChild` and adopted although task containers require a
Git worktree. That does not make any frozen ASMA-8239 test red, but it is
release-relevant and must not be hidden. Its owner remains ASMA-8115; this turn
did not duplicate or modify that repair. Re-run the focused ASMA-8239 suites
after the ASMA-8115 fix is integrated.

No production source was changed. No live native, topology, task, seat, run,
deployment, gate, watchdog or lifecycle state was created or mutated; only the
recorded handoff metadata was read.

## Candidate and handoff boundary

- The registered source handoff is commit
  `5f75a96172b4a8f6883cad7fe0eb61630dd0067c`.
- Its frozen scope record is
  `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`, SHA-256
  `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33`.
- Its high-change record is
  `docs/evidence/ASMA-8239/HIGH-CHANGE-RECORD.md`, SHA-256
  `cc8a5231c3b6c42000a9391f3d713b849ac4970006ccf4eb41e6474ee41eb6a8`.
- Both files at exact target `24290153` have those exact hashes.
- Original implementation commits are `aaf5d9762fc425f94b78fc9ff9252e9f25699333`
  and `35cbbd40e1343ec39fbd088d44530295e99c8cbd`.
- Integration staging commits are `f95daa775c571221079460591bb3701f3ad4bcce`,
  `5941d27af4e3798cec6d9933a0f30e46f6b0796a`,
  `f1b4b0344431eb0bcd164f650f57a1b7290145af` and
  `7f47598e24d6b61f837a49a2dcd5f94c53f945c2`.
- The merge contains their rebased equivalents `27ce37ff`, `380cc710`,
  `ce3360ae` and `6c2199ca`; the first three pairs have identical stable patch
  IDs. `6c2199ca` is the integration-context form of the schema-114 recovery
  disposition fix and is an ancestor of `24290153`.

All test commands ran in a clean detached worktree at exact `24290153`. Its
tree hash remained `186dcd0a9ee8c3527197b3d347862cbd20f80a40` and `git status
--porcelain` remained empty after verification. Build outputs were directed to
an ignored shared Cargo target directory and are not candidate source.

## Acceptance results

| Acceptance | Independent evidence at `24290153` | Result |
| --- | --- | --- |
| exact old native absent; exact parent/path vacant; exact rendered title | runtime recreation suite creates exactly once and reads back parent/path/title; daemon preview is write-free and apply creates once | **pass** |
| exact-parent/path/title refusals | live persisted native, duplicate candidates, wrong title, and post-create wrong-parent readback all refuse with no second create | **pass** |
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
detached target; none is copied from the implement report.

All Cargo commands used
`CARGO_TARGET_DIR=/Users/igor/carasent/asma-modules/.worktrees/asma-8239/asma-rs-kontor/target`;
the table omits that common environment prefix for readability.

| Command | Result |
| --- | --- |
| `cargo test --locked -p kontor-runtime-paseo --test contract container_recreation -- --nocapture` | **6 passed, 0 failed** |
| `cargo test --locked -p kontor-daemon --test container_recreation -- --nocapture` | **12 passed, 0 failed** |
| `cargo test --locked -p kontor-runtime-paseo --test contract` | **264 passed, 0 failed** |
| `cargo test --locked -p kontor-store --test legacy_naming_recovery` | **2 passed, 0 failed** |
| `cargo test --locked -p kontor-store --test schema_v1 an_empty_database_migrates_to_the_current_schema_version -- --nocapture` | **1 passed, 0 failed**; schema 114 asserted |
| `cargo test --locked -p kontor-api -- --nocapture` | **34 passed, 0 failed** (library 26, error contract 5, OpenAPI 3) |
| `cargo test --locked -p kontor-daemon --test loopback_api an_admin_replaces_one_runtime_cancelled_seat_inside_the_existing_team -- --exact --nocapture` | **1 passed, 0 failed** |
| `cargo test --locked -p kontor-core --test domain_state every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state -- --exact --nocapture` | **1 passed, 0 failed**; historical ASMA-8120 blocker is closed here |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --locked -p kontor-runtime -p kontor-runtime-paseo -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | passed, zero warnings |
| `git diff --check` and final detached-worktree status | passed; clean |

The separately executed exact
`a_bound_tsw_container_refuses_every_non_worktree_shape` inspection regression
also passed. It establishes the intended task-container invariant, but does not
close the ASMA-8115 limitation because the ASMA-8239 recreation functions do
not invoke that inspection check.

## Adjacent limitation — ASMA-8115, not reassigned

At exact target `24290153`:

- `ContainerWorkspaceKind::is_applicable_to(true)` accepts only `Worktree` in
  `crates/kontor-runtime/src/container.rs`.
- ordinary exact-ID container inspection maps and validates
  `workspace.workspace_kind` before rehydrating a task container in
  `crates/kontor-runtime-paseo/src/adapter.rs`.
- `ContainerRecreationRequest` carries no task/epic scope or required workspace
  kind.
- `recreation_census` selects candidates only by exact parent and canonical
  path, then title; `recreation_readback` rechecks parent, path, title and
  absent-native inequality. Neither checks `workspace.workspace_kind`.
- all checked-in recreation fixtures report `workspaceKind: worktree`; none
  exercises `local_checkout`.

Therefore the known ASMA-8115 defect is relevant to the recovery surface even
though the frozen ASMA-8239 matrix is green. The corrective branch named by the
operator, `fix/ASMA-8115-recovered-container-kind` from `592dc87f`, had no
committed delta available to this verifier and was not inspected, altered or
reimplemented. This report neither approves that pending fix nor treats the
gap as an ASMA-8239 code change.

## Limitations and non-claims

- No full workspace/archive gate was run; verification was focused on the
  frozen ASMA-8239 behavior and affected Rust surfaces.
- No fresh mutation was performed. The implement turn's mutation receipts were
  reviewed, but independent status here rests on retained tests and static
  inspection.
- Runtime tests use the recorded Paseo protocol harness and daemon fake, not a
  live Paseo daemon or production Realm.
- No deployment, publication, merge, gate, attestation, Jira/Kontor lifecycle
  transition or `Done` claim is made by this report.
- ASMA-8188 item 6 remains correctly named; this report does not claim separate
  ASMA-8189 remediation negatives.

## Open questions

None. The exact target, registered source artifacts, integrated commit chain,
historical ASMA-8120 resolution and adjacent ASMA-8115 ownership are all
evidenced. The remaining ASMA-8115 work is a known external dependency, not an
ambiguous choice for this verifier.

## Handoff

ASMA-8239's frozen contract passes at `24290153`. Preserve the exact report
commit and file digest returned with this artifact. Before an unqualified
release claim, integrate the separately owned ASMA-8115 workspace-kind repair
and re-run at least the runtime recreation suite, daemon container-recreation
suite, store recovery suite, restart replay and strict clippy checks on the new
exact candidate.
