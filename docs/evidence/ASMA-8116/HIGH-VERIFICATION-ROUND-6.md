# ASMA-8116 high-verification report — round 6

Date: 2026-09-12
Artifact: `high-verification-report-round-6`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `560db2c4581fbc9dea581f20e71d008ce55d48cb`
Candidate tree: `1604115f7afd8e3edd01a3f9faf468c42828e706`
Candidate parent: `a04a85a479eee4ba173913800447c1f9c2f4c97e`
Repair account: `GATE-5-AUDIT-REPAIR-REPORT.md`
Status: **REJECT — one release-blocking finding remains at the exact candidate**

## Verdict

The repair closes the stale-object selector exercised by M14: after a same-ID
epic rename, `write_key_for` selects the post-observation current key and the
epic code rebuilds its write-bearing delegation with that key. It nevertheless
does not complete a real current-key write in the rename pass.

An independent valid-route probe reached the provisional intent and final
intent, then the native Jira dry run returned
`Conflict { operation: "apply", kind: IncompatibleHumanMove }`. The ledger had
already advanced from `ASMA-1` to `MOVED-9`, the report counted one renamed and
one blocked subject, and no write was emitted. This is a release blocker because
the audit exit criterion requires the real epic effect to address `MOVED-9` in
that pass, with no stale-key effect and no rename-only read.

The result is **REJECT** at
`560db2c4581fbc9dea581f20e71d008ce55d48cb`.

## Exact candidate boundary

- Before this report was added, `HEAD` and the configured upstream both resolved
  to `560db2c4581fbc9dea581f20e71d008ce55d48cb`; its parent is `a04a85a` and its
  tree is `1604115f7afd8e3edd01a3f9faf468c42828e706`.
- The candidate worktree was clean on branch
  `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`.
- The repair delta is four files: daemon application code, daemon loopback tests
  and two ASMA-8116 evidence documents. `git diff --check a04a85a..560db2c`
  passes.
- The supplied `HIGH-AUDIT-ROUND-5.md` SHA-256 is
  `bc538b15eb39a2214ecc30498bf6b4570237d340d130f52395dcec665b0d80cc`,
  matching the repair report's chain-of-custody statement.
- No production source in the real worktree was modified by verification. The
  probe and mutation ran in an exact archive under `/private/tmp` and were
  restored before the report was written. No Jira, PR, topology, Kontor
  lifecycle, merge or deployment state was changed.

## Release-blocking finding

### F-8116-V14 — current-key dry run invalidates its own pre-rename observation

Severity: **high / release-blocking**

The repaired epic path observes through the entry key, decides identity, then
rebuilds `JiraIssueDelegation` under the response's current key. That is the
correct address selection, but the native connector's evidence representation
makes the original observation unusable at the new address:

1. `JiraConnector::live(issue_key)` computes `observation_hash` from a canonical
   document whose `key` is the *requested* `issue_key`, not the top-level key in
   Jira's response.
2. The first observation requests `ASMA-1`. Jira's response proves the same
   immutable issue is now `MOVED-9`, but the retained observation hash still
   contains `ASMA-1`.
3. The repair rebuilds the dry-run delegation under `MOVED-9`, correctly avoiding
   the old address. Native `execute` calls `live(MOVED-9)` and therefore computes
   the otherwise identical live hash with `MOVED-9` in the canonical document.
4. `validate_expected` compares that hash with the original hash and returns
   `IncompatibleHumanMove` before `apply_effects` can emit the transition.

The independent probe altered only the exact candidate archive. Its functional
fixture changes assigned an explicit legacy epic code and offered the next
configured DRAFT workflow route (`10236`, `TO BE GROOMED`); temporary diagnostic
instrumentation recorded the boundary reached. Pure policy selected a
transition and both intent constructions succeeded. Native dry run then
produced the conflict above. The resulting report was
`JiraReconcileReport { task_subjects: 0, epic_subjects: 1, converged: 0,
applied: 0, blocked: 1, renamed: 1, content_conflicts: 0 }`; the recorded write
list was empty.

This also disproves the repair report's attribution of the missing effect to
OQ-004 for this regression. OQ-004 remains real for unplaced epics without an
immutable legacy code, but the probe supplied that code and reached the native
write boundary. The remaining stop is observation-hash alias drift, not
placement.

The checked-in end-to-end test does not detect this. It permits an empty write
list and asserts only that every emitted path omits `ASMA-1`; an empty list
satisfies that assertion. Its offered direct transition from DRAFT to
`10214`/In Development is not the next configured epic route. Even after the
fixture is given an explicit legacy code and the valid `10236` route, the hash
failure above remains.

Required correction: make evidence from an old-key request that resolves to a
proved current key remain valid when the current-key delegation performs its
normal boundary validation, without adding a rename-specific read. Retain an
end-to-end regression that requires at least one real write, requires every
write path to name `MOVED-9`, refuses every `ASMA-1` write, and compares rename
and steady-pass read counts. The exact representation change is an
implementation decision; hashing the proved response key consistently is one
candidate, but this report does not prescribe it without a retained boundary
test.

## M14 independent mutation record

The mutation workspace was the exact candidate archive, not the real worktree.
Only the `IdentityDecision::Proceed` arm of `write_key_for` was changed to return
the entry key.

| Mutant | Exact retained test | Result |
| --- | --- | --- |
| M14 stale selector | `applications::tests::a_same_issue_rename_selects_the_current_key_for_every_write` | **killed, 0/1 passed**: observed `ASMA-1` instead of `MOVED-9` |

After restoration the exact test passed 1/1. The restored files were
byte-identical to the exact candidate:

| Source | Exact-candidate and restored SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `7772b2edafdc547c1cea5f0c1e3f3339bdaecda4df5bd4ffee9c3a3a85007a0a` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `f673c13363d3a0a4ec7fb7a4b901c59b6c0d101b564951e618f8f43f6acca132` |

`git diff --no-index` between both restored isolated files and their real
exact-candidate counterparts was empty. Mutation score for this repair is
**1/1 killed (100%)**. M14 proves the selector seam; it does not discriminate
the native observation-hash boundary that causes F-8116-V14.

## Independent gate and test record

All Rust commands ran against an exact candidate archive with a fresh isolated
`CARGO_TARGET_DIR`, except the initial diagnostic described in the next section.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| affected-crate clippy, all targets, `-D warnings` | passed |
| `cargo test -p kontor-daemon --lib` | passed, 81/81 |
| `cargo test -p kontor-store --test jira_materialization` | passed, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | passed, 58/58 |
| `cargo test -p kontor-jira` | passed, unit 9/9 + native 19/19 |
| `cargo test -p kontor-api` | passed, library 23/23 + error envelope 5/5 + OpenAPI 3/3 |
| daemon `loopback_api` Jira filter | passed, 9; 1 predeclared ignored |
| daemon `the_resident_reconciler` filter | passed, 4/4 |
| checked-in same-ID epic rename E2E | passed, 1/1, but vacuously permits zero writes |
| exact task rename/current-conflict/classification regressions | passed, 3/3 |
| audit-only valid-route same-ID epic probe | **failed as required to expose F-8116-V14**, 0/1; current-key dry run returned `IncompatibleHumanMove`, zero writes |

The focused retained commands produced **245 passing tests, zero retained-test
failures and one predeclared ignore**. The audit-only probe is reported
separately because its stronger required-effect assertion is the release
blocker, not part of the checked-in suite.

The previous round's console generation/typecheck passes, unrelated inherited
299/300 DeepSeek display-fixture failure and expected MCP OQ-004
`placement_blocked` result remain applicable: the repair delta changes no
console, MCP or placement source. They were not recharacterized as new evidence
for this candidate.

## Shared-target false positive and exact attribution

The repair report records `jira_materialization` as 29/30 because
`a_rewound_rename_sequence_cannot_reactivate_spent_task_authority` appeared to
accept a forbidden rewind. The same failure initially reproduced here when the
real worktree's shared `target/` directory was reused. It is **not a candidate
store failure**.

The preceding verifier turn built M11 from an isolated source archive while
pointing Cargo at that shared target. M11 removes the task monotonic trigger.
Although its source was restored byte-identically, the shared target retained a
store artifact compiled from the mutant. Cargo dependency metadata used paths
that allowed the separate source roots to collide in that target cache. The
exact `560db2c` archive, compiled into a fresh target directory, passes the
formerly failing exact test 1/1, the complete materialization suite 30/30 and
`schema_v1` 58/58.

This resolves the repair report's contradictory observation and attributes it
to verifier-environment cache state, not migration 0095 or candidate code. The
real shared target was left in place and was not cleaned or destructively
altered.

## Preserved contracts and inherited conditions

- Legacy `JiraItemCode`, native naming-token and hash semantics are unchanged.
  `crates/kontor-core/src/backlog_identity.rs` and
  `crates/kontor-core/src/naming.rs` are byte-identical to `origin/master`.
- OQ-002 remains open and correctly characterized as intermittent. This round's
  fresh 58/58 schema run is one observation; it neither blames nor clears
  migration 0095. The resolved shared-target M11 contamination above is a
  separate deterministic event and does not settle OQ-002.
- OQ-004 remains the operational placement/template gap for subjects without an
  active immutable legacy code. It does not explain F-8116-V14 because the
  independent probe supplied the explicit code and reached native dry run.
- Migration `0095_immutable_jira_issue_identity.sql` and its task/epic
  monotonic guards are unchanged by this repair and pass both fresh store
  suites. ASMA-8116 retains branch-local 0095 ownership.
- No ASMA-8117 file or implementation appears in the four-file repair delta.
  ASMA-8117's competing migration must still be renumbered or rebased after
  ASMA-8116 according to the recorded integration sequence; this report does
  not perform that integration.
- The earlier permanent/transient HTTP classification, current conflict DTO,
  A→B→C→A occurrence authority, M8/M9 write-boundary and zero-extra-read task
  results remain unchanged by this narrow daemon epic-path repair.

Release remains blocked until F-8116-V14 is corrected and the real valid-route
epic transition is proved at a new exact pushed candidate.
