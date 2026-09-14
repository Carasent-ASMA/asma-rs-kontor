# ASMA-8116 high-verification report — round 9

Date: 2026-09-13
Artifact: `high-verification-report-round-9`
Task: `ASMA-8116`
Phase: `high-verification`
Candidate: `429b358622146ab8d557643e4bfd0feded99287a`
Candidate tree: `c7092778e921dcf060d8b823dafda0a8e54e7041`
Candidate parent: `a93c7348096a1904de417b833999a55b7e6ef218`
Repair account: `GATE-8-REPAIR-REPORT.md`
Status: **REJECT — one release-blocking finding remains at the exact candidate**

## Verdict

F-8116-V17 is closed on the write-bearing transition path exercised by the
retained two-stage regression. The initially proved immutable issue ID is
carried separately from `observation_hash`, the write-boundary read compares it
before an effect, and the exact restored regression passes with no transition
and no premature binding advance. M17a, M17b and M17c were independently
seeded, killed and restored.

The repair is nevertheless **REJECTED**. It defers an epic same-issue rename
until after transition dry-run, but both non-writing policy outcomes return
before that point. Consequently a rename that is already converged (`NoOp`) or
policy-blocked (`Conflict`) is proved against the stored immutable Jira issue
but never advances the durable key or rename occurrence. The next resident pass
starts from the obsolete alias again.

The result is **REJECT** at
`429b358622146ab8d557643e4bfd0feded99287a`.

## Exact candidate boundary

- Before this report was added, `HEAD` and the configured upstream both resolved
  to `429b358622146ab8d557643e4bfd0feded99287a`; its parent is `a93c734` and its
  tree is `c7092778e921dcf060d8b823dafda0a8e54e7041`.
- The candidate worktree was clean on branch
  `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`.
- Review covered the complete five-file candidate delta: daemon application
  code, daemon loopback tests, Jira connector/request code and
  `GATE-8-REPAIR-REPORT.md`. `git diff --check a93c734..429b358` passes.
- The candidate delta contains no `kontor-core`, `kontor-store`, migration or
  ASMA-8117 edit.
- Executable probing and mutations ran only in the exact isolated candidate
  archive `/private/tmp/asma-8116-v9.ocAEgO/repo` with isolated target
  `/private/tmp/asma-8116-v9-target-429b358`. The real implementation files
  were not edited.
- No Jira, PR, topology, Kontor lifecycle, merge, deployment or other external
  state was changed.

## Closed portion of F-8116-V17

`ExpectedObservation.issue_id` now carries the immutable ID from the
observation against which each native write was planned. At
`JiraConnector::validate_expected`, that proved ID is compared with the ID in
the write-boundary `LiveIssue` before digest validation and before any field,
assignment or transition effect. It remains separate from
`observation_hash`, preserving the legacy digest contract.

The restored test
`a_write_boundary_rebind_is_refused_before_any_effect_or_binding_advance`
passes 1/1. Its initial alias read answers issue `901`; the later canonical-key
read answers issue `999` with identical protected mutable fields. The test
reaches the boundary, observes zero writes, leaves the key at `ASMA-1` and does
not spend a rename occurrence. This closes the external-effect race reported as
F-8116-V17 for the transition path.

## Release-blocking finding

### F-8116-V18 — deferred same-issue epic rename is lost on non-writing policy exits

Severity: **high / release-blocking**

`reconcile_jira_epic` calls `decide_jira_identity(..., advance_now: false)`.
For a proved same-issue rename this returns the canonical current key but does
not update the durable binding. The only subsequent call to
`commit_same_issue_rename` is after a transition plan has been built and its
connector dry-run has passed.

Both other policy outcomes return earlier:

- `ReconciliationOutcome::NoOp` confirms matching transition intents and
  returns `Converged` before committing the rename.
- `ReconciliationOutcome::Conflict` records the status conflict and returns
  `Blocked` before committing the rename.

An audit-only regression independently exercised the policy-conflict route:

1. The stored epic binding was key `ASMA-1`, immutable issue ID `901`.
2. The alias observation answered current key `MOVED-9`, the same immutable ID
   `901`, and unchanged protected mutable fields.
3. The epic had an explicit legacy backlog code, so OQ-004 placement was not the
   cause; the policy exited without a transition dry-run or external write.
4. The result was
   `JiraReconcileReport { task_subjects: 0, epic_subjects: 1, converged: 0,
   applied: 0, blocked: 1, renamed: 0, content_conflicts: 0 }`.
5. The outbound write log remained empty, but the durable binding still read
   `ASMA-1` rather than `MOVED-9` and the rename was not reported.

The audit-only test therefore failed 0/1 on the exact candidate, at the durable
key assertion. Static control-flow inspection establishes that `NoOp` has the
same pre-commit return. This is a durable reconciliation defect, not an
external-effect safety failure: the immutable issue proof succeeds, yet the
current key is not carried forward.

### Bounded repair requirement

Retain the F-8116-V17 write-boundary immutable-ID comparison and refusal before
every external effect. Also commit a proved same-issue epic rename on every
valid non-writing terminal outcome, including `NoOp` and typed policy conflict,
without adding an identity read. A transition-bearing path must still wait for
the existing write-boundary proof before advancing the binding. Add retained
regressions for both non-writing exits which assert the current key and rename
occurrence advance exactly once, replay does not advance again, and the write
log remains empty.

## Independent mutation and restoration record

Each supplied mutant was applied alone to the exact isolated candidate and run
against
`a_write_boundary_rebind_is_refused_before_any_effect_or_binding_advance`.

| Mutant | Independent result |
| --- | --- |
| M17a — remove the immutable-ID comparison at the write boundary | **killed, 0/1 passed**; a transition was posted |
| M17b — compare the proved ID with itself | **killed, 0/1 passed**; a transition was posted |
| M17c — advance the epic binding before boundary agreement | **killed, 0/1 passed**; the binding moved to `MOVED-9` |

After restoration, the named regression passed 1/1. Mutation score is **3/3
killed (100%)**.

The isolated sources match the exact candidate byte-for-byte after restoration:

| Source | Candidate and restored SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `56722934986c62350f9963e3af2321ae4f945e1fb329b316c63da9e946393a80` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `a1a9a63e8154b804c33adb7f6d69f3f0e45b1e11bd13cbf3e19e202faad4d305` |
| `crates/kontor-jira/src/connector.rs` | `882afc25a83bbfcca73d5d34cb4e85da3a1acc93761bf37d0d3245e803b043d6` |
| `crates/kontor-jira/src/jira.rs` | `b7e8a2c1cb87e63441e367311164aa498394c7fdab1c44f72e39b141e0bcf2d3` |

## Gate and test record

Checks independently completed on the isolated exact candidate in this verifier
seat:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| focused clippy, four affected crates, all targets, `-D warnings` | passed |
| `cargo test -p kontor-store --test jira_materialization` | passed, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | **57/58**; the sole failure is OQ-002 `DatabaseBusy`; the v95 migration test passed |
| restored exact F-8116-V17 boundary regression | passed, 1/1 |
| audit-only non-writing same-issue rename regression | **failed as required to expose F-8116-V18, 0/1** |
| supplied mutation executions | **3/3 killed**, each 0/1 at the intended assertion |

The implement handoff records these additional exact-candidate gates; they were
not needlessly repeated after the rejection was independently established:

| Recorded handoff gate | Result |
| --- | --- |
| `cargo test -p kontor-daemon --lib` | passed, 81/81 |
| `cargo test -p kontor-jira` | passed, unit 9/9 + native connector 19/19 |
| `cargo test -p kontor-api` | passed, library 23/23 + error envelope 5/5 + OpenAPI 3/3 |
| daemon `loopback_api` Jira filter | passed, 9; 1 predeclared ignored |
| reconciler / rename / rebind focused targets | passed, 7/7 |
| `an_epic_placeholder_body_is_typed_reported_and_repairable` | inherited ASMA-8123 failure, 0/1: HTTP 503 `unavailable` |

## Preserved contracts and attribution

- The repair adds no standalone identity fetch. Identity remains carried on the
  ordinary observation and the transition dry-run's ordinary write-boundary
  read. F-8116-V18 requires no new read.
- The exact candidate/restored hashes prove the only mutations were isolated
  and temporary. The real worktree was clean before this report.
- Legacy `JiraItemCode`, native naming-token and legacy hash semantics are
  unchanged; the candidate has no core/store delta.
- OQ-002 remains open and was independently reproduced here as the sole
  `schema_v1` failure. That observation neither blames nor clears migration
  0095; its own v95 migration regression passed.
- OQ-004 remains the operational placement/template gap for unplaced epics.
  The F-8116-V18 probe supplied an explicit legacy backlog code, so this finding
  is not attributable to OQ-004.
- Migration `0095_immutable_jira_issue_identity.sql`, its storage-monotonic
  task/epic rename guards and A→B→C→A occurrence authority are outside the
  candidate delta and remain covered by the passing materialization suite and
  the v95 schema regression.
- No ASMA-8117 file was edited. Its post-ASMA-8116 migration
  renumbering/rebase ownership remains unchanged.
- The inherited ASMA-8123 placeholder-body 503 is recorded from the handoff and
  is not a basis for this rejection.

Release remains blocked until F-8116-V18 is repaired on a new exact pushed
candidate. This verifier report is the only intended local worktree change.
