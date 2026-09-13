# ASMA-8116 high-verification report — round 8

Date: 2026-09-13
Artifact: `high-verification-report-round-8`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `3ee6e76f125e43b3107d2e3d1aec21c122739d9c`
Candidate tree: `3a494cb9b563a4dc56e8a89bfe36d23cd84af33c`
Candidate parent: `e8b2e64f9ecfb83304a813abb6336bc25701830e`
Repair account: `GATE-7-REPAIR-REPORT.md`
Status: **REJECT — one release-blocking finding remains at the exact candidate**

## Verdict

F-8116-V15 and F-8116-V16 are closed for the cases their retained regressions
exercise. The stable same-ID rename reaches a real write through `MOVED-9`, no
write names `ASMA-1`, and the renaming pass has the same issue-read count as its
steady replay. An initially observed different immutable issue is refused before
the binding or rename occurrence moves. M16a, M16b and M16c were independently
seeded and killed for their claimed reasons.

The repair is nevertheless **REJECTED**. The evidence used by dry-run and apply
is not bound to the immutable Jira issue ID. If the key resolves to a different
issue after the initial identity proof but before the write-boundary read, and
that issue exposes identical protected mutable fields, validation succeeds and
the different issue receives the transition. The pass blocks only after the
external effect has occurred.

The result is **REJECT** at
`3ee6e76f125e43b3107d2e3d1aec21c122739d9c`.

## Exact candidate boundary

- Before this report was added, `HEAD` and the configured upstream both resolved
  to `3ee6e76f125e43b3107d2e3d1aec21c122739d9c`; its parent is `e8b2e64` and its
  tree is `3a494cb9b563a4dc56e8a89bfe36d23cd84af33c`.
- The candidate worktree was clean on branch
  `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`.
- Review covered the complete four-file repair delta: daemon application code,
  daemon loopback tests, the native Jira connector and
  `GATE-7-REPAIR-REPORT.md`. `git diff --check e8b2e64..3ee6e76` passes.
- All executable probing and mutations ran against an exact candidate archive
  with a fresh isolated `CARGO_TARGET_DIR`. The real worktree's production
  sources were not edited.
- No Jira, PR, topology, Kontor lifecycle, gate, merge, deployment or other
  external state was changed.

## Release-blocking finding

### F-8116-V17 — expected write evidence omits the immutable Jira issue ID

Severity: **high / release-blocking**

The repaired `JiraConnector::live` correctly canonicalizes observation evidence
under the key Jira reports rather than the alias requested. Its canonical
`observation_hash` contains schema version, observed key, project and the Jira
`fields` object. The response's top-level immutable `issue_id` is parsed into
`JiraIssueIdentity`, but it is not included in that hash.

The expected write evidence is therefore also identity-blind. Native
`validate_expected` checks status, assignee, update token and
`observation_hash`; it has no expected immutable issue ID to compare with the
`LiveIssue` read immediately before dry-run or apply.

An audit-only boundary probe reproduced the consequence with this exact
sequence:

1. The initial request used old alias `ASMA-1`. Jira answered with current key
   `MOVED-9` and immutable issue ID `901`.
2. The stored binding also held issue ID `901`, so the initial same-issue proof
   succeeded and the rename advanced normally.
3. Every subsequent write-boundary read of `MOVED-9` answered with immutable
   issue ID `999`. The entire protected mutable `fields` object was otherwise
   byte-for-byte equivalent: project, status, issue type, assignee, description
   and update token were identical.
4. Because both observations hashed the same observed key and identical fields,
   dry-run/apply validation did not distinguish issue `999` from issue `901`.
5. Kontor emitted
   `POST /rest/api/3/issue/MOVED-9/transitions` to issue `999`.
6. Only afterward did the pass settle as
   `JiraReconcileReport { task_subjects: 0, epic_subjects: 1, converged: 0,
   applied: 0, blocked: 1, renamed: 1, content_conflicts: 0 }`.

The terminal report does not make the boundary safe: the wrong immutable issue
already received the transition before the later block. This violates the exact
same-external-issue and terminal anti-rebind guarantees.

The supplied regressions cover a different timing. The stable rename fixture
keeps issue ID `901` for every read. The different-issue fixture returns `999`
on the initial alias observation, so it proves refusal before a rename begins.
Neither changes the immutable ID between the successful initial proof and the
write-boundary read.

### Bounded repair requirement

Bind expected write evidence to the immutable Jira issue ID and validate that ID
against every live issue read used to authorize dry-run or apply. If it changes,
return the typed terminal anti-rebind refusal before any external field,
assignment or transition effect. Retain a two-stage regression in which the
initial alias observation proves issue `901`, the write-boundary `MOVED-9`
observation reports issue `999` with otherwise identical protected fields, and
the assertion requires zero outbound effects. Preserve the restored
zero-extra-identity-read budget; no additional preflight observation is needed
when the normal write-boundary evidence itself carries the immutable ID.

## Independent mutation record

Each supplied mutant was applied alone to the exact isolated candidate, run
against its named retained test, then restored.

| Mutant | Independent result |
| --- | --- |
| M16a — reinstate the rename-only identity observation | **killed, 0/1 passed**; the non-vacuous epic regression observed steady reads `4` versus renaming reads `5` |
| M16b — allow any initially observed issue ID through same-issue proof | **killed, 0/1 passed**; the anti-rebind regression observed `renamed = 1` instead of `0` |
| M16c — hash evidence under the requested alias | **killed, 0/1 passed**; the current-key regression observed `applied = 0`, `blocked = 1` and no write |

After restoration, the non-vacuous current-key/read-budget regression and the
pre-advance anti-rebind regression each passed 1/1. Mutation score for the
supplied round is **3/3 killed (100%)**.

Restoration hashes matched the exact candidate:

| Source | Exact-candidate and restored SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `40bd405749344242b60073129e09012edd9551e8fda502917cd5cbcc58f298f0` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `febb549bb61c04fa6241c4bfee1051719ef7c22c525458447d9a683309df60e8` |
| `crates/kontor-jira/src/connector.rs` | `f75feb3cbe2aab7785660c3ece8ae23fe2805787e20630a13ef011bdf4a55f1a` |

## Independent gate and test record

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| focused clippy, four affected crates, all targets, `-D warnings` | passed |
| `cargo test -p kontor-daemon --lib` | passed, 81/81 |
| `cargo test -p kontor-store --test jira_materialization` | passed, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | passed, 58/58 |
| `cargo test -p kontor-jira` | passed, unit 9/9 + native connector 19/19 |
| `cargo test -p kontor-api` | passed, library 23/23 + error envelope 5/5 + OpenAPI 3/3 |
| daemon `loopback_api` Jira filter | passed, 9; 1 predeclared ignored |
| daemon `the_resident_reconciler` filter | passed, 4/4 |
| exact non-vacuous epic current-key/read-budget regression | passed, 1/1 |
| exact pre-advance different-issue regression | passed, 1/1 |
| exact task rename/current-conflict/classification regressions | passed, 3/3 |
| audit-only identity-change-at-write-boundary probe | **failed as required to expose F-8116-V17**, 0/1; transition POST occurred before the later block |
| `an_epic_placeholder_body_is_typed_reported_and_repairable` | reproduced inherited failure, 0/1: HTTP 503 `unavailable` instead of 200 |

The distinct focused retained tests produced **246 passes, zero ASMA-8116 gate
test failures and one predeclared ignore**. Mutation runs and the audit-only
F-8116-V17 probe are reported separately. The ASMA-8123 placeholder-body test
still returns the inherited HTTP 503; the handoff's revert evidence attributes
it outside this repair, and it is not the basis for this rejection.

## Preserved contracts and inherited conditions

- Stable same-ID rename writes through the current key with a nonempty effect
  list, no superseded-key effect and no extra identity read.
- Initial different-ID proof remains terminal before a binding or occurrence
  moves. F-8116-V17 concerns a later write-boundary identity change.
- Legacy `JiraItemCode`, native naming-token and legacy hash semantics are
  unchanged; the corresponding core files remain byte-identical to
  `origin/master`.
- OQ-002 remains open. The fresh 58/58 schema run is one green observation and
  neither blames nor clears migration 0095.
- OQ-004 remains the operational placement/template gap. The current-key and
  F-8116-V17 probes use explicit legacy backlog codes, so this finding is not a
  placement refusal.
- Migration 0095, both storage-monotonic rename guards and A→B→C→A occurrence
  authority are unchanged and pass the fresh store suites.
- No ASMA-8117 file or implementation appears in the repair delta; recorded
  post-ASMA-8116 renumbering/rebase ownership remains unchanged.
- Earlier permanent/transient HTTP classification, current conflict DTO and
  M8/M9 write-boundary behavior are unchanged by this repair.

Release remains blocked until F-8116-V17 is corrected and the two-stage
immutable-ID change is refused before any external effect at a new exact pushed
candidate.
