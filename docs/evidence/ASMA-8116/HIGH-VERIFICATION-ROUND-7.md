# ASMA-8116 high-verification report — round 7

Date: 2026-09-12
Artifact: `high-verification-report-round-7`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `b8e954925c95f7d3b101c6eb1da7114a137e79b8`
Candidate tree: `dd6b1861ccb70f022b11be03144db4ae7064e8a9`
Candidate parent: `3e905cfd6a91dd1432e058fbd769bfc20988a990`
Repair account: `GATE-6-REPAIR-REPORT.md`
Status: **REJECT — two release-blocking findings remain at the exact candidate**

## Verdict

The stable same-ID epic fixture now reaches a real current-key write, emits at
least one `MOVED-9` path and emits no `ASMA-1` path. M15 is independently killed
for the claimed reason. This closes the immediate hash-mismatch symptom from
F-8116-V14 for a rename whose identity remains stable across the repair's
additional observation.

The repair is not releasable. First, it explicitly adds the identity read that
ASMA-8116 previously removed and froze as a zero-extra-read invariant. Second,
the durable rename is committed before the new observation proves that the
current key still names the same immutable issue. When that proof fails, the
code stops writes but leaves the binding advanced to the rejected key. The new
anti-rebind regression does not exercise this sequence, and a mutant removing
the refreshed issue-id comparison survives it.

The result is **REJECT** at
`b8e954925c95f7d3b101c6eb1da7114a137e79b8`.

## Exact candidate boundary

- Before this report was added, `HEAD` and the configured upstream both resolved
  to `b8e954925c95f7d3b101c6eb1da7114a137e79b8`; its parent is `3e905cf` and its
  tree is `dd6b1861ccb70f022b11be03144db4ae7064e8a9`.
- The candidate worktree was clean on branch
  `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`.
- Review covered the complete three-file repair delta: daemon application code,
  daemon loopback tests and `GATE-6-REPAIR-REPORT.md`. `git diff --check
  3e905cf..b8e9549` passes.
- All executable probing and mutations ran in an exact archive under
  `/private/tmp` with a fresh isolated target. Both changed source files were
  restored byte-identically and compare equal to the real exact candidate.
- No production source in the real worktree was modified by verification. No
  Jira, PR, topology, Kontor lifecycle, gate, merge or deployment state was
  changed.

## Release-blocking findings

### F-8116-V15 — the frozen zero-extra-read invariant is regressed

Severity: **high / release-blocking**

ASMA-8116 already rejected and withdrew a second identity request. The retained
contract states that identity rides the status observation already in hand and
that a renaming pass and steady pass have equal issue-read counts. The round-5
audit's required correction repeated that the current-key epic write must reuse
the original observation and preserve zero extra reads. The round-6 verifier
required the same result without a rename-specific read.

This candidate intentionally changes that contract. After committing a rename,
`reconcile_jira_epic` calls `current_delegation.observe()` and its edited
resident regression now asserts
`replay_reads + 1 == reads_for_one_pass`. The exact restored test passes, so it
positively proves one additional issue GET in the rename pass rather than the
required zero. Because `JiraConnector::live` also fetches transitions and the
principal, the additional observation is three Jira HTTP GETs even though the
fixture counter names only the issue GET.

The handoff report labels this a changed read budget and a price of matching
evidence. An implementation handoff cannot relax a frozen acceptance condition;
the explicit disclosure makes the regression attributable, not acceptable.

Required correction: restore identity reconciliation to the normal observation
budget. Evidence and the proved current address must be represented consistently
without a rename-only `observe()`. Retain the original equality assertion
between rename and steady issue-read counts and the non-vacuous real-write
assertions.

### F-8116-V16 — failed refreshed proof leaves the durable binding advanced

Severity: **high / release-blocking**

The refresh proof is ordered after the durable mutation:

1. the old-key observation reports immutable issue `901` at current key
   `MOVED-9`;
2. `decide_jira_identity` calls the store reconciliation and commits the binding
   move from `ASMA-1` to `MOVED-9`;
3. only then does `current_delegation.observe()` read `MOVED-9` and compare its
   response with issue `901`;
4. if the refreshed answer is a different issue, the function returns Blocked
   without undoing the already committed rename.

An audit-only two-stage responder proved the exact boundary. Its `ASMA-1`
request returned key `MOVED-9`, id `901`; its subsequent `MOVED-9` request
returned key `MOVED-9`, id `999`. Reconciliation correctly reported
`applied = 0` and `blocked = 1`, but
`confirmed_jira_epic_key(project_id, epic_id)` returned `MOVED-9`, not the
required unchanged `ASMA-1`. Thus the stored current key is the very address
whose refreshed immutable identity was rejected. The stop precedes an external
write, but it does not preserve authoritative binding state.

The checked-in regression
`an_epic_key_that_moved_to_another_issue_is_still_refused_after_the_refresh`
does not model its name or comment. `RenamedEpicJira { issue_id: "999" }`
returns `999` for the *first* old-key observation as well as any later read.
Initial identity processing therefore returns `DifferentIssue` before a rename,
before `renamed: true`, and before the refresh. Its assertions that
`report.renamed == 0`, the binding remains `ASMA-1`, and writes are empty cover
the pre-existing initial anti-rebind path only.

A supplemental M16 mutant removed the refreshed `issue_id` comparison while
retaining the current-key comparison. The purported after-refresh regression
still passed 1/1. This survived mutation independently confirms that the new
test never exercises the proof it claims to protect.

Required correction: do not make the new key authoritative until every proof
required for that decision has succeeded, or otherwise ensure a failed refresh
cannot leave the ledger advanced. Retain a two-stage responder that returns the
same immutable issue on the old-alias observation and a different immutable
issue on the current-key observation, asserts the old binding remains
authoritative, asserts zero writes, and kills removal of the refreshed-ID check.

## Mutation and probe record

| Check | Exact mutation/probe | Result |
| --- | --- | --- |
| M15 | change the refresh match from `renamed: true` to `renamed: false` | **killed, 0/1 passed**; real-write assertion observed `applied: 0`, `blocked: 1`, empty writes |
| M15 restored | exact non-vacuous current-key epic regression | passed, 1/1 |
| supplemental M16 | remove refreshed immutable-ID equality, retain current-key equality | **survived, 1/1 passed** in the claimed after-refresh anti-rebind test |
| two-stage rebind probe | old alias proves `901` at `MOVED-9`; current-key refresh returns `999` | **failed as required to expose F-8116-V16**, 0/1; stored key was `MOVED-9` instead of `ASMA-1` |

Restoration hashes:

| Source | Exact-candidate and restored SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `ad721c18300cd915b5ef7ecfbe6e83aa179198781ba3739801b3ac09e163de64` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `14cdc4e59fdd87f9fe4ab71fd7dfd2cc6a17cfa359d28614712acdd2b46a1bad` |

`git diff --no-index` between each restored isolated file and its real
exact-candidate counterpart was empty. The claimed M15 score is **1/1 killed**;
the broader independently tested repair score is **1/2 killed**, with M16
surviving.

## Independent gate and test record

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| focused clippy, four affected crates, all targets, `-D warnings` | passed |
| `cargo test -p kontor-daemon --lib` | passed, 81/81 |
| `cargo test -p kontor-store --test jira_materialization` | passed, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | passed, 58/58 |
| `cargo test -p kontor-jira` | passed, unit 9/9 + native 19/19 |
| `cargo test -p kontor-api` | passed, library 23/23 + error envelope 5/5 + OpenAPI 3/3 |
| daemon `loopback_api` Jira filter | passed, 9; 1 predeclared ignored |
| daemon `the_resident_reconciler` filter | passed, 4/4, including the assertion that proves the extra read |
| exact non-vacuous epic current-key regression | passed, 1/1 |
| checked-in refreshed anti-rebind regression | passed, 1/1, but does not reach refresh |
| exact task rename/current-conflict/classification regressions | passed, 3/3 |
| `an_epic_placeholder_body_is_typed_reported_and_repairable` | reproduced inherited failure, 0/1: HTTP 503 `unavailable` instead of 200 |

The distinct focused retained tests produced **246 passes, zero ASMA-8116 gate
test failures and one predeclared ignore**. Mutation runs and the audit-only
two-stage probe are reported separately. The placeholder-body failure is an
ASMA-8123 regression. The handoff records the identical failure with this repair
reverted against the prior executable candidate; this verification independently
reproduced it at the new head. It is not the basis for this rejection and remains
a reproduced, unattributed inherited failure rather than being presented as
green.

The store suite confirms that the round-6 report's earlier shared-target failure
was contamination from M11 rather than candidate behavior. The exact rewind
test and complete materialization suite are green in the fresh target.

## Preserved contracts and inherited conditions

- Stable same-ID epic rename now reaches a nonempty current-key write, and M15
  proves the refresh branch is necessary for this implementation. No write in
  that restored regression names the superseded `ASMA-1` address.
- Legacy `JiraItemCode`, native naming-token and hash semantics are unchanged.
  `crates/kontor-core/src/backlog_identity.rs` and
  `crates/kontor-core/src/naming.rs` remain byte-identical to `origin/master`.
- OQ-002 remains open and correctly characterized as intermittent. This round's
  fresh 58/58 schema run is one observation; it neither blames nor clears
  migration 0095.
- OQ-004 remains the operational placement/template gap for subjects without an
  active immutable legacy code. The current-key regression supplies an explicit
  legacy code; neither rejection finding is attributable to OQ-004.
- Migration `0095_immutable_jira_issue_identity.sql`, its task/epic monotonic
  guards and the A→B→C→A occurrence-authority behavior are unchanged by this
  repair and pass the fresh store suites.
- No ASMA-8117 file or implementation appears in the three-file repair delta.
  The recorded post-ASMA-8116 migration renumbering/rebase responsibility is
  unchanged.
- Earlier permanent/transient HTTP classification, current conflict DTO and
  M8/M9 write-boundary behavior are unchanged by this repair.

Release remains blocked until F-8116-V15 and F-8116-V16 are corrected and both
the zero-extra-read and two-stage anti-rebind regressions pass at a new exact
pushed candidate.
