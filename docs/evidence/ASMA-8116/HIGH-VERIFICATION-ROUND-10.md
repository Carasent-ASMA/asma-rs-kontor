# ASMA-8116 high-verification report — round 10

Date: 2026-09-14
Artifact: `high-verification-report-round-10`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `04941eb9c260c76dd1573b09d88e564d29bc4728`
Candidate tree: `0b4bdfb70ae23721fcfce05ea8014f749ff8d1a2`
Candidate parent: `67aa8168cb1e88fcf49c0ae988a78b3679db03e3`
Repair account: `GATE-9-REPAIR-REPORT.md`
Status: **PASS — no release-blocking finding remains at the exact candidate**

## Verdict

F-8116-V18 is closed at
`04941eb9c260c76dd1573b09d88e564d29bc4728`. A proved same-immutable-issue
epic rename now advances durably on all three reconciliation outcomes:

- `Transition`: after the canonical-key dry run re-proves the immutable Jira
  issue at the write boundary;
- `NoOp` / `Converged`: at the non-writing exit, from the alias observation
  already proved against the stored immutable ID; and
- policy `Conflict` / `Blocked`: at that non-writing exit, after the conflict is
  recorded against the current key.

The two non-writing regressions prove zero outbound Jira mutation, exactly one
rename occurrence, a durable key of `MOVED-9`, and a replay that neither reports
nor spends another rename. The transition/current-key regressions remain green.
Initial different-ID and later write-boundary rebinds remain terminal before a
binding advance or Jira effect, so F-8116-V17 remains closed.

The result is **PASS / APPROVE** at the exact pushed candidate. No new
release-blocking finding was found.

## Exact candidate and review boundary

- Before this report was added, `HEAD` and the configured upstream both resolved
  to `04941eb9c260c76dd1573b09d88e564d29bc4728`; its tree is
  `0b4bdfb70ae23721fcfce05ea8014f749ff8d1a2` and its parent is the preserved
  round-9 verifier report commit `67aa8168cb1e88fcf49c0ae988a78b3679db03e3`.
- The candidate worktree was clean on branch
  `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`
  and exactly matched upstream (`+0/-0`).
- The candidate delta is three files: daemon application code, daemon loopback
  regressions and `GATE-9-REPAIR-REPORT.md`; 288 insertions and 9 deletions.
  `git diff --check 67aa816..04941eb` passes.
- No `kontor-core`, `kontor-store`, migration, naming-token, legacy-code or
  ASMA-8117 implementation file appears in this repair delta.
- Executable verification and every mutant ran only in an exact candidate
  archive at `/private/tmp/asma-8116-v10.pYVGc4/repo`. The real implementation
  sources were not edited.
- No Jira, topology, PR, merge, deployment or other external state was changed.

## Approved-plan and scope conformance

The historical approved ASMA-8049 plan remains the naming authority. Its
configuration-driven naming contract requires immutable native identity,
explicit durable item-code facts and pre-effect refusal when those facts are
absent. The ASMA-8116 high-scope contract further forbids inferring a legacy
`JiraItemCode` from a Jira key.

This repair changes only when an already-proved Jira rename is persisted. Both
new fixtures assign the explicit legacy epic backlog code `AUTO`; no code is
derived from `ASMA-1` or `MOVED-9`. The candidate does not change the renderer,
native naming tokens, templates or legacy hashes, and it preserves the Kontor
epic UUID while moving only the confirmed current Jira key. It therefore stays
inside the approved ASMA-8049 and ASMA-8116 boundaries.

## Finding disposition

### F-8116-V17 — closed and preserved

The initially proved immutable Jira issue ID remains carried separately from
`observation_hash`. `JiraConnector::validate_expected` compares that ID with
the write-boundary `LiveIssue` before digest validation and before any field,
assignment or transition effect.

The restored two-stage regression passes 1/1: the alias read proves issue
`901`; the canonical-key write-boundary read answers issue `999` with otherwise
identical protected fields; no transition is posted, the binding remains
`ASMA-1`, and no rename occurrence is spent. M17a removes only this comparison
and is killed because a transition is then posted.

### F-8116-V18 — closed

`advance_proved_rename` is now the one deferred-rename commit seam. It matches
only `IdentityDecision::Proceed { renamed: true, .. }`, delegates to the same
transactional store reconciliation used before, and increments `report.renamed`
only after that commit succeeds.

It is called in exactly three places:

1. the `NoOp` exit after matching transition intents are confirmed;
2. the typed policy-conflict exit after its current-key conflict is recorded;
3. the transition path after connector dry-run has re-proved the issue ID.

No additional identity read is introduced. A non-writing exit has no later
write boundary to await, while the transition path retains V17's stricter
ordering.

The two new regressions each execute the pass and a replay. Independently
observed results are:

| Outcome | First pass | Replay | Jira mutations |
| --- | --- | --- | --- |
| `NoOp` → `Converged` | `renamed = 1`, key `MOVED-9`, occurrence `+1` | `renamed = 0`, occurrence unchanged | zero throughout |
| `Conflict` → `Blocked` | `renamed = 1`, key `MOVED-9`, occurrence `+1` | `renamed = 0`, occurrence unchanged | zero throughout |

Normal transition reconciliation also passes under the current key and never
addresses the superseded alias. Task rename, stale-preview/current-key and task
different-ID behavior remain green and unchanged.

## Independent mutation and restoration record

Each mutant was applied alone in the exact isolated candidate and run against
its discriminating retained regression.

| Mutant | Independent result |
| --- | --- |
| M18a — remove the converged-exit advance | **killed, 0/1 passed**; outcome converged but `renamed` was `0` |
| M18b — remove the policy-conflict-exit advance | **killed, 0/1 passed**; outcome blocked but `renamed` was `0` |
| M17a — remove the write-boundary immutable-ID comparison | **killed, 0/1 passed**; a transition was posted to the rebound issue |
| M18c — treat every `Proceed` decision as a rename | **killed, 0/1 passed**; replay incorrectly reported `renamed = 1` |

After restoration the two non-writing regressions passed 2/2 and the V17
boundary regression passed 1/1. Mutation score is **4/4 killed (100%)**.

Exact-candidate and restored SHA-256 values match:

| Source | Candidate and restored SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `0a9704e2461f70540a064e6006aabb0a61ebafb8b8d83c400b45c9f568ce56e7` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `89493714a68debe94466ce09910486578ed0c14505f33021cf97d69a7a5e1e88` |
| `crates/kontor-jira/src/connector.rs` | `882afc25a83bbfcca73d5d34cb4e85da3a1acc93761bf37d0d3245e803b043d6` |
| `crates/kontor-jira/src/jira.rs` | `b7e8a2c1cb87e63441e367311164aa498394c7fdab1c44f72e39b141e0bcf2d3` |
| `docs/evidence/ASMA-8116/GATE-9-REPAIR-REPORT.md` | `977228c9f27e2a99e76a6c87c7f7713344eeca4fef3607d676b855acc760f45c` |

## Independent command and test record

All executable checks used the isolated exact candidate archive and an isolated
`CARGO_TARGET_DIR`. Wiremock targets were rerun with local loopback permission
after sandbox-only port denials; only the permitted reruns are counted below.

| Command or target | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy -p kontor-store -p kontor-jira -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | passed |
| `cargo test -p kontor-store --test jira_materialization` | passed, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | 57/58; sole failure is the ledgered OQ-002 `DatabaseBusy`; v95 passed |
| `cargo test -p kontor-jira` | passed, unit 9/9 + native connector 19/19 |
| `cargo test -p kontor-api` | passed, library 23/23 + error envelope 5/5 + OpenAPI 3/3 |
| `cargo test -p kontor-daemon --lib` | passed, 81/81 |
| daemon `loopback_api` Jira filter | passed, 9; 1 predeclared ignored |
| distinct focused Transition/NoOp/Conflict/current-key/rebind/classification/task targets | passed, 12/12 |
| restored V18 non-writing targets | passed together, 2/2 |
| restored V17 write-boundary target | passed, 1/1 |
| independent mutation executions | **4/4 killed**, each 0/1 at its intended assertion |

The 12 distinct focused targets comprise both new non-writing outcomes, the
write-boundary V17 case, normal epic transition rename, superseded-key
exclusion, two initial epic rebind refusals, current-key content conflict,
permanent/transient classification, task same-ID rename, task different-ID
refusal, and task preview/apply current-key behavior.

## Inherited ASMA-8123 comparison

`an_epic_placeholder_body_is_typed_reported_and_repairable` failed 0/1 at both
the exact candidate and its exact parent `67aa816`. Both executions returned
HTTP 503 with code `unavailable`, rule
`the configured native Jira connector could not answer`, and action
`retry once the dependency answers; nothing was changed`; only the generated
realm UUID differed. The candidate's only production change is the later
same-issue rename-advance control flow, and the exact parent comparison matches
at this earlier refusal. ASMA-8123 is therefore reproduced unchanged and remains
inherited/external to F-8116-V18; it is not a release blocker for this repair.

## Preserved contracts and attribution

- Legacy `JiraItemCode`, native naming-token and hash semantics are unchanged.
  Explicit legacy codes remain usable; omission does not mint or infer one from
  a Jira key.
- Both task and epic A→B→C→A authority and storage-monotonic
  `rename_sequence` guards pass in the 30-test materialization suite.
- OQ-002 remains open and was reproduced as the sole schema failure. This
  repair has no store or migration delta; the v95 regression passed, so this
  run neither blames nor clears migration 0095.
- OQ-004 remains the operational placement/template gap for subjects without a
  legacy code. Both V18 fixtures supply an explicit code, so their results are
  not attributable to OQ-004.
- No ASMA-8117 file was edited; post-ASMA-8116 migration renumbering/rebase
  ownership is unchanged.
- Content-conflict current-key projection, typed permanent/transient identity
  classification and all prior write-boundary behavior remain green.

## Kontor recording and canonical runtime positions

Supported Kontor reads succeeded:

- `kontor_task_get`: HTTP 200 at snapshot cursor `2972`; task revision `2`,
  state `in_progress`, current phase `high-implementation`, prior
  `high-verification-gate = rejected`.
- `kontor_session_timeline_get` for the preserved verifier AgentRun
  `01a09541-637c-7883-ad90-6c8e0d51ebee`: HTTP 200, anchor
  `01a09541-637c-7883-ad90-6c8e0d51ebef:1:49`, timeline epoch `1`, end sequence
  `49`.

That timeline ends on 2026-09-12, carries no current round-10 user message or
terminal response, and its message items expose no `message_id`. It therefore
cannot supply the exact current message/response proof required by
`kontor_turn_settle`. A gate pass also cannot cite an unsettled round-10 report
artifact. No evaluator account, role or runtime position was guessed, so no
Kontor gate/turn write was attempted. The exact canonical fallback positions
above are recorded as requested.

No release-blocking finding remains at
`04941eb9c260c76dd1573b09d88e564d29bc4728`. This verifier report is the only
intended local worktree change.
